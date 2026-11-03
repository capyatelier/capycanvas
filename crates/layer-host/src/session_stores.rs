use layer_core::{PortableId,ProjectLimits,package::session_store::DrawingStore,raster_storage::RetainedTiles};
use layer_ui::{SessionCapture,SessionStamp};
use std::{collections::{BTreeMap,VecDeque},path::PathBuf,sync::{Arc,Mutex,atomic::AtomicBool},task::{Poll,Waker}};

pub enum StoreResult {Checkpoint(PortableId),Durable(usize),Restored {session:Option<layer_ui::SessionRestore>,recovered_previous:bool},Retired,Superseded}
#[derive(Default)]
struct ReplyState {result:Option<Result<StoreResult,String>>,waker:Option<Waker>,completion:Option<Box<dyn FnOnce(Result<(),String>)+Send>>}
#[derive(Clone,Default)]
pub struct StoreReply(Arc<(Mutex<ReplyState>,std::sync::Condvar)>);
impl StoreReply {
    fn finish(&self,result:Result<StoreResult,String>) {
        let mut state=self.0.0.lock().unwrap();state.result=Some(result);let wake=state.waker.take();let completion=state.completion.take();
        let status=state.result.as_ref().unwrap().as_ref().map(|_|()).map_err(Clone::clone);self.0.1.notify_all();drop(state);
        if let Some(wake)=wake {wake.wake();}if let Some(completion)=completion {completion(status);}
    }
    fn on_finished(&self,completion:impl FnOnce(Result<(),String>)+Send+'static) {
        let mut state=self.0.0.lock().unwrap();
        if let Some(result)=&state.result {let result=result.as_ref().map(|_|()).map_err(Clone::clone);drop(state);completion(result);}
        else {state.completion=Some(Box::new(completion));}
    }
    pub fn try_take(&self)->Option<Result<StoreResult,String>> {self.0.0.lock().unwrap().result.take()}
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
enum Work {Checkpoint {capture:SessionCapture,stamp:SessionStamp,forced:bool,prepare:Option<CapturePreparation>},Pressure(Vec<RetainedTiles>),Restore(ProjectLimits),Retire}
struct Request {work:Work,replies:Vec<StoreReply>}
impl Request {
    fn finish(self,result:Result<StoreResult,String>) {
        let mut result=Some(result);let count=self.replies.len();
        for (index,reply) in self.replies.into_iter().enumerate() {
            let value=if index+1==count {result.take().unwrap()}else {match result.as_ref().unwrap() {
                Ok(StoreResult::Checkpoint(id))=>Ok(StoreResult::Checkpoint(*id)),Ok(StoreResult::Durable(bytes))=>Ok(StoreResult::Durable(*bytes)),
                Ok(StoreResult::Retired)=>Ok(StoreResult::Retired),Ok(StoreResult::Superseded)=>Ok(StoreResult::Superseded),Err(error)=>Err(error.clone()),
                Ok(StoreResult::Restored {..})=>unreachable!(),
            }};reply.finish(value);
        }
    }
}
#[derive(Default)]
struct Pending {checkpoint:Option<Request>,pressure:Option<Request>,restores:VecDeque<Request>,retire:Option<Request>}
impl Pending {
    fn take(&mut self)->Option<Request> {self.retire.take().or_else(||self.restores.pop_front()).or_else(||self.pressure.take()).or_else(||self.checkpoint.take())}
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
            Work::Restore(_)=>self.restores.push_back(request),
            Work::Retire=>{
                if let Some(previous)=self.retire.take() {request.replies.extend(previous.replies);}
                self.retire=Some(request);
                return self.pressure.take().into_iter().chain(self.checkpoint.take()).chain(self.restores.drain(..)).collect();
            },
        }Vec::new()
    }
}
struct Drawing {
    binding:DrawingStore,observed:Option<SessionStamp>,committed:Option<SessionStamp>,committed_at:Option<u64>,due:Option<u64>,
    sequence:u64,running:bool,inflight:Option<SessionStamp>,retiring:bool,pending:Pending,
}
struct State {drawings:BTreeMap<String,Drawing>,aliases:BTreeMap<u64,String>}
struct Inner {
    root:PathBuf,state:Mutex<State>,execute:Arc<dyn Fn(StoreWork)+Send+Sync>,wake:Arc<dyn Fn()+Send+Sync>,pressure:Mutex<Option<(layer_ui::StoragePressure,u64)>>,trims:Mutex<TrimRequests>,
}
#[derive(Clone)]
pub struct SessionStores(Arc<Inner>);
impl SessionStores {
    pub fn native(root:PathBuf,wake:Arc<dyn Fn()+Send+Sync>)->Self {Self::new(root,Arc::new(native_worker),wake)}
    pub fn new(root:PathBuf,execute:Arc<dyn Fn(StoreWork)+Send+Sync>,wake:Arc<dyn Fn()+Send+Sync>)->Self {
        Self(Arc::new(Inner {root,state:Mutex::new(State {drawings:BTreeMap::new(),aliases:BTreeMap::new()}),execute,wake,pressure:Mutex::new(None),trims:Mutex::new(TrimRequests::default())}))
    }
    pub fn bind(&self,id:u64,key:Option<&str>)->Result<DrawingStore,String> {
        let mut state=self.0.state.lock().unwrap();
        if let Some(current)=state.aliases.get(&id) {
            if key.is_some_and(|key|key!=current) {return Err("Drawing storage identity changed".into());}
            return Ok(state.drawings[current].binding.clone());
        }
        let binding=DrawingStore::new(&self.0.root,key)?;
        let key=binding.key().to_owned();
        if state.drawings.len()>=layer_ui::session_recovery::MAX_SESSION_DRAWINGS && !state.drawings.contains_key(&key) {return Err("Too many session drawings".into());}
        state.drawings.entry(key.clone()).or_insert_with(||Drawing {binding,observed:None,committed:None,committed_at:None,due:None,sequence:0,running:false,inflight:None,retiring:false,pending:Pending::default()});
        state.aliases.insert(id,key.clone());Ok(state.drawings[&key].binding.clone())
    }
    pub fn remap(&self,mapping:&[(u64,u64)]) {
        let mut state=self.0.state.lock().unwrap();
        let mapped=mapping.iter().filter_map(|&(old,new)|state.aliases.remove(&old).map(|key|(new,key))).collect::<Vec<_>>();
        state.aliases.extend(mapped);
    }
    pub fn adopt(&self,id:u64,key:&str)->Result<DrawingStore,String> {
        let binding=DrawingStore::new(&self.0.root,Some(key))?;
        self.adopt_binding(id,binding)?;
        self.bind(id,Some(key))
    }
    pub fn observe(&self,id:u64,stamp:SessionStamp,now_ms:u64,immediate:bool)->Result<bool,String> {
        self.bind(id,None)?;
        let mut state=self.0.state.lock().unwrap();let key=state.aliases[&id].clone();let drawing=state.drawings.get_mut(&key).unwrap();
        drawing.observed=Some(stamp);
        if drawing.observed==drawing.committed {drawing.due=None;return Ok(false);}
        if immediate {drawing.due=Some(now_ms);}
        let deadline=drawing.committed_at.unwrap_or(now_ms).saturating_add(Self::checkpoint_interval_ms());
        let due=*drawing.due.get_or_insert(deadline);
        Ok(!drawing.running && now_ms>=due)
    }
    pub const fn checkpoint_interval_ms()->u64 {layer_ui::session_recovery::CHECKPOINT_INTERVAL_MS}
    fn now_ms()->u64 {
        static START:std::sync::OnceLock<std::time::Instant>=std::sync::OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed().as_millis().min(u64::MAX as u128) as u64
    }
    pub fn checkpoint_due(&self,id:u64,stamp:SessionStamp,force:bool)->Result<bool,String> {
        self.bind(id,None)?;
        let (current,first)={let state=self.0.state.lock().unwrap();let drawing=&state.drawings[&state.aliases[&id]];(drawing.current(&stamp),drawing.committed.is_none())};
        if current {self.observe(id,stamp,Self::now_ms(),false)?;return Ok(false);}
        let urgent=force || first;
        let ready=self.observe(id,stamp,Self::now_ms(),urgent)?;
        Ok(urgent || ready)
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
        state.drawings.entry(key.clone()).or_insert_with(||Drawing {binding,observed:None,committed:None,committed_at:None,due:None,sequence:0,running:false,inflight:None,retiring:false,pending:Pending::default()});
        state.aliases.retain(|&current,current_key|current==id || current_key!=&key);
        let previous=state.aliases.insert(id,key);
        if let Some(previous)=previous && !state.aliases.values().any(|key|key==&previous) && state.drawings.get(&previous).is_some_and(|drawing|!drawing.running) {state.drawings.remove(&previous);}
        Ok(())
    }
    pub fn import(&self,id:u64,capture:SessionCapture,stamp:SessionStamp)->Result<StoreReply,String> {self.checkpoint(id,capture,stamp,true)}
    pub fn pressure(&self,id:u64,tiles:RetainedTiles)->Result<StoreReply,String> {self.request(id,Work::Pressure(vec![tiles]))}
    pub fn restore(&self,id:u64,limits:ProjectLimits)->Result<StoreReply,String> {self.request(id,Work::Restore(limits))}
    pub fn retire(&self,id:u64)->Result<StoreReply,String> {self.request(id,Work::Retire)}
    pub fn bindings(&self)->Vec<(u64,DrawingStore)> {
        {let state=self.0.state.lock().unwrap();state.aliases.iter().map(|(&id,key)|(id,state.drawings[key].binding.clone())).collect()}
    }
    fn request(&self,id:u64,work:Work)->Result<StoreReply,String> {
        let binding=self.bind(id,None)?;self.request_key(binding.key(),work)
    }
    fn request_key(&self,key:&str,work:Work)->Result<StoreReply,String> {
        let reply=StoreReply::default();let request=Request {work,replies:vec![reply.clone()]};
        let mut state=self.0.state.lock().unwrap();let drawing=state.drawings.get_mut(key).ok_or("Drawing store is not registered")?;
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
                if let Some((source,generation))=watched && let Some((candidates,budget))=source.snapshot(generation) {
                    stores.trim(candidates,budget).on_finished(move |_|source.completed(generation));
                }
            }
        }));
        *watched=Some((source.clone(),generation));drop(watched);
        if let Some((candidates,budget))=source.snapshot(generation) {self.trim(candidates,budget).on_finished(move |_|source.completed(generation));}
    }
    pub fn trim(&self,candidates:Vec<(u64,u64,RetainedTiles)>,budget:usize)->StoreReply {
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
        reply.on_finished(move |result| {
            let mut trims=stores.0.trims.lock().unwrap();let next=trims.pending.take();trims.active=next.is_some();drop(trims);
            for reply in batch.replies {reply.finish(result.clone().map(|_|StoreResult::Durable(0)));}
            if let Some(next)=next {stores.start_trim(next);}(stores.0.wake)();
        });
        let trim=PressureTrim {stores:self.clone(),candidates:batch.candidates,budget:batch.budget,next:0,reply};
        (self.0.execute)(StoreWork(Some(StoreTask::Trim(trim))));
    }

}
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
impl Drawing {
    fn current(&self,stamp:&SessionStamp)->bool {
        self.committed.as_ref()==Some(stamp) && self.inflight.as_ref().is_none_or(|current|current==stamp)
            && self.pending.checkpoint.as_ref().is_none_or(|request|matches!(&request.work,Work::Checkpoint {stamp:current,..} if current==stamp))
    }
}
pub struct StoreWork(Option<StoreTask>);
enum StoreTask {Drawing(DrawingWork),Trim(PressureTrim)}
impl StoreWork {pub fn run(mut self) {match self.0.take().unwrap() {StoreTask::Drawing(work)=>work.run(),StoreTask::Trim(trim)=>{let reply=trim.reply.clone();if std::panic::catch_unwind(std::panic::AssertUnwindSafe(||trim.run())).is_err() {reply.finish(Err("Drawing storage worker failed".into()));}}}}}
impl Drop for StoreWork {fn drop(&mut self) {if let Some(StoreTask::Trim(trim))=self.0.take() {trim.reply.finish(Err("Drawing storage worker stopped".into()));}}}
struct DrawingWork {stores:SessionStores,key:String,sequence:u64,binding:DrawingStore,request:Option<Request>}
impl DrawingWork {
    pub fn run(mut self) {
        let mut request=self.request.take().unwrap();
        let current=match &request.work {
            Work::Checkpoint {stamp,forced:false,..}=>self.stores.0.state.lock().unwrap().drawings.get(&self.key)
                .is_some_and(|drawing|drawing.observed.as_ref().is_none_or(|observed|observed==stamp)),
            _=>true,
        };
        let cancelled=AtomicBool::new(false);
        let result=if !current {Ok(StoreResult::Superseded)}else {std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Work::Checkpoint {capture,prepare,..}=&mut request.work && let Some(prepare)=prepare.take() {prepare(capture)?;}
            self.binding.on_worker(|store|match &request.work {
            Work::Checkpoint {capture,..}=>capture.checkpoint(store,&cancelled).map(StoreResult::Checkpoint),
            Work::Pressure(tiles)=>tiles.iter().try_fold(0usize,|bytes,tiles|store.make_durable(tiles,&cancelled).map(|released|bytes.saturating_add(released))).map(StoreResult::Durable),
            Work::Restore(limits)=>{
                let weak=Arc::downgrade(&self.stores.0);let key=self.key.clone();
                let session=store.load(*limits,&cancelled)?.map(layer_ui::SessionRestore::from_core).transpose()?.map(|restore|restore.on_adopted(move |stamp| {
                    if let Some(inner)=weak.upgrade() && let Some(drawing)=inner.state.lock().unwrap().drawings.get_mut(&key) {
                        drawing.observed=Some(stamp.clone());drawing.committed=Some(stamp);drawing.committed_at=Some(SessionStores::now_ms());drawing.due=None;
                    }
                }));
                Ok(StoreResult::Restored {session,recovered_previous:store.recovered_previous()})
            },
            Work::Retire=>store.retire().map(|()|StoreResult::Retired),
        })
        })).unwrap_or_else(|_|Err("Drawing storage worker failed".into()))};
        let committed=match (&request.work,&result) {(Work::Checkpoint {stamp,..},Ok(StoreResult::Checkpoint(_)))=>Some(stamp.clone()),_=>None};
        self.complete(committed,matches!(result,Ok(StoreResult::Retired)));request.finish(result);(self.stores.0.wake)();
    }
    fn complete(&self,committed:Option<SessionStamp>,retired:bool) {
        let mut state=self.stores.0.state.lock().unwrap();
        if retired {
            state.aliases.retain(|_,key|key!=&self.key);state.drawings.remove(&self.key);return;
        }
        let next=state.drawings.get_mut(&self.key).and_then(|drawing| {
            if drawing.sequence!=self.sequence {return None;}
            drawing.running=false;drawing.inflight=None;drawing.retiring=false;
            if let Some(stamp)=committed {drawing.committed=Some(stamp);drawing.committed_at=Some(SessionStores::now_ms());if drawing.observed.as_ref()==drawing.committed.as_ref() {drawing.due=None;}}
            drawing.pending.take().map(|request|self.stores.start(drawing,request))
        });
        if next.is_none() && !state.aliases.values().any(|key|key==&self.key) {state.drawings.remove(&self.key);}
        drop(state);if let Some(next)=next {(self.stores.0.execute)(next);}
    }
}
impl Drop for DrawingWork {
    fn drop(&mut self) {
        if let Some(request)=self.request.take() {
            self.complete(None,false);request.finish(Err("Drawing storage worker stopped".into()));(self.stores.0.wake)();
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
            Ok(reply)=>reply.on_finished(move |result|match result {
                Ok(())=>{let stores=self.stores.clone();(stores.0.execute)(StoreWork(Some(StoreTask::Trim(self))));},
                Err(error)=>self.reply.finish(Err(error)),
            }),
            Err(error)=>self.reply.finish(Err(error)),
        }
    }
}

#[cfg(test)]
mod tests;
