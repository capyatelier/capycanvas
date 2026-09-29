use super::*;
use layer_core::{Affine, EffectInstance, EffectValue, Point};

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
    assert_settled(&mut r, frame, &pixels(&exact, exact.composite_texture.as_ref().unwrap()));
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
    let error = quality(&animated, &pixels(&exact, exact.composite_texture.as_ref().unwrap()), r.scale_display.as_ref().unwrap().plan);
    assert!(error[2] < 0.002, "animated display {error:?}");
}

#[test]
fn idle_display_refines_placement_windows_and_reuses_exact_overlap() {
    for space in layer_core::BlendSpace::ALL {
        let mut doc = document_at([1541, 771]);
        let extent = [doc.width, doc.height];
        doc.layers[0].properties.placement = Affine::around(Point { x: 770., y: 385. }, [0.9, 0.8], 0.17, Point::default());
        let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
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
            assert_settled(&mut r, frame, &pixels(&exact, exact.composite_texture.as_ref().unwrap()));
        }
        frame.view.width_px = extent[0];
        frame.view.height_px = extent[1];
        r.submit(frame).unwrap();
        assert!(r.scale_display.as_ref().unwrap().placed.is_some());
        assert_settled(&mut r, frame, &pixels(&exact, exact.composite_texture.as_ref().unwrap()));
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
        assert_settled(&mut r, frame, &pixels(&exact, exact.composite_texture.as_ref().unwrap()));
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
        assert_settled(&mut r, frame, &pixels(&exact, exact.composite_texture.as_ref().unwrap()));
        let dab = crate::tests::test_dab([254., 128.], [0.9, 0.1, 0.3, 1.], 0.8);
        let batch = dab_batch(paint, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        let stroke = FramePacket { composite_all: false, dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame };
        r.submit(stroke).unwrap(); exact.submit(stroke).unwrap();
        assert_settled(&mut r, frame, &pixels(&exact, exact.composite_texture.as_ref().unwrap()));
    }
}
