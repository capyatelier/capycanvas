//! Browser canvas transport for the shared drawing collection. Parked editors
//! contain CPU document state only; a failed activation remains saveable.
use super::*;

/// The window keeps its device and surface through a switch. This contains no
/// document textures, readback, renderer, preview or raster worker.
pub(super) struct DocumentGpu {
    adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    queue: wgpu::Queue,
}

#[wasm_bindgen]
pub struct WebDocumentClose {
    blank:Option<(UiSession<AttachedRenderer>,layer_ui::DocumentSessions<UiSession<AttachedRenderer>>)>,
}

#[wasm_bindgen]
impl WebApp {
    pub fn session_stamp_for(&self,id:u64)->Result<JsValue,JsValue> {
        serialize(&self.document_session(id)?.session_stamp())
    }
    pub fn session_manifest_update(&self,state:&str,event:JsValue)->Result<JsValue,JsValue> {
        serialize(&layer_ui::session_manifest_update(state,serde_wasm_bindgen::from_value(event).map_err(js)?).map_err(js)?)
    }
    pub fn session_checkpoint_interval(&self)->u32 {layer_ui::CHECKPOINT_INTERVAL_MS as u32}
    pub fn session_can_replace_startup(&self,stamp:JsValue)->Result<bool,JsValue> {
        let stamp=serde_wasm_bindgen::from_value(stamp).map_err(js)?;
        Ok(self.documents.order().len()==1&&self.session.can_replace_startup_session(&stamp))
    }
    pub fn session_destination_matches(&self,observed:JsValue)->Result<bool,JsValue> {
        let observed:Option<layer_ui::DestinationFingerprint>=serde_wasm_bindgen::from_value(observed).map_err(js)?;
        Ok(self.session.destination_matches(observed.as_ref()))
    }
    pub fn session_record_destination(&mut self,location:JsValue,fingerprint:JsValue)->Result<(),JsValue> {
        self.session.record_destination_fingerprint(&serde_wasm_bindgen::from_value(location).map_err(js)?,serde_wasm_bindgen::from_value(fingerprint).map_err(js)?).map_err(js)
    }
    pub fn resume_document_gpu(&mut self) -> Result<bool, JsValue> {
        let Some(context) = self.document_gpu.as_ref() else {
            return Ok(false);
        };
        let surface = self.surface.as_ref().ok_or_else(|| js("Canvas surface unavailable"))?;
        if surface.lost.lock().unwrap().is_some() {
            return Err(js("The window GPU device was lost; restart the canvas"));
        }
        let mut renderer = WgpuRasterizer::from_wgpu_native_staged(
            context.adapter.clone(),
            context.device.clone(),
            context.queue.clone(),
            self.session.engine().document().composition().color,
        )
        .map_err(js)?;
        raster_worker::install(&mut renderer);
        let mut surface = self.surface.take().unwrap();
        surface.blank_presented = false;
        self.attach_gpu(WebGpu { renderer, surface })?;
        self.document_gpu = None;
        Ok(true)
    }
    pub fn recovery_document_for(&self, id: u64) -> Result<JsValue, JsValue> {
        serialize(&self.document_session(id)?.recovery_document())
    }
    pub fn document_tabs(&self, width: f32) -> Result<JsValue, JsValue> {
        serialize(&serde_json::json!({
            "tabs": self.documents.labels(&self.session.state().document_file, |s| &s.state().document_file, self.session.localization()),
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
            return Err(js(layer_ui::DocumentSessionError::TabClosed.message(self.session.localization())));
        }
        if !self.session.can_park_document() {
            return Err(js(layer_ui::DocumentTransportRefusal::SwitchOperation.message(self.session.localization()).as_ref()));
        }
        let tiles = self.session.park_document().map_err(js)?;
        self.retire_document_gpu();
        let next = self.documents.parked_owner_mut(id).unwrap();
        next.inherit_window_state(&self.session).map_err(js)?;
        self.documents
            .exchange_in_place(id, &mut self.session, tiles)
            .map_err(|reason| js(reason.message(self.session.localization())))?;
        self.document_changed()
    }

    pub fn prepare_document_close(&mut self) -> Result<WebDocumentClose, JsValue> {
        if !self.session.state().document_file.close_ready {
            return Err(js("Confirm closing the drawing first"));
        }
        let blank=if self.documents.order().len() > 1 {
            let id = self.documents.after_close().unwrap();
            let next = self.documents.parked_owner_mut(id).unwrap();
            next.inherit_window_state(&self.session).map_err(js)?;
            None
        } else {
            let mut next = UiSession::blank_localized(
                AttachedRenderer::default(),
                self.session.state().camera.viewport,
                layer_ui::Platform::Web,
                self.session.localization().clone(),
            )
            .map_err(js)?;
            next.inherit_window_state(&self.session).map_err(js)?;
            next.set_document_replacement(false);
            let documents=self.documents.prepare_empty(next.localization()).map_err(|reason|js(reason.message(next.localization())))?;
            Some((next,documents))
        };
        self.session.park_document().map_err(js)?;
        Ok(WebDocumentClose {blank})
    }
    pub fn commit_document_close(&mut self,prepared:WebDocumentClose)->Result<JsValue,JsValue> {
        self.retire_document_gpu();
        if let Some((next,documents))=prepared.blank {
            self.documents=documents;self.session=next;
        }else{self.session=self.documents.close_selected().expect("Prepared next drawing");}
        self.document_changed()
    }
    pub fn cancel_document_close(&mut self)->Result<JsValue,JsValue> {
        self.session.reset_document_close();
        if self.session.rendering_suspended(){serialize(&self.session.cancel_document_park().map_err(js)?)}else{self.document_changed()}
    }
}

impl WebApp {
    pub(super) fn document_session(&self, id: u64) -> Result<&UiSession<AttachedRenderer>, JsValue> {
        if id == self.documents.selected() {
            return Ok(&self.session);
        }
        self.documents
            .parked()
            .find_map(|(&key, p)| (id == key).then_some(&p.owner))
            .ok_or_else(|| js(layer_ui::DocumentSessionError::TabClosed.message(self.session.localization())))
    }
    fn retire_document_gpu(&mut self) {
        if let Some(control) = self.tone.pending.take() {
            control.cancel();
        }
        self.tone = Default::default();
        self.proof = Default::default();
        if let Some(gpu) = self.session.renderer_mut().0.take() {
            self.document_gpu = Some(DocumentGpu {
                adapter: gpu.adapter().clone(),
                device: gpu.device().clone(),
                queue: gpu.queue().clone(),
            });
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
