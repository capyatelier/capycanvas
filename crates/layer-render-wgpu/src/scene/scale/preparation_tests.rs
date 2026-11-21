#![cfg(not(target_arch="wasm32"))]
use super::*;

fn gather(scene:&mut Scene,r:&WgpuRasterizer,doc:&Document,effect:OccurrenceHandle,destination:&wgpu::Texture)
    ->(std::sync::Weak<wgpu::Buffer>,crate::effects::Pipeline) {
    let input=scene.alloc(r,wgpu::Color {r:0.65,g:0.2,b:0.1,a:1.}).unwrap();
    let output=scene.effect(r,frame(doc,0),&[effect],[0;2],input,&mut Default::default()).unwrap();
    let (parameters,pipeline)=scene.jobs.iter().find_map(|job|match job {
        Job::Effect {prepared,..}=>Some((Arc::downgrade(&prepared._parameters),prepared.pipeline.clone())),
        _=>None,
    }).unwrap();
    scene.jobs.push(Job::Copy {source:scene.pool[output].texture.clone(),source_origin:[0;2],
        destination:destination.clone(),origin:[0;2],width:PAGE_SIZE,height:PAGE_SIZE});
    (parameters,pipeline)
}

fn prepare(r:&WgpuRasterizer,pipeline:&crate::effects::Pipeline) {
    match pipeline {
        crate::effects::Pipeline::Render(pipeline)=>pollster::block_on(r.compilation.prepare_all([pipeline])).unwrap(),
        crate::effects::Pipeline::Compute(pipeline)=>pollster::block_on(r.compilation.prepare_all([pipeline])).unwrap(),
    }
}

#[test]
fn a_retained_cold_lookup_does_not_block_an_independent_ready_unit() {
    let mut doc=document_at([PAGE_SIZE;2]);
    composition_mut(&mut doc).blend=layer_core::BlendSpace::Linear;
    let mut program=(*crate::tests::fixture("selective_color").program()).clone();
    program.id="independent_lookup".into();
    let mut lookups=program.lookups.to_vec();
    assert_eq!(lookups.len(),1);
    lookups[0].wgsl=format!("{}\n\n",lookups[0].wgsl.sources().unwrap().join("\n")).into();
    program.lookups=lookups.into();
    let mut instance=layer_core::EffectInstance::new(Arc::new(program));
    instance.set("reds_cyan",layer_core::EffectValue::Number(-30.)).unwrap();
    let a=effect_occurrence(&mut doc,instance,"Cold lookup");
    let mut instance=layer_core::EffectInstance::new(crate::tests::fixture("exposure").program());
    instance.set("exposure",layer_core::EffectValue::Number(0.5)).unwrap();
    let b=effect_occurrence(&mut doc,instance,"Ready exposure");
    let paint=doc.scene().order()[0];
    set_root_entries(&mut doc,vec![a,b,paint]);
    let reference=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut r=crate::test_support::staged_renderer(&reference,doc.composition().color);
    for renderer in [&r,&reference] {
        pollster::block_on(renderer.compilation.prepare_all([&renderer.scene_pipelines.constant.2,&renderer.scene_pipelines.fold.pipeline])).unwrap();
    }
    let mut scene=Scene::new(&r).unwrap();
    let image=||crate::test_support::page_texture(&r,r.device.working_format());
    let warm=image();let pending=image();let ready=image();let expected=image();
    let (_,pipeline)=gather(&mut scene,&r,&doc,b,&warm);prepare(&r,&pipeline);
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    scene.encode_jobs(&mut r,&mut encoder).unwrap();
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    scene.discard_unencoded_jobs();
    let expected_b=pixels(&r,&warm);
    assert!(expected_b.iter().all(|pixel|pixel[3]==1. && pixel[0]>0.));
    let (parameters,pipeline)=gather(&mut scene,&r,&doc,a,&pending);prepare(&r,&pipeline);
    assert!(pipeline.ready());
    let compiler=r.startup.as_ref().unwrap().compiler.clone();
    let (entered,entry)=std::sync::mpsc::channel();
    let (release,held)=std::sync::mpsc::channel();
    compiler.enqueue(startup::BRUSH,move||{let _=entered.send(());let _=held.recv();Ok(())});
    entry.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    assert!(!scene.prepare_jobs(&mut r,&mut encoder,0,Class::Visible).unwrap(),"only A's lookup preparation shader must remain unavailable");
    assert_eq!(encoder.pass_count(),0);
    assert!(compiler.pending()>1,"A must queue its actual lookup behind the held compiler job");
    scene.discard_unencoded_jobs();
    assert!(parameters.upgrade().is_some(),"discarding A's job must retain its cached parameter Instance");
    let prepared=scene.effects.preparation_count();
    let (_,pipeline)=gather(&mut scene,&r,&doc,b,&ready);
    assert!(pipeline.ready());
    assert!(scene.prepare_jobs(&mut r,&mut encoder,0,Class::Visible).unwrap(),
        "an excluded unit's retained lookup preparation must not block B's ready jobs");
    scene.encode_jobs(&mut r,&mut encoder).unwrap();
    assert!(encoder.pass_count()>0);
    assert_eq!(scene.effects.preparation_count(),prepared,"B must not encode A's unready lookup");
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    scene.discard_unencoded_jobs();
    assert_eq!(pixels(&r,&ready),expected_b);
    assert!(pixels(&r,&pending).iter().all(|pixel|*pixel==[0.;4]),"A's discarded unit must not publish pixels");
    release.send(()).unwrap();
    let (finished,done)=std::sync::mpsc::channel();
    compiler.enqueue(startup::DOCUMENT,move||{let _=finished.send(());Ok(())});
    done.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
    gather(&mut scene,&r,&doc,a,&pending);
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    scene.encode_jobs(&mut r,&mut encoder).unwrap();
    assert_eq!(scene.effects.preparation_count(),prepared+1,"A must prepare its retained lookup exactly once after release");
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    let mut oracle=Scene::new(&reference).unwrap();
    gather(&mut oracle,&reference,&doc,a,&expected);pollster::block_on(oracle.effects.needs().prepare(&reference.compilation)).unwrap();
    let mut reference=reference;
    let mut encoder=submission::CommandEncoder::new(&reference.device,&Default::default());
    oracle.encode_jobs(&mut reference,&mut encoder).unwrap();
    reference.uploads.finish(&encoder);encoder.submit(&reference.queue);crate::test_support::complete(&reference);
    assert_eq!(pixels(&r,&pending),pixels(&reference,&expected));
}

#[test]
fn a_prepared_drawing_shows_its_first_stroke_on_the_first_admitted_frame_and_compiles_only_its_display_kernels() {
    for (case,depth,zoom,offset,layers,kind) in [
        ("half reduced preview",SampleDepth::U8,0.125,[0;2],1,DabBatchKind::Preview),
        ("half reduced commit",SampleDepth::U8,0.125,[0;2],1,DabBatchKind::Persistent),
        ("full reduced preview",SampleDepth::F32,0.125,[0;2],1,DabBatchKind::Preview),
        ("half full-size preview",SampleDepth::U8,1.,[0;2],1,DabBatchKind::Preview),
        ("half offset layer preview",SampleDepth::U8,0.125,[3,5],1,DabBatchKind::Preview),
        ("half pivot commit",SampleDepth::U8,0.25,[0;2],3,DabBatchKind::Persistent),
    ] {
        let mut doc=document_at([517,259]);
        composition_mut(&mut doc).color.depth=depth;
        let photo=doc.scene().order()[0];
        let mut entries:Vec<_>=(1..layers).map(|index| {
            let copy=copy_paint(&mut doc,0);
            doc.artwork.occurrences.get_mut(copy).unwrap().opacity=0.17+index as f32*0.02;
            copy
        }).collect();
        entries.push(photo);
        let stroked=(layers-1)/2;
        let ink=paint_occurrence(&mut doc,"Ink",None);
        doc.artwork.occurrences.get_mut(ink).unwrap().offset=offset;
        entries.insert(stroked,ink);set_root_entries(&mut doc,entries);
        let extent=doc.composition().size;
        let mut exact=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut r=crate::test_support::staged_renderer(&exact,doc.composition().color);
        exact.test.reference=true;
        r.shader_idle(false,false);
        r.prepare_startup(&doc,&layer_core::default_brush(DefaultBrushPreset::GPen),false).unwrap();
        crate::test_support::wait_startup(&mut r,std::time::Instant::now()+std::time::Duration::from_secs(60),|progress|progress.brush_ready,format_args!("{case}: startup"));
        let mut frame=FramePacket {blend_space:doc.composition().blend,..packet(doc.scene(),extent)};
        frame.view.document_to_surface=[zoom,0.,0.,zoom,0.,0.];
        crate::test_support::submit_settled(&mut r,frame);
        crate::test_support::submit_published(&mut exact,frame);
        let original=display_pixels(&r);
        let mut dab=crate::tests::test_dab([255.,129.],[0.9,0.02,0.1,0.7],1.);
        dab.radii=[45.;2];
        let style=layer_render::DabStyle {blend_space:doc.composition().blend,..crate::layer_tests::preset_style(DefaultBrushPreset::GPen)};
        let batch=DabBatch {kind,..dab_batch(source_at(&doc,stroked),style,dab.bounds())};
        let stroke=FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),composite_all:false,..frame};
        let _held=r.compilation.batch();
        r.poll_pending(stroke.view).unwrap();
        crate::test_support::pace(&mut r);
        assert!(matches!(r.submit_when_ready(stroke,None),std::task::Poll::Ready(Ok(()))),"{case}: a prepared drawing admits its first stroke at once");
        let shown=display_pixels(&r);
        assert!(shown.iter().zip(&original).any(|(shown,original)|shown!=original),"{case}: the first admitted stroke frame shows ink");
        assert!(!publication_pending(&r),"{case}: the first admitted stroke frame presents the stroked artwork: {:?}",r.outstanding_terms());
        crate::test_support::submit_published(&mut exact,stroke);
        let cache=r.scale_display.as_ref().unwrap();
        let (plan,format)=(cache.plan,cache.texture().format());
        let error=quality(&shown,&pixels(&exact,crate::test_support::document_texture(&exact)),plan,format);
        assert!(error[0]<0.003,"{case}: the first stroke frame stays close to the exact reduction: {error:?}");
        crate::test_support::submit_settled(&mut r,FramePacket {composite_all:false,..frame});
        assert_eq!(crate::compile::tests::queued_recipes(&r.compilation),0,"{case}: the stroke and its refinement wait on no compilation");
        let half=format==wgpu::TextureFormat::Rgba16Float;
        assert_eq!(half,depth==SampleDepth::U8,"{case}");
        let scale=&r.scene_pipelines.scale;
        let compiled:Vec<_>=[("full",&scale.full),("half",&scale.half)].into_iter().flat_map(|(precision,kernels)|
            [("compose",&kernels.compose),("reduce",&kernels.reduce),("reduce_phased",&kernels.reduce_phased),("reduce_pair",&kernels.reduce_pair),("project",&kernels.project)]
                .into_iter().filter(|(_,kernel)|kernel.ready()).map(move|(name,_)|format!("{precision}.{name}"))).collect();
        let expected:&[&str]=if half {&["full.compose","full.reduce","full.reduce_phased","half.compose","half.reduce","half.reduce_pair","half.project"]}
            else {&["full.compose","full.reduce","full.reduce_phased","full.reduce_pair"]};
        assert_eq!(compiled,expected,"{case}: a display compiles the kernels it can run and no others");
    }
}
