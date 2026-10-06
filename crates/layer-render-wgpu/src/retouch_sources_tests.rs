use crate::test_support::floats;
use super::*;
use crate::test_support::pen;
use layer_core::{
    BrushSnapshot, Document, CoverageSnapshot, RasterOperation, RasterOperationKind, Point, Retouch,
    Edit, authored::{CoverageHandle, Occurrence, OccurrenceContent, OccurrenceHandle,
        PaintHandle, PaintSource, PortableId, RecordChange, SourceTarget},
    RetouchSource, color::source::rgba8_source,
};
use layer_engine::{
    CanvasEngine, InputProducer, InstantFeedbackConfig, PenEvent, PenPhase, SampleFlags,
    ViewTransform, input_queue,
};
use layer_render::{RetouchPreparation, ViewState};
#[path = "clone_tests.rs"]
mod clone;
#[path = "heal_tests.rs"]
mod heal;

const SIZE: [u32; 2] = [768, 512];
const TARGET: SourceTarget = SourceTarget::Paint(PaintHandle::from_index(0));
const PHOTO: SourceTarget = SourceTarget::Paint(PaintHandle::from_index(1));
const TARGET_USE: OccurrenceHandle = OccurrenceHandle::from_index(0);
const PAPER_USE: OccurrenceHandle = OccurrenceHandle::from_index(1);
const PHOTO_USE: OccurrenceHandle = OccurrenceHandle::from_index(2);
const FILL: [f32; 4] = [1., 0., 0., 0.5];

fn view(extent: [u32; 2]) -> ViewState {
    ViewState {
        width_px: extent[0],
        height_px: extent[1],
        document_to_surface: [1., 0., 0., 1., 0., 0.],
    }
}

fn pattern(x: u32, y: u32) -> [u8; 4] {
    [(x % 251) as u8, (y % 241) as u8, ((x / 3 + y / 5) % 256) as u8, 255]
}

fn linear(v: u8) -> f32 {
    let c = f32::from(v) / 255.;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn reference(x: u32, y: u32) -> [f32; 4] {
    let [r, g, b, _] = pattern(x, y).map(linear);
    [r, g, b, 1.]
}

fn over(top: [f32; 4], below: [f32; 4]) -> [f32; 4] {
    std::array::from_fn(|i| top[i] + below[i] * (1. - top[3]))
}

/// `top` over `below`, premultiplied linear, combined in `space`.
fn over_in(space: layer_core::BlendSpace, top: [f32; 4], below: [f32; 4]) -> [f32; 4] {
    let convert = |p: [f32; 4], f: fn(layer_core::color::RgbSpace, f64) -> f64| -> [f32; 4] {
        if p[3] <= 0. { return p; }
        std::array::from_fn(|i| if i == 3 { p[3] } else { (f(layer_core::color::RgbSpace::Srgb, f64::from(p[i] / p[3])) * f64::from(p[3])) as f32 })
    };
    if space == layer_core::BlendSpace::Linear {
        return over(top, below);
    }
    convert(over(convert(top, layer_core::color::RgbSpace::encode), convert(below, layer_core::color::RgbSpace::encode)), layer_core::color::RgbSpace::decode)
}

fn photo_document(extent: [u32; 2], pixel: impl Fn(u32, u32) -> [u8; 4]) -> Document {
    let mut doc = Document::new(PortableId::random(), extent[0], extent[1],
        layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let paint = RecordChange::insert(&doc.artwork.paint, PaintSource { color_mode: Default::default(),
        domain: extent, base: Some(layer_core::authored::PaintBase::new((rgba8_source(extent, pixel)).into())),
        raster: Default::default(), operations: Default::default(),
    });
    assert_eq!(SourceTarget::Paint(paint.handle), PHOTO);
    let mut photo = Occurrence::new(OccurrenceContent::Paint(paint.handle), "Photo");
    photo.reference = true;
    let photo = RecordChange::insert(&doc.artwork.occurrences, photo);
    assert_eq!(photo.handle, PHOTO_USE);
    let stack_handle = doc.composition().result;
    let mut stack = doc.artwork.stacks.get(stack_handle).unwrap().clone();
    stack.entries.insert(1, photo.handle);
    let stack = RecordChange::replace(&doc.artwork.stacks, stack_handle, Some(stack)).unwrap();
    doc.apply(Edit::Batch(vec![Edit::Paint(paint), Edit::Occurrence(photo), Edit::Stack(stack)])).unwrap();
    doc
}

fn document(extent: [u32; 2]) -> Document { photo_document(extent, pattern) }

fn set_blend(doc: &mut Document, blend: layer_core::BlendSpace) {
    let mut composition = doc.composition().clone();
    composition.blend = blend;
    let change = RecordChange::replace(&doc.artwork.compositions, doc.artwork.root, Some(composition)).unwrap();
    doc.apply(Edit::Composition(change)).unwrap();
}

fn occurrence_edit(doc: &Document, handle: OccurrenceHandle, update: impl FnOnce(&mut Occurrence)) -> Edit {
    let mut occurrence = doc.artwork.occurrences.get(handle).unwrap().clone();
    update(&mut occurrence);
    Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, handle, Some(occurrence)).unwrap())
}

fn reveal_all(extent: [u32; 2]) -> CoverageSnapshot {
    CoverageSnapshot::reveal_all(CoverageHandle::from_index(0), extent, [0, 0])
}

fn flush(engine: &mut CanvasEngine<WgpuRasterizer>) {
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    loop {
        engine.render_frame().unwrap();
        if !engine.has_pending_input() && !engine.has_pending_document_edits() && !engine.backend().has_pending_work() {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
}

fn engine(doc: Document, feedback: bool) -> (InputProducer<PenEvent>, CanvasEngine<WgpuRasterizer>) {
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.source_tiles.get_mut().admit(0);
    let (input, consumer) = input_queue(256);
    let extent = doc.composition().size;
    let mut engine = CanvasEngine::new(r, doc, consumer, view(extent), ViewTransform::IDENTITY).unwrap();
    engine
        .set_brush(BrushSnapshot {
            color_rgba_linear: [0., 0., 1., 1.],
            diameter: 48.,
            hardness: 1.,
            flow: 1.,
            mappings: Arc::from([]),
            ..Default::default()
        })
        .unwrap();
    engine
        .set_instant_feedback(InstantFeedbackConfig { enabled: feedback, ..Default::default() })
        .unwrap();
    engine
        .append_raster_operation(TARGET, RasterOperation {
            placement: layer_core::Affine::IDENTITY,
            coverage: reveal_all(extent),
            kind: RasterOperationKind::Fill { color: FILL, alpha_locked: false },
        })
        .unwrap();
    flush(&mut engine);
    (input, engine)
}

fn draw(engine: &mut CanvasEngine<WgpuRasterizer>, input: &mut InputProducer<PenEvent>, event: PenEvent) {
    input.push(event).unwrap();
    engine.render_frame().unwrap();
}

fn stroke(engine: &mut CanvasEngine<WgpuRasterizer>, input: &mut InputProducer<PenEvent>, sequence: u64, from: [f32; 2], to: [f32; 2]) {
    draw(engine, input, pen(sequence, PenPhase::Down, from, SampleFlags::PRIMARY));
    for i in 1..8 {
        let t = i as f32 / 8.;
        let at = [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t];
        draw(engine, input, pen(sequence + i, PenPhase::Move, at, SampleFlags::PRIMARY));
    }
    draw(engine, input, pen(sequence + 8, PenPhase::Up, to, SampleFlags::PRIMARY));
    flush(engine);
}

fn corrected_replay(engine: &mut CanvasEngine<WgpuRasterizer>, input: &mut InputProducer<PenEvent>,
    path: &[[f32; 2]], up: [f32; 2], late: bool) {
    let estimated = SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::ESTIMATED.0);
    let corrected = |mut event: PenEvent| {
        event.pressure = 0.4;
        event.surface_position.y += 6.;
        event
    };
    let events: Vec<_> = path.iter().enumerate().map(|(i, &at)| {
        let phase = if i == 0 { PenPhase::Down } else { PenPhase::Move };
        pen(1 + i as u64, phase, at, if late { estimated } else { SampleFlags::PRIMARY })
    }).collect();
    for (i, event) in events.iter().enumerate() {
        draw(engine, input, if late { *event } else { corrected(*event) });
        if late && i == 1 {
            let mut fix = corrected(events[0]);
            fix.flags = SampleFlags(SampleFlags::CORRECTION.0 | SampleFlags::ESTIMATED.0);
            draw(engine, input, fix);
        }
    }
    draw(engine, input, pen(9, PenPhase::Up, up, SampleFlags::PRIMARY));
    flush(engine);
    if late {
        for event in &events {
            let mut fix = corrected(*event);
            fix.flags = SampleFlags::CORRECTION;
            draw(engine, input, fix);
        }
        flush(engine);
    }
}

/// What a clone reads for the target page at `page`, each pixel shifted by
/// `offset`, and whether every page it needed was ready.
fn sample(r: &mut WgpuRasterizer, page: [u32; 2], offset: [f32; 2]) -> (Vec<[f32; 4]>, bool) {
    let field = crate::test_support::page_texture(r, wgpu::TextureFormat::Rgba32Float);
    let view = field.create_view(&Default::default());
    let complete = r
        .draw_retouch_source(&view, page.map(|v| v * PAGE_SIZE), [PAGE_SIZE; 2], [1.; 2], offset)
        .unwrap();
    (floats(&crate::layer_tests::page_bytes(r, &field)), complete)
}

fn live_page(r: &WgpuRasterizer, page: [u32; 2]) -> Vec<[f32; 4]> {
    let texture = r
        .paint_layers
        .iter()
        .find(|l| l.id == TARGET)
        .and_then(|l| l.pages.iter().find(|p| p.coordinate == page))
        .unwrap()
        .active()
        .texture
        .clone();
    floats(&crate::layer_tests::page_bytes(r, &texture))
}

fn target_pages(r: &WgpuRasterizer) -> Vec<([u32; 2], Vec<[f32; 4]>)> {
    let mut pages: Vec<_> =
        r.paint_layers.iter().find(|l| l.id == TARGET).unwrap().pages.iter().map(|p| p.coordinate).collect();
    pages.sort_unstable();
    pages.into_iter().map(|c| (c, live_page(r, c))).collect()
}

fn close(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 2e-3)
}

fn premultiplied(color: [f32; 4]) -> [f32; 4] {
    [color[0] * color[3], color[1] * color[3], color[2] * color[3], color[3]]
}

fn counts(engine: &CanvasEngine<WgpuRasterizer>) -> Counts {
    engine.backend().retouch.as_ref().unwrap().counts
}

#[test]
fn clone_reads_over_the_strokes_own_dabs_see_the_pixels_it_started_on() {
    let (mut input, mut engine) = engine(document(SIZE), false);
    engine.set_retouch(Some(RetouchSource::Editing));
    flush(&mut engine);
    let start = premultiplied(FILL);
    let blue = [0., 0., 1., 1.];
    draw(&mut engine, &mut input, pen(1, PenPhase::Down, [60., 64.], SampleFlags::PRIMARY));
    for (i, x) in [120., 180., 240., 300., 360.].into_iter().enumerate() {
        draw(&mut engine, &mut input, pen(2 + i as u64, PenPhase::Move, [x, 64.], SampleFlags::PRIMARY));
    }
    let painted = live_page(engine.backend(), [0, 0]);
    assert!(close(painted[64 * 256 + 150], blue), "the stroke painted its page: {:?}", painted[64 * 256 + 150]);
    for page in [[0, 0], [1, 0]] {
        let (source, complete) = sample(engine.backend_mut(), page, [0.; 2]);
        assert!(complete);
        assert!(source.iter().all(|p| close(*p, start)), "page {page:?} reads the stroke's own dabs");
    }
    let (shifted, _) = sample(engine.backend_mut(), [0, 0], [37., -30.]);
    assert!(shifted[64 * 256 + 100..64 * 256 + 219].iter().all(|p| close(*p, start)));
    assert!(close(shifted[10 * 256 + 10], [0.; 4]), "above the layer reads nothing");

    draw(&mut engine, &mut input, pen(9, PenPhase::Up, [360., 64.], SampleFlags::PRIMARY));
    flush(&mut engine);
    let (after, _) = sample(engine.backend_mut(), [0, 0], [0.; 2]);
    assert!(after.iter().all(|p| close(*p, start)), "the finished stroke keeps its start");

    draw(&mut engine, &mut input, pen(10, PenPhase::Down, [60., 400.], SampleFlags::PRIMARY));
    draw(&mut engine, &mut input, pen(11, PenPhase::Move, [90., 400.], SampleFlags::PRIMARY));
    let committed = live_page(engine.backend(), [0, 0]);
    let (second, complete) = sample(engine.backend_mut(), [0, 0], [0.; 2]);
    assert!(complete);
    assert!(close(second[64 * 256 + 150], blue), "a second stroke copies from the first");
    assert!(second.iter().zip(&committed).all(|(a, b)| close(*a, *b)));
    draw(&mut engine, &mut input, pen(12, PenPhase::Up, [90., 400.], SampleFlags::PRIMARY));
    flush(&mut engine);
}

#[test]
fn retouch_source_preserves_unmixed_perceptual_pixels() {
    for opacity in [0., 1.] {
        let mut doc = document(SIZE);
        set_blend(&mut doc, layer_core::BlendSpace::Perceptual);
        let edit = occurrence_edit(&doc, PAPER_USE, |o| o.reference = true);
        doc.apply(edit).unwrap();
        let (mut input, mut engine) = engine(doc, false);
        engine.append_raster_operation(TARGET, RasterOperation {
            placement: layer_core::Affine::IDENTITY,
            coverage: reveal_all(SIZE),
            kind: RasterOperationKind::Fill { color: [0.1433, 0.27534, 0.423, 1.], alpha_locked: false },
        }).unwrap();
        engine.set_layer_opacity(TARGET_USE, opacity).unwrap();
        engine.set_retouch(Some(RetouchSource::References));
        engine.set_retouch_points(&[Point { x: 40., y: 40. }]);
        flush(&mut engine);
        draw(&mut engine, &mut input, pen(1, PenPhase::Down, [600., 400.], SampleFlags::PRIMARY));
        let (actual, complete) = sample(engine.backend_mut(), [0, 0], [0.; 2]);
        assert!(complete);
        let r = engine.backend();
        let expected = if opacity == 0. {
            let cache = &r.retouch.as_ref().unwrap().cache;
            let texture = &cache.slots[cache.pages[&[0, 0]]].page.texture;
            floats(&crate::layer_tests::page_bytes(r, texture))
        } else { live_page(r, [0, 0]) };
        let error = actual.iter().flatten().zip(expected.iter().flatten()).map(|(a,b)| (a-b).abs()).fold(0., f32::max);
        assert_eq!(error, 0., "opacity {opacity}: an unmixed source must keep its exact pixels");
    }
}

#[test]
fn current_and_below_lays_the_target_at_its_opacity_over_the_references() {
    for space in layer_core::BlendSpace::ALL {
        current_and_below_in(space);
    }
}
fn current_and_below_in(space: layer_core::BlendSpace) {
    let mut doc = document(SIZE);
    set_blend(&mut doc, space);
    let (mut input, mut engine) = engine(doc, false);
    engine.set_layer_opacity(TARGET_USE, 0.5).unwrap();
    engine.set_retouch(Some(RetouchSource::References));
    engine.set_retouch_points(&[Point { x: 40., y: 40. }]);
    flush(&mut engine);
    draw(&mut engine, &mut input, pen(1, PenPhase::Down, [600., 400.], SampleFlags::PRIMARY));
    let top = premultiplied(FILL).map(|v| v * 0.5);
    for offset in [[0., 0.], [13., 7.]] {
        let (source, complete) = sample(engine.backend_mut(), [0, 0], offset);
        assert!(complete);
        for (x, y) in [(20u32, 30u32), (100, 200), (230, 17)] {
            let expected = over_in(space, top, reference(x + offset[0] as u32, y + offset[1] as u32));
            let got = source[(y * 256 + x) as usize];
            assert!(close(got, expected), "{space:?} {offset:?} ({x},{y}): {got:?} != {expected:?}");
        }
    }
    let (half, _) = sample(engine.backend_mut(), [0, 0], [-5.5, 0.]);
    let [left, right] = [reference(94, 40), reference(95, 40)];
    let expected = over_in(space, top, std::array::from_fn(|i| (left[i] + right[i]) / 2.));
    assert!(close(half[40 * 256 + 100], expected), "a half-pixel shift blends neighbors in linear light");
    draw(&mut engine, &mut input, pen(2, PenPhase::Up, [600., 400.], SampleFlags::PRIMARY));
    flush(&mut engine);

    let edit = occurrence_edit(engine.document(), PHOTO_USE, |o| o.reference = false);
    engine.apply_edit(edit).unwrap();
    flush(&mut engine);
    draw(&mut engine, &mut input, pen(3, PenPhase::Down, [600., 400.], SampleFlags::PRIMARY));
    let (alone, _) = sample(engine.backend_mut(), [0, 0], [0.; 2]);
    assert!(alone.iter().all(|p| close(*p, premultiplied(FILL))), "with nothing marked the target alone is the source");
    draw(&mut engine, &mut input, pen(4, PenPhase::Up, [600., 400.], SampleFlags::PRIMARY));
    flush(&mut engine);
}

fn settle(r: &mut WgpuRasterizer, doc: &Document) {
    let mut frame = crate::test_support::packet(doc.scene(), [2560, 2560]);
    frame.composite_all = false;
    for _ in 0..200 {
        r.submit(frame).unwrap();
        if !r.has_pending_work() {
            return;
        }
    }
    panic!("prefetch never settled");
}

#[test]
fn the_reference_cache_follows_its_frame_and_evicts_the_least_recent_page() {
    let mut doc = document([2560, 2560]);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.prepare_retouch(Some(&RetouchPreparation {
        target: TARGET,
        retouch: Retouch { source: RetouchSource::References, references: Arc::new([PHOTO_USE].into()), ..Retouch::default() },
        points: vec![Point { x: 10., y: 10. }],
    }));
    settle(&mut r, &doc);
    let retouch = |r: &WgpuRasterizer| r.retouch.as_ref().unwrap().counts;
    let cached = |r: &WgpuRasterizer| r.retouch.as_ref().unwrap().cached_pages();
    assert_eq!(retouch(&r).captures, 9, "the ring around the focus point, clipped to the canvas");
    assert_eq!(cached(&r), 9);
    let (source, complete) = sample(&mut r, [1, 1], [0.; 2]);
    assert!(complete);
    assert!(close(source[5 * 256 + 7], reference(263, 261)));
    assert_eq!(retouch(&r).captures, 9, "cached pages are reused");

    let edit = occurrence_edit(&doc, TARGET_USE, |o| o.opacity = 0.25);
    doc.apply(edit).unwrap();
    settle(&mut r, &doc);
    assert_eq!(retouch(&r).captures, 9, "changing the target keeps the cache");

    let edit = occurrence_edit(&doc, PHOTO_USE, |o| o.opacity = 0.5);
    doc.apply(edit).unwrap();
    settle(&mut r, &doc);
    assert_eq!(retouch(&r).captures, 18, "changing a reference captures again");
    let (dimmed, _) = sample(&mut r, [1, 1], [0.; 2]);
    assert!(close(dimmed[5 * 256 + 7], reference(263, 261).map(|v| v * 0.5)));

    let SourceTarget::Paint(photo) = PHOTO else { unreachable!() };
    let mut source = doc.artwork.paint.get(photo).unwrap().clone();
    source.base = Some(layer_core::authored::PaintBase::new((rgba8_source([2560, 2560], |x, y| pattern(y, x))).into()));
    let change = RecordChange::replace(&doc.artwork.paint, photo, Some(source)).unwrap();
    doc.apply(Edit::Paint(change)).unwrap();
    settle(&mut r, &doc);
    let (swapped, _) = sample(&mut r, [1, 1], [0.; 2]);
    assert!(close(swapped[5 * 256 + 7], reference(261, 263).map(|v| v * 0.5)), "a replaced photo is a new frame");

    for y in 0..10 {
        for x in 0..10 {
            assert!(sample(&mut r, [x, y], [0.; 2]).1);
        }
    }
    assert!(cached(&r) <= REFERENCE_PAGES);
    let captures = retouch(&r).captures;
    let (first, _) = sample(&mut r, [0, 0], [0.; 2]);
    assert!(retouch(&r).captures > captures, "the least recently used page was evicted");
    assert!(close(first[3 * 256 + 3], reference(3, 3).map(|v| v * 0.5)), "an evicted page is captured again");
    settle(&mut r, &doc);
    let bytes = r.retouch.as_ref().unwrap().storage_bytes();
    let page = u64::from(PAGE_SIZE * PAGE_SIZE) * 16;
    assert_eq!(bytes, (16 + REFERENCE_PAGES as u64) * page + PARAMETER_BYTES, "idle sources keep only their pages");
    assert_eq!(r.metrics().retouch_storage_bytes, bytes);
    r.set_telemetry_enabled(true);
    assert!(r.telemetry().resident_bytes >= bytes);
    r.prepare_retouch(None);
    r.submit(crate::test_support::packet(doc.scene(), [2560, 2560])).unwrap();
    assert_eq!(r.metrics().retouch_storage_bytes, 0);
}

fn scribble(
    engine: &mut CanvasEngine<WgpuRasterizer>,
    input: &mut InputProducer<PenEvent>,
    during: &mut dyn FnMut(&mut CanvasEngine<WgpuRasterizer>),
) {
    draw(engine, input, pen(1, PenPhase::Down, [120., 120.], SampleFlags::PRIMARY));
    for (i, x) in [150., 180., 210.].into_iter().enumerate() {
        draw(engine, input, pen(2 + i as u64, PenPhase::Move, [x, 130.], SampleFlags::PRIMARY));
    }
    during(engine);
    draw(engine, input, pen(8, PenPhase::Up, [220., 130.], SampleFlags::PRIMARY));
    flush(engine);
}

#[test]
fn contacts_neither_upload_nor_wait_and_a_miss_replays_after_pen_up() {
    let mut warm = None;
    let mut live_copies = 0;
    for miss in [false, true] {
        let mut doc = document([4608, 1024]);
        let edit = occurrence_edit(&doc, PAPER_USE, |o| o.reference = true);
        doc.apply(edit).unwrap();
        let (mut input, mut engine) = engine(doc, false);
        engine.set_retouch(Some(RetouchSource::References));
        engine.set_retouch_points(&[Point { x: 100., y: 100. }]);
        flush(&mut engine);
        let metrics = engine.backend().metrics();
        let before = counts(&engine);
        scribble(&mut engine, &mut input, &mut |engine| {
            let (_, complete) = sample(engine.backend_mut(), [0, 0], [30., 40.]);
            assert!(complete, "prefetched pages are ready during contact");
            if miss {
                let (source, complete) = sample(engine.backend_mut(), [0, 0], [1000., 200.]);
                assert!(!complete, "a page that would upload is a miss");
                assert!(close(source[0], premultiplied(FILL)), "a missed reference stays transparent");
            }
            let during = counts(engine);
            let now = engine.backend().metrics();
            assert_eq!(during.blocking, before.blocking, "no capture uploads or waits during contact");
            assert_eq!(during.misses > before.misses, miss);
            assert_eq!(now.source_upload_submissions, metrics.source_upload_submissions);
            assert_eq!(now.source_tile_misses, metrics.source_tile_misses);
            assert_eq!(now.native_restore_submissions, metrics.native_restore_submissions);
        });
        let copies = counts(&engine).copies - before.copies;
        let pages = target_pages(engine.backend());
        if miss {
            assert_eq!(copies, 2 * live_copies, "the replay copied the restored pages again");
            let (source, complete) = sample(engine.backend_mut(), [0, 0], [1000., 200.]);
            assert!(complete, "after contact the source can wait");
            assert!(close(source[10 * 256 + 10], over(premultiplied(FILL), reference(1010, 210))));
            assert_eq!(Some(pages), warm.take(), "the replay paints what a stroke without a miss painted");
            engine.undo().unwrap();
            flush(&mut engine);
            assert!(
                target_pages(engine.backend()).iter().all(|(_, page)| page.iter().all(|p| close(*p, premultiplied(FILL)))),
                "the replayed stroke is one undo step"
            );
        } else {
            assert!(copies > 0);
            live_copies = copies;
            warm = Some(pages);
        }
    }
}

#[test]
fn stroke_start_pages_survive_estimated_samples_and_corrections() {
    let path = [[80., 90.], [130., 110.], [190., 100.], [250., 140.]];
    let mut results = Vec::new();
    for late in [false, true] {
        let (mut input, mut engine) = engine(document(SIZE), true);
        engine.set_retouch(Some(RetouchSource::Editing));
        flush(&mut engine);
        corrected_replay(&mut engine, &mut input, &path, [260., 140.], late);
        let (source, complete) = sample(engine.backend_mut(), [0, 0], [0.; 2]);
        assert!(complete);
        assert!(source.iter().all(|p| close(*p, premultiplied(FILL))), "late={late}: replays copy the restored start");
        results.push(target_pages(engine.backend()));
    }
    assert_eq!(results[0], results[1], "corrections replay to the direct stroke");
}

#[test]
fn retouch_reference_reads_arbitrary_base_offset_and_transparent_tile_override() {
    use layer_core::authored::{PaintBase, PaintBasePolicy};
    use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey};
    let extent=[600,520];let mut doc=document(extent);
    let SourceTarget::Paint(photo)=PHOTO else {unreachable!()};
    let paint=doc.artwork.paint.get_mut(photo).unwrap();
    paint.base=Some(PaintBase {image:rgba8_source([300,270],pattern).into(),offset:[17,31],policy:PaintBasePolicy::SourceProfile});
    paint.raster=RasterRevision::backed(RasterData {tiles:[(TileKey {plane:RasterPlane::Color,coordinate:[1,1]},RasterTile::backed(TileBlob::encode(layer_core::color::DocumentColor::default().paint_descriptor(),&vec![0;256*256*4]).unwrap()))].into(),..Default::default()});
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.prepare_retouch(Some(&RetouchPreparation {target:TARGET,retouch:Retouch {source:RetouchSource::References,references:Arc::new([PHOTO_USE].into()),..Default::default()},points:vec![Point {x:250.,y:250.}]}));
    for _ in 0..100 {r.submit(crate::test_support::packet(doc.scene(),extent)).unwrap();if !r.has_pending_work(){break;}}
    let (pixels,complete)=sample(&mut r,[0,0],[0.;2]);assert!(complete);
    for (x,y) in [(0,0),(17,31),(250,250),(255,255)] {
        let expected=if x>=17&&y>=31 {reference(x-17,y-31)}else{[0.;4]};
        assert!(close(pixels[(y*256+x) as usize],expected),"{x},{y}");
    }
    let (pixels,complete)=sample(&mut r,[1,1],[0.;2]);assert!(complete);
    assert!(pixels.iter().all(|p|*p==[0.;4]));
}
