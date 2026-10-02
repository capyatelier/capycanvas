use super::*;

#[test]
fn display_level_updates_share_one_compute_pass() {
    let doc = document();
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    r.set_complete_display_allowance(1024 << 20);
    let mut frame = packet(&doc.layers, [doc.width, doc.height]);
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    r.submit(frame).unwrap();
    r.submit(FramePacket { composite_all: false, ..frame }).unwrap();
    let mut cache = r.scale_display.take().unwrap();
    assert!(matches!(&cache.pixels, hierarchy::Pixels::Resident { levels, level } if levels.len() - *level as usize > 2));
    let mut commands = Commands::new(&r);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    cache.reduce_output(&mut r, &mut encoder, PixelRect::full(cache.output_plan().size), &mut commands).unwrap();
    assert_eq!(encoder.pass_count(), 1);
    r.uploads.finish(&encoder);
    encoder.submit(&r.queue);
    r.scale_display = Some(cache);
    assert_presentation_mip(&r);
}

#[test]
fn strokes_and_replays_repair_only_touched_pages() {
    use layer_engine::{CanvasEngine, InstantFeedbackConfig, PenEvent, PenPhase,
        SampleFlags, ViewTransform, input_queue};
    for (enabled, preset, taper) in [(false, DefaultBrushPreset::GPen, 0.), (true, DefaultBrushPreset::GPen, 0.),
        (true, DefaultBrushPreset::CloneStamp, 0.), (true, DefaultBrushPreset::GPen, 1.)] {
        let mut doc = document_at([1537, 1025]);
        doc.blend_space = layer_core::BlendSpace::Perceptual;
        doc.reference_layers.insert(doc.layers[0].id);
        doc.layers.insert(0, Layer::paint(LayerId(80), "stroke"));
        doc.active_layer = LayerId(80);
        let mut v = crate::test_support::view([256, 192]);
        v.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        let (mut input, consumer) = input_queue(64);
        let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        r.set_complete_display_allowance(1024 << 20);
        let mut engine = CanvasEngine::new(r, doc, consumer, v,
            ViewTransform { revision: 0, surface_to_document: [8., 0., 0., 8., 0., 0.] }).unwrap();
        engine.set_instant_feedback(InstantFeedbackConfig { enabled, prediction_horizon_micros: 16_000, ..Default::default() }).unwrap();
        let mut brush = layer_core::default_brush(preset);
        brush.diameter = 50.;
        brush.taper.end_distance_diameters = taper;
        brush.color_rgba_linear = [0.08, 0.015, 0.25, 1.];
        engine.set_brush(brush).unwrap();
        if preset == DefaultBrushPreset::CloneStamp {
            engine.set_retouch(Some(layer_core::RetouchSource::References));
            engine.set_clone_source(layer_core::CloneSource { point: Some(layer_core::Point { x: 90., y: 80. }), ..Default::default() });
        }
        let drain = |engine: &mut CanvasEngine<WgpuRasterizer>, settle: bool| {
            let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
            loop {
                engine.render_frame().unwrap();
                engine.backend_mut().wait_idle().unwrap();
                if !engine.has_pending_input() && (!settle || !engine.backend().has_pending_work()) { break; }
                assert!(std::time::Instant::now() < deadline);
            }
        };
        drain(&mut engine, true);
        for stroke in 0..3 {
            for i in 0..=32 {
                let phase = if i == 0 { PenPhase::Down } else if i == 32 { PenPhase::Up } else { PenPhase::Move };
                let t = i as f32 / 32.;
                input.push(PenEvent { timestamp_ns: ((stroke * 33 + i) as u64 + 1) * 8_333_333, pressure: 0.8,
                    ..crate::test_support::pen((stroke * 33 + i) as u64 + 1, phase, [(90. + 530. * t) / 8., (80. + 310. * t) / 8.], SampleFlags::PRIMARY) }).unwrap();
                drain(&mut engine, false);
            }
            assert_eq!(engine.metrics().committed_strokes, stroke as u64 + 1);
            let backing = engine.document().layers[0].raster.wait_data().unwrap();
            let pages = backing.tiles.len() as u64;
            assert!(pages > 0 && pages < 10, "sparse stroke: {pages} pages");
            let work = engine.backend().metrics.composited_pixels;
            drain(&mut engine, true);
            assert!(engine.backend().metrics.composited_pixels - work <= pages * u64::from(PAGE_SIZE).pow(2),
                "native publication must not expand exact repair beyond {pages} touched pages: {} pixels",
                engine.backend().metrics.composited_pixels - work);
            let mut exact = WgpuRasterizer::new_native_headless(engine.document().color).unwrap();
            exact.test.reference = true;
            let frame = FramePacket { view: v, blend_space: engine.document().blend_space, ..packet(&engine.document().layers, [1537, 1025]) };
            exact.submit(frame).unwrap();
            let expected = pixels(&exact, crate::test_support::document_texture(&exact));
            let error = quality(&display_pixels(engine.backend()), &expected, engine.backend().scale_display.as_ref().unwrap().plan);
            assert!(error[2] < 2e-5, "canonical stroke: {error:?}");
            engine.undo().unwrap();
            drain(&mut engine, true);
        }
    }
}

#[test]
fn idle_refinement_presents_completion_and_never_holds_new_input() {
    let doc = document_at([1537, 1025]);
    let extent = [doc.width, doc.height];
    for allowance in [0, 1024 << 20] {
        let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        r.set_complete_display_allowance(allowance);
        let mut frame = packet(&doc.layers, extent);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        r.submit(frame).unwrap();
        r.submit(frame).unwrap();
        let mut presenter = ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba32Float,
            SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
        let (_, target) = create_target(&r.device, [frame.view.width_px, frame.view.height_px],
            wgpu::TextureFormat::Rgba32Float, "refinement presentation");
        presenter.set_target_retention(true);
        presenter.present(&r, &target, frame.view, [0.; 4]).unwrap();
        r.submit(frame).unwrap();
        assert!(r.has_pending_work());
        assert!(!presenter.needs_present(&r, frame.view, [0.; 4]));
        let mut moved = frame.view;
        moved.document_to_surface[4] += 1.;
        assert!(presenter.needs_present(&r, moved, [0.; 4]));
        presenter.set_cursor(r.device(), &[layer_render::CursorSegment {
            from: [20., 30.], to: [50., 60.], distance: 0., marker: 1., scale: 1.,
        }], 1.);
        assert!(presenter.needs_present(&r, frame.view, [0.; 4]));
        presenter.present(&r, &target, frame.view, [0.; 4]).unwrap();
        let dab = crate::tests::test_dab([90., 80.], [0.9, 0.1, 0.3, 1.], 0.8);
        let batch = dab_batch(doc.layers[0].id, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        r.submit(FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
        assert!(presenter.needs_present(&r, frame.view, [0.; 4]));
        presenter.present(&r, &target, frame.view, [0.; 4]).unwrap();
        r.submit(frame).unwrap();
        if presenter.needs_present(&r, frame.view, [0.; 4]) {
            presenter.present(&r, &target, frame.view, [0.; 4]).unwrap();
        }
        for _ in 0..64 {
            if !r.has_pending_work() { break; }
            r.wait_idle().unwrap();
            r.submit(frame).unwrap();
            assert_eq!(presenter.needs_present(&r, frame.view, [0.; 4]), !r.has_pending_work());
        }
        assert!(!r.has_pending_work());
        presenter.present(&r, &target, frame.view, [0.; 4]).unwrap();
        assert!(!presenter.needs_present(&r, frame.view, [0.; 4]));
        let retained = crate::layer_tests::page_bytes(&r, target.texture());
        presenter.set_target_retention(false);
        presenter.present(&r, &target, frame.view, [0.; 4]).unwrap();
        assert_eq!(retained, crate::layer_tests::page_bytes(&r, target.texture()));
        assert!(r.can_submit());
    }
}

#[test]
fn idle_refinement_batches_pages_and_yields_to_new_artwork() {
    let doc = document_at([1537, 1025]);
    let extent = [doc.width, doc.height];
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    let mut frame = packet(&doc.layers, extent);
    frame.composite_all = false;
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    for renderer in [&mut r, &mut exact] { renderer.submit(frame).unwrap(); }
    let work = r.metrics.composited_pixels;
    let passes = r.metrics.command_passes;
    r.submit(frame).unwrap();
    let refined = r.metrics.composited_pixels - work;
    assert!(r.metrics.command_passes - passes <= 5, "resident pages share one composition pass");
    assert!(refined > 2 * u64::from(PAGE_SIZE).pow(2), "idle work must amortize submission and presentation across pages");
    assert!(refined <= 4 * u64::from(PAGE_SIZE).pow(2), "an idle submission must leave room for new input");
    assert!(r.has_pending_work());
    let completed = r.background_ready.clone();
    r.hold_background(false);
    completed.store(true, std::sync::atomic::Ordering::Release);
    assert!(!r.background_ready.load(std::sync::atomic::Ordering::Acquire),
        "an earlier completion cannot release the latest background work");
    r.background_ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
    r.background_refinement = false;
    assert!(!r.can_submit(), "mandatory raster work retains submission backpressure");
    r.background_refinement = true;
    assert!(r.can_submit(), "fresh artwork can queue behind unfinished refinement");
    let submissions = r.metrics.submissions;
    r.submit(frame).unwrap();
    assert_eq!(r.metrics.submissions, submissions, "unfinished refinement cannot queue another idle batch");
    let dab = crate::tests::test_dab([90., 80.], [0.9, 0.1, 0.3, 1.], 0.8);
    let batch = dab_batch(doc.layers[0].id, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
    let stroke = FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame };
    for renderer in [&mut r, &mut exact] { renderer.submit(stroke).unwrap(); }
    r.background_ready.store(true, std::sync::atomic::Ordering::Release);
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
}

#[test]
fn sparse_contacts_preserve_exact_pages_between_their_footprints() {
    let doc = document_at([769, 513]);
    let extent = [doc.width, doc.height];
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    r.set_complete_display_allowance(1024 << 20);
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    let mut frame = packet(&doc.layers, extent);
    frame.composite_all = false;
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    for renderer in [&mut r, &mut exact] { renderer.submit(frame).unwrap(); }
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    let dabs = [[90., 80.], [620., 390.]].map(|position| crate::tests::test_dab(position, [0.9, 0.1, 0.3, 1.], 0.8));
    let mut batch = dab_batch(doc.layers[0].id, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dabs[0].bounds().union(dabs[1].bounds()));
    batch.dab_count = dabs.len() as u32;
    let stroke = FramePacket { dabs: &dabs, dab_batches: std::slice::from_ref(&batch), ..frame };
    let source = &r.scene.as_ref().unwrap().scale_sources.entries[&doc.layers[0].id];
    let updates = source.updates;
    let levels = source.levels.len() as u64;
    for renderer in [&mut r, &mut exact] { renderer.submit(stroke).unwrap(); }
    assert!(r.scene.as_ref().unwrap().scale_sources.entries[&doc.layers[0].id].updates - updates <= 2 * levels,
        "only touched source pages need reduction at each retained level");
    let work = r.metrics.composited_pixels;
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    assert_eq!(r.metrics.composited_pixels - work, 2 * u64::from(PAGE_SIZE).pow(2), "only the two touched pages need exact repair");
}

#[test]
fn settled_composition_reuses_every_zoom_and_refines_only_changed_pages() {
    let mut doc = document_at([769, 513]);
    let extent = [doc.width, doc.height];
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    r.set_complete_display_allowance(1024 << 20);
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    let mut frame = packet(&doc.layers, extent);
    frame.composite_all = false;
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    r.submit(frame).unwrap(); exact.submit(frame).unwrap();
    let drawing_target = r.scale_display.as_ref().unwrap().texture().clone();
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    assert_eq!(r.scale_display.as_ref().unwrap().texture(), &drawing_target, "settling repairs the drawing texture in place");
    assert!(r.scale_display.as_ref().unwrap().hierarchy.as_ref().unwrap().missing().is_none());
    assert!(r.scale_display.as_ref().unwrap().exact_tile.is_none(), "resident refinement writes directly into its retained image");
    let work = r.metrics.composited_pixels;
    for zoom in [0.5, 1., 2., 0.25, 0.125] {
        frame.view.document_to_surface = [zoom, 0., 0., zoom, -40., -20.];
        r.submit(frame).unwrap();
        assert!(matches!(r.scale_display.as_ref().unwrap().pixels, hierarchy::Pixels::Resident { .. }));
        assert_eq!(r.metrics.composited_pixels, work, "camera motion only samples settled pixels");
        assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        assert_presentation_mip(&r);
    }
    let drawing_target = r.scale_display.as_ref().unwrap().texture().clone();
    let dab = crate::tests::test_dab([90., 80.], [0.9, 0.1, 0.3, 1.], 0.8);
    let batch = dab_batch(doc.layers[0].id, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
    let stroke = FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame };
    r.submit(stroke).unwrap(); exact.submit(stroke).unwrap();
    assert_eq!(r.scale_display.as_ref().unwrap().texture(), &drawing_target, "drawing updates the resident level");
    let retained = r.scale_display.as_ref().unwrap().hierarchy.as_ref().unwrap();
    assert_eq!(retained.missing(), Some([0, 0]));
    let work = r.metrics.composited_pixels;
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    assert_eq!(r.metrics.composited_pixels - work, u64::from(PAGE_SIZE).pow(2));
    assert_eq!(r.scale_display.as_ref().unwrap().texture(), &drawing_target, "dirty repair preserves the presentation binding");
    frame.view.document_to_surface = [1., 0., 0., 1., 0., 0.];
    r.submit(frame).unwrap();
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    let mut preview = layer_render::TransformPreview { transaction: 1, layer: doc.layers[0].id,
        moving: true, selection: None, transform: layer_core::ImageTransform::affine(Affine::translation(Point { x: 23., y: 17. })) };
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    for moving in [true, false] {
        preview.moving = moving;
        for renderer in [&mut r, &mut exact] { renderer.set_transform_preview(Some(&preview)).unwrap(); renderer.submit(frame).unwrap(); }
        if !moving { assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact))); }
        assert_eq!(r.readback_srgb_rgba8().unwrap(), exact.readback_srgb_rgba8().unwrap());
    }
    for renderer in [&mut r, &mut exact] { renderer.set_transform_preview(None).unwrap(); renderer.submit(frame).unwrap(); }
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    frame.view.width_px = 128;
    frame.view.height_px = 96;
    frame.view.document_to_surface = [1., 0., 0., 1., -40., -20.];
    for renderer in [&mut r, &mut exact] { renderer.submit(frame).unwrap(); }
    let stroke = FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame };
    for renderer in [&mut r, &mut exact] { renderer.submit(stroke).unwrap(); }
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    let mut effect = Layer::paint(LayerId(80), "native blur");
    effect.kind = LayerKind::Effect;
    let mut program = (*crate::tests::fixture("gaussian_blur").program()).clone();
    program.resolution = layer_core::EffectResolution::Native;
    effect.effect = Some(Arc::new(EffectInstance::new(Arc::new(program))));
    doc.layers.insert(0, effect);
    frame = packet(&doc.layers, extent);
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    r.submit(frame).unwrap(); exact.submit(frame).unwrap();
    assert!(!r.scale_display.as_ref().unwrap().has_pending_work(&r));
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
}
use layer_core::{Affine, EffectInstance, EffectValue, Point};

#[test]
fn global_filters_evict_optional_levels_before_rejecting_the_document() {
    let mut doc = document_at([65, 33]);
    doc.width = 2561; doc.height = 2561;
    let extent = [doc.width, doc.height];
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    r.set_complete_display_allowance(1024 << 20);
    r.native_edit.as_mut().unwrap().composition_bytes = 0;
    let mut frame = packet(&doc.layers, extent);
    frame.composite_all = false;
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    r.submit(frame).unwrap();
    for _ in 0..=page_coordinates(PixelRect::full(extent)).count() {
        if !r.has_pending_work() { break; }
        r.wait_idle().unwrap();
        r.submit(frame).unwrap();
    }
    assert!(!r.has_pending_work());
    let resident = r.scale_display.as_ref().unwrap().resident_bytes();
    assert!(resident > 0);
    let mut program = (*crate::tests::fixture("gaussian_blur").program()).clone();
    program.resolution = layer_core::EffectResolution::Native;
    program.passes = vec![layer_core::EffectPass { entry: program.entry.clone(), sampling: layer_core::EffectSampling::Document }].into();
    let mut effect = Layer::paint(LayerId(80), "global");
    effect.kind = LayerKind::Effect;
    effect.effect = Some(Arc::new(EffectInstance::new(Arc::new(program))));
    doc.layers.insert(0, effect);
    let budget = r.native_edit.as_ref().unwrap();
    assert!(windows::Plan::new(&doc.layers, extent, budget.image_pixel_budget(resident)).is_err());
    assert!(windows::Plan::new(&doc.layers, extent, budget.image_pixel_budget(0)).is_ok());
    frame = packet(&doc.layers, extent);
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    r.submit(frame).unwrap();
    assert_eq!(r.scale_display.as_ref().unwrap().resident_bytes(), 0);
    assert!(!r.scale_display.as_ref().unwrap().has_pending_work(&r));
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    exact.submit(frame).unwrap();
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
}

#[test]
fn native_filter_windows_share_display_storage_and_preserve_halos_during_navigation() {
    for space in layer_core::BlendSpace::ALL {
        let mut doc = document_at([1541, 771]);
        let extent = [doc.width, doc.height];
        let mut effect = Layer::paint(LayerId(80), "native blur");
        effect.kind = LayerKind::Effect;
        let mut program = (*crate::tests::fixture("gaussian_blur").program()).clone();
        program.resolution = layer_core::EffectResolution::Native;
        let mut instance = EffectInstance::new(Arc::new(program));
        instance.set("sigma", EffectValue::Number(7.)).unwrap();
        effect.effect = Some(Arc::new(instance));
        doc.layers.insert(0, effect);
        let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        r.native_edit.as_mut().unwrap().image_pixel_bytes = Some(8 * 1024 * 1024);
        let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        exact.test.reference = true;
        for (zoom, x, opacity) in [(0.5, -192., 0.8), (0.5, -384., 0.8), (1., -512., 0.8), (1., -512., 0.4), (0.125, 0., 0.4)] {
            doc.layers[1].opacity = opacity;
            let mut frame = packet(&doc.layers, extent);
            frame.blend_space = space;
            frame.view.width_px = 96;
            frame.view.height_px = 64;
            frame.view.document_to_surface = [zoom, 0., 0., zoom, x, -32.];
            r.submit(frame).unwrap(); exact.submit(frame).unwrap();
            assert!(r.scale_display.as_ref().unwrap().evaluation == Evaluation::Native);
            assert!(!r.scale_display.as_ref().unwrap().has_pending_work(&r), "native filters publish complete exact output");
            assert!(r.metrics.image_window_submissions > 0);
            assert!(r.metrics.image_window_peak_bytes <= 8 * 1024 * 1024);
            assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        }
    }
}

#[test]
fn watercolor_uses_the_display_graph_across_preview_commit_and_zoom() {
    for space in layer_core::BlendSpace::ALL {
        let doc = document();
        let extent = [doc.width, doc.height];
        let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        exact.test.reference = true;
        let mut frame = packet(&doc.layers, extent);
        frame.blend_space = space;
        frame.composite_all = false;
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        for renderer in [&mut r, &mut exact] { renderer.submit(frame).unwrap(); }
        for (step, kind) in [DabBatchKind::Persistent, DabBatchKind::Preview, DabBatchKind::Persistent].into_iter().enumerate() {
            let mut dab = crate::tests::test_dab([252. + step as f32 * 20., 128.], [0.2, 0.3, 0.8, 0.8], 0.8);
            dab.radii = [90.; 2];
            dab.material = [0.5, 0.8, 1., 0.8];
            let mut batch = dab_batch(doc.layers[0].id, crate::layer_tests::preset_style(DefaultBrushPreset::WatercolorWash), dab.bounds());
            batch.kind = kind;
            batch.stroke_start = step == 0;
            batch.stroke_end = step == 2;
            let stroke = FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame };
            for renderer in [&mut r, &mut exact] { renderer.submit(stroke).unwrap(); }
            assert!(r.paint_layers[0].watercolor.is_some());
            assert!(r.scale_display.is_some(), "watercolor stays in the common display cache");
            let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), r.scale_display.as_ref().unwrap().plan);
            assert!(error[0] < 0.002 && error[1] < 0.015, "watercolor {space:?} {kind:?}: {error:?}");
            if kind == DabBatchKind::Preview {
                for renderer in [&mut r, &mut exact] { renderer.submit(frame).unwrap(); }
            }
            assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        }
        for zoom in [1., 0.5, 0.125] {
            frame.view.document_to_surface = [zoom, 0., 0., zoom, 0., 0.];
            for renderer in [&mut r, &mut exact] { renderer.submit(frame).unwrap(); }
            assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        }
    }
}

#[test]
fn idle_display_refines_an_overview_created_during_transform_motion() {
    let doc = document_at([1541, 771]);
    let extent = [doc.width, doc.height];
    let mut preview = layer_render::TransformPreview { transaction: 1, layer: doc.layers[0].id,
        moving: true, selection: None, transform: layer_core::ImageTransform::affine(
            Affine::around(Point { x: 770., y: 385. }, [0.8, 0.9], 0.15, Point::default())) };
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    let mut frame = packet(&doc.layers, extent);
    frame.composite_all = false;
    frame.view.width_px = 96;
    frame.view.height_px = 64;
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, -96., -32.];
    r.set_transform_preview(Some(&preview)).unwrap(); r.submit(frame).unwrap();
    assert!(!r.has_pending_work());
    preview.moving = false;
    for renderer in [&mut r, &mut exact] {
        renderer.set_transform_preview(Some(&preview)).unwrap(); renderer.submit(frame).unwrap();
    }
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
}

#[test]
fn animated_display_invalidates_retained_pixels_when_time_changes() {
    let mut doc = document();
    let extent = [doc.width, doc.height];
    let mut program = crate::tests::with_time_controls((*crate::tests::fixture("exposure").program()).clone());
    program.id = "time_probe".into();
    program.entry = "time_probe".into();
    program.wgsl = "fn time_probe(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4(c.rgb*(.5+.1*fx_time(b)),c.a);}".into();
    let mut effect = Layer::paint(LayerId(80), "clock");
    effect.kind = LayerKind::Effect;
    effect.effect = Some(Arc::new(EffectInstance::new(Arc::new(program))));
    doc.layers.insert(0, effect);
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    let mut frame = packet(&doc.layers, extent);
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    r.submit(frame).unwrap(); exact.submit(frame).unwrap();
    let original = display_pixels(&r);
    frame.composite_all = false;
    frame.time_seconds = 2.;
    r.submit(frame).unwrap(); exact.submit(frame).unwrap();
    let animated = display_pixels(&r);
    assert!(original != animated, "animation changes retained display output");
    let error = quality(&animated, &pixels(&exact, crate::test_support::document_texture(&exact)), r.scale_display.as_ref().unwrap().plan);
    assert!(error[2] < 0.002, "animated display {error:?}");
}

#[test]
fn idle_display_refines_placement_windows_and_reuses_exact_overlap() {
    for space in layer_core::BlendSpace::ALL {
        let mut doc = document_at([1541, 771]);
        let extent = [doc.width, doc.height];
        doc.layers[0].properties.placement = layer_core::LayerPlacement::from_affine(Affine::around(Point { x: 770., y: 385. }, [0.9, 0.8], 0.17, Point::default()));
        let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        r.set_complete_display_allowance(0);
        let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        exact.test.reference = true;
        let mut frame = packet(&doc.layers, extent);
        frame.blend_space = space;
        frame.composite_all = false;
        frame.view.width_px = 96;
        frame.view.height_px = 64;
        for (x, y) in [(0., 0.), (-180., -60.), (-290., -120.), (0., 0.)] {
            frame.view.document_to_surface = [0.25, 0., 0., 0.25, x, y];
            r.submit(frame).unwrap(); exact.submit(FramePacket { composite_all: true, ..frame }).unwrap();
            assert!(r.scale_display.as_ref().unwrap().overview.is_some());
            assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        }
        frame.view.width_px = extent[0];
        frame.view.height_px = extent[1];
        r.submit(frame).unwrap();
        assert!(r.scale_display.as_ref().unwrap().placed.is_some());
        assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    }
}

#[test]
fn idle_display_reuses_global_effect_dependencies_until_the_next_edit() {
    let mut doc = document();
    let extent = [doc.width, doc.height];
    let mut program = (*crate::tests::fixture("gaussian_blur").program()).clone();
    program.id = "global_probe".into();
    program.entry = "global_probe".into();
    program.wgsl = "fn global_probe(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return fx_sample(fx_extent()-p)*.6+fx_original(p)*.4;}".into();
    program.lookups = Arc::new([]);
    program.passes = vec![layer_core::EffectPass { entry: program.entry.clone(), sampling: layer_core::EffectSampling::Document }].into();
    let mut effect = Layer::paint(LayerId(80), "global");
    effect.kind = LayerKind::Effect;
    effect.effect = Some(Arc::new(EffectInstance::new(Arc::new(program))));
    doc.layers.insert(0, effect);
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    for opacity in [0.7, 0.4] {
        doc.layers[1].opacity = opacity;
        let mut frame = packet(&doc.layers, extent);
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        r.submit(frame).unwrap(); exact.submit(frame).unwrap();
        r.submit(FramePacket { composite_all: false, ..frame }).unwrap();
        let work = r.scene.as_ref().unwrap().image_pass_pixels();
        assert!(work > 0);
        assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        assert_eq!(r.scene.as_ref().unwrap().image_pass_pixels(), work, "unchanged global dependencies execute once");
    }
}

#[test]
fn idle_display_refines_spatial_effects_and_invalidates_committed_paint() {
    for space in layer_core::BlendSpace::ALL {
        let mut doc = document();
        let extent = [doc.width, doc.height];
        let paint = doc.layers[0].id;
        let mut effect = Layer::paint(LayerId(80), "blur");
        effect.kind = LayerKind::Effect;
        effect.effect = Some(Arc::new(EffectInstance::new(crate::tests::fixture("gaussian_blur").program())));
        Arc::make_mut(effect.effect.as_mut().unwrap()).set("sigma", EffectValue::Number(7.)).unwrap();
        effect.mask = Some(layer_core::LayerMask::reveal_all(LayerId(81), Point::default()));
        effect.mask.as_mut().unwrap().default_coverage = 0.7;
        doc.layers.insert(0, effect);
        let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        exact.test.reference = true;
        let mut frame = packet(&doc.layers, extent);
        frame.blend_space = space;
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        r.submit(frame).unwrap(); exact.submit(frame).unwrap();
        assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        let dab = crate::tests::test_dab([254., 128.], [0.9, 0.1, 0.3, 1.], 0.8);
        let batch = dab_batch(paint, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        let stroke = FramePacket { composite_all: false, dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame };
        r.submit(stroke).unwrap(); exact.submit(stroke).unwrap();
        assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    }
}
