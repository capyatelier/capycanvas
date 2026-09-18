//! SDR delivery, independent of editing data and the connected monitor.
//! The default is Skia's reference-white tone mapping operator (RWTMO),
//! evaluated analytically in linear Rec.2020. The analytic Bezier avoids the
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
}

/// An authored SDR rendition. Exposure is in stops, contrast pivots around
/// linear 18%, and headroom is the input white endpoint in stops above RGB 1.
/// These settings never alter the HDR master, its alpha or reference white.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SdrRendition {
    pub exposure: f32,
    pub contrast: f32,
    pub headroom: f32,
    pub method: SdrMethod,
}
impl Default for SdrRendition {
    fn default() -> Self {
        Self {
            exposure: 0.,
            contrast: 1.,
            headroom: DEFAULT_HEADROOM,
            method: SdrMethod::ToneMap,
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
            || !(0.25..=4.).contains(&self.contrast)
            || !self.headroom.is_finite()
            || !(0. ..=16.).contains(&self.headroom)
        {
            return Err("Invalid SDR rendition settings");
        }
        Ok(())
    }
    /// Shared compact parameters for renderer uniforms and preview cache keys.
    /// Method zero is reserved for disabled SDR mapping, not a saved recipe.
    pub fn parameters(self) -> [f32; 4] {
        [
            self.exposure,
            self.contrast,
            self.headroom,
            match self.method {
                SdrMethod::ToneMap => 1.,
                SdrMethod::Scale => 2.,
                SdrMethod::Clip => 3.,
            },
        ]
    }
    pub fn from_parameters(p: [f32; 4]) -> Result<Self, &'static str> {
        let method = match p[3] {
            1. => SdrMethod::ToneMap,
            2. => SdrMethod::Scale,
            3. => SdrMethod::Clip,
            _ => return Err("Invalid SDR mapping method"),
        };
        let r = Self {
            exposure: p[0],
            contrast: p[1],
            headroom: p[2],
            method,
        };
        r.validate()?;
        Ok(r)
    }
    /// Hoist matrices, exposure and spline construction out of pixel loops.
    pub fn mapper(self, source: RgbSpace, destination: RgbSpace) -> SdrMapper {
        SdrMapper {
            recipe: self,
            gain: self.exposure.exp2(),
            peak: self.headroom.exp2(),
            curve: Rwtmo::new(self.headroom),
            to_rec2020: to_bt2020(source).map(|r| r.map(|v| v as f32)),
            to_output: source
                .linear_transform(destination)
                .map(|r| r.map(|v| v as f32)),
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
    to_rec2020: [[f32; 3]; 3],
    to_output: [[f32; 3]; 3],
}
impl SdrMapper {
    /// Unbounded straight source RGB after the common Rec.2020 max-RGB gain.
    /// Destination gamut limiting follows primary conversion (or precedes an
    /// ICC proof LUT with a bounded working-RGB input domain).
    pub fn tone_rgb(self, rgb: [f32; 3]) -> [f32; 3] {
        let peak = apply(self.to_rec2020, rgb).into_iter().fold(0., f32::max);
        if peak <= 0. {
            return [0.; 3];
        }
        let x = if self.recipe.contrast == 1. {
            peak * self.gain
        } else {
            0.18 * (self.recipe.contrast * (peak / 0.18).log2() + self.recipe.exposure)
                .clamp(-126., 120.)
                .exp2()
        };
        let mapped = match self.recipe.method {
            SdrMethod::ToneMap => self.curve.map(x),
            SdrMethod::Scale => x / self.peak,
            SdrMethod::Clip => x,
        };
        rgb.map(|v| v / peak * mapped)
    }
    pub fn tone_premultiplied(self, p: [f32; 4]) -> [f32; 4] {
        if p[3] <= 0. {
            return [0.; 4];
        }
        let rgb = self.tone_rgb([p[0] / p[3], p[1] / p[3], p[2] / p[3]]);
        [rgb[0] * p[3], rgb[1] * p[3], rgb[2] * p[3], p[3]]
    }
    pub fn map_rgb(self, rgb: [f32; 3]) -> [f32; 3] {
        apply(self.to_output, self.tone_rgb(rgb)).map(|v| v.clamp(0., 1.))
    }
    pub fn map_premultiplied(self, p: [f32; 4]) -> [f32; 4] {
        if p[3] <= 0. {
            return [0.; 4];
        }
        let rgb = self.map_rgb([p[0] / p[3], p[1] / p[3], p[2] / p[3]]);
        [rgb[0] * p[3], rgb[1] * p[3], rgb[2] * p[3], p[3]]
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
        let r = SdrRendition::default();
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
        for method in [SdrMethod::ToneMap, SdrMethod::Scale, SdrMethod::Clip] {
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
        for method in [SdrMethod::ToneMap, SdrMethod::Scale, SdrMethod::Clip] {
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
        for method in [SdrMethod::ToneMap, SdrMethod::Scale, SdrMethod::Clip] {
            let r = SdrRendition {
                method,
                exposure: -1.,
                headroom: 4.,
                contrast: 1.5,
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
        assert_eq!(legacy(0.75).unwrap(), SdrRendition::default());
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
}
