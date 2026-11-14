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
        pollster::block_on(renderer.compilation.prepare_all([&renderer.scene_pipelines.constant.2])).unwrap();
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
