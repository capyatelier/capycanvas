use super::PortableId;
use std::{collections::BTreeMap, fmt, marker::PhantomData, sync::Arc};

pub struct Handle<T> { index: u32, marker: PhantomData<fn() -> T> }
impl<T> Handle<T> {
    pub const INVALID: Self = Self::from_index(u32::MAX);
    pub const fn from_index(index: u32) -> Self { Self { index, marker: PhantomData } }
    pub const fn index(self) -> u32 { self.index }
}
impl<T> Default for Handle<T> { fn default() -> Self { Self::INVALID } }
impl<T> Copy for Handle<T> {}
impl<T> Clone for Handle<T> { fn clone(&self) -> Self { *self } }
impl<T> PartialEq for Handle<T> { fn eq(&self, other: &Self) -> bool { self.index == other.index } }
impl<T> Eq for Handle<T> {}
impl<T> PartialOrd for Handle<T> { fn partial_cmp(&self, other:&Self)->Option<std::cmp::Ordering>{Some(self.cmp(other))} }
impl<T> Ord for Handle<T> { fn cmp(&self,other:&Self)->std::cmp::Ordering{self.index.cmp(&other.index)} }
impl<T> std::hash::Hash for Handle<T> { fn hash<H: std::hash::Hasher>(&self, state: &mut H) { self.index.hash(state); } }
impl<T> fmt::Debug for Handle<T> { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.index.fmt(f) } }
impl<T> serde::Serialize for Handle<T> { fn serialize<S:serde::Serializer>(&self,s:S)->Result<S::Ok,S::Error>{s.serialize_u32(self.index)} }
impl<'de,T> serde::Deserialize<'de> for Handle<T> { fn deserialize<D:serde::Deserializer<'de>>(d:D)->Result<Self,D::Error>{Ok(Self::from_index(serde::Deserialize::deserialize(d)?))} }

#[derive(Clone, Debug, PartialEq)]
pub struct Store<T> {
    entries: Arc<Vec<(PortableId, Option<Arc<T>>)>>,
    ids: Arc<BTreeMap<PortableId, Handle<T>>>,
}
impl<T> Default for Store<T> { fn default() -> Self { Self { entries: Arc::default(), ids: Arc::default() } } }
impl<T> Store<T> {
    pub fn next_handle(&self)->Handle<T>{Handle::from_index(u32::try_from(self.entries.len()).expect("Authored handle limit exceeded"))}
    pub fn capacity(&self)->usize {self.entries.len()}
    pub fn len(&self)->usize {self.entries.iter().filter(|(_,v)|v.is_some()).count()}
    pub fn is_empty(&self)->bool {self.entries.iter().all(|(_,v)|v.is_none())}
    pub fn same_root(&self,other:&Self)->bool {Arc::ptr_eq(&self.entries,&other.entries)}
    pub fn reserve(&mut self, id: PortableId) -> Result<Handle<T>, &'static str> {
        if self.ids.contains_key(&id) { return Err("Duplicate authored identity"); }
        let index = u32::try_from(self.entries.len()).map_err(|_| "Authored handle limit exceeded")?;
        if index==u32::MAX {return Err("Authored handle limit exceeded");}
        let handle = Handle::from_index(index);
        Arc::make_mut(&mut self.entries).push((id, None));
        Arc::make_mut(&mut self.ids).insert(id, handle);
        Ok(handle)
    }
    pub fn install(&mut self, handle: Handle<T>, value: T) -> Result<(), &'static str> {
        let entry = Arc::make_mut(&mut self.entries).get_mut(handle.index as usize).ok_or("Unknown authored handle")?;
        if entry.1.is_some() { return Err("Authored handle is already installed"); }
        entry.1 = Some(Arc::new(value)); Ok(())
    }
    pub fn allocated(&self, id: PortableId) -> Option<Handle<T>> { self.ids.get(&id).copied() }
    pub fn insert(&mut self, id: PortableId, value: T) -> Result<Handle<T>, &'static str> {
        let handle = self.reserve(id)?; self.install(handle, value)?; Ok(handle)
    }
    pub fn resolve(&self, id: PortableId) -> Option<Handle<T>> { self.ids.get(&id).copied().filter(|h| self.get(*h).is_some()) }
    pub fn shared(&self,handle:Handle<T>)->Option<&Arc<T>> {self.entries.get(handle.index as usize)?.1.as_ref()}
    pub fn get(&self, handle: Handle<T>) -> Option<&T> { self.entries.get(handle.index as usize)?.1.as_deref() }
    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> where T:Clone {
        Arc::make_mut(&mut self.entries).get_mut(handle.index as usize)?.1.as_mut().map(Arc::make_mut)
    }
    pub fn id(&self, handle: Handle<T>) -> Option<PortableId> { self.entries.get(handle.index as usize).map(|entry| entry.0) }
    pub fn remove(&mut self, handle: Handle<T>) -> Option<T> where T:Clone {
        Arc::make_mut(&mut self.entries).get_mut(handle.index as usize)?.1.take().map(Arc::unwrap_or_clone)
    }
    pub fn restore(&mut self, handle: Handle<T>, id: PortableId, value: T) -> Result<(), &'static str> {
        let entry = Arc::make_mut(&mut self.entries).get_mut(handle.index as usize).ok_or("Unknown authored handle")?;
        if entry.0 != id || entry.1.is_some() { return Err("Invalid authored restoration"); }
        entry.1 = Some(Arc::new(value)); Ok(())
    }
    pub fn change(&mut self,handle:Handle<T>,id:PortableId,value:Option<T>)->Result<Option<T>,&'static str> where T:Clone {
        if handle==self.next_handle() && value.is_some() {self.reserve(id)?;}
        let entry=Arc::make_mut(&mut self.entries).get_mut(handle.index as usize).ok_or("Unknown authored handle")?;
        if entry.0!=id {return Err("Authored identity does not match its handle");}
        Ok(std::mem::replace(&mut entry.1,value.map(Arc::new)).map(Arc::unwrap_or_clone))
    }
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (Handle<T>, PortableId, &T)> {
        self.entries.iter().enumerate().filter_map(|(index, (id, value))| value.as_deref().map(|value| (Handle::from_index(index as u32), *id, value)))
    }
}

#[derive(Clone,Debug,PartialEq)]
pub struct RecordChange<T> {pub handle:Handle<T>,pub id:PortableId,pub value:Option<T>}
impl<T> RecordChange<T> {
    pub fn replace(store:&Store<T>,handle:Handle<T>,value:Option<T>)->Result<Self,&'static str>{
        Ok(Self {handle,id:store.id(handle).ok_or("Unknown authored handle")?,value})
    }
    pub fn insert(store:&Store<T>,value:T)->Self{Self {handle:store.next_handle(),id:PortableId::random(),value:Some(value)}}
    pub fn remove(store:&Store<T>,handle:Handle<T>)->Result<Self,&'static str>{Self::replace(store,handle,None)}
}
