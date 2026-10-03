use super::*;
use layer_core::{EffectInstance, EffectResolution, EffectValue};

fn hue(values: &[(&str, f32)]) -> Layer {
    let mut effect = EffectInstance::new(crate::tests::fixture("hue_saturation").program());
    for (key, value) in values { effect.set(key, EffectValue::Number(*value)).unwrap(); }
    let mut layer = Layer::paint(LayerId(99), "Hue qualification");
    layer.kind = LayerKind::Effect; layer.effect = Some(Arc::new(effect)); layer
}

#[test]
fn p21_hue_nondefault_ranges_and_colorize_meet_existing_reduced_graph_quality_or_use_native() {
    let cases: &[&[(&str, f32)]] = &[
        &[("reds_hue", 90.), ("reds_saturation", 80.)],
        &[("reds_hue", 180.), ("reds_width", 70.), ("reds_feather", 0.)],
        &[("reds_hue", 180.), ("yellows_hue", -120.), ("reds_center", 30.), ("yellows_center", 30.),
            ("reds_width", 180.), ("yellows_width", 180.), ("reds_feather", 0.), ("yellows_feather", 0.),
            ("reds_saturation", 100.), ("yellows_saturation", 100.), ("reds_lightness", 70.), ("yellows_lightness", 70.)],
        &[("colorize_hue", 120.), ("colorize_saturation", 100.), ("lightness", 25.)],
        &[("reds_hue", 180.), ("reds_width", 0.), ("reds_feather", 30.)],
        &[("reds_hue", 180.), ("reds_width", 180.), ("reds_feather", 90.)],
    ];
    let extent = [1033, 517];
    let mut doc = document_at(extent);
    let paint = doc.layers[0].id;
    let mut window = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap(); exact.test.reference = true;
    for (state, values) in cases.iter().enumerate() {
        doc.layers.retain(|layer| layer.id != LayerId(99));
        let mut adjustment = hue(values);
        if state == 3 { Arc::make_mut(adjustment.effect.as_mut().unwrap()).set("colorize", EffectValue::Toggle(true)).unwrap(); }
        let resolution = adjustment.effect.as_ref().unwrap().program.resolution;
        let mut mask = layer_core::LayerMask::reveal_all(LayerId(98), Default::default());
        mask.initial = Some(layer_core::Selection::polygon(vec![
            layer_core::Point { x: 573., y: 237. }, layer_core::Point { x: 1001., y: 257. },
            layer_core::Point { x: 987., y: 507. }, layer_core::Point { x: 587., y: 479. },
        ]).unwrap());
        adjustment.mask = Some(mask); adjustment.opacity = 0.7; adjustment.properties.clipped = state % 2 == 0;
        doc.layers.insert(0, adjustment);
        for level in [1, 2, 3] {
            let scale = 1. / (1 << level) as f32;
            let mut frame = packet(&doc.layers, extent);
            frame.view.width_px = 43; frame.view.height_px = 25;
            frame.view.document_to_surface = [scale, 0., 0., scale, -610. * scale, -270. * scale];
            window.submit(frame).unwrap(); exact.submit(frame).unwrap();
            let cache = window.scale_display.as_ref().unwrap();
            assert_eq!(cache.plan.level, level);
            assert!(cache.plan.bounds.min_x() > 0 || cache.plan.bounds.min_y() > 0);
            assert!(cache.evaluation == if resolution == EffectResolution::Display { Evaluation::Display } else { Evaluation::Native });
            let error = quality(&display_pixels(&window), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan);
            assert!(error[0] < 0.002 && error[1] < 0.015, "state={state} level={level} {resolution:?} error={error:?}");
            assert_eq!(window.readback_srgb_rgba8().unwrap(), exact.readback_srgb_rgba8().unwrap(), "state={state} level={level}");
            let updates = window.scene.as_ref().unwrap().scale_sources.entries[&paint].updates;
            window.submit(FramePacket { composite_all: false, reset_layers: false, ..frame }).unwrap();
            assert_eq!(updates, window.scene.as_ref().unwrap().scale_sources.entries[&paint].updates);
            assert_presentation_mip(&window);
        }
    }
}

fn float_source(extent: [u32; 2], threshold_edges: bool) -> Arc<layer_core::color::source::SourceImage> {
    use layer_core::color::{ColorProfile, RgbSpace, SampleDepth};
    use layer_core::color::source::{SourceBuilder, SourceChannels, SourceInterpretation};
    let mut builder = SourceBuilder::new(extent, SourceInterpretation {
        channels: SourceChannels::Rgba, depth: SampleDepth::F32,
        profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false,
    }, 64 * 1024 * 1024).unwrap();
    for y in 0..extent[1] {
        let mut row = Vec::with_capacity(extent[0] as usize * 16);
        for x in 0..extent[0] {
            let fx = x as f32 / extent[0] as f32;
            let fy = y as f32 / extent[1] as f32;
            let pixel = if threshold_edges {
                let gray = match (x / 3 + y / 2) % 8 { 0 => -0.125, 1 => 4.,
                    i if i % 2 == 0 => 0.179, _ => 0.181 };
                [gray, gray, gray, 1.]
            } else {
                [-0.125 + 2.125 * fx, 0.15 + 1.4 * fy, 1.1 - 0.8 * fx,
                    match (x / 67 + y / 43) % 3 { 0 => 8e-8, 1 => 0.37, _ => 1. }]
            };
            for value in pixel { row.extend_from_slice(&value.to_le_bytes()); }
        }
        builder.push_row(&row).unwrap();
    }
    Arc::new(builder.finish().unwrap())
}

fn candidate_errors(name: &str) -> [f32; 2] {
    use layer_core::color::{RgbColor, RgbSpace, SampleDepth};
    let extent = [1033, 517];
    let hdr = float_source(extent, name == "threshold");
    let mut worst = [0_f32; 2];
    for space in RgbSpace::ALL {
        let mut doc = document_at(extent);
        doc.color.space = space; doc.color.depth = SampleDepth::F32;
        let mut window = WgpuRasterizer::new_native_headless(doc.color).unwrap();
        let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap(); exact.test.reference = true;
        for source in 0..2 {
            doc.layers.retain(|layer| layer.id != LayerId(99));
            doc.layers[0].source = Some(if source == 1 { hdr.clone() }
                else if name == "threshold" { layer_core::color::source::rgba8_source(extent, |x, y| {
                    let gray = if (x / 3 + y / 2) % 2 == 0 { 126 } else { 130 }; [gray, gray, gray, 255]
                }) } else { document_at(extent).layers[0].source.as_ref().unwrap().clone() });
            let states = if name == "photo_filter" { 3 } else { 1 };
            for state in 0..states {
                doc.layers.retain(|layer| layer.id != LayerId(99));
                let program = crate::tests::fixture(name).program().for_depth(SampleDepth::F32);
                let mut program = (*program).clone(); program.resolution = EffectResolution::Display;
                let mut effect = EffectInstance::new(Arc::new(program));
                if name == "threshold" {
                    let linear = if source == 1 { 0.18 } else { RgbSpace::Srgb.decode(128. / 255.) };
                    effect.set("threshold", EffectValue::Number(space.encode(linear) as f32)).unwrap();
                }
                if name == "photo_filter" && state > 0 {
                    effect.set("density", EffectValue::Number(100.)).unwrap();
                    effect.set("preserve_luminance", EffectValue::Toggle(state == 1)).unwrap();
                    effect.set("color", EffectValue::Color(RgbColor::new(RgbSpace::DisplayP3, [0.9, 0.2, 0.15, 1.]).unwrap())).unwrap();
                }
                let mut adjustment = Layer::paint(LayerId(99), "Display candidate");
                adjustment.kind = LayerKind::Effect; adjustment.effect = Some(Arc::new(effect));
                if source == 0 {
                    let mut mask = layer_core::LayerMask::reveal_all(LayerId(98), Default::default());
                    mask.initial = Some(layer_core::Selection::polygon(vec![
                        layer_core::Point { x: 573., y: 237. }, layer_core::Point { x: 1001., y: 257. },
                        layer_core::Point { x: 987., y: 507. }, layer_core::Point { x: 587., y: 479. },
                    ]).unwrap());
                    adjustment.mask = Some(mask); adjustment.opacity = 0.7; adjustment.properties.clipped = true;
                }
                doc.layers.insert(0, adjustment);
                for level in [1, 2, 3] {
                    let scale = 1. / (1 << level) as f32;
                    let mut frame = packet(&doc.layers, extent);
                    frame.view.width_px = 43; frame.view.height_px = 25;
                    frame.view.document_to_surface = [scale, 0., 0., scale, -610. * scale, -270. * scale];
                    window.submit(frame).unwrap(); exact.submit(frame).unwrap();
                    let cache = window.scale_display.as_ref().unwrap();
                    assert_eq!(cache.plan.level, level); assert!(cache.evaluation == Evaluation::Display);
                    let error = quality(&display_pixels(&window), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan);
                    for i in 0..2 { worst[i] = worst[i].max(error[i]); }
                    println!("{name} {space:?} source={source} state={state} level={level} error={error:?}");
                    assert!(error.iter().all(|v| v.is_finite()));
                    assert_eq!(window.readback_srgb_rgba8().unwrap(), exact.readback_srgb_rgba8().unwrap());
                }
            }
        }
    }
    println!("{name} worst mean={} p99={} eligible={}", worst[0], worst[1], worst[0] < 0.002 && worst[1] < 0.015);
    worst
}

#[test]
fn p21_invert_display_candidate_qualification() {
    assert_eq!(crate::tests::fixture("invert").program().resolution, EffectResolution::Display);
    let error = candidate_errors("invert");
    assert!(error[0] < 0.002 && error[1] < 0.015, "{error:?}");
}
#[test]
fn p21_desaturate_display_candidate_qualification() {
    assert_eq!(crate::tests::fixture("desaturate").program().resolution, EffectResolution::Display);
    let error = candidate_errors("desaturate");
    assert!(error[0] < 0.002 && error[1] < 0.015, "{error:?}");
}
#[test]
fn p21_threshold_display_candidate_qualification() {
    assert_eq!(crate::tests::fixture("threshold").program().resolution, EffectResolution::Native);
    let error = candidate_errors("threshold");
    assert!(error[0] >= 0.002 || error[1] >= 0.015, "edge fixture must reject reduced threshold: {error:?}");
}
#[test]
fn p21_photo_filter_display_candidate_qualification() {
    assert_eq!(crate::tests::fixture("photo_filter").program().resolution, EffectResolution::Display);
    let error = candidate_errors("photo_filter");
    assert!(error[0] < 0.002 && error[1] < 0.015, "{error:?}");
}

#[test]
fn resident_native_pointwise_batches_preserve_odd_edges_masks_clipping_and_windows() {
    for (preload, admitted) in [(true, true), (false, true), (false, false)] {
        native_pointwise_batches(preload, admitted);
    }
}

fn native_pointwise_batches(preload: bool, admitted: bool) {
    let extent = [1795, 773];
    let mut doc = document_at(extent);
    doc.layers.truncate(1);
    doc.layers[0].source = Some(rgba8_source(extent, |x, y| [
        ((x * 3 + y * 7) % 256) as u8, ((y * 5 + x / 17) % 256) as u8,
        ((x / 5 + y / 3) % 256) as u8, 255,
    ]));
    let mut effect = Layer::paint(LayerId(99), "Resident native Threshold");
    effect.kind = LayerKind::Effect;
    effect.effect = Some(Arc::new(EffectInstance::new(crate::tests::fixture("threshold").program())));
    let mut cached = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.color).unwrap(); exact.test.reference = true;
    cached.native_edit.as_mut().unwrap().display_complete_bytes = if admitted { u64::MAX } else { 0 };
    if preload {
        let mut loaded = packet(&doc.layers, extent);
        loaded.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        cached.submit(loaded).unwrap();
        for _ in 0..32 {
            if !cached.has_pending_work() { break; }
            cached.wait_idle().unwrap();
            cached.submit(FramePacket { composite_all: false, reset_layers: false, ..loaded }).unwrap();
        }
        assert!(cached.scale_display.as_ref().unwrap().hierarchy.is_some(), "the loaded source must earn resident hierarchy admission");
    }
    doc.layers.insert(0, effect);
    let mut source_bytes = None;
    for (state, threshold) in [0.31, 0.47, 0.63, 0.38].into_iter().enumerate() {
        Arc::make_mut(doc.layers[0].effect.as_mut().unwrap()).set("threshold", EffectValue::Number(threshold)).unwrap();
        if state == 2 {
            let mut mask = layer_core::LayerMask::reveal_all(LayerId(98), layer_core::Point { x: 11., y: -7. });
            mask.initial = Some(layer_core::Selection::polygon(vec![
                layer_core::Point { x: 109., y: 37. }, layer_core::Point { x: 1781., y: 91. },
                layer_core::Point { x: 1589., y: 767. }, layer_core::Point { x: 7., y: 599. },
            ]).unwrap());
            doc.layers[0].mask = Some(mask); doc.layers[0].opacity = 0.61; doc.layers[0].properties.clipped = true;
        }
        let mut frame = packet(&doc.layers, extent);
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        if state == 3 {
            frame.view.width_px = 43; frame.view.height_px = 25;
            frame.view.document_to_surface = [0.25, 0., 0., 0.25, -810. * 0.25, -410. * 0.25];
        }
        cached.submit(frame).unwrap(); exact.submit(packet(&doc.layers, extent)).unwrap();
        let cache = cached.scale_display.as_ref().unwrap();
        assert_eq!(cache.hierarchy.is_some(), admitted);
        assert_eq!(matches!(cache.pixels, hierarchy::Pixels::Resident { .. }), admitted);
        assert!(!cache.has_pending_work(&cached), "full exact native updates must leave no unnecessary refinement: preload={preload} admitted={admitted} state={state} plan={:?}", cache.plan);
        let reference = pixels(&exact, crate::test_support::document_texture(&exact));
        let error = quality(&display_pixels(&cached), &reference, cache.plan);
        assert!(error[2] < 2e-5, "state={state} resident output error={error:?}");
        if state == 3 {
            assert!(frame.view.document_to_surface[4] < 0. && frame.view.document_to_surface[5] < 0.);
            assert_presentation_mip(&cached);
        }
        if state < 2 {
            let independent: Vec<_> = (0..extent[0] * extent[1]).map(|i| {
                let x = i as u32 % extent[0]; let y = i as u32 / extent[0];
                let codes = [(x * 3 + y * 7) % 256, (y * 5 + x / 17) % 256, (x / 5 + y / 3) % 256];
                let luminance: f64 = codes.into_iter().zip(doc.color.space.to_xyz()[1])
                    .map(|(code, weight)| f64::from(code) / 255. * weight).sum();
                let value = if luminance >= f64::from(threshold) { 1. } else { 0. };
                [value, value, value, 1.]
            }).collect();
            if let Some(hierarchy) = &cache.hierarchy {
                assert_eq!(pixels(&cached, &hierarchy.root().texture), independent);
            }
            let error = quality(&display_pixels(&cached), &independent, cache.plan);
            assert!(error[2] < 2e-5, "preload={preload} admitted={admitted} state={state} independent error={error:?}");
        }
        if state == 1 || (state == 0 && !preload && admitted) {
            let pages = extent.map(|n| n.div_ceil(PAGE_SIZE)).into_iter().product::<u32>();
            let passes = cached.scene.as_ref().unwrap().effect_passes;
            eprintln!("resident pointwise native pages={pages} effect_passes={passes}");
            if admitted { assert!(passes <= u64::from(pages.div_ceil(SOURCE_SLOTS as u32)), "warm direct-output effects must batch: {passes} for {pages} pages"); }
        }
        let bytes = cached.source_tiles.borrow().gpu_bytes();
        if let Some(expected) = source_bytes { assert_eq!(bytes, expected); } else { source_bytes = Some(bytes); }
    }
}
