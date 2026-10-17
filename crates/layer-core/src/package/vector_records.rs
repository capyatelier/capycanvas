use super::values::{DecodeError, DecodeResult};
use crate::authored::vector::*;
use crate::color::{RgbColor, RgbSpace};
use std::sync::Arc;

pub const KIND_PARENT: u64 = PARENT;
const KIND_RESERVED: u64 = 8;
const KIND_FLAGS: u32 = 4;
const TAG_FLAGS: u32 = 5;
const TAG_RESERVED: u64 = 16;
const CLASS_SHIFT: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueClass { Data, Items, Slots, Nested }
impl ValueClass {
    fn of(tag: u64) -> Self { [Self::Data, Self::Items, Self::Slots, Self::Nested][(tag >> CLASS_SHIFT & 3) as usize] }
    const fn bits(self) -> u64 { (self as u64) << CLASS_SHIFT }
}
pub const fn kind_word(number: u64) -> u64 { number << KIND_FLAGS }
pub const fn tag_word(number: u64, flags: u64, class: ValueClass) -> u64 { number << TAG_FLAGS | flags | class.bits() }

pub const PATH: u64 = kind_word(1);
pub const FILL: u64 = kind_word(2);
pub const STROKE: u64 = kind_word(3);
pub const COLOR: u64 = kind_word(4);

pub const GEOMETRY: u64 = tag_word(1, 0, ValueClass::Data);
pub const FILL_RULE: u64 = tag_word(2, 0, ValueClass::Data);
pub const STYLE: u64 = tag_word(3, 0, ValueClass::Nested);
pub const HIDDEN: u64 = tag_word(4, 0, ValueClass::Data);
pub const NAME: u64 = tag_word(5, IGNORABLE | COPY_SAFE, ValueClass::Data);
pub const LOCKED: u64 = tag_word(6, IGNORABLE | COPY_SAFE, ValueClass::Data);
pub const PAINT: u64 = tag_word(7, 0, ValueClass::Nested);
pub const WIDTH: u64 = tag_word(8, 0, ValueClass::Data);
pub const CAP: u64 = tag_word(9, 0, ValueClass::Data);
pub const JOIN: u64 = tag_word(10, 0, ValueClass::Data);
pub const MITER_LIMIT: u64 = tag_word(11, 0, ValueClass::Data);
pub const PROFILE: u64 = tag_word(12, 0, ValueClass::Data);
pub const COLOR_VALUE: u64 = tag_word(13, 0, ValueClass::Data);

pub struct Number { pub word: u64, pub name: &'static str, pub on: &'static [u64] }
pub const KINDS: &[Number] = &[
    Number { word: PATH, name: "path", on: &[] },
    Number { word: FILL, name: "fill", on: &[STYLE] },
    Number { word: STROKE, name: "stroke", on: &[STYLE] },
    Number { word: COLOR, name: "color", on: &[PAINT] },
];
pub const TAGS: &[Number] = &[
    Number { word: GEOMETRY, name: "geometry", on: &[PATH] },
    Number { word: FILL_RULE, name: "fill_rule", on: &[PATH] },
    Number { word: STYLE, name: "style", on: &[PATH] },
    Number { word: HIDDEN, name: "hidden", on: &[PATH] },
    Number { word: NAME, name: "name", on: &[PATH] },
    Number { word: LOCKED, name: "locked", on: &[PATH] },
    Number { word: PAINT, name: "paint", on: &[FILL, STROKE] },
    Number { word: WIDTH, name: "width", on: &[STROKE] },
    Number { word: CAP, name: "cap", on: &[STROKE] },
    Number { word: JOIN, name: "join", on: &[STROKE] },
    Number { word: MITER_LIMIT, name: "miter_limit", on: &[STROKE] },
    Number { word: PROFILE, name: "width_profile", on: &[STROKE] },
    Number { word: COLOR_VALUE, name: "color", on: &[COLOR] },
];

pub const MAX_RECORDS: usize = 1 << 20;
pub const MAX_PATH_ANCHORS: usize = 1 << 20;
pub const MAX_PROPERTY_BYTES: usize = 16 << 20;
pub const MAX_CHUNK_BYTES: usize = 64 << 20;
pub const MAX_DEPTH: u32 = 64;
const ANCHOR_BYTES: usize = 49;
const KNOT_BYTES: usize = 28;

fn invalid(message: &str) -> DecodeError { DecodeError::Invalid(format!("Invalid vector data: {message}")) }
fn unsupported(message: &str) -> DecodeError { DecodeError::Unsupported(format!("Unsupported vector data: {message}")) }

pub fn put_varuint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 { out.push(value as u8 | 0x80); value >>= 7; }
    out.push(value as u8);
}
struct Bytes<'a> { data: &'a [u8], at: usize }
impl<'a> Bytes<'a> {
    fn new(data: &'a [u8]) -> Self { Self { data, at: 0 } }
    fn done(&self) -> bool { self.at == self.data.len() }
    fn take(&mut self, count: usize) -> DecodeResult<&'a [u8]> {
        let end = self.at.checked_add(count).filter(|end| *end <= self.data.len()).ok_or_else(|| invalid("truncated value"))?;
        let bytes = &self.data[self.at..end];
        self.at = end;
        Ok(bytes)
    }
    fn u8(&mut self) -> DecodeResult<u8> { Ok(self.take(1)?[0]) }
    fn u32(&mut self) -> DecodeResult<u32> { Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
    fn u64(&mut self) -> DecodeResult<u64> { Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap())) }
    fn f32(&mut self) -> DecodeResult<f32> { Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
    fn f64(&mut self) -> DecodeResult<f64> {
        let value = f64::from_le_bytes(self.take(8)?.try_into().unwrap());
        if value.is_finite() { Ok(value) } else { Err(invalid("nonfinite number")) }
    }
    fn varuint(&mut self) -> DecodeResult<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.u8()?;
            if shift == 63 && byte > 1 { return Err(invalid("varuint overflow")); }
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                if byte == 0 && shift > 0 { return Err(invalid("noncanonical varuint")); }
                return Ok(value);
            }
        }
        Err(invalid("varuint overflow"))
    }
    fn length(&mut self, maximum: usize) -> DecodeResult<usize> {
        let length = self.varuint()?;
        if length > (self.data.len() - self.at) as u64 { return Err(invalid("length exceeds its container")); }
        if length > maximum as u64 { return Err(unsupported("value exceeds the admitted size")); }
        Ok(length as usize)
    }
}

struct Framed<'a> { kind: u64, id: u64, properties: &'a [u8] }
fn framed<'a>(bytes: &mut Bytes<'a>) -> DecodeResult<Option<Framed<'a>>> {
    let kind = bytes.varuint()?;
    if kind == 0 { return Ok(None); }
    let id = bytes.u64()?;
    let length = bytes.length(MAX_PROPERTY_BYTES * 4)?;
    Ok(Some(Framed { kind, id, properties: bytes.take(length)? }))
}
fn properties<'a>(data: &'a [u8]) -> DecodeResult<Vec<(u64, &'a [u8])>> {
    let mut bytes = Bytes::new(data);
    let mut list: Vec<(u64, &[u8])> = Vec::new();
    while !bytes.done() {
        let tag = bytes.varuint()?;
        let length = bytes.length(MAX_PROPERTY_BYTES)?;
        let value = bytes.take(length)?;
        if let Some((previous, _)) = list.last() && previous >> TAG_FLAGS >= tag >> TAG_FLAGS { return Err(invalid("properties out of order")); }
        list.push((tag, value));
    }
    Ok(list)
}
fn validate_frame(data: &[u8], depth: u32) -> DecodeResult<()> {
    if depth > MAX_DEPTH { return Err(unsupported("nesting exceeds the admitted depth")); }
    for (tag, value) in properties(data)? {
        match ValueClass::of(tag) {
            ValueClass::Nested => validate_stream(value, depth + 1)?,
            ValueClass::Items if value.len() % 8 != 0 => return Err(invalid("item list length")),
            ValueClass::Slots if !value.is_empty() => return Err(invalid("slot index out of range")),
            _ => {}
        }
    }
    Ok(())
}
fn validate_stream(data: &[u8], depth: u32) -> DecodeResult<()> {
    let mut bytes = Bytes::new(data);
    let mut open = 0u32;
    while !bytes.done() {
        match framed(&mut bytes)? {
            None => open = open.checked_sub(1).ok_or_else(|| invalid("unbalanced end marker"))?,
            Some(record) => {
                validate_frame(record.properties, depth)?;
                if record.kind & KIND_PARENT != 0 { open += 1; if open > MAX_DEPTH { return Err(unsupported("nesting exceeds the admitted depth")); } }
            }
        }
    }
    if open == 0 { Ok(()) } else { Err(invalid("unbalanced end marker")) }
}
fn opaque(record: &Framed) -> OpaqueRecord { OpaqueRecord { kind: record.kind, id: record.id, properties: record.properties.into() } }
fn unknown_kind(record: &Framed, depth: u32) -> DecodeResult<OpaqueRecord> {
    if record.kind & KIND_RESERVED != 0 || record.kind & IGNORABLE == 0 { return Err(unsupported("required record kind")); }
    validate_frame(record.properties, depth)?;
    Ok(opaque(record))
}
fn same_number(a: u64, b: u64, shift: u32) -> bool { a >> shift == b >> shift }
fn check_kind(word: u64) -> DecodeResult<bool> {
    match KINDS.iter().find(|n| same_number(n.word, word, KIND_FLAGS)) {
        Some(n) if n.word == word => Ok(true),
        Some(_) => Err(invalid("record kind with altered flags")),
        None => Ok(false),
    }
}
fn unknown_property(tag: u64, value: &[u8], owner: u64, depth: u32, unknown: &mut Vec<UnknownProperty>) -> DecodeResult<()> {
    if let Some(n) = TAGS.iter().find(|n| same_number(n.word, tag, TAG_FLAGS)) {
        if n.word != tag { return Err(invalid("property with altered flags")); }
        if !n.on.contains(&owner) { return Err(unsupported("property on another record kind")); }
    }
    if tag & TAG_RESERVED != 0 || tag & IGNORABLE == 0 { return Err(unsupported("required property")); }
    if ValueClass::of(tag) == ValueClass::Nested { validate_stream(value, depth + 1)?; }
    if ValueClass::of(tag) == ValueClass::Items && value.len() % 8 != 0 { return Err(invalid("item list length")); }
    if ValueClass::of(tag) == ValueClass::Slots && !value.is_empty() { return Err(invalid("slot index out of range")); }
    unknown.push(UnknownProperty { tag, value: value.into() });
    Ok(())
}
fn finish(unknown: Vec<UnknownProperty>) -> Unknown { (!unknown.is_empty()).then(|| unknown.into()) }
fn flag(value: &[u8]) -> Option<bool> { match value { [0] => Some(false), [1] => Some(true), _ => None } }
fn exact<T>(bytes: Bytes, value: T) -> DecodeResult<T> { if bytes.done() { Ok(value) } else { Err(invalid("value has trailing bytes")) } }

fn decode_geometry(value: &[u8]) -> DecodeResult<PathGeometry> {
    let mut bytes = Bytes::new(value);
    let count = bytes.varuint()?;
    if count == 0 || count > (value.len() / 2) as u64 { return Err(invalid("subpath count")); }
    let mut subpaths = Vec::with_capacity(count as usize);
    let mut total = 0usize;
    for _ in 0..count {
        let anchors = bytes.varuint()?;
        let flags = bytes.u8()?;
        if anchors == 0 { return Err(invalid("empty subpath")); }
        if anchors > ((value.len() - bytes.at) / ANCHOR_BYTES) as u64 { return Err(invalid("anchor count exceeds the value")); }
        total += anchors as usize;
        if total > MAX_PATH_ANCHORS { return Err(unsupported("path exceeds the admitted anchor count")); }
        if flags > 1 { return Err(unsupported("subpath flags")); }
        let mut list = Vec::with_capacity(anchors as usize);
        for _ in 0..anchors {
            let kind = AnchorKind(bytes.u8()?);
            let [px, py, ix, iy, ox, oy] = [bytes.f64()?, bytes.f64()?, bytes.f64()?, bytes.f64()?, bytes.f64()?, bytes.f64()?];
            list.push(Anchor { point: [px, py], in_handle: [ix, iy], out_handle: [ox, oy], kind });
        }
        subpaths.push(Subpath::new(&list, flags == 1));
    }
    exact(bytes, ())?;
    PathGeometry::new(subpaths).map_err(|e| invalid(e))
}
fn decode_profile(value: &[u8]) -> DecodeResult<WidthProfile> {
    let mut bytes = Bytes::new(value);
    let count = bytes.varuint()?;
    if count == 0 || count > (value.len() / KNOT_BYTES) as u64 { return Err(invalid("width knot count")); }
    let knots = (0..count).map(|_| Ok(WidthKnot { segment: bytes.u32()?, t: bytes.f64()?, left: bytes.f64()?, right: bytes.f64()? }))
        .collect::<DecodeResult<Vec<_>>>()?;
    exact(bytes, ())?;
    WidthProfile::new(knots).map_err(|e| invalid(e))
}
fn decode_color(value: &[u8]) -> DecodeResult<RgbColor> {
    let mut bytes = Bytes::new(value);
    let (space, form) = (bytes.u8()?, bytes.u8()?);
    let rgba = [bytes.f32()?, bytes.f32()?, bytes.f32()?, bytes.f32()?];
    exact(bytes, ())?;
    let space = *RgbSpace::ALL.get(space as usize).ok_or_else(|| unsupported("color space"))?;
    match form {
        0 => RgbColor::new(space, rgba).map_err(|e| invalid(&e)),
        1 => RgbColor::from_linear(space, rgba).map_err(|e| invalid(&e)),
        _ => Err(unsupported("color form")),
    }
}
fn decode_paint(value: &[u8], depth: u32) -> DecodeResult<Paint> {
    let mut bytes = Bytes::new(value);
    let record = framed(&mut bytes)?.ok_or_else(|| invalid("paint requires a record"))?;
    exact(bytes, ())?;
    if !check_kind(record.kind)? { return Ok(Paint::Opaque(unknown_kind(&record, depth)?)); }
    if record.kind & KIND_PARENT != 0 { return Err(invalid("paint record has children")); }
    if record.kind != COLOR { return Err(unsupported("record kind in a paint")); }
    let (mut color, mut unknown) = (None, Vec::new());
    for (tag, value) in properties(record.properties)? {
        if tag == COLOR_VALUE { color = Some(decode_color(value)?); } else { unknown_property(tag, value, COLOR, depth, &mut unknown)?; }
    }
    Ok(Paint::Color { color: color.ok_or_else(|| invalid("color paint requires a color"))?, unknown: finish(unknown) })
}
fn decode_entry(record: &Framed, depth: u32) -> DecodeResult<PaintEntry> {
    if !check_kind(record.kind)? { return Ok(PaintEntry::Opaque(unknown_kind(record, depth)?)); }
    if record.kind != FILL && record.kind != STROKE { return Err(unsupported("record kind in a style")); }
    let mut paint = None;
    let mut stroke = Stroke::new(Paint::color(RgbColor::BLACK), Stroke::DEFAULT_WIDTH);
    let mut unknown = Vec::new();
    for (tag, value) in properties(record.properties)? {
        match tag {
            PAINT => paint = Some(decode_paint(value, depth + 1)?),
            WIDTH if record.kind == STROKE => stroke.width = exact_f64(value)?,
            CAP if record.kind == STROKE => stroke.cap = match value { [0] => Cap::Butt, [1] => Cap::Round, [2] => Cap::Square, [_] => return Err(unsupported("stroke cap")), _ => return Err(invalid("stroke cap")) },
            JOIN if record.kind == STROKE => stroke.join = match value { [0] => Join::Miter, [1] => Join::Round, [2] => Join::Bevel, [_] => return Err(unsupported("stroke join")), _ => return Err(invalid("stroke join")) },
            MITER_LIMIT if record.kind == STROKE => stroke.miter_limit = exact_f64(value)?,
            PROFILE if record.kind == STROKE => stroke.profile = Some(Arc::new(decode_profile(value)?)),
            _ => unknown_property(tag, value, record.kind, depth, &mut unknown)?,
        }
    }
    let paint = paint.ok_or_else(|| invalid("paint entry requires a paint"))?;
    if record.kind == FILL { return Ok(PaintEntry::Fill(Fill { paint, unknown: finish(unknown) })); }
    stroke.paint = paint;
    stroke.unknown = finish(unknown);
    stroke.validate().map_err(invalid)?;
    Ok(PaintEntry::Stroke(stroke))
}
fn exact_f64(value: &[u8]) -> DecodeResult<f64> { let mut bytes = Bytes::new(value); let v = bytes.f64()?; exact(bytes, v) }
fn decode_style(value: &[u8], depth: u32) -> DecodeResult<Style> {
    let mut bytes = Bytes::new(value);
    let mut entries = Vec::new();
    let mut open = 0u32;
    while !bytes.done() {
        match framed(&mut bytes)? {
            None => {
                open = open.checked_sub(1).ok_or_else(|| invalid("unbalanced end marker"))?;
                entries.push(PaintEntry::Opaque(OpaqueRecord { kind: 0, id: 0, properties: Arc::from([]) }));
            }
            Some(record) if open > 0 => {
                validate_frame(record.properties, depth + 1)?;
                if record.kind & KIND_PARENT != 0 { open += 1; }
                entries.push(PaintEntry::Opaque(opaque(&record)));
            }
            Some(record) => {
                let entry = decode_entry(&record, depth + 1)?;
                if record.kind & KIND_PARENT != 0 { open += 1; }
                entries.push(entry);
            }
        }
        if open > MAX_DEPTH { return Err(unsupported("nesting exceeds the admitted depth")); }
    }
    if open != 0 { return Err(invalid("unbalanced end marker")); }
    Ok(Style::new(entries))
}
fn decode_path(record: &Framed) -> DecodeResult<PathItem> {
    if record.id == 0 { return Err(invalid("path identity")); }
    let mut geometry = None;
    let mut path = PathItem::new(ItemId(record.id), Arc::new(PathGeometry::new(vec![Subpath::new(&[Anchor::corner([0.; 2])], false)]).unwrap()), Arc::default());
    let mut unknown = Vec::new();
    for (tag, value) in properties(record.properties)? {
        match tag {
            GEOMETRY => geometry = Some(decode_geometry(value)?),
            FILL_RULE => path.fill_rule = match value { [0] => FillRule::NonZero, [1] => FillRule::EvenOdd, [_] => return Err(unsupported("fill rule")), _ => return Err(invalid("fill rule")) },
            STYLE => path.style = Arc::new(decode_style(value, 1)?),
            HIDDEN => path.hidden = flag(value).ok_or_else(|| invalid("hidden flag"))?,
            NAME => match std::str::from_utf8(value) {
                Ok(name) if name.len() <= crate::authored::MAX_NAME_BYTES => path.name = Some(name.into()),
                _ => unknown.push(UnknownProperty { tag, value: value.into() }),
            },
            LOCKED => match flag(value) { Some(locked) => path.locked = locked, None => unknown.push(UnknownProperty { tag, value: value.into() }) },
            _ => unknown_property(tag, value, PATH, 1, &mut unknown)?,
        }
    }
    path.geometry = Arc::new(geometry.ok_or_else(|| invalid("path requires geometry"))?);
    path.unknown = finish(unknown);
    path.validate().map_err(invalid)?;
    Ok(path)
}

pub fn decode_chunk(data: &[u8], expected: usize, open: &mut u32) -> DecodeResult<VectorChunk> {
    if expected == 0 || expected > CHUNK_RECORDS { return Err(invalid("chunk record count")); }
    let mut bytes = Bytes::new(data);
    let mut records = Vec::with_capacity(expected);
    while !bytes.done() {
        if records.len() == expected { return Err(invalid("chunk holds more records than declared")); }
        let record = match framed(&mut bytes)? {
            None => { *open = open.checked_sub(1).ok_or_else(|| invalid("unbalanced end marker"))?; Record::End }
            Some(record) if *open > 0 => { validate_frame(record.properties, *open)?; if record.kind & KIND_PARENT != 0 { *open += 1; } Record::Opaque(opaque(&record)) }
            Some(record) => {
                let decoded = if check_kind(record.kind)? {
                    if record.kind != PATH { return Err(unsupported("record kind in an item list")); }
                    Record::Path(decode_path(&record)?)
                } else { Record::Opaque(unknown_kind(&record, 0)?) };
                if record.kind & KIND_PARENT != 0 { *open += 1; }
                decoded
            }
        };
        if *open > MAX_DEPTH { return Err(unsupported("nesting exceeds the admitted depth")); }
        records.push(record);
    }
    if records.len() != expected { return Err(invalid("chunk holds fewer records than declared")); }
    let chunk = VectorChunk::new(records);
    chunk.note_encoded_len(data.len());
    Ok(chunk)
}
pub fn decode_object(chunks: &[(&[u8], usize)]) -> DecodeResult<Vec<VectorChunk>> {
    let mut open = 0;
    let mut total = 0usize;
    let mut ids = std::collections::HashSet::new();
    let decoded = chunks.iter().map(|(data, count)| {
        total += count;
        if total > MAX_RECORDS { return Err(unsupported("vector object exceeds the admitted record count")); }
        let chunk = decode_chunk(data, *count, &mut open)?;
        for id in chunk.records().iter().filter_map(Record::id) {
            if !ids.insert(id) { return Err(invalid("duplicate item identity")); }
        }
        Ok(chunk)
    }).collect::<DecodeResult<Vec<_>>>()?;
    if open != 0 { return Err(invalid("unbalanced end marker")); }
    Ok(decoded)
}

fn put_property(out: &mut Vec<u8>, tag: u64, value: &[u8]) {
    put_varuint(out, tag);
    put_varuint(out, value.len() as u64);
    out.extend_from_slice(value);
}
fn put_record(out: &mut Vec<u8>, kind: u64, id: u64, properties: &[u8]) {
    put_varuint(out, kind);
    if kind == 0 { return; }
    out.extend_from_slice(&id.to_le_bytes());
    put_varuint(out, properties.len() as u64);
    out.extend_from_slice(properties);
}
struct Properties { list: Vec<(u64, Vec<u8>)> }
impl Properties {
    fn new() -> Self { Self { list: Vec::new() } }
    fn put(&mut self, tag: u64, value: Vec<u8>) { self.list.push((tag, value)); }
    fn unknown(&mut self, unknown: &Unknown) { for p in unknown.iter().flat_map(|u| u.iter()) { self.list.push((p.tag, p.value.to_vec())); } }
    fn bytes(mut self) -> Vec<u8> {
        self.list.sort_by_key(|(tag, _)| tag >> TAG_FLAGS);
        let mut out = Vec::new();
        for (tag, value) in &self.list { put_property(&mut out, *tag, value); }
        out
    }
}
pub fn encode_geometry(geometry: &PathGeometry) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + geometry.anchor_count() * ANCHOR_BYTES);
    put_varuint(&mut out, geometry.subpaths().len() as u64);
    for subpath in geometry.subpaths() {
        put_varuint(&mut out, subpath.len() as u64);
        out.push(u8::from(subpath.closed));
        for a in subpath.anchors() {
            out.push(a.kind.0);
            for v in [a.point[0], a.point[1], a.in_handle[0], a.in_handle[1], a.out_handle[0], a.out_handle[1]] { out.extend_from_slice(&v.to_le_bytes()); }
        }
    }
    out
}
fn encode_profile(profile: &WidthProfile) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + profile.knots().len() * KNOT_BYTES);
    put_varuint(&mut out, profile.knots().len() as u64);
    for k in profile.knots() {
        out.extend_from_slice(&k.segment.to_le_bytes());
        for v in [k.t, k.left, k.right] { out.extend_from_slice(&v.to_le_bytes()); }
    }
    out
}
fn encode_color(color: &RgbColor) -> Vec<u8> {
    let space = RgbSpace::ALL.iter().position(|s| *s == color.space).unwrap() as u8;
    let (form, rgba) = match color.linear_rgb { Some([r, g, b]) => (1, [r, g, b, color.rgba[3]]), None => (0, color.rgba) };
    let mut out = vec![space, form];
    for v in rgba { out.extend_from_slice(&v.to_le_bytes()); }
    out
}
fn encode_opaque(out: &mut Vec<u8>, record: &OpaqueRecord) { put_record(out, record.kind, record.id, &record.properties); }
fn encode_paint(paint: &Paint) -> Vec<u8> {
    let mut out = Vec::new();
    match paint {
        Paint::Color { color, unknown } => { let mut p = Properties::new(); p.put(COLOR_VALUE, encode_color(color)); p.unknown(unknown); put_record(&mut out, COLOR, 0, &p.bytes()); }
        Paint::Opaque(record) => encode_opaque(&mut out, record),
    }
    out
}
fn encode_style(style: &Style) -> Vec<u8> {
    let mut out = Vec::new();
    for entry in style.entries.iter() {
        match entry {
            PaintEntry::Fill(fill) => {
                let mut p = Properties::new();
                p.put(PAINT, encode_paint(&fill.paint));
                p.unknown(&fill.unknown);
                put_record(&mut out, FILL, 0, &p.bytes());
            }
            PaintEntry::Stroke(s) => {
                let mut p = Properties::new();
                p.put(PAINT, encode_paint(&s.paint));
                if s.width != Stroke::DEFAULT_WIDTH { p.put(WIDTH, s.width.to_le_bytes().to_vec()); }
                if s.cap != Cap::Butt { p.put(CAP, vec![s.cap as u8]); }
                if s.join != Join::Miter { p.put(JOIN, vec![s.join as u8]); }
                if s.miter_limit != Stroke::DEFAULT_MITER_LIMIT { p.put(MITER_LIMIT, s.miter_limit.to_le_bytes().to_vec()); }
                if let Some(profile) = &s.profile { p.put(PROFILE, encode_profile(profile)); }
                p.unknown(&s.unknown);
                put_record(&mut out, STROKE, 0, &p.bytes());
            }
            PaintEntry::Opaque(record) => encode_opaque(&mut out, record),
        }
    }
    out
}
fn encode_path(out: &mut Vec<u8>, path: &PathItem) {
    let mut p = Properties::new();
    p.put(GEOMETRY, encode_geometry(&path.geometry));
    if path.fill_rule != FillRule::NonZero { p.put(FILL_RULE, vec![1]); }
    if !path.style.entries.is_empty() { p.put(STYLE, encode_style(&path.style)); }
    if path.hidden { p.put(HIDDEN, vec![1]); }
    if let Some(name) = path.name.as_deref().filter(|n| !n.is_empty()) { p.put(NAME, name.as_bytes().to_vec()); }
    if path.locked { p.put(LOCKED, vec![1]); }
    p.unknown(&path.unknown);
    put_record(out, PATH, path.id.0, &p.bytes());
}
pub fn encode_chunk(chunk: &VectorChunk) -> Vec<u8> {
    let out = encode_records(chunk);
    chunk.note_encoded_len(out.len());
    out
}
fn encode_records(chunk: &VectorChunk) -> Vec<u8> {
    let mut out = Vec::new();
    for record in chunk.records() {
        match record {
            Record::Path(path) => encode_path(&mut out, path),
            Record::Opaque(record) => encode_opaque(&mut out, record),
            Record::End => out.push(0),
        }
    }
    out
}
