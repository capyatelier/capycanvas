//! Platform-neutral policy for the replaceable tip of an active stroke.

use layer_core::{Point, StrokePoint};

const MAX_FINALIZATION_LAG_MICROS: u32 = 50_000;
const MAX_PREDICTION_HORIZON_MICROS: u32 = 50_000;
const MAX_PREDICTION_DISTANCE_PX: f32 = 512.0;

/// Runtime-tunable instant-feedback policy. This is interaction state, not part
/// of a brush preset or persisted stroke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InstantFeedbackConfig {
    pub enabled: bool,
    pub use_platform_prediction: bool,
    pub use_engine_prediction: bool,
    /// Real input newer than this remains in the replaceable tail.
    pub finalization_lag_micros: u32,
    /// Prediction used when a frontend cannot provide an exact presentation
    /// timestamp, and the maximum lookahead accepted from any predictor.
    pub prediction_horizon_micros: u32,
    /// Clamp in physical surface pixels, independent of document zoom.
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

    if config.use_engine_prediction
        && target_time > latest.elapsed_micros
        && let Some(point) = extrapolate(real, target_time, document_to_surface, config)
    {
        return Some(TipEstimate {
            point,
            source: TipSource::Engine,
        });
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

fn extrapolate(
    real: &[StrokePoint],
    target_time: u32,
    document_to_surface: [f32; 6],
    config: InstantFeedbackConfig,
) -> Option<StrokePoint> {
    let current = *real.last()?;
    let previous_index = (0..real.len().saturating_sub(1)).rev().find(|index| {
        real[*index].elapsed_micros < current.elapsed_micros
            && distance(real[*index].position, current.position) > f32::EPSILON
    })?;
    let previous = real[previous_index];
    let elapsed = current
        .elapsed_micros
        .saturating_sub(previous.elapsed_micros) as f32;
    if elapsed <= 0.0 {
        return None;
    }
    let velocity = Point {
        x: (current.position.x - previous.position.x) / elapsed,
        y: (current.position.y - previous.position.y) / elapsed,
    };
    let surface_velocity = transform_vector(document_to_surface, velocity);
    let speed = surface_velocity.x.hypot(surface_velocity.y) * 1_000_000.0;
    if speed < config.minimum_prediction_speed_px_per_second {
        return None;
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
    if confidence <= f32::EPSILON {
        return None;
    }

    let future = target_time.saturating_sub(current.elapsed_micros) as f32;
    let mut delta = Point {
        x: velocity.x * future * confidence,
        y: velocity.y * future * confidence,
    };
    let surface_delta = transform_vector(document_to_surface, delta);
    let surface_distance = surface_delta.x.hypot(surface_delta.y);
    if surface_distance > config.max_prediction_distance_px && surface_distance > 0.0 {
        let scale = config.max_prediction_distance_px / surface_distance;
        delta.x *= scale;
        delta.y *= scale;
    }
    Some(StrokePoint {
        position: Point {
            x: current.position.x + delta.x,
            y: current.position.y + delta.y,
        },
        elapsed_micros: target_time,
        ..current
    })
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

fn distance(a: Point, b: Point) -> f32 {
    (a.x - b.x).hypot(a.y - b.y)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let real = [point(0.0, 0.0, 0), point(100.0, 0.0, 1_000)];
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
