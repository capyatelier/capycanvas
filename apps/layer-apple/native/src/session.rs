use super::*;
use layer_core::package::session_store::{SessionLease, SessionStore, collect_unreferenced_stores, sync_directory};
use layer_host::open::OpenEnvironment;
use layer_ui::session_recovery::{SessionCapture, SessionDrawing, SessionManifest, SessionRestore};
use std::{collections::BTreeMap, path::{Component, Path, PathBuf}, sync::{Arc, Mutex, atomic::AtomicBool}};

pub(crate) struct WindowDisk {
    path: PathBuf,
    _owner: SessionLease,
    stores: BTreeMap<u64, SessionStore>,
    metadata: BTreeMap<u64, Vec<u8>>,
    manifest: SessionManifest,
    published_sequence: u64,
}
pub struct CapySessionTask(Mutex<SessionJob>);
pub struct CapySessionDestination(layer_ui::DestinationFingerprint);
pub type SessionDestinationObserver = unsafe extern "C" fn(*const c_char) -> *mut CapySessionDestination;
struct SessionOpen {
    sessions: PathBuf,
    scene: String,
    adopt: bool,
}
fn has_drawings(directory: &Path) -> Result<bool, String> {
    Ok(SessionManifest::read(&directory.join("window.json"))?.is_some_and(|manifest| !manifest.drawings.is_empty()))
}
/// The scene's own session or, when it has no drawings and `adopt` is set, the
/// newest unlocked session that has drawings, renamed to the scene.
fn claim(open: SessionOpen) -> Result<(PathBuf, SessionLease), String> {
    let mut parts = Path::new(&open.scene).components();
    if !matches!((parts.next(), parts.next()), (Some(Component::Normal(_)), None)) {
        return Err("Invalid editing session".into());
    }
    let own = open.sessions.join(&open.scene);
    let lease = SessionLease::claim(&own)?.ok_or("This editing session is already open")?;
    if !open.adopt || has_drawings(&own)? { return Ok((own, lease)); }
    let mut candidates = Vec::new();
    for entry in std::fs::read_dir(&open.sessions).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path != own && let Ok(modified) = std::fs::metadata(path.join("window.json")).and_then(|metadata| metadata.modified()) {
            candidates.push((modified, path));
        }
    }
    candidates.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    for (_, candidate) in candidates {
        if !has_drawings(&candidate).unwrap_or(false) { continue; }
        let Ok(Some(adopted)) = SessionLease::claim(&candidate) else { continue };
        std::fs::remove_dir_all(&own).map_err(|e| e.to_string())?;
        std::fs::rename(&candidate, &own).map_err(|e| e.to_string())?;
        sync_directory(&open.sessions)?;
        return Ok((own, adopted));
    }
    Ok((own, lease))
}
struct SessionJob {
    open: Option<SessionOpen>,
    disk: Option<Arc<Mutex<WindowDisk>>>,
    environment: Option<OpenEnvironment>,
    captures: Option<Vec<(u64, SessionCapture)>>,
    candidates: Vec<(u64, Box<layer_ui::UiSession<layer_host::Renderer>>)>,
    active: u64,
    clean_exit: bool,
    retry: bool,
    append: bool,
    live: Vec<u64>,
    stamp: layer_ui::SessionStamp,
    retired: Vec<Box<layer_render_wgpu::WgpuRasterizer>>,
    error: Option<CString>,
    warning: Option<CString>,
    restored: bool,
    adopted: bool,
    sequence: u64,
    exclusion: u64,
    committed: bool,
    restore_attempts: Vec<layer_ui::SessionRestoreAttempt>,
}
impl SessionJob {
    fn manifest_path(&self) -> Result<PathBuf, String> {
        self.disk.as_ref().map(|disk| disk.lock().unwrap_or_else(|e| e.into_inner()).path.join("window.json"))
            .ok_or_else(|| "The editing session is unavailable".into())
    }
    fn load(&mut self, observe: Option<SessionDestinationObserver>) -> Result<(), String> {
        let open = self.open.take().ok_or("Session storage is unavailable")?;
        if self.disk.is_none() {
            let (path, owner) = claim(open)?;
            self.disk = Some(Arc::new(Mutex::new(WindowDisk { path, _owner: owner, stores: BTreeMap::new(), metadata: BTreeMap::new(), manifest: SessionManifest::default(), published_sequence: 0 })));
        }
        let manifest_path = self.manifest_path()?;
        let Some(mut manifest) = SessionManifest::read(&manifest_path)? else { return Ok(()) };
        if !manifest.restoring.is_empty() {
            manifest = manifest.recover_interrupted()?;
            manifest.publish(&manifest_path)?;
        }
        if !manifest.blocked.is_empty() {
            self.warning = CString::new("A drawing interrupted the previous restart. Its saved session has been preserved.").ok();
        }
        let recovered = !manifest.clean_exit;
        self.active = manifest.active;
        let environment = self.environment.as_ref().ok_or("The canvas is unavailable")?;
        let disk = self.disk.as_ref().unwrap();
        let mut disk = disk.lock().unwrap_or_else(|e| e.into_inner());
        disk.manifest = manifest.clone();
        let cancel = AtomicBool::new(false);
        collect_unreferenced_stores(&disk.path, &manifest.drawings.iter().map(|drawing| drawing.key.clone()).collect(), &cancel)?;
        let mut drawings = manifest.drawings.clone();
        if self.append { drawings.retain(|drawing| manifest.blocked.contains(&drawing.id)); }
        drawings.sort_by_key(|drawing| drawing.id != manifest.active);
        let mut restored = Vec::with_capacity(drawings.len());
        for drawing in drawings {
            if manifest.blocked.contains(&drawing.id) && !self.retry { continue; }
            manifest = if self.retry { manifest.retry_restore(drawing.id)? } else { manifest.begin_restore(drawing.id)? };
            manifest.publish(&manifest_path)?;
            let result = (|| -> Result<_, String> {
                let mut store = match disk.stores.remove(&drawing.id) {
                    Some(store) => store,
                    None => SessionStore::open(&disk.path.join(&drawing.key))?,
                };
                let core = store.load(environment.limits(), &cancel)?.ok_or("A saved drawing is missing; its editing session has been preserved")?;
                let restore = SessionRestore::from_core(core)?;
                environment.admit_sessions(restored.iter().map(|(_, restore, _): &(_, SessionRestore, _)| &restore.editor).chain(std::iter::once(&restore.editor)))?;
                let recovered = recovered || store.recovered_previous();
                disk.stores.insert(drawing.id, store);
                Ok((drawing.id, restore, recovered))
            })();
            match result {
                Ok(restore) => restored.push(restore),
                Err(error) => {
                    let attempt = *manifest.restoring.iter().find(|attempt| attempt.id == drawing.id).unwrap();
                    manifest = manifest.finish_restore(attempt, false)?;
                    manifest.publish(&manifest_path)?;
                    self.warning = CString::new(error.replace('\0', " ")).ok();
                }
            }
        }
        for (id, restore, recovered) in restored {
            let result = (|| -> Result<_, String> {
                let observed = restore.state.location.as_ref().and_then(|location| {
                    let uri = CString::new(location.uri.as_str()).ok()?;
                    let fingerprint = unsafe { observe?(uri.as_ptr()) };
                    if fingerprint.is_null() { None } else { Some(unsafe { Box::from_raw(fingerprint) }.0) }
                });
                let mut candidate = environment.prepare(restore.document().clone(), || false)?;
                candidate.restore_session(restore, recovered, observed)?;
                candidate.set_document_replacement(false);
                let deadline = std::time::Instant::now() + layer_host::open::PREPARE_DEADLINE;
                loop {
                    candidate.frame(0, 0)?;
                    if candidate.can_park_document() && candidate.retained_document_tiles().try_blobs()?.is_some() { break; }
                    candidate.engine().backend().0.as_ref().ok_or("The canvas is unavailable")?.device()
                        .poll(wgpu::PollType::Poll).map_err(|e| e.to_string())?;
                    if std::time::Instant::now() >= deadline { return Err("The saved drawing could not finish preparing".into()); }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                Ok(candidate)
            })();
            match result {
                Ok(candidate) => self.candidates.push((id, candidate)),
                Err(error) => {
                    let attempt = *manifest.restoring.iter().find(|attempt| attempt.id == id).unwrap();
                    manifest = manifest.finish_restore(attempt, false)?;
                    manifest.publish(&manifest_path)?;
                    self.warning = CString::new(error.replace('\0', " ")).ok();
                }
            }
        }
        self.restored = !self.candidates.is_empty();
        if !self.append {
            if !self.candidates.iter().any(|(id, _)| *id == self.active) {
                self.active = self.candidates.first().map_or(0, |(id, _)| *id);
            }
            for (id, candidate) in &mut self.candidates {
                if *id != self.active { candidate.park_document()?; drop(candidate.renderer_mut().0.take()); }
            }
            if !self.restored { manifest = manifest.reserve_live_identities(&self.live)?; disk.stores.clear(); }
        }
        if manifest.blocked.is_empty() { self.warning = None; }
        manifest = manifest.reconcile(manifest.drawings.clone(), manifest.active, false)?;
        self.candidates.sort_by_key(|(id, _)| manifest.drawings.iter().position(|drawing| drawing.id == *id).unwrap());
        manifest.publish(&manifest_path)?;
        self.restore_attempts = manifest.restoring.clone();
        disk.manifest = manifest.clone();
        Ok(())
    }
    fn write(&mut self) -> Result<(), String> {
        let captures = self.captures.take().ok_or("The editing session was already written")?;
        let disk = self.disk.as_ref().ok_or("The editing session is unavailable")?;
        let mut disk = disk.lock().unwrap_or_else(|e| e.into_inner());
        if self.sequence <= disk.published_sequence { return Err("A newer editing session was already saved".into()); }
        match SessionManifest::read(&disk.path.join("window.json"))? {
            Some(manifest) if manifest.generation >= disk.manifest.generation => disk.manifest = manifest,
            Some(_) => return Err("The saved editing session changed unexpectedly".into()),
            None if disk.manifest.generation != 0 => return Err("The saved editing session is missing; its drawings have been preserved".into()),
            None => {},
        }
        let cancel = AtomicBool::new(false);
        let drawings: Vec<_> = captures.iter().map(|(id, _)| SessionDrawing { id: *id,
            key: disk.manifest.drawings.iter().find(|drawing| drawing.id == *id).map(|drawing| drawing.key.clone())
                .unwrap_or_else(|| layer_core::PortableId::random().to_string()) }).collect();
        let mut manifest = disk.manifest.clone();
        for id in manifest.restoring.clone() { manifest = manifest.finish_restore(id, true)?; }
        let retiring: Vec<_> = disk.manifest.drawings.iter().filter(|drawing| !disk.manifest.blocked.contains(&drawing.id)
            && (self.exclusion == u64::MAX || drawing.id == self.exclusion)).map(|drawing| drawing.id).collect();
        for id in retiring {
            if !disk.stores.contains_key(&id) {
                let key = disk.manifest.drawings.iter().find(|drawing| drawing.id == id).unwrap().key.clone();
                let store = SessionStore::open(&disk.path.join(key))?;
                disk.stores.insert(id, store);
            }
            disk.stores.get_mut(&id).unwrap().prepare_retirement()?;
        }
        manifest = manifest.stage(drawings.clone(), self.active)?;
        manifest.publish(&disk.path.join("window.json"))?;
        disk.manifest = manifest;
        disk.published_sequence = self.sequence;
        for (id, capture) in captures {
            let prepared = capture.prepare(&cancel)?;
            let metadata = prepared.metadata().to_vec();
            if disk.metadata.get(&id) != Some(&metadata) {
                if !disk.stores.contains_key(&id) {
                    let key = disk.manifest.drawings.iter().find(|drawing| drawing.id == id).ok_or("The drawing has no editing session")?.key.clone();
                    let store = SessionStore::open(&disk.path.join(key))?;
                    disk.stores.insert(id, store);
                }
                disk.stores.get_mut(&id).unwrap().commit(&prepared, &cancel)?;
                disk.metadata.insert(id, metadata);
            }
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
        let removed: Vec<_> = disk.stores.keys().copied().filter(|id| !manifest.drawings.iter().any(|d| d.id == *id)).collect();
        for id in removed {
            disk.stores.get_mut(&id).unwrap().retire()?;
            disk.stores.remove(&id);
            disk.metadata.remove(&id);
        }
        collect_unreferenced_stores(&disk.path, &manifest.drawings.iter().map(|drawing| drawing.key.clone()).collect(), &cancel)?;
        Ok(())
    }
}

/// # Safety
/// Serial owner; sessions and scene are UTF-8 and the returned job is owned by
/// the file worker. `adopt` lets a scene without drawings take over another
/// unlocked session that has some.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_session_open(app: *mut CapyApple, sessions: *const c_char, scene: *const c_char,
    adopt: bool, retry: bool) -> *mut CapySessionTask {
    let Some(a) = (unsafe { app.as_mut() }) else { return std::ptr::null_mut(); };
    a.perform(|a| {
        let open = SessionOpen { sessions: PathBuf::from(unsafe { project::read_title(sessions) }?),
            scene: unsafe { project::read_title(scene) }?.to_owned(), adopt };
        let environment = OpenEnvironment::capture(&a.host.session,
            a.window.documents.admission(&a.host.session.retained_document_tiles()), a.host.renderer_options(a.metal.cache.clone()))?;
        Ok(Box::into_raw(Box::new(CapySessionTask(Mutex::new(SessionJob {
            open: Some(open), disk: if retry { a.session_disk.clone() } else { None }, environment: Some(environment), captures: None,
            candidates: Vec::new(), active: 0, clean_exit: false, retry, stamp: a.host.session.session_stamp(), retired: Vec::new(), error: None, restored: false, adopted: false, sequence: 0, exclusion: 0, committed: false, restore_attempts: Vec::new(),
            warning: None, append: retry && a.session_disk.is_some(), live: a.window.documents.order().to_vec(),
        })))))
    }).unwrap_or(std::ptr::null_mut())
}
/// # Safety
/// Serial owner after recovery preparation; exclusion 0 retains all drawings,
/// u64::MAX removes the window, otherwise it removes one explicitly closed tab.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_session_capture(app: *mut CapyApple, exclusion: u64, clean_exit: bool) -> *mut CapySessionTask {
    let Some(a) = (unsafe { app.as_mut() }) else { return std::ptr::null_mut(); };
    a.perform(|a| {
        let disk = a.session_disk.clone().ok_or("The editing session has not finished restoring")?;
        a.session_capture_sequence = a.session_capture_sequence.checked_add(1).ok_or("Editing session capture sequence exhausted")?;
        let order: Vec<_> = a.window.documents.order().iter().copied().filter(|id| exclusion != u64::MAX && *id != exclusion).collect();
        let active = if order.contains(&a.window.documents.selected()) { a.window.documents.selected() } else { order.first().copied().unwrap_or(0) };
        let captures = order.into_iter().map(|id| Ok((id, a.window.session(&a.host, id)?.capture_session()?))).collect::<Result<Vec<_>,String>>()?;
        Ok(Box::into_raw(Box::new(CapySessionTask(Mutex::new(SessionJob {
            open: None, disk: Some(disk), environment: None, captures: Some(captures), candidates: Vec::new(), active, clean_exit, retry: false,
            stamp: a.host.session.session_stamp(), retired: Vec::new(), error: None, restored: false, adopted: false, sequence: a.session_capture_sequence, exclusion, committed: false, restore_attempts: Vec::new(),
            warning: None, append: false, live: Vec::new(),
        })))))
    }).unwrap_or(std::ptr::null_mut())
}
/// # Safety
/// File worker only; no concurrent calls use this task. The observer transfers
/// a fingerprint allocation from capy_session_destination_read or returns null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_session_work(task: *mut CapySessionTask, observe: Option<SessionDestinationObserver>) -> i32 {
    let Some(task) = (unsafe { task.as_ref() }) else { return -1; };
    on_large_stack("capy-session", || {
        let mut job = task.0.lock().unwrap_or_else(|e| e.into_inner());
        let result = catch_unwind(AssertUnwindSafe(|| if job.open.is_some() { job.load(observe) } else { job.write() }));
        match result {
            Ok(Ok(())) => 0,
            error => {
                let message = match error { Ok(Err(e)) => e, _ => "Editing session storage failed".into() };
                job.error = CString::new(message.replace('\0', " ")).ok(); -1
            }
        }
    }).unwrap_or(-1)
}
/// # Safety
/// File worker only. The borrowed descriptor is exclusively readable at offset
/// zero for this call; the returned allocation transfers to the session observer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_session_destination_read(fd: i32) -> *mut CapySessionDestination {
    use std::os::fd::BorrowedFd;
    if fd < 0 { return std::ptr::null_mut(); }
    let result = unsafe { BorrowedFd::borrow_raw(fd) }.try_clone_to_owned().ok()
        .and_then(|fd| layer_ui::DestinationFingerprint::read(std::fs::File::from(fd)).ok());
    result.map_or(std::ptr::null_mut(), |fingerprint| Box::into_raw(Box::new(CapySessionDestination(fingerprint))))
}
/// # Safety
/// Releases an observation not transferred to capy_session_work.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_session_destination_free(fingerprint: *mut CapySessionDestination) {
    if !fingerprint.is_null() { drop(unsafe { Box::from_raw(fingerprint) }); }
}
/// # Safety
/// Serial owner after successful file work; no input may replace the launch drawing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_session_adopt(app: *mut CapyApple, task: *mut CapySessionTask) -> i32 {
    let (Some(a), Some(task)) = (unsafe { app.as_mut() }, unsafe { task.as_ref() }) else { return -1; };
    a.perform(|a| {
        let mut job = task.0.lock().unwrap_or_else(|e| e.into_inner());
        if job.error.is_some() { return Err("The saved editing session could not be restored".into()); }
        let manifest = job.disk.as_ref().ok_or("The editing session is unavailable")?
            .lock().unwrap_or_else(|e| e.into_inner()).manifest.clone();
        if (!job.restored && manifest.blocked.iter().any(|id| a.window.documents.order().contains(id)))
            || (job.append && job.candidates.iter().any(|(id, _)| a.window.documents.order().contains(id))) {
            return Err(layer_ui::DocumentTransportRefusal::SnapshotChanged.message(a.host.session.localization()).to_string());
        }
        if job.restored {
            if job.append {
                let (_, retired) = a.window.append_restored_sessions(&mut a.host, &mut job.candidates, Box::new)?;
                job.retired = retired;
            } else {
                let active = job.active;
                let stamp = job.stamp.clone();
                job.retired = a.window.restore_sessions(&mut a.host, &mut job.candidates, active, stamp, Box::new)?;
            }
            a.document_retired();
            a.metal.document_changed();
        }
        let reserved: Vec<_> = manifest.drawings.iter().map(|drawing| drawing.id).collect();
        a.window.documents.reserve_identities(&reserved)?;
        a.session_disk = job.disk.clone();
        job.adopted = true;
        a.host.invalidate_snapshot();
        Ok(())
    }).map_or(-1, |_| 0)
}
/// # Safety
/// File worker after successful adoption; leaves failed publication retryable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_session_restore_finished(task: *mut CapySessionTask) -> i32 {
    let Some(task) = (unsafe { task.as_ref() }) else { return -1; };
    let mut job = task.0.lock().unwrap_or_else(|e| e.into_inner());
    let result = (|| -> Result<(),String> {
        if !job.adopted { return Err("The editing session was not adopted".into()); }
        let mut disk = job.disk.as_ref().ok_or("The editing session is unavailable")?.lock().unwrap_or_else(|e| e.into_inner());
        if !disk.manifest.restoring.is_empty() {
            let mut manifest = disk.manifest.clone();
            for attempt in job.restore_attempts.clone() { manifest = manifest.finish_restore(attempt, true)?; }
            manifest.publish(&disk.path.join("window.json"))?;
            disk.manifest = manifest;
        }
        Ok(())
    })();
    match result { Ok(()) => 0, Err(error) => { job.error = CString::new(error.replace('\0', " ")).ok(); -1 } }
}
/// # Safety
/// Task must remain alive for this call; release the result with capy_apple_string_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_session_error(task: *const CapySessionTask) -> *mut c_char {
    unsafe { task.as_ref() }.and_then(|task| {
        let job = task.0.lock().unwrap_or_else(|e| e.into_inner());
        job.error.clone().or_else(|| job.warning.clone())
    })
        .map_or(std::ptr::null_mut(), CString::into_raw)
}
/// # Safety
/// Worker completion must precede this read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_session_committed(task: *const CapySessionTask) -> bool {
    unsafe { task.as_ref() }.is_some_and(|task| task.0.lock().unwrap_or_else(|e| e.into_inner()).committed)
}
/// # Safety
/// No outstanding task calls. Free on the file worker.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_session_free(task: *mut CapySessionTask) {
    if !task.is_null() { drop(unsafe { Box::from_raw(task) }); }
}
