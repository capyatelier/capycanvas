use serde::Serialize;

/// Endpoint position error; use the frame export for perceptual stability.
#[derive(Debug, Default, Serialize)]
pub struct Accuracy {
    pub graded_queries: usize,
    pub position_rms_px: f64,
    #[serde(skip)]
    position_squared: f64,
}
impl Accuracy {
    pub(super) fn add(&mut self, error: [f64; 2]) {
        self.graded_queries += 1;
        self.position_squared += error[0].hypot(error[1]).powi(2);
    }
    pub(super) fn finish(&mut self) {
        self.position_rms_px = (self.position_squared / self.graded_queries.max(1) as f64).sqrt();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn score(points: impl Iterator<Item = ([f64; 2], [f64; 2])>) -> Accuracy {
        let mut result = Accuracy::default();
        for (prediction, truth) in points {
            result.add([prediction[0] - truth[0], prediction[1] - truth[1]]);
        }
        result.finish();
        result
    }
    #[test]
    fn position_rms_covers_both_axes_and_constant_bias() {
        for axis in 0..2 {
            let m = score((0..12).map(|i| {
                let truth = [i as f64 * 24., 0.];
                let mut p = truth;
                p[axis] += if i % 2 == 0 { -10. } else { 10. };
                (p, truth)
            }));
            assert_eq!(m.graded_queries, 12);
            assert_eq!(m.position_rms_px, 10.);
        }
        let m = score((0..12).map(|i| ([i as f64 + 40., 0.], [i as f64, 0.])));
        assert_eq!(m.position_rms_px, 40.);
    }
}
