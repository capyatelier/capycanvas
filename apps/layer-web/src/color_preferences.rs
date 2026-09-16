//! Preferences live in the worker's atomic origin store, outside workspace/UI snapshots.
use super::*;
use wasm_bindgen_futures::{JsFuture, future_to_promise};
#[wasm_bindgen]
impl WebApp {
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
