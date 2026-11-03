use layer_core::{BrushStabilization, Point, StrokePoint};

const MAX_TIME_SECONDS: f64 = 0.1;
const INTERVAL_RESPONSE_SECONDS: f64 = 0.015;

mod curve;
mod preview;
pub(crate) use preview::{CurveModel, PositionField, CurvePreview, pen_up_settlement_micros};

#[derive(Clone, Debug, Default)]
pub(super) struct PositionSmoother {
    previous: Option<StrokePoint>,
    position: Option<Point>,
    interval: f64,
    curve: Option<curve::Curve>,
}
impl PositionSmoother {
    pub(super) fn previous(&self) -> Option<StrokePoint> { self.previous }
    pub(super) fn settled_position(&self) -> Option<Point> {
        self.curve.as_ref().map(|curve| curve.latest().position)
    }
    pub(super) fn observe(&mut self, point: StrokePoint) { self.previous = Some(point); }
    pub(super) fn preview_model(&self, settings: &BrushStabilization, real: &[StrokePoint]) -> Option<CurveModel> {
        CurveModel::from_smoother(self.clone(), settings, real)
    }
    pub(super) fn append(&mut self, point: StrokePoint, settings: &BrushStabilization) -> Point {
        let previous = self.previous.replace(point);
        if settings.strength == 0. {
            self.position = Some(point.position);
            return point.position;
        }
        let elapsed = previous.map_or(0., |p| f64::from(point.elapsed_micros.saturating_sub(p.elapsed_micros)) * 1e-6);
        if elapsed > 0. {
            let response = -(-elapsed/INTERVAL_RESPONSE_SECONDS).exp_m1();
            self.interval = if self.interval == 0. { elapsed } else { self.interval+response*(elapsed-self.interval) };
        }
        let tau = f64::from(settings.strength)*MAX_TIME_SECONDS;
        let Some(last) = self.position else {
            self.position = Some(point.position);
            self.curve = Some(curve::Curve::new(point));
            return point.position;
        };
        let next = if elapsed == 0. {
            self.curve.as_mut().unwrap().append(point, curve::calibrated_support(tau, self.interval));
            if previous.is_some_and(|p| p.elapsed_micros > 0) { last } else { point.position }
        } else {
            self.curve.as_mut().unwrap().append(point, curve::calibrated_support(tau, self.interval))
        };
        self.position = Some(next);
        next
    }
    pub(super) fn completion_model(&self) -> Option<CurveModel> {
        self.curve
            .as_ref()
            .filter(|curve| curve.latest().elapsed_micros > 0)
            .cloned()
            .map(CurveModel::completion)
    }
    pub(super) fn take_completion_model(&mut self) -> Option<CurveModel> {
        self.curve.take().filter(|curve| curve.latest().elapsed_micros > 0).map(CurveModel::completion)
    }
    pub(super) fn curve_position(&self, time: f64) -> Option<Point> { self.curve.as_ref().map(|c| c.position(time)) }
    pub(super) fn retain_from(&mut self, time: u32) { if let Some(curve) = &mut self.curve { curve.retain_from(time); } }
    pub(super) fn acceleration_bound(&self, begin: f64, end: f64) -> f64 {
        self.curve.as_ref().map_or(0., |curve| curve.acceleration_bound(begin, end))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::point;

    fn filter(points: &[StrokePoint], settings: BrushStabilization) -> Vec<Point> {
        let mut smoother = PositionSmoother::default();
        points.iter().map(|&p| smoother.append(p, &settings)).collect()
    }

    #[test]
    fn disabled_and_untimed_paths_preserve_every_position() {
        let points: Vec<_> = (0..30).map(|i| point(i as f32, (i % 3) as f32, 1., i * 4000)).collect();
        let settings = BrushStabilization { strength: 0., ..Default::default() };
        assert_eq!(filter(&points, settings), points.iter().map(|p| p.position).collect::<Vec<_>>());
        let untimed: Vec<_> = points.iter().map(|&p| StrokePoint { elapsed_micros: 0, ..p }).collect();
        let settings = BrushStabilization { strength: 1., ..Default::default() };
        assert_eq!(filter(&untimed, settings), untimed.iter().map(|p| p.position).collect::<Vec<_>>());
    }

    #[test]
    fn duplicate_timed_release_does_not_jump_to_the_raw_pen_position() {
        let points = [point(0., 0., 1., 0), point(100., 0., 1., 4000), point(100., 0., 0., 4000)];
        let output = filter(&points, BrushStabilization { strength: 1., ..Default::default() });
        assert_eq!(output[1], output[2]);
        assert!(output[2].x < 10.);
    }

    #[test]
    fn curve_fit_is_causal_across_strengths_and_input_rates() {
        for interval in [1000, 4000, 8333, 16000] {
            let points: Vec<_> = (0..300).map(|i| point(i as f32, (i as f32 * 0.17).sin() * 20., 1., i * interval)).collect();
            for strength in [0.01, 0.06, 0.3, 0.5, 1.] {
                let settings = BrushStabilization { strength, ..Default::default() };
                let fitted = filter(&points, settings);
                for end in [80, 120, 160] { assert_eq!(filter(&points[..end], settings), fitted[..end]); }
            }
        }
    }

    #[test]
    fn smoothing_is_independent_of_speed_and_view_scale() {
        let points = |speed: f32| (0..300).map(|i| point(i as f32 * 0.004 * speed, 0., 1., i * 4000)).collect::<Vec<_>>();
        let settings = BrushStabilization { strength: 0.25, ..Default::default() };
        let slow = filter(&points(60.), settings);
        let fast = filter(&points(1200.), settings);
        for (a, b) in slow.iter().zip(&fast) { assert!((a.x * 20. - b.x).abs() < 0.001); }
        let zoomed = filter(&points(60.), BrushStabilization { input_scale: 2., ..settings });
        assert_eq!(slow, zoomed);
    }

}
