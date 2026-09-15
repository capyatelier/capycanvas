//! Source repair never reconstructs or reinterprets committed raster edits.
use super::*;
use layer_core::{Edit, Layer, color::source::SourceImage};
use std::sync::Arc;

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn can_repair_source(&self, id: LayerId) -> bool {
        let document = self.engine.document();
        !document.is_locked(id)
            && document
                .layer(id)
                .is_some_and(|l| l.kind == LayerKind::Paint && l.source.is_some())
    }

    pub(super) fn request_source_repair(&mut self, id: LayerId) -> Result<(), String> {
        if self.state.platform != Platform::Gtk || !self.can_repair_source(id) {
            return Err("Select an unlocked retained image layer".into());
        }
        self.request_document(DocumentRequest::RepairSourceProfile { layer: id.0 })?;
        Ok(())
    }

    /// The host validates the replacement interpretation with its source CMM
    /// before publishing. Only interpretation metadata may change: all original
    /// sample tiles must still be shared with the captured source.
    pub fn repair_layer_source(
        &mut self,
        id: LayerId,
        original: &Arc<SourceImage>,
        corrected: SourceImage,
    ) -> Result<LayerId, String> {
        self.require_document_idle()?;
        if !self.can_repair_source(id) {
            return Err("Select an unlocked retained image layer".into());
        }
        let doc = self.engine.document();
        let layer = doc.layer(id).unwrap();
        if !Arc::ptr_eq(layer.source.as_ref().unwrap(), original) {
            return Err("The source changed while choosing its profile; try again".into());
        }
        corrected.validate()?;
        if corrected.extent != original.extent
            || corrected.interpretation.channels != original.interpretation.channels
            || corrected.interpretation.depth != original.interpretation.depth
            || corrected.interpretation.profile_assumed
            || corrected.tiles.len() != original.tiles.len()
            || !corrected
                .tiles
                .iter()
                .zip(&original.tiles)
                .all(|((a, x), (b, y))| a == b && Arc::ptr_eq(x, y))
        {
            return Err("Source profile repair must preserve the original image samples".into());
        }
        if corrected == **original {
            return Ok(id);
        }
        let mut layer = layer.clone();
        let corrected = Arc::new(corrected);
        let edit = if layer.raster.is_empty()
            && layer.pending_operations.is_empty()
            && layer.asset.is_none()
        {
            layer.source = Some(corrected);
            Edit::ReplaceLayer(Box::new(layer))
        } else {
            // Baked paint/transform/mask application remains in the existing
            // layer. Offer a plain corrected original at the same placement.
            let index = doc.layers.iter().position(|l| l.id == id).unwrap();
            let name = format!(
                "{} (corrected source)",
                layer.name.chars().take(109).collect::<String>()
            );
            let next_id = self.engine.allocate_layer_id();
            let mut next = Layer::paint(next_id, name.as_str());
            next.properties.parent = layer.properties.parent;
            next.properties.offset = layer.properties.offset;
            next.source = Some(corrected);
            self.engine
                .apply_edit(Edit::Batch(vec![
                    Edit::InsertLayer { layer: next, index },
                    Edit::SetActiveLayer { id: next_id },
                ]))
                .map_err(error)?;
            self.refresh_document();
            self.refresh_commands();
            self.layer_interaction.changed = true;
            return Ok(next_id);
        };
        self.engine.apply_edit(edit).map_err(error)?;
        self.refresh_document();
        self.refresh_commands();
        self.layer_interaction.changed = true;
        Ok(id)
    }
}
