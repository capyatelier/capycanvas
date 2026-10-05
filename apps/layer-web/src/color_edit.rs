//! Worker conversion + private GPU preparation; only publication borrows the
//! live session. Retained original photographs are never copied to the worker.
use super::*;
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotPreview};
use layer_ui::{ColorWorkflow, ColorPreparation};
use wasm_bindgen_futures::{JsFuture, future_to_promise};

#[wasm_bindgen]
pub struct WebColorCandidate {
    workflow: ColorWorkflow,
    renderer: Option<WgpuRasterizer>,
    lost: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    control: CaptureControl,
    previews: Vec<SnapshotPreview>,
    clipped: u64,
}
#[wasm_bindgen]
impl WebColorCandidate {
    pub fn previews(&self) -> Result<JsValue, JsValue> {
        let result = js_sys::Array::new();
        for preview in &self.previews {
            result.push(&output::preview_value(preview)?);
        }
        Ok(result.into())
    }
    pub fn cancel(&self) {
        self.control.cancel();
    }
    pub fn is_copy(&self) -> bool {
        self.workflow.is_copy()
    }
    pub fn clipped_channels(&self) -> f64 {
        self.clipped as f64
    }
}
#[wasm_bindgen]
impl WebApp {
    pub fn prepare_color(
        &self,
        id: u32,
        choice: JsValue,
        control: &output::WebCaptureControl,
        copy: Option<bool>,
    ) -> Result<js_sys::Promise, JsValue> {
        let copy = copy.unwrap_or(false);
        let s = &self.session;
        let mut workflow = ColorWorkflow::begin(s, id).map_err(js)?;
        let change: Option<layer_color::DocumentColorChange> = serde_wasm_bindgen::from_value(choice).map_err(js)?;
        let plan = workflow.select(change, copy).map_err(js)?;
        let original = workflow.original.clone();
        let gpu = s
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or_else(|| js("Canvas unavailable"))?
            .snapshot_gpu();
        let lost = self.gpu_owner().ok_or_else(|| js("Canvas unavailable"))?;
        let mut brush = s.engine().configured_brush().clone();
        let view = s.engine().view();
        let context = workflow.context.clone();
        let time = context.elapsed;
        let control = control.inner.clone();
        Ok(future_to_promise(async move {
            artwork_transfer::wait_backing(&original.artwork).await?;
            output::cancelled(&control)?;
            let history = workflow.is_history();
            let (project, clipped) = if let Some(project) = workflow.candidate.take() {
                (project, 0)
            } else if copy {
                let ColorPreparation::Flatten { color, options } = plan else { unreachable!() };
                let mut recipe = layer_ui::ExportRecipe::further_editing(color);
                recipe.depth = color.depth;
                recipe.encoding.conversion = options;
                let wire = output::render_output(
                    gpu.clone(),
                    layer_ui::DocumentExport {
                        capture: layer_core::Editor::new(original.clone()).capture(0,context.clone()).map_err(js)?,
                        time,
                    },
                    recipe,
                    control.clone(),
                    false,
                    Some(color),
                )
                .await.map_err(|error| crate::color_preferences::color_feature_rejection(crate::color_preferences::color_feature_reason(error)))?;
                output::cancelled(&control)?;
                let metadata = js_sys::Reflect::get(&wire, &js("metadata"))?
                    .as_string()
                    .ok_or_else(|| js("Missing converted copy"))?;
                let buffers =
                    js_sys::Reflect::get(&wire, &js("buffers"))?.dyn_into::<js_sys::Array>()?;
                let clipped = js_sys::Reflect::get(&wire, &js("clipped"))?
                    .as_f64()
                    .unwrap_or(0.) as u64;
                (
                    layer_core::Document::from_artwork(artwork_transfer::unpack(&metadata, buffers).await?).map_err(js)?,
                    clipped,
                )
            } else {
                let transfer = layer_color::color_job_artwork(&original.artwork);
                let wire = artwork_transfer::pack(transfer).await?;
                let metadata = js_sys::Reflect::get(&wire, &js("metadata"))?
                    .as_string()
                    .ok_or_else(|| js("Missing color metadata"))?;
                let buffers =
                    js_sys::Reflect::get(&wire, &js("buffers"))?.dyn_into::<js_sys::Array>()?;
                let request =
                    serde_json::to_string(&serde_json::json!({"project":metadata,"change":change}))
                        .map_err(js)?;
                let result =
                    JsFuture::from(raster_worker::call("color-convert", &request, &buffers)?)
                        .await?;
                output::cancelled(&control)?;
                let metadata = js_sys::Reflect::get(&result, &js("metadata"))?
                    .as_string()
                    .ok_or_else(|| js("Missing color result"))?;
                let buffers =
                    js_sys::Reflect::get(&result, &js("buffers"))?.dyn_into::<js_sys::Array>()?;
                let clipped = js_sys::Reflect::get(&result, &js("clipped"))?
                    .as_f64()
                    .unwrap_or(0.) as u64;
                let artwork = artwork_transfer::unpack(&metadata, buffers).await?;
                let project=layer_color::adopt_color_job_artwork(&original,artwork).map_err(js)?;
                (project, clipped)
            };
            hdr::admit_document(&project)?;
            layer_render::remap_document_colors(original.composition().color.space, project.composition().color.space, &mut brush);
            let mut previews = Vec::new();
            if !history {
                for source in [
                    original,
                    project.clone(),
                ] {
                    previews.push(hdr::preview_document(&gpu,source.artwork,context.clone(),control.clone()).await?);
                }
            }
            let renderer = if copy {
                None
            } else {
                let mut canvas = gpu
                    .color_canvas(project.clone(), context.clone(), &brush, view, control.clone())
                    .map_err(js)?;
                let start = js_sys::Date::now();
                loop {
                    canvas.compile_step().await.map_err(js)?;
                    if canvas.poll().map_err(js)? {
                        break;
                    }
                    if js_sys::Date::now() - start > 120_000. {
                        return Err(js("Color canvas preparation timed out"));
                    }
                    documents::yield_browser().await?;
                }
                let mut renderer = canvas.take_ready().map_err(js)?;
                raster_worker::install(&mut renderer);
                Some(renderer)
            };
            workflow.candidate = Some(project);
            if !history { workflow.comparison_completed().map_err(js)?; }
            Ok(WebColorCandidate {
                workflow,
                renderer,
                lost,
                control,
                previews,
                clipped,
            }
            .into())
        }))
    }
    pub fn save_color_copy(
        &self,
        candidate: &WebColorCandidate,
    ) -> Result<js_sys::Promise, JsValue> {
        let project = layer_core::Editor::new(candidate.workflow.copy_project(candidate.control.is_cancelled()).map_err(js)?.clone()).capture(0,candidate.workflow.context.clone()).map_err(js)?;
        let control = candidate.control.clone();
        Ok(future_to_promise(async move {
            let bytes = artwork_transfer::save(project).await?;
            output::cancelled(&control)?;
            Ok(bytes.into())
        }))
    }
    pub fn adopt_color(&mut self, mut candidate: WebColorCandidate) -> Result<JsValue, JsValue> {
        let lost = self.gpu_owner().ok_or_else(|| js("Canvas unavailable"))?;
        let device_current = std::sync::Arc::ptr_eq(&lost, &candidate.lost) && lost.lock().unwrap().is_none();
        let s = &mut self.session;
        let prepared = candidate.workflow.prepare_commit(s, candidate.control.is_cancelled(), device_current).map_err(js)?;
        let mut renderer = candidate
            .renderer
            .take()
            .ok_or_else(|| js("Color canvas is not ready"))?;
        let [width, height] = s.state().camera.viewport;
        renderer.resize_surface(width, height).map_err(js)?;
        s.commit_document_color_candidate(prepared, |live| {
            std::mem::swap(live.0.as_deref_mut().unwrap(), &mut renderer);
        }).map_err(js)?;
        self.deferred_contacts.clear();
        self.prepare_startup()?;
        serialize(
            &self
                .session
                .complete_document_request(candidate.workflow.identity.request(), Ok(true))
                .map_err(js)?,
        )
    }
}
#[wasm_bindgen]
pub async fn raster_worker_color(
    metadata: &str,
    buffers: js_sys::Array,
) -> Result<JsValue, JsValue> {
    #[derive(Deserialize)]
    struct Request {
        project: String,
        change: layer_color::DocumentColorChange,
    }
    let request: Request = serde_json::from_str(metadata).map_err(js)?;
    let project = layer_core::Document::from_artwork(artwork_transfer::unpack(&request.project, buffers).await?).map_err(js)?;
    let converted =
        layer_color::prepare_document_color(&project, request.change, artwork_transfer::photo_memory_budget().encode_bytes, || false)
            .map_err(js)?;
    let wire = artwork_transfer::pack(converted.document.artwork).await?;
    js_sys::Reflect::set(
        &wire,
        &js("clipped"),
        &JsValue::from_f64(converted.statistics.clipped_channels as f64),
    )?;
    Ok(wire)
}
