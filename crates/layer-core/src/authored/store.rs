use super::PortableId;
use std::{collections::BTreeMap, fmt, marker::PhantomData};

pub struct Handle<T> { index: u32, marker: PhantomData<fn() -> T> }
impl<T> Copy for Handle<T> {}
impl<T> Clone for Handle<T> { fn clone(&self) -> Self { *self } }
impl<T> PartialEq for Handle<T> { fn eq(&self, other: &Self) -> bool { self.index == other.index } }
impl<T> Eq for Handle<T> {}
impl<T> std::hash::Hash for Handle<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) { self.index.hash(state); }
}
impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.index.fmt(f) }
}

#[derive(Clone, Debug)]
pub struct Store<T> {
    entries: Vec<(PortableId, Option<T>)>,
    ids: BTreeMap<PortableId, Handle<T>>,
}
impl<T> Default for Store<T> {
    fn default() -> Self { Self { entries: Vec::new(), ids: BTreeMap::new() } }
}
impl<T> Store<T> {
    pub fn insert(&mut self, id: PortableId, value: T) -> Result<Handle<T>, &'static str> {
        if self.ids.contains_key(&id) { return Err("Duplicate authored identity"); }
        let index = u32::try_from(self.entries.len()).map_err(|_| "Authored handle limit exceeded")?;
        let handle = Handle { index, marker: PhantomData };
        self.entries.push((id, Some(value)));
        self.ids.insert(id, handle);
        Ok(handle)
    }
    pub fn resolve(&self, id: PortableId) -> Option<Handle<T>> {
        self.ids.get(&id).copied().filter(|h| self.get(*h).is_some())
    }
    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        self.entries.get(handle.index as usize)?.1.as_ref()
    }
    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        self.entries.get_mut(handle.index as usize)?.1.as_mut()
    }
    pub fn id(&self, handle: Handle<T>) -> Option<PortableId> {
        self.entries.get(handle.index as usize).map(|entry| entry.0)
    }
    pub fn remove(&mut self, handle: Handle<T>) -> Option<T> {
        self.entries.get_mut(handle.index as usize)?.1.take()
    }
    pub fn restore(&mut self, handle: Handle<T>, id: PortableId, value: T) -> Result<(), &'static str> {
        let entry = self.entries.get_mut(handle.index as usize).ok_or("Unknown authored handle")?;
        if entry.0 != id || entry.1.is_some() { return Err("Invalid authored restoration"); }
        entry.1 = Some(value);
        Ok(())
    }
    pub fn iter(&self) -> impl Iterator<Item = (Handle<T>, PortableId, &T)> {
        self.entries.iter().enumerate().filter_map(|(index, (id, value))| {
            value.as_ref().map(|value| (Handle { index: index as u32, marker: PhantomData }, *id, value))
        })
    }
}
