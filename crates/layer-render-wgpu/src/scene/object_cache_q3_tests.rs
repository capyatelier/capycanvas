use super::*;
use crate::test_support::submit_encoded;
use layer_core::authored::*;

impl ObjectCache {
    pub(in crate::scene) fn for_scene(scene:&Scene,retains:bool)->Self {
        Self::new(&scene.tiers,scene.object_results.keys.clone(),retains)
    }
    pub(in crate::scene) fn owner(&self) -> u64 { self.owner }
    pub fn evict_to(&mut self, gpu: u64) -> u64 {
        let used = self.gpu.used().saturating_sub(self.retiring.iter().map(|task| task.gpu.bytes()).sum::<u64>());
        self.gpu.heap().evict(used.saturating_sub(gpu), Class::Fallback);
        self.retire_evicted()
    }
    pub fn request_windows(&self) -> Vec<(std::sync::Weak<AtomicBool>, OccurrenceHandle, objects::ObjectWindow)> {
        self.tasks.values().filter_map(|task| task.collection.as_ref().map(|collection|
            (Arc::downgrade(&task.valid), collection.request.owner, collection.request.window))).collect()
    }
    pub fn completed_windows(&self) -> Vec<(std::sync::Weak<AtomicBool>, OccurrenceHandle, objects::ObjectWindow)> {
        self.tasks.values().filter(|task| task.complete).filter_map(|task| task.collection.as_ref().map(|collection|
            (Arc::downgrade(&task.valid), collection.request.owner, collection.request.window))).collect()
    }
    pub fn work_progress(&self) -> (usize, usize, u64) {
        (self.tasks.len(), self.tasks.values().filter(|task|task.complete).count(), self.tasks.values().map(|task|task.step + task.tile as u64 * 1000000 + task.phase as u64 * 1000000000 + task.collection.as_ref().map_or(0,|c|c.part as u64 * 1000000000000)).sum())
    }
    pub fn bytes(&self) -> u64 { let [ram, gpu] = self.memory_bytes(); ram + gpu }
    pub fn memory_bytes(&self) -> [u64; 2] { [self.ram.used() + self.retired.used(), self.gpu.used() + self.retired_gpu.used()] }
    pub fn walked_bytes(&self) -> [u64; 3] {
        let tasks = || self.tasks.values().chain(self.retiring.iter());
        let gpu = self.uniform.as_ref().map_or(0, |(uniform, _)| uniform.size()) + self.retired_gpu.used()
            + tasks().map(|task| task.gpu_bytes() + task.seats.len() as u64 * sources::NORMALIZED_PAGE_BYTES).sum::<u64>();
        let ram = self.damage.values().map(|damage| std::mem::size_of::<(u32, scale::Damage)>() as u64 + damage.regions.capacity() as u64 * std::mem::size_of::<PixelRect>() as u64).sum::<u64>()
            + self.retired.used() + self.coordinates.values().filter_map(std::sync::Weak::upgrade).map(|coordinates| { let state = coordinates.lock().unwrap(); std::mem::size_of::<Option<crate::object_image_mips::CoordinateResult>>() as u64 + state.result.as_ref().and_then(|result| result.as_ref().ok()).map_or(0, |bytes| bytes.capacity() as u64) }).sum::<u64>()
            + self.coordinates.capacity() as u64 * (std::mem::size_of::<CoordinateKey>() + std::mem::size_of::<crate::object_image_mips::CoordinateDestination>()) as u64
            + self.queue_bytes() + tasks().map(Task::ram_bytes).sum::<u64>();
        let pinned = self.retired_gpu.used() + self.retiring.iter().map(Task::gpu_bytes).sum::<u64>()
            + self.tasks.values().filter(|task| task.used == self.frame || task.pin.strong_count() > usize::from(task.hold.is_some()) || !self.retains).map(Task::gpu_bytes).sum::<u64>();
        [ram, gpu, pinned]
    }
    pub fn pinned_gpu_bytes(&self) -> u64 { self.pinned.used() }
    pub fn advance(&mut self,r:&mut WgpuRasterizer,scene:&mut Scene,encoder:&mut crate::submission::CommandEncoder,budget:&mut WorkBudget)->Result<u64,GpuRasterError> {
        let allowance=budget.allowance(Class::Prefetch);
        let used=self.advance_with_memory(r,scene,encoder,|_|allowance)?;
        budget.spend(used);Ok(used)
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
    let closure=ObjectCache::closure(&r,&request).unwrap();
    assert!(cache.admit(&r,crate::residency::Class::Visible,std::slice::from_ref(&request),None).unwrap().is_none());
    let mut task=cache.tasks.pop_first().unwrap().1;
    let mut native=sources::Inputs::default();native.image(&task.job.source,[0;2],r.device.working_space());
    task.inputs=crate::test_support::prepare_raster(&mut r,|r| {
        native.acquire(r,crate::residency::Class::Visible)
    }).unwrap();
    let before=r.scene_tiers.display.used();
    let sampler=r.scene_pipelines.objects.clone();
    submit_encoded(&mut r,|r,encoder| {
        cache.advance_task(r,&mut scene,encoder,&sampler,&mut task,PASS_TAPS).unwrap();
        let accumulation=task.accumulation.as_ref().expect("initialization must allocate the accumulator");
        let uniform=cache.uniform.as_ref().map_or(0,|(uniform,_)|uniform.size());
        let physical=task.texture.as_ref().map_or(0,texture_bytes)+accumulation.gpu_bytes()
            +task.collection.as_ref().map_or(0,|c|c.spare.as_ref().map_or(0,texture_bytes)+c.result.as_ref().map_or(0,texture_bytes));
        assert!(physical-uniform<=closure.bytes,"the admitted closure bounds the allocation");
        assert_eq!(r.scene_tiers.display.used()-before,uniform,"object allocation consumes its admitted reservation; only the single shared uniform adds");
    });crate::test_support::complete(&r);
}

fn two_page_photo_windows(sizes:&[[u32;2]],cached:bool)->(WgpuRasterizer,ObjectCache,Vec<Task>) {
    let extent=[2*PAGE_SIZE,PAGE_SIZE];
    let source=layer_core::color::source::rgba8_source(extent,|_,_|[71,113,29,255]);
    let mut artwork=Artwork::new(extent).unwrap();
    let object=artwork.objects.insert(PortableId::random(),ImageObject::new(layer_core::authored::Image::new(source.clone()))).unwrap();
    let owner=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(object),"Image")).unwrap();
    let stack=artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries=vec![owner];
    let document=layer_core::Document::from_artwork(artwork).unwrap();
    let frame=crate::test_support::packet(document.scene(),extent);
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
    crate::memory::memory_tests::legacy_scene_frame(&mut r,frame);
    if cached {
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        for coordinate in [[0,0],[1,0]] {
            let pixels=crate::prepare_image_tile(&source,coordinate,r.device.working_space(),&r.device.source_samples,None).unwrap();
            crate::test_support::prepare_raster(&mut r,|r|Scene::decode_prepared_for(r,&mut encoder,&source,coordinate,pixels.clone(),sources::SourceUse::Exact,Class::Visible)).unwrap();
        }
        r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
    }
    let mut scene=Scene::new(&r).unwrap();
    let content=scene.object_spatial.content(document.scene(),owner).unwrap();
    let requests:Vec<_>=sizes.iter().map(|&size|scene.collection_job(&r,frame,owner,content.clone(),objects::ObjectWindow {origin:[0.;2],side:1.,size})).collect();
    let mut cache=ObjectCache::for_scene(&scene,false);
    for request in &requests {assert!(cache.resolve_collection(&r,document.scene(),request,None).unwrap().is_none());}
    assert!(cache.admit(&r,Class::Visible,&requests,None).unwrap().is_none());
    let mut tasks:Vec<Task>=std::iter::from_fn(||cache.tasks.pop_first().map(|(_,task)|task)).collect();
    for task in &mut tasks {
        assert!(cache.schedule.start(task.group.as_ref().unwrap()));
        task.tiles=page_coordinates(working_set(&task.job).unwrap().1).collect();
    }
    (r,cache,tasks)
}

fn charged_slots(r:&WgpuRasterizer)->u64 {r.scene_tiers.display.used()-r.source_tiles.borrow().non_slot_bytes()}

#[test]
fn a_window_seat_pays_for_its_missed_input_until_the_slot_returns_to_the_cache() {
    let (mut r,mut cache,mut tasks)=two_page_photo_windows(&[[2*PAGE_SIZE,PAGE_SIZE]],false);
    let task=&mut tasks[0];
    let admitted=charged_slots(&r);
    cache.prepare_inputs(&mut r,task).unwrap();
    assert_eq!(task.seats.len(),2,"the window holds a seat for each missed input");
    assert_eq!(charged_slots(&r),admitted,"the seats the window was admitted with pay for the slots its missed inputs decode into");
    cache.vacate(task);
    assert_eq!(charged_slots(&r),admitted+2*sources::NORMALIZED_PAGE_BYTES,"released slots stay as cache, charged once");
    assert_eq!(cache.schedule.reserved(),ObjectCache::closure(&r,&task.collection.as_ref().unwrap().request).unwrap().total(),"the window budget is whole again");
}

#[test]
fn a_window_reading_pages_another_windows_seats_pay_for_holds_no_seat_for_them() {
    let (mut r,mut cache,mut tasks)=two_page_photo_windows(&[[2*PAGE_SIZE,PAGE_SIZE],[2*PAGE_SIZE,PAGE_SIZE-1]],true);
    let admitted=charged_slots(&r);
    for task in &mut tasks {cache.prepare_inputs(&mut r,task).unwrap();}
    assert_eq!(tasks[0].seats.len(),2,"the first window's seats pay for the two cached pages it reads");
    assert!(tasks[1].seats.is_empty(),"a window holds no seat for pages another window's seats pay for, but held {}",tasks[1].seats.len());
    assert_eq!(charged_slots(&r),admitted-3*sources::NORMALIZED_PAGE_BYTES,"each cached page is charged once, by the seat that pays for it, and the second window keeps one of its two seats");
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
    let bytes=ObjectCache::closure(&r,&request).unwrap().total();
    let held=scene.object_results.working.charge(1);
    let display=r.scene_tiers.display.clone();
    display.set_limit(display.used()+bytes/2,0);
    crate::test_support::complete(&r);r.device.retirement_handle().flush().unwrap();
    let (send,woken)=std::sync::mpsc::channel();
    r.preparation_wake=Some(Arc::new(move||{let _=send.send(());}));
    assert!(scene.object_results.admit(&r,Class::Visible,std::slice::from_ref(&request),None).unwrap().is_some(),"the display tier must refuse the object window");
    assert_ne!(scene.object_results.demand(&r,|_| false),Some(crate::frame_demand::Outstanding::Runnable));
    assert!(woken.recv_timeout(std::time::Duration::from_millis(200)).is_err(),"a refused admission is not woken before a release");
    display.set_limit(u64::MAX,0);
    woken.recv_timeout(std::time::Duration::from_secs(1)).expect("a raised display limit must wake the refused admission");
    assert!(woken.recv_timeout(std::time::Duration::from_millis(200)).is_err(),"a raised limit wakes the refused admission once");
    assert_eq!(scene.object_results.demand(&r,|_| false),Some(crate::frame_demand::Outstanding::Runnable));
    assert!(scene.object_results.admit(&r,Class::Visible,std::slice::from_ref(&request),None).unwrap().is_none(),"the woken admission fits");
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
    let bytes=ObjectCache::closure(&r,&request).unwrap().total();
    let raster=r.admit_raster_working(512*MIB,crate::frame_demand::Allowance::Fresh).expect("the raster holds most of the device");
    r.memory.display_owner().report(layer_core::MemoryReport {demand_gpu_bytes:1,..Default::default()});
    r.scene_tiers.display.set_limit(u64::MAX,0);
    let held=r.scene_tiers.display.charge(384*MIB);
    let (send,woken)=std::sync::mpsc::channel();
    r.preparation_wake=Some(Arc::new(move||{let _=send.send(());}));
    r.watch_ledger();
    let blocked=scene.object_results.admit(&r,Class::Visible,std::slice::from_ref(&request),None).unwrap().expect("the ledger refuses the object window");
    assert!(r.scene_tiers.display.blocked(Class::Visible,bytes).is_some(),"the display tier judges the window against the ledger's room, not its stored limit");
    assert!(!blocked.fits(),"a ledger refusal stays blocked while the raster holds the device");
    assert_ne!(scene.object_results.demand(&r,|_| false),Some(crate::frame_demand::Outstanding::Runnable));
    assert!(woken.recv_timeout(std::time::Duration::from_millis(200)).is_err(),"a ledger-refused admission is not woken before a release");
    r.commit_raster_working(raster);
    woken.recv_timeout(std::time::Duration::from_secs(1)).expect("the ledger's release edge wakes the refused admission");
    assert!(woken.recv_timeout(std::time::Duration::from_millis(200)).is_err(),"one release wakes the refused admission once");
    assert!(blocked.fits(),"the refusal fits once the ledger can admit it");
    assert_eq!(scene.object_results.demand(&r,|_| false),Some(crate::frame_demand::Outstanding::Runnable));
    assert!(scene.object_results.admit(&r,Class::Visible,std::slice::from_ref(&request),None).unwrap().is_none(),"the woken admission fits");
    drop(held);
}
