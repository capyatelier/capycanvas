//! Browser transport owns prepared candidates and readbacks across event-loop
//! yields. A validated drawing appends without replacing the outgoing editor.
use super::*;
use layer_core::ProjectLimits;
use layer_ui::{DocumentLocation, DocumentRequest, HostRequestKind};
use wasm_bindgen_futures::{JsFuture, future_to_promise};

pub(super) async fn yield_browser() -> Result<(), JsValue> {
    let timer = js_sys::Reflect::get(&js_sys::global(), &js("setTimeout"))?
        .dyn_into::<js_sys::Function>()?;
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        if let Err(e) = timer.call2(&JsValue::NULL, &resolve, &JsValue::from(0)) {
            let _ = reject.call1(&JsValue::NULL, &e);
        }
    });
    JsFuture::from(promise).await.map(|_| ())
}

#[wasm_bindgen]
pub struct WebProject {
    session: Option<Box<UiSession<AttachedRenderer>>>,
    request: u32,
    epoch: u64,
    revision: u64,
    recovered: bool,
    source: layer_ui::ImportSource,
    placed: Option<std::sync::Arc<layer_core::color::source::SourceImage>>,
    source_name: String,
    target: layer_core::LayerId,
    lost: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

#[wasm_bindgen]
impl WebApp {
    pub fn document_properties(&self) -> Result<js_sys::Promise, JsValue> {
        let info = layer_color::DocumentInfo::capture(self.session.engine().document());
        Ok(future_to_promise(async move {
            let metadata = serde_json::to_string(&info).map_err(js)?;
            JsFuture::from(raster_worker::call(
                "properties",
                &metadata,
                &js_sys::Array::new(),
            )?)
            .await
        }))
    }
    pub fn export_form(&self) -> Result<JsValue, JsValue> {
        serialize(&layer_ui::ExportForm::new(self.session.engine().document()))
    }
    pub fn export_draft(&self, recipe: JsValue, action: JsValue) -> Result<JsValue, JsValue> {
        let recipe: layer_ui::ExportRecipe = serde_wasm_bindgen::from_value(recipe).map_err(js)?;
        let action: layer_ui::ExportDraftAction = serde_wasm_bindgen::from_value(action).map_err(js)?;
        serialize(&recipe.draft(action))
    }
    pub fn export_validate(&self, recipe: JsValue) -> Result<JsValue, JsValue> {
        let recipe: layer_ui::ExportRecipe = serde_wasm_bindgen::from_value(recipe).map_err(js)?;
        recipe.validate().map_err(js)?;
        serialize(&recipe)
    }

    pub fn finish_document(
        &mut self,
        id: u32,
        success: bool,
        error: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let result = error.map_or(Ok(success), Err);
        serialize(
            &self
                .session
                .complete_document_request(id, result)
                .map_err(js)?,
        )
    }
    pub fn respond_document(&mut self, id: u32, decision: JsValue) -> Result<JsValue, JsValue> {
        let decision = serde_wasm_bindgen::from_value(decision).map_err(js)?;
        serialize(
            &self
                .session
                .respond_document_close(id, decision)
                .map_err(js)?,
        )
    }
    pub fn reset_document_close(&mut self) {
        self.session.reset_document_close();
    }
    pub fn save_project(&mut self, id: u32, location: JsValue) -> Result<js_sys::Promise, JsValue> {
        let location = serde_wasm_bindgen::from_value(location).map_err(js)?;
        let project = self
            .session
            .capture_project_save(id, location)
            .map_err(js)?;
        Ok(future_to_promise(async move {
            raster_project::save(project).await
        }))
    }
    pub fn recovery_update(&self, state: &str, event: JsValue) -> Result<JsValue, JsValue> {
        let event = serde_wasm_bindgen::from_value(event).map_err(js)?;
        serialize(&layer_ui::recovery::recovery_update(state, event).map_err(js)?)
    }
    pub fn save_recovery(&self, key: String) -> Result<js_sys::Promise, JsValue> {
        let project = self.session.capture_project_recovery().map_err(js)?;
        Ok(future_to_promise(async move {
            raster_project::save_recovery(project, key).await
        }))
    }
    pub fn prepare_document(
        &self,
        id: u32,
        bytes: Option<js_sys::Uint8Array>,
        width: u32,
        height: u32,
        epoch: u64,
        revision: u64,
        recovered: Option<bool>,
        source_name: Option<String>,
        options: JsValue,
        interpret: Option<js_sys::Function>,
        cancelled: Option<js_sys::Function>,
    ) -> Result<js_sys::Promise, JsValue> {
        self.session.require_document_idle().map_err(js)?;
        if self.session.state().document_file.epoch != epoch
            || self.session.engine().document().revision != revision
        {
            return Err(js(
                "The document changed while choosing a file; review those changes first",
            ));
        }
        let recovered = recovered.unwrap_or(false);
        if recovered
            && (id != 0
                || bytes.is_none()
                || self.session.state().document_file.busy)
        {
            return Err(js("Recovery requires an idle drawing"));
        }
        let request = if recovered {
            Some(DocumentRequest::Open)
        } else {
            self.session.state().requests.iter().find_map(|r| {
                if r.id == id {
                    if let HostRequestKind::Document { request } = &r.kind {
                        Some(request.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
        }
        .ok_or_else(|| js("The document request is no longer active"))?;
        if !matches!(
            (&request, &bytes),
            (
                DocumentRequest::Open | DocumentRequest::Place | DocumentRequest::Paste,
                Some(_)
            ) | (DocumentRequest::New, None)
        ) {
            return Err(js("Invalid project preparation request"));
        }
        let placing = matches!(request, DocumentRequest::Place | DocumentRequest::Paste);
        let target = self.session.engine().document().active_target();
        let live = self
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or_else(|| js("Wait for the canvas"))?;
        let (adapter, device, queue) = (
            live.adapter().clone(),
            live.device().clone(),
            live.queue().clone(),
        );
        let lost = self.gpu_owner().ok_or_else(|| js("Wait for the canvas"))?;
        let viewport = self.session.state().camera.viewport;
        let brush = self.session.engine().configured_brush().clone();
        let photo_policy = self.session.state().settings.photo_open;
        let admission = self.documents.admission(&self.session.retained_document_tiles());
        let new_options: layer_ui::NewDocumentOptions =
            if options.is_undefined() || options.is_null() {
                layer_ui::NewDocumentOptions {
                    extent: [width, height],
                    ..self.session.state().settings.new_document.defaults
                }
            } else {
                serde_wasm_bindgen::from_value(options).map_err(js)?
            };
        Ok(future_to_promise(async move {
            let check_cancelled=||->Result<(),JsValue>{
                if let Some(check)=&cancelled {if check.call0(&JsValue::NULL)?.as_bool()==Some(true){let error=js_sys::Error::new("Opening cancelled");error.set_name("AbortError");return Err(error.into());}}
                Ok(())
            };
            check_cancelled()?;
            // Yield before decoding so the file-progress UI is painted first.
            yield_browser().await?;
            let limits = ProjectLimits {
                dimension: device
                    .limits()
                    .max_texture_dimension_2d
                    .min(ProjectLimits::default().dimension),
                ..Default::default()
            };
            let source_name = source_name.unwrap_or_else(|| "Photo".into());
            let mut imported = match bytes {
                Some(bytes) => {
                    raster_project::open(
                        bytes,
                        raster_project::OpenOptions {
                            dimension: limits.dimension,
                            photo_policy,
                            name: source_name.clone(),
                            intent: if recovered { layer_ui::ImportIntent::Recovery } else if placing { layer_ui::ImportIntent::Place } else { layer_ui::ImportIntent::Open },
                            source_bytes: None,
                        },
                    )
                    .await?
                }
                None => layer_ui::ImportedDocument { project: new_options.project().map_err(js)?, source: layer_ui::ImportSource::Master },
            };
            check_cancelled()?;
            if let Some(source) = imported.interpretation_required(photo_policy) {
                    let callback = interpret
                        .as_ref()
                        .ok_or_else(|| js("Choose how to interpret this untagged image"))?;
                    let result =
                        callback.call1(&JsValue::NULL, &serialize(&source.interpretation)?)?;
                    let choice = JsFuture::from(js_sys::Promise::resolve(&result)).await?;
                    if choice.is_null() || choice.is_undefined() {
                        let error = js_sys::Error::new("Image opening cancelled");
                        error.set_name("AbortError");
                        return Err(error.into());
                    }
                    let profile = serde_wasm_bindgen::from_value(choice).map_err(js)?;
                    imported.interpret(profile).map_err(js)?;
            }
            let source_kind = imported.source;
            let project = imported.project;
            hdr::admit_document(&project.document)?;
            if !placing { admission.admit(&project).map_err(js)?; }
            if placing {
                let source = project
                    .document
                    .layers
                    .iter()
                    .find_map(|l| l.source.clone())
                    .ok_or_else(|| js("The selected file is not a photo"))?;
                return Ok(WebProject {
                    session: None,
                    request: id,
                    epoch,
                    revision,
                    recovered: false,
                    source: source_kind,
                    placed: Some(source),
                    source_name: project.document.layers[0].name.to_string(),
                    target,
                    lost,
                }
                .into());
            }
            let mut renderer = WgpuRasterizer::from_wgpu_native_staged(
                adapter,
                device,
                queue,
                project.document.color,
            )
            .map_err(js)?;
            raster_worker::install(&mut renderer);
            let mut programs = Vec::new();
            for effect in project
                .document
                .layers
                .iter()
                .filter_map(|l| l.effect.as_ref())
            {
                if !programs.contains(&effect.program) {
                    programs.push(effect.program.clone());
                }
            }
            let mut validating = !programs.is_empty();
            if validating {
                renderer
                    .request_effect_validation(layer_render::EffectValidationRequest {
                        request_id: 1,
                        namespace: programs.clone(),
                        programs,
                    })
                    .map_err(js)?;
            }
            renderer
                .prepare_startup(&project.document, &brush, false)
                .map_err(js)?;
            let start = js_sys::Date::now();
            loop {
                check_cancelled()?;
                renderer.compile_startup_step(false).await.map_err(js)?;
                if validating && let Some(result) = renderer.take_effect_validation() {
                    result.result.map_err(js)?;
                    validating = false;
                }
                let ready = renderer.poll_startup().map_err(js)?;
                if !validating && ready.canvas_ready && ready.brush_ready {
                    break;
                }
                if js_sys::Date::now() - start > 60_000. {
                    return Err(js("Project canvas preparation timed out"));
                }
                yield_browser().await?;
            }
            // Finite-range and digest validation of cold HDR samples can take
            // seconds at photo sizes. Yield in bounded batches before rendering.
            if project.document.color.depth.is_float() {
                let mut batch=0;
                for source in project.document.layers.iter().filter_map(|layer| layer.source.as_ref()) {
                    for tile in source.tiles.values() {
                        renderer.prepare_source_sample(tile).map_err(js)?;
                        batch+=1;
                        if batch==4 {batch=0;yield_browser().await?;check_cancelled()?;}
                    }
                }
            }
            let mut candidate = UiSession::from_project(
                AttachedRenderer(Some(Box::new(renderer))),
                project,
                None,
                viewport,
                layer_ui::Platform::Web,
            )
            .map_err(js)?;
            candidate.frame(0, 0).map_err(js)?;
            Ok(WebProject {
                session: Some(Box::new(candidate)),
                request: id,
                epoch,
                revision,
                recovered,
                source: source_kind,
                placed: None,
                source_name,
                target,
                lost,
            }
            .into())
        }))
    }
    pub fn adopt_document(
        &mut self,
        mut project: WebProject,
        location: JsValue,
    ) -> Result<JsValue, JsValue> {
        let location: Option<DocumentLocation> =
            serde_wasm_bindgen::from_value(location).map_err(js)?;
        let lost = self.gpu_owner().ok_or_else(|| js("Canvas unavailable"))?;
        if !std::sync::Arc::ptr_eq(&lost, &project.lost) || lost.lock().unwrap().is_some() {
            return Err(js(
                "The canvas changed while preparing this image; try again",
            ));
        }
        if let Some(source) = project.placed.take() {
            if !self.session.state().requests.iter().any(|r| r.id == project.request && matches!(r.kind,
                HostRequestKind::Document { request: DocumentRequest::Place | DocumentRequest::Paste })) {
                return Err(js("Image import is no longer active"));
            }
            if self.session.state().document_file.epoch != project.epoch
                || self.session.engine().document().revision != project.revision
                || self.session.engine().document().active_target() != project.target
            {
                return Err(js(
                    "The document or selected layer changed while importing; try again",
                ));
            }
            let name = project
                .source_name
                .chars()
                .filter(|c| !c.is_control())
                .take(128)
                .collect::<String>();
            self.session
                .place_layer_source(
                    if name.is_empty() { "Image" } else { &name },
                    (*source).clone(),
                    None,
                )
                .map_err(js)?;
            let mut change = self
                .session
                .complete_document_request(project.request, Ok(true))
                .map_err(js)?;
            change.canvas_wake = true;
            change.regions |= 255;
            return serialize(&change);
        }
        let location = project.source.adoption_location(location);
        if self.session.state().document_file.epoch != project.epoch
            || self.session.engine().document().revision != project.revision
        { return Err(js("The drawing changed while opening; try again")); }
        let mut candidate = *project.session.take().ok_or_else(|| js("Project already adopted"))?;
        candidate.initialize_document_location(location).map_err(js)?;
        if project.recovered { candidate.mark_recovered(); }
        candidate.set_document_replacement(false);
        candidate.inherit_window_state(&self.session).map_err(js)?;
        candidate.inherit_initial_drawing_tools(&self.session).map_err(js)?;
        let active = self.session.retained_document_tiles();
        self.documents.admit(&active, &candidate.capture_project_recovery().map_err(js)?).map_err(js)?;
        let config = &self.surface.as_ref().ok_or_else(|| js("Canvas unavailable"))?.config;
        candidate.renderer_mut().resize_surface(config.width, config.height).map_err(js)?;
        // All fallible candidate preparation precedes retiring the live editor.
        // The host has completed its initiating request and drained captures.
        let tiles = self.session.park_document().map_err(js)?;
        // Both renderers share this device. Only the retired renderer is dropped;
        // destroying the device here would also destroy the prepared drawing.
        self.session.renderer_mut().0.take();
        let previous = std::mem::replace(&mut self.session, candidate);
        self.documents.append(previous, tiles);
        if let Some(control) = self.tone.pending.take() { control.cancel(); }
        self.tone = Default::default();
        self.proof = Default::default();
        self.reset_document_views();
        self.document_changed()
    }
}

#[wasm_bindgen]
pub fn raster_worker_properties(metadata: &str) -> Result<JsValue, JsValue> {
    let info: layer_color::DocumentInfo = serde_json::from_str(metadata).map_err(js)?;
    serialize(&info.describe().map_err(js)?)
}
