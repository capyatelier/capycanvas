//! Deterministic stochastic rounding. Adjacent integer codes are chosen with
//! probability equal to the fractional code, so the mean follows the signal.
//! One threshold per pixel preserves neutral RGB; alpha never uses this noise.

pub(super) fn threshold([x, y]: [u32; 2]) -> f64 {
    // SplitMix64 finalizer, adapted from Sebastiano Vigna's public-domain code:
    // https://prng.di.unimi.it/splitmix64.c . Coordinates replace mutable RNG
    // state: repeated exports and independently processed strips agree.
    let mut z = ((u64::from(y) << 32) | u64::from(x)).wrapping_add(0x9e3779b97f4a7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    let bits = (z ^ (z >> 31)) >> 11;
    bits as f64 * (1. / 9007199254740992.)
}

pub(super) fn code(value: f64, maximum: f64, threshold: Option<f64>) -> u16 {
    let scaled = value.clamp(0., 1.) * maximum;
    let rounded = if let Some(threshold) = threshold {
        let lower = scaled.floor();
        lower + f64::from(u8::from(threshold < scaled - lower))
    } else {
        scaled.round()
    };
    rounded as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dither_tracks_fractional_codes_without_drift_and_preserves_endpoints() {
        for fraction in [0., 0.01, 0.1, 0.25, 0.5, 0.75, 0.9, 0.99] {
            let target = 128. + fraction;
            let mut sum = 0.;
            for y in 0..256 {
                for x in 0..256 {
                    let actual = f64::from(code(target / 255., 255., Some(threshold([x, y]))));
                    assert!((actual - target).abs() < 1. || fraction == 0. && actual == target);
                    sum += actual;
                }
            }
            assert!((sum / 65536. - target).abs() < 0.004, "fraction {fraction}");
        }
        for v in [-1., 0., 1., 2.] {
            for x in 0..1024 {
                assert_eq!(
                    code(v, 255., Some(threshold([x, 5]))),
                    if v <= 0. { 0 } else { 255 }
                );
            }
        }
    }
}
