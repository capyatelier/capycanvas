use crate::test_support::{complete,submit_encoded,wait_until};

fn next_build(images:&mut MovingImages)->Option<(Arc<SourceImage>,[u32;2])> {
    images.next_build(|_|true,|_,_,_|true).map(|(source,coordinate,_)|(source,coordinate))
}
fn paced_builds(r:&mut WgpuRasterizer,images:&mut MovingImages,encoder:&mut crate::submission::CommandEncoder,texture:&wgpu::Texture,until:impl Fn(&MovingImages)->bool)->usize {
    let mut work=crate::scene::Scene::begin_paced(true,crate::scene::PASS_TAPS);let mut built=[false;6];let mut builds=0;
    while let Some((source,coordinate,class))=images.next_build(|class|!built[class as usize] || work.remaining()>=crate::scene::PASS_TAPS,|_,_,_|true) {
        images.write_tile(r,encoder,&source,coordinate,texture,None).unwrap();
        work.spend(crate::scene::PASS_TAPS);built[class as usize]=true;builds+=1;
        if until(images) {break;}
    }
    builds
}

fn window_builds_while_overview_is_partial(bounds:PixelRect) {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut request=request([2048;2],PixelRect::full([2048;2]),31);
    let mut images=MovingImages::within(&r.scene_tiers);let texture=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
    pollster::block_on(r.compilation.prepare_all(images.pipelines(&r.device))).unwrap();
    upload_page(&r,&texture,&vec![0;PAGE_SIZE as usize*PAGE_SIZE as usize*16]);
    submit_encoded(&mut r,|r,encoder| {
        let root=request.root(&r.device).unwrap().unwrap();
        images.allocate(r,MipPlan {targets:vec![root],waiting:false},encoder);
        for _ in 0..8 {
            let (source,coordinate)=next_build(&mut images).unwrap();
            images.write_tile(r,encoder,&source,coordinate,&texture,None).unwrap();
        }
    });complete(&r);
    request.bounds=bounds;
    let window=request.allocation(&r.device,1,false).unwrap().unwrap();
    let (own,builds)=submit_encoded(&mut r,|r,encoder| {
        images.allocate(r,MipPlan {targets:vec![window],waiting:false},encoder);
        let own=images.entries.iter().find(|entry|!entry.root).unwrap().pending.len();
        let mut builds=0;
        while images.entries.iter().any(|entry|!entry.root && !entry.pending.is_empty()) {
            builds+=paced_builds(r,&mut images,encoder,&texture,|images|images.entries.iter().all(|entry|entry.root || entry.pending.is_empty()));
        }
        (own,builds)
    });complete(&r);
    assert!(builds<=2*own,"window waited behind unrelated overview inputs: builds={builds}, own={own}");
}
fn trim(images:&mut MovingImages,limit:u64,encoder:&mut crate::submission::CommandEncoder)->u64 {
    images.live.heap().begin_frame();
    images.live.heap().evict(images.storage_bytes().saturating_sub(images.retiring.used()).saturating_sub(limit),Class::Fallback);
    images.retire_evicted(encoder)
}

#[test]
fn zoom_window_does_not_wait_for_remaining_overview_inputs() {
    window_builds_while_overview_is_partial(PixelRect::new(768,768,1024,1024));
}
#[test]
fn corner_window_does_not_wait_for_remaining_overview_inputs() {
    window_builds_while_overview_is_partial(PixelRect::new(0,0,256,256));
}

#[test]
fn overview_source_progress_survives_continuously_replaced_windows() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut request=request([2048;2],PixelRect::new(0,0,256,256),31);
    let mut images=MovingImages::within(&r.scene_tiers);let texture=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
    pollster::block_on(r.compilation.prepare_all(images.pipelines(&r.device))).unwrap();
    upload_page(&r,&texture,&vec![0;PAGE_SIZE as usize*PAGE_SIZE as usize*16]);
    let mut previous=usize::MAX;
    for frame in 0..64 {
        request.bounds=if frame%2==0 {PixelRect::new(0,0,256,256)}else {PixelRect::new(1792,1792,2048,2048)};
        submit_encoded(&mut r,|r,encoder| {
            let plan=images.plan(r,std::slice::from_ref(&request),32<<20,encoder).unwrap();
            images.allocate(r,plan,encoder);
            paced_builds(r,&mut images,encoder,&texture,|_|false);
        });complete(&r);
        let remaining=images.entries.iter().filter(|entry|entry.root).map(|entry|entry.pending.len()).sum::<usize>();
        assert!(remaining<previous,"camera-window replacement cannot starve the overview: frame={frame}, remaining={remaining}, previous={previous}");
        if remaining==0 {return;}
        previous=remaining;
    }
    panic!("the overview must complete after each of its64source tiles is offered");
}


fn request(extent: [u32;2], bounds: PixelRect, value: u8) -> MovingRequest {
    let tile=layer_core::color::source::rgba8_source([PAGE_SIZE;2],|_,_|[value,73,149,255]);
    let blob=tile.tiles.values().next().unwrap().clone();
    let tiles=crate::page_coordinates(PixelRect::full(extent)).map(|coordinate|(coordinate,blob.clone())).collect();
    MovingRequest {bounds,source:Arc::new(SourceImage {extent,tiles,..(*tile).clone()}),..moving_request()}
}
fn drain(r: &mut WgpuRasterizer, images: &mut MovingImages, encoder: &mut crate::submission::CommandEncoder) {
    pollster::block_on(r.compilation.prepare_all(images.pipelines(&r.device))).unwrap();
    let texture=page_texture(r,wgpu::TextureFormat::Rgba32Float);
    let bytes:Vec<_>=(0..PAGE_SIZE*PAGE_SIZE).flat_map(|_|[0.25f32,0.5,0.75,1.]).flat_map(f32::to_le_bytes).collect();
    upload_page(r,&texture,&bytes);
    while let Some((source,coordinate))=next_build(images) {
        images.write_tile(r,encoder,&source,coordinate,&texture,None).unwrap();
    }
}

#[test]
fn ready_windows_publish_before_the_whole_image_completes_once() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let request=request([1024;2],PixelRect::new(256,256,512,512),31);
    let targets=vec![request.allocation(&r.device,request.level,false).unwrap().unwrap(),request.root(&r.device).unwrap().unwrap()];
    let mut images=MovingImages::within(&r.scene_tiers);
    pollster::block_on(r.compilation.prepare_all(images.pipelines(&r.device))).unwrap();
    submit_encoded(&mut r,|r,encoder| {
        images.allocate(r,MipPlan {targets,waiting:false},encoder);
        let texture=page_texture(r,wgpu::TextureFormat::Rgba32Float);let mut window_published=false;
        while let Some((source,coordinate))=next_build(&mut images) {
            let changed=images.write_tile(r,encoder,&source,coordinate,&texture,None).unwrap();
            if !window_published && images.entries.iter().filter(|entry|!entry.root).all(|entry|entry.pending.is_empty()) {
                assert!(images.entries.iter().any(|entry|entry.root && !entry.pending.is_empty()));
                assert!(changed,"a ready viewport publishes before its whole-image fallback finishes");
                assert_eq!(images.completed_images,0);window_published=true;
            }
        }
        assert!(window_published);assert_eq!(images.completed_images,1);
    });complete(&r);
}

#[test]
fn a_gigapixel_overview_commits_its_ready_regional_object_mip() {
    use layer_core::authored::*;
    let extent=[32768,30518];let source_extent=[9504,6336];
    let tile=layer_core::color::source::rgba8_source([PAGE_SIZE;2],|_,_|[31,73,149,255]);
    let blob=tile.tiles.values().next().unwrap().clone();
    let source=Arc::new(layer_core::color::source::SourceImage {extent:source_extent,
        tiles:crate::page_coordinates(PixelRect::full(source_extent)).map(|coordinate|(coordinate,blob.clone())).collect(),..(*tile).clone()});
    let mut artwork=Artwork::new(extent).unwrap();
    let image=layer_core::Image::new(source.clone());let id=image.id();
    let mut object=ImageObject::new(image);
    object.affine=layer_core::Affine64([extent[0] as f64/source_extent[0] as f64,0.,0.,extent[1] as f64/source_extent[1] as f64,0.,0.]);
    let handle=artwork.objects.insert(PortableId::random(),object).unwrap();
    let owner=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(handle),"Photo")).unwrap();
    let stack=artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries.push(owner);
    let document=layer_core::Document::from_artwork(artwork).unwrap();
    let mut renderer=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
    renderer.ensure_document_metadata(extent,document.scene()).unwrap();
    let mut frame=crate::test_support::packet(document.scene(),extent);
    frame.view=layer_render::ViewState {width_px:256,height_px:256,document_to_surface:[1./16.,0.,0.,1./16.,0.,0.]};
    crate::memory::memory_tests::legacy_scene_frame(&mut renderer,frame);
    let request=crate::object_image_mips::MovingRequest {id,source:source.clone(),level:1,nearest:false,
        bounds:PixelRect::new(0,0,1191,853),format:wgpu::TextureFormat::Rgba16Float,rank:0};
    let mut images=crate::object_image_mips::MovingImages::within(&renderer.scene_tiers);
    pollster::block_on(renderer.compilation.prepare_all(images.pipelines(&renderer.device))).unwrap();
    submit_encoded(&mut renderer,|renderer,encoder| {
        let allocation=images.plan(renderer,std::slice::from_ref(&request),32<<20,encoder).unwrap();
        images.allocate(renderer,allocation,encoder);
        images.entries.retain(|entry|entry.root);
        assert_eq!(images.entries.len(),1);
        let root=&mut images.entries[0];
        assert_eq!(root.image.plan.level,6);
        let halo=crate::scene::scale::regional_object_overview_footprint(renderer,&document,owner).expand(1<<root.image.plan.level,source_extent);
        let footprint=crate::page_coordinates(halo).count();let pending=root.pending.len()-footprint;
        let mut delivery=root.pending.drain(..).collect::<Vec<_>>();
        delivery.sort_by_key(|coordinate|crate::page_rect(*coordinate).intersect(halo).is_empty());
        root.pending=delivery.into();
        let texture=page_texture(renderer,wgpu::TextureFormat::Rgba32Float);
        let bytes:Vec<_>=(0..PAGE_SIZE*PAGE_SIZE).flat_map(|_|[0.25f32,0.5,0.75,1.]).flat_map(f32::to_le_bytes).collect();
        upload_page(renderer,&texture,&bytes);
        for _ in 0..footprint {
            let (source,coordinate)=next_build(&mut images).unwrap();
            assert!(!crate::page_rect(coordinate).intersect(halo).is_empty());
            images.write_tile(renderer,encoder,&source,coordinate,&texture,None).unwrap();
        }
        assert_eq!(images.tile_builds,footprint as u64);assert_eq!(images.completed_images,0);
        assert_eq!(images.entries[0].pending.len(),pending);
        assert!(images.entries.iter().all(|entry|!entry.ready()));
    });renderer.wait_idle().unwrap();
    renderer.moving_images=images;
    let _store=crate::test_support::GatedStore::new(&layer_core::raster_storage::RetainedTiles::from_sources([source.as_ref()]),false);
    crate::scene::scale::assert_regional_object_overview(&mut renderer,&document,owner,id);
}

#[test]
fn regional_root_inputs_are_ready_before_unrelated_source_chunks() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let bounds=PixelRect::new(0,0,1191,853);
    let request=request([9728,6400],bounds,31);
    let mut images=MovingImages::within(&r.scene_tiers);
    pollster::block_on(r.compilation.prepare_all(images.pipelines(&r.device))).unwrap();
    let source=submit_encoded(&mut r,|r,encoder| {
        let plan=images.plan(r,std::slice::from_ref(&request),32<<20,encoder).unwrap();
        images.allocate(r,plan,encoder);
        let texture=page_texture(r,wgpu::TextureFormat::Rgba32Float);
        let bytes:Vec<_>=(0..PAGE_SIZE*PAGE_SIZE).flat_map(|_|[0.25f32,0.5,0.75,1.]).flat_map(f32::to_le_bytes).collect();
        upload_page(r,&texture,&bytes);
        for _ in 0..20 {
            let (source,coordinate)=next_build(&mut images).unwrap();
            assert!(!crate::page_rect(coordinate).intersect(bounds.expand(64,request.source.extent)).is_empty());
            images.write_tile(r,encoder,&source,coordinate,&texture,None).unwrap();
        }
        assert_eq!(images.tile_builds,20);
        let root=images.entries.iter().find(|entry|entry.root).unwrap();
        assert_eq!(root.image.plan.level,6);assert_eq!(root.pending.len(),930);assert!(!root.ready());
        let source=images.lookup(request.id,&request.source,6,bounds,request.format,r.device.working_space())
            .expect("twenty committed regional inputs must publish without the other930chunks");
        assert_eq!(source.level,6);
        assert!(images.lookup(request.id,&request.source,6,PixelRect::new(1200,0,1280,853),request.format,r.device.working_space()).is_none(),
            "an uncommitted sampling-halo tile must still defer the region");
        source
    });complete(&r);
    let actual=crate::test_support::float_pixels(&r,source.view.texture());
    let width=source.view.texture().width() as usize;
    for y in 0..13 {for x in 0..18 {
        assert_eq!(actual[y*width+x],[0.25,0.5,0.75,1.]);
    }}
}

#[test]
fn panning_keeps_allocations_inside_the_view_margin() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut request=request([8192;2],PixelRect::new(2048,2048,3072,2816),31);
    let mut images=MovingImages::within(&r.scene_tiers);
    let budget=MovingImages::demand(std::slice::from_ref(&request),[1024,768]).unwrap();
    submit_encoded(&mut r,|r,encoder| {
        let plan=images.plan(r,std::slice::from_ref(&request),budget,encoder).unwrap();
        images.allocate(r,plan,encoder);drain(r,&mut images,encoder);
    });complete(&r);
    request.bounds=PixelRect::new(2304,2048,3328,2816);
    submit_encoded(&mut r,|r,encoder| {
        let plan=images.plan(r,std::slice::from_ref(&request),budget,encoder).unwrap();
        assert_eq!(plan.targets.len(),1);
        let allocation=plan.targets[0].plan;
        assert_eq!(allocation.level,request.level);
        assert!(allocation.bounds.width()<=request.bounds.width()*2+2*PAGE_SIZE);
        assert!(allocation.bounds.height()<=request.bounds.height()*2+2*PAGE_SIZE);
        images.allocate(r,plan,encoder);drain(r,&mut images,encoder);
    });complete(&r);
    for offset in [32,128,256] {
        request.bounds=PixelRect::new(2304+offset,2048,3328+offset,2816);
        submit_encoded(&mut r,|r,encoder| {
            let plan=images.plan(r,std::slice::from_ref(&request),budget,encoder).unwrap();
            assert!(plan.targets.is_empty(),"pan {offset} must reuse its allocation");
            assert!(images.lookup(request.id,&request.source,request.level,request.bounds,request.format,r.device.working_space()).is_some());
            assert!(images.storage_bytes()<=budget);
        });complete(&r);
    }
}

#[test]
fn view_windows_build_and_evict_by_stack_rank() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let requests=[MovingRequest {rank:1,..request([2048;2],PixelRect::new(256,256,512,512),31)},
        MovingRequest {rank:28,..request([2048;2],PixelRect::new(1536,1536,1792,1792),73)}];
    let mut images=MovingImages::within(&r.scene_tiers);
    submit_encoded(&mut r,|r,encoder| {
        let plan=images.plan(r,&requests,32<<20,encoder).unwrap();
        images.allocate(r,plan,encoder);
        assert!(Arc::ptr_eq(&next_build(&mut images).unwrap().0,&requests[1].source));
        assert_eq!(images.wanted()[0].0,requests[1].id);
        drain(r,&mut images,encoder);
    });complete(&r);
    let low=images.entries.iter().find(|entry|entry.id==requests[0].id).unwrap().storage_bytes();
    submit_encoded(&mut r,|_,encoder| {
        let limit=images.storage_bytes()-low;trim(&mut images,limit,encoder);
        assert!(images.entries.iter().all(|entry|entry.id==requests[1].id));
    });complete(&r);
}

#[test]
fn one_twenty_eight_and_one_hundred_mip_windows_share_one_gpu_bound_and_workspace() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let bound=working_bytes()+2*4*2560u64*1600*8;
    for count in [1,28,100] {
        let requests:Vec<_>=(0..count).map(|index|request([4096,3072],PixelRect::new(1536,1024,2048,1536),index)).collect();
        let mut images=MovingImages::within(&r.scene_tiers);
        submit_encoded(&mut r,|r,encoder| {
            let plan=images.plan(r,&requests,bound,encoder).unwrap();
            assert_eq!(plan.targets.iter().filter(|target|!target.root).count(),count as usize);
            assert_eq!(plan.targets.iter().filter(|target|target.root).count(),count as usize);
            images.allocate(r,plan,encoder);
            assert_eq!(images.entries.len(),2*count as usize);
            assert!(images.entries.iter().all(|entry|entry.pending.len()==crate::page_coordinates(entry.image.plan.bounds).count()));
            assert!(images.storage_bytes()<=bound);
            drain(r,&mut images,encoder);
            assert!(images.entries.iter().all(Entry::ready));
            assert_eq!(images.completed_images,u64::from(count));
            assert!(images.storage_bytes()<=bound);
            assert!(images.pinned_bytes()<images.storage_bytes());
        });
        complete(&r);
        submit_encoded(&mut r,|_,encoder| {
            assert!(trim(&mut images,0,encoder)>0);
            assert!(images.entries.is_empty());
            assert!(images.retiring.used()!=0);
            assert!(images.pinned_bytes()>0);
            assert!(images.storage_bytes()<=bound);
        });
        complete(&r);
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        trim(&mut images,0,&mut encoder);
        assert_eq!(images.storage_bytes(),0);
        assert!(!images.pending());
    }
}

#[test]
fn image_window_pixels_are_local_and_never_claim_missing_source_regions() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    for format in [wgpu::TextureFormat::Rgba16Float,wgpu::TextureFormat::Rgba32Float] {
        let bounds=PixelRect::new(2048,1024,2560,1536);
        let request=MovingRequest {format,..request([4096,3072],bounds,31)};
        let mut images=MovingImages::within(&r.scene_tiers);
        pollster::block_on(r.compilation.prepare_all(images.pipelines(&r.device))).unwrap();
        submit_encoded(&mut r,|r,encoder| {
            let plan=images.plan(r,std::slice::from_ref(&request),32<<20,encoder).unwrap();
            images.allocate(r,plan,encoder);
            assert_eq!(images.entries[0].image.plan.size,[512;2]);
            let upcoming:Vec<_>=images.wanted().into_iter().take(16).collect();
            assert_eq!(upcoming.len(),16);
            assert!(upcoming.iter().all(|(_,_,coordinate,_,_)|images.entries[0].image.plan.bounds.intersect(crate::page_rect(*coordinate)).area()>0));
            let texture=page_texture(r,wgpu::TextureFormat::Rgba32Float);
            let bytes:Vec<_>=(0..PAGE_SIZE*PAGE_SIZE).flat_map(|_|[0.25f32,0.5,0.75,1.]).flat_map(f32::to_le_bytes).collect();
            upload_page(r,&texture,&bytes);
            for _ in 0..upcoming.len() {
                let (source,coordinate)=next_build(&mut images).unwrap();
                images.write_tile(r,encoder,&source,coordinate,&texture,None).unwrap();
            }
        });
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
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let extent=[257,259];
    let request=request(extent,PixelRect::full(extent),31);
    let mut images=MovingImages::within(&r.scene_tiers);
    pollster::block_on(r.compilation.prepare_all(images.pipelines(&r.device))).unwrap();
    submit_encoded(&mut r,|r,encoder| {
        let plan=images.plan(r,std::slice::from_ref(&request),16<<20,encoder).unwrap();
        images.allocate(r,plan,encoder);
        while let Some((source,coordinate))=next_build(&mut images) {
            let texture=page_texture(r,wgpu::TextureFormat::Rgba32Float);
            let bytes:Vec<_>=(0..PAGE_SIZE).flat_map(|y|(0..PAGE_SIZE).map(move|x|[x,y]))
                .flat_map(|local|if (0..2).all(|axis|coordinate[axis]*PAGE_SIZE+local[axis]<extent[axis]) {[0.25f32,0.5,0.75,1.]}else{[0.;4]})
                .flat_map(f32::to_le_bytes).collect();
            upload_page(r,&texture,&bytes);
            images.write_tile(r,encoder,&source,coordinate,&texture,None).unwrap();
        }
    });
    let image=&images.entries[0].image;
    let bytes=crate::layer_tests::page_bytes(&r,&image.texture);
    let pixel=(129*image.texture.width() as usize+128)*8;
    let values:Vec<_>=bytes[pixel..pixel+8].chunks_exact(2)
        .map(|bytes|layer_core::color::f16::from_bits(u16::from_le_bytes(bytes.try_into().unwrap())).to_f32()).collect();
    assert_eq!(values,[0.0625,0.125,0.1875,0.25]);
}

#[test]
fn panning_reuses_overlapping_pages_and_holds_the_previous_window_until_ready() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let initial=request([2048;2],PixelRect::new(256,256,768,768),31);
    let mut images=MovingImages::within(&r.scene_tiers);
    submit_encoded(&mut r,|r,encoder| {
        let plan=images.plan(r,std::slice::from_ref(&initial),16<<20,encoder).unwrap();
        images.allocate(r,plan,encoder);
        drain(r,&mut images,encoder);
    });
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
    drain(&mut r,&mut images,&mut encoder);
    let plan=images.plan(&r,std::slice::from_ref(&shifted),16<<20,&mut encoder).unwrap();
    assert!(plan.targets.is_empty());
    assert_eq!(images.entries.len(),2);
    assert_eq!(images.evictions,1);
    assert!(images.lookup(initial.id,&initial.source,1,shifted.bounds,initial.format,r.device.working_space()).is_some());
    encoder.submit(&r.queue);
    complete(&r);
}

#[test]
fn mip_windows_share_sources_and_invalidate_on_format_context_or_abandoned_submission() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let request=request([512;2],PixelRect::new(0,0,256,256),31);
    let mut images=MovingImages::within(&r.scene_tiers);
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    let plan=images.plan(&r,&[request.clone(),request.clone()],16<<20,&mut encoder).unwrap();
    assert_eq!(plan.targets.len(),2);
    images.allocate(&r,plan,&mut encoder);
    drain(&mut r,&mut images,&mut encoder);
    drop(encoder);
    assert!(images.pending());
    assert!(next_build(&mut images).is_some());
    assert_eq!(images.entries[0].pending.len(),4);
    submit_encoded(&mut r,|r,encoder|{drain(r,&mut images,encoder);});
    complete(&r);
    submit_encoded(&mut r,|r,encoder| {
        let reused=images.plan(r,std::slice::from_ref(&request),16<<20,encoder).unwrap();
        assert!(reused.targets.is_empty());
        let changed=MovingRequest {format:wgpu::TextureFormat::Rgba32Float,..request};
        let replaced=images.plan(r,&[changed],16<<20,encoder).unwrap();
        assert_eq!(replaced.targets.len(),2);
        assert_eq!(images.evictions,2);
        assert!(images.pinned_bytes()>0);
        images.allocate(r,replaced,encoder);
        drain(r,&mut images,encoder);
    });
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
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let initial=request([4096;2],PixelRect::new(256,256,2304,2304),31);
    let target=MovingRequest {bounds:PixelRect::new(3840,1024,4096,1280),..initial.clone()};
    let mut images=MovingImages::within(&r.scene_tiers);
    submit_encoded(&mut r,|r,encoder| {
        let plan=images.plan(r,std::slice::from_ref(&initial),64<<20,encoder).unwrap();
        images.allocate(r,plan,encoder);drain(r,&mut images,encoder);
    });complete(&r);
    let budget=working_bytes()+(1<<20);
    submit_encoded(&mut r,|r,encoder| {
        let plan=images.plan(r,std::slice::from_ref(&target),budget,encoder).unwrap();
        assert!(plan.waiting);assert!(images.retiring.used()!=0);
    });complete(&r);
    submit_encoded(&mut r,|r,encoder| {
        let plan=images.plan(r,std::slice::from_ref(&target),budget,encoder).unwrap();
        assert!(!plan.waiting);assert!(!plan.targets.is_empty());
        images.allocate(r,plan,encoder);drain(r,&mut images,encoder);
    });complete(&r);
    assert!(images.lookup(target.id,&target.source,1,target.bounds,target.format,r.device.working_space()).is_some());
    assert!(images.storage_bytes()<=budget);
    assert_eq!(MovingImages::demand(&[initial],[2560,1600]).unwrap(),MovingImages::demand(&[target],[2560,1600]).unwrap());
}

#[test]
fn retargeting_unfinished_windows_preserves_every_completed_overlap_page() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut request=request([4096;2],PixelRect::new(512,512,1024,1024),31);
    let mut images=MovingImages::within(&r.scene_tiers);
    pollster::block_on(r.compilation.prepare_all(images.pipelines(&r.device))).unwrap();let mut completed=std::collections::BTreeSet::new();
    let texture=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
    upload_page(&r,&texture,&vec![0;PAGE_SIZE as usize*PAGE_SIZE as usize*16]);
    for step in 0..6 {
        request.bounds=PixelRect::new(512+step*256,512,1024+step*256,1024);
        submit_encoded(&mut r,|r,encoder| {
            let plan=images.plan(r,std::slice::from_ref(&request),32<<20,encoder).unwrap();
            images.allocate(r,plan,encoder);
            let entry=images.entries.last().unwrap();
            for coordinate in &completed {
                if entry.image.plan.bounds.intersect(crate::page_rect(*coordinate)).area()>0 {
                    assert!(!entry.pending.contains(coordinate),"completed {coordinate:?} was discarded on step {step}");
                }
            }
            let (source,coordinate)=next_build(&mut images).unwrap();
            assert!(completed.insert(coordinate));
            images.write_tile(r,encoder,&source,coordinate,&texture,None).unwrap();
        });complete(&r);
    }
    submit_encoded(&mut r,|r,encoder|{drain(r,&mut images,encoder);});complete(&r);
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
            assert_eq!(r.memory.mip_budget(),bound);
            let mut images=MovingImages::within(&r.scene_tiers);
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            let plan=images.plan(&r,&requests,bound,&mut encoder).unwrap();
            assert!(plan.targets.iter().any(|target|!target.root && target.request.id==selected && target.plan.level==selected_request.level),"{device}: top image must keep its requested level");
            images.allocate(&r,plan,&mut encoder);drain(&mut r,&mut images,&mut encoder);
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
    let mut r=crate::memory::memory_tests::renderer_in_budget(Default::default(),layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<40),..Default::default()},budget).unwrap();
    let first=request([2048;2],PixelRect::new(256,256,512,512),31);
    let requests=[first.clone(),MovingRequest {bounds:PixelRect::new(1536,1536,1792,1792),..first}];
    r.memory.mip_demand(32<<20,[2560,1600]);r.publish_memory();r.factory.memory_budget().flush_reports();
    let mut images=MovingImages::within(&r.scene_tiers);
    pollster::block_on(r.compilation.prepare_all(images.pipelines(&r.device))).unwrap();submit_encoded(&mut r,|r,encoder| {
        let plan=images.plan(r,&requests,32<<20,encoder).unwrap();
        images.allocate(r,plan,encoder);
        let texture=page_texture(r,wgpu::TextureFormat::Rgba32Float);
        upload_page(r,&texture,&vec![0;PAGE_SIZE as usize*PAGE_SIZE as usize*16]);
        while images.entries.iter().any(|entry|!entry.root && !entry.ready()) {
            let (source,coordinate,_)=images.next_build(|class|class==Class::Visible,|_,_,_|true).unwrap();
            images.write_tile(r,encoder,&source,coordinate,&texture,None).unwrap();
        }
        assert_eq!(images.entries.iter().filter(|entry|!entry.root && entry.ready()).count(),2);
        assert!(images.entries.iter().any(|entry|entry.root && !entry.ready()));
        assert_eq!(trim(&mut images,32<<20,encoder),0);
        assert_eq!(images.entries.iter().filter(|entry|!entry.root && entry.ready()).count(),2);
        assert_eq!(images.retiring.used(),0);
        drain(r,&mut images,encoder);
        let roots=images.entries.iter().filter(|entry|entry.root).map(Entry::storage_bytes).sum::<u64>();
        assert!(roots>0);
        let limit=images.storage_bytes()-roots;assert_eq!(trim(&mut images,limit,encoder),roots);
        assert_eq!(images.entries.iter().filter(|entry|!entry.root && entry.ready()).count(),2);
        assert!(images.entries.iter().all(|entry|!entry.root));
    });complete(&r);
}

#[test]
fn overview_inputs_and_prefetch_use_the_same_build_order() {
    let budget=layer_core::MemoryBudget::with_facts(layer_core::MemoryFacts {host_default_bytes:Some(1<<40),override_bytes:Some(1<<40),..Default::default()});
    let mut r=crate::memory::memory_tests::renderer_in_budget(Default::default(),layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<40),..Default::default()},budget).unwrap();
    let requests=[request([2048;2],PixelRect::new(256,256,512,512),31),request([2048;2],PixelRect::new(1536,1536,1792,1792),73)];
    r.memory.mip_demand(32<<20,[2560,1600]);r.publish_memory();r.factory.memory_budget().flush_reports();
    let mut images=MovingImages::within(&r.scene_tiers);
    pollster::block_on(r.compilation.prepare_all(images.pipelines(&r.device))).unwrap();submit_encoded(&mut r,|r,encoder| {
        let plan=images.plan(r,&requests,32<<20,encoder).unwrap();
        images.allocate(r,plan,encoder);
        let roots=images.entries.iter().filter(|entry|entry.root).map(|entry|entry.pending.len()).sum::<usize>();
        let upcoming=images.wanted();
        assert!(upcoming.len()>roots);
        for (id,_,coordinate,_,_) in &upcoming[..roots] {
            assert!(images.entries.iter().any(|entry|entry.root && entry.id==*id && entry.pending.contains(coordinate)));
        }
        let texture=page_texture(r,wgpu::TextureFormat::Rgba32Float);
        upload_page(r,&texture,&vec![0;PAGE_SIZE as usize*PAGE_SIZE as usize*16]);
        while let Some((source,coordinate))=next_build(&mut images) {
            if images.entries.iter().any(|entry|entry.root && !entry.ready()) {
                assert!(images.entries.iter().any(|entry|entry.root && entry.wants(&source,coordinate)));
            }
            images.write_tile(r,encoder,&source,coordinate,&texture,None).unwrap();
            assert!(!images.entries.iter().any(|entry|entry.wants(&source,coordinate)));
        }
        assert!(images.entries.iter().all(Entry::ready));
    });complete(&r);
}

#[test]
fn offscreen_nearest_images_do_not_request_unused_storage() {
    let request=MovingRequest {nearest:true,level:0,bounds:PixelRect::EMPTY,..moving_request()};
    assert_eq!(MovingImages::demand(&[request],[2560,1600]).unwrap(),0);
}

#[test]
fn level_zero_fallback_roots_keep_their_working_pixel_format() {
    use layer_core::color::{RgbSpace,SampleDepth};
    for depth in [SampleDepth::U8,SampleDepth::U16] {
        for format in [wgpu::TextureFormat::Rgba16Float,wgpu::TextureFormat::Rgba32Float] {
            let extent=[17,19];
            let source=crate::test_support::depth_source(extent,depth,RgbSpace::Srgb,8<<20,|_,_|[0.25,0.5,0.75,1.]);
            let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
            let request=MovingRequest {source:source.clone(),format,bounds:PixelRect::full(extent),..moving_request()};
            let mut images=MovingImages::within(&r.scene_tiers);
        pollster::block_on(r.compilation.prepare_all([r.scene_pipelines.source.for_format(if depth==SampleDepth::U8 {wgpu::TextureFormat::Rgba8Unorm}else{wgpu::TextureFormat::Rg32Uint})])).unwrap();
        pollster::block_on(r.compilation.prepare_all(images.pipelines(&r.device))).unwrap();
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            let plan=images.plan(&r,std::slice::from_ref(&request),64<<20,&mut encoder).unwrap();
            assert!(plan.targets.iter().any(|target|target.root && target.plan.level==0));
            images.allocate(&r,plan,&mut encoder);
            let pixels=prepare_image_tile(&source,[0;2],r.device.working_space(),&r.device.source_samples,None).unwrap();
            let tile=crate::scene::Scene::decode_prepared(&mut r,&mut encoder,&source,[0;2],pixels.clone()).unwrap();
            images.write_tile(&mut r,&mut encoder,&source,[0;2],&tile.texture,Some(&pixels)).unwrap();
            r.uploads.finish(&encoder);encoder.submit(&r.queue);complete(&r);
            assert!(images.entries.iter().all(Entry::ready));
            let root=images.lookup(request.id,&source,0,PixelRect::full(extent),format,r.device.working_space()).unwrap();
            assert_eq!(root.view.texture().format(),format);
            assert!(root.samples.is_none(),"working pixels cannot be interpreted as stored codes");
            let bytes=crate::layer_tests::page_bytes(&r,root.view.texture());
            let values:Vec<f32>=if format==wgpu::TextureFormat::Rgba16Float {
                bytes.as_chunks::<2>().0.iter().map(|bytes|layer_core::color::f16::from_bits(u16::from_le_bytes(*bytes)).to_f32()).collect()
            } else {bytes.as_chunks::<4>().0.iter().map(|bytes|f32::from_le_bytes(*bytes)).collect()};
            let expected=[0.25,0.5,0.75].map(|value|(value*depth.maximum() as f64).round()/depth.maximum() as f64);
            for y in 0..PAGE_SIZE {for x in 0..PAGE_SIZE {
                let pixel=&values[((y*PAGE_SIZE+x)*4) as usize..][..4];
                if x<extent[0] && y<extent[1] {
                    assert_eq!(pixel[3],1.);
                    for channel in 0..3 {
                        let actual=(RgbSpace::Srgb.encode(f64::from(pixel[channel])).clamp(0.,1.)*255.).round();
                        assert!((actual-(expected[channel]*255.).round()).abs()<=1.,"{depth:?} {format:?}: {pixel:?}");
                    }
                } else {assert_eq!(pixel,[0.;4]);}
            }}
        }
    }
}

#[test]
fn nearest_native_windows_keep_partial_rgb_and_gray_padding_transparent() {
    use layer_core::color::{ColorProfile,SampleDepth,source::{SourceBuilder,SourceChannels,SourceInterpretation}};
    let extent=[257,259];
    let size=[512;2];
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut failures=Vec::new();
    for depth in [SampleDepth::U8,SampleDepth::U16] {for channels in [SourceChannels::Rgb,SourceChannels::Gray] {
        let mut builder=SourceBuilder::new(extent,SourceInterpretation {channels,depth,
            profile:ColorProfile::Builtin(RgbSpace::Srgb),profile_assumed:false},8<<20).unwrap();
        let pixel:Vec<_>=(0..channels.count()).flat_map(|channel| {
            let code=64+channel as u16*64;
            if depth==SampleDepth::U8 {vec![code as u8]}else{(code*257).to_le_bytes().to_vec()}
        }).collect();
        let row=pixel.repeat(extent[0] as usize);
        for _ in 0..extent[1] {builder.push_row(&row).unwrap();}
        let source=Arc::new(builder.finish().unwrap());
        let request=MovingRequest {source:source.clone(),nearest:true,level:0,bounds:PixelRect::full(extent),
            format:MovingImages::source_format(&source),..moving_request()};
        let mut images=MovingImages::within(&r.scene_tiers);
        pollster::block_on(r.compilation.prepare_all(images.pipelines(&r.device))).unwrap();
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let plan=images.plan(&r,std::slice::from_ref(&request),64<<20,&mut encoder).unwrap();
        assert!(plan.targets.iter().all(|target|target.plan.level==0));
        images.allocate(&r,plan,&mut encoder);
        while let Some((source,coordinate))=next_build(&mut images) {
            let pixels=prepare_image_tile(&source,coordinate,r.device.working_space(),&r.device.source_samples,None).unwrap();
            let tile=crate::scene::Scene::decode_prepared(&mut r,&mut encoder,&source,coordinate,pixels.clone()).unwrap();
            assert_eq!(tile.texture.format(),request.format);
            images.write_tile(&mut r,&mut encoder,&source,coordinate,&tile.texture,Some(&pixels)).unwrap();
        }
        let mip=images.lookup(request.id,&source,0,request.bounds,request.format,r.device.working_space()).unwrap();
        assert_eq!([mip.view.texture().width(),mip.view.texture().height()],size);
        assert_eq!(mip.origin,[0;2]);
        let window_extent=mip.extent;
        let (output,view)=crate::create_color_target(&r.device,size,"partial opaque source preview");
        pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.objects.preview,&r.scene_pipelines.objects.preview_integer])).unwrap();
        r.scene_pipelines.objects.clone().preview(&r.device,&mut encoder,&mut r.uploads,&crate::object_sampling::CollectionPreview {
            sources:vec![mip.view],objects:vec![crate::object_sampling::PreviewSource {source:0,inverse:[1.,0.,0.,1.,-0.25,-0.25],
                extent:window_extent,nearest:true,samples:mip.samples}],size,output:view,scratch:None,encode:false,
        },&r.empty_view).unwrap();
        r.uploads.finish(&encoder);encoder.submit(&r.queue);complete(&r);
        let pixels=crate::test_support::float_pixels(&r,&output);
        for (index,pixel) in pixels.iter().enumerate() {
            let [x,y]=[index as u32%size[0],index as u32/size[0]];
            let inside=[x,y].into_iter().zip(extent).all(|(position,side)| {
                let center=f64::from(position)+0.5;
                center>=0.25 && center<f64::from(side)+0.25
            });
            let expected=if inside {1.}else{0.};
            if pixel[3]!=expected {
                failures.push(format!("{depth:?} {channels:?}: pixel [{x},{y}]={pixel:?}, expected alpha={expected}, source={extent:?}, window={window_extent:?}"));
                break;
            }
        }
    }}
    assert!(failures.is_empty(),"{}",failures.join("\n"));
}

#[test]
fn nearest_windows_retain_codes_and_keep_their_identity_when_the_source_promotes() {
    use layer_core::color::{ColorProfile,SampleDepth,source::{SourceBuilder,SourceChannels,SourceInterpretation}};
    for depth in [SampleDepth::U8,SampleDepth::U16] {
        let mut builder=SourceBuilder::new([PAGE_SIZE;2],SourceInterpretation {channels:SourceChannels::Rgba,depth,
            profile:ColorProfile::Builtin(RgbSpace::Srgb),profile_assumed:false},8<<20).unwrap();
        let pixel=if depth==SampleDepth::U8 {vec![64,128,192,255]}else{[16384u16,32768,49152,65535].into_iter().flat_map(u16::to_le_bytes).collect()};
        let row=pixel.repeat(PAGE_SIZE as usize);
        for _ in 0..PAGE_SIZE {builder.push_row(&row).unwrap();}
        let source=Arc::new(builder.finish().unwrap());
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let request=MovingRequest {source:source.clone(),nearest:true,level:0,bounds:PixelRect::full(source.extent),
            format:MovingImages::source_format(&source),..moving_request()};
        let mut images=MovingImages::within(&r.scene_tiers);
        pollster::block_on(r.compilation.prepare_all(images.pipelines(&r.device))).unwrap();
        let (pixels,tile)=submit_encoded(&mut r,|r,encoder| {
            let plan=images.plan(r,std::slice::from_ref(&request),64<<20,encoder).unwrap();
            images.allocate(r,plan,encoder);
            let pixels=crate::prepare_image_tile(&source,[0;2],r.device.working_space(),&r.device.source_samples,None).unwrap();
            let tile=crate::scene::Scene::decode_prepared(r,encoder,&source,[0;2],pixels.clone()).unwrap();
            images.write_tile(r,encoder,&source,[0;2],&tile.texture,Some(&pixels)).unwrap();
            (pixels,tile)
        });complete(&r);
        let before=images.lookup(request.id,&source,0,request.bounds,request.format,r.device.working_space()).unwrap();
        assert_eq!(crate::texture_bytes(before.view.texture()),u64::from(PAGE_SIZE).pow(2)*if depth==SampleDepth::U8 {4}else{8});
        let expected=match &pixels {crate::scene::sources::PreparedSourcePixels::NativeSamples(bytes)=>bytes,_=>unreachable!()};
        assert_eq!(crate::layer_tests::page_bytes(&r,before.view.texture()),**expected);
        let promoted=submit_encoded(&mut r,|r,encoder| {
            pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.source.pipeline,&r.scene_pipelines.source.integer])).unwrap();
            let promoted=crate::scene::Scene::promote_source(r,encoder,tile).unwrap();
            assert_eq!(promoted.texture.format(),wgpu::TextureFormat::Rgba32Float);
            promoted
        });complete(&r);
        let builds=images.tile_builds;
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let plan=images.plan(&r,std::slice::from_ref(&request),64<<20,&mut encoder).unwrap();
        assert!(plan.targets.is_empty());
        let after=images.lookup(request.id,&source,0,request.bounds,request.format,r.device.working_space()).unwrap();
        assert_eq!(before.view,after.view);assert_eq!(images.tile_builds,builds);
        let output=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
        pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.objects.preview,&r.scene_pipelines.objects.preview_integer])).unwrap();
        r.scene_pipelines.objects.clone().preview(&r.device,&mut encoder,&mut r.uploads,&crate::object_sampling::CollectionPreview {
            sources:vec![after.view],objects:vec![crate::object_sampling::PreviewSource {source:0,inverse:[1.,0.,0.,1.,0.,0.],extent:[PAGE_SIZE;2],nearest:true,samples:after.samples}],
            size:[PAGE_SIZE;2],output:output.create_view(&Default::default()),scratch:None,encode:false,
        },&r.empty_view).unwrap();
        r.uploads.finish(&encoder);encoder.submit(&r.queue);complete(&r);
        let actual=crate::test_support::float_pixels(&r,&output);
        let exact=crate::test_support::float_pixels(&r,&promoted.texture);
        for (actual,exact) in actual.into_iter().zip(exact) {for channel in 0..4 {assert!((actual[channel]-exact[channel]).abs()<=1./255.);}}
    }
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
        assert_eq!(requests.len(),count as usize);assert_eq!(r.memory.mip_budget(),bound);
        let mut images=MovingImages::within(&r.scene_tiers);
        pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.source.pipeline])).unwrap();
        pollster::block_on(r.compilation.prepare_all(images.pipelines(&r.device))).unwrap();let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let plan=images.plan(&r,&requests,bound,&mut encoder).unwrap();
        assert_eq!(plan.targets.iter().filter(|target|target.root).count(),count as usize);
        images.allocate(&r,plan,&mut encoder);
        assert!(editor.retained_tiles().resident_bytes()==0);
        let mut count=0;
        while let Some((source,coordinate))=next_build(&mut images) {
            let pixels=prepare_image_tile(&source,coordinate,r.device.working_space(),&r.device.source_samples,None).unwrap();
            let tile=crate::scene::Scene::decode_prepared(&mut r,&mut encoder,&source,coordinate,pixels).unwrap();
            images.write_tile(&mut r,&mut encoder,&source,coordinate,&tile.texture,None).unwrap();count+=1;
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
    let mut r=crate::memory::memory_tests::renderer_in_budget(Default::default(),layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<40),..Default::default()},budget).unwrap();
    let initial=request([4096;2],PixelRect::new(1024,1024,1536,1536),31);
    let requests=[initial.clone(),MovingRequest {level:3,..initial.clone()},MovingRequest {level:4,bounds:PixelRect::new(2560,2560,3072,3072),..initial.clone()},
        MovingRequest {bounds:PixelRect::EMPTY,..request([512;2],PixelRect::EMPTY,17)}];
    r.memory.mip_demand(64<<20,[2560,1600]);r.publish_memory();r.factory.memory_budget().flush_reports();
    let mut images=MovingImages::within(&r.scene_tiers);
    submit_encoded(&mut r,|r,encoder| {
        let plan=images.plan(r,&requests,64<<20,encoder).unwrap();
        assert_eq!(plan.targets.iter().filter(|target|!target.root).count(),3);
        images.allocate(r,plan,encoder);drain(r,&mut images,encoder);
    });complete(&r);
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
    submit_encoded(&mut r,|r,encoder| {
        for coordinate in crate::page_coordinates(bounds) {
            let pixels=prepare_image_tile(&projected.source,coordinate,r.device.working_space(),&r.device.source_samples,None).unwrap();
            crate::scene::Scene::decode_prepared(r,encoder,&projected.source,coordinate,pixels).unwrap();
        }
    });complete(&r);
    submit_encoded(&mut r,|r,encoder| {
        pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.source.pipeline,&r.scene_pipelines.source.integer])).unwrap();
        pollster::block_on(r.compilation.prepare_all(r.moving_images.pipelines(&r.device))).unwrap();
        let changed=r.prepare_moving_images(frame,encoder,&mut crate::test_support::work(false)).unwrap();
        assert!(r.moving_images.lookup(projected.id,&projected.source,projected.level,projected.bounds,projected.format,r.device.working_space()).is_some());
        assert!(r.moving_images.pending());assert_eq!(r.moving_images.completed_images,0);
        assert!(changed,"a newly ready view window must invalidate its presentation while the root is pending");
    });complete(&r);
}

#[test]
#[cfg(not(target_arch="wasm32"))]
fn a_tile_decoded_for_another_consumer_is_written_into_the_mip_windows_waiting_for_it_in_the_same_drain() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let request=request([1024;2],PixelRect::new(256,256,512,512),31);
    let targets=vec![request.allocation(&r.device,request.level,false).unwrap().unwrap(),request.root(&r.device).unwrap().unwrap()];
    let mut images=MovingImages::within(&r.scene_tiers);
    pollster::block_on(r.compilation.prepare_all(images.pipelines(&r.device))).unwrap();
    pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.source.pipeline,&r.scene_pipelines.source.integer])).unwrap();
    submit_encoded(&mut r,|r,encoder| {
        images.allocate(r,MipPlan {targets,waiting:false},encoder);
        let last=*images.entries.iter().find(|entry|entry.root).unwrap().pending.back().unwrap();
        r.moving_images=images;
        assert!(r.request_image_decode(request.id,request.source.clone(),last,Class::Visible).unwrap());
        wait_until("the requested tile arrives",|| {
            if r.source_tiles.borrow().prepared_view(&request.source,last).is_some() {return Ok(());}
            r.drain_image_decodes(encoder).unwrap();
            Err(())
        });
        assert!(!r.moving_images.entries.iter().any(|entry|entry.pending.contains(&last)),
            "the drain that uploads a tile writes it into every window waiting for it, so a later pass never decodes it again");
        assert_eq!(r.source_tiles.borrow().misses,1);
    });complete(&r);
}

#[test]
#[cfg(not(target_arch="wasm32"))]
fn window_samples_whose_upload_is_refused_wait_for_room_instead_of_being_dropped() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let base=MovingRequest {nearest:true,level:0,format:wgpu::TextureFormat::Rgba8Unorm,..request([PAGE_SIZE;2],PixelRect::full([PAGE_SIZE;2]),31)};
    let targets=(0..4).map(|_|MovingRequest {id:PortableId::random(),..base.clone()}.allocation(&r.device,0,false).unwrap().unwrap()).collect();
    let mut images=MovingImages::within(&r.scene_tiers);
    pollster::block_on(r.compilation.prepare_all(images.pipelines(&r.device))).unwrap();
    pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.source.pipeline,&r.scene_pipelines.source.integer])).unwrap();
    submit_encoded(&mut r,|r,encoder| {
        images.allocate(r,MipPlan {targets,waiting:false},encoder);
        assert_eq!(images.entries.len(),4);
    });complete(&r);
    r.moving_images=images;
    r.source_tiles.borrow_mut().admit_limits(crate::scene::sources::NORMALIZED_PAGE_BYTES);
    assert!(r.request_exact_image_decode(base.id,base.source.clone(),[0;2],Class::Visible).unwrap());
    submit_encoded(&mut r,|r,encoder| {
        wait_until("the requested tile arrives",|| {
            if r.source_tiles.borrow().prepared_view(&base.source,[0;2]).is_some() {return Ok(());}
            r.drain_image_decodes(encoder).unwrap();
            Err(())
        });
        assert!(r.moving_images.entries.iter().any(|entry|!entry.ready()),"four windows' samples exceed the upload room left after the display's upload");
    });complete(&r);
    submit_encoded(&mut r,|r,encoder| {
        r.drain_image_decodes(encoder).unwrap();
        assert!(r.moving_images.entries.iter().all(Entry::ready),"the held samples are written once the upload room frees, without another decode");
        assert_eq!(r.source_tiles.borrow().misses,1);
    });complete(&r);
}

#[test]
#[cfg(not(target_arch="wasm32"))]
fn a_root_window_queues_the_decodes_only_it_needs_at_its_own_class() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let requests=[request([2048;2],PixelRect::full([2048;2]),31)];
    let doc=photo_document(&requests,[2048;2]);
    let mut frame=crate::test_support::packet(doc.scene(),[2048;2]);
    frame.view=layer_render::ViewState {width_px:256,height_px:256,document_to_surface:[1.,0.,0.,1.,-512.,-512.]};
    r.prepare_moving_projection(frame).unwrap();r.begin_frame_memory(frame);
    let (_,results)=std::sync::mpsc::channel();
    let mut queue=queued(results,Default::default(),Vec::new(),64);
    queue.pool=Arc::new(DecodePool {dispatch:Default::default(),capacity:64,speculative:64*TILE_BYTES,epoch:Default::default(),capacity_waiters:Default::default()});
    r.moving_decode=Some((r.device.working_space(),queue));
    pollster::block_on(r.compilation.prepare_all(r.moving_images.pipelines(&r.device))).unwrap();
    submit_encoded(&mut r,|r,encoder| {
        r.prepare_moving_images(frame,encoder,&mut crate::test_support::work(false)).unwrap();
        let view=r.moving_images.entries.iter().find(|entry|!entry.root).unwrap().image.plan.bounds;
        let root=r.moving_images.entries.iter().find(|entry|entry.root).unwrap().image.plan.bounds;
        let queued:Vec<_>=r.moving_decode.as_ref().unwrap().1.outstanding.iter().map(|outstanding|match &outstanding.key {
            Some(DecodeKey::Image(_,coordinate))=>(*coordinate,outstanding.class),_=>unreachable!(),
        }).collect();
        assert_eq!(queued.len(),crate::page_coordinates(root).count(),"every tile the root needs is queued once");
        for (coordinate,class) in queued {
            let expected=if crate::page_rect(coordinate).intersect(view).is_empty() {Class::Navigator} else {Class::Visible};
            assert_eq!(class,expected,"tile {coordinate:?} queues at the class of the most urgent window that needs it");
        }
    });complete(&r);
}

fn promoted_boundary_window(r:&mut WgpuRasterizer,request:&MovingRequest,tiles:[&crate::source_access::TileTexture;2])->Vec<[f32;4]> {
    let mut images=MovingImages::within(&r.scene_tiers);
    pollster::block_on(r.compilation.prepare_all(images.pipelines(&r.device))).unwrap();
    submit_encoded(r,|r,encoder| {
        let mut plan=images.plan(r,std::slice::from_ref(request),8<<20,encoder).unwrap();
        plan.targets.retain(|target|!target.root);
        images.allocate(r,plan,encoder);
        assert_eq!(images.entries.len(),1);
        for (x,texture) in tiles.into_iter().enumerate() {
            images.write_tile(r,encoder,&request.source,[x as u32,0],texture,None).unwrap();
        }
    });complete(r);
    let window=images.lookup(request.id,&request.source,1,request.bounds,request.format,r.device.working_space()).unwrap();
    assert_eq!(window.level,1);
    assert_eq!([window.view.texture().width(),window.view.texture().height()],[PAGE_SIZE,PAGE_SIZE/2]);
    crate::layer_tests::page_bytes(r,window.view.texture()).as_chunks::<8>().0.iter().map(|pixel|
        std::array::from_fn(|c|layer_core::color::f16::from_bits(u16::from_le_bytes(pixel[c*2..c*2+2].try_into().unwrap())).to_f32())
    ).collect()
}

#[test]
fn promoted_p3_page_boundary_keeps_a_reduced_gradient_within_two_display_codes() {
    use layer_core::color::{ColorProfile,SampleDepth,source::{SourceBuilder,SourceChannels,SourceInterpretation}};
    let extent=[PAGE_SIZE*2,PAGE_SIZE];
    let mut builder=SourceBuilder::new(extent,SourceInterpretation {channels:SourceChannels::Rgba,depth:SampleDepth::U8,
        profile:ColorProfile::Builtin(RgbSpace::DisplayP3),profile_assumed:false},8<<20).unwrap();
    for y in 0..extent[1] {
        let row:Vec<_>=(0..extent[0]).flat_map(|x| {
            let q=((x+8)/16) as u8;
            let rgb=match (y/32)%4 {
                0=>[q/4,q/4+1,q/4+2],
                1=>[q*3,q*7,q*2],
                2=>[q*7,224-q*6,q*5],
                _=>[q*7,q*7,q*7],
            };
            [rgb[0],rgb[1],rgb[2],if y<128 {255}else if y<224 {128}else{0}]
        }).collect();
        assert_eq!(&row[(PAGE_SIZE as usize-2)*4..PAGE_SIZE as usize*4],
            &row[PAGE_SIZE as usize*4..(PAGE_SIZE as usize+2)*4]);
        builder.push_row(&row).unwrap();
    }
    let source=Arc::new(builder.finish().unwrap());
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    assert_eq!(r.device.working_space(),RgbSpace::Srgb);
    let request=MovingRequest {source:source.clone(),bounds:PixelRect::full(extent),..moving_request()};
    let (left,right)=submit_encoded(&mut r,|r,encoder| {
        let mut tiles=Vec::new();
        for x in 0..2 {
            let pixels=crate::prepare_image_tile(&source,[x,0],r.device.working_space(),&r.device.source_samples,None).unwrap();
            let tile=crate::scene::Scene::decode_prepared(r,encoder,&source,[x,0],pixels).unwrap();
            assert_eq!(tile.texture.format(),wgpu::TextureFormat::Rgba8Unorm);
            tiles.push(tile);
        }
        pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.source.pipeline,&r.scene_pipelines.source.integer])).unwrap();
        let left=crate::scene::Scene::promote_source(r,encoder,tiles.remove(0)).unwrap();
        let right=tiles.remove(0);
        (left,right)
    });complete(&r);
    assert_eq!(r.source_tiles.borrow().prepared_view(&source,[0,0]).unwrap().texture().format(),wgpu::TextureFormat::Rgba32Float);
    assert_eq!(r.source_tiles.borrow().prepared_view(&source,[1,0]).unwrap().texture().format(),wgpu::TextureFormat::Rgba8Unorm);
    let mixed=promoted_boundary_window(&mut r,&request,[&left.texture,&right.texture]);
    assert_eq!(r.source_tiles.borrow().prepared_view(&source,[1,0]).unwrap().texture().format(),wgpu::TextureFormat::Rgba8Unorm);
    let right=submit_encoded(&mut r,|r,encoder| {
        pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.source.pipeline,&r.scene_pipelines.source.integer])).unwrap();
        crate::scene::Scene::promote_source(r,encoder,right).unwrap()
    });complete(&r);
    let reference=promoted_boundary_window(&mut r,&request,[&left.texture,&right.texture]);
    let code=|pixel:[f32;4],channel:usize|if pixel[3]==0. {0i16}else {
        (RgbSpace::Srgb.encode(f64::from(pixel[channel]/pixel[3])).clamp(0.,1.)*255.).round() as i16
    };
    for (i,(&actual,&expected)) in mixed.iter().zip(&reference).enumerate() {
        assert_eq!(actual[3],expected[3],"alpha at reduced pixel {i}");
        for c in 0..3 {assert!((code(actual,c)-code(expected,c)).abs()<=2,"pixel {i}, channel {c}: {actual:?} versus {expected:?}");}
    }
    for y in 0..PAGE_SIZE as usize/2 {
        let [a,b]=[y*PAGE_SIZE as usize+PAGE_SIZE as usize/2-1,y*PAGE_SIZE as usize+PAGE_SIZE as usize/2];
        for c in 0..3 {
            assert_eq!(code(reference[a],c),code(reference[b],c),"reference slope at seam row {y}, channel {c}");
            assert!((code(mixed[a],c)-code(mixed[b],c)).abs()<=2,"promoted boundary step at row {y}, channel {c}");
        }
        assert_eq!(mixed[a][3],mixed[b][3]);
    }
}

#[test]
#[cfg(not(target_arch="wasm32"))]
fn a_drain_writes_window_samples_as_it_takes_them_so_none_wait_for_a_frame_or_delay_an_exact_query() {
    use crate::scene::sources::{PreparedSourcePixels,SourceUse};
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let request=MovingRequest {nearest:true,level:0,format:wgpu::TextureFormat::Rgba8Unorm,
        ..request([PAGE_SIZE;2],PixelRect::full([PAGE_SIZE;2]),31)};
    let exact=layer_core::color::source::rgba8_source([PAGE_SIZE;2],|_,_|[97,113,149,255]);
    let pixels=prepare_image_tile(&request.source,[0;2],r.device.working_space(),&r.device.source_samples,None).unwrap();
    assert!(matches!(pixels,PreparedSourcePixels::NativeSamples(_)));
    submit_encoded(&mut r,|r,encoder| {
        pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.source.pipeline,&r.scene_pipelines.source.integer])).unwrap();
        crate::scene::Scene::decode_prepared_for(r,encoder,&request.source,[0;2],pixels.clone(),SourceUse::Exact,crate::residency::Class::Visible).unwrap();
    });complete(&r);
    submit_encoded(&mut r,|r,encoder| {
        let target=request.allocation(&r.device,0,false).unwrap().unwrap();
        let mut images=MovingImages::within(&r.scene_tiers);
        pollster::block_on(r.compilation.prepare_all(images.pipelines(&r.device))).unwrap();
        images.allocate(r,MipPlan {targets:vec![target],waiting:false},encoder);
        r.moving_images=images;
        r.source_results.push_back((r.device.working_space(),PreparedImageWork::Image(PreparedMipTile {
            class:crate::residency::Class::Visible,use_:SourceUse::Native,id:request.id,source:request.source.clone(),coordinate:[0;2],pixels:Ok(pixels),
        }),r.scene_tiers.ram.charge(0)));
        r.drain_image_decodes(encoder).unwrap();
        assert!(r.source_results.is_empty(),"the drain that takes a window's samples writes them instead of holding them");
        assert_eq!(r.moving_images.tile_builds,1);
        assert!(r.moving_images.entries.iter().all(Entry::ready));
        assert!(r.moving_images_pending(),"the completed window keeps a frame runnable until it is published");
        let (send,results)=std::sync::mpsc::sync_channel(2);
        let ready=Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let queue=queued(results,ready.clone(),Vec::new(),2);let dispatch=queue.pool.dispatch.clone();
        r.moving_decode=Some((r.device.working_space(),queue));
        r.set_preparation_wake(Arc::new(|| {}));
        let id=PortableId::random();
        assert!(r.request_exact_image_decode(id,exact.clone(),[0;2],crate::residency::Class::Visible).unwrap());
        let Some(DecodeJob {sequence,request:MipDecodeRequest::Image(_),..})=dispatch.try_take() else {panic!("the exact request reaches a worker");};
        send.send((sequence,PreparedImageWork::Image(PreparedMipTile {class:crate::residency::Class::Visible,use_:SourceUse::Exact,id,source:exact.clone(),coordinate:[0;2],
            pixels:prepare_image_tile(&exact,[0;2],r.device.working_space(),&r.device.source_samples,None)}))).unwrap();
        ready.fetch_add(1,Ordering::AcqRel);
        r.drain_image_decodes(encoder).unwrap();
        assert!(r.source_tiles.borrow().prepared_view(&exact,[0;2]).is_some(),"an exact query's result is taken by the next drain");
    });complete(&r);
}


#[test]
fn deferred_window_color_preparation_preserves_the_submitted_dab() {
    use layer_core::{authored::*, color::ColorProfile, BrushExecution};
    use layer_render::FramePacket;
    let extent = [256; 2];
    let mut photo = request(extent, PixelRect::full(extent), 31);
    Arc::make_mut(&mut photo.source).interpretation.profile = ColorProfile::Builtin(RgbSpace::AdobeRgb);
    let mut doc = photo_document(std::slice::from_ref(&photo), extent);
    let paint = doc.artwork.paint.insert(PortableId::random(), PaintSource {
        domain: extent, color_mode: Default::default(), base: None,
        raster: Default::default(), operations: Default::default(),
    }).unwrap();
    let owner = doc.artwork.occurrences.insert(PortableId::random(),
        Occurrence::new(OccurrenceContent::Paint(paint), "Ink")).unwrap();
    let stack = doc.artwork.compositions.get(doc.artwork.root).unwrap().result;
    doc.artwork.stacks.get_mut(stack).unwrap().entries.insert(0, owner);
    let doc = layer_core::Document::from_artwork(doc.artwork).unwrap();
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    r.ensure_document_metadata(extent, doc.scene()).unwrap();
    r.source_tiles.borrow_mut().prepare_transfer(&r.device, RgbSpace::Srgb).unwrap();
    assert_eq!(r.source_tiles.borrow().non_slot_bytes(), crate::native_tiles::transfer::TABLE_BYTES);
    let dab = crate::tests::test_dab([128.; 2], [1., 0., 0., 1.], 1.);
    let batch = crate::test_support::dab_batch(SourceTarget::Paint(paint),
        crate::tests::test_style(BrushExecution::Dry),
        layer_core::Rect { min: layer_core::Point {x: 108., y: 108.}, max: layer_core::Point {x: 148., y: 148.} });
    let frame = FramePacket {
        view: layer_render::ViewState { width_px: 128, height_px: 128,
            document_to_surface: [0.5, 0., 0., 0.5, 0., 0.] },
        dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch),
        ..crate::test_support::packet(doc.scene(), extent)
    };
    crate::memory::memory_tests::declare_source_budget(&mut r, frame, 20, 8 << 20, 8 << 20);
    assert_eq!(r.moving_projection.as_ref().unwrap().requests[0].level, 1);
    assert!(!r.moving_projection.as_ref().unwrap().requests[0].nearest);
    let encoded_bytes = u64::from(PAGE_SIZE).pow(2) * 4;
    let allowance = 20 * crate::scene::sources::NORMALIZED_PAGE_BYTES;
    assert_eq!(allowance % encoded_bytes, 0);
    let mut sources = vec![photo.source.clone()];
    sources.extend((1..allowance / encoded_bytes).map(|value|
        layer_core::color::source::rgba8_source(extent, |_, _| [value as u8, 73, 149, 255])));
    submit_encoded(&mut r, |r, encoder| {
        for source in &sources {
            crate::test_support::query_ready(r, encoder, |r, encoder| {
                let pixels = prepare_image_tile(source, [0; 2], r.device.working_space(), &r.device.source_samples, None).unwrap();
                crate::scene::Scene::decode_prepared(r, encoder, source, [0; 2], pixels)
            });
        }
    }); complete(&r);
    assert_eq!(r.source_tiles.borrow().gpu_bytes(), allowance + r.source_tiles.borrow().non_slot_bytes());
    assert!(r.source_tiles.borrow().prepared_view(&photo.source, [0; 2]).is_some());
    assert_eq!(r.source_tiles.borrow().non_slot_bytes(), crate::native_tiles::transfer::TABLE_BYTES,
        "only the working-space sRGB table is allocated");
    crate::test_support::submit_ready(&mut r, frame).unwrap();
    complete(&r);
    let page = r.paint_layers.iter().find(|layer| layer.id == SourceTarget::Paint(paint)).unwrap()
        .pages.iter().find(|page| page.coordinate == [0; 2]).unwrap();
    let expected = r.raster_version_for(doc.scene(), SourceTarget::Paint(paint), layer_core::raster::RasterPlane::Color, page.coordinate).unwrap();
    let ink = crate::test_support::float_pixels(&r, &page.active().read(expected, None).unwrap().value().texture);
    assert_eq!(ink[128 * 256 + 128], [1., 0., 0., 1.]);
    let idle = FramePacket { dabs: &[], dab_batches: &[], composite_all: false, ..frame };
    crate::test_support::submit_settled(&mut r, idle);
    let output = r.readback_srgb_rgba8().unwrap();
    assert_eq!(&output[(128 * 256 + 128) * 4..(128 * 256 + 128) * 4 + 4], &[255, 0, 0, 255]);
}

pub(crate) fn assert_optional_display_grant_decrease() {
    let extent=[8192;2];
    let mut requests=[request(extent,PixelRect::full(extent),31),request(extent,PixelRect::full(extent),47)];
    let document=photo_document(&requests,extent);
    for request in &mut requests {
        request.id=document.artwork.objects.iter().find(|(_,_,object)|Arc::ptr_eq(object.image.storage(),&request.source)).unwrap().2.image.id();
    }
    let mut r=crate::memory::memory_tests::renderer(document.composition().color,
        layer_core::DeviceMemoryFacts {unified:true,..Default::default()}).unwrap();
    r.ensure_document_metadata(extent,document.scene()).unwrap();
    let mut frame=crate::test_support::packet(document.scene(),extent);
    frame.view=layer_render::ViewState {width_px:8192,height_px:8192,document_to_surface:[1.,0.,0.,1.,0.,0.]};
    crate::memory::memory_tests::legacy_scene_frame(&mut r,frame);
    let mut images=MovingImages::within(&r.scene_tiers);submit_encoded(&mut r,|r,encoder| {
        let plan=images.plan(r,&requests,512<<20,encoder).unwrap();
        images.allocate(r,plan,encoder);drain(r,&mut images,encoder);
    });complete(&r);
    assert!(images.entries.iter().all(Entry::ready));
    r.end_frame_memory();
    r.moving_images=images;
    crate::scene::scale::assert_display_grant_decrease_reconciles_retention(&mut r,&document);
}

#[test]
fn ready_moving_windows_within_the_display_grant_survive_a_smaller_mip_share() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let requests=(0..4u8).map(|index|MovingRequest {rank:usize::from(index),..request([1024;2],PixelRect::new(0,0,512,512),31+index)}).collect::<Vec<_>>();
    let mut images=std::mem::take(&mut r.moving_images);
    submit_encoded(&mut r,|r,encoder| {
        let plan=images.plan(r,&requests,256<<20,encoder).unwrap();
        images.allocate(r,plan,encoder);drain(r,&mut images,encoder);
    });complete(&r);
    let entries=images.entries.len();
    assert!(entries>1 && images.entries.iter().all(Entry::ready));
    r.moving_images=images;
    r.memory.mip_demand(0,[1024;2]);r.publish_memory();r.factory.memory_budget().flush_reports();
    r.memory.invalidate_limits();r.refresh_memory_limits();
    assert!(r.scene_tiers.display.used()<=r.memory.display_owner().grant().gpu_bytes,"the fixture display fits its grant");
    assert_eq!(r.moving_images.entries.len(),entries,"windows the display grant holds must not be trimmed to the mip share");
}

#[test]
fn a_moving_window_beyond_the_display_limit_is_refused_instead_of_allocated() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let requests=[request([1024;2],PixelRect::new(0,0,512,512),31)];
    let mut images=std::mem::take(&mut r.moving_images);
    submit_encoded(&mut r,|r,encoder| {
        let plan=images.plan(r,&requests,256<<20,encoder).unwrap();
        let display=r.scene_tiers.display.clone();
        display.set_limit(display.used()+plan.targets.iter().map(|target|target.bytes).sum::<u64>()/2,0);
        images.allocate(r,plan,encoder);
        assert!(display.used()<=display.limit(),"moving windows hold {} display bytes beyond the limit {}",display.used(),display.limit());
        assert!(images.pending(),"the refused window waits for display room");
    });complete(&r);
}

#[test]
fn display_reports_read_no_moving_image_entry_and_count_retiring_windows_until_completion() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let requests=(0..4u8).map(|index|MovingRequest {rank:usize::from(index),..request([1024;2],PixelRect::new(0,0,512,512),31+index)}).collect::<Vec<_>>();
    let mut images=std::mem::take(&mut r.moving_images);
    let report=|r:&WgpuRasterizer|{let report=r.current_display_memory_report();[report.gpu_bytes,report.pinned_gpu_bytes]};
    let before=report(&r);
    submit_encoded(&mut r,|r,encoder| {
        let plan=images.plan(r,&requests,256<<20,encoder).unwrap();
        images.allocate(r,plan,encoder);drain(r,&mut images,encoder);
    });complete(&r);
    assert!(images.entries.len()>1 && images.entries.iter().all(Entry::ready));
    r.moving_images=images;
    let with=report(&r);
    let stored=r.moving_images.storage_bytes();
    assert_eq!(r.scene_tiers.moving[0].used(),stored,"the renderer tiers hold every window and the work image charge");
    assert_eq!(with[0]-before[0],stored,"the display report counts each window once");
    let entries=std::mem::take(&mut r.moving_images.entries);
    assert_eq!(report(&r),with,"a report reads counters, not entries");
    r.moving_images.entries=entries;
    let factory=r.factory.clone();
    let mut next=WgpuRasterizer::from_factory_inner(factory.clone(),factory.pipeline_device(Default::default()),crate::Initialization::Headless).unwrap();
    layer_render::CanvasRenderer::adopt_committed_pages(&mut *next,&mut r);
    assert_eq!(report(&next),with,"a same-device replacement reports the windows it adopted");
    let mut r=next;
    let work=r.moving_images.work_bytes();
    submit_encoded(&mut r,|r,encoder| {
        let retired=trim(&mut r.moving_images,work,encoder);
        assert!(r.moving_images.entries.is_empty() && retired==stored-work);
        assert_eq!((r.moving_images.storage_bytes(),r.moving_images.pinned_bytes()),(stored,work+retired),"retiring windows stay counted and pinned");
    });complete(&r);r.device.retirement_handle().flush().unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    trim(&mut r.moving_images,0,&mut encoder);
    assert_eq!((r.moving_images.storage_bytes(),r.moving_images.pending()),(0,false));
}
