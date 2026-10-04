use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::{gio, glib};
use layer_core::{package::session_store::{SessionStore, OwnerLock}, ProjectLimits};
use layer_ui::{recovery::{RecoveryState, RecoveryEvent, RecoveryWork, RecoveryWorkKind}, session_recovery::{SessionCapture, SessionRestore, SessionManifest, SessionDrawing}};
use std::{cell::{Cell, RefCell}, path::PathBuf, rc::Rc, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::Duration};

fn directory() -> PathBuf {
    crate::storage::sessions().map(Into::into).unwrap_or_default()
}
fn key() -> String { layer_core::authored::PortableId::random().to_string() }
fn enabled() -> bool { crate::storage::sessions().is_some() }

fn storage_gate(root:&std::path::Path)->Result<OwnerLock,String> {
    layer_core::package::session_store::create_directory(root)?;
    loop {
        if let Some(lease)=OwnerLock::claim(&root.join(".gc.lock"))? {return Ok(lease);}
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn collect_stores()->Result<(),String> {
    let root=directory();let _gate=storage_gate(&root)?;
    let mut reachable=std::collections::BTreeSet::new();
    for entry in std::fs::read_dir(&root).map_err(|error|error.to_string())? {
        let path=entry.map_err(|error|error.to_string())?.path();
        if path.extension().is_some_and(|extension|extension=="json") {
            let manifest=SessionManifest::read(&path)?.ok_or("Saved session disappeared during cleanup")?;
            reachable.extend(manifest.drawings.into_iter().map(|drawing|drawing.key));
        }
    }
    layer_core::package::session_store::collect_unreferenced_stores(&root,&reachable,&AtomicBool::new(false))?;
    Ok(())
}

pub(crate) struct Recovery {
    pub recovered: Cell<bool>,
    path: PathBuf,
    store: Arc<Mutex<Option<SessionStore>>>,
    policy: RefCell<RecoveryState>,
    discarded: Arc<AtomicBool>,
    snapshot: RefCell<Option<(SessionCapture, crate::render_thread::ContextCapture)>>,
    running: Cell<bool>,
    current: Cell<bool>,
    error: RefCell<Option<String>>,
    stamp: RefCell<Option<layer_ui::session_recovery::SessionStamp>>,
    window: RefCell<std::rc::Weak<Workspace>>,
    registered: Cell<bool>,
}
impl Default for Recovery {
    fn default() -> Self {
        let mut policy = RecoveryState::default();
        policy.event(RecoveryEvent::Ownership { owned: true }).unwrap();
        Self { recovered: Cell::new(false), path: directory().join(key()), store: Default::default(), policy: RefCell::new(policy), discarded: Arc::new(AtomicBool::new(false)), snapshot: Default::default(), running: Cell::new(false), current: Cell::new(false), error: Default::default(), stamp: Default::default(), window: Default::default(), registered: Cell::new(false) }
    }
}
impl Recovery {
    pub fn discard(self: &Rc<Self>) {
        self.snapshot.borrow_mut().take();
        self.discarded.store(true, Ordering::Release);
        let work = self.policy.borrow_mut().event(RecoveryEvent::Retire).unwrap().work;
        self.policy.borrow_mut().event(RecoveryEvent::Close).unwrap();
        self.execute(work);
    }
    pub fn capture(self: &Rc<Self>, w: &Rc<Workspace>) {
        self.window.replace(Rc::downgrade(w));
        if let Some(gpu) = w.gpu.borrow().as_ref() { self.capture_session(&gpu.session); }
    }
    pub fn capture_session(self: &Rc<Self>, session: &layer_ui::UiSession<crate::render_thread::RenderWorker>) {
        if !enabled() || self.discarded.load(Ordering::Acquire) { return; }
        let stamp=session.session_stamp();
        if self.current.get() && self.stamp.borrow().as_ref()==Some(&stamp) {return;}
        let document = session.recovery_document();
        if !document.busy {
            match session.capture_session().and_then(|capture| session.engine().backend().capture_context().map(|context| (capture, context))) {
                Ok(snapshot) => { *self.snapshot.borrow_mut() = Some(snapshot); self.stamp.replace(Some(stamp)); }
                Err(error) => { self.error.replace(Some(error)); return; }
            }
        }
        let update = self.policy.borrow_mut().event(RecoveryEvent::Observe { document, owned: true }).unwrap();
        self.current.set(update.current);
        self.execute(update.work);
    }
    pub fn retry(self:&Rc<Self>) {
        if !self.current.get()&&!self.running.get()&&!self.discarded.load(Ordering::Acquire) {
            let update=self.policy.borrow_mut().event(RecoveryEvent::Ownership {owned:true}).unwrap();
            self.execute(update.work);
        }
    }
    pub async fn prepare_retirement(&self)->Result<(),String> {
        self.drain().await;
        let store=self.store.clone();
        gio::spawn_blocking(move || {
            let mut store=store.lock().map_err(|error|error.to_string())?;
            if let Some(store)=store.as_mut() {store.prepare_retirement()?;}
            Ok(())
        }).await.map_err(|error|format!("Drawing retirement preparation stopped: {error:?}"))?
    }
    pub async fn drain(&self) {
        while self.running.get() { glib::timeout_future(Duration::from_millis(5)).await; }
    }
    pub async fn flush(&self) -> Result<(), String> {
        self.drain().await;
        if let Some(error) = self.error.borrow().clone() { return Err(error); }
        if !self.current.get() { return Err("The latest drawing changes are still being prepared".into()); }
        Ok(())
    }
    #[cfg(test)]
    pub async fn read_snapshot(&self)->Result<SessionRestore,String> {
        let store=self.store.clone();
        gio::spawn_blocking(move || {
            let mut store=store.lock().map_err(|e|e.to_string())?;
            let open=store.as_mut().ok_or("Drawing has not been checkpointed")?.load(ProjectLimits::default(),&AtomicBool::new(false))?.ok_or("No saved checkpoint")?;
            SessionRestore::from_core(open)
        }).await.map_err(|e|format!("Snapshot reader stopped: {e:?}"))?
    }
    #[cfg(test)]
    pub fn published_path(&self)->PathBuf {self.path.clone()}
    fn execute(self: &Rc<Self>, first: Option<RecoveryWork>) {
        let Some(first) = first else { return; };
        self.running.set(true);
        let recovery = self.clone();
        let mut captured=self.snapshot.borrow().clone();
        glib::spawn_future_local(async move {
            let mut next = Some(first);
            while let Some(work) = next {
                let result = match work.kind {
                    RecoveryWorkKind::Capture => {
                        let staged=if recovery.registered.get() {Ok(())} else {
                            let window=recovery.window.borrow().upgrade();
                            match window {
                                Some(w)=>w.restart.stage(&w).await,
                                None=>Err("The drawing's window closed before session membership was published".into()),
                            }
                        };
                        if staged.is_ok() {recovery.registered.set(true);}
                        let snapshot = staged.and_then(|_|captured.take().ok_or_else(|| "Drawing snapshot unavailable".to_string()));
                        match snapshot {
                            Ok((mut snapshot, context)) => {
                                let path = recovery.path.clone();
                                let store = recovery.store.clone();
                                let discarded = recovery.discarded.clone();
                                gio::spawn_blocking(move || {
                                    context.install(snapshot.artwork_mut())?;
                                    let prepared = snapshot.prepare(&discarded)?;
                                    let mut store = store.lock().map_err(|e| e.to_string())?;
                                    if store.is_none() { *store = Some(SessionStore::open(&path)?); }
                                    if discarded.load(Ordering::Acquire) { return Ok(()); }
                                    store.as_mut().unwrap().commit(&prepared, &discarded).map(|_| ())
                                }).await.map_err(|e| format!("Session writer stopped: {e:?}")).and_then(|r| r)
                            }
                            Err(error) => Err(error),
                        }
                    }
                    RecoveryWorkKind::Retire => {
                        let store = recovery.store.clone();
                        gio::spawn_blocking(move || {
                            let mut store = store.lock().map_err(|e| e.to_string())?;
                            if let Some(store) = store.as_mut() { store.retire()?; }
                            Ok(())
                        }).await.map_err(|e| format!("Session cleanup stopped: {e:?}")).and_then(|r| r)
                    }
                };
                recovery.error.replace(result.as_ref().err().cloned());
                let update = recovery.policy.borrow_mut().event(RecoveryEvent::Complete { token: work.token, success: result.is_ok() }).unwrap();
                recovery.current.set(update.current);
                next = update.work;
                captured=recovery.snapshot.borrow().clone();
            }
            recovery.running.set(false);
            if recovery.current.get() || recovery.discarded.load(Ordering::Acquire) {recovery.snapshot.borrow_mut().take();}
        });
    }
}

pub(crate) struct Restart {
    path: RefCell<PathBuf>,
    lease: Arc<Mutex<Option<OwnerLock>>>,
    last: RefCell<Option<SessionManifest>>,
    running: Cell<bool>,
    restoring: Cell<bool>,
    retained: RefCell<Vec<SessionDrawing>>,
    collect_due: Cell<bool>,
    publishing: Cell<bool>,
    durable: Cell<bool>,
}
impl Default for Restart {
    fn default() -> Self { Self { path: RefCell::new(directory().join(format!("{}.json",key()))), lease: Default::default(), last: Default::default(), running: Cell::new(false), restoring: Cell::new(false), retained: Default::default(), collect_due: Cell::new(true), publishing: Cell::new(false), durable:Cell::new(true) } }
}
impl Restart {
    pub fn is_restoring(&self)->bool {self.restoring.get()}
    fn manifest(&self, w: &Workspace, clean_exit: bool) -> SessionManifest {
        let selected = w.documents.selected();
        let model = w.documents.model.borrow();
        let mut drawings: Vec<SessionDrawing> = model.order().iter().filter_map(|&id| {
            let recovery = if id == selected { w.recovery() } else { model.parked().find(|(key,_)| **key == id)?.1.owner.recovery.clone() };
            Some(SessionDrawing { id, key: recovery.path.file_name()?.to_string_lossy().into_owned() })
        }).collect();
        drawings.extend(self.retained.borrow().iter().cloned());
        SessionManifest { blocked: self.last.borrow().as_ref().map_or_else(Vec::new,|manifest|manifest.blocked.clone()), generation: 0, drawings, active: selected, clean_exit, restoring: Vec::new() }
    }
    async fn update(&self,change:impl FnOnce(SessionManifest)->Result<SessionManifest,String>)->Result<(),String> {
        self.update_checked(change,false).await.map(|_|())
    }
    async fn update_checked(&self,change:impl FnOnce(SessionManifest)->Result<SessionManifest,String>,accept_published:bool)->Result<Option<String>,String> {
        while self.publishing.get() {glib::timeout_future(Duration::from_millis(5)).await;}
        self.publishing.set(true);
        let previous=self.last.borrow().clone().unwrap_or_default();
        let result=match change(previous) {Ok(manifest)=>self.publish_inner(manifest,accept_published).await,Err(error)=>Err(error)};
        self.publishing.set(false);result
    }
    async fn begin_restore(&self,id:u64,retry:bool)->Result<layer_ui::SessionRestoreAttempt,String> {
        let mut ticket=None;
        self.update(|previous| {
            let mut next=if retry {previous.retry_restore(id)?} else {previous.begin_restore(id)?};
            next.clean_exit=false;
            ticket=next.restoring.iter().find(|attempt|attempt.id==id).copied();
            Ok(next)
        }).await?;
        ticket.ok_or_else(||"Missing restore attempt".into())
    }
    async fn publish_inner(&self, manifest: SessionManifest,accept_published:bool) -> Result<Option<String>,String> {
        if self.durable.get()&&self.last.borrow().as_ref().is_some_and(|last| {
            let mut last=last.clone(); let mut next=manifest.clone();last.generation=0;next.generation=0;
            serde_json::to_vec(&last).ok()==serde_json::to_vec(&next).ok()
        }) {return Ok(None);}
        let path = self.path.borrow().clone();
        let lease = self.lease.clone();
        let published=manifest.clone();
        let result=gio::spawn_blocking(move || {
            let _gate=storage_gate(path.parent().unwrap())?;
            let mut lease = lease.lock().map_err(|e|e.to_string())?;
            if lease.is_none() { *lease = Some(OwnerLock::claim(&path.with_extension("lock"))?.ok_or("Session is already open")?); }
            published.publish_checked(&path)
        }).await.map_err(|e|format!("Session membership writer stopped: {e:?}"))?;
        match result {
            Ok(())=>{self.last.replace(Some(manifest));self.durable.set(true);Ok(None)},
            Err(failure)=> {
                if failure.published {self.last.replace(Some(manifest));self.durable.set(false);}
                if accept_published&&failure.published {Ok(Some(failure.error))} else {Err(failure.error)}
            }
        }
    }
    async fn stage(&self,w:&Workspace)->Result<(),String> {
        self.update(|previous| {
            let desired=self.manifest(w,false);
            previous.stage(desired.drawings,desired.active)
        }).await
    }
    pub fn busy(&self)->bool {self.running.get()||self.restoring.get()}
    pub async fn checkpoint(&self, w: &Rc<Workspace>, clean_exit: bool) -> Result<(),String> {
        if !enabled() { return Ok(()); }
        if self.running.replace(true) { return Err("Session checkpoint is still being written".into()); }
        let result = self.checkpoint_inner(w,clean_exit).await;
        self.running.set(false);
        result
    }
    async fn checkpoint_inner(&self,w:&Rc<Workspace>,clean_exit:bool)->Result<(),String> {
        if self.restoring.get() { return Err("Drawings are still being restored".into()); }
        let mut owners = Vec::new();
        {
            let model = w.documents.model.borrow();
            for (_, parked) in model.parked() {
                let owner = parked.owner.recovery.clone();
                owner.retry();
                owners.push(owner);
            }
        }
        let active = w.recovery();
        active.capture(w);
        owners.push(active);
        for owner in owners { owner.flush().await?; }
        self.update(|previous| {
            let desired=self.manifest(w,clean_exit);
            previous.reconcile(desired.drawings,desired.active,clean_exit)
        }).await?;
        if !clean_exit&&self.collect_due.get() {
            match gio::spawn_blocking(collect_stores).await.map_err(|error|format!("Session cleanup stopped: {error:?}")).and_then(|result|result) {
                Ok(())=>self.collect_due.set(false),Err(error)=>w.changed(Err(error)),
            }
        }
        Ok(())
    }
    pub async fn remove(&self,w:&Rc<Workspace>,id:u64)->Result<(),String> {
        if !enabled() {return Ok(());}
        while self.running.get() { glib::timeout_future(Duration::from_millis(5)).await; }
        self.running.set(true);
        let result=self.update_checked(|previous| {
            let removed=previous.remove(id)?;
            let active=w.documents.model.borrow().after_close().filter(|id|removed.drawings.iter().any(|drawing|drawing.id==*id)).unwrap_or(removed.active);
            removed.reconcile(removed.drawings.clone(),active,false)
        },true).await;
        if let Ok(warning)=&result {self.collect_due.set(true);if let Some(warning)=warning {w.changed(Err(warning.clone()));}}
        self.running.set(false);
        result.map(|_|())
    }
}

pub(crate) fn install(w:&Rc<Workspace>) {
    if !enabled() { return; }
    let weak=Rc::downgrade(w);
    glib::timeout_add_local(Duration::from_secs(2),move || {
        let Some(w)=weak.upgrade() else {return glib::ControlFlow::Break;};
        if !w.restart.running.get() && !w.documents.closing_window.get() && !w.documents.changing.get() && !w.restart.is_restoring() && !w.restart_busy() {
            glib::spawn_future_local(async move {
                if let Err(error)=w.restart.checkpoint(&w,false).await { w.changed(Err(error)); }
            });
        }
        glib::ControlFlow::Continue
    });
}

struct ClaimedWindow { path: PathBuf, manifest: SessionManifest, lease: OwnerLock }
fn abandoned() -> Result<Vec<ClaimedWindow>,String> {
    let dir=directory();
    let _gate=storage_gate(&dir)?;
    let mut paths=std::fs::read_dir(&dir).map_err(|e|e.to_string())?.flatten().map(|entry|entry.path()).filter(|path|path.extension().is_some_and(|e|e=="json")).collect::<Vec<_>>();
    paths.sort();
    let mut result=Vec::new();
    for path in paths {
        let lease=match OwnerLock::claim(&path.with_extension("lock")) {Ok(Some(lease))=>lease,Ok(None)=>continue,Err(error)=>{eprintln!("Saved session claim failed: {error}");continue;}};
        let bytes=match layer_core::package::session_store::read_bounded(&path,1024*1024) {Ok(bytes)=>bytes,Err(error)=>{eprintln!("Saved session read failed; copy retained: {error}");continue;}};
        let manifest:SessionManifest=match serde_json::from_slice(&bytes) {Ok(manifest)=>manifest,Err(error)=>{eprintln!("Cannot read saved session; copy retained: {error}");continue;}};
        if let Err(error)=manifest.validate() {eprintln!("Saved session validation failed; copy retained: {error}");continue;}
        if manifest.drawings.is_empty() {std::fs::remove_file(&path).map_err(|e|e.to_string())?;continue;}
        result.push(ClaimedWindow {path,manifest,lease});
    }
    Ok(result)
}
pub(crate) fn restore_stale(w:&Rc<Workspace>,app:&adw::Application,active:&Rc<RefCell<Vec<Rc<Workspace>>>>) {
    if !enabled() {return;}
    w.restart.restoring.set(true);
    let w=w.clone(); let app=app.clone(); let active=active.clone();
    glib::spawn_future_local(async move {
        while w.gpu.borrow().is_none() {
            if !w.window.is_visible() {w.restart.restoring.set(false);return;}
            glib::timeout_future(Duration::from_millis(5)).await;
        }
        let startup=w.gpu.borrow().as_ref().map(|gpu|gpu.session.session_stamp());
        let result=gio::spawn_blocking(abandoned).await.map_err(|e|format!("Session reader stopped: {e:?}")).and_then(|r|r);
        match result {
            Ok(claims)=>{
                for (index,claim) in claims.into_iter().enumerate() {
                    let window=if index==0 {w.clone()} else {
                        crate::open_workspace_ready(&app,&active,None,None,w.localization());
                        active.borrow().last().unwrap().clone()
                    };
                    window.restart.restoring.set(true);
                    if let Err(error)=restore_window(&window,claim,if index==0 {startup.clone()} else {None}).await {window.changed(Err(error));}
                    window.restart.restoring.set(false);
                }
            }
            Err(error)=>w.changed(Err(error)),
        }
        w.restart.restoring.set(false);
    });
}
async fn restore_window(w:&Rc<Workspace>,claim:ClaimedWindow,startup:Option<layer_ui::session_recovery::SessionStamp>)->Result<(),String> {
    *w.restart.path.borrow_mut()=claim.path;
    *w.restart.lease.lock().map_err(|e|e.to_string())?=Some(claim.lease);
    let abnormal=!claim.manifest.clean_exit;
    let mut original=claim.manifest.clone();
    original=original.recover_interrupted()?;
    w.restart.last.replace(Some(original.clone()));
    while w.gpu.borrow().is_none() {
        if !w.window.is_visible() {return Err("Session window closed before restoration".into());}
        glib::timeout_future(Duration::from_millis(5)).await;
    }
    let reserved=original.drawings.iter().map(|drawing|drawing.id).collect::<Vec<_>>();
    w.documents.model.borrow_mut().reserve_identities(&reserved)?;
    *w.restart.retained.borrow_mut()=original.drawings.clone();
    let mut drawings=original.drawings.clone();
    drawings.sort_by_key(|drawing|drawing.id!=original.active);
    let mut restored_ids=Vec::new();
    let mut identity=std::collections::BTreeMap::new();
    let mut replace_initial=true;
    let mut launch_stamp=startup;
    let mut protected_placeholder=false;
    for drawing in drawings {
        if original.blocked.contains(&drawing.id) {continue;}
        while w.gpu.borrow().is_none() || w.documents.changing.get() || w.documents.has_pending_open() || w.servicing.get() {
            if !w.window.is_visible() || w.documents.closing_window.get() {return Err("Restoration cancelled; saved drawings were kept".into());}
            glib::timeout_future(Duration::from_millis(10)).await;
        }
        if launch_stamp.is_none() {launch_stamp=w.gpu.borrow().as_ref().map(|gpu|gpu.session.session_stamp());}
        let attempt=w.restart.begin_restore(drawing.id,false).await?;
        let path=directory().join(&drawing.key);
        let result=gio::spawn_blocking(move || {
            let mut store=SessionStore::open(&path)?;
            let opened=store.load(ProjectLimits::default(),&AtomicBool::new(false))?.ok_or("Saved drawing has no complete checkpoint")?;
            let restored=SessionRestore::from_core(opened)?;
            let observed=observed_destination(&restored);
            Ok::<_,String>((store,restored,observed))
        }).await.map_err(|e|format!("Drawing reader stopped: {e:?}")).and_then(|r|r);
        let (store,restored,observed)=match result {Ok(result)=>result,Err(error)=>{
            w.restart.update(|manifest|manifest.finish_restore(attempt,false)).await?;
            w.changed(Err(error));continue;
        }};
        let recovered=abnormal||store.recovered_previous();
        let recovery=Rc::new(Recovery {path:store.path().to_path_buf(),store:Arc::new(Mutex::new(Some(store))),registered:Cell::new(true),..Default::default()});
        recovery.recovered.set(recovered);
        let pristine=w.gpu.borrow().as_ref().is_some_and(|gpu|launch_stamp.as_ref().is_some_and(|stamp|gpu.session.can_replace_startup_session(stamp)));
        if replace_initial&&!pristine {protected_placeholder=true;}
        let mut desired=if w.documents.model.borrow().order().contains(&drawing.id) && !(replace_initial&&pristine&&w.documents.len()==1) {
            let used=original.drawings.iter().map(|drawing|drawing.id).chain(w.documents.model.borrow().order().iter().copied()).collect::<std::collections::BTreeSet<_>>();
            (1..=layer_ui::MAX_SESSION_DRAWING_ID).find(|id|!used.contains(id)).ok_or("No drawing identity is available")?
        } else {drawing.id};
        let result=if replace_initial&&pristine&&w.documents.len()==1 {
            match w.documents.open_restored(w,restored,recovery,observed,launch_stamp.clone().ok_or("Missing startup drawing stamp")?,desired).await {Ok(id)=>{desired=id;Ok(())},Err(error)=>Err(error)}
        } else {w.documents.restore_inactive(w,restored,recovery,observed,desired).await};
        if let Err(error)=result {
            w.restart.update(|manifest|manifest.finish_restore(attempt,false)).await?;w.changed(Err(error));continue;
        }
        identity.insert(drawing.id,desired);
        restored_ids.push(drawing.id);
        w.restart.retained.borrow_mut().retain(|entry|entry.id!=drawing.id);
        replace_initial=false;
        w.restart.update(|manifest| {
            let manifest=manifest.finish_restore(attempt,true)?;
            if desired!=drawing.id {manifest.remap(&[(drawing.id,desired)])} else {Ok(manifest)}
        }).await?;
    }
    if !protected_placeholder {if let Some(&id)=identity.get(&original.active) {w.documents.activate(w,id).await?;}}
    let mut order=original.drawings.iter().filter(|drawing|restored_ids.contains(&drawing.id)).map(|drawing|identity[&drawing.id]).collect::<Vec<_>>();
    for &id in w.documents.model.borrow().order() {if !order.contains(&id) {order.push(id);}}
    let selected=w.documents.selected();
    w.documents.model.borrow_mut().restore_order(&order,selected)?;
    let mut retained=w.restart.retained.borrow().clone();
    let mut used=original.drawings.iter().map(|drawing|drawing.id).chain(order.iter().copied()).collect::<std::collections::BTreeSet<_>>();
    let mut mapping=Vec::new();
    for drawing in &mut retained {
        if order.contains(&drawing.id) {
            let id=(1..=layer_ui::MAX_SESSION_DRAWING_ID).find(|id|!used.contains(id)).ok_or("No drawing identity is available")?;
            used.insert(id);mapping.push((drawing.id,id));drawing.id=id;
        }
    }
    if !mapping.is_empty() {
        w.restart.update(|manifest|manifest.remap(&mapping)).await?;w.restart.retained.replace(retained);
    }
    let reserved=w.restart.retained.borrow().iter().map(|drawing|drawing.id).collect::<Vec<_>>();
    w.documents.model.borrow_mut().reserve_identities(&reserved)?;
    w.documents.refresh(w);
    failed_drawings(w).await
}

fn observed_destination(restored:&SessionRestore)->Option<layer_ui::DestinationFingerprint> {
    let location=restored.state.location.as_ref()?;
    let path=gio::File::for_uri(&location.uri).path()?;
    layer_ui::DestinationFingerprint::observe_path(&path,restored.state.destination.as_ref())
}

async fn failed_drawings(w:&Rc<Workspace>)->Result<(),String> {
    let drawings=w.restart.retained.borrow().clone();
    for drawing in drawings {
        if !w.window.is_visible()||w.documents.closing_window.get() {break;}
        let copy=layer_ui::bootstrap_view(&w.localization()).recovery;
        let dialog=adw::AlertDialog::builder().heading(copy.title.as_ref()).body(copy.explanation.as_ref()).build();
        dialog.add_responses(&[("later",copy.later.as_ref()),("discard",copy.discard.as_ref()),("retry",copy.retry.as_ref())]);
        w.on_localization(glib::clone!(#[weak] dialog,#[upgrade_or] false,move |localization| {
            let copy=layer_ui::bootstrap_view(localization).recovery;
            dialog.set_heading(Some(&copy.title));dialog.set_body(&copy.explanation);
            dialog.set_response_label("later",&copy.later);dialog.set_response_label("discard",&copy.discard);dialog.set_response_label("retry",&copy.retry);true
        }));
        dialog.set_close_response("later");dialog.set_default_response(Some("later"));
        dialog.set_response_appearance("discard",adw::ResponseAppearance::Destructive);
        let response=crate::alert::choose(dialog,&w.window).await;
        if response=="discard" {
            let path=directory().join(&drawing.key);
            gio::spawn_blocking(move ||layer_core::package::session_store::prepare_store_retirement(&path)).await.map_err(|error|format!("Drawing retirement preparation stopped: {error:?}"))??;
            if let Some(warning)=w.restart.update_checked(|manifest|manifest.remove(drawing.id),true).await? {w.changed(Err(warning));}
            w.restart.retained.borrow_mut().retain(|entry|entry.id!=drawing.id);
            let result=gio::spawn_blocking(collect_stores).await.map_err(|error|format!("Drawing cleanup stopped: {error:?}")).and_then(|result|result);
            if let Err(error)=result {w.changed(Err(error));}
        } else if response=="retry" {
            let attempt=w.restart.begin_restore(drawing.id,true).await?;
            let path=directory().join(&drawing.key);
            let result=gio::spawn_blocking(move || {
                let mut store=SessionStore::open(&path)?;
                let opened=store.load(ProjectLimits::default(),&AtomicBool::new(false))?.ok_or("No complete drawing checkpoint")?;
                let restored=SessionRestore::from_core(opened)?;
                let observed=observed_destination(&restored);
                Ok::<_,String>((restored,store,observed))
            }).await.map_err(|error|format!("Drawing reader stopped: {error:?}")).and_then(|result|result);
            let result=match result {
                Ok((restore,store,observed))=>{
                    let recovery=Rc::new(Recovery {path:store.path().to_path_buf(),store:Arc::new(Mutex::new(Some(store))),registered:Cell::new(true),..Default::default()});
                    recovery.recovered.set(true);
                    w.documents.restore_inactive(w,restore,recovery,observed,drawing.id).await
                },
                Err(error)=>Err(error),
            };
            w.restart.update(|manifest|manifest.finish_restore(attempt,result.is_ok())).await?;
            if result.is_ok() {w.restart.retained.borrow_mut().retain(|entry|entry.id!=drawing.id);} else if let Err(error)=result {w.changed(Err(error));}
        }
    }
    Ok(())
}
