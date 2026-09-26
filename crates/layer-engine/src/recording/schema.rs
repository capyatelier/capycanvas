use crate::{InstantFeedbackConfig, PredictionAlgorithm, SampleFlags, ToolKind};
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
// Keep the tuple layout and Optimized slot (3). Retired values, including
// Previous (4), migrate to the supported predictor without retaining old code.
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
            3u32,
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
        let (
            enabled,
            use_platform_prediction,
            use_engine_prediction,
            algorithm,
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
        let prediction_algorithm = match algorithm {
            0..=4 => PredictionAlgorithm::Optimized,
            _ => return Err(serde::de::Error::custom("invalid version 2 predictor slot")),
        };
        Ok(InstantFeedbackConfig {
            enabled,
            use_platform_prediction,
            use_engine_prediction,
            prediction_algorithm,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predictor_slot_preserves_optimized_and_migrates_retired_captures() {
        let config = InstantFeedbackConfig {
            prediction_horizon_micros: 23_000,
            timestamp_resolution_micros: 1,
            use_platform_prediction: false,
            ..Default::default()
        };
        let transform = [1., 0., 0., 1., 0., 0.];
        for slot in 0u32..=5 {
            // Freeze the original v2 tuple, independent of the runtime struct.
            let wire = (
                (
                    true, false, true, slot, 1u32, 8_000u32, 23_000u32, 96f32, 1f32, 1.5f32, 12f32,
                    1f32,
                ),
                transform,
            );
            let bytes = bincode::serde::encode_to_vec(wire, bincode::config::standard()).unwrap();
            let result =
                bincode::serde::decode_from_slice::<Policy, _>(&bytes, bincode::config::standard());
            if slot == 5 {
                assert!(result.is_err());
                continue;
            }
            let (policy, used) = result.unwrap();
            assert_eq!(used, bytes.len());
            assert_eq!(policy.transform, transform);
            assert_eq!(policy.config, config);
            if slot == 3 {
                assert_eq!(
                    bincode::serde::encode_to_vec(policy, bincode::config::standard()).unwrap(),
                    bytes
                );
            }
            let current = Policy { config, transform };
            assert_eq!(
                bincode::serde::encode_to_vec(policy, bincode::config::standard()).unwrap(),
                bincode::serde::encode_to_vec(current, bincode::config::standard()).unwrap()
            );
        }
    }
}
