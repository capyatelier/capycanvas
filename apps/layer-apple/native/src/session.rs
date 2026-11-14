use super::*;
use layer_core::package::session_store::{SessionLease, collect_unreferenced_stores, preserve, restore_error, set_restore_error, sync_directory};
use layer_host::{open::OpenEnvironment,session_stores::{SessionStores,StoreResult}};
use layer_ui::session_recovery::{ClaimedManifest, SessionCapture, SessionDrawing, SessionManifest, SessionReadError, SessionRestore};
use std::{collections::BTreeMap, path::{Component, Path, PathBuf}, sync::{Arc, Mutex, atomic::AtomicBool}};

pub(crate) struct WindowDisk {
    path: PathBuf,
    _owner: SessionLease,
    stores: SessionStores,
    manifest: SessionManifest,
    unpublished: Option<SessionManifest>,
    published_sequence: u64,
}
impl WindowDisk {
    fn finish_restore(&mut self) -> Result<(), String> {
        let Some(previous) = &self.unpublished else { return Ok(()); };
        let path = self.path.join("window.json");
        let stored = SessionManifest::read(&path)?.unwrap_or_default();
        if stored != *previous && stored != self.manifest { return Err("The saved editing session changed unexpectedly".into()); }
        let mut next = self.manifest.clone();
        for attempt in next.restoring.clone() { next = next.finish_restore(attempt, true)?; }
        match next.publish_checked(&path) {
            Ok(()) => {
                for drawing in &next.drawings {
                    if !next.blocked.contains(&drawing.id) { let _ = set_restore_error(&self.path.join(&drawing.key), None); }
                }
                self.manifest = next; self.unpublished = None; Ok(())
            }
            Err(error) => {
                if error.published { self.manifest = next.clone(); self.unpublished = Some(next); }
                Err(error.error)
            }
        }
    }
}
pub struct CapySessionTask(Mutex<SessionJob>);
pub struct CapySessionDestination(layer_ui::DestinationFingerprint);
pub type SessionDestinationObserver = unsafe extern "C" fn(*const c_char, *mut std::ffi::c_void) -> u64;
struct SessionOpen {
    sessions: PathBuf,
    scene: String,
    adopt: bool,
}
fn has_drawings(directory: &Path) -> Result<bool, SessionReadError> {
    Ok(SessionManifest::read(&directory.join("window.json"))?.is_some_and(|manifest| !manifest.drawings.is_empty()))
}
/// The scene's own session or, when it has no drawings and `adopt` is set, the
/// newest unlocked session that has drawings, renamed to the scene.
fn claim(mut open: SessionOpen) -> Result<(PathBuf, SessionLease, Option<String>), String> {
    let mut parts = Path::new(&open.scene).components();
    if !matches!((parts.next(), parts.next()), (Some(Component::Normal(_)), None)) {
        return Err("Invalid editing session".into());
    }
    open.sessions = layer_core::package::session_store::import_session_directories(&open.sessions, "window.json", ".lock")?;
    let own = open.sessions.join(&open.scene);
    let lease = SessionLease::claim(&own)?.ok_or("This editing session is already open")?;
    match SessionManifest::read_claimed(&own.join("window.json"))? {
        ClaimedManifest::SetAside { error, preserved } => return Ok((own, lease, Some(format!("Could not read the saved drawing list: {error}\nOriginal files preserved at {}", preserved.display())))),
        ClaimedManifest::Ready(manifest) if !manifest.drawings.is_empty() => return Ok((own, lease, None)),
        _ if !open.adopt => return Ok((own, lease, None)),
        _ => {}
    }
    let mut candidates = Vec::new();
    for entry in std::fs::read_dir(&open.sessions).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path != own && !entry_name_is_preserved(&path) && let Ok(modified) = std::fs::metadata(path.join("window.json")).and_then(|metadata| metadata.modified()) {
            candidates.push((modified, path));
        }
    }
    candidates.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    for (_, candidate) in candidates {
        if !has_drawings(&candidate).unwrap_or(false) { continue; }
        let Ok(Some(adopted)) = SessionLease::claim(&candidate) else { continue };
        preserve(&own)?;
        std::fs::rename(&candidate, &own).map_err(|e| e.to_string())?;
        sync_directory(&open.sessions)?;
        return Ok((own, adopted, None));
    }
    Ok((own, lease, None))
}
fn entry_name_is_preserved(path: &Path) -> bool { path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("preserved-")) }
fn record_failure(path: &Path, error: String) -> String {
    match set_restore_error(path, Some(&error)) {
        Ok(()) => error,
        Err(diagnostic) => format!("{error}\nCould not record the restore failure: {diagnostic}"),
    }
}
struct SessionJob {
    open: Option<SessionOpen>,
    disk: Option<Arc<Mutex<WindowDisk>>>,
    environment: Option<OpenEnvironment>,
    captures: Option<Vec<(u64, SessionCapture,layer_ui::SessionStamp)>>,
    candidates: Vec<(u64, Box<layer_ui::UiSession<layer_host::Renderer>>)>,
    active: u64,
    clean_exit: bool,
    retry: bool,
    append: bool,
    stamp: layer_ui::SessionStamp,
    retired: Vec<Box<layer_render_wgpu::WgpuRasterizer>>,
    error: Option<CString>,
    warning: Option<CString>,
    failures: BTreeMap<String, String>,
    restored: bool,
    adopted: bool,
    sequence: u64,
    exclusion: u64,
    committed: bool,
}
impl Drop for SessionJob {
    fn drop(&mut self) {
        if !self.adopted && let Some(environment)=&self.environment {
            environment.storage_failed(self.error.as_ref().map_or_else(||"Drawing storage owner closed".into(),|error|error.to_string_lossy().into_owned()));
        }
    }
}
impl SessionJob {
    fn manifest_path(&self) -> Result<PathBuf, String> {
        self.disk.as_ref().map(|disk| disk.lock().unwrap_or_else(|e| e.into_inner()).path.join("window.json"))
            .ok_or_else(|| "The editing session is unavailable".into())
    }
    fn load(&mut self, observe: Option<SessionDestinationObserver>, context: *mut std::ffi::c_void) -> Result<(), String> {
        let open = self.open.take().ok_or("Session storage is unavailable")?;
        if self.disk.is_none() {
            let (path, owner, warning) = claim(open)?;
            self.warning = warning.and_then(|text| CString::new(text.replace('\0', " ")).ok());
            self.disk = Some(Arc::new(Mutex::new(WindowDisk { stores:SessionStores::native(path.clone(),Arc::new(||{})), path, _owner: owner, manifest: SessionManifest::default(), unpublished: None, published_sequence: 0 })));
        }
        self.disk.as_ref().unwrap().lock().unwrap_or_else(|e| e.into_inner()).finish_restore()?;
        let manifest_path = self.manifest_path()?;
        let Some(mut manifest) = SessionManifest::read(&manifest_path)? else { return Ok(()) };
        let interrupted: Vec<_> = manifest.restoring.iter().map(|attempt| attempt.id).collect();
        if !interrupted.is_empty() {
            manifest = manifest.recover_interrupted()?;
            manifest.publish(&manifest_path)?;
        }
        let recovered = !manifest.clean_exit;
        self.active = manifest.active;
        let environment = self.environment.as_mut().ok_or("The canvas is unavailable")?;
        let disk = self.disk.as_ref().unwrap();
        let mut disk = disk.lock().unwrap_or_else(|e| e.into_inner());
        disk.manifest = manifest.clone();
        let mut drawings = manifest.drawings.clone();
        if self.append { drawings.retain(|drawing| manifest.blocked.contains(&drawing.id)); }
        drawings.sort_by_key(|drawing| drawing.id != manifest.active);
        let mut restored = Vec::with_capacity(drawings.len());
        let mut failures = BTreeMap::new();
        for drawing in drawings {
            let path = disk.path.join(&drawing.key);
            if manifest.blocked.contains(&drawing.id) && !self.retry {
                let error = match restore_error(&path).unwrap_or_else(|error| Some(format!("Could not read the saved failure: {error}"))) {
                    Some(error) if interrupted.contains(&drawing.id) => format!("Restore was interrupted. Previous failure: {error}"),
                    Some(error) => error,
                    None => "A drawing interrupted the previous restart. Its saved session has been preserved.".into(),
                };
                failures.insert(drawing.key, error);
                continue;
            }
            manifest = if self.retry { manifest.retry_restore(drawing.id)? } else { manifest.begin_restore(drawing.id)? };
            manifest.publish(&manifest_path)?;
            let result = (|| -> Result<_, String> {
                disk.stores.adopt(drawing.id,&drawing.key)?;
                let StoreResult::Restored {session,recovered_previous}=disk.stores.restore(drawing.id,environment.limits())?.wait_on_worker()? else {return Err("Unexpected drawing storage reply".into());};
                let restore = session.ok_or("A saved drawing is missing; its editing session has been preserved")?;
                environment.admit_sessions(restored.iter().map(|(_, restore, _): &(_, SessionRestore, _)| &restore.editor).chain(std::iter::once(&restore.editor)))?;
                let recovered = recovered || recovered_previous;
                Ok((drawing.id, restore, recovered))
            })();
            match result {
                Ok(restore) => restored.push(restore),
                Err(error) => {
                    let error = record_failure(&path, error);
                    let attempt = *manifest.restoring.iter().find(|attempt| attempt.id == drawing.id).unwrap();
                    manifest = manifest.finish_restore(attempt, false)?;
                    manifest.publish(&manifest_path)?;
                    failures.insert(drawing.key, error);
                }
            }
        }
        for (id, restore, recovered) in restored {
            let result = (|| -> Result<_, String> {
                let observed = restore.state.location.as_ref().and_then(|location| {
                    let uri = CString::new(location.uri.as_str()).ok()?;
                    let fingerprint = unsafe { observe?(uri.as_ptr(), context) };
                    handles::take::<CapySessionDestination>(fingerprint).map(|fingerprint|fingerprint.0)
                });
                environment.use_store(&disk.stores,id)?;
                let mut candidate = environment.prepare(restore.document().clone(), || false)?;
                candidate.restore_session(restore, recovered, observed)?;
                candidate.set_document_replacement(false);
                let deadline = std::time::Instant::now() + layer_host::open::PREPARE_DEADLINE;
                loop {
                    candidate.frame(0, 0)?;
                    if candidate.can_park_document() && candidate.retained_document_tiles().try_blobs()?.is_some() { break; }
                    if std::time::Instant::now() >= deadline { return Err("The saved drawing could not finish preparing".into()); }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                Ok(candidate)
            })();
            match result {
                Ok(candidate) => self.candidates.push((id, candidate)),
                Err(error) => {
                    let key = manifest.drawings.iter().find(|drawing| drawing.id == id).unwrap().key.clone();
                    let error = record_failure(&disk.path.join(&key), error);
                    let attempt = *manifest.restoring.iter().find(|attempt| attempt.id == id).unwrap();
                    manifest = manifest.finish_restore(attempt, false)?;
                    manifest.publish(&manifest_path)?;
                    failures.insert(key, error);
                }
            }
        }
        self.restored = !self.candidates.is_empty();
        if !self.append
            && !self.candidates.iter().any(|(id, _)| *id == self.active) {
                self.active = self.candidates.first().map_or(0, |(id, _)| *id);
            }
        self.failures = failures;
        let warnings: Vec<_> = self.failures.iter().map(|(key, error)| format!("Drawing {key}: {error}")).collect();
        self.warning = if warnings.is_empty() { None } else { CString::new(warnings.join("\n\n").replace('\0', " ")).ok() };
        manifest = manifest.reconcile(manifest.drawings.clone(), manifest.active, false)?;
        self.candidates.sort_by_key(|(id, _)| manifest.drawings.iter().position(|drawing| drawing.id == *id).unwrap());
        manifest.publish(&manifest_path)?;
        disk.manifest = manifest.clone();
        Ok(())
    }
    fn write(&mut self) -> Result<(), String> {
        let captures = self.captures.take().ok_or("The editing session was already written")?;
        let disk = self.disk.as_ref().ok_or("The editing session is unavailable")?;
        let mut disk = disk.lock().unwrap_or_else(|e| e.into_inner());
        disk.finish_restore()?;
        if self.sequence <= disk.published_sequence { return Err("A newer editing session was already saved".into()); }
        match SessionManifest::read(&disk.path.join("window.json"))? {
            Some(manifest) if manifest.generation >= disk.manifest.generation => disk.manifest = manifest,
            Some(_) => return Err("The saved editing session changed unexpectedly".into()),
            None if disk.manifest.generation != 0 => return Err("The saved editing session is missing; its drawings have been preserved".into()),
            None => {},
        }
        let cancel = AtomicBool::new(false);
        let drawings = captures.iter().map(|(id,_,_)|Ok(SessionDrawing {id:*id,key:disk.stores.bind(*id,None)?.key().to_owned()})).collect::<Result<Vec<_>,String>>()?;
        let mut manifest = disk.manifest.clone();
        for id in manifest.restoring.clone() { manifest = manifest.finish_restore(id, true)?; }
        let retiring: Vec<_> = disk.manifest.drawings.iter().filter(|drawing| drawing.id==self.exclusion
            || self.exclusion==u64::MAX && !disk.manifest.blocked.contains(&drawing.id)).map(|drawing|drawing.key.clone()).collect();
        for key in retiring {
            disk.stores.prepare_retirement(&key)?.wait_on_worker()?;
        }
        manifest = manifest.stage(drawings.clone(), self.active)?;
        manifest.publish(&disk.path.join("window.json"))?;
        disk.manifest = manifest;
        disk.published_sequence = self.sequence;
        for (id, capture,stamp) in captures {
            if disk.stores.checkpoint_due(id,stamp.clone(),true)? {disk.stores.checkpoint(id,capture,stamp,true)?.wait_on_worker()?;}
        }
        let mut manifest = disk.manifest.clone();
        if self.exclusion == u64::MAX {
            for id in manifest.drawings.iter().filter(|drawing| !manifest.blocked.contains(&drawing.id)).map(|drawing| drawing.id).collect::<Vec<_>>() { manifest = manifest.remove(id)?; }
        } else if self.exclusion != 0 { manifest = manifest.remove(self.exclusion)?; }
        let manifest = manifest.checkpoint(drawings, self.active, self.clean_exit)?;
        if self.exclusion != 0 {
            if let Err(error) = manifest.publish_checked(&disk.path.join("window.json")) {
                if error.published {
                    disk.manifest = manifest;
                    disk.published_sequence = self.sequence;
                    self.committed = true;
                }
                return Err(error.error);
            }
        } else { manifest.publish(&disk.path.join("window.json"))?; }
        disk.manifest = manifest.clone();
        disk.published_sequence = self.sequence;
        self.committed = true;
        if self.exclusion != 0 && self.exclusion != u64::MAX { return Ok(()); }
        for (_, binding) in disk.stores.bindings().into_iter().filter(|(id, _)| !manifest.drawings.iter().any(|drawing| drawing.id == *id)) {
            if let Ok(reply) = disk.stores.retire_copy(binding.key()) { let _ = reply.wait_on_worker(); }
        }
        pollster::block_on(collect_unreferenced_stores(&disk.path, &manifest.drawings.iter().map(|drawing| drawing.key.clone()).collect(), &cancel));
        Ok(())
    }
}

/// # Safety
/// Serial owner; sessions and scene are UTF-8 and the returned job is owned by
/// the file worker. `adopt` lets a scene without drawings take over another
/// unlocked session that has some.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_session_open(app: u64, sessions: *const c_char, scene: *const c_char,
    adopt: bool, retry: bool) -> u64 {
    let Some(mut lease_a)=handles::app(app) else { return 0; };
    let a=&mut *lease_a;
    let opened=a.perform(|a| {
        let open = SessionOpen { sessions: PathBuf::from(unsafe { project::read_title(sessions) }?),
            scene: unsafe { project::read_title(scene) }?.to_owned(), adopt };
        let environment = OpenEnvironment::capture(&a.host.session,
            a.window.documents.admission(&a.host.session.retained_document_tiles()), a.host.renderer_options())?;
        Ok(handles::store(CapySessionTask(Mutex::new(SessionJob {
            open: Some(open), disk: if retry { a.session_disk.clone() } else { None }, environment: Some(environment), captures: None,
            candidates: Vec::new(), active: 0, clean_exit: false, retry, stamp: a.host.session.session_stamp(), retired: Vec::new(), error: None, restored: false, adopted: false, sequence: 0, exclusion: 0, committed: false,
            warning: None, failures: Default::default(), append: retry && a.session_disk.is_some(),
        }))))
    });
    if opened.is_none() && let Some(error)=&a.error {a.host.session.storage_failed(error.to_string_lossy().into_owned());}
    opened.unwrap_or(0)
}
/// # Safety
/// Serial owner after recovery preparation; exclusion 0 retains all drawings,
/// u64::MAX removes the window, otherwise it removes one explicitly closed tab.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_session_capture(app: u64, exclusion: u64, clean_exit: bool) -> u64 {
    let Some(mut lease_a)=handles::app(app) else { return 0; };
    let a=&mut *lease_a;
    a.perform(|a| {
        let disk = a.session_disk.clone().ok_or("The editing session has not finished restoring")?;
        a.session_capture_sequence = a.session_capture_sequence.checked_add(1).ok_or("Editing session capture sequence exhausted")?;
        let order: Vec<_> = a.window.documents.order().iter().copied().filter(|id| exclusion != u64::MAX && *id != exclusion).collect();
        let active = if order.contains(&a.window.documents.selected()) { a.window.documents.selected() } else { order.first().copied().unwrap_or(0) };
        let captures = order.into_iter().map(|id| {let session=a.window.session(&a.host,id)?;Ok((id,session.capture_session()?,session.session_stamp()))}).collect::<Result<Vec<_>,String>>()?;
        Ok(handles::store(CapySessionTask(Mutex::new(SessionJob {
            open: None, disk: Some(disk), environment: None, captures: Some(captures), candidates: Vec::new(), active, clean_exit, retry: false,
            stamp: a.host.session.session_stamp(), retired: Vec::new(), error: None, restored: false, adopted: false, sequence: a.session_capture_sequence, exclusion, committed: false,
            warning: None, failures: Default::default(), append: false,
        }))))
    }).unwrap_or(0)
}
/// # Safety
/// File worker only; no concurrent calls use this task. The observer transfers
/// a fingerprint handle from capy_session_destination_read or returns zero.
/// Its context remains valid until this call returns, including on the worker.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_session_work(task: u64, observe: Option<SessionDestinationObserver>, context: *mut std::ffi::c_void) -> i32 {
    let Some(lease_task)=handles::shared::<CapySessionTask>(task) else { return -1; };
    let task=&*lease_task;
    let context = context as usize;
    on_large_stack("capy-session", || {
        let mut job = task.0.lock().unwrap_or_else(|e| e.into_inner());
        let result = catch_unwind(AssertUnwindSafe(|| if job.open.is_some() { job.load(observe, context as *mut std::ffi::c_void) } else { job.write() }));
        match result {
            Ok(Ok(())) => 0,
            error => {
                let message = match error { Ok(Err(e)) => e, Err(payload) => layer_host::panic_diagnostic(&*payload), Ok(Ok(())) => unreachable!() };
                if let Some(environment)=&job.environment {environment.storage_failed(message.clone());}
                job.error = CString::new(message.replace('\0', " ")).ok(); -1
            }
        }
    }).unwrap_or_else(|error| {
        task.0.lock().unwrap_or_else(|e| e.into_inner()).error = CString::new(error.replace('\0', " ")).ok();
        -1
    })
}
/// # Safety
/// File worker only. The borrowed descriptor is exclusively readable at offset
/// zero for this call; the returned allocation transfers to the session observer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_session_destination_read(fd: i32) -> u64 {
    use std::os::fd::BorrowedFd;
    if fd < 0 { return 0; }
    let result = unsafe { BorrowedFd::borrow_raw(fd) }.try_clone_to_owned().ok()
        .and_then(|fd| layer_ui::DestinationFingerprint::read(std::fs::File::from(fd)).ok());
    result.map_or(0, |fingerprint| handles::mutable(CapySessionDestination(fingerprint)))
}
/// # Safety
/// Releases an observation not transferred to capy_session_work.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_session_destination_free(fingerprint: u64) {
    handles::free::<CapySessionDestination>(fingerprint);
}
/// # Safety
/// Serial owner after successful file work; no input may replace the launch drawing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_session_adopt(app: u64, task: u64) -> i32 {
    let (Some(mut lease_a),Some(lease_task))=(handles::app(app),handles::shared::<CapySessionTask>(task)) else { return -1; };
    let (a,task)=(&mut *lease_a,&*lease_task);
    a.perform(|a| {
        let mut job = task.0.lock().unwrap_or_else(|e| e.into_inner());
        if job.adopted { return Ok(()); }
        if let Some(error) = &job.error { return Err(error.to_string_lossy().into_owned()); }
        let disk = job.disk.clone().ok_or("The editing session is unavailable")?;
        let original = disk.lock().unwrap_or_else(|e| e.into_inner()).manifest.clone();
        let replace = !job.append && a.window.documents.order().len() == 1
            && a.host.session.can_replace_startup_session(&job.stamp);
        let live: Vec<_> = a.window.documents.order().iter().copied().filter(|id| !job.append
            || !original.drawings.iter().any(|drawing| drawing.id == *id)
            || original.blocked.contains(id) || job.candidates.iter().any(|(candidate, _)| candidate == id)).collect();
        let manifest = if replace && job.restored { original.clone() }
            else { original.reserve_live_identities(&live)? };
        let mapping: BTreeMap<_, _> = original.drawings.iter().zip(&manifest.drawings).map(|(old, new)| (old.id, new.id)).collect();
        for (id, _) in &mut job.candidates { *id = mapping[id]; }
        let reserved: Vec<_> = manifest.drawings.iter().map(|drawing| drawing.id).collect();
        a.window.documents.reserve_identities(&reserved)?;
        if job.restored {
            if !replace {
                let (_, retired) = a.window.append_restored_sessions(&mut a.host, &mut job.candidates, |session| session)?;
                job.retired = retired;
            } else {
                let active = job.active;
                let stamp = job.stamp.clone();
                job.retired = a.window.restore_sessions(&mut a.host, &mut job.candidates, active, stamp, |session| session)?;
            }
            a.document_retired();
            a.metal.document_changed();
        }
        a.window.documents.reserve_identities(&reserved)?;
        {
            let mut disk = disk.lock().unwrap_or_else(|e| e.into_inner());
            disk.stores.remap(&mapping.iter().map(|(&old,&new)|(old,new)).collect::<Vec<_>>());
            disk.unpublished = (manifest != original || !manifest.restoring.is_empty()).then_some(original);
            disk.manifest = manifest;
            disk.stores.set_wake(a.host.wake_callback());
            disk.stores.attach_window(&a.window,&a.host)?;
        }
        a.session_disk = Some(disk);
        job.adopted = true;
        a.host.invalidate_snapshot();
        Ok(())
    }).map_or(-1, |_| 0)
}
/// # Safety
/// File worker after successful adoption; leaves failed publication retryable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_session_restore_finished(task: u64) -> i32 {
    let Some(lease_task)=handles::shared::<CapySessionTask>(task) else { return -1; };
    let task=&*lease_task;
    let mut job = task.0.lock().unwrap_or_else(|e| e.into_inner());
    let result = (|| -> Result<(),String> {
        if !job.adopted { return Err("The editing session was not adopted".into()); }
        let mut disk = job.disk.as_ref().ok_or("The editing session is unavailable")?.lock().unwrap_or_else(|e| e.into_inner());
        disk.finish_restore()?;
        Ok(())
    })();
    match result { Ok(()) => 0, Err(error) => { job.error = CString::new(error.replace('\0', " ")).ok(); -1 } }
}
/// # Safety
/// Task must remain alive for this call; release the result with capy_apple_string_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_session_error(task: u64) -> u64 {
    handles::shared::<CapySessionTask>(task).and_then(|task| {
        let job = task.0.lock().unwrap_or_else(|e| e.into_inner());
        job.error.clone().or_else(|| job.warning.clone())
    })
        .map_or(0, handles::text)
}
/// # Safety
/// Worker completion must precede this read. Release the owned JSON string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_session_failures(task: u64) -> u64 {
    handles::shared::<CapySessionTask>(task).and_then(|task| {
        let job = task.0.lock().unwrap_or_else(|e| e.into_inner());
        let disk = job.disk.as_ref()?.lock().unwrap_or_else(|e| e.into_inner());
        let failures: Vec<_> = disk.manifest.drawings.iter().filter(|drawing| disk.manifest.blocked.contains(&drawing.id))
            .filter_map(|drawing| job.failures.get(&drawing.key).map(|error| serde_json::json!({"id": drawing.id, "error": error}))).collect();
        serde_json::to_string(&failures).ok().and_then(|json| CString::new(json).ok())
    }).map_or(0, handles::text)
}
/// # Safety
/// Worker completion must precede this read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_session_committed(task: u64) -> bool {
    handles::shared::<CapySessionTask>(task).is_some_and(|task| task.0.lock().unwrap_or_else(|e| e.into_inner()).committed)
}
/// # Safety
/// No outstanding task calls. Free on the file worker.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_session_free(task: u64) {
    handles::free::<CapySessionTask>(task);
}

#[unsafe(no_mangle)]
pub extern "C" fn capy_session_checkpoint_interval_ms()->u64 {SessionStores::checkpoint_interval_ms()}

#[unsafe(no_mangle)]
pub extern "C" fn capy_session_set_background(background:bool) {layer_core::package::session_store::CompactionActivity::set_background(background);}
