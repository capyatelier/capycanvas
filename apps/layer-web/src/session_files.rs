use super::*;
use layer_core::package::session_store::{check_store_key,MetadataStore};
use layer_host::session_stores::StoreResult;
use layer_ui::session_recovery::SessionManifest;
use std::sync::Arc;
use wasm_bindgen_futures::future_to_promise;

#[wasm_bindgen]
pub struct WebSessionWindow {key:String,store:Arc<MetadataStore>}
#[wasm_bindgen]
impl WebSessionWindow {
    #[wasm_bindgen(getter)]
    pub fn key(&self)->String {self.key.clone()}
    pub fn read(&self)->js_sys::Promise {
        let store=self.store.clone();
        future_to_promise(async move {serialize(&SessionManifest::read_index(&store).await.map_err(js)?)})
    }
    pub fn publish(&self,manifest:JsValue)->Result<js_sys::Promise,JsValue> {
        let manifest:SessionManifest=serde_wasm_bindgen::from_value(manifest).map_err(js)?;manifest.validate().map_err(js)?;
        let store=self.store.clone();
        Ok(future_to_promise(async move {manifest.publish_index(&store).await.map_err(js)?;Ok(JsValue::UNDEFINED)}))
    }
}
#[wasm_bindgen]
impl WebApp {
    pub fn import_legacy_sessions(&self)->js_sys::Promise {
        let stores=self.stores.clone();future_to_promise(async move {
            let warnings=legacy_import::import_all(&stores).await.map_err(js)?;
            collect_unreferenced(&stores).await.map_err(js)?;serialize(&warnings)
        })
    }
    pub fn session_windows(&self)->js_sys::Promise {
        future_to_promise(async move {serialize(&store_transport::directories("windows").await.map_err(js)?)})
    }
    pub fn open_session_window(&self,key:Option<String>)->Result<js_sys::Promise,JsValue> {
        let key=key.unwrap_or_else(||layer_core::PortableId::random().to_string());check_store_key(&key).map_err(js)?;
        Ok(future_to_promise(async move {
            let Some(io)=store_transport::transport(&format!("windows/{key}")).await.map_err(js)? else {return Ok(JsValue::NULL);};
            Ok(WebSessionWindow {key,store:Arc::new(MetadataStore::new(io,1024*1024))}.into())
        }))
    }
    pub fn session_binding(&self,id:u64,key:Option<String>)->Result<String,JsValue> {
        let session=self.document_session(id)?;
        if let Some(binding)=session.engine().document().memory().storage() {
            self.stores.adopt_binding(id,binding.clone()).map_err(js)?;
            self.stores.install_storage(id,session).map_err(js)?;return Ok(binding.key().to_owned());
        }
        self.stores.bind(id,key.as_deref()).map_err(js)?;
        self.stores.install_storage(id,session).map(|binding|binding.key().to_owned()).map_err(js)
    }
    pub fn session_existing_binding(&self,key:&str)->Option<u64> {
        self.stores.bindings().into_iter().find_map(|(id,binding)|(binding.key()==key).then_some(id))
    }
    pub fn session_current(&self,id:u64)->Result<bool,JsValue> {
        Ok(self.stores.is_current(id,&self.document_session(id)?.session_stamp()))
    }
    pub fn finish_session_package(&self,id:u64)->Result<js_sys::Promise,JsValue> {
        let reply=self.stores.finish_package(id).map_err(js)?;
        Ok(future_to_promise(async move {reply.await.map_err(js)?;Ok(JsValue::UNDEFINED)}))
    }
    pub fn checkpoint_session(&self,id:u64,forced:bool)->Result<js_sys::Promise,JsValue> {
        let session=self.document_session(id)?;let stamp=session.session_stamp();
        if !self.stores.checkpoint_due(id,stamp.clone(),forced).map_err(js)? {return Ok(js_sys::Promise::resolve(&JsValue::FALSE));}
        let capture=session.capture_session().map_err(js)?;
        let reply=self.stores.checkpoint(id,capture,stamp,forced).map_err(js)?;
        Ok(future_to_promise(async move {
            match reply.await.map_err(js)? {StoreResult::Checkpoint(_)=>Ok(JsValue::TRUE),StoreResult::Superseded=>Ok(JsValue::FALSE),_=>Err(js("Invalid checkpoint completion"))}
        }))
    }
    pub fn prepare_session_retirement(&self,key:String)->Result<js_sys::Promise,JsValue> {
        let reply=self.stores.prepare_retirement(&key).map_err(js)?;
        Ok(future_to_promise(async move {reply.await.map_err(js)?;Ok(JsValue::UNDEFINED)}))
    }
    pub fn retire_session_copy(&self,key:String)->Result<js_sys::Promise,JsValue> {
        let reply=self.stores.retire_copy(&key).map_err(js)?;
        Ok(future_to_promise(async move {reply.await.map_err(js)?;Ok(JsValue::UNDEFINED)}))
    }
}

async fn collect_unreferenced(stores:&layer_host::session_stores::SessionStores)->Result<(),String> {
    let mut roots:std::collections::BTreeSet<_>=stores.bindings().into_iter().map(|(_,binding)|binding.key().to_owned()).collect();
    for key in store_transport::directories("windows").await? {
        let Some(io)=store_transport::transport(&format!("windows/{key}")).await? else {return Ok(());};
        if let Some(manifest)=SessionManifest::read_index(&MetadataStore::new(io,1024*1024)).await? {roots.extend(manifest.drawings.into_iter().map(|drawing|drawing.key));}
    }
    for key in store_transport::directories("drawings").await? {
        if !roots.contains(&key) {store_transport::collect_unreferenced(&format!("drawings/{key}")).await?;}
    }Ok(())
}
