use super::*;
use crate::test_support::pen;
use layer_core::{BrushSnapshot, default_brush};
use layer_engine::{CanvasEngine, PenPhase, SampleFlags, ViewTransform, input_queue};

fn settle(engine: &mut CanvasEngine<WgpuRasterizer>) {
    for _ in 0..256 {
        engine.render_frame().unwrap();
        engine.backend_mut().wait_idle().unwrap();
        if !engine.has_pending_input() && !engine.has_pending_document_edits() && !engine.backend().has_pending_work() { return; }
    }
    panic!("the canvas did not settle");
}

fn brush_frames(offset: [i64; 2]) -> Vec<[u64; 2]> {
    let scale = 0.25;
    let extent = [3072, 2048];
    let mut doc = document_at(extent);
    let photo = doc.scene().order()[0];
    let moved = copy_paint(&mut doc, 0);
    let occurrence = doc.artwork.occurrences.get_mut(moved).unwrap();
    occurrence.offset = offset;
    occurrence.opacity = 0.35;
    set_root_entries(&mut doc, vec![moved, photo]);
    doc.apply(doc.select_occurrence_edit(moved).unwrap()).unwrap();
    let r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let (mut input, consumer) = input_queue(256);
    let mut fit = crate::test_support::view(extent);
    fit.width_px = 768;
    fit.height_px = 512;
    fit.document_to_surface = [scale, 0., 0., scale, 0., 0.];
    let mut engine = CanvasEngine::new(r, doc, consumer, fit, ViewTransform { revision: 1, surface_to_document: [1. / scale, 0., 0., 1. / scale, 0., 0.] }).unwrap();
    engine.set_brush(BrushSnapshot { diameter: 1024., mappings: Arc::from([]), ..default_brush(DefaultBrushPreset::GPen) }).unwrap();
    settle(&mut engine);
    let at = |local: [f32; 2]| std::array::from_fn(|i| (local[i] + offset[i] as f32) * scale);
    let mut passes = Vec::new();
    for step in 0..12u64 {
        let mut event = pen(step + 1, if step == 0 { PenPhase::Down } else { PenPhase::Move },
            at([800. + step as f32 * 60., 900. + step as f32 * 16.]), SampleFlags::PRIMARY);
        event.view_revision = 1;
        let before = [engine.backend().metrics.command_passes, engine.backend().metrics.display_reduction_reads];
        input.push(event).unwrap();
        engine.render_frame().unwrap();
        let metrics = &engine.backend().metrics;
        passes.push([metrics.command_passes - before[0], metrics.display_reduction_reads - before[1]]);
        engine.backend_mut().wait_idle().unwrap();
    }
    let mut event = pen(13, PenPhase::Up, at([1520., 1092.]), SampleFlags::PRIMARY);
    event.view_revision = 1;
    input.push(event).unwrap();
    settle(&mut engine);
    passes
}

#[test]
fn painting_a_moved_layer_records_the_passes_and_reads_of_an_unmoved_one() {
    let aligned = brush_frames([0, 0]);
    let moved = brush_frames([37, 101]);
    for (frame, ([moved, _], [aligned, _])) in moved.iter().zip(&aligned).enumerate() {
        assert!(*moved <= aligned + 2, "frame {frame}: {moved} passes at (37, 101) against {aligned} at (0, 0)");
    }
    let reads = |frames: &[[u64; 2]]| frames.iter().map(|[_, reads]| reads).sum::<u64>();
    assert!(reads(&moved) * 10 <= reads(&aligned) * 11,
        "display reductions read {} texels at (37, 101) against {} at (0, 0)", reads(&moved), reads(&aligned));
}

#[test]
fn a_moved_layer_previews_a_large_brush_at_the_reduced_level_close_to_exact() {
    let mut doc = document();
    let paint = copy_paint(&mut doc, 0);
    let OccurrenceContent::Paint(source) = doc.artwork.occurrences.get(paint).unwrap().content else { unreachable!() };
    doc.artwork.paint.get_mut(source).unwrap().base = None;
    doc.artwork.occurrences.get_mut(paint).unwrap().offset = [37, 21];
    insert_occurrence(&mut doc, paint, 0);
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut p = packet(doc.scene(), extent);
    p.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    r.submit(p).unwrap();
    exact.submit(p).unwrap();
    let mut dab = crate::tests::test_dab([218., 108.], [0.9, 0.02, 0.1, 0.7], 1.);
    dab.radii = [45.; 2];
    let batch = DabBatch { kind: DabBatchKind::Preview,
        ..dab_batch(source_at(&doc, 0), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds()) };
    let stroke = FramePacket { dabs: &[dab], dab_batches: &[batch], composite_all: false, ..p };
    r.submit(stroke).unwrap();
    exact.submit(stroke).unwrap();
    assert_eq!(r.preview_level, 2);
    let coarse = r.scale_display.as_ref().unwrap();
    let error = quality(&pixels(&r, coarse.texture()), &pixels(&exact, crate::test_support::document_texture(&exact)), coarse.plan);
    assert!(error[0] < 0.003, "a moved layer's large brush preview must remain close to exact reduction: {error:?}");
}

#[test]
fn painting_a_moved_layer_updates_only_the_painted_part_of_its_reduced_levels() {
    let extent = [1027, 773];
    let mut doc = document_at(extent);
    let photo = doc.scene().order()[0];
    let moved = copy_paint(&mut doc, 0);
    doc.artwork.occurrences.get_mut(moved).unwrap().offset = [37, 101];
    set_root_entries(&mut doc, vec![moved, photo]);
    let target = doc.scene().source_target(moved).unwrap();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut frame = packet(doc.scene(), extent);
    frame.composite_all = false;
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    for renderer in [&mut r, &mut exact] { renderer.submit(frame).unwrap(); }
    for (index, at) in [[300., 200.], [330., 260.], [700., 520.]].into_iter().enumerate() {
        let mut dab = crate::tests::test_dab(at, [0.9, 0.2, 0.1, 0.8], 1.);
        dab.radii = [70.; 2];
        let batch = dab_batch(target, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        let stroke = FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame };
        for renderer in [&mut r, &mut exact] { renderer.submit(stroke).unwrap(); }
        let cache = r.scale_display.as_ref().unwrap();
        assert!(cache.plan.level > 0);
        let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan);
        assert!(error[2] < 1e-3, "dab {index}: {error:?}");
    }
}

#[test]
fn a_moved_layer_settles_its_reduced_predictions_to_exact_reduction() {
    let extent = [1027, 773];
    let mut doc = document_at(extent);
    let photo = doc.scene().order()[0];
    let moved = copy_paint(&mut doc, 0);
    doc.artwork.occurrences.get_mut(moved).unwrap().offset = [37, 101];
    set_root_entries(&mut doc, vec![moved, photo]);
    let target = doc.scene().source_target(moved).unwrap();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut frame = packet(doc.scene(), extent);
    frame.composite_all = false;
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    for renderer in [&mut r, &mut exact] { renderer.submit(frame).unwrap(); }
    let dabs = [[300., 200.], [520., 330.]].map(|at| {
        let mut dab = crate::tests::test_dab(at, [0.9, 0.2, 0.1, 0.8], 1.);
        dab.radii = [70.; 2];
        dab
    });
    let style = crate::layer_tests::preset_style(DefaultBrushPreset::GPen);
    let predicted = DabBatch { kind: DabBatchKind::Preview, dab_count: 2, ..dab_batch(target, style.clone(), dabs[0].bounds().union(dabs[1].bounds())) };
    let preview = FramePacket { dabs: &dabs, dab_batches: std::slice::from_ref(&predicted), ..frame };
    for renderer in [&mut r, &mut exact] { renderer.submit(preview).unwrap(); }
    assert!(r.preview_level > 0);
    let committed = dab_batch(target, style, dabs[0].bounds());
    let stroke = FramePacket { dabs: &dabs[..1], dab_batches: std::slice::from_ref(&committed), ..frame };
    for renderer in [&mut r, &mut exact] { renderer.submit(stroke).unwrap(); }
    let cache = r.scale_display.as_ref().unwrap();
    let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan);
    assert!(error[2] < 1e-3, "{error:?}");
}
