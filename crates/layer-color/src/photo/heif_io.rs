//! Native HEIF stills and AVIF images/first frames through packaged decoders. Source color
//! policy, bounded admission and publication remain shared with other formats.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
mod bridge;
pub(super) fn available() -> bool {
    bridge::available()
}

fn check_cancel(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) {
        Err("Image read cancelled".into())
    } else {
        Ok(())
    }
}
fn require_sdr(info: &bridge::Info) -> Result<(), String> {
    if info.nclx != 0 && matches!(info.transfer, 16 | 18) {
        return Err("HDR HEIF/AVIF needs an explicit SDR conversion before import".into());
    }
    if !(1..=16).contains(&info.bits) {
        return Err("Unsupported HEIF/AVIF sample precision".into());
    }
    Ok(())
}
fn source_profile(info: &bridge::Info, icc: Vec<u8>) -> Result<(ColorProfile, bool), String> {
    require_sdr(info)?;
    if !icc.is_empty() {
        return Ok((ColorProfile::Icc(icc.into()), false));
    }
    if info.nclx == 0 || info.primaries == 2 || info.transfer == 2 {
        return Ok((ColorProfile::default(), true));
    }
    let profile = match (info.primaries, info.transfer) {
        (1, 13) => ColorProfile::Builtin(RgbSpace::Srgb),
        (12, 13) => ColorProfile::Builtin(RgbSpace::DisplayP3),
        _ => crate::icc::nclx_profile(info.chromaticities, info.transfer)?,
    };
    Ok((profile, false))
}

pub(super) fn read(
    input: impl BufRead + Seek,
    limits: DecodeLimits,
    cancelled: &AtomicBool,
) -> Result<DecodedPhoto, String> {
    check_cancel(cancelled)?;
    let mut input = super::raster_io::Input::new(input, limits)?;
    let encoded_size = usize::try_from(input.length).map_err(|_| "HEIF input is too large")?;
    let native_budget = limits
        .codec_bytes
        .checked_sub(encoded_size)
        .filter(|v| *v > 0)
        .ok_or("HEIF input exceeds the codec budget")?;
    let mut encoded = super::raster_io::allocate(encoded_size)?;
    // Chunked reads continue to honor a host's cancellable reader and token.
    for part in encoded.chunks_mut(64 * 1024) {
        check_cancel(cancelled)?;
        input.read_exact(part).map_err(err)?;
    }
    check_cancel(cancelled)?;
    let mut photo = bridge::Photo::open(
        encoded,
        native_budget,
        limits.dimension.min(32768),
        cancelled,
    )?;
    require_sdr(&photo.info)?;
    let extent = [photo.info.width, photo.info.height];
    super::raster_io::frame_bytes(
        [photo.info.plane_width, photo.info.plane_height],
        8,
        16,
        encoded_size as u64,
        crate::MAX_ICC_BYTES,
        limits,
    )?;
    let icc = photo.metadata(false)?;
    let exif = photo.metadata(true)?;
    let mut resolution = if exif.is_empty() {
        None
    } else {
        let offset = u32::from_be_bytes(
            exif.get(..4)
                .ok_or("Incomplete HEIF EXIF offset")?
                .try_into()
                .unwrap(),
        ) as usize;
        let start = offset.checked_add(4).ok_or("HEIF EXIF offset overflow")?;
        // HEIF's irot/imir/clap properties control display geometry. EXIF
        // orientation is descriptive; applying it again would double-rotate.
        super::metadata::exif(exif.get(start..).ok_or("Invalid HEIF EXIF offset")?)?.resolution
    };
    if photo.info.quarter_turns % 2 != 0 {
        resolution = resolution.map(|r| r.swapped());
    }
    let first_frame = photo.info.first_frame != 0;
    let primary_image = !first_frame && photo.info.images > 1;
    // Reserve the Rust row and SourceBuilder band outside libheif's context
    // allowance. libheif also enforces this budget while parsing/decoding.
    let scratch = (extent[0] as usize)
        .checked_mul(8)
        .and_then(|v| v.checked_mul(layer_core::raster::TILE_SIZE as usize + 1))
        .and_then(|v| v.checked_add(icc.len()))
        .and_then(|v| v.checked_add(exif.len()))
        .ok_or("HEIF workspace size overflow")?;
    let native_budget = native_budget
        .checked_sub(scratch)
        .filter(|v| *v > 0)
        .ok_or("The decoded HEIF image exceeds the codec budget")?;
    check_cancel(cancelled)?;
    let plane = photo.decode(native_budget, cancelled)?;
    check_cancel(cancelled)?;
    let info = plane.info;
    let extent = [info.width, info.height];
    limits.extent(extent)?;
    let (profile, profile_assumed) = source_profile(&info, icc)?;
    let channels = match crate::profile_channels(&profile)? {
        ProfileChannels::Gray => SourceChannels::GrayAlpha,
        ProfileChannels::Rgb => SourceChannels::Rgba,
        _ => return Err("HEIF/AVIF decoded RGB pixels disagree with the embedded profile".into()),
    };
    let interpretation = SourceInterpretation {
        channels,
        depth: if info.bits <= 8 {
            IntegerDepth::U8
        } else {
            IntegerDepth::U16
        },
        profile,
        profile_assumed,
    };
    check_channels(channels, &interpretation.profile)?;
    let row_size = extent[0] as usize * interpretation.pixel_bytes();
    let mut row = super::raster_io::allocate(row_size)?;
    let mut builder = SourceBuilder::new(extent, interpretation, limits.source_bytes)?;
    let gray = channels == SourceChannels::GrayAlpha;
    let stored_max = if info.bits <= 8 { 255 } else { 65535 };
    let max = (1u32 << info.bits) - 1;
    for y in 0..extent[1] {
        check_cancel(cancelled)?;
        let mut at = 0;
        for x in 0..extent[0] {
            let pixel = plane.pixel(x, y);
            let sample = |c: usize| -> u32 {
                if info.storage_bpp == 4 {
                    pixel[c] as u32
                } else {
                    u16::from_le_bytes([pixel[c * 2], pixel[c * 2 + 1]]) as u32
                }
            };
            let alpha = sample(3);
            if alpha > max {
                return Err("HEIF alpha exceeds its declared precision".into());
            }
            for c in 0..4 {
                if gray && matches!(c, 1 | 2) {
                    continue;
                }
                let mut value = sample(c);
                if value > max {
                    return Err("HEIF sample exceeds its declared precision".into());
                }
                if info.premultiplied != 0 && c != 3 {
                    value = if alpha == 0 {
                        0
                    } else {
                        ((value * max + alpha / 2) / alpha).min(max)
                    };
                }
                let value = (value * stored_max + max / 2) / max;
                if info.bits <= 8 {
                    row[at] = value as u8;
                    at += 1;
                } else {
                    row[at..at + 2].copy_from_slice(&(value as u16).to_le_bytes());
                    at += 2;
                }
            }
        }
        builder.push_row(&row)?;
    }
    let mut source = builder.finish()?;
    source.resolution = resolution;
    check_cancel(cancelled)?;
    Ok(DecodedPhoto {
        source,
        first_frame,
        primary_image,
    })
}

#[cfg(test)]
mod tests;
