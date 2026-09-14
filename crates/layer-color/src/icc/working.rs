//! Bounded native sample decoding into document-native linear Float32. Compiled
//! transforms belong to a worker and are reused for every tile of that source.
use super::*;
use layer_core::{color::source::*, raster::TILE_SIZE};

pub struct WorkingDecoder {
    source: SourceInterpretation,
    kind: DecoderKind,
    // LCMS handles must be destroyed before their owning context.
    _context: ThreadContext,
}
enum DecoderKind {
    Builtin {
        transfer: Vec<f32>,
        matrix: [[f32; 3]; 3],
        identity_primaries: bool,
    },
    Rgb(FloatTransform<4>),
    Gray(FloatTransform<1>),
    Cmyk(FloatTransform<4>),
}

impl WorkingDecoder {
    pub fn new(
        source: &SourceInterpretation,
        destination: RgbSpace,
        options: ConversionOptions,
    ) -> Result<Self, String> {
        let context = ThreadContext::new();
        let kind = if let ColorProfile::Builtin(space) = source.profile {
            if source.channels == SourceChannels::Cmyk {
                return Err("CMYK source samples require an embedded CMYK profile".into());
            }
            // This table indexes every possible integer code; it is exact at
            // each source sample rather than an interpolated tone-curve LUT.
            // Relative/perceptual/saturation are equivalent for these matrix
            // spaces. Absolute intent must retain the different source white.
            if options.intent == RenderingIntent::AbsoluteColorimetric
                && space.white() != destination.white()
            {
                Self::icc_kind(&context, source, destination, options)?
            } else {
                DecoderKind::Builtin {
                    transfer: (0..=source.depth.maximum())
                        .map(|code| {
                            space.decode(f64::from(code) / f64::from(source.depth.maximum())) as f32
                        })
                        .collect(),
                    matrix: space
                        .linear_transform(destination)
                        .map(|row| row.map(|v| v as f32)),
                    identity_primaries: space == destination,
                }
            }
        } else {
            Self::icc_kind(&context, source, destination, options)?
        };
        Ok(Self {
            source: source.clone(),
            kind,
            _context: context,
        })
    }

    fn icc_kind(
        context: &ThreadContext,
        source: &SourceInterpretation,
        destination: RgbSpace,
        options: ConversionOptions,
    ) -> Result<DecoderKind, String> {
        let input = open(context, &source.profile)?;
        let output = linear_profile(context, destination)?;
        // Go straight from source samples to linear destination coordinates.
        // An intermediate encoded/bounded sRGB image would lose wide-gamut RGB.
        match (channels(&input)?, source.channels) {
            (ProfileChannels::Rgb, SourceChannels::Rgb | SourceChannels::Rgba)
            | (ProfileChannels::Rgb, SourceChannels::Gray | SourceChannels::GrayAlpha)
                if matches!(source.profile, ColorProfile::Builtin(_))
                    || matches!(source.channels, SourceChannels::Rgb | SourceChannels::Rgba) =>
            {
                Ok(DecoderKind::Rgb(
                    Transform::new_flags_context(
                        context,
                        &input,
                        PixelFormat::RGBA_FLT,
                        &output,
                        PixelFormat::RGBA_FLT,
                        intent(options.intent),
                        flags(options) | Flags::COPY_ALPHA,
                    )
                    .map_err(error)?,
                ))
            }
            (ProfileChannels::Gray, SourceChannels::Gray | SourceChannels::GrayAlpha) => {
                Ok(DecoderKind::Gray(
                    Transform::new_flags_context(
                        context,
                        &input,
                        PixelFormat::GRAY_FLT,
                        &output,
                        PixelFormat::RGBA_FLT,
                        intent(options.intent),
                        flags(options),
                    )
                    .map_err(error)?,
                ))
            }
            (ProfileChannels::Cmyk, SourceChannels::Cmyk) => Ok(DecoderKind::Cmyk(
                Transform::new_flags_context(
                    context,
                    &input,
                    PixelFormat::CMYK_FLT,
                    &output,
                    PixelFormat::RGBA_FLT,
                    intent(options.intent),
                    flags(options),
                )
                .map_err(error)?,
            )),
            _ => Err("Source channels disagree with the embedded ICC profile".into()),
        }
    }

    /// Straight linear RGB plus unchanged linear coverage. Hidden RGB is kept
    /// here; callers premultiply only when populating a working edit surface.
    /// Output is caller-owned. Fixed stack scratch covers 256 pixels, regardless
    /// of document size; a built-in transfer table occupies at most 256 KiB.
    pub fn decode_pixels(&self, encoded: &[u8], output: &mut [[f32; 4]]) -> Result<(), String> {
        let bpp = self.source.pixel_bytes();
        if output.len().checked_mul(bpp) != Some(encoded.len()) {
            return Err("Incomplete source samples for working conversion".into());
        }
        let step = self.source.depth.bytes();
        let channels = self.source.channels;
        let maximum = self.source.depth.maximum() as f32;
        let code = |pixel: &[u8], channel: usize| -> usize {
            let at = channel * step;
            if step == 1 {
                pixel[at] as usize
            } else {
                u16::from_le_bytes([pixel[at], pixel[at + 1]]) as usize
            }
        };
        for (input, destination) in encoded.chunks(256 * bpp).zip(output.chunks_mut(256)) {
            let mut values = [[0f32; 4]; 256];
            let mut gray = [[0f32; 1]; 256];
            for (i, pixel) in input.chunks_exact(bpp).enumerate() {
                let alpha = if channels.has_alpha() {
                    code(pixel, channels.count() - 1) as f32 / maximum
                } else {
                    1.
                };
                let mut codes = [code(pixel, 0); 3];
                if matches!(
                    channels,
                    SourceChannels::Rgb | SourceChannels::Rgba | SourceChannels::Cmyk
                ) {
                    codes = std::array::from_fn(|c| code(pixel, c));
                }
                match &self.kind {
                    DecoderKind::Builtin {
                        transfer,
                        matrix,
                        identity_primaries,
                    } => {
                        let linear = codes.map(|c| transfer[c]);
                        let rgb = if *identity_primaries {
                            linear
                        } else {
                            matrix.map(|row| {
                                row[0] * linear[0] + row[1] * linear[1] + row[2] * linear[2]
                            })
                        };
                        destination[i] = [rgb[0], rgb[1], rgb[2], alpha];
                    }
                    DecoderKind::Rgb(_) => {
                        values[i] = [
                            codes[0] as f32 / maximum,
                            codes[1] as f32 / maximum,
                            codes[2] as f32 / maximum,
                            alpha,
                        ];
                    }
                    DecoderKind::Gray(_) => gray[i] = [codes[0] as f32 / maximum],
                    DecoderKind::Cmyk(_) => {
                        values[i] = std::array::from_fn(|c| code(pixel, c) as f32 * 100. / maximum)
                    }
                }
            }
            match &self.kind {
                DecoderKind::Builtin { .. } => (),
                DecoderKind::Rgb(transform) => {
                    transform.transform_pixels(&values[..destination.len()], destination)
                }
                DecoderKind::Gray(transform) => {
                    transform.transform_pixels(&gray[..destination.len()], destination)
                }
                DecoderKind::Cmyk(transform) => {
                    transform.transform_pixels(&values[..destination.len()], destination)
                }
            }
            // Gray/CMYK transforms do not own coverage. Reinstall exact source
            // alpha independently of CMM formatter behavior in every case.
            for (pixel, result) in input.chunks_exact(bpp).zip(destination) {
                result[3] = if channels.has_alpha() {
                    code(pixel, channels.count() - 1) as f32 / maximum
                } else {
                    1.
                };
                if result.iter().any(|v| !v.is_finite()) {
                    return Err("Source profile produced non-finite working samples".into());
                }
            }
        }
        Ok(())
    }

    /// Validate/decode one source tile, cropping padded samples to transparent.
    /// No converted full-image copy or mutable source allocation is retained.
    pub fn decode_tile(
        &self,
        source: &SourceImage,
        coordinate: [u32; 2],
        output: &mut [[f32; 4]],
    ) -> Result<(), String> {
        if source.interpretation != self.source || output.len() != (TILE_SIZE * TILE_SIZE) as usize
        {
            return Err("Source interpretation changed during working conversion".into());
        }
        let tile = source.tiles.get(&coordinate).ok_or("Missing source tile")?;
        if tile.descriptor != self.source.descriptor() {
            return Err("Source tile has the wrong sample representation".into());
        }
        self.decode_pixels(&tile.decode()?, output)?;
        let origin =
            coordinate.map(|v| v.checked_mul(TILE_SIZE).ok_or("Invalid source coordinate"));
        let [x, y] = [origin[0].clone()?, origin[1].clone()?];
        for (i, pixel) in output.iter_mut().enumerate() {
            if x + i as u32 % TILE_SIZE >= source.extent[0]
                || y + i as u32 / TILE_SIZE >= source.extent[1]
            {
                *pixel = [0.; 4];
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
