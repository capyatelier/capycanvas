use layer_core::{Artwork, Document, Edit, Editor, EvaluationContext, Occurrence, OccurrenceContent, PaintSource, RecordChange,
    authored::{Handle, PortableId, Resource, Store, vector::*}, color::RgbColor};
use std::{error::Error, io::Write, sync::{Arc, atomic::AtomicBool}};

pub struct Random(pub u64);
impl Random {
    pub fn next(&mut self) -> f64 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; (self.0 >> 11) as f64 / (1u64 << 53) as f64 }
    pub fn range(&mut self, a: f64, b: f64) -> f64 { a + (b - a) * self.next() }
}

pub struct Ids(u128);
impl Ids {
    pub fn new(key: &str) -> Self {
        let seed = key.bytes().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3));
        Self(u128::from(seed) << 64)
    }
    pub fn next(&mut self) -> PortableId { self.0 += 1; PortableId::from_bytes(self.0.to_be_bytes()) }
}
fn renumber<T: Clone>(store: &Store<T>, ids: &mut Ids) -> Store<T> {
    let mut renumbered = Store::default();
    for index in 0..store.capacity() as u32 {
        match store.get(Handle::from_index(index)) { Some(value) => renumbered.insert(ids.next(), value.clone()), None => renumbered.reserve(ids.next()) }.expect("fresh identity");
    }
    renumbered
}
pub fn package(mut artwork: Artwork, mut ids: Ids, output: &mut impl Write) -> Result<(), Box<dyn Error>> {
    let ids = &mut ids;
    for (handle, _, _) in artwork.vectors.clone().iter() {
        let object = artwork.vectors.get_mut(handle).ok_or("Missing vector object")?;
        *object = VectorObject::from_chunks(object.chunks().iter().map(|chunk| Resource::with_id(ids.next(), chunk.storage().clone())).collect()).with_next_item(object.next_item());
    }
    let a = &mut artwork;
    a.id = ids.next();
    (a.compositions, a.stacks, a.occurrences, a.paint) = (renumber(&a.compositions, ids), renumber(&a.stacks, ids), renumber(&a.occurrences, ids), renumber(&a.paint, ids));
    (a.objects, a.vectors, a.coverage, a.effects) = (renumber(&a.objects, ids), renumber(&a.vectors, ids), renumber(&a.coverage, ids), renumber(&a.effects, ids));
    (a.selections, a.guides, a.outputs) = (renumber(&a.selections, ids), renumber(&a.guides, ids), renumber(&a.outputs, ids));
    let capture = Editor::new(Document::from_artwork(artwork)?).capture(0, EvaluationContext::default())?;
    let cancelled = AtomicBool::new(false);
    layer_core::package::codec::PreparedPackage::prepare(&capture, None, &cancelled)?.write(output, &cancelled)?;
    Ok(())
}

fn paint(rgba: [f32; 4]) -> Paint { Paint::color(RgbColor::new(Default::default(), rgba).unwrap()) }
fn hatching(extent: [f64; 2], count: u64) -> Vec<Record> {
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    (0..count).map(|i| {
        let start = [random.range(0., extent[0]), random.range(0., extent[1])];
        let (angle, length, bend) = (random.range(0.4, 0.9), random.range(40., 220.), random.range(-20., 20.));
        let points: Vec<[f64; 2]> = (0..4).map(|k| { let t = f64::from(k) / 3.; [start[0] + angle.cos() * length * t - angle.sin() * bend * t * (1. - t) * 4., start[1] + angle.sin() * length * t + angle.cos() * bend * t * (1. - t) * 4.] }).collect();
        let anchors: Vec<Anchor> = points.iter().map(|p| Anchor::corner(*p)).collect();
        let mut stroke = Stroke::new(paint([random.next() as f32 * 0.3, random.next() as f32 * 0.3, random.next() as f32 * 0.3, 1.]), random.range(2., 6.));
        (stroke.cap, stroke.join) = (Cap::Round, Join::Round);
        stroke.profile = Some(Arc::new(WidthProfile::new(vec![WidthKnot::new(0, 0., 0.1, 0.1), WidthKnot::new(1, 0.5, 1., 1.), WidthKnot::new(2, 1., 0.05, 0.05)]).unwrap()));
        Record::Path(PathItem::new(ItemId(i + 1), Arc::new(PathGeometry::new(vec![Subpath::new(&anchors, false)]).unwrap()), Arc::new(Style::new(vec![PaintEntry::Stroke(stroke)]))))
    }).collect()
}
fn tones(extent: [f64; 2]) -> Vec<Record> {
    (0..12u64).map(|i| {
        let x = extent[0] * i as f64 / 12.;
        let corners = [[x, 0.], [x + extent[0] / 20., 0.], [x + extent[0] / 20., extent[1]], [x, extent[1]]].map(Anchor::corner);
        Record::Path(PathItem::new(ItemId(i + 1), Arc::new(PathGeometry::new(vec![Subpath::new(&corners, true)]).unwrap()),
            Arc::new(Style::new(vec![PaintEntry::Fill(Fill { paint: paint([0.2, 0.4, 0.9, 0.25]), unknown: None })]))))
    }).collect()
}

pub fn crowded_vector(width: u32, height: u32, count: u64, raster_ink: bool) -> Result<Document, Box<dyn Error>> {
    let extent = [f64::from(width), f64::from(height)];
    let mut document = Document::new(PortableId::random(), width, height, layer_core::DocumentNames { paint: "Base".into(), paper: "Paper".into() });
    let (hatched, edit) = document.create_vector_layer_edit(if raster_ink { "Hatching" } else { "Ink" }, VectorObject::from_records(hatching(extent, count)), None, 0)?;
    document.apply(edit)?;
    let ink = if raster_ink {
        let source = RecordChange::insert(&document.artwork.paint, PaintSource { color_mode: Default::default(), domain: [width, height], raster: Default::default(), base: None, operations: Default::default() });
        let occurrence = RecordChange::insert(&document.artwork.occurrences, Occurrence::new(OccurrenceContent::Paint(source.handle), "Ink"));
        let stack = document.composition().result;
        let mut entries = document.artwork.stacks.get(stack).unwrap().clone();
        let at = entries.entries.iter().position(|h| *h == hatched).ok_or("The hatching layer is missing")?;
        entries.entries.insert(at, occurrence.handle);
        let membership = RecordChange::replace(&document.artwork.stacks, stack, Some(entries))?;
        let handle = occurrence.handle;
        document.apply(Edit::Batch(vec![Edit::Paint(source), Edit::Occurrence(occurrence), Edit::Stack(membership)]))?;
        handle
    } else { hatched };
    let (_, edit) = document.create_vector_layer_edit("Tones", VectorObject::from_records(tones(extent)), None, 0)?;
    document.apply(edit)?;
    document.apply(document.select_occurrence_edit(ink)?)?;
    Ok(document)
}
