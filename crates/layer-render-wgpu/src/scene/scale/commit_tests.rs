use super::*;
use crate::test_support::{GatedStore,PassCounts};

fn frame(doc:&Document,level:u32)->FramePacket<'_> {
    let mut frame=FramePacket {blend_space:layer_core::BlendSpace::Linear,..packet(doc.scene(),doc.composition().size)};
    let scale=1./(1<<level) as f32;
    frame.view.width_px=doc.composition().size[0].div_ceil(1<<level);
    frame.view.height_px=doc.composition().size[1].div_ceil(1<<level);
    frame.view.document_to_surface=[scale,0.,0.,scale,0.,0.];
    frame
}

fn committed_composite_pages(cache:&Cache)->Vec<(pages::Key,wgpu::Texture,pages::PageSet,pages::PageSet,pages::PageSet)> {
    cache.pages.lock().unwrap().retained_pages().into_iter().filter(|page|page.key.source.is_none() && !page.written.is_empty())
        .map(|page|(page.key,page.texture,page.written,page.current,page.exact)).collect()
}

#[test]
fn ready_units_hold_one_lease_per_source_storage() {
    let mut attempt=Attempt::inputs(1,1,[PAGE_SIZE;2],0,false,false,None);
    let source=attempt.doc.artwork.paint.iter().filter_map(|(_,_,paint)|paint.base.as_ref()).next().unwrap().image.storage().clone();
    let mut inputs=crate::scene::sources::Inputs::default();
    for _ in 0..64 {inputs.image_for(&source,[0;2],attempt.r.document_color().space,crate::scene::sources::SourceUse::Exact);}
    let mut unit=Unit {sources:inputs.accept_query(&mut attempt.r,Some(&attempt.scene)).unwrap(),..Default::default()};
    let read=crate::scene::sources::accepted_image(&unit.sources,&source,[0;2]).unwrap();
    let output=attempt.scene.alloc(&attempt.r,wgpu::Color::TRANSPARENT).unwrap();let start=attempt.scene.jobs.len();
    for _ in 0..64 {
        attempt.scene.draw(&attempt.r,output,read.tile.view.clone(),None,[0.,0.,256.,256.],[0.;4],false,Convert::layers(frame(&attempt.doc,0)));
    }
    let mut encoder=submission::CommandEncoder::new(&attempt.r.device,&Default::default());
    assert!(unit.ready(&mut attempt.scene,&mut attempt.r,&mut encoder,start,Class::Visible).unwrap());
    assert_eq!(unit.sources.len(),1,"repeated material reads lease their physical source once per accepted unit");
}

#[test]
fn a_ready_parent_commits_before_unrelated_cold_source_preparation_spends_its_frame() {
    let extent=[5*512,512];let middle=PixelRect::new(1024,0,1536,512);
    let mut attempt=Attempt::inputs(0,28,extent,1,false,false,Some(middle));
    let frame=frame(&attempt.doc,1);
    let ids=attempt.doc.scene().order().iter().map(|owner|attempt.doc.scene().source_target(*owner).unwrap()).collect::<Vec<_>>();
    assert_eq!(ids.len(),28);
    assert!(ids.len()*(page_coordinates(PixelRect::full(extent)).count()-page_coordinates(middle).count())>128);
    attempt.scene.scale_sources.prepare(&attempt.r,frame,&[]);
    let mut ready=Vec::new();
    for &id in &ids {
        attempt.scene.begin_frame();
        let mut encoder=submission::CommandEncoder::new(&attempt.r.device,&Default::default());
        attempt.cache.prepare_source(&mut attempt.scene,&mut attempt.r,frame,&mut encoder,id,SourceRequest {
            plan:display_mips::Plan::window(extent,1,middle),required:middle.into(),covered:PixelRect::EMPTY,
        },Class::Prefetch,&mut Scene::begin_work(false)).unwrap();
        attempt.r.uploads.finish(&encoder);encoder.submit(&attempt.r.queue);crate::test_support::complete(&attempt.r);
        let key=attempt.scene.scale_sources.entries[&id].key(&attempt.r,id,1,[2,0]);
        ready.push(attempt.cache.pages.lock().unwrap().get(&key,middle,pages::Accuracy::Display,key.level,crate::residency::Class::Visible).expect("each direct middle-parent source input must be committed before the attempt"));
    }
    attempt.scene.scale_sources.prepare(&attempt.r,FramePacket {composite_all:false,..frame},&[]);
    assert_eq!(ready.len(),28);
    assert!(ready.iter().map(|image|image.bytes()).sum::<u64>()<=attempt.r.memory.display_owner().grant().gpu_bytes);
    let view=frame.view;
    let (result,_)=attempt.attempt();
    assert!(result.is_ok() || matches!(result,Err(GpuRasterError::NotReady)),"{result:?}");
    for (index,&id) in ids.iter().enumerate() {
        let key=attempt.scene.scale_sources.entries[&id].key(&attempt.r,id,1,[2,0]);
        let input=attempt.cache.pages.lock().unwrap().get(&key,middle,pages::Accuracy::Display,key.level,crate::residency::Class::Visible).expect("unrelated gathering cannot invalidate the isolated ready inputs");
        assert!(Arc::ptr_eq(input.image(),ready[index].image()));
    }
    let coverage=attempt.cache.coverage_snapshot();
    assert!(coverage.ready(middle,pages::Accuracy::Display),"the already ready nearest parent must commit before unrelated cold gathering consumes its frame");
    for x in [0,1,3,4] {assert!(!coverage.ready(PixelRect::new(x*512,0,(x+1)*512,512),pages::Accuracy::Display));}
    let key=attempt.cache.page_key(attempt.cache.graph.root.clone().unwrap(),middle);
    let image=attempt.cache.pages.lock().unwrap().get(&key,middle,pages::Accuracy::Display,key.level,crate::residency::Class::Visible).unwrap();
    let actual=pixels(&attempt.r,&image.texture);let alpha=193./255.;
    let rgb=layer_core::color::RgbSpace::Srgb;
    for [x,y] in [[0,0],[128,128],[255,255]] {
        let mut expected=[0.;4];
        for seed in (0..28).rev() {
            let mut source=[0.;4];
            for yy in 0..2 {for xx in 0..2 {
                let pixel=[31+(seed*23%180),(1024+x*2+xx)%173,(y*2+yy)%191];
                for c in 0..3 {source[c]+=rgb.decode(f64::from(pixel[c])/255.) as f32*alpha/4.;}
            }}
            source[3]=alpha;
            for c in 0..4 {expected[c]=source[c]+expected[c]*(1.-alpha);}
        }
        let expected=committed_color(image.texture.format(),expected);
        assert!(crate::test_support::max_error(&[actual[(y*256+x) as usize]],&[expected])<2e-5,"ready parent pixels must contain all28 real source inputs");
    }
    attempt.r.scale_display=Some(attempt.cache);attempt.r.scene=Some(attempt.scene);
    let mut presenter=crate::test_support::float_presenter(&attempt.r);
    let output=create_color_target(&attempt.r.device,[view.width_px,view.height_px],"ready parent presentation");
    attempt.r.presentation.showing=false;
    presenter.present(&mut attempt.r,&output.1,view,[0.18,0.18,0.18,1.]).unwrap();
    let surround=pixels(&attempt.r,&output.0);attempt.r.presentation.showing=true;
    presenter.present(&mut attempt.r,&output.1,view,[0.18,0.18,0.18,1.]).unwrap();
    let shown=pixels(&attempt.r,&output.0);
    assert_ne!(shown[128*1280+640],surround[128*1280+640]);
    assert_eq!(shown[128*1280+128],surround[128*1280+128],"gated parents remain surround rather than sampling unwritten pixels");
}

#[test]
fn a_ready_display_page_commits_during_continuous_input_without_an_idle_frame() {
    for held in [true,false] {
    let extent=[PAGE_SIZE*(1+8*u32::from(held)),PAGE_SIZE];
    let damaged=PixelRect::new(extent[0]-PAGE_SIZE,0,extent[0],PAGE_SIZE);
    let mut attempt=Attempt::inputs(0,20,extent,0,false,false,Some(PixelRect::full(extent)));
    let initial=frame(&attempt.doc,0);
    let targets=attempt.doc.scene().order().iter().filter_map(|owner|attempt.doc.scene().source_target(*owner))
        .filter(|target|matches!(target,SourceTarget::Paint(_))).collect::<Vec<_>>();
    assert_eq!(targets.len(),20);
    attempt.scene.scale_sources.prepare(&attempt.r,initial,&[]);
    prepare_fixture_frame(&attempt.r,initial,&attempt.scene);
    attempt.cache.prepare_graph(&attempt.r,initial,&attempt.scene).unwrap();
    for &target in &targets {
        attempt.scene.begin_frame();
        let mut encoder=submission::CommandEncoder::new(&attempt.r.device,&Default::default());
        attempt.cache.prepare_source(&mut attempt.scene,&mut attempt.r,initial,&mut encoder,target,SourceRequest {
            plan:display_mips::Plan::at(extent,0),required:PixelRect::full(extent).into(),covered:PixelRect::EMPTY,
        },Class::Prefetch,&mut Scene::begin_work(false)).unwrap();
        attempt.r.uploads.finish(&encoder);encoder.submit(&attempt.r.queue);crate::test_support::complete(&attempt.r);
        for coordinate in page_coordinates(PixelRect::full(extent)) {
            let key=attempt.scene.scale_sources.entries[&target].key(&attempt.r,target,0,coordinate);
            let region=pages::bounds(coordinate,0,extent);
            assert!(attempt.cache.pages.lock().unwrap().get(&key,region,pages::Accuracy::Display,key.level,crate::residency::Class::Visible).is_some(),
                "all twenty real source pages must be ready before continuous input: {coordinate:?}, cache_limit={}, free={}",attempt.r.memory.display_owner().grant().gpu_bytes,attempt.r.source_tiles.borrow().free_slots());
        }
    }
    assert!(committed_composite_pages(&attempt.cache).is_empty());
    if held {
        attempt.cache.prepare_root(&attempt.r,initial,&attempt.scene.scale_sources,false).unwrap();
        let mut encoder=submission::CommandEncoder::new(&attempt.r.device,&Default::default());
        for x in 0..8 {
            let previous=pages::bounds([x,0],0,extent);
            let requests=attempt.cache.region_sources(&attempt.scene,&attempt.r,initial,previous,false);
            let root=attempt.cache.graph.root.clone().unwrap();
            assert!(attempt.cache.accept_parent(&mut attempt.scene,&mut attempt.r,initial,previous,&requests,0,false,std::slice::from_ref(&root),Root::Store,false,crate::residency::Class::Visible,crate::residency::Class::Visible,&mut encoder).unwrap().is_some());
        }
        let root=attempt.cache.graph.root.clone().unwrap();let key=attempt.cache.page_key(root,PixelRect::full(extent));
        let accepted=attempt.cache.pages.lock().unwrap().accepted_coordinates(&key,0).collect::<std::collections::HashSet<_>>();
        assert_eq!(accepted,(0..8).map(|x|[x,0]).collect());
        attempt.r.uploads.finish(&encoder);encoder.submit(&attempt.r.queue);crate::test_support::complete(&attempt.r);
    }
    attempt.r.scale_display=Some(attempt.cache);attempt.r.scene=Some(attempt.scene);
    let mut current_frames=Vec::new();
    for index in 0..30 {
        let mut dab=crate::tests::test_dab([(extent[0]-PAGE_SIZE) as f32+112.+index as f32,128.],[0.9,0.02,0.1,0.8],1.);
        dab.radii=[8.;2];
        let mut batch=dab_batch(targets[0],crate::layer_tests::preset_style(DefaultBrushPreset::GPen),dab.bounds());
        batch.stroke_id=layer_core::StrokeId(1);batch.stroke_start=index==0;batch.stroke_end=false;batch.material_update=index;
        let mut input=FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),composite_all:false,
            ..frame(&attempt.doc,0)};
        if held {input.view.document_to_surface=[0.9,0.,0.,0.9,-16.,8.];}
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(30);
        while !attempt.r.can_submit() || !attempt.r.raster_dependencies_ready(input,None) {
            assert!(std::time::Instant::now()<deadline,"the fixture must accept each nonempty input packet without an idle submission");
            crate::test_support::complete(&attempt.r);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        attempt.r.submit(input).unwrap();
        crate::test_support::complete(&attempt.r);
        let cache=attempt.r.scale_display.as_ref().unwrap();
        assert!(cache.evaluation==Evaluation::Display);
        if cache.coverage_snapshot().ready(damaged,pages::Accuracy::Display) || cache.displays(damaged) {current_frames.push(index+1);}
    }
    let idle=FramePacket {composite_all:false,..frame(&attempt.doc,0)};
    crate::test_support::submit_ready(&mut attempt.r,idle).unwrap();crate::test_support::complete(&attempt.r);
    let cache=attempt.r.scale_display.as_ref().unwrap();
    let idle_current=cache.coverage_snapshot().ready(damaged,pages::Accuracy::Display) || cache.displays(damaged);
    assert!(idle_current,"the same ready twenty-layer page must be capable of an idle commit");
    assert!(current_frames.first().is_some_and(|frame|*frame<=2),
        "a ready damaged page must commit current within two input frames, without waiting for idle: held={held}, current_input_frames={current_frames:?}, idle_current={idle_current}");
    }
}


struct Attempt {
    store:GatedStore,
    r:WgpuRasterizer,
    doc:Document,
    scene:Box<Scene>,
    cache:Cache,
    commands:RegionRecords,
    cold:Vec<(usize,[u32;2])>,
}
impl Attempt {
    fn new(effect:bool)->Self {
        Self::configured(effect,false)
    }
    fn configured(effect:bool,staged:bool)->Self {
        Self::inputs(4,3,[PAGE_SIZE;2],0,effect,staged,None)
    }
    fn inputs(warm:usize,cold_count:usize,extent:[u32;2],level:u32,effect:bool,staged:bool,ready:Option<PixelRect>)->Self {
        let mut doc=document_at(extent);
        composition_mut(&mut doc).blend=layer_core::BlendSpace::Linear;
        if ready.is_some() {composition_mut(&mut doc).color.depth=layer_core::color::SampleDepth::F32;}
        let sources:Vec<_>=(0..warm+cold_count).map(|seed|rgba8_source(extent,move|x,y|
            [31+(seed*23%180) as u8,(x%173) as u8,(y%191) as u8,193])).collect();
        let owners:Vec<_>=sources.iter().map(|source|paint_occurrence(&mut doc,"Commit input",Some(source.clone()))).collect();
        let mut lower=owners[..warm].to_vec();
        if effect {
            let handle=effect_occurrence(&mut doc,layer_core::EffectInstance::new(crate::tests::fixture("exposure").program()),"Commit effect");
            set_effect_value(&mut doc,handle,"exposure",layer_core::EffectValue::Number(0.5));
            doc.artwork.occurrences.get_mut(handle).unwrap().attachment=Attachment::Effect;
            lower.insert(0,handle);
        }
        let mut entries=owners[warm..].to_vec();
        if !lower.is_empty() {entries.push(stack_occurrence(&mut doc,"Ready lower branch",lower));}
        set_root_entries(&mut doc,entries);
        let reference=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut r=if staged {crate::test_support::staged_renderer(&reference,doc.composition().color)}else{reference};
        pollster::block_on(r.compilation.prepare_all([r.scene_pipelines.source.for_format(wgpu::TextureFormat::Rgba8Unorm)])).unwrap();
        pollster::block_on(r.compilation.prepare_all(r.scene_pipelines.for_scene(&r,doc.scene(),&[]))).unwrap();
        pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.constant.2,&r.scene_pipelines.scale.full.reduce])).unwrap();
        r.ensure_document_metadata(extent,doc.scene()).unwrap();
        working_scene_frame(&mut r,frame(&doc,level));
        if let Some(region)=ready {
            let slots=sources.len()*page_coordinates(region).count();
            crate::memory::memory_tests::declare_source_budget(&mut r,frame(&doc,level),slots+16,16<<20,44<<20);
        }
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        for (index,source) in sources.iter().enumerate() {
            let coordinates=if index<warm {vec![[0;2]]} else {ready.map(|region|page_coordinates(region).collect()).unwrap_or_default()};
            for coordinate in coordinates {
                let pixels=crate::prepare_image_tile(source,coordinate,r.device.working_space(),&r.device.source_samples,None).unwrap();
                let tile=Scene::decode_prepared_for(&mut r,&mut encoder,source,coordinate,pixels,crate::scene::sources::SourceUse::Exact,crate::residency::Class::Visible).unwrap();
                Scene::promote_source(&mut r,&mut encoder,tile).unwrap();
            }
            if ready.is_some() {
                r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
                encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            }
        }
        r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
        if let Some(region)=ready {
            let prepared=sources.iter().flat_map(|source|page_coordinates(region).map(move|coordinate|(source,coordinate)))
                .filter(|(source,coordinate)|r.source_tiles.borrow().prepared_view(source,*coordinate).is_some()).count();
            assert_eq!(prepared,sources.len()*page_coordinates(region).count(),
                "fixture raw inputs must all survive construction: free={}, scene_grant={}",
                r.source_tiles.borrow().free_slots(),r.memory.display_grant());
        }
        let store=GatedStore::new(&layer_core::raster_storage::RetainedTiles::from_sources(sources[warm..].iter().map(Arc::as_ref)),false);
        let scene=r.scene.take().unwrap_or_else(||Scene::new(&r).unwrap());
        prepare_fixture_frame(&r,frame(&doc,level),&scene);
        let cache=Cache::new(&r,Request {plan:display_mips::Plan::at(extent,level),evaluation:Evaluation::Display},doc.scene().order().len());
        let commands=RegionRecords::new(&r);
        let mut cold:Vec<_>=sources[warm..].iter().flat_map(|source|page_coordinates(PixelRect::full(extent))
            .map(move|coordinate|(Arc::as_ptr(source) as usize,coordinate))).collect();cold.sort();
        Self {store,r,doc,scene,cache,commands,cold}
    }
    fn attempt(&mut self)->(Result<PixelRect,GpuRasterError>,PassCounts) {
        let Self {r,doc,scene,cache,commands,..}=self;
        let extent=doc.composition().size;
        let frame=frame(doc,cache.plan.level);
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        scene.begin_frame();commands.begin();scene.scale_sources.prepare(r,frame,&[]);
        let before=PassCounts::snapshot();
        let source_updates=scene.scale_sources.entries.values().map(|source|source.updates).sum::<u64>();
        let before_coverage=cache.coverage_snapshot();
        let before_pages=committed_composite_pages(cache);
        crate::scene::scale::tests::prepare_fixture_frame(r,frame,scene);
        let prepared=cache.prepare_graph(r,frame,scene);
        let before_branches=cache.commit_state();
        let result=prepared.and_then(|()|
            cache.render_graph(scene,r,frame,PixelRect::full(extent),&mut Encoding {encoder:&mut encoder,commands, retained:0, budget:&mut Scene::begin_work(false)},Destination::View,None));
        let counts=PassCounts::snapshot().delta(before);
        cache.unchanged=true;
        if result.is_ok() {
            scene.encode_jobs(r,&mut encoder).unwrap();
            if result.as_ref().is_ok_and(|region|region.is_empty()) {
                let prepared=scene.scale_sources.entries.values().map(|source|source.updates).sum::<u64>()-source_updates;
                assert!(counts.total<=counts.normalizations+prepared,"an unready composite can encode only completed source-page units: {counts:?}, sources={prepared}");
            }
            r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(r);
        } else {
            let prepared=scene.scale_sources.entries.values().map(|source|source.updates).sum::<u64>()-source_updates;
            assert!(counts.total<=counts.normalizations+prepared,"an unavailable composite can encode only completed source-page units: {counts:?}, sources={prepared}");
            scene.discard_unencoded_jobs();commands.begin();r.uploads.finish(&encoder);
            if encoder.has_work() {encoder.submit(&r.queue);crate::test_support::complete(r);} else {drop(encoder);}
        }
        if result.as_ref().is_ok_and(|region|region.is_empty()) || matches!(result,Err(GpuRasterError::NotReady)) {
            assert_coverage_eq(&before_coverage,&cache.coverage_snapshot());
            assert_eq!(cache.commit_state(),before_branches);
            let after_pages=committed_composite_pages(cache);
            assert_eq!(after_pages.len(),before_pages.len());
            assert!(before_pages.iter().all(|page|after_pages.contains(page)),"an unready attempt changed committed keyed coverage");
        }
        (result,counts)
    }
    fn assert_uncommitted(&self) {
        let coverage=self.cache.coverage_snapshot();
        assert!(coverage.written.is_empty() && coverage.matching.is_empty() && coverage.exact.is_empty());
        assert!(committed_composite_pages(&self.cache).is_empty());
        assert!(self.cache.commit_state().is_empty());
        assert!(self.cache.pages.lock().unwrap().retained_pages().iter().filter(|page|page.key.source.is_some()).all(|page|page.current.is_empty()));
    }
    fn finish(&mut self) {
        self.store.gate.release();
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(30);
        while !self.cache.coverage_complete() {
            let (result,_)=self.attempt();
            assert!(result.is_ok() || matches!(result,Err(GpuRasterError::NotReady)),"{result:?}");
            assert!(std::time::Instant::now()<deadline,"opened inputs must commit the same output");
            crate::test_support::settle_decodes(&mut self.r);
        }
        assert!(self.cache.coverage_complete());
        let mut reference=WgpuRasterizer::new_native_headless(self.doc.composition().color).unwrap();
        crate::test_support::submit_published(&mut reference,frame(&self.doc,self.cache.plan.level));
        let expected=pixels(&reference,reference.scale_display.as_ref().unwrap().texture());
        let actual=pixels(&self.r,self.cache.texture());
        assert_eq!(actual.len(),expected.len());
        assert!(crate::test_support::max_error(&actual,&expected)<2e-5);
    }
    fn footprint(&self)->(usize,u64,u64,usize,u64,u64,[u64;2],Vec<wgpu::Texture>) {
        let sources=self.r.source_tiles.borrow();
        (sources.allocated_slots(),sources.gpu_bytes(),sources.cpu_bytes(),self.scene.pool.len(),
            self.scene.pool_bytes(),self.cache.storage_bytes(),self.r.uploads.memory_bytes(),
            self.cache.output.iter().map(|image|image.texture.clone()).collect())
    }
}

#[path="independent_page_tests.rs"]
mod independent_pages;

#[path="preparation_tests.rs"]
mod preparation_dependencies;

#[test]
fn display_cold_leaf_discards_all_prefix_work_and_requests_every_named_input() {
    let mut attempt=Attempt::new(false);
    let revision=(attempt.r.artwork_revision,attempt.r.composite_revision);
    let (result,counts)=attempt.attempt();
    assert!(matches!(result,Ok(region) if region.is_empty()) || matches!(result,Err(GpuRasterError::NotReady)),"{result:?}");
    assert_eq!(counts.total,0,"a cold upper leaf cannot encode a ready lower prefix");
    attempt.assert_uncommitted();
    assert_eq!((attempt.r.artwork_revision,attempt.r.composite_revision),revision);
    let requested=attempt.r.moving_decode.as_ref().map(|(_,queue)|queue.requested_image_tiles()).unwrap_or_default();
    for expected in &attempt.cold {assert!(requested.contains(expected),"missing named input {expected:?}; requested={requested:?}");}
    attempt.finish();
}

#[test]
fn discarded_display_attempt_never_queues_effect_parameter_writes() {
    let mut attempt=Attempt::new(true);
    let (result,_)=attempt.attempt();
    assert!(matches!(result,Ok(region) if region.is_empty()) || matches!(result,Err(GpuRasterError::NotReady)),"{result:?}");
    let buffers=attempt.scene.effects.parameter_buffers();
    assert!(!buffers.is_empty(),"the discarded prefix must prepare actual effect parameters");
    for buffer in buffers {
        let bytes=pollster::block_on(crate::local_tone::read_buffer_async(&attempt.r.device,&attempt.r.queue,buffer)).unwrap();
        assert!(bytes.iter().all(|byte|*byte==0),"a discarded unit wrote its parameter buffer before commit");
    }
    attempt.assert_uncommitted();
    attempt.finish();
}

#[test]
fn one_thousand_discarded_display_attempts_reuse_all_claimed_allocations() {
    let mut attempt=Attempt::new(true);
    let (result,counts)=attempt.attempt();
    assert!(matches!(result,Ok(region) if region.is_empty()) || matches!(result,Err(GpuRasterError::NotReady)),"{result:?}");
    assert_eq!(counts.total,0);
    attempt.assert_uncommitted();
    let footprint=attempt.footprint();
    for discarded in 0..1000 {
        let (result,_)=attempt.attempt();
        assert!(matches!(result,Ok(region) if region.is_empty()) || matches!(result,Err(GpuRasterError::NotReady)),"attempt {discarded}: {result:?}");
        assert_eq!(attempt.footprint(),footprint,"attempt {discarded} grew discarded allocation claims");
    }
    attempt.assert_uncommitted();
    attempt.finish();
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn pending_display_shader_does_not_hide_other_missing_decode_inputs() {
    let mut attempt=Attempt::configured(false,true);
    let compiler=attempt.r.startup.as_ref().unwrap().compiler.clone();
    let (entered,entry)=std::sync::mpsc::channel();
    let (release,held)=std::sync::mpsc::channel();
    compiler.enqueue(startup::BRUSH,move|| {let _=entered.send(());let _=held.recv_timeout(std::time::Duration::from_secs(10));Ok(())});
    entry.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    let (result,counts)=attempt.attempt();
    let pending=compiler.pending();
    let compose_pending=!attempt.r.scene_pipelines.scale.full.compose.ready();
    let requested=attempt.r.moving_decode.as_ref().map(|(_,queue)|queue.requested_image_tiles()).unwrap_or_default();
    release.send(()).unwrap();
    assert!(matches!(result,Ok(region) if region.is_empty()) || matches!(result,Err(GpuRasterError::NotReady)),"{result:?}");
    assert!(pending>0,"the fixture must retain a pending shader while inputs are discovered");
    assert!(compose_pending,"the required Display composition pipeline must still be pending");
    assert_eq!(counts.total,0);
    for expected in &attempt.cold {assert!(requested.contains(expected),"shader wait hid input {expected:?}: {requested:?}");}
    attempt.assert_uncommitted();
    attempt.finish();
}

#[test]
fn twenty_four_delayed_display_layers_commit_with_bounded_attempts_uploads_and_composition() {
    let mut attempt=Attempt::inputs(0,24,[PAGE_SIZE;2],0,false,false,None);
    let frame=frame(&attempt.doc,0);
    crate::memory::memory_tests::declare_source_budget(&mut attempt.r,frame,64,64<<20,0);
    let before=PassCounts::snapshot();
    let uploads=attempt.r.source_cache_work()[1];
    let mut committed=false;
    for readiness in 0..3 {
        let (result,_)=attempt.attempt();
        if attempt.cache.coverage_complete() {assert!(result.is_ok(),"{result:?}");committed=true;break;}
        assert!(matches!(result,Ok(region) if region.is_empty()) || matches!(result,Err(GpuRasterError::NotReady)),"attempt {readiness}: {result:?}");
        attempt.assert_uncommitted();
        attempt.store.gate.release();
        crate::test_support::settle_decodes(&mut attempt.r);
    }
    let counts=PassCounts::snapshot().delta(before);
    assert!(committed,"fitting L24 must commit within three controlled readiness attempts: {counts:?}");
    let needed=attempt.cold.len() as u64;
    let actual=attempt.r.source_cache_work()[1]-uploads;
    assert!(actual*100<=needed*105,"L24 uploaded {actual} pages for {needed} needed pages");
    assert!(counts.total-counts.normalizations<=3,"L24 composition passes exclude {} prerequisite normalizations: {counts:?}",counts.normalizations);
    attempt.finish();
}

#[test]
fn a_region_larger_than_the_source_allowance_finishes_in_page_units() {
    let mut attempt=Attempt::inputs(0,1,[4*PAGE_SIZE,PAGE_SIZE],1,false,false,None);
    let dummy=rgba8_source([PAGE_SIZE;2],|_,_|[31,67,113,193]);
    let prepared=crate::prepare_image_tile(&dummy,[0;2],attempt.r.device.working_space(),&attempt.r.device.source_samples,None).unwrap();
    let mut encoder=crate::submission::CommandEncoder::new(&attempt.r.device,&Default::default());
    Scene::decode_prepared_for(&mut attempt.r,&mut encoder,&dummy,[0;2],prepared,crate::scene::sources::SourceUse::Exact,crate::residency::Class::Visible).unwrap();
    attempt.r.uploads.finish(&encoder);encoder.submit(&attempt.r.queue);crate::test_support::complete(&attempt.r);
    attempt.store.gate.release();
    let SourceTarget::Paint(paint)=source_at(&attempt.doc,0) else {unreachable!()};
    let source=attempt.doc.artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.storage().clone();
    let mut encoder=crate::submission::CommandEncoder::new(&attempt.r.device,&Default::default());
    let mut encoded_bytes=0;
    for coordinate in page_coordinates(PixelRect::full(attempt.doc.composition().size)) {
        let prepared=crate::prepare_image_tile(&source,coordinate,attempt.r.device.working_space(),&attempt.r.device.source_samples,None).unwrap();
        let tile=Scene::decode_prepared(&mut attempt.r,&mut encoder,&source,coordinate,prepared).unwrap();
        assert_eq!(tile.texture.format(),wgpu::TextureFormat::Rgba8Unorm);
        encoded_bytes+=u64::from(tile.texture.width())*u64::from(tile.texture.height())*4;
    }
    attempt.r.uploads.finish(&encoder);encoder.submit(&attempt.r.queue);crate::test_support::complete(&attempt.r);
    assert_eq!(encoded_bytes,crate::scene::sources::NORMALIZED_PAGE_BYTES);
    attempt.finish();
    reset_page_store(&mut attempt.cache);
    attempt.cache.unchanged=false;
    attempt.cache.graph=attempt.cache.graph.without_pixels();
    attempt.finish();
    reset_page_store(&mut attempt.cache);
    attempt.cache.unchanged=false;
    attempt.cache.graph=attempt.cache.graph.without_pixels();
    let (result,recovered)=attempt.attempt();
    assert!(result.is_ok(),"a recovered region must commit with resident inputs: {result:?}");
    assert!(attempt.cache.coverage_complete());
    let request=Request {plan:attempt.cache.plan,evaluation:Evaluation::Display};
    attempt.cache=Cache::new(&attempt.r,request,attempt.doc.scene().order().len());
    let (result,fresh)=attempt.attempt();
    assert!(result.is_ok(),"fresh high-grant cache: {result:?}");
    assert!(attempt.cache.coverage_complete());
    assert_eq!(recovered.total-recovered.normalizations,fresh.total-fresh.normalizations,
        "prior low-grant preparation cannot retain extra composition work: recovered={recovered:?}, fresh={fresh:?}");
    attempt.finish();
}

#[test]
fn one_thousand_changed_effect_discard_attempts_release_obsolete_parameter_storage() {
    let mut attempt=Attempt::new(true);
    let effect=*attempt.doc.scene().order().iter().find(|handle|attempt.doc.scene().effect(**handle).is_some()).unwrap();
    let change=|attempt:&mut Attempt,value:f32| {
        set_effect_value(&mut attempt.doc,effect,"exposure",layer_core::EffectValue::Number(value));
        let (result,counts)=attempt.attempt();
        assert!(matches!(result,Ok(region) if region.is_empty()) || matches!(result,Err(GpuRasterError::NotReady)),"{result:?}");
        assert_eq!(counts.total,0);
        attempt.assert_uncommitted();
    };
    change(&mut attempt,0.);change(&mut attempt,1.);
    let storage=attempt.scene.effects.storage_bytes();
    let allocated=|attempt:&Attempt|attempt.r.device.generate_allocator_report()
        .map(|report|report.allocations.iter().map(|allocation|allocation.size).sum::<u64>());
    let bytes=allocated(&attempt);
    for discarded in 0..1000 {
        change(&mut attempt,2.+discarded as f32/1000.);
        assert!(attempt.scene.effects.storage_bytes()<=storage,"discard {discarded} retains obsolete parameter storage");
        if let Some(limit)=bytes {
            assert!(allocated(&attempt).unwrap()<=limit,"discard {discarded} grows live GPU allocations beyond two primed parameter cases");
        }
        attempt.assert_uncommitted();
    }
    attempt.finish();
}

#[test]
fn wanted_page_inputs_release_cancelled_leases_and_preserve_source_working_capacity() {
    let extent=[32*PAGE_SIZE,12*PAGE_SIZE];
    let mut doc=document_at(extent);
    composition_mut(&mut doc).blend=layer_core::BlendSpace::Linear;
    let warm=rgba8_source(extent,|x,y|[31+(x/PAGE_SIZE) as u8,67+(y/PAGE_SIZE) as u8,113,193]);
    let cold=rgba8_source(extent,|x,y|[99+(x/PAGE_SIZE) as u8,41+(y/PAGE_SIZE) as u8,83,193]);
    let bottom=paint_occurrence(&mut doc,"Ready wanted inputs",Some(warm.clone()));
    let top=paint_occurrence(&mut doc,"Held wanted inputs",Some(cold.clone()));
    set_root_entries(&mut doc,vec![top,bottom]);
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    r.ensure_document_metadata(extent,doc.scene()).unwrap();
    let mut frame=frame(&doc,0);
    frame.view.width_px=8*PAGE_SIZE;frame.view.height_px=8*PAGE_SIZE;
    working_scene_frame(&mut r,frame);
    pollster::block_on(r.compilation.prepare_all(crate::startup::SourceKinds::for_source(&warm).pipelines(&r.scene_pipelines.source))).unwrap();
    crate::memory::memory_tests::declare_source_budget(&mut r,frame,64,16<<20,44<<20);
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    for y in 0..8 {for x in 0..8 {
        let prepared=crate::prepare_image_tile(&warm,[x,y],r.device.working_space(),&r.device.source_samples,None).unwrap();
        Scene::decode_prepared_for(&mut r,&mut encoder,&warm,[x,y],prepared,crate::scene::sources::SourceUse::Exact,crate::residency::Class::Visible).unwrap();
    }}
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    let gate=GatedStore::new(&layer_core::raster_storage::RetainedTiles::from_sources([cold.as_ref()]),false);
    let (wake,wakes)=std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move||{let _=wake.send(());}));
    let mut scene=r.scene.take().unwrap_or_else(||Scene::new(&r).unwrap());
    let mut cache=Cache::new(&r,Request {plan:display_mips::Plan::window(extent,0,PixelRect::full([8*PAGE_SIZE;2])),evaluation:Evaluation::Display},2);
    let mut commands=RegionRecords::new(&r);
    let mut changed=doc.clone();occurrence_mut(&mut changed,0).opacity=0.5;
    let mut full=doc.clone();composition_mut(&mut full).color.depth=SampleDepth::F32;
    let mut changed_full=changed.clone();composition_mut(&mut changed_full).color.depth=SampleDepth::F32;
    let coordinates:Vec<_>=(0..12).flat_map(|y|(0..32).map(move|x|[x,y])).filter(|[x,y]|*x>=8 || *y>=8).collect();
    let mut previous:Option<(pages::Key,Evaluation)>=None;
    for step in 0..300u32 {
        gate.gate.close();
        let coordinate=if step==0 {[0,0]}else{coordinates[(step-1) as usize]};
        let current=match (step/5%2,step/7%2) {(0,0)=>&doc,(1,0)=>&changed,(0,1)=>&full,_=>&changed_full};
        let mut frame=self::frame(current,0);
        let region=if step==0 {PixelRect::full([8*PAGE_SIZE;2])}else{page_rect(coordinate)};
        frame.view.width_px=region.width();frame.view.height_px=region.height();
        frame.view.document_to_surface=[1.,0.,0.,1.,-(region.min_x() as f32),-(region.min_y() as f32)];
        if r.document_color()!=current.composition().color {
            r.initialize_native(current.composition().color).unwrap();
            scene=Scene::new(&r).unwrap();
            working_scene_frame(&mut r,frame);
            crate::memory::memory_tests::declare_source_budget(&mut r,frame,64,16<<20,44<<20);
        }
        r.ensure_document_metadata(extent,current.scene()).unwrap();
        assert_eq!(r.document_color(),current.composition().color);
        let request=Request {plan:display_mips::Plan::window(extent,0,region),
            evaluation:match step%3 {0=>Evaluation::Display,1=>Evaluation::Tiled(64),_=>Evaluation::Native}};
        let retained=cache.pages.clone();
        (cache,_)=Cache::select(Some(cache),&r,frame,request,false);
        assert!(Arc::ptr_eq(&retained,&cache.pages),"camera and metadata changes must reconcile the same accepted-input Store");
        scene.begin_frame();commands.begin();scene.scale_sources.prepare(&r,frame,&[]);
        crate::scene::scale::tests::prepare_fixture_frame(&r,frame,&scene);
        let prepared=cache.prepare_graph(&r,frame,&scene);
        assert!(prepared.is_ok() || matches!(prepared,Err(GpuRasterError::NotReady)),"step {step}: {prepared:?}");
        if prepared.is_ok() {cache.retain_shared_work(&r,frame,&scene,&r.navigator);}
        if step>0 {
            assert_eq!(cache.pages.lock().unwrap().pinned_bytes(),0,"step {step}: cancelled accepted units must release before current-view admission");
            assert_eq!(r.source_tiles.borrow().leased_bytes(),0,"step {step}: cancelled accepted source leases survived camera/mode/key replacement");
        }
        if prepared.is_err() {continue;}
        let key=cache.page_key(cache.graph.root.clone().unwrap(),region);
        assert_eq!(key.format,if current.composition().color.depth.is_float() {wgpu::TextureFormat::Rgba32Float}else{wgpu::TextureFormat::Rgba16Float});
        if let Some((previous_key,previous_mode))=&previous {
            if step%5!=0 && step%7!=0 {
                assert!(key.expression==previous_key.expression,"camera/mode changes must retain content identity");
            }
            assert!(cache.evaluation!=*previous_mode,"each new camera also changes evaluation mode");
        }
        previous=Some((key.clone(),cache.evaluation));
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        let prepared=crate::prepare_image_tile(&warm,coordinate,r.device.working_space(),&r.device.source_samples,None).unwrap();
        Scene::decode_prepared_for(&mut r,&mut encoder,&warm,coordinate,prepared,crate::scene::sources::SourceUse::Exact,crate::residency::Class::Visible).unwrap();
        let result=cache.render_graph(&mut scene,&mut r,frame,region,&mut Encoding {encoder:&mut encoder,commands:&mut commands, retained:0, budget:&mut Scene::begin_work(false)},Destination::View,None);
        assert!(result.is_ok() || matches!(result,Err(GpuRasterError::NotReady)),"step {step}: {result:?}");
        scene.discard_unencoded_jobs();r.uploads.finish(&encoder);
        if encoder.has_work() {encoder.submit(&r.queue);crate::test_support::complete(&r);}
        let displayed=cache.displays(region);
        let mut store=cache.pages.lock().unwrap();
        let accepted=store.accepted_coordinates(&key,cache.required_input_level()).any(|coordinate|!page_rect(coordinate).intersect(region).is_empty());
        assert!(accepted || displayed || store.get(&key,region,pages::Accuracy::Display,key.level,crate::residency::Class::Visible).is_some(),"step {step}: a current-view unit must keep getting accepted or committed");
        drop(store);
        if step%17==16 {gate.gate.release();crate::test_support::settle_decodes(&mut r);for _ in wakes.try_iter() {}}
    }
}

#[test]
fn wanted_page_inputs_cancel_same_region_when_only_mode_key_or_format_changes() {
    let mut attempt=Attempt::new(false);
    let (result,_)=attempt.attempt();
    assert!(result.is_ok() || matches!(result,Err(GpuRasterError::NotReady)),"{result:?}");
    let region=PixelRect::full([PAGE_SIZE;2]);
    let key=attempt.cache.page_key(attempt.cache.graph.root.clone().unwrap(),region);
    let source=rgba8_source([PAGE_SIZE;2],|_,_|[31,67,113,193]);
    let mut encoder=submission::CommandEncoder::new(&attempt.r.device,&Default::default());
    let prepared=crate::prepare_image_tile(&source,[0;2],attempt.r.device.working_space(),&attempt.r.device.source_samples,None).unwrap();
    Scene::decode_prepared_for(&mut attempt.r,&mut encoder,&source,[0;2],prepared,crate::scene::sources::SourceUse::Exact,crate::residency::Class::Visible).unwrap();
    attempt.r.uploads.finish(&encoder);encoder.submit(&attempt.r.queue);crate::test_support::complete(&attempt.r);
    let mut store=pages::Store::default();
    store.set_expected(attempt.cache.pages.lock().unwrap().expected().unwrap().clone());
    for change in 0..3 {
        let free=attempt.r.source_tiles.borrow().free_slots();
        let lease={let sources=attempt.r.source_tiles.borrow();sources.lease(sources.prepared_view(&source,[0;2]).unwrap()).unwrap()};
        let image=store.claim(&attempt.r,key.clone(),u64::MAX,crate::scene::scale::pages::WindowId::default(),crate::residency::Class::Visible).unwrap();drop(image);
        pages::hold_fixture_inputs(&mut store,&key,region,Evaluation::Display,vec![lease].into(),key.level);
        assert!(store.pinned_bytes()>0);assert_eq!(attempt.r.source_tiles.borrow().free_slots()+1,free);
        let mut wanted=key.clone();let mut mode=Evaluation::Display;
        match change {
            0=>mode=Evaluation::Tiled(64),
            1=>wanted.expression=Arc::new(graph::Expression::Color([0;4])),
            _=>wanted.format=if key.format==wgpu::TextureFormat::Rgba16Float {wgpu::TextureFormat::Rgba32Float}else{wgpu::TextureFormat::Rgba16Float},
        }
        if change==0 {assert!(wanted==key,"evaluation mode is not part of the authored page key");}
        store.set_expected(Arc::new(SignatureFrame::from_snapshot(store.expected().unwrap().snapshot())));
        store.retain_work(&[pages::Interest::new(wanted,mode,region,key.level)]);
        assert_eq!(store.pinned_bytes(),0,"same-region change {change} retained an obsolete accepted unit");
        assert_eq!(attempt.r.source_tiles.borrow().free_slots(),free,"same-region change {change} did not release its source lease before new admission");
    }
}

#[test]
fn accepted_native_inputs_reuse_their_existing_normalization_workspace() {
    let extent=[PAGE_SIZE;2];
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(Default::default());
    let count=4*crate::SOURCE_SLOTS+1;
    let mut doc=document_at(extent);composition_mut(&mut doc).blend=layer_core::BlendSpace::Linear;
    let sources:Vec<_>=(0..count).map(|index|rgba8_source(extent,move|_,_|[31+(index%193) as u8,67,113,193])).collect();
    let mut owners:Vec<_>=sources.iter().map(|source|paint_occurrence(&mut doc,"Accepted cold input",Some(source.clone()))).collect();
    let blur=effect_occurrence(&mut doc,layer_core::EffectInstance::new(crate::tests::fixture("gaussian_blur").program()),"Native blur");
    set_effect_value(&mut doc,blur,"sigma",layer_core::EffectValue::Number(1.));owners.insert(0,blur);
    let alpha=effect_occurrence(&mut doc,layer_core::EffectInstance::new(crate::tests::fixture("brightness_to_opacity").program()),"Native alpha");
    owners.insert(0,alpha);set_root_entries(&mut doc,owners);
    r.ensure_document_metadata(extent,doc.scene()).unwrap();working_scene_frame(&mut r,frame(&doc,0));
    let mut scene=Scene::new(&r).unwrap();scene.begin_frame();scene.scale_sources.prepare(&r,frame(&doc,0),&[]);
    let mut cache=Cache::new(&r,Request {plan:display_mips::Plan::at(extent,0),evaluation:Evaluation::Native},doc.scene().order().len());
    crate::scene::scale::tests::prepare_fixture_frame(&r,frame(&doc,0),&scene);
    let mut commands=RegionRecords::new(&r);commands.begin();cache.prepare_graph(&r,frame(&doc,0),&scene).unwrap();
    let root=cache.graph.root.as_ref().unwrap().clone();assert!(root.native_capture(doc.scene(),0));
    let region=PixelRect::full(extent);let key=cache.page_key(root,region);
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    let requests=cache.region_sources(&scene,&r,frame(&doc,0),region,true);
    let roots=[cache.graph.root.clone().unwrap()];
    assert!(cache.accept_parent(&mut scene,&mut r,frame(&doc,0),region,&requests,0,true,&roots,Root::Store,false,crate::residency::Class::Visible,crate::residency::Class::Visible,&mut encoder).unwrap().is_some(),"ample Core working capacity must admit the whole native unit");
    let inputs=cache.pages.lock().unwrap().take_fixture_inputs(&key,region,0);
    for &owner in doc.scene().order().iter().filter(|owner|doc.scene().paint_source(**owner).is_some()) {
        let base=doc.scene().paint_source(owner).unwrap().base.as_ref().unwrap();
        let read=crate::scene::sources::accepted_base(&inputs,base,[0;2]).unwrap().unwrap();
        let pixels=crate::prepare_image_tile(base.image.storage(),[0;2],r.device.working_space(),&r.device.source_samples,None).unwrap();
        assert!(!read.pending.is_empty(),"cold exact normalization must still be unrecorded");
        for pending in read.pending {pending.set_prepared(Some(pixels.clone()));}
    }
    pages::hold_fixture_inputs(&mut cache.pages.lock().unwrap(),&key,region,cache.evaluation,inputs,key.level);
    let slots=r.source_tiles.borrow().allocated_slots();let workspace=r.source_tiles.borrow().non_slot_bytes();
    assert_eq!(slots,count);
    let validation=r.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let window=scene.cached_capture_window(doc.scene(),region);
    let result=scene.prepare_region_in_frame(&mut r,frame(&doc,0),(region,window),PixelRect::EMPTY,false,&mut encoder);
    assert!(result.is_ok(),"an accepted native unit must reuse its already allocated normalization workspace: {result:?}; count={count}");
    assert_eq!(r.source_tiles.borrow().allocated_slots(),slots,"zero-extra preflight cannot allocate another slot");
    assert_eq!(r.source_tiles.borrow().non_slot_bytes(),workspace,"zero-extra preflight cannot allocate another input or transfer table");
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    assert!(sources.iter().all(|source|r.source_tiles.borrow().prepared_view(source,[0;2]).is_some()),"accepted cold normalizations must make ready progress");
    assert!(scene.image_pass_pixels()>0);
    assert!(pollster::block_on(validation.pop()).is_none());
}

#[test]
fn navigator_keeps_all_refused_units_pending_without_a_false_publication() {
    let extent=[PAGE_SIZE;2];let doc=document_at(extent);
    let mut r=crate::memory::memory_tests::renderer(doc.composition().color,layer_core::DeviceMemoryFacts {unified:true,..Default::default()}).unwrap();
    r.ensure_document_metadata(extent,doc.scene()).unwrap();r.begin_frame_memory(frame(&doc,0));r.publish_memory();
    let mut scene=Scene::new(&r).unwrap();scene.begin_frame();scene.scale_sources.prepare(&r,frame(&doc,0),&[]);
    let mut cache=Cache::new(&r,Request {plan:display_mips::Plan::at(extent,0),evaluation:Evaluation::Display},doc.scene().order().len());
    crate::scene::scale::tests::prepare_fixture_frame(&r,frame(&doc,0),&scene);
    cache.prepare_graph(&r,frame(&doc,0),&scene).unwrap();
    r.scene=Some(scene);r.scale_display=Some(cache);
    let budget=r.factory.memory_budget().clone();budget.flush_reports();
    let pressure=budget.admit_import(budget.import_headroom_bytes()).unwrap();
    assert!(r.memory.display_owner().admit_working(layer_core::MemoryGrant {ram_bytes:0,gpu_bytes:1}).is_none(),"the ordinary import must cause genuine working-admission refusal");
    let mut navigator=Navigator::default();let validation=r.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    navigator.refresh(&mut r,frame(&doc,0),false,&mut encoder,&mut Scene::begin_work(false)).unwrap();
    assert!(navigator.pending());assert!(navigator.view().is_none());
    assert_eq!(navigator.revision,0,"no admitted unit means no completed Navigator publication");
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);drop(pressure);
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    crate::test_support::query_ready(&mut r,&mut encoder,|r,encoder| {
        r.scene.as_mut().unwrap().begin_frame();
        navigator.refresh(r,frame(&doc,0),false,encoder,&mut Scene::begin_work(false))?;
        if navigator.view().is_some() {Ok(())}else {Err(GpuRasterError::NotReady)}
    });
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    assert_eq!(navigator.revision,1);assert!(navigator.view().is_some());
    assert!(pollster::block_on(validation.pop()).is_none());
}
