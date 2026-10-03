//! Immutable inspection jobs and separately owned cancellation handles. JNI
//! never borrows a running worker's mutable Task to cancel it.
use crate::android::{app, error, or_throw, string};
use jni::{
    JNIEnv,
    objects::JClass,
    sys::{jlong, jstring},
};
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotGpu};

pub(crate) struct Task<T> {
    pub task: T,
    pub control: CaptureControl,
}
pub(crate) unsafe fn borrow<'a, T>(handle: jlong) -> &'a mut T {
    unsafe { &mut *(handle as *mut T) }
}
pub(crate) unsafe fn borrow_ref<'a, T>(handle: jlong) -> &'a T {
    unsafe { &*(handle as *const T) }
}
pub(crate) unsafe fn release<T>(handle: jlong) {
    if handle != 0 { drop(unsafe { Box::from_raw(handle as *mut T) }); }
}
pub(crate) fn capture_task<T>(env: &mut JNIEnv, handle: jlong, id: jni::sys::jint, cancel: jlong,
    capture: impl FnOnce(&layer_ui::UiSession<layer_host::Renderer>, Option<u32>, layer_core::color::RgbSpace) -> Result<T, String>) -> jlong {
    let result = capture(&unsafe { app(handle) }.host.session, Some(id as u32), layer_core::color::RgbSpace::Srgb)
        .map(|task| Box::into_raw(Box::new(Task { task, control: control(cancel) })) as jlong);
    or_throw(env, result, 0)
}
pub(crate) fn on_worker<T: Send, E: From<String> + Send>(name: &str, failed: &str, work: impl FnOnce() -> Result<T, E> + Send) -> Result<T, E> {
    std::thread::scope(|scope| std::thread::Builder::new().name(name.into()).stack_size(8 * 1024 * 1024)
        .spawn_scoped(scope, work).map_err(|value| E::from(error(value)))?.join().map_err(|_| E::from(failed.to_owned()))?)
}

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
pub extern "system" fn Java_art_capycanvas_Native_inspectionSample(
    mut env: JNIEnv, _: JClass, handle: jlong, source: jni::objects::JString,
    x: jni::sys::jfloat, y: jni::sys::jfloat, width: jni::sys::jint,
) -> jstring {
    let job = unsafe { Box::from_raw(handle as *mut Inspection) };
    let result = (|| {
        let source: layer_core::ArtworkSource = serde_json::from_str(&crate::android::read(&mut env, &source)?).map_err(error)?;
        on_worker("capy-artwork-sample", "Artwork sample worker failed", move || {
            let mut request = layer_core::ArtworkSampleRequest::new(&job.project.document, source, [x, y], width as u32);
            request.time = job.time;
            let sample = pollster::block_on(job.gpu.artwork_sample(request, job.control))?;
            serde_json::to_string(&serde_json::json!({"epoch":job.epoch,"revision":job.project.document.revision,"time":job.time,"sample":sample})).map_err(error)
        })
    })();
    string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_inspectionStatistics(
    mut env: JNIEnv, _: JClass, handle: jlong, source: jni::objects::JString,
    preview: jni::sys::jboolean, selection: jni::sys::jboolean,
) -> jstring {
    let job = unsafe { Box::from_raw(handle as *mut Inspection) };
    let result = (|| {
        let source: layer_core::ArtworkSource = serde_json::from_str(&crate::android::read(&mut env, &source)?).map_err(error)?;
        on_worker("capy-artwork-statistics", "Artwork statistics worker failed", move || {
            let mut query = layer_core::ArtworkQuery::new(&job.project.document, source);
            query.time = job.time;
            let request = layer_core::ArtworkStatisticsRequest { query, preview: preview != 0, selection: selection != 0 };
            let histogram = pollster::block_on(job.gpu.artwork_statistics(request, job.control))?;
            serde_json::to_string(&serde_json::json!({"epoch":job.epoch,"revision":job.project.document.revision,"time":job.time,"histogram":histogram})).map_err(error)
        })
    })();
    string(&mut env, result)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_inspectionLevelsStatistics(
    mut env: JNIEnv, _: JClass, handle: jlong, source: jni::objects::JString,
) -> jstring {
    let job = unsafe { Box::from_raw(handle as *mut Inspection) };
    let result = (|| {
        let source: layer_core::ArtworkSource = serde_json::from_str(&crate::android::read(&mut env, &source)?).map_err(error)?;
        on_worker("capy-levels-statistics", "Levels statistics worker failed", move || {
            let mut query = layer_core::ArtworkQuery::new(&job.project.document, source);
            query.time = job.time;
            let statistics = pollster::block_on(job.gpu.levels_statistics(query, job.control))?;
            serde_json::to_string(&serde_json::json!({"epoch":job.epoch,"revision":job.project.document.revision,"time":job.time,"statistics":statistics})).map_err(error)
        })
    })();
    string(&mut env, result)
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
    or_throw(&mut env, result, 0)
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
        crate::inspection::on_worker("capy-inspection", "Histogram worker failed", move || {
            let revision = job.project.document.revision;
            let sampled_time = job.project.document.has_animated_effects().then_some(job.time);
            let mut renderer = job.gpu.capture(job.project, job.background, job.time, job.control).map_err(error)?;
            let histogram = renderer.histogram().map_err(error)?;
            serde_json::to_string(&serde_json::json!({"epoch":job.epoch,"revision":revision,"axis":histogram.axis(),"histogram":histogram,"sampled_time":sampled_time})).map_err(error)
        })
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
        ( || { let inspected = info.inspect()?;
            let rows = layer_ui::document_properties(&inspected, &*crate::launch::active_localization()?);
            serde_json::to_string(&rows).map_err(error) })(),
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
    let result = (|| -> Result<_, crate::color_preferences::ColorCallError> {
        let recipe: layer_ui::ExportRecipe =
            serde_json::from_str(&crate::android::read(&mut env, &recipe)?).map_err(error)?;
        let (previews,stats)=crate::inspection::on_worker("capy-output-preview", "Output preview worker failed", move || {
            let mut renderer=job.gpu.capture(job.project,job.background,job.time,job.control).map_err(error)?;
            let before=renderer.preview_document([512,384],layer_core::color::RgbSpace::Srgb)?;
            let output=layer_host::export::preview_recipe(&mut renderer,[512,384],layer_core::color::RgbSpace::Srgb,1.,&recipe)?;
            let json=serde_json::json!({"extent":recipe.size.extent(renderer.extent())?,"clipped_channels":output.clipped,"range_blocked":output.range_blocked});
            Ok::<_,crate::color_preferences::ColorCallError>(([before,output.after].into_iter().chain(output.sdr_base).collect::<Vec<_>>(),json.to_string()))
        })?;
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
    crate::color_preferences::color_or_throw(&mut env, result, std::ptr::null_mut())
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
