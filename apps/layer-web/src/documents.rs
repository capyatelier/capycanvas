//! Browser transport owns prepared candidates and readbacks across event-loop
//! yields. A working document is replaced only after validation and stale checks.
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
    session: Option<Box<UiSession<WebRenderer>>>,
    request: u32,
    epoch: u64,
    revision: u64,
    closing: bool,
    recovered: bool,
    photo: bool,
}

#[wasm_bindgen]
impl WebApp {
    pub fn inspect_profile(&self, bytes: js_sys::Uint8Array) -> Result<JsValue, JsValue> {
        if bytes.length() as usize > layer_color::MAX_ICC_BYTES {
            return Err(js("ICC profile exceeds 16 MiB"));
        }
        let profile = layer_core::color::ColorProfile::Icc(bytes.to_vec().into());
        let channels = layer_color::profile_channels(&profile).map_err(js)?;
        let name = layer_color::profile_description(&profile).map_err(js)?;
        serialize(&layer_ui::ExportProfile {
            profile,
            channels,
            name,
        })
    }
    pub fn export_form(&self) -> Result<JsValue, JsValue> {
        serialize(&layer_ui::ExportForm::new(self.session.engine().document()))
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
                || self.session.state().document_file.modified
                || self.session.state().document_file.busy)
        {
            return Err(js("Recovery requires an unchanged, idle drawing"));
        }
        let closing = id == 0 && self.session.state().document_file.close_ready;
        let request = if recovered {
            Some(DocumentRequest::Open)
        } else if closing {
            Some(DocumentRequest::New)
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
            (DocumentRequest::Open, Some(_)) | (DocumentRequest::New, None)
        ) {
            return Err(js("Invalid project preparation request"));
        }
        let live = self
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or_else(|| js("Wait for the canvas"))?;
        let (adapter, device, queue) = (
            live.renderer.adapter().clone(),
            live.renderer.device().clone(),
            live.renderer.queue().clone(),
        );
        let instance = live.instance.clone();
        let lost = live.lost.clone();
        let config = live.config.clone();
        let viewport = self.session.state().camera.viewport;
        let brush = self.session.engine().configured_brush().clone();
        let photo_policy = self.session.state().settings.photo_open;
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
            // Yield before decoding so the file-progress UI is painted first.
            yield_browser().await?;
            let limits = ProjectLimits {
                dimension: device
                    .limits()
                    .max_texture_dimension_2d
                    .min(ProjectLimits::default().dimension),
                ..Default::default()
            };
            let photo = bytes
                .as_ref()
                .is_some_and(|bytes| bytes.subarray(0, 4).to_vec() != b"CAPY");
            let mut project = match bytes {
                Some(bytes) => {
                    raster_project::open(
                        bytes,
                        raster_project::OpenOptions {
                            dimension: limits.dimension,
                            photo_policy,
                            name: source_name.unwrap_or_else(|| "Photo".into()),
                            recovered,
                        },
                    )
                    .await?
                }
                None => new_options.project().map_err(js)?,
            };
            if photo && photo_policy.missing_profile == layer_ui::MissingProfilePolicy::Ask {
                if let Some(source) = project
                    .document
                    .layers
                    .iter()
                    .find_map(|l| l.source.as_ref())
                    .filter(|s| s.interpretation.profile_assumed)
                {
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
                    let source = layer_color::assume_source_profile((**source).clone(), profile)
                        .map_err(js)?;
                    let name = project.document.layers[0].name.to_string();
                    project =
                        layer_color::photo_project(source, &name, project.document.color.depth)
                            .map_err(js)?;
                }
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
                renderer.compile_startup_step().await.map_err(js)?;
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
            let presenter = ViewportPresenter::new(renderer.device(), config.format);
            let gpu = WebGpu {
                renderer,
                instance,
                config,
                surface: None,
                presenter,
                blank_presented: true,
                lost,
            };
            let mut candidate =
                UiSession::from_project(WebRenderer(Some(gpu)), project, None, viewport)
                    .map_err(js)?;
            candidate.frame(0, 0).map_err(js)?;
            Ok(WebProject {
                session: Some(Box::new(candidate)),
                request: id,
                epoch,
                revision,
                closing,
                recovered,
                photo,
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
        let location = if project.photo { None } else { location };
        if project.closing && !self.session.state().document_file.close_ready {
            return Err(js("Document close was cancelled"));
        }
        let candidate = project
            .session
            .take()
            .ok_or_else(|| js("Project already adopted"))?;
        let mut retired = if project.recovered {
            self.session
                .adopt_recovered_project(candidate, project.epoch, project.revision)
        } else {
            self.session
                .adopt_project(candidate, project.epoch, project.revision, location)
        }
        .map_err(|(e, _)| js(e))?;
        let old = retired.renderer_mut().0.as_mut().unwrap();
        let next = self.session.renderer_mut().0.as_mut().unwrap();
        next.surface = old.surface.take();
        next.config = old.config.clone();
        next.renderer
            .resize_surface(next.config.width, next.config.height)
            .map_err(js)?;
        next.surface
            .as_ref()
            .unwrap()
            .configure(next.renderer.device(), &next.config);
        // Tool/settings changes are allowed while the candidate is preparing.
        // Recompute readiness for the brush retained by shared adoption before
        // accepting the first contact in the new document.
        self.prepare_startup()?;
        self.startup = self
            .session
            .renderer_mut()
            .0
            .as_mut()
            .unwrap()
            .renderer
            .poll_startup()
            .map_err(js)?;
        self.deferred_contacts.clear();
        if project.closing || project.recovered {
            serialize(&layer_ui::UiChange {
                revision: self.session.state().revision,
                regions: 255,
                canvas_wake: true,
            })
        } else {
            serialize(
                &self
                    .session
                    .complete_document_request(project.request, Ok(true))
                    .map_err(js)?,
            )
        }
    }
}
