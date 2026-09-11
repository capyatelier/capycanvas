//! Browser transport owns prepared candidates and readbacks across event-loop
//! yields. A working document is replaced only after validation and stale checks.
use super::*;
use layer_core::{Project, ProjectLimits};
use layer_ui::{DocumentLocation, DocumentRequest, HostRequestKind};
use wasm_bindgen_futures::{JsFuture, future_to_promise};

async fn yield_browser() -> Result<(), JsValue> {
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
}

#[wasm_bindgen]
impl WebApp {
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
    pub fn save_project(&mut self, id: u32, location: JsValue) -> Result<Vec<u8>, JsValue> {
        let location = serde_wasm_bindgen::from_value(location).map_err(js)?;
        let project = self
            .session
            .capture_project_save(id, location)
            .map_err(js)?
            .pruned()
            .map_err(js)?;
        let mut bytes = Vec::new();
        project.write(&mut bytes).map_err(js)?;
        Ok(bytes)
    }
    pub fn export_ready(&self) -> bool {
        self.session
            .engine()
            .backend()
            .0
            .as_ref()
            .is_some_and(|gpu| gpu.renderer.export_ready())
    }
    pub fn export_png(&mut self, id: u32) -> Result<js_sys::Promise, JsValue> {
        self.session.require_document_idle().map_err(js)?;
        if !self.session.state().requests.iter().any(|r| {
            r.id == id
                && matches!(
                    r.kind,
                    HostRequestKind::Document {
                        request: DocumentRequest::Export { .. }
                    }
                )
        }) {
            return Err(js("The export request is no longer active"));
        }
        let gpu = &mut self
            .session
            .renderer_mut()
            .0
            .as_mut()
            .ok_or_else(|| js("Wait for the canvas"))?
            .renderer;
        let mut ticket = gpu.begin_export_readback(id as u64).map_err(js)?;
        Ok(future_to_promise(async move {
            let start = js_sys::Date::now();
            loop {
                if let Some(image) = ticket.try_finish().map_err(js)? {
                    let mut bytes = Vec::new();
                    image.write_png(&mut bytes).map_err(js)?;
                    return Ok(js_sys::Uint8Array::from(bytes.as_slice()).into());
                }
                if js_sys::Date::now() - start > 60_000. {
                    return Err(js("PNG readback timed out"));
                }
                yield_browser().await?;
            }
        }))
    }
    pub fn prepare_document(
        &self,
        id: u32,
        bytes: Option<Vec<u8>>,
        width: u32,
        height: u32,
        epoch: u64,
        revision: u64,
    ) -> Result<js_sys::Promise, JsValue> {
        self.session.require_document_idle().map_err(js)?;
        if self.session.state().document_file.epoch != epoch
            || self.session.engine().document().revision != revision
        {
            return Err(js(
                "The document changed while choosing a file; review those changes first",
            ));
        }
        let closing = id == 0 && self.session.state().document_file.close_ready;
        let request = if closing {
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
        let config = live.config.clone();
        let viewport = self.session.state().camera.viewport;
        let brush = self.session.engine().configured_brush().clone();
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
            let project = match bytes {
                Some(bytes) => Project::read(bytes.as_slice(), limits),
                None => layer_ui::new_drawing(width, height),
            }
            .map_err(js)?;
            let mut renderer =
                WgpuRasterizer::from_wgpu_staged(adapter, device, queue).map_err(js)?;
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
        if project.closing && !self.session.state().document_file.close_ready {
            return Err(js("Document close was cancelled"));
        }
        let candidate = project
            .session
            .take()
            .ok_or_else(|| js("Project already adopted"))?;
        let mut retired = self
            .session
            .adopt_project(candidate, project.epoch, project.revision, location)
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
        if project.closing {
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
