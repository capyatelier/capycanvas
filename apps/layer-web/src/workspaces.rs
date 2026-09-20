use super::*;
use layer_workspace::*;

pub(super) struct BrowserStore(js_sys::Function);
impl WorkspaceStore for BrowserStore {
    async fn execute(&self, request: StoreRequest) -> Result<StoreResponse, StoreError> {
        let request = serde_json::to_string(&request)?;
        let promise = self
            .0
            .call1(&JsValue::NULL, &JsValue::from_str(&request))
            .map_err(store_error)?;
        let value = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(&promise))
            .await
            .map_err(store_error)?;
        serde_json::from_str(
            &value
                .as_string()
                .ok_or_else(|| StoreError::invalid("Invalid IndexedDB reply."))?,
        )
        .map_err(Into::into)
    }
}
fn store_error(value: JsValue) -> StoreError {
    let text = value.as_string().unwrap_or_else(|| format!("{value:?}"));
    serde_json::from_str(&text).unwrap_or_else(|_| StoreError::new(ErrorKind::Unavailable, text))
}
// Keep one decoded database, keyed by the exact bytes read inside the current
// IndexedDB transaction. Compare JS strings before copying UTF-8 into Wasm.
// External writes and aborted writes change the key; reducer errors drop it.
struct CachedDatabase {
    source: JsValue,
    bytes: usize,
    database: BrowserDatabase,
    list: Option<String>,
}
thread_local! {
    static DATABASE_CACHE: std::cell::RefCell<Option<CachedDatabase>> =
        const { std::cell::RefCell::new(None) };
}
#[derive(Serialize)]
struct DatabaseReply<'a> {
    snapshot: Option<&'a str>,
    response: String,
}

/// Called synchronously inside an IndexedDB transaction. Return the strings
/// directly rather than JSON-escaping an entire snapshot across the bridge.
#[wasm_bindgen]
pub fn workspace_database(
    snapshot: JsValue,
    request: String,
    pending: bool,
    now: f64,
) -> Result<JsValue, JsValue> {
    let result = (|| -> Result<JsValue, StoreError> {
        let cached = DATABASE_CACHE.with(|cache| cache.borrow_mut().take());
        let mut cached = match cached {
            Some(cached) if js_sys::Object::is(&cached.source, &snapshot) => cached,
            _ => {
                let source = snapshot.as_string();
                if source.is_none() && !snapshot.is_null() && !snapshot.is_undefined() {
                    return Err(StoreError::invalid("Invalid workspace database snapshot."));
                }
                CachedDatabase {
                    database: source.as_deref().map(BrowserDatabase::decode).transpose()?.unwrap_or_default(),
                    bytes: source.as_ref().map_or(0, String::len),
                    source: snapshot,
                    list: None,
                }
            }
        };
        let request: StoreRequest = serde_json::from_str(&request)?;
        let read_only = !pending && matches!(&request,
            StoreRequest::List | StoreRequest::Load { .. } | StoreRequest::Raw { .. }
            | StoreRequest::Receipt { .. } | StoreRequest::Binding { .. }
            | StoreRequest::LegacyImport { .. } | StoreRequest::Pending
            | StoreRequest::Reopen | StoreRequest::Switcher | StoreRequest::WorkspaceOrder
            | StoreRequest::Maintenance { apply: false, .. });
        let listing = !pending && matches!(&request, StoreRequest::List);
        let response = if listing && let Some(reply) = &cached.list {
            reply.clone()
        } else {
            let response = if pending {
                let StoreRequest::Commit { batch } = request else {
                    return Err(StoreError::invalid("Expected a workspace delivery."));
                };
                cached.database.prepare_delivery(&batch)?;
                StoreResponse::Done
            } else {
                let (database, response) = cached.database.execute_owned(request, now.max(0.) as u64)?;
                cached.database = database;
                response
            };
            serde_json::to_string(&response)?
        };
        if listing { cached.list = Some(response.clone()); }
        // Read transactions never publish a replacement snapshot. Mutations
        // invalidate cached validation even if their catalog looks unchanged.
        let encoded = if read_only { None } else {
            cached.list = None;
            Some(cached.database.encoded()?)
        };
        let result = serialize(&DatabaseReply { snapshot: encoded.as_deref(), response }).map_err(store_error)?;
        if let Some(encoded) = encoded {
            cached.bytes = encoded.len();
            cached.source = js_sys::Reflect::get(&result, &JsValue::from_str("snapshot")).map_err(store_error)?;
        }
        if cached.bytes <= 8 * 1024 * 1024 {
            DATABASE_CACHE.with(|cache| *cache.borrow_mut() = Some(cached));
        }
        Ok(result)
    })();
    result.map_err(|e| JsValue::from_str(&serde_json::to_string(&e).unwrap()))
}
#[wasm_bindgen]
impl WebApp {
    pub fn workspace_start(
        &mut self,
        execute: js_sys::Function,
        owner: String,
        has_legacy: bool,
        legacy_error: Option<String>,
    ) -> Result<(), JsValue> {
        if self.workspaces.is_some() {
            return Ok(());
        }
        let capture = self.session.capture_workspace().map_err(js)?;
        self.session.set_workspace_read_only(true);
        self.workspaces = Some(WorkspaceController::new_owned(
            BrowserStore(execute),
            layer_ui::Platform::Web,
            "web:layer.workspace.v1".into(),
            has_legacy.then_some(capture),
            serde_json::from_str(&owner).map_err(js)?,
            js_sys::Date::now() as u64,
        ));
        if let Some(error) = legacy_error {
            self.workspaces
                .as_mut()
                .unwrap()
                .legacy_error(error, js_sys::Date::now() as u64);
        }
        Ok(())
    }
    pub fn workspace_observe(&mut self) {
        if let Some(c) = &mut self.workspaces {
            c.observe(&mut self.session, js_sys::Date::now() as u64);
        }
    }
    pub fn workspace_tick(&mut self) -> Result<JsValue, JsValue> {
        let change = self
            .workspaces
            .as_mut()
            .map(|c| c.tick(&mut self.session, js_sys::Date::now() as u64))
            .unwrap_or_default();
        serialize(&change)
    }
    pub fn workspace_view(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.workspaces.as_ref().map(|c| &c.view)).map_err(js)
    }
    pub fn workspace_input(&mut self, input: String) -> Result<JsValue, JsValue> {
        let input: WorkspaceInput = serde_json::from_str(&input).map_err(js)?;
        let c = self
            .workspaces
            .as_mut()
            .ok_or_else(|| js("Workspace storage is not ready."))?;
        match c.input(&mut self.session, input, js_sys::Date::now() as u64) {
            Ok(change) => serialize(&change),
            Err(error) => {
                c.view.error = Some(error.to_string());
                serialize(&layer_ui::UiChange::default())
            }
        }
    }
    pub fn workspace_capture(&mut self) -> Result<String, JsValue> {
        serde_json::to_string(&self.session.capture_workspace().map_err(js)?).map_err(js)
    }
}
