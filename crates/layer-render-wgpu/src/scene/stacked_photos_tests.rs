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
        let exact=r.has_presentable_frame() && !cache.approximating() && !crate::scene::scale::tests::publication_pending(&r)
            && !r.scene.as_ref().unwrap().object_results.pending() && r.next_frame()!=layer_render::FrameDemand::Work;
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        r.scene.as_mut().unwrap().retire_victims(&mut encoder);
        r.uploads.finish(&encoder);encoder.submit(&r.queue);r.wait_idle().unwrap();
        if exact {settled=Some(index+1);break;}
        if repeated>0 || texels>texel_bound {break;}
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(texels<=texel_bound,"canonical work exceeds 1.2 times the display's needed texels: {texels}");
    assert_eq!(repeated,0,"retired page results must not be admitted again");
    assert!(settled.is_some(),"the canonical region workload completes");
}

#[test]
fn imported_fit_photos_pin_only_the_visible_demand() {
    use layer_core::color::{ColorProfile, RgbSpace};
    let extent=[2108,1508];
    let profile=ColorProfile::Icc(layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::Srgb)).unwrap().into());
    let photo=|seed:u32| {
        let mut builder=SourceBuilder::new(extent,SourceInterpretation {channels:SourceChannels::Rgb,depth:SampleDepth::U8,profile:profile.clone(),profile_assumed:false},16<<20).unwrap();
        let mut row=vec![0u8;extent[0] as usize*3];
        for y in 0..extent[1] {
            for (x,pixel) in (0..).zip(row.chunks_exact_mut(3)) {pixel.copy_from_slice(&[((x*7+seed*13)%256) as u8,((y*11+seed*17)%256) as u8,((x^y^seed)%256) as u8]);}
            builder.push_row(&row).unwrap();
        }
        AuthoredImage::new(Arc::new(builder.finish().unwrap()))
    };
    let mut doc=layer_core::Document::from_artwork(Artwork::new(extent).unwrap()).unwrap();
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let viewport=[1500,1100];
    let zoom=(viewport[0] as f32/extent[0] as f32).min(viewport[1] as f32/extent[1] as f32);
    let view=layer_render::ViewState {width_px:viewport[0],height_px:viewport[1],
        document_to_surface:[zoom,0.,0.,zoom,(viewport[0] as f32-extent[0] as f32*zoom)/2.,(viewport[1] as f32-extent[1] as f32*zoom)/2.]};
    let mut settled=Vec::new();
    for seed in 0..10 {
        let at=doc.scene().children(None).len();
        let (_,edit)=doc.create_object_layer_edit("Photo",ImageObject::new(photo(seed)),None,at).unwrap();doc.apply(edit).unwrap();
        let frame=FramePacket {view,..packet(doc.scene(),extent)};
        crate::test_support::submit_ready(&mut r,frame).unwrap();
        for _ in 0..1000 {
            r.wait_idle().unwrap();
            if r.next_frame()!=layer_render::FrameDemand::Work {break;}
            crate::test_support::submit_ready(&mut r,FramePacket {composite_all:false,reset_layers:false,..frame}).unwrap();
        }
        let report=r.current_display_memory_report();
        let photos=seed+1;
        settled.push([report.gpu_bytes,report.pinned_gpu_bytes]);
        assert!(report.pinned_gpu_bytes<=settled[0][1]+(32<<20),"{photos} photos: settled pins outgrow the view's demand: {} MiB with one photo, {} MiB now",
            settled[0][1]>>20,report.pinned_gpu_bytes>>20);
        assert!(r.next_frame()!=layer_render::FrameDemand::Work,"{photos} photos: the view settles");
    }
    let [plateau,last]=[settled[7][0],settled[9][0]];
    assert!(last<=plateau+(32<<20),"tracked bytes keep growing once the view's demand is resident: {} MiB with eight photos, {} MiB with ten",plateau>>20,last>>20);
}

fn noisy_photo(seed:u32,extent:[u32;2],profile:layer_core::color::ColorProfile)->AuthoredImage {
    let mut builder=SourceBuilder::new(extent,SourceInterpretation {channels:SourceChannels::Rgb,depth:SampleDepth::U8,profile,profile_assumed:false},64<<20).unwrap();
    let mut state=0x9e37_79b9_7f4a_7c15u64^u64::from(seed).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    let mut row=vec![0u8;extent[0] as usize*3];
    for y in 0..extent[1] {
        for (x,pixel) in (0..).zip(row.chunks_exact_mut(3)) {
            state^=state<<13;state^=state>>7;state^=state<<17;
            pixel.copy_from_slice(&[((x*200/extent[0]) as u8).wrapping_add(state as u8&31).wrapping_add(seed as u8*7),
                ((y*200/extent[1]) as u8).wrapping_add((state>>8) as u8&31),(((x+y)/40) as u8).wrapping_add((state>>16) as u8&31)]);
        }
        builder.push_row(&row).unwrap();
    }
    AuthoredImage::new(Arc::new(builder.finish().unwrap()))
}

fn photo_stack(extent:[u32;2],profile:layer_core::color::ColorProfile)->layer_core::Document {
    document(extent,(0..28).map(|seed|ImageObject::new(noisy_photo(seed,extent,profile.clone()))).collect()).0
}

fn fit_frame(doc:&layer_core::Document,viewport:[u32;2])->FramePacket<'_> {
    let extent=doc.composition().size;
    let scale=(viewport[0] as f32/extent[0] as f32).min(viewport[1] as f32/extent[1] as f32);
    let view=layer_render::ViewState {width_px:viewport[0],height_px:viewport[1],document_to_surface:[scale,0.,0.,scale,
        (viewport[0] as f32-extent[0] as f32*scale)/2.,(viewport[1] as f32-extent[1] as f32*scale)/2.]};
    FramePacket {view,..packet(doc.scene(),extent)}
}

const PHOTO:[u32;2]=[1024,768];

const PHOTO_TILES:u64=28*4*3;

struct Open {frames:usize,idle:usize}
fn progress(r:&WgpuRasterizer)->(u64,u64,(usize,usize,u64),u64,u64,u64) {
    let counts=crate::test_support::PassCounts::snapshot();
    (r.source_tiles.borrow().misses,r.moving_images.tile_builds,r.scene.as_ref().map_or((0,0,0),|scene|scene.object_results.work_progress()),counts.composite_commits,counts.unit_acceptances,r.navigator.revision)
}
fn open_photos(doc:&layer_core::Document,budget:u64)->(WgpuRasterizer,Open) {
    let frame=fit_frame(doc,[560,420]);
    assert_eq!(crate::display_mips::view_level(frame.view.document_to_surface,crate::display_mips::MAX_LEVEL),Some(0));
    let mut r=crate::memory::memory_tests::renderer(doc.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(budget),..Default::default()}).unwrap();
    let (signal,woken)=std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move||{let _=signal.send(());}));
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(60);
    let (mut open,mut first)=(Open {frames:0,idle:0},true);
    loop {
        r.poll_startup().unwrap();r.poll_pending(frame.view).unwrap();
        let before=progress(&r);
        let next=if first {frame} else {FramePacket {composite_all:false,reset_layers:false,..frame}};
        if r.can_submit() && let std::task::Poll::Ready(result)=r.submit_when_ready(next,None) {
            result.unwrap();first=false;open.frames+=1;
            r.wait_idle().unwrap();
            if progress(&r)==before {open.idle+=1;}
        }
        r.wait_idle().unwrap();
        assert!(std::time::Instant::now()<deadline,"the photo open settles within a minute: {} frames, {} without progress",open.frames,open.idle);
        if first {std::thread::sleep(std::time::Duration::from_millis(1));continue;}
        if r.next_frame()==layer_render::FrameDemand::Work {continue;}
        if r.settled() {return (r,open);}
        assert!(woken.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())).is_ok(),"an idle open waits on inputs whose completion never woke it: terms {:?} census {:?} held {}",
            r.outstanding_terms(),r.scene.as_ref().map(|scene|scene.tiers.schedule.census()),r.source_results.len());
    }
}

#[test]
fn a_cold_fit_open_of_twenty_eight_photos_decodes_each_tile_once_and_never_spins() {
    let doc=photo_stack(PHOTO,Default::default());
    let (r,open)=open_photos(&doc,8<<30);
    let decodes=r.source_tiles.borrow().misses;
    assert!(decodes<=2*PHOTO_TILES,"the open decodes each tile at most once for its pages and once for its mips: {decodes} for {PHOTO_TILES} tiles");
    assert_eq!(open.idle,0,"every Work frame of the open advances something");
    assert_eq!(r.moving_images.completed_images,28,"every photo's mips are complete when the view settles");
}

#[test]
fn an_icc_photo_stack_settles_under_a_grant_smaller_than_its_windows() {
    use layer_core::color::{ColorProfile,RgbSpace};
    let profile=ColorProfile::Icc(layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap().into());
    let doc=photo_stack(PHOTO,profile);
    let (r,open)=open_photos(&doc,384<<20);
    let decodes=r.source_tiles.borrow().misses;
    assert!(decodes<=2*PHOTO_TILES,"windows admitted with their inputs decode each tile at most twice: {decodes} for {PHOTO_TILES} tiles");
    assert_eq!(open.idle,0,"every Work frame of the open advances something: {} frames",open.frames);
}

#[test]
fn fit_photo_page_windows_each_read_only_their_own_tile() {
    let doc=photo_stack(PHOTO,Default::default());
    let (mut r,_)=open_photos(&doc,8<<30);
    let frame=fit_frame(&doc,[560,420]);
    let mut scene=r.scene.take().unwrap();
    let windows:Vec<_>=scene.object_results.request_windows().into_iter().filter(|(_,_,window)|window.side==1.).collect();
    assert!(!windows.is_empty(),"the level-0 view samples its photos in page windows");
    for (_,owner,window) in windows {
        let content=scene.object_spatial.content(frame.scene,owner).unwrap();
        let job=scene.collection_job(&r,frame,owner,content,window);
        let pages=super::super::object_cache::working_set(&job.child(0,job.part(0),r.empty_view.clone()).unwrap()).unwrap().2;
        assert_eq!(pages,1,"the page window at {:?} reads {pages} source tiles",window.origin);
    }
}

#[test]
fn a_fit_open_finds_object_windows_by_key_and_composes_a_waiting_cell_only_after_its_windows_settle() {
    let doc=photo_stack(PHOTO,Default::default());
    let before=crate::test_support::PassCounts::snapshot();
    let (r,open)=open_photos(&doc,8<<30);
    let counts=crate::test_support::PassCounts::snapshot().delta(before);
    let windows=r.scene.as_ref().unwrap().object_results.work_progress().0;
    let cells=u64::from(PHOTO[0].div_ceil(PAGE_SIZE)*PHOTO[1].div_ceil(PAGE_SIZE));
    assert!(counts.object_lookup_visits<=2*counts.object_lookups,
        "an object window lookup visits only the windows indexed under its content and cell: {} visits for {} lookups among {windows} windows",counts.object_lookup_visits,counts.object_lookups);
    assert!(2*counts.destination_units<=open.frames as u64*cells,
        "a cell waiting on object windows is composed again when they settle, not every frame: {} units over {} frames of {cells} cells",counts.destination_units,open.frames);
}
