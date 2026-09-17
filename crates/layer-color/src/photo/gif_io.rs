//! GIF first-frame import, composited on the logical screen with its palette.
use super::raster_io::{self, Input};
use super::*;
use std::io::SeekFrom;

pub(super) fn read(
    input: impl BufRead + Seek,
    limits: DecodeLimits,
) -> Result<DecodedPhoto, String> {
    let mut input = Input::new(input, limits)?;
    let first_frame = inspect(&mut input, limits)?;
    let encoded = input.length;
    input.rewind().map_err(err)?;
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    options.set_memory_limit(gif::MemoryLimit::Bytes(
        std::num::NonZeroU64::new(limits.codec_bytes as u64)
            .ok_or("The GIF codec budget is empty")?,
    ));
    let mut decoder = options.read_info(input).map_err(err)?;
    let extent = [u32::from(decoder.width()), u32::from(decoder.height())];
    let icc = decoder.icc_profile().map(Vec::from);
    let interpretation = interpretation(SourceChannels::Rgba, SampleDepth::U8, icc)?;
    let frame = decoder
        .next_frame_info()
        .map_err(err)?
        .ok_or("GIF contains no image frame")?;
    let (left, top, width, height, transparent) = (
        frame.left as usize,
        frame.top as usize,
        frame.width as usize,
        frame.height as usize,
        frame.transparent.is_some(),
    );
    let bytes = raster_io::frame_bytes(extent, 4, 4, encoded.saturating_mul(2), 65536, limits)?;
    let mut pixels = raster_io::allocate(bytes)?;
    // A transparency declaration keeps the logical screen transparent. Otherwise
    // an opaque sub-frame sits on the declared global background color.
    if !transparent
        && let Some(index) = decoder.bg_color()
        && let Some(rgb) = decoder
            .global_palette()
            .and_then(|p| p.get(index * 3..index * 3 + 3))
    {
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[..3].copy_from_slice(rgb);
            pixel[3] = 255;
        }
    }
    let mut frame = raster_io::allocate(width * height * 4)?;
    decoder.read_into_buffer(&mut frame).map_err(err)?;
    for y in 0..height {
        let to = ((y + top) * extent[0] as usize + left) * 4;
        pixels[to..to + width * 4].copy_from_slice(&frame[y * width * 4..(y + 1) * width * 4]);
    }
    drop(frame);
    drop(decoder);
    let source = raster_io::source(&pixels, extent, interpretation, Default::default(), limits)?;
    Ok(DecodedPhoto {
        source,
        first_frame,
        primary_image: false,
    })
}

fn byte(input: &mut impl Read) -> Result<u8, String> {
    let mut byte = [0];
    input.read_exact(&mut byte).map_err(err)?;
    Ok(byte[0])
}
fn blocks(input: &mut (impl Read + Seek), cap: u64) -> Result<(), String> {
    let mut length = 0;
    loop {
        let count = byte(input)?;
        if count == 0 {
            return Ok(());
        }
        length += u64::from(count);
        if length > cap {
            return Err("GIF metadata exceeds the codec budget".into());
        }
        input
            .seek(SeekFrom::Current(i64::from(count)))
            .map_err(err)?;
    }
}

/// Inspect structure without allocating/decompressing subsequent frames. This
/// finds animation and rejects malformed ranges before any full-frame allocation.
fn inspect(input: &mut Input<impl BufRead + Seek>, limits: DecodeLimits) -> Result<bool, String> {
    let mut header = [0; 13];
    input.read_exact(&mut header).map_err(err)?;
    if !matches!(&header[..6], b"GIF87a" | b"GIF89a") {
        return Err("Invalid GIF signature".into());
    }
    let width = u32::from(u16::from_le_bytes(header[6..8].try_into().unwrap()));
    let height = u32::from(u16::from_le_bytes(header[8..10].try_into().unwrap()));
    limits.extent([width, height])?;
    if header[10] & 0x80 != 0 {
        input
            .seek(SeekFrom::Current(3 * (2i64 << (header[10] & 7))))
            .map_err(err)?;
    }
    let mut frames = 0u64;
    let mut has_icc = false;
    loop {
        match byte(input)? {
            0x3b => {
                return if frames == 0 {
                    Err("GIF contains no image frame".into())
                } else {
                    Ok(frames > 1)
                };
            }
            0x2c => {
                let mut frame = [0; 9];
                input.read_exact(&mut frame).map_err(err)?;
                let value = |i| u32::from(u16::from_le_bytes(frame[i..i + 2].try_into().unwrap()));
                if value(4) == 0
                    || value(6) == 0
                    || value(0) + value(4) > width
                    || value(2) + value(6) > height
                {
                    return Err("GIF frame is outside its logical screen".into());
                }
                if frame[8] & 0x80 != 0 {
                    input
                        .seek(SeekFrom::Current(3 * (2i64 << (frame[8] & 7))))
                        .map_err(err)?;
                }
                let code_size = byte(input)?;
                if !(2..=8).contains(&code_size) {
                    return Err("Invalid GIF LZW code size".into());
                }
                blocks(input, input.length)?;
                frames += 1;
            }
            0x21 => {
                let label = byte(input)?;
                if label == 0xff {
                    if byte(input)? != 11 {
                        return Err("Invalid GIF application extension".into());
                    }
                    let mut application = [0; 11];
                    input.read_exact(&mut application).map_err(err)?;
                    let icc = &application == b"ICCRGBG1012";
                    if icc && (has_icc || frames != 0) {
                        return Err("GIF ICC profile must occur once before image frames".into());
                    }
                    has_icc |= icc;
                    blocks(
                        input,
                        if icc {
                            crate::MAX_ICC_BYTES as u64
                        } else {
                            limits.codec_bytes as u64
                        },
                    )?;
                } else if label == 0x01 {
                    return Err("GIF plain-text rendering is not supported for photo import".into());
                } else {
                    blocks(input, limits.codec_bytes as u64)?;
                }
            }
            _ => return Err("Invalid GIF block".into()),
        }
    }
}
