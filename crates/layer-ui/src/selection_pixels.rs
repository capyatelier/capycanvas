//! Artwork edits through the current selection: Clear Selected and Clear
//! Outside, Copy and Cut to a new layer, and the selection as a new mask.
//! Each is one undo step, and soft and inverted selections keep their coverage.
use super::*;
use layer_core::{Affine, Edit, Layer, LayerMask, LayerOperation, LayerOperationKind, Point, Selection};
use std::collections::BTreeSet;

const ALPHA_LOCKED: &str = "Alpha lock keeps transparency; unlock the layer first";
const NO_SELECTION: &str = "Make a selection first";

impl<R: CanvasRenderer> UiSession<R> {
    /// Why Clear Selected and Clear Outside can't run on the drawing target
    /// once the document is idle.
    pub(super) fn clear_refusal(&self) -> Option<&'static str> {
        let document = self.engine.document();
        if self.selection_masks.quick() {
            return Some("Quick Mask edits the selection; leave it to clear artwork");
        }
        if self.selection_masks.target().is_some() {
            return Some("Return to the artwork first");
        }
        if document.active_mask {
            return Some("Masks aren't cleared this way; return to the layer's artwork first");
        }
        if document.selection.is_none() {
            return Some(NO_SELECTION);
        }
        match document.try_drawing_content() {
            Err(refusal) => Some(notices::drawing_refusal_text(refusal)),
            Ok(target) => document
                .layer(target)
                .is_some_and(|l| l.properties.alpha_locked)
                .then_some(ALPHA_LOCKED),
        }
    }

    /// Erase the drawing target's selected pixels, or with `outside` the
    /// pixels outside the selection. A placed photo keeps its original.
    pub(super) fn clear_selection(&mut self, outside: bool) -> Result<(), String> {
        if let Some(reason) = self.clear_refusal() {
            return Err(reason.into());
        }
        let document = self.engine.document();
        let target = document.drawing_content().ok_or("Select a drawing layer")?;
        let alpha_locked = document.layer(target).is_some_and(|l| l.properties.alpha_locked);
        let mut selection = document.selection.clone().ok_or(NO_SELECTION)?;
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
            .layer_transform(layer)
            .inverse()
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
    pub(super) fn selection_to_layer_refusal(&self, cut: bool) -> Option<&'static str> {
        let document = self.engine.document();
        if self.selection_masks.target().is_some() {
            return Some("Return to the artwork first");
        }
        if document.active_mask {
            return Some("Return to the layer's artwork first");
        }
        if document.selection.is_none() {
            if cut {
                return Some(NO_SELECTION);
            }
            let mut selected = self.layer_interaction.selected.iter().filter_map(|id| document.layer(*id));
            return match selected.next() {
                None => Some("Select layers first"),
                Some(first) if std::iter::once(first).chain(selected).any(|l| l.kind == LayerKind::Background) => {
                    Some("The paper can't be duplicated")
                }
                Some(_) => None,
            };
        }
        let Some(layer) = document.layer(document.active_layer) else {
            return Some("Select a layer first");
        };
        match layer.kind {
            LayerKind::Paint => {}
            LayerKind::Background => return Some("The paper can't be copied to a layer"),
            LayerKind::Group => return Some(notices::drawing_refusal_text(layer_core::DrawingRefusal::Group)),
            LayerKind::Effect => return Some("An effect layer has no pixels of its own"),
            LayerKind::Selection => {
                return Some(notices::drawing_refusal_text(layer_core::DrawingRefusal::SelectionLayer));
            }
        }
        if layer.properties.parent.is_some_and(|parent| document.is_locked(parent)) {
            return Some("The layer's group is locked");
        }
        if cut && document.is_locked(layer.id) {
            return Some("The active layer is locked");
        }
        (cut && layer.properties.alpha_locked).then_some(ALPHA_LOCKED)
    }

    /// Copy the active layer's selected pixels to a new layer above its
    /// clipping stack, in place and unclipped. `cut` also erases them from the
    /// source. The copy shares the source's pixels and placed photo, and the
    /// selection is consumed, with Reselect restoring it.
    pub(super) fn selection_to_layer(&mut self, cut: bool) -> Result<(), String> {
        if let Some(reason) = self.selection_to_layer_refusal(cut) {
            return Err(reason.into());
        }
        let Some(selection) = self.engine.document().selection.clone() else {
            return self.layer_action(LayerAction::DuplicateSelected);
        };
        let document = self.engine.document();
        let source = document.layer(document.active_layer).ok_or("Select a layer first")?.clone();
        let top = document.clipping_stack_top(source.id).ok_or("Unknown layer")?;
        let index = document.layers.iter().position(|l| l.id == top).ok_or("Unknown layer")?;
        let mut copy = source.clone();
        copy.id = self.engine.allocate_layer_id();
        copy.name = format!("{} copy", source.name).into();
        copy.mask = None;
        copy.pending_operations.clear();
        copy.properties.clipped = false;
        copy.properties.locked = false;
        let id = copy.id;
        let mut outside = selection.clone();
        outside.inverted = !outside.inverted;
        let mut operations = vec![(id, self.erase_operation(source.id, &outside, false)?)];
        if cut {
            let alpha_locked = source.properties.alpha_locked;
            operations.push((source.id, self.erase_operation(source.id, &selection, alpha_locked)?));
        }
        self.engine
            .insert_with_operations(
                vec![Edit::InsertLayer { index, layer: copy }, Edit::SetActiveLayer { id }],
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
            let geometry = mask.transform_in_parent(&layer.properties).then(Affine::translation(parent));
            mask.initial = Some(
                selection
                    .transformed(geometry.inverse().ok_or("Invalid mask placement")?)
                    .map_err(error)?,
            );
            mask.default_coverage = f32::from(selection.inverted);
        }
        Ok(mask)
    }
}
