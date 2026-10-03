//! Source conversion runs on IO; validation/publication stays on the editor owner.
use crate::android::{app, error, fail, read, string};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jboolean, jbyteArray, jint, jlong, jstring},
};
use layer_core::color::ColorProfile;
use layer_host::tasks::SourceTask;
type Task = crate::inspection::Task<SourceTask>;
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sourceTask(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    id: jint,
    cancel: jlong,
) -> jlong {
    crate::inspection::capture_task(&mut env, handle, id, cancel, SourceTask::capture)
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
        let t = unsafe { crate::inspection::borrow::<Task>(handle) };
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
        let t = unsafe { crate::inspection::borrow::<Task>(job) };
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
        let t = unsafe { crate::inspection::borrow::<Task>(handle) };
        crate::inspection::on_worker("capy-source", "Source comparison worker failed", move || {
            t.task.compare(t.control.clone())?;
            serde_json::to_string(&t.task.details_localized(&*crate::launch::active_localization()?)?).map_err(error)
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
    crate::color_edit::preview_bytes(&mut env, unsafe { crate::inspection::borrow::<Task>(handle) }.task.previews(), after)
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
        let t = unsafe { crate::inspection::borrow::<Task>(job) };
        t.task.adopt(&mut a.host, t.control.is_cancelled(), || true)
    })();
    fail(&mut env, result)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sourceFree(_: JNIEnv, _: JClass, handle: jlong) {
    unsafe { crate::inspection::release::<Task>(handle) };
}
