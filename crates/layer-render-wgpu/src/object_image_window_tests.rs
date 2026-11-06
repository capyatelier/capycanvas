use crate::test_support::complete;


fn request(extent: [u32;2], bounds: PixelRect, value: u8) -> MovingRequest {
    let tile=layer_core::color::source::rgba8_source([PAGE_SIZE;2],|_,_|[value,73,149,255]);
    let blob=tile.tiles.values().next().unwrap().clone();
    let tiles=crate::page_coordinates(PixelRect::full(extent)).map(|coordinate|(coordinate,blob.clone())).collect();
    MovingRequest {bounds,source:Arc::new(SourceImage {extent,tiles,..(*tile).clone()}),..moving_request()}
}
fn drain(r: &WgpuRasterizer, images: &mut MovingImages, encoder: &mut crate::submission::CommandEncoder) {
    let texture=page_texture(r,wgpu::TextureFormat::Rgba32Float);
    let bytes:Vec<_>=(0..PAGE_SIZE*PAGE_SIZE).flat_map(|_|[0.25f32,0.5,0.75,1.]).flat_map(f32::to_le_bytes).collect();
    upload_page(r,&texture,&bytes);
    while let Some((id,source,coordinate))=images.next_build() {
        images.write_tile(r,encoder,id,&source,coordinate,&texture).unwrap();
    }
}

#[test]
fn ready_windows_publish_before_the_whole_image_completes_once() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let request=request([1024;2],PixelRect::new(256,256,512,512),31);
    let targets=vec![request.allocation(&r.device,request.level,false).unwrap().unwrap(),request.root(&r.device).unwrap().unwrap()];
    let mut images=MovingImages::default();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    images.allocate(&r,MipPlan {targets,bytes:8<<20,waiting:false},&mut encoder);
    let texture=page_texture(&r,wgpu::TextureFormat::Rgba32Float);let mut window_published=false;
    while let Some((id,source,coordinate))=images.next_build() {
        let changed=images.write_tile(&r,&mut encoder,id,&source,coordinate,&texture).unwrap();
        if !window_published && images.entries.iter().filter(|entry|!entry.root).all(|entry|entry.pending.is_empty()) {
            assert!(images.entries.iter().any(|entry|entry.root && !entry.pending.is_empty()));
            assert!(changed,"a ready viewport publishes before its whole-image fallback finishes");
            assert_eq!(images.completed_images,0);window_published=true;
        }
    }
    assert!(window_published);assert_eq!(images.completed_images,1);
    encoder.submit(&r.queue);complete(&r);
}

#[test]
fn panning_keeps_allocations_inside_the_view_margin() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut request=request([8192;2],PixelRect::new(2048,2048,3072,2816),31);
    let mut images=MovingImages::default();
    let budget=MovingImages::demand(std::slice::from_ref(&request),[1024,768]).unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let plan=images.plan(&r,std::slice::from_ref(&request),budget,&mut encoder).unwrap();
    images.allocate(&r,plan,&mut encoder);drain(&r,&mut images,&mut encoder);
    encoder.submit(&r.queue);complete(&r);
    request.bounds=PixelRect::new(2304,2048,3328,2816);
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let plan=images.plan(&r,std::slice::from_ref(&request),budget,&mut encoder).unwrap();
    assert_eq!(plan.targets.len(),1);
    let allocation=plan.targets[0].plan;
    assert_eq!(allocation.level,request.level);
    assert!(allocation.bounds.width()<=request.bounds.width()*2+2*PAGE_SIZE);
    assert!(allocation.bounds.height()<=request.bounds.height()*2+2*PAGE_SIZE);
    images.allocate(&r,plan,&mut encoder);drain(&r,&mut images,&mut encoder);
    encoder.submit(&r.queue);complete(&r);
    for offset in [32,128,256] {
        request.bounds=PixelRect::new(2304+offset,2048,3328+offset,2816);
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let plan=images.plan(&r,std::slice::from_ref(&request),budget,&mut encoder).unwrap();
        assert!(plan.targets.is_empty(),"pan {offset} must reuse its allocation");
        assert!(images.lookup(request.id,&request.source,request.level,request.bounds,request.format,r.device.working_space()).is_some());
        assert!(images.storage_bytes()<=budget);
        encoder.submit(&r.queue);complete(&r);
    }
}

#[test]
fn view_windows_build_and_evict_by_stack_rank() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let requests=[MovingRequest {rank:1,..request([2048;2],PixelRect::new(256,256,512,512),31)},
        MovingRequest {rank:28,..request([2048;2],PixelRect::new(1536,1536,1792,1792),73)}];
    let mut images=MovingImages::default();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let plan=images.plan(&r,&requests,32<<20,&mut encoder).unwrap();
    images.allocate(&r,plan,&mut encoder);
    assert_eq!(images.next_build().unwrap().0,requests[1].id);
    assert_eq!(images.upcoming(1)[0].0,requests[1].id);
    drain(&r,&mut images,&mut encoder);
    encoder.submit(&r.queue);complete(&r);
    let low=images.entries.iter().find(|entry|entry.id==requests[0].id).unwrap().storage_bytes();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    images.trim_unused(images.storage_bytes()-low,&requests,&mut encoder);
    assert!(images.entries.iter().all(|entry|entry.id==requests[1].id));
    encoder.submit(&r.queue);complete(&r);
}

#[test]
fn one_twenty_eight_and_one_hundred_mip_windows_share_one_gpu_bound_and_workspace() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let bound=working_bytes()+2*4*2560u64*1600*8;
    for count in [1,28,100] {
        let requests:Vec<_>=(0..count).map(|index|request([4096,3072],PixelRect::new(1536,1024,2048,1536),index)).collect();
        let mut images=MovingImages::default();
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let plan=images.plan(&r,&requests,bound,&mut encoder).unwrap();
        assert_eq!(plan.targets.iter().filter(|target|!target.root).count(),count as usize);
        assert_eq!(plan.targets.iter().filter(|target|target.root).count(),count as usize);
        assert!(plan.bytes<=bound);
        images.allocate(&r,plan,&mut encoder);
        assert_eq!(images.entries.len(),2*count as usize);
        assert!(images.entries.iter().all(|entry|entry.pending.len()==crate::page_coordinates(entry.image.plan.bounds).count()));
        assert!(images.storage_bytes()<=bound);
        drain(&r,&mut images,&mut encoder);
        assert!(images.entries.iter().all(Entry::ready));
        assert_eq!(images.completed_images,u64::from(count));
        assert!(images.storage_bytes()<=bound);
        assert!(images.pinned_bytes()<images.storage_bytes());
        encoder.submit(&r.queue);
        complete(&r);
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        assert!(images.trim_unused(0,&[],&mut encoder)>0);
        assert!(images.entries.is_empty());
        assert!(!images.retired.is_empty());
        assert!(images.pinned_bytes()>0);
        assert!(images.storage_bytes()<=bound);
        encoder.submit(&r.queue);
        complete(&r);
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        images.trim_unused(0,&[],&mut encoder);
        assert_eq!(images.storage_bytes(),0);
        assert!(!images.pending());
    }
}

#[test]
fn image_window_pixels_are_local_and_never_claim_missing_source_regions() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    for format in [wgpu::TextureFormat::Rgba16Float,wgpu::TextureFormat::Rgba32Float] {
        let bounds=PixelRect::new(2048,1024,2560,1536);
        let request=MovingRequest {format,..request([4096,3072],bounds,31)};
        let mut images=MovingImages::default();
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let plan=images.plan(&r,std::slice::from_ref(&request),32<<20,&mut encoder).unwrap();
        images.allocate(&r,plan,&mut encoder);
        assert_eq!(images.entries[0].image.plan.size,[512;2]);
        let upcoming=images.upcoming(16);
        assert_eq!(upcoming.len(),16);
        assert!(upcoming.iter().all(|(_,_,coordinate)|images.entries[0].image.plan.bounds.intersect(crate::page_rect(*coordinate)).area()>0));
        let texture=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
        let bytes:Vec<_>=(0..PAGE_SIZE*PAGE_SIZE).flat_map(|_|[0.25f32,0.5,0.75,1.]).flat_map(f32::to_le_bytes).collect();
        upload_page(&r,&texture,&bytes);
        for _ in 0..upcoming.len() {
            let (id,source,coordinate)=images.next_build().unwrap();
            images.write_tile(&r,&mut encoder,id,&source,coordinate,&texture).unwrap();
        }
        encoder.submit(&r.queue);
        let source=images.lookup(request.id,&request.source,1,bounds,request.format,r.device.working_space()).unwrap();
        assert_eq!(source.origin,[1792,768]);
        assert_eq!(source.extent,[512;2]);
        assert!(images.lookup(request.id,&request.source,1,PixelRect::full(request.source.extent),request.format,r.device.working_space()).is_none());
        let bytes=crate::layer_tests::page_bytes(&r,&images.entries[0].image.texture);
        let pixels:Vec<_>=if format==wgpu::TextureFormat::Rgba16Float {
            bytes.chunks_exact(2).map(|bytes|layer_core::color::f16::from_bits(u16::from_le_bytes(bytes.try_into().unwrap())).to_f32()).collect()
        } else {bytes.chunks_exact(4).map(|bytes|f32::from_le_bytes(bytes.try_into().unwrap())).collect()};
        assert!(pixels.chunks_exact(4).all(|pixel|pixel==[0.25,0.5,0.75,1.]));
    }
}

#[test]
fn window_mips_keep_transparent_partial_source_edges() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let extent=[257,259];
    let request=request(extent,PixelRect::full(extent),31);
    let mut images=MovingImages::default();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let plan=images.plan(&r,std::slice::from_ref(&request),16<<20,&mut encoder).unwrap();
    images.allocate(&r,plan,&mut encoder);
    while let Some((id,source,coordinate))=images.next_build() {
        let texture=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
        let bytes:Vec<_>=(0..PAGE_SIZE).flat_map(|y|(0..PAGE_SIZE).map(move|x|[x,y]))
            .flat_map(|local|if (0..2).all(|axis|coordinate[axis]*PAGE_SIZE+local[axis]<extent[axis]) {[0.25f32,0.5,0.75,1.]}else{[0.;4]})
            .flat_map(f32::to_le_bytes).collect();
        upload_page(&r,&texture,&bytes);
        images.write_tile(&r,&mut encoder,id,&source,coordinate,&texture).unwrap();
    }
    encoder.submit(&r.queue);
    let image=&images.entries[0].image;
    let bytes=crate::layer_tests::page_bytes(&r,&image.texture);
    let pixel=(129*image.texture.width() as usize+128)*8;
    let values:Vec<_>=bytes[pixel..pixel+8].chunks_exact(2)
        .map(|bytes|layer_core::color::f16::from_bits(u16::from_le_bytes(bytes.try_into().unwrap())).to_f32()).collect();
    assert_eq!(values,[0.0625,0.125,0.1875,0.25]);
}

#[test]
fn panning_reuses_overlapping_pages_and_holds_the_previous_window_until_ready() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let initial=request([2048;2],PixelRect::new(256,256,768,768),31);
    let mut images=MovingImages::default();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let plan=images.plan(&r,std::slice::from_ref(&initial),16<<20,&mut encoder).unwrap();
    images.allocate(&r,plan,&mut encoder);
    drain(&r,&mut images,&mut encoder);
    encoder.submit(&r.queue);
    complete(&r);
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let shifted=MovingRequest {bounds:PixelRect::new(512,256,1024,768),..initial.clone()};
    let plan=images.plan(&r,std::slice::from_ref(&shifted),16<<20,&mut encoder).unwrap();
    assert_eq!(plan.targets.len(),1);
    assert_eq!(images.entries.len(),2);
    let previous=images.entries.iter().find(|entry|!entry.root).unwrap().image.plan;
    images.allocate(&r,plan,&mut encoder);
    assert_eq!(images.entries.len(),3);
    let pending=images.entries.iter().find(|entry|!entry.root && !entry.pending.is_empty()).unwrap();
    assert!(pending.pending.iter().all(|coordinate|previous.bounds.intersect(crate::page_rect(*coordinate)).is_empty()));
    assert!(images.lookup(initial.id,&initial.source,1,PixelRect::new(512,256,768,768),initial.format,r.device.working_space()).is_some());
    let fallback=images.lookup(initial.id,&initial.source,1,shifted.bounds,initial.format,r.device.working_space()).unwrap();
    assert!(fallback.level>initial.level);assert_eq!(fallback.origin,[0;2]);
    drain(&r,&mut images,&mut encoder);
    let plan=images.plan(&r,std::slice::from_ref(&shifted),16<<20,&mut encoder).unwrap();
    assert!(plan.targets.is_empty());
    assert_eq!(images.entries.len(),2);
    assert_eq!(images.retired.len(),1);
    assert!(images.lookup(initial.id,&initial.source,1,shifted.bounds,initial.format,r.device.working_space()).is_some());
    encoder.submit(&r.queue);
    complete(&r);
}

#[test]
fn mip_windows_share_sources_and_invalidate_on_format_context_or_abandoned_submission() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let request=request([512;2],PixelRect::new(0,0,256,256),31);
    let mut images=MovingImages::default();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let plan=images.plan(&r,&[request.clone(),request.clone()],16<<20,&mut encoder).unwrap();
    assert_eq!(plan.targets.len(),2);
    images.allocate(&r,plan,&mut encoder);
    drain(&r,&mut images,&mut encoder);
    drop(encoder);
    assert!(images.pending());
    assert!(images.next_build().is_some());
    assert_eq!(images.entries[0].pending.len(),4);
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    drain(&r,&mut images,&mut encoder);
    encoder.submit(&r.queue);
    complete(&r);
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let reused=images.plan(&r,std::slice::from_ref(&request),16<<20,&mut encoder).unwrap();
    assert!(reused.targets.is_empty());
    let changed=MovingRequest {format:wgpu::TextureFormat::Rgba32Float,..request};
    let replaced=images.plan(&r,&[changed],16<<20,&mut encoder).unwrap();
    assert_eq!(replaced.targets.len(),2);
    assert_eq!(images.retired.len(),2);
    assert!(images.pinned_bytes()>0);
    images.allocate(&r,replaced,&mut encoder);
    drain(&r,&mut images,&mut encoder);
    encoder.submit(&r.queue);
    complete(&r);
}

#[test]
fn any_effect_float_source_or_hdr_keeps_the_whole_evaluation_full_precision() {
    use layer_core::{authored::*, color::{DocumentColor,SampleDepth,RgbSpace}};
    fn document(source: Arc<SourceImage>, color: DocumentColor) -> layer_core::Document {
        let mut artwork=Artwork::new([16;2]).unwrap();
        let object=artwork.objects.insert(PortableId::random(),ImageObject::new(Image::new(source))).unwrap();
        let owner=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(object),"Photo")).unwrap();
        let composition=artwork.compositions.get_mut(artwork.root).unwrap();
        composition.color=color;let stack=composition.result;
        artwork.stacks.get_mut(stack).unwrap().entries.push(owner);
        layer_core::Document::from_artwork(artwork).unwrap()
    }
    let source=layer_core::color::source::rgba8_source([16;2],|_,_|[31,73,149,255]);
    let mut plain=document(source.clone(),Default::default());
    assert_eq!(MovingImages::format(plain.scene()),wgpu::TextureFormat::Rgba16Float);
    crate::artwork_sample_tests::insert_effect(&mut plain,layer_core::EffectInstance::new(crate::tests::fixture("invert").program()),0);
    assert_eq!(MovingImages::format(plain.scene()),wgpu::TextureFormat::Rgba32Float);
    let hdr=document(source,DocumentColor {depth:SampleDepth::F32,..Default::default()});
    assert_eq!(MovingImages::format(hdr.scene()),wgpu::TextureFormat::Rgba32Float);
    let source=crate::test_support::depth_source([16;2],SampleDepth::F32,RgbSpace::Srgb,1<<20,|_,_|[2.,0.5,0.25,1.]);
    let float=document(source,Default::default());
    assert_eq!(MovingImages::format(float.scene()),wgpu::TextureFormat::Rgba32Float);
}

#[test]
fn shrinking_grants_retire_fallbacks_and_complete_a_clipped_retarget() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let initial=request([4096;2],PixelRect::new(256,256,2304,2304),31);
    let target=MovingRequest {bounds:PixelRect::new(3840,1024,4096,1280),..initial.clone()};
    let mut images=MovingImages::default();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let plan=images.plan(&r,std::slice::from_ref(&initial),64<<20,&mut encoder).unwrap();
    images.allocate(&r,plan,&mut encoder);drain(&r,&mut images,&mut encoder);
    encoder.submit(&r.queue);complete(&r);
    let budget=working_bytes()+(1<<20);
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let plan=images.plan(&r,std::slice::from_ref(&target),budget,&mut encoder).unwrap();
    assert!(plan.waiting);assert!(!images.retired.is_empty());
    encoder.submit(&r.queue);complete(&r);
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let plan=images.plan(&r,std::slice::from_ref(&target),budget,&mut encoder).unwrap();
    assert!(!plan.waiting);assert!(!plan.targets.is_empty());
    images.allocate(&r,plan,&mut encoder);drain(&r,&mut images,&mut encoder);
    encoder.submit(&r.queue);complete(&r);
    assert!(images.lookup(target.id,&target.source,1,target.bounds,target.format,r.device.working_space()).is_some());
    assert!(images.storage_bytes()<=budget);
    assert_eq!(MovingImages::demand(&[initial],[2560,1600]).unwrap(),MovingImages::demand(&[target],[2560,1600]).unwrap());
}

#[test]
fn retargeting_unfinished_windows_preserves_every_completed_overlap_page() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut request=request([4096;2],PixelRect::new(512,512,1024,1024),31);
    let mut images=MovingImages::default();let mut completed=std::collections::BTreeSet::new();
    let texture=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
    upload_page(&r,&texture,&vec![0;PAGE_SIZE as usize*PAGE_SIZE as usize*16]);
    for step in 0..6 {
        request.bounds=PixelRect::new(512+step*256,512,1024+step*256,1024);
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let plan=images.plan(&r,std::slice::from_ref(&request),32<<20,&mut encoder).unwrap();
        images.allocate(&r,plan,&mut encoder);
        let entry=images.entries.last().unwrap();
        for coordinate in &completed {
            if entry.image.plan.bounds.intersect(crate::page_rect(*coordinate)).area()>0 {
                assert!(!entry.pending.contains(coordinate),"completed {coordinate:?} was discarded on step {step}");
            }
        }
        let (id,source,coordinate)=images.next_build().unwrap();
        assert!(completed.insert(coordinate));
        images.write_tile(&r,&mut encoder,id,&source,coordinate,&texture).unwrap();
        encoder.submit(&r.queue);complete(&r);
    }
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    drain(&r,&mut images,&mut encoder);encoder.submit(&r.queue);complete(&r);
    assert!(images.lookup(request.id,&request.source,1,request.bounds,request.format,r.device.working_space()).is_some());
}

fn photo_document(requests:&[MovingRequest],extent:[u32;2])->layer_core::Document {
    use layer_core::authored::*;
    let mut artwork=Artwork::new(extent).unwrap();
    let stack=artwork.compositions.get(artwork.root).unwrap().result;
    for request in requests {
        let object=artwork.objects.insert(PortableId::random(),ImageObject::new(Image::new(request.source.clone()))).unwrap();
        let owner=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(object),"Photo")).unwrap();
        artwork.stacks.get_mut(stack).unwrap().entries.push(owner);
    }
    layer_core::Document::from_artwork(artwork).unwrap()
}

#[test]
fn overlapping_photos_keep_the_top_or_dragged_window_at_the_requested_level() {
    let budget=layer_core::MemoryBudget::with_facts(layer_core::MemoryFacts {host_default_bytes:Some(1<<40),override_bytes:Some(1<<40),..Default::default()});
    let mut r=crate::memory::memory_tests::renderer_in_budget(Default::default(),layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<40),..Default::default()},budget).unwrap();
    for (device,extent,viewport) in [("Desktop",[4216,3016],[2560,1600]),("TCL",[4248,2832],[1920,1200]),("S7",[4216,3016],[2560,1600]),("Smaller view",[4216,3016],[2000,1200])] {
        let requests:Vec<_>=(0..28).map(|index|request(extent,PixelRect::full(extent),index)).collect();
        let doc=photo_document(&requests,extent);
        let top=doc.scene().order()[0];let bottom=*doc.scene().order().last().unwrap();
        let zoom=(viewport[0] as f32/extent[0] as f32).min(viewport[1] as f32/extent[1] as f32);
        let mut packet=crate::test_support::packet(doc.scene(),extent);
        packet.view=layer_render::ViewState {width_px:viewport[0],height_px:viewport[1],document_to_surface:[zoom,0.,0.,zoom,
            (viewport[0] as f32-extent[0] as f32*zoom)/2.,(viewport[1] as f32-extent[1] as f32*zoom)/2.]};
        for moving in [None,Some(bottom)] {
            layer_render::CanvasRenderer::prepare_moving_layer(&mut r,moving);
            r.prepare_moving_projection(packet).unwrap();r.begin_frame_memory(packet);
            let requests=r.moving_projection.as_ref().unwrap().requests.clone();
            assert_eq!(requests.len(),28);
            let selected=doc.scene().object_layer(moving.unwrap_or(top)).unwrap().image.id();
            let selected_request=requests.iter().find(|request|request.id==selected).unwrap();
            assert_eq!(selected_request.level,1);
            assert_eq!(selected_request.rank,if moving.is_some() {usize::MAX}else{28});
            let bound=MovingImages::demand(&requests,viewport).unwrap();
            assert_eq!(bound,working_bytes()+2*4*u64::from(viewport[0])*u64::from(viewport[1])*8);
            assert_eq!(r.memory_limit(layer_core::MemoryOwnerKind::Mips),bound);
            let mut images=MovingImages::default();let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            let plan=images.plan(&r,&requests,bound,&mut encoder).unwrap();
            assert!(plan.bytes<=bound);
            assert!(plan.targets.iter().any(|target|!target.root && target.request.id==selected && target.plan.level==selected_request.level),"{device}: top image must keep its requested level");
            images.allocate(&r,plan,&mut encoder);drain(&r,&mut images,&mut encoder);
            encoder.submit(&r.queue);complete(&r);
            assert!(images.storage_bytes()<=bound);
            for request in &requests {
                let ready=images.lookup(request.id,&request.source,request.level,request.bounds,request.format,r.device.working_space()).unwrap();
                if request.id==selected {assert_eq!(ready.level,request.level);}
            }
        }
    }
}

#[test]
fn actual_size_projection_plans_only_the_current_photo_window_and_halo() {
    let extent=[4216,3016];let viewport=[2560,1600];
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let requests=[request(extent,PixelRect::full(extent),31)];
    let doc=photo_document(&requests,extent);
    let mut packet=crate::test_support::packet(doc.scene(),extent);
    packet.view=layer_render::ViewState {width_px:viewport[0],height_px:viewport[1],document_to_surface:[1.,0.,0.,1.,-828.,-708.]};
    let mut projection=None;let mut failures=Vec::new();
    for offset in [0.,32.,200.,-200.] {
        packet.view.document_to_surface[4]=-828.+offset;
        crate::MovingProjection::update(&mut projection,doc.scene(),packet.view,r.device.working_space(),true,&mut failures,None).unwrap();
        let request=&projection.as_ref().unwrap().requests[0];
        let visible=display_mips::view_bounds(packet.view,extent,0).unwrap();
        assert!(!request.bounds.is_empty());assert_eq!(request.level,1);
        assert_eq!(request.bounds.intersect(visible),visible);
        let target=request.allocation(&r.device,request.level,false).unwrap().unwrap();
        assert!(target.plan.bounds.width()<=viewport[0]+2*PAGE_SIZE);
        assert!(target.plan.bounds.height()<=viewport[1]+2*PAGE_SIZE);
        assert!(target.plan.bounds.area()<PixelRect::full(extent).area()*3/5);
        let halo=request.bounds.expand(1<<target.plan.level,extent);
        assert_eq!(target.plan.bounds.intersect(halo),halo);
    }
}

#[test]
fn roots_share_the_view_allowance_and_evict_before_requested_windows() {
    let budget=layer_core::MemoryBudget::with_facts(layer_core::MemoryFacts {host_default_bytes:Some(1<<40),override_bytes:Some(1<<40),..Default::default()});
    let r=crate::memory::memory_tests::renderer_in_budget(Default::default(),layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<40),..Default::default()},budget).unwrap();
    let first=request([2048;2],PixelRect::new(256,256,512,512),31);
    let requests=[first.clone(),MovingRequest {bounds:PixelRect::new(1536,1536,1792,1792),..first}];
    r.memory.mip_demand(32<<20,[2560,1600]);r.publish_memory(true);r.factory.memory_budget().flush_reports();
    let mut images=MovingImages::default();let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let plan=images.plan(&r,&requests,32<<20,&mut encoder).unwrap();
    images.allocate(&r,plan,&mut encoder);
    let texture=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
    upload_page(&r,&texture,&vec![0;PAGE_SIZE as usize*PAGE_SIZE as usize*16]);
    while images.entries.iter().any(|entry|!entry.root && !entry.ready()) {
        let (id,source,coordinate)=images.next_build().unwrap();
        images.write_tile(&r,&mut encoder,id,&source,coordinate,&texture).unwrap();
    }
    assert_eq!(images.entries.iter().filter(|entry|!entry.root && entry.ready()).count(),2);
    assert!(images.entries.iter().any(|entry|entry.root && !entry.ready()));
    assert_eq!(images.trim_unused(32<<20,&requests,&mut encoder),0);
    assert_eq!(images.entries.iter().filter(|entry|!entry.root && entry.ready()).count(),2);
    assert!(images.retired.is_empty());
    drain(&r,&mut images,&mut encoder);
    let roots=images.entries.iter().filter(|entry|entry.root).map(Entry::storage_bytes).sum::<u64>();
    assert!(roots>0);
    assert_eq!(images.trim_unused(images.storage_bytes()-roots,&requests,&mut encoder),roots);
    assert_eq!(images.entries.iter().filter(|entry|!entry.root && entry.ready()).count(),2);
    assert!(images.entries.iter().all(|entry|!entry.root));
    encoder.submit(&r.queue);complete(&r);
}

#[test]
fn every_view_page_is_offered_before_root_only_pages() {
    let budget=layer_core::MemoryBudget::with_facts(layer_core::MemoryFacts {host_default_bytes:Some(1<<40),override_bytes:Some(1<<40),..Default::default()});
    let r=crate::memory::memory_tests::renderer_in_budget(Default::default(),layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<40),..Default::default()},budget).unwrap();
    let requests=[request([2048;2],PixelRect::new(256,256,512,512),31),request([2048;2],PixelRect::new(1536,1536,1792,1792),73)];
    r.memory.mip_demand(32<<20,[2560,1600]);r.publish_memory(true);r.factory.memory_budget().flush_reports();
    let mut images=MovingImages::default();let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let plan=images.plan(&r,&requests,32<<20,&mut encoder).unwrap();
    images.allocate(&r,plan,&mut encoder);
    let windows=images.entries.iter().filter(|entry|!entry.root).map(|entry|entry.pending.len()).sum::<usize>();
    let upcoming=images.upcoming(usize::MAX);
    assert!(upcoming.len()>windows);
    for (id,_,coordinate) in &upcoming[..windows] {
        assert!(images.entries.iter().any(|entry|!entry.root && entry.id==*id && entry.pending.contains(coordinate)));
    }
    let texture=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
    upload_page(&r,&texture,&vec![0;PAGE_SIZE as usize*PAGE_SIZE as usize*16]);
    while let Some((id,source,coordinate))=images.next_build() {
        if images.entries.iter().any(|entry|!entry.root && !entry.ready()) {
            assert!(images.entries.iter().any(|entry|!entry.root && entry.id==id && entry.pending.contains(&coordinate)));
        }
        images.write_tile(&r,&mut encoder,id,&source,coordinate,&texture).unwrap();
        assert!(images.entries.iter().filter(|entry|entry.id==id).all(|entry|!entry.pending.contains(&coordinate)));
    }
    assert!(images.entries.iter().all(Entry::ready));
    encoder.submit(&r.queue);complete(&r);
}

#[test]
fn offscreen_nearest_images_do_not_request_unused_storage() {
    let request=MovingRequest {nearest:true,level:0,bounds:PixelRect::EMPTY,..moving_request()};
    assert_eq!(MovingImages::demand(&[request],[2560,1600]).unwrap(),0);
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn projected_store_backed_photos_share_one_grant_bound_and_small_whole_image_roots() {
    use crate::test_support::{StoredTiles,tile_read_cache};
    let budget=layer_core::MemoryBudget::with_facts(layer_core::MemoryFacts {host_default_bytes:Some(1<<40),override_bytes:Some(1<<40),..Default::default()});
    let mut r=crate::memory::memory_tests::renderer_in_budget(Default::default(),layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<40),..Default::default()},budget).unwrap();
    let bound=working_bytes()+2*4*2560u64*1600*8;
    for count in [1,28,100] {
        let requests:Vec<_>=(0..count).map(|index|request([1024,768],PixelRect::full([1024,768]),index)).collect();
        let editor=layer_core::Editor::new(photo_document(&requests,[4096,3072]));
        let _store=StoredTiles::new(&editor.retained_tiles(),tile_read_cache());
        let mut packet=crate::test_support::packet(editor.document().scene(),[4096,3072]);
        packet.view=layer_render::ViewState {width_px:2560,height_px:1600,document_to_surface:[1.,0.,0.,1.,-768.,-512.]};
        r.prepare_moving_projection(packet).unwrap();r.begin_frame_memory(packet);
        let requests=r.moving_projection.as_ref().unwrap().requests.clone();
        assert_eq!(requests.len(),count as usize);assert_eq!(r.memory_limit(layer_core::MemoryOwnerKind::Mips),bound);
        let mut images=MovingImages::default();let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let plan=images.plan(&r,&requests,bound,&mut encoder).unwrap();
        assert_eq!(plan.targets.iter().filter(|target|target.root).count(),count as usize);
        images.allocate(&r,plan,&mut encoder);
        assert!(editor.retained_tiles().resident_bytes()==0);
        let mut count=0;
        while let Some((id,source,coordinate))=images.next_build() {
            let pixels=prepare_image_tile(&source,coordinate,r.device.working_space(),&r.device.source_samples,None).unwrap();
            let tile=crate::scene::Scene::decode_prepared(&mut r,&mut encoder,&source,coordinate,pixels).unwrap();
            images.write_tile(&r,&mut encoder,id,&source,coordinate,&tile.texture).unwrap();count+=1;
            if count%8==0 {
                r.uploads.finish(&encoder);encoder.submit(&r.queue);complete(&r);
                encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            }
        }
        r.uploads.finish(&encoder);encoder.submit(&r.queue);complete(&r);
        assert!(images.storage_bytes()<=bound);assert!(images.entries.iter().all(Entry::ready));
        for request in &requests {
            let root=images.lookup(request.id,&request.source,5,PixelRect::full(request.source.extent),request.format,r.device.working_space()).unwrap();
            assert_eq!(root.level,2);assert!(root.extent.into_iter().all(|side|side<=256));
        }
    }
}

#[test]
fn mixed_scale_windows_and_roots_use_the_requested_level_format_and_context() {
    let budget=layer_core::MemoryBudget::with_facts(layer_core::MemoryFacts {host_default_bytes:Some(1<<40),override_bytes:Some(1<<40),..Default::default()});
    let r=crate::memory::memory_tests::renderer_in_budget(Default::default(),layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<40),..Default::default()},budget).unwrap();
    let initial=request([4096;2],PixelRect::new(1024,1024,1536,1536),31);
    let requests=[initial.clone(),MovingRequest {level:3,..initial.clone()},MovingRequest {level:4,bounds:PixelRect::new(2560,2560,3072,3072),..initial.clone()},
        MovingRequest {bounds:PixelRect::EMPTY,..request([512;2],PixelRect::EMPTY,17)}];
    r.memory.mip_demand(64<<20,[2560,1600]);r.publish_memory(true);r.factory.memory_budget().flush_reports();
    let mut images=MovingImages::default();let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let plan=images.plan(&r,&requests,64<<20,&mut encoder).unwrap();
    assert_eq!(plan.targets.iter().filter(|target|!target.root).count(),3);
    images.allocate(&r,plan,&mut encoder);drain(&r,&mut images,&mut encoder);encoder.submit(&r.queue);complete(&r);
    let lookup=|level,bounds,format,space|images.lookup(initial.id,&initial.source,level,bounds,format,space);
    assert_eq!(lookup(1,initial.bounds,initial.format,r.device.working_space()).unwrap().level,1);
    assert_eq!(lookup(3,initial.bounds,initial.format,r.device.working_space()).unwrap().level,3);
    assert_eq!(lookup(5,PixelRect::full(initial.source.extent),initial.format,r.device.working_space()).unwrap().level,4);
    assert!(lookup(1,initial.bounds,wgpu::TextureFormat::Rgba32Float,r.device.working_space()).is_none());
    let other=if r.device.working_space()==RgbSpace::Srgb {RgbSpace::DisplayP3}else{RgbSpace::Srgb};
    assert!(lookup(1,initial.bounds,initial.format,other).is_none());
    let offscreen=&requests[3];
    assert!(images.lookup(offscreen.id,&offscreen.source,5,PixelRect::full(offscreen.source.extent),offscreen.format,r.device.working_space()).is_some());
}

#[test]
fn rotated_nearest_and_mixed_scale_occurrences_keep_disjoint_source_windows() {
    use layer_core::authored::*;
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let source=request([4096;2],PixelRect::full([4096;2]),31).source;
    let image=Image::new(source);let mut artwork=Artwork::new([4096;2]).unwrap();
    let stack=artwork.compositions.get(artwork.root).unwrap().result;
    for (affine,interpolation) in [
        (Affine64::default(),ImageInterpolation::Linear),
        (Affine64([1.,0.,0.,1.,-3000.,0.]),ImageInterpolation::Linear),
        (Affine64([0.25,0.,0.,0.25,0.,0.]),ImageInterpolation::Linear),
        (Affine64::default(),ImageInterpolation::Nearest),
    ] {
        let mut object=ImageObject::new(image.clone());object.affine=affine;object.interpolation=interpolation;
        let handle=artwork.objects.insert(PortableId::random(),object).unwrap();
        let owner=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(handle),"Photo")).unwrap();
        artwork.stacks.get_mut(stack).unwrap().entries.push(owner);
    }
    let doc=layer_core::Document::from_artwork(artwork).unwrap();
    let view=layer_render::ViewState {width_px:512,height_px:512,document_to_surface:[1.,0.,0.,1.,0.,0.]};
    let mut cache=None;let mut failures=Vec::new();
    crate::MovingProjection::update(&mut cache,doc.scene(),view,r.device.working_space(),true,&mut failures,None).unwrap();
    let requests=&cache.as_ref().unwrap().requests;
    let demand=MovingImages::demand(requests,[512;2]).unwrap();
    assert!(demand>working_bytes());
    assert!(requests.iter().any(|request|request.root(&r.device).unwrap().is_some()));
    let same_scale:Vec<_>=requests.iter().filter(|request|request.level==1 && !request.nearest).collect();
    assert_eq!(same_scale.len(),2);assert!(same_scale[0].bounds.intersect(same_scale[1].bounds).is_empty());
    assert!(requests.iter().any(|request|request.level==2));assert!(requests.iter().any(|request|request.nearest && request.level==0));
    let rotated=layer_render::ViewState {document_to_surface:[0.,0.5,-0.25,0.,512.,0.],..view};
    crate::MovingProjection::update(&mut cache,doc.scene(),rotated,r.device.working_space(),true,&mut failures,None).unwrap();
    for request in &cache.as_ref().unwrap().requests {
        let level=if request.nearest {0} else {request.level.max(3)};
        let plan=request.allocation(&r.device,level,false).unwrap().unwrap().plan;
        let halo=request.bounds.expand(1<<level,request.source.extent);
        assert_eq!(plan.bounds.intersect(halo),halo);
    }
    let outside=layer_render::ViewState {document_to_surface:[1.,0.,0.,1.,-8192.,0.],..view};
    crate::MovingProjection::update(&mut cache,doc.scene(),outside,r.device.working_space(),true,&mut failures,None).unwrap();
    assert!(!cache.as_ref().unwrap().requests.is_empty());
    assert!(cache.as_ref().unwrap().requests.iter().all(|request|request.bounds.is_empty()));
    for view in [layer_render::ViewState {width_px:0,..view},layer_render::ViewState {document_to_surface:[0.;6],..view}] {
        crate::MovingProjection::update(&mut cache,doc.scene(),view,r.device.working_space(),true,&mut failures,None).unwrap();
        assert!(cache.as_ref().unwrap().requests.is_empty());
    }
}

#[test]
fn partial_view_readiness_notifies_the_renderer_before_its_root_completes() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let requests=[request([2048;2],PixelRect::full([2048;2]),31)];
    let doc=photo_document(&requests,[2048;2]);
    let mut frame=crate::test_support::packet(doc.scene(),[2048;2]);
    frame.view=layer_render::ViewState {width_px:256,height_px:256,document_to_surface:[1.,0.,0.,1.,-512.,-512.]};
    r.prepare_moving_projection(frame).unwrap();r.begin_frame_memory(frame);
    let projected=r.moving_projection.as_ref().unwrap().requests[0].clone();
    let bounds=projected.allocation(&r.device,projected.level,false).unwrap().unwrap().plan.bounds;
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    for coordinate in crate::page_coordinates(bounds) {
        let pixels=prepare_image_tile(&projected.source,coordinate,r.device.working_space(),&r.device.source_samples,None).unwrap();
        crate::scene::Scene::decode_prepared(&mut r,&mut encoder,&projected.source,coordinate,pixels).unwrap();
    }
    r.uploads.finish(&encoder);encoder.submit(&r.queue);complete(&r);
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let changed=r.prepare_moving_images(frame,&mut encoder).unwrap();
    assert!(r.moving_images.lookup(projected.id,&projected.source,projected.level,projected.bounds,projected.format,r.device.working_space()).is_some());
    assert!(r.moving_images.pending());assert_eq!(r.moving_images.completed_images,0);
    assert!(changed,"a newly ready view window must invalidate its presentation while the root is pending");
    r.uploads.finish(&encoder);encoder.submit(&r.queue);complete(&r);
}
