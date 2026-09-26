//! Android scheduling around the shared drawing collection. Parked editors have
//! no renderer. One bounded worker drains retirement before activation.
use crate::{
    android::{app, error, fail, read, string},
    app::App,
};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jlong, jstring},
};
use layer_host::{
    Renderer,
    window::{Activation, DocumentWindow, TabRequest},
};
use layer_ui::UiSession;

pub(crate) type Window = DocumentWindow<UiSession<Renderer>>;
impl App {
    pub(crate) fn document_retired(&mut self) {
        if let Some(control) = self.tone.pending.take() {
            control.cancel();
        }
        self.tone = Default::default();
        self.proof = Default::default();
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
    match result {
        Ok(id) => id,
        Err(e) => {
            fail(&mut env, Err(e));
            0
        }
    }
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
struct Spill {
    tiles: layer_core::raster_storage::RetainedTiles,
    directory: std::path::PathBuf,
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
        .map(|tiles| {
            Box::into_raw(Box::new(Spill {
                tiles,
                directory: std::path::Path::new(&a.cache_directory).join("drawing-tiles"),
            })) as jlong
        })
        .unwrap_or(0)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentSpillWork(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) {
    let job = unsafe { Box::from_raw(handle as *mut Spill) };
    fail(
        &mut env,
        layer_core::raster_storage::spill_to_directory(&job.tiles, &job.directory).map(|_| ()),
    );
}
