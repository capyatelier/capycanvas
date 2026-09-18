//! Profile/depth-preserving source interchange. Codec and CMM work belongs on a
//! file worker. PNG/TIFF output consumes rows, never a second full CPU canvas.
use crate::{ProfileChannels, profile_bytes, profile_channels};
use layer_core::color::source::{SourceBuilder, SourceChannels, SourceImage, SourceInterpretation};
use layer_core::color::{ColorProfile, SampleDepth, RgbSpace};
use std::io::{BufRead, Read, Seek, Write};

mod jpeg_codec;
mod jpeg_io;
mod jpeg_markers;
mod jpeg_mpf;
mod memory;
pub use memory::PhotoMemoryBudget;
mod metadata;
#[cfg(test)]
mod metadata_tests;
mod orientation;
mod png_io;
mod hdr_png;
pub use hdr_png::{inspect_hdr_rows, preview_hdr_rows, write_hdr_png_rows};
mod tiff_io;
mod raster_io;
mod bmp_io;
mod gif_io;
mod webp_io;
#[cfg(all(feature = "heif", target_os = "linux"))]
mod heif_io;
#[cfg(test)]
mod tiff_policy_tests;
pub use jpeg_io::{
    JpegEncodeOptions, read_jpeg, write_jpeg, write_jpeg_rows, write_jpeg_rows_with_options,
    write_jpeg_with_options,
};
pub use png_io::{read_png, write_png, write_png_rows};
pub use tiff_io::{read_tiff, write_tiff, write_tiff_rows};

/// Decoder capabilities, also used by file pickers, clipboard and file drops.
/// These describe implemented readers, not formats merely known to a host OS.
#[derive(serde::Serialize)]
pub struct PhotoFormat {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub mime_types: &'static [&'static str],
}
pub const PHOTO_FORMATS: &[PhotoFormat] = &[
    PhotoFormat { name: "TIFF", extensions: &["tif", "tiff"], mime_types: &["image/tiff"] },
    PhotoFormat { name: "PNG", extensions: &["png"], mime_types: &["image/png"] },
    PhotoFormat { name: "WebP", extensions: &["webp"], mime_types: &["image/webp"] },
    PhotoFormat { name: "BMP", extensions: &["bmp", "dib"], mime_types: &["image/bmp", "image/x-bmp", "image/x-ms-bmp"] },
    PhotoFormat { name: "JPEG", extensions: &["jpg", "jpeg", "jpe"], mime_types: &["image/jpeg"] },
    PhotoFormat { name: "GIF", extensions: &["gif"], mime_types: &["image/gif"] },
    #[cfg(all(feature = "heif", target_os = "linux"))]
    PhotoFormat { name: "HEIF", extensions: &["heif", "heic", "hif"], mime_types: &["image/heif", "image/heic"] },
    #[cfg(all(feature = "heif", target_os = "linux"))]
    PhotoFormat { name: "AVIF", extensions: &["avif"], mime_types: &["image/avif"] },
];
pub fn formats() -> impl Iterator<Item = &'static PhotoFormat> {
    PHOTO_FORMATS.iter().filter(|_format| {
        #[cfg(all(feature = "heif", target_os = "linux"))]
        if matches!(_format.name, "HEIF" | "AVIF") { return heif_io::available(); }
        true
    })
}
pub fn extensions() -> impl Iterator<Item = &'static str> {
    formats().flat_map(|f| f.extensions.iter().copied())
}
pub fn mime_types() -> impl Iterator<Item = &'static str> {
    formats().flat_map(|f| f.mime_types.iter().copied())
}
pub fn format_names() -> String {
    formats().map(|f| f.name).collect::<Vec<_>>().join(", ")
}

/// An animation is imported as one composited still frame. Hosts using detailed
/// preparation must disclose that choice in the resulting image/layer name.
pub struct DecodedPhoto {
    pub source: SourceImage,
    pub first_frame: bool,
    pub primary_image: bool,
}
impl DecodedPhoto {
    pub fn display_name(&self, name: &str) -> String {
        let suffix = if self.first_frame { " (first frame)" }
            else if self.primary_image { " (primary image)" } else { "" };
        let mut name: String = name.chars().filter(|c| !c.is_control()).take(128 - suffix.len()).collect();
        if name.trim().is_empty() { name = "Image".into(); }
        name.push_str(suffix);
        name
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DecodeLimits {
    pub source_bytes: usize,
    pub codec_bytes: usize,
    pub dimension: u32,
}
impl Default for DecodeLimits {
    fn default() -> Self {
        Self::from_memory_budget(PhotoMemoryBudget::current())
    }
}
impl DecodeLimits {
    pub fn from_memory_budget(budget: PhotoMemoryBudget) -> Self {
        Self {
            source_bytes: budget.source_bytes,
            codec_bytes: budget.decode_bytes,
            dimension: 32768,
        }
    }
}
impl DecodeLimits {
    fn extent(self, extent: [u32; 2]) -> Result<(), String> {
        validate_extent(extent, self.dimension)
    }
}
fn validate_extent(extent: [u32; 2], dimension: u32) -> Result<(), String> {
    if extent.contains(&0) || extent.iter().any(|v| *v > dimension.min(32768)) {
        return Err("This image exceeds the dimension limit".into());
    }
    Ok(())
}

/// Recognition uses file signatures; an extension never changes interpretation.
pub fn read_photo(
    input: impl BufRead + Seek,
    limits: DecodeLimits,
) -> Result<SourceImage, String> {
    read_photo_detailed(input, limits).map(|photo| photo.source)
}

pub fn read_photo_detailed(
    input: impl BufRead + Seek,
    limits: DecodeLimits,
) -> Result<DecodedPhoto, String> {
    read_photo_detailed_with_cancel(input, limits, &std::sync::atomic::AtomicBool::new(false))
}

/// Native codec callbacks and row packing can acknowledge cancellation even
/// after the encoded file has been read. Hosts still wait for worker completion.
pub fn read_photo_detailed_with_cancel(
    input: impl BufRead + Seek,
    limits: DecodeLimits,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<DecodedPhoto, String> {
    let check = || if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        Err("Image read cancelled".to_string())
    } else { Ok(()) };
    check()?;
    let photo = read_photo_impl(input, limits, cancelled)?;
    check()?;
    Ok(photo)
}
fn read_photo_impl(
    mut input: impl BufRead + Seek,
    limits: DecodeLimits,
    _cancelled: &std::sync::atomic::AtomicBool,
) -> Result<DecodedPhoto, String> {
    let origin = input.stream_position().map_err(err)?;
    let mut signature = [0; 8];
    input.read_exact(&mut signature).map_err(err)?;
    input.seek(std::io::SeekFrom::Start(origin)).map_err(err)?;
    let source = if signature == *b"\x89PNG\r\n\x1a\n" {
        png_io::read_with_cancel(input, limits, _cancelled)
    } else if signature[..2] == [0xff, 0xd8] {
        read_jpeg(input, limits)
    } else if matches!(
        &signature[..4],
        b"II\x2a\x00" | b"MM\x00\x2a" | b"II\x2b\x00" | b"MM\x00\x2b"
    ) {
        read_tiff(input, limits)
    } else if &signature[..4] == b"RIFF" {
        return webp_io::read(input, limits);
    } else if matches!(&signature[..6], b"GIF87a" | b"GIF89a") {
        return gif_io::read(input, limits);
    } else if &signature[..2] == b"BM" || bmp_io::dib_signature(&signature) {
        bmp_io::read(input, limits)
    } else if &signature[4..8] == b"ftyp" {
        #[cfg(all(feature = "heif", target_os = "linux"))]
        return heif_io::read(input, limits, _cancelled);
        #[cfg(not(all(feature = "heif", target_os = "linux")))]
        return Err("HEIF/AVIF decoding is not available on this host".into());
    } else {
        Err(format!("Supported photo formats: {}", format_names()))
    }?;
    Ok(DecodedPhoto { source, first_frame: false, primary_image: false })
}

fn interpretation(
    channels: SourceChannels,
    depth: SampleDepth,
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
    validate_extent(extent, 32768)?;
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
mod jpeg_tests;
#[cfg(test)]
mod output_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod raster_tests;
