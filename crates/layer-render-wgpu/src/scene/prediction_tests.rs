use super::*;
use crate::{layer_tests::placement, submission::ColorPass, test_support::{packet, submit_published}};

fn document() -> layer_core::Document {
    let mut document = placement::paint_document([256; 2], "prediction");
    placement::occurrence_mut(&mut document).opacity = 0.73;
    document
}

#[test]
fn plain_normal_prediction_leaves_coverage_and_blend_cold() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let document = document();
    let predicted: Vec<_> = r.scene_pipelines.for_scene(&r, document.scene(), &[]).collect();
    for index in [2, 6] {
        assert!(!predicted.iter().any(|pipeline| pipeline.identity() == r.scene_pipelines.pipeline[index].identity()),
            "plain Normal must not require {}", FRAGMENTS[index].1);
        assert!(!r.scene_pipelines.pipeline[index].ready());
    }
    pollster::block_on(r.compilation.prepare_all(predicted)).unwrap();
    assert!(r.scene_pipelines.pipeline[..2].iter().all(Deferred::ready));
    assert!([2, 6].into_iter().all(|index| !r.scene_pipelines.pipeline[index].ready()));
}

fn group(document: &mut layer_core::Document, pass_through: bool) -> OccurrenceHandle {
    let child = placement::occurrence_id(document);
    let group = crate::artwork_sample_tests::add_group(document, vec![child], 0);
    let occurrence = document.artwork.occurrences.get_mut(group).unwrap();
    occurrence.opacity = 0.61;
    if pass_through { occurrence.blend = layer_core::LayerBlend::PassThrough; }
    group
}

fn assert_bindings(r: &mut WgpuRasterizer, frame: FramePacket<'_>, case: &str) {
    let predicted: Vec<_> = r.scene_pipelines.for_scene(r, frame.scene, frame.dab_batches).map(Deferred::identity).collect();
    pollster::block_on(compile::Needs::capture_scene(r, frame.scene, false).prepare(&r.compilation)).unwrap();
    for ordered in [false, true] {
        let mut scene = r.scene.take().unwrap();
        scene.discard_unencoded_jobs();
        scene.presented_backdrop = None;
        scene.stop_before = None;
        let previous_modes=(scene.ordered,scene.object_display);
        scene.ordered = ordered;
        scene.object_display = true;
        let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
        let region=PixelRect::full(frame.document_extent);
        let window=scene.cached_capture_window(frame.scene,region);
        crate::test_support::query_ready(r,&mut encoder,|r,encoder|
            scene.prepare_region(r,frame,(region,window),region,false,encoder));
        let declaration=scene.capture_declaration(r,frame)
            .unwrap_or_else(|error|panic!("{case}, portable={}, ordered={ordered}, declaration: {error:?}",r.device.portable_blend()));
        let mut accepted=scene.accept_capture(r,frame,Some(Output::Artwork(None)),&declaration,|scene,r,accepted|scene.group(r,frame,None,[0;2],accepted).map(drop))
            .unwrap_or_else(|error|panic!("{case}, portable={}, ordered={ordered}, acceptance: {error:?}",r.device.portable_blend()));
        scene.group(r, frame, None, [0; 2],&mut accepted).unwrap_or_else(|error| panic!("{case}, ordered={ordered}, planning: {error:?}"));
        let mut fragments = Vec::new();
        for i in 0..scene.jobs.len() {
            if let Some(index) = bound_fragment(&r.device, &scene.jobs, i) {
                assert!(predicted.contains(&scene.pipeline[index].identity()),
                    "{case}, portable={}, ordered={ordered}: missing {}", r.device.portable_blend(), FRAGMENTS[index].1);
                fragments.push(index);
            }
        }
        assert!(scene.jobs.iter().any(|job| matches!(job, Job::Draw {..} | Job::Effect {..})), "{case}: exercise a draw or fused effect");
        if matches!(case, "multiply" | "clipped" | "hidden clip" | "offset" | "mask offset" | "masked Flow" | "future masked Flow" | "watercolor" | "batch watercolor" | "omitted clip" | "omitted ancestor") {
            assert!(fragments.contains(&6), "{case}: exercise fallback blending");
        }
        pollster::block_on(scene.prepare_capture_work_async(r)).unwrap();
        scene.encode_query_jobs(r, &mut encoder, None).unwrap_or_else(|error| panic!("{case}, ordered={ordered}: {error:?}"));
        assert!(encoder.pass_count() > 0);
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        (scene.ordered,scene.object_display)=previous_modes;
        r.scene = Some(scene);
    }
}

#[test]
fn prediction_covers_normal_and_fallback_fragment_bindings() {
    use layer_core::authored::*;
    for omitted in [wgpu::Features::empty(), wgpu::Features::FLOAT32_BLENDABLE] {
        let mut r = WgpuRasterizer::native_headless_without(Default::default(), omitted).unwrap();
        for case in ["plain", "multiply", "clipped", "hidden clip", "offset", "mask", "mask offset", "masked Flow", "future masked Flow", "watercolor", "batch watercolor", "restored watercolor", "isolated group", "pass-through group", "omitted clip", "omitted ancestor", "image", "vector", "attached effect", "spatial effect"] {
            let mut document = document();
            let owner = placement::occurrence_id(&document);
            let mut scope = SceneScope::All;
            match case {
                "multiply" => placement::occurrence_mut(&mut document).blend = layer_core::LayerBlend::Multiply,
                "clipped" | "hidden clip" | "omitted clip" => {
                    let source = placement::paint(&document).clone();
                    let (clip, _) = placement::append_paint(&mut document, "clip", source);
                    let occurrence = document.artwork.occurrences.get_mut(clip).unwrap();
                    occurrence.attachment = layer_core::Attachment::Clip;
                    occurrence.opacity = 0.61;
                    occurrence.visible = case != "hidden clip";
                    let root = document.composition().result;
                    document.artwork.stacks.get_mut(root).unwrap().entries = vec![clip, owner];
                    if case == "omitted clip" { scope = SceneScope::Members(vec![clip].into()); }
                }
                "offset" => placement::occurrence_mut(&mut document).offset = [3, 5],
                "restored watercolor" => placement::paint_mut(&mut document).raster = layer_core::raster::RasterRevision::backed(layer_core::raster::RasterData {
                    watercolor: Some(layer_core::raster::RasterWatercolor {wet_edge: 0.9, burnt_edge: 0.6, edge_width: 16.}), ..Default::default()
                }),
                "mask" | "mask offset" | "masked Flow" | "future masked Flow" => placement::set_mask(&mut document, placement::reveal_all([256; 2], if case == "mask offset" {[3, 5]} else {[0; 2]})),
                "isolated group" | "pass-through group" | "omitted ancestor" => {
                    group(&mut document, case == "pass-through group");
                    if case == "omitted ancestor" {
                        placement::set_mask(&mut document, placement::reveal_all([256; 2], [0; 2]));
                        scope = SceneScope::Members(vec![owner].into());
                    }
                }
                "image" => {
                    let image = ImageObject::new(layer_core::color::source::rgba8_source([256; 2], |_, _| [255; 4]).into());
                    let object = document.artwork.objects.insert(PortableId::random(), image).unwrap();
                    document.artwork.occurrences.get_mut(owner).unwrap().content = OccurrenceContent::Objects(object);
                }
                "vector" => {
                    use layer_core::authored::vector::*;
                    let anchors = [[4., 4.], [28., 4.], [16., 28.]].map(Anchor::corner);
                    let color = layer_core::color::RgbColor::new(Default::default(), [0., 0., 0., 1.]).unwrap();
                    let path = Record::Path(PathItem::new(ItemId(1), Arc::new(PathGeometry::new(vec![Subpath::new(&anchors, true)]).unwrap()), Arc::new(Style::new(vec![PaintEntry::Stroke(Stroke::new(Paint::color(color), 4.))]))));
                    let mut editor = layer_core::Editor::new(document);
                    let (_, edit) = editor.document().create_vector_layer_edit("Vector", VectorObject::from_records(vec![path]), None, 0).unwrap();
                    editor.perform(edit).unwrap();
                    document = editor.document().clone();
                }
                "attached effect" | "spatial effect" => {
                    let effect = layer_core::EffectInstance::new(crate::tests::fixture(if case == "spatial effect" {"gaussian_blur"} else {"invert"}).program());
                    let handle = crate::artwork_sample_tests::insert_effect(&mut document, effect, 0);
                    document.artwork.occurrences.get_mut(handle).unwrap().attachment = layer_core::Attachment::Effect;
                }
                _ => {},
            }
            crate::artwork_sample_tests::refresh(&mut document);
            let target = document.scene().source_target(owner).unwrap_or_default();
            let frame = packet(document.scene(), [256; 2]);
            submit_published(&mut r, frame);
            let targets: Vec<_> = r.paint_layers.iter().map(|layer| layer.id).collect();
            for target in targets {
                let mut page = r.create_page([0; 2], "prediction paint");
                let pixels = [0.125_f32, 0.25, 0.5, 0.75].into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>().repeat(PAGE_SIZE.pow(2) as usize);
                let intent = r.raster_intent(target, layer_core::raster::RasterPlane::Color, [0; 2]).unwrap();
                let name = layer_core::raster::ContentName::fresh();
                let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
                let receipt = encoder.submission();
                let write = page.primary.initialize(&receipt).unwrap();
                crate::test_support::upload_written_page(&mut r.uploads, &mut encoder, &write, &pixels);
                write.encoded(name).publish(&receipt);
                let written = page.primary.read(name, Some(&receipt)).unwrap().replace(&intent, &receipt);
                page.primary_needs_clear = false;
                r.paint_layers.iter_mut().find(|layer| layer.id == target).unwrap().pages = vec![page];
                r.record_raster_write(target, written).unwrap();
                r.uploads.finish(&encoder);
                encoder.submit(&r.queue);
            }
            crate::test_support::prepare_scene_effects(&mut r, frame);
            let dabs=[crate::tests::test_dab([128.;2],[0.2,0.4,0.6,1.],1.)];
            let mut batches = Vec::new();
            if matches!(case, "batch watercolor" | "future masked Flow") {
                let style = crate::tests::test_style(if case == "batch watercolor" {BrushExecution::Watercolor} else {BrushExecution::Dry});
                let mut batch = crate::test_support::dab_batch(target, style, layer_core::Rect::from_extent([256; 2]));
                if case == "future masked Flow" { batch.kind = DabBatchKind::Preview; }
                batches.push(batch);
            }
            let frame = FramePacket {scene: document.scene().with_scope(&scope), dabs:&dabs, dab_batches: &batches, ..frame};
            let before: Vec<_> = r.scene_pipelines.for_scene(&r, frame.scene, &batches).map(Deferred::identity).collect();
            if case == "watercolor" {
                r.paint_layers.iter_mut().find(|layer| layer.id == target).unwrap().watercolor = Some(WatercolorLayerStyle::from_dab_style(&crate::tests::test_style(BrushExecution::Watercolor)));
            }
            if matches!(case, "masked Flow" | "future masked Flow") {
                let mut batch=frame.dab_batches.iter().find(|batch|batch.kind==DabBatchKind::Preview).cloned()
                    .unwrap_or_else(||crate::test_support::dab_batch(target,crate::tests::test_style(BrushExecution::Dry),dabs[0].bounds()));
                batch.kind=DabBatchKind::Preview;
                let declaration=Arc::new(PreviewDeclaration::from_scene(FramePacket {dab_batches:std::slice::from_ref(&batch),..frame},
                    Default::default(),Default::default(),PreviewQuality::default(),None,None).unwrap().unwrap());
                declaration.accept_inputs(target,RasterPlane::Color,[0;2],PreviewStage::AfterBatch(0),PreviewPass::Color,Vec::new()).unwrap();
                let name=declaration.version(target,RasterPlane::Color,[0;2],PreviewStage::Final).unwrap();
                let mut page=r.create_page([0;2],"prediction preview");
                let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
                let receipt=encoder.submission();
                let write=page.primary.initialize(&receipt).unwrap();
                drop(encoder.color_pass("prediction preview",&write.value().view,wgpu::LoadOp::Clear(wgpu::Color {r:0.2,g:0.4,b:0.6,a:1.})));
                write.publish(&name);
                page.primary_needs_clear=false;
                encoder.submit(&r.queue);
                assert!(page.primary.read(&name,None).is_some());
                r.preview_declaration=Some(declaration);
                r.preview_layer_id = Some(target);
                r.preview_requires_base = false;
                r.preview_damage = PixelRect::full([256; 2]);
                r.preview_contact_tiles = None;
                r.preview_pages = vec![page];
            }
            if case == "future masked Flow" {
                assert!(before.contains(&r.scene_pipelines.pipeline[6].identity()), "preview batches must predict blending before preview pages exist");
            }
            assert_bindings(&mut r, frame, case);
            r.preview_layer_id = None;
            r.preview_declaration=None;
            r.preview_pages.clear();
        }
    }
}

#[test]
fn coverage_prediction_prepares_bake_and_erase_before_binding() {
    for omitted in [wgpu::Features::empty(), wgpu::Features::FLOAT32_BLENDABLE] {
        for ordered in [false, true] {
            for operation in [21., 22.] {
                let mut r = WgpuRasterizer::native_headless_without(Default::default(), omitted).unwrap();
                let mut document = document();
                placement::set_mask(&mut document, placement::reveal_all([256; 2], [0; 2]));
                let mut scene = Scene::new(&r).unwrap();
                scene.ordered = ordered;
                let output = scene.reserve_format(&r, true).unwrap();
                scene.draw(&r, output, r.empty_view.clone(), Some(r.empty_view.clone()),
                    [0., 0., 256., 256.], [operation, 1., 0., 0.], false, Convert::None);
                assert_eq!(bound_fragment(&r.device, &scene.jobs, 0), Some(2));
                let predicted: Vec<_> = r.scene_pipelines.for_scene(&r, document.scene(), &[]).cloned().collect();
                assert!(predicted.iter().any(|pipeline| pipeline.identity() == scene.pipeline[2].identity()));
                let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
                assert!(matches!(scene.encode_query_jobs(&mut r, &mut encoder, None), Err(GpuRasterError::NotReady)));
                assert_eq!(encoder.pass_count(), 0);
                assert!(scene.pending_pipelines.render.iter().any(|pipeline| pipeline.identity() == scene.pipeline[2].identity()));
                pollster::block_on(r.compilation.prepare_all(&predicted)).unwrap();
                if !ordered {
                    scene.draw(&r, output, r.empty_view.clone(), Some(r.empty_view.clone()),
                        [0., 0., 256., 256.], [operation, 1., 0., 0.], false, Convert::None);
                }
                scene.encode_query_jobs(&mut r, &mut encoder, None).unwrap();
                assert!(encoder.pass_count() > 0);
                r.uploads.finish(&encoder);
                encoder.submit(&r.queue);
            }
        }
    }
}

#[test]
fn prediction_tracks_composition_edits_and_unloaded_material() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document();
    let key = startup::ShaderDocument::new(&document);
    placement::occurrence_mut(&mut document).blend = layer_core::LayerBlend::Multiply;
    document.revision += 1;
    assert!(!key.matches(&document), "changing Normal to Multiply changes the document gate");
    let key = startup::ShaderDocument::new(&document);
    placement::set_mask(&mut document, placement::reveal_all([256; 2], [0; 2]));
    assert!(!key.matches(&document), "adding coverage changes the document gate");
    let mut restored = self::document();
    placement::paint_mut(&mut restored).raster = layer_core::raster::RasterRevision::pending();
    assert!(r.scene_pipelines.for_scene(&r, restored.scene(), &[]).any(|pipeline| pipeline.identity() == r.scene_pipelines.pipeline[6].identity()),
        "an unpublished backing can contain watercolor");
}
