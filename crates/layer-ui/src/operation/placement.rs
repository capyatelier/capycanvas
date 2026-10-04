//! Provisional imports and accepted whole-photo poses share the existing handles.
//! Restore the preview before committing one edit; cancellation has no history.
use super::*;
use layer_core::Edit;
use layer_core::authored::{Occurrence, OccurrenceHandle, RecordChange, SceneSnapshot, SourceTarget};
use std::sync::Arc;
use std::collections::BTreeSet;

pub(super) struct Placement {
    pub(super) members: Vec<(OccurrenceHandle, Occurrence)>,
    pub(super) source: Arc<SceneSnapshot>,
    initial: Option<Vec<(OccurrenceHandle, Occurrence)>>,
    rollback: Edit,
    insertion: Option<usize>,
    selected: BTreeSet<OccurrenceHandle>,
    roots: Vec<OccurrenceHandle>,
}
pub(crate) struct PlacementInsertion {
    pub index: usize,
    pub rollback: Edit,
    pub ids: Vec<OccurrenceHandle>,
    pub selected: BTreeSet<OccurrenceHandle>,
}
impl Placement {
    pub(super) fn reset(&mut self) {
        if let Some(initial) = self.initial.take() { self.members = initial; }
    }
    fn repeat_delta(&self, t: &Transaction) -> Option<Projective> {
        if self.insertion.is_some() || self.initial.is_some() || t.start.mesh.as_ref() != t.mapped_mesh() { return None; }
        let before = t.start.outer()?;
        let after = t.outer()?;
        if before == after { return None; }
        Projective::from_affine(t.basis.inverse()?).then(before.inverse()?)?.then(after)?
            .then(Projective::from_affine(t.basis))
    }
    pub(super) fn count(&self) -> usize {
        self.roots.len()
    }
    pub(super) fn single_leaf(&self) -> bool {
        self.members.len() == 1 && self.members[0].1.kind() == LayerKind::Paint
    }
    pub(super) fn targets(&self, doc: &Document) -> Vec<SourceTarget> {
        member_targets(doc, &self.members)
    }
    pub(super) fn preview_occurrences(&self, doc: &Document, map: &LayerPlacement, interpolation: Option<Interpolation>) -> Result<Vec<(OccurrenceHandle, Occurrence)>, String> {
        let mut candidate = doc.clone();
        candidate.apply(Edit::Batch(replacement_edits(&candidate, &self.members)?)).map_err(error)?;
        candidate.retained_transform_targets(&self.roots).map_err(error)?;
        if self.single_leaf() {
            let (handle, original) = &self.members[0];
            let mut occurrence = original.clone();
            occurrence.placement = map.clone();
            candidate.apply(Edit::Occurrence(RecordChange::replace(&candidate.artwork.occurrences, *handle, Some(occurrence))?)).map_err(error)?;
        } else {
            let edit = candidate.retained_transform_edit(&self.roots, map.outer).map_err(error)?;
            candidate.apply(edit).map_err(error)?;
        }
        self.members.iter().map(|(handle, _)| {
            let mut occurrence = candidate.scene().occurrence(*handle).cloned().ok_or("The transformed layer was removed")?;
            if occurrence.kind() == LayerKind::Paint && let Some(interpolation) = interpolation {
                occurrence.placement.interpolation = interpolation;
            }
            Ok((*handle, occurrence))
        }).collect()
    }
}
pub(super) fn replacement_edits(doc: &Document, members: &[(OccurrenceHandle, Occurrence)]) -> Result<Vec<Edit>, String> {
    members.iter().map(|(handle, occurrence)| RecordChange::replace(&doc.artwork.occurrences, *handle, Some(occurrence.clone())).map(Edit::Occurrence).map_err(str::to_owned)).collect()
}
fn member_targets(doc: &Document, members: &[(OccurrenceHandle, Occurrence)]) -> Vec<SourceTarget> {
    let scene = doc.scene();
    members.iter().flat_map(|(h, o)| [scene.source_target(*h), o.mask.as_ref().map(|m| SourceTarget::Coverage(m.source))]).flatten().collect::<BTreeSet<_>>().into_iter().collect()
}
fn batch_bounds(doc: &Document, members: &[(OccurrenceHandle, Occurrence)]) -> Rect {
    members.iter().filter(|(_, o)| o.kind() == LayerKind::Paint).fold(Rect::EMPTY, |bounds, (handle, occurrence)| {
        bounds.union(occurrence.placement.post(Projective::from_affine(Affine::translation(doc.layer_offset(*handle))))
            .map_or(Rect::UNBOUNDED, |map| map.forward_bounds(source_frame(doc, *handle))))
    })
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(crate) fn begin_layer_placement(
        &mut self,
        imported: Option<PlacementInsertion>,
    ) -> Result<(), String> {
        self.begin_retained_placement(imported, false)
    }
    pub(super) fn begin_retained_placement(&mut self, imported: Option<PlacementInsertion>, moving: bool) -> Result<(), String> {
        if self.operation.active() { return Err("Finish the current transform first".into()); }
        let doc = self.engine.document();
        let active = doc.working.occurrence.ok_or("Select a paint or photo layer")?;
        let layer = doc.scene().occurrence(active).ok_or("Select a paint or photo layer")?.clone();
        let selected = self.layer_interaction.selected.clone();
        let roots = imported.as_ref().map_or_else(|| self.transform_roots(), |insert| insert.ids.clone());
        let ids = doc.retained_transform_targets(&roots).map_err(error)?;
        let members: Vec<_> = ids.iter().map(|id| doc.scene().occurrence(*id).cloned()
            .map(|o| (*id, o)).ok_or("The transformed layer was removed")).collect::<Result<_, _>>()?;
        let single = members.len() == 1 && layer.kind() == LayerKind::Paint;
        let bounds = if imported.is_some() || moving {
            self.measured_target_bounds().filter(|_| moving).unwrap_or_else(||
                if single { source_frame(doc, active) } else { batch_bounds(doc, &members) })
        } else { self.measured_target_bounds().ok_or("The content bounds are still being measured")? };
        if bounds.is_empty() { return Err("The layers have no pixels to transform".into()); }
        let (insertion, rollback, selected) = if let Some(insert) = imported {
            (Some(insert.index), insert.rollback, insert.selected)
        } else { (None, Edit::Batch(replacement_edits(doc, &members)?), selected) };
        let basis = if single { Affine::translation(doc.layer_offset(active)) } else { Affine::IDENTITY };
        let mut transaction = Transaction::new(self.operation.serial.wrapping_add(1), members.iter().find_map(|(h, _)| doc.scene().source_target(*h)).ok_or("The layers have no pixels to transform")?, None,
            doc.revision, basis, bounds, Pose::identity());
        transaction.source = source_frame(doc, active);
        transaction.retained_move = moving;
        if single {
            let map = &layer.placement;
            transaction.geometry.mesh = map.mesh.clone();
            transaction.geometry.inner = Some(map.outer);
            transaction.fold();
            transaction.geometry.interpolation = insertion.is_none().then_some(map.interpolation);
        } else {
            let mut filters = members.iter().filter(|(_, o)| o.kind() == LayerKind::Paint).map(|(_, o)| o.placement.interpolation);
            transaction.geometry.interpolation = filters.next().filter(|first| filters.all(|filter| filter == *first));
        }
        transaction.geometry.pivot = center(transaction.geometry.frame);
        transaction.start = transaction.geometry.clone();
        transaction.accepted = transaction.geometry.clone();
        self.operation.serial = self.operation.serial.wrapping_add(1);
        self.operation.current = Some(Transaction {
            placement: Some(Placement {
                source: self.engine.scene_snapshot(),
                members,
                initial: None,
                rollback,
                insertion,
                selected,
                roots: roots.clone(),
            }),
            ..transaction
        });
        if let Err(cause) = self.update_transform() {
            self.operation.current = None;
            self.operation.changed = true;
            self.refresh_tools();
            self.refresh_commands();
            return Err(cause);
        }
        self.operation.aspect = true;
        self.engine.backend_mut().prepare_moving_layer(Some(active));
        self.layer_interaction.editing = Some(active);
        self.layer_interaction.selected = roots.into_iter().collect();
        let tool = if moving { LayerCanvasTool::Move } else { LayerCanvasTool::Transform };
        self.layer_interaction.tool = tool;
        self.state.layer_tools.tool = tool;
        self.layer_interaction.changed = true;
        self.refresh_tools();
        self.refresh_commands();
        Ok(())
    }

    pub(super) fn finish_layer_placement(&mut self, apply: bool) -> Result<(), String> {
        let t = self.operation.current.as_ref().ok_or("No active placement")?;
        let placement = t.placement.as_ref().ok_or("No active placement")?;
        let current: Vec<_> = placement.members.iter().map(|(handle, _)|
            self.engine.document().scene().occurrence(*handle).cloned().map(|o| (*handle, o)).ok_or("The placed layer was removed")
        ).collect::<Result<_, _>>()?;
        if apply && (t.revision != self.engine.document().revision
            || current.iter().any(|(handle, _)| self.engine.document().is_locked(*handle))) {
            return Err("The destination changed; cancel this placement".into());
        }
        let selected = if apply && placement.insertion.is_some() {
            current.iter().map(|(handle, _)| *handle).collect()
        } else { placement.selected.clone() };
        let mut original = self.engine.document().clone();
        let restore_preview = original.apply(placement.rollback.clone()).map_err(error)?;
        let repeat = placement.repeat_delta(t);
        let changed = placement.insertion.is_some() || current.iter().any(|(h, o)| original.scene().occurrence(*h) != Some(o));
        if changed {
            let rollback = placement.rollback.clone();
            let mut edits = vec![restore_preview.clone()];
            if apply {
                edits.extend(self.engine.document().paint_extent_plan(&member_targets(self.engine.document(), &current), self.engine.geometry_limits()).map_err(error)?);
                original.apply(Edit::Batch(edits.clone())).map_err(error)?;
            }
            self.engine.preview_edit(rollback).map_err(error)?;
            if apply && let Err(cause) = self.engine.apply_edit(Edit::Batch(edits)) {
                self.engine.preview_edit(restore_preview).map_err(error)?;
                self.operation.current.as_mut().unwrap().revision = self.engine.document().revision;
                return Err(error(cause));
            }
        }
        if apply && changed && let Some(delta) = repeat { self.operation.last_transform = Some(delta); }
        self.operation.nudging = None;
        self.operation.current = None;
        self.engine.backend_mut().prepare_moving_layer(None);
        self.layer_interaction.editing = self.engine.document().working.occurrence;
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
        if !self.operation.original_size_available() { return Err("Original Size requires a photo layer".into()); }
        let t = self
            .operation
            .current
            .as_mut()
            .filter(|t| t.placement.is_some())
            .ok_or("Select an active photo placement")?;
        let pivot_position = t.pivot();
        let placement = t.placement.as_mut().unwrap();
        let previous = (placement.members.clone(), placement.initial.clone(), t.bounds);
        if placement.members.len() > 1 {
            let mut members = Vec::with_capacity(placement.members.len());
            for (handle, _) in &placement.members {
                let mut layer = self.engine.document().scene().occurrence(*handle).cloned().ok_or("The placed layer was removed")?;
                let local = source_frame(self.engine.document(), *handle);
                let pivot = center(local);
                let affine = layer.placement.as_affine().ok_or("Original Size requires a photo without Distort or Warp")?;
                let position = affine.map(pivot);
                let [a, b, c, d, _, _] = affine.0;
                layer.placement = LayerPlacement::from_affine(Affine::around(pivot,
                    [1., (a * d - b * c).signum()], b.atan2(a), sub(position, pivot)));
                members.push((*handle, layer));
            }
            placement.initial.get_or_insert_with(|| placement.members.clone());
            placement.members = members;
            t.bounds = batch_bounds(self.engine.document(), &placement.members);
            t.geometry.frame = t.bounds;
            t.geometry.pose = Pose::identity();
            t.geometry.inner = None;
            t.geometry.exact_affine = None;
        } else {
            let affine = t.map().and_then(|map| map.as_affine()).ok_or("Original Size requires an affine photo")?;
            let pivot = center(t.bounds);
            let position = affine.map(pivot);
            let [a, b, c, d, _, _] = affine.0;
            t.geometry.inner = Some(Projective::from_affine(Affine::around(pivot,
                [1., (a * d - b * c).signum()], b.atan2(a), sub(position, pivot))));
            t.geometry.pose = Pose::identity();
            t.fold();
        }
        t.geometry.pivot = t.pose_affine().inverse().map_or(pivot_position, |map| map.map(pivot_position));
        let result = self.update_transform();
        if result.is_err() {
            let t = self.operation.current.as_mut().unwrap();
            let placement = t.placement.as_mut().unwrap();
            (placement.members, placement.initial, t.bounds) = previous;
        }
        result
    }
}
