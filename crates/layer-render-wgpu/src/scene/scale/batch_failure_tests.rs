use super::*;

#[test]
fn shared_ready_encode_error_discards_jobs_before_any_unit_state_commits() {
    let extent=[2*PAGE_SIZE,PAGE_SIZE];
    let mut doc=document_at(extent);
    composition_mut(&mut doc).blend=layer_core::BlendSpace::Linear;
    let sources:Vec<_>=(0..3).map(|index|rgba8_source(extent,move|_,_|[31+index*43,67,113,193])).collect();
    let owners:Vec<_>=sources.iter().map(|source|paint_occurrence(&mut doc,"Ready input",Some(source.clone()))).collect();
    let group=stack_occurrence(&mut doc,"Retained ready branch",owners[..2].to_vec());
    set_root_entries(&mut doc,vec![owners[2],group]);
    let frame=FramePacket {blend_space:layer_core::BlendSpace::Linear,composite_all:false,..packet(doc.scene(),extent)};
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.ensure_document_metadata(extent,doc.scene()).unwrap();working_scene_frame(&mut r,frame);
    let mut required=crate::compile::Needs::capture_scene(&r,doc.scene(),false);
    required.render.extend(crate::startup::SourceKinds::for_scene(doc.scene()).pipelines(&r.scene_pipelines.source).cloned());
    required.compute.extend(r.scene_pipelines.scale_pipelines(r.display_precision(doc.scene()).format()).into_iter().cloned());
    pollster::block_on(required.prepare(&r.compilation)).unwrap();
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    for source in &sources {for coordinate in [[0,0],[1,0]] {
        let prepared=crate::prepare_image_tile(source,coordinate,r.device.working_space(),&r.device.source_samples,None).unwrap();
        Scene::decode_prepared_for(&mut r,&mut encoder,source,coordinate,prepared,crate::scene::sources::SourceUse::Exact,crate::residency::Class::Visible).unwrap();
    }}
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    let mut scene=r.scene.take().unwrap_or_else(||Scene::new(&r).unwrap());
    scene.begin_frame();scene.scale_sources.prepare(&r,frame,&[]);
    let plan=display_mips::Plan::at(extent,0);
    let mut cache=Cache::new(&r,Request {plan,evaluation:Evaluation::Display},doc.scene().order().len());
    let mut commands=RegionRecords::new(&r);commands.begin();
    crate::scene::scale::tests::prepare_fixture_frame(&r,frame,&scene);
    cache.prepare_graph(&r,frame,&scene).unwrap();
    cache.graph.prepare(&r,frame,&scene.scale_sources,plan,false).unwrap();
    let root=cache.graph.root.clone().unwrap();
    let target=doc.scene().source_target(owners[0]).unwrap();
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    for owner in &owners {
        let id=doc.scene().source_target(*owner).unwrap();
        cache.prepare_source(&mut scene,&mut r,frame,&mut encoder,id,SourceRequest {plan,required:PixelRect::full(extent).into(),covered:PixelRect::EMPTY},Class::Prefetch,&mut Scene::begin_work(false)).unwrap();
    }
    let before_coverage=cache.coverage_snapshot();
    let before_branches=cache.commit_state();
    let written_state=|cache:&Cache| {
        let mut pages=cache.pages.lock().unwrap().retained_pages().into_iter().filter(|page|!page.written.is_empty())
            .map(|page|(crate::test_support::coverage_hash(&page.key),[page.written,page.current,page.exact])).collect::<Vec<_>>();
        pages.sort_by_key(|(key,_)|*key);pages
    };
    let before_written=written_state(&cache);
    let before_levels=source_chunks(&cache,target,1);
    let before_updates=scene.scale_sources.entries[&target].updates;
    assert!(scene.vectors.motion.is_none());
    let mut units=Vec::new();
    for page in 0..2 {
        let start=scene.jobs.len();let mut unit=Unit::default();
        let mut accepted=cache.declare_unit(&mut scene,&mut r,frame,page_rect([page,0]),plan,false,false,0,crate::residency::Class::Visible,&mut encoder,|evaluator| {
            let output=evaluator.evaluate_root(&root,Root::Store)?;evaluator.materialize(output,None).map(drop)
        }).unwrap().accept_query(&mut r,Some(&scene)).unwrap();
        {
            let mut evaluator=Evaluator {class:crate::residency::Class::Visible,accepted:&mut accepted,cache:&mut cache,scene:&mut scene,packet:frame,r:&mut r,
                encoder:&mut encoder,region:page_rect([page,0]).into(),input:plan,tiled:false,
                unit:&mut unit,previous_motion:None,retained:0,approximate:false,preview:false};
            let output=evaluator.evaluate_root(&root,Root::Store).unwrap();
            evaluator.materialize(output,None).unwrap();
        }
        unit.sources.extend(accepted).unwrap();
        let source_key=scene.scale_sources.entries[&target].key(&r,target,1,[page/2,0]);
        let source_image=cache.pages.lock().unwrap().claim(&r,source_key.clone(),u64::MAX,crate::scene::scale::pages::WindowId::default(),crate::residency::Class::Visible).unwrap();
        let write=cache.pages.lock().unwrap().prepare_write(source_key.clone(),page_rect([page,0]),pages::Accuracy::Display,1).unwrap();
        scene.jobs.push(Job::Clear(source_image.view.clone(),wgpu::Color::TRANSPARENT));
        unit.reads.push(source_image);
        unit.pages.push(write);
        unit.updates.push((target,1));
        unit.motion=Some(crate::scene::vectors::MotionImage {
            key:(owners[0],crate::scene::vectors::Moving(Arc::new(BTreeSet::new())),0,[0;2],crate::scene::vectors::Replaced(None)),
            window:page_rect([page,0]).into(),image:Image::new(&r,plan,"staged unit motion"),revision:0,
        });
        assert!(scene.jobs_ready(&r,start));
        assert!(unit.ready(&mut scene,&mut r,&mut encoder,start,Class::Visible).unwrap());
        if page==1 {
            let source=rgba8_source([PAGE_SIZE;2],|_,_|[31,67,113,193]);
            let (_,pending)=r.source_tiles.borrow_mut().plan_exact(&r,&source,[0;2]).unwrap();
            let pending=pending.unwrap();
            pending.set_prepared(Some(crate::prepare_image_tile(&source,[0;2],r.device.working_space(),&r.device.source_samples,None).unwrap()));
            assert!(pending.ready() && !pending.recorded());
            scene.jobs.push(Job::DecodedTile(pending));
        }
        assert!(unit.pages.iter().any(|page|page.key.source==Some(target)) && unit.motion.is_some());
        units.push(unit);
    }
    assert!(units.iter().any(|unit|unit.pages.iter().any(|page|page.key.source.is_none()
        && page.key.expression!=root && Some(&page.key.expression)!=cache.graph.content.as_ref())),
        "the ready graph must stage retained branch page coverage");
    let prepared_passes=encoder.pass_count();
    let result=cache.encode_units(&mut scene,&mut r,&mut encoder,units);
    assert!(matches!(&result,Err(GpuRasterError::Invariant("a source upload reached its batch unrecorded"))),"{result:?}");
    assert!(encoder.pass_count()>prepared_passes,"the fault must follow an encoded unit prefix");
    assert!(scene.jobs.is_empty());assert!(scene.source_jobs.is_empty());
    assert_coverage_eq(&before_coverage,&cache.coverage_snapshot());
    assert_eq!(cache.commit_state(),before_branches);
    assert_eq!(written_state(&cache),before_written);
    assert_eq!(source_chunks(&cache,target,1),before_levels);
    assert_eq!(scene.scale_sources.entries[&target].updates,before_updates);
    assert!(scene.vectors.motion.is_none());
    r.uploads.finish(&encoder);drop(encoder);
}
