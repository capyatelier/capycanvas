use super::*;
use layer_core::{EffectInstance, EffectValue};

fn attach_filter(doc: &mut Document, name: &str) -> OccurrenceHandle {
    let filter = effect_occurrence(doc, EffectInstance::new(crate::tests::fixture(name).program()), name);
    if name == "threshold" {
        set_effect_value(doc, filter, "colors", EffectValue::Choice(1));
        set_effect_value(doc, filter, "transparency", EffectValue::Choice(1));
        set_effect_value(doc, filter, "alpha_threshold", EffectValue::Number(37.));
    }
    doc.artwork.occurrences.get_mut(filter).unwrap().attachment = Attachment::Effect;
    insert_occurrence(doc, filter, 0);
    filter
}

fn native_pixels(doc: &Document, view: layer_render::ViewState) -> Vec<[f32; 4]> {
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    exact.submit(FramePacket { view, blend_space: doc.composition().blend, ..packet(doc.scene(), doc.composition().size) }).unwrap();
    pixels(&exact, crate::test_support::document_texture(&exact))
}

fn assert_pixels_equal(actual: &[[f32; 4]], expected: &[[f32; 4]], width: u32, context: impl std::fmt::Debug) {
    assert_eq!(actual.len(), expected.len(), "{context:?} pixel count");
    let mut changed = 0;
    let mut first = None;
    for (index, (a, b)) in actual.iter().zip(expected).enumerate() {
        if a != b { changed += 1; first.get_or_insert([index as u32 % width, index as u32 / width]); }
    }
    assert_eq!(changed, 0, "{context:?}: first differing coordinate={first:?}");
}

#[test]
fn native_alpha_reuse_preserves_noops_and_forced_authored_changes() {
    let extent = [517, 259];
    for name in ["brightness_to_opacity", "threshold"] { for level in [0, 2] {
        let mut doc = document_at(extent);
        paint_mut(&mut doc, 0).base = (Some(rgba8_source(extent, |_, _| [0, 0, 0, 255]))).map(|source|layer_core::authored::PaintBase::new(source.into()));
        let target = source_at(&doc, 0);
        let owner = doc.scene().order()[0];
        let filter = attach_filter(&mut doc, name);
        let mut view = crate::test_support::view(extent);
        let scale = 1. / (1 << level) as f32;
        view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        let mut cached = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut rebuilt = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let frame = FramePacket { view, composite_all: false, ..packet(doc.scene(), extent) };
        for r in [&mut cached, &mut rebuilt] { r.submit(frame).unwrap(); }
        let initial = display_pixels(&cached);
        for (step, color) in [[0., 0., 0., 1.], [1., 1., 1., 0.], [1., 1., 1., 1.]].into_iter().enumerate() {
            let mut dab = crate::tests::test_dab([255.5, 127.5], color, 0.8);
            dab.radii = [13.25, 11.75];
            let batch = dab_batch(target, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
            let contact = FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame };
            cached.submit(contact).unwrap();
            rebuilt.submit(FramePacket { composite_all: true, ..contact }).unwrap();
            let actual = display_pixels(&cached);
            assert!(actual == display_pixels(&rebuilt), "{name} level {level} contact {step}");
            if step < 2 { assert!(actual == initial, "{name} must preserve an unchanged native store"); }
            else { assert!(actual != initial, "{name} must publish changed native pixels"); }
        }
        cached = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        cached.submit(frame).unwrap(); cached.wait_idle().unwrap(); cached.submit(frame).unwrap();
        for state in 0..5 {
            match state {
                0 => paint_mut(&mut doc, 1).base = (Some(rgba8_source(extent, |x, y| {
                    let gray = if (x / 3 + y / 2) % 2 == 0 { 126 } else { 130 };
                    [gray, gray, gray, if (x / 13 + y / 7) % 2 == 0 { 37 } else { 193 }]
                }))).map(|source|layer_core::authored::PaintBase::new(source.into())),
                1 => doc.artwork.occurrences.get_mut(owner).unwrap().opacity = 0.43,
                2 => { coverage_mask(&mut doc, filter, layer_core::Point { x: 7., y: -3. }, Some(layer_core::Selection::polygon(vec![
                    layer_core::Point { x: 11., y: 23. }, layer_core::Point { x: 414., y: 23. },
                    layer_core::Point { x: 414., y: 204. }, layer_core::Point { x: 11., y: 204. },
                ]).unwrap())); },
                3 => doc.artwork.occurrences.get_mut(filter).unwrap().mask.as_mut().unwrap().inverted = true,
                4 => composition_mut(&mut doc).blend = layer_core::BlendSpace::Linear,
                _ => unreachable!(),
            }
            let changed = FramePacket { view, composite_all: false, blend_space: doc.composition().blend, ..packet(doc.scene(), extent) };
            cached.submit(changed).unwrap();
            assert_settled(&mut cached, changed, &native_pixels(&doc, view));
        }
    }}
}

#[test]
fn compact_contribution_omits_unused_paint_and_matches_bound_source() {
    let extent=[517,259];
    for name in ["brightness_to_opacity","threshold"] {for blend in layer_core::BlendSpace::ALL {
        let mut doc=document_at(extent);
        composition_mut(&mut doc).blend=blend;
        paint_mut(&mut doc,0).base = (Some(rgba8_source(extent,|x,y|[(x%251) as u8,(y%239) as u8,173,211]))).map(|source|layer_core::authored::PaintBase::new(source.into()));
        let target=source_at(&doc,0);
        attach_filter(&mut doc,name);
        let mut frame=packet(doc.scene(),extent);
        frame.composite_all=false;frame.blend_space=blend;
        frame.view.document_to_surface=[0.25,0.,0.,0.25,0.,0.];
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        r.submit(frame).unwrap();
        let mut dab=crate::tests::test_dab([125.5,126.5],[0.031,0.13,0.29,0.63],0.8);
        dab.radii=[13.25,11.75];
        let mut persistent=dab_batch(target,crate::layer_tests::preset_style(DefaultBrushPreset::GPen),dab.bounds());
        persistent.style.blend_space=blend;persistent.stroke_end=false;
        r.submit(FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&persistent),..frame}).unwrap();
        dab.center=layer_core::Point {x:129.5,y:127.5};
        let mut preview=persistent.clone();preview.kind=DabBatchKind::Preview;preview.stroke_start=false;preview.damage=dab.bounds();
        r.submit(FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&preview),..frame}).unwrap();
        r.wait_idle().unwrap();
        assert!(r.compact_preview_contribution(&preview),"{name} {blend:?} must exercise actual contribution execution");
        let coverage=||r.paint_layers.iter().find(|p|p.id==target).unwrap().coverage_pages.iter().find(|p|p.coordinate==[0,0]).unwrap().active();
        let key=( [r.dab_buffer.clone(),r.dry_records.binding().buffer.clone()],
            std::array::from_fn(|i|if i==9 {coverage().view.clone()}else {r.empty_view.clone()}) );
        coverage().material_input.get(key,||panic!("{name} {blend:?}: actual contribution must bind empty paint and committed coverage"));
        let actual=crate::layer_tests::page_bytes(&r,&r.preview_page([0,0]).unwrap().active().texture);
        let reference=create_page_surface(&r.device,&r.texture_layout,&r.sampler,[PAGE_SIZE>>r.preview_level;2],r.device.working_format(),"bound paint contribution oracle");
        let output=r.pipelines.dry_display.output(&r,&reference,None,target,[0,0]);
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        r.preview_contribution=false;
        let source=r.material_source_binding(0,&preview,std::slice::from_ref(&dab),[0,0],false,&mut encoder).unwrap();
        r.preview_contribution=true;
        r.encode_dry_material_jobs(&mut encoder,0,&preview,&[(output,source,[0,0],false,0,false)]);
        r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
        assert_eq!(actual,crate::layer_tests::page_bytes(&r,&reference.texture),"{name} {blend:?}: omitted original must preserve every prediction bit");
        for (level,contribution,preset) in [(1,false,DefaultBrushPreset::GPen),(0,true,DefaultBrushPreset::GPen),(1,true,DefaultBrushPreset::CloneStamp)] {
            r.preview_level=level;r.preview_contribution=contribution;
            let mut fallback=preview.clone();fallback.style=crate::layer_tests::preset_style(preset);fallback.style.blend_space=blend;
            assert!(!r.compact_preview_contribution(&fallback));
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            r.material_source_binding(0,&fallback,std::slice::from_ref(&dab),[0,0],false,&mut encoder).unwrap();
            let layer=r.paint_layers.iter().find(|p|p.id==target).unwrap();
            let color=layer.pages.iter().find(|p|p.coordinate==[0,0]).unwrap().active();
            let coverage=layer.coverage_pages.iter().find(|p|p.coordinate==[0,0]).unwrap().active();
            let key=([r.dab_buffer.clone(),r.dry_records.binding().buffer.clone()],std::array::from_fn(|i|match i {4=>color.view.clone(),9=>coverage.view.clone(),_=>r.empty_view.clone()}));
            coverage.material_input.get(key,||panic!("{name} {blend:?}: fallback {level} {contribution} {preset:?} must retain paint and coverage"));
            r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
        }
    }}
}

#[test]
fn native_alpha_reuse_admits_warm_pages_beside_cold_output_pages() {
    let extent=[517,259];
    for name in ["brightness_to_opacity","threshold"] {
        let mut doc=document_at(extent);
        let target=source_at(&doc,0);
        attach_filter(&mut doc,name);
        let mut frame=packet(doc.scene(),extent);
        frame.composite_all=false;
        frame.view.document_to_surface=[0.25,0.,0.,0.25,0.,0.];
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        r.submit(frame).unwrap();
        let mut dab=crate::tests::test_dab([258.5,129.5],[0.,0.,0.,0.],1.);
        dab.radii=[260.,131.];
        let batch=dab_batch(target,crate::layer_tests::preset_style(DefaultBrushPreset::GPen),dab.bounds());
        for _ in 0..2 {
            r.submit(FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
            r.wait_idle().unwrap();
        }
        let flags=r.changed_cells.as_ref().unwrap().reusable(target,[0,0]).unwrap();
        let bytes=pollster::block_on(crate::local_tone::read_buffer_async(&r.device,&r.queue,flags)).unwrap();
        assert!(bytes[16..].chunks_exact(4).all(|v|u32::from_le_bytes(v.try_into().unwrap())==0));
        let mut cache=r.scale_display.take().unwrap();
        assert_eq!(cache.plan.level,2);
        assert!(cache.evaluation==Evaluation::Display);
        cache.pixels=Default::default();
        cache.valid.clear();
        cache.pixels.ensure_root(&r,cache.plan,"partial retained effect output");
        let image=cache.pixels.root().unwrap().clone();
        let output=Target {view:image.view.clone(),slot:Some(Slot::Root),plan:image.plan};
        let scratch=Target {slot:Some(Slot::Cache(0)),..output.clone()};
        assert!(cache.initialized_pages(&scratch).is_none());
        let handle=doc.scene().order()[0];
        let mut scene=r.scene.take().unwrap();
        let warm=page_rect([0,0]);
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        scene.prepare_region_in_frame(&mut r,frame,warm.into(),PixelRect::EMPTY,false,&mut encoder).unwrap();
        scene.capture_prepared_regions_reuse(&mut r,frame,&image,&[warm],scene::Output::EffectComposite(handle),false,None,
            cache.initialized_pages(&output),&mut encoder).unwrap();
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        r.wait_idle().unwrap();
        cache.valid.insert([0,0]);
        let candidates=r.metrics.native_effect_reuse_candidate_cells;
        let forced=r.metrics.native_effect_forced_cells;
        let full=PixelRect::full(extent);
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        scene.prepare_region_in_frame(&mut r,frame,full.into(),PixelRect::EMPTY,false,&mut encoder).unwrap();
        scene.capture_prepared_regions_reuse(&mut r,frame,&image,&[full],scene::Output::EffectComposite(handle),false,None,
            cache.initialized_pages(&output),&mut encoder).unwrap();
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        r.wait_idle().unwrap();
        assert_eq!(r.metrics.native_effect_reuse_candidate_cells-candidates,4096,"{name}: warm page must stay eligible beside cold pages");
        assert_eq!(r.metrics.native_effect_forced_cells-forced,u64::from(image.plan.size[0])*u64::from(image.plan.size[1])-4096,
            "{name}: every cold output cell must receive full evaluation");
        let reference=Image::new(&r,image.plan,"full effect capture reference");
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        scene.prepare_region_in_frame(&mut r,frame,full.into(),PixelRect::EMPTY,false,&mut encoder).unwrap();
        scene.capture_prepared_regions(&mut r,frame,&reference,&[full],scene::Output::EffectComposite(handle),false,None,&mut encoder).unwrap();
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        r.wait_idle().unwrap();
        assert_pixels_equal(&pixels(&r,&image.texture),&pixels(&r,&reference.texture),image.plan.size[0],name);
        r.scene=Some(scene);
        r.scale_display=Some(cache);
    }
}

#[test]
fn native_alpha_reuse_matches_canonical_paint_erase_and_history() {
    use layer_engine::{CanvasEngine, InstantFeedbackConfig, PenEvent, PenPhase, SampleFlags, ViewTransform, input_queue};
    let extent = [517, 259];
    for name in ["brightness_to_opacity", "threshold"] { for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32] { for blend in layer_core::BlendSpace::ALL {
        if matches!(depth, SampleDepth::F16 | SampleDepth::F32) && blend == layer_core::BlendSpace::Perceptual { continue; }
        let mut doc = document_at(extent);
        composition_mut(&mut doc).color.depth = depth;
        composition_mut(&mut doc).blend = blend;
        paint_mut(&mut doc, 0).base = (Some(rgba8_source(extent, |x, y| {
            let gray = if (x / 3 + y / 2) % 2 == 0 { 126 } else { 130 };
            [gray, gray, gray, if (x / 13 + y / 7) % 2 == 0 { 37 } else { 193 }]
        }))).map(|source|layer_core::authored::PaintBase::new(source.into()));
        let owner = doc.scene().order()[0];
        let target = source_at(&doc, 0);
        attach_filter(&mut doc, name);
        doc.working.occurrence = Some(owner); doc.working.target = Some(target);
        let mut view = crate::test_support::view([160, 90]);
        view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        let (mut input, consumer) = input_queue(64);
        let renderer = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut engine = CanvasEngine::new(renderer, doc, consumer, view, ViewTransform { revision: 0, surface_to_document: [4., 0., 0., 4., 0., 0.] }).unwrap();
        engine.set_instant_feedback(InstantFeedbackConfig { enabled: true, prediction_horizon_micros: 16_000, ..Default::default() }).unwrap();
        let drain = |engine: &mut CanvasEngine<WgpuRasterizer>, settle: bool| {
            let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
            loop {
                engine.render_frame().unwrap(); engine.backend_mut().wait_idle().unwrap();
                if !engine.has_pending_input() && (!settle || !engine.backend().has_pending_work()) { break; }
                assert!(std::time::Instant::now() < deadline);
            }
        };
        drain(&mut engine, true);
        let before = native_pixels(engine.document(), view);
        let candidates = engine.backend().metrics.native_effect_reuse_candidate_cells;
        let mut painted = Vec::new();
        for (stroke, tool) in [layer_core::StrokeTool::Brush, layer_core::StrokeTool::Eraser].into_iter().enumerate() {
            let mut brush = layer_core::default_brush(DefaultBrushPreset::GPen);
            brush.diameter = 31.5; brush.color_rgba_linear = [0.017, 0.031, 0.13, 0.63];
            engine.set_brush(brush).unwrap(); engine.set_tool(tool);
            for i in 0..=8 {
                let phase = if i == 0 { PenPhase::Down } else if i == 8 { PenPhase::Up } else { PenPhase::Move };
                let sequence = (stroke * 9 + i) as u64 + 1;
                input.push(PenEvent { timestamp_ns: sequence * 8_333_333, pressure: 0.8,
                    ..crate::test_support::pen(sequence, phase, [(235.5 + i as f32 * 4.) / 4., (117.5 + i as f32 * 2.) / 4.], SampleFlags::PRIMARY) }).unwrap();
                drain(&mut engine, false);
            }
            drain(&mut engine, true);
            assert_eq!(engine.metrics().committed_strokes, stroke as u64 + 1);
            let canonical = native_pixels(engine.document(), view);
            let error = quality(&display_pixels(engine.backend()), &canonical, engine.backend().scale_display.as_ref().unwrap().plan);
            assert!(error[2] < 2e-5, "{name} {depth:?} {blend:?} {tool:?}: {error:?}");
            if stroke == 0 { assert!(canonical != before, "{name} paint must change native pixels"); painted = canonical; }
            else { assert!(canonical != painted, "{name} erase must change native pixels"); }
        }
        assert!(engine.backend().metrics.native_effect_reuse_candidate_cells > candidates,
            "{name} {depth:?} {blend:?} real strokes must exercise eligible reuse: candidates={}, forced={}",
            engine.backend().metrics.native_effect_reuse_candidate_cells, engine.backend().metrics.native_effect_forced_cells);
        for (redo, expected) in [(false, &painted), (false, &before), (true, &painted)] {
            assert!(if redo { engine.redo().unwrap() } else { engine.undo().unwrap() });
            drain(&mut engine, true);
            assert_pixels_equal(&native_pixels(engine.document(), view), expected, extent[0], (name, depth, blend, redo));
            let error = quality(&display_pixels(engine.backend()), expected, engine.backend().scale_display.as_ref().unwrap().plan);
            assert!(error[2] < 2e-5, "{name} {depth:?} {blend:?} history redo={redo}: {error:?}");
        }
    }}}
}

#[test]
fn native_alpha_reuse_auxiliaries_retire_with_predictions_sources_and_scale() {
    let extent = [1029, 517];
    for name in ["brightness_to_opacity", "threshold"] { for level in [2, 3, 4] {
        let mut doc = document_at(extent);
        let owner = doc.scene().order()[0];
        let target = source_at(&doc, 0);
        let filter = attach_filter(&mut doc, name);
        let replacement = paint_occurrence(&mut doc, "replacement", Some(rgba8_source(extent, |_, _| [70, 120, 210, 255])));
        let mut view = crate::test_support::view([300, 180]);
        let scale = 1. / (1 << level) as f32;
        view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let disabled = r.changed_cells.as_ref().unwrap().storage_bytes();
        let frame = FramePacket { view, composite_all: false, ..packet(doc.scene(), extent) };
        for _ in 0..2 { r.submit(frame).unwrap(); r.wait_idle().unwrap(); }
        let mut seed = crate::tests::test_dab([125.5, 126.5], [0.1, 0.1, 0.1, 0.6], 0.8);
        seed.radii = [13.25, 11.75];
        let batch = dab_batch(target, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), seed.bounds());
        r.submit(FramePacket { dabs: std::slice::from_ref(&seed), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
        for _ in 0..2 { r.submit(frame).unwrap(); r.wait_idle().unwrap(); }
        let warm = r.changed_cells.as_ref().unwrap().storage_bytes();
        assert!(warm > disabled, "{name} must exercise admitted reuse auxiliaries");
        for contact in 0..8 {
            let position = if contact % 2 == 0 { [125.5, 126.5] } else { [901.5, 387.5] };
            let mut dab = crate::tests::test_dab(position, [0.2, 0.1, 0.3, 0.25], 0.8);
            dab.radii = [13.25, 11.75];
            let mut batch = dab_batch(target, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
            batch.kind = DabBatchKind::Preview; batch.stroke_end = false;
            r.submit(FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
            let auxiliary = r.changed_cells.as_ref().unwrap().storage_bytes();
            assert!(auxiliary <= 32 * 1024 * 1024 + disabled, "{name} auxiliary admission: {auxiliary}");
            let total = resident_bytes_with_pool(r.scale_display.as_ref().unwrap(), r.scene.as_ref().unwrap()) + auxiliary;
            assert!(total <= CACHE_BYTES, "{name} admitted resident cache including auxiliaries: {total}");
        }
        for _ in 0..2 { r.submit(frame).unwrap(); r.wait_idle().unwrap(); }
        assert_eq!(r.changed_cells.as_ref().unwrap().storage_bytes(), warm, "{name} retired prediction-only flags must release their bytes");
        doc.artwork.occurrences.get_mut(filter).unwrap().attachment = Attachment::None;
        set_root_entries(&mut doc, vec![replacement]);
        for _ in 0..2 { r.submit(FramePacket { view, composite_all: false, ..packet(doc.scene(), extent) }).unwrap(); r.wait_idle().unwrap(); }
        assert_eq!(r.changed_cells.as_ref().unwrap().storage_bytes(), disabled, "{name} unused source auxiliaries must retire");
        set_root_entries(&mut doc, vec![filter, owner]);
        doc.artwork.occurrences.get_mut(filter).unwrap().attachment = Attachment::Effect;
        reindex(&mut doc);
        let frame = FramePacket { view, composite_all: false, ..packet(doc.scene(), extent) };
        for _ in 0..2 { r.submit(frame).unwrap(); r.wait_idle().unwrap(); }
        r.submit(FramePacket { dabs: std::slice::from_ref(&seed), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
        r.submit(frame).unwrap(); r.wait_idle().unwrap();
        assert!(r.changed_cells.as_ref().unwrap().storage_bytes() > disabled);
        view.document_to_surface = [1., 0., 0., 1., 0., 0.];
        r.submit(FramePacket { view, composite_all: false, ..packet(doc.scene(), extent) }).unwrap();
        assert_eq!(r.scale_display.as_ref().unwrap().plan.level, 0);
        assert_eq!(r.changed_cells.as_ref().unwrap().storage_bytes(), disabled, "{name} native presentation must release reduced-cell auxiliaries");
    }}
}

#[test]
fn native_alpha_reuse_real_unchanged_stores_leave_gpu_flags_clear() {
    use layer_engine::{CanvasEngine, InstantFeedbackConfig, PenEvent, PenPhase, SampleFlags, ViewTransform, input_queue};
    use layer_core::raster::*;
    let extent = [517, 259];
    for name in ["brightness_to_opacity", "threshold"] { for level in [2, 3, 4] {
        let mut doc = document_at(extent);
        let bytes: Vec<_> = (0..PAGE_SIZE * PAGE_SIZE).flat_map(|_| [0, 0, 0, 255]).collect();
        let tile = RasterTile::backed(TileBlob::encode(RasterPlane::Color.descriptor(doc.composition().color), &bytes).unwrap());
        paint_mut(&mut doc, 0).base = None;
        paint_mut(&mut doc, 0).raster = RasterRevision::backed(RasterData {
            tiles: [(TileKey { plane: RasterPlane::Color, coordinate: [0, 0] }, tile)].into(), watercolor: None,
        });
        let owner = doc.scene().order()[0];
        let target = source_at(&doc, 0);
        attach_filter(&mut doc, name);
        doc.working.occurrence = Some(owner); doc.working.target = Some(target);
        let mut view = crate::test_support::view([160, 90]);
        let scale = 1. / (1 << level) as f32;
        view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        let (mut input, consumer) = input_queue(64);
        let renderer = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut engine = CanvasEngine::new(renderer, doc, consumer, view, ViewTransform { revision: 0, surface_to_document: [1. / scale, 0., 0., 1. / scale, 0., 0.] }).unwrap();
        engine.set_instant_feedback(InstantFeedbackConfig { enabled: true, prediction_horizon_micros:16_000, ..Default::default() }).unwrap();
        let mut brush = layer_core::default_brush(DefaultBrushPreset::GPen);
        brush.diameter = 31.5; brush.color_rgba_linear = [0., 0., 0., 0.];
        engine.set_brush(brush).unwrap();
        let drain = |engine: &mut CanvasEngine<WgpuRasterizer>, settle: bool| {
            let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
            loop {
                engine.render_frame().unwrap(); engine.backend_mut().wait_idle().unwrap();
                if !engine.has_pending_input() && (!settle || !engine.backend().has_pending_work()) { break; }
                assert!(std::time::Instant::now() < deadline);
            }
        };
        drain(&mut engine, true);
        let before = display_pixels(engine.backend());
        let candidates = engine.backend().metrics.native_effect_reuse_candidate_cells;
        for i in 0..=4 {
            let phase = if i == 0 { PenPhase::Down } else if i == 4 { PenPhase::Up } else { PenPhase::Move };
            input.push(PenEvent { timestamp_ns: (i as u64 + 1) * 8_333_333, pressure: 0.8,
                ..crate::test_support::pen(i as u64 + 1, phase, [(100. + i as f32 * 8.) * scale, 100. * scale], SampleFlags::PRIMARY) }).unwrap();
            let timestamp=(i as u64+1)*8_333_333;
            engine.render_frame_for(timestamp,timestamp+16_000_000).unwrap();engine.backend_mut().wait_idle().unwrap();
            assert!(!engine.has_pending_input());
            assert_pixels_equal(&display_pixels(engine.backend()), &before, engine.backend().scale_display.as_ref().unwrap().plan.size[0], (name, phase));
            if phase == PenPhase::Down {assert!(engine.backend().changed_cells.as_ref().unwrap().reusable(target,[0,0]).is_none(),"first prediction must force cold work");}
            if phase == PenPhase::Move {
                let r = engine.backend();
                let flags = r.changed_cells.as_ref().unwrap().reusable(target, [0, 0]).expect("warm real Engine prediction must be eligible for reuse");
                let page=r.preview_page([0,0]).expect("real enabled prediction must remain live");
                assert_eq!(page.primary.preview.get(),Some((target,[0,0])));assert!(!page.active_secondary);
                page.primary.material_output.get((r.style_buffer.clone(),page.primary.view.clone(),None,false,Some(flags.clone())),||panic!("real warm Engine prediction must execute actual tracked Display output"));
                let bytes = pollster::block_on(crate::local_tone::read_buffer_async(&r.device, &r.queue, flags)).unwrap();
                let changed = bytes[16..].chunks_exact(4).filter(|cell| u32::from_le_bytes((*cell).try_into().unwrap()) != 0).count();
                assert_eq!(changed, 0, "{name} unchanged warm real stores must leave every GPU dirty-cell flag clear: candidates={}, forced={}",
                    r.metrics.native_effect_reuse_candidate_cells, r.metrics.native_effect_forced_cells);
            }
        }
        drain(&mut engine, true);
        assert!(engine.backend().metrics.native_effect_reuse_candidate_cells > candidates,
            "{name} unchanged real contact must execute eligible retained reuse");
        assert_pixels_equal(&display_pixels(engine.backend()), &before, engine.backend().scale_display.as_ref().unwrap().plan.size[0], name);
        let document = engine.document().clone();
        assert_settled(engine.backend_mut(), FramePacket { view, ..packet(document.scene(), extent) }, &native_pixels(&document, view));
    }}
}

#[test]
fn native_alpha_reuse_raw_mask_paint_forces_unchanged_paint_cells() {
    let extent = [517, 259];
    for name in ["brightness_to_opacity", "threshold"] { for blend in layer_core::BlendSpace::ALL {
        let mut doc = document_at(extent);
        composition_mut(&mut doc).blend = blend;
        let paint = source_at(&doc, 0);
        let filter = attach_filter(&mut doc, name);
        let mask = coverage_mask(&mut doc, filter, Default::default(), None);
        doc.artwork.coverage.get_mut(mask).unwrap().default_coverage = 0.;
        let mut frame = packet(doc.scene(), extent);
        frame.composite_all = false; frame.blend_space = blend;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        let mut cached = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut rebuilt = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        for r in [&mut cached, &mut rebuilt] { r.submit(frame).unwrap(); }
        let mut dab = crate::tests::test_dab([125.5, 126.5], [0.1, 0.1, 0.1, 0.6], 0.8);
        dab.radii = [13.25, 11.75];
        let paint_batch = dab_batch(paint, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        for r in [&mut cached, &mut rebuilt] {
            r.submit(FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&paint_batch), ..frame }).unwrap();
            for _ in 0..2 { r.submit(frame).unwrap(); r.wait_idle().unwrap(); }
        }
        assert!(cached.changed_cells.as_ref().unwrap().buffer(paint, [0, 0]).is_some(), "{name} retained paint flags must be admitted before the mask write");
        let before = display_pixels(&cached);
        let candidates = cached.metrics.native_effect_reuse_candidate_cells;
        let forced = cached.metrics.native_effect_forced_cells;
        dab.color_rgba_linear = [1.; 4];
        let mask_batch = dab_batch(SourceTarget::Coverage(mask), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        let changed = FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&mask_batch), ..frame };
        cached.submit(changed).unwrap();
        rebuilt.submit(FramePacket { composite_all: true, ..changed }).unwrap();
        let actual = display_pixels(&cached); let expected = display_pixels(&rebuilt);
        let error = crate::test_support::max_error(&actual, &expected);
        assert!(error < 2e-5, "{name} {blend:?} raw mask painting must match full recomposition: error={error}");
        assert!(actual != before, "{name} {blend:?} mask painting must visibly change the filtered artwork");
        assert!(cached.changed_cells.as_ref().unwrap().reusable(paint, [0, 0]).is_none(), "{name} mask writes cannot reuse unchanged paint-cell flags");
        assert_eq!(cached.metrics.native_effect_reuse_candidate_cells, candidates, "{name} mask writes must force native-effect evaluation");
        assert!(cached.metrics.native_effect_forced_cells > forced, "{name} mask write must exercise forced native-effect work");
        assert_presentation_mip(&cached);
    }}
}

#[test]
fn native_alpha_reuse_captured_phase_change_forces_same_elapsed_recomposition() {
    let extent = [517, 259];
    let mut doc = document_at(extent);
    let paint = source_at(&doc, 0);
    let mut program = (*crate::tests::fixture("brightness_to_opacity").program()).clone();
    program.id = "captured_native_alpha".into();
    program.entry = "captured_phase_alpha".into();
    program.wgsl = "fn captured_phase_alpha(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(c.rgb,c.a*fx_time(b));}".into();
    let filter = effect_occurrence(&mut doc, EffectInstance::new(Arc::new(program)), "Captured native alpha");
    doc.artwork.occurrences.get_mut(filter).unwrap().attachment = Attachment::Effect;
    insert_occurrence(&mut doc, filter, 0);
    let effect = effect_handle(&doc, filter);
    let previous = EvaluationContext { elapsed: 8., phases: vec![(effect, 0.25)].into() };
    let changed = EvaluationContext { elapsed: 8., phases: vec![(effect, 0.75)].into() };
    let mut frame = packet(doc.scene().with_context(&previous), extent);
    frame.time_seconds = 8.; frame.composite_all = false;
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    let mut cached = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut dab = crate::tests::test_dab([125.5, 126.5], [0.1, 0.1, 0.1, 0.6], 0.8);
    dab.radii = [13.25, 11.75];
    let batch = dab_batch(paint, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
    for r in [&mut cached, &mut exact] {
        r.submit(frame).unwrap();
        r.submit(FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
        for _ in 0..2 { r.submit(frame).unwrap(); r.wait_idle().unwrap(); }
    }
    assert!(scene::same_metadata(&cached, frame));
    assert!(cached.changed_cells.as_ref().unwrap().reusable(paint, [0, 0]).is_some());
    let before = display_pixels(&cached);
    let candidates = cached.metrics.native_effect_reuse_candidate_cells;
    let changed = FramePacket { scene: doc.scene().with_context(&changed), ..frame };
    assert!(!scene::same_metadata(&cached, changed), "a captured phase change at the same elapsed time changes effect inputs");
    cached.submit(changed).unwrap();
    exact.submit(FramePacket { composite_all: true, ..changed }).unwrap();
    let native = pixels(&exact, crate::test_support::document_texture(&exact));
    let error = quality(&display_pixels(&cached), &native, cached.scale_display.as_ref().unwrap().plan);
    assert!(error[2] < 2e-5, "changed captured phase must match native recomposition: {error:?}");
    assert!(display_pixels(&cached) != before, "captured phase must change native alpha");
    assert_eq!(cached.metrics.native_effect_reuse_candidate_cells, candidates);
    assert!(cached.changed_cells.as_ref().unwrap().reusable(paint, [0, 0]).is_none());
}

#[test]
fn native_alpha_reuse_untracked_materials_have_no_writable_cell_binding() {
    use crate::dry_material::Target;
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let (_texture, output) = create_color_target(&r.device, [PAGE_SIZE; 2], "material binding output");
    for (target, name, pipelines) in [
        (Target::Exact, "exact", &r.pipelines.dry_material),
        (Target::InPlace, "in place", r.pipelines.dry_in_place.as_ref().unwrap()),
        (Target::Display, "display", &r.pipelines.dry_display),
        (Target::DisplayTracked,"tracked display",r.pipelines.dry_display_tracked.as_ref().unwrap()),
        (Target::Tracked, "tracked", r.pipelines.dry_tracked.as_ref().unwrap()),
    ] {
        let source = crate::dry_material::shader_source(&r.device, target, include_str!("../../material_brush.wgsl"));
        let module = naga::front::wgsl::parse_str(&source).unwrap();
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&module).unwrap();
        let writable: Vec<_> = module.global_variables.iter().filter_map(|(_, variable)| {
            matches!(variable.space, naga::AddressSpace::Storage { access } if access.contains(naga::StorageAccess::STORE))
                .then_some((variable.name.as_deref(), variable.binding.as_ref().map(|binding| (binding.group, binding.binding))))
        }).collect();
        let expected = if matches!(target,Target::Tracked|Target::DisplayTracked) { vec![(Some("changed_cells"), Some((0, 3)))] } else { vec![] };
        assert_eq!(writable, expected, "{name} writable buffers");
        let pipeline = pipelines.kernels[0].compile();
        let layout = pipeline.get_bind_group_layout(0);
        let mut entries = vec![
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &r.style_buffer, offset: 0, size: NonZeroU64::new(mem::size_of::<StyleGpu>() as u64),
            }) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&output) },
        ];
        if matches!(target,Target::Tracked|Target::DisplayTracked) { entries.push(wgpu::BindGroupEntry {
            binding: 3, resource: r.changed_cells.as_ref().unwrap().disabled.as_entire_binding(),
        }); }
        r.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("material cell binding contract"), layout: &layout, entries: &entries });
    }
}

#[test]
fn native_alpha_reuse_idle_preparation_preserves_flags_until_the_next_contact() {
    let extent = [517, 259];
    for name in ["brightness_to_opacity", "threshold"] {
        let mut doc = document_at(extent);
        let target = source_at(&doc, 0);
        attach_filter(&mut doc, name);
        let mut frame = packet(doc.scene(), extent);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        r.submit(frame).unwrap();
        let mut dab = crate::tests::test_dab([125.5, 126.5], [0.1, 0.1, 0.1, 0.6], 0.8);
        dab.radii = [13.25, 11.75];
        let batch = dab_batch(target, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        r.submit(FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
        for _ in 0..2 { r.submit(frame).unwrap(); r.wait_idle().unwrap(); }
        let before = display_pixels(&r);
        let flags = r.changed_cells.as_ref().unwrap().buffer(target, [0, 0]).unwrap().clone();
        let read = |r: &WgpuRasterizer| pollster::block_on(crate::local_tone::read_buffer_async(&r.device, &r.queue, &flags)).unwrap();
        let prior = read(&r);
        assert!(prior[16..].chunks_exact(4).any(|cell| u32::from_le_bytes(cell.try_into().unwrap()) != 0), "{name} actual tracked stores must mark changed cells");
        let mut cells = r.changed_cells.take().unwrap();
        r.document_damage.clear(); r.transform_damage.clear();
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        cells.prepare(&r, frame, &[], false, &mut encoder);
        encoder.submit(&r.queue);
        r.queue.write_buffer(&flags, 16, &prior[16..]);
        cells.force(target, PixelRect::new(100, 100, 150, 150));
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        cells.prepare(&r, frame, &[], true, &mut encoder);
        encoder.submit(&r.queue);
        assert_eq!(cells.buffer(target, [0, 0]), Some(&flags), "{name} idle preparation must preserve the admitted buffer");
        assert!(cells.reusable(target, [0, 0]).is_none(), "{name} background work must retain forced cells");
        assert_eq!(read(&r), prior, "{name} background work must retain tracked GPU flags");
        r.changed_cells = Some(cells);
        let candidates = r.metrics.native_effect_reuse_candidate_cells;
        dab.color_rgba_linear = [0.; 4];
        r.submit(FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
        assert!(r.metrics.native_effect_reuse_candidate_cells > candidates, "{name} next contact must restore eligible reuse");
        assert!(r.changed_cells.as_ref().unwrap().reusable(target, [0, 0]).is_some());
        assert!(read(&r)[16..].chunks_exact(4).all(|cell| u32::from_le_bytes(cell.try_into().unwrap()) == 0), "{name} next unchanged contact must clear prior flags before stores");
        assert_pixels_equal(&display_pixels(&r), &before, r.scale_display.as_ref().unwrap().plan.size[0], name);
        let plan = r.scale_display.as_ref().unwrap().plan;
        r.submit(FramePacket { view: crate::test_support::view(extent), composite_all: true, ..frame }).unwrap();
        let native = pixels(&r, crate::test_support::document_texture(&r));
        let error = quality(&before, &native, plan);
        assert!(error[2] < 2e-5, "{name} preserved idle flags must retain native paint output: {error:?}");
    }
}

#[test]
fn native_alpha_reuse_direct_edged_flow_contacts_force_native_alpha_recomposition() {
    let extent = [517, 259];
    for name in ["brightness_to_opacity", "threshold"] { for burnt in [false, true] {
        let mut doc = document_at(extent);
        composition_mut(&mut doc).color.depth = SampleDepth::F32;
        composition_mut(&mut doc).blend = layer_core::BlendSpace::Linear;
        let target = source_at(&doc, 0);
        attach_filter(&mut doc, name);
        let mut frame = packet(doc.scene(), extent);
        frame.composite_all = false; frame.blend_space = layer_core::BlendSpace::Linear;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        let mut cached = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut native = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        native.test.reference = true;
        let mut dab = crate::tests::test_dab([125.5, 126.5], [0.1, 0.1, 0.1, 0.6], 0.8);
        dab.radii = [21.25, 19.5];
        let mut seed_style = crate::layer_tests::preset_style(DefaultBrushPreset::GPen);
        seed_style.blend_space = frame.blend_space;
        let seed = dab_batch(target, seed_style, dab.bounds());
        for r in [&mut cached, &mut native] {
            r.submit(frame).unwrap();
            r.submit(FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&seed), ..frame }).unwrap();
            for _ in 0..2 { r.submit(frame).unwrap(); r.wait_idle().unwrap(); }
        }
        assert!(cached.changed_cells.as_ref().unwrap().buffer(target, [0, 0]).is_some());
        let before = display_pixels(&cached);
        let candidates = cached.metrics.native_effect_reuse_candidate_cells;
        let forced = cached.metrics.native_effect_forced_cells;
        let mut style = crate::layer_tests::preset_style(DefaultBrushPreset::Airbrush);
        style.blend_space = frame.blend_space;
        style.rendering.accumulation = BrushAccumulation::Flow;
        style.rendering.edge_after_stroke = false;
        style.rendering.wet_edge = if burnt { 0. } else { 0.8 };
        style.rendering.burnt_edge = if burnt { 0.8 } else { 0. };
        dab.color_rgba_linear = [0.8, 0.9, 0.4, 0.9];
        let batch = dab_batch(target, style, dab.bounds());
        assert!(BrushPassPlan::for_device(&batch.style, &cached.device).direct.is_some());
        let contact = FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame };
        cached.submit(contact).unwrap(); native.submit(contact).unwrap();
        let actual = display_pixels(&cached);
        assert!(actual != before, "{name} burnt={burnt} direct paint must visibly change native alpha output");
        let expected = pixels(&native, crate::test_support::document_texture(&native));
        let error = quality(&actual, &expected, cached.scale_display.as_ref().unwrap().plan);
        assert!(error[2] < 2e-5, "{name} burnt={burnt} direct paint must match native recomposition: {error:?}");
        assert!(!cached.in_place_dry_material(&batch), "direct fragment stores cannot claim tracked compute output");
        assert!(cached.changed_cells.as_ref().unwrap().reusable(target, [0, 0]).is_none());
        assert_eq!(cached.metrics.native_effect_reuse_candidate_cells, candidates);
        assert!(cached.metrics.native_effect_forced_cells > forced);
    }}
}

#[test]
fn native_alpha_reuse_zero_dab_terminal_edges_force_every_earlier_stroke_page() {
    let extent = [1029, 517];
    for name in ["brightness_to_opacity", "threshold"] {
        let mut doc = document_at(extent);
        composition_mut(&mut doc).color.depth = SampleDepth::F32;
        composition_mut(&mut doc).blend = layer_core::BlendSpace::Linear;
        let target = source_at(&doc, 0);
        let filter = attach_filter(&mut doc, name);
        if name == "threshold" { set_effect_value(&mut doc, filter, "transparency", EffectValue::Choice(0)); }
        let mut frame = packet(doc.scene(), extent);
        frame.composite_all = false; frame.blend_space = layer_core::BlendSpace::Linear;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        let mut cached = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut native = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        native.test.reference = true;
        for r in [&mut cached, &mut native] { r.submit(frame).unwrap(); }
        let mut style = crate::layer_tests::preset_style(DefaultBrushPreset::GPen);
        style.blend_space = frame.blend_space;
        let mut dab = crate::tests::test_dab([125.5, 126.5], [0.1, 0.2, 0.8, 0.7], 0.6);
        dab.radii = [24., 20.]; dab.motion = [13.25, -24.5]; dab.material = [0., 0.8, 0.9, 0.7];
        for position in [[125.5, 126.5], [901.5, 387.5]] {
            dab.center = layer_core::Point { x: position[0], y: position[1] };
            let batch = dab_batch(target, style.clone(), dab.bounds());
            for r in [&mut cached, &mut native] {
                r.submit(FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
            }
        }
        for r in [&mut cached, &mut native] { for _ in 0..2 { r.submit(frame).unwrap(); r.wait_idle().unwrap(); } }
        for coordinate in [[0, 0], [3, 1]] { assert!(cached.changed_cells.as_ref().unwrap().buffer(target, coordinate).is_some()); }
        style.rendering.edge_after_stroke = true;
        style.rendering.wet_edge = 0.8; style.rendering.burnt_edge = 0.4; style.rendering.edge_width = 4.;
        let mut batch = dab_batch(target, style, dab.bounds());
        batch.stroke_id = layer_core::StrokeId(17); batch.stroke_end = false;
        for (step, position) in [[125.5, 126.5], [901.5, 387.5]].into_iter().enumerate() {
            dab.center = layer_core::Point { x: position[0], y: position[1] };
            batch.damage = dab.bounds(); batch.stroke_start = step == 0; batch.material_update = step as u32;
            for r in [&mut cached, &mut native] {
                r.submit(FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
            }
        }
        let before = display_pixels(&cached);
        let earlier_source = |r: &WgpuRasterizer| {
            let layer = r.paint_layers.iter().find(|layer| layer.id == target).unwrap();
            let page = layer.pages.iter().find(|page| page.coordinate == [0, 0]).unwrap();
            pixels(r, &page.active().texture)
        };
        let source_before = earlier_source(&cached);
        batch.stroke_start = false; batch.stroke_end = true; batch.dab_count = 0; batch.material_update += 1;
        batch.damage = Default::default();
        let terminal = FramePacket { dab_batches: std::slice::from_ref(&batch), ..frame };
        cached.submit(terminal).unwrap(); native.submit(terminal).unwrap();
        let actual = display_pixels(&cached);
        let plan = cached.scale_display.as_ref().unwrap().plan;
        let first_page_changed = actual.iter().zip(&before).enumerate().any(|(i, (a, b))| {
            i as u32 % plan.size[0] < PAGE_SIZE / 4 && i as u32 / plan.size[0] < PAGE_SIZE / 4 && a != b
        });
        assert!(earlier_source(&cached) != source_before, "{name} zero-dab terminal edge pass must change authored pixels on an earlier distant stroke page");
        if name == "brightness_to_opacity" { assert!(first_page_changed, "zero-dab terminal edge pass must visibly change earlier native alpha output"); }
        let expected = pixels(&native, crate::test_support::document_texture(&native));
        let error = quality(&actual, &expected, plan);
        assert!(error[2] < 2e-5, "{name} terminal edge output must match native recomposition: {error:?}");
        for coordinate in [[0, 0], [3, 1]] {
            assert!(cached.changed_cells.as_ref().unwrap().reusable(target, coordinate).is_none(), "{name} zero-dab terminal must force earlier page {coordinate:?}");
        }
    }
}

#[test]
fn fused_prediction_tracks_actual_warm_primary_and_forces_relocated_pages() {
    for name in ["brightness_to_opacity","threshold"] {for blend in [layer_core::BlendSpace::Perceptual,layer_core::BlendSpace::Linear] {for level in [2,3] {
        let extent=[1029,517];let mut doc=document_at(extent);composition_mut(&mut doc).blend=blend;
        let target=source_at(&doc,0);attach_filter(&mut doc,name);
        let mut view=crate::test_support::view(extent);let scale=1./(1<<level) as f32;
        view.document_to_surface=[scale,0.,0.,scale,0.,0.];
        let frame=FramePacket {view,blend_space:blend,composite_all:false,..packet(doc.scene(),extent)};
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut reference=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        for backend in [&mut r,&mut reference] {backend.submit(frame).unwrap();}
        let mut dab=crate::tests::test_dab([125.5,127.5],[0.,0.,0.,1.],0.8);dab.radii=[24.,21.];
        let mut style=crate::layer_tests::preset_style(DefaultBrushPreset::GPen);style.blend_space=blend;
        let mut batch=dab_batch(target,style,dab.bounds());
        for backend in [&mut r,&mut reference] {backend.submit(FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();}
        batch.kind=DabBatchKind::Preview;batch.stroke_start=false;
        let mut previous=None;
        for (step,position) in [[133.5,127.5],[133.5,127.5],[143.5,133.5],[1023.5,513.5],[143.5,133.5]].into_iter().enumerate() {
            dab.center=layer_core::Point {x:position[0],y:position[1]};batch.damage=dab.bounds();
            if step==1 {
                let old=r.preview_page([0,0]).unwrap().primary.view.clone();
                let stamp=r.preview_page([0,0]).unwrap().primary.preview.get();let contacts=r.preview_contact_tiles.clone();
                r.submit(FramePacket {commit_rasters:false,..frame}).unwrap();
                assert_eq!(r.preview_page([0,0]).unwrap().primary.view,old);
                assert_eq!(r.preview_page([0,0]).unwrap().primary.preview.get(),stamp);
                assert_eq!(r.preview_contact_tiles,contacts);assert_eq!(r.preview_layer_id,Some(target));assert!(r.preview_contribution);
            }
            let current=FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame};
            r.submit(current).unwrap();reference.submit(FramePacket {composite_all:true,..current}).unwrap();r.wait_idle().unwrap();reference.wait_idle().unwrap();
            assert!(r.compact_preview_contribution(&batch));
            let coordinate=[position[0] as u32/PAGE_SIZE,position[1] as u32/PAGE_SIZE];
            let page=r.preview_page(coordinate).unwrap();assert!(!page.active_secondary);
            assert_eq!(page.primary.preview.get(),Some((target,coordinate)));
            let cells=r.changed_cells.as_ref().unwrap();
            let flags=cells.buffer(target,coordinate).unwrap();
            let tracked=step==1 || step==2;
            let expected=tracked.then(||flags.clone());
            page.primary.material_output.get((r.style_buffer.clone(),page.primary.view.clone(),None,false,expected),||panic!("{name} {blend:?} level{level} step{step}: actual kernel/output binding must have expected tracking"));
            let data=pollster::block_on(crate::local_tone::read_buffer_async(&r.device,&r.queue,flags)).unwrap();
            let dirty=data[16..].chunks_exact(4).filter(|v|u32::from_le_bytes((*v).try_into().unwrap())!=0).count();
            if step==1 {assert_eq!(dirty,0,"identical retained prediction must leave every cell clear");assert_eq!(Some(page.primary.view.clone()),previous);}
            if step==2 {assert!(dirty>0,"changed retained prediction must mark actual cells");}
            if !tracked {assert!(cells.reusable(target,coordinate).is_none(),"cold/rebound output must force full native effect work");}
            let oracle=reference.preview_page(coordinate).unwrap();
            assert_eq!(crate::layer_tests::page_bytes(&r,&page.primary.texture),crate::layer_tests::page_bytes(&reference,&oracle.primary.texture));
            assert_pixels_equal(&display_pixels(&r),&display_pixels(&reference),r.scale_display.as_ref().unwrap().plan.size[0],(name,blend,level,step));
            previous=Some(page.primary.view.clone());
        }
        r.submit(frame).unwrap();reference.submit(FramePacket {composite_all:true,..frame}).unwrap();
        assert!(r.preview_pages.is_empty());
        assert_pixels_equal(&display_pixels(&r),&display_pixels(&reference),r.scale_display.as_ref().unwrap().plan.size[0],(name,blend,level,"removed"));
    }}}
}

#[test]
fn fused_prediction_one_pixel_compares_signed_zero_and_tiny_alpha_bits() {
    use wgpu::util::DeviceExt;
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let (texture,view)=create_color_target(&r.device,[1,1],"tracked one-pixel contribution");
    let flags=r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:Some("one-cell comparison flags"),
        contents:&[256u32,1,0,0,0].map(u32::to_le_bytes).as_flattened(),usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::COPY_SRC});
    let doc=document_at([256;2]);let target=source_at(&doc,0);
    let dab=crate::tests::test_dab([0.,0.],[0.,0.,0.,1.],0.8);
    let batch=dab_batch(target,crate::layer_tests::preset_style(DefaultBrushPreset::GPen),dab.bounds());
    let record=StyleGpu::brush([256;2],&batch,PAGE_SIZE,&r.device);
    r.queue.write_buffer(&r.style_buffer,0,style_bytes(&record));
    let normal=[0.125f32,0.25,0.5,0.75];let tiny=[0.,0.,0.,1e-30];
    for (old,new,dirty) in [(normal,normal,0),([0.,0.,0.,1.],[-0.,0.,0.,1.],1),([-0.,0.,0.,1.],[0.,0.,0.,1.],1),([0.;4],tiny,1),(tiny,[0.;4],1)] {
        let bytes:Vec<_>=old.into_iter().flat_map(f32::to_le_bytes).collect();
        r.queue.write_texture(texture.as_image_copy(),&bytes,wgpu::TexelCopyBufferLayout {offset:0,bytes_per_row:Some(16),rows_per_image:Some(1)},texture.size());
        let components=new.map(|v|format!("bitcast<f32>({}u)",v.to_bits())).join(",");
        let material=format!("{}\n@compute @workgroup_size(1) fn comparison_test() {{store_display_color(vec2<u32>(0u),vec4<f32>({components}));}}",include_str!("../../material_brush.wgsl"));
        let shader=r.device.create_shader_module(wgpu::ShaderModuleDescriptor {label:Some("actual tracked display compare helper"),
            source:wgpu::ShaderSource::Wgsl(crate::dry_material::shader_source(&r.device,crate::dry_material::Target::DisplayTracked,&material))});
        let pipeline=r.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {label:Some("tracked one-pixel bit compare"),layout:None,module:&shader,entry_point:Some("comparison_test"),compilation_options:Default::default(),cache:None});
        let binding=r.device.create_bind_group(&wgpu::BindGroupDescriptor {label:Some("tracked one-pixel actual output"),layout:&pipeline.get_bind_group_layout(0),entries:&[
            wgpu::BindGroupEntry {binding:0,resource:wgpu::BindingResource::Buffer(wgpu::BufferBinding {buffer:&r.style_buffer,offset:0,size:NonZeroU64::new(mem::size_of::<StyleGpu>() as u64)})},
            wgpu::BindGroupEntry {binding:1,resource:wgpu::BindingResource::TextureView(&view)},
            wgpu::BindGroupEntry {binding:3,resource:flags.as_entire_binding()},
        ]});
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());encoder.clear_buffer(&flags,16,None);
        {let mut pass=encoder.begin_compute_pass(&Default::default());pass.set_pipeline(&pipeline);pass.set_bind_group(0,&binding,&[]);pass.dispatch_workgroups(1,1,1);}
        encoder.submit(&r.queue);
        let actual=pollster::block_on(crate::local_tone::read_buffer_async(&r.device,&r.queue,&flags)).unwrap();
        assert_eq!(u32::from_le_bytes(actual[16..20].try_into().unwrap()),dirty,"{old:?}→{new:?} must compare exact bits");
        let bytes=crate::layer_tests::page_bytes(&r,&texture);
        assert_eq!(&bytes[..16],new.into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>(),"tracked store must preserve exact RGBA32F");
    }
}
