//! OPFS transport for core immutable tile chunks. No JS handle is retained in
//! an editor or tile; the final Rust chunk owner schedules file retirement.
use super::*;
use layer_core::raster_storage::{TileChunk, prepare_external_spill};
use std::sync::{Arc, Mutex};
use wasm_bindgen_futures::{JsFuture, future_to_promise, spawn_local};

enum Read {
    Cold,
    Loading,
    Ready(Arc<[u8]>),
    Failed(String),
}
struct BrowserChunk {
    key: String,
    len: usize,
    state: Arc<Mutex<Read>>,
}
impl TileChunk for BrowserChunk {
    fn len(&self) -> usize {
        self.len
    }
    fn resident_bytes(&self) -> usize {
        if matches!(*self.state.lock().unwrap(), Read::Ready(_)) {
            self.len
        } else {
            0
        }
    }
    fn evict(&self) {
        let mut state = self.state.lock().unwrap();
        if matches!(*state, Read::Ready(_)) {
            *state = Read::Cold;
        }
    }
    fn poll(&self) -> Result<Option<Arc<[u8]>>, String> {
        let mut state = self.state.lock().map_err(|_| "Drawing cache lock failed")?;
        match &*state {
            Read::Ready(bytes) => return Ok(Some(bytes.clone())),
            Read::Failed(error) => return Err(error.clone()),
            Read::Loading => return Ok(None),
            Read::Cold => (),
        }
        *state = Read::Loading;
        let (key, len, state) = (self.key.clone(), self.len, self.state.clone());
        spawn_local(async move {
            let result = async {
                let value = JsFuture::from(raster_worker::call(
                    "tab-read",
                    &key,
                    &js_sys::Array::new(),
                )?)
                .await?;
                let bytes = js_sys::Uint8Array::new(&value).to_vec();
                if bytes.len() != len {
                    return Err(js("Incomplete parked drawing chunk"));
                }
                Ok(bytes)
            }
            .await;
            *state.lock().unwrap() = match result {
                Ok(bytes) => Read::Ready(bytes.into()),
                Err(error) => Read::Failed(format!("Cannot read parked drawing: {error:?}")),
            };
        });
        Ok(None)
    }
}
impl Drop for BrowserChunk {
    fn drop(&mut self) {
        let key = self.key.clone();
        spawn_local(async move {
            if let Ok(promise) = raster_worker::call("tab-release", &key, &js_sys::Array::new()) {
                let _ = JsFuture::from(promise).await;
            }
        });
    }
}

#[wasm_bindgen]
impl WebApp {
    /// A diagnostic override can lower the fixed default budget, never raise it.
    pub fn set_document_cache_budget(&mut self, bytes: u32) {
        self.documents.budget.inactive_ram =
            (bytes as usize).min(layer_ui::DocumentBudget::default().inactive_ram);
    }
    pub fn document_storage_result(&mut self, error: Option<String>) {
        self.documents.storage_completed(error.map_or(Ok(()), Err));
    }
    pub fn spill_document_tiles(&self) -> Result<js_sys::Promise, JsValue> {
        let candidate = self.documents.spill_candidate();
        let prepared = candidate
            .as_ref()
            .map(prepare_external_spill)
            .transpose()
            .map_err(js)?
            .flatten();
        Ok(future_to_promise(async move {
            if let Some(spill) = prepared {
                let buffers = js_sys::Array::new();
                buffers.push(&js_sys::Uint8Array::from(spill.bytes.as_slice()));
                let key = JsFuture::from(raster_worker::call("tab-write", "", &buffers)?)
                    .await?
                    .as_string()
                    .ok_or_else(|| js("Invalid drawing cache acknowledgement"))?;
                let chunk = BrowserChunk {
                    key,
                    len: spill.bytes.len(),
                    state: Arc::new(Mutex::new(Read::Cold)),
                };
                spill.commit(Arc::new(chunk)).map_err(js)?;
            }
            Ok(JsValue::from_bool(candidate.is_some()))
        }))
    }
}
