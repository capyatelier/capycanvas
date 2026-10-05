use super::PortableId;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::{fmt, hash::{Hash, Hasher}, ops::Deref, sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, Ordering}}};

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ResourceEncoding { Raw, Lz4 }
#[derive(Clone, Debug)]
pub struct EncodedBytes { pub encoding: ResourceEncoding, pub bytes: Arc<[u8]> }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EncodedIntegrity { pub bytes: u64, pub crc32: u32 }
impl EncodedIntegrity {
    pub(crate) fn calculate(bytes:&[u8],cancelled:&AtomicBool)->Result<Self,String> {
        let mut crc=crc32fast::Hasher::new();
        for chunk in bytes.chunks(crate::package::MAX_RANGE_BYTES) {
            if cancelled.load(Ordering::Relaxed) { return Err("Package operation cancelled".into()); }
            crc.update(chunk);
        }
        if cancelled.load(Ordering::Relaxed) { return Err("Package operation cancelled".into()); }
        Ok(Self {bytes:bytes.len() as u64,crc32:crc.finalize()})
    }
}
#[derive(Default)]
pub(crate) struct IntegrityCache { value: OnceLock<EncodedIntegrity>, digest: OnceLock<[u8;32]>, initializing: Mutex<()> }
impl IntegrityCache {
    pub(crate) fn get(&self) -> Option<EncodedIntegrity> { self.value.get().copied() }
    pub(crate) fn seed(&self,value:EncodedIntegrity)->Result<(),String> {
        let _guard=self.initializing.lock().map_err(|_|"Resource integrity lock failed")?;
        if let Some(previous)=self.get() {return if previous==value {Ok(())}else{Err("Conflicting immutable resource integrity".into())};}
        self.value.set(value).map_err(|_|"Resource integrity already initialized".into())
    }
    fn initialized<T:Copy>(&self,slot:&OnceLock<T>,cancelled:&AtomicBool,compute:impl FnOnce()->Result<T,String>)->Result<T,String> {
        if cancelled.load(Ordering::Relaxed) { return Err("Package operation cancelled".into()); }
        if let Some(value)=slot.get() { return Ok(*value); }
        let _guard=self.initializing.lock().map_err(|_|"Resource integrity lock failed")?;
        if cancelled.load(Ordering::Relaxed) { return Err("Package operation cancelled".into()); }
        if let Some(value)=slot.get() { return Ok(*value); }
        let value=compute()?;
        slot.set(value).map_err(|_|"Resource integrity already initialized")?;
        Ok(value)
    }
    pub(crate) fn get_or_compute(&self,cancelled:&AtomicBool,bytes:impl FnOnce()->Result<Arc<[u8]>,String>)->Result<EncodedIntegrity,String> {
        self.initialized(&self.value,cancelled,||EncodedIntegrity::calculate(&bytes()?,cancelled))
    }
    pub(crate) fn digest(&self,cancelled:&AtomicBool,bytes:impl FnOnce()->Result<Arc<[u8]>,String>)->Result<[u8;32],String> {
        self.initialized(&self.digest,cancelled,|| {
            let bytes=bytes()?;let mut hash=Sha256::new();
            for chunk in bytes.chunks(crate::package::MAX_RANGE_BYTES) {
                if cancelled.load(Ordering::Relaxed) { return Err("Package operation cancelled".into()); }
                hash.update(chunk);
            }
            if cancelled.load(Ordering::Relaxed) { return Err("Package operation cancelled".into()); }
            Ok(hash.finalize().into())
        })
    }
}
struct Owner<T: ?Sized> { value: Arc<T>, encoded: OnceLock<EncodedBytes>, integrity: IntegrityCache }
pub struct Resource<T: ?Sized> { id: PortableId, owner: Arc<Owner<T>> }
impl<T: ?Sized> Resource<T> {
    pub fn new(value: Arc<T>) -> Self { Self::with_id(PortableId::random(), value) }
    pub fn with_id(id: PortableId, value: Arc<T>) -> Self {
        Self { id, owner: Arc::new(Owner { value, encoded: OnceLock::new(), integrity: IntegrityCache::default() }) }
    }
    pub fn with_id_and_encoded(id: PortableId, value: Arc<T>, encoded: EncodedBytes) -> Self {
        Self { id, owner: Arc::new(Owner { value, encoded: OnceLock::from(encoded), integrity: IntegrityCache::default() }) }
    }
    pub fn id(&self) -> PortableId { self.id }
    pub(crate) fn alias(&self, id:PortableId) -> Self { Self {id,owner:self.owner.clone()} }
    pub(crate) fn encoded_integrity(&self, cancelled:&AtomicBool, bytes:impl FnOnce()->Result<Arc<[u8]>,String>) -> Result<EncodedIntegrity,String> {
        self.owner.integrity.get_or_compute(cancelled,bytes)
    }
    pub(crate) fn set_encoded_integrity(&self,value:EncodedIntegrity)->Result<(),String> { self.owner.integrity.seed(value) }
    pub(crate) fn encoded_integrity_if_ready(&self)->Option<EncodedIntegrity> { self.owner.integrity.get() }
    pub(crate) fn encoded_digest(&self,cancelled:&AtomicBool,bytes:impl FnOnce()->Result<Arc<[u8]>,String>)->Result<[u8;32],String> {
        self.owner.integrity.digest(cancelled,bytes)
    }
    pub fn storage(&self) -> &Arc<T> { &self.owner.value }
    pub fn same_owner(&self, other: &Self) -> bool { Arc::ptr_eq(&self.owner, &other.owner) }
    pub(crate) fn owner_identity(&self)->usize {Arc::as_ptr(&self.owner) as usize}
    pub(crate) fn owner_metadata_bytes(&self)->usize {std::mem::size_of::<Owner<T>>()}
    pub fn encoded_if_ready(&self) -> Option<&EncodedBytes> { self.owner.encoded.get() }
    pub fn encoded(&self, encode: impl FnOnce(&T) -> EncodedBytes) -> &EncodedBytes {
        self.owner.encoded.get_or_init(|| encode(&self.owner.value))
    }
}
impl<T: ?Sized> Clone for Resource<T> {
    fn clone(&self) -> Self { Self { id:self.id, owner: self.owner.clone() } }
}
impl<T: ?Sized> Deref for Resource<T> {
    type Target = T;
    fn deref(&self) -> &T { &self.owner.value }
}
impl<T: ?Sized> AsRef<T> for Resource<T> {
    fn as_ref(&self) -> &T { &self.owner.value }
}
impl<T: ?Sized> std::borrow::Borrow<T> for Resource<T> {
    fn borrow(&self) -> &T { &self.owner.value }
}
impl<T: ?Sized + PartialEq> PartialEq for Resource<T> {
    fn eq(&self, other: &Self) -> bool { self.owner.value == other.owner.value }
}
impl<T: ?Sized + Eq> Eq for Resource<T> {}
impl<T: ?Sized + Hash> Hash for Resource<T> {
    fn hash<H: Hasher>(&self, state: &mut H) { self.owner.value.hash(state); }
}
impl<T: ?Sized + fmt::Debug> fmt::Debug for Resource<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Resource").field("id", &self.id).field("value", &self.owner.value).finish()
    }
}
impl<T: ?Sized + Serialize> Serialize for Resource<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> { self.owner.value.serialize(serializer) }
}
impl<'de, T: ?Sized> Deserialize<'de> for Resource<T> where Arc<T>: Deserialize<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Arc::deserialize(deserializer).map(Self::new)
    }
}
impl<T: ?Sized> From<Arc<T>> for Resource<T> {
    fn from(value: Arc<T>) -> Self { Self::new(value) }
}
impl<T> From<Vec<T>> for Resource<[T]> {
    fn from(value: Vec<T>) -> Self { Self::new(value.into()) }
}
impl<T: Clone> From<&[T]> for Resource<[T]> {
    fn from(value: &[T]) -> Self { Self::new(value.into()) }
}
impl From<String> for Resource<str> {
    fn from(value: String) -> Self { Self::new(value.into()) }
}
impl From<&str> for Resource<str> {
    fn from(value: &str) -> Self { Self::new(value.into()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_resource_identity_and_lifetime_survive_capture() {
        let resource = Resource::<[u8]>::from(vec![0, 17, 255]);
        let id = resource.id();
        let bytes = Arc::downgrade(resource.storage());
        let capture = resource.clone();
        drop(resource);
        assert_eq!(capture.id(), id);
        assert_eq!(&*capture, &[0, 17, 255]);
        assert!(bytes.upgrade().is_some());
        let independent = Resource::<[u8]>::from(capture.to_vec());
        assert_eq!(capture, independent);
        assert_ne!(capture.id(), independent.id());
        drop(capture);
        assert!(bytes.upgrade().is_none());
    }
    #[test]
    fn encoding_is_shared_and_saved_compressed_bytes_are_reused_exactly() {
        let resource = Resource::<[u8]>::from(vec![6; 32]);
        let stored: Arc<[u8]> = vec![1, 2, 3, 4].into();
        let encoded = resource.encoded(|_| EncodedBytes { encoding: ResourceEncoding::Lz4, bytes: stored.clone() });
        assert!(Arc::ptr_eq(&encoded.bytes, &stored));
        let capture = resource.clone();
        assert!(resource.same_owner(&capture));
        assert!(Arc::ptr_eq(&capture.encoded(|_| panic!("repeated encoding")).bytes, &stored));
        let loaded = Resource::with_id_and_encoded(resource.id(), resource.storage().clone(), encoded.clone());
        assert!(!loaded.same_owner(&resource));
        assert_eq!(loaded.id(), resource.id());
        assert!(Arc::ptr_eq(&loaded.encoded(|_| panic!("loaded bytes recompressed")).bytes, &stored));
    }
    #[test]
    fn encoded_integrity_is_lazy_shared_by_aliases_and_released_with_the_owner() {
        let cancelled=AtomicBool::new(false);
        let bytes:Arc<[u8]>=Arc::from([7,3,19,255]);let weak=Arc::downgrade(&bytes);
        let resource=Resource::<[u8]>::new(bytes.clone());let alias=resource.alias(PortableId::random());
        assert_ne!(resource.id(),alias.id());assert!(resource.same_owner(&alias));
        assert!(resource.encoded_integrity_if_ready().is_none());
        let crc=resource.encoded_integrity(&cancelled,||Ok(bytes.clone())).unwrap();
        assert_eq!(crc,EncodedIntegrity {bytes:4,crc32:crc32fast::hash(&bytes)});
        assert_eq!(alias.encoded_integrity(&cancelled,||panic!("repeated integrity read")).unwrap(),crc);
        let digest=resource.encoded_digest(&cancelled,||Ok(bytes.clone())).unwrap();
        assert_eq!(digest,<[u8;32]>::from(Sha256::digest(&bytes)));
        assert_eq!(alias.encoded_digest(&cancelled,||panic!("repeated digest read")).unwrap(),digest);
        let edited=Resource::<[u8]>::new(Arc::from([7,3,19,254]));
        assert!(edited.encoded_integrity_if_ready().is_none());assert!(!edited.same_owner(&resource));
        drop(resource);drop(bytes);assert!(weak.upgrade().is_some());
        drop(alias);assert!(weak.upgrade().is_none());
    }
    #[test]
    fn failed_and_cancelled_integrity_computation_can_retry_without_poisoning() {
        let cancelled=AtomicBool::new(false);let cache=IntegrityCache::default();let bytes:Arc<[u8]>=Arc::from([13,17]);
        assert!(cache.get_or_compute(&cancelled,||Err("cold backing".into())).is_err());assert!(cache.get().is_none());
        assert!(cache.get_or_compute(&cancelled,||{cancelled.store(true,Ordering::Relaxed);Ok(bytes.clone())}).is_err());assert!(cache.get().is_none());
        cancelled.store(false,Ordering::Relaxed);
        let expected=cache.get_or_compute(&cancelled,||Ok(bytes.clone())).unwrap();
        assert!(cache.digest(&cancelled,||{cancelled.store(true,Ordering::Relaxed);Ok(bytes.clone())}).is_err());
        cancelled.store(false,Ordering::Relaxed);
        let digest=cache.digest(&cancelled,||Ok(bytes.clone())).unwrap();
        assert_eq!(digest,<[u8;32]>::from(Sha256::digest(&bytes)));
        cancelled.store(true,Ordering::Relaxed);
        assert!(cache.get_or_compute(&cancelled,||panic!("cancelled read")).is_err());
        assert!(cache.digest(&cancelled,||panic!("cancelled read")).is_err());
        cancelled.store(false,Ordering::Relaxed);
        assert_eq!(cache.get_or_compute(&cancelled,||panic!("repeated integrity read")).unwrap(),expected);
        assert_eq!(cache.digest(&cancelled,||panic!("repeated digest read")).unwrap(),digest);
    }
    #[test]
    fn resource_ids_are_explicit_and_legacy_serde_remains_value_only() {
        let id = PortableId::from_bytes([9; 16]);
        let resource = Resource::<str>::with_id(id, "exact shader\n".into());
        assert_eq!(resource.id(), id);
        let json = serde_json::to_string(&resource).unwrap();
        assert_eq!(json, serde_json::to_string("exact shader\n").unwrap());
        let reopened: Resource<str> = serde_json::from_str(&json).unwrap();
        assert_eq!(reopened, resource);
        assert_ne!(reopened.id(), id);
    }
}
