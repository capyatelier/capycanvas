use super::*;
use layer_core::authored::*;

impl ObjectCache {
    pub(in crate::scene) fn for_scene(scene:&Scene,retains:bool)->Self {
        Self::new(&scene.tiers,scene.object_results.keys.clone(),retains)
    }
}

#[test]
fn initialized_object_bytes_are_charged_once_before_settle() {
    let extent=[8;2];
    let source=layer_core::color::source::rgba8_source(extent,|_,_|[71,113,29,255]);
    let mut artwork=Artwork::new(extent).unwrap();
    let image=ImageObject::new(layer_core::authored::Image::new(source));
    let object=artwork.objects.insert(PortableId::random(),image).unwrap();
    let owner=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(object),"Image")).unwrap();
    let stack=artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries=vec![owner];
    let document=layer_core::Document::from_artwork(artwork).unwrap();
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
    crate::memory::memory_tests::legacy_scene_frame(&mut r,crate::test_support::packet(document.scene(),extent));
    let mut scene=Scene::new(&r).unwrap();
    let content=scene.object_spatial.content(document.scene(),owner).unwrap();
    let request=scene.collection_job(&r,crate::test_support::packet(document.scene(),extent),owner,content,
        objects::ObjectWindow {origin:[0.;2],side:1.,size:extent});
    let mut cache=ObjectCache::for_scene(&scene,false);
    assert!(cache.resolve_collection(&r,document.scene(),&request,None).unwrap().is_none());
    let mut task=cache.tasks.pop_front().unwrap();
    let mut native=sources::Inputs::default();native.image(&task.job.source,[0;2],r.device.working_space());
    task.inputs=crate::test_support::prepare_raster(&mut r,|r| {
        native.acquire(r,crate::residency::Class::Visible)
    }).unwrap();
    let before=r.scene_tiers.display.used();
    let sampler=r.scene_pipelines.objects.clone();
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    cache.advance_task(&mut r,&mut scene,&mut encoder,&sampler,&mut task,PASS_TAPS).unwrap();
    let accumulation=task.accumulation.as_ref().expect("initialization must allocate the accumulator");
    let physical=task.texture.as_ref().map_or(0,texture_bytes)+accumulation.gpu_bytes()
        +task.collection.as_ref().map_or(0,|c|c.spare.as_ref().map_or(0,texture_bytes)+c.result.as_ref().map_or(0,texture_bytes));
    assert_eq!(r.scene_tiers.display.used()-before,physical,"object allocation must consume its reservation, including the single shared uniform");
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn a_refused_object_admission_wakes_once_when_the_display_limit_rises() {
    let extent=[8;2];
    let source=layer_core::color::source::rgba8_source(extent,|_,_|[71,113,29,255]);
    let mut artwork=Artwork::new(extent).unwrap();
    let object=artwork.objects.insert(PortableId::random(),ImageObject::new(layer_core::authored::Image::new(source))).unwrap();
    let owner=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(object),"Image")).unwrap();
    let stack=artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries=vec![owner];
    let document=layer_core::Document::from_artwork(artwork).unwrap();
    let frame=crate::test_support::packet(document.scene(),extent);
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
    crate::memory::memory_tests::legacy_scene_frame(&mut r,frame);
    let mut scene=Scene::new(&r).unwrap();
    let content=scene.object_spatial.content(document.scene(),owner).unwrap();
    let request=scene.collection_job(&r,frame,owner,content,objects::ObjectWindow {origin:[0.;2],side:1.,size:extent});
    let bytes=scene.object_results.window_bytes(&r,&request).unwrap();
    let held=scene.object_results.working.charge(1);
    let display=r.scene_tiers.display.clone();
    display.set_limit(display.used()+bytes/2,0);
    crate::test_support::complete(&r);r.device.retirement_handle().flush().unwrap();
    let (send,woken)=std::sync::mpsc::channel();
    r.preparation_wake=Some(Arc::new(move||{let _=send.send(());}));
    let mut admit=|r:&WgpuRasterizer|scene.object_results.admit(r,Class::Visible,std::slice::from_ref(&request),None).unwrap().is_none();
    assert!(!admit(&r),"the display tier must refuse the object window");
    assert!(woken.recv_timeout(std::time::Duration::from_millis(200)).is_err(),"a refused admission is not woken before a release");
    display.set_limit(u64::MAX,0);
    woken.recv_timeout(std::time::Duration::from_secs(1)).expect("a raised display limit must wake the refused admission");
    assert!(woken.recv_timeout(std::time::Duration::from_millis(200)).is_err(),"a raised limit wakes the refused admission once");
    assert!(admit(&r),"the woken admission fits");
    drop(held);
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn a_ledger_refused_object_admission_is_blocked_on_the_ledger_and_wakes_once_when_it_releases() {
    const MIB:u64=1<<20;
    let extent=[8;2];
    let source=layer_core::color::source::rgba8_source(extent,|_,_|[71,113,29,255]);
    let mut artwork=Artwork::new(extent).unwrap();
    let object=artwork.objects.insert(PortableId::random(),ImageObject::new(layer_core::authored::Image::new(source))).unwrap();
    let owner=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(object),"Image")).unwrap();
    let stack=artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries=vec![owner];
    let document=layer_core::Document::from_artwork(artwork).unwrap();
    let frame=crate::test_support::packet(document.scene(),extent);
    let mut r=crate::memory::memory_tests::renderer(document.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<30),..Default::default()}).unwrap();
    let mut scene=Scene::new(&r).unwrap();
    let content=scene.object_spatial.content(document.scene(),owner).unwrap();
    let request=scene.collection_job(&r,frame,owner,content,objects::ObjectWindow {origin:[0.;2],side:1.,size:extent});
    let bytes=scene.object_results.window_bytes(&r,&request).unwrap();
    let raster=r.admit_raster_working(512*MIB).expect("the raster holds most of the device");
    r.memory.display_owner().report(layer_core::MemoryReport {demand_gpu_bytes:1,..Default::default()});
    r.scene_tiers.display.set_limit(u64::MAX,0);
    let held=r.scene_tiers.display.charge(384*MIB);
    let (send,woken)=std::sync::mpsc::channel();
    r.preparation_wake=Some(Arc::new(move||{let _=send.send(());}));
    let blocked=scene.object_results.admit(&r,Class::Visible,std::slice::from_ref(&request),None).unwrap().expect("the ledger refuses the object window");
    assert!(r.scene_tiers.display.blocked(Class::Visible,bytes).is_none(),"the display tier alone would admit the window");
    assert!(!blocked.fits(),"a ledger refusal stays blocked while only the tier fits");
    assert!(woken.recv_timeout(std::time::Duration::from_millis(200)).is_err(),"a ledger-refused admission is not woken before a release");
    r.commit_raster_working(raster);
    woken.recv_timeout(std::time::Duration::from_secs(1)).expect("the ledger's release edge wakes the refused admission");
    assert!(woken.recv_timeout(std::time::Duration::from_millis(200)).is_err(),"one release wakes the refused admission once");
    assert!(blocked.fits(),"the refusal fits once the ledger can admit it");
    assert!(scene.object_results.admit(&r,Class::Visible,std::slice::from_ref(&request),None).unwrap().is_none(),"the woken admission fits");
    drop(held);
}
