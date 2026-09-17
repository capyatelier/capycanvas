//! WebP still import with explicit ICC/EXIF and animation interpretation.
use super::raster_io::{self, Input};
use super::*;
use std::io::SeekFrom;

pub(super) fn read(
    input: impl BufRead + Seek,
    limits: DecodeLimits,
) -> Result<DecodedPhoto, String> {
    let mut input = Input::new(input, limits)?;
    let info = inspect(&mut input, limits)?;
    let encoded = input.length;
    input.rewind().map_err(err)?;
    let mut decoder = image_webp::WebPDecoder::new(input).map_err(err)?;
    let (width, height) = decoder.dimensions();
    let extent = [width, height];
    if extent != info.extent {
        return Err("Inconsistent WebP image dimensions".into());
    }
    // The vendored patch also bounds entropy tables, whose size is independent
    // of pixel dimensions. Other pixel/bitstream workspaces are admitted here;
    // one temporary Huffman tree and small codec state fit in the 1 MiB margin.
    let entropy_budget = limits.codec_bytes / 4;
    let bpp = if decoder.has_alpha() { 4 } else { 3 };
    let size = raster_io::frame_bytes(
        extent,
        bpp,
        32,
        encoded.saturating_mul(2),
        entropy_budget.saturating_add(1024 * 1024),
        limits,
    )?;
    decoder.set_memory_limit(entropy_budget);
    let icc = decoder.icc_profile().map_err(err)?;
    if info.icc != icc.is_some() {
        return Err("The WebP declares an unreadable ICC profile".into());
    }
    let metadata = decoder
        .exif_metadata()
        .map_err(err)?
        .as_deref()
        .map(super::metadata::exif)
        .transpose()?
        .unwrap_or_default();
    let first_frame = decoder.is_animated();
    if first_frame {
        decoder.set_background_color(info.background).map_err(err)?;
    }
    let mut pixels = raster_io::allocate(size)?;
    decoder.read_image(&mut pixels).map_err(err)?;
    drop(decoder);
    let channels = if bpp == 4 || first_frame {
        SourceChannels::Rgba
    } else {
        SourceChannels::Rgb
    };
    // VP8X's alpha flag describes the frame bitstreams. An opaque first frame
    // can still occupy only part of a translucent animation background.
    if first_frame && bpp == 3 {
        let mut rgba = raster_io::allocate(width as usize * height as usize * 4)?;
        let [x, y, w, h] = info.first_rect.ok_or("WebP animation contains no frame")?;
        for (i, (rgb, to)) in pixels
            .chunks_exact(3)
            .zip(rgba.chunks_exact_mut(4))
            .enumerate()
        {
            to[..3].copy_from_slice(rgb);
            let px = i as u32 % width;
            let py = i as u32 / width;
            to[3] = if px >= x && px < x + w && py >= y && py < y + h {
                255
            } else {
                info.background[3]
            };
        }
        pixels = rgba;
    }
    let interpretation = interpretation(channels, IntegerDepth::U8, icc)?;
    let source = raster_io::source(&pixels, extent, interpretation, metadata, limits)?;
    Ok(DecodedPhoto {
        source,
        first_frame,
        primary_image: false,
    })
}

#[derive(Default)]
struct Info {
    extent: [u32; 2],
    icc: bool,
    first_rect: Option<[u32; 4]>,
    background: [u8; 4],
}
fn u24(bytes: &[u8]) -> u32 {
    u32::from(bytes[0]) | u32::from(bytes[1]) << 8 | u32::from(bytes[2]) << 16
}
fn header(input: &mut (impl Read + Seek), end: u64) -> Result<([u8; 4], u64, u64), String> {
    let mut chunk = [0; 8];
    input.read_exact(&mut chunk).map_err(err)?;
    let size = u64::from(u32::from_le_bytes(chunk[4..].try_into().unwrap()));
    let position = input.stream_position().map_err(err)?;
    let next = position
        .checked_add(size + (size & 1))
        .filter(|n| *n <= end)
        .ok_or("WebP chunk is outside the file")?;
    Ok((chunk[..4].try_into().unwrap(), size, next))
}
fn bitstream_extent(input: &mut impl Read, kind: [u8; 4], size: u64) -> Result<[u32; 2], String> {
    if &kind == b"VP8 " && size >= 10 {
        let mut h = [0; 10];
        input.read_exact(&mut h).map_err(err)?;
        if h[0] & 1 != 0 || h[3..6] != [0x9d, 1, 0x2a] {
            return Err("Invalid WebP VP8 frame".into());
        }
        Ok([
            u32::from(u16::from_le_bytes(h[6..8].try_into().unwrap()) & 0x3fff),
            u32::from(u16::from_le_bytes(h[8..10].try_into().unwrap()) & 0x3fff),
        ])
    } else if &kind == b"VP8L" && size >= 5 {
        let mut h = [0; 5];
        input.read_exact(&mut h).map_err(err)?;
        let bits = u32::from_le_bytes(h[1..].try_into().unwrap());
        if h[0] != 0x2f || bits >> 29 != 0 {
            return Err("Invalid WebP lossless frame".into());
        }
        Ok([(bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1])
    } else {
        Err("Incomplete WebP frame header".into())
    }
}

/// Validate outer and embedded frame bounds before the codec can allocate from
/// their dimensions. VP8X alone is insufficient: inner headers may disagree.
fn inspect(input: &mut Input<impl BufRead + Seek>, limits: DecodeLimits) -> Result<Info, String> {
    let mut riff = [0; 12];
    input.read_exact(&mut riff).map_err(err)?;
    if &riff[..4] != b"RIFF" || &riff[8..] != b"WEBP" {
        return Err("Invalid WebP signature".into());
    }
    let end = u64::from(u32::from_le_bytes(riff[4..8].try_into().unwrap())) + 8;
    if end < 20 || end > input.length {
        return Err("Incomplete WebP container".into());
    }
    let mut info = Info::default();
    let mut flags = None;
    let mut exif = false;
    let mut image = false;
    while input.stream_position().map_err(err)? < end {
        let (kind, size, next) = header(input, end)?;
        match &kind {
            b"VP8X" => {
                if flags.is_some() || image || size != 10 {
                    return Err("Invalid WebP extended header".into());
                }
                let mut h = [0; 10];
                input.read_exact(&mut h).map_err(err)?;
                flags = Some(h[0]);
                info.extent = [u24(&h[4..7]) + 1, u24(&h[7..10]) + 1];
                limits.extent(info.extent)?;
            }
            b"ICCP" => {
                if info.icc || size == 0 || size > crate::MAX_ICC_BYTES as u64 {
                    return Err("Invalid WebP ICC profile chunk".into());
                }
                info.icc = true;
            }
            b"EXIF" => {
                if exif || size > crate::MAX_ICC_BYTES as u64 {
                    return Err("Invalid WebP EXIF chunk".into());
                }
                exif = true;
            }
            b"ANIM" => {
                if size != 6 || flags.is_none_or(|f| f & 2 == 0) {
                    return Err("Invalid WebP animation header".into());
                }
                let mut h = [0; 6];
                input.read_exact(&mut h).map_err(err)?;
                info.background = [h[2], h[1], h[0], h[3]];
            }
            b"ANMF" => {
                if size < 24 || flags.is_none_or(|f| f & 2 == 0) {
                    return Err("Invalid WebP animation frame".into());
                }
                let mut h = [0; 16];
                input.read_exact(&mut h).map_err(err)?;
                let rect = [
                    u24(&h[..3]) * 2,
                    u24(&h[3..6]) * 2,
                    u24(&h[6..9]) + 1,
                    u24(&h[9..12]) + 1,
                ];
                if rect[0] + rect[2] > info.extent[0] || rect[1] + rect[3] > info.extent[1] {
                    return Err("WebP frame is outside its canvas".into());
                }
                let frame_end = next - (size & 1);
                let mut frame_image = false;
                while input.stream_position().map_err(err)? < frame_end {
                    let (sub, len, after) = header(input, frame_end)?;
                    if matches!(&sub, b"VP8 " | b"VP8L") {
                        if bitstream_extent(input, sub, len)? != [rect[2], rect[3]] {
                            return Err("Inconsistent WebP frame dimensions".into());
                        }
                        frame_image = true;
                    }
                    input.seek(SeekFrom::Start(after)).map_err(err)?;
                }
                if !frame_image {
                    return Err("WebP animation frame has no image".into());
                }
                info.first_rect.get_or_insert(rect);
                image = true;
            }
            b"VP8 " | b"VP8L" => {
                let extent = bitstream_extent(input, kind, size)?;
                limits.extent(extent)?;
                if flags.is_some() && info.extent != extent {
                    return Err("Inconsistent WebP image dimensions".into());
                }
                info.extent = extent;
                image = true;
            }
            _ => (),
        }
        input.seek(SeekFrom::Start(next)).map_err(err)?;
    }
    if !image {
        return Err("WebP contains no image".into());
    }
    if flags.is_some_and(|f| f & 0x20 != 0) != info.icc {
        return Err("The WebP declares an unreadable ICC profile".into());
    }
    if flags.is_some_and(|f| f & 8 != 0) != exif {
        return Err("Inconsistent WebP EXIF declaration".into());
    }
    Ok(info)
}
