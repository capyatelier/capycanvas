//! One SDR rendition: fixed local baseline, then macro/micro contrast in log odds.
//! The spatial guide, working master and reference white never depend on the UI.
use super::super::{RgbSpace, rgb::Matrix3};
use super::{LocalToneGuide, pq_decode, pq_encode};
use serde::{Deserialize, Serialize};

const DEFAULT_HEADROOM: f32 = 2.300_448_4;
/// Fixed baseline compression. The contrast control must never change this or
/// retune the output shoulder. HDR range fitting precedes artistic contrast.
pub const BASELINE_COMPRESSION: f32 = 0.6;
const GRAY_LOG: f32 = -2.473931;
const ODDS_PIVOT: f32 = -2.187627;
// Authored baseline: the reviewed 130% contrast / +30% micro treatment.
// Stored controls and their readouts are relative to this baseline.
const BASELINE_CONTRAST: f32 = 1.3;
const BASELINE_BALANCE: f32 = 0.3;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SdrRendition {
    pub exposure: f32,
    /// Overall contrast multiplier; 1 is the centered, automatic baseline.
    pub contrast: f32,
    /// Saved input white endpoint, stops above RGB 1.
    pub headroom: f32,
    /// 0 favors luminous white highlights; 1 retains color by lowering luminance.
    pub highlight_color: f32,
    /// Macro/micro balance relative to the authored baseline.
    pub balance: f32,
}
impl Default for SdrRendition {
    fn default() -> Self {
        Self {
            exposure: 0.,
            contrast: 1.,
            headroom: DEFAULT_HEADROOM,
            highlight_color: 0.3,
            balance: 0.,
        }
    }
}
impl<'de> Deserialize<'de> for SdrRendition {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Stored {
            exposure: f32,
            contrast: f32,
            headroom: f32,
            highlight_color: f32,
            balance: f32,
        }
        let s = Stored::deserialize(d)?;
        let r = Self {
            exposure: s.exposure,
            contrast: s.contrast,
            headroom: s.headroom,
            highlight_color: s.highlight_color,
            balance: s.balance,
        };
        r.validate().map_err(serde::de::Error::custom)?;
        Ok(r)
    }
}
impl SdrRendition {
    pub fn validate(self) -> Result<(), &'static str> {
        if !self.exposure.is_finite()
            || !(-12. ..=12.).contains(&self.exposure)
            || !self.contrast.is_finite()
            || !(0.5..=2.).contains(&self.contrast)
            || !self.headroom.is_finite()
            || !(0. ..=16.).contains(&self.headroom)
            || !self.highlight_color.is_finite()
            || !(0. ..=1.).contains(&self.highlight_color)
            || !self.balance.is_finite()
            || !(-1. ..=1.).contains(&self.balance)
        {
            return Err("Invalid SDR rendition settings");
        }
        Ok(())
    }
    /// Same transport for native/Web shaders and preview keys. Slot 3 enables
    /// mapping; zero disables it. No method discriminator or legacy recipes.
    pub fn parameters(self) -> [f32; 8] {
        [
            self.exposure,
            self.contrast,
            self.headroom,
            1.,
            self.highlight_color,
            self.balance,
            0.,
            0.,
        ]
    }
    pub fn from_parameters(p: [f32; 8]) -> Result<Self, &'static str> {
        if p[3] != 1. || p[6] != 0. || p[7] != 0. {
            return Err("Invalid SDR parameters");
        }
        let r = Self {
            exposure: p[0],
            contrast: p[1],
            headroom: p[2],
            highlight_color: p[4],
            balance: p[5],
        };
        r.validate()?;
        Ok(r)
    }
    pub fn gains(self) -> [f32; 2] {
        let contrast = BASELINE_CONTRAST * self.contrast;
        let balance = BASELINE_BALANCE + self.balance;
        [
            contrast * (-0.5 * balance).exp2(),
            contrast * (0.5 * balance).exp2(),
        ]
    }
    pub fn mapper(self, source: RgbSpace, destination: RgbSpace) -> SdrMapper {
        SdrMapper {
            recipe: self,
            gains: self.gains(),
            baseline: Bt2390::new(
                (self.headroom * (1. - BASELINE_COMPRESSION) + BASELINE_COMPRESSION * GRAY_LOG)
                    .max(0.),
            ),
            source_luma: sdr_luminance_weights(source),
            to_output: source
                .linear_transform(destination)
                .map(|r| r.map(|v| v as f32)),
            output_luma: sdr_luminance_weights(destination),
        }
    }
    pub fn map_rgb(self, rgb: [f32; 3], space: RgbSpace) -> [f32; 3] {
        self.mapper(space, space).map_rgb(rgb)
    }
    pub fn map_premultiplied(self, p: [f32; 4], space: RgbSpace) -> [f32; 4] {
        self.mapper(space, space).map_premultiplied(p)
    }
}
pub fn to_bt2020(source: RgbSpace) -> Matrix3 {
    let a = super::srgb_to_bt2020();
    let b = source.linear_transform(RgbSpace::Srgb);
    std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
}
fn apply(m: [[f32; 3]; 3], rgb: [f32; 3]) -> [f32; 3] {
    m.map(|r| r[0] * rgb[0] + r[1] * rgb[1] + r[2] * rgb[2])
}

#[derive(Clone, Copy)]
pub struct SdrMapper {
    recipe: SdrRendition,
    gains: [f32; 2],
    baseline: Bt2390,
    source_luma: [f32; 3],
    to_output: [[f32; 3]; 3],
    output_luma: [f32; 3],
}
fn log_odds(y: f32) -> f32 {
    (y / (1. - y)).log2()
}
fn from_odds(u: f32) -> f32 {
    let v = u.clamp(-126., 120.).exp2();
    v / (1. + v)
}
impl SdrMapper {
    fn tone_with_base(self, rgb: [f32; 3], base: f32) -> [f32; 3] {
        let y = luminance(rgb, self.source_luma);
        if y <= 0. {
            return [0.; 3];
        }
        let log_y = y.max(2f32.powi(-24)).log2();
        let broad = GRAY_LOG + (1. - BASELINE_COMPRESSION) * (base - GRAY_LOG);
        let baseline = self.baseline.map((broad + log_y - base).exp2());
        let mapped = if baseline <= 0. || baseline >= 1. {
            baseline.clamp(0., 1.)
        } else if self.gains == [1., 1.] && self.recipe.exposure == 0. {
            baseline
        } else {
            let u = log_odds(baseline);
            // The same edge-preserving HDR illumination guide supplies B.
            // Transform the broad component through the fixed SDR baseline;
            // D=u-B then reconstructs u exactly at the centered control.
            let b = log_odds(
                self.baseline
                    .map(broad.exp2())
                    .clamp(2f32.powi(-24), 1. - 2f32.powi(-24)),
            );
            from_odds(
                ODDS_PIVOT
                    + self.gains[0] * (b - ODDS_PIVOT)
                    + self.gains[1] * (u - b)
                    + self.recipe.exposure,
            )
        };
        rgb.map(|v| v / y * mapped)
    }
    /// Isolated colors have no neighborhood: use the homogeneous illumination
    /// estimate. Canvas, proof and delivery use tone_local_premultiplied instead.
    pub fn tone_rgb(self, rgb: [f32; 3]) -> [f32; 3] {
        let base = luminance(rgb, self.source_luma).max(2f32.powi(-24)).log2();
        self.tone_with_base(rgb, base)
    }
    pub fn tone_premultiplied(self, p: [f32; 4]) -> [f32; 4] {
        if p[3] <= 0. {
            return [0.; 4];
        }
        let rgb = self.tone_rgb([p[0] / p[3], p[1] / p[3], p[2] / p[3]]);
        [rgb[0] * p[3], rgb[1] * p[3], rgb[2] * p[3], p[3]]
    }
    pub fn tone_local_premultiplied(
        self,
        p: [f32; 4],
        position: [f32; 2],
        guide: &LocalToneGuide,
    ) -> [f32; 4] {
        if p[3] <= 0. {
            return [0.; 4];
        }
        let rgb = [p[0] / p[3], p[1] / p[3], p[2] / p[3]];
        let log_y = luminance(rgb, self.source_luma).max(2f32.powi(-24)).log2();
        let base = guide.illumination(position, log_y);
        let rgb = self.tone_with_base(rgb, base);
        [rgb[0] * p[3], rgb[1] * p[3], rgb[2] * p[3], p[3]]
    }
    fn map_toned(self, p: [f32; 4]) -> [f32; 4] {
        if p[3] <= 0. {
            return [0.; 4];
        }
        let rgb = apply(self.to_output, [p[0] / p[3], p[1] / p[3], p[2] / p[3]]);
        let rgb = unified_sdr_gamut(rgb, self.output_luma, self.recipe.highlight_color);
        [rgb[0] * p[3], rgb[1] * p[3], rgb[2] * p[3], p[3]]
    }
    pub fn map_rgb(self, rgb: [f32; 3]) -> [f32; 3] {
        let p = self.map_premultiplied([rgb[0], rgb[1], rgb[2], 1.]);
        [p[0], p[1], p[2]]
    }
    pub fn map_premultiplied(self, p: [f32; 4]) -> [f32; 4] {
        self.map_toned(self.tone_premultiplied(p))
    }
    pub fn map_local_premultiplied(
        self,
        p: [f32; 4],
        position: [f32; 2],
        guide: &LocalToneGuide,
    ) -> [f32; 4] {
        self.map_toned(self.tone_local_premultiplied(p, position, guide))
    }
}
pub const BT2020_LUMA: [f32; 3] = [0.2627002, 0.6779981, 0.0593017];
fn luminance(rgb: [f32; 3], weights: [f32; 3]) -> f32 {
    rgb[0] * weights[0] + rgb[1] * weights[1] + rgb[2] * weights[2]
}
/// D65-adapted luminance keeps the mapping invariant under working-space changes,
/// including ProPhoto's D50 white. The weights sum to one.
pub fn sdr_luminance_weights(space: RgbSpace) -> [f32; 3] {
    let matrix = to_bt2020(space);
    std::array::from_fn(|c| {
        (0..3)
            .map(|r| f64::from(BT2020_LUMA[r]) * matrix[r][c])
            .sum::<f64>() as f32
    })
}
/// Unified destination-gamut policy. White retains luminance; Color retains
/// RGB ratios where possible, lowering luminance to fit the destination gamut.
/// Negative channels first move toward neutral, never clip independently.
pub fn unified_sdr_gamut(rgb: [f32; 3], weights: [f32; 3], color: f32) -> [f32; 3] {
    let white = compress_sdr_gamut(rgb, weights);
    if color == 0. {
        return white;
    }
    let y = luminance(rgb, weights);
    if y <= 0. {
        return [0.; 3];
    }
    let low = rgb.into_iter().fold(0., f32::min);
    let scale = y / (y - low);
    let positive = rgb.map(|v| (y + (v - y) * scale).max(0.));
    let peak = positive.into_iter().fold(1., f32::max);
    std::array::from_fn(|c| (white[c] + color * (positive[c] / peak - white[c])).clamp(0., 1.))
}
/// Compress toward the neutral axis at constant D65 luminance. This preserves
/// RGB hue direction instead of clipping channels independently. A rational
/// shoulder starts at 98% of the gamut boundary, with continuous first derivative.
/// It approaches (but cannot cross) that boundary. Neutral black/white are exact.
pub fn compress_sdr_gamut(rgb: [f32; 3], weights: [f32; 3]) -> [f32; 3] {
    let y = luminance(rgb, weights);
    if y <= 0. {
        return [0.; 3];
    }
    if y >= 1. {
        return [1.; 3];
    }
    let chroma = rgb.map(|v| v - y);
    let extent = chroma
        .into_iter()
        .map(|v| if v > 0. { v / (1. - y) } else { -v / y })
        .fold(0., f32::max);
    if extent <= 0.98 {
        return rgb;
    }
    let compressed = 0.98 + 0.02 * (extent - 0.98) / (extent - 0.96);
    chroma.map(|v| (y + v * (compressed / extent)).clamp(0., 1.))
}

/// ITU-R BT.2390 (2016), section 5.4: normalized PQ with a Hermite shoulder
/// with knee offset 1 (the libplacebo default), KS = 2 * maxLum - 1.
/// This starts the shoulder earlier than the report’s offset 0.5 to retain more
/// bright detail. Clamp the knee to zero for extended (>10,000 nit) masters
/// so the shoulder cannot produce negative PQ codes. Zero black stays zero.
/// Independently implemented from the report's equations, not libplacebo code.
/// RGB 1 is always 203 cd/m²; the destination white is the same reference white.
#[derive(Clone, Copy)]
struct Bt2390 {
    peak: f32,
    pq_peak: f32,
    output: f32,
    knee: f32,
}
impl Bt2390 {
    fn new(headroom: f32) -> Self {
        let peak = headroom.exp2();
        let pq_peak = pq_encode(f64::from(peak * super::REFERENCE_WHITE_NITS)) as f32;
        let output = pq_encode(f64::from(super::REFERENCE_WHITE_NITS)) as f32 / pq_peak;
        Self {
            peak,
            pq_peak,
            output,
            knee: (2. * output - 1.).max(0.),
        }
    }
    fn map(self, x: f32) -> f32 {
        if x <= 0. {
            return 0.;
        }
        if self.peak == 1. || x >= self.peak {
            return x.min(1.);
        }
        let q = pq_encode(f64::from(x * super::REFERENCE_WHITE_NITS)) as f32 / self.pq_peak;
        if q <= self.knee {
            return x;
        }
        let t = (q - self.knee) / (1. - self.knee);
        let t2 = t * t;
        let t3 = t2 * t;
        let q = (2. * t3 - 3. * t2 + 1.) * self.knee
            + (t3 - 2. * t2 + t) * (1. - self.knee)
            + (-2. * t3 + 3. * t2) * self.output;
        (pq_decode(f64::from(q * self.pq_peak)) as f32 / super::REFERENCE_WHITE_NITS).clamp(0., 1.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{hdr::LocalToneBuilder, rgb};
    #[test]
    fn perceptual_shoulder_matches_itu_float64_reference_and_retains_midtones() {
        // Independent Float64 evaluation of the report's E1/E2 and Hermite
        // equations. Tolerance includes Float32 PQ powers and inverse powers.
        let reference = |headroom: f64, x: f64| {
            if headroom == 0. {
                return x.min(1.);
            }
            if x >= headroom.exp2() {
                return 1.;
            }
            let pq = super::super::pq_encode;
            let span = pq(203. * headroom.exp2());
            let white = pq(203.) / span;
            let knee = (2. * white - 1.).max(0.);
            let input = pq(x * 203.) / span;
            if input <= knee {
                return x;
            }
            let t = (input - knee) / (1. - knee);
            let h = [
                2. * t.powi(3) - 3. * t * t + 1.,
                t.powi(3) - 2. * t * t + t,
                -2. * t.powi(3) + 3. * t * t,
            ];
            super::super::pq_decode((h[0] * knee + h[1] * (1. - knee) + h[2] * white) * span) / 203.
        };
        for h in [0., 0.0001, 0.25, 1., 2.3004484, 4., 8., 16.] {
            let curve = Bt2390::new(h);
            let mut previous = 0.;
            for i in 0..2048 {
                let x = (-16f32 + (h + 18.) * i as f32 / 2047.).exp2();
                let y = curve.map(x);
                assert!(
                    (y as f64 - reference(h as f64, x as f64)).abs() < 0.00015,
                    "{h} {x} {y}"
                );
                assert!(y >= previous - 0.0001 && y.is_finite() && y <= 1.);
                previous = y;
            }
        }
    }

    fn fixture() -> (LocalToneGuide, Vec<[f32; 4]>) {
        let mut builder = LocalToneBuilder::new([96, 32], RgbSpace::Srgb).unwrap();
        let pixels: Vec<_> = (0..96 * 32)
            .map(|i| {
                let y = (if i % 96 < 48 { 0.04 } else { 4. }) * (if i % 4 < 2 { 0.8 } else { 1.2 });
                [y, y, y, 1.]
            })
            .collect();
        for row in pixels.chunks_exact(96) {
            builder.push(row).unwrap();
        }
        (builder.finish(|| false).unwrap(), pixels)
    }
    #[test]
    fn center_matches_reviewed_130_30_and_vertical_is_monotonic_at_every_balance() {
        let (guide, pixels) = fixture();
        let r = SdrRendition {
            headroom: 6.,
            ..Default::default()
        };
        for x in [10, 20, 23, 46, 49, 70, 73, 90] {
            let p = pixels[x];
            let position = [x as f32 + 0.5, 0.5];
            let base = guide.illumination(position, p[0].log2());
            let curve = r.mapper(RgbSpace::Srgb, RgbSpace::Srgb).baseline;
            let broad = GRAY_LOG + (1. - BASELINE_COMPRESSION) * (base - GRAY_LOG);
            let y0 = curve.map((p[0].log2() - BASELINE_COMPRESSION * (base - GRAY_LOG)).exp2());
            let b = log_odds(
                curve
                    .map(broad.exp2())
                    .clamp(2f32.powi(-24), 1. - 2f32.powi(-24)),
            );
            // Independent reference to the previously approved 130% / +30%
            // rendering, before destination gamut policy.
            let expected = from_odds(
                ODDS_PIVOT
                    + 1.3 * 2f32.powf(-0.15) * (b - ODDS_PIVOT)
                    + 1.3 * 2f32.powf(0.15) * (log_odds(y0) - b),
            );
            let actual = r
                .mapper(RgbSpace::Srgb, RgbSpace::Srgb)
                .tone_local_premultiplied(p, position, &guide);
            assert!((actual[0] - expected).abs() < 0.000002);
            for balance in [-1., -0.5, 0., 0.5, 1.] {
                let mut odds = Vec::new();
                for contrast in [0.5, 1., 2.] {
                    let mapper = SdrRendition {
                        contrast,
                        balance,
                        ..r
                    }
                    .mapper(RgbSpace::Srgb, RgbSpace::Srgb);
                    let v = mapper.tone_local_premultiplied(p, position, &guide)[0];
                    assert!(v > 0. && v < 1.);
                    odds.push(log_odds(v) - ODDS_PIVOT);
                }
                for i in 0..2 {
                    // Compare in linear output: log odds amplifies Float32 quantization near white.
                    assert!(
                        (from_odds(odds[i + 1] + ODDS_PIVOT)
                            - from_odds(2. * odds[i] + ODDS_PIVOT))
                        .abs()
                            < 0.000002,
                        "{x} {balance} {odds:?}"
                    );
                    assert!(odds[i + 1].abs() >= odds[i].abs());
                }
            }
        }
    }
    #[test]
    fn gains_trade_scales_brightness_is_independent_and_alpha_is_coverage() {
        let (guide, pixels) = fixture();
        for balance in [-1., 0., 1.] {
            for contrast in [0.5, 1., 2.] {
                let r = SdrRendition {
                    balance,
                    contrast,
                    headroom: 6.,
                    ..Default::default()
                };
                let g = r.gains();
                assert!(((g[0] * g[1]).sqrt() - contrast * 1.3).abs() < 1e-6);
                let mapper = r.mapper(RgbSpace::Srgb, RgbSpace::Srgb);
                for alpha in [0.01, 0.25, 1.] {
                    let p = pixels[20];
                    let actual =
                        mapper.tone_local_premultiplied(p.map(|v| v * alpha), [20.5, 0.5], &guide);
                    let full = mapper.tone_local_premultiplied(p, [20.5, 0.5], &guide);
                    assert_eq!(actual[3], alpha);
                    assert!((actual[0] / alpha - full[0]).abs() < 1e-6);
                    let brighter = SdrRendition { exposure: 1., ..r }
                        .mapper(RgbSpace::Srgb, RgbSpace::Srgb)
                        .tone_local_premultiplied(p, [20.5, 0.5], &guide)[0];
                    assert!((log_odds(brighter) - log_odds(full[0]) - 1.).abs() < 0.0001);
                }
                assert_eq!(mapper.map_premultiplied([0.; 4]), [0.; 4]);
                assert_eq!(mapper.map_rgb([0.; 3]), [0.; 3]);
                assert_eq!(mapper.map_rgb([65504.; 3]), [1.; 3]);
            }
        }
    }
    #[test]
    fn working_primaries_and_gamut_mapping_preserve_luminance_and_hue_direction() {
        for input in [
            [0.18; 3],
            [8., 0., 0.],
            [0., 0., 16.],
            [-0.1, 3., 0.5],
            [0.7, 0.3, 0.15],
        ] {
            for color in [0., 0.5, 1.] {
                let r = SdrRendition {
                    headroom: 6.,
                    highlight_color: color,
                    ..Default::default()
                };
                let expected = r.mapper(RgbSpace::Srgb, RgbSpace::Srgb).map_rgb(input);
                for working in RgbSpace::ALL {
                    let converted = rgb::apply(
                        RgbSpace::Srgb.linear_transform(working),
                        input.map(f64::from),
                    )
                    .map(|v| v as f32);
                    let actual = r.mapper(working, RgbSpace::Srgb).map_rgb(converted);
                    for c in 0..3 {
                        assert!(
                            (actual[c] - expected[c]).abs() < 0.00015,
                            "{working:?} {actual:?} {expected:?}"
                        );
                        assert!((0. ..=1.).contains(&actual[c]));
                    }
                }
            }
        }
        let rgb = [3., 0.2, -0.1];
        let white = unified_sdr_gamut(rgb, BT2020_LUMA, 0.);
        let color = unified_sdr_gamut(rgb, BT2020_LUMA, 1.);
        assert!(luminance(white, BT2020_LUMA) >= luminance(color, BT2020_LUMA));
    }
    #[test]
    fn single_recipe_serializes_validates_and_uses_no_legacy_discriminator() {
        let r = SdrRendition {
            balance: 0.7,
            contrast: 1.3,
            exposure: -1.,
            headroom: 5.,
            highlight_color: 0.4,
        };
        let json = serde_json::to_string(&r).unwrap();
        for retired in ["method", "tone", "detail", "knee", "highlights"] {
            assert!(!json.contains(retired));
        }
        assert_eq!(serde_json::from_str::<SdrRendition>(&json).unwrap(), r);
        assert_eq!(SdrRendition::from_parameters(r.parameters()).unwrap(), r);
        for bad in [f32::NAN, f32::INFINITY, -1., 0., 2.01] {
            assert!(SdrRendition { contrast: bad, ..r }.validate().is_err());
        }
        for bad in [f32::NAN, -1.01, 1.01] {
            assert!(SdrRendition { balance: bad, ..r }.validate().is_err());
        }
        assert!(
            serde_json::from_str::<SdrRendition>(r#"{"exposure":0,"contrast":0,"headroom":3}"#)
                .is_err()
        );
    }
}
