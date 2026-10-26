use super::*;

#[test]
fn display_level_updates_and_dirty_regions_batch_compute_passes() {
    let mut doc = document_at([2048; 2]);
    occurrence_mut(&mut doc, 0).opacity = 0.5;
    add_fill(&mut doc, layer_core::color::RgbColor::WHITE);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.set_complete_display_allowance(1024 << 20);
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    submit_published(&mut r, frame);
    crate::test_support::submit_ready(&mut r, FramePacket { composite_all: false, ..frame }).unwrap();
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
    let expected = display_pixels(&r);
    let mut cache = r.scale_display.take().unwrap();
    let mut scene = r.scene.take().unwrap();
    let frame = FramePacket { composite_all: false, ..frame };
    scene.scale_sources.prepare(&r, frame, &[]);
    assert!(cache.graph.root.as_ref().unwrap().damage(&scene.scale_sources, cache.plan).is_empty());
    assert_eq!(cache.plan.level, 2);
    assert!(cache.valid.remove(&[0, 0]));
    assert!(cache.valid.remove(&[7, 7]));
    cache.reuse_output = false;
    r.metrics.frame_composited_pages.clear();
    let mut commands = Commands::new(&r);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let mut encoding = Encoding { encoder: &mut encoder, commands: &mut commands };
    cache.render_graph(&mut scene, &mut r, frame, PixelRect::EMPTY, &mut encoding, Destination::View, None).unwrap();
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
    engine.set_brush(brush).unwrap();
    let settle = |engine: &mut CanvasEngine<WgpuRasterizer>| {
        let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
        loop {
            engine.render_frame().unwrap();
            engine.backend_mut().wait_idle().unwrap();
            if !engine.has_pending_input() && !engine.backend().has_pending_work() { break; }
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
        let error = quality(&display_pixels(engine.backend()), &expected, engine.backend().scale_display.as_ref().unwrap().plan);
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
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
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
    let extent = doc.composition().size;
    for allowance in [0, 1024 << 20] {
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        r.set_complete_display_allowance(allowance);
        let mut frame = packet(doc.scene(), extent);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        submit_published(&mut r, frame);
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        let mut presenter = ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba32Float,
            SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
        let (_, target) = create_target(&r.device, [frame.view.width_px, frame.view.height_px],
            wgpu::TextureFormat::Rgba32Float, "refinement presentation");
        presenter.set_target_retention(true);
        presenter.present(&r, &target, frame.view, [0.; 4]).unwrap();
        crate::test_support::submit_ready(&mut r, frame).unwrap();
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
        let batch = dab_batch(source_at(&doc, 0), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        crate::test_support::submit_ready(&mut r, FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
        assert!(presenter.needs_present(&r, frame.view, [0.; 4]));
        presenter.present(&r, &target, frame.view, [0.; 4]).unwrap();
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        if presenter.needs_present(&r, frame.view, [0.; 4]) {
            presenter.present(&r, &target, frame.view, [0.; 4]).unwrap();
        }
        for _ in 0..64 {
            if !r.has_pending_work() { break; }
            r.wait_idle().unwrap();
            crate::test_support::submit_ready(&mut r, frame).unwrap();
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
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.set_complete_display_allowance(1024 << 20);
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
    let levels = source.levels.len() as u64;
    for renderer in [&mut r, &mut exact] { crate::test_support::submit_ready(renderer, stroke).unwrap(); }
    assert!(r.scene.as_ref().unwrap().scale_sources.entries[&source_at(&doc, 0)].updates - updates <= 2 * levels,
        "only touched source pages need reduction at each retained level");
    let work = r.metrics.composited_pixels;
    crate::test_support::complete(&r);
    r.metrics.frame_composited_pages.clear();
    let mut scene=r.scene.take().unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    scene.refine_display(&mut r,frame,&mut encoder).unwrap();
    r.uploads.finish(&encoder);encoder.submit(&r.queue);r.scene=Some(scene);
    assert_eq!(r.metrics.frame_composited_pages,vec![(0,[0,0]),(0,[2,1])],"plain painting batches explicit regions across gaps");
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    assert_eq!(r.metrics.composited_pixels - work, 2 * u64::from(PAGE_SIZE).pow(2), "only the two touched pages need exact repair");
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
    scene.refine_display(&mut r,frame,&mut encoder).unwrap();
    r.uploads.finish(&encoder);encoder.submit(&r.queue);
    r.scene=Some(scene);
    assert_eq!(r.metrics.frame_composited_pages,vec![(0,[0,0])]);
    assert_eq!(r.scene.as_ref().unwrap().image_work(),[1,1]);
    let output = page_rect([0,0]).intersect(PixelRect::full(extent));
    let scene = r.scene.as_ref().unwrap();
    assert_eq!(scene.cached_capture_window(frame.scene,output).in_frame(extent).area(),272u64.pow(2),"the document-edge page reads only its declared halo");
    assert_eq!(scene.image_pass_pixels(),output.area(),"the pass writes only the requested output page");
    assert!(r.has_pending_work(),"the distant page waits for another idle batch");
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
fn settled_composition_reuses_every_zoom_and_refines_only_changed_pages() {
    let mut doc = document_at([769, 513]);
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.set_complete_display_allowance(1024 << 20);
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut frame = packet(doc.scene(), extent);
    frame.composite_all = false;
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    submit_published(&mut r, frame); submit_published(&mut exact, frame);
    let drawing_target = r.scale_display.as_ref().unwrap().texture().clone();
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    assert_eq!(r.scale_display.as_ref().unwrap().texture(), &drawing_target, "settling repairs the drawing texture in place");
    assert!(r.scale_display.as_ref().unwrap().hierarchy.as_ref().unwrap().missing().is_none());
    let cache = r.scale_display.as_ref().unwrap();
    let strip = cache.exact_tile.as_ref().expect("resident refinement uses a bounded render attachment");
    assert_eq!([strip.texture.width(), strip.texture.height()], exact_strip(extent));
    assert_ne!(strip.texture, cache.hierarchy.as_ref().unwrap().root().texture);
    let work = r.metrics.composited_pixels;
    for zoom in [0.5, 1., 2., 0.25, 0.125] {
        frame.view.document_to_surface = [zoom, 0., 0., zoom, -40., -20.];
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        assert!(matches!(r.scale_display.as_ref().unwrap().pixels, hierarchy::Pixels::Resident { .. }));
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
    let retained = r.scale_display.as_ref().unwrap().hierarchy.as_ref().unwrap();
    assert_eq!(retained.missing(), Some([0, 0]));
    let work = r.metrics.composited_pixels;
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
    assert_eq!(r.metrics.composited_pixels - work, u64::from(PAGE_SIZE).pow(2));
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
    crate::test_support::submit_ready(&mut r, frame).unwrap(); crate::test_support::submit_ready(&mut exact, frame).unwrap();
    assert!(!r.scale_display.as_ref().unwrap().has_pending_work(&r));
    assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
}
use layer_core::{Affine, EffectInstance, EffectValue, Point};

#[test]
fn global_filters_evict_optional_levels_before_rejecting_the_document() {
    let mut doc = document_at([65, 33]);
    composition_mut(&mut doc).size=[2561;2];
    paint_mut(&mut doc,0).domain = [2561;2];
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.set_complete_display_allowance(1024 << 20);
    r.native_edit.as_mut().unwrap().composition_bytes = 0;
    let mut frame = packet(doc.scene(), extent);
    frame.composite_all = false;
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    submit_published(&mut r, frame);
    for _ in 0..=page_coordinates(PixelRect::full(extent)).count() {
        if !r.has_pending_work() { break; }
        r.wait_idle().unwrap();
        crate::test_support::submit_ready(&mut r, frame).unwrap();
    }
    assert!(!r.has_pending_work());
    let resident = r.scale_display.as_ref().unwrap().resident_bytes();
    assert!(resident > 0);
    let mut program = (*crate::tests::fixture("gaussian_blur").program()).clone();
    program.resolution = layer_core::EffectResolution::Native;
    program.passes = vec![layer_core::EffectPass { entry: program.entry.clone(), sampling: layer_core::EffectSampling::Document }].into();
    let effect=effect_occurrence(&mut doc,EffectInstance::new(Arc::new(program)),"effect");
    insert_occurrence(&mut doc,effect,0);
    let budget = r.native_edit.as_ref().unwrap();
    assert!(windows::Plan::new(doc.scene(), extent, budget.image_pixel_budget(resident), r.device.limits().max_texture_dimension_2d).is_err());
    assert!(windows::Plan::new(doc.scene(), extent, budget.image_pixel_budget(0), r.device.limits().max_texture_dimension_2d).is_ok());
    frame = packet(doc.scene(), extent);
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    crate::test_support::submit_ready(&mut r, frame).unwrap();
    assert_eq!(r.scale_display.as_ref().unwrap().resident_bytes(), 0);
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
        let extent = doc.composition().size;
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        exact.test.reference = true;
        let mut frame = packet(doc.scene(), extent);
        frame.blend_space = space;
        frame.composite_all = false;
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        for renderer in [&mut r, &mut exact] { crate::test_support::submit_ready(renderer, frame).unwrap(); }
        for (step, kind) in [DabBatchKind::Persistent, DabBatchKind::Preview, DabBatchKind::Persistent].into_iter().enumerate() {
            let mut dab = crate::tests::test_dab([252. + step as f32 * 20., 128.], [0.2, 0.3, 0.8, 0.8], 0.8);
            dab.radii = [90.; 2];
            dab.material = [0.5, 0.8, 1., 0.8];
            let mut batch = dab_batch(source_at(&doc, 0), crate::layer_tests::preset_style(DefaultBrushPreset::WatercolorWash), dab.bounds());
            batch.kind = kind;
            batch.stroke_start = step == 0;
            batch.stroke_end = step == 2;
            let stroke = FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame };
            for renderer in [&mut r, &mut exact] { crate::test_support::submit_ready(renderer, stroke).unwrap(); }
            assert!(r.paint_layers[0].watercolor.is_some());
            assert!(r.scale_display.is_some(), "watercolor stays in the common display cache");
            let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), r.scale_display.as_ref().unwrap().plan);
            assert!(error[0] < 0.002 && error[1] < 0.015, "watercolor {space:?} {kind:?}: {error:?}");
            if kind == DabBatchKind::Preview {
                for renderer in [&mut r, &mut exact] { crate::test_support::submit_ready(renderer, frame).unwrap(); }
            }
            assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        }
        for zoom in [1., 0.5, 0.125] {
            frame.view.document_to_surface = [zoom, 0., 0., zoom, 0., 0.];
            for renderer in [&mut r, &mut exact] { crate::test_support::submit_ready(renderer, frame).unwrap(); }
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
    assert!(!r.has_pending_work());
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
    crate::test_support::submit_ready(&mut r, frame).unwrap(); crate::test_support::submit_ready(&mut exact, frame).unwrap();
    let animated = display_pixels(&r);
    assert!(original != animated, "animation changes retained display output");
    let error = quality(&animated, &pixels(&exact, crate::test_support::document_texture(&exact)), r.scale_display.as_ref().unwrap().plan);
    assert!(error[2] < 0.002, "animated display {error:?}");
    let dab = crate::tests::test_dab([30., 30.], [0.9, 0.1, 0.3, 1.], 0.8);
    let batch = dab_batch(source_at(&doc, 1), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
    frame.time_seconds = 3.;
    let painting = FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame };
    crate::test_support::submit_ready(&mut r, painting).unwrap(); crate::test_support::submit_ready(&mut exact, painting).unwrap();
    let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), r.scale_display.as_ref().unwrap().plan);
    assert!(error[2] < 0.002, "animation advances outside the simultaneous paint contact {error:?}");
}

#[test]
fn idle_display_refines_paused_transform_windows_and_reuses_exact_overlap() {
    for space in layer_core::BlendSpace::ALL {
        let doc = document_at([1541, 771]);
        let extent = doc.composition().size;
        let preview = layer_render::TransformPreview { transaction: 1, target: source_at(&doc, 0), moving: false, selection: None,
            transform: layer_core::ImageTransform::affine(Affine::around(Point { x: 770., y: 385. }, [0.9, 0.8], 0.17, Point::default())) };
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        r.set_complete_display_allowance(0);
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
            submit_published(&mut r, frame); crate::test_support::submit_settled(&mut exact, FramePacket { composite_all: true, ..frame });
            assert!(r.scale_display.as_ref().unwrap().overview.is_some());
            assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        }
        frame.view.width_px = extent[0];
        frame.view.height_px = extent[1];
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        assert!(r.scale_display.as_ref().unwrap().placed.is_some());
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
        crate::test_support::submit_ready(&mut r, FramePacket { composite_all: false, ..frame }).unwrap();
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
        crate::test_support::submit_ready(&mut r, stroke).unwrap(); crate::test_support::submit_ready(&mut exact, stroke).unwrap();
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
            if !r.has_pending_work() { break; }
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
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.set_complete_display_allowance(1024 << 20);
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
