//! Source conversion runs on IO; validation/publication stays on the editor owner.
use crate::android::{app, error, fail, read, string};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jboolean, jbyteArray, jint, jlong, jstring},
};
use layer_core::{
    Project,
    color::{ColorProfile, source::SourceImage},
};
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotGpu, SnapshotPreview};
use layer_ui::SourceWorkflow;
use std::sync::Arc;
struct Task {
    workflow: SourceWorkflow,
    candidate: Option<Project>,
    converted: Option<Arc<SourceImage>>,
    gpu: SnapshotGpu,
    control: CaptureControl,
    background: [f32; 4],
    time: f32,
    generation: u64,
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
        let workflow = SourceWorkflow::begin(s, id as u32)?;
        let gpu = s
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or("Canvas unavailable")?
            .snapshot_gpu();
        Ok(Box::into_raw(Box::new(Task {
            workflow,
            candidate: None,
            converted: None,
            gpu,
            control: crate::inspection::control(cancel),
            background: s.engine().view().background_rgba_linear,
            time: s.engine().animation_time(),
            generation: a.gpu_generation,
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
        let (source, clipped) = t.workflow.prepare(profile, 512 * 1024 * 1024, || t.control.is_cancelled())?;
        t.clipped = clipped;
        t.converted = Some(source);
        Ok(())
    })();
    fail(&mut env, result)
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
        let s = &a.host.session;
        let source = t
            .converted
            .clone()
            .ok_or("Source conversion is not ready")?;
        t.candidate = Some(t.workflow.preview(s, source, t.control.is_cancelled(), a.gpu_generation == t.generation)?);
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
            for p in [&t.workflow.project,candidate] {
                let mut snapshot=t.gpu.capture(p.clone(),t.background,t.time,Default::default(),t.control.clone()).map_err(error)?;
                t.previews.push(snapshot.preview_document([512,384],layer_core::color::RgbSpace::Srgb)?);
            }
            t.workflow.comparison_completed()?;
            serde_json::to_string(&serde_json::json!({"clipped_channels":t.clipped,"adds_layer":t.workflow.adds_layer(),
                "source_profile":layer_color::profile_description(&t.workflow.original.interpretation.profile)?})).map_err(error)
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
        let s = &mut a.host.session;
        let previous = s.state().revision;
        t.workflow.commit(s, t.control.is_cancelled(), a.gpu_generation == t.generation)?;
        let mut change = s.complete_document_request(t.workflow.identity.request(), Ok(true))?;
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
