use super::*;
use crate::input::calibrated_stroke_point;

pub(super) struct EstimatedPoint {
    pub stroke: StrokeId,
    pub index: usize,
    pub copies: Vec<(usize, layer_core::StrokePoint)>,
    point: layer_core::StrokePoint,
    transform: ViewTransform,
    pub(super) curve: PressureCurve,
    pub(super) tilt: crate::TiltCurve,
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
        }
        self.estimates.insert(
            (event.device_id, event.sequence),
            EstimatedPoint {
                stroke: active.id,
                index,
                copies: Vec::new(),
                point,
                transform,
                curve: self.settings.pressure.clone(),
                tilt: self.settings.tilt.clone(),
            },
        );
    }

    pub(super) fn correct_input(&mut self, event: PenEvent) -> Result<bool, EngineError<B::Error>> {
        let key = (event.device_id, event.sequence);
        let Some(mut estimate) = self.estimates.remove(&key) else {
            return Ok(false);
        };
        // All spatial/pressure policy belongs to the original observation.
        let mut point = calibrated_stroke_point(event, estimate.transform, estimate.curve.clone(), &estimate.tilt, 0);
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
                    self.recording
                        .event(crate::recording::Event::Replace(index, new.into()));
                    changed = true;
                } else {
                    if let Some(stroke) = self.completed_stroke.as_mut()
                        && stroke.id == estimate.stroke
                        && stroke.points.get(index) == Some(&old)
                    {
                        std::sync::Arc::make_mut(&mut stroke.points)[index] = new;
                        changed = true;
                    }
                }
            }
            if changed {
                if active {
                    if estimate.index == 0 {
                        self.reanchor_clone_stroke();
                    }
                    let active = self.active_stroke.as_mut().unwrap();
                    // A late sensor update is not fresh pressure/motion. Drop
                    // preview history instead of treating delivery as input.
                    active.tip.prediction = crate::feedback::PredictionState::default();
                    self.recording.event(crate::recording::Event::Reset);
                    if !active.feedback.enabled || estimate.index < self.finalized_real_points {
                        self.rebuild_corrected_active();
                    }
                } else {
                    self.replay_completed().map_err(EngineError::Document)?;
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
        Ok(changed)
    }

    fn rebuild_corrected_active(&mut self) {
        let active = self.active_stroke.as_mut().unwrap();
        let momentum = active.momentum.take().map(|(_, emitted)| emitted);
        let count = if active.feedback.enabled {
            self.finalized_real_points
        } else {
            self.builder.real_points().len()
        };
        active.persistent_started = false;
        active.committed_chunk_dabs = 0;
        self.dab_generator
            .reset_for_stroke(active.id, &active.brush);
        self.dab_generator.set_barrel_twist(active.barrel_twist);
        if let Some(taper) = active.taper.as_mut() {
            taper.restart();
            self.dab_generator.begin_taper(&active.brush);
        }
        self.pending_chunk_dabs.clear();
        // A correction may invalidate persistent pigment or smudge work. Replay
        // faithfully; do not try to cover the old result with a second stroke.
        self.dabs.clear();
        self.batches.clear();
        for index in 0..count {
            self.append_real_dab(Some((self.builder.real_points()[index], index)), false);
        }
        if let Some(emitted) = momentum {
            let active = self.active_stroke.as_mut().unwrap();
            self.dab_generator.finish_contact(&active.brush, &mut self.pending_chunk_dabs);
            let tail = self.dab_generator.momentum(&active.brush);
            let emitted = emitted.min(tail.len());
            self.pending_chunk_dabs.extend_from_slice(&tail[..emitted]);
            active.momentum = Some((tail, emitted));
            self.flush_material_chunks(false);
        }
        self.rebuild_all = true;
    }
}
