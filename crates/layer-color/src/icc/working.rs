//! Bounded native sample decoding into document-native linear Float32. Compiled
//! transforms belong to a worker and are reused for every tile of that source.
use super::*;
use layer_core::{color::source::*, raster::TILE_SIZE};

pub struct WorkingDecoder {
    source: SourceInterpretation,
    kind: DecoderKind,
}
enum DecoderKind {
    Linear { matrix: [[f32; 3]; 3], identity: bool },
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
        validate_options(options)?;
        let kind = if source.depth.is_float() {
            let ColorProfile::Builtin(space) = source.profile else { return Err("HDR source needs explicit linear RGB primaries".into()); };
            if !matches!(source.channels, SourceChannels::Rgb | SourceChannels::Rgba) { return Err("HDR supports RGB and RGBA samples".into()); }
            let matrix = if options.intent == RenderingIntent::AbsoluteColorimetric {
                space.absolute_linear_transform(destination)
            } else {
                space.linear_transform(destination)
            };
            DecoderKind::Linear { matrix: matrix.map(|r| r.map(|v| v as f32)), identity: space == destination }
        } else if let ColorProfile::Builtin(space) = source.profile {
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
                Self::icc_kind(source, destination, options)?
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
            Self::icc_kind(source, destination, options)?
        };
        Ok(Self {
            source: source.clone(),
            kind,
        })
    }

    fn icc_kind(
        source: &SourceInterpretation,
        destination: RgbSpace,
        options: ConversionOptions,
    ) -> Result<DecoderKind, String> {
        let input = open(&source.profile)?;
        let output = linear_profile(destination)?;
        // Go straight from source samples to linear destination coordinates.
        // An intermediate encoded/bounded sRGB image would lose wide-gamut RGB.
        match (channels(&input)?, source.channels) {
            (ProfileChannels::Rgb, SourceChannels::Rgb | SourceChannels::Rgba)
            | (ProfileChannels::Rgb, SourceChannels::Gray | SourceChannels::GrayAlpha)
                if matches!(source.profile, ColorProfile::Builtin(_))
                    || matches!(source.channels, SourceChannels::Rgb | SourceChannels::Rgba) =>
            {
                Ok(DecoderKind::Rgb(CompiledTransform::new(
                    &input, &output, options,
                )?))
            }
            (ProfileChannels::Gray, SourceChannels::Gray | SourceChannels::GrayAlpha) => Ok(
                DecoderKind::Gray(CompiledTransform::new(&input, &output, options)?),
            ),
            (ProfileChannels::Cmyk, SourceChannels::Cmyk) => Ok(DecoderKind::Cmyk(
                CompiledTransform::new(&input, &output, options)?,
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
        if let DecoderKind::Linear { matrix, identity } = &self.kind {
            for (input, output) in encoded.chunks_exact(bpp).zip(output) {
                let mut p = [0.,0.,0.,1.];
                for (c, b) in input.chunks_exact(2).enumerate() { p[c] = layer_core::color::f16::from_bits(u16::from_le_bytes([b[0],b[1]])).to_f32(); }
                layer_core::color::hdr::encode_pixel(p).map_err(str::to_string)?;
                if !identity { let rgb = [p[0],p[1],p[2]]; for c in 0..3 { p[c] = matrix[c][0]*rgb[0]+matrix[c][1]*rgb[1]+matrix[c][2]*rgb[2]; } }
                *output = p;
            }
            return Ok(());
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
                    DecoderKind::Linear { .. } => unreachable!(),
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
                DecoderKind::Linear { .. } => unreachable!(),
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
        self.decode_tile_with(source, coordinate, output, |tile, output| {
            self.decode_pixels(&tile.decode()?, output)
        })
    }

    /// Reuse exact encoded samples while retaining this worker's own profile
    /// transform and the same descriptor, extent and padding validation.
    pub fn decode_tile_cached(
        &self,
        source: &SourceImage,
        coordinate: [u32; 2],
        output: &mut [[f32; 4]],
        cache: &layer_core::raster::DecodedTileCache,
    ) -> Result<(), String> {
        self.decode_tile_with(source, coordinate, output, |tile, output| {
            let samples = cache.decode(tile)?;
            self.decode_pixels(&samples, output)
        })
    }

    fn decode_tile_with(
        &self,
        source: &SourceImage,
        coordinate: [u32; 2],
        output: &mut [[f32; 4]],
        decode: impl FnOnce(
            &std::sync::Arc<layer_core::raster::TileBlob>,
            &mut [[f32; 4]],
        ) -> Result<(), String>,
    ) -> Result<(), String> {
        if source.interpretation != self.source || output.len() != (TILE_SIZE * TILE_SIZE) as usize
        {
            return Err("Source interpretation changed during working conversion".into());
        }
        let tile = source.tiles.get(&coordinate).ok_or("Missing source tile")?;
        if tile.descriptor != self.source.descriptor() {
            return Err("Source tile has the wrong sample representation".into());
        }
        decode(tile, output)?;
        let origin =
            coordinate.map(|v| v.checked_mul(TILE_SIZE).ok_or("Invalid source coordinate"));
        let [x, y] = [origin[0]?, origin[1]?];
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
