use super::*;

#[test]
fn stacked_photos_reuse_canonical_regions_within_the_retained_budget() {
    let extent=[1024;2];let owners=28u64;let object_grant=64u64<<20;
    let objects=(0..owners).map(|seed|ImageObject::new(AuthoredImage::new(rgba8_source(extent,|x,y|
        [((x*7+seed as u32*13)%256) as u8,((y*11+seed as u32*17)%256) as u8,((x^y)%256) as u8,255])))).collect();
    let (mut doc,_,_)=document(extent,objects);
    let layers=doc.scene().order().to_vec();
    for (index,owner) in layers.into_iter().enumerate() {
        doc.artwork.occurrences.get_mut(owner).unwrap().opacity=if index==27 {1.}else{0.35};
    }
    let doc=layer_core::Document::from_artwork(doc.artwork).unwrap();
    let view=layer_render::ViewState {width_px:512,height_px:512,document_to_surface:[0.5,0.,0.,0.5,0.,0.]};
    let frame=FramePacket {view,..packet(doc.scene(),extent)};
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    crate::memory::memory_tests::legacy_scene_frame(&mut r,frame);
    crate::memory::memory_tests::object_frame_with_budget(&mut r,frame,object_grant);
    for _ in 0..4000 {
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        r.drain_image_decodes(&mut encoder).unwrap();
        r.prepare_moving_images(frame,&mut encoder).unwrap();
        r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
        if !r.moving_images_pending() {break;}
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(r.moving_images.completed_images,owners);
    assert!(r.scene.as_ref().unwrap().object_results.request_windows().is_empty());
    let mut identities=std::collections::HashSet::new();let mut leases=Vec::new();
    let mut requests=std::collections::BTreeMap::new();let mut texels=0u64;let mut repeated=0usize;let mut settled=None;
    let needed=owners*extent.map(|side|u64::from(side/2)).into_iter().product::<u64>();
    let texel_bound=needed*6/5;
    assert!(owners*u64::from(PAGE_SIZE).pow(2)*16<object_grant/2);
    assert!(needed*16>object_grant/2);
    for index in 0..4000 {
        crate::test_support::submit_ready(&mut r,FramePacket {composite_all:index==0,reset_layers:index==0,..frame}).unwrap();
        r.wait_idle().unwrap();
        let cache=r.scale_display.as_ref().unwrap();
        assert_eq!(cache.plan.level,1);assert!(cache.evaluation==crate::scene::scale::Evaluation::Display);
        for (lease,owner,window) in r.scene.as_ref().unwrap().object_results.request_windows() {
            if !identities.insert(lease.as_ptr() as usize) {continue;}
            texels+=window.size.map(u64::from).into_iter().product::<u64>();
            let count=requests.entry((owner,window.origin.map(f64::to_bits),window.side.to_bits(),window.size)).or_insert(0usize);
            repeated+=usize::from(*count>0);*count+=1;leases.push(lease);
        }
        let exact=r.has_presentable_frame() && !cache.approximating() && !r.object_deferred
            && !r.scene.as_ref().unwrap().object_results.pending() && !r.has_pending_work();
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        r.scene.as_mut().unwrap().trim_memory(u64::MAX,object_grant,&mut encoder);
        r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
        if exact {settled=Some(index+1);break;}
        if repeated>0 || texels>texel_bound {break;}
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(texels<=texel_bound,"canonical work exceeds 1.2 times the display's needed texels: {texels}");
    assert_eq!(repeated,0,"retired page results must not be admitted again");
    assert!(settled.is_some(),"the canonical region workload completes");
}
