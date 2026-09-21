//! Put lookahead on the display clock, with a bounded input-age reserve.

use super::trajectory::Trajectory;
use super::{InstantFeedbackConfig, MAX_PREDICTION_DISTANCE_PX, MAX_PREDICTION_HORIZON_MICROS};

#[derive(Clone, Debug, Default)]
pub(super) struct PredictionClock {
    previous: Option<(u32, u32)>,
    confidence: Option<(u32, f64)>,
}

impl PredictionClock {
    pub fn clear(&mut self) {
        self.previous = None;
        self.confidence = None;
    }

    pub fn horizon(
        &mut self,
        motion: &Trajectory,
        requested: u32,
        now: u32,
        maximum_age: u32,
        config: InstantFeedbackConfig,
    ) -> u32 {
        let sample = motion.point_at(0).elapsed_micros;
        let age = now.saturating_sub(sample);
        let reserve = self.previous.map_or(age, |(before, reserve)| {
            if sample >= before && sample - before <= maximum_age {
                reserve.max(age)
            } else {
                age
            }
        });
        self.previous = Some((sample, reserve));
        // Timing calibration survives motion changes. Its purpose is to keep
        // the target attainable throughout the input/display phase cycle.
        // Treat confidence as a conservative envelope: contract immediately,
        // recover over one requested lookahead interval as fresh reports arrive.
        // A single confident fit must not flash the entire preview back on.
        // The envelope never exceeds CURRENT statistical confidence; physical
        // stopping and distance limits are applied afterwards without delay.
        let confidence = f64::from(motion.confidence_horizon(MAX_PREDICTION_HORIZON_MICROS));
        let confidence = self.confidence.map_or(confidence, |(before, old)| {
            if sample >= before && sample - before <= maximum_age {
                confidence.min(
                    old + (confidence - old)
                        * (1.
                            - (-f64::from(sample - before)
                                / f64::from(config.prediction_horizon_micros.max(1)))
                            .exp()),
                )
            } else {
                confidence
            }
        });
        let budget = motion.motion_horizon(
            confidence as u32,
            InstantFeedbackConfig {
                max_prediction_distance_px: MAX_PREDICTION_DISTANCE_PX,
                ..config
            },
        );
        // Remember the usable budget after physical limiting (anti-windup).
        // A resolved stop can have tiny position variance, hence a nominally
        // long statistical horizon. Remembering that unused horizon lets the
        // full tail flash back on as soon as braking releases. Keep recovery
        // driven by current statistical confidence, but start at the budget
        // that was actually attainable. Repaints still cannot advance it.
        self.confidence = Some((sample, f64::from(budget)));
        let lead = i64::from(budget) - i64::from(reserve);
        let target = (i64::from(age) + lead.min(i64::from(requested))).max(0) as u32;
        motion.motion_horizon(
            target.min(MAX_PREDICTION_HORIZON_MICROS),
            InstantFeedbackConfig {
                max_prediction_distance_px: motion.display_distance_budget(age, config),
                ..config
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::{Point, StrokePoint};

    fn motion(end: u32, braking: bool) -> Trajectory {
        let samples: Vec<_> = (0..=24)
            .map(|i| {
                let t = i as f64 * 0.004;
                StrokePoint {
                    position: Point {
                        x: if braking {
                            (2000. * t - 9000. * t * t) as f32
                        } else {
                            (500. * t) as f32
                        },
                        y: 0.,
                    },
                    elapsed_micros: end - 96_000 + i * 4000,
                    pressure: 0.6,
                    tilt: [0.; 2],
                    twist: 0.,
                }
            })
            .collect();
        Trajectory::fit(&samples, [1., 0., 0., 1., 0., 0.], 0).unwrap()
    }

    #[test]
    fn releasing_braking_recovers_lookahead_without_releasing_unused_confidence() {
        let cfg = InstantFeedbackConfig {
            prediction_horizon_micros: 32_000,
            ..Default::default()
        };
        let mut clock = PredictionClock::default();
        let brake = motion(100_000, true);
        let stopped = clock.horizon(&brake, 32_000, 100_000, 32_000, cfg);
        assert!((14_000..16_000).contains(&stopped));
        // A new, confident fit should regain useful lead progressively. This
        // exercises the clock contract independently of how a fit was selected.
        let steady = motion(108_000, false);
        let resumed = clock.horizon(&steady, 32_000, 108_000, 32_000, cfg);
        assert!((20_000..27_000).contains(&resumed), "restart {resumed}");
        assert_eq!(
            resumed,
            clock.horizon(&steady, 32_000, 108_000, 32_000, cfg)
        );
        for end in [116_000, 124_000, 132_000, 140_000] {
            let steady = motion(end, false);
            clock.horizon(&steady, 32_000, end, 32_000, cfg);
        }
        assert_eq!(
            clock.horizon(&motion(148_000, false), 32_000, 148_000, 32_000, cfg),
            32_000
        );
        clock.clear();
        assert_eq!(clock.horizon(&steady, 32_000, 108_000, 32_000, cfg), 32_000);
    }
}
