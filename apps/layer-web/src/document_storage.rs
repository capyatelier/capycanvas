use super::*;
use wasm_bindgen_futures::future_to_promise;

#[wasm_bindgen]
impl WebApp {
    pub fn set_document_cache_budget(&mut self,bytes:u32) {
        self.documents.set_inactive_ram_budget((bytes as usize).min(layer_ui::DocumentBudget::default().inactive_ram));
    }
    pub fn await_document_backing(&self)->js_sys::Promise {
        let artwork=self.session.engine().document().artwork.clone();
        future_to_promise(async move {artwork_transfer::wait_backing(&artwork).await?;Ok(JsValue::UNDEFINED)})
    }
    pub fn await_document_trim(&self)->js_sys::Promise {
        self.stores.watch_pressure(self.documents.pressure_source());
        let wait=self.documents.trim_wait();
        future_to_promise(async move {if let Some(wait)=wait {wait.await.map_err(js)?;}Ok(JsValue::UNDEFINED)})
    }
}
