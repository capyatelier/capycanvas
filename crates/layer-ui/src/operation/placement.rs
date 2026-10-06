use super::*;
use layer_core::Edit;
use layer_core::authored::{Occurrence, OccurrenceHandle, RecordChange, SourceTarget};
use std::collections::BTreeSet;

pub(super) struct Placement {
    pub(super) members: Vec<(OccurrenceHandle, Occurrence)>,
    rollback: Edit,
    selected: BTreeSet<OccurrenceHandle>,
    roots: Vec<OccurrenceHandle>,
}
impl Placement {
    fn repeat_delta(&self, t: &Transaction) -> Option<Projective> {
        if t.start.mesh.as_ref() != t.mapped_mesh() { return None; }
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
    pub(super) fn preview_occurrences(&self, doc: &Document, map: &LayerPlacement, moves_only: &str) -> Result<Vec<(OccurrenceHandle, Occurrence)>, String> {
        let delta = map.as_affine().filter(|affine| map.mesh.is_none() && affine.0[..4] == Affine::IDENTITY.0[..4])
            .and_then(|affine| layer_core::offsets::rounded(Point { x: affine.0[4], y: affine.0[5] }))
            .ok_or_else(|| moves_only.to_string())?;
        let mut candidate = doc.clone();
        candidate.apply(Edit::Batch(replacement_edits(&candidate, &self.members)?)).map_err(error)?;
        let edit = candidate.move_layers_edit(&self.roots, delta).map_err(error)?;
        candidate.apply(edit).map_err(error)?;
        self.members.iter().map(|(handle, _)| {
            let occurrence = candidate.scene().occurrence(*handle).cloned().ok_or("The transformed layer was removed")?;
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
    members.iter().filter(|(_, o)| matches!(o.kind(), LayerKind::Paint | LayerKind::Object)).fold(Rect::EMPTY, |bounds, (handle, _)| {
        let offset = layer_core::offsets::point(doc.layer_offset(*handle));
        let frame = source_frame(doc, *handle);
        bounds.union(Rect { min: add(frame.min, offset), max: add(frame.max, offset) })
    })
}

impl<R: CanvasRenderer> UiSession<R> {
    /// Start moving the checked layers, or the active one, by whole pixels.
    pub(super) fn begin_layer_move(&mut self) -> Result<(), String> {
        if self.operation.active() { return Err("Finish the current transform first".into()); }
        let doc = self.engine.document();
        let selected = self.selected_layers().clone();
        let roots = self.transform_roots();
        let ids = doc.layer_move_targets(&roots).map_err(error)?;
        let members: Vec<_> = ids.iter().map(|id| doc.scene().occurrence(*id).cloned()
            .map(|o| (*id, o)).ok_or("The transformed layer was removed")).collect::<Result<_, _>>()?;
        let (active, _) = members.iter().find(|(_, o)| matches!(o.kind(), LayerKind::Paint | LayerKind::Object))
            .ok_or("The layers have no pixels to transform")?;
        let active = *active;
        let single = members.len() == 1;
        let objects = members.iter().any(|(_, o)| o.kind() == LayerKind::Object);
        let bounds = self.measured_target_bounds().filter(|_| !objects).unwrap_or_else(||
            if single { source_frame(doc, active) } else { batch_bounds(doc, &members) });
        if bounds.is_empty() { return Err("The layers have no pixels to transform".into()); }
        let rollback = Edit::Batch(replacement_edits(doc, &members)?);
        let basis = if single { Affine::translation(layer_core::offsets::point(doc.layer_offset(active))) } else { Affine::IDENTITY };
        let mut transaction = Transaction::new(self.operation.serial.wrapping_add(1), members.iter().find_map(|(h, _)| doc.scene().source_target(*h)), None,
            doc.revision, basis, bounds, Pose::identity());
        transaction.source = source_frame(doc, active);
        transaction.layer_move = true;
        transaction.geometry.pivot = center(transaction.geometry.frame);
        transaction.start = transaction.geometry.clone();
        transaction.accepted = transaction.geometry.clone();
        self.operation.serial = self.operation.serial.wrapping_add(1);
        self.operation.current = Some(Transaction {
            placement: Some(Placement { members, rollback, selected, roots }),
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
        self.layer_interaction.tool = LayerCanvasTool::Move;
        self.state.layer_tools.tool = LayerCanvasTool::Move;
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
        let selected = placement.selected.clone();
        let mut original = self.engine.document().clone();
        let restore_preview = original.apply(placement.rollback.clone()).map_err(error)?;
        let repeat = placement.repeat_delta(t);
        let changed = current.iter().any(|(h, o)| original.scene().occurrence(*h) != Some(o));
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
        self.set_selected_layers(selected)?;
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
}
