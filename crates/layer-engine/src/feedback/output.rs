//! Join the disposable forecast to measured ink without copying input noise to its tip.

use super::{surface_distance, trajectory::Trajectory};
use layer_core::{Point, StrokePoint};

#[derive(Clone, Debug)]
pub(super) struct Output {
    motion: Trajectory,
    pub horizon: u32,
    anchor: StrokePoint,
    transform: [f32; 6],
    maximum_distance: f32,
}

impl Output {
    pub fn new(
        motion: Trajectory,
        horizon: u32,
        transform: [f32; 6],
        maximum_distance: f32,
    ) -> Self {
        Self {
            anchor: motion.point_at(0),
            motion,
            horizon,
            maximum_distance,
            transform,
        }
    }

    pub fn point_at(&self, time: u32) -> StrokePoint {
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
        super::clamp_prediction(self.anchor, point, self.transform, self.maximum_distance)
    }
}
