//! Color mode and committed history move together after host preparation.
use super::*;
use layer_core::{ColorTransition, PreparedColorTransition, color::DocumentColor};

impl<B: CanvasRenderer> CanvasEngine<B> {
    fn require_color_idle(&self) -> Result<(), DocumentError> {
        if self.has_active_stroke()
            || self.has_pending_input()
            || self.pending_frame.is_some()
            || self.backend.has_pending_submission()
            || !self.batches.is_empty()
            || !self.dabs.is_empty()
            || self.transform_preview.is_some()
        {
            return Err(DocumentError::InvalidLayerOperation(
                "Finish the current canvas operation before changing color",
            ));
        }
        Ok(())
    }

    pub fn history_color(&self, redo: bool) -> DocumentColor {
        let color = self.editor.document().composition().color;
        self.editor
            .next_history_edit(redo)
            .map_or(color, |edit| edit.resulting_color(color))
    }

    pub fn prepare_color_transition(
        &self,
        transition: ColorTransition,
    ) -> Result<PreparedColorTransition, DocumentError> {
        self.require_color_idle()?;
        self.editor.prepare_color_transition(transition)
    }

    pub fn commit_color_transition(
        &mut self,
        prepared: PreparedColorTransition,
    ) -> Result<(), EngineError<B::Error>> {
        self.require_color_idle()?;
        let old = self.document().composition().color;
        let target = prepared.document().composition().color;
        let mut brush = self.settings.brush.clone();
        layer_render::remap_document_colors(old.space, target.space, &mut brush);
        brush.validate().map_err(DocumentError::InvalidBrush)?;
        self.editor.commit_color_transition(prepared, |document| {
            if !self
                .backend
                .adopt_prepared_color(document.composition().color)
                .map_err(EngineError::Backend)?
            {
                return Err(EngineError::Document(DocumentError::InvalidLayerOperation(
                    "The prepared color renderer is not ready",
                )));
            }
            Ok(())
        })?;
        self.settings.brush = brush;
        self.dab_generator.set_space(target.space);
        self.dab_generator.reset();
        self.completed_stroke = None;
        self.completed_before = None;
        self.completed_at = None;
        self.estimates.clear();
        self.restore_rasters.clear();
        self.rebuild_all = true;
        self.composite_all = true;
        Ok(())
    }
}
