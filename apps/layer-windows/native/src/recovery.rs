use crate::{document_io::io_error, documents::recovery_environment};
use layer_core::package::session_store::{OwnerLock, SessionStore, create_directory,collect_unreferenced_stores};
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
        })
    }
    fn open(root: &std::path::Path) -> Result<Self, String> {
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
        candidates.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
        for (_,directory) in &candidates {
            let Some(_lease)=OwnerLock::claim(&directory.join("owner.lock"))? else{continue};
            if matches!(SessionManifest::read(&directory.join("session.json")),Ok(Some(manifest)) if manifest.drawings.is_empty()) {
                collect_unreferenced_stores(directory,&Default::default(),&AtomicBool::new(false))?;
            }
        }
        for (_, directory) in candidates {
            let Some(lease) = OwnerLock::claim(&directory.join("owner.lock"))? else {
                continue;
            };
            let path = directory.join("session.json");
            let Some(manifest)=SessionManifest::read(&path)? else {continue};
            if manifest.drawings.is_empty() {
                continue;
            }
            let mut storage = Self {
                directory,
                _lease: lease,
                manifest,
                stores: BTreeMap::new(),
            };
            if !storage.manifest.restoring.is_empty() {
                let manifest = storage.manifest.recover_interrupted()?;
                storage.publish(manifest, &AtomicBool::new(false))?;
            }
            collect_unreferenced_stores(&storage.directory,&storage.manifest.drawings.iter().map(|drawing|drawing.key.clone()).collect(),&AtomicBool::new(false))?;
            return Ok(storage);
        }
        Self::create(root)
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
        let reachable=self.manifest.drawings.iter().map(|drawing|drawing.key.clone()).collect::<std::collections::BTreeSet<_>>();
        for key in self.stores.keys().filter(|key|!reachable.contains(*key)).cloned().collect::<Vec<_>>() {self.stores.get_mut(&key).unwrap().retire()?;self.stores.remove(&key);}
        collect_unreferenced_stores(&self.directory,&reachable,cancel)?;Ok(())
    }

}
enum Job {
    Restore {
        environment: Box<layer_host::open::OpenEnvironment>,
    },
    Adopt{mapping:Vec<(u64,u64)>,attempts:Vec<SessionRestoreAttempt>},
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
    Opened(Result<SessionManifest, String>),
    Restored(Result<PreparedRestore, String>),
    Captured{result:Result<(),String>,manifest:Option<SessionManifest>},
    Updated{result:Result<(),String>,manifest:Option<SessionManifest>},
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Action {
    Retry,
    KeepOpen,
}
pub(crate) struct Restored {
    pub candidates: Vec<(u64, Box<UiSession<Renderer>>)>,
    pub active: u64,
    pub stamp: SessionStamp,
}
struct PreparedRestore{candidates:Vec<(u64,Box<UiSession<Renderer>>)>,attempts:Vec<SessionRestoreAttempt>}
pub(crate) struct Service {
    send: SyncSender<Job>,
    receive: Receiver<Finished>,
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
        let root = crate::settings::data_directory()?.join("sessions");
        let (send, jobs) = mpsc::sync_channel(1);
        let (reply, receive) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stopping = cancel.clone();
        let thread=std::thread::Builder::new().name("capy-session".into()).stack_size(8*1024*1024).spawn(move||{
            let (mut storage,initial)=match Storage::open(&root){Ok(storage)=>{let manifest=storage.manifest.clone();(Some(storage),Ok(manifest))},Err(error)=>(None,Err(error))};let _=reply.send(Finished::Opened(initial));wake();
            while let Ok(job)=jobs.recv(){
                let completed=match job {
                    Job::Stop=>break,Job::RetireSession(session)=>{drop(session);continue},Job::RetireRenderer(renderer)=>{drop(renderer);continue},
                    Job::Retry=>{if let Some(storage)=&mut storage {let result=(||{let mut manifest=storage.manifest.clone();manifest.restoring.clear();manifest.blocked.clear();manifest.generation=manifest.generation.checked_add(1).ok_or("Session generation exhausted")?;storage.publish(manifest,&stopping)?;storage.collect(&stopping)?;Ok(())})();let _=reply.send(Finished::Updated{result,manifest:Some(storage.manifest.clone())});wake();}else{let result=Storage::open(&root).map(|next|{let manifest=next.manifest.clone();storage=Some(next);manifest});let _=reply.send(Finished::Opened(result));wake();}continue},
                    Job::Fresh=>{let result=Storage::create(&root).map(|next|{let manifest=next.manifest.clone();storage=Some(next);manifest});Finished::Opened(result)},
                    Job::Adopt{mapping,attempts}=>{let result=(||{let storage=storage.as_mut().ok_or("Session storage is unavailable")?;let mut manifest=storage.manifest.clone();for attempt in attempts{manifest=manifest.finish_restore(attempt,true)?;}let manifest=manifest.remap(&mapping)?;storage.publish(manifest,&stopping)?;Ok(())})();let _=reply.send(Finished::Updated{result,manifest:storage.as_ref().map(|storage|storage.manifest.clone())});wake();continue},
                    Job::Restore{environment}=>Finished::Restored((||{
                        let storage=storage.as_mut().ok_or("Session storage is unavailable")?;
                        if !storage.manifest.restoring.is_empty()||!storage.manifest.blocked.is_empty(){return Err("A drawing could not finish reopening. Its session copy has been kept.".into());}
                        let mut drawings=storage.manifest.drawings.clone();
                        if let Some(position)=drawings.iter().position(|drawing|drawing.id==storage.manifest.active){drawings.swap(0,position);}
                        let recovered=!storage.manifest.clean_exit;
                        let mut decoded:Vec<(u64,SessionRestore,bool)>=Vec::new();
                        let mut attempts=Vec::new();
                        for drawing in drawings {
                            let manifest=storage.manifest.begin_restore(drawing.id)?;storage.publish(manifest,&stopping)?;
                            attempts.push(*storage.manifest.restoring.iter().find(|attempt|attempt.id==drawing.id).ok_or("Session restore attempt is missing")?);
                            let store=storage.store(drawing.id,&drawing.key)?;
                            let opened=store.load(environment.limits(),&stopping)?.ok_or("Session drawing is missing")?;
                            let drawing_recovered=recovered||store.recovered_previous();
                            let restored=SessionRestore::from_core(opened)?;
                            environment.admit_sessions(decoded.iter().map(|(_,restore,_)|&restore.editor).chain(std::iter::once(&restored.editor)))?;
                            decoded.push((drawing.id,restored,drawing_recovered));
                        }
                        let mut candidates=Vec::new();
                        for (id,restored,recovered) in decoded {
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
                            if id!=storage.manifest.active {candidate.park_document()?;drop(candidate.renderer_mut().0.take());}
                            candidates.push((id,candidate));
                        }
                        Ok(PreparedRestore{candidates,attempts})
                    })()),
                    Job::Capture{manifest,captures}=>{let result=(||{
                        let storage=storage.as_mut().ok_or("Session storage is unavailable")?;
                        if manifest.generation!=storage.manifest.generation {return Err("Stale session checkpoint membership".into());}
                        if manifest.drawings.iter().any(|drawing|!storage.manifest.drawings.iter().any(|previous|previous.key==drawing.key)){let staged=storage.manifest.stage(manifest.drawings.clone(),manifest.active)?;storage.publish(staged,&stopping)?;}
                        for (id,capture) in captures {let drawing=manifest.drawings.iter().find(|d|d.id==id).ok_or("Session drawing is missing")?;let prepared=capture.prepare(&stopping)?;storage.store(id,&drawing.key)?.commit(&prepared,&stopping)?;}
                        let retired=storage.manifest.drawings.iter().filter(|d|!manifest.drawings.iter().any(|live|live.key==d.key)).cloned().collect::<Vec<_>>();
                        for drawing in &retired{storage.store(drawing.id,&drawing.key)?.prepare_retirement()?;}
                        let mut committed=storage.manifest.clone();for drawing in &retired{committed=committed.remove(drawing.id)?;}
                        let committed=committed.reconcile(manifest.drawings,manifest.active,manifest.clean_exit)?;storage.publish(committed,&stopping)?;
                        for drawing in retired {storage.store(drawing.id,&drawing.key)?.retire()?;storage.stores.remove(&drawing.key);}
                        storage.collect(&stopping)?;
                        Ok(())
                    })();Finished::Captured{result,manifest:storage.as_ref().map(|storage|storage.manifest.clone())}},
                };let _=reply.send(completed);wake();
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
            let completed=match self.receive.try_recv(){
                Ok(completed)=>completed,
                Err(mpsc::TryRecvError::Empty)=>break,
                Err(mpsc::TryRecvError::Disconnected)=>{
                    let error="Session worker stopped unexpectedly".to_string();
                    self.changed|=self.error.as_ref()!=Some(&error);self.error=Some(error);self.busy=false;break;
                }
            };
            self.busy = false;
            self.changed = true;
            match completed {
                Finished::Opened(result) => match result {
                    Ok(manifest) => {
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
                Finished::Restored(result) => match result {
                    Ok(PreparedRestore{candidates,attempts}) => {
                        self.restore_attempts=attempts;
                        self.restored = Some(Restored {
                            candidates,
                            active: self.manifest.active,
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
                },
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
        if self.ready && !self.busy && self.error.is_none() && self.restored.is_none() {
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
            self.manifest.restoring.clear();
            let mapping=std::mem::take(&mut self.restore_mapping);let attempts=std::mem::take(&mut self.restore_attempts);self.busy=true;self.queue(Job::Adopt{mapping,attempts});
        } else {
            self.error = result.err();
        }
        self.restoring = false;
        self.next_observation = Instant::now();
        self.changed = true;
        self.drain()
    }
    pub fn remap_restored(&mut self,mapping:Vec<(u64,u64)>)->Result<(),String>{
        self.manifest=self.manifest.remap(&mapping)?;self.restore_mapping=mapping;Ok(())
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
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({"busy":self.busy,"restoring":self.restoring,"closing":self.closing,"ready":self.close_ready(),"error":self.error})
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
        let concurrent=Storage::open(&root.path).unwrap();assert_ne!(concurrent.directory,original.directory);assert!(concurrent.manifest.drawings.is_empty());
        drop(concurrent);
        let directory=original.directory.clone();drop(original);
        let mut restored=Storage::open(&root.path).unwrap();assert_eq!(restored.directory,directory);assert_eq!(restored.manifest.drawings.iter().map(|drawing|drawing.id).collect::<Vec<_>>(),[7,4]);assert_eq!(restored.manifest.active,4);
        let closed=restored.manifest.remove(4).unwrap();restored.publish(closed,&cancel).unwrap();restored.store(4,"drawing-four").unwrap().retire().unwrap();drop(restored);
        let reopened=Storage::open(&root.path).unwrap();assert_eq!(reopened.manifest.drawings.iter().map(|drawing|drawing.id).collect::<Vec<_>>(),[7]);assert_eq!(reopened.manifest.active,7);
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
        let mut reopened=Storage::open(&root.path).unwrap();assert_eq!(reopened.directory,directory);assert_eq!(reopened.manifest.drawings[0].id,9);assert!(!reopened.manifest.clean_exit);
        assert!(reopened.store(9,"first-drawing").unwrap().load(Default::default(),&cancel).unwrap().is_none());
        let prepared=capture().prepare(&cancel).unwrap();reopened.store(9,"first-drawing").unwrap().commit(&prepared,&cancel).unwrap();drop(reopened);
        let mut completed=Storage::open(&root.path).unwrap();assert_eq!(completed.manifest.drawings[0].key,"first-drawing");assert!(completed.store(9,"first-drawing").unwrap().load(Default::default(),&cancel).unwrap().is_some());
    }
    #[test]
    fn interrupted_restore_is_blocked_and_corrupt_membership_is_preserved() {
        let root=TempDir::new();let cancel=AtomicBool::new(false);let mut storage=Storage::create(&root.path).unwrap();
        let manifest=SessionManifest::default().reconcile(vec![SessionDrawing{id:1,key:"drawing".into()}],1,true).unwrap();storage.publish(manifest,&cancel).unwrap();
        let interrupted=storage.manifest.begin_restore(1).unwrap();storage.publish(interrupted,&cancel).unwrap();let directory=storage.directory.clone();drop(storage);
        let restored=Storage::open(&root.path).unwrap();assert_eq!(restored.manifest.blocked,[1]);assert!(restored.manifest.restoring.is_empty());drop(restored);
        fs::write(directory.join("session.json"),b"incomplete membership").unwrap();assert!(Storage::open(&root.path).is_err());assert_eq!(fs::read(directory.join("session.json")).unwrap(),b"incomplete membership");
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
