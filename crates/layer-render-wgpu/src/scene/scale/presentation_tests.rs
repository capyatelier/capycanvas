use super::*;
use layer_core::color::{ColorProfile, DocumentColor, SampleDepth, RgbSpace};
use layer_render::{ViewState, ColorSampleArea, ColorSampleRequest, ColorSampleSource};

fn bounded_renderer(color: DocumentColor) -> Result<WgpuRasterizer, GpuRasterError> {
    let mut r = WgpuRasterizer::new_native_headless(color)?;
    r.set_complete_display_allowance(0);
    Ok(r)
}

fn document(extent: [u32; 2]) -> layer_core::Document {
    let mut doc = layer_core::Document::new("bounded live display", extent[0], extent[1]);
    doc.color = DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: SampleDepth::U16,
    };
    let mut builder = SourceBuilder::new(
        extent,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U16,
            profile: ColorProfile::Builtin(RgbSpace::DisplayP3),
            profile_assumed: false,
        },
        64 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..extent[1] {
        let mut row = Vec::with_capacity(extent[0] as usize * 8);
        for x in 0..extent[0] {
            for v in [
                ((x * 53 + y * 17) % 60000) as u16,
                ((x * 7 + y * 31) % 50000) as u16,
                (5000 + (x * 3 + y * 19) % 55000) as u16,
                40000,
            ] {
                row.extend_from_slice(&v.to_le_bytes());
            }
        }
        builder.push_row(&row).unwrap();
    }
    doc.layers[0].source = Some(Arc::new(builder.finish().unwrap()));
    doc
}

fn view(matrix: [f32; 6]) -> ViewState {
    ViewState {
        width_px: 320,
        height_px: 240,
        document_to_surface: matrix,
        background_rgba_linear: [1.; 4],
    }
}

fn submit(r: &mut WgpuRasterizer, doc: &layer_core::Document, view: ViewState, all: bool) {
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    r.submit(FramePacket { view, composite_all: all, ..packet(&doc.layers, [doc.width, doc.height]) })
    .unwrap();
    while r.has_pending_work() {
        assert!(std::time::Instant::now() < deadline, "display did not settle");
        r.submit(FramePacket { view, composite_all: false, ..packet(&doc.layers, [doc.width, doc.height]) }).unwrap();
    }
}

fn pixels(r: &WgpuRasterizer, texture: &wgpu::Texture) -> Vec<[f32; 4]> {
    crate::layer_tests::page_bytes(r, texture)
        .chunks_exact(16)
        .map(|p| {
            std::array::from_fn(|i| f32::from_le_bytes(p[i * 4..i * 4 + 4].try_into().unwrap()))
        })
        .collect()
}

fn present(
    r: &WgpuRasterizer,
    presenter: &mut ViewportPresenter,
    view: ViewState,
) -> Vec<[f32; 4]> {
    let (texture, target) = create_target(
        &r.device,
        [view.width_px, view.height_px],
        wgpu::TextureFormat::Rgba32Float,
        "bounded display oracle target",
    );
    presenter.present(r, &target, view, [0.; 4]).unwrap();
    pixels(r, &texture)
}

#[track_caller]
fn close(a: &[[f32; 4]], b: &[[f32; 4]]) {
    assert_eq!(a.len(), b.len());
    for (pixel, (a, b)) in a.iter().zip(b).enumerate() {
        for i in 0..4 {
            assert!(
                (a[i] - b[i]).abs() < 5e-6,
                "pixel {pixel}, channel {i}: {} != {}",
                a[i],
                b[i]
            );
        }
    }
}

#[test]
fn rejected_views_and_abandoned_composition_preserve_artwork() {
    let doc = document([517, 259]);
    let mut r = bounded_renderer(doc.color).unwrap();
    let v = view([0.125, 0., 0., 0.125, 0., 0.]);
    submit(&mut r, &doc, v, true);
    let mut presenter = ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    let before = present(&r, &mut presenter, v);
    let texture = r.scale_display.as_ref().unwrap().texture().clone();
    let metrics = r.metrics();
    for rejected in [
        ViewState { document_to_surface: [f32::NAN; 6], ..v },
        ViewState { document_to_surface: [0.; 6], ..v },
        ViewState { width_px: 8192, height_px: 8192,
            document_to_surface: [1., 0., 0., 1., 0., 0.], ..v },
    ] {
        assert!(r.submit(FramePacket { view: rejected, reset_layers: true,
            ..packet(&doc.layers, [8192; 2]) }).is_err());
        assert_eq!(r.document_extent, [doc.width, doc.height]);
        assert_eq!(r.scale_display.as_ref().unwrap().texture(), &texture);
        assert_eq!(r.metrics(), metrics);
        close(&before, &present(&r, &mut presenter, v));
    }
    let mut abandoned = doc.clone();
    abandoned.layers[0].source = Some(layer_core::color::source::rgba8_source([doc.width, doc.height], |x, y|
        [if (x / 3 + y / 2) % 2 == 0 { 40 } else { 220 }, 128, 70, 255]));
    let mut overlay = abandoned.layers[0].clone();
    overlay.id = abandoned.allocate_layer_id();
    overlay.opacity = 0.7;
    overlay.properties.blend = layer_core::LayerBlend::Multiply;
    abandoned.layers.insert(0, overlay);
    let frame = FramePacket { view: v, ..packet(&abandoned.layers, [doc.width, doc.height]) };
    r.submit(frame).unwrap();
    assert!(r.has_pending_work());
    let mut scene = r.scene.take().unwrap();
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    scene.refine_display(&mut r, frame, &mut encoder).unwrap();
    drop(encoder);
    r.scene = Some(scene);
    submit(&mut r, &abandoned, v, false);
    let mut reference = bounded_renderer(doc.color).unwrap();
    reference.test.exact_display = true;
    submit(&mut reference, &abandoned, v, true);
    let mut oracle = ViewportPresenter::for_surface(&reference, wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    close(&present(&r, &mut presenter, v), &present(&reference, &mut oracle, v));
    assert_eq!(r.readback_srgb_rgba8().unwrap(), reference.readback_srgb_rgba8().unwrap());
}

#[test]
fn filter_images_share_the_display_composition_budget() {
    let mut r = bounded_renderer(DocumentColor::default()).unwrap();
    let extent = [5184, 3456];
    let layers = [crate::tests::image_windows::effect(1, false, false)];
    let image_bytes = scene::Scene::capture_image_bound(&layers, PixelRect::full(extent));
    for allowance in [0, 512 << 20, 1536 << 20] {
        r.set_complete_display_allowance(allowance);
        let native = r.native_edit.as_ref().unwrap();
        let display = CACHE_BYTES;
        let images = native.image_pixel_budget(0);
        let floor = CACHE_BYTES + scene::windows::DEFAULT_IMAGE_PIXEL_BYTES;
        assert_eq!(display + images, native.composition_bytes.max(floor));
        let plan = scene::windows::Plan::new(&layers, extent, images).unwrap();
        assert_eq!(plan.is_none(), allowance == 1536 << 20);
        if plan.is_none() { assert!(image_bytes + display <= native.composition_bytes); }
    }
}

#[test]
fn visible_detail_matches_dense_composition_through_pan_wrap_rotation_and_resize() {
    let doc = document([1537, 769]);
    let mut dense = bounded_renderer(doc.color).unwrap();
    dense.test.exact_display = true;
    let mut cached = bounded_renderer(doc.color).unwrap();

    let mut a = ViewportPresenter::for_surface(
        &dense,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    let mut b = ViewportPresenter::for_surface(
        &cached,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    for (index, matrix) in [
        [1., 0., 0., 1., 0., 0.],
        [1., 0., 0., 1., -100., -100.],
        [1., 0., 0., 1., -700., -450.],
        [1., 0., 0., 1., -1200., -529.],
        [1., 0., 0., 1., -256., -256.],
        [0., 1., -1., 0., 600., -700.],
        [-1., 0., 0., 1., 1200., -300.],
        [2., 0., 0., 2., -1111., -555.],
        [0.5, 0., 0., 0.5, -233., -100.],
        [1., 0., 0., 1., 0., 0.],
    ]
    .into_iter()
    .enumerate()
    {
        let view = view(matrix);
        submit(&mut dense, &doc, view, index == 0);
        submit(&mut cached, &doc, view, index == 0);

        assert!(cached.scale_display.as_ref().unwrap().storage_bytes() <= CACHE_BYTES);
        close(
            &present(&dense, &mut a, view),
            &present(&cached, &mut b, view),
        );
        if index > 0 {
            assert_eq!(
                dense.artwork_revision, cached.artwork_revision,
                "camera motion is not an artwork change"
            );
        }
    }
    let revision = cached.composite_revision;
    let work = cached.metrics.composited_pixels;
    submit(&mut cached, &doc, view([1., 0., 0., 1., -1., 0.]), false);
    assert_eq!(
        cached.metrics.composited_pixels, work,
        "resident pan must reuse detail"
    );
    assert_eq!(cached.composite_revision, revision);
    assert!(
        cached
            .request_color_sample(ColorSampleRequest {
                request_id: 11,
                source: ColorSampleSource::Composite,
                position: [255, 256],
                area: ColorSampleArea::Average5,
            })
            .unwrap()
    );
    cached
        .device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
    let sample = cached.take_color_sample().unwrap().unwrap();
    assert!(sample.rgba.iter().all(|v| v.is_finite()));
}

#[test]
fn filtered_masked_source_edits_and_restoration_refresh_detail_and_coarse_display() {
    use layer_core::{LayerMask, Point, Selection};
    let mut doc = document([777, 533]);
    let source = doc.layers[0].source.clone().unwrap();
    let mut mask = LayerMask::reveal_all(LayerId(99), Point { x: 7., y: -9. });
    mask.default_coverage = 0.;
    mask.initial = Some(
        Selection::polygon(vec![
            Point { x: 0., y: 0. },
            Point { x: 760., y: 99. },
            Point { x: 440., y: 533. },
        ])
        .unwrap(),
    );
    doc.layers[0].mask = Some(mask);
    doc.layers
        .insert(0, crate::tests::image_windows::effect(20, false, false));
    doc.layers
        .insert(0, crate::tests::image_windows::effect(21, false, false));
    let original = doc.clone();
    let mut dense = bounded_renderer(doc.color).unwrap();
    dense.test.exact_display = true;
    let mut r = bounded_renderer(doc.color).unwrap();

    let mut a = ViewportPresenter::for_surface(
        &dense,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    let mut b = ViewportPresenter::for_surface(
        &r,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    let v = view([1., 0., 0., 1., -233., -111.]);
    let mut first = Vec::new();
    for step in 0..5 {
        match step {
            1 => doc.layers[2].properties.offset = Point { x: 17., y: -9. },
            2 => {
                doc.layers[2].mask.as_mut().unwrap().inverted = true;
                doc.layers[0].opacity = 0.6;
            }
            3 => doc.layers[2].mask.as_mut().unwrap().show_area = true,
            4 => doc = original.clone(),
            _ => {}
        }
        // Layer property actions request recomposition through FramePacket;
        // only camera motion uses an otherwise clean packet.
        submit(&mut dense, &doc, v, true);
        submit(&mut r, &doc, v, true);
        let displayed = present(&r, &mut b, v);
        close(&present(&dense, &mut a, v), &displayed);
        if step == 0 {
            first = displayed;
        } else if step == 4 {
            close(&first, &displayed);
        } else {
            assert!(
                first != displayed,
                "step {step} must change visible artwork"
            );
        }
        assert!(Arc::ptr_eq(doc.layers[2].source.as_ref().unwrap(), &source));
    }
    // Recreating the GPU cache reconstructs the same pixels from retained source.
    let mut recovered = bounded_renderer(doc.color).unwrap();

    submit(&mut recovered, &doc, v, true);
    let mut p = ViewportPresenter::for_surface(
        &recovered,
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    close(&first, &present(&recovered, &mut p, v));
}

#[test]
fn in_surface_navigator_keeps_clipped_geometry() {
    let doc = document([1537, 769]);
    let mut r = bounded_renderer(doc.color).unwrap();

    submit(&mut r, &doc, view([1., 0., 0., 1., -900., -400.]), true);
    let size = [256, (256. * 769. / 1537f32).ceil() as u32];
    let mut oracle = Vec::new();
    let (texture, target) = create_target(
        &r.device,
        size,
        wgpu::TextureFormat::Rgba8Unorm,
        "coarse native Navigator",
    );
    let mut presenter = ViewportPresenter::for_overview_surface(&r, wgpu::TextureFormat::Rgba8Unorm, crate::SdrSurfaceColor::Srgb).unwrap();
    for clipped in [false, true] {
        presenter.set_overviews(
            &r,
            &[OverviewPlacement {
                bounds: [0., 0., size[0] as f32, size[1] as f32],
                clip: clipped.then_some([13., 17., (size[0] - 31) as f32, (size[1] - 37) as f32]),
                work_area: [[-1000.; 2]; 4],
                outline_linear: [0.; 3],
                background_linear: [1.; 3],
                scale: 1.,
                opacity: 1.,
            }],
        );
        presenter.present_overviews(&r, &target, size).unwrap();
        let actual = crate::layer_tests::page_bytes(&r, &texture);
        if !clipped {
            assert_ne!(&actual[..4], [0; 4]);
            oracle = actual;
            continue;
        }
        for y in 0..size[1] {
            for x in 0..size[0] {
                let i = ((y * size[0] + x) * 4) as usize;
                if !(13..size[0] - 18).contains(&x) || !(17..size[1] - 20).contains(&y) {
                    assert_eq!(&actual[i..i + 4], [0; 4]);
                } else {
                    for c in 0..4 {
                        assert!(
                            actual[i + c].abs_diff(oracle[i + c]) <= 1,
                            "Navigator {x},{y} channel {c}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn large_document_waits_for_mip_compilation_before_reporting_canvas_ready() {
    use std::time::{Duration, Instant};
    let color = DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: SampleDepth::U16,
    };
    let mut doc = layer_core::Document::new("large staged document", 4097, 1025);
    doc.color = color;
    let mut r = bounded_renderer(color).unwrap();
    r.startup = Some(startup::Startup::new(&r.device).unwrap());
    let (release, wait) = mpsc::channel();
    let (entered, blocked) = mpsc::channel();
    let compiler = &r.startup.as_ref().unwrap().compiler;
    compiler.enqueue(0, move || {
        entered.send(()).map_err(|e| e.to_string())?;
        wait.recv_timeout(Duration::from_secs(20))
            .map_err(|e| e.to_string())
    });
    compiler.start();
    blocked.recv_timeout(Duration::from_secs(20)).unwrap();
    let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
    r.prepare_startup(&doc, &brush, false).unwrap();
    assert!(!r.poll_startup().unwrap().canvas_ready);
    assert!(!r.scene_pipelines.scale.reduce.ready());
    assert!(!r.scene_pipelines.scale.reduce_pair.ready());

    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !r.poll_startup().unwrap().canvas_ready {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(r.scene_pipelines.scale.reduce.ready());
    assert!(r.scene_pipelines.scale.reduce_pair.ready());
    submit(&mut r, &doc, view([1., 0., 0., 1., 0., 0.]), true);
    assert!(r.scale_display.is_some());
    drop(r);
    startup::finish_shader_compiler_shutdown();
}

#[test]
fn committed_contact_strokes_present_their_canonical_native_pixels() {
    use layer_engine::{CanvasEngine, InstantFeedbackConfig, PenEvent, PenPhase,
        SampleFlags, ToolKind, ViewTransform, input_queue};
    for preset in [layer_core::DefaultBrushPreset::AntiquePen, layer_core::DefaultBrushPreset::BrushedInk] {
        let doc = layer_core::Document::new("canonical contact", 1024, 512);
        let v = view([0.5, 0., 0., 0.5, 0., 0.]);
        let (mut input, consumer) = input_queue(64);
        let mut engine = CanvasEngine::new(bounded_renderer(doc.color).unwrap(), doc, consumer, v,
            ViewTransform { revision: 0, surface_to_document: [2., 0., 0., 2., 0., 0.] }).unwrap();
        engine.set_instant_feedback(InstantFeedbackConfig { enabled: false, ..Default::default() }).unwrap();
        let mut brush = layer_core::default_brush(preset);
        brush.diameter = 70.;
        brush.color_rgba_linear = [0.08, 0.015, 0.25, 1.];
        engine.set_brush(brush).unwrap();
        for i in 0..=32 {
            let t = i as f32 / 32.;
            input.push(PenEvent {
                device_id: 1, sequence: i + 1, timestamp_ns: (i + 1) * 8_333_333,
                view_revision: 0,
                surface_position: layer_core::Point { x: 40. + 240. * t, y: 120. + 50. * (t * std::f32::consts::TAU).sin() },
                pressure: if i < 32 { 0.2 + 0.7 * (t * std::f32::consts::PI).sin() } else { 0. },
                tilt_radians: [0.; 2], twist_radians: 0., distance: 0.,
                phase: if i == 0 { PenPhase::Down } else if i == 32 { PenPhase::Up } else { PenPhase::Move },
                tool: ToolKind::Pen, flags: SampleFlags::PRIMARY,
            }).unwrap();
            let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
            loop {
                engine.render_frame().unwrap();
                engine.backend_mut().wait_idle().unwrap();
                if !engine.has_pending_input() { break; }
                assert!(std::time::Instant::now() < deadline, "input did not drain at sample {i}");
            }
        }
        let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
        while engine.backend().has_pending_work() {
            engine.render_frame().unwrap();
            engine.backend_mut().wait_idle().unwrap();
            assert!(std::time::Instant::now() < deadline, "committed stroke did not settle");
        }
        assert_eq!(engine.metrics().committed_strokes, 1);
        let mut presenter = ViewportPresenter::for_surface(engine.backend(),
            wgpu::TextureFormat::Rgba32Float, SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
        let live = present(engine.backend(), &mut presenter, v);
        let doc = engine.document().clone();
        submit(engine.backend_mut(), &doc, v, true);
        assert!(live == present(engine.backend(), &mut presenter, v),
            "{preset:?}: the committed stroke must present the pixels Undo/Redo and reopening restore");
    }
}

#[test]
fn native_stroke_undo_redo_and_replaced_device_rebuild_visible_tiles_from_exact_backing() {
    use layer_engine::{
        CanvasEngine, PenEvent, PenPhase, SampleFlags, ToolKind, ViewTransform, input_queue,
    };
    let color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    };
    let mut doc = layer_core::Document::new("bounded native drawing", 1025, 513);
    doc.color = color;
    let v = view([1., 0., 0., 1., 0., 0.]);
    let renderer = || bounded_renderer(color).unwrap();
    let (mut input, consumer) = input_queue(64);
    let mut engine =
        CanvasEngine::new(renderer(), doc, consumer, v, ViewTransform::IDENTITY).unwrap();
    let flush = |engine: &mut CanvasEngine<WgpuRasterizer>| {
        let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
        loop {
            engine.render_frame().unwrap();
            if !engine.has_pending_input() && !engine.has_pending_document_edits()
                && (engine.has_active_stroke() || !engine.backend().has_pending_work()) {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    };
    flush(&mut engine);
    let mut p = ViewportPresenter::for_surface(
        engine.backend(),
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    let blank = present(engine.backend(), &mut p, v);
    for (i, phase) in [PenPhase::Down, PenPhase::Move, PenPhase::Up]
        .into_iter()
        .enumerate()
    {
        input
            .push(PenEvent {
                device_id: 1,
                sequence: i as u64 + 1,
                timestamp_ns: (i as u64 + 1) * 10_000_000,
                view_revision: 0,
                surface_position: layer_core::Point {
                    x: 245. + i as f32 * 18.,
                    y: 80.,
                },
                pressure: 0.37,
                tilt_radians: [0.; 2],
                twist_radians: 0.,
                distance: 0.,
                phase,
                tool: ToolKind::Pen,
                flags: SampleFlags::PRIMARY,
            })
            .unwrap();
        flush(&mut engine);
    }
    let painted = present(engine.backend(), &mut p, v);
    assert!(painted != blank);
    let root = engine.document().layers[0].raster.clone();
    let backed = root.wait_data().unwrap();
    assert!(
        backed.tiles.len() >= 2,
        "stroke crosses native tile boundaries"
    );
    let exact: Vec<_> = backed
        .tiles
        .iter()
        .map(|(key, tile)| (*key, tile.wait_backing().unwrap().decode().unwrap()))
        .collect();
    assert!(engine.undo().unwrap());
    flush(&mut engine);
    close(&blank, &present(engine.backend(), &mut p, v));
    assert!(engine.redo().unwrap());
    flush(&mut engine);
    close(&painted, &present(engine.backend(), &mut p, v));
    let retired = engine.replace_backend(renderer()).unwrap();
    retired.device.destroy();
    drop(retired);
    drop(p);
    flush(&mut engine);

    let mut p = ViewportPresenter::for_surface(
        engine.backend(),
        wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb,
    )
    .unwrap();
    close(&painted, &present(engine.backend(), &mut p, v));
    let restored = engine.document().layers[0].raster.wait_data().unwrap();
    for (key, bytes) in exact {
        assert_eq!(
            bytes,
            restored.tiles[&key]
                .wait_backing()
                .unwrap()
                .decode()
                .unwrap()
        );
    }
}

#[test]
fn pass_through_edits_and_mode_changes_match_full_recomposition() {
    let mut doc = document([777, 533]);
    let photo = doc.layers[0].id;
    let layer = |doc: &mut layer_core::Document, kind, filter: Option<&str>| {
        let mut layer = Layer::paint(doc.allocate_layer_id(), "grouped");
        layer.kind = kind;
        layer.effect = filter.map(|id| {
            Arc::new(layer_core::EffectInstance::new(layer_core::bundled_effect_catalog().get(id).unwrap().program()))
        });
        layer
    };
    let mut group = layer(&mut doc, LayerKind::Group, None);
    group.properties.blend = layer_core::LayerBlend::PassThrough;
    let mut upper = layer(&mut doc, LayerKind::Paint, None);
    upper.properties.blend = layer_core::LayerBlend::Multiply;
    let desaturate = layer(&mut doc, LayerKind::Effect, Some("black_white"));
    let blur = layer(&mut doc, LayerKind::Effect, Some("gaussian_blur"));
    let lower = layer(&mut doc, LayerKind::Paint, None);
    let (id, upper_id, lower_id) = (group.id, upper.id, lower.id);
    for (i, mut child) in [upper, desaturate, blur, lower].into_iter().enumerate() {
        child.properties.parent = Some(id);
        doc.layers.insert(i, child);
    }
    doc.layers.insert(0, group);
    let mut incremental = bounded_renderer(doc.color).unwrap();
    let mut reference = bounded_renderer(doc.color).unwrap();
    reference.test.exact_display = true;
    for r in [&mut incremental, &mut reference] {

        r.set_complete_display_allowance(64 * 1024 * 1024);
    }
    let mut a = ViewportPresenter::for_surface(&incremental, wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    let mut b = ViewportPresenter::for_surface(&reference, wgpu::TextureFormat::Rgba32Float,
        SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    let v = centered_view([doc.width, doc.height], [320, 240], 0.4, 0.);
    fn find(doc: &mut layer_core::Document, id: LayerId) -> &mut Layer {
        doc.layers.iter_mut().find(|l| l.id == id).unwrap()
    }
    for step in 0..8 {
        let target = match step {
            1 | 7 => Some((photo, [300., 300.])),
            2 => Some((lower_id, [500., 200.])),
            3 => Some((upper_id, [650., 400.])),
            _ => None,
        };
        let edited = matches!(step, 4..=6);
        match step {
            4 => find(&mut doc, id).properties.blend = layer_core::LayerBlend::Normal,
            5 => find(&mut doc, id).properties.blend = layer_core::LayerBlend::PassThrough,
            6 => {
                let mut mask = layer_core::LayerMask::reveal_all(LayerId(999), Default::default());
                mask.default_coverage = 0.6;
                find(&mut doc, id).mask = Some(mask);
                find(&mut doc, id).opacity = 0.5;
            }
            _ => {}
        }
        let dabs: Vec<_> = target.iter().map(|(_, center)| {
            let mut dab = crate::tests::test_dab(*center, [0.9, 0.3, 0.1, 0.8], 1.);
            dab.radii = [60.; 2];
            dab
        }).collect();
        let batches: Vec<_> = target.iter().zip(&dabs).map(|((layer, _), dab)| {
            crate::test_support::dab_batch(*layer, crate::tests::test_style(BrushExecution::Dry), dab.bounds())
        }).collect();
        reference.scene = None;
        for (r, all) in [(&mut incremental, step == 0 || edited), (&mut reference, true)] {
            r.submit(FramePacket {
                view: v,
                dabs: &dabs,
                dab_batches: &batches,
                composite_all: all,
                ..packet(&doc.layers, [doc.width, doc.height])
            }).unwrap();
            submit(r, &doc, v, false);
        }
        close(&present(&incremental, &mut a, v), &present(&reference, &mut b, v));
    }
    for layer in [lower_id, photo] {
        for moving in [true, false] {
            let transform = layer_render::TransformPreview {
                transaction: 1, layer, moving, selection: None,
                transform: layer_core::ImageTransform::affine(layer_core::Affine::translation(layer_core::Point { x: 23.5, y: -11.25 })),
            };
            reference.scene = None;
            for (r, all) in [(&mut incremental, false), (&mut reference, true)] {
                r.set_transform_preview(Some(&transform)).unwrap();
                submit(r, &doc, v, all);
            }
            assert!(!incremental.has_pending_work());
            assert!(!reference.has_pending_work());
            let actual = present(&incremental, &mut a, v);
            let exact = present(&reference, &mut b, v);
            if moving {
                let error = quality(&actual, &exact, display_mips::Plan::at([v.width_px, v.height_px], 0));
                assert!(error[0] < 0.004 && error[1] < 0.06, "moving filtered group: {error:?}");
            } else { close(&actual, &exact); }
        }
        for r in [&mut incremental, &mut reference] {
            r.set_transform_preview(None).unwrap();
            submit(r, &doc, v, false);
        }
        assert!(!incremental.has_pending_work());
        close(&present(&incremental, &mut a, v), &present(&reference, &mut b, v));
    }
}

fn centered_view(extent: [u32; 2], viewport: [u32; 2], scale: f32, angle: f32) -> ViewState {
    let (sin, cos) = angle.sin_cos();
    let a = scale * cos;
    let b = scale * sin;
    let [x, y] = extent.map(|v| v as f32 * 0.5);
    ViewState {
        width_px: viewport[0], height_px: viewport[1],
        ..view([a, b, -b, a, viewport[0] as f32 * 0.5 - a*x + b*y,
            viewport[1] as f32 * 0.5 - b*x - a*y])
    }
}
