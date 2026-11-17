use super::*;

fn stroke_batch(target:SourceTarget,dab:&layer_render::Dab,start:bool,end:bool)->layer_render::DabBatch {
    layer_render::DabBatch {stroke_start:start,stroke_end:end,..dab_batch(target,crate::layer_tests::preset_style(DefaultBrushPreset::GPen),dab.bounds())}
}
fn stroke_dab(x:f32,y:f32)->layer_render::Dab {let mut dab=crate::tests::test_dab([x,y],[0.9,0.1,0.3,1.],0.8);dab.radii=[40.;2];dab}
fn max_error(actual:&[[f32;4]],expected:&[[f32;4]])->f32 {
    actual.iter().zip(expected).flat_map(|(a,b)|a.iter().zip(b).map(|(a,b)|(a-b).abs())).fold(0f32,f32::max)
}

fn view_at(extent:[u32;2],divisor:u32)->layer_render::ViewState {
    let mut view=crate::test_support::view([extent[0]/divisor,extent[1]/divisor]);
    view.document_to_surface=[1./divisor as f32,0.,0.,1./divisor as f32,0.,0.];view
}
fn renderer(doc:&layer_core::Document)->WgpuRasterizer {
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    layer_render::CanvasRenderer::set_working_layer(&mut r,doc.working.occurrence);r
}

#[expect(clippy::too_many_arguments,reason="Each contact case names its document, view, brush, dabs and renderer")]
fn each_contact_frame_matches_a_fresh_renderer(doc:&layer_core::Document,view:layer_render::ViewState,level:u32,preset:DefaultBrushPreset,dabs:&[layer_render::Dab],
    renderer:&dyn Fn(&layer_core::Document)->WgpuRasterizer,contact:std::ops::Range<usize>,mut before:impl FnMut(&mut WgpuRasterizer,usize),
) {
    let extent=doc.composition().size;let target=doc.active_target().unwrap();let style=crate::layer_tests::preset_style(preset);
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view;
    let mut r=renderer(doc);layer_render::CanvasRenderer::set_working_layer(&mut r,doc.working.occurrence);
    crate::test_support::submit_settled(&mut r,frame);
    assert_eq!(r.scale_display.as_ref().unwrap().plan.level,level);
    let batch=|dabs:&[layer_render::Dab],start:bool|layer_render::DabBatch {stroke_start:start,stroke_end:false,dab_count:dabs.len() as u32,
        damage:dabs.iter().map(|dab|dab.bounds()).reduce(|a,b|a.union(b)).unwrap(),..crate::test_support::dab_batch(target,style.clone(),dabs[0].bounds())};
    let mut held=None;
    for step in 0..dabs.len() {
        before(&mut r,step);
        let time=0.01*(step+1) as f32;
        let next=batch(&dabs[step..=step],step==0);
        let counts=crate::test_support::PassCounts::snapshot();
        crate::test_support::submit_ready(&mut r,FramePacket {dabs:&dabs[step..=step],dab_batches:std::slice::from_ref(&next),time_seconds:time,..frame}).unwrap();
        let counts=crate::test_support::PassCounts::snapshot().delta(counts);
        let id=r.scale_display.as_ref().unwrap().contact_id();
        if contact.contains(&step) {assert!(counts.unit_acceptances==0 && id.is_some() && id==held,"frame {step} composes in the contact begun before it: {counts:?}");}
        held=id;
        let mut fresh=renderer(doc);layer_render::CanvasRenderer::set_working_layer(&mut fresh,doc.working.occurrence);
        crate::test_support::submit_settled(&mut fresh,frame);
        let whole=batch(&dabs[..=step],true);
        crate::test_support::submit_ready(&mut fresh,FramePacket {dabs:&dabs[..=step],dab_batches:std::slice::from_ref(&whole),time_seconds:time,..frame}).unwrap();
        let error=max_error(&display_pixels(&r),&display_pixels(&fresh));
        assert!(error<1e-4,"contact frame {step} differs from a fresh renderer by {error}");
        assert!(r.has_presentable_frame(),"contact frame {step} presents");
    }
}
fn dabs(xs:&[f32],y:f32)->Vec<layer_render::Dab> {xs.iter().map(|x|stroke_dab(*x,y)).collect()}

#[test]
fn a_stroke_entering_a_new_level_page_mid_contact_matches_a_fresh_renderer() {
    let doc=document_at([2048,1024]);
    each_contact_frame_matches_a_fresh_renderer(&doc,view_at([2048,1024],4),2,DefaultBrushPreset::GPen,&dabs(&[880.,940.,1000.,1060.,1120.],500.),&renderer,1..5,|_,_|{});
}

#[test]
fn a_cell_revisited_later_in_the_same_contact_recomposes_and_matches_a_fresh_renderer() {
    let doc=document_at([2048,1024]);
    each_contact_frame_matches_a_fresh_renderer(&doc,view_at([2048,1024],1),0,DefaultBrushPreset::GPen,&dabs(&[1500.,300.,700.,320.,1520.],300.),&renderer,1..5,|_,_|{});
}

#[test]
fn a_stroke_on_an_offset_layer_crossing_a_level_page_edge_stays_in_contact_and_matches_a_fresh_renderer() {
    let mut doc=document_at([2048,1024]);
    let working=doc.working.occurrence.unwrap();doc.artwork.occurrences.get_mut(working).unwrap().offset=[3,5];
    each_contact_frame_matches_a_fresh_renderer(&doc,view_at([2048,1024],4),2,DefaultBrushPreset::GPen,&dabs(&[900.,960.,1000.,1040.],500.),&renderer,1..4,|_,_|{});
}

#[test]
fn a_stroke_on_a_layer_offset_off_the_page_grid_reduces_only_tiles_its_contact_declared() {
    let extent=[2048,2048];
    let mut doc=document_at(extent);
    let working=doc.working.occurrence.unwrap();doc.artwork.occurrences.get_mut(working).unwrap().offset=[-2,-1];
    let target=doc.active_target().unwrap();
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,2);
    let [mut r,mut twin]=[renderer(&doc),renderer(&doc)];
    for r in [&mut r,&mut twin] {crate::test_support::submit_settled(r,frame);}
    let xs=[1500.,1510.,1520.,1530.,1540.,1550.,1560.,1570.,1580.];
    let mut contact=false;
    for (step,x) in xs.iter().enumerate() {
        let dab=stroke_dab(*x,1200.);let batch=stroke_batch(target,&dab,step==0,step+1==xs.len());
        let next=FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),time_seconds:0.01*(step+1) as f32,..frame};
        crate::test_support::submit_ready(&mut twin,FramePacket {composite_all:true,..next}).unwrap();
        crate::test_support::submit_ready(&mut r,next).expect("a contact frame reads only tiles its contact declared");
        contact|=r.scale_display.as_ref().unwrap().contact_id().is_some();
    }
    assert!(contact,"the stroke enters contact");
    for r in [&mut r,&mut twin] {crate::test_support::submit_settled(r,frame);}
    let error=max_error(&display_pixels(&r),&display_pixels(&twin));
    assert!(error<1e-4,"the offset stroke keeps every dab: {error}");
}

#[test]
fn watercolor_carried_across_a_level_page_edge_during_contact_matches_a_recomposing_twin() {
    let extent=[2048,1024];let doc=document_at(extent);let target=doc.active_target().unwrap();
    let style=crate::layer_tests::preset_style(DefaultBrushPreset::WatercolorWash);
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,4);
    let [mut r,mut twin]=[renderer(&doc),renderer(&doc)];
    crate::test_support::submit_settled(&mut r,frame);crate::test_support::submit_settled(&mut twin,frame);
    assert_eq!(r.scale_display.as_ref().unwrap().plan.level,2);
    for (step,dab) in dabs(&[940.,980.,1010.,1040.],500.).iter().enumerate() {
        let batch=layer_render::DabBatch {stroke_start:step==0,stroke_end:false,..crate::test_support::dab_batch(target,style.clone(),dab.bounds())};
        let next=FramePacket {dabs:std::slice::from_ref(dab),dab_batches:std::slice::from_ref(&batch),time_seconds:0.01*(step+1) as f32,..frame};
        crate::test_support::submit_ready(&mut r,next).unwrap();
        crate::test_support::submit_ready(&mut twin,FramePacket {composite_all:true,..next}).unwrap();
        let error=max_error(&display_pixels(&r),&display_pixels(&twin));
        assert!(error<1e-4,"watercolor contact frame {step} differs from a twin that recomposes every frame by {error}");
    }
}

#[test]
fn store_pressure_mid_stroke_releases_the_contact_and_every_frame_matches_a_fresh_renderer() {
    let doc=document_at([1333,1000]);
    let mut view=crate::test_support::view([400,300]);view.document_to_surface=[0.29,0.,0.,0.29,0.,0.];
    let tight=|doc:&layer_core::Document|crate::memory::memory_tests::renderer(doc.composition().color,
        layer_core::DeviceMemoryFacts {budget_bytes:Some(256<<20),unified:true,..Default::default()}).unwrap();
    let mut large=dabs(&[400.,450.,500.,550.,600.,650.],500.);
    for dab in &mut large {dab.radii=[400.;2];}
    let (release,gate)=std::sync::mpsc::channel::<()>();let mut gate=Some(gate);let mut held=Vec::new();
    each_contact_frame_matches_a_fresh_renderer(&doc,view,1,DefaultBrushPreset::GPen,&large,&tight,1..3,|r,step| {
        let store=r.scene.as_ref().unwrap().scale_pages.clone();
        if step==3 {
            held=r.scale_display.as_ref().unwrap().gesture.as_ref().unwrap().pages.keys().cloned().collect();
            let (entered,ready)=std::sync::mpsc::channel();let gate=gate.take().unwrap();
            r.device.retirement_handle().run(move||{let _=entered.send(());let _=gate.recv_timeout(crate::READBACK_TIMEOUT);});
            ready.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
            let tier=store.lock().unwrap().tier();tier.set_limit(tier.used()/2,0);
        }
        if step==4 {
            let ended=r.scale_display.as_ref().unwrap().gesture.is_none();
            let pinned=held.iter().filter(|key|store.lock().unwrap().pinned(key)).count();
            release.send(()).unwrap();
            assert!(!held.is_empty(),"the contact holds its source pages");
            assert!(ended,"the frame that finds the Store over its limit ends the contact");
            assert_eq!(pinned,0,"that frame's completion releases every page the contact held, before the retirement actor runs");
        }
    });
}

#[test]
fn a_stroke_under_a_blur_and_a_layer_above_matches_a_fresh_renderer_in_every_contact_frame() {
    let extent=[1024,512];let mut doc=document_at(extent);let ink=doc.working.occurrence.unwrap();
    let above=paint_occurrence(&mut doc,"Above",Some(layer_core::color::source::rgba8_source(extent,|x,y|[90,(x/5) as u8,(y/3) as u8,110])));
    let blur=effect_occurrence(&mut doc,layer_core::EffectInstance::new(crate::tests::fixture("gaussian_blur").program()),"Blur");
    set_effect_value(&mut doc,blur,"sigma",layer_core::EffectValue::Number(12.));
    set_root_entries(&mut doc,vec![above,blur,ink]);
    each_contact_frame_matches_a_fresh_renderer(&doc,view_at(extent,1),0,DefaultBrushPreset::GPen,&dabs(&[200.,250.,500.,540.,580.],240.),&renderer,1..5,|_,_|{});
}

fn contact_frame(r:&mut WgpuRasterizer,frame:FramePacket<'_>,target:SourceTarget,x:f32,start:bool)->Option<Arc<crate::scene::SignatureFrame>> {
    let dab=stroke_dab(x,200.);let batch=stroke_batch(target,&dab,start,false);
    crate::test_support::submit_ready(r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
    r.scene.as_ref().unwrap().scale_pages.lock().unwrap().expected().cloned()
}

#[test]
fn a_renderer_write_to_another_layer_mid_contact_ends_the_frozen_frame_and_shows_the_write() {
    let extent=[1024,512];let mut doc=document_at(extent);let ink=doc.working.occurrence.unwrap();let target=primary_target(&doc);
    let other=paint_occurrence(&mut doc,"Other",None);set_root_entries(&mut doc,vec![other,ink]);
    let other_target=doc.scene().source_target(other).unwrap();
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,1);
    let mut r=renderer(&doc);
    crate::test_support::submit_settled(&mut r,frame);
    let mut dab=stroke_dab(900.,400.);dab.radii=[60.;2];
    crate::test_support::submit_ready(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&stroke_batch(other_target,&dab,true,true)),..frame}).unwrap();
    crate::test_support::submit_settled(&mut r,frame);
    contact_frame(&mut r,frame,target,200.,true);
    let held=contact_frame(&mut r,frame,target,240.,false);
    assert!(Arc::ptr_eq(held.as_ref().unwrap(),contact_frame(&mut r,frame,target,280.,false).as_ref().unwrap()),"the contact holds its frame");
    let at=(300*extent[0]+800) as usize;let before=display_pixels(&r)[at];
    let bytes=(0..PAGE_SIZE*PAGE_SIZE).flat_map(|_|[0.1f32,0.8,0.2,1.]).flat_map(f32::to_le_bytes).collect::<Vec<_>>();
    crate::test_support::upload_layer_page(&mut r,doc.scene(),other_target,[3,1],&bytes);
    let after=contact_frame(&mut r,frame,target,320.,false);
    assert!(!Arc::ptr_eq(held.as_ref().unwrap(),after.as_ref().unwrap()),"a write to another layer ends the frozen frame");
    let pixel=display_pixels(&r)[at];
    assert!(pixel.iter().zip(before).any(|(after,before)|(after-before).abs()>0.05),"the view shows the other layer's write: {before:?} -> {pixel:?}");
}

#[test]
fn inspecting_a_mask_mid_contact_ends_the_frozen_frame() {
    let extent=[1024,512];let mut doc=document_at(extent);let ink=doc.working.occurrence.unwrap();let target=primary_target(&doc);
    coverage_mask(&mut doc,ink,[0,0],None);
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,1);
    let mut r=renderer(&doc);
    crate::test_support::submit_settled(&mut r,frame);
    contact_frame(&mut r,frame,target,200.,true);
    let held=contact_frame(&mut r,frame,target,240.,false);
    assert!(Arc::ptr_eq(held.as_ref().unwrap(),contact_frame(&mut r,frame,target,280.,false).as_ref().unwrap()),"the contact holds its frame");
    let inspected=contact_frame(&mut r,FramePacket {inspect_mask:Some(ink),..frame},target,320.,false);
    assert!(!Arc::ptr_eq(held.as_ref().unwrap(),inspected.as_ref().unwrap()),"inspecting the mask ends the frozen frame");
}

fn held_frame(r:&WgpuRasterizer)->Arc<crate::scene::SignatureFrame> {r.scene.as_ref().unwrap().scale_pages.lock().unwrap().expected().cloned().unwrap()}
fn stroke_frames(r:&mut WgpuRasterizer,frame:FramePacket<'_>,target:SourceTarget,dabs:&[layer_render::Dab],steps:std::ops::Range<usize>)->Vec<Arc<crate::scene::SignatureFrame>> {
    let style=crate::layer_tests::preset_style(DefaultBrushPreset::GPen);
    steps.map(|step| {
        let batch=layer_render::DabBatch {stroke_start:step==0,stroke_end:false,..crate::test_support::dab_batch(target,style.clone(),dabs[step].bounds())};
        crate::test_support::submit_ready(r,FramePacket {dabs:&dabs[step..=step],dab_batches:std::slice::from_ref(&batch),time_seconds:0.01*(step+1) as f32,..frame}).unwrap();
        held_frame(r)
    }).collect()
}
fn fresh_stroke(doc:&layer_core::Document,frame:FramePacket<'_>,target:SourceTarget,dabs:&[layer_render::Dab])->Vec<[f32;4]> {
    let mut fresh=renderer(doc);crate::test_support::submit_settled(&mut fresh,frame);
    let batch=layer_render::DabBatch {stroke_start:true,stroke_end:false,dab_count:dabs.len() as u32,damage:dabs.iter().map(|dab|dab.bounds()).reduce(|a,b|a.union(b)).unwrap(),
        ..crate::test_support::dab_batch(target,crate::layer_tests::preset_style(DefaultBrushPreset::GPen),dabs[0].bounds())};
    crate::test_support::submit_ready(&mut fresh,FramePacket {dabs,dab_batches:std::slice::from_ref(&batch),time_seconds:0.01*dabs.len() as f32,..frame}).unwrap();
    display_pixels(&fresh)
}

#[test]
fn a_mask_stroke_names_every_frame_and_each_frame_and_its_undo_match_a_fresh_renderer() {
    let extent=[1024,512];let mut doc=document_at(extent);let ink=doc.working.occurrence.unwrap();
    let mask=coverage_mask(&mut doc,ink,[0,0],None);doc.artwork.coverage.get_mut(mask).unwrap().default_coverage=0.;
    let target=SourceTarget::Coverage(mask);doc.working.target=Some(target);
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,1);
    let mut r=renderer(&doc);crate::test_support::submit_settled(&mut r,frame);
    let mut dabs=dabs(&[200.,250.,300.,350.,400.],240.);for dab in &mut dabs {dab.color_rgba_linear=[1.;4];}
    for step in 0..dabs.len() {
        let before=held_frame(&r);let named=stroke_frames(&mut r,frame,target,&dabs,step..step+1);
        assert!(!Arc::ptr_eq(&before,&named[0]),"mask frame {step} names its frame");
        let error=max_error(&display_pixels(&r),&fresh_stroke(&doc,frame,target,&dabs[..=step]));
        assert!(error<1e-4,"mask frame {step} differs from a fresh renderer by {error}");
    }
    let original=doc.artwork.coverage.get(mask).unwrap().raster.clone();
    crate::test_support::submit_settled(&mut r,FramePacket {restore_rasters:&[(target,original)],..frame});
    crate::test_support::submit_settled(&mut r,frame);
    let mut fresh=renderer(&doc);crate::test_support::submit_settled(&mut fresh,frame);
    let error=max_error(&display_pixels(&r),&display_pixels(&fresh));
    assert!(error<1e-4,"the undone mask stroke differs from a fresh renderer by {error}");
}

#[test]
fn a_retracted_prediction_recomposes_its_tiles_like_a_fresh_renderer() {
    let extent=[1024,512];let doc=document_at(extent);let target=primary_target(&doc);
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,1);
    let mut r=renderer(&doc);crate::test_support::submit_settled(&mut r,frame);
    let kept=dabs(&[200.,240.,280.],200.);let predicted=stroke_dab(900.,400.);
    let send=|r:&mut WgpuRasterizer,dab:&layer_render::Dab,kind:layer_render::DabBatchKind,start:bool,time:f32| {
        let batch=layer_render::DabBatch {kind,..stroke_batch(target,dab,start,false)};
        crate::test_support::submit_ready(r,FramePacket {dabs:std::slice::from_ref(dab),dab_batches:std::slice::from_ref(&batch),time_seconds:time,..frame}).unwrap();
        held_frame(r)
    };
    send(&mut r,&kept[0],layer_render::DabBatchKind::Persistent,true,0.01);
    let held=send(&mut r,&kept[1],layer_render::DabBatchKind::Persistent,false,0.02);
    assert!(Arc::ptr_eq(&held,&send(&mut r,&predicted,layer_render::DabBatchKind::Preview,false,0.03)),"the contact holds its frame through a prediction");
    send(&mut r,&kept[2],layer_render::DabBatchKind::Persistent,false,0.04);
    let error=max_error(&display_pixels(&r),&fresh_stroke(&doc,frame,target,&kept));
    assert!(error<1e-4,"the frame that retracts the prediction differs from a fresh renderer by {error}");
}

#[test]
fn a_zoom_during_a_held_contact_names_its_own_frame_and_settles_like_a_fresh_renderer() {
    let extent=[2048,1024];let doc=document_at(extent);let target=primary_target(&doc);
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,1);
    let zoomed=FramePacket {view:view_at(extent,2),..frame};
    let mut r=renderer(&doc);crate::test_support::submit_settled(&mut r,frame);
    let dabs=dabs(&[300.,340.,380.,420.],300.);
    let held=stroke_frames(&mut r,frame,target,&dabs,0..3);
    assert!(Arc::ptr_eq(&held[1],&held[2]),"the contact holds its frame");
    let zoom=stroke_frames(&mut r,zoomed,target,&dabs,3..4);
    assert!(!Arc::ptr_eq(&held[2],&zoom[0]),"the zoom frame names its own frame");
    crate::test_support::submit_settled(&mut r,zoomed);
    let error=max_error(&display_pixels(&r),&fresh_stroke(&doc,zoomed,target,&dabs));
    assert!(error<1e-4,"the zoomed stroke differs from a fresh renderer by {error}");
}

#[test]
fn a_discarded_contact_frame_is_reduced_and_composed_again_by_the_next_frame() {
    let extent=[2048,1024];let doc=document_at(extent);let target=primary_target(&doc);
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,4);
    let mut r=renderer(&doc);
    crate::test_support::submit_settled(&mut r,frame);
    let kept=dabs(&[300.,360.],300.);let predicted=stroke_dab(420.,300.);
    let send=|r:&mut WgpuRasterizer,dab:Option<&layer_render::Dab>,kind:layer_render::DabBatchKind,start:bool,time:f32| {
        let batch=dab.map(|dab|layer_render::DabBatch {kind,..stroke_batch(target,dab,start,false)});
        let counts=crate::test_support::PassCounts::snapshot();
        crate::test_support::submit_ready(r,FramePacket {dabs:dab.map_or(&[][..],std::slice::from_ref),dab_batches:batch.as_ref().map_or(&[][..],std::slice::from_ref),time_seconds:time,..frame}).unwrap();
        crate::test_support::PassCounts::snapshot().delta(counts)
    };
    send(&mut r,Some(&kept[0]),layer_render::DabBatchKind::Persistent,true,0.01);
    send(&mut r,Some(&kept[1]),layer_render::DabBatchKind::Persistent,false,0.02);
    send(&mut r,Some(&predicted),layer_render::DabBatchKind::Preview,false,0.03);
    r.test.discard_frame=true;
    send(&mut r,None,layer_render::DabBatchKind::Persistent,false,0.04);
    assert!(!r.test.discard_frame,"the pause frame's encoder is dropped");
    let next=send(&mut r,None,layer_render::DabBatchKind::Persistent,false,0.05);
    assert!(next.unit_acceptances>0,"the frame after a discarded contact frame reduces the stroked layer's source pages again");
    assert!(!r.metrics.frame_composited_pages.is_empty(),"the frame after a discarded contact frame composes the dropped cells again");
}

#[test]
fn a_frame_whose_commit_is_discarded_keeps_presenting_the_last_committed_root() {
    let extent=[2048,1024];let doc=document_at(extent);let target=primary_target(&doc);
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,1);
    let mut r=renderer(&doc);
    crate::test_support::submit_settled(&mut r,frame);crate::test_support::complete(&r);
    let coverage=|r:&WgpuRasterizer|r.scale_display.as_ref().unwrap().storm_visible_coverage(frame.view);
    let [settled,_,_,known]=coverage(&r);
    assert!(settled>0. && known==1.,"the settled view presents a committed root");
    for (step,x) in [300.,1500.].into_iter().enumerate() {
        let dab=stroke_dab(x,300.);let batch=stroke_batch(target,&dab,step==0,false);
        r.test.discard_frame=true;
        crate::test_support::submit_ready(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),time_seconds:0.01*(step+1) as f32,..frame}).unwrap();
        assert!(!r.test.discard_frame,"the stroke frame's encoder is dropped");
        let [covered,_,_,known]=coverage(&r);
        assert_eq!(known,1.,"frame {step}: the presented root has a matching publication witness");
        assert!(covered>=settled,"frame {step}: a discarded commit lost presented coverage: settled {settled}, covered {covered}");
        assert!(r.scale_display.as_ref().unwrap().pixels.committed_root().is_some(),"frame {step}: the display keeps its last committed root");
    }
    crate::test_support::submit_ready(&mut r,frame).unwrap();crate::test_support::complete(&r);
    let [covered,_,_,known]=coverage(&r);
    assert!(known==1. && covered>=settled,"the frame after the discarded commits keeps every written cell: settled {settled}, covered {covered}");
}

#[test]
fn after_pen_up_every_written_back_page_equals_a_keyed_composition_of_the_same_state() {
    let extent=[2048,1024];let doc=document_at(extent);let target=primary_target(&doc);
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
    frame.view.width_px=extent[0];frame.view.height_px=extent[1];frame.view.document_to_surface=[1.,0.,0.,1.,0.,0.];
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    crate::test_support::submit_settled(&mut r,frame);
    let xs=[300.,360.,420.,480.];let mut touched=PixelRect::EMPTY;
    for (step,x) in xs.into_iter().enumerate() {
        let dab=stroke_dab(x,300.);let batch=stroke_batch(target,&dab,step==0,step+1==xs.len());
        touched=touched.union(crate::pixel_rect::pixel_rect(dab.bounds(),extent));
        crate::test_support::submit_ready(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),time_seconds:0.01*(step+1) as f32,..frame}).unwrap();
    }
    crate::test_support::submit_ready(&mut r,frame).unwrap();
    crate::test_support::complete(&r);
    let cache=r.scale_display.as_ref().unwrap();let plan=cache.plan;let root=cache.graph.root.clone().unwrap();
    let retained=cache.pages.lock().unwrap().retained_pages();
    let mut written=Vec::new();
    for coordinate in pages::coordinates(touched.intersect(plan.bounds),plan.level) {
        let region=pages::bounds(coordinate,plan.level,plan.extent).intersect(plan.bounds);
        let key=cache.page_key(root.clone(),region);
        let page=retained.iter().find(|page|page.key==key && !page.written.is_empty());
        assert!(page.is_some(),"the composed cell over {region:?} is written back to the Store at pen-up");
        let page_plan=key.plan();
        written.push((region,page_plan,pixels(&r,&page.unwrap().texture)));
    }
    let cache=r.scale_display.as_ref().unwrap();
    cache.evict_window(plan.bounds);evict_composite_pages(cache,&[plan.bounds]);
    crate::test_support::submit_published(&mut r,frame);
    let window=display_pixels(&r);let plan=r.scale_display.as_ref().unwrap().plan;
    for (region,page_plan,page) in written {
        let [x,y,width,height]=paint_transform::texel_rect(region.window_local(plan.bounds),1<<plan.level);
        let [px,py,_,_]=paint_transform::texel_rect(region.window_local(page_plan.bounds),1<<plan.level);
        for row in 0..height {
            let from=((py+row)*page_plan.size[0]+px) as usize;let to=((y+row)*plan.size[0]+x) as usize;
            let error=max_error(&page[from..from+width as usize],&window[to..to+width as usize]);
            assert!(error<1e-5,"the written-back page over {region:?} differs from a keyed composition by {error}");
        }
    }
}

#[test]
fn pen_up_writes_back_the_cells_shown_earlier_in_the_contact_without_composing_them_again() {
    let extent=[2048,1024];let doc=document_at(extent);let target=primary_target(&doc);
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,1);
    let mut r=renderer(&doc);crate::test_support::submit_settled(&mut r,frame);
    let xs=(0..40).map(|step|300.+30.*step as f32).collect::<Vec<_>>();
    let cells=|x:f32|pages::coordinates(crate::pixel_rect::pixel_rect(stroke_dab(x,300.).bounds(),extent),0).collect::<BTreeSet<_>>();
    let mut counts=Default::default();
    for (step,x) in xs.iter().copied().enumerate() {
        let dab=stroke_dab(x,300.);let batch=stroke_batch(target,&dab,step==0,step+1==xs.len());
        let before=crate::test_support::PassCounts::snapshot();
        crate::test_support::submit_ready(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),time_seconds:0.01*(step+1) as f32,..frame}).unwrap();
        counts=crate::test_support::PassCounts::snapshot().delta(before);
    }
    let composed=r.metrics.frame_composited_pages.iter().map(|(_,coordinate)|*coordinate).collect::<BTreeSet<_>>();
    let last=cells(xs[39]);let earlier=xs.iter().flat_map(|x|cells(*x)).filter(|cell|!last.contains(cell)).collect::<BTreeSet<_>>();
    assert!(composed.is_subset(&last),"pen-up composes only the cells its own dab reaches: {composed:?}");
    let cache=r.scale_display.as_ref().unwrap();let plan=cache.plan;let root=cache.graph.root.clone().unwrap();
    let retained=cache.pages.lock().unwrap().retained_pages();
    for coordinate in &earlier {
        let key=cache.page_key(root.clone(),pages::bounds(*coordinate,plan.level,plan.extent).intersect(plan.bounds));
        assert!(retained.iter().any(|page|page.key==key && !page.written.is_empty()),"pen-up writes back the cell {coordinate:?} shown earlier in the contact");
    }
    let view_cells=pages::coordinates(plan.bounds,plan.level).count() as u64;
    assert!(counts.composite_commits>=earlier.len() as u64,"pen-up copies each earlier cell into the Store: {counts:?}");
    assert!(counts.expected_keys<=6*view_cells,"pen-up keys stay bounded by the view, not the contact: {} keys for {view_cells} cells",counts.expected_keys);
}

fn material_dab(x:f32,y:f32,style:&layer_render::DabStyle)->layer_render::Dab {
    let mut dab=stroke_dab(x,y);
    if let Some(dry)=style.contact.and_then(|contact|contact.dry) {
        let relief=dry.pose(0.5).relief;
        dab.previous=[x-10.,y,1.,0.];dab.motion=[10.,0.];dab.contact=[0.7,relief[0],relief[1],relief[2]];dab.previous_contact=dab.contact;
    }
    dab
}
fn predicted_batches(target:SourceTarget,style:&layer_render::DabStyle,stroke:u64,dabs:&[layer_render::Dab;2],start:bool)->[layer_render::DabBatch;2] {
    let batch=|kind,index:usize|layer_render::DabBatch {kind,stroke_id:layer_core::StrokeId(stroke),first_dab:index as u32,stroke_start:start && index==0,stroke_end:false,
        ..crate::test_support::dab_batch(target,style.clone(),dabs[index].bounds())};
    [batch(layer_render::DabBatchKind::Persistent,0),batch(layer_render::DabBatchKind::Preview,1)]
}
fn predicted_frame(r:&mut WgpuRasterizer,frame:FramePacket<'_>,target:SourceTarget,style:&layer_render::DabStyle,stroke:u64,dabs:&[layer_render::Dab;2],start:bool,time:f32)
    ->crate::test_support::PassCounts {
    let batches=predicted_batches(target,style,stroke,dabs,start);
    let counts=crate::test_support::PassCounts::snapshot();
    crate::test_support::submit_ready(r,FramePacket {dabs,dab_batches:&batches,time_seconds:time,..frame}).unwrap();
    crate::test_support::PassCounts::snapshot().delta(counts)
}
fn in_contact(counts:&crate::test_support::PassCounts)->bool {counts.unit_acceptances==0}
fn predicted_stroke(preset:DefaultBrushPreset,xs:&[f32])->Vec<crate::test_support::PassCounts> {
    let extent=[2048,1024];let doc=document_at(extent);let target=primary_target(&doc);let style=crate::layer_tests::preset_style(preset);
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,4);
    let mut r=renderer(&doc);
    crate::test_support::submit_settled(&mut r,frame);
    xs.windows(2).enumerate().map(|(step,pair)|predicted_frame(&mut r,frame,target,&style,1,&[material_dab(pair[0],300.,&style),material_dab(pair[1],300.,&style)],step==0,0.01*(step+1) as f32)).collect()
}

#[test]
fn a_contact_frame_after_the_first_accepts_no_brush_inputs_and_computes_no_preview_versions_for_held_tiles() {
    let xs=[300.,304.,308.,312.,316.,320.,324.];
    let counts=predicted_stroke(DefaultBrushPreset::Paintbrush,&xs);
    let contact:Vec<_>=counts.iter().enumerate().filter(|(_,counts)|in_contact(counts)).map(|(step,_)|step).collect();
    assert!(contact.len()>=3,"the stroke stays in contact: {contact:?}");
    for step in contact.iter().skip(1) {
        let counts=&counts[*step];
        assert_eq!([counts.brush_acceptances,counts.preview_computations],[0;2],"contact frame {step} reuses the contact's brush inputs and preview stamps: {counts:?}");
    }
}

#[test]
fn a_stroke_started_before_its_halves_exist_enters_contact_within_its_half_units_and_keeps_its_ink() {
    let extent=[2048,1024];let mut doc=document_at(extent);
    let below=paint_occurrence(&mut doc,"Below",Some(rgba8_source(extent,|x,y|[(x/9) as u8,(y/5) as u8,140,255])));
    let working=doc.scene().position(doc.working.occurrence.unwrap()).unwrap();
    insert_occurrence(&mut doc,below,working+1);
    let target=doc.active_target().unwrap();
    let style=crate::layer_tests::preset_style(DefaultBrushPreset::Paintbrush);
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,4);
    let [mut r,mut twin]=[renderer(&doc),renderer(&doc)];
    for r in [&mut r,&mut twin] {crate::test_support::submit_settled(r,frame);}
    let cache=r.scale_display.as_ref().unwrap();
    let cell=pages::bounds([0,0],cache.plan.level,extent);
    assert!(cache.pivot_missing(cell),"the stroke starts before its cell has halves");
    let bound=cache.pivot_halves().len()*pages::coordinates(cell,0).count()+2;
    let mut entered=None;
    for step in 0..bound+4 {
        let x=300.+2.*step as f32;let dabs=[material_dab(x,300.,&style),material_dab(x+2.,300.,&style)];
        let time=0.01*(step+1) as f32;
        let counts=predicted_frame(&mut r,frame,target,&style,1,&dabs,step==0,time);
        let batches=predicted_batches(target,&style,1,&dabs,step==0);
        crate::test_support::submit_ready(&mut twin,FramePacket {dabs:&dabs,dab_batches:&batches,time_seconds:time,composite_all:true,..frame}).unwrap();
        if in_contact(&counts) {entered=Some(step);break;}
    }
    assert!(entered.is_some_and(|step|step<=bound),"under continuous predicted frames the stroke enters contact within {bound} frames: {entered:?}");
    let last=material_dab(400.,300.,&style);let end=stroke_batch(target,&last,false,true);
    for r in [&mut r,&mut twin] {
        crate::test_support::submit_ready(r,FramePacket {dabs:std::slice::from_ref(&last),dab_batches:std::slice::from_ref(&end),time_seconds:1.,..frame}).unwrap();
        crate::test_support::submit_settled(r,frame);
    }
    let error=max_error(&display_pixels(&r),&display_pixels(&twin));
    assert!(error<1e-4,"the frames before contact keep every dab's ink: {error}");
}

#[test]
fn a_stroke_after_a_settled_selection_enters_contact_on_its_first_frame() {
    let extent=[2048,1024];let mut doc=document_at(extent);
    let below=paint_occurrence(&mut doc,"Below",Some(rgba8_source(extent,|x,y|[(x/9) as u8,(y/5) as u8,140,255])));
    let working=doc.scene().position(doc.working.occurrence.unwrap()).unwrap();
    insert_occurrence(&mut doc,below,working+1);
    let style=crate::layer_tests::preset_style(DefaultBrushPreset::Paintbrush);
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,4);
    let mut r=renderer(&doc);
    crate::test_support::submit_settled(&mut r,frame);
    layer_render::CanvasRenderer::set_working_layer(&mut r,Some(below));
    crate::test_support::submit_settled(&mut r,frame);
    let target=doc.scene().source_target(below).unwrap();
    let dabs=|x:f32|[material_dab(x,300.,&style),material_dab(x+2.,300.,&style)];
    predicted_frame(&mut r,frame,target,&style,1,&dabs(300.),true,0.01);
    assert!(r.scale_display.as_ref().unwrap().gesture.is_some(),"a settled selection leaves its first stroke frame everything contact needs");
    let counts=predicted_frame(&mut r,frame,target,&style,1,&dabs(302.),false,0.02);
    assert!(in_contact(&counts),"the second frame accepts no units: acceptances {} declarations {}",counts.unit_acceptances,counts.declarations);
}

#[test]
fn a_steady_predicted_stroke_admits_raster_working_memory_only_while_its_preview_grows() {
    let xs=[300.,304.,308.,312.,316.,320.,324.];
    let counts=predicted_stroke(DefaultBrushPreset::Pencil,&xs);
    let contact:Vec<_>=counts.iter().enumerate().filter(|(_,counts)|in_contact(counts)).map(|(step,_)|step).collect();
    assert!(contact.len()>=3,"the stroke stays in contact: {contact:?}");
    for step in contact.iter().skip(1) {
        assert_eq!(counts[*step].raster_admissions,0,"contact frame {step} keeps the stroke's preview reservation: {:?}",counts[*step]);
    }
}

#[test]
fn a_contact_frame_after_the_first_releases_its_holds_with_one_callback_per_submission() {
    let xs=[300.,304.,308.,312.,316.,320.,324.];
    let counts=predicted_stroke(DefaultBrushPreset::Paintbrush,&xs);
    let contact:Vec<_>=counts.iter().enumerate().filter(|(_,counts)|in_contact(counts)).map(|(step,_)|step).collect();
    assert!(contact.len()>=3,"the stroke stays in contact: {contact:?}");
    for step in contact.iter().skip(1) {
        assert!(counts[*step].completion_callbacks<=4,"contact frame {step} adds no completion callback per held page or input: {:?}",counts[*step]);
    }
}

#[test]
fn a_contact_frame_after_the_first_names_its_preview_from_the_frame_without_hashing_dabs() {
    let extent=[2048,1024];let doc=document_at(extent);let target=primary_target(&doc);let style=crate::layer_tests::preset_style(DefaultBrushPreset::Paintbrush);
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,4);
    let mut r=renderer(&doc);
    crate::test_support::submit_settled(&mut r,frame);
    let xs=[300.,304.,308.,312.,316.,320.,324.];
    let frames:Vec<_>=xs.windows(2).enumerate().map(|(step,pair)| {
        let counts=predicted_frame(&mut r,frame,target,&style,1,&[material_dab(pair[0],300.,&style),material_dab(pair[1],300.,&style)],step==0,0.01*(step+1) as f32);
        (counts,r.preview_declaration.as_ref().and_then(|declaration|declaration.authored_version().ok()).map(|version|version.authored()))
    }).collect();
    let contact:Vec<_>=frames.iter().enumerate().filter(|(_,(counts,_))|in_contact(counts)).map(|(step,_)|step).collect();
    assert!(contact.len()>=3,"the stroke stays in contact: {contact:?}");
    for step in contact.iter().skip(1) {assert_eq!(frames[*step].0.preview_definitions,0,"contact frame {step} hashes no dabs: {:?}",frames[*step].0);}
    let names=contact.iter().map(|step|frames[*step].1.expect("every contact frame names its preview")).collect::<std::collections::BTreeSet<_>>();
    assert_eq!(names.len(),contact.len(),"no two contact frames name their preview alike");
}

#[test]
fn a_contact_frame_after_the_first_refreshes_the_navigator_from_the_view_window_at_most_every_fifty_milliseconds() {
    let extent=[2048,1024];let doc=document_at(extent);let target=primary_target(&doc);let style=crate::layer_tests::preset_style(DefaultBrushPreset::Paintbrush);
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,4);
    let mut r=renderer(&doc);
    crate::test_support::submit_settled(&mut r,frame);
    let xs=[300.,304.,308.,312.,316.,320.,324.,328.,332.,336.,340.,344.];
    let frames:Vec<_>=xs.windows(2).enumerate().map(|(step,pair)| {
        let revision=r.navigator.revision;let time=0.01*(step+1) as f32;
        let counts=predicted_frame(&mut r,frame,target,&style,1,&[material_dab(pair[0],300.,&style),material_dab(pair[1],300.,&style)],step==0,time);
        (counts,r.navigator.revision-revision,r.navigator.pending(),time)
    }).collect();
    let contact:Vec<_>=frames.iter().enumerate().filter(|(_,(counts,..))|in_contact(counts)).map(|(step,_)|step).collect();
    assert!(contact.len()>=6,"the stroke stays in contact: {contact:?}");
    let mut refreshed_at=frame.time_seconds;let mut refreshes=0;
    for (step,(counts,refreshed,pending,time)) in frames.iter().enumerate() {
        let checked=contact.iter().skip(1).any(|contact|*contact==step);
        if checked {
            assert_eq!(counts.navigator_checks,0,"contact frame {step} refreshes the Navigator from the view window's records: {counts:?}");
            if *refreshed>0 {assert!(*time-refreshed_at>=0.05-1e-4,"contact frame {step} refreshes the Navigator {}s after the last refresh",*time-refreshed_at);refreshes+=1;}
            else {assert!(*pending,"contact frame {step} keeps a Navigator it did not refresh pending");}
        }
        if *refreshed>0 {refreshed_at=*time;}
        if checked {assert!(*time-refreshed_at<=0.06+1e-4,"contact frame {step} shows a Navigator {}s old",*time-refreshed_at);}
    }
    assert!(refreshes>=1,"the Navigator refreshes during the contact");
    let paused=refreshed_at+0.02;let dab=material_dab(348.,300.,&style);
    let persistent=predicted_batches(target,&style,1,&[dab,dab],false)[0].clone();
    crate::test_support::submit_ready(&mut r,FramePacket {dabs:&[dab],dab_batches:std::slice::from_ref(&persistent),time_seconds:paused,..frame}).unwrap();
    assert!(r.navigator.pending() && r.next_frame()==layer_render::FrameDemand::Work,"a Navigator the contact did not refresh asks for another frame");
    let revision=r.navigator.revision;
    crate::test_support::submit_ready(&mut r,FramePacket {time_seconds:paused+0.04,..frame}).unwrap();
    assert!(r.navigator.revision>revision && !r.navigator.pending(),"a pen pause flushes the Navigator without further input");
    let dab=material_dab(324.,300.,&style);
    let end=layer_render::DabBatch {stroke_end:true,..predicted_batches(target,&style,1,&[dab,dab],false)[0].clone()};
    let revision=r.navigator.revision;let counts=crate::test_support::PassCounts::snapshot();
    crate::test_support::submit_ready(&mut r,FramePacket {dabs:&[dab],dab_batches:&[end],time_seconds:paused+0.05,..frame}).unwrap();
    crate::test_support::submit_settled(&mut r,FramePacket {time_seconds:paused+0.06,..frame});
    assert!(crate::test_support::PassCounts::snapshot().delta(counts).navigator_checks>0 && r.navigator.revision>revision && !r.navigator.pending(),
        "the Navigator checks and refreshes its picture once the contact ends");
}

#[test]
fn a_contact_frame_after_the_first_prepares_no_composition_graph() {
    let xs=[300.,304.,308.,312.,316.,320.,324.];
    let counts=predicted_stroke(DefaultBrushPreset::Paintbrush,&xs);
    let contact:Vec<_>=counts.iter().enumerate().filter(|(_,counts)|in_contact(counts)).map(|(step,_)|step).collect();
    assert!(contact.len()>=3,"the stroke stays in contact: {contact:?}");
    for step in contact.iter().skip(1) {
        assert_eq!(counts[*step].graph_prepares,0,"contact frame {step} keeps the contact's graph: {:?}",counts[*step]);
    }
}

#[test]
fn a_contact_on_an_empty_layer_matches_a_twin_without_contact_while_its_graph_inputs_change() {
    let extent=[2048,1024];
    let doc=layer_core::Document::new(layer_core::authored::PortableId::random(),extent[0],extent[1],layer_core::DocumentNames {paint:"Current ink".into(),paper:"Paper".into()});
    let target=doc.working.target.unwrap();let style=crate::layer_tests::preset_style(DefaultBrushPreset::Paintbrush);
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,4);
    let [mut r,mut twin]=[renderer(&doc),renderer(&doc)];
    crate::test_support::submit_settled(&mut r,frame);crate::test_support::submit_settled(&mut twin,frame);
    let xs=[440.,470.,500.,530.,560.,590.];
    let mut contact=0;
    for (step,pair) in xs.windows(2).enumerate() {
        let time=0.01*(step+1) as f32;
        let dabs=[material_dab(pair[0],300.,&style),material_dab(pair[1],300.,&style)];
        let counts=predicted_frame(&mut r,frame,target,&style,1,&dabs,step==0,time);
        predicted_frame(&mut twin,FramePacket {composite_all:true,..frame},target,&style,1,&dabs,step==0,time);
        if in_contact(&counts) {contact+=1;}
        let error=max_error(&display_pixels(&r),&display_pixels(&twin));
        assert!(error<1e-4,"frame {step} differs from a twin without contact by {error}");
    }
    assert!(contact>=2,"the stroke enters contact");
}

#[test]
fn a_tile_entering_the_footprint_mid_contact_is_accepted_once() {
    let xs=[400.,410.,420.,430.,440.,450.,460.,470.,480.,490.];
    let counts=predicted_stroke(DefaultBrushPreset::Paintbrush,&xs);
    let entering=xs.windows(2).position(|pair|pair[1]+40.>=512.).unwrap();
    assert!(in_contact(&counts[entering-1]) && in_contact(&counts[entering]),"the stroke is in contact before and when the tile enters");
    assert_eq!(counts[entering].brush_acceptances,1,"only the entering tile is accepted: {:?}",counts[entering]);
}

#[test]
fn a_predicted_stroke_crossing_tiles_with_an_undo_mid_stroke_matches_a_twin_without_contact() {
    for preset in [DefaultBrushPreset::Paintbrush,DefaultBrushPreset::WatercolorWash] {
        let extent=[2048,1024];let doc=document_at(extent);let target=primary_target(&doc);let style=crate::layer_tests::preset_style(preset);
        let original=doc.scene().raster(target).unwrap().clone();
        let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,4);
        let [mut r,mut twin]=[renderer(&doc),renderer(&doc)];
        crate::test_support::submit_settled(&mut r,frame);crate::test_support::submit_settled(&mut twin,frame);
        let strokes=[(1,vec![440.,470.,500.,530.,560.,590.]),(2,vec![600.,630.,660.,690.,720.,750.])];
        let mut time=0.;
        for (index,(stroke,xs)) in strokes.iter().enumerate() {
            if index>0 {
                time+=0.01;
                let undo=FramePacket {restore_rasters:&[(target,original.clone())],time_seconds:time,..frame};
                crate::test_support::submit_ready(&mut r,undo).unwrap();crate::test_support::submit_ready(&mut twin,FramePacket {composite_all:true,..undo}).unwrap();
            }
            let mut contact=0;
            for (step,pair) in xs.windows(2).enumerate() {
                time+=0.01;
                let dabs=[material_dab(pair[0],300.,&style),material_dab(pair[1],300.,&style)];
                let counts=predicted_frame(&mut r,frame,target,&style,*stroke,&dabs,step==0,time);
                predicted_frame(&mut twin,FramePacket {composite_all:true,..frame},target,&style,*stroke,&dabs,step==0,time);
                if step==0 {assert!(counts.brush_acceptances>0,"{preset:?} stroke {stroke} starts on the per-frame path");}
                if in_contact(&counts) {contact+=1;if contact>1 && preset==DefaultBrushPreset::Paintbrush {assert!(counts.preview_computations==0,"{preset:?} stroke {stroke} frame {step} stamps its preview: {counts:?}");}}
                let error=max_error(&display_pixels(&r),&display_pixels(&twin));
                assert!(error<1e-4,"{preset:?} stroke {stroke} frame {step} differs from a twin without contact by {error}");
            }
            assert!(contact>=3,"{preset:?} stroke {stroke} enters contact");
        }
    }
}

struct BrushContact {engine:layer_engine::CanvasEngine<WgpuRasterizer>,frames:Vec<(Option<u64>,Arc<crate::scene::SignatureFrame>,bool,bool)>,clock:u64,before:Vec<[f32;4]>}
fn settle_engine(engine:&mut layer_engine::CanvasEngine<WgpuRasterizer>,clock:&mut u64) {
    let deadline=std::time::Instant::now()+crate::READBACK_TIMEOUT;
    loop {
        *clock+=8_333_333;
        engine.render_frame_at(*clock).unwrap();engine.backend_mut().wait_idle().unwrap();
        if !engine.has_pending_input() && !engine.has_pending_document_edits() && engine.backend().next_frame()!=layer_render::FrameDemand::Work {break;}
        assert!(std::time::Instant::now()<deadline,"the engine settles");
    }
}
fn brush_contact(preset:DefaultBrushPreset)->BrushContact {
    use layer_engine::{CanvasEngine,PenEvent,PenPhase,SampleFlags,ViewTransform,input_queue};
    let extent=[2048,1024];let frames=12;let doc=document_at(extent);
    let brush=layer_core::default_brush(preset);
    let (mut input,consumer)=input_queue(64);
    let mut engine=CanvasEngine::new(renderer(&doc),doc,consumer,crate::test_support::view(extent),ViewTransform::IDENTITY).unwrap();
    engine.set_brush(brush.clone()).unwrap();
    if preset==DefaultBrushPreset::Eraser {engine.set_tool(layer_core::StrokeTool::Eraser);}
    if matches!(brush.execution,layer_core::BrushExecution::Clone|layer_core::BrushExecution::Heal|layer_core::BrushExecution::SpotHeal) {
        engine.set_retouch(Some(layer_core::RetouchSource::Editing));
        engine.set_clone_source(layer_core::CloneSource {point:Some(layer_core::Point {x:100.,y:700.}),offset:Some([-200.,400.]),aligned:true,flip:[false;2]});
    }
    let mut clock=1_000_000_000;
    settle_engine(&mut engine,&mut clock);
    let before=display_pixels(engine.backend());
    let mut recorded=Vec::new();
    for step in 0..=frames {
        clock+=8_333_333;
        let phase=if step==0 {PenPhase::Down}else if step==frames {PenPhase::Up}else {PenPhase::Move};
        input.push(PenEvent {timestamp_ns:clock,pressure:0.8,..crate::test_support::pen(step as u64+1,phase,[300.+40.*step as f32,300.],SampleFlags::PRIMARY)}).unwrap();
        let counts=crate::test_support::PassCounts::snapshot();let dabs=engine.backend().metrics.dabs;
        let deadline=std::time::Instant::now()+crate::READBACK_TIMEOUT;
        loop {
            engine.render_frame_at(clock).unwrap();engine.backend_mut().wait_idle().unwrap();
            if !engine.has_pending_input() && !engine.has_pending_document_edits() {break;}
            assert!(std::time::Instant::now()<deadline,"{preset:?} frame {step} is consumed");
            clock+=8_333_333;
        }
        let counts=crate::test_support::PassCounts::snapshot().delta(counts);
        let r=engine.backend();
        if step<frames {recorded.push((r.scale_display.as_ref().unwrap().contact_id(),held_frame(r),in_contact(&counts),r.metrics.dabs>dabs));}
    }
    settle_engine(&mut engine,&mut clock);
    BrushContact {engine,frames:recorded,clock,before}
}
fn fresh_view(doc:&layer_core::Document)->Vec<[f32;4]> {
    let mut frame=packet(doc.scene(),doc.composition().size);frame.composite_all=false;
    let mut fresh=renderer(doc);crate::test_support::submit_settled(&mut fresh,frame);display_pixels(&fresh)
}

#[test]
fn a_reduced_pencil_stroke_on_a_perceptual_photo_changes_display_texels_outside_its_damaged_rects_only_back_to_its_paint() {
    let (extent,frames,resting)=([2400,1600],24,16);
    let photo=rgba8_source(extent,|x,y| {
        let mut h=x.wrapping_mul(0x9E37_79B1)^y.wrapping_mul(0x85EB_CA77);h^=h>>15;h=h.wrapping_mul(0x2C1B_3C6D);h^=h>>12;
        let noise=((h&255)+((h>>8)&255)+((h>>16)&255)) as i32/4-96;
        let base=[90+(x*80/extent[0]) as i32,120-(y*60/extent[1]) as i32,110+((x+y)*50/(extent[0]+extent[1])) as i32];
        let [r,g,b]=base.map(|b|(b+noise).clamp(0,255) as u8);[r,g,b,255]
    });
    let doc=layer_color::photo_project(Arc::unwrap_or_clone(photo),Default::default(),"Ink".into(),SampleDepth::U8).unwrap();
    let target=doc.active_target().unwrap();
    let mut style=crate::layer_tests::preset_style(DefaultBrushPreset::Pencil);style.blend_space=doc.composition().blend;
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,4);frame.blend_space=doc.composition().blend;
    let [mut r,mut paint]=[renderer(&doc),renderer(&doc)];
    for r in [&mut r,&mut paint] {crate::test_support::submit_settled(r,frame);}
    let plan=r.scale_display.as_ref().unwrap().plan;let side=1u32<<plan.level;
    let xs:Vec<f32>=(0..=frames+1).map(|step|300.+40.*step.min(resting) as f32).collect();
    let dab=|x:f32|layer_render::Dab {radii:[500.;2],..material_dab(x,800.,&style)};
    let mut shown=display_pixels(&r);let mut previous=Vec::new();let mut moved=Vec::new();let mut composed=Vec::new();
    for step in 0..=frames {
        let dabs=[dab(xs[step]),dab(xs[step+1])];
        let [persistent,preview]=predicted_batches(target,&style,1,&dabs,step==0);
        let persistent=layer_render::DabBatch {stroke_end:step==frames,..persistent};
        let batches=if step==frames {vec![persistent.clone()]}else {vec![persistent.clone(),preview]};
        let packet=FramePacket {dabs:&dabs,dab_batches:&batches,time_seconds:0.01*(step+1) as f32,..frame};
        let counts=crate::test_support::PassCounts::snapshot();
        crate::test_support::submit_ready(&mut r,packet).unwrap();
        if !in_contact(&crate::test_support::PassCounts::snapshot().delta(counts)) {composed.push(step);}
        crate::test_support::submit_ready(&mut paint,FramePacket {dabs:&dabs[..1],dab_batches:std::slice::from_ref(&persistent),..packet}).unwrap();
        let tiles=crate::brush_tiles::plan_packet(packet,|target|r.target_extent(target)).unwrap();
        let current:Vec<_>=tiles.get(1).into_iter().flatten().map(Damage::tile_region).collect();
        let damaged:Vec<_>=tiles[0].iter().map(Damage::tile_region).chain(current.iter().copied()).chain(previous).collect();
        let (now,painted)=(display_pixels(&r),display_pixels(&paint));
        let difference=|a:&[f32;4],b:&[f32;4]|a[..3].iter().zip(&b[..3]).map(|(a,b)|(a-b).abs()).fold(0f32,f32::max);
        let outside=(0..plan.size[1]).flat_map(|y|(0..plan.size[0]).map(move |x|[x,y])).filter_map(|[x,y]| {
            let index=(y*plan.size[0]+x) as usize;
            let texel=PixelRect::new(plan.bounds.min_x()+x*side,plan.bounds.min_y()+y*side,plan.bounds.min_x()+(x+1)*side,plan.bounds.min_y()+(y+1)*side);
            (difference(&now[index],&shown[index])>2e-3 && difference(&now[index],&painted[index])>1e-4 && damaged.iter().all(|rect|rect.intersect(texel).is_empty()))
                .then_some(difference(&now[index],&shown[index]))
        }).collect::<Vec<_>>();
        if step>0 && !outside.is_empty() {moved.push((step,outside.len(),outside.into_iter().fold(0f32,f32::max)));}
        shown=now;previous=current;
    }
    for r in [&mut r,&mut paint] {crate::test_support::submit_settled(r,frame);}
    let error=max_error(&display_pixels(&r),&display_pixels(&paint));
    assert_eq!(composed,[0,frames],"every frame between the stroke's first and last composes in contact");
    assert!(moved.is_empty(),"frames after the first changing display texels outside their damaged rects to anything but the paint (frame, texels, largest change): {moved:?}");
    assert!(error<1e-4,"after pen-up the view differs from a renderer given only the persistent dabs by {error}");
}

#[test]
fn a_wide_wet_watercolor_stroke_on_a_zoomed_out_photo_reads_only_tiles_its_contact_declared() {
    use layer_engine::{CanvasEngine,PenEvent,PenPhase,SampleFlags,ViewTransform,input_queue};
    let (extent,zoom,events)=([9504,6336],0.2,120);
    let mut doc=document_at(extent);
    let root=doc.artwork.root;doc.artwork.compositions.get_mut(root).unwrap().blend=layer_core::BlendSpace::Perceptual;
    let mut view=crate::test_support::view([2200,1440]);
    view.document_to_surface=[zoom,0.,0.,zoom,1100.-zoom*extent[0] as f32/2.,720.-zoom*extent[1] as f32/2.];
    let [_,_,_,_,x,y]=view.document_to_surface;
    let (mut input,consumer)=input_queue(1024);
    let mut engine=CanvasEngine::new(renderer(&doc),doc,consumer,view,ViewTransform {revision:1,surface_to_document:[1./zoom,0.,0.,1./zoom,-x/zoom,-y/zoom]}).unwrap();
    engine.set_brush(layer_core::BrushSnapshot {diameter:512.,..layer_core::default_brush(DefaultBrushPreset::WetWatercolor)}).unwrap();
    let mut clock=1_000_000_000;
    settle_engine(&mut engine,&mut clock);
    let mut contact=false;
    for step in 0..=events {
        clock+=8_333_333;
        let angle=step as f32/events as f32*std::f32::consts::TAU;
        let phase=if step==0 {PenPhase::Down}else if step==events {PenPhase::Up}else {PenPhase::Move};
        input.push(PenEvent {timestamp_ns:clock,view_revision:1,..crate::test_support::pen(step as u64+1,phase,[1100.+520.*angle.cos(),720.+299.*angle.sin()],SampleFlags::PRIMARY)}).unwrap();
        let deadline=std::time::Instant::now()+crate::READBACK_TIMEOUT;
        loop {
            engine.render_frame_at(clock).expect("a watercolor contact frame reads only tiles its contact declared");
            engine.backend_mut().wait_idle().unwrap();
            contact|=engine.backend().scale_display.as_ref().unwrap().contact_id().is_some();
            if !engine.has_pending_input() {break;}
            assert!(std::time::Instant::now()<deadline,"frame {step} is consumed");
            clock+=8_333_333;
        }
    }
    settle_engine(&mut engine,&mut clock);
    assert!(contact,"the wash enters contact");
}
#[derive(PartialEq)]
enum BrushFamily {DryOrRetouching,Smudge,WetOrWatercolor,Liquify}
fn brush_family(preset:DefaultBrushPreset)->BrushFamily {
    use layer_core::BrushExecution as E;
    match layer_core::default_brush(preset).execution_class() {
        E::Dry|E::Clone|E::Heal|E::SpotHeal=>BrushFamily::DryOrRetouching,E::Smudge=>BrushFamily::Smudge,E::Wet|E::Watercolor=>BrushFamily::WetOrWatercolor,E::Liquify=>BrushFamily::Liquify,
    }
}
fn presets_hold_their_contact_frame_until_pen_up(family:BrushFamily) {
    let presets=layer_core::DefaultBrushPreset::ALL.into_iter().filter(|preset|brush_family(*preset)==family).collect::<Vec<_>>();
    assert!(!presets.is_empty());
    let mut failures=Vec::new();
    for preset in presets {
        let BrushContact {mut engine,frames,mut clock,before}=brush_contact(preset);
        let start=frames.iter().position(|(id,..)|id.is_some());
        if preset==DefaultBrushPreset::LiquifyReconstruct {
            assert!(start.is_none() && !engine.can_undo(),"Reconstruct on a layer that was never liquified paints nothing, so it never starts a contact");
            continue;
        }
        let Some(start)=start else {failures.push(format!("{preset:?} never starts a contact"));continue;};
        if frames.iter().position(|(..,painted)|*painted)!=Some(start) {failures.push(format!("{preset:?} starts its contact on frame {start}, not on its first painting frame"));}
        if let Some(k)=(start+1..frames.len()).find(|k|!(frames[*k].0==frames[k-1].0 && Arc::ptr_eq(&frames[*k].1,&frames[k-1].1) && frames[*k].2)) {
            failures.push(format!("{preset:?} contact frame {k} does not hold the contact's frame"));
        }
        let shown=display_pixels(engine.backend());
        if max_error(&shown,&before)==0. {failures.push(format!("{preset:?} paints nothing"));}
        let error=max_error(&shown,&fresh_view(engine.document()));
        if error>=1e-4 {failures.push(format!("{preset:?} after pen-up differs from a fresh renderer by {error}"));}
        for (step,redo) in [("undo",false),("redo",true)] {
            assert!(if redo {engine.redo().unwrap()}else {engine.undo().unwrap()},"{preset:?} {step}");
            settle_engine(&mut engine,&mut clock);
            let error=max_error(&display_pixels(engine.backend()),&fresh_view(engine.document()));
            if error>=1e-4 {failures.push(format!("{preset:?} after {step} differs from a fresh renderer by {error}"));}
        }
    }
    assert!(failures.is_empty(),"{}",failures.join("\n"));
}

#[test]
fn dry_and_retouching_presets_hold_their_contact_frame_until_pen_up() {presets_hold_their_contact_frame_until_pen_up(BrushFamily::DryOrRetouching);}

#[test]
fn smudge_presets_hold_their_contact_frame_until_pen_up() {presets_hold_their_contact_frame_until_pen_up(BrushFamily::Smudge);}

#[test]
fn wet_and_watercolor_presets_hold_their_contact_frame_until_pen_up() {presets_hold_their_contact_frame_until_pen_up(BrushFamily::WetOrWatercolor);}

#[test]
fn liquify_presets_hold_their_contact_frame_until_pen_up() {presets_hold_their_contact_frame_until_pen_up(BrushFamily::Liquify);}

#[test]
fn a_predicted_watercolor_wash_leaving_a_settled_tile_matches_a_recomposing_twin_at_every_reduced_level() {
    let extent=[2048,1024];
    let doc=layer_core::Document::new(layer_core::authored::PortableId::random(),extent[0],extent[1],layer_core::DocumentNames {paint:"Current ink".into(),paper:"Paper".into()});
    let target=doc.working.target.unwrap();let style=crate::layer_tests::preset_style(DefaultBrushPreset::WatercolorWash);
    let wash=|x:f32|{let mut dab=stroke_dab(x,300.);dab.radii=[70.;2];dab};
    for (divisor,level) in [(2,1),(4,2)] {
        let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,divisor);
        let [mut r,mut twin]=[renderer(&doc),renderer(&doc)];
        crate::test_support::submit_settled(&mut r,frame);crate::test_support::submit_settled(&mut twin,frame);
        assert_eq!(r.scale_display.as_ref().unwrap().plan.level,level);
        let (mut contact,mut x,mut time)=(0,100.,0.);
        for step in 0..30usize {
            x=100.+30.*step.saturating_sub(9) as f32;time=0.01*(step+1) as f32;
            let dabs=[wash(x),wash(x+30.)];
            if in_contact(&predicted_frame(&mut r,frame,target,&style,1,&dabs,step==0,time)) {contact+=1;}
            predicted_frame(&mut twin,FramePacket {composite_all:true,..frame},target,&style,1,&dabs,step==0,time);
            let error=max_error(&display_pixels(&r),&display_pixels(&twin));
            assert!(error<1e-4,"watercolor contact frame {step} at view level {level} differs from a twin that recomposes every frame by {error}");
        }
        assert!(contact>=20,"the wash at view level {level} stays in contact: {contact} frames");
        let dab=wash(x);
        let end=layer_render::DabBatch {stroke_end:true,..predicted_batches(target,&style,1,&[dab,dab],false)[0].clone()};
        for (renderer,composite_all) in [(&mut r,false),(&mut twin,true)] {
            crate::test_support::submit_ready(renderer,FramePacket {dabs:&[dab],dab_batches:std::slice::from_ref(&end),time_seconds:time+0.01,composite_all,..frame}).unwrap();
            crate::test_support::submit_settled(renderer,FramePacket {time_seconds:time+0.02,..frame});
        }
        let error=max_error(&display_pixels(&r),&display_pixels(&twin));
        assert!(error<1e-4,"the wash at view level {level} after pen-up differs from a twin that recomposed every frame by {error}");
    }
}

#[test]
fn a_contact_frame_decides_the_tiles_its_brush_wrote_from_its_writes_as_a_declaration_would() {
    for preset in [DefaultBrushPreset::Paintbrush,DefaultBrushPreset::WatercolorWash] {
        let extent=[2048,1024];let doc=document_at(extent);let target=primary_target(&doc);let style=crate::layer_tests::preset_style(preset);
        let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,4);
        let mut r=renderer(&doc);
        crate::test_support::submit_settled(&mut r,frame);
        let xs=[480.,500.,520.,540.,560.,580.];
        let mut checked=0;
        for (step,pair) in xs.windows(2).enumerate() {
            let counts=predicted_frame(&mut r,frame,target,&style,1,&[material_dab(pair[0],300.,&style),material_dab(pair[1],300.,&style)],step==0,0.01*(step+1) as f32);
            if step==0 || !in_contact(&counts) {continue;}
            let cache=r.scale_display.take().expect("a view cache");
            let compared=cache.contact_decisions_differ_from_a_declaration(&r,doc.scene());
            r.scale_display=Some(cache);
            let (declared,differing)=compared.expect("the written tiles declare");
            assert!(declared>0,"{preset:?} contact frame {step} has written tiles");
            assert!(differing.is_empty(),"{preset:?} contact frame {step} decides written tiles unlike a declaration: {}",differing.join(", "));
            assert_eq!(counts.contact_declarations,0,"{preset:?} contact frame {step} declares no tile its brush wrote: {counts:?}");
            checked+=1;
        }
        assert!(checked>=3,"{preset:?} stays in contact");
    }
}

#[test]
fn a_watercolor_contact_frame_reads_each_material_neighbour_once_per_batch() {
    let extent=[2048,1024];let doc=document_at(extent);let target=primary_target(&doc);let style=crate::layer_tests::preset_style(DefaultBrushPreset::WatercolorWash);
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,4);
    let mut r=renderer(&doc);
    crate::test_support::submit_settled(&mut r,frame);
    let xs=[480.,500.,520.,540.,560.,580.];
    let mut checked=0;
    for (step,pair) in xs.windows(2).enumerate() {
        let dabs=[material_dab(pair[0],300.,&style),material_dab(pair[1],300.,&style)];
        let (counts,reads)=crate::test_support::material_reads(||predicted_frame(&mut r,frame,target,&style,1,&dabs,step==0,0.01*(step+1) as f32));
        if step==0 || !in_contact(&counts) {continue;}
        assert!(reads.len()>=4,"contact frame {step} reads a material neighbourhood: {}",reads.len());
        let reread=reads.iter().filter(|(_,count)|**count>1).map(|((coordinate,stage),count)|format!("tile {coordinate:?} {stage:?} x{count}")).collect::<Vec<_>>();
        assert!(reread.is_empty(),"contact frame {step} reads a material neighbour more than once per batch: {}",reread.join(", "));
        checked+=1;
    }
    assert!(checked>=3,"the wash stays in contact");
}

#[test]
fn watercolor_contact_frames_name_read_and_bind_each_transported_tile_once() {
    let extent=[2048,1024];let doc=document_at(extent);let target=primary_target(&doc);let style=crate::layer_tests::preset_style(DefaultBrushPreset::WatercolorWash);
    assert!(style.transport.as_ref().is_some_and(|transport|transport.distance>=1. && (transport.wet_flow>0. || transport.dry_flow>0.)),"the wash transports");
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,4);
    let mut r=renderer(&doc);
    crate::test_support::submit_settled(&mut r,frame);
    let xs=[480.,500.,520.,540.,560.,580.];
    let (mut checked,mut previous,mut repeated_transport)=(0,None,0);
    for (step,pair) in xs.windows(2).enumerate() {
        let dabs=[material_dab(pair[0],300.,&style),material_dab(pair[1],300.,&style)];
        let (((counts,names),regions),reads)=crate::test_support::transport_reads(||crate::test_support::preview_regions(||crate::test_support::preview_names(||predicted_frame(&mut r,frame,target,&style,1,&dabs,step==0,0.01*(step+1) as f32))));
        if step==0 || !in_contact(&counts) {continue;}
        let transported=names.iter().filter(|((_,_,_,stage),_)|matches!(stage,crate::scene::PreviewStage::AfterBatch(_))).collect::<Vec<_>>();
        assert!(!transported.is_empty(),"contact frame {step} names its transported tiles");
        let repeated=transported.iter().filter(|(_,count)|**count>1).map(|((_,plane,coordinate,_),count)|format!("plane {plane} tile {coordinate:?} x{count}")).collect::<Vec<_>>();
        assert!(repeated.is_empty(),"contact frame {step} names a transported tile more than once per plane: {}",repeated.join(", "));
        let derived=regions.iter().filter(|(_,count)|**count>1).map(|((plane,bounds,stage,pass),count)|format!("plane {plane} {bounds:?} {stage:?} {pass:?} x{count}")).collect::<Vec<_>>();
        assert!(derived.is_empty(),"contact frame {step} derives a preview region name more than once: {}",derived.join(", "));
        assert!(!reads.is_empty(),"contact frame {step} transports");
        let reread=reads.iter().filter(|(_,count)|**count>1).map(|((plane,coordinate,batch,transport),count)|format!("plane {plane} tile {coordinate:?} batch {batch} step {transport} x{count}")).collect::<Vec<_>>();
        assert!(reread.is_empty(),"contact frame {step} reads a transported tile more than once per step: {}",reread.join(", "));
        let transported=reads.keys().map(|(_,coordinate,batch,transport)|(*coordinate,*batch,*transport)).collect::<std::collections::BTreeSet<_>>();
        if previous.as_ref()==Some(&transported) {
            assert_eq!(counts.transport_bind_groups,0,"contact frame {step} transports the tiles of the frame before and binds them again");
            repeated_transport+=1;
        }
        previous=Some(transported);
        checked+=1;
    }
    assert!(checked>=3,"the wash stays in contact");
    assert!(repeated_transport>=1,"the wash transports the same tiles on consecutive contact frames");
}

#[test]
fn a_stroke_crossing_into_a_cell_without_its_halves_keeps_every_dab_and_shows_the_crossing_within_the_stroke() {
    let extent=[2048,1024];
    let mut doc=document_at(extent);
    let below=paint_occurrence(&mut doc,"Below",Some(rgba8_source(extent,|x,y|[(x/9) as u8,(y/5) as u8,140,255])));
    let working=doc.scene().position(doc.working.occurrence.unwrap()).unwrap();
    insert_occurrence(&mut doc,below,working+1);
    let target=doc.active_target().unwrap();
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,4);
    let [mut r,mut twin]=[renderer(&doc),renderer(&doc)];
    for r in [&mut r,&mut twin] {crate::test_support::submit_settled(r,frame);}
    let unready=PixelRect::new(1024,0,2048,1024);
    {let cache=r.scale_display.as_ref().unwrap();evict_composite_pages(cache,&[unready]);assert!(cache.pivot_missing(unready));}
    let xs=[700.,800.,900.,1000.,1100.,1200.,1300.,1400.,1400.,1400.,1400.,1400.];
    let original=display_pixels(&r);let level_side=4;
    let ink=|pixels:&[[f32;4]],x:f32|{let index=(500/level_side*(extent[0]/level_side)+x as u32/level_side) as usize;max_error(&pixels[index..=index],&original[index..=index])};
    let mut crossed=None;
    for (step,x) in xs.iter().enumerate() {
        let dab=stroke_dab(*x,500.);let batch=stroke_batch(target,&dab,step==0,step+1==xs.len());
        let next=FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),time_seconds:0.01*(step+1) as f32,..frame};
        crate::test_support::submit_ready(&mut r,next).unwrap();
        crate::test_support::submit_ready(&mut twin,FramePacket {composite_all:true,..next}).unwrap();
        if *x>=1400. && crossed.is_none() && ink(&display_pixels(&r),1400.)>0.1 {crossed=Some(step);}
        let reduced=crate::scene::scale::layer_pages(&r);
        assert!(reduced.iter().all(|source|*source==target),"the stroke reduces only its own layer: {reduced:?}");
    }
    assert!(crossed.is_some_and(|step|step+1<xs.len()),"the dab in the cell without halves shows within the stroke: {crossed:?}");
    for r in [&mut r,&mut twin] {crate::test_support::submit_settled(r,frame);}
    let error=max_error(&display_pixels(&r),&display_pixels(&twin));
    assert!(error<1e-4,"every dab keeps its ink across the crossing: {error}");
}

#[test]
fn a_memory_trim_during_a_stroke_ends_the_contact_for_cells_it_did_not_declare_instead_of_stopping_the_canvas() {
    let extent=[2048,1024];
    let mut doc=Document::new(PortableId::random(),extent[0],extent[1],layer_core::DocumentNames {paint:"Current ink".into(),paper:"Paper".into()});
    let below=paint_occurrence(&mut doc,"Below",Some(rgba8_source(extent,|x,y|[(x/9) as u8,(y/5) as u8,140,255])));
    let working=doc.scene().position(doc.working.occurrence.unwrap()).unwrap();
    insert_occurrence(&mut doc,below,working+1);
    let (ink,lower)=(doc.active_target().unwrap(),doc.scene().source_target(below).unwrap());
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,2);
    let [mut r,mut twin]=[renderer(&doc),renderer(&doc)];
    let submit=|r:&mut WgpuRasterizer,target:SourceTarget,stroke:u64,[x,y]:[f32;2],[start,end]:[bool;2],time:f32| {
        let dab=stroke_dab(x,y);let batch=layer_render::DabBatch {stroke_id:layer_core::StrokeId(stroke),..stroke_batch(target,&dab,start,end)};
        crate::test_support::submit_ready(r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),time_seconds:time,..frame})
    };
    for r in [&mut r,&mut twin] {
        crate::test_support::submit_settled(r,frame);
        for (step,x) in [1150.,1200.,1250.,1300.,1350.].into_iter().enumerate() {submit(r,ink,1,[x,250.],[step==0,false],0.01*(step+1) as f32).unwrap();}
        submit(r,ink,1,[1800.,800.],[false,true],0.06).unwrap();
        layer_render::CanvasRenderer::set_working_layer(r,Some(below));
    }
    let mut trimmed=None;
    for step in 0..16 {
        let time=0.1+0.01*step as f32;
        submit(&mut twin,lower,2,[300.+10.*step as f32,700.],[step==0,false],time).unwrap();
        submit(&mut r,lower,2,[300.+10.*step as f32,700.],[step==0,false],time)
            .expect("a stroke frame after a memory trim ends the contact instead of stopping the canvas");
        let cache=r.scale_display.as_ref().unwrap();
        if trimmed.is_none() && cache.gesture.is_some() {
            assert!(cache.pivot_missing(cache.plan.bounds),"the contact begins without the halves of some cells it shows");
            r.scene.as_ref().unwrap().release_pages_by(u64::MAX);
            trimmed=Some(step);
        }
    }
    assert!(trimmed.is_some_and(|step|step<15),"the stroke on the selected layer enters contact before its last frame: {trimmed:?}");
    for r in [&mut r,&mut twin] {
        submit(r,lower,2,[460.,700.],[false,true],0.3).unwrap();
        crate::test_support::submit_settled(r,frame);
    }
    let error=max_error(&display_pixels(&r),&display_pixels(&twin));
    assert!(error<1e-4,"the trimmed renderer keeps every dab and recomposes the cells it released: {error}");
}

pub(crate) fn layered_fit(doc:&layer_core::Document)->(WgpuRasterizer,FramePacket<'_>) {
    let extent=doc.composition().size;let scale=1200./extent[1] as f32;
    let r=crate::memory::memory_tests::renderer(doc.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(8<<30),..Default::default()}).unwrap();
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
    frame.view.width_px=1920;frame.view.height_px=1200;frame.view.document_to_surface=[scale,0.,0.,scale,(1920.-scale*extent[0] as f32)/2.,0.];
    (r,frame)
}
pub(crate) fn named(doc:&layer_core::Document,name:&str)->layer_core::OccurrenceHandle {
    doc.scene().order().iter().copied().find(|handle|doc.scene().occurrence(*handle).unwrap().name.as_ref()==name).unwrap()
}
fn pencil(x:f32,y:f32,radius:f32,style:&layer_render::DabStyle)->layer_render::Dab {
    let mut dab=material_dab(x,y,style);dab.radii=[radius;2];dab.previous=[radius,radius,1.,0.];dab.motion=[8.,0.];dab
}
pub(crate) fn pencil_frame(r:&mut WgpuRasterizer,frame:FramePacket<'_>,target:SourceTarget,stroke:u64,[x,y]:[f32;2],radius:f32,step:usize)->crate::test_support::PassCounts {
    let style=crate::layer_tests::preset_style(DefaultBrushPreset::Pencil);
    let x=x+16.*step as f32;
    let counts=predicted_frame(r,frame,target,&style,stroke,&[pencil(x,y,radius,&style),pencil(x+8.,y,radius,&style)],step==0,0.01*(step+1) as f32);
    crate::test_support::complete(r);counts
}
pub(crate) fn pencil_lift(r:&mut WgpuRasterizer,frame:FramePacket<'_>,target:SourceTarget,stroke:u64,[x,y]:[f32;2],radius:f32) {
    let style=crate::layer_tests::preset_style(DefaultBrushPreset::Pencil);
    let last=pencil(x,y,radius,&style);let end=layer_render::DabBatch {stroke_id:layer_core::StrokeId(stroke),stroke_start:false,stroke_end:true,..dab_batch(target,style,last.bounds())};
    crate::test_support::submit_ready(r,FramePacket {dabs:std::slice::from_ref(&last),dab_batches:std::slice::from_ref(&end),time_seconds:1.,..frame}).unwrap();
    crate::test_support::complete(r);
}
fn pencil_cells(r:&WgpuRasterizer,[x,y]:[f32;2],radius:f32)->usize {
    let plan=r.scale_display.as_ref().unwrap().plan;let style=crate::layer_tests::preset_style(DefaultBrushPreset::Pencil);
    [x,x+8.].into_iter().flat_map(|x|pages::coordinates(crate::pixel_rect::pixel_rect(pencil(x,y,radius,&style).bounds(),plan.extent).intersect(plan.bounds),plan.level))
        .collect::<BTreeSet<_>>().len()
}
struct Switched {counts:crate::test_support::PassCounts,decodes:u64,reach:usize,contact:bool,shown:bool}
fn first_dab_after_a_switch_and_a_reaction_gap() -> Switched {
    let doc=crate::memory::memory_tests::layered_raster_document(32,16);
    let (mut r,frame)=layered_fit(&doc);
    let (drawn,selected)=(named(&doc,"Layer 31"),named(&doc,"Layer 28"));
    let at=[640.,640.];
    layer_render::CanvasRenderer::set_working_layer(&mut r,Some(drawn));
    crate::test_support::submit_settled(&mut r,frame);
    let target=doc.scene().source_target(drawn).unwrap();
    for step in 0..3 {pencil_frame(&mut r,frame,target,1,at,12.,step);}
    pencil_lift(&mut r,frame,target,1,at,12.);
    crate::test_support::submit_settled(&mut r,frame);
    layer_render::CanvasRenderer::set_working_layer(&mut r,Some(selected));
    crate::test_support::submit_settled(&mut r,frame);
    let target=doc.scene().source_target(selected).unwrap();
    let at=[at[0],at[1]+96.];
    let misses=r.metrics().source_tile_misses;
    let counts=pencil_frame(&mut r,frame,target,2,at,12.,0);
    let style=crate::layer_tests::preset_style(DefaultBrushPreset::Pencil);
    let plan=r.scale_display.as_ref().unwrap().plan;
    let reach=[at[0],at[0]+8.].into_iter().flat_map(|x|page_coordinates(crate::pixel_rect::pixel_rect(pencil(x,at[1],12.,&style).bounds(),plan.extent))).collect::<BTreeSet<_>>();
    let shown=reach.iter().all(|tile|r.metrics.frame_composited_pages.contains(&(plan.level,*tile)));
    Switched {counts,decodes:r.metrics().source_tile_misses-misses,reach:reach.len(),contact:r.scale_display.as_ref().unwrap().gesture.is_some(),shown}
}

#[test]
fn an_edited_pivot_prepares_its_next_stroke_sources_when_the_frame_budget_is_spent() {
    let doc=crate::memory::memory_tests::layered_raster_document(8,16);
    let (mut r,frame)=layered_fit(&doc);
    let layer=named(&doc,"Layer 5");let target=doc.scene().source_target(layer).unwrap();
    layer_render::CanvasRenderer::set_working_layer(&mut r,Some(layer));
    crate::test_support::submit_settled(&mut r,frame);
    let at=[1200.,1200.];
    for step in 0..3 {pencil_frame(&mut r,frame,target,1,at,12.,step);}
    pencil_lift(&mut r,frame,target,1,at,12.);
    crate::test_support::submit_settled(&mut r,frame);
    assert!(r.scale_display.as_ref().unwrap().pivot_edited(),"the stroke edits the pivot");
    let (mut cache,mut scene)=(r.scale_display.take().unwrap(),r.scene.take().unwrap());
    for level in 0..=cache.plan.level {cache.pages.lock().unwrap().remove_source_level(target,level);}
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    let admitted={
        let mut budget=crate::test_support::work(false);budget.spend(budget.remaining());
        let spent=budget.spent();
        scene.begin_frame();scene.scale_sources.prepare(&r,frame,&[]);
        cache.prepare_edit_sources(&mut scene,&mut r,frame,&mut encoder,&mut budget).unwrap();
        budget.spent()-spent
    };
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    r.scale_display=Some(cache);r.scene=Some(scene);
    assert!(admitted>=object_cache::PASS_TAPS,"a frame whose budget earlier work spent admits no unit to prepare the next stroke's sources");
}

#[test]
fn the_first_contact_frame_after_a_switch_folds_no_layers_and_declares_only_cells_near_its_dab() {
    let Switched {counts,decodes,reach,contact,..}=first_dab_after_a_switch_and_a_reaction_gap();
    assert!(contact,"the pen-down frame after a switch and a reaction gap begins its contact: {counts:?}");
    assert_eq!(counts.fold_inputs,0,"the pen-down frame builds no halves: {counts:?}");
    assert!(decodes<=reach as u64,"the pen-down frame decodes {decodes} tiles; its dabs reach {reach}");
    assert!(counts.declarations<=10+counts.unit_acceptances,"the pen-down frame declares {} times; the cells within one cell of its dab are 9, with one reduction and one declaration per unit it accepts: {counts:?}",counts.declarations);
}

#[test]
fn the_dab_after_a_switch_and_a_reaction_gap_is_composed_on_its_first_frame() {
    let Switched {counts,contact,shown,..}=first_dab_after_a_switch_and_a_reaction_gap();
    assert!(contact && shown,"the first frame after the gap composes the dab's cell into the view and begins its contact: shown {shown}, contact {contact}, {counts:?}");
}

#[test]
fn a_stroke_started_while_the_last_one_composes_begins_in_contact_and_commits_only_its_own_cells() {
    let doc=crate::memory::memory_tests::layered_raster_document(8,16);
    let (mut r,frame)=layered_fit(&doc);
    layer_render::CanvasRenderer::set_working_layer(&mut r,Some(named(&doc,"Layer 7")));
    crate::test_support::submit_settled(&mut r,frame);
    let layer=named(&doc,"Layer 5");let target=doc.scene().source_target(layer).unwrap();
    layer_render::CanvasRenderer::set_working_layer(&mut r,Some(layer));
    crate::test_support::submit_settled(&mut r,frame);
    for step in 0..3 {pencil_frame(&mut r,frame,target,1,[1800.,2048.],512.,step);}
    pencil_lift(&mut r,frame,target,1,[1900.,2048.],512.);
    assert!(r.scale_display.as_ref().unwrap().has_pending_work(&r),"the last stroke's cells still compose");
    let at=[2400.,2048.];
    let counts=pencil_frame(&mut r,frame,target,2,at,12.,0);
    let own=pencil_cells(&r,at,12.);
    assert!(r.scale_display.as_ref().unwrap().gesture.is_some(),"the new stroke begins in contact: {counts:?}");
    assert!(counts.composite_commits<=own as u64,"the new stroke's first frame commits {} cells; its dabs reach {own}: {counts:?}",counts.composite_commits);
}
