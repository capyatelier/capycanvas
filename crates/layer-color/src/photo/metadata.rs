//! Only supported photo metadata is retained. Stale EXIF dimensions, thumbnails
//! and orientation are never copied into delivery files; export builds fresh
//! blocks from the descriptive metadata the recipe keeps.
use super::exif::{self, Directories, Entry};
use layer_core::{ImageResolution, PhotoMetadata, ResolutionUnit};
use serde::{Deserialize, Serialize};
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
/// Bounds-checked TIFF header and values in either byte order.
#[derive(Clone, Copy)]
pub(super) struct Tiff<'a> {
    bytes: &'a [u8],
    little: bool,
    error: &'static str,
}
impl<'a> Tiff<'a> {
    pub fn new(bytes: &'a [u8], error: &'static str) -> Result<Self, String> {
        let little = match bytes.get(..4) {
            Some(b"II\x2a\0") => true,
            Some(b"MM\0\x2a") => false,
            _ => return Err(error.into()),
        };
        Ok(Self {
            bytes,
            little,
            error,
        })
    }
    pub fn len(&self) -> usize {
        self.bytes.len()
    }
    fn get<const N: usize>(&self, at: usize) -> Result<[u8; N], String> {
        at.checked_add(N)
            .and_then(|end| self.bytes.get(at..end))
            .map(|b| b.try_into().unwrap())
            .ok_or_else(|| self.error.into())
    }
    pub fn u16(&self, at: usize) -> Result<u16, String> {
        let b = self.get(at)?;
        Ok(if self.little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    }
    pub fn u32(&self, at: usize) -> Result<u32, String> {
        let b = self.get(at)?;
        Ok(if self.little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    }
}
pub(super) fn exif(bytes: &[u8]) -> Result<Exif, String> {
    let tiff = Tiff::new(
        bytes.strip_prefix(b"Exif\0\0").unwrap_or(bytes),
        "Invalid EXIF metadata",
    )?;
    let ifd = tiff.u32(4)? as usize;
    if ifd == 0 {
        return Ok(Exif::default());
    }
    let count = usize::from(tiff.u16(ifd)?);
    let mut result = Exif::default();
    let mut density = [None; 2];
    let mut unit = 2;
    for index in 0..count {
        let at = ifd
            .checked_add(2 + index * 12)
            .ok_or("EXIF directory overflow")?;
        match tiff.u16(at)? {
            274 => {
                if tiff.u16(at + 2)? != 3 || tiff.u32(at + 4)? != 1 {
                    return Err("Invalid EXIF orientation field".into());
                }
                let value = tiff.u16(at + 8)?;
                if !(1..=8).contains(&value) {
                    return Err("Invalid EXIF orientation".into());
                }
                result.orientation = value;
            }
            tag @ (282 | 283) => {
                if tiff.u16(at + 2)? == 5 && tiff.u32(at + 4)? == 1 {
                    let offset = tiff.u32(at + 8)? as usize;
                    density[(tag - 282) as usize] = Some([
                        tiff.u32(offset)?,
                        tiff.u32(offset.checked_add(4).ok_or("EXIF offset overflow")?)?,
                    ]);
                }
            }
            296 if tiff.u16(at + 2)? == 3 && tiff.u32(at + 4)? == 1 => unit = tiff.u16(at + 8)?,
            _ => (),
        }
    }
    result.resolution = physical(unit, density);
    Ok(result)
}

/// Which of the photo's descriptive metadata an export keeps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetadataKeep {
    #[default]
    All,
    CopyrightContact,
    None,
}
impl MetadataKeep {
    pub const ALL: [Self; 3] = [Self::All, Self::CopyrightContact, Self::None];
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::CopyrightContact => "Copyright & Contact",
            Self::None => "None",
        }
    }
}

/// Export metadata policy. Dimensions, orientation and print density are always
/// regenerated for the delivered image.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportMetadata {
    pub keep: MetadataKeep,
    /// Leaves out GPS coordinates and place names when everything else is kept.
    pub remove_location: bool,
}
impl Default for ExportMetadata {
    fn default() -> Self {
        Self { keep: MetadataKeep::All, remove_location: true }
    }
}

/// Everything a writer embeds besides pixels and color.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeliveryMetadata {
    pub resolution: Option<ImageResolution>,
    pub photo: PhotoMetadata,
    pub policy: ExportMetadata,
}
impl DeliveryMetadata {
    pub fn resolution(resolution: Option<ImageResolution>) -> Self {
        Self { resolution, ..Default::default() }
    }
    /// The photo's kept directories, with the delivered size in the Exif IFD.
    /// Orientation and density are left to the caller's container.
    pub(super) fn directories(&self, extent: [u32; 2]) -> Result<Directories, String> {
        let source = self
            .photo
            .exif
            .as_deref()
            .map(exif::read_block)
            .transpose()?
            .map(|found| found.directories)
            .unwrap_or_default();
        let keep = self.policy.keep;
        let mut result = Directories {
            image: source
                .image
                .into_iter()
                .filter(|e| match keep {
                    MetadataKeep::All => true,
                    MetadataKeep::CopyrightContact => matches!(e.tag, exif::ARTIST | exif::COPYRIGHT),
                    MetadataKeep::None => false,
                })
                .collect(),
            ..Default::default()
        };
        if keep == MetadataKeep::All {
            result.exif = source.exif;
            if !self.policy.remove_location {
                result.gps = source.gps;
            }
        }
        if !result.exif.is_empty() {
            if !result.exif.iter().any(|e| e.tag == exif::EXIF_VERSION) {
                result.exif.push(Entry { tag: exif::EXIF_VERSION, kind: exif::UNDEFINED, count: 4, value: b"0232".to_vec() });
            }
            result.exif.push(Entry::long(exif::PIXEL_X, extent[0]));
            result.exif.push(Entry::long(exif::PIXEL_Y, extent[1]));
        }
        Ok(result)
    }
    /// A fresh Exif TIFF block without the `Exif\0\0` identifier, or none when
    /// nothing is kept and no print density is set.
    pub(super) fn exif(&self, extent: [u32; 2]) -> Result<Option<Vec<u8>>, String> {
        let mut directories = self.directories(extent)?;
        if directories.is_empty() && self.resolution.is_none() {
            return Ok(None);
        }
        directories.image.push(Entry::short(exif::ORIENTATION, 1));
        if let Some(resolution) = self.resolution {
            let (unit, [x, y]) = resolution.tiff_density()?;
            directories.image.extend([
                Entry::rational(exif::X_RESOLUTION, x),
                Entry::rational(exif::Y_RESOLUTION, y),
                Entry::short(exif::RESOLUTION_UNIT, unit),
            ]);
        }
        directories.encode().map(Some)
    }
    /// The kept XMP properties as self-contained `rdf:Description` elements.
    pub(super) fn xmp_descriptions(&self) -> Result<Option<String>, String> {
        match &self.photo.xmp {
            Some(packet) => super::xmp::descriptions(packet, self.policy),
            None => Ok(None),
        }
    }
    /// A filtered XMP packet, or none when nothing is kept.
    pub(super) fn xmp(&self) -> Result<Option<Vec<u8>>, String> {
        Ok(self.xmp_descriptions()?.map(|d| super::xmp::packet(&d)))
    }
}

/// Descriptive metadata read with a photo. Unreadable blocks, and blocks past
/// the 64 MiB allowance, are left out and never stop the photo from opening.
pub(super) fn collect(exif: Option<exif::Found>, xmp: Option<Vec<u8>>, iptc: Option<Vec<u8>>) -> PhotoMetadata {
    let (directories, xmp, iptc) = match exif {
        Some(found) => (Some(found.directories), xmp.or(found.xmp), iptc.or(found.iptc)),
        None => (None, xmp, iptc),
    };
    let exif = directories
        .filter(|d| !d.is_empty())
        .and_then(|d| d.encode().ok());
    let everything = ExportMetadata { keep: MetadataKeep::All, remove_location: false };
    let xmp = xmp.filter(|packet| super::xmp::descriptions(packet, everything).is_ok());
    let mut result = PhotoMetadata::default();
    let mut total = 0usize;
    for (block, out) in [(exif, &mut result.exif), (xmp, &mut result.xmp), (iptc, &mut result.iptc)] {
        if let Some(block) = block.filter(|b| !b.is_empty())
            && total + block.len() <= PhotoMetadata::MAX_BYTES
        {
            total += block.len();
            *out = Some(block.into());
        }
    }
    result
}

/// The IPTC-IIM records in Photoshop image resources.
pub(super) fn photoshop_iptc(mut resources: &[u8]) -> Option<Vec<u8>> {
    while let Some(rest) = resources.strip_prefix(b"8BIM") {
        let id = u16::from_be_bytes(rest.get(..2)?.try_into().ok()?);
        let name = usize::from(*rest.get(2)?);
        let at = 2 + (1 + name).next_multiple_of(2);
        let size = u32::from_be_bytes(rest.get(at..at + 4)?.try_into().ok()?) as usize;
        let data = rest.get(at + 4..at + 4 + size)?;
        if id == 0x0404 {
            return Some(data.to_vec());
        }
        resources = rest.get(at + 4 + size.next_multiple_of(2)..)?;
    }
    None
}
