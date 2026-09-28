//! Exif directories as typed entries. Reading keeps IFD0's descriptive text
//! tags, the Exif IFD and the GPS IFD, and leaves out maker notes,
//! interoperability data and thumbnails, whose offsets cannot move. Writing
//! lays the entries out in a fresh little-endian TIFF block.
use layer_core::PhotoMetadata;
use std::io::{Cursor, Read, Seek, SeekFrom};

pub(super) const ORIENTATION: u16 = 0x0112;
pub(super) const X_RESOLUTION: u16 = 0x011a;
pub(super) const Y_RESOLUTION: u16 = 0x011b;
pub(super) const RESOLUTION_UNIT: u16 = 0x0128;
pub(super) const ARTIST: u16 = 0x013b;
pub(super) const COPYRIGHT: u16 = 0x8298;
pub(super) const PIXEL_X: u16 = 0xa002;
pub(super) const PIXEL_Y: u16 = 0xa003;
pub(super) const EXIF_VERSION: u16 = 0x9000;
const XMP: u16 = 700;
const IPTC: u16 = 0x83bb;
const EXIF_IFD: u16 = 0x8769;
const GPS_IFD: u16 = 0x8825;
const INTEROP_IFD: u16 = 0xa005;
const MAKER_NOTE: u16 = 0x927c;
/// Description, make, model, software, date and time, artist and copyright.
const TEXT_TAGS: [u16; 7] = [0x010e, 0x010f, 0x0110, 0x0131, 0x0132, ARTIST, COPYRIGHT];
const MAX_ENTRIES: usize = 1024;
const INVALID: &str = "Invalid Exif directory";

pub(super) const SHORT: u16 = 3;
pub(super) const LONG: u16 = 4;
pub(super) const RATIONAL: u16 = 5;
pub(super) const UNDEFINED: u16 = 7;

/// One directory entry. Multibyte values are little-endian.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Entry {
    pub tag: u16,
    pub kind: u16,
    pub count: u32,
    pub value: Vec<u8>,
}
impl Entry {
    pub fn short(tag: u16, value: u16) -> Self {
        Self { tag, kind: SHORT, count: 1, value: value.to_le_bytes().to_vec() }
    }
    pub fn long(tag: u16, value: u32) -> Self {
        Self { tag, kind: LONG, count: 1, value: value.to_le_bytes().to_vec() }
    }
    pub fn rational(tag: u16, [n, d]: [u32; 2]) -> Self {
        Self { tag, kind: RATIONAL, count: 1, value: [n.to_le_bytes(), d.to_le_bytes()].concat() }
    }
}

/// Byte width of one component, and components per value.
fn component(kind: u16) -> Option<(usize, u32)> {
    match kind {
        1 | 2 | 6 | 7 => Some((1, 1)),
        3 | 8 => Some((2, 1)),
        4 | 9 | 11 => Some((4, 1)),
        5 | 10 => Some((4, 2)),
        12 => Some((8, 1)),
        _ => None,
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Directories {
    pub image: Vec<Entry>,
    pub exif: Vec<Entry>,
    pub gps: Vec<Entry>,
}

/// The descriptive directories, plus the XMP and IPTC tags a TIFF file carries.
#[derive(Default)]
pub(super) struct Found {
    pub directories: Directories,
    pub xmp: Option<Vec<u8>>,
    pub iptc: Option<Vec<u8>>,
}

struct Walker<'a, R> {
    input: &'a mut R,
    base: u64,
    length: u64,
    little: bool,
    budget: usize,
}
impl<R: Read + Seek> Walker<'_, R> {
    fn bytes(&mut self, offset: u64, len: usize) -> Result<Vec<u8>, String> {
        offset
            .checked_add(len as u64)
            .filter(|end| *end <= self.length)
            .ok_or(INVALID)?;
        self.budget = self
            .budget
            .checked_sub(len)
            .ok_or("Photo metadata exceeds 64 MiB")?;
        self.input
            .seek(SeekFrom::Start(self.base + offset))
            .map_err(|e| e.to_string())?;
        let mut bytes = vec![0; len];
        self.input.read_exact(&mut bytes).map_err(|e| e.to_string())?;
        Ok(bytes)
    }
    fn u16(&self, b: &[u8]) -> u16 {
        let b = [b[0], b[1]];
        if self.little { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) }
    }
    fn u32(&self, b: &[u8]) -> u32 {
        let b = [b[0], b[1], b[2], b[3]];
        if self.little { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) }
    }
    /// Entries in file byte order, and the Exif and GPS directory offsets.
    fn directory(
        &mut self,
        offset: u64,
        keep: impl Fn(u16) -> bool,
    ) -> Result<(Vec<Entry>, [Option<u32>; 2]), String> {
        let count = self.bytes(offset, 2)?;
        let count = usize::from(self.u16(&count));
        if count > MAX_ENTRIES {
            return Err(INVALID.into());
        }
        let table = self.bytes(offset + 2, count * 12)?;
        let mut entries = Vec::new();
        let mut pointers = [None; 2];
        for raw in table.chunks_exact(12) {
            let (tag, kind, count) = (self.u16(&raw[..2]), self.u16(&raw[2..4]), self.u32(&raw[4..8]));
            if matches!(tag, EXIF_IFD | GPS_IFD) {
                if matches!(kind, LONG | 13) && count == 1 {
                    pointers[usize::from(tag == GPS_IFD)] = Some(self.u32(&raw[8..]));
                }
                continue;
            }
            let Some((width, per)) = component(kind) else { continue };
            let Some(len) = (count as usize).checked_mul(width * per as usize) else { continue };
            if !keep(tag) || len == 0 {
                continue;
            }
            let value = if len <= 4 {
                raw[8..8 + len].to_vec()
            } else {
                match self.bytes(u64::from(self.u32(&raw[8..])), len) {
                    Ok(value) => value,
                    Err(e) if e == INVALID => continue,
                    Err(e) => return Err(e),
                }
            };
            entries.push(Entry { tag, kind, count, value });
        }
        Ok((entries, pointers))
    }
    fn normalize(&self, entries: &mut [Entry]) {
        if self.little {
            return;
        }
        for entry in entries {
            let (width, _) = component(entry.kind).unwrap();
            for value in entry.value.chunks_exact_mut(width) {
                value.reverse();
            }
        }
    }
}

/// Walk the TIFF structure that starts at `base`. BigTIFF carries none.
pub(super) fn read(input: &mut (impl Read + Seek), base: u64, length: u64) -> Result<Found, String> {
    let mut walker = Walker { input, base, length, little: true, budget: PhotoMetadata::MAX_BYTES };
    let header = walker.bytes(0, 8)?;
    walker.little = match &header[..4] {
        b"II\x2a\0" => true,
        b"MM\0\x2a" => false,
        b"II\x2b\0" | b"MM\0\x2b" => return Ok(Found::default()),
        _ => return Err(INVALID.into()),
    };
    let first = u64::from(walker.u32(&header[4..]));
    let mut found = Found::default();
    if first == 0 {
        return Ok(found);
    }
    let (mut image, [exif, gps]) =
        walker.directory(first, |tag| TEXT_TAGS.contains(&tag) || matches!(tag, XMP | IPTC))?;
    for (tag, out) in [(XMP, &mut found.xmp), (IPTC, &mut found.iptc)] {
        if let Some(at) = image.iter().position(|e| e.tag == tag) {
            *out = Some(image.remove(at).value);
        }
    }
    let mut directories = Directories { image, ..Default::default() };
    if let Some(offset) = exif {
        directories.exif = walker
            .directory(u64::from(offset), |tag| {
                !matches!(tag, INTEROP_IFD | MAKER_NOTE | PIXEL_X | PIXEL_Y)
            })
            .map_or_else(|_| Vec::new(), |(entries, _)| entries);
    }
    if let Some(offset) = gps {
        directories.gps = walker
            .directory(u64::from(offset), |_| true)
            .map_or_else(|_| Vec::new(), |(entries, _)| entries);
    }
    for entries in [&mut directories.image, &mut directories.exif, &mut directories.gps] {
        walker.normalize(entries);
    }
    found.directories = directories;
    Ok(found)
}

/// An Exif block, with or without its `Exif\0\0` identifier.
pub(super) fn read_block(bytes: &[u8]) -> Result<Found, String> {
    let bytes = bytes.strip_prefix(b"Exif\0\0").unwrap_or(bytes);
    read(&mut Cursor::new(bytes), 0, bytes.len() as u64)
}

fn ifd_len(entries: &[Entry]) -> usize {
    6 + 12 * entries.len()
        + entries
            .iter()
            .filter(|e| e.value.len() > 4)
            .map(|e| e.value.len().next_multiple_of(2))
            .sum::<usize>()
}
fn write_ifd(out: &mut Vec<u8>, entries: &[Entry]) -> Result<(), String> {
    let mut entries: Vec<&Entry> = entries.iter().collect();
    entries.sort_by_key(|e| e.tag);
    let offset = |n: usize| u32::try_from(n).map_err(|_| "Exif metadata is too large".to_string());
    let mut data_at = out.len() + 6 + 12 * entries.len();
    let mut data = Vec::new();
    out.extend_from_slice(&u16::try_from(entries.len()).map_err(|_| INVALID)?.to_le_bytes());
    for entry in &entries {
        out.extend_from_slice(&entry.tag.to_le_bytes());
        out.extend_from_slice(&entry.kind.to_le_bytes());
        out.extend_from_slice(&entry.count.to_le_bytes());
        if entry.value.len() <= 4 {
            let mut inline = [0; 4];
            inline[..entry.value.len()].copy_from_slice(&entry.value);
            out.extend_from_slice(&inline);
        } else {
            out.extend_from_slice(&offset(data_at)?.to_le_bytes());
            data.extend_from_slice(&entry.value);
            if entry.value.len() % 2 == 1 {
                data.push(0);
            }
            data_at += entry.value.len().next_multiple_of(2);
        }
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&data);
    Ok(())
}

impl Directories {
    pub fn is_empty(&self) -> bool {
        self.image.is_empty() && self.exif.is_empty() && self.gps.is_empty()
    }
    /// A little-endian TIFF block, without the `Exif\0\0` identifier.
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        let mut image = self.image.clone();
        image.retain(|e| !matches!(e.tag, EXIF_IFD | GPS_IFD));
        let pointers = [(EXIF_IFD, &self.exif), (GPS_IFD, &self.gps)]
            .into_iter()
            .filter(|(_, entries)| !entries.is_empty())
            .collect::<Vec<_>>();
        image.extend(pointers.iter().map(|(tag, _)| Entry::long(*tag, 0)));
        let mut at = 8 + ifd_len(&image);
        for (tag, entries) in &pointers {
            let pointer = image.iter_mut().find(|e| e.tag == *tag).unwrap();
            pointer.value = u32::try_from(at).map_err(|_| INVALID)?.to_le_bytes().to_vec();
            at += ifd_len(entries);
        }
        let mut out = Vec::with_capacity(at);
        out.extend_from_slice(b"II\x2a\0\x08\0\0\0");
        write_ifd(&mut out, &image)?;
        for (_, entries) in pointers {
            write_ifd(&mut out, entries)?;
        }
        debug_assert_eq!(out.len(), at);
        Ok(out)
    }
}
