//! The engine and renderer harness the integration tests share: a headless
//! native renderer, composite captures and pen strokes.
#![allow(dead_code)]
use layer_core::*;
use std::sync::{Arc, atomic::AtomicBool};
use layer_engine::{
    CanvasEngine, InputProducer, PenEvent, PenPhase, SampleFlags, ToolKind, ViewTransform,
    input_queue,
};
use layer_render::ViewState;
use layer_render_wgpu::WgpuRasterizer;

pub type Engine = CanvasEngine<WgpuRasterizer>;
pub const SIZE: [u32; 2] = [384, 256];

pub struct Image {
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
}
impl Image {
    pub fn crop(&self, origin: [u32; 2], size: [u32; 2]) -> Image {
        let mut rgba = Vec::with_capacity((size[0] * size[1] * 4) as usize);
        for y in origin[1]..origin[1] + size[1] {
            let start = ((y * self.size[0] + origin[0]) * 4) as usize;
            rgba.extend_from_slice(&self.rgba[start..start + size[0] as usize * 4]);
        }
        Image { size, rgba }
    }
    pub fn assert_eq(&self, other: &Image, what: &str) {
        assert_eq!(self.size, other.size, "{what}: size");
        let first = self.rgba.iter().zip(&other.rgba).position(|(a, b)| a != b);
        assert_eq!(first.map(|i| [(i / 4) as u32 % self.size[0], (i / 4) as u32 / self.size[0]]), None, "{what}: first differing pixel");
    }
    /// Every channel within `tolerance` codes of `other`; the color of pixels
    /// transparent in both is ignored.
    pub fn assert_near(&self, other: &Image, tolerance: u8, what: &str) {
        assert_eq!(self.size, other.size, "{what}: size");
        let (pixel, difference) = self
            .rgba
            .chunks_exact(4)
            .zip(other.rgba.chunks_exact(4))
            .map(|(a, b)| {
                let channels = if a[3] == 0 && b[3] == 0 { 3..4 } else { 0..4 };
                channels.map(|c| a[c].abs_diff(b[c])).max().unwrap_or(0)
            })
            .enumerate()
            .max_by_key(|(_, d)| *d)
            .unwrap_or_default();
        let at = [pixel as u32 % self.size[0], pixel as u32 / self.size[0]];
        assert!(difference <= tolerance, "{what}: {difference} codes apart at {at:?}");
    }
}

pub fn engine(document: Document) -> (Engine, InputProducer<PenEvent>) {
    let gpu = WgpuRasterizer::new_native_headless(document.composition().color).expect("physical GPU required");
    let (producer, consumer) = input_queue(64);
    let view = ViewState {
        width_px: 1024,
        height_px: 768,
        document_to_surface: Affine::IDENTITY.0,
        };
    let mut engine = CanvasEngine::new(gpu, document, consumer, view, ViewTransform::IDENTITY).unwrap();
    engine.render_frame_at(0).unwrap();
    (engine, producer)
}

pub fn image(engine: &mut Engine, time: u64) -> Image {
    engine.render_frame_at(time).unwrap();
    while engine.has_pending_document_edits() {
        std::thread::yield_now();
        engine.render_frame_at(time).unwrap();
    }
    let size = engine.document().composition().size;
    let rgba = engine.backend_mut().readback_srgb_rgba8().unwrap();
    Image { size, rgba }
}

pub fn draw(engine: &mut Engine, input: &mut InputProducer<PenEvent>, color: [f32; 4], from: Point, to: Point, start: u64) {
    let mut brush = default_brush(DefaultBrushPreset::GPen);
    brush.diameter = 40.;
    brush.color_rgba_linear = color;
    engine.set_brush(brush).unwrap();
    draw_with_brush(engine, input, from, to, start);
}

/// A straight pen stroke with the engine's current brush.
pub fn draw_with_brush(engine: &mut Engine, input: &mut InputProducer<PenEvent>, from: Point, to: Point, start: u64) {
    for i in 0..10u64 {
        let t = i as f32 / 9.;
        let timestamp_ns = start + i * 8_000_000;
        input.push(pen_sample(i, 9, timestamp_ns,
            Point { x: from.x + (to.x - from.x) * t, y: from.y + (to.y - from.y) * t })).unwrap();
        drain_input(engine, timestamp_ns, false);
    }
}

pub fn pen_sample(index: u64, last: u64, timestamp_ns: u64, surface_position: Point) -> PenEvent {
    PenEvent {
        device_id: 1,
        sequence: index,
        timestamp_ns,
        view_revision: 0,
        surface_position,
        pressure: 0.8,
        tilt_radians: [0., 0.],
        twist_radians: 0.,
        distance: 0.,
        phase: if index == 0 {
            PenPhase::Down
        } else if index == last {
            PenPhase::Up
        } else {
            PenPhase::Move
        },
        tool: ToolKind::Pen,
        flags: SampleFlags::PRIMARY,
    }
}

pub fn drain_input(engine: &mut Engine, timestamp_ns: u64, wait_idle: bool) {
    engine.render_frame_at(timestamp_ns).unwrap();
    if wait_idle { engine.backend_mut().wait_idle().unwrap(); }
    while engine.has_pending_input() {
        if !wait_idle { std::thread::yield_now(); }
        engine.render_frame_at(timestamp_ns).unwrap();
        if wait_idle { engine.backend_mut().wait_idle().unwrap(); }
    }
}


pub fn named_document(names: &[&str], extent: [u32; 2], space: BlendSpace) -> Document {
    let mut doc = Document::new(PortableId::random(), extent[0], extent[1], DocumentNames {
        paint: names[0].into(), paper: "Paper".into(),
    });
    doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().blend = space;
    for (index, name) in names.iter().enumerate().skip(1) { add_paint(&mut doc, name, index); }
    doc
}

pub fn named_occurrence(doc: &Document, name: &str) -> OccurrenceHandle {
    doc.scene().order().iter().copied().find(|h| doc.scene().occurrence(*h).unwrap().name.as_ref() == name).unwrap()
}

pub fn add_paint(doc: &mut Document, name: &str, index: usize) -> OccurrenceHandle {
    let source = RecordChange::insert(&doc.artwork.paint, PaintSource {
        domain: doc.composition().size, raster: Default::default(), original: None, operations: Default::default(),
    });
    let occurrence = RecordChange::insert(&doc.artwork.occurrences,
        Occurrence::new(OccurrenceContent::Paint(source.handle), name));
    let handle = occurrence.handle;
    let stack_handle = doc.composition().result;
    let mut stack = doc.artwork.stacks.get(stack_handle).unwrap().clone();
    stack.entries.insert(index, handle);
    doc.apply(Edit::Batch(vec![Edit::Paint(source), Edit::Occurrence(occurrence),
        Edit::Stack(RecordChange::replace(&doc.artwork.stacks, stack_handle, Some(stack)).unwrap())])).unwrap();
    handle
}

pub fn paint(doc: &Document, occurrence: OccurrenceHandle) -> &PaintSource {
    doc.scene().paint_source(occurrence).unwrap()
}

pub fn paint_mut(doc: &mut Document, occurrence: OccurrenceHandle) -> &mut PaintSource {
    let OccurrenceContent::Paint(handle) = doc.artwork.occurrences.get(occurrence).unwrap().content else { panic!("paint occurrence") };
    doc.artwork.paint.get_mut(handle).unwrap()
}

pub fn occurrence_edit(doc: &Document, handle: OccurrenceHandle, update: impl FnOnce(&mut Occurrence)) -> Edit {
    let mut occurrence = doc.artwork.occurrences.get(handle).unwrap().clone();
    update(&mut occurrence);
    Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, handle, Some(occurrence)).unwrap())
}

pub fn mask_edit(doc: &Document, handle: OccurrenceHandle, mut mask: CoverageSnapshot) -> Edit {
    let coverage = RecordChange::insert(&doc.artwork.coverage, mask.source);
    mask.use_.source = coverage.handle;
    Edit::Batch(vec![Edit::Coverage(coverage), occurrence_edit(doc, handle, |o| o.mask = Some(mask.use_))])
}

pub fn convert_group(doc: &Document, handle: OccurrenceHandle, children: &[OccurrenceHandle]) -> Edit {
    let stack = RecordChange::insert(&doc.artwork.stacks, Stack { entries: children.to_vec() });
    let mut edits = vec![occurrence_edit(doc, handle, |o| o.content = OccurrenceContent::Stack(stack.handle))];
    for (id, _, old) in doc.artwork.stacks.iter() {
        if old.entries.iter().any(|h| children.contains(h)) {
            let mut next = old.clone();
            next.entries.retain(|h| !children.contains(h));
            edits.push(Edit::Stack(RecordChange::replace(&doc.artwork.stacks, id, Some(next)).unwrap()));
        }
    }
    edits.push(Edit::Stack(stack));
    Edit::Batch(edits)
}

pub fn effect_edit(doc: &Document, handle: OccurrenceHandle, effect: EffectInstance) -> Edit {
    let definition = RecordChange::insert(&doc.artwork.definitions, Definition {
        program: effect.program,
    });
    let application = RecordChange::insert(&doc.artwork.effects, EffectApplication {
        definition: definition.handle, values: effect.values, domain: doc.composition().size,
    });
    Edit::Batch(vec![occurrence_edit(doc, handle, |o| o.content = OccurrenceContent::Effect(application.handle)),
        Edit::Definition(definition), Edit::Effect(application)])
}

pub fn capture(doc: &Document) -> ArtworkCapture {
    Editor::new(doc.clone()).capture(0, doc.output().context.clone()).unwrap()
}

pub fn package_bytes(doc: &Document) -> Vec<u8> {
    let cancelled = AtomicBool::new(false);
    let package = package::codec::PreparedPackage::prepare(&capture(doc), None, &cancelled).unwrap();
    let mut bytes = Vec::new();
    package.write(&mut bytes, &cancelled).unwrap();
    bytes
}

pub fn reopen(bytes: &[u8]) -> Document {
    let source = package::transport::ChunkedBytes::new(bytes.chunks(package::MAX_RANGE_BYTES).map(Arc::from).collect()).unwrap();
    let backing = package::ImmutableBacking::new(Arc::new(source)).unwrap();
    let package::codec::OpenOutcome::Candidate { artwork, .. } = package::codec::open(backing,
        ProjectLimits::default(), &AtomicBool::new(false)).unwrap() else { panic!("editable package") };
    let mut doc = Document::from_artwork(artwork).unwrap();
    let active = doc.scene().order().iter().copied().find(|h| doc.scene().paint_source(*h).is_some());
    doc.working.occurrence = active;
    doc.working.target = active.and_then(|h| doc.scene().source_target(h));
    doc
}
