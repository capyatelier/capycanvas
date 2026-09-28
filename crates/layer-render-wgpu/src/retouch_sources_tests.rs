use super::*;
use layer_core::{
    BrushSnapshot, Document, LayerMask, LayerOperation, LayerOperationKind, Point, Retouch,
    RetouchSource, color::source::rgba8_source,
};
use layer_engine::{
    CanvasEngine, InputProducer, InstantFeedbackConfig, PenEvent, PenPhase, SampleFlags, ToolKind,
    ViewTransform, input_queue,
};
use layer_render::{RetouchPreparation, ViewState};
#[path = "clone_tests.rs"]
mod clone;
#[path = "heal_tests.rs"]
mod heal;

const SIZE: [u32; 2] = [768, 512];
const TARGET: LayerId = LayerId(1);
const PHOTO: LayerId = LayerId(3);
const FILL: [f32; 4] = [1., 0., 0., 0.5];

fn view(extent: [u32; 2]) -> ViewState {
    ViewState {
        width_px: extent[0],
        height_px: extent[1],
        document_to_surface: [1., 0., 0., 1., 0., 0.],
        background_rgba_linear: [0.; 4],
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

/// The target (layer 1) above a patterned photo marked as a reference, above
/// the paper.
fn document(extent: [u32; 2]) -> Document {
    let mut doc = Document::new("retouch sources", extent[0], extent[1]);
    assert_eq!(doc.allocate_layer_id(), PHOTO);
    let mut photo = Layer::paint(PHOTO, "Photo");
    photo.source = Some(rgba8_source(extent, pattern));
    doc.layers.insert(1, photo);
    doc.reference_layers = [PHOTO].into();
    doc
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
    let r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let (input, consumer) = input_queue(256);
    let extent = [doc.width, doc.height];
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
        .append_layer_operation(TARGET, LayerOperation {
            placement: layer_core::Affine::IDENTITY,
            coverage: LayerMask::reveal_all(LayerId(20), Point::default()),
            kind: LayerOperationKind::Fill { color: FILL, alpha_locked: false },
        })
        .unwrap();
    flush(&mut engine);
    (input, engine)
}

fn pen(sequence: u64, phase: PenPhase, [x, y]: [f32; 2], flags: SampleFlags) -> PenEvent {
    PenEvent {
        device_id: 1,
        sequence,
        timestamp_ns: sequence * 8_000_000,
        view_revision: 0,
        surface_position: Point { x, y },
        pressure: 1.,
        tilt_radians: [0.; 2],
        twist_radians: 0.,
        distance: 0.,
        phase,
        tool: ToolKind::Pen,
        flags,
    }
}

fn draw(engine: &mut CanvasEngine<WgpuRasterizer>, input: &mut InputProducer<PenEvent>, event: PenEvent) {
    input.push(event).unwrap();
    engine.render_frame().unwrap();
}

fn floats(bytes: &[u8]) -> Vec<[f32; 4]> {
    bytes
        .chunks_exact(16)
        .map(|p| std::array::from_fn(|i| f32::from_le_bytes(p[i * 4..i * 4 + 4].try_into().unwrap())))
        .collect()
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
fn current_and_below_lays_the_target_at_its_opacity_over_the_references() {
    for space in layer_core::BlendSpace::ALL {
        current_and_below_in(space);
    }
}
fn current_and_below_in(space: layer_core::BlendSpace) {
    let mut doc = document(SIZE);
    doc.blend_space = space;
    let (mut input, mut engine) = engine(doc, false);
    engine.set_layer_opacity(TARGET, 0.5).unwrap();
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

    engine.apply_edit(layer_core::Edit::SetReferences(Default::default())).unwrap();
    flush(&mut engine);
    draw(&mut engine, &mut input, pen(3, PenPhase::Down, [600., 400.], SampleFlags::PRIMARY));
    let (alone, _) = sample(engine.backend_mut(), [0, 0], [0.; 2]);
    assert!(alone.iter().all(|p| close(*p, premultiplied(FILL))), "with nothing marked the target alone is the source");
    draw(&mut engine, &mut input, pen(4, PenPhase::Up, [600., 400.], SampleFlags::PRIMARY));
    flush(&mut engine);
}

fn settle(r: &mut WgpuRasterizer, layers: &[Layer]) {
    for _ in 0..200 {
        r.submit(crate::test_support::packet(layers, [2560, 2560])).unwrap();
        if !r.has_pending_work() {
            return;
        }
    }
    panic!("prefetch never settled");
}

#[test]
fn the_reference_cache_follows_its_frame_and_evicts_the_least_recent_page() {
    let doc = document([2560, 2560]);
    let mut layers = doc.layers.clone();
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    r.prepare_retouch(Some(&RetouchPreparation {
        target: TARGET,
        retouch: Retouch { source: RetouchSource::References, references: Arc::new([PHOTO].into()), ..Retouch::default() },
        points: vec![Point { x: 10., y: 10. }],
    }));
    settle(&mut r, &layers);
    let retouch = |r: &WgpuRasterizer| r.retouch.as_ref().unwrap().counts;
    let cached = |r: &WgpuRasterizer| r.retouch.as_ref().unwrap().cached_pages();
    assert_eq!(retouch(&r).captures, 9, "the ring around the focus point, clipped to the canvas");
    assert_eq!(cached(&r), 9);
    let (source, complete) = sample(&mut r, [1, 1], [0.; 2]);
    assert!(complete);
    assert!(close(source[5 * 256 + 7], reference(263, 261)));
    assert_eq!(retouch(&r).captures, 9, "cached pages are reused");

    layers[0].opacity = 0.25;
    settle(&mut r, &layers);
    assert_eq!(retouch(&r).captures, 9, "changing the target keeps the cache");

    layers[1].opacity = 0.5;
    settle(&mut r, &layers);
    assert_eq!(retouch(&r).captures, 18, "changing a reference captures again");
    let (dimmed, _) = sample(&mut r, [1, 1], [0.; 2]);
    assert!(close(dimmed[5 * 256 + 7], reference(263, 261).map(|v| v * 0.5)));

    layers[1].source = Some(rgba8_source([2560, 2560], |x, y| pattern(y, x)));
    settle(&mut r, &layers);
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
    settle(&mut r, &layers);
    let bytes = r.retouch.as_ref().unwrap().storage_bytes();
    let page = u64::from(PAGE_SIZE * PAGE_SIZE) * 16;
    assert_eq!(bytes, (16 + REFERENCE_PAGES as u64) * page + PARAMETER_BYTES, "idle sources keep only their pages");
    assert_eq!(r.metrics().retouch_storage_bytes, bytes);
    r.set_telemetry_enabled(true);
    assert!(r.telemetry().resident_bytes >= bytes);
    r.prepare_retouch(None);
    r.submit(crate::test_support::packet(&layers, [2560, 2560])).unwrap();
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
        let mut doc = document([1536, 512]);
        doc.reference_layers.insert(LayerId(2));
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
    let estimated = SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::ESTIMATED.0);
    let path = [[80., 90.], [130., 110.], [190., 100.], [250., 140.]];
    let corrected = |mut event: PenEvent| {
        event.pressure = 0.4;
        event.surface_position.y += 6.;
        event
    };
    let mut results = Vec::new();
    for late in [false, true] {
        let (mut input, mut engine) = engine(document(SIZE), true);
        engine.set_retouch(Some(RetouchSource::Editing));
        flush(&mut engine);
        let events: Vec<_> = path
            .iter()
            .enumerate()
            .map(|(i, &at)| {
                let phase = if i == 0 { PenPhase::Down } else { PenPhase::Move };
                pen(1 + i as u64, phase, at, if late { estimated } else { SampleFlags::PRIMARY })
            })
            .collect();
        for (i, event) in events.iter().enumerate() {
            draw(&mut engine, &mut input, if late { *event } else { corrected(*event) });
            if late && i == 1 {
                let mut fix = corrected(events[0]);
                fix.flags = SampleFlags(SampleFlags::CORRECTION.0 | SampleFlags::ESTIMATED.0);
                draw(&mut engine, &mut input, fix);
            }
        }
        draw(&mut engine, &mut input, pen(9, PenPhase::Up, [260., 140.], SampleFlags::PRIMARY));
        flush(&mut engine);
        if late {
            for event in &events {
                let mut fix = corrected(*event);
                fix.flags = SampleFlags::CORRECTION;
                draw(&mut engine, &mut input, fix);
            }
            flush(&mut engine);
        }
        let (source, complete) = sample(engine.backend_mut(), [0, 0], [0.; 2]);
        assert!(complete);
        assert!(source.iter().all(|p| close(*p, premultiplied(FILL))), "late={late}: replays copy the restored start");
        results.push(target_pages(engine.backend()));
    }
    assert_eq!(results[0], results[1], "corrections replay to the direct stroke");
}

/// CPU time of each engine frame, and its GPU time from timestamps.
fn timed_frame(engine: &mut CanvasEngine<WgpuRasterizer>, samples: &mut Vec<[f64; 2]>) {
    let before = engine.backend().telemetry().gpu.count;
    let start = std::time::Instant::now();
    engine.render_frame().unwrap();
    let cpu = start.elapsed().as_secs_f64() * 1000.;
    engine.backend_mut().wait_idle().unwrap();
    let gpu = engine.backend().telemetry().gpu;
    let latest = gpu.ordered().last().copied().filter(|_| gpu.count > before).map_or(f64::NAN, f64::from);
    samples.push([cpu, latest]);
}

fn summary(label: &str, samples: &[[f64; 2]]) {
    let stat = |i: usize| {
        let mut v: Vec<f64> = samples.iter().map(|s| s[i]).filter(|v| v.is_finite()).collect();
        v.sort_by(f64::total_cmp);
        let at = |q: f64| v.get(((v.len() as f64 - 1.) * q).round() as usize).copied().unwrap_or(f64::NAN);
        format!("p50 {:.3} p99 {:.3} max {:.3}", at(0.5), at(0.99), at(1.))
    };
    eprintln!("{label}: {} frames; CPU ms {}; GPU ms {}", samples.len(), stat(0), stat(1));
}

#[test]
#[ignore = "hardware 24-megapixel retouch source benchmark; release, serial"]
fn retouch_source_frame_cost() {
    let extent = [6000, 4000];
    let mut doc = document(extent);
    doc.reference_layers.insert(LayerId(2));
    let zoom = 1600. / 6000.;
    let fit = ViewState { width_px: 1600, height_px: 1067, document_to_surface: [zoom, 0., 0., zoom, 0., 0.], background_rgba_linear: [0.; 4] };
    let open = |doc: &Document| {
        let r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        let (input, consumer) = input_queue(1024);
        let transform = ViewTransform { revision: 0, surface_to_document: [1. / zoom, 0., 0., 1. / zoom, 0., 0.] };
        let mut engine = CanvasEngine::new(r, doc.clone(), consumer, fit, transform).unwrap();
        engine
            .set_brush(BrushSnapshot { diameter: 300., hardness: 1., flow: 1., mappings: Arc::from([]), ..Default::default() })
            .unwrap();
        engine.set_instant_feedback(InstantFeedbackConfig { enabled: false, ..Default::default() }).unwrap();
        engine.backend_mut().set_telemetry_enabled(true);
        flush(&mut engine);
        for _ in 0..5 {
            timed_frame(&mut engine, &mut Vec::new());
        }
        (input, engine)
    };

    let (_input, mut engine) = open(&doc);
    let mut still = Vec::new();
    for _ in 0..20 {
        timed_frame(&mut engine, &mut still);
    }
    summary("still frames", &still);
    engine.set_retouch(Some(RetouchSource::References));
    engine.set_retouch_points(&[Point { x: 1500., y: 1500. }, Point { x: 4200., y: 2600. }]);
    let mut prefetch = Vec::new();
    while engine.backend().has_pending_work() {
        timed_frame(&mut engine, &mut prefetch);
    }
    summary("prefetch frames", &prefetch);
    eprintln!("prefetched pages {}, blocking {}", counts(&engine).captures, counts(&engine).blocking);

    doc.active_layer = PHOTO;
    for retouch in [None, Some(RetouchSource::Editing)] {
        let (mut input, mut engine) = open(&doc);
        engine.set_retouch(retouch);
        flush(&mut engine);
        let mut stroke = Vec::new();
        for i in 0..90u64 {
            let t = i as f32 / 89.;
            let at = [(400. + 5200. * t) * zoom, (1200. + 1600. * (t * 6.).sin().abs()) * zoom];
            let phase = match i { 0 => PenPhase::Down, 89 => PenPhase::Up, _ => PenPhase::Move };
            input.push(pen(1 + i, phase, at, SampleFlags::PRIMARY)).unwrap();
            timed_frame(&mut engine, &mut stroke);
        }
        summary(&format!("{} stroke frames on the photo", if retouch.is_some() { "retouch" } else { "plain" }), &stroke);
        if let Some(sources) = &engine.backend().retouch {
            eprintln!("stroke-start copies {}, pool pages {}", sources.counts.copies, sources.pool.len());
        }
    }
}
