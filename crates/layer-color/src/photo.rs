//! Profile/depth-preserving source interchange. Codec and CMM work belongs on a
//! file worker. PNG/TIFF output consumes rows, never a second full CPU canvas.
use crate::{ProfileChannels, profile_bytes, profile_channels};
use layer_core::color::source::{SourceBuilder, SourceChannels, SourceImage, SourceInterpretation};
use layer_core::color::{ColorProfile, IntegerDepth, RgbSpace};
use std::io::{BufRead, Read, Seek, Write};

mod jpeg_io;
mod orientation;
mod png_io;
mod tiff_io;
pub use jpeg_io::read_jpeg;
pub use png_io::{read_png, write_png, write_png_rows};
pub use tiff_io::{read_tiff, write_tiff, write_tiff_rows};

#[derive(Clone, Copy, Debug)]
pub struct DecodeLimits {
    pub source_bytes: usize,
    pub codec_bytes: usize,
    pub dimension: u32,
}
impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            source_bytes: 512 * 1024 * 1024,
            codec_bytes: 128 * 1024 * 1024,
            dimension: 32768,
        }
    }
}
impl DecodeLimits {
    fn extent(self, extent: [u32; 2]) -> Result<(), String> {
        if extent.contains(&0) || extent.iter().any(|v| *v > self.dimension.min(32768)) {
            return Err("This image exceeds the dimension limit".into());
        }
        Ok(())
    }
}

/// Recognition uses file signatures; an extension never changes interpretation.
pub fn read_photo(
    mut input: impl BufRead + Seek,
    limits: DecodeLimits,
) -> Result<SourceImage, String> {
    let origin = input.stream_position().map_err(err)?;
    let mut signature = [0; 8];
    input.read_exact(&mut signature).map_err(err)?;
    input.seek(std::io::SeekFrom::Start(origin)).map_err(err)?;
    if signature == *b"\x89PNG\r\n\x1a\n" {
        read_png(input, limits)
    } else if signature[..2] == [0xff, 0xd8] {
        read_jpeg(input, limits)
    } else if matches!(
        &signature[..4],
        b"II\x2a\x00" | b"MM\x00\x2a" | b"II\x2b\x00" | b"MM\x00\x2b"
    ) {
        read_tiff(input, limits)
    } else {
        Err("Open supports PNG, JPEG and TIFF photos".into())
    }
}

fn interpretation(
    channels: SourceChannels,
    depth: IntegerDepth,
    embedded: Option<Vec<u8>>,
) -> Result<SourceInterpretation, String> {
    let profile_assumed = embedded.is_none();
    if channels == SourceChannels::Cmyk && profile_assumed {
        return Err("This CMYK image needs an explicit source ICC profile".into());
    }
    let profile = embedded
        .map(|p| ColorProfile::Icc(p.into()))
        .unwrap_or_default();
    check_channels(channels, &profile)?;
    Ok(SourceInterpretation {
        channels,
        depth,
        profile,
        profile_assumed,
    })
}

fn check_channels(channels: SourceChannels, profile: &ColorProfile) -> Result<(), String> {
    let actual = profile_channels(profile)?;
    let compatible = match channels {
        SourceChannels::Rgb | SourceChannels::Rgba => actual == ProfileChannels::Rgb,
        SourceChannels::Gray | SourceChannels::GrayAlpha => {
            actual == ProfileChannels::Gray || matches!(profile, ColorProfile::Builtin(_))
        }
        SourceChannels::Cmyk => actual == ProfileChannels::Cmyk,
    };
    if compatible {
        Ok(())
    } else {
        Err("Image channels disagree with the embedded ICC profile".into())
    }
}

fn output_row_bytes(
    extent: [u32; 2],
    interpretation: &SourceInterpretation,
) -> Result<usize, String> {
    DecodeLimits::default().extent(extent)?;
    check_channels(interpretation.channels, &interpretation.profile)?;
    Ok(extent[0] as usize * interpretation.pixel_bytes())
}

fn swap_u16(bytes: &mut [u8]) {
    for code in bytes.chunks_exact_mut(2) {
        code.swap(0, 1);
    }
}
fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod output_tests;
#[cfg(test)]
mod tests;
