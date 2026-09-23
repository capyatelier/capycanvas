//! Join the disposable forecast to measured ink without copying input noise to its tip.

use super::{
    InstantFeedbackConfig, local_motion::LocalMotion, motion_fit::MotionFit, surface_distance,
};
use layer_core::{Point, StrokePoint};

#[derive(Clone, Debug)]
pub(super) struct Output {
    motion: MotionFit,
    pub horizon: u32,
    anchor: StrokePoint,
    transform: [f32; 6],
    maximum_distance: f32,
    join_horizon: Option<u32>,
    local: Option<LocalMotion>,
    corrections: Option<CorrectionField>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feedback::test_support::{IDENTITY, point};

    fn line(now: u32, y: f32, transform: [f32; 6]) -> Output {
        let real: Vec<_> = (0..=now / 4_000)
            .map(|i| point(i as f32 * 9.6, y, i * 4_000))
            .collect();
        Output::new(
            MotionFit::fit(&real, transform, 1).unwrap(),
            24_000,
            transform,
            96.,
        )
    }

    #[test]
    fn latent_forecast_memory_does_not_retain_display_distance_clipping() {
        let mut field = None;
        for now in (200_000..400_000).step_by(8_000) {
            let raw = line(now, 0., IDENTITY);
            let expected = raw.point_at(24_000);
            let (filtered, next) = raw.smooth_corrections(field.as_ref(), now, 1.);
            assert!(
                surface_distance(
                    filtered.point_at(24_000).position,
                    expected.position,
                    IDENTITY
                ) < 0.002
            );
            assert_eq!(filtered.point_at(0).position, filtered.anchor.position);
            field = Some(next);
        }
    }

    #[test]
    fn correction_field_is_bounded_idempotent_and_preserves_the_measured_anchor() {
        let (_, prior) = line(200_000, 0., IDENTITY).smooth_corrections(None, 200_000, 1.);
        let raw = line(208_000, 40., IDENTITY);
        let (filtered, field) = raw.clone().smooth_corrections(Some(&prior), 208_000, 1.);
        let (repeated, _) = raw.clone().smooth_corrections(Some(&field), 208_000, 1.);
        assert_eq!(filtered.point_at(0), raw.point_at(0));
        for time in (0..=64_000).step_by(500) {
            let a = raw.point_at(time);
            let b = filtered.point_at(time);
            assert!(surface_distance(a.position, b.position, IDENTITY) <= 8.001);
            assert_eq!(b, repeated.point_at(time));
            assert_eq!(
                (a.pressure, a.tilt, a.twist, a.elapsed_micros),
                (b.pressure, b.tilt, b.twist, b.elapsed_micros)
            );
        }
        for (now, m) in [(248_000, IDENTITY), (208_000, [2., 0., 0., 2., 0., 0.])] {
            let raw = line(now, 40., m);
            let (filtered, _) = raw.clone().smooth_corrections(Some(&field), now, 1.);
            assert_eq!(raw.point_at(24_000), filtered.point_at(24_000));
        }
    }

    #[test]
    fn cropping_preview_length_preserves_the_remaining_curve() {
        let mut real: Vec<_> = (0..=50)
            .map(|i| point(i as f32 * 9.6, 0.3 * (i as f32).sin(), i * 4000))
            .collect();
        real.last_mut().unwrap().position.y += 0.5;
        let motion = MotionFit::fit(&real, IDENTITY, 1000).unwrap();
        for algorithm in [
            super::super::PredictionAlgorithm::Optimized,
            super::super::PredictionAlgorithm::Previous,
        ] {
            let config = InstantFeedbackConfig {
                prediction_algorithm: algorithm,
                prediction_horizon_micros: 24_000,
                ..Default::default()
            };
            let full = Output::new(motion.clone(), 24_000, IDENTITY, 96.)
                .with_local_motion(&real, config, 0, None, None, 0.);
            let mut cropped = full.clone();
            cropped.horizon = 4000;
            let a = full.point_at(4000);
            let b = cropped.point_at(4000);
            if algorithm == super::super::PredictionAlgorithm::Optimized {
                assert_eq!(a, b, "visibility cannot change the curve's anchor join");
            } else {
                assert_ne!(a, b, "the A/B reference preserves its original geometry");
            }
        }
    }
}

/// Fixed-size, disposable forecast in document coordinates. Its knots are
/// absolute future times: transporting it never adds a pen-motion delay.
#[derive(Clone, Debug)]
pub(super) struct CorrectionField {
    anchor: u32,
    now: u32,
    supported_until: u32,
    transform: [f32; 6],
    points: [Point; 33],
    unfiltered: [Point; 33],
    deltas: [Point; 33],
}

impl CorrectionField {
    fn at(&self, time: u32) -> Option<Point> {
        if time > self.supported_until {
            return None;
        }
        self.interpolate(time, &self.points)
    }

    fn interpolate(&self, time: u32, points: &[Point; 33]) -> Option<Point> {
        let offset = time.checked_sub(self.anchor)?;
        if offset > 64_000 {
            return None;
        }
        let i = (offset / 2_000).min(31) as usize;
        let t = (offset - i as u32 * 2_000) as f32 / 2_000.;
        let a = points[i];
        let b = points[i + 1];
        Some(Point {
            x: a.x + t * (b.x - a.x),
            y: a.y + t * (b.y - a.y),
        })
    }
}

impl Output {
    pub fn new(
        motion: MotionFit,
        horizon: u32,
        transform: [f32; 6],
        maximum_distance: f32,
    ) -> Self {
        Self {
            anchor: motion.point_at(0),
            motion,
            horizon,
            maximum_distance,
            join_horizon: None,
            transform,
            local: None,
            corrections: None,
        }
    }

    pub fn with_local_motion(
        mut self,
        real: &[StrokePoint],
        config: InstantFeedbackConfig,
        age: u32,
        memory: Option<(u32, f64)>,
        previous: Option<&LocalMotion>,
        continuity: f64,
    ) -> Self {
        self.local = LocalMotion::fit(
            real,
            self.transform,
            config.timestamp_resolution_micros,
            self.horizon,
            memory,
            previous,
            continuity,
        );
        if let Some(local) = &self.local {
            if config.prediction_algorithm == super::PredictionAlgorithm::Optimized {
                self.join_horizon = Some(local.join_horizon());
            }
            self.horizon = local.horizon(self.horizon, age, config.prediction_horizon_micros);

            // A stopping distance belongs to the model that estimated it.
            // Reusing the old model's radius with a new velocity can clip a
            // correctly predicted curve to an obsolete braking envelope.
            // Measure this model's age travel at its accepted time.
            self.maximum_distance = (config.max_prediction_distance_px
                + surface_distance(
                    self.anchor.position,
                    local.point_at(age).position,
                    self.transform,
                ))
            .min(super::MAX_PREDICTION_DISTANCE_PX);
        }
        self
    }

    pub fn smooth_corrections(
        mut self,
        previous: Option<&CorrectionField>,
        now: u32,
        confidence: f64,
    ) -> (Self, CorrectionField) {
        let previous = previous
            .filter(|p| p.transform == self.transform && now >= p.now && now - p.now <= 32_000);
        // Retain the latent motion, not its display-distance clipping. A clipped
        // far-future endpoint would otherwise propagate backwards through this
        // time-aligned memory and make even constant pen velocity lag.
        let unfiltered =
            std::array::from_fn(|i| self.unclamped_point_at(i as u32 * 2_000).position);
        if let Some(old) = previous
            && old.now == now
            && old.anchor == self.anchor.elapsed_micros
            && old.unfiltered == unfiltered
        {
            self.corrections = Some(old.clone());
            return (self, old.clone());
        }
        let weight = previous.map_or(0., |p| {
            confidence as f32 * (-f64::from(now - p.now) / 20_000.).exp() as f32
        });
        let points = std::array::from_fn(|i| {
            let time = i as u32 * 2_000;
            let mut p = unfiltered[i];
            if let Some(old) =
                previous.and_then(|prior| prior.at(self.anchor.elapsed_micros.saturating_add(time)))
            {
                let d = surface_distance(p, old, self.transform);
                let u = (time as f32 / 8_000.).min(1.);
                let gain = weight * u * u * (3. - 2. * u) * (8. / d.max(1e-6)).min(1.);
                p.x += gain * (old.x - p.x);
                p.y += gain * (old.y - p.y);
            }
            p
        });
        let deltas = std::array::from_fn(|i| Point {
            x: points[i].x - unfiltered[i].x,
            y: points[i].y - unfiltered[i].y,
        });
        let extension = previous.map_or(16_000, |old| {
            now.saturating_sub(old.now)
                .saturating_mul(2)
                .clamp(8_000, 32_000)
        });
        let supported_until = self
            .anchor
            .elapsed_micros
            .saturating_add(self.horizon.saturating_add(extension).min(64_000));
        let field = CorrectionField {
            anchor: self.anchor.elapsed_micros,
            now,
            supported_until,
            transform: self.transform,
            points,
            unfiltered,
            deltas,
        };
        self.corrections = Some(field.clone());
        (self, field)
    }

    pub fn point_at(&self, time: u32) -> StrokePoint {
        let mut point = self.unclamped_point_at(time);
        if let Some(field) = &self.corrections
            && let Some(p) = field.interpolate(point.elapsed_micros, &field.deltas)
        {
            point.position.x += p.x;
            point.position.y += p.y;
        }
        super::clamp_prediction(self.anchor, point, self.transform, self.maximum_distance)
    }

    fn unclamped_point_at(&self, time: u32) -> StrokePoint {
        if let Some(local) = &self.local {
            return local.output_at(time, self.join_horizon.unwrap_or(self.horizon));
        }
        let mut point = self.motion.point_at(time);
        let fitted_anchor = self.motion.fitted_point_at(0).position;
        let t = (time as f32 / self.horizon.max(1) as f32).clamp(0., 1.);
        let weight = t * t * (3. - 2. * t);
        let mut innovation = Point {
            x: (fitted_anchor.x - self.anchor.position.x) * weight,
            y: (fitted_anchor.y - self.anchor.position.y) * weight,
        };
        let length = surface_distance(self.anchor.position, point.position, self.transform);
        let correction = surface_distance(Point { x: 0., y: 0. }, innovation, self.transform);
        if correction > length {
            innovation.x *= length / correction;
            innovation.y *= length / correction;
        }
        point.position.x += innovation.x;
        point.position.y += innovation.y;
        point
    }

    pub fn sample_time(&self) -> u32 {
        self.anchor.elapsed_micros
    }

    pub fn local_motion(&self) -> Option<LocalMotion> {
        self.local.clone()
    }
}
