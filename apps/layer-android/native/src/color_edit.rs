//! Color changes prepare immutable backing and a complete private GPU canvas on
//! IO. The render owner publishes renderer + document/history in one turn.
use crate::android::{app, error, fail, or_throw, read, string};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jboolean, jbyteArray, jint, jlong, jstring},
};
use layer_host::tasks::{ColorTask, Preview};

type Task = crate::inspection::Task<ColorTask>;
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_colorTask(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    id: jint,
    cancel: jlong,
) -> jlong {
    crate::inspection::capture_task(&mut env, handle, id, cancel, ColorTask::capture)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_colorWork(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    choice: JString,
    copy: jboolean,
) -> jstring {
    let result = (|| {
        let choice = serde_json::from_str(&read(&mut env, &choice)?).map_err(error)?;
        let t = unsafe { crate::inspection::borrow::<Task>(handle) };
        crate::inspection::on_worker("capy-color", "Color worker failed", move || {
            t.task.work(choice, copy != 0, t.control.clone())?;
            serde_json::to_string(&t.task.details()).map_err(error)
        })
    })();
    string(&mut env, result)
}
pub(crate) fn preview_bytes(env: &mut JNIEnv, previews: &[Preview], after: jboolean) -> jbyteArray {
    let result = (|| {
        let preview = previews
            .get(usize::from(after != 0))
            .ok_or("Comparison is unavailable")?;
        let mut bytes = Vec::with_capacity(8 + preview.pixels.len());
        for v in preview.extent {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes.extend_from_slice(&preview.pixels);
        env.byte_array_from_slice(&bytes)
            .map(|a| a.into_raw())
            .map_err(error)
    })();
    or_throw(env, result, std::ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_colorPreview(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    after: jboolean,
) -> jbyteArray {
    preview_bytes(&mut env, unsafe { crate::inspection::borrow::<Task>(handle) }.task.previews(), after)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_colorAdopt(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    job: jlong,
) {
    let result = (|| {
        let a = unsafe { app(handle) };
        let t = unsafe { crate::inspection::borrow::<Task>(job) };
        t.task
            .adopt(&mut a.host, t.control.is_cancelled(), || true)?;
        a.project_adopted();
        Ok(())
    })();
    fail(&mut env, result)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_colorFree(_: JNIEnv, _: JClass, handle: jlong) {
    unsafe { crate::inspection::release::<Task>(handle) };
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_colorWriteCopy(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    fd: jint,
) {
    use std::io::Write;
    use std::os::fd::FromRawFd;
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    let result = (|| {
        let t = unsafe { crate::inspection::borrow::<Task>(handle) };
        let mut output = std::io::BufWriter::new(file);
        t.task.write_copy(&mut output, t.control.is_cancelled())?;
        output.flush().map_err(error)?;
        if t.control.is_cancelled() {
            return Err("Converted copy cancelled".into());
        }
        Ok(())
    })();
    fail(&mut env, result)
}
