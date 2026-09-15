//! Only supported photo metadata is retained. Stale EXIF dimensions, thumbnails,
//! camera settings and orientation are never blindly copied into delivery files.
use layer_core::{ImageResolution, ResolutionUnit};
#[derive(Clone, Copy, Debug)]
pub(super) struct Exif {
    pub orientation: u16,
    pub resolution: Option<ImageResolution>,
}
impl Default for Exif {
    fn default() -> Self {
        Self {
            orientation: 1,
            resolution: None,
        }
    }
}
/// Unknown units and invalid/zero print densities do not invent a physical size.
/// They do not invalidate otherwise supported photographic pixels.
pub(super) fn physical(unit: u16, density: [Option<[u32; 2]>; 2]) -> Option<ImageResolution> {
    let unit = match unit {
        2 => ResolutionUnit::Inch,
        3 => ResolutionUnit::Centimetre,
        _ => return None,
    };
    let result = ImageResolution {
        unit,
        density: [density[0]?, density[1]?],
    };
    result.validate().ok().map(|_| result)
}
pub(super) fn exif(bytes: &[u8]) -> Result<Exif, String> {
    let bytes = bytes.strip_prefix(b"Exif\0\0").unwrap_or(bytes);
    if bytes.len() < 8 {
        return Err("Incomplete EXIF header".into());
    }
    let little = match &bytes[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Err("Invalid EXIF byte order".into()),
    };
    let u16_at = |at: usize| -> Result<u16, String> {
        let b: [u8; 2] = bytes
            .get(at..at.checked_add(2).ok_or("EXIF offset overflow")?)
            .ok_or("Incomplete EXIF value")?
            .try_into()
            .unwrap();
        Ok(if little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    };
    let u32_at = |at: usize| -> Result<u32, String> {
        let b: [u8; 4] = bytes
            .get(at..at.checked_add(4).ok_or("EXIF offset overflow")?)
            .ok_or("Incomplete EXIF value")?
            .try_into()
            .unwrap();
        Ok(if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    if u16_at(2)? != 42 {
        return Err("Unsupported EXIF TIFF header".into());
    }
    let ifd = u32_at(4)? as usize;
    if ifd == 0 {
        return Ok(Exif::default());
    }
    let count = usize::from(u16_at(ifd)?);
    let mut result = Exif::default();
    let mut density = [None; 2];
    let mut unit = 2;
    for index in 0..count {
        let at = ifd
            .checked_add(2 + index * 12)
            .ok_or("EXIF directory overflow")?;
        match u16_at(at)? {
            274 => {
                if u16_at(at + 2)? != 3 || u32_at(at + 4)? != 1 {
                    return Err("Invalid EXIF orientation field".into());
                }
                let value = u16_at(at + 8)?;
                if !(1..=8).contains(&value) {
                    return Err("Invalid EXIF orientation".into());
                }
                result.orientation = value;
            }
            tag @ (282 | 283) => {
                if u16_at(at + 2)? == 5 && u32_at(at + 4)? == 1 {
                    let offset = u32_at(at + 8)? as usize;
                    density[(tag - 282) as usize] = Some([
                        u32_at(offset)?,
                        u32_at(offset.checked_add(4).ok_or("EXIF offset overflow")?)?,
                    ]);
                }
            }
            296 if u16_at(at + 2)? == 3 && u32_at(at + 4)? == 1 => unit = u16_at(at + 8)?,
            _ => (),
        }
    }
    result.resolution = physical(unit, density);
    Ok(result)
}

/// Minimal TIFF IFD in an Exif APP1 payload. Resolution remains rational and
/// orientation is normalized. No stale input tags or thumbnail are propagated.
pub(super) fn exif_output(resolution: ImageResolution) -> Result<Vec<u8>, String> {
    let (unit, density) = resolution.tiff_density()?;
    let mut bytes = b"Exif\0\0II*\0\x08\0\0\0".to_vec();
    bytes.extend_from_slice(&4u16.to_le_bytes());
    for (tag, ty, value) in [
        (274u16, 3u16, 1u32),
        (282, 5, 62),
        (283, 5, 70),
        (296, 3, u32::from(unit)),
    ] {
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(&ty.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for code in density.into_iter().flatten() {
        bytes.extend_from_slice(&code.to_le_bytes());
    }
    Ok(bytes)
}
