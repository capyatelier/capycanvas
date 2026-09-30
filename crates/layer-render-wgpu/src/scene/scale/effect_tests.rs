use super::*;
use layer_core::{EffectInstance, EffectResolution, EffectValue};

fn effect(id: u64, name: &str) -> Layer {
    let mut layer = Layer::paint(LayerId(id), name);
    layer.kind = LayerKind::Effect;
    layer.effect = Some(Arc::new(EffectInstance::new(crate::tests::fixture(name).program())));
    layer
}

fn exact_pixels(r: &mut WgpuRasterizer, extent: [u32; 2]) -> Vec<u8> {
    let mut bytes = vec![0; (extent[0] * extent[1] * 4) as usize];
    r.copy_rgba8_srgb(&mut bytes, extent[0] as usize * 4).unwrap();
    bytes
}

#[test]
fn pointwise_graph_keeps_document_coordinates_masks_clipping_and_exact_queries() {
    for space in layer_core::BlendSpace::ALL {
    let mut doc = document();
    let extent = [doc.width, doc.height];
    let paint = doc.layers[0].id;
    let mut adjustment = effect(80, "exposure");
    let program = Arc::make_mut(&mut Arc::make_mut(adjustment.effect.as_mut().unwrap()).program);
    program.id = "position_adjustment".into();
    program.entry = "position_adjustment".into();
    program.wgsl = "fn position_adjustment(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4(c.rgb*.7+vec3(p/fx_extent(),0.)*.1*c.a,c.a);}".into();
    let mut mask = layer_core::LayerMask::reveal_all(LayerId(81), Default::default());
    mask.initial = Some(layer_core::Selection::polygon(vec![
        layer_core::Point { x: 63., y: 37. }, layer_core::Point { x: 410., y: 37. },
        layer_core::Point { x: 410., y: 206. }, layer_core::Point { x: 63., y: 206. },
    ]).unwrap());
    adjustment.mask = Some(mask);
    let mut next = effect(82, "exposure");
    Arc::make_mut(next.effect.as_mut().unwrap()).set("exposure", EffectValue::Number(0.3)).unwrap();
    doc.layers.insert(0, adjustment);
    doc.layers.insert(0, next);
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    for level in [0, 1, 2, 3] {
        let mut updates = None;
        for state in 0..4 {
            Arc::make_mut(doc.layers[0].effect.as_mut().unwrap()).set("exposure", EffectValue::Number(state as f32 * 0.1)).unwrap();
            doc.layers[1].properties.clipped = state % 2 == 0;
            doc.layers[1].mask.as_mut().unwrap().inverted = state >= 2;
            doc.layers[1].mask.as_mut().unwrap().offset.x = if state == 3 { 17. } else { 0. };
            let mut frame = packet(&doc.layers, extent);
        frame.blend_space = space;
            let scale = 1. / (1 << level) as f32;
            frame.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
            r.submit(frame).unwrap(); exact.submit(frame).unwrap();
            let work = r.scene.as_ref().unwrap().scale_sources.entries[&paint].updates;
            if let Some(previous) = updates { assert_eq!(work, previous, "filter edits reuse unchanged sources"); }
            updates = Some(work);
            let plan = r.scale_display.as_ref().expect("qualified effects use graph composition").plan;
            assert_eq!(plan.level, level);

            assert_eq!(r.scene.as_ref().unwrap().image_cache_bytes(), 0);
            let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), plan);
            assert!(error[0] < 0.002 && error[1] < 0.015, "level={level} state={state} error={error:?}");
            assert_eq!(exact_pixels(&mut r, extent), exact_pixels(&mut exact, extent));
            r.submit(FramePacket { composite_all: false, ..frame }).unwrap();
            assert_eq!(work, r.scene.as_ref().unwrap().scale_sources.entries[&paint].updates);
            assert_presentation_mip(&r);
        }
    }
    let dab = crate::tests::test_dab([400., 210.], [0.8, 0.2, 0.1, 1.], 0.6);
    let batch = dab_batch(paint, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
    let mut frame = packet(&doc.layers, extent);
        frame.blend_space = space;
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    frame.composite_all = false;
    frame.dabs = std::slice::from_ref(&dab);
    frame.dab_batches = std::slice::from_ref(&batch);
    r.submit(frame).unwrap(); exact.submit(frame).unwrap();
    let plan = r.scale_display.as_ref().unwrap().plan;
    let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), plan);
    assert!(error[0] < 0.002 && error[1] < 0.015, "partial paint error={error:?}");
    assert_eq!(exact_pixels(&mut r, extent), exact_pixels(&mut exact, extent));
    }
}

#[test]
fn window_effects_preserve_document_coordinates_and_shifted_masks() {
    let mut doc = document_at([1541, 1027]);
    let extent = [doc.width, doc.height];
    let mut adjustment = effect(80, "exposure");
    let program = Arc::make_mut(&mut Arc::make_mut(adjustment.effect.as_mut().unwrap()).program);
    program.id = "window_position".into();
    program.entry = "window_position".into();
    program.wgsl = "fn window_position(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4(c.rgb*.7+vec3(p/fx_extent(),0.)*.1*c.a,c.a);}".into();
    let mut mask = layer_core::LayerMask::reveal_all(LayerId(81), layer_core::Point { x: 17., y: -9. });
    mask.initial = Some(layer_core::Selection::polygon(vec![
        layer_core::Point { x: 260., y: 130. }, layer_core::Point { x: 1300., y: 170. },
        layer_core::Point { x: 1100., y: 920. }, layer_core::Point { x: 310., y: 850. },
    ]).unwrap());
    adjustment.mask = Some(mask);
    doc.layers.insert(0, adjustment);
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    let mut whole = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut frame = packet(&doc.layers, extent);
    exact.submit(frame).unwrap();
    let oracle = pixels(&exact, crate::test_support::document_texture(&exact));
    frame.view.width_px = 192;
    frame.view.height_px = 128;
    frame.composite_all = false;
    for level in [0, 1, 2, 4] {
        let scale = 1. / (1 << level) as f32;
        let mut full = packet(&doc.layers, extent);
        full.view.width_px = extent[0];
        full.view.height_px = extent[1];
        full.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        whole.submit(full).unwrap();
        let full_plan = whole.scale_display.as_ref().unwrap().plan;
        assert_eq!(full_plan.bounds, PixelRect::full(extent));
        let complete = display_pixels(&whole);
        for [x, y] in [[-610., -390.], [-810., -490.], [-1210., -810.], [-610., -390.]] {
            frame.view.document_to_surface = [scale, 0., 0., scale, x * scale, y * scale];
            r.submit(frame).unwrap();
            let cache = r.scale_display.as_ref().unwrap();
            assert_eq!(cache.plan.level, level);
            assert!(cache.graph.root.is_some());
            assert!(cache.plan.bounds.min_x() > 0);
            let actual = display_pixels(&r);
            let error = quality(&actual, &oracle, cache.plan);
            let limit = if level == 0 { [1e-6, 1e-5] } else { [0.002, 0.015] };
            assert!(error[0] < limit[0] && error[1] < limit[1], "level={level} window [{x}, {y}]: {error:?}");
            let origin = [cache.plan.bounds.min_x() >> level, cache.plan.bounds.min_y() >> level];
            for (i, pixel) in actual.iter().enumerate() {
                let x = origin[0] + i as u32 % cache.plan.size[0];
                let y = origin[1] + i as u32 / cache.plan.size[0];
                let expected = complete[(y * full_plan.size[0] + x) as usize];
                assert!(pixel.iter().zip(expected).all(|(a, b)| (a - b).abs() < 2e-5),
                    "window and full effects level={level} at [{x}, {y}]: {pixel:?} != {expected:?}");
            }
            assert_presentation_mip(&r);
        }
    }
    assert_eq!(exact_pixels(&mut r, extent), exact_pixels(&mut exact, extent));
}

#[test]
fn qualified_pointwise_catalog_uses_display_graph_and_keeps_native_output() {
    let mut doc = document_at([65, 33]);
    let extent = [doc.width, doc.height];
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    for fixture in crate::tests::fixtures().iter().filter(|f| {
        f.program().resolution == EffectResolution::Display && !f.program().image_boundary()
    }) {
        doc.layers.retain(|l| l.id != LayerId(99));
        doc.layers.insert(0, effect(99, &fixture.program().id));
        let mut frame = packet(&doc.layers, extent);
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        r.submit(frame).unwrap(); exact.submit(frame).unwrap();
        assert_eq!(r.scale_display.as_ref().unwrap().plan.level, 3, "{}", fixture.program().id);
        assert_eq!(exact_pixels(&mut r, extent), exact_pixels(&mut exact, extent), "{}", fixture.program().id);
        assert!(display_pixels(&r).iter().flatten().all(|v| v.is_finite()));
    }
}

#[test]
fn pass_through_graph_matches_ungrouping_and_fades_its_backdrop() {
    for space in layer_core::BlendSpace::ALL {
    let mut doc = document();
    let extent = [doc.width, doc.height];
    let mut group = Layer::paint(LayerId(90), "pass through");
    group.kind = LayerKind::Group;
    group.properties.blend = layer_core::LayerBlend::PassThrough;
    let mut nested = group.clone();
    nested.id = LayerId(91);
    nested.properties.parent = Some(group.id);
    let mut adjustment = effect(92, "exposure");
    Arc::make_mut(adjustment.effect.as_mut().unwrap()).set("exposure", EffectValue::Number(-1.)).unwrap();
    adjustment.properties.parent = Some(nested.id);
    let mut paint = doc.layers[0].clone();
    paint.id = LayerId(93);
    paint.opacity = 0.6;
    paint.properties.blend = layer_core::LayerBlend::Multiply;
    paint.properties.parent = Some(nested.id);
    doc.layers.splice(0..0, [group, nested, adjustment, paint]);
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    for level in [1, 2, 3] {
        let mut draw = |layers: &[Layer]| {
            let mut frame = packet(layers, extent);
        frame.blend_space = space;
            let scale = 1. / (1 << level) as f32;
            frame.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
            r.submit(frame).unwrap();
            assert_eq!(r.scale_display.as_ref().unwrap().plan.level, level);

            display_pixels(&r)
        };
        let mut flat = doc.layers.clone();
        flat.retain(|l| l.kind != LayerKind::Group);
        for layer in &mut flat { layer.properties.parent = None; }
        let expected = draw(&flat);
        let full = draw(&doc.layers);
        let error = crate::test_support::max_error;
        assert!(error(&full, &expected) < 2e-5, "nested pass through equals ungrouping");
        let mut hidden = doc.layers.clone(); hidden[0].visible = false;
        let backdrop = draw(&hidden);
        for masked in [false, true] {
            let mut faded = doc.layers.clone();
            faded[0].opacity = 0.4;
            if masked {
                let mut mask = layer_core::LayerMask::reveal_all(LayerId(94), Default::default());
                mask.default_coverage = 0.3;
                faded[0].mask = Some(mask);
            }
            let amount = if masked { 0.12 } else { 0.4 };
            let expected: Vec<[f32; 4]> = backdrop.iter().zip(&full)
                .map(|(back, front)| std::array::from_fn(|i| back[i] + (front[i] - back[i]) * amount)).collect();
            assert!(error(&draw(&faded), &expected) < 2e-5, "level={level} masked={masked}");
        }
        let mut clipped = doc.layers.clone(); clipped[0].properties.clipped = true;
        let passing = draw(&clipped);
        clipped[0].properties.blend = layer_core::LayerBlend::Normal;
        assert!(error(&passing, &draw(&clipped)) < 2e-5, "clipped groups remain isolated");
    }
    }
}

#[test]
fn spatial_graph_updates_dependency_halos_and_preserves_exact_output() {
    for space in layer_core::BlendSpace::ALL {
    let mut doc = document_at([1027, 773]);
    let extent = [doc.width, doc.height];
    let paint = doc.layers[0].id;
    for (id, sigma) in [(80, 9.), (81, 15.)] {
        let mut blur = effect(id, "gaussian_blur");
        let instance = Arc::make_mut(blur.effect.as_mut().unwrap());
        Arc::make_mut(&mut instance.program).resolution = EffectResolution::Display;
        instance.set("sigma", EffectValue::Number(sigma)).unwrap();
        doc.layers.insert(0, blur);
    }
    doc.layers.insert(0, effect(82, "exposure"));
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    let mut frame = packet(&doc.layers, extent);
        frame.blend_space = space;
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    for renderer in [&mut r, &mut exact] { renderer.submit(frame).unwrap(); }
    assert!(r.scale_display.is_some(), "spatial programs execute in the shared graph");

    assert_eq!(r.scene.as_ref().unwrap().image_cache_bytes(), 0);
    for center in [[510., 255.], [765., 510.], [1020., 769.]] {
        let mut dab = crate::tests::test_dab(center, [0.9, 0.1, 0.2, 1.], 1.);
        dab.radii = [7.; 2];
        let batch = dab_batch(paint, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        let stroke = FramePacket { composite_all: false, dabs: std::slice::from_ref(&dab),
            dab_batches: std::slice::from_ref(&batch), ..frame };
        for renderer in [&mut r, &mut exact] { renderer.submit(stroke).unwrap(); }
        let incremental = display_pixels(&r);
        r.scale_display.as_mut().unwrap().graph = Default::default();
        r.submit(frame).unwrap();
        let full = display_pixels(&r);
        let difference = crate::test_support::max_error(&incremental, &full);
        assert!(difference < 1e-5, "halo at {center:?}: {difference}");
        let error = quality(&full, &pixels(&exact, crate::test_support::document_texture(&exact)), r.scale_display.as_ref().unwrap().plan);
        assert!(error[0] < 0.003 && error[1] < 0.03, "spatial quality at {center:?}: {error:?}");
        assert_eq!(exact_pixels(&mut r, extent), exact_pixels(&mut exact, extent));
    }
    }
}

#[test]
fn spatial_graph_keeps_masks_clipping_global_dependencies_and_scale_preparation() {
    for space in layer_core::BlendSpace::ALL {
    let mut doc = document();
    let extent = [doc.width, doc.height];
    let mut blur = effect(80, "gaussian_blur");
    let mut mask = layer_core::LayerMask::reveal_all(LayerId(81), Default::default());
    mask.default_coverage = 0.4;
    blur.mask = Some(mask);
    blur.opacity = 0.7;
    doc.layers.insert(0, blur);
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    exact.test.reference = true;
    for (level, sigma, clipped) in [(1, 0., false), (2, 0.5, false), (3, 3., true), (4, 21., false), (1, 21., true)] {
        Arc::make_mut(doc.layers[0].effect.as_mut().unwrap()).set("sigma", EffectValue::Number(sigma)).unwrap();
        doc.layers[0].properties.clipped = clipped;
        let mut frame = packet(&doc.layers, extent);
        frame.blend_space = space;
        let scale = 1. / (1 << level) as f32;
        frame.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        for renderer in [&mut r, &mut exact] { renderer.submit(frame).unwrap(); }
        let plan = r.scale_display.as_ref().unwrap().plan;
        let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), plan);
        assert!(error[0] < 0.004 && error[1] < 0.04, "level={level} sigma={sigma} clipped={clipped}: {error:?}");
        assert_eq!(exact_pixels(&mut r, extent), exact_pixels(&mut exact, extent));
        assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        let preparations = r.scene.as_ref().unwrap().effects.preparation_count();
        let work = r.metrics.composited_pixels;
        r.submit(FramePacket { composite_all: false, ..frame }).unwrap();
        assert_eq!(preparations, r.scene.as_ref().unwrap().effects.preparation_count());
        assert_eq!(work, r.metrics.composited_pixels);
    }
    let instance = Arc::make_mut(doc.layers[0].effect.as_mut().unwrap());
    let program = Arc::make_mut(&mut instance.program);
    program.id = "global_probe".into();
    program.wgsl = "fn global_probe(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return fx_sample(fx_extent()-p)*.6+fx_original(p)*.4;}".into();
    program.entry = "global_probe".into();
    program.lookups = Arc::new([]);
    program.passes = vec![layer_core::EffectPass { entry: program.entry.clone(), sampling: layer_core::EffectSampling::Document }].into();
    let mut frame = packet(&doc.layers, extent);
        frame.blend_space = space;
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    for renderer in [&mut r, &mut exact] { renderer.submit(frame).unwrap(); }
    let dab = crate::tests::test_dab([73., 81.], [0.8, 0.1, 0.2, 1.], 1.);
    let batch = dab_batch(doc.layers[1].id, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
    r.submit(FramePacket { composite_all: false, dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
    let incremental = display_pixels(&r);
    r.scale_display.as_mut().unwrap().graph = Default::default();
    r.submit(frame).unwrap();
    assert_eq!(incremental, display_pixels(&r), "global dependencies update pixels far from paint damage");
    }
}
