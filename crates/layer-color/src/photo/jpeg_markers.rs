//! Bounded metadata preflight across every scan, before interpretation is chosen.
//! APP payloads are never confused with marker bytes inside entropy-coded data.
use super::*;
use std::collections::BTreeMap;
use std::io::BufRead;

#[derive(Default)]
pub(super) struct Metadata {
    pub gain_map: bool,
    pub adobe_transform: Option<u8>,
    pub profile: Option<Vec<u8>>,
    pub orientation: Option<u16>,
    pub resolution: Option<layer_core::ImageResolution>,
}

fn byte(input: &mut impl BufRead) -> Result<u8, String> {
    let mut byte = [0];
    input
        .read_exact(&mut byte)
        .map_err(|_| "Incomplete JPEG marker stream")?;
    Ok(byte[0])
}
#[cfg(test)]
pub(super) fn read(input: impl BufRead) -> Result<Metadata, String> { read_impl(input, false) }
pub(super) fn read_source(input: impl BufRead) -> Result<Metadata, String> { read_impl(input, true) }
fn read_impl(mut input: impl BufRead, allow_hdr: bool) -> Result<Metadata, String> {
    if [byte(&mut input)?, byte(&mut input)?] != [0xff, 0xd8] {
        return Err("Invalid JPEG signature".into());
    }
    let mut chunks = BTreeMap::new();
    let mut total = None;
    let mut retained = 0;
    let mut scans = 0;
    let mut entropy = false;
    let mut result = Metadata::default();
    let mut jfif_resolution = None;
    let mut mpf_error = None;
    loop {
        if entropy {
            loop {
                let bytes = input.fill_buf().map_err(err)?;
                if bytes.is_empty() {
                    return Err("JPEG has no end marker".into());
                }
                if let Some(index) = bytes.iter().position(|v| *v == 0xff) {
                    input.consume(index);
                    break;
                }
                let len = bytes.len();
                input.consume(len);
            }
        }
        if byte(&mut input)? != 0xff {
            return Err("Invalid JPEG marker boundary".into());
        }
        let mut marker = byte(&mut input)?;
        while marker == 0xff {
            marker = byte(&mut input)?;
        }
        if entropy && (marker == 0 || (0xd0..=0xd7).contains(&marker)) {
            continue;
        }
        if marker == 0xd9 {
            break;
        }
        if marker == 1 {
            continue;
        }
        if marker == 0xd8 || marker == 0 || (0xd0..=0xd7).contains(&marker) {
            return Err("Unexpected JPEG marker".into());
        }
        let size = usize::from(u16::from_be_bytes([byte(&mut input)?, byte(&mut input)?]));
        if size < 2 {
            return Err("Invalid JPEG segment size".into());
        }
        let mut segment = vec![0; size - 2];
        input
            .read_exact(&mut segment)
            .map_err(|_| "Incomplete JPEG segment")?;
        entropy = marker == 0xda;
        if entropy {
            scans += 1;
            if scans > 256 {
                return Err("JPEG exceeds the supported scan limit".into());
            }
        }
        if marker == 0xee && segment.starts_with(b"Adobe") {
            if segment.len() < 12 {
                return Err("Incomplete JPEG Adobe marker".into());
            }
            if result.adobe_transform.is_some_and(|v| v != segment[11]) {
                return Err("Conflicting JPEG Adobe transforms".into());
            }
            result.adobe_transform = Some(segment[11]);
        }
        if marker == 0xe2 && segment.starts_with(b"ICC_PROFILE\0") {
            if segment.len() < 14 {
                return Err("Incomplete JPEG ICC chunk".into());
            }
            let (sequence, count) = (segment[12], segment[13]);
            if sequence == 0
                || sequence > count
                || total.is_some_and(|v| v != count)
                || chunks.contains_key(&sequence)
            {
                return Err("Conflicting JPEG ICC chunk sequence".into());
            }
            retained += segment.len() - 14;
            if retained > crate::MAX_ICC_BYTES {
                return Err("JPEG ICC profile exceeds the memory budget".into());
            }
            total = Some(count);
            chunks.insert(sequence, segment[14..].to_vec());
        } else if marker == 0xe0 && segment.starts_with(b"JFIF\0") {
            if segment.len() >= 12 {
                jfif_resolution = super::metadata::physical(
                    u16::from(segment[7]) + 1,
                    [
                        Some([u32::from(u16::from_be_bytes([segment[8], segment[9]])), 1]),
                        Some([u32::from(u16::from_be_bytes([segment[10], segment[11]])), 1]),
                    ],
                );
            }
        } else if marker == 0xe1 && segment.starts_with(b"Exif\0\0") {
            let metadata = super::metadata::exif(&segment)?;
            let orientation = metadata.orientation;
            if result.orientation.is_some_and(|v| v != orientation) {
                return Err("Conflicting JPEG EXIF orientations".into());
            }
            result.orientation = Some(orientation);
            if let Some(resolution) = metadata.resolution {
                if result.resolution.is_some_and(|r| r != resolution) {
                    return Err("Conflicting JPEG EXIF resolutions".into());
                }
                result.resolution = Some(resolution);
            }
        } else if marker == 0xe2 && segment.starts_with(b"urn:iso:std:iso:ts:21496:-1")
            || marker == 0xe1
                && [
                    b"http://ns.adobe.com/hdr-gain-map/1.0/".as_slice(),
                    b"http://ns.google.com/photos/1.0/gainmap/",
                ]
                .iter()
                .any(|signature| segment.windows(signature.len()).any(|w| w == *signature))
        {
            if !allow_hdr { return Err("This JPEG contains an HDR gain map; use the HDR reader".into()); }
            result.gain_map = true;
        } else if marker == 0xe2 && segment.starts_with(b"MPF\0") {
            mpf_error = super::jpeg_mpf::validate_previews(&segment).err();
        }
    }
    if !result.gain_map { if let Some(error) = mpf_error { return Err(error); } }
    if let Some(count) = total {
        if chunks.len() != usize::from(count) {
            return Err("JPEG ICC profile is incomplete".into());
        }
        let mut profile = Vec::with_capacity(retained);
        for bytes in chunks.into_values() {
            profile.extend_from_slice(&bytes);
        }
        result.profile = Some(profile);
    }
    // Rational Exif print density takes precedence over rounded JFIF values.
    result.resolution = result.resolution.or(jfif_resolution);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};
    fn segment(marker: u8, bytes: &[u8]) -> Vec<u8> {
        [
            vec![0xff, marker],
            ((bytes.len() + 2) as u16).to_be_bytes().to_vec(),
            bytes.to_vec(),
        ]
        .concat()
    }
    fn icc(sequence: u8, total: u8, bytes: &[u8]) -> Vec<u8> {
        segment(
            0xe2,
            &[b"ICC_PROFILE\0".as_slice(), &[sequence, total], bytes].concat(),
        )
    }
    fn wrap(chunks: Vec<Vec<u8>>) -> Vec<u8> {
        [vec![0xff, 0xd8], chunks.concat(), vec![0xff, 0xd9]].concat()
    }
    #[test]
    fn strict_profile_sequences_include_late_scans_and_all_255_chunks() {
        assert!(read(Cursor::new(wrap(vec![]))).unwrap().profile.is_none());
        let file = wrap(vec![
            icc(2, 2, b"def"),
            segment(0xda, &[]),
            vec![7, 0xff, 0, 9, 0xff, 0xd0, 4],
            icc(1, 2, b"abc"),
        ]);
        for size in 1..=16 {
            assert_eq!(
                read(BufReader::with_capacity(size, Cursor::new(&file)))
                    .unwrap()
                    .profile
                    .unwrap(),
                b"abcdef"
            );
        }
        assert!(read(Cursor::new(wrap(vec![icc(1, 2, b"a")]))).is_err());
        assert!(read(Cursor::new(wrap(vec![icc(1, 2, b"a"), icc(1, 2, b"b")]))).is_err());
        let file = wrap((1..=255).map(|i| icc(i, 255, &[i])).collect());
        assert_eq!(
            read(Cursor::new(file)).unwrap().profile.unwrap(),
            (1..=255).collect::<Vec<u8>>()
        );
    }
    #[test]
    fn missing_end_hdr_and_multiple_images_are_not_plain_sdr() {
        assert!(read(Cursor::new([0xff, 0xd8])).is_err());
        for (marker, bytes) in [
            (0xe2, b"urn:iso:std:iso:ts:21496:-1\0".as_slice()),
            (0xe2, b"MPF\0"),
            (0xe1, b"http://ns.adobe.com/hdr-gain-map/1.0/"),
            (0xe1, b"http://ns.google.com/photos/1.0/gainmap/"),
        ] {
            assert!(read(Cursor::new(wrap(vec![segment(marker, bytes)]))).is_err());
        }
    }
}
