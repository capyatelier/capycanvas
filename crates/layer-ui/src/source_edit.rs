//! Source repair never reconstructs or reinterprets committed raster edits.
use super::*;
use layer_core::{Edit, Layer, Project, color::source::SourceImage};
use std::sync::Arc;

fn baked(layer: &Layer) -> bool {
    !layer.raster.is_empty() || !layer.pending_operations.is_empty() || layer.asset.is_some()
}
fn repair_edit(
    mut layer: Layer,
    corrected: SourceImage,
    index: usize,
    next: LayerId,
) -> (Edit, LayerId) {
    if !baked(&layer) {
        layer.source = Some(Arc::new(corrected));
        let id = layer.id;
        return (Edit::ReplaceLayer(Box::new(layer)), id);
    }
    // Baked paint/transform/mask application stays on the existing layer.
    let name = format!(
        "{} (corrected source)",
        layer.name.chars().take(109).collect::<String>()
    );
    let mut replacement = Layer::paint(next, name.as_str());
    replacement.properties.parent = layer.properties.parent;
    replacement.properties.offset = layer.properties.offset;
    replacement.source = Some(Arc::new(corrected));
    (
        Edit::Batch(vec![
            Edit::InsertLayer {
                layer: replacement,
                index,
            },
            Edit::SetActiveLayer { id: next },
        ]),
        next,
    )
}

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
    fn validate_source_repair(
        &self,
        id: LayerId,
        original: &Arc<SourceImage>,
        corrected: &SourceImage,
    ) -> Result<Layer, String> {
        self.require_document_idle()?;
        if !self.can_repair_source(id) {
            return Err("Select an unlocked retained image layer".into());
        }
        let layer = self.engine.document().layer(id).unwrap();
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
        Ok(layer.clone())
    }
    /// Prepare the complete candidate stack with the same edit used by Apply.
    /// The cloned document owns provisional IDs; no live history/counter changes.
    pub fn preview_layer_source(
        &self,
        id: LayerId,
        original: &Arc<SourceImage>,
        corrected: SourceImage,
    ) -> Result<Project, String> {
        let layer = self.validate_source_repair(id, original, &corrected)?;
        let mut project = self.capture_project_recovery()?;
        if corrected != **original {
            let index = project
                .document
                .layers
                .iter()
                .position(|l| l.id == id)
                .unwrap();
            let next = if baked(&layer) {
                project.document.allocate_layer_id()
            } else {
                id
            };
            project
                .document
                .apply(repair_edit(layer, corrected, index, next).0)
                .map_err(error)?;
        }
        Ok(project)
    }
    /// The host validates interpretation with its source CMM before publishing.
    /// Original sample tiles remain shared with the captured source.
    pub fn repair_layer_source(
        &mut self,
        id: LayerId,
        original: &Arc<SourceImage>,
        corrected: SourceImage,
    ) -> Result<LayerId, String> {
        let layer = self.validate_source_repair(id, original, &corrected)?;
        if corrected == **original {
            return Ok(id);
        }
        let index = self
            .engine
            .document()
            .layers
            .iter()
            .position(|l| l.id == id)
            .unwrap();
        let next = if baked(&layer) {
            self.engine.allocate_layer_id()
        } else {
            id
        };
        let (edit, result) = repair_edit(layer, corrected, index, next);
        self.engine.apply_edit(edit).map_err(error)?;
        self.refresh_document();
        self.refresh_commands();
        self.layer_interaction.changed = true;
        Ok(result)
    }
}
