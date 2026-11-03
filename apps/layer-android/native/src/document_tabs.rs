//! Android scheduling around the shared drawing collection. Parked editors have
//! no renderer. One bounded worker drains retirement before activation.
use crate::{
    android::{error, fail, or_throw, read, string},
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

pub(crate) type Window = DocumentWindow<Box<UiSession<Renderer>>>;
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
    crate::jni_entry!(env, handle, capy_app, {
    let result = (|| {
        let request: TabRequest =
            serde_json::from_str(&read(&mut env, &request)?).map_err(error)?;
        let a = &mut *capy_app;
        Ok(a.window.request(&mut a.host, request)?.to_string())
    })();
    string(&mut env, result)
    })
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
    crate::jni_entry!(env, handle, capy_app, {
    let result = (|| {
        let a = &mut *capy_app;
        let options = a
            .host
            .renderer_options();
        let Some((activation, _)) =
            a.window
                .switch(&mut a.host, id as u64, close != 0, options, |_| {})?
        else {
            return Ok(0);
        };
        a.document_retired();
        a.blank_presented = false;
        crate::inspection::owned(activation)
    })();
    or_throw(&mut env, result, 0)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentPrepareClose(mut env: JNIEnv, _: JClass, handle: jlong) -> jlong {
    crate::jni_entry!(env, handle, capy_app, {
    let a = &mut *capy_app;
    let options = a.host.renderer_options();
    let result=a.window.prepare_close(&mut a.host,options).and_then(|job|crate::inspection::owned(crate::document_close::CloseJob(Some(job))));
    or_throw(&mut env,result,0)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentCommitClose(mut env: JNIEnv, _: JClass, handle: jlong, job: jlong) -> jlong {
    crate::jni_entry!(env, handle, capy_app, {
    let a = &mut *capy_app;
    let mut prepared_owner = crate::inspection::borrow::<crate::document_close::CloseJob>(job)?;let prepared=&mut *prepared_owner;
    let result = prepared.commit(&mut a.window, &mut a.host).and_then(|activation| {
        a.document_retired();a.blank_presented=false;
        crate::inspection::owned(activation)
    });
    or_throw(&mut env,result,0)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentReleasePreparedClose(mut env: JNIEnv, _: JClass, handle: jlong, job: jlong) {
    crate::jni_entry!(env, {
    let prepared=crate::inspection::take::<crate::document_close::CloseJob>(job)?;
    if let Some(prepared)=prepared.0 {
        crate::android::with_app_owner(handle,|app| {
            if app.host.poisoned().is_none(){app.window.cancel_close(&mut app.host,prepared)}else{Ok(())}
        })?;
    }
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentResumeWork(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) {
    crate::jni_entry!(env, {
    let mut job_owner = crate::inspection::borrow::<Activation>(handle)?;let job=&mut *job_owner;
    fail(&mut env, job.work());
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentResume(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    task: jlong,
) {
    crate::jni_entry!(env, handle, capy_app, {
    let result = (|| {
        let a = &mut *capy_app;
        let mut job_owner = crate::inspection::borrow::<Activation>(task)?;let job=&mut *job_owner;
        if let Some(gpu) = a.window.resume(&a.host, job)? {
            a.host.replace_renderer(Renderer(Some(gpu)))?;
            a.project_adopted();
            a.host.startup = Default::default();
            a.host.error = None;
        }
        a.window.changed(&mut a.host);
        Ok(())
    })();
    fail(&mut env, result)
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentResumeFree(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) {
    crate::jni_entry!(env, {
    crate::inspection::release::<Activation>(handle)?;
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentStorageWait(mut env:JNIEnv,_:JClass,handle:jlong)->jlong {
    crate::jni_entry!(env,handle,capy_app, {
        match capy_app.window.documents.trim_wait() {Some(wait)=>or_throw(&mut env,crate::inspection::owned(wait),0),None=>0}
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentStorageWaitWork(mut env:JNIEnv,_:JClass,task:jlong) {
    crate::jni_entry!(env, {
        let wait=crate::inspection::borrow::<layer_ui::TrimWait>(task)?.clone();
        fail(&mut env,wait.wait_on_worker())
    })
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_documentStorageWaitFree(mut env:JNIEnv,_:JClass,task:jlong) {
    crate::jni_entry!(env, {crate::inspection::release::<layer_ui::TrimWait>(task)?;})
}
