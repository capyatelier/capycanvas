//! Bounded metadata preflight across every scan, before interpretation is chosen.
//! APP payloads are never confused with marker bytes inside entropy-coded data.
use std::collections::BTreeMap;

const INVALID: &str = "Invalid JPEG marker stream";

#[derive(Default)]
pub(super) struct Metadata {
    pub gain_map: bool,
    pub adobe_transform: Option<u8>,
    pub profile: Option<Vec<u8>>,
    pub orientation: Option<u16>,
    pub resolution: Option<layer_core::ImageResolution>,
}

pub(super) struct Segment<'a> {
    pub marker: u8,
    pub offset: usize,
    pub bytes: &'a [u8],
}

/// The end of the first image and its APPn payloads in file order.
pub(super) fn scan(bytes: &[u8]) -> Result<(usize, Vec<Segment<'_>>), String> {
    if !bytes.starts_with(b"\xff\xd8") {
        return Err("Invalid JPEG signature".into());
    }
    let mut at = 2usize;
    let mut entropy = false;
    let mut segments = Vec::new();
    let mut scans = 0;
    loop {
        if entropy {
            at += bytes
                .get(at..)
                .and_then(|b| b.iter().position(|v| *v == 0xff))
                .ok_or(INVALID)?;
        }
        if bytes.get(at) != Some(&0xff) {
            return Err(INVALID.into());
        }
        while bytes.get(at) == Some(&0xff) {
            at += 1;
        }
        let marker = *bytes.get(at).ok_or(INVALID)?;
        at += 1;
        if entropy && (marker == 0 || (0xd0..=0xd7).contains(&marker)) {
            continue;
        }
        if marker == 0xd9 {
            return Ok((at, segments));
        }
        if marker == 1 {
            continue;
        }
        if marker == 0 || (0xd0..=0xd8).contains(&marker) {
            return Err(INVALID.into());
        }
        let size =
            u16::from_be_bytes(bytes.get(at..at + 2).ok_or(INVALID)?.try_into().unwrap()) as usize;
        if size < 2 {
            return Err(INVALID.into());
        }
        let data = bytes.get(at + 2..at + size).ok_or(INVALID)?;
        if (0xe0..=0xef).contains(&marker) {
            if segments.len() >= 4096 {
                return Err("Too many JPEG metadata segments".into());
            }
            segments.push(Segment {
                marker,
                offset: at + 2,
                bytes: data,
            });
        }
        at += size;
        entropy = marker == 0xda;
        if entropy {
            scans += 1;
            if scans > 256 {
                return Err("JPEG exceeds the supported scan limit".into());
            }
        }
    }
}

pub(super) fn read_source(bytes: &[u8]) -> Result<Metadata, String> {
    let (_, segments) = scan(bytes)?;
    let mut chunks = BTreeMap::new();
    let mut total = None;
    let mut retained = 0;
    let mut result = Metadata::default();
    let mut jfif_resolution = None;
    let mut mpf_error = None;
    for Segment {
        marker,
        bytes: segment,
        ..
    } in segments
    {
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
            chunks.insert(sequence, &segment[14..]);
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
            let metadata = super::metadata::exif(segment)?;
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
            result.gain_map = true;
        } else if marker == 0xe2 && segment.starts_with(b"MPF\0") {
            mpf_error = super::jpeg_mpf::validate_previews(segment).err();
        }
    }
    if !result.gain_map
        && let Some(error) = mpf_error
    {
        return Err(error);
    }
    if let Some(count) = total {
        if chunks.len() != usize::from(count) {
            return Err("JPEG ICC profile is incomplete".into());
        }
        result.profile = Some(chunks.into_values().collect::<Vec<_>>().concat());
    }
    // Rational Exif print density takes precedence over rounded JFIF values.
    result.resolution = result.resolution.or(jfif_resolution);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(read_source(&wrap(vec![])).unwrap().profile.is_none());
        let file = wrap(vec![
            icc(2, 2, b"def"),
            segment(0xda, &[]),
            vec![7, 0xff, 0, 9, 0xff, 0xd0, 4],
            icc(1, 2, b"abc"),
        ]);
        assert_eq!(read_source(&file).unwrap().profile.unwrap(), b"abcdef");
        assert!(read_source(&wrap(vec![icc(1, 2, b"a")])).is_err());
        assert!(read_source(&wrap(vec![icc(1, 2, b"a"), icc(1, 2, b"b")])).is_err());
        let file = wrap((1..=255).map(|i| icc(i, 255, &[i])).collect());
        assert_eq!(
            read_source(&file).unwrap().profile.unwrap(),
            (1..=255).collect::<Vec<u8>>()
        );
    }
}
