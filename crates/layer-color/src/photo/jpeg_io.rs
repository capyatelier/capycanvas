use super::*;
use zune_jpeg::zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};

pub fn read_jpeg(input: impl Read + Seek, limits: DecodeLimits) -> Result<SourceImage, String> {
    // The pinned JPEG decoder materializes output. Account and limit compressed
    // input and decoded output independently; tile ownership follows decoding.
    let mut input_bytes = Vec::new();
    input
        .take(limits.codec_bytes as u64 + 1)
        .read_to_end(&mut input_bytes)
        .map_err(err)?;
    if input_bytes.len() > limits.codec_bytes {
        return Err("JPEG input exceeds the codec memory budget".into());
    }
    let profile = embedded_profile(&input_bytes)?;
    let options = DecoderOptions::default()
        .set_strict_mode(true)
        .set_max_width(limits.dimension as usize)
        .set_max_height(limits.dimension as usize);
    let mut decoder =
        zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(input_bytes.as_slice()), options);
    decoder.decode_headers().map_err(err)?;
    let orientation = decoder
        .exif()
        .map(|v| super::orientation::exif(v))
        .transpose()?
        .unwrap_or(1);
    let (w, h) = decoder.dimensions().ok_or("JPEG has no dimensions")?;
    let extent = [w as u32, h as u32];
    limits.extent(extent)?;
    let input_space = decoder
        .input_colorspace()
        .ok_or("JPEG has no color interpretation")?;
    let (channels, output_space) = match input_space {
        ColorSpace::Luma => (SourceChannels::Gray, ColorSpace::Luma),
        ColorSpace::RGB | ColorSpace::YCbCr => (SourceChannels::Rgb, ColorSpace::RGB),
        ColorSpace::CMYK | ColorSpace::YCCK => {
            return Err("CMYK JPEG decoding is not yet qualified; use a profiled CMYK TIFF".into());
        }
        _ => return Err("Unsupported JPEG color encoding".into()),
    };
    // Gain maps are not discarded and mislabeled as an ordinary SDR source.
    if decoder
        .info()
        .is_some_and(|info| !info.gain_map_info.is_empty())
    {
        return Err(
            "This JPEG contains an HDR gain map; its SDR rendition needs an explicit import choice"
                .into(),
        );
    }
    let interpretation = interpretation(channels, IntegerDepth::U8, profile)?;
    decoder.set_options(decoder.options().jpeg_set_out_colorspace(output_space));
    let size = decoder
        .output_buffer_size()
        .filter(|n| *n <= limits.codec_bytes)
        .ok_or("JPEG output exceeds the codec memory budget")?;
    let mut pixels = vec![0; size];
    decoder.decode_into(&mut pixels).map_err(err)?;
    let row_bytes = w * interpretation.pixel_bytes();
    let mut builder = SourceBuilder::new(extent, interpretation, limits.source_bytes)?;
    for row in pixels.chunks_exact(row_bytes) {
        builder.push_row(row)?;
    }
    super::orientation::normalize(builder.finish()?, orientation, limits.source_bytes)
}

// zune-jpeg returns None for both absent and malformed ICC chunk sequences.
// Preserve that distinction, and accept the ICC specification's full 255-chunk
// limit. Scan markers (including between progressive scans), never payload text.
fn embedded_profile(bytes: &[u8]) -> Result<Option<Vec<u8>>, String> {
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return Err("Invalid JPEG signature".into());
    }
    let mut chunks = std::collections::BTreeMap::new();
    let mut total = None;
    let mut at = 2;
    let mut entropy = false;
    while at < bytes.len() {
        if entropy {
            at += bytes[at..]
                .iter()
                .position(|b| *b == 0xff)
                .ok_or("JPEG has no end marker")?;
        }
        if bytes[at] != 0xff {
            return Err("Invalid JPEG marker boundary".into());
        }
        while bytes.get(at) == Some(&0xff) {
            at += 1;
        }
        let marker = *bytes.get(at).ok_or("Incomplete JPEG marker")?;
        at += 1;
        if entropy && (marker == 0 || (0xd0..=0xd7).contains(&marker)) {
            continue;
        }
        if marker == 0xd9 {
            break;
        }
        if marker == 1 {
            continue;
        }
        let size_bytes: [u8; 2] = bytes
            .get(at..at + 2)
            .ok_or("Incomplete JPEG segment size")?
            .try_into()
            .unwrap();
        let size = usize::from(u16::from_be_bytes(size_bytes));
        if size < 2 {
            return Err("Invalid JPEG segment size".into());
        }
        let segment = bytes
            .get(at + 2..at + size)
            .ok_or("Incomplete JPEG segment")?;
        at += size;
        entropy = marker == 0xda;
        if marker == 0xe2 && segment.starts_with(b"ICC_PROFILE\0") {
            if segment.len() < 14 {
                return Err("Incomplete JPEG ICC chunk".into());
            }
            let (sequence, count) = (segment[12], segment[13]);
            if sequence == 0
                || sequence > count
                || total.is_some_and(|v| v != count)
                || chunks.insert(sequence, &segment[14..]).is_some()
            {
                return Err("Conflicting JPEG ICC chunk sequence".into());
            }
            total = Some(count);
        }
    }
    let Some(total) = total else {
        return Ok(None);
    };
    if chunks.len() != usize::from(total) {
        return Err("JPEG ICC profile is incomplete".into());
    }
    let size: usize = chunks.values().map(|b| b.len()).sum();
    if size > crate::MAX_ICC_BYTES {
        return Err("JPEG ICC profile exceeds the memory budget".into());
    }
    let mut profile = Vec::with_capacity(size);
    for chunk in chunks.values() {
        profile.extend_from_slice(chunk);
    }
    Ok(Some(profile))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_icc_is_not_confused_with_an_untagged_jpeg() {
        let segment = |sequence, total, payload: &[u8]| {
            let mut bytes = vec![0xff, 0xe2];
            bytes.extend_from_slice(&((payload.len() + 16) as u16).to_be_bytes());
            bytes.extend_from_slice(b"ICC_PROFILE\0");
            bytes.extend_from_slice(&[sequence, total]);
            bytes.extend_from_slice(payload);
            bytes
        };
        let wrap =
            |chunks: Vec<Vec<u8>>| [vec![0xff, 0xd8], chunks.concat(), vec![0xff, 0xd9]].concat();
        assert_eq!(embedded_profile(&wrap(vec![])).unwrap(), None);
        assert_eq!(
            embedded_profile(&wrap(vec![segment(2, 2, b"def"), segment(1, 2, b"abc")])).unwrap(),
            Some(b"abcdef".to_vec())
        );
        assert!(embedded_profile(&wrap(vec![segment(1, 2, b"abc")])).is_err());
        assert!(
            embedded_profile(&wrap(vec![segment(1, 2, b"abc"), segment(1, 2, b"def")])).is_err()
        );
        let full: Vec<_> = (1..=255).map(|i| segment(i, 255, &[i])).collect();
        assert_eq!(
            embedded_profile(&wrap(full)).unwrap().unwrap(),
            (1..=255).collect::<Vec<u8>>()
        );
    }
}
