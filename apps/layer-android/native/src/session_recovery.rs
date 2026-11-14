use crate::android::{error, fail, or_throw, read};
use jni::{JNIEnv, JavaVM, objects::{GlobalRef, JClass, JString, JValue}, sys::{jboolean, jlong}};
use layer_host::{Renderer, open::OpenEnvironment,session_stores::{SessionStores,StoreResult}};
use layer_ui::{UiSession, session_recovery::{ClaimedManifest, SessionCapture, SessionManifest, SessionRestore}};
use std::{path::Path, sync::atomic::AtomicBool};
use std::time::{Duration, Instant};
use std::{fs::File, os::fd::FromRawFd};
use std::{cell::RefCell, rc::Rc, sync::Arc};

struct SessionOwner { object: GlobalRef, vm: Arc<JavaVM>, failure: Rc<RefCell<Option<String>>> }
impl Drop for SessionOwner {
    fn drop(&mut self) {
        let result = (|| {
            let mut env = self.vm.attach_current_thread().map_err(error)?;
            let result = env.call_method(self.object.as_obj(), "close", "()V", &[]).map(|_| ()).map_err(error);
            if result.is_err() { let _ = env.exception_clear(); }
            result
        })();
        if let Err(error) = result { self.failure.borrow_mut().get_or_insert(error); }
    }
}

enum Work {
    Capture {capture:Option<Box<SessionCapture>>,stamp:layer_ui::SessionStamp},
    Restore { environment: Box<OpenEnvironment>, candidates: Vec<(u64, Box<UiSession<Renderer>>)>, stamp: Box<layer_ui::SessionStamp>, pending: Option<Box<(SessionRestore,bool)>> },
    Retired { _renderers: Vec<Box<layer_render_wgpu::WgpuRasterizer>> },
}
struct Task { generation: u64, work: Work }

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionSettle(mut env: JNIEnv, _: JClass, handle: jlong, now: jlong) {
    crate::jni_entry!(env, handle, capy_app, {
    let result = capy_app.host.prepare_canvas_frame(now.max(0) as u64,now.max(0) as u64,true);
    fail(&mut env,result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionStamp(mut env: JNIEnv, _: JClass, handle: jlong, id: jlong) -> jni::sys::jstring {
    crate::jni_entry!(env, handle, capy_app, {
    let result = (|| {
        let a = &mut *capy_app;
        serde_json::to_string(&a.window.session(&a.host, id as u64)?.session_stamp()).map_err(error)
    })();
    crate::android::string(&mut env, result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionCapture(mut env: JNIEnv, _: JClass, handle: jlong, id: jlong) -> jlong {
    crate::jni_entry!(env, handle, capy_app, {
    let result = (|| {
        let a = &mut *capy_app;
        let session = a.window.session(&a.host, id as u64)?;
        if session.recovery_document().busy { return Ok(0); }
        crate::inspection::owned(Task {
            generation: a.gpu_generation,
            work: Work::Capture {capture:Some(Box::new(session.capture_session()?)),stamp:session.session_stamp()},
        })
    })();
    or_throw(&mut env, result, 0)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionStoresOpen(mut env:JNIEnv,_:JClass,directory:JString)->jlong {
    crate::jni_entry!(env, {
        let result=read(&mut env,&directory).and_then(|path|crate::inspection::owned(SessionStores::native(path.into(),Arc::new(||{}))));
        or_throw(&mut env,result,0)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionBind(mut env:JNIEnv,_:JClass,stores:jlong,id:jlong,key:JString)->jni::sys::jstring {
    crate::jni_entry!(env, {
        let key=if key.is_null() {None}else {Some(read(&mut env,&key)?)};
        let stores=crate::inspection::borrow::<SessionStores>(stores)?;
        let result=match key {Some(key)=>stores.adopt(id as u64,&key),None=>stores.bind(id as u64,None)}.map(|binding|binding.key().to_owned());
        crate::android::string(&mut env,result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionWatchPressure(mut env:JNIEnv,_:JClass,handle:jlong,stores:jlong) {
    crate::jni_entry!(env,handle,capy_app, {
        let stores=crate::inspection::borrow::<SessionStores>(stores)?;
        stores.set_wake(capy_app.host.wake_callback());
        stores.attach_window(&capy_app.window,&capy_app.host)?;
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionCheckpointInterval(mut env:JNIEnv,_:JClass)->jlong {
    crate::jni_entry!(env, {SessionStores::checkpoint_interval_ms() as jlong})
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionCheckpointNeeded(mut env:JNIEnv,_:JClass,stores:jlong,id:jlong,stamp:JString,force:jboolean)->jboolean {
    crate::jni_entry!(env, {
        let stamp=serde_json::from_str(&read(&mut env,&stamp)?).map_err(error)?;
        let result=crate::inspection::borrow::<SessionStores>(stores)?.checkpoint_due(id as u64,stamp,force!=0).map(u8::from);
        or_throw(&mut env,result,0)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionImport(mut env: JNIEnv, _: JClass, directory: JString) -> jni::sys::jstring {
    crate::jni_entry!(env, {
    let result = (|| {
        let root = read(&mut env, &directory)?;
        let vm = Arc::new(env.get_java_vm().map_err(error)?); let failure = Rc::new(RefCell::new(None));
        let result = layer_core::package::session_store::import_session_directories_with_owner(Path::new(&root), "session.json", "owner.lock", |lock| {
            let path = env.new_string(lock.parent().ok_or("Invalid session owner path")?.to_string_lossy()).map_err(error)?;
            let path = env.auto_local(path);
            let owner = env.call_static_method("art/capycanvas/SessionOwners", "claim", "(Ljava/lang/String;)Ljava/lang/AutoCloseable;", &[JValue::Object(path.as_ref())]).and_then(|value| value.l()).map_err(error)?;
            let owner = env.auto_local(owner);
            if owner.is_null() { return Ok(None); }
            let object = match env.new_global_ref(&owner) {
                Ok(object) => object,
                Err(error) => { let _ = env.exception_clear(); let _ = env.call_method(&owner, "close", "()V", &[]); let _ = env.exception_clear(); return Err(crate::android::error(error)); }
            };
            Ok(Some(SessionOwner { object, vm: vm.clone(), failure: failure.clone() }))
        });
        let release = failure.borrow_mut().take();
        if let Some(error) = release { return Err(error); }
        result.map(|path| path.to_string_lossy().into_owned())
    })();
    crate::android::string(&mut env, result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionStoresFree(mut env: JNIEnv, _: JClass, store: jlong) {
    crate::jni_entry!(env, {
    if store != 0 { crate::inspection::release::<SessionStores>(store)?; }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionStoreRetire(mut env: JNIEnv, _: JClass, store: jlong,id:jlong) {
    crate::jni_entry!(env, {
    fail(&mut env, crate::inspection::borrow::<SessionStores>(store)?.retire(id as u64)?.wait_on_worker().map(|_|()))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionStorePrepareRetirement(mut env: JNIEnv, _: JClass, store: jlong,id:jlong) {
    crate::jni_entry!(env, {
    let stores=crate::inspection::borrow::<SessionStores>(store)?.clone();let binding=stores.bind(id as u64,None)?;
    fail(&mut env,stores.prepare_retirement(binding.key())?.wait_on_worker().map(|_|()))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionCommit(mut env: JNIEnv, _: JClass, task: jlong, store: jlong,id:jlong) {
    crate::jni_entry!(env, {
    let mut task_owner = crate::inspection::borrow::<Task>(task)?;let task=&mut *task_owner;
    let stores=crate::inspection::borrow::<SessionStores>(store)?.clone();
    let result = crate::inspection::on_worker("capy-session-save", "Session worker failed", move || {
        let Work::Capture {capture,stamp} = &mut task.work else { return Err("Not a session capture".into()); };
        let capture=*capture.take().ok_or("Session capture already written")?;
        if !stores.is_current(id as u64,stamp) {stores.checkpoint(id as u64,capture,stamp.clone(),true)?.wait_on_worker()?;}
        Ok(())
    });
    fail(&mut env, result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionFailure(mut env: JNIEnv, _: JClass, handle: jlong, detail: JString, recovery: jboolean) -> jni::sys::jstring {
    crate::jni_entry!(env, handle, capy_app, {
    let result = read(&mut env,&detail).map(|detail| {
        capy_app.host.session.storage_failed(detail.clone());
        let localization = capy_app.host.session.localization();
        if recovery != 0 {layer_ui::document_recovery_unavailable(localization,&detail)}
        else {layer_ui::document_storage_retained(localization,&detail)}
    });
    crate::android::string(&mut env,result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionRequireEmptyRetry(mut env: JNIEnv, _: JClass, handle: jlong) {
    crate::jni_entry!(env, handle, capy_app, {
    let result = {
        let a = &mut *capy_app;
        if a.window.documents.order().len() == 1 && a.host.session.can_replace_startup_session(&a.host.session.session_stamp()) {Ok(())}
        else {Err(layer_ui::DocumentSessionError::DrawingsStillOpen.message(a.host.session.localization()))}
    };
    fail(&mut env,result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionReserveIdentities(mut env: JNIEnv, _: JClass, handle: jlong, identities: JString) {
    crate::jni_entry!(env, handle, capy_app, {
    let result = (|| {
        let identities: Vec<u64> = serde_json::from_str(&read(&mut env,&identities)?).map_err(error)?;
        capy_app.window.documents.reserve_identities(&identities)
    })();
    fail(&mut env,result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionRestoreTask(mut env: JNIEnv, _: JClass, handle: jlong, stamp: JString) -> jlong {
    crate::jni_entry!(env, handle, capy_app, {
    let result = (|| {
        let stamp: Option<layer_ui::SessionStamp> = serde_json::from_str(&read(&mut env, &stamp)?).map_err(error)?;
        let a = &mut *capy_app;
        let session = &a.host.session;
        session.require_document_idle()?;
        let environment = OpenEnvironment::capture(session,
            a.window.documents.admission(&session.retained_document_tiles()),
            a.host.renderer_options())?;
        crate::inspection::owned(Task {
            generation: a.gpu_generation,
            work: Work::Restore { environment:Box::new(environment), candidates: Vec::new(), stamp: Box::new(stamp.unwrap_or_else(||session.session_stamp())), pending: None },
        })
    })();
    or_throw(&mut env, result, 0)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionRead(mut env: JNIEnv, _: JClass, task: jlong, store: jlong,id:jlong,key:JString,recovered: jboolean) -> jni::sys::jstring {
    crate::jni_entry!(env, {
    let mut task_owner = crate::inspection::borrow::<Task>(task)?;let task=&mut *task_owner;
    let stores=crate::inspection::borrow::<SessionStores>(store)?.clone();let key=read(&mut env,&key)?;
    let result = crate::inspection::on_worker("capy-session-read", "Session worker failed", move || {
        let Work::Restore { environment, pending, .. } = &mut task.work else { return Err("Not a session restore".into()); };
        if pending.is_some() {return Err("Session restore already read".into());}
        stores.adopt(id as u64,&key)?;
        environment.use_store(&stores,id as u64)?;
        let StoreResult::Restored {session,recovered_previous}=stores.restore(id as u64,environment.limits())?.wait_on_worker()? else {return Err("Unexpected drawing storage reply".into());};
        let restored = session.ok_or("The drawing has no complete session copy")?;
        let location = serde_json::to_string(&restored.state.location.as_ref().filter(|_|restored.state.destination.is_some())).map_err(error)?;
        *pending = Some(Box::new((restored,recovered != 0 || recovered_previous)));
        Ok(location)
    });
    crate::android::string(&mut env,result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionObserve(mut env: JNIEnv, _: JClass, task: jlong, fd: jni::sys::jint) -> jni::sys::jstring {
    crate::jni_entry!(env, {
    let file = unsafe {File::from_raw_fd(fd)};
    let result = (|| {
        let mut task_owner = crate::inspection::borrow::<Task>(task)?;let task=&mut *task_owner;
        let Work::Restore {pending,..} = &task.work else {return Err("Session restore is not read".into());};
        let (restored,_)=pending.as_deref().ok_or("Session restore is not read")?;
        let observed = restored.state.destination.as_ref().and_then(|expected|expected.observe(file));
        serde_json::to_string(&observed).map_err(error)
    })();
    crate::android::string(&mut env,result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionPrepare(mut env: JNIEnv, _: JClass, task: jlong, id: jlong, observed: JString) {
    crate::jni_entry!(env, {
    let result = (|| {
        let observed: Option<layer_ui::DestinationFingerprint> = serde_json::from_str(&read(&mut env,&observed)?).map_err(error)?;
        let mut task_owner = crate::inspection::borrow::<Task>(task)?;let task=&mut *task_owner;
        crate::inspection::on_worker("capy-session-open", "Session worker failed", move || {
            let Work::Restore { environment, candidates, pending, .. } = &mut task.work else { return Err("Not a session restore".into()); };
            let (restored,recovered) = *pending.take().ok_or("Session restore is not read")?;
            let mut prepared = environment.prepare(restored.document().clone(), || false)?;
            prepared.restore_session(restored, recovered, observed)?;
            let deadline = Instant::now() + layer_host::open::PREPARE_DEADLINE;
            loop {
                prepared.frame(0, 0)?;
                if prepared.can_park_document() && prepared.retained_document_tiles().try_blobs()?.is_some() { break; }
                if Instant::now() >= deadline { return Err("Session canvas preparation timed out".into()); }
                std::thread::sleep(Duration::from_millis(2));
            }
            candidates.push((id as u64, prepared));
            Ok(())
        })
    })();
    fail(&mut env, result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionAdopt(mut env: JNIEnv, _: JClass, handle: jlong, task: jlong, active: jlong, reserved: JString) -> jni::sys::jstring {
    crate::jni_entry!(env, handle, capy_app, {
    let result = (|| {
        let reserved: Vec<u64> = serde_json::from_str(&read(&mut env,&reserved)?).map_err(error)?;
        let a = &mut *capy_app;
        let mut task_owner = crate::inspection::borrow::<Task>(task)?;let task=&mut *task_owner;
        if task.generation != a.gpu_generation { return Err("The canvas changed while restoring".into()); }
        let Work::Restore { candidates, stamp, .. } = &mut task.work else { return Err("Session restore is not prepared".into()); };
        let mut ids = std::collections::BTreeMap::new();
        let replace = a.window.documents.order().len() == 1 && a.host.session.can_replace_startup_session(stamp);
        let retired = if replace {
            for(id,_)in candidates.iter() { ids.insert(*id,*id); }
            a.window.restore_sessions(&mut a.host, candidates, active as u64, stamp.as_ref().clone(), |s| s)?
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
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionHydrate(mut env: JNIEnv, _: JClass, handle: jlong, task: jlong, id: jlong) -> jlong {
    crate::jni_entry!(env, handle, capy_app, {
    let result = (|| {
        let a = &mut *capy_app;
        let mut task_owner = crate::inspection::borrow::<Task>(task)?;let task=&mut *task_owner;
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
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionFree(mut env: JNIEnv, _: JClass, task: jlong) {
    crate::jni_entry!(env, {
    if task != 0 { crate::inspection::release::<Task>(task)?; }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionManifestRead(mut env: JNIEnv, _: JClass, path: JString) -> jni::sys::jstring {
    crate::jni_entry!(env, {
    let result = read(&mut env, &path).and_then(|path| SessionManifest::read(Path::new(&path)).map_err(error))
        .and_then(|manifest| serde_json::to_string(&manifest).map_err(error));
    crate::android::string(&mut env, result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionManifestClaim(mut env: JNIEnv, _: JClass, path: JString) -> jni::sys::jstring {
    crate::jni_entry!(env, {
    let result = read(&mut env, &path).and_then(|path| SessionManifest::read_claimed(Path::new(&path))).and_then(|claimed| serde_json::to_string(&match claimed {
        ClaimedManifest::Ready(manifest) => serde_json::json!({"manifest": manifest}),
        ClaimedManifest::SetAside { error, .. } => serde_json::json!({"set_aside": error}),
        ClaimedManifest::Missing => serde_json::json!({}),
    }).map_err(error));
    crate::android::string(&mut env, result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionManifestWrite(mut env: JNIEnv, _: JClass, path: JString, value: JString) -> jni::sys::jstring {
    crate::jni_entry!(env, {
    let result = (|| {
        let path = read(&mut env, &path)?;
        let manifest: SessionManifest = serde_json::from_str(&read(&mut env, &value)?).map_err(error)?;
        let (published,diagnostic) = match manifest.publish_checked(Path::new(&path)) {
            Ok(()) => (true,None),Err(failure) => (failure.published,Some(failure.error)),
        };
        serde_json::to_string(&serde_json::json!({"published":published,"error":diagnostic})).map_err(error)
    })();
    crate::android::string(&mut env, result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionPrepareRetirement(mut env: JNIEnv, _: JClass, stores:jlong,key:JString) {
    crate::jni_entry!(env, {
    let key=read(&mut env,&key)?;let stores=crate::inspection::borrow::<SessionStores>(stores)?.clone();
    fail(&mut env,stores.prepare_retirement(&key)?.wait_on_worker().map(|_|()))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionRetireCopy(mut env:JNIEnv,_:JClass,stores:jlong,key:JString) {
    crate::jni_entry!(env, {
    let key=read(&mut env,&key)?;let stores=crate::inspection::borrow::<SessionStores>(stores)?.clone();
    fail(&mut env,stores.retire_copy(&key)?.wait_on_worker().map(|_|()))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionCollect(mut env: JNIEnv, _: JClass, directory: JString, keys: JString) {
    crate::jni_entry!(env, {
    let result = (|| {
        let path = read(&mut env, &directory)?;
        let reachable = serde_json::from_str(&read(&mut env, &keys)?).map_err(error)?;
        pollster::block_on(layer_core::package::session_store::collect_unreferenced_stores(Path::new(&path), &reachable, &AtomicBool::new(false)));
        Ok(())
    })();
    fail(&mut env, result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionManifestUpdate(mut env: JNIEnv, _: JClass, state: JString, event: JString) -> jni::sys::jstring {
    crate::jni_entry!(env, {
    let result = (|| {
        let state = read(&mut env, &state)?;
        let event = serde_json::from_str(&read(&mut env, &event)?).map_err(error)?;
        let next = layer_ui::session_recovery::session_manifest_update(&state, event)?;
        serde_json::to_string(&next).map_err(error)
    })();
    crate::android::string(&mut env, result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionClose(mut env: JNIEnv, _: JClass, handle: jlong) {
    crate::jni_entry!(env, handle, capy_app, {
    let result = (|| {
        let a = &mut *capy_app;
        let previous = a.host.session.state().revision;
        let change = a.host.session.request_session_close()?;
        a.host.apply_change(previous, change);
        Ok(())
    })();
    fail(&mut env, result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionFingerprint(mut env: JNIEnv, _: JClass, fd: jni::sys::jint) -> jni::sys::jstring {
    crate::jni_entry!(env, {
    let result = layer_ui::DestinationFingerprint::read(unsafe { File::from_raw_fd(fd) })
        .and_then(|fingerprint| serde_json::to_string(&fingerprint).map_err(error));
    crate::android::string(&mut env, result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionDestination(mut env: JNIEnv, _: JClass, handle: jlong) -> jni::sys::jstring {
    crate::jni_entry!(env, handle, capy_app, {
    let result = serde_json::to_string(&capy_app.host.session.save_destination_expectation()).map_err(error);
    crate::android::string(&mut env, result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionDestinationMatches(mut env: JNIEnv, _: JClass, expectation: JString, observed: JString) -> jboolean {
    crate::jni_entry!(env, {
    let result = (|| {
        let expected: layer_ui::DestinationExpectation = serde_json::from_str(&read(&mut env, &expectation)?).map_err(error)?;
        let actual: Option<layer_ui::DestinationFingerprint> = serde_json::from_str(&read(&mut env, &observed)?).map_err(error)?;
        Ok(u8::from(expected.matches(actual.as_ref())))
    })();
    or_throw(&mut env, result, 0)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionRecordDestination(mut env: JNIEnv, _: JClass, handle: jlong, location: JString, fingerprint: JString) {
    crate::jni_entry!(env, handle, capy_app, {
    let result = (|| {
        let location = serde_json::from_str(&read(&mut env, &location)?).map_err(error)?;
        let fingerprint = serde_json::from_str(&read(&mut env, &fingerprint)?).map_err(error)?;
        capy_app.host.session.record_destination_fingerprint(&location, fingerprint)
    })();
    fail(&mut env, result)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_sessionCompleteSave(mut env: JNIEnv, _: JClass, handle: jlong, request: jni::sys::jint, location: JString, fingerprint: JString) {
    crate::jni_entry!(env, handle, capy_app, {
    let result = (|| {
        let location = serde_json::from_str(&read(&mut env, &location)?).map_err(error)?;
        let fingerprint = serde_json::from_str(&read(&mut env, &fingerprint)?).map_err(error)?;
        let a = &mut *capy_app;
        let previous = a.host.session.state().revision;
        let change = a.host.session.complete_document_request(request as u32, Ok(true))?;
        a.host.session.record_destination_fingerprint(&location, fingerprint)?;
        a.host.apply_change(previous, change);
        Ok(())
    })();
    fail(&mut env, result)
    })
}
