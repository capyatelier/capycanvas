use super::*;
use layer_core::package::{store_io::{self,DirectoryEntry,FileAccess,FileFuture,FileTransport,StoreFile},session_store::{SessionStore,StoreOpener}};
use layer_core::tile_io::{ReadCache,RangeReader,ReadCompletion};
use std::{cell::RefCell,collections::BTreeMap,path::Path,sync::{Arc,Weak}};
use wasm_bindgen_futures::{JsFuture,spawn_local};

struct TransferBuffers {owner:layer_core::MemoryOwner,bytes:std::sync::Mutex<(u64,u64)>}
struct BufferUse {buffers:Arc<TransferBuffers>,bytes:u64}
impl TransferBuffers {
    fn update(&self,change:impl FnOnce(u64)->u64) {
        let mut bytes=self.bytes.lock().unwrap();bytes.0=change(bytes.0);
        if bytes.0.abs_diff(bytes.1)>=1024*1024 || bytes.0==0&&bytes.1!=0 {
            self.owner.publish(layer_core::MemoryReport {ram_bytes:bytes.0,pinned_ram_bytes:bytes.0,demand_ram_bytes:bytes.0,..Default::default()});bytes.1=bytes.0;
        }
    }
}
impl BufferUse {
    fn new(bytes:u64)->Option<Self> {
        if bytes==0 {return None;}
        static BUFFERS:std::sync::OnceLock<Arc<TransferBuffers>>=std::sync::OnceLock::new();
        let buffers=BUFFERS.get_or_init(||Arc::new(TransferBuffers {owner:layer_core::MemoryBudget::process().register_owner(layer_core::MemoryOwnerKind::Staging,layer_core::MemoryClass::Visible,None),bytes:Default::default()})).clone();
        buffers.update(|current|current+bytes);Some(Self {buffers,bytes})
    }
}
impl Drop for BufferUse {fn drop(&mut self) {self.buffers.update(|current|current-self.bytes);}}

thread_local! {
    static TRANSPORT:RefCell<Option<js_sys::Function>>=const {RefCell::new(None)};
    static WAKE:RefCell<Option<js_sys::Function>>=const {RefCell::new(None)};
    static NORMALIZE:RefCell<Option<js_sys::Function>>=const {RefCell::new(None)};
    static LEASES:RefCell<BTreeMap<String,Weak<Lease>>>=const {RefCell::new(BTreeMap::new())};
}

#[wasm_bindgen]
pub fn configure_store_transport(transport:js_sys::Function,wake:js_sys::Function,normalize:Option<js_sys::Function>) {
    WAKE.with_borrow_mut(|slot|*slot=Some(wake));
    NORMALIZE.with_borrow_mut(|slot|*slot=normalize);
    TRANSPORT.with_borrow_mut(|slot|*slot=Some(transport));
    configure_reads();
}
pub(crate) fn configure_reads() {ReadCache::process().set_scheduler(Arc::new(|cache|spawn_local(async move {cache.drive();})));}
fn error(value:JsValue)->String {
    if js_sys::Reflect::get(&value,&js("name")).ok().and_then(|name|name.as_string()).as_deref()==Some("QuotaExceededError") {return layer_core::package::storage_budget::STORAGE_UNAVAILABLE.into();}
    value.as_string().or_else(||js_sys::Reflect::get(&value,&js("message")).ok().and_then(|message|message.as_string())).unwrap_or_else(||format!("{value:?}"))
}
fn begin_call(operation:&str,metadata:serde_json::Value,bytes:Option<&[u8]>)->Result<js_sys::Promise,String> {
    let request=js_sys::Object::new();let buffers=js_sys::Array::new();
    if let Some(bytes)=bytes {buffers.push(&js_sys::Uint8Array::from(bytes));}
    js_sys::Reflect::set(&request,&js("operation"),&js(operation)).map_err(error)?;
    let metadata=metadata.serialize(&serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true)).map_err(|error|error.to_string())?;
    js_sys::Reflect::set(&request,&js("metadata"),&metadata).map_err(error)?;
    js_sys::Reflect::set(&request,&js("buffers"),&buffers).map_err(error)?;
    let transport=TRANSPORT.with_borrow(Clone::clone).ok_or("Drawing storage unavailable")?;
    transport.call1(&JsValue::NULL,&request).map_err(error)?.dyn_into::<js_sys::Promise>().map_err(error)
}
pub(super) async fn call(operation:&str,metadata:serde_json::Value,bytes:Option<&[u8]>)->Result<JsValue,String> {
    let transfer_bytes=bytes.map_or_else(||if matches!(operation,"read_source"|"read_at"|"copy_read_at"|"legacy-range") {metadata["length"].as_u64().unwrap_or(0)}else {0},|bytes|bytes.len() as u64);
    let _transfer=BufferUse::new(transfer_bytes);
    JsFuture::from(begin_call(operation,metadata,bytes)?).await.map_err(error)
}

pub(super) async fn offer_sources(references:&[layer_core::package::transfer::BackingReference])->Result<Option<String>,String> {
    if references.is_empty() {return Ok(None);}
    let ticket=layer_core::PortableId::random().to_string();let mut guard=artwork_transfer::SourceOffer(Some(ticket.clone()));
    let response=call("offer_sources",serde_json::json!({"references":references,"ticket":ticket}),None).await?;
    if response.as_string().as_deref()!=Some(&ticket) {return Err("Invalid backing offer".into());}Ok(guard.0.take())
}
pub(super) fn cancel_sources(ticket:&str) {
    if let Ok(promise)=begin_call("cancel_sources",serde_json::json!({"ticket":ticket}),None) {spawn_local(async move {let _=JsFuture::from(promise).await;});}
}
struct SourceLease {id:String,key:String}
pub(super) struct RegisteredBlob {lease:SourceLease,file:web_sys::Blob}
impl RegisteredBlob {pub fn key(&self)->&str {&self.lease.key} pub fn file(&self)->&web_sys::Blob {&self.file}}
pub(super) async fn register_blob(file:&web_sys::Blob)->Result<RegisteredBlob,String> {
    artwork_transfer::seed_memory_budget();
    let id=layer_core::PortableId::random().to_string();let registered=RegisteredBlob {lease:SourceLease {key:format!("blob:{id}"),id:id.clone()},file:file.clone()};
    let request=js_sys::Object::new();let metadata=js_sys::Object::new();
    js_sys::Reflect::set(&metadata,&js("lease"),&js(&id)).map_err(error)?;
    js_sys::Reflect::set(&request,&js("metadata"),&metadata).map_err(error)?;
    js_sys::Reflect::set(&request,&js("operation"),&js("register_blob")).map_err(error)?;
    js_sys::Reflect::set(&request,&js("file"),file).map_err(error)?;
    let transport=TRANSPORT.with_borrow(Clone::clone).ok_or("Drawing storage unavailable")?;
    let promise=transport.call1(&JsValue::NULL,&request).map_err(error)?.dyn_into::<js_sys::Promise>().map_err(error)?;
    let value=JsFuture::from(promise).await.map_err(error)?;
    #[derive(Deserialize)]struct Registered {key:String,lease:String}
    let identity:Registered=serde_wasm_bindgen::from_value(value.clone()).map_err(|error|error.to_string())?;
    if identity.lease!=id || identity.key!=registered.key() {return Err("Invalid file generation reply".into());}
    let file=js_sys::Reflect::get(&value,&js("file")).map_err(error)?.dyn_into::<web_sys::Blob>().map_err(error)?;
    Ok(RegisteredBlob {lease:registered.lease,file})
}
impl Drop for SourceLease {
    fn drop(&mut self) {
        if let Ok(promise)=begin_call("release_source",serde_json::json!({"lease":self.id}),None) {spawn_local(async move {let _=JsFuture::from(promise).await;});}
    }
}
impl SourceLease {
    fn range(&self,offset:u64,length:usize)->FileFuture<'static,Arc<[u8]>> {
        let result=begin_call("read_source",serde_json::json!({"lease":self.id,"offset":offset,"length":length}),None);
        let buffers=BufferUse::new(length as u64);
        Box::pin(async move {
            let _buffers=buffers;
            let response=JsFuture::from(result?).await.map_err(error)?;
            let buffers=js_sys::Reflect::get(&response,&js("buffers")).map_err(error)?.dyn_into::<js_sys::Array>().map_err(error)?;
            let bytes=buffers.get(0).dyn_into::<js_sys::Uint8Array>().map_err(error)?;
            if buffers.length()!=1 || bytes.length() as usize!=length {return Err("Incomplete backing range".into());}Ok(bytes.to_vec().into())
        })
    }
}
impl RangeReader for SourceLease {
    fn transfer_key(&self)->Option<String> {Some(self.key.clone())}
    fn read(&self,offset:u64,length:usize,_crc:Option<u32>,completion:ReadCompletion) {
        let read=self.range(offset,length);spawn_local(async move {completion.complete(read.await);});
    }
    fn read_filtered(&self,offset:u64,length:usize,crc:u32,descriptor:layer_core::color::PixelDescriptor,completion:ReadCompletion) {
        let read=self.range(offset,length);let normalize=NORMALIZE.with_borrow(Clone::clone);
        spawn_local(async move {
            let result=async {
                let bytes=read.await?;
                let metadata=serde_json::to_string(&(descriptor,crc)).map_err(|error|error.to_string())?;
                let normalize=normalize.ok_or("Package preparation worker unavailable")?;
                let promise=normalize.call2(&JsValue::NULL,&js(metadata),&js_sys::Uint8Array::from(bytes.as_ref())).map_err(error)?.dyn_into::<js_sys::Promise>().map_err(error)?;
                let prepared=JsFuture::from(promise).await.map_err(error)?.dyn_into::<js_sys::Uint8Array>().map_err(error)?;
                if prepared.length() as usize>layer_core::package::MAX_RANGE_BYTES {return Err("Oversized normalized package tile".into());}Ok(prepared.to_vec().into())
            }.await;
            completion.complete(result);
        });
    }
}
pub(super) async fn adopt_sources(ticket:Option<&str>,references:&[layer_core::package::transfer::BackingReference])->Result<Vec<Arc<dyn layer_core::package::ByteSource>>,String> {
    if references.is_empty() {if ticket.is_some() {return Err("Unreferenced backing offer".into());}return Ok(Vec::new());}
    let ticket=ticket.ok_or("Missing backing offer")?;
    let leases=references.iter().map(|reference|Arc::new(SourceLease {id:layer_core::PortableId::random().to_string(),key:reference.key.clone()})).collect::<Vec<_>>();
    let requested=leases.iter().map(|lease|lease.id.as_str()).collect::<Vec<_>>();
    let ids:Vec<String>=serde_wasm_bindgen::from_value(call("adopt_sources",serde_json::json!({"ticket":ticket,"references":references,"leases":requested}),None).await?).map_err(|error|error.to_string())?;
    if ids.iter().map(String::as_str).ne(requested) {return Err("Backing adoption identity mismatch".into());}
    Ok(leases.into_iter().zip(references).map(|(lease,reference)| {
        Arc::new(ReadCache::process().source(reference.length,lease)) as Arc<dyn layer_core::package::ByteSource>
    }).collect())
}
pub(super) async fn remember_handles(handles:JsValue)->Result<(),String> {
    if handles.is_undefined() || handles.is_null() {return Ok(());}
    let request=js_sys::Object::new();
    js_sys::Reflect::set(&request,&js("operation"),&js("remember-handles")).map_err(error)?;
    js_sys::Reflect::set(&request,&js("handles"),&handles).map_err(error)?;
    let transport=TRANSPORT.with_borrow(Clone::clone).ok_or("Drawing storage unavailable")?;
    let promise=transport.call1(&JsValue::NULL,&request).map_err(error)?.dyn_into::<js_sys::Promise>().map_err(error)?;
    JsFuture::from(promise).await.map_err(error)?;Ok(())
}
struct Lease {name:String,legacy:Option<&'static str>}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Ok(promise)=begin_call("release",serde_json::json!({"name":self.name,"legacy":self.legacy}),None) {
            spawn_local(async move {let _=JsFuture::from(promise).await;});
        }
    }
}
pub(crate) fn wake() {if let Some(wake)=WAKE.with_borrow(Clone::clone) {let _=wake.call0(&JsValue::NULL);}}
pub(super) fn coordinator()->layer_host::session_stores::SessionStores {
    layer_host::session_stores::SessionStores::with_opener("drawings".into(),Arc::new(BrowserStoreOpener),
        Arc::new(|work|spawn_local(work.run_async())),Arc::new(wake),||js_sys::Date::now() as u64,layer_core::package::session_store::CompactionActivity::default())
}
async fn lease(name:&str,legacy:Option<&'static str>)->Result<Option<Arc<Lease>>,String> {
    store_io::check_member(name)?;
    let identity=legacy.map_or_else(||name.to_owned(),|kind|format!("{kind}:{name}"));
    if let Some(lease)=LEASES.with_borrow(|leases|leases.get(&identity).and_then(Weak::upgrade)) {return Ok(Some(lease));}
    let claimed=call("claim",serde_json::json!({"name":name,"legacy":legacy}),None).await?;
    if claimed.is_null() {return Ok(None);}
    if claimed.as_string().as_deref()!=Some(name) {return Err("Invalid drawing store ownership reply".into());}
    let lease=Arc::new(Lease {name:name.into(),legacy});
    LEASES.with_borrow_mut(|leases|{leases.retain(|_,lease|lease.strong_count()!=0);leases.insert(identity,Arc::downgrade(&lease));});
    Ok(Some(lease))
}
pub(super) async fn legacy_lock(kind:&'static str,name:&str)->Result<Option<Arc<dyn Send+Sync>>,String> {
    Ok(lease(name,Some(kind)).await?.map(|lease|lease as Arc<dyn Send+Sync>))
}
pub(super) async fn transport(name:&str)->Result<Option<Arc<dyn FileTransport>>,String> {
    Ok(lease(name,None).await?.map(|lease|Arc::new(BrowserTransport {lease}) as Arc<dyn FileTransport>))
}
pub(super) async fn collect_unreferenced(name:&str) {
    if let Ok(Some(lease))=lease(name,None).await {SessionStore::collect_unreferenced_transport(name.into(),Arc::new(BrowserTransport {lease:lease.clone()}),lease).await;}
}
#[derive(Deserialize)]struct Entry {name:String,directory:bool}
fn entries(value:JsValue)->Result<Vec<DirectoryEntry>,String> {
    let entries:Vec<Entry>=serde_wasm_bindgen::from_value(value).map_err(|error|error.to_string())?;
    entries.into_iter().map(|entry|{store_io::check_member(&entry.name)?;if entry.name.contains('/') {return Err("Invalid drawing store entry".into());}Ok(DirectoryEntry {name:entry.name,directory:entry.directory})}).collect()
}
pub(super) async fn directories(name:&str)->Result<Vec<String>,String> {
    store_io::check_member(name)?;
    Ok(entries(call("list",serde_json::json!({"name":name}),None).await?)?.into_iter().filter(|entry|entry.directory).map(|entry|entry.name).collect())
}
fn browser_storage_facts()->FileFuture<'static,layer_core::package::storage_budget::StorageFacts> {
    Box::pin(async move {
        let epoch=layer_core::package::storage_budget::StorageBudget::process().sample_epoch();
        serde_wasm_bindgen::from_value(call("estimate",serde_json::json!({"epoch":epoch}),None).await?).map_err(|error|error.to_string())
    })
}
struct BrowserStoreOpener;
impl StoreOpener for BrowserStoreOpener {
    fn storage_facts<'a>(&'a self,_path:&'a Path)->FileFuture<'a,layer_core::package::storage_budget::StorageFacts> {browser_storage_facts()}
    fn open<'a>(&'a self,path:&'a Path,retirement:bool)->FileFuture<'a,Option<SessionStore>> {
        Box::pin(async move {
            let name=path.to_str().ok_or("Invalid drawing store path")?;
            let lease=lease(name,None).await?.ok_or("Recovery drawing is already open")?;
            let transport=Arc::new(BrowserTransport {lease:lease.clone()});
            if retirement {
                if transport.list("").await?.is_empty() {return Ok(None);}
                SessionStore::retirement_transport(path.into(),transport,lease).await
            }else {SessionStore::open_transport(path.into(),transport,lease,ReadCache::process()).await.map(Some)}
        })
    }
}
struct BrowserTransport {lease:Arc<Lease>}
impl BrowserTransport {
    fn name(&self,member:&str)->Result<String,String> {store_io::check_member(member)?;Ok(format!("{}/{member}",self.lease.name))}
}
impl FileTransport for BrowserTransport {
    fn storage_facts(&self)->FileFuture<'_,layer_core::package::storage_budget::StorageFacts> {browser_storage_facts()}
    fn open<'a>(&'a self,name:&'a str,access:FileAccess)->FileFuture<'a,Arc<dyn StoreFile>> {
        Box::pin(async move {
            let name=self.name(name)?;
            let value=match access {FileAccess::Read=>"read",FileAccess::Write=>"write",FileAccess::Create=>"create"};
            call("open",serde_json::json!({"name":name,"access":value}),None).await?;
            Ok(Arc::new(BrowserFile {name,_lease:self.lease.clone(),writable:access!=FileAccess::Read}) as Arc<dyn StoreFile>)
        })
    }
    fn list<'a>(&'a self,directory:&'a str)->FileFuture<'a,Vec<DirectoryEntry>> {
        Box::pin(async move {
            let name=if directory.is_empty() {self.lease.name.clone()}else {self.name(directory)?};
            entries(call("list",serde_json::json!({"name":name}),None).await?)
        })
    }
    fn remove<'a>(&'a self,name:&'a str)->FileFuture<'a,store_io::Removal> {
        Box::pin(async move {serde_wasm_bindgen::from_value(call("remove",serde_json::json!({"name":self.name(name)?}),None).await?).map_err(|error|error.to_string())})
    }
}
#[derive(Clone)]
struct BrowserFile {name:String,_lease:Arc<Lease>,writable:bool}
impl BrowserFile {
    async fn range(&self,offset:u64,length:usize,crc:Option<u32>)->Result<Arc<[u8]>,String> {self.range_using("read_at",offset,length,crc).await}
    async fn range_using(&self,operation:&str,offset:u64,length:usize,crc:Option<u32>)->Result<Arc<[u8]>,String> {
        store_io::check_range(offset,length)?;
        let response=call(operation,serde_json::json!({"name":self.name,"offset":offset,"length":length,"crc":crc}),None).await?;
        let buffers=js_sys::Reflect::get(&response,&js("buffers")).map_err(error)?.dyn_into::<js_sys::Array>().map_err(error)?;
        let bytes=buffers.get(0).dyn_into::<js_sys::Uint8Array>().map_err(error)?;
        if buffers.length()!=1 || bytes.length() as usize!=length {return Err("Incomplete drawing store range".into());}
        Ok(bytes.to_vec().into())
    }
}
impl RangeReader for BrowserFile {
    fn transfer_key(&self)->Option<String> {Some(format!("store:{}",self.name))}
    fn read(&self,offset:u64,length:usize,crc:Option<u32>,completion:ReadCompletion) {
        let file=self.clone();spawn_local(async move {completion.complete(file.range(offset,length,crc).await);});
    }
}
impl StoreFile for BrowserFile {
    fn len(&self)->FileFuture<'_,u64> {
        Box::pin(async move {
            let length=call("len",serde_json::json!({"name":self.name}),None).await?.as_f64().ok_or("Invalid drawing store length")?;
            if length<0. || length.fract()!=0. || length>9_007_199_254_740_991. {return Err("Invalid drawing store length".into());}Ok(length as u64)
        })
    }
    fn read_at(&self,offset:u64,length:usize)->FileFuture<'_,Arc<[u8]>> {Box::pin(self.range(offset,length,None))}
    fn read_cold_at(&self,offset:u64,length:usize)->FileFuture<'_,Arc<[u8]>> {Box::pin(self.range_using("copy_read_at",offset,length,None))}
    fn write_at<'a>(&'a self,offset:u64,bytes:&'a [u8])->FileFuture<'a,()> {
        Box::pin(async move {
            if !self.writable {return Err("Drawing store file is read only".into());}store_io::check_range(offset,bytes.len())?;
            call("write_at",serde_json::json!({"name":self.name,"offset":offset}),Some(bytes)).await?;Ok(())
        })
    }
    fn flush(&self)->FileFuture<'_,()> {
        Box::pin(async move {call("flush",serde_json::json!({"name":self.name}),None).await?;Ok(())})
    }
}
