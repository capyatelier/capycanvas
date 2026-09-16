//! Source conversion runs on IO; validation/publication stays on the editor owner.
use crate::android::{app, error, fail, read, string};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jboolean, jbyteArray, jint, jlong, jstring},
};
use layer_core::{
    LayerId, Project,
    color::{ColorProfile, source::SourceImage},
};
use layer_render::CanvasRenderer;
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotGpu, SnapshotPreview};
use layer_ui::{DocumentRequest, HostRequestKind};
use std::sync::Arc;
struct Task {
    project: Project,
    candidate: Option<Project>,
    original: Arc<SourceImage>,
    converted: Option<Arc<SourceImage>>,
    gpu: SnapshotGpu,
    control: CaptureControl,
    background: [f32; 4],
    time: f32,
    epoch: u64,
    revision: u64,
    generation: u64,
    request: u32,
    layer: LayerId,
    rasterize: bool,
    baked: bool,
    clipped: u64,
    previews: Vec<SnapshotPreview>,
}
unsafe fn task<'a>(handle: jlong) -> &'a mut Task {
    unsafe { &mut *(handle as *mut Task) }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sourceTask(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    id: jint,
    cancel: jlong,
) -> jlong {
    let result = (|| {
        let a = unsafe { app(handle) };
        let s = &a.host.session;
        s.require_document_idle()?;
        let (layer, rasterize) = s
            .state()
            .requests
            .iter()
            .find_map(|r| {
                if r.id == id as u32 {
                    match &r.kind {
                        HostRequestKind::Document {
                            request: DocumentRequest::RepairSourceProfile { layer },
                        } => Some((LayerId(*layer), false)),
                        HostRequestKind::Document {
                            request: DocumentRequest::RasterizeSource { layer },
                        } => Some((LayerId(*layer), true)),
                        _ => None,
                    }
                } else {
                    None
                }
            })
            .ok_or("Source request is no longer active")?;
        let project = s.capture_project_recovery()?;
        let l = project
            .document
            .layer(layer)
            .ok_or("Source layer no longer exists")?;
        let original = l.source.clone().ok_or("No retained source")?;
        let baked = !l.raster.is_empty() || !l.pending_operations.is_empty() || l.asset.is_some();
        let gpu = s
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or("Canvas unavailable")?
            .snapshot_gpu();
        Ok(Box::into_raw(Box::new(Task {
            revision: project.document.revision,
            project,
            candidate: None,
            original,
            converted: None,
            gpu,
            control: crate::inspection::control(cancel),
            background: s.engine().view().background_rgba_linear,
            time: s.engine().animation_time(),
            epoch: s.state().document_file.epoch,
            generation: a.gpu_generation,
            request: id as u32,
            layer,
            rasterize,
            baked,
            clipped: 0,
            previews: Vec::new(),
        })) as jlong)
    })();
    match result {
        Ok(h) => h,
        Err(e) => {
            fail(&mut env, Err(e));
            0
        }
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sourceWork(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    profile: JString,
) {
    let result = (|| {
        let profile: Option<ColorProfile> =
            serde_json::from_str(&read(&mut env, &profile)?).map_err(error)?;
        let t = unsafe { task(handle) };
        if t.control.is_cancelled() {
            return Err("Source change cancelled".into());
        }
        let source = if t.rasterize {
            if profile.is_some() {
                return Err("Rasterization uses the document profile".into());
            }
            let (source, statistics) = layer_color::rasterize_source(
                &t.original,
                t.project.document.color,
                512 * 1024 * 1024,
                || t.control.is_cancelled(),
            )?;
            t.clipped = statistics.clipped_channels;
            source
        } else {
            let mut source = (*t.original).clone();
            source.interpretation.profile = profile.ok_or("Choose a source profile")?;
            source.interpretation.profile_assumed = false;
            layer_color::WorkingDecoder::new(
                &source.interpretation,
                t.project.document.color.space,
                Default::default(),
            )?;
            source.validate()?;
            source
        };
        if t.control.is_cancelled() {
            return Err("Source change cancelled".into());
        }
        t.converted = Some(Arc::new(source));
        Ok(())
    })();
    fail(&mut env, result)
}
fn current(a: &crate::app::App, t: &Task) -> Result<(), String> {
    let s = &a.host.session;
    if t.control.is_cancelled()
        || a.gpu_generation != t.generation
        || s.state().document_file.epoch != t.epoch
        || s.engine().document().revision != t.revision
        || !s.state().requests.iter().any(|r| r.id == t.request)
    {
        return Err("The document or canvas changed; prepare the source change again".into());
    }
    Ok(())
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sourcePrepareComparison(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    job: jlong,
) {
    let result = (|| {
        let a = unsafe { app(handle) };
        let t = unsafe { task(job) };
        current(a, t)?;
        let s = &a.host.session;
        let source = t
            .converted
            .clone()
            .ok_or("Source conversion is not ready")?;
        t.candidate = Some(if t.rasterize {
            s.preview_rasterized_source(t.layer, &t.original, source)?
        } else {
            s.preview_layer_source(t.layer, &t.original, (*source).clone())?
        });
        Ok(())
    })();
    fail(&mut env, result)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sourceCompare(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) -> jstring {
    let result = (|| {
        let t = unsafe { task(handle) };
        std::thread::scope(|scope| {
            std::thread::Builder::new().name("capy-source".into()).stack_size(8*1024*1024).spawn_scoped(scope,||{
            t.previews.clear();let candidate=t.candidate.as_ref().ok_or("Comparison is not prepared")?;
            for p in [&t.project,candidate] {
                let mut snapshot=t.gpu.capture(p.clone(),t.background,t.time,Default::default(),t.control.clone()).map_err(error)?;
                t.previews.push(snapshot.preview_document([512,384],layer_core::color::RgbSpace::Srgb)?);
            }
            serde_json::to_string(&serde_json::json!({"clipped_channels":t.clipped,"adds_layer":t.baked&&!t.rasterize,
                "source_profile":layer_color::profile_description(&t.original.interpretation.profile)?})).map_err(error)
        }).map_err(error)?.join().map_err(|_|"Source comparison worker failed".to_string())?
        })
    })();
    string(&mut env, result)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sourcePreview(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    after: jboolean,
) -> jbyteArray {
    let result = (|| {
        let p = unsafe { task(handle) }
            .previews
            .get(usize::from(after != 0))
            .ok_or("Comparison is unavailable")?;
        let mut bytes = Vec::with_capacity(8 + p.pixels.len() * 4);
        for v in p.extent {
            bytes.extend_from_slice(&v.to_le_bytes())
        }
        bytes.extend(p.srgb_bytes()?);
        env.byte_array_from_slice(&bytes)
            .map(|a| a.into_raw())
            .map_err(error)
    })();
    match result {
        Ok(b) => b,
        Err(e) => {
            fail(&mut env, Err(e));
            std::ptr::null_mut()
        }
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sourceAdopt(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    job: jlong,
) {
    let result = (|| {
        let a = unsafe { app(handle) };
        let t = unsafe { task(job) };
        current(a, t)?;
        if t.previews.len() != 2 {
            return Err("Preview the complete source result first".into());
        }
        let source = t.converted.clone().ok_or("Source candidate is missing")?;
        let s = &mut a.host.session;
        let previous = s.state().revision;
        if t.rasterize {
            s.apply_rasterized_source(t.layer, &t.original, source)?
        } else {
            s.repair_layer_source(t.layer, &t.original, (*source).clone())?;
        }
        let mut change = s.complete_document_request(t.request, Ok(true))?;
        change.canvas_wake = true;
        a.host.apply_change(previous, change);
        t.converted = None;
        Ok(())
    })();
    fail(&mut env, result)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sourceFree(_: JNIEnv, _: JClass, handle: jlong) {
    if handle != 0 {
        drop(unsafe { Box::from_raw(handle as *mut Task) })
    }
}
