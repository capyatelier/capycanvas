//! MPF camera previews do not make the primary photograph a multi-frame image.
//! Accept a baseline primary followed only by explicitly typed large thumbnails.

pub(super) fn validate_previews(segment: &[u8]) -> Result<(), String> {
    let invalid = "Invalid JPEG MPF directory";
    let unsupported = "Multiple-picture JPEG is not supported. Export the intended image as a separate SDR PNG, JPEG or TIFF.";
    let bytes = segment.strip_prefix(b"MPF\0").ok_or(invalid)?;
    let little = match bytes.get(..4) {
        Some(b"II\x2a\0") => true,
        Some(b"MM\0\x2a") => false,
        _ => return Err(invalid.into()),
    };
    let u16_at = |at: usize| -> Result<u16, String> {
        let b = bytes
            .get(at..at.checked_add(2).ok_or(invalid)?)
            .ok_or(invalid)?;
        Ok(if little {
            u16::from_le_bytes(b.try_into().unwrap())
        } else {
            u16::from_be_bytes(b.try_into().unwrap())
        })
    };
    let u32_at = |at: usize| -> Result<u32, String> {
        let b = bytes
            .get(at..at.checked_add(4).ok_or(invalid)?)
            .ok_or(invalid)?;
        Ok(if little {
            u32::from_le_bytes(b.try_into().unwrap())
        } else {
            u32::from_be_bytes(b.try_into().unwrap())
        })
    };
    let ifd = u32_at(4)? as usize;
    let count = usize::from(u16_at(ifd)?);
    let end = ifd.checked_add(2 + 12 * count + 4).ok_or(invalid)?;
    if ifd < 8 || end > bytes.len() {
        return Err(invalid.into());
    }
    let mut images = None;
    let mut entries = None;
    for index in 0..count {
        let at = ifd + 2 + 12 * index;
        match u16_at(at)? {
            0xb001 => {
                if images.is_some() || u16_at(at + 2)? != 4 || u32_at(at + 4)? != 1 {
                    return Err(invalid.into());
                }
                images = Some(u32_at(at + 8)? as usize);
            }
            0xb002 => {
                if entries.is_some() || u16_at(at + 2)? != 7 {
                    return Err(invalid.into());
                }
                entries = Some((u32_at(at + 8)? as usize, u32_at(at + 4)? as usize));
            }
            _ => (),
        }
    }
    let images = images.ok_or(invalid)?;
    let (at, length) = entries.ok_or(invalid)?;
    if images == 0
        || images.checked_mul(16) != Some(length)
        || at < end
        || at.checked_add(length).is_none_or(|end| end > bytes.len())
    {
        return Err(invalid.into());
    }
    for index in 0..images {
        let at = at + index * 16;
        let attributes = u32_at(at)?;
        let size = u32_at(at + 4)?;
        let offset = u32_at(at + 8)?;
        let kind = attributes & 0x00ff_ffff;
        if size < 4 || attributes & 0x0700_0000 != 0 {
            return Err(invalid.into());
        }
        if if index == 0 {
            kind != 0x030000 || offset != 0
        } else {
            !matches!(kind, 0x010001 | 0x010002) || offset == 0
        } {
            return Err(unsupported.into());
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
