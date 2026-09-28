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
    for level in [1, 2, 3] {
        let mut updates = None;
        for state in 0..4 {
            Arc::make_mut(doc.layers[0].effect.as_mut().unwrap()).set("exposure", EffectValue::Number(state as f32 * 0.1)).unwrap();
            doc.layers[1].properties.clipped = state % 2 == 0;
            doc.layers[1].mask.as_mut().unwrap().inverted = state >= 2;
            doc.layers[1].mask.as_mut().unwrap().offset.x = if state == 3 { 17. } else { 0. };
            let mut frame = packet(&doc.layers, extent);
            let scale = 1. / (1 << level) as f32;
            frame.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
            r.submit(frame).unwrap(); exact.submit(frame).unwrap();
            let work = r.scene.as_ref().unwrap().scale_sources.entries[&paint].updates;
            if let Some(previous) = updates { assert_eq!(work, previous, "filter edits reuse unchanged sources"); }
            updates = Some(work);
            let plan = r.scale_display.as_ref().expect("qualified effects use graph composition").plan;
            assert_eq!(plan.level, level);
            assert!(r.live_display.is_none() && r.composite_texture.is_none());
            assert_eq!(r.scene.as_ref().unwrap().image_cache_bytes(), 0);
            let error = quality(&display_pixels(&r), &pixels(&exact, exact.composite_texture.as_ref().unwrap()), plan);
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
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    frame.composite_all = false;
    frame.dabs = std::slice::from_ref(&dab);
    frame.dab_batches = std::slice::from_ref(&batch);
    r.submit(frame).unwrap(); exact.submit(frame).unwrap();
    let plan = r.scale_display.as_ref().unwrap().plan;
    let error = quality(&display_pixels(&r), &pixels(&exact, exact.composite_texture.as_ref().unwrap()), plan);
    assert!(error[0] < 0.002 && error[1] < 0.015, "partial paint error={error:?}");
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
            let scale = 1. / (1 << level) as f32;
            frame.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
            r.submit(frame).unwrap();
            assert_eq!(r.scale_display.as_ref().unwrap().plan.level, level);
            assert!(r.live_display.is_none() && r.composite_texture.is_none());
            display_pixels(&r)
        };
        let mut flat = doc.layers.clone();
        flat.retain(|l| l.kind != LayerKind::Group);
        for layer in &mut flat { layer.properties.parent = None; }
        let expected = draw(&flat);
        let full = draw(&doc.layers);
        let error = |a: &[[f32; 4]], b: &[[f32; 4]]| a.iter().flatten().zip(b.iter().flatten())
            .map(|(a, b)| (a - b).abs()).fold(0., f32::max);
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
