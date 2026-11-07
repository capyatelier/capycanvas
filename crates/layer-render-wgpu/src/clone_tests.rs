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
fn cloner(doc: Document, target: SourceTarget, source: RetouchSource, feedback: bool) -> (InputProducer<PenEvent>, CanvasEngine<WgpuRasterizer>) {
    let mut doc = doc;
    let owner = doc.target_owner(target).unwrap();
    let edit = doc.select_occurrence_edit(owner).unwrap();
    doc.apply(edit).unwrap();
    let extent = doc.composition().size;
    let r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
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
    let SourceTarget::Paint(photo) = PHOTO else { unreachable!() };
    doc.artwork.paint.get_mut(photo).unwrap().base = Some(layer_core::authored::PaintBase::new((rgba8_source(EXTENT, |_, _| [255; 4])).into()));
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
    let mut working = engine.document().working.clone();
    working.selection = Some(rect(100., 200.));
    engine.apply_edit(Edit::Working(working)).unwrap();
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
    let mut coverage = reveal_all(EXTENT);
    coverage.source.default_coverage = 0.;
    coverage.selection = Some(rect(0., 150.));
    engine
        .append_raster_operation(TARGET, RasterOperation {
            placement: layer_core::Affine::IDENTITY,
            coverage,
            kind: RasterOperationKind::Fill { edges: None, underfill: None, texture: Default::default(), color: FILL, alpha_locked: false },
        })
        .unwrap();
    let edit = occurrence_edit(engine.document(), TARGET_USE, |o| o.alpha_locked = true);
    engine.apply_edit(edit).unwrap();
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
    let path = [[80., 90.], [350., 90.], [650., 140.], [650., 380.], [80., 380.], [80., 90.]];
    let mut results = Vec::new();
    for late in [false, true] {
        let (mut input, mut engine) = cloner(document(EXTENT), TARGET, RetouchSource::References, true);
        source_at(&mut engine, 400., 300., |_| {});
        corrected_replay(&mut engine, &mut input, &path, [80., 96.], late);
        results.push(target_pages(engine.backend()));
    }
    assert!(results[0].iter().any(|(_, page)| page.iter().any(|p| p[3] > 0.)), "the clone painted");
    assert_eq!(results[0], results[1], "corrections replay to the direct stroke");
}

/// The ink layer over an image layer that is a reference: one rotated,
/// smoothly sampled photo at a fractional position, in a float drawing.
fn image_reference_document() -> (Document, OccurrenceHandle) {
    let mut doc = Document::new(PortableId::random(), EXTENT[0], EXTENT[1],
        layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().color.depth = layer_core::color::SampleDepth::F32;
    let mut object = layer_core::ImageObject::new(rgba8_source([320, 240], pattern).into());
    object.affine = layer_core::Affine64([1.2, 0.35, -0.35, 1.2, 140.25, 20.5]);
    let (layer, edit) = doc.create_object_layer_edit("Images", object, None, 1).unwrap();
    doc.apply(edit).unwrap();
    let edit = occurrence_edit(&doc, layer, |o| o.reference = true);
    doc.apply(edit).unwrap();
    (doc, layer)
}

/// Clone reads an image layer reference through its canonical pixels: zooming
/// the view out changes nothing the stroke copies, the result matches cloning
/// from the same layer rasterized, and the strokes land on the paint layer.
#[test]
fn clone_copies_reference_image_layers_by_their_canonical_pixels_at_any_zoom() {
    let cloned = |doc: Document, zoom: f32| {
        let (mut input, mut engine) = cloner(doc, TARGET, RetouchSource::References, false);
        engine.set_view(ViewState { document_to_surface: [zoom, 0., 0., zoom, 0., 0.], ..view(EXTENT) }, ViewTransform::IDENTITY);
        flush(&mut engine);
        source_at(&mut engine, 300., 160., |_| {});
        stroke(&mut engine, &mut input, 1, [60., 70.], [420., 90.]);
        let pages: Vec<_> = (0..2).map(|x| layer_page(engine.backend(), TARGET, [x, 0])).collect();
        (pages, engine)
    };
    let (doc, layer) = image_reference_document();
    let (native, engine) = cloned(doc.clone(), 1.);
    assert!(native.iter().flatten().any(|pixel| pixel[3] > 0.99 && pixel[..3].iter().any(|v| *v > 0.05)), "the stroke copied the image");
    let scene = engine.document().scene();
    assert_eq!(scene.object_layer(layer), doc.scene().object_layer(layer), "the image layer is only read");
    assert_eq!(engine.document().working.target, Some(TARGET));
    drop(engine);
    let (reduced, _) = cloned(doc.clone(), 0.25);
    for (index, (a, b)) in native.iter().flatten().zip(reduced.iter().flatten()).enumerate() {
        assert_eq!(a, b, "pixel {index} depends on the view's zoom");
    }

    let (mut input, mut engine) = cloner(doc, TARGET, RetouchSource::References, false);
    let plan = engine.document().rasterize_plan(layer, false).unwrap();
    let capture = plan.image_capture(EXTENT, None).unwrap();
    assert!(engine.backend_mut().request_snapshot(layer_render::SnapshotRequest::Image(capture)).unwrap());
    let image = loop {
        if let Some(result) = engine.backend_mut().take_snapshot() { break result.unwrap(); }
        std::thread::yield_now();
    };
    let layer_render::SnapshotResult::Image(image) = image else { panic!("captured image") };
    let edits = plan.with_image(image.map(|(samples, origin)| (layer_core::Image::new(samples), origin))).unwrap();
    engine.insert_with_operations(edits, Vec::new(), None).unwrap();
    assert_eq!(engine.document().working.target, engine.document().scene().source_target(layer), "rasterizing selects the new paint");
    let edit = engine.document().select_occurrence_edit(TARGET_USE).unwrap();
    engine.apply_edit(edit).unwrap();
    engine.set_retouch(Some(RetouchSource::References));
    flush(&mut engine);
    source_at(&mut engine, 300., 160., |_| {});
    stroke(&mut engine, &mut input, 1, [60., 70.], [420., 90.]);
    let rasterized: Vec<_> = (0..2).map(|x| layer_page(engine.backend(), TARGET, [x, 0])).collect();
    for (index, (a, b)) in native.iter().flatten().zip(rasterized.iter().flatten()).enumerate() {
        assert!(close(*a, *b), "pixel {index}: image layer {a:?}, rasterized {b:?}");
    }
}

/// Smudge mixes only its own paint: on an image layer it is refused and leaves
/// the images unchanged, and above one it never picks up the image's pixels.
#[test]
fn smudge_keeps_to_paint_and_never_writes_image_layers() {
    let smudge = || BrushSnapshot { diameter: 48., mappings: Arc::from([]), ..default_brush(DefaultBrushPreset::Smudge) };
    let (doc, layer) = image_reference_document();
    let mut on_images = doc.clone();
    let edit = on_images.select_occurrence_edit(layer).unwrap();
    on_images.apply(edit).unwrap();
    assert_eq!(on_images.try_drawing_target(), Err(layer_core::DrawingRefusal::Object));
    let (mut input, mut engine) = cloner(doc, TARGET, RetouchSource::Editing, false);
    engine.set_retouch(None);
    engine.set_brush(smudge()).unwrap();
    flush(&mut engine);
    stroke(&mut engine, &mut input, 1, [160., 80.], [400., 120.]);
    assert!((0..2).flat_map(|x| layer_page(engine.backend(), TARGET, [x, 0])).all(|pixel| pixel[3] == 0.),
        "smudging empty paint above the image picks up nothing from it");
    assert!(engine.document().scene().object_layer(layer).is_some());
}

fn resident_code_cloner(depth: layer_core::color::SampleDepth) -> (
    InputProducer<PenEvent>, CanvasEngine<WgpuRasterizer>, Arc<layer_core::color::source::SourceImage>,
) {
    let extent = [1024, 768];
    let source = crate::test_support::depth_source(extent, depth, layer_core::color::RgbSpace::Srgb, 16 << 20,
        |x, y| [(x % 251) as f32 / 250., (y % 241) as f32 / 240., 0.37, 1.]);
    let mut doc = document(extent);
    doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().color.depth = depth;
    let SourceTarget::Paint(photo) = PHOTO else { unreachable!() };
    doc.artwork.paint.get_mut(photo).unwrap().base = Some(layer_core::authored::PaintBase::new(source.clone().into()));
    let (input, mut engine) = cloner(doc, PHOTO, RetouchSource::Editing, false);
    let r = engine.backend_mut();
    crate::test_support::complete(r);
    let (decoded_grant, upload_grant, mip_grant) = {
        let mut sources = r.source_tiles.borrow_mut();
        let [decoded, upload] = sources.admitted_bytes();
        let grants = (decoded + sources.non_slot_bytes(), upload, sources.mip_budget());
        sources.require_images(std::iter::empty());
        let overhead = sources.non_slot_bytes();
        sources.admit_limits(overhead, upload, grants.2);
        grants
    };
    while r.source_tiles.borrow().gpu_bytes() > r.source_tiles.borrow().non_slot_bytes() {
        let encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        assert!(r.source_tiles.borrow_mut().trim_unused(&encoder) > 0, "completed fixture sources must remain evictable");
        encoder.submit(&r.queue);
        crate::test_support::complete(r);
    }
    assert!(r.source_tiles.borrow().prepared_view(&source, [1, 1]).is_none());
    r.source_tiles.borrow_mut().admit_limits(decoded_grant, upload_grant, mip_grant);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    for coordinate in page_coordinates(PixelRect::full(extent)) {
        let prepared = crate::prepare_image_tile(&source, coordinate, r.document_color().space, &r.device.source_samples, None).unwrap();
        crate::scene::Scene::decode_prepared(r, &mut encoder, &source, coordinate, prepared).unwrap();
    }
    r.uploads.finish(&encoder); encoder.submit(&r.queue); crate::test_support::complete(r);
    (input, engine, source)
}

fn resident_code_format(depth: layer_core::color::SampleDepth) -> wgpu::TextureFormat {
    if depth == layer_core::color::SampleDepth::U8 { wgpu::TextureFormat::Rgba8Unorm } else { wgpu::TextureFormat::Rg32Uint }
}

fn prepared_photo_view(r: &WgpuRasterizer, source: &Arc<layer_core::color::source::SourceImage>, coordinate: [u32; 2]) -> wgpu::TextureView {
    r.source_tiles.borrow().prepared_view(source, coordinate).unwrap().clone()
}

fn assert_clone_promotes_resident_codes_once(depth: layer_core::color::SampleDepth) {
    let (mut input, mut engine, source) = resident_code_cloner(depth);
    let original = prepared_photo_view(engine.backend(), &source, [1, 1]);
    let untouched = prepared_photo_view(engine.backend(), &source, [3, 0]);
    assert_eq!(original.texture().format(), resident_code_format(depth));
    assert_eq!(untouched.texture().format(), resident_code_format(depth));
    let lease = engine.backend().source_tiles.borrow().lease(&original).unwrap();
    source_at(&mut engine, 384., 384., |_| {});
    draw(&mut engine, &mut input, pen(1, PenPhase::Down, [64., 64.], SampleFlags::PRIMARY));
    assert_eq!(offset_of(&engine), [320, 320]);
    let exact = prepared_photo_view(engine.backend(), &source, [1, 1]);
    assert_eq!(exact.texture().format(), wgpu::TextureFormat::Rgba32Float);
    assert_ne!(exact, original);
    assert!(Arc::ptr_eq(&lease, &engine.backend().source_tiles.borrow().lease(&exact).unwrap()));
    for (sequence, x) in [(2, 68.), (3, 72.), (4, 80.)] {
        draw(&mut engine, &mut input, pen(sequence, PenPhase::Move, [x, 64.], SampleFlags::PRIMARY));
        assert_eq!(prepared_photo_view(engine.backend(), &source, [1, 1]), exact);
        assert_eq!(prepared_photo_view(engine.backend(), &source, [3, 0]), untouched);
        let r = engine.backend_mut();
        let mut retouch = r.retouch_sources();
        let mapping = retouch.mapping(r, &Gather { region: PixelRect::new(64, 64, 81, 65), scale: [1.; 2], offset: [320.; 2], stroke: None }).unwrap();
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let (_, complete) = retouch.pages(r, &mapping, &mut encoder).unwrap();
        assert!(complete);
        assert_eq!(encoder.pass_count(), 0, "warm clone sources cannot normalize again");
        assert!(Arc::ptr_eq(&lease, &r.source_tiles.borrow().lease(&exact).unwrap()));
        r.retouch = Some(retouch);
    }
    draw(&mut engine, &mut input, pen(5, PenPhase::Up, [80., 64.], SampleFlags::PRIMARY));
    flush(&mut engine);
    assert_eq!(prepared_photo_view(engine.backend(), &source, [1, 1]), exact);
    assert_eq!(prepared_photo_view(engine.backend(), &source, [3, 0]).texture().format(), resident_code_format(depth));
    let maximum = depth.maximum() as f32;
    for (x, y) in [(68, 64), (72, 64)] {
        let code = [((x + 320) % 251) as f32 / 250., ((y + 320) % 241) as f32 / 240., 0.37];
        let expected = code.map(|value| layer_core::color::RgbSpace::Srgb.decode(f64::from((value * maximum).round() / maximum)) as f32);
        let actual = pixel(engine.backend(), PHOTO, x, y);
        assert!((actual[3] - 1.).abs() < 1e-6);
        for channel in 0..3 { assert!((actual[channel] - expected[channel]).abs() < 2e-5, "{depth:?}: {actual:?}, {expected:?}"); }
    }
}

#[test]
fn clone_stroke_promotes_resident_u8_once_and_keeps_untouched_codes() {
    assert_clone_promotes_resident_codes_once(layer_core::color::SampleDepth::U8);
}

#[test]
fn clone_stroke_promotes_resident_u16_once_and_keeps_untouched_codes() {
    assert_clone_promotes_resident_codes_once(layer_core::color::SampleDepth::U16);
}

#[test]
fn retouch_source_promotion_keeps_the_ready_display_window() {
    for depth in [layer_core::color::SampleDepth::U8, layer_core::color::SampleDepth::U16] {
        let (_input, mut engine, source) = resident_code_cloner(depth);
        let r = engine.backend_mut();
        let encoded = prepared_photo_view(r, &source, [1, 1]);
        assert_eq!(encoded.texture().format(), resident_code_format(depth));
        let window = r.scale_display.as_ref().map(|cache| cache.view().clone());
        let document = crate::test_support::document_texture(r).clone();
        let composed = r.metrics.composited_pixels;
        let mut retouch = r.retouch_sources();
        let mapping = retouch.mapping(r, &Gather { region: PixelRect::new(64, 64, 81, 65), scale: [1.; 2], offset: [320.; 2], stroke: None }).unwrap();
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let (_, complete) = retouch.pages(r, &mapping, &mut encoder).unwrap();
        assert!(complete);
        assert_eq!(prepared_photo_view(r, &source, [1, 1]).texture().format(), wgpu::TextureFormat::Rgba32Float);
        assert_eq!(r.scale_display.as_ref().map(|cache| cache.view().clone()), window);
        assert_eq!(crate::test_support::document_texture(r), &document);
        assert_eq!(r.metrics.composited_pixels, composed);
        r.retouch = Some(retouch);
        r.uploads.finish(&encoder); encoder.submit(&r.queue);
        engine.render_frame().unwrap();
        assert!(engine.backend().metrics.frame_composited_pages.is_empty(), "promotion alone cannot invalidate display windows");
    }
}
