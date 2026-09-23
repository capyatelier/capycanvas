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
// Keep the tuple layout; retired values 0..=2 use today's default. New values
// identify the two Smooth Motion revisions without reusing historical meanings.
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
            match value.prediction_algorithm {
                PredictionAlgorithm::Optimized => 3u32,
                PredictionAlgorithm::Previous => 4u32,
            },
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
            let mut value = serde_json::Value::deserialize(deserializer)?;
            // Legacy JSON captures named experimental predictors that have
            // since been retired; retain the same default migration as v2.
            if let Some(fields) = value.as_object_mut()
                && fields
                    .get("prediction_algorithm")
                    .is_some_and(|v| !matches!(v.as_str(), Some("optimized" | "previous")))
            {
                fields.remove("prediction_algorithm");
            }
            return serde_json::from_value(value).map_err(serde::de::Error::custom);
        }
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
            0..=3 => PredictionAlgorithm::Optimized,
            4 => PredictionAlgorithm::Previous,
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
    fn predictor_slot_preserves_new_choices_and_decodes_legacy_captures() {
        let config = InstantFeedbackConfig::default();
        let transform = [1., 0., 0., 1., 0., 0.];
        for slot in 0u32..=5 {
            // Freeze the original v2 tuple, independent of the runtime struct.
            let wire = (
                (
                    true, true, true, slot, 1u32, 8_000u32, 8_000u32, 96f32, 1f32, 1.5f32, 12f32,
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
            assert_eq!(
                policy.config,
                InstantFeedbackConfig {
                    prediction_algorithm: if slot == 4 {
                        PredictionAlgorithm::Previous
                    } else {
                        PredictionAlgorithm::Optimized
                    },
                    ..config
                }
            );
            if slot >= 3 {
                assert_eq!(
                    bincode::serde::encode_to_vec(policy, bincode::config::standard()).unwrap(),
                    bytes
                );
            }
            let json = serde_json::to_value(policy).unwrap();
            assert_eq!(
                serde_json::from_value::<Policy>(json.clone())
                    .unwrap()
                    .config,
                policy.config
            );
            let mut legacy = json;
            legacy["config"]
                .as_object_mut()
                .unwrap()
                .remove("prediction_algorithm");
            assert_eq!(
                serde_json::from_value::<Policy>(legacy).unwrap().config,
                config
            );
        }
        let mut legacy = serde_json::to_value(Policy { config, transform }).unwrap();
        legacy["config"]["prediction_algorithm"] = "trajectory".into();
        assert_eq!(
            serde_json::from_value::<Policy>(legacy).unwrap().config,
            config
        );
    }
}
