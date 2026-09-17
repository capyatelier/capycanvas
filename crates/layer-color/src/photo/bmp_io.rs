//! BMP/DIB pixel decoding with V4/V5 interpretation and physical resolution.
use super::raster_io::{self, Input};
use super::*;
use image::{ImageDecoder, codecs::bmp::BmpDecoder};
use std::io::SeekFrom;

pub(super) fn dib_signature(bytes: &[u8]) -> bool {
    bytes.get(..4).is_some_and(|b| {
        matches!(
            u32::from_le_bytes(b.try_into().unwrap()),
            12 | 40 | 52 | 56 | 64 | 108 | 124
        )
    })
}

pub(super) fn read(
    input: impl BufRead + Seek,
    limits: DecodeLimits,
) -> Result<SourceImage, String> {
    let mut input = Input::new(input, limits)?;
    let (file_header, profile, resolution) = metadata(&mut input, limits)?;
    let decoder = BmpDecoder::new(pixel_input(input, file_header)?).map_err(err)?;
    let (w, h) = decoder.dimensions();
    let channels = match decoder.color_type() {
        image::ColorType::Rgb8 => SourceChannels::Rgb,
        image::ColorType::Rgba8 => SourceChannels::Rgba,
        _ => return Err("Unsupported BMP sample layout".into()),
    };
    let bpp = decoder.color_type().bytes_per_pixel() as usize;
    let profile_bytes = match &profile {
        Some(ColorProfile::Icc(bytes)) => bytes.len() as u64,
        _ => 0,
    };
    let bytes = raster_io::frame_bytes([w, h], bpp, 4, profile_bytes, 65536, limits)?;
    let mut pixels = raster_io::allocate(bytes)?;
    decoder.read_image(&mut pixels).map_err(err)?;
    let mut interpretation = interpretation(channels, SampleDepth::U8, None)?;
    if let Some(profile) = profile {
        check_channels(channels, &profile)?;
        interpretation.profile = profile;
        interpretation.profile_assumed = false;
    }
    raster_io::source(
        &pixels,
        [w, h],
        interpretation,
        super::metadata::Exif {
            orientation: 1,
            resolution,
        },
        limits,
    )
}

/// The pinned image 0.25.9 decoder skips twelve extra mask bytes after V4/V5
/// headers, although those headers contain their masks. Present their equivalent
/// V3 pixel header after retaining the original color metadata above. A raw DIB
/// also gets a virtual BITMAPFILEHEADER with its actual pixel offset. Pixels and
/// their source samples are never converted by this header view.
fn pixel_input(
    mut input: Input<impl BufRead + Seek>,
    file_header: bool,
) -> Result<impl BufRead + Seek, String> {
    input.rewind().map_err(err)?;
    let mut header = vec![0; 18];
    if file_header {
        input.read_exact(&mut header[..14]).map_err(err)?;
    }
    input.read_exact(&mut header[14..18]).map_err(err)?;
    let size = u32::from_le_bytes(header[14..18].try_into().unwrap()) as usize;
    header.resize(14 + size, 0);
    input.read_exact(&mut header[18..]).map_err(err)?;
    let bits = u16::from_le_bytes(
        header[if size == 12 { 24..26 } else { 28..30 }]
            .try_into()
            .unwrap(),
    );
    let compression = if size == 12 {
        0
    } else {
        u32::from_le_bytes(header[30..34].try_into().unwrap())
    };
    let inserted = if file_header { 0 } else { 14 };
    if !file_header {
        header[..2].copy_from_slice(b"BM");
        let length =
            u32::try_from(input.length + 14).map_err(|_| "BMP exceeds its file-size range")?;
        header[2..6].copy_from_slice(&length.to_le_bytes());
        let used = if size == 12 {
            0
        } else {
            u32::from_le_bytes(header[46..50].try_into().unwrap())
        };
        let colors = if used != 0 {
            used
        } else if bits <= 8 {
            1 << bits
        } else {
            0
        };
        let mask_bytes = if size == 40 && compression == 3 {
            12
        } else {
            0
        };
        let offset =
            14u64 + size as u64 + mask_bytes + u64::from(colors) * if size == 12 { 3 } else { 4 };
        if offset > input.length + 14 {
            return Err("BMP palette is outside the file".into());
        }
        header[10..14].copy_from_slice(&(offset as u32).to_le_bytes());
    }
    if size >= 108 && compression == 3 {
        header[14..18].copy_from_slice(&56u32.to_le_bytes());
    }
    Ok(std::io::BufReader::new(PixelInput {
        input,
        header,
        position: 0,
        inserted,
    }))
}

struct PixelInput<R> {
    input: Input<R>,
    header: Vec<u8>,
    position: u64,
    inserted: u64,
}
impl<R: Read + Seek> Read for PixelInput<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let count = if self.position < self.header.len() as u64 {
            let from = &self.header[self.position as usize..];
            let count = out.len().min(from.len());
            out[..count].copy_from_slice(&from[..count]);
            count
        } else {
            self.input
                .seek(SeekFrom::Start(self.position - self.inserted))?;
            self.input.read(out)?
        };
        self.position += count as u64;
        Ok(count)
    }
}
impl<R: Read + Seek> Seek for PixelInput<R> {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        let end = self.input.length + self.inserted;
        let position = match from {
            SeekFrom::Start(p) => i128::from(p),
            SeekFrom::Current(p) => i128::from(self.position) + i128::from(p),
            SeekFrom::End(p) => i128::from(end) + i128::from(p),
        };
        if !(0..=i128::from(end)).contains(&position) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "BMP offset is outside the file",
            ));
        }
        self.position = position as u64;
        Ok(self.position)
    }
}

fn metadata(
    input: &mut Input<impl BufRead + Seek>,
    limits: DecodeLimits,
) -> Result<
    (
        bool,
        Option<ColorProfile>,
        Option<layer_core::ImageResolution>,
    ),
    String,
> {
    let mut prefix = [0; 14];
    input.read_exact(&mut prefix[..4]).map_err(err)?;
    let file_header = &prefix[..2] == b"BM";
    let dib_start = if file_header {
        input.read_exact(&mut prefix[4..]).map_err(err)?;
        14
    } else {
        input.rewind().map_err(err)?;
        0
    };
    let mut header = [0; 124];
    input.read_exact(&mut header[..4]).map_err(err)?;
    if !dib_signature(&header) {
        return Err("Unsupported BMP information header".into());
    }
    let size = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
    input.read_exact(&mut header[4..size]).map_err(err)?;
    let u32_at = |at| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
    let i32_at = |at| i32::from_le_bytes(header[at..at + 4].try_into().unwrap());
    let extent = if size == 12 {
        [
            u16::from_le_bytes(header[4..6].try_into().unwrap()) as u32,
            u16::from_le_bytes(header[6..8].try_into().unwrap()) as u32,
        ]
    } else {
        [
            u32::try_from(i32_at(4)).map_err(|_| "Invalid BMP width")?,
            i32_at(8).checked_abs().ok_or("Invalid BMP height")? as u32,
        ]
    };
    limits.extent(extent)?;
    let resolution = if size >= 40 && i32_at(24) > 0 && i32_at(28) > 0 {
        Some(layer_core::ImageResolution {
            unit: layer_core::ResolutionUnit::Metre,
            density: [[u32_at(24), 1], [u32_at(28), 1]],
        })
    } else {
        None
    };
    let profile = if size < 108 {
        None
    } else {
        match u32_at(56) {
            0x73524742 | 0x57696e20 => Some(ColorProfile::default()), // sRGB / Windows
            0 if header[60..108].iter().all(|b| *b == 0) => None,     // legacy untagged V4
            0 => {
                let endpoints = std::array::from_fn(|c| {
                    std::array::from_fn(|i| {
                        f64::from(i32_at(60 + c * 12 + i * 4)) / (1u64 << 30) as f64
                    })
                });
                let gamma = std::array::from_fn(|i| f64::from(u32_at(96 + i * 4)) / 65536.);
                Some(crate::icc::calibrated_rgb_profile(endpoints, gamma)?)
            }
            0x4d424544 if size == 124 => {
                // PROFILE_EMBEDDED
                let start = u64::from(u32_at(112));
                let length = u32_at(116) as usize;
                if length == 0
                    || length > crate::MAX_ICC_BYTES
                    || length > limits.codec_bytes
                    || start < size as u64
                    || start + dib_start + length as u64 > input.length
                {
                    return Err("Invalid BMP embedded ICC profile range".into());
                }
                input
                    .seek(SeekFrom::Start(dib_start + start))
                    .map_err(err)?;
                let mut bytes = raster_io::allocate(length)?;
                input.read_exact(&mut bytes).map_err(err)?;
                let profile = ColorProfile::Icc(bytes.into());
                profile_channels(&profile)?;
                Some(profile)
            }
            0x4c494e4b => {
                return Err("BMP linked ICC profiles need to be embedded before import".into());
            }
            _ => return Err("Unsupported BMP color-space declaration".into()),
        }
    };
    Ok((file_header, profile, resolution))
}
