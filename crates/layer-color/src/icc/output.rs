//! Worker-owned encoding of bounded linear working rows for SDR delivery.
use super::*;
use layer_core::color::source::{SourceChannels, SourceInterpretation};
use layer_core::color::{OutputDither, OutputEncoding};

mod quantize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutputStatistics {
    /// Channels outside the integer destination by more than half a code.
    pub clipped_channels: u64,
}

pub struct WorkingEncoder {
    destination: SourceInterpretation,
    kind: OutputKind,
    dither: OutputDither,
    sdr_highlight: Option<f32>,
    source_luma: [f32; 3],
    output_luma: [f32; 3],
}
enum OutputKind {
    Builtin {
        space: RgbSpace,
        matrix: layer_core::color::rgb::Matrix3,
        identity: bool,
    },
    Rgb(FloatTransform<4>),
    Gray(CompiledTransform<4, 1>),
    Cmyk(FloatTransform<4>),
}
impl WorkingEncoder {
    pub fn new(
        source: RgbSpace,
        destination: &SourceInterpretation,
        encoding: OutputEncoding,
    ) -> Result<Self, String> {
        encoding.validate(destination.depth)?;
        let options = encoding.conversion;
        validate_options(options)?;
        let mut destination = destination.clone();
        destination.profile_assumed = false;
        if destination.depth.is_float() && (!matches!(destination.profile, ColorProfile::Builtin(_)) || !matches!(destination.channels, SourceChannels::Rgb | SourceChannels::Rgba)) { return Err("HDR output requires linear RGB primaries".into()); }
        if matches!(
            destination.channels,
            SourceChannels::Gray | SourceChannels::GrayAlpha
        ) && let ColorProfile::Builtin(space) = destination.profile
        {
            destination.profile = gray_profile(space)?;
        }
        let output = open(&destination.profile)?;
        let actual = channels(&output)?;
        let valid = match destination.channels {
            SourceChannels::Rgb | SourceChannels::Rgba => actual == ProfileChannels::Rgb,
            SourceChannels::Gray | SourceChannels::GrayAlpha => actual == ProfileChannels::Gray,
            SourceChannels::Cmyk => actual == ProfileChannels::Cmyk,
        };
        if !valid {
            return Err("Output channels disagree with the destination profile".into());
        }
        let input = linear_profile(source)?;
        let kind = match &destination.profile {
            ColorProfile::Builtin(space)
                if destination.depth.is_float()
                    || options.intent != RenderingIntent::AbsoluteColorimetric
                    || source.white() == space.white() =>
            {
                OutputKind::Builtin {
                    space: *space,
                    matrix: if destination.depth.is_float() && options.intent == RenderingIntent::AbsoluteColorimetric {
                        source.absolute_linear_transform(*space)
                    } else { source.linear_transform(*space) },
                    identity: source == *space,
                }
            }
            _ => match actual {
                ProfileChannels::Rgb => {
                    OutputKind::Rgb(CompiledTransform::new(&input, &output, options)?)
                }
                ProfileChannels::Gray => {
                    OutputKind::Gray(CompiledTransform::new(&input, &output, options)?)
                }
                ProfileChannels::Cmyk => {
                    OutputKind::Cmyk(CompiledTransform::new(&input, &output, options)?)
                }
            },
        };
        Ok(Self {
            source_luma: layer_core::color::hdr::sdr_luminance_weights(source),
            output_luma: layer_core::color::hdr::sdr_luminance_weights(match destination.profile { ColorProfile::Builtin(space) => space, _ => source }),
            destination,
            kind,
            dither: encoding.dither,
            sdr_highlight: None,
        })
    }

    /// Compress HDR rendition color before alpha/matte compositing. Builtin RGB
    /// uses the destination gamut; ICC delivery shares the proof LUT input gamut.
    pub fn with_sdr_gamut(mut self, rendition: Option<layer_core::color::hdr::SdrRendition>) -> Self {
        self.sdr_highlight = rendition.map(|r| r.highlight_color);
        self
    }

    /// Use this interpretation for the written file, including any generated
    /// gray ICC definition. Merely attaching an RGB profile to gray is invalid.
    pub fn interpretation(&self) -> &SourceInterpretation {
        &self.destination
    }

    /// Preserve straight hidden RGB if supplied. Opaque output with any coverage
    /// below one requires an explicit matte in the linear working RGB space.
    /// `origin` names the first pixel of this output row, keeping dithering
    /// stable when the row is split across independently encoded chunks.
    pub fn encode_straight(
        &self,
        input: &[[f32; 4]],
        output: &mut [u8],
        matte: Option<[f32; 3]>,
        origin: [u32; 2],
    ) -> Result<OutputStatistics, String> {
        self.encode(input, output, matte, false, origin)
    }
    /// Linear premultiplied artwork; zero coverage becomes transparent black.
    /// Matte compositing precedes nonlinear/profile conversion and quantization.
    pub fn encode_premultiplied(
        &self,
        input: &[[f32; 4]],
        output: &mut [u8],
        matte: Option<[f32; 3]>,
        origin: [u32; 2],
    ) -> Result<OutputStatistics, String> {
        self.encode(input, output, matte, true, origin)
    }
    fn encode(
        &self,
        input: &[[f32; 4]],
        output: &mut [u8],
        matte: Option<[f32; 3]>,
        premultiplied: bool,
        origin: [u32; 2],
    ) -> Result<OutputStatistics, String> {
        let destination = &self.destination;
        let bpp = destination.pixel_bytes();
        if input.len().checked_mul(bpp) != Some(output.len()) {
            return Err("Incomplete output row".into());
        }
        if input
            .iter()
            .any(|p| p.iter().any(|v| !v.is_finite()) || !(0.0..=1.0).contains(&p[3]))
            || matte.is_some_and(|m| m.iter().any(|v| !v.is_finite()))
        {
            return Err("Output requires finite linear RGB and valid coverage".into());
        }
        if matte.is_none() && !destination.channels.has_alpha() && input.iter().any(|p| p[3] < 1.) {
            return Err("Opaque output requires an explicit matte for transparency".into());
        }
        let matte = matte.map(|m| {
            if self.sdr_highlight.is_some() && let OutputKind::Builtin { matrix, .. } = &self.kind {
                layer_core::color::rgb::apply(*matrix, m.map(f64::from)).map(|v| v as f32)
            } else { m }
        });
        let maximum = if destination.depth.is_float() { 1. } else { f64::from(destination.depth.maximum()) };
        let step = destination.depth.bytes();
        let mut statistics = OutputStatistics::default();
        for (chunk, (input, output)) in input
            .chunks(256)
            .zip(output.chunks_mut(256 * bpp))
            .enumerate()
        {
            let mut values = [[0f32; 4]; 256];
            for (i, &p) in input.iter().enumerate() {
                let rgb = if premultiplied {
                    if p[3] > 0. {
                        [p[0] / p[3], p[1] / p[3], p[2] / p[3]]
                    } else {
                        [0.; 3]
                    }
                } else {
                    [p[0], p[1], p[2]]
                };
                let rgb = if let Some(color) = self.sdr_highlight {
                    let (rgb, weights) = match &self.kind {
                        OutputKind::Builtin { matrix, .. } => (
                            layer_core::color::rgb::apply(*matrix, rgb.map(f64::from)).map(|v| v as f32), self.output_luma),
                        _ => (rgb, self.source_luma),
                    };
                    layer_core::color::hdr::unified_sdr_gamut(rgb, weights, color)
                } else { rgb };
                let (rgb, alpha) = if let Some(matte) = matte {
                    (
                        std::array::from_fn(|c| rgb[c] * p[3] + matte[c] * (1. - p[3])),
                        1.,
                    )
                } else {
                    (rgb, p[3])
                };
                if rgb.iter().any(|v| !v.is_finite()) {
                    return Err("Working RGB exceeds finite output precision".into());
                }
                let rgb = if self.sdr_highlight.is_some() && !matches!(self.kind, OutputKind::Builtin { .. }) {
                    {
                        statistics.clipped_channels += rgb.iter().filter(|v| **v < 0. || **v > 1.).count() as u64;
                        rgb.map(|v| v.clamp(0.,1.))
                    }
                } else { rgb };
                values[i] = [rgb[0], rgb[1], rgb[2], alpha];
            }
            let mut converted = [[0.; 4]; 256];
            let mut gray = [[0.; 1]; 256];
            match &self.kind {
                OutputKind::Builtin { .. } => (),
                OutputKind::Rgb(transform) | OutputKind::Cmyk(transform) => transform
                    .transform_pixels(&values[..input.len()], &mut converted[..input.len()]),
                OutputKind::Gray(transform) => {
                    transform.transform_pixels(&values[..input.len()], &mut gray[..input.len()])
                }
            }
            for (i, pixel) in output.chunks_exact_mut(bpp).enumerate() {
                let mut encoded = match &self.kind {
                    OutputKind::Builtin {
                        space,
                        matrix,
                        identity,
                    } => {
                        let linear = [values[i][0], values[i][1], values[i][2]].map(f64::from);
                        let rgb = if *identity || self.sdr_highlight.is_some() {
                            linear
                        } else {
                            layer_core::color::rgb::apply(*matrix, linear)
                        };
                        let rgb = rgb.map(|v| if destination.depth.is_float() { v } else { space.encode(v) });
                        [rgb[0], rgb[1], rgb[2], 0.]
                    }
                    OutputKind::Rgb(_) => converted[i].map(f64::from),
                    OutputKind::Gray(_) => [f64::from(gray[i][0]), 0., 0., 0.],
                    OutputKind::Cmyk(_) => converted[i].map(|v| f64::from(v) / 100.),
                };
                if destination.channels.has_alpha() {
                    encoded[destination.channels.count() - 1] = f64::from(values[i][3]);
                }
                let threshold = if self.dither == OutputDither::Stochastic8 {
                    Some(quantize::threshold([
                        origin[0].wrapping_add((chunk * 256 + i) as u32),
                        origin[1],
                    ]))
                } else {
                    None
                };
                for (channel, (code, &value)) in
                    pixel.chunks_exact_mut(step).zip(&encoded).enumerate()
                {
                    if !value.is_finite() {
                        return Err("Destination profile produced non-finite output".into());
                    }
                    if destination.depth.is_float() {
                        let alpha = destination.channels.has_alpha() && channel + 1 == destination.channels.count();
                        if value.abs() > f64::from(destination.depth.max_linear()) || (alpha && !(0. ..=1.).contains(&value)) { return Err("HDR result exceeds the selected storage range".into()); }
                        if destination.depth == layer_core::color::SampleDepth::F32 {
                            code.copy_from_slice(&(value as f32).to_le_bytes());
                        } else {
                            code.copy_from_slice(&layer_core::color::f16::from_f32(value as f32).to_bits().to_le_bytes());
                        }
                        continue;
                    }
                    let unbounded = (value * maximum).round();
                    statistics.clipped_channels += u64::from(unbounded < 0. || unbounded > maximum);
                    let alpha = destination.channels.has_alpha()
                        && channel + 1 == destination.channels.count();
                    let value = quantize::code(value, maximum, threshold.filter(|_| !alpha));
                    code.copy_from_slice(&value.to_le_bytes()[..step]);
                }
            }
        }
        Ok(statistics)
    }
}

#[cfg(test)]
mod tests;
