use super::*;
use tiff::{
    ColorType,
    decoder::{ChunkType, DecodingResult},
    encoder::colortype,
    tags::Tag,
};

pub fn read_tiff(input: impl Read + Seek, limits: DecodeLimits) -> Result<SourceImage, String> {
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
        return Err("Multi-page TIFF needs a page-selection import".into());
    }
    if decoder
        .find_tag_unsigned::<u16>(Tag::PlanarConfiguration)
        .map_err(err)?
        .unwrap_or(1)
        != 1
    {
        return Err("Planar TIFF is not supported by the pinned chunk decoder".into());
    }
    let orientation = decoder
        .find_tag_unsigned::<u16>(Tag::Orientation)
        .map_err(err)?
        .unwrap_or(1);
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
        8 => IntegerDepth::U8,
        16 => IntegerDepth::U16,
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
                DecodingResult::U8(v) if depth == IntegerDepth::U8 => v,
                DecodingResult::U16(v) if depth == IntegerDepth::U16 => {
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
    super::orientation::normalize(builder.finish()?, orientation, limits.source_bytes)
}

pub fn write_tiff(mut output: impl Write + Seek, source: &SourceImage) -> Result<(), String> {
    source.validate()?;
    check_channels(
        source.interpretation.channels,
        &source.interpretation.profile,
    )?;
    let profile = if matches!(
        source.interpretation.channels,
        SourceChannels::Gray | SourceChannels::GrayAlpha
    ) && source.interpretation.profile == ColorProfile::default()
    {
        crate::icc::matrix_profile(
            RgbSpace::Srgb.white(),
            RgbSpace::Srgb.primaries(),
            None,
            true,
        )?
    } else {
        source.interpretation.profile.clone()
    };
    let icc = profile_bytes(&profile)?;
    let mut encoder = tiff::encoder::TiffEncoder::new(&mut output).map_err(err)?;
    macro_rules! write {
        ($ty:ty, $u16:tt) => {{
            let mut image = encoder
                .new_image::<$ty>(source.extent[0], source.extent[1])
                .map_err(err)?;
            image
                .encoder()
                .write_tag(Tag::IccProfile, icc.as_slice())
                .map_err(err)?;
            if source.interpretation.channels.has_alpha() {
                image
                    .encoder()
                    .write_tag(Tag::ExtraSamples, &[2u16][..])
                    .map_err(err)?;
            }
            image.rows_per_strip(1).map_err(err)?;
            let mut rows = source.rows();
            let mut row = vec![0; source.row_bytes()];
            for y in 0..source.extent[1] {
                rows.read(y, &mut row)?;
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
            let codes: Vec<u16> = $row
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect();
            $image.write_strip(&codes).map_err(err)?;
        }};
    }
    let result = match (source.interpretation.channels, source.interpretation.depth) {
        (SourceChannels::Gray, IntegerDepth::U8) => write!(colortype::Gray8, false),
        (SourceChannels::Gray, IntegerDepth::U16) => write!(colortype::Gray16, true),
        (SourceChannels::Rgb, IntegerDepth::U8) => write!(colortype::RGB8, false),
        (SourceChannels::Rgb, IntegerDepth::U16) => write!(colortype::RGB16, true),
        (SourceChannels::Rgba, IntegerDepth::U8) => write!(colortype::RGBA8, false),
        (SourceChannels::Rgba, IntegerDepth::U16) => write!(colortype::RGBA16, true),
        (SourceChannels::Cmyk, IntegerDepth::U8) => write!(colortype::CMYK8, false),
        (SourceChannels::Cmyk, IntegerDepth::U16) => write!(colortype::CMYK16, true),
        _ => Err("Grayscale-alpha TIFF output is not yet supported".into()),
    };
    result?;
    drop(encoder);
    output.flush().map_err(err)
}
