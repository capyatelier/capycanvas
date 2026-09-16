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
            serde_json::to_string(&serde_json::json!({"epoch":job.epoch,"revision":revision,"histogram":histogram,"sampled_time":sampled_time})).map_err(error)
        }).map_err(error)?.join().map_err(|_| "Histogram worker failed".to_string())?
    })();
    string(&mut env, result)
}
