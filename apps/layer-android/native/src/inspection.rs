//! Immutable inspection jobs and separately owned cancellation handles. JNI
//! never borrows a running worker's mutable Task to cancel it.
use crate::android::{app, error, fail, string};
use jni::{
    JNIEnv,
    objects::JClass,
    sys::{jlong, jstring},
};
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotGpu};

pub(crate) fn control(handle: jlong) -> CaptureControl {
    if handle == 0 {
        CaptureControl::default()
    } else {
        unsafe { &*(handle as *const CaptureControl) }.clone()
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_captureControl(_: JNIEnv, _: JClass) -> jlong {
    Box::into_raw(Box::new(CaptureControl::default())) as jlong
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_captureCancel(
    _: JNIEnv,
    _: JClass,
    handle: jlong,
) {
    control(handle).cancel();
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_captureFree(_: JNIEnv, _: JClass, handle: jlong) {
    if handle != 0 {
        drop(unsafe { Box::from_raw(handle as *mut CaptureControl) });
    }
}

struct Inspection {
    project: layer_core::Project,
    gpu: SnapshotGpu,
    background: [f32; 4],
    time: f32,
    epoch: u64,
    control: CaptureControl,
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_inspectionTask(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    cancel: jlong,
) -> jlong {
    let result = (|| {
        let a = unsafe { app(handle) };
        let session = &a.host.session;
        session.require_document_snapshot_idle()?;
        let project = session.capture_project_recovery()?;
        let gpu = session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or("Canvas unavailable")?
            .snapshot_gpu();
        Ok(Box::into_raw(Box::new(Inspection {
            project,
            gpu,
            background: session.engine().view().background_rgba_linear,
            time: session.engine().animation_time(),
            epoch: session.state().document_file.epoch,
            control: control(cancel),
        })) as jlong)
    })();
    match result {
        Ok(task) => task,
        Err(e) => {
            fail(&mut env, Err(e));
            0
        }
    }
}

/// Consumes the private job. The Kotlin caller invokes this exactly once on IO.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_inspectionHistogram(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) -> jstring {
    let job = unsafe { Box::from_raw(handle as *mut Inspection) };
    let result = (|| {
        std::thread::Builder::new().name("capy-inspection".into()).stack_size(8 * 1024 * 1024).spawn(move || {
            let revision = job.project.document.revision;
            let sampled_time = job.project.document.has_animated_effects().then_some(job.time);
            let mut renderer = job.gpu.capture(job.project, job.background, job.time, Default::default(), job.control).map_err(error)?;
            let histogram = renderer.histogram().map_err(error)?;
            serde_json::to_string(&serde_json::json!({"epoch":job.epoch,"revision":revision,"axis":histogram.axis(),"histogram":histogram,"sampled_time":sampled_time})).map_err(error)
        }).map_err(error)?.join().map_err(|_| "Histogram worker failed".to_string())?
    })();
    string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentInfoTask(
    _: JNIEnv,
    _: JClass,
    handle: jlong,
) -> jlong {
    let a = unsafe { app(handle) };
    Box::into_raw(Box::new(layer_color::DocumentInfo::capture(
        a.host.session.engine().document(),
    ))) as jlong
}
/// Consumes the small metadata job on IO, including profile parsing.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentInfo(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) -> jstring {
    let info = unsafe { Box::from_raw(handle as *mut layer_color::DocumentInfo) };
    string(
        &mut env,
        info.describe()
            .and_then(|rows| serde_json::to_string(&rows).map_err(error)),
    )
}

/// Consumes one immutable inspection job on IO, like histogram capture.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_inspectionOutput(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    recipe: jni::objects::JString,
) -> jni::sys::jobjectArray {
    let job = unsafe { Box::from_raw(handle as *mut Inspection) };
    let result = (|| {
        let recipe: layer_ui::ExportRecipe =
            serde_json::from_str(&crate::android::read(&mut env, &recipe)?).map_err(error)?;
        recipe.validate()?;
        let (previews,stats)=std::thread::Builder::new().name("capy-output-preview".into()).stack_size(8*1024*1024).spawn(move || {
            let extent=recipe.size.extent([job.project.document.width,job.project.document.height])?;
            let mut renderer=job.gpu.capture(job.project,job.background,job.time,Default::default(),job.control).map_err(error)?;
            let before=renderer.preview_document([512,384],layer_core::color::RgbSpace::Srgb)?;
            renderer.set_output_extent(extent)?;
            let mut previews = vec![before];
            let statistics = if let Some(format) = recipe.format.gainmap() {
                let (hdr, sdr, stats) = renderer.preview_gainmap_output(
                    [512,384], layer_core::color::RgbSpace::Srgb, 1., format,
                    recipe.jpeg_quality, recipe.background.matte(),
                )?;
                previews.extend([hdr, sdr]);
                stats
            } else {
                let (after, stats) = if recipe.format == layer_ui::ExportFormat::Exr {
                    (renderer.preview_document([512,384],layer_core::color::RgbSpace::Srgb)?,layer_color::OutputStatistics::default())
                } else if recipe.format.is_hdr() {
                    renderer.preview_hdr_output([512,384],layer_core::color::RgbSpace::Srgb,1.)?
                } else {
                    renderer.preview_output([512,384],layer_core::color::RgbSpace::Srgb,&recipe.interpretation(),recipe.encoding,recipe.background.matte())?
                };
                previews.push(after);
                stats
            };
            Ok::<_,String>((previews,serde_json::json!({"extent":extent,"clipped_channels":statistics.clipped_channels}).to_string()))
        }).map_err(error)?.join().map_err(|_|"Output preview worker failed".to_string())??;
        let result = env
            .new_object_array(previews.len() as i32 + 1, "java/lang/Object", jni::objects::JObject::null())
            .map_err(error)?;
        let stats = env.new_string(stats).map_err(error)?;
        env.set_object_array_element(&result, 0, stats)
            .map_err(error)?;
        for (i, preview) in previews.iter().enumerate() {
            let bytes = env
                .byte_array_from_slice(&preview_bytes(preview)?)
                .map_err(error)?;
            env.set_object_array_element(&result, i as i32 + 1, bytes)
                .map_err(error)?;
        }
        Ok(result.into_raw())
    })();
    match result {
        Ok(result) => result,
        Err(e) => {
            fail(&mut env, Err(e));
            std::ptr::null_mut()
        }
    }
}
pub(crate) fn preview_bytes(
    preview: &layer_render_wgpu::snapshot::SnapshotPreview,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(8 + preview.pixels.len() * 4);
    for v in preview.extent {
        bytes.extend_from_slice(&v.to_le_bytes())
    }
    bytes.extend(preview.srgb_bytes()?);
    Ok(bytes)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_captureCancelled(_: JNIEnv, _: JClass, control: jlong) -> jni::sys::jboolean {
    self::control(control).is_cancelled() as jni::sys::jboolean
}
