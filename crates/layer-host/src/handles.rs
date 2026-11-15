use std::{any::{Any, TypeId}, cell::RefCell, collections::BTreeMap, fmt,
    ops::{Deref,DerefMut},
    sync::{Arc, Mutex, OnceLock, TryLockError}, thread::{self, ThreadId}};
mod local;
pub use local::{LocalLease,LocalBorrowed};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Handle(u64);
impl Handle {
    pub fn from_raw(raw:u64)->Result<Self,Error> {
        let value=Self(raw);value.parts()?;Ok(value)
    }
    pub fn raw(self)->u64 {self.0}
    fn parts(self)->Result<(usize,u32),Error> {
        let slot=self.0 as u32;let generation=(self.0>>32) as u32;
        if slot==0 || generation==0 || generation>MAX_GENERATION {return Err(Error::Invalid);}
        Ok(((slot-1) as usize,generation))
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {Invalid,WrongType,WrongThread,Busy,Poisoned,Consumed,Exhausted}
impl fmt::Display for Error {
    fn fmt(&self,out:&mut fmt::Formatter<'_>)->fmt::Result {out.write_str(match self {
        Self::Invalid=>"Invalid or expired native handle",Self::WrongType=>"Wrong native handle type",
        Self::WrongThread=>"Native handle belongs to another thread",Self::Busy=>"Native handle is busy",
        Self::Poisoned=>"Native handle operation failed",Self::Consumed=>"Native handle was consumed",
        Self::Exhausted=>"Native handle capacity exhausted",
    })}
}
impl std::error::Error for Error {}
const MAX_GENERATION:u32=i32::MAX as u32;
enum Storage {Mutable(Arc<dyn Any+Send+Sync>),Shared(Arc<dyn Any+Send+Sync>),Local(ThreadId)}
struct Entry {kind:TypeId,storage:Storage}
struct Slot {generation:u32,entry:Option<Entry>}
#[derive(Default)]
struct Table {slots:Vec<Slot>,free:Vec<u32>}
impl Table {
    fn vacant(&mut self)->Result<usize,Error> {
        match self.free.pop() {Some(index)=>Ok(index as usize),None=>{
            if self.slots.len()>=u32::MAX as usize {return Err(Error::Exhausted);}
            self.slots.try_reserve(1).map_err(|_|Error::Exhausted)?;
            let index=self.slots.len();self.slots.push(Slot {generation:1,entry:None});Ok(index)
        }}
    }
    fn entry<T:'static>(&self,handle:Handle)->Result<&Entry,Error> {
        let (index,generation)=handle.parts()?;let slot=self.slots.get(index).ok_or(Error::Invalid)?;
        if slot.generation!=generation {return Err(Error::Invalid);}
        let entry=slot.entry.as_ref().ok_or(Error::Invalid)?;
        if entry.kind!=TypeId::of::<T>() {return Err(Error::WrongType);}
        Ok(entry)
    }
    fn remove<T:'static>(&mut self,handle:Handle)->Result<Entry,Error> {
        self.entry::<T>(handle)?;let (index,_)=handle.parts()?;let slot=&mut self.slots[index];
        let entry=slot.entry.take().ok_or(Error::Invalid)?;
        if slot.generation<MAX_GENERATION {slot.generation+=1;self.free.push(index as u32);}
        Ok(entry)
    }
}
fn table()->&'static Mutex<Table> {static TABLE:OnceLock<Mutex<Table>>=OnceLock::new();TABLE.get_or_init(Default::default)}
fn lock(values:&Mutex<Table>)->Result<std::sync::MutexGuard<'_,Table>,Error> {values.lock().map_err(|_|Error::Poisoned)}
fn lock_table()->Result<std::sync::MutexGuard<'static,Table>,Error> {lock(table())}

fn register(entry:Entry)->Result<Handle,Error> {
    let mut table=lock_table()?;let index=table.vacant()?;let slot=&mut table.slots[index];
    slot.entry=Some(entry);Ok(Handle((u64::from(slot.generation)<<32)|(index as u64+1)))
}

pub struct Lease<T>(Arc<Mutex<Option<Box<T>>>>);
impl<T> Clone for Lease<T> {fn clone(&self)->Self {Self(self.0.clone())}}
impl<T> Lease<T> {
    pub fn with<R>(&self,work:impl FnOnce(&mut T)->R)->Result<R,Error> {
        let mut value=self.0.try_lock().map_err(|error|match error {TryLockError::WouldBlock=>Error::Busy,TryLockError::Poisoned(_)=>Error::Poisoned})?;
        Ok(work(value.as_deref_mut().ok_or(Error::Consumed)?))
    }
    pub fn borrow(&self)->Result<Borrowed<T>,Error> {
        let value=self.0.try_lock().map_err(|error|match error {TryLockError::WouldBlock=>Error::Busy,TryLockError::Poisoned(_)=>Error::Poisoned})?.take().ok_or(Error::Busy)?;
        Ok(Borrowed {value:Some(value),slot:self.0.clone()})
    }
}
pub struct Borrowed<T> {value:Option<Box<T>>,slot:Arc<Mutex<Option<Box<T>>>>}
impl<T> Deref for Borrowed<T> {type Target=T;fn deref(&self)->&T {self.value.as_deref().unwrap()}}
impl<T> DerefMut for Borrowed<T> {fn deref_mut(&mut self)->&mut T {self.value.as_deref_mut().unwrap()}}
impl<T> Drop for Borrowed<T> {
    fn drop(&mut self) {
        if !thread::panicking() && let Ok(mut slot)=self.slot.lock() {*slot=self.value.take();}
    }
}
pub fn insert<T:Send+'static>(value:T)->Result<Handle,Error> {
    let storage=Arc::new(Mutex::new(Some(Box::new(value))));
    register(Entry {kind:TypeId::of::<T>(),storage:Storage::Mutable(storage)})
}
pub fn lease<T:Send+'static>(handle:Handle)->Result<Lease<T>,Error> {
    let storage={let table=lock_table()?;match &table.entry::<T>(handle)?.storage {
        Storage::Mutable(value)=>value.clone(),_=>return Err(Error::WrongType),
    }};
    storage.downcast::<Mutex<Option<Box<T>>>>().map(Lease).map_err(|_|Error::WrongType)
}
pub fn take<T:Send+'static>(handle:Handle)->Result<T,Error> {
    let lease=lease::<T>(handle)?;
    let mut value=lease.0.try_lock().map_err(|error|match error {TryLockError::WouldBlock=>Error::Busy,TryLockError::Poisoned(_)=>Error::Poisoned})?;
    if value.is_none(){return Err(Error::Busy);}
    let removed={lock_table()?.remove::<T>(handle)?};drop(removed);
    value.take().map(|value|*value).ok_or(Error::Consumed)
}
pub fn insert_shared<T:Send+Sync+'static>(value:Arc<T>)->Result<Handle,Error> {
    register(Entry {kind:TypeId::of::<T>(),storage:Storage::Shared(value)})
}
pub fn shared<T:Send+Sync+'static>(handle:Handle)->Result<Arc<T>,Error> {
    let storage={let table=lock_table()?;match &table.entry::<T>(handle)?.storage {
        Storage::Shared(value)=>value.clone(),_=>return Err(Error::WrongType),
    }};
    storage.downcast::<T>().map_err(|_|Error::WrongType)
}
pub fn release<T:'static>(handle:Handle)->Result<(),Error> {
    let removed={let mut table=lock_table()?;
        if matches!(table.entry::<T>(handle)?.storage,Storage::Local(_)) {return Err(Error::WrongThread);}
        table.remove::<T>(handle)?
    };drop(removed);Ok(())
}

pub struct LocalHandles {owner:ThreadId,values:RefCell<BTreeMap<Handle,Box<dyn Any>>>}
impl Default for LocalHandles {fn default()->Self {Self {owner:thread::current().id(),values:RefCell::new(BTreeMap::new())}}}
impl LocalHandles {
    fn owner(&self)->Result<(),Error> {if self.owner==thread::current().id() {Ok(())} else {Err(Error::WrongThread)}}
    pub fn insert<T:'static>(&self,value:T)->Result<Handle,Error> {
        self.owner()?;let mut values=self.values.try_borrow_mut().map_err(|_|Error::Busy)?;
        let handle=register(Entry {kind:TypeId::of::<T>(),storage:Storage::Local(self.owner)})?;
        values.insert(handle,Box::new(value));Ok(handle)
    }
    fn check<T:'static>(&self,handle:Handle)->Result<(),Error> {
        self.owner()?;let table=lock_table()?;
        match table.entry::<T>(handle)?.storage {Storage::Local(owner) if owner==self.owner=>Ok(()),Storage::Local(_)=>Err(Error::WrongThread),_=>Err(Error::WrongType)}
    }
    pub fn with<T:'static,R>(&self,handle:Handle,work:impl FnOnce(&mut T)->R)->Result<R,Error> {
        self.check::<T>(handle)?;
        let mut values=self.values.try_borrow_mut().map_err(|_|Error::Busy)?;
        let value=values.get_mut(&handle).ok_or(Error::Invalid)?.downcast_mut::<T>().ok_or(Error::WrongType)?;
        Ok(work(value))
    }
    pub fn take<T:'static>(&self,handle:Handle)->Result<T,Error> {
        self.check::<T>(handle)?;
        let value={let mut values=self.values.try_borrow_mut().map_err(|_|Error::Busy)?;
            let value=values.get(&handle).ok_or(Error::Invalid)?;
            if !value.is::<T>() {return Err(Error::WrongType);}
            let removed={lock_table()?.remove::<T>(handle)?};drop(removed);
            values.remove(&handle).ok_or(Error::Invalid)?
        };
        value.downcast::<T>().map(|value|*value).map_err(|_|Error::WrongType)
    }
}
impl Drop for LocalHandles {
    fn drop(&mut self) {
        if let Ok(mut table)=lock_table() {for handle in self.values.get_mut().keys() {
            if let Ok((index,generation))=handle.parts() {
                let slot=&mut table.slots[index];
                if slot.generation==generation && slot.entry.as_ref().is_some_and(|entry|matches!(entry.storage,Storage::Local(owner) if owner==self.owner)) {
                    slot.entry=None;
                    if slot.generation<MAX_GENERATION {slot.generation+=1;table.free.push(index as u32);}
                }
            }
        }}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, panic::{catch_unwind, AssertUnwindSafe}, rc::Rc,
        sync::{atomic::{AtomicBool, AtomicUsize, Ordering}, mpsc, Barrier}, time::Duration};

    static SERIAL:Mutex<()>=Mutex::new(());
    fn serial()->std::sync::MutexGuard<'static,()> {SERIAL.lock().unwrap_or_else(|error|error.into_inner())}
    struct Counted(Arc<AtomicUsize>);
    impl Drop for Counted {fn drop(&mut self) {self.0.fetch_add(1,Ordering::SeqCst);}}
    struct LocalCounted(Rc<Cell<usize>>);
    impl Drop for LocalCounted {fn drop(&mut self) {self.0.set(self.0.get()+1);}}
    struct Reenter(Arc<AtomicBool>);
    impl Drop for Reenter {
        fn drop(&mut self) {
            let completed=insert(19u8).and_then(release::<u8>).is_ok();
            self.0.store(completed,Ordering::SeqCst);
        }
    }
    fn entry(value:usize)->Entry {
        Entry {kind:TypeId::of::<usize>(),storage:Storage::Mutable(Arc::new(Mutex::new(Some(Box::new(value)))))}
    }

    #[test]
    fn raw_handles_require_nonzero_slot_and_bounded_generation() {
        let _serial=serial();
        for raw in [0,1,u32::MAX as u64,1u64<<32,(MAX_GENERATION as u64+1)<<32|1,u64::MAX] {
            assert_eq!(Handle::from_raw(raw),Err(Error::Invalid));
        }
        for raw in [1u64<<32|1,1u64<<32|u32::MAX as u64,(MAX_GENERATION as u64)<<32|1] {
            assert_eq!(Handle::from_raw(raw).unwrap().raw(),raw);
        }
        assert_eq!(lease::<usize>(Handle(0)).err(),Some(Error::Invalid));
        assert_eq!(release::<usize>(Handle(u64::MAX)),Err(Error::Invalid));
    }

    #[test]
    fn owned_borrow_excludes_other_access_and_returns_updated_value() {
        let _serial=serial();let handle=insert(41usize).unwrap();let lease=lease::<usize>(handle).unwrap();
        let mut value=lease.borrow().unwrap();*value+=1;
        assert_eq!(lease.borrow().err(),Some(Error::Busy));
        assert_eq!(take::<usize>(handle),Err(Error::Busy));
        assert_eq!(lease.with(|value|*value),Err(Error::Consumed));
        drop(value);assert_eq!(take::<usize>(handle),Ok(42));
    }

    #[test]
    fn owned_calls_keep_large_host_state_at_its_original_address() {
        let _serial=serial();let handle=insert([19u8;65536]).unwrap();let lease=lease::<[u8;65536]>(handle).unwrap();
        let address=lease.with(|value|value as *const [u8;65536]).unwrap();
        for _ in 0..4 {
            let mut value=lease.borrow().unwrap();
            assert!(std::ptr::eq(address,&*value));value[0]+=1;
        }
        assert_eq!(take::<[u8;65536]>(handle).unwrap()[0],23);
    }

    #[test]
    fn release_during_owned_borrow_retires_token_before_dropping_value() {
        let _serial=serial();let drops=Arc::new(AtomicUsize::new(0));let handle=insert(Counted(drops.clone())).unwrap();
        let lease=lease::<Counted>(handle).unwrap();let value=lease.borrow().unwrap();
        release::<Counted>(handle).unwrap();assert_eq!(drops.load(Ordering::SeqCst),0);
        assert_eq!(super::lease::<Counted>(handle).err(),Some(Error::Invalid));
        drop(lease);drop(value);assert_eq!(drops.load(Ordering::SeqCst),1);
    }

    #[test]
    fn owned_borrow_unwind_never_republishes_partial_task() {
        let _serial=serial();let drops=Arc::new(AtomicUsize::new(0));let handle=insert(Counted(drops.clone())).unwrap();
        let lease=lease::<Counted>(handle).unwrap();
        assert!(catch_unwind(AssertUnwindSafe(||{let _value=lease.borrow().unwrap();panic!("partial task");})).is_err());
        assert_eq!(drops.load(Ordering::SeqCst),1);assert_eq!(lease.borrow().err(),Some(Error::Busy));
        release::<Counted>(handle).unwrap();drop(lease);assert_eq!(drops.load(Ordering::SeqCst),1);
    }

    #[test]
    fn wrong_type_operations_preserve_mutable_values() {
        let _serial=serial();
        let handle=insert(41usize).unwrap();
        assert_eq!(lease::<u8>(handle).err(),Some(Error::WrongType));
        assert_eq!(shared::<usize>(handle).err(),Some(Error::WrongType));
        assert_eq!(take::<u8>(handle),Err(Error::WrongType));
        assert_eq!(release::<u8>(handle),Err(Error::WrongType));
        assert_eq!(lease::<usize>(handle).unwrap().with(|value|*value),Ok(41));
        assert_eq!(take::<usize>(handle),Ok(41));
        assert_eq!(take::<usize>(handle),Err(Error::Invalid));
        assert_eq!(release::<usize>(handle),Err(Error::Invalid));
    }

    #[test]
    fn wrong_storage_and_wrong_type_preserve_shared_values() {
        let _serial=serial();
        let value=Arc::new(73usize);
        let handle=insert_shared(value.clone()).unwrap();
        assert_eq!(lease::<usize>(handle).err(),Some(Error::WrongType));
        assert_eq!(take::<usize>(handle),Err(Error::WrongType));
        assert_eq!(shared::<u8>(handle).err(),Some(Error::WrongType));
        assert_eq!(release::<u8>(handle),Err(Error::WrongType));
        assert!(Arc::ptr_eq(&shared::<usize>(handle).unwrap(),&value));
        release::<usize>(handle).unwrap();
        assert_eq!(shared::<usize>(handle).err(),Some(Error::Invalid));
        assert_eq!(*value,73);
    }

    #[test]
    fn removing_a_slot_reuses_it_with_a_new_generation() {
        let _serial=serial();
        let mut values=Table::default();
        let index=values.vacant().unwrap();
        values.slots[index].entry=Some(entry(3));
        let old=Handle((1u64<<32)|(index as u64+1));
        assert_eq!(values.remove::<u8>(old).err(),Some(Error::WrongType));
        assert!(values.free.is_empty());
        drop(values.remove::<usize>(old).unwrap());
        assert_eq!(values.entry::<usize>(old).err(),Some(Error::Invalid));
        let reused=values.vacant().unwrap();
        assert_eq!(reused,index);
        assert_eq!(values.slots[reused].generation,2);
        values.slots[reused].entry=Some(entry(5));
        let current=Handle((2u64<<32)|(reused as u64+1));
        assert!(values.entry::<usize>(current).is_ok());
        assert_eq!(values.entry::<usize>(old).err(),Some(Error::Invalid));
        assert_eq!(values.entry::<usize>(Handle(1u64<<32|u32::MAX as u64)).err(),Some(Error::Invalid));
    }

    #[test]
    fn maximum_generation_retires_a_slot_instead_of_wrapping() {
        let _serial=serial();
        let mut values=Table::default();
        let index=values.vacant().unwrap();
        values.slots[index].generation=MAX_GENERATION-1;
        values.slots[index].entry=Some(entry(1));
        let before=Handle((u64::from(MAX_GENERATION-1)<<32)|(index as u64+1));
        drop(values.remove::<usize>(before).unwrap());
        assert_eq!(values.vacant(),Ok(index));
        values.slots[index].entry=Some(entry(2));
        let last=Handle((u64::from(MAX_GENERATION)<<32)|(index as u64+1));
        assert!(Handle::from_raw(last.raw()).is_ok());
        drop(values.remove::<usize>(last).unwrap());
        assert!(values.free.is_empty());
        assert_eq!(values.vacant(),Ok(index+1));
        assert_eq!(values.slots[index].generation,MAX_GENERATION);
        assert_eq!(values.entry::<usize>(last).err(),Some(Error::Invalid));
    }

    #[test]
    fn take_transfers_one_owner_and_queued_leases_observe_consumption() {
        let _serial=serial();
        let drops=Arc::new(AtomicUsize::new(0));
        let handle=insert(Counted(drops.clone())).unwrap();
        let queued=lease::<Counted>(handle).unwrap();
        let second=queued.clone();
        let value=take::<Counted>(handle).unwrap();
        assert_eq!(drops.load(Ordering::SeqCst),0);
        assert_eq!(queued.with(|_|()).err(),Some(Error::Consumed));
        assert_eq!(second.with(|_|()).err(),Some(Error::Consumed));
        assert_eq!(take::<Counted>(handle).err(),Some(Error::Invalid));
        assert_eq!(release::<Counted>(handle),Err(Error::Invalid));
        drop(value);drop(queued);drop(second);
        assert_eq!(drops.load(Ordering::SeqCst),1);
    }

    #[test]
    fn releasing_a_handle_preserves_an_active_lease_until_work_finishes() {
        let _serial=serial();
        let drops=Arc::new(AtomicUsize::new(0));
        let handle=insert(Counted(drops.clone())).unwrap();
        let active=lease::<Counted>(handle).unwrap();
        let entered=Arc::new(Barrier::new(2));let proceed=Arc::new(Barrier::new(2));
        let worker={let entered=entered.clone();let proceed=proceed.clone();thread::spawn(move|| {
            active.with(|value| {entered.wait();proceed.wait();assert_eq!(value.0.load(Ordering::SeqCst),0)}).unwrap();
        })};
        entered.wait();
        release::<Counted>(handle).unwrap();
        assert_eq!(super::lease::<Counted>(handle).err(),Some(Error::Invalid));
        assert_eq!(release::<Counted>(handle),Err(Error::Invalid));
        assert_eq!(drops.load(Ordering::SeqCst),0);
        proceed.wait();worker.join().unwrap();
        assert_eq!(drops.load(Ordering::SeqCst),1);
    }

    #[test]
    fn queued_cross_thread_lease_observes_a_completed_take() {
        let _serial=serial();
        let handle=insert(31usize).unwrap();
        let queued=lease::<usize>(handle).unwrap();
        let ready=Arc::new(Barrier::new(2));let proceed=Arc::new(Barrier::new(2));
        let worker={let ready=ready.clone();let proceed=proceed.clone();thread::spawn(move|| {
            ready.wait();proceed.wait();queued.with(|value|*value)
        })};
        ready.wait();assert_eq!(take::<usize>(handle),Ok(31));proceed.wait();
        assert_eq!(worker.join().unwrap(),Err(Error::Consumed));
    }

    #[test]
    fn mutable_reentry_and_concurrent_operations_return_busy_without_waiting() {
        let _serial=serial();
        let handle=insert(17usize).unwrap();
        let active=lease::<usize>(handle).unwrap();
        let probe=active.clone();
        let entered=Arc::new(Barrier::new(2));let proceed=Arc::new(Barrier::new(2));
        let worker={let entered=entered.clone();let proceed=proceed.clone();thread::spawn(move|| {
            active.with(|_| {entered.wait();proceed.wait()}).unwrap();
        })};
        entered.wait();
        let (send,receive)=mpsc::channel();
        let checking=thread::spawn(move||send.send((probe.with(|_|()).err(),take::<usize>(handle).err())).unwrap());
        let result=receive.recv_timeout(Duration::from_secs(5));
        proceed.wait();worker.join().unwrap();checking.join().unwrap();
        assert_eq!(result.unwrap(),(Some(Error::Busy),Some(Error::Busy)));
        let outer=lease::<usize>(handle).unwrap();
        outer.with(|_|assert_eq!(outer.with(|_|()).err(),Some(Error::Busy))).unwrap();
        assert_eq!(take::<usize>(handle),Ok(17));
    }

    #[test]
    fn shared_cancellation_stays_available_during_mutable_work() {
        let _serial=serial();
        let handle=insert(0usize).unwrap();
        let cancellation=insert_shared(Arc::new(AtomicBool::new(false))).unwrap();
        let work=lease::<usize>(handle).unwrap();
        let signal=shared::<AtomicBool>(cancellation).unwrap();
        let entered=Arc::new(Barrier::new(2));let proceed=Arc::new(Barrier::new(2));
        let worker={let entered=entered.clone();let proceed=proceed.clone();thread::spawn(move|| {
            work.with(|value| {entered.wait();proceed.wait();*value=usize::from(signal.load(Ordering::SeqCst))}).unwrap();
        })};
        entered.wait();shared::<AtomicBool>(cancellation).unwrap().store(true,Ordering::SeqCst);
        release::<AtomicBool>(cancellation).unwrap();proceed.wait();worker.join().unwrap();
        assert_eq!(take::<usize>(handle),Ok(1));
        assert_eq!(shared::<AtomicBool>(cancellation).err(),Some(Error::Invalid));
    }

    #[test]
    fn racing_takes_transfer_the_value_exactly_once() {
        let _serial=serial();
        let drops=Arc::new(AtomicUsize::new(0));
        let handle=insert(Counted(drops.clone())).unwrap();
        let ready=Arc::new(Barrier::new(3));
        let workers=(0..2).map(|_| {let ready=ready.clone();thread::spawn(move|| {
            ready.wait();take::<Counted>(handle)
        })}).collect::<Vec<_>>();
        ready.wait();
        let results=workers.into_iter().map(|worker|worker.join().unwrap()).collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|value|value.is_ok()).count(),1);
        assert!(results.iter().filter_map(|value|value.as_ref().err()).all(|error|matches!(error,Error::Invalid|Error::Busy|Error::Consumed)));
        drop(results);assert_eq!(drops.load(Ordering::SeqCst),1);
    }

    #[test]
    fn work_panics_poison_only_the_value_and_release_remains_available() {
        let _serial=serial();
        let drops=Arc::new(AtomicUsize::new(0));
        let handle=insert(Counted(drops.clone())).unwrap();
        let active=lease::<Counted>(handle).unwrap();
        assert!(catch_unwind(AssertUnwindSafe(||active.with(|_|panic!("value panic")))).is_err());
        assert_eq!(active.with(|_|()).err(),Some(Error::Poisoned));
        assert_eq!(take::<Counted>(handle).err(),Some(Error::Poisoned));
        let independent=insert(7u8).unwrap();assert_eq!(take::<u8>(independent),Ok(7));
        release::<Counted>(handle).unwrap();drop(active);
        assert_eq!(drops.load(Ordering::SeqCst),1);
    }

    #[test]
    fn registry_poison_refuses_access_without_dropping_live_values() {
        let _serial=serial();
        let drops=Arc::new(AtomicUsize::new(0));let values=Mutex::new(Table::default());
        {let mut table=lock(&values).unwrap();let index=table.vacant().unwrap();
            table.slots[index].entry=Some(Entry {kind:TypeId::of::<Counted>(),storage:Storage::Mutable(
                Arc::new(Mutex::new(Some(Box::new(Counted(drops.clone()))))))});}
        assert!(catch_unwind(|| {let _guard=values.lock().unwrap();panic!("table panic")}).is_err());
        assert_eq!(lock(&values).err(),Some(Error::Poisoned));
        assert_eq!(drops.load(Ordering::SeqCst),0);
        drop(values);assert_eq!(drops.load(Ordering::SeqCst),1);
    }

    #[test]
    fn value_destructors_reenter_after_the_global_lock_is_released() {
        let _serial=serial();
        let released=Arc::new(AtomicBool::new(false));
        release::<Reenter>(insert(Reenter(released.clone())).unwrap()).unwrap();
        assert!(released.load(Ordering::SeqCst));
        let shared_drop=Arc::new(AtomicBool::new(false));
        release::<Reenter>(insert_shared(Arc::new(Reenter(shared_drop.clone()))).unwrap()).unwrap();
        assert!(shared_drop.load(Ordering::SeqCst));
        let taken=Arc::new(AtomicBool::new(false));
        drop(take::<Reenter>(insert(Reenter(taken.clone())).unwrap()).unwrap());
        assert!(taken.load(Ordering::SeqCst));
    }

    #[test]
    fn local_rc_values_support_mutation_take_and_wrong_type_preservation() {
        let _serial=serial();
        let local=LocalHandles::default();let value=Rc::new(Cell::new(5usize));
        let handle=local.insert(value.clone()).unwrap();
        assert_eq!(local.take::<u8>(handle),Err(Error::WrongType));
        assert_eq!(release::<u8>(handle),Err(Error::WrongType));
        assert_eq!(release::<Rc<Cell<usize>>>(handle),Err(Error::WrongThread));
        local.with::<Rc<Cell<usize>>,_>(handle,|value|value.set(8)).unwrap();
        let taken=local.take::<Rc<Cell<usize>>>(handle).unwrap();
        assert!(Rc::ptr_eq(&value,&taken));assert_eq!(value.get(),8);
        assert_eq!(local.with::<Rc<Cell<usize>>,_>(handle,|_|()),Err(Error::Invalid));
    }

    #[test]
    fn local_reentry_returns_busy_and_unwind_releases_the_borrow() {
        let _serial=serial();
        let local=LocalHandles::default();let first=local.insert(3usize).unwrap();let second=local.insert(4usize).unwrap();
        local.with::<usize,_>(first,|_| {
            assert_eq!(local.with::<usize,_>(second,|_|()),Err(Error::Busy));
            assert_eq!(local.take::<usize>(second),Err(Error::Busy));
            assert_eq!(local.insert(9u8),Err(Error::Busy));
        }).unwrap();
        assert!(catch_unwind(AssertUnwindSafe(||local.with::<usize,_>(first,|_|panic!("local panic")))).is_err());
        assert_eq!(local.with::<usize,_>(first,|value|*value),Ok(3));
        assert_eq!(local.take::<usize>(first),Ok(3));assert_eq!(local.take::<usize>(second),Ok(4));
    }

    #[test]
    fn another_threads_local_registry_cannot_access_the_owner_value() {
        let _serial=serial();
        let local=LocalHandles::default();let handle=local.insert(Rc::new(Cell::new(9usize))).unwrap();
        thread::spawn(move|| {
            let other=LocalHandles::default();
            assert_eq!(other.with::<Rc<Cell<usize>>,_>(handle,|_|()),Err(Error::WrongThread));
            assert_eq!(other.take::<Rc<Cell<usize>>>(handle).err(),Some(Error::WrongThread));
            assert_eq!(release::<Rc<Cell<usize>>>(handle),Err(Error::WrongThread));
        }).join().unwrap();
        assert_eq!(local.with::<Rc<Cell<usize>>,_>(handle,|value|value.get()),Ok(9));
        drop(local.take::<Rc<Cell<usize>>>(handle).unwrap());
    }

    #[test]
    fn separate_local_registries_on_one_thread_preserve_each_others_values() {
        let _serial=serial();
        let first=LocalHandles::default();let second=LocalHandles::default();let handle=first.insert(13usize).unwrap();
        assert_eq!(second.with::<usize,_>(handle,|_|()),Err(Error::Invalid));
        assert_eq!(second.take::<usize>(handle),Err(Error::Invalid));
        drop(second);assert_eq!(first.take::<usize>(handle),Ok(13));
    }

    #[test]
    fn dropping_a_local_registry_invalidates_handles_and_drops_rc_values_once() {
        let _serial=serial();
        let drops=Rc::new(Cell::new(0));let first=LocalHandles::default();let second=LocalHandles::default();
        let expired=first.insert(LocalCounted(drops.clone())).unwrap();let live=second.insert(27usize).unwrap();
        drop(first);assert_eq!(drops.get(),1);
        assert_eq!(release::<LocalCounted>(expired),Err(Error::Invalid));
        assert_eq!(second.with::<LocalCounted,_>(expired,|_|()),Err(Error::Invalid));
        assert_eq!(second.take::<usize>(live),Ok(27));
    }

    #[test]
    fn local_destructors_reenter_after_registry_drop_and_take() {
        let _serial=serial();
        let released=Arc::new(AtomicBool::new(false));let local=LocalHandles::default();
        local.insert(Reenter(released.clone())).unwrap();drop(local);
        assert!(released.load(Ordering::SeqCst));
        let taken=Arc::new(AtomicBool::new(false));let local=LocalHandles::default();
        let handle=local.insert(Reenter(taken.clone())).unwrap();drop(local.take::<Reenter>(handle).unwrap());
        assert!(taken.load(Ordering::SeqCst));
    }

    #[test]
    fn a_take_racing_release_drops_the_value_once() {
        let _serial=serial();
        let drops=Arc::new(AtomicUsize::new(0));let handle=insert(Counted(drops.clone())).unwrap();
        let ready=Arc::new(Barrier::new(3));
        let taking={let ready=ready.clone();thread::spawn(move|| {ready.wait();take::<Counted>(handle)})};
        let releasing={let ready=ready.clone();thread::spawn(move|| {ready.wait();release::<Counted>(handle)})};
        ready.wait();let taken=taking.join().unwrap();let released=releasing.join().unwrap();
        assert_eq!(usize::from(taken.is_ok())+usize::from(released.is_ok()),1);
        if taken.is_ok() {assert_eq!(released,Err(Error::Invalid));} else {assert_eq!(taken.as_ref().err().copied(),Some(Error::Invalid));}
        drop(taken);assert_eq!(drops.load(Ordering::SeqCst),1);
    }

    #[test]
    fn shared_clones_drop_once_after_handle_release() {
        let _serial=serial();
        let drops=Arc::new(AtomicUsize::new(0));let handle=insert_shared(Arc::new(Counted(drops.clone()))).unwrap();
        let retained=shared::<Counted>(handle).unwrap();let second=retained.clone();
        assert_eq!(release::<u8>(handle),Err(Error::WrongType));
        release::<Counted>(handle).unwrap();assert_eq!(drops.load(Ordering::SeqCst),0);
        drop(retained);assert_eq!(drops.load(Ordering::SeqCst),0);
        drop(second);assert_eq!(drops.load(Ordering::SeqCst),1);
    }

    #[test]
    fn panicking_destructors_do_not_poison_the_global_registry() {
        let _serial=serial();
        struct Panics(Arc<AtomicUsize>);
        impl Drop for Panics {fn drop(&mut self) {self.0.fetch_add(1,Ordering::SeqCst);panic!("destructor panic");}}
        let drops=Arc::new(AtomicUsize::new(0));let handle=insert(Panics(drops.clone())).unwrap();
        assert!(catch_unwind(AssertUnwindSafe(||release::<Panics>(handle))).is_err());
        assert_eq!(drops.load(Ordering::SeqCst),1);assert_eq!(release::<Panics>(handle),Err(Error::Invalid));
        assert_eq!(take::<usize>(insert(42usize).unwrap()),Ok(42));
    }

    #[test]
    fn local_drop_invalidates_every_handle_before_a_destructor_panics() {
        let _serial=serial();
        struct Panics;impl Drop for Panics {fn drop(&mut self) {panic!("local destructor panic");}}
        let local=LocalHandles::default();let first=local.insert(Panics).unwrap();let second=local.insert(5u8).unwrap();
        assert!(catch_unwind(AssertUnwindSafe(||drop(local))).is_err());
        assert_eq!(release::<Panics>(first),Err(Error::Invalid));assert_eq!(release::<u8>(second),Err(Error::Invalid));
        assert_eq!(take::<usize>(insert(43usize).unwrap()),Ok(43));
    }
}
