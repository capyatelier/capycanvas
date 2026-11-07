//! Immutable inspection jobs and separately owned cancellation handles. JNI
//! never borrows a running worker's mutable Task to cancel it.
use crate::android::{error, or_throw, string};
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
pub(crate) fn owned<T:Send+'static>(value:T)->Result<jlong,String> {layer_host::handles::insert(value).map(|handle|handle.raw() as jlong).map_err(error)}
pub(crate) fn borrow<T:Send+'static>(handle:jlong)->Result<layer_host::handles::Borrowed<T>,String> {
    layer_host::handles::lease(layer_host::handles::Handle::from_raw(handle as u64).map_err(error)?).map_err(error)?.borrow().map_err(error)
}
pub(crate) fn borrow_ref<T:Send+'static>(handle:jlong)->Result<layer_host::handles::Borrowed<T>,String> {borrow(handle)}
pub(crate) fn take<T:Send+'static>(handle:jlong)->Result<T,String> {layer_host::handles::take(layer_host::handles::Handle::from_raw(handle as u64).map_err(error)?).map_err(error)}
pub(crate) fn release<T:'static>(handle:jlong)->Result<(),String> {if handle==0{return Ok(());}layer_host::handles::release::<T>(layer_host::handles::Handle::from_raw(handle as u64).map_err(error)?).map_err(error)}
pub(crate) fn shared_owned<T:Send+Sync+'static>(value:T)->Result<jlong,String> {layer_host::handles::insert_shared(std::sync::Arc::new(value)).map(|handle|handle.raw() as jlong).map_err(error)}
pub(crate) fn shared<T:Send+Sync+'static>(handle:jlong)->Result<std::sync::Arc<T>,String> {layer_host::handles::shared(layer_host::handles::Handle::from_raw(handle as u64).map_err(error)?).map_err(error)}
pub(crate) fn capture_task<T:Send+'static>(env: &mut JNIEnv, handle: jlong, id: jni::sys::jint, cancel: jlong,
    capture: impl FnOnce(&layer_ui::UiSession<layer_host::Renderer>, Option<u32>, layer_core::color::RgbSpace) -> Result<T, String>) -> jlong {
    let result=crate::android::with_app(handle,|app| {
        let task=capture(&app.host.session,Some(id as u32),layer_core::color::RgbSpace::Srgb)?;
        owned(Task {task,control:control(cancel)?})
    });
    or_throw(env,result,0)
}
pub(crate) fn on_worker<T: Send, E: From<String> + Send>(name: &str, failed: &str, work: impl FnOnce() -> Result<T, E> + Send) -> Result<T, E> {
    std::thread::scope(|scope| std::thread::Builder::new().name(name.into()).stack_size(8 * 1024 * 1024)
        .spawn_scoped(scope, work).map_err(|value| E::from(error(value)))?.join().map_err(|_| E::from(failed.to_owned()))?)
}

pub(crate) fn control(handle:jlong)->Result<CaptureControl,String> {
    if handle==0 {Ok(CaptureControl::default())}else{shared::<CaptureControl>(handle).map(|control|(*control).clone())}
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_captureControl(mut env: JNIEnv, _: JClass) -> jlong {
    crate::jni_entry!(env, {
    crate::inspection::shared_owned(CaptureControl::default())?
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_captureCancel(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) {
    crate::jni_entry!(env, {
    control(handle)?.cancel();
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_captureFree(mut env: JNIEnv, _: JClass, handle: jlong) {
    crate::jni_entry!(env, {
    if handle != 0 {
        crate::inspection::release::<CaptureControl>(handle)?;
    }
    })
}

struct Inspection {
    capture: layer_core::authored::ArtworkCapture,
    snapshot: std::sync::Arc<layer_core::authored::SceneSnapshot>,
    selection: Option<layer_core::Selection>,
    gpu: SnapshotGpu,
    epoch: u64,
    control: CaptureControl,
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_inspectionSample(
    mut env: JNIEnv, _: JClass, handle: jlong, source: jni::objects::JString,
    x: jni::sys::jfloat, y: jni::sys::jfloat, width: jni::sys::jint,
) -> jstring {
    crate::jni_entry!(env, {
    let job = crate::inspection::take::<Inspection>(handle)?;
    let result = (|| {
        let source = serde_json::from_str::<layer_core::SnapshotSource>(&crate::android::read(&mut env, &source)?).map_err(error)?.artwork_source();
        on_worker("capy-artwork-sample", "Artwork sample worker failed", move || {
            let request = layer_core::ArtworkSampleRequest {
                query: layer_core::ArtworkQuery::from_snapshot(job.snapshot.clone(), source, job.selection.clone()),
                position: [x, y], width: width as u32,
            };
            let sample = pollster::block_on(job.gpu.artwork_sample(request, job.control))?;
            serde_json::to_string(&serde_json::json!({"epoch":job.epoch,"revision":job.snapshot.revision,"time":job.snapshot.context.elapsed,"sample":sample})).map_err(error)
        })
    })();
    string(&mut env, result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_inspectionStatistics(
    mut env: JNIEnv, _: JClass, handle: jlong, source: jni::objects::JString,
    preview: jni::sys::jboolean, selection: jni::sys::jboolean, waveform: jni::sys::jboolean,
) -> jstring {
    crate::jni_entry!(env, {
    let job = crate::inspection::take::<Inspection>(handle)?;
    let result = (|| {
        let source = serde_json::from_str::<layer_core::SnapshotSource>(&crate::android::read(&mut env, &source)?).map_err(error)?.artwork_source();
        on_worker("capy-artwork-statistics", "Artwork statistics worker failed", move || {
            let query = layer_core::ArtworkQuery::from_snapshot(job.snapshot.clone(), source, job.selection.clone());
            let request = layer_core::ArtworkStatisticsRequest { query, preview: preview != 0, selection: selection != 0, waveform: waveform != 0 };
            let histogram = pollster::block_on(job.gpu.artwork_statistics(request, job.control))?;
            serde_json::to_string(&serde_json::json!({"epoch":job.epoch,"revision":job.snapshot.revision,"time":job.snapshot.context.elapsed,"histogram":histogram})).map_err(error)
        })
    })();
    string(&mut env, result)
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_inspectionLevelsStatistics(
    mut env: JNIEnv, _: JClass, handle: jlong, source: jni::objects::JString,
) -> jstring {
    crate::jni_entry!(env, {
    let job = crate::inspection::take::<Inspection>(handle)?;
    let result = (|| {
        let source = serde_json::from_str::<layer_core::SnapshotSource>(&crate::android::read(&mut env, &source)?).map_err(error)?.artwork_source();
        on_worker("capy-levels-statistics", "Levels statistics worker failed", move || {
            let query = layer_core::ArtworkQuery::from_snapshot(job.snapshot.clone(), source, job.selection.clone());
            let statistics = pollster::block_on(job.gpu.levels_statistics(query, job.control))?;
            serde_json::to_string(&serde_json::json!({"epoch":job.epoch,"revision":job.snapshot.revision,"time":job.snapshot.context.elapsed,"statistics":statistics})).map_err(error)
        })
    })();
    string(&mut env, result)
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_inspectionTask(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    cancel: jlong,
) -> jlong {
    crate::jni_entry!(env, handle, capy_app, {
    let result = (|| {
        let a = &mut *capy_app;
        let session = &a.host.session;
        session.require_document_snapshot_idle()?;
        let capture = session.capture_artwork()?;
        let snapshot = session.engine().scene_snapshot();
        let selection = session.engine().document().working.selection.clone();
        let gpu = session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or("Canvas unavailable")?
            .snapshot_gpu();
        crate::inspection::owned(Inspection {
            capture,
            snapshot,
            selection,
            gpu,
            epoch: session.state().document_file.epoch,
            control: control(cancel)?,
        })
    })();
    or_throw(&mut env, result, 0)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentInfoTask(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) -> jlong {
    crate::jni_entry!(env, handle, capy_app, {
    let a = &mut *capy_app;
    crate::inspection::owned(layer_color::DocumentInfo::capture(
        a.host.session.engine().document(),
    ))?
    })
}
/// Consumes the small metadata job on IO, including profile parsing.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentInfo(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) -> jstring {
    crate::jni_entry!(env, {
    let info = crate::inspection::take::<layer_color::DocumentInfo>(handle)?;
    string(
        &mut env,
        ( || { let inspected = info.inspect()?;
            let rows = layer_ui::document_properties(&inspected, &*crate::launch::active_localization()?);
            serde_json::to_string(&rows).map_err(error) })(),
    )
    })
}

/// Consumes one immutable inspection job on IO, like histogram capture.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_inspectionOutput(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    recipe: jni::objects::JString,
) -> jni::sys::jobjectArray {
    crate::jni_entry!(env, {
    let job = crate::inspection::take::<Inspection>(handle)?;
    let result = (|| -> Result<_, crate::color_preferences::ColorCallError> {
        let recipe: layer_ui::ExportRecipe =
            serde_json::from_str(&crate::android::read(&mut env, &recipe)?).map_err(error)?;
        let (previews,stats)=crate::inspection::on_worker("capy-output-preview", "Output preview worker failed", move || {
            let mut renderer=job.gpu.capture(job.capture,job.control).map_err(error)?;
            let (before,transparent)=renderer.preview_document_with_coverage([512,384],layer_core::color::RgbSpace::Srgb,1.)?;
            let output=layer_host::export::preview_recipe(&mut renderer,[512,384],layer_core::color::RgbSpace::Srgb,1.,&recipe)?;
            let json=serde_json::json!({"extent":recipe.size.extent(renderer.extent())?,"clipped_channels":output.clipped,"range_blocked":output.range_blocked,"transparent":transparent});
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
    })
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
