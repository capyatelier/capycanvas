use super::jpeg_codec;
use super::*;
use libjpeg_turbo_rs::PixelFormat;

pub(super) fn read_jpeg_with_cancel(
    input: impl Read,
    limits: DecodeLimits,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<(SourceImage, layer_core::PhotoMetadata), String> {
    let bytes = jpeg_codec::read_bounded(input, limits.codec_bytes)?;
    let mut metadata = super::jpeg_markers::read_source(&bytes)?;
    let photo = std::mem::take(&mut metadata.photo);
    let source = if metadata.gain_map {
        super::gainmap::read_jpeg(&bytes, bytes.capacity(), limits, cancelled)?
    } else {
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
        builder.finish()?
    };
    let source = super::orientation::normalize(
        source,
        metadata.resolution,
        metadata.orientation.unwrap_or(1),
        limits.source_bytes,
    )?;
    Ok((source, photo))
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
    source.validate()?;
    let mut rows = source.rows();
    write_jpeg_rows(
        output,
        source.extent,
        &source.interpretation,
        &DeliveryMetadata::resolution(source.resolution),
        JpegEncodeOptions::from_memory_budget(quality, PhotoMemoryBudget::current()),
        |y, row| rows.read(y, row),
    )
}

/// Encoded straight 8-bit RGB, gray or CMYK rows. The caller performs color
/// conversion and explicitly flattens transparency before this opaque format.
/// Baseline coding uses full chroma resolution. The Rust backend buffers pixels
/// within the available-memory budget. Provider errors stop requesting rows.
pub fn write_jpeg_rows(
    mut output: impl Write,
    extent: [u32; 2],
    interpretation: &SourceInterpretation,
    metadata: &DeliveryMetadata,
    options: JpegEncodeOptions,
    mut read_row: impl FnMut(u32, &mut [u8]) -> Result<(), String>,
) -> Result<(), String> {
    let row_bytes = output_row_bytes(extent, interpretation)?;
    if interpretation.depth != SampleDepth::U8 {
        return Err("JPEG output requires 8-bit samples".into());
    }
    if interpretation.channels.has_alpha() {
        return Err("JPEG output requires explicit transparency flattening".into());
    }
    if !(1..=100).contains(&options.quality) {
        return Err("JPEG quality must be between 1 and 100".into());
    }
    let format = match interpretation.channels.count() {
        1 => PixelFormat::Grayscale,
        3 => PixelFormat::Rgb,
        4 => PixelFormat::Cmyk,
        _ => return Err("Unsupported JPEG color encoding".into()),
    };
    let icc = delivery_icc(interpretation)?;
    if let Some(resolution) = metadata.resolution {
        resolution.jfif_density()?;
    }
    let exif = metadata.exif(extent)?;
    let xmp = metadata.xmp()?;
    let len = jpeg_codec::admit(
        extent,
        interpretation.channels.count(),
        icc.len() + exif.as_ref().map_or(0, Vec::len) + xmp.as_ref().map_or(0, Vec::len),
        options.codec_bytes,
    )?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(len)
        .map_err(|_| jpeg_codec::MEMORY_ERROR)?;
    pixels.resize(len, 0);
    for (y, row) in pixels.chunks_exact_mut(row_bytes).enumerate() {
        read_row(y as u32, row)?;
        if interpretation.channels == SourceChannels::Cmyk {
            for v in row {
                *v = 255 - *v;
            }
        }
    }
    let bytes = jpeg_codec::encode(
        &pixels,
        extent,
        format,
        options.quality,
        &icc,
        jpeg_codec::Markers { exif: exif.as_deref(), xmp: xmp.as_deref() },
        metadata.resolution,
    )?;
    output.write_all(&bytes).map_err(err)
}
