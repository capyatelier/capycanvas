//! Artwork edits through the current selection: Clear Selected and Clear
//! Outside, Copy and Cut to a new layer, and the selection as a new mask.
//! Each is one undo step, and soft and inverted selections keep their coverage.
use super::*;
use layer_core::{Affine, Edit, Layer, LayerMask, LayerOperation, LayerOperationKind, Point, Selection};
use std::collections::BTreeSet;

const NO_SELECTION: MessageId = MessageId::COMMANDS_MAKE_A_SELECTION_FIRST;

impl<R: CanvasRenderer> UiSession<R> {
    /// Why Clear Selected and Clear Outside can't run on the drawing target
    /// once the document is idle.
    pub(super) fn clear_refusal(&self) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        let document = self.engine.document();
        if self.selection_masks.quick() {
            return Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_QUICK_MASK_EDITS_THE_SELECTION_LEAVE_IT_TO_CLEAR_ARTWORK));
        }
        if self.selection_masks.target().is_some() {
            return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_ARTWORK_FIRST));
        }
        if document.active_mask {
            return Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_MASKS_AREN_T_CLEARED_THIS_WAY_RETURN_TO_THE_LAYER_S_ARTWORK_FIRST));
        }
        if document.selection.is_none() {
            return Some(l.text(MessageId::COMMANDS_MAKE_A_SELECTION_FIRST));
        }
        match document.try_drawing_content() {
            Err(refusal) => Some(notices::drawing_refusal_text(refusal, l)),
            Ok(target) => document
                .layer(target)
                .is_some_and(|l| l.properties.alpha_locked)
                .then_some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_ALPHA_LOCK_KEEPS_TRANSPARENCY_UNLOCK_THE_LAYER_FIRST)),
        }
    }

    /// Erase the drawing target's selected pixels, or with `outside` the
    /// pixels outside the selection. A placed photo keeps its original.
    pub(super) fn clear_selection(&mut self, outside: bool) -> Result<(), String> {
        refused(self.clear_refusal())?;
        let document = self.engine.document();
        let target = document.drawing_content().ok_or("Select a drawing layer")?;
        let alpha_locked = document.layer(target).is_some_and(|l| l.properties.alpha_locked);
        let mut selection = document.selection.clone().ok_or_else(|| self.localization().text(NO_SELECTION).to_string())?;
        selection.inverted ^= outside;
        let operation = self.erase_operation(target, &selection, alpha_locked)?;
        self.engine.append_layer_operation(target, operation).map_err(error)?;
        self.layer_interaction.changed = true;
        Ok(())
    }

    /// Erases `layer`'s pixels where `selection` covers them.
    fn erase_operation(
        &mut self,
        layer: LayerId,
        selection: &Selection,
        alpha_locked: bool,
    ) -> Result<LayerOperation, String> {
        let inverse = self
            .engine
            .document()
            .affine_edit_transform(layer)
            .and_then(Affine::inverse)
            .ok_or("Invalid layer placement")?;
        let mut coverage = LayerMask::reveal_all(self.engine.allocate_layer_id(), Point::default());
        coverage.default_coverage = f32::from(selection.inverted);
        coverage.initial = Some(selection.transformed(inverse).map_err(error)?);
        Ok(LayerOperation {
            placement: Affine::IDENTITY,
            coverage,
            kind: LayerOperationKind::Erase { alpha_locked },
        })
    }

    /// Why Copy or Cut Selection to New Layer can't run once the document is
    /// idle. Copy without a selection duplicates the selected layers.
    pub(super) fn selection_to_layer_refusal(&self, cut: bool) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        let document = self.engine.document();
        if self.selection_masks.target().is_some() {
            return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_ARTWORK_FIRST));
        }
        if document.active_mask {
            return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_LAYER_S_ARTWORK_FIRST));
        }
        if document.selection.is_none() {
            if cut {
                return Some(l.text(MessageId::COMMANDS_MAKE_A_SELECTION_FIRST));
            }
            let mut selected = self.layer_interaction.selected.iter().filter_map(|id| document.layer(*id));
            return match selected.next() {
                None => Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_SELECT_LAYERS_FIRST)),
                Some(_) => None,
            };
        }
        let Some(layer) = document.layer(document.active_layer) else {
            return Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_SELECT_A_LAYER_FIRST));
        };
        match layer.kind {
            LayerKind::Paint => {}
            LayerKind::Group => return Some(notices::drawing_refusal_text(layer_core::DrawingRefusal::Group, l)),
            LayerKind::Effect => return Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_AN_EFFECT_LAYER_HAS_NO_PIXELS_OF_ITS_OWN)),
            LayerKind::Selection => {
                return Some(notices::drawing_refusal_text(layer_core::DrawingRefusal::SelectionLayer, l));
            }
        }
        if layer.properties.parent.is_some_and(|parent| document.is_locked(parent)) {
            return Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_THE_LAYER_S_GROUP_IS_LOCKED));
        }
        if cut && let Err(reason) = document.validate_content_write(layer.id) {
            return Some(notices::drawing_refusal_text(reason, l));
        }
        (cut && layer.properties.alpha_locked).then_some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_ALPHA_LOCK_KEEPS_TRANSPARENCY_UNLOCK_THE_LAYER_FIRST))
    }

    /// Copy the active layer's selected pixels to a new layer above its
    /// clipping stack, in place and unclipped. `cut` also erases them from the
    /// source. The copy captures placed appearance; Reselect restores the selection.
    pub(super) fn selection_to_layer(&mut self, cut: bool) -> Result<(), String> {
        refused(self.selection_to_layer_refusal(cut))?;
        let Some(selection) = self.engine.document().selection.clone() else {
            return self.layer_action(LayerAction::DuplicateSelected);
        };
        let document = self.engine.document();
        let source = document.layer(document.active_layer).ok_or("Select a layer first")?.clone();
        let top = document.clipping_stack_top(source.id).ok_or("Unknown layer")?;
        let index = document.layers.iter().position(|l| l.id == top).ok_or("Unknown layer")?;
        let parent = source.properties.parent;
        let parent_offset = parent.map_or(Point::default(), |id| document.layer_offset(id));
        let (origin, extent) = document.bake_extent(&BTreeSet::from([source.id]))
            .map_err(|_| self.localization().text(MessageId::COMMANDS_COPY_PIXELS_TOO_LARGE).to_string())?;
        let to_local = Affine::translation(Point { x: -origin.x, y: -origin.y });
        let mut member = source.composite_snapshot();
        member.properties.extent = Some(source.local_extent([document.width, document.height]));
        member.properties.parent = None;
        member.properties.offset = document.layer_offset(source.id);
        member.properties.clipped = false;
        member.properties.blend = layer_core::LayerBlend::Normal;
        member.visible = true;
        member.opacity = 1.;
        if let Some(mask) = &mut member.mask { mask.offset = document.layer_offset(mask.id); }
        let mut copy = Layer::paint(self.engine.allocate_layer_id(), format!("{} copy", source.name));
        copy.opacity = source.opacity;
        copy.visible = source.visible;
        copy.properties.blend = source.properties.blend;
        copy.properties.parent = parent;
        copy.properties.offset = Point { x: origin.x - parent_offset.x, y: origin.y - parent_offset.y };
        copy.properties.extent = Some(extent);
        let id = copy.id;
        let mut coverage = LayerMask::reveal_all(self.engine.allocate_layer_id(), Point::default());
        coverage.default_coverage = f32::from(selection.inverted);
        coverage.initial = Some(selection.transformed(to_local).map_err(error)?);
        let mut operations = vec![(id, LayerOperation { placement: Affine::IDENTITY, coverage,
            kind: LayerOperationKind::Bake { members: vec![member].into(), offset: Point { x: -origin.x, y: -origin.y } } })];
        if cut {
            let alpha_locked = source.properties.alpha_locked;
            operations.push((source.id, self.erase_operation(source.id, &selection, alpha_locked)?));
        }
        self.engine
            .insert_with_operations(
                vec![Edit::InsertLayer { index, layer: Box::new(copy) }, Edit::SetActiveLayer { id }],
                operations,
                Some(None),
            )
            .map_err(error)?;
        self.selection_masks.reselect = Some(selection);
        self.layer_interaction.editing = Some(id);
        self.layer_interaction.selected = BTreeSet::from([id]);
        self.layer_interaction.changed = true;
        Ok(())
    }

    /// A reveal-all mask for `layer` showing only the current selection, or
    /// hiding it with `hide`. `layer` need not be in the document yet.
    pub(super) fn selection_mask(&mut self, layer: &Layer, hide: bool) -> Result<LayerMask, String> {
        let id = self.engine.allocate_layer_id();
        self.selection_mask_with_id(layer, hide, id)
    }

    /// `selection_mask` with an identity the caller already allocated.
    pub(super) fn selection_mask_with_id(&self, layer: &Layer, hide: bool, id: LayerId) -> Result<LayerMask, String> {
        let linked = layer.mask.as_ref().is_none_or(|m| m.linked);
        let offset = layer.mask.as_ref().map_or(layer.properties.offset, |m| m.offset);
        let mut mask = LayerMask::reveal_all(id, offset);
        mask.linked = linked;
        let document = self.engine.document();
        if let Some(selection) = &document.selection {
            let mut selection = selection.clone();
            selection.inverted ^= hide;
            let parent = layer.properties.parent.map_or(Point::default(), |p| document.layer_offset(p));
            let geometry = mask.geometry_in_parent(&layer.properties).as_affine()
                .map(|map| map.then(Affine::translation(parent)));
            let inverse = if let Some(geometry) = geometry { geometry.inverse().ok_or("Invalid mask placement")? }
                else {
                    mask.linked = false;
                    mask.offset = Point { x: -parent.x, y: -parent.y };
                    mask.extent = Some([document.width, document.height]);
                    Affine::IDENTITY
                };
            mask.initial = Some(selection.transformed(inverse).map_err(error)?);
            mask.default_coverage = f32::from(selection.inverted);
        }
        Ok(mask)
    }
}
