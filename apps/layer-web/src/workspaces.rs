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
/// Called synchronously from an IndexedDB request callback; no promise/await can
/// yield the transaction between the read, comparisons, and atomic publication.
#[wasm_bindgen]
pub fn workspace_database(
    snapshot: Option<String>,
    request: String,
    pending: bool,
    now: f64,
) -> Result<String, JsValue> {
    let result = (|| -> Result<String, StoreError> {
        let mut database = snapshot
            .map(|s| BrowserDatabase::decode(&s))
            .transpose()?
            .unwrap_or_default();
        let request: StoreRequest = serde_json::from_str(&request)?;
        let response = if pending {
            let StoreRequest::Commit { batch } = request else {
                return Err(StoreError::invalid("Expected a workspace delivery."));
            };
            database.prepare_delivery(&batch)?;
            StoreResponse::Done
        } else {
            database.execute(request, now.max(0.) as u64)?
        };
        Ok(serde_json::json!({ "snapshot": database.encoded()?, "response": serde_json::to_string(&response)? }).to_string())
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
