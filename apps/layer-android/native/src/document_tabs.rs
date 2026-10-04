//! Android scheduling around the shared drawing collection. Parked editors have
//! no renderer. One bounded worker drains retirement before activation.
use crate::{
    android::{app, error, fail, or_throw, read, string},
    app::App,
};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jlong, jstring},
};
use layer_host::{
    Renderer,
    window::{Activation, DocumentWindow, TabRequest, PreparedClose},
};
use layer_ui::UiSession;

pub(crate) type Window = DocumentWindow<UiSession<Renderer>>;
impl App {
    pub(crate) fn document_retired(&mut self) {
        self.tone.clear();
        self.host.proof = Default::default();
        // Navigator placements belong to the window, not the retiring document.
        // The host publishes them again only when layout changes.
        self.cursor = Default::default();
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentTabs(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    request: JString,
) -> jstring {
    let result = (|| {
        let request: TabRequest =
            serde_json::from_str(&read(&mut env, &request)?).map_err(error)?;
        let a = unsafe { app(handle) };
        Ok(a.window.request(&mut a.host, request)?.to_string())
    })();
    string(&mut env, result)
}
/// Render owner: select atomically, then transfer retired resources to one job.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentSwitch(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    id: jlong,
    close: jni::sys::jboolean,
) -> jlong {
    let result = (|| {
        let a = unsafe { app(handle) };
        let options = a
            .host
            .renderer_options(Some(a.cache_directory.clone().into()));
        let Some((activation, _)) =
            a.window
                .switch(&mut a.host, id as u64, close != 0, options, |_| {})?
        else {
            return Ok(0);
        };
        a.document_retired();
        a.blank_presented = false;
        Ok(Box::into_raw(Box::new(activation)) as jlong)
    })();
    or_throw(&mut env, result, 0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentPrepareClose(mut env: JNIEnv, _: JClass, handle: jlong) -> jlong {
    let result = (|| {
        let a = unsafe {app(handle)};
        let options = a.host.renderer_options(Some(a.cache_directory.clone().into()));
        a.window.prepare_close(&mut a.host, options).map(|job|Box::into_raw(Box::new(job)) as jlong)
    })();
    or_throw(&mut env,result,0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentCommitClose(_: JNIEnv, _: JClass, handle: jlong, job: jlong) -> jlong {
    let a = unsafe {app(handle)};
    let prepared = *unsafe {Box::from_raw(job as *mut PreparedClose)};
    let (activation,_) = a.window.commit_close(&mut a.host,prepared,|_|{});
    a.document_retired();a.blank_presented=false;
    Box::into_raw(Box::new(activation)) as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentCancelPreparedClose(mut env: JNIEnv, _: JClass, handle: jlong, job: jlong) {
    let a = unsafe {app(handle)};
    let prepared = *unsafe {Box::from_raw(job as *mut PreparedClose)};
    fail(&mut env,a.window.cancel_close(&mut a.host,prepared))
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentResumeWork(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) {
    let job = unsafe { &mut *(handle as *mut Activation) };
    fail(&mut env, job.work());
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentResume(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    task: jlong,
) {
    let result = (|| {
        let a = unsafe { app(handle) };
        let job = unsafe { &mut *(task as *mut Activation) };
        if let Some(gpu) = a.window.resume(&a.host, job)? {
            let previous = a.host.session.state().revision;
            let (_, change) = a.host.session.replace_renderer(Renderer(Some(gpu)))?;
            a.host.apply_change(previous, change);
            a.project_adopted();
            a.host.startup = Default::default();
            a.host.error = None;
        }
        a.window.changed(&mut a.host);
        Ok(())
    })();
    fail(&mut env, result)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentResumeFree(
    _: JNIEnv,
    _: JClass,
    handle: jlong,
) {
    if handle != 0 {
        drop(unsafe { Box::from_raw(handle as *mut Activation) })
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentSpillTask(
    _: JNIEnv,
    _: JClass,
    handle: jlong,
) -> jlong {
    let a = unsafe { app(handle) };
    a.window
        .documents
        .spill_candidate()
        .map(|tiles| Box::into_raw(Box::new(tiles)) as jlong)
        .unwrap_or(0)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentSpillWork(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) {
    let tiles = unsafe { Box::from_raw(handle as *mut layer_core::raster_storage::RetainedTiles) };
    fail(
        &mut env,
        layer_core::temp_files::directory()
            .and_then(|directory| layer_core::raster_storage::spill_to_directory(&tiles, directory)),
    );
}
