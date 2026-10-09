use crate::{document_io::io_error, documents::recovery_environment};
use layer_core::package::session_store::{OwnerLock, SessionStore, create_directory,collect_unreferenced_stores,prepare_store_retirement,restore_error,set_restore_error};
use layer_host::{
    NativeHost, Renderer,
    window::{DocumentWindow, Parked},
};
use layer_ui::{
    DestinationFingerprint, UiSession,
    session_recovery::{
        SessionCapture, SessionDrawing, SessionManifest, SessionRestore, SessionRestoreAttempt, SessionStamp,
    },
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
static NEXT: AtomicU64 = AtomicU64::new(1);
static LAUNCHED: AtomicBool = AtomicBool::new(false);
fn key() -> String {
    format!(
        "{:x}-{:x}-{:x}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}
struct Storage {
    directory: PathBuf,
    _lease: OwnerLock,
    manifest: SessionManifest,
    stores: BTreeMap<String, SessionStore>,
    warnings: Vec<String>,
}
impl Storage {
    fn create(root: &std::path::Path) -> Result<Self, String> {
        let directory = root.join(key());
        create_directory(&directory)?;
        let lease = OwnerLock::claim(&directory.join("owner.lock"))?
            .ok_or("Session storage is already open")?;
        Ok(Self {
            directory,
            _lease: lease,
            manifest: SessionManifest {
                generation: 0,
                drawings: vec![],
                active: 0,
                clean_exit: false,
                restoring: vec![],
                blocked: vec![],
            },
            stores: BTreeMap::new(),
            warnings: Vec::new(),
        })
    }
    fn open(root: &std::path::Path) -> Result<(Self, usize), String> {
        create_directory(root)?;
        let mut candidates = Vec::new();
        for entry in fs::read_dir(root).map_err(|e| io_error("list sessions", e))? {
            let entry = entry.map_err(|e| io_error("list sessions", e))?;
            if entry
                .file_type()
                .map_err(|e| io_error("inspect session", e))?
                .is_dir()
            {
                candidates.push((
                    entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(UNIX_EPOCH),
                    entry.path(),
                ));
            }
        }
        let mut warnings=Vec::new();
        candidates.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
        for (_,directory) in &candidates {
            let Some(_lease)=OwnerLock::claim(&directory.join("owner.lock"))? else{continue};
            if matches!(SessionManifest::read(&directory.join("session.json")),Ok(Some(manifest)) if manifest.drawings.is_empty())
                && let Err(error)=collect_unreferenced_stores(directory,&Default::default(),&AtomicBool::new(false)){warnings.push(error);}
        }
        let mut claimed = None;
        let mut others = 0;
        for (_, directory) in candidates {
            let Some(lease) = OwnerLock::claim(&directory.join("owner.lock"))? else {
                continue;
            };
            let path = directory.join("session.json");
            if claimed.is_some() {
                others += usize::from(matches!(SessionManifest::read(&path), Ok(Some(manifest)) if !manifest.drawings.is_empty()));
                continue;
            }
            let Some(manifest)=SessionManifest::read(&path)? else {continue};
            if manifest.drawings.is_empty() {
                continue;
            }
            let mut storage = Self {
                directory,
                _lease: lease,
                manifest,
                stores: BTreeMap::new(),
            warnings: Vec::new(),
            };
            if !storage.manifest.restoring.is_empty() {
                let manifest = storage.manifest.recover_interrupted()?;
                storage.publish(manifest, &AtomicBool::new(false))?;
            }
            storage.collect(&AtomicBool::new(false))?;
            claimed = Some(storage);
        }
        let mut storage=match claimed {Some(storage)=>storage,None=>Self::create(root)?};
        storage.warnings.extend(warnings);Ok((storage,others))
    }
    fn publish(
        &mut self,
        manifest: SessionManifest,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        manifest.validate()?;
        if cancel.load(Ordering::Acquire) {
            return Err("Session operation cancelled".into());
        }
        match manifest.publish_checked(&self.directory.join("session.json")) {
            Ok(())=>{self.manifest=manifest;Ok(())},
            Err(failure)=>{if failure.published{self.manifest=manifest;}Err(failure.error)},
        }
    }
    fn store(&mut self,_id:u64,store_key:&str)->Result<&mut SessionStore,String>{
        if !self.stores.contains_key(store_key){self.stores.insert(store_key.into(),SessionStore::open(&self.directory.join(store_key))?);}
        Ok(self.stores.get_mut(store_key).unwrap())
    }
    fn collect(&mut self,cancel:&AtomicBool)->Result<(),String>{
        let result=(||{
            let reachable=self.manifest.drawings.iter().map(|drawing|drawing.key.clone()).collect::<std::collections::BTreeSet<_>>();
            for key in self.stores.keys().filter(|key|!reachable.contains(*key)).cloned().collect::<Vec<_>>() {self.stores.get_mut(&key).unwrap().retire()?;self.stores.remove(&key);}
            collect_unreferenced_stores(&self.directory,&reachable,cancel)?;Ok(())
        })();
        match result {Err(error) if !cancel.load(Ordering::Acquire)=>{self.warnings.push(error);Ok(())},result=>result}
    }
    fn begin_restore(&mut self,id:u64,retry:bool,cancel:&AtomicBool)->Result<SessionRestoreAttempt,String>{
        let manifest=if retry {self.manifest.retry_restore(id)?} else {self.manifest.begin_restore(id)?};self.publish(manifest,cancel)?;
        self.manifest.restoring.iter().find(|attempt|attempt.id==id).copied().ok_or_else(||"Session restore attempt is missing".into())
    }
    fn complete_restores(&mut self,attempts:&[(SessionRestoreAttempt,Result<(),String>)],mapping:&[(u64,u64)],cancel:&AtomicBool)->Result<(),String>{
        let mut manifest=self.manifest.clone();
        let mut diagnostics=Vec::new();
        for (attempt,result) in attempts {
            manifest=manifest.finish_restore(*attempt,result.is_ok())?;
            let drawing=self.manifest.drawings.iter().find(|drawing|drawing.id==attempt.id).ok_or("Session drawing is missing")?;
            diagnostics.push((self.directory.join(&drawing.key),result));
        }
        let manifest=manifest.remap(mapping)?;
        if cancel.load(Ordering::Acquire){return Err("Session operation cancelled".into());}
        for (path,result) in &diagnostics {
            if let Err(error)=result && let Err(diagnostic)=set_restore_error(path,Some(error)) {self.warnings.push(format!("{error}\n{diagnostic}"));}
        }
        self.publish(manifest,cancel)?;
        for (path,result) in diagnostics {
            if result.is_ok() && let Err(error)=set_restore_error(&path,None){self.warnings.push(error);}
        }
        Ok(())
    }
    fn read(&mut self,drawing:&SessionDrawing,limits:layer_core::ProjectLimits,cancel:&AtomicBool)->Result<(SessionRestore,bool),String>{
        let store=self.store(drawing.id,&drawing.key)?;
        let opened=store.load(limits,cancel)?.ok_or("Session drawing is missing")?;
        let recovered=store.recovered_previous();
        Ok((SessionRestore::from_core(opened)?,recovered))
    }
    fn discard(&mut self,id:u64,cancel:&AtomicBool)->Result<(),String>{
        let Some(drawing)=self.manifest.drawings.iter().find(|drawing|drawing.id==id).cloned() else {return Ok(())};
        self.stores.remove(&drawing.key);prepare_store_retirement(&self.directory.join(&drawing.key))?;
        let manifest=self.manifest.remove(id)?;self.publish(manifest,cancel)?;self.collect(cancel)
    }
    fn capture(&mut self,manifest:SessionManifest,captures:Vec<(u64,SessionCapture)>,cancel:&AtomicBool)->Result<(),String>{
        if manifest.generation!=self.manifest.generation {return Err("Stale session checkpoint membership".into());}
        if manifest.drawings.iter().any(|drawing|!self.manifest.drawings.iter().any(|previous|previous.key==drawing.key)){let staged=self.manifest.stage(manifest.drawings.clone(),manifest.active)?;self.publish(staged,cancel)?;}
        for (id,capture) in captures {let drawing=manifest.drawings.iter().find(|d|d.id==id).ok_or("Session drawing is missing")?;let prepared=capture.prepare(cancel)?;self.store(id,&drawing.key)?.commit(&prepared,cancel)?;}
        let retired=self.manifest.drawings.iter().filter(|d|!manifest.drawings.iter().any(|live|live.key==d.key)).cloned().collect::<Vec<_>>();
        for drawing in &retired{self.store(drawing.id,&drawing.key)?.prepare_retirement()?;}
        let mut committed=self.manifest.clone();for drawing in &retired{committed=committed.remove(drawing.id)?;}
        let committed=committed.reconcile(manifest.drawings,manifest.active,manifest.clean_exit)?;self.publish(committed,cancel)?;
        self.collect(cancel)?;
        Ok(())
    }
    fn decode(&mut self,limits:layer_core::ProjectLimits,admit:impl Fn(Vec<&layer_core::Editor>)->Result<(),String>,cancel:&AtomicBool)->Result<Decoded,String>{
        let recovered=!self.manifest.clean_exit;
        let mut drawings=self.manifest.drawings.clone();
        drawings.sort_by_key(|drawing|drawing.id!=self.manifest.active);
        let mut decoded=Decoded::default();
        for drawing in drawings {
            if self.manifest.blocked.contains(&drawing.id) {
                match restore_error(&self.directory.join(&drawing.key)) {Ok(Some(error))|Err(error)=>decoded.errors.push(error),Ok(None)=>{}}
                decoded.failed.push(drawing.key);continue;
            }
            let attempt=self.begin_restore(drawing.id,false,cancel)?;
            let result=self.read(&drawing,limits,cancel).and_then(|(restore,previous)|{
                admit(decoded.drawings.iter().map(|(_,_,restore,_)|&restore.editor).chain(std::iter::once(&restore.editor)).collect())?;Ok((restore,previous))
            });
            match result {
                Ok((restore,previous))=>decoded.drawings.push((drawing.id,attempt,restore,recovered||previous)),
                Err(error)=>{if cancel.load(Ordering::Acquire){return Err(error);}self.complete_restores(&[(attempt,Err(error.clone()))],&[],cancel)?;decoded.failed.push(drawing.key);decoded.errors.push(error);}
            }
        }
        Ok(decoded)
    }
}
#[derive(Default)]
struct Decoded{drawings:Vec<(u64,SessionRestoreAttempt,SessionRestore,bool)>,failed:Vec<String>,errors:Vec<String>}
fn prepare(environment:&layer_host::open::OpenEnvironment,restored:SessionRestore,recovered:bool,active:bool,stopping:&AtomicBool)->Result<Box<UiSession<Renderer>>,String>{
    let mut candidate=environment.prepare(restored.document().clone(),||stopping.load(Ordering::Acquire))?;
    let observed=restored.state.location.as_ref().and_then(|location|DestinationFingerprint::observe_path(std::path::Path::new(&location.uri),restored.state.destination.as_ref()));
    candidate.restore_session(restored,recovered,observed)?;
    let deadline=Instant::now()+layer_host::open::PREPARE_DEADLINE;
    while !candidate.can_park_document()||candidate.retained_document_tiles().try_blobs()?.is_none(){
        if stopping.load(Ordering::Acquire){return Err("Session operation cancelled".into());}
        candidate.frame(0,0)?;
        if let Some(renderer)=candidate.engine().backend().0.as_ref(){renderer.device().poll(wgpu::PollType::Poll).map_err(|error|error.to_string())?;}
        if Instant::now()>=deadline{return Err("Session drawing preparation timed out".into());}
        std::thread::sleep(Duration::from_millis(2));
    }
    if !active {candidate.park_document()?;drop(candidate.renderer_mut().0.take());}
    Ok(candidate)
}
enum Job {
    Restore {
        environment: Box<layer_host::open::OpenEnvironment>,
    },
    RestoreDrawing {
        id: u64,
        environment: Box<layer_host::open::OpenEnvironment>,
    },
    Adopt{mapping:Vec<(u64,u64)>,attempts:Vec<(SessionRestoreAttempt,Result<(),String>)>},
    Discard{id:u64},
    Capture {
        manifest: SessionManifest,
        captures: Vec<(u64, SessionCapture)>,
    },
    Fresh,
    Retry,
    RetireSession(Box<UiSession<Renderer>>),
    #[cfg_attr(
        not(target_os = "windows"),
        expect(dead_code, reason = "Used by the Windows host")
    )]
    RetireRenderer(Box<Renderer>),
    Stop,
}
enum Finished {
    Opened(Result<(SessionManifest, usize), String>),
    Restored{result:Result<PreparedRestore, String>,manifest:Option<SessionManifest>},
    RestoredDrawing{result:Result<(u64,SessionRestoreAttempt,Box<UiSession<Renderer>>),String>,manifest:Option<SessionManifest>},
    Captured{result:Result<(),String>,manifest:Option<SessionManifest>},
    Updated{result:Result<(),String>,manifest:Option<SessionManifest>},
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Action {
    Retry,
    KeepOpen,
    Later { id: u64 },
    Discard { id: u64 },
    RestoreDrawing { id: u64 },
}
pub(crate) struct Restored {
    pub candidates: Vec<(u64, Box<UiSession<Renderer>>)>,
    pub active: u64,
    pub stamp: SessionStamp,
}
struct PreparedRestore{candidates:Vec<(u64,Box<UiSession<Renderer>>)>,attempts:Vec<SessionRestoreAttempt>,failed:Vec<String>,errors:Vec<String>}
pub(crate) struct Service {
    send: SyncSender<Job>,
    receive: Receiver<(Finished,Vec<String>)>,
    thread: Option<JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
    deferred: VecDeque<Job>,
    manifest: SessionManifest,
    startup_stamp: Option<SessionStamp>,
    stamps: BTreeMap<u64, SessionStamp>,
    pending_stamps: BTreeMap<u64, SessionStamp>,
    restored: Option<Restored>,
    restore_queue: VecDeque<u64>,
    restore_mapping:Vec<(u64,u64)>,
    restore_attempts:Vec<SessionRestoreAttempt>,
    retained: Vec<String>,
    asking: VecDeque<String>,
    retry: Option<u64>,
    restored_drawing: Option<(u64,SessionRestoreAttempt,Box<UiSession<Renderer>>)>,
    restore_errors: Vec<String>,
    windows: usize,
    ready: bool,
    busy: bool,
    restoring: bool,
    closing: bool,
    closed: bool,
    error: Option<String>,
    next_observation: Instant,
    changed: bool,
}
impl Service {
    pub fn open(wake: impl Fn() + Send + 'static) -> Result<Self, String> {
        let root = crate::storage::roots()?.sessions();
        let (send, jobs) = mpsc::sync_channel(1);
        let (reply, receive) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stopping = cancel.clone();
        let thread=std::thread::Builder::new().name("capy-session".into()).stack_size(8*1024*1024).spawn(move||{
            let (mut storage,initial)=match Storage::open(&root){Ok((storage,others))=>{let manifest=storage.manifest.clone();(Some(storage),Ok((manifest,if LAUNCHED.swap(true,Ordering::AcqRel){0}else{others})))},Err(error)=>(None,Err(error))};let warnings=storage.as_mut().map_or_else(Vec::new,|storage|std::mem::take(&mut storage.warnings));let _=reply.send((Finished::Opened(initial),warnings));wake();
            while let Ok(job)=jobs.recv(){
                let completed=match job {
                    Job::Stop=>break,Job::RetireSession(session)=>{drop(session);continue},Job::RetireRenderer(renderer)=>{drop(renderer);continue},
                    Job::Retry=>if let Some(storage)=&mut storage {let result=(||{let mut manifest=storage.manifest.clone();manifest.restoring.clear();manifest.blocked.clear();manifest.generation=manifest.generation.checked_add(1).ok_or("Session generation exhausted")?;storage.publish(manifest,&stopping)?;storage.collect(&stopping)?;Ok(())})();Finished::Updated{result,manifest:Some(storage.manifest.clone())}}else{let result=Storage::open(&root).map(|(next,_)|{let manifest=next.manifest.clone();storage=Some(next);(manifest,0)});Finished::Opened(result)},
                    Job::Fresh=>{let result=Storage::create(&root).map(|next|{let manifest=next.manifest.clone();storage=Some(next);(manifest,0)});Finished::Opened(result)},
                    Job::Adopt{mapping,attempts}=>{let result=storage.as_mut().ok_or_else(||"Session storage is unavailable".to_string()).and_then(|storage|storage.complete_restores(&attempts,&mapping,&stopping));Finished::Updated{result,manifest:storage.as_ref().map(|storage|storage.manifest.clone())}},
                    Job::Discard{id}=>{let result=storage.as_mut().ok_or_else(||"Session storage is unavailable".to_string()).and_then(|storage|storage.discard(id,&stopping));Finished::Updated{result,manifest:storage.as_ref().map(|storage|storage.manifest.clone())}},
                    Job::Restore{environment}=>{let result=(||{
                        let storage=storage.as_mut().ok_or("Session storage is unavailable")?;
                        let Decoded{drawings,mut failed,mut errors}=storage.decode(environment.limits(),|editors|environment.admit_sessions(editors),&stopping)?;
                        let mut prepared=PreparedRestore{candidates:Vec::new(),attempts:Vec::new(),failed:Vec::new(),errors:Vec::new()};
                        for (id,attempt,restored,recovered) in drawings {
                            match prepare(&environment,restored,recovered,prepared.candidates.is_empty(),&stopping) {
                                Ok(candidate)=>{prepared.attempts.push(attempt);prepared.candidates.push((id,candidate));}
                                Err(error)=>{
                                    if stopping.load(Ordering::Acquire){return Err(error);}
                                    storage.complete_restores(&[(attempt,Err(error.clone()))],&[],&stopping)?;
                                    failed.extend(storage.manifest.drawings.iter().find(|drawing|drawing.id==id).map(|drawing|drawing.key.clone()));errors.push(error);
                                }
                            }
                        }
                        prepared.failed=failed;prepared.errors=errors;
                        Ok(prepared)
                    })();Finished::Restored{result,manifest:storage.as_ref().map(|storage|storage.manifest.clone())}},
                    Job::RestoreDrawing{id,environment}=>{let result=(||{
                        let storage=storage.as_mut().ok_or("Session storage is unavailable")?;
                        let drawing=storage.manifest.drawings.iter().find(|drawing|drawing.id==id).cloned().ok_or("Session drawing is missing")?;
                        let attempt=storage.begin_restore(id,true,&stopping)?;
                        let result=storage.read(&drawing,environment.limits(),&stopping).and_then(|(restored,_)|{
                            environment.admit_sessions(std::iter::once(&restored.editor))?;prepare(&environment,restored,true,false,&stopping)
                        });
                        match result {
                            Ok(candidate)=>Ok((id,attempt,candidate)),
                            Err(error)=>{if !stopping.load(Ordering::Acquire){storage.complete_restores(&[(attempt,Err(error.clone()))],&[],&stopping)?;}Err(error)}
                        }
                    })();Finished::RestoredDrawing{result,manifest:storage.as_ref().map(|storage|storage.manifest.clone())}},
                    Job::Capture{manifest,captures}=>{let result=storage.as_mut().ok_or_else(||"Session storage is unavailable".to_string()).and_then(|storage|storage.capture(manifest,captures,&stopping));Finished::Captured{result,manifest:storage.as_ref().map(|storage|storage.manifest.clone())}},
                };let warnings=storage.as_mut().map_or_else(Vec::new,|storage|std::mem::take(&mut storage.warnings));let _=reply.send((completed,warnings));wake();
            }
        }).map_err(|_|"Could not start session worker")?;
        Ok(Self {
            send,
            receive,
            thread: Some(thread),
            cancel,
            deferred: VecDeque::new(),
            manifest: SessionManifest {
                generation: 0,
                drawings: vec![],
                active: 0,
                clean_exit: false,
                restoring: vec![],
                blocked: vec![],
            },
            startup_stamp: None,
            stamps: BTreeMap::new(),
            pending_stamps: BTreeMap::new(),
            restored: None,
            restore_queue: VecDeque::new(),
            restore_mapping:vec![],
            restore_attempts:vec![],
            retained: vec![],
            asking: VecDeque::new(),
            retry: None,
            restored_drawing: None,
            restore_errors: vec![],
            windows: 0,
            ready: false,
            busy: true,
            restoring: false,
            closing: false,
            closed: false,
            error: None,
            next_observation: Instant::now(),
            changed: true,
        })
    }
    fn queue(&mut self, job: Job) {
        self.deferred.push_back(job);
    }
    fn drain(&mut self) -> Result<(), String> {
        while let Some(job) = self.deferred.pop_front() {
            match self.send.try_send(job) {
                Ok(()) => {}
                Err(mpsc::TrySendError::Full(job)) => {
                    self.deferred.push_front(job);
                    break;
                }
                Err(_) => return Err("Session worker stopped".into()),
            }
        }
        Ok(())
    }
    pub fn poll<P: Parked>(
        &mut self,
        host: &mut NativeHost,
        window: &DocumentWindow<P>,
        quitting: bool,
    ) -> Result<bool, String> {
        loop {
            let (completed,warnings)=match self.receive.try_recv(){
                Ok(completed)=>completed,
                Err(mpsc::TryRecvError::Empty)=>break,
                Err(mpsc::TryRecvError::Disconnected)=>{
                    let error="Session worker stopped unexpectedly".to_string();
                    self.changed|=self.error.as_ref()!=Some(&error);self.error=Some(error);self.busy=false;break;
                }
            };
            self.restore_errors.extend(warnings);
            self.busy = false;
            self.changed = true;
            match completed {
                Finished::Opened(result) => match result {
                    Ok((manifest, windows)) => {
                        self.windows = windows;
                        self.restore_queue = manifest.drawings.iter().map(|d| d.id).collect();
                        if let Some(position) = self
                            .restore_queue
                            .iter()
                            .position(|id| *id == manifest.active)
                        {
                            self.restore_queue.swap(0, position);
                        }
                        self.manifest = manifest;
                        self.ready = true
                    }
                    Err(error) => self.error = Some(error),
                },
                Finished::Restored{result,manifest} => {if let Some(manifest)=manifest{self.manifest=manifest;}match result {
                    Ok(PreparedRestore{candidates,attempts,failed,errors}) => {
                        self.restore_attempts=attempts;self.retained=failed;self.restore_errors.extend(errors);
                        self.restored = Some(Restored {
                            active: candidates.first().map_or(0,|(id,_)|*id),
                            candidates,
                            stamp: self
                                .startup_stamp
                                .take()
                                .ok_or("Session startup identity is missing")?,
                        })
                    }
                    Err(error) => {
                        self.restoring = false;
                        self.error = Some(error)
                    }
                }},
                Finished::RestoredDrawing{result,manifest}=>{if let Some(manifest)=manifest{self.manifest=manifest;}match result {
                    Ok(restored)=>self.restored_drawing=Some(restored),
                    Err(error)=>self.restore_errors.push(error),
                }},
                Finished::Updated{result,manifest}=>{
                    if let Some(manifest)=manifest{self.manifest=manifest;}
                    if let Err(error)=result{self.error=Some(error);}
                },
                Finished::Captured{result,manifest} => {if let Some(manifest)=manifest{self.manifest=manifest;}match result {
                    Ok(()) => {self.stamps=std::mem::take(&mut self.pending_stamps);self.closed=self.closing;}
                    Err(error)=>{self.pending_stamps.clear();self.closed=self.closing&&!quitting&&!self.manifest.drawings.iter().any(|drawing|drawing.id==window.documents.selected());self.error=Some(error);}
                }},
            }
        }
        if self.closed && (quitting || host.session.state().document_file.close_ready) {
            self.drain()?;
            return Ok(std::mem::take(&mut self.changed));
        }
        if !host.session.state().document_file.close_ready && !quitting {
            self.closing = false;
            self.closed = false;
        }
        if self.ready && !self.busy && self.error.is_none() && self.restored.is_none() && self.restored_drawing.is_none() {
            if !self.restore_queue.is_empty() {
                if host.session.can_park_document() {
                    let mut environment = recovery_environment(&host.session)?;
                    environment.admission = window
                        .documents
                        .admission(&host.session.retained_document_tiles());
                    self.startup_stamp = Some(host.session.session_stamp());
                    self.restoring = true;
                    self.busy = true;
                    self.queue(Job::Restore {
                        environment: Box::new(environment),
                    });
                }
            } else if let Some(id) = self.retry.filter(|_| !quitting && host.session.can_park_document()) {
                let mut environment = recovery_environment(&host.session)?;
                environment.admission = window.documents.admission(&host.session.retained_document_tiles());
                self.retry = None;
                self.busy = true;
                self.queue(Job::RestoreDrawing { id, environment: Box::new(environment) });
            } else if !host.session.recovery_document().busy && (quitting || Instant::now() >= self.next_observation) {
                self.next_observation = Instant::now() + Duration::from_secs(2);
                let closing_drawing = host.session.state().document_file.close_ready && !quitting;
                let ids = window
                    .documents
                    .order()
                    .iter()
                    .copied()
                    .filter(|id| !closing_drawing || *id != window.documents.selected())
                    .collect::<Vec<_>>();
                let mut manifest = SessionManifest {
                    generation: self.manifest.generation,
                    drawings: vec![],
                    active: if ids.contains(&window.documents.selected()) {
                        window.documents.selected()
                    } else {
                        window.documents.after_close().filter(|id|ids.contains(id)).unwrap_or(0)
                    },
                    clean_exit: quitting,
                    restoring: vec![],
                    blocked: vec![],
                };
                let mut captures = Vec::new();
                let mut stamps = BTreeMap::new();
                for id in ids {
                    let session = window.session(host, id)?;
                    let stamp = session.session_stamp();
                    let key = self
                        .manifest
                        .drawings
                        .iter()
                        .find(|d| d.id == id)
                        .map(|d| d.key.clone())
                        .unwrap_or_else(key);
                    manifest.drawings.push(SessionDrawing { id, key });
                    if self.stamps.get(&id) != Some(&stamp) {
                        captures.push((id, session.capture_session()?));
                    }
                    stamps.insert(id, stamp);
                }
                manifest.drawings.extend(self.manifest.drawings.iter().filter(|drawing| self.retained.contains(&drawing.key)).cloned());
                if manifest.active == 0 {
                    manifest.active = manifest.drawings.first().map_or(0, |drawing| drawing.id);
                }
                if !captures.is_empty()
                    || manifest.drawings != self.manifest.drawings
                    || manifest.active != self.manifest.active
                    || quitting
                    || closing_drawing
                {
                    self.closing = quitting || closing_drawing;
                    self.closed = false;
                    self.pending_stamps = stamps;
                    self.busy = true;
                    self.queue(Job::Capture { manifest, captures });
                }
            }
        }
        self.drain()?;
        Ok(std::mem::take(&mut self.changed))
    }
    pub fn take_restored(&mut self) -> Option<Restored> {
        self.restored.take()
    }
    pub fn complete_restore(&mut self, result: Result<(), String>) -> Result<(), String> {
        if result.is_ok() {
            self.restore_queue.clear();
            self.asking=self.retained.iter().cloned().collect();
        } else {
            self.error = result.clone().err();
        }
        let mapping=std::mem::take(&mut self.restore_mapping);
        let attempts=std::mem::take(&mut self.restore_attempts).into_iter().map(|attempt|(attempt,result.clone())).collect();
        self.busy=true;self.queue(Job::Adopt{mapping,attempts});
        self.restoring = false;
        self.next_observation = Instant::now();
        self.changed = true;
        self.drain()
    }
    pub fn remap_restored(&mut self,mapping:Vec<(u64,u64)>)->Result<(),String>{
        self.manifest=self.manifest.remap(&mapping)?;self.restore_mapping=mapping;Ok(())
    }
    pub fn retained_ids(&self)->Vec<u64>{
        self.manifest.drawings.iter().filter(|drawing|self.retained.contains(&drawing.key)).map(|drawing|drawing.id).collect()
    }
    pub fn take_restored_drawing(&mut self)->Option<(u64,SessionRestoreAttempt,Box<UiSession<Renderer>>)>{
        self.restored_drawing.take()
    }
    pub fn complete_drawing(&mut self,attempt:SessionRestoreAttempt,restored:Result<u64,String>)->Result<(),String>{
        let mapping=restored.as_ref().ok().filter(|id|**id!=attempt.id).map(|id|vec![(attempt.id,*id)]).unwrap_or_default();
        if restored.is_ok() {
            let key=self.manifest.drawings.iter().find(|drawing|drawing.id==attempt.id).map(|drawing|drawing.key.clone());
            self.retained.retain(|retained|Some(retained)!=key.as_ref());
            self.manifest=self.manifest.remap(&mapping)?;
        }
        self.busy=true;self.queue(Job::Adopt{mapping,attempts:vec![(attempt,restored.map(|_|()))]});
        self.next_observation=Instant::now();self.changed=true;
        self.drain()
    }
    pub fn take_errors(&mut self)->Option<String>{
        (!self.restore_errors.is_empty()).then(||std::mem::take(&mut self.restore_errors).join("\n"))
    }
    fn answer(&mut self,id:u64)->Result<String,String>{
        let key=self.manifest.drawings.iter().find(|drawing|drawing.id==id).map(|drawing|drawing.key.clone()).filter(|key|self.asking.front()==Some(key)).ok_or("This drawing is not waiting for a recovery choice")?;
        self.asking.pop_front();Ok(key)
    }
    pub fn restore_order(&self) -> Vec<u64> {
        self.manifest
            .drawings
            .iter()
            .map(|drawing| drawing.id)
            .collect()
    }
    pub fn dispatch(
        &mut self,
        session: &mut UiSession<Renderer>,
        action: Action,
    ) -> Result<(), String> {
        match action {
            Action::Retry => {
                self.error = None;
                self.closed = false;
                self.next_observation = Instant::now();
                self.queue(Job::Retry);self.busy=true;
            }
            Action::KeepOpen => {
                session.reset_document_close();
                self.closing = false;
                self.closed = false;
                if !self.restore_queue.is_empty() || !self.ready {
                    self.restore_queue.clear();
                    self.stamps.clear();
                    self.ready = false;
                    self.busy = true;
                    self.queue(Job::Fresh);
                }
                self.error = None;
            }
            Action::Later { id } => {
                self.answer(id)?;
            }
            Action::Discard { id } => {
                let key = self.answer(id)?;
                self.retained.retain(|retained| *retained != key);
                self.busy = true;
                self.queue(Job::Discard { id });
            }
            Action::RestoreDrawing { id } => {
                self.answer(id)?;
                self.retry = Some(id);
            }
        }
        self.changed = true;
        self.drain()
    }
    pub fn restoring(&self) -> bool {
        self.restoring
    }
    pub fn close_ready(&self) -> bool {
        self.closed && !self.busy
    }
    pub fn failed(&self)->bool{self.error.is_some()&&!self.closed}
    #[cfg_attr(
        not(target_os = "windows"),
        expect(dead_code, reason = "Used by the Windows host")
    )]
    pub fn status(&self, localization: &layer_ui::Localizer) -> serde_json::Value {
        let drawing=self.asking.front().and_then(|key|self.manifest.drawings.iter().find(|drawing|drawing.key==*key)).map(|drawing|drawing.id);
        serde_json::json!({"busy":self.busy,"restoring":self.restoring,"closing":self.closing,"ready":self.close_ready(),
            "error":self.error.as_ref().map(|error|layer_ui::document_recovery_unavailable(localization,error)),"drawing":drawing,"retained":self.retained.len(),"windows":self.windows})
    }
    #[cfg_attr(
        not(target_os = "windows"),
        expect(dead_code, reason = "Used by the Windows host")
    )]
    pub fn retire_renderer(&mut self, renderer: Renderer) {
        self.queue(Job::RetireRenderer(Box::new(renderer)));
    }
    pub fn stop(&mut self) -> Result<(), String> {
        if self.thread.is_none() {
            return Ok(());
        }
        self.cancel.store(true, Ordering::Release);
        if let Some(restored) = self.restored.take() {
            for (_, candidate) in restored.candidates {
                self.queue(Job::RetireSession(candidate));
            }
        }
        if let Some((_, _, candidate)) = self.restored_drawing.take() {
            self.queue(Job::RetireSession(candidate));
        }
        while let Some(job) = self.deferred.pop_front() {
            self.send.send(job).map_err(|_| "Session worker stopped")?;
        }
        let _ = self.send.send(Job::Stop);
        if let Some(thread) = self.thread.take() {
            thread.join().map_err(|_| "Session shutdown failed")?;
        }
        Ok(())
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;
    use layer_ui::{Platform, UiAction};
    fn capture() -> SessionCapture {
        UiSession::from_project(Renderer(None),layer_ui::new_drawing(32,24,&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(),None,[32,24],Platform::Windows).unwrap().capture_session().unwrap()
    }
    #[test]
    fn window_membership_survives_restart_and_live_owners_are_never_claimed() {
        let root=TempDir::new();let cancel=AtomicBool::new(false);
        let mut original=Storage::create(&root.path).unwrap();
        let prepared=capture().prepare(&cancel).unwrap();
        original.store(4,"drawing-four").unwrap().commit(&prepared,&cancel).unwrap();
        original.store(7,"drawing-seven").unwrap().commit(&prepared,&cancel).unwrap();
        let manifest=SessionManifest::default().reconcile(vec![SessionDrawing{id:7,key:"drawing-seven".into()},SessionDrawing{id:4,key:"drawing-four".into()}],4,false).unwrap();
        original.publish(manifest,&cancel).unwrap();
        let concurrent=Storage::open(&root.path).unwrap().0;assert_ne!(concurrent.directory,original.directory);assert!(concurrent.manifest.drawings.is_empty());
        drop(concurrent);
        let directory=original.directory.clone();drop(original);
        let mut restored=Storage::open(&root.path).unwrap().0;assert_eq!(restored.directory,directory);assert_eq!(restored.manifest.drawings.iter().map(|drawing|drawing.id).collect::<Vec<_>>(),[7,4]);assert_eq!(restored.manifest.active,4);
        let closed=restored.manifest.remove(4).unwrap();restored.publish(closed,&cancel).unwrap();restored.store(4,"drawing-four").unwrap().retire().unwrap();drop(restored);
        let reopened=Storage::open(&root.path).unwrap().0;assert_eq!(reopened.manifest.drawings.iter().map(|drawing|drawing.id).collect::<Vec<_>>(),[7]);assert_eq!(reopened.manifest.active,7);
    }
    #[test]
    fn stale_membership_cannot_resurrect_an_explicitly_closed_drawing() {
        let root=TempDir::new();let cancel=AtomicBool::new(false);let mut storage=Storage::create(&root.path).unwrap();
        let original=storage.manifest.stage(vec![SessionDrawing{id:3,key:"drawing".into()}],3).unwrap();storage.publish(original.clone(),&cancel).unwrap();let removed=storage.manifest.remove(3).unwrap();storage.publish(removed,&cancel).unwrap();
        assert!(storage.publish(original,&cancel).is_err());assert!(SessionManifest::read(&storage.directory.join("session.json")).unwrap().unwrap().drawings.is_empty());
    }
    #[test]
    fn staged_first_checkpoint_remains_reachable_after_interruption() {
        let root=TempDir::new();let cancel=AtomicBool::new(false);let mut storage=Storage::create(&root.path).unwrap();
        let staged=storage.manifest.stage(vec![SessionDrawing{id:9,key:"first-drawing".into()}],9).unwrap();storage.publish(staged,&cancel).unwrap();let directory=storage.directory.clone();drop(storage);
        let mut reopened=Storage::open(&root.path).unwrap().0;assert_eq!(reopened.directory,directory);assert_eq!(reopened.manifest.drawings[0].id,9);assert!(!reopened.manifest.clean_exit);
        assert!(reopened.store(9,"first-drawing").unwrap().load(Default::default(),&cancel).unwrap().is_none());
        let prepared=capture().prepare(&cancel).unwrap();reopened.store(9,"first-drawing").unwrap().commit(&prepared,&cancel).unwrap();drop(reopened);
        let mut completed=Storage::open(&root.path).unwrap().0;assert_eq!(completed.manifest.drawings[0].key,"first-drawing");assert!(completed.store(9,"first-drawing").unwrap().load(Default::default(),&cancel).unwrap().is_some());
    }
    #[test]
    fn interrupted_restore_is_blocked_and_corrupt_membership_is_preserved() {
        let root=TempDir::new();let cancel=AtomicBool::new(false);let mut storage=Storage::create(&root.path).unwrap();
        let manifest=SessionManifest::default().reconcile(vec![SessionDrawing{id:1,key:"drawing".into()}],1,true).unwrap();storage.publish(manifest,&cancel).unwrap();
        let interrupted=storage.manifest.begin_restore(1).unwrap();storage.publish(interrupted,&cancel).unwrap();let directory=storage.directory.clone();drop(storage);
        let restored=Storage::open(&root.path).unwrap().0;assert_eq!(restored.manifest.blocked,[1]);assert!(restored.manifest.restoring.is_empty());drop(restored);
        fs::write(directory.join("session.json"),b"incomplete membership").unwrap();assert!(Storage::open(&root.path).is_err());assert_eq!(fs::read(directory.join("session.json")).unwrap(),b"incomplete membership");
    }
    #[test]
    fn an_unreadable_drawing_waits_for_a_choice_while_the_others_reopen() {
        let root=TempDir::new();let cancel=AtomicBool::new(false);let mut storage=Storage::create(&root.path).unwrap();
        let prepared=capture().prepare(&cancel).unwrap();storage.store(2,"readable").unwrap().commit(&prepared,&cancel).unwrap();
        let manifest=SessionManifest::default().reconcile(vec![SessionDrawing{id:5,key:"unreadable".into()},SessionDrawing{id:2,key:"readable".into()}],5,false).unwrap();storage.publish(manifest,&cancel).unwrap();
        let decoded=storage.decode(Default::default(),|_|Ok(()),&cancel).unwrap();
        assert_eq!(decoded.drawings.iter().map(|(id,..)|*id).collect::<Vec<_>>(),[2]);assert_eq!(decoded.failed,["unreadable"]);assert_eq!(decoded.errors.len(),1);
        assert_eq!(storage.manifest.blocked,[5]);storage.complete_restores(&[(decoded.drawings[0].1,Ok(()))],&[],&cancel).unwrap();
        let failures=decoded.errors.clone();let directory=storage.directory.clone();drop(decoded);drop(storage);
        let mut reopened=Storage::open(&root.path).unwrap().0;assert_eq!(reopened.directory,directory);
        let decoded=reopened.decode(Default::default(),|_|Ok(()),&cancel).unwrap();
        assert_eq!(decoded.drawings.iter().map(|(id,..)|*id).collect::<Vec<_>>(),[2]);assert_eq!(decoded.failed,["unreadable"]);assert_eq!(decoded.errors,failures);
        reopened.complete_restores(&[(decoded.drawings[0].1,Ok(()))],&[],&cancel).unwrap();drop(decoded);
        assert!(reopened.begin_restore(5,true,&cancel).is_ok());assert!(reopened.read(&SessionDrawing{id:5,key:"unreadable".into()},Default::default(),&cancel).is_err());
        let attempt=*reopened.manifest.restoring.iter().find(|attempt|attempt.id==5).unwrap();reopened.complete_restores(&[(attempt,Err("Session drawing is missing".into()))],&[],&cancel).unwrap();
        reopened.discard(5,&cancel).unwrap();assert_eq!(reopened.manifest.drawings,[SessionDrawing{id:2,key:"readable".into()}]);assert!(reopened.manifest.blocked.is_empty());drop(reopened);
        let restarted=Storage::open(&root.path).unwrap().0;assert_eq!(restarted.manifest.drawings.iter().map(|drawing|drawing.id).collect::<Vec<_>>(),[2]);
    }
    fn damaged_orphan(storage:&Storage)->PathBuf {
        let orphan=storage.directory.join("unreferenced-copy");
        fs::create_dir_all(orphan.join("resources")).unwrap();fs::create_dir_all(orphan.join("generations")).unwrap();
        fs::write(orphan.join(".lock"),b"").unwrap();fs::write(orphan.join(".retiring"),b"invalid").unwrap();orphan
    }
    #[test]
    fn unrelated_retirement_damage_does_not_block_restore() {
        let root=TempDir::new();let cancel=AtomicBool::new(false);let mut storage=Storage::create(&root.path).unwrap();
        let prepared=capture().prepare(&cancel).unwrap();storage.store(1,"drawing").unwrap().commit(&prepared,&cancel).unwrap();
        let manifest=SessionManifest::default().reconcile(vec![SessionDrawing{id:1,key:"drawing".into()}],1,false).unwrap();storage.publish(manifest,&cancel).unwrap();
        let orphan=damaged_orphan(&storage);let directory=storage.directory.clone();drop(storage);
        let mut restored=Storage::open(&root.path).unwrap().0;assert_eq!(restored.directory,directory);
        assert!(restored.warnings.iter().any(|error|error.contains("Invalid drawing retirement intent")));
        let decoded=restored.decode(Default::default(),|_|Ok(()),&cancel).unwrap();
        assert_eq!(decoded.drawings.iter().map(|(id,..)|*id).collect::<Vec<_>>(),[1]);assert!(decoded.failed.is_empty());
        assert_eq!(fs::read(orphan.join(".retiring")).unwrap(),b"invalid");
    }
    #[test]
    fn unrelated_retirement_damage_does_not_reject_committed_checkpoints() {
        let root=TempDir::new();let cancel=AtomicBool::new(false);let mut storage=Storage::create(&root.path).unwrap();
        let mut session=UiSession::from_project(Renderer(None),layer_ui::new_drawing(32,24,&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(),None,[32,24],Platform::Windows).unwrap();
        let mut manifest=storage.manifest.clone();manifest.drawings=vec![SessionDrawing{id:1,key:"drawing".into()}];manifest.active=1;
        storage.capture(manifest,vec![(1,session.capture_session().unwrap())],&cancel).unwrap();let orphan=damaged_orphan(&storage);
        for opacity in [0.4,0.8] {
            session.dispatch(UiAction::SetLayerOpacity{id:None,opacity}).unwrap();
            let previous=fs::read(storage.directory.join("drawing/head.json")).unwrap();
            storage.capture(storage.manifest.clone(),vec![(1,session.capture_session().unwrap())],&cancel).unwrap();
            assert_ne!(fs::read(storage.directory.join("drawing/head.json")).unwrap(),previous);
            assert!(storage.warnings.pop().unwrap().contains("Invalid drawing retirement intent"));
            let restored=storage.store(1,"drawing").unwrap().load(Default::default(),&cancel).unwrap().unwrap();
            assert_eq!(restored.editor.checkpoint(),session.engine().checkpoint());assert!(restored.editor.can_undo());
            assert_eq!(fs::read(orphan.join(".retiring")).unwrap(),b"invalid");
        }
    }
    #[test]
    fn unrelated_retirement_damage_does_not_reject_published_discard() {
        let root=TempDir::new();let cancel=AtomicBool::new(false);let mut storage=Storage::create(&root.path).unwrap();
        let prepared=capture().prepare(&cancel).unwrap();
        for (id,key) in [(1,"closed"),(2,"retained")] {storage.store(id,key).unwrap().commit(&prepared,&cancel).unwrap();}
        let manifest=SessionManifest::default().reconcile(vec![SessionDrawing{id:1,key:"closed".into()},SessionDrawing{id:2,key:"retained".into()}],2,false).unwrap();storage.publish(manifest,&cancel).unwrap();
        let orphan=damaged_orphan(&storage);storage.discard(1,&cancel).unwrap();
        assert!(storage.warnings.iter().any(|error|error.contains("Invalid drawing retirement intent")));
        assert_eq!(storage.manifest.drawings,[SessionDrawing{id:2,key:"retained".into()}]);
        assert!(storage.store(2,"retained").unwrap().load(Default::default(),&cancel).unwrap().is_some());
        assert_eq!(fs::read(orphan.join(".retiring")).unwrap(),b"invalid");
    }
    #[test]
    fn restore_causes_follow_durable_keys_and_clear_only_after_acknowledgment() {
        let root=TempDir::new();let cancel=AtomicBool::new(false);let mut storage=Storage::create(&root.path).unwrap();
        let prepared=capture().prepare(&cancel).unwrap();
        for (id,key) in [(1,"first"),(2,"second")] {storage.store(id,key).unwrap().commit(&prepared,&cancel).unwrap();}
        let manifest=SessionManifest::default().reconcile(vec![SessionDrawing{id:1,key:"first".into()},SessionDrawing{id:2,key:"second".into()}],1,false).unwrap();storage.publish(manifest,&cancel).unwrap();
        let first=storage.begin_restore(1,false,&cancel).unwrap();let second=storage.begin_restore(2,false,&cancel).unwrap();
        storage.complete_restores(&[(first,Err("first restore cause".into())),(second,Err("second restore cause".into()))],&[],&cancel).unwrap();
        let directory=storage.directory.clone();drop(storage);
        let mut storage=Storage::open(&root.path).unwrap().0;
        let decoded=storage.decode(Default::default(),|_|Ok(()),&cancel).unwrap();
        assert!(decoded.drawings.is_empty());assert_eq!(decoded.failed,["first","second"]);assert_eq!(decoded.errors,["first restore cause","second restore cause"]);
        let retry=storage.begin_restore(1,true,&cancel).unwrap();
        assert!(storage.read(&SessionDrawing{id:1,key:"first".into()},Default::default(),&cancel).is_ok());
        let membership=fs::read(directory.join("session.json")).unwrap();
        fs::write(directory.join("session.json"),b"unreadable membership").unwrap();
        assert!(storage.complete_restores(&[(retry,Ok(()))],&[(1,7)],&cancel).is_err());
        assert_eq!(restore_error(&directory.join("first")).unwrap().as_deref(),Some("first restore cause"));
        assert_eq!(fs::read(directory.join("session.json")).unwrap(),b"unreadable membership");
        fs::write(directory.join("session.json"),membership).unwrap();
        storage.complete_restores(&[(retry,Ok(()))],&[(1,7)],&cancel).unwrap();
        assert_eq!(storage.manifest.drawings,[SessionDrawing{id:7,key:"first".into()},SessionDrawing{id:2,key:"second".into()}]);assert_eq!(storage.manifest.blocked,[2]);
        assert_eq!(restore_error(&directory.join("first")).unwrap(),None);
        assert_eq!(restore_error(&directory.join("second")).unwrap().as_deref(),Some("second restore cause"));
    }
    #[test]
    fn rejected_restore_completion_preserves_membership_and_saved_causes() {
        let root=TempDir::new();let cancel=AtomicBool::new(false);let mut storage=Storage::create(&root.path).unwrap();
        let prepared=capture().prepare(&cancel).unwrap();
        for (id,key) in [(1,"first"),(2,"second")] {storage.store(id,key).unwrap().commit(&prepared,&cancel).unwrap();set_restore_error(&storage.directory.join(key),Some(key)).unwrap();}
        let manifest=SessionManifest::default().reconcile(vec![SessionDrawing{id:1,key:"first".into()},SessionDrawing{id:2,key:"second".into()}],1,false).unwrap();storage.publish(manifest,&cancel).unwrap();
        let first=storage.begin_restore(1,false,&cancel).unwrap();let second=storage.begin_restore(2,false,&cancel).unwrap();
        let manifest=storage.manifest.clone();let membership=fs::read(storage.directory.join("session.json")).unwrap();
        let stale=SessionRestoreAttempt{generation:second.generation+1,..second};
        for (attempt,mapping,cancelled) in [(stale,vec![],false),(second,vec![(1,2)],false),(second,vec![],true)] {
            cancel.store(cancelled,Ordering::Release);
            assert!(storage.complete_restores(&[(first,Err("replacement first".into())),(attempt,Err("replacement second".into()))],&mapping,&cancel).is_err());
            assert_eq!(storage.manifest,manifest);assert_eq!(fs::read(storage.directory.join("session.json")).unwrap(),membership);
            for key in ["first","second"] {assert_eq!(restore_error(&storage.directory.join(key)).unwrap().as_deref(),Some(key));}
        }
    }
    #[test]
    fn cancelled_checkpoint_keeps_the_previous_artwork_and_history() {
        let root=TempDir::new();let cancel=AtomicBool::new(false);let mut storage=Storage::create(&root.path).unwrap();
        let mut session=UiSession::from_project(Renderer(None),layer_ui::new_drawing(32,24,&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(),None,[32,24],Platform::Windows).unwrap();
        session.dispatch(UiAction::SetLayerOpacity{id:None,opacity:0.4}).unwrap();
        let expected=session.engine().checkpoint();let first=session.capture_session().unwrap().prepare(&cancel).unwrap();storage.store(1,"drawing").unwrap().commit(&first,&cancel).unwrap();
        session.dispatch(UiAction::SetLayerOpacity{id:None,opacity:0.8}).unwrap();let next=session.capture_session().unwrap().prepare(&cancel).unwrap();cancel.store(true,Ordering::Release);assert!(storage.store(1,"drawing").unwrap().commit(&next,&cancel).is_err());cancel.store(false,Ordering::Release);
        let restored=storage.store(1,"drawing").unwrap().load(Default::default(),&cancel).unwrap().unwrap();assert_eq!(restored.editor.checkpoint(),expected);assert!(restored.editor.can_undo());
    }
}
