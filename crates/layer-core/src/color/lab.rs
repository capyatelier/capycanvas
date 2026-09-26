//! CIELAB relative to the ICC D50 profile connection space white.

pub const D50: [f64; 3] = [0.9642, 1., 0.8249];

pub fn xyz_to_lab(xyz: [f64; 3]) -> [f64; 3] {
    let f = |v: f64| {
        if v > (6f64 / 29.).powi(3) {
            v.cbrt()
        } else {
            v * 841. / 108. + 4. / 29.
        }
    };
    let [x, y, z] = std::array::from_fn(|i| f(xyz[i] / D50[i]));
    [116. * y - 16., 500. * (x - y), 200. * (y - z)]
}

pub fn lab_to_xyz([l, a, b]: [f64; 3]) -> [f64; 3] {
    let y = (l + 16.) / 116.;
    let f = |v: f64| {
        if v > 6. / 29. {
            v.powi(3)
        } else {
            (v - 4. / 29.) * 108. / 841.
        }
    };
    let lab = [y + a / 500., y, y - b / 200.];
    std::array::from_fn(|i| f(lab[i]) * D50[i])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lab_round_trips_white_black_and_knee() {
        let white = xyz_to_lab(D50);
        assert!(
            (white[0] - 100.).abs() < 1e-12 && white[1].abs() < 1e-12 && white[2].abs() < 1e-12
        );
        assert!(xyz_to_lab([0.; 3]).into_iter().all(|v| v.abs() < 1e-12));
        let knee = (6f64 / 29.).powi(3);
        for scale in [0.5, 0.999, 1.001, 2.] {
            let xyz = D50.map(|w| w * knee * scale);
            let back = lab_to_xyz(xyz_to_lab(xyz));
            assert!(
                back.into_iter()
                    .zip(xyz)
                    .all(|(a, b)| (a - b).abs() < 1e-12),
                "{scale}"
            );
        }
    }
}
