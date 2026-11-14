use crate::test_support::float_pixels as pixels;
use super::*;
use layer_core::color::{ColorProfile, DocumentColor, SampleDepth, RgbSpace};
use layer_render::{ViewState, ColorSampleArea, ColorSampleRequest, ColorSampleSource};

fn bounded_renderer(color: DocumentColor) -> Result<WgpuRasterizer, GpuRasterError> {
    Ok(crate::memory::memory_tests::legacy_scene_renderer(color))
}

fn document(extent: [u32; 2]) -> layer_core::Document {
    let mut doc = layer_core::Document::new(PortableId::random(), extent[0], extent[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    composition_mut(&mut doc).color = DocumentColor {
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
    paint_mut(&mut doc,0).base = Some(layer_core::authored::PaintBase::new((Arc::new(builder.finish().unwrap())).into()));
    doc
}

fn view(matrix: [f32; 6]) -> ViewState {
    ViewState {
        width_px: 320,
        height_px: 240,
        document_to_surface: matrix,
    }
}

fn submit(r: &mut WgpuRasterizer, doc: &layer_core::Document, view: ViewState, all: bool, bounded: bool) {
    let mut frame=FramePacket {view,inspect_mask:doc.working.inspect_mask,composite_all:all,blend_space:doc.composition().blend,..packet(doc.scene(),doc.composition().size)};
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    loop {
        if bounded {working_scene_frame(r,frame);}
        crate::test_support::submit_ready(r,frame).unwrap();
        if r.next_frame()!=layer_render::FrameDemand::Work {break;}
        assert!(std::time::Instant::now()<deadline,"display did not settle");
        r.wait_idle().unwrap();
        frame.composite_all=false;
    }
}

fn prepare_engine_frame(engine: &mut layer_engine::CanvasEngine<WgpuRasterizer>) {
    let document=engine.document().clone();
    let frame=FramePacket {view:engine.view(),inspect_mask:document.working.inspect_mask,..packet(document.scene(),document.composition().size)};
    working_scene_frame(engine.backend_mut(),frame);
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
    let mut r = bounded_renderer(doc.composition().color).unwrap();
    let v = view([0.125, 0., 0., 0.125, 0., 0.]);
    submit(&mut r, &doc, v, true, true);
    let mut presenter = crate::test_support::float_presenter(&r);
    let before = present(&r, &mut presenter, v);
    let texture = r.scale_display.as_ref().unwrap().texture().clone();
    let metrics = r.metrics();
    let oversized = [r.max_document_dimension() + 1, 8192];
    for (rejected, extent) in [
        (ViewState { document_to_surface: [f32::NAN; 6], ..v }, [8192; 2]),
        (ViewState { document_to_surface: [0.; 6], ..v }, [8192; 2]),
        (ViewState { width_px: 8192, height_px: 8192,
            document_to_surface: [1., 0., 0., 1., 0., 0.] }, oversized),
    ] {
        assert!(r.submit(FramePacket { view: rejected, reset_layers: true,
            ..packet(doc.scene(), extent) }).is_err());
        assert_eq!(r.document_extent, doc.composition().size);
        assert_eq!(r.scale_display.as_ref().unwrap().texture(), &texture);
        assert_eq!(r.metrics(), metrics);
        close(&before, &present(&r, &mut presenter, v));
    }
    let mut abandoned = doc.clone();
    paint_mut(&mut abandoned,0).base = Some(layer_core::authored::PaintBase::new((layer_core::color::source::rgba8_source(doc.composition().size, |x, y|
        [if (x / 3 + y / 2) % 2 == 0 { 40 } else { 220 }, 128, 70, 255])).into()));
    let overlay=copy_paint(&mut abandoned,0);
    let occurrence=abandoned.artwork.occurrences.get_mut(overlay).unwrap();occurrence.opacity=0.7;occurrence.blend=layer_core::LayerBlend::Multiply;
    insert_occurrence(&mut abandoned,overlay,0);
    let frame = FramePacket { view: v, ..packet(abandoned.scene(), doc.composition().size) };
    working_scene_frame(&mut r,frame);
    super::submit_published(&mut r, frame);
    assert!(r.next_frame()==layer_render::FrameDemand::Work);
    let mut scene = r.scene.take().unwrap();
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    scene.refine_display(&mut r, frame, &mut encoder,&mut Scene::begin_work(false)).unwrap();
    drop(encoder);
    r.scene = Some(scene);
    submit(&mut r, &abandoned, v, false, true);
    let mut reference = bounded_renderer(doc.composition().color).unwrap();
    reference.test.exact_display = true;
    submit(&mut reference, &abandoned, v, true, true);
    let mut oracle = crate::test_support::float_presenter(&reference);
    close(&present(&r, &mut presenter, v), &present(&reference, &mut oracle, v));
    crate::test_support::assert_same_canonical(&mut r, &mut reference);
}

#[test]
fn filter_images_share_the_display_composition_budget() {
    let mut r = bounded_renderer(DocumentColor::default()).unwrap();
    let extent = [5184, 3456];
    let mut document=Document::new(PortableId::random(),extent[0],extent[1],layer_core::DocumentNames{paint:"Ink".into(),paper:"Paper".into()});
    let effect=effect_occurrence(&mut document,crate::tests::image_windows::program(false,false),"window fixture");set_root_entries(&mut document,vec![effect]);let scene=document.scene();
    let image_bytes = scene::Scene::capture_image_bound(scene, PixelRect::full(extent));
    for allowance in [0, 512 << 20, 1536 << 20] {
        r.native_edit.as_mut().unwrap().image_pixel_bytes = Some(allowance);
        let images = r.image_pixel_budget(0);
        assert!(images <= allowance);
        let plan = scene::windows::Plan::new(scene, extent, images, r.device.limits().max_texture_dimension_2d);
        if allowance == 0 { assert!(plan.is_err()); continue; }
        let plan = plan.unwrap();
        assert_eq!(plan.is_none(), image_bytes <= images);
    }
}

#[test]
fn visible_detail_matches_dense_composition_through_pan_wrap_rotation_and_resize() {
    let doc = document([1537, 769]);
    let mut dense = bounded_renderer(doc.composition().color).unwrap();
    dense.test.exact_display = true;
    let mut cached = bounded_renderer(doc.composition().color).unwrap();

    let mut a = crate::test_support::float_presenter(&dense);
    let mut b = crate::test_support::float_presenter(&cached);
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
        submit(&mut dense, &doc, view, index == 0, true);
        submit(&mut cached, &doc, view, index == 0, true);

        assert!(cached.scale_display.as_ref().unwrap().storage_bytes() <= crate::memory::CACHE_BYTES);
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
    submit(&mut cached, &doc, view([1., 0., 0., 1., -1., 0.]), false, true);
    assert_eq!(
        cached.metrics.composited_pixels, work,
        "resident pan must reuse detail"
    );
    assert_eq!(cached.composite_revision, revision);
    let sample=crate::test_support::color_sample(&mut cached,ColorSampleRequest {
        request_id:11,source:ColorSampleSource::Composite,position:[255,256],area:ColorSampleArea::Average5,
    });
    assert!(sample.rgba.iter().all(|v| v.is_finite()));
}

#[test]
fn filtered_masked_source_edits_and_restoration_refresh_detail_and_coarse_display() {
    use layer_core::{Point, Selection};
    let mut doc = document([777, 533]);
    let source = paint_at(&doc,0).base.as_ref().unwrap().image.storage().clone();
    let owner=doc.scene().order()[0];
    let initial=Selection::polygon(vec![Point{x:0.,y:0.},Point{x:760.,y:99.},Point{x:440.,y:533.}]).unwrap();
    let mask=coverage_mask(&mut doc,owner,[7, -9],Some(initial));doc.artwork.coverage.get_mut(mask).unwrap().default_coverage=0.;
    for name in ["20","21"] {let effect=effect_occurrence(&mut doc,crate::tests::image_windows::program(false,false),name);insert_occurrence(&mut doc,effect,0);}
    let original = doc.clone();
    let mut dense = bounded_renderer(doc.composition().color).unwrap();
    dense.test.exact_display = true;
    let mut r = bounded_renderer(doc.composition().color).unwrap();

    let mut a = crate::test_support::float_presenter(&dense);
    let mut b = crate::test_support::float_presenter(&r);
    let v = view([1., 0., 0., 1., -233., -111.]);
    let mut first = Vec::new();
    for step in 0..5 {
        match step {
            1 => occurrence_mut(&mut doc,2).offset = [17, -9],
            2 => {
                occurrence_mut(&mut doc,2).mask.as_mut().unwrap().inverted = true;
                occurrence_mut(&mut doc,0).opacity = 0.6;
            }
            3 => doc.working.inspect_mask=Some(owner),
            4 => doc = original.clone(),
            _ => {}
        }
        // Layer property actions request recomposition through FramePacket;
        // only camera motion uses an otherwise clean packet.
        submit(&mut dense, &doc, v, true, true);
        submit(&mut r, &doc, v, true, true);
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
        assert!(Arc::ptr_eq(paint_at(&doc,2).base.as_ref().unwrap().image.storage(), &source));
    }
    // Recreating the GPU cache reconstructs the same pixels from retained source.
    let mut recovered = bounded_renderer(doc.composition().color).unwrap();

    submit(&mut recovered, &doc, v, true, true);
    let mut p = crate::test_support::float_presenter(&recovered);
    close(&first, &present(&recovered, &mut p, v));
}

#[test]
fn in_surface_navigator_keeps_clipped_geometry() {
    let doc = document([1537, 769]);
    let mut r = bounded_renderer(doc.composition().color).unwrap();

    submit(&mut r, &doc, view([1., 0., 0., 1., -900., -400.]), true, true);
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
                scale: 1.,
                opacity: 1.,
            }],
        );
        pollster::block_on(presenter.prepare(&r)).unwrap();
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
    let mut doc = layer_core::Document::new(PortableId::random(), 4097, 1025, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    composition_mut(&mut doc).color = color;
    let mut r = bounded_renderer(color).unwrap();
    r.startup = Some(startup::Startup::new(r.compilation.clone()).unwrap());
    let (release, wait) = mpsc::channel();
    let (entered, blocked) = mpsc::channel();
    let compiler = &r.startup.as_ref().unwrap().compiler;
    compiler.enqueue(crate::startup::DOCUMENT, move || {
        entered.send(()).map_err(|e| e.to_string())?;
        wait.recv_timeout(Duration::from_secs(20))
            .map_err(|e| e.to_string())
    });
    blocked.recv_timeout(Duration::from_secs(20)).unwrap();
    let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
    r.prepare_startup(&doc, &brush, false).unwrap();
    assert!(!r.poll_startup().unwrap().canvas_ready);
    assert!(!r.scene_pipelines.scale.full.reduce.ready());
    assert!(!r.scene_pipelines.scale.full.reduce_pair.ready());

    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !r.poll_startup().unwrap().canvas_ready {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(r.scene_pipelines.scale.full.reduce.ready());
    assert!(r.scene_pipelines.scale.full.reduce_pair.ready());
    submit(&mut r, &doc, view([1., 0., 0., 1., 0., 0.]), true, true);
    assert!(r.scale_display.is_some());
    drop(r);
    startup::finish_shader_compiler_shutdown();
}

#[test]
fn committed_contact_strokes_present_their_canonical_native_pixels() {
    use layer_engine::{CanvasEngine, InstantFeedbackConfig, PenEvent, PenPhase,
        SampleFlags, ViewTransform, input_queue};
    for preset in [layer_core::DefaultBrushPreset::AntiquePen, layer_core::DefaultBrushPreset::BrushedInk] {
        let doc = layer_core::Document::new(PortableId::random(), 1024, 512, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        let v = view([0.5, 0., 0., 0.5, 0., 0.]);
        let (mut input, consumer) = input_queue(64);
        let mut engine = CanvasEngine::new(bounded_renderer(doc.composition().color).unwrap(), doc, consumer, v,
            ViewTransform { revision: 0, surface_to_document: [2., 0., 0., 2., 0., 0.] }).unwrap();
        engine.set_instant_feedback(InstantFeedbackConfig { enabled: false, ..Default::default() }).unwrap();
        let mut brush = layer_core::default_brush(preset);
        brush.diameter = 70.;
        brush.color_rgba_linear = [0.08, 0.015, 0.25, 1.];
        engine.set_brush(brush).unwrap();
        for i in 0..=32 {
            let t = i as f32 / 32.;
            input.push(PenEvent { timestamp_ns: (i + 1) * 8_333_333, pressure: if i < 32 { 0.2 + 0.7 * (t * std::f32::consts::PI).sin() } else { 0. },
                    ..crate::test_support::pen(i + 1, if i == 0 { PenPhase::Down } else if i == 32 { PenPhase::Up } else { PenPhase::Move }, [40. + 240. * t, 120. + 50. * (t * std::f32::consts::TAU).sin()], SampleFlags::PRIMARY) }).unwrap();
            let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
            loop {
                prepare_engine_frame(&mut engine);
                engine.render_frame().unwrap();
                engine.backend_mut().wait_idle().unwrap();
                if !engine.has_pending_input() { break; }
                assert!(std::time::Instant::now() < deadline, "input did not drain at sample {i}");
            }
        }
        let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
        while engine.backend().next_frame()==layer_render::FrameDemand::Work {
            prepare_engine_frame(&mut engine);
            engine.render_frame().unwrap();
            engine.backend_mut().wait_idle().unwrap();
            assert!(std::time::Instant::now() < deadline, "committed stroke did not settle");
        }
        assert_eq!(engine.metrics().committed_strokes, 1);
        let mut presenter = crate::test_support::float_presenter(engine.backend());
        let live = present(engine.backend(), &mut presenter, v);
        let doc = engine.document().clone();
        submit(engine.backend_mut(), &doc, v, true, true);
        assert!(live == present(engine.backend(), &mut presenter, v),
            "{preset:?}: the committed stroke must present the pixels Undo/Redo and reopening restore");
    }
}

#[test]
fn native_stroke_undo_redo_and_replaced_device_rebuild_visible_tiles_from_exact_backing() {
    use layer_engine::{
        CanvasEngine, PenEvent, PenPhase, SampleFlags, ViewTransform, input_queue,
    };
    let color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    };
    let mut doc = layer_core::Document::new(PortableId::random(), 1025, 513, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    composition_mut(&mut doc).color = color;
    let v = view([1., 0., 0., 1., 0., 0.]);
    let renderer = || bounded_renderer(color).unwrap();
    let (mut input, consumer) = input_queue(64);
    let mut engine =
        CanvasEngine::new(renderer(), doc, consumer, v, ViewTransform::IDENTITY).unwrap();
    let flush = |engine: &mut CanvasEngine<WgpuRasterizer>| {
        let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
        loop {
            prepare_engine_frame(engine);
            engine.render_frame().unwrap();
            if !engine.has_pending_input() && !engine.has_pending_document_edits()
                && (engine.has_active_stroke() || engine.backend().next_frame()!=layer_render::FrameDemand::Work) {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    };
    flush(&mut engine);
    let mut p = crate::test_support::float_presenter(engine.backend());
    let blank = present(engine.backend(), &mut p, v);
    for (i, phase) in [PenPhase::Down, PenPhase::Move, PenPhase::Up]
        .into_iter()
        .enumerate()
    {
        input
            .push(PenEvent { timestamp_ns: (i as u64 + 1) * 10_000_000, pressure: 0.37,
                    ..crate::test_support::pen(i as u64 + 1, phase, [245. + i as f32 * 18., 80.], SampleFlags::PRIMARY) })
            .unwrap();
        flush(&mut engine);
    }
    let painted = present(engine.backend(), &mut p, v);
    assert!(painted != blank);
    let root = paint_at(engine.document(),0).raster.clone();
    let backed = root.wait_data().unwrap();
    assert!(
        backed.tiles.len() >= 2,
        "stroke crosses native tile boundaries"
    );
    let exact: Vec<_> = backed
        .tiles
        .iter()
        .map(|(key, tile)| (*key, tile.wait_backing().unwrap().decode_on_worker().unwrap()))
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

    let mut p = crate::test_support::float_presenter(engine.backend());
    close(&painted, &present(engine.backend(), &mut p, v));
    let restored = paint_at(engine.document(),0).raster.wait_data().unwrap();
    for (key, bytes) in exact {
        assert_eq!(
            bytes,
            restored.tiles[&key]
                .wait_backing()
                .unwrap()
                .decode_on_worker()
                .unwrap()
        );
    }
}

#[test]
fn pass_through_edits_and_mode_changes_match_full_recomposition() {
    let mut doc = document([777, 533]);
    let photo = source_at(&doc,0);
    let upper=paint_occurrence(&mut doc,"grouped",None);doc.artwork.occurrences.get_mut(upper).unwrap().blend=layer_core::LayerBlend::Multiply;
    let make_effect=|doc:&mut Document,program:&str| {effect_occurrence(doc,layer_core::EffectInstance::new(layer_core::bundled_effect_catalog().get(program).unwrap().program()),"grouped")};
    let desaturate=make_effect(&mut doc,"black_white");let blur=make_effect(&mut doc,"gaussian_blur");let lower=paint_occurrence(&mut doc,"grouped",None);
    let id=stack_occurrence(&mut doc,"grouped",vec![upper,desaturate,blur,lower]);doc.artwork.occurrences.get_mut(id).unwrap().blend=layer_core::LayerBlend::PassThrough;insert_occurrence(&mut doc,id,0);
    let upper_id=doc.scene().source_target(upper).unwrap();let lower_id=doc.scene().source_target(lower).unwrap();
    let renderer = || crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    let mut incremental = renderer();
    let mut reference = renderer();
    reference.test.exact_display = true;
    let mut a = crate::test_support::float_presenter(&incremental);
    let mut b = crate::test_support::float_presenter(&reference);
    let v = centered_view(doc.composition().size, [320, 240], 0.4, 0.);
    fn find(doc:&mut Document,id:OccurrenceHandle)->&mut Occurrence {doc.artwork.occurrences.get_mut(id).unwrap()}
    for step in 0..8 {
        let target = match step {
            1 | 7 => Some((photo, [300., 300.])),
            2 => Some((lower_id, [500., 200.])),
            3 => Some((upper_id, [650., 400.])),
            _ => None,
        };
        let edited = matches!(step, 4..=6);
        match step {
            4 => find(&mut doc,id).blend = layer_core::LayerBlend::Normal,
            5 => find(&mut doc,id).blend = layer_core::LayerBlend::PassThrough,
            6 => {
                let mask=coverage_mask(&mut doc,id,[0, 0],None);doc.artwork.coverage.get_mut(mask).unwrap().default_coverage=0.6;
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
            if let Some((SourceTarget::Paint(paint),_))=target {
                doc.artwork.paint.get_mut(paint).unwrap().raster=layer_core::raster::RasterRevision::pending();
            }
            crate::test_support::submit_ready(r, FramePacket {
                view: v,
                dabs: &dabs,
                dab_batches: &batches,
                composite_all: all,
                ..packet(doc.scene(), doc.composition().size)
            }).unwrap();
            submit(r, &doc, v, false, false);
            if let Some((target,_))=target {doc.target_raster(target).unwrap().wait_data().unwrap();}
        }
        close(&present(&incremental, &mut a, v), &present(&reference, &mut b, v));
    }
    for layer in [lower_id, photo] {
        for moving in [true, false] {
            let transform = layer_render::TransformPreview {
                transaction: 1, target:layer, moving, selection: None,
                transform: layer_core::ImageTransform::affine(layer_core::Affine::translation(layer_core::Point { x: 23.5, y: -11.25 })),
            };
            reference.scene = None;
            for (mut r, all) in [(&mut incremental, false), (&mut reference, true)] {
                r.set_transform_preview(Some(&transform)).unwrap();
                crate::test_support::submit_ready(&mut r, FramePacket { view: v, composite_all: all, ..packet(doc.scene(), doc.composition().size) }).unwrap();
                submit(r, &doc, v, false, false);
            }
            assert!(incremental.next_frame()!=layer_render::FrameDemand::Work);
            assert!(reference.next_frame()!=layer_render::FrameDemand::Work);
            let actual = present(&incremental, &mut a, v);
            let exact = present(&reference, &mut b, v);
            if moving {
                let error = quality(&actual, &exact, display_mips::Plan::at([v.width_px, v.height_px], 0), wgpu::TextureFormat::Rgba32Float);
                assert!(error[0] < 0.004 && error[1] < 0.06, "moving filtered group: {error:?}");
            } else { close(&actual, &exact); }
        }
        for r in [&mut incremental, &mut reference] {
            r.set_transform_preview(None).unwrap();
            submit(r, &doc, v, false, false);
        }
        assert!(incremental.next_frame()!=layer_render::FrameDemand::Work);
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

fn photo(extent: [u32; 2]) -> layer_core::Document {
    let mut doc = layer_core::Document::new(PortableId::random(), extent[0], extent[1], layer_core::DocumentNames { paint: "Photo".into(), paper: "Paper".into() });
    let mut builder = SourceBuilder::new(extent, SourceInterpretation {
        channels: SourceChannels::Rgb, depth: SampleDepth::U8, profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false,
    }, 512 * 1024 * 1024).unwrap();
    let mut row = vec![0u8; extent[0] as usize * 3];
    for y in 0..extent[1] {
        for (x, pixel) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
            let x = x as u32;
            let hash = (x.wrapping_mul(0x9e37_79b9) ^ y.wrapping_mul(0x85eb_ca6b)).wrapping_mul(0xc2b2_ae35);
            let wave = ((x / 37 + y / 23) % 7) * 18;
            pixel.copy_from_slice(&[(hash >> 24) as u8 / 2 + wave as u8, ((x * 255) / extent[0]) as u8 / 2 + (hash >> 16) as u8 / 4, ((y * 255) / extent[1]) as u8 / 2 + (hash >> 8) as u8 / 4]);
        }
        builder.push_row(&row).unwrap();
    }
    paint_mut(&mut doc, 0).base = Some(layer_core::authored::PaintBase::new(Arc::new(builder.finish().unwrap()).into()));
    doc
}

fn fit_view(extent: [u32; 2], viewport: [u32; 2], zoom: f32, pan: f32) -> ViewState {
    ViewState { width_px: viewport[0], height_px: viewport[1], document_to_surface: [zoom, 0., 0., zoom,
        (viewport[0] as f32 - zoom * extent[0] as f32) * 0.5 + pan, (viewport[1] as f32 - zoom * extent[1] as f32) * 0.5] }
}

fn quantile(values: &mut [f64], q: f64) -> f64 {
    values.sort_by(f64::total_cmp);
    values[((values.len() - 1) as f64 * q).round() as usize]
}

#[test]
#[ignore = "workstation presenter benchmark at the tiers' Fit zooms"]
fn fit_presentation_frames() {
    let viewport = [2560, 1600];
    for (name, extent, zoom) in [("12 MP at the low tier's Fit", [4248, 2832], 0.298f32), ("61 MP at the top tier's Fit", [9504, 6336], 0.213)] {
        for omitted in [wgpu::Features::empty(), wgpu::Features::FLOAT32_FILTERABLE] {
            let doc = photo(extent);
            let mut r = WgpuRasterizer::native_headless_without(doc.composition().color, omitted).unwrap();
            submit(&mut r, &doc, fit_view(extent, viewport, zoom, 0.), true, false);
            let cache = r.scale_display.as_ref().unwrap();
            eprintln!("{name}, float32 filtering {}: display level {}, retained {} bytes, footprint {:.2} texels, format {:?}", omitted.is_empty(), cache.plan.level, cache.pages.lock().unwrap().bytes(),
                1. / zoom / (1 << cache.plan.level) as f32, cache.view().texture().format());
            let (_texture, target) = create_target(&r.device, viewport, wgpu::TextureFormat::Rgba8UnormSrgb, "fit benchmark target");
            let mut presenter = ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba8UnormSrgb, crate::SdrSurfaceColor::Srgb).unwrap();
            presenter.gpu_timings(&r, true);
            let mut gpu = Vec::new();
            let mut walls = Vec::new();
            for frame in 0..160 {
                let view = fit_view(extent, viewport, zoom, (frame % 40) as f32 * 3.);
                let started = std::time::Instant::now();
                crate::test_support::submit_ready(&mut r, FramePacket { view, composite_all: false, ..packet(doc.scene(), extent) }).unwrap();
                presenter.present(&r, &target, view, [0.2, 0.2, 0.2, 1.]).unwrap();
                r.wait_idle().unwrap();
                if frame >= 10 { walls.push(started.elapsed().as_secs_f64() * 1e3); }
                for _ in 0..100 {
                    let samples = presenter.gpu_timings(&r, true);
                    if !samples.is_empty() {
                        if frame >= 10 { gpu.extend(samples.iter().filter(|s| s.status == 1).map(|s| s.elapsed_ns as f64 / 1e6)); }
                        break;
                    }
                    r.wait_idle().unwrap();
                }
            }
            eprintln!("  pan presents: {} GPU samples, presenter GPU p50 {:.3} p95 {:.3} ms; frame wall p50 {:.2} p95 {:.2} ms",
                gpu.len(), quantile(&mut gpu, 0.5), quantile(&mut gpu, 0.95), quantile(&mut walls, 0.5), quantile(&mut walls, 0.95));
        }
    }
}


fn patterned(extent: [u32; 2]) -> layer_core::Document {
    let mut doc = layer_core::Document::new(PortableId::random(), extent[0], extent[1], layer_core::DocumentNames { paint: "Pattern".into(), paper: "Paper".into() });
    paint_mut(&mut doc, 0).base = Some(layer_core::authored::PaintBase::new(layer_core::color::source::rgba8_source(extent, |x, y|
        [if (x / 5 + y / 7) % 2 == 0 { 40 } else { 220 }, if (x / 13 + y / 17) % 2 == 0 { 40 } else { 220 },
         if (x / 2 + y / 3) % 2 == 0 { 40 } else { 220 }, 255]).into()));
    doc
}

fn first_preview_stroke(doc:layer_core::Document,view:ViewState) {
    let extent=doc.composition().size;
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    submit(&mut r,&doc,view,true,false);
    let mut dab=crate::tests::test_dab([200.,300.],[0.9,0.02,0.1,0.8],1.);
    dab.radii=[60.;2];
    let batch=dab_batch(source_at(&doc,0),crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen),dab.bounds());
    for kind in [DabBatchKind::Preview,DabBatchKind::Persistent] {
        let batch=DabBatch {kind,..batch.clone()};
        let frame=FramePacket {view,dabs:&[dab],dab_batches:std::slice::from_ref(&batch),composite_all:false,..packet(doc.scene(),extent)};
        if let Err(error)=crate::test_support::submit_ready(&mut r,frame) {panic!("{kind:?} batch after readiness: {error:?}");}
    }
}

fn blank_first_stroke_document()->layer_core::Document {
    layer_core::Document::new(PortableId::random(),1100,700,layer_core::DocumentNames {paint:"First stroke".into(),paper:"Paper".into()})
}

#[test]
fn first_preview_stroke_blank_native() {
    first_preview_stroke(blank_first_stroke_document(),crate::test_support::view([1100,700]));
}

#[test]
fn first_preview_stroke_blank_fit() {
    first_preview_stroke(blank_first_stroke_document(),ViewState {width_px:330,height_px:210,document_to_surface:[0.2976,0.,0.,0.2976,1.,1.]});
}

#[test]
fn first_preview_stroke_pattern_native() {
    first_preview_stroke(patterned([1100,700]),crate::test_support::view([1100,700]));
}

#[test]
fn first_preview_stroke_pattern_fit() {
    first_preview_stroke(patterned([1100,700]),ViewState {width_px:330,height_px:210,document_to_surface:[0.2976,0.,0.,0.2976,1.,1.]});
}

fn half_float_presentation(r: &WgpuRasterizer) -> bool {
    r.presentation_views().is_some_and(|[moved, _, kept]| [moved, kept].iter().all(|view| view.texture().format() == wgpu::TextureFormat::Rgba16Float))
}

#[test]
fn fit_views_read_one_half_float_level_closer_to_an_area_average() {
    let extent = [1100, 700];
    let doc = patterned(extent);
    let color = doc.composition().color;
    for (zoom, level, bound) in [(0.2137f32, 2, 0.084f32), (0.2976, 1, 0.086)] {
        for bounded in [false, true] {
            let mut r = if bounded { bounded_renderer(color) } else { WgpuRasterizer::new_native_headless(color) }.unwrap();
            let size = extent.map(|n| (n as f32 * zoom).ceil() as u32 + 2);
            let view = ViewState { width_px: size[0], height_px: size[1], document_to_surface: [zoom, 0., 0., zoom, 1., 1.] };
            submit(&mut r, &doc, view, true, bounded);
            assert_eq!(r.scale_display.as_ref().unwrap().plan.level, level);
            assert!(half_float_presentation(&r), "zoom {zoom} presents half-float levels");
            let actual = present(&r, &mut crate::test_support::float_presenter(&r), view);
            let mut exact = WgpuRasterizer::new_native_headless(color).unwrap();
            exact.test.reference = true;
            let high = ViewState { width_px: size[0] * 8, height_px: size[1] * 8, document_to_surface: view.document_to_surface.map(|n| n * 8.) };
            submit(&mut exact, &doc, high, true, false);
            let reference = present(&exact, &mut crate::test_support::float_presenter(&exact), high);
            let mut errors = Vec::new();
            for y in 2..(extent[1] as f32 * zoom) as u32 {
                for x in 2..(extent[0] as f32 * zoom) as u32 {
                    let mut expected = [0f32; 4];
                    for yy in y * 8..(y + 1) * 8 { for xx in x * 8..(x + 1) * 8 {
                        for c in 0..4 { expected[c] += reference[(yy * high.width_px + xx) as usize][c] / 64.; }
                    }}
                    errors.push(actual[(y * size[0] + x) as usize].iter().zip(expected).map(|(a, b)| (a - b).abs()).fold(0., f32::max));
                }
            }
            let mean = errors.iter().sum::<f32>() / errors.len() as f32;
            assert!(mean < bound, "zoom {zoom} bounded {bounded}: mean area error {mean}");
        }
    }
}

#[test]
fn half_float_levels_present_alike_with_or_without_float32_filtering() {
    let extent = [1100, 700];
    let doc = patterned(extent);
    let color = doc.composition().color;
    for zoom in [0.2137f32, 0.2976] {
        let size = extent.map(|n| (n as f32 * zoom).ceil() as u32 + 2);
        let view = ViewState { width_px: size[0], height_px: size[1], document_to_surface: [zoom, 0., 0., zoom, 1., 1.] };
        let shown = |mut r: WgpuRasterizer| {
            submit(&mut r, &doc, view, true, false);
            assert!(half_float_presentation(&r), "zoom {zoom} presents half-float levels");
            present(&r, &mut crate::test_support::float_presenter(&r), view)
        };
        let filtered = shown(WgpuRasterizer::new_native_headless(color).unwrap());
        let texels = shown(WgpuRasterizer::native_headless_without(color, wgpu::Features::FLOAT32_FILTERABLE).unwrap());
        let error = crate::test_support::max_error(&filtered, &texels);
        assert!(error < 1e-5, "zoom {zoom}: half-float levels filter alike without float32 filtering: {error}");
    }
}

#[test]
fn the_surround_skips_the_artwork_shader() {
    let extent = [1100, 700];
    let doc = patterned(extent);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let view = ViewState { width_px: 800, height_px: 600, document_to_surface: [0.25, 0., 0., 0.25, 300., 200.] };
    submit(&mut r, &doc, view, true, false);
    let mut presenter = crate::test_support::float_presenter(&r);
    let shown = present(&r, &mut presenter, view);
    let [width, height] = [1100. * 0.25, 700. * 0.25];
    assert!(presenter.shaded_area_pixels() as f32 <= (width + 8.) * (height + 8.), "the artwork shader runs on {} pixels", presenter.shaded_area_pixels());
    let surround = shown[0];
    for (index, pixel) in shown.iter().enumerate() {
        let [x, y] = [(index % 800) as f32 + 0.5, (index / 800) as f32 + 0.5];
        if x < 300. || y < 200. || x > 300. + width || y > 200. + height { assert_eq!(*pixel, surround, "surround pixel {x}, {y}"); }
    }
    assert!(crate::test_support::max_error(&[shown[(200 + 80) * 800 + 400]], &[surround]) > 0.1, "the artwork shows inside the surround");
}

#[test]
fn painting_at_fit_updates_the_half_float_level_it_presents() {
    let extent = [1100, 700];
    let doc = patterned(extent);
    let color = doc.composition().color;
    let fit = ViewState { width_px: 330, height_px: 210, document_to_surface: [0.2976, 0., 0., 0.2976, 1., 1.] };
    let native = crate::test_support::view(extent);
    for bounded in [false, true] {
        let renderer = || if bounded { bounded_renderer(color) } else { WgpuRasterizer::new_native_headless(color) }.unwrap();
        let (mut painted, mut fresh) = (renderer(), renderer());
        submit(&mut painted, &doc, fit, true, bounded);
        submit(&mut fresh, &doc, native, true, bounded);
        let mut presenter = crate::test_support::float_presenter(&painted);
        let before = present(&painted, &mut presenter, fit);
        let conversions = painted.metrics.half_level_passes;
        for step in 0..3 {
            let mut dab = crate::tests::test_dab([200. + step as f32 * 250., 300.], [0.9, 0.02, 0.1, 0.8], 1.);
            dab.radii = [60.; 2];
            let batch = dab_batch(source_at(&doc, 0), crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen), dab.bounds());
            for kind in [DabBatchKind::Preview, DabBatchKind::Persistent] {
                let batch = DabBatch { kind, ..batch.clone() };
                for (mut r, view) in [(&mut painted, fit), (&mut fresh, native)] {
                    let frame=FramePacket {view,dabs:&[dab],dab_batches:std::slice::from_ref(&batch),composite_all:false,..packet(doc.scene(),extent)};
                    if bounded {working_scene_frame(&mut r,frame);}
                    crate::test_support::submit_ready(&mut r,frame).unwrap();
                }
                present(&painted, &mut presenter, fit);
            }
        }
        assert_eq!(painted.metrics.half_level_passes, conversions, "bounded {bounded}: strokes convert their half-float pixels while reducing them");
        submit(&mut painted, &doc, fit, false, bounded);
        submit(&mut fresh, &doc, fit, false, bounded);
        assert!(half_float_presentation(&painted) && half_float_presentation(&fresh), "bounded {bounded}");
        let painted_pixels = present(&painted, &mut presenter, fit);
        let fresh_pixels = present(&fresh, &mut crate::test_support::float_presenter(&fresh), fit);
        assert!(crate::test_support::max_error(&painted_pixels, &before) > 0.3, "bounded {bounded}: the strokes show");
        let error = crate::test_support::max_error(&painted_pixels, &fresh_pixels);
        assert!(error < 2e-3, "bounded {bounded}: strokes painted at Fit present like a fresh display: {error}");
    }
}

#[test]
fn precision_change_preserves_a_settled_61mp_picture_until_new_pages_commit() {
    use crate::display_precision::DisplayPrecision;

    let extent=[9504,6336];
    let viewport=[2560,1600];
    let mut doc=photo(extent);
    let owner=doc.scene().order().iter().copied().find(|&owner|doc.scene().paint_source(owner).is_some()).unwrap();
    set_root_entries(&mut doc,vec![owner]);
    let zoom=(viewport[0] as f32/extent[0] as f32).min(viewport[1] as f32/extent[1] as f32);
    let camera=fit_view(extent,viewport,zoom,0.);
    let mut r=bounded_renderer(doc.composition().color).unwrap();
    submit(&mut r,&doc,camera,true,true);
    assert!(r.next_frame()!=layer_render::FrameDemand::Work);
    let cache=r.scale_display.as_ref().unwrap();
    assert_eq!(cache.precision,DisplayPrecision::Half);
    let before=cache.presentable_pages();
    assert!(before>0);
    let request=Request {plan:cache.plan,evaluation:cache.evaluation};
    let store=cache.pages.clone();
    let old_key=cache.page_key(cache.graph.root.clone().unwrap(),cache.plan.bounds);
    let mut presenter=ViewportPresenter::for_surface(&r,wgpu::TextureFormat::Rgba8UnormSrgb,crate::SdrSurfaceColor::Srgb).unwrap();
    let (texture,target)=create_target(&r.device,viewport,wgpu::TextureFormat::Rgba8UnormSrgb,"precision change presentation");
    presenter.present(&r,&target,camera,[0.;4]).unwrap();
    r.wait_idle().unwrap();
    assert_eq!(presenter.presented_camera(),Some(camera));
    assert!(presenter.canvas_revision().is_some());

    let effect=effect_occurrence(&mut doc,layer_core::EffectInstance::new(crate::tests::fixture("exposure").program()),"Precision change");
    set_effect_value(&mut doc,effect,"exposure",layer_core::EffectValue::Number(0.5));
    doc.artwork.occurrences.get_mut(effect).unwrap().attachment=Attachment::Effect;
    insert_occurrence(&mut doc,effect,0);
    let frame=FramePacket {view:camera,composite_all:false,..packet(doc.scene(),extent)};
    let mut next=Cache::new(&r,request,doc.scene().order().len());
    crate::scene::scale::tests::prepare_fixture_frame(&r,frame,r.scene.as_ref().unwrap());
    next.prepare_graph(&r,frame,r.scene.as_ref().unwrap()).unwrap();
    assert_eq!(next.precision,DisplayPrecision::Full);
    let key=next.page_key(next.graph.root.clone().unwrap(),next.plan.bounds);
    assert!(key.expression!=old_key.expression);
    let area=key.plan().bounds.intersect(next.plan.bounds);
    assert!(store.lock().unwrap().get(&key,area,pages::Accuracy::Exact,0,crate::residency::Class::Visible).is_none(),"prior-key pages cannot satisfy an exact read after the effect changes");
    drop(next);

    let deadline=std::time::Instant::now()+READBACK_TIMEOUT;
    let mut committed=false;
    loop {
        working_scene_frame(&mut r,frame);
        crate::test_support::submit_ready(&mut r,frame).unwrap();
        assert!(r.has_presentable_frame(),"precision changes cannot withhold an existing picture");
        let cache=r.scale_display.as_ref().unwrap();
        let new_pages=cache.pages.lock().unwrap().retained_pages().into_iter().any(|page|
            page.key.expression==key.expression && page.key.format==key.format && !page.current.is_empty());
        assert!(cache.presentable_pages()>=before,"prior-key coverage must remain until all replacement pages commit");
        if !committed && !new_pages {
            assert!(cache.pages.lock().unwrap().get(&key,area,pages::Accuracy::Exact,0,crate::residency::Class::Visible).is_none());
        }
        committed|=new_pages;
        presenter.present(&r,&target,camera,[0.;4]).unwrap();
        assert_eq!(presenter.presented_camera(),Some(camera));
        assert!(presenter.canvas_revision().is_some());
        let image=crate::layer_tests::page_bytes(&r,&texture);
        let row=&image[(viewport[1] as usize/2*viewport[0] as usize)*4..((viewport[1] as usize/2+1)*viewport[0] as usize)*4];
        let interior=&row[viewport[0] as usize/4*4..viewport[0] as usize*3/4*4];
        assert!(interior.as_chunks::<4>().0.iter().all(|pixel|pixel[3]==255),"the photo remains opaque on every presented frame");
        let (low,high)=interior.as_chunks::<4>().0.iter().map(|pixel|pixel[0]).fold((255,0),|(low,high),value|(low.min(value),high.max(value)));
        assert!(high-low>16,"the presented picture cannot become a blank surround or paper frame");
        r.wait_idle().unwrap();
        if r.next_frame()!=layer_render::FrameDemand::Work {break;}
        assert!(std::time::Instant::now()<deadline,"new-key effect pages must settle");
    }
    assert!(committed);
    assert_eq!(r.scale_display.as_ref().unwrap().precision,DisplayPrecision::Full);
    assert!(store.lock().unwrap().get(&key,area,pages::Accuracy::Exact,0,crate::residency::Class::Visible).is_some());
}

#[test]
fn published_preview_survives_a_gated_persistent_composite_handoff() {
    let extent=[PAGE_SIZE;2];
    let image=||rgba8_source(extent,|x,y|[31+(x%113) as u8,47+(y%127) as u8,91,255]);
    let warm=image();let cold=image();assert!(!Arc::ptr_eq(&warm,&cold));
    let mut doc=super::document_at(extent);
    composition_mut(&mut doc).color.depth=SampleDepth::F32;
    composition_mut(&mut doc).blend=layer_core::BlendSpace::Linear;
    let ink=doc.scene().order()[0];paint_mut(&mut doc,0).base=None;
    let lower=paint_occurrence(&mut doc,"Gated identical base",Some(warm));
    set_root_entries(&mut doc,vec![ink,lower]);
    let target=source_at(&doc,0);
    let v=ViewState {width_px:PAGE_SIZE,height_px:PAGE_SIZE,document_to_surface:[1.,0.,0.,1.,0.,0.]};
    let frame=FramePacket {view:v,blend_space:layer_core::BlendSpace::Linear,..packet(doc.scene(),extent)};
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    crate::test_support::submit_published(&mut r,frame);crate::test_support::complete(&r);
    let mut presenter=crate::test_support::float_presenter(&r);
    let before=present(&r,&mut presenter,v);
    let mut dab=crate::tests::test_dab([128.,128.],[0.9,0.02,0.1,0.6],1.);dab.radii=[24.;2];
    let mut batch=dab_batch(target,crate::layer_tests::preset_style(DefaultBrushPreset::GPen),dab.bounds());
    batch.stroke_id=layer_core::StrokeId(7);batch.kind=DabBatchKind::Preview;batch.stroke_start=true;batch.stroke_end=false;
    let preview=FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),composite_all:false,..frame};
    crate::test_support::submit_ready(&mut r,preview).unwrap();crate::test_support::complete(&r);
    let region=pixel_rect(dab.bounds(),extent);
    assert!(r.scale_display.as_ref().unwrap().coverage_snapshot().ready(region,pages::Accuracy::Display),"the preview must be fully published before the handoff");
    let shown=present(&r,&mut presenter,v);
    assert!(crate::test_support::max_error(&shown,&before)>0.05,"the preview must show actual semitransparent ink");
    batch.kind=DabBatchKind::Persistent;batch.stroke_end=true;
    let mut reference=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    reference.test.reference=true;
    crate::test_support::submit_published(&mut reference,FramePacket {
        dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame});
    crate::test_support::complete(&reference);
    let mut reference_presenter=crate::test_support::float_presenter(&reference);
    let expected=present(&reference,&mut reference_presenter,v);
    close(&shown,&expected);
    let gate=crate::test_support::GatedStore::new(&layer_core::raster_storage::RetainedTiles::from_sources([cold.as_ref()]),false);
    assert!(r.source_tiles.borrow().prepared_view(&cold,[0;2]).is_none());
    let mut changed=doc.clone();paint_mut(&mut changed,1).base=Some(PaintBase::new(cold.into()));
    let up=FramePacket {scene:changed.scene(),dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),composite_all:false,..frame};
    crate::test_support::submit_ready(&mut r,up).unwrap();crate::test_support::complete(&r);gate.gate.entered();
    let idle=FramePacket {scene:changed.scene(),dabs:&[],dab_batches:&[],composite_all:false,..frame};
    for _ in 0..3 {
        let cache=r.scale_display.as_ref().unwrap();let coverage=cache.coverage_snapshot();
        assert!(pages::Coverage::covers(&coverage.written,region),"gated replacement must retain written preview pixels");
        assert!(!coverage.ready(region,pages::Accuracy::Display),"the closed source must prevent a current persistent composite");
        close(&shown,&present(&r,&mut presenter,v));
        crate::test_support::submit_ready(&mut r,idle).unwrap();crate::test_support::complete(&r);
    }
    let cache=r.scale_display.as_ref().unwrap();
    let pending_receipt=crate::test_support::published_coverage(crate::test_support::coverage_hash(&cache.geometry)).unwrap().receipt;
    gate.gate.release();
    crate::test_support::submit_published(&mut r,idle);crate::test_support::complete(&r);
    let cache=r.scale_display.as_ref().unwrap();
    assert!(cache.coverage_snapshot().ready(region,pages::Accuracy::Display));
    let published=crate::test_support::published_coverage(crate::test_support::coverage_hash(&cache.geometry)).unwrap();
    assert_ne!(published.receipt,pending_receipt,"persistent replacement must publish a new current receipt");
    assert!(published.written.iter().any(|word|*word!=0));
    close(&expected,&present(&r,&mut presenter,v));
    crate::test_support::submit_ready(&mut r,idle).unwrap();crate::test_support::complete(&r);
    close(&expected,&present(&r,&mut presenter,v));
}

#[path="undo_integrity_tests.rs"]
mod undo_integrity;

#[test]
fn a_24mp_fit_reshow_finishes_within_two_frames_once_its_real_inputs_are_resident() {
    let extent=[6000,4000];let area=PixelRect::full(extent);
    let mut doc=super::document_at(extent);
    composition_mut(&mut doc).color.depth=SampleDepth::F32;composition_mut(&mut doc).blend=layer_core::BlendSpace::Linear;
    let image=rgba8_source(extent,|x,y|[31+(x/1024) as u8*27,47+(y/1024) as u8*31,113,255]);
    paint_mut(&mut doc,0).base=Some(PaintBase::new(image.clone().into()));
    let owner=doc.scene().order()[0];let id=source_at(&doc,0);
    let v=ViewState {width_px:600,height_px:800,document_to_surface:[0.1,0.,0.,0.1,0.,0.]};
    let frame=FramePacket {view:v,blend_space:layer_core::BlendSpace::Linear,..packet(doc.scene(),extent)};
    let mut r=bounded_renderer(doc.composition().color).unwrap();
    working_scene_frame(&mut r,frame);crate::test_support::submit_settled(&mut r,frame);
    let mut presenter=crate::test_support::float_presenter(&r);let shown=present(&r,&mut presenter,v);
    let mut hidden=doc.clone();hidden.artwork.occurrences.get_mut(owner).unwrap().visible=false;
    let hide=FramePacket {scene:hidden.scene(),composite_all:false,time_seconds:0.1,..frame};
    crate::test_support::submit_settled(&mut r,hide);crate::test_support::complete(&r);
    drop(r.scale_display.take());r.navigator=Default::default();
    r.scene.as_ref().unwrap().scale_pages.lock().unwrap().trim(0,crate::residency::Class::Visible);
    assert!(r.scene.as_ref().unwrap().scale_pages.lock().unwrap().retained_pages().is_empty(),"source and composite Store pages must really be evicted");
    *r.source_tiles.get_mut()=crate::scene::sources::DecodedTiles::within(&r.scene_tiers.display,r.device.working_space());
    r.memory.invalidate_limits();r.publish_memory();r.refresh_memory_limits();
    let coordinates=page_coordinates(area).collect::<Vec<_>>();assert_eq!(coordinates.len(),384);
    assert!(coordinates.iter().all(|coordinate|r.source_tiles.borrow().prepared_view(&image,*coordinate).is_none()));
    let show=FramePacket {composite_all:false,time_seconds:0.2,..frame};
    crate::test_support::submit_ready(&mut r,show).unwrap();crate::test_support::complete(&r);
    assert!(!r.scale_display.as_ref().unwrap().coverage_snapshot().ready(area,pages::Accuracy::Display),
        "the first cold show must leave real work owed before the residency condition is met");
    crate::memory::memory_tests::declare_source_budget(&mut r,show,coordinates.len()+16,16<<20,44<<20);
    let expected=r.scene.as_ref().unwrap().scale_pages.lock().unwrap().expected().unwrap().clone();
    let wanted=crate::scene::sources::declare_colors(&r,show.scene,expected,None,id,area).unwrap();
    let ((),resident)=wanted.accept(&mut r,&mut (),|_,r,inputs| {
        let bytes=inputs.additional_bytes(&r.source_tiles.borrow());
        r.admit_display(None,crate::residency::Class::Visible,bytes).ok().map(|reservation|(reservation,bytes))
    },|_,_|Ok(()),|_,_,_|Ok(())).unwrap().expect("Core must admit the ample native-input fixture without overriding a grant");
    assert_eq!(resident.len(),coordinates.len());
    let deadline=std::time::Instant::now()+READBACK_TIMEOUT;
    loop {
        resident.prepare(&mut r).unwrap();let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        r.drain_image_decodes(&mut encoder).unwrap();
        for &coordinate in &coordinates {
            let read=crate::scene::sources::accepted_image(&resident,&image,coordinate).unwrap();
            match read.encode(&mut r,&mut encoder,crate::residency::Class::Visible) {Ok(_)=>{},Err(error) if error.is_preparation_pending()=>{},Err(error)=>panic!("{error}")}
        }
        r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
        if resident.ready_for(&wanted) && coordinates.iter().all(|coordinate|r.source_tiles.borrow().prepared_view(&image,*coordinate).is_some()) {break;}
        assert!(std::time::Instant::now()<deadline,"all actual native GPU inputs must become ready before counting composition frames");
        std::thread::yield_now();
    }
    let source_bytes=r.source_tiles.borrow().gpu_bytes();
    let mut completed=None;let mut coverage=Vec::new();
    for index in 1..=2 {
        assert!(coordinates.iter().all(|coordinate|r.source_tiles.borrow().prepared_view(&image,*coordinate).is_some()),"resident inputs cannot be assumed from elapsed time");
        let next=FramePacket {time_seconds:0.2+index as f32*0.1,..show};
        assert!(r.can_submit());assert!(r.raster_dependencies_ready(next,None));r.submit(next).unwrap();crate::test_support::complete(&r);
        let cache=r.scale_display.as_ref().unwrap();let current=cache.coverage_snapshot();
        coverage.push((index,current.matching.len(),pages::coordinates(cache.plan.bounds,cache.plan.level).count()));
        if current.ready(area,pages::Accuracy::Display) && completed.is_none() {completed=Some(index);}
    }
    crate::test_support::submit_settled(&mut r,FramePacket {time_seconds:0.5,..show});
    close(&shown,&present(&r,&mut presenter,v));
    assert!(completed.is_some(),"24MP Fit re-show must finish within two still frames after all384 needed GPU inputs are actually resident: complete={completed:?}, coverage={coverage:?}, source_physical={source_bytes}");
}

#[test]
fn native_to_fit_stroke_undo_preserves_attachment_precision_and_pixels() {
    use layer_core::EffectValue;
    use layer_engine::{CanvasEngine, PenPhase, SampleFlags, ViewTransform, input_queue};
    let extent = [2133, 1600];
    let mut doc = Document::new(PortableId::random(), extent[0], extent[1], layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    composition_mut(&mut doc).color = DocumentColor {space:RgbSpace::Srgb,depth:SampleDepth::U8};
    composition_mut(&mut doc).blend = layer_core::BlendSpace::Perceptual;
    let paper = doc.scene().order()[1];
    set_effect_value(&mut doc,paper,"texture",EffectValue::Texture(layer_core::paper::built_in_paper("cold_press").unwrap().texture.clone()));
    set_effect_value(&mut doc,paper,"relief",EffectValue::Number(100.));
    set_effect_value(&mut doc,paper,"light",EffectValue::Number(225.));
    set_effect_value(&mut doc,paper,"color",EffectValue::Color(layer_core::color::RgbColor::new(RgbSpace::Srgb,[244./255.,241./255.,232./255.,1.]).unwrap()));
    let authored = doc.artwork.clone();
    let native = ViewState {width_px:1418,height_px:988,document_to_surface:[1.,0.,0.,1.,0.,0.]};
    let fit = ViewState {document_to_surface:[0.19957805,0.,0.,0.19957805,524.65,425.83755],..native};
    let color = doc.composition().color;
    let (mut input, consumer) = input_queue(64);
    let mut engine = CanvasEngine::new(WgpuRasterizer::new_native_headless(color).unwrap(),doc,consumer,native,ViewTransform::IDENTITY).unwrap();
    let flush = |engine:&mut CanvasEngine<WgpuRasterizer>| {
        let deadline = std::time::Instant::now()+READBACK_TIMEOUT;
        loop {
            engine.render_frame().unwrap();
            if !engine.has_pending_input() && !engine.has_pending_document_edits()
                && (engine.has_active_stroke() || engine.backend().next_frame()!=layer_render::FrameDemand::Work) {break;}
            assert!(std::time::Instant::now()<deadline,"native-to-Fit engine work did not publish");
            std::thread::yield_now();
        }
    };
    flush(&mut engine);
    let resident = engine.backend().scale_display.as_ref().unwrap().resident_bytes();
    assert!(resident>0,"initial native view must admit the resident hierarchy before Fit");
    let inverse = ViewTransform {revision:1,surface_to_document:[1./fit.document_to_surface[0],0.,0.,1./fit.document_to_surface[3],-fit.document_to_surface[4]/fit.document_to_surface[0],-fit.document_to_surface[5]/fit.document_to_surface[3]]};
    engine.set_view(fit,inverse);
    engine.render_frame().unwrap();
    flush(&mut engine);
    let capture = |r:&WgpuRasterizer| {
        let formats = r.presentation_views().unwrap().map(|view|view.texture().format());
        let (texture,target) = create_target(&r.device,[fit.width_px,fit.height_px],wgpu::TextureFormat::Bgra8UnormSrgb,"native-to-Fit attachment");
        let mut presenter = ViewportPresenter::for_surface(r,wgpu::TextureFormat::Bgra8UnormSrgb,crate::SdrSurfaceColor::Srgb).unwrap();
        pollster::block_on(presenter.prepare(r)).unwrap();
        presenter.present(r,&target,fit,[0.2,0.2,0.2,1.]).unwrap();
        (formats,crate::layer_tests::page_bytes(r,&texture))
    };
    let baseline = capture(engine.backend());
    for (i,phase) in [PenPhase::Down,PenPhase::Move,PenPhase::Up].into_iter().enumerate() {
        let mut event = crate::test_support::pen(i as u64+1,phase,[780.+i as f32*12.,630.],SampleFlags::PRIMARY);
        event.view_revision = inverse.revision;
        input.push(event).unwrap();
        flush(&mut engine);
    }
    let painted = capture(engine.backend());
    let stroke_checkpoint = engine.checkpoint();
    let undo_applied = engine.undo().unwrap();
    flush(&mut engine);
    let undone = capture(engine.backend());
    let mut fresh = WgpuRasterizer::new_native_headless(color).unwrap();
    submit(&mut fresh,engine.document(),fit,true,false);
    let control = capture(&fresh);
    let center = ((634*fit.width_px+796)*4) as usize;
    let paper_visible = baseline.1[center..center+3].iter().all(|v|*v>180) && baseline.1[center+3]==255;
    let restored_authored = engine.document().artwork == authored;
    let differences = baseline.1.iter().zip(&undone.1).filter(|(a,b)|a!=b).count();
    assert!(paper_visible,"first fitted attachment must contain the lit paper");
    assert!(painted.1!=baseline.1,"seed stroke must visibly change the attachment");
    assert!(restored_authored && engine.checkpoint()!=stroke_checkpoint && !engine.can_undo() && engine.can_redo(),"real Undo must return the authored baseline and advance history");
    assert!(undo_applied,"seed stroke must produce a real Undo entry");
    assert_eq!(baseline.0,undone.0,"camera-only Fit and real Undo must select stable attachment precision");
    assert_eq!(baseline.0,control.0,"fresh Fit must select the baseline attachment precision");
    assert!(baseline.1==undone.1,"real Undo changed {differences} attachment bytes");
    assert!(baseline.1==control.1,"fresh Fit differs from the first fitted baseline attachment");
}
