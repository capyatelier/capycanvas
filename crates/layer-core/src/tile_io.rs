use crate::{MemoryBudget, MemoryClass, MemoryOwner, MemoryOwnerKind, MemoryReport};
use crate::package::{ByteRange, RangeState, MAX_RANGE_BYTES};
use std::{collections::HashMap, sync::{Arc, Mutex, OnceLock, Weak, atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering}}, task::{Context, Poll, Waker}};

pub const HOT_SET_BYTES: u64 = 256 * 1024 * 1024;
const QUEUED_READS: usize = 18;
const ACTIVE_READS: usize = 2;

pub fn read_buffer(length:usize)->Arc<[u8]> {
    unsafe {Arc::<[u8]>::new_zeroed_slice(length).assume_init()}
}

#[cfg(not(target_arch="wasm32"))]
pub(crate) fn read_exact_at(file:&std::fs::File, mut offset:u64, mut bytes:&mut [u8])->Result<(),String> {
    while !bytes.is_empty() {
        #[cfg(unix)]
        let count={use std::os::unix::fs::FileExt;file.read_at(bytes,offset)};
        #[cfg(windows)]
        let count={use std::os::windows::fs::FileExt;file.seek_read(bytes,offset)};
        let count=match count {Err(error) if error.kind()==std::io::ErrorKind::Interrupted=>continue,other=>other.map_err(|error|error.to_string())?};
        if count==0 {return Err("Incomplete tile range".into());}
        offset=offset.checked_add(count as u64).ok_or("Tile read offset overflow")?;bytes=&mut bytes[count..];
    }
    Ok(())
}

#[cfg(not(target_arch="wasm32"))]
struct FileReader(Arc<std::fs::File>);
#[cfg(not(target_arch="wasm32"))]
impl RangeReader for FileReader {
    fn read(&self,offset:u64,length:usize,crc:Option<u32>,completion:ReadCompletion) {
        let mut bytes=read_buffer(length);
        let result=read_exact_at(&self.0,offset,Arc::get_mut(&mut bytes).unwrap()).and_then(|_|verify_crc(&bytes,crc)).map(|_|bytes);
        completion.complete(result);
    }
}

pub fn verify_crc(bytes:&[u8],expected:Option<u32>)->Result<(),String> {
    if expected.is_some_and(|expected|crc32fast::hash(bytes)!=expected) {return Err("Recovery resource checksum failed".into());}
    Ok(())
}

pub trait RangeReader: Send + Sync {
    fn read(&self, offset: u64, length: usize, crc:Option<u32>, completion: ReadCompletion);
}

#[derive(Clone)]
pub struct ReadCache(Arc<Cache>);
struct Cache {
    owner: MemoryOwner,
    state: Mutex<State>,
    #[cfg(not(target_arch = "wasm32"))]
    changed: std::sync::Condvar,
    scheduler: OnceLock<Arc<dyn Fn(ReadCache) + Send + Sync>>,
    scheduled: AtomicBool,
    pending_reads: AtomicUsize,
    physical: AtomicU64,
    expired: AtomicBool,
}
#[derive(Default)]
struct State {
    entries: HashMap<Key, Entry>,
    clock: u64,
    physical: u64,
    pinned: u64,
    running: usize,
    queued: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Key(u64, u64, usize);
struct Entry {
    source: Weak<Source>,
    phase: Phase,
    used: u64,
    leases: usize,
    waiters: Vec<(u64, Option<Waker>)>,
    crc:Option<u32>,
}
enum Phase { Cold, Queued, Reading, Ready(Arc<[u8]>), Failed(String) }
struct Source { id: u64, length: AtomicU64, reader: Arc<dyn RangeReader>, cache: ReadCache }
impl Drop for Source { fn drop(&mut self) { self.cache.0.expired.store(true,Ordering::Release);self.cache.schedule(); } }
#[derive(Clone)]
pub struct ReadSource(Arc<Source>);
#[derive(Default)]
pub(crate) struct WeakReadSource(Weak<Source>);
impl WeakReadSource {pub(crate) fn upgrade(&self)->Option<ReadSource> {self.0.upgrade().map(ReadSource)}}

pub(crate) struct Lease { source: Arc<Source>, key: Key }
impl std::fmt::Debug for Lease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_tuple("ReadLease").field(&self.key).finish() }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let cache=&self.source.cache;
        let mut state=cache.0.state.lock().unwrap();
        if let Some(entry)=state.entries.get_mut(&self.key) {
            entry.leases-=1;
            if entry.leases==0 && entry.waiters.is_empty() { state.pinned-=self.key.2 as u64; }
        }
        let trim=state.physical>cache.limit_bytes();cache.publish(&state);drop(state);if trim {cache.schedule();}
    }
}

pub struct ReadCompletion { cache: ReadCache, key: Key, done: bool }
impl ReadCompletion {
    pub fn complete(mut self, result: Result<Arc<[u8]>, String>) {
        self.done=true;self.cache.complete(self.key,result);
    }
}
impl Drop for ReadCompletion {
    fn drop(&mut self) { if !self.done { self.cache.complete(self.key,Err("Tile read was abandoned".into())); } }
}

impl ReadCache {
    pub fn new(budget: MemoryBudget) -> Self {
        let owner=budget.register_owner(MemoryOwnerKind::Other,MemoryClass::Warm,None);
        owner.report(MemoryReport {demand_ram_bytes:HOT_SET_BYTES,..Default::default()});
        let cache=Self(Arc::new(Cache {owner,state:Default::default(),scheduler:Default::default(),scheduled:AtomicBool::new(false),pending_reads:AtomicUsize::new(0),physical:AtomicU64::new(0),expired:AtomicBool::new(false),
            #[cfg(not(target_arch="wasm32"))] changed:Default::default()}));
        let weak=Arc::downgrade(&cache.0);
        cache.0.owner.set_wake(Arc::new(move || {if let Some(cache)=weak.upgrade() {let cache=ReadCache(cache);if cache.has_pending_reads() || cache.0.physical.load(Ordering::Acquire)>cache.limit_bytes() {cache.schedule();}}}));
        cache
    }
    pub fn process() -> Self {
        static CACHE: OnceLock<ReadCache> = OnceLock::new();
        CACHE.get_or_init(||Self::new(MemoryBudget::process())).clone()
    }
    pub fn source(&self, length: u64, reader: Arc<dyn RangeReader>) -> ReadSource {
        static NEXT: AtomicU64=AtomicU64::new(1);
        ReadSource(Arc::new(Source {id:NEXT.fetch_add(1,Ordering::Relaxed),length:AtomicU64::new(length),reader,cache:self.clone()}))
    }
    pub fn set_scheduler(&self, scheduler: Arc<dyn Fn(ReadCache) + Send + Sync>) {let _=self.0.scheduler.set(scheduler);self.0.scheduled.store(false,Ordering::Release);self.schedule();}
    pub fn has_pending_reads(&self)->bool {self.0.pending_reads.load(Ordering::Acquire)!=0}
    pub fn limit_bytes(&self) -> u64 {self.0.owner.grant().ram_bytes.min(HOT_SET_BYTES)}
    fn report(state: &State) -> MemoryReport {
        MemoryReport {ram_bytes:state.physical,pinned_ram_bytes:state.pinned,demand_ram_bytes:HOT_SET_BYTES,..Default::default()}
    }
    fn publish(&self, state: &State) {self.0.pending_reads.store(state.queued+state.running,Ordering::Release);self.0.physical.store(state.physical,Ordering::Release);self.0.owner.publish(Self::report(state));}
    fn schedule(&self) {
        if self.0.scheduled.swap(true,Ordering::AcqRel) {return;}
        if let Some(schedule)=self.0.scheduler.get() {schedule(self.clone());return;}
        #[cfg(not(target_arch="wasm32"))]
        workers::schedule(self.clone());
    }
    fn trim(&self,state:&mut State,limit:u64,expired:bool) {
        if expired {
            let dead=state.entries.iter().filter(|(_,entry)|entry.source.strong_count()==0 && entry.leases==0 && entry.waiters.is_empty() && !matches!(entry.phase,Phase::Reading))
                .map(|(&key,_)|key).collect::<Vec<_>>();
            for key in dead {match state.entries.remove(&key).unwrap().phase {
                Phase::Ready(_)=>state.physical-=key.2 as u64,Phase::Queued=>state.queued-=1,_=>{},
            }}
        }
        if state.physical<=limit {return;}
        let mut candidates=state.entries.iter().filter(|(_,entry)|entry.leases==0 && entry.waiters.is_empty() && matches!(entry.phase,Phase::Ready(_)))
            .map(|(&key,entry)|(entry.used,key)).collect::<Vec<_>>();
        candidates.sort_unstable_by_key(|(used,_)|*used);
        for (_,key) in candidates {
            if state.physical<=limit {break;}
            state.entries.remove(&key);state.physical-=key.2 as u64;
        }
    }
    fn ready(&self, state: &mut State, key: Key, source: &Arc<Source>, crc:Option<u32>) -> Result<RangeState,String> {
        state.clock=state.clock.saturating_add(1);
        let queued=state.queued;
        let entry=state.entries.entry(key).or_insert_with(||Entry {source:Arc::downgrade(source),phase:Phase::Cold,used:state.clock,leases:0,waiters:Vec::new(),crc});
        if crc.is_some() && entry.crc!=crc {return Err("Conflicting immutable range checksum".into());}
        match &entry.phase {
            Phase::Ready(bytes)=>{
                entry.used=state.clock;
                if entry.leases==0 && entry.waiters.is_empty() {state.pinned+=key.2 as u64;}
                entry.leases+=1;
                Ok(RangeState::Ready(ByteRange::leased(bytes.clone(),Arc::new(Lease {source:source.clone(),key}))))
            },
            Phase::Failed(error)=>Err(error.clone()),
            Phase::Cold=>{if queued<QUEUED_READS {entry.phase=Phase::Queued;state.queued+=1;} Ok(RangeState::Pending)},
            _=>Ok(RangeState::Pending),
        }
    }
    fn complete(&self, key: Key, result: Result<Arc<[u8]>,String>) {
        let mut state=self.0.state.lock().unwrap();
        let result=result.and_then(|bytes|if bytes.len()==key.2 {Ok(bytes)}else{Err("Incomplete tile read".into())});
        let entry=state.entries.get_mut(&key).unwrap();
        let pinned=entry.leases>0 || !entry.waiters.is_empty();
        let wakers=entry.waiters.iter().filter_map(|(_,wake)|wake.clone()).collect::<Vec<_>>();
        let success=result.is_ok();
        entry.phase=match result {Ok(bytes)=>Phase::Ready(bytes),Err(error)=>Phase::Failed(error)};
        state.running-=1;
        if !success {state.physical-=key.2 as u64;}
        if !success || !pinned {state.pinned-=key.2 as u64;}
        self.publish(&state);
        #[cfg(not(target_arch="wasm32"))] self.0.changed.notify_all();
        let drive=state.queued>0 || state.physical>self.limit_bytes();
        drop(state);
        for wake in wakers {wake.wake();}
        if drive {self.schedule();}
    }
    fn drive_inner(&self) {
        self.0.scheduled.store(false,Ordering::Release);
        let task={
            let mut state=self.0.state.lock().unwrap();
            let expired=self.0.expired.swap(false,Ordering::AcqRel);
            if expired || state.physical>self.limit_bytes() {self.trim(&mut state,self.limit_bytes(),expired);}
            self.publish(&state);
            let queued=state.queued;
            let mut available=QUEUED_READS.saturating_sub(queued);
            for entry in state.entries.values_mut().filter(|entry|matches!(entry.phase,Phase::Cold) && !entry.waiters.is_empty()) {
                if available==0 {break;}entry.phase=Phase::Queued;available-=1;
            }
            state.queued+=QUEUED_READS.saturating_sub(queued)-available;
            self.publish(&state);
            if state.running>=ACTIVE_READS {return;}
            let mut pending=state.entries.iter().filter(|(_,entry)|matches!(entry.phase,Phase::Queued))
                .map(|(&key,entry)|((entry.waiters.is_empty(),entry.used),key)).collect::<Vec<_>>();
            pending.sort_unstable_by_key(|(priority,_)|*priority);
            let mut task=None;
            for (_,key) in pending {
                let Some(source)=state.entries[&key].source.upgrade() else {state.entries.remove(&key);state.queued-=1;continue;};
                state.entries.get_mut(&key).unwrap().phase=Phase::Reading;
                state.physical+=key.2 as u64;state.pinned+=key.2 as u64;state.running+=1;state.queued-=1;
                self.0.pending_reads.store(state.queued+state.running,Ordering::Release);
                task=Some((source,key,state.entries[&key].crc));break;
            }
            self.publish(&state);
            task
        };
        if let Some((source,key,crc))=task {
            if self.has_pending_reads() && self.0.state.lock().unwrap().queued>0 {self.schedule();}
            source.reader.read(key.1,key.2,crc,ReadCompletion {cache:self.clone(),key,done:false});
        }
    }
    pub fn drive(&self) {self.drive_inner();}
}

impl ReadSource {
    pub(crate) fn cached(&self,offset:u64,length:u64)->bool {
        let Ok(length)=usize::try_from(length) else {return false;};
        self.0.cache.0.state.lock().unwrap().entries.get(&Key(self.0.id,offset,length)).is_some_and(|entry|matches!(entry.phase,Phase::Ready(_)))
    }

    #[cfg(not(target_arch="wasm32"))]
    pub fn file(file:Arc<std::fs::File>, length:u64)->Self {ReadCache::process().source(length,Arc::new(FileReader(file)))}
    fn key(&self, offset:u64, length:usize)->Result<Key,String> {
        if length>MAX_RANGE_BYTES || offset.checked_add(length as u64).is_none_or(|end|end>self.0.length.load(Ordering::Acquire)) {return Err("Tile read exceeds backing".into());}
        Ok(Key(self.0.id,offset,length))
    }
    pub(crate) fn extend_to(&self,length:u64) {self.0.length.fetch_max(length,Ordering::Release);}
    pub(crate) fn byte_len(&self)->u64 {self.0.length.load(Ordering::Acquire)}
    pub(crate) fn downgrade(&self)->WeakReadSource {WeakReadSource(Arc::downgrade(&self.0))}
    pub fn poll(&self,offset:u64,length:usize)->Result<RangeState,String> {self.poll_range(offset,length,None)}
    pub fn poll_checked(&self,offset:u64,length:usize,crc:u32)->Result<RangeState,String> {self.poll_range(offset,length,Some(crc))}
    fn poll_range(&self,offset:u64,length:usize,crc:Option<u32>)->Result<RangeState,String> {
        if length==0 {self.key(offset,length)?;return Ok(RangeState::Ready(ByteRange::new(Arc::from([]),0..0)?));}
        let key=self.key(offset,length)?;
        let mut state=self.0.cache.0.state.lock().unwrap();let queued=state.queued;
        let result=self.0.cache.ready(&mut state,key,&self.0,crc);
        let changed=state.queued>queued;
        self.0.cache.publish(&state);drop(state);if changed {self.0.cache.schedule();}result
    }
    pub fn evict(&self) {
        let mut state=self.0.cache.0.state.lock().unwrap();
        let keys=state.entries.iter().filter(|(key,entry)|key.0==self.0.id && entry.leases==0 && entry.waiters.is_empty() && matches!(entry.phase,Phase::Ready(_)))
            .map(|(&key,_)|key).collect::<Vec<_>>();
        for key in keys {state.entries.remove(&key);state.physical-=key.2 as u64;}
        self.0.cache.publish(&state);
    }
    pub fn wait(&self,offset:u64,length:usize)->Result<ReadWait,String> {self.wait_range(offset,length,None)}
    pub(crate) fn wait_checked(&self,offset:u64,length:usize,crc:u32)->Result<ReadWait,String> {self.wait_range(offset,length,Some(crc))}
    fn wait_range(&self,offset:u64,length:usize,crc:Option<u32>)->Result<ReadWait,String> {
        let key=self.key(offset,length)?;
        static NEXT:AtomicU64=AtomicU64::new(1);
        let id=NEXT.fetch_add(1,Ordering::Relaxed);
        let mut state=self.0.cache.0.state.lock().unwrap();
        let existed=state.entries.get(&key).is_some_and(|entry|entry.leases>0 || !entry.waiters.is_empty());
        let ready=state.entries.get(&key).is_some_and(|entry|matches!(entry.phase,Phase::Ready(_)));
        state.clock+=1;let used=state.clock;
        state.entries.entry(key).or_insert_with(||Entry {source:Arc::downgrade(&self.0),phase:Phase::Cold,used,leases:0,waiters:Vec::new(),crc}).waiters.push((id,None));
        if ready && !existed {state.pinned+=length as u64;}
        self.0.cache.publish(&state);drop(state);
        Ok(ReadWait {source:self.clone(),key,id,crc})
    }
    #[cfg(not(target_arch="wasm32"))]
    pub fn wait_on_worker(&self,offset:u64,length:usize)->Result<ByteRange,String> {self.read_range_on_worker(offset,length,None)}
    #[cfg(not(target_arch="wasm32"))]
    pub fn read_checked_on_worker(&self,offset:u64,length:usize,crc:u32)->Result<ByteRange,String> {self.read_range_on_worker(offset,length,Some(crc))}
    #[cfg(not(target_arch="wasm32"))]
    fn read_range_on_worker(&self,offset:u64,length:usize,crc:Option<u32>)->Result<ByteRange,String> {
        let wait=self.wait_range(offset,length,crc)?;
        let mut state=self.0.cache.0.state.lock().unwrap();
        loop {
            match self.0.cache.ready(&mut state,wait.key,&self.0,crc)? {
                RangeState::Ready(bytes)=>return Ok(bytes),
                RangeState::Pending=>{
                    self.0.cache.publish(&state);self.0.cache.schedule();
                    state=self.0.cache.0.changed.wait(state).map_err(|_|"Tile read wait failed")?;
                },
            }
        }
    }
}

impl crate::package::ByteSource for ReadSource {
    fn byte_len(&self)->u64 {self.byte_len()}
    fn resident_bytes(&self)->usize {0}
    fn poll(&self,offset:u64,length:usize)->Result<RangeState,String> {self.poll(offset,length)}
    #[cfg(not(target_arch="wasm32"))]
    fn poll_on_worker(&self,offset:u64,length:usize)->Result<RangeState,String> {self.wait_on_worker(offset,length).map(RangeState::Ready)}
    fn wait(&self,offset:u64,length:usize)->crate::package::store_io::FileFuture<'_,ByteRange> {
        Box::pin(async move {ReadSource::wait(self,offset,length)?.await})
    }
}

pub struct ReadWait { source: ReadSource, key: Key, id:u64, crc:Option<u32> }
impl std::future::Future for ReadWait {
    type Output=Result<ByteRange,String>;
    fn poll(self:std::pin::Pin<&mut Self>,context:&mut Context<'_>)->Poll<Self::Output> {
        let cache=&self.source.0.cache;
        let mut state=cache.0.state.lock().unwrap();
        match cache.ready(&mut state,self.key,&self.source.0,self.crc) {
            Ok(RangeState::Ready(bytes))=>Poll::Ready(Ok(bytes)),
            Err(error)=>Poll::Ready(Err(error)),
            Ok(RangeState::Pending)=>{
                let wake=&mut state.entries.get_mut(&self.key).unwrap().waiters.iter_mut().find(|(id,_)|*id==self.id).unwrap().1;
                if wake.as_ref().is_none_or(|wake|!wake.will_wake(context.waker())) {*wake=Some(context.waker().clone());}
                cache.publish(&state);drop(state);cache.schedule();Poll::Pending
            },
        }
    }
}
impl Drop for ReadWait {
    fn drop(&mut self) {
        let cache=&self.source.0.cache;
        let mut state=cache.0.state.lock().unwrap();
        if let Some(entry)=state.entries.get_mut(&self.key) {
            entry.waiters.retain(|(id,_)|*id!=self.id);
            if entry.leases==0 && entry.waiters.is_empty() && matches!(entry.phase,Phase::Ready(_)) {state.pinned-=self.key.2 as u64;}
        }
        let trim=state.physical>cache.limit_bytes();cache.publish(&state);drop(state);if trim {cache.schedule();}
    }
}

#[cfg(not(target_arch="wasm32"))]
mod workers {
    use super::*;
    struct Pool {queue:Arc<Mutex<std::collections::VecDeque<ReadCache>>>, threads:Vec<std::thread::Thread>}
    fn pool()->&'static Arc<Pool> {
        static POOL:OnceLock<Arc<Pool>>=OnceLock::new();
        POOL.get_or_init(||{
            let queue=Arc::new(Mutex::new(std::collections::VecDeque::<ReadCache>::new()));
            let mut threads=Vec::new();
            for index in 0..ACTIVE_READS {
                let queue=queue.clone();
                let thread=std::thread::Builder::new().name(format!("tile-read-{index}")).spawn(move ||loop {
                    let cache={queue.lock().unwrap().pop_front()};
                    if let Some(cache)=cache {let _=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||cache.drive_inner()));}
                    else {std::thread::park();}
                }).expect("tile read worker");
                threads.push(thread.thread().clone());
            }
            Arc::new(Pool {queue,threads})
        })
    }
    pub(super) fn schedule(cache:ReadCache) {
        let pool=pool();pool.queue.lock().unwrap().push_back(cache);
        for thread in &pool.threads {thread.unpark();}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::VecDeque, sync::atomic::AtomicUsize};

    #[derive(Default)]
    struct Controlled {requests:Mutex<VecDeque<(u64,usize,ReadCompletion)>>,calls:AtomicUsize}
    impl RangeReader for Controlled {
        fn read(&self,offset:u64,length:usize,_crc:Option<u32>,completion:ReadCompletion) {
            self.calls.fetch_add(1,Ordering::Relaxed);
            self.requests.lock().unwrap().push_back((offset,length,completion));
        }
    }
    impl Controlled {
        fn finish(&self,value:u8) {
            let (_,length,done)=self.requests.lock().unwrap().pop_front().unwrap();
            done.complete(Ok(vec![value;length].into()));
        }
    }
    fn setup()->(MemoryBudget,ReadCache,Arc<Controlled>,ReadSource) {
        let budget=MemoryBudget::with_facts(crate::MemoryFacts {total_bytes:Some(1<<40),available_bytes:Some(1<<40),override_bytes:Some(1<<40),..Default::default()});
        let cache=ReadCache::new(budget.clone());cache.set_scheduler(Arc::new(|_|{}));
        let reader=Arc::new(Controlled::default());let source=cache.source(2*HOT_SET_BYTES,reader.clone());
        (budget,cache,reader,source)
    }
    #[test]
    fn a_terabyte_budget_keeps_the_hot_set_demand_at_256_mib() {
        let (budget,cache,reader,source)=setup();
        assert_eq!(cache.limit_bytes(),HOT_SET_BYTES);
        for index in 0..32 {
            let offset=index*1024;
            assert!(matches!(source.poll(offset,1024).unwrap(),RangeState::Pending));
            cache.drive_inner();reader.finish(index as u8);
            let RangeState::Ready(bytes)=source.poll(offset,1024).unwrap() else {panic!("completed range")};
            assert!(bytes.iter().all(|byte|*byte==index as u8));drop(bytes);
            budget.flush_reports();
            assert_eq!(cache.limit_bytes(),HOT_SET_BYTES);
            assert_eq!(cache.0.owner.reported().demand_ram_bytes,HOT_SET_BYTES);
        }
        assert_eq!(cache.0.owner.reported().pinned_ram_bytes,0);
        assert_eq!(reader.calls.load(Ordering::Relaxed),32);
    }
    #[test]
    fn polling_deduplicates_without_reading_and_leases_survive_eviction() {
        let (budget,cache,reader,source)=setup();
        assert!(!cache.has_pending_reads());
        for _ in 0..10 {assert!(matches!(source.poll(7,128).unwrap(),RangeState::Pending));}
        assert_eq!(reader.calls.load(Ordering::Relaxed),0);
        assert!(cache.has_pending_reads());
        cache.drive_inner();assert_eq!(reader.calls.load(Ordering::Relaxed),1);
        reader.finish(71);assert!(!cache.has_pending_reads());
        let RangeState::Ready(bytes)=source.poll(7,128).unwrap() else {panic!("completed range")};
        let slice=bytes.slice(11..31).unwrap();drop(bytes);source.evict();budget.flush_reports();
        assert_eq!(cache.0.owner.reported().pinned_ram_bytes,128);
        assert!(slice.iter().all(|byte|*byte==71));
        drop(source);cache.drive_inner();assert_eq!(cache.0.state.lock().unwrap().physical,128);
        drop(slice);cache.drive_inner();budget.flush_reports();
        assert_eq!(cache.0.owner.reported().ram_bytes,0);
        assert_eq!(cache.0.owner.reported().pinned_ram_bytes,0);
    }
    #[test]
    fn polling_a_queued_read_keeps_its_admission_order() {
        let (_budget,cache,reader,source)=setup();
        let waits=(0..4).map(|index|source.wait(index*64,64).unwrap()).collect::<Vec<_>>();
        cache.drive_inner();cache.drive_inner();
        let mut waiting=std::pin::pin!(source.wait(128,64).unwrap());
        let mut context=Context::from_waker(Waker::noop());
        for _ in 0..10 {assert!(std::future::Future::poll(waiting.as_mut(),&mut context).is_pending());}
        reader.finish(17);cache.drive_inner();
        assert_eq!(reader.requests.lock().unwrap().back().unwrap().0,128);
        drop(waits);
    }
    #[test]
    fn ready_range_polls_and_lease_drops_do_not_wake_read_workers() {
        let (budget,_cache,reader,_source)=setup();
        let calls=Arc::new(AtomicUsize::new(0));
        let counter=calls.clone();
        let cache=ReadCache::new(budget);
        cache.set_scheduler(Arc::new(move |_|{counter.fetch_add(1,Ordering::Relaxed);}));
        cache.drive_inner();
        let source=cache.source(4096,reader.clone());
        assert!(matches!(source.poll(0,1024).unwrap(),RangeState::Pending));
        cache.drive_inner();reader.finish(43);
        let scheduled=calls.load(Ordering::Relaxed);
        for _ in 0..100 {
            let RangeState::Ready(bytes)=source.poll(0,1024).unwrap() else {panic!("cached range")};
            assert!(bytes.iter().all(|byte|*byte==43));
        }
        assert_eq!(calls.load(Ordering::Relaxed),scheduled);
        drop(source);cache.drive_inner();
        assert_eq!(cache.0.state.lock().unwrap().physical,0);
    }
    #[test]
    fn queue_and_active_reads_are_bounded_and_failures_release_the_charge() {
        let (budget,cache,reader,source)=setup();
        for index in 0..100 {assert!(matches!(source.poll(index*64,64).unwrap(),RangeState::Pending));}
        assert_eq!(cache.0.state.lock().unwrap().entries.values().filter(|entry|matches!(entry.phase,Phase::Queued)).count(),QUEUED_READS);
        for _ in 0..100 {cache.drive_inner();}
        assert_eq!(reader.calls.load(Ordering::Relaxed),ACTIVE_READS);
        let (offset,_,done)=reader.requests.lock().unwrap().pop_front().unwrap();
        done.complete(Err("Read failed".into()));budget.flush_reports();
        assert!(source.poll(offset,64).unwrap_err().contains("Read failed"));
        assert_eq!(cache.0.owner.reported().ram_bytes,64);
        assert_eq!(cache.0.owner.reported().pinned_ram_bytes,64);
        reader.finish(11);cache.drive_inner();assert_eq!(cache.0.state.lock().unwrap().running,1);
    }
    struct WakeCount(AtomicUsize);
    impl std::task::Wake for WakeCount {fn wake(self:Arc<Self>) {self.0.fetch_add(1,Ordering::Relaxed);}}
    #[test]
    fn a_pending_read_wakes_once_and_retains_its_bytes_until_consumed() {
        let (budget,cache,reader,source)=setup();
        let wake=Arc::new(WakeCount(AtomicUsize::new(0)));let waker=Waker::from(wake.clone());
        let mut context=Context::from_waker(&waker);let mut wait=std::pin::pin!(source.wait(0,512).unwrap());
        assert!(std::future::Future::poll(wait.as_mut(),&mut context).is_pending());
        cache.drive_inner();reader.finish(93);source.evict();budget.flush_reports();
        assert_eq!(wake.0.load(Ordering::Relaxed),1);
        assert_eq!(cache.0.owner.reported().pinned_ram_bytes,512);
        let Poll::Ready(Ok(bytes))=std::future::Future::poll(wait.as_mut(),&mut context) else {panic!("completion wakes the waiter")};
        assert!(bytes.iter().all(|byte|*byte==93));
    }
}
