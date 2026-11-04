use crate::import_policy::AssetWriteFn;
use layer_core::{Document,package::session_store::DrawingStore};
use std::{collections::HashMap,sync::{Arc,Mutex,Weak,atomic::{AtomicU64,Ordering}}};

#[cfg(not(target_arch="wasm32"))]
pub type AssetResolveFn=Arc<dyn Fn(Option<DrawingStore>)->Result<AssetBinding,String>+Send+Sync>;
#[cfg(target_arch="wasm32")]
pub type AssetResolveFn=Arc<dyn Fn(Option<DrawingStore>)->Result<AssetBinding,String>>;
pub struct AssetBinding {pub store:DrawingStore,pub write:AssetWriteFn}
impl AssetBinding {
    pub async fn store_document(&self,document:&Document)->Result<(),String> {
        let tiles=layer_core::Editor::new(document.clone()).retained_tiles();
        if tiles.try_blobs()?.is_none_or(|blobs|!blobs.is_empty()) {(self.write)(tiles,document.memory().clone().take_reservation())?.await?;}
        document.memory().bind_storage(self.store.clone())
    }
}
#[derive(Clone)]
pub struct AssetResolver {root:std::path::PathBuf,resolve:AssetResolveFn}
impl AssetResolver {
    pub fn new(root:std::path::PathBuf,resolve:AssetResolveFn)->Self {Self {root,resolve}}
}
struct Registration {
    id:u64,resolver:Mutex<Result<Option<AssetResolver>,String>>,
    documents:Mutex<HashMap<u64,Weak<Mutex<Option<DrawingStore>>>>>,
}
type Registrations=HashMap<u64,Weak<Registration>>;
#[cfg(not(target_arch="wasm32"))]
fn registrations<T>(f:impl FnOnce(&mut Registrations)->T)->T {
    static SINK:std::sync::OnceLock<Mutex<Registrations>>=std::sync::OnceLock::new();
    f(&mut SINK.get_or_init(Default::default).lock().unwrap())
}
#[cfg(target_arch="wasm32")]
fn registrations<T>(f:impl FnOnce(&mut Registrations)->T)->T {
    thread_local! {static SINK:std::cell::RefCell<Registrations>=Default::default();}
    SINK.with(|sink|f(&mut sink.borrow_mut()))
}
impl Drop for Registration {fn drop(&mut self) {registrations(|sink| {sink.remove(&self.id);});}}

pub struct AssetStore(Arc<Registration>);
impl Default for AssetStore {
    fn default()->Self {
        static NEXT:AtomicU64=AtomicU64::new(1);
        let state=Arc::new(Registration {id:NEXT.fetch_add(1,Ordering::Relaxed),resolver:Mutex::new(Ok(None)),documents:Default::default()});
        registrations(|sink| {sink.insert(state.id,Arc::downgrade(&state));});Self(state)
    }
}
impl AssetStore {
    pub fn writer(&self)->AssetWriter {AssetWriter {scope:self.0.id,target:Default::default()}}
    pub fn attach(&self,resolver:AssetResolver)->Result<(),String> {
        let mut current=self.0.resolver.lock().unwrap();
        if current.as_ref().map_err(Clone::clone)?.as_ref().is_some_and(|old|old.root!=resolver.root) {return Err("Drawing store belongs to another session root".into());}
        if current.as_ref().unwrap().as_ref().is_some_and(|old|!Arc::ptr_eq(&old.resolve,&resolver.resolve)) {return Err("Drawing storage owner changed".into());}
        *current=Ok(Some(resolver));Ok(())
    }
    pub fn fail(&self,error:String) {
        let mut resolver=self.0.resolver.lock().unwrap();
        if matches!(&*resolver,Ok(None)) {*resolver=Err(error);}
    }
    pub(crate) fn for_writer(writer:&AssetWriter)->Result<Self,String> {Ok(Self(writer.registration()?))}
}
#[derive(Clone)]
pub struct AssetWriter {scope:u64,target:Arc<Mutex<Option<DrawingStore>>>}
impl AssetWriter {
    fn registration(&self)->Result<Arc<Registration>,String> {
        registrations(|sink|sink.get(&self.scope).and_then(Weak::upgrade)).ok_or_else(||"Drawing storage owner closed".into())
    }
    pub fn resolve(&self)->Result<Option<AssetBinding>,String> {
        let registration=self.registration()?;
        let Some(resolver)=registration.resolver.lock().unwrap().clone()? else {return Ok(None);};
        let mut target=self.target.lock().unwrap();
        let binding=(resolver.resolve)(target.clone())?;
        *target=Some(binding.store.clone());Ok(Some(binding))
    }
    pub fn binding(&self)->Option<DrawingStore> {self.target.lock().unwrap().clone()}
    pub fn fork(&self)->Result<Self,String> {self.registration()?;Ok(Self {scope:self.scope,target:Default::default()})}
    pub fn for_binding(&self,binding:DrawingStore)->Result<Self,String> {
        let writer=self.fork()?;writer.bind(binding)?;Ok(writer)
    }
    fn bind(&self,binding:DrawingStore)->Result<(),String> {
        let registration=self.registration()?;
        if registration.resolver.lock().unwrap().as_ref().map_err(Clone::clone)?.as_ref().is_some_and(|resolver|binding.path().parent()!=Some(resolver.root.as_path())) {return Err("Drawing store belongs to another session root".into());}
        let mut current=self.target.lock().unwrap();
        if current.as_ref().is_some_and(|old|old.path()!=binding.path()) {return Err("Drawing storage identity changed".into());}
        *current=Some(binding);Ok(())
    }
    pub fn for_document(&self,document:&Document)->Result<Self,String> {
        let registration=self.registration()?;let mut documents=registration.documents.lock().unwrap();
        documents.retain(|_,target|target.strong_count()!=0);
        let target=documents.entry(document.memory().storage_identity()).or_default();
        let writer=Self {scope:self.scope,target:target.upgrade().unwrap_or_default()};*target=Arc::downgrade(&writer.target);
        if let Some(binding)=document.memory().storage() {writer.bind(binding)?;}Ok(writer)
    }
    pub(crate) fn install(&self,resolver:AssetResolver,preferred:Option<DrawingStore>,fallback:DrawingStore)->Result<DrawingStore,String> {
        let mut target=self.target.lock().unwrap();
        if preferred.as_ref().zip(target.as_ref()).is_some_and(|(a,b)|a.path()!=b.path()) {return Err("Drawing storage identity changed".into());}
        let binding=preferred.or_else(||target.clone()).unwrap_or(fallback);
        if binding.path().parent()!=Some(resolver.root.as_path()) {return Err("Drawing store belongs to another session root".into());}
        AssetStore::for_writer(self)?.attach(resolver)?;*target=Some(binding.clone());Ok(binding)
    }
    pub fn storage_failed(&self,error:String) {if let Ok(store)=AssetStore::for_writer(self) {store.fail(error);}}
    pub async fn prepare_document(&self,document:&mut Document)->Result<(),String> {
        document.initialize_memory();
        let binding=self.resolve()?;
        if document.memory().has_package() {
            if let Some(binding)=binding {document.memory().bind_storage(binding.store)?;}
            document.reserve_import()
        }else if let Some(binding)=binding {binding.store_document(document).await}
        else {document.reserve_import()}
    }
    pub async fn store_document(&self,document:&Document)->Result<(),String> {
        let Some(binding)=self.resolve()? else {return Ok(());};
        binding.store_document(document).await
    }
    pub async fn store_clip(&self,clip:&crate::PixelClip,localization:&crate::Localizer)->Result<(),String> {self.store_document(&clip.document(localization)?).await}
}

pub(crate) struct DrawingAssets {pub store:AssetStore,pub writer:AssetWriter}
impl Default for DrawingAssets {fn default()->Self {let store=AssetStore::default();let writer=store.writer();Self {store,writer}}}
impl DrawingAssets {
    pub fn adopt(&mut self,writer:AssetWriter)->Result<(),String> {self.store=AssetStore::for_writer(&writer)?;self.writer=writer;Ok(())}
    pub fn install(&mut self,resolver:AssetResolver,preferred:Option<DrawingStore>,fallback:DrawingStore)->Result<DrawingStore,String> {
        let replace=match &*self.store.0.resolver.lock().unwrap() {
            Err(_)=>true,
            Ok(Some(old))=>{
                if old.root!=resolver.root {return Err("Drawing store belongs to another session root".into());}
                !Arc::ptr_eq(&old.resolve,&resolver.resolve)
            },
            Ok(None)=>false,
        };
        if replace {*self=Self::default();}
        self.writer.install(resolver,preferred,fallback)
    }
}
