//! Retained-source operations keep original tiles shared until explicit Apply.
use super::*;
use layer_core::{
    Project,
    color::{
        ColorProfile, DocumentColor,
        source::{SourceImage, SourceInterpretation},
    },
};
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotGpu, SnapshotPreview};
use layer_ui::SourceWorkflow;
use std::sync::Arc;
use wasm_bindgen_futures::{JsFuture, future_to_promise};

#[wasm_bindgen]
pub struct WebSourceCandidate {
    workflow: SourceWorkflow,
    converted: Arc<SourceImage>,
    gpu: SnapshotGpu,
    control: CaptureControl,
    lost: Arc<std::sync::Mutex<Option<String>>>,
    background: [f32; 4],
    time: f32,
    clipped: u64,
    profile: String,
    previews: Vec<SnapshotPreview>,
}
#[wasm_bindgen]
impl WebSourceCandidate {
    pub fn previews(&self) -> Result<JsValue, JsValue> {
        let result = js_sys::Array::new();
        for p in &self.previews {
            let entry = js_sys::Object::new();
            js_sys::Reflect::set(&entry, &js("extent"), &serialize(&p.extent)?)?;
            js_sys::Reflect::set(
                &entry,
                &js("pixels"),
                &js_sys::Uint8Array::from(p.srgb_bytes().map_err(js)?.as_slice()),
            )?;
            result.push(&entry);
        }
        Ok(result.into())
    }
    pub fn clipped_channels(&self) -> f64 {
        self.clipped as f64
    }
    pub fn adds_layer(&self) -> bool {
        self.workflow.adds_layer()
    }
    pub fn source_profile(&self) -> String {
        self.profile.clone()
    }
}
impl WebApp {
    fn validate_source_candidate(&self, c: &WebSourceCandidate) -> Result<(), JsValue> {
        let s = &self.session;
        let live = s
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or_else(|| js("Canvas unavailable"))?;
        c.workflow.identity.validate(s, c.control.is_cancelled(), Arc::ptr_eq(&live.lost, &c.lost) && live.lost.lock().unwrap().is_none()).map_err(js)
    }
}
#[wasm_bindgen]
impl WebApp {
    pub fn prepare_source(
        &self,
        id: u32,
        profile: JsValue,
        control: &output::WebCaptureControl,
    ) -> Result<js_sys::Promise, JsValue> {
        let s = &self.session;
        let profile: Option<ColorProfile> = serde_wasm_bindgen::from_value(profile).map_err(js)?;
        let workflow = SourceWorkflow::begin(s, id).map_err(js)?;
        workflow.validate_choice(&profile).map_err(js)?;
        let project = workflow.project.clone();
        let original = workflow.original.clone();
        let live = s
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or_else(|| js("Canvas unavailable"))?;
        let gpu = live.renderer.snapshot_gpu();
        let lost = live.lost.clone();
        let control = control.inner.clone();
        let background = s.engine().view().background_rgba_linear;
        let time = s.engine().animation_time();
        Ok(future_to_promise(async move {
            output::cancelled(&control)?;
            let (converted, clipped, name) = if workflow.rasterize() {
                // Only the selected source enters the conversion worker, never
                // the rest of the master or its painted raster backing.
                let mut document =
                    layer_core::Document::new("source", original.extent[0], original.extent[1]);
                document.color = project.document.color;
                document.layers[0].source = Some(original.clone());
                document.layers[1].visible = false;
                let wire = raster_project::pack(Project {
                    document,
                    assets: Default::default(),
                })
                .await?;
                let metadata = js_sys::Reflect::get(&wire, &js("metadata"))?
                    .as_string()
                    .ok_or_else(|| js("Missing source metadata"))?;
                let buffers =
                    js_sys::Reflect::get(&wire, &js("buffers"))?.dyn_into::<js_sys::Array>()?;
                let result = JsFuture::from(raster_worker::call(
                    "source-rasterize",
                    &metadata,
                    &buffers,
                )?)
                .await?;
                output::cancelled(&control)?;
                let metadata = js_sys::Reflect::get(&result, &js("metadata"))?
                    .as_string()
                    .ok_or_else(|| js("Missing source result"))?;
                let buffers =
                    js_sys::Reflect::get(&result, &js("buffers"))?.dyn_into::<js_sys::Array>()?;
                let clipped = js_sys::Reflect::get(&result, &js("clipped"))?
                    .as_f64()
                    .unwrap_or(0.) as u64;
                let name = js_sys::Reflect::get(&result, &js("source_profile"))?
                    .as_string()
                    .unwrap_or_default();
                let converted = raster_project::unpack(&metadata, buffers, true)
                    .await?
                    .document
                    .layers[0]
                    .source
                    .clone()
                    .ok_or_else(|| js("Missing converted source"))?;
                (converted, clipped, name)
            } else {
                let metadata=serde_json::to_string(&serde_json::json!({"interpretation":original.interpretation,"color":project.document.color,"profile":profile.unwrap()})).map_err(js)?;
                let result = JsFuture::from(raster_worker::call(
                    "source-profile",
                    &metadata,
                    &js_sys::Array::new(),
                )?)
                .await?;
                output::cancelled(&control)?;
                let mut source = (*original).clone();
                source.interpretation = serde_wasm_bindgen::from_value(js_sys::Reflect::get(
                    &result,
                    &js("interpretation"),
                )?)
                .map_err(js)?;
                let name = js_sys::Reflect::get(&result, &js("source_profile"))?
                    .as_string()
                    .unwrap_or_default();
                source.validate().map_err(js)?;
                (Arc::new(source), 0, name)
            };
            output::cancelled(&control)?;
            Ok(WebSourceCandidate {
                workflow,
                converted,
                gpu,
                control,
                lost,
                background,
                time,
                clipped,
                profile: name,
                previews: Vec::new(),
            }
            .into())
        }))
    }
    pub fn prepare_source_comparison(
        &self,
        mut c: WebSourceCandidate,
    ) -> Result<js_sys::Promise, JsValue> {
        self.validate_source_candidate(&c)?;
        let candidate = c.workflow.preview(&self.session, c.converted.clone(), c.control.is_cancelled(), true).map_err(js)?;
        Ok(future_to_promise(async move {
            for project in [&c.workflow.project, &candidate] {
                c.previews.push(hdr::preview_document(&c.gpu,project.clone(),c.background,c.time,c.control.clone()).await?);
            }
            output::cancelled(&c.control)?;
            c.workflow.comparison_completed().map_err(js)?;
            Ok(c.into())
        }))
    }
    pub fn adopt_source(&mut self, mut c: WebSourceCandidate) -> Result<JsValue, JsValue> {
        self.validate_source_candidate(&c)?;
        c.workflow.commit(&mut self.session, c.control.is_cancelled(), true).map_err(js)?;
        let mut change = self
            .session
            .complete_document_request(c.workflow.identity.request(), Ok(true))
            .map_err(js)?;
        change.canvas_wake = true;
        serialize(&change)
    }
}
#[wasm_bindgen]
pub fn raster_worker_source_profile(metadata: &str) -> Result<JsValue, JsValue> {
    #[derive(Deserialize)]
    struct Request {
        interpretation: SourceInterpretation,
        color: DocumentColor,
        profile: ColorProfile,
    }
    let mut request: Request = serde_json::from_str(metadata).map_err(js)?;
    let name = layer_color::profile_description(&request.interpretation.profile).map_err(js)?;
    request.interpretation = layer_color::repair_source_interpretation(request.interpretation, request.color.space, request.profile).map_err(js)?;
    serialize(&serde_json::json!({"interpretation":request.interpretation,"source_profile":name}))
}
#[wasm_bindgen]
pub async fn raster_worker_source_rasterize(
    metadata: &str,
    buffers: js_sys::Array,
) -> Result<JsValue, JsValue> {
    let mut project = raster_project::unpack(metadata, buffers, false).await?;
    let source = project.document.layers[0]
        .source
        .as_ref()
        .ok_or_else(|| js("No source"))?;
    let name = layer_color::profile_description(&source.interpretation.profile).map_err(js)?;
    let (converted, statistics) =
        layer_color::rasterize_source(source, project.document.color, 512 * 1024 * 1024, || false)
            .map_err(js)?;
    project.document.layers[0].source = Some(Arc::new(converted));
    let wire = raster_project::pack(project).await?;
    js_sys::Reflect::set(
        &wire,
        &js("clipped"),
        &JsValue::from_f64(statistics.clipped_channels as f64),
    )?;
    js_sys::Reflect::set(&wire, &js("source_profile"), &js(name))?;
    Ok(wire)
}
