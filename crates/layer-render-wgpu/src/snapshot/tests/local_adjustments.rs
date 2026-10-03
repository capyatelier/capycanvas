use super::*;
use layer_core::{ArtworkSample, ArtworkSampleRequest, ArtworkSource, EffectValue, LayerBlend};
use layer_engine::{CanvasEngine, ViewTransform, input_queue};

fn engine(document:Document)->CanvasEngine<WgpuRasterizer> {
    let extent=[document.width,document.height];
    let renderer=WgpuRasterizer::new_native_headless(document.color).unwrap();
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

fn baked_pixel(engine:&CanvasEngine<WgpuRasterizer>,id:LayerId,x:u32,y:u32)->[f32;4] {
    let data=engine.document().layer(id).unwrap().raster.wait_data().unwrap();
    let tile=data.tiles.get(&TileKey{plane:layer_core::raster::RasterPlane::Color,coordinate:[0,0]}).unwrap();
    let bytes=tile.wait_backing().unwrap().decode().unwrap();
    let offset=((y*256+x)*16) as usize;
    std::array::from_fn(|channel|f32::from_le_bytes(bytes[offset+channel*4..offset+channel*4+4].try_into().unwrap()))
}

fn merge(engine:&mut CanvasEngine<WgpuRasterizer>)->LayerId {
    let (result,coverage)=(engine.allocate_layer_id(),engine.allocate_layer_id());
    let plan=engine.document().merge_plan(layer_core::MergeKind::Down,result,coverage).unwrap();
    engine.insert_with_operations(plan.edits,vec![(result,plan.operation)],None).unwrap();result
}

fn adjustment(id:u64,kind:&str,key:&str,value:f32)->Layer {
    let mut layer=Layer::paint(LayerId(id),kind);layer.kind=LayerKind::Effect;
    let mut effect=EffectInstance::new(crate::tests::fixture(kind).program().for_depth(SampleDepth::F32));
    effect.set(key,EffectValue::Number(value)).unwrap();layer.effect=Some(Arc::new(effect));layer
}

fn set(document:&mut Document,id:u64,key:&str,value:f32) {
    Arc::make_mut(document.layers.iter_mut().find(|l|l.id==LayerId(id)).unwrap().effect.as_mut().unwrap())
        .set(key,EffectValue::Number(value)).unwrap();
}

fn constant_project(space:RgbSpace,alpha:f32)->Project {
    let extent=[33,17];let mut document=Document::new("Cold local analysis",extent[0],extent[1],
        layer_core::DocumentNames{paint:"Original".into(),paper:"Paper".into()});
    document.color=DocumentColor{space,depth:SampleDepth::F32};
    for _ in 0..100{document.allocate_layer_id();}
    let interpretation=SourceInterpretation{channels:SourceChannels::Rgba,depth:SampleDepth::F32,
        profile:ColorProfile::Builtin(space),profile_assumed:false};
    let mut source=SourceBuilder::new(extent,interpretation,1024*1024).unwrap();
    let row=(0..extent[0]).flat_map(|_|[0.01125_f32,0.01125,0.01125,alpha]).flat_map(f32::to_le_bytes).collect::<Vec<_>>();
    for _ in 0..extent[1]{source.push_row(&row).unwrap();}
    let mut original=document.layers[0].clone();original.source=Some(Arc::new(source.finish().unwrap()));
    let mut group=Layer::paint(LayerId(30),"Lower root");group.kind=LayerKind::Group;group.properties.blend=LayerBlend::Normal;
    original.properties.parent=Some(group.id);
    let mut lower=adjustment(10,"shadows_highlights","shadows",50.);lower.properties.parent=Some(group.id);
    document.layers=vec![lower,adjustment(20,"shadows_highlights","shadows",50.),group,original];
    Project{document}
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

fn resource(reader:&SnapshotRenderer,id:u64)->Arc<crate::effects::resources::Resource> {
    reader.renderer.effect_analyses.iter().find(|entry|entry.layer()==LayerId(id)).unwrap().resource.clone()
}

#[test]
fn cold_noncontiguous_local_adjustments_prepare_lower_before_upper_for_regions_and_samples() {
    for space in RgbSpace::ALL {for alpha in [1.,0.5,8e-8] {
        let project=constant_project(space,alpha);let y=f64::from(0.01125_f32);let expected=output(y,0.5,0.5);
        let mut reader=capture(project.clone()).unwrap();assert!(reader.renderer.effect_analyses.is_empty());
        for pixel in reader.read_region([7,5,3,2]).unwrap(){close(pixel,expected,alpha,true);}
        assert_eq!(reader.renderer.effect_analyses.len(),2);
        assert_eq!(reader.renderer.effect_analyses.iter().map(|entry|entry.layer()).collect::<Vec<_>>(),[LayerId(10),LayerId(20)]);
        for (source,gray) in [(ArtworkSource::Visible,expected),(ArtworkSource::EffectInput(LayerId(20)),y*shadow_gain(y,0.5))] {
            let result=pollster::block_on(gpu().artwork_sample(ArtworkSampleRequest::new(&project.document,source,[8.,6.],1),Default::default())).unwrap();
            let ArtworkSample::Color(pixel)=result else{panic!("cold local sample {result:?}")};close(pixel,gray,alpha,false);
        }
    }}
}

#[test]
fn local_analysis_reuses_own_amount_lease_and_invalidates_upper_after_lower_edits() {
    let mut project=constant_project(RgbSpace::Srgb,1.);let y=f64::from(0.01125_f32);
    let mut reader=capture(project.clone()).unwrap();reader.read_region([0,0,1,1]).unwrap();
    let lower=resource(&reader,10);let upper=resource(&reader,20);let weak=Arc::downgrade(&upper);
    let inherited=reader.renderer.snapshot_gpu();
    set(&mut project.document,20,"shadows",25.);
    let mut edited=inherited.capture(project.clone(),[0.;4],0.,Default::default()).unwrap();
    close(edited.read_region([0,0,1,1]).unwrap()[0],output(y,0.5,0.25),1.,true);
    assert!(Arc::ptr_eq(&lower,&resource(&edited,10)));assert!(Arc::ptr_eq(&upper,&resource(&edited,20)));
    set(&mut project.document,10,"shadows",25.);
    let mut changed=edited.renderer.snapshot_gpu().capture(project,[0.;4],0.,Default::default()).unwrap();
    close(changed.read_region([0,0,1,1]).unwrap()[0],output(y,0.25,0.25),1.,true);
    assert!(Arc::ptr_eq(&lower,&resource(&changed,10)));assert!(!Arc::ptr_eq(&upper,&resource(&changed,20)));
    drop(changed);drop(edited);drop(reader);drop(inherited);drop(upper);assert!(weak.upgrade().is_none());
}

#[test]
fn hidden_ancestor_local_adjustments_need_no_guide_and_cancelled_chain_cannot_publish_output() {
    let mut project=constant_project(RgbSpace::Srgb,0.5);
    let mut hidden=Layer::paint(LayerId(40),"Hidden root");hidden.kind=LayerKind::Group;hidden.visible=false;
    let mut effect=adjustment(41,"clarity","amount",100.);effect.properties.parent=Some(hidden.id);
    project.document.layers.insert(0,effect);project.document.layers.push(hidden);
    let control=CaptureControl::default();let mut reader=gpu().capture(project,[0.;4],0.,control.clone()).unwrap();
    pollster::block_on(reader.prepare_effect_analysis_async(scene::Output::EffectInput(LayerId(20)))).unwrap();
    assert_eq!(reader.renderer.effect_analyses.len(),1);assert_eq!(reader.renderer.effect_analyses[0].layer(),LayerId(10));
    control.cancel();assert!(reader.read_region([0,0,1,1]).unwrap_err().to_string().contains("cancel"));
    assert_eq!(reader.renderer.effect_analyses.len(),1);
    let project=(*reader.document).clone();
    let mut full=capture(Project{document:project}).unwrap();
    close(full.read_region([0,0,1,1]).unwrap()[0],output(f64::from(0.01125_f32),0.5,0.5),0.5,true);
    assert_eq!(full.renderer.effect_analyses.len(),2);assert!(!full.analysis_ready.contains(&LayerId(41)));
}

#[test]
fn local_analysis_honors_frozen_lower_animation_phase_and_clarity_constant_identity() {
    let mut project=constant_project(RgbSpace::Srgb,1.);
    let mut program=(*crate::tests::fixture("domain_warp").program()).clone();program.kind=layer_core::EffectKind::Generator;
    program.entry="local_phase_gray".into();program.passes=Arc::new([]);
    program.wgsl="fn local_phase_gray(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{let y=.01125*exp2(fx_time(b));return vec4<f32>(y,y,y,1.);}".into();
    let mut effect=EffectInstance::new(Arc::new(program));effect.set("animate",EffectValue::Toggle(true)).unwrap();effect.set("speed",EffectValue::Number(1.)).unwrap();
    let original=project.document.layers.iter_mut().find(|layer|layer.id==LayerId(1)).unwrap();
    original.kind=LayerKind::Effect;original.source=None;original.effect=Some(Arc::new(effect));
    let mut inherited=gpu();let mut previous=None;
    for phase in [0.,1.] {
        let mut request=ArtworkSampleRequest::new(&project.document,ArtworkSource::EffectInput(LayerId(20)),[8.;2],1);
        request.effect_times.push((LayerId(1),phase));
        let (mut snapshot,output)=pollster::block_on(inherited.artwork_capture(&request.query,Default::default())).unwrap();
        pollster::block_on(snapshot.prepare_effect_analysis_async(output)).unwrap();
        let analysis=snapshot.renderer.effect_analyses.iter().find(|entry|entry.layer()==LayerId(10)).unwrap();
        assert_eq!(analysis.query.effect_times,[(LayerId(1),phase)]);
        if let Some(old)=previous{assert!(!Arc::ptr_eq(&old,&analysis.resource));}previous=Some(analysis.resource.clone());
        let result=pollster::block_on(inherited.artwork_sample(request,Default::default())).unwrap();
        let ArtworkSample::Color(pixel)=result else{panic!("phase sample {result:?}")};
        let y=f64::from(0.01125_f32)*f64::from(phase).exp2();close(pixel,y*shadow_gain(y,0.5),1.,false);
        inherited=snapshot.renderer.snapshot_gpu();
    }
    let mut project=constant_project(RgbSpace::Srgb,0.5);
    project.document.layers.retain(|layer|layer.id!=LayerId(10)&&layer.id!=LayerId(20));
    project.document.layers.insert(0,adjustment(20,"clarity","amount",100.));
    let mut reader=capture(project).unwrap();close(reader.read_region([0,0,1,1]).unwrap()[0],f64::from(0.01125_f32),0.5,true);
    assert_eq!(reader.renderer.effect_analyses.len(),1);
}

#[test]
fn cold_merge_prepares_retained_noncontiguous_members_and_preserves_group_offset() {
    let mut document=constant_project(RgbSpace::Srgb,0.5).document;
    document.layers.iter_mut().find(|layer|layer.id==LayerId(30)).unwrap().properties.offset=Point{x:3.,y:2.};
    document.active_layer=LayerId(20);
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
    let mut document=constant_project(RgbSpace::Srgb,0.5).document;
    document.blend_space=layer_core::BlendSpace::Perceptual;
    let members:Arc<[Layer]>=document.layers.clone().into();
    let original=members.iter().find(|layer|layer.id==LayerId(1)).unwrap().clone();
    let mut engine=engine(document);
    let (low,high,coverage)=(engine.allocate_layer_id(),engine.allocate_layer_id(),engine.allocate_layer_id());
    let operation=|kind|layer_core::LayerOperation{placement:Affine::IDENTITY,
        coverage:LayerMask::reveal_all(coverage,Point::default()),kind};
    let edits=vec![layer_core::Edit::InsertLayer{index:0,layer:Layer::paint(low,"Low")},
        layer_core::Edit::InsertLayer{index:0,layer:Layer::paint(high,"High")}];
    engine.insert_with_operations(edits,vec![(low,operation(layer_core::LayerOperationKind::Bake{
        members:vec![original].into(),offset:Point::default()})),
        (high,operation(layer_core::LayerOperationKind::FrequencyDetail{members,offset:Point::default(),low}))],None).unwrap();
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
    let mut document=constant_project(RgbSpace::Srgb,0.5).document;document.active_layer=LayerId(20);
    let bytes=(0..256*256).flat_map(|_|[0.01125_f32*0.5,0.01125*0.5,0.01125*0.5,0.5]).flat_map(f32::to_le_bytes).collect::<Vec<_>>();
    let mut blob=TileBlob::encode(document.color.paint_descriptor(),&bytes).unwrap();blob.digest[0]^=1;
    let mut data=layer_core::raster::RasterData::default();
    data.tiles.insert(TileKey{plane:layer_core::raster::RasterPlane::Color,coordinate:[0,0]},RasterTile::backed(blob));
    document.layers.iter_mut().find(|layer|layer.id==LayerId(1)).unwrap().raster=RasterRevision::backed(data);
    let mut engine=engine(document);merge(&mut engine);
    let error=finish(&mut engine).unwrap_err();
    assert!(error.contains("digest")||error.contains("integrity"),"unexpected terminal bake error: {error}");
}

#[test]
fn cold_color_candidate_prepares_local_guides_before_its_first_submission_and_cancels() {
    let project=constant_project(RgbSpace::DisplayP3,0.5);
    let make=|control|gpu().color_canvas(project.clone(),&Default::default(),crate::test_support::view([33,17]),0.,control).unwrap();
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
    let mut project=constant_project(RgbSpace::Srgb,0.5);
    let mut unrelated=Layer::paint(LayerId(90),"Unrelated upper input");
    unrelated.raster=RasterRevision::pending();unrelated.raster.publish(Err("upper unrelated backing failed".into())).unwrap();
    project.document.layers.insert(0,unrelated);
    let request=ArtworkSampleRequest::new(&project.document,ArtworkSource::EffectInput(LayerId(20)),[8.,6.],1);
    let result=pollster::block_on(gpu().artwork_sample(request,Default::default())).unwrap();
    let ArtworkSample::Color(pixel)=result else{panic!("source-specific local sample {result:?}")};
    let y=f64::from(0.01125_f32);close(pixel,y*shadow_gain(y,0.5),0.5,false);

    let mut project=constant_project(RgbSpace::Srgb,0.5);
    let mut hidden=Layer::paint(LayerId(91),"Hidden root");hidden.kind=LayerKind::Group;hidden.visible=false;
    let mut child=Layer::paint(LayerId(92),"Hidden failed input");child.properties.parent=Some(hidden.id);
    child.raster=RasterRevision::pending();child.raster.publish(Err("hidden descendant backing failed".into())).unwrap();
    project.document.layers.insert(0,child);project.document.layers.push(hidden);
    let request=ArtworkSampleRequest::new(&project.document,ArtworkSource::Visible,[8.,6.],1);
    let result=pollster::block_on(gpu().artwork_sample(request,Default::default())).unwrap();
    let ArtworkSample::Color(pixel)=result else{panic!("effective-hidden local sample {result:?}")};
    close(pixel,output(y,0.5,0.5),0.5,false);
}

#[test]
fn cold_clone_reference_prepares_local_guides_without_a_settled_preview() {
    let mut document=constant_project(RgbSpace::Srgb,0.5).document;
    let target=LayerId(90);document.layers.insert(0,Layer::paint(target,"Clone result"));document.active_layer=target;
    document.reference_layers=[LayerId(20),LayerId(30)].into();
    let renderer=WgpuRasterizer::new_native_headless(document.color).unwrap();
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
    let mut moved=engine.document().layer(target).unwrap().clone();moved.properties.offset=Point{x:8.,y:0.};
    engine.apply_edit(layer_core::Edit::ReplaceLayer(Box::new(moved))).unwrap();
    let mut source=layer_core::CloneSource::default();source.aligned=false;source.set(Point{x:3.,y:6.});engine.set_clone_source(source);
    for (sequence,phase) in [(3,layer_engine::PenPhase::Down),(4,layer_engine::PenPhase::Up)] {
        input.push(crate::test_support::pen(sequence,phase,[20.,14.],layer_engine::SampleFlags::PRIMARY)).unwrap();
    }
    finish(&mut engine).unwrap();
    close(baked_pixel(&engine,target,12,14),output(f64::from(0.01125_f32),0.5,0.5),0.5,false);
}

#[test]
fn dropping_a_pending_cold_merge_releases_members_without_publishing_a_partial_raster() {
    let mut document=constant_project(RgbSpace::Srgb,0.5).document;document.active_layer=LayerId(20);
    let source=Arc::downgrade(document.layer(LayerId(1)).unwrap().source.as_ref().unwrap());
    let mut engine=engine(document);let result=merge(&mut engine);
    engine.render_frame_at(0).unwrap();assert_eq!(engine.backend().bake_analyses.len(),1);
    assert!(engine.has_pending_document_edits());
    let raster=engine.document().layer(result).unwrap().raster.clone();assert!(raster.try_data().is_none());
    drop(engine);
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(15);
    while source.upgrade().is_some(){assert!(std::time::Instant::now()<deadline,"cancelled merge retained its input");std::thread::yield_now();}
    assert!(raster.try_data().is_none(),"cancelled guide work published a partial bake");
}
