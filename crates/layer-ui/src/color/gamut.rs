//! RGB-gamut geometry for the perceptual picker. Ottosson's original fitted
//! cusp coefficients describe sRGB only; derive the outer channel boundary
//! from the actual RGB matrix for each supported document space instead.
use layer_core::color::{
    RgbSpace,
    rgb::{Matrix3, apply},
};

pub(super) struct Gamut {
    pub space: RgbSpace,
    to_srgb: Matrix3,
    from_srgb: Matrix3,
    from_lms: Matrix3,
}

impl Gamut {
    pub fn get(space: RgbSpace) -> &'static Self {
        static GAMUTS: std::sync::LazyLock<[Gamut; 4]> =
            std::sync::LazyLock::new(|| RgbSpace::ALL.map(Gamut::new));
        &GAMUTS[RgbSpace::ALL.iter().position(|s| *s == space).unwrap()]
    }

    fn new(space: RgbSpace) -> Self {
        let from_srgb = RgbSpace::Srgb.linear_transform(space);
        let lms = [
            [4.0767416621, -3.3077115913, 0.2309699292],
            [-1.2684380046, 2.6097574011, -0.3413193965],
            [-0.0041960863, -0.7034186147, 1.7076147010],
        ];
        Self {
            space,
            to_srgb: space.linear_transform(RgbSpace::Srgb),
            from_srgb,
            from_lms: std::array::from_fn(|i| {
                std::array::from_fn(|j| (0..3).map(|k| from_srgb[i][k] * lms[k][j]).sum())
            }),
        }
    }

    pub fn lab(&self, linear_rgb: [f64; 3]) -> [f64; 3] {
        super::okhsv::to_lab(apply(self.to_srgb, linear_rgb))
    }

    pub fn linear_rgb(&self, lab: [f64; 3]) -> [f64; 3] {
        apply(self.from_srgb, super::okhsv::from_lab(lab))
    }

    /// At L=1, each channel is a cubic in C/L. Select the outermost root with
    /// all channels nonnegative. Near blue the gamut can leave and reenter a
    /// hue ray; its first root would exclude valid blue colors. No sRGB-fitted
    /// branch classifier or clipping determines the cusp.
    pub fn max_saturation(&self, a: f64, b: f64) -> f64 {
        let slopes = [
            0.3963377774 * a + 0.2158037573 * b,
            -0.1055613458 * a - 0.0638541728 * b,
            -0.0894841775 * a - 1.2914855480 * b,
        ];
        self.from_lms
            .iter()
            .flat_map(|weights| {
                let coefficients = [
                    weights.iter().sum(),
                    (0..3).map(|i| 3. * weights[i] * slopes[i]).sum(),
                    (0..3).map(|i| 3. * weights[i] * slopes[i].powi(2)).sum(),
                    (0..3).map(|i| weights[i] * slopes[i].powi(3)).sum(),
                ];
                positive_roots(coefficients)
            })
            .filter(|s| {
                self.linear_rgb([1., s * a, s * b])
                    .iter()
                    .all(|v| *v >= -1e-10)
            })
            .fold(0., f64::max)
    }
}

/// Split at derivative roots, so each bracket is monotonic even when a channel
/// leaves and later reenters the gamut. Cauchy's bound closes the final bracket.
fn positive_roots(c: [f64; 4]) -> Vec<f64> {
    let Some(degree) = (1..=3).rev().find(|i| c[*i].abs() > 1e-14) else {
        return Vec::new();
    };
    let bound = 1.
        + c[..degree]
            .iter()
            .map(|v| v.abs() / c[degree].abs())
            .fold(0., f64::max);
    let mut endpoints = vec![0., bound];
    if degree == 2 {
        endpoints.push(-c[1] / (2. * c[2]));
    } else if degree == 3 {
        let [a, b, d] = [3. * c[3], 2. * c[2], c[1]];
        let discriminant = b * b - 4. * a * d;
        if discriminant >= 0. {
            let q = -0.5 * (b + discriminant.sqrt().copysign(b));
            if q != 0. {
                endpoints.extend([q / a, d / q]);
            } else {
                endpoints.push(-b / (2. * a));
            }
        }
    }
    endpoints.retain(|v| v.is_finite() && *v >= 0. && *v <= bound);
    endpoints.sort_by(f64::total_cmp);
    let evaluate = |t: f64| {
        c[..=degree]
            .iter()
            .rev()
            .fold(0., |v, coefficient| v * t + coefficient)
    };
    let mut roots = Vec::new();
    for pair in endpoints.windows(2) {
        let [mut lo, mut hi] = [pair[0], pair[1]];
        let left = evaluate(lo);
        let right = evaluate(hi);
        if hi > 0. && right.abs() < 1e-14 {
            roots.push(hi);
            continue;
        }
        if left.is_sign_positive() == right.is_sign_positive() {
            continue;
        }
        for _ in 0..96 {
            let mid = (lo + hi) * 0.5;
            if evaluate(mid).is_sign_positive() == left.is_sign_positive() {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        roots.push((lo + hi) * 0.5);
    }
    roots.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cubic_roots_include_reentry_and_degenerate_cases() {
        for (polynomial, expected) in [
            ([1., -1., 0., 0.], vec![1.]),
            ([2., -3., 1., 0.], vec![1., 2.]),
            ([6., -11., 6., -1.], vec![1., 2., 3.]),
            ([1., -2., 1., 0.], vec![1.]),
        ] {
            let actual = positive_roots(polynomial);
            assert_eq!(actual.len(), expected.len());
            for (a, b) in actual.into_iter().zip(expected) {
                assert!((a - b).abs() < 1e-12);
            }
        }
        assert!(positive_roots([1., 0., 1., 0.]).is_empty());
    }

    #[test]
    fn every_builtin_hue_reaches_its_own_lower_gamut_boundary() {
        for space in RgbSpace::ALL {
            let gamut = Gamut::get(space);
            for step in 0..3600 {
                let angle = (step as f64 / 10.).to_radians();
                let (b, a) = angle.sin_cos();
                let saturation = gamut.max_saturation(a, b);
                assert!(
                    saturation.is_finite() && saturation > 0.,
                    "{space:?} {step}"
                );
                let boundary = gamut.linear_rgb([1., saturation * a, saturation * b]);
                assert!(
                    boundary.iter().all(|v| *v >= -1e-10),
                    "{space:?} {step}: {boundary:?}"
                );
                assert!(
                    boundary.iter().any(|v| v.abs() < 1e-10),
                    "{space:?} {step}: {boundary:?}"
                );
                let beyond =
                    gamut.linear_rgb([1., saturation * 1.0001 * a, saturation * 1.0001 * b]);
                assert!(
                    beyond.iter().any(|v| *v < 0.),
                    "{space:?} {step}: {beyond:?}"
                );
            }
        }
    }
}
