//! Platform-neutral policy for the replaceable tip of an active stroke.

use crate::input::{PenEvent, SampleFlags, ToolKind};
use layer_core::{Point, StrokePoint};
use std::collections::VecDeque;

mod drawing_state;
mod local_motion;
#[cfg(test)]
mod local_motion_tests;
mod motion_fit;
mod output;
mod prediction_clock;
#[cfg(test)]
mod test_support;

pub(crate) const MAX_FINALIZATION_LAG_MICROS: u32 = 50_000;
const MAX_PREDICTION_HORIZON_MICROS: u32 = 64_000;
const MAX_PREDICTION_DISTANCE_PX: f32 = 512.0;

/// Supported engine predictors; retained as a setting for future alternatives.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredictionAlgorithm {
    #[default]
    Optimized,
}

/// Runtime-tunable instant-feedback policy. This is interaction state, not part
/// of a brush preset or persisted stroke.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct InstantFeedbackConfig {
    pub enabled: bool,
    pub use_platform_prediction: bool,
    pub use_engine_prediction: bool,
    pub prediction_algorithm: PredictionAlgorithm,
    /// Input clock quantum, supplied by the host (GTK/GDK: 1 ms). This is
    /// measurement uncertainty, not prediction time or a user preference.
    pub timestamp_resolution_micros: u32,
    /// Real input newer than this remains in the replaceable tail.
    pub finalization_lag_micros: u32,
    /// Engine lookahead, also used without a presentation timestamp. Native
    /// samples use their own horizon, capped independently at 64 ms.
    pub prediction_horizon_micros: u32,
    /// Physical-pixel distance limit, independent of document zoom. Smooth Motion
    /// prediction applies this to future travel beyond frame time, separately
    /// compensating for input age; total extrapolation is still capped at 512 px.
    /// Native predictors measure this from the latest observation.
    pub max_prediction_distance_px: f32,
    /// `0` preserves modeled geometry; `1` puts terminal coverage at the tip.
    pub tip_lock: f32,
    /// Power applied to the smooth endpoint-correction envelope.
    pub correction_easing: f32,
    /// Below this physical-pixel velocity, the engine does not extrapolate.
    pub minimum_prediction_speed_px_per_second: f32,
    /// Suppression applied as recent motion approaches a right-angle turn.
    pub corner_suppression: f32,
}

impl Default for InstantFeedbackConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            use_platform_prediction: true,
            use_engine_prediction: true,
            prediction_algorithm: PredictionAlgorithm::default(),
            timestamp_resolution_micros: 1,
            finalization_lag_micros: 8_000,
            prediction_horizon_micros: 8_000,
            max_prediction_distance_px: 96.0,
            tip_lock: 1.0,
            correction_easing: 1.5,
            minimum_prediction_speed_px_per_second: 12.0,
            corner_suppression: 1.0,
        }
    }
}

impl InstantFeedbackConfig {
    pub fn validate(self) -> Result<(), FeedbackConfigError> {
        let finite = [
            self.max_prediction_distance_px,
            self.tip_lock,
            self.correction_easing,
            self.minimum_prediction_speed_px_per_second,
            self.corner_suppression,
        ]
        .iter()
        .all(|value| value.is_finite());
        if !finite
            || self.finalization_lag_micros > MAX_FINALIZATION_LAG_MICROS
            || self.timestamp_resolution_micros > MAX_PREDICTION_HORIZON_MICROS
            || self.prediction_horizon_micros > MAX_PREDICTION_HORIZON_MICROS
            || !(0.0..=MAX_PREDICTION_DISTANCE_PX).contains(&self.max_prediction_distance_px)
            || !(0.0..=1.0).contains(&self.tip_lock)
            || !(0.25..=4.0).contains(&self.correction_easing)
            || !(0.0..=10_000.0).contains(&self.minimum_prediction_speed_px_per_second)
            || !(0.0..=1.0).contains(&self.corner_suppression)
        {
            return Err(FeedbackConfigError);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FeedbackConfigError;

impl std::fmt::Display for FeedbackConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("invalid instant-feedback configuration")
    }
}

impl std::error::Error for FeedbackConfigError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TipSource {
    Real,
    Platform,
    Engine,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TipEstimate {
    pub point: StrokePoint,
    pub source: TipSource,
}

/// Per-contact, preview-only state. Never changes recorded stroke samples.
#[derive(Clone, Debug, Default)]
pub(crate) struct PredictionState {
    correction_field: Option<output::CorrectionField>,
    pressure: VecDeque<(u64, f32)>,
    lead: Option<(u32, f32, Point)>,
    output: Option<output::Output>,
    immediate: MotionState,
    sustained: MotionState,
    policy: Option<InstantFeedbackConfig>,
    last_motion: Option<motion_fit::MotionFit>,
    continuing: bool,
    transform: Option<[f32; 6]>,
}

/// Motion and timing components of Smooth Motion. Neither is an independently
/// selectable predictor: the immediate component supplies prompt stop/turn
/// response, while the sustained component preserves ordinary preview continuity.
#[derive(Clone, Debug, Default)]
struct MotionState {
    clock: prediction_clock::PredictionClock,
    local: Option<local_motion::LocalMotion>,
    lead_time: Option<(u32, u32, f64)>,
}

impl MotionState {
    fn forecast(
        &mut self,
        real: &[StrokePoint],
        motion: &motion_fit::MotionFit,
        requested: u32,
        now: u32,
        lifetime: u32,
        transform: [f32; 6],
        config: InstantFeedbackConfig,
        history: Option<drawing_state::DrawingState>,
        continuity: f64,
    ) -> Option<output::Output> {
        let age = now.saturating_sub(real.last()?.elapsed_micros);
        let requested = requested
            .saturating_sub(now.max(real.last()?.elapsed_micros))
            .min(config.prediction_horizon_micros);
        let horizon = if requested == 0 {
            0
        } else {
            self.clock.horizon(
                motion,
                requested,
                now,
                lifetime,
                config,
                history.map_or(0., |h| h.reach),
            )
        };
        let memory = history.map(|h| (config.prediction_horizon_micros, h.smooth));
        let coherence = history.map_or(continuity, |h| h.continuity);
        if requested == 0 || (horizon == 0 && coherence == 0.) {
            self.lead_time = None;
            // Preserve hidden sustained motion through a temporary statistical
            // confidence loss; timing phase must not pulse off and on.
            self.local = memory.and_then(|memory| {
                local_motion::LocalMotion::fit(
                    real,
                    transform,
                    config.timestamp_resolution_micros,
                    config.prediction_horizon_micros,
                    Some(memory),
                    self.local.as_ref(),
                    continuity,
                )
            });
            return None;
        }
        let mut output = output::Output::new(
            motion.clone(),
            horizon,
            transform,
            motion.display_distance_budget(age, config),
        )
        .with_local_motion(real, config, age, memory, self.local.as_ref(), continuity);
        // Preserve display lead through fit-window confidence changes. Fresh
        // local geometry still follows the pen; a confirmed stop/turn clears
        // coherence and bypasses this length memory immediately.
        let sample = real.last()?.elapsed_micros;
        let desired = f64::from(output.horizon) - f64::from(age);
        let lead = self.lead_time.map_or(desired, |(before, before_now, old)| {
            if coherence > 0. && sample >= before && sample - before <= 32_000 {
                if sample == before {
                    // No new evidence: consume the existing forecast instead
                    // of advancing its endpoint on every repaint.
                    return old - f64::from(now.saturating_sub(before_now));
                }
                desired
                    + (old - desired)
                        * (-f64::from(sample - before)
                            / (if desired > old {
                                8_000.
                            } else {
                                48_000. * coherence
                            }))
                        .exp()
            } else {
                desired
            }
        });
        self.lead_time = Some((sample, now, lead));
        output.horizon = (f64::from(age) + lead).round().max(0.) as u32;
        output.horizon = output
            .horizon
            .min(age.saturating_add(requested))
            .min(MAX_PREDICTION_HORIZON_MICROS);
        self.local = output.local_motion();
        (output.horizon > 0).then_some(output)
    }
}

impl PredictionState {
    pub fn observe(&mut self, event: PenEvent) {
        if event.flags.contains(SampleFlags::PREDICTED)
            || event.flags.contains(SampleFlags::CORRECTION)
        {
            return;
        }
        if event.flags.contains(SampleFlags::ESTIMATED)
            || !matches!(
                event.tool,
                ToolKind::Pen
                    | ToolKind::Eraser
                    | ToolKind::Brush
                    | ToolKind::Pencil
                    | ToolKind::Airbrush
            )
            || !event.pressure.is_finite()
        {
            self.pressure.clear();
            return;
        }
        if let Some(&(time, pressure)) = self.pressure.back() {
            if event.timestamp_ns <= time {
                return;
            }
            // A rebound ends the release gesture; do not keep an old falling
            // trend alive through a fresh press or a gap in the input stream.
            if event.pressure > pressure + 0.01 || event.timestamp_ns - time > 32_000_000 {
                self.pressure.clear();
            }
        }
        self.pressure
            .push_back((event.timestamp_ns, event.pressure.clamp(0.0, 1.0)));
        while self.pressure.len() > 16
            || self
                .pressure
                .front()
                .is_some_and(|&(time, _)| event.timestamp_ns - time > 24_000_000)
        {
            self.pressure.pop_front();
        }
    }

    fn lift_horizon(&self) -> Option<u32> {
        if self.pressure.len() < 3 {
            return None;
        }
        let &(start, first) = self.pressure.front()?;
        let &(end, last) = self.pressure.back()?;
        if end - start < 6_000_000 || first - last < 0.04 {
            return None;
        }
        let n = self.pressure.len() as f64;
        let mean_t = self
            .pressure
            .iter()
            .map(|&(t, _)| (t - start) as f64 / 1000.0)
            .sum::<f64>()
            / n;
        let mean_p = self
            .pressure
            .iter()
            .map(|&(_, p)| f64::from(p))
            .sum::<f64>()
            / n;
        let mut covariance = 0.0;
        let mut variance = 0.0;
        for &(time, pressure) in &self.pressure {
            let dt = (time - start) as f64 / 1000.0 - mean_t;
            covariance += dt * (f64::from(pressure) - mean_p);
            variance += dt * dt;
        }
        let slope = covariance / variance;
        if slope >= -0.000_001 {
            return None;
        }
        // Predict only halfway to the estimated zero-pressure time. This is a
        // safety bound, not a synthetic pen-up or a brush-pressure adjustment.
        Some((f64::from(last) / -slope * 0.5).clamp(0.0, 64_000.0) as u32)
    }

    #[cfg(test)]
    pub fn estimate(
        &mut self,
        real: &[StrokePoint],
        platform: &[StrokePoint],
        requested: u32,
        transform: [f32; 6],
        config: InstantFeedbackConfig,
    ) -> Option<TipEstimate> {
        self.estimate_for(
            real,
            platform,
            requested,
            real.last()?.elapsed_micros,
            transform,
            config,
        )
    }

    pub fn estimate_for(
        &mut self,
        real: &[StrokePoint],
        platform: &[StrokePoint],
        requested: u32,
        now: u32,
        transform: [f32; 6],
        config: InstantFeedbackConfig,
    ) -> Option<TipEstimate> {
        if self.policy != Some(config) || self.transform != Some(transform) {
            self.immediate = MotionState::default();
            self.sustained = MotionState::default();
            self.correction_field = None;
            self.last_motion = None;
            self.continuing = false;
            self.output = None;
            self.policy = Some(config);
            self.transform = Some(transform);
        }
        let latest = *real.last()?;
        let previous_output = self.output.take();
        let horizon = self.lift_horizon();
        let mut drawing = if config.use_engine_prediction {
            drawing_state::DrawingState::measure(real, transform)
        } else {
            Default::default()
        };
        let interrupted = drawing.interrupted
            || drawing.stop_in_micros
                <= f64::from(
                    config
                        .prediction_horizon_micros
                        // Pressure release confirms an earlier stop alarm.
                        // Otherwise use an 8 ms warning beyond the target.
                        .saturating_add(if horizon.is_some_and(|h| h < 48_000) {
                            24_000
                        } else {
                            8_000
                        })
                        .saturating_add(now.saturating_sub(latest.elapsed_micros)),
                );
        if let Some(p) = previous_output.as_ref() {
            if latest.elapsed_micros > p.sample_time() {
                self.continuing = true;
            }
        } else {
            self.continuing = false;
        }
        // A continuation requires an existing forecast and a fresh report.
        // Repainting the first forecast must not manufacture this evidence.
        if !self.continuing {
            drawing.continuity = 0.;
        }
        if interrupted {
            drawing.continuity = 0.;
            self.last_motion = None;
            self.correction_field = None;
        }
        let mut intervals = [0; 8];
        let mut count = 0;
        for pair in real.windows(2).rev() {
            if pair[1].elapsed_micros > pair[0].elapsed_micros {
                intervals[count] = pair[1].elapsed_micros - pair[0].elapsed_micros;
                count += 1;
                if count == intervals.len() {
                    break;
                }
            }
        }
        intervals[..count].sort_unstable();
        let period = if count > 0 {
            intervals[count / 2]
        } else {
            config.prediction_horizon_micros
        };
        let lifetime = period
            .saturating_mul(2)
            .max(config.prediction_horizon_micros)
            .min(MAX_PREDICTION_HORIZON_MICROS);
        if now.saturating_sub(latest.elapsed_micros) > lifetime {
            self.output = None;
            self.immediate = MotionState::default();
            self.sustained = MotionState::default();
            self.lead = None;
            self.last_motion = None;
            self.continuing = false;
            return Some(TipEstimate {
                point: latest,
                source: TipSource::Real,
            });
        }
        let requested = requested.max(latest.elapsed_micros);
        let limited = horizon.map_or(requested, |h| {
            requested.min(latest.elapsed_micros.saturating_add(h))
        });
        let native = config.use_platform_prediction
            && platform
                .iter()
                .any(|p| p.elapsed_micros > latest.elapsed_micros);
        if config.use_engine_prediction && !native {
            self.lead = None;
            let measured =
                motion_fit::MotionFit::fit(real, transform, config.timestamp_resolution_micros);
            // Model-window selection can briefly fail on correlated report noise.
            // Bridge only a corroborated continuation, never a stop/turn or stale
            // stream. Local geometry is still refit from the latest real samples.
            let motion = measured.clone().or_else(|| {
                self.last_motion
                    .as_ref()
                    .filter(|old| {
                        drawing.continuity > 0.25
                            && latest
                                .elapsed_micros
                                .checked_sub(old.point_at(0).elapsed_micros)
                                .is_some_and(|age| age <= 24_000)
                    })
                    .map(|old| old.advanced_to(latest, transform))
            });
            if measured.is_some() {
                self.last_motion = measured;
            }
            if let Some(motion) = motion {
                let requested = if interrupted { limited } else { requested };
                // Two components of Smooth Motion: keep the immediate fit warm
                // while sustained motion retains confidence and corrections.
                // The expensive trajectory fit and drawing-state scan are shared.
                let immediate = self.immediate.forecast(
                    real,
                    &motion,
                    requested,
                    now,
                    lifetime,
                    transform,
                    config,
                    None,
                    if interrupted { 0. } else { drawing.continuity },
                );
                let sustained = self.sustained.forecast(
                    real,
                    &motion,
                    requested,
                    now,
                    lifetime,
                    transform,
                    config,
                    Some(drawing),
                    0.,
                );
                let output = if drawing.speed < 1400. || interrupted || drawing.reach == 0. {
                    immediate
                } else {
                    sustained
                };
                self.output = if interrupted {
                    self.correction_field = None;
                    output
                } else {
                    output.map(|output| {
                        let (output, field) = output.smooth_corrections(
                            self.correction_field.as_ref(),
                            now,
                            drawing.smooth,
                        );
                        self.correction_field = Some(field);
                        output
                    })
                };
                return Some(self.output.as_ref().map_or(
                    TipEstimate {
                        point: latest,
                        source: TipSource::Real,
                    },
                    |output| TipEstimate {
                        point: output.point_at(output.horizon),
                        source: TipSource::Engine,
                    },
                ));
            }
            self.immediate = MotionState::default();
            self.sustained = MotionState::default();
            return Some(TipEstimate {
                point: latest,
                source: TipSource::Real,
            });
        }

        self.immediate = MotionState::default();
        self.sustained = MotionState::default();
        self.correction_field = None;
        self.last_motion = None;
        self.continuing = false;
        let mut estimate = estimate_tip(real, platform, limited, transform, config)?;
        if let Some(distance) = motion_fit::braking_distance(
            real,
            transform,
            config.timestamp_resolution_micros,
            estimate
                .point
                .elapsed_micros
                .saturating_sub(latest.elapsed_micros),
        ) {
            estimate.point = clamp_prediction(latest, estimate.point, transform, distance);
        }
        let delta = Point {
            x: estimate.point.position.x - latest.position.x,
            y: estimate.point.position.y - latest.position.y,
        };
        let surface = transform_vector(transform, delta);
        let distance = surface.x.hypot(surface.y);
        // Retreat immediately for loss of motion, corners and impending lift.
        // Smooth extension only. A shorter forecast or braking bound always
        // wins immediately; old preview state must never lengthen it.
        let unsafe_motion = motion_confidence(real, transform, config) < 0.5;
        let mut lead = distance;
        if let Some((time, old, direction)) = self.lead {
            let agreement = direction.x * surface.x + direction.y * surface.y;
            if distance > old
                && (old == 0.0 || agreement > 0.0)
                && !unsafe_motion
                && limited == requested
            {
                let dt = requested.saturating_sub(time).min(32_000) as f32;
                let tau = 32_000.0;
                let filtered = old + (distance - old) * (1.0 - (-dt / tau).exp());
                lead = filtered
                    .min(distance)
                    .min(old + dt * 0.0008)
                    .min(config.max_prediction_distance_px);
            }
        }
        if unsafe_motion && estimate.source == TipSource::Platform {
            // A native predictor may still supply a long tail after a real
            // stop/reversal. Only real samples decide this safety veto.
            if real.len() >= 3 {
                lead = 0.0;
            }
        }
        if distance > 0.0 {
            let scale = lead / distance;
            estimate.point.position = Point {
                x: latest.position.x + delta.x * scale,
                y: latest.position.y + delta.y * scale,
            };
        }
        if lead == 0.0 {
            estimate = TipEstimate {
                point: latest,
                source: TipSource::Real,
            };
        }
        self.lead = Some((requested, lead, surface));
        Some(estimate)
    }

    /// Intermediate engine samples follow the same model and accepted horizon
    /// as the endpoint. Evaluating a shorter time preserves the curve geometry.
    pub fn engine_intermediates(&self) -> impl Iterator<Item = StrokePoint> + '_ {
        self.output.iter().flat_map(|output| {
            (1..=output.horizon.saturating_sub(1) / motion_fit::STEP_MICROS)
                .map(|i| output.point_at(i * motion_fit::STEP_MICROS))
        })
    }

    /// Apply the same endpoint correction and safety radius to every native
    /// sample. Otherwise an intermediate point could leave a long loop even
    /// after the terminal estimate was shortened.
    pub fn platform_point(
        anchor: StrokePoint,
        point: StrokePoint,
        raw_tip: StrokePoint,
        tip: StrokePoint,
        transform: [f32; 6],
        maximum: f32,
    ) -> StrokePoint {
        let raw_distance = surface_distance(anchor.position, raw_tip.position, transform);
        let lead = surface_distance(anchor.position, tip.position, transform);
        let scale = if raw_distance > f32::EPSILON {
            lead / raw_distance
        } else {
            0.0
        };
        let point = StrokePoint {
            position: Point {
                x: anchor.position.x + (point.position.x - anchor.position.x) * scale,
                y: anchor.position.y + (point.position.y - anchor.position.y) * scale,
            },
            ..point
        };
        clamp_prediction(anchor, point, transform, maximum.min(lead))
    }
}

pub(crate) fn finalized_count(
    real: &[StrokePoint],
    already_finalized: usize,
    lag_micros: u32,
) -> usize {
    let Some(latest) = real.last() else {
        return 0;
    };
    let cutoff = latest.elapsed_micros.saturating_sub(lag_micros);
    let stable = real.partition_point(|point| point.elapsed_micros <= cutoff);
    stable.max(1).max(already_finalized).min(real.len())
}

pub(crate) fn estimate_tip(
    real: &[StrokePoint],
    platform: &[StrokePoint],
    requested_elapsed_micros: u32,
    document_to_surface: [f32; 6],
    config: InstantFeedbackConfig,
) -> Option<TipEstimate> {
    let latest = *real.last()?;
    let target_time = requested_elapsed_micros.min(
        latest
            .elapsed_micros
            .saturating_add(config.prediction_horizon_micros),
    );

    if config.use_platform_prediction
        && let Some(first) = platform
            .iter()
            .position(|point| point.elapsed_micros > latest.elapsed_micros)
    {
        let target_time = requested_elapsed_micros.min(
            latest
                .elapsed_micros
                .saturating_add(MAX_PREDICTION_HORIZON_MICROS),
        );
        let point = clamp_prediction(
            latest,
            sample_at_time(latest, &platform[first..], target_time),
            document_to_surface,
            config.max_prediction_distance_px,
        );
        return Some(TipEstimate {
            point,
            source: TipSource::Platform,
        });
    }

    if config.use_engine_prediction && target_time > latest.elapsed_micros {
        return PredictionState::default().estimate_for(
            real,
            &[],
            target_time,
            latest.elapsed_micros,
            document_to_surface,
            config,
        );
    }

    Some(TipEstimate {
        point: latest,
        source: TipSource::Real,
    })
}

fn sample_at_time(anchor: StrokePoint, predicted: &[StrokePoint], target: u32) -> StrokePoint {
    let mut previous = anchor;
    for current in predicted.iter().copied() {
        if current.elapsed_micros >= target {
            let span = current
                .elapsed_micros
                .saturating_sub(previous.elapsed_micros);
            let fraction = if span == 0 {
                1.0
            } else {
                target.saturating_sub(previous.elapsed_micros) as f32 / span as f32
            };
            return interpolate(previous, current, fraction.clamp(0.0, 1.0), target);
        }
        previous = current;
    }
    previous
}

fn motion_confidence(
    real: &[StrokePoint],
    document_to_surface: [f32; 6],
    config: InstantFeedbackConfig,
) -> f32 {
    let Some(&current) = real.last() else {
        return 0.0;
    };
    let Some(previous_index) = (0..real.len().saturating_sub(1))
        .rev()
        .find(|&i| real[i].elapsed_micros < current.elapsed_micros)
    else {
        return 1.0;
    };
    let previous = real[previous_index];
    let elapsed = (current.elapsed_micros - previous.elapsed_micros) as f32;
    let surface_velocity = transform_vector(
        document_to_surface,
        Point {
            x: (current.position.x - previous.position.x) / elapsed,
            y: (current.position.y - previous.position.y) / elapsed,
        },
    );
    if surface_velocity.x.hypot(surface_velocity.y) * 1_000_000.0
        < config.minimum_prediction_speed_px_per_second
    {
        return 0.0;
    }
    let mut confidence = 1.0;
    if previous_index > 0 {
        let older = real[previous_index - 1];
        let older_elapsed = previous.elapsed_micros.saturating_sub(older.elapsed_micros) as f32;
        if older_elapsed > 0.0 {
            let prior_velocity = Point {
                x: (previous.position.x - older.position.x) / older_elapsed,
                y: (previous.position.y - older.position.y) / older_elapsed,
            };
            let prior_surface = transform_vector(document_to_surface, prior_velocity);
            let prior_speed = prior_surface.x.hypot(prior_surface.y);
            let current_speed = surface_velocity.x.hypot(surface_velocity.y);
            if prior_speed > f32::EPSILON && current_speed > f32::EPSILON {
                let direction_agreement = ((prior_surface.x * surface_velocity.x
                    + prior_surface.y * surface_velocity.y)
                    / (prior_speed * current_speed))
                    .clamp(0.0, 1.0);
                confidence *= 1.0 - config.corner_suppression * (1.0 - direction_agreement);
                confidence *= (current_speed / prior_speed).clamp(0.0, 1.0);
            }
        }
    }
    confidence
}

fn clamp_prediction(
    anchor: StrokePoint,
    mut prediction: StrokePoint,
    document_to_surface: [f32; 6],
    maximum_distance_px: f32,
) -> StrokePoint {
    let mut delta = Point {
        x: prediction.position.x - anchor.position.x,
        y: prediction.position.y - anchor.position.y,
    };
    let surface_delta = transform_vector(document_to_surface, delta);
    let surface_distance = surface_delta.x.hypot(surface_delta.y);
    if surface_distance > maximum_distance_px && surface_distance > 0.0 {
        let scale = maximum_distance_px / surface_distance;
        delta.x *= scale;
        delta.y *= scale;
        prediction.position = Point {
            x: anchor.position.x + delta.x,
            y: anchor.position.y + delta.y,
        };
    }
    prediction
}

fn interpolate(a: StrokePoint, b: StrokePoint, t: f32, elapsed_micros: u32) -> StrokePoint {
    let mix = |a: f32, b: f32| a + (b - a) * t;
    StrokePoint {
        position: Point {
            x: mix(a.position.x, b.position.x),
            y: mix(a.position.y, b.position.y),
        },
        pressure: mix(a.pressure, b.pressure),
        tilt: [mix(a.tilt[0], b.tilt[0]), mix(a.tilt[1], b.tilt[1])],
        twist: mix_angle(a.twist, b.twist, t),
        elapsed_micros,
    }
}

fn mix_angle(a: f32, b: f32, t: f32) -> f32 {
    let period = std::f32::consts::TAU;
    let delta = (b - a + period * 0.5).rem_euclid(period) - period * 0.5;
    (a + delta * t).rem_euclid(period)
}

pub(crate) fn transform_vector(transform: [f32; 6], vector: Point) -> Point {
    Point {
        x: transform[0].mul_add(vector.x, transform[2] * vector.y),
        y: transform[1].mul_add(vector.x, transform[3] * vector.y),
    }
}

pub(crate) fn surface_distance(a: Point, b: Point, transform: [f32; 6]) -> f32 {
    let delta = transform_vector(
        transform,
        Point {
            x: a.x - b.x,
            y: a.y - b.y,
        },
    );
    delta.x.hypot(delta.y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::PenPhase;

    const IDENTITY: [f32; 6] = [1., 0., 0., 1., 0., 0.];

    fn pressure_sample(index: u64, pressure: f32) -> PenEvent {
        PenEvent {
            device_id: 1,
            sequence: index,
            timestamp_ns: index * 4_000_000,
            view_revision: 1,
            surface_position: Point {
                x: index as f32 * 4.,
                y: 0.,
            },
            pressure,
            tilt_radians: [0.; 2],
            twist_radians: 0.,
            distance: 0.,
            phase: PenPhase::Move,
            tool: ToolKind::Pen,
            flags: SampleFlags::PRIMARY,
        }
    }

    #[test]
    fn native_lead_filter_reduces_jitter_and_bounds_extension_at_different_frame_rates_and_zoom() {
        let config = InstantFeedbackConfig::default();
        for zoom in [0.25, 1., 4.] {
            for step in [4_000, 8_000, 16_000] {
                let transform = [zoom, 0., 0., zoom, 0., 0.];
                let mut real = vec![point(0., 0., 0)];
                let mut state = PredictionState::default();
                let mut raw_leads = Vec::new();
                let mut filtered_leads = Vec::new();
                for i in 1..40 {
                    let time = i * step;
                    // Constant physical motion with alternating native horizon
                    // noise. Engine fallback has its own trajectory clock.
                    real.push(point(time as f32 * 0.001 / zoom, 0., time));
                    let latest = *real.last().unwrap();
                    let platform = [point(
                        latest.position.x + if i % 2 == 0 { 16. / zoom } else { 8. / zoom },
                        0.,
                        time + 8_000,
                    )];
                    let platform = &platform[..];
                    let raw =
                        estimate_tip(&real, platform, time + 8_000, transform, config).unwrap();
                    let tip = state
                        .estimate(&real, platform, time + 8_000, transform, config)
                        .unwrap();
                    let lead = surface_distance(latest.position, tip.point.position, transform);
                    if let Some(&old) = filtered_leads.last() {
                        assert!(lead <= old + step as f32 * 0.0008 + 0.001);
                    }
                    raw_leads.push(surface_distance(
                        latest.position,
                        raw.point.position,
                        transform,
                    ));
                    filtered_leads.push(lead);
                    assert_eq!(
                        state
                            .estimate(&real, platform, time + 8_000, transform, config)
                            .unwrap(),
                        tip,
                        "same presentation cannot advance the filter"
                    );
                }
                let variation =
                    |values: &[f32]| values.windows(2).map(|p| (p[1] - p[0]).abs()).sum::<f32>();
                assert!(variation(&filtered_leads) < variation(&raw_leads) * 0.5);
                assert!(
                    filtered_leads.last().unwrap() > &6.,
                    "keep useful lookahead"
                );
            }
        }
    }

    #[test]
    fn stop_reversal_and_pressure_release_retract_without_filter_lag() {
        for last in [
            point(8., 0., 12_000),
            point(6., 0., 12_000),
            point(8., 4., 12_000),
        ] {
            let mut state = PredictionState::default();
            let mut real = vec![point(0., 0., 0), point(4., 0., 4_000), point(8., 0., 8_000)];
            state
                .estimate(
                    &real,
                    &[point(40., 0., 16_000)],
                    16_000,
                    IDENTITY,
                    InstantFeedbackConfig::default(),
                )
                .unwrap();
            // A stop, reversal or right-angle corner must also veto stale OS
            // predictions, including a stationary latest sample.
            real.push(last);
            let tip = state
                .estimate(
                    &real,
                    &[point(60., 0., 20_000)],
                    20_000,
                    IDENTITY,
                    InstantFeedbackConfig::default(),
                )
                .unwrap();
            assert_eq!(tip.point, last);
            assert_eq!(tip.source, TipSource::Real);
        }
        let mut state = PredictionState::default();
        let real = [
            point(0., 0., 0),
            point(4., 0., 4_000),
            point(8., 0., 8_000),
            point(12., 0., 12_000),
        ];
        state
            .estimate(
                &real[..3],
                &[],
                16_000,
                IDENTITY,
                InstantFeedbackConfig::default(),
            )
            .unwrap();
        for (i, p) in [0.8, 0.6, 0.4, 0.2].into_iter().enumerate() {
            state.observe(pressure_sample(i as u64, p));
        }
        for platform in [&[point(28., 0., 28_000)][..]] {
            let tip = state
                .estimate(
                    &real,
                    platform,
                    20_000,
                    IDENTITY,
                    InstantFeedbackConfig::default(),
                )
                .unwrap();
            assert!(
                tip.point.position.x <= 14.01,
                "release limits even a previously extended tail"
            );
        }
    }

    #[test]
    fn pressure_gate_ignores_light_steady_noisy_and_non_pen_input() {
        for values in [
            [0.08; 5],
            [0.6, 0.59, 0.61, 0.6, 0.59],
            [0.8, 0.6, 0.4, 0.2, 0.3],
        ] {
            let mut state = PredictionState::default();
            for (i, p) in values.into_iter().enumerate() {
                state.observe(pressure_sample(i as u64, p));
            }
            assert_eq!(state.lift_horizon(), None);
        }
        for (tool, flags) in [
            (ToolKind::Mouse, SampleFlags::NONE),
            (ToolKind::Finger, SampleFlags::NONE),
            (ToolKind::Pen, SampleFlags::PREDICTED),
            (ToolKind::Pen, SampleFlags::CORRECTION),
            (ToolKind::Pen, SampleFlags::ESTIMATED),
        ] {
            let mut state = PredictionState::default();
            for (i, p) in [0.8, 0.6, 0.4, 0.2].into_iter().enumerate() {
                state.observe(PenEvent {
                    tool,
                    flags,
                    ..pressure_sample(i as u64, p)
                });
            }
            assert_eq!(state.lift_horizon(), None);
        }
    }

    #[test]
    fn native_horizon_is_independent_of_manual_time_and_entire_tail_is_bounded() {
        let real = [point(0., 0., 0), point(4., 0., 4_000)];
        let platform = [point(400., 200., 6_000), point(20., 0., 20_000)];
        let config = InstantFeedbackConfig {
            prediction_horizon_micros: 0,
            ..InstantFeedbackConfig::default()
        };
        let raw = estimate_tip(&real, &platform, 20_000, IDENTITY, config).unwrap();
        assert_eq!(raw.point, platform[1]);
        let tip = point(8., 0., 20_000);
        let intermediate =
            PredictionState::platform_point(real[1], platform[0], raw.point, tip, IDENTITY, 96.);
        assert!(surface_distance(real[1].position, intermediate.position, IDENTITY) <= 4.001);
    }

    fn point(x: f32, y: f32, elapsed_micros: u32) -> StrokePoint {
        StrokePoint {
            position: Point { x, y },
            pressure: 0.5,
            tilt: [0.0; 2],
            twist: 0.0,
            elapsed_micros,
        }
    }

    #[test]
    fn finalization_is_timestamp_based() {
        let points = [
            point(0.0, 0.0, 0),
            point(4.0, 0.0, 4_000),
            point(8.0, 0.0, 8_000),
        ];
        assert_eq!(finalized_count(&points, 0, 4_000), 2);
        assert_eq!(finalized_count(&points, 2, 8_000), 2);
    }

    #[test]
    fn platform_prediction_is_interpolated_to_presentation_time() {
        let real = [point(0.0, 0.0, 0), point(10.0, 0.0, 10_000)];
        let predicted = [point(14.0, 0.0, 14_000), point(18.0, 0.0, 18_000)];
        let estimate = estimate_tip(
            &real,
            &predicted,
            16_000,
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            InstantFeedbackConfig::default(),
        )
        .unwrap();
        assert_eq!(estimate.source, TipSource::Platform);
        assert!((estimate.point.position.x - 16.0).abs() < 0.001);
    }

    #[test]
    fn disabling_platform_prediction_uses_engine_even_when_platform_samples_exist() {
        let real = [
            point(-20., 0., 0),
            point(-10., 0., 10_000),
            point(0., 0., 20_000),
            point(10., 0., 30_000),
        ];
        let predicted = [point(14.0, 8.0, 34_000), point(18.0, 16.0, 38_000)];
        for enabled in [true, false] {
            let estimate = estimate_tip(
                &real,
                &predicted,
                38_000,
                [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                InstantFeedbackConfig {
                    use_platform_prediction: enabled,
                    ..InstantFeedbackConfig::default()
                },
            )
            .unwrap();
            assert_eq!(
                estimate.source,
                if enabled {
                    TipSource::Platform
                } else {
                    TipSource::Engine
                }
            );
            assert_eq!(
                estimate.point.position,
                Point {
                    // The default adaptive model may shorten a forecast with
                    // only 30 ms of history. It must still follow the real
                    // line at its honest output time, not the native bend.
                    x: if enabled {
                        18.0
                    } else {
                        10. + (estimate.point.elapsed_micros - 30_000) as f32 * 0.001
                    },
                    y: if enabled { 16.0 } else { 0.0 }
                }
            );
        }
    }

    #[test]
    fn engine_prediction_stops_at_a_reversal() {
        let real = [
            point(0.0, 0.0, 0),
            point(10.0, 0.0, 10_000),
            point(4.0, 0.0, 20_000),
        ];
        let estimate = estimate_tip(
            &real,
            &[],
            28_000,
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            InstantFeedbackConfig::default(),
        )
        .unwrap();
        assert_eq!(estimate.source, TipSource::Real);
        assert_eq!(estimate.point.position, real[2].position);
    }

    #[test]
    fn engine_prediction_clamps_in_surface_pixels() {
        let real = [
            point(70., 0., 0),
            point(80., 0., 1_000),
            point(90., 0., 2_000),
            point(100., 0., 3_000),
        ];
        let config = InstantFeedbackConfig {
            max_prediction_distance_px: 12.0,
            ..InstantFeedbackConfig::default()
        };
        let estimate =
            estimate_tip(&real, &[], 9_000, [2.0, 0.0, 0.0, 2.0, 0.0, 0.0], config).unwrap();
        assert_eq!(estimate.source, TipSource::Engine);
        assert!((estimate.point.position.x - 106.0).abs() < 0.001);
    }
}
