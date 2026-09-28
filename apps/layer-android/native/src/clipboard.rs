//! Pixel copies: frozen on the render Looper, composed and encoded on a file
//! worker. The window keeps the clip; Kotlin shares its PNG through a
//! FileProvider URI whose clip description carries the nonce.
use crate::android::{app, error, fail, read, string};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jboolean, jint, jlong, jstring},
};
use layer_host::clipboard::ClipTask;
use layer_ui::{DocumentRequest, HostRequestKind, PixelClip};

unsafe fn task<'a>(handle: jlong) -> &'a ClipTask {
    unsafe { &*(handle as *const ClipTask) }
}
unsafe fn clip<'a>(handle: jlong) -> &'a PixelClip {
    unsafe { &*(handle as *const PixelClip) }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_clipTask(mut env: JNIEnv, _: JClass, handle: jlong, id: jint) -> jlong {
    let a = unsafe { app(handle) };
    match ClipTask::capture(&mut a.host.session, id as u32) {
        Ok(task) => Box::into_raw(Box::new(task)) as jlong,
        Err(e) => {
            fail(&mut env, Err(e));
            0
        }
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_clipTaskLarge(_: JNIEnv, _: JClass, handle: jlong) -> jboolean {
    jboolean::from(unsafe { task(handle) }.capture_details().large)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_clipTaskProgress(mut env: JNIEnv, _: JClass, handle: jlong) -> jstring {
    string(&mut env, Ok(unsafe { task(handle) }.capture_details().progress.into()))
}
/// Worker: consumes the task and returns the finished clip.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_clipRun(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    control: jlong,
    nonce: JString,
) -> jlong {
    let task = unsafe { Box::from_raw(handle as *mut ClipTask) };
    let result = read(&mut env, &nonce).and_then(|nonce| task.run(nonce, crate::inspection::control(control)));
    match result {
        Ok(clip) => Box::into_raw(Box::new(clip)) as jlong,
        Err(e) => {
            fail(&mut env, Err(e));
            0
        }
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_clipTaskFree(_: JNIEnv, _: JClass, handle: jlong) {
    if handle != 0 {
        drop(unsafe { Box::from_raw(handle as *mut ClipTask) });
    }
}
/// Worker: write the clip's PNG for other applications.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_clipWritePng(mut env: JNIEnv, _: JClass, handle: jlong, path: JString) {
    let result = read(&mut env, &path).and_then(|path| std::fs::write(path, &unsafe { clip(handle) }.png[..]).map_err(error));
    fail(&mut env, result);
}
/// Keep the clip for the window and complete its copy, which erases a Cut.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_clipAdopt(mut env: JNIEnv, _: JClass, handle: jlong, id: jint, clip: jlong) {
    let clip = unsafe { Box::from_raw(clip as *mut PixelClip) };
    let result = (|| {
        let a = unsafe { app(handle) };
        a.window.documents.clip = Some(*clip);
        let previous = a.host.session.state().revision;
        let mut change = a.host.session.complete_document_request(id as u32, Ok(true))?;
        change.canvas_wake = true;
        a.host.apply_change(previous, change);
        Ok(())
    })();
    fail(&mut env, result);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_clipFree(_: JNIEnv, _: JClass, handle: jlong) {
    if handle != 0 {
        drop(unsafe { Box::from_raw(handle as *mut PixelClip) });
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_clipNonce(mut env: JNIEnv, _: JClass, handle: jlong) -> jstring {
    let a = unsafe { app(handle) };
    match &a.window.documents.clip {
        Some(clip) => string(&mut env, Ok(clip.nonce.clone())),
        None => std::ptr::null_mut(),
    }
}
/// Answer the pending Paste request `id` with the window's clip.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_pasteClip(mut env: JNIEnv, _: JClass, handle: jlong, id: jint) {
    let result = (|| {
        let a = unsafe { app(handle) };
        let mode = a
            .host
            .session
            .state()
            .requests
            .iter()
            .find_map(|r| match &r.kind {
                HostRequestKind::Document { request: DocumentRequest::Paste { mode } } if r.id == id as u32 => Some(*mode),
                _ => None,
            })
            .ok_or("The paste request is no longer active")?;
        let clip = a.window.documents.clip.clone().ok_or("Nothing was copied in this window")?;
        let previous = a.host.session.state().revision;
        a.host.session.paste_clip(&clip, mode)?;
        let mut change = a.host.session.complete_document_request(id as u32, Ok(true))?;
        change.canvas_wake = true;
        change.regions |= 255;
        a.host.apply_change(previous, change);
        Ok(())
    })();
    fail(&mut env, result);
}
