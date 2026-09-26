//! One window's drawing collection. Swift owns native contacts and serial/worker
//! scheduling; Rust owns membership, parking, admission and activation validity.
use super::*;
use layer_host::{GpuContext, Renderer, RendererOptions};
use layer_render_wgpu::WgpuRasterizer;
use layer_ui::{DocumentSessions, UiSession};
use serde_json::{Value, json};
use std::sync::Mutex;

pub(crate) type Sessions = DocumentSessions<Box<UiSession<Renderer>>>;
struct Activation {
    selected: u64,
    epoch: u64,
    context: Option<GpuContext>,
    options: RendererOptions,
    color: layer_core::color::DocumentColor,
    retired: Option<Box<WgpuRasterizer>>,
    renderer: Option<Box<WgpuRasterizer>>,
    tiles: Vec<layer_core::raster_storage::RetainedTiles>,
    budget: usize,
    storage_only: bool,
    error: Option<CString>,
    storage_error: Option<String>,
}
pub struct CapyDocumentTask(Mutex<Activation>);
impl CapyApple {
    pub(crate) fn document_session(&self, id: u64) -> Result<&UiSession<Renderer>, String> {
        if id == self.documents.selected() {
            Ok(&self.host.session)
        } else {
            self.documents
                .parked()
                .find(|(key, _)| **key == id)
                .map(|(_, p)| p.owner.as_ref())
                .ok_or_else(|| "Drawing tab is no longer open".into())
        }
    }
    pub(crate) fn retire_document_gpu(&mut self) -> Option<Box<WgpuRasterizer>> {
        self.metal.document_changed();
        self.dismissed_contacts.clear();
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
    pub(crate) fn tabs_view(&self, width: f32) -> Value {
        json!({"tabs":self.documents.labels(&self.host.session.state().document_file,|s|&s.state().document_file),
            "selected":self.documents.selected(),"compact":layer_ui::DocumentTabs::compact(width,self.documents.order().len()),
            "can_undo":self.documents.can_undo(),"can_redo":self.documents.can_redo(),
            "resident_bytes":self.documents.resident_bytes(),"storage_error":self.documents.storage_error(),
            "parked_renderers":self.documents.parked().filter(|(_,p)|p.owner.engine().backend().0.is_some()).count()})
    }
    pub(crate) fn tabs_request(&mut self, value: Value) -> Result<Value, String> {
        #[derive(serde::Deserialize)]
        #[serde(tag = "op", rename_all = "snake_case")]
        enum Request {
            View {
                #[serde(default)]
                width: f32,
            },
            Recovery {
                id: u64,
            },
            Ready,
            Open,
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
            Budget {
                bytes: usize,
            },
            ResetClose,
        }
        let request: Request = serde_json::from_value(value).map_err(|e| e.to_string())?;
        Ok(match request {
            Request::View { width } => self.tabs_view(width),
            Request::Open => {
                if self.host.session.state().document_file.busy
                    || !self
                        .host
                        .session
                        .command(layer_ui::CommandId::OpenDocument)
                        .enabled
                {
                    json!(false)
                } else {
                    self.host.dispatch(layer_ui::UiAction::Invoke {
                        command: layer_ui::CommandId::OpenDocument,
                    })?;
                    json!(true)
                }
            }
            Request::Recovery { id } => {
                let s = self.document_session(id)?;
                let mut document = s.recovery_document();
                // New/Open keeps this drawing alive. Its pending file dialog
                // must not prevent the outgoing drawing's recovery barrier.
                if !s.state().requests.is_empty()
                    && s.state().requests.iter().all(|r| {
                        matches!(
                            r.kind,
                            layer_ui::HostRequestKind::Document {
                                request: layer_ui::DocumentRequest::New
                                    | layer_ui::DocumentRequest::Open
                            }
                        )
                    })
                {
                    document.busy = s.capture_project_recovery().is_err();
                }
                json!(document)
            }
            Request::Ready => json!(
                self.host.session.can_park_document()
                    && self
                        .host
                        .session
                        .retained_document_tiles()
                        .try_blobs()?
                        .is_some()
            ),
            Request::Adjacent { forward } => json!(self.documents.adjacent(forward)),
            Request::Drop {
                hits,
                point,
                vertical,
            } => json!(
                self.documents
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
                self.documents
                    .drag(id, press, &hits, clip)
                    .and_then(|drag| drag.preview(point))
            ),
            Request::Budget { bytes } => {
                self.documents.budget.inactive_ram =
                    bytes.min(layer_ui::DocumentBudget::default().inactive_ram);
                Value::Null
            }
            Request::ResetClose => {
                self.host.session.reset_document_close();
                let ids: Vec<_> = self.documents.parked().map(|(id, _)| *id).collect();
                for id in ids {
                    self.documents
                        .parked_owner_mut(id)
                        .unwrap()
                        .reset_document_close();
                }
                self.tabs_changed();
                Value::Null
            }
            other => {
                if !self.host.session.can_park_document() {
                    return Err("Finish the current operation before reordering drawings".into());
                }
                match other {
                    Request::Reorder { id, before } => {
                        self.documents.reorder(id, before);
                    }
                    Request::Step { id, forward } => {
                        if let Some(before) = self.documents.step(id, forward) {
                            self.documents.reorder(id, before);
                        }
                    }
                    Request::History { redo } => {
                        if redo {
                            self.documents.redo()
                        } else {
                            self.documents.undo()
                        }
                    }
                    _ => unreachable!(),
                }
                self.tabs_changed();
                Value::Null
            }
        })
    }
    fn document_job(
        &self,
        retired: Option<Box<WgpuRasterizer>>,
        storage_only: bool,
    ) -> *mut CapyDocumentTask {
        Box::into_raw(Box::new(CapyDocumentTask(Mutex::new(Activation {
            selected: self.documents.selected(),
            epoch: self.host.session.state().document_file.epoch,
            context: self.document_gpu.clone(),
            options: self.host.renderer_options(self.metal.cache.clone()),
            color: self.host.session.engine().document().color,
            retired,
            renderer: None,
            tiles: self
                .documents
                .parked()
                .map(|(_, p)| p.tiles.clone())
                .collect(),
            budget: self.documents.budget.inactive_ram,
            storage_only,
            error: None,
            storage_error: None,
        }))))
    }
}
/// # Safety
/// Serial owner. Flush native input and wait for every retained undo raster,
/// not just the current recovery snapshot, before retiring the GPU.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_document_prepare_switch(app: *mut CapyApple, now: u64) -> i32 {
    let ready = unsafe { capy_apple_prepare_recovery(app, now) };
    if ready != 0 {
        return ready;
    }
    let Some(a) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    a.perform(|a| {
        let session = &a.host.session;
        let backed = session.rendering_suspended()
            || session.state().document_file.close_ready
            || session.retained_document_tiles().try_blobs()?.is_some();
        Ok(i32::from(!session.can_park_document() || !backed))
    })
    .unwrap_or(-1)
}
/// # Safety
/// Serial owner only. The returned job must be prepared on a worker and freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_document_switch(
    app: *mut CapyApple,
    id: u64,
    closing: bool,
) -> *mut CapyDocumentTask {
    let Some(a) = (unsafe { app.as_mut() }) else {
        return std::ptr::null_mut();
    };
    a.perform(|a| {
        if closing && !a.host.session.state().document_file.close_ready {
            return Err("Confirm closing the drawing first".into());
        }
        let target = if closing {
            a.documents.after_close().unwrap_or(0)
        } else {
            id
        };
        if !closing && target == a.documents.selected() {
            return Ok(a.document_job(None, true));
        }
        if !closing && !a.documents.contains_parked(target) {
            return Err("Drawing tab is no longer open".into());
        }
        if !a.host.session.can_park_document() {
            return Err("Finish the current operation before switching drawings".into());
        }
        if target != 0 {
            a.documents
                .parked_owner_mut(target)
                .ok_or("Drawing tab is no longer open")?
                .inherit_window_state(&a.host.session)?;
        }
        let tiles = a.host.session.park_document()?;
        let retired = a.retire_document_gpu();
        if closing {
            if let Some(mut next) = a.documents.close_selected() {
                std::mem::swap(&mut a.host.session, next.as_mut());
            }
        } else {
            a.documents.exchange_with(target, tiles, |next| {
                std::mem::swap(&mut a.host.session, next.as_mut())
            })?;
        }
        a.tabs_changed();
        a.host.startup = Default::default();
        Ok(a.document_job(retired, false))
    })
    .unwrap_or(std::ptr::null_mut())
}
/// # Safety
/// Serial owner only; schedules inactive backing I/O without changing selection.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_document_storage(app: *mut CapyApple) -> *mut CapyDocumentTask {
    unsafe { app.as_ref() }.map_or(std::ptr::null_mut(), |a| a.document_job(None, true))
}
/// # Safety
/// Worker only; directory is UTF-8, job stays alive and has no concurrent caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_document_prepare(
    task: *mut CapyDocumentTask,
    directory: *const c_char,
) -> i32 {
    let Some(task) = (unsafe { task.as_ref() }) else {
        return -1;
    };
    let mut job = task.0.lock().unwrap_or_else(|e| e.into_inner());
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
        drop(job.retired.take());
        let path = unsafe { project::read_title(directory) }?;
        for tiles in &job.tiles {
            if layer_core::raster_storage::resident_tile_bytes(job.tiles.iter()) <= job.budget {
                break;
            }
            if let Err(e) =
                layer_core::raster_storage::spill_to_directory(tiles, std::path::Path::new(path))
            {
                job.storage_error = Some(e);
                break;
            }
        }
        if job.storage_only || job.selected == 0 {
            return Ok(());
        }
        let context = job
            .context
            .as_ref()
            .ok_or("The window GPU is unavailable; restart the canvas")?;
        job.renderer = Some(context.rasterizer(job.color, &job.options, true)?.into());
        Ok(())
    }))
    .unwrap_or_else(|_| Err("Drawing activation failed".into()));
    match result {
        Ok(()) => 0,
        Err(e) => {
            job.error = CString::new(e.replace('\0', " ")).ok();
            -1
        }
    }
}
/// # Safety
/// Serial owner after worker completion; preserves the selected document on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_document_resume(
    app: *mut CapyApple,
    task: *mut CapyDocumentTask,
) -> i32 {
    let (Some(a), Some(task)) = (unsafe { app.as_mut() }, unsafe { task.as_ref() }) else {
        return -1;
    };
    a.perform(|a| {
        let mut job = task.0.lock().unwrap_or_else(|e| e.into_inner());
        a.documents
            .storage_completed(job.storage_error.clone().map_or(Ok(()), Err));
        if !job.storage_only
            && (job.selected != a.documents.selected()
                || job.epoch != a.host.session.state().document_file.epoch)
        {
            return Err("Drawing activation is no longer current".into());
        }
        if let Some(e) = &job.error {
            return Err(e.to_string_lossy().into_owned());
        }
        if !job.storage_only && job.selected != 0 {
            let gpu = job
                .renderer
                .take()
                .ok_or("Drawing renderer is not prepared")?;
            a.metal.install_renderer(&mut a.host, gpu)?;
            a.host.error = None;
        }
        if job.storage_only {
            a.host.invalidate_snapshot();
        } else {
            a.tabs_changed();
        }
        Ok(())
    })
    .map_or(-1, |_| 0)
}
/// # Safety
/// No outstanding job calls. Resource destruction belongs on the file worker.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_document_free(task: *mut CapyDocumentTask) {
    if !task.is_null() {
        drop(unsafe { Box::from_raw(task) });
    }
}
