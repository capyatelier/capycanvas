use super::*;
use layer_host::session_stores::{SessionStores,StoreResult};
use std::sync::Arc;
use wasm_bindgen_futures::{future_to_promise,JsFuture};

struct PendingBinding {stores:SessionStores,id:Option<u64>}
impl PendingBinding {
    fn adopt(&mut self,id:u64) {self.stores.remap(&[(self.id.take().unwrap(),id)]);}
}
impl Drop for PendingBinding {fn drop(&mut self) {if let Some(id)=self.id.take() {self.stores.release_binding(id);}}}

#[wasm_bindgen]
pub struct WebSessionProject {session:Option<Box<UiSession<AttachedRenderer>>>,lost:Arc<std::sync::Mutex<Option<String>>>,binding:PendingBinding}
#[derive(Serialize)]
struct SessionAdoption {id:u64,change:layer_ui::UiChange}

#[wasm_bindgen]
impl WebApp {
    pub fn reserve_session_ids(&mut self,ids:JsValue)->Result<(),JsValue> {
        self.documents.reserve_identities(&serde_wasm_bindgen::from_value::<Vec<u64>>(ids).map_err(js)?).map_err(js)
    }
    pub fn prepare_session_restart(&mut self,key:String,recovered:bool,observe:js_sys::Function)->Result<js_sys::Promise,JsValue> {
        let renderer=self.session.engine().backend().0.as_ref().ok_or_else(||js("Wait for the canvas"))?;
        let factory=renderer.renderer_factory();
        let lost=self.gpu_owner().ok_or_else(||js("Wait for the canvas"))?;
        let viewport=self.session.state().camera.viewport;let localization=self.session.localization().clone();
        let brush=self.session.engine().configured_brush().clone();
        let admission=self.documents.admission(&self.session.retained_document_tiles());
        let id=self.documents.upcoming_id();self.documents.reserve_identities(&[id]).map_err(js)?;
        let binding=PendingBinding {stores:self.stores.clone(),id:Some(id)};
        Ok(future_to_promise(async move {
            let Some(key)=legacy_import::ensure_copy(&binding.stores,&key).await.map_err(js)? else {return Ok(JsValue::NULL);};
            binding.stores.bind(id,Some(&key)).map_err(js)?;
            let Some(_ownership)=store_transport::transport(&format!("drawings/{key}")).await.map_err(js)? else {return Ok(JsValue::NULL);};
            let reply=binding.stores.restore(id,layer_core::ProjectLimits::for_import()).map_err(js)?;
            let StoreResult::Restored {session,recovered_previous}=reply.await.map_err(js)? else {return Err(js("Invalid restore completion"));};
            let restore=session.ok_or_else(||js("The saved drawing is missing"))?;
            let observed=observe.call1(&JsValue::NULL,&serialize(&restore.state.location)?)?;
            let observed:Option<layer_ui::DestinationFingerprint>=serde_wasm_bindgen::from_value(JsFuture::from(js_sys::Promise::resolve(&observed)).await?).map_err(js)?;
            admission.admit(restore.document()).map_err(|reason|js(reason.message(&localization)))?;
            let mut candidate=documents::prepare_session(restore.document().clone(),factory,&brush,viewport,localization,None).await?;
            candidate.restore_session(restore,recovered||recovered_previous,observed).map_err(js)?;
            Ok(WebSessionProject {session:Some(Box::new(candidate)),lost,binding}.into())
        }))
    }
    pub fn adopt_session_restart(&mut self,mut project:WebSessionProject,initial:JsValue,identity:u64,activate:bool)->Result<JsValue,JsValue> {
        let lost=self.gpu_owner().ok_or_else(||js("Canvas unavailable"))?;
        if !Arc::ptr_eq(&lost,&project.lost)||lost.lock().unwrap().is_some(){return Err(js("The canvas changed while restoring the drawing"));}
        self.documents.reserve_identities(&[identity]).map_err(js)?;
        let initial=serde_wasm_bindgen::from_value(initial).map_err(js)?;
        let replace=self.documents.order().len()==1&&self.session.can_replace_startup_session(&initial);
        let mut candidate=*project.session.take().ok_or_else(||js("Drawing checkpoint was already adopted"))?;
        candidate.set_document_replacement(false);candidate.inherit_restored_window_state(&self.session).map_err(js)?;
        let config=&self.surface.as_ref().ok_or_else(||js("Canvas unavailable"))?.config;
        candidate.renderer_mut().resize_surface(config.width,config.height).map_err(js)?;
        if !activate || !replace {
            let tiles=candidate.park_document().map_err(js)?;candidate.renderer_mut().0.take();
            let id=if self.documents.order().contains(&identity){self.documents.append_parked(candidate,tiles,self.session.localization())}else{
                self.documents.append_parked_with_id(identity,candidate,tiles,self.session.localization()).map_err(|(reason,_)|js(reason))?;identity
            };
            project.binding.adopt(id);
            return serialize(&SessionAdoption{id,change:layer_ui::UiChange{revision:self.session.state().revision,regions:layer_ui::regions::DOCUMENT,..Default::default()}});
        }
        self.session.park_document().map_err(js)?;
        self.session.renderer_mut().0.take();
        self.session=candidate;
        self.documents.restore_identity(identity).map_err(js)?;
        if let Some(control)=self.tone.pending.take(){control.cancel();}
        self.tone=Default::default();self.proof=Default::default();self.startup = Default::default();
        project.binding.adopt(identity);
        serialize(&SessionAdoption{id:identity,change:layer_ui::UiChange{revision:self.session.state().revision,regions:layer_ui::regions::ALL,canvas_wake:true,..Default::default()}})
    }
    pub fn restore_session_order(&mut self,order:JsValue)->Result<JsValue,JsValue> {
        let order:Vec<u64>=serde_wasm_bindgen::from_value(order).map_err(js)?;
        self.documents.restore_order(&order,self.documents.selected()).map_err(js)?;
        self.document_changed()
    }
}

#[wasm_bindgen]
pub struct WebFileFingerprint(Option<layer_ui::FingerprintWriter<std::io::Sink>>);
#[wasm_bindgen]
impl WebFileFingerprint {
    #[wasm_bindgen(constructor)]
    pub fn new()->Self {Self(Some(layer_ui::FingerprintWriter::new(std::io::sink())))}
    pub fn update(&mut self,bytes:&[u8])->Result<(),JsValue> {
        use std::io::Write;
        self.0.as_mut().ok_or_else(||js("File fingerprint is complete"))?.write_all(bytes).map_err(js)
    }
    pub fn finish(&mut self)->Result<JsValue,JsValue> {
        serialize(&self.0.take().ok_or_else(||js("File fingerprint is complete"))?.finish())
    }
}
