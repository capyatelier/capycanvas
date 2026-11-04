use layer_core::{PortableId,ProjectLimits,package::session_store::{DrawingStore,StoreOpener},raster_storage::RetainedTiles};
use layer_ui::{SessionCapture,SessionStamp};
#[cfg(not(target_arch="wasm32"))]
use futures_util::FutureExt;
use std::{collections::{BTreeMap,VecDeque},path::PathBuf,sync::{Arc,Mutex,atomic::AtomicBool},task::{Poll,Waker}};

#[cfg(not(target_arch="wasm32"))]
type StoreCompletion=Box<dyn FnOnce(Result<(),String>)+Send>;
#[cfg(target_arch="wasm32")]
type StoreCompletion=Box<dyn FnOnce(Result<(),String>)>;
#[cfg(not(target_arch="wasm32"))]
pub type StoreExecutor=Arc<dyn Fn(StoreWork)+Send+Sync>;
#[cfg(target_arch="wasm32")]
pub type StoreExecutor=Arc<dyn Fn(StoreWork)>;

pub enum StoreResult {PackageCopy {inventory:Arc<RetainedTiles>,copy:Option<layer_core::package::session_store::PackageCopy>,progress:bool},Checkpoint(PortableId),Imported(Result<PortableId,layer_core::package::session_store::ImportFailure>),Durable(usize),StorageReady(bool),Restored {session:Option<layer_ui::SessionRestore>,recovered_previous:bool},RetirementPrepared,Retired,Superseded}
#[derive(Default)]
struct ReplyState {result:Option<Result<StoreResult,String>>,waker:Option<Waker>,completion:Option<StoreCompletion>}
#[derive(Clone,Default)]
pub struct StoreReply(Arc<(Mutex<ReplyState>,std::sync::Condvar)>);
impl StoreReply {
    fn finish(&self,result:Result<StoreResult,String>) {
        let mut state=self.0.0.lock().unwrap();state.result=Some(result);let wake=state.waker.take();let completion=state.completion.take();
        let status=state.result.as_ref().unwrap().as_ref().map(|_|()).map_err(Clone::clone);self.0.1.notify_all();drop(state);
        if let Some(wake)=wake {wake.wake();}if let Some(completion)=completion {completion(status);}
    }
    fn on_finished(&self,completion:StoreCompletion) {
        let mut state=self.0.0.lock().unwrap();
        if let Some(result)=&state.result {let result=result.as_ref().map(|_|()).map_err(Clone::clone);drop(state);completion(result);}
        else {state.completion=Some(completion);}
    }
    pub fn try_take(&self)->Option<Result<StoreResult,String>> {self.0.0.lock().unwrap().result.take()}
    #[cfg(not(target_arch="wasm32"))]
    pub fn wait_on_worker(self)->Result<StoreResult,String> {
        let mut state=self.0.0.lock().unwrap();
        loop {if let Some(result)=state.result.take() {return result;}state=self.0.1.wait(state).map_err(|_|"Storage completion failed")?;}
    }
}
impl std::future::Future for StoreReply {
    type Output=Result<StoreResult,String>;
    fn poll(self:std::pin::Pin<&mut Self>,context:&mut std::task::Context<'_>)->Poll<Self::Output> {
        let mut state=self.0.0.lock().unwrap();
        if let Some(result)=state.result.take() {Poll::Ready(result)}else {state.waker=Some(context.waker().clone());Poll::Pending}
    }
}
pub type CapturePreparation=Box<dyn FnOnce(&mut SessionCapture)->Result<(),String>+Send>;
enum Work {Package {inventory:Arc<RetainedTiles>,copy:Option<layer_core::package::session_store::PackageCopy>,checkpoint:bool},Checkpoint {capture:SessionCapture,stamp:SessionStamp,forced:bool,prepare:Option<CapturePreparation>},Pressure(Vec<RetainedTiles>),Import {expected:Option<PortableId>,metadata:Arc<[u8]>,resources:BTreeMap<PortableId,layer_core::package::ImmutableBacking>},ImportTiles {tiles:RetainedTiles,_reservation:Option<layer_core::MemoryReservation>},Compact,SampleStorage,Restore(ProjectLimits),PrepareRetirement,Retire}
struct Request {work:Work,replies:Vec<StoreReply>}
impl Request {
    fn finish(self,result:Result<StoreResult,String>) {
        let mut result=Some(result);let count=self.replies.len();
        for (index,reply) in self.replies.into_iter().enumerate() {
            let value=if index+1==count {result.take().unwrap()}else {match result.as_ref().unwrap() {
                Ok(StoreResult::Imported(result))=>Ok(StoreResult::Imported(result.clone())),Ok(StoreResult::Checkpoint(id))=>Ok(StoreResult::Checkpoint(*id)),Ok(StoreResult::Durable(bytes))=>Ok(StoreResult::Durable(*bytes)),
                Ok(StoreResult::StorageReady(ready))=>Ok(StoreResult::StorageReady(*ready)),Ok(StoreResult::RetirementPrepared)=>Ok(StoreResult::RetirementPrepared),Ok(StoreResult::Retired)=>Ok(StoreResult::Retired),Ok(StoreResult::Superseded)=>Ok(StoreResult::Superseded),Err(error)=>Err(error.clone()),
                Ok(StoreResult::Restored {..}|StoreResult::PackageCopy {..})=>unreachable!(),
            }};reply.finish(value);
        }
    }
}
#[derive(Default)]
struct Pending {checkpoint:Option<Request>,pressure:Option<Request>,restores:VecDeque<Request>,preparations:VecDeque<Request>,retire:Option<Request>,compaction:Option<Request>}
impl Pending {
    fn take(&mut self)->Option<Request> {self.retire.take().or_else(||self.restores.pop_front()).or_else(||self.pressure.take()).or_else(||self.checkpoint.take()).or_else(||self.preparations.pop_front()).or_else(||self.compaction.take())}
    fn push(&mut self,mut request:Request)->Vec<Request> {
        if self.retire.is_some() && !matches!(request.work,Work::Retire) {return vec![request];}
        match &mut request.work {
            Work::Checkpoint {forced,..}=>{
                if let Some(previous)=self.checkpoint.take() {
                    if let Work::Checkpoint {forced:old,..}=previous.work {*forced|=old;}
                    request.replies.extend(previous.replies);
                }
                self.checkpoint=Some(request);
            },
            Work::Pressure(tiles)=>{
                if let Some(previous)=self.pressure.take() {
                    if let Work::Pressure(old)=previous.work {tiles.extend(old);}
                    request.replies.extend(previous.replies);
                }
                self.pressure=Some(request);
            },
            Work::ImportTiles {..}|Work::SampleStorage|Work::Restore(_)|Work::Import {..}=>self.restores.push_back(request),
            Work::Package {..}|Work::Compact=>{if let Some(previous)=self.compaction.replace(request) {return vec![previous];}},
            Work::PrepareRetirement=>self.preparations.push_back(request),
            Work::Retire=>{
                if let Some(previous)=self.retire.take() {request.replies.extend(previous.replies);}
                self.retire=Some(request);
                return self.pressure.take().into_iter().chain(self.checkpoint.take()).chain(self.restores.drain(..)).chain(self.preparations.drain(..)).chain(self.compaction.take()).collect();
            },
        }Vec::new()
    }
}
use layer_core::package::storage_budget::StorageRefusal;
struct Drawing {
    memory:Option<layer_core::DocumentMemory>,package:Option<(Arc<RetainedTiles>,Option<layer_core::package::session_store::PackageCopy>)>,binding:DrawingStore,observed:Option<SessionStamp>,committed:Option<SessionStamp>,committed_at:Option<u64>,due:Option<u64>,deferred_since:Option<u64>,
    storage_error:Option<StorageRefusal>,next_storage_sample:u64,next_compaction:u64,sequence:u64,running:bool,inflight:Option<SessionStamp>,retiring:bool,pending:Pending,
}
struct State {drawings:BTreeMap<String,Drawing>,aliases:BTreeMap<u64,String>,next_compaction:u64}
struct Inner {
    assets:layer_ui::AssetStore,resolver:std::sync::OnceLock<layer_ui::AssetResolver>,root:PathBuf,opener:Arc<dyn StoreOpener>,clock:fn()->u64,state:Mutex<State>,execute:StoreExecutor,wake:Mutex<Arc<dyn Fn()+Send+Sync>>,pressure:Mutex<Option<(layer_ui::StoragePressure,u64)>>,trims:Mutex<TrimRequests>,
}
#[derive(Clone)]
pub struct SessionStores(Arc<Inner>);
impl SessionStores {
    #[cfg(not(target_arch="wasm32"))]
    pub fn native(root:PathBuf,wake:Arc<dyn Fn()+Send+Sync>)->Self {Self::new(root,Arc::new(native_worker),wake)}
    #[cfg(not(target_arch="wasm32"))]
    pub fn new(root:PathBuf,execute:StoreExecutor,wake:Arc<dyn Fn()+Send+Sync>)->Self {
        Self::with_opener(root,Arc::new(layer_core::package::session_store::NativeStoreOpener),execute,wake,native_now_ms)
    }
    pub fn with_opener(root:PathBuf,opener:Arc<dyn StoreOpener>,execute:StoreExecutor,wake:Arc<dyn Fn()+Send+Sync>,clock:fn()->u64)->Self {
        let stores=Self(Arc::new(Inner {assets:Default::default(),resolver:Default::default(),root,opener,clock,state:Mutex::new(State {drawings:BTreeMap::new(),aliases:BTreeMap::new(),next_compaction:0}),execute,wake:Mutex::new(wake),pressure:Mutex::new(None),trims:Mutex::new(TrimRequests::default())}));
        stores.0.resolver.set(stores.create_asset_resolver()).unwrap_or_else(|_|unreachable!());
        stores.0.assets.attach(stores.asset_resolver()).expect("new store registration");stores
    }
    pub fn set_wake(&self,wake:Arc<dyn Fn()+Send+Sync>) {*self.0.wake.lock().unwrap()=wake;}
    fn wake(&self) {let wake=self.0.wake.lock().unwrap().clone();wake();}
    pub fn bind(&self,id:u64,key:Option<&str>)->Result<DrawingStore,String> {
        let mut state=self.0.state.lock().unwrap();
        if let Some(current)=state.aliases.get(&id) {
            if key.is_some_and(|key|key!=current) {return Err("Drawing storage identity changed".into());}
            return Ok(state.drawings[current].binding.clone());
        }
        let binding=DrawingStore::with_opener(&self.0.root,key,self.0.opener.clone())?;
        let key=binding.key().to_owned();
        if state.drawings.len()>=layer_ui::session_recovery::MAX_SESSION_DRAWINGS && !state.drawings.contains_key(&key) {return Err("Too many session drawings".into());}
        state.drawings.entry(key.clone()).or_insert_with(||Drawing::new(binding));
        state.aliases.insert(id,key.clone());Ok(state.drawings[&key].binding.clone())
    }
    pub fn attach_session<R:layer_render::CanvasRenderer>(&self,id:u64,session:&layer_ui::UiSession<R>,pressure:layer_ui::StoragePressure)->Result<bool,String> {
        if session.engine().document().memory().storage().is_some_and(|binding|binding.path().parent()!=Some(self.0.root.as_path())) {return Ok(false);}
        self.install_storage(id,session)?;
        self.watch_pressure(pressure);Ok(true)
    }
    #[cfg(not(target_arch="wasm32"))]
    pub fn attach_window<P:crate::window::Parked>(&self,window:&crate::window::DocumentWindow<P>,host:&crate::NativeHost)->Result<bool,String> {
        if !self.attach_session(window.documents.selected(),&host.session,window.documents.pressure_source())? {return Ok(false);}
        for &id in window.documents.order() {
            if id==window.documents.selected() {continue;}
            let session=window.session(host,id)?;
            if session.engine().document().memory().storage().is_none_or(|binding|binding.path().parent()==Some(self.0.root.as_path())) {self.install_storage(id,session)?;}
        }
        Ok(true)
    }
    pub fn install_storage<R:layer_render::CanvasRenderer>(&self,id:u64,session:&layer_ui::UiSession<R>)->Result<DrawingStore,String> {
        let binding=match session.engine().document().memory().storage().or_else(||session.asset_writer().binding()) {Some(binding)=>binding,None=>self.bind(id,None)?};
        let binding=session.install_asset_store(self.asset_resolver(),binding)?;
        self.adopt_binding(id,binding.clone())?;
        self.0.state.lock().unwrap().drawings.get_mut(binding.key()).unwrap().memory=Some(session.engine().document().memory().clone());
        Ok(binding)
    }
    pub fn asset_writer(&self,id:u64)->Result<layer_ui::AssetWriter,String> {self.0.assets.writer().for_binding(self.bind(id,None)?)}
    fn asset_resolver(&self)->layer_ui::AssetResolver {self.0.resolver.get().unwrap().clone()}
    fn create_asset_resolver(&self)->layer_ui::AssetResolver {
        let weak=Arc::downgrade(&self.0);
        layer_ui::AssetResolver::new(self.0.root.clone(),Arc::new(move |binding| {
            let stores=SessionStores(weak.upgrade().ok_or("Drawing storage owner closed")?);
            let binding=match binding {Some(binding)=>binding,None=>DrawingStore::with_opener(&stores.0.root,None,stores.0.opener.clone())?};
            if binding.path().parent()!=Some(stores.0.root.as_path()) {return Err("Drawing store belongs to another session root".into());}
            let weak=Arc::downgrade(&stores.0);let target=binding.clone();
            Ok(layer_ui::AssetBinding {store:binding,write:Arc::new(move |tiles,memory| {
                let stores=SessionStores(weak.upgrade().ok_or("Drawing storage owner closed")?);
                let reply=stores.request_binding(target.key(),Some(target.clone()),Work::ImportTiles {tiles,_reservation:memory})?;
                Ok(Box::pin(async move {reply.await?;Ok(())}))
            })})
        }))
    }
    pub fn remap(&self,mapping:&[(u64,u64)]) {
        let mut state=self.0.state.lock().unwrap();
        let mapped=mapping.iter().filter_map(|&(old,new)|state.aliases.remove(&old).map(|key|(new,key))).collect::<Vec<_>>();
        state.aliases.extend(mapped);
    }
    pub fn release_binding(&self,id:u64) {
        let mut state=self.0.state.lock().unwrap();
        if let Some(key)=state.aliases.remove(&id) && !state.aliases.values().any(|current|current==&key)
            && state.drawings.get(&key).is_some_and(|drawing|!drawing.running) {state.drawings.remove(&key);}
    }
    pub fn adopt(&self,id:u64,key:&str)->Result<DrawingStore,String> {
        let binding=DrawingStore::with_opener(&self.0.root,Some(key),self.0.opener.clone())?;
        self.adopt_binding(id,binding)?;
        self.bind(id,Some(key))
    }
    pub fn observe(&self,id:u64,stamp:SessionStamp,now_ms:u64,immediate:bool)->Result<bool,String> {
        self.bind(id,None)?;
        let mut state=self.0.state.lock().unwrap();let key=state.aliases[&id].clone();let drawing=state.drawings.get_mut(&key).unwrap();
        drawing.observed=Some(stamp);
        if drawing.storage_error.is_some() {
            let sample=!drawing.running && now_ms>=drawing.next_storage_sample;
            if sample {drawing.next_storage_sample=now_ms.saturating_add(Self::checkpoint_interval_ms());}
            drop(state);if sample {self.request_key(&key,Work::SampleStorage)?;}return Ok(false);
        }
        if drawing.observed.as_ref().zip(drawing.committed.as_ref()).is_some_and(|(a,b)|same_checkpoint(a,b)) {
            drawing.due=None;
            drop(state);self.maintain()?;return Ok(false);
        }
        if immediate {drawing.due=Some(now_ms);}
        let deadline=drawing.committed_at.unwrap_or(now_ms).saturating_add(Self::checkpoint_interval_ms());
        let due=*drawing.due.get_or_insert(deadline);
        let ready=!drawing.running && now_ms>=due;
        drop(state);self.copy_package(&key);Ok(ready)
    }
    fn copy_package(&self,key:&str)->bool {
        let mut state=self.0.state.lock().unwrap();let Some(drawing)=state.drawings.get_mut(key) else {return false;};
        if drawing.running || drawing.retiring || drawing.storage_error.is_some() {return false;}
        let checkpoint=drawing.deferred_since.is_some_and(|since|(self.0.clock)().saturating_sub(since)>=5*Self::checkpoint_interval_ms());
        if !layer_core::package::session_store::CompactionActivity::ready(checkpoint) {return false;}
        let Some(inventory)=drawing.memory.as_ref().and_then(layer_core::DocumentMemory::package_inventory) else {return false;};
        let copy=match drawing.package.take() {
            Some((current,copy)) if Arc::ptr_eq(&current,&inventory)=>match copy {
                Some(copy)=>Some(copy),None=>{drawing.package=Some((current,None));return false;},
            },_=>None,
        };
        let task=self.start(drawing,Request {work:Work::Package {inventory,copy,checkpoint},replies:Vec::new()});
        drop(state);(self.0.execute)(task);true
    }
    pub fn maintain(&self)->Result<(),String> {
        let keys=self.0.state.lock().unwrap().drawings.keys().cloned().collect::<Vec<_>>();
        for key in keys {if self.copy_package(&key) {return Ok(());}}
        if !layer_core::package::session_store::CompactionActivity::idle() {return Ok(());}
        let now=(self.0.clock)();let mut state=self.0.state.lock().unwrap();
        if now<state.next_compaction {return Ok(());}
        let key=state.drawings.iter().filter(|(_,drawing)|!drawing.running && !drawing.retiring && drawing.storage_error.is_none()
            && drawing.observed.as_ref().is_some_and(|stamp|drawing.current(stamp)))
            .min_by_key(|(_,drawing)|drawing.next_compaction).map(|(key,_)|key.clone());
        let Some(key)=key else {return Ok(());};
        state.next_compaction=now.saturating_add(Self::checkpoint_interval_ms());
        state.drawings.get_mut(&key).unwrap().next_compaction=state.next_compaction;
        drop(state);self.request_key(&key,Work::Compact)?;Ok(())
    }
    pub const fn checkpoint_interval_ms()->u64 {layer_ui::session_recovery::CHECKPOINT_INTERVAL_MS}
    pub fn checkpoint_due(&self,id:u64,stamp:SessionStamp,force:bool)->Result<bool,String> {
        self.bind(id,None)?;
        let (current,first)={let state=self.0.state.lock().unwrap();let drawing=&state.drawings[&state.aliases[&id]];(drawing.current(&stamp),drawing.committed.is_none())};
        if current {self.observe(id,stamp,(self.0.clock)(),false)?;return Ok(false);}
        if !force {
            let mut state=self.0.state.lock().unwrap();let key=state.aliases[&id].clone();let drawing=state.drawings.get_mut(&key).unwrap();
            if drawing.memory.as_ref().is_some_and(layer_core::DocumentMemory::package_pending) {
                drawing.deferred_since.get_or_insert((self.0.clock)());drop(state);self.copy_package(&key);return Ok(false);
            }
        }
        let urgent=force || first;
        let ready=self.observe(id,stamp,(self.0.clock)(),urgent)?;
        let state=self.0.state.lock().unwrap();let drawing=&state.drawings[&state.aliases[&id]];
        if force && drawing.storage_error.is_some() {return Err(layer_core::package::storage_budget::STORAGE_UNAVAILABLE.into());}
        Ok(drawing.storage_error.is_none() && (urgent || ready))
    }
    pub fn is_current(&self,id:u64,stamp:&SessionStamp)->bool {
        let state=self.0.state.lock().unwrap();state.aliases.get(&id).and_then(|key|state.drawings.get(key)).is_some_and(|drawing|drawing.current(stamp))
    }
    pub fn checkpoint(&self,id:u64,capture:SessionCapture,stamp:SessionStamp,forced:bool)->Result<StoreReply,String> {
        self.request(id,Work::Checkpoint {capture,stamp,forced,prepare:None})
    }
    pub fn checkpoint_bound(&self,binding:&DrawingStore,capture:SessionCapture,stamp:SessionStamp,forced:bool,prepare:Option<CapturePreparation>)->Result<StoreReply,String> {
        self.request_key(binding.key(),Work::Checkpoint {capture,stamp,forced,prepare})
    }
    pub fn pressure_bound(&self,binding:&DrawingStore,tiles:RetainedTiles)->Result<StoreReply,String> {self.request_key(binding.key(),Work::Pressure(vec![tiles]))}
    pub fn restore_bound(&self,binding:&DrawingStore,limits:ProjectLimits)->Result<StoreReply,String> {self.request_key(binding.key(),Work::Restore(limits))}
    pub fn retire_bound(&self,binding:&DrawingStore)->Result<StoreReply,String> {self.request_key(binding.key(),Work::Retire)}
    pub fn adopt_binding(&self,id:u64,binding:DrawingStore)->Result<(),String> {
        if binding.path().parent()!=Some(self.0.root.as_path()) {return Err("Drawing store belongs to another session root".into());}
        let mut state=self.0.state.lock().unwrap();let key=binding.key().to_owned();
        if state.drawings.len()>=layer_ui::session_recovery::MAX_SESSION_DRAWINGS && !state.drawings.contains_key(&key) {return Err("Too many session drawings".into());}
        state.drawings.entry(key.clone()).or_insert_with(||Drawing::new(binding));
        state.aliases.retain(|&current,current_key|current==id || current_key!=&key);
        let previous=state.aliases.insert(id,key);
        if let Some(previous)=previous && !state.aliases.values().any(|key|key==&previous) && state.drawings.get(&previous).is_some_and(|drawing|!drawing.running) {state.drawings.remove(&previous);}
        Ok(())
    }
    pub fn import(&self,id:u64,capture:SessionCapture,stamp:SessionStamp)->Result<StoreReply,String> {self.checkpoint(id,capture,stamp,true)}
    pub fn pressure(&self,id:u64,tiles:RetainedTiles)->Result<StoreReply,String> {self.request(id,Work::Pressure(vec![tiles]))}
    pub fn restore(&self,id:u64,limits:ProjectLimits)->Result<StoreReply,String> {self.request(id,Work::Restore(limits))}
    pub fn retire(&self,id:u64)->Result<StoreReply,String> {self.request(id,Work::Retire)}
    pub fn prepare_retirement(&self,key:&str)->Result<StoreReply,String> {self.request_copy(key,Work::PrepareRetirement)}
    pub fn retire_copy(&self,key:&str)->Result<StoreReply,String> {self.request_copy(key,Work::Retire)}
    fn request_copy(&self,key:&str,work:Work)->Result<StoreReply,String> {
        let binding=DrawingStore::with_opener(&self.0.root,Some(key),self.0.opener.clone())?;
        self.request_binding(key,Some(binding),work)
    }
    pub fn import_legacy(&self,key:&str,expected:Option<PortableId>,metadata:Arc<[u8]>,resources:BTreeMap<PortableId,layer_core::package::ImmutableBacking>)->Result<StoreReply,String> {
        self.request_copy(key,Work::Import {expected,metadata,resources})
    }
    pub fn bindings(&self)->Vec<(u64,DrawingStore)> {
        {let state=self.0.state.lock().unwrap();state.aliases.iter().map(|(&id,key)|(id,state.drawings[key].binding.clone())).collect()}
    }
    fn request(&self,id:u64,work:Work)->Result<StoreReply,String> {
        let binding=self.bind(id,None)?;self.request_key(binding.key(),work)
    }
    fn request_key(&self,key:&str,work:Work)->Result<StoreReply,String> {
        self.request_binding(key,None,work)
    }
    fn request_binding(&self,key:&str,binding:Option<DrawingStore>,work:Work)->Result<StoreReply,String> {
        if !matches!(work,Work::Package {..}|Work::Compact|Work::SampleStorage|Work::Checkpoint {forced:false,..}) {layer_core::package::session_store::CompactionActivity::interrupt();}
        let reply=StoreReply::default();let request=Request {work,replies:vec![reply.clone()]};
        let mut state=self.0.state.lock().unwrap();
        if let Some(binding)=binding {
            if state.drawings.len()>=layer_ui::session_recovery::MAX_SESSION_DRAWINGS && !state.drawings.contains_key(key) {return Err("Too many session drawings".into());}
            state.drawings.entry(key.to_owned()).or_insert_with(||Drawing::new(binding));
        }
        let drawing=state.drawings.get_mut(key).ok_or("Drawing store is not registered")?;
        if drawing.retiring {drop(state);request.finish(Ok(StoreResult::Superseded));return Ok(reply);}
        if drawing.running {
            let superseded=drawing.pending.push(request);drop(state);
            for request in superseded {request.finish(Ok(StoreResult::Superseded));}
        }else {let task=self.start(drawing,request);drop(state);(self.0.execute)(task);}
        Ok(reply)
    }
    fn start(&self,drawing:&mut Drawing,request:Request)->StoreWork {
        drawing.sequence=drawing.sequence.wrapping_add(1);drawing.running=true;drawing.retiring=matches!(request.work,Work::Retire);
        drawing.inflight=match &request.work {Work::Checkpoint {stamp,..}=>Some(stamp.clone()),_=>None};
        StoreWork(Some(StoreTask::Drawing(DrawingWork {stores:self.clone(),key:drawing.binding.key().to_owned(),sequence:drawing.sequence,binding:drawing.binding.clone(),request:Some(request)})))
    }
    pub fn watch_pressure(&self,source:layer_ui::StoragePressure) {
        let mut watched=self.0.pressure.lock().unwrap();
        if watched.as_ref().is_some_and(|(current,generation)|current.same_source(&source) && current.is_current(*generation)) {return;}
        let weak=Arc::downgrade(&self.0);
        let generation=source.subscribe(Arc::new(move || {
            if let Some(inner)=weak.upgrade() {
                let stores=SessionStores(inner);let watched=stores.0.pressure.lock().unwrap().clone();
                if let Some((source,generation))=watched && let Some((candidates,budget))=source.request(generation) {
                    stores.trim(candidates,budget).on_finished(Box::new(move |result|source.completed(generation,result)));
                }
            }
        }));
        *watched=Some((source.clone(),generation));drop(watched);
        if let Some((candidates,budget))=source.request(generation) {self.trim(candidates,budget).on_finished(Box::new(move |result|source.completed(generation,result)));}
    }
    fn trim(&self,candidates:Vec<(u64,u64,RetainedTiles)>,budget:usize)->StoreReply {
        let reply=StoreReply::default();let batch=TrimBatch {candidates,budget,replies:vec![reply.clone()]};
        let mut trims=self.0.trims.lock().unwrap();
        if trims.active {
            let mut batch=batch;if let Some(previous)=trims.pending.take() {batch.replies.extend(previous.replies);}
            trims.pending=Some(batch);
        }else {trims.active=true;drop(trims);self.start_trim(batch);}
        reply
    }
    fn start_trim(&self,batch:TrimBatch) {
        let reply=StoreReply::default();let stores=self.clone();
        reply.on_finished(Box::new(move |result| {
            let mut trims=stores.0.trims.lock().unwrap();let next=trims.pending.take();trims.active=next.is_some();drop(trims);
            for reply in batch.replies {reply.finish(result.clone().map(|_|StoreResult::Durable(0)));}
            if let Some(next)=next {stores.start_trim(next);}stores.wake();
        }));
        let trim=PressureTrim {stores:self.clone(),candidates:batch.candidates,budget:batch.budget,next:0,reply};
        (self.0.execute)(StoreWork(Some(StoreTask::Trim(trim))));
    }

}
#[cfg(not(target_arch="wasm32"))]
fn native_now_ms()->u64 {
        static START:std::sync::OnceLock<std::time::Instant>=std::sync::OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed().as_millis().min(u64::MAX as u128) as u64
    }

#[cfg(not(target_arch="wasm32"))]
fn native_worker(work:StoreWork) {
    static WORKER:std::sync::OnceLock<Result<std::sync::mpsc::Sender<StoreWork>,String>>=std::sync::OnceLock::new();
    let worker=WORKER.get_or_init(|| {
        let (send,receive)=std::sync::mpsc::channel::<StoreWork>();let receive=Arc::new(Mutex::new(receive));
        for index in 0..2 {
            let receive=receive.clone();
            std::thread::Builder::new().name(format!("session-store-{index}")).stack_size(8*1024*1024).spawn(move ||loop {
                let work=receive.lock().unwrap().recv();
                let Ok(work)=work else {break;};work.run();
            }).map_err(|error|error.to_string())?;
        }Ok(send)
    });
    if let Ok(worker)=worker {let _=worker.send(work);}
}
fn same_checkpoint(a:&SessionStamp,b:&SessionStamp)->bool {
    let mut current=a.clone();current.epoch=b.epoch;
    if current.camera_navigation==b.camera_navigation {current.state.camera.center=b.state.camera.center;}
    current==*b
}
impl Drawing {
    fn new(binding:DrawingStore)->Self {Self {memory:None,package:None,binding,observed:None,committed:None,committed_at:None,due:None,deferred_since:None,storage_error:None,next_storage_sample:0,next_compaction:0,sequence:0,running:false,inflight:None,retiring:false,pending:Pending::default()}}
    fn release_package_wait(&mut self) {self.deferred_since=None;if let Some(memory)=&self.memory {memory.release_package_wait();}}
    fn current(&self,stamp:&SessionStamp)->bool {
        self.committed.as_ref().is_some_and(|current|same_checkpoint(current,stamp)) && self.inflight.as_ref().is_none_or(|current|same_checkpoint(current,stamp))
            && self.pending.checkpoint.as_ref().is_none_or(|request|matches!(&request.work,Work::Checkpoint {stamp:current,..} if same_checkpoint(current,stamp)))
    }
}
pub struct StoreWork(Option<StoreTask>);
enum StoreTask {Drawing(DrawingWork),Trim(PressureTrim)}
impl StoreWork {
    #[cfg(not(target_arch="wasm32"))]
    pub fn run(self) {pollster::block_on(self.run_async());}
    pub async fn run_async(mut self) {
        match self.0.take().unwrap() {
            StoreTask::Drawing(work)=>work.run().await,
            StoreTask::Trim(trim)=>{let reply=trim.reply.clone();if std::panic::catch_unwind(std::panic::AssertUnwindSafe(||trim.run())).is_err() {reply.finish(Err("Drawing storage worker failed".into()));}}
        }
    }
}
impl Drop for StoreWork {fn drop(&mut self) {if let Some(StoreTask::Trim(trim))=self.0.take() {trim.reply.finish(Err("Drawing storage worker stopped".into()));}}}
struct DrawingWork {stores:SessionStores,key:String,sequence:u64,binding:DrawingStore,request:Option<Request>}
impl DrawingWork {
    pub async fn run(mut self) {
        let mut request=self.request.take().unwrap();
        let current=match &request.work {
            Work::Checkpoint {stamp,forced:false,..}=>self.stores.0.state.lock().unwrap().drawings.get(&self.key)
                .is_some_and(|drawing|drawing.observed.as_ref().is_none_or(|observed|same_checkpoint(observed,stamp))),
            _=>true,
        };
        let blocked={let state=self.stores.0.state.lock().unwrap();state.drawings.get(&self.key).and_then(|drawing|drawing.storage_error.clone())};
        let mut activity=layer_core::package::session_store::CompactionActivity::default();
        activity.set_busy(!matches!(request.work,Work::Package {..}|Work::Compact|Work::SampleStorage));
        let cancelled=AtomicBool::new(false);
        let rejected=blocked.is_some() && matches!(request.work,Work::Checkpoint {..}|Work::Pressure(_)|Work::Import {..}|Work::ImportTiles {..}|Work::Package {..});
        let work=async {
            if rejected {return Err(layer_core::package::storage_budget::STORAGE_UNAVAILABLE.into());}
            if let Work::Checkpoint {capture,prepare,..}=&mut request.work && let Some(prepare)=prepare.take() {prepare(capture)?;}
            let sampling=matches!(request.work,Work::SampleStorage);
            let retirement=matches!(request.work,Work::PrepareRetirement|Work::Retire);let preparing_retirement=matches!(request.work,Work::PrepareRetirement);
            let operation=async |store:&mut layer_core::package::session_store::SessionStore|match &mut request.work {
                Work::Package {inventory,copy,checkpoint}=>{
                    let mut copy=match copy.take() {Some(copy)=>copy,None=>layer_core::package::session_store::PackageCopy::new(inventory).await?};
                    let before=copy.remaining();store.copy_package_step(&mut copy,&cancelled,*checkpoint).await?;
                    let progress=before!=copy.remaining();
                    Ok(StoreResult::PackageCopy {inventory:inventory.clone(),copy:(!copy.complete()).then_some(copy),progress})
                },
                Work::Checkpoint {capture,..}=>capture.checkpoint(store,&cancelled).await.map(StoreResult::Checkpoint),
                Work::Pressure(tiles)=>{
                    let mut bytes=0usize;
                    for tiles in tiles {bytes=bytes.saturating_add(store.make_durable(tiles,&cancelled).await?);}
                    Ok(StoreResult::Durable(bytes))
                },
                Work::Import {expected,metadata,resources}=>match store.import_generation(*expected,metadata,resources.clone(),&cancelled).await {
                    Err(layer_core::package::session_store::ImportFailure::Io(error))=>Err(error),result=>Ok(StoreResult::Imported(result)),
                },
                Work::ImportTiles {tiles,..}=>store.make_durable(tiles,&cancelled).await.map(StoreResult::Durable),
                Work::Compact=>store.compact_step().await.map(|_|StoreResult::Durable(0)),
                Work::SampleStorage=>unreachable!(),
                Work::Restore(limits)=>{
                    let weak=Arc::downgrade(&self.stores.0);let key=self.key.clone();
                    let restored=store.load(*limits,&cancelled).await?;
                    if let Some(restored)=&restored {restored.editor.document().memory().bind_storage(self.binding.clone())?;}
                    let session=restored.map(layer_ui::SessionRestore::from_core).transpose()?.map(|restore|restore.on_adopted(Box::new(move |stamp:SessionStamp| {
                        if let Some(inner)=weak.upgrade() && let Some(drawing)=inner.state.lock().unwrap().drawings.get_mut(&key) {
                            drawing.observed=Some(stamp.clone());drawing.committed=Some(stamp);drawing.committed_at=Some((inner.clock)());drawing.due=None;
                        }
                    })));
                    Ok(StoreResult::Restored {session,recovered_previous:store.recovered_previous()})
                },
                Work::PrepareRetirement=>store.prepare_retirement().await.map(|()|StoreResult::RetirementPrepared),
                Work::Retire=>store.retire().await.map(|()|StoreResult::Retired),
            };
            if sampling {
                let facts=self.binding.storage_facts().await?;
                Ok(StoreResult::StorageReady(blocked.as_ref().map_or_else(||facts.available_bytes.is_some_and(|available|available>0),|refusal|refusal.allows(&facts))))
            }
            else if retirement {
                let missing=if preparing_retirement {StoreResult::RetirementPrepared}else {StoreResult::Retired};
                Ok(self.binding.with_retirement_store(operation).await?.unwrap_or(missing))
            }else {self.binding.with_store(operation).await}
        };
        #[cfg(not(target_arch="wasm32"))]
        let mut result=if !current {drop(work);Ok(StoreResult::Superseded)}else {std::panic::AssertUnwindSafe(work).catch_unwind().await.unwrap_or_else(|_|Err("Drawing storage worker failed".into()))};
        #[cfg(target_arch="wasm32")]
        let mut result=if !current {drop(work);Ok(StoreResult::Superseded)}else {work.await};
        let refusal=if !rejected && !matches!(request.work,Work::SampleStorage|Work::Compact)
            && result.as_ref().is_err_and(|error|error==layer_core::package::storage_budget::STORAGE_UNAVAILABLE) {
            let epoch=layer_core::package::storage_budget::StorageBudget::process().event_epoch();
            Some(StorageRefusal {facts:self.binding.storage_facts().await.ok(),epoch})
        }else {None};
        if let Err(error)=&result && let Work::Package {inventory,..}=&request.work {
            if let Some(drawing)=self.stores.0.state.lock().unwrap().drawings.get_mut(&self.key) {
                drawing.package=if error==layer_core::package::storage_budget::STORAGE_UNAVAILABLE {None}else {
                    drawing.release_package_wait();Some((inventory.clone(),None))
                };
            }
        }
        let continue_package=match &mut result {Ok(StoreResult::PackageCopy {inventory,copy,progress})=>{
            if let Some(drawing)=self.stores.0.state.lock().unwrap().drawings.get_mut(&self.key) {
                if copy.is_none() {drawing.release_package_wait();}
                drawing.package=Some((inventory.clone(),copy.take()));
            }
            *progress
        },_=>false};
        let committed=match (&request.work,&result) {(Work::Checkpoint {stamp,..},Ok(StoreResult::Checkpoint(_)))=>Some(stamp.clone()),_=>None};
        let written=committed.is_some();
        {
            let mut state=self.stores.0.state.lock().unwrap();
            if let Some(drawing)=state.drawings.get_mut(&self.key) {
                match &result {
                    Err(_) if refusal.is_some()=>{
                        drawing.storage_error=refusal;drawing.next_storage_sample=(self.stores.0.clock)().saturating_add(SessionStores::checkpoint_interval_ms());
                    },
                    Ok(StoreResult::StorageReady(true))=>{drawing.storage_error=None;},_=>{},
                }
            }
        }
        let storage_ready=matches!(result,Ok(StoreResult::StorageReady(true)));
        self.complete(committed,matches!(result,Ok(StoreResult::Retired)));
        if continue_package {self.stores.copy_package(&self.key);}
        if matches!(request.work,Work::Package {..}) && let Err(error)=&result {
            let watched=self.stores.0.pressure.lock().unwrap().clone();if let Some((source,generation))=watched && source.is_current(generation) {source.storage_completed(Err(error.clone()));}
        }
        if written || storage_ready {
            let watched=self.stores.0.pressure.lock().unwrap().clone();
            if let Some((source,generation))=watched && source.is_current(generation) && source.error().is_some() {source.storage_completed(Ok(()));}
        }
        let wake=!matches!(result,Ok(StoreResult::StorageReady(false))) && !matches!(request.work,Work::Package {..}|Work::Compact);request.finish(result);if wake {self.stores.wake();}
    }
    fn complete(&self,committed:Option<SessionStamp>,retired:bool) {
        let mut state=self.stores.0.state.lock().unwrap();
        if retired {
            state.aliases.retain(|_,key|key!=&self.key);state.drawings.remove(&self.key);return;
        }
        let next=state.drawings.get_mut(&self.key).and_then(|drawing| {
            if drawing.sequence!=self.sequence {return None;}
            drawing.running=false;drawing.inflight=None;drawing.retiring=false;
            if let Some(stamp)=committed {
                if drawing.observed.as_ref().is_none_or(|observed|same_checkpoint(observed,&stamp)) {drawing.release_package_wait();}
                drawing.committed=Some(stamp);drawing.committed_at=Some((self.stores.0.clock)());if drawing.observed.as_ref().zip(drawing.committed.as_ref()).is_some_and(|(a,b)|same_checkpoint(a,b)) {drawing.due=None;}}
            drawing.pending.take().map(|request|self.stores.start(drawing,request))
        });
        if next.is_none() && !state.aliases.values().any(|key|key==&self.key) {state.drawings.remove(&self.key);}
        drop(state);if let Some(next)=next {(self.stores.0.execute)(next);}
    }
}
impl Drop for DrawingWork {
    fn drop(&mut self) {
        if let Some(request)=self.request.take() {
            self.complete(None,false);request.finish(Err("Drawing storage worker stopped".into()));self.stores.wake();
        }
    }
}

#[derive(Default)]
struct TrimRequests {active:bool,pending:Option<TrimBatch>}
struct TrimBatch {candidates:Vec<(u64,u64,RetainedTiles)>,budget:usize,replies:Vec<StoreReply>}
struct PressureTrim {stores:SessionStores,candidates:Vec<(u64,u64,RetainedTiles)>,budget:usize,next:usize,reply:StoreReply}
impl PressureTrim {
    fn run(mut self) {
        if self.next==0 {self.candidates.sort_by_key(|(_,used,_)|*used);}
        let resident=layer_core::raster_storage::resident_tile_bytes(self.candidates.iter().map(|(_,_,tiles)|tiles));
        if resident<=self.budget {self.reply.finish(Ok(StoreResult::Durable(0)));return;}
        let next=self.next;self.next+=1;
        let Some((id,_,tiles))=self.candidates.get(next) else {self.reply.finish(Err("Inactive drawing bytes remain above their allowance".into()));return;};
        let result=self.stores.pressure(*id,tiles.clone());
        match result {
            Ok(reply)=>reply.on_finished(Box::new(move |result|match result {
                Ok(())=>{let stores=self.stores.clone();(stores.0.execute)(StoreWork(Some(StoreTask::Trim(self))));},
                Err(error)=>self.reply.finish(Err(error)),
            })),
            Err(error)=>self.reply.finish(Err(error)),
        }
    }
}

#[cfg(test)]
mod tests;
