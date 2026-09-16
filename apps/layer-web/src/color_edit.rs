//! Worker conversion + private GPU preparation; only publication borrows the
//! live session. Retained original photographs are never copied to the worker.
use super::*;
use layer_core::{ColorTransition, PreparedColorTransition, Project};
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotPreview};
use layer_ui::{DocumentColorOperation, DocumentRequest, HostRequestKind};
use wasm_bindgen_futures::{JsFuture, future_to_promise};

#[wasm_bindgen]
pub struct WebColorCandidate {
    project: Project,
    transition: Option<PreparedColorTransition>,
    renderer: Option<WgpuRasterizer>,
    request: u32,
    epoch: u64,
    revision: u64,
    lost: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    control: CaptureControl,
    previews: Vec<SnapshotPreview>,
    clipped: u64,
    copy: bool,
}
#[wasm_bindgen]
impl WebColorCandidate {
    pub fn previews(&self) -> Result<JsValue, JsValue> {
        let result = js_sys::Array::new();
        for preview in &self.previews {
            let entry = js_sys::Object::new();
            js_sys::Reflect::set(&entry, &js("extent"), &serialize(&preview.extent)?)?;
            js_sys::Reflect::set(
                &entry,
                &js("pixels"),
                &js_sys::Uint8Array::from(preview.srgb_bytes().map_err(js)?.as_slice()),
            )?;
            result.push(&entry);
        }
        Ok(result.into())
    }
    pub fn cancel(&self) {
        self.control.cancel();
    }
    pub fn is_copy(&self) -> bool {
        self.copy
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
        s.require_document_idle().map_err(js)?;
        let request = s
            .state()
            .requests
            .iter()
            .find(|r| r.id == id)
            .ok_or_else(|| js("Color request is no longer active"))?;
        let change: Option<layer_color::DocumentColorChange> =
            serde_wasm_bindgen::from_value(choice).map_err(js)?;
        let (transition, candidate) = match &request.kind {
            HostRequestKind::Document {
                request: DocumentRequest::ColorHistory { redo },
            } => {
                let (prepared, project) = s
                    .prepare_document_color_transition(if *redo {
                        ColorTransition::Redo
                    } else {
                        ColorTransition::Undo
                    })
                    .map_err(js)?;
                (Some(prepared), Some(project))
            }
            HostRequestKind::Document {
                request: DocumentRequest::ChangeColor { operation },
            } => {
                if !matches!(
                    (operation, change),
                    (
                        DocumentColorOperation::Assign,
                        Some(layer_color::DocumentColorChange::Assign(_))
                    ) | (
                        DocumentColorOperation::Convert,
                        Some(layer_color::DocumentColorChange::Convert { .. })
                    ) | (
                        DocumentColorOperation::Depth,
                        Some(layer_color::DocumentColorChange::Depth { .. })
                    )
                ) {
                    return Err(js("Color choice does not match the request"));
                }
                (None, None)
            }
            _ => return Err(js("Not a document color request")),
        };
        if copy
            && !matches!(
                change,
                Some(layer_color::DocumentColorChange::Convert { .. })
            )
        {
            return Err(js("Only color conversion can create a flattened copy"));
        }
        if copy && transition.is_some() {
            return Err(js("A history operation cannot create a copy"));
        }
        let original = s.capture_project_recovery().map_err(js)?;
        let live = s
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or_else(|| js("Canvas unavailable"))?;
        let gpu = live.renderer.snapshot_gpu();
        let lost = live.lost.clone();
        let epoch = s.state().document_file.epoch;
        let revision = original.document.revision;
        let mut brush = s.engine().configured_brush().clone();
        let mut view = s.engine().view();
        let time = s.engine().animation_time();
        let control = control.inner.clone();
        Ok(future_to_promise(async move {
            raster_project::wait_backing(&original).await?;
            output::cancelled(&control)?;
            let history = transition.is_some();
            let (project, clipped) = if let Some(project) = candidate {
                (project, 0)
            } else if copy {
                let Some(layer_color::DocumentColorChange::Convert { space, options }) = change
                else {
                    unreachable!()
                };
                let color = layer_core::color::DocumentColor {
                    space,
                    depth: original.document.color.depth,
                };
                let mut recipe = layer_ui::ExportRecipe::further_editing(color);
                recipe.depth = color.depth;
                recipe.encoding.conversion = options;
                let wire = output::render_output(
                    gpu.clone(),
                    layer_ui::DocumentExport {
                        project: original.clone(),
                        background: view.background_rgba_linear,
                        time,
                    },
                    recipe,
                    control.clone(),
                    false,
                    Some(color),
                )
                .await?;
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
                    raster_project::unpack(&metadata, buffers, true).await?,
                    clipped,
                )
            } else {
                let mut transfer = original.clone();
                for layer in &mut transfer.document.layers {
                    if layer.source.as_ref().is_some_and(|s| s.is_original()) {
                        layer.source = None;
                    }
                }
                let wire = raster_project::pack(transfer).await?;
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
                let mut project = raster_project::unpack(&metadata, buffers, true).await?;
                for layer in &mut project.document.layers {
                    if let Some(source) = original
                        .document
                        .layer(layer.id)
                        .and_then(|l| l.source.as_ref())
                        .filter(|s| s.is_original())
                    {
                        layer.source = Some(source.clone());
                    }
                }
                project.validate(Default::default()).map_err(js)?;
                (project, clipped)
            };
            let old_background = view.background_rgba_linear;
            let matrix = original
                .document
                .color
                .space
                .linear_transform(project.document.color.space);
            let transform = |color: &mut [f32; 4]| {
                let rgb = layer_core::color::rgb::apply(
                    matrix,
                    [color[0], color[1], color[2]].map(f64::from),
                );
                color[..3].copy_from_slice(&rgb.map(|v| v as f32));
            };
            transform(&mut brush.color_rgba_linear);
            transform(&mut brush.color_dynamics.secondary_color_rgba_linear);
            transform(&mut view.background_rgba_linear);
            let mut previews = Vec::new();
            if !history {
                for (source, background) in [
                    (original, old_background),
                    (project.clone(), view.background_rgba_linear),
                ] {
                    let mut snapshot = gpu
                        .capture(
                            source,
                            background,
                            time,
                            Default::default(),
                            control.clone(),
                        )
                        .map_err(js)?;
                    previews.push(
                        snapshot
                            .preview_document_async([512, 384], layer_core::color::RgbSpace::Srgb)
                            .await
                            .map_err(js)?,
                    );
                }
            }
            let renderer = if copy {
                None
            } else {
                let mut canvas = gpu
                    .color_canvas(project.clone(), &brush, view, time, control.clone())
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
            Ok(WebColorCandidate {
                project,
                transition,
                renderer,
                request: id,
                epoch,
                revision,
                lost,
                control,
                previews,
                clipped,
                copy,
            }
            .into())
        }))
    }
    pub fn save_color_copy(
        &self,
        candidate: &WebColorCandidate,
    ) -> Result<js_sys::Promise, JsValue> {
        if !candidate.copy || candidate.previews.len() != 2 {
            return Err(js("Preview a flattened copy before saving"));
        }
        output::cancelled(&candidate.control)?;
        let project = candidate.project.clone();
        let control = candidate.control.clone();
        Ok(future_to_promise(async move {
            let bytes = raster_project::save(project).await?;
            output::cancelled(&control)?;
            Ok(bytes.into())
        }))
    }
    pub fn adopt_color(&mut self, mut candidate: WebColorCandidate) -> Result<JsValue, JsValue> {
        if candidate.copy {
            return Err(js("Save the converted copy as a separate document"));
        }
        let s = &mut self.session;
        output::cancelled(&candidate.control)?;
        s.require_document_idle().map_err(js)?;
        let live = s
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or_else(|| js("Canvas unavailable"))?;
        if !std::sync::Arc::ptr_eq(&live.lost, &candidate.lost)
            || live.lost.lock().unwrap().is_some()
            || s.state().document_file.epoch != candidate.epoch
            || s.engine().document().revision != candidate.revision
            || !s.state().requests.iter().any(|r| r.id == candidate.request)
        {
            return Err(js(
                "The document or canvas changed; prepare the color change again",
            ));
        }
        let prepared = if let Some(prepared) = candidate.transition.take() {
            prepared
        } else {
            s.prepare_document_color_transition(ColorTransition::Apply {
                color: candidate.project.document.color,
                layers: candidate.project.document.layers,
            })
            .map_err(js)?
            .0
        };
        let mut renderer = candidate
            .renderer
            .take()
            .ok_or_else(|| js("Color canvas is not ready"))?;
        let [width, height] = s.state().camera.viewport;
        renderer.resize_surface(width, height).map_err(js)?;
        let live = s.renderer_mut().0.as_mut().unwrap();
        let retired = std::mem::replace(&mut live.renderer, renderer);
        if let Err(error) = s.commit_document_color_transition(prepared) {
            s.renderer_mut().0.as_mut().unwrap().renderer = retired;
            return Err(js(error));
        }
        let live = s.renderer_mut().0.as_mut().unwrap();
        live.presenter = ViewportPresenter::for_renderer(&live.renderer, live.config.format);
        self.deferred_contacts.clear();
        self.prepare_startup()?;
        serialize(
            &self
                .session
                .complete_document_request(candidate.request, Ok(true))
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
    let project = raster_project::unpack(&request.project, buffers, false).await?;
    let converted =
        layer_color::prepare_document_color(&project, request.change, 512 * 1024 * 1024, || false)
            .map_err(js)?;
    let wire = raster_project::pack(converted.project).await?;
    js_sys::Reflect::set(
        &wire,
        &js("clipped"),
        &JsValue::from_f64(converted.statistics.clipped_channels as f64),
    )?;
    Ok(wire)
}
