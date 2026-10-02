//! Preferences live in the worker's atomic origin store, outside workspace/UI snapshots.
use super::*;
use wasm_bindgen_futures::{JsFuture, future_to_promise};
#[wasm_bindgen]
impl WebApp {
    pub fn profile_library(
        &self,
        operation: &str,
        id: Option<String>,
        bytes: Option<js_sys::Uint8Array>,
    ) -> Result<js_sys::Promise, JsValue> {
        if !matches!(operation, "list" | "get" | "import" | "remove" | "show" | "hide") {
            return Err(js("Unknown profile library action"));
        }
        let buffers = js_sys::Array::new();
        if let Some(bytes) = bytes {
            if bytes.length() as usize > layer_color::MAX_ICC_BYTES {
                return Err(js(layer_ui::ColorFeatureError::ProfileReadLimit.profile_message(self.session.localization())));
            }
            // The public caller retains its profile bytes; transfer a private copy.
            buffers.push(&bytes.slice(0, bytes.length()));
        }
        let metadata = serde_json::to_string(&serde_json::json!({"operation":operation,"id":id}))
            .map_err(js)?;
        let operation = operation.to_owned();
        let localizer = self.session.localization().clone();
        Ok(future_to_promise(async move {
            let result = JsFuture::from(raster_worker::call("profile-library", &metadata, &buffers)?).await.map_err(|error|color_feature_failure(error,&localizer,true))?;
            if operation == "list" {
                let mut entries: serde_json::Value = serde_wasm_bindgen::from_value(result).map_err(js)?;
                if let Some(entries) = entries.as_array_mut() {
                    for value in entries {
                        let entry: layer_ui::profile_library::ProfileEntry = serde_json::from_value(value.clone()).map_err(js)?;
                        value["name"] = serde_json::json!(entry.display_name(&localizer));
                        value["details"] = serde_json::json!(entry.details(&localizer));
                        value["issue"] = serde_json::json!(entry.issue.as_ref().map(|reason|reason.profile_message(&localizer)));
                    }
                }
                serialize(&entries)
            } else if operation == "get" || operation == "import" {
                let mut profile: layer_ui::ExportProfile = serde_wasm_bindgen::from_value(result).map_err(js)?;
                if profile.name.is_empty() { profile.name = localizer.text(layer_ui::MessageId::COLOR_FEATURES_PROFILE_EMBEDDED).to_string(); }
                serialize(&profile)
            } else { Ok(result) }
        }))
    }
    pub fn export_presets(&self, action: JsValue) -> Result<js_sys::Promise, JsValue> {
        let action: layer_ui::ExportPresetAction =
            serde_wasm_bindgen::from_value(action).map_err(js)?;
        let metadata = serde_json::to_string(
            &serde_json::json!({"action":action,"color":self.session.engine().document().color}),
        )
        .map_err(js)?;
        let color = self.session.engine().document().color;
        let localizer = self.session.localization().clone();
        Ok(future_to_promise(async move {
            let result = JsFuture::from(raster_worker::call("export-presets", &metadata, &js_sys::Array::new())?).await.map_err(|error|color_feature_failure(error,&localizer,false))?;
            let mut view: layer_ui::ExportPresetView = serde_wasm_bindgen::from_value(result).map_err(js)?;
            view.localize_names(color, &localizer);
            serialize(&view)
        }))
    }
}
#[wasm_bindgen]
pub fn raster_worker_export_presets(
    metadata: &str,
    bytes: js_sys::Uint8Array,
) -> Result<JsValue, JsValue> {
    #[derive(Deserialize)]
    struct Request {
        action: layer_ui::ExportPresetAction,
        color: layer_core::color::DocumentColor,
    }
    let request: Request = serde_json::from_str(metadata).map_err(js)?;
    let mut library = layer_ui::ExportPresets::restore(&bytes.to_vec());
    let view = library
        .operate(request.action, request.color)
        .map_err(color_feature_rejection)?;
    let result = js_sys::Object::new();
    js_sys::Reflect::set(&result, &js("view"), &serialize(&view)?)?;
    if view.changed {
        js_sys::Reflect::set(
            &result,
            &js("bytes"),
            &js_sys::Uint8Array::from(library.encode().map_err(color_feature_rejection)?.as_slice()),
        )?;
    }
    Ok(result.into())
}

#[wasm_bindgen]
pub fn raster_worker_profile_library(request: &str, bytes: js_sys::Uint8Array) -> Result<JsValue, JsValue> {
    let action: layer_ui::profile_library::ProfileLibraryAction = serde_json::from_str(request).map_err(js)?;
    let result = action.execute(&bytes.to_vec()).map_err(color_feature_rejection)?;
    js_sys::JSON::parse(&serde_json::to_string(&result).map_err(js)?)
}

#[wasm_bindgen]
pub fn raster_worker_palette_file(
    request: &str,
    bytes: js_sys::Uint8Array,
) -> Result<js_sys::Array, JsValue> {
    let request: layer_ui::PaletteFileRequest = serde_json::from_str(request).map_err(js)?;
    let (metadata, bytes) = layer_ui::palette_file(request, &bytes.to_vec()).map_err(js)?;
    Ok(js_sys::Array::of2(
        &js_sys::JSON::parse(&serde_json::to_string(&metadata).map_err(js)?)?,
        &js_sys::Uint8Array::from(bytes.as_slice()),
    ))
}

pub(crate) fn color_feature_rejection(reason: layer_ui::ColorFeatureError) -> JsValue {
    let object = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&object, &js("color_feature_error"), &serialize(&reason).unwrap_or(JsValue::NULL));
    object.into()
}
pub(crate) fn color_feature_reason(error: JsValue) -> layer_ui::ColorFeatureError {
    let payload = js_sys::Reflect::get(&error, &js("color_feature_error")).unwrap_or(JsValue::NULL);
    serde_wasm_bindgen::from_value::<layer_ui::ColorFeatureError>(payload).unwrap_or_else(|_|layer_ui::ColorFeatureError::Diagnostic(error.as_string().unwrap_or_else(||js_sys::JsString::from(error).into())))
}
fn color_feature_failure(error: JsValue, localizer: &layer_ui::Localizer, profile: bool) -> JsValue {
    let reason = color_feature_reason(error);
    js(if profile { reason.profile_message(localizer) } else { reason.preset_message(localizer) })
}
