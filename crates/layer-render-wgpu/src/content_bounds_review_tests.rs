use super::*;

#[test]
fn batched_bounds_preserve_source_reads_before_late_color_and_mask_restores() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().size = [2048, 512];
    paint(&mut document).domain = [2048, 512];
    paint(&mut document).base = Some(layer_core::authored::PaintBase::new((source([2048, 512], |x, y| if x == 3 && y == 7 { 255 } else { 0 })).into()));
    let late_page = |revision: RasterRevision| {
        let mut data = (*revision.wait_data().unwrap()).clone();
        data.tiles = data.tiles.into_iter().map(|(mut key, tile)| {
            key.coordinate = [6, 0];
            (key, tile)
        }).collect();
        RasterRevision::backed(data)
    };
    paint(&mut document).raster = late_page(raster(RasterPlane::Color, document.composition().color,
        |x, y| if (x == 20 && y == 17) || (x == 40 && y == 40) { 1. } else { 0. }));
    let h = owner(&document);
    let mask = attach_mask(&mut document, h, Point::default());
    document.artwork.coverage.get_mut(mask).unwrap().raster = late_page(raster(RasterPlane::Mask, document.composition().color,
        |x, y| if x == 20 && y == 17 { 0.5 } else { 0. }));
    let captured_bounds = |document: &Document| {
        let mut capture = renderer.snapshot_gpu().capture_scene(captured(document), SceneScope::All, Default::default()).unwrap();
        capture.read_region([0, 0, 2048, 512]).unwrap().iter().enumerate()
            .filter(|(_, pixel)| pixel[3] > 0.).fold(Rect::EMPTY, |bounds, (i, _)| {
                let x = (i % 2048) as f32;
                let y = (i / 2048) as f32;
                bounds.union(rect(x, y, x + 1., y + 1.))
            })
    };
    let visible = captured_bounds(&document);
    assert_eq!(visible, rect(3., 7., 1557., 18.), "independent masked composite");
    let mut unmasked = document.clone();
    occurrence(&mut unmasked).mask = None;
    let raw = captured_bounds(&unmasked);
    assert_eq!(raw, rect(3., 7., 1577., 41.), "independent raw content composite");
    assert_eq!(bounds(&renderer, &document, ContentScope::Target(paint_target(&document))), raw,
        "queued source-only pages precede native color restoration near the third boundary page");
    for scope in [ContentScope::Canvas, ContentScope::Visible] {
        assert_eq!(bounds(&renderer, &document, scope), visible,
            "{scope:?}: later color and mask restores cannot overtake queued source reads");
    }
    assert_eq!(bounds(&renderer, &document, ContentScope::All), raw);
}

#[test]
fn shared_bounds_pipeline_preserves_alpha_across_working_depth_and_space_changes() {
    let gpu = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut original_pipeline = None;
    let mut transfers: Vec<crate::native_tiles::NativeTransfer> = Vec::new();
    for (depth, space) in [
        (SampleDepth::U8, layer_core::color::RgbSpace::Srgb),
        (SampleDepth::U16, layer_core::color::RgbSpace::DisplayP3),
        (SampleDepth::U16, layer_core::color::RgbSpace::AdobeRgb),
        (SampleDepth::F16, layer_core::color::RgbSpace::ProPhoto),
        (SampleDepth::F32, layer_core::color::RgbSpace::Srgb),
    ] {
        let color = DocumentColor { depth, space };
        let renderer = WgpuRasterizer::native_capture_on_gpu(gpu.adapter.clone(), gpu.device.clone(), gpu.queue.clone(), color).unwrap();
        let transfer = renderer.source_tiles.borrow_mut().prepare_transfer(&renderer.device, space).unwrap();
        let shared = gpu.source_tiles.borrow_mut().prepare_transfer(&gpu.device, space).unwrap();
        assert_eq!(transfer, shared, "device clones share the exact native transfer handle for {space:?}");
        if let Some(previous) = transfers.iter().find(|previous| previous.curve == transfer.curve) {
            assert_eq!(previous, &transfer, "sRGB and Display P3 retain one native curve identity");
        } else {
            assert!(transfers.iter().all(|previous| previous.table != transfer.table), "distinct curves retain distinct tables");
            transfers.push(transfer);
        }
        let mut document = document(color);
        let alpha = match depth { SampleDepth::U8 => 1. / 255., SampleDepth::U16 => 1. / 65535., _ => 1. / 65536. };
        paint(&mut document).raster = raster(RasterPlane::Color, color,
            |x, y| if (x == 17 && y == 23) || (x == 61 && y == 47) { alpha } else { 0. });
        for scope in [ContentScope::Target(paint_target(&document)), ContentScope::Visible] {
            assert_eq!(bounds(&renderer, &document, scope), rect(17., 23., 62., 48.), "{depth:?} {space:?} {scope:?}");
        }
        let pipeline = gpu.device.bounds_pipeline.get().unwrap();
        if let Some(original) = &original_pipeline {
            assert!(Arc::ptr_eq(original, pipeline), "working color changes reuse the same alpha-only pipeline");
        } else {
            original_pipeline = Some(pipeline.clone());
        }
    }
    assert_eq!(transfers.len(), 3);
}

#[test]
fn bounded_bounds_batch_falls_back_for_regions_above_the_batch_share() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().size = [512, 256];
    paint(&mut document).domain = [512, 256];
    paint(&mut document).base = Some(layer_core::authored::PaintBase::new((source([16, 16], |_, _| 255)).into()));
    occurrence(&mut document).translation = Point { x: 32., y: 48. };
    let extent = document.composition().size;
    let (second, SourceTarget::Paint(second_paint)) = crate::test_support::add_paint(&mut document.artwork, "second boundary page", extent) else { panic!("paint") };
    document.artwork.paint.get_mut(second_paint).unwrap().base = Some(layer_core::authored::PaintBase::new((source([16, 16], |_, _| 255)).into()));
    document.artwork.occurrences.get_mut(second).unwrap().translation = Point { x: 300., y: 24. };
    while document.artwork.occurrences.len() < 16 {
        crate::test_support::add_paint(&mut document.artwork, "additional compositing layer", extent);
    }
    let window = crate::pixel_rect::PixelRect::new(0, 0, 256, 256);
    let planned = crate::scene::Scene::capture_image_bound(captured(&document).view(), window)
        + window.area() * 32 + (document.artwork.occurrences.len() as u64 * 3 + 32) * 256 * 256 * 16 + 32;
    assert!(planned > 64 * 1024 * 1024 && planned < 512 * 1024 * 1024,
        "fixture requires the existing full capture allowance, planned={planned}");
    let mut capture = renderer.snapshot_gpu().capture_scene(captured(&document), SceneScope::All, Default::default()).unwrap();
    let pixels = capture.read_region([0, 0, 512, 256]).unwrap();
    let expected = pixels.iter().enumerate().filter(|(_, pixel)| pixel[3] > 0.)
        .fold(Rect::EMPTY, |bounds, (i, _)| {
            let x = (i % 512) as f32;
            let y = (i / 512) as f32;
            bounds.union(rect(x, y, x + 1., y + 1.))
        });
    assert_eq!(expected, rect(32., 24., 316., 64.), "independent exact composite");
    for scope in [ContentScope::Canvas, ContentScope::Visible, ContentScope::All] {
        assert_eq!(bounds(&renderer, &document, scope), expected,
            "{scope:?}: one oversized batch region must drain and retry within the full allowance");
    }
}

#[test]
fn paper_bounds_follow_alpha_filters_and_masked_pass_through_groups() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    for case in 0..3 {
        let grouped = case == 1;
        let mut document = Document::new(PortableId::random(), 128, 96, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        if case == 2 {
            paint(&mut document).base = Some(layer_core::authored::PaintBase::new((source([10, 10], |_, _| 255)).into()));
            occurrence(&mut document).translation = Point { x: -10., y: -10. };
        }
        let mut program = (*crate::tests::fixture("gaussian_blur").program()).clone();
        program.wgsl = if grouped {
            "fn paper(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(0.);}"
        } else if case == 2 {
            "fn paper(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return select(vec4<f32>(0.),c,p.x<0.||(p.x>=20.&&p.x<40.&&p.y>=30.&&p.y<50.));}"
        } else {
            "fn paper(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return select(vec4<f32>(0.),c,p.x>=20.&&p.x<40.&&p.y>=30.&&p.y<50.);}"
        }.into();
        program.entry = "paper".into();
        program.passes = Arc::new([]);
        program.lookups = Arc::new([]);
        program.space = layer_core::EffectSpace::Linear;
        program.alpha = layer_core::EffectAlpha::Filter;
        let effect = add_effect(&mut document, layer_core::EffectInstance::new(Arc::new(program)), "paper alpha filter");
        if grouped {
            let group = wrap(&mut document, effect, "masked pass-through group");
            document.artwork.occurrences.get_mut(group).unwrap().blend = layer_core::LayerBlend::PassThrough;
            let mask = attach_mask(&mut document, group, Point::default());
            let coverage = document.artwork.coverage.get_mut(mask).unwrap();
            coverage.default_coverage = 0.;
            coverage.initial = Some(Selection::polygon(rect(0., 0., 64., 96.).corners().to_vec()).unwrap());
        }
        let expected = if grouped { rect(64., 0., 128., 96.) } else { rect(20., 30., 40., 50.) };
        let mut capture = renderer.snapshot_gpu().capture_scene(captured(&document), SceneScope::All, Default::default()).unwrap();
        let pixels = capture.read_region([0, 0, 128, 96]).unwrap();
        for y in 0..96 {
            for x in 0..128 {
                let opaque = x as f32 >= expected.min.x && (x as f32) < expected.max.x
                    && y as f32 >= expected.min.y && (y as f32) < expected.max.y;
                assert_eq!(pixels[y * 128 + x][3], f32::from(opaque), "exact paper capture grouped={grouped} at {x},{y}");
            }
        }
        assert_eq!(bounds(&renderer, &document, ContentScope::Canvas), expected, "case={case} canvas");
        let visible = if case == 2 { rect(-10., -10., 40., 50.) } else { expected };
        assert_eq!(bounds(&renderer, &document, ContentScope::Visible), visible, "case={case} visible paper remains inside the original canvas");
        let all = if case == 2 { rect(-10., -10., 128., 96.) } else { rect(0., 0., 128., 96.) };
        assert_eq!(bounds(&renderer, &document, ContentScope::All), all, "case={case} all");
    }
}

#[test]
fn remote_transparent_or_fully_masked_sources_do_not_refuse_small_actual_bounds() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    paint(&mut document).raster = raster(RasterPlane::Color, document.composition().color, |x, y| if x == 3 && y == 7 { 1. } else { 0. });
    let extent = document.composition().size;
    let (remote_id, SourceTarget::Paint(remote_paint)) = crate::test_support::add_paint(&mut document.artwork, "remote source", extent) else { panic!("paint") };
    document.artwork.paint.get_mut(remote_paint).unwrap().base = Some(layer_core::authored::PaintBase::new((source([16, 16], |_, _| 0)).into()));
    document.artwork.occurrences.get_mut(remote_id).unwrap().translation = Point { x: 40000., y: -40000. };
    let root = document.composition().result;
    let entries = &mut document.artwork.stacks.get_mut(root).unwrap().entries; entries.pop(); entries.insert(0, remote_id);
    let expected = rect(3., 7., 4., 8.);
    assert_eq!(bounds(&renderer, &document, ContentScope::Visible), expected);
    assert_eq!(bounds(&renderer, &document, ContentScope::All), expected);
    assert_eq!(bounds(&renderer, &document, ContentScope::Canvas), expected);

    let mask = attach_mask(&mut document, remote_id, Point { x: 40000., y: -40000. });
    document.artwork.coverage.get_mut(mask).unwrap().default_coverage = 0.;
    document.artwork.paint.get_mut(remote_paint).unwrap().base = Some(layer_core::authored::PaintBase::new((source([16, 16], |_, _| 255)).into()));
    assert_eq!(bounds(&renderer, &document, ContentScope::Visible), expected);
    assert_eq!(bounds(&renderer, &document, ContentScope::Canvas), expected);
    assert_eq!(bounds(&renderer, &document, ContentScope::All), rect(3., -40000., 40016., 8.));
    document.artwork.coverage.get_mut(mask).unwrap().initial = Some(Selection::polygon(rect(80., 80., 96., 96.).corners().to_vec()).unwrap());
    assert_eq!(bounds(&renderer, &document, ContentScope::Visible), expected,
        "bounded mask preparation cannot reserve the empty space between remote objects");
}

#[test]
fn visible_alpha_filter_uses_the_canvas_domain_when_it_creates_alpha() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    paint(&mut document).base = Some(layer_core::authored::PaintBase::new((source([16, 16], |_, _| 255)).into()));
    occurrence(&mut document).translation = Point { x: 32., y: 48. };
    let mut program = (*crate::tests::fixture("gaussian_blur").program()).clone();
    program.wgsl = "fn opaque(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(.2,.3,.4,1.);}".into();
    program.entry = "opaque".into();
    program.passes = Arc::new([]);
    program.lookups = Arc::new([]);
    program.space = layer_core::EffectSpace::Linear;
    add_effect(&mut document, layer_core::EffectInstance::new(Arc::new(program)), "alpha creator");
    let mut capture = renderer.snapshot_gpu().capture_scene(captured(&document), SceneScope::All, Default::default()).unwrap();
    let pixels = capture.read_region([0, 0, 128, 128]).unwrap();
    assert!(pixels.iter().all(|p| p[3] == 1.), "independent exact capture establishes full-canvas alpha");
    assert_eq!(bounds(&renderer, &document, ContentScope::Canvas), rect(0., 0., 128., 128.));
    assert_eq!(bounds(&renderer, &document, ContentScope::Visible), rect(0., 0., 128., 128.));
    assert_eq!(bounds(&renderer, &document, ContentScope::All), rect(32., 48., 48., 64.));
}

#[test]
fn mask_target_selection_keeps_nested_nonuniform_registration_and_ignores_paint_visibility() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    occurrence(&mut document).visible = false;
    occurrence(&mut document).opacity = 0.;
    occurrence(&mut document).placement = layer_core::LayerPlacement::from_affine(layer_core::Affine([2., 0., 0., 0.5, 0., 0.]));
    occurrence(&mut document).translation = Point { x: 10., y: 12. };
    let h = owner(&document);
    let group = wrap(&mut document, h, "parent");
    document.artwork.occurrences.get_mut(group).unwrap().translation = Point { x: 30., y: 40. };
    let mask = attach_mask(&mut document, h, Point { x: 10., y: 12. });
    occurrence(&mut document).mask.as_mut().unwrap().enabled = false;
    let color = document.composition().color;
    let coverage = document.artwork.coverage.get_mut(mask).unwrap();
    coverage.default_coverage = 0.;
    coverage.raster = raster(RasterPlane::Mask, color, |x, y| if y == 20 && (x == 20 || x == 40) { 1. } else { 0. });
    let mask_id = SourceTarget::Coverage(mask);
    reindex(&mut document);
    for linked in [true, false] {
        occurrence(&mut document).mask.as_mut().unwrap().linked = linked;
        let map = document.target_geometry(mask_id);
        document.working.selection = Some(Selection::polygon(rect(19., 19., 22., 22.).corners().map(|p| map.map(p).unwrap()).to_vec()).unwrap());
        assert_eq!(bounds(&renderer, &document, ContentScope::Target(mask_id)), rect(20., 20., 21., 21.), "linked={linked}");
        document.working.selection.as_mut().unwrap().inverted = true;
        assert_eq!(bounds(&renderer, &document, ContentScope::Target(mask_id)), rect(40., 20., 41., 21.), "inverted linked={linked}");
    }
}

#[test]
fn visible_and_all_bounds_include_placement_interpolation_beyond_source_rectangle() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    paint(&mut document).base = Some(layer_core::authored::PaintBase::new((source([16, 16], |_, _| 255)).into()));
    occurrence(&mut document).translation = Point { x: 20., y: 30. };
    occurrence(&mut document).placement = layer_core::LayerPlacement::from_affine(layer_core::Affine([2., 0., 0., 2., 0., 0.]));
    let canvas = bounds(&renderer, &document, ContentScope::Canvas);
    assert!(canvas.min.x < 20. && canvas.min.y < 30. && canvas.max.x > 52. && canvas.max.y > 62.,
        "independent canvas query captures interpolation fringe: {canvas:?}");
    assert_eq!(bounds(&renderer, &document, ContentScope::Visible), canvas);
    assert_eq!(bounds(&renderer, &document, ContentScope::All), canvas);
}

#[test]
fn transferred_bounds_request_keeps_accumulated_alpha_animation_phase_after_pause() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    paint(&mut document).base = Some(layer_core::authored::PaintBase::new((source([128, 128], |_, _| 255)).into()));
    let mut program = (*crate::tests::fixture("domain_warp").program()).clone();
    program.wgsl = "fn phase(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{let low=floor(fx_time(b))*4.;return select(vec4<f32>(0.),vec4<f32>(1.),p.x>=low&&p.x<low+8.&&p.y>=12.&&p.y<20.);}".into();
    program.entry = "phase".into();
    program.passes = Arc::new([]);
    program.lookups = Arc::new([]);
    program.space = layer_core::EffectSpace::Linear;
    program.alpha = layer_core::EffectAlpha::Filter;
    let mut effect = layer_core::EffectInstance::new(Arc::new(program));
    effect.set("animate", layer_core::EffectValue::Toggle(true)).unwrap();
    let mut clock = layer_core::EffectClock::default();
    let mut phase = 0.;
    for (elapsed, speed) in [(2., 1.), (2., 2.), (3., 2.), (3., 0.), (8., 0.)] {
        effect.set("speed", layer_core::EffectValue::Number(speed)).unwrap();
        phase = clock.advance((&effect).into(), elapsed);
    }
    assert_eq!(phase, 4.);
    assert_eq!(effect.time_seconds(8.), 0., "elapsed and paused speed alone lose accumulated phase");
    let id = add_effect(&mut document, effect, "paused alpha animation");
    let OccurrenceContent::Effect(effect) = document.artwork.occurrences.get(id).unwrap().content else { panic!("effect") };
    let mut snapshot = (*captured(&document)).clone();
    snapshot.context = EvaluationContext { elapsed: 8., phases: vec![(effect, phase)].into() };
    let request = ContentBoundsRequest { snapshot: Arc::new(snapshot), scope: ContentScope::Visible, selection: document.working.selection.clone() };
    let transferred = request.clone();
    let fresh_worker = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    assert_eq!(pollster::block_on(fresh_worker.snapshot_gpu().content_bounds(transferred, Default::default())).unwrap(),
        rect(16., 12., 24., 20.), "worker GPU starts without the canvas clock history");
    assert_eq!(pollster::block_on(renderer.snapshot_gpu().content_bounds(request, Default::default())).unwrap(),
        rect(16., 12., 24., 20.));
}


fn target_bytes(renderer: &WgpuRasterizer, target: SourceTarget) -> Vec<u8> {
    if let Some(page) = renderer.layer_masks.pages.get(&(target, [0, 0])) {
        return crate::layer_tests::page_bytes(renderer, &page.texture);
    }
    let page = renderer.paint_layers.iter().find(|l| l.id == target).unwrap().pages.iter()
        .find(|p| p.coordinate == [0, 0]).unwrap();
    crate::layer_tests::page_bytes(renderer, &page.active().texture)
}

#[test]
fn identity_warp_preserves_linked_companion_pixels_outside_primary_tight_bounds() {
    for primary_mask in [false, true] {
        let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut document = document(Default::default());
        paint(&mut document).raster = raster(RasterPlane::Color, document.composition().color, |x, y|
            if (20..40).contains(&x) && (20..40).contains(&y) { 1. } else { 0. });
        let paint = paint_target(&document);
        let h = owner(&document);
        let mask = attach_mask(&mut document, h, Point::default());
        document.artwork.coverage.get_mut(mask).unwrap().default_coverage = 0.;
        document.artwork.coverage.get_mut(mask).unwrap().raster = raster(RasterPlane::Mask, document.composition().color, |x, y|
            if (80..100).contains(&x) && (80..100).contains(&y) { 1. } else { 0. });
        let mask_id = SourceTarget::Coverage(mask);
        let primary = if primary_mask { mask_id } else { paint };
        let companion = if primary_mask { paint } else { mask_id };
        document.working.target = Some(primary);
        document.working.inspect_mask = primary_mask.then_some(h);
        reindex(&mut document);
        let primary_bounds = bounds(&renderer, &document, ContentScope::Target(primary));
        assert_eq!(primary_bounds, if primary_mask { rect(80., 80., 100., 100.) } else { rect(20., 20., 40., 40.) });
        let companion_bounds = bounds(&renderer, &document, ContentScope::Target(companion));
        assert_eq!(companion_bounds, if primary_mask { rect(20., 20., 40., 40.) } else { rect(80., 80., 100., 100.) });
        let to_primary = document.target_geometry(companion).as_affine().unwrap()
            .then(document.target_geometry(primary).as_affine().unwrap().inverse().unwrap());
        let prepared = primary_bounds.union(to_primary.bounds(companion_bounds));
        assert_eq!(prepared, rect(20., 20., 100., 100.));
        let frame = |renderer: &mut WgpuRasterizer, reset| {
            renderer.submit(layer_render::FramePacket {
                reset_layers: reset,
                composite_all: true,
                ..crate::test_support::packet(document.scene(), [128; 2])
            }).unwrap();
            renderer.readback_srgb_rgba8().unwrap();
        };
        frame(&mut renderer, true);
        let original = target_bytes(&renderer, companion);
        let original_primary = target_bytes(&renderer, primary);
        let preview = layer_render::TransformPreview {
            transaction: 1, moving: false, target: primary, selection: None,
            transform: layer_core::ImageTransform { placement: layer_core::LayerPlacement { interpolation: layer_core::Interpolation::Nearest, ..layer_core::LayerPlacement { mesh: Some(Arc::new(layer_core::MeshMap::identity(prepared, [3, 3]).unwrap())), ..Default::default() } },
                ..Default::default()
            },
        };
        let linked = preview.companion(document.scene()).unwrap();
        assert_eq!(linked.target, companion);
        renderer.set_transform_preview(Some(&preview)).unwrap();
        frame(&mut renderer, false);
        let warped = target_bytes(&renderer, companion);
        let warped_primary = target_bytes(&renderer, primary);
        renderer.set_transform_preview(None).unwrap();
        frame(&mut renderer, false);
        assert!(target_bytes(&renderer, companion) == original, "Cancel restores exact companion pixels");
        assert!(target_bytes(&renderer, primary) == original_primary, "Cancel restores exact primary pixels");
        let at = if primary_mask { 30 } else { 90 };
        let stride = if primary_mask { 16 } else { 4 };
        let offset = (at * 256 + at) * stride;
        let alpha = if primary_mask { 12 } else { 0 };
        let scalar = |bytes: &[u8]| f32::from_le_bytes(bytes[offset + alpha..][..4].try_into().unwrap());
        assert!(scalar(&original) > 0.99);
        println!("primary_mask={primary_mask}: identity Warp companion coverage at {at},{at}: {} -> {}",
            scalar(&original), scalar(&warped));
        assert!(warped == original, "identity Warp must preserve linked companion data outside primary tight bounds");
        assert!(warped_primary == original_primary, "identity Warp must preserve the primary pixels");
    }
}

#[test]
fn placed_target_bounds_match_masked_world_pixels_and_exclude_siblings() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    let target = paint_target(&document);
    paint(&mut document).base = Some(layer_core::authored::PaintBase::new((source([48, 32], |_, _| 255)).into()));
    occurrence(&mut document).placement = layer_core::LayerPlacement::from_projective(
        layer_core::Projective::rect_to_quad(Rect::from_extent([48, 32]), [
            Point { x: 20., y: 15. }, Point { x: 65., y: 19. },
            Point { x: 72., y: 56. }, Point { x: 15., y: 52. },
        ]).unwrap());
    let h = owner(&document);
    let group = wrap(&mut document, h, "masked parent");
    let o = document.artwork.occurrences.get_mut(group).unwrap(); o.translation = Point { x: 7., y: 9. }; o.blend = layer_core::LayerBlend::PassThrough;
    let mask = attach_mask(&mut document, group, Point { x: 7., y: 9. });
    document.artwork.coverage.get_mut(mask).unwrap().default_coverage = 0.;
    document.artwork.coverage.get_mut(mask).unwrap().raster = raster(RasterPlane::Mask, document.composition().color,
        |x, y| if (10..50).contains(&x) && (10..55).contains(&y) { 0.5 } else { 0. });
    let extent = document.composition().size;
    let (sibling, SourceTarget::Paint(sibling_paint)) = crate::test_support::add_paint(&mut document.artwork, "unrelated outside target", extent) else { panic!("paint") };
    document.artwork.paint.get_mut(sibling_paint).unwrap().base = Some(layer_core::authored::PaintBase::new((source([8, 8], |_, _| 255)).into()));
    document.artwork.occurrences.get_mut(sibling).unwrap().translation = Point { x: 106., y: 106. };
    let mut reference = document.clone();
    let root = reference.composition().result;
    reference.artwork.stacks.get_mut(root).unwrap().entries.retain(|h| *h != sibling);
    let id = reference.artwork.occurrences.id(sibling).unwrap(); reference.artwork.occurrences.change(sibling, id, None).unwrap();
    let mut capture = renderer.snapshot_gpu().capture_scene(captured(&reference), SceneScope::All, Default::default()).unwrap();
    let expected = capture.read_region([0, 0, 128, 128]).unwrap().iter().enumerate()
        .filter(|(_, pixel)| pixel[3] > 0.).fold(Rect::EMPTY, |bounds, (i, _)| {
            let x = (i % 128) as f32; let y = (i / 128) as f32;
            bounds.union(rect(x, y, x + 1., y + 1.))
        });
    assert!(!expected.is_empty());
    assert!(expected.max.x < 80. && expected.max.y < 80., "masked, placed target: {expected:?}");
    assert_eq!(bounds(&renderer, &document, ContentScope::PlacedTarget(owner(&document))), expected);
    assert_eq!(bounds(&renderer, &document, ContentScope::Target(target)), Rect::from_extent([48, 32]),
        "raw target bounds remain local and unmasked");
    assert!(bounds(&renderer, &document, ContentScope::Visible).max.x > expected.max.x,
        "the independent visible scene includes the unrelated sibling");
}
