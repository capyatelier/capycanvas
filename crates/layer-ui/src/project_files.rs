//! Publish prepared projects while retaining the window and shared document policy.
use super::*;
impl<R: CanvasRenderer> UiSession<R> {
    /// Adopt a fully prepared candidate. Retain settings/workspace/brushes in
    /// this window, reset document gestures/caches and start fresh undo history.
    /// Return the retired session so a host can release it off the input queue.
    pub fn adopt_project(
        &mut self,
        candidate: Box<Self>,
        epoch: u64,
        revision: u64,
        location: Option<DocumentLocation>,
    ) -> Result<Box<Self>, (String, Box<Self>)> {
        self.adopt_project_inner(candidate, epoch, revision, location, false)
    }

    /// A private recovery copy has no durable user destination. Even Undo back
    /// to its initial checkpoint must keep Save/Discard/Cancel protection.
    pub fn adopt_recovered_project(
        &mut self,
        candidate: Box<Self>,
        epoch: u64,
        revision: u64,
    ) -> Result<Box<Self>, (String, Box<Self>)> {
        self.adopt_project_inner(candidate, epoch, revision, None, true)
    }

    fn adopt_project_inner(
        &mut self,
        mut candidate: Box<Self>,
        epoch: u64,
        revision: u64,
        location: Option<DocumentLocation>,
        recovered: bool,
    ) -> Result<Box<Self>, (String, Box<Self>)> {
        let checked = (|| {
            self.require_document_snapshot_idle()?;
            self.require_document_idle()?;
            if epoch != self.state.document_file.epoch
                || revision != self.engine.document().revision
            {
                return Err(
                    "The document changed while opening; review those changes first".into(),
                );
            }
            if let Some(location) = &location {
                location.validate()?;
            }
            let next = epoch
                .checked_add(1)
                .ok_or("Document generation exhausted")?;
            let source = self.engine.document().color.space;
            let destination = candidate.engine.document().color.space;
            let transform = source.linear_transform(destination);
            let mut brush = self.engine.configured_brush().clone();
            brush.color_rgba_linear = self.state.colors.definition().linear_in(destination)?;
            let secondary = &mut brush.color_dynamics.secondary_color_rgba_linear;
            let rgb = layer_core::color::rgb::apply(transform, [secondary[0] as f64, secondary[1] as f64, secondary[2] as f64]);
            secondary[..3].copy_from_slice(&rgb.map(|v| v as f32));
            candidate.engine.set_brush(brush).map_err(error)?;
            // Prepare the new picker before publishing document resources.
            candidate.state.colors = self.state.colors.clone();
            candidate.state.colors.set_rgb_space(destination)?;
            candidate.state.colors.set_document_depth(candidate.engine.document().color.depth)?;
            // File preparation uses a generic session. Resolve prediction with
            // this window's live capability before transferring its engine.
            candidate.state.platform = self.state.platform;
            candidate.platform_prediction_available = self.platform_prediction_available;
            candidate.apply_settings(self.state.settings.clone())?;
            Ok(next)
        })();
        let next = match checked {
            Ok(v) => v,
            Err(e) => return Err((e, candidate)),
        };
        std::mem::swap(&mut self.state.colors, &mut candidate.state.colors);
        self.state.brush.color = self.state.colors.preview(self.state.colors.definition());
        // Capture belongs to the window, including across document switches.
        self.engine.recording.end(true);
        std::mem::swap(&mut self.engine.recording, &mut candidate.engine.recording);
        std::mem::swap(&mut self.engine, &mut candidate.engine);
        std::mem::swap(&mut self.pen, &mut candidate.pen);
        self.input_pending = false;
        self.touch.clear();
        self.navigator_drag = None;
        self.navigator_preview = Default::default();
        self.eyedropper = Default::default();
        self.region_tools = Default::default();
        self.painted_selections = Default::default();
        self.selection_masks = Default::default();
        self.operation = Default::default();
        self.rulers.selected = None;
        self.layer_interaction = Default::default();
        self.cursor = Default::default();
        // Keep the latest host viewport and monotonic input revision, including
        // any resize that occurred while the candidate was being prepared.
        self.state.camera.flipped = [false; 2];
        self.initial_fit = false;
        self.sync_work_area();
        let d = self.engine.document();
        self.state.camera.fit([d.width, d.height]);
        std::mem::swap(&mut self.files.assets, &mut candidate.files.assets);
        self.files.saved_checkpoint = self.engine.checkpoint();
        self.files.unpublished = recovered || (location.is_none() && candidate.files.unpublished);
        self.state.document_file.unsaved_name = candidate.state.document_file.unsaved_name.clone();
        self.state.document_file.location = location;
        self.state.document_file.epoch = next;
        self.state.preview_sdr = false;
        self.last_proof_mode = None;
        self.proof_setup_pending = false;
        self.state.sdr_appearance_preview = None;
        self.state.hdr_display_available = false;
        self.state.soft_proof = false;
        self.state.gamut_warning = false;
        self.state.document_file.close_ready = false;
        self.engine.start_document_view(
            self.state.camera.view(),
            self.state.camera.input_transform(),
        );
        self.sync_camera();
        // The current brush/tool and pressure/feedback settings remain shared
        // policy; the candidate carries only document/render resources.
        self.apply_brush()
            .expect("previously validated brush state");
        self.refresh_document();
        self.refresh_commands();
        self.sync_renderer_telemetry();
        self.changed(
            regions::DOCUMENT | regions::CAMERA | regions::COMMANDS | regions::BRUSH,
            true,
        );
        Ok(candidate)
    }
}
