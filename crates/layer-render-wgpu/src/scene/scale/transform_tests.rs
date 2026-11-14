use super::*;
use layer_core::{Affine, ImageTransform, Interpolation, MeshMap, Point, Projective, Rect, Selection, LayerPlacement};

#[test]
fn navigator_visibility_preserves_keyed_transform_presentation() {
    let doc = document();
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), extent);
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    submit_published(&mut r, frame);
    frame.composite_all = false;
    let mut presenter = Some(crate::ViewportPresenter::for_renderer(&r, wgpu::TextureFormat::Rgba8Unorm));
    let mut standalone = crate::ViewportPresenter::for_overview_surface(&r, wgpu::TextureFormat::Rgba8Unorm,
        crate::SdrSurfaceColor::Srgb).unwrap();
    let overview = crate::OverviewPlacement {
        bounds: [0., 0., 100., 50.], clip: None, work_area: [[0.; 2]; 4],
        outline_linear: [0.; 3], scale: 1., opacity: 1.,
    };
    standalone.set_overviews(&r, std::slice::from_ref(&overview));
    for (step, (visible, x)) in [(false, 10.), (true, 10.), (true, 20.), (false, 30.), (true, 40.), (false, 50.)].into_iter().enumerate() {
        if step == 5 { drop(presenter.take()); }
        else if let Some(presenter) = &mut presenter {
            presenter.set_overviews(&r, if visible { std::slice::from_ref(&overview) } else { &[] });
        }
        r.set_transform_preview(Some(&layer_render::TransformPreview {
            transaction: 1, target: source_at(&doc,0), moving: true, selection: None,
            transform: ImageTransform::affine(Affine::translation(Point { x, y: 0. })),
        })).unwrap();
        submit_published(&mut r, frame);
        assert_keyed_presentation(&r);
        assert!(display_pixels(&r).iter().flatten().any(|value| *value != 0.));
    }
}

#[test]
fn keyed_transforms_present_rotated_views_and_navigators() {
    let extent = [517, 259];
    let bounds = Rect { min: Point::default(), max: Point { x: 517., y: 259. } };
    let projective = Projective::rect_to_quad(bounds,
        [[28., 4.], [489., 41.], [512., 235.], [8., 251.]].map(|[x,y]| Point { x,y })).unwrap();
    let selection = Selection::polygon(vec![bounds.min, Point { x: 517., y: 0. }, bounds.max, Point { x: 0., y: 259. }]).unwrap();
    let render = |r: &WgpuRasterizer, view: layer_render::ViewState, overview: bool| {
        let size = [view.width_px, view.height_px];
        let (texture, target) = create_color_target(&r.device, size, "mapped presentation oracle");
        let mut presenter = if overview { crate::ViewportPresenter::for_overview_surface }
            else { crate::ViewportPresenter::for_surface }(
                r, wgpu::TextureFormat::Rgba32Float, crate::SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
        if overview {
            presenter.set_overviews(r, &[crate::OverviewPlacement {
                bounds: [0., 0., size[0] as f32, size[1] as f32], clip: None,
                work_area: [[-1000.; 2]; 4], outline_linear: [0.; 3], scale: 1., opacity: 1.,
            }]);
            presenter.present_overviews(r, &target, size).unwrap();
        } else { presenter.present(r, &target, view, [1.; 4]).unwrap(); }
        pixels(r, &texture)
    };
    for omitted in [wgpu::Features::empty(), wgpu::Features::FLOAT32_FILTERABLE] {
    for patterned in [false, true] {
    for space in layer_core::BlendSpace::ALL {
    for (selected, opacity) in [(false, 1.), (true, 0.71)] {
        let mut doc = document();
        add_fill(&mut doc, layer_core::color::RgbColor::WHITE);
        if patterned { paint_mut(&mut doc,0).base = Some(layer_core::authored::PaintBase::new((layer_core::color::source::rgba8_source(extent, |x, y|
            [if (x / 5 + y / 7) % 2 == 0 { 40 } else { 220 },
             if (x / 13 + y / 17) % 2 == 0 { 40 } else { 220 },
             if (x / 2 + y / 3) % 2 == 0 { 40 } else { 220 }, 255])).into())); }
        occurrence_mut(&mut doc,0).opacity = opacity;
        let mut r = WgpuRasterizer::native_headless_without(doc.composition().color, omitted).unwrap();
        assert_eq!(r.device.float32_filterable(),
            r.adapter.features().contains(wgpu::Features::FLOAT32_FILTERABLE)
                && !omitted.contains(wgpu::Features::FLOAT32_FILTERABLE));
        let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        exact.test.reference = true;
        let mut frame = packet(doc.scene(), extent);
        frame.blend_space = space;
        frame.view.width_px = 160; frame.view.height_px = 100;
        frame.view.document_to_surface = [0.19, 0., 0., 0.19, 8.25, 7.5];
        submit_published(&mut r, frame); submit_published(&mut exact, frame);
        frame.composite_all = false;
        for map in [LayerPlacement::from_affine(Affine([0.8, 0.12, -0.1, 0.9, 40., 0.])), LayerPlacement::from_projective(projective)] {
            let preview = layer_render::TransformPreview { transaction: 1, target: source_at(&doc,0), moving: true,
                selection: selected.then(|| selection.clone()), transform: ImageTransform { placement: map, ..Default::default() } };
            frame.time_seconds += 0.1;
            let work = r.metrics.composited_pixels;
            for renderer in [&mut r, &mut exact] {
                renderer.set_transform_preview(Some(&preview)).unwrap(); submit_published(renderer, frame);
            }
            let cache = r.scale_display.as_ref().unwrap();
            assert_keyed_presentation(&r);
            let side = r.navigator.scale() as u32;
            let navigator=extent.map(|n|u64::from(n.div_ceil(side))).into_iter().product::<u64>();
            let viewport=cache.plan.size.into_iter().map(u64::from).product::<u64>();
            assert!(r.metrics.composited_pixels-work<=viewport+navigator,"each keyed viewport and Navigator texel composes at most once");
            for (camera, overview) in [([0.19, 0., 0., 0.19, 8.25, 7.5], false),
                ([0.17, 0.075, -0.075, 0.17, 37.5, 6.25], false), ([0.04, 0., 0., 0.04, 0., 0.], true)] {
                let mut view = frame.view;
                view.document_to_surface = camera;
                if overview { view.width_px = 23; view.height_px = 13; }
                let actual = render(&r, view, overview);
                let inverse = Affine(camera).inverse().unwrap();
                let inside = |x, y| {
                    let p = inverse.map(Point { x, y });
                    overview || (p.x >= 0. && p.y >= 0. && p.x < extent[0] as f32 && p.y < extent[1] as f32)
                };
                let mut high = view;
                high.width_px *= 8; high.height_px *= 8;
                high.document_to_surface = high.document_to_surface.map(|n| n * 8.);
                let reference = render(&exact, high, overview);
                let mut errors = Vec::new();
                for y in 0..view.height_px { for x in 0..view.width_px {
                    if !inside(x as f32 + 0.5, y as f32 + 0.5) { continue; }
                    let mut expected = [0.; 4];
                    let mut count = 0.;
                    for yy in y * 8..(y + 1) * 8 { for xx in x * 8..(x + 1) * 8 {
                        if !inside((xx as f32 + 0.5) / 8., (yy as f32 + 0.5) / 8.) { continue; }
                        for c in 0..4 { expected[c] += reference[(yy * high.width_px + xx) as usize][c]; }
                        count += 1.;
                    }}
                    errors.push(actual[(y * view.width_px + x) as usize].iter().zip(expected).map(|(a,b)| (a-b/count).abs()).fold(0., f32::max));
                }}
                errors.sort_by(f32::total_cmp);
                let mean = errors.iter().sum::<f32>() / errors.len() as f32;
                let p99 = errors[errors.len() * 99 / 100];
                assert!(if overview { mean < 0.06 && p99 < 0.25 } else { mean < 0.004 && p99 < 0.04 },
                    "omitted={omitted:?} patterned={patterned} {space:?} selected={selected} overview={overview} camera={camera:?}: mean={mean} p99={p99}");
                if !overview && !patterned && space == layer_core::BlendSpace::Linear {
                    assert!(mean < 0.004 && p99 < 0.04, "omitted={omitted:?} linear area error: mean={mean} p99={p99}");
                }
            }
        }
    }}}}
}

#[test]
fn pass_through_children_above_a_transform_keep_shared_display_composition() {
    let mut doc = document();
    let extent = doc.composition().size;
    let moving = source_at(&doc,0);
    let child=copy_paint(&mut doc,0);doc.artwork.occurrences.get_mut(child).unwrap().opacity=0.35;
    let group=stack_occurrence(&mut doc,"pass through",vec![child]);doc.artwork.occurrences.get_mut(group).unwrap().blend=layer_core::LayerBlend::PassThrough;insert_occurrence(&mut doc,group,0);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    for blend in [layer_core::LayerBlend::Normal, layer_core::LayerBlend::Multiply] {
        occurrence_mut(&mut doc,1).blend = blend;
        let mut frame = packet(doc.scene(), extent);
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        submit_published(&mut r, frame); submit_published(&mut exact, frame);
        frame.composite_all = false;
        let original = display_pixels(&r);
        for step in 1..4 {
            let transform = layer_render::TransformPreview { transaction: 1, target: moving, moving: step < 3, selection: None,
                transform: ImageTransform::affine(Affine::translation(Point { x: 8. * step as f32, y: -4. })) };
            for renderer in [&mut r, &mut exact] {
                renderer.set_transform_preview(Some(&transform)).unwrap(); crate::test_support::submit_ready(renderer, frame).unwrap();
            }

            if transform.moving { assert!(!r.scale_display.as_ref().unwrap().has_pending_work(&r)); }
            let cache = r.scale_display.as_ref().expect("group children stay on the display graph");
            let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan, cache.texture().format());
            assert!(error[0] < 0.004 && error[1] < 0.06, "{blend:?} step={step} error={error:?}");
        }
        for renderer in [&mut r, &mut exact] {
            renderer.set_transform_preview(None).unwrap(); crate::test_support::submit_ready(renderer, frame).unwrap();
        }
        assert_eq!(original, display_pixels(&r));
    }
}

#[test]
fn transform_sources_compose_with_the_stack_without_native_preview_during_motion() {
    for space in layer_core::BlendSpace::ALL {
    let extent = [517, 259];
    let bounds = Rect { min: Point::default(), max: Point { x: 517., y: 259. } };
    let selected = Selection::polygon(vec![Point { x: 50.5, y: 20.25 }, Point { x: 490., y: 50.5 },
        Point { x: 420.5, y: 240. }, Point { x: 90., y: 190.5 }]).unwrap();
    let all = Selection::polygon(vec![bounds.min, Point { x: 517., y: 0. }, bounds.max, Point { x: 0., y: 259. }]).unwrap();
    let projective = Projective::rect_to_quad(bounds,
        [[10., 5.], [510., 20.], [480., 254.], [25., 235.]].map(|[x,y]| Point { x,y })).unwrap();
    let mesh = MeshMap::fit(bounds, [3,3], |p| projective.map(p)).unwrap()
        .move_node(5, Point { x: 24., y: -12. }).unwrap();
    let folded = mesh.move_node(5, Point { x: 360., y: 160. }).unwrap();
    let maps = [LayerPlacement::from_affine(Affine::translation(Point { x: 12., y: -8. })),
        LayerPlacement::from_affine(Affine::around(Point { x: 250., y: 125. }, [0.8, 1.1], 0.2, Point { x: 8., y: 2. })),
        LayerPlacement::from_projective(projective), layer_core::LayerPlacement { mesh: Some(Arc::new(mesh)), ..Default::default() }, layer_core::LayerPlacement { mesh: Some(Arc::new(folded)), ..Default::default() }];
    for (stacked, blend) in [
        (false, layer_core::LayerBlend::Normal),
        (true, layer_core::LayerBlend::Normal),
        (true, layer_core::LayerBlend::Difference),
        (true, layer_core::LayerBlend::Luminosity),
    ] {
    for offset in [[0, 0], [200, 10]] {
        for selection in [None, Some(all.clone()), Some(selected.clone())] {
            let mut doc = document();
            let id = source_at(&doc,0);
            occurrence_mut(&mut doc,0).opacity = 0.8;
            occurrence_mut(&mut doc,0).blend = blend;
            occurrence_mut(&mut doc,0).offset = offset;
            if stacked {
                let below=copy_paint(&mut doc,0);let occurrence=doc.artwork.occurrences.get_mut(below).unwrap();occurrence.opacity=1.;occurrence.offset=[0,0];
                let above=copy_paint(&mut doc,0);let mut occurrence=doc.artwork.occurrences.get(below).unwrap().clone();let content=doc.artwork.occurrences.get(above).unwrap().content.clone();occurrence.content=content;occurrence.opacity=0.23;occurrence.blend=layer_core::LayerBlend::Multiply;*doc.artwork.occurrences.get_mut(above).unwrap()=occurrence;
                insert_occurrence(&mut doc,above,0);insert_occurrence(&mut doc,below,2);
            }
            let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
            let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
            exact.test.reference = true;
            let mut frame = packet(doc.scene(), extent);
        frame.blend_space = space;
            frame.composite_all = false;
            frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
            submit_published(&mut r, frame); submit_published(&mut exact, frame);
            let original = display_pixels(&r);
            for (step, map) in maps.iter().enumerate() {
                let preview = layer_render::TransformPreview { transaction: 1, target: id, moving: true,
                    selection: selection.clone(), transform: ImageTransform { placement: layer_core::LayerPlacement { interpolation: Interpolation::Bicubic, ..map.clone() }, ..Default::default() } };
                for renderer in [&mut r, &mut exact] {
                    renderer.set_transform_preview(Some(&preview)).unwrap(); crate::test_support::submit_ready(renderer, frame).unwrap();
                }
                assert!(r.scale_display.is_some());
                assert!(r.paint_layers.iter().all(|l| l.pages.is_empty()), "display motion does not allocate native preview pages");
                assert!(!r.scale_display.as_ref().unwrap().has_pending_work(&r), "moving transforms defer exact refinement");
                assert_eq!(r.test.source_captures.get(), 1);
                let displayed = display_pixels(&r);
                let exact_pixels = pixels(&exact, crate::test_support::document_texture(&exact));
                let error = quality_linear(&displayed, &exact_pixels, r.scale_display.as_ref().unwrap().plan, r.scale_display.as_ref().unwrap().texture().format(),
                    |color| linear_color(color, space, doc.composition().color.space));
                eprintln!("{space:?} offset={offset:?} selection={} step={step} error={error:?}", selection.is_some());
                assert!(error[0] < 0.004 && error[1] < 0.06, "transform reduction quality {error:?}");
                let a = r.readback_srgb_rgba8().unwrap();
                let b = exact.readback_srgb_rgba8().unwrap();
                assert_eq!(a, b, "exact queries evaluate native transform pixels");
                assert_eq!(displayed, display_pixels(&r), "an exact query preserves the displayed approximation");
                crate::test_support::submit_ready(&mut r, frame).unwrap();
                assert_eq!(displayed, display_pixels(&r));
                assert!(r.paint_layers.iter().map(|l| l.pages.len()).sum::<usize>() <= 9,
                    "an exact query retains only its final native dependency window");
            }
            for renderer in [&mut r, &mut exact] {
                renderer.set_transform_preview(None).unwrap(); crate::test_support::submit_ready(renderer, frame).unwrap();
            }
            assert_eq!(original, display_pixels(&r), "cancelling restores the original composition");
        }
    }
}
    }
}

#[test]
fn transform_queries_keep_only_the_native_tiles_the_requested_window_reads() {
    let doc = document_at([2053, 1541]);
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.source_tiles.get_mut().admit_limits(16 << 20);
    let mut frame = packet(doc.scene(), extent);
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    frame.composite_all = false;
    crate::test_support::submit_ready(&mut r, frame).unwrap();
    r.set_transform_preview(Some(&layer_render::TransformPreview {
        transaction: 1, target: source_at(&doc,0), moving: false, selection: None,
        transform: ImageTransform::affine(Affine::translation(Point { x: 13.5, y: -5.25 })),
    })).unwrap();
    crate::test_support::submit_ready(&mut r, frame).unwrap();
    let mut capture = artwork::Capture::default();
    while r.next_frame()==layer_render::FrameDemand::Work { r.wait_idle().unwrap(); crate::test_support::submit_ready(&mut r, frame).unwrap(); }
    r.wait_idle().unwrap();
    for origin in [[256,256], [1024,768], [1536,1024], [0,0]].into_iter().cycle().take(16) {
        let region = PixelRect::new(origin[0], origin[1], origin[0] + 32, origin[1] + 32);
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let result = crate::test_support::query_ready(&mut r, &mut encoder, |r, encoder| capture.region(r, frame, region, [32,32], encoder));
        r.uploads.finish(&encoder); encoder.submit(&r.queue);
        assert!(r.paint_layers.iter().map(|l| l.pages.len()).sum::<usize>() <= 9,
            "one native tile with interpolation support reaches at most three tiles per axis");
        assert!(pixels(&r, &result.texture).iter().all(|p| p.iter().all(|v| v.is_finite())));
        if let Some(report) = r.device.generate_allocator_report() {
            let tiles = report.allocations.iter().filter(|a| a.name == "bounded source tile").count();
            assert!(tiles <= 64, "decoded source cache retained {tiles} tiles");
        }
    }
    r.set_transform_preview(None).unwrap(); crate::test_support::submit_ready(&mut r, frame).unwrap();
    assert!(r.paint_layers.iter().all(|l| l.pages.is_empty()));
}

#[test]
fn transform_zoom_release_and_commit_preserve_native_pixels() {
    let mut doc = document();
    let extent = doc.composition().size;
    let selection = Selection::polygon([[150.,50.],[350.,50.],[350.,200.],[150.,200.]]
        .map(|[x,y]| Point { x,y }).to_vec()).unwrap();
    let mut preview = layer_render::TransformPreview { transaction: 1, target: source_at(&doc,0),
        moving: true, selection: Some(selection.clone()),
        transform: ImageTransform { placement: layer_core::LayerPlacement { interpolation: Interpolation::Bicubic, ..LayerPlacement::from_affine(
            Affine::around(Point { x: 250., y: 125. }, [3.2,2.1], 0.31, Point { x: 7., y: -3. })) }, ..Default::default() } };
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut frame = packet(doc.scene(), extent);
    frame.composite_all = false;
    crate::test_support::submit_published(&mut r, frame); crate::test_support::submit_published(&mut exact, frame);
    for (scale, moving) in [(0.125,true),(0.5,true),(1.,true),(0.25,true),(0.25,false)] {
        preview.moving = moving;
        frame.view.document_to_surface = [scale,0.,0.,scale,0.,0.];
        for renderer in [&mut r, &mut exact] {
            renderer.set_transform_preview(Some(&preview)).unwrap(); crate::test_support::submit_published(renderer, frame);
        }
        assert!(r.scale_display.is_some());
        if moving { assert!(!r.scale_display.as_ref().unwrap().has_pending_work(&r)); }
        let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), r.scale_display.as_ref().unwrap().plan, r.scale_display.as_ref().unwrap().texture().format());
        assert!(error[0] < 0.004 && error[1] < 0.06, "scale={scale} moving={moving} {error:?}");
        crate::test_support::assert_same_canonical(&mut r, &mut exact);
        assert_eq!(r.test.source_captures.get(), 1);
        if !moving { assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact))); }
    }
    let expected = exact.readback_srgb_rgba8().unwrap();
    let mut coverage=CoverageSnapshot::reveal_all(doc.artwork.coverage.next_handle(),extent, [0, 0]);
    coverage.source.default_coverage=0.;coverage.selection=Some(selection);
    let operation = layer_core::RasterOperation { placement: Affine::IDENTITY, coverage,
        kind: layer_core::RasterOperationKind::Transform(preview.transform) };
    let batch = layer_render::DabBatch { kind: layer_render::DabBatchKind::RasterOperation(0), dab_count: 0,
        ..dab_batch(source_at(&doc,0), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), operation.bounds(extent)) };
    Arc::make_mut(&mut paint_mut(&mut doc,0).operations).push(operation);
    let mut frame = packet(doc.scene(), extent);
    frame.composite_all = false; frame.view.document_to_surface = [0.25,0.,0.,0.25,0.,0.];
    frame.dab_batches = std::slice::from_ref(&batch);
    r.set_transform_preview(None).unwrap(); crate::test_support::submit_published(&mut r, frame);
    assert_eq!(r.readback_srgb_rgba8().unwrap(), expected);
    assert!(!r.transforms.as_ref().unwrap().has_preview());
}

#[test]
fn transformed_group_children_keep_clipping_and_linked_mask_semantics() {
    let extent = [257,129];
    for (mask_target, linked) in [(false,false),(false,true),(true,true)] {
        let mut doc = document_at(extent);
        let owner=doc.scene().order()[0];let paint=source_at(&doc,0);let base=copy_paint(&mut doc,0);doc.artwork.occurrences.get_mut(base).unwrap().opacity=0.73;
        occurrence_mut(&mut doc,0).blend=layer_core::LayerBlend::Multiply;
        let initial=Selection::polygon([[20.,10.],[230.,20.],[190.,120.],[30.,90.]].map(|[x,y]|Point{x,y}).to_vec()).unwrap();
        let mask=coverage_mask(&mut doc,owner,[13, -4],Some(initial));doc.artwork.coverage.get_mut(mask).unwrap().default_coverage=0.23;doc.artwork.occurrences.get_mut(owner).unwrap().mask.as_mut().unwrap().linked=linked;
        let id=if mask_target{SourceTarget::Coverage(mask)}else{paint};
        set_attachment(occurrence_mut(&mut doc,0), true);
        let group=stack_occurrence(&mut doc,"group",vec![owner,base]);doc.artwork.occurrences.get_mut(group).unwrap().opacity=0.71;set_root_entries(&mut doc,vec![group]);
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        exact.test.reference = true;
        let mut frame = packet(doc.scene(), extent);
        frame.composite_all = false; frame.view.document_to_surface = [0.25,0.,0.,0.25,0.,0.];
        submit_published(&mut r, frame); submit_published(&mut exact, frame);
        let original = display_pixels(&r);
        for offset in [9.,-13.] {
            let preview = layer_render::TransformPreview { transaction: 1, target: id, moving: true, selection: None,
                transform: ImageTransform::affine(Affine::translation(Point { x: offset, y: 5. })) };
            for renderer in [&mut r, &mut exact] {
                renderer.set_transform_preview(Some(&preview)).unwrap(); crate::test_support::submit_ready(renderer, frame).unwrap();
            }
            assert!(r.scale_display.is_some());
            let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), r.scale_display.as_ref().unwrap().plan, r.scale_display.as_ref().unwrap().texture().format());
            assert!(error[0] < 0.004 && error[1] < 0.06, "mask={mask_target} linked={linked} {error:?}");
            crate::test_support::assert_same_canonical(&mut r, &mut exact);
        }
        r.set_transform_preview(None).unwrap(); crate::test_support::submit_ready(&mut r, frame).unwrap();
        assert_eq!(original, display_pixels(&r));
    }
}

#[test]
fn retained_transform_detail_still_filters_the_current_output_footprint() {
    let extent = [65,33];
    let mut doc = Document::new(PortableId::random(), extent[0], extent[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    paint_mut(&mut doc,0).base = Some(layer_core::authored::PaintBase::new((rgba8_source(extent, |x, _| [if (x/3)%2 == 0 {255} else {0}, 0, 0, 255])).into()));
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), extent);
    frame.composite_all = false; frame.view.document_to_surface = [0.25,0.,0.,0.25,0.,0.];
    submit_published(&mut r, frame);
    let original = display_pixels(&r);
    let mut work = 0;
    for (step, scale) in [4.,1.,4.,1.].into_iter().enumerate() {
        r.set_transform_preview(Some(&layer_render::TransformPreview { transaction: 1, target: source_at(&doc,0),
            moving: true, selection: None, transform: ImageTransform::affine(
                Affine::around(Point { x: 32., y: 16. }, [scale;2], 0., Point::default())) })).unwrap();
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        if step == 0 { work = r.test.reduced_pages.get(); }
        else { assert_eq!(r.test.reduced_pages.get(), work, "the immutable source is not recaptured on scale oscillations"); }
        if scale == 1. {
            let actual = display_pixels(&r);
            let error = crate::test_support::max_error(&actual, &original);
            let mismatch: Vec<_> = actual.iter().zip(&original).enumerate().filter(|(_, (a,b))| a.iter().zip(b.iter()).any(|(a,b)| (a-b).abs()>1e-5)).take(8).collect();
            assert!(error < 1e-5, "restored identity must reconstruct the original averaged pixels: {error}: {mismatch:?}");
        }
    }
}

#[test]
fn whole_image_selections_reserve_one_transform_input_pyramid() {
    let extent = [4248,2832];
    let doc = Document::new(PortableId::random(), extent[0], extent[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), extent);
    frame.composite_all = false; frame.view.document_to_surface = [0.1597,0.,0.,0.1597,0.,0.];
    let plan = display_mips::Plan::at(extent, 2);
    let mut preview = layer_render::TransformPreview { transaction: 1, target: source_at(&doc,0),
        moving: true, selection: None, transform: ImageTransform::affine(Affine([4.,0.,0.,4.,0.,0.])) };
    r.set_transform_preview(Some(&preview)).unwrap();
    r.begin_frame_memory(frame);
    assert_eq!(request(&r, frame).ok().map(|q| q.plan.level), Some(2));
    preview.selection = Some(Selection::polygon([[0.,0.],[4248.,0.],[4248.,2832.],[0.,2832.]]
        .map(|[x,y]| Point { x,y }).to_vec()).unwrap());
    r.set_transform_preview(Some(&preview)).unwrap();
    r.begin_frame_memory(frame);
    assert_eq!(request(&r, frame).ok().map(|q| q.plan.level), Some(2));
    preview.selection = Some(Selection::polygon([[0.,0.],[2124.,0.],[2124.,2832.],[0.,2832.]]
        .map(|[x,y]| Point { x,y }).to_vec()).unwrap());
    r.set_transform_preview(Some(&preview)).unwrap();
    r.begin_frame_memory(frame);

    for (label, selection, kept) in [("none", None, false),
        ("whole", Some(Selection::polygon([[0.,0.],[4248.,0.],[4248.,2832.],[0.,2832.]]
            .map(|[x,y]| Point { x,y }).to_vec()).unwrap()), false),
        ("partial", preview.selection.clone(), true)] {
        let mut r = crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
        let mut transforms = crate::paint_transform::PaintTransforms::new(&r.device);
        let preview = layer_render::TransformPreview { selection, ..preview.clone() };
        let level = crate::paint_transform::input_level(plan.level, &preview, extent);
        assert_eq!(transforms.input_requirements(&preview, level, extent), (level, kept), "{label}: transform input requirements");
        let pyramid = display_mips::Plan::at(extent, level).pixel_bytes_through(display_mips::MAX_LEVEL);
        let _budget = crate::memory::memory_tests::display_frame_with_budget(&mut r, frame, pyramid);
        assert_eq!(r.projected_restore_bytes(), 0, "{label}: no pending raster restores");
        let available = r.memory.display_grant() + r.memory.raster_grant();
        assert!(pyramid <= available && available < pyramid + pyramid,
            "{label}: preparation must have headroom for exactly one input pyramid: {pyramid} <= {available} < {}", pyramid + pyramid);
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let result = transforms.prepare_preview(&mut r, &mut encoder, &preview, doc.scene(), plan.level, false);
        if kept {
            assert!(matches!(result, Err(GpuRasterError::NotReady | GpuRasterError::SourceWorkingSetExceeded)),
                "{label}: a partial selection needs a second input pyramid: {result:?}");
        } else {
            assert!(result.is_ok(), "{label}: one input pyramid must admit an entire-image preview: {result:?}");
        }
    }
}

#[test]
fn moved_copies_reconstruct_original_coverage_for_every_map() {
    let extent = [257,129];
    let bounds = Rect { min: Point::default(), max: Point { x: 257., y: 129. } };
    let mut doc = document_at(extent);
    let id = source_at(&doc,0);
    paint_mut(&mut doc,0).base = Some(layer_core::authored::PaintBase::new((rgba8_source(extent, |x, y| [128, (x/2) as u8, y as u8, (64+x/2) as u8])).into()));
    let part = Selection::polygon([[20.5,15.25],[210.,22.],[240.,110.5],[38.,117.]]
        .map(|[x,y]| Point {x,y}).to_vec()).unwrap();
    let projective = Projective::rect_to_quad(bounds,
        [[8.,4.],[251.,9.],[235.,124.],[15.,115.]].map(|[x,y]| Point {x,y})).unwrap();
    let mesh = MeshMap::fit(bounds, [3,3], |p| projective.map(p)).unwrap()
        .move_node(5,Point {x:12.,y:-6.}).unwrap();
    for selection in [None, Some(part)] {
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        exact.test.reference = true;
        let mut frame = packet(doc.scene(), extent);
        frame.composite_all = false; frame.view.document_to_surface = [0.25,0.,0.,0.25,0.,0.];
        submit_published(&mut r, frame); submit_published(&mut exact, frame);
        let original = r.readback_srgb_rgba8().unwrap();
        let original_display = display_pixels(&r);
        let maps = [LayerPlacement::default(), LayerPlacement::from_affine(Affine::translation(Point {x:48.,y:12.})),
            LayerPlacement::from_projective(projective), layer_core::LayerPlacement { mesh: Some(Arc::new(mesh.clone())), ..Default::default() }, LayerPlacement::default()];
        for map in maps {
            for keep_source in [false,true] {
                let preview = layer_render::TransformPreview { transaction: 1, target: id, moving: true,
                    selection: selection.clone(), transform: ImageTransform { placement: layer_core::LayerPlacement { interpolation: Interpolation::Bicubic, ..map.clone() }, keep_source, source_from_owner:None, source_base:None } };
                for renderer in [&mut r,&mut exact] {
                    renderer.set_transform_preview(Some(&preview)).unwrap(); crate::test_support::submit_ready(renderer, frame).unwrap();
                }
                if preview.transform.is_identity() {
                    let actual=display_pixels(&r);
                    let largest = crate::test_support::max_error(&actual, &original_display);
                    let mismatches:Vec<_>=actual.iter().zip(&original_display).enumerate().filter(|(_, (a,b))|a.iter().zip(b.iter()).any(|(a,b)|(a-b).abs()>1e-5)).take(8).collect();
                    assert!(largest<1e-5, "an unchanged transform preserves displayed coverage: {largest} keep={keep_source} selection={} {mismatches:?}", selection.is_some());
                }
                let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), r.scale_display.as_ref().unwrap().plan, r.scale_display.as_ref().unwrap().texture().format());
                assert!(error[0] < 0.004 && error[1] < 0.06, "map={map:?} keep={keep_source} selection={} {error:?}",selection.is_some());
                crate::test_support::assert_same_canonical(&mut r, &mut exact);
                assert_eq!(r.test.source_captures.get(),1);
            }
        }
        r.prepare_moving_pixels(None); r.set_transform_preview(None).unwrap(); crate::test_support::submit_ready(&mut r, frame).unwrap();
        assert_eq!(r.readback_srgb_rgba8().unwrap(),original);
    }
}

#[test]
fn optional_transform_inputs_park_until_the_compiler_wakes_the_owner() {
    let doc=document_at([769,513]);
    let extent=doc.composition().size;
    let id=source_at(&doc,0);
    let selection=Selection::polygon([[64.,64.],[640.,64.],[640.,448.],[64.,448.]]
        .map(|[x,y]|Point {x,y}).to_vec()).unwrap();
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let (woken,wake)=std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move||{let _=woken.send(());}));
    let mut frame=packet(doc.scene(),extent);
    frame.composite_all=false;frame.view.document_to_surface=[0.25,0.,0.,0.25,0.,0.];
    crate::test_support::submit_settled(&mut r,frame);
    while wake.try_recv().is_ok() {}
    r.shader_idle(false,false);
    r.prepare_moving_pixels(Some((id,selection)));
    frame.time_seconds+=0.1;
    crate::test_support::submit_published(&mut r,frame);
    let ready=std::time::Instant::now()+READBACK_TIMEOUT;
    while r.navigator.pending() || publication_pending(&r) || r.scale_display.as_ref().is_some_and(|cache|cache.has_pending_work(&r)) {
        assert!(std::time::Instant::now()<ready,"selection display must finish before optional warming");
        r.wait_idle().unwrap();
        frame.time_seconds+=0.1;
        crate::test_support::submit_ready(&mut r,frame).unwrap();
    }
    let work=r.metrics.composited_pixels;
    let captures=r.test.source_captures.get();
    for _ in 0..3 {
        crate::test_support::submit_ready(&mut r,frame).unwrap();
        assert!(r.next_frame()!=layer_render::FrameDemand::Work,"unadmitted optional compilation parks the owner");
        assert_eq!(r.metrics.composited_pixels,work);
        assert_eq!(r.test.source_captures.get(),captures);
    }
    r.shader_idle(true,true);
    let deadline=std::time::Instant::now()+READBACK_TIMEOUT;
    while r.test.source_captures.get()==captures {
        wake.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())).unwrap();
        crate::test_support::submit_ready(&mut r,frame).unwrap();
    }
    crate::test_support::submit_settled(&mut r,frame);
    assert_eq!(r.test.source_captures.get(),captures+1);
}

#[test]
fn move_transactions_adopt_prepared_inputs_and_invalidate_them_on_restore() {
    let mut doc = document_at([769,513]);
    let extent = doc.composition().size; let id = source_at(&doc,0);
    let part = Selection::polygon([[64.,64.],[640.,64.],[640.,448.],[64.,448.]]
        .map(|[x,y]| Point {x,y}).to_vec()).unwrap();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut frame = packet(doc.scene(),extent);
    frame.composite_all=false; frame.view.document_to_surface=[0.25,0.,0.,0.25,0.,0.];
    submit_published(&mut r, frame); submit_published(&mut exact, frame);
    let original = r.readback_srgb_rgba8().unwrap();
    r.background_ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
    r.prepare_moving_pixels(Some((id,part.clone())));
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    while !r.raster_dependencies_ready(frame, None) {
        assert!(std::time::Instant::now() < deadline);
        r.poll_pending(frame.view).unwrap();
        std::thread::yield_now();
    }
    assert!(!r.can_submit());
    r.background_ready.store(true, std::sync::atomic::Ordering::Release);
    crate::test_support::submit_settled(&mut r, frame);
    let captures=r.test.source_captures.get(); let reduced=r.test.reduced_pages.get();
    assert_eq!(captures,1); assert!(reduced>0);
    for _ in 0..2 {crate::test_support::submit_ready(&mut r, frame).unwrap();}
    assert_eq!(r.test.reduced_pages.get(),reduced);
    for offset in [0.,64.,-32.] {
        let preview=layer_render::TransformPreview {transaction:7,target: id,moving:true,selection:Some(part.clone()),
            transform:ImageTransform { placement: layer_core::LayerPlacement { interpolation: Interpolation::Nearest, ..LayerPlacement::from_affine(Affine::translation(Point {x:offset,y:0.})) },keep_source:true,source_from_owner:None,source_base:None}};
        for renderer in [&mut r,&mut exact] {renderer.set_transform_preview(Some(&preview)).unwrap();crate::test_support::submit_ready(renderer, frame).unwrap();}
        assert_eq!(r.test.source_captures.get(),captures);
        assert_eq!(r.test.reduced_pages.get(),reduced);
        assert!(r.paint_layers.iter().all(|l|l.pages.is_empty()));
        assert_eq!(r.readback_srgb_rgba8().unwrap(),exact.readback_srgb_rgba8().unwrap());
    }
    r.prepare_moving_pixels(None);r.set_transform_preview(None).unwrap();crate::test_support::submit_ready(&mut r, frame).unwrap();
    assert_eq!(r.readback_srgb_rgba8().unwrap(),original);
    paint_mut(&mut doc,0).raster =layer_core::raster::RasterRevision::pending();
    frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view.document_to_surface=[0.25,0.,0.,0.25,0.,0.];
    r.prepare_moving_pixels(Some((id,part.clone())));crate::test_support::submit_ready(&mut r, frame).unwrap();
    let captures=r.test.source_captures.get();let reduced=r.test.reduced_pages.get();
    assert!(captures>1);
    let restored=[(id,paint_at(&doc,0).raster.clone())];
    crate::test_support::submit_ready(&mut r, FramePacket {restore_rasters:&restored,..frame}).unwrap();
    assert_eq!(r.test.source_captures.get(),captures);
    crate::test_support::submit_ready(&mut r, frame).unwrap();
    assert_eq!(r.test.source_captures.get(),captures+1);
    assert!(r.test.reduced_pages.get()>reduced);
}

#[test]
fn native_previews_waiting_for_photo_tiles_draw_and_recompose_once_they_decode() {
    let doc = document_at([769,513]);
    let extent = doc.composition().size;
    let mut frame = packet(doc.scene(),extent);
    let preview = layer_render::TransformPreview {transaction:1,target:source_at(&doc,0),moving:false,selection:None,
        transform:ImageTransform::affine(Affine::around(Point {x:384.,y:256.},[0.8,0.9],0.15,Point::default()))};
    let [mut waiting,mut decoded] = [(),()].map(|_| {
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        r.test.reference = true;
        crate::test_support::submit_settled(&mut r,frame);
        r
    });
    waiting.source_tiles.get_mut().invalidate();
    frame.composite_all = false;
    for renderer in [&mut waiting,&mut decoded] {
        renderer.set_transform_preview(Some(&preview)).unwrap();
        crate::test_support::submit_settled(renderer,frame);
    }
    let [actual,expected] = [&waiting,&decoded].map(|r| pixels(r,crate::test_support::document_texture(r)));
    let differing = actual.iter().zip(&expected).filter(|(a,b)| a != b).count();
    assert_eq!(differing,0,"composite pixels differ from a preview whose tiles were decoded");
}

#[test]
fn moving_pixels_invalidate_the_cut_only_when_its_coverage_changes() {
    let doc = document_at([769,257]);
    let extent = doc.composition().size;
    let selection = Selection::polygon([[64.,64.],[192.,64.],[192.,192.],[64.,192.]]
        .map(|[x,y]| Point {x,y}).to_vec()).unwrap();
    let cut = PixelRect::new(64,64,192,192);
    for scale in [1.,0.25] {
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        exact.test.reference = true;
        let mut frame = packet(doc.scene(),extent);
        frame.composite_all = false;
        frame.view.document_to_surface = [scale,0.,0.,scale,0.,0.];
        submit_published(&mut r, frame); submit_published(&mut exact, frame);
        for (step,(offset,keep_source)) in [(384.,false),(416.,false),(416.,true),
            (448.,true),(448.,false),(0.,false)].into_iter().enumerate() {
            let preview = layer_render::TransformPreview {transaction:1,target: source_at(&doc,0),
                moving:true,selection:Some(selection.clone()),transform:ImageTransform { placement: LayerPlacement::from_affine(Affine::translation(Point {x:offset,y:0.})),
                    keep_source,..Default::default()}};
            for renderer in [&mut r,&mut exact] {
                renderer.set_transform_preview(Some(&preview)).unwrap();crate::test_support::submit_ready(renderer, frame).unwrap();
            }
            if step == 1 || step == 3 {
                assert!(r.composite_damage.intersect(cut).is_empty(),
                    "unchanged cut scale={scale} step={step} damage={:?}",r.composite_damage);
            } else {
                assert_eq!(r.composite_damage.intersect(cut),cut,
                    "changed cut scale={scale} step={step}");
            }
            let error = quality(&display_pixels(&r), &pixels(&exact,crate::test_support::document_texture(&exact)),r.scale_display.as_ref().unwrap().plan, r.scale_display.as_ref().unwrap().texture().format());
            assert!(error[0] < 0.004 && error[1] < 0.06, "scale={scale} step={step} {error:?}");
            assert_eq!(r.readback_srgb_rgba8().unwrap(),exact.readback_srgb_rgba8().unwrap());
        }
    }
}
#[test]
fn retained_committed_pages_are_independent_of_mutable_source_pixels() {
    let doc = document();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    crate::test_support::submit_settled(&mut r, frame);
    let retained = r.scale_display.as_ref().unwrap().pages.clone();
    let pages = retained.lock().unwrap().retained_pages().into_iter().filter(|page|page.key.source.is_none()).collect::<Vec<_>>();
    assert!(!pages.is_empty());
    let before: Vec<_> = pages.iter().map(|page| pixels(&r, &page.texture)).collect();
    assert!(before.iter().flatten().flatten().any(|value| *value != 0.));
    let sources: Vec<_> = r.scene.as_ref().unwrap().scale_sources.entries.keys()
        .flat_map(|id| source_textures(&r,*id,None)).collect();
    assert!(!sources.is_empty());
    for source in sources {
        assert!(pages.iter().all(|page| page.texture != source));
        let bytes = texture_bytes(&source) / u64::from(source.width() * source.height());
        r.queue.write_texture(source.as_image_copy(), &vec![0; (u64::from(source.width()*source.height())*bytes) as usize],
            wgpu::TexelCopyBufferLayout {offset:0,bytes_per_row:Some(source.width()*bytes as u32),rows_per_image:None}, source.size());
        assert!(pixels(&r, &source).iter().flatten().all(|value| *value == 0.));
    }
    for (page, expected) in pages.iter().zip(before) { assert_eq!(pixels(&r, &page.texture), expected); }
}

#[test]
fn native_previews_larger_than_the_decoded_floor_draw_from_resumed_original_captures() {
    let doc = document_at([4097,2049]);
    let extent = doc.composition().size;
    let preview = layer_render::TransformPreview {transaction:1,target:source_at(&doc,0),moving:false,selection:None,
        transform:ImageTransform::affine(Affine::around(Point {x:2048.,y:1024.},[0.8,0.9],0.15,Point::default()))};
    let [floor,reference] = [64,1024].map(|slots| {
        let mut r = crate::memory::memory_tests::renderer_for_restore(&doc);
        r.test.reference = true;
        let mut frame = packet(doc.scene(),extent);
        crate::memory::memory_tests::declare_source_budget(&mut r, frame, slots, 16 << 20, 44 << 20);
        crate::test_support::submit_settled(&mut r,frame);
        frame.composite_all = false;
        r.set_transform_preview(Some(&preview)).unwrap();
        crate::memory::memory_tests::declare_source_budget(&mut r, frame, slots, 16 << 20, 44 << 20);
        crate::test_support::submit_settled(&mut r,frame);
        r
    });
    assert!(!floor.transform_sources_pending && floor.transforms.as_ref().unwrap().has_preview());
    assert_eq!(pixels(&floor,crate::test_support::document_texture(&floor)),pixels(&reference,crate::test_support::document_texture(&reference)),
        "a preview reading more originals than the decoded slots hold must match one whose originals all fit");
}

#[test]
fn selection_standby_waits_for_cold_samples_without_deferring_the_ready_frame() {
    let doc=document_at([769,513]);let extent=doc.composition().size;let id=source_at(&doc,0);
    let part=Selection::polygon([[64.,64.],[640.,64.],[640.,448.],[64.,448.]].map(|[x,y]|Point {x,y}).to_vec()).unwrap();
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame=packet(doc.scene(),extent);frame.view.document_to_surface=[0.25,0.,0.,0.25,0.,0.];
    crate::test_support::submit_settled(&mut r,frame);
    assert!(r.scale_display.as_ref().is_some_and(|cache|cache.plan.level>0 && cache.evaluation!=Evaluation::Native));
    let original=display_pixels(&r);let revision=r.artwork_revision;
    let source=paint_at(&doc,0).base.as_ref().unwrap().image.storage().clone();
    let store=crate::test_support::GatedStore::new(&layer_core::raster_storage::RetainedTiles::from_sources([source.as_ref()]),false);
    let samples=&r.device.source_samples;let limit=samples.stats().limit_bytes;
    samples.set_limit(0);samples.trim_unused(usize::MAX);samples.set_limit(limit);
    crate::test_support::evict_sources(&mut r);
    assert!(page_coordinates(PixelRect::full(extent)).all(|coordinate|r.source_tiles.borrow().prepared_view(&source,coordinate).is_none()));
    r.prepare_moving_pixels(Some((id,part)));frame.composite_all=false;
    let submitted=r.submitted_updates();let result=r.submit_when_ready(frame,None);
    assert!(!matches!(result,std::task::Poll::Pending),"the committed picture must not require the optional standby samples");
    store.gate.entered();
    assert!(matches!(result,std::task::Poll::Ready(Ok(()))),"cold optional standby escaped readiness: {result:?}");
    assert_eq!(r.submitted_updates(),submitted+1);
    assert_eq!(r.artwork_revision,revision);assert_eq!(display_pixels(&r),original);
    store.gate.release();crate::test_support::submit_settled(&mut r,frame);
    let captures=r.test.source_captures.get();let reduced=r.test.reduced_pages.get();assert_eq!(captures,1);assert!(reduced>0);
    crate::test_support::submit_ready(&mut r,frame).unwrap();
    assert_eq!(r.test.source_captures.get(),captures);assert_eq!(r.test.reduced_pages.get(),reduced);
    assert_eq!(r.artwork_revision,revision);assert_eq!(display_pixels(&r),original);
}
