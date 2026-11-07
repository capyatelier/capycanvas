//! Preferences live in the worker's atomic origin store, outside workspace/UI snapshots.
use super::*;
use wasm_bindgen_futures::{JsFuture, future_to_promise};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SourcePickerContext {
    Request { id: u32 },
    Interpretation { source: layer_core::color::source::SourceInterpretation },
}
#[derive(Serialize, Deserialize)]
struct PickerProfile { profile: layer_core::color::ColorProfile, name: Option<String> }
#[wasm_bindgen]
impl WebApp {
    pub fn validate_profile_picker(&self, profile: JsValue, purpose: JsValue, context: JsValue) -> Result<js_sys::Promise, JsValue> {
        use layer_ui::profile_library::{ProfilePurpose,ProfilePickerPurpose};
        let profile: PickerProfile = serde_wasm_bindgen::from_value(profile).map_err(js)?;
        let purpose: ProfilePickerPurpose = serde_wasm_bindgen::from_value(purpose).map_err(js)?;
        let purpose = match purpose {
            ProfilePickerPurpose::Proof => ProfilePurpose::Proof,
            ProfilePickerPurpose::Output => ProfilePurpose::Output,
            ProfilePickerPurpose::Source(_) => ProfilePurpose::Source(match serde_wasm_bindgen::from_value(context).map_err(js)? {
                SourcePickerContext::Request {id} => layer_ui::SourceWorkflow::begin(&self.session,id).map_err(js)?.original.interpretation.clone(),
                SourcePickerContext::Interpretation {source} => source,
            }),
        };
        let working = self.session.engine().document().composition().color.space;
        let metadata = serde_json::to_string(&serde_json::json!({"profile":profile,"purpose":purpose,"working":working})).map_err(js)?;
        Ok(future_to_promise(async move {
            JsFuture::from(raster_worker::call("profile-validate",&metadata,&js_sys::Array::new())?).await
                .map_err(|error|color_feature_rejection(color_feature_reason(error)))
        }))
    }
    pub fn profile_picker_inventory(&self, current: JsValue, document: JsValue, purpose: JsValue, context: JsValue, saved: JsValue) -> Result<js_sys::Promise, JsValue> {
        let current: Option<PickerProfile> = serde_wasm_bindgen::from_value(current).map_err(js)?;
        let document: Option<PickerProfile> = serde_wasm_bindgen::from_value(document).map_err(js)?;
        let context: Option<SourcePickerContext> = serde_wasm_bindgen::from_value(context).map_err(js)?;
        let current = match (current,context) {
            (None,Some(SourcePickerContext::Request {id}))=>Some(layer_ui::SourceWorkflow::begin(&self.session,id).map_err(js)?.initial_profile()),
            (current,_)=>current.map(|profile|profile.profile),
        };
        let purpose: layer_ui::profile_library::ProfilePickerPurpose=serde_wasm_bindgen::from_value(purpose).map_err(js)?;
        let saved: Vec<layer_ui::profile_library::ProfilePickerSaved>=serde_wasm_bindgen::from_value(saved).map_err(js)?;
        let metadata=serde_json::to_string(&serde_json::json!({"current":current,"document":document.map(|profile|profile.profile),"purpose":purpose,"saved":saved})).map_err(js)?;
        Ok(future_to_promise(async move {JsFuture::from(raster_worker::call("profile-inventory",&metadata,&js_sys::Array::new())?).await}))
    }
    pub fn profile_picker_standard(&self, purpose: JsValue) -> Result<JsValue, JsValue> {
        let purpose: layer_ui::profile_library::ProfilePickerPurpose = serde_wasm_bindgen::from_value(purpose).map_err(js)?;
        serialize(&layer_core::color::RgbSpace::ALL.into_iter().filter(|_|purpose.accepts(layer_core::color::ProfileChannels::Rgb))
            .map(layer_ui::ExportProfile::builtin).collect::<Vec<_>>())
    }
    pub fn workspace_name_validation(&self, name: &str) -> Result<JsValue, JsValue> {
        serialize(&layer_workspace::name_validation(name, self.session.localization()))
    }
    pub fn profile_library_matches(&self, entry: JsValue, query: &str) -> Result<bool, JsValue> {
        let entry: layer_ui::profile_library::ProfileEntry = serde_wasm_bindgen::from_value(entry).map_err(js)?;
        Ok(entry.matches(query, self.session.localization()))
    }
    pub fn profile_entry_copy(&self, entry: JsValue) -> Result<JsValue, JsValue> {
        let entry: layer_ui::profile_library::ProfileEntry = serde_wasm_bindgen::from_value(entry).map_err(js)?;
        serialize(&entry.localized_view(self.session.localization()))
    }
    pub fn profile_name_copy(&self, name: &str) -> String {
        layer_ui::ExportProfileCaption::for_name(name.to_owned()).message(self.session.localization())
    }
    pub fn profile_options_copy(&self, name: &str) -> String {
        layer_ui::color_feature_copy::named(self.session.localization(),layer_ui::MessageId::COLOR_FEATURES_PROFILE_OPTIONS,name)
    }
    pub fn source_profile_name_copy(&self, name: JsValue) -> Result<String, JsValue> {
        let name = serde_wasm_bindgen::from_value(name).map_err(js)?;
        Ok(layer_ui::profile_library::profile_description_name(name, self.session.localization()))
    }
    pub fn proof_choices_copy(&self) -> Result<JsValue, JsValue> {
        serialize(&serde_json::json!({"intents":layer_ui::proof_panel::proof_intents(self.session.localization()),"simulations":layer_ui::proof_panel::proof_simulations(self.session.localization())}))
    }
    pub fn color_feature_error_copy(&self, error: JsValue, profile: Option<bool>) -> String {
        if js_sys::Reflect::has(&error, &js("document_host_error")).unwrap_or(false) {
            if let Some(reason) = documents::host_error_reason(&error) { return reason.message(self.session.localization()); }
        }
        if js_sys::Reflect::has(&error, &js("color_feature_error")).unwrap_or(false) {
            let reason = color_feature_reason(error);
            match profile { Some(true)=>reason.profile_message(self.session.localization()), Some(false)=>reason.preset_message(self.session.localization()), None=>reason.message(self.session.localization()) }
        } else {
            let text=diagnostic_text(&error);
            layer_ui::refusal_message(&text).map_or(text,|message|self.session.localization().text(message).to_string())
        }
    }
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
                return Err(color_feature_rejection(layer_ui::ColorFeatureError::ProfileReadLimit));
            }
            // The public caller retains its profile bytes; transfer a private copy.
            buffers.push(&bytes.slice(0, bytes.length()));
        }
        let metadata = serde_json::to_string(&serde_json::json!({"operation":operation,"id":id}))
            .map_err(js)?;
        let operation = operation.to_owned();
        let localizer = self.session.localization().clone();
        Ok(future_to_promise(async move {
            let result = JsFuture::from(raster_worker::call("profile-library", &metadata, &buffers)?).await.map_err(|error|color_feature_rejection(color_feature_reason(error)))?;
            if operation == "list" {
                let mut entries: serde_json::Value = serde_wasm_bindgen::from_value(result).map_err(js)?;
                if let Some(entries) = entries.as_array_mut() {
                    for value in entries {
                        let entry: layer_ui::profile_library::ProfileEntry = serde_json::from_value(value.clone()).map_err(js)?;
                        value["entry_copy_source"] = value.clone();
                        let copy = entry.localized_view(&localizer);
                        for field in ["name", "details", "issue"] { value[field] = copy[field].clone(); }
                    }
                }
                serialize(&entries)
            } else if operation == "get" || operation == "import" {
                let profile: layer_ui::ExportProfile = serde_wasm_bindgen::from_value(result).map_err(js)?;
                serialize(&profile)
            } else { Ok(result) }
        }))
    }
    pub fn load_export_dialog(&self)->Result<js_sys::Promise,JsValue> {
        let color=self.session.engine().document().composition().color;
        let metadata=serde_json::to_string(&serde_json::json!({"operation":"load","color":color})).map_err(js)?;
        Ok(future_to_promise(async move {
            JsFuture::from(raster_worker::call("export-presets",&metadata,&js_sys::Array::new())?).await
                .map_err(|error|color_feature_rejection(color_feature_reason(error)))
        }))
    }
    pub fn begin_export_preset_write(&self,id:u32,operation:JsValue)->Result<WebExportPresetWrite,JsValue> {
        self.session.export_dialog_view(id).map_err(js)?;
        let operation=serde_wasm_bindgen::from_value(operation).map_err(js)?;
        Ok(WebExportPresetWrite {id,write:self.session.export_dialog_preset_write(id,operation).map_err(color_feature_rejection)?})
    }
    pub fn finish_export_preset_write(&mut self,id:u32,candidate:WebExportPresetWrite,error:JsValue)->Result<JsValue,JsValue> {
        if candidate.id!=id {return Err(color_feature_rejection(layer_ui::ColorFeatureError::ExportCombination));}
        let result=if error.is_null()||error.is_undefined() {Ok(candidate.write)} else {Err(color_feature_reason(error))};
        serialize(&self.session.export_dialog_action(id,layer_ui::ExportDialogAction::PresetResult {result}).map_err(js)?)
    }
    pub fn remember_export_dialog(&self,id:u32)->Result<js_sys::Promise,JsValue> {
        let (expected,next)=self.session.export_dialog_remember_write(id).map_err(color_feature_rejection)?;
        store_export_presets(&expected,&next)
    }

}
#[wasm_bindgen]
pub fn raster_worker_profile_inventory(metadata: &str) -> Result<JsValue, JsValue> {
    #[derive(Deserialize)]
    struct Request { current:Option<layer_core::color::ColorProfile>,document:Option<layer_core::color::ColorProfile>,purpose:layer_ui::profile_library::ProfilePickerPurpose,saved:Vec<layer_ui::profile_library::ProfilePickerSaved> }
    let request: Request=serde_json::from_str(metadata).map_err(js)?;
    serialize(&layer_ui::profile_library::picker_inventory(&request.purpose,request.document.as_ref(),request.current.as_ref(),&request.saved))
}
#[wasm_bindgen]
pub fn raster_worker_validate_profile(metadata: &str) -> Result<JsValue, JsValue> {
    #[derive(Deserialize)]
    struct Request { profile: PickerProfile, purpose: layer_ui::profile_library::ProfilePurpose, working: layer_core::color::RgbSpace }
    let request: Request = serde_json::from_str(metadata).map_err(js)?;
    let channels = layer_color::profile_channels(&request.profile.profile).map_err(|error|color_feature_rejection(error.into()))?;
    let name = match request.profile.name { Some(name)=>name, None=>layer_color::profile_description_optional(&request.profile.profile).map_err(|error|color_feature_rejection(error.into()))?.unwrap_or_default() };
    let profile = layer_ui::ExportProfile { profile:request.profile.profile,name,channels };
    request.purpose.validate(&profile,request.working).map_err(color_feature_rejection)?;
    serialize(&profile)
}
#[wasm_bindgen]
pub struct WebExportPresetWrite {id:u32,write:layer_ui::ExportDialogPresetWrite}
#[wasm_bindgen]
impl WebExportPresetWrite {
    pub fn store(&self)->Result<js_sys::Promise,JsValue> {store_export_presets(&self.write.expected,&self.write.next)}
}
fn store_export_presets(expected:&layer_ui::ExportPresets,next:&layer_ui::ExportPresets)->Result<js_sys::Promise,JsValue> {
    let buffers=js_sys::Array::new();
    for library in [expected,next] {buffers.push(&js_sys::Uint8Array::from(library.encode().map_err(color_feature_rejection)?.as_slice()));}
    Ok(future_to_promise(async move {
        JsFuture::from(raster_worker::call("export-presets",r#"{"operation":"write"}"#,&buffers)?).await
            .map_err(|error|color_feature_rejection(color_feature_reason(error)))?;
        Ok(JsValue::UNDEFINED)
    }))
}
#[wasm_bindgen]
pub fn raster_worker_export_presets(metadata:&str,buffers:js_sys::Array)->Result<JsValue,JsValue> {
    #[derive(Deserialize)]
    #[serde(tag="operation",rename_all="snake_case")]
    enum Request {Load {color:layer_core::color::DocumentColor},Write}
    let bytes=js_sys::Uint8Array::new(&buffers.get(0));
    let library=layer_ui::ExportPresets::restore(&bytes.to_vec());
    match serde_json::from_str(metadata).map_err(js)? {
        Request::Load {color}=>{
            library.validate_profiles(color).map_err(color_feature_rejection)?;
            Ok(bytes.into())
        }
        Request::Write=>{
            let expected=js_sys::Uint8Array::new(&buffers.get(1)).to_vec();
            let next=js_sys::Uint8Array::new(&buffers.get(2));
            layer_ui::ExportPresets::replace_encoded(&bytes.to_vec(),&expected,&next.to_vec()).map_err(color_feature_rejection)?;
            Ok(next.into())
        }
    }
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
pub(crate) fn diagnostic_text(error: &JsValue) -> String {
    if let Some(text) = error.as_string() { return text; }
    if let Some(error) = error.dyn_ref::<js_sys::Error>() {
        return error.message().as_string().unwrap_or_default();
    }
    if let Some(error) = error.dyn_ref::<web_sys::DomException>() { return error.message(); }
    serde_wasm_bindgen::from_value::<serde_json::Value>(error.clone())
        .map(|value|value.to_string()).unwrap_or_else(|_|"[object Object]".into())
}
pub(crate) fn color_feature_reason(error: JsValue) -> layer_ui::ColorFeatureError {
    let payload = js_sys::Reflect::get(&error, &js("color_feature_error")).unwrap_or(JsValue::NULL);
    serde_wasm_bindgen::from_value::<layer_ui::ColorFeatureError>(payload).unwrap_or_else(|_|layer_ui::ColorFeatureError::Diagnostic(diagnostic_text(&error)))
}
