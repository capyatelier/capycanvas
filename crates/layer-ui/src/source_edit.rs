//! Source repair never reconstructs or reinterprets committed raster edits.
use super::*;
use layer_core::{Edit, Layer, Project, color::source::SourceImage};
use std::sync::Arc;

/// Host preview identities follow immutable source replacement, including
/// profile-only repair. Weak owners prevent both source retention and allocator
/// address reuse while an identity is cached; no photo/profile bytes are hashed
/// on a UI refresh.
#[derive(Default)]
pub(super) struct PreviewRevisions {
    layers: std::collections::BTreeMap<LayerId, (std::sync::Weak<SourceImage>, u64)>,
    next: u64,
}
impl PreviewRevisions {
    pub(super) fn update(&mut self, layers: &[Layer]) {
        self.layers.retain(|id, _| layers.iter().any(|l| l.id == *id && l.source.is_some()));
        for layer in layers {
            let Some(source) = &layer.source else { continue };
            let weak = Arc::downgrade(source);
            if self.layers.get(&layer.id).is_none_or(|(old, _)| !old.ptr_eq(&weak)) {
                self.next = self.next.wrapping_add(1);
                self.layers.insert(layer.id, (weak, self.next));
            }
        }
    }
    pub(super) fn id(&self, id: LayerId) -> u64 {
        self.layers.get(&id).map_or(0, |(_, revision)| *revision)
    }
}

pub(crate) fn baked(layer: &Layer) -> bool {
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
    replacement.properties.placement = layer.properties.placement;
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
    /// Validate total source ownership against the same limits as native Save,
    /// and ensure both directions fit history, before allocating a live ID.
    pub(super) fn source_edit_candidate(
        &self,
        edit: &Edit,
        allocated: Option<LayerId>,
        limits: layer_core::ProjectLimits,
    ) -> Result<Project, String> {
        self.source_edit_candidates(edit, allocated.as_slice(), limits)
    }
    pub(super) fn source_edit_candidates(
        &self,
        edit: &Edit,
        allocated: &[LayerId],
        limits: layer_core::ProjectLimits,
    ) -> Result<Project, String> {
        let mut project = self.capture_project_recovery()?;
        for id in allocated {
            if project.document.allocate_layer_id() != *id {
                return Err("The layer allocation changed; try again".into());
            }
        }
        project.document.apply(edit.clone()).map_err(error)?;
        let project = project.pruned()?;
        project.validate(limits)?;
        self.engine.validate_edit(edit).map_err(error)?;
        Ok(project)
    }

    pub(super) fn can_edit_original(&self, id: LayerId) -> bool {
        let document = self.engine.document();
        !document.is_locked(id)
            && document.layer(id).is_some_and(|l| {
                l.kind == LayerKind::Paint && l.source.as_ref().is_some_and(|s| s.is_original())
            })
    }
    pub(super) fn request_source_repair(&mut self, id: LayerId) -> Result<(), String> {
        if !self.can_edit_original(id) {
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
        if !self.can_edit_original(id) {
            return Err("Select an unlocked retained image layer".into());
        }
        let layer = self.engine.document().layer(id).unwrap();
        if !Arc::ptr_eq(layer.source.as_ref().unwrap(), original) {
            return Err("The source changed while choosing its profile; try again".into());
        }
        corrected.validate()?;
        if corrected.kind != original.kind
            || corrected.extent != original.extent
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
        if corrected != **original {
            let index = self
                .engine
                .document()
                .layers
                .iter()
                .position(|l| l.id == id)
                .unwrap();
            let allocated = baked(&layer);
            let next = if allocated {
                self.engine.document().next_layer_id()
            } else {
                id
            };
            let edit = repair_edit(layer, corrected, index, next).0;
            return self.source_edit_candidate(
                &edit,
                allocated.then_some(next),
                Default::default(),
            );
        }
        self.capture_project_recovery()
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
        let allocated = baked(&layer);
        let next = if allocated {
            self.engine.document().next_layer_id()
        } else {
            id
        };
        let (edit, result) = repair_edit(layer, corrected, index, next);
        self.source_edit_candidate(&edit, allocated.then_some(next), Default::default())?;
        if allocated {
            let allocated = self.engine.allocate_layer_id();
            debug_assert_eq!(allocated, next);
        }
        self.engine.apply_edit(edit).map_err(error)?;
        self.refresh_document();
        self.refresh_commands();
        self.layer_interaction.changed = true;
        Ok(result)
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn request_source_rasterize(&mut self, id: LayerId) -> Result<(), String> {
        if !self.can_edit_original(id) {
            return Err("Select an unlocked retained image layer".into());
        }
        self.request_document(DocumentRequest::RasterizeSource { layer: id.0 })?;
        Ok(())
    }
    fn rasterized_layer(
        &self,
        id: LayerId,
        original: &Arc<SourceImage>,
        converted: Arc<SourceImage>,
    ) -> Result<Layer, String> {
        self.require_document_idle()?;
        if !self.can_edit_original(id) {
            return Err("Select an unlocked retained image layer".into());
        }
        let document = self.engine.document();
        let mut layer = document.layer(id).unwrap().clone();
        if !Arc::ptr_eq(layer.source.as_ref().unwrap(), original) {
            return Err("The source changed while rasterizing; try again".into());
        }
        converted.validate()?;
        if converted.kind != layer_core::color::source::SourceKind::Rasterized
            || converted.extent != original.extent
            || converted.interpretation.profile
                != layer_core::color::ColorProfile::Builtin(document.color.space)
            || converted.interpretation.depth != document.color.depth
        {
            return Err(
                "Rasterization must retain the full image extent in the document color mode".into(),
            );
        }
        layer.source = Some(converted);
        Ok(layer)
    }
    pub fn preview_rasterized_source(
        &self,
        id: LayerId,
        original: &Arc<SourceImage>,
        converted: Arc<SourceImage>,
    ) -> Result<Project, String> {
        let layer = self.rasterized_layer(id, original, converted)?;
        self.source_edit_candidate(
            &Edit::ReplaceLayer(Box::new(layer)),
            None,
            Default::default(),
        )
    }
    pub fn apply_rasterized_source(
        &mut self,
        id: LayerId,
        original: &Arc<SourceImage>,
        converted: Arc<SourceImage>,
    ) -> Result<(), String> {
        let layer = self.rasterized_layer(id, original, converted)?;
        let edit = Edit::ReplaceLayer(Box::new(layer));
        self.source_edit_candidate(&edit, None, Default::default())?;
        self.engine.apply_edit(edit).map_err(error)?;
        self.refresh_document();
        self.refresh_commands();
        self.layer_interaction.changed = true;
        Ok(())
    }
}
