//! SDR delivery, independent of editing data and the connected monitor.
//! Default: unified luminance rendering with a controllable BT.2390 shoulder
//! and constant-luminance gamut compression. Browser matching retains
//! Skia's reference-white operator (RWTMO) in linear Rec.2020. Its Bezier avoids the
//! reversals/overshoot of the eight-point approximation at extreme HDR ranges.
//! See docs/history/color-management-sdr-proof-update.md and THIRD_PARTY_NOTICES.md.
use super::super::{RgbSpace, rgb::Matrix3};
use serde::{Deserialize, Serialize};

const DEFAULT_HEADROOM: f32 = 2.300_448_4; // log2(1000 cd/m² / 203 cd/m²)

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SdrMethod {
    #[default]
    ToneMap,
    Scale,
    Clip,
    Bt2390,
    Photographic,
    /// Unified shoulder, bounded brightness/contrast and destination color tradeoff.
    Unified,
}

/// An authored SDR rendition. Unified brightness (`exposure`) and contrast
/// operate in output log odds, with a linear 18% contrast pivot and fixed black
/// and white. Older methods retain pre-tone exposure/contrast semantics.
/// Headroom is the measured input white endpoint in stops above RGB 1.
/// These settings never alter the HDR master, its alpha or reference white.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SdrRendition {
    pub exposure: f32,
    pub contrast: f32,
    pub headroom: f32,
    pub method: SdrMethod,
    /// 0 favors highlight luminance/white; 1 favors the original colorfulness.
    /// Used by Photographic and Unified; their saved color policies differ.
    /// Alpha and the HDR master are never affected.
    pub highlight_color: f32,
    /// Unified shoulder bias: -1 retains detail, +1 favors bright highlights.
    pub highlights: f32,
}
impl Default for SdrRendition {
    fn default() -> Self {
        Self {
            exposure: 0.,
            contrast: 1.,
            headroom: DEFAULT_HEADROOM,
            method: SdrMethod::Unified,
            highlight_color: 0.,
            highlights: 0.,
        }
    }
}
impl<'de> Deserialize<'de> for SdrRendition {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Stored {
            exposure: f32,
            contrast: f32,
            #[serde(default)]
            headroom: Option<f32>,
            #[serde(default)]
            method: SdrMethod,
            #[serde(default)]
            highlight_color: f32,
            #[serde(default)]
            highlights: f32,
            // Pre-release documents used a shoulder position, not an HDR range.
            // Preserve its direction around neutral; the new curve intentionally
            // changes the SDR derivative. Never discard invalid legacy data.
            #[serde(default)]
            knee: Option<f32>,
        }
        let s = Stored::deserialize(d)?;
        if s.headroom.is_some() && s.knee.is_some() {
            return Err(serde::de::Error::custom("Conflicting SDR range settings"));
        }
        let headroom = if let Some(knee) = s.knee {
            if !knee.is_finite() || !(0.25..=0.95).contains(&knee) {
                return Err(serde::de::Error::custom("Invalid legacy SDR shoulder"));
            }
            let highlights = (knee - 0.75) / if knee < 0.75 { 0.5 } else { 0.2 };
            DEFAULT_HEADROOM - 2. * highlights
        } else {
            s.headroom.unwrap_or(DEFAULT_HEADROOM)
        };
        let r = Self {
            exposure: s.exposure,
            contrast: s.contrast,
            headroom,
            method: s.method,
            highlight_color: s.highlight_color,
            highlights: s.highlights,
        };
        r.validate().map_err(serde::de::Error::custom)?;
        Ok(r)
    }
}
impl SdrRendition {
    /// Missing document recipes predate Photographic; never migrate on opening.
    pub fn legacy_default() -> Self { Self { method: SdrMethod::Bt2390, ..Self::default() } }
    pub fn validate(self) -> Result<(), &'static str> {
        if !self.exposure.is_finite()
            || !(-12. ..=12.).contains(&self.exposure)
            || !self.contrast.is_finite()
            || !(0.25..=4.).contains(&self.contrast)
            || !self.headroom.is_finite()
            || !(0. ..=16.).contains(&self.headroom)
            || !self.highlights.is_finite()
            || !(-1. ..=1.).contains(&self.highlights)
            || !self.highlight_color.is_finite()
            || !(0. ..=1.).contains(&self.highlight_color)
        {
            return Err("Invalid SDR rendition settings");
        }
        Ok(())
    }
    /// Shared compact parameters for renderer uniforms and preview cache keys.
    /// Method zero is reserved for disabled SDR mapping, not a saved recipe.
    pub fn parameters(self) -> [f32; 8] {
        [
            self.exposure,
            self.contrast,
            self.headroom,
            match self.method {
                SdrMethod::ToneMap => 1.,
                SdrMethod::Scale => 2.,
                SdrMethod::Clip => 3.,
                SdrMethod::Bt2390 => 4.,
                SdrMethod::Photographic => 5.,
                SdrMethod::Unified => 6.,
            },
            self.highlight_color, self.highlights, 0., 0.,
        ]
    }
    pub fn from_parameters(p: [f32; 8]) -> Result<Self, &'static str> {
        let method = match p[3] {
            1. => SdrMethod::ToneMap,
            2. => SdrMethod::Scale,
            3. => SdrMethod::Clip,
            4. => SdrMethod::Bt2390,
            5. => SdrMethod::Photographic,
            6. => SdrMethod::Unified,
            _ => return Err("Invalid SDR mapping method"),
        };
        let r = Self {
            exposure: p[0],
            contrast: p[1],
            headroom: p[2],
            method,
            highlight_color: p[4],
            highlights: p[5],
        };
        r.validate()?;
        Ok(r)
    }
    pub fn uses_gamut_mapping(self) -> bool { matches!(self.method, SdrMethod::Photographic | SdrMethod::Unified) }
    /// Hoist matrices, exposure and spline construction out of pixel loops.
    pub fn mapper(self, source: RgbSpace, destination: RgbSpace) -> SdrMapper {
        SdrMapper {
            recipe: self,
            gain: self.exposure.exp2(),
            peak: self.headroom.exp2(),
            curve: Rwtmo::new(self.headroom),
            perceptual: if self.method == SdrMethod::Unified { Bt2390::with_highlights(self.headroom, self.highlights) } else { Bt2390::new(self.headroom) },
            to_rec2020: to_bt2020(source).map(|r| r.map(|v| v as f32)),
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
    gain: f32,
    peak: f32,
    curve: Rwtmo,
    perceptual: Bt2390,
    to_rec2020: [[f32; 3]; 3],
    to_output: [[f32; 3]; 3],
    output_luma: [f32; 3],
}
impl SdrMapper {
    /// Unbounded straight source RGB after the shared Rec.2020 tone gain:
    /// luminance/peak blend for Photographic, max-RGB for legacy methods.
    /// Destination gamut limiting follows primary conversion (or precedes an
    /// ICC proof LUT with a bounded working-RGB input domain).
    pub fn tone_rgb(self, rgb: [f32; 3]) -> [f32; 3] {
        let rec = apply(self.to_rec2020, rgb);
        let peak = rec.into_iter().fold(0., f32::max);
        if peak <= 0. {
            return [0.; 3];
        }
        if self.recipe.method == SdrMethod::Unified {
            let y = luminance(rec, BT2020_LUMA);
            if y <= 0. { return [0.; 3]; }
            let mapped = self.bounded_adjust(self.perceptual.map(y));
            return rgb.map(|v| v / y * mapped);
        }
        if self.recipe.method == SdrMethod::Photographic {
            let y = luminance(rec, BT2020_LUMA);
            if y <= 0. { return [0.; 3]; }
            let bright = self.perceptual.map(self.adjust(y));
            let colorful = self.perceptual.map(self.adjust(peak)) * (y / peak).min(1.);
            let mapped = bright + self.recipe.highlight_color * (colorful - bright);
            return rgb.map(|v| v / y * mapped);
        }
        let x = self.adjust(peak);
        let mapped = match self.recipe.method {
            SdrMethod::ToneMap => self.curve.map(x),
            SdrMethod::Scale => x / self.peak,
            SdrMethod::Clip => x,
            SdrMethod::Bt2390 => self.perceptual.map(x),
            SdrMethod::Photographic | SdrMethod::Unified => unreachable!(),
        };
        rgb.map(|v| v / peak * mapped)
    }
    // Log-odds contrast pivots at display-linear 18%; brightness shifts the
    // odds in stops. Both leave black and white fixed, unlike pre-tone exposure.
    fn bounded_adjust(self, value: f32) -> f32 {
        if value <= 0. || value >= 1. { return value.clamp(0., 1.); }
        if self.recipe.contrast == 1. { return value * self.gain / (1. - value + value * self.gain); }
        let pivot = (0.18f32 / 0.82).log2();
        let odds = (value / (1. - value)).log2();
        let adjusted = (self.recipe.contrast * (odds - pivot) + pivot + self.recipe.exposure).clamp(-126., 120.).exp2();
        adjusted / (1. + adjusted)
    }
    fn adjust(self, peak: f32) -> f32 {
        if self.recipe.contrast == 1. {
            peak * self.gain
        } else {
            0.18 * (self.recipe.contrast * (peak / 0.18).log2() + self.recipe.exposure)
                .clamp(-126., 120.)
                .exp2()
        }
    }
    pub fn tone_premultiplied(self, p: [f32; 4]) -> [f32; 4] {
        if p[3] <= 0. {
            return [0.; 4];
        }
        let rgb = self.tone_rgb([p[0] / p[3], p[1] / p[3], p[2] / p[3]]);
        [rgb[0] * p[3], rgb[1] * p[3], rgb[2] * p[3], p[3]]
    }
    pub fn map_rgb(self, rgb: [f32; 3]) -> [f32; 3] {
        let rgb = apply(self.to_output, self.tone_rgb(rgb));
        if self.recipe.method == SdrMethod::Unified {
            unified_sdr_gamut(rgb, self.output_luma, self.recipe.highlight_color)
        } else if self.recipe.method == SdrMethod::Photographic {
            compress_sdr_gamut(rgb, self.output_luma)
        } else { rgb.map(|v| v.clamp(0., 1.)) }
    }
    pub fn map_premultiplied(self, p: [f32; 4]) -> [f32; 4] {
        if p[3] <= 0. {
            return [0.; 4];
        }
        let rgb = self.map_rgb([p[0] / p[3], p[1] / p[3], p[2] / p[3]]);
        [rgb[0] * p[3], rgb[1] * p[3], rgb[2] * p[3], p[3]]
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
    std::array::from_fn(|c| (0..3).map(|r| f64::from(BT2020_LUMA[r]) * matrix[r][c]).sum::<f64>() as f32)
}
/// Unified destination-gamut policy. White retains luminance; Color retains
/// RGB ratios where possible, lowering luminance to fit the destination gamut.
/// Negative channels first move toward neutral, never clip independently.
pub fn unified_sdr_gamut(rgb: [f32; 3], weights: [f32; 3], color: f32) -> [f32; 3] {
    let white = compress_sdr_gamut(rgb, weights);
    if color == 0. { return white; }
    let y = luminance(rgb, weights);
    if y <= 0. { return [0.; 3]; }
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
    if y <= 0. { return [0.; 3]; }
    if y >= 1. { return [1.; 3]; }
    let chroma = rgb.map(|v| v - y);
    let extent = chroma.into_iter().map(|v| if v > 0. { v / (1. - y) } else { -v / y }).fold(0., f32::max);
    if extent <= 0.98 { return rgb; }
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
struct Bt2390 { peak: f32, pq_peak: f32, output: f32, knee: f32 }
fn pq_encode(nits: f32) -> f32 {
    let p = (nits / 10000.).powf(2610. / 16384.);
    ((3424. / 4096. + 2413. / 128. * p) / (1. + 2392. / 128. * p)).powf(2523. / 32.)
}
fn pq_decode(code: f32) -> f32 {
    let p = code.powf(32. / 2523.);
    10000. * ((p - 3424. / 4096.).max(0.) / (2413. / 128. - 2392. / 128. * p)).powf(16384. / 2610.)
}
impl Bt2390 {
    fn new(headroom: f32) -> Self {
        let peak = headroom.exp2();
        let pq_peak = pq_encode(peak * super::REFERENCE_WHITE_NITS);
        let output = pq_encode(super::REFERENCE_WHITE_NITS) / pq_peak;
        Self { peak, pq_peak, output, knee: (2. * output - 1.).max(0.) }
    }
    fn with_highlights(headroom: f32, highlights: f32) -> Self {
        let mut curve = Self::new(headroom);
        // Offset >= .5 keeps this Hermite shoulder monotonic (no overshoot).
        if highlights != 0. { curve.knee = (curve.output - (-highlights).exp2() * (1. - curve.output)).max(0.); }
        curve
    }
    fn map(self, x: f32) -> f32 {
        if x <= 0. { return 0.; }
        if self.peak == 1. || x >= self.peak { return x.min(1.); }
        let q = pq_encode(x * super::REFERENCE_WHITE_NITS) / self.pq_peak;
        if q <= self.knee { return x; }
        let t = (q - self.knee) / (1. - self.knee);
        let t2 = t * t;
        let t3 = t2 * t;
        let q = (2. * t3 - 3. * t2 + 1.) * self.knee
            + (t3 - 2. * t2 + t) * (1. - self.knee)
            + (-2. * t3 + 3. * t2) * self.output;
        (pq_decode(q * self.pq_peak) / super::REFERENCE_WHITE_NITS).clamp(0., 1.)
    }
}

/// RWTMO Bezier construction from Skia's PopulateUsingRwtmo, Copyright 2025
/// Google LLC, BSD-3-Clause. Full notice in THIRD_PARTY_NOTICES.md.
/// Evaluate the underlying monotonic curve directly instead of approximating
/// log gain with eight points: the approximation overshoots at high headroom.
#[derive(Clone, Copy)]
struct Rwtmo {
    peak: f32,
    white: f32,
    a: [f32; 2],
    b: [f32; 2],
}
impl Rwtmo {
    fn new(headroom: f32) -> Self {
        let peak = headroom.exp2();
        let white = 1. - 0.5 * (headroom / DEFAULT_HEADROOM).min(1.);
        let x_mid = 0.35 + 0.65 / white;
        let y_mid = 0.35 * white + 0.65;
        Self {
            peak,
            white,
            a: [1. - 2. * x_mid + peak, white - 2. * y_mid + 1.],
            b: [2. * x_mid - 2., 2. * y_mid - 2. * white],
        }
    }
    fn map(self, x: f32) -> f32 {
        if self.peak == 1. {
            return x.min(1.);
        }
        if x <= 1. {
            return x * self.white;
        }
        if x >= self.peak {
            return 1.;
        }
        // Stable quadratic inversion, including a.x=0. The alternative
        // (-b+sqrt(...))/(2a) suffers cancellation near reference white.
        let t = 2. * (x - 1.)
            / (self.b[0]
                + (self.b[0] * self.b[0] + 4. * self.a[0] * (x - 1.))
                    .max(0.)
                    .sqrt());
        self.white + t * (self.b[1] + t * self.a[1])
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::rgb;
    use super::*;

    #[test]
    fn perceptual_shoulder_matches_itu_float64_reference_and_retains_midtones() {
        // Independent Float64 evaluation of the report's E1/E2 and Hermite
        // equations. Tolerance includes Float32 PQ powers and inverse powers.
        let reference=|headroom:f64,x:f64|{
            if headroom==0. {return x.min(1.);}
            if x>=headroom.exp2(){return 1.;}
            let pq=super::super::pq_encode;
            let span=pq(203.*headroom.exp2());let white=pq(203.)/span;
            let knee=(2.*white-1.).max(0.);let input=pq(x*203.)/span;
            if input<=knee{return x;}
            let t=(input-knee)/(1.-knee);
            let h=[2.*t.powi(3)-3.*t*t+1.,t.powi(3)-2.*t*t+t,-2.*t.powi(3)+3.*t*t];
            super::super::pq_decode((h[0]*knee+h[1]*(1.-knee)+h[2]*white)*span)/203.
        };
        for h in [0.,0.0001,0.25,1.,2.3004484,4.,8.,16.] {
            let curve=Bt2390::new(h);let mut previous=0.;
            for i in 0..2048{
                let x=(-16f32+(h+18.)*i as f32/2047.).exp2();let y=curve.map(x);
                assert!((y as f64-reference(h as f64,x as f64)).abs()<0.00015,"{h} {x} {y}");
                assert!(y>=previous-0.0001&&y.is_finite()&&y<=1.);previous=y;
            }
        }
        let r=SdrRendition::legacy_default();assert_eq!(r.method,SdrMethod::Bt2390);
        let m=|r:SdrRendition,x|r.map_rgb([x;3],RgbSpace::Srgb)[0];
        assert!((m(r,0.18)-0.18).abs()<0.00002);
        assert!((m(r,1.)-0.661213).abs()<0.00015);
        assert!(m(SdrRendition{exposure:1.,..r},0.18)>m(r,0.18)*1.8);
        assert!(m(SdrRendition{headroom:4.,..r},4.)<m(r,4.)-0.1);
        assert!(m(SdrRendition{contrast:1.5,..r},0.02)<m(r,0.02)*0.5);
    }

    #[test]
    fn browser_reference_sweep() {
        // The pinned Skia C++ functions generate all 1,542 samples. At normal
        // 203–1000 nit headroom, the analytic curve is within 0.0006 linear
        // of the browser approximation. Above that its eight-point spline can
        // reverse or exceed white (67.998 at 16 stops), so is not our oracle.
        // Analytic accuracy over the full range is verified separately below.
        for line in include_str!("reference/skia-rwtmo.csv").lines() {
            let v = line
                .split(',')
                .map(|v| v.parse::<f32>().unwrap())
                .collect::<Vec<_>>();
            if v[0] > DEFAULT_HEADROOM {
                continue;
            }
            let actual = Rwtmo::new(v[0]).map(v[1]);
            assert!((actual - v[2]).abs() < 0.0006, "{line}: {actual}");
        }
        let curve = Rwtmo::new(DEFAULT_HEADROOM);
        assert_eq!(curve.map(0.), 0.);
        assert_eq!(curve.map(1.), 0.5);
        assert_eq!(curve.map(1000. / 203.), 1.);
    }
    #[test]
    fn analytic_curve_matches_float64_parametric_bezier() {
        for h in [0.01, 0.25, 1., 2.300448367476911, 4., 8., 16.] {
            let peak = 2f64.powf(h);
            let w = 1. - 0.5 * (h / (1000f64 / 203.).log2()).min(1.);
            let mid = [0.35 + 0.65 / w, 0.35 * w + 0.65];
            for i in 0..=1000 {
                let t = i as f64 / 1000.;
                let x = (1. - t).powi(2) + 2. * (1. - t) * t * mid[0] + t * t * peak;
                let y = (1. - t).powi(2) * w + 2. * (1. - t) * t * mid[1] + t * t;
                let actual = Rwtmo::new(h as f32).map(x as f32);
                assert!((actual as f64 - y).abs() < 2e-6, "{h} {x} {actual} != {y}");
            }
        }
    }
    #[test]
    fn continuous_monotonic_mapping_including_near_sdr_range() {
        for headroom in [0., 0.00001, 0.01, 0.25, 1., DEFAULT_HEADROOM, 4., 8., 16.] {
            let curve = Rwtmo::new(headroom);
            let mut previous = 0.;
            for i in 0..=16384 {
                let x = (-16. + i as f32 * (headroom + 18.) / 16384.).exp2();
                let y = curve.map(x);
                assert!(
                    y.is_finite() && y >= previous - 2e-6 && y <= 1.000002,
                    "{headroom} {x} {y} {previous}"
                );
                previous = y;
            }
        }
    }
    #[test]
    fn knobs_change_distinct_parts_of_the_rendition() {
        let r = SdrRendition { method: SdrMethod::ToneMap, ..Default::default() };
        let m = |r: SdrRendition, x| r.map_rgb([x; 3], RgbSpace::Srgb)[0];
        // Exposure raises shadows and highlights; range only changes highlights
        // once the source endpoint is above the 1000-nit default.
        assert!(m(SdrRendition { exposure: 1., ..r }, 0.18) > m(r, 0.18) * 1.99);
        let wide = SdrRendition { headroom: 4., ..r };
        assert_eq!(m(wide, 0.18), m(r, 0.18));
        assert!(m(wide, 4.) < m(r, 4.) - 0.15);
        let contrast = SdrRendition { contrast: 2., ..r };
        assert!((m(contrast, 0.18) - m(r, 0.18)).abs() < 1e-6);
        assert!(m(contrast, 0.02) < m(r, 0.02) * 0.2);
        assert!(m(contrast, 1.) > m(r, 1.) + 0.4);
        let scale = SdrRendition {
            method: SdrMethod::Scale,
            headroom: 2.,
            ..r
        };
        assert!((m(scale, 1.) - 0.25).abs() < 1e-6);
        assert!((m(scale, 4.) - 1.).abs() < 1e-6);
        let clip = SdrRendition {
            method: SdrMethod::Clip,
            ..r
        };
        assert!((m(clip, 0.18) - 0.18).abs() < 1e-6);
        assert_eq!(m(clip, 4.), 1.);
    }
    #[test]
    fn no_desaturation_before_output_gamut_and_alpha_is_coverage() {
        for method in [SdrMethod::ToneMap, SdrMethod::Scale, SdrMethod::Clip, SdrMethod::Bt2390] {
            let r = SdrRendition {
                method,
                ..Default::default()
            };
            let mapper = r.mapper(RgbSpace::Srgb, RgbSpace::Srgb);
            for rgb in [
                [4., 1., 0.25],
                [8., -0.1, 2.],
                [-0.1, 3., 0.5],
                [65504., 2., 1.],
                [0.00000006; 3],
            ] {
                let tone = mapper.tone_rgb(rgb);
                assert!((tone[0] / rgb[0] - tone[1] / rgb[1]).abs() <= 2e-6);
                let opaque = mapper.map_rgb(rgb);
                for a in [0.00001, 0.25, 0.5, 1.] {
                    let p = mapper.map_premultiplied([rgb[0] * a, rgb[1] * a, rgb[2] * a, a]);
                    assert_eq!(p[3], a);
                    for c in 0..3 {
                        assert!((p[c] / a - opaque[c]).abs() < 2e-6);
                    }
                }
            }
            assert_eq!(mapper.map_premultiplied([2., -1., 0., 0.]), [0.; 4]);
        }
    }
    #[test]
    fn equivalent_colors_across_working_and_delivery_primaries() {
        for method in [SdrMethod::ToneMap, SdrMethod::Scale, SdrMethod::Clip, SdrMethod::Bt2390] {
            let r = SdrRendition {
                method,
                ..Default::default()
            };
            for output in RgbSpace::ALL {
                for original in [[4., 1., 0.25], [0.18; 3], [8., -0.1, 2.], [0., 0., 1.]] {
                    let expected = r.mapper(RgbSpace::Srgb, output).map_rgb(original);
                    for source in RgbSpace::ALL {
                        let input = rgb::apply(
                            RgbSpace::Srgb.linear_transform(source),
                            original.map(f64::from),
                        );
                        let actual = r.mapper(source, output).map_rgb(input.map(|v| v as f32));
                        for c in 0..3 {
                            assert!(
                                (actual[c] - expected[c]).abs() < 3e-6,
                                "{source:?} -> {output:?}: {actual:?} != {expected:?}"
                            );
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn saved_recipes_and_previous_review_settings_validate() {
        for method in [SdrMethod::ToneMap, SdrMethod::Scale, SdrMethod::Clip, SdrMethod::Bt2390, SdrMethod::Photographic] {
            let r = SdrRendition {
                method,
                exposure: -1.,
                headroom: 4.,
                contrast: 1.5,
                highlight_color: 0.37,
                highlights: 0.,
            };
            assert_eq!(SdrRendition::from_parameters(r.parameters()).unwrap(), r);
            let json = serde_json::to_string(&r).unwrap();
            assert_eq!(serde_json::from_str::<SdrRendition>(&json).unwrap(), r);
            assert!(!json.contains("knee"));
        }
        let legacy = |knee| {
            serde_json::from_value::<SdrRendition>(
                serde_json::json!({"exposure":0.,"contrast":1.,"knee":knee}),
            )
        };
        assert_eq!(legacy(0.75).unwrap(), SdrRendition { method: SdrMethod::ToneMap, ..Default::default() });
        assert!(legacy(0.25).unwrap().headroom > DEFAULT_HEADROOM);
        assert!(legacy(0.95).unwrap().headroom < DEFAULT_HEADROOM);
        assert!(legacy(0.).is_err());
        assert!(
            SdrRendition {
                headroom: f32::NAN,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            SdrRendition {
                headroom: 17.,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn unified_controls_preserve_endpoints_and_have_separate_effects() {
        let r = SdrRendition::default();
        let gray = |recipe: SdrRendition, x| recipe.map_rgb([x; 3], RgbSpace::Srgb)[0];
        for exposure in [-12., -2., 0., 2., 12.] {
            for contrast in [0.25, 1., 4.] {
                for highlights in [-1., 0., 1.] {
                    let recipe = SdrRendition { exposure, contrast, highlights, ..r };
                    assert_eq!(gray(recipe, 0.), 0.);
                    assert!((gray(recipe, r.headroom.exp2() * 1.001) - 1.).abs() < 1e-6);
                    let mut previous = 0.;
                    for i in 0..512 {
                        let x = (-16. + 20. * i as f32 / 511.).exp2();
                        let y = gray(recipe, x);
                        assert!(y.is_finite() && (0. ..=1.).contains(&y));
                        assert!(y >= previous - 0.00015, "tone reversal {recipe:?} {x} {y}");
                        previous = y;
                    }
                    assert_eq!(SdrRendition::from_parameters(recipe.parameters()).unwrap(), recipe);
                    assert_eq!(serde_json::from_str::<SdrRendition>(&serde_json::to_string(&recipe).unwrap()).unwrap(), recipe);
                }
            }
        }
        assert!(gray(SdrRendition { exposure: 1., ..r }, 0.18) > gray(r, 0.18) + 0.1);
        assert!((gray(SdrRendition { contrast: 2., ..r }, 0.18) - gray(r, 0.18)).abs() < 0.0001);
        assert!(gray(SdrRendition { contrast: 2., ..r }, 0.05) < gray(r, 0.05));
        assert!(gray(SdrRendition { highlights: 1., ..r }, 1.) > gray(SdrRendition { highlights: -1., ..r }, 1.) + 0.1);
        let red = SdrRendition { highlight_color: 1., ..r }.map_rgb([16., 0., 0.], RgbSpace::Srgb);
        assert!(red[0] > 0.999 && red[1] < 1e-6 && red[2] < 1e-6);
        let white = r.map_rgb([16., 0., 0.], RgbSpace::Srgb);
        assert!(white[1] > 0.8 && white[2] > 0.8);
        for highlights in [-1.001, 1.001, f32::NAN, f32::INFINITY] {
            assert!(SdrRendition { highlights, ..r }.validate().is_err());
        }
    }

    #[test]
    fn unified_shoulder_matches_float64_reference_at_control_extremes() {
        for headroom in [0.0001, 0.25, 2.3004484, 8., 16.] {
            for highlights in [-1., -0.3, 0., 0.5, 1.] {
                let curve = Bt2390::with_highlights(headroom, highlights);
                let peak = f64::from(headroom).exp2();
                let span = super::super::pq_encode(203. * peak);
                let output = super::super::pq_encode(203.) / span;
                let knee = (output - (-f64::from(highlights)).exp2() * (1. - output)).max(0.);
                let mut previous = 0.;
                for i in 0..1024 {
                    let x = (-16. + (f64::from(headroom) + 17.) * f64::from(i) / 1023.).exp2();
                    let q = super::super::pq_encode(x * 203.) / span;
                    let expected = if x >= peak { 1. } else if q <= knee { x } else {
                        let t = (q - knee) / (1. - knee);
                        let mapped = (2.*t*t*t-3.*t*t+1.)*knee + (t*t*t-2.*t*t+t)*(1.-knee) + (-2.*t*t*t+3.*t*t)*output;
                        super::super::pq_decode(mapped*span)/203.
                    };
                    let actual = curve.map(x as f32);
                    assert!((f64::from(actual) - expected).abs() < 0.00016, "{headroom} {highlights} {x}: {actual} {expected}");
                    assert!(actual >= previous - 0.00015);
                    previous = actual;
                }
            }
        }
    }

    #[test]
    fn loading_old_recipes_does_not_change_their_rendering() {
        for method in ["tone_map", "scale", "clip", "bt2390", "photographic"] {
            let old = serde_json::json!({"method":method,"exposure":-0.7,"contrast":1.2,"headroom":3.,"highlight_color":0.6});
            let restored: SdrRendition = serde_json::from_value(old).unwrap();
            assert_ne!(restored.method, SdrMethod::Unified);
            assert_eq!(restored.highlights, 0.);
            // New shoulder control is ignored by every retained legacy method.
            for rgb in [[0.18;3], [8.,2.,0.1], [-0.1,1.,4.]] {
                assert_eq!(restored.map_rgb(rgb, RgbSpace::Srgb), SdrRendition { highlights: 1., ..restored }.map_rgb(rgb, RgbSpace::Srgb));
            }
        }
    }

    #[test]
    fn photographic_highlights_keep_luminance_hue_direction_and_coverage() {
        let weights = sdr_luminance_weights(RgbSpace::Srgb);
        let recipe = SdrRendition { method: SdrMethod::Photographic, ..Default::default() };
        let mapper = recipe.mapper(RgbSpace::Srgb, RgbSpace::Srgb);
        for color in [[1.,0.,0.], [0.,1.,0.], [0.,0.,1.], [1.,0.3,0.02], [0.65,0.32,0.2], [-0.1,0.5,1.], [0.18;3]] {
            let mut previous = 0.;
            for i in 0..256 {
                let ev = -12. + i as f32 * 24. / 255.;
                let input = color.map(|v| v * ev.exp2());
                let output = mapper.map_rgb(input);
                let y = luminance(output, weights);
                assert!(output.iter().all(|v| v.is_finite() && (0. ..=1.).contains(v)));
                assert!(y >= previous - 0.00015, "lightness reversal: {color:?} {ev}: {y} < {previous}");
                previous = y;
                let tone = mapper.tone_rgb(input);
                let expected_y = luminance(tone, weights).clamp(0., 1.);
                assert!((y - expected_y).abs() < 2e-6, "gamut mapping must preserve luminance");
                let chroma = tone.map(|v| v - expected_y);
                let mapped = output.map(|v| v - y);
                let cross = [chroma[0]*mapped[1]-chroma[1]*mapped[0],chroma[1]*mapped[2]-chroma[2]*mapped[1]];
                assert!(cross.iter().all(|v| v.abs() < 2e-5), "RGB hue direction changed");
                let a = 0.03125;
                let covered = mapper.map_premultiplied([input[0]*a,input[1]*a,input[2]*a,a]);
                assert_eq!(covered[3], a);
                for c in 0..3 { assert!((covered[c]/a-output[c]).abs()<2e-6); }
            }
            assert!(mapper.map_rgb(color.map(|v|v*4096.)).iter().all(|v|*v>0.999));
        }
        let bright = mapper.map_rgb([8.,0.,0.]);
        let old = SdrRendition::legacy_default().map_rgb([8.,0.,0.],RgbSpace::Srgb);
        assert!(luminance(bright,weights) > luminance(old,weights)*3.);
        assert!(bright[1] > 0.6 && bright[2] > 0.6);
        let color = SdrRendition { highlight_color: 1., ..recipe }.map_rgb([8.,0.,0.],RgbSpace::Srgb);
        assert!(color[0]-color[1] > bright[0]-bright[1] + 0.4);
        for x in [0.01,0.18,1.,4.,16.] {
            assert_eq!(recipe.map_rgb([x;3],RgbSpace::Srgb),SdrRendition {highlight_color:1.,..recipe}.map_rgb([x;3],RgbSpace::Srgb));
        }
    }

    #[test]
    fn photographic_working_space_invariance_and_invalid_settings() {
        for output in RgbSpace::ALL {
            for highlight_color in [0.,0.3,1.] {
                let recipe = SdrRendition {highlight_color,..Default::default()};
                for rgb in [[4.,1.,0.25],[8.,0.,0.],[0.,0.,16.],[-0.1,3.,0.5],[0.18;3]] {
                    let expected = recipe.mapper(RgbSpace::Srgb,output).map_rgb(rgb);
                    for space in RgbSpace::ALL {
                        let input=rgb::apply(RgbSpace::Srgb.linear_transform(space),rgb.map(f64::from)).map(|v|v as f32);
                        let actual=recipe.mapper(space,output).map_rgb(input);
                        for c in 0..3 {assert!((actual[c]-expected[c]).abs()<0.00015,"{space:?} {output:?}: {actual:?} != {expected:?}");}
                    }
                }
            }
        }
        for bad in [-0.001,1.001,f32::NAN,f32::INFINITY] {
            assert!(SdrRendition {highlight_color:bad,..Default::default()}.validate().is_err());
        }
        let mut document=serde_json::to_value(crate::Document::new("legacy",8,8)).unwrap();
        document.as_object_mut().unwrap().remove("sdr_rendition");
        let document:crate::Document=serde_json::from_value(document).unwrap();
        assert_eq!(document.sdr_rendition,SdrRendition::legacy_default());
        for (stored, expected) in [("bt2390",SdrMethod::Bt2390),("tone_map",SdrMethod::ToneMap)] {
            let r:SdrRendition=serde_json::from_value(serde_json::json!({"exposure":0.,"contrast":1.,"headroom":2.3,"method":stored})).unwrap();
            assert_eq!(r.method,expected);
            assert_eq!(r.highlight_color,0.);
        }
    }
}
