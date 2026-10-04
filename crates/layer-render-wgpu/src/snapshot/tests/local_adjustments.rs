use super::*;
use layer_core::{ArtworkSample, ArtworkSampleRequest, ArtworkSource, EffectValue, LayerBlend};
use layer_engine::{CanvasEngine, ViewTransform, input_queue};

fn engine(document:Document)->CanvasEngine<WgpuRasterizer> {
    let extent=document.composition().size;
    let renderer=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    let (_,consumer)=input_queue(8);
    CanvasEngine::new(renderer,document,consumer,crate::test_support::view(extent),ViewTransform::IDENTITY).unwrap()
}

fn finish(engine:&mut CanvasEngine<WgpuRasterizer>)->Result<(),String> {
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(15);
    loop {
        engine.render_frame_at(0).map_err(|error|error.to_string())?;
        if !engine.has_pending_document_edits()&&!engine.has_pending_input(){return Ok(());}
        assert!(std::time::Instant::now()<deadline,"cold local-analysis raster operation remained pending");
        std::thread::yield_now();
    }
}

fn baked_pixel(engine:&CanvasEngine<WgpuRasterizer>,target:SourceTarget,x:u32,y:u32)->[f32;4] {
    let data=engine.document().target_raster(target).unwrap().wait_data().unwrap();
    let tile=data.tiles.get(&TileKey{plane:layer_core::raster::RasterPlane::Color,coordinate:[0,0]}).unwrap();
    let bytes=tile.wait_backing().unwrap().decode().unwrap();
    let offset=((y*256+x)*16) as usize;
    std::array::from_fn(|channel|f32::from_le_bytes(bytes[offset+channel*4..offset+channel*4+4].try_into().unwrap()))
}

fn merge(engine:&mut CanvasEngine<WgpuRasterizer>)->SourceTarget {
    let plan=engine.document().merge_plan(layer_core::MergeKind::Down).unwrap();let target=plan.target;
    engine.insert_with_operations(plan.edits,vec![(target,plan.operation)],None).unwrap();target
}

fn adjustment(kind:&str,key:&str,value:f32)->EffectInstance {
    let mut effect=EffectInstance::new(crate::tests::fixture(kind).program());
    effect.set(key,EffectValue::Number(value)).unwrap();effect
}
fn occurrence(document:&Document,name:&str)->OccurrenceHandle {
    document.artwork.occurrences.iter().find(|(_,_,o)|o.name.as_ref()==name).unwrap().0
}
fn set(document:&mut Document,target:OccurrenceHandle,key:&str,value:f32) {
    let mut effect=effect_draft(document,target);effect.set(key,EffectValue::Number(value)).unwrap();set_effect(document,target,effect);
}
fn named_effect(document:&mut Document,name:&str,effect:EffectInstance,index:usize)->OccurrenceHandle {
    let target=insert_effect(document,effect,index);document.artwork.occurrences.get_mut(target).unwrap().name=name.into();target
}
fn constant_document(space:RgbSpace,alpha:f32)->Document {
    let extent=[33,17];let mut document=Document::new(PortableId::random(),extent[0],extent[1],
        layer_core::DocumentNames{paint:"Original".into(),paper:"Paper".into()});
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().color=DocumentColor{space,depth:SampleDepth::F32};
    for _ in 0..100 {document.artwork.occurrences.reserve(PortableId::random()).unwrap();}
    let interpretation=SourceInterpretation{channels:SourceChannels::Rgba,depth:SampleDepth::F32,
        profile:ColorProfile::Builtin(space),profile_assumed:false};
    let mut source=SourceBuilder::new(extent,interpretation,1024*1024).unwrap();
    let row=(0..extent[0]).flat_map(|_|[0.01125_f32,0.01125,0.01125,alpha]).flat_map(f32::to_le_bytes).collect::<Vec<_>>();
    for _ in 0..extent[1]{source.push_row(&row).unwrap();}
    paint_mut(&mut document).original=Some(Arc::new(source.finish().unwrap()));
    hide_paper(&mut document);
    let original=paint_occurrence(&document);
    let lower=named_effect(&mut document,"Lower",adjustment("shadows_highlights","shadows",50.),0);
    let group=add_group(&mut document,vec![lower,original],0);
    let group_record=document.artwork.occurrences.get_mut(group).unwrap();group_record.name="Lower root".into();group_record.blend=LayerBlend::Normal;
    named_effect(&mut document,"Upper",adjustment("shadows_highlights","shadows",50.),0);
    document
}

fn shadow_gain(y:f64,amount:f64)->f64 {
    let t=((y.log2()-(0.18_f64.log2()-4.))/4.).clamp(0.,1.);
    (2.*amount*(1.-t*t*(3.-2.*t))).exp2()
}

fn output(y:f64,lower:f64,upper:f64)->f64 {
    let lower=y*shadow_gain(y,lower);lower*shadow_gain(lower,upper)
}

fn close(actual:[f32;4],gray:f64,alpha:f32,premultiplied:bool) {
    assert_eq!(actual[3],alpha);
    let target=gray*if premultiplied{f64::from(alpha)}else{1.};
    for value in &actual[..3]{assert!(value.is_finite()&&(f64::from(*value)-target).abs()<=2e-5*target.abs().max(1e-30),
        "local adjustment {actual:?} expected gray={target} alpha={alpha}");}
}

fn resource(reader:&SnapshotRenderer,target:OccurrenceHandle)->Arc<crate::effects::resources::Resource> {
    reader.renderer.effect_analyses.iter().find(|entry|entry.layer()==target).unwrap().resource.clone()
}

#[test]
fn cold_noncontiguous_local_adjustments_prepare_lower_before_upper_for_regions_and_samples() {
    for space in RgbSpace::ALL {for alpha in [1.,0.5,8e-8] {
        let project=constant_document(space,alpha);let y=f64::from(0.01125_f32);let expected=output(y,0.5,0.5);
        let mut reader=capture(project.clone()).unwrap();assert!(reader.renderer.effect_analyses.is_empty());
        for pixel in reader.read_region([7,5,3,2]).unwrap(){close(pixel,expected,alpha,true);}
        assert_eq!(reader.renderer.effect_analyses.len(),2);
        assert_eq!(reader.renderer.effect_analyses.iter().map(|entry|entry.layer()).collect::<Vec<_>>(),[occurrence(&project,"Lower"),occurrence(&project,"Upper")]);
        for (source,gray) in [(ArtworkSource::Visible,expected),(ArtworkSource::EffectInput(occurrence(&project,"Upper")),y*shadow_gain(y,0.5))] {
            let result=pollster::block_on(gpu().artwork_sample(ArtworkSampleRequest::new(&project,source,[8.,6.],1),Default::default())).unwrap();
            let ArtworkSample::Color(pixel)=result else{panic!("cold local sample {result:?}")};close(pixel,gray,alpha,false);
        }
    }}
}

#[test]
fn local_analysis_reuses_own_amount_lease_and_invalidates_upper_after_lower_edits() {
    let mut project=constant_document(RgbSpace::Srgb,1.);let y=f64::from(0.01125_f32);
    let mut reader=capture(project.clone()).unwrap();reader.read_region([0,0,1,1]).unwrap();
    let lower_handle=occurrence(&project,"Lower");let upper_handle=occurrence(&project,"Upper");
    let lower=resource(&reader,lower_handle);let upper=resource(&reader,upper_handle);let weak=Arc::downgrade(&upper);
    let inherited=reader.renderer.snapshot_gpu();
    set(&mut project,upper_handle,"shadows",25.);
    let mut edited=inherited.capture_scene(project.snapshot(),SceneScope::All,Default::default()).unwrap();
    close(edited.read_region([0,0,1,1]).unwrap()[0],output(y,0.5,0.25),1.,true);
    assert!(Arc::ptr_eq(&lower,&resource(&edited,lower_handle)));assert!(Arc::ptr_eq(&upper,&resource(&edited,upper_handle)));
    set(&mut project,lower_handle,"shadows",25.);
    let mut changed=edited.renderer.snapshot_gpu().capture_scene(project.snapshot(),SceneScope::All,Default::default()).unwrap();
    close(changed.read_region([0,0,1,1]).unwrap()[0],output(y,0.25,0.25),1.,true);
    assert!(Arc::ptr_eq(&lower,&resource(&changed,lower_handle)));assert!(!Arc::ptr_eq(&upper,&resource(&changed,upper_handle)));
    drop(changed);drop(edited);drop(reader);drop(inherited);drop(upper);assert!(weak.upgrade().is_none());
}

#[test]
fn hidden_ancestor_local_adjustments_need_no_guide_and_cancelled_chain_cannot_publish_output() {
    let mut project=constant_document(RgbSpace::Srgb,0.5);
    let upper=occurrence(&project,"Upper");let lower=occurrence(&project,"Lower");
    let effect=named_effect(&mut project,"Hidden clarity",adjustment("clarity","amount",100.),0);
    let hidden=add_group(&mut project,vec![effect],0);project.artwork.occurrences.get_mut(hidden).unwrap().visible=false;
    let control=CaptureControl::default();let mut reader=gpu().capture_scene(project.snapshot(),SceneScope::All,control.clone()).unwrap();
    pollster::block_on(reader.prepare_effect_analysis_async(scene::Output::EffectInput(upper))).unwrap();
    assert_eq!(reader.renderer.effect_analyses.len(),1);assert_eq!(reader.renderer.effect_analyses[0].layer(),lower);
    control.cancel();assert!(reader.read_region([0,0,1,1]).unwrap_err().to_string().contains("cancel"));
    assert_eq!(reader.renderer.effect_analyses.len(),1);
    let mut full=gpu().capture_scene(reader.scene.clone(),SceneScope::All,Default::default()).unwrap();
    close(full.read_region([0,0,1,1]).unwrap()[0],output(f64::from(0.01125_f32),0.5,0.5),0.5,true);
    assert_eq!(full.renderer.effect_analyses.len(),2);assert!(!full.analysis_ready.contains(&effect));
}

#[test]
fn local_analysis_honors_frozen_lower_animation_phase_and_clarity_constant_identity() {
    let mut project=constant_document(RgbSpace::Srgb,1.);
    let mut program=(*crate::tests::fixture("domain_warp").program()).clone();program.kind=layer_core::EffectKind::Generator;
    program.entry="local_phase_gray".into();program.passes=Arc::new([]);
    program.wgsl="fn local_phase_gray(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{let y=.01125*exp2(fx_time(b));return vec4<f32>(y,y,y,1.);}".into();
    let mut effect=EffectInstance::new(Arc::new(program));effect.set("animate",EffectValue::Toggle(true)).unwrap();effect.set("speed",EffectValue::Number(1.)).unwrap();
    let original=paint_occurrence(&project);let group=occurrence(&project,"Lower root");
    let generator=named_effect(&mut project,"Phase",effect,0);
    let root=project.composition().result;project.artwork.stacks.get_mut(root).unwrap().entries.retain(|h|*h!=generator);
    let OccurrenceContent::Stack(stack)=project.artwork.occurrences.get(group).unwrap().content else {panic!("group");};
    let entries=&mut project.artwork.stacks.get_mut(stack).unwrap().entries;let index=entries.iter().position(|h|*h==original).unwrap();entries[index]=generator;project.artwork.occurrences.remove(original);
    refresh(&mut project);let phase_target=project.scene().effect_handle(generator).unwrap();
    let mut inherited=gpu();let mut previous=None;
    for phase in [0.,1.] {
        let mut request=ArtworkSampleRequest::new(&project,ArtworkSource::EffectInput(occurrence(&project,"Upper")),[8.;2],1);
        request.set_context(EvaluationContext {elapsed:0.,phases:vec![(phase_target,phase)].into()});
        let (mut snapshot,output)=pollster::block_on(inherited.artwork_capture(&request.query,Default::default())).unwrap();
        pollster::block_on(snapshot.prepare_effect_analysis_async(output)).unwrap();
        let analysis=snapshot.renderer.effect_analyses.iter().find(|entry|entry.layer()==occurrence(&project,"Lower")).unwrap();
        assert_eq!(analysis.query.snapshot.context.phases.as_slice(),&[(phase_target,phase)]);
        if let Some(old)=previous{assert!(!Arc::ptr_eq(&old,&analysis.resource));}previous=Some(analysis.resource.clone());
        let result=pollster::block_on(inherited.artwork_sample(request,Default::default())).unwrap();
        let ArtworkSample::Color(pixel)=result else{panic!("phase sample {result:?}")};
        let y=f64::from(0.01125_f32)*f64::from(phase).exp2();close(pixel,y*shadow_gain(y,0.5),1.,false);
        inherited=snapshot.renderer.snapshot_gpu();
    }
    let mut project=constant_document(RgbSpace::Srgb,0.5);
    let lower=occurrence(&project,"Lower");let upper=occurrence(&project,"Upper");
    let stacks:Vec<_>=project.artwork.stacks.iter().map(|(h,_,_)|h).collect();
    for stack in stacks {project.artwork.stacks.get_mut(stack).unwrap().entries.retain(|h|*h!=lower&&*h!=upper);}
    project.artwork.occurrences.remove(lower);project.artwork.occurrences.remove(upper);refresh(&mut project);
    named_effect(&mut project,"Clarity",adjustment("clarity","amount",100.),0);
    let mut reader=capture(project).unwrap();close(reader.read_region([0,0,1,1]).unwrap()[0],f64::from(0.01125_f32),0.5,true);
    assert_eq!(reader.renderer.effect_analyses.len(),1);
}

#[test]
fn cold_merge_prepares_retained_noncontiguous_members_and_preserves_group_offset() {
    let mut document=constant_document(RgbSpace::Srgb,0.5);
    let group=occurrence(&document,"Lower root");document.artwork.occurrences.get_mut(group).unwrap().translation=Point{x:3.,y:2.};
    document.working.occurrence=Some(occurrence(&document,"Upper"));
    let mut engine=engine(document);assert!(engine.backend().effect_analyses.is_empty());
    let result=merge(&mut engine);
    engine.render_frame_at(0).unwrap();assert!(engine.has_pending_document_edits());
    assert_eq!(engine.backend().bake_analyses.len(),1);
    finish(&mut engine).unwrap();
    close(baked_pixel(&engine,result,8,6),output(f64::from(0.01125_f32),0.5,0.5),0.5,false);
    assert_eq!(baked_pixel(&engine,result,1,1),[0.;4]);
    assert!(engine.backend().effect_analyses.is_empty(),"isolated bake guides leaked into the live scene");
}

#[test]
fn cold_frequency_detail_prepares_its_retained_adjustment_input() {
    let document=constant_document(RgbSpace::Srgb,0.5);
    let mut source=document.snapshot();
    let scene=Arc::make_mut(&mut source);
    scene.artwork.compositions.get_mut(scene.artwork.root).unwrap().blend=layer_core::BlendSpace::Perceptual;
    let scene=source;let original=paint_occurrence(&document);
    let mut draft=document.clone();
    let (low_occurrence,low)=crate::test_support::add_paint(&mut draft.artwork,"Low",[33,17]);
    let (high_occurrence,high)=crate::test_support::add_paint(&mut draft.artwork,"High",[33,17]);
    let SourceTarget::Paint(low_handle)=low else {unreachable!()};
    let SourceTarget::Paint(high_handle)=high else {unreachable!()};
    let mut edits=Vec::new();
    for handle in [low_handle,high_handle] {edits.push(layer_core::Edit::Paint(RecordChange {handle,id:draft.artwork.paint.id(handle).unwrap(),value:draft.artwork.paint.get(handle).cloned()}));}
    for handle in [low_occurrence,high_occurrence] {edits.push(layer_core::Edit::Occurrence(RecordChange {handle,id:draft.artwork.occurrences.id(handle).unwrap(),value:draft.artwork.occurrences.get(handle).cloned()}));}
    let root=document.composition().result;
    edits.push(layer_core::Edit::Stack(RecordChange::replace(&document.artwork.stacks,root,draft.artwork.stacks.get(root).cloned()).unwrap()));
    let coverage=document.artwork.coverage.next_handle();
    let operation=|kind|layer_core::RasterOperation {placement:Affine::IDENTITY,
        coverage:layer_core::CoverageSnapshot::reveal_all(coverage,[33,17],Point::default()),kind};
    let mut engine=engine(document);
    engine.insert_with_operations(edits,vec![(low,operation(layer_core::RasterOperationKind::Bake {
        scene:scene.clone(),scope:SceneScope::Members(vec![original].into()),offset:Point::default()})),
        (high,operation(layer_core::RasterOperationKind::FrequencyDetail {
            scene,scope:SceneScope::All,offset:Point::default(),low:low_handle}))],None).unwrap();
    engine.render_frame_at(0).unwrap();assert!(engine.has_pending_document_edits());
    assert_eq!(engine.backend().bake_analyses.len(),1);
    finish(&mut engine).unwrap();
    close(baked_pixel(&engine,low,8,6),f64::from(0.01125_f32),0.5,false);
    let encode=|v:f64|if v<=0.0031308{12.92*v}else{1.055*v.powf(1./2.4)-0.055};
    let encoded=0.5+(encode(output(f64::from(0.01125_f32),0.5,0.5))-encode(f64::from(0.01125_f32)))*0.5;
    let linear=if encoded<=0.04045{encoded/12.92}else{((encoded+0.055)/1.055).powf(2.4)};
    close(baked_pixel(&engine,high,8,6),linear,0.5,false);
}

#[test]
fn corrupt_retained_bake_input_reports_terminal_error_instead_of_waiting_forever() {
    let mut document=constant_document(RgbSpace::Srgb,0.5);document.working.occurrence=Some(occurrence(&document,"Upper"));
    let bytes=(0..256*256).flat_map(|_|[0.01125_f32*0.5,0.01125*0.5,0.01125*0.5,0.5]).flat_map(f32::to_le_bytes).collect::<Vec<_>>();
    let blob=TileBlob::encode(document.composition().color.paint_descriptor(),&bytes).unwrap();let blob=crate::test_support::corrupt_tile(blob);
    let mut data=layer_core::raster::RasterData::default();
    data.tiles.insert(TileKey{plane:layer_core::raster::RasterPlane::Color,coordinate:[0,0]},blob);
    paint_mut(&mut document).raster=RasterRevision::backed(data);
    let mut engine=engine(document);merge(&mut engine);
    let error=finish(&mut engine).unwrap_err();
    assert!(error.contains("digest")||error.contains("integrity"),"unexpected terminal bake error: {error}");
}

#[test]
fn cold_color_candidate_prepares_local_guides_before_its_first_submission_and_cancels() {
    let project=constant_document(RgbSpace::DisplayP3,0.5);
    let make=|control|gpu().color_canvas(project.clone(),EvaluationContext::default(),&Default::default(),crate::test_support::view([33,17]),control).unwrap();
    let control=CaptureControl::default();let mut cancelled=make(control.clone());control.cancel();
    assert!(cancelled.poll().unwrap_err().to_string().contains("cancel"));
    let mut candidate=make(Default::default());let deadline=std::time::Instant::now()+std::time::Duration::from_secs(15);
    while !candidate.poll().unwrap(){assert!(std::time::Instant::now()<deadline,"cold color candidate remained pending");std::thread::yield_now();}
    let renderer=candidate.take_ready().unwrap();assert_eq!(renderer.effect_analyses.len(),2);
    let pixels=crate::test_support::float_pixels(&renderer,crate::test_support::document_texture(&renderer));
    for pixel in pixels{close(pixel,output(f64::from(0.01125_f32),0.5,0.5),0.5,true);}
}

#[test]
fn exact_effect_input_ignores_failed_backing_above_its_insertion_point() {
    let mut project=constant_document(RgbSpace::Srgb,0.5);
    let (upper,target)=crate::test_support::add_paint(&mut project.artwork,"Unrelated upper input",[33,17]);
    let root=project.composition().result;let entries=&mut project.artwork.stacks.get_mut(root).unwrap().entries;entries.retain(|h|*h!=upper);entries.insert(0,upper);refresh(&mut project);
    let raster=project.target_raster_mut(target).unwrap();*raster=RasterRevision::pending();raster.publish(Err("upper unrelated backing failed".into())).unwrap();
    let request=ArtworkSampleRequest::new(&project,ArtworkSource::EffectInput(occurrence(&project,"Upper")),[8.,6.],1);
    let result=pollster::block_on(gpu().artwork_sample(request,Default::default())).unwrap();
    let ArtworkSample::Color(pixel)=result else{panic!("source-specific local sample {result:?}")};
    let y=f64::from(0.01125_f32);close(pixel,y*shadow_gain(y,0.5),0.5,false);

    let mut project=constant_document(RgbSpace::Srgb,0.5);
    let (child,target)=crate::test_support::add_paint(&mut project.artwork,"Hidden failed input",[33,17]);refresh(&mut project);
    let raster=project.target_raster_mut(target).unwrap();*raster=RasterRevision::pending();raster.publish(Err("hidden descendant backing failed".into())).unwrap();
    let hidden=add_group(&mut project,vec![child],0);project.artwork.occurrences.get_mut(hidden).unwrap().visible=false;
    let request=ArtworkSampleRequest::new(&project,ArtworkSource::Visible,[8.,6.],1);
    let result=pollster::block_on(gpu().artwork_sample(request,Default::default())).unwrap();
    let ArtworkSample::Color(pixel)=result else{panic!("effective-hidden local sample {result:?}")};
    close(pixel,output(y,0.5,0.5),0.5,false);
}

#[test]
fn cold_clone_reference_prepares_local_guides_without_a_settled_preview() {
    let mut document=constant_document(RgbSpace::Srgb,0.5);
    let (owner,target)=crate::test_support::add_paint(&mut document.artwork,"Clone result",[33,17]);refresh(&mut document);
    let root=document.composition().result;let entries=&mut document.artwork.stacks.get_mut(root).unwrap().entries;entries.retain(|h|*h!=owner);entries.insert(0,owner);refresh(&mut document);
    document.working.occurrence=Some(owner);document.working.target=Some(target);
    for name in ["Upper","Lower root"] {let h=occurrence(&document,name);document.artwork.occurrences.get_mut(h).unwrap().reference=true;}
    let renderer=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    let (mut input,consumer)=input_queue(8);
    let mut engine=CanvasEngine::new(renderer,document,consumer,crate::test_support::view([33,17]),ViewTransform::IDENTITY).unwrap();
    engine.set_brush(layer_core::BrushSnapshot{diameter:6.,hardness:1.,opacity:1.,flow:1.,mappings:Arc::new([]),
        ..layer_core::default_brush(layer_core::DefaultBrushPreset::CloneStamp)}).unwrap();
    engine.set_retouch(Some(layer_core::RetouchSource::References));
    let mut source=layer_core::CloneSource::default();source.set(Point{x:8.,y:6.});engine.set_clone_source(source);
    assert!(engine.backend().effect_analyses.is_empty());
    for (sequence,phase) in [(1,layer_engine::PenPhase::Down),(2,layer_engine::PenPhase::Up)] {
        input.push(crate::test_support::pen(sequence,phase,[20.,10.],layer_engine::SampleFlags::PRIMARY)).unwrap();
    }
    finish(&mut engine).unwrap();
    close(baked_pixel(&engine,target,20,10),output(f64::from(0.01125_f32),0.5,0.5),0.5,false);
    assert!(engine.backend().effect_analyses.is_empty(),"reference guides leaked into the live scene");
    let mut moved=engine.document().artwork.occurrences.get(owner).unwrap().clone();moved.translation=Point{x:8.,y:0.};
    let change=RecordChange::replace(&engine.document().artwork.occurrences,owner,Some(moved)).unwrap();
    engine.apply_edit(layer_core::Edit::Occurrence(change)).unwrap();
    let mut source=layer_core::CloneSource::default();source.aligned=false;source.set(Point{x:3.,y:6.});engine.set_clone_source(source);
    for (sequence,phase) in [(3,layer_engine::PenPhase::Down),(4,layer_engine::PenPhase::Up)] {
        input.push(crate::test_support::pen(sequence,phase,[20.,14.],layer_engine::SampleFlags::PRIMARY)).unwrap();
    }
    finish(&mut engine).unwrap();
    close(baked_pixel(&engine,target,12,14),output(f64::from(0.01125_f32),0.5,0.5),0.5,false);
}

#[test]
fn dropping_a_pending_cold_merge_releases_members_without_publishing_a_partial_raster() {
    let mut document=constant_document(RgbSpace::Srgb,0.5);document.working.occurrence=Some(occurrence(&document,"Upper"));
    let source=Arc::downgrade(document.scene().paint(paint_id(&document)).unwrap().original.as_ref().unwrap());
    let mut engine=engine(document);let result=merge(&mut engine);
    engine.render_frame_at(0).unwrap();assert_eq!(engine.backend().bake_analyses.len(),1);
    assert!(engine.has_pending_document_edits());
    let raster=engine.document().target_raster(result).unwrap().clone();assert!(raster.try_data().is_none());
    drop(engine);
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(15);
    while source.upgrade().is_some(){assert!(std::time::Instant::now()<deadline,"cancelled merge retained its input");std::thread::yield_now();}
    assert!(raster.try_data().is_none(),"cancelled guide work published a partial bake");
}

#[test]
fn scoped_worker_transfer_keeps_required_geometry_phases_and_handles_without_unrelated_backing() {
    use layer_core::package::transfer::PreparedTransfer;
    let mut document=constant_document(RgbSpace::Srgb,0.5);
    let original=paint_occurrence(&document);let target=document.scene().source_target(original).unwrap();
    let group=occurrence(&document,"Lower root");let upper=occurrence(&document,"Upper");let lower=occurrence(&document,"Lower");
    document.artwork.occurrences.get_mut(group).unwrap().translation=Point{x:17.,y:23.};
    document.artwork.occurrences.get_mut(original).unwrap().placement=layer_core::LayerPlacement::from_affine(Affine([0.9,0.2,-0.2,0.9,7.,-1.]));
    let (above,above_target)=crate::test_support::add_paint(&mut document.artwork,"Above failed",[33,17]);
    let (pending,pending_target)=crate::test_support::add_paint(&mut document.artwork,"Above pending",[33,17]);
    let (hidden,hidden_target)=crate::test_support::add_paint(&mut document.artwork,"Hidden failed",[33,17]);
    let hidden_group=add_group(&mut document,vec![hidden],0);document.artwork.occurrences.get_mut(hidden_group).unwrap().visible=false;
    let library=document.artwork.paint.insert(PortableId::random(),PaintSource {domain:[33,17],raster:Default::default(),original:None,operations:Arc::default()}).unwrap();
    let root=document.composition().result;let entries=&mut document.artwork.stacks.get_mut(root).unwrap().entries;
    entries.retain(|h|*h!=above&&*h!=pending);entries.splice(0..0,[above,pending]);refresh(&mut document);
    for (source,error) in [(above_target,"above source failed"),(hidden_target,"hidden source failed"),(SourceTarget::Paint(library),"unplaced source failed")] {
        let raster=document.target_raster_mut(source).unwrap();*raster=RasterRevision::pending();raster.publish(Err(error.into())).unwrap();
    }
    *document.target_raster_mut(pending_target).unwrap()=RasterRevision::pending();
    let mut query=layer_core::ArtworkQuery::new(&document,ArtworkSource::EffectInput(upper));
    query.set_context(EvaluationContext {elapsed:42.,phases:vec![(document.scene().effect_handle(lower).unwrap(),3.25)].into()});
    let scene=SnapshotGpu::artwork_source_scene(&query).unwrap();
    let geometry=scene.view().target_geometry(target);let context=scene.context.clone();
    let artwork=SnapshotGpu::scoped_transfer_artwork(&scene,&[]);
    for source in [above_target,pending_target,hidden_target,SourceTarget::Paint(library)] {
        let SourceTarget::Paint(handle)=source else {unreachable!()};assert!(artwork.paint.get(handle).unwrap().raster.is_empty());
        assert!(document.target_raster(source).unwrap().try_data().is_none_or(|data|data.is_err()));
    }
    let SourceTarget::Paint(paint)=target else {unreachable!()};
    assert_eq!(artwork.paint.get(paint).unwrap().raster,document.target_raster(target).unwrap().clone());
    assert!(Arc::ptr_eq(artwork.paint.get(paint).unwrap().original.as_ref().unwrap(),document.scene().original(target).unwrap()));
    let checkpoint=CaptureCheckpoint {document:artwork.id,owner:scene.owner,session_generation:0,artwork_generation:scene.revision,working_generation:0,edit_checkpoint:0};
    let cancel=std::sync::atomic::AtomicBool::new(false);
    let capture=artwork.capture(checkpoint).unwrap();
    let transfer=PreparedTransfer::capture(&capture,&cancel).unwrap();
    let adopted=transfer.adopt_verified(Default::default(),&cancel).unwrap();
    let index=Arc::new(SceneIndex::build(&adopted.artwork).unwrap());
    let restored=SceneSnapshot::new((*adopted.artwork).clone(),index,scene.owner,scene.revision,context.clone()).with_scope(scene.scope.clone());
    assert_eq!(restored.view().target_geometry(target),geometry);
    assert_eq!(restored.context,context);
    assert_eq!(restored.view().effect_handle(upper),scene.view().effect_handle(upper));
    assert_eq!(restored.view().order(),scene.view().order());
    let mut failed=(*scene).clone();let raster=&mut failed.artwork.paint.get_mut(paint).unwrap().raster;
    *raster=RasterRevision::pending();raster.publish(Err("required source failed".into())).unwrap();
    let failed=SnapshotGpu::scoped_transfer_artwork(&failed,&[]).capture(checkpoint).unwrap();
    let error=match PreparedTransfer::capture(&failed,&cancel) {Ok(_)=>panic!("required backing was omitted"),Err(error)=>error};
    assert!(error.contains("required source failed"),"{error}");
}

fn mixed_dehaze_project(space:RgbSpace,alpha:f32,dehaze_below:bool,amount:f32)->Document {
    let mut document=constant_document(space,alpha);
    let target=occurrence(&document,if dehaze_below {"Lower"}else{"Upper"});
    set_effect(&mut document,target,adjustment("dehaze","amount",amount));document
}

#[test]
fn cold_mixed_dehaze_constant_airlight_preserves_exact_scoped_lower_order() {
    for (space, alpha) in [(RgbSpace::Srgb, 0.5), (RgbSpace::DisplayP3, 1.),
        (RgbSpace::AdobeRgb, 8e-8), (RgbSpace::ProPhoto, 0.5)] {
            for dehaze_below in [false, true] {
                for amount in [-100., 0., 100.] {
                    let project = mixed_dehaze_project(space, alpha, dehaze_below, amount);
                    let y = f64::from(0.01125_f32);
                    let expected = y * shadow_gain(y, 0.5);
                    let mut reader = capture(project.clone()).unwrap();
                    assert!(reader.renderer.effect_analyses.is_empty());
                    close(reader.read_region([7, 5, 1, 1]).unwrap()[0], expected, alpha, true);
                    let kinds = reader.renderer.effect_analyses.iter().map(|entry| (entry.layer(), entry.kind)).collect::<Vec<_>>();
                    let dehaze = layer_core::EffectAnalysisKind::Dehaze;
                    let illumination = layer_core::EffectAnalysisKind::LocalIllumination;
                    if amount != 0. {
                        assert_eq!(kinds, vec![(occurrence(&project,"Lower"), if dehaze_below { dehaze } else { illumination }),
                            (occurrence(&project,"Upper"), if dehaze_below { illumination } else { dehaze })]);
                    }
                    for entry in &reader.renderer.effect_analyses {
                        assert_eq!(entry.query.source, ArtworkSource::EffectInput(entry.layer()));
                    }
                    let lower = if dehaze_below { y } else { expected };
                    let request = ArtworkSampleRequest::new(&project, ArtworkSource::EffectInput(occurrence(&project,"Upper")), [8., 6.], 1);
                    let ArtworkSample::Color(pixel) = pollster::block_on(gpu().artwork_sample(request, Default::default())).unwrap()
                        else { panic!("mixed Dehaze source sample") };
                    close(pixel, lower, alpha, false);
                }
            }
    }
}

#[test]
fn dehaze_own_amount_reuses_airlight_and_lower_edits_replace_upper_scoped_guide() {
    for dehaze_below in [false, true] {
        let mut project = mixed_dehaze_project(RgbSpace::Srgb, 0.5, dehaze_below, 75.);
        let mut reader = capture(project.clone()).unwrap();
        reader.read_region([0, 0, 1, 1]).unwrap();
        let lower = resource(&reader,occurrence(&project,"Lower"));
        let upper = resource(&reader,occurrence(&project,"Upper"));
        let upper_key = if dehaze_below { "shadows" } else { "amount" };
        let upper_handle=occurrence(&project,"Upper");let lower_handle=occurrence(&project,"Lower");
        set(&mut project,upper_handle,upper_key,25.);
        let mut own = reader.renderer.snapshot_gpu().capture_scene(project.snapshot(),SceneScope::All,Default::default()).unwrap();
        own.read_region([0, 0, 1, 1]).unwrap();
        assert!(Arc::ptr_eq(&lower, &resource(&own,lower_handle)));
        assert!(Arc::ptr_eq(&upper, &resource(&own,upper_handle)));
        let lower_key = if dehaze_below { "amount" } else { "shadows" };
        set(&mut project,lower_handle,lower_key,30.);
        let mut contributing = own.renderer.snapshot_gpu().capture_scene(project.snapshot(),SceneScope::All,Default::default()).unwrap();
        contributing.read_region([0, 0, 1, 1]).unwrap();
        assert!(Arc::ptr_eq(&lower, &resource(&contributing,lower_handle)));
        assert!(!Arc::ptr_eq(&upper, &resource(&contributing,upper_handle)));
    }
}

#[test]
fn dehaze_effect_input_excludes_unrelated_failed_upper_backing() {
    let mut project = mixed_dehaze_project(RgbSpace::Srgb, 0.5, true, 100.);
    let (upper,target)=crate::test_support::add_paint(&mut project.artwork,"Unrelated failed upper backing",[33,17]);
    let root=project.composition().result;let entries=&mut project.artwork.stacks.get_mut(root).unwrap().entries;
    entries.retain(|handle|*handle!=upper);entries.insert(0,upper);refresh(&mut project);
    let raster=project.target_raster_mut(target).unwrap();*raster=RasterRevision::pending();
    raster.publish(Err("upper unrelated backing failed".into())).unwrap();
    let request = ArtworkSampleRequest::new(&project, ArtworkSource::EffectInput(occurrence(&project,"Upper")), [8., 6.], 1);
    let ArtworkSample::Color(pixel) = pollster::block_on(gpu().artwork_sample(request, Default::default())).unwrap()
        else { panic!("source-aware Dehaze sample") };
    close(pixel, f64::from(0.01125_f32), 0.5, false);
}

#[test]
fn registered_dehaze_nonconstant_material_matches_scalar_reconstruction() {
    let extent=[2,1];
    let pixels=[[0.4_f32,0.5,0.6,1.],[0.1,0.2,0.3,1.]];
    let mut document=Document::new(PortableId::random(),extent[0],extent[1],
        layer_core::DocumentNames{paint:"Original".into(),paper:"Paper".into()});
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().color=DocumentColor{space:RgbSpace::Srgb,depth:SampleDepth::F32};
    let interpretation=SourceInterpretation{channels:SourceChannels::Rgba,depth:SampleDepth::F32,
        profile:ColorProfile::Builtin(RgbSpace::Srgb),profile_assumed:false};
    let mut source=SourceBuilder::new(extent,interpretation,1024*1024).unwrap();
    let row=pixels.into_iter().flatten().flat_map(f32::to_le_bytes).collect::<Vec<_>>();
    source.push_row(&row).unwrap();
    paint_mut(&mut document).original=Some(Arc::new(source.finish().unwrap()));hide_paper(&mut document);
    let target=insert_effect(&mut document,adjustment("dehaze","amount",0.),0);
    for amount in [0_f32,50.,100.,-100.] {
        set(&mut document,target,"amount",amount);
        let mut reader=capture(document.clone()).unwrap();
        let actual=reader.read_region([0,0,2,1]).unwrap();
        let transmission=(1.-0.95*f64::from(amount.abs())*0.01*f64::from(pixels[1][0])/f64::from(pixels[0][0])).max(0.1);
        for (index,pixel) in actual.iter().enumerate(){
            assert_eq!(pixel[3],1.);
            for channel in 0..3{
                let input=f64::from(pixels[index][channel]);let air=f64::from(pixels[0][channel]);
                let expected=if amount>=0.{(input-air)/transmission+air}else{input*transmission+air*(1.-transmission)};
                assert!(pixel[channel].is_finite()&&(f64::from(pixel[channel])-expected).abs()<=1e-5,
                    "amount {amount} pixel {index} channel {channel}: {} expected {expected}",pixel[channel]);
            }
        }
        assert_eq!(reader.renderer.effect_analyses.len(),1);assert_eq!(reader.renderer.effect_analyses[0].query.source,ArtworkSource::EffectInput(target));
    }
}
