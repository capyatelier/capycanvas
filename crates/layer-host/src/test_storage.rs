use crate::NativeHost;
use std::{ops::{Deref,DerefMut},path::PathBuf,sync::Arc};

pub(crate) struct StoredHost {host:Option<NativeHost>,stores:Option<crate::session_stores::SessionStores>,root:PathBuf}
impl StoredHost {
    pub(crate) fn new(host:NativeHost)->Self {
        let root=std::env::temp_dir().join(format!("capy-host-storage-{}",layer_core::PortableId::random()));
        let stores=crate::session_stores::SessionStores::native(root.clone(),Arc::new(||{}));
        stores.install_storage(1,&host.session).unwrap();
        Self {host:Some(host),stores:Some(stores),root}
    }
}
impl Deref for StoredHost {type Target=NativeHost;fn deref(&self)->&NativeHost {self.host.as_ref().unwrap()}}
impl DerefMut for StoredHost {fn deref_mut(&mut self)->&mut NativeHost {self.host.as_mut().unwrap()}}
impl Drop for StoredHost {
    fn drop(&mut self) {
        drop(self.host.take());
        drop(self.stores.take());
        if let Err(error)=std::fs::remove_dir_all(&self.root) {assert_eq!(error.kind(),std::io::ErrorKind::NotFound);}
    }
}
