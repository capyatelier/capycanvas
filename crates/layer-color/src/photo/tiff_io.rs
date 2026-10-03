use super::exif::Entry;
use super::*;
use std::borrow::Cow;
use tiff::{
    ColorType,
    decoder::{ChunkType, DecodingResult},
    encoder::{TiffValue, colortype},
    tags::{Tag, Type},
};

/// The descriptive metadata of a TIFF file's first directory.
fn photo_metadata(input: &mut (impl Read + Seek)) -> Result<layer_core::PhotoMetadata, String> {
    let origin = input.stream_position().map_err(err)?;
    let end = input.seek(std::io::SeekFrom::End(0)).map_err(err)?;
    let found = super::exif::read(input, origin, end - origin).ok();
    input.seek(std::io::SeekFrom::Start(origin)).map_err(err)?;
    Ok(super::metadata::collect(found, None, None))
}

pub(super) fn read_tiff(
    mut input: impl Read + Seek,
    limits: DecodeLimits,
) -> Result<(SourceImage, layer_core::PhotoMetadata), String> {
    let photo = photo_metadata(&mut input)?;
    let mut codec_limits = tiff::decoder::Limits::default();
    codec_limits.decoding_buffer_size = limits.codec_bytes;
    codec_limits.intermediate_buffer_size = limits.codec_bytes;
    codec_limits.ifd_value_size = crate::MAX_ICC_BYTES;
    let mut decoder = tiff::decoder::Decoder::new(input)
        .map_err(err)?
        .with_limits(codec_limits);
    let (w, h) = decoder.dimensions().map_err(err)?;
    let extent = [w, h];
    limits.extent(extent)?;
    if decoder.more_images() {
        return Err("Multi-page TIFF is not supported. Export the intended page as a separate SDR TIFF or PNG.".into());
    }
    if decoder
        .find_tag_unsigned::<u16>(Tag::PlanarConfiguration)
        .map_err(err)?
        .unwrap_or(1)
        != 1
    {
        return Err("Planar TIFF is not supported. Export an interleaved (chunky) TIFF or PNG.".into());
    }
    let orientation = decoder
        .find_tag_unsigned::<u16>(Tag::Orientation)
        .map_err(err)?
        .unwrap_or(1);
    let mut density = [None; 2];
    for (out, tag) in density.iter_mut().zip([Tag::XResolution, Tag::YResolution]) {
        if let Some(tiff::decoder::ifd::Value::Rational(n, d)) =
            decoder.find_tag(tag).map_err(err)?
        {
            *out = Some([n, d]);
        }
    }
    let resolution = super::metadata::physical(
        decoder
            .find_tag_unsigned::<u16>(Tag::ResolutionUnit)
            .map_err(err)?
            .unwrap_or(2),
        density,
    );
    if decoder
        .find_tag_unsigned_vec::<u16>(Tag::SampleFormat)
        .map_err(err)?
        .is_some_and(|formats| formats.iter().any(|f| *f != 1))
    {
        return Err("SDR TIFF import requires unsigned integer samples".into());
    }
    let (channels, bits) = match decoder.colortype().map_err(err)? {
        ColorType::Gray(bits) => (SourceChannels::Gray, bits),
        ColorType::GrayA(bits) => (SourceChannels::GrayAlpha, bits),
        // The pinned decoder reports gray alpha as Multiband. Only accept
        // the standard black-is-zero layout with one explicitly straight alpha.
        ColorType::Multiband {
            bit_depth,
            num_samples: 2,
        } if decoder
            .find_tag_unsigned::<u16>(Tag::PhotometricInterpretation)
            .map_err(err)?
            == Some(1)
            && decoder
                .find_tag_unsigned_vec::<u16>(Tag::ExtraSamples)
                .map_err(err)?
                == Some(vec![2]) =>
        {
            (SourceChannels::GrayAlpha, bit_depth)
        }
        ColorType::RGB(bits) => (SourceChannels::Rgb, bits),
        ColorType::RGBA(bits) => (SourceChannels::Rgba, bits),
        ColorType::CMYK(bits) => (SourceChannels::Cmyk, bits),
        _ => return Err("Unsupported TIFF channel layout".into()),
    };
    if channels.has_alpha()
        && decoder
            .find_tag_unsigned_vec::<u16>(Tag::ExtraSamples)
            .map_err(err)?
            != Some(vec![2])
    {
        return Err("TIFF import currently requires explicitly unassociated alpha".into());
    }
    let depth = match bits {
        8 => SampleDepth::U8,
        16 => SampleDepth::U16,
        _ => return Err("TIFF SDR sample depth must be 8 or 16 bits".into()),
    };
    let profile = decoder
        .find_tag(Tag::IccProfile)
        .map_err(err)?
        .map(|v| v.into_u8_vec().map_err(err))
        .transpose()?;
    let interpretation = interpretation(channels, depth, profile)?;
    let pixel_bytes = interpretation.pixel_bytes();
    let row_bytes = w as usize * pixel_bytes;
    let mut builder = SourceBuilder::new(extent, interpretation, limits.source_bytes)?;
    let count = match decoder.get_chunk_type() {
        ChunkType::Strip => decoder.strip_count(),
        ChunkType::Tile => decoder.tile_count(),
    }
    .map_err(err)?;
    let (chunk_w, chunk_h) = decoder.chunk_dimensions();
    if chunk_w == 0 || chunk_h == 0 {
        return Err("Invalid TIFF chunk dimensions".into());
    }
    let columns = w.div_ceil(chunk_w);
    let chunk_rows = h.div_ceil(chunk_h);
    if columns.checked_mul(chunk_rows) != Some(count) {
        return Err("TIFF chunks do not cover the image".into());
    }
    for cy in 0..chunk_rows {
        let height = chunk_h.min(h - cy * chunk_h);
        let size = row_bytes
            .checked_mul(height as usize)
            .filter(|n| *n <= limits.codec_bytes)
            .ok_or("TIFF chunk band exceeds the codec memory budget")?;
        let mut band = vec![0; size];
        for cx in 0..columns {
            let index = cy * columns + cx;
            let (data_w, data_h) = decoder.chunk_data_dimensions(index);
            let decoded = decoder.read_chunk(index).map_err(err)?;
            let bytes = match decoded {
                DecodingResult::U8(v) if depth == SampleDepth::U8 => v,
                DecodingResult::U16(v) if depth == SampleDepth::U16 => {
                    v.into_iter().flat_map(u16::to_le_bytes).collect()
                }
                _ => return Err("TIFF decoder changed the declared sample depth".into()),
            };
            let width = chunk_w.min(w - cx * chunk_w) as usize * pixel_bytes;
            if data_h < height
                || data_w as usize * pixel_bytes < width
                || bytes.len() != data_w as usize * data_h as usize * pixel_bytes
            {
                return Err("Incomplete TIFF chunk".into());
            }
            for y in 0..height as usize {
                let from = y * data_w as usize * pixel_bytes;
                let to = y * row_bytes + cx as usize * chunk_w as usize * pixel_bytes;
                band[to..to + width].copy_from_slice(&bytes[from..from + width]);
            }
        }
        for row in band.chunks_exact(row_bytes) {
            builder.push_row(row)?;
        }
    }
    let source = super::orientation::normalize(builder.finish()?, resolution, orientation, limits.source_bytes)?;
    Ok((source, photo))
}

/// A directory entry written with its own TIFF type.
macro_rules! entry_values {
    ($($name:ident = $ty:ident, $len:literal;)*) => {
        $(
            struct $name<'a>(&'a Entry);
            impl TiffValue for $name<'_> {
                const BYTE_LEN: u8 = $len;
                const FIELD_TYPE: Type = Type::$ty;
                fn count(&self) -> usize {
                    self.0.count as usize
                }
                fn bytes(&self) -> usize {
                    self.0.value.len()
                }
                fn data(&self) -> Cow<'_, [u8]> {
                    Cow::Borrowed(&self.0.value)
                }
            }
        )*
        fn write_entry<W: Write + Seek>(
            directory: &mut tiff::encoder::DirectoryEncoder<'_, W, tiff::encoder::TiffKindStandard>,
            entry: &Entry,
        ) -> Result<(), String> {
            let tag = Tag::from_u16_exhaustive(entry.tag);
            match entry.kind {
                $(k if k == Type::$ty.to_u16() => directory.write_tag(tag, $name(entry)),)*
                _ => return Err("Unsupported Exif entry type".into()),
            }
            .map_err(err)
        }
    };
}
entry_values! {
    Bytes = BYTE, 1; Ascii = ASCII, 1; Shorts = SHORT, 2; Longs = LONG, 4;
    Rationals = RATIONAL, 8; SignedBytes = SBYTE, 1; Undefined = UNDEFINED, 1;
    SignedShorts = SSHORT, 2; SignedLongs = SLONG, 4; SignedRationals = SRATIONAL, 8;
    Floats = FLOAT, 4; Doubles = DOUBLE, 8;
}

// The pinned TIFF encoder exposes RGB alpha types but no gray-alpha types.
// Describe the standard two-sample layout; ExtraSamples=2 is written below.
macro_rules! gray_alpha {
    ($name:ident, $sample:ty, $bits:literal) => {
        struct $name;
        impl colortype::ColorType for $name {
            type Inner = $sample;
            const TIFF_VALUE: tiff::tags::PhotometricInterpretation =
                tiff::tags::PhotometricInterpretation::BlackIsZero;
            const BITS_PER_SAMPLE: &'static [u16] = &[$bits, $bits];
            const SAMPLE_FORMAT: &'static [tiff::tags::SampleFormat] =
                &[tiff::tags::SampleFormat::Uint; 2];
            fn horizontal_predict(row: &[$sample], result: &mut Vec<$sample>) {
                result.extend(row.iter().enumerate().map(|(i, &value)| {
                    if i < 2 {
                        value
                    } else {
                        value.wrapping_sub(row[i - 2])
                    }
                }));
            }
        }
    };
}
gray_alpha!(GrayAlpha8, u8, 8);
gray_alpha!(GrayAlpha16, u16, 16);

pub fn write_tiff(output: impl Write + Seek, source: &SourceImage) -> Result<(), String> {
    source.validate()?;
    let mut rows = source.rows();
    write_tiff_rows(
        output,
        source.extent,
        &source.interpretation,
        &DeliveryMetadata::resolution(source.resolution),
        |y, row| rows.read(y, row),
    )
}

/// Stream top-to-bottom encoded rows, with little-endian integer16 samples.
/// See `write_png_rows` for cancellation and temporary-file publication rules.
pub fn write_tiff_rows(
    mut output: impl Write + Seek,
    extent: [u32; 2],
    interpretation: &SourceInterpretation,
    metadata: &DeliveryMetadata,
    mut read_row: impl FnMut(u32, &mut [u8]) -> Result<(), String>,
) -> Result<(), String> {
    let row_bytes = output_row_bytes(extent, interpretation)?;
    let icc = delivery_icc(interpretation)?;
    let density = metadata
        .resolution
        .map(layer_core::ImageResolution::tiff_density)
        .transpose()?;
    let directories = metadata.directories(extent)?;
    let xmp = metadata.xmp()?;
    let metadata_bytes = [&directories.image, &directories.exif, &directories.gps]
        .into_iter()
        .flatten()
        .map(|e| e.value.len() + 12)
        .sum::<usize>()
        + xmp.as_ref().map_or(0, Vec::len);
    // Classic TIFF uses 32-bit offsets. Bound uncompressed payload, per-row
    // strip tables/alignment, ICC and fixed tags before requesting any pixels.
    let bound = u64::from(extent[1]) * (row_bytes as u64 + 16)
        + icc.len() as u64
        + metadata_bytes as u64
        + 4096;
    if bound > u64::from(u32::MAX) {
        return Err("This image exceeds classic TIFF's 4 GiB file limit. Choose PNG or smaller output dimensions; BigTIFF export is not supported.".into());
    }
    let mut encoder = tiff::encoder::TiffEncoder::new(&mut output).map_err(err)?;
    let mut pointers = Vec::new();
    for (tag, entries) in [(Tag::ExifDirectory, &directories.exif), (Tag::GpsDirectory, &directories.gps)] {
        if entries.is_empty() {
            continue;
        }
        let mut directory = encoder.extra_directory().map_err(err)?;
        for entry in entries {
            write_entry(&mut directory, entry)?;
        }
        pointers.push((tag, directory.finish_with_offsets().map_err(err)?.offset));
    }
    let mut codes = Vec::<u16>::new();
    macro_rules! write {
        ($ty:ty, $u16:tt) => {{
            let mut image = encoder
                .new_image::<$ty>(extent[0], extent[1])
                .map_err(err)?;
            if let Some((unit, density)) = density {
                image
                    .encoder()
                    .write_tag(Tag::ResolutionUnit, unit)
                    .map_err(err)?;
                for (tag, [n, d]) in [Tag::XResolution, Tag::YResolution]
                    .into_iter()
                    .zip(density)
                {
                    image
                        .encoder()
                        .write_tag(tag, tiff::encoder::Rational { n, d })
                        .map_err(err)?;
                }
            }
            image
                .encoder()
                .write_tag(Tag::IccProfile, icc.as_slice())
                .map_err(err)?;
            for entry in &directories.image {
                write_entry(image.encoder(), entry)?;
            }
            for (tag, offset) in &pointers {
                image.encoder().write_tag(tag.clone(), *offset).map_err(err)?;
            }
            if let Some(xmp) = &xmp {
                image.encoder().write_tag(Tag::Unknown(700), xmp.as_slice()).map_err(err)?;
            }
            if interpretation.channels.has_alpha() {
                image
                    .encoder()
                    .write_tag(Tag::ExtraSamples, &[2u16][..])
                    .map_err(err)?;
            }
            image.rows_per_strip(1).map_err(err)?;
            let mut row = vec![0; row_bytes];
            for y in 0..extent[1] {
                read_row(y, &mut row)?;
                write_tiff_row!(&mut image, &row, $u16);
            }
            image.finish().map_err(err)
        }};
    }
    macro_rules! write_tiff_row {
        ($image:expr, $row:expr, false) => {
            $image.write_strip($row).map_err(err)?
        };
        ($image:expr, $row:expr, true) => {{
            codes.clear();
            codes.extend(
                $row.chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]])),
            );
            $image.write_strip(&codes).map_err(err)?;
        }};
    }
    let result = match (interpretation.channels, interpretation.depth) {
        (_, SampleDepth::F16 | SampleDepth::F32) => return Err("TIFF HDR delivery is not supported; select PQ PNG or an SDR rendition".into()),
        (SourceChannels::Gray, SampleDepth::U8) => write!(colortype::Gray8, false),
        (SourceChannels::Gray, SampleDepth::U16) => write!(colortype::Gray16, true),
        (SourceChannels::GrayAlpha, SampleDepth::U8) => write!(GrayAlpha8, false),
        (SourceChannels::GrayAlpha, SampleDepth::U16) => write!(GrayAlpha16, true),
        (SourceChannels::Rgb, SampleDepth::U8) => write!(colortype::RGB8, false),
        (SourceChannels::Rgb, SampleDepth::U16) => write!(colortype::RGB16, true),
        (SourceChannels::Rgba, SampleDepth::U8) => write!(colortype::RGBA8, false),
        (SourceChannels::Rgba, SampleDepth::U16) => write!(colortype::RGBA16, true),
        (SourceChannels::Cmyk, SampleDepth::U8) => write!(colortype::CMYK8, false),
        (SourceChannels::Cmyk, SampleDepth::U16) => write!(colortype::CMYK16, true),
    };
    result?;
    output.flush().map_err(err)
}
