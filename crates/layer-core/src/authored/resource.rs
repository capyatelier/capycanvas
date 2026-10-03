use super::PortableId;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, hash::{Hash, Hasher}, ops::Deref, sync::{Arc, OnceLock}};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceEncoding { Raw, Lz4 }
#[derive(Clone, Debug)]
pub struct EncodedBytes { pub encoding: ResourceEncoding, pub bytes: Arc<[u8]> }
struct Owner<T: ?Sized> { id: PortableId, value: Arc<T>, encoded: OnceLock<EncodedBytes> }
pub struct Resource<T: ?Sized> { owner: Arc<Owner<T>> }
impl<T: ?Sized> Resource<T> {
    pub fn new(value: Arc<T>) -> Self { Self::with_id(PortableId::random(), value) }
    pub fn with_id(id: PortableId, value: Arc<T>) -> Self {
        Self { owner: Arc::new(Owner { id, value, encoded: OnceLock::new() }) }
    }
    pub fn with_id_and_encoded(id: PortableId, value: Arc<T>, encoded: EncodedBytes) -> Self {
        Self { owner: Arc::new(Owner { id, value, encoded: OnceLock::from(encoded) }) }
    }
    pub fn id(&self) -> PortableId { self.owner.id }
    pub fn storage(&self) -> &Arc<T> { &self.owner.value }
    pub fn same_owner(&self, other: &Self) -> bool { Arc::ptr_eq(&self.owner, &other.owner) }
    pub fn encoded_if_ready(&self) -> Option<&EncodedBytes> { self.owner.encoded.get() }
    pub fn encoded(&self, encode: impl FnOnce(&T) -> EncodedBytes) -> &EncodedBytes {
        self.owner.encoded.get_or_init(|| encode(&self.owner.value))
    }
}
impl<T: ?Sized> Clone for Resource<T> {
    fn clone(&self) -> Self { Self { owner: self.owner.clone() } }
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
        f.debug_struct("Resource").field("id", &self.owner.id).field("value", &self.owner.value).finish()
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
