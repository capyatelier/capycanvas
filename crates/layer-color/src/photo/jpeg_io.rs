use super::jpeg_codec::{Decoder, Encoder};
use super::*;
use std::io::{BufReader, SeekFrom};

pub fn read_jpeg(mut input: impl Read + Seek, limits: DecodeLimits) -> Result<SourceImage, String> {
    let origin = input.stream_position().map_err(err)?;
    // Bound preflight work independently of working pixels. The compressed
    // stream is not retained; this ceiling also bounds arbitrary APP padding.
    let max_input = limits.source_bytes.saturating_add(limits.codec_bytes);
    let metadata = super::jpeg_markers::read(BufReader::new((&mut input).take(max_input as u64)))?;
    input.seek(SeekFrom::Start(origin)).map_err(err)?;
    let mut decoder = Decoder::new(input)?;
    let extent = [decoder.info.width, decoder.info.height];
    limits.extent(extent)?;
    if decoder.info.precision != 8 {
        return Err("SDR JPEG import requires 8-bit samples".into());
    }
    let channels = match decoder.info.channels {
        1 => SourceChannels::Gray,
        3 => SourceChannels::Rgb,
        4 if decoder.info.adobe != 0 && matches!(decoder.info.transform, 0 | 2) => {
            SourceChannels::Cmyk
        }
        4 => return Err("This CMYK JPEG needs an explicit sample-polarity interpretation".into()),
        _ => return Err("Unsupported JPEG color encoding".into()),
    };
    let interpretation = interpretation(channels, IntegerDepth::U8, metadata.profile)?;
    let mut builder = SourceBuilder::new(extent, interpretation, limits.source_bytes)?;
    decoder.start(limits.codec_bytes)?;
    let mut row = vec![0; extent[0] as usize * channels.count()];
    for _ in 0..extent[1] {
        decoder.row(&mut row)?;
        if channels == SourceChannels::Cmyk {
            // Adobe CMYK/YCCK uses inverted ink values. Retained source/CMM
            // channels use conventional 0 = no ink, 255 = full ink.
            for v in &mut row {
                *v = 255 - *v;
            }
        }
        builder.push_row(&row)?;
    }
    decoder.finish()?;
    super::orientation::normalize(
        builder.finish()?,
        metadata.orientation.unwrap_or(1),
        limits.source_bytes,
    )
}

pub fn write_jpeg(output: impl Write, source: &SourceImage, quality: u8) -> Result<(), String> {
    source.validate()?;
    let mut rows = source.rows();
    write_jpeg_rows(
        output,
        source.extent,
        &source.interpretation,
        quality,
        |y, row| rows.read(y, row),
    )
}

/// Encoded straight 8-bit RGB, gray or CMYK rows. The caller performs color
/// conversion and explicitly flattens transparency before this opaque format.
/// Baseline coding uses full chroma resolution at every quality, and bounded
/// row/I/O storage. Cancellation/errors abort without requesting further rows.
pub fn write_jpeg_rows(
    output: impl Write,
    extent: [u32; 2],
    interpretation: &SourceInterpretation,
    quality: u8,
    mut read_row: impl FnMut(u32, &mut [u8]) -> Result<(), String>,
) -> Result<(), String> {
    let row_bytes = output_row_bytes(extent, interpretation)?;
    if interpretation.depth != IntegerDepth::U8 {
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
    let mut encoder = Encoder::new(output, extent, interpretation.channels.count(), quality)?;
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
