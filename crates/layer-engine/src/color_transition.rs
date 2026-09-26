//! Color mode and committed history move together after host preparation.
use super::*;
use layer_core::{ColorTransition, PreparedColorTransition, color::DocumentColor};

impl<B: CanvasRenderer> CanvasEngine<B> {
    fn require_color_idle(&self) -> Result<(), DocumentError> {
        if self.has_active_stroke()
            || self.has_pending_input()
            || self.pending_frame.is_some()
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
        let color = self.editor.document().color;
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
        let old = self.document().color;
        let target = prepared.document().color;
        // Tool colors and the canvas background keep their color appearance;
        // the document operation only changes artwork interpretation/backing.
        let mut brush = self.brush.clone();
        let mut view = self.view;
        layer_render::remap_document_colors(old.space, target.space, &mut brush, &mut view);
        brush.validate().map_err(DocumentError::InvalidBrush)?;
        if !view.background_rgba_linear.iter().all(|v| v.is_finite()) {
            return Err(DocumentError::InvalidLayerOperation(
                "Canvas background exceeds finite color precision",
            )
            .into());
        }
        self.editor.commit_color_transition(prepared, |document| {
            if !self
                .backend
                .adopt_prepared_color(document.color)
                .map_err(EngineError::Backend)?
            {
                return Err(EngineError::Document(DocumentError::InvalidLayerOperation(
                    "The prepared color renderer is not ready",
                )));
            }
            Ok(())
        })?;
        self.brush = brush;
        self.view = view;
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
