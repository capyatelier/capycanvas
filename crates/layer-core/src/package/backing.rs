use std::{ops::{Deref, Range}, sync::{Arc, Mutex, atomic::{AtomicU64, Ordering}}};

pub const MAX_RANGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ByteRange { owner: Arc<[u8]>, range: Range<usize> }
impl ByteRange {
    pub fn new(owner: Arc<[u8]>, range: Range<usize>) -> Result<Self, &'static str> {
        if range.start > range.end || range.end > owner.len() { return Err("Invalid owned byte range"); }
        Ok(Self { owner, range })
    }
    pub fn slice(&self, range: Range<usize>) -> Result<Self, &'static str> {
        if range.start > range.end || range.end > self.len() { return Err("Invalid owned byte range"); }
        Self::new(self.owner.clone(), self.range.start + range.start..self.range.start + range.end)
    }
    pub fn retained_bytes(&self) -> usize { self.owner.len() }
}
impl Deref for ByteRange {
    type Target = [u8];
    fn deref(&self) -> &[u8] { &self.owner[self.range.clone()] }
}

#[derive(Clone, Debug)]
pub enum RangeState { Pending, Ready(ByteRange) }

pub trait ByteSource: Send + Sync {
    fn byte_len(&self) -> u64;
    fn resident_bytes(&self) -> usize { self.byte_len().min(usize::MAX as u64) as usize }
    fn poll(&self, offset: u64, len: usize) -> Result<RangeState, String>;
}
impl ByteSource for Arc<[u8]> {
    fn byte_len(&self) -> u64 { self.as_ref().len() as u64 }
    fn poll(&self, offset: u64, len: usize) -> Result<RangeState, String> {
        let start = usize::try_from(offset).map_err(|_| "Package offset exceeds address space")?;
        let end = start.checked_add(len).ok_or("Package range overflow")?;
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
struct Owner { identity: u64, length: u64, source: Arc<dyn ByteSource>, failure: Mutex<Option<String>> }
impl ImmutableBacking {
    pub fn new(source: Arc<dyn ByteSource>) -> Result<Self, &'static str> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let identity = NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| "Package owner identity exhausted")?;
        Ok(Self { owner: Arc::new(Owner { identity, length: source.byte_len(), source, failure: Mutex::new(None) }) })
    }
    pub fn fail(&self, error: String) -> String {
        match self.owner.failure.lock() {
            Ok(mut failure) => failure.get_or_insert(error).clone(),
            Err(_) => "Package failure lock".into(),
        }
    }
    pub fn identity(&self) -> u64 { self.owner.identity }
    pub fn byte_len(&self) -> u64 { self.owner.length }
    pub fn resident_bytes(&self) -> usize { self.owner.source.resident_bytes() }
    pub fn poll(&self, offset: u64, len: usize) -> Result<RangeState, String> {
        if len > MAX_RANGE_BYTES { return Err("Package read exceeds bounded range".into()); }
        let end = offset.checked_add(len as u64).ok_or("Package range overflow")?;
        if end > self.byte_len() { return Err("Package range exceeds immutable backing".into()); }
        if let Some(failure) = self.owner.failure.lock().map_err(|_| "Package failure lock")?.as_ref() {
            return Err(failure.clone());
        }
        let result = self.owner.source.poll(offset, len).and_then(|state| {
            if let RangeState::Ready(bytes) = &state {
                if bytes.len() != len { return Err("Incomplete package range".into()); }
                if bytes.retained_bytes() > MAX_RANGE_BYTES { return Err("Package range retains an oversized buffer".into()); }
            }
            Ok(state)
        });
        let mut failure = self.owner.failure.lock().map_err(|_| "Package failure lock")?;
        if failure.is_none() && let Err(error) = &result { *failure = Some(error.clone()); }
        if let Some(failure) = failure.as_ref() { return Err(failure.clone()); }
        result
    }
}
