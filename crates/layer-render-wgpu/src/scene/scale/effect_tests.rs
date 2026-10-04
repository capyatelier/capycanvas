use super::*;
use layer_core::{EffectInstance, EffectResolution, EffectValue};

fn effect(doc: &mut Document, name: &str) -> OccurrenceHandle {
    effect_occurrence(doc,EffectInstance::new(crate::tests::fixture(name).program()),name)
}

fn effect_program_mut(doc: &mut Document, handle: OccurrenceHandle) -> &mut layer_core::EffectProgram {
    let application=effect_handle(doc,handle);let definition=doc.artwork.effects.get(application).unwrap().definition;
    Arc::make_mut(&mut doc.artwork.definitions.get_mut(definition).unwrap().program)
}
fn effect_program_at_mut(doc: &mut Document,index:usize)->&mut layer_core::EffectProgram {
    let handle=doc.scene().order()[index];effect_program_mut(doc,handle)
}
fn set_effect_at(doc: &mut Document,index:usize,key:&str,value:EffectValue){
    let handle=doc.scene().order()[index];set_effect_value(doc,handle,key,value);
}

fn exact_pixels(r: &mut WgpuRasterizer) -> Vec<u8> {
    r.readback_srgb_rgba8().unwrap()
}

fn blur_chain(doc: &mut layer_core::Document) {
    for (_, sigma) in [(80, 9.), (81, 21.), (82, 13.)] {
        let blur = effect(doc,"gaussian_blur");
        set_effect_value(doc,blur,"sigma", EffectValue::Number(sigma));
        insert_occurrence(doc,blur,0);
    }
}

fn assert_window_matches_full(window: &WgpuRasterizer, full: &WgpuRasterizer, context: impl std::fmt::Debug) {
    let plan = window.scale_display.as_ref().unwrap().plan;
    let full_plan = full.scale_display.as_ref().unwrap().plan;
    assert_eq!(plan.level, full_plan.level);
    assert_eq!(full_plan.bounds, PixelRect::full(full_plan.extent));
    let actual = display_pixels(window);
    let expected = display_pixels(full);
    let origin = [plan.bounds.min_x() >> plan.level, plan.bounds.min_y() >> plan.level];
    for (i, pixel) in actual.iter().enumerate() {
        let x = origin[0] + i as u32 % plan.size[0];
        let y = origin[1] + i as u32 / plan.size[0];
        let reference = expected[(y * full_plan.size[0] + x) as usize];
        assert!(pixel.iter().zip(reference).all(|(a, b)| (a - b).abs() < 2e-5),
            "{context:?} level={} at [{x}, {y}]: {pixel:?} != {reference:?}", plan.level);
    }
}

fn assert_spatial_storage_reserved(r: &WgpuRasterizer, frame: FramePacket<'_>) {
    let cache = r.scale_display.as_ref().unwrap();
    let input = input_plan(cache.plan, frame.scene);
    let images = graph::scratch_images(frame, cache.plan.level, r.device.working_space()).unwrap();
    let reserved = input.level_bytes(input.level) * (images - 1);
    let actual = cache.output.iter().map(|image| texture_bytes(&image.texture)).sum::<u64>();
    assert!(actual <= reserved, "spatial scratch storage={actual}, reservation={reserved}, images={images}");
    let resident = resident_bytes_with_pool(cache, r.scene.as_ref().unwrap());
    assert!(resident <= CACHE_BYTES, "spatial resident storage={resident}");
}

#[test]
fn attached_finite_support_preserves_two_distant_contact_pages() {
    let extent = [2048; 2];
    let mut doc = document_at(extent);
    let paint = source_at(&doc, 0);
    let blur = effect(&mut doc, "gaussian_blur");
    set_effect_value(&mut doc, blur, "sigma", EffectValue::Number(2.));
    let program = effect_program_mut(&mut doc, blur);
    let count = program.passes.len() as u32;
    assert_eq!(16 % count, 0);
    for pass in Arc::make_mut(&mut program.passes) {
        pass.sampling = layer_core::EffectSampling::Neighborhood { radius: 16 / count };
    }
    doc.artwork.occurrences.get_mut(blur).unwrap().attachment = Attachment::Effect;
    insert_occurrence(&mut doc, blur, 0);
    assert_eq!(crate::effects::damage_radius(doc.scene().effect(blur).unwrap(), 0), Some(16));
    let dabs = [[128., 128.], [1664., 1664.]].map(|position| crate::tests::test_dab(position, [0.9, 0.1, 0.3, 1.], 0.8));
    let mut batch = dab_batch(paint, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dabs[0].bounds().union(dabs[1].bounds()));
    batch.dab_count = dabs.len() as u32;
    for level in [0, 2] {
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let scale = 1. / (1 << level) as f32;
        let mut frame = packet(doc.scene(), extent);
        frame.composite_all = false;
        frame.view.width_px = extent[0]; frame.view.height_px = extent[1];
        frame.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        r.submit(frame).unwrap();
        let work = r.metrics.composited_pixels;
        r.submit(FramePacket { dabs: &dabs, dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
        assert_eq!(r.metrics.composited_pixels - work, 2 * u64::from(PAGE_SIZE >> level).pow(2),
            "support expands each contact independently at level {level}");
        let mut pages = r.metrics.frame_composited_pages.clone();pages.sort_unstable();
        assert_eq!(pages, vec![(level, [0, 0]), (level, [6, 6])]);
        let incremental = display_pixels(&r);
        r.scale_display = None;
        r.submit(FramePacket { composite_all: true, ..frame }).unwrap();
        assert_eq!(display_pixels(&r), incremental, "sparse spatial output matches a full rebuild at level {level}");
    }
}

#[test]
fn finite_radius_default_tablet_view_admits_reduced_composition_with_reserved_scratch() {
    let mut doc = document_at([6000, 4000]);
    let ink=paint_occurrence(&mut doc,"empty ink",None);insert_occurrence(&mut doc,ink,0);
    let blur = effect(&mut doc,"gaussian_blur");
    set_effect_value(&mut doc,blur,"sigma", EffectValue::Number(3.));
    insert_occurrence(&mut doc,blur,0);
    assert_eq!(doc.scene().order().len(), 3);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view.width_px = 2200;
    frame.view.height_px = 1440;
    frame.view.document_to_surface = [0.5, 0., 0., 0.5, -341., -207.];
    let request = request(&r, frame).unwrap();
    assert!(request.evaluation == Evaluation::Display);
    assert_eq!(request.plan.level, 1);
    r.submit(frame).unwrap();
    assert!(r.scale_display.as_ref().unwrap().evaluation == Evaluation::Display);
    assert_spatial_storage_reserved(&r, frame);
}

#[test]
fn finite_radius_effects_admit_large_reduced_windows_and_global_effects_keep_full_bounds() {
    let mut doc = document_at([65, 33]);
    blur_chain(&mut doc);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    for extent in [[6000, 4000], [9504, 6336]] {
        composition_mut(&mut doc).size=extent;
        let mut frame = packet(doc.scene(), extent);
        frame.view.width_px = 960;
        frame.view.height_px = 640;
        frame.view.document_to_surface = [0.5, 0., 0., 0.5, -1200., -800.];
        let plan = view_plan(frame, 1, Evaluation::Display).unwrap();
        assert!(plan.bounds.area() < PixelRect::full(extent).area() / 2);
        let request = request(&r, frame).unwrap();
        assert_eq!(request.plan.level, 1);
        assert!(request.evaluation == Evaluation::Display, "{extent:?} finite-radius windows stay reduced");
        assert!(allocation(&r, plan, frame, None).into_iter().sum::<u64>() <= CACHE_BYTES);
        r.submit(frame).unwrap();
        let cache = r.scale_display.as_ref().unwrap();
        assert!(cache.evaluation == Evaluation::Display);
        let bytes = resident_bytes_with_pool(cache, r.scene.as_ref().unwrap());
        assert!(bytes <= CACHE_BYTES, "{extent:?} resident storage={bytes}");
        assert_spatial_storage_reserved(&r, frame);
    }
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view.width_px = 512;
    frame.view.height_px = 384;
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, -1200., -800.];
    let plan = view_plan(frame, 2, Evaluation::Display).unwrap();
    assert!(plan.bounds.area() < PixelRect::full(plan.extent).area() / 8);
    let request = request(&r, frame).unwrap();
    assert_eq!(request.plan.level, 2);
    assert!(request.evaluation == Evaluation::Display);
    assert!(allocation(&r, plan, frame, None).into_iter().sum::<u64>() <= CACHE_BYTES);
    let view = frame.view;

    let program = effect_program_at_mut(&mut doc,0);
    program.passes = vec![layer_core::EffectPass {
        entry: program.entry.clone(), sampling: layer_core::EffectSampling::Document,
    }].into();
    let frame = FramePacket { view, ..packet(doc.scene(), doc.composition().size) };
    assert_eq!(view_plan(frame, 2, Evaluation::Display).unwrap().bounds, PixelRect::full(plan.extent));
    occurrence_mut(&mut doc,0).visible = false;
    let frame = FramePacket { view, ..packet(doc.scene(), doc.composition().size) };
    assert!(view_plan(frame, 2, Evaluation::Display).unwrap().bounds.area() < PixelRect::full(plan.extent).area() / 8);
}

#[test]
fn finite_radius_windows_match_full_chains_through_navigation_damage_and_support_changes() {
    for space in layer_core::BlendSpace::ALL {
        let mut doc = document_at([2053, 1541]);
        let extent = doc.composition().size;
        let paint = source_at(&doc, 0);
        blur_chain(&mut doc);
        occurrence_mut(&mut doc, 2).attachment = Attachment::Effect;
        let masked=doc.scene().order()[1];
        coverage_mask(&mut doc,masked,layer_core::Point { x: 17., y: -9. },Some(layer_core::Selection::polygon(vec![
            layer_core::Point { x: 270., y: 170. }, layer_core::Point { x: 1700., y: 270. },
            layer_core::Point { x: 1600., y: 1290. }, layer_core::Point { x: 310., y: 1250. },
        ]).unwrap()));
        occurrence_mut(&mut doc,1).opacity = 0.7;
        set_attachment_at(&mut doc, 1, true);
        let mut window = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut full = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        for (step, (level, origin, sigma)) in [
            (1, [610., 610.], 13.), (1, [1110., 790.], 13.), (1, [610., 610.], 13.),
            (2, [610., 610.], 21.), (2, [810., 690.], 0.), (2, [610., 610.], 7.),
        ].into_iter().enumerate() {
            set_effect_at(&mut doc,0,"sigma", EffectValue::Number(sigma));
            let scale = 1. / (1 << level) as f32;
            let mut frame = packet(doc.scene(), extent);
            frame.blend_space = space;
            frame.composite_all = false;
            frame.view.width_px = 192;
            frame.view.height_px = 128;
            frame.view.document_to_surface = [scale, 0., 0., scale, -origin[0] * scale, -origin[1] * scale];
            let whole = FramePacket { view: layer_render::ViewState { width_px: extent[0], height_px: extent[1],
                document_to_surface: [scale, 0., 0., scale, 0., 0.], ..frame.view }, ..frame };
            window.submit(frame).unwrap();
            full.submit(whole).unwrap();
            let plan = window.scale_display.as_ref().unwrap().plan;
            assert_eq!(plan.level, level);
            assert!(plan.bounds.min_x() > 0 && plan.bounds.min_y() > 0);
            assert!(plan.bounds.area() < PixelRect::full(extent).area());
            assert_window_matches_full(&window, &full, (space, step, "navigation"));
            assert_spatial_storage_reserved(&window, frame);
            if step == 0 || step == 3 {
                for center in [[plan.bounds.min_x() as f32 - 7., origin[1] + 70.],
                    [768., 512.], [plan.bounds.max_x() as f32 + 7., origin[1] + 70.]] {
                    let mut dab = crate::tests::test_dab(center, [0.9, 0.1, 0.2, 1.], 1.);
                    dab.radii = [11.; 2];
                    let batch = dab_batch(paint, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
                    window.submit(FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
                    full.submit(FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..whole }).unwrap();
                    assert_window_matches_full(&window, &full, (space, step, center));
                    assert_spatial_storage_reserved(&window, frame);
                }
            }
            assert_presentation_mip(&window);
        }
        assert_eq!(exact_pixels(&mut window), exact_pixels(&mut full));
        full.test.reference = true;
        full.submit(FramePacket { blend_space: space, ..packet(doc.scene(), extent) }).unwrap();
        let reference = pixels(&full, crate::test_support::document_texture(&full));
        let mut frame = packet(doc.scene(), extent);
        frame.blend_space = space;
        frame.view.width_px = 192;
        frame.view.height_px = 128;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, -152.5, -152.5];
        window.submit(frame).unwrap();
        assert!(window.has_pending_work());
        let mut scene = window.scene.take().unwrap();
        let mut encoder = crate::submission::CommandEncoder::new(&window.device, &Default::default());
        scene.refine_display(&mut window, frame, &mut encoder).unwrap();
        drop(encoder);
        window.scene = Some(scene);
        window.submit(FramePacket { composite_all: false, ..frame }).unwrap();
        assert_settled(&mut window, frame, &reference);
    }
}

#[test]
fn pointwise_graph_keeps_document_coordinates_masks_clipping_and_exact_queries() {
    for space in layer_core::BlendSpace::ALL {
    let mut doc = document();
    let extent = doc.composition().size;
    let paint = source_at(&doc, 0);
    let adjustment = effect(&mut doc,"exposure");
    let program = effect_program_mut(&mut doc,adjustment);
    program.id = "position_adjustment".into();
    program.entry = "position_adjustment".into();
    program.wgsl = "fn position_adjustment(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4(c.rgb*.7+vec3(p/fx_extent(),0.)*.1*c.a,c.a);}".into();
    coverage_mask(&mut doc,adjustment,Default::default(),Some(layer_core::Selection::polygon(vec![
        layer_core::Point { x: 63., y: 37. }, layer_core::Point { x: 410., y: 37. },
        layer_core::Point { x: 410., y: 206. }, layer_core::Point { x: 63., y: 206. },
    ]).unwrap()));
    let next = effect(&mut doc,"exposure");
    set_effect_value(&mut doc,next,"exposure", EffectValue::Number(0.3));
    insert_occurrence(&mut doc,adjustment,0);
    insert_occurrence(&mut doc,next,0);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    for level in [0, 1, 2, 3] {
        let mut updates = None;
        for state in 0..4 {
            set_effect_at(&mut doc,0,"exposure", EffectValue::Number(state as f32 * 0.1));
            set_attachment_at(&mut doc, 1, state % 2 == 0);
            occurrence_mut(&mut doc,1).mask.as_mut().unwrap().inverted = state >= 2;
            occurrence_mut(&mut doc,1).mask.as_mut().unwrap().translation.x = if state == 3 { 17. } else { 0. };
            let mut frame = packet(doc.scene(), extent);
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
            assert_eq!(exact_pixels(&mut r), exact_pixels(&mut exact));
            r.submit(FramePacket { composite_all: false, ..frame }).unwrap();
            assert_eq!(work, r.scene.as_ref().unwrap().scale_sources.entries[&paint].updates);
            assert_presentation_mip(&r);
        }
    }
    let dab = crate::tests::test_dab([400., 210.], [0.8, 0.2, 0.1, 1.], 0.6);
    let batch = dab_batch(paint, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
    let mut frame = packet(doc.scene(), extent);
        frame.blend_space = space;
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    frame.composite_all = false;
    frame.dabs = std::slice::from_ref(&dab);
    frame.dab_batches = std::slice::from_ref(&batch);
    r.submit(frame).unwrap(); exact.submit(frame).unwrap();
    let plan = r.scale_display.as_ref().unwrap().plan;
    let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), plan);
    assert!(error[0] < 0.002 && error[1] < 0.015, "partial paint error={error:?}");
    assert_eq!(exact_pixels(&mut r), exact_pixels(&mut exact));
    }
}

#[test]
fn window_effects_preserve_document_coordinates_and_shifted_masks() {
    let mut doc = document_at([1541, 1027]);
    let extent = doc.composition().size;
    let adjustment = effect(&mut doc,"exposure");
    let program = effect_program_mut(&mut doc,adjustment);
    program.id = "window_position".into();
    program.entry = "window_position".into();
    program.wgsl = "fn window_position(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4(c.rgb*.7+vec3(p/fx_extent(),0.)*.1*c.a,c.a);}".into();
    coverage_mask(&mut doc,adjustment,layer_core::Point { x: 17., y: -9. },Some(layer_core::Selection::polygon(vec![
        layer_core::Point { x: 260., y: 130. }, layer_core::Point { x: 1300., y: 170. },
        layer_core::Point { x: 1100., y: 920. }, layer_core::Point { x: 310., y: 850. },
    ]).unwrap()));
    insert_occurrence(&mut doc,adjustment,0);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut whole = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), extent);
    exact.submit(frame).unwrap();
    let oracle = pixels(&exact, crate::test_support::document_texture(&exact));
    frame.view.width_px = 192;
    frame.view.height_px = 128;
    frame.composite_all = false;
    for level in [0, 1, 2, 4] {
        let scale = 1. / (1 << level) as f32;
        let mut full = packet(doc.scene(), extent);
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
    assert_eq!(exact_pixels(&mut r), exact_pixels(&mut exact));
}

#[test]
fn qualified_pointwise_catalog_uses_display_graph_and_keeps_native_output() {
    let mut doc = document_at([65, 33]);
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let base_entries=doc.scene().order().to_vec();
    for fixture in crate::tests::fixtures().iter().filter(|f| {
        f.program().resolution == EffectResolution::Display && !f.program().image_boundary()
    }) {
        set_root_entries(&mut doc,base_entries.clone());
        let current=effect(&mut doc,&fixture.program().id);insert_occurrence(&mut doc,current,0);
        let mut frame = packet(doc.scene(), extent);
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        r.submit(frame).unwrap(); exact.submit(frame).unwrap();
        assert_eq!(r.scale_display.as_ref().unwrap().plan.level, 3, "{}", fixture.program().id);
        assert_eq!(exact_pixels(&mut r), exact_pixels(&mut exact), "{}", fixture.program().id);
        assert!(display_pixels(&r).iter().flatten().all(|v| v.is_finite()));
    }
}

#[test]
fn pass_through_graph_matches_ungrouping_and_fades_its_backdrop() {
    for space in layer_core::BlendSpace::ALL {
    let mut doc = document();
    let extent = doc.composition().size;
    let backdrop=doc.scene().order().to_vec();
    let adjustment=effect(&mut doc,"exposure");
    set_effect_value(&mut doc,adjustment,"exposure",EffectValue::Number(-1.));
    let paint=copy_paint(&mut doc,0);
    doc.artwork.occurrences.get_mut(paint).unwrap().opacity=0.6;
    doc.artwork.occurrences.get_mut(paint).unwrap().blend=layer_core::LayerBlend::Multiply;
    let nested=stack_occurrence(&mut doc,"nested",vec![adjustment,paint]);
    doc.artwork.occurrences.get_mut(nested).unwrap().blend=layer_core::LayerBlend::PassThrough;
    let group=stack_occurrence(&mut doc,"pass through",vec![nested]);
    doc.artwork.occurrences.get_mut(group).unwrap().blend=layer_core::LayerBlend::PassThrough;
    set_root_entries(&mut doc,[vec![group],backdrop.clone()].concat());
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    for level in [1, 2, 3] {
        let mut draw = |doc: &Document| {
            let mut frame = packet(doc.scene(), extent);
        frame.blend_space = space;
            let scale = 1. / (1 << level) as f32;
            frame.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
            r.submit(frame).unwrap();
            assert_eq!(r.scale_display.as_ref().unwrap().plan.level, level);

            display_pixels(&r)
        };
        let mut flat=doc.clone();
        for handle in [group,nested] {
            let OccurrenceContent::Stack(stack)=flat.artwork.occurrences.remove(handle).unwrap().content else {unreachable!()};
            flat.artwork.stacks.remove(stack);
        }
        set_root_entries(&mut flat,[vec![adjustment,paint],backdrop.clone()].concat());
        let expected = draw(&flat);
        let full = draw(&doc);
        let error = crate::test_support::max_error;
        assert!(error(&full, &expected) < 2e-5, "nested pass through equals ungrouping");
        let mut hidden=doc.clone();occurrence_mut(&mut hidden,0).visible=false;
        let backdrop = draw(&hidden);
        for masked in [false, true] {
            let mut faded=doc.clone();occurrence_mut(&mut faded,0).opacity=0.4;
            if masked {
                let mask=coverage_mask(&mut faded,group,Default::default(),None);
                faded.artwork.coverage.get_mut(mask).unwrap().default_coverage=0.3;
            }
            let amount = if masked { 0.12 } else { 0.4 };
            let expected: Vec<[f32; 4]> = backdrop.iter().zip(&full)
                .map(|(back, front)| std::array::from_fn(|i| back[i] + (front[i] - back[i]) * amount)).collect();
            assert!(error(&draw(&faded), &expected) < 2e-5, "level={level} masked={masked}");
        }
    }
    }
}

#[test]
fn spatial_graph_updates_dependency_halos_and_preserves_exact_output() {
    for space in layer_core::BlendSpace::ALL {
    let mut doc = document_at([1027, 773]);
    let extent = doc.composition().size;
    let paint = source_at(&doc, 0);
    for (_, sigma) in [(80, 9.), (81, 15.)] {
        let blur = effect(&mut doc,"gaussian_blur");
        effect_program_mut(&mut doc,blur).resolution=EffectResolution::Display;
        set_effect_value(&mut doc,blur,"sigma",EffectValue::Number(sigma));
        insert_occurrence(&mut doc,blur,0);
    }
    let exposure=effect(&mut doc,"exposure");insert_occurrence(&mut doc,exposure,0);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut frame = packet(doc.scene(), extent);
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
        assert_eq!(exact_pixels(&mut r), exact_pixels(&mut exact));
    }
    }
}

#[test]
fn spatial_graph_keeps_masks_clipping_global_dependencies_and_scale_preparation() {
    for space in layer_core::BlendSpace::ALL {
    let mut doc = document();
    let extent = doc.composition().size;
    let blur = effect(&mut doc,"gaussian_blur");
    let mask=coverage_mask(&mut doc,blur,Default::default(),None);
    doc.artwork.coverage.get_mut(mask).unwrap().default_coverage=0.4;
    doc.artwork.occurrences.get_mut(blur).unwrap().opacity=0.7;
    insert_occurrence(&mut doc,blur,0);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    for (level, sigma, clipped) in [(1, 0., false), (2, 0.5, false), (3, 3., true), (4, 21., false), (1, 21., true)] {
        set_effect_at(&mut doc,0,"sigma", EffectValue::Number(sigma));
        set_attachment_at(&mut doc, 0, clipped);
        let mut frame = packet(doc.scene(), extent);
        frame.blend_space = space;
        let scale = 1. / (1 << level) as f32;
        frame.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        for renderer in [&mut r, &mut exact] { renderer.submit(frame).unwrap(); }
        let plan = r.scale_display.as_ref().unwrap().plan;
        let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), plan);
        assert!(error[0] < 0.004 && error[1] < 0.04, "level={level} sigma={sigma} clipped={clipped}: {error:?}");
        assert_eq!(exact_pixels(&mut r), exact_pixels(&mut exact));
        assert_settled(&mut r, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        let preparations = r.scene.as_ref().unwrap().effects.preparation_count();
        let work = r.metrics.composited_pixels;
        r.submit(FramePacket { composite_all: false, ..frame }).unwrap();
        assert_eq!(preparations, r.scene.as_ref().unwrap().effects.preparation_count());
        assert_eq!(work, r.metrics.composited_pixels);
    }
    let program=effect_program_at_mut(&mut doc,0);
    program.id = "global_probe".into();
    program.wgsl = "fn global_probe(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return fx_sample(fx_extent()-p)*.6+fx_original(p)*.4;}".into();
    program.entry = "global_probe".into();
    program.lookups = Arc::new([]);
    program.passes = vec![layer_core::EffectPass { entry: program.entry.clone(), sampling: layer_core::EffectSampling::Document }].into();
    let mut frame = packet(doc.scene(), extent);
        frame.blend_space = space;
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    for renderer in [&mut r, &mut exact] { renderer.submit(frame).unwrap(); }
    let dab = crate::tests::test_dab([73., 81.], [0.8, 0.1, 0.2, 1.], 1.);
    let batch = dab_batch(source_at(&doc, 1), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
    r.submit(FramePacket { composite_all: false, dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
    let incremental = display_pixels(&r);
    r.scale_display.as_mut().unwrap().graph = Default::default();
    r.submit(frame).unwrap();
    assert_eq!(incremental, display_pixels(&r), "global dependencies update pixels far from paint damage");
    }
}

#[test]
fn pointwise_curve_edits_reuse_sources_and_bound_window_passes_with_masked_coordinates() {
    for space in layer_core::BlendSpace::ALL {
        let mut doc=document_at([2053,1541]);
        let paint=source_at(&doc, 0);
        let curves=effect(&mut doc,"curves");
        doc.artwork.occurrences.get_mut(curves).unwrap().opacity=0.7;
        coverage_mask(&mut doc,curves,layer_core::Point{x:17.,y:-9.},Some(layer_core::Selection::polygon(vec![
            layer_core::Point{x:230.,y:140.},layer_core::Point{x:1750.,y:220.},
            layer_core::Point{x:1680.,y:1310.},layer_core::Point{x:270.,y:1240.},
        ]).unwrap()));
        insert_occurrence(&mut doc,curves,0);
        let mut window=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut full=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let extent=doc.composition().size;
        for (step,y) in [0.2,0.7,0.4].into_iter().enumerate() {
            set_effect_at(&mut doc,0,"rgb",EffectValue::Curve(vec![[0.,0.],[0.4,y],[1.,1.]]));
            let mut frame=packet(doc.scene(),extent);frame.blend_space=space;frame.composite_all=false;
            frame.view.width_px=640;frame.view.height_px=480;
            frame.view.document_to_surface=[0.5,0.,0.,0.5,-190.,-130.];
            let whole=FramePacket{view:layer_render::ViewState{width_px:extent[0],height_px:extent[1],document_to_surface:[0.5,0.,0.,0.5,0.,0.],..frame.view},..frame};
            let updates=window.scene.as_ref().and_then(|scene|scene.scale_sources.entries.get(&paint)).map(|source|source.updates);
            window.submit(frame).unwrap();full.submit(whole).unwrap();
            assert_window_matches_full(&window,&full, (space, step, "curves"));
            assert_spatial_storage_reserved(&window,frame);
            if step>0 {
                assert_eq!(window.scene.as_ref().unwrap().scale_sources.entries[&paint].updates,updates.unwrap(),"curve edits must not rebuild photo source levels");
                let cache=window.scale_display.as_ref().unwrap();
                let expected=1+cache.overview.as_ref().map_or(0,|overview|overview.plan.bounds.subtract(cache.plan.bounds).into_iter().filter(|region|!region.is_empty()).count() as u64);
                assert_eq!(window.scene.as_ref().unwrap().effect_passes,expected,"one pointwise effect pass per main/overview window, not per tile");
            }
        }
        full.test.reference=true;
        full.submit(FramePacket{blend_space:space,..packet(doc.scene(),extent)}).unwrap();
        assert_eq!(exact_pixels(&mut window),exact_pixels(&mut full),"masked coordinates preserve the independent native reference");
    }
}

#[test]
fn pointwise_large_window_keeps_tiled_scratch_admission() {
    let mut doc=document_at([64,64]);
    composition_mut(&mut doc).size=[8192;2];
    paint_mut(&mut doc,0).domain=[64;2];
    let curves=effect(&mut doc,"curves");insert_occurrence(&mut doc,curves,0);
    let r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame=packet(doc.scene(),doc.composition().size);
    frame.view.width_px=4096;frame.view.height_px=4096;
    frame.view.document_to_surface=[0.5,0.,0.,0.5,0.,0.];
    let plan=view_plan(frame,1,Evaluation::Display).unwrap();
    assert!(use_tiles(&r,plan,frame,None,false,None),"whole-window scratch must not exceed the existing cache budget");
    assert!(allocation_for(&r,plan,frame,None,false,None).into_iter().sum::<u64>()<=CACHE_BYTES,"tiled fallback remains admitted without full-window scratch");
}

#[test]
fn decoded_gaussian_display_refinement_finishes_with_bounded_chunks() {
    let extent=[2049,1281];
    let mut doc=document_at(extent);
    paint_mut(&mut doc,0).original=Some(crate::test_support::depth_source(extent,SampleDepth::U8,
        layer_core::color::RgbSpace::Srgb,16<<20,|x,y| {
            if x<1024 {[0.08,0.4,0.9,1.]}else{[0.8,0.12+0.2*(y%257) as f32/256.,0.25,1.]}
        }));
    let blur=effect(&mut doc,"gaussian_blur");
    set_effect_value(&mut doc,blur,"sigma",EffectValue::Number(85.));
    insert_occurrence(&mut doc,blur,0);
    for cap in [256u64<<20,96<<20] {
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.set_complete_display_allowance(1024<<20);
    r.native_edit.as_mut().unwrap().image_pixel_bytes=Some(cap);
    r.source_tiles.get_mut().admit(0);
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
    frame.view.width_px=512;frame.view.height_px=320;
    frame.view.document_to_surface=[0.25,0.,0.,0.25,0.,0.];
    r.submit(frame).unwrap();
    assert!(r.scale_display.as_ref().unwrap().evaluation==Evaluation::Display);
    let pages=page_coordinates(PixelRect::full(extent)).count();
    let revision=r.artwork_revision;
    let mut records=Vec::new();
    for step in 0..=pages {
        if !r.has_pending_work(){break;}
        assert!(step<pages,"Gaussian display refinement did not finish within one visit per native page");
        r.wait_idle().unwrap();
        let composed=r.metrics.composited_pixels;
        let passes=r.metrics.command_passes;
        let image_before=r.scene.as_ref().unwrap().image_pass_pixels();
        r.submit(frame).unwrap();
        let image_after=r.scene.as_ref().unwrap().image_pass_pixels();
        assert!(r.scene.as_ref().unwrap().image_cache_bytes()<=cap,"refinement images exceed {cap} bytes");
        let produced=r.metrics.composited_pixels-composed;
        assert!(produced<=4*u64::from(PAGE_SIZE).pow(2),"refinement exceeded the input-yield chunk");
        assert_eq!(r.artwork_revision,revision);
        records.push((step,produced,r.metrics.command_passes-passes,image_after-image_before));
    }
    assert!(!r.has_pending_work());
    assert!(r.scale_display.as_ref().unwrap().resident_bytes()>0);
    let image_pixels=records.iter().map(|r|r.3).sum::<u64>();
    let single_page_pixels=page_coordinates(PixelRect::full(extent)).map(|c| {
        let page=page_rect(c).intersect(PixelRect::full(extent));
        Scene::capture_window(doc.scene(),page,extent).area()*2
    }).sum::<u64>();
    println!("decoded Gaussian sigma85 cap={cap} refinement image_pixels={image_pixels} single_page_pixels={single_page_pixels} chunks={records:?}");
    assert!(image_pixels*4<=single_page_pixels*3,"batched refinement must avoid rebuilding each page's complete blur halo");
    let native=pixels(&r,&r.scale_display.as_ref().unwrap().hierarchy.as_ref().unwrap().root().texture);
    assert_presentation_mip(&r);
    let mut exact=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();exact.test.reference=true;
    exact.submit(packet(doc.scene(),extent)).unwrap();
    let reference=pixels(&exact,crate::test_support::document_texture(&exact));
    assert_eq!(native.len(),reference.len());
    let maximum=crate::test_support::max_error(&native,&reference);
    assert!(maximum<=2e-5,"completed native refinement pixels differ from full source reference: {maximum}");
    let work=r.metrics.composited_pixels;r.submit(frame).unwrap();
    assert_eq!(r.metrics.composited_pixels,work,"settled Gaussian composition must not restart");
    }
}

mod gaussian {
use super::*;
use layer_core::{EffectInstance, EffectKind, EffectResolution, EffectSpace, EffectValue};
use std::sync::Arc;

const SIGMAS:[f32;9]=[0.,0.1,1.,3.,21.,21.1,64.,85.,120.];
const CONSUMERS:[&str;6]=["gaussian_blur","unsharp_mask","high_pass","bloom","soft_focus","pencil"];

fn gaussian_fixture(id:&str,sigma:f32,resolution:EffectResolution)->EffectInstance {
    let mut program=(*crate::tests::fixture(id).program()).clone();
    program.resolution=resolution;
    let mut effect=EffectInstance::new(Arc::new(program));
    set(&mut effect,"sigma",sigma);
    effect
}

fn set(effect:&mut EffectInstance,key:&str,value:f32) {
    effect.set(key,EffectValue::Number(value)).unwrap();
}

fn gaussian_document(extent:[u32;2],color:layer_core::color::DocumentColor,effects:impl IntoIterator<Item=EffectInstance>)->Document {
    let mut artwork=Artwork::new(extent).unwrap();
    artwork.compositions.get_mut(artwork.root).unwrap().color=color;
    let mut doc=Document::from_artwork(artwork).unwrap();
    let entries=effects.into_iter().map(|effect|effect_occurrence(&mut doc,effect,"Gaussian fixture")).collect();
    set_root_entries(&mut doc,entries);doc
}

#[derive(Clone,Copy,Debug)]
enum Field { Constant([f32;4]), Impulse([i32;2],[f32;4]), Step(i32,[f32;4],[f32;4]) }

fn original(field:Field)->EffectInstance {
    let literal=|c:[f32;4]|format!("vec4<f32>({:?},{:?},{:?},{:?})",c[0],c[1],c[2],c[3]);
    let expression=match field {
        Field::Constant(c)=>literal(c),
        Field::Impulse(p,c)=>format!("select(vec4<f32>(0.),{},all(vec2<i32>(floor(p))==vec2<i32>({},{})))",literal(c),p[0],p[1]),
        Field::Step(x,left,right)=>format!("select({},{},p.x>=f32({}))",literal(left),literal(right),x),
    };
    let mut program=(*crate::tests::fixture("exposure").program()).clone();
    program.kind=EffectKind::Generator;program.space=EffectSpace::Linear;
    program.entry="gaussian_original_field".into();
    program.wgsl=format!("fn gaussian_original_field(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{{return {expression};}}").into();
    EffectInstance::new(Arc::new(program))
}

fn kernel(sigma:f32)->Vec<f64> {
    if sigma==0. {return vec![1.];}
    let sigma=f64::from(sigma);let radius=(3.*sigma).ceil() as usize;
    let mut weights=(0..=radius).map(|i|(-0.5*(i as f64/sigma).powi(2)).exp()).collect::<Vec<_>>();
    let total=weights[0]+2.*weights[1..].iter().sum::<f64>();
    for weight in &mut weights{*weight/=total;}weights
}

fn weight(weights:&[f64],distance:i32)->f64 {
    weights.get(distance.unsigned_abs() as usize).copied().unwrap_or(0.)
}

fn expected(field:Field,p:[i32;2],extent:[u32;2],weights:&[f64])->[f64;4] {
    let radius=weights.len() as i32-1;
    let mass=|axis:usize,predicate:&dyn Fn(i32)->bool|(-radius..=radius)
        .filter(|k|predicate((p[axis]+k).clamp(0,extent[axis] as i32-1)))
        .map(|k|weight(weights,k)).sum::<f64>();
    match field {
        Field::Impulse(center,c)=>c.map(|v|f64::from(v)*mass(0,&|x|x==center[0])*mass(1,&|y|y==center[1])),
        Field::Constant(c)=>{let total=mass(0,&|_|true)*mass(1,&|_|true);c.map(|v|f64::from(v)*total)},
        Field::Step(edge,left,right)=>{
            let vertical=mass(1,&|_|true);let a=mass(0,&|x|x<edge);let b=mass(0,&|x|x>=edge);
            std::array::from_fn(|i|(a*f64::from(left[i])+b*f64::from(right[i]))*vertical)
        }
    }
}

fn assert_points(image:&[[f32;4]],extent:[u32;2],field:Field,sigma:f32,points:&[[i32;2]]) {
    let weights=kernel(sigma);let mut maximum=0_f64;
    for &p in points {
        let actual=image[(p[1] as u32*extent[0]+p[0] as u32) as usize];let target=expected(field,p,extent,&weights);
        for i in 0..4 {
            let error=(f64::from(actual[i])-target[i]).abs();maximum=maximum.max(error);
            let tolerance=match field {Field::Impulse(..)=>target[i].abs()*0.003+if sigma>85. {f64::from(layer_core::color::f16::from_bits(1).to_f32())}else{2e-10},_=>2e-5*target[i].abs().max(1.)};
            assert!(actual[i].is_finite()&&error<=tolerance,"{field:?} sigma={sigma} at={p:?} actual={actual:?} expected={target:?}");
        }
    }
    println!("Gaussian {field:?} sigma={sigma} maximum selected component error={maximum}");
}

#[test]
fn gaussian_support_edges_and_seams_match_unpaired_f64_kernel() {
    let extent=[1029,517];let color=layer_core::color::DocumentColor{depth:SampleDepth::F32,..Default::default()};
    let mut r=WgpuRasterizer::new_native_headless(color).unwrap();r.test.reference=true;
    let center=[256,258];
    let fields=[Field::Constant([1.5,-0.25,2.,0.5]),Field::Impulse(center,[0.25,0.5,1.,1.]),Field::Step(1024,[0.1,0.2,0.3,0.5],[0.8,0.4,0.2,1.])];
    for field in fields {
        for sigma in SIGMAS {
            let doc=gaussian_document(extent,color,[gaussian_fixture("gaussian_blur",sigma,EffectResolution::Native),original(field)]);
            r.submit(packet(doc.scene(),extent)).unwrap();
            let image=pixels(&r,crate::test_support::document_texture(&r));
            let mut points=vec![[0,0],[0,258],[255,258],[256,258],[257,258],[1023,258],[1024,258],[1028,516]];
            points.extend([0,1,63,64,127,128,200,254,255,256].into_iter().map(|d|[center[0]+d,center[1]]));
            assert_points(&image,extent,field,sigma,&points);
            if sigma==0. {
                for p in [[0,0],center,[1028,516]] {assert_eq!(image[(p[1] as u32*extent[0]+p[0] as u32) as usize],expected(field,p,extent,&[1.]).map(|v|v as f32));}
            }
        }
    }
}

#[test]
fn saved_large_sigmas_preserve_gaussian_width_and_edge_mass() {
    let extent=[257,9];let color=layer_core::color::DocumentColor {depth:SampleDepth::F32,..Default::default()};
    let mut r=WgpuRasterizer::new_native_headless(color).unwrap();r.test.reference=true;
    let fields=[Field::Constant([1.5,-0.25,2.,0.5]),Field::Impulse([128,4],[0.25,0.5,1.,1.]),
        Field::Step(128,[0.1,0.2,0.3,0.5],[0.8,0.4,0.2,1.])];
    for sigma in [85.01,120.,512.,65536.] {
        for field in fields {
            let doc=gaussian_document(extent,color,[gaussian_fixture("gaussian_blur",sigma,EffectResolution::Native),original(field)]);
            r.submit(packet(doc.scene(),extent)).unwrap();
            let image=pixels(&r,crate::test_support::document_texture(&r));
            assert_points(&image,extent,field,sigma,&[[0,0],[1,4],[64,4],[127,4],[128,4],[129,4],[192,4],[255,4],[256,8]]);
        }
    }
}

#[test]
fn saved_spatial_lengths_keep_their_scale_beyond_editor_bounds() {
    let extent=[129,3];let color=layer_core::color::DocumentColor {depth:SampleDepth::F32,..Default::default()};
    let mut r=WgpuRasterizer::new_native_headless(color).unwrap();r.test.reference=true;
    let sample=|x:f64| {let x=(x-0.5).clamp(0.,128.);let a=if x.floor()<64. {0.1}else{0.8};
        let b=if x.floor()+1.<64. {0.1}else{0.8};a+(b-a)*x.fract()};
    for (id,key) in [("motion_blur","distance"),("pixel_mosaic","size")] {
        for value in [0.25,120.,65536.] {
            let mut effect=EffectInstance::new(crate::tests::fixture(id).program());set(&mut effect,key,value);
            if id=="motion_blur" {set(&mut effect,"angle",0.);}
            let doc=gaussian_document(extent,color,[effect,original(Field::Step(64,[0.1;4],[0.8;4]))]);
            r.submit(packet(doc.scene(),extent)).unwrap();let image=pixels(&r,crate::test_support::document_texture(&r));
            for x in [0,32,63,64,65,96,128] {
                let value=f64::from(value);let p=f64::from(x)+0.5;
                let expected=if id=="pixel_mosaic" {sample(((p/value).floor()+0.5)*value)}else {
                    let count=(value.ceil() as usize+1).max(2);
                    (0..count).map(|i|sample(p+value*(i as f64/(count-1) as f64-0.5))).sum::<f64>()/count as f64
                };
                let actual=image[129+x as usize];
                assert!(actual.iter().all(|v|v.is_finite()&&(f64::from(*v)-expected).abs()<2e-5),"{id} {value} at {x}: {actual:?} != {expected}");
            }
        }
    }
}

#[test]
fn gaussian_consumers_share_preparation_and_reuse_it_for_amount_changes() {
    let extent=[517,517];let color=layer_core::color::DocumentColor{depth:SampleDepth::F32,..Default::default()};
    let field=Field::Constant([0.1,0.4,0.9,1.]);
    for id in CONSUMERS {
        let mut r=WgpuRasterizer::new_native_headless(color).unwrap();r.test.reference=true;
        let mut doc=gaussian_document(extent,color,[gaussian_fixture(id,85.,EffectResolution::Native),original(field)]);
        if id=="bloom" {set_effect_at(&mut doc,0,"threshold",EffectValue::Number(0.));}
        r.submit(packet(doc.scene(),extent)).unwrap();
        let count=r.scene.as_ref().unwrap().effects.preparation_count();assert_eq!(count,1,"{id}");
        let image=pixels(&r,crate::test_support::document_texture(&r));let actual=image[(258*extent[0]+258) as usize];
        let target=match id {
            "gaussian_blur"|"unsharp_mask"|"soft_focus"=>[0.1,0.4,0.9,1.],
            "high_pass"=>{let v=layer_core::color::RgbSpace::Srgb.decode(0.5);[v,v,v,1.]},
            "bloom"=>{let amount=match doc.scene().effect(doc.scene().order()[0]).unwrap().value("amount").unwrap(){EffectValue::Number(v)=>f64::from(*v)/100.,_=>panic!("amount")};[0.1*(1.+amount),0.4*(1.+amount),0.9*(1.+amount),1.]},
            "pencil"=>{let c=[0.97,0.95,0.9].map(|v|layer_core::color::RgbSpace::Srgb.decode(v));[c[0],c[1],c[2],1.]},_=>unreachable!(),
        };
        for i in 0..4 {assert!((f64::from(actual[i])-target[i]).abs()<2e-5,"{id} constant actual={actual:?} expected={target:?}");}
        let independent_key=if id=="pencil"{"contrast"}else{"amount"};
        if doc.scene().effect(doc.scene().order()[0]).unwrap().value(independent_key).is_some(){set_effect_at(&mut doc,0,independent_key,EffectValue::Number(61.));}else{occurrence_mut(&mut doc,0).opacity=0.61;}
        r.submit(packet(doc.scene(),extent)).unwrap();assert_eq!(r.scene.as_ref().unwrap().effects.preparation_count(),count,"{id} amount-only");
        set_effect_at(&mut doc,0,"sigma",EffectValue::Number(64.));r.submit(packet(doc.scene(),extent)).unwrap();assert_eq!(r.scene.as_ref().unwrap().effects.preparation_count(),count+1,"{id} sigma edit");
        r.submit(FramePacket{composite_all:false,..packet(doc.scene(),extent)}).unwrap();assert_eq!(r.scene.as_ref().unwrap().effects.preparation_count(),count+1,"{id} frozen");
    }
}

#[test]
fn sigma_zero_preserves_tiny_alpha_signed_hdr_and_transparent_pixels_exactly() {
    let extent=[259,257];let color=layer_core::color::DocumentColor{depth:SampleDepth::F32,..Default::default()};
    let mut r=WgpuRasterizer::new_native_headless(color).unwrap();r.test.reference=true;
    for c in [[2e-7,-8e-8,4e-7,8e-8],[4.,-2.,8.,1.],[0.;4]] {
        let doc=gaussian_document(extent,color,[gaussian_fixture("gaussian_blur",0.,EffectResolution::Native),original(Field::Constant(c))]);
        r.submit(packet(doc.scene(),extent)).unwrap();
        let image=pixels(&r,crate::test_support::document_texture(&r));
        assert!(image.iter().all(|actual|*actual==c),"sigma zero must preserve original {c:?}");
    }
}

#[test]
fn gaussian_normalized_fields_have_valid_coverage_before_output_encoding() {
    let extent=[33,17];let color=layer_core::color::DocumentColor{depth:SampleDepth::F32,..Default::default()};
    let mut r=WgpuRasterizer::new_native_headless(color).unwrap();r.test.reference=true;
    for sigma in [0.,0.1,1.,3.,21.,21.1,64.,85.] {
        for c in [[0.25,0.5,0.75,1.],[4.,-2.,8.,1.],[0.1,0.2,0.4,0.5],
            [f32::MIN_POSITIVE,-f32::MIN_POSITIVE,2.*f32::MIN_POSITIVE,1.],
            [2e-7,-8e-8,4e-7,8e-8]] {
            let source=original(Field::Constant(c));
            let unfiltered=if sigma<=0.1 {
                let doc=gaussian_document(extent,color,[source.clone()]);
                r.submit(packet(doc.scene(),extent)).unwrap();Some(pixels(&r,crate::test_support::document_texture(&r)))
            }else{None};
            let doc=gaussian_document(extent,color,[gaussian_fixture("gaussian_blur",sigma,EffectResolution::Native),source]);
            r.submit(packet(doc.scene(),extent)).unwrap();
            let image=pixels(&r,crate::test_support::document_texture(&r));
            for actual in &image {
                assert!(actual.iter().all(|v|v.is_finite())&&(0. ..=1.).contains(&actual[3]),"sigma={sigma} original={c:?} normalized={actual:?}");
                if c[3]<1e-6 {for i in 0..4 {
                    assert!((f64::from(actual[i])-f64::from(c[i])).abs()<=2e-5*f64::from(c[i]).abs(),"small normal alpha sigma={sigma} original={c:?} normalized={actual:?}");
                }}
            }
            if let Some(unfiltered)=unfiltered {for (index,(actual,original)) in image.iter().zip(&unfiltered).enumerate() {
                assert_eq!(actual,original,"degenerate kernel sigma={sigma} must preserve actual unfiltered field {c:?} at pixel {index}");
            }}
            if c[0]==f32::MIN_POSITIVE {println!("minimum-normal RGB sigma={sigma} input={c:?} output={:?}",image[0]);}
        }
    }
}

#[test]
fn gaussian_convex_filter_preserves_finite_extreme_and_opposite_signed_fields() {
    let extent=[129,65];let color=layer_core::color::DocumentColor{depth:SampleDepth::F32,..Default::default()};
    let mut r=WgpuRasterizer::new_native_headless(color).unwrap();r.test.reference=true;
    for limit in [f32::MAX*0.75,f32::MAX] {
        for field in [Field::Constant([limit,-limit,limit,1.]),
            Field::Step(64,[limit,-limit,limit,1.],[-limit,limit,-limit,1.])] {
            for sigma in SIGMAS {
                let doc=gaussian_document(extent,color,[gaussian_fixture("gaussian_blur",sigma,EffectResolution::Native),original(field)]);
                r.submit(packet(doc.scene(),extent)).unwrap();
                let image=pixels(&r,crate::test_support::document_texture(&r));let weights=kernel(sigma);
                for x in [0,1,32,63,64,96,128] {
                    let target=expected(field,[x,32],extent,&weights);let actual=image[(32*extent[0]+x as u32) as usize];
                    assert!(actual.iter().all(|v|v.is_finite())&&(0. ..=1.).contains(&actual[3]),"{field:?} sigma={sigma} x={x}: {actual:?}");
                    for i in 0..3 {assert!((f64::from(actual[i])-target[i]).abs()/f64::from(limit)<2e-5,"{field:?} sigma={sigma} x={x}: {actual:?} expected={target:?}");}
                }
            }
        }
    }
}

#[test]
fn native_bilinear_sampling_is_convex_at_finite_rgb_extremes_on_both_axes() {
    let extent=[8,8];let color=layer_core::color::DocumentColor{depth:SampleDepth::F32,..Default::default()};
    let mut r=WgpuRasterizer::new_native_headless(color).unwrap();r.test.reference=true;
    for limit in [32.,f32::MAX*0.75,f32::MAX] {
        let mut source=original(Field::Constant([limit,-limit,limit,1.]));
        Arc::make_mut(&mut source.program).wgsl=format!(
            "fn gaussian_original_field(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{{let sign=select(-1.,1.,(u32(floor(p.x))+u32(floor(p.y)))%2u==0u);return vec4<f32>(sign*{limit:?},-sign*{limit:?},sign*{limit:?},1.);}}").into();
        for [dx,dy] in [[0.,0.],[-0.25,0.],[0.,-0.25],[-0.25,-0.25],[0.5,0.5]] {
            let mut sampled=gaussian_fixture("gaussian_blur",0.,EffectResolution::Native);
            let program=Arc::make_mut(&mut sampled.program);
            program.entry="bilinear_probe".into();
            program.wgsl=format!("fn bilinear_probe(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{{return fx_sample(p+vec2<f32>({dx:?},{dy:?}));}}").into();
            program.passes=vec![layer_core::EffectPass {entry:program.entry.clone(),sampling:layer_core::EffectSampling::Document}].into();
            let doc=gaussian_document(extent,color,[sampled,source.clone()]);
            r.submit(packet(doc.scene(),extent)).unwrap();
            let actual=pixels(&r,crate::test_support::document_texture(&r))[3*8+3];
            let x=3.+f64::from(dx);let y=3.+f64::from(dy);let fx=x-x.floor();let fy=y-y.floor();
            let mut expected=0.;for j in 0..2 {for i in 0..2 {
                let sign=if (x.floor() as i32+i+y.floor() as i32+j)%2==0 {1.}else{-1.};
                expected+=sign*f64::from(limit)*if i==0 {1.-fx}else{fx}*if j==0 {1.-fy}else{fy};
            }}
            assert!(actual.iter().all(|v|v.is_finite()),"limit={limit} offset={dx},{dy}: {actual:?}");
            for (component,target) in actual[..3].iter().zip([expected,-expected,expected]) {
                assert!((f64::from(*component)-target).abs()/f64::from(limit)<=4.*f64::from(f32::EPSILON),"limit={limit} offset={dx},{dy}: {actual:?} expected={expected}");
            }
            assert_eq!(actual[3],1.);
        }
    }
}

#[test]
fn native_gaussian_windows_masks_and_clipping_match_full_rebuild() {
    let extent=[1541,771];let color=layer_core::color::DocumentColor{depth:SampleDepth::F32,..Default::default()};
    let field=Field::Step(1024,[0.1,0.4,0.9,1.],[0.8,0.2,0.3,1.]);
    let mut doc=gaussian_document(extent,color,[gaussian_fixture("gaussian_blur",85.,EffectResolution::Native),original(field)]);
    let blur=doc.scene().order()[0];let content=doc.scene().order()[1];
    let owner=stack_occurrence(&mut doc,"Gaussian input",vec![content]);set_root_entries(&mut doc,vec![blur,owner]);
    let mut window=WgpuRasterizer::new_native_headless(color).unwrap();window.native_edit.as_mut().unwrap().image_pixel_bytes=Some(128*1024*1024);
    let mut exact=WgpuRasterizer::new_native_headless(color).unwrap();exact.test.reference=true;
    for (step,(zoom,x,clipped)) in [(0.5,-384.,false),(1.,-1000.,false),(0.125,0.,true)].into_iter().enumerate() {
        let handle=doc.scene().order()[0];
        let mask=coverage_mask(&mut doc,handle,Default::default(),None);doc.artwork.coverage.get_mut(mask).unwrap().default_coverage=0.25;
        occurrence_mut(&mut doc,0).opacity=0.6;set_attachment_at(&mut doc, 0, clipped);
        let mut frame=packet(doc.scene(),extent);frame.view.width_px=96;frame.view.height_px=64;frame.view.document_to_surface=[zoom,0.,0.,zoom,x,-32.];
        window.submit(frame).unwrap();exact.submit(frame).unwrap();
        assert!(window.scale_display.as_ref().unwrap().evaluation==Evaluation::Native);
        assert!(!window.scale_display.as_ref().unwrap().has_pending_work(&window));
        assert!(window.metrics.image_window_peak_bytes<=128*1024*1024);
        let reference=pixels(&exact,crate::test_support::document_texture(&exact));assert_settled(&mut window,frame,&reference);
        if step==0 {let p=[1024,385];let raw=expected(field,p,extent,&kernel(85.));let original=[0.8,0.2,0.3,1.];let actual=reference[(p[1] as u32*extent[0]+p[0] as u32) as usize];for i in 0..4{let target=original[i]+0.15*(raw[i]-original[i]);assert!((f64::from(actual[i])-target).abs()<2e-5,"mask mix {actual:?} expected={target}");}}
    }
}

#[test]
fn native_gaussian_windows_bound_decoded_sources_across_budget_and_support_changes() {
    let extent=[4101,1029];let color=layer_core::color::DocumentColor{depth:SampleDepth::F32,..Default::default()};
    let codes=[[26u8,102,230,255],[204,51,77,255]];
    let source=crate::test_support::depth_source(extent,SampleDepth::U8,layer_core::color::RgbSpace::Srgb,
        32<<20,|x,_|codes[usize::from(x>=2048)].map(|v|f32::from(v)/255.));
    let mut base=gaussian_document(extent,color,[]);
    let paint=paint_occurrence(&mut base,"immutable step source",Some(source));set_root_entries(&mut base,vec![paint]);
    let decoded=codes.map(|c|[0,1,2,3].map(|i|if i==3 {1.}else{layer_core::color::RgbSpace::Srgb.decode(f64::from(c[i])/255.) as f32}));
    let field=Field::Step(2048,decoded[0],decoded[1]);
    for id in ["gaussian_blur"] {for sigma in [21.,85.] {
        let mut adjustment=gaussian_fixture(id,sigma,EffectResolution::Native);
        if adjustment.value("amount").is_some() {set(&mut adjustment,"amount",61.);}
        let mut doc=base.clone();
        let handle=effect_occurrence(&mut doc,adjustment,id);insert_occurrence(&mut doc,handle,0);
        let mask=coverage_mask(&mut doc,handle,Default::default(),None);doc.artwork.coverage.get_mut(mask).unwrap().default_coverage=0.25;
        let occurrence=doc.artwork.occurrences.get_mut(handle).unwrap();occurrence.opacity=0.6;set_attachment(occurrence, true);reindex(&mut doc);
        let mut exact=WgpuRasterizer::new_native_headless(color).unwrap();exact.test.reference=true;
        let mut frame=packet(doc.scene(),extent);frame.view.width_px=extent[0];frame.view.height_px=extent[1];
        exact.submit(frame).unwrap();let reference=pixels(&exact,crate::test_support::document_texture(&exact));
        let mut window=WgpuRasterizer::new_native_headless(color).unwrap();window.source_tiles.get_mut().admit(0);
        let [resident,uploads]=window.source_tiles.borrow().admitted_bytes();
        assert!(u64::from(extent[0].div_ceil(PAGE_SIZE)*extent[1].div_ceil(PAGE_SIZE))*u64::from(PAGE_SIZE).pow(2)*16>resident);
        let tight=if sigma==21. {128u64<<20}else{160<<20};let mut allocated=0;
        for cap in [320u64<<20,tight] {
            window.native_edit.as_mut().unwrap().image_pixel_bytes=Some(cap);window.metrics.image_window_peak_bytes=0;
            let before=window.metrics().image_window_submissions;
            window.submit(frame).unwrap();
            let actual=display_pixels(&window);assert_eq!(actual.len(),reference.len());
            let maximum=actual.iter().zip(&reference).flat_map(|(a,b)|a.iter().zip(b).map(|(a,b)|(a-b).abs())).fold(0f32,f32::max);
            assert!(maximum<=2e-5,"{id} sigma={sigma} cap={cap} seam/pixel error={maximum}");
            let windows=window.metrics().image_window_submissions-before;
            if cap==320<<20 {assert_eq!(windows,0,"{id} sigma={sigma}: retained image stages fit the full-image allowance");}
            else {assert!(windows>0,"{id} sigma={sigma}: constrained budget must use windows");}
            let metrics=window.metrics();assert!(metrics.image_window_peak_bytes<=cap);
            assert!(metrics.source_upload_peak_bytes>0&&metrics.source_upload_peak_bytes<=uploads);
            assert!(metrics.source_tile_misses>=85,"all85 imported tiles must actually be decoded");
            let image_bytes=window.scene.as_ref().unwrap().image_cache_bytes();
            if windows>0 {assert_eq!(image_bytes,0);}else{assert!(image_bytes<=cap);}
            let bytes=window.source_tiles.borrow().gpu_bytes();assert!(bytes<=resident+4*(1<<20));
            if allocated!=0 {assert_eq!(bytes,allocated,"source working-set changes must not grow the fixed decoded allocation");}allocated=bytes;
            println!("{id} sigma={sigma} cap={cap} submissions={windows} image peak={} source bytes={bytes} upload peak={} misses={} hits={} maximum={maximum}",metrics.image_window_peak_bytes,metrics.source_upload_peak_bytes,metrics.source_tile_misses,metrics.source_tile_hits);
            if id=="gaussian_blur" {for x in [2047,2048,2049,3071,3072,4099] {
                let expected=expected(field,[x,514],extent,&kernel(sigma));let input=if x<2048 {decoded[0]}else{decoded[1]};
                let actual=actual[(514*extent[0]+x as u32) as usize];
                for i in 0..4 {let target=f64::from(input[i])+0.15*(expected[i]-f64::from(input[i]));
                    assert!((f64::from(actual[i])-target).abs()<=2e-5,"independent seam[{x}] sigma={sigma} actual={actual:?} expected={target}");}
            }}
        }
    }}
    let mut doc=base;
    for _ in 0..3 {let effect=gaussian_fixture("gaussian_blur",85.,EffectResolution::Native);let handle=effect_occurrence(&mut doc,effect,"gaussian_blur");insert_occurrence(&mut doc,handle,0);}
    let mut frame=packet(doc.scene(),extent);frame.view.width_px=extent[0];frame.view.height_px=extent[1];
    let mut exact=WgpuRasterizer::new_native_headless(color).unwrap();exact.test.reference=true;exact.submit(frame).unwrap();
    let reference=pixels(&exact,crate::test_support::document_texture(&exact));
    let mut window=WgpuRasterizer::new_native_headless(color).unwrap();window.source_tiles.get_mut().admit(0);
    window.native_edit.as_mut().unwrap().image_pixel_bytes=Some(448<<20);window.submit(frame).unwrap();
    assert!(window.metrics().image_window_submissions>11,"deep halos must use a smaller admitted window");
    assert!(window.metrics().image_window_peak_bytes<=448<<20);
    let actual=display_pixels(&window);assert_eq!(actual.len(),reference.len());
    assert!(actual.iter().zip(&reference).all(|(a,b)|a.iter().zip(b).all(|(a,b)|(a-b).abs()<=2e-5)));
}

#[test]
fn native_and_reduced_gaussian_preparation_variants_do_not_overwrite_each_other() {
    let extent=[1029,517];let color=layer_core::color::DocumentColor{depth:SampleDepth::F32,..Default::default()};
    let mut doc=gaussian_document(extent,color,[gaussian_fixture("gaussian_blur",85.,EffectResolution::Display),original(Field::Step(256,[0.1,0.2,0.3,1.],[0.8,0.4,0.2,1.]))]);
    let mut reused=WgpuRasterizer::new_native_headless(color).unwrap();
    for (sigma,level) in [(85.,0),(85.,1),(85.,2),(85.,3),(64.,3),(64.,0),(120.,0),(120.,1),(512.,2),(85.,2),(85.,0)] {
        set_effect_at(&mut doc,0,"sigma",EffectValue::Number(sigma));let side=(1<<level) as f32;
        reused.scale_display=None;
        let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view.document_to_surface=[1./side,0.,0.,1./side,0.,0.];
        reused.submit(frame).unwrap();let mut fresh=WgpuRasterizer::new_native_headless(color).unwrap();fresh.submit(frame).unwrap();
        let error=crate::test_support::max_error(&display_pixels(&reused),&display_pixels(&fresh));
        assert!(error<2e-5,"Gaussian prepared variant sigma={sigma} level={level} differs from fresh {error}");
        let size=reused.scale_display.as_ref().unwrap().plan.size;let step=1<<level;
        assert_points(&display_pixels(&reused),size,Field::Step(256/step,[0.1,0.2,0.3,1.],[0.8,0.4,0.2,1.]),
            sigma/side,&[[256/step-1,258/step],[256/step,258/step],[512/step,258/step]]);
    }
}


#[test]
fn every_gaussian_consumer_matches_discrete_step_reference_across_the_admitted_range() {
    let extent=[517,517];let left=[0.1,0.1,0.1,1.];let right=[0.8,0.8,0.8,1.];
    let field=Field::Step(256,left,right);
    let encode=|v:f64|if v<=0.0031308 {12.92*v}else{1.055*v.powf(1./2.4)-0.055};
    let decode=|v:f64|if v<=0.04045 {v/12.92}else{((v+0.055)/1.055).powf(2.4)};
    let color=layer_core::color::DocumentColor{depth:SampleDepth::F32,..Default::default()};
    let mut r=WgpuRasterizer::new_native_headless(color).unwrap();r.test.reference=true;
    for id in CONSUMERS {
        let mut maximum=0_f64;
        for sigma in SIGMAS {
            let mut doc=gaussian_document(extent,color,[gaussian_fixture(id,sigma,EffectResolution::Native),original(field)]);
            if id!="gaussian_blur" && id!="pencil" {set_effect_at(&mut doc,0,"amount",EffectValue::Number(61.));}
            if id=="bloom" || id=="unsharp_mask" {set_effect_at(&mut doc,0,"threshold",EffectValue::Number(0.));}
            if id=="pencil" {set_effect_at(&mut doc,0,"contrast",EffectValue::Number(23.));}
            r.submit(packet(doc.scene(),extent)).unwrap();
            let image=pixels(&r,crate::test_support::document_texture(&r));
            let weights=kernel(sigma);
            for x in [1,128,255,256,384,515] {
                let p=[x,258];let input=f64::from(if x<256{left[0]}else{right[0]});
                let blurred=expected(field,p,extent,&weights)[0];
                let target=match id {
                    "gaussian_blur"=>blurred,
                    "unsharp_mask"=>{let detail=input-blurred;let t=(detail.abs()*3_f64.sqrt()/0.02).clamp(0.,1.);input+detail*0.61*t*t*(3.-2.*t)},
                    "high_pass"=>decode(0.5+0.61*(encode(input)-encode(blurred))),
                    "bloom"=>input+0.61*blurred,
                    "soft_focus"=>input+0.61*(blurred-input).max(0.),
                    "pencil"=>{let ratio=((encode(input)+0.01)/(encode(blurred)+0.01)).clamp(0.,1.);let ink=1.-ratio.powf(1.+23.*0.12);decode(0.97+(0.07-0.97)*ink)},
                    _=>unreachable!(),
                };
                let actual=image[(p[1] as u32*extent[0]+x as u32) as usize];
                let error=(f64::from(actual[0])-target).abs();maximum=maximum.max(error);
                assert!(error<2e-5,"{id} sigma={sigma} x={x} actual={actual:?} expected red={target}");
                if id=="gaussian_blur" {assert!((actual[3]-1.).abs()<2e-5,"normalized Gaussian coverage");}
                else {assert_eq!(actual[3],1.,"{id} opaque input coverage");}
            }
        }
        println!("Gaussian consumer {id} maximum selected red error={maximum}");
    }
}

}
