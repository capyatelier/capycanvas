use super::{PortableId, Resource};
use crate::color::RgbColor;
use std::{collections::{BTreeMap, HashSet}, sync::{Arc, OnceLock}};

pub const ANCHOR_RUN: usize = 256;
pub const CHUNK_RECORDS: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId(pub u64);
impl ItemId {
    pub fn random() -> Self {
        loop {
            let bytes = PortableId::random().bytes();
            let value = u64::from_le_bytes(bytes[..8].try_into().unwrap());
            if value != 0 { return Self(value); }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AnchorKind(pub u8);
impl AnchorKind {
    pub const CORNER: Self = Self(0);
    pub const SMOOTH: Self = Self(1);
    pub const SYMMETRIC: Self = Self(2);
    pub fn editable(self) -> Self { if self.0 <= 2 { self } else { Self::CORNER } }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor { pub point: [f64; 2], pub in_handle: [f64; 2], pub out_handle: [f64; 2], pub kind: AnchorKind }
impl Anchor {
    pub fn corner(point: [f64; 2]) -> Self { Self { point, in_handle: [0.; 2], out_handle: [0.; 2], kind: AnchorKind::CORNER } }
    pub fn incoming(&self) -> [f64; 2] { [self.point[0] + self.in_handle[0], self.point[1] + self.in_handle[1]] }
    pub fn outgoing(&self) -> [f64; 2] { [self.point[0] + self.out_handle[0], self.point[1] + self.out_handle[1]] }
    fn finite(&self) -> bool { self.point.iter().chain(&self.in_handle).chain(&self.out_handle).all(|v| v.is_finite()) }
    pub fn transformed(&self, map: impl Fn([f64; 2]) -> [f64; 2]) -> Self {
        let point = map(self.point);
        let relative = |handle: [f64; 2]| { let [x, y] = map(handle); [x - point[0], y - point[1]] };
        Self { point, in_handle: relative(self.incoming()), out_handle: relative(self.outgoing()), kind: self.kind }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment { pub from: [f64; 2], pub control: [[f64; 2]; 2], pub to: [f64; 2] }
impl Segment {
    pub fn is_line(&self) -> bool { self.control[0] == self.from && self.control[1] == self.to }
    pub fn point(&self, t: f64) -> [f64; 2] {
        let u = 1. - t;
        let [a, b, c, d] = [u * u * u, 3. * u * u * t, 3. * u * t * t, t * t * t];
        std::array::from_fn(|i| a * self.from[i] + b * self.control[0][i] + c * self.control[1][i] + d * self.to[i])
    }
    pub fn split(&self, t: f64) -> (Self, Self) {
        let lerp = |a: [f64; 2], b: [f64; 2]| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        let [p0, p1, p2, p3] = [self.from, self.control[0], self.control[1], self.to];
        let (a, b, c) = (lerp(p0, p1), lerp(p1, p2), lerp(p2, p3));
        let (d, e) = (lerp(a, b), lerp(b, c));
        let mid = lerp(d, e);
        (Self { from: p0, control: [a, d], to: mid }, Self { from: mid, control: [e, c], to: p3 })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Subpath { runs: Box<[Arc<[Anchor]>]>, len: usize, pub closed: bool }
impl Subpath {
    pub fn new(anchors: &[Anchor], closed: bool) -> Self {
        Self { runs: anchors.chunks(ANCHOR_RUN).map(Arc::from).collect(), len: anchors.len(), closed }
    }
    pub fn len(&self) -> usize { self.len }
    pub fn is_empty(&self) -> bool { self.len == 0 }
    pub fn runs(&self) -> &[Arc<[Anchor]>] { &self.runs }
    pub fn anchor(&self, index: usize) -> &Anchor { &self.runs[index / ANCHOR_RUN][index % ANCHOR_RUN] }
    pub fn anchors(&self) -> impl ExactSizeIterator<Item = &Anchor> + DoubleEndedIterator + Clone {
        AnchorIter { subpath: self, front: 0, back: self.len }
    }
    pub fn to_vec(&self) -> Vec<Anchor> { self.anchors().copied().collect() }
    pub fn segment_count(&self) -> usize { if self.closed { self.len } else { self.len.saturating_sub(1) } }
    pub fn segment(&self, index: usize) -> Segment {
        let (a, b) = (self.anchor(index), self.anchor((index + 1) % self.len));
        Segment { from: a.point, control: [a.outgoing(), b.incoming()], to: b.point }
    }
    pub fn segments(&self) -> impl Iterator<Item = Segment> + '_ { (0..self.segment_count()).map(|i| self.segment(i)) }
    pub fn with_anchor(&self, index: usize, anchor: Anchor) -> Self {
        let mut runs = self.runs.to_vec();
        let run = &mut runs[index / ANCHOR_RUN];
        let mut anchors = run.to_vec();
        anchors[index % ANCHOR_RUN] = anchor;
        *run = anchors.into();
        Self { runs: runs.into(), len: self.len, closed: self.closed }
    }
    pub fn map_runs(&self, map: impl Fn(&Anchor) -> Anchor) -> Self {
        Self { runs: self.runs.iter().map(|run| run.iter().map(&map).collect()).collect(), len: self.len, closed: self.closed }
    }
}
#[derive(Clone)]
struct AnchorIter<'a> { subpath: &'a Subpath, front: usize, back: usize }
impl<'a> Iterator for AnchorIter<'a> {
    type Item = &'a Anchor;
    fn next(&mut self) -> Option<&'a Anchor> {
        (self.front < self.back).then(|| { self.front += 1; self.subpath.anchor(self.front - 1) })
    }
    fn size_hint(&self) -> (usize, Option<usize>) { (self.back - self.front, Some(self.back - self.front)) }
}
impl DoubleEndedIterator for AnchorIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        (self.front < self.back).then(|| { self.back -= 1; self.subpath.anchor(self.back) })
    }
}
impl ExactSizeIterator for AnchorIter<'_> {}

#[derive(Clone, Debug, PartialEq)]
pub struct PathGeometry { subpaths: Box<[Subpath]>, hull: [f64; 4], anchors: usize, segments: usize }
impl PathGeometry {
    pub fn new(subpaths: Vec<Subpath>) -> Result<Self, &'static str> {
        if subpaths.is_empty() { return Err("A path requires a subpath"); }
        let mut hull = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        let (mut anchors, mut segments) = (0usize, 0usize);
        for subpath in &subpaths {
            if subpath.is_empty() { return Err("A subpath requires an anchor"); }
            anchors += subpath.len();
            segments += subpath.segment_count();
            for anchor in subpath.anchors() {
                if !anchor.finite() { return Err("Path coordinates must be finite"); }
                for point in [anchor.point, anchor.incoming(), anchor.outgoing()] {
                    if !point.iter().all(|v| v.is_finite()) { return Err("Path coordinates must be finite"); }
                    hull = [hull[0].min(point[0]), hull[1].min(point[1]), hull[2].max(point[0]), hull[3].max(point[1])];
                }
            }
        }
        Ok(Self { subpaths: subpaths.into(), hull, anchors, segments })
    }
    pub fn subpaths(&self) -> &[Subpath] { &self.subpaths }
    pub fn hull(&self) -> [f64; 4] { self.hull }
    pub fn anchor_count(&self) -> usize { self.anchors }
    pub fn segment_count(&self) -> usize { self.segments }
    pub fn segments(&self) -> impl Iterator<Item = Segment> + '_ { self.subpaths.iter().flat_map(Subpath::segments) }
    pub fn transformed(&self, map: impl Fn([f64; 2]) -> [f64; 2]) -> Result<Self, &'static str> {
        Self::new(self.subpaths.iter().map(|s| s.map_runs(|a| a.transformed(&map))).collect())
    }
    pub fn with_subpath(&self, index: usize, subpath: Subpath) -> Result<Self, &'static str> {
        let mut subpaths = self.subpaths.to_vec();
        subpaths[index] = subpath;
        Self::new(subpaths)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FillRule { #[default] NonZero, EvenOdd }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Cap { #[default] Butt, Round, Square }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Join { #[default] Miter, Round, Bevel }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WidthKnot { pub segment: u32, pub t: f64, pub left: f64, pub right: f64 }
#[derive(Clone, Debug, PartialEq)]
pub struct WidthProfile { knots: Box<[WidthKnot]> }
impl WidthProfile {
    pub fn new(knots: Vec<WidthKnot>) -> Result<Self, &'static str> {
        if knots.is_empty() { return Err("A width profile requires a knot"); }
        for knot in &knots {
            if !(0.0..=1.0).contains(&knot.t) { return Err("Width knot parameter is outside the segment"); }
            if !(knot.left.is_finite() && knot.right.is_finite() && knot.left >= 0. && knot.right >= 0.) { return Err("Width factors must be finite and nonnegative"); }
        }
        if knots.windows(2).any(|w| (w[0].segment, w[0].t) >= (w[1].segment, w[1].t)) { return Err("Width knots must be ordered along the path"); }
        Ok(Self { knots: knots.into() })
    }
    pub fn knots(&self) -> &[WidthKnot] { &self.knots }
    pub fn max_factor(&self) -> f64 { self.knots.iter().map(|k| k.left.max(k.right)).fold(0., f64::max) }
    pub fn at(&self, segment: u32, t: f64) -> [f64; 2] {
        let position = f64::from(segment) + t;
        let place = |k: &WidthKnot| f64::from(k.segment) + k.t;
        let after = self.knots.partition_point(|k| place(k) <= position);
        if after == 0 { let k = &self.knots[0]; return [k.left, k.right]; }
        if after == self.knots.len() { let k = &self.knots[after - 1]; return [k.left, k.right]; }
        let (a, b) = (&self.knots[after - 1], &self.knots[after]);
        let span = place(b) - place(a);
        let f = if span > 0. { (position - place(a)) / span } else { 0. };
        [a.left + (b.left - a.left) * f, a.right + (b.right - a.right) * f]
    }
    pub fn validate_for(&self, segments: usize) -> Result<(), &'static str> {
        if self.knots.iter().any(|k| k.segment as usize >= segments.max(1)) { return Err("Width knot names a missing segment"); }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OpaqueRecord { pub kind: u64, pub id: u64, pub properties: Arc<[u8]> }
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct UnknownProperty { pub tag: u64, pub value: Arc<[u8]> }
pub type Unknown = Option<Arc<[UnknownProperty]>>;

pub const IGNORABLE: u64 = 1;
pub const COPY_SAFE: u64 = 2;
pub const PARENT: u64 = 4;
pub fn retained(unknown: &Unknown, content_changed: bool) -> Unknown {
    if !content_changed { return unknown.clone(); }
    let kept: Vec<_> = unknown.iter().flat_map(|u| u.iter()).filter(|p| p.tag & COPY_SAFE != 0).cloned().collect();
    (!kept.is_empty()).then(|| kept.into())
}

#[derive(Clone, Debug, PartialEq)]
pub enum Paint { Color { color: RgbColor, unknown: Unknown }, Opaque(OpaqueRecord) }
impl Paint {
    pub fn color(color: RgbColor) -> Self { Self::Color { color, unknown: None } }
    pub fn rgb(&self) -> Option<RgbColor> { if let Self::Color { color, .. } = self { Some(*color) } else { None } }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Fill { pub paint: Paint, pub unknown: Unknown }
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub paint: Paint,
    pub width: f64,
    pub cap: Cap,
    pub join: Join,
    pub miter_limit: f64,
    pub profile: Option<Arc<WidthProfile>>,
    pub unknown: Unknown,
}
impl Stroke {
    pub const DEFAULT_WIDTH: f64 = 1.;
    pub const DEFAULT_MITER_LIMIT: f64 = 4.;
    pub fn new(paint: Paint, width: f64) -> Self {
        Self { paint, width, cap: Cap::Butt, join: Join::Miter, miter_limit: Self::DEFAULT_MITER_LIMIT, profile: None, unknown: None }
    }
    pub fn reach(&self) -> f64 {
        let half = self.width * 0.5 * self.profile.as_ref().map_or(1., |p| p.max_factor());
        let join = if self.join == Join::Miter { self.miter_limit.max(1.) } else { 1. };
        let cap = if self.cap == Cap::Square { std::f64::consts::SQRT_2 } else { 1. };
        half * join.max(cap)
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(self.width.is_finite() && self.width >= 0.) { return Err("Stroke width must be finite and nonnegative"); }
        if !(self.miter_limit.is_finite() && self.miter_limit >= 1.) { return Err("Miter limit must be at least one"); }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum PaintEntry { Fill(Fill), Stroke(Stroke), Opaque(OpaqueRecord) }
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style { pub entries: Box<[PaintEntry]> }
impl Style {
    pub fn new(entries: Vec<PaintEntry>) -> Self { Self { entries: entries.into() } }
    pub fn reach(&self) -> f64 {
        self.entries.iter().map(|e| if let PaintEntry::Stroke(s) = e { s.reach() } else { 0. }).fold(0., f64::max)
    }
    pub fn strokes(&self) -> impl Iterator<Item = &Stroke> {
        self.entries.iter().filter_map(|e| if let PaintEntry::Stroke(s) = e { Some(s) } else { None })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PathItem {
    pub id: ItemId,
    pub geometry: Arc<PathGeometry>,
    pub style: Arc<Style>,
    pub fill_rule: FillRule,
    pub hidden: bool,
    pub locked: bool,
    pub name: Option<Arc<str>>,
    pub unknown: Unknown,
}
impl PathItem {
    pub fn new(id: ItemId, geometry: Arc<PathGeometry>, style: Arc<Style>) -> Self {
        Self { id, geometry, style, fill_rule: FillRule::NonZero, hidden: false, locked: false, name: None, unknown: None }
    }
    pub fn bounds(&self) -> [f64; 4] {
        let [x0, y0, x1, y1] = self.geometry.hull();
        let reach = self.style.reach();
        [x0 - reach, y0 - reach, x1 + reach, y1 + reach]
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.id.0 == 0 { return Err("A path requires a nonzero identity"); }
        for stroke in self.style.strokes() {
            stroke.validate()?;
            if let Some(profile) = &stroke.profile { profile.validate_for(self.geometry.segment_count())?; }
        }
        Ok(())
    }
    pub fn with_geometry(&self, geometry: Arc<PathGeometry>) -> Self {
        Self { geometry, unknown: retained(&self.unknown, true), ..self.clone() }
    }
    pub fn with_style(&self, style: Arc<Style>) -> Self {
        Self { style, unknown: retained(&self.unknown, true), ..self.clone() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Record { Path(PathItem), Opaque(OpaqueRecord), End }
impl Record {
    pub fn path(&self) -> Option<&PathItem> { if let Self::Path(p) = self { Some(p) } else { None } }
    pub fn id(&self) -> Option<ItemId> {
        match self { Self::Path(p) => Some(p.id), Self::Opaque(o) if o.id != 0 => Some(ItemId(o.id)), _ => None }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChunkSummary { pub bounds: Option<[f64; 4]>, pub paths: u32, pub anchors: u64, pub opaque: u32 }
impl ChunkSummary {
    fn of(records: &[Record]) -> Self {
        let mut summary = Self::default();
        for record in records {
            match record {
                Record::Path(path) => {
                    summary.paths += 1;
                    summary.anchors += path.geometry.anchor_count() as u64;
                    if !path.hidden { summary.bounds = Some(union(summary.bounds, path.bounds())); }
                }
                Record::Opaque(_) => summary.opaque += 1,
                Record::End => {}
            }
        }
        summary
    }
}
pub fn union(a: Option<[f64; 4]>, b: [f64; 4]) -> [f64; 4] {
    a.map_or(b, |a| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
}

#[derive(Clone, Debug)]
pub struct VectorChunk { records: Box<[Record]>, summary: ChunkSummary, encoded_len: OnceLock<usize> }
impl PartialEq for VectorChunk { fn eq(&self, other: &Self) -> bool { self.records == other.records } }
impl VectorChunk {
    pub fn new(records: Vec<Record>) -> Self {
        let summary = ChunkSummary::of(&records);
        Self { records: records.into(), summary, encoded_len: OnceLock::new() }
    }
    pub fn encoded_len(&self) -> Option<usize> { self.encoded_len.get().copied() }
    pub fn note_encoded_len(&self, length: usize) { let _ = self.encoded_len.set(length); }
    pub fn records(&self) -> &[Record] { &self.records }
    pub fn summary(&self) -> &ChunkSummary { &self.summary }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position { pub chunk: u32, pub index: u32 }

#[derive(Clone, Debug, Default)]
pub struct VectorObject { chunks: Arc<[Resource<VectorChunk>]> }
impl PartialEq for VectorObject {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.chunks, &other.chunks) || (self.chunks.len() == other.chunks.len()
            && self.chunks.iter().zip(other.chunks.iter()).all(|(a, b)| a.same_owner(b) || **a == **b))
    }
}
impl VectorObject {
    pub fn from_chunks(chunks: Vec<Resource<VectorChunk>>) -> Self { Self { chunks: chunks.into() } }
    pub fn from_records(records: Vec<Record>) -> Self { Self::from_chunks(pack(records)) }
    pub fn chunks(&self) -> &[Resource<VectorChunk>] { &self.chunks }
    pub fn same_root(&self, other: &Self) -> bool { Arc::ptr_eq(&self.chunks, &other.chunks) }
    pub fn records(&self) -> impl DoubleEndedIterator<Item = (Position, &Record)> {
        self.chunks.iter().enumerate().flat_map(|(c, chunk)| chunk.records().iter().enumerate()
            .map(move |(i, record)| (Position { chunk: c as u32, index: i as u32 }, record)))
    }
    pub fn paths(&self) -> impl DoubleEndedIterator<Item = (Position, &PathItem)> {
        self.records().filter_map(|(p, r)| r.path().map(|path| (p, path)))
    }
    pub fn record(&self, position: Position) -> Option<&Record> {
        self.chunks.get(position.chunk as usize)?.records().get(position.index as usize)
    }
    pub fn record_count(&self) -> usize { self.chunks.iter().map(|c| c.records().len()).sum() }
    pub fn path_count(&self) -> usize { self.chunks.iter().map(|c| c.summary().paths as usize).sum() }
    pub fn anchor_count(&self) -> u64 { self.chunks.iter().map(|c| c.summary().anchors).sum() }
    pub fn bounds(&self) -> Option<[f64; 4]> {
        self.chunks.iter().filter_map(|c| c.summary().bounds).fold(None, |a, b| Some(union(a, b)))
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        let mut ids = HashSet::with_capacity(self.record_count());
        let mut open = 0u32;
        for (_, record) in self.records() {
            match record {
                Record::Path(path) if open == 0 => path.validate()?,
                Record::Path(_) => return Err("Contents of unknown containers stay uninterpreted"),
                Record::Opaque(opaque) => if opaque.kind & PARENT != 0 { open += 1; },
                Record::End => open = open.checked_sub(1).ok_or("Unbalanced end marker")?,
            }
            if let Some(id) = record.id() && !ids.insert(id) { return Err("Duplicate item identity"); }
        }
        if open == 0 { Ok(()) } else { Err("Unbalanced end marker") }
    }
    pub fn validate_changed(&self, previous: &Self) -> Result<(), &'static str> {
        let kept: HashSet<usize> = previous.chunks.iter().map(|c| Arc::as_ptr(c.storage()) as usize).collect();
        for chunk in self.chunks.iter().filter(|c| !kept.contains(&(Arc::as_ptr(c.storage()) as usize))) {
            let mut ids = HashSet::with_capacity(chunk.records().len());
            for record in chunk.records() {
                if let Record::Path(path) = record { path.validate()?; }
                if let Some(id) = record.id() && !ids.insert(id) { return Err("Duplicate item identity"); }
            }
        }
        Ok(())
    }
    pub fn find(&self, id: ItemId) -> Option<Position> { self.records().find(|(_, r)| r.id() == Some(id)).map(|(p, _)| p) }
    pub fn ids(&self) -> HashSet<ItemId> { self.records().filter_map(|(_, r)| r.id()).collect() }
    pub fn allocate_id(&self, used: impl Fn(ItemId) -> bool) -> ItemId {
        loop { let id = ItemId::random(); if !used(id) { return id; } }
    }
    pub fn insert_front(&self, records: Vec<Record>) -> Self {
        if records.is_empty() { return self.clone(); }
        let mut edits = BTreeMap::new();
        if self.chunks.is_empty() { return Self::from_records(records); }
        let first = &self.chunks[0];
        if first.records().len() + records.len() <= CHUNK_RECORDS {
            let mut joined = records;
            joined.extend(first.records().iter().cloned());
            edits.insert(0, joined);
            return self.edit_chunks(edits);
        }
        let mut chunks = pack(records);
        chunks.extend(self.chunks.iter().cloned());
        Self::from_chunks(chunks)
    }
    pub fn replace(&self, changes: impl IntoIterator<Item = (Position, Option<Record>)>) -> Self {
        let mut grouped: BTreeMap<u32, Vec<(u32, Option<Record>)>> = BTreeMap::new();
        for (position, record) in changes { grouped.entry(position.chunk).or_default().push((position.index, record)); }
        let edits = grouped.into_iter().map(|(chunk, changes)| {
            let mut records: Vec<Option<Record>> = self.chunks[chunk as usize].records().iter().cloned().map(Some).collect();
            for (index, record) in changes { records[index as usize] = record; }
            (chunk as usize, records.into_iter().flatten().collect())
        }).collect();
        self.edit_chunks(edits)
    }
    pub fn edit_chunks(&self, edits: BTreeMap<usize, Vec<Record>>) -> Self {
        let mut chunks: Vec<Resource<VectorChunk>> = Vec::with_capacity(self.chunks.len() + edits.len());
        for (index, chunk) in self.chunks.iter().enumerate() {
            let Some(records) = edits.get(&index) else { chunks.push(chunk.clone()); continue };
            for packed in pack(records.clone()) {
                if let Some(last) = chunks.last()
                    && packed.records().len() < CHUNK_RECORDS / 4 && last.records().len() + packed.records().len() <= CHUNK_RECORDS / 2 {
                    let mut joined = last.records().to_vec();
                    joined.extend(packed.records().iter().cloned());
                    *chunks.last_mut().unwrap() = Resource::new(Arc::new(VectorChunk::new(joined)));
                } else {
                    chunks.push(packed);
                }
            }
        }
        Self::from_chunks(chunks)
    }
}
fn pack(records: Vec<Record>) -> Vec<Resource<VectorChunk>> {
    if records.is_empty() { return Vec::new(); }
    let pieces = records.len().div_ceil(CHUNK_RECORDS);
    let per = records.len().div_ceil(pieces);
    let mut records = records.into_iter();
    (0..pieces).map(|_| Resource::new(Arc::new(VectorChunk::new(records.by_ref().take(per).collect())))).collect()
}
