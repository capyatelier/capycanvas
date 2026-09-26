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
                return Err(js("ICC profile exceeds 16 MiB"));
            }
            // The public caller retains its profile bytes; transfer a private copy.
            buffers.push(&bytes.slice(0, bytes.length()));
        }
        let metadata = serde_json::to_string(&serde_json::json!({"operation":operation,"id":id}))
            .map_err(js)?;
        Ok(future_to_promise(async move {
            JsFuture::from(raster_worker::call("profile-library", &metadata, &buffers)?).await
        }))
    }
    pub fn export_presets(&self, action: JsValue) -> Result<js_sys::Promise, JsValue> {
        let action: layer_ui::ExportPresetAction =
            serde_wasm_bindgen::from_value(action).map_err(js)?;
        let metadata = serde_json::to_string(
            &serde_json::json!({"action":action,"color":self.session.engine().document().color}),
        )
        .map_err(js)?;
        Ok(future_to_promise(async move {
            JsFuture::from(raster_worker::call(
                "export-presets",
                &metadata,
                &js_sys::Array::new(),
            )?)
            .await
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
    let mut library = if bytes.length() == 0 {
        layer_ui::ExportPresets::default()
    } else {
        layer_ui::ExportPresets::decode(&bytes.to_vec()).map_err(js)?
    };
    let view = library
        .operate(request.action, request.color, |recipe| {
            recipe.validate()?;
            if layer_color::profile_channels(&recipe.profile.profile)? != recipe.profile.channels {
                return Err("Profile channels do not match the ICC data".into());
            }
            layer_color::WorkingEncoder::new(
                request.color.space,
                &recipe.interpretation(),
                recipe.encoding,
            )?;
            Ok(())
        })
        .map_err(js)?;
    let result = js_sys::Object::new();
    js_sys::Reflect::set(&result, &js("view"), &serialize(&view)?)?;
    if view.changed {
        js_sys::Reflect::set(
            &result,
            &js("bytes"),
            &js_sys::Uint8Array::from(library.encode().map_err(js)?.as_slice()),
        )?;
    }
    Ok(result.into())
}

#[wasm_bindgen]
pub fn raster_worker_profile_library(request: &str, bytes: js_sys::Uint8Array) -> Result<JsValue, JsValue> {
    let action: layer_ui::profile_library::ProfileLibraryAction = serde_json::from_str(request).map_err(js)?;
    let result = action.execute(&bytes.to_vec()).map_err(js)?;
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
