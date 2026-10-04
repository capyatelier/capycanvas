use crate::android::{app, error, fail, or_throw, read};
use jni::{JNIEnv, objects::{JClass, JString}, sys::{jboolean, jlong}};
use layer_host::{Renderer, open::OpenEnvironment};
use layer_ui::{UiSession, session_recovery::{SessionCapture, SessionManifest, SessionRestore}};
use std::{path::Path, sync::atomic::AtomicBool};
use std::time::{Duration, Instant};
use std::{fs::File, os::fd::FromRawFd};

enum Work {
    Capture(Option<SessionCapture>),
    Restore { environment: OpenEnvironment, candidates: Vec<(u64, Box<UiSession<Renderer>>)>, stamp: layer_ui::SessionStamp, pending: Option<(SessionRestore,bool)> },
    Retired { _renderers: Vec<Box<layer_render_wgpu::WgpuRasterizer>> },
}
struct Task { generation: u64, work: Work }

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionSettle(mut env: JNIEnv, _: JClass, handle: jlong, now: jlong) {
    let result = unsafe {app(handle)}.host.prepare_canvas_frame(now.max(0) as u64,now.max(0) as u64,true);
    fail(&mut env,result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionStamp(mut env: JNIEnv, _: JClass, handle: jlong, id: jlong) -> jni::sys::jstring {
    let result = (|| {
        let a = unsafe { app(handle) };
        serde_json::to_string(&a.window.session(&a.host, id as u64)?.session_stamp()).map_err(error)
    })();
    crate::android::string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionCapture(mut env: JNIEnv, _: JClass, handle: jlong, id: jlong) -> jlong {
    let result = (|| {
        let a = unsafe { app(handle) };
        let session = a.window.session(&a.host, id as u64)?;
        if session.recovery_document().busy { return Ok(0); }
        Ok(Box::into_raw(Box::new(Task {
            generation: a.gpu_generation,
            work: Work::Capture(Some(session.capture_session()?)),
        })) as jlong)
    })();
    or_throw(&mut env, result, 0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionStoreOpen(mut env: JNIEnv, _: JClass, directory: JString) -> jlong {
    let result = read(&mut env, &directory).and_then(|path| layer_core::package::session_store::SessionStore::open(Path::new(&path)))
        .map(|store| Box::into_raw(Box::new(store)) as jlong);
    or_throw(&mut env, result, 0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionStoreFree(_: JNIEnv, _: JClass, store: jlong) {
    if store != 0 { drop(unsafe { Box::from_raw(store as *mut layer_core::package::session_store::SessionStore) }); }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionStoreRetire(mut env: JNIEnv, _: JClass, store: jlong) {
    fail(&mut env, unsafe { crate::inspection::borrow::<layer_core::package::session_store::SessionStore>(store) }.retire())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionStorePrepareRetirement(mut env: JNIEnv, _: JClass, store: jlong) {
    fail(&mut env, unsafe { crate::inspection::borrow::<layer_core::package::session_store::SessionStore>(store) }.prepare_retirement())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionCommit(mut env: JNIEnv, _: JClass, task: jlong, store: jlong) {
    let task = unsafe { crate::inspection::borrow::<Task>(task) };
    let store = unsafe { crate::inspection::borrow::<layer_core::package::session_store::SessionStore>(store) };
    let result = crate::inspection::on_worker("capy-session-save", "Session worker failed", move || {
        let Work::Capture(capture) = &mut task.work else { return Err("Not a session capture".into()); };
        let cancel = AtomicBool::new(false);
        let prepared = capture.take().ok_or("Session capture already written")?.prepare(&cancel)?;
        store.commit(&prepared, &cancel)?;
        Ok(())
    });
    fail(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionFailure(mut env: JNIEnv, _: JClass, handle: jlong, detail: JString, recovery: jboolean) -> jni::sys::jstring {
    let result = read(&mut env,&detail).map(|detail| {
        let localization = unsafe {app(handle)}.host.session.localization();
        if recovery != 0 {layer_ui::document_recovery_unavailable(localization,&detail)}
        else {layer_ui::document_storage_retained(localization,&detail)}
    });
    crate::android::string(&mut env,result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionRequireEmptyRetry(mut env: JNIEnv, _: JClass, handle: jlong) {
    let result = (|| {
        let a = unsafe {app(handle)};
        if a.window.documents.order().len() == 1 && a.host.session.can_replace_startup_session(&a.host.session.session_stamp()) {Ok(())}
        else {Err(layer_ui::DocumentSessionError::DrawingsStillOpen.message(a.host.session.localization()))}
    })();
    fail(&mut env,result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionReserveIdentities(mut env: JNIEnv, _: JClass, handle: jlong, identities: JString) {
    let result = (|| {
        let identities: Vec<u64> = serde_json::from_str(&read(&mut env,&identities)?).map_err(error)?;
        unsafe {app(handle)}.window.documents.reserve_identities(&identities)
    })();
    fail(&mut env,result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionRestoreTask(mut env: JNIEnv, _: JClass, handle: jlong, stamp: JString) -> jlong {
    let result = (|| {
        let stamp: Option<layer_ui::SessionStamp> = serde_json::from_str(&read(&mut env, &stamp)?).map_err(error)?;
        let a = unsafe { app(handle) };
        let session = &a.host.session;
        session.require_document_idle()?;
        let environment = OpenEnvironment::capture(session,
            a.window.documents.admission(&session.retained_document_tiles()),
            a.host.renderer_options(Some(a.cache_directory.clone().into())))?;
        Ok(Box::into_raw(Box::new(Task {
            generation: a.gpu_generation,
            work: Work::Restore { environment, candidates: Vec::new(), stamp: stamp.unwrap_or_else(||session.session_stamp()), pending: None },
        })) as jlong)
    })();
    or_throw(&mut env, result, 0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionRead(mut env: JNIEnv, _: JClass, task: jlong, store: jlong, recovered: jboolean) -> jni::sys::jstring {
    let task = unsafe { crate::inspection::borrow::<Task>(task) };
    let store = unsafe { crate::inspection::borrow::<layer_core::package::session_store::SessionStore>(store) };
    let result = crate::inspection::on_worker("capy-session-read", "Session worker failed", move || {
        let Work::Restore { environment, pending, .. } = &mut task.work else { return Err("Not a session restore".into()); };
        if pending.is_some() {return Err("Session restore already read".into());}
        let opened = store.load(environment.limits(), &AtomicBool::new(false))?.ok_or("The drawing has no complete session copy")?;
        let restored = SessionRestore::from_core(opened)?;
        let location = serde_json::to_string(&restored.state.location.as_ref().filter(|_|restored.state.destination.is_some())).map_err(error)?;
        *pending = Some((restored,recovered != 0 || store.recovered_previous()));
        Ok(location)
    });
    crate::android::string(&mut env,result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionObserve(mut env: JNIEnv, _: JClass, task: jlong, fd: jni::sys::jint) -> jni::sys::jstring {
    let file = unsafe {File::from_raw_fd(fd)};
    let result = (|| {
        let task = unsafe {crate::inspection::borrow::<Task>(task)};
        let Work::Restore {pending:Some((restored,_)),..} = &task.work else {return Err("Session restore is not read".into());};
        let observed = restored.state.destination.as_ref().and_then(|expected|expected.observe(file));
        serde_json::to_string(&observed).map_err(error)
    })();
    crate::android::string(&mut env,result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionPrepare(mut env: JNIEnv, _: JClass, task: jlong, id: jlong, observed: JString) {
    let result = (|| {
        let observed: Option<layer_ui::DestinationFingerprint> = serde_json::from_str(&read(&mut env,&observed)?).map_err(error)?;
        let task = unsafe { crate::inspection::borrow::<Task>(task) };
        crate::inspection::on_worker("capy-session-open", "Session worker failed", move || {
            let Work::Restore { environment, candidates, pending, .. } = &mut task.work else { return Err("Not a session restore".into()); };
            let (restored,recovered) = pending.take().ok_or("Session restore is not read")?;
            let mut prepared = environment.prepare(restored.document().clone(), || false)?;
            prepared.restore_session(restored, recovered, observed)?;
            let deadline = Instant::now() + layer_host::open::PREPARE_DEADLINE;
            loop {
                prepared.frame(0, 0)?;
                if prepared.can_park_document() && prepared.retained_document_tiles().try_blobs()?.is_some() { break; }
                if Instant::now() >= deadline { return Err("Session canvas preparation timed out".into()); }
                if let Some(renderer) = &prepared.engine().backend().0 { renderer.device().poll(wgpu::PollType::Poll).map_err(error)?; }
                std::thread::sleep(Duration::from_millis(2));
            }
            candidates.push((id as u64, prepared));
            Ok(())
        })
    })();
    fail(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionAdopt(mut env: JNIEnv, _: JClass, handle: jlong, task: jlong, active: jlong, reserved: JString) -> jni::sys::jstring {
    let result = (|| {
        let reserved: Vec<u64> = serde_json::from_str(&read(&mut env,&reserved)?).map_err(error)?;
        let a = unsafe { app(handle) };
        let task = unsafe { crate::inspection::borrow::<Task>(task) };
        if task.generation != a.gpu_generation { return Err("The canvas changed while restoring".into()); }
        let Work::Restore { candidates, stamp, .. } = &mut task.work else { return Err("Session restore is not prepared".into()); };
        let mut ids = std::collections::BTreeMap::new();
        let replace = a.window.documents.order().len() == 1 && a.host.session.can_replace_startup_session(stamp);
        let retired = if replace {
            for(id,_)in candidates.iter() { ids.insert(*id,*id); }
            a.window.restore_sessions(&mut a.host, candidates, active as u64, stamp.clone(), |s| s)?
        } else {
            let (mapping, retired) = a.window.append_restored_sessions(&mut a.host, candidates, |s| s)?;
            ids.extend(mapping);
            retired
        };
        a.window.documents.reserve_identities(&reserved)?;
        task.work = Work::Retired { _renderers: retired };
        if replace {a.document_retired();a.project_adopted();}
        serde_json::to_string(&serde_json::json!({"ids":ids,"preserved":!replace})).map_err(error)
    })();
    crate::android::string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionHydrate(mut env: JNIEnv, _: JClass, handle: jlong, task: jlong, id: jlong) -> jlong {
    let result = (|| {
        let a = unsafe { app(handle) };
        let task = unsafe { crate::inspection::borrow::<Task>(task) };
        if task.generation != a.gpu_generation { return Err("The canvas changed while restoring".into()); }
        let Work::Restore {candidates,..} = &mut task.work else {return Err("Session restore is not prepared".into());};
        if candidates.len()!=1 {return Err("Expected one restored drawing".into());}
        let (_,candidate)=candidates.pop().unwrap();
        let mut candidate=Some(candidate);
        let (restored,retired)=a.window.hydrate_restored(&mut a.host,&mut candidate,id as u64,|s|s)?;
        task.work=Work::Retired {_renderers:retired.into_iter().collect()};
        Ok(restored as jlong)
    })();
    or_throw(&mut env,result,0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionFree(_: JNIEnv, _: JClass, task: jlong) {
    if task != 0 { drop(unsafe { Box::from_raw(task as *mut Task) }); }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionManifestRead(mut env: JNIEnv, _: JClass, path: JString) -> jni::sys::jstring {
    let result = read(&mut env, &path).and_then(|path| SessionManifest::read(Path::new(&path)))
        .and_then(|manifest| serde_json::to_string(&manifest).map_err(error));
    crate::android::string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionManifestWrite(mut env: JNIEnv, _: JClass, path: JString, value: JString) -> jni::sys::jstring {
    let result = (|| {
        let path = read(&mut env, &path)?;
        let manifest: SessionManifest = serde_json::from_str(&read(&mut env, &value)?).map_err(error)?;
        let (published,diagnostic) = match manifest.publish_checked(Path::new(&path)) {
            Ok(()) => (true,None),Err(failure) => (failure.published,Some(failure.error)),
        };
        serde_json::to_string(&serde_json::json!({"published":published,"error":diagnostic})).map_err(error)
    })();
    crate::android::string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionCollect(mut env: JNIEnv, _: JClass, directory: JString, value: JString) {
    let result = (|| {
        let path = read(&mut env, &directory)?;
        let manifest = SessionManifest::parse(read(&mut env, &value)?.as_bytes())?;
        let reachable = manifest.drawings.into_iter().map(|drawing|drawing.key).collect();
        layer_core::package::session_store::collect_unreferenced_stores(Path::new(&path), &reachable, &AtomicBool::new(false))?;
        Ok(())
    })();
    fail(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionManifestUpdate(mut env: JNIEnv, _: JClass, state: JString, event: JString) -> jni::sys::jstring {
    let result = (|| {
        let state = read(&mut env, &state)?;
        let event = serde_json::from_str(&read(&mut env, &event)?).map_err(error)?;
        let next = layer_ui::session_recovery::session_manifest_update(&state, event)?;
        serde_json::to_string(&next).map_err(error)
    })();
    crate::android::string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionClose(mut env: JNIEnv, _: JClass, handle: jlong) {
    let result = (|| {
        let a = unsafe { app(handle) };
        let previous = a.host.session.state().revision;
        let change = a.host.session.request_session_close()?;
        a.host.apply_change(previous, change);
        Ok(())
    })();
    fail(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionFingerprint(mut env: JNIEnv, _: JClass, fd: jni::sys::jint) -> jni::sys::jstring {
    let result = layer_ui::DestinationFingerprint::read(unsafe { File::from_raw_fd(fd) })
        .and_then(|fingerprint| serde_json::to_string(&fingerprint).map_err(error));
    crate::android::string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionDestination(mut env: JNIEnv, _: JClass, handle: jlong) -> jni::sys::jstring {
    let result = serde_json::to_string(&unsafe { app(handle) }.host.session.save_destination_expectation()).map_err(error);
    crate::android::string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionDestinationMatches(mut env: JNIEnv, _: JClass, expectation: JString, observed: JString) -> jboolean {
    let result = (|| {
        let expected: layer_ui::DestinationExpectation = serde_json::from_str(&read(&mut env, &expectation)?).map_err(error)?;
        let actual: Option<layer_ui::DestinationFingerprint> = serde_json::from_str(&read(&mut env, &observed)?).map_err(error)?;
        Ok(u8::from(expected.matches(actual.as_ref())))
    })();
    or_throw(&mut env, result, 0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionRecordDestination(mut env: JNIEnv, _: JClass, handle: jlong, location: JString, fingerprint: JString) {
    let result = (|| {
        let location = serde_json::from_str(&read(&mut env, &location)?).map_err(error)?;
        let fingerprint = serde_json::from_str(&read(&mut env, &fingerprint)?).map_err(error)?;
        unsafe { app(handle) }.host.session.record_destination_fingerprint(&location, fingerprint)
    })();
    fail(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionCompleteSave(mut env: JNIEnv, _: JClass, handle: jlong, request: jni::sys::jint, location: JString, fingerprint: JString) {
    let result = (|| {
        let location = serde_json::from_str(&read(&mut env, &location)?).map_err(error)?;
        let fingerprint = serde_json::from_str(&read(&mut env, &fingerprint)?).map_err(error)?;
        let a = unsafe { app(handle) };
        let previous = a.host.session.state().revision;
        let change = a.host.session.complete_document_request(request as u32, Ok(true))?;
        a.host.session.record_destination_fingerprint(&location, fingerprint)?;
        a.host.apply_change(previous, change);
        Ok(())
    })();
    fail(&mut env, result)
}
