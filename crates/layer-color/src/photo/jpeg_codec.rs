//! Pure Rust JPEG codec. This backend buffers full images; enforce admission
//! limits before allocating pixels, and account for retained compressed input.
use super::*;
use libjpeg_turbo_rs::{ColorSpace, Decoder, Encoder, PixelFormat, Subsampling};

pub(super) const MEMORY_ERROR: &str = "JPEG exceeds the codec memory budget; use a smaller image";
const SCRATCH_BYTES: usize = 2 * 1024 * 1024;

pub(super) fn read_bounded(mut input: impl Read + Seek, budget: usize) -> Result<Vec<u8>, String> {
    let length = remaining(&mut input)?;
    let length = usize::try_from(length).ok().filter(|n| *n <= budget).ok_or(MEMORY_ERROR)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(|_| MEMORY_ERROR)?;
    bytes.resize(length, 0);
    input.read_exact(&mut bytes).map_err(err)?;
    Ok(bytes)
}

fn remaining(input: &mut impl Seek) -> Result<u64, String> {
    let origin = input.stream_position().map_err(err)?;
    let end = input.seek(std::io::SeekFrom::End(0)).map_err(err)?;
    input.seek(std::io::SeekFrom::Start(origin)).map_err(err)?;
    Ok(end.saturating_sub(origin))
}

pub(super) fn plain_need(input: &mut (impl Read + Seek)) -> Result<Option<usize>, String> {
    let length = remaining(input)?;
    let mut byte = [0u8; 2];
    input.read_exact(&mut byte).map_err(err)?;
    let mut metadata = 0u64;
    loop {
        input.read_exact(&mut byte[..1]).map_err(err)?;
        if byte[0] != 0xff {
            return Err("Invalid JPEG marker".into());
        }
        while byte[0] == 0xff {
            input.read_exact(&mut byte[..1]).map_err(err)?;
        }
        let code = byte[0];
        if matches!(code, 0x01 | 0xd0..=0xd7) {
            continue;
        }
        input.read_exact(&mut byte).map_err(err)?;
        let size = u16::from_be_bytes(byte).checked_sub(2).ok_or("Invalid JPEG marker")? as usize;
        let mut segment = vec![0; size];
        input.read_exact(&mut segment).map_err(err)?;
        match code {
            0xe0..=0xef if super::jpeg_markers::gain_map(code, &segment) => return Ok(None),
            0xe0..=0xef => metadata += size as u64,
            0xc0..=0xcf if !matches!(code, 0xc4 | 0xc8 | 0xcc) => {
                let [_, height, height_low, width, width_low, components] =
                    segment.get(..6).ok_or("Invalid JPEG frame")?.try_into().unwrap();
                let [width, height] = [[width, width_low], [height, height_low]].map(|v| u64::from(u16::from_be_bytes(v)));
                let (pixels, components) = (width * height, u64::from(components));
                let channels = match components { 1 => SourceChannels::Gray, 4 => SourceChannels::Cmyk, _ => SourceChannels::Rgb };
                let output = channels.count() as u64;
                let estimate = pixels * (output + components)
                    + if matches!(code, 0xc2 | 0xca) { pixels * 2 * components + pixels * components / 64 } else { 0 };
                let interpretation = SourceInterpretation { channels, depth: SampleDepth::U8, profile: ColorProfile::default(), profile_assumed: true };
                let tile = layer_core::raster::TILE_SIZE as u64;
                let tiles = width.div_ceil(tile) * height.div_ceil(tile)
                    * layer_core::raster::TileBlob::max_compressed_len(interpretation.descriptor()).ok_or(MEMORY_ERROR)? as u64;
                let decoding = estimate.max(pixels * output + tiles).max(2 * tiles);
                let need = length + 3 * metadata + SCRATCH_BYTES as u64 + width * output * tile + decoding;
                return usize::try_from(need).map(Some).map_err(|_| MEMORY_ERROR.into());
            }
            0xd9 | 0xda => return Err("Invalid JPEG frame".into()),
            _ => {}
        }
    }
}

pub(super) fn decoder(
    bytes: &[u8],
    held: usize,
    limits: DecodeLimits,
) -> Result<(Decoder<'_>, usize), String> {
    let metadata: usize = super::jpeg_markers::scan(bytes)?.1.iter().map(|segment| segment.bytes.len()).sum();
    let budget = limits
        .codec_bytes
        .checked_sub(
            held.saturating_add(metadata.saturating_mul(3))
                .saturating_add(SCRATCH_BYTES),
        )
        .ok_or(MEMORY_ERROR)?;
    let mut decoder = Decoder::new_with_limits(
        bytes,
        libjpeg_turbo_rs::DecodeLimits {
            max_width: limits.dimension.min(layer_core::MAX_EXTENT) as usize,
            max_height: limits.dimension.min(layer_core::MAX_EXTENT) as usize,
            max_scans: 256,
            max_memory: Some(budget as u64),
            ..Default::default()
        },
    )
    .map_err(err)?;
    decoder.set_stop_on_warning(true);
    decoder.set_dct_method(libjpeg_turbo_rs::DctMethod::IsLow);
    Ok((decoder, budget))
}

pub(super) fn channels(
    decoder: &Decoder<'_>,
    adobe: Option<u8>,
) -> Result<(SourceChannels, PixelFormat), String> {
    match decoder.jpeg_color_space() {
        ColorSpace::Grayscale => Ok((SourceChannels::Gray, PixelFormat::Grayscale)),
        ColorSpace::YCbCr | ColorSpace::Rgb => Ok((SourceChannels::Rgb, PixelFormat::Rgb)),
        ColorSpace::Cmyk | ColorSpace::Ycck if matches!(adobe, Some(0 | 2)) => {
            Ok((SourceChannels::Cmyk, PixelFormat::Cmyk))
        }
        ColorSpace::Cmyk | ColorSpace::Ycck => {
            Err("This CMYK JPEG needs an explicit sample-polarity interpretation".into())
        }
        _ => Err("Unsupported JPEG color encoding".into()),
    }
}

/// Pixels, coding planes, entropy output and metadata insertion copies coexist
/// within the caller's available-memory budget. Returns the packed pixel length.
pub(super) fn admit(
    extent: [u32; 2],
    channels: usize,
    metadata: usize,
    budget: usize,
) -> Result<usize, String> {
    super::validate_extent(extent, JPEG_MAX_DIMENSION)?;
    let len = (extent[0] as usize)
        .checked_mul(extent[1] as usize)
        .and_then(|n| n.checked_mul(channels))
        .ok_or(MEMORY_ERROR)?;
    let working = len
        .checked_mul(8)
        .and_then(|v| v.checked_add(SCRATCH_BYTES))
        .ok_or(MEMORY_ERROR)?
        .saturating_add(metadata.saturating_mul(4));
    if working > budget {
        return Err(MEMORY_ERROR.into());
    }
    Ok(len)
}

/// APP1 payloads: an Exif TIFF block and an XMP packet, without identifiers.
#[derive(Clone, Copy, Default)]
pub(super) struct Markers<'a> {
    pub exif: Option<&'a [u8]>,
    pub xmp: Option<&'a [u8]>,
}
pub(super) const METADATA_TOO_LARGE: &str =
    "This photo's metadata is too large for JPEG. Choose Copyright & Contact or None under Metadata.";

/// Baseline, full-chroma coding of packed 8-bit samples.
pub(super) fn encode(
    pixels: &[u8],
    extent: [u32; 2],
    format: PixelFormat,
    quality: u8,
    icc: &[u8],
    markers: Markers<'_>,
    resolution: Option<layer_core::ImageResolution>,
) -> Result<Vec<u8>, String> {
    let [width, height] = extent.map(|v| v as usize);
    if pixels.len() != width * height * format.bytes_per_pixel() {
        return Err("Incomplete JPEG output".into());
    }
    if icc.len() > crate::MAX_ICC_BYTES.min(255 * 65519) {
        return Err("JPEG ICC profile is too large".into());
    }
    let mut encoder = Encoder::new(pixels, width, height, format)
        .quality(quality)
        .subsampling(Subsampling::S444)
        .force_baseline(true)
        .icc_profile(icc);
    for (payload, identifier) in [(markers.exif, 6), (markers.xmp, super::jpeg_markers::XMP.len())] {
        if payload.is_some_and(|p| p.len() + identifier > 65533) {
            return Err(METADATA_TOO_LARGE.into());
        }
    }
    if let Some(exif) = markers.exif {
        encoder = encoder.exif_data(exif);
    }
    if let Some(xmp) = markers.xmp {
        encoder = encoder.xmp_data(xmp);
    }
    if let Some(resolution) = resolution {
        let (unit, [x, y]) = resolution.jfif_density()?;
        encoder = encoder.density(unit, x, y);
    }
    encoder.encode().map_err(err)
}
