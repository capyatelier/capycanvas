//! Amend estimated input in its original history step, including redoable ink.
use super::*;

impl Editor {
    /// Replace one still-matching estimated point. No undo step is added and
    /// redo remains available. Saved states containing the changed point get
    /// fresh checkpoint identities; states before the stroke retain theirs.
    /// The expected value prevents a delayed sensor update overwriting a later
    /// explicit edit to the same point. Captured snapshots keep their old Arc.
    pub fn correct_stroke_point(
        &mut self,
        id: StrokeId,
        index: usize,
        expected: StrokePoint,
        mut point: StrokePoint,
    ) -> Result<bool, DocumentError> {
        if !point.position.x.is_finite()
            || !point.position.y.is_finite()
            || !point.pressure.is_finite()
            || !point.tilt.into_iter().all(f32::is_finite)
            || !point.twist.is_finite()
        {
            return Err(DocumentError::NonFiniteStroke);
        }
        point.pressure = point.pressure.clamp(0., 1.);
        if point == expected {
            return Ok(false);
        }
        fn replace(
            stroke: &mut Stroke,
            index: usize,
            old: StrokePoint,
            point: StrokePoint,
        ) -> bool {
            if stroke.points.get(index) != Some(&old) {
                return false;
            }
            Arc::make_mut(&mut stroke.points)[index] = point;
            stroke.bounds = Rect::EMPTY;
            let radius = stroke.brush.conservative_radius();
            for p in stroke.points.iter() {
                stroke.bounds.include_circle(p.position, radius);
            }
            true
        }
        fn amend(
            edit: &mut Edit,
            id: StrokeId,
            index: usize,
            old: StrokePoint,
            point: StrokePoint,
            affected: &mut bool,
            changed: &mut bool,
        ) {
            match edit {
                Edit::Batch(edits) => {
                    for edit in edits {
                        amend(edit, id, index, old, point, affected, changed);
                    }
                }
                Edit::InsertStroke(stroke) if stroke.id == id => {
                    *affected = replace(stroke, index, old, point);
                    *changed |= *affected;
                }
                Edit::RemoveStroke { id: target } if *target == id => *affected = false,
                _ => {}
            }
        }
        fn fresh(checkpoint: &mut u64, next: &mut u64, remapped: &mut BTreeMap<u64, u64>) {
            *checkpoint = *remapped.entry(*checkpoint).or_insert_with(|| {
                let result = *next;
                *next = next.checked_add(1).expect("document history exhausted");
                result
            });
        }
        let current = self
            .document
            .strokes
            .get_mut(&id)
            .is_some_and(|stroke| replace(stroke, index, expected, point));
        let mut changed = current;
        let mut remapped = BTreeMap::new();
        if current {
            fresh(
                &mut self.checkpoint,
                &mut self.next_checkpoint,
                &mut remapped,
            );
        }
        for history in [&mut self.undo, &mut self.redo] {
            let mut affected = current;
            for entry in history.iter_mut().rev() {
                amend(
                    &mut entry.edit,
                    id,
                    index,
                    expected,
                    point,
                    &mut affected,
                    &mut changed,
                );
                if affected {
                    fresh(
                        &mut entry.checkpoint,
                        &mut self.next_checkpoint,
                        &mut remapped,
                    );
                }
            }
        }
        if changed {
            self.document.revision = self.document.revision.saturating_add(1);
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn point(x: f32) -> StrokePoint {
        StrokePoint {
            position: Point { x, y: 4. },
            pressure: 0.5,
            tilt: [0.; 2],
            twist: 0.,
            elapsed_micros: 0,
        }
    }
    #[test]
    fn corrections_preserve_history_snapshots_and_saved_states_before_the_stroke() {
        for undone in [false, true] {
            let mut editor = Editor::new(Document::new("corrections", 64, 64));
            let clean = editor.checkpoint();
            let id = editor.allocate_stroke_id();
            editor
                .perform(Edit::InsertStroke(Box::new(
                    Stroke::new(
                        id,
                        LayerId(1),
                        StrokeTool::Brush,
                        BrushSnapshot::default(),
                        vec![point(10.)],
                    )
                    .unwrap(),
                )))
                .unwrap();
            let saved = editor.checkpoint();
            let snapshot = editor.document().clone();
            editor
                .perform(Edit::SetLayerOpacity {
                    id: LayerId(1),
                    opacity: 0.7,
                })
                .unwrap();
            if undone {
                editor.undo().unwrap();
                editor.undo().unwrap();
            }
            assert!(
                editor
                    .correct_stroke_point(id, 0, point(10.), point(20.))
                    .unwrap()
            );
            assert_eq!(snapshot.stroke(id).unwrap().points[0], point(10.));
            if undone {
                assert_eq!(editor.checkpoint(), clean);
                editor.redo().unwrap();
            } else {
                editor.undo().unwrap();
            }
            assert_ne!(editor.checkpoint(), saved);
            assert_eq!(editor.document().stroke(id).unwrap().points[0], point(20.));
            editor.undo().unwrap();
            assert_eq!(editor.checkpoint(), clean);
            assert!(editor.document().stroke(id).is_none());
            editor.redo().unwrap();
            assert_eq!(editor.document().stroke(id).unwrap().points[0], point(20.));
            assert!(
                !editor
                    .correct_stroke_point(id, 0, point(10.), point(30.))
                    .unwrap()
            );
        }
    }
}
