//! MPF camera previews do not make the primary photograph a multi-frame image.
//! Accept a baseline primary followed only by explicitly typed large thumbnails.
use super::metadata::Tiff;

const INVALID: &str = "Invalid JPEG MPF directory";

/// Attributes, size and offset of each of 1..=64 baseline MPF images. Offsets
/// are relative to the TIFF header that follows the `MPF\0` signature.
pub(super) fn mpf_entries(bytes: &[u8]) -> Result<Vec<[u32; 3]>, String> {
    let tiff = Tiff::new(bytes, INVALID)?;
    let ifd = tiff.u32(4)? as usize;
    let count = usize::from(tiff.u16(ifd)?);
    let end = ifd.checked_add(2 + 12 * count + 4).ok_or(INVALID)?;
    if ifd < 8 || end > tiff.len() {
        return Err(INVALID.into());
    }
    let mut images = None;
    let mut entries = None;
    for index in 0..count {
        let at = ifd + 2 + 12 * index;
        match tiff.u16(at)? {
            0xb001 => {
                if tiff.u16(at + 2)? != 4
                    || tiff.u32(at + 4)? != 1
                    || images.replace(tiff.u32(at + 8)? as usize).is_some()
                {
                    return Err(INVALID.into());
                }
            }
            0xb002 if tiff.u16(at + 2)? != 7
                    || entries
                        .replace((tiff.u32(at + 8)? as usize, tiff.u32(at + 4)? as usize))
                        .is_some()
            => {
                return Err(INVALID.into());
            }
            _ => (),
        }
    }
    let images = images.ok_or(INVALID)?;
    let (at, length) = entries.ok_or(INVALID)?;
    if !(1..=64).contains(&images)
        || images * 16 != length
        || at < end
        || at.checked_add(length).is_none_or(|end| end > tiff.len())
    {
        return Err(INVALID.into());
    }
    (0..images)
        .map(|index| {
            let at = at + index * 16;
            let entry = [tiff.u32(at)?, tiff.u32(at + 4)?, tiff.u32(at + 8)?];
            if entry[0] & 0x0700_0000 != 0 {
                return Err(INVALID.into());
            }
            Ok(entry)
        })
        .collect()
}

pub(super) fn validate_previews(segment: &[u8]) -> Result<(), String> {
    let entries = mpf_entries(segment.strip_prefix(b"MPF\0").ok_or(INVALID)?)?;
    for (index, [attributes, size, offset]) in entries.into_iter().enumerate() {
        let kind = attributes & 0x00ff_ffff;
        if size < 4 {
            return Err(INVALID.into());
        }
        if if index == 0 {
            kind != 0x030000 || offset != 0
        } else {
            !matches!(kind, 0x010001 | 0x010002) || offset == 0
        } {
            return Err("Multiple-picture JPEG is not supported. Export the intended image as a separate SDR PNG, JPEG or TIFF.".into());
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub(in crate::photo) fn directory(little: bool, secondary: u32) -> Vec<u8> {
        let u16 = |v: u16| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        let u32 = |v: u32| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        let mut b = b"MPF\0".to_vec();
        b.extend_from_slice(if little { b"II" } else { b"MM" });
        b.extend(u16(42));
        b.extend(u32(8));
        b.extend(u16(2));
        for (tag, ty, count, value) in [(0xb001, 4, 1, 2), (0xb002, 7, 32, 38)] {
            b.extend(u16(tag));
            b.extend(u16(ty));
            b.extend(u32(count));
            b.extend(u32(value));
        }
        b.extend(u32(0));
        for (attributes, offset) in [(0xa0030000, 0), (0x40000000 | secondary, 1024)] {
            b.extend(u32(attributes));
            b.extend(u32(1024));
            b.extend(u32(offset));
            b.extend(u32(0));
        }
        b
    }
    #[test]
    fn camera_thumbnails_are_allowed_but_multi_frame_and_unknown_images_are_not() {
        for little in [false, true] {
            for kind in [0x010001, 0x010002] {
                let b = directory(little, kind);
                validate_previews(&b).unwrap();
                for len in 0..b.len() {
                    assert!(validate_previews(&b[..len]).is_err());
                }
            }
            for kind in [0, 0x020001, 0x020002, 0x020003, 0x030000] {
                assert!(validate_previews(&directory(little, kind)).is_err());
            }
        }
    }
}
