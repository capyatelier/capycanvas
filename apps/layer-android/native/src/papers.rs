//! Paper library requests: captured on the render owner, read and written on
//! IO, then adopted by the owner.
use crate::android::{fail, or_throw, read};
use jni::{JNIEnv, objects::{JClass, JString}, sys::{jboolean, jint, jlong}};
use layer_host::papers::PaperTask;
use layer_ui::paper_library::PaperResult;
use std::{os::fd::FromRawFd, sync::{Mutex, atomic::{AtomicBool, Ordering}}};

struct Job {
    task: PaperTask,
    cancelled: AtomicBool,
    result: Mutex<Option<Result<PaperResult, String>>>,
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_paperTask(mut env: JNIEnv, _: JClass, handle: jlong, request: jint, directory: JString) -> jlong {
    crate::jni_entry!(env, handle, capy_app, {
    let result = (|| {
        let directory = read(&mut env, &directory)?;
        let task = PaperTask::capture(&capy_app.host.session, u32::try_from(request).ok(), directory.into())?;
        crate::inspection::shared_owned(Job { task, cancelled: AtomicBool::new(false), result: Mutex::new(None) })
    })();
    or_throw(&mut env, result, 0)
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_paperImports(mut env: JNIEnv, _: JClass, job: jlong) -> jboolean {
    crate::jni_entry!(env, {
    jboolean::from(crate::inspection::shared::<Job>(job)?.task.imports())
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_paperWork(mut env: JNIEnv, _: JClass, job: jlong, fd: jint, name: JString) {
    crate::jni_entry!(env, {
    let name = read(&mut env, &name).unwrap_or_default();
    let job = crate::inspection::shared::<Job>(job)?;
    let file = (fd >= 0).then(|| unsafe { std::fs::File::from_raw_fd(fd) });
    let result = crate::inspection::on_worker("capy-paper", "Paper library worker failed",
        || job.task.work(file.as_ref().map(|file| (file, name.as_str())), &job.cancelled, crate::photo_reader::read));
    *job.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_paperApply(mut env: JNIEnv, _: JClass, handle: jlong, job: jlong) {
    crate::jni_entry!(env, handle, capy_app, {
    let result = (|| {
        let job = crate::inspection::shared::<Job>(job)?;
        let result = job.result.lock().unwrap_or_else(|e| e.into_inner()).take().ok_or("Paper library worker failed")?;
        job.task.apply(&mut capy_app.host, result, job.cancelled.load(Ordering::Relaxed))
    })();
    fail(&mut env, result)
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_paperCancel(mut env: JNIEnv, _: JClass, job: jlong) {
    crate::jni_entry!(env, {
    crate::inspection::shared::<Job>(job)?.cancelled.store(true, Ordering::Relaxed);
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_paperFree(mut env: JNIEnv, _: JClass, job: jlong) {
    crate::jni_entry!(env, {
    crate::inspection::release::<Job>(job)?;
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_paperImportTask(mut env: JNIEnv, _: JClass, handle: jlong, directory: JString) -> jlong {
    crate::jni_entry!(env, handle, capy_app, {
    let result = (|| {
        let directory = read(&mut env, &directory)?;
        let task = PaperTask::library_import(&capy_app.host.session, directory.into());
        crate::inspection::shared_owned(Job { task, cancelled: AtomicBool::new(false), result: Mutex::new(None) })
    })();
    or_throw(&mut env, result, 0)
    })
}
