//! Exact encoded histogram classification without evaluating a transfer curve
//! per sample. A Float32 lookup supplies only a hint; Float64 boundaries decide
//! the final bin of the original, unrounded unassociated value.
use crate::color::RgbSpace;
use std::sync::OnceLock;

// Seven mantissa bits plus exponent. The three tables occupy under 54 KiB;
// sRGB and Display P3 share a transfer curve. No image-sized cache is retained.
const SHIFT: u32 = 16;
const SEEDS: usize = (1.0f32.to_bits() >> SHIFT) as usize + 1;

pub(super) struct EncodedBins {
    boundaries: [f64; 257],
    seeds: Box<[u8]>,
}

pub(super) fn for_space(space: RgbSpace) -> &'static EncodedBins {
    static SRGB: OnceLock<EncodedBins> = OnceLock::new();
    static ADOBE: OnceLock<EncodedBins> = OnceLock::new();
    static PROPHOTO: OnceLock<EncodedBins> = OnceLock::new();
    let cached = match space {
        RgbSpace::Srgb | RgbSpace::DisplayP3 => &SRGB,
        RgbSpace::AdobeRgb => &ADOBE,
        RgbSpace::ProPhoto => &PROPHOTO,
    };
    cached.get_or_init(|| EncodedBins::new(space))
}

impl EncodedBins {
    fn new(space: RgbSpace) -> Self {
        let boundaries = std::array::from_fn(|i| {
            if i == 0 {
                return 0.;
            }
            if i == 256 {
                return 1.;
            }
            // Find the first Float64 value classified into bin i by the
            // forward curve. Using decode(i/256) alone can differ by an ULP
            // at boundaries because encode/decode are rounded independently.
            let target = i as f64 / 256.;
            let (mut lo, mut hi) = (0u64, 1.0f64.to_bits());
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                if space.encode(f64::from_bits(mid)) < target {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            f64::from_bits(lo)
        });
        let seeds = (0..SEEDS)
            .map(|i| {
                let value = f64::from(f32::from_bits((i as u32) << SHIFT));
                boundaries[1..256].partition_point(|&b| b <= value) as u8
            })
            .collect();
        Self { boundaries, seeds }
    }

    pub(super) fn index(&self, linear: f64) -> usize {
        // Histogram::add validates finite pixels and positive, finite alpha.
        // Their Float64 ratio remains finite even for Float32 subnormal alpha.
        if linear <= 0. {
            return 0;
        }
        if linear >= 1. {
            return 255;
        }
        let bin = usize::from(self.seeds[((linear as f32).to_bits() >> SHIFT) as usize]);
        // The hint can straddle a boundary in either direction. Never quantize
        // the actual sample to Float32 or the lookup grid for classification.
        // Every lookup cell spans at most one bin boundary in these transfer
        // curves (including Float32 rounding into a neighboring cell). The
        // exhaustive cell-extent test guards this property if a curve changes.
        bin + usize::from(linear >= self.boundaries[bin + 1])
            - usize::from(linear < self.boundaries[bin])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direct(space: RgbSpace, linear: f64) -> usize {
        (space.encode(linear).clamp(0., 1.) * 256.)
            .floor()
            .min(255.) as usize
    }

    #[test]
    fn classification_matches_transfer_at_native_codes_and_bin_boundaries() {
        for space in RgbSpace::ALL {
            let bins = for_space(space);
            let check = |v| assert_eq!(bins.index(v), direct(space, v), "{space:?}, {v:e}");
            for code in 0..=65535 {
                let v = space.decode(f64::from(code) / 65535.);
                check(v);
                check(f64::from(v as f32));
            }
            for &boundary in &bins.boundaries[1..256] {
                // Test both the Float64 decision and the Float32 lookup hint,
                // including values that round onto either side of an edge.
                for delta in -8..=8 {
                    check(f64::from_bits(
                        boundary.to_bits().checked_add_signed(delta).unwrap(),
                    ));
                    check(f64::from(f32::from_bits(
                        (boundary as f32)
                            .to_bits()
                            .checked_add_signed(delta as i32)
                            .unwrap(),
                    )));
                }
            }
            for v in [
                -1e83,
                -0.1,
                -0.,
                0.,
                f64::MIN_POSITIVE,
                1e-83,
                0.0031308,
                1.,
                1e83,
            ] {
                check(v);
            }
        }
    }

    #[test]
    fn every_lookup_cell_needs_at_most_one_exact_correction() {
        for space in RgbSpace::ALL {
            let bins = for_space(space);
            for i in 0..SEEDS {
                let seed = i32::from(bins.seeds[i]);
                // Enclose every Float64 input that can round into this cell,
                // extending below its first Float32 and up to the next cell.
                let low = f64::from(f32::from_bits(((i as u32) << SHIFT).saturating_sub(1)));
                let high = f64::from(f32::from_bits(((i + 1) as u32) << SHIFT)).min(1.);
                for v in [low, high] {
                    let expected = direct(space, v);
                    assert!((expected as i32 - seed).abs() <= 1, "{space:?}, cell {i}");
                    assert_eq!(bins.index(v), expected);
                }
            }
        }
    }
}
