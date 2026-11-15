use super::*;
use crate::test_support::{packet, float_pixels};
use layer_core::authored::*;
use layer_core::authored::Image as AuthoredImage;
use crate::scene::Output;
use layer_core::color::{SampleDepth, source::*};

#[path="stacked_photos_tests.rs"]
mod stacked_photos;
#[path="cold_navigator_stroke_tests.rs"]
mod cold_navigator_stroke;

fn image(extent: [u32; 2], color: [u8; 4]) -> AuthoredImage {
    let mut builder = SourceBuilder::new(extent, SourceInterpretation { channels: SourceChannels::Rgba,
        depth: SampleDepth::U8, profile: Default::default(), profile_assumed: false }, 16 * 1024 * 1024).unwrap();
    for _ in 0..extent[1] { builder.push_row(&color.repeat(extent[0] as usize)).unwrap(); }
    AuthoredImage::new(Arc::new(builder.finish().unwrap()))
}

fn document(extent: [u32; 2], objects: Vec<ImageObject>) -> (layer_core::Document, OccurrenceHandle, Vec<ImageObjectHandle>) {
    let mut artwork = Artwork::new(extent).unwrap();
    let handles: Vec<_> = objects.into_iter().map(|object| artwork.objects.insert(PortableId::random(), object).unwrap()).collect();
    let owners = handles.iter().map(|&handle| artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Objects(handle), "Image")).unwrap()).collect::<Vec<_>>();
    let owner = owners[0];
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries = owners;
    (layer_core::Document::from_artwork(artwork).unwrap(), owner, handles)
}

fn resident_object_bytes(extent: [u32; 2], owners: u64, level: u32) -> u64 {
    let side = 256u32 << level;
    owners * extent.into_iter().map(|length|u64::from(length.div_ceil(side))).product::<u64>() * 256 * 256 * 16
}

fn native(r: &mut WgpuRasterizer, doc: &layer_core::Document, output: Output) -> Vec<[f32; 4]> {
    let extent = doc.composition().size;
    let (texture, _) = create_color_target(&r.device, extent, "object native reference");
    let mut scene = r.scene.take().unwrap();
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    for _ in 0..5000 {
        let result=scene.capture_region_prepared(r,packet(doc.scene(),extent),&texture,PixelRect::full(extent),output,&mut encoder);
        match result {Ok(())=>break,Err(GpuRasterError::NotReady)=>{},Err(error)=>panic!("{error}")}
        r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
        encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    }
    r.uploads.finish(&encoder);encoder.submit(&r.queue);
    r.scene = Some(scene);
    float_pixels(r, &texture)
}

fn submit_ready(r: &mut WgpuRasterizer, frame: FramePacket<'_>) {
    let wakes = wake_channel(r);
    crate::test_support::submit_ready(r, frame).unwrap();
    assert!(settle_on_wakes(r, frame, &wakes, 2000, |_| {}).is_some(), "object display preparation must settle");
}

fn wake_channel(r: &mut WgpuRasterizer) -> std::sync::mpsc::Receiver<()> {
    let (wake, wakes) = std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move || { let _ = wake.send(()); }));
    wakes
}

fn settle_on_wakes(r: &mut WgpuRasterizer, frame: FramePacket<'_>, wakes: &std::sync::mpsc::Receiver<()>, limit: usize, each: impl Fn(&WgpuRasterizer)) -> Option<usize> {
    for submitted in 1..=limit {
        r.wait_idle().unwrap();
        each(r);
        if r.next_frame()!=layer_render::FrameDemand::Work {
            if !r.scene.as_ref().is_some_and(|scene| scene.object_results.pending()) && !r.moving_images_pending() && !r.image_decode_waiting() { return Some(submitted); }
            wakes.recv_timeout(crate::READBACK_TIMEOUT).expect("object work waiting on a decode or a release must wake the host");
        }
        crate::test_support::submit_ready(r, FramePacket { composite_all: false, reset_layers: false, ..frame }).unwrap();
    }
    None
}

fn canonical_request(r: &WgpuRasterizer, scene: &mut Scene, doc: &layer_core::Document, owner: OccurrenceHandle, window: ObjectWindow) -> super::objects::CollectionJob {
    let content = scene.object_spatial.content(doc.scene(), owner).unwrap();
    scene.collection_job(r, packet(doc.scene(), doc.composition().size), owner, content, window)
}

#[test]
fn rasterized_objects_feed_the_next_paint_frame_from_accepted_inputs() {
    let extent=[64,32];
    let (mut doc,owner,_)=document(extent,vec![ImageObject::new(image(extent,[171,37,83,255]))]);
    let mut renderer=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    crate::test_support::submit_settled(&mut renderer,packet(doc.scene(),extent));
    let original=renderer.readback_srgb_rgba8().unwrap();
    let plan=doc.rasterize_plan(owner,false).unwrap();
    let capture=plan.image_capture(extent,None).expect("the object rasterizes through the shared image worker");
    let gpu=renderer.snapshot_gpu();
    let (send,receive)=std::sync::mpsc::channel();
    let worker=std::thread::spawn(move||{let result=pollster::block_on(gpu.image_capture(capture,Default::default()));let _=send.send(result);});
    let result=receive.recv_timeout(READBACK_TIMEOUT).expect("Rasterize Layer completes before its deadline").unwrap();
    worker.join().unwrap();
    let edits=plan.with_image(result.map(|(source,origin)|(AuthoredImage::new(source),origin))).unwrap();
    doc.apply(layer_core::Edit::Batch(edits)).unwrap();
    assert_eq!(doc.scene().occurrence(owner).unwrap().kind(),LayerKind::Paint);
    let frame=packet(doc.scene(),extent);
    super::super::sources::Inputs::begin_accepted_reads();
    crate::test_support::submit_published(&mut renderer,frame);
    let reads=super::super::sources::Inputs::take_accepted_read_results();
    assert!(reads.iter().map(|(count,_)|count).sum::<usize>()>0,"the converted frame must consume its paint-base pixels");
    assert!(reads.iter().all(|(_,unexpected)|unexpected.is_empty()),"{reads:?}");
    let converted=renderer.readback_srgb_rgba8().unwrap();
    assert_eq!(converted.len(),original.len());
    assert!(converted.iter().zip(&original).all(|(a,b)|a.abs_diff(*b)<=1),"Rasterize Layer preserves its composite within one display code");
    let target=doc.scene().source_target(owner).unwrap();
    let dab=crate::tests::test_dab([32.,16.],[1.,0.,0.,1.],1.);
    let batch=crate::test_support::dab_batch(target,crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen),dab.bounds());
    let stroke=FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame};
    crate::test_support::submit_ready(&mut renderer,stroke).unwrap();
    let mut replay=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    crate::test_support::submit_settled(&mut replay,frame);
    crate::test_support::submit_ready(&mut replay,stroke).unwrap();
    let actual=renderer.readback_srgb_rgba8().unwrap();
    let expected=replay.readback_srgb_rgba8().unwrap();
    assert_eq!(actual.len(),expected.len());
    assert!(actual.iter().zip(&expected).all(|(a,b)|a.abs_diff(*b)<=1),"painting the rasterized layer matches a fresh replay");
}

#[test]
fn ordered_query_capture_replans_a_cold_object_stack() {
    let extent=[16;2];
    let (doc,_,_)=document(extent,vec![
        ImageObject::new(image(extent,[230,40,20,255])),
        ImageObject::new(image(extent,[30,70,210,255])),
    ]);
    let (send,receive)=std::sync::mpsc::channel();
    let worker=std::thread::spawn(move || {
        let result=(|| {
            let mut r=WgpuRasterizer::new_native_headless(doc.composition().color)?;
            assert!(!super::objects::inline_sampling(&r));
            r.ensure_document_metadata(extent,doc.scene())?;
            let mut scene=Scene::new(&r)?;
            let (texture,_)=create_color_target(&r.device,extent,"ordered object capture");
            let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
            pollster::block_on(scene.capture_query_tiles(&mut r,packet(doc.scene(),extent),PixelRect::full(extent),
                Output::Artwork(None),false,1,None,&mut encoder,|_,view,region,encoder| {
                    encoder.copy_texture_to_texture(view.texture().as_image_copy(),texture.as_image_copy(),
                        wgpu::Extent3d {width:region.width(),height:region.height(),depth_or_array_layers:1});
                    Ok(())
                }))?;
            r.uploads.finish(&encoder);encoder.submit(&r.queue);
            Ok::<_,GpuRasterError>(float_pixels(&r,&texture))
        })();
        let _=send.send(result);
    });
    let pixels=receive.recv_timeout(std::time::Duration::from_secs(60))
        .expect("ordered object capture completes before its deadline").unwrap();
    worker.join().unwrap();
    let expected=[230.,40.,20.].map(|channel|layer_core::color::srgb_decode(channel/255.));
    assert_eq!(pixels.len(),16*16);
    for (index,pixel) in pixels.into_iter().enumerate() {
        assert!((pixel[3]-1.).abs()<3e-5,"pixel {index} must be opaque: {pixel:?}");
        for channel in 0..3 {
            assert!((pixel[channel]-expected[channel]).abs()<3e-5,
                "pixel {index}, channel {channel}: {pixel:?}, expected {expected:?}");
        }
    }
}


#[test]
fn an_object_request_is_admitted_from_the_display_tier_without_an_object_share() {
    let extent=[32;2];
    let (doc,owner,_)=document(extent,vec![ImageObject::new(image(extent,[255,0,0,255]))]);
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    crate::memory::memory_tests::legacy_scene_frame(&mut r,packet(doc.scene(),extent));
    let mut scene=Scene::new(&r).unwrap();scene.object_display=true;
    let request=canonical_request(&r,&mut scene,&doc,owner,ObjectWindow {origin:[0.;2],side:1.,size:extent});
    let mut cache=super::super::object_cache::ObjectCache::for_scene(&scene,true);
    assert!(matches!(cache.resolve_collection(&r,doc.scene(),&request,None),Ok(None)),"the display tier, not an object share, admits the window");
    assert_eq!(cache.work_progress().0,1);
}

#[test]
fn a_visible_object_window_is_admitted_at_visible_while_another_window_is_working() {
    let extent=[512,256];
    let (doc,owner,_)=document(extent,vec![ImageObject::new(image(extent,[255,0,0,255]))]);
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    crate::memory::memory_tests::legacy_scene_frame(&mut r,packet(doc.scene(),extent));
    let mut scene=Scene::new(&r).unwrap();scene.object_display=true;
    let requests=[0.,256.].map(|x|canonical_request(&r,&mut scene,&doc,owner,ObjectWindow {origin:[x,0.],side:1.,size:[256;2]}));
    let mut cache=super::super::object_cache::ObjectCache::for_scene(&scene,true);
    assert!(cache.admit(&r,Class::Visible,&requests[..1],None).unwrap().is_none());
    let display=r.scene_tiers.display.clone();
    display.set_limit(display.limit(),display.limit());
    let refusal=cache.admit(&r,Class::Visible,&requests[1..],None).unwrap();
    assert!(refusal.is_none(),"a visible window must not be refused at a lower class because another window is working: {refusal:?}");
    assert_eq!(cache.work_progress().0,2);
}

#[test]
fn a_reserved_object_window_starts_while_queued_reservations_fill_the_optional_room() {
    let extent=[512,256];
    let (doc,owner,_)=document(extent,vec![ImageObject::new(image(extent,[255,0,0,255]))]);
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    crate::memory::memory_tests::legacy_scene_frame(&mut r,packet(doc.scene(),extent));
    pollster::block_on(r.compilation.prepare_all(r.scene_pipelines.objects.pipelines())).unwrap();
    let mut scene=Scene::new(&r).unwrap();scene.object_display=true;
    let requests=[0.,256.].map(|x|canonical_request(&r,&mut scene,&doc,owner,ObjectWindow {origin:[x,0.],side:1.,size:[256;2]}));
    assert!(scene.object_results.admit(&r,Class::Visible,&requests,None).unwrap().is_none());
    let display=r.scene_tiers.display.clone();
    display.set_limit(display.limit(),display.limit());
    for _ in 0..64 {
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        r.drain_image_decodes(&mut encoder).unwrap();
        let empty=super::super::object_cache::ObjectCache::for_scene(&scene,false);
        let mut cache=std::mem::replace(&mut scene.object_results,empty);
        cache.advance_with_memory(&mut r,&mut scene,&mut encoder,u64::MAX).unwrap();
        scene.object_results=cache;
        r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
        if scene.object_results.work_progress().2>0 {return;}
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("a demanded window must start although queued reservations leave no optional room");
}

fn ready_results(r:&mut WgpuRasterizer,scene:&mut Scene,doc:&layer_core::Document,requests:&[super::objects::CollectionJob]) {
    pollster::block_on(r.compilation.prepare_all(r.scene_pipelines.objects.pipelines())).unwrap();
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(20);
    while !requests.iter().map(|request|scene.object_results.resolve_collection(r,doc.scene(),request,None).unwrap().is_some()).fold(true,|all,ready|all&ready) {
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        r.drain_image_decodes(&mut encoder).unwrap();
        let empty=super::super::object_cache::ObjectCache::for_scene(scene,false);
        let mut cache=std::mem::replace(&mut scene.object_results,empty);
        cache.advance(r,scene,&mut encoder,&mut Scene::begin_work(false)).unwrap();
        scene.object_results=cache;
        r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
        assert!(std::time::Instant::now()<deadline,"canonical results complete");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn only_the_current_frames_demand_keeps_an_object_result_from_grant_eviction() {
    let extent=[512,256];
    let (doc,owner,_)=document(extent,vec![ImageObject::new(image(extent,[255,0,0,255]))]);
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    let frame=packet(doc.scene(),extent);
    r.begin_frame_memory(frame);
    let mut scene=Scene::new(&r).unwrap();scene.object_display=true;
    let requests=[0.,256.].map(|x|canonical_request(&r,&mut scene,&doc,owner,ObjectWindow {origin:[x,0.],side:1.,size:[256;2]}));
    ready_results(&mut r,&mut scene,&doc,&requests);
    let content=scene.object_spatial.content(doc.scene(),owner).unwrap();
    let demand=|r:&mut WgpuRasterizer,scene:&mut Scene,requests:&[super::objects::CollectionJob]| {
        r.begin_frame_memory(frame);
        for request in requests {
            let mut accepted=sources::AcceptedInputs::default();
            assert!(scene.canonical_cover(r,frame,&content,request,&mut accepted,Class::Visible).unwrap().is_some());
        }
        scene.object_results.reset_used();
    };
    demand(&mut r,&mut scene,&requests);
    r.end_frame_memory();
    demand(&mut r,&mut scene,&requests[..1]);
    r.scene_tiers.display.heap().evict(r.scene_tiers.display.used(),Class::Fallback);
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    scene.retire_victims(&mut encoder);
    encoder.submit(&r.queue);
    let kept=scene.object_results.completed_windows().into_iter().map(|(_,_,window)|window.origin).collect::<Vec<_>>();
    assert_eq!(kept,[[0.,0.]],"the grant pass keeps the result this frame demands and evicts the one only an earlier frame demanded");
}

fn unfinished_windows(extent:[u32;2])->(WgpuRasterizer,layer_core::Document,[super::objects::CollectionJob;2]) {
    let (doc,owner,_)=document(extent,vec![ImageObject::new(image(extent,[255,0,0,255]))]);
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    r.begin_frame_memory(packet(doc.scene(),extent));
    let mut scene=Scene::new(&r).unwrap();scene.object_display=true;
    let requests=[0.,256.].map(|x|canonical_request(&r,&mut scene,&doc,owner,ObjectWindow {origin:[x,0.],side:1.,size:[256;2]}));
    assert!(scene.object_results.admit(&r,Class::Visible,&requests,None).unwrap().is_none());
    r.scene=Some(scene);
    (r,doc,requests)
}
fn frame_demanding(r:&mut WgpuRasterizer,frame:FramePacket<'_>,requests:&[super::objects::CollectionJob],pressure:bool)->usize {
    r.begin_frame_memory(frame);
    let mut scene=r.scene.take().unwrap();
    assert!(scene.object_results.admit(r,Class::Visible,requests,None).unwrap().is_none());
    if pressure {
        r.scene_tiers.display.heap().evict(r.scene_tiers.display.used(),Class::Fallback);
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        scene.retire_victims(&mut encoder);encoder.submit(&r.queue);
    }
    r.scene=Some(scene);
    r.end_frame_memory();
    r.scene.as_ref().unwrap().object_results.request_windows().len()
}

#[test]
fn an_unfinished_object_window_no_frame_or_held_unit_demands_is_evicted_under_pressure() {
    let extent=[512,256];
    let (mut r,doc,requests)=unfinished_windows(extent);
    let admitted=r.scene.as_ref().unwrap().object_results.memory_bytes()[1];
    let mut held=sources::AcceptedInputs::default();
    r.scene.as_mut().unwrap().object_results.cover(&requests[1],Some(&mut held));
    assert_eq!(frame_demanding(&mut r,packet(doc.scene(),extent),&requests[..1],false),2,"without pressure an undemanded window stays resident");
    assert_eq!(frame_demanding(&mut r,packet(doc.scene(),extent),&requests[..1],true),2,"a unit that holds the window's inputs keeps it under pressure");
    drop(held);
    assert_eq!(frame_demanding(&mut r,packet(doc.scene(),extent),&requests[..1],true),1,"pressure evicts the window no frame or held unit demands");
    assert!(r.scene.as_ref().unwrap().object_results.memory_bytes()[1]<admitted,"the evicted window returns its reservation");
}

#[test]
fn unfinished_object_windows_no_recent_frame_demands_request_no_frame_but_frames_that_run_advance_them() {
    let extent=[512,256];
    let (mut r,doc,_requests)=unfinished_windows(extent);
    r.end_frame_memory();
    assert!(r.frame_work(),"the windows the last frame demanded request frames");
    for _ in 0..2 {r.begin_frame_memory(packet(doc.scene(),extent));r.end_frame_memory();}
    assert_eq!(r.scene.as_ref().unwrap().object_results.request_windows().len(),2,"the windows stay resident without pressure");
    assert!(!r.frame_work(),"windows no recent frame demands request no frame");
    assert!(r.objects_runnable(),"a frame that runs for other work still composes the unfinished windows");
    assert!(r.scene.as_ref().unwrap().object_results.pending(),"a capture drain still waits for every unfinished window");
}

#[test]
fn an_unfinished_object_window_the_next_frames_do_not_demand_completes_without_pressure() {
    let extent=[512,256];
    let (mut r,doc,requests)=unfinished_windows(extent);
    r.end_frame_memory();
    pollster::block_on(r.compilation.prepare_all(r.scene_pipelines.objects.pipelines())).unwrap();
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(20);
    while r.scene.as_ref().unwrap().object_results.completed_windows().len()<2 {
        r.begin_frame_memory(packet(doc.scene(),extent));
        let mut scene=r.scene.take().unwrap();
        assert!(scene.object_results.resolve_collection(&r,doc.scene(),&requests[0],None).is_ok());
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        r.drain_image_decodes(&mut encoder).unwrap();
        let empty=super::super::object_cache::ObjectCache::for_scene(&scene,false);
        let mut cache=std::mem::replace(&mut scene.object_results,empty);
        cache.advance(&mut r,&mut scene,&mut encoder,&mut Scene::begin_work(false)).unwrap();
        scene.object_results=cache;r.scene=Some(scene);
        r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
        r.end_frame_memory();
        assert_eq!(r.scene.as_ref().unwrap().object_results.request_windows().len(),2,"no pressure releases the window the camera left");
        assert!(std::time::Instant::now()<deadline,"the window only an earlier frame demanded completes");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn an_unrelated_effect_format_change_reuses_ready_canonical_image_results() {
    let extent=[32;2];
    let (doc,owner,_)=document(extent,vec![ImageObject::new(image(extent,[255,0,0,255]))]);
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    crate::memory::memory_tests::legacy_scene_frame(&mut r,packet(doc.scene(),extent));
    let mut scene=Scene::new(&r).unwrap();scene.object_display=true;
    let window=ObjectWindow {origin:[0.;2],side:1.,size:extent};
    let request=canonical_request(&r,&mut scene,&doc,owner,window);
    assert_eq!(request.format,wgpu::TextureFormat::Rgba16Float);
    let mut cache=super::super::object_cache::ObjectCache::for_scene(&scene,true);
    assert!(cache.resolve_collection(&r,doc.scene(),&request,None).unwrap().is_none());
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
    let result=loop {
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        r.drain_image_decodes(&mut encoder).unwrap();cache.advance(&mut r,&mut scene,&mut encoder,&mut Scene::begin_work(false)).unwrap();
        r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
        if let Some(result)=cache.resolve_collection(&r,doc.scene(),&request,None).unwrap() {break result;}
        assert!(std::time::Instant::now()<deadline,"canonical image preparation completes");
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    assert_eq!(result.texture().format(),wgpu::TextureFormat::Rgba32Float);
    let before=cache.request_windows();assert_eq!(before.len(),1);
    let mut artwork=doc.artwork.clone();
    let instance=layer_core::EffectInstance::new(crate::tests::fixture("invert").program());
    let application=artwork.effects.insert(PortableId::random(),EffectApplication::new(instance.program,instance.values,extent)).unwrap();
    let effect=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Effect(application),"Invert")).unwrap();
    artwork.stacks.get_mut(doc.composition().result).unwrap().entries.insert(0,effect);
    let changed=layer_core::Document::from_artwork(artwork).unwrap();
    let request=canonical_request(&r,&mut scene,&changed,owner,window);
    assert_eq!(request.format,wgpu::TextureFormat::Rgba32Float);
    assert!(cache.resolve_collection(&r,changed.scene(),&request,None).unwrap().is_some(),"an unrelated preview format change reuses the exact F32 result");
    let after=cache.request_windows();assert_eq!(after.len(),1,"no canonical work is admitted");
    assert!(before[0].0.ptr_eq(&after[0].0));
    assert!(float_pixels(&r,result.texture()).iter().all(|pixel|*pixel==[1.,0.,0.,1.]));
}

#[test]
fn object_frame_work_uses_the_scene_grant_beyond_cache_retention() {
    let extent = [4096, 3072];
    let (doc, owner, _) = document(extent, vec![ImageObject::new(image(extent, [31, 73, 149, 255]))]);
    let mut r = crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    r.begin_frame_memory(packet(doc.scene(), extent));
    let mut scene = Scene::new(&r).unwrap();
    let request = canonical_request(&r, &mut scene, &doc, owner,
        ObjectWindow {origin: [0.; 2], side: 1., size: [3000, 2000]});
    assert!(u64::from(request.window.size[0]) * u64::from(request.window.size[1]) * 16
        > (crate::memory::OBJECT_CACHE_BYTES / 2) + r.memory.working_limit());
    assert!(matches!(scene.object_results.resolve_collection(&r, doc.scene(), &request,None), Ok(None)));
    let empty=super::super::object_cache::ObjectCache::for_scene(&scene,false);
    let mut cache=std::mem::replace(&mut scene.object_results,empty);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    pollster::block_on(r.compilation.prepare_all(r.scene_pipelines.objects.pipelines())).unwrap();
    cache.advance(&mut r, &mut scene, &mut encoder,&mut Scene::begin_work(false)).unwrap();
    assert!(cache.pinned_gpu_bytes() > crate::memory::OBJECT_CACHE_BYTES / 2);
    r.uploads.finish(&encoder);
    encoder.submit(&r.queue);
    r.wait_idle().unwrap();
}

#[test]
fn a_downscaled_object_under_drained_headroom_queues_once_within_the_display_floor() {
    let mut object = ImageObject::new(image([2048; 2], [255, 0, 0, 255]));
    object.affine = Affine64([0.5, 0., 0., 0.5, 0., 0.]);
    let (doc, owner, _) = document([1024; 2], vec![object]);
    let mut r = crate::memory::memory_tests::renderer(doc.composition().color,
        layer_core::DeviceMemoryFacts {unified: true, ..Default::default()}).unwrap();
    let budget = r.factory.memory_budget().clone();
    budget.set_ram_override(Some(384 << 20));
    r.memory_demand(packet(doc.scene(), [1024; 2]));
    r.refresh_memory_limits();
    let _import = budget.admit_import(budget.import_headroom_bytes()).unwrap();
    assert_eq!(budget.import_headroom_bytes(), 0);
    r.begin_frame_memory(packet(doc.scene(), [1024; 2]));
    let mut scene = Scene::new(&r).unwrap();
    let request = canonical_request(&r, &mut scene, &doc, owner,
        ObjectWindow {origin: [0.; 2], side: 1., size: [1024; 2]});
    for _ in 0..2 {
        assert!(matches!(scene.exact_object_results.resolve_collection(&r, doc.scene(), &request,None), Ok(None)));
    }
    assert_eq!(scene.exact_object_results.work_progress().0, 1, "a repeated request queues one unit, not a refusal each frame");
}

#[test]
fn object_only_native_and_display_composite_ordered_siblings_and_raw_queries() {
    let mut front = ImageObject::new(image([8; 2], [0, 0, 255, 128]));
    front.affine = Affine64([1., 0., 0., 1., 4., 4.]);
    let back = ImageObject::new(image([8; 2], [255, 0, 0, 255]));
    let (mut doc, owner, _) = document([16; 2], vec![front, back]);
    assert!(doc.scene().source_target(owner).is_none());
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    submit_ready(&mut r, packet(doc.scene(), doc.composition().size));
    let native_pixels = native(&mut r, &doc, Output::Artwork(None));
    let display = r.scale_display.as_ref().unwrap().texture().clone();
    assert!(crate::test_support::max_error(&native_pixels, &float_pixels(&r, &display)) < 2e-5);
    let alpha = 128. / 255.;
    assert!(crate::test_support::max_error(&native_pixels[5 * 16 + 5..5 * 16 + 6], &[[1. - alpha, 0., alpha, 1.]]) < 2e-5);
    let raw_pixels = native(&mut r, &doc, Output::Objects(owner));
    let coverage = doc.artwork.coverage.next_handle();
    let mut mask = layer_core::CoverageSnapshot::reveal_all(coverage, [16; 2], Default::default());
    mask.source.default_coverage = 0.5;
    doc.artwork.coverage.insert(PortableId::random(), mask.source).unwrap();
    let occurrence = doc.artwork.occurrences.get_mut(owner).unwrap();
    occurrence.opacity = 0.25; occurrence.mask = Some(mask.use_); occurrence.visible = false;
    let scoped = SceneScope::RawObjects(owner);
    let raw_frame = packet(doc.scene().with_scope(&scoped), doc.composition().size);
    let (target, _) = create_color_target(&r.device, [16; 2], "raw hidden images");
    let mut scene = r.scene.take().unwrap();
    let mut captured = false;
    for _ in 0..5000 {
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let result = scene.capture_region_prepared(&mut r, raw_frame, &target, PixelRect::full([16; 2]), Output::Objects(owner), &mut encoder);
        if let Err(error) = &result { assert!(matches!(error, GpuRasterError::NotReady), "{error:?}"); }
        r.uploads.finish(&encoder); encoder.submit(&r.queue); r.wait_idle().unwrap();
        if result.is_ok() { captured = true; break; }
    }
    r.scene = Some(scene);
    assert!(captured, "hidden raw images complete through the deferred query path");
    assert!(crate::test_support::max_error(&raw_pixels, &float_pixels(&r, &target)) < 2e-5);
}

#[test]
fn object_affine_preflight_rejects_unsupported_sampling_without_changing_the_scene() {
    let (doc, _, children) = document([64; 2], vec![ImageObject::new(image([8; 2], [255; 4]))]);
    let r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let view = packet(doc.scene(), [64; 2]).view;
    let before = doc.artwork.clone();
    let unsupported = Affine64([0.0001, 0., 0., 0.0001, 0., 0.]);
    assert!(r.preflight_image_object_affine(doc.scene(), children[0], unsupported, view).is_err());
    assert_eq!(doc.artwork, before);
    let far = Affine64([1., 0., 0., 1., 1e15, -1e15]);
    r.preflight_image_object_affine(doc.scene(), children[0], far, view).unwrap();
}

#[test]
fn object_damage_excludes_unchanged_children() {
    let shared = image([8; 2], [255; 4]);
    let mut distant = ImageObject::new(shared.clone()); distant.affine.0[4] = 600.;
    let (mut doc, _, children) = document([768, 256], vec![ImageObject::new(shared), distant]);
    let before = doc.scene().snapshot();
    doc.artwork.objects.get_mut(children[0]).unwrap().affine.0[4] = 32.;
    let damage = edited_damage(before.view(), doc.scene(), [768, 256]).unwrap();
    assert!(!damage.is_empty());
    assert!(damage.regions.iter().all(|region| region.max_x() < 100), "unchanged child bounds remain reusable");
}

#[test]
fn affine_edits_clear_vacated_bounds_and_keep_shared_source_residency() {
    let shared = image([16; 2], [255, 0, 0, 255]);
    let mut second = ImageObject::new(shared.clone());
    second.affine.0[4] = 48.;
    let (mut doc, _, children) = document([768, 256], vec![ImageObject::new(shared), second]);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    submit_ready(&mut r, packet(doc.scene(), doc.composition().size));
    let misses = r.source_cache_work()[1];
    for position in [96., 144., 192., 0.] {
        doc.artwork.objects.get_mut(children[0]).unwrap().affine.0[4] = position;
        let work = r.metrics.composited_pixels;
        submit_ready(&mut r, packet(doc.scene(), doc.composition().size));
        assert!(r.metrics.composited_pixels - work < 768 * 256, "object movement does not replay the full frame");
        assert_eq!(r.source_cache_work()[1], misses, "affine edits share immutable decoded source tiles");
        let expected = native(&mut r, &doc, Output::Artwork(None));
        let actual = float_pixels(&r, &r.scale_display.as_ref().unwrap().texture().clone());
        assert!(crate::test_support::max_error(&actual, &expected) < 2e-5, "old and new object bounds repaint");
    }
}

#[test]
fn object_read_mapping_preserves_f64_relative_coordinates_and_requested_density() {
    let mut object = ImageObject::new(image([2; 2], [255, 0, 0, 255]));
    object.affine.0[4] = 1_048_576.125;
    let (doc, owner, _) = document([16; 2], vec![object]);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.document_extent = [16; 2];
    let mut scene = Scene::new(&r).unwrap();
    for side in [0.25, 0.5, 1., 2.] {
        let request = canonical_request(&r, &mut scene, &doc, owner, ObjectWindow { origin: [1_048_576.125, 0.], side, size: [8; 2] });
        let mut cache = super::super::object_cache::ObjectCache::for_scene(&scene,false);
        let deadline = std::time::Instant::now() + crate::READBACK_TIMEOUT;
        let view = loop {
            if let Some(view) = cache.resolve_collection(&r, doc.scene(), &request,None).unwrap() { break view; }
            let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
            r.drain_image_decodes(&mut encoder).unwrap();
            cache.advance(&mut r, &mut scene, &mut encoder,&mut Scene::begin_work(true)).unwrap();
            r.uploads.finish(&encoder); encoder.submit(&r.queue); r.wait_idle().unwrap();
            assert!(std::time::Instant::now() < deadline, "object sampling at side {side} must finish through the decode queue");
        };
        let pixels = float_pixels(&r, view.texture());
        assert!(pixels[0][0] > 0. && pixels[0][3] > 0.);
        assert!(pixels.iter().all(|pixel| pixel.iter().all(|v| v.is_finite())));
    }
}
#[test]
fn private_canonical_jobs_yield_and_cancel_before_publication() {
    let mut object = ImageObject::new(image([512; 2], [255, 0, 0, 255]));
    object.affine = Affine64([1. / 16., 0., 0., 1. / 16., 0., 0.]);
    let (mut doc, owner, children) = document([64; 2], vec![object]);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.document_extent = [64; 2];
    let mut compositor=Scene::new(&r).unwrap();
    let mut cache = super::super::object_cache::ObjectCache::for_scene(&compositor,false);
    let request = canonical_request(&r, &mut compositor, &doc, owner, ObjectWindow { origin: [0.; 2], side: 1., size: [16; 2] });
    assert!(cache.resolve_collection(&r,doc.scene(),&request,None).unwrap().is_none());
    let mut complete = None;
    for _ in 0..300 {
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        r.prepare_moving_images(packet(doc.scene(), [64; 2]), &mut encoder).unwrap();
        let passes = encoder.pass_count();
        let taps = cache.advance(&mut r, &mut compositor, &mut encoder,&mut Scene::begin_work(true)).unwrap();
        let limit = super::super::object_cache::INTERACTIVE_TAPS + crate::object_sampling::DISPATCH_TAPS;
        assert!(taps <= limit && (encoder.pass_count() - passes) * super::super::object_cache::PASS_TAPS <= limit, "canonical refinement has a bounded per-frame workload");
        r.uploads.finish(&encoder); encoder.submit(&r.queue);
        if !cache.pending() {
            complete = cache.resolve_collection(&r,doc.scene(),&request,None).unwrap();
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let view = complete.expect("canonical work completes across bounded frames");
    let pixels = float_pixels(&r, view.texture());
    assert!(pixels[8 * 16 + 8][0] > 0.999);
    assert!(cache.bytes() < 64 * 1024);
    let reused = cache.resolve_collection(&r,doc.scene(),&request,None).unwrap().unwrap();
    assert_eq!(view, reused, "multiple consumers share the completed result within one submission");
    assert!(!cache.pending());
    let encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    cache.flush_retired(&encoder); r.uploads.finish(&encoder); r.last_submission = encoder.submit(&r.queue); r.wait_idle().unwrap();
    assert!(cache.resolve_collection(&r,doc.scene(),&request,None).unwrap().is_none(), "a query cache releases encoded results");
    doc.artwork.objects.get_mut(children[0]).unwrap().affine.0[4] = 32.;
    cache.retain(doc.scene(),layer_core::BlendSpace::Linear,r.device.working_space());
    assert!(!cache.pending(), "obsolete affine work cannot publish");
    assert!(cache.bytes() < 64 * 1024);
}

fn ordered_capture_before_deadline(scene:&mut Scene,r:&mut WgpuRasterizer) {
    let cancelled=Arc::new(std::sync::atomic::AtomicBool::new(false));
    let wake=Arc::new(std::sync::Mutex::new(None::<std::task::Waker>));
    r.snapshot_cancelled=Some(cancelled.clone());r.snapshot_decode_wake=Some(wake.clone());
    let started=std::time::Instant::now();
    let result=std::thread::scope(|scope| {
        let (done,waiting)=std::sync::mpsc::channel();
        scope.spawn(move||if waiting.recv_timeout(crate::READBACK_TIMEOUT).is_err() {
            cancelled.store(true,std::sync::atomic::Ordering::Relaxed);
            let signal=wake.lock().unwrap().take();if let Some(signal)=signal {signal.wake();}
        });
        let result=pollster::block_on(scene.prepare_capture_work_async(r));
        let _=done.send(());result
    });
    result.expect("ordered worker capture must complete before its deadline");
    assert!(started.elapsed()<crate::READBACK_TIMEOUT,"ordered worker capture exceeded its deadline");
}

fn ordered_worker_captures(single_capture: bool) {
    let side=if single_capture {512}else {2048};
    let colors=if single_capture {vec![[255,0,0,255]]}else {vec![[255,0,0,255],[0,0,255,255]]};
    let objects=colors.iter().map(|color| {
        let mut object=ImageObject::new(image([side;2],*color));
        let scale=64./f64::from(side);
        object.affine=Affine64([scale,0.,0.,scale,0.,0.]);
        object
    }).collect();
    let (doc,_,handles)=document([64;2],objects);
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.snapshot_worker=true;
    r.document_extent=[64;2];
    pollster::block_on(r.compilation.prepare_all(r.scene_pipelines.objects.pipelines())).unwrap();
    pollster::block_on(r.compilation.prepare_all([r.scene_pipelines.source.for_format(wgpu::TextureFormat::Rgba8Unorm)])).unwrap();
    r.wait_idle().unwrap();
    let mut scene=Scene::new(&r).unwrap();
    scene.ordered=true;
    scene.exact_object_results=super::super::object_cache::ObjectCache::for_scene(&scene,true);
    if !single_capture {
        assert!(u64::from(side).pow(2)*handles.len() as u64+64*64*handles.len() as u64
            >super::super::object_cache::IDLE_TAPS);
    }
    let mut retained=Vec::new();
    for (index,handle) in handles.into_iter().enumerate() {
        let owner=doc.scene().object_owner(handle).unwrap();
        let request=canonical_request(&r,&mut scene,&doc,owner,ObjectWindow {origin:[0.;2],side:1.,size:[64;2]});
        assert!(scene.exact_object_results.resolve_collection(&r,doc.scene(),&request,None).unwrap().is_none());
        ordered_capture_before_deadline(&mut scene,&mut r);
        let output=scene.exact_object_results.resolve_collection(&r,doc.scene(),&request,None).unwrap()
            .expect("completed capture must retain its exact result");
        let pixels=float_pixels(&r,output.texture());
        let expected=colors[index].map(|channel|f32::from(channel)/255.);
        assert!(crate::test_support::max_error(&[pixels[32*64+32]],&[expected])<2e-5);
        retained.push(output);
    }
    for (output,color) in retained.iter().zip(colors) {
        let pixels=float_pixels(&r,output.texture());
        let expected=color.map(|channel|f32::from(channel)/255.);
        assert!(crate::test_support::max_error(&[pixels[32*64+32]],&[expected])<2e-5,
            "the next worker capture must preserve the earlier exact result");
    }
}

#[test]
fn ordered_worker_capture_completes_a_cold_exact_object() {
    ordered_worker_captures(true);
}

#[test]
fn consecutive_ordered_captures_have_independent_submission_budgets() {
    ordered_worker_captures(false);
}

#[test]
fn required_canonical_sources_follow_only_the_next_bounded_task() {
    let objects = [[255, 0, 0, 255], [0, 0, 255, 255]].map(|color| {
        let mut object = ImageObject::new(image([8192, 256], color));
        object.affine = Affine64([1. / 32., 0., 0., 1., 0., 0.]);
        object
    });
    let (doc, owner, handles) = document([256; 2], objects.into());
    let r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut scene = Scene::new(&r).unwrap();
    let mut cache = super::super::object_cache::ObjectCache::for_scene(&scene,false);
    for handle in handles {
        let owner = doc.scene().object_owner(handle).unwrap();
        let request = canonical_request(&r, &mut scene, &doc, owner,
            ObjectWindow { origin: [0.; 2], side: 1., size: [256; 2] });
        assert!(cache.resolve_collection(&r, doc.scene(), &request,None).unwrap().is_none());
    }
    let mut required = Vec::new();
    cache.required_sources(&mut required).unwrap();
    let first = doc.scene().object_layer(owner).unwrap();
    assert_eq!(required.len(), 18);
    assert!(required.iter().all(|(source, _)| Arc::ptr_eq(source, first.image.storage())));
    assert_eq!(required.iter().map(|(_, coordinate)| *coordinate).collect::<Vec<_>>(),
        (0..18).map(|x| [x, 0]).collect::<Vec<_>>());
}
#[test]
fn object_masks_attached_effects_and_clipping_follow_both_stack_evaluators() {
    let (mut doc, owner, _) = document([64; 2], vec![ImageObject::new(image([16; 2], [255, 0, 0, 255]))]);
    let mut clip_image = ImageObject::new(image([16; 2], [0, 0, 255, 128]));
    clip_image.affine.0[4] = 8.; clip_image.affine.0[5] = 8.;
    let child = doc.artwork.objects.insert(PortableId::random(), clip_image).unwrap();
    let mut clip = Occurrence::new(OccurrenceContent::Objects(child), "Clipped image"); clip.attachment = layer_core::Attachment::Clip;
    let clip = doc.artwork.occurrences.insert(PortableId::random(), clip).unwrap();
    let instance = layer_core::EffectInstance::new(crate::tests::fixture("invert").program());
    let application = doc.artwork.effects.insert(PortableId::random(), EffectApplication::new(instance.program, instance.values, [64; 2])).unwrap();
    let mut effect = Occurrence::new(OccurrenceContent::Effect(application), "Invert"); effect.attachment = layer_core::Attachment::Effect;
    let effect = doc.artwork.occurrences.insert(PortableId::random(), effect).unwrap();
    let coverage = doc.artwork.coverage.next_handle();
    let mut mask = layer_core::CoverageSnapshot::reveal_all(coverage, [64; 2], Default::default()); mask.source.default_coverage = 0.5;
    doc.artwork.coverage.insert(PortableId::random(), mask.source).unwrap();
    doc.artwork.occurrences.get_mut(owner).unwrap().mask = Some(mask.use_);
    doc.artwork.occurrences.get_mut(owner).unwrap().opacity = 0.25;
    let stack = doc.composition().result;
    doc.artwork.stacks.get_mut(stack).unwrap().entries = vec![clip, effect, owner];
    let doc = layer_core::Document::from_artwork(doc.artwork).unwrap();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    submit_ready(&mut r, packet(doc.scene(), doc.composition().size));
    let expected = native(&mut r, &doc, Output::Artwork(None));
    let actual = float_pixels(&r, &r.scale_display.as_ref().unwrap().texture().clone());
    assert!(crate::test_support::max_error(&actual, &expected) < 2e-5);
    let pixel = actual[10 * 64 + 10];
    assert!((pixel[3] - 0.125).abs() < 2e-5, "outer properties apply once: {pixel:?}");
    assert!(pixel[0] < 2e-5 && pixel[2] > 0.12, "local effect precedes clipping: {pixel:?}");
    assert_eq!(actual[20 * 64 + 20][3], 0., "clipped image cannot expand the base shape");
}

#[test]
fn coarse_linear_object_node_includes_source_outside_its_output_cell() {
    let mut object = ImageObject::new(image([8; 2], [255, 0, 0, 255]));
    object.affine.0[4] = 200.; object.affine.0[5] = 200.;
    let (doc, _, _) = document([8192; 2], vec![object]);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), [8192; 2]);
    frame.view.width_px = 1; frame.view.height_px = 1;
    frame.view.document_to_surface = [1. / 256., 0., 0., 1. / 256., -1., -1.];
    let window = PixelRect::new(256, 256, 512, 512);
    r.document_extent = [8192; 2];
    r.scale_display = Some(super::super::scale::Cache::new(&r, super::super::scale::Request {
        plan: display_mips::Plan::window([8192; 2], 8, window), evaluation: super::super::scale::Evaluation::Display,
    }, 1));
    let mut scene = r.scene.take().unwrap();
    let mut settled = false;
    for iteration in 0..2000 {
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        if r.prepare_moving_images(frame, &mut encoder).unwrap() { scene.invalidate_object_previews([8192; 2]); }
        let composed = scene.compose(&mut r, frame, if iteration == 0 { window } else { PixelRect::EMPTY }, &mut encoder, None, &mut Scene::begin_work(false));
        if let Err(error) = &composed { assert!(matches!(error, GpuRasterError::NotReady), "{error:?}"); }
        r.uploads.finish(&encoder); r.last_submission = encoder.submit(&r.queue); r.wait_idle().unwrap();
        if composed.is_ok() && !scene.objects_pending() && !r.moving_images.pending() && !r.image_decode_waiting() { settled = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(settled);
    r.scene = Some(scene);
    let cache = r.scale_display.as_ref().unwrap();
    assert_eq!(cache.plan.level, 8);
    assert!(cache.evaluation == super::super::scale::Evaluation::Display);
    let pixels = float_pixels(&r, cache.texture());
    assert_eq!(cache.plan.size, [1; 2]);
    let index = 0;
    let weight: f64 = (0..8).map(|i| 1. - ((f64::from(i) + 0.5 - 184.).abs() / 256.)).sum();
    let expected = (weight * weight / (256. * 256.)) as f32;
    assert!(expected > 0.);
    assert!((pixels[index][0] - expected).abs() < 2e-7, "coarse tent includes pixels beyond the allocation cell: {:?}, {expected}", pixels[index]);
    assert!((pixels[index][3] - expected).abs() < 2e-7);
}

#[test]
fn nearest_reduced_object_display_keeps_level_zero_aliases_after_idle() {
    let mut builder = SourceBuilder::new([256; 2], SourceInterpretation { channels: SourceChannels::Rgba,
        depth: SampleDepth::U8, profile: Default::default(), profile_assumed: false }, 1024 * 1024).unwrap();
    for y in 0..256 { builder.push_row(&(0..256).flat_map(|x| {
        let value = if (x + y) % 2 == 0 { 255 } else { 0 }; [value, value, value, 255]
    }).collect::<Vec<_>>()).unwrap(); }
    let mut object = ImageObject::new(AuthoredImage::new(Arc::new(builder.finish().unwrap())));
    object.interpolation = ImageInterpolation::Nearest;
    object.affine = Affine64([0.25, 0., 0., 0.25, 0., 0.]);
    let (doc, _, _) = document([64; 2], vec![object]);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), [64; 2]);
    frame.view.width_px = 8; frame.view.height_px = 8;
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    submit_ready(&mut r, frame);
    for _ in 0..3 { crate::test_support::submit_ready(&mut r, FramePacket { composite_all: false, ..frame }).unwrap(); }
    let cache = r.scale_display.as_ref().unwrap();
    let pixels = float_pixels(&r, cache.texture());
    assert!(pixels.iter().all(|pixel| pixel[..3] == [1.; 3] && pixel[3] == 1.), "Nearest never uses averaged source or native-result levels");
    assert!(r.next_frame()!=layer_render::FrameDemand::Work);
}

#[test]
fn exact_interactive_objects_read_parked_samples_on_decode_workers() {
    let (doc,owner,_)=document([32;2],vec![ImageObject::new(image([32;2],[255,0,0,255]))]);
    let store=crate::test_support::GatedStore::new(&layer_core::Editor::new(doc.clone()).retained_tiles(),false);
    store.gate.release();
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut compositor=Scene::new(&r).unwrap();compositor.object_spatial.prepare(doc.scene());
    let request=canonical_request(&r,&mut compositor,&doc,owner,ObjectWindow {origin:[0.;2],side:1.,size:[32;2]});
    assert!(!request.live && !r.snapshot_worker);
    let mut cache=super::super::object_cache::ObjectCache::for_scene(&compositor,false);
    assert!(cache.resolve_collection(&r,doc.scene(),&request,None).unwrap().is_none());
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
    let result=loop {
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        r.drain_image_decodes(&mut encoder).unwrap();cache.advance(&mut r,&mut compositor,&mut encoder,&mut Scene::begin_work(false)).unwrap();
        r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
        if let Some(view)=cache.resolve_collection(&r,doc.scene(),&request,None).unwrap() {break view;}
        assert!(std::time::Instant::now()<deadline,"exact samples must finish through the decode queue");
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    let readers=store.gate.readers.lock().unwrap();assert!(!readers.is_empty());
    assert!(readers.iter().all(|thread|*thread!=std::thread::current().id()),"the renderer owner must not read/decode parked source samples");
    assert!(float_pixels(&r,result.texture()).iter().all(|pixel|*pixel==[1.,0.,0.,1.]));
}

#[test]
fn canonical_results_at_the_request_limit_survive_a_fitting_grant_until_consumed() {
    let extent=[256;2];
    let (doc,owner,_)=document(extent,vec![ImageObject::new(image(extent,[255,0,0,255]))]);
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    crate::memory::memory_tests::legacy_scene_frame(&mut r,packet(doc.scene(),extent));
    let mut scene=Scene::new(&r).unwrap();scene.object_display=true;scene.object_spatial.prepare(doc.scene());
    let mut cache=super::super::object_cache::ObjectCache::for_scene(&scene,true);
    let requests=(0..16).flat_map(|y|(0..16).map(move|x|[x,y])).map(|coordinate| {
        let request=canonical_request(&r,&mut scene,&doc,owner,ObjectWindow {
            origin:coordinate.map(|value|f64::from(value*16+8)),side:1.,size:[1;2]});
        assert!(cache.resolve_collection(&r,doc.scene(),&request,None).unwrap().is_none());request
    }).collect::<Vec<_>>();
    let mut consumed=std::collections::BTreeSet::new();
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
    loop {
        scene.begin_frame();
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        r.drain_image_decodes(&mut encoder).unwrap();cache.advance(&mut r,&mut scene,&mut encoder,&mut Scene::begin_work(false)).unwrap();
        cache.flush_retired(&encoder);
        r.uploads.finish(&encoder);r.last_submission=encoder.submit(&r.queue);r.wait_idle().unwrap();
        let completed=cache.work_progress().1;
        if completed!=0 {
            assert_eq!(cache.evict_to(u64::MAX),0,"a full request queue must not evict fitting results before consumption");
            assert_eq!(cache.work_progress().1,completed);
            for (index,request) in requests.iter().enumerate().filter(|(index,_)|!consumed.contains(index)).collect::<Vec<_>>() {
                if let Some(result)=cache.resolve_collection(&r,doc.scene(),request,None).unwrap() {
                    assert_eq!(float_pixels(&r,result.texture()),vec![[1.,0.,0.,1.]]);consumed.insert(index);
                }
            }
            if consumed.len()==requests.len() {break;}
        }
        assert!(std::time::Instant::now()<deadline,"accepted canonical work must complete");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn attempt(r:&mut WgpuRasterizer)->bool {
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    r.apply_frame_grants(&mut encoder);
    let mut scene=r.scene.take().unwrap();scene.begin_frame();
    r.drain_image_decodes(&mut encoder).unwrap();
    let empty=super::super::object_cache::ObjectCache::for_scene(&scene,false);
    let mut cache=std::mem::replace(&mut scene.object_results,empty);
    cache.advance(r,&mut scene,&mut encoder,&mut Scene::begin_work(false)).unwrap();
    cache.flush_retired(&encoder);
    let complete=!cache.pending();scene.object_results=cache;r.scene=Some(scene);
    r.apply_frame_grants(&mut encoder);
    r.uploads.finish(&encoder);r.last_submission=encoder.submit(&r.queue);r.wait_idle().unwrap();
    complete
}
#[test]
fn canonical_tasks_waiting_for_reads_start_queued_work_only_within_the_display_grant() {
    let objects=(0..8u8).map(|index|ImageObject::new(image([4*PAGE_SIZE;2],[31+index*23,67,113,255]))).collect();
    let (mut doc,_,handles)=document([4*PAGE_SIZE;2],objects);
    let root=doc.artwork.root;doc.artwork.compositions.get_mut(root).unwrap().blend=layer_core::BlendSpace::Linear;
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.document_extent=[4*PAGE_SIZE;2];
    let mut scene=Scene::new(&r).unwrap();scene.object_display=true;
    let mut queued=0;
    for &handle in &handles {for window in 0..8 {
        let origin=[f64::from(window%4*PAGE_SIZE),f64::from(window/4*PAGE_SIZE)];
        let request=canonical_request(&r,&mut scene,&doc,doc.scene().object_owner(handle).unwrap(),ObjectWindow {origin,side:1.,size:[PAGE_SIZE;2]});
        assert!(scene.object_results.resolve_collection(&r,doc.scene(),&request,None).unwrap().is_none());
        queued+=super::super::object_cache::working_set(&request.child(0,request.part(0),r.empty_view.clone()).unwrap()).unwrap().0;
    }}
    scene.object_display=false;r.scene=Some(scene);
    assert!(queued>crate::memory::OBJECT_CACHE_BYTES);
    let (mut pinned,mut budget)=(0,u64::MAX);
    let settled=(0..4000).any(|_| {
        let complete=attempt(&mut r);
        budget=budget.min(r.memory.display_owner().grant().gpu_bytes);
        pinned=pinned.max(r.scene.as_ref().unwrap().object_results.memory_bytes()[1]);complete
    });
    assert!(pinned<=budget,"tasks waiting for reads start no more work than the display grant holds: {pinned} bytes pinned, {budget} grant, {queued} queued");
    assert!(settled,"every queued task completes");
}
#[test]
fn retiring_an_unstarted_window_releases_its_unit_reservation_at_once() {
    let extent=[PAGE_SIZE;2];
    let (doc,owner,_)=document(extent,vec![ImageObject::new(image(extent,[83,141,29,255]))]);
    let r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut scene=Scene::new(&r).unwrap();scene.object_display=true;
    let request=canonical_request(&r,&mut scene,&doc,owner,ObjectWindow {origin:[0.;2],side:1.,size:[PAGE_SIZE;2]});
    let before=scene.object_results.memory_bytes()[1];
    assert!(scene.object_results.admit(&r,crate::residency::Class::Visible,&[request],None).unwrap().is_none());
    assert!(scene.object_results.memory_bytes()[1]>before,"the unit reserves its window");
    scene.object_results.clear();
    assert_eq!(scene.object_results.memory_bytes()[1],before,"a retired window that never started holds no reservation until the next flush");
}
#[test]
fn a_started_canonical_task_finishes_its_parts_under_pressure_while_queued_starts_wait() {
    let extent=[PAGE_SIZE;2];
    let (mut doc,owner,_)=document(extent,vec![ImageObject::new(image(extent,[83,141,29,255]))]);
    let root=doc.artwork.root;doc.artwork.compositions.get_mut(root).unwrap().blend=layer_core::BlendSpace::Linear;
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.document_extent=extent;
    let mut scene=Scene::new(&r).unwrap();scene.object_display=true;
    let window=|row:u32|ObjectWindow {origin:[0.,f64::from(row*PAGE_SIZE/4)],side:0.25,size:[2*super::COLLECTION_PART,PAGE_SIZE]};
    let queue=|r:&WgpuRasterizer,scene:&mut Scene,row:u32| {
        let request=canonical_request(r,scene,&doc,owner,window(row));
        assert_eq!(request.parts(),2);
        assert!(scene.object_results.resolve_collection(r,doc.scene(),&request,None).unwrap().is_none());
    };
    queue(&r,&mut scene,0);
    fn pass(r:&mut WgpuRasterizer,scene:&mut Scene,allowance:u64)->(usize,usize,u64) {
        scene.begin_frame();
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        r.drain_image_decodes(&mut encoder).unwrap();
        let empty=super::super::object_cache::ObjectCache::for_scene(&scene,false);
        let mut cache=std::mem::replace(&mut scene.object_results,empty);
        cache.advance_with_memory(r,scene,&mut encoder,allowance).unwrap();
        cache.flush_retired(&encoder);
        let progress=cache.work_progress();scene.object_results=cache;
        r.uploads.finish(&encoder);r.last_submission=encoder.submit(&r.queue);r.wait_idle().unwrap();
        progress
    }
    let between_parts=(0..4000).any(|_|{let (_,complete,progress)=pass(&mut r,&mut scene,super::super::object_cache::PASS_TAPS);complete==0 && progress>=1_000_000_000_000});
    assert!(between_parts,"the fixture stops between the parts of one started task");
    queue(&r,&mut scene,1);
    let limit=r.scene_tiers.display.limit();
    r.scene_tiers.display.set_limit(r.scene_tiers.display.used().saturating_sub(16<<20),0);
    let finished=(0..400).any(|_|pass(&mut r,&mut scene,super::super::object_cache::IDLE_TAPS).1==1);
    assert!(finished,"a started task continues into its next part without a new admission");
    assert_eq!(scene.object_results.pinned_gpu_bytes(),0,"a queued task whose working set does not fit stays unstarted");
    r.scene_tiers.display.set_limit(limit,0);
    let settled=(0..4000).any(|_|pass(&mut r,&mut scene,super::super::object_cache::IDLE_TAPS).1==2);
    assert!(settled,"the queued task starts once the release makes room");
}
#[test]
fn canonical_disjoint_tasks_keep_pending_inputs_until_their_contribution() {
    fn codes(coordinate:[u32;2])->[u8;4] {[31+coordinate[0] as u8*11,43+coordinate[1] as u8*19,113,255]}
    let source=AuthoredImage::new(rgba8_source([4096;2],|x,y|codes([x/PAGE_SIZE,y/PAGE_SIZE])));
    let (mut doc,owner,_)=document([4096;2],vec![ImageObject::new(source)]);
    let root=doc.artwork.root;doc.artwork.compositions.get_mut(root).unwrap().blend=layer_core::BlendSpace::Linear;
    let store=crate::test_support::GatedStore::new(&layer_core::Editor::new(doc.clone()).retained_tiles(),false);
    let mut r=crate::memory::memory_tests::renderer_for_restore(&doc);
    r.document_extent=[4096;2];let frame=packet(doc.scene(),[4096;2]);
    crate::memory::memory_tests::declare_source_budget(&mut r,frame,32,16<<20,0);
    let mut scene=r.scene.take().unwrap_or_else(||Scene::new(&r).unwrap());scene.object_display=true;
    let coordinates=(0..8).flat_map(|y|(0..16).map(move|x|[x,y])).collect::<Vec<_>>();
    let requests=coordinates.iter().map(|coordinate| {
        let request=canonical_request(&r,&mut scene,&doc,owner,ObjectWindow {
            origin:coordinate.map(|v|f64::from(v*PAGE_SIZE+96)),side:1.,size:[64;2]});
        assert!(request.live && request.display);
        let job=request.child(0,request.part(0),r.empty_view.clone()).unwrap();
        let (_,bounds,pages)=super::super::object_cache::working_set(&job).unwrap();
        assert_eq!(pages,1);assert_eq!(page_coordinates(bounds).collect::<Vec<_>>(),vec![*coordinate]);
        assert!(scene.object_results.resolve_collection(&r,doc.scene(),&request,None).unwrap().is_none());request
    }).collect::<Vec<_>>();
    scene.object_display=false;r.scene=Some(scene);
    let before=crate::test_support::PassCounts::snapshot();
    assert!(!attempt(&mut r));store.gate.entered();store.gate.release();
    let mut settled=false;
    for _ in 1..4000 {if attempt(&mut r) {settled=true;break;}}
    let counts=crate::test_support::PassCounts::snapshot().delta(before);
    assert_eq!(counts.source_input_misses,0);assert_eq!(counts.raster_output_misses,0);
    assert!(settled,"disjoint canonical tasks must settle within4000attempts: {counts:?}");
    assert!(counts.normalizations*100<=requests.len() as u64*105,
        "each disjoint source key normalizes at most1.05times: unique={}, {counts:?}",requests.len());
    let mut scene=r.scene.take().unwrap();
    for (request,coordinate) in requests.iter().zip(coordinates) {
        let view=scene.object_results.resolve_collection(&r,doc.scene(),request,None).unwrap().unwrap();
        let expected=codes(coordinate);let pixels=float_pixels(&r,view.texture());
        for actual in pixels {for channel in 0..3 {
            assert!((actual[channel]-doc.composition().color.space.decode(f64::from(expected[channel])/255.) as f32).abs()<2e-5);
        }assert!((actual[3]-1.).abs()<2e-5);}
    }
    r.scene=Some(scene);
    assert!(store.gate.readers.lock().unwrap().iter().all(|thread|*thread!=std::thread::current().id()));
}

#[test]
fn pending_native_store_reads_keep_the_display_and_wake_exact_preparation() {
    let extent=[32;2];
    let (mut doc,_,objects)=document(extent,vec![ImageObject::new(image(extent,[255,0,0,255]))]);
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    crate::test_support::submit_settled(&mut r,packet(doc.scene(),extent));
    let previous=float_pixels(&r,r.presentation_views().unwrap()[0].texture());
    assert!(previous.iter().all(|pixel|*pixel==[1.,0.,0.,1.]));
    let mut object=doc.artwork.objects.get(objects[0]).unwrap().clone();
    object.image=image(extent,[0,0,255,255]);
    doc.apply(layer_core::Edit::ImageObject(RecordChange::replace(&doc.artwork.objects,objects[0],Some(object)).unwrap())).unwrap();
    let store=crate::test_support::GatedStore::new(&layer_core::Editor::new(doc.clone()).retained_tiles(),false);
    let (wake,woken)=std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move || {let _=wake.send(());}));
    let frame=packet(doc.scene(),extent);
    crate::test_support::submit_ready(&mut r,frame).unwrap();
    crate::test_support::submit_ready(&mut r,frame).unwrap();
    store.gate.entered();
    r.wait_idle().unwrap();
    crate::test_support::submit_ready(&mut r,FramePacket {composite_all:false,..frame}).unwrap();
    assert!(r.image_decode_waiting());
    assert!(!r.moving_images_waiting,"completed mip retirements must admit the incoming image");
    assert!(r.next_frame()!=layer_render::FrameDemand::Work,"a registered completion wake releases native frame demand during stored reads");
    r.analysis_dirty=true;
    assert!(r.next_frame()==layer_render::FrameDemand::Work,"independent ready work must remain runnable");
    r.analysis_dirty=false;
    for _ in 0..2 {
        if r.raster_dependencies_ready(frame,None) {crate::test_support::submit_ready(&mut r, frame).unwrap();}
        assert_eq!(float_pixels(&r,r.presentation_views().unwrap()[0].texture()),previous);
    }
    assert!(r.image_decode_waiting());
    while woken.try_recv().is_ok() {}
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
    store.gate.release();
    woken.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())).unwrap();
    while r.image_decode_waiting() {
        woken.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())).unwrap();
    }
    assert!(!r.image_decode_waiting());
    crate::test_support::submit_settled(&mut r,frame);
    assert!(float_pixels(&r,r.presentation_views().unwrap()[0].texture()).iter().all(|pixel|*pixel==[0.,0.,1.,1.]));
    assert!(r.next_frame()!=layer_render::FrameDemand::Work);
}

#[test]
fn simultaneous_photo_results_bound_batches_and_survive_deferred_retries() {
    let shared = image([4000, 3000], [255, 0, 0, 255]);
    let mut objects = vec![ImageObject::new(shared.clone()), ImageObject::new(shared),
        ImageObject::new(image([4000, 3000], [0, 255, 0, 255])),
        ImageObject::new(image([4000, 3000], [0, 0, 255, 255]))];
    for object in &mut objects { object.affine = Affine64([1. / 7., 0., 0., 1. / 7., 0., 0.]); }
    let (doc, _, _) = document([768; 2], objects);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.document_extent = [768; 2];
    let mut compositor=Scene::new(&r).unwrap();
    let mut cache = super::super::object_cache::ObjectCache::for_scene(&compositor,false);
    let requests: Vec<_> = [[0., 0.], [256., 0.], [0., 256.], [256., 256.]].into_iter()
        .zip(doc.scene().order().iter().copied()).map(|(origin,owner)| canonical_request(&r, &mut compositor, &doc, owner, ObjectWindow { origin, side: 1., size: [256; 2] })).collect();
    for request in &requests { assert!(cache.resolve_collection(&r, doc.scene(), request,None).unwrap().is_none()); }
    let mut complete = false;
    for _ in 0..4000 {
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        r.drain_image_decode(&mut encoder).unwrap();
        let before = encoder.pass_count();
        cache.advance(&mut r, &mut compositor, &mut encoder,&mut Scene::begin_work(true)).unwrap();
        let passes = encoder.pass_count() - before;
        cache.advance(&mut r, &mut compositor, &mut encoder,&mut Scene::begin_work(true)).unwrap();
        assert_eq!(encoder.pass_count() - before, passes, "a cache has only one batch in flight");
        assert!(passes <= 24, "sampling and source preparation remain bounded: {passes}");
        assert!(cache.bytes() <= 64 * 1024 * 1024);
        if let Some(report) = r.device.generate_allocator_report() {
            assert!(report.allocations.iter().filter(|allocation| allocation.name == "image-object sampling parameters").count() <= 1);
        }
        r.uploads.finish(&encoder); r.last_submission = encoder.submit(&r.queue); r.wait_idle().unwrap();
        if !cache.pending() { complete = true; break; }
    }
    assert!(complete, "all four private results make bounded progress");
    let views: Vec<_> = requests.iter().map(|request| cache.resolve_collection(&r, doc.scene(), request,None).unwrap().unwrap()).collect();
    cache.reset_used();
    for (request, view) in requests.iter().zip(&views) {
        assert_eq!(&cache.resolve_collection(&r, doc.scene(), request,None).unwrap().unwrap(), view, "a deferred frame retains every completed result");
    }
    let (destination, _) = create_color_target(&r.device, [256; 2], "retained object result consumer");
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    for view in &views { encoder.copy_texture_to_texture(view.texture().as_image_copy(), destination.as_image_copy(),
        wgpu::Extent3d { width: 256, height: 256, depth_or_array_layers: 1 }); }
    cache.flush_retired(&encoder);
    assert!(cache.bytes() >= 4 * 256 * 256 * 16, "encoded consumers retain charged results through completion");
    r.uploads.finish(&encoder); r.last_submission = encoder.submit(&r.queue); r.wait_idle().unwrap();
    assert!(cache.bytes() < 64 * 1024);
}
#[test]
fn tablet_fit_photo_scene_completes_many_private_windows_within_cache_budget() {
    let shared = image([4000, 3000], [255, 0, 0, 255]);
    let mut objects = vec![ImageObject::new(shared.clone()), ImageObject::new(shared),
        ImageObject::new(image([4000, 3000], [0, 255, 0, 255])),
        ImageObject::new(image([4000, 3000], [0, 0, 255, 255]))];
    for object in &mut objects { object.affine = Affine64([0.55, 0., 0., 0.55, 80., 80.]); object.interpolation = ImageInterpolation::Nearest; }
    let (doc, _, _) = document([4248, 2832], objects);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), [4248, 2832]);
    frame.view.width_px = 1440; frame.view.height_px = 960;
    let scale = 1440. / 4248.; frame.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
    let mut ready = false; let mut peak = 0;
    for iteration in 0..10000 {
        crate::test_support::submit_ready(&mut r, FramePacket { composite_all: iteration == 0, reset_layers: iteration == 0, ..frame }).unwrap();
        let scene = r.scene.as_ref().unwrap();
        let bytes = scene.object_results.bytes(); peak = peak.max(bytes);
        assert!(bytes <= 64 * 1024 * 1024 + resident_object_bytes([4248,2832],4,1), "private sampling scratch and four independently paged output planes stay bounded: {bytes}");
        r.wait_idle().unwrap();
        if r.next_frame()!=layer_render::FrameDemand::Work { ready = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(ready, "a photo viewport must progress beyond its first 64 completed object windows");
    let display = r.scale_display.as_ref().unwrap();
    assert!(display.plan.level > 0);
    let pixels = float_pixels(&r, display.texture());
    let x = 400 >> display.plan.level; let y = 400 >> display.plan.level;
    let pixel = pixels[(y * display.plan.size[0] + x) as usize];
    assert!(pixel[0] > 0.999 && pixel[3] > 0.999, "ordered shared front images publish a complete pose: {pixel:?}");
    eprintln!("tablet Fit private-cache peak={peak}");
}

#[test]
fn tablet_fit_shared_photos_with_paint_above_remain_admitted_during_strokes() {
    let extent = [4248, 2832];
    let shared = image(extent, [180, 90, 40, 255]);
    let objects = [[955.8, 863.76], [1295.64, 637.2], [615.96, 637.2], [955.8, 410.64]].into_iter().map(|[x,y]| {
        let mut object = ImageObject::new(shared.clone());
        object.affine = Affine64([0.55, 0., 0., 0.55, x, y]); object
    }).collect();
    let (mut doc, _, handles) = document(extent, objects);
    for &handle in &handles { let owner=doc.scene().object_owner(handle).unwrap();doc.artwork.occurrences.get_mut(owner).unwrap().opacity=0.35; }
    let (_, base) = crate::test_support::add_paint(&mut doc.artwork, "Original", extent);
    let SourceTarget::Paint(base) = base else { unreachable!() };
    doc.artwork.paint.get_mut(base).unwrap().base = Some(PaintBase { image: shared, offset: [0;2], policy: PaintBasePolicy::SourceProfile });
    let (paint, target) = crate::test_support::add_paint(&mut doc.artwork, "Paint", extent);
    let stack = doc.composition().result;
    let entries = &mut doc.artwork.stacks.get_mut(stack).unwrap().entries;
    entries.retain(|h|*h != paint); entries.insert(0,paint);
    let doc = layer_core::Document::from_artwork(doc.artwork).unwrap();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let zoom = 0.15970339;
    let view = layer_render::ViewState { width_px:1920,height_px:1200,document_to_surface:[zoom,0.,0.,zoom,670.,260.] };
    let frame = FramePacket { view, composite_all:false,..packet(doc.scene(),extent) };
    crate::test_support::submit_ready(&mut r, FramePacket { reset_layers:true,composite_all:true,..frame }).unwrap();
    let mut ready = false;
    let mut scratch_peak = 0;
    for _ in 0..50000 {
        r.wait_idle().unwrap();
        scratch_peak = scratch_peak.max(r.scene.as_ref().unwrap().pool.len());
        assert!(scratch_peak <= 64,"cold retry scratch stays bounded: {scratch_peak} tiles");
        if r.next_frame()!=layer_render::FrameDemand::Work { ready = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(1));
        crate::test_support::submit_ready(&mut r, frame).unwrap();
    }
    assert!(ready,"shared photo cold preparation settles");
    let mut ink = crate::layer_tests::dab([0.2,0.6,0.9,1.]);
    ink.radii = [512.;2]; ink.contact = [1.,0.,0.,0.];
    let style = crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen);
    for step in 0..160 {
        ink.center = layer_core::Point {x:2100.+(step as f32*0.1).cos()*1200.,y:1400.+(step as f32*0.1).sin()*800.};
        let mut batch = crate::test_support::dab_batch(target,style.clone(),ink.bounds());
        batch.stroke_start = step == 0; batch.stroke_end = step ==159;
        crate::test_support::submit_ready(&mut r, FramePacket {dabs:std::slice::from_ref(&ink),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
        r.wait_idle().unwrap();
        assert!(r.scene.as_ref().unwrap().object_results.bytes() <= 64*1024*1024 + resident_object_bytes(extent,4,2));
        scratch_peak = scratch_peak.max(r.scene.as_ref().unwrap().pool.len());
        assert!(scratch_peak <= 64,"stroke scratch stays bounded: {scratch_peak} tiles");
    }
    ready = false;
    let start = std::time::Instant::now();
    let mut checkpoint = start;
    let mut previous = None;
    while start.elapsed() < std::time::Duration::from_secs(120) {
        crate::test_support::submit_ready(&mut r, frame).unwrap(); r.wait_idle().unwrap();
        let scene = r.scene.as_ref().unwrap();
        assert!(scene.object_results.bytes() <= 64*1024*1024 + resident_object_bytes(extent,4,2));
        scratch_peak = scratch_peak.max(scene.pool.len());
        assert!(scratch_peak <= 64);
        if r.next_frame()!=layer_render::FrameDemand::Work { ready = true; break; }
        if checkpoint.elapsed() >= std::time::Duration::from_secs(5) {
            let progress = (scene.object_results.work_progress(),r.metrics.composited_pixels);
            assert_ne!(previous,Some(progress),"canonical refinement must advance: {progress:?}");
            eprintln!("post-stroke elapsed={:?} progress={progress:?}",start.elapsed());
            previous = Some(progress); checkpoint = std::time::Instant::now();
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(ready,"shared photo post-stroke refinement settles");
    eprintln!("shared photo stroke scratch peak={scratch_peak} tiles; settle={:?}",start.elapsed());
}

#[test]
fn live_object_read_defers_without_inline_decode_then_returns_canonical_pixels() {
    let mut object = ImageObject::new(image([512; 2], [255, 0, 0, 255]));
    object.affine = Affine64([1. / 7., 0., 0., 1. / 7., 0., 0.]);
    let (doc, owner, _) = document([128; 2], vec![object]);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.document_extent = [128; 2];
    let mut scene = Scene::new(&r).unwrap();
    let (target, _) = create_color_target(&r.device, [64; 2], "prepared reference read");
    let frame = packet(doc.scene(), [128; 2]);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let before = r.device.source_samples.stats();
    let result = scene.capture_region_prepared(&mut r, frame, &target, PixelRect::full([64; 2]), Output::Objects(owner), &mut encoder);
    assert!(matches!(result, Err(GpuRasterError::NotReady)));
    assert_eq!(r.device.source_samples.stats(), before, "the initiating read cannot decompress source pixels");
    assert!(!r.image_decode_waiting(), "initial admission schedules only metadata");
    assert_eq!(encoder.pass_count(), 0);
    r.uploads.finish(&encoder); r.last_submission = encoder.submit(&r.queue); r.wait_idle().unwrap();
    let mut complete = false;
    for _ in 0..500 {
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let result = scene.capture_region_prepared(&mut r, frame, &target, PixelRect::full([64; 2]), Output::Objects(owner), &mut encoder);
        if let Err(error) = &result { assert!(matches!(error, GpuRasterError::NotReady), "{error:?}"); }
        assert!(encoder.pass_count() < 40, "live reads advance bounded GPU work");
        r.uploads.finish(&encoder); r.last_submission = encoder.submit(&r.queue); r.wait_idle().unwrap();
        if result.is_ok() { complete = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(complete);
    let pixels = float_pixels(&r, &target);
    assert!((pixels[32 * 64 + 32][0] - 1.).abs() < 2e-5);
    assert!((pixels[32 * 64 + 32][3] - 1.).abs() < 2e-5);
}

#[test]
fn cold_object_reads_advance_every_finished_tile_and_the_idle_budget_per_attempt() {
    let mut object = ImageObject::new(image([2048; 2], [255, 0, 0, 255]));
    object.affine = Affine64([1. / 16., 0., 0., 1. / 16., 0., 0.]);
    let (doc, owner, _) = document([128; 2], vec![object]);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.document_extent = [128; 2];
    let mut scene = Scene::new(&r).unwrap();
    let (target, _) = create_color_target(&r.device, [128; 2], "prepared reference read");
    let frame = packet(doc.scene(), [128; 2]);
    let mut attempts = 0;
    loop {
        attempts += 1;
        assert!(attempts < 32, "a read of 64 source tiles and 2^24 taps completes in a few host polls");
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let result = scene.capture_region_prepared(&mut r, frame, &target, PixelRect::full([128; 2]), Output::Objects(owner), &mut encoder);
        if let Err(error) = &result { assert!(matches!(error, GpuRasterError::NotReady), "{error:?}"); }
        r.uploads.finish(&encoder); r.last_submission = encoder.submit(&r.queue); r.wait_idle().unwrap();
        if result.is_ok() { break; }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    assert!((float_pixels(&r, &target)[64 * 128 + 64][0] - 1.).abs() < 2e-5);
}

fn cold_query_renderer(doc: &layer_core::Document) -> WgpuRasterizer {
    let mut r = crate::memory::memory_tests::renderer_for_restore(doc);
    r.document_extent = doc.composition().size;
    r.artwork_frame = Some(Arc::new(crate::artwork::Frame::new(packet(doc.scene(), r.document_extent), Default::default())));
    r
}

fn hide_objects(doc: &mut layer_core::Document, owner: OccurrenceHandle) {
    let mut occurrence = doc.artwork.occurrences.get(owner).unwrap().clone();
    occurrence.visible = false;
    doc.apply(layer_core::Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, owner, Some(occurrence)).unwrap())).unwrap();
}

#[test]
fn cold_composite_color_sample_retries_its_frame_before_accepting_the_next_query() {
    use layer_render::{ColorSampleRequest, ColorSampleSource, ColorSampleArea};
    let mut object = ImageObject::new(image([512; 2], [255, 0, 0, 255]));
    object.affine = Affine64([1. / 7., 0., 0., 1. / 7., 0., 0.]);
    let (mut doc, owner, _) = document([128; 2], vec![object]);
    let mut r = cold_query_renderer(&doc);
    let request = ColorSampleRequest { request_id: 100, source: ColorSampleSource::Composite, position: [32; 2], area: ColorSampleArea::Average5 };
    let before = r.device.source_samples.stats();
    assert!(r.request_color_sample(request).unwrap());
    assert!(r.color_sample_pending());
    assert_eq!(r.device.source_samples.stats(), before);
    assert!(!r.request_color_sample(ColorSampleRequest {request_id:101,..request}).unwrap());
    hide_objects(&mut doc, owner);
    r.artwork_frame = Some(Arc::new(crate::artwork::Frame::new(packet(doc.scene(), r.document_extent), Default::default())));
    let deadline = std::time::Instant::now() + crate::READBACK_TIMEOUT;
    let first = loop {
        if let Some(sample) = r.take_color_sample() { break sample.unwrap(); }
        assert!(std::time::Instant::now() < deadline, "cold composite color query must complete");
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    assert_eq!(first.request_id, 100);
    assert!(crate::test_support::max_error(&[first.rgba], &[[1., 0., 0., 1.]]) < 2e-5);
    assert!(r.request_color_sample(ColorSampleRequest {request_id:101,..request}).unwrap());
    let second = loop {
        if let Some(sample) = r.take_color_sample() { break sample.unwrap(); }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    assert_eq!(second.request_id, 101);
    assert_eq!(second.rgba, [0.; 4]);
    assert!(!r.color_sample_pending());
}

#[test]
fn cold_composite_region_classification_progresses_beyond_its_capture_cache() {
    use layer_render::{RegionRequest, RegionSource};
    let extent = [257, 8449];
    let mut object = ImageObject::new(image([1; 2], [255, 0, 0, 255]));
    object.interpolation = ImageInterpolation::Nearest;
    object.affine = Affine64([f64::from(extent[0]), 0., 0., f64::from(extent[1]), 0., 0.]);
    let (doc, _, _) = document(extent, vec![object]);
    let mut r = cold_query_renderer(&doc);
    let before = r.device.source_samples.stats();
    assert!(r.request_region(RegionRequest { fill_areas: None, colour_mode: Default::default(), fill: None, reference_count: None, paper: None,request_id:200,source:RegionSource::Composite,position:[0;2],tolerance:0.,contiguous:false,selection:None,refinement:Default::default(),limit:None,enclosure:None}.into()).unwrap());
    assert_eq!(r.device.source_samples.stats(), before);
    let deadline = std::time::Instant::now() + crate::READBACK_TIMEOUT;
    let result = loop {
        if let Some(result) = r.take_region() { break crate::test_support::mask_result(result.unwrap()); }
        assert!(std::time::Instant::now() < deadline, "classification must retain completed batches while cold windows advance");
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    assert_eq!(result.request_id, 200);
    assert_eq!(result.pixels.coverage_bounds(), [0, 0, extent[0], extent[1]]);
    let count = result.pixels.pixels_per_word();
    let stride = extent[0].div_ceil(count);
    for y in 0..extent[1] { for x in 0..extent[0] {
        let word = result.pixels.words()[(y * stride + x / count) as usize];
        assert_eq!((word >> ((x % count) * 4)) & 15, 4, "coverage at {x},{y}");
    }}
    assert!(!r.region_pending());
}

#[test]
fn cancelling_a_cold_object_region_never_publishes_into_its_replacement() {
    use layer_render::{RegionRequest, RegionSource};
    let (mut doc, owner, _) = document([32; 2], vec![ImageObject::new(image([16; 2], [255, 0, 0, 255]))]);
    let mut r = cold_query_renderer(&doc);
    let request = RegionRequest { fill_areas: None, colour_mode: Default::default(), fill: None, reference_count: None, paper: None,request_id:300,source:RegionSource::Composite,position:[8;2],tolerance:0.,contiguous:true,selection:None,refinement:Default::default(),limit:None,enclosure:None};
    assert!(r.request_region(request.clone().into()).unwrap());
    assert!(r.region_pending());
    r.cancel_region();
    assert!(!r.region_pending());
    hide_objects(&mut doc, owner);
    r.artwork_frame = Some(Arc::new(crate::artwork::Frame::new(packet(doc.scene(), r.document_extent), Default::default())));
    let result = crate::test_support::receive_request(&mut r, RegionRequest {request_id:301,..request});
    assert_eq!(result.request_id, 301);
    assert_eq!(result.pixels.coverage_bounds(), [0, 0, 32, 32]);
    for _ in 0..10 { assert!(r.take_region().is_none()); }
}

#[test]
fn overlapping_sibling_images_keep_bounded_residency_and_composite_order() {
    let colors=[[211,37,83,32],[29,163,61,57],[73,43,197,91]];
    let images=colors.map(|color|image([256;2],color));
    let objects=(0..80).map(|index| {
        let mut object=ImageObject::new(images[index%3].clone());
        object.interpolation=ImageInterpolation::Nearest;object
    }).collect();
    let (mut doc,_,_)=document([256;2],objects);
    for blend in [layer_core::BlendSpace::Linear,layer_core::BlendSpace::Perceptual] {
        doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().blend=blend;
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();r.document_extent=[256;2];
        r.prepare_capture_memory(doc.scene(), [256;2]);
        let (target,_) = create_color_target(&r.device,[256;2],"overlapping sibling images");
        let mut scene=Scene::new(&r).unwrap();let start=std::time::Instant::now();let mut peak=0;
        loop {
            assert!(start.elapsed()<std::time::Duration::from_secs(40),"ordered sibling images make bounded progress");
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            let result=scene.capture_region_prepared(&mut r,FramePacket {blend_space:blend,..packet(doc.scene(),[256;2])},&target,PixelRect::full([256;2]),Output::Artwork(None),&mut encoder);
            if let Err(error)=&result {assert!(matches!(error,GpuRasterError::NotReady),"{error:?}");}
            peak=peak.max(scene.object_results.bytes());
            assert!(scene.object_results.bytes()<=r.scene_tiers.display.limit(),"resident outputs stay within the display tier");
            r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
            if result.is_ok() {break;}
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let pixels=float_pixels(&r,&target);let mut expected=[0f64;4];
        for index in (0..80).rev() {
            let alpha=f64::from(colors[index%3][3])/255.;
            for axis in 0..3 {
                let value=f64::from(colors[index%3][axis])/255.;
                let value=if blend==layer_core::BlendSpace::Linear {layer_core::color::RgbSpace::Srgb.decode(value)} else {value};
                expected[axis]=value*alpha+expected[axis]*(1.-alpha);
            }
            expected[3]=alpha+expected[3]*(1.-alpha);
        }
        if blend==layer_core::BlendSpace::Perceptual {for axis in 0..3 {expected[axis]=layer_core::color::RgbSpace::Srgb.decode(expected[axis]/expected[3])*expected[3];}}
        for (actual,expected) in pixels[128*256+128].iter().zip(expected) {assert!((f64::from(*actual)-expected).abs()<0.00002,"{actual} != {expected}");}
        assert!(!scene.object_results.pending());
        eprintln!("overlapping siblings {blend:?} peak={peak} elapsed={:?}",start.elapsed());
    }
}

#[test]
fn unfinished_collection_cancels_changed_content_and_display_visibility_but_keeps_raw_queries() {
    let mut object=ImageObject::new(image([256;2],[171,37,83,190]));object.interpolation=ImageInterpolation::Nearest;
    let (mut doc,owner,handles)=document([256;2],vec![object.clone(),object]);
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();r.document_extent=[256;2];
    let mut scene=Scene::new(&r).unwrap();scene.object_display=true;
    let content=scene.object_spatial.content(doc.scene(),owner).unwrap();
    let request=scene.collection_job(&r,packet(doc.scene(),[256;2]),owner,content,ObjectWindow {origin:[0.;2],side:1.,size:[256;2]});
    let mut cache=super::super::object_cache::ObjectCache::for_scene(&scene,false);cache.resolve_collection(&r,doc.scene(),&request,None).unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());cache.advance(&mut r,&mut scene,&mut encoder,&mut Scene::begin_work(true)).unwrap();
    r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
    doc.artwork.occurrences.get_mut(owner).unwrap().visible=false;
    cache.retain(doc.scene(),request.blend,request.context);assert!(!cache.pending(),"hidden display owner cancels unfinished work");
    let encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());cache.flush_retired(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
    let mut raw=request.clone();raw.display=false;
    cache.resolve_collection(&r,doc.scene(),&raw,None).unwrap();cache.retain(doc.scene(),raw.blend,raw.context);assert!(cache.pending(),"raw objects include hidden owners");
    let sibling=doc.scene().object_owner(handles[1]).unwrap();
    let mut first=doc.scene().occurrence(owner).unwrap().clone();let mut second=doc.scene().occurrence(sibling).unwrap().clone();
    std::mem::swap(&mut first.content,&mut second.content);
    let undo=doc.apply(layer_core::Edit::Batch(vec![
        layer_core::Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,owner,Some(first)).unwrap()),
        layer_core::Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,sibling,Some(second)).unwrap()),
    ])).unwrap();
    cache.retain(doc.scene(),raw.blend,raw.context);assert!(!cache.pending(),"replacing the image cancels its captured source");
    doc.apply(undo).unwrap();
    let content=scene.object_spatial.content(doc.scene(),owner).unwrap();
    raw=scene.collection_job(&r,packet(doc.scene(),[256;2]),owner,content,ObjectWindow {origin:[0.;2],side:1.,size:[256;2]});raw.display=false;
    cache.resolve_collection(&r,doc.scene(),&raw,None).unwrap();
    doc.artwork.objects.get_mut(handles[0]).unwrap().affine.0[4]=0.5;
    cache.retain(doc.scene(),raw.blend,raw.context);assert!(!cache.pending(),"pose changes cancel a captured source mapping");
    let content=scene.object_spatial.content(doc.scene(),owner).unwrap();
    raw=scene.collection_job(&r,packet(doc.scene(),[256;2]),owner,content,ObjectWindow {origin:[0.;2],side:1.,size:[256;2]});raw.display=false;
    cache.resolve_collection(&r,doc.scene(),&raw,None).unwrap();
    cache.retain(doc.scene(),layer_core::BlendSpace::Perceptual,raw.context);assert!(!cache.pending(),"blend changes retire the previous color domain");
    cache.resolve_collection(&r,doc.scene(),&raw,None).unwrap();
    cache.retain(doc.scene(),raw.blend,layer_core::color::RgbSpace::DisplayP3);assert!(!cache.pending(),"working RGB changes retire the previous interpretation");
}

#[test]
fn collection_nearest_preserves_f64_source_floor_at_affine_boundaries() {
    let source=AuthoredImage::new(rgba8_source([256;2],|x,y|[if x%2==0 {211} else {29},if y%2==0 {37} else {163},83,255]));
    let mut object=ImageObject::new(source);object.interpolation=ImageInterpolation::Nearest;
    object.affine=Affine64([0.55,0.,0.,0.55,80.,13.]);
    let inverse=object.affine.inverse().unwrap();
    let (doc,owner,_)=document([256;2],vec![object]);
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let pixels=native(&mut r,&doc,Output::Objects(owner));
    for y in 0..256 {for x in 0..256 {
        let source=inverse.map([x as f64+0.5,y as f64+0.5]).map(|v|v.floor() as i64);
        let expected=if source.iter().all(|v|(0..256).contains(v)) {
            let color=[if source[0]%2==0 {211.} else {29.},if source[1]%2==0 {37.} else {163.},83.];
            [layer_core::color::RgbSpace::Srgb.decode(color[0]/255.) as f32,layer_core::color::RgbSpace::Srgb.decode(color[1]/255.) as f32,layer_core::color::RgbSpace::Srgb.decode(color[2]/255.) as f32,1.]
        } else {[0.;4]};
        for (actual,expected) in pixels[y*256+x].iter().zip(expected) {assert!((*actual-expected).abs()<0.00002,"pixel {x},{y}: {actual} != {expected}");}
    }}
}

#[test]
fn deferred_collection_construction_discards_unencoded_prefix_and_reuses_scratch() {
    let mut object=ImageObject::new(image([256;2],[171,37,83,190]));object.interpolation=ImageInterpolation::Nearest;
    let (doc,owner,_)=document([256;2],vec![object]);
    let r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut scene=Scene::new(&r).unwrap();scene.object_query=true;
    for _ in 0..12 {
        scene.alloc(&r,wgpu::Color::TRANSPARENT).unwrap();
        let before=crate::test_support::PassCounts::snapshot();
        assert!(matches!(scene.object_tile(&r,packet(doc.scene(),[256;2]),owner,[0;2],&mut Default::default()),Err(GpuRasterError::NotReady)));
        assert_eq!(crate::test_support::PassCounts::snapshot().delta(before).total,0,"deferred construction must not encode the prefix");
        assert!(matches!(scene.jobs.as_slice(),[Job::Clear(target,_)] if *target==scene.pool[0].view),"deferred construction must leave the prefix unwritten");
        scene.discard_unencoded_jobs();
        assert!(scene.jobs.is_empty());assert!(scene.source_jobs.is_empty());assert!(scene.used.iter().all(|used|!used.occupied()));
        assert_eq!(scene.pool.len(),1,"abandoned composition prefix remains reusable");
        assert!(scene.object_results.pending(),"private collection progress remains scheduled");
    }
}

fn tablet_scene(distinct: bool, count: usize, blur: bool) -> (layer_core::Document, OccurrenceHandle, Vec<ImageObjectHandle>, layer_render::ViewState) {
    let extent = [4248, 2832];
    let photo = AuthoredImage::new(rgba8_source(extent, |x, y| [(x * 7 % 256) as u8, (y * 5 % 256) as u8, ((x ^ y) % 256) as u8, 255]));
    let objects = (0..count).map(|index| {
        let phase = index as f64 / count as f64 * std::f64::consts::TAU;
        let source = if distinct { AuthoredImage::new(rgba8_source(extent, move |x, y| [((x + index as u32 * 31) % 256) as u8, (y % 256) as u8, 90, 255])) } else { photo.clone() };
        let mut object = ImageObject::new(source);
        object.affine = Affine64([0.55, 0., 0., 0.55, f64::from(extent[0]) * (0.225 + 0.08 * phase.cos()), f64::from(extent[1]) * (0.225 + 0.08 * phase.sin())]);
        object
    }).collect();
    let (mut doc, owner, handles) = document(extent, objects);
    for &handle in &handles { let owner=doc.scene().object_owner(handle).unwrap();doc.artwork.occurrences.get_mut(owner).unwrap().opacity=0.35; }
    let (_, target) = crate::test_support::add_paint(&mut doc.artwork, "Photo", extent);
    let SourceTarget::Paint(target) = target else { unreachable!() };
    doc.artwork.paint.get_mut(target).unwrap().base = Some(PaintBase { image: photo, offset: [0; 2], policy: PaintBasePolicy::SourceProfile });
    let stack = doc.composition().result;
    let mut entries = doc.scene().children(None).to_vec();
    if blur {
        let instance = layer_core::EffectInstance::new(crate::tests::fixture("gaussian_blur").program());
        let mut application = EffectApplication::new(instance.program, instance.values, extent);
        let sigma = application.program.parameters.iter().position(|parameter| parameter.key.as_ref() == "sigma").unwrap();
        application.values[sigma] = layer_core::EffectValue::Number(8.);
        let application = doc.artwork.effects.insert(PortableId::random(), application).unwrap();
        let mut effect = Occurrence::new(OccurrenceContent::Effect(application), "Blur"); effect.attachment = layer_core::Attachment::Effect;
        entries.insert(0, doc.artwork.occurrences.insert(PortableId::random(), effect).unwrap());
    }
    doc.artwork.stacks.get_mut(stack).unwrap().entries = entries;
    let doc = layer_core::Document::from_artwork(doc.artwork).unwrap();
    let zoom = (1920. / 4248f32).min(1200. / 2832.);
    let view = layer_render::ViewState { width_px: 1920, height_px: 1200,
        document_to_surface: [zoom, 0., 0., zoom, (1920. - 4248. * zoom) / 2., (1200. - 2832. * zoom) / 2.] };
    (doc, owner, handles, view)
}

fn stacked_windows_scene(count:u32)->(layer_core::Document,layer_render::ViewState) {
    let extent=[2048,1024];
    let objects=(0..count).map(|index| {
        let mut object=ImageObject::new(AuthoredImage::new(rgba8_source([512,256],move|x,y|[((x+index*31)%256) as u8,((y*3+index*17)%256) as u8,(40+index*15) as u8,255])));
        object.affine=Affine64([4.,0.,0.,4.,0.,0.]);object
    }).collect();
    let (mut doc,_,handles)=document(extent,objects);
    for &handle in &handles {let owner=doc.scene().object_owner(handle).unwrap();doc.artwork.occurrences.get_mut(owner).unwrap().opacity=0.35;}
    let doc=layer_core::Document::from_artwork(doc.artwork).unwrap();
    (doc,layer_render::ViewState {width_px:1024,height_px:512,document_to_surface:[0.5,0.,0.,0.5,0.,0.]})
}
#[test]
fn stacked_canonical_windows_beyond_the_cache_budget_are_each_computed_once() {
    let (doc,view)=stacked_windows_scene(12);
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let frame=FramePacket {view,..packet(doc.scene(),[2048,1024])};
    let budget=crate::memory::OBJECT_CACHE_BYTES/2;
    assert!(96*u64::from(PAGE_SIZE).pow(2)*16>budget,"the visible windows exceed the canonical cache budget");
    let mut tasks=std::collections::HashMap::<(OccurrenceHandle,[u64;2],u64,[u32;2]),Vec<std::sync::Weak<std::sync::atomic::AtomicBool>>>::new();
    crate::test_support::submit_ready(&mut r,frame).unwrap();
    let mut settled=false;
    for _ in 0..4000 {
        r.wait_idle().unwrap();
        for (valid,owner,window) in r.scene.as_ref().unwrap().object_results.completed_windows() {
            let seen=tasks.entry((owner,window.origin.map(f64::to_bits),window.side.to_bits(),window.size)).or_default();
            if !seen.iter().any(|known|known.ptr_eq(&valid)) {seen.push(valid);}
        }
        if r.next_frame()!=layer_render::FrameDemand::Work {settled=true;break;}
        crate::test_support::submit_ready(&mut r,FramePacket {composite_all:false,reset_layers:false,..frame}).unwrap();
    }
    let repeated=tasks.iter().filter(|(_,computed)|computed.len()>1).map(|((_,_,side,_),_)|f64::from_bits(*side)).collect::<Vec<_>>();
    assert!(repeated.is_empty(),"{} of {} completed canonical windows were computed more than once, at sides {repeated:?}",repeated.len(),tasks.len());
    assert!(settled,"the stacked scene settles");
}
#[test]
fn idle_refinement_of_settled_fit_photos_creates_no_level_zero_windows() {
    let (doc,_,_,mut view)=tablet_scene(true,8,false);
    view.document_to_surface=[0.1597,0.,0.,0.1597,670.09,445.26];
    let mut r=crate::memory::memory_tests::renderer_for_restore(&doc);
    let frame=FramePacket {view,..packet(doc.scene(),[4248,2832])};
    crate::memory::memory_tests::declare_source_budget(&mut r,frame,400,64<<20,380<<20);
    let mut level_zero=0;
    let mut settled=false;
    crate::test_support::submit_ready(&mut r,frame).unwrap();
    for _ in 0..4000 {
        r.wait_idle().unwrap();
        level_zero+=r.scene.as_ref().unwrap().object_results.request_windows().iter().filter(|(_,_,window)|window.side==1.).count();
        if r.next_frame()!=layer_render::FrameDemand::Work {settled=true;break;}
        crate::test_support::submit_ready(&mut r,FramePacket {composite_all:false,reset_layers:false,..frame}).unwrap();
    }
    assert!(settled,"the Fit photos settle");
    assert!(r.scale_display.as_ref().is_some_and(|cache|cache.plan.level>0),"Fit shows the photos above level 0");
    for _ in 0..8 {
        crate::test_support::submit_ready(&mut r,FramePacket {composite_all:false,reset_layers:false,..frame}).unwrap();r.wait_idle().unwrap();
        level_zero+=r.scene.as_ref().unwrap().object_results.request_windows().iter().filter(|(_,_,window)|window.side==1.).count();
    }
    assert_eq!(level_zero,0,"settling and idling at Fit requested level-0 canonical windows below the view level");
}
#[test]
fn object_work_waiting_on_a_held_read_needs_no_frames_until_its_decode_wakes_the_host() {
    let extent=[512,256];
    let held=rgba8_source(extent,|x,y|[(x%251) as u8,(y%241) as u8,90,255]);
    let mut inventory=(*held).clone();inventory.tiles.retain(|coordinate,_|*coordinate==[1,0]);
    let store=crate::test_support::GatedStore::new(&layer_core::raster_storage::RetainedTiles::from_sources([&inventory]),false);
    let mut waiting=ImageObject::new(AuthoredImage::new(held));waiting.affine=Affine64([0.5,0.,0.,0.5,256.,0.]);
    let (doc,_,_)=document(extent,vec![ImageObject::new(image([256;2],[30,140,200,255])),waiting]);
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let wakes=wake_channel(&mut r);
    let frame=packet(doc.scene(),extent);
    let idle=FramePacket {composite_all:false,reset_layers:false,..frame};
    crate::test_support::submit_ready(&mut r,frame).unwrap();
    for _ in 0..200 {
        r.wait_idle().unwrap();
        if r.next_frame()!=layer_render::FrameDemand::Work {break;}
        crate::test_support::submit_ready(&mut r,idle).unwrap();
    }
    assert!(r.scene.as_ref().unwrap().object_results.pending(),"the held tile keeps a canonical window incomplete");
    assert!(r.next_frame()!=layer_render::FrameDemand::Work,"object work waiting on a held read needs no frames");
    store.gate.release();
    let deadline=std::time::Instant::now()+crate::READBACK_TIMEOUT;
    while r.next_frame()!=layer_render::FrameDemand::Work {
        wakes.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())).expect("the landed decode wakes the host");
    }
    assert!(settle_on_wakes(&mut r,frame,&wakes,2000,|_|{}).is_some(),"the woken object work settles");
    assert!(!r.scene.as_ref().unwrap().object_results.pending());
}
fn settle_live_frames(r: &mut WgpuRasterizer, frame: FramePacket<'_>, limit: usize) -> usize {
    let wakes = wake_channel(r);
    crate::test_support::submit_ready(r, frame).unwrap();
    if let Some(submitted) = settle_on_wakes(r, frame, &wakes, limit, |r| if let Some(cache)=&r.scale_display {cache.assert_pending_capacity(r);}) { return submitted; }
    panic!("object scene must settle within {limit} submissions: deferred={}, image_waiting={}, moving={}, analysis={}, results_pending={}, results_bytes={}, completed_images={}, passes={}",
        crate::scene::scale::tests::publication_pending(&r),r.image_decode_waiting(),r.moving_images_pending(),r.analysis_dirty,
        r.scene.as_ref().is_some_and(|scene|scene.object_results.pending()),r.scene.as_ref().map_or(0,|scene|scene.object_results.bytes()),
        r.moving_images.completed_images,r.metrics.command_passes);
}

#[test]
fn batch_motion_of_twenty_sibling_images_pauses_queued_canonical_work_and_resumes_it() {
    let images=(0..4).map(|index|AuthoredImage::new(rgba8_source([64;2],|x,y|[(x*7%256) as u8,(y*5%256) as u8,40+index*45,160]))).collect::<Vec<_>>();
    let objects=(0..20).map(|index| {
        let mut object=ImageObject::new(images[index%4].clone());object.interpolation=ImageInterpolation::Nearest;
        object.affine=Affine64([1.,0.,0.,1.,(index%5) as f64*4.+0.25,(index/5) as f64*4.+0.125]);object
    }).collect();
    let (mut doc,owner,handles)=document([128;2],objects);
    let view=layer_render::ViewState {width_px:32,height_px:32,document_to_surface:[0.25,0.,0.,0.25,0.,0.]};
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    settle_live_frames(&mut r,FramePacket {view,..packet(doc.scene(),[128;2])},2000);
    let mut scene=r.scene.take().unwrap();
    let request=canonical_request(&r,&mut scene,&doc,owner,ObjectWindow {origin:[3.,5.],side:4.,size:[32;2]});
    assert!(scene.object_results.resolve_collection(&r,doc.scene(),&request,None).unwrap().is_none());
    assert!(scene.object_results.pending());r.scene=Some(scene);
    layer_render::CanvasRenderer::prepare_moving_layer(&mut r,Some(owner));
    settle_live_frames(&mut r,FramePacket {view,reset_layers:false,composite_all:true,..packet(doc.scene(),[128;2])},2000);
    assert!(r.scene.as_ref().unwrap().object_results.pending(),"accepted canonical work stays queued during motion");
    let paused=r.scene.as_ref().unwrap().object_results.work_progress();
    for _ in 0..3 {
        crate::test_support::submit_ready(&mut r, FramePacket {view,reset_layers:false,composite_all:false,..packet(doc.scene(),[128;2])}).unwrap();r.wait_idle().unwrap();
        assert!(r.next_frame()!=layer_render::FrameDemand::Work);assert_eq!(r.scene.as_ref().unwrap().object_results.work_progress(),paused);
    }
    let original=handles.iter().map(|h|doc.scene().object(*h).unwrap().affine).collect::<Vec<_>>();
    for delta in [4.,8.,-4.] {
        let changes=handles.iter().copied().zip(&original).map(|(h,original)| {
            let mut affine=*original;affine.0[4]+=delta;affine.0[5]+=delta;(h,affine)
        }).collect::<Vec<_>>();
        doc.apply(doc.set_image_object_affines_edit(&changes).unwrap()).unwrap();
        settle_live_frames(&mut r,FramePacket {view,reset_layers:false,composite_all:true,..packet(doc.scene(),[128;2])},2000);
        let mut reference=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        layer_render::CanvasRenderer::prepare_moving_layer(&mut reference,Some(owner));
        settle_live_frames(&mut reference,FramePacket {view,..packet(doc.scene(),[128;2])},2000);
        let actual=float_pixels(&r,r.scale_display.as_ref().unwrap().texture());
        let expected=float_pixels(&reference,reference.scale_display.as_ref().unwrap().texture());
        assert!(crate::test_support::max_error(&actual,&expected)<2e-5,"every moved sibling matches a fresh pose at {delta}");
        assert!(!r.scene.as_ref().unwrap().object_results.pending(),"new canonical jobs stay paused while previews are available");
    }
    layer_render::CanvasRenderer::prepare_moving_layer(&mut r,None);
    settle_live_frames(&mut r,FramePacket {view,reset_layers:false,composite_all:true,..packet(doc.scene(),[128;2])},2000);
    assert!(!r.scene.as_ref().unwrap().object_results.pending());
    let mut reference=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    settle_live_frames(&mut reference,FramePacket {view,..packet(doc.scene(),[128;2])},2000);
    assert!(crate::test_support::max_error(&float_pixels(&r,r.scale_display.as_ref().unwrap().texture()),&float_pixels(&reference,reference.scale_display.as_ref().unwrap().texture()))<2e-5);
}

#[test]
fn moving_blurred_object_layer_publishes_every_pose_without_queueing_canonical_work() {
    for distinct in [false, true] {
        let (mut doc, owner, handles, view) = tablet_scene(distinct, 4, true);
        let mut r = crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
        let frame = FramePacket { view, ..packet(doc.scene(), [4248, 2832]) };
        crate::memory::memory_tests::legacy_scene_frame(&mut r, frame);
        settle_live_frames(&mut r, frame, 2000);
        assert_eq!(r.scale_display.as_ref().unwrap().cached_branches(), 5, "presentation copies leave the composition its budget for five branches (distinct={distinct})");
        layer_render::CanvasRenderer::prepare_moving_layer(&mut r, Some(owner));
        let original = doc.scene().object(handles[0]).unwrap().affine;
        for step in 0..24 {
            let mut affine = original; affine.0[4] += 200. * (f64::from(step) * 0.1).sin(); affine.0[5] += 150. * (f64::from(step) * 0.1).cos();
            doc.apply(doc.set_image_object_affine_edit(handles[0], affine).unwrap()).unwrap();
            let (revision, composited) = (r.composite_revision, r.metrics.composited_pixels);
            let frame = FramePacket { view, composite_all: true, time_seconds: f32::from(step as u8) / 60., ..packet(doc.scene(), [4248, 2832]) };
            crate::memory::memory_tests::legacy_scene_frame(&mut r, frame);
            crate::test_support::submit_ready(&mut r, frame).unwrap();
            r.wait_idle().unwrap();
            assert!(r.composite_revision != revision && !crate::scene::scale::tests::publication_pending(&r), "pose {step} publishes a fresh preview (distinct={distinct})");
            let full = r.scale_display.as_ref().unwrap().plan.size.into_iter().map(u64::from).product::<u64>();
            assert!(step == 0 || (r.metrics.composited_pixels - composited) * 2 < full, "an advancing frame clock without animated effects keeps bounded object damage");
            assert!(!r.scene.as_ref().unwrap().object_results.pending(), "a moving layer never starts canonical work for a superseded pose");
        }
        layer_render::CanvasRenderer::prepare_moving_layer(&mut r, None);
        let frame = FramePacket { view, ..packet(doc.scene(), [4248, 2832]) };
        crate::memory::memory_tests::legacy_scene_frame(&mut r, frame);
        settle_live_frames(&mut r, frame, 2000);
        let display = r.scale_display.as_ref().unwrap();
        assert!(display.plan.level > 0);
        let bytes = r.scene.as_ref().unwrap().object_results.bytes();
        assert!(bytes <= 64 * 1024 * 1024 + resident_object_bytes([4248,2832],4,1), "settled canonical scratch and four independently paged output planes stay bounded: {bytes}");
    }
}

#[test]
fn cold_tablet_object_previews_and_canonical_results_settle_in_bounded_submissions() {
    let (doc, _, handles, mut view) = tablet_scene(false, 4, false);
    let zoom = 0.1597f32;
    view.document_to_surface = [zoom, 0., 0., zoom, (1920. - 4248. * zoom) / 2., (1200. - 2832. * zoom) / 2.];
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let before=crate::test_support::PassCounts::snapshot();
    settle_live_frames(&mut r, FramePacket { view, ..packet(doc.scene(), [4248, 2832]) }, 4000);
    let counts=crate::test_support::PassCounts::snapshot().delta(before);
    let source_pages=doc.scene().object(handles[0]).unwrap().image.extent.into_iter().map(|side|u64::from(side.div_ceil(PAGE_SIZE))).product::<u64>();
    let non_normalization=counts.total-counts.normalizations;
    assert!(non_normalization<4000,"exact canonical tent windows keep their pass budget: non-normalization={non_normalization}, {counts:?}");
    assert!(counts.normalizations<=704+source_pages,"source normalization is bounded by the parent work plus {source_pages} display pages: {counts:?}");
    assert!(counts.material_batches<=7,"cold source normalization must preserve the parent placement batches: {counts:?}");
    let (doc, _, _, view) = tablet_scene(true, 4, false);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    settle_live_frames(&mut r, FramePacket { view, ..packet(doc.scene(), [4248, 2832]) }, 4000);
    assert_eq!(r.moving_images.completed_images, 4);
}

#[test]
fn moving_collection_preview_composes_ordered_children_across_source_batches() {
    let colors: Vec<[u8; 4]> = (0..10u8).map(|index| [20 + index * 23, 200 - index * 17, 60 + index * 9, 40 + index * 7]).collect();
    let images: Vec<_> = colors.iter().map(|color| image([64; 2], *color)).collect();
    let objects = (0..20).map(|index| {
        let mut object = ImageObject::new(images[index % 10].clone());
        object.affine = Affine64([1., 0., 0., 1., f64::from((index % 5) as u32 * 4), f64::from((index / 5) as u32 * 4)]);
        object
    }).collect();
    let (doc, owner, _) = document([128; 2], objects);
    for blend in [layer_core::BlendSpace::Linear, layer_core::BlendSpace::Perceptual] {
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        layer_render::CanvasRenderer::prepare_moving_layer(&mut r, Some(owner));
        let mut frame = FramePacket { blend_space: blend, ..packet(doc.scene(), [128; 2]) };
        frame.view.width_px = 32; frame.view.height_px = 32; frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        settle_live_frames(&mut r, frame, 2000);
        assert!(!r.scene.as_ref().unwrap().object_results.pending(), "a moving layer composes previews without canonical work");
        let display = r.scale_display.as_ref().unwrap();
        assert_eq!(display.plan.level, 2);
        let pixels = float_pixels(&r, display.texture());
        let mut expected = [0f64; 4];
        for index in (0..20).rev() {
            let color = colors[index % 10];
            let alpha = f64::from(color[3]) / 255.;
            for axis in 0..3 {
                let value = f64::from(color[axis]) / 255.;
                let value = if blend == layer_core::BlendSpace::Linear { layer_core::color::RgbSpace::Srgb.decode(value) } else { value };
                expected[axis] = value * alpha + expected[axis] * (1. - alpha);
            }
            expected[3] = alpha + expected[3] * (1. - alpha);
        }
        let covered = pixels[(10 * display.plan.size[0] + 10) as usize];
        for (actual, expected) in covered.iter().zip(expected) { assert!((f64::from(*actual) - expected).abs() < 2e-3, "{blend:?}: {actual} != {expected}"); }
        assert_eq!(pixels[(30 * display.plan.size[0] + 30) as usize], [0.; 4]);
    }
}

#[test]
fn moving_nearest_objects_publish_level_zero_poses_with_and_without_effects() {
    let mut builder = SourceBuilder::new([64; 2], SourceInterpretation { channels: SourceChannels::Rgba,
        depth: SampleDepth::U8, profile: Default::default(), profile_assumed: false }, 1024 * 1024).unwrap();
    for y in 0..64 { builder.push_row(&(0..64).flat_map(|x| { let value = if (x + y) % 2 == 0 { 255 } else { 0 }; [value, value, value, 255] }).collect::<Vec<_>>()).unwrap(); }
    let checker = AuthoredImage::new(Arc::new(builder.finish().unwrap()));
    for blur in [false, true] {
        let mut object = ImageObject::new(checker.clone());
        object.interpolation = ImageInterpolation::Nearest;
        object.affine = Affine64([0.75, 0., 0., 0.75, 64., 64.]);
        let (mut doc, owner, handles) = document([256; 2], vec![object]);
        if blur {
            let instance = layer_core::EffectInstance::new(crate::tests::fixture("gaussian_blur").program());
            let application = doc.artwork.effects.insert(PortableId::random(), EffectApplication::new(instance.program, instance.values, [256; 2])).unwrap();
            let mut effect = Occurrence::new(OccurrenceContent::Effect(application), "Blur"); effect.attachment = layer_core::Attachment::Effect;
            let effect = doc.artwork.occurrences.insert(PortableId::random(), effect).unwrap();
            let stack = doc.composition().result;
            doc.artwork.stacks.get_mut(stack).unwrap().entries.insert(0, effect);
            doc = layer_core::Document::from_artwork(doc.artwork).unwrap();
        }
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let view = layer_render::ViewState { width_px: 64, height_px: 64, document_to_surface: [0.25, 0., 0., 0.25, 0., 0.] };
        settle_live_frames(&mut r, FramePacket { view, ..packet(doc.scene(), [256; 2]) }, 2000);
        layer_render::CanvasRenderer::prepare_moving_layer(&mut r, Some(owner));
        for step in 0..12 {
            let mut affine = doc.scene().object(handles[0]).unwrap().affine; affine.0[4] += 1.37; affine.0[5] -= 0.61;
            doc.apply(doc.set_image_object_affine_edit(handles[0], affine).unwrap()).unwrap();
            let revision = r.composite_revision;
            crate::test_support::submit_ready(&mut r, FramePacket { view, composite_all: false, ..packet(doc.scene(), [256; 2]) }).unwrap();
            r.wait_idle().unwrap();
            assert!(r.composite_revision != revision && !crate::scene::scale::tests::publication_pending(&r), "Nearest pose {step} publishes (blur={blur})");
            assert!(!r.scene.as_ref().unwrap().object_results.pending(), "Nearest motion queues no canonical work");
            if !blur {
                let pixels = float_pixels(&r, r.scale_display.as_ref().unwrap().texture());
                assert!(pixels.iter().all(|pixel| [[0., 0., 0., 1.], [1.; 4], [0.; 4]].contains(pixel)), "Nearest previews never average source texels");
                assert!(pixels.contains(&[1.; 4]) && pixels.contains(&[0., 0., 0., 1.]));
            }
        }
    }
}

#[test]
fn overlapping_canonical_windows_sample_only_their_uncovered_content() {
    let (doc, owner, _, _) = tablet_scene(false, 4, false);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.document_extent = [4248, 2832];
    let mut scene = Scene::new(&r).unwrap();
    scene.object_spatial.prepare(doc.scene());
    let content = scene.object_spatial.content(doc.scene(), owner).unwrap();
    let evaluate = |r: &mut WgpuRasterizer, scene: &mut Scene, origin: [f64; 2]| {
        let frame = packet(doc.scene(), [4248, 2832]);
        let mut request = scene.collection_job(r, frame, owner, content.clone(), ObjectWindow { origin, side: 4., size: [256; 2] });
        request.live = true; request.display = true;
        let mut taps = 0;
        let accepted=&mut sources::AcceptedInputs::default();
        for _ in 0..2000 {
            let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
            r.drain_image_decodes(&mut encoder).unwrap();
            if let Some(pieces) = scene.canonical_cover(r, frame, &content, &request, accepted, Class::Visible).unwrap() {
                r.uploads.finish(&encoder); encoder.submit(&r.queue);
                return (taps, pieces.into_iter().map(|(_, bounds)| bounds).collect::<Vec<_>>());
            }
            let empty=super::super::object_cache::ObjectCache::for_scene(&scene,false);
            let mut cache=std::mem::replace(&mut scene.object_results,empty);
            taps += cache.advance(r, scene, &mut encoder,&mut Scene::begin_work(false)).unwrap();
            scene.object_results = cache;
            r.uploads.finish(&encoder); encoder.submit(&r.queue); r.wait_idle().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("canonical window completes");
    };
    let (first, pieces) = evaluate(&mut r, &mut scene, [600., 400.]);
    assert_eq!(pieces.len(), 1);
    let (shifted, pieces) = evaluate(&mut r, &mut scene, [1112., 400.]);
    assert!(pieces.len() == 2 && pieces.iter().all(|bounds| !bounds.intersect(DocRect { min: [1112, 400], max: [2136, 1424] }).is_empty()));
    let mut empty = Scene::new(&r).unwrap();
    let (fresh, _) = evaluate(&mut r, &mut empty, [1112., 400.]);
    assert!(shifted * 3 < fresh * 2, "half of the shifted window is reused: {shifted} of {fresh} taps (first {first})");
    let (covered, _) = evaluate(&mut r, &mut scene, [856., 400.]);
    assert_eq!(covered, 0, "a window inside completed results samples nothing");
}

#[test]
fn fit_strokes_update_a_held_presenter_after_an_exact_preview() {
    let extent=[1536;2];
    let (mut doc,_,_)=document(extent,vec![ImageObject::new(image(extent,[255,0,0,255]))]);
    let (paint,target)=crate::test_support::add_paint(&mut doc.artwork,"Ink",extent);
    let stack=doc.composition().result;
    let entries=&mut doc.artwork.stacks.get_mut(stack).unwrap().entries;
    entries.retain(|entry|*entry!=paint);entries.insert(0,paint);
    let doc=layer_core::Document::from_artwork(doc.artwork).unwrap();
    let mut r=crate::memory::memory_tests::renderer_for_restore(&doc);
    let view=layer_render::ViewState {width_px:384,height_px:384,document_to_surface:[0.25,0.,0.,0.25,0.,0.]};
    let frame=FramePacket {view,composite_all:false,..packet(doc.scene(),extent)};
    settle_live_frames(&mut r,frame,4000);
    let cache=r.scale_display.as_ref().unwrap();
    assert!(cache.evaluation==super::super::scale::Evaluation::Display);
    assert_eq!(cache.plan.bounds,PixelRect::full(extent));
    let mut presenter=crate::test_support::float_presenter(&r);presenter.set_target_retention(true);
    let texture=create_color_target(&r.device,[384;2],"held Fit stroke");
    presenter.present(&mut r,&texture.1,view,[0.;4]).unwrap();
    assert!(float_pixels(&r,&texture.0)[192*384+192][1]<0.01);
    let mut dab=crate::layer_tests::dab([1.,0.,1.,1.]);dab.radii=[32.;2];dab.contact=[1.,0.,0.,0.];
    let style=crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen);
    for (step,x) in [640,768,896].into_iter().enumerate() {
        dab.center=layer_core::Point {x:x as f32,y:768.};
        let mut batch=crate::test_support::dab_batch(target,style.clone(),dab.bounds());batch.stroke_start=step==0;batch.stroke_end=step==2;
        crate::test_support::submit_ready(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
        r.wait_idle().unwrap();presenter.present(&mut r,&texture.1,view,[0.;4]).unwrap();
        let pixel=float_pixels(&r,&texture.0)[192*384+x as usize/4];
        assert!(pixel[0]>0.99 && pixel[2]>0.99 && pixel[1]<0.01,"Fit frame {step} must repaint current ink in its held target: {pixel:?}");
    }
}

#[test]
fn ready_mip_pixels_present_while_canonical_results_are_rebuilt() {
    let (doc, owner, _, mut view) = tablet_scene(true, 8, false);
    view.document_to_surface = [0.1597, 0., 0., 0.1597, 670.09, 445.26];
    let mut r = crate::memory::memory_tests::renderer_for_restore(&doc);
    let frame = FramePacket { view, ..packet(doc.scene(), [4248, 2832]) };
    crate::memory::memory_tests::declare_source_budget(&mut r, frame, 400, 64 << 20, 380 << 20);
    settle_live_frames(&mut r, frame, 4000);
    assert!(r.has_presentable_frame());
    let mut scene = r.scene.take().unwrap();
    scene.object_results.clear();
    scene.invalidate_object_previews(frame.document_extent);
    let request = canonical_request(&r, &mut scene, &doc, owner,
        ObjectWindow { origin: [1280., 768.], side: 1., size: [PAGE_SIZE; 2] });
    assert!(!request.keys.is_empty());
    assert!(!scene.object_results.prepare_collection(&r, doc.scene(), &request,None).unwrap());
    r.scene = Some(scene);
    for _ in 0..2 {
        crate::test_support::submit_ready(&mut r, FramePacket { composite_all: false, ..frame }).unwrap();
        r.wait_idle().unwrap();
    }
    assert!(r.scene.as_ref().unwrap().object_results.pending(), "canonical rebuilding remains in flight");
    assert!(r.has_presentable_frame(), "ready mips must keep a frame available while exact results rebuild");
}

#[test]
fn eight_distinct_sources_settle_in_few_frames_with_a_tablet_sized_source_cache() {
    let (doc, _, _, mut view) = tablet_scene(true, 8, false);
    view.document_to_surface = [0.1597, 0., 0., 0.1597, 670.09, 445.26];
    let mut r = crate::memory::memory_tests::renderer_for_restore(&doc);
    let frame = FramePacket { view, ..packet(doc.scene(), [4248, 2832]) };
    crate::memory::memory_tests::declare_source_budget(&mut r, frame, 400, 64 << 20, 380 << 20);
    settle_live_frames(&mut r, frame, 4000);
}

#[test]
fn photo_navigation_presents_current_camera_within_two_submissions() {
    fn color(x: u32, y: u32, seed: u32) -> [u8; 4] {
        let color = match (x < 768, y < 768) {
            (true, true) => [220, 40, 20], (false, true) => [20, 200, 40],
            (true, false) => [20, 40, 220], (false, false) => [220, 200, 20],
        };
        let [r, g, b] = color.map(|value| (value + seed) as u8);
        [r, g, b, 255]
    }
    fn preview(r: &mut WgpuRasterizer, presenter: &mut crate::present::ViewportPresenter, target: &(wgpu::Texture, wgpu::TextureView), doc: &layer_core::Document, view: layer_render::ViewState, seed: u32) {
        for _ in 0..2 {
            let frame = FramePacket { view, composite_all: false,
                blend_space: layer_core::BlendSpace::Linear, ..packet(doc.scene(), [1536; 2]) };
            crate::memory::memory_tests::declare_source_budget(r, frame, 64, 16 << 20, 44 << 20);
            crate::test_support::submit_ready(r, frame).unwrap();
            r.wait_idle().unwrap();
            if !r.has_presentable_frame() || crate::scene::scale::tests::publication_pending(r)
                || !r.artwork_frame.as_ref().is_some_and(|frame| frame.view == view) { continue; }
            if r.scene.as_ref().is_some_and(|scene| scene.object_results.pending()) {
                assert!(crate::scene::scale::tests::publication_pending(&r) || r.selected_display_cache().unwrap().approximating(),
                    "pending canonical work must not label mip pixels exact");
            }
            if seed != 0 {
                assert!(presenter.needs_present(r, view, [0.; 4]), "new artwork must publish while canonical work remains pending");
            }
            presenter.present(r, &target.1, view, [0.; 4]).unwrap();
            let pixel = float_pixels(r, &target.0)[96 * view.width_px as usize + 96];
            let [a, _, _, d, tx, ty] = view.document_to_surface;
            let expected = color(((96.5 - tx) / a) as u32, ((96.5 - ty) / d) as u32, seed);
            for channel in 0..3 {
                let expected = r.document_color().space.decode(f64::from(expected[channel]) / 255.) as f32;
                assert!((pixel[channel] - expected).abs() < 0.0015,
                    "camera {:?}, channel {channel}: {} != {expected}", view.document_to_surface, pixel[channel]);
            }
            assert_eq!(pixel[3], 1.);
            return;
        }
        panic!("current artwork and camera must present within two submissions: {:?}", view.document_to_surface);
    }
    let objects = (0..28).map(|seed| ImageObject::new(AuthoredImage::new(
        rgba8_source([1536; 2], |x, y| color(x, y, seed))))).collect();
    let (mut doc, owner, _) = document([1536; 2], objects);
    let mut r = crate::memory::memory_tests::renderer_for_restore(&doc);
    assert_ne!(r.adapter.get_info().device_type, wgpu::DeviceType::Cpu);
    let fit = layer_render::ViewState { width_px: 384, height_px: 384,
        document_to_surface: [0.25, 0., 0., 0.25, 0., 0.] };
    layer_render::CanvasRenderer::prepare_moving_layer(&mut r, Some(owner));
    let frame = FramePacket { view: fit, blend_space: layer_core::BlendSpace::Linear,
        ..packet(doc.scene(), [1536; 2]) };
    crate::memory::memory_tests::declare_source_budget(&mut r, frame, 64, 16 << 20, 44 << 20);
    assert!(r.source_tiles.borrow().free_slots() >= 64);
    settle_live_frames(&mut r, frame, 4000);
    assert_eq!(r.moving_images.completed_images, 28);
    layer_render::CanvasRenderer::prepare_moving_layer(&mut r, None);
    let mut presenter = crate::test_support::float_presenter(&r);
    presenter.set_target_retention(true);
    let target = create_color_target(&r.device, [384; 2], "current camera preview");
    for exact in [false, true] {
        r.test.exact_display = exact;
        for document_to_surface in [fit.document_to_surface,
            [1., 0., 0., 1., -512., -384.], [1., 0., 0., 1., -768., -640.],
            [0.5, 0., 0., 0.5, -192., -128.], [0.5, 0., 0., 0.5, -448., -384.], fit.document_to_surface] {
                preview(&mut r, &mut presenter, &target, &doc, layer_render::ViewState { document_to_surface, ..fit }, 0);
        }
    }
    r.test.exact_display = false;
    let actual = layer_render::ViewState { document_to_surface: [1., 0., 0., 1., -512., -384.], ..fit };
    preview(&mut r, &mut presenter, &target, &doc, actual, 0);
    let revision = r.canvas_preview_revision();
    doc.apply(doc.delete_layers_edit(&[owner]).unwrap()).unwrap();
    preview(&mut r, &mut presenter, &target, &doc, actual, 1);
    assert_eq!(r.canvas_preview_revision(), revision.wrapping_add(1));
    crate::test_support::submit_ready(&mut r, FramePacket { view: actual, composite_all: false,
        blend_space: layer_core::BlendSpace::Linear, ..packet(doc.scene(), [1536; 2]) }).unwrap();
    assert_eq!(r.canvas_preview_revision(), revision.wrapping_add(1));
}

#[test]
fn a_placed_visible_object_settles_before_thumbnail_preparation() {
    use layer_engine::{CanvasEngine, ViewTransform, input_queue};
    let mut doc = layer_core::Document::new(PortableId::random(), 4096, 2048,
        layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
    doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().blend = layer_core::BlendSpace::Linear;
    for visible in [true, false] {
        let source = layer_core::color::source::rgba8_source([1024, 512], |x, _| if x < 512 { [230, 20, 20, 255] } else { [20, 20, 230, 255] });
        let mut object = ImageObject::new(source.into());
        object.affine = Affine64([2., 0., 0., 2., 1024., 512.]);
        let (layer, edit) = doc.create_object_layer_edit("Photo", object, None, 0).unwrap();
        doc.apply(edit).unwrap();
        doc.artwork.occurrences.get_mut(layer).unwrap().visible = visible;
    }
    let renderer = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let (_input, consumer) = input_queue(64);
    let view = layer_render::ViewState { width_px: 1024, height_px: 768, document_to_surface: layer_core::Affine::IDENTITY.0 };
    let mut engine = CanvasEngine::new(renderer, doc, consumer, view, ViewTransform::IDENTITY).unwrap();
    engine.render_frame_at(0).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut clock = 1;
    loop {
        engine.render_frame_at(clock).unwrap();
        engine.backend_mut().wait_idle().unwrap();
        let r = engine.backend();
        if r.next_frame()!=layer_render::FrameDemand::Work { break; }
        assert!(std::time::Instant::now() < deadline, "the placed visible layer settles");
        clock += 8_000_000;
    }
}

#[test]
fn oversized_nearest_sources_skip_optional_mips_and_keep_exact_capture() {
    let reference=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let required_limits=wgpu::Limits {max_texture_dimension_2d:reference.device().limits().max_texture_dimension_2d.min(8192),..reference.device().limits()};
    let (device,queue)=pollster::block_on(reference.adapter.request_device(&wgpu::DeviceDescriptor {
        label:Some("oversized nearest source"),required_features:reference.device().features(),required_limits,
        memory_hints:crate::memory_hints(),..Default::default()})).unwrap();
    let mut r=*WgpuRasterizer::from_wgpu_inner(reference.adapter.clone(),device.into(),queue,Initialization::Headless).unwrap();
    r.initialize_native(Default::default()).unwrap();
    let extent=[64;2];let source_extent=[r.device.limits().max_texture_dimension_2d+1,1];
    let mut object=ImageObject::new(image(source_extent,[255,0,0,255]));
    object.interpolation=ImageInterpolation::Nearest;
    object.affine=Affine64([f64::from(extent[0])/f64::from(source_extent[0]),0.,0.,64.,0.,0.]);
    let (doc,_,_)=document(extent,vec![object]);
    crate::test_support::submit_ready(&mut r,packet(doc.scene(),extent)).unwrap();
    r.wait_idle().unwrap();
    let mut reader=r.snapshot_gpu().capture_scene(doc.snapshot(),layer_core::SceneScope::All,Default::default()).unwrap();
    let pixels=reader.read_region([0,0,extent[0],extent[1]]).unwrap();
    assert!(pixels.iter().all(|pixel|*pixel==[1.,0.,0.,1.]),"optional display mips must not refuse canonical sampling");
}

#[test]
fn eight_ready_photo_mips_resettle_without_fragmented_canonical_work() {
    let (doc,_,_,mut view)=tablet_scene(true,8,false);
    view.document_to_surface=[0.1597,0.,0.,0.1597,670.09,445.26];
    let mut r=crate::memory::memory_tests::renderer_for_restore(&doc);
    let frame=FramePacket {view,..packet(doc.scene(),[4248,2832])};
    crate::memory::memory_tests::declare_source_budget(&mut r,frame,400,64<<20,380<<20);
    settle_live_frames(&mut r,frame,4000);
    assert!(!r.moving_images_pending());assert!(!r.scene.as_ref().unwrap().object_results.pending());
    let completed=r.moving_images.completed_images;let built=r.moving_images.tile_builds;
    r.scale_display=None;
    let scene=r.scene.as_mut().unwrap();scene.object_results.clear();
    let encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    scene.object_results.flush_retired(&encoder);r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
    let revision=r.composite_revision;
    settle_live_frames(&mut r,frame,4000);
    assert_eq!(r.moving_images.completed_images,completed);assert_eq!(r.moving_images.tile_builds,built);
    assert!(r.composite_revision!=revision);assert!(!crate::scene::scale::tests::publication_pending(&r));
    assert!(!r.scene.as_ref().unwrap().object_results.pending());
    assert!(r.scale_display.as_ref().is_some_and(|cache|!cache.approximating()));
}

#[test]
fn accepted_canonical_results_survive_pressure_until_consumed_and_released() {
    let extent=[256;2];let batch_bytes=32*16;
    let (doc,owner,_)=document(extent,vec![ImageObject::new(image(extent,[255,0,0,255]))]);
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    crate::memory::memory_tests::legacy_scene_frame(&mut r,packet(doc.scene(),extent));
    let mut scene=Scene::new(&r).unwrap();scene.object_display=true;scene.object_spatial.prepare(doc.scene());
    let mut cache=super::super::object_cache::ObjectCache::for_scene(&scene,true);
    let mut previous=Vec::<std::sync::Weak<std::sync::atomic::AtomicBool>>::new();
    let mut previous_requests=Vec::new();
    let mut uniform_bytes=0;
    for batch in 0..3 {
        let mut accepted=sources::AcceptedInputs::default();
        let requests=(batch*32..(batch+1)*32).map(|index| {
            let request=canonical_request(&r,&mut scene,&doc,owner,ObjectWindow {
                origin:[f64::from(index%16*16+8),f64::from(index/16*16+8)],side:1.,size:[1;2]});
            assert!(cache.resolve_collection(&r,doc.scene(),&request,Some(&mut accepted)).unwrap().is_none());request
        }).collect::<Vec<_>>();
        assert_eq!(accepted.len(),32);
        let interests=cache.request_windows().into_iter().skip(previous.len()).map(|(valid,_,_)|valid).collect::<Vec<_>>();
        assert_eq!(interests.len(),32);
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
        while cache.pending() {
            scene.begin_frame();
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            r.drain_image_decodes(&mut encoder).unwrap();cache.advance(&mut r,&mut scene,&mut encoder,&mut Scene::begin_work(false)).unwrap();
            cache.flush_retired(&encoder);
            r.uploads.finish(&encoder);r.last_submission=encoder.submit(&r.queue);r.wait_idle().unwrap();
            assert!(interests.iter().all(|interest|interest.upgrade().is_some()),"accepted canonical work cannot disappear before consumption");
            assert!(std::time::Instant::now()<deadline,"each accepted batch must make bounded progress");
            if cache.pending() {std::thread::sleep(std::time::Duration::from_millis(1));}
        }
        assert_eq!(cache.work_progress().0,32+previous.len());
        assert_eq!(cache.work_progress().1,32+previous.len());
        if batch==0 {
            uniform_bytes=cache.memory_bytes()[1].checked_sub(batch_bytes).unwrap();
            assert!(uniform_bytes>0);
            crate::memory::memory_tests::legacy_scene_frame(&mut r,packet(doc.scene(),extent));
        }
        let allowance=uniform_bytes+batch_bytes;
        assert_eq!(cache.pinned_gpu_bytes(),batch_bytes,"accepted completed results stay reported as pinned");
        for request in &previous_requests {assert!(cache.resolve_collection(&r,doc.scene(),request,None).unwrap().is_some());}
        let encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        cache.flush_retired(&encoder);r.uploads.finish(&encoder);r.last_submission=encoder.submit(&r.queue);r.wait_idle().unwrap();
        assert_eq!(cache.memory_bytes()[1],uniform_bytes+batch_bytes*(1+u64::from(!previous.is_empty())));
        assert_eq!(cache.evict_to(allowance),if previous.is_empty() {0}else {batch_bytes});
        assert_eq!(cache.work_progress().0,32,"pressure must evict only the previous unaccepted batch");
        assert_eq!(cache.work_progress().1,32,"no accepted completed result may be evicted before consumption");
        assert!(interests.iter().all(|interest|interest.upgrade().is_some()));
        let encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        cache.flush_retired(&encoder);r.uploads.finish(&encoder);r.last_submission=encoder.submit(&r.queue);r.wait_idle().unwrap();
        assert!(previous.iter().all(|interest|interest.upgrade().is_none()),"released results must be evictable for the next accepted batch");
        assert_eq!(cache.memory_bytes()[1],allowance);
        for request in &requests {
            let result=cache.resolve_collection(&r,doc.scene(),request,Some(&mut accepted)).unwrap()
                .expect("every accepted result must survive completion and pressure until its actual read");
            assert_eq!(float_pixels(&r,result.texture()),vec![[1.,0.,0.,1.]]);
        }
        assert_eq!(accepted.len(),32);
        drop(accepted);
        previous=interests;
        previous_requests=requests;
        let encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        cache.flush_retired(&encoder);r.uploads.finish(&encoder);r.last_submission=encoder.submit(&r.queue);r.wait_idle().unwrap();
    }
    assert_eq!(cache.evict_to(uniform_bytes),batch_bytes,"cancelling the last accepted interest releases its result capacity");
    let encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    cache.flush_retired(&encoder);r.uploads.finish(&encoder);r.last_submission=encoder.submit(&r.queue);r.wait_idle().unwrap();
    assert!(previous.iter().all(|interest|interest.upgrade().is_none()));
    assert_eq!(cache.work_progress().0,0);
    assert_eq!(cache.memory_bytes()[1],uniform_bytes);
}

#[test]
fn perceptual_object_completion_preserves_an_unready_capture_prefix() {
    let extent=[768,32];
    let source=layer_core::color::source::rgba8_source(extent,|_,_|[64,128,192,255]);
    let (mut doc,owner,_)=document(extent,vec![ImageObject::new(AuthoredImage::new(source.clone()))]);
    doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().blend=layer_core::BlendSpace::Perceptual;
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.document_extent=extent;
    let mut scene=Scene::new(&r).unwrap();scene.ordered=true;
    pollster::block_on(r.compilation.prepare_all(r.scene_pipelines.objects.pipelines())).unwrap();
    pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.constant.2])).unwrap();
    pollster::block_on(r.compilation.prepare_all([r.scene_pipelines.source.for_format(wgpu::TextureFormat::Rgba8Unorm)])).unwrap();
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    for coordinate in page_coordinates(PixelRect::full(extent)) {
        let prepared=crate::prepare_image_tile(&source,coordinate,r.device.working_space(),&r.device.source_samples,None).unwrap();
        Scene::decode_prepared_for(&mut r,&mut encoder,&source,coordinate,prepared,crate::scene::sources::SourceUse::Exact,crate::residency::Class::Visible).unwrap();
    }
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    let cold=layer_core::color::source::rgba8_source([PAGE_SIZE;2],|_,_|[17,31,63,255]);
    let gate=crate::test_support::GatedStore::new(&layer_core::raster_storage::RetainedTiles::from_sources([cold.as_ref()]),false);
    let (_,pending)=r.source_tiles.borrow_mut().plan_exact(&r,&cold,[0;2]).unwrap();
    let pending=pending.expect("the unrelated capture input must require decoding");
    assert!(!pending.ready());
    scene.jobs.push(Job::DecodedTile(pending.clone()));scene.source_jobs=vec![0..1];
    let ranges=scene.source_jobs.clone();
    let content=scene.object_spatial.content(doc.scene(),owner).unwrap();
    let frame=FramePacket {blend_space:layer_core::BlendSpace::Perceptual,..packet(doc.scene(),extent)};
    let request=scene.collection_job(&r,frame,owner,content,ObjectWindow {origin:[0.;2],side:1.,size:extent});
    assert_eq!(request.blend,layer_core::BlendSpace::Perceptual);assert_eq!(request.parts(),2);
    let mut cache=super::super::object_cache::ObjectCache::for_scene(&scene,true);
    assert!(cache.resolve_collection(&r,doc.scene(),&request,None).unwrap().is_none());
    for _ in 0..64 {
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        cache.advance(&mut r,&mut scene,&mut encoder,&mut Scene::begin_work(false)).expect("canonical completion must not encode another capture's cold prefix");
        assert_eq!(scene.jobs.len(),1);
        assert!(matches!(&scene.jobs[0],Job::DecodedTile(held) if Arc::ptr_eq(held,&pending)));
        assert_eq!(scene.source_jobs,ranges);assert!(!pending.ready());
        r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
        if !cache.pending() {break;}
    }
    assert!(!cache.pending(),"both Perceptual collection parts must complete independently of the cold prefix");
    let output=cache.resolve_collection(&r,doc.scene(),&request,None).unwrap().expect("the retained exact collection must be readable");
    let pixels=float_pixels(&r,output.texture());
    for x in [32,384,736] {
        assert!(crate::test_support::max_error(&[pixels[16*extent[0] as usize+x]],&[[64./255.,128./255.,192./255.,1.]])<2e-5,
            "Perceptual encoding and both part copies must preserve exact colors");
    }
    drop(gate);
}

#[test]
fn cold_linear_canonical_preparation_retains_its_requested_result() {
    let extent=[16;2];
    let (doc,owner,_)=document(extent,vec![ImageObject::new(image(extent,[230,40,20,255]))]);
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    assert!(!super::objects::inline_sampling(&r));
    r.ensure_document_metadata(extent,doc.scene()).unwrap();
    let mut scene=Scene::new(&r).unwrap();
    scene.ordered=true;
    let request=canonical_request(&r,&mut scene,&doc,owner,
        ObjectWindow {origin:[0.;2],side:1.,size:extent});
    assert!(scene.exact_object_results.resolve_collection(&r,doc.scene(),&request,None).unwrap().is_none());
    ordered_capture_before_deadline(&mut scene,&mut r);
    let output=scene.exact_object_results.resolve_collection(&r,doc.scene(),&request,None).unwrap()
        .expect("completed preparation must retain the requested exact canonical result");
    let pixels=float_pixels(&r,output.texture());
    let expected=[230.,40.,20.].map(|channel|layer_core::color::srgb_decode(channel/255.));
    assert_eq!(pixels.len(),256);
    for pixel in pixels {
        assert!((pixel[3]-1.).abs()<3e-5);
        for channel in 0..3 {assert!((pixel[channel]-expected[channel]).abs()<3e-5);}
    }
}

#[test]
fn prepared_upload_keeps_its_source_through_the_consuming_submission() {
    let source=rgba8_source([PAGE_SIZE;2],|_,_|[64,128,192,255]);
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    pollster::block_on(r.compilation.prepare_all(crate::startup::SourceKinds::for_source(&source).pipelines(&r.scene_pipelines.source))).unwrap();
    let prepared=crate::prepare_image_tile(&source,[0;2],r.document_color().space,&r.device.source_samples,None).unwrap();
    let mut warm=submission::CommandEncoder::new(&r.device,&Default::default());
    Scene::decode_prepared_for(&mut r,&mut warm,&source,[0;2],prepared.clone(),sources::SourceUse::Display,crate::residency::Class::Visible).unwrap();
    r.uploads.finish(&warm);warm.submit(&r.queue);crate::test_support::complete(&r);
    let mut consumer=submission::CommandEncoder::new(&r.device,&Default::default());
    let encoded=Scene::decode_prepared_for(&mut r,&mut consumer,&source,[0;2],prepared.clone(),sources::SourceUse::Display,crate::residency::Class::Visible).unwrap();
    assert_eq!(encoded.texture.format(),wgpu::TextureFormat::Rgba8Unorm);
    let copy=r.device.create_texture(&wgpu::TextureDescriptor {label:Some("accepted encoded upload read"),size:encoded.texture.size(),
        mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:encoded.texture.format(),
        usage:wgpu::TextureUsages::COPY_DST|wgpu::TextureUsages::COPY_SRC,view_formats:&[]});
    consumer.copy_texture_to_texture(encoded.texture.as_image_copy(),copy.as_image_copy(),copy.size());
    let scope=r.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut promotion=submission::CommandEncoder::new(&r.device,&Default::default());
    Scene::promote_source(&mut r,&mut promotion,encoded.clone()).unwrap();
    r.uploads.finish(&promotion);promotion.submit(&r.queue);crate::test_support::complete(&r);
    r.uploads.finish(&consumer);consumer.submit(&r.queue);crate::test_support::complete(&r);
    assert!(pollster::block_on(scope.pop()).is_none(),"promotion destroyed an accepted encoded upload before its consuming submission");
    assert_eq!(crate::layer_tests::page_bytes(&r,&copy),[64,128,192,255].repeat((PAGE_SIZE*PAGE_SIZE) as usize));
}
#[test]
fn object_admission_and_charges_are_unchanged_when_the_caches_move_out() {
    let (doc,_,_)=document([512;2],vec![ImageObject::new(image([300,200],[255,0,0,255]))]);
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    crate::test_support::submit_settled(&mut r,packet(doc.scene(),[512;2]));
    let _=native(&mut r,&doc,Output::Artwork(None));
    assert!(r.scene_tiers.objects.used()>0,"the moved caches must own GPU allocations");
    let mut scene=r.scene.take().unwrap();
    let charged=r.scene_tiers.display.used();
    r.scene_tiers.display.set_limit(charged+1,0);
    let admission=|scene:&Scene| {
        let accepted=r.admit_display(Some(scene),Class::Visible,1);
        (accepted.is_ok(),r.scene_tiers.display.used())
    };
    let before=admission(&scene);
    let empty=(super::super::object_cache::ObjectCache::for_scene(&scene,false),super::super::object_cache::ObjectCache::for_scene(&scene,false));
    let caches=(std::mem::replace(&mut scene.object_results,empty.0),std::mem::replace(&mut scene.exact_object_results,empty.1));
    let detached=admission(&scene);
    assert_eq!(before,detached,"moving charged caches must not alter admission");
    assert!(before.0);
    assert_eq!(r.scene_tiers.display.used(),charged);
    (scene.object_results,scene.exact_object_results)=caches;
    r.scene=Some(scene);
}
#[test]
fn object_counters_match_a_cache_walk_after_seeded_windows_edits_and_completions() {
    let shared = image([600, 400], [255, 0, 0, 255]);
    let mut objects = vec![ImageObject::new(shared.clone()), ImageObject::new(shared), ImageObject::new(image([500, 300], [0, 255, 0, 255]))];
    for (index, object) in objects.iter_mut().enumerate() {
        object.affine = Affine64([0.75, 0., 0., 0.75, 40. + 120. * index as f64, 30.]);
        if index == 1 { object.interpolation = ImageInterpolation::Nearest; }
    }
    let (mut doc, _, handles) = document([1024, 768], objects);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut seed = 0x2545_f491_u32;
    for step in 0..48 {
        seed ^= seed << 13; seed ^= seed >> 17; seed ^= seed << 5;
        if step % 8 == 7 {
            let handle = handles[seed as usize % handles.len()];
            doc.apply(doc.set_image_object_affine_edit(handle, Affine64([0.5 + f64::from(seed % 5) / 8., 0., 0., 0.5 + f64::from(seed % 5) / 8., f64::from(seed % 300), f64::from(seed % 200)])).unwrap()).unwrap();
        }
        let mut frame = packet(doc.scene(), [1024, 768]);
        frame.view.width_px = 512; frame.view.height_px = 384;
        let scale = 0.5 / f64::from(1 << (seed >> 9) % 2) as f32 as f64;
        frame.view.document_to_surface = [scale as f32, 0., 0., scale as f32, 0., 0.];
        crate::test_support::submit_ready(&mut r, FramePacket { composite_all: step == 0, reset_layers: step == 0, ..frame }).unwrap();
        if seed & 4 != 0 { r.wait_idle().unwrap(); }
        let scene = r.scene.as_ref().unwrap();
        let [counted, walked] = [scene.usage(), scene.walked_usage()];
        let [objects, walked_objects] = scene.object_gpu();
        assert_eq!([counted.working, counted.ram, counted.pinned, objects], [walked.working, walked.ram, walked.pinned, walked_objects], "step {step}");
    }
}

#[test]
fn an_admission_refused_cell_takes_no_idle_frames_until_a_release_then_wakes() {
    use layer_render::{CanvasRenderer,FrameDemand};
    let (doc, _, _) = document([1024; 2], vec![ImageObject::new(image([1024; 2], [255, 0, 0, 255]))]);
    let mut r = crate::memory::memory_tests::renderer(doc.composition().color,
        layer_core::DeviceMemoryFacts {unified: true, ..Default::default()}).unwrap();
    let (wake, woken) = std::sync::mpsc::channel(); let wake = std::sync::Mutex::new(wake);
    r.set_preparation_wake(Arc::new(move || { let _ = wake.lock().unwrap().send(()); }));
    let budget = r.factory.memory_budget().clone();
    budget.set_ram_override(Some(384 << 20));
    let frame = packet(doc.scene(), [1024; 2]);
    r.memory_demand(frame);
    r.refresh_memory_limits();
    let import = budget.admit_import(budget.import_headroom_bytes()).unwrap();
    crate::test_support::submit_ready(&mut r, frame).unwrap();crate::test_support::complete(&r);
    let mut frames = 0;
    while r.next_frame() == FrameDemand::Work {
        frames += 1; assert!(frames < 20, "a cell refused object admission stops demanding frames");
        crate::test_support::submit_ready(&mut r, frame).unwrap(); crate::test_support::complete(&r);
    }
    assert!(!r.settled(), "the refused cell is still owed");
    let start = std::time::Instant::now(); let mut idle = 0;
    while start.elapsed() < std::time::Duration::from_secs(1) {
        if r.next_frame() == FrameDemand::Work { idle += 1; crate::test_support::submit_ready(&mut r, frame).unwrap(); crate::test_support::complete(&r); }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(idle, 0, "a cell refused object admission runs no frames before a release");
    while woken.try_recv().is_ok() {}
    drop(import); r.refresh_memory_limits();
    woken.recv_timeout(std::time::Duration::from_secs(10)).expect("the release wakes the refused cell");
    assert!(r.next_frame() == FrameDemand::Work, "the woken renderer retries the refused cell");
}
