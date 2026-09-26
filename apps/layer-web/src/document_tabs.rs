//! Browser canvas transport for the shared drawing collection. Parked editors
//! contain CPU document state only; a failed activation remains saveable.
use super::*;

/// The window keeps its device and surface through a switch. This contains no
/// document textures, presenter, readback, renderer, preview or raster worker.
pub(super) struct DocumentGpu {
    adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    queue: wgpu::Queue,
    instance: wgpu::Instance,
    surface: Option<wgpu::Surface<'static>>,
    config: wgpu::SurfaceConfiguration,
    color: SdrSurfaceColor,
    sdr_format: wgpu::TextureFormat,
    hdr_capable: bool,
    lost: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}
impl From<WebGpu> for DocumentGpu {
    fn from(gpu: WebGpu) -> Self {
        Self {
            adapter: gpu.renderer.adapter().clone(),
            device: gpu.renderer.device().clone(),
            queue: gpu.renderer.queue().clone(),
            instance: gpu.instance,
            surface: gpu.surface,
            config: gpu.config,
            color: gpu.color,
            sdr_format: gpu.sdr_format,
            hdr_capable: gpu.hdr_capable,
            lost: gpu.lost,
        }
    }
}

#[wasm_bindgen]
pub struct WebRecoveryCapture(Option<layer_core::Project>);
#[wasm_bindgen]
impl WebRecoveryCapture {
    pub fn write(&mut self, key: String) -> Result<js_sys::Promise, JsValue> {
        let project = self
            .0
            .take()
            .ok_or_else(|| js("Recovery capture already written"))?;
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            raster_project::save_recovery(project, key).await
        }))
    }
}

#[wasm_bindgen]
impl WebApp {
    pub fn resume_document_gpu(&mut self) -> Result<bool, JsValue> {
        let Some(context) = self.document_gpu.as_mut() else {
            return Ok(false);
        };
        if context.lost.lock().unwrap().is_some() {
            return Err(js("The window GPU device was lost; restart the canvas"));
        }
        let mut renderer = WgpuRasterizer::from_wgpu_native_staged(
            context.adapter.clone(),
            context.device.clone(),
            context.queue.clone(),
            self.session.engine().document().color,
        )
        .map_err(js)?;
        raster_worker::install(&mut renderer);
        let presenter =
            ViewportPresenter::for_surface(&renderer, context.config.format, context.color)
                .map_err(js)?;
        let gpu = WebGpu {
            renderer,
            presenter,
            instance: context.instance.clone(),
            surface: context.surface.take(),
            config: context.config.clone(),
            color: context.color,
            sdr_format: context.sdr_format,
            hdr_capable: context.hdr_capable,
            lost: context.lost.clone(),
            blank_presented: false,
        };
        self.attach_gpu(gpu)?;
        self.document_gpu = None;
        Ok(true)
    }
    pub fn recovery_document_for(&self, id: u64) -> Result<JsValue, JsValue> {
        serialize(&self.document_session(id)?.recovery_document())
    }
    pub fn capture_tab_recovery(&self, id: u64) -> Result<WebRecoveryCapture, JsValue> {
        Ok(WebRecoveryCapture(Some(
            self.document_session(id)?
                .capture_project_recovery()
                .map_err(js)?,
        )))
    }
    pub fn document_tabs(&self, width: f32) -> Result<JsValue, JsValue> {
        serialize(&serde_json::json!({
            "tabs": self.documents.labels(&self.session.state().document_file, |s| &s.state().document_file),
            "selected": self.documents.selected(),
            "compact": layer_ui::DocumentTabs::compact(width, self.documents.order().len()),
            "can_undo": self.documents.can_undo(),
            "can_redo": self.documents.can_redo(),
            "storage_error": self.documents.storage_error(),
            "resident_bytes": self.documents.resident_bytes(),
        }))
    }

    /// Poll without blocking the browser: an immediate Undo may have moved an
    /// unpublished capture into redo, so inspect all exact retained roots.
    pub fn document_park_ready(&self) -> Result<bool, JsValue> {
        if !self.session.can_park_document() {
            return Ok(false);
        }
        if self.session.rendering_suspended() || self.session.state().document_file.close_ready {
            return Ok(true);
        }
        Ok(self
            .session
            .retained_document_tiles()
            .try_blobs()
            .map_err(js)?
            .is_some())
    }
    pub fn document_close_available(&self) -> bool {
        self.session
            .command(layer_ui::CommandId::CloseDocument)
            .enabled
    }

    pub fn adjacent_document(&self, forward: bool) -> Option<u64> {
        self.documents.adjacent(forward)
    }
    pub fn document_drop(
        &self,
        hits: JsValue,
        x: f32,
        y: f32,
        vertical: bool,
    ) -> Result<JsValue, JsValue> {
        let hits =
            serde_wasm_bindgen::from_value::<Vec<layer_ui::DocumentTabHit>>(hits).map_err(js)?;
        serialize(
            &self
                .documents
                .drop_target(&hits, [x, y], vertical)
                .map(|before| serde_json::json!({ "before": before })),
        )
    }
    pub fn document_slide(&self, request: JsValue) -> Result<JsValue, JsValue> {
        let SlideRequest {
            id,
            hits,
            clip,
            press,
            point,
        } = serde_wasm_bindgen::from_value(request).map_err(js)?;
        serialize(
            &self
                .documents
                .drag(id, press, &hits, clip)
                .and_then(|drag| drag.preview(point)),
        )
    }
    pub fn step_document(&mut self, id: u64, forward: bool) -> Result<JsValue, JsValue> {
        let before = self
            .documents
            .step(id, forward)
            .ok_or_else(|| js("The drawing is already at the end"))?;
        self.reorder_document(id, before)
    }

    pub fn reorder_document(&mut self, id: u64, before: Option<u64>) -> Result<JsValue, JsValue> {
        if !self.session.can_park_document() {
            return Err(js(
                "Finish the current operation before reordering drawings",
            ));
        }
        self.documents.reorder(id, before);
        self.document_changed()
    }

    pub fn document_order_history(&mut self, redo: bool) -> Result<JsValue, JsValue> {
        if !self.session.can_park_document() {
            return Err(js(
                "Finish the current operation before reordering drawings",
            ));
        }
        if redo {
            self.documents.redo();
        } else {
            self.documents.undo();
        }
        self.document_changed()
    }

    /// Switch only after the host has drained its document jobs. Rebuilding the
    /// GPU happens after this atomic owner exchange; it cannot lose either tab.
    pub fn select_document(&mut self, id: u64) -> Result<JsValue, JsValue> {
        if id == self.documents.selected() {
            return self.document_changed();
        }
        if !self.documents.contains_parked(id) {
            return Err(js("Drawing tab is no longer open"));
        }
        let tiles = self.session.park_document().map_err(js)?;
        self.retire_document_gpu();
        let next = self.documents.parked_owner_mut(id).unwrap();
        next.inherit_window_state(&self.session).map_err(js)?;
        self.documents
            .exchange_in_place(id, &mut self.session, tiles)
            .map_err(js)?;
        self.document_changed()
    }

    pub fn close_document_tab(&mut self) -> Result<JsValue, JsValue> {
        if !self.session.state().document_file.close_ready {
            return Err(js("Confirm closing the drawing first"));
        }
        if self.documents.order().len() > 1 {
            let id = self.documents.after_close().unwrap();
            let next = self.documents.parked_owner_mut(id).unwrap();
            next.inherit_window_state(&self.session).map_err(js)?;
            self.session.park_document().map_err(js)?;
            self.retire_document_gpu();
            self.session = self.documents.close_selected().unwrap();
        } else {
            // Keep the existing browser window usable after its last drawing
            // closes, with a new identity and no retained discarded history.
            let mut next =
                UiSession::blank(WebRenderer::default(), self.session.state().camera.viewport)
                    .map_err(js)?;
            next.inherit_window_state(&self.session).map_err(js)?;
            next.set_document_replacement(false);
            self.session.park_document().map_err(js)?;
            self.retire_document_gpu();
            self.documents.close_selected();
            self.documents.start_empty().map_err(js)?;
            self.session = next;
        }
        self.document_changed()
    }
}

impl WebApp {
    fn document_session(&self, id: u64) -> Result<&UiSession<WebRenderer>, JsValue> {
        if id == self.documents.selected() {
            return Ok(&self.session);
        }
        self.documents
            .parked()
            .find_map(|(&key, p)| (id == key).then_some(&p.owner))
            .ok_or_else(|| js("Drawing tab is no longer open"))
    }
    fn retire_document_gpu(&mut self) {
        if let Some(control) = self.tone.pending.take() {
            control.cancel();
        }
        self.tone = Default::default();
        self.proof = Default::default();
        if let Some(gpu) = self.session.renderer_mut().0.take() {
            self.document_gpu = Some(gpu.into());
        }
        self.reset_document_views();
    }
    pub(super) fn reset_document_views(&mut self) {
        self.startup = Default::default();
        self.deferred_contacts.clear();
        for slot in self.overviews.values_mut() {
            slot.gpu = None;
        }
    }
    pub(super) fn document_changed(&self) -> Result<JsValue, JsValue> {
        serialize(&layer_ui::UiChange {
            revision: self.session.state().revision,
            regions: layer_ui::regions::ALL,
            canvas_wake: true,
        })
    }
}

#[derive(serde::Deserialize)]
struct SlideRequest {
    id: u64,
    hits: Vec<layer_ui::DocumentTabHit>,
    clip: layer_ui::Bounds,
    press: [f32; 2],
    point: [f32; 2],
}
