use super::*;
use crate::{artwork_sample_tests::{insert_effect, refresh}, test_support::{packet, float_pixels, staged_renderer}};
use layer_core::{Document, DocumentNames, EffectInstance, EffectValue, authored::{OccurrenceHandle, PortableId}};
use layer_core::color::{RgbColor, RgbSpace};

const EXTENT: [u32; 2] = [17, 19];

#[cfg(not(target_arch="wasm32"))]
#[test]
fn cold_fragment_family_defers_before_encoding_and_capture_prepares_only_that_family() {
    for ordered in [false,true] {
        let reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut r = staged_renderer(&reference,Default::default());
        let mut scene = Scene::new(&r).unwrap();
        let compiler = r.startup.as_ref().unwrap().compiler.clone();
        let (output,view) = create_color_target(&r.device,EXTENT,"cold fill output");
        let (coverage,coverage_view) = create_target(&r.device,EXTENT,r.device.scalar_format(),"cold fill coverage");
        let pixels:Vec<_> = (0..EXTENT[0]*EXTENT[1]).flat_map(|_| 0.5_f32.to_le_bytes()).collect();
        r.queue.write_texture(coverage.as_image_copy(),&pixels,wgpu::TexelCopyBufferLayout {
            offset:0,bytes_per_row:Some(EXTENT[0]*4),rows_per_image:Some(EXTENT[1]),
        },coverage.size());
        let mut data = [0.;32];
        data[..8].copy_from_slice(&[0.,0.,EXTENT[0] as f32,EXTENT[1] as f32,EXTENT[0] as f32,EXTENT[1] as f32,0.,0.]);
        data[8] = 6.;
        data[12..16].copy_from_slice(&[0.25,0.5,0.75,0.5]);
        let job = Job::Draw {target:view,sources:[coverage_view,r.empty_view.clone(),r.empty_view.clone(),r.empty_paper_view.clone()],data,over:false,clip:None,source_target:None,edges:None,pipeline:None};
        scene.jobs.push(job.clone());
        scene.source_jobs.push(0..1);
        scene.ordered = ordered;
        let mut encoder = submission::CommandEncoder::new(&r.device,&Default::default());
        let (entered,entry) = std::sync::mpsc::channel();
        let (release,held) = std::sync::mpsc::channel();
        compiler.enqueue(startup::BRUSH,move || {let _=entered.send(());let _=held.recv_timeout(std::time::Duration::from_secs(10));Ok(())});
        entry.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        let deferred = scene.encode_query_jobs(&mut r,&mut encoder,None);
        let passes = encoder.pass_count();
        let records = scene.record_count;
        let retained = scene.jobs.len();
        let ready = scene.pipeline[3].ready();
        if !ordered {
            assert!(scene.encode_query_jobs(&mut r,&mut encoder,None).is_ok(),"discarded fragment work must not block an empty batch");
            scene.jobs.push(job.clone());
            assert!(matches!(scene.encode_query_jobs(&mut r,&mut encoder,None),Err(GpuRasterError::NotReady)));
        }
        release.send(()).unwrap();
        assert!(matches!(deferred,Err(GpuRasterError::NotReady)),"{deferred:?}");
        assert_eq!(passes,0);
        assert_eq!(records,0);
        assert_eq!(retained,usize::from(ordered));
        assert!(!ready);
        pollster::block_on(scene.prepare_capture_work_async(&mut r)).unwrap();
        assert!(scene.pipeline[3].ready());
        assert!(scene.pipeline.iter().enumerate().all(|(i,pipeline)| i==3 || !pipeline.ready()));
        if !ordered {scene.jobs.push(job);}
        scene.encode_query_jobs(&mut r,&mut encoder,None).unwrap();
        assert!(scene.pending_pipelines.render.is_empty() && scene.pending_pipelines.compute.is_empty());
        assert!(encoder.pass_count()>0);
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        assert_pixels(&r,&output,[0.0625,0.125,0.1875,0.25]);
    }
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn cold_compute_copy_encodes_without_admitting_fragment_pipelines() {
    let reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut r = staged_renderer(&reference,Default::default());
    let mut scene = Scene::new(&r).unwrap();
    r.compilation.blocking(||r.scene_pipelines.constant.2.compile());
    let (source,source_view) = create_color_target(&r.device,EXTENT,"compute copy source");
    let (output,view) = create_color_target(&r.device,EXTENT,"compute copy output");
    let color = [0.125_f32,0.25,0.375,0.5];
    let pixels:Vec<_> = (0..EXTENT[0]*EXTENT[1]).flat_map(|_| color.into_iter().flat_map(f32::to_ne_bytes)).collect();
    r.queue.write_texture(source.as_image_copy(),&pixels,wgpu::TexelCopyBufferLayout {
        offset:0,bytes_per_row:Some(EXTENT[0]*16),rows_per_image:Some(EXTENT[1]),
    },source.size());
    let mut data = [0.;32];
    data[..8].copy_from_slice(&[0.,0.,EXTENT[0] as f32,EXTENT[1] as f32,EXTENT[0] as f32,EXTENT[1] as f32,0.,0.]);
    data[8] = 1.;data[9] = 1.;
    scene.jobs.push(Job::Draw {target:view,sources:[source_view,r.empty_view.clone(),r.empty_view.clone(),r.empty_paper_view.clone()],
        data,over:false,clip:None,source_target:None,edges:None,pipeline:None});
    scene.source_jobs.push(0..1);
    let mut encoder = submission::CommandEncoder::new(&r.device,&Default::default());
    let result = scene.encode_query_jobs(&mut r,&mut encoder,None);
    assert!(result.is_ok(),"compute-only copy must not wait for a fragment pipeline: {result:?}");
    assert!(scene.pipeline.iter().all(|pipeline| !pipeline.ready()));
    r.uploads.finish(&encoder);encoder.submit(&r.queue);
    assert_pixels(&r,&output,color);
}

fn fixture(hidden_separator: bool) -> (Document, Vec<OccurrenceHandle>, Vec<OccurrenceHandle>) {
    let mut doc = Document::new(PortableId::random(), EXTENT[0], EXTENT[1],
        DocumentNames {paint: "Ink".into(), paper: "Paper".into()});
    for handle in doc.scene().children(None).to_vec() {
        doc.artwork.occurrences.get_mut(handle).unwrap().visible = false;
    }
    let mut fill = EffectInstance::new(crate::tests::fixture("solid_color").program());
    fill.set("color", EffectValue::Color(RgbColor::from_linear(RgbSpace::Srgb, [0., 0., 0., 1.]).unwrap())).unwrap();
    insert_effect(&mut doc, fill, 0);
    let a = insert_effect(&mut doc, EffectInstance::new(crate::tests::fixture("invert").program()), 0);
    let b = insert_effect(&mut doc, EffectInstance::new(crate::tests::fixture("invert").program()), 0);
    if hidden_separator {
        let x = insert_effect(&mut doc, EffectInstance::new(crate::tests::fixture("invert").program()), 0);
        let c = insert_effect(&mut doc, EffectInstance::new(crate::tests::fixture("invert").program()), 0);
        let d = insert_effect(&mut doc, EffectInstance::new(crate::tests::fixture("invert").program()), 0);
        doc.artwork.occurrences.get_mut(x).unwrap().visible = false;
        refresh(&mut doc);
        (doc, vec![a, b, c, d], vec![a, b])
    } else {
        let mut curves = EffectInstance::new(crate::tests::fixture("curves").program());
        for key in ["red", "green", "blue"] {
            curves.set(key, EffectValue::Curve(vec![[0., 1.], [1., 0.]])).unwrap();
        }
        let program = Arc::make_mut(&mut curves.program);
        program.entry = format!("{}_channels", program.entry).into();
        let c = insert_effect(&mut doc, curves, 0);
        (doc, vec![a, b, c], vec![a, b])
    }
}

fn prepared_scene(doc: &Document, broad: &[OccurrenceHandle], missing: &[OccurrenceHandle]) -> (WgpuRasterizer, Box<Scene>) {
    let reference = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    assert_ne!(reference.adapter.get_info().device_type, wgpu::DeviceType::Cpu);
    let mut r = staged_renderer(&reference, doc.composition().color);
    assert!(r.startup.is_some());
    r.ensure_document_metadata(EXTENT, doc.scene()).unwrap();
    let mut scene = Scene::new(&r).unwrap();
    let frame = packet(doc.scene(), EXTENT);
    let chains = effect_chains(doc.scene(), EffectPreparation::Capture);
    assert!(chains.iter().any(|(chain, execution)| chain == broad && *execution == effects::Execution::Fused));
    assert!(!chains.iter().any(|(chain, _)| chain == missing));
    for (chain, execution) in chains {
        scene.effects.prepare(&r, doc.scene(), &chain, execution, 0., 0, frame.blend_space).unwrap();
    }
    pollster::block_on(scene.effects.needs().prepare(&r.compilation)).unwrap();
    assert!(scene.effects.chain_ready(doc.scene(), broad, effects::Execution::Fused, frame.blend_space));
    assert!(!scene.effects.chain_ready(doc.scene(), missing, effects::Execution::Fused, frame.blend_space));
    (r, scene)
}

fn assert_pixels(r: &WgpuRasterizer, destination: &wgpu::Texture, expected: [f32; 4]) {
    let pixels = float_pixels(r, destination);
    assert_eq!(pixels.len(), (EXTENT[0] * EXTENT[1]) as usize);
    for (index, pixel) in pixels.iter().enumerate() {
        assert!(pixel.iter().zip(expected).all(|(actual, expected)| (actual - expected).abs() < 3e-5),
            "pixel {index}: {pixel:?}, expected {expected:?}");
    }
}

fn capture_after_cold_deferral(hidden_separator: bool) {
    let (doc, broad, missing) = fixture(hidden_separator);
    let (mut r, mut scene) = prepared_scene(&doc, &broad, &missing);
    let output = if hidden_separator {Output::Artwork(None)} else {Output::EffectChannels(*broad.last().unwrap())};
    let (destination, _) = create_color_target(&r.device, EXTENT, "cold fused capture");
    let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
    let result = scene.capture_region(&mut r, packet(doc.scene(), EXTENT), &destination,
        PixelRect::full(EXTENT), output, &mut encoder);
    assert!(matches!(result, Err(GpuRasterError::NotReady)), "{result:?}");
    assert_eq!(encoder.pass_count(), 0);
    assert_eq!(scene.record_count, 0);
    assert!(scene.jobs.is_empty());
    assert!(scene.source_jobs.is_empty());
    pollster::block_on(scene.prepare_capture_work_async(&mut r)).unwrap();
    assert!(scene.effects.chain_ready(doc.scene(), &missing, effects::Execution::Fused, Default::default()));
    scene.capture_region(&mut r, packet(doc.scene(), EXTENT), &destination,
        PixelRect::full(EXTENT), output, &mut encoder).unwrap();
    assert!(encoder.pass_count() > 0);
    r.uploads.finish(&encoder);
    encoder.submit(&r.queue);
    assert_pixels(&r, &destination, if hidden_separator {[0., 0., 0., 1.]} else {[1.; 4]});
}

#[test]
fn cold_hidden_separator_fusion_defers_before_encoding_then_capture_preparation_retries() {
    capture_after_cold_deferral(true);
}

#[test]
fn cold_effect_channels_prefix_defers_before_encoding_then_capture_preparation_retries() {
    capture_after_cold_deferral(false);
}

#[test]
fn ordered_cold_fusion_retains_all_jobs_and_source_ranges_until_prepared() {
    let (doc, broad, missing) = fixture(true);
    let (mut r, mut scene) = prepared_scene(&doc, &broad, &missing);
    let frame = packet(doc.scene(), EXTENT);
    let region = PixelRect::full(EXTENT);
    let (texture, view) = create_color_target(&r.device, EXTENT, "ordered cold fused capture");
    let destination = Image {texture: texture.clone(), view,
        plan: display_mips::Plan::window(EXTENT, 0, region)};
    let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
    scene.prepare_region(&mut r, frame, (region, region.into()), region, false, &mut encoder).unwrap();
    let declaration=scene.capture_declaration(&r,frame).unwrap();let mut accepted=scene.accept_capture(&mut r,frame,Some(Output::Artwork(None)),&declaration,|scene,r,accepted|scene.group(r,frame,None,[0,0],accepted).map(drop)).unwrap();
    let image = scene.group(&r, frame, None, [0, 0],&mut accepted).unwrap();
    let image = scene.converted(&r, image, Convert::linear(frame)).unwrap();
    scene.copy_window_tile(image, &destination, [0, 0]);
    assert!(scene.jobs.iter().any(|job| matches!(job, Job::Effect {prepared, ..} if !prepared.pipeline.ready())));
    let jobs: Vec<_> = scene.jobs.iter().map(std::mem::discriminant).collect();
    scene.source_jobs = vec![0..1, 1..scene.jobs.len()];
    let ranges = scene.source_jobs.clone();
    assert!(!scene.encode_ordered_jobs(&mut r, &mut encoder,None).unwrap());
    assert_eq!(scene.jobs.iter().map(std::mem::discriminant).collect::<Vec<_>>(), jobs);
    assert_eq!(scene.source_jobs, ranges);
    assert_eq!(scene.record_count, 0);
    assert_eq!(encoder.pass_count(), 0);
    pollster::block_on(scene.prepare_capture_work_async(&mut r)).unwrap();
    assert!(scene.encode_ordered_jobs(&mut r, &mut encoder,None).unwrap());
    assert!(scene.jobs.is_empty());
    assert!(scene.source_jobs.is_empty());
    assert!(encoder.pass_count() > 0);
    r.uploads.finish(&encoder);
    encoder.submit(&r.queue);
    assert_pixels(&r, &texture, [0., 0., 0., 1.]);
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn native_bake_defers_cold_effects_to_its_parent_compiler_and_preserves_history() {
    use layer_render::CanvasRenderer;
    use std::{sync::mpsc, time::{Duration, Instant}};
    let (document, _, _) = fixture(false);
    let reference = WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    assert_ne!(reference.adapter().get_info().device_type, wgpu::DeviceType::Cpu);
    let renderer = staged_renderer(&reference, document.composition().color);
    let compiler = renderer.startup.as_ref().unwrap().compiler.clone();
    let mut engine = layer_engine::CanvasEngine::new(renderer, document, layer_engine::input_queue(8).1,
        crate::test_support::view(EXTENT), layer_engine::ViewTransform::IDENTITY).unwrap();
    let before = engine.checkpoint();
    let plan = engine.document().merge_plan(layer_core::MergeKind::Visible).unwrap();
    let target = plan.target;
    engine.insert_with_operations(plan.edits, vec![(target, plan.operation)], None).unwrap();
    let merged = engine.checkpoint();
    assert_ne!(before, merged);
    let document = engine.document().clone();
    let batch = layer_render::DabBatch {kind:layer_render::DabBatchKind::RasterOperation(0), dab_count:0,
        ..crate::test_support::dab_batch(target,crate::tests::test_style(layer_core::BrushExecution::Dry),layer_core::Rect::from_extent(EXTENT))};
    let batches = [batch];
    let frame = FramePacket {dab_batches:&batches,..packet(document.scene(),EXTENT)};
    engine.backend_mut().ensure_document_metadata(EXTENT,document.scene()).unwrap();
    assert!(!engine.backend_mut().prepare_bakes(frame));
    let deadline=Instant::now()+Duration::from_secs(30);
    while compiler.pending()!=0 {
        compiler.check().unwrap();assert!(Instant::now()<deadline,"private capture recipes did not prepare");
        std::thread::sleep(Duration::from_millis(1));
    }
    let (entered, entry) = mpsc::channel();
    let (release, held) = mpsc::channel();
    compiler.enqueue(startup::DOCUMENT,move || {let _=entered.send(());let _=held.recv();Ok(())});
    entry.recv_timeout(Duration::from_secs(5)).unwrap();
    let pending = compiler.pending();
    assert!(!engine.backend_mut().prepare_bakes(frame));
    assert!(compiler.pending()>pending,"cold bake recipes were not admitted to the held parent compiler");
    assert!(engine.backend().prepared_bake(target,0,[0;2]).is_none(),"cold bake published pixels while its parent compiler was held");
    assert_eq!(engine.checkpoint(),merged);
    release.send(()).unwrap();
    let settle = |engine:&mut layer_engine::CanvasEngine<WgpuRasterizer>| {
        let deadline=Instant::now()+Duration::from_secs(30);
        loop {
            engine.render_frame_at(0).unwrap();engine.backend_mut().wait_idle().unwrap();
            if !engine.has_pending_document_edits() && !engine.raster_backing_pending() && engine.backend().next_frame()!=layer_render::FrameDemand::Work {break;}
            assert!(Instant::now()<deadline,"native bake did not settle after compiler release");
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    settle(&mut engine);
    let pixels=float_pixels(engine.backend(),crate::test_support::document_texture(engine.backend()));
    assert_eq!(pixels.len(),(EXTENT[0]*EXTENT[1]) as usize);
    assert!(pixels.iter().all(|pixel|pixel.iter().zip([1.;4]).all(|(actual,expected)|(actual-expected).abs()<3e-5)));
    assert_eq!(engine.checkpoint(),merged);
    assert!(engine.undo().unwrap());settle(&mut engine);assert_eq!(engine.checkpoint(),before);
    assert_eq!(float_pixels(engine.backend(),crate::test_support::document_texture(engine.backend())),pixels);
    assert!(engine.redo().unwrap());settle(&mut engine);assert_eq!(engine.checkpoint(),merged);
    assert_eq!(float_pixels(engine.backend(),crate::test_support::document_texture(engine.backend())),pixels);
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn native_bake_defers_cold_mask_restoration_to_its_parent_compiler() {
    use layer_core::raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileBlob,TileKey};
    use crate::layer_tests::placement::{paint_document,paint_mut,set_mask,reveal_all};
    use layer_render::CanvasRenderer;
    use std::{sync::mpsc,time::{Duration,Instant}};
    let mut document=paint_document(EXTENT,"Masked red");
    let color=document.composition().color;
    let pixels=[255,0,0,255].repeat(PAGE_SIZE.pow(2) as usize);
    paint_mut(&mut document).raster=RasterRevision::backed(RasterData {tiles:[(TileKey {plane:RasterPlane::Color,coordinate:[0;2]},
        RasterTile::backed(TileBlob::encode(color.paint_descriptor(),&pixels).unwrap()))].into(),..Default::default()});
    let mut mask=reveal_all(EXTENT,[0;2]);
    let pixels:Vec<_>=(0..PAGE_SIZE).flat_map(|_|(0..PAGE_SIZE).map(|x|if x<EXTENT[0]/2 {0}else{255})).collect();
    mask.source.raster=RasterRevision::backed(RasterData {tiles:[(TileKey {plane:RasterPlane::Mask,coordinate:[0;2]},
        RasterTile::backed(TileBlob::encode(RasterPlane::Mask.descriptor(color),&pixels).unwrap()))].into(),..Default::default()});
    set_mask(&mut document,mask);
    crate::test_support::add_paint(&mut document.artwork,"Empty",EXTENT);refresh(&mut document);
    assert!(document.scene().order().iter().all(|&owner|document.scene().effect(owner).is_none()));
    let reference=WgpuRasterizer::new_native_headless(color).unwrap();
    assert_ne!(reference.adapter().get_info().device_type,wgpu::DeviceType::Cpu);
    let renderer=staged_renderer(&reference,color);
    let compiler=renderer.startup.as_ref().unwrap().compiler.clone();
    let normalization=renderer.scene_pipelines.source.for_format(wgpu::TextureFormat::Rgba8Unorm).clone();
    assert!(!normalization.ready());
    let mut engine=layer_engine::CanvasEngine::new(renderer,document,layer_engine::input_queue(8).1,
        crate::test_support::view(EXTENT),layer_engine::ViewTransform::IDENTITY).unwrap();
    let before=engine.checkpoint();let plan=engine.document().merge_plan(layer_core::MergeKind::Visible).unwrap();let target=plan.target;
    engine.insert_with_operations(plan.edits,vec![(target,plan.operation)],None).unwrap();let merged=engine.checkpoint();
    let document=engine.document().clone();
    let batches=[layer_render::DabBatch {kind:layer_render::DabBatchKind::RasterOperation(0),dab_count:0,
        ..crate::test_support::dab_batch(target,crate::tests::test_style(layer_core::BrushExecution::Dry),layer_core::Rect::from_extent(EXTENT))}];
    let frame=FramePacket {dab_batches:&batches,..packet(document.scene(),EXTENT)};
    engine.backend_mut().ensure_document_metadata(EXTENT,document.scene()).unwrap();
    assert!(!engine.backend_mut().prepare_bakes(frame));
    let deadline=Instant::now()+Duration::from_secs(30);
    while compiler.pending()!=0 {
        compiler.check().unwrap();assert!(Instant::now()<deadline,"private capture recipes did not prepare");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!normalization.ready());
    let (entered,entry)=mpsc::channel();let (release,held)=mpsc::channel();
    compiler.enqueue(startup::DOCUMENT,move || {let _=entered.send(());let _=held.recv();Ok(())});
    entry.recv_timeout(Duration::from_secs(5)).unwrap();let pending=compiler.pending();
    while compiler.pending()==pending {
        assert!(!engine.backend_mut().prepare_bakes(frame));
        assert!(!normalization.ready());
        assert!(engine.backend().prepared_bake(target,0,[0;2]).is_none());
        assert_eq!(engine.checkpoint(),merged);
        assert!(Instant::now()<deadline,"masked color normalization did not reach the held parent compiler");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(compiler.pending()>pending,"color normalization bypassed the held parent compiler");
    assert!(!normalization.ready());
    assert!(engine.backend().prepared_bake(target,0,[0;2]).is_none());assert_eq!(engine.checkpoint(),merged);
    release.send(()).unwrap();
    let settle=|engine:&mut layer_engine::CanvasEngine<WgpuRasterizer>| {
        let deadline=Instant::now()+Duration::from_secs(30);
        loop {
            engine.render_frame_at(0).unwrap();engine.backend_mut().wait_idle().unwrap();
            if !engine.has_pending_document_edits() && !engine.raster_backing_pending() && engine.backend().next_frame()!=layer_render::FrameDemand::Work {break;}
            assert!(Instant::now()<deadline,"masked bake did not settle");std::thread::sleep(Duration::from_millis(1));
        }
    };
    settle(&mut engine);assert!(normalization.ready());
    let pixels=float_pixels(engine.backend(),crate::test_support::document_texture(engine.backend()));
    assert_eq!(pixels.len(),(EXTENT[0]*EXTENT[1]) as usize);
    for (i,pixel) in pixels.iter().enumerate() {
        let expected=if (i as u32)%EXTENT[0]<EXTENT[0]/2 {[0.;4]}else{[1.,0.,0.,1.]};
        assert!(pixel.iter().zip(expected).all(|(actual,expected)|(actual-expected).abs()<3e-5),"pixel {i}: {pixel:?}");
    }
    assert_eq!(engine.checkpoint(),merged);
    assert!(engine.undo().unwrap());settle(&mut engine);assert_eq!(engine.checkpoint(),before);
    assert_eq!(float_pixels(engine.backend(),crate::test_support::document_texture(engine.backend())),pixels);
    assert!(engine.redo().unwrap());settle(&mut engine);assert_eq!(engine.checkpoint(),merged);
    assert_eq!(float_pixels(engine.backend(),crate::test_support::document_texture(engine.backend())),pixels);
}

#[test]
fn a_cloned_publish_job_cannot_consume_one_encoding_twice() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    let receipt=encoder.submission();
    let expected=layer_core::raster::ContentName::fresh();
    let mut physical=resident::Resident::unwritten(std::cell::Cell::new(7u32));
    let write=physical.initialize(&receipt).unwrap();
    write.value().set(11);
    let job=Job::Publish {target:SourceTarget::Paint(layer_core::authored::PaintHandle::from_index(0)),
        output:Arc::new(std::sync::Mutex::new(Some(write.encoded(expected))))};
    let duplicate=job.clone();
    Scene::publish_job(&mut r,&receipt,&job).unwrap();
    assert_eq!(physical.read(expected,Some(&receipt)).unwrap().value().get(),11);
    let error=Scene::publish_job(&mut r,&receipt,&duplicate).expect_err("a consumed publication must fail within the same frame");
    assert!(matches!(&error,GpuRasterError::Invariant(_)),"{error:?}");
    assert!(!error.is_preparation_pending(),"a consumed encoding is an invariant failure, not missing preparation: {error:?}");
}
