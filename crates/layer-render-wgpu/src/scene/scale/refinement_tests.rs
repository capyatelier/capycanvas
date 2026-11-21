use super::*;

fn stroke_once(r:&mut WgpuRasterizer,frame:FramePacket<'_>,target:SourceTarget,at:[f32;2]) {
    let dab=crate::tests::test_dab(at,[0.2,0.7,0.4,1.],0.8);
    let batch=layer_render::DabBatch {stroke_start:true,stroke_end:true,..dab_batch(target,crate::layer_tests::preset_style(DefaultBrushPreset::GPen),dab.bounds())};
    crate::test_support::submit_ready(r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
}

#[test]
fn display_level_updates_and_dirty_regions_batch_compute_passes() {
    let mut doc = document_at([2048; 2]);
    occurrence_mut(&mut doc, 0).opacity = 0.5;
    add_fill(&mut doc, layer_core::color::RgbColor::WHITE);
    let mut r = crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    submit_published(&mut r, frame);
    crate::test_support::submit_ready(&mut r, FramePacket { composite_all: false, ..frame }).unwrap();
    let mut cache = r.scale_display.take().unwrap();
    assert!(cache.retained_ready());
    assert!(cache.pixels.root().is_some() && cache.pixels.next().is_some());
    let mut commands = RegionRecords::new(&r);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let scene=r.scene.take().unwrap();
    cache.pixels.wrote(PixelRect::full(cache.output_plan().size));
    cache.reduce_output(&scene,&mut r,&mut encoder,&mut commands).unwrap();
    r.scene=Some(scene);
    assert_eq!(encoder.pass_count(), 1);
    r.uploads.finish(&encoder);
    encoder.submit(&r.queue);
    r.scale_display = Some(cache);
    assert_presentation_mip(&r);
    let expected = display_pixels(&r);
    let mut cache = r.scale_display.take().unwrap();
    let mut scene = r.scene.take().unwrap();
    let frame = FramePacket { composite_all: false, ..frame };
    scene.scale_sources.prepare(&r, frame, &[]);
    assert_eq!(cache.plan.level, 2);
    let before=cache.coverage_snapshot();
    for coordinate in [[0,0],[7,7]] {assert!(pages::Coverage::covers(&before.matching,page_rect(coordinate)));}
    evict_composite_pages(&cache,&[[0,0],[7,7]].map(page_rect));
    let stale=cache.coverage_snapshot();
    assert!(cache.pixels.committed_root().is_some());
    for coordinate in [[0,0],[7,7]] {assert!(!pages::Coverage::covers(&stale.matching,page_rect(coordinate)));}
    r.metrics.frame_composited_pages.clear();
    let mut commands = RegionRecords::new(&r);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let mut work = crate::test_support::work(false);
    let mut encoding = Encoding { encoder: &mut encoder, commands: &mut commands, retained:0, budget:&mut work };
    cache.render_graph(&mut scene, &mut r, frame, &mut encoding, Destination::View).unwrap();
    assert_eq!(r.metrics.frame_composited_pages, vec![(2, [0, 0]), (2, [7, 7])]);
    assert_eq!(encoder.pass_count(), 2, "distant dirty pages share composition and display reduction passes");
    r.uploads.finish(&encoder);
    encoder.submit(&r.queue);
    r.scene = Some(scene);
    r.scale_display = Some(cache);
    assert!(crate::test_support::max_error(&display_pixels(&r), &expected) < 2e-5);
}

#[test]
fn first_watercolor_contact_on_empty_layer_batches_source_reduction() {
    use layer_engine::{CanvasEngine, PenPhase, SampleFlags, ViewTransform, input_queue};
    let mut doc = document_at([2048; 2]);
    let ink = paint_occurrence(&mut doc, "ink", None);
    insert_occurrence(&mut doc, ink, 0);
    doc.working.occurrence = Some(ink);
    doc.working.target = Some(source_at(&doc, 0));
    let mut view = crate::test_support::view([256; 2]);
    view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    let (mut input, consumer) = input_queue(64);
    let renderer = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut engine = CanvasEngine::new(renderer, doc, consumer, view,
        ViewTransform { revision: 0, surface_to_document: [8., 0., 0., 8., 0., 0.] }).unwrap();
    let mut brush = layer_core::default_brush(DefaultBrushPreset::WatercolorWash);
    brush.diameter = 50.;
    let mut required=crate::compile::Needs::default();
    let style=layer_render::DabStyle::for_brush(&brush,layer_core::StrokeTool::Brush);
    for preview in [false,true] {required.style(engine.backend(),&style,crate::dry_material::Reach::of(&brush),false,preview);}
    pollster::block_on(required.prepare(&engine.backend().compilation)).unwrap();
    engine.set_brush(brush).unwrap();
    let settle = |engine: &mut CanvasEngine<WgpuRasterizer>| {
        let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
        loop {
            engine.render_frame().unwrap();
            engine.backend_mut().wait_idle().unwrap();
            if !engine.has_pending_input() && !engine.has_pending_document_edits() && engine.backend().next_frame()!=layer_render::FrameDemand::Work { break; }
            assert!(std::time::Instant::now() < deadline);
        }
    };
    settle(&mut engine);
    let original = display_pixels(engine.backend());
    for contact in 0..2 {
        let before = [engine.backend().metrics.dabs, engine.backend().metrics.command_passes];
        input.push(crate::test_support::pen(contact * 2 + 1, PenPhase::Down, [96.; 2], SampleFlags::PRIMARY)).unwrap();
        engine.render_frame().unwrap();
        engine.backend_mut().wait_idle().unwrap();
        let metrics = &engine.backend().metrics;
        assert!(metrics.dabs > before[0]);
        let passes = metrics.command_passes - before[1];
        assert!(passes <= 64, "first watercolor contact {contact} must batch cold source pages: {passes} passes");
        let painted = display_pixels(engine.backend());
        assert_ne!(painted, original);
        assert_eq!(painted.last(), original.last(), "distant photo pixels stay unchanged");
        input.push(crate::test_support::pen(contact * 2 + 2, PenPhase::Up, [96.; 2], SampleFlags::PRIMARY)).unwrap();
        settle(&mut engine);
        let mut exact = WgpuRasterizer::new_native_headless(engine.document().composition().color).unwrap();
        exact.test.reference = true;
        submit_published(&mut exact, packet(engine.document().scene(), [2048; 2]));
        let expected = pixels(&exact, crate::test_support::document_texture(&exact));
        let error = quality(&display_pixels(engine.backend()), &expected, engine.backend().scale_display.as_ref().unwrap().plan, engine.backend().scale_display.as_ref().unwrap().texture().format());
        assert!(error[2] < 2e-5, "committed watercolor: {error:?}");
        engine.undo().unwrap();
        settle(&mut engine);
        assert!(crate::test_support::max_error(&display_pixels(engine.backend()), &original) < 2e-5);
    }
}

#[test]
fn strokes_and_replays_repair_only_touched_pages() {
    use layer_engine::{CanvasEngine, InstantFeedbackConfig, PenEvent, PenPhase,
        SampleFlags, ViewTransform, input_queue};
    for (enabled, preset, taper) in [(false, DefaultBrushPreset::GPen, 0.), (true, DefaultBrushPreset::GPen, 0.),
        (true, DefaultBrushPreset::CloneStamp, 0.), (true, DefaultBrushPreset::GPen, 1.)] {
        let mut doc = document_at([1537, 1025]);
        composition_mut(&mut doc).blend = layer_core::BlendSpace::Perceptual;
        occurrence_mut(&mut doc,0).reference=true;
        let stroke=paint_occurrence(&mut doc,"stroke",None);insert_occurrence(&mut doc,stroke,0);
        doc.working.occurrence=Some(stroke);doc.working.target=Some(source_at(&doc,0));
        let mut v = crate::test_support::view([256, 192]);
        v.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        let (mut input, consumer) = input_queue(64);
        let r = crate::memory::memory_tests::renderer(doc.composition().color,
            layer_core::DeviceMemoryFacts { budget_bytes: Some(2 << 30), ..Default::default() }).unwrap();
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
                if !engine.has_pending_input() && (!settle || !engine.has_pending_document_edits() && engine.backend().next_frame()!=layer_render::FrameDemand::Work) { break; }
                assert!(std::time::Instant::now() < deadline);
            }
        };
        drain(&mut engine, true);
        assert!(!engine.has_pending_document_edits(),"initial stroke fixture has an unsubmitted document frame");
        for stroke in 0..3 {
            for i in 0..=32 {
                let phase = if i == 0 { PenPhase::Down } else if i == 32 { PenPhase::Up } else { PenPhase::Move };
                let t = i as f32 / 32.;
                input.push(PenEvent { timestamp_ns: ((stroke * 33 + i) as u64 + 1) * 8_333_333, pressure: 0.8,
                    ..crate::test_support::pen((stroke * 33 + i) as u64 + 1, phase, [(90. + 530. * t) / 8., (80. + 310. * t) / 8.], SampleFlags::PRIMARY) }).unwrap();
                drain(&mut engine, false);
            }
            assert_eq!(engine.metrics().committed_strokes, stroke as u64 + 1);
            let backing = paint_at(engine.document(),0).raster.wait_data().unwrap();
            let pages = backing.tiles.len() as u64;
            assert!(pages > 0 && pages < 10, "sparse stroke: {pages} pages");
            let work = engine.backend().metrics.composited_pixels;
            drain(&mut engine, true);
            assert!(engine.backend().metrics.composited_pixels - work <= pages * u64::from(PAGE_SIZE).pow(2),
                "native publication must not expand exact repair beyond {pages} touched pages: {} pixels",
                engine.backend().metrics.composited_pixels - work);
            let mut exact = WgpuRasterizer::new_native_headless(engine.document().composition().color).unwrap();
            exact.test.reference = true;
            let frame = FramePacket { view: v, blend_space: engine.document().composition().blend, ..packet(engine.document().scene(), [1537, 1025]) };
            submit_published(&mut exact, frame);
            let expected = pixels(&exact, crate::test_support::document_texture(&exact));
            let error = quality(&display_pixels(engine.backend()), &expected, engine.backend().scale_display.as_ref().unwrap().plan, engine.backend().scale_display.as_ref().unwrap().texture().format());
            assert!(error[2] < 2e-5, "canonical stroke: {error:?}");
            engine.undo().unwrap();
            drain(&mut engine, true);
        }
    }
}

#[test]
fn idle_refinement_presents_completion_and_never_holds_new_input() {
    let doc = document_at([1537, 1025]);
    let extent = doc.composition().size;
    for admitted in [false,true] {
        let mut r = crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
        let mut frame = packet(doc.scene(), extent);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        layer_render::CanvasRenderer::set_working_layer(&mut r,doc.working.occurrence);
        if admitted {r.begin_frame_memory(frame);}
        else {working_scene_frame(&mut r, frame);}
        submit_published(&mut r, frame);
        stroke_once(&mut r,frame,source_at(&doc,0),[700.,500.]);
        let cache=r.scale_display.as_ref().unwrap();
        assert!(cache.evaluation==Evaluation::Display);
        assert!(!retained_root_pages(cache).is_empty());
        assert!(cache.pages.lock().unwrap().bytes()<=r.memory.display_owner().grant().gpu_bytes);
        assert!(r.next_frame()==layer_render::FrameDemand::Work,"the stroked display still refines");
        let mut presenter = ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba32Float,
            SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
        let (_, target) = create_target(&r.device, [frame.view.width_px, frame.view.height_px],
            wgpu::TextureFormat::Rgba32Float, "refinement presentation");
        presenter.set_target_retention(true);
        pollster::block_on(presenter.prepare(&r)).unwrap();
        presenter.present(&r, &target, frame.view, [0.; 4]).unwrap();
        let mut moved = frame.view;
        moved.document_to_surface[4] += 1.;
        assert!(presenter.needs_present(&r, moved, [0.; 4]));
        presenter.set_cursor(r.device(), &[layer_render::CursorSegment {
            from: [20., 30.], to: [50., 60.], distance: 0., marker: 1., scale: 1.,
        }], 1.);
        assert!(presenter.needs_present(&r, frame.view, [0.; 4]));
        presenter.present(&r, &target, frame.view, [0.; 4]).unwrap();
        let dab = crate::tests::test_dab([90., 80.], [0.9, 0.1, 0.3, 1.], 0.8);
        let batch = dab_batch(source_at(&doc, 0), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        crate::test_support::submit_ready(&mut r, FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
        assert!(presenter.needs_present(&r, frame.view, [0.; 4]));
        presenter.present(&r, &target, frame.view, [0.; 4]).unwrap();
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        if presenter.needs_present(&r, frame.view, [0.; 4]) {
            presenter.present(&r, &target, frame.view, [0.; 4]).unwrap();
        }
        for _ in 0..64 {
            if r.next_frame()!=layer_render::FrameDemand::Work { break; }
            r.wait_idle().unwrap();
            let revision=r.composite_revision;
            crate::test_support::submit_ready(&mut r,frame).unwrap();
            assert_eq!(presenter.needs_present(&r,frame.view,[0.;4]),r.composite_revision!=revision,"only committed page damage requests artwork presentation");
            if presenter.needs_present(&r,frame.view,[0.;4]) {presenter.present(&r,&target,frame.view,[0.;4]).unwrap();}
        }
        assert!(r.next_frame()!=layer_render::FrameDemand::Work);
        let cache=r.scale_display.as_ref().unwrap();
        assert!(cache.coverage_snapshot().ready(cache.plan.bounds,pages::Accuracy::Display));
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
    for (extent, expected_pages, expected_pixels) in [
        ([1537, 1025], 14, 1537 * 512), ([4609, 769], SOURCE_SLOTS, 4096 * 256),
    ] {
        for (visible,opacity) in [(true,1.),(false,1.),(true,0.35)] {
            let mut doc = document_at(extent);
            let fill_id=add_fill(&mut doc, layer_core::color::RgbColor::WHITE);
            let fill=doc.artwork.occurrences.get_mut(fill_id).unwrap();
            fill.visible=visible;fill.opacity=opacity;
            let extent = doc.composition().size;
            let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
            let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
            exact.test.reference = true;
            let mut frame = packet(doc.scene(), extent);
            frame.composite_all = false;
            frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
            for renderer in [&mut r, &mut exact] { submit_published(renderer, frame); }
            let work = r.metrics.composited_pixels;
            let passes = r.metrics.command_passes;
            crate::test_support::submit_ready(&mut r, frame).unwrap();
            let refined = r.metrics.composited_pixels - work;
            assert!(r.metrics.command_passes - passes <= 5, "resident pages share one composition pass");
            assert_eq!(r.metrics.frame_composited_pages.len(), expected_pages, "idle work fills one bounded strip");
            assert_eq!(refined, expected_pixels, "idle work stops at the strip boundary");
            let strip = r.scale_display.as_ref().unwrap().exact_tile.as_ref().unwrap();
            assert!(refined <= u64::from(strip.texture.width()) * u64::from(strip.texture.height()));
            assert!(r.next_frame()==layer_render::FrameDemand::Work);
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
            crate::test_support::submit_ready(&mut r, frame).unwrap();
            assert_eq!(r.metrics.submissions, submissions, "unfinished refinement cannot queue another idle batch: fill visible={visible}, opacity={opacity}");
            let dab = crate::tests::test_dab([90., 80.], [0.9, 0.1, 0.3, 1.], 0.8);
            let batch = dab_batch(source_at(&doc, 0), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
            let stroke = FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame };
            for renderer in [&mut r, &mut exact] { crate::test_support::submit_ready(renderer, stroke).unwrap(); }
            assert!(r.metrics.submissions>submissions,"fresh artwork still submits: fill visible={visible}, opacity={opacity}");
            r.background_ready.store(true, std::sync::atomic::Ordering::Release);
            assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
            let mut changed=doc.clone();
            changed.artwork.occurrences.get_mut(fill_id).unwrap().visible= !visible;
            r.background_ready=Arc::new(std::sync::atomic::AtomicBool::new(false));
            r.background_refinement=true;
            let submissions=r.metrics.submissions;
            crate::test_support::submit_ready(&mut r, FramePacket {scene:changed.scene(),..frame}).unwrap();
            assert!(r.metrics.submissions>submissions,"fill visibility edits bypass unfinished idle backpressure");
        }
    }
}

#[test]
fn sparse_contacts_preserve_exact_pages_between_their_footprints() {
    let doc = document_at([769, 513]);
    let extent = doc.composition().size;
    let mut r = crate::memory::memory_tests::renderer(doc.composition().color,
        layer_core::DeviceMemoryFacts { budget_bytes: Some(2 << 30), ..Default::default() }).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut frame = packet(doc.scene(), extent);
    frame.composite_all = false;
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    for renderer in [&mut r, &mut exact] { submit_published(renderer, frame); }
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    let dabs = [[90., 80.], [620., 390.]].map(|position| crate::tests::test_dab(position, [0.9, 0.1, 0.3, 1.], 0.8));
    let mut batch = dab_batch(source_at(&doc, 0), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dabs[0].bounds().union(dabs[1].bounds()));
    batch.dab_count = dabs.len() as u32;
    let stroke = FramePacket { dabs: &dabs, dab_batches: std::slice::from_ref(&batch), ..frame };
    let source = &r.scene.as_ref().unwrap().scale_sources.entries[&source_at(&doc, 0)];
    let updates = source.updates;
    let levels = source_levels(r.scale_display.as_ref().unwrap(),source_at(&doc,0)).len() as u64;
    for renderer in [&mut r, &mut exact] { crate::test_support::submit_ready(renderer, stroke).unwrap(); }
    assert!(r.scene.as_ref().unwrap().scale_sources.entries[&source_at(&doc, 0)].updates - updates <= 2 * levels,
        "only touched source pages need reduction at each retained level");
    let work = r.metrics.composited_pixels;
    crate::test_support::complete(&r);
    r.metrics.frame_composited_pages.clear();
    let mut scene=r.scene.take().unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    {let mut work=crate::test_support::work(false);scene.refine_display(&mut r,frame,&mut encoder,&mut work).unwrap();}
    r.uploads.finish(&encoder);encoder.submit(&r.queue);r.scene=Some(scene);
    assert_eq!(r.metrics.frame_composited_pages,vec![(0,[0,0]),(0,[2,1])],"plain painting batches explicit regions across gaps");
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    assert_eq!(r.metrics.composited_pixels - work, 2 * u64::from(PAGE_SIZE).pow(2), "only the two touched pages need exact repair");
}

#[test]
fn a_stroke_preview_keeps_exact_cells_it_does_not_reach() {
    use layer_engine::{CanvasEngine, InstantFeedbackConfig, PenPhase, SampleFlags, ViewTransform, input_queue};
    use layer_core::color::{ColorProfile, RgbSpace, SampleDepth, source::{SourceBuilder, SourceChannels, SourceInterpretation}};
    let extent = [2048, 1024];
    let mut photo = SourceBuilder::new(extent, SourceInterpretation {channels: SourceChannels::Rgb, depth: SampleDepth::U8,
        profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false}, 64 << 20).unwrap();
    for y in 0..extent[1] {photo.push_row(&(0..extent[0]).flat_map(|x| [(x / 7) as u8, (y / 3) as u8, (x ^ y) as u8]).collect::<Vec<_>>()).unwrap();}
    let doc = layer_color::photo_project(photo.finish().unwrap(), Default::default(), "Photo".into(), SampleDepth::U8).unwrap();
    let scale = 0.4;
    let mut view = crate::test_support::view([820, 410]);
    view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
    let (mut input, consumer) = input_queue(64);
    let brush = layer_core::BrushSnapshot { diameter: 24., ..layer_core::default_brush(DefaultBrushPreset::Pencil) };
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.prepare_startup(&doc, &brush, false).unwrap();
    r.finish_startup_catalog();
    crate::test_support::wait_startup(&mut r, std::time::Instant::now() + READBACK_TIMEOUT, |progress| progress.complete, format_args!("startup"));
    let mut engine = CanvasEngine::new(r, doc, consumer, view, ViewTransform { revision: 0, surface_to_document: [1. / scale, 0., 0., 1. / scale, 0., 0.] }).unwrap();
    engine.set_instant_feedback(InstantFeedbackConfig { enabled: true, use_platform_prediction: false, prediction_horizon_micros: 16_000, ..Default::default() }).unwrap();
    engine.set_brush(brush).unwrap();
    let mut clock = 1_000_000_000u64;
    let settle = |engine: &mut CanvasEngine<WgpuRasterizer>, clock: &mut u64| {
        let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
        loop {
            *clock += 16_666_667;
            engine.render_frame_at(*clock).unwrap();
            engine.backend_mut().wait_idle().unwrap();
            let paints = { use layer_render::CanvasRenderer; engine.backend().paint_ready(engine.document(), engine.brush(), false) };
            if paints && engine.backend().settled() && !crate::scene::scale::tests::publication_pending(engine.backend()) && !engine.has_pending_document_edits() { return; }
            assert!(std::time::Instant::now() < deadline, "the canvas did not settle");
        }
    };
    settle(&mut engine, &mut clock);
    assert!(engine.backend().scale_display.as_ref().unwrap().exact_cells().iter().all(|(_, exact)| *exact), "the settled view is exact before the stroke");
    let revision = engine.document().scene().revision();
    engine.set_active_layer(engine.document().working.occurrence.unwrap()).unwrap();
    assert_ne!(engine.document().scene().revision(), revision, "selecting the layer is a document edit the stroke's first frame carries");
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    let mut drawn = None;
    for step in 0u64.. {
        if drawn.is_none() && engine.backend().metrics().dabs > 0 {drawn = Some(step);}
        if drawn.is_some_and(|first| step >= first + 6) {break;}
        assert!(std::time::Instant::now() < deadline, "the stroke never drew");
        for sample in (!engine.has_pending_input()).then_some(0..2).into_iter().flatten() {
            clock += 8_333_333;
            let mut event = crate::test_support::pen(step * 2 + sample + 1, if step + sample == 0 { PenPhase::Down } else { PenPhase::Move }, [20. + (step * 2 + sample).min(24) as f32, 30.], SampleFlags::PRIMARY);
            event.timestamp_ns = clock;
            input.push(event).unwrap();
        }
        clock += 8_333_333;
        engine.render_frame_at(clock).unwrap();
        engine.backend_mut().wait_idle().unwrap();
    }
    let preview = engine.backend().scene.as_ref().unwrap().scale_pages.lock().unwrap().expected().and_then(|frame| frame.preview_for(engine.document().working.target.unwrap()).map(|preview| preview.level()));
    assert!(preview.is_some_and(|level| level > 0), "the stroke previews at a reduced level: {preview:?}");
    let reach = PixelRect::new(0, 0, 512, 512);
    let stale: Vec<_> = engine.backend().scale_display.as_ref().unwrap().exact_cells().into_iter()
        .filter(|(cell, exact)| !exact && cell.intersect(reach).is_empty()).map(|(cell, _)| cell).collect();
    assert!(stale.is_empty(), "cells the stroke and its preview do not reach lost their exact pixels: {stale:?}");
}

#[test]
fn idle_spatial_refinement_bounds_pass_work_and_yields_between_distant_contacts() {
    let mut doc = document_at([2048;2]);
    let extent = doc.composition().size;
    let paint = source_at(&doc,0);
    let mut program = (*crate::tests::fixture("exposure").program()).clone();
    program.alpha = layer_core::EffectAlpha::Filter;
    program.entry = "idle_average".into();
    program.wgsl = "fn idle_average(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return (fx_sample(p-vec2<f32>(16.,0.))+fx_sample(p+vec2<f32>(16.,0.)))*.5;}".into();
    program.passes = vec![layer_core::EffectPass {entry:program.entry.clone(),sampling:layer_core::EffectSampling::Neighborhood {radius:16}}].into();
    let effect = effect_occurrence(&mut doc,EffectInstance::new(Arc::new(program)),"local average");
    doc.artwork.occurrences.get_mut(effect).unwrap().attachment = layer_core::Attachment::Effect;
    insert_occurrence(&mut doc,effect,0);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut frame = packet(doc.scene(),extent);
    frame.composite_all = false;
    frame.view.document_to_surface = [0.25,0.,0.,0.25,0.,0.];
    for renderer in [&mut r,&mut exact] {submit_published(renderer, frame);}
    assert_settled(&mut r,frame,&pixels(&exact,crate::test_support::document_texture(&exact)));
    let mut dabs = [[128.,128.],[1664.,1664.]].map(|position|crate::tests::test_dab(position,[0.9,0.1,0.3,1.],0.8));
    for dab in &mut dabs {dab.radii=[8.;2];}
    let mut batch = dab_batch(paint,crate::layer_tests::preset_style(DefaultBrushPreset::GPen),dabs[0].bounds().union(dabs[1].bounds()));
    batch.dab_count = 2;
    let stroke = FramePacket {dabs:&dabs,dab_batches:std::slice::from_ref(&batch),..frame};
    for renderer in [&mut r,&mut exact] {crate::test_support::submit_ready(renderer, stroke).unwrap();}
    crate::test_support::complete(&r);
    r.metrics.frame_composited_pages.clear();
    let mut scene=r.scene.take().unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    {let mut work=crate::test_support::work(false);scene.refine_display(&mut r,frame,&mut encoder,&mut work).unwrap();}
    r.uploads.finish(&encoder);encoder.submit(&r.queue);
    r.scene=Some(scene);
    assert_eq!(r.metrics.frame_composited_pages,vec![(0,[0,0])]);
    assert_eq!(r.scene.as_ref().unwrap().image_work(),[1,1]);
    let output = page_rect([0,0]).intersect(PixelRect::full(extent));
    let scene = r.scene.as_ref().unwrap();
    assert_eq!(scene.cached_capture_window(frame.scene,output).in_frame(extent).area(),272u64.pow(2),"the document-edge page reads only its declared halo");
    assert_eq!(scene.image_pass_pixels(),output.area(),"the pass writes only the requested output page");
    assert!(r.next_frame()==layer_render::FrameDemand::Work,"the distant page waits for another idle batch");
    let submissions = r.metrics.submissions;
    r.background_ready=Arc::new(std::sync::atomic::AtomicBool::new(false));
    r.background_refinement=true;
    let dab=crate::tests::test_dab([640.,640.],[0.1,0.7,0.3,1.],0.8);
    let batch=dab_batch(paint,crate::layer_tests::preset_style(DefaultBrushPreset::GPen),dab.bounds());
    let stroke=FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame};
    for renderer in [&mut r,&mut exact] {crate::test_support::submit_ready(renderer, stroke).unwrap();}
    assert!(r.metrics.submissions>submissions,"fresh paint interrupts unfinished spatial refinement");
    r.background_ready.store(true,std::sync::atomic::Ordering::Release);
    assert_settled(&mut r,frame,&pixels(&exact,crate::test_support::document_texture(&exact)));
}

#[test]
fn cold_multilayer_idle_refinement_finishes_with_a_bounded_source_batch() {
    use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey};
    let extent = [PAGE_SIZE * 8, PAGE_SIZE * 2];
    let mut doc = document_at(extent);
    let paint = primary_paint(&doc);
    doc.artwork.paint.get_mut(paint).unwrap().base = None;
    for layer in 0..4 {
        let occurrence = if layer == 0 { doc.scene().order()[0] } else {
            let occurrence = paint_occurrence(&mut doc, "Ink", None);
            insert_occurrence(&mut doc, occurrence, 0);
            occurrence
        };
        doc.artwork.occurrences.get_mut(occurrence).unwrap().opacity = 0.72;
        let OccurrenceContent::Paint(paint) = doc.scene().occurrence(occurrence).unwrap().content else { unreachable!() };
        let mut data = RasterData::default();
        for y in 0..2 {
            for x in 0..8 {
                let pixel = [31 + layer * 41, 17 + x as u8 * 23, 37 + y as u8 * 53, 255];
                let blob = TileBlob::encode(doc.composition().color.paint_descriptor(),
                    &pixel.repeat(PAGE_SIZE.pow(2) as usize)).unwrap();
                data.tiles.insert(TileKey { plane: RasterPlane::Color, coordinate: [x, y] }, RasterTile::backed(blob));
            }
        }
        doc.artwork.paint.get_mut(paint).unwrap().raster = RasterRevision::backed(data);
    }
    let authored = doc.artwork.clone();
    let mut exact = crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    crate::test_support::submit_settled(&mut exact, packet(doc.scene(), extent));
    let expected = pixels(&exact, crate::test_support::document_texture(&exact));
    let mut r = crate::memory::memory_tests::renderer(doc.composition().color,
        layer_core::DeviceMemoryFacts { unified: true, ..Default::default() }).unwrap();
    let mut frame = packet(doc.scene(), extent);
    frame.composite_all = false;
    frame.view = crate::test_support::view([extent[0] / 4, extent[1] / 4]);
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    submit_published(&mut r, frame);
    evict_view(&r);
    assert!(r.next_frame()==layer_render::FrameDemand::Work, "the evicted Fit view streams its exact cells again");
    crate::test_support::complete(&r);
    crate::memory::memory_tests::lower_memory_to_floor(&mut r);
    for _ in 0..(page_coordinates(PixelRect::full(extent)).count() * 4).div_ceil(SOURCE_SLOTS) + 1 {
        r.publish_memory();
        r.factory.memory_budget().flush_reports();
        r.refresh_memory_limits();
        r.shader_idle(true, true);
        r.after_present();
        crate::test_support::complete(&r);
    }
    assert!(r.source_tiles.borrow().free_slots() >= SOURCE_SLOTS + 4);
    let pages = page_coordinates(PixelRect::full(extent)).count();
    crate::scene::sources::Inputs::begin_accepted_reads();
    let mut completed_batches = Vec::new();
    let mut frames = 0;
    for _ in 0..pages * 4 + 8 {
        if r.next_frame()!=layer_render::FrameDemand::Work { break; }
        crate::test_support::complete(&r);
        crate::test_support::settle_decodes(&mut r);
        let before = covered_document_pages(r.scale_display.as_ref().unwrap(),pages::Accuracy::Exact).count();
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        frames += 1;
        let after = covered_document_pages(r.scale_display.as_ref().unwrap(),pages::Accuracy::Exact).count();
        if after > before { completed_batches.push(after - before); }
    }
    assert!(r.next_frame()!=layer_render::FrameDemand::Work, "idle refinement must finish; frames={frames}, completed batches={completed_batches:?}");
    assert!(!completed_batches.is_empty(), "idle frames complete at least one native page");
    let cache = r.scale_display.as_ref().unwrap();
    assert!(quality(&display_pixels(&r), &expected, cache.plan, cache.texture().format())[2] < 2e-5);
    assert_eq!(doc.artwork, authored);
    let reads=crate::scene::sources::Inputs::take_accepted_read_results();
    assert!(reads.iter().map(|(count,_)|count).sum::<usize>()>0,"cold exact refinement must consume decoded sources");
    assert!(reads.iter().all(|(_,unexpected)|unexpected.is_empty()),"{reads:?}");
}

#[test]
fn settled_photo_pages_survive_warm_zoom_and_pan() {
    use layer_core::color::{DocumentColor, RgbSpace};
    let extent = [6000, 4000];
    let mut doc = document_at(extent);
    composition_mut(&mut doc).color = DocumentColor {
        space: RgbSpace::ProPhoto, depth: SampleDepth::U16,
    };
    composition_mut(&mut doc).blend = layer_core::BlendSpace::Linear;
    for _ in 0..31 {
        let paint = paint_occurrence(&mut doc, "Empty", None);
        insert_occurrence(&mut doc, paint, 1);
    }
    for (name, key, value) in [
        ("exposure", "exposure", 0.25),
        ("white_balance", "temperature", 4.),
        ("levels", "gamma", 1.08),
        ("hue_saturation", "saturation", 5.),
        ("color_balance", "midtones_red", 2.),
    ] {
        let mut effect = layer_core::EffectInstance::new(
            layer_core::bundled_effect_catalog().get(name).unwrap().program());
        effect.set(key, layer_core::EffectValue::Number(value)).unwrap();
        let occurrence = effect_occurrence(&mut doc, effect, name);
        insert_occurrence(&mut doc, occurrence, 0);
    }
    let mut r = crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    let mut frame = packet(doc.scene(), extent);
    frame.composite_all = false;
    frame.view = crate::test_support::view([1500, 1000]);
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    crate::test_support::submit_settled(&mut r, frame);
    let fitted = display_pixels(&r);
    assert!(fitted.iter().all(|pixel| pixel.iter().all(|value| value.is_finite())));
    assert!(fitted.iter().any(|pixel| pixel[3] > 0.99));
    assert_presentation_mip(&r);
    assert!(r.scale_display.as_ref().unwrap().retained_ready());
    let views=[(0.5,-700.,-400.),(0.5,-760.,-440.),(0.25,0.,0.)];
    for (zoom,x,y) in views {
        frame.view.document_to_surface=[zoom,0.,0.,zoom,x,y];
        crate::test_support::submit_settled(&mut r,frame);
        assert!(r.scale_display.as_ref().unwrap().retained_ready());
        assert_presentation_mip(&r);
    }
    let work=r.metrics.composited_pixels;
    let reads=r.source_cache_work();
    for (zoom,x,y) in views {
        frame.view.document_to_surface = [zoom, 0., 0., zoom, x, y];
        crate::test_support::submit_settled(&mut r, frame);
        assert_eq!(r.metrics.composited_pixels, work,
            "unchanged navigation samples already committed photo pages");
        assert_eq!(r.source_cache_work(),reads,"warm revisits neither read nor re-upload source pages");
        assert_presentation_mip(&r);
    }
    assert!(crate::test_support::max_error(&display_pixels(&r), &fitted) < 2e-5);
}

#[test]
fn settled_composition_reuses_every_zoom_and_refines_only_changed_pages() {
    let mut doc = document_at([769, 513]);
    let extent = doc.composition().size;
    let mut r = crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut frame = packet(doc.scene(), extent);
    frame.composite_all = false;
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    submit_published(&mut r, frame); submit_published(&mut exact, frame);
    let drawing_target = r.scale_display.as_ref().unwrap().texture().clone();
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    assert_eq!(r.scale_display.as_ref().unwrap().texture(), &drawing_target, "settling repairs the drawing texture in place");
    assert!(r.scale_display.as_ref().unwrap().retained_ready());
    let cache = r.scale_display.as_ref().unwrap();
    let strip = cache.exact_tile.as_ref().expect("refinement uses a bounded render attachment");
    assert_eq!([strip.texture.width(), strip.texture.height()], exact_strip(extent));
    assert!(retained_root_pages(cache).iter().all(|page|page.texture!=strip.texture));
    let zooms=[0.5,1.,2.,0.25,0.125];
    for zoom in zooms {
        frame.view.document_to_surface=[zoom,0.,0.,zoom,-40.,-20.];
        crate::test_support::submit_ready(&mut r,frame).unwrap();
        assert_settled(&mut r,frame,&pixels(&exact,crate::test_support::document_texture(&exact)));
    }
    let work=r.metrics.composited_pixels;
    let reads=r.source_cache_work();
    for zoom in zooms {
        frame.view.document_to_surface = [zoom, 0., 0., zoom, -40., -20.];
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        assert!(r.scale_display.as_ref().unwrap().retained_ready());
        assert_eq!(r.source_cache_work(),reads,"a warmed level cannot reload sources");
        assert_eq!(r.metrics.composited_pixels, work, "camera motion only samples settled pixels");
        assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        assert_presentation_mip(&r);
    }
    let drawing_target = r.scale_display.as_ref().unwrap().texture().clone();
    let target = source_at(&doc, 0);
    let dab = crate::tests::test_dab([90., 80.], [0.9, 0.1, 0.3, 1.], 0.8);
    let batch = dab_batch(target, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
    let view = frame.view;
    doc.apply(layer_core::Edit::SetRaster { target, revision: layer_core::raster::RasterRevision::pending() }).unwrap();
    frame = FramePacket { view, composite_all: false, ..packet(doc.scene(), extent) };
    crate::test_support::submit_ready(&mut r, FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
    exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    submit_published(&mut exact, frame);
    assert_eq!(r.scale_display.as_ref().unwrap().texture(), &drawing_target, "drawing updates the resident level");
    let cache=r.scale_display.as_ref().unwrap();
    let coverage=cache.coverage_snapshot();
    assert!(page_coordinates(cache.plan.bounds).filter(|coordinate|*coordinate!=[0,0])
        .all(|coordinate|pages::Coverage::covers(&coverage.exact,page_rect(coordinate).intersect(cache.plan.bounds))),"only the damaged page may lose exactness");
    let work = r.metrics.composited_pixels;
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    assert!(r.metrics.composited_pixels-work<=u64::from(PAGE_SIZE).pow(2),"idle repair stays within the one touched page");
    assert_eq!(r.scale_display.as_ref().unwrap().texture(), &drawing_target, "dirty repair preserves the presentation binding");
    frame.view.document_to_surface = [1., 0., 0., 1., 0., 0.];
    crate::test_support::submit_ready(&mut r, frame).unwrap();
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    let mut preview = layer_render::TransformPreview { transaction: 1, target: source_at(&doc, 0),
        moving: true, selection: None, transform: layer_core::ImageTransform::affine(Affine::translation(Point { x: 23., y: 17. })) };
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    for moving in [true, false] {
        preview.moving = moving;
        for renderer in [&mut r, &mut exact] { renderer.set_transform_preview(Some(&preview)).unwrap(); crate::test_support::submit_ready(renderer, frame).unwrap(); }
        if !moving { assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact))); }
        crate::test_support::assert_same_canonical(&mut r, &mut exact);
    }
    for renderer in [&mut r, &mut exact] { renderer.set_transform_preview(None).unwrap(); crate::test_support::submit_ready(renderer, frame).unwrap(); }
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    frame.view.width_px = 128;
    frame.view.height_px = 96;
    frame.view.document_to_surface = [1., 0., 0., 1., -40., -20.];
    for renderer in [&mut r, &mut exact] { crate::test_support::submit_ready(renderer, frame).unwrap(); }
    let view = frame.view;
    doc.apply(layer_core::Edit::SetRaster { target, revision: layer_core::raster::RasterRevision::pending() }).unwrap();
    frame = FramePacket { view, composite_all: false, ..packet(doc.scene(), extent) };
    crate::test_support::submit_ready(&mut r, FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
    exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    submit_published(&mut exact, frame);
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    let mut program = (*crate::tests::fixture("gaussian_blur").program()).clone();
    program.resolution = layer_core::EffectResolution::Native;
    let effect=effect_occurrence(&mut doc,EffectInstance::new(Arc::new(program)),"effect");
    insert_occurrence(&mut doc,effect,0);
    frame = packet(doc.scene(), extent);
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    submit_published(&mut r, frame); submit_published(&mut exact, frame);
    assert!(!r.scale_display.as_ref().unwrap().has_pending_work(&r));
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
}
use layer_core::{Affine, EffectInstance, EffectValue, Point};

#[test]
fn global_filters_evict_optional_levels_before_rejecting_the_document() {
    let mut doc = document_at([65, 33]);
    composition_mut(&mut doc).size=[2561;2];
    paint_mut(&mut doc,0).domain = [2561;2];
    let front=copy_paint(&mut doc,0);
    doc.artwork.occurrences.get_mut(front).unwrap().opacity=0.5;
    insert_occurrence(&mut doc,front,0);
    let extent = doc.composition().size;
    let mut r = crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    let mut frame = packet(doc.scene(), extent);
    frame.composite_all = false;
    frame.view.width_px=extent[0];frame.view.height_px=extent[1];
    frame.view.document_to_surface=[1.,0.,0.,1.,0.,0.];
    r.begin_frame_memory(frame);
    submit_published(&mut r, frame);
    for _ in 0..=page_coordinates(PixelRect::full(extent)).count() {
        if r.next_frame()!=layer_render::FrameDemand::Work { break; }
        r.wait_idle().unwrap();
        crate::test_support::submit_ready(&mut r, frame).unwrap();
    }
    assert!(r.next_frame()!=layer_render::FrameDemand::Work);
    let cache=r.scale_display.as_ref().unwrap();
    let old_root=cache.graph.root.clone().unwrap();
    let resident=cache.pages.lock().unwrap().bytes();
    assert!(resident>0);
    let mut program = (*crate::tests::fixture("gaussian_blur").program()).clone();
    program.resolution = layer_core::EffectResolution::Native;
    program.passes = vec![layer_core::EffectPass { entry: program.entry.clone(), sampling: layer_core::EffectSampling::Document }].into();
    let effect=effect_occurrence(&mut doc,EffectInstance::new(Arc::new(program)),"effect");
    insert_occurrence(&mut doc,effect,0);
    frame = packet(doc.scene(), extent);
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    let _budget = crate::memory::memory_tests::display_frame_with_budget(&mut r, frame, 256 << 20);
    assert_eq!(r.image_pixel_budget(0),256 << 20);
    assert!(windows::Plan::new(doc.scene(), extent, r.image_pixel_budget(resident), r.device.limits().max_texture_dimension_2d).is_err());
    assert!(windows::Plan::new(doc.scene(), extent, r.image_pixel_budget(0), r.device.limits().max_texture_dimension_2d).is_ok());
    submit_published(&mut r,frame);
    let kept=r.scale_display.as_ref().unwrap().pages.lock().unwrap().retained_pages().into_iter().filter(|page|page.key.expression==old_root).count();
    let bytes=r.scale_display.as_ref().unwrap().pages.lock().unwrap().bytes();
    assert!(bytes<=256<<20,"global work keeps {kept} old-content pages only beside the drawing within its budget: {bytes}");
    assert!(!r.scale_display.as_ref().unwrap().has_pending_work(&r));
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    submit_published(&mut exact, frame);
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
}

#[test]
fn native_filter_windows_share_display_storage_and_preserve_halos_during_navigation() {
    for space in layer_core::BlendSpace::ALL {
        let mut doc = document_at([1541, 771]);
        let extent = doc.composition().size;
        let mut program = (*crate::tests::fixture("gaussian_blur").program()).clone();
        program.resolution = layer_core::EffectResolution::Native;
        let mut instance = EffectInstance::new(Arc::new(program));
        instance.set("sigma", EffectValue::Number(7.)).unwrap();
        let effect=effect_occurrence(&mut doc,instance,"effect");
        insert_occurrence(&mut doc,effect,0);
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        r.native_edit.as_mut().unwrap().image_pixel_bytes = Some(8 * 1024 * 1024);
        let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        exact.test.reference = true;
        for (zoom, x, opacity) in [(0.5, -192., 0.8), (0.5, -384., 0.8), (1., -512., 0.8), (1., -512., 0.4), (0.125, 0., 0.4)] {
            occurrence_mut(&mut doc,1).opacity = opacity;
            let mut frame = packet(doc.scene(), extent);
            frame.blend_space = space;
            frame.view.width_px = 96;
            frame.view.height_px = 64;
            frame.view.document_to_surface = [zoom, 0., 0., zoom, x, -32.];
            submit_published(&mut r, frame); submit_published(&mut exact, frame);
            assert!(r.scale_display.as_ref().unwrap().evaluation == Evaluation::Native);
            assert!(!r.scale_display.as_ref().unwrap().has_pending_work(&r), "native filters publish complete exact output");
            assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        }
    }
}

#[test]
fn watercolor_uses_the_display_graph_across_preview_commit_and_zoom() {
    for space in layer_core::BlendSpace::ALL {
        let doc = document();
        let extent = doc.composition().size;
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        exact.test.reference = true;
        let mut frame = packet(doc.scene(), extent);
        frame.blend_space = space;
        frame.composite_all = false;
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        for renderer in [&mut r, &mut exact] { crate::test_support::submit_published(renderer, frame); }
        for (step, kind) in [DabBatchKind::Persistent, DabBatchKind::Preview, DabBatchKind::Persistent].into_iter().enumerate() {
            let mut dab = crate::tests::test_dab([252. + step as f32 * 20., 128.], [0.2, 0.3, 0.8, 0.8], 0.8);
            dab.radii = [90.; 2];
            dab.material = [0.5, 0.8, 1., 0.8];
            let mut batch = dab_batch(source_at(&doc, 0), crate::layer_tests::preset_style(DefaultBrushPreset::WatercolorWash), dab.bounds());
            batch.kind = kind;
            batch.stroke_start = step == 0;
            batch.stroke_end = step == 2;
            let stroke = FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame };
            for renderer in [&mut r, &mut exact] { crate::test_support::submit_published(renderer, stroke); }
            assert!(r.paint_layers[0].watercolor.is_some());
            assert!(r.scale_display.is_some(), "watercolor stays in the common display cache");
            let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), r.scale_display.as_ref().unwrap().plan, r.scale_display.as_ref().unwrap().texture().format());
            assert!(error[0] < 0.002 && error[1] < 0.015, "watercolor {space:?} {kind:?}: {error:?}");
            if kind == DabBatchKind::Preview {
                for renderer in [&mut r, &mut exact] { crate::test_support::submit_published(renderer, frame); }
            }
            assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        }
        for zoom in [1., 0.5, 0.125] {
            frame.view.document_to_surface = [zoom, 0., 0., zoom, 0., 0.];
            for renderer in [&mut r, &mut exact] { crate::test_support::submit_published(renderer, frame); }
            assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        }
    }
}

#[test]
fn idle_display_refines_an_overview_created_during_transform_motion() {
    let doc = document_at([1541, 771]);
    let extent = doc.composition().size;
    let mut preview = layer_render::TransformPreview { transaction: 1, target: source_at(&doc, 0),
        moving: true, selection: None, transform: layer_core::ImageTransform::affine(
            Affine::around(Point { x: 770., y: 385. }, [0.8, 0.9], 0.15, Point::default())) };
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut frame = packet(doc.scene(), extent);
    frame.composite_all = false;
    frame.view.width_px = 96;
    frame.view.height_px = 64;
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, -96., -32.];
    r.set_transform_preview(Some(&preview)).unwrap(); submit_published(&mut r, frame);
    assert!(r.next_frame()!=layer_render::FrameDemand::Work);
    preview.moving = false;
    r.set_transform_preview(Some(&preview)).unwrap(); submit_published(&mut r, frame);
    exact.set_transform_preview(Some(&preview)).unwrap(); crate::test_support::submit_settled(&mut exact, frame);
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
}

#[test]
fn animated_display_invalidates_retained_pixels_when_time_changes() {
    let mut doc = document();
    let extent = doc.composition().size;
    let mut program = crate::tests::with_time_controls((*crate::tests::fixture("exposure").program()).clone());
    program.id = "time_probe".into();
    program.entry = "time_probe".into();
    program.wgsl = "fn time_probe(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4(c.rgb*(.5+.1*fx_time(b)),c.a);}".into();
    let effect=effect_occurrence(&mut doc,EffectInstance::new(Arc::new(program)),"effect");
    insert_occurrence(&mut doc,effect,0);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut frame = packet(doc.scene(), extent);
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    submit_published(&mut r, frame); submit_published(&mut exact, frame);
    let original = display_pixels(&r);
    frame.composite_all = false;
    frame.time_seconds = 2.;
    crate::test_support::submit_ready(&mut r, frame).unwrap(); submit_published(&mut exact, frame);
    let animated = display_pixels(&r);
    assert!(original != animated, "animation changes retained display output");
    let error = quality(&animated, &pixels(&exact, crate::test_support::document_texture(&exact)), r.scale_display.as_ref().unwrap().plan, r.scale_display.as_ref().unwrap().texture().format());
    assert!(error[2] < 0.002, "animated display {error:?}");
    let dab = crate::tests::test_dab([30., 30.], [0.9, 0.1, 0.3, 1.], 0.8);
    let batch = dab_batch(source_at(&doc, 1), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
    frame.time_seconds = 3.;
    let painting = FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame };
    crate::test_support::submit_ready(&mut r, painting).unwrap(); submit_published(&mut exact, painting);
    let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), r.scale_display.as_ref().unwrap().plan, r.scale_display.as_ref().unwrap().texture().format());
    assert!(error[2] < 0.002, "animation advances outside the simultaneous paint contact {error:?}");
}

#[test]
fn idle_display_refines_paused_transform_windows_and_reuses_exact_overlap() {
    for space in layer_core::BlendSpace::ALL {
        let doc = document_at([1541, 771]);
        let extent = doc.composition().size;
        let preview = layer_render::TransformPreview { transaction: 1, target: source_at(&doc, 0), moving: false, selection: None,
            transform: layer_core::ImageTransform::affine(Affine::around(Point { x: 770., y: 385. }, [0.9, 0.8], 0.17, Point::default())) };
        let mut r = crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
        let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        exact.test.reference = true;
        for renderer in [&mut r, &mut exact] { renderer.set_transform_preview(Some(&preview)).unwrap(); }
        let mut frame = packet(doc.scene(), extent);
        frame.blend_space = space;
        frame.composite_all = false;
        frame.view.width_px = 96;
        frame.view.height_px = 64;
        for (x, y) in [(0., 0.), (-180., -60.), (-290., -120.), (0., 0.)] {
            frame.view.document_to_surface = [0.25, 0., 0., 0.25, x, y];
            working_scene_frame(&mut r, frame);
            submit_published(&mut r, frame); crate::test_support::submit_settled(&mut exact, FramePacket { composite_all: true, ..frame });
            assert!(r.scale_display.as_ref().unwrap().overview.is_some());
            assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        }
        frame.view.width_px = extent[0];
        frame.view.height_px = extent[1];
        working_scene_frame(&mut r, frame);
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        assert_keyed_presentation(&r);
        assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    }
}

#[test]
fn idle_display_reuses_global_effect_dependencies_until_the_next_edit() {
    let mut doc = document();
    let extent = doc.composition().size;
    let mut program = (*crate::tests::fixture("gaussian_blur").program()).clone();
    program.id = "global_probe".into();
    program.entry = "global_probe".into();
    program.wgsl = "fn global_probe(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return fx_sample(fx_extent()-p)*.6+fx_original(p)*.4;}".into();
    program.lookups = Arc::new([]);
    program.passes = vec![layer_core::EffectPass { entry: program.entry.clone(), sampling: layer_core::EffectSampling::Document }].into();
    let effect=effect_occurrence(&mut doc,EffectInstance::new(Arc::new(program)),"effect");
    insert_occurrence(&mut doc,effect,0);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    for opacity in [0.7, 0.4] {
        occurrence_mut(&mut doc,1).opacity = opacity;
        let mut frame = packet(doc.scene(), extent);
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        submit_published(&mut r, frame); submit_published(&mut exact, frame);
        let deadline=std::time::Instant::now()+READBACK_TIMEOUT;
        while r.scene.as_ref().unwrap().image_pass_pixels()==0 {
            crate::test_support::submit_ready(&mut r, FramePacket { composite_all: false, ..frame }).unwrap();
            r.wait_idle().unwrap();
            assert!(std::time::Instant::now()<deadline,"global effect work must start");
        }
        let work = r.scene.as_ref().unwrap().image_pass_pixels();
        assert!(work > 0);
        assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        assert_eq!(r.scene.as_ref().unwrap().image_pass_pixels(), u64::from(extent[0]) * u64::from(extent[1]), "unchanged global dependencies execute once across all requested pages");
        let work = r.scene.as_ref().unwrap().image_pass_pixels();
        crate::test_support::submit_ready(&mut r, FramePacket { composite_all: false, ..frame }).unwrap();
        assert_eq!(r.scene.as_ref().unwrap().image_pass_pixels(), work, "unchanged global dependencies execute once");
    }
}

#[test]
fn idle_display_refines_spatial_effects_and_invalidates_committed_paint() {
    for space in layer_core::BlendSpace::ALL {
        let mut doc = document();
        let extent = doc.composition().size;
        let paint = source_at(&doc, 0);
        let effect=effect_occurrence(&mut doc,EffectInstance::new(crate::tests::fixture("gaussian_blur").program()),"blur");
        set_effect_value(&mut doc,effect,"sigma",EffectValue::Number(7.));
        let mask=coverage_mask(&mut doc,effect,[0, 0],None);
        doc.artwork.coverage.get_mut(mask).unwrap().default_coverage=0.7;
        insert_occurrence(&mut doc,effect,0);
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        exact.test.reference = true;
        let mut frame = packet(doc.scene(), extent);
        frame.blend_space = space;
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        submit_published(&mut r, frame); submit_published(&mut exact, frame);
        assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        let dab = crate::tests::test_dab([254., 128.], [0.9, 0.1, 0.3, 1.], 0.8);
        let batch = dab_batch(paint, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        let stroke = FramePacket { composite_all: false, dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame };
        crate::test_support::submit_ready(&mut r, stroke).unwrap(); submit_published(&mut exact, stroke);
        assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    }
}

#[test]
fn published_clipped_paint_preserves_exact_pages_during_the_next_contact() {
    fn frame(doc: &Document) -> FramePacket<'_> {
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.composite_all = false;
        frame.blend_space = layer_core::BlendSpace::Perceptual;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        frame
    }
    fn settle(mut r: &mut WgpuRasterizer, doc: &Document) {
        for step in 0..=65 {
            if r.next_frame()!=layer_render::FrameDemand::Work { break; }
            assert!(step < 65);
            r.wait_idle().unwrap();
            crate::test_support::submit_ready(&mut r, frame(doc)).unwrap();
        }
        r.wait_idle().unwrap();
    }
    fn verify(r: &mut WgpuRasterizer, doc: &Document) {
        for tile in doc.scene().raster(source_at(doc, 0)).unwrap().wait_data().unwrap().tiles.values() { tile.wait_backing().unwrap(); }
        let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        exact.test.reference = true;
        submit_published(&mut exact, frame(doc));
        assert_settled(r, frame(doc), &pixels(&exact, crate::test_support::document_texture(&exact)));
    }
    let mut doc = document_at([2048; 2]);
    let paint = paint_occurrence(&mut doc, "clipped paint", None);
    insert_occurrence(&mut doc, paint, 0);
    occurrence_mut(&mut doc, 0).opacity = 0.35;
    set_attachment_at(&mut doc, 0, true);
    let target = source_at(&doc, 0);
    let mut r = crate::memory::memory_tests::renderer(doc.composition().color,
        layer_core::DeviceMemoryFacts { budget_bytes: Some(2 << 30), ..Default::default() }).unwrap();
    submit_published(&mut r, frame(&doc));
    settle(&mut r, &doc);
    let mut checkpoint = None;
    for (contact, position) in [[128., 128.], [1664., 1664.]].into_iter().enumerate() {
        doc.apply(layer_core::Edit::SetRaster { target, revision: layer_core::raster::RasterRevision::pending() }).unwrap();
        let mut dab = crate::tests::test_dab(position, [0.9, 0.1, 0.3, 1.], 0.8);
        dab.radii = [8.; 2];
        let mut batch = dab_batch(target, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        batch.stroke_id = layer_core::StrokeId(contact as u64 + 1);
        r.metrics.frame_composited_pages.clear();
        crate::test_support::submit_ready(&mut r, FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame(&doc) }).unwrap();
        assert_eq!(r.metrics.frame_composited_pages, vec![(2, [contact as u32 * 6; 2])]);
        assert_eq!(doc.scene().raster(target).unwrap().wait_data().unwrap().tiles.len(), contact + 1);
        if contact == 0 { checkpoint = Some(doc.scene().raster(target).unwrap().clone()); }
        settle(&mut r, &doc);
    }
    verify(&mut r, &doc);
    let checkpoint = checkpoint.unwrap();
    doc.apply(layer_core::Edit::SetRaster { target, revision: checkpoint.clone() }).unwrap();
    r.metrics.frame_composited_pages.clear();
    crate::test_support::submit_ready(&mut r, FramePacket { restore_rasters: &[(target, checkpoint)], ..frame(&doc) }).unwrap();
    assert!(r.metrics.frame_composited_pages.contains(&(2, [6, 6])), "restoration repairs the removed page");
    verify(&mut r, &doc);
}


#[test]
fn fit_refinement_accepts_native_input_quality_and_reaches_exact() {
    let extent=[256;2];let mut doc=crate::layer_tests::placement::paint_document(extent,"Fit native quality");
    crate::layer_tests::placement::set_source(&mut doc,rgba8_source(extent,|_,_|[31,71,113,255]));
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let view=layer_render::ViewState {width_px:64,height_px:64,document_to_surface:[0.25,0.,0.,0.25,0.,0.]};
    let frame=FramePacket {view,..packet(doc.scene(),extent)};
    for index in 0..32 {
        crate::test_support::submit_ready(&mut r,FramePacket {composite_all:index==0,..frame}).unwrap();
        crate::test_support::complete(&r);
        if r.scale_display.as_ref().is_some_and(Cache::output_complete) && r.next_frame()!=layer_render::FrameDemand::Work {break;}
    }
    let cache=r.scale_display.as_ref().unwrap();assert_eq!(cache.plan.level,2);
    assert!(cache.output_complete() && r.next_frame()!=layer_render::FrameDemand::Work,"a Fit request must finish its native exact producers");
    let mut exact=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();exact.test.reference=true;
    submit_published(&mut exact,packet(doc.scene(),extent));
    let expected=pixels(&exact,crate::test_support::document_texture(&exact));
    let key=cache.refinement_interest(cache.plan.level,PixelRect::full(extent)).key;
    let image=cache.pages.lock().unwrap().get(&key,key.plan().bounds,pages::Accuracy::Exact,0,crate::residency::Class::Visible);
    let image=image.expect("the native exact producer reduced to the view level");
    let actual=pixels(&r,&image.texture);let error=quality(&actual,&expected,image.plan,image.texture.format());
    assert!(error[2]<2e-5,"{error:?}: actual={:?}, expected={:?}",actual[0],expected[0]);
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn a_refused_refinement_takes_no_idle_frames_and_one_release_wakes_it() {
    let mut doc=document_at([1537,1025]);let extent=doc.composition().size;
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    let (send,woken)=std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move||{let _=send.send(());}));
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
    frame.view.document_to_surface=[0.125,0.,0.,0.125,0.,0.];
    let needs_exact=|r:&WgpuRasterizer|r.scale_display.as_ref().is_some_and(|cache|cache.coverage_snapshot().needs(cache.plan.bounds,pages::Accuracy::Exact));
    r.begin_frame_memory(frame);
    submit_published(&mut r,frame);
    crate::test_support::complete(&r);
    let display=r.scene_tiers.display.clone();
    let pages={
        let mut store=r.scale_display.as_ref().unwrap().pages.lock().unwrap();
        store.retained_pages().into_iter().map(|page|store.allocated(&page.key,crate::residency::Class::Visible).unwrap()).collect::<Vec<_>>()
    };
    let sources={
        let cache=r.source_tiles.borrow();let source=paint_at(&doc,0).base.as_ref().unwrap().image.storage();
        page_coordinates(PixelRect::full(extent)).filter_map(|coordinate|cache.prepared_view(source,coordinate).and_then(|view|cache.lease(view))).collect::<Vec<_>>()
    };
    assert!(!pages.is_empty() && !sources.is_empty(),"the fixture pins existing composite pages and decoded inputs");
    let held=(display.charge(display.grant().saturating_sub(display.used())),pages,sources);
    let owner=doc.scene().order()[0];
    let mut occurrence=doc.scene().occurrence(owner).unwrap().clone();occurrence.opacity=0.5;
    doc.apply(layer_core::Edit::Occurrence(layer_core::RecordChange::replace(&doc.artwork.occurrences,owner,Some(occurrence)).unwrap())).unwrap();
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
    frame.view.document_to_surface=[0.125,0.,0.,0.125,0.,0.];
    crate::test_support::submit_ready(&mut r,frame).unwrap();crate::test_support::complete(&r);
    for _ in 0..16 {
        if r.next_frame()!=layer_render::FrameDemand::Work {break;}
        crate::test_support::submit_ready(&mut r,frame).unwrap();crate::test_support::complete(&r);
    }
    assert!(needs_exact(&r),"refinement is refused while the grant is held");
    while woken.try_recv().is_ok() {}
    assert_ne!(r.next_frame(),layer_render::FrameDemand::Work,"a refused refinement requests no idle frame");
    assert!(woken.recv_timeout(std::time::Duration::from_millis(200)).is_err(),"a refused refinement is not woken before a release");
    drop(held);
    woken.recv_timeout(std::time::Duration::from_secs(1)).expect("the release wakes the refused refinement");
    assert!(woken.recv_timeout(std::time::Duration::from_millis(200)).is_err(),"one release wakes it once");
    assert_eq!(r.next_frame(),layer_render::FrameDemand::Work,"the woken refinement runs");
    for _ in 0..64 {
        if !needs_exact(&r) {break;}
        crate::test_support::submit_ready(&mut r,frame).unwrap();crate::test_support::complete(&r);
    }
    assert!(!needs_exact(&r),"the woken refinement reaches exact");
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn an_overview_refused_before_it_shows_a_cell_waits_for_the_release_instead_of_requesting_frames() {
    let extent=[2304;2];let doc=document_at(extent);
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
    frame.view.width_px=512;frame.view.height_px=512;
    frame.view.document_to_surface=[1.,0.,0.,1.,-896.,-896.];
    crate::test_support::submit_settled(&mut r,frame);
    let (send,woken)=std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move||{let _=send.send(());}));
    let objects=Arc::new(crate::residency::Tier::default());
    let refusal=objects.blocked(crate::residency::Class::Prefetch,1).expect("a full tier refuses a prefetch unit");
    r.wait_for_release(refusal.clone());
    let overview=r.scale_display.as_mut().unwrap().overview.as_mut().expect("the zoomed view keeps an overview");
    overview.pixels.take();overview.evict_window(overview.plan.bounds);overview.refuse(None,overview.plan.bounds,&refusal);
    let committed=|r:&WgpuRasterizer|r.scale_display.as_ref().unwrap().overview.as_ref().unwrap().pixels.committed_root().is_some();
    let mut audit=crate::frame_demand::DemandAudit::default();
    for step in 0..4 {
        audit.begin(&r);
        crate::test_support::submit_ready(&mut r,frame).unwrap();crate::test_support::complete(&r);
        audit.end(&r,format_args!("refused step={step}"));
    }
    assert!(!committed(&r),"an overview whose every cell is refused shows no cell and commits no window");
    while woken.try_recv().is_ok() {}
    assert_ne!(r.next_frame(),layer_render::FrameDemand::Work,"an overview with only parked cells requests no frame");
    objects.update_limit(u64::MAX);
    woken.recv_timeout(std::time::Duration::from_secs(1)).expect("the release wakes the parked overview");
    crate::test_support::submit_settled(&mut r,frame);
    assert!(committed(&r),"the woken overview shows its cells and commits its window");
}

#[test]
fn a_selected_layer_leaves_no_display_demand_that_its_frames_never_attempt() {
    for objects in [false,true] {
        let mut doc=document_at([1537,1025]);let extent=doc.composition().size;
        let ink=doc.scene().order()[0];
        let pixels=|x:u32,y:u32|[(y%251) as u8,90,(x%239) as u8,160];
        let above=if objects {
            let object=doc.artwork.objects.insert(PortableId::random(),ImageObject::new(layer_core::authored::Image::new(rgba8_source(extent,pixels)))).unwrap();
            doc.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(object),"Photo")).unwrap()
        } else {paint_occurrence(&mut doc,"Above",Some(rgba8_source(extent,pixels)))};
        set_root_entries(&mut doc,vec![ink,above]);
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        layer_render::CanvasRenderer::set_working_layer(&mut r,Some(ink));
        let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
        frame.view=layer_render::ViewState {width_px:512,height_px:384,document_to_surface:[1.,0.,0.,1.,-256.,-128.]};
        let mut audit=crate::frame_demand::DemandAudit::default();
        for evicted in [false,true] {
            if evicted {let cache=r.scale_display.as_ref().unwrap();evict_composite_pages(cache,&[cache.plan.bounds]);}
            let mut settled=false;
            for step in 0..256 {
                audit.begin(&r);
                crate::test_support::submit_ready(&mut r,FramePacket {composite_all:step==0,..frame}).unwrap();
                crate::test_support::complete(&r);
                audit.end(&r,format_args!("objects={objects}, evicted={evicted}, step={step}"));
                if r.next_frame()!=layer_render::FrameDemand::Work {settled=true;break;}
            }
            assert!(r.scale_display.as_ref().is_some_and(|cache|cache.overview.is_some()),"the zoomed view keeps an overview");
            assert!(settled,"objects={objects}, evicted={evicted}: the display settles with a selected layer between other layers");
        }
    }
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn exact_refinement_keeps_its_source_until_the_consuming_submission() {
    let extent=[PAGE_SIZE;2];
    let mut document=crate::layer_tests::placement::paint_document(extent,"Refinement source lifetime");
    let source=rgba8_source(extent,|_,_|[31,71,113,255]);
    crate::layer_tests::placement::set_source(&mut document,source.clone());
    let mut r=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    let view=layer_render::ViewState {width_px:64,height_px:64,document_to_surface:[0.25,0.,0.,0.25,0.,0.]};
    let frame=FramePacket {view,..packet(document.scene(),extent)};
    crate::test_support::submit_settled(&mut r,frame);
    crate::test_support::complete(&r);
    let cache=r.scale_display.as_ref().unwrap();
    let key=cache.refinement_interest(cache.plan.level,PixelRect::full(extent)).key;
    let color=[31.,71.,113.].map(|channel|layer_core::color::srgb_decode(channel/255.));
    let expected=vec![[color[0],color[1],color[2],1.];key.plan().size.into_iter().product::<u32>() as usize];
    assert!(r.source_tiles.borrow().prepared_view(&source,[0;2]).is_some());
    evict_composite_pages(cache,&[PixelRect::full(extent)]);
    let validation=r.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    let mut scene=r.scene.take().unwrap();
    scene.begin_frame();r.metrics.frame_composited_pages.clear();
    {let mut work=crate::test_support::work(false);scene.refine_display(&mut r,FramePacket {composite_all:false,..frame},&mut encoder,&mut work).unwrap();}
    r.scene=Some(scene);
    assert!(r.metrics.frame_composited_pages.contains(&(0,[0;2])),"the real refinement path must encode a new native page");
    assert!(encoder.has_work());
    r.uploads.finish(&encoder);
    let image=r.scale_display.as_ref().unwrap().pages.lock().unwrap().get(&key,key.plan().bounds,pages::Accuracy::Exact,0,crate::residency::Class::Visible)
        .expect("refinement must commit the replacement page before source pressure");
    crate::test_support::evict_sources(&mut r);
    encoder.submit(&r.queue);crate::test_support::complete(&r);
    let error=pollster::block_on(validation.pop());
    assert!(error.is_none(),"source pressure invalidated an encoded refinement before submission: {error:?}");
    assert!(crate::test_support::max_error(&pixels(&r,&image.texture),&expected)<2e-5);
}

#[test]
fn fit_exact_producer_survives_shared_preparation_while_its_store_read_waits() {
    fn attempt(cache:&mut Cache,scene:&mut Scene,r:&mut WgpuRasterizer,frame:FramePacket<'_>,encoder:&mut submission::CommandEncoder,commands:&mut RegionRecords)->Result<PixelRect,GpuRasterError> {
        let write=scene.begin_write(r,encoder)?;cache.submission_valid=Some(scene.valid.clone());
        let result={let mut work=crate::test_support::work(false);cache.refine(scene,r,frame,encoder,commands,&mut work)};
        if matches!(&result,Err(error) if error.is_preparation_pending()) {scene.clear_unencoded_jobs();}
        if result.is_ok() || matches!(&result,Err(error) if error.is_preparation_pending()) {write.track(encoder);}
        result
    }
    for (mode,evaluation) in [("Display",Evaluation::Display),("Native",Evaluation::Native),("Tiled",Evaluation::Tiled(2))] {
    let extent=[256;2];let mut doc=crate::layer_tests::placement::paint_document(extent,"Held Fit exact producer");
    let source=rgba8_source(extent,|_,_|[31,71,113,255]);crate::layer_tests::placement::set_source(&mut doc,source.clone());
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let gate=crate::test_support::GatedStore::new(&layer_core::raster_storage::RetainedTiles::from_sources([source.as_ref()]),false);
    let view=layer_render::ViewState {width_px:64,height_px:64,document_to_surface:[0.25,0.,0.,0.25,0.,0.]};
    let frame=FramePacket {view,..packet(doc.scene(),extent)};
    crate::test_support::submit_ready(&mut r,frame).unwrap();crate::test_support::complete(&r);gate.gate.entered();
    let frame=FramePacket {composite_all:false,..frame};
    let mut cache=r.scale_display.take().unwrap();let mut scene=r.scene.take().unwrap();
    let plan=cache.plan;(cache,_)=Cache::select(Some(cache),&r,frame,Request {plan,evaluation});
    prepare_fixture_frame(&r,frame,&scene);cache.prepare_graph(&r,frame,&scene).unwrap();
    let mut commands=scene.scale_commands.take().unwrap_or_else(||RegionRecords::new(&r));
    let key=cache.refinement_interest(0,PixelRect::full(extent)).key;
    let region=key.plan().bounds;
    for _ in 0..3 {
        scene.begin_frame();commands.begin();
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        let result=attempt(&mut cache,&mut scene,&mut r,frame,&mut encoder,&mut commands);
        assert!(result.is_ok() || matches!(result,Err(GpuRasterError::NotReady|GpuRasterError::SourceWorkingSetExceeded)),"{result:?}");
        assert!(cache.pages.lock().unwrap().accepted(&key,region,0),"a delayed exact native producer must have accepted its input");
        prepare_fixture_frame(&r,frame,&scene);cache.prepare_graph(&r,frame,&scene).unwrap();
        cache.retain_shared_work(&r,frame,&scene,&r.navigator);
        assert!(cache.pages.lock().unwrap().accepted(&key,region,0),"common preparation must retain the still-wanted finer F32 producer");
        assert!(cache.pages.lock().unwrap().get(&key,region,pages::Accuracy::Exact,0,crate::residency::Class::Visible).is_none());
        r.uploads.finish(&encoder);if encoder.has_work() {encoder.submit(&r.queue);crate::test_support::complete(&r);}
    }
    gate.gate.release();
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    crate::test_support::query_ready(&mut r,&mut encoder,|r,encoder| {
        scene.begin_frame();commands.begin();
        attempt(&mut cache,&mut scene,r,frame,encoder,&mut commands)?;
        cache.copy_level(&scene,r,encoder,&mut commands)?;
        if cache.output_complete() && cache.pixels.committed_root().is_some() {Ok(())}else {Err(GpuRasterError::NotReady)}
    });
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    let shown=cache.refinement_interest(cache.plan.level,PixelRect::full(extent)).key;
    let image=cache.pages.lock().unwrap().get(&shown,shown.plan().bounds,pages::Accuracy::Exact,0,crate::residency::Class::Visible);
    let image=image.expect("the delayed native exact producer reduced to the view level");
    let mut exact=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();exact.test.reference=true;
    submit_published(&mut exact,packet(doc.scene(),extent));
    let expected=pixels(&exact,crate::test_support::document_texture(&exact));
    let actual=pixels(&r,&image.texture);let error=quality(&actual,&expected,image.plan,image.texture.format());
    assert!(error[2]<2e-5,"{mode}: {error:?}: actual={:?}, expected={:?}",actual[0],expected[0]);
    }
}

fn settle_refinement(r:&mut WgpuRasterizer,doc:&Document,extent:[u32;2],context:&str) {
    let frame=FramePacket {composite_all:false,..packet(doc.scene(),extent)};
    let mut idle=0;
    for step in 0..256 {
        let before=crate::test_support::PassCounts::snapshot();
        crate::test_support::submit_ready(r,FramePacket {composite_all:step==0,..frame}).unwrap();crate::test_support::complete(r);
        if r.next_frame()!=layer_render::FrameDemand::Work {return;}
        let committed=crate::test_support::PassCounts::snapshot().delta(before).composite_commits!=0;
        if !committed && r.image_decode_waiting() {crate::test_support::settle_decodes(r);continue;}
        idle=if committed {0}else {idle+1};
        assert!(idle<16,"{context}: {idle} Work frames in a row committed nothing: {:?}",r.outstanding_terms());
    }
    panic!("{context}: the display never settled: {:?}",r.outstanding_terms());
}

fn refine_frame(r:&mut WgpuRasterizer,frame:FramePacket<'_>,spent:bool) {
    let mut cache=r.scale_display.take().unwrap();let mut scene=r.scene.take().unwrap();
    let mut commands=scene.scale_commands.take().unwrap_or_else(||RegionRecords::new(r));
    scene.begin_frame();commands.begin();
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    let write=scene.begin_write(r,&mut encoder).unwrap();cache.submission_valid=Some(scene.valid.clone());
    let mut budget=crate::test_support::work(false);if spent {budget.spend(budget.remaining());}
    cache.refine(&mut scene,r,frame,&mut encoder,&mut commands,&mut budget).unwrap();
    write.track(&encoder);
    scene.scale_commands=Some(commands);r.scale_display=Some(cache);r.scene=Some(scene);
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(r);
}

fn fold_in_flight(r:&mut WgpuRasterizer,frame:FramePacket<'_>)->Vec<pages::Key> {
    (0..8).find_map(|_| {
        refine_frame(r,frame,true);
        let cache=r.scale_display.as_ref().unwrap();
        let folds:Vec<_>=cache.streamed_interests(frame.view).map(|interest|interest.key).filter(|key|cache.graph.content.as_ref()==Some(&key.expression)).collect();
        if folds.is_empty() {crate::test_support::settle_decodes(r);r.wait_idle().unwrap();None}else {Some(folds)}
    }).expect("a spent frame leaves a fold unit in flight once its inputs are decoded")
}

fn translucent_layers(doc:&mut Document,extent:[u32;2],count:u32)->Vec<OccurrenceHandle> {
    (0..count).map(|index| {
        let handle=paint_occurrence(doc,"Layer",Some(rgba8_source(extent,move|x,y|[((x+index*7)%251) as u8,((y+index*13)%241) as u8,(index*11) as u8,255])));
        doc.artwork.occurrences.get_mut(handle).unwrap().opacity=0.2+index as f32*0.03;handle
    }).collect()
}

fn edited_pivot_with_evicted_cells(doc:&mut Document,extent:[u32;2],pivot:OccurrenceHandle)->(WgpuRasterizer,Vec<PixelRect>) {
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    layer_render::CanvasRenderer::set_working_layer(&mut r,Some(pivot));
    settle_refinement(&mut r,doc,extent,"open");
    let index=doc.scene().order().iter().position(|&handle|handle==pivot).unwrap();
    paint_mut(doc,index).base=Some(layer_core::authored::PaintBase::new(rgba8_source(extent,|x,y|[(x%233) as u8,(y%229) as u8,40,200]).into()));
    settle_refinement(&mut r,doc,extent,"edit");
    let cells:Vec<_>={let cache=r.scale_display.as_ref().unwrap();pages::coordinates(cache.plan.bounds,cache.plan.level).map(|coordinate|pages::bounds(coordinate,cache.plan.level,cache.plan.extent)).collect()};
    assert!(cells.len()>=2 && r.scale_display.as_ref().unwrap().plan.level==0,"the fixture refines two cells at level 0");
    evict_composite_pages(r.scale_display.as_ref().unwrap(),&cells);
    (r,cells)
}

#[test]
fn a_fold_unit_in_flight_whose_half_is_gone_admits_the_half_and_settles() {
    let extent=[512,256];let mut doc=document_at(extent);
    let below=translucent_layers(&mut doc,extent,18);
    let top=paint_occurrence(&mut doc,"Top",Some(rgba8_source(extent,|x,y|[(y%251) as u8,90,(x%239) as u8,160])));
    set_root_entries(&mut doc,std::iter::once(top).chain(below).collect());
    let (mut r,cells)=edited_pivot_with_evicted_cells(&mut doc,extent,top);
    let frame=FramePacket {composite_all:false,..packet(doc.scene(),extent)};
    let folds=fold_in_flight(&mut r,frame);
    let cache=r.scale_display.as_ref().unwrap();let halves=cache.pivot_halves();
    cache.pages.lock().unwrap().trim(0,crate::residency::Class::Fallback);
    for half in folds.iter().flat_map(|key|halves.iter().map(move|half|pages::Key {expression:half.clone(),..key.clone()})) {
        assert!(cache.pages.lock().unwrap().get(&half,half.plan().bounds,pages::Accuracy::Exact,0,crate::residency::Class::Refinement).is_some(),"a trim keeps the halves of a fold in flight");
    }
    pages::evict_composite_pages_where(cache,&cells,|key|halves.contains(&key.expression));
    settle_refinement(&mut r,&doc,extent,"half evicted under a unit in flight");
}

#[test]
fn selecting_another_layer_releases_the_units_admitted_for_the_previous_pivot() {
    let extent=[512,256];let mut doc=document_at(extent);
    let mut entries=translucent_layers(&mut doc,extent,18);
    let pivot=paint_occurrence(&mut doc,"Pivot",Some(rgba8_source(extent,|x,y|[(y%251) as u8,90,(x%239) as u8,160])));
    entries.insert(entries.len()/2,pivot);
    set_root_entries(&mut doc,entries.clone());
    let (mut r,_)=edited_pivot_with_evicted_cells(&mut doc,extent,pivot);
    let frame=FramePacket {composite_all:false,..packet(doc.scene(),extent)};
    refine_frame(&mut r,frame,true);
    let (halves,admitted)={
        let cache=r.scale_display.as_ref().unwrap();let halves=cache.pivot_halves();
        let admitted:Vec<_>=cache.streamed_interests(frame.view).map(|interest|interest.key).filter(|key|halves.contains(&key.expression)).collect();
        (halves,admitted)
    };
    assert!(halves.len()==2 && !admitted.is_empty(),"the spent frame leaves a half of the edited pivot admitted");
    layer_render::CanvasRenderer::set_working_layer(&mut r,Some(entries[0]));
    settle_refinement(&mut r,&doc,extent,"another layer selected");
    {
        let cache=r.scale_display.as_ref().unwrap();
        assert!(cache.pivot_halves().iter().all(|half|!halves.contains(half)),"the selection moves the pivot");
        let flying:Vec<_>=cache.streamed_interests(frame.view).map(|interest|interest.key).collect();
        assert!(admitted.iter().all(|key|!flying.contains(key)),"a unit admitted for the previous pivot is still in flight: {:?}",flying.iter().map(|key|key.coordinate).collect::<Vec<_>>());
    }
    crate::test_support::submit_ready(&mut r,FramePacket {composite_all:false,..packet(doc.scene(),extent)}).unwrap();crate::test_support::complete(&r);
    let pages=r.scale_display.as_ref().unwrap().pages.lock().unwrap();
    assert!(admitted.iter().all(|key|!pages.accepted(key,key.plan().bounds,0)),"the next frame releases the inputs of the dropped unit");
}

#[test]
fn a_unit_in_flight_reserves_no_ancestor_above_the_level_it_reduces_into() {
    let extent=[2048,1024];let mut doc=document_at(extent);
    let below=translucent_layers(&mut doc,extent,18);
    let top=paint_occurrence(&mut doc,"Top",Some(rgba8_source(extent,|x,y|[(y%251) as u8,90,(x%239) as u8,160])));
    set_root_entries(&mut doc,std::iter::once(top).chain(below).collect());
    let (mut r,_)=edited_pivot_with_evicted_cells(&mut doc,extent,top);
    let frame=FramePacket {composite_all:false,..packet(doc.scene(),extent)};
    refine_frame(&mut r,frame,true);
    let reduced=|cache:&Cache|cache.plan.level.max(cache.overview.as_ref().map_or(0,|overview|overview.plan.level));
    let navigator=r.navigator.cache().map_or(0,reduced);
    let cache=r.scale_display.as_ref().unwrap();let level=reduced(cache);
    assert!(navigator>level,"the Navigator reduces to level {navigator}, deeper than the view's {level}");
    assert!(cache.streamed_interests(frame.view).next().is_some(),"the spent frame leaves a unit in flight");
    let reserved:Vec<_>={let store=cache.pages.lock().unwrap();store.retained_pages().into_iter().filter(|page|page.key.source.is_none() && page.key.level>level && page.written.is_empty() && store.pinned(&page.key))
        .map(|page|(page.key.level,page.key.coordinate)).collect()};
    assert!(reserved.is_empty(),"a view unit in flight holds unwritten ancestor pages above the level {level} it reduces into: {reserved:?}");
}

#[test]
fn selecting_another_layer_lets_the_previous_pivots_halves_yield() {
    let extent=[512,256];let mut doc=document_at(extent);
    let mut entries=translucent_layers(&mut doc,extent,18);
    let pivot=paint_occurrence(&mut doc,"Pivot",Some(rgba8_source(extent,|x,y|[(y%251) as u8,90,(x%239) as u8,160])));
    entries.insert(entries.len()/2,pivot);
    set_root_entries(&mut doc,entries.clone());
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    layer_render::CanvasRenderer::set_working_layer(&mut r,Some(pivot));
    settle_refinement(&mut r,&doc,extent,"open");
    let index=doc.scene().order().iter().position(|&handle|handle==pivot).unwrap();
    paint_mut(&mut doc,index).base=Some(layer_core::authored::PaintBase::new(rgba8_source(extent,|x,y|[(x%233) as u8,(y%229) as u8,40,200]).into()));
    settle_refinement(&mut r,&doc,extent,"edit");
    let halves=r.scale_display.as_ref().unwrap().pivot_halves();
    let built:Vec<_>={let store=r.scale_display.as_ref().unwrap().pages.lock().unwrap();
        store.retained_pages().into_iter().filter(|page|page.key.source.is_none() && halves.contains(&page.key.expression) && !page.written.is_empty()).map(|page|page.key).collect()};
    assert!(halves.len()==2 && !built.is_empty(),"the edited pivot's halves are built");
    layer_render::CanvasRenderer::set_working_layer(&mut r,Some(entries[0]));
    settle_refinement(&mut r,&doc,extent,"another layer selected");
    let cache=r.scale_display.as_ref().unwrap();
    assert!(cache.pivot_halves().iter().all(|half|!halves.contains(half)),"the selection moves the pivot");
    let (stale,yielding)={let store=cache.pages.lock().unwrap();
        (store.retained_pages().into_iter().filter(|page|built.contains(&page.key) && !store.pinned(&page.key)).map(|page|page.key.bytes()).sum::<u64>(),store.tier().yielding())};
    assert!(stale>0,"the previous pivot's halves are still retained");
    pages::evict_composite_pages_where(cache,&[cache.plan.bounds],|key|built.contains(key));
    let released=yielding-cache.pages.lock().unwrap().tier().yielding();
    assert_eq!(released,stale,"the previous pivot's halves yield their {stale} bytes to a refusal once the selection moves");
}

#[test]
fn a_fold_unit_whose_half_is_refused_holds_only_its_committed_halves() {
    let extent=[512,256];let mut doc=document_at(extent);
    let below=translucent_layers(&mut doc,extent,18);
    let top=paint_occurrence(&mut doc,"Top",Some(rgba8_source(extent,|x,y|[(y%251) as u8,90,(x%239) as u8,160])));
    set_root_entries(&mut doc,std::iter::once(top).chain(below).collect());
    let (mut r,cells)=edited_pivot_with_evicted_cells(&mut doc,extent,top);
    let frame=FramePacket {composite_all:false,..packet(doc.scene(),extent)};
    let folds=fold_in_flight(&mut r,frame);
    let cache=r.scale_display.as_ref().unwrap();let halves=cache.pivot_halves();
    let lost:Vec<_>=folds.iter().flat_map(|key|halves.iter().map(move|half|pages::Key {expression:half.clone(),..key.clone()})).collect();
    pages::evict_composite_pages_where(cache,&cells,|key|lost.contains(key));
    let display=r.scene_tiers.display.clone();
    let pages={let mut store=cache.pages.lock().unwrap();
        store.retained_pages().into_iter().filter(|page|!folds.contains(&page.key)).map(|page|store.allocated(&page.key,crate::residency::Class::Visible).unwrap()).collect::<Vec<_>>()};
    let pivot=doc.scene().order().iter().position(|&handle|handle==top).unwrap();
    let sources={let tiles=r.source_tiles.borrow();(0..doc.scene().order().len()).filter(|index|*index!=pivot).flat_map(|index|{let source=paint_at(&doc,index).base.as_ref().unwrap().image.storage();
        page_coordinates(PixelRect::full(extent)).filter_map(|coordinate|tiles.prepared_view(source,coordinate).and_then(|view|tiles.lease(view))).collect::<Vec<_>>()}).collect::<Vec<_>>()};
    let held=(display.charge(display.headroom(crate::residency::Class::Refinement)),pages,sources);
    assert_eq!(display.headroom(crate::residency::Class::Refinement),0,"only the fold's own closure leaves room for its lost half");
    for step in 0..64 {
        refine_frame(&mut r,frame,false);
        let cache=r.scale_display.as_ref().unwrap();
        let committed=lost.iter().all(|half|cache.pages.lock().unwrap().get(half,half.plan().bounds,pages::Accuracy::Exact,0,crate::residency::Class::Refinement).is_some());
        let flying=cache.streamed_interests(frame.view).any(|interest|folds.contains(&interest.key));
        if committed || !flying {drop(held);return;}
        assert!(step<63,"a fold in flight holds its closure while its lost half is refused for memory");
        crate::test_support::settle_decodes(&mut r);r.wait_idle().unwrap();
    }
}

#[cfg(not(target_arch="wasm32"))]
fn moved_selection_builds_the_new_halves_after_the_previous_release(extent:[u32;2],zoom:f32,level:u32,room:u64) {
    let mut doc=document_at(extent);
    let mut entries=translucent_layers(&mut doc,extent,18);
    let pivot=paint_occurrence(&mut doc,"Pivot",Some(rgba8_source(extent,|x,y|[(y%251) as u8,90,(x%239) as u8,160])));
    entries.insert(entries.len()/2,pivot);
    set_root_entries(&mut doc,entries.clone());
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    fn zoomed(doc:&Document,extent:[u32;2],zoom:f32)->FramePacket<'_> {let mut frame=packet(doc.scene(),extent);frame.view.document_to_surface=[zoom,0.,0.,zoom,0.,0.];frame}
    layer_render::CanvasRenderer::set_working_layer(&mut r,Some(pivot));
    crate::test_support::submit_settled(&mut r,zoomed(&doc,extent,zoom));
    let index=doc.scene().order().iter().position(|&handle|handle==pivot).unwrap();
    paint_mut(&mut doc,index).base=Some(layer_core::authored::PaintBase::new(rgba8_source(extent,|x,y|[(x%233) as u8,(y%229) as u8,40,200]).into()));
    crate::test_support::submit_settled(&mut r,zoomed(&doc,extent,zoom));
    let frame=FramePacket {composite_all:false,..zoomed(&doc,extent,zoom)};
    let previous=r.scale_display.as_ref().unwrap().pivot_halves();
    assert_eq!(r.scale_display.as_ref().unwrap().plan.level,level);
    pages::evict_composite_pages_where(r.scale_display.as_ref().unwrap(),&[PixelRect::full(extent)],|key|key.level>=level || key.coordinate==[0,0]);
    refine_frame(&mut r,frame,true);
    let (in_flight,pinned_below,unpinned)={
        let cache=r.scale_display.as_ref().unwrap();let store=cache.pages.lock().unwrap();
        let halves:Vec<_>=store.retained_pages().into_iter().filter(|page|page.key.source.is_none() && previous.contains(&page.key.expression)).collect();
        (cache.streamed_interests(frame.view).any(|interest|previous.contains(&interest.key.expression)),
            halves.iter().any(|page|page.key.level<level && !page.written.is_empty() && store.pinned(&page.key)),
            halves.iter().filter(|page|!store.pinned(&page.key)).map(|page|page.key.bytes()).sum::<u64>())
    };
    assert!(in_flight,"the spent frame leaves a half of the edited pivot in flight");
    assert!(level==0 || pinned_below,"the cells in flight pin the previous pivot's lower-level halves");
    let display=r.scene_tiers.display.clone();
    let held=display.charge((display.headroom(crate::residency::Class::Refinement)+display.yielding()+unpinned).saturating_sub(room));
    let (send,woken)=std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move||{let _=send.send(());}));
    layer_render::CanvasRenderer::set_working_layer(&mut r,Some(entries[0]));
    crate::test_support::submit_ready(&mut r,frame).unwrap();crate::test_support::complete(&r);
    assert!(r.scale_display.as_ref().unwrap().pivot_refusal.is_some(),"the new pivot's halves are refused while the previous halves are in flight");
    let built=|r:&WgpuRasterizer|{let cache=r.scale_display.as_ref().unwrap();!cache.pivot_halves().is_empty() && !cache.pivot_missing(cache.plan.bounds)};
    for _ in 0..16 {
        if built(&r) {break;}
        if r.next_frame()!=layer_render::FrameDemand::Work {
            assert!(woken.recv_timeout(std::time::Duration::from_secs(1)).is_ok(),"the new pivot's refused halves are never woken: {:?}",r.outstanding_terms());
        }
        crate::test_support::submit_ready(&mut r,frame).unwrap();crate::test_support::complete(&r);
    }
    let cache=r.scale_display.as_ref().unwrap();
    assert!(cache.pivot_halves().iter().all(|half|!previous.contains(half)),"the selection moves the pivot");
    assert!(built(&r),"with {room} more bytes of room the new pivot's halves are built without a pen-down: {:?}",r.outstanding_terms());
    drop(held);
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn a_selection_that_moves_while_the_previous_halves_are_in_flight_builds_the_new_halves_once_they_release() {
    moved_selection_builds_the_new_halves_after_the_previous_release([512,256],1.,0,0);
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn a_half_unit_in_flight_is_driven_to_its_commit_while_a_sibling_refusal_is_parked() {
    let extent=[512,256];let mut doc=document_at(extent);
    let mut entries=translucent_layers(&mut doc,extent,18);
    let pivot=paint_occurrence(&mut doc,"Pivot",Some(rgba8_source(extent,|x,y|[(y%251) as u8,90,(x%239) as u8,160])));
    entries.insert(entries.len()/2,pivot);
    set_root_entries(&mut doc,entries);
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    layer_render::CanvasRenderer::set_working_layer(&mut r,Some(pivot));
    settle_refinement(&mut r,&doc,extent,"open");
    let index=doc.scene().order().iter().position(|&handle|handle==pivot).unwrap();
    paint_mut(&mut doc,index).base=Some(layer_core::authored::PaintBase::new(rgba8_source(extent,|x,y|[(x%233) as u8,(y%229) as u8,40,200]).into()));
    settle_refinement(&mut r,&doc,extent,"edit");
    let frame=FramePacket {composite_all:false,..packet(doc.scene(),extent)};
    let halves=r.scale_display.as_ref().unwrap().pivot_halves();
    pages::evict_composite_pages_where(r.scale_display.as_ref().unwrap(),&[PixelRect::full(extent)],|key|halves.contains(&key.expression));
    refine_frame(&mut r,frame,true);
    let flying=|r:&WgpuRasterizer|r.scale_display.as_ref().unwrap().streamed_interests(frame.view).map(|interest|interest.key).filter(|key|halves.contains(&key.expression)).collect::<Vec<_>>();
    let admitted=flying(&r);
    assert!(!admitted.is_empty(),"the spent frame leaves a half unit in flight");
    let display=r.scene_tiers.display.clone();
    let refusal=display.blocked(crate::residency::Class::Refinement,u64::MAX/2).expect("a sibling's refusal that cannot fit");
    r.scale_display.as_mut().unwrap().pivot_refusal=Some(refusal);
    assert!(r.scale_display.as_ref().unwrap().outstanding(&r).is_some(),"a parked refusal hides the half unit in flight: {:?}",r.outstanding_terms());
    let (send,woken)=std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move||{let _=send.send(());}));
    for _ in 0..16 {
        if flying(&r).is_empty() {break;}
        if r.next_frame()!=layer_render::FrameDemand::Work {
            assert!(woken.recv_timeout(std::time::Duration::from_secs(5)).is_ok(),"the half unit in flight is never driven: {:?}",r.outstanding_terms());
        }
        crate::test_support::submit_ready(&mut r,frame).unwrap();crate::test_support::complete(&r);
    }
    let store=r.scale_display.as_ref().unwrap().pages.lock().unwrap();
    assert!(admitted.iter().all(|key|store.retained_pages().into_iter().any(|page|page.key==*key && !page.written.is_empty())),"the admitted half units commit");
}
