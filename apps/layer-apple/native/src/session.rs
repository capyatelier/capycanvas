use super::*;
use layer_core::package::session_store::{SessionLease, SessionStore, collect_unreferenced_stores, preserve_directory, restore_error, set_restore_error, sync_directory};
use layer_host::open::OpenEnvironment;
use layer_ui::session_recovery::{SessionCapture, SessionDrawing, SessionManifest, SessionReadError, SessionRestore};
use std::{collections::BTreeMap, path::{Component, Path, PathBuf}, sync::{Arc, Mutex, atomic::AtomicBool}};

pub(crate) struct WindowDisk {
    path: PathBuf,
    _owner: SessionLease,
    stores: BTreeMap<u64, SessionStore>,
    metadata: BTreeMap<u64, Vec<u8>>,
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
pub type SessionDestinationObserver = unsafe extern "C" fn(*const c_char) -> *mut CapySessionDestination;
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
fn claim(open: SessionOpen) -> Result<(PathBuf, SessionLease, Option<String>), String> {
    let mut parts = Path::new(&open.scene).components();
    if !matches!((parts.next(), parts.next()), (Some(Component::Normal(_)), None)) {
        return Err("Invalid editing session".into());
    }
    let own = open.sessions.join(&open.scene);
    let lease = SessionLease::claim(&own)?.ok_or("This editing session is already open")?;
    match has_drawings(&own) {
        Err(SessionReadError::Invalid(error)) => {
            let preserved = preserve_directory(&own)?;
            let owner = SessionLease::claim(&own)?.ok_or("This editing session is already open")?;
            return Ok((own, owner, Some(format!("Could not read the saved drawing list: {error}\nOriginal files preserved at {}", preserved.display()))));
        }
        Err(error) => return Err(error.to_string()),
        Ok(drawings) if !open.adopt || drawings => return Ok((own, lease, None)),
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
        preserve_directory(&own)?;
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
    captures: Option<Vec<(u64, SessionCapture)>>,
    candidates: Vec<(u64, Box<layer_ui::UiSession<layer_host::Renderer>>)>,
    active: u64,
    clean_exit: bool,
    retry: bool,
    append: bool,
    stamp: layer_ui::SessionStamp,
    retired: Vec<Box<layer_render_wgpu::WgpuRasterizer>>,
    error: Option<CString>,
    warning: Option<CString>,
    restored: bool,
    adopted: bool,
    sequence: u64,
    exclusion: u64,
    committed: bool,
}
impl SessionJob {
    fn manifest_path(&self) -> Result<PathBuf, String> {
        self.disk.as_ref().map(|disk| disk.lock().unwrap_or_else(|e| e.into_inner()).path.join("window.json"))
            .ok_or_else(|| "The editing session is unavailable".into())
    }
    fn load(&mut self, observe: Option<SessionDestinationObserver>) -> Result<(), String> {
        let open = self.open.take().ok_or("Session storage is unavailable")?;
        if self.disk.is_none() {
            let (path, owner, warning) = claim(open)?;
            self.warning = warning.and_then(|text| CString::new(text.replace('\0', " ")).ok());
            self.disk = Some(Arc::new(Mutex::new(WindowDisk { path, _owner: owner, stores: BTreeMap::new(), metadata: BTreeMap::new(), manifest: SessionManifest::default(), unpublished: None, published_sequence: 0 })));
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
        let environment = self.environment.as_ref().ok_or("The canvas is unavailable")?;
        let disk = self.disk.as_ref().unwrap();
        let mut disk = disk.lock().unwrap_or_else(|e| e.into_inner());
        disk.manifest = manifest.clone();
        let cancel = AtomicBool::new(false);
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
        if !self.append {
            if !self.candidates.iter().any(|(id, _)| *id == self.active) {
                self.active = self.candidates.first().map_or(0, |(id, _)| *id);
            }
        }
        let warnings: Vec<_> = failures.into_iter().map(|(key, error)| format!("Drawing {key}: {error}")).collect();
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
        let cleanup = (|| -> Result<(), String> {
            let removed: Vec<_> = disk.stores.keys().copied().filter(|id| !manifest.drawings.iter().any(|d| d.id == *id)).collect();
            for id in removed {
                disk.stores.get_mut(&id).unwrap().retire()?;
                disk.stores.remove(&id);
                disk.metadata.remove(&id);
            }
            collect_unreferenced_stores(&disk.path, &manifest.drawings.iter().map(|drawing| drawing.key.clone()).collect(), &cancel)?;
            Ok(())
        })();
        if let Err(error) = cleanup {
            self.warning = CString::new(format!("Could not clean retired recovery files: {error}").replace('\0', " ")).ok();
        }
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
            candidates: Vec::new(), active: 0, clean_exit: false, retry, stamp: a.host.session.session_stamp(), retired: Vec::new(), error: None, restored: false, adopted: false, sequence: 0, exclusion: 0, committed: false,
            warning: None, append: retry && a.session_disk.is_some(),
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
            stamp: a.host.session.session_stamp(), retired: Vec::new(), error: None, restored: false, adopted: false, sequence: a.session_capture_sequence, exclusion, committed: false,
            warning: None, append: false,
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
                let message = match error { Ok(Err(e)) => e, Err(payload) => panic_diagnostic(payload), Ok(Ok(())) => unreachable!() };
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
        a.window.documents.reserve_identities(&reserved)?;
        {
            let mut disk = disk.lock().unwrap_or_else(|e| e.into_inner());
            disk.stores = std::mem::take(&mut disk.stores).into_iter().map(|(id, store)| (mapping.get(&id).copied().unwrap_or(id), store)).collect();
            disk.metadata = std::mem::take(&mut disk.metadata).into_iter().map(|(id, value)| (mapping.get(&id).copied().unwrap_or(id), value)).collect();
            disk.unpublished = (manifest != original || !manifest.restoring.is_empty()).then_some(original);
            disk.manifest = manifest;
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
pub unsafe extern "C" fn capy_session_restore_finished(task: *mut CapySessionTask) -> i32 {
    let Some(task) = (unsafe { task.as_ref() }) else { return -1; };
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
