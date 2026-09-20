use super::jpeg_codec::{self, Encoder};
use super::*;
use std::io::{BufReader, SeekFrom};

pub fn read_jpeg(input: impl Read + Seek, limits: DecodeLimits) -> Result<SourceImage, String> {
    read_jpeg_with_cancel(input, limits, &std::sync::atomic::AtomicBool::new(false))
}
pub(super) fn read_jpeg_with_cancel(mut input: impl Read + Seek, limits: DecodeLimits, cancelled: &std::sync::atomic::AtomicBool) -> Result<SourceImage, String> {
    let origin = input.stream_position().map_err(err)?;
    // Bound preflight work independently of working pixels. The compressed
    // stream and full decoded pixels are budgeted by the portable backend.
    let max_input = limits.codec_bytes;
    let mut preflight = (&mut input).take(max_input as u64);
    let metadata = super::jpeg_markers::read_source(BufReader::new(&mut preflight)).map_err(|error| {
        if preflight.limit() == 0 {
            jpeg_codec::MEMORY_ERROR.into()
        } else {
            error
        }
    })?;
    input.seek(SeekFrom::Start(origin)).map_err(err)?;
    if metadata.gain_map {
        { let mut source=super::gainmap::read_gainmap(input,GainMapFormat::Jpeg,limits,cancelled)?;
          source.resolution=metadata.resolution;
          return super::orientation::normalize(source,metadata.orientation.unwrap_or(1),limits.source_bytes); }
    }
    let bytes = jpeg_codec::read_bounded(input, limits.codec_bytes)?;
    let mut decoder = jpeg_codec::decoder(&bytes, bytes.capacity(), limits)?;
    let extent = [
        u32::from(decoder.header().width),
        u32::from(decoder.header().height),
    ];
    limits.extent(extent)?;
    if decoder.header().precision != 8 {
        return Err("SDR JPEG import requires 8-bit samples".into());
    }
    let (channels, format) = jpeg_codec::channels(&decoder, metadata.adobe_transform)?;
    decoder.set_output_format(format);
    decoder
        .output_buffer_size()
        .map_err(|e| format!("{}: {e}", jpeg_codec::MEMORY_ERROR))?;
    let interpretation = interpretation(channels, SampleDepth::U8, metadata.profile)?;
    let mut builder = SourceBuilder::new(extent, interpretation, limits.source_bytes)?;
    let image = decoder.decode_image().map_err(err)?;
    let mut row = vec![0; extent[0] as usize * channels.count()];
    for samples in image.data.chunks_exact(row.len()) {
        row.copy_from_slice(samples);
        if channels == SourceChannels::Cmyk {
            // JPEG's Adobe ink values are inverted; retained source samples use
            // conventional 0 = no ink, 255 = full ink on every platform.
            for v in &mut row {
                *v = 255 - *v;
            }
        }
        builder.push_row(&row)?;
    }
    let mut source = builder.finish()?;
    source.resolution = metadata.resolution;
    super::orientation::normalize(
        source,
        metadata.orientation.unwrap_or(1),
        limits.source_bytes,
    )
}

/// JPEG output admission settings. Hosts can pass their current process memory
/// allowance instead of relying on a native query or the browser fallback.
#[derive(Clone, Copy, Debug)]
pub struct JpegEncodeOptions {
    pub quality: u8,
    pub codec_bytes: usize,
}
impl JpegEncodeOptions {
    pub fn from_memory_budget(quality: u8, budget: PhotoMemoryBudget) -> Self {
        Self {
            quality,
            codec_bytes: budget.encode_bytes,
        }
    }
}

pub fn write_jpeg(output: impl Write, source: &SourceImage, quality: u8) -> Result<(), String> {
    write_jpeg_with_options(
        output,
        source,
        JpegEncodeOptions::from_memory_budget(quality, PhotoMemoryBudget::current()),
    )
}

pub fn write_jpeg_with_options(
    output: impl Write,
    source: &SourceImage,
    options: JpegEncodeOptions,
) -> Result<(), String> {
    source.validate()?;
    let mut rows = source.rows();
    write_jpeg_rows_with_options(
        output,
        source.extent,
        &source.interpretation,
        source.resolution,
        options,
        |y, row| rows.read(y, row),
    )
}

/// Encoded straight 8-bit RGB, gray or CMYK rows. The caller performs color
/// conversion and explicitly flattens transparency before this opaque format.
/// Baseline coding uses full chroma resolution. The Rust backend buffers pixels
/// within the available-memory budget. Provider errors stop requesting rows.
pub fn write_jpeg_rows(
    output: impl Write,
    extent: [u32; 2],
    interpretation: &SourceInterpretation,
    resolution: Option<layer_core::ImageResolution>,
    quality: u8,
    read_row: impl FnMut(u32, &mut [u8]) -> Result<(), String>,
) -> Result<(), String> {
    write_jpeg_rows_with_options(
        output,
        extent,
        interpretation,
        resolution,
        JpegEncodeOptions::from_memory_budget(quality, PhotoMemoryBudget::current()),
        read_row,
    )
}

pub fn write_jpeg_rows_with_options(
    output: impl Write,
    extent: [u32; 2],
    interpretation: &SourceInterpretation,
    resolution: Option<layer_core::ImageResolution>,
    options: JpegEncodeOptions,
    mut read_row: impl FnMut(u32, &mut [u8]) -> Result<(), String>,
) -> Result<(), String> {
    let quality = options.quality;
    let row_bytes = output_row_bytes(extent, interpretation)?;
    if interpretation.depth != SampleDepth::U8 {
        return Err("JPEG output requires 8-bit samples".into());
    }
    if interpretation.channels.has_alpha() {
        return Err("JPEG output requires explicit transparency flattening".into());
    }
    if !(1..=100).contains(&quality) {
        return Err("JPEG quality must be between 1 and 100".into());
    }
    let profile = if interpretation.channels == SourceChannels::Gray
        && let ColorProfile::Builtin(space) = interpretation.profile
    {
        crate::gray_profile(space)?
    } else {
        interpretation.profile.clone()
    };
    let icc = profile_bytes(&profile)?;
    let mut encoder = Encoder::new(
        output,
        extent,
        interpretation.channels.count(),
        quality,
        resolution,
        options.codec_bytes,
    )?;
    if let Some(resolution) = resolution {
        encoder.marker(1, &super::metadata::exif_output(resolution)?)?;
    }
    encoder.profile(&icc)?;
    let mut row = vec![0; row_bytes];
    for y in 0..extent[1] {
        read_row(y, &mut row)?;
        if interpretation.channels == SourceChannels::Cmyk {
            for v in &mut row {
                *v = 255 - *v;
            }
        }
        encoder.row(&row)?;
    }
    encoder.finish()
}
