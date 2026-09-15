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
        if redo {
            self.editor.redo_color()
        } else {
            self.editor.undo_color()
        }
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
        let matrix = old.space.linear_transform(target.space);
        let convert = |color: &mut [f32; 4]| {
            let rgb = layer_core::color::rgb::apply(
                matrix,
                [color[0], color[1], color[2]].map(f64::from),
            );
            color[..3].copy_from_slice(&rgb.map(|v| v as f32));
        };
        let mut brush = self.brush.clone();
        let mut view = self.view;
        convert(&mut brush.color_rgba_linear);
        convert(&mut brush.color_dynamics.secondary_color_rgba_linear);
        convert(&mut view.background_rgba_linear);
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
