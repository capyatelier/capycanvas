//! Retained-source operations keep original tiles shared until explicit Apply.
use super::*;
use layer_core::{
    LayerId, Project,
    color::{
        ColorProfile, DocumentColor,
        source::{SourceImage, SourceInterpretation},
    },
};
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotGpu, SnapshotPreview};
use layer_ui::{DocumentRequest, HostRequestKind};
use std::sync::Arc;
use wasm_bindgen_futures::{JsFuture, future_to_promise};

#[wasm_bindgen]
pub struct WebSourceCandidate {
    original: Arc<SourceImage>,
    converted: Arc<SourceImage>,
    project: Project,
    gpu: SnapshotGpu,
    control: CaptureControl,
    lost: Arc<std::sync::Mutex<Option<String>>>,
    background: [f32; 4],
    time: f32,
    epoch: u64,
    revision: u64,
    request: u32,
    layer: LayerId,
    rasterize: bool,
    baked: bool,
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
        self.baked && !self.rasterize
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
        if c.control.is_cancelled()
            || !Arc::ptr_eq(&live.lost, &c.lost)
            || live.lost.lock().unwrap().is_some()
            || s.state().document_file.epoch != c.epoch
            || s.engine().document().revision != c.revision
            || !s.state().requests.iter().any(|r| r.id == c.request)
        {
            return Err(js(
                "The document or canvas changed; prepare the source change again",
            ));
        }
        Ok(())
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
        s.require_document_idle().map_err(js)?;
        let profile: Option<ColorProfile> = serde_wasm_bindgen::from_value(profile).map_err(js)?;
        let (layer, rasterize) = s
            .state()
            .requests
            .iter()
            .find_map(|r| {
                if r.id == id {
                    match &r.kind {
                        HostRequestKind::Document {
                            request: DocumentRequest::RepairSourceProfile { layer },
                        } => Some((LayerId(*layer), false)),
                        HostRequestKind::Document {
                            request: DocumentRequest::RasterizeSource { layer },
                        } => Some((LayerId(*layer), true)),
                        _ => None,
                    }
                } else {
                    None
                }
            })
            .ok_or_else(|| js("Source request is no longer active"))?;
        let project = s.capture_project_recovery().map_err(js)?;
        let l = project
            .document
            .layer(layer)
            .ok_or_else(|| js("Source layer no longer exists"))?;
        let original = l.source.clone().ok_or_else(|| js("No retained source"))?;
        let baked = !l.raster.is_empty() || !l.pending_operations.is_empty() || l.asset.is_some();
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
        let epoch = s.state().document_file.epoch;
        let revision = project.document.revision;
        Ok(future_to_promise(async move {
            output::cancelled(&control)?;
            let (converted, clipped, name) = if rasterize {
                if profile.is_some() {
                    return Err(js("Rasterization uses the document profile"));
                }
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
                let metadata=serde_json::to_string(&serde_json::json!({"interpretation":original.interpretation,"color":project.document.color,"profile":profile.ok_or_else(||js("Choose a source profile"))?})).map_err(js)?;
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
                original,
                converted,
                project,
                gpu,
                control,
                lost,
                background,
                time,
                epoch,
                revision,
                request: id,
                layer,
                rasterize,
                baked,
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
        let candidate = if c.rasterize {
            self.session
                .preview_rasterized_source(c.layer, &c.original, c.converted.clone())
        } else {
            self.session
                .preview_layer_source(c.layer, &c.original, (*c.converted).clone())
        }
        .map_err(js)?;
        Ok(future_to_promise(async move {
            for project in [&c.project, &candidate] {
                let mut snapshot = c
                    .gpu
                    .capture(
                        project.clone(),
                        c.background,
                        c.time,
                        Default::default(),
                        c.control.clone(),
                    )
                    .map_err(js)?;
                c.previews.push(
                    snapshot
                        .preview_document_async([512, 384], layer_core::color::RgbSpace::Srgb)
                        .await
                        .map_err(js)?,
                );
            }
            output::cancelled(&c.control)?;
            Ok(c.into())
        }))
    }
    pub fn adopt_source(&mut self, c: WebSourceCandidate) -> Result<JsValue, JsValue> {
        self.validate_source_candidate(&c)?;
        if c.previews.len() != 2 {
            return Err(js("Preview the complete source result first"));
        }
        if c.rasterize {
            self.session
                .apply_rasterized_source(c.layer, &c.original, c.converted)
                .map_err(js)?
        } else {
            self.session
                .repair_layer_source(c.layer, &c.original, (*c.converted).clone())
                .map_err(js)?;
        }
        let mut change = self
            .session
            .complete_document_request(c.request, Ok(true))
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
    request.interpretation.profile = request.profile;
    request.interpretation.profile_assumed = false;
    layer_color::WorkingDecoder::new(
        &request.interpretation,
        request.color.space,
        Default::default(),
    )
    .map_err(js)?;
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
