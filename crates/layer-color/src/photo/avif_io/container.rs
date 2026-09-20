//! Bounded, borrowing HEIF item reader. AV1 bytes and metadata stay in the
//! admitted input buffer; only item/property/reference records allocate here.
use std::{
    borrow::Cow,
    sync::atomic::{AtomicBool, Ordering},
};

pub(super) type Result<T> = std::result::Result<T, String>;
const MAX_RECORDS: usize = 65_536;
const MAX_ITEMS: usize = 4096;

#[derive(Clone, Copy, Debug)]
pub(super) struct BoxView<'a> {
    pub kind: [u8; 4],
    pub data: &'a [u8],
}
pub(super) struct Boxes<'a> {
    data: &'a [u8],
    count: usize,
}
pub(super) fn boxes(data: &[u8]) -> Boxes<'_> {
    Boxes { data, count: 0 }
}
impl<'a> Iterator for Boxes<'a> {
    type Item = Result<BoxView<'a>>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.data.is_empty() {
            return None;
        }
        let result = (|| {
            self.count += 1;
            if self.count > MAX_RECORDS {
                return Err("Too many AVIF boxes".into());
            }
            let mut r = Reader::new(self.data);
            let size = r.u32()?;
            let kind = r.take(4)?.try_into().unwrap();
            let size = match size {
                0 => self.data.len(),
                1 => usize::try_from(r.u64()?).map_err(|_| "AVIF box size overflow")?,
                n => n as usize,
            };
            let header = self.data.len() - r.left();
            if size < header || size > self.data.len() {
                return Err("Invalid AVIF box size".into());
            }
            let view = BoxView {
                kind,
                data: &self.data[header..size],
            };
            self.data = &self.data[size..];
            Ok(view)
        })();
        if result.is_err() {
            self.data = &[];
        }
        Some(result)
    }
}

#[derive(Clone, Copy)]
pub(super) struct Reader<'a> {
    pub data: &'a [u8],
}
impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data }
    }
    pub fn left(self) -> usize {
        self.data.len()
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let part = self.data.get(..n).ok_or("Truncated AVIF metadata")?;
        self.data = &self.data[n..];
        Ok(part)
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn uint(&mut self, n: usize) -> Result<u64> {
        if !matches!(n, 0 | 4 | 8) {
            return Err("Unsupported AVIF offset width".into());
        }
        let mut value = 0;
        for &b in self.take(n)? {
            value = (value << 8) | u64::from(b);
        }
        Ok(value)
    }
    pub fn full(&mut self) -> Result<(u8, u32)> {
        let word = self.u32()?;
        Ok(((word >> 24) as u8, word & 0x00ff_ffff))
    }
    pub fn end(self) -> Result<()> {
        if self.data.is_empty() {
            Ok(())
        } else {
            Err("Unexpected AVIF metadata bytes".into())
        }
    }
}

struct Budget<'a> {
    remaining: usize,
    used: usize,
    records: usize,
    cancel: &'a AtomicBool,
}
impl Budget<'_> {
    fn check(&self) -> Result<()> {
        if self.cancel.load(Ordering::Acquire) {
            Err("Image read cancelled".into())
        } else {
            Ok(())
        }
    }
    fn vec<T>(&mut self, count: usize, limit: usize) -> Result<Vec<T>> {
        self.check()?;
        if count > limit || count > MAX_RECORDS - self.records {
            return Err("Too many AVIF metadata records".into());
        }
        // Include allocator bookkeeping as well as exact requested capacity.
        let bytes = count
            .checked_mul(std::mem::size_of::<T>())
            .and_then(|n| n.checked_add(64))
            .ok_or("AVIF metadata size overflow")?;
        self.remaining = self
            .remaining
            .checked_sub(bytes)
            .ok_or("AVIF metadata exceeds the codec budget")?;
        self.used += bytes;
        self.records += count;
        let mut out = Vec::new();
        out.try_reserve_exact(count)
            .map_err(|_| "AVIF metadata allocation failed")?;
        Ok(out)
    }
    fn push<T>(&mut self, out: &mut Vec<T>, value: T) -> Result<()> {
        self.check()?;
        if self.records == MAX_RECORDS {
            return Err("Too many AVIF metadata records".into());
        }
        let bytes = std::mem::size_of::<T>() + 64;
        self.remaining = self
            .remaining
            .checked_sub(bytes)
            .ok_or("AVIF metadata exceeds the codec budget")?;
        self.used += bytes;
        self.records += 1;
        out.try_reserve_exact(1)
            .map_err(|_| "AVIF metadata allocation failed")?;
        out.push(value);
        Ok(())
    }
}

#[derive(Debug)]
pub(super) struct Item {
    pub id: u32,
    pub kind: [u8; 4],
    pub hidden: bool,
    pub protected: bool,
}
#[derive(Debug)]
struct Location {
    id: u32,
    method: u16,
    base: u64,
    extents: Vec<(u64, u64)>,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Association {
    pub index: usize,
    pub essential: bool,
}
#[derive(Debug)]
struct Associations {
    id: u32,
    links: Vec<Association>,
}
#[derive(Debug)]
pub(super) struct Reference {
    pub kind: [u8; 4],
    pub from: u32,
    pub to: Vec<u32>,
}

pub(super) struct Container<'a> {
    pub file: &'a [u8],
    pub primary: Option<u32>,
    pub items: Vec<Item>,
    pub properties: Vec<BoxView<'a>>,
    locations: Vec<Location>,
    associations: Vec<Associations>,
    pub references: Vec<Reference>,
    pub movie: Option<&'a [u8]>,
    pub groups: Option<&'a [u8]>,
    idat: Option<&'a [u8]>,
    pub metadata_bytes: usize,
}
impl<'a> Container<'a> {
    fn empty(file: &'a [u8]) -> Self {
        Self {
            file,
            primary: None,
            items: Vec::new(),
            properties: Vec::new(),
            locations: Vec::new(),
            associations: Vec::new(),
            references: Vec::new(),
            movie: None,
            groups: None,
            idat: None,
            metadata_bytes: 0,
        }
    }
    pub fn track_metadata(
        file: &'a [u8],
        meta: &'a [u8],
        budget: usize,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        let mut b = Budget {
            remaining: budget,
            used: 0,
            records: 0,
            cancel,
        };
        let mut me = Self::empty(file);
        me.parse_meta(meta, &mut b)?;
        me.finish(b.used)?;
        Ok(me)
    }
    pub fn parse(file: &'a [u8], budget: usize, cancel: &AtomicBool) -> Result<Self> {
        Self::parse_kind(file, budget, cancel, false)
    }
    pub fn parse_heif(file: &'a [u8], budget: usize, cancel: &AtomicBool) -> Result<Self> {
        Self::parse_kind(file, budget, cancel, true)
    }
    fn parse_kind(file: &'a [u8], budget: usize, cancel: &AtomicBool, heif: bool) -> Result<Self> {
        let mut b = Budget {
            remaining: budget,
            used: 0,
            records: 0,
            cancel,
        };
        let mut me = Self::empty(file);
        let mut ftyp = false;
        let mut meta = None;
        for view in boxes(file) {
            b.check()?;
            let view = view?;
            match &view.kind {
                b"ftyp" => {
                    if ftyp {
                        return Err("Duplicate AVIF file type".into());
                    }
                    let mut r = Reader::new(view.data);
                    let major = r.take(4)?;
                    r.take(4)?;
                    if r.left() % 4 != 0 {
                        return Err("Invalid AVIF brands".into());
                    }
                    ftyp = [major].into_iter().chain(r.data.chunks_exact(4)).any(|v| {
                        if heif {
                            matches!(v, b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1")
                        } else {
                            matches!(v, b"avif" | b"avis")
                        }
                    });
                    if !ftyp {
                        return Err("Not an AVIF image".into());
                    }
                }
                b"meta" => {
                    if meta.replace(view.data).is_some() {
                        return Err("Duplicate AVIF metadata".into());
                    }
                }
                b"moov" => {
                    if me.movie.replace(view.data).is_some() {
                        return Err("Duplicate AVIF movie".into());
                    }
                }
                _ => {}
            }
        }
        if !ftyp {
            return Err("Missing AVIF file type".into());
        }
        if let Some(meta) = meta {
            me.parse_meta(meta, &mut b)?;
        }
        if me.primary.is_none() && me.movie.is_none() {
            return Err("Missing AVIF primary image".into());
        }
        me.finish(b.used)?;
        Ok(me)
    }
    fn finish(&mut self, used: usize) -> Result<()> {
        self.items.sort_by_key(|v| v.id);
        self.locations.sort_by_key(|v| v.id);
        self.associations.sort_by_key(|v| v.id);
        if self.items.windows(2).any(|v| v[0].id == v[1].id)
            || self.locations.windows(2).any(|v| v[0].id == v[1].id)
            || self.associations.windows(2).any(|v| v[0].id == v[1].id)
        {
            return Err("Duplicate AVIF item records".into());
        }
        self.metadata_bytes = used;
        Ok(())
    }
    fn parse_meta(&mut self, bytes: &'a [u8], b: &mut Budget<'_>) -> Result<()> {
        let mut r = Reader::new(bytes);
        if r.full()? != (0, 0) {
            return Err("Unsupported AVIF meta version".into());
        }
        let mut seen = Vec::new();
        for view in boxes(r.data) {
            b.check()?;
            let view = view?;
            if matches!(
                &view.kind,
                b"hdlr" | b"pitm" | b"iinf" | b"iloc" | b"iprp" | b"iref" | b"idat" | b"grpl"
            ) {
                if seen.contains(&view.kind) {
                    return Err("Duplicate AVIF metadata box".into());
                }
                b.push(&mut seen, view.kind)?;
            }
            match &view.kind {
                b"hdlr" => {
                    let mut r = Reader::new(view.data);
                    if r.full()? != (0, 0) {
                        return Err("Invalid AVIF handler".into());
                    }
                    r.take(4)?;
                    if r.take(4)? != b"pict" {
                        return Err("Unsupported AVIF metadata handler".into());
                    }
                }
                b"pitm" => {
                    let mut r = Reader::new(view.data);
                    let (v, flags) = r.full()?;
                    if v > 1 || flags != 0 {
                        return Err("Invalid AVIF primary item".into());
                    }
                    self.primary = Some(if v == 0 { r.u16()? as u32 } else { r.u32()? });
                    r.end()?;
                }
                b"iinf" => self.parse_items(view.data, b)?,
                b"iloc" => self.parse_locations(view.data, b)?,
                b"iprp" => self.parse_properties(view.data, b)?,
                b"iref" => self.parse_references(view.data, b)?,
                b"idat" => self.idat = Some(view.data),
                b"grpl" => self.groups = Some(view.data),
                _ => {}
            }
        }
        Ok(())
    }
    fn parse_items(&mut self, bytes: &[u8], b: &mut Budget<'_>) -> Result<()> {
        let mut r = Reader::new(bytes);
        let (v, flags) = r.full()?;
        if v > 1 || flags != 0 {
            return Err("Unsupported AVIF item info version".into());
        }
        let count = if v == 0 {
            r.u16()? as usize
        } else {
            r.u32()? as usize
        };
        if count > r.left() / 20 {
            return Err("Truncated AVIF item info".into());
        }
        self.items = b.vec(count, MAX_ITEMS)?;
        for view in boxes(r.data) {
            let view = view?;
            if &view.kind != b"infe" || self.items.len() == count {
                return Err("Invalid AVIF item info".into());
            }
            let mut r = Reader::new(view.data);
            let (v, flags) = r.full()?;
            if !(2..=3).contains(&v) || flags & !1 != 0 {
                return Err("Unsupported AVIF item info".into());
            }
            let id = if v == 2 { r.u16()? as u32 } else { r.u32()? };
            let protected = r.u16()? != 0;
            let kind = r.take(4)?.try_into().unwrap();
            if !r.data.contains(&0) {
                return Err("Invalid AVIF item name".into());
            }
            self.items.push(Item {
                id,
                kind,
                hidden: flags & 1 != 0,
                protected,
            });
        }
        if self.items.len() != count {
            return Err("Truncated AVIF item info".into());
        }
        Ok(())
    }
    fn parse_locations(&mut self, bytes: &[u8], b: &mut Budget<'_>) -> Result<()> {
        let mut r = Reader::new(bytes);
        let (v, flags) = r.full()?;
        if v > 2 || flags != 0 {
            return Err("Unsupported AVIF location version".into());
        }
        let sizes = r.u8()?;
        let bases = r.u8()?;
        let (off, len, base, index) = (
            (sizes >> 4) as usize,
            (sizes & 15) as usize,
            (bases >> 4) as usize,
            if v == 0 { 0 } else { (bases & 15) as usize },
        );
        if [off, len, base, index]
            .iter()
            .any(|n| !matches!(n, 0 | 4 | 8))
        {
            return Err("Invalid AVIF location field size".into());
        }
        let count = if v < 2 {
            r.u16()? as usize
        } else {
            r.u32()? as usize
        };
        if count
            > r.left()
                / (6 + base
                    + if v == 0 {
                        0
                    } else if v == 1 {
                        2
                    } else {
                        4
                    })
        {
            return Err("Truncated AVIF locations".into());
        }
        self.locations = b.vec(count, MAX_ITEMS)?;
        for _ in 0..count {
            let id = if v < 2 { r.u16()? as u32 } else { r.u32()? };
            let method = if v == 0 { 0 } else { r.u16()? };
            if method > 1 || r.u16()? != 0 {
                return Err("Unsupported AVIF item data reference".into());
            }
            let base = r.uint(base)?;
            let count = r.u16()? as usize;
            let width = off + len + index;
            if count == 0 || width == 0 || count > r.left() / width {
                return Err("Invalid AVIF item extents".into());
            }
            let mut extents = b.vec(count, MAX_RECORDS)?;
            for _ in 0..count {
                if r.uint(index)? != 0 {
                    return Err("Unsupported AVIF extent index".into());
                }
                extents.push((r.uint(off)?, r.uint(len)?));
            }
            self.locations.push(Location {
                id,
                method,
                base,
                extents,
            });
        }
        r.end()
    }
    fn parse_properties(&mut self, bytes: &'a [u8], b: &mut Budget<'_>) -> Result<()> {
        let mut store = false;
        for view in boxes(bytes) {
            let view = view?;
            match &view.kind {
                b"ipco" => {
                    if store {
                        return Err("Duplicate AVIF property store".into());
                    }
                    store = true;
                    for property in boxes(view.data) {
                        b.push(&mut self.properties, property?)?;
                    }
                }
                b"ipma" => {
                    let mut r = Reader::new(view.data);
                    let (v, flags) = r.full()?;
                    if v > 1 || flags > 1 {
                        return Err("Unsupported AVIF property association version".into());
                    }
                    let count = r.u32()? as usize;
                    if count > MAX_ITEMS || count > r.left() / if v == 0 { 3 } else { 5 } {
                        return Err("Invalid AVIF association count".into());
                    }
                    for _ in 0..count {
                        let id = if v == 0 { r.u16()? as u32 } else { r.u32()? };
                        let count = r.u8()? as usize;
                        if count > r.left() / (flags as usize + 1) {
                            return Err("Truncated AVIF associations".into());
                        }
                        let mut links = b.vec(count, 255)?;
                        for _ in 0..count {
                            let (value, bit) = if flags == 0 {
                                (r.u8()? as usize, 0x80)
                            } else {
                                (r.u16()? as usize, 0x8000)
                            };
                            let index = value & (bit - 1);
                            if index != 0 {
                                links.push(Association {
                                    index: index - 1,
                                    essential: value & bit != 0,
                                });
                            }
                        }
                        b.push(&mut self.associations, Associations { id, links })?;
                    }
                    r.end()?;
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn parse_references(&mut self, bytes: &[u8], b: &mut Budget<'_>) -> Result<()> {
        let mut r = Reader::new(bytes);
        let (v, flags) = r.full()?;
        if v > 1 || flags != 0 {
            return Err("Unsupported AVIF reference version".into());
        }
        for view in boxes(r.data) {
            let view = view?;
            let mut r = Reader::new(view.data);
            let from = if v == 0 { r.u16()? as u32 } else { r.u32()? };
            let count = r.u16()? as usize;
            if count > r.left() / if v == 0 { 2 } else { 4 } {
                return Err("Truncated AVIF references".into());
            }
            let mut to = b.vec(count, MAX_ITEMS)?;
            for _ in 0..count {
                to.push(if v == 0 { r.u16()? as u32 } else { r.u32()? });
            }
            r.end()?;
            b.push(
                &mut self.references,
                Reference {
                    kind: view.kind,
                    from,
                    to,
                },
            )?;
        }
        Ok(())
    }
    pub fn item(&self, id: u32) -> Result<&Item> {
        let at = self
            .items
            .binary_search_by_key(&id, |v| v.id)
            .map_err(|_| "Missing AVIF item")?;
        let item = &self.items[at];
        if item.protected {
            return Err("Protected AVIF images are unsupported".into());
        }
        Ok(item)
    }
    pub fn links(&self, id: u32) -> &[Association] {
        self.associations
            .binary_search_by_key(&id, |v| v.id)
            .ok()
            .map(|i| self.associations[i].links.as_slice())
            .unwrap_or(&[])
    }
    pub fn property(&self, link: Association) -> Result<BoxView<'a>> {
        self.properties
            .get(link.index)
            .copied()
            .ok_or_else(|| "Invalid AVIF property index".into())
    }
    pub fn targets(&self, id: u32, kind: &[u8; 4]) -> Result<&[u32]> {
        let mut found = self
            .references
            .iter()
            .filter(|r| r.from == id && &r.kind == kind);
        let out = found.next().map(|v| v.to.as_slice()).unwrap_or(&[]);
        if found.next().is_some() {
            return Err("Duplicate AVIF item references".into());
        }
        Ok(out)
    }
    pub fn payload(&self, id: u32, budget: usize) -> Result<Cow<'a, [u8]>> {
        self.item(id)?;
        let at = self
            .locations
            .binary_search_by_key(&id, |v| v.id)
            .map_err(|_| "Missing AVIF item location")?;
        let loc = &self.locations[at];
        let source = if loc.method == 0 {
            self.file
        } else {
            self.idat.ok_or("Missing AVIF item data box")?
        };
        let part = |(offset, length): (u64, u64)| -> Result<&'a [u8]> {
            let start = usize::try_from(
                loc.base
                    .checked_add(offset)
                    .ok_or("AVIF item offset overflow")?,
            )
            .map_err(|_| "AVIF item offset overflow")?;
            let length = usize::try_from(length).map_err(|_| "AVIF item length overflow")?;
            let end = if length == 0 {
                source.len()
            } else {
                start.checked_add(length).ok_or("AVIF item size overflow")?
            };
            source
                .get(start..end)
                .ok_or_else(|| "AVIF item extends beyond its data".into())
        };
        let mut size = 0usize;
        for &extent in &loc.extents {
            size = size
                .checked_add(part(extent)?.len())
                .filter(|&v| v <= budget)
                .ok_or("AVIF item exceeds the codec budget")?;
        }
        if loc.extents.len() == 1 {
            return Ok(Cow::Borrowed(part(loc.extents[0])?));
        }
        let mut out = Vec::new();
        out.try_reserve_exact(size)
            .map_err(|_| "AVIF item allocation failed")?;
        for &extent in &loc.extents {
            out.extend_from_slice(part(extent)?);
        }
        Ok(Cow::Owned(out))
    }
}
