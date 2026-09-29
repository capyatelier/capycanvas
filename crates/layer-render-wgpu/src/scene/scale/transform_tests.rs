use super::*;
use layer_core::{Affine, ImageTransform, Interpolation, MeshMap, Point, Projective, Rect, Selection, TransformMap};

#[test]
fn pass_through_children_above_a_transform_keep_shared_display_composition() {
    let mut doc = document();
    let extent = [doc.width, doc.height];
    let moving = doc.layers[0].id;
    let mut child = doc.layers[0].clone();
    child.id = LayerId(90);
    child.opacity = 0.35;
    let mut group = Layer::paint(LayerId(91), "pass through");
    group.kind = LayerKind::Group;
    group.properties.blend = layer_core::LayerBlend::PassThrough;
    child.properties.parent = Some(group.id);
    doc.layers.splice(0..0, [group, child]);
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    for blend in [layer_core::LayerBlend::Normal, layer_core::LayerBlend::Multiply] {
        doc.layers[1].properties.blend = blend;
        let mut frame = packet(&doc.layers, extent);
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        r.submit(frame).unwrap(); exact.submit(frame).unwrap();
        frame.composite_all = false;
        let original = display_pixels(&r);
        for step in 1..4 {
            let transform = layer_render::TransformPreview { transaction: 1, layer: moving, moving: step < 3, selection: None,
                transform: ImageTransform::affine(Affine::translation(Point { x: 8. * step as f32, y: -4. })) };
            for renderer in [&mut r, &mut exact] {
                renderer.set_transform_preview(Some(&transform)).unwrap(); renderer.submit(frame).unwrap();
            }

            if transform.moving { assert!(!r.has_pending_work()); }
            let cache = r.scale_display.as_ref().expect("group children stay on the display graph");
            let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan);
            assert!(error[0] < 0.004 && error[1] < 0.06, "{blend:?} step={step} error={error:?}");
        }
        for renderer in [&mut r, &mut exact] {
            renderer.set_transform_preview(None).unwrap(); renderer.submit(frame).unwrap();
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
    let mesh = MeshMap::from_projective(bounds, [3,3], &projective).unwrap()
        .move_node(5, Point { x: 24., y: -12. }).unwrap();
    let folded = mesh.move_node(5, Point { x: 360., y: 160. }).unwrap();
    let maps = [TransformMap::Affine(Affine::translation(Point { x: 12., y: -8. })),
        TransformMap::Affine(Affine::around(Point { x: 250., y: 125. }, [0.8, 1.1], 0.2, Point { x: 8., y: 2. })),
        TransformMap::Projective(projective), TransformMap::Mesh(Arc::new(mesh)), TransformMap::Mesh(Arc::new(folded))];
    for (stacked, blend) in [
        (false, layer_core::LayerBlend::Normal),
        (true, layer_core::LayerBlend::Normal),
        (true, layer_core::LayerBlend::Difference),
        (true, layer_core::LayerBlend::Luminosity),
    ] {
    for placement in [Affine::IDENTITY, Affine([0.45, 0.1, -0.1, 0.45, 200., 10.])] {
        for selection in [None, Some(all.clone()), Some(selected.clone())] {
            let mut doc = document();
            let id = doc.layers[0].id;
            doc.layers[0].opacity = 0.8;
            doc.layers[0].properties.blend = blend;
            doc.layers[0].properties.placement = placement;
            let mut below = doc.layers[0].clone();
            below.id = LayerId(40); below.opacity = 1.; below.properties.placement = Affine::IDENTITY;
            let mut above = below.clone();
            above.id = LayerId(41); above.opacity = 0.23; above.properties.blend = layer_core::LayerBlend::Multiply;
            if stacked { doc.layers.insert(0, above); doc.layers.insert(2, below); }
            let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
            let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
            exact.test.reference = true;
            let mut frame = packet(&doc.layers, extent);
        frame.blend_space = space;
            frame.composite_all = false;
            frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
            r.submit(frame).unwrap(); exact.submit(frame).unwrap();
            let original = display_pixels(&r);
            for (step, map) in maps.iter().enumerate() {
                let preview = layer_render::TransformPreview { transaction: 1, layer: id, moving: true,
                    selection: selection.clone(), transform: ImageTransform { map: map.clone(), interpolation: Interpolation::Bicubic, ..Default::default() } };
                for renderer in [&mut r, &mut exact] {
                    renderer.set_transform_preview(Some(&preview)).unwrap(); renderer.submit(frame).unwrap();
                }
                assert!(r.scale_display.is_some());
                assert!(r.paint_layers.iter().all(|l| l.pages.is_empty()), "display motion does not allocate native preview pages");
                assert!(!r.has_pending_work(), "moving transforms defer exact refinement");
                assert_eq!(r.test.source_captures.get(), 1);
                let displayed = display_pixels(&r);
                let exact_pixels = pixels(&exact, crate::test_support::document_texture(&exact));
                let error = quality_linear(&displayed, &exact_pixels, r.scale_display.as_ref().unwrap().plan,
                    |color| linear_color(color, space, doc.color.space));
                eprintln!("{space:?} placement={placement:?} selection={} step={step} error={error:?}", selection.is_some());
                assert!(error[0] < 0.004 && error[1] < 0.06, "transform reduction quality {error:?}");
                let mut a = vec![0; extent[0] as usize * extent[1] as usize * 4];
                let mut b = a.clone();
                r.copy_rgba8_srgb(&mut a, extent[0] as usize * 4).unwrap();
                exact.copy_rgba8_srgb(&mut b, extent[0] as usize * 4).unwrap();
                assert_eq!(a, b, "exact queries evaluate native transform pixels");
                assert_eq!(displayed, display_pixels(&r), "an exact query preserves the displayed approximation");
                r.submit(frame).unwrap();
                assert_eq!(displayed, display_pixels(&r));
                assert!(r.paint_layers.iter().map(|l| l.pages.len()).sum::<usize>() <= 9,
                    "an exact query retains only its final native dependency window");
            }
            for renderer in [&mut r, &mut exact] {
                renderer.set_transform_preview(None).unwrap(); renderer.submit(frame).unwrap();
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
    let extent = [doc.width, doc.height];
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut frame = packet(&doc.layers, extent);
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    frame.composite_all = false;
    r.submit(frame).unwrap();
    r.set_transform_preview(Some(&layer_render::TransformPreview {
        transaction: 1, layer: doc.layers[0].id, moving: true, selection: None,
        transform: ImageTransform::affine(Affine::translation(Point { x: 13.5, y: -5.25 })),
    })).unwrap();
    r.submit(frame).unwrap();
    let mut capture = artwork::Capture::default();
    for origin in [[256,256], [1024,768], [1536,1024], [0,0]] {
        let region = PixelRect::new(origin[0], origin[1], origin[0] + 32, origin[1] + 32);
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let result = capture.region(&mut r, frame, region, [32,32], &mut encoder).unwrap();
        r.uploads.finish(&encoder); encoder.submit(&r.queue);
        assert!(r.paint_layers.iter().map(|l| l.pages.len()).sum::<usize>() <= 9,
            "one native tile with interpolation support reaches at most three tiles per axis");
        assert!(pixels(&r, &result.texture).iter().all(|p| p.iter().all(|v| v.is_finite())));
    }
    r.set_transform_preview(None).unwrap(); r.submit(frame).unwrap();
    assert!(r.paint_layers.iter().all(|l| l.pages.is_empty()));
}

#[test]
fn transform_zoom_release_and_commit_preserve_native_pixels() {
    let mut doc = document();
    let extent = [doc.width, doc.height];
    let selection = Selection::polygon([[150.,50.],[350.,50.],[350.,200.],[150.,200.]]
        .map(|[x,y]| Point { x,y }).to_vec()).unwrap();
    let mut preview = layer_render::TransformPreview { transaction: 1, layer: doc.layers[0].id,
        moving: true, selection: Some(selection.clone()),
        transform: ImageTransform { interpolation: Interpolation::Bicubic, map: TransformMap::Affine(
            Affine::around(Point { x: 250., y: 125. }, [3.2,2.1], 0.31, Point { x: 7., y: -3. })), ..Default::default() } };
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    let mut frame = packet(&doc.layers, extent);
    frame.composite_all = false;
    r.submit(frame).unwrap(); exact.submit(frame).unwrap();
    for (scale, moving) in [(0.125,true),(0.5,true),(1.,true),(0.25,true),(0.25,false)] {
        preview.moving = moving;
        frame.view.document_to_surface = [scale,0.,0.,scale,0.,0.];
        for renderer in [&mut r, &mut exact] {
            renderer.set_transform_preview(Some(&preview)).unwrap(); renderer.submit(frame).unwrap();
        }
        assert!(r.scale_display.is_some());
        if moving { assert!(!r.has_pending_work()); }
        let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), r.scale_display.as_ref().unwrap().plan);
        assert!(error[0] < 0.004 && error[1] < 0.06, "scale={scale} moving={moving} {error:?}");
        assert_eq!(r.readback_srgb_rgba8().unwrap(), exact.readback_srgb_rgba8().unwrap());
        assert_eq!(r.test.source_captures.get(), 1);
        if !moving { assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact))); }
    }
    let expected = exact.readback_srgb_rgba8().unwrap();
    let mut coverage = layer_core::LayerMask::reveal_all(LayerId(50), Point::default());
    coverage.default_coverage = 0.; coverage.initial = Some(selection);
    let operation = layer_core::LayerOperation { placement: Affine::IDENTITY, coverage,
        kind: layer_core::LayerOperationKind::Transform(preview.transform) };
    let batch = layer_render::DabBatch { kind: layer_render::DabBatchKind::LayerOperation(0), dab_count: 0,
        ..dab_batch(doc.layers[0].id, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), operation.bounds(extent)) };
    doc.layers[0].pending_operations.push(operation);
    let mut frame = packet(&doc.layers, extent);
    frame.composite_all = false; frame.view.document_to_surface = [0.25,0.,0.,0.25,0.,0.];
    frame.dab_batches = std::slice::from_ref(&batch);
    r.set_transform_preview(None).unwrap(); r.submit(frame).unwrap();
    assert_eq!(r.readback_srgb_rgba8().unwrap(), expected);
    assert!(!r.transforms.as_ref().unwrap().has_preview());
}

#[test]
fn transformed_group_children_keep_clipping_and_linked_mask_semantics() {
    let extent = [257,129];
    for (mask_target, linked) in [(false,false),(false,true),(true,true)] {
        let mut doc = document_at(extent);
        let mut group = Layer::paint(LayerId(20), "group");
        group.kind = LayerKind::Group; group.opacity = 0.71;
        let mut base = doc.layers[0].clone();
        base.id = LayerId(30); base.properties.parent = Some(group.id); base.opacity = 0.73;
        doc.layers[0].properties.parent = Some(group.id);
        doc.layers[0].properties.clipped = true;
        doc.layers[0].properties.blend = layer_core::LayerBlend::Multiply;
        let mut mask = layer_core::LayerMask::reveal_all(LayerId(40), Point { x: 13., y: -4. });
        mask.default_coverage = 0.23;
        mask.initial = Some(Selection::polygon([[20.,10.],[230.,20.],[190.,120.],[30.,90.]]
            .map(|[x,y]| Point {x,y}).to_vec()).unwrap());
        mask.linked = linked;
        doc.layers[0].mask = Some(mask);
        let id = if mask_target { LayerId(40) } else { doc.layers[0].id };
        doc.layers.insert(0,group); doc.layers.insert(2,base);
        let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        exact.test.reference = true;
        let mut frame = packet(&doc.layers, extent);
        frame.composite_all = false; frame.view.document_to_surface = [0.25,0.,0.,0.25,0.,0.];
        r.submit(frame).unwrap(); exact.submit(frame).unwrap();
        let original = display_pixels(&r);
        for offset in [9.,-13.] {
            let preview = layer_render::TransformPreview { transaction: 1, layer: id, moving: true, selection: None,
                transform: ImageTransform::affine(Affine::translation(Point { x: offset, y: 5. })) };
            for renderer in [&mut r, &mut exact] {
                renderer.set_transform_preview(Some(&preview)).unwrap(); renderer.submit(frame).unwrap();
            }
            assert!(r.scale_display.is_some());
            let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), r.scale_display.as_ref().unwrap().plan);
            assert!(error[0] < 0.004 && error[1] < 0.06, "mask={mask_target} linked={linked} {error:?}");
            assert_eq!(r.readback_srgb_rgba8().unwrap(), exact.readback_srgb_rgba8().unwrap());
        }
        r.set_transform_preview(None).unwrap(); r.submit(frame).unwrap();
        assert_eq!(original, display_pixels(&r));
    }
}

#[test]
fn retained_transform_detail_still_filters_the_current_output_footprint() {
    let extent = [65,33];
    let mut doc = Document::new("transform reconstruction", extent[0], extent[1]);
    let mut source = SourceBuilder::new(extent, SourceInterpretation { channels: SourceChannels::Rgba,
        depth: SampleDepth::U8, profile: Default::default(), profile_assumed: false }, 1 << 20).unwrap();
    let row: Vec<_> = (0..extent[0]).flat_map(|x| [if (x/3)%2 == 0 {255} else {0}, 0, 0, 255]).collect();
    for _ in 0..extent[1] { source.push_row(&row).unwrap(); }
    doc.layers[0].source = Some(Arc::new(source.finish().unwrap()));
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut frame = packet(&doc.layers, extent);
    frame.composite_all = false; frame.view.document_to_surface = [0.25,0.,0.,0.25,0.,0.];
    r.submit(frame).unwrap();
    let original = display_pixels(&r);
    let mut work = 0;
    for (step, scale) in [4.,1.,4.,1.].into_iter().enumerate() {
        r.set_transform_preview(Some(&layer_render::TransformPreview { transaction: 1, layer: doc.layers[0].id,
            moving: true, selection: None, transform: ImageTransform::affine(
                Affine::around(Point { x: 32., y: 16. }, [scale;2], 0., Point::default())) })).unwrap();
        r.submit(frame).unwrap();
        if step == 0 { work = r.test.reduced_pages.get(); }
        else { assert_eq!(r.test.reduced_pages.get(), work, "the immutable source is not recaptured on scale oscillations"); }
        if scale == 1. {
            let actual = display_pixels(&r);
            let error = actual.iter().flatten().zip(original.iter().flatten())
                .map(|(a,b)| (a-b).abs()).fold(0.,f32::max);
            let mismatch: Vec<_> = actual.iter().zip(&original).enumerate().filter(|(_, (a,b))| a.iter().zip(b.iter()).any(|(a,b)| (a-b).abs()>1e-5)).take(8).collect();
            assert!(error < 1e-5, "restored identity must reconstruct the original averaged pixels: {error}: {mismatch:?}");
        }
    }
}

#[test]
fn whole_image_selections_reserve_one_transform_input_pyramid() {
    let extent = [4248,2832];
    let doc = Document::new("transform admission", extent[0], extent[1]);
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut frame = packet(&doc.layers, extent);
    frame.composite_all = false; frame.view.document_to_surface = [0.1597,0.,0.,0.1597,0.,0.];
    let plan = display_mips::Plan::at(extent, 2);
    let mut preview = layer_render::TransformPreview { transaction: 1, layer: doc.layers[0].id,
        moving: true, selection: None, transform: ImageTransform::affine(Affine([4.,0.,0.,4.,0.,0.])) };
    r.set_transform_preview(Some(&preview)).unwrap();
    let no_selection = allocation(&r, plan, frame, None);
    assert_eq!(request(&r, frame).ok().map(|q| q.plan.level), Some(2));
    preview.selection = Some(Selection::polygon([[0.,0.],[4248.,0.],[4248.,2832.],[0.,2832.]]
        .map(|[x,y]| Point { x,y }).to_vec()).unwrap());
    r.set_transform_preview(Some(&preview)).unwrap();
    assert_eq!(allocation(&r, plan, frame, None), no_selection);
    assert_eq!(request(&r, frame).ok().map(|q| q.plan.level), Some(2));
    preview.selection = Some(Selection::polygon([[0.,0.],[2124.,0.],[2124.,2832.],[0.,2832.]]
        .map(|[x,y]| Point { x,y }).to_vec()).unwrap());
    r.set_transform_preview(Some(&preview)).unwrap();
    assert!(allocation(&r, plan, frame, None)[1] > no_selection[1]);
}

#[test]
fn moved_copies_reconstruct_original_coverage_for_every_map() {
    let extent = [257,129];
    let bounds = Rect { min: Point::default(), max: Point { x: 257., y: 129. } };
    let mut doc = document_at(extent);
    let id = doc.layers[0].id;
    let mut builder = SourceBuilder::new(extent, SourceInterpretation { channels: SourceChannels::Rgba,
        depth: SampleDepth::U8, profile: Default::default(), profile_assumed: false }, 1 << 20).unwrap();
    for y in 0..extent[1] {
        builder.push_row(&(0..extent[0]).flat_map(|x| [128, (x/2) as u8, y as u8, (64+x/2) as u8]).collect::<Vec<_>>()).unwrap();
    }
    doc.layers[0].source = Some(Arc::new(builder.finish().unwrap()));
    let part = Selection::polygon([[20.5,15.25],[210.,22.],[240.,110.5],[38.,117.]]
        .map(|[x,y]| Point {x,y}).to_vec()).unwrap();
    let projective = Projective::rect_to_quad(bounds,
        [[8.,4.],[251.,9.],[235.,124.],[15.,115.]].map(|[x,y]| Point {x,y})).unwrap();
    let mesh = MeshMap::from_projective(bounds,[3,3],&projective).unwrap()
        .move_node(5,Point {x:12.,y:-6.}).unwrap();
    for selection in [None, Some(part)] {
        let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        exact.test.reference = true;
        let mut frame = packet(&doc.layers, extent);
        frame.composite_all = false; frame.view.document_to_surface = [0.25,0.,0.,0.25,0.,0.];
        r.submit(frame).unwrap(); exact.submit(frame).unwrap();
        let original = r.readback_srgb_rgba8().unwrap();
        let original_display = display_pixels(&r);
        let maps = [TransformMap::default(), TransformMap::Affine(Affine::translation(Point {x:48.,y:12.})),
            TransformMap::Projective(projective), TransformMap::Mesh(Arc::new(mesh.clone())), TransformMap::default()];
        for map in maps {
            for keep_source in [false,true] {
                let preview = layer_render::TransformPreview { transaction: 1, layer: id, moving: true,
                    selection: selection.clone(), transform: ImageTransform { map: map.clone(),
                        interpolation: Interpolation::Bicubic, keep_source } };
                for renderer in [&mut r,&mut exact] {
                    renderer.set_transform_preview(Some(&preview)).unwrap(); renderer.submit(frame).unwrap();
                }
                if preview.transform.is_identity() {
                    let largest = display_pixels(&r).iter().flatten().zip(original_display.iter().flatten())
                        .map(|(a,b)|(a-b).abs()).fold(0.,f32::max);
                    let actual=display_pixels(&r);
                    let mismatches:Vec<_>=actual.iter().zip(&original_display).enumerate().filter(|(_, (a,b))|a.iter().zip(b.iter()).any(|(a,b)|(a-b).abs()>1e-5)).take(8).collect();
                    assert!(largest<1e-5, "an unchanged transform preserves displayed coverage: {largest} keep={keep_source} selection={} {mismatches:?}", selection.is_some());
                }
                let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), r.scale_display.as_ref().unwrap().plan);
                assert!(error[0] < 0.004 && error[1] < 0.06, "map={map:?} keep={keep_source} selection={} {error:?}",selection.is_some());
                assert_eq!(r.readback_srgb_rgba8().unwrap(), exact.readback_srgb_rgba8().unwrap());
                assert_eq!(r.test.source_captures.get(),1);
            }
        }
        r.prepare_moving_pixels(None); r.set_transform_preview(None).unwrap(); r.submit(frame).unwrap();
        assert_eq!(r.readback_srgb_rgba8().unwrap(),original);
    }
}

#[test]
fn move_transactions_adopt_prepared_inputs_and_invalidate_them_on_restore() {
    let mut doc = document_at([769,513]);
    let extent = [doc.width,doc.height]; let id = doc.layers[0].id;
    let part = Selection::polygon([[64.,64.],[640.,64.],[640.,448.],[64.,448.]]
        .map(|[x,y]| Point {x,y}).to_vec()).unwrap();
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    let mut frame = packet(&doc.layers,extent);
    frame.composite_all=false; frame.view.document_to_surface=[0.25,0.,0.,0.25,0.,0.];
    r.submit(frame).unwrap(); exact.submit(frame).unwrap();
    let original = r.readback_srgb_rgba8().unwrap();
    r.prepare_moving_pixels(Some((id,part.clone())));
    r.submit(frame).unwrap();
    let captures=r.test.source_captures.get(); let reduced=r.test.reduced_pages.get();
    assert_eq!(captures,1); assert!(reduced>0);
    for _ in 0..2 {r.submit(frame).unwrap();}
    assert_eq!(r.test.reduced_pages.get(),reduced);
    for offset in [0.,64.,-32.] {
        let preview=layer_render::TransformPreview {transaction:7,layer:id,moving:true,selection:Some(part.clone()),
            transform:ImageTransform {map:TransformMap::Affine(Affine::translation(Point {x:offset,y:0.})),
                interpolation:Interpolation::Nearest,keep_source:true}};
        for renderer in [&mut r,&mut exact] {renderer.set_transform_preview(Some(&preview)).unwrap();renderer.submit(frame).unwrap();}
        assert_eq!(r.test.source_captures.get(),captures);
        assert_eq!(r.test.reduced_pages.get(),reduced);
        assert!(r.paint_layers.iter().all(|l|l.pages.is_empty()));
        assert_eq!(r.readback_srgb_rgba8().unwrap(),exact.readback_srgb_rgba8().unwrap());
    }
    r.prepare_moving_pixels(None);r.set_transform_preview(None).unwrap();r.submit(frame).unwrap();
    assert_eq!(r.readback_srgb_rgba8().unwrap(),original);
    doc.layers[0].raster=layer_core::raster::RasterRevision::pending();
    frame=packet(&doc.layers,extent);frame.composite_all=false;frame.view.document_to_surface=[0.25,0.,0.,0.25,0.,0.];
    r.prepare_moving_pixels(Some((id,part.clone())));r.submit(frame).unwrap();
    let captures=r.test.source_captures.get();let reduced=r.test.reduced_pages.get();
    assert!(captures>1);
    let restored=[(id,doc.layers[0].raster.clone())];
    r.submit(FramePacket {restore_rasters:&restored,..frame}).unwrap();
    assert_eq!(r.test.source_captures.get(),captures);
    r.submit(frame).unwrap();
    assert_eq!(r.test.source_captures.get(),captures+1);
    assert!(r.test.reduced_pages.get()>reduced);
}

#[test]
fn moving_pixels_invalidate_the_cut_only_when_its_coverage_changes() {
    let doc = document_at([769,257]);
    let extent = [doc.width,doc.height];
    let selection = Selection::polygon([[64.,64.],[192.,64.],[192.,192.],[64.,192.]]
        .map(|[x,y]| Point {x,y}).to_vec()).unwrap();
    let cut = PixelRect::new(64,64,192,192);
    for scale in [1.,0.25] {
        let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        exact.test.reference = true;
        let mut frame = packet(&doc.layers,extent);
        frame.composite_all = false;
        frame.view.document_to_surface = [scale,0.,0.,scale,0.,0.];
        r.submit(frame).unwrap(); exact.submit(frame).unwrap();
        for (step,(offset,keep_source)) in [(384.,false),(416.,false),(416.,true),
            (448.,true),(448.,false),(0.,false)].into_iter().enumerate() {
            let preview = layer_render::TransformPreview {transaction:1,layer:doc.layers[0].id,
                moving:true,selection:Some(selection.clone()),transform:ImageTransform {
                    map:TransformMap::Affine(Affine::translation(Point {x:offset,y:0.})),
                    keep_source,..Default::default()}};
            for renderer in [&mut r,&mut exact] {
                renderer.set_transform_preview(Some(&preview)).unwrap();renderer.submit(frame).unwrap();
            }
            if step == 1 || step == 3 {
                assert!(r.composite_damage.intersect(cut).is_empty(),
                    "unchanged cut scale={scale} step={step} damage={:?}",r.composite_damage);
            } else {
                assert_eq!(r.composite_damage.intersect(cut),cut,
                    "changed cut scale={scale} step={step}");
            }
            let error = quality(&display_pixels(&r), &pixels(&exact,crate::test_support::document_texture(&exact)),r.scale_display.as_ref().unwrap().plan);
            assert!(error[0] < 0.004 && error[1] < 0.06, "scale={scale} step={step} {error:?}");
            assert_eq!(r.readback_srgb_rgba8().unwrap(),exact.readback_srgb_rgba8().unwrap());
        }
    }
}
