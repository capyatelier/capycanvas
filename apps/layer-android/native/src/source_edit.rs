//! Source conversion runs on IO; validation/publication stays on the editor owner.
use crate::android::{app, error, fail, read, string};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jboolean, jbyteArray, jint, jlong, jstring},
};
use layer_core::color::{ColorProfile, RgbSpace};
use layer_host::tasks::SourceTask;
use layer_render_wgpu::snapshot::CaptureControl;
struct Task {
    task: SourceTask,
    control: CaptureControl,
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
        Ok(Box::into_raw(Box::new(Task {
            task: SourceTask::capture(&a.host.session, Some(id as u32), RgbSpace::Srgb)?,
            control: crate::inspection::control(cancel),
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
        t.task.work(profile, || t.control.is_cancelled())
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
        t.task.prepare(&a.host, t.control.is_cancelled())
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
            std::thread::Builder::new()
                .name("capy-source".into())
                .stack_size(8 * 1024 * 1024)
                .spawn_scoped(scope, || {
                    t.task.compare(t.control.clone())?;
                    serde_json::to_string(&t.task.details()?).map_err(error)
                })
                .map_err(error)?
                .join()
                .map_err(|_| "Source comparison worker failed".to_string())?
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
    crate::color_edit::preview_bytes(&mut env, unsafe { task(handle) }.task.previews(), after)
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
        t.task.adopt(&mut a.host, t.control.is_cancelled(), || true)
    })();
    fail(&mut env, result)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sourceFree(_: JNIEnv, _: JClass, handle: jlong) {
    if handle != 0 {
        drop(unsafe { Box::from_raw(handle as *mut Task) })
    }
}
