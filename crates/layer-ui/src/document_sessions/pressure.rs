use super::*;
use std::sync::{Arc,Mutex};
#[cfg(not(target_arch="wasm32"))]
use std::sync::Weak;

#[derive(Clone)]
pub struct StoragePressure(Arc<Pressure>);
struct Pressure {owner:MemoryOwner,state:Mutex<PressureState>}
struct PressureState {candidates:Vec<(u64,u64,RetainedTiles)>,resident:usize,limit:usize,revision:u64,generation:u64,wake:Option<Arc<dyn Fn()+Send+Sync>>}
impl Pressure {
    fn allowance(&self,state:&PressureState)->usize {state.limit.min(self.owner.grant().ram_bytes.min(usize::MAX as u64) as usize)}
    fn wake(&self)->Option<Arc<dyn Fn()+Send+Sync>> {
        let state=self.state.lock().unwrap();(state.resident>self.allowance(&state)).then(||state.wake.clone()).flatten()
    }
}

impl StoragePressure {
    pub(super) fn new(owner:MemoryOwner)->Self {
        let pressure=Self(Arc::new(Pressure {owner:owner.clone(),state:Mutex::new(PressureState {candidates:Vec::new(),resident:0,limit:usize::MAX,revision:0,generation:0,wake:None})}));
        #[cfg(not(target_arch="wasm32"))]
        {let weak=Arc::downgrade(&pressure.0);owner.set_wake(Arc::new(move ||Self::notify(&weak)));}
        pressure
    }
    #[cfg(not(target_arch="wasm32"))]
    fn notify(weak:&Weak<Pressure>) {
        let wake=weak.upgrade().and_then(|source|source.wake());
        if let Some(wake)=wake {wake();}
    }
    pub fn same_source(&self,other:&Self)->bool {Arc::ptr_eq(&self.0,&other.0)}
    pub fn subscribe(&self,wake:Arc<dyn Fn()+Send+Sync>)->u64 {
        let mut state=self.0.state.lock().unwrap();state.generation=state.generation.wrapping_add(1);state.wake=Some(wake);state.generation
    }
    pub fn is_current(&self,generation:u64)->bool {self.0.state.lock().unwrap().generation==generation}
    pub fn snapshot(&self,generation:u64)->Option<(Vec<(u64,u64,RetainedTiles)>,usize)> {
        let state=self.0.state.lock().unwrap();
        let allowance=self.0.allowance(&state);
        (state.generation==generation && state.resident>allowance).then(||(state.candidates.clone(),allowance))
    }
    pub fn completed(&self,generation:u64) {
        let (candidates,revision)={let state=self.0.state.lock().unwrap();if state.generation!=generation {return;}(state.candidates.clone(),state.revision)};
        let bytes=layer_core::raster_storage::resident_tile_bytes(candidates.iter().map(|(_,_,tiles)|tiles)) as u64;
        {let mut state=self.0.state.lock().unwrap();if state.generation!=generation || state.revision!=revision {return;}state.resident=bytes.min(usize::MAX as u64) as usize;}
        self.0.owner.report(MemoryReport {ram_bytes:bytes,demand_ram_bytes:bytes,..Default::default()});
    }
    pub(super) fn update(&self,candidates:Vec<(u64,u64,RetainedTiles)>,resident:usize,limit:usize) {
        {let mut state=self.0.state.lock().unwrap();state.candidates=candidates;state.resident=resident;state.limit=limit;state.revision=state.revision.wrapping_add(1);}
        let wake=self.0.wake();
        if let Some(wake)=wake {wake();}
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
        let source=StoragePressure::new(owner.clone());
        let queued=Arc::new(AtomicUsize::new(0));let executor=queued.clone();
        let generation=source.subscribe(Arc::new(move || {executor.fetch_add(1,Ordering::Relaxed);}));
        owner.report(MemoryReport {ram_bytes:1<<20,demand_ram_bytes:1<<20,..Default::default()});
        source.update(Vec::new(),1<<20,usize::MAX);
        assert!(source.snapshot(generation).is_none());
        let reads=budget.register_owner(MemoryOwnerKind::Other,MemoryClass::Pinned,None);
        let wakes=Arc::new(AtomicUsize::new(0));let count=wakes.clone();
        reads.set_wake(Arc::new(move || {count.fetch_add(1,Ordering::Relaxed);}));
        for pinned in 1..=100 {
            reads.report(MemoryReport {pinned_ram_bytes:pinned<<20,..Default::default()});
        }
        assert_eq!(wakes.load(Ordering::Relaxed),100);
        assert_eq!(queued.load(Ordering::Relaxed),0);
        source.update(Vec::new(),1<<20,0);
        assert_eq!(queued.load(Ordering::Relaxed),1);
        assert_eq!(source.snapshot(generation).unwrap().1,0);
    }
}
