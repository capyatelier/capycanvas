use crate::{MemoryBudget, MemoryClass, MemoryOwner, MemoryOwnerKind, MemoryReport};
use crate::package::{ByteRange, RangeState, MAX_RANGE_BYTES};
use std::{collections::{HashMap,VecDeque}, sync::{Arc, Mutex, OnceLock, Weak, atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering}}, task::{Context, Poll, Waker}};

pub const HOT_SET_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) const QUEUED_READS: usize = 18;
const ACTIVE_READS: usize = QUEUED_READS;
const BUFFER_BYTES:u64=64*1024*1024;

pub(crate) fn buffer_class(length:usize)->usize {
    if length<=4096 {return 4096;}
    let octave=1usize<<(usize::BITS-1-(length-1).leading_zeros());
    length.div_ceil(octave/4)*(octave/4)
}
#[cfg(test)]
std::thread_local! {static READ_ALLOCATIONS:std::cell::Cell<usize>=const {std::cell::Cell::new(0)};}

pub fn read_buffer(length:usize)->Arc<[u8]> {
    #[cfg(test)]
    READ_ALLOCATIONS.set(READ_ALLOCATIONS.get()+1);
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
    fn allocation_bytes(&self,_:u64,length:usize)->usize {buffer_class(length)}
    fn read(&self,offset:u64,length:usize,crc:Option<u32>,mut completion:ReadCompletion) {
        let mut bytes=completion.buffer(length);
        let result=read_exact_at(&self.0,offset,&mut Arc::get_mut(&mut bytes).unwrap()[..length]).and_then(|_|verify_crc(&bytes[..length],crc))
            .and_then(|_|ByteRange::new(bytes,0..length).map_err(str::to_string));
        completion.complete_range(result);
    }
}

pub fn verify_crc(bytes:&[u8],expected:Option<u32>)->Result<(),String> {
    if expected.is_some_and(|expected|crc32fast::hash(bytes)!=expected) {return Err("Recovery resource checksum failed".into());}
    Ok(())
}

pub trait RangeReader: Send + Sync {
    fn transfer_key(&self)->Option<String> {None}
    fn read_filtered(&self,offset:u64,length:usize,crc:u32,descriptor:crate::color::PixelDescriptor,completion:ReadCompletion) {
        self.read(offset,length,None,completion.map(Box::new(move |bytes|crate::package::normalize_filtered_tile(descriptor,&bytes?,crc).map(ByteRange::whole))));
    }
    fn result_bounds(&self,_offset:u64,length:usize)->std::ops::RangeInclusive<usize> {length..=length}
    fn allocation_bytes(&self,offset:u64,length:usize)->usize {*self.result_bounds(offset,length).end()}
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
    running: Vec<Key>,
    buffers:std::collections::BTreeMap<usize,Vec<Arc<[u8]>>>,
    pooled:u64,
    queued: Vec<Key>,
    cold: VecDeque<Key>,
}
impl State {
    fn retire(&mut self,key:&Key,retired:&mut Vec<ByteRange>) {
        let entry=self.remove(key);self.physical-=entry.bytes() as u64;
        if let Phase::Ready(bytes)=entry.phase {retired.push(bytes);}
    }
    fn remove(&mut self,key:&Key)->Entry {
        let entry=self.entries.remove(key).unwrap();
        match &entry.phase {
            Phase::Cold=>self.cold.retain(|cold|cold!=key),
            Phase::Queued=>self.queued.retain(|queued|queued!=key),
            _=>{},
        }
        entry
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Key(u64, u64, usize);
struct Waiter {id:u64,wake:Option<Waker>,distance_squared:u64}
struct Entry {
    source: Weak<Source>,
    phase: Phase,
    used: u64,
    leases: usize,
    waiters: Vec<Waiter>,
    crc:Option<u32>,
    bounds:std::ops::RangeInclusive<usize>,
    reservation:usize,
}
impl Entry {
    fn bytes(&self)->usize {match &self.phase {Phase::Ready(bytes)=>bytes.retained_bytes(),Phase::Reading=>self.reservation,_=>0}}
    fn priority(&self)->u64 {self.waiters.iter().map(|waiter|waiter.distance_squared).min().unwrap_or(u64::MAX)}
    fn working(&self)->bool {self.waiters.iter().any(|waiter|waiter.wake.is_some())}
}
enum Phase { Cold, Queued, Reading, Ready(ByteRange), Failed(String) }
struct Source { id: u64, length: AtomicU64, reader: Arc<dyn RangeReader>, cache: ReadCache }
type ReadTask=(Arc<Source>,Key,Option<u32>,Option<Arc<[u8]>>);
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
            if entry.leases==0 && entry.waiters.is_empty() { state.pinned-=entry.bytes() as u64; }
        }
        let drive=state.physical>cache.limit_bytes() || !state.queued.is_empty() || !state.cold.is_empty();cache.publish(&state);drop(state);if drive {cache.schedule();}
    }
}

type ReadTransform=Box<dyn FnOnce(Result<ByteRange,String>)->Result<ByteRange,String>+Send>;
pub struct ReadCompletion { cache: ReadCache, key: Key, done: bool,transform:Option<ReadTransform>,buffer:Option<Arc<[u8]>> }
impl ReadCompletion {
    pub(crate) fn map(mut self,transform:ReadTransform)->Self {let previous=self.transform.take();self.transform=Some(Box::new(move |result|match previous {Some(previous)=>previous(transform(result)),None=>transform(result)}));self}
    pub fn complete(mut self, result: Result<Arc<[u8]>, String>) {
        self.finish(result.map(ByteRange::whole));
    }
    #[cfg(any(test,not(target_arch="wasm32")))]
    pub(crate) fn complete_range(mut self,result:Result<ByteRange,String>) {self.finish(result);}
    #[cfg(any(test,not(target_arch="wasm32")))]
    pub(crate) fn buffer(&mut self,length:usize)->Arc<[u8]> {
        if let Some(buffer)=self.buffer.take() {assert_eq!(buffer.len(),buffer_class(length));return buffer;}
        let class=buffer_class(length);let mut state=self.cache.0.state.lock().unwrap();
        let bytes=state.buffers.get_mut(&class).and_then(Vec::pop);
        if bytes.is_some() {state.pooled-=class as u64;state.physical-=class as u64;self.cache.publish(&state);}
        drop(state);bytes.unwrap_or_else(||read_buffer(class))
    }
    fn finish(&mut self,result:Result<ByteRange,String>) {
        let input=self.transform.as_ref().and_then(|_|result.as_ref().ok().cloned());
        let result=match self.transform.take() {Some(transform)=>transform(result),None=>result};
        self.done=true;self.cache.complete(self.key,result);
        if let Some(input)=input {self.cache.recycle(vec![input]);}
        if let Some(buffer)=self.buffer.take() {self.cache.recycle(vec![ByteRange::whole(buffer)]);}
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
    fn publish(&self, state: &State) {self.0.pending_reads.store(state.queued.len()+state.running.len(),Ordering::Release);self.0.physical.store(state.physical,Ordering::Release);self.0.owner.publish(Self::report(state));}
    fn schedule(&self) {
        if self.0.scheduled.swap(true,Ordering::AcqRel) {return;}
        if let Some(schedule)=self.0.scheduler.get() {schedule(self.clone());return;}
        #[cfg(not(target_arch="wasm32"))]
        workers::schedule(self.clone());
    }
    fn trim(&self,state:&mut State,limit:u64,expired:bool)->Vec<ByteRange> {
        let mut retired=Vec::new();
        while state.physical>limit && state.pooled>0 {
            let Some(bytes)=state.buffers.values_mut().find_map(Vec::pop) else {break;};
            state.pooled-=bytes.len() as u64;state.physical-=bytes.len() as u64;
            retired.push(ByteRange::whole(bytes));
        }
        if expired {
            let dead=state.entries.iter().filter(|(_,entry)|entry.source.strong_count()==0 && entry.leases==0 && entry.waiters.is_empty() && !matches!(entry.phase,Phase::Reading))
                .map(|(&key,_)|key).collect::<Vec<_>>();
            for key in dead {
                state.retire(&key,&mut retired);
            }
        }
        if state.physical<=limit {return retired;}
        let target=limit-limit/8;
        let mut candidates=state.entries.iter().filter(|(_,entry)|entry.leases==0 && entry.waiters.is_empty() && matches!(entry.phase,Phase::Ready(_)))
            .map(|(&key,entry)|(entry.used,key)).collect::<Vec<_>>();
        candidates.sort_unstable_by_key(|(used,_)|*used);
        for (_,key) in candidates {
            if state.physical<=target {break;}
            state.retire(&key,&mut retired);
        }
        retired
    }
    fn pool_room(state:&State)->u64 {
        state.queued.iter().chain(&state.cold).map(|key|state.entries[key].reservation as u64).fold(0,u64::saturating_add).min(BUFFER_BYTES)
    }
    fn recycle(&self,retired:Vec<ByteRange>) {
        let mut discarded=Vec::new();let mut state=self.0.state.lock().unwrap();
        let room=Self::pool_room(&state);
        for bytes in retired {
            let bytes=bytes.allocation();let length=bytes.len() as u64;
            if Arc::strong_count(&bytes)==1 && Arc::weak_count(&bytes)==0 && bytes.len()==buffer_class(bytes.len()) && state.pooled+length<=room && state.physical+length<=self.limit_bytes() {
                state.pooled+=length;state.physical+=length;state.buffers.entry(bytes.len()).or_default().push(bytes);
            }else{discarded.push(bytes);}
        }
        while state.pooled>room && let Some(bytes)=state.buffers.values_mut().find_map(Vec::pop) {
            state.pooled-=bytes.len() as u64;state.physical-=bytes.len() as u64;discarded.push(bytes);
        }
        self.publish(&state);drop(state);drop(discarded);
    }
    fn ready(&self, state: &mut State, key: Key, source: &Arc<Source>, crc:Option<u32>) -> Result<RangeState,String> {
        state.clock=state.clock.saturating_add(1);
        let queued=state.queued.len();
        let entry=state.entries.entry(key).or_insert_with(||Entry {source:Arc::downgrade(source),phase:Phase::Cold,used:state.clock,leases:0,waiters:Vec::new(),crc,bounds:source.reader.result_bounds(key.1,key.2),reservation:source.reader.allocation_bytes(key.1,key.2)});
        if crc.is_some() && entry.crc!=crc {return Err("Conflicting immutable range checksum".into());}
        match &entry.phase {
            Phase::Ready(bytes)=>{
                entry.used=state.clock;
                if entry.leases==0 && entry.waiters.is_empty() {state.pinned+=bytes.retained_bytes() as u64;}
                entry.leases+=1;
                Ok(RangeState::Ready(bytes.clone().leased(Arc::new(Lease {source:source.clone(),key}))))
            },
            Phase::Failed(error)=>{
                let error=error.clone();
                if error==crate::MEMORY_BUDGET_EXCEEDED && entry.waiters.is_empty() {state.remove(&key);}
                Err(error)
            },
            Phase::Cold=>{
                if queued<QUEUED_READS {
                    entry.phase=Phase::Queued;state.cold.retain(|cold|*cold!=key);state.queued.push(key);
                }
                Ok(RangeState::Pending)
            },
            _=>Ok(RangeState::Pending),
        }
    }
    fn complete(&self, key: Key, result: Result<ByteRange,String>) {
        let mut state=self.0.state.lock().unwrap();
        let entry=state.entries.get_mut(&key).unwrap();
        let result=result.and_then(|bytes|if entry.bounds.contains(&bytes.len()) && bytes.len()<=MAX_RANGE_BYTES {Ok(bytes)}else{Err("Incomplete tile read".into())});
        let reserved=entry.bytes() as u64;
        let pinned=entry.leases>0 || !entry.waiters.is_empty();
        let wakers=entry.waiters.iter().filter_map(|waiter|waiter.wake.clone()).collect::<Vec<_>>();
        entry.phase=match result {Ok(bytes)=>Phase::Ready(bytes),Err(error)=>Phase::Failed(error)};
        let actual=entry.bytes() as u64;
        state.running.retain(|running|*running!=key);
        state.physical=state.physical-reserved+actual;
        state.pinned=state.pinned-reserved+if pinned {actual}else {0};
        self.publish(&state);
        #[cfg(not(target_arch="wasm32"))] self.0.changed.notify_all();
        let drive=!state.queued.is_empty() || state.physical>self.limit_bytes();
        drop(state);
        for wake in wakers {wake.wake();}
        if drive {self.schedule();}
    }
    fn take_read(&self,state:&mut State)->(Option<ReadTask>,Vec<ByteRange>) {
        let expired=self.0.expired.swap(false,Ordering::AcqRel);
        let mut retired=if expired || state.physical>self.limit_bytes() {self.trim(state,self.limit_bytes(),expired)}else{Vec::new()};
        let task=loop {
            while state.queued.len()<QUEUED_READS {
                let Some(index)=(0..state.cold.len()).min_by_key(|&index|state.entries[&state.cold[index]].priority()) else {break;};
                let key=state.cold.remove(index).unwrap();
                state.entries.get_mut(&key).unwrap().phase=Phase::Queued;state.queued.push(key);
            }
            if state.running.len()>=ACTIVE_READS || state.queued.is_empty() {break None;}
            let index=(0..state.queued.len()).min_by_key(|&index| {
                let key=state.queued[index];let entry=&state.entries[&key];
                let sharing=state.running.iter().filter(|other|other.0==key.0).count();
                (!entry.working(),entry.priority(),sharing,entry.used)
            }).unwrap();
            let key=state.queued[index];
            let Some(source)=state.entries[&key].source.upgrade() else {state.retire(&key,&mut retired);continue;};
            let bytes=state.entries[&key].reservation as u64;
            let limit=if state.entries[&key].working() {self.limit_bytes().max(state.pinned.saturating_add(bytes))}else{self.limit_bytes()};
            retired.extend(self.trim(state,limit.saturating_sub(bytes),false));
            if state.physical.saturating_add(bytes)>limit {break None;}
            state.queued.swap_remove(index);
            let entry=state.entries.get_mut(&key).unwrap();
            entry.phase=Phase::Reading;let bytes=entry.bytes() as u64;
            state.physical+=bytes;state.pinned+=bytes;state.running.push(key);
            let class=buffer_class(key.2);
            let buffer=(class<=state.entries[&key].reservation).then(||retired.iter().position(|bytes|bytes.retained_bytes()==class && bytes.unique_allocation()))
                .flatten()
                .map(|index|retired.swap_remove(index).allocation());
            break Some((source,key,state.entries[&key].crc,buffer));
        };
        self.publish(state);(task,retired)
    }
    fn read(&self,(source,key,crc,buffer):ReadTask) {
        if !self.0.state.lock().unwrap().queued.is_empty() {self.schedule();}
        source.reader.read(key.1,key.2,crc,ReadCompletion {cache:self.clone(),key,done:false,transform:None,buffer});
    }
    fn drive_inner(&self) {
        self.0.scheduled.store(false,Ordering::Release);
        let (task,retired)=self.take_read(&mut self.0.state.lock().unwrap());
        self.recycle(retired);
        if let Some(task)=task {self.read(task);}
    }
    pub fn drive(&self) {self.drive_inner();}
}

impl ReadSource {
    pub(crate) fn allocation_bytes(&self,offset:u64,length:usize)->usize {self.0.reader.allocation_bytes(offset,length)}
    pub(crate) fn read_filtered(&self,offset:u64,length:usize,crc:u32,descriptor:crate::color::PixelDescriptor,completion:ReadCompletion) {
        match self.key(offset,length) {Ok(_)=>self.0.reader.read_filtered(offset,length,crc,descriptor,completion),Err(error)=>completion.complete(Err(error))}
    }
    pub(crate) fn read_direct(&self,offset:u64,length:usize,crc:Option<u32>,completion:ReadCompletion) {
        match self.key(offset,length) {Ok(_)=>self.0.reader.read(offset,length,crc,completion),Err(error)=>completion.complete(Err(error))}
    }
    pub(crate) fn cached(&self,offset:u64,length:u64)->bool {
        let Ok(length)=usize::try_from(length) else {return false;};
        self.0.cache.0.state.lock().unwrap().entries.get(&Key(self.0.id,offset,length)).is_some_and(|entry|matches!(entry.phase,Phase::Ready(_)))
    }

    #[cfg(not(target_arch="wasm32"))]
    pub fn file(file:Arc<std::fs::File>, length:u64)->Self {ReadCache::process().source(length,Arc::new(FileReader(file)))}
    fn key(&self, offset:u64, length:usize)->Result<Key,String> {
        if length>MAX_RANGE_BYTES || offset.checked_add(length as u64).is_none_or(|end|end>self.0.length.load(Ordering::Acquire)) {return Err("Tile read exceeds backing".into());}
        let bounds=self.0.reader.result_bounds(offset,length);
        if bounds.is_empty() || *bounds.end()>MAX_RANGE_BYTES {return Err("Tile result exceeds bounded range".into());}
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
        let mut state=self.0.cache.0.state.lock().unwrap();let queued=state.queued.len();
        let result=self.0.cache.ready(&mut state,key,&self.0,crc);
        let changed=state.queued.len()>queued;
        self.0.cache.publish(&state);drop(state);if changed {self.0.cache.schedule();}result
    }
    pub fn evict(&self) {
        let mut state=self.0.cache.0.state.lock().unwrap();
        let keys=state.entries.iter().filter(|(key,entry)|key.0==self.0.id && entry.leases==0 && entry.waiters.is_empty() && matches!(entry.phase,Phase::Ready(_)))
            .map(|(&key,_)|key).collect::<Vec<_>>();
        let mut retired=Vec::new();for key in keys {state.retire(&key,&mut retired);}
        self.0.cache.publish(&state);
        drop(state);self.0.cache.recycle(retired);
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
        let entry=state.entries.entry(key).or_insert_with(||Entry {source:Arc::downgrade(&self.0),phase:Phase::Cold,used,leases:0,waiters:Vec::new(),crc,bounds:self.0.reader.result_bounds(offset,length),reservation:self.0.reader.allocation_bytes(offset,length)});
        entry.waiters.push(Waiter {id,wake:None,distance_squared:0});let bytes=entry.bytes() as u64;
        if entry.waiters.len()==1 && matches!(entry.phase,Phase::Cold) {state.cold.push_back(key);}
        if ready && !existed {state.pinned+=bytes;}
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
        wait.read_on_worker()
    }
}

impl ReadWait {
    pub fn start(&self) {self.source.0.cache.schedule();}
    #[cfg(not(target_arch="wasm32"))]
    pub fn read_on_worker(&self)->Result<ByteRange,String> {
        let crc=self.crc;
        let wait=self;
        let source=&self.source;
        let cache=&source.0.cache;let mut state=cache.0.state.lock().unwrap();
        state.entries.get_mut(&wait.key).unwrap().waiters.iter_mut().find(|waiter|waiter.id==wait.id).unwrap().wake=Some(Waker::noop().clone());
        loop {
            match cache.ready(&mut state,wait.key,&source.0,crc)? {
                RangeState::Ready(bytes)=>return Ok(bytes),
                RangeState::Pending=>{
                    let (task,retired)=cache.take_read(&mut state);
                    if task.is_none() && retired.is_empty() {state=cache.0.changed.wait(state).map_err(|_|"Tile read wait failed")?;}
                    else {
                        drop(state);cache.recycle(retired);if let Some(task)=task {cache.read(task);}
                        state=cache.0.state.lock().unwrap();
                    }
                },
            }
        }
    }
}

impl crate::package::ByteSource for ReadSource {
    fn transfer_key(&self)->Option<String> {self.0.reader.transfer_key()}
    fn byte_len(&self)->u64 {self.byte_len()}
    fn resident_bytes(&self)->usize {0}
    fn read_source(&self)->Option<ReadSource> {Some(self.clone())}
    fn poll(&self,offset:u64,length:usize)->Result<RangeState,String> {self.poll(offset,length)}
    #[cfg(not(target_arch="wasm32"))]
    fn poll_on_worker(&self,offset:u64,length:usize)->Result<RangeState,String> {self.wait_on_worker(offset,length).map(RangeState::Ready)}
    fn wait(&self,offset:u64,length:usize)->crate::package::store_io::FileFuture<'_,ByteRange> {
        Box::pin(async move {ReadSource::wait(self,offset,length)?.await})
    }
}

pub struct ReadWait { source: ReadSource, key: Key, id:u64, crc:Option<u32> }
impl ReadWait {
    pub fn prioritize(&self,distance_squared:u64) {
        let mut state=self.source.0.cache.0.state.lock().unwrap();
        if let Some(entry)=state.entries.get_mut(&self.key) && let Some(waiter)=entry.waiters.iter_mut().find(|waiter|waiter.id==self.id) {waiter.distance_squared=distance_squared;}
    }
}
impl ReadWait {
    pub fn poll_bytes(&self,waker:&Waker)->Poll<Result<ByteRange,String>> {
        let cache=&self.source.0.cache;
        let mut state=cache.0.state.lock().unwrap();
        match cache.ready(&mut state,self.key,&self.source.0,self.crc) {
            Ok(RangeState::Ready(bytes))=>Poll::Ready(Ok(bytes)),
            Err(error)=>Poll::Ready(Err(error)),
            Ok(RangeState::Pending)=>{
                let wake=&mut state.entries.get_mut(&self.key).unwrap().waiters.iter_mut().find(|waiter|waiter.id==self.id).unwrap().wake;
                if wake.as_ref().is_none_or(|wake|!wake.will_wake(waker)) {*wake=Some(waker.clone());}
                cache.publish(&state);drop(state);cache.schedule();Poll::Pending
            },
        }
    }
}
impl std::future::Future for ReadWait {
    type Output=Result<ByteRange,String>;
    fn poll(self:std::pin::Pin<&mut Self>,context:&mut Context<'_>)->Poll<Self::Output> {self.poll_bytes(context.waker())}
}
impl Drop for ReadWait {
    fn drop(&mut self) {
        let cache=&self.source.0.cache;
        let mut state=cache.0.state.lock().unwrap();
        if let Some(entry)=state.entries.get_mut(&self.key) {
            entry.waiters.retain(|waiter|waiter.id!=self.id);
            let unpinned=if entry.leases==0 && entry.waiters.is_empty() && matches!(entry.phase,Phase::Ready(_)) {entry.bytes() as u64}else{0};
            let cold=entry.waiters.is_empty() && matches!(entry.phase,Phase::Cold);
            let refused=entry.waiters.is_empty() && matches!(&entry.phase,Phase::Failed(error) if error==crate::MEMORY_BUDGET_EXCEEDED);
            state.pinned-=unpinned;if cold {state.cold.retain(|cold|*cold!=self.key);}
            if refused {state.remove(&self.key);}
        }
        let drive=state.physical>cache.limit_bytes() || !state.queued.is_empty() || !state.cold.is_empty();cache.publish(&state);drop(state);if drive {cache.schedule();}
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
    fn one_package_and_multiple_sources_fill_the_available_read_window() {
        for multiple in [false,true] {
            let (_budget,cache,reader,source)=setup();
            let sources=(0..8).map(|_|if multiple {cache.source(1024,reader.clone())}else{source.clone()}).collect::<Vec<_>>();
            let waits=sources.iter().enumerate().map(|(index,source)|source.wait(index as u64*64,64).unwrap()).collect::<Vec<_>>();
            for _ in 0..8 {cache.drive_inner();}
            assert_eq!(reader.calls.load(Ordering::Relaxed),8,"a single package must fill the same window as independent sources");
            drop(waits);
        }
    }
    fn allowance(budget:&MemoryBudget,bytes:u64)->MemoryOwner {
        let owner=budget.register_owner(MemoryOwnerKind::Authored,MemoryClass::Pinned,None);
        let pinned=budget.ram_limit_bytes()-bytes;
        owner.report(MemoryReport {ram_bytes:pinned,pinned_ram_bytes:pinned,..Default::default()});owner
    }
    #[test]
    fn poll_reads_fit_the_hot_set_and_releasing_a_lease_starts_the_next_read() {
        let (budget,cache,reader,source)=setup();let _other=allowance(&budget,128);
        assert_eq!(cache.limit_bytes(),128);
        for index in 0..8 {assert!(matches!(source.poll(index*64,64).unwrap(),RangeState::Pending));}
        for _ in 0..8 {cache.drive_inner();}
        assert_eq!(reader.calls.load(Ordering::Relaxed),2);assert_eq!(cache.0.state.lock().unwrap().physical,128);
        reader.finish(7);reader.finish(9);
        let RangeState::Ready(first)=source.poll(0,64).unwrap() else {panic!("completed input")};
        let RangeState::Ready(second)=source.poll(64,64).unwrap() else {panic!("completed input")};
        cache.drive_inner();assert_eq!(reader.calls.load(Ordering::Relaxed),2);
        drop(first);assert!(cache.0.scheduled.load(Ordering::Acquire));cache.drive_inner();
        assert_eq!(reader.calls.load(Ordering::Relaxed),3);assert_eq!(cache.0.state.lock().unwrap().physical,128);
        drop(second);
    }
    #[test]
    fn camera_priority_precedes_source_sharing_without_serializing_a_package() {
        let (budget,cache,reader,source)=setup();let _other=allowance(&budget,128);
        let other=cache.source(1024,reader.clone());
        let mut waits=[source.wait(0,64).unwrap(),source.wait(64,64).unwrap(),other.wait(128,64).unwrap()];
        for (wait,distance) in waits.iter_mut().zip([0,1,100]) {wait.prioritize(distance);}
        cache.drive_inner();cache.drive_inner();
        let offsets=reader.requests.lock().unwrap().iter().map(|(offset,_,_)|*offset).collect::<Vec<_>>();
        assert_eq!(offsets,[0,64],"camera-near inputs precede a distant source");
    }
    #[test]
    fn an_idle_read_cache_keeps_no_recycled_buffers() {
        struct Pooled;
        impl RangeReader for Pooled {
            fn allocation_bytes(&self,_:u64,length:usize)->usize {buffer_class(length)}
            fn read(&self,_:u64,length:usize,_:Option<u32>,mut done:ReadCompletion) {
                let bytes=done.buffer(length);done.complete_range(ByteRange::new(bytes,0..length).map_err(str::to_string));
            }
        }
        let (_,cache,_,_)=setup();let source=cache.source(64*1024*1024,Arc::new(Pooled));let length=1<<20;
        let mut waits=(0..8u64).map(|index|Box::pin(source.wait(index*length as u64,length).unwrap())).collect::<Vec<_>>();
        for _ in 0..8 {cache.drive_inner();}
        let reads=waits.iter_mut().map(|wait|match std::future::Future::poll(wait.as_mut(),&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(Ok(bytes))=>bytes,_=>panic!("completed read")}).collect::<Vec<_>>();
        drop(waits);drop(reads);source.evict();
        let held={let state=cache.0.state.lock().unwrap();(state.pooled,state.physical)};
        assert_eq!(held,(0,0),"no read waits for a recycled buffer");
    }
    #[test]
    fn pooled_reads_keep_the_logical_range_and_reuse_only_unleased_allocations() {
        struct Pooled;
        impl RangeReader for Pooled {
            fn allocation_bytes(&self,_:u64,length:usize)->usize {buffer_class(length)}
            fn read(&self,_:u64,length:usize,_:Option<u32>,mut done:ReadCompletion) {
                let mut bytes=done.buffer(length);Arc::get_mut(&mut bytes).unwrap()[..length].fill(71);
                done.complete_range(ByteRange::new(bytes,0..length).map_err(str::to_string));
            }
        }
        let (budget,cache,_,_)=setup();let source=cache.source(1024*1024,Arc::new(Pooled));let length=262_145;
        let mut wait=Box::pin(source.wait(0,length).unwrap());cache.drive_inner();
        let Poll::Ready(Ok(bytes))=std::future::Future::poll(wait.as_mut(),&mut Context::from_waker(Waker::noop())) else {panic!("completed pooled read")};drop(wait);
        let allocation=bytes.clone().allocation();let pointer=Arc::as_ptr(&allocation);drop(allocation);
        assert_eq!(bytes.len(),length);assert_eq!(bytes.retained_bytes(),buffer_class(length));assert!(bytes.iter().all(|byte|*byte==71));
        source.evict();let pooled=cache.0.state.lock().unwrap().pooled;assert_eq!(pooled,0);
        let mut wait=Box::pin(source.wait(length as u64,length).unwrap());
        drop(bytes);source.evict();let pooled=cache.0.state.lock().unwrap().pooled;assert_eq!(pooled,buffer_class(length) as u64);
        cache.drive_inner();
        let Poll::Ready(Ok(bytes))=std::future::Future::poll(wait.as_mut(),&mut Context::from_waker(Waker::noop())) else {panic!("reused pooled read")};
        assert!(std::ptr::addr_eq(Arc::as_ptr(&bytes.clone().allocation()),pointer));
        let pooled=cache.0.state.lock().unwrap().pooled;assert_eq!(pooled,0);drop(wait);drop(bytes);
        source.evict();budget.flush_reports();
        let (pooled,physical)={let state=cache.0.state.lock().unwrap();(state.pooled,state.physical)};
        assert!(pooled<=BUFFER_BYTES);assert_eq!(cache.0.owner.reported().ram_bytes,physical);
    }
    #[test]
    fn filtered_pooled_input_is_recycled_without_truncating_the_transformed_result() {
        struct Filtered;
        impl RangeReader for Filtered {
            fn result_bounds(&self,_:u64,_:usize)->std::ops::RangeInclusive<usize> {192..=192}
            fn allocation_bytes(&self,_:u64,length:usize)->usize {buffer_class(length)+192}
            fn read(&self,_:u64,length:usize,_:Option<u32>,mut done:ReadCompletion) {
                let mut bytes=done.buffer(length);Arc::get_mut(&mut bytes).unwrap()[..length].fill(39);
                let done=done.map(Box::new(|input|{let input=input?;assert_eq!(input.len(),64);assert!(input.iter().all(|byte|*byte==39));Ok(ByteRange::whole(Arc::from([17u8;192])))}));
                done.complete_range(ByteRange::new(bytes,0..length).map_err(str::to_string));
            }
        }
        let (budget,cache,_,_)=setup();let source=cache.source(128,Arc::new(Filtered));
        let mut wait=Box::pin(source.wait(0,64).unwrap());let _waiting=source.wait(64,64).unwrap();cache.drive_inner();
        let Poll::Ready(Ok(bytes))=std::future::Future::poll(wait.as_mut(),&mut Context::from_waker(Waker::noop())) else {panic!("filtered input completed")};
        let pooled=cache.0.state.lock().unwrap().pooled;
        assert_eq!(&*bytes,&[17u8;192]);assert_eq!(bytes.retained_bytes(),192);assert_eq!(pooled,4096,"a waiting read keeps the filtered input for reuse");
        budget.flush_reports();let reported=cache.0.owner.reported();assert_eq!(reported.ram_bytes,4096+192);assert_eq!(reported.pinned_ram_bytes,192);
    }
    #[test]
    fn a_full_hot_set_transfers_an_evicted_buffer_to_its_replacement() {
        struct Pooled(Mutex<Vec<usize>>);
        impl RangeReader for Pooled {
            fn allocation_bytes(&self,_:u64,length:usize)->usize {buffer_class(length)}
            fn read(&self,_:u64,length:usize,_:Option<u32>,mut done:ReadCompletion) {
                let mut bytes=done.buffer(length);self.0.lock().unwrap().push(Arc::as_ptr(&bytes) as *const u8 as usize);
                Arc::get_mut(&mut bytes).unwrap()[..length].fill(71);
                done.complete_range(ByteRange::new(bytes,0..length).map_err(str::to_string));
            }
        }
        let (budget,cache,_,_)=setup();let _other=allowance(&budget,4096);let reader=Arc::new(Pooled(Default::default()));let source=cache.source(128,reader.clone());
        let allocations=READ_ALLOCATIONS.get();
        for offset in [0,64] {
            let mut wait=Box::pin(source.wait(offset,64).unwrap());cache.drive_inner();
            let Poll::Ready(Ok(bytes))=std::future::Future::poll(wait.as_mut(),&mut Context::from_waker(Waker::noop())) else {panic!("replacement read completed")};
            assert_eq!(&*bytes,&[71;64]);assert_eq!(cache.0.state.lock().unwrap().physical,4096);
        }
        let pointers=reader.0.lock().unwrap();assert_eq!(pointers.len(),2);assert_eq!(pointers[0],pointers[1],"a saturated cache reuses its victim without a fresh allocation");
        assert_eq!(READ_ALLOCATIONS.get()-allocations,1,"only the first read allocates, even at a full grant");
    }
    #[test]
    fn four_times_the_read_allowance_more_than_doubles_delayed_read_throughput() {
        fn frames(window:u64,multiple:bool)->usize {
            let (budget,cache,reader,source)=setup();let _other=allowance(&budget,window*64);
            let sources=(0..32).map(|_|if multiple {cache.source(4096,reader.clone())}else{source.clone()}).collect::<Vec<_>>();
            let mut waits=sources.iter().enumerate().map(|(index,source)|Some(Box::pin(source.wait(index as u64*64,64).unwrap()))).collect::<Vec<_>>();
            let mut context=Context::from_waker(Waker::noop());let mut due=VecDeque::new();let mut completed=0;let mut delivered=[false;32];
            for frame in 0..512 {
                while due.front().is_some_and(|(ready,_,_,_)|*ready<=frame) {
                    let (_,index,length,done):(usize,usize,usize,ReadCompletion)=due.pop_front().unwrap();done.complete(Ok(vec![17;length].into()));delivered[index]=true;
                }
                for (index,wait) in waits.iter_mut().enumerate() {
                    if delivered[index] && let Some(pending)=wait && let Poll::Ready(bytes)=std::future::Future::poll(pending.as_mut(),&mut context) {drop(bytes.unwrap());*wait=None;completed+=1;}
                }
                if completed==32 {return frame;}
                for _ in 0..ACTIVE_READS {cache.drive_inner();}
                for (offset,length,done) in reader.requests.lock().unwrap().drain(..) {due.push_back((frame+4,offset as usize/64,length,done));}
            }
            panic!("bounded read window must drain");
        }
        for multiple in [false,true] {let small=frames(2,multiple);let large=frames(8,multiple);assert!(large*2<=small,"2-slot={small}, 8-slot={large}, multiple={multiple}");}
    }
    #[test]
    fn temporary_refusals_reach_all_consumers_and_allow_a_new_read() {
        for subscribers in [0, 2] {
            let (_budget, cache, reader, source) = setup();
            let mut waits = (0..subscribers).map(|_| source.wait(0, 64).unwrap()).collect::<Vec<_>>();
            let mut context = Context::from_waker(Waker::noop());
            if subscribers == 0 { assert!(matches!(source.poll(0, 64).unwrap(), RangeState::Pending)); }
            for wait in &mut waits {
                assert!(std::future::Future::poll(std::pin::Pin::new(wait), &mut context).is_pending());
            }
            cache.drive_inner();
            let (_, _, completion) = reader.requests.lock().unwrap().pop_front().unwrap();
            completion.complete(Err(crate::MEMORY_BUDGET_EXCEEDED.into()));
            if subscribers == 0 { assert_eq!(source.poll(0, 64).unwrap_err(), crate::MEMORY_BUDGET_EXCEEDED); }
            for wait in &mut waits {
                assert!(matches!(std::future::Future::poll(std::pin::Pin::new(wait), &mut context),
                    Poll::Ready(Err(error)) if error == crate::MEMORY_BUDGET_EXCEEDED));
            }
            drop(waits);
            cache.drive_inner();
            assert_eq!(reader.calls.load(Ordering::Relaxed), 1);
            assert!(matches!(source.poll(0, 64).unwrap(), RangeState::Pending));
            cache.drive_inner();
            reader.finish(9);
            let RangeState::Ready(bytes) = source.poll(0, 64).unwrap() else { panic!("explicit read completes"); };
            assert_eq!(&*bytes, &[9; 64]);
            assert_eq!(reader.calls.load(Ordering::Relaxed), 2);
        }
    }
    fn trim(cache:&ReadCache,limit:u64)->(u64,u64) {
        let mut state=cache.0.state.lock().unwrap();let retired=cache.trim(&mut state,limit,false);
        let bytes=(state.physical,state.pinned);drop(state);drop(retired);bytes
    }
    #[test]
    fn trim_leaves_room_for_a_batch_and_preserves_leases() {
        let (_budget,cache,reader,source)=setup();
        for index in 0..32 {
            assert!(matches!(source.poll(index*64,64).unwrap(),RangeState::Pending));
            cache.drive_inner();reader.finish(index as u8);
        }
        let RangeState::Ready(leased)=source.poll(31*64,64).unwrap() else {panic!("cached range")};
        assert!(matches!(source.poll(32*64,64).unwrap(),RangeState::Pending));cache.drive_inner();
        let charged={let state=cache.0.state.lock().unwrap();(state.physical,state.pinned)};
        assert_eq!(charged,(2112,128));
        let trimmed=trim(&cache,2048);
        assert_eq!(trimmed,(1792,128));
        reader.finish(32);
        for index in 33..37 {
            assert!(matches!(source.poll(index*64,64).unwrap(),RangeState::Pending));
            cache.drive_inner();reader.finish(index as u8);
            let physical=cache.0.state.lock().unwrap().physical;
            assert_eq!(trim(&cache,2048).0,physical);
        }
        assert_eq!(&*leased,&[31;64]);
        let pinned=trim(&cache,0);
        assert_eq!(pinned,(64,64));
        drop(leased);
        let empty=trim(&cache,0);
        assert_eq!(empty,(0,0));
    }
    #[test]
    fn trim_detaches_buffers_before_releasing_them() {
        let (_budget,cache,reader,source)=setup();
        assert!(matches!(source.poll(0,64).unwrap(),RangeState::Pending));cache.drive_inner();reader.finish(17);
        let buffer={let state=cache.0.state.lock().unwrap();
            let Phase::Ready(bytes)=&state.entries[&Key(source.0.id,0,64)].phase else {panic!("cached range")};
            Arc::downgrade(&bytes.clone().allocation())};
        let (retired,physical,alive)={
            let mut state=cache.0.state.lock().unwrap();let retired=cache.trim(&mut state,0,false);
            (retired,state.physical,buffer.strong_count())
        };
        assert_eq!(physical,0);assert_eq!(alive,1);
        drop(retired);assert_eq!(buffer.strong_count(),0);
    }
    fn assert_queue_indexes(cache:&ReadCache) {
        use std::collections::HashSet;
        let (cold,queued,expected_cold,expected_queued,actual,expected)={
            let state=cache.0.state.lock().unwrap();
            let cold=state.cold.iter().copied().collect::<Vec<_>>();let queued=state.queued.clone();
            let expected_cold=state.entries.iter().filter(|(_,entry)|matches!(entry.phase,Phase::Cold)&&!entry.waiters.is_empty()).map(|(&key,_)|key).collect::<HashSet<_>>();
            let expected_queued=state.entries.iter().filter(|(_,entry)|matches!(entry.phase,Phase::Queued)).map(|(&key,_)|key).collect::<HashSet<_>>();
            let physical=state.entries.values().map(|entry|entry.bytes() as u64).sum::<u64>()+state.pooled;
            let pinned=state.entries.values().filter(|entry|matches!(entry.phase,Phase::Reading)||entry.leases>0||!entry.waiters.is_empty()).map(|entry|entry.bytes() as u64).sum::<u64>();
            let running=state.entries.values().filter(|entry|matches!(entry.phase,Phase::Reading)).count();
            (cold,queued,expected_cold,expected_queued,(state.physical,state.pinned,state.running.len()),(physical,pinned,running))
        };
        assert_eq!(cold.len(),expected_cold.len());assert_eq!(cold.into_iter().collect::<HashSet<_>>(),expected_cold);
        assert_eq!(queued.len(),expected_queued.len());assert_eq!(queued.iter().copied().collect::<HashSet<_>>(),expected_queued);
        assert!(queued.len()<=QUEUED_READS);assert!(actual.2<=ACTIVE_READS);assert_eq!(actual,expected);
    }
    #[test]
    fn seeded_queue_operations_match_a_full_scan() {
        let (_budget,cache,reader,source)=setup();
        let mut sources=vec![Some(source)];let mut waits=Vec::new();let mut leases=Vec::new();
        let mut context=Context::from_waker(Waker::noop());let mut seed=0xace0_251f_736d_9b47u64;let mut seen=[0;10];
        for index in 0..3*QUEUED_READS {
            let mut wait=sources[0].as_ref().unwrap().wait(index as u64*64,64).unwrap();
            assert!(std::future::Future::poll(std::pin::Pin::new(&mut wait),&mut context).is_pending());waits.push(wait);
            assert_queue_indexes(&cache);
        }
        for _ in 0..2048 {
            seed^=seed<<13;seed^=seed>>7;seed^=seed<<17;
            let operation=(seed%10) as usize;seen[operation]+=1;let index=(seed>>16) as usize;
            match operation {
                0|1=>if let Some(source)=&sources[index%sources.len()] {
                    if let Ok(RangeState::Ready(bytes))=source.poll((index%128) as u64*64,64) {leases.push(bytes);}
                },
                2=>if let Some(source)=&sources[index%sources.len()] {
                    let mut wait=source.wait((index%128) as u64*64,64).unwrap();
                    if let Poll::Ready(Ok(bytes))=std::future::Future::poll(std::pin::Pin::new(&mut wait),&mut context) {leases.push(bytes);}
                    waits.push(wait);
                },
                3=>if !waits.is_empty() {drop(waits.swap_remove(index%waits.len()));},
                4|5=>{
                    let request=reader.requests.lock().unwrap().pop_front();
                    if let Some((_,length,done))=request {done.complete(if operation==4 {Ok(vec![index as u8;length].into())}else{Err("Read failed".into())});}
                },
                6=>{
                    if sources.len()<8 {sources.push(Some(cache.source(64*256,reader.clone())));}
                    else {let slot=&mut sources[index%8];*slot=if slot.is_some() {None}else{Some(cache.source(64*256,reader.clone()))};}
                },
                7=>cache.drive_inner(),
                8=>{trim(&cache,(index%16) as u64*64);},
                _=>if !leases.is_empty() {drop(leases.swap_remove(index%leases.len()));}
                    else if let Some(source)=&sources[index%sources.len()] {source.evict();},
            }
            assert_queue_indexes(&cache);
        }
        assert!(seen.into_iter().all(|count|count>0));
        drop(waits);drop(leases);drop(sources);assert_queue_indexes(&cache);
        while let Some((_,_,done))={reader.requests.lock().unwrap().pop_front()} {done.complete(Err("Read owner closed".into()));}
        cache.drive_inner();assert_queue_indexes(&cache);
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn worker_reads_execute_on_the_calling_worker() {
        use std::sync::mpsc;
        struct Immediate(mpsc::Sender<std::thread::ThreadId>);
        impl RangeReader for Immediate {
            fn read(&self,_:u64,length:usize,_:Option<u32>,done:ReadCompletion) {
                self.0.send(std::thread::current().id()).unwrap();done.complete(Ok(vec![73;length].into()));
            }
        }
        let (_budget,cache,_reader,_source)=setup();let (send,receive)=mpsc::channel();
        let source=cache.source(64,Arc::new(Immediate(send)));
        let worker=std::thread::spawn(move ||(std::thread::current().id(),source.wait_on_worker(0,64).unwrap()));
        let inline=receive.recv_timeout(std::time::Duration::from_secs(10));
        if inline.is_err() {cache.drive_inner();}
        let reader=inline.unwrap_or_else(|_|receive.recv_timeout(std::time::Duration::from_secs(10)).unwrap());
        let (caller,bytes)=worker.join().unwrap();assert_eq!(&*bytes,&[73;64]);assert_eq!(reader,caller);
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn inline_and_background_reads_share_the_active_limit_and_deduplicate_keys() {
        use std::sync::mpsc;
        struct Held {
            pending:Controlled,active:Arc<AtomicUsize>,peak:AtomicUsize,
            calls:Mutex<HashMap<u64,usize>>,started:mpsc::Sender<u64>,
        }
        impl RangeReader for Held {
            fn read(&self,offset:u64,length:usize,crc:Option<u32>,done:ReadCompletion) {
                let active=self.active.fetch_add(1,Ordering::AcqRel)+1;self.peak.fetch_max(active,Ordering::AcqRel);
                *self.calls.lock().unwrap().entry(offset).or_default()+=1;self.started.send(offset).unwrap();
                let active=self.active.clone();
                self.pending.read(offset,length,crc,done.map(Box::new(move |result| {active.fetch_sub(1,Ordering::AcqRel);result})));
            }
        }
        let (_budget,cache,_reader,_source)=setup();let (send,receive)=mpsc::channel();
        let reader=Arc::new(Held {pending:Default::default(),active:Default::default(),peak:Default::default(),calls:Default::default(),started:send});
        let source=cache.source((ACTIVE_READS as u64+1)*64,reader.clone());
        assert!(matches!(source.poll(0,64).unwrap(),RangeState::Pending));cache.drive_inner();assert_eq!(receive.recv().unwrap(),0);
        let workers=(1..=ACTIVE_READS).map(|index| {let source=source.clone();std::thread::spawn(move ||source.wait_on_worker(index as u64*64,64))}).collect::<Vec<_>>();
        receive.recv().unwrap();
        let (running,queued)=loop {
            let state=cache.0.state.lock().unwrap();
            let queued=state.queued.iter().any(|key|key.0==source.0.id);
            if state.entries.len()==ACTIVE_READS+1 && state.running.len()>=ACTIVE_READS && queued {
                break (state.running.len(),queued);
            }
            drop(state);std::thread::yield_now();
        };
        assert!(matches!(source.poll(64,64).unwrap(),RangeState::Pending));cache.drive_inner();
        for _ in 2..running {receive.recv().unwrap();}
        let starts=reader.calls.lock().unwrap().values().sum::<usize>();
        while !workers.iter().all(|worker|worker.is_finished()) {
            let request=reader.pending.requests.lock().unwrap().pop_front();
            if let Some((offset,length,done))=request {done.complete(if offset==0 {Err("Read failed".into())}else {Ok(vec![offset as u8;length].into())});}
            else {cache.drive_inner();std::thread::yield_now();}
        }
        for (index,worker) in workers.into_iter().enumerate() {assert_eq!(&*worker.join().unwrap().unwrap(),&[(64+index*64) as u8;64]);}
        assert_eq!(running,ACTIVE_READS);assert!(queued);assert_eq!(starts,ACTIVE_READS);
        assert!(reader.peak.load(Ordering::Acquire)<=ACTIVE_READS);
        assert_eq!(*reader.calls.lock().unwrap(),(0..=ACTIVE_READS).map(|index|(index as u64*64,1)).collect());
        assert_eq!(reader.active.load(Ordering::Acquire),0);assert!(source.poll(0,64).unwrap_err().contains("Read failed"));
        assert_queue_indexes(&cache);
    }
    #[test]
    fn transfer_preparation_registers_only_a_bounded_window_of_lazy_tiles() {
        use crate::{Document,DocumentNames,Editor,Image,PaintBase,SourceTarget,
            color::{ColorProfile,RgbSpace,SampleDepth,source::{SourceChannels,SourceImage,SourceInterpretation}},
            package::{ImmutableBacking,PackageLease,transfer::PreparedTransfer},raster::{TileBlob,TILE_SIZE}};
        struct Held {reader:Controlled,closed:AtomicBool}
        impl RangeReader for Held {
            fn read(&self,offset:u64,length:usize,_crc:Option<u32>,completion:ReadCompletion) {
                let mut requests=self.reader.requests.lock().unwrap();
                if self.closed.load(Ordering::Acquire) {drop(requests);completion.complete(Err("Read owner closed".into()));}
                else {requests.push_back((offset,length,completion));}
            }
        }
        impl Held {
            fn close(&self) {
                let requests={let mut requests=self.reader.requests.lock().unwrap();self.closed.store(true,Ordering::Release);std::mem::take(&mut *requests)};
                for (_,_,completion) in requests {completion.complete(Err("Read owner closed".into()));}
            }
        }
        let cache=ReadCache::process();let held=Arc::new(Held {reader:Default::default(),closed:AtomicBool::new(false)});
        let interpretation=SourceInterpretation {channels:SourceChannels::Rgba,depth:SampleDepth::U8,profile:ColorProfile::Builtin(RgbSpace::Srgb),profile_assumed:false};
        let descriptor=interpretation.descriptor();
        let mut encoder=flate2::write::DeflateEncoder::new(Vec::new(),flate2::Compression::fast());
        std::io::Write::write_all(&mut encoder,&vec![0;(TILE_SIZE*TILE_SIZE*4) as usize]).unwrap();let encoded=encoder.finish().unwrap();
        let backing=ImmutableBacking::new(Arc::new(cache.source(encoded.len() as u64*100,held.clone()))).unwrap();
        let lease=PackageLease::new(backing);let source_id=lease.read_source().0.id;
        let tiles=(0..100).map(|index| {
            let range=lease.range(index*encoded.len() as u64,encoded.len() as u64,crc32fast::hash(&encoded),descriptor,true).unwrap();
            ([index as u32%10,index as u32/10],Arc::new(TileBlob::from_package_range(crate::PortableId::random(),descriptor,range).unwrap()))
        }).collect();
        let mut document=Document::new(crate::PortableId::random(),10*TILE_SIZE,10*TILE_SIZE,DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        let SourceTarget::Paint(paint)=document.working.target.unwrap() else {unreachable!()};
        document.artwork.paint.get_mut(paint).unwrap().base=Some(PaintBase::new(Image::new(Arc::new(SourceImage {extent:[10*TILE_SIZE;2],resolution:None,tiles,interpretation}))));
        let editor=Editor::new(document);let capture=editor.capture(0,Default::default()).unwrap();
        let before=cache.0.state.lock().unwrap().entries.keys().copied().collect::<std::collections::HashSet<_>>();
        let foreign=cache.source(1,held.clone());
        assert!(matches!(foreign.poll(0,1).unwrap(),RangeState::Pending));
        let cancelled=AtomicBool::new(false);let selection=None;
        let budget=MemoryBudget::process();let mut preparing=Box::pin(PreparedTransfer::capture_async(&capture,&selection,&cancelled,&budget));
        let pending=std::future::Future::poll(preparing.as_mut(),&mut Context::from_waker(Waker::noop())).is_pending();
        let registered=cache.0.state.lock().unwrap().entries.keys().filter(|key|key.0==source_id && !before.contains(key)).count();
        drop(preparing);drop(capture);drop(editor);drop(lease);held.close();
        assert!(pending);assert!(registered>0 && registered<=QUEUED_READS,"registered {registered} tile ranges");
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
    fn transformed_ranges_charge_the_result_size_and_keep_it_pinned_until_consumed() {
        struct Expanded(Arc<Controlled>);
        impl RangeReader for Expanded {
            fn result_bounds(&self,_:u64,_:usize)->std::ops::RangeInclusive<usize> {1..=256}
            fn read(&self,offset:u64,length:usize,crc:Option<u32>,completion:ReadCompletion) {
                self.0.read(offset,length,crc,completion.map(Box::new(|bytes|bytes.map(|_|ByteRange::whole(Arc::from([17u8;192]))))));
            }
        }
        let (budget,cache,reader,_source)=setup();let source=cache.source(1024,Arc::new(Expanded(reader.clone())));
        let mut wait=Box::pin(source.wait(0,64).unwrap());let mut context=Context::from_waker(Waker::noop());
        assert!(std::future::Future::poll(wait.as_mut(),&mut context).is_pending());cache.drive_inner();budget.flush_reports();
        assert_eq!(cache.0.owner.reported().ram_bytes,256);assert_eq!(cache.0.owner.reported().pinned_ram_bytes,256);
        reader.finish(1);budget.flush_reports();
        assert_eq!(cache.0.owner.reported().ram_bytes,192);assert_eq!(cache.0.owner.reported().pinned_ram_bytes,192);
        let Poll::Ready(Ok(bytes))=std::future::Future::poll(wait.as_mut(),&mut context) else {panic!("transformed read completed")};drop(wait);
        source.evict();assert_eq!(&*bytes,&[17u8;192]);drop(bytes);source.evict();budget.flush_reports();
        assert_eq!(cache.0.owner.reported().ram_bytes,0);assert_eq!(cache.0.owner.reported().pinned_ram_bytes,0);
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
        assert_eq!(cache.0.owner.reported().ram_bytes,(ACTIVE_READS as u64-1)*64);
        assert_eq!(cache.0.owner.reported().pinned_ram_bytes,(ACTIVE_READS as u64-1)*64);
        reader.finish(11);cache.drive_inner();assert_eq!(cache.0.state.lock().unwrap().running.len(),ACTIVE_READS-2);
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
