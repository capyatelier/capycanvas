use super::{Affine64, Resource};
use crate::color::RgbColor;
use std::{collections::{BTreeMap, BTreeSet, HashSet}, hash::{BuildHasherDefault, Hasher}, sync::{Arc, OnceLock}};

mod erase;
mod faces;
mod index;
mod ops;
mod outline;
#[cfg(test)]
mod ops_tests;
mod points;
mod trace;
pub use erase::{EraseMode, Eraser};
pub use faces::{FaceLine, SealId, VectorFace, vector_face, vector_faces};
pub use points::{AnchorRef, SegmentHit};
pub use index::VectorIndex;
pub use trace::ContourTrace;

pub const ANCHOR_RUN: usize = 256;
pub const CHUNK_RECORDS: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrange { Front, Forward, Backward, Back }

#[derive(Default)]
pub struct IdHasher(u64);
impl Hasher for IdHasher {
    #[inline] fn finish(&self) -> u64 { self.0 }
    #[inline] fn write(&mut self, bytes: &[u8]) {
        let mut words = bytes.chunks_exact(8);
        for word in &mut words { self.write_u64(u64::from_le_bytes(word.try_into().unwrap())); }
        let rest = words.remainder();
        if !rest.is_empty() { let mut word = [0; 8]; word[..rest.len()].copy_from_slice(rest); self.write_u64(u64::from_le_bytes(word)); }
    }
    #[inline] fn write_u64(&mut self, value: u64) { self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x9e37_79b9_7f4a_7c15); }
    #[inline] fn write_u128(&mut self, value: u128) { self.write_u64(value as u64); self.write_u64((value >> 64) as u64); }
    #[inline] fn write_u32(&mut self, value: u32) { self.write_u64(u64::from(value)); }
    #[inline] fn write_usize(&mut self, value: usize) { self.write_u64(value as u64); }
    #[inline] fn write_i32(&mut self, value: i32) { self.write_u64(u64::from(value as u32)); }
}
pub type IdBuild = BuildHasherDefault<IdHasher>;
type IdSet = HashSet<ItemId, IdBuild>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AnchorKind(pub u8);
impl AnchorKind {
    pub const CORNER: Self = Self(0);
    pub const SMOOTH: Self = Self(1);
    pub const SYMMETRIC: Self = Self(2);
    pub fn editable(self) -> Self { if self.0 <= 2 { self } else { Self::CORNER } }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor { point: [f32; 2], in_handle: [f32; 2], out_handle: [f32; 2], pub kind: AnchorKind }
pub const GRID: f64 = 256.;
pub const MAX_COORDINATE: f64 = 8_388_608.;
pub fn snap(value: f64) -> f64 { (value * GRID).round() / GRID }
fn narrow(v: [f64; 2]) -> [f32; 2] { v.map(|value| snap(value) as f32) }
fn widen(v: [f32; 2]) -> [f64; 2] { [f64::from(v[0]), f64::from(v[1])] }
impl Anchor {
    pub fn new(point: [f64; 2], in_handle: [f64; 2], out_handle: [f64; 2], kind: AnchorKind) -> Self {
        Self { point: narrow(point), in_handle: narrow(in_handle), out_handle: narrow(out_handle), kind }
    }
    pub fn corner(point: [f64; 2]) -> Self { Self::new(point, [0.; 2], [0.; 2], AnchorKind::CORNER) }
    pub fn from_units(units: [i64; 6], kind: AnchorKind) -> Option<Self> {
        if !units.iter().all(|&unit| unit.unsigned_abs() < 1 << f32::MANTISSA_DIGITS || unit as f32 as i64 == unit) { return None; }
        let v = units.map(|unit| unit as f32 * (1. / GRID as f32));
        Some(Self { point: [v[0], v[1]], in_handle: [v[2], v[3]], out_handle: [v[4], v[5]], kind })
    }
    pub fn units(&self) -> [i64; 6] { self.bits().map(|value| (f64::from(value) * GRID) as i64) }
    pub fn bits(&self) -> [f32; 6] {
        [self.point[0], self.point[1], self.in_handle[0], self.in_handle[1], self.out_handle[0], self.out_handle[1]]
    }
    pub fn point(&self) -> [f64; 2] { widen(self.point) }
    pub fn in_handle(&self) -> [f64; 2] { widen(self.in_handle) }
    pub fn out_handle(&self) -> [f64; 2] { widen(self.out_handle) }
    pub fn with_out_handle(self, handle: [f64; 2]) -> Self { Self { out_handle: narrow(handle), ..self } }
    pub fn incoming(&self) -> [f64; 2] { let [p, h] = [self.point(), self.in_handle()]; [p[0] + h[0], p[1] + h[1]] }
    pub fn outgoing(&self) -> [f64; 2] { let [p, h] = [self.point(), self.out_handle()]; [p[0] + h[0], p[1] + h[1]] }
    fn finite(&self) -> bool { self.bits().iter().all(|v| v.is_finite() && f64::from(v.abs()) < MAX_COORDINATE) }
    pub fn transformed(&self, map: impl Fn([f64; 2]) -> [f64; 2]) -> Self {
        let point = map(self.point());
        let relative = |handle: [f64; 2]| { let [x, y] = map(handle); [x - point[0], y - point[1]] };
        Self::new(point, relative(self.incoming()), relative(self.outgoing()), self.kind)
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
    pub fn map(&self, map: impl Fn([f64; 2]) -> [f64; 2]) -> Self {
        Self { from: map(self.from), control: [map(self.control[0]), map(self.control[1])], to: map(self.to) }
    }
    pub fn flatten(&self, tolerance: f64, mut emit: impl FnMut([f64; 2], f64)) {
        let [p0, p1, p2, p3] = [self.from, self.control[0], self.control[1], self.to];
        let second = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| ((a[0] - 2. * b[0] + c[0]).powi(2) + (a[1] - 2. * b[1] + c[1]).powi(2)).sqrt();
        let bend = second(p0, p1, p2).max(second(p1, p2, p3));
        let count = if self.is_line() { 1 } else { ((0.75 * bend / tolerance).sqrt().ceil() as usize).clamp(1, 4096) };
        for i in 1..count { let t = i as f64 / count as f64; emit(self.point(t), t); }
        emit(p3, 1.);
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
        Segment { from: a.point(), control: [a.outgoing(), b.incoming()], to: b.point() }
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

#[derive(Clone, Debug)]
pub struct PathGeometry { subpaths: Box<[Subpath]>, hull: [f64; 4], anchors: usize, segments: usize, content: crate::raster::ContentName }
impl PartialEq for PathGeometry {
    fn eq(&self, other: &Self) -> bool { self.subpaths == other.subpaths }
}
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
                if !anchor.finite() { return Err("Path coordinates must be finite and within the admitted range"); }
                for point in [anchor.point(), anchor.incoming(), anchor.outgoing()] {
                    if !point.iter().all(|v| v.is_finite()) { return Err("Path coordinates must be finite"); }
                    hull = [hull[0].min(point[0]), hull[1].min(point[1]), hull[2].max(point[0]), hull[3].max(point[1])];
                }
            }
        }
        Ok(Self { subpaths: subpaths.into(), hull, anchors, segments, content: crate::raster::ContentName::fresh() })
    }
    pub fn content_version(&self) -> crate::raster::ContentVersion { self.content.into() }
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
pub struct WidthKnot { segment: u32, t: f32, left: f32, right: f32 }
impl WidthKnot {
    pub fn new(segment: u32, t: f64, left: f64, right: f64) -> Self { Self { segment, t: t as f32, left: left as f32, right: right as f32 } }
    pub fn bits(&self) -> [f32; 3] { [self.t, self.left, self.right] }
    pub fn segment(&self) -> u32 { self.segment }
    pub fn t(&self) -> f64 { f64::from(self.t) }
    pub fn left(&self) -> f64 { f64::from(self.left) }
    pub fn right(&self) -> f64 { f64::from(self.right) }
    fn place(&self) -> f64 { f64::from(self.segment) + self.t() }
}
#[derive(Clone, Debug, PartialEq)]
pub struct WidthProfile { knots: Box<[WidthKnot]> }
impl WidthProfile {
    pub fn segment_knots(&self, segment: u32) -> &[WidthKnot] {
        &self.knots[self.knots.partition_point(|k| k.segment() < segment)..self.knots.partition_point(|k| k.segment() <= segment)]
    }
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
    pub fn mirrored(&self) -> Option<Self> {
        self.knots.iter().any(|k| k.left != k.right).then(|| Self { knots: self.knots.iter().map(|k| WidthKnot { left: k.right, right: k.left, ..*k }).collect() })
    }
    pub fn max_factor(&self) -> f64 { self.knots.iter().map(|k| k.left().max(k.right())).fold(0., f64::max) }
    pub fn at(&self, segment: u32, t: f64) -> [f64; 2] {
        let position = f64::from(segment) + t;
        let after = self.knots.partition_point(|k| k.place() <= position);
        if after == 0 { let k = &self.knots[0]; return [k.left(), k.right()]; }
        if after == self.knots.len() { let k = &self.knots[after - 1]; return [k.left(), k.right()]; }
        let (a, b) = (&self.knots[after - 1], &self.knots[after]);
        let span = b.place() - a.place();
        let f = if span > 0. { (position - a.place()) / span } else { 0. };
        [a.left() + (b.left() - a.left()) * f, a.right() + (b.right() - a.right()) * f]
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
    pub fn flatten_segment(&self, segment: &Segment, index: u32, tolerance: f64, scale: f64, mut emit: impl FnMut([f64; 2], f64)) {
        let Some(profile) = &self.profile else { segment.flatten(tolerance, emit); return };
        let knots = profile.segment_knots(index);
        let first = profile.at(index, 0.);
        if profile.at(index, 1.) == first && knots.iter().all(|k| [k.left(), k.right()] == first) { segment.flatten(tolerance, emit); return; }
        let width = |t| profile.at(index, t).map(|f| f * self.width * 0.5 * scale);
        fn flatten(segment: &Segment, width: &impl Fn(f64) -> [f64; 2], knots: &[WidthKnot], [a, b]: [f64; 2], tolerance: f64, depth: u32, emit: &mut impl FnMut([f64; 2], f64)) {
            let knots = &knots[knots.partition_point(|k| k.t() <= a)..knots.partition_point(|k| k.t() < b)];
            let (from, to, wa, wb) = (segment.point(a), segment.point(b), width(a), width(b));
            let d = [to[0] - from[0], to[1] - from[1]];
            let length = d[0] * d[0] + d[1] * d[1];
            let accurate = [0.25, 0.5, 0.75].into_iter().map(|s| a + (b - a) * s)
                .chain(knots.iter().map(WidthKnot::t)).all(|t| {
                let s = (t - a) / (b - a);
                let (p, w) = (segment.point(t), width(t));
                let u = if length > 0. { (((p[0] - from[0]) * d[0] + (p[1] - from[1]) * d[1]) / length).clamp(0., 1.) } else { s };
                (0..2).map(|i| (w[i] - wa[i] - (wb[i] - wa[i]) * u).abs()).fold(0., f64::max) <= tolerance
            });
            if accurate || depth == 24 { emit(to, b); } else {
                let middle = (a + b) * 0.5;
                flatten(segment, width, knots, [a, middle], tolerance, depth + 1, emit);
                flatten(segment, width, knots, [middle, b], tolerance, depth + 1, emit);
            }
        }
        let mut from = 0.;
        segment.flatten(tolerance, |_, to| {
            flatten(segment, &width, knots, [from, to], tolerance, 0, &mut emit);
            from = to;
        });
    }
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
        if !(self.width.is_finite() && self.width >= 0. && self.width < MAX_COORDINATE) { return Err("Stroke width must be finite, nonnegative and within the admitted range"); }
        if !(self.miter_limit.is_finite() && self.miter_limit >= 1.) { return Err("Miter limit must be at least one"); }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum PaintEntry { Fill(Fill), Stroke(Stroke), Opaque(OpaqueRecord) }
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style { pub entries: Box<[PaintEntry]> }
impl Style {
    pub fn new(entries: Vec<PaintEntry>) -> Self {
        Self { entries: entries.into_iter().map(|entry| match entry { PaintEntry::Stroke(stroke) => PaintEntry::Stroke(Stroke { width: snap(stroke.width), ..stroke }), entry => entry }).collect() }
    }
    pub fn reach(&self) -> f64 {
        self.entries.iter().map(|e| if let PaintEntry::Stroke(s) = e { s.reach() } else { 0. }).fold(0., f64::max)
    }
    pub fn strokes(&self) -> impl Iterator<Item = &Stroke> {
        self.entries.iter().filter_map(|e| if let PaintEntry::Stroke(s) = e { Some(s) } else { None })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShapeKind { Rectangle { radii: [f64; 4] }, Ellipse, Polygon { corners: u32 }, Star { points: u32, inner: f64 } }
impl ShapeKind {
    pub const MAX_CORNERS: u32 = 100;
    pub fn valid(&self) -> bool {
        match *self {
            Self::Rectangle { radii } => radii.iter().all(|r| r.is_finite() && *r >= 0.),
            Self::Ellipse => true,
            Self::Polygon { corners } => (3..=Self::MAX_CORNERS).contains(&corners),
            Self::Star { points, inner } => (3..=Self::MAX_CORNERS).contains(&points) && inner > 0. && inner <= 1.,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveShape { pub kind: ShapeKind, pub center: [f64; 2], pub size: [f64; 2], pub rotation: f64 }
const ARC: f64 = 0.552_284_749_830_793_4;
impl LiveShape {
    pub fn geometry(&self) -> Result<PathGeometry, &'static str> {
        let [w, h] = self.size.map(|v| v * 0.5);
        let (sin, cos) = self.rotation.sin_cos();
        let place = |p: [f64; 2]| [self.center[0] + p[0] * cos - p[1] * sin, self.center[1] + p[0] * sin + p[1] * cos];
        let turn = |v: [f64; 2]| [v[0] * cos - v[1] * sin, v[0] * sin + v[1] * cos];
        if !self.kind.valid() { return Err("Invalid live shape"); }
        let around = |count: u32, radius: &dyn Fn(u32) -> f64| (0..count).map(|i| {
            let angle = -std::f64::consts::FRAC_PI_2 + std::f64::consts::TAU * f64::from(i) / f64::from(count);
            let r = radius(i);
            Anchor::corner(place([w * r * angle.cos(), h * r * angle.sin()]))
        }).collect::<Vec<_>>();
        let anchors: Vec<Anchor> = match self.kind {
            ShapeKind::Polygon { corners } => around(corners, &|_| 1.),
            ShapeKind::Star { points, inner } => around(points * 2, &|i| if i % 2 == 0 { 1. } else { inner }),
            ShapeKind::Ellipse => [([0., -h], [w, 0.]), ([w, 0.], [0., h]), ([0., h], [-w, 0.]), ([-w, 0.], [0., -h])].into_iter()
                .map(|(p, t)| Anchor::new(place(p), turn([-t[0] * ARC, -t[1] * ARC]), turn([t[0] * ARC, t[1] * ARC]), AnchorKind::SYMMETRIC)).collect(),
            ShapeKind::Rectangle { radii } => {
                let limit = w.min(h).max(0.);
                let corners = [[-w, -h], [w, -h], [w, h], [-w, h]];
                let mut anchors = Vec::with_capacity(8);
                for (i, corner) in corners.into_iter().enumerate() {
                    let r = radii[i].clamp(0., limit);
                    if r == 0. { anchors.push(Anchor::corner(place(corner))); continue; }
                    let (previous, next) = (corners[(i + 3) % 4], corners[(i + 1) % 4]);
                    let toward = |from: [f64; 2]| { let d = [from[0] - corner[0], from[1] - corner[1]]; let l = d[0].hypot(d[1]); [corner[0] + d[0] / l * r, corner[1] + d[1] / l * r] };
                    let (start, end) = (toward(previous), toward(next));
                    let handle = |from: [f64; 2]| turn([(corner[0] - from[0]) * ARC, (corner[1] - from[1]) * ARC]);
                    anchors.push(Anchor::new(place(start), [0.; 2], handle(start), AnchorKind::SMOOTH));
                    anchors.push(Anchor::new(place(end), handle(end), [0.; 2], AnchorKind::SMOOTH));
                }
                anchors
            }
        };
        PathGeometry::new(vec![Subpath::new(&anchors, true)])
    }
    pub fn transformed(&self, affine: Affine64) -> Option<Self> {
        let [a, b, c, d, _, _] = affine.0;
        let (sin, cos) = self.rotation.sin_cos();
        let map = |v: [f64; 2]| [a * v[0] + c * v[1], b * v[0] + d * v[1]];
        let (u, v) = (map([cos, sin]), map([-sin, cos]));
        let (su, sv) = (u[0].hypot(u[1]), v[0].hypot(v[1]));
        if su == 0. || sv == 0. || (u[0] * v[0] + u[1] * v[1]).abs() > 1e-9 * su * sv { return None; }
        let flipped = u[0] * v[1] - u[1] * v[0] < 0.;
        let kind = match self.kind {
            ShapeKind::Rectangle { radii } if radii.iter().any(|r| *r > 0.) && (su - sv).abs() > 1e-9 * su.max(sv) => return None,
            ShapeKind::Rectangle { radii: [r0, r1, r2, r3] } => ShapeKind::Rectangle { radii: if flipped { [r3, r2, r1, r0] } else { [r0, r1, r2, r3] }.map(|r| r * su) },
            kind => kind,
        };
        let turned = if flipped && matches!(kind, ShapeKind::Polygon { .. } | ShapeKind::Star { .. }) { std::f64::consts::PI } else { 0. };
        Some(Self { kind, center: affine.map(self.center), size: [self.size[0] * su, self.size[1] * sv], rotation: u[1].atan2(u[0]) + turned })
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
    pub shape: Option<Arc<LiveShape>>,
    pub unknown: Unknown,
}
impl PathItem {
    pub fn new(id: ItemId, geometry: Arc<PathGeometry>, style: Arc<Style>) -> Self {
        Self { id, geometry, style, fill_rule: FillRule::NonZero, hidden: false, locked: false, name: None, shape: None, unknown: None }
    }
    pub fn bounds(&self) -> [f64; 4] {
        let [x0, y0, x1, y1] = self.geometry.hull();
        let reach = self.style.reach();
        [x0 - reach, y0 - reach, x1 + reach, y1 + reach]
    }
    pub fn pickable(&self) -> bool { !self.hidden && !self.locked }
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.id.0 == 0 { return Err("A path requires a nonzero identity"); }
        for stroke in self.style.strokes() {
            stroke.validate()?;
            if let Some(profile) = &stroke.profile { profile.validate_for(self.geometry.segment_count())?; }
        }
        Ok(())
    }
    pub fn hit(&self, point: [f64; 2], slop: f64) -> bool {
        let [x0, y0, x1, y1] = self.bounds();
        if self.hidden || point[0] < x0 - slop || point[1] < y0 - slop || point[0] > x1 + slop || point[1] > y1 + slop { return false; }
        let tolerance = (slop * 0.25).max(0.05);
        self.style.entries.iter().any(|entry| match entry {
            PaintEntry::Fill(_) => self.winds(point, tolerance),
            PaintEntry::Stroke(stroke) => stroke.width > 0. && self.touches(point, stroke, slop, tolerance),
            PaintEntry::Opaque(_) => false,
        })
    }
    /// Whether a click at `point` lands inside this path's fill, or inside it
    /// when every subpath is closed.
    pub fn encloses(&self, point: [f64; 2], tolerance: f64) -> bool {
        let [x0, y0, x1, y1] = self.geometry.hull();
        let filled = self.style.entries.iter().any(|entry| matches!(entry, PaintEntry::Fill(_)));
        !self.hidden && (filled || self.geometry.subpaths().iter().all(|s| s.closed && s.segment_count() > 1))
            && point[0] >= x0 && point[0] <= x1 && point[1] >= y0 && point[1] <= y1 && self.winds(point, tolerance)
    }
    fn winds(&self, point: [f64; 2], tolerance: f64) -> bool {
        let mut winding = 0i32;
        let mut cross = |a: [f64; 2], b: [f64; 2]| {
            if (a[1] <= point[1]) == (b[1] <= point[1]) { return; }
            let side = (b[0] - a[0]) * (point[1] - a[1]) - (point[0] - a[0]) * (b[1] - a[1]);
            if (side > 0.) == (b[1] > a[1]) { winding += if b[1] > a[1] { 1 } else { -1 }; }
        };
        for subpath in self.geometry.subpaths() {
            let start = subpath.anchor(0).point();
            let mut last = start;
            for segment in subpath.segments() { segment.flatten(tolerance, |p, _| { cross(last, p); last = p; }); }
            cross(last, start);
        }
        match self.fill_rule { FillRule::NonZero => winding != 0, FillRule::EvenOdd => winding % 2 != 0 }
    }
    pub fn stroke_outline(&self, stroke: &Stroke, tolerance: f64) -> Vec<Vec<[f64; 2]>> {
        outline::outline(self, stroke, tolerance)
    }
    pub fn visual_bounds(&self) -> [f64; 4] {
        let mut bounds: Option<[f64; 4]> = None;
        let mut include = |p: [f64; 2]| bounds = Some(union(bounds, [p[0], p[1], p[0], p[1]]));
        for entry in self.style.entries.iter() {
            match entry {
                PaintEntry::Fill(_) => for subpath in self.geometry.subpaths() {
                    include(subpath.anchor(0).point());
                    for segment in subpath.segments() { segment.flatten(0.01, |p, _| include(p)); }
                },
                PaintEntry::Stroke(stroke) if stroke.width > 0. => {
                    if stroke.cap == Cap::Round && stroke.join == Join::Round {
                        outline::capsules(self, stroke, 0.01, |a, b| {
                            for disk in [a, b] {
                                include([disk.p[0] - disk.r, disk.p[1] - disk.r]);
                                include([disk.p[0] + disk.r, disk.p[1] + disk.r]);
                            }
                            false
                        });
                    } else { for p in self.stroke_outline(stroke, 0.01).into_iter().flatten() { include(p); } }
                },
                _ => {},
            }
        }
        bounds.unwrap_or_else(|| self.geometry.hull())
    }
    fn touches(&self, point: [f64; 2], stroke: &Stroke, slop: f64, tolerance: f64) -> bool {
        if stroke.cap == Cap::Round && stroke.join == Join::Round {
            return outline::capsules(self, stroke, tolerance.min(0.05), |a, b| outline::capsule_distance(point, a, b) <= slop);
        }
        let rings = self.stroke_outline(stroke, tolerance.min(0.05));
        let mut winding = 0i32;
        for ring in rings {
            for (a, b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
                let d = [b[0] - a[0], b[1] - a[1]];
                let length = d[0] * d[0] + d[1] * d[1];
                let t = if length > 0. { (((point[0] - a[0]) * d[0] + (point[1] - a[1]) * d[1]) / length).clamp(0., 1.) } else { 0. };
                if (a[0] + d[0] * t - point[0]).hypot(a[1] + d[1] * t - point[1]) <= slop { return true; }
                if (a[1] <= point[1]) != (b[1] <= point[1]) {
                    let side = d[0] * (point[1] - a[1]) - (point[0] - a[0]) * d[1];
                    if (side > 0.) == (b[1] > a[1]) { winding += if b[1] > a[1] { 1 } else { -1 }; }
                }
            }
        }
        winding != 0
    }
    pub fn transformed(&self, affine: Affine64, scale_widths: bool) -> Result<Self, &'static str> {
        let geometry = Arc::new(self.geometry.transformed(|p| affine.map(p))?);
        let [a, b, c, d, _, _] = affine.0;
        let determinant = a * d - b * c;
        let scale = if scale_widths { determinant.abs().sqrt() } else { 1. };
        let mirror = |stroke: &Stroke| stroke.profile.as_ref().filter(|_| determinant < 0.).and_then(|profile| profile.mirrored()).map(Arc::new);
        let moved = Self { shape: self.shape.as_ref().and_then(|shape| shape.transformed(affine)).map(Arc::new), ..self.with_geometry(geometry) };
        if scale == 1. && self.style.strokes().all(|stroke| mirror(stroke).is_none()) { return Ok(moved); }
        let entries = self.style.entries.iter().map(|entry| match entry {
            PaintEntry::Stroke(stroke) => PaintEntry::Stroke(Stroke { width: stroke.width * scale, profile: mirror(stroke).or_else(|| stroke.profile.clone()), ..stroke.clone() }),
            other => other.clone(),
        }).collect();
        Ok(moved.with_style(Arc::new(Style::new(entries))))
    }
    pub fn with_geometry(&self, geometry: Arc<PathGeometry>) -> Self {
        Self { geometry, shape: None, unknown: retained(&self.unknown, true), ..self.clone() }
    }
    pub fn with_shape(&self, shape: LiveShape) -> Result<Self, &'static str> {
        Ok(Self { shape: Some(Arc::new(shape)), ..self.with_geometry(Arc::new(shape.geometry()?)) })
    }
    pub fn detached(&self, slots: bool) -> Self {
        let keep = |unknown: &Unknown| -> Unknown {
            let kept: Vec<_> = unknown.iter().flat_map(|u| u.iter()).filter(|p| !matches!(p.tag >> 2 & 3, 1 | 3) && (!slots || p.tag >> 2 & 3 != 2)).cloned().collect();
            (!kept.is_empty()).then(|| kept.into())
        };
        let paint = |paint: &Paint| match paint { Paint::Color { color, unknown } => Some(Paint::Color { color: *color, unknown: keep(unknown) }), Paint::Opaque(_) => None };
        let entries = self.style.entries.iter().filter_map(|entry| match entry {
            PaintEntry::Fill(fill) => Some(PaintEntry::Fill(Fill { paint: paint(&fill.paint)?, unknown: keep(&fill.unknown) })),
            PaintEntry::Stroke(stroke) => Some(PaintEntry::Stroke(Stroke { paint: paint(&stroke.paint)?, unknown: keep(&stroke.unknown), ..stroke.clone() })),
            PaintEntry::Opaque(_) => None,
        }).collect();
        Self { unknown: keep(&self.unknown), style: Arc::new(Style::new(entries)), ..self.clone() }
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
pub struct ChunkSummary { pub bounds: Option<[f64; 4]>, pub paths: u32, pub anchors: u64, pub opaque: u32, pub next_item: u64 }
impl ChunkSummary {
    fn of(records: &[Record]) -> Self {
        let mut summary = Self::default();
        let mut highest = 0;
        for record in records {
            match record {
                Record::Path(path) => {
                    summary.paths += 1;
                    summary.anchors += path.geometry.anchor_count() as u64;
                    if !path.hidden { summary.bounds = Some(union(summary.bounds, path.bounds())); }
                    highest = highest.max(path.id.0).max(path.referenced());
                }
                Record::Opaque(opaque) => { summary.opaque += 1; highest = highest.max(opaque.referenced()); }
                Record::End => {}
            }
        }
        summary.next_item = highest.saturating_add(1);
        summary
    }
}
fn varuint(bytes: &[u8], at: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *bytes.get(*at)?;
        *at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 { return Some(value); }
    }
    None
}
fn referenced_in(class: u64, value: &[u8], depth: u32) -> u64 {
    let mut at = 0;
    let mut highest = 0;
    match class {
        1 => while let Some(id) = varuint(value, &mut at) { highest = highest.max(id); },
        3 if depth < 64 => while let Some(kind) = varuint(value, &mut at) {
            if kind == 0 { continue; }
            let (Some(id), Some(length)) = (varuint(value, &mut at), varuint(value, &mut at)) else { break };
            let Some(properties) = value.get(at..at.saturating_add(length as usize)) else { break };
            at += properties.len();
            highest = highest.max(id).max(properties_referenced(properties, depth + 1));
        },
        _ => {}
    }
    highest
}
fn properties_referenced(properties: &[u8], depth: u32) -> u64 {
    let mut at = 0;
    let mut highest = 0;
    while let (Some(tag), Some(length)) = (varuint(properties, &mut at), varuint(properties, &mut at)) {
        let Some(value) = properties.get(at..at.saturating_add(length as usize)) else { break };
        at += value.len();
        highest = highest.max(referenced_in(tag >> 2 & 3, value, depth));
    }
    highest
}
fn unknown_referenced(unknown: &Unknown) -> u64 { unknown.iter().flat_map(|u| u.iter()).map(|p| referenced_in(p.tag >> 2 & 3, &p.value, 0)).max().unwrap_or(0) }
impl OpaqueRecord { fn referenced(&self) -> u64 { self.id.max(properties_referenced(&self.properties, 0)) } }
impl PathItem {
    fn referenced(&self) -> u64 {
        let paint = |paint: &Paint| match paint { Paint::Color { unknown, .. } => unknown_referenced(unknown), Paint::Opaque(record) => record.referenced() };
        self.style.entries.iter().map(|entry| match entry {
            PaintEntry::Fill(fill) => paint(&fill.paint).max(unknown_referenced(&fill.unknown)),
            PaintEntry::Stroke(stroke) => paint(&stroke.paint).max(unknown_referenced(&stroke.unknown)),
            PaintEntry::Opaque(record) => record.referenced(),
        }).fold(unknown_referenced(&self.unknown), u64::max)
    }
}
pub fn union(a: Option<[f64; 4]>, b: [f64; 4]) -> [f64; 4] {
    a.map_or(b, |a| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
}

const DUPLICATE: &str = "Duplicate item identity";
const UNBALANCED: &str = "Unbalanced end marker";
const CONTAINED: &str = "Contents of unknown containers stay uninterpreted";
const REPLACED: &str = "A replaced path keeps its identity and place";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Nesting { net: i64, low: i64, deepest_path: Option<i64> }
impl Nesting {
    fn of(records: &[Record]) -> Self {
        let mut nesting = Self::default();
        for record in records {
            match record {
                Record::Path(_) => nesting.deepest_path = nesting.deepest_path.max(Some(nesting.net)),
                Record::Opaque(opaque) => if opaque.kind & PARENT != 0 { nesting.net += 1; },
                Record::End => { nesting.net -= 1; nesting.low = nesting.low.min(nesting.net); }
            }
        }
        nesting
    }
}

#[derive(Clone, Debug)]
pub struct VectorChunk {
    records: Box<[Record]>,
    summary: ChunkSummary,
    ids: Box<[(ItemId, u32)]>,
    nesting: Nesting,
    local: Result<(), &'static str>,
    encoded_len: OnceLock<usize>,
    footprint: OnceLock<Footprint>,
}
#[derive(Clone, Debug)]
pub(crate) struct Footprint { pub bytes: usize, pub parts: Box<[(usize, usize)]> }
impl PartialEq for VectorChunk { fn eq(&self, other: &Self) -> bool { self.records == other.records } }
impl VectorChunk {
    pub fn new(records: Vec<Record>) -> Self {
        let summary = ChunkSummary::of(&records);
        let mut ids: Vec<(ItemId, u32)> = records.iter().enumerate().filter_map(|(index, record)| Some((record.id()?, index as u32))).collect();
        ids.sort_unstable();
        let local = records.iter().filter_map(Record::path).try_for_each(PathItem::validate)
            .and_then(|()| if ids.windows(2).any(|pair| pair[0].0 == pair[1].0) { Err(DUPLICATE) } else { Ok(()) });
        Self { nesting: Nesting::of(&records), records: records.into(), summary, ids: ids.into(), local, encoded_len: OnceLock::new(), footprint: OnceLock::new() }
    }
    fn with_paths(&self, paths: Vec<(usize, PathItem)>) -> Result<Self, &'static str> {
        let mut records = self.records.to_vec();
        let mut local = self.local;
        for (index, path) in paths {
            if records.get(index).and_then(Record::path).is_none_or(|old| old.id != path.id) { return Err(REPLACED); }
            local = local.and_then(|()| path.validate());
            records[index] = Record::Path(path);
        }
        if self.local.is_err() { return Ok(Self::new(records)); }
        Ok(Self { summary: ChunkSummary::of(&records), records: records.into(), ids: self.ids.clone(), nesting: self.nesting, local, encoded_len: OnceLock::new(), footprint: OnceLock::new() })
    }
    pub(crate) fn footprint(&self) -> &Footprint {
        self.footprint.get_or_init(|| {
            let mut bytes = 128 + std::mem::size_of_val(self.records());
            let mut parts = Vec::new();
            for record in self.records() {
                match record {
                    Record::Path(path) => {
                        parts.push((Arc::as_ptr(&path.geometry) as usize, 64 + std::mem::size_of_val(path.geometry.subpaths())));
                        parts.extend(path.geometry.subpaths().iter().flat_map(|s| s.runs()).map(|run| (run.as_ptr() as usize, run.len() * std::mem::size_of::<Anchor>())));
                        parts.push((Arc::as_ptr(&path.style) as usize, path.style.entries.iter().map(|e| std::mem::size_of::<PaintEntry>() + match e {
                            PaintEntry::Stroke(s) => s.profile.as_ref().map_or(0, |p| p.knots().len() * 32),
                            PaintEntry::Opaque(o) => o.properties.len(),
                            PaintEntry::Fill(_) => 0,
                        }).sum::<usize>()));
                    }
                    Record::Opaque(opaque) => bytes = bytes.saturating_add(opaque.properties.len()),
                    Record::End => {}
                }
            }
            Footprint { bytes, parts: parts.into() }
        })
    }
    pub fn encoded_len(&self) -> Option<usize> { self.encoded_len.get().copied() }
    pub fn note_encoded_len(&self, length: usize) { let _ = self.encoded_len.set(length); }
    pub fn records(&self) -> &[Record] { &self.records }
    pub fn summary(&self) -> &ChunkSummary { &self.summary }
    fn index_of(&self, id: ItemId) -> Option<usize> {
        let at = self.ids.partition_point(|(other, _)| *other < id);
        self.ids.get(at).filter(|(other, _)| *other == id).map(|(_, index)| *index as usize)
    }
    pub fn contains(&self, id: ItemId) -> bool { self.index_of(id).is_some() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position { pub chunk: u32, pub index: u32 }

#[derive(Clone, Debug, PartialEq)]
pub enum Slot { Image(super::Image), Resource(Arc<super::OpaqueResource>) }

#[derive(Debug)]
struct Root { chunks: Box<[Resource<VectorChunk>]>, slots: Arc<[Slot]>, counter: u64, valid: OnceLock<Result<(), &'static str>>, content: crate::raster::ContentName }
impl Default for Root {
    fn default() -> Self { Self { chunks: Box::default(), slots: Arc::default(), counter: 0, valid: OnceLock::new(), content: crate::raster::ContentName::fresh() } }
}

#[derive(Clone, Debug, Default)]
pub struct VectorObject { root: Arc<Root> }
#[derive(Clone, Debug)]
pub(crate) struct WeakVectorObject(std::sync::Weak<Root>);
impl WeakVectorObject {
    pub(crate) fn identity(&self) -> Option<usize> { (self.0.strong_count() > 0).then_some(self.0.as_ptr() as usize) }
}
impl PartialEq for VectorObject {
    fn eq(&self, other: &Self) -> bool {
        self.same_root(other) || (self.chunks().len() == other.chunks().len() && self.slots() == other.slots() && self.next_item() == other.next_item()
            && self.chunks().iter().zip(other.chunks()).all(|(a, b)| a.same_owner(b) || **a == **b))
    }
}
impl VectorObject {
    pub fn content_version(&self) -> crate::raster::ContentVersion { self.root.content.into() }
    pub fn from_chunks(chunks: Vec<Resource<VectorChunk>>) -> Self { Self::with_slots(chunks, Arc::default()) }
    pub fn with_slots(chunks: Vec<Resource<VectorChunk>>, slots: Arc<[Slot]>) -> Self { Self { root: Arc::new(Root { chunks: chunks.into(), slots, counter: 0, valid: OnceLock::new(), content: crate::raster::ContentName::fresh() }) } }
    fn rechunked(&self, chunks: Vec<Resource<VectorChunk>>) -> Self { Self { root: Arc::new(Root { chunks: chunks.into(), slots: self.root.slots.clone(), counter: self.next_item(), valid: OnceLock::new(), content: crate::raster::ContentName::fresh() }) } }
    pub fn with_next_item(&self, next_item: u64) -> Self { Self { root: Arc::new(Root { chunks: self.root.chunks.clone(), slots: self.root.slots.clone(), counter: next_item, valid: OnceLock::new(), content: self.root.content }) } }
    pub fn next_item(&self) -> u64 { self.chunks().iter().map(|chunk| chunk.summary().next_item).fold(self.root.counter.max(1), u64::max) }
    pub fn slots(&self) -> &Arc<[Slot]> { &self.root.slots }
    pub fn shares_slots(&self, other: &Self) -> bool { self.slots().is_empty() || Arc::ptr_eq(self.slots(), other.slots()) }
    pub fn from_records(records: Vec<Record>) -> Self { Self::from_chunks(pack(records)) }
    pub fn chunks(&self) -> &[Resource<VectorChunk>] { &self.root.chunks }
    pub fn same_root(&self, other: &Self) -> bool { Arc::ptr_eq(&self.root, &other.root) }
    pub(crate) fn identity(&self) -> usize { Arc::as_ptr(&self.root) as usize }
    pub(crate) fn downgrade(&self) -> WeakVectorObject { WeakVectorObject(Arc::downgrade(&self.root)) }
    pub fn records(&self) -> impl DoubleEndedIterator<Item = (Position, &Record)> {
        self.chunks().iter().enumerate().flat_map(|(c, chunk)| chunk.records().iter().enumerate()
            .map(move |(i, record)| (Position { chunk: c as u32, index: i as u32 }, record)))
    }
    pub fn paths(&self) -> impl DoubleEndedIterator<Item = (Position, &PathItem)> {
        self.records().filter_map(|(p, r)| r.path().map(|path| (p, path)))
    }
    pub fn record(&self, position: Position) -> Option<&Record> {
        self.chunks().get(position.chunk as usize)?.records().get(position.index as usize)
    }
    pub fn record_count(&self) -> usize { self.chunks().iter().map(|c| c.records().len()).sum() }
    pub fn path_count(&self) -> usize { self.chunks().iter().map(|c| c.summary().paths as usize).sum() }
    pub fn anchor_count(&self) -> u64 { self.chunks().iter().map(|c| c.summary().anchors).sum() }
    pub fn bounds(&self) -> Option<[f64; 4]> {
        self.chunks().iter().filter_map(|c| c.summary().bounds).fold(None, |a, b| Some(union(a, b)))
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        *self.root.valid.get_or_init(|| {
            self.validate_nesting()?;
            let mut ids = HashSet::with_capacity(self.record_count());
            for chunk in self.chunks() {
                chunk.local?;
                if !chunk.ids.iter().all(|(id, _)| ids.insert(*id)) { return Err(DUPLICATE); }
            }
            Ok(())
        })
    }
    pub fn validate_changed(&self, previous: &Self) -> Result<(), &'static str> {
        if previous.root.valid.get() != Some(&Ok(())) { return self.validate(); }
        *self.root.valid.get_or_init(|| self.validate_edit_of(previous))
    }
    fn validate_nesting(&self) -> Result<(), &'static str> {
        let mut open = 0i64;
        for chunk in self.chunks() {
            let nesting = chunk.nesting;
            if open + nesting.low < 0 { return Err(UNBALANCED); }
            if nesting.deepest_path.is_some_and(|depth| open + depth > 0) { return Err(CONTAINED); }
            open += nesting.net;
        }
        if open == 0 { Ok(()) } else { Err(UNBALANCED) }
    }
    fn validate_edit_of(&self, previous: &Self) -> Result<(), &'static str> {
        self.validate_nesting()?;
        let key = |chunk: &Resource<VectorChunk>| Arc::as_ptr(chunk.storage()) as usize;
        let before: HashSet<usize> = previous.chunks().iter().map(key).collect();
        let mut present = HashSet::with_capacity(self.chunks().len());
        let (mut kept, mut added) = (Vec::new(), Vec::new());
        for chunk in self.chunks() {
            if present.insert(key(chunk)) && before.contains(&key(chunk)) { kept.push(chunk); } else { added.push(chunk); }
        }
        let mut removed: Vec<ItemId> = previous.chunks().iter().filter(|chunk| !present.contains(&key(chunk)))
            .flat_map(|chunk| chunk.ids.iter().map(|(id, _)| *id)).collect();
        removed.sort_unstable();
        let mut fresh = Vec::new();
        for chunk in added {
            chunk.local?;
            fresh.extend(chunk.ids.iter().map(|(id, _)| *id));
        }
        fresh.sort_unstable();
        if fresh.windows(2).any(|pair| pair[0] == pair[1]) { return Err(DUPLICATE); }
        let unseen: Vec<ItemId> = fresh.into_iter().filter(|id| removed.binary_search(id).is_err()).collect();
        let kept_ids: usize = kept.iter().map(|chunk| chunk.ids.len()).sum();
        let collides = if unseen.len().saturating_mul(kept.len()) <= kept_ids {
            unseen.iter().any(|&id| kept.iter().any(|chunk| chunk.contains(id)))
        } else {
            kept.iter().any(|chunk| chunk.ids.iter().any(|(id, _)| unseen.binary_search(id).is_ok()))
        };
        if collides { Err(DUPLICATE) } else { Ok(()) }
    }
    pub fn contains(&self, id: ItemId) -> bool { self.chunks().iter().any(|chunk| chunk.contains(id)) }
    pub fn find(&self, id: ItemId) -> Option<Position> {
        self.chunks().iter().enumerate().find_map(|(chunk, records)| Some(Position { chunk: chunk as u32, index: records.index_of(id)? as u32 }))
    }
    fn located(&self, ids: &BTreeSet<ItemId>) -> Vec<Position> {
        if ids.len().saturating_mul(self.chunks().len()) <= self.record_count() {
            ids.iter().filter_map(|&id| self.find(id)).collect()
        } else {
            let ids: IdSet = ids.iter().copied().collect();
            self.chunks().iter().enumerate().flat_map(|(chunk, records)| records.ids.iter().filter(|(id, _)| ids.contains(id))
                .map(move |&(_, index)| Position { chunk: chunk as u32, index })).collect()
        }
    }
    pub fn positions(&self, ids: &BTreeSet<ItemId>) -> BTreeMap<usize, Vec<usize>> {
        let mut found: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for position in self.located(ids) { found.entry(position.chunk as usize).or_default().push(position.index as usize); }
        found
    }
    fn ordered(&self, ids: &BTreeSet<ItemId>) -> Vec<Position> {
        let mut positions = self.located(ids);
        positions.sort_unstable();
        positions
    }
    pub fn selected_paths(&self, ids: &BTreeSet<ItemId>) -> Vec<(Position, &PathItem)> {
        self.ordered(ids).into_iter().filter_map(|position| Some((position, self.record(position)?.path()?))).collect()
    }
    pub fn transformed(&self, affine: Affine64) -> Result<Self, &'static str> {
        let mut edits = BTreeMap::new();
        for (chunk, records) in self.chunks().iter().enumerate().filter(|(_, chunk)| chunk.summary().paths > 0) {
            let records = records.records().iter().map(|record| match record {
                Record::Path(path) => path.transformed(affine, true).map(Record::Path),
                other => Ok(other.clone()),
            }).collect::<Result<Vec<_>, _>>()?;
            edits.insert(chunk, records);
        }
        Ok(self.edit_chunks(edits))
    }
    pub fn transform_items(&self, ids: &BTreeSet<ItemId>, affine: Affine64, scale_widths: bool) -> Result<Self, &'static str> {
        self.transform_paths(&self.selected_paths(ids).into_iter().map(|(position, _)| position).collect::<Vec<_>>(), affine, scale_widths)
    }
    pub fn transform_paths(&self, positions: &[Position], affine: Affine64, scale_widths: bool) -> Result<Self, &'static str> {
        let paths = positions.iter().filter_map(|&position| Some((position, self.record(position)?.path()?)))
            .map(|(position, path)| path.transformed(affine, scale_widths).map(|path| (position, path))).collect::<Result<Vec<_>, _>>()?;
        self.with_paths(paths)
    }
    pub fn with_paths(&self, paths: impl IntoIterator<Item = (Position, PathItem)>) -> Result<Self, &'static str> {
        let mut grouped: BTreeMap<usize, Vec<(usize, PathItem)>> = BTreeMap::new();
        for (position, path) in paths { grouped.entry(position.chunk as usize).or_default().push((position.index as usize, path)); }
        if grouped.keys().next_back().is_some_and(|last| *last >= self.chunks().len()) { return Err(REPLACED); }
        let mut valid = self.root.valid.get().copied().filter(Result::is_ok);
        let mut chunks: Vec<Resource<VectorChunk>> = Vec::with_capacity(self.chunks().len());
        for (index, chunk) in self.chunks().iter().enumerate() {
            let Some(paths) = grouped.remove(&index) else { chunks.push(chunk.clone()); continue };
            let edited = chunk.with_paths(paths)?;
            if edited.local.is_err() { valid = None; }
            match chunks.last_mut() {
                Some(last) if joins(last, edited.records().len()) => *last = Resource::new(Arc::new(VectorChunk::new(last.records().iter().chain(edited.records()).cloned().collect()))),
                _ => chunks.push(Resource::new(Arc::new(edited))),
            }
        }
        let object = self.rechunked(chunks);
        if let Some(valid) = valid { let _ = object.root.valid.set(valid); }
        Ok(object)
    }
    pub fn selected(&self, ids: &BTreeSet<ItemId>) -> Vec<PathItem> {
        self.selected_paths(ids).into_iter().map(|(_, path)| path.clone()).collect()
    }
    pub fn copies(&self, ids: &BTreeSet<ItemId>, target: &VectorObject) -> (BTreeSet<ItemId>, Vec<Record>) {
        let mut fresh = BTreeSet::new();
        let records = self.selected(ids).into_iter().zip(target.allocate_ids()).map(|(path, id)| {
            fresh.insert(id);
            let path = if self.same_root(target) { path } else { path.detached(!self.shares_slots(target)) };
            Record::Path(PathItem { id, unknown: retained(&path.unknown, true), ..path })
        }).collect();
        (fresh, records)
    }
    pub fn present(&self, ids: &BTreeSet<ItemId>) -> BTreeSet<ItemId> {
        self.located(ids).into_iter().filter_map(|position| self.record(position)?.id()).collect()
    }
    pub fn items_bounds(&self, ids: &BTreeSet<ItemId>) -> Option<[f64; 4]> {
        self.located(ids).into_iter().filter_map(|position| self.record(position)?.path().map(PathItem::bounds)).fold(None, |a, b| Some(union(a, b)))
    }
    pub fn allocate_ids(&self) -> impl Iterator<Item = ItemId> + use<> { (self.next_item()..=u64::MAX).map(ItemId) }
    pub fn allocate_id(&self) -> ItemId { ItemId(self.next_item()) }
    pub fn insert_front(&self, records: Vec<Record>) -> Self { self.insert(records, true) }
    pub fn insert_back(&self, records: Vec<Record>) -> Self { self.insert(records, false) }
    fn insert(&self, records: Vec<Record>, front: bool) -> Self {
        if records.is_empty() { return self.clone(); }
        if self.chunks().is_empty() { return self.rechunked(pack(records)); }
        let at = if front { 0 } else { self.chunks().len() - 1 };
        let edge = self.chunks()[at].records();
        if edge.len() + records.len() <= CHUNK_RECORDS {
            let joined = if front { records.into_iter().chain(edge.iter().cloned()).collect() } else { edge.iter().cloned().chain(records).collect() };
            return self.edit_chunks(BTreeMap::from([(at, joined)]));
        }
        let mut chunks = self.chunks().to_vec();
        if front { chunks.splice(0..0, pack(records)); } else { chunks.extend(pack(records)); }
        self.rechunked(chunks)
    }
    fn step(&self, position: Position, forward: bool) -> Option<Position> {
        let (chunk, index) = (position.chunk as usize, position.index as usize);
        if forward {
            if index > 0 { return Some(Position { chunk: position.chunk, index: position.index - 1 }); }
            (0..chunk).rev().find(|c| !self.chunks()[*c].records().is_empty()).map(|c| Position { chunk: c as u32, index: self.chunks()[c].records().len() as u32 - 1 })
        } else {
            if index + 1 < self.chunks()[chunk].records().len() { return Some(Position { chunk: position.chunk, index: position.index + 1 }); }
            (chunk + 1..self.chunks().len()).find(|c| !self.chunks()[*c].records().is_empty()).map(|c| Position { chunk: c as u32, index: 0 })
        }
    }
    pub fn arrange(&self, ids: &BTreeSet<ItemId>, arrange: Arrange) -> Self {
        let selected = self.ordered(ids);
        let record = |p: Position| self.record(p).cloned().unwrap();
        let forward = matches!(arrange, Arrange::Front | Arrange::Forward);
        if matches!(arrange, Arrange::Front | Arrange::Back) {
            let edge = if forward { self.records().next() } else { self.records().next_back() }.map(|(p, _)| p);
            let mut packed = edge;
            let ordered: Vec<Position> = if forward { selected.clone() } else { selected.iter().rev().copied().collect() };
            if ordered.iter().all(|p| { let at = packed == Some(*p); packed = packed.and_then(|q| self.step(q, !forward)); at }) { return self.clone(); }
            let moved = selected.iter().map(|p| record(*p)).collect();
            let removed = self.replace(selected.iter().map(|p| (*p, None)));
            return if forward { removed.insert_front(moved) } else { removed.insert_back(moved) };
        }
        let ordered: Vec<Position> = if forward { selected } else { selected.into_iter().rev().collect() };
        let mut changes = BTreeMap::new();
        let mut run: Vec<Position> = Vec::new();
        for (i, &p) in ordered.iter().enumerate() {
            run.push(p);
            if ordered.get(i + 1).is_some_and(|next| self.step(p, !forward) == Some(*next)) { continue; }
            let block = std::mem::take(&mut run);
            let Some(jumped) = self.step(block[0], forward).filter(|q| self.record(*q).is_some_and(|r| r.path().is_some())) else { continue };
            let slots: Vec<Position> = std::iter::once(jumped).chain(block.iter().copied()).collect();
            let records = block.iter().map(|q| record(*q)).chain([record(jumped)]);
            changes.extend(slots.into_iter().zip(records));
        }
        if changes.is_empty() { return self.clone(); }
        self.replace(changes.into_iter().map(|(p, r)| (p, Some(r))))
    }
    pub fn replace(&self, changes: impl IntoIterator<Item = (Position, Option<Record>)>) -> Self {
        self.splice(changes.into_iter().map(|(position, record)| (position, record.into_iter().collect())).collect())
    }
    pub fn splice(&self, changes: BTreeMap<Position, Vec<Record>>) -> Self {
        let mut grouped: BTreeMap<u32, Vec<(u32, Vec<Record>)>> = BTreeMap::new();
        for (position, records) in changes { grouped.entry(position.chunk).or_default().push((position.index, records)); }
        let edits = grouped.into_iter().map(|(chunk, changes)| {
            let mut records: Vec<Vec<Record>> = self.chunks()[chunk as usize].records().iter().cloned().map(|record| vec![record]).collect();
            for (index, replacement) in changes { records[index as usize] = replacement; }
            (chunk as usize, records.into_iter().flatten().collect())
        }).collect();
        self.edit_chunks(edits)
    }
    pub fn edit_chunks(&self, mut edits: BTreeMap<usize, Vec<Record>>) -> Self {
        let mut chunks: Vec<Resource<VectorChunk>> = Vec::with_capacity(self.chunks().len() + edits.len());
        for (index, chunk) in self.chunks().iter().enumerate() {
            let Some(records) = edits.remove(&index) else { chunks.push(chunk.clone()); continue };
            for packed in pack(records) {
                if let Some(last) = chunks.last()
                    && joins(last, packed.records().len()) {
                    let mut joined = last.records().to_vec();
                    joined.extend(packed.records().iter().cloned());
                    *chunks.last_mut().unwrap() = Resource::new(Arc::new(VectorChunk::new(joined)));
                } else {
                    chunks.push(packed);
                }
            }
        }
        self.rechunked(chunks)
    }
}
fn joins(last: &VectorChunk, records: usize) -> bool { records < CHUNK_RECORDS / 4 && last.records().len() + records <= CHUNK_RECORDS / 2 }
fn pack(records: Vec<Record>) -> Vec<Resource<VectorChunk>> {
    if records.is_empty() { return Vec::new(); }
    let pieces = records.len().div_ceil(CHUNK_RECORDS);
    let per = records.len().div_ceil(pieces);
    let mut records = records.into_iter();
    (0..pieces).map(|_| Resource::new(Arc::new(VectorChunk::new(records.by_ref().take(per).collect())))).collect()
}
