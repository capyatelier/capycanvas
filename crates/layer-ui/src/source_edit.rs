//! Source repair never reconstructs or reinterprets committed raster edits.
use super::*;
use layer_core::{Edit, Layer, Point, Project, color::source::SourceImage};
use std::sync::Arc;

type PreviewGeometry = (layer_core::Projective, layer_core::Interpolation, Point, Option<[u32; 2]>,
    Option<(LayerId, layer_core::Projective, Point, Option<[u32; 2]>, bool)>);
type PreviewRevision = (Option<std::sync::Weak<SourceImage>>, Option<std::sync::Weak<layer_core::MeshMap>>, PreviewGeometry, u64);

#[derive(Default)]
pub(super) struct PreviewRevisions {
    layers: std::collections::BTreeMap<LayerId, PreviewRevision>,
    next: u64,
}
impl PreviewRevisions {
    pub(super) fn update(&mut self, layers: &[Layer]) {
        for layer in layers {
            let source = layer.source.as_ref().map(Arc::downgrade);
            let placement = &layer.properties.placement;
            let mesh = placement.mesh.as_ref().map(Arc::downgrade);
            let geometry = (placement.outer, placement.interpolation, layer.properties.offset, layer.properties.extent,
                layer.mask.as_ref().map(|m| (m.id, m.placement, m.offset, m.extent, m.linked)));
            let same = self.layers.get(&layer.id).is_some_and(|(a, b, previous, _)| {
                let source = match (a, &source) { (Some(a), Some(b)) => a.ptr_eq(b), (None, None) => true, _ => false };
                let mesh = match (b, &mesh) { (Some(a), Some(b)) => a.ptr_eq(b), (None, None) => true, _ => false };
                source && mesh && *previous == geometry
            });
            if !same {
                self.next = self.next.wrapping_add(1);
                self.layers.insert(layer.id, (source, mesh, geometry, self.next));
            }
        }
        if self.layers.len() != layers.len() {
            self.layers.retain(|id, _| layers.iter().any(|l| l.id == *id));
        }
    }
    pub(super) fn id(&self, id: LayerId) -> u64 { self.layers.get(&id).map_or(0, |value| value.3) }
}

pub(crate) fn baked(layer: &Layer) -> bool {
    !layer.raster.is_empty() || !layer.pending_operations.is_empty()
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
                layer: Box::new(replacement),
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
    /// Why Revert to Original Photo can't run on the active layer once the
    /// document is idle.
    pub(super) fn revert_to_original_refusal(&self) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        let document = self.engine.document();
        if self.selection_masks.target().is_some() {
            return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_ARTWORK_FIRST));
        }
        if document.active_mask {
            return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_LAYER_S_ARTWORK_FIRST));
        }
        if self.state.document_file.busy {
            return Some(l.text(MessageId::COMMANDS_WAIT_FOR_THE_CURRENT_FILE_OPERATION));
        }
        let Some(layer) = document.layer(document.active_layer).filter(|l| l.kind == LayerKind::Paint) else {
            return Some(l.text(MessageId::COMMANDS_REFUSAL_SOURCE_EDIT_SELECT_A_PLACED_PHOTO_LAYER));
        };
        match &layer.source {
            None => Some(l.text(MessageId::COMMANDS_REFUSAL_SOURCE_EDIT_SELECT_A_PLACED_PHOTO_LAYER)),
            Some(source) if !source.is_original() => Some(l.text(MessageId::COMMANDS_REFUSAL_SOURCE_EDIT_A_RASTERIZED_PHOTO_HAS_NO_ORIGINAL_TO_RETURN_TO)),
            Some(_) if document.is_locked(layer.id) => Some(l.text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED)),
            Some(_) => (!baked(layer)).then_some(l.text(MessageId::COMMANDS_REFUSAL_SOURCE_EDIT_THIS_PHOTO_HAS_NO_EDITS)),
        }
    }
    /// Discards the active photo's raster edits in one undo step, keeping its
    /// source, placement, mask, opacity and blend mode.
    pub(super) fn revert_to_original(&mut self) -> Result<(), String> {
        self.require_document_idle()?;
        refused(self.revert_to_original_refusal())?;
        let document = self.engine.document();
        let mut layer = document.layer(document.active_layer).ok_or("Select a placed photo layer")?.clone();
        layer.raster = Default::default();
        layer.pending_operations.clear();
        self.layer_edit(Edit::ReplaceLayer(Box::new(layer)))?;
        self.layer_interaction.changed = true;
        Ok(())
    }
    pub(super) fn request_source_edit(&mut self, id: LayerId, request: DocumentRequest) -> Result<(), String> {
        if !self.can_edit_original(id) {
            return Err("Select an unlocked retained image layer".into());
        }
        self.request_document(request)
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
    fn repair_plan(&self, id: LayerId, layer: Layer, corrected: SourceImage) -> (Edit, LayerId, Option<LayerId>) {
        let document = self.engine.document();
        let index = document.layers.iter().position(|l| l.id == id).unwrap();
        let allocated = baked(&layer).then(|| document.next_layer_id());
        let (edit, result) = repair_edit(layer, corrected, index, allocated.unwrap_or(id));
        (edit, result, allocated)
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
        if corrected == **original {
            return self.capture_project_recovery();
        }
        let (edit, _, allocated) = self.repair_plan(id, layer, corrected);
        self.source_edit_candidates(&edit, allocated.as_slice(), Default::default())
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
        let (edit, result, allocated) = self.repair_plan(id, layer, corrected);
        self.source_edit_candidates(&edit, allocated.as_slice(), Default::default())?;
        if let Some(next) = allocated {
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
        self.source_edit_candidates(&Edit::ReplaceLayer(Box::new(layer)), &[], Default::default())
    }
    pub fn apply_rasterized_source(
        &mut self,
        id: LayerId,
        original: &Arc<SourceImage>,
        converted: Arc<SourceImage>,
    ) -> Result<(), String> {
        let layer = self.rasterized_layer(id, original, converted)?;
        let edit = Edit::ReplaceLayer(Box::new(layer));
        self.source_edit_candidates(&edit, &[], Default::default())?;
        self.engine.apply_edit(edit).map_err(error)?;
        self.refresh_document();
        self.refresh_commands();
        self.layer_interaction.changed = true;
        Ok(())
    }
}
