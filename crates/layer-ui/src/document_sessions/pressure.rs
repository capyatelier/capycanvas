use super::*;
use layer_core::MemoryReport;
use std::sync::{Arc,Mutex};
use std::sync::Weak;

#[cfg(not(target_arch="wasm32"))]
pub type PressureWake=Arc<dyn Fn()+Send+Sync>;
#[cfg(target_arch="wasm32")]
pub type PressureWake=Arc<dyn Fn()>;
#[derive(Clone)]
pub struct StoragePressure(Arc<Pressure>);
struct Pressure {owner:MemoryOwner,memory:MemoryBudget,state:Mutex<PressureState>}
struct PressureState {candidates:Vec<(u64,u64,RetainedTiles)>,resident:usize,limit:usize,revision:u64,generation:u64,running:bool,reclaim:u64,wait:Option<TrimWait>,error:Option<String>,wake:Option<PressureWake>}
impl Pressure {
    fn allowance(&self,state:&PressureState)->usize {state.limit.min(self.owner.grant().ram_bytes.min(usize::MAX as u64) as usize)}
    fn notify(&self)->bool {
        let (wait,result,wake)={
            let mut state=self.state.lock().unwrap();
            if state.running {return true;}
            let result=state.error.clone().map_or(Ok(()),Err);
            if result.is_err() || state.resident<=self.allowance(&state) {(state.wait.take(),result,None)}
            else {(None,result,state.wake.clone())}
        };
        if let Some(wait)=wait {wait.complete(result);}
        let Some(wake)=wake else {self.owner.reclaimed(self.owner.reclaim_epoch());return false;};
        wake();true
    }
    fn report(resident:usize)->MemoryReport {MemoryReport {ram_bytes:resident as u64,demand_ram_bytes:resident as u64,..Default::default()}}
    fn publish(&self,resident:usize) {assert!(self.owner.publish(Self::report(resident)));}
}

impl StoragePressure {
    pub(super) fn new(owner:MemoryOwner,memory:MemoryBudget)->Self {
        let pressure=Self(Arc::new(Pressure {owner:owner.clone(),memory,state:Mutex::new(PressureState {candidates:Vec::new(),resident:0,limit:usize::MAX,revision:0,generation:0,running:false,reclaim:0,wait:None,error:None,wake:None})}));
        #[cfg(not(target_arch="wasm32"))]
        {let weak=Arc::downgrade(&pressure.0);owner.set_wake(Arc::new(move ||Self::notify(&weak)));}
        #[cfg(target_arch="wasm32")]
        {
            thread_local! {static SOURCES:std::cell::RefCell<std::collections::BTreeMap<u64,Weak<Pressure>>>=const {std::cell::RefCell::new(std::collections::BTreeMap::new())};}
            static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(1);
            let id=NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
            SOURCES.with_borrow_mut(|sources|{sources.retain(|_,source|source.strong_count()!=0);sources.insert(id,Arc::downgrade(&pressure.0));});
            owner.set_wake(Arc::new(move || {
                let source=SOURCES.with_borrow(|sources|sources.get(&id).cloned());
                source.is_some_and(|source|Self::notify(&source))
            }));
        }
        pressure
    }
    fn notify(weak:&Weak<Pressure>)->bool {
        weak.upgrade().is_some_and(|source|source.notify())
    }
    pub fn same_source(&self,other:&Self)->bool {Arc::ptr_eq(&self.0,&other.0)}
    pub fn subscribe(&self,wake:PressureWake)->u64 {
        let (generation,wait)={let mut state=self.0.state.lock().unwrap();state.generation=state.generation.wrapping_add(1);state.wake=Some(wake);state.running=false;(state.generation,state.wait.take())};
        if let Some(wait)=wait {wait.complete(Err("Drawing storage owner changed".into()));}generation
    }
    pub fn is_current(&self,generation:u64)->bool {self.0.state.lock().unwrap().generation==generation}
    pub fn request(&self,generation:u64)->Option<(Vec<(u64,u64,RetainedTiles)>,usize)> {
        let mut state=self.0.state.lock().unwrap();
        let allowance=self.0.allowance(&state);
        if state.generation!=generation || state.running || state.error.is_some() || state.resident<=allowance {return None;}
        state.running=true;state.reclaim=self.0.owner.reclaim_epoch();state.wait.get_or_insert_with(TrimWait::pending);
        Some((state.candidates.clone(),allowance))
    }
    pub fn completed(&self,generation:u64,result:Result<(),String>) {
        let reclaim=self.0.state.lock().unwrap().reclaim;
        self.finish(generation,result);
        self.0.owner.reclaimed(reclaim);
    }
    fn finish(&self,generation:u64,result:Result<(),String>) {
        let (candidates,revision)={let state=self.0.state.lock().unwrap();if state.generation!=generation || !state.running {return;}(state.candidates.clone(),state.revision)};
        let bytes=layer_core::raster_storage::resident_tile_bytes(candidates.iter().map(|(_,_,tiles)|tiles)) as u64;
        {
            let mut state=self.0.state.lock().unwrap();if state.generation!=generation {return;}
            state.running=false;state.error=result.err();
            if state.revision==revision {state.resident=bytes.min(usize::MAX as u64) as usize;self.0.publish(state.resident);}
        }
        self.0.memory.flush_reports();self.0.notify();
    }
    pub fn wait(&self)->Option<TrimWait> {self.0.state.lock().unwrap().wait.clone()}
    pub fn in_flight(&self)->bool {self.0.state.lock().unwrap().wait.is_some()}
    pub fn error(&self)->Option<String> {self.0.state.lock().unwrap().error.clone()}
    pub fn storage_completed(&self,result:Result<(),String>) {
        self.0.state.lock().unwrap().error=result.err();self.0.notify();
    }
    pub(super) fn update(&self,candidates:Vec<(u64,u64,RetainedTiles)>,resident:usize,limit:usize,transition:Option<(&RetainedTiles,bool)>) {
        {let mut state=self.0.state.lock().unwrap();state.candidates=candidates;state.resident=resident;state.limit=limit;state.revision=state.revision.wrapping_add(1);if transition.is_none() {self.0.publish(resident);}}
        if let Some((tiles,parked))=transition {tiles.set_parked(parked,&self.0.owner,Pressure::report(resident));}
        self.0.memory.flush_reports();self.0.notify();
    }
}

impl Drop for Pressure {
    fn drop(&mut self) {
        if let Some(wait)=self.state.get_mut().unwrap().wait.take() {wait.complete(Err("Drawing storage owner closed".into()));}
    }
}

#[cfg(all(test,not(target_arch="wasm32")))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize,Ordering};

    #[test]
    fn budget_wakes_under_the_allowance_do_not_schedule_storage() {
        let budget=MemoryBudget::with_facts(layer_core::MemoryFacts {override_bytes:Some(1<<30),..Default::default()});
        let owner=budget.register_owner(MemoryOwnerKind::ParkedTabs,MemoryClass::Warm,None);
        let source=StoragePressure::new(owner.clone(),budget.clone());
        let queued=Arc::new(AtomicUsize::new(0));let executor=queued.clone();
        let generation=source.subscribe(Arc::new(move || {executor.fetch_add(1,Ordering::Relaxed);}));
        owner.report(MemoryReport {ram_bytes:1<<20,demand_ram_bytes:1<<20,..Default::default()});
        source.update(Vec::new(),1<<20,usize::MAX,None);
        assert!(source.request(generation).is_none());
        let reads=budget.register_owner(MemoryOwnerKind::Other,MemoryClass::Pinned,None);
        let wakes=Arc::new(AtomicUsize::new(0));let count=wakes.clone();
        reads.set_wake(Arc::new(move || {count.fetch_add(1,Ordering::Relaxed);true}));
        for pinned in 1..=100 {
            reads.report(MemoryReport {pinned_ram_bytes:pinned<<20,..Default::default()});
        }
        assert_eq!(wakes.load(Ordering::Relaxed),100);
        assert_eq!(queued.load(Ordering::Relaxed),0);
        source.update(Vec::new(),1<<20,0,None);
        assert_eq!(queued.load(Ordering::Relaxed),1);
        assert_eq!(source.request(generation).unwrap().1,0);
    }
    #[test]
    fn satisfied_or_dropped_sources_finish_idle_trim_waiters() {
        use std::{future::Future,pin::Pin,task::{Context,Poll,Waker}};
        let budget=MemoryBudget::with_facts(layer_core::MemoryFacts {override_bytes:Some(1<<30),..Default::default()});
        for dropped in [false,true] {
            let owner=budget.register_owner(MemoryOwnerKind::ParkedTabs,MemoryClass::Warm,None);
            let source=StoragePressure::new(owner,budget.clone());
            let generation=source.subscribe(Arc::new(||{}));source.update(Vec::new(),1<<20,0,None);
            source.request(generation).unwrap();let mut wait=source.wait().unwrap();
            source.0.state.lock().unwrap().running=false;
            if dropped {drop(source);}else {source.update(Vec::new(),1<<20,usize::MAX,None);assert!(!source.in_flight());}
            let result=Pin::new(&mut wait).poll(&mut Context::from_waker(Waker::noop()));
            assert!(matches!(result,Poll::Ready(Err(_)))==dropped);
            if !dropped {assert_eq!(result,Poll::Ready(Ok(())));}
        }
    }

    #[test]
    fn a_reclaim_accepted_during_a_trim_is_answered_when_the_trim_fails() {
        use layer_core::raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileBlob,TileKey,TILE_SIZE};
        use std::{future::Future,task::{Context,Poll,Waker}};
        let mut document=layer_core::Document::new(layer_core::PortableId::random(),64,64,layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        let layer_core::SourceTarget::Paint(target)=document.working.target.unwrap() else {panic!()};
        let descriptor=document.composition().color.paint_descriptor();
        let pixels=[23,47,83,255].repeat((TILE_SIZE*TILE_SIZE) as usize);
        document.artwork.paint.get_mut(target).unwrap().raster=RasterRevision::backed(RasterData {
            tiles:[(TileKey {plane:RasterPlane::Color,coordinate:[0;2]},RasterTile::backed(TileBlob::encode(descriptor,&pixels).unwrap()))].into(),
            ..Default::default()
        });
        let tiles=layer_core::Editor::new(document).retained_tiles();
        let resident=layer_core::raster_storage::resident_tile_bytes([&tiles]);
        assert!(resident>1);
        let budget=MemoryBudget::with_facts(layer_core::MemoryFacts {override_bytes:Some(1<<30),..Default::default()});
        let source=StoragePressure::new(budget.register_owner(MemoryOwnerKind::ParkedTabs,MemoryClass::Warm,None),budget.clone());
        let generation=source.subscribe(Arc::new(||{}));
        source.update(vec![(1,1,tiles)],resident,usize::MAX,None);
        let import=budget.admit_import(budget.ram_limit_bytes()-resident as u64/2).unwrap();
        assert!(source.request(generation).is_some());
        let cancel=std::sync::atomic::AtomicBool::new(false);
        let mut waiting=Box::pin(import.covering(&cancel));
        let mut context=Context::from_waker(Waker::noop());
        assert!(waiting.as_mut().poll(&mut context).is_pending());
        source.completed(generation,Err("Disk full".into()));
        assert_eq!(waiting.as_mut().poll(&mut context),Poll::Ready(Err(layer_core::CoverageFailure::Refused)),
            "a trim that fails with its tiles still resident answers the reclaim it accepted while running");
    }
}
