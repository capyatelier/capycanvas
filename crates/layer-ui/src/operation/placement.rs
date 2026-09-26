//! Provisional imports and accepted whole-photo poses share the existing handles.
//! Restore the preview before committing one edit; cancellation has no history.
use super::*;
use layer_core::{Edit, Layer};
use std::collections::BTreeSet;

pub(super) struct Placement {
    pub original: Layer,
    members: Vec<Layer>,
    rollback: Edit,
    insertion: Option<usize>,
    selected: BTreeSet<layer_core::LayerId>,
}
pub(crate) struct PlacementInsertion {
    pub index: usize,
    pub rollback: Edit,
    pub ids: Vec<layer_core::LayerId>,
    pub selected: BTreeSet<layer_core::LayerId>,
}
impl Placement {
    pub(super) fn count(&self) -> usize {
        self.members.len()
    }
    pub(super) fn preview_layers(&self, doc: &Document, affine: Affine) -> Result<Vec<Layer>, String> {
        self.members.iter().map(|original| {
            let mut layer = original.clone();
            layer.properties.placement = if self.members.len() == 1 {
                affine
            } else {
                let basis = Affine::translation(doc.layer_offset(layer.id));
                original.properties.placement.then(basis).then(affine)
                    .then(basis.inverse().ok_or("Invalid placement destination")?)
            };
            Ok(layer)
        }).collect()
    }
}
fn batch_bounds(doc: &Document, members: &[Layer]) -> Rect {
    members.iter().fold(Rect::EMPTY, |bounds, layer| {
        bounds.union(layer.properties.placement
            .then(Affine::translation(doc.layer_offset(layer.id)))
            .bounds(content_bounds(doc, layer.id)))
    })
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(crate) fn begin_layer_placement(
        &mut self,
        imported: Option<PlacementInsertion>,
    ) -> Result<(), String> {
        if self.operation.active() { return Err("Finish the current transform first".into()); }
        let doc = self.engine.document();
        let layer = doc
            .layer(doc.active_layer)
            .ok_or("Select a photo layer")?
            .clone();
        if layer.source.is_none() { return Err("Select a retained photo layer".into()); }
        let mut bounds = content_bounds(doc, layer.id);
        let mut pose = Pose::from_affine(layer.properties.placement, center(bounds))
            .ok_or("This layer has invalid placement geometry")?;
        let (insertion, rollback, ids, selected) = imported.map_or_else(
            || (None, Edit::ReplaceLayer(Box::new(layer.clone())), vec![layer.id],
                self.layer_interaction.selected.clone()),
            |insert| (Some(insert.index), insert.rollback, insert.ids, insert.selected),
        );
        let members: Vec<_> = ids.iter().map(|id| doc.layer(*id).cloned()
            .ok_or("The imported layer was removed")).collect::<Result<_, _>>()?;
        let mut basis = Affine::translation(doc.layer_offset(layer.id));
        if members.len() > 1 {
            bounds = batch_bounds(doc, &members);
            pose = Pose::identity();
            basis = Affine::IDENTITY;
        }
        self.operation.serial = self.operation.serial.wrapping_add(1);
        self.operation.current = Some(Transaction {
            placement: Some(Placement {
                original: layer.clone(),
                members,
                rollback,
                insertion,
                selected,
            }),
            request: TransformPreview {
                transaction: self.operation.serial,
                moving: false,
                layer: layer.id,
                selection: None,
                transform: Default::default(),
            },
            revision: doc.revision,
            basis,
            bounds,
            start: pose,
            pose,
            drag: None,
        });
        // Establish the first valid preview before switching the visible tool
        // or selection. A rejected preview must not strand an active operation.
        if let Err(cause) = self.update_transform() {
            self.operation.current = None;
            self.operation.changed = true;
            self.refresh_tools();
            self.refresh_commands();
            return Err(cause);
        }
        self.operation.aspect = true;
        self.layer_interaction.editing = Some(layer.id);
        self.layer_interaction.selected = ids.into_iter().collect();
        self.layer_interaction.tool = LayerCanvasTool::Transform;
        self.state.layer_tools.tool = LayerCanvasTool::Transform;
        self.layer_interaction.changed = true;
        self.refresh_tools();
        self.refresh_commands();
        Ok(())
    }

    pub(super) fn finish_layer_placement(&mut self, apply: bool) -> Result<(), String> {
        let t = self.operation.current.as_ref().ok_or("No active placement")?;
        let placement = t.placement.as_ref().ok_or("No active placement")?;
        let current: Vec<_> = placement.members.iter().map(|original|
            self.engine.document().layer(original.id).cloned().ok_or("The placed layer was removed")
        ).collect::<Result<_, _>>()?;
        if apply && (t.revision != self.engine.document().revision
            || current.iter().any(|layer| self.engine.document().is_locked(layer.id))) {
            return Err("The destination changed; cancel this placement".into());
        }
        let selected = if apply && placement.insertion.is_some() {
            current.iter().map(|layer| layer.id).collect()
        } else { placement.selected.clone() };
        let changed = current[0] != placement.original || placement.insertion.is_some();
        if changed {
            let rollback = placement.rollback.clone();
            let id = current[0].id;
            let edit = if let Some(index) = placement.insertion {
                let mut edits: Vec<_> = current.into_iter().enumerate()
                    .map(|(i, layer)| Edit::InsertLayer { index: index + i, layer }).collect();
                edits.push(Edit::SetActiveLayer { id });
                Edit::Batch(edits)
            } else { Edit::ReplaceLayer(Box::new(current.into_iter().next().unwrap())) };
            // Validate both transitions before touching live state. Keep the
            // inverse of rollback so a busy/failed renderer cannot strand an
            // import outside its still-active placement transaction.
            let mut probe = self.engine.document().clone();
            let restore_preview = probe.apply(rollback.clone()).map_err(error)?;
            if apply { probe.apply(edit.clone()).map_err(error)?; }
            self.engine.preview_edit(rollback).map_err(error)?;
            if apply && let Err(cause) = self.engine.apply_edit(edit) {
                self.engine.preview_edit(restore_preview).map_err(error)?;
                self.operation.current.as_mut().unwrap().revision = self.engine.document().revision;
                return Err(error(cause));
            }
        }
        self.operation.current = None;
        self.layer_interaction.editing = Some(self.engine.document().active_layer);
        self.layer_interaction.selected = selected;
        self.layer_interaction.path.clear();
        self.layer_interaction.tool = LayerCanvasTool::Move;
        self.state.layer_tools.tool = LayerCanvasTool::Move;
        self.layer_interaction.changed = true;
        self.operation.changed = true;
        self.refresh_document();
        self.refresh_tools();
        self.refresh_commands();
        Ok(())
    }

    pub(crate) fn placement_original_size(&mut self) -> Result<(), String> {
        let t = self
            .operation
            .current
            .as_mut()
            .filter(|t| t.placement.is_some())
            .ok_or("Select an active photo placement")?;
        let placement = t.placement.as_mut().unwrap();
        if placement.members.len() > 1 {
            let mut members = Vec::with_capacity(placement.members.len());
            for original in &placement.members {
                let mut layer = self.engine.document().layer(original.id).cloned().ok_or("The placed layer was removed")?;
                let local = content_bounds(self.engine.document(), layer.id);
                let pivot = center(local);
                let position = layer.properties.placement.map(pivot);
                let [a, b, c, d, _, _] = layer.properties.placement.0;
                layer.properties.placement = Affine::around(pivot,
                    [1., (a * d - b * c).signum()], b.atan2(a), sub(position, pivot));
                members.push(layer);
            }
            placement.members = members;
            t.bounds = batch_bounds(self.engine.document(), &placement.members);
            t.pose = Pose::identity();
        } else { t.pose.scale = [1.; 2]; }
        self.update_transform()
    }
}
