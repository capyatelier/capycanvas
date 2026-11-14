use super::*;
use crate::{package::session_store::{OwnerLock,SessionStore},tile_io::{ReadCache,ReadCompletion}};

type Observe=Arc<dyn Fn(u64,usize)+Send+Sync>;
struct Observed {inner:Arc<dyn FileTransport>,observe:Observe}
struct ObservedFile {inner:Arc<dyn StoreFile>,observe:Observe}
impl Observed {
    fn wrap<'a>(&'a self,file:FileFuture<'a,Arc<dyn StoreFile>>)->FileFuture<'a,Arc<dyn StoreFile>> {
        Box::pin(async move {Ok(Arc::new(ObservedFile {inner:file.await?,observe:self.observe.clone()}) as Arc<dyn StoreFile>)})
    }
}
impl RangeReader for ObservedFile {
    fn allocation_bytes(&self,offset:u64,length:usize)->usize {self.inner.allocation_bytes(offset,length)}
    fn read(&self,offset:u64,length:usize,crc:Option<u32>,completion:ReadCompletion) {
        (self.observe)(offset,length);self.inner.read(offset,length,crc,completion);
    }
}
impl StoreFile for ObservedFile {
    fn len(&self)->FileFuture<'_,u64> {self.inner.len()}
    fn read_at(&self,offset:u64,length:usize)->FileFuture<'_,Arc<[u8]>> {self.inner.read_at(offset,length)}
    fn read_cold_at(&self,offset:u64,length:usize)->FileFuture<'_,Arc<[u8]>> {self.inner.read_cold_at(offset,length)}
    fn write_at<'a>(&'a self,offset:u64,bytes:&'a [u8])->FileFuture<'a,()> {self.inner.write_at(offset,bytes)}
    fn flush(&self)->FileFuture<'_,()> {self.inner.flush()}
    fn discard_cold(&self,offset:u64,length:u64)->FileFuture<'_,()> {self.inner.discard_cold(offset,length)}
}
impl FileTransport for Observed {
    fn storage_facts(&self)->FileFuture<'_,crate::package::storage_budget::StorageFacts> {self.inner.storage_facts()}
    fn open<'a>(&'a self,name:&'a str,access:FileAccess)->FileFuture<'a,Arc<dyn StoreFile>> {self.wrap(self.inner.open(name,access))}
    fn open_copy<'a>(&'a self,name:&'a str,access:FileAccess)->FileFuture<'a,Arc<dyn StoreFile>> {self.wrap(self.inner.open_copy(name,access))}
    fn list<'a>(&'a self,name:&'a str)->FileFuture<'a,Vec<DirectoryEntry>> {self.inner.list(name)}
    fn remove<'a>(&'a self,name:&'a str)->FileFuture<'a,Removal> {self.inner.remove(name)}
}
pub async fn open_observed(path:&std::path::Path,cache:ReadCache,observe:Observe)->Result<SessionStore,String> {
    std::fs::create_dir_all(path).map_err(|error|error.to_string())?;
    let path=std::fs::canonicalize(path).map_err(|error|error.to_string())?;
    let lease=Arc::new(OwnerLock::claim(&path.join(".lock"))?.ok_or("Recovery drawing is already open")?);
    let io=Arc::new(Observed {inner:Arc::new(NativeTransport::new(path.clone())),observe});
    SessionStore::open_transport(path,io,lease,cache).await
}
