use super::*;
use crate::test_support::{packet, float_pixels, max_error};
use layer_core::authored::vector::*;
use layer_core::color::RgbColor;
use crate::scene::Output;

fn paint(rgba: [f32; 4]) -> Paint { Paint::color(RgbColor::new(Default::default(), rgba).unwrap()) }
fn stroke(id: u64, points: &[[f64; 2]], width: f64, rgba: [f32; 4]) -> Record {
    let anchors: Vec<_> = points.iter().map(|p| Anchor::corner(*p)).collect();
    let mut stroke = Stroke::new(paint(rgba), width);
    stroke.cap = Cap::Round; stroke.join = Join::Round;
    Record::Path(PathItem::new(ItemId(id), Arc::new(PathGeometry::new(vec![Subpath::new(&anchors, false)]).unwrap()), Arc::new(Style::new(vec![PaintEntry::Stroke(stroke)]))))
}
fn shape(id: u64, rect: [f64; 4], rgba: [f32; 4]) -> Record {
    let [x0, y0, x1, y1] = rect;
    let anchors = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(Anchor::corner);
    Record::Path(PathItem::new(ItemId(id), Arc::new(PathGeometry::new(vec![Subpath::new(&anchors, true)]).unwrap()),
        Arc::new(Style::new(vec![PaintEntry::Fill(Fill { paint: paint(rgba), unknown: None })]))))
}
fn document(extent: [u32; 2], records: Vec<Record>) -> (layer_core::Editor, OccurrenceHandle) {
    let artwork = layer_core::authored::Artwork::new(extent).unwrap();
    let mut editor = layer_core::Editor::new(layer_core::Document::from_artwork(artwork).unwrap());
    let (layer, edit) = editor.document().create_vector_layer_edit("Ink", VectorObject::from_records(records), None, 0).unwrap();
    editor.perform(edit).unwrap();
    (editor, layer)
}
fn records() -> Vec<Record> {
    let mut list: Vec<Record> = (0..40).map(|i| stroke(i + 1, &[[10. + i as f64 * 7., 20.], [60. + i as f64 * 5., 150.], [20. + i as f64 * 6., 190.]], 2. + (i % 4) as f64, [0.05, 0.1, 0.3, 1.])).collect();
    list.push(shape(100, [40., 60., 260., 120.], [0.9, 0.5, 0.2, 0.8]));
    list
}
fn submit_ready(r: &mut WgpuRasterizer, frame: FramePacket<'_>) {
    crate::test_support::submit_ready(r, frame).unwrap();
    for _ in 0..200 {
        r.wait_idle().unwrap();
        if !r.has_pending_work() { return; }
        crate::test_support::submit_ready(r, FramePacket { composite_all: false, reset_layers: false, ..frame }).unwrap();
    }
    panic!("vector display must settle");
}
fn native(r: &mut WgpuRasterizer, doc: &layer_core::Document) -> Vec<[f32; 4]> {
    let extent = doc.composition().size;
    let (texture, _) = create_color_target(&r.device, extent, "vector native reference");
    let mut scene = r.scene.take().unwrap_or_else(|| Scene::new(r).unwrap());
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    scene.capture_region_prepared(r, packet(doc.scene(), extent), &texture, PixelRect::full(extent), Output::Artwork(None), &mut encoder).unwrap();
    r.uploads.finish(&encoder); encoder.submit(&r.queue);
    r.scene = Some(scene);
    float_pixels(r, &texture)
}
fn translation(offset: [f64; 2]) -> layer_core::authored::Affine64 { layer_core::authored::Affine64([1., 0., 0., 1., offset[0], offset[1]]) }
fn display(r: &WgpuRasterizer) -> Vec<[f32; 4]> { float_pixels(r, r.scale_display.as_ref().unwrap().texture()) }
fn engine_for(doc: layer_core::Document) -> layer_engine::CanvasEngine<WgpuRasterizer> {
    let r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let view = crate::test_support::view(doc.composition().size);
    layer_engine::CanvasEngine::new(r, doc, layer_engine::input_queue(16).1, view, layer_engine::ViewTransform::IDENTITY).unwrap()
}
fn settle(engine: &mut layer_engine::CanvasEngine<WgpuRasterizer>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        engine.render_frame().unwrap();
        if !engine.has_pending_document_edits() && !engine.backend().has_pending_work() { return; }
        assert!(std::time::Instant::now() < deadline, "the bake must finish");
        std::thread::yield_now();
    }
}

#[test]
fn pressure_peaks_and_small_eraser_gaps_survive_gpu_rendering() {
    for cap in [Cap::Round, Cap::Butt, Cap::Square] {
        let Record::Path(mut path) = stroke(1, &[[30., 70.], [250., 70.]], 40., [0., 0., 0., 1.]) else { unreachable!() };
        if let PaintEntry::Stroke(stroke) = &mut Arc::make_mut(&mut path.style).entries[0] {
            stroke.cap = cap;
            stroke.profile = Some(Arc::new(WidthProfile::new(vec![WidthKnot::new(0, 0., 0.1, 0.1), WidthKnot::new(0, 0.5, 1., 1.), WidthKnot::new(0, 1., 0.1, 0.1)]).unwrap()));
        }
        let (editor, _) = document([280, 140], vec![Record::Path(path.clone())]);
        let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
        let pixels = native(&mut r, editor.document());
        assert!(pixels[85 * 280 + 140][3] > 0.95, "{cap:?}: the pressure peak remains visible: {:?}", pixels[85 * 280 + 140]);
        assert!(pixels[85 * 280 + 35][3] < 0.01, "{cap:?}: the thin end remains thin");
        let pieces = path.erased(&Eraser { from: [140., 70.], to: [140., 70.], radius: 4. }, None).unwrap();
        let (cut, _) = document([280, 140], pieces.into_iter().enumerate().map(|(i, p)| Record::Path(PathItem { id: ItemId(i as u64 + 1), ..p })).collect());
        let pixels = native(&mut r, cut.document());
        assert!(pixels[70 * 280 + 140][3] < 0.01, "{cap:?}: a small eraser clears visible ink");
        assert!(pixels[70 * 280 + 70][3] > 0.95 && pixels[70 * 280 + 210][3] > 0.95);
        submit_ready(&mut r, packet(cut.document().scene(), [280, 140]));
        assert!(max_error(&pixels, &display(&r)) < 1e-5);
        if let PaintEntry::Stroke(stroke) = &mut Arc::make_mut(&mut path.style).entries[0] { stroke.profile = None; }
        let pieces = path.erased(&Eraser { from: [140., 85.], to: [140., 85.], radius: 4. }, None).unwrap();
        let (cut, _) = document([280, 140], pieces.into_iter().enumerate().map(|(i, p)| Record::Path(PathItem { id: ItemId(i as u64 + 1), ..p })).collect());
        let pixels = native(&mut r, cut.document());
        assert!(pixels[85 * 280 + 140][3] < 0.01, "{cap:?}: erasing near the edge clears the rendered cap");
        for x in [132, 148] { assert!(pixels[85 * 280 + x][3] > 0.95, "{cap:?}: nearby ink at {x} remains"); }
    }
    let Record::Path(mut taper) = stroke(1, &[[140., 70.], [150., 70.]], 100., [0., 0., 0., 1.]) else { unreachable!() };
    if let PaintEntry::Stroke(stroke) = &mut Arc::make_mut(&mut taper.style).entries[0] {
        stroke.profile = Some(Arc::new(WidthProfile::new(vec![WidthKnot::new(0, 0., 0., 0.), WidthKnot::new(0, 1., 1., 1.)]).unwrap()));
    }
    let (editor, _) = document([280, 140], vec![Record::Path(taper)]);
    let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    assert!(native(&mut r, editor.document())[70 * 280 + 115][3] > 0.95);
}

#[test]
fn vector_layers_render_the_same_through_display_and_native_paths() {
    let (editor, _) = document([300, 200], records());
    let doc = editor.document();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    submit_ready(&mut r, packet(doc.scene(), doc.composition().size));
    let shown = display(&r);
    let exact = native(&mut r, doc);
    let covered = exact.iter().filter(|p| p[3] > 0.5).count();
    eprintln!("covered {covered}, display vs native {}", max_error(&shown, &exact));
    assert!(covered > 5000, "vector content must be visible");
    assert!(max_error(&shown, &exact) < 1e-5);
}

#[test]
fn raw_object_captures_of_vector_layers_match_their_render() {
    let (editor, layer) = document([300, 200], records());
    let doc = editor.document();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let exact = native(&mut r, doc);
    let (texture, _) = create_color_target(&r.device, [300, 200], "vector raw capture");
    let mut scene = r.scene.take().unwrap_or_else(|| Scene::new(&r).unwrap());
    let scope = layer_core::SceneScope::RawObjects(layer);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    scene.capture_region_prepared(&mut r, FramePacket { scene: doc.scene().with_scope(&scope), ..packet(doc.scene(), [300, 200]) }, &texture, PixelRect::full([300, 200]), Output::Objects(layer), &mut encoder).unwrap();
    r.uploads.finish(&encoder); encoder.submit(&r.queue);
    r.scene = Some(scene);
    let raw = float_pixels(&r, &texture);
    assert!(raw.iter().filter(|p| p[3] > 0.5).count() > 5000);
    assert!(max_error(&raw, &exact) < 1e-5, "Rasterize Layer bakes what the layer shows: {}", max_error(&raw, &exact));
}

#[test]
fn vector_edits_damage_only_changed_items_and_match_a_fresh_render() {
    let (mut editor, layer) = document([300, 200], records());
    let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    submit_ready(&mut r, packet(editor.document().scene(), editor.document().composition().size));
    let before = editor.document().clone();
    let object = editor.document().scene().vector_layer(layer).unwrap().clone();
    let drawn = object.insert_front(vec![stroke(500, &[[250., 10.], [290., 40.]], 4., [1., 0., 0., 1.])]);
    editor.perform(editor.document().vector_object_edit(layer, drawn).unwrap()).unwrap();
    let damage = objects::edited_damage(before.scene(), editor.document().scene(), [300, 200]).expect("vector edits must not composite everything");
    let area: u64 = damage.regions.iter().map(|r| u64::from(r.width()) * u64::from(r.height())).sum();
    eprintln!("drawing damage {area} px over {:?}", damage.regions);
    assert!(area < 300 * 200 / 4, "a new stroke damages only its own bounds");
    submit_ready(&mut r, FramePacket { composite_all: true, ..packet(editor.document().scene(), [300, 200]) });
    let incremental = display(&r);
    let mut fresh = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    submit_ready(&mut fresh, packet(editor.document().scene(), [300, 200]));
    assert!(max_error(&incremental, &display(&fresh)) < 1e-5);
    let target = editor.document().scene().vector_layer(layer).unwrap().find(ItemId(7)).unwrap();
    let recolored = editor.document().scene().vector_layer(layer).unwrap().replace([(target, Some(stroke(7, &[[59., 20.], [95., 150.], [62., 190.]], 5., [0., 0.8, 0., 1.])))]);
    let before = editor.document().clone();
    editor.perform(editor.document().vector_object_edit(layer, recolored).unwrap()).unwrap();
    assert!(objects::edited_damage(before.scene(), editor.document().scene(), [300, 200]).is_some());
    submit_ready(&mut r, FramePacket { composite_all: true, ..packet(editor.document().scene(), [300, 200]) });
    let mut fresh = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    submit_ready(&mut fresh, packet(editor.document().scene(), [300, 200]));
    assert!(max_error(&display(&r), &display(&fresh)) < 1e-5);
}

#[test]
fn provisional_strokes_draw_over_the_cached_layer_and_hand_off_without_change() {
    use layer_render::CanvasRenderer;
    let (mut editor, layer) = document([300, 200], records());
    let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    submit_ready(&mut r, packet(editor.document().scene(), [300, 200]));
    let before = display(&r);
    let Record::Path(path) = stroke(900, &[[30., 30.], [150., 90.], [280., 40.]], 6., [0.9, 0.1, 0.1, 1.]) else { unreachable!() };
    let path = Arc::new(path);
    r.set_vector_preview(Some(layer_render::VectorPreview { owner: layer, path: path.clone(), revision: 1 }));
    submit_ready(&mut r, FramePacket { composite_all: false, ..packet(editor.document().scene(), [300, 200]) });
    let drawn = display(&r);
    let changed = drawn.iter().zip(&before).filter(|(a, b)| (a[0] - b[0]).abs() > 0.01).count();
    eprintln!("preview changed {changed} pixels");
    assert!(changed > 500, "the provisional stroke must show");
    let object = editor.document().scene().vector_layer(layer).unwrap().insert_front(vec![Record::Path((*path).clone())]);
    editor.perform(editor.document().vector_object_edit(layer, object).unwrap()).unwrap();
    r.set_vector_preview(None);
    submit_ready(&mut r, FramePacket { composite_all: true, ..packet(editor.document().scene(), [300, 200]) });
    assert!(max_error(&display(&r), &drawn) < 1e-5, "committing the stroke must look the same as its preview");
}

fn timed(r: &mut WgpuRasterizer, frame: FramePacket<'_>) -> (f64, f64) {
    let start = std::time::Instant::now();
    r.submit(frame).unwrap();
    let cpu = start.elapsed().as_secs_f64() * 1e3;
    r.wait_idle().unwrap();
    (cpu, start.elapsed().as_secs_f64() * 1e3)
}
fn percentile(values: &mut [f64], p: f64) -> f64 { values.sort_by(f64::total_cmp); values[((values.len() - 1) as f64 * p) as usize] }

#[test]
#[ignore = "crowded vector layer drawing benchmark"]
fn crowded_vector_layer_drawing_frames() {
    use layer_render::CanvasRenderer;
    let extent = [4000u32, 3000u32];
    let zoom: f32 = std::env::var("VECTOR_ZOOM").ok().and_then(|v| v.parse().ok()).unwrap_or(0.4);
    let view = layer_render::ViewState { width_px: 1920, height_px: 1200, document_to_surface: [zoom, 0., 0., zoom, 160. - 1200. * (zoom - 0.4).max(0.), -1300. * (zoom - 0.4).max(0.)] };
    for count in [10_000u64, 50_000] {
        let records: Vec<Record> = crate::vector::bench::scene_b([4000., 3000.], count).into_iter().map(Record::Path).collect();
        let (mut editor, layer) = document(extent, records);
        let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
        let start = std::time::Instant::now();
        let mut opening = vec![timed(&mut r, FramePacket { view, ..packet(editor.document().scene(), extent) }).1];
        while r.has_pending_work() { opening.push(timed(&mut r, FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) }).1); }
        let open = start.elapsed().as_secs_f64() * 1e3;
        eprintln!("open over {} frames, longest wall {:.1} ms", opening.len(), opening.iter().copied().fold(0., f64::max));
        let initial = display(&r);
        let mut frames = Vec::new();
        let mut points = Vec::new();
        let loop_start = std::time::Instant::now();
        for i in 0..120u64 {
            points.push([1200. + i as f64 * 9., 1400. + (i as f64 * 0.1).sin() * 300.]);
            let anchors: Vec<_> = points.iter().map(|p| Anchor::corner(*p)).collect();
            let mut stroke = Stroke::new(paint([0.8, 0.1, 0.1, 1.]), 6.);
            stroke.cap = Cap::Round; stroke.join = Join::Round;
            let path = PathItem::new(ItemId(u64::MAX - 1), Arc::new(PathGeometry::new(vec![Subpath::new(&anchors, false)]).unwrap()), Arc::new(Style::new(vec![PaintEntry::Stroke(stroke)])));
            r.set_vector_preview(Some(layer_render::VectorPreview { owner: layer, path: Arc::new(path), revision: i + 1 }));
            frames.push(timed(&mut r, FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) }));
            let mut extra = 0;
            while r.has_pending_work() && extra < 50 { r.submit(FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) }).unwrap(); r.wait_idle().unwrap(); extra += 1; }
            assert!(extra < 50, "drawing frames settle");
        }
        eprintln!("120 drawing frames took {:.1} ms; changed {} pixels", loop_start.elapsed().as_secs_f64() * 1e3, display(&r).iter().zip(&initial).filter(|(a, b)| (a[0] - b[0]).abs() > 0.05).count());
        let Some(layer_render::VectorPreview { path, .. }) = r.vector_preview.clone() else { unreachable!() };
        let object = editor.document().scene().vector_layer(layer).unwrap().insert_front(vec![Record::Path(PathItem { id: ItemId(u64::MAX - 2), ..(*path).clone() })]);
        let start = std::time::Instant::now();
        editor.perform(editor.document().vector_object_edit(layer, object).unwrap()).unwrap();
        let perform = start.elapsed().as_secs_f64() * 1e3;
        r.set_vector_preview(None);
        let commit = timed(&mut r, FramePacket { view, composite_all: true, ..packet(editor.document().scene(), extent) });
        let start = std::time::Instant::now();
        editor.undo().unwrap();
        let undo_perform = start.elapsed().as_secs_f64() * 1e3;
        let undo = timed(&mut r, FramePacket { view, composite_all: true, ..packet(editor.document().scene(), extent) });
        let mut small = Vec::new();
        for k in 0..5u64 {
            let object = editor.document().scene().vector_layer(layer).unwrap().insert_front(vec![stroke(70_000 + k, &[[1500. + k as f64 * 40., 1500.], [1510. + k as f64 * 40., 1505.]], 4., [0.2, 0.7, 0.2, 1.])]);
            editor.perform(editor.document().vector_object_edit(layer, object).unwrap()).unwrap();
            small.push(timed(&mut r, FramePacket { view, composite_all: true, ..packet(editor.document().scene(), extent) }).1);
        }
        eprintln!("small commits wall {:?}", small.iter().map(|v| format!("{v:.2}")).collect::<Vec<_>>());
        let (mut cpu, mut wall): (Vec<f64>, Vec<f64>) = frames.into_iter().unzip();
        eprintln!("{count} strokes: open {open:.0} ms; drawing frame cpu p50 {:.2} p95 {:.2} ms, wall p50 {:.2} p95 {:.2} ms; commit perform {perform:.2} ms frame cpu {:.2} wall {:.2} ms; undo perform {undo_perform:.2} ms frame cpu {:.2} wall {:.2} ms",
            percentile(&mut cpu, 0.5), percentile(&mut cpu, 0.95), percentile(&mut wall, 0.5), percentile(&mut wall, 0.95), commit.0, commit.1, undo.0, undo.1);
    }
}

#[test]
fn crowded_layers_derive_across_frames_and_settle_to_the_exact_render() {
    let extent = [2000u32, 1500u32];
    let records: Vec<Record> = crate::vector::bench::scene_b([2000., 1500.], 20_000).into_iter().map(Record::Path).collect();
    let (editor, _) = document(extent, records);
    let doc = editor.document();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.submit(packet(doc.scene(), extent)).unwrap();
    r.wait_idle().unwrap();
    let derived = |r: &WgpuRasterizer| r.scene.as_ref().unwrap().vectors.layers.values().flat_map(|layer| layer.items.iter().flatten()).filter(|item| !item.derived.is_empty()).count();
    let first = derived(&r);
    let opened = r.metrics.composited_pixels;
    assert!(r.has_pending_work() && first < 20_000, "one frame derived {first} of 20000 paths");
    submit_ready(&mut r, FramePacket { composite_all: false, ..packet(doc.scene(), extent) });
    let settled = r.metrics.composited_pixels - opened;
    assert!(settled <= opened + opened / 2, "frames that only derive composite nothing until the window is complete: {settled} after {opened}");
    let shown = display(&r);
    let exact = native(&mut r, doc);
    assert!(max_error(&shown, &exact) < 1e-5, "the settled display must match the exact render: {}", max_error(&shown, &exact));
}

#[test]
fn moved_and_removed_items_repaint_only_damaged_tiles_and_match_a_fresh_render() {
    use std::collections::BTreeSet;
    let extent = [1200u32, 900u32];
    for zoom in [1f32, 0.5] {
        let view = layer_render::ViewState { width_px: 600, height_px: 450, document_to_surface: [zoom, 0., 0., zoom, 0., 0.] };
        let records: Vec<Record> = crate::vector::bench::scene_b([1200., 900.], 2000).into_iter().map(Record::Path).collect();
        let (mut editor, layer) = document(extent, records);
        let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
        submit_ready(&mut r, FramePacket { view, ..packet(editor.document().scene(), extent) });
        let matches = |r: &WgpuRasterizer, editor: &layer_core::Editor, step: &str| {
            let mut fresh = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
            submit_ready(&mut fresh, FramePacket { view, ..packet(editor.document().scene(), extent) });
            let error = max_error(&display(r), &display(&fresh));
            assert!(error < 1e-5, "{step} at zoom {zoom} must match a fresh render: {error}");
        };
        let object = editor.document().scene().vector_layer(layer).unwrap().clone();
        let moving: BTreeSet<ItemId> = object.paths().step_by(97).map(|(_, p)| p.id).collect();
        let moved = object.transform_items(&moving, layer_core::authored::Affine64([1., 0., 0., 1., 37., -21.]), true).unwrap();
        editor.perform(editor.document().vector_object_edit(layer, moved).unwrap()).unwrap();
        submit_ready(&mut r, FramePacket { view, composite_all: true, ..packet(editor.document().scene(), extent) });
        matches(&r, &editor, "moving items");
        let object = editor.document().scene().vector_layer(layer).unwrap().clone();
        let removed = object.replace(object.paths().step_by(131).map(|(position, _)| (position, None)).collect::<Vec<_>>());
        editor.perform(editor.document().vector_object_edit(layer, removed).unwrap()).unwrap();
        submit_ready(&mut r, FramePacket { view, composite_all: true, ..packet(editor.document().scene(), extent) });
        matches(&r, &editor, "removing items");
        let object = editor.document().scene().vector_layer(layer).unwrap().insert_front(vec![stroke(9100, &[[100., 300.], [1100., 500.]], 9., [0.9, 0.2, 0.1, 0.5])]);
        editor.perform(editor.document().vector_object_edit(layer, object).unwrap()).unwrap();
        submit_ready(&mut r, FramePacket { view, composite_all: true, ..packet(editor.document().scene(), extent) });
        submit_ready(&mut r, FramePacket { view, composite_all: true, ..packet(editor.document().scene(), extent) });
        matches(&r, &editor, "an append followed by an unchanged frame");
    }
}

fn derivation_work(r: &WgpuRasterizer, since: u64, known: &mut HashSet<usize>) -> (usize, usize) {
    let (mut derived, mut batched) = (0, 0);
    for layer in r.scene.as_ref().unwrap().vectors.layers.values() {
        for item in layer.items.iter().flatten() {
            batched += usize::from(item.derived.iter().any(|d| d.used > since));
            derived += item.derived.iter().filter(|d| known.insert(Arc::as_ptr(&d.geometry) as usize)).count();
        }
    }
    (derived, batched)
}

#[test]
fn crossing_a_level_shows_the_previous_one_resampled_and_refines_a_few_pages_a_frame() {
    let extent = [1200u32, 900u32];
    let records: Vec<Record> = crate::vector::bench::scene_b([1200., 900.], 5000).into_iter().map(Record::Path).collect();
    let (editor, _) = document(extent, records);
    let at = |zoom: f32| layer_render::ViewState { width_px: 600, height_px: 450, document_to_surface: [zoom, 0., 0., zoom, 300. - 600. * zoom, 225. - 450. * zoom] };
    for (from, to) in [(0.4f32, 0.6f32), (0.9, 0.45)] {
        let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
        submit_ready(&mut r, FramePacket { view: at(from), ..packet(editor.document().scene(), extent) });
        let level = r.scale_display.as_ref().unwrap().plan.level;
        let mut fresh = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
        submit_ready(&mut fresh, FramePacket { view: at(to), ..packet(editor.document().scene(), extent) });
        let expected = presented(&fresh, at(to));
        r.submit(FramePacket { view: at(to), composite_all: false, ..packet(editor.document().scene(), extent) }).unwrap();
        r.wait_idle().unwrap();
        assert_ne!(r.scale_display.as_ref().unwrap().plan.level, level, "the zoom from {from} to {to} crosses a level");
        let pages = r.metrics.frame_composited_pages.len();
        assert!(pages <= scale::APPROXIMATE_PAGES, "the first frame at {to} renders {pages} pages exactly");
        let resampled = presented(&r, at(to));
        let error = resampled.iter().zip(&expected).map(|(a, b)| f64::from(a.abs_diff(*b))).sum::<f64>() / expected.len() as f64;
        assert!(error < 16., "the level at {from} stands in for the one at {to}: mean byte error {error}");
        let mut frames = 0;
        while r.has_pending_work() {
            r.submit(FramePacket { view: at(to), composite_all: false, ..packet(editor.document().scene(), extent) }).unwrap();
            r.wait_idle().unwrap();
            assert!(r.metrics.frame_composited_pages.len() <= scale::APPROXIMATE_PAGES + 4, "refinement stays within its budget");
            frames += 1;
            assert!(frames < 500, "refinement finishes");
        }
        assert!(max_error(&display(&r), &display(&fresh)) < 1e-5, "the refined level at {to} matches a fresh render");
    }
}

#[test]
fn frame_work_after_small_edits_and_drags_stays_flat_from_ten_to_fifty_thousand_paths() {
    use std::collections::BTreeSet;
    let view = layer_render::ViewState { width_px: 800, height_px: 600, document_to_surface: [1., 0., 0., 1., 0., 0.] };
    let rows: Vec<Vec<(&str, usize, usize)>> = [10_000u64, 50_000].into_iter().map(|count| {
        let columns = (count as f64).sqrt().ceil() as u64;
        let at = |i: u64| [(i % columns) as f64 * 12. + 4., (i / columns) as f64 * 12. + 4.];
        let records = (0..count).map(|i| { let [x, y] = at(i); stroke(i + 1, &[[x, y], [x + 7., y + 9.]], 2., [0.1, 0.1, 0.1, 1.]) }).collect();
        let side = (columns * 12 + 16) as u32;
        let extent = [side, side];
        let (mut editor, layer) = document(extent, records);
        let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
        submit_ready(&mut r, FramePacket { view, ..packet(editor.document().scene(), extent) });
        let mut known = HashSet::new();
        derivation_work(&r, 0, &mut known);
        let mut rows = Vec::new();
        let mut step = |r: &mut WgpuRasterizer, editor: &layer_core::Editor, name: &'static str| {
            let since = r.scene.as_ref().unwrap().vectors.frame;
            submit_ready(r, FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) });
            let (derived, batched) = derivation_work(r, since, &mut known);
            rows.push((name, derived, batched));
        };
        step(&mut r, &editor, "idle frame");
        let object = editor.document().scene().vector_layer(layer).unwrap().clone();
        let target = ItemId(5 * columns + 6);
        let position = object.find(target).unwrap();
        let Some(Record::Path(path)) = object.record(position).cloned() else { unreachable!() };
        let mut ink = Stroke::new(paint([0.9, 0.1, 0.1, 1.]), 2.);
        (ink.cap, ink.join) = (Cap::Round, Join::Round);
        let recolored = path.with_style(Arc::new(Style::new(vec![PaintEntry::Stroke(ink)])));
        editor.perform(editor.document().vector_object_edit(layer, object.replace([(position, Some(Record::Path(recolored)))])).unwrap()).unwrap();
        step(&mut r, &editor, "recolor one path");
        editor.undo().unwrap();
        step(&mut r, &editor, "undo");
        editor.redo().unwrap();
        step(&mut r, &editor, "redo");
        let near: Vec<ItemId> = (0..count).filter(|i| { let [x, y] = at(*i); x < 400. && y < 300. }).map(|i| ItemId(i + 1)).collect();
        for (name, moving) in [("drag one path", vec![target]), ("drag 500 paths", near[..500].to_vec())] {
            let items: Arc<BTreeSet<ItemId>> = Arc::new(moving.into_iter().collect());
            for offset in [[3., 4.], [9., 7.]] {
                r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: items.clone(), transform: translation(offset) }));
                step(&mut r, &editor, name);
            }
            r.set_vector_motion(None);
            step(&mut r, &editor, "drag cancelled");
        }
        eprintln!("{count} paths: {rows:?}");
        rows
    }).collect();
    assert_eq!(rows[0], rows[1], "frame work must not grow with paths outside the view");
}

#[test]
fn frames_over_the_derived_budget_rescan_only_after_growth_or_a_stale_level() {
    let (mut editor, layer) = document([300, 200], records());
    let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    let at = |zoom: f32, editor: &layer_core::Editor, r: &mut WgpuRasterizer| submit_ready(r, FramePacket {
        view: layer_render::ViewState { width_px: 300, height_px: 200, document_to_surface: [zoom, 0., 0., zoom, 0., 0.] }, composite_all: false, ..packet(editor.document().scene(), [300, 200]) });
    let entries = |r: &WgpuRasterizer, level: u32| -> usize { r.scene.as_ref().unwrap().vectors.layers.values().flat_map(|layer| layer.items.iter().flatten()).map(|item| item.derived.iter().filter(|d| d.level == level).count()).sum() };
    at(1., &editor, &mut r);
    let drawn = entries(&r, 0);
    for layer in r.scene.as_mut().unwrap().vectors.layers.values_mut() { layer.derived_bytes += DERIVED_BYTES; }
    let draw = |i: u64, zoom: f32, editor: &mut layer_core::Editor, r: &mut WgpuRasterizer| {
        let object = editor.document().scene().vector_layer(layer).unwrap().insert_front(vec![stroke(1000 + i, &[[10. + i as f64 * 9., 5.], [14. + i as f64 * 9., 9.]], 2., [0.9, 0.1, 0.1, 1.])]);
        editor.perform(editor.document().vector_object_edit(layer, object).unwrap()).unwrap();
        at(zoom, editor, r);
    };
    for i in 0..4 { draw(i, 1., &mut editor, &mut r); }
    assert_eq!(entries(&r, 0), drawn + 4, "frames that derive little keep the unused entries of the level in use");
    for i in 4..8 { draw(i, 0.5, &mut editor, &mut r); }
    assert!(entries(&r, 1) > 0);
    assert_eq!(entries(&r, 0), 0, "a level no longer drawn is evicted");
}

#[test]
fn removing_or_dragging_a_lone_path_clears_tiles_no_other_path_covers() {
    use std::collections::BTreeSet;
    let records = vec![stroke(1, &[[40., 40.], [120., 60.]], 6., [0.9, 0.2, 0.1, 1.]), stroke(2, &[[200., 150.], [280., 170.]], 6., [0.1, 0.2, 0.9, 1.])];
    let (mut editor, layer) = document([300, 200], records);
    let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    submit_ready(&mut r, packet(editor.document().scene(), [300, 200]));
    let fresh = |editor: &layer_core::Editor| {
        let mut fresh = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
        submit_ready(&mut fresh, packet(editor.document().scene(), [300, 200]));
        display(&fresh)
    };
    let items: Arc<BTreeSet<ItemId>> = Arc::new([ItemId(1)].into());
    r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: items.clone(), transform: translation([30., 20.]) }));
    submit_ready(&mut r, FramePacket { composite_all: false, ..packet(editor.document().scene(), [300, 200]) });
    r.set_vector_motion(None);
    submit_ready(&mut r, FramePacket { composite_all: false, ..packet(editor.document().scene(), [300, 200]) });
    assert!(max_error(&display(&r), &fresh(&editor)) < 1e-5, "a cancelled drag restores the lone path");
    let object = editor.document().scene().vector_layer(layer).unwrap().clone();
    editor.perform(editor.document().vector_object_edit(layer, object.replace([(object.find(ItemId(1)).unwrap(), None)])).unwrap()).unwrap();
    submit_ready(&mut r, FramePacket { composite_all: true, ..packet(editor.document().scene(), [300, 200]) });
    assert!(max_error(&display(&r), &fresh(&editor)) < 1e-5, "removing the lone path clears its tiles");
}

#[test]
fn vector_layer_thumbnails_show_their_paths() {
    let (editor, layer) = document([2048, 1536], vec![stroke(1, &[[200., 700.], [1800., 760.]], 24., [0.2, 0.6, 0.2, 1.])]);
    let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    submit_ready(&mut r, packet(editor.document().scene(), [2048, 1536]));
    let inked = |r: &mut WgpuRasterizer| {
        for _ in 0..50 { if r.prepare_thumbnail_batch(layer_render::ThumbnailTarget::Occurrence(layer)).unwrap() { break; } crate::test_support::complete(r); }
        r.request_thumbnail(1, layer_render::ThumbnailTarget::Occurrence(layer)).unwrap();
        crate::test_support::complete(r);
        r.take_thumbnail().unwrap().unwrap().bytes.as_chunks::<4>().0.iter().filter(|p| p[3] > 128).count()
    };
    let shown = inked(&mut r);
    assert!(shown > 20, "the thumbnail shows the line: {shown} pixels");
    r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: Arc::new([ItemId(1)].into()), transform: translation([40., 30.]) }));
    submit_ready(&mut r, FramePacket { composite_all: false, ..packet(editor.document().scene(), [2048, 1536]) });
    assert_eq!(inked(&mut r), shown, "a drag in progress keeps its paths in the thumbnail");
    let mut editor = editor;
    let moved = editor.document().scene().vector_layer(layer).unwrap().transform_items(&[ItemId(1)].into(), layer_core::authored::Affine64([1., 0., 0., 1., 40., 30.]), true).unwrap();
    editor.perform(editor.document().vector_object_edit(layer, moved).unwrap()).unwrap();
    r.set_vector_motion(None);
    r.submit(FramePacket { composite_all: true, ..packet(editor.document().scene(), [2048, 1536]) }).unwrap();
    assert!(inked(&mut r) > 20, "the thumbnail after the move shows the line");
}

fn presented(r: &WgpuRasterizer, view: layer_render::ViewState) -> Vec<u8> {
    let (texture, target) = crate::create_target(r.device(), [view.width_px, view.height_px], wgpu::TextureFormat::Rgba8UnormSrgb, "vector motion presentation");
    let mut presenter = crate::present::ViewportPresenter::for_surface(r, wgpu::TextureFormat::Rgba8UnormSrgb, crate::SdrSurfaceColor::Srgb).unwrap();
    presenter.present(r, &target, view, [1.; 4]).unwrap();
    crate::layer_tests::page_bytes(r, &texture)
}
fn motion_fixture(blend: layer_core::LayerBlend, opacity: f32) -> (layer_core::Editor, OccurrenceHandle, Arc<std::collections::BTreeSet<ItemId>>) {
    let mut records: Vec<Record> = (0..6).map(|i| stroke(9000 + i, &[[200. + i as f64 * 60., 300.], [400. + i as f64 * 50., 500.]], 9., [0.9, 0.2, 0.1, 0.6])).collect();
    records.extend(crate::vector::bench::scene_b([1200., 900.], 1500).into_iter().map(Record::Path));
    let (mut editor, layer) = document([1200, 900], records);
    let mut occurrence = editor.document().scene().occurrence(layer).unwrap().clone();
    occurrence.opacity = opacity;
    editor.perform(layer_core::Edit::Occurrence(layer_core::RecordChange::replace(&editor.document().artwork.occurrences, layer, Some(occurrence)).unwrap())).unwrap();
    let cover = VectorObject::from_records(vec![shape(1, [150., 350., 700., 520.], [0.1, 0.3, 0.9, 0.5])]);
    let (upper, edit) = editor.document().create_vector_layer_edit("Above", cover, None, 0).unwrap();
    editor.perform(edit).unwrap();
    let mut occurrence = editor.document().scene().occurrence(upper).unwrap().clone();
    occurrence.blend = blend;
    editor.perform(layer_core::Edit::Occurrence(layer_core::RecordChange::replace(&editor.document().artwork.occurrences, upper, Some(occurrence)).unwrap())).unwrap();
    (editor, layer, Arc::new((9000..9006).map(ItemId).collect()))
}
fn committed(editor: &layer_core::Editor, layer: OccurrenceHandle, items: &std::collections::BTreeSet<ItemId>, transform: layer_core::authored::Affine64) -> layer_core::Editor {
    let moved_object = editor.document().scene().vector_layer(layer).unwrap().transform_items(items, transform, true).unwrap();
    let mut moved = layer_core::Editor::new(editor.document().clone());
    moved.perform(moved.document().vector_object_edit(layer, moved_object).unwrap()).unwrap();
    moved
}
#[test]
fn moving_paths_show_at_their_offset_under_other_layers_and_hand_off_to_the_committed_move() {
    let extent = [1200u32, 900u32];
    for (zoom, blend, opacity) in [(1f32, layer_core::LayerBlend::Normal, 1f32), (0.5, layer_core::LayerBlend::Normal, 1.), (1., layer_core::LayerBlend::Multiply, 1.), (0.5, layer_core::LayerBlend::Normal, 0.8)] {
        let view = layer_render::ViewState { width_px: 600, height_px: 450, document_to_surface: [zoom, 0., 0., zoom, 0., 0.] };
        let (mut editor, layer, items) = motion_fixture(blend, opacity);
        let split = blend == layer_core::LayerBlend::Normal && opacity == 1.;
        let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
        submit_ready(&mut r, FramePacket { view, ..packet(editor.document().scene(), extent) });
        let shown = |r: &WgpuRasterizer| if split { presented(r, view).into_iter().map(f32::from).collect::<Vec<_>>() } else { display(r).into_iter().flatten().collect() };
        let tolerance = if split { 1.5 } else { 1e-5 };
        let close = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0., f32::max);
        let original = shown(&r);
        let moved = committed(&editor, layer, &items, translation([38., -22.]));
        let mut fresh = WgpuRasterizer::new_native_headless(moved.document().composition().color).unwrap();
        submit_ready(&mut fresh, FramePacket { view, ..packet(moved.document().scene(), extent) });
        let expected = shown(&fresh);
        for offset in [[10., 6.], [24., -12.], [38., -22.]] {
            r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: items.clone(), transform: translation(offset) }));
            submit_ready(&mut r, FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) });
        }
        let error = close(&shown(&r), &expected);
        assert!(error < tolerance, "moving paths at zoom {zoom} under a {blend:?} layer must show where the move puts them: {error}");
        assert_eq!(max_error(&display(&r), &display(&fresh)) > 0.1, split, "drags under normal layers present moving paths without recompositing them");
        r.set_vector_motion(None);
        submit_ready(&mut r, FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) });
        assert!(close(&shown(&r), &original) < tolerance, "cancelling a move restores the layer at zoom {zoom} under a {blend:?} layer");
        r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: items.clone(), transform: translation([38., -22.]) }));
        submit_ready(&mut r, FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) });
        editor.perform(editor.document().vector_object_edit(layer, moved.document().scene().vector_layer(layer).unwrap().clone()).unwrap()).unwrap();
        r.set_vector_motion(None);
        submit_ready(&mut r, FramePacket { view, composite_all: true, ..packet(editor.document().scene(), extent) });
        let error = close(&shown(&r), &expected);
        assert!(error < tolerance, "committing the move looks like its preview at zoom {zoom} under a {blend:?} layer: {error}");
    }
}

#[test]
fn pressing_selected_paths_draws_nothing_their_selection_prepared_and_deselecting_releases_it() {
    let extent = [1200u32, 900u32];
    for zoom in [1f32, 0.5] {
        let view = layer_render::ViewState { width_px: 600, height_px: 450, document_to_surface: [zoom, 0., 0., zoom, 0., 0.] };
        let (editor, layer, items) = motion_fixture(layer_core::LayerBlend::Normal, 1.);
        let selection = layer_core::authored::VectorItemSelection { layer, items: items.clone() };
        let frame = FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) };
        let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
        submit_ready(&mut r, FramePacket { view, ..packet(editor.document().scene(), extent) });
        let idle = layer_render::CanvasRenderer::telemetry(&r).resident_bytes;
        r.set_vector_selection(Some(&selection));
        submit_ready(&mut r, frame);
        assert!(r.scene.as_ref().unwrap().vectors.motion.is_some(), "selecting paths prepares their moving image at zoom {zoom}");
        let shown = presented(&r, view);
        let (tiles, composited) = (r.metrics.vector_tiles, r.metrics.composited_pixels);
        r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: items.clone(), transform: translation([0.; 2]) }));
        submit_ready(&mut r, frame);
        assert_eq!(r.metrics.vector_tiles, tiles, "pressing prepared paths at zoom {zoom} draws no vector tiles");
        let bounds = super::motion_bounds(editor.document().scene(), layer, &items).in_frame(extent);
        assert!(r.metrics.composited_pixels - composited <= page_coordinates(bounds).count() as u64 * u64::from(PAGE_SIZE * PAGE_SIZE),
            "pressing recomposites only the pages the paths leave");
        assert!(presented(&r, view).iter().zip(&shown).all(|(a, b)| a.abs_diff(*b) <= 1), "pressing changes nothing on screen");
        let moved = committed(&editor, layer, &items, translation([38., -22.]));
        let mut fresh = WgpuRasterizer::new_native_headless(moved.document().composition().color).unwrap();
        submit_ready(&mut fresh, FramePacket { view, ..packet(moved.document().scene(), extent) });
        r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: items.clone(), transform: translation([38., -22.]) }));
        submit_ready(&mut r, frame);
        assert_eq!(r.metrics.vector_tiles, tiles, "dragging prepared paths draws no vector tiles");
        assert!(presented(&r, view).iter().zip(presented(&fresh, view)).all(|(a, b)| a.abs_diff(b) <= 1), "the prepared drag shows where the move puts the paths at zoom {zoom}");
        r.set_vector_motion(None);
        r.set_vector_selection(None);
        submit_ready(&mut r, frame);
        let vectors = &r.scene.as_ref().unwrap().vectors;
        assert!(vectors.motion.is_none(), "deselecting drops the moving image");
        let retained = vectors.spare_motion.as_ref().map(|image| image.texture.clone()).expect("the moving image's texture is kept for the next selection");
        let spare = r.scale_display.as_ref().unwrap().spare_branch_textures();
        assert!(!spare.is_empty(), "deselecting keeps the halves' images for the next selection at zoom {zoom}");
        let resident = layer_render::CanvasRenderer::telemetry(&r).resident_bytes;
        assert!(resident <= idle + texture_bytes(&retained) + spare.iter().map(texture_bytes).sum::<u64>(),
            "deselecting releases what the selection prepared but the textures kept for the next one: {resident} after {idle}");
        assert!(presented(&r, view).iter().zip(&shown).all(|(a, b)| a.abs_diff(*b) <= 1), "the cancelled drag restores the paths at zoom {zoom}");
        let fewer = Arc::new(items.iter().take(2).copied().collect::<std::collections::BTreeSet<_>>());
        r.set_vector_selection(Some(&layer_core::authored::VectorItemSelection { layer, items: fewer }));
        submit_ready(&mut r, frame);
        assert_eq!(r.scene.as_ref().unwrap().vectors.motion.as_ref().map(|motion| motion.image.texture.clone()), Some(retained),
            "selecting other paths draws their moving image into the kept texture instead of allocating one at zoom {zoom}");
        let branches = r.scale_display.as_ref().unwrap().branch_textures();
        assert!(spare.iter().all(|texture| branches.contains(texture)), "the other paths' halves reuse the kept images at zoom {zoom}");
    }
}

#[test]
fn crossing_a_level_with_selected_paths_prepares_a_few_pages_a_frame_and_keeps_the_moving_image() {
    use std::collections::BTreeSet;
    let extent = [1200u32, 900u32];
    let (editor, layer, _) = motion_fixture(layer_core::LayerBlend::Normal, 1.);
    let items: Arc<BTreeSet<ItemId>> = Arc::new(editor.document().scene().vector_layer(layer).unwrap().paths().map(|(_, path)| path.id).collect());
    let at = |zoom: f32| layer_render::ViewState { width_px: 600, height_px: 450, document_to_surface: [zoom, 0., 0., zoom, 0., 0.] };
    let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    submit_ready(&mut r, FramePacket { view: at(1.), ..packet(editor.document().scene(), extent) });
    r.set_vector_selection(Some(&layer_core::authored::VectorItemSelection { layer, items: items.clone() }));
    submit_ready(&mut r, FramePacket { view: at(1.), composite_all: false, ..packet(editor.document().scene(), extent) });
    let texture = |r: &WgpuRasterizer| r.scene.as_ref().unwrap().vectors.motion.as_ref().map(|motion| (motion.image.texture.clone(), motion.key.2));
    let (prepared, level) = texture(&r).expect("selecting prepares the moving image");
    let mut frames = 0;
    loop {
        let (pages, before) = (r.scale_display.as_ref().unwrap().prepared_pages(), texture(&r).unwrap());
        r.submit(FramePacket { view: at(0.5), composite_all: false, ..packet(editor.document().scene(), extent) }).unwrap();
        r.wait_idle().unwrap();
        let cache = r.scale_display.as_ref().unwrap();
        let (now, image) = (cache.prepared_pages(), texture(&r).unwrap());
        assert!(now.saturating_sub(pages) <= scale::APPROXIMATE_PAGES * 2, "frame {frames} prepares {} pages of the selection's halves", now.saturating_sub(pages));
        assert!(image.1 == level && image.0 == prepared || image.1 != level, "the moving image stays until its new level is ready");
        if image.1 != before.1 { assert_ne!(image.1, level); }
        frames += 1;
        if !r.has_pending_work() { break; }
        assert!(frames < 400, "preparation finishes");
    }
    assert!(frames > 2, "a level crossing spreads the selection's preparation over frames: {frames}");
    assert_ne!(texture(&r).unwrap().1, level, "the moving image reaches the new level");
    let extent = [4000u32, 3000u32];
    let (editor, layer) = crowded(extent, 2000);
    let items: Arc<BTreeSet<ItemId>> = Arc::new(editor.document().scene().vector_layer(layer).unwrap().paths().step_by(10).map(|(_, path)| path.id).collect());
    let view = layer_render::ViewState { width_px: 4000, height_px: 3000, document_to_surface: [1., 0., 0., 1., 0., 0.] };
    let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    r.set_vector_selection(Some(&layer_core::authored::VectorItemSelection { layer, items }));
    submit_ready(&mut r, FramePacket { view, ..packet(editor.document().scene(), extent) });
    let passes = r.metrics.command_passes;
    for _ in 0..3 { submit_ready(&mut r, FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) }); }
    assert_eq!(r.metrics.command_passes, passes, "selected paths whose halves the display cannot keep prepare nothing in later frames");
}

fn widened(paths: Vec<PathItem>, width: f64) -> Arc<[PathItem]> {
    paths.into_iter().map(|path| {
        let entries = path.style.entries.iter().map(|entry| match entry { PaintEntry::Stroke(stroke) => PaintEntry::Stroke(Stroke { width, ..stroke.clone() }), entry => entry.clone() }).collect();
        path.with_style(Arc::new(Style::new(entries)))
    }).collect()
}
#[test]
fn replacement_paths_redraw_only_the_moving_image_and_match_the_applied_edit() {
    let extent = [1200u32, 900u32];
    for (zoom, opacity) in [(1f32, 1f32), (0.5, 1.), (0.5, 0.8)] {
        let split = opacity == 1.;
        let view = layer_render::ViewState { width_px: 600, height_px: 450, document_to_surface: [zoom, 0., 0., zoom, 0., 0.] };
        let (mut editor, layer, items) = motion_fixture(layer_core::LayerBlend::Normal, opacity);
        let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
        fn frame(editor: &layer_core::Editor, view: layer_render::ViewState, extent: [u32; 2]) -> FramePacket<'_> { FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) } }
        submit_ready(&mut r, FramePacket { view, ..packet(editor.document().scene(), extent) });
        r.set_vector_selection(Some(&layer_core::authored::VectorItemSelection { layer, items: items.clone() }));
        submit_ready(&mut r, frame(&editor, view, extent));
        r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: items.clone(), transform: translation([0., 0.]) }));
        submit_ready(&mut r, frame(&editor, view, extent));
        let layer_display = display(&r);
        let object = editor.document().scene().vector_layer(layer).unwrap().clone();
        let shown = |r: &WgpuRasterizer| -> Vec<f32> { if split { presented(r, view).into_iter().map(|v| f32::from(v) / 255.).collect() } else { display(r).into_iter().flatten().collect() } };
        let close = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0., f32::max);
        let tolerance = if split { 2.5 / 255. } else { 1e-5 };
        for width in [12., 3., 16.] {
            let paths = widened(object.selected(&items), width);
            r.set_vector_motion_paths(Some(paths.clone()));
            let tiles = r.metrics.vector_tiles;
            r.submit(frame(&editor, view, extent)).unwrap();
            r.wait_idle().unwrap();
            assert!(r.metrics.vector_tiles > tiles, "a {width} px width preview redraws the moving image");
            if split {
                assert!(r.metrics.frame_composited_pages.is_empty(), "a {width} px width preview at zoom {zoom} recomposites no layer pages: {:?}", r.metrics.frame_composited_pages);
                assert!(!r.has_pending_work(), "a {width} px width preview is complete in its frame");
            }
            submit_ready(&mut r, frame(&editor, view, extent));
            let edited = object.with_paths(object.selected_paths(&items).into_iter().map(|(position, _)| position).zip(paths.iter().cloned())).unwrap();
            let mut applied = layer_core::Editor::new(editor.document().clone());
            applied.perform(applied.document().vector_object_edit(layer, edited).unwrap()).unwrap();
            let mut fresh = WgpuRasterizer::new_native_headless(applied.document().composition().color).unwrap();
            submit_ready(&mut fresh, FramePacket { view, ..packet(applied.document().scene(), extent) });
            let error = close(&shown(&r), &shown(&fresh));
            assert!(error <= tolerance, "a {width} px width preview at zoom {zoom}, opacity {opacity} shows what applying it draws: {error}");
        }
        assert!(!split || max_error(&display(&r), &layer_display) == 0., "the layer keeps its own paths until the edit is applied");
        let paths = widened(object.selected(&items), 16.);
        let edited = object.with_paths(object.selected_paths(&items).into_iter().map(|(position, _)| position).zip(paths.iter().cloned())).unwrap();
        editor.perform(editor.document().vector_object_edit(layer, edited).unwrap()).unwrap();
        r.set_vector_motion_paths(None);
        r.set_vector_motion(None);
        submit_ready(&mut r, FramePacket { view, composite_all: true, ..packet(editor.document().scene(), extent) });
        let mut fresh = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
        submit_ready(&mut fresh, FramePacket { view, ..packet(editor.document().scene(), extent) });
        assert!(close(&shown(&r), &shown(&fresh)) <= tolerance, "applying the edit at zoom {zoom}, opacity {opacity} hands off without change");
        assert!(r.scene.as_ref().unwrap().vectors.replacement_count() == 0, "applying the edit releases the replacement derivations");
    }
}

#[test]
fn replacement_paths_derive_only_changed_outlines_once() {
    let extent = [1200u32, 900u32];
    let view = layer_render::ViewState { width_px: 600, height_px: 450, document_to_surface: [0.5, 0., 0., 0.5, 0., 0.] };
    let (editor, layer, items) = motion_fixture(layer_core::LayerBlend::Normal, 1.);
    let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    let frame = FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) };
    submit_ready(&mut r, FramePacket { view, ..packet(editor.document().scene(), extent) });
    r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: items.clone(), transform: translation([0.; 2]) }));
    submit_ready(&mut r, frame);
    let object = editor.document().scene().vector_layer(layer).unwrap().clone();
    let recolored = |paths: &[PathItem], rgba: [f32; 4]| -> Arc<[PathItem]> { paths.iter().map(|path| {
        let entries = path.style.entries.iter().map(|entry| match entry { PaintEntry::Stroke(stroke) => PaintEntry::Stroke(Stroke { paint: paint(rgba), ..stroke.clone() }), entry => entry.clone() }).collect();
        path.with_style(Arc::new(Style::new(entries)))
    }).collect() };
    let derived = |r: &WgpuRasterizer| r.scene.as_ref().unwrap().vectors.derived_paths();
    let wide = widened(object.selected(&items), 14.);
    for (paths, expected, change) in [(wide.clone(), items.len(), "a width"), (recolored(&wide, [0.1, 0.8, 0.2, 1.]), 0, "a color"),
        (recolored(&wide, [0.1, 0.8, 0.2, 1.]).iter().cloned().collect(), 0, "the same color again"), (widened(wide.to_vec(), 5.), items.len(), "another width")] {
        let (before, tiles) = (derived(&r), r.metrics.vector_tiles);
        r.set_vector_motion_paths(Some(paths));
        submit_ready(&mut r, frame);
        assert_eq!(derived(&r) - before, expected as u64, "{change} change derives only the paths whose outline changed, once");
        assert!(r.metrics.vector_tiles > tiles, "{change} change redraws the moving image");
    }
}

#[test]
fn replacement_previews_present_only_the_moving_images_bounds() {
    let extent = [1200u32, 900u32];
    let view = layer_render::ViewState { width_px: 600, height_px: 450, document_to_surface: [0.5, 0., 0., 0.5, 0., 0.] };
    let (editor, layer, items) = motion_fixture(layer_core::LayerBlend::Normal, 1.);
    let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    let frame = FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) };
    submit_ready(&mut r, FramePacket { view, ..packet(editor.document().scene(), extent) });
    r.set_vector_selection(Some(&layer_core::authored::VectorItemSelection { layer, items: items.clone() }));
    submit_ready(&mut r, frame);
    r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: items.clone(), transform: translation([0.; 2]) }));
    submit_ready(&mut r, frame);
    let (texture, target) = crate::create_target(r.device(), [view.width_px, view.height_px], wgpu::TextureFormat::Rgba8UnormSrgb, "replacement presentation");
    let mut presenter = crate::present::ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba8UnormSrgb, crate::SdrSurfaceColor::Srgb).unwrap();
    presenter.set_target_retention(true);
    presenter.present(&r, &target, view, [1.; 4]).unwrap();
    let object = editor.document().scene().vector_layer(layer).unwrap().clone();
    for width in [14., 4., 20.] {
        let before = r.display_motion().unwrap().2;
        r.set_vector_motion_paths(Some(widened(object.selected(&items), width)));
        submit_ready(&mut r, frame);
        let after = r.display_motion().unwrap().2;
        presenter.present(&r, &target, view, [1.; 4]).unwrap();
        let mut expected = Vec::new();
        for moved in [before, after] { crate::present_damage::add_region(&mut expected, crate::present_damage::damage(moved.in_frame(extent), view, 0)); }
        let expected: u64 = expected.iter().map(|area| area.intersect(PixelRect::full([view.width_px, view.height_px])).area()).sum();
        assert_eq!(presenter.damage_area_pixels(), expected, "a {width} px preview presents the moving image's old and new bounds");
        assert!(expected * 4 < u64::from(view.width_px * view.height_px), "the moving bounds are a small part of the view: {expected}");
        let error = crate::layer_tests::page_bytes(&r, &texture).iter().zip(presented(&r, view)).map(|(a, b)| a.abs_diff(b)).max().unwrap();
        assert!(error <= 1, "the {width} px preview's partial presentation matches a full one: {error}");
    }
}

#[test]
fn turning_prepared_paths_composes_no_pages() {
    let extent = [1200u32, 900u32];
    let view = layer_render::ViewState { width_px: 600, height_px: 450, document_to_surface: [0.5, 0., 0., 0.5, 0., 0.] };
    let (editor, layer, items) = motion_fixture(layer_core::LayerBlend::Normal, 1.);
    let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    let frame = FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) };
    submit_ready(&mut r, FramePacket { view, ..packet(editor.document().scene(), extent) });
    r.set_vector_selection(Some(&layer_core::authored::VectorItemSelection { layer, items: items.clone() }));
    submit_ready(&mut r, frame);
    let [x0, y0, x1, y1] = editor.document().scene().vector_layer(layer).unwrap().items_bounds(&items).unwrap();
    let center = [(x0 + x1) * 0.5, (y0 + y1) * 0.5];
    r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: items.clone(), transform: translation([0.; 2]) }));
    submit_ready(&mut r, frame);
    for degrees in [10f64, 25., 40., 60., 90.] {
        let (sin, cos) = degrees.to_radians().sin_cos();
        let transform = layer_core::authored::Affine64([cos, sin, -sin, cos, center[0] - cos * center[0] + sin * center[1], center[1] - sin * center[0] - cos * center[1]]);
        r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: items.clone(), transform }));
        r.submit(frame).unwrap();
        r.wait_idle().unwrap();
        assert!(r.display_motion().is_some(), "a {degrees}° turn presents through the split");
        assert!(r.metrics.frame_composited_pages.is_empty(), "a {degrees}° turn of prepared paths composes {:?}", r.metrics.frame_composited_pages);
    }
}

#[test]
fn scaled_and_rotated_paths_preview_from_their_moving_image_near_the_committed_transform() {
    let extent = [1200u32, 900u32];
    let view = layer_render::ViewState { width_px: 600, height_px: 450, document_to_surface: [0.5, 0., 0., 0.5, 0., 0.] };
    let turn = |degrees: f64, scale: f64| {
        let (sin, cos) = degrees.to_radians().sin_cos();
        let [a, b, c, d] = [cos * scale, sin * scale, -sin * scale, cos * scale];
        let center = [450., 400.];
        layer_core::authored::Affine64([a, b, c, d, center[0] - a * center[0] - c * center[1], center[1] - b * center[0] - d * center[1]])
    };
    for (blend, opacity) in [(layer_core::LayerBlend::Normal, 1f32), (layer_core::LayerBlend::Normal, 0.8)] {
        let (editor, layer, items) = motion_fixture(blend, opacity);
        let split = opacity == 1.;
        let shown = |r: &WgpuRasterizer| -> Vec<f32> { if split { presented(r, view).into_iter().map(|v| f32::from(v) / 255.).collect() } else { display(r).into_iter().flatten().collect() } };
        let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
        submit_ready(&mut r, FramePacket { view, ..packet(editor.document().scene(), extent) });
        let frame = FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) };
        r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: items.clone(), transform: turn(15., 1.2) }));
        submit_ready(&mut r, frame);
        let tiles = r.metrics.vector_tiles;
        for (degrees, scale) in [(20., 1.25), (25., 1.3)] {
            let transform = turn(degrees, scale);
            r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: items.clone(), transform }));
            submit_ready(&mut r, frame);
            assert_eq!(r.metrics.vector_tiles, tiles, "a {degrees}° turn scaled by {scale} resamples the moving image without drawing paths");
            let moved = committed(&editor, layer, &items, transform);
            let mut fresh = WgpuRasterizer::new_native_headless(moved.document().composition().color).unwrap();
            submit_ready(&mut fresh, FramePacket { view, ..packet(moved.document().scene(), extent) });
            let (actual, expected, before) = (shown(&r), shown(&fresh), { let mut plain = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap(); submit_ready(&mut plain, FramePacket { view, ..packet(editor.document().scene(), extent) }); shown(&plain) });
            let differing = |a: &[f32], b: &[f32]| a.iter().zip(b).filter(|(x, y)| (*x - *y).abs() > 0.15).count();
            let (error, moved_by) = (differing(&actual, &expected), differing(&before, &expected));
            assert!(moved_by > 500 && error * 10 < moved_by, "split {split}: the preview shows the turned paths where committing puts them: {error} differing values against {moved_by} for the unmoved layer");
        }
        r.set_vector_motion(None);
        submit_ready(&mut r, frame);
    }
}

#[test]
#[ignore = "crowded vector layer editing benchmark"]
fn crowded_vector_layer_edit_frames() {
    use std::collections::BTreeSet;
    let extent = [4000u32, 3000u32];
    let zoom: f32 = std::env::var("VECTOR_ZOOM").ok().and_then(|v| v.parse().ok()).unwrap_or(0.4);
    let view = layer_render::ViewState { width_px: 1920, height_px: 1200, document_to_surface: [zoom, 0., 0., zoom, 160. - 1200. * (zoom - 0.4).max(0.), -1300. * (zoom - 0.4).max(0.)] };
    let count = 50_000u64;
    let records: Vec<Record> = crate::vector::bench::scene_b([4000., 3000.], count).into_iter().map(Record::Path).collect();
    let (mut editor, layer) = document(extent, records);
    let tones = VectorObject::from_records((0..12).map(|i| shape(i + 1, [300. * i as f64, 200., 300. * i as f64 + 220., 2900.], [0.2, 0.4, 0.9, 0.3])).collect());
    let (_, edit) = editor.document().create_vector_layer_edit("Tones", tones, None, 0).unwrap();
    editor.perform(edit).unwrap();
    let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    submit_ready(&mut r, FramePacket { view, ..packet(editor.document().scene(), extent) });
    let object = editor.document().scene().vector_layer(layer).unwrap().clone();
    let mut index = layer_core::authored::vector::VectorIndex::default();
    let start = std::time::Instant::now();
    index.hit(&object, [0.; 2], 4.);
    let build = start.elapsed().as_secs_f64() * 1e3;
    let start = std::time::Instant::now();
    let probes: Vec<_> = (0..100).map(|i| index.hit(&object, [1200. + i as f64 * 13., 1400. + (i % 7) as f64 * 31.], 4.)).collect();
    let hit = start.elapsed().as_secs_f64() * 1e3 / 100.;
    eprintln!("hit index build {build:.2} ms, then {hit:.4} ms per probe, {} hits of 100", probes.iter().flatten().count());
    let [a, _, _, d, tx, ty] = view.document_to_surface.map(f64::from);
    let center = [(f64::from(view.width_px) * 0.5 - tx) / a, (f64::from(view.height_px) * 0.5 - ty) / d];
    let mut nearest: Vec<(f64, ItemId)> = object.paths().map(|(_, p)| { let b = p.bounds(); ((b[0] + b[2] - 2. * center[0]).hypot(b[1] + b[3] - 2. * center[1]), p.id) }).collect();
    nearest.sort_by(|x, y| x.0.total_cmp(&y.0));
    let ids: Vec<ItemId> = nearest.into_iter().map(|(_, id)| id).collect();
    let (_texture, target) = crate::create_target(r.device(), [view.width_px, view.height_px], wgpu::TextureFormat::Rgba8UnormSrgb, "vector motion presentation");
    let mut presenter = crate::present::ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba8UnormSrgb, crate::SdrSurfaceColor::Srgb).unwrap();
    presenter.set_target_retention(true);
    {
        let points: Vec<_> = (0..400).map(|i| Anchor::corner([1200. + (i % 40) as f64 * 70., 1300. + (i / 40) as f64 * 170. + (i % 2) as f64 * 60.])).collect();
        let mut stroke = Stroke::new(paint([0.1, 0.2, 0.9, 1.]), 3.);
        stroke.cap = Cap::Round; stroke.join = Join::Round;
        let path = Arc::new(PathItem::new(ItemId(u64::MAX - 9), Arc::new(PathGeometry::new(vec![Subpath::new(&points, false)]).unwrap()), Arc::new(Style::new(vec![PaintEntry::Stroke(stroke)]))));
        r.set_vector_preview(Some(layer_render::VectorPreview { owner: layer, path, revision: 1 }));
        timed(&mut r, FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) });
        r.set_vector_preview(None);
        let hidden = timed(&mut r, FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) });
        eprintln!("recompositing without vector work over {} pages: frame cpu {:.2} wall {:.2} ms", r.metrics.frame_composited_pages.len(), hidden.0, hidden.1);
        submit_ready(&mut r, FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) });
        presenter.present(&r, &target, view, [1.; 4]).unwrap();
        r.wait_idle().unwrap();
    }
    for k in [1usize, 100, 5000] {
        let selected: BTreeSet<ItemId> = ids.iter().copied().take(k).collect();
        let items = Arc::new(selected.clone());
        let start = std::time::Instant::now();
        r.set_vector_selection(Some(&layer_core::authored::VectorItemSelection { layer, items: items.clone() }));
        submit_ready(&mut r, FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) });
        eprintln!("select {k} of {count}: prepared in {:.2} ms", start.elapsed().as_secs_f64() * 1e3);
        let mut drags = Vec::new();
        for i in 0..60 {
            r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: items.clone(), transform: translation([i as f64 * 0.7, i as f64 * -0.4]) }));
            let start = std::time::Instant::now();
            r.submit(FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) }).unwrap();
            presenter.present(&r, &target, view, [1.; 4]).unwrap();
            let cpu = start.elapsed().as_secs_f64() * 1e3;
            r.wait_idle().unwrap();
            drags.push((cpu, start.elapsed().as_secs_f64() * 1e3));
        }
        let first = drags[0];
        let (mut cpu, mut wall): (Vec<f64>, Vec<f64>) = drags[1..].iter().copied().unzip();
        eprintln!("drag {k} of {count}: first frame cpu {:.2} wall {:.2} ms; frame cpu p50 {:.2} p95 {:.2} ms, wall p50 {:.2} p95 {:.2} ms", first.0, first.1,
            percentile(&mut cpu, 0.5), percentile(&mut cpu, 0.95), percentile(&mut wall, 0.5), percentile(&mut wall, 0.95));
        let object = editor.document().scene().vector_layer(layer).unwrap().clone();
        let [x0, y0, x1, y1] = object.items_bounds(&selected).unwrap();
        let turn = |i: usize| {
            let (sin, cos) = (i as f64 * 0.01).sin_cos();
            let scale = 1. + i as f64 * 0.005;
            let [a, b, c, d] = [cos * scale, sin * scale, -sin * scale, cos * scale];
            let center = [(x0 + x1) * 0.5, (y0 + y1) * 0.5];
            layer_core::authored::Affine64([a, b, c, d, center[0] - a * center[0] - c * center[1], center[1] - b * center[0] - d * center[1]])
        };
        let mut turns = Vec::new();
        for i in 1..=30 {
            r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: items.clone(), transform: turn(i) }));
            let start = std::time::Instant::now();
            r.submit(FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) }).unwrap();
            presenter.present(&r, &target, view, [1.; 4]).unwrap();
            let cpu = start.elapsed().as_secs_f64() * 1e3;
            r.wait_idle().unwrap();
            turns.push((cpu, start.elapsed().as_secs_f64() * 1e3));
        }
        r.set_vector_motion(None);
        submit_ready(&mut r, FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) });
        let mut previewed = Vec::new();
        let mut preview = layer_core::Editor::new(editor.document().clone());
        for i in 1..=10 {
            let start = std::time::Instant::now();
            let turned = object.transform_items(&selected, turn(i), true).unwrap();
            preview.perform(preview.document().vector_object_edit(layer, turned).unwrap()).unwrap();
            r.submit(FramePacket { view, composite_all: false, ..packet(preview.document().scene(), extent) }).unwrap();
            presenter.present(&r, &target, view, [1.; 4]).unwrap();
            let cpu = start.elapsed().as_secs_f64() * 1e3;
            r.wait_idle().unwrap();
            previewed.push((cpu, start.elapsed().as_secs_f64() * 1e3));
        }
        submit_ready(&mut r, FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) });
        report(&format!("turn {k} of {count} through the moving image"), &mut turns);
        report(&format!("turn {k} of {count} by re-deriving the paths"), &mut previewed);
        r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: items.clone(), transform: translation([0., 0.]) }));
        submit_ready(&mut r, FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) });
        let mut scrubbed = Vec::new();
        let mut damage = 0;
        for i in 1..=30 {
            let start = std::time::Instant::now();
            r.set_vector_motion_paths(Some(widened(object.selected(&selected), 1. + i as f64 * 0.2)));
            r.submit(FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) }).unwrap();
            presenter.present(&r, &target, view, [1.; 4]).unwrap();
            let cpu = start.elapsed().as_secs_f64() * 1e3;
            r.wait_idle().unwrap();
            scrubbed.push((cpu, start.elapsed().as_secs_f64() * 1e3));
            damage += presenter.damage_area_pixels();
        }
        eprintln!("width scrub {k}: presented {} pixels a frame of {}", damage / 30, view.width_px * view.height_px);
        r.set_vector_motion_paths(None);
        r.set_vector_motion(None);
        submit_ready(&mut r, FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) });
        let mut edited = Vec::new();
        let mut preview = layer_core::Editor::new(editor.document().clone());
        let positions: Vec<_> = object.selected_paths(&selected).into_iter().map(|(position, _)| position).collect();
        for i in 1..=10 {
            let start = std::time::Instant::now();
            let widths = object.with_paths(positions.iter().copied().zip(widened(object.selected(&selected), 1. + i as f64 * 0.2).iter().cloned())).unwrap();
            preview.perform(preview.document().vector_object_edit(layer, widths).unwrap()).unwrap();
            r.submit(FramePacket { view, composite_all: false, ..packet(preview.document().scene(), extent) }).unwrap();
            presenter.present(&r, &target, view, [1.; 4]).unwrap();
            let cpu = start.elapsed().as_secs_f64() * 1e3;
            r.wait_idle().unwrap();
            edited.push((cpu, start.elapsed().as_secs_f64() * 1e3));
        }
        submit_ready(&mut r, FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) });
        report(&format!("width scrub {k} of {count} through the moving image"), &mut scrubbed);
        report(&format!("width scrub {k} of {count} by editing the layer"), &mut edited);
        r.set_vector_selection(None);
        submit_ready(&mut r, FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) });
        let start = std::time::Instant::now();
        let moved = object.transform_items(&selected, layer_core::authored::Affine64([1., 0., 0., 1., 37., -21.]), true).unwrap();
        let transform = start.elapsed().as_secs_f64() * 1e3;
        let start = std::time::Instant::now();
        editor.perform(editor.document().vector_object_edit(layer, moved).unwrap()).unwrap();
        let perform = start.elapsed().as_secs_f64() * 1e3;
        let commit = timed(&mut r, FramePacket { view, composite_all: true, ..packet(editor.document().scene(), extent) });
        let pages = r.metrics.frame_composited_pages.len();
        let start = std::time::Instant::now();
        editor.undo().unwrap();
        let undo_perform = start.elapsed().as_secs_f64() * 1e3;
        let undo = timed(&mut r, FramePacket { view, composite_all: true, ..packet(editor.document().scene(), extent) });
        submit_ready(&mut r, FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) });
        eprintln!("move {k} of {count}: transform {transform:.2} ms, perform {perform:.2} ms, frame over {pages} pages cpu {:.2} wall {:.2} ms; undo perform {undo_perform:.2} ms, frame cpu {:.2} wall {:.2} ms",
            commit.0, commit.1, undo.0, undo.1);
    }
}

#[test]
fn split_drags_redraw_neither_the_overview_nor_the_navigator() {
    use std::collections::BTreeSet;
    let extent = [2400u32, 1800u32];
    let view = layer_render::ViewState { width_px: 600, height_px: 450, document_to_surface: [1., 0., 0., 1., -300., -200.] };
    let mut records: Vec<Record> = (0..4).map(|i| stroke(9000 + i, &[[400. + i as f64 * 40., 300.], [600. + i as f64 * 30., 500.]], 9., [0.9, 0.2, 0.1, 0.8])).collect();
    records.extend(crate::vector::bench::scene_b([2400., 1800.], 1500).into_iter().map(Record::Path));
    let (mut editor, layer) = document(extent, records);
    let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    fn frame(editor: &layer_core::Editor, view: layer_render::ViewState, extent: [u32; 2], time_seconds: f32) -> FramePacket<'_> {
        FramePacket { view, time_seconds, composite_all: false, ..packet(editor.document().scene(), extent) }
    }
    submit_ready(&mut r, FramePacket { view, ..packet(editor.document().scene(), extent) });
    let navigator = r.navigator.revision;
    let items: Arc<BTreeSet<ItemId>> = Arc::new((9000..9004).map(ItemId).collect());
    let mut filled = BTreeSet::new();
    for i in 1..=12u32 {
        r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: items.clone(), transform: translation([f64::from(i) * 3., f64::from(i) * -2.]) }));
        r.submit(frame(&editor, view, extent, i as f32 * 0.03)).unwrap();
        r.wait_idle().unwrap();
        assert!(r.display_motion().is_some(), "a top vector layer drags through the split preview");
        let region = r.vector_motion_region(extent).unwrap();
        for &(level, page) in &r.metrics.frame_composited_pages {
            assert!(level == 0 && !page_rect(page).intersect(region).is_empty() && filled.insert(page), "drag frame {i} recomposited {:?} outside {region:?} or again", (level, page));
        }
    }
    assert_eq!(r.navigator.revision, navigator, "the Navigator keeps the layer as it was until the move commits");
    let moved = editor.document().scene().vector_layer(layer).unwrap().transform_items(&items, layer_core::authored::Affine64([1., 0., 0., 1., 36., -24.]), true).unwrap();
    editor.perform(editor.document().vector_object_edit(layer, moved).unwrap()).unwrap();
    r.set_vector_motion(None);
    r.submit(FramePacket { composite_all: true, ..frame(&editor, view, extent, 1.) }).unwrap();
    r.wait_idle().unwrap();
    assert_ne!(r.navigator.revision, navigator, "the committed move refreshes the Navigator");
}

#[test]
fn appended_strokes_at_zoomed_out_levels_match_a_fresh_render() {
    let extent = [1200u32, 900u32];
    let view = layer_render::ViewState { width_px: 600, height_px: 450, document_to_surface: [0.5, 0., 0., 0.5, 0., 0.] };
    let records: Vec<Record> = crate::vector::bench::scene_b([1200., 900.], 2000).into_iter().map(Record::Path).collect();
    let (mut editor, layer) = document(extent, records);
    let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    submit_ready(&mut r, FramePacket { view, ..packet(editor.document().scene(), extent) });
    for (i, id) in [9001u64, 9002, 9003].into_iter().enumerate() {
        let y = 200. + i as f64 * 180.;
        let object = editor.document().scene().vector_layer(layer).unwrap().insert_front(vec![stroke(id, &[[100., y], [600., y + 90.], [1100., y]], 7., [0.9, 0.2, 0.1, 0.7])]);
        editor.perform(editor.document().vector_object_edit(layer, object).unwrap()).unwrap();
        submit_ready(&mut r, FramePacket { view, composite_all: true, ..packet(editor.document().scene(), extent) });
    }
    let mut fresh = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    submit_ready(&mut fresh, FramePacket { view, ..packet(editor.document().scene(), extent) });
    let (a, b) = (display(&r), display(&fresh));
    let error = max_error(&a, &b);
    let width = r.scale_display.as_ref().unwrap().texture().width() as usize;
    let bad: Vec<usize> = a.iter().zip(&b).enumerate().filter(|(_, (x, y))| (0..4).any(|k| (x[k] - y[k]).abs() > 1e-3)).map(|(i, _)| i).collect();
    if !bad.is_empty() {
        let (xs, ys): (Vec<usize>, Vec<usize>) = bad.iter().map(|i| (i % width, i / width)).unzip();
        eprintln!("differing {} texels in x {}..{} y {}..{} (texture width {width})", bad.len(), xs.iter().min().unwrap(), xs.iter().max().unwrap(), ys.iter().min().unwrap(), ys.iter().max().unwrap());
    }
    assert!(error < 1e-5, "appended strokes must match a full render: {error}");
}

#[test]
fn merging_trimmed_crossings_preserves_ink_across_raster_pages() {
    let extent = [2133, 1600];
    let mut artwork = layer_core::authored::Artwork::new(extent).unwrap();
    crate::test_support::add_paint(&mut artwork, "Below", extent);
    let mut doc = layer_core::Document::from_artwork(artwork).unwrap();
    let ink = VectorObject::from_records(vec![
        stroke(3, &[[900.703125, 850.171875], [900.703125, 1149.859375]], 17.453125, [0., 0., 0., 1.]),
        stroke(2, &[[500.62890625, 850.171875], [500.62890625, 1149.859375]], 17.453125, [0., 0., 0., 1.]),
        stroke(1, &[[299.859375, 999.28125], [1100.00390625, 999.28125]], 17.453125, [0., 0., 0., 1.]),
    ]);
    let (layer, edit) = doc.create_vector_layer_edit("Ink", ink, None, 0).unwrap();
    doc.apply(edit).unwrap();
    doc.working.occurrence = Some(layer);
    let r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let view = layer_render::ViewState { width_px: 1067, height_px: 800, document_to_surface: [0.5, 0., 0., 0.5, 0., 0.] };
    let mut engine = layer_engine::CanvasEngine::new(r, doc, layer_engine::input_queue(16).1, view, layer_engine::ViewTransform::IDENTITY).unwrap();
    settle(&mut engine);
    let object = engine.document().scene().vector_layer(layer).unwrap();
    let trimmed = object.erase(&mut VectorIndex::default(), &Eraser { from: [700., 980.], to: [700., 1020.], radius: 32. }, EraseMode::ToIntersection).unwrap();
    engine.apply_edit(engine.document().vector_object_edit(layer, trimmed).unwrap()).unwrap();
    settle(&mut engine);
    let document = engine.document().clone();
    let mut reference = WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    let shown = native(&mut reference, &document);
    let plan = engine.document().merge_plan(layer_core::MergeKind::Down).unwrap();
    engine.insert_with_operations(plan.edits, vec![(plan.target, plan.operation)], None).unwrap();
    settle(&mut engine);
    let document = engine.document().clone();
    let baked = native(engine.backend_mut(), &document);
    for [x, y] in [[400, 1000], [1000, 1000], [500, 900], [900, 900]] {
        let at = y * extent[0] as usize + x;
        assert!(shown[at][3] > 0.99, "native capture loses ink at {x},{y}: {:?}", shown[at]);
        assert!(baked[at][3] > 0.99, "Merge Down loses ink at {x},{y}: {:?}", baked[at]);
    }
    assert!(max_error(&baked, &shown) < 2. / 255.);
}

#[test]
fn rasterizing_a_vector_layer_bakes_what_it_shows() {
    let (editor, layer) = document([300, 200], records());
    let doc = editor.document().clone();
    let mut engine = engine_for(doc.clone());
    let shown = native(engine.backend_mut(), &doc);
    let plan = engine.document().rasterize_plan(layer, false).unwrap();
    engine.insert_with_operations(plan.edits, vec![(plan.target, plan.operation)], None).unwrap();
    settle(&mut engine);
    assert!(engine.document().scene().vector_layer(layer).is_none());
    let document = engine.document().clone();
    let rasterized = native(engine.backend_mut(), &document);
    assert!(rasterized.iter().filter(|p| p[3] > 0.5).count() > 5000, "the paint layer holds the paths' pixels");
    assert!(max_error(&rasterized, &shown) < 2. / 255., "Rasterize Layer keeps the picture: {}", max_error(&rasterized, &shown));
}

#[test]
fn baking_a_window_of_paths_records_a_few_passes_not_several_per_page() {
    let extent = [2048u32, 2048u32];
    let (editor, layer) = crowded(extent, 3000);
    let view = layer_render::ViewState { width_px: 1024, height_px: 1024, document_to_surface: [0.5, 0., 0., 0.5, 0., 0.] };
    let r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    let mut engine = layer_engine::CanvasEngine::new(r, editor.document().clone(), layer_engine::input_queue(16).1, view, layer_engine::ViewTransform::IDENTITY).unwrap();
    settle(&mut engine);
    let plan = engine.document().rasterize_plan(layer, false).unwrap();
    engine.insert_with_operations(plan.edits, vec![(plan.target, plan.operation)], None).unwrap();
    let mut windows = 0;
    for _ in 0..2000 {
        let passes = engine.backend().metrics.command_passes;
        engine.render_frame().unwrap();
        engine.backend_mut().wait_idle().unwrap();
        let recorded = engine.backend().metrics.command_passes - passes;
        if !engine.has_pending_document_edits() { break; }
        if recorded > 0 {
            windows += 1;
            assert!(recorded <= 4, "baking a 1024-pixel window of 16 pages records {recorded} passes");
        }
    }
    assert!(windows >= 3, "Rasterize bakes the layer a window a frame: {windows} windows");
}

#[test]
fn baking_crowded_paths_derives_them_off_the_canvas_thread_and_keeps_the_picture() {
    let extent = [1200u32, 900u32];
    let (editor, layer) = document(extent, crate::vector::bench::scene_b([1200., 900.], 3000).into_iter().map(Record::Path).collect());
    let doc = editor.document().clone();
    let mut reference = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let shown = native(&mut reference, &doc);
    let view = layer_render::ViewState { width_px: 300, height_px: 225, document_to_surface: [0.25, 0., 0., 0.25, 0., 0.] };
    let r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut engine = layer_engine::CanvasEngine::new(r, doc.clone(), layer_engine::input_queue(16).1, view, layer_engine::ViewTransform::IDENTITY).unwrap();
    settle(&mut engine);
    let derived = |engine: &layer_engine::CanvasEngine<WgpuRasterizer>| engine.backend().scene.as_ref().unwrap().vectors.derived_paths();
    let before = derived(&engine);
    let plan = engine.document().rasterize_plan(layer, false).unwrap();
    engine.insert_with_operations(plan.edits, vec![(plan.target, plan.operation)], None).unwrap();
    settle(&mut engine);
    assert_eq!(derived(&engine), before, "the bake derives no path on the canvas thread");
    let document = engine.document().clone();
    let rasterized = native(engine.backend_mut(), &document);
    assert!(rasterized.iter().filter(|p| p[3] > 0.5).count() > 20_000, "the paint layer holds the paths' pixels");
    assert!(max_error(&rasterized, &shown) < 2. / 255., "Rasterize Layer keeps the picture: {}", max_error(&rasterized, &shown));
}

#[test]
fn paths_painted_onto_paint_layers_match_the_path_drawn_above_them() {
    let (editor, base) = document([300, 200], vec![shape(1, [30., 40., 200., 150.], [0.2, 0.6, 0.3, 0.7])]);
    let mut engine = engine_for(editor.document().clone());
    let plan = engine.document().rasterize_plan(base, false).unwrap();
    engine.insert_with_operations(plan.edits, vec![(plan.target, plan.operation)], None).unwrap();
    settle(&mut engine);
    let target = plan.target;
    assert_eq!(engine.document().target_offset(target), [0, 0]);
    let Record::Path(ink) = stroke(9, &[[20., 30.], [150., 180.], [280., 20.]], 12., [0.8, 0.1, 0.1, 0.9]) else { unreachable!() };
    for alpha_locked in [false, true] {
        let mut doc = engine.document().clone();
        if alpha_locked {
            let mut occurrence = doc.scene().occurrence(base).unwrap().clone();
            occurrence.alpha_locked = true;
            engine.apply_edit(layer_core::Edit::Occurrence(layer_core::RecordChange::replace(&doc.artwork.occurrences, base, Some(occurrence)).unwrap())).unwrap();
            settle(&mut engine);
            doc = engine.document().clone();
        }
        let (above, edit) = doc.create_vector_layer_edit("Ink", VectorObject::from_records(vec![Record::Path(ink.clone())]), None, 0).unwrap();
        let mut expected = doc.clone();
        expected.apply(edit).unwrap();
        if alpha_locked {
            let mut occurrence = expected.scene().occurrence(above).unwrap().clone();
            occurrence.attachment = layer_core::Attachment::Clip;
            expected.apply(layer_core::Edit::Occurrence(layer_core::RecordChange::replace(&expected.artwork.occurrences, above, Some(occurrence)).unwrap())).unwrap();
        }
        let shown = native(engine.backend_mut(), &expected);
        let (edits, operation) = engine.document().paint_path_plan(target, ink.clone()).unwrap();
        engine.insert_with_operations(edits, vec![(target, operation)], None).unwrap();
        settle(&mut engine);
        let document = engine.document().clone();
        let painted = native(engine.backend_mut(), &document);
        assert!(max_error(&painted, &shown) < 2. / 255., "alpha lock {alpha_locked}: {}", max_error(&painted, &shown));
        assert_eq!(document.scene().order().len(), doc.scene().order().len(), "no layer is added");
    }
}

#[test]
fn masks_hide_vector_layers_on_screen_and_once_rasterized_or_merged() {
    use layer_core::{Edit, RecordChange};
    let extent = [300u32, 200u32];
    let mut artwork = layer_core::authored::Artwork::new(extent).unwrap();
    crate::test_support::add_paint(&mut artwork, "Paint", extent);
    let mut doc = layer_core::Document::from_artwork(artwork).unwrap();
    let (layer, edit) = doc.create_vector_layer_edit("Ink", VectorObject::from_records(vec![stroke(1, &[[20., 100.], [280., 100.]], 12., [0.1, 0.1, 0.1, 1.])]), None, 0).unwrap();
    doc.apply(edit).unwrap();
    let mut engine = engine_for(doc);
    settle(&mut engine);
    let coverage = layer_core::CoverageSnapshot::reveal_all(engine.document().artwork.coverage.next_handle(), extent, [0, 0]);
    let mask = SourceTarget::Coverage(coverage.target);
    let mut occurrence = engine.document().scene().occurrence(layer).unwrap().clone();
    occurrence.mask = Some(coverage.use_);
    let mut working = engine.document().working.clone();
    working.occurrence = Some(layer);
    working.target = Some(mask);
    engine.apply_edit(Edit::Batch(vec![Edit::Coverage(RecordChange::insert(&engine.document().artwork.coverage, coverage.source)),
        Edit::Occurrence(RecordChange::replace(&engine.document().artwork.occurrences, layer, Some(occurrence)).unwrap()), Edit::Working(working)])).unwrap();
    settle(&mut engine);
    let mut hidden = layer_core::CoverageSnapshot::reveal_all(engine.document().artwork.coverage.next_handle(), extent, [0, 0]);
    hidden.source.default_coverage = 0.;
    hidden.selection = Some(layer_core::Selection::polygon(layer_core::Rect::from_extent([150, 200]).corners().to_vec()).unwrap());
    engine.append_raster_operation(mask, layer_core::RasterOperation {
        placement: layer_core::Affine::IDENTITY, coverage: hidden, kind: layer_core::RasterOperationKind::Erase { alpha_locked: false },
    }).unwrap();
    settle(&mut engine);
    let masked = engine.document().clone();
    let shown = display(engine.backend());
    let exact = native(engine.backend_mut(), &masked);
    let alpha = |pixels: &[[f32; 4]], x: usize| pixels[100 * extent[0] as usize + x][3];
    for pixels in [&shown, &exact] {
        assert!(alpha(pixels, 80) < 1e-3 && alpha(pixels, 220) > 0.99, "the mask hides the left of the line only: {} {}", alpha(pixels, 80), alpha(pixels, 220));
    }
    assert!(max_error(&shown, &exact) < 1e-5);
    for (bake, apply_mask) in [("Rasterize Layer", Some(false)), ("Apply Mask", Some(true)), ("Merge Down", None)] {
        let mut engine = engine_for(masked.clone());
        settle(&mut engine);
        let plan = apply_mask.map_or_else(|| engine.document().merge_plan(layer_core::MergeKind::Down).unwrap(), |apply| engine.document().rasterize_plan(layer, apply).unwrap());
        engine.insert_with_operations(plan.edits, vec![(plan.target, plan.operation)], None).unwrap();
        settle(&mut engine);
        let document = engine.document().clone();
        assert!(document.scene().vector_layer(layer).is_none());
        let baked = native(engine.backend_mut(), &document);
        assert!(max_error(&baked, &exact) < 2. / 255., "{bake} keeps the masked picture: {}", max_error(&baked, &exact));
    }
}

fn crowded(extent: [u32; 2], count: u64) -> (layer_core::Editor, OccurrenceHandle) {
    let records: Vec<Record> = crate::vector::bench::scene_b(extent.map(f64::from), count).into_iter().map(Record::Path).collect();
    let mut artwork = layer_core::authored::Artwork::new(extent).unwrap();
    crate::test_support::add_paint(&mut artwork, "Base", extent);
    let mut editor = layer_core::Editor::new(layer_core::Document::from_artwork(artwork).unwrap());
    let (layer, edit) = editor.document().create_vector_layer_edit("Ink", VectorObject::from_records(records), None, 0).unwrap();
    editor.perform(edit).unwrap();
    let tones = VectorObject::from_records((0..12).map(|i| shape(i + 1, [300. * i as f64, 200., 300. * i as f64 + 220., 2900.], [0.2, 0.4, 0.9, 0.3])).collect());
    let (_, edit) = editor.document().create_vector_layer_edit("Tones", tones, None, 0).unwrap();
    editor.perform(edit).unwrap();
    (editor, layer)
}
fn report(name: &str, frames: &mut [(f64, f64)]) {
    let worst = frames.iter().copied().enumerate().max_by(|a, b| a.1.1.total_cmp(&b.1.1)).unwrap();
    let (mut cpu, mut wall): (Vec<f64>, Vec<f64>) = frames.iter().copied().unzip();
    eprintln!("{name}: {} frames, total wall {:.1} ms; cpu p50 {:.2} p95 {:.2} max {:.2} ms; wall p50 {:.2} p95 {:.2} max {:.2} ms (frame {})", frames.len(), wall.iter().sum::<f64>(),
        percentile(&mut cpu, 0.5), percentile(&mut cpu, 0.95), percentile(&mut cpu, 1.), percentile(&mut wall, 0.5), percentile(&mut wall, 0.95), percentile(&mut wall, 1.), worst.0);
}

#[test]
#[ignore = "crowded vector layer zoom benchmark"]
fn crowded_vector_layer_zoom_frames() {
    let extent = [4000u32, 3000u32];
    let (editor, layer) = crowded(extent, 50_000);
    let at = |zoom: f32| layer_render::ViewState { width_px: 1920, height_px: 1200, document_to_surface: [zoom, 0., 0., zoom, 960. - 2000. * zoom, 600. - 1500. * zoom] };
    let selected: Arc<std::collections::BTreeSet<ItemId>> = Arc::new(editor.document().scene().vector_layer(layer).unwrap().paths().step_by(10).map(|(_, path)| path.id).collect());
    for (from, to, selection) in [(0.4f32, 0.8f32, false), (0.2, 0.4, false), (0.4, 0.8, true), (0.8, 0.4, false), (0.8, 0.4, true), (0.4, 0.2, true)] {
        let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
        if selection { r.set_vector_selection(Some(&layer_core::authored::VectorItemSelection { layer, items: selected.clone() })); }
        let (_texture, target) = crate::create_target(r.device(), [1920, 1200], wgpu::TextureFormat::Rgba8UnormSrgb, "vector zoom presentation");
        let mut presenter = crate::present::ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba8UnormSrgb, crate::SdrSurfaceColor::Srgb).unwrap();
        presenter.set_target_retention(true);
        submit_ready(&mut r, FramePacket { view: at(from), ..packet(editor.document().scene(), extent) });
        presenter.present(&r, &target, at(from), [1.; 4]).unwrap();
        r.wait_idle().unwrap();
        let mut frames = Vec::new();
        let mut passes = Vec::new();
        for i in 0..=60 {
            let view = at(from * (to / from).powf(i as f32 / 60.));
            let (start, recorded) = (std::time::Instant::now(), r.metrics.command_passes);
            r.submit(FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) }).unwrap();
            presenter.present(&r, &target, view, [1.; 4]).unwrap();
            let cpu = start.elapsed().as_secs_f64() * 1e3;
            r.wait_idle().unwrap();
            frames.push((cpu, start.elapsed().as_secs_f64() * 1e3));
            passes.push(r.metrics.command_passes - recorded);
        }
        eprintln!("  passes per frame: max {} at frame {:?}", passes.iter().max().unwrap(), passes.iter().position(|p| p == passes.iter().max().unwrap()));
        let start = std::time::Instant::now();
        let mut settling = 0;
        while r.has_pending_work() && settling < 2000 {
            r.submit(FramePacket { view: at(to), composite_all: false, ..packet(editor.document().scene(), extent) }).unwrap();
            presenter.present(&r, &target, at(to), [1.; 4]).unwrap();
            r.wait_idle().unwrap();
            settling += 1;
        }
        eprintln!("  frame walls {:?}", frames.iter().map(|f| f.1.round() as u64).collect::<Vec<_>>());
        report(&format!("zoom {from}->{to} over 50000 paths, {} selected", if selection { selected.len() } else { 0 }), &mut frames);
        eprintln!("  settled after {settling} more frames, {:.1} ms", start.elapsed().as_secs_f64() * 1e3);
    }
}

#[test]
fn filling_outside_paths_classifies_captured_pages_in_batches() {
    let extent = [4096u32, 4096u32];
    let (editor, layer) = crowded(extent, 3000);
    let view = layer_render::ViewState { width_px: 128, height_px: 128, document_to_surface: [1. / 32., 0., 0., 1. / 32., 0., 0.] };
    let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    submit_ready(&mut r, FramePacket { view, ..packet(editor.document().scene(), extent) });
    let request = layer_render::RegionRequest {
        enclosure: None, contiguous: true, selection: None, request_id: 7,
        source: layer_render::RegionSource::Scene { snapshot: editor.document().snapshot(), scope: layer_core::authored::SceneScope::RawObjects(layer) },
        position: [extent[0] - 2, extent[1] - 2], tolerance: 0.1, refinement: Default::default(), limit: None,
    };
    let passes = r.metrics.command_passes;
    let result = crate::test_support::receive_request(&mut r, request);
    let pages = page_coordinates(PixelRect::full(extent)).count() as u64;
    let recorded = r.metrics.command_passes - passes;
    assert!(recorded <= pages + pages / 4, "classifying {pages} captured pages records {recorded} passes");
    assert_eq!(result.pixels.extent(), extent);
}

fn hatching(extent: [f64; 2], count: u64) -> Vec<Record> {
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = || { state ^= state << 13; state ^= state >> 7; state ^= state << 17; (state >> 11) as f64 / (1u64 << 53) as f64 };
    (0..count).map(|i| {
        let start = [next() * extent[0], next() * extent[1]];
        let (angle, length, bend) = (0.4 + 0.5 * next(), 40. + 180. * next(), 40. * next() - 20.);
        let anchors: Vec<Anchor> = (0..4).map(|k| { let t = f64::from(k) / 3.; Anchor::corner([start[0] + angle.cos() * length * t - angle.sin() * bend * t * (1. - t) * 4., start[1] + angle.sin() * length * t + angle.cos() * bend * t * (1. - t) * 4.]) }).collect();
        let mut stroke = Stroke::new(paint([next() as f32 * 0.3, next() as f32 * 0.3, next() as f32 * 0.3, 1.]), 2. + 4. * next());
        (stroke.cap, stroke.join) = (Cap::Round, Join::Round);
        stroke.profile = Some(Arc::new(WidthProfile::new(vec![WidthKnot::new(0, 0., 0.1, 0.1), WidthKnot::new(1, 0.5, 1., 1.), WidthKnot::new(2, 1., 0.05, 0.05)]).unwrap()));
        Record::Path(PathItem::new(ItemId(i + 1), Arc::new(PathGeometry::new(vec![Subpath::new(&anchors, false)]).unwrap()), Arc::new(Style::new(vec![PaintEntry::Stroke(stroke)]))))
    }).collect()
}
#[cfg(target_os = "linux")]
fn thread_cpu_ms(clock: libc::clockid_t) -> f64 {
    let mut time = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    assert_eq!(unsafe { libc::clock_gettime(clock, &mut time) }, 0);
    time.tv_sec as f64 * 1e3 + time.tv_nsec as f64 * 1e-6
}

/// `records` on "Ink" under translucent "Tones" columns, and Ink's paths
/// nearest the center first.
fn hatched(extent: [u32; 2], records: Vec<Record>) -> (layer_core::Editor, OccurrenceHandle, Vec<ItemId>) {
    let (mut editor, layer) = document(extent, records);
    let tones = VectorObject::from_records((0..12u64).map(|i| {
        let x = f64::from(extent[0]) * i as f64 / 12.;
        shape(i + 1, [x, 0., x + f64::from(extent[0]) / 20., f64::from(extent[1])], [0.2, 0.4, 0.9, 0.25])
    }).collect());
    let (_, edit) = editor.document().create_vector_layer_edit("Tones", tones, None, 0).unwrap();
    editor.perform(edit).unwrap();
    let center = extent.map(|v| f64::from(v) * 0.5);
    let mut nearest: Vec<(f64, ItemId)> = editor.document().scene().vector_layer(layer).unwrap().paths()
        .map(|(_, p)| { let b = p.bounds(); ((b[0] + b[2] - 2. * center[0]).hypot(b[1] + b[3] - 2. * center[1]), p.id) }).collect();
    nearest.sort_by(|x, y| x.0.total_cmp(&y.0));
    (editor, layer, nearest.into_iter().map(|(_, id)| id).collect())
}
fn centered(extent: [u32; 2], size: [u32; 2], zoom: f32) -> layer_render::ViewState {
    layer_render::ViewState { width_px: size[0], height_px: size[1],
        document_to_surface: [zoom, 0., 0., zoom, (size[0] as f32 - extent[0] as f32 * zoom) / 2., (size[1] as f32 - extent[1] as f32 * zoom) / 2.] }
}

#[test]
fn width_previews_on_a_display_level_that_keeps_one_layer_image_show_the_paths_and_compose_no_pages() {
    let extent = [9504u32, 6336u32];
    let mut records: Vec<Record> = (0..60u64).map(|i| stroke(9000 + i, &[[4200. + (i % 10) as f64 * 110., 2700. + (i / 10) as f64 * 160.], [4300. + (i % 10) as f64 * 100., 2850. + (i / 10) as f64 * 150.]], 6., [0.8, 0.3, 0.1, 0.9])).collect();
    records.extend(hatching(extent.map(f64::from), 2000));
    let (editor, layer, _) = hatched(extent, records);
    let view = centered(extent, [2880, 1800], 0.3);
    let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    let frame = FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) };
    for frame in [FramePacket { view: centered(extent, [2880, 1800], 0.2), ..frame }, frame] {
        r.submit(frame).unwrap();
        for frames in 0.. {
            r.wait_idle().unwrap();
            if !r.has_pending_work() { break; }
            assert!(frames < 1000, "the display settles at a new level");
            r.submit(frame).unwrap();
        }
    }
    let items: Arc<std::collections::BTreeSet<ItemId>> = Arc::new((9000..9060).map(ItemId).collect());
    r.set_vector_selection(Some(&layer_core::authored::VectorItemSelection { layer, items: items.clone() }));
    r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: items.clone(), transform: translation([0., 0.]) }));
    submit_ready(&mut r, frame);
    let object = editor.document().scene().vector_layer(layer).unwrap().clone();
    for width in [12., 3.] {
        let paths = widened(object.selected(&items), width);
        r.set_vector_motion_paths(Some(paths.clone()));
        r.submit(frame).unwrap();
        r.wait_idle().unwrap();
        assert!(r.metrics.frame_composited_pages.is_empty(), "a {width} px width preview recomposites no layer pages: {}", r.metrics.frame_composited_pages.len());
        assert!(r.display_motion().is_some(), "the presenter draws the {width} px preview");
        submit_ready(&mut r, frame);
        let edited = object.with_paths(object.selected_paths(&items).into_iter().map(|(position, _)| position).zip(paths.iter().cloned())).unwrap();
        let mut applied = layer_core::Editor::new(editor.document().clone());
        applied.perform(applied.document().vector_object_edit(layer, edited).unwrap()).unwrap();
        let mut fresh = WgpuRasterizer::new_native_headless(applied.document().composition().color).unwrap();
        submit_ready(&mut fresh, FramePacket { view, ..packet(applied.document().scene(), extent) });
        let (shown, expected) = (presented(&r, view), presented(&fresh, view));
        let differing = shown.as_chunks::<4>().0.iter().zip(expected.as_chunks::<4>().0.iter()).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 8)).count();
        assert!(differing <= 64, "the {width} px preview shows what applying it draws, but for {differing} pixels");
    }
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "crowded vector layer width scrub benchmark"]
fn crowded_vector_layer_width_scrub_frames() {
    let extent = [9504u32, 6336u32];
    let (editor, layer, nearest) = hatched(extent, hatching(extent.map(f64::from), 50_000));
    let view = centered(extent, [2880, 1800], (2880. / extent[0] as f32).min(1800. / extent[1] as f32));
    let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    let frame = || FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) };
    submit_ready(&mut r, FramePacket { view, ..packet(editor.document().scene(), extent) });
    let (_texture, target) = crate::create_target(r.device(), [view.width_px, view.height_px], wgpu::TextureFormat::Rgba8UnormSrgb, "width scrub presentation");
    let mut presenter = crate::present::ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba8UnormSrgb, crate::SdrSurfaceColor::Srgb).unwrap();
    presenter.set_target_retention(true);
    let object = editor.document().scene().vector_layer(layer).unwrap().clone();
    for count in [100usize, 5000] {
        let selected: std::collections::BTreeSet<ItemId> = nearest.iter().take(count).copied().collect();
        let items = Arc::new(selected.clone());
        r.set_vector_selection(Some(&layer_core::authored::VectorItemSelection { layer, items: items.clone() }));
        r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: items.clone(), transform: translation([0., 0.]) }));
        submit_ready(&mut r, frame());
        presenter.present(&r, &target, view, [1.; 4]).unwrap();
        r.wait_idle().unwrap();
        let paths = object.selected(&selected);
        let (mut canvas, mut process, mut frames, mut pages) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for i in 0..120 {
            let widened = widened(paths.clone(), 4. + 3. * (i as f64 * 0.05).sin());
            let (thread, all, start) = (thread_cpu_ms(libc::CLOCK_THREAD_CPUTIME_ID), thread_cpu_ms(libc::CLOCK_PROCESS_CPUTIME_ID), std::time::Instant::now());
            r.set_vector_motion_paths(Some(widened));
            r.submit(frame()).unwrap();
            pages.push(r.metrics.frame_composited_pages.len() as f64);
            presenter.present(&r, &target, view, [1.; 4]).unwrap();
            let cpu = start.elapsed().as_secs_f64() * 1e3;
            canvas.push(thread_cpu_ms(libc::CLOCK_THREAD_CPUTIME_ID) - thread);
            process.push(thread_cpu_ms(libc::CLOCK_PROCESS_CPUTIME_ID) - all);
            r.wait_idle().unwrap();
            frames.push((cpu, start.elapsed().as_secs_f64() * 1e3));
        }
        report(&format!("width scrub {count} of 50000 on 61 MP"), &mut frames);
        eprintln!("  canvas thread cpu p50 {:.2} p95 {:.2} max {:.2} ms; process cpu p50 {:.2} p95 {:.2} max {:.2} ms; composited pages p50 {} max {}",
            percentile(&mut canvas, 0.5), percentile(&mut canvas, 0.95), percentile(&mut canvas, 1.), percentile(&mut process, 0.5), percentile(&mut process, 0.95), percentile(&mut process, 1.),
            percentile(&mut pages, 0.5), percentile(&mut pages, 1.));
        r.set_vector_motion_paths(None);
        r.set_vector_motion(None);
        r.set_vector_selection(None);
        submit_ready(&mut r, frame());
    }
}

#[test]
#[ignore = "crowded vector layer drag start benchmark"]
fn crowded_vector_layer_press_frames() {
    let extent = [9504u32, 6336u32];
    let (editor, layer) = crowded(extent, 10_000);
    let view = layer_render::ViewState { width_px: 2880, height_px: 1800, document_to_surface: [0.2, 0., 0., 0.2, 490., 266.] };
    let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
    submit_ready(&mut r, FramePacket { view, ..packet(editor.document().scene(), extent) });
    let (_texture, target) = crate::create_target(r.device(), [view.width_px, view.height_px], wgpu::TextureFormat::Rgba8UnormSrgb, "press presentation");
    let mut presenter = crate::present::ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba8UnormSrgb, crate::SdrSurfaceColor::Srgb).unwrap();
    presenter.set_target_retention(true);
    let ids: Vec<ItemId> = editor.document().scene().vector_layer(layer).unwrap().paths().map(|(_, path)| path.id).collect();
    for (press, id) in ids.iter().step_by(997).take(4).enumerate() {
        let items = Arc::new(std::collections::BTreeSet::from([*id]));
        let start = std::time::Instant::now();
        r.set_vector_selection(Some(&layer_core::authored::VectorItemSelection { layer, items: items.clone() }));
        r.set_vector_motion(Some(layer_render::VectorMotion { owner: layer, items: items.clone(), transform: translation([4., 3.]) }));
        r.submit(FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) }).unwrap();
        presenter.present(&r, &target, view, [1.; 4]).unwrap();
        let cpu = start.elapsed().as_secs_f64() * 1e3;
        r.wait_idle().unwrap();
        eprintln!("press {press}: first frame cpu {cpu:.2} wall {:.2} ms", start.elapsed().as_secs_f64() * 1e3);
        submit_ready(&mut r, FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) });
        r.set_vector_motion(None);
        r.set_vector_selection(None);
        submit_ready(&mut r, FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) });
    }
}

#[test]
#[ignore = "crowded vector layer fill benchmark"]
fn crowded_vector_layer_fill_outside_frames() {
    for (extent, count, zoom) in [([4248u32, 2832u32], 10_000u64, 0.344f32), ([9504, 6336], 10_000, 0.2)] {
        let (editor, layer) = crowded(extent, count);
        let view = layer_render::ViewState { width_px: 1920, height_px: 1200, document_to_surface: [zoom, 0., 0., zoom, 0., 0.] };
        let mut r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
        submit_ready(&mut r, FramePacket { view, ..packet(editor.document().scene(), extent) });
        let request = layer_render::RegionRequest {
            enclosure: None, contiguous: true, selection: None, request_id: 1,
            source: layer_render::RegionSource::Scene { snapshot: editor.document().snapshot(), scope: layer_core::authored::SceneScope::RawObjects(layer) },
            position: [extent[0] - 2, extent[1] - 2], tolerance: 0.1, refinement: Default::default(), limit: None,
        };
        let (start, passes) = (std::time::Instant::now(), r.metrics.command_passes);
        assert!(r.request_region(request).unwrap());
        let requested = start.elapsed().as_secs_f64() * 1e3;
        let passes = r.metrics.command_passes - passes;
        let mut frames = Vec::new();
        let result = loop {
            let frame = std::time::Instant::now();
            r.submit(FramePacket { view, composite_all: false, ..packet(editor.document().scene(), extent) }).unwrap();
            let polled = r.take_region();
            frames.push((frame.elapsed().as_secs_f64() * 1e3, frame.elapsed().as_secs_f64() * 1e3));
            if let Some(result) = polled { break result.unwrap(); }
            assert!(start.elapsed() < std::time::Duration::from_secs(60), "the region must finish");
        };
        let total = start.elapsed().as_secs_f64() * 1e3;
        eprintln!("fill outside {count} paths over {extent:?}: request {requested:.1} ms recording {passes} passes, result after {total:.1} ms and {} frames, {} pixels wide", frames.len(), result.pixels.extent()[0]);
        report("  frames until the result", &mut frames);
    }
}

#[test]
#[ignore = "crowded vector layer bake benchmark"]
fn crowded_vector_layer_bake_frames() {
    for ((extent, count, zoom), merge) in [([4000u32, 3000u32], 50_000u64, 0.4f32), ([9504, 6336], 10_000, 0.2)].into_iter().flat_map(|size| [(size, false), (size, true)]) {
        let view = layer_render::ViewState { width_px: 1920, height_px: 1200, document_to_surface: [zoom, 0., 0., zoom, 0., 0.] };
        let (editor, layer) = crowded(extent, count);
        let r = WgpuRasterizer::new_native_headless(editor.document().composition().color).unwrap();
        let mut engine = layer_engine::CanvasEngine::new(r, editor.document().clone(), layer_engine::input_queue(16).1, view, layer_engine::ViewTransform::IDENTITY).unwrap();
        settle(&mut engine);
        let plan = if merge {
            let mut working = engine.document().working.clone();
            working.occurrence = Some(layer);
            engine.apply_edit(layer_core::Edit::Working(working)).unwrap();
            settle(&mut engine);
            engine.document().merge_plan(layer_core::MergeKind::Down).unwrap()
        } else { engine.document().rasterize_plan(layer, false).unwrap() };
        let mut frames = Vec::new();
        let start = std::time::Instant::now();
        engine.insert_with_operations(plan.edits, vec![(plan.target, plan.operation)], None).unwrap();
        let mut work = Vec::new();
        loop {
            let frame = std::time::Instant::now();
            let (passes, submissions) = (engine.backend().metrics.command_passes, engine.backend().metrics.submissions);
            engine.render_frame().unwrap();
            let cpu = frame.elapsed().as_secs_f64() * 1e3;
            engine.backend_mut().wait_idle().unwrap();
            frames.push((cpu, frame.elapsed().as_secs_f64() * 1e3));
            work.push((engine.backend().metrics.command_passes - passes, engine.backend().metrics.submissions - submissions));
            if !engine.has_pending_document_edits() && !engine.backend().has_pending_work() { break; }
            assert!(start.elapsed() < std::time::Duration::from_secs(60), "the bake must finish");
        }
        let total = start.elapsed().as_secs_f64() * 1e3;
        eprintln!("  frame walls {:?}", frames.iter().map(|f| f.1.round() as u64).collect::<Vec<_>>());
        eprintln!("  passes and submissions per frame {work:?}");
        report(&format!("{} of {count} paths over {extent:?}", if merge { "Merge Down onto paint" } else { "Rasterize Layer" }), &mut frames);
        eprintln!("  complete after {total:.1} ms");
    }
}
