//! Publish prepared projects while retaining the window and shared document policy.
use super::*;
impl<R: CanvasRenderer> UiSession<R> {
    /// Adopt a fully prepared candidate. Retain settings/workspace/brushes in
    /// this window, reset document gestures/caches and start fresh undo history.
    /// Return the retired session so a host can release it off the input queue.
    pub fn adopt_project(
        &mut self,
        mut candidate: Box<Self>,
        epoch: u64,
        revision: u64,
        location: Option<DocumentLocation>,
    ) -> Result<Box<Self>, (String, Box<Self>)> {
        let checked = (|| {
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
            candidate
                .engine
                .set_brush(self.engine.configured_brush().clone())
                .map_err(error)?;
            candidate.apply_settings(self.state.settings.clone())?;
            Ok(next)
        })();
        let next = match checked {
            Ok(v) => v,
            Err(e) => return Err((e, candidate)),
        };
        std::mem::swap(&mut self.engine, &mut candidate.engine);
        std::mem::swap(&mut self.pen, &mut candidate.pen);
        self.input_pending = false;
        self.touch.clear();
        self.navigator_drag = None;
        self.navigator_preview = Default::default();
        self.eyedropper = Default::default();
        self.region_tools = Default::default();
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
        self.state.document_file.location = location;
        self.state.document_file.epoch = next;
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
        self.changed(
            regions::DOCUMENT | regions::CAMERA | regions::COMMANDS,
            true,
        );
        Ok(candidate)
    }
}
