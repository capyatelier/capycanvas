//! Flat RGB/RGBA OpenEXR, decoded one scanline block at a time. EXR uses
//! premultiplied linear RGB; native sources store straight RGB. Untagged files
//! use EXR's Rec.709 primaries and our 203 nit reference-white interpretation.
use super::*;
use exr::{
    block::{self, UncompressedBlock, reader::ChunksReader, writer::SequentialBlocksCompressor},
    math::Vec2,
    meta::{
        BlockDescription,
        attribute::{ChannelDescription, Chromaticities, Compression, LineOrder, SampleType},
        header::Header,
    },
};
use layer_core::color::hdr;
use std::sync::atomic::{AtomicBool, Ordering};

fn chromaticities(space: RgbSpace) -> Chromaticities {
    let xy = |p: [f64; 2]| Vec2(p[0] as f32, p[1] as f32);
    let p = space.primaries();
    Chromaticities {
        red: xy(p[0]),
        green: xy(p[1]),
        blue: xy(p[2]),
        white: xy(space.white()),
    }
}

// Bound attribute allocations before handing metadata to the codec. Single-part,
// scanline version 2 only; reject unknown feature flags instead of guessing.
fn preflight(input: &mut (impl Read + Seek), budget: usize) -> Result<(), String> {
    let start = input.stream_position().map_err(err)?;
    let mut prefix = [0; 8];
    input.read_exact(&mut prefix).map_err(err)?;
    if prefix[..4] != [0x76, 0x2f, 0x31, 0x01]
        || u32::from_le_bytes(prefix[4..].try_into().unwrap()) & !0x400 != 2
    {
        return Err("OpenEXR requires a flat, single-part scanline image".into());
    }
    let mut used = 8usize;
    let limit = budget.min(1024 * 1024);
    let read_name = |input: &mut _, used: &mut usize| -> Result<usize, String> {
        for n in 0..=255 {
            let mut b = [0];
            Read::read_exact(input, &mut b).map_err(err)?;
            *used += 1;
            if *used > limit {
                return Err("OpenEXR header exceeds the memory budget".into());
            }
            if b[0] == 0 {
                return Ok(n);
            }
        }
        Err("OpenEXR attribute name is too long".into())
    };
    while read_name(input, &mut used)? != 0 {
        read_name(input, &mut used)?;
        let mut size = [0; 4];
        input.read_exact(&mut size).map_err(err)?;
        let size = u32::from_le_bytes(size) as usize;
        used = used
            .checked_add(4)
            .and_then(|v| v.checked_add(size))
            .ok_or("Invalid OpenEXR header")?;
        if used > limit {
            return Err("OpenEXR header exceeds the memory budget".into());
        }
        std::io::copy(&mut input.take(size as u64), &mut std::io::sink()).map_err(err)?;
    }
    input.seek(std::io::SeekFrom::Start(start)).map_err(err)?;
    Ok(())
}

pub(super) fn read_exr(
    mut input: impl BufRead + Seek,
    limits: DecodeLimits,
    cancelled: &AtomicBool,
) -> Result<SourceImage, String> {
    if cancelled.load(Ordering::Acquire) {
        return Err("Image read cancelled".into());
    }
    preflight(&mut input, limits.codec_bytes)?;
    let reader = block::read(input, true).map_err(err)?;
    if reader.headers().len() != 1 {
        return Err("Multipart OpenEXR is not supported".into());
    }
    let header = reader.headers()[0].clone();
    let extent = [
        u32::try_from(header.layer_size.0).map_err(err)?,
        u32::try_from(header.layer_size.1).map_err(err)?,
    ];
    limits.extent(extent)?;
    if header.deep
        || header.blocks != BlockDescription::ScanLines
        || !matches!(
            header.compression,
            Compression::Uncompressed | Compression::RLE | Compression::ZIP1 | Compression::ZIP16
        )
        || header.line_order != LineOrder::Increasing
        || header.data_window() != header.shared_attributes.display_window
    {
        return Err("OpenEXR requires increasing flat scanlines, matching data/display windows and lossless NONE, RLE or ZIP compression".into());
    }
    let names: Vec<_> = header
        .channels
        .list
        .iter()
        .map(|c| c.name.to_string())
        .collect();
    let channels = match names
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["B", "G", "R"] => SourceChannels::Rgb,
        ["A", "B", "G", "R"] => SourceChannels::Rgba,
        _ => return Err("OpenEXR requires only RGB or RGBA channels".into()),
    };
    if header
        .channels
        .list
        .iter()
        .any(|c| c.sample_type != SampleType::F32 || c.sampling != Vec2(1, 1))
    {
        return Err("OpenEXR requires full-resolution FLOAT channels".into());
    }
    let declared = header.shared_attributes.chromaticities;
    let space = match declared {
        None => RgbSpace::Srgb,
        Some(c) => RgbSpace::ALL.into_iter().find(|&s| {
            let expected = chromaticities(s);
            [c.red, c.green, c.blue, c.white].into_iter().zip([expected.red, expected.green, expected.blue, expected.white])
                .all(|(a,b)| (a.0-b.0).abs() <= 1e-5 && (a.1-b.1).abs() <= 1e-5)
        }).ok_or("OpenEXR primaries are unsupported; convert to sRGB, Display P3, Adobe RGB or ProPhoto primaries")?,
    };
    let white = header
        .own_attributes
        .white_luminance
        .unwrap_or(hdr::REFERENCE_WHITE_NITS);
    if !white.is_finite() || white <= 0. {
        return Err("Invalid OpenEXR white luminance".into());
    }
    if header.own_attributes.adopted_neutral.is_some() {
        return Err("OpenEXR adopted-neutral rendering is not supported".into());
    }
    let width = extent[0] as usize;
    // SourceBuilder owns a 256-row band; codec owns at most a 16-row block
    // and compressed scratch. Include metadata, offsets and row conversions.
    let scratch = width
        .checked_mul(4 * 4 * (256 + 16 * 4 + 2))
        .and_then(|n| n.checked_add(1024 * 1024))
        .ok_or("OpenEXR size overflow")?;
    if scratch > limits.codec_bytes {
        return Err("OpenEXR decoding exceeds the memory budget".into());
    }
    let mut builder = SourceBuilder::new(
        extent,
        SourceInterpretation {
            channels,
            depth: SampleDepth::F32,
            profile: ColorProfile::Builtin(space),
            profile_assumed: declared.is_none(),
        },
        limits.source_bytes,
    )?;
    let mut decoder = reader
        .all_chunks(true)
        .map_err(err)?
        .sequential_decompressor(true);
    let count = channels.count();
    let mut next_y = 0;
    while let Some(block) = decoder.next() {
        if cancelled.load(Ordering::Acquire) {
            return Err("Image read cancelled".into());
        }
        let block = block.map_err(err)?;
        if block.index.pixel_position != Vec2(0, next_y)
            || block.index.pixel_size.0 != width
            || block.index.pixel_size.1 > 16
        {
            return Err("OpenEXR scanline blocks must occur once in increasing order".into());
        }
        for scanline in block.data.chunks_exact(width * count * 4) {
            let mut row = vec![0; width * count * 4];
            for x in 0..width {
                let sample = |c: usize| {
                    f32::from_ne_bytes(scanline[(c * width + x) * 4..][..4].try_into().unwrap())
                };
                let a = if count == 4 { sample(0) } else { 1. };
                let mut p = [sample(count - 1), sample(count - 2), sample(count - 3), a];
                hdr::validate_pixel(SampleDepth::F32, p).map_err(str::to_string)?;
                if a == 0. && p[..3].iter().any(|v| *v != 0.) {
                    return Err("OpenEXR zero-alpha emission cannot be represented by straight-alpha layers".into());
                }
                for v in &mut p[..3] {
                    *v = if a == 0. {
                        0.
                    } else {
                        (f64::from(*v) / f64::from(a) * f64::from(white)
                            / f64::from(hdr::REFERENCE_WHITE_NITS)) as f32
                    };
                }
                hdr::validate_pixel(SampleDepth::F32, p).map_err(str::to_string)?;
                for (out, v) in row[x * count * 4..][..count * 4].chunks_exact_mut(4).zip(p) {
                    out.copy_from_slice(&v.to_le_bytes());
                }
            }
            builder.push_row(&row)?;
            next_y += 1;
        }
    }
    let mut image = builder.finish()?;
    if let Some(x) = header.own_attributes.horizontal_density {
        let y = f64::from(x) * f64::from(header.shared_attributes.pixel_aspect);
        let fraction = |value: f64| -> Result<[u32; 2], String> {
            if !value.is_finite() || value <= 0. || value > f64::from(u32::MAX) {
                return Err("Invalid OpenEXR pixel density".into());
            }
            let denominator = (f64::from(u32::MAX) / value).min(10000.).floor() as u32;
            let numerator = (value * f64::from(denominator)).round() as u32;
            if numerator == 0 {
                return Err("OpenEXR pixel density is too small".into());
            }
            Ok([numerator, denominator])
        };
        image.resolution = Some(layer_core::ImageResolution {
            unit: layer_core::ResolutionUnit::Inch,
            density: [fraction(f64::from(x))?, fraction(y)?],
        });
    }
    Ok(image)
}

/// Writes premultiplied linear working rows as FLOAT channels with lossless ZIP.
/// Pixels are not tone mapped or narrowed. Cancellation/provider errors propagate
/// to the host's atomic-file transaction. No full image or unbounded worker queue.
pub fn write_exr_rows(
    output: impl Write + Seek,
    extent: [u32; 2],
    space: RgbSpace,
    resolution: Option<layer_core::ImageResolution>,
    mut read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
) -> Result<(), String> {
    validate_extent(extent, 32768)?;
    let mut header = Header::new(
        "RGB".try_into().unwrap(),
        (extent[0] as usize, extent[1] as usize),
        ["A", "B", "G", "R"]
            .into_iter()
            .map(|n| ChannelDescription::new(n, SampleType::F32, true))
            .collect(),
    )
    .with_encoding(
        Compression::ZIP1,
        BlockDescription::ScanLines,
        LineOrder::Increasing,
    );
    header.shared_attributes.chromaticities = Some(chromaticities(space));
    header.own_attributes.white_luminance = Some(hdr::REFERENCE_WHITE_NITS);
    header.own_attributes.layer_name = None;
    if let Some(resolution) = resolution {
        resolution.validate()?;
        let [x, y] = resolution.pixels_per_inch();
        header.own_attributes.horizontal_density = Some(x as f32);
        header.shared_attributes.pixel_aspect = (y / x) as f32;
    }
    block::write(output, vec![header].into(), true, |meta, writer| {
        let mut compressor = SequentialBlocksCompressor::new(&meta, writer);
        let mut row = vec![[0.; 4]; extent[0] as usize];
        for (index, location) in block::enumerate_ordered_header_block_indices(&meta.headers) {
            read(location.pixel_position.1 as u32, &mut row)
                .map_err(|e| exr::error::Error::Invalid(e.into()))?;
            for &pixel in &row {
                hdr::validate_pixel(SampleDepth::F32, pixel)
                    .map_err(|e| exr::error::Error::Invalid(e.into()))?;
            }
            let mut data = Vec::with_capacity(row.len() * 16);
            for channel in [3, 2, 1, 0] {
                for pixel in &row {
                    data.extend_from_slice(&pixel[channel].to_ne_bytes());
                }
            }
            compressor.compress_block(
                index,
                UncompressedBlock {
                    index: location,
                    data,
                },
            )?;
        }
        Ok(())
    })
    .map_err(err)
}

#[cfg(test)]
mod tests;
