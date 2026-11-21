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
fn a_watercolor_update_ending_in_a_batch_off_its_layer_commits_its_pages_before_the_next_update() {
    let extent=[1024,1024];let doc=document_at(extent);let target=doc.active_target().unwrap();
    let style=crate::layer_tests::preset_style(DefaultBrushPreset::WetWatercolor);
    let frame=FramePacket {composite_all:true,..packet(doc.scene(),extent)};
    let [mut r,mut twin]=[renderer(&doc),renderer(&doc)];
    for r in [&mut r,&mut twin] {crate::test_support::submit_settled(r,frame);}
    let wash=|x:f32,y:f32|layer_render::Dab {radii:[60.;2],material:[0.5,0.8,1.,0.8],..stroke_dab(x,y)};
    let dabs=[wash(128.,128.),wash(-4000.,-4000.),wash(900.,900.)];
    let batch=|kind,index:usize|layer_render::DabBatch {kind,first_dab:index as u32,stroke_start:index==0,
        ..crate::test_support::dab_batch(target,style.clone(),dabs[index].bounds())};
    let persistent=[batch(layer_render::DabBatchKind::Persistent,0),batch(layer_render::DabBatchKind::Persistent,1)];
    crate::test_support::submit_ready(&mut r,FramePacket {dabs:&dabs,dab_batches:&persistent,time_seconds:0.01,..frame}).unwrap();
    crate::test_support::submit_ready(&mut twin,FramePacket {dabs:&dabs,dab_batches:&persistent[..1],time_seconds:0.01,..frame}).unwrap();
    let preview=[batch(layer_render::DabBatchKind::Preview,2)];
    for r in [&mut r,&mut twin] {
        crate::test_support::submit_ready(r,FramePacket {dabs:&dabs,dab_batches:&preview,time_seconds:0.02,..frame}).expect("the next update commits only the pages it wrote");
        crate::test_support::submit_settled(r,FramePacket {time_seconds:0.03,..frame});
    }
    let error=max_error(&display_pixels(&r),&display_pixels(&twin));
    assert!(error<1e-4,"the update keeps the wash its first batch painted: {error}");
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
fn a_zoom_after_pen_up_writes_back_pages_equal_to_a_keyed_composition_of_the_same_state() {
    let extent=[2048,1024];let doc=document_at(extent);let target=primary_target(&doc);
    let whole=layer_render::ViewState {width_px:extent[0],height_px:extent[1],document_to_surface:[1.,0.,0.,1.,0.,0.]};
    let quarter=layer_render::ViewState {width_px:extent[0]/4,height_px:extent[1]/4,document_to_surface:[0.25,0.,0.,0.25,0.,0.]};
    let part=layer_render::ViewState {width_px:512,height_px:512,document_to_surface:[1.,0.,0.,1.,-256.,-256.]};
    for (label,view,zoomed) in [("a window over the whole drawing",whole,quarter),("a window over part of the drawing, zoomed to its overview",part,quarter)] {
        let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view;
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        crate::test_support::submit_settled(&mut r,frame);
        let xs=[300.,360.,420.,480.];let mut touched=PixelRect::EMPTY;
        for (step,x) in xs.into_iter().enumerate() {
            let dab=stroke_dab(x,384.);let batch=stroke_batch(target,&dab,step==0,step+1==xs.len());
            touched=touched.union(crate::pixel_rect::pixel_rect(dab.bounds(),extent));
            crate::test_support::submit_ready(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),time_seconds:0.01*(step+1) as f32,..frame}).unwrap();
        }
        crate::test_support::submit_ready(&mut r,frame).unwrap();
        crate::test_support::complete(&r);
        let cache=r.scale_display.as_ref().unwrap();let plan=cache.plan;let root=cache.graph.root.clone().unwrap();
        let keys=pages::coordinates(touched.intersect(plan.bounds),plan.level).map(|coordinate| {
            let region=pages::bounds(coordinate,plan.level,plan.extent).intersect(plan.bounds);(region,cache.page_key(root.clone(),region))
        }).filter(|(region,key)|*region==key.plan().bounds).collect::<Vec<_>>();
        assert!(keys.len()>=2,"{label}: the stroke covers whole cells of the window");
        let before=crate::test_support::PassCounts::snapshot();
        crate::test_support::submit_ready(&mut r,FramePacket {view:zoomed,..frame}).unwrap();
        crate::test_support::complete(&r);
        assert!(crate::test_support::PassCounts::snapshot().delta(before).written_back>=keys.len() as u64,"{label}: the zoom copies each stroked cell into the Store");
        assert!(r.scale_display.as_ref().unwrap().retired.is_none(),"{label}: the displaced window is released once its cells are copied");
        let retained=r.scale_display.as_ref().unwrap().pages.lock().unwrap().retained_pages();
        let mut written=Vec::new();
        for (region,key) in keys {
            let page=retained.iter().find(|page|page.key==key && !page.written.is_empty());
            assert!(page.is_some(),"{label}: the composed cell over {region:?} is written back to the Store when the window is displaced");
            written.push((region,key.plan(),pixels(&r,&page.unwrap().texture)));
        }
        crate::test_support::submit_settled(&mut r,frame);
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
                assert!(error<1e-5,"{label}: the written-back page over {region:?} differs from a keyed composition by {error}");
            }
        }
    }
}

#[test]
fn pen_up_keeps_the_cells_shown_earlier_in_the_contact_without_composing_or_copying_them() {
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
    let cache=r.scale_display.as_ref().unwrap();let plan=cache.plan;
    for coordinate in &earlier {
        assert!(cache.displays(pages::bounds(*coordinate,plan.level,plan.extent).intersect(plan.bounds)),"pen-up keeps the cell {coordinate:?} shown earlier in the contact");
    }
    let view_cells=pages::coordinates(plan.bounds,plan.level).count() as u64;
    assert_eq!(counts.written_back,0,"pen-up copies no cell into the Store: {counts:?}");
    assert!(counts.expected_keys<=6*view_cells,"pen-up keys stay bounded by the view, not the contact: {} keys for {view_cells} cells",counts.expected_keys);
}

pub(in crate::scene::scale) fn material_dab(x:f32,y:f32,style:&layer_render::DabStyle)->layer_render::Dab {
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

fn sized_dab([x,y]:[f32;2],radius:f32,style:&layer_render::DabStyle)->layer_render::Dab {
    let mut dab=material_dab(x,y,style);dab.radii=[radius;2];dab.previous=[radius,radius,1.,0.];dab.motion=[3.,0.];
    dab.material[3]=f32::from(style.execution==layer_core::BrushExecution::Liquify);dab
}
fn pen_dab(x:f32,style:&layer_render::DabStyle)->layer_render::Dab {sized_dab([x,300.],4.5,style)}
fn stroked_over_a_layer(extent:[u32;2],painted:bool)->layer_core::Document {
    let mut doc=document_at(extent);
    let paint=primary_paint(&doc);
    doc.artwork.paint.get_mut(paint).unwrap().base=painted.then(||layer_core::authored::PaintBase::new(rgba8_source(extent,|x,y|[(x/3) as u8,(y/2) as u8,80,((x*7+y*13)%256) as u8]).into()));
    let below=paint_occurrence(&mut doc,"Below",Some(rgba8_source(extent,|x,y|[(x/3) as u8,(y/2) as u8,140,255])));
    let working=doc.scene().position(doc.working.occurrence.unwrap()).unwrap();
    insert_occurrence(&mut doc,below,working+1);
    doc
}
const RECORD_ROW_SIZES:[f32;1]=[2000.];
const FRAME_ROW_SIZES:[(f32,usize);1]=[(600.,4)];
fn grayed_copy(doc:&layer_core::Document)->layer_core::Document {
    let mut doc=doc.clone();
    let paint=primary_paint(&doc);doc.artwork.paint.get_mut(paint).unwrap().color_mode=layer_core::color::LayerColorMode::Grayscale;doc.revision+=1;doc
}
fn celled(preset:DefaultBrushPreset,cell_size:u32)->layer_render::DabStyle {
    let style=crate::layer_tests::preset_style(preset);
    layer_render::DabStyle {blender:layer_core::BrushBlender {cell_size:cell_size as f32,..style.blender},..style}
}
fn sized_writers()->Vec<(String,layer_render::DabStyle,bool,usize,bool)> {
    use DefaultBrushPreset::*;
    let style=crate::layer_tests::preset_style;
    let mut writers=vec![("a Pencil".to_string(),style(Pencil),true,1,false),("a Pencil stored write-only".into(),style(Pencil),false,1,false),
        ("a smudge".into(),style(Smudge),true,0,false),("wet paint".into(),style(WetRound),true,0,false),("a wet-edged brush".into(),wet_edged_brush(),true,0,false),
        ("a clone stamp".into(),clone_stamp(),true,1,true),("a Liquify push".into(),style(LiquifyPush),true,0,true),("a Liquify twirl".into(),style(LiquifyTwirl),true,0,true)];
    for cell_size in [1,4,layer_core::BLENDER_CELL_SIZE_LIMIT] {
        writers.push((format!("a Dither blender with {cell_size} px cells"),celled(DitherBlender,cell_size),true,0,true));
        writers.push((format!("a Scatter blender with {cell_size} px cells"),celled(ScatterBlender,cell_size),true,0,true));
    }
    writers
}
fn swept_reach(size:f32)->f32 {(0.75*size).ceil()+1.}
fn sized_drawing(size:f32)->layer_core::Document {
    let reach=swept_reach(size) as u32;
    stroked_over_a_layer([540+reach,520+reach],true)
}
fn over_paper(mut doc:layer_core::Document,paper:bool)->layer_core::Document {
    if paper {
        let paper=doc.artwork.occurrences.iter().find(|(_,_,occurrence)|occurrence.kind()==LayerKind::Effect).unwrap().0;
        let below=doc.scene().order().len();
        insert_occurrence(&mut doc,paper,below);
    }
    doc
}
const STROKE:[(f32,f32);12]=[(480.,486.),(486.,492.),(492.,498.),(498.,504.),(504.,510.),(510.,516.),(516.,522.),(522.,516.),(522.,510.),(522.,510.),(522.,510.),(528.,534.)];
fn stroke_frame(r:&mut WgpuRasterizer,frame:FramePacket<'_>,target:SourceTarget,style:&layer_render::DabStyle,dabs:&[layer_render::Dab],inked:bool,step:usize)->crate::test_support::PassCounts {
    batched_frame(r,frame,target,style,dabs,usize::from(inked),step)
}
fn batched_frame(r:&mut WgpuRasterizer,frame:FramePacket<'_>,target:SourceTarget,style:&layer_render::DabStyle,dabs:&[layer_render::Dab],inked:usize,step:usize)->crate::test_support::PassCounts {
    let batches=dabs.iter().enumerate().map(|(index,dab)|layer_render::DabBatch {
        kind:if index<inked {layer_render::DabBatchKind::Persistent}else {layer_render::DabBatchKind::Preview},
        stroke_id:layer_core::StrokeId(1),first_dab:index as u32,stroke_start:inked>0 && step==0 && index==0,stroke_end:false,
        ..crate::test_support::dab_batch(target,style.clone(),dab.bounds())}).collect::<Vec<_>>();
    let counts=crate::test_support::PassCounts::snapshot();
    crate::test_support::submit_ready(r,FramePacket {dabs,dab_batches:&batches,time_seconds:0.1*(step+1) as f32,..frame}).unwrap();
    crate::test_support::PassCounts::snapshot().delta(counts)
}
fn clone_stamp()->layer_render::DabStyle {retouching(DefaultBrushPreset::CloneStamp)}
fn wet_edged_brush()->layer_render::DabStyle {
    let mut style=crate::tests::test_style(layer_core::BrushExecution::Dry);style.rendering.wet_edge=0.6;style
}
fn select_retouch(r:&mut WgpuRasterizer,target:SourceTarget,style:&layer_render::DabStyle) {
    let Some(retouch)=style.retouch.clone() else {return;};
    layer_render::CanvasRenderer::prepare_retouch(r,Some(&layer_render::RetouchPreparation {execution:style.execution,target,retouch,points:vec![layer_core::Point {x:300.,y:300.}]}));
}
fn bits(r:&WgpuRasterizer)->Vec<[u32;4]> {display_pixels(r).into_iter().map(|pixel|pixel.map(f32::to_bits)).collect()}
fn redraw_whole(r:&mut WgpuRasterizer,frame:FramePacket<'_>,target:SourceTarget,style:&layer_render::DabStyle,tips:&[layer_render::Dab],cells:&[PixelRect],step:usize)->u64 {
    let cache=r.scale_display.as_ref().unwrap();
    for cell in cells {cache.evict_window(*cell);}
    stroke_frame(r,frame,target,style,tips,false,step).composed_texels
}
struct Stroked<'a> {label:String,doc:&'a layer_core::Document,style:layer_render::DabStyle,tips:usize,in_place:bool,radius:f32,shift:[f32;2],frames:usize,display:wgpu::TextureFormat}
impl<'a> Stroked<'a> {
    fn new(label:impl Into<String>,doc:&'a layer_core::Document,preset:DefaultBrushPreset,tips:usize)->Self {
        Self {label:label.into(),doc,style:crate::layer_tests::preset_style(preset),tips,in_place:true,radius:4.5,shift:[0.;2],frames:STROKE.len(),display:wgpu::TextureFormat::Rgba16Float}
    }
    fn started(&self)->(WgpuRasterizer,FramePacket<'a>,SourceTarget) {
        let extent=self.doc.composition().size;
        let mut frame=packet(self.doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,1);
        let mut r=renderer(self.doc);let target=self.doc.active_target().unwrap();
        if !self.in_place {crate::test_support::write_only_dry_material(&mut r);}
        select_retouch(&mut r,target,&self.style);
        crate::test_support::submit_settled(&mut r,frame);
        assert_eq!(r.scale_display.as_ref().unwrap().pixels.root().unwrap().texture.format(),self.display,"{}: the window's format",self.label);
        (r,frame,target)
    }
    fn dabs(&self,(ink,tip):(f32,f32))->[layer_render::Dab;3] {[ink,tip,tip+9.].map(|x|sized_dab([x+self.shift[0],300.+self.shift[1]],self.radius,&self.style))}
    fn patched_and_whole_texels(&self,mut before:impl FnMut(usize,&mut WgpuRasterizer))->Vec<[u64;2]> {
        let Self {label,style,tips,frames,..}=self;
        let (mut r,frame,target)=self.started();
        let mut sequence=None;
        let texels=STROKE.into_iter().take(*frames).enumerate().map(|(step,at)| {
            before(step,&mut r);
            let dabs=self.dabs(at);
            let patched=stroke_frame(&mut r,frame,target,style,&dabs[..=*tips],true,step).composed_texels;
            let shown=bits(&r);
            let plan=r.scale_display.as_ref().unwrap().plan;
            let cells=r.metrics.frame_composited_pages.iter().map(|(_,coordinate)|pages::bounds(*coordinate,plan.level,plan.extent)).collect::<Vec<_>>();
            let whole=redraw_whole(&mut r,frame,target,style,&dabs[1..=*tips],&cells,step);
            let held=r.scale_display.as_ref().unwrap().contact_id();
            assert!(held.is_some() && (step==0 || held==sequence),"{label} frame {step}: the renderer keeps its stroke sequence");
            sequence=held;
            assert!(step==0 || bits(&r)==shown,"{label} frame {step}: a patched frame differs from a whole redraw of the same cells");
            [patched,whole]
        }).skip(1).collect();
        let last=self.dabs(STROKE[frames-1])[0];
        let lift=layer_render::DabBatch {stroke_id:layer_core::StrokeId(1),stroke_start:false,stroke_end:true,..crate::test_support::dab_batch(target,style.clone(),last.bounds())};
        crate::test_support::submit_settled(&mut r,FramePacket {dabs:std::slice::from_ref(&last),dab_batches:std::slice::from_ref(&lift),time_seconds:0.1*(frames+1) as f32,..frame});
        let settled=bits(&r);
        evict_view(&r);
        crate::test_support::submit_settled(&mut r,frame);
        assert!(bits(&r)==settled,"{label}: the picture settled after pen up differs from a whole redraw of the window");
        texels
    }
}
fn sums(texels:&[[u64;2]])->[u64;2] {[0,1].map(|renderer|texels.iter().map(|frame|frame[renderer]).sum())}

type PageReads=BTreeMap<[u32;2],(crate::resident::Input,Vec<[u32;4]>)>;
fn page_reads(r:&WgpuRasterizer,target:SourceTarget,preview:bool)->PageReads {
    let pages=if preview {&r.preview_pages}else {&r.paint_layers.iter().find(|layer|layer.id==target).unwrap().pages};
    pages.iter().filter_map(|page| {
        let read=r.read_color_page(target,page,preview,None).ok()?;
        Some((page.coordinate,(read.input(),crate::test_support::float_pixels(r,&read.value().texture).into_iter().map(|pixel|pixel.map(f32::to_bits)).collect())))
    }).collect()
}
fn differing_outside(rect:PixelRect,now:&[[u32;4]],then:&[[u32;4]])->usize {
    (0..PAGE_SIZE*PAGE_SIZE).filter(|index| {
        let [x,y]=[index%PAGE_SIZE,index/PAGE_SIZE];
        !(rect.min_x()..rect.max_x()).contains(&x) || !(rect.min_y()..rect.max_y()).contains(&y)
    }).filter(|index|now[*index as usize]!=then[*index as usize]).count()
}
struct Written {label:String,doc:layer_core::Document,style:layer_render::DabStyle,in_place:bool,tips:usize,radius:f32,at:[f32;2]}
impl Written {
    fn is_bounded_by_its_page_records(&self) {
        let Self {label,doc,style,in_place,tips,radius,at}=self;
        let extent=doc.composition().size;let target=doc.active_target().unwrap();
        let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,1);
        let mut r=renderer(doc);
        if !in_place {crate::test_support::write_only_dry_material(&mut r);}
        select_retouch(&mut r,target,style);
        crate::test_support::submit_settled(&mut r,frame);
        let dabs=|step:usize|[0.,3.,6.].map(|ahead|sized_dab([at[0]+3.*step as f32+ahead,at[1]],*radius,style));
        for step in 0..2 {stroke_frame(&mut r,frame,target,style,&dabs(step)[..=*tips],true,step);}
        let before=page_reads(&r,target,false);
        let drawn=dabs(2);
        let (ink,tips)=drawn[..=*tips].split_first().unwrap();
        stroke_frame(&mut r,frame,target,style,&drawn[..=tips.len()],true,2);
        let written=page_reads(&r,target,false);
        let grid=extent.map(|side|side.next_multiple_of(PAGE_SIZE));
        let cells=if crate::destination_material::blender(style).cells {style.blender.cell_size as u32-1}else {0};
        let reached=|dab:&layer_render::Dab,coordinate:[u32;2]| {
            let page=page_rect(coordinate);crate::pixel_rect::pixel_rect(dab.bounds(),grid).expand(cells,grid).intersect(page).window_local(page)
        };
        let past_the_edge=|coordinate:[u32;2]|page_rect(coordinate).intersect(PixelRect::full(extent))!=page_rect(coordinate);
        let mut partial=0;
        for (coordinate,(page,now)) in &written {
            let Some((earlier,then))=before.get(coordinate).filter(|(earlier,_)|!page.same(earlier)) else {continue;};
            let Some(rect)=page.changed_since(earlier).or_else(||page.seed().filter(|(from,_)|from.same(earlier)).map(|(_,rect)|rect)) else {
                assert!(style.blender.kind==layer_core::BlenderKind::Scatter && past_the_edge(*coordinate),"{label}: the stroke's write to page {coordinate:?} is recorded against the version before the frame");
                continue;
            };
            let reach=reached(ink,*coordinate);
            assert!(!rect.is_empty() && rect.union(reach)==reach,"{label}: the record of page {coordinate:?} names no more than its dab reaches: {rect:?} in {reach:?}");
            assert_eq!(differing_outside(rect,now,then),0,"{label}: page {coordinate:?} changed outside its recorded rectangle {rect:?}");
            partial+=usize::from(rect.area()<u64::from(PAGE_SIZE).pow(2));
        }
        assert!(partial>0,"{label}: a record names less than its page");
        if tips.is_empty() {return;}
        let tipped=page_reads(&r,target,true);
        assert!(!tipped.is_empty(),"{label}: the tip is drawn");
        for (coordinate,(page,shown)) in &tipped {
            let (under,now)=&written[coordinate];
            let (from,rect)=page.seed().unwrap_or_else(||panic!("{label}: the tip's write to page {coordinate:?} is recorded"));
            let reach=tips.iter().fold(PixelRect::EMPTY,|reach,tip|reach.union(reached(tip,*coordinate)));
            assert!(from.same(under) && rect.union(reach)==reach,"{label}: the tip on page {coordinate:?} is recorded as a copy of the page it was drawn over: {rect:?} in {reach:?}");
            assert_eq!(differing_outside(rect,shown,now),0,"{label}: the tip differs from page {coordinate:?} outside its recorded rectangle {rect:?}");
        }
    }
}

#[test]
fn each_page_record_bounds_what_its_stroke_frame_wrote() {
    use DefaultBrushPreset::*;
    let doc=stroked_over_a_layer([768,512],true);
    let style=crate::layer_tests::preset_style;
    let locked=layer_render::DabStyle {alpha_locked:true,..style(Pencil)};
    let colored=[("a Pencil",style(Pencil),true,1),("an alpha-locked Pencil",locked,true,1),("an ink pen",style(GPen),true,1),
        ("an eraser",style(Eraser),true,1),("a paintbrush",style(Paintbrush),true,1),("a multiply glaze",style(MultiplyGlaze),true,1),
        ("a Pencil stored write-only",style(Pencil),false,1),("an ink pen stored write-only",style(GPen),false,1),("a smudge",style(Smudge),true,0),("wet paint",style(WetRound),true,0),
        ("a wet-edged brush",wet_edged_brush(),true,0)];
    let grayed=[("a Dither blender",style(DitherBlender),true,0),("a Dither blender without cells",celled(DitherBlender,1),true,0),("a Scatter blender",style(ScatterBlender),true,0),
        ("a clone stamp",clone_stamp(),true,1),("a clone stamp stored write-only",clone_stamp(),false,1),("a Liquify push",style(LiquifyPush),true,0),
        ("a smudge under a tip",style(Smudge),true,1),("a smudge under two tips",style(Smudge),true,2),("a soft blender under a tip",style(SoftBlender),true,1),
        ("a Dither blender under a tip",style(DitherBlender),true,1),("a Dither blender without cells under a tip",celled(DitherBlender,1),true,1),("a Scatter blender under a tip",style(ScatterBlender),true,1),
        ("a Scatter blender under two tips",style(ScatterBlender),true,2),("a Pencil under two tips",style(Pencil),true,2),
        ("a Pencil stored write-only under two tips",style(Pencil),false,2),("a wet-edged brush under a tip",wet_edged_brush(),true,1),
        ("a Liquify push under a tip",style(LiquifyPush),true,1),("a Liquify twirl under a tip",style(LiquifyTwirl),true,1),
        ("a Liquify crystals brush under a tip",style(LiquifyCrystals),true,1)];
    for (gray,(label,style,in_place,tips)) in colored.into_iter().map(|row|(false,row)).chain(grayed.into_iter().map(|row|(true,row))) {
        Written {label:label.into(),doc:if gray {grayed_copy(&doc)}else {doc.clone()},style,in_place,tips,radius:4.5,at:[300.;2]}.is_bounded_by_its_page_records();
    }
    for size in RECORD_ROW_SIZES {
        let doc=sized_drawing(size);
        for (label,style,in_place,tips,gray) in sized_writers() {
            Written {label:format!("{label} of {size} px"),doc:if gray {grayed_copy(&doc)}else {doc.clone()},style,in_place,tips,radius:size/2.,at:[480.+swept_reach(size),300.+swept_reach(size)]}
                .is_bounded_by_its_page_records();
        }
    }
}

#[test]
fn each_page_record_bounds_what_its_frame_wrote_at_every_brush_size() {
    use DefaultBrushPreset::*;
    let extent=[768,512];let doc=grayed_copy(&stroked_over_a_layer(extent,true));let target=doc.active_target().unwrap();
    let style=crate::layer_tests::preset_style;
    let page=u64::from(PAGE_SIZE).pow(2);
    let read=|r:&WgpuRasterizer,preview:bool|page_reads(r,target,preview).remove(&[1,1]).expect("the stroke's page is stored");
    for (label,style,inked,tips) in [("a smudge under a tip",style(Smudge),1,1),("a smudge in three batches under two tips",style(Smudge),3,2),
        ("a smudge in two batches under three tips",style(Smudge),2,3),("a Dither blender under a tip",style(DitherBlender),1,1),
        ("a Scatter blender in two batches under a tip",style(ScatterBlender),2,1),("a Liquify push under a tip",style(LiquifyPush),1,1)] {
        for diameter in [5.,30.,100.,300.,600.,2000.] {
            let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,1);
            let mut r=renderer(&doc);
            crate::test_support::submit_settled(&mut r,frame);
            let reach=if diameter>600. {diameter*0.75}else {diameter/2.};
            let dabs=|step:usize|(0..inked+tips).map(|index|sized_dab([300.-reach+3.*(step*inked+index) as f32,300.],diameter/2.,&style)).collect::<Vec<_>>();
            for step in 0..2 {batched_frame(&mut r,frame,target,&style,&dabs(step),inked,step);}
            let (before,then)=read(&r,false);
            batched_frame(&mut r,frame,target,&style,&dabs(2),inked,2);
            let (written,now)=read(&r,false);
            let rect=written.changed_since(&before).unwrap_or_else(|| {
                let (from,rect)=written.seed().unwrap_or_else(||panic!("{label}, {diameter} px: the stroke's write is recorded"));
                assert!(from.same(&before),"{label}, {diameter} px: the page is recorded as a copy of the version before the frame");rect
            });
            assert!(!rect.is_empty() && rect.area()<page,"{label}, {diameter} px: the record names part of the page: {rect:?}");
            assert_eq!(differing_outside(rect,&now,&then),0,"{label}, {diameter} px: the page changed outside its recorded rectangle {rect:?}");
            let (tip,shown)=read(&r,true);
            let (from,rect)=tip.seed().unwrap_or_else(||panic!("{label}, {diameter} px: the tip's write is recorded"));
            assert!(from.same(&written) && !rect.is_empty() && rect.area()<page,"{label}, {diameter} px: the tip is recorded as a copy of the page it was drawn over outside part of it: {rect:?}");
            assert_eq!(differing_outside(rect,&shown,&now),0,"{label}, {diameter} px: the tip differs from the page outside its recorded rectangle {rect:?}");
        }
    }
}

fn patches_bit_for_bit<'a>(strokes:impl IntoIterator<Item=Stroked<'a>>) {
    for stroked in strokes {
        let texels=sums(&stroked.patched_and_whole_texels(|_,_|{}));
        assert!(texels[0]<texels[1],"{}: the patched renderer composes less than the whole one: {texels:?}",stroked.label);
    }
}
fn stays_whole<'a>(strokes:impl IntoIterator<Item=Stroked<'a>>) {
    for stroked in strokes {
        let texels=stroked.patched_and_whole_texels(|_,_|{});
        assert!(texels.iter().all(|[patched,whole]|patched==whole),"{}: every cell is composed whole: {texels:?}",stroked.label);
    }
}

fn patches_bit_for_bit_for_each_kind_of_page_record(paper:bool) {
    use DefaultBrushPreset::*;
    let display=if paper {wgpu::TextureFormat::Rgba32Float}else {wgpu::TextureFormat::Rgba16Float};
    let [painted,unpainted]=[true,false].map(|painted|over_paper(stroked_over_a_layer([768,512],painted),paper));
    patches_bit_for_bit([
        Stroked::new("a page written in place under a tip",&painted,Pencil,1),
        Stroked {in_place:false,..Stroked::new("a page copied to its other surface under a tip",&painted,Pencil,1)},
        Stroked::new("a page transported without a tip",&painted,Smudge,0),
        Stroked::new("a page transported under a tip",&painted,Smudge,1),
        Stroked::new("a page transported under two tips",&painted,Smudge,2),
        Stroked::new("a page written in place under two tips",&painted,Pencil,2),
        Stroked::new("a page a Dither blender rewrote in cells under a tip",&painted,DitherBlender,1),
        Stroked::new("a page a Scatter blender rewrote in cells under two tips",&painted,ScatterBlender,2),
        Stroked::new("a page that first exists mid-stroke",&unpainted,Pencil,1),
        Stroked::new("a page a Dither blender rewrote in cells",&painted,DitherBlender,0),
        Stroked::new("a page a Scatter blender rewrote in cells",&painted,ScatterBlender,0),
        Stroked {style:clone_stamp(),..Stroked::new("a page a clone stamp copied onto under its tip",&painted,CloneStamp,1)},
        Stroked::new("a page Liquify pushed in place without a tip",&painted,LiquifyPush,0),
        Stroked::new("a page Liquify pushed in place under a tip",&painted,LiquifyPush,1),
        Stroked::new("a page Liquify twirled in place under a tip",&painted,LiquifyTwirl,1),
        Stroked {style:wet_edged_brush(),..Stroked::new("a page a wet-edged brush drew on in place without a tip",&painted,Pencil,0)},
    ].map(|stroked|Stroked {display,..stroked}));
    for (size,frames) in FRAME_ROW_SIZES {
        let doc=over_paper(sized_drawing(size),paper);
        patches_bit_for_bit(sized_writers().into_iter().map(|(label,style,in_place,tips,_)|Stroked {style,in_place,radius:size/2.,shift:[swept_reach(size);2],frames,display,
            ..Stroked::new(format!("{label} of {size} px shown as {display:?}"),&doc,Pencil,tips)}));
    }
}

#[test]
fn every_patched_frame_equals_a_whole_frame_bit_for_bit_for_each_kind_of_page_record() {
    patches_bit_for_bit_for_each_kind_of_page_record(false);
}

#[test]
fn every_patched_frame_over_paper_equals_a_whole_frame_bit_for_bit_for_each_kind_of_page_record() {
    patches_bit_for_bit_for_each_kind_of_page_record(true);
}

#[test]
fn every_patched_frame_equals_a_whole_frame_bit_for_bit_for_each_layer_setup() {
    use DefaultBrushPreset::Pencil;
    let painted=stroked_over_a_layer([768,512],true);let partial=stroked_over_a_layer([540,500],true);
    let working=|doc:&layer_core::Document|doc.working.occurrence.unwrap();
    let mut float=painted.clone();composition_mut(&mut float).color.depth=SampleDepth::F32;
    let mut covered=painted.clone();
    let above=paint_occurrence(&mut covered,"Above",Some(rgba8_source([768,512],|x,y|[200,(x/4) as u8,(y/3) as u8,((x+y)%200) as u8])));
    let position=covered.scene().position(working(&covered)).unwrap();
    insert_occurrence(&mut covered,above,position);
    let mut clipped=painted.clone();
    let owner=working(&clipped);set_attachment(clipped.artwork.occurrences.get_mut(owner).unwrap(),true);reindex(&mut clipped);
    let mut grouped=painted.clone();group_working_layer(&mut grouped);
    let mut faded=painted.clone();
    let owner=working(&faded);
    let occurrence=faded.artwork.occurrences.get_mut(owner).unwrap();occurrence.opacity=0.6;occurrence.blend=layer_core::LayerBlend::Multiply;
    reindex(&mut faded);
    patches_bit_for_bit([
        Stroked::new("partial cells at the drawing's edge",&partial,Pencil,1),Stroked {display:wgpu::TextureFormat::Rgba32Float,..Stroked::new("a float drawing",&float,Pencil,1)},
        Stroked::new("a layer above",&covered,Pencil,1),Stroked::new("a clipped layer",&clipped,Pencil,1),
        Stroked::new("a faded multiply layer",&faded,Pencil,1),Stroked::new("a layer in a group",&grouped,Pencil,1),
    ]);
}

#[test]
fn a_write_without_a_usable_record_composes_whole_cells_and_the_same_pixels() {
    use DefaultBrushPreset::*;
    let plain=stroked_over_a_layer([768,512],true);
    stays_whole([Stroked::new("a watercolor wash",&plain,WatercolorWash,0),Stroked::new("a watercolor wash under a tip",&plain,WatercolorWash,1)]);
}

#[test]
fn a_cell_drawn_by_more_than_whole_cell_draws_composes_whole_cells_and_the_same_pixels() {
    use DefaultBrushPreset::Pencil;
    let plain=stroked_over_a_layer([768,512],true);
    let working=|doc:&layer_core::Document|doc.working.occurrence.unwrap();
    let mut offset=plain.clone();
    let owner=working(&offset);offset.artwork.occurrences.get_mut(owner).unwrap().offset=[3,5];
    let mut masked=plain.clone();
    let owner=working(&masked);
    coverage_mask(&mut masked,owner,[0,0],Some(layer_core::Selection::polygon(vec![
        layer_core::Point {x:100.,y:100.},layer_core::Point {x:700.,y:120.},layer_core::Point {x:680.,y:480.},layer_core::Point {x:120.,y:460.}]).unwrap()));
    let mut covered=plain.clone();
    let owner=working(&covered);coverage_mask(&mut covered,owner,[0,0],None);
    let above=|name:&str| {
        let mut doc=plain.clone();
        let effect=effect_occurrence(&mut doc,layer_core::EffectInstance::new(crate::tests::fixture(name).program()),name);
        insert_occurrence(&mut doc,effect,0);doc
    };
    let [blurred,inverted]=["gaussian_blur","invert"].map(above);
    stays_whole([Stroked::new("an offset layer",&offset,Pencil,1),Stroked::new("a masked layer",&masked,Pencil,1),Stroked::new("a layer under a mask that covers it",&covered,Pencil,1),
        Stroked {display:wgpu::TextureFormat::Rgba32Float,..Stroked::new("a blur above",&blurred,Pencil,1)},
        Stroked {display:wgpu::TextureFormat::Rgba32Float,..Stroked::new("an adjustment above",&inverted,Pencil,1)}]);
}

#[test]
fn a_cell_whose_cached_inputs_were_rebuilt_or_whose_record_was_dropped_is_composed_whole_once() {
    let doc=stroked_over_a_layer([768,512],true);
    let cell=PixelRect::new(256,256,512,512);
    let stroked=Stroked::new("a Pencil",&doc,DefaultBrushPreset::Pencil,1);
    let plain=stroked.patched_and_whole_texels(|_,_|{});
    assert!(plain[1][0]<plain[1][1],"the third frame patches when nothing is lost: {plain:?}");
    let rebuilt=stroked.patched_and_whole_texels(|step,r|if step==2 {evict_composite_pages(r.scale_display.as_ref().unwrap(),&[cell]);});
    assert_eq!(rebuilt[1][0],rebuilt[1][1],"a cell whose cached inputs were rebuilt composes whole: {rebuilt:?}");
    assert!(rebuilt[2][0]<rebuilt[2][1],"the cell patches again from its new baseline: {rebuilt:?}");
    let dropped=stroked.patched_and_whole_texels(|step,r|if step==2 {r.scale_display.as_ref().unwrap().evict_window(cell);});
    assert_eq!(dropped[1][0],dropped[1][1],"a cell without its record composes whole: {dropped:?}");
    assert!(dropped[2][0]<dropped[2][1],"the cell patches again from its new baseline: {dropped:?}");
}

#[test]
fn a_chain_of_patched_frames_ends_equal_to_a_whole_redraw_of_every_cell_the_sequence_drew() {
    let doc=stroked_over_a_layer([768,512],true);
    for in_place in [true,false] {
        let stroked=Stroked {in_place,..Stroked::new("a Pencil",&doc,DefaultBrushPreset::Pencil,1)};
        let (mut r,frame,target)=stroked.started();
        let (mut texels,mut cells)=(Vec::new(),BTreeSet::new());
        for (step,at) in STROKE.into_iter().enumerate() {
            texels.push(stroke_frame(&mut r,frame,target,&stroked.style,&stroked.dabs(at)[..=1],true,step).composed_texels);
            if step>0 {cells.extend(r.metrics.frame_composited_pages.iter().map(|(_,coordinate)|*coordinate));}
        }
        let shown=bits(&r);
        let plan=r.scale_display.as_ref().unwrap().plan;
        let cells=cells.into_iter().map(|coordinate|pages::bounds(coordinate,plan.level,plan.extent)).collect::<Vec<_>>();
        let whole=redraw_whole(&mut r,frame,target,&stroked.style,&stroked.dabs(STROKE[11])[1..=1],&cells,12);
        assert!(r.scale_display.as_ref().unwrap().contact_id().is_some() && texels[2..].iter().filter(|texels|**texels<whole/8).count()>=6,"in place {in_place}: the stroke stays one sequence and patches: {texels:?} against {whole}");
        let after=bits(&r);
        let differing=shown.iter().zip(&after).enumerate().filter(|(_,(a,b))|a!=b).map(|(index,_)|[index as u32%768,index as u32/768]).collect::<Vec<_>>();
        assert!(differing.is_empty(),"in place {in_place}: a chain of patched frames differs from a whole redraw at {} pixels, from {:?} to {:?}, cells {cells:?}",differing.len(),differing.first(),differing.last());
    }
}

#[test]
fn a_stroke_that_starts_and_ends_in_one_frame_forms_no_sequence_and_settles() {
    let doc=stroked_over_a_layer([768,512],true);let extent=doc.composition().size;let target=doc.active_target().unwrap();
    let style=crate::layer_tests::preset_style(DefaultBrushPreset::Pencil);
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,1);
    let mut r=renderer(&doc);
    crate::test_support::submit_settled(&mut r,frame);
    let dab=pen_dab(300.,&style);
    let batch=layer_render::DabBatch {stroke_start:true,stroke_end:true,..crate::test_support::dab_batch(target,style.clone(),dab.bounds())};
    let before=crate::test_support::PassCounts::snapshot();
    crate::test_support::submit_settled(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),time_seconds:0.1,..frame});
    assert!(r.scale_display.as_ref().unwrap().contact_id().is_none() && r.next_frame()!=layer_render::FrameDemand::Work,"a one-frame stroke leaves no sequence and no frame owed");
    assert_eq!(crate::test_support::PassCounts::snapshot().delta(before).written_back,0,"a one-frame stroke copies no cell into the Store");
}

#[test]
fn a_stroke_inside_one_cell_composes_whole_cells_twice_and_then_only_its_patch() {
    let extent=[768,512];let doc=stroked_over_a_layer(extent,true);let target=doc.active_target().unwrap();
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.view=view_at(extent,1);
    let cell=u64::from(PAGE_SIZE).pow(2);
    for (preset,tips) in [(DefaultBrushPreset::Pencil,1),(DefaultBrushPreset::GPen,1),(DefaultBrushPreset::Smudge,0)] {
        let style=crate::layer_tests::preset_style(preset);
        let mut r=renderer(&doc);
        crate::test_support::submit_settled(&mut r,frame);
        let mut whole=0;
        for step in 0..8 {
            let x=300.+3.*step as f32;let dabs=[pen_dab(x,&style),pen_dab(x+3.,&style)];
            let texels=stroke_frame(&mut r,frame,target,&style,&dabs[..=tips],true,step).composed_texels;
            if step==0 {whole=texels;assert!(whole>=cell && whole%cell==0,"{preset:?} with {tips} tips: the first frame composes whole cells: {texels}");continue;}
            if step==1 {assert_eq!(texels,whole,"{preset:?} with {tips} tips: the second frame composes whole cells");continue;}
            let reach=[pen_dab(x-3.,&style),dabs[0],dabs[1]].iter().map(|dab|crate::pixel_rect::pixel_rect(dab.bounds(),extent)).reduce(PixelRect::union).unwrap();
            assert!(texels<=whole/cell*reach.area(),"{preset:?} with {tips} tips, frame {step}: {texels} texels composed for a {reach:?} patch");
        }
    }
}

fn group_working_layer(doc:&mut Document) {
    let owner=doc.working.occurrence.unwrap();
    let mut entries=doc.artwork.stacks.get(doc.composition().result).unwrap().entries.clone();
    let group=stack_occurrence(doc,"Group",vec![owner]);
    doc.artwork.occurrences.get_mut(group).unwrap().opacity=0.7;
    for entry in &mut entries {if *entry==owner {*entry=group;}}
    set_root_entries(doc,entries);
}
const OFFSETS:[(&str,[i64;2]);3]=[("a layer offset by 3 and 5",[3,5]),("a layer offset by 259 and -5",[259,-5]),("a layer offset by -130 and 77",[-130,77])];
fn zoomed_scene(scene:&str,extent:[u32;2])->Document {
    let image=|seed:u32|rgba8_source(extent,move |x,y|[(x/(3+seed)) as u8,(y/2) as u8,(60*seed) as u8,((x*7+y*(13+seed))%256) as u8]);
    let mut doc=if scene.ends_with("without Paper") {document_at(extent)}else {
        Document::new(PortableId::random(),extent[0],extent[1],layer_core::DocumentNames {paint:"Current ink".into(),paper:"Paper".into()})
    };
    let owner=doc.working.occurrence.unwrap();
    let paint=primary_paint(&doc);
    doc.artwork.paint.get_mut(paint).unwrap().base=(scene!="a blank layer").then(||PaintBase::new(image(0).into()));
    let above=|doc:&mut Document,name:&str| {
        let effect=effect_occurrence(doc,layer_core::EffectInstance::new(crate::tests::fixture(name).program()),name);
        insert_occurrence(doc,effect,0);
    };
    match scene {
        "the middle layer of five"=>for (index,name) in ["Top","Upper","Lower","Bottom"].into_iter().enumerate() {
            let layer=paint_occurrence(&mut doc,name,Some(image(index as u32+1)));
            let position=doc.scene().position(owner).unwrap();
            insert_occurrence(&mut doc,layer,position+index/2);
        },
        "a masked layer"|"a masked layer without Paper"=>{
            let [width,height]=extent.map(|side|side as f32);
            coverage_mask(&mut doc,owner,[0,0],Some(layer_core::Selection::polygon(vec![layer_core::Point {x:width*0.1,y:height*0.1},
                layer_core::Point {x:width*0.95,y:height*0.15},layer_core::Point {x:width*0.9,y:height*0.95},layer_core::Point {x:width*0.15,y:height*0.9}]).unwrap()));
        },
        "a layer under an adjustment"=>above(&mut doc,"invert"),
        "a layer under a blur"=>above(&mut doc,"gaussian_blur"),
        "a layer in a group"=>group_working_layer(&mut doc),
        _=>if let Some((_,offset))=OFFSETS.into_iter().find(|(name,_)|*name==scene) {doc.artwork.occurrences.get_mut(owner).unwrap().offset=offset;},
    }
    doc
}
struct Zoomed<'a> {scene:&'a str,brush:String,style:layer_render::DabStyle,zoom:f32,diameter:f32,tips:usize,frames:usize,redraws:bool}
struct Redrawn {case:String,level:u32,float:bool,restarts:usize,texels:Vec<[u64;2]>}
impl Redrawn {
    fn patched(&self)->usize {self.texels[2..].iter().filter(|[patched,whole]|patched<whole).count()}
    fn steady(&self)->[u64;2] {[0,1].map(|renderer|self.texels[2..].iter().map(|frame|frame[renderer]).sum::<u64>()/(self.texels.len() as u64-2))}
    fn row(&self)->String {
        let [patched,whole]=self.steady();
        format!("| {} | {} | {} | {} of {} | {patched} | {whole} | {} |",self.case,self.level,if self.float {"float"}else {"half-float"},self.patched(),self.texels.len()-2,self.restarts)
    }
}
impl<'a> Zoomed<'a> {
    fn new(scene:&'a str,brush:&str,style:layer_render::DabStyle,tips:usize,zoom:f32)->Self {Self {scene,brush:brush.into(),style,zoom,diameter:9.,tips,frames:STROKE.len(),redraws:true}}
    fn level(&self)->u32 {(1./self.zoom).log2().floor() as u32}
    fn at(&self,x:f32)->layer_render::Dab {
        let [left,top]=OFFSETS.into_iter().find(|(name,_)|*name==self.scene).map_or([0.;2],|(_,offset)|offset.map(|side|side as f32));
        sized_dab([(x-256.)*(1u32<<self.level()) as f32-left,257.-top],self.diameter/2.,&self.style)
    }
    fn case(&self)->String {format!("{}, {} px, {} tips, on {} at {}%",self.brush,self.diameter,self.tips,self.scene,self.zoom*100.)}
    fn document(&self)->Document {zoomed_scene(self.scene,[320<<self.level(),140<<self.level()])}
    fn frame<'d>(&self,doc:&'d Document)->FramePacket<'d> {
        let extent=doc.composition().size;
        let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
        frame.view=crate::test_support::view(extent.map(|side|(side as f32*self.zoom).ceil() as u32));
        frame.view.document_to_surface=[self.zoom,0.,0.,self.zoom,0.,0.];frame
    }
    fn settled(&self,doc:&Document,frame:FramePacket<'_>)->WgpuRasterizer {
        let mut r=renderer(doc);
        select_retouch(&mut r,doc.active_target().unwrap(),&self.style);crate::test_support::submit_settled(&mut r,frame);r
    }
    fn level_pages_against_a_full_reduction(&self,frames:usize)->Option<([usize;2],f32,bool)> {
        let (doc,level)=(self.document(),self.level());let (frame,target)=(self.frame(&doc),doc.active_target().unwrap());
        let mut r=self.settled(&doc,frame);
        let mut tips=Vec::new();
        for (step,(ink,tip)) in STROKE.into_iter().take(frames).enumerate() {
            let dabs=[ink,tip,tip+9.].map(|x|self.at(x));
            stroke_frame(&mut r,frame,target,&self.style,&dabs[..=self.tips],true,step);
            tips=dabs[1..=self.tips].to_vec();
        }
        let compact=r.preview_level>0 || r.preview_contribution;
        let held=r.scale_display.as_ref().unwrap().gesture.as_ref()?.pages.iter().filter(|(key,_)|key.source==Some(target) && key.level==level)
            .map(|(key,page)|(key.clone(),page.clone())).collect::<Vec<_>>();
        let texels=|r:&WgpuRasterizer,page:&pages::Held|pixels(r,&page.texture).into_iter().map(|texel|texel.map(f32::to_bits)).collect::<Vec<_>>();
        let reduced=held.iter().map(|(_,page)|texels(&r,page)).collect::<Vec<_>>();
        let batches=tips.iter().enumerate().map(|(index,dab)|layer_render::DabBatch {kind:layer_render::DabBatchKind::Preview,stroke_id:layer_core::StrokeId(1),
            first_dab:index as u32,stroke_start:false,stroke_end:false,..crate::test_support::dab_batch(target,self.style.clone(),dab.bounds())}).collect::<Vec<_>>();
        let whole=FramePacket {dabs:&tips,dab_batches:&batches,composite_all:true,time_seconds:3.,..frame};
        let tip=crate::brush_tiles::plan_packet(whole,|target|r.target_extent(target)).unwrap().iter().flatten().map(Damage::tile_region).collect::<Vec<_>>();
        crate::test_support::submit_ready(&mut r,whole).unwrap();
        let cache=r.scale_display.as_ref().unwrap();
        {let mut store=cache.pages.lock().unwrap();for (key,_) in &held {store.forget(key,key.plan().bounds);}}
        cache.evict_window(cache.plan.bounds);
        crate::test_support::submit_ready(&mut r,FramePacket {time_seconds:3.1,..whole}).unwrap();
        let (mut differing,mut largest,side)=([0;2],0f32,1u32<<level);
        for ((_,page),reduced) in held.iter().zip(&reduced) {
            let (again,width,bounds)=(texels(&r,page),page.texture.width(),page.plan.bounds);
            for y in 0..page.plan.size[1] {for x in 0..page.plan.size[0] {
                let index=(y*width+x) as usize;
                if reduced[index]==again[index] {continue;}
                let texel=PixelRect::new(bounds.min_x()+x*side,bounds.min_y()+y*side,bounds.min_x()+(x+1)*side,bounds.min_y()+(y+1)*side);
                differing[usize::from(tip.iter().any(|rect|!rect.intersect(texel).is_empty()))]+=1;
                largest=reduced[index].iter().zip(&again[index]).map(|(a,b)|(f32::from_bits(*a)-f32::from_bits(*b)).abs()).fold(largest,f32::max);
            }}
        }
        Some((differing,largest,compact))
    }
    fn retraces_like_a_renderer_that_never_patches(&self)->[u64;2] {
        let (case,level,doc)=(self.case(),self.level(),self.document());let (frame,target)=(self.frame(&doc),doc.active_target().unwrap());
        let [mut patched,mut whole]=[self.settled(&doc,frame),self.settled(&doc,frame)];
        let cache=patched.scale_display.as_ref().unwrap();
        let (plan,float)=(cache.plan,cache.texture().format()==wgpu::TextureFormat::Rgba32Float);
        let same=|patched:&WgpuRasterizer,whole:&WgpuRasterizer| {
            let (patched,whole)=(display_pixels(patched),display_pixels(whole));
            let step=|value:f32|i32::from(layer_core::color::f16::from_f32(value).to_bits());
            patched.iter().zip(&whole).all(|(a,b)|a.iter().zip(b).all(|(a,b)|if float {a.to_bits()==b.to_bits()}else {(step(*a)-step(*b)).abs()<=1}))
        };
        let sequence=|r:&WgpuRasterizer|r.scale_display.as_ref().unwrap().contact_id();
        let composed=|r:&WgpuRasterizer|r.metrics.frame_composited_pages.iter().flat_map(|(_,tile)|pages::coordinates(page_rect(*tile),level)).collect::<BTreeSet<_>>();
        let (mut held,mut drawn,mut compared,mut texels)=([None;2],BTreeSet::new(),0,[0;2]);
        for (step,(ink,tip)) in RETRACE.into_iter().enumerate() {
            let dabs=[self.at(ink),self.at(tip)];
            texels[0]+=stroke_frame(&mut patched,frame,target,&self.style,&dabs,true,step).composed_texels;
            let cache=whole.scale_display.as_ref().unwrap();
            for cell in composed(&patched).intersection(&drawn) {cache.evict_window(pages::bounds(*cell,level,plan.extent));}
            texels[1]+=stroke_frame(&mut whole,frame,target,&self.style,&dabs,true,step).composed_texels;
            let now=[sequence(&patched),sequence(&whole)];
            if now[1].is_none() || now[1]!=held[1] {drawn.clear();}else {drawn.extend(composed(&whole));}
            let continued=now[0].is_some() && now[0]==held[0];
            held=now;
            if !continued {continue;}
            compared+=1;
            for _ in 0..3 {
                if same(&patched,&whole) {break;}
                stroke_frame(&mut whole,frame,target,&self.style,&dabs[1..],false,step);
            }
            assert!(same(&patched,&whole),"{case}, frame {step} of a retraced stroke: the display differs from a renderer that composes whole cells");
        }
        let last=self.at(474.);
        let lift=layer_render::DabBatch {kind:layer_render::DabBatchKind::Persistent,stroke_id:layer_core::StrokeId(1),stroke_end:true,
            ..crate::test_support::dab_batch(target,self.style.clone(),last.bounds())};
        for r in [&mut patched,&mut whole] {
            crate::test_support::submit_settled(r,FramePacket {dabs:std::slice::from_ref(&last),dab_batches:std::slice::from_ref(&lift),time_seconds:2.,..frame});
            crate::test_support::submit_settled(r,frame);
        }
        assert!(same(&patched,&whole),"{case}: after pen up the display differs from a renderer that composed whole cells");
        assert!(compared+3>=RETRACE.len(),"{case}: the stroke kept its sequence in only {compared} frames");
        println!("| {case}, retraced | {level} | {} | {compared} of {} frames compared | {} | {} |",if float {"float"}else {"half-float"},RETRACE.len(),texels[0],texels[1]);
        texels
    }
    fn redrawn(&self)->Redrawn {
        let Self {style,tips,frames,redraws,..}=self;
        let (case,level,doc)=(self.case(),self.level(),self.document());let (frame,target)=(self.frame(&doc),doc.active_target().unwrap());
        let mut r=self.settled(&doc,frame);
        let cache=r.scale_display.as_ref().unwrap();
        let (plan,float)=(cache.plan,cache.texture().format()==wgpu::TextureFormat::Rgba32Float);
        assert_eq!(plan.level,level,"{case}: the view's level");
        let sequence=|r:&WgpuRasterizer|r.scale_display.as_ref().unwrap().contact_id();
        let mut restarts=0;
        let texels=STROKE.into_iter().take(*frames).enumerate().map(|(step,(ink,tip))| {
            let dabs=[ink,tip,tip+9.].map(|x|self.at(x));
            let before=sequence(&r);
            let patched=stroke_frame(&mut r,frame,target,style,&dabs[..=*tips],true,step).composed_texels;
            let (shown,held)=(bits(&r),sequence(&r));
            let tiles=r.metrics.frame_composited_pages.iter().map(|(_,tile)|*tile).collect::<BTreeSet<_>>();
            let cells=tiles.iter().flat_map(|tile|pages::coordinates(page_rect(*tile),level)).collect::<BTreeSet<_>>()
                .into_iter().map(|cell|pages::bounds(cell,level,plan.extent)).collect::<Vec<_>>();
            let narrowed=cells.iter().copied().filter(|cell|page_coordinates(*cell).any(|tile|!tiles.contains(&tile))).collect::<Vec<_>>();
            if !redraws {assert!(narrowed.is_empty(),"{case}, frame {step}: every cell is composed whole, not {narrowed:?}");return [patched,patched];}
            let whole=redraw_whole(&mut r,frame,target,style,&dabs[1..=*tips],&cells,step);
            if step==0 || held.is_none() || held!=before || sequence(&r)!=held {restarts+=usize::from(step>0);return [whole,whole];}
            let redrawn=bits(&r);
            assert_eq!(shown.len(),(plan.size[0]*plan.size[1]) as usize,"{case}: the display holds the window's texels");
            let differing=narrowed.iter().flat_map(|cell| {
                let [x,y,width,height]=paint_transform::texel_rect(cell.window_local(plan.bounds),1<<level);
                (y..y+height).flat_map(move |row|(x..x+width).map(move |column|(row*plan.size[0]+column) as usize))
            }).filter(|index|shown[*index]!=redrawn[*index]).count();
            assert_eq!(differing,0,"{case}, frame {step}: the cells composed in part, {narrowed:?}, differ from a whole redraw of them");
            assert!(patched<=whole,"{case}, frame {step}: {patched} texels composed where whole cells take {whole}");
            [patched,whole]
        }).collect::<Vec<_>>();
        let last=self.at(540.);
        let lift=layer_render::DabBatch {kind:layer_render::DabBatchKind::Persistent,stroke_id:layer_core::StrokeId(1),stroke_end:true,
            ..crate::test_support::dab_batch(target,style.clone(),last.bounds())};
        crate::test_support::submit_settled(&mut r,FramePacket {dabs:std::slice::from_ref(&last),dab_batches:std::slice::from_ref(&lift),time_seconds:2.,..frame});
        crate::test_support::submit_settled(&mut r,frame);
        let shown=bits(&r);
        r.scale_display.as_ref().unwrap().evict_window(plan.bounds);
        crate::test_support::submit_settled(&mut r,frame);
        assert!(bits(&r)==shown,"{case}: after pen up the display differs from a whole redraw of every cell");
        let redrawn=Redrawn {case,level,float,restarts,texels};
        println!("{}",redrawn.row());
        redrawn
    }
    fn patches(&self) {let redrawn=self.redrawn();assert!(redrawn.patched()*2>=redrawn.texels.len()-2,"{}: most frames compose less than whole cells: {:?}",redrawn.case,redrawn.texels);}
    fn stays_whole(self) {Self {redraws:false,..self}.redrawn();}
}
const ZOOMS:[f32;3]=[0.5,0.25,0.125];
fn retouching(preset:DefaultBrushPreset)->layer_render::DabStyle {
    layer_render::DabStyle {retouch:Some(layer_core::Retouch {source:layer_core::RetouchSource::Editing,offset:[40.,-120.],..Default::default()}),..crate::layer_tests::preset_style(preset)}
}
fn writers()->Vec<(&'static str,layer_render::DabStyle,usize)> {
    use DefaultBrushPreset::*;
    let style=crate::layer_tests::preset_style;
    vec![("a Pencil",style(Pencil),1),("a Pencil",style(Pencil),0),("an eraser",style(Eraser),1),("a paintbrush",style(Paintbrush),1),("a smudge",style(Smudge),0),("a smudge",style(Smudge),1),
        ("wet paint",style(WetRound),0),("a Dither blender",style(DitherBlender),0),("a Scatter blender",style(ScatterBlender),0),("a clone stamp",retouching(CloneStamp),1),
        ("a Liquify push",style(LiquifyPush),0),("a wet-edged brush",wet_edged_brush(),0)]
}

#[test]
fn below_full_size_every_patched_frame_equals_whole_cells_for_each_kind_of_brush_write() {
    for zoom in ZOOMS {for (brush,style,tips) in writers() {Zoomed::new("a photo layer",brush,style,tips,zoom).patches();}}
    for zoom in ZOOMS {Zoomed::new("a photo layer","a watercolor wash",crate::layer_tests::preset_style(DefaultBrushPreset::WatercolorWash),0,zoom).stays_whole();}
}

const RETRACE:[(f32,f32);12]=[(492.,498.),(498.,504.),(504.,510.),(510.,516.),(516.,522.),(522.,528.),(516.,510.),(510.,504.),(504.,498.),(498.,492.),(492.,486.),(486.,480.)];
#[test]
fn at_half_size_a_stroke_that_retraces_its_path_with_a_tip_equals_a_renderer_that_never_patches_in_each_scene() {
    use DefaultBrushPreset::{Paintbrush,Pencil};
    let style=crate::layer_tests::preset_style;
    let patching=["a blank layer","a photo layer","the middle layer of five","a layer in a group","a masked layer","a drawing without Paper","a masked layer without Paper"];
    for scene in patching {
        let [patched,whole]=Zoomed::new(scene,"a Pencil",style(Pencil),1,0.5).retraces_like_a_renderer_that_never_patches();
        assert!(patched<whole,"a Pencil on {scene}: the retraced stroke composes less than whole cells: {patched} against {whole}");
    }
    for scene in ["a layer under an adjustment","a layer under a blur","a layer offset by 3 and 5"] {
        Zoomed::new(scene,"a Pencil",style(Pencil),1,0.5).retraces_like_a_renderer_that_never_patches();
    }
    for scene in ["a photo layer","a masked layer without Paper"] {Zoomed::new(scene,"a paintbrush",style(Paintbrush),1,0.5).retraces_like_a_renderer_that_never_patches();}
}

fn level_pages_equal_a_full_reduction(zoom:f32) {
    for (brush,style,tips) in writers() {
        let stroke=Zoomed::new("a photo layer",brush,style,tips,zoom);
        for frames in [3,4,6,8,10,12] {
            let Some((differing,largest,compact))=stroke.level_pages_against_a_full_reduction(frames) else {println!("| {} | {frames} | no sequence |",stroke.case());continue;};
            println!("| {} | {frames} | {differing:?} | {largest:e} | compact {compact} |",stroke.case());
            assert!(differing==[0;2] || compact && largest<1e-6,
                "{}, after {frames} frames: {differing:?} texels of the level pages, away from the tip and under it, differ from a full reduction by up to {largest:e}",stroke.case());
        }
    }
}
#[test]
fn at_half_size_a_stroke_leaves_its_level_pages_equal_to_a_full_reduction() {level_pages_equal_a_full_reduction(0.5);}
#[test]
fn at_quarter_size_a_stroke_leaves_its_level_pages_equal_to_a_full_reduction() {level_pages_equal_a_full_reduction(0.25);}
#[test]
fn at_an_eighth_size_a_stroke_leaves_its_level_pages_equal_to_a_full_reduction() {level_pages_equal_a_full_reduction(0.125);}

fn pencil_and_smudge(scene:&str,zoom:f32)->[Zoomed<'_>;2] {
    let style=crate::layer_tests::preset_style;
    [Zoomed::new(scene,"a Pencil",style(DefaultBrushPreset::Pencil),1,zoom),Zoomed::new(scene,"a smudge",style(DefaultBrushPreset::Smudge),0,zoom)]
}

#[test]
fn below_full_size_every_patched_frame_equals_whole_cells_for_each_layer_setup() {
    for zoom in ZOOMS {
        Zoomed::new("a blank layer","a Pencil",crate::layer_tests::preset_style(DefaultBrushPreset::Pencil),1,zoom).patches();
        for scene in ["the middle layer of five","a layer in a group","a drawing without Paper","a masked layer"] {for stroke in pencil_and_smudge(scene,zoom) {stroke.patches();}}
    }
    for zoom in [0.37,0.14] {for stroke in pencil_and_smudge("a photo layer",zoom) {stroke.patches();}}
}

#[test]
fn below_full_size_a_cell_drawn_through_an_effect_or_on_an_offset_layer_composes_whole_cells_and_the_same_pixels() {
    for zoom in ZOOMS {
        for scene in ["a layer under an adjustment","a layer under a blur"].into_iter().chain(OFFSETS.map(|(scene,_)|scene)) {
            for stroke in pencil_and_smudge(scene,zoom) {stroke.stays_whole();}
        }
    }
}

fn every_preset(zoom:f32) {
    for preset in DefaultBrushPreset::ALL {
        let brush=layer_core::default_brush(preset);
        let style=if brush.execution.retouches() {retouching(preset)}else {crate::layer_tests::preset_style(preset)};
        use layer_core::BrushExecution::{Liquify,Watercolor};
        let class=brush.execution_class();
        let redrawn=Zoomed {diameter:30.,frames:6,redraws:class!=Watercolor,
            ..Zoomed::new("a photo layer",&format!("{preset:?} ({class:?})"),style,usize::from(!matches!(class,Liquify|Watercolor)),zoom)}.redrawn();
        let whole=class==Watercolor || redrawn.texels.iter().all(|[_,whole]|*whole==0);
        assert_eq!(redrawn.patched()==0,whole,"{}: {:?}",redrawn.case,redrawn.texels);
    }
}
#[test]
fn at_half_size_every_brush_preset_but_watercolor_composes_less_than_whole_cells_and_the_same_pixels() {every_preset(0.5);}
#[test]
fn at_quarter_size_every_brush_preset_but_watercolor_composes_less_than_whole_cells_and_the_same_pixels() {every_preset(0.25);}
#[test]
fn at_an_eighth_size_every_brush_preset_but_watercolor_composes_less_than_whole_cells_and_the_same_pixels() {every_preset(0.125);}

#[test]
fn below_full_size_no_brush_size_composes_more_than_whole_cells_and_small_ones_compose_their_footprint() {
    for (brush,style,tips) in writers().into_iter().filter(|(_,_,tips)|*tips==0).chain([("a Pencil",crate::layer_tests::preset_style(DefaultBrushPreset::Pencil),1)]) {
        for zoom in if brush=="a Pencil" {ZOOMS.to_vec()}else {vec![0.25]} {
            for diameter in [5.,30.,100.,300.,600.,2000.] {Zoomed {diameter,frames:6,..Zoomed::new("a photo layer",brush,style.clone(),tips,zoom)}.redrawn();}
        }
    }
    let redrawn=Zoomed::new("a blank layer","a Pencil",crate::layer_tests::preset_style(DefaultBrushPreset::Pencil),0,0.25).redrawn();
    let (cell,footprint)=(u64::from(PAGE_SIZE).pow(2),(9u64.div_ceil(4)+1).pow(2));
    let inside=redrawn.texels[2..].iter().filter(|[patched,_]|*patched<cell/64).collect::<Vec<_>>();
    assert!(inside.len()>=8 && inside.iter().all(|[patched,_]|*patched<=4*footprint),"{}: a frame inside a cell composes a few times its {footprint} texel footprint: {:?}",redrawn.case,redrawn.texels);
}

#[test]
fn a_blender_prediction_over_a_rectangle_prepares_the_neighbors_of_every_page_it_writes() {
    use layer_core::color::SampleDepth;
    use layer_engine::{CanvasEngine, InstantFeedbackConfig, PenEvent, PenPhase, SampleFlags, ViewTransform, input_queue};

    let extent = [4248, 2832];
    let photo = rgba8_source(extent, |x, y| [(x * 3 % 251) as u8, (y * 5 % 241) as u8, ((x + y) % 197) as u8, 255]);
    let mut project = layer_color::photo_project(Arc::unwrap_or_clone(photo), Default::default(), "Photo".into(), SampleDepth::U8).unwrap();
    crate::test_support::add_paint(&mut project.artwork, "Paint", extent);
    let mut document = layer_core::Document::from_artwork(project.artwork).unwrap();
    assert_eq!(document.composition().size, extent);
    let photo = document.scene().order().iter().copied()
        .find(|id| document.scene().occurrence(*id).unwrap().name.as_ref() == "Photo").unwrap();
    document.working.occurrence = Some(photo);
    document.working.target = document.scene().source_target(photo);
    assert!(document.scene().paint_base(document.working.target.unwrap()).is_some());

    let [zoom, x, y] = [0.33686438, 286.50006, 181.00003];
    let view = layer_render::ViewState { width_px: 1920, height_px: 1200, document_to_surface: [zoom, 0., 0., zoom, x, y] };
    let transform = ViewTransform { revision: 5, surface_to_document: [1. / zoom, 0., 0., 1. / zoom, -x / zoom, -y / zoom] };
    let mut rasterizer = renderer(&document);
    crate::test_support::write_only_dry_material(&mut rasterizer);
    let (mut input, consumer) = input_queue(2048);
    let mut engine = CanvasEngine::new(rasterizer, document, consumer, view, transform).unwrap();
    let brush = layer_core::BrushSnapshot { diameter: 512., ..layer_core::default_brush(DefaultBrushPreset::DitherBlender) };
    engine.set_brush(brush).unwrap();
    engine.set_instant_feedback(InstantFeedbackConfig {
        enabled: true, use_platform_prediction: false, prediction_horizon_micros: 16_000, ..Default::default()
    }).unwrap();
    let mut clock = 1_000_000_000;
    let mut sequence = 0;
    settle_engine(&mut engine, &mut clock);
    for (label, samples) in [("prime", 300), ("second", 25)] {
        let start = clock + 16_666_667;
        let mut next_frame = start;
        for step in 0..=samples {
            let sample_ns = start + step * 5_000_000;
            while next_frame < sample_ns {
                engine.render_frame_at(next_frame).unwrap_or_else(|error| panic!("{label}, before sample {step}: {error}"));
                engine.backend_mut().wait_idle().unwrap();
                next_frame += 16_666_667;
            }
            let angle = step as f32 * 0.005 * std::f32::consts::TAU;
            let phase = if step == 0 { PenPhase::Down } else if step == samples { PenPhase::Up } else { PenPhase::Move };
            sequence += 1;
            input.push(PenEvent {
                timestamp_ns: sample_ns, view_revision: 5, pressure: if phase == PenPhase::Up { 0. } else { 1. },
                ..crate::test_support::pen(sequence, phase, [1002. + 185. * angle.cos(), 658. + 90. * angle.sin()], SampleFlags::PRIMARY)
            }).unwrap();
        }
        clock = next_frame;
        settle_engine(&mut engine, &mut clock);
        assert!(engine.can_undo(), "{label} Dither contact should commit");
    }
}
