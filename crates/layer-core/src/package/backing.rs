use std::{ops::{Deref, Range}, sync::{Arc, Mutex, atomic::{AtomicU64, Ordering}}};

pub const MAX_RANGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ByteRange { owner: Arc<[u8]>, range: Range<usize>, lease: Option<Arc<crate::tile_io::Lease>> }
impl ByteRange {
    pub fn new(owner: Arc<[u8]>, range: Range<usize>) -> Result<Self, &'static str> {
        if range.start > range.end || range.end > owner.len() { return Err("Invalid owned byte range"); }
        Ok(Self { owner, range, lease: None })
    }
    pub fn slice(&self, range: Range<usize>) -> Result<Self, &'static str> {
        if range.start > range.end || range.end > self.len() { return Err("Invalid owned byte range"); }
        Ok(Self {owner:self.owner.clone(),range:self.range.start+range.start..self.range.start+range.end,lease:self.lease.clone()})
    }
    pub(crate) fn whole(owner:Arc<[u8]>)->Self {let end=owner.len();Self {owner,range:0..end,lease:None}}
    pub(crate) fn leased(mut self,lease:Arc<crate::tile_io::Lease>)->Self {self.lease=Some(lease);self}
    pub(crate) fn allocation(self)->Arc<[u8]> {self.owner}
    pub(crate) fn unique_allocation(&self)->bool {Arc::strong_count(&self.owner)==1 && Arc::weak_count(&self.owner)==0 && self.lease.is_none()}
    #[cfg(not(target_arch="wasm32"))]
    pub(crate) fn into_arc(self)->Arc<[u8]> {if self.lease.is_none() && self.range==(0..self.owner.len()) {self.owner.clone()}else{Arc::from(&*self)}}
    pub fn retained_bytes(&self) -> usize { self.owner.len() }
}
impl AsRef<[u8]> for ByteRange {fn as_ref(&self)->&[u8] {self}}
impl Deref for ByteRange {
    type Target = [u8];
    fn deref(&self) -> &[u8] { &self.owner[self.range.clone()] }
}

#[derive(Clone, Debug)]
pub enum RangeState { Pending, Ready(ByteRange) }

pub trait ByteSource: Send + Sync {
    fn byte_len(&self) -> u64;
    fn transfer_key(&self)->Option<String> {None}
    fn resident_bytes(&self) -> usize { self.byte_len().min(usize::MAX as u64) as usize }
    fn read_source(&self)->Option<crate::tile_io::ReadSource> {None}
    fn poll(&self, offset: u64, len: usize) -> Result<RangeState, String>;
    fn poll_on_worker(&self, offset:u64, len:usize)->Result<RangeState,String> {self.poll(offset,len)}
    fn wait(&self,offset:u64,len:usize)->super::store_io::FileFuture<'_,ByteRange> {
        Box::pin(async move {match self.poll_on_worker(offset,len)? {RangeState::Ready(bytes)=>Ok(bytes),RangeState::Pending=>Err("Byte source has no read completion".into())}})
    }
}
impl ByteSource for Arc<[u8]> {
    fn byte_len(&self) -> u64 { self.as_ref().len() as u64 }
    fn poll(&self, offset: u64, len: usize) -> Result<RangeState, String> {
        if len > MAX_RANGE_BYTES { return Err("Package read exceeds bounded range".into()); }
        let start = usize::try_from(offset).map_err(|_| "Package offset exceeds address space")?;
        let end = start.checked_add(len).ok_or("Package range overflow")?;
        if self.len() > MAX_RANGE_BYTES {
            let bytes = self.get(start..end).ok_or("Invalid owned byte range")?;
            return Ok(RangeState::Ready(ByteRange::new(Arc::from(bytes), 0..len)?));
        }
        Ok(RangeState::Ready(ByteRange::new(self.clone(), start..end)?))
    }
}

#[derive(Clone)]
pub struct ImmutableBacking { owner: Arc<Owner> }
impl std::fmt::Debug for ImmutableBacking {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImmutableBacking").field("identity", &self.identity()).field("length", &self.byte_len()).finish()
    }
}
struct Owner { generation:crate::PortableId,package:Mutex<super::lease::WeakPackageLease>,identity: u64, length: u64, source: Arc<dyn ByteSource>, failure: Mutex<Option<String>>, integrity_failure: Mutex<Option<String>> }
impl ImmutableBacking {
    pub fn new(source: Arc<dyn ByteSource>) -> Result<Self, &'static str> {
        Self::with_generation(source,crate::PortableId::random())
    }
    pub(crate) fn with_generation(source:Arc<dyn ByteSource>,generation:crate::PortableId)->Result<Self,&'static str> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let identity = NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| "Package owner identity exhausted")?;
        Ok(Self { owner: Arc::new(Owner { generation,package:Default::default(),identity, length: source.byte_len(), source, failure: Mutex::new(None), integrity_failure: Mutex::new(None) }) })
    }
    pub(crate) fn transfer_reference(&self)->Option<super::transfer::BackingReference> {
        Some(super::transfer::BackingReference {key:self.owner.source.transfer_key()?,generation:self.owner.generation,length:self.byte_len()})
    }
    pub fn fail(&self, error: String) -> String {
        match self.owner.integrity_failure.lock() {
            Ok(failure) if error==crate::MEMORY_BUDGET_EXCEEDED => failure.clone().unwrap_or(error),
            Ok(mut failure) => failure.get_or_insert(error).clone(),
            Err(_) => "Package failure lock".into(),
        }
    }
    pub fn identity(&self) -> u64 { self.owner.identity }
    pub(crate) fn package_lease(&self)->super::PackageLease {
        let mut slot=self.owner.package.lock().unwrap();
        if let Some(lease)=slot.upgrade() {return lease;}
        let lease=super::PackageLease::create(self.clone(),self.owner.generation);*slot=lease.downgrade();lease
    }
    pub(crate) fn read_source(&self)->Option<crate::tile_io::ReadSource> {self.owner.source.read_source()}
    pub(crate) fn healthy(&self)->Result<(),String> {
        for failure in [&self.owner.integrity_failure,&self.owner.failure] {
            if let Some(error)=failure.lock().map_err(|_|"Package failure lock")?.as_ref() {return Err(error.clone());}
        }Ok(())
    }
    pub(crate) fn weak_owner(&self)->std::sync::Weak<dyn Send+Sync> {Arc::downgrade(&(self.owner.clone() as Arc<dyn Send+Sync>))}
    pub fn byte_len(&self) -> u64 { self.owner.length }
    pub fn resident_bytes(&self) -> usize { self.owner.source.resident_bytes() }
    pub(crate) fn read_on_worker(&self, offset:u64, len:usize)->Result<ByteRange,String> {
        match self.checked(offset,len,true)? {RangeState::Ready(bytes)=>Ok(bytes),RangeState::Pending=>Err("Package bytes are pending".into())}
    }
    pub fn poll(&self, offset:u64, len:usize)->Result<RangeState,String> {self.checked(offset,len,false)}
    pub async fn wait(&self,offset:u64,len:usize)->Result<ByteRange,String> {
        if len>MAX_RANGE_BYTES || offset.checked_add(len as u64).is_none_or(|end|end>self.byte_len()) {return Err("Package read exceeds immutable range".into());}
        if let Some(error)=self.owner.integrity_failure.lock().map_err(|_|"Package failure lock")?.clone() {return Err(error);}
        if let Some(error)=self.owner.failure.lock().map_err(|_|"Package failure lock")?.clone() {return Err(error);}
        let result=self.owner.source.wait(offset,len).await.and_then(|bytes| {
            if bytes.len()!=len || bytes.retained_bytes()>MAX_RANGE_BYTES {Err("Incomplete or oversized package range".into())}else {Ok(bytes)}
        });
        if let Some(error)=self.owner.integrity_failure.lock().map_err(|_|"Package failure lock")?.clone() {return Err(error);}
        let mut failure=self.owner.failure.lock().map_err(|_|"Package failure lock")?;
        if failure.is_none() && let Err(error)=&result && error!=crate::MEMORY_BUDGET_EXCEEDED {*failure=Some(error.clone());}
        if let Some(error)=&*failure {return Err(error.clone());}result
    }
    fn checked(&self,offset:u64,len:usize,worker:bool)->Result<RangeState,String> {
        if let Some(failure) = self.owner.integrity_failure.lock().map_err(|_| "Package failure lock")?.as_ref() {
            return Err(failure.clone());
        }
        let result = self.access(offset,len,worker);
        if let Some(failure) = self.owner.integrity_failure.lock().map_err(|_| "Package failure lock")?.as_ref() {
            return Err(failure.clone());
        }
        result
    }
    pub(super) fn poll_original_on_worker(&self,offset:u64,len:usize)->Result<RangeState,String> {self.access(offset,len,!cfg!(target_arch="wasm32"))}
    fn access(&self,offset:u64,len:usize,worker:bool)->Result<RangeState,String> {
        if len > MAX_RANGE_BYTES { return Err("Package read exceeds bounded range".into()); }
        let end = offset.checked_add(len as u64).ok_or("Package range overflow")?;
        if end > self.byte_len() { return Err("Package range exceeds immutable backing".into()); }
        if let Some(failure) = self.owner.failure.lock().map_err(|_| "Package failure lock")?.as_ref() {
            return Err(failure.clone());
        }
        let result = (if worker {self.owner.source.poll_on_worker(offset,len)}else{self.owner.source.poll(offset,len)}).and_then(|state| {
            if let RangeState::Ready(bytes) = &state {
                if bytes.len() != len { return Err("Incomplete package range".into()); }
                if bytes.retained_bytes() > MAX_RANGE_BYTES { return Err("Package range retains an oversized buffer".into()); }
            }
            Ok(state)
        });
        let mut failure = self.owner.failure.lock().map_err(|_| "Package failure lock")?;
        if failure.is_none() && let Err(error) = &result && error!=crate::MEMORY_BUDGET_EXCEEDED { *failure = Some(error.clone()); }
        if let Some(failure) = failure.as_ref() { return Err(failure.clone()); }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    #[test]
    fn temporary_memory_refusals_leave_poll_and_wait_backings_healthy() {
        struct Budgeted(AtomicBool);
        impl ByteSource for Budgeted {
            fn byte_len(&self) -> u64 { 4 }
            fn poll(&self, offset: u64, len: usize) -> Result<RangeState, String> {
                if !self.0.load(Ordering::Acquire) { return Err(crate::MEMORY_BUDGET_EXCEEDED.into()); }
                Ok(RangeState::Ready(ByteRange::new(Arc::from([17, 23, 31, 41]), offset as usize..offset as usize + len)?))
            }
        }
        for wait in [false, true] {
            let source = Arc::new(Budgeted(AtomicBool::new(false)));
            let backing = ImmutableBacking::new(source.clone()).unwrap();
            let read = || if wait { pollster::block_on(backing.wait(0, 4)) } else {
                backing.poll(0, 4).map(|state| match state { RangeState::Ready(bytes) => bytes, _ => panic!("ready source") })
            };
            assert_eq!(read().unwrap_err(), crate::MEMORY_BUDGET_EXCEEDED);
            assert!(backing.healthy().is_ok());
            source.0.store(true, Ordering::Release);
            assert_eq!(&*read().unwrap(), &[17, 23, 31, 41]);
            assert_eq!(backing.fail("bad checksum".into()), "bad checksum");
            assert_eq!(backing.fail(crate::MEMORY_BUDGET_EXCEEDED.into()), "bad checksum");
            assert_eq!(read().unwrap_err(), "bad checksum");
        }
    }
    struct Deferred(AtomicBool);
    impl ByteSource for Deferred {
        fn byte_len(&self)->u64 {4}
        fn poll(&self,offset:u64,len:usize)->Result<RangeState,String> {
            if !self.0.load(Ordering::Acquire) {return Ok(RangeState::Pending);}
            Ok(RangeState::Ready(ByteRange::new(Arc::from([17,23,31,41]),offset as usize..offset as usize+len)?))
        }
    }
    #[test]
    fn a_worker_pending_range_does_not_poison_the_backing() {
        let source=Arc::new(Deferred(AtomicBool::new(false)));
        let backing=ImmutableBacking::new(source.clone()).unwrap();
        assert!(matches!(backing.poll(0,4).unwrap(),RangeState::Pending));
        assert!(backing.read_on_worker(0,4).is_err());
        source.0.store(true,Ordering::Release);
        assert_eq!(&*backing.read_on_worker(0,4).unwrap(),&[17,23,31,41]);
    }
}
