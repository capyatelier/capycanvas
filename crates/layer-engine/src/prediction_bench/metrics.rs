use serde::Serialize;

/// Flicker is a change in prediction error at each prediction's own target time.
/// Actual motion, including genuine turns and stops, therefore scores zero.
#[derive(Debug, Default, Serialize)]
pub struct Accuracy {
    pub graded_queries: usize,
    pub transitions: usize,
    pub tiny_4_to_8: usize,
    pub small_8_to_16: usize,
    pub medium_16_to_32: usize,
    pub severe_ge32: usize,
    pub position_rms_px: f64,
    pub error_step_rms_px: f64,
    pub worst_step_px: f64,
    #[serde(skip)]
    position_squared: f64,
    #[serde(skip)]
    step_squared: f64,
}
impl Accuracy {
    pub(super) fn add(&mut self, error: [f64; 2], previous: Option<[f64; 2]>) {
        let norm = |e: [f64; 2]| e[0].hypot(e[1]);
        self.graded_queries += 1;
        self.position_squared += norm(error).powi(2);
        if let Some(old) = previous {
            let jump = norm([error[0] - old[0], error[1] - old[1]]);
            let worst_error = norm(error).max(norm(old));
            self.transitions += 1;
            self.step_squared += jump * jump;
            self.worst_step_px = self.worst_step_px.max(jump);
            if (4. ..8.).contains(&jump) && worst_error >= 4. {
                self.tiny_4_to_8 += 1;
            } else if jump >= 8. && worst_error >= 8. {
                if jump < 16. {
                    self.small_8_to_16 += 1;
                } else if jump < 32. {
                    self.medium_16_to_32 += 1;
                } else {
                    self.severe_ge32 += 1;
                }
            }
        }
    }
    pub(super) fn finish(&mut self) {
        self.position_rms_px = (self.position_squared / self.graded_queries.max(1) as f64).sqrt();
        self.error_step_rms_px = (self.step_squared / self.transitions.max(1) as f64).sqrt();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn score(points: impl Iterator<Item = ([f64; 2], [f64; 2])>) -> Accuracy {
        let mut result = Accuracy::default();
        let mut previous = None;
        for (prediction, truth) in points {
            let error = [prediction[0] - truth[0], prediction[1] - truth[1]];
            result.add(error, previous);
            previous = Some(error);
        }
        result.finish();
        result
    }
    #[test]
    fn actual_motion_and_variable_target_times_have_zero_flicker() {
        for shape in 0..5 {
            for varying in [false, true] {
                let m = score((0..100).map(|i| {
                    // Repeated and backward targets alongside ordinary advancement.
                    let t =
                        (i * 8 + if varying { [32, 0, 16, 8][i % 4] } else { 0 }) as f64 / 1000.;
                    let p = match shape {
                        0 => [6000. * t, 0.],
                        1 => [400. * (20. * t).cos(), 400. * (20. * t).sin()],
                        2 => [
                            (100. + 1000. * t) * (20. * t).cos(),
                            (100. + 1000. * t) * (20. * t).sin(),
                        ],
                        3 => [2000. * t + 100. * (40. * t).sin(), 80. * (60. * t).sin()],
                        _ if t <= 0.032 => [4000. * t, 0.],
                        _ if t <= 0.064 => [128., 0.],
                        _ if t <= 0.096 => [128., 4000. * (t - 0.064)],
                        _ if t <= 0.128 => [128., 128.],
                        _ => [128. - 4000. * (t - 0.128), 128.],
                    };
                    (p, p)
                }));
                assert_eq!(m.transitions, 99);
                assert_eq!(m.worst_step_px, 0.);
                assert_eq!(m.position_rms_px, 0.);
                assert_eq!(m.error_step_rms_px, 0.);
                assert_eq!(
                    m.tiny_4_to_8 + m.small_8_to_16 + m.medium_16_to_32 + m.severe_ge32,
                    0
                );
            }
        }
    }
    #[test]
    fn detects_wrong_output_on_both_axes_but_separates_constant_bias() {
        for axis in 0..2 {
            let m = score((0..12).map(|i| {
                let truth = [i as f64 * 24., 0.];
                let mut p = truth;
                p[axis] += if i % 2 == 0 { -10. } else { 10. };
                (p, truth)
            }));
            assert_eq!(m.medium_16_to_32, 11);
            assert_eq!(m.error_step_rms_px, 20.);
            assert_eq!(m.position_rms_px, 10.);
        }
        let m = score((0..12).map(|i| ([i as f64 + 40., 0.], [i as f64, 0.])));
        assert_eq!(m.error_step_rms_px, 0.);
        assert_eq!(m.position_rms_px, 40.);
    }
}
