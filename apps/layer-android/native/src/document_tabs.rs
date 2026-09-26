//! Android scheduling around the shared drawing collection. Parked editors have
//! no renderer. One bounded worker drains retirement before activation.
use crate::{
    android::{app, error, fail, read, string},
    app::App,
};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jlong, jstring},
};
use layer_host::{GpuContext, Renderer, RendererOptions};
use layer_render_wgpu::WgpuRasterizer;
use layer_ui::{DocumentSessions, UiSession};
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) type Sessions = DocumentSessions<UiSession<Renderer>>;
struct Activation {
    selected: u64,
    generation: u64,
    context: Option<GpuContext>,
    color: layer_core::color::DocumentColor,
    options: RendererOptions,
    retired: Option<Box<WgpuRasterizer>>,
    renderer: Option<Box<WgpuRasterizer>>,
}
impl App {
    pub(crate) fn document_session(&self, id: u64) -> Result<&UiSession<Renderer>, String> {
        if id == self.documents.selected() {
            Ok(&self.host.session)
        } else {
            self.documents
                .parked()
                .find(|(key, _)| **key == id)
                .map(|(_, p)| &p.owner)
                .ok_or_else(|| "Drawing tab is no longer open".into())
        }
    }
    pub(crate) fn document_park_ready(&self) -> Result<bool, String> {
        let session = &self.host.session;
        if !session.can_park_document() {
            return Ok(false);
        }
        if session.rendering_suspended() || session.state().document_file.close_ready {
            return Ok(true);
        }
        Ok(session.retained_document_tiles().try_blobs()?.is_some())
    }
    pub(crate) fn retire_document_gpu(&mut self) -> Option<Box<WgpuRasterizer>> {
        if let Some(control) = self.tone.pending.take() {
            control.cancel();
        }
        self.tone = Default::default();
        self.proof = Default::default();
        // Navigator placements belong to the window, not the retiring document.
        // The host publishes them again only when layout changes.
        self.cursor = Default::default();
        let renderer = self.host.session.renderer_mut().0.take();
        if let Some(gpu) = &renderer {
            self.document_gpu = Some(GpuContext::of(gpu));
        }
        renderer
    }
    pub(crate) fn tabs_changed(&mut self) {
        self.host.document_count = self.documents.order().len();
        self.host.document_adopted();
        self.host.invalidate_snapshot();
    }
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    View {
        #[serde(default)]
        width: f32,
    },
    Ready,
    Recovery {
        id: u64,
    },
    Adjacent {
        forward: bool,
    },
    Reorder {
        id: u64,
        before: Option<u64>,
    },
    Step {
        id: u64,
        forward: bool,
    },
    History {
        redo: bool,
    },
    Drop {
        hits: Vec<layer_ui::DocumentTabHit>,
        point: [f32; 2],
        vertical: bool,
    },
    Slide {
        id: u64,
        hits: Vec<layer_ui::DocumentTabHit>,
        clip: layer_ui::Bounds,
        press: [f32; 2],
        point: [f32; 2],
    },
    Storage {
        error: Option<String>,
    },
    Budget {
        bytes: usize,
    },
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentTabs(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    request: JString,
) -> jstring {
    let result = (|| {
        let request: Request = serde_json::from_str(&read(&mut env, &request)?).map_err(error)?;
        let a = unsafe { app(handle) };
        let value = match request {
            Request::View { width } => {
                json!({"tabs":a.documents.labels(&a.host.session.state().document_file,|s|&s.state().document_file),"selected":a.documents.selected(),"compact":layer_ui::DocumentTabs::compact(width,a.documents.order().len()),"can_undo":a.documents.can_undo(),"can_redo":a.documents.can_redo(),"resident_bytes":a.documents.resident_bytes(),"parked_renderers":a.documents.parked().filter(|(_,p)|p.owner.engine().backend().0.is_some()).count(),"gpu_generation":a.gpu_generation,"storage_error":a.documents.storage_error()})
            }
            Request::Ready => {
                json!({"available":a.host.session.can_park_document(),"park":a.document_park_ready()?,"close":a.host.session.command(layer_ui::CommandId::CloseDocument).enabled,"approved":a.host.session.state().document_file.close_ready})
            }
            Request::Recovery { id } => json!(a.document_session(id)?.recovery_document()),
            Request::Adjacent { forward } => json!(a.documents.adjacent(forward)),
            Request::Drop {
                hits,
                point,
                vertical,
            } => json!(
                a.documents
                    .drop_target(&hits, point, vertical)
                    .map(|before| json!({"before":before}))
            ),
            Request::Slide {
                id,
                hits,
                clip,
                press,
                point,
            } => json!(
                a.documents
                    .drag(id, press, &hits, clip)
                    .and_then(|drag| drag.preview(point))
            ),
            Request::Storage { error } => {
                a.documents.storage_completed(error.map_or(Ok(()), Err));
                Value::Null
            }
            Request::Budget { bytes } => {
                a.documents.budget.inactive_ram =
                    bytes.min(layer_ui::DocumentBudget::default().inactive_ram);
                Value::Null
            }
            other => {
                if !a.host.session.can_park_document() {
                    return Err("Finish the current operation before reordering drawings".into());
                }
                match other {
                    Request::Reorder { id, before } => {
                        a.documents.reorder(id, before);
                    }
                    Request::Step { id, forward } => {
                        if let Some(before) = a.documents.step(id, forward) {
                            a.documents.reorder(id, before);
                        }
                    }
                    Request::History { redo } => {
                        if redo {
                            a.documents.redo()
                        } else {
                            a.documents.undo()
                        }
                    }
                    _ => unreachable!(),
                }
                a.tabs_changed();
                Value::Null
            }
        };
        Ok(value.to_string())
    })();
    string(&mut env, result)
}
/// Render owner: select atomically, then transfer retired resources to one job.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentSwitch(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    id: jlong,
    close: jni::sys::jboolean,
) -> jlong {
    let result = (|| {
        let a = unsafe { app(handle) };
        let closing = close != 0;
        if closing && !a.host.session.state().document_file.close_ready {
            return Err("Confirm closing the drawing first".into());
        }
        let id = if closing {
            a.documents.after_close().unwrap_or(0)
        } else {
            id as u64
        };
        if !closing && id == a.documents.selected() {
            return Ok(0);
        }
        if id != 0 {
            let next = a
                .documents
                .parked_owner_mut(id)
                .ok_or("Drawing tab is no longer open")?;
            next.inherit_window_state(&a.host.session)?;
        }
        let tiles = a.host.session.park_document()?;
        let retired = a.retire_document_gpu();
        if closing {
            if let Some(next) = a.documents.close_selected() {
                a.host.session = next;
            }
        } else {
            a.documents
                .exchange_in_place(id, &mut a.host.session, tiles)?;
        }
        a.tabs_changed();
        a.host.startup = Default::default();
        a.blank_presented = false;
        Ok(Box::into_raw(Box::new(Activation {
            selected: id,
            generation: a.gpu_generation,
            context: if id == 0 {
                None
            } else {
                a.document_gpu.clone()
            },
            color: a.host.session.engine().document().color,
            options: a
                .host
                .renderer_options(Some(a.cache_directory.clone().into())),
            retired,
            renderer: None,
        })) as jlong)
    })();
    match result {
        Ok(id) => id,
        Err(e) => {
            fail(&mut env, Err(e));
            0
        }
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentResumeWork(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) {
    let job = unsafe { &mut *(handle as *mut Activation) };
    let result = (|| {
        drop(job.retired.take());
        if job.selected == 0 {
            return Ok(());
        }
        let context = job
            .context
            .as_ref()
            .ok_or("The window GPU is unavailable; restart the canvas")?;
        job.renderer = Some(context.rasterizer(job.color, &job.options, true)?.into());
        Ok(())
    })();
    fail(&mut env, result)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentResume(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    task: jlong,
) {
    let result = (|| {
        let a = unsafe { app(handle) };
        let job = unsafe { &mut *(task as *mut Activation) };
        if job.selected != a.documents.selected() || job.generation != a.gpu_generation {
            return Err("Drawing activation is no longer current".into());
        }
        if job.selected != 0 {
            let gpu = job
                .renderer
                .take()
                .ok_or("Drawing renderer is not prepared")?;
            let previous = a.host.session.state().revision;
            let (_, change) = a.host.session.replace_renderer(Renderer(Some(gpu)))?;
            a.host.apply_change(previous, change);
            a.project_adopted();
            a.host.startup = Default::default();
            a.host.error = None;
        }
        a.tabs_changed();
        Ok(())
    })();
    fail(&mut env, result)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentResumeFree(
    _: JNIEnv,
    _: JClass,
    handle: jlong,
) {
    if handle != 0 {
        drop(unsafe { Box::from_raw(handle as *mut Activation) })
    }
}
struct Spill {
    tiles: layer_core::raster_storage::RetainedTiles,
    directory: std::path::PathBuf,
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentSpillTask(
    _: JNIEnv,
    _: JClass,
    handle: jlong,
) -> jlong {
    let a = unsafe { app(handle) };
    a.documents
        .spill_candidate()
        .map(|tiles| {
            Box::into_raw(Box::new(Spill {
                tiles,
                directory: std::path::Path::new(&a.cache_directory).join("drawing-tiles"),
            })) as jlong
        })
        .unwrap_or(0)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentSpillWork(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) {
    let job = unsafe { Box::from_raw(handle as *mut Spill) };
    fail(
        &mut env,
        layer_core::raster_storage::spill_to_directory(&job.tiles, &job.directory).map(|_| ()),
    );
}
