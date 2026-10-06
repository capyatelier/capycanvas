use super::*;
use layer_core::{EffectInstance, EffectResolution, EffectValue};

fn hue(values: &[(&str, f32)]) -> EffectInstance {
    let mut effect = EffectInstance::new(crate::tests::fixture("hue_saturation").program());
    for (key, value) in values { effect.set(key, EffectValue::Number(*value)).unwrap(); }
    effect
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
    let paint = primary_target(&doc);
    let mut adjustment_handle = None;
    let mut window = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap(); exact.test.reference = true;
    for (state, values) in cases.iter().enumerate() {
        if let Some(handle) = adjustment_handle.take() { remove_occurrence(&mut doc, handle); }
        let mut adjustment = hue(values);
        if state == 3 { adjustment.set("colorize", EffectValue::Toggle(true)).unwrap(); }
        let resolution = adjustment.program.resolution;
        let handle = effect_occurrence(&mut doc, adjustment, "Hue qualification");
        coverage_mask(&mut doc, handle, [0, 0], Some(layer_core::Selection::polygon(vec![
            layer_core::Point { x: 573., y: 237. }, layer_core::Point { x: 1001., y: 257. },
            layer_core::Point { x: 987., y: 507. }, layer_core::Point { x: 587., y: 479. },
        ]).unwrap()));
        let occurrence = doc.artwork.occurrences.get_mut(handle).unwrap();
        occurrence.opacity = 0.7; set_attachment(occurrence, state % 2 == 0);
        insert_occurrence(&mut doc, handle, 0);
        adjustment_handle = Some(handle);
        for level in [1, 2, 3] {
            let scale = 1. / (1 << level) as f32;
            let mut frame = packet(doc.scene(), extent);
            frame.view.width_px = 43; frame.view.height_px = 25;
            frame.view.document_to_surface = [scale, 0., 0., scale, -610. * scale, -270. * scale];
            window.submit(frame).unwrap(); exact.submit(frame).unwrap();
            let cache = window.scale_display.as_ref().unwrap();
            assert_eq!(cache.plan.level, level);
            assert!(cache.plan.bounds.min_x() > 0 || cache.plan.bounds.min_y() > 0);
            assert!(cache.evaluation == if resolution == EffectResolution::Display { Evaluation::Display } else { Evaluation::Native });
            let error = quality(&display_pixels(&window), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan);
            assert!(error[0] < 0.002 && error[1] < 0.015, "state={state} level={level} {resolution:?} error={error:?}");
            crate::test_support::assert_same_canonical(&mut window, &mut exact);
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
        let mut adjustment_handle = None;
        let root = doc.artwork.root;
        doc.artwork.compositions.get_mut(root).unwrap().color.space = space;
        doc.artwork.compositions.get_mut(root).unwrap().color.depth = SampleDepth::F32;
        let mut window = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap(); exact.test.reference = true;
        for source in 0..2 {
            if let Some(handle) = adjustment_handle.take() { remove_occurrence(&mut doc, handle); }
            let paint = primary_paint(&doc);
            let original = if source == 1 { hdr.clone() }
                else if name == "threshold" { layer_core::color::source::rgba8_source(extent, |x, y| {
                    let gray = if (x / 3 + y / 2) % 2 == 0 { 126 } else { 130 }; [gray, gray, gray, 255]
                }) } else { let base = document_at(extent); base.artwork.paint.get(primary_paint(&base)).unwrap().base.as_ref().unwrap().image.storage().clone() };
            doc.artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::authored::PaintBase::new((original).into()));
            let states = match name { "photo_filter" => 3, "selective_color" | "channel_mixer" => 5, _ => 1 };
            for state in 0..states {
                if let Some(handle) = adjustment_handle.take() { remove_occurrence(&mut doc, handle); }
                let program = crate::tests::fixture(name).program();
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
                if matches!(name, "selective_color" | "channel_mixer") { mixing_state(&mut effect, name, state); }
                let handle = effect_occurrence(&mut doc, effect, "Display candidate");
                if source == 0 {
                    coverage_mask(&mut doc, handle, [0, 0], Some(layer_core::Selection::polygon(vec![
                        layer_core::Point { x: 573., y: 237. }, layer_core::Point { x: 1001., y: 257. },
                        layer_core::Point { x: 987., y: 507. }, layer_core::Point { x: 587., y: 479. },
                    ]).unwrap()));
                    let occurrence = doc.artwork.occurrences.get_mut(handle).unwrap();
                    occurrence.opacity = 0.7; set_attachment(occurrence, true);
                }
                insert_occurrence(&mut doc, handle, 0);
                adjustment_handle = Some(handle);
                for level in [1, 2, 3] {
                    let scale = 1. / (1 << level) as f32;
                    let mut frame = packet(doc.scene(), extent);
                    frame.view.width_px = 43; frame.view.height_px = 25;
                    frame.view.document_to_surface = [scale, 0., 0., scale, -610. * scale, -270. * scale];
                    window.submit(frame).unwrap(); exact.submit(frame).unwrap();
                    let cache = window.scale_display.as_ref().unwrap();
                    assert_eq!(cache.plan.level, level); assert!(cache.evaluation == Evaluation::Display);
                    let error = quality(&display_pixels(&window), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan);
                    for i in 0..2 { worst[i] = worst[i].max(error[i]); }
                    println!("{name} {space:?} source={source} state={state} level={level} error={error:?}");
                    assert!(error.iter().all(|v| v.is_finite()));
                    crate::test_support::assert_same_canonical(&mut window, &mut exact);
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

fn mixing_state(effect: &mut EffectInstance, name: &str, state: usize) {
    if name == "selective_color" {
        let mild = [[0.; 4], [0.; 4], [0.; 4], [0.; 4], [0.; 4], [0.; 4], [0., 0., 8., 0.], [2., -2., 6., 0.], [0.; 4]];
        let strong = [[45., -25., 10., 20.], [-25., 35., 40., 10.], [35., 15., -35., 20.],
            [30., -30., 35., 15.], [-35., 35., 15., 20.], [40., 25., -35., 15.],
            [15., -10., 20., 15.], [-15., 20., 10., -15.], [20., -20., 15., 30.]];
        let values = match state { 0 | 1 => mild, 2 | 3 => strong, _ => [[100., -100., 100., 75.]; 9] };
        for (family, row) in ["reds", "yellows", "greens", "cyans", "blues", "magentas", "whites", "neutrals", "blacks"].into_iter().zip(values) {
            for (component, value) in ["cyan", "magenta", "yellow", "black"].into_iter().zip(row) {
                effect.set(&format!("{family}_{component}"), EffectValue::Number(value)).unwrap();
            }
        }
        effect.set("mode", EffectValue::Choice(u32::from(state % 2 == 1))).unwrap();
    } else {
        let rows = match state {
            0 => [[0., 100., 0., 0.], [0., 0., 100., 0.], [100., 0., 0., 0.]],
            1 => [[80., -20., 50., 10.], [-30., 120., 20., -15.], [25., 35., 70., 5.]],
            _ => [[-125., 80., 145., 35.], [200., -200., 0., -100.], [0., 0., 0., 75.]],
        };
        for (output, row) in ["red", "green", "blue"].into_iter().zip(rows) {
            for (input, value) in ["red", "green", "blue", "constant"].into_iter().zip(row) {
                effect.set(&format!("{output}_{input}"), EffectValue::Number(value)).unwrap();
            }
        }
        effect.set("monochrome", EffectValue::Toggle(state >= 3)).unwrap();
        if state == 4 {
            for (input, value) in ["red", "green", "blue", "constant"].into_iter().zip([150., -75., 25., -20.]) {
                effect.set(&format!("gray_{input}"), EffectValue::Number(value)).unwrap();
            }
        }
    }
}

#[test]
fn selective_color_display_candidate_qualification() {
    let error = candidate_errors("selective_color");
    let eligible = error[0] < 0.002 && error[1] < 0.015;
    assert!(eligible, "Display Selective Color must earn its approximation: {error:?}");
    assert_eq!(crate::tests::fixture("selective_color").program().resolution, EffectResolution::Display);
}

#[test]
fn channel_mixer_display_candidate_qualification() {
    let error = candidate_errors("channel_mixer");
    let eligible = error[0] < 0.002 && error[1] < 0.015;
    assert!(eligible, "Display Channel Mixer must earn its approximation: {error:?}");
    assert_eq!(crate::tests::fixture("channel_mixer").program().resolution, EffectResolution::Display);
}

#[test]
fn resident_native_pointwise_batches_preserve_odd_edges_masks_clipping_and_windows() {
    for (preload, admitted) in [(true, true), (false, true), (false, false)] {
        native_pointwise_batches(preload, admitted);
    }
}

fn native_pointwise_batches(preload: bool, admitted: bool) {
    let extent = [if admitted {1795} else {4355}, 773];
    let mut doc = document_at(extent);
    let paint = primary_paint(&doc);
    let stack = doc.composition().result;
    doc.artwork.stacks.get_mut(stack).unwrap().entries.truncate(1); reindex(&mut doc);
    doc.artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::authored::PaintBase::new((rgba8_source(extent, |x, y| [
        ((x * 3 + y * 7) % 256) as u8, ((y * 5 + x / 17) % 256) as u8,
        ((x / 5 + y / 3) % 256) as u8, 255,
    ])).into()));
    let effect = effect_occurrence(&mut doc, EffectInstance::new(crate::tests::fixture("threshold").program()), "Resident native Threshold");
    let mut cached = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    cached.test.exact_display = true;
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap(); exact.test.reference = true;
    cached.native_edit.as_mut().unwrap().display_complete_bytes = if admitted { u64::MAX } else { 0 };
    if preload {
        let mut loaded = packet(doc.scene(), extent);
        loaded.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        cached.submit(loaded).unwrap();
        for _ in 0..32 {
            if !cached.has_pending_work() { break; }
            cached.wait_idle().unwrap();
            cached.submit(FramePacket { composite_all: false, reset_layers: false, ..loaded }).unwrap();
        }
        assert!(cached.scale_display.as_ref().unwrap().hierarchy.is_some(), "the loaded source must earn resident hierarchy admission");
    }
    insert_occurrence(&mut doc, effect, 0);
    let mut source_bytes = None;
    for (state, threshold) in [0.31, 0.47, 0.63, 0.38].into_iter().enumerate() {
        set_effect_value(&mut doc, effect, "threshold", EffectValue::Number(threshold));
        if state == 2 {
            coverage_mask(&mut doc, effect, [11, -7], Some(layer_core::Selection::polygon(vec![
                layer_core::Point { x: 109., y: 37. }, layer_core::Point { x: (extent[0]-14) as f32, y: 91. },
                layer_core::Point { x: (extent[0]-206) as f32, y: 767. }, layer_core::Point { x: 7., y: 599. },
            ]).unwrap()));
            let occurrence = doc.artwork.occurrences.get_mut(effect).unwrap();
            occurrence.opacity = 0.61; set_attachment(occurrence, true);
            reindex(&mut doc);
        }
        let mut frame = packet(doc.scene(), extent);
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        if state == 3 {
            frame.view.width_px = 43; frame.view.height_px = 25;
            frame.view.document_to_surface = [0.25, 0., 0., 0.25, -(extent[0] as f32-305.) * 0.25, -410. * 0.25];
        }
        let command_passes = cached.metrics.command_passes;
        cached.submit(frame).unwrap();
        let command_passes = cached.metrics.command_passes - command_passes;
        exact.submit(packet(doc.scene(), extent)).unwrap();
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
                let luminance: f64 = codes.into_iter().zip(doc.composition().color.space.to_xyz()[1])
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
            else {
                let [columns,rows]=extent.map(|n|n.div_ceil(PAGE_SIZE));
                let batches = u64::from(rows*(columns.div_ceil(SOURCE_SLOTS as u32)+1));
                assert!(passes<=batches,
                    "bounded working strips must batch native effects: {passes} for {columns}x{rows} pages");
                eprintln!("native working strip command_passes={command_passes} effect_passes={passes} permitted_batches={batches}");
                assert!(command_passes <= 3 * batches + 8,
                    "working-strip composition and both display reductions must batch: {command_passes} command passes for {pages} pages");
            }
        }
        if !admitted {assert_eq!(cache.exact_tile.as_ref().unwrap().texture.size().width,PAGE_SIZE*SOURCE_SLOTS as u32);
            assert_eq!(cache.exact_tile.as_ref().unwrap().texture.size().height,PAGE_SIZE);}
        let bytes = cached.source_tiles.borrow().gpu_bytes();
        if let Some(expected) = source_bytes { assert_eq!(bytes, expected); } else { source_bytes = Some(bytes); }
    }
    if !admitted {
        let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
        frame.view.document_to_surface=[0.25,0.,0.,0.25,0.,0.];
        cached.submit(frame).unwrap();exact.submit(frame).unwrap();
        let strip=cached.scale_display.as_ref().unwrap().exact_tile.as_ref().unwrap().texture.clone();
        for positions in [[[73.,67.],[4117.,613.]],[[4117.,613.],[73.,67.]]] {
            let dabs=positions.map(|point|crate::tests::test_dab(point,[0.91,0.12,0.67,1.],0.8));
            let mut batch=dab_batch(SourceTarget::Paint(paint),crate::layer_tests::preset_style(DefaultBrushPreset::GPen),dabs[0].bounds().union(dabs[1].bounds()));
            batch.dab_count=2;
            let stroke=FramePacket{dabs:&dabs,dab_batches:std::slice::from_ref(&batch),..frame};
            let work=cached.metrics.composited_pixels;
            let reverse=cached.scene.as_ref().unwrap().native_reverse;
            cached.submit(stroke).unwrap();exact.submit(stroke).unwrap();
            assert_ne!(cached.scene.as_ref().unwrap().native_reverse,reverse);
            let cache=cached.scale_display.as_ref().unwrap();
            assert!(cache.hierarchy.is_none());assert!(!cache.has_pending_work(&cached));
            assert_eq!(cache.exact_tile.as_ref().unwrap().texture,strip);
            let error=quality(&display_pixels(&cached),&pixels(&exact,crate::test_support::document_texture(&exact)),cache.plan);
            assert!(error[2]<2e-5,"sparse disjoint working-strip contacts {positions:?}: {error:?}");
            assert!(cached.metrics.composited_pixels-work<=4*u64::from(PAGE_SIZE).pow(2),"sparse contacts recompose only affected native pages");
        }
    }
}
