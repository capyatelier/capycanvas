use super::*;
use crate::input::to_stroke_point;

pub(super) struct EstimatedPoint {
    pub stroke: StrokeId,
    pub index: usize,
    pub copies: Vec<(usize, layer_core::StrokePoint)>,
    point: layer_core::StrokePoint,
    transform: ViewTransform,
    curve: PressureCurve,
}

impl<B: CanvasRenderer> CanvasEngine<B> {
    pub(super) fn track_estimate(&mut self, event: PenEvent, transform: ViewTransform) {
        if !event.flags.contains(SampleFlags::ESTIMATED)
            || event.flags.contains(SampleFlags::PREDICTED)
        {
            return;
        }
        let Some(active) = &self.active_stroke else {
            return;
        };
        let index = self.builder.real_points().len() - 1;
        let point = *self.builder.real_points().last().unwrap();
        if let Some(existing) = self.estimates.get_mut(&(event.device_id, event.sequence)) {
            // A terminal delivery can repeat the last coalesced observation.
            // Both stored points belong to the same later sensor correction.
            if existing.stroke == active.id {
                existing.copies.push((index, point));
                return;
            }
        }
        // Adapters release unresolved tokens on teardown. This last-resort
        // bound also protects hosts whose input driver stops sending updates.
        if self.estimates.len() >= 8192 {
            self.estimates.pop_first();
            self.metrics.expired_input_estimates += 1;
        }
        self.estimates.insert(
            (event.device_id, event.sequence),
            EstimatedPoint {
                stroke: active.id,
                index,
                copies: Vec::new(),
                point,
                transform,
                curve: self.pressure,
            },
        );
    }

    pub(super) fn correct_input(&mut self, event: PenEvent) -> Result<(), EngineError<B::Error>> {
        let key = (event.device_id, event.sequence);
        let Some(mut estimate) = self.estimates.remove(&key) else {
            return Ok(());
        };
        // All spatial/pressure policy belongs to the original observation.
        let mut point = to_stroke_point(event, estimate.transform, estimate.curve, 0);
        point.elapsed_micros = estimate.point.elapsed_micros;
        let mut changed = false;
        if point != estimate.point
            || estimate.copies.iter().any(|(_, copy)| {
                *copy
                    != layer_core::StrokePoint {
                        elapsed_micros: copy.elapsed_micros,
                        ..point
                    }
            })
        {
            let active = self
                .active_stroke
                .as_ref()
                .is_some_and(|s| s.id == estimate.stroke);
            for (index, old) in std::iter::once((estimate.index, estimate.point))
                .chain(estimate.copies.iter().copied())
            {
                let new = layer_core::StrokePoint {
                    elapsed_micros: old.elapsed_micros,
                    ..point
                };
                if new == old {
                    continue;
                }
                if active {
                    self.builder.replace_real(index, new);
                    changed = true;
                } else {
                    changed |= self
                        .editor
                        .correct_stroke_point(estimate.stroke, index, old, new)
                        .map_err(EngineError::Document)?;
                }
            }
            if changed {
                self.metrics.corrected_input_samples += 1;
                if active {
                    let active = self.active_stroke.as_ref().unwrap();
                    if !active.feedback.enabled || estimate.index < self.finalized_real_points {
                        self.rebuild_corrected_active();
                    }
                } else if self.document().stroke(estimate.stroke).is_some() {
                    self.rebuild_all = true;
                }
                estimate.point = point;
                for (_, copy) in &mut estimate.copies {
                    *copy = layer_core::StrokePoint {
                        elapsed_micros: copy.elapsed_micros,
                        ..point
                    };
                }
            }
        }
        if event.flags.contains(SampleFlags::ESTIMATED) {
            self.estimates.insert(key, estimate);
        }
        Ok(())
    }

    fn rebuild_corrected_active(&mut self) {
        let active = self.active_stroke.as_mut().unwrap();
        let count = if active.feedback.enabled {
            self.finalized_real_points
        } else {
            self.builder.real_points().len()
        };
        active.persistent_started = false;
        active.committed_smudge_dabs = 0;
        self.dab_generator
            .reset_for_stroke(active.id, &active.brush);
        self.pending_smudge_dabs.clear();
        // A correction may invalidate persistent pigment or smudge work. Replay
        // faithfully; do not try to cover the old result with a second stroke.
        self.dabs.clear();
        self.batches.clear();
        for index in 0..count {
            self.append_real_dab(self.builder.real_points()[index], index);
        }
        self.rebuild_all = true;
    }
}
