use crate::{InstantFeedbackConfig, SampleFlags, ToolKind};
use layer_core::{Point, StrokePoint};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    #[serde(with = "config_wire")]
    pub config: InstantFeedbackConfig,
    /// Document coordinates to physical surface pixels, including translation.
    pub transform: [f32; 6],
}

/// `[elapsed_us, x, y, pressure, tilt_x, tilt_y, twist]`; angles are radians.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Sample(
    pub u32,
    pub f32,
    pub f32,
    pub f32,
    pub f32,
    pub f32,
    pub f32,
);
impl From<Sample> for StrokePoint {
    fn from(p: Sample) -> Self {
        Self {
            elapsed_micros: p.0,
            position: Point { x: p.1, y: p.2 },
            pressure: p.3,
            tilt: [p.4, p.5],
            twist: p.6,
        }
    }
}

/// Replay order is delivery order, not timestamp order. Actuals are never
/// supplied before their event, even when used later as the scoring reference.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    Sample(Sample),
    Predicted(Sample),
    Stationary(Sample),
    Replace(usize, Sample),
    /// `[contact_relative_ns, raw_pressure, tool, flags]`; raw pressure precedes
    /// the brush pressure curve. Only these fields affect pressure observation.
    Observe(u64, f32, ToolKind, SampleFlags),
    Reset,
    Policy(Policy),
    /// `[query_id, frame_elapsed_us, requested_elapsed_us]`.
    Query(u64, u32, u32),
}

impl From<StrokePoint> for Sample {
    fn from(p: StrokePoint) -> Self {
        Self(
            p.elapsed_micros,
            p.position.x,
            p.position.y,
            p.pressure,
            p.tilt[0],
            p.tilt[1],
            p.twist,
        )
    }
}

// Version 2 recordings reserve an enum discriminant after the three switches.
// Decode that old slot only here. Runtime config and settings have one predictor.
mod config_wire {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        value: &InstantFeedbackConfig,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            return value.serialize(serializer);
        }
        (
            value.enabled,
            value.use_platform_prediction,
            value.use_engine_prediction,
            1u32,
            value.timestamp_resolution_micros,
            value.finalization_lag_micros,
            value.prediction_horizon_micros,
            value.max_prediction_distance_px,
            value.tip_lock,
            value.correction_easing,
            value.minimum_prediction_speed_px_per_second,
            value.corner_suppression,
        )
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<InstantFeedbackConfig, D::Error> {
        if deserializer.is_human_readable() {
            return InstantFeedbackConfig::deserialize(deserializer);
        }
        let (
            enabled,
            use_platform_prediction,
            use_engine_prediction,
            retired,
            timestamp_resolution_micros,
            finalization_lag_micros,
            prediction_horizon_micros,
            max_prediction_distance_px,
            tip_lock,
            correction_easing,
            minimum_prediction_speed_px_per_second,
            corner_suppression,
        ): (
            bool,
            bool,
            bool,
            u32,
            u32,
            u32,
            u32,
            f32,
            f32,
            f32,
            f32,
            f32,
        ) = Deserialize::deserialize(deserializer)?;
        if retired > 2 {
            return Err(serde::de::Error::custom("invalid version 2 predictor slot"));
        }
        Ok(InstantFeedbackConfig {
            enabled,
            use_platform_prediction,
            use_engine_prediction,
            timestamp_resolution_micros,
            finalization_lag_micros,
            prediction_horizon_micros,
            max_prediction_distance_px,
            tip_lock,
            correction_easing,
            minimum_prediction_speed_px_per_second,
            corner_suppression,
        })
    }
}
