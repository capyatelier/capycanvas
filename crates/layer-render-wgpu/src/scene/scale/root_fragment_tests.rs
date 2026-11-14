use super::*;
use layer_core::EffectValue;

fn inputs(r:&mut WgpuRasterizer,doc:&Document,blend:layer_core::BlendSpace)->(Box<Scene>,Cache,Vec<Image>) {
    let mut frame=packet(doc.scene(),doc.composition().size);
    frame.view.document_to_surface=[0.25,0.,0.,0.25,0.,0.];
    frame.blend_space=blend;
    crate::test_support::submit_settled(r, frame);
    let scene=r.scene.take().unwrap();let cache=r.scale_display.take().unwrap();
    let images=doc.scene().order().iter().map(|h|{let id=doc.scene().source_target(*h).unwrap();source_image(r,&cache,&scene.scale_sources.entries[&id],id,cache.plan)}).collect();
    (scene,cache,images)
}

fn capture(r:&mut WgpuRasterizer,scene:&mut Scene,cache:&mut Cache,doc:&Document,images:&[Image],deferred:bool,blend:layer_core::BlendSpace)->(Vec<[f32;4]>,usize,u64) {
    let plan=cache.output_plan();let root=cache.pixels.root().unwrap().view.clone();
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    let mut unit=Unit::default();let mut frame=packet(doc.scene(),doc.composition().size);frame.blend_space=blend;
    for rect in [PixelRect::new(0,0,3,3),PixelRect::new(6,2,11,7),PixelRect::new(plan.size[0]-2,plan.size[1]-2,plan.size[0],plan.size[1])] {
        let side=1<<plan.level;
        let region=PixelRect::new(rect.min_x()*side,rect.min_y()*side,rect.max_x()*side,rect.max_y()*side).intersect(plan.bounds);
        let mut accepted=crate::scene::sources::AcceptedInputs::default();
        let mut e=Evaluator {class:crate::residency::Class::Visible,accepted:&mut accepted,cache,scene,packet:frame,r,encoder:&mut encoder,region:region.into(),input:plan,tiled:false,
            unit:&mut unit,previous_motion:None,retained:0,approximate:false,preview:false};
        let value=|image:&Image|Value::Image {view:image.view.clone(),slot:None,opacity:1.,plan:image.plan,preview:None,encode:false};
        e.draw(value(&images[0]),value(&images[1]),layer_core::LayerBlend::Normal,0,Some(Target {view:root.clone(),slot:Some(Slot::Root),plan})).unwrap();
        if !deferred {e.scene.encode_jobs(e.r,e.encoder).unwrap();}
    }
    let count=scene.jobs.len();scene.encode_jobs(r,&mut encoder).unwrap();
    let passes=encoder.pass_count();r.uploads.finish(&encoder);encoder.submit(&r.queue);
    (pixels(r,cache.texture()),count,passes)
}

#[test]
fn root_deferred_retained_images_preserve_float_bits_and_disjoint_scissors() {
    for blend in layer_core::BlendSpace::ALL {
        let mut doc=document_at([65,33]);composition_mut(&mut doc).color.depth=SampleDepth::F32;
        let back=paint_occurrence(&mut doc,"photo",Some(rgba8_source([65,33],|_,_|[79,101,151,193])));insert_occurrence(&mut doc,back,1);
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let (mut scene,mut cache,images)=inputs(&mut r,&doc,blend);
        let plan=cache.output_plan();assert_eq!(plan.bounds,PixelRect::full(plan.extent));
        for image in &images {assert_eq!(image.plan,plan);}
        let colors=[[-0.,0.,-0.,0.],[0.,-0.,0.,-0.],[1e-30,-1e-30,2e-30,1e-30],[-2.,3.,1e5,0.37],[0.2,0.1,0.7,1.]];
        for (i,image) in images.iter().enumerate() {
            let bytes=(0..image.plan.size[0]*image.plan.size[1]).flat_map(|p|colors[(p as usize+i)%colors.len()].into_iter().flat_map(f32::to_le_bytes)).collect::<Vec<_>>();
            r.queue.write_texture(image.texture.as_image_copy(),&bytes,wgpu::TexelCopyBufferLayout {offset:0,bytes_per_row:Some(image.plan.size[0]*16),rows_per_image:Some(image.plan.size[1])},image.texture.size());
        }
        let before=pixels(&r,cache.texture());
        let (reference,count,passes)=capture(&mut r,&mut scene,&mut cache,&doc,&images,false,blend);assert_eq!(count,0);assert_eq!(passes,3);
        let bytes=before.iter().flat_map(|p|p.iter().flat_map(|v|v.to_le_bytes())).collect::<Vec<_>>();
        r.queue.write_texture(cache.texture().as_image_copy(),&bytes,wgpu::TexelCopyBufferLayout {offset:0,bytes_per_row:Some(plan.size[0]*16),rows_per_image:Some(plan.size[1])},cache.texture().size());
        let (actual,count,passes)=capture(&mut r,&mut scene,&mut cache,&doc,&images,true,blend);assert_eq!(count,3);assert_eq!(passes,1);
        for (i,(actual,expected)) in actual.iter().zip(&reference).enumerate() {assert_eq!(actual.map(f32::to_bits),expected.map(f32::to_bits),"{blend:?} pixel{i}: {actual:?} != {expected:?}");}
    }
}

#[test]
fn root_deferred_rejects_unretained_and_changed_coordinate_inputs() {
    let mut doc=document_at([65,33]);let back=paint_occurrence(&mut doc,"photo",Some(rgba8_source([65,33],|_,_|[79,101,151,193])));insert_occurrence(&mut doc,back,1);
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();let (mut scene,mut cache,images)=inputs(&mut r,&doc,doc.composition().blend);
    let plan=cache.output_plan();let root=cache.pixels.root().unwrap().view.clone();
    for case in 0..10 {
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());let mut unit=Unit::default();let mut front=Value::Image {view:images[0].view.clone(),slot:None,opacity:1.,plan,preview:None,encode:false};
        let mut back=Value::Image {view:images[1].view.clone(),slot:None,opacity:1.,plan,preview:None,encode:false};
        let Value::Image {view,slot,opacity,plan:source,preview,encode}=&mut front else {unreachable!()};
        match case {
            0=>*opacity=0.7,1=>*encode=true,2=>*preview=Some(r.empty_view.clone()),
            3=>*source=display_mips::Plan::window(plan.extent,plan.level,PixelRect::new(4,0,65,33)),
            4=>*view=root.clone(),5=>*view=Image::new(&r,plan,"unretained input").view,
            6=>{let index=cache.allocate(&r,plan).unwrap();*slot=Some(Slot::Cache(index));},
            7=>{let Value::Image {opacity,..}=&mut back else {unreachable!()};*opacity=0.5;},_=>{},
        }
        let mut frame=packet(doc.scene(),doc.composition().size);frame.blend_space=doc.composition().blend;
        let mut accepted=crate::scene::sources::AcceptedInputs::default();
        let mut e=Evaluator {class:crate::residency::Class::Visible,accepted:&mut accepted,cache:&mut cache,scene:&mut scene,packet:frame,r:&mut r,encoder:&mut encoder,region:PixelRect::new(0,0,12,12).into(),input:plan,tiled:false,unit:&mut unit,previous_motion:None,retained:0,approximate:false,preview:false};
        e.draw(front,back,if case==8 {layer_core::LayerBlend::Multiply}else{layer_core::LayerBlend::Normal},if case==9 {16}else{0},Some(Target {view:root.clone(),slot:Some(Slot::Root),plan})).unwrap();
        assert_eq!(encoder.pass_count(),0,"case{case} must stage ordinary jobs");assert!(!scene.jobs.is_empty());scene.discard_unencoded_jobs();
    }
}

#[test]
fn root_deferred_native_alpha_over_photo_matches_full_composition() {
    let extent=[517,259];
    for name in ["brightness_to_opacity","threshold"] {for blend in layer_core::BlendSpace::ALL {
        let mut doc=document_at(extent);
        paint_mut(&mut doc,0).base = (Some(rgba8_source(extent,|x,y|[if (x/3+y/2)%2==0 {126}else{130},(x*17) as u8,(y*23) as u8,if (x/7+y/11)%3==0 {0}else{97+(x%159) as u8}]))).map(|source|layer_core::authored::PaintBase::new(source.into()));
        let owner=doc.scene().order()[0];let target=source_at(&doc,0);
        let filter=effect_occurrence(&mut doc,layer_core::EffectInstance::new(crate::tests::fixture(name).program()),name);doc.artwork.occurrences.get_mut(filter).unwrap().attachment=Attachment::Effect;insert_occurrence(&mut doc,filter,0);
        if name=="threshold" {set_effect_value(&mut doc,filter,"colors",EffectValue::Choice(1));set_effect_value(&mut doc,filter,"transparency",EffectValue::Choice(1));set_effect_value(&mut doc,filter,"alpha_threshold",EffectValue::Number(37.));}
        let photo=paint_occurrence(&mut doc,"photo",Some(rgba8_source(extent,|x,y|[(x*29) as u8,(y*31) as u8,173,211])));insert_occurrence(&mut doc,photo,2);
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        for state in 0..3 {
            if state==1 {doc.artwork.occurrences.get_mut(owner).unwrap().opacity=0.61;}
            if state==2 {doc.artwork.occurrences.get_mut(owner).unwrap().opacity=1.;coverage_mask(&mut doc,filter,Default::default(),Some(layer_core::Selection::polygon(vec![layer_core::Point{x:11.,y:7.},layer_core::Point{x:501.,y:17.},layer_core::Point{x:127.,y:243.}]).unwrap()));}
            let mut frame=packet(doc.scene(),extent);frame.blend_space=blend;frame.composite_all=false;frame.view.document_to_surface=[0.25,0.,0.,0.25,0.,0.];
            submit_published(&mut r, frame);
            for position in [[123.5,125.5],[389.5,247.5]] {
                let mut dab=crate::tests::test_dab(position,[0.2,0.1,0.3,0.37],0.9);dab.radii=[11.75,9.25];
                let mut batch=dab_batch(target,crate::layer_tests::preset_style(DefaultBrushPreset::GPen),dab.bounds());batch.style.blend_space=blend;
                crate::test_support::submit_ready(&mut r, FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
                let incremental=display_pixels(&r);
                r.scale_display.as_ref().unwrap().pages.lock().unwrap().remove_composites();
                r.scale_display=None;submit_published(&mut r, FramePacket {composite_all:true,..frame});
                let expected=display_pixels(&r);
                for (pixel,(actual,expected)) in incremental.iter().zip(&expected).enumerate() {assert_eq!(actual.map(f32::to_bits),expected.map(f32::to_bits),"{name} {blend:?} state{state} {position:?} pixel{pixel}");}
            }
        }
    }}
}

#[test]
fn root_deferred_flushes_before_placed_root_fallback() {
    let mut doc=document_at([65,33]);let back=paint_occurrence(&mut doc,"photo",Some(rgba8_source([65,33],|_,_|[79,101,151,193])));insert_occurrence(&mut doc,back,1);
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();let (mut scene,mut cache,images)=inputs(&mut r,&doc,doc.composition().blend);
    let plan=cache.output_plan();cache.pixels=Default::default();cache.pixels.ensure_root(&r,plan,wgpu::TextureFormat::Rgba32Float,"composition window",r.scene_tiers.windows.charge(0));cache.pixels.ensure(&r,plan,wgpu::TextureFormat::Rgba32Float,r.scene_tiers.windows.charge(0));
    let root=cache.pixels.root().unwrap().view.clone();let mut results=Vec::new();
    for (deferred,direct) in [(false,false),(true,false),(false,true),(true,true)] {
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());let mut unit=Unit::default();{let _pass=encoder.color_pass("ordered root fallback",&root,wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));}
        let frame=packet(doc.scene(),doc.composition().size);
        let mut accepted=crate::scene::sources::AcceptedInputs::default();
        let mut e=Evaluator {class:crate::residency::Class::Visible,accepted:&mut accepted,cache:&mut cache,scene:&mut scene,packet:frame,r:&mut r,encoder:&mut encoder,region:PixelRect::new(0,0,12,12).into(),input:plan,tiled:false,unit:&mut unit,previous_motion:None,retained:0,approximate:false,preview:false};
        let output=Target {view:root.clone(),slot:Some(Slot::Root),plan};
        let value=|image:&Image|Value::Image {view:image.view.clone(),slot:None,opacity:1.,plan:image.plan,preview:None,encode:false};
        e.draw(value(&images[0]),value(&images[1]),layer_core::LayerBlend::Normal,0,Some(output.clone())).unwrap();
        assert_eq!(e.encoder.pass_count(),1);assert_eq!(e.scene.jobs.len(),1);
        if !deferred {e.scene.encode_jobs(e.r,e.encoder).unwrap();}
        let mut placed=Placed {slot:None,view:images[0].view.clone(),transform:Default::default(),shift:[0;2],plan,outside:0.,opacity:1.,backdrop:[0.;4],encode:false};
        if direct {placed.backdrop=[0.11,0.22,0.33,1.];e.materialize(Value::Placed(placed),Some(output)).unwrap();}
        else {e.draw(Value::Placed(placed),Value::Color([0.11,0.22,0.33,1.]),layer_core::LayerBlend::Normal,0,Some(output)).unwrap();}
        assert!(!e.scene.jobs.is_empty());
        scene.encode_jobs(&mut r,&mut encoder).unwrap();r.uploads.finish(&encoder);encoder.submit(&r.queue);
        results.push(pixels(&r,cache.texture()));
    }
    for result in &results[1..] {assert_eq!(results[0],*result);}assert!(results[0][0][3]>0.);
}

#[test]
fn composition_jobs_preserve_overlapping_writes_without_a_record_batch_cap() {
    let mut doc=document_at([65,33]);let back=paint_occurrence(&mut doc,"photo",Some(rgba8_source([65,33],|_,_|[79,101,151,193])));insert_occurrence(&mut doc,back,1);
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();let (mut scene,mut cache,images)=inputs(&mut r,&doc,doc.composition().blend);
    let plan=cache.output_plan();cache.pixels=Default::default();cache.pixels.ensure_root(&r,plan,wgpu::TextureFormat::Rgba32Float,"composition window",r.scene_tiers.windows.charge(0));cache.pixels.ensure(&r,plan,wgpu::TextureFormat::Rgba32Float,r.scene_tiers.windows.charge(0));
    let root=cache.pixels.root().unwrap().view.clone();let mut results=Vec::new();
    for deferred in [false,true] {
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());let mut unit=Unit::default();{let _pass=encoder.color_pass("bounded root",&root,wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));}
        let frame=packet(doc.scene(),doc.composition().size);
        for i in 0..34 {
            let coordinate=if i==33 {[32%plan.size[0],32/plan.size[0]]}else{[i%plan.size[0],i/plan.size[0]]};let side=1<<plan.level;
            let region=PixelRect::new(coordinate[0]*side,coordinate[1]*side,(coordinate[0]+1)*side,(coordinate[1]+1)*side).intersect(plan.bounds);
            let mut accepted=crate::scene::sources::AcceptedInputs::default();
        let mut e=Evaluator {class:crate::residency::Class::Visible,accepted:&mut accepted,cache:&mut cache,scene:&mut scene,packet:frame,r:&mut r,encoder:&mut encoder,region:region.into(),input:plan,tiled:false,unit:&mut unit,previous_motion:None,retained:0,approximate:false,preview:false};
            let value=|image:&Image|Value::Image {view:image.view.clone(),slot:None,opacity:1.,plan:image.plan,preview:None,encode:false};
            let order=if i==33 {[1,0]}else{[0,1]};
            e.draw(value(&images[order[0]]),value(&images[order[1]]),layer_core::LayerBlend::Normal,0,Some(Target {view:root.clone(),slot:Some(Slot::Root),plan})).unwrap();
            if deferred {assert_eq!(e.scene.jobs.len(),i as usize+1);assert_eq!(e.encoder.pass_count(),1);}
            else {e.scene.encode_jobs(e.r,e.encoder).unwrap();}
        }
        scene.encode_jobs(&mut r,&mut encoder).unwrap();
        assert_eq!(encoder.pass_count(),if deferred {2}else{35});r.uploads.finish(&encoder);encoder.submit(&r.queue);results.push(pixels(&r,cache.texture()));
    }
    for (actual,expected) in results[0].iter().zip(&results[1]) {assert_eq!(actual.map(f32::to_bits),expected.map(f32::to_bits));}
}


#[test]
fn ready_motion_units_keep_the_current_image_out_of_the_spare_slot() {
    let doc=document_at([PAGE_SIZE;2]);
    let r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut vectors=crate::scene::vectors::Vectors::default();
    let plan=display_mips::Plan::at(doc.composition().size,0);
    let first=vectors.motion_image(&r,plan);let second=vectors.motion_image(&r,plan);
    let key=(doc.scene().order()[0],crate::scene::vectors::Moving(Arc::new(BTreeSet::new())),0,[0;2],crate::scene::vectors::Replaced(None));
    vectors.replace_motion(crate::scene::vectors::MotionImage {key:key.clone(),window:plan.doc_bounds,image:first,revision:0});
    vectors.replace_motion(crate::scene::vectors::MotionImage {key,window:plan.doc_bounds,image:second,revision:0});
    let next=vectors.motion_image(&r,plan);
    assert_ne!(next.texture,vectors.motion.as_ref().unwrap().image.texture,
        "the next unit's writable spare must not alias the committed motion image");
}

#[test]
fn a_terminal_capture_error_discards_its_unsubmitted_prefix() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut scene=Scene::new(&r).unwrap();scene.ordered=true;
    let slot=scene.reserve(&r).unwrap();let _held=scene.own(slot);
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    let declaration=crate::scene::sources::Inputs::default().for_unit(crate::scene::sources::InputUnit::tile([0;2])).unwrap();let mut accepted=None;
    let result=pollster::block_on(scene.complete_capture_plan(&mut r,&mut encoder,None,&declaration,&mut accepted,|scene,_,_,_| {
        scene.jobs.push(Job::Clear(scene.pool[slot].view.clone(),wgpu::Color::TRANSPARENT));
        scene.source_jobs.push(0..1);
        Err(GpuRasterError::Invariant("capture rejected its plan"))
    }));
    assert!(matches!(result,Err(GpuRasterError::Invariant("capture rejected its plan"))));
    assert!(scene.jobs.is_empty(),"a failed capture retained jobs after releasing their accepted owner");
    assert!(scene.source_jobs.is_empty());
    assert!(!encoder.has_work());
}

#[test]
fn a_capture_admission_refusal_waits_for_the_working_reservation_release() {
    let extent=[16;2];let mut doc=document_at(extent);
    paint_mut(&mut doc,0).base=Some(layer_core::authored::PaintBase::new(rgba8_source(extent,|_,_|[64,128,192,255]).into()));
    let mut r=crate::memory::memory_tests::renderer(doc.composition().color,layer_core::DeviceMemoryFacts {unified:true,budget_bytes:Some(64<<30),..Default::default()}).unwrap();
    r.ensure_document_metadata(extent,doc.scene()).unwrap();
    let frame=packet(doc.scene(),extent);
    crate::memory::memory_tests::legacy_scene_frame(&mut r,frame);
    let mut scene=Scene::new(&r).unwrap();scene.ordered=true;
    let budget=r.factory.memory_budget().clone();
    r.memory.sync_report(layer_core::MemoryOwnerKind::Scene,r.display_memory_report(Some(&scene),r.composition_memory(Some(&scene))));budget.flush_reports();
    let device=budget.snapshot().devices[0].device;
    let owner=budget.register_owner(layer_core::MemoryOwnerKind::Other,layer_core::MemoryClass::Visible,Some(device));
    let pressure=owner.admit_working(layer_core::MemoryGrant {ram_bytes:0,gpu_bytes:budget.import_headroom_bytes()}).unwrap();
    let window=Image::new(&r,display_mips::Plan::at(extent,0),"declared capture window");
    let requested=scene.declare_capture(&mut r,frame,&window,&[PixelRect::full(extent)],scene::Output::Artwork(None),None,None).unwrap();
    assert!(matches!(requested.accept_query(&mut r,Some(&scene)),Err(GpuRasterError::NotReady)),"the working reservation must refuse the cold capture's actual input admission");
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    let released=Arc::new(std::sync::atomic::AtomicBool::new(false));let release=released.clone();
    encoder.on_submitted_work_done(move|| {drop(pressure);release.store(true,std::sync::atomic::Ordering::Release);});
    let output=Image::new(&r,display_mips::Plan::at(extent,0),"capture after working release");
    let cancel=Arc::new(std::sync::atomic::AtomicBool::new(false));r.snapshot_cancelled=Some(cancel.clone());
    let (done,deadline)=std::sync::mpsc::channel();
    let timer=std::thread::spawn(move|| {if deadline.recv_timeout(READBACK_TIMEOUT).is_err() {cancel.store(true,std::sync::atomic::Ordering::Release);}});
    let result=pollster::block_on(scene.capture_query_tiles(&mut r,frame,PixelRect::full(extent),scene::Output::Artwork(None),false,1,None,&mut encoder,|_,view,_,encoder| {
        encoder.copy_texture_to_texture(view.texture().as_image_copy(),output.texture.as_image_copy(),output.texture.size());Ok(())
    }));
    let _=done.send(());timer.join().unwrap();
    assert!(result.is_ok(),"a transient working refusal escaped the capture: {result:?}");
    assert!(released.load(std::sync::atomic::Ordering::Acquire),"capture must await the reservation's release edge");
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    let expected=[64,128,192].map(|v|layer_core::color::RgbSpace::Srgb.decode(f64::from(v)/255.) as f32);
    for pixel in pixels(&r,&output.texture) {assert_eq!(pixel[3],1.);for c in 0..3 {assert!((pixel[c]-expected[c]).abs()<2e-6,"{pixel:?}");}}
}
