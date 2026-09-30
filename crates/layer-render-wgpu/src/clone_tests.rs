//! Clone oracles through the real engine: what each stroke copies, bit for bit
//! where the brush fully covers a pixel.
use super::*;
use layer_core::{CloneSource, DefaultBrushPreset, Edit, Selection, color::source::rgba8_source, default_brush};

const EXTENT: [u32; 2] = [768, 512];

fn clone_brush() -> BrushSnapshot {
    BrushSnapshot {
        color_rgba_linear: [0., 0., 0., 1.],
        opacity: 1.,
        diameter: 48.,
        hardness: 1.,
        mappings: Arc::from([]),
        ..default_brush(DefaultBrushPreset::CloneStamp)
    }
}

/// A Clone engine painting on `target`, the photo itself or the layer above
/// it, prepared as the session prepares a selected Clone tool.
fn cloner(doc: Document, target: LayerId, source: RetouchSource, feedback: bool) -> (InputProducer<PenEvent>, CanvasEngine<WgpuRasterizer>) {
    let mut doc = doc;
    doc.active_layer = target;
    let extent = [doc.width, doc.height];
    let r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let (input, consumer) = input_queue(256);
    let mut engine = CanvasEngine::new(r, doc, consumer, view(extent), ViewTransform::IDENTITY).unwrap();
    engine.set_brush(clone_brush()).unwrap();
    engine.set_instant_feedback(InstantFeedbackConfig { enabled: feedback, ..Default::default() }).unwrap();
    engine.set_retouch(Some(source));
    flush(&mut engine);
    (input, engine)
}

/// What a clone reads at every pixel of the top page row, with no offset.
fn source_row(r: &mut WgpuRasterizer, rows: std::ops::Range<u32>) -> impl Fn(u32, u32) -> [f32; 4] + use<> {
    let pages: Vec<Vec<Vec<[f32; 4]>>> =
        rows.clone().map(|y| (0..EXTENT[0] / PAGE_SIZE).map(|x| sample(r, [x, y], [0.; 2]).0).collect()).collect();
    let first = rows.start;
    move |x, y| pages[(y / PAGE_SIZE - first) as usize][(x / PAGE_SIZE) as usize][((y % PAGE_SIZE) * PAGE_SIZE + x % PAGE_SIZE) as usize]
}

fn layer_page(r: &WgpuRasterizer, layer: LayerId, page: [u32; 2]) -> Vec<[f32; 4]> {
    r.paint_layers
        .iter()
        .find(|l| l.id == layer)
        .and_then(|l| l.pages.iter().find(|p| p.coordinate == page))
        .map_or_else(|| vec![[0.; 4]; (PAGE_SIZE * PAGE_SIZE) as usize], |p| floats(&crate::layer_tests::page_bytes(r, &p.active().texture)))
}

fn pixel(r: &WgpuRasterizer, layer: LayerId, x: u32, y: u32) -> [f32; 4] {
    layer_page(r, layer, [x / PAGE_SIZE, y / PAGE_SIZE])[((y % PAGE_SIZE) * PAGE_SIZE + x % PAGE_SIZE) as usize]
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

fn source_at(engine: &mut CanvasEngine<WgpuRasterizer>, x: f32, y: f32, update: impl FnOnce(&mut CloneSource)) {
    let mut source = CloneSource::default();
    update(&mut source);
    source.set(Point { x, y });
    engine.set_clone_source(source);
}

/// Pixels a 48 px stroke along `y` from `x0` to `x1` covers fully.
fn interior(x0: u32, x1: u32, y: u32) -> impl Iterator<Item = (u32, u32)> {
    (x0 + 10..x1 - 10).step_by(3).flat_map(move |x| (y - 20..=y + 20).step_by(4).map(move |y| (x, y)))
}

fn offset_of(engine: &CanvasEngine<WgpuRasterizer>) -> [i64; 2] {
    let offset = engine.clone_source().offset.unwrap();
    assert!(offset.iter().all(|v| v.fract() == 0.), "{offset:?}");
    offset.map(|v| v as i64)
}

fn shifted(x: u32, y: u32, offset: [i64; 2]) -> (u32, u32) {
    ((x as i64 + offset[0]) as u32, (y as i64 + offset[1]) as u32)
}

#[test]
fn an_integer_offset_clones_the_source_exactly_and_a_second_pass_sees_the_first() {
    for (target, source) in [(PHOTO, RetouchSource::Editing), (TARGET, RetouchSource::References)] {
        let (mut input, mut engine) = cloner(document(EXTENT), target, source, false);
        let original = source_row(engine.backend_mut(), 0..2);
        source_at(&mut engine, 191., 121., |_| {});
        stroke(&mut engine, &mut input, 1, [60., 64.], [300., 64.]);
        let first = offset_of(&engine);
        assert_eq!(first, [131, 57]);
        for (x, y) in interior(60, 300, 64) {
            let (sx, sy) = shifted(x, y, first);
            assert_eq!(pixel(engine.backend(), target, x, y), original(sx, sy), "{source:?} ({x},{y})");
        }

        source_at(&mut engine, 80., 64., |s| s.aligned = false);
        stroke(&mut engine, &mut input, 20, [80., 300.], [280., 300.]);
        let second = [0, -236];
        for (x, y) in interior(80, 280, 300) {
            let (bx, by) = shifted(x, y, second);
            let (sx, sy) = shifted(bx, by, first);
            assert_eq!(pixel(engine.backend(), target, x, y), original(sx, sy), "{source:?} second pass ({x},{y})");
        }
    }
}

/// A stroke's coverage across its width, where it runs straight, is one
/// continuous stamp with the brush's linear edge to within a code, hard or
/// soft; stamps laid a spacing apart scallop instead.
#[test]
fn clone_strokes_lay_one_continuous_stamp() {
    let mut doc = document(EXTENT);
    doc.layers.iter_mut().find(|l| l.id == PHOTO).unwrap().source = Some(rgba8_source(EXTENT, |_, _| [255; 4]));
    let radius = 48.;
    let deviation = |brush: BrushSnapshot| {
        let (mut input, mut engine) = cloner(doc.clone(), TARGET, RetouchSource::References, false);
        engine.set_brush(brush.clone()).unwrap();
        source_at(&mut engine, 110., 230., |_| {});
        stroke(&mut engine, &mut input, 1, [100., 200.], [668., 200.]);
        let edge = (1. - brush.hardness).max(1. / radius);
        let pages: Vec<_> = (0..3).map(|x| layer_page(engine.backend(), TARGET, [x, 0])).collect();
        let mut worst = 0f32;
        for x in 200..568u32 {
            for y in 140..256u32 {
                let rho = ((y as f32 + 0.5 - 200.).abs() / radius).min(1.);
                let ideal = ((1. - rho) / edge).clamp(0., 1.);
                let got = pages[(x / PAGE_SIZE) as usize][(y * PAGE_SIZE + x % PAGE_SIZE) as usize][3];
                worst = worst.max((got - ideal).abs());
            }
        }
        worst
    };
    for hardness in [1., 0.5] {
        let swept = BrushSnapshot { hardness, diameter: 2. * radius, ..clone_brush() };
        let stamped = BrushSnapshot { contact: None, ..swept.clone() };
        let (swept, stamped) = (deviation(swept), deviation(stamped));
        assert!(swept <= 1. / 255., "hardness {hardness}: {swept}");
        assert!(stamped > swept, "hardness {hardness}: {stamped} {swept}");
    }
}

#[test]
fn a_flipped_source_mirrors_about_the_source_point() {
    let (mut input, mut engine) = cloner(document(EXTENT), TARGET, RetouchSource::References, false);
    let original = source_row(engine.backend_mut(), 0..2);
    source_at(&mut engine, 500., 100., |s| s.flip = [true, false]);
    stroke(&mut engine, &mut input, 1, [60., 64.], [300., 64.]);
    let [ox, oy] = offset_of(&engine);
    assert_eq!([ox, oy], [560, 36]);
    for (x, y) in interior(60, 300, 64) {
        let source = ((ox - x as i64 - 1) as u32, (y as i64 + oy) as u32);
        assert_eq!(pixel(engine.backend(), TARGET, x, y), original(source.0, source.1), "({x},{y})");
    }
    source_at(&mut engine, 500., 400., |s| s.flip = [false, true]);
    stroke(&mut engine, &mut input, 20, [60., 200.], [300., 200.]);
    let [ox, oy] = offset_of(&engine);
    for (x, y) in interior(60, 300, 200) {
        let source = ((x as i64 + ox) as u32, (oy - y as i64 - 1) as u32);
        assert_eq!(pixel(engine.backend(), TARGET, x, y), original(source.0, source.1), "vertical ({x},{y})");
    }
}

#[test]
fn clone_strokes_stay_inside_the_selection_and_keep_alpha_locked_transparency() {
    let (mut input, mut engine) = cloner(document(EXTENT), TARGET, RetouchSource::References, false);
    let original = source_row(engine.backend_mut(), 0..1);
    let rect = |x0: f32, x1: f32| {
        Selection::polygon([[x0, 0.], [x1, 0.], [x1, 512.], [x0, 512.]].map(|[x, y]| Point { x, y }).to_vec()).unwrap()
    };
    engine.apply_edit(Edit::SetSelection(Some(rect(100., 200.)))).unwrap();
    source_at(&mut engine, 191., 121., |_| {});
    stroke(&mut engine, &mut input, 1, [60., 64.], [300., 64.]);
    let offset = offset_of(&engine);
    for (x, y) in interior(60, 300, 64) {
        let got = pixel(engine.backend(), TARGET, x, y);
        if (102..198).contains(&x) {
            let (sx, sy) = shifted(x, y, offset);
            assert_eq!(got, original(sx, sy), "inside ({x},{y})");
        } else if !(97..=203).contains(&x) {
            assert_eq!(got, [0.; 4], "outside ({x},{y})");
        }
    }

    let (mut input, mut engine) = cloner(document(EXTENT), TARGET, RetouchSource::References, false);
    let mut coverage = LayerMask::reveal_all(LayerId(20), Point::default());
    coverage.default_coverage = 0.;
    coverage.initial = Some(rect(0., 150.));
    engine
        .append_layer_operation(TARGET, LayerOperation {
            placement: layer_core::Affine::IDENTITY,
            coverage,
            kind: LayerOperationKind::Fill { color: FILL, alpha_locked: false },
        })
        .unwrap();
    let mut locked = engine.document().layer(TARGET).unwrap().clone();
    locked.properties.alpha_locked = true;
    engine.apply_edit(Edit::ReplaceLayer(Box::new(locked))).unwrap();
    flush(&mut engine);
    source_at(&mut engine, 191., 121., |_| {});
    let read = source_row(engine.backend_mut(), 0..1);
    let before = pixel(engine.backend(), TARGET, 20, 20);
    assert!(before[3] > 0.4 && before[3] < 0.6);
    stroke(&mut engine, &mut input, 1, [60., 64.], [300., 64.]);
    let offset = offset_of(&engine);
    for (x, y) in interior(60, 300, 64) {
        let got = pixel(engine.backend(), TARGET, x, y);
        if x < 148 {
            let (sx, sy) = shifted(x, y, offset);
            let source = read(sx, sy);
            let expected = [0, 1, 2].map(|i| source[i] / source[3] * before[3]);
            assert_eq!(got[3], before[3], "alpha lock keeps coverage at ({x},{y})");
            assert!(got.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-5), "({x},{y}): {got:?} != {expected:?}");
        } else if x > 152 {
            assert_eq!(got, [0.; 4], "alpha lock keeps ({x},{y}) transparent");
        }
    }
}

#[test]
fn aligned_strokes_keep_one_offset_and_others_restart_at_the_source() {
    for aligned in [true, false] {
        let (mut input, mut engine) = cloner(document(EXTENT), TARGET, RetouchSource::References, false);
        let original = source_row(engine.backend_mut(), 0..2);
        source_at(&mut engine, 300., 100., |s| s.aligned = aligned);
        stroke(&mut engine, &mut input, 1, [60., 64.], [200., 64.]);
        stroke(&mut engine, &mut input, 20, [60., 200.], [200., 200.]);
        let second = if aligned { [240, 36] } else { [240, -100] };
        for (x, y) in interior(60, 200, 200) {
            let (sx, sy) = shifted(x, y, second);
            assert_eq!(pixel(engine.backend(), TARGET, x, y), original(sx, sy), "aligned={aligned} ({x},{y})");
        }
    }
}

#[test]
fn clone_replays_from_estimates_and_corrections_match_a_direct_stroke() {
    let estimated = SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::ESTIMATED.0);
    let path = [[80., 90.], [350., 90.], [650., 140.], [650., 380.], [80., 380.], [80., 90.]];
    let corrected = |mut event: PenEvent| {
        event.pressure = 0.4;
        event.surface_position.y += 6.;
        event
    };
    let mut results = Vec::new();
    for late in [false, true] {
        let (mut input, mut engine) = cloner(document(EXTENT), TARGET, RetouchSource::References, true);
        source_at(&mut engine, 400., 300., |_| {});
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
        draw(&mut engine, &mut input, pen(9, PenPhase::Up, [80., 96.], SampleFlags::PRIMARY));
        flush(&mut engine);
        if late {
            for event in &events {
                let mut fix = corrected(*event);
                fix.flags = SampleFlags::CORRECTION;
                draw(&mut engine, &mut input, fix);
            }
            flush(&mut engine);
        }
        results.push(target_pages(engine.backend()));
    }
    assert!(results[0].iter().any(|(_, page)| page.iter().any(|p| p[3] > 0.)), "the clone painted");
    assert_eq!(results[0], results[1], "corrections replay to the direct stroke");
}
