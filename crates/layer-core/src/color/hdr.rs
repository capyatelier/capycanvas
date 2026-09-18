//! Portable display-referred HDR semantics. Displays never redefine RGB 1.
use super::f16;
use serde::{Deserialize, Serialize};

pub const REFERENCE_WHITE_NITS: f32 = 203.;
pub const MAX_LINEAR: f32 = 65504.;
pub fn bt2020_to_srgb() -> super::rgb::Matrix3 {
    super::rgb::linear_rgb_transform(
        [[0.708, 0.292], [0.170, 0.797], [0.131, 0.046]],
        [0.3127, 0.3290],
        super::RgbSpace::Srgb,
    )
}
pub fn srgb_to_bt2020() -> super::rgb::Matrix3 {
    super::rgb::inverse(bt2020_to_srgb())
}

/// Validate straight RGB before half quantization. Zero-alpha hidden RGB may be
/// retained in source files; edited pixels use canonical transparent black.
pub fn encode_pixel(pixel: [f32; 4]) -> Result<[u16; 4], &'static str> {
    if pixel.iter().any(|v| !v.is_finite()) || !(0. ..=1.).contains(&pixel[3]) {
        return Err("HDR requires finite RGB and coverage between zero and one");
    }
    if pixel[..3].iter().any(|v| v.abs() > MAX_LINEAR) {
        return Err("HDR RGB exceeds the supported half-float range");
    }
    Ok(pixel.map(|v| f16::from_f32(v).to_bits()))
}

pub fn decode_pixel(bits: [u16; 4]) -> Result<[f32; 4], &'static str> {
    let pixel = bits.map(|v| f16::from_bits(v).to_f32());
    encode_pixel(pixel)?;
    Ok(pixel)
}

/// HDR display shoulder, matching `hdr_view.wgsl`. Signed RGB is scaled together
/// and coverage is unchanged. This is a viewing derivative, never editing data.
pub fn map_display_premultiplied(p: [f32; 4], headroom: f32) -> [f32; 4] {
    if p[3] <= 0. { return p; }
    let rgb = [p[0] / p[3], p[1] / p[3], p[2] / p[3]];
    let peak = rgb.into_iter().map(f32::abs).fold(0., f32::max);
    if peak == 0. { return [0., 0., 0., p[3]]; }
    let knee = headroom * 0.75;
    let mapped = if peak <= knee { peak }
        else { headroom - (headroom - knee).powi(2) / (peak + headroom - 2. * knee) };
    let rgb = rgb.map(|v| v / peak * mapped * p[3]);
    [rgb[0], rgb[1], rgb[2], p[3]]
}

/// A deliberate SDR rendition, owned by the document and shared by viewing,
/// proofing and delivery. `knee` retains the persisted highlight-control value;
/// the luminance shoulder and destination gamut compression are viewing derivatives.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SdrRendition {
    pub exposure: f32,
    pub contrast: f32,
    pub knee: f32,
}
impl Default for SdrRendition {
    fn default() -> Self {
        Self {
            exposure: 0.,
            contrast: 1.,
            knee: 0.75,
        }
    }
}
impl SdrRendition {
    /// User-facing highlight adjustment: neutral at the default shoulder;
    /// increasing it retains brighter highlights, decreasing it compresses more.
    pub fn highlights(self) -> f32 {
        ((self.knee - 0.75) / if self.knee < 0.75 { 0.5 } else { 0.2 } * 100.).clamp(-100., 100.)
    }
    pub fn from_appearance(exposure: f32, contrast: f32, highlights: f32) -> Result<Self, &'static str> {
        if !highlights.is_finite() || !(-100. ..=100.).contains(&highlights) { return Err("Highlights must be between -100 and 100"); }
        let result = Self { exposure, contrast, knee: (0.75 + highlights / 100. * if highlights < 0. { 0.5 } else { 0.2 }).clamp(0.25, 0.95) };
        result.validate()?;
        Ok(result)
    }

    pub fn validate(self) -> Result<(), &'static str> {
        if !self.exposure.is_finite()
            || !(-12. ..=12.).contains(&self.exposure)
            || !self.contrast.is_finite()
            || !(0.25..=4.).contains(&self.contrast)
            || !self.knee.is_finite()
            || !(0.25..=0.95).contains(&self.knee)
        {
            return Err("Invalid SDR rendition settings");
        }
        Ok(())
    }
    /// Compile the shared SDR appearance for a source and an RGB destination.
    /// Matrix and luminance work is hoisted out of pixel/row loops.
    pub fn mapper(self, source: super::RgbSpace, destination: super::RgbSpace) -> SdrMapper {
        SdrMapper { recipe: self, source_y: luminance(source), destination_y: luminance(destination),
            matrix: source.linear_transform(destination).map(|r| r.map(|v| v as f32)) }
    }
    pub fn map_rgb(self, rgb: [f32; 3], space: super::RgbSpace) -> [f32; 3] {
        self.mapper(space, space).map_rgb(rgb)
    }
    pub fn map_premultiplied(self, p: [f32; 4], space: super::RgbSpace) -> [f32; 4] {
        self.mapper(space, space).map_premultiplied(p)
    }

}

/// D65-relative luminance: adaptation precedes tone mapping, so equivalent
/// colors in different working spaces receive the same brightness transform.
pub fn luminance(space: super::RgbSpace) -> [f32; 3] {
    let m = space.linear_transform(super::RgbSpace::Srgb);
    let y = super::RgbSpace::Srgb.to_xyz()[1];
    std::array::from_fn(|c| (y[0]*m[0][c] + y[1]*m[1][c] + y[2]*m[2][c]) as f32)
}
pub fn relative_luminance(rgb: [f32; 3], y: [f32; 3]) -> f32 {
    rgb[1] + y[0] * (rgb[0] - rgb[1]) + y[2] * (rgb[2] - rgb[1])
}
/// Smooth compression towards the equal-luminance neutral, in the actual
/// destination gamut. Leaves neutrals and interior colors unchanged; rolls off
/// chroma before the boundary instead of clipping RGB channels independently.
pub fn gamut_map(rgb: [f32; 3], y: [f32; 3]) -> [f32; 3] {
    let light = relative_luminance(rgb, y).clamp(0., 1.);
    if light <= 0. || light >= 1. { return [light; 3]; }
    let mut extent = 0f32;
    for v in rgb {
        extent = extent.max(if v > light { (v-light)/(1.-light) } else { (light-v)/light });
    }
    if extent <= 0.8 { return rgb; }
    let compressed = 1. - 0.04 / (extent - 0.6);
    rgb.map(|v| (light + (v-light) * (compressed/extent)).clamp(0., 1.))
}
#[derive(Clone, Copy)]
pub struct SdrMapper {
    recipe: SdrRendition,
    source_y: [f32; 3],
    destination_y: [f32; 3],
    matrix: [[f32; 3]; 3],
}
impl SdrMapper {
    /// Unbounded, straight source RGB after luminance mapping. Destination gamut
    /// compression belongs after the RGB matrix (or before an ICC proof LUT).
    pub fn tone_rgb(self, rgb: [f32; 3]) -> [f32; 3] {
        let light = relative_luminance(rgb, self.source_y).max(0.);
        if light <= 0. { return [0.; 3]; }
        let r = self.recipe;
        let x = 0.18 * (r.contrast * ((light / 0.18).log2() + r.exposure)).clamp(-126., 120.).exp2();
        // Preserve black and middle gray. Reserve a broad shoulder for highlights
        // rather than spending almost all SDR range below reference white.
        let shape = (r.highlights() * 0.02).exp2();
        let mapped = if x <= 0.18 { x } else {
            0.18 + 0.82 * (1. - (1. + (x - 0.18) / (0.82 * shape)).powf(-shape))
        };
        rgb.map(|v| v / light * mapped)
    }
    pub fn tone_premultiplied(self, p: [f32; 4]) -> [f32; 4] {
        if p[3] <= 0. { return [0.; 4]; }
        let rgb = self.tone_rgb([p[0]/p[3],p[1]/p[3],p[2]/p[3]]);
        [rgb[0]*p[3],rgb[1]*p[3],rgb[2]*p[3],p[3]]
    }
    pub fn map_rgb(self, rgb: [f32; 3]) -> [f32; 3] {
        let rgb = self.tone_rgb(rgb);
        let out = self.matrix.map(|r| r[0]*rgb[0]+r[1]*rgb[1]+r[2]*rgb[2]);
        gamut_map(out, self.destination_y)
    }
    pub fn map_premultiplied(self, p: [f32; 4]) -> [f32; 4] {
        if p[3] <= 0. { return [0.; 4]; }
        let rgb = self.map_rgb([p[0]/p[3],p[1]/p[3],p[2]/p[3]]);
        [rgb[0]*p[3],rgb[1]*p[3],rgb[2]*p[3],p[3]]
    }
}

/// SMPTE ST 2084 (PQ), absolute luminance in cd/m². Float64 is used only at
/// interchange boundaries, not as a separate document processing mode.
pub fn pq_decode(code: f64) -> f64 {
    let p = code.powf(32. / 2523.);
    10000. * ((p - 3424. / 4096.).max(0.) / (2413. / 128. - 2392. / 128. * p)).powf(16384. / 2610.)
}
pub fn pq_encode(nits: f64) -> f64 {
    let p = (nits / 10000.).powf(2610. / 16384.);
    ((3424. / 4096. + 2413. / 128. * p) / (1. + 2392. / 128. * p)).powf(2523. / 32.)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn appearance_highlights_roundtrip_and_direction() {
        for highlights in [-100., -50., 0., 50., 100.] {
            let recipe = SdrRendition::from_appearance(0., 1., highlights).unwrap();
            assert!((recipe.highlights() - highlights).abs() < 0.0001);
            let previous = SdrRendition::from_appearance(0., 1., (highlights - 1.).max(-100.)).unwrap();
            assert!(recipe.map_rgb([4.; 3], super::super::RgbSpace::Srgb)[0] >= previous.map_rgb([4.; 3], super::super::RgbSpace::Srgb)[0]);
        }
        assert_eq!(SdrRendition::from_appearance(0., 1., 0.).unwrap(), SdrRendition::default());
        assert!(SdrRendition::from_appearance(0., 1., f32::NAN).is_err());
    }
    #[test]
    fn every_finite_half_code_round_trips_with_subnormals_and_signed_zero() {
        for bits in 0..=u16::MAX {
            let value = f16::from_bits(bits).to_f32();
            if value.is_finite() {
                assert_eq!(encode_pixel([value, 0., 0., 1.]).unwrap()[0], bits);
            } else {
                assert!(encode_pixel([value, 0., 0., 1.]).is_err());
            }
        }
        assert!(encode_pixel([65505., 0., 0., 1.]).is_err());
        assert!(encode_pixel([1., 0., 0., -0.1]).is_err());
    }
    #[test]
    fn pq_absolute_landmarks_and_every_16_bit_code() {
        assert!((pq_decode(0.508078421517399) - 100.).abs() < 1e-8);
        assert!((pq_decode(0.751827096247041) - 1000.).abs() < 1e-7);
        assert_eq!(pq_decode(1.), 10000.);
        for code in 0..=65535 {
            assert_eq!(
                (pq_encode(pq_decode(code as f64 / 65535.)) * 65535.).round() as u32,
                code
            );
        }
    }
    #[test]
    fn rendition_retains_highlight_separation_and_alpha() {
        use super::super::RgbSpace;
        let r = SdrRendition::default();
        let mapper = r.mapper(RgbSpace::Srgb, RgbSpace::Srgb);
        for (input, expected) in [(0.,0.),(0.18,0.18),(1.,0.59),(4.,0.8550862)] {
            assert!((mapper.map_rgb([input;3])[0]-expected).abs()<1e-6);
        }
        let code = |v| (super::super::srgb_encode(mapper.map_rgb([v;3])[0])*255.).round();
        assert!(code(4.)-code(1.)>=30.);
        assert!(code(16.)-code(4.)>=10.);
        let mut previous=0.;
        for i in 0..=65504 {
            let mapped=mapper.map_rgb([i as f32/16.;3]);
            assert!((previous..=1.).contains(&mapped[0]));
            previous=mapped[0];
        }
        for p in [[4.,1.,0.25],[8.,-0.1,2.],[-0.1,3.,0.5]] {
            let opaque=mapper.map_rgb(p);
            assert!(opaque.into_iter().all(|v|v.is_finite() && (0. ..=1.).contains(&v)));
            for a in [0.00001,0.25,0.5,1.] {
                let out=mapper.map_premultiplied([p[0]*a,p[1]*a,p[2]*a,a]);
                assert_eq!(out[3],a);
                for c in 0..3 {assert!((out[c]/a-opaque[c]).abs()<2e-6);}
            }
        }
        assert_eq!(mapper.map_premultiplied([2.,-1.,0.,0.]),[0.;4]);
    }
    #[test]
    fn equivalent_colors_map_equally_across_working_spaces() {
        use super::super::{RgbSpace, rgb};
        let recipe=SdrRendition::default();
        for output in RgbSpace::ALL {
            for original in [[4.,1.,0.25],[0.18;3],[8.,-0.1,2.],[0.,0.,1.]] {
                let expected=recipe.mapper(RgbSpace::Srgb,output).map_rgb(original);
                for source in RgbSpace::ALL {
                    let input=rgb::apply(RgbSpace::Srgb.linear_transform(source),original.map(f64::from));
                    let actual=recipe.mapper(source,output).map_rgb(input.map(|v|v as f32));
                    for c in 0..3 {assert!((actual[c]-expected[c]).abs()<2e-6,"{source:?} -> {output:?}: {actual:?} != {expected:?}");}
                }
            }
        }
    }
    #[test]
    fn gamut_compression_preserves_luminance_and_interior_colors() {
        use super::super::RgbSpace;
        for space in RgbSpace::ALL {
            let y=luminance(space);
            for input in [[0.3,0.4,0.5],[0.18;3],[2.,0.1,0.1],[-0.2,0.8,0.2]] {
                let light=relative_luminance(input,y);
                let out=gamut_map(input,y);
                assert!((relative_luminance(out,y)-light.clamp(0.,1.)).abs()<2e-6);
                assert!(out.into_iter().all(|v| (0. ..=1.).contains(&v)));
            }
            assert_eq!(gamut_map([0.3,0.4,0.5],y),[0.3,0.4,0.5]);
        }
    }
}
