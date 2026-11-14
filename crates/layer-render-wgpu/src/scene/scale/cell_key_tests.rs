use super::*;
use layer_core::raster::ContentVersion;
use crate::IdMap;

pub(in crate::scene::scale) fn cached_cells(r:&WgpuRasterizer,cache:&Cache)->IdMap<[i64;4],ContentVersion> {
    let pages=r.scene.as_ref().unwrap().scale_pages.lock().unwrap();
    super::super::signatures::tests::cached_cells(pages.expected().unwrap(),cache.graph.content.as_ref().unwrap(),cache.plan.level)
}
pub(in crate::scene::scale) fn begin_frame(r:&mut WgpuRasterizer,frame:FramePacket<'_>) {
    let scene=r.scene.take().unwrap();scene.prepare_signatures(r,frame).unwrap();r.scene=Some(scene);
}
fn cell(region:[i64;4])->PixelRect {PixelRect::new(region[0] as u32,region[1] as u32,region[2] as u32,region[3] as u32)}
fn regions(cells:&IdMap<[i64;4],ContentVersion>)->BTreeSet<[i64;4]> {cells.keys().copied().collect()}
pub(in crate::scene::scale) fn expected(r:&WgpuRasterizer)->Arc<SignatureFrame> {r.scene.as_ref().unwrap().scale_pages.lock().unwrap().expected().unwrap().clone()}
pub(in crate::scene::scale) fn touched(batch:&layer_render::DabBatch,cells:&IdMap<[i64;4],ContentVersion>)->BTreeSet<[i64;4]> {
    let damage=PixelRect::new(batch.damage.min.x.floor() as u32,batch.damage.min.y.floor() as u32,batch.damage.max.x.ceil() as u32,batch.damage.max.y.ceil() as u32);
    regions(cells).into_iter().filter(|region|!cell(*region).intersect(damage).is_empty()).collect()
}
pub(in crate::scene::scale) fn replanned(cache:&Cache,cells:&IdMap<[i64;4],ContentVersion>,before:&SignatureFrame,after:&SignatureFrame)->BTreeSet<[i64;4]> {
    let [previous,next]=[before,after].map(|frame|SignatureFrame::from_snapshot(frame.snapshot()));
    for region in cells.keys() {previous.expected_key(&cache.page_key(cache.graph.root.clone().unwrap(),cell(*region)),cell(*region),cache.required_input_level()).unwrap();}
    next.carry(&previous);
    regions(cells).difference(&regions(&super::super::signatures::tests::cached_cells(&next,cache.graph.content.as_ref().unwrap(),cache.plan.level))).copied().collect()
}
pub(in crate::scene::scale) fn assert_cached_keys_are_exact(cache:&Cache,frame:&SignatureFrame,cells:&IdMap<[i64;4],ContentVersion>) {
    let fresh=SignatureFrame::from_snapshot(frame.snapshot());
    for region in cells.keys() {
        let key=cache.page_key(cache.graph.root.clone().unwrap(),cell(*region));
        let [cached,uncached]=[frame,&fresh].map(|frame|frame.expected_key(&key,cell(*region),cache.required_input_level()).unwrap().into_version().identity());
        assert_eq!(cached,uncached,"the cached key of {region:?} equals an uncached evaluation");
    }
}

#[test]
fn a_stroke_frame_replans_only_the_cells_its_dabs_touch() {
    let doc=document_at([2048,1024]);let extent=doc.composition().size;let target=primary_target(&doc);
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
    frame.view.width_px=2048;frame.view.height_px=1024;frame.view.document_to_surface=[1.,0.,0.,1.,0.,0.];
    crate::test_support::submit_published(&mut r,frame);
    begin_frame(&mut r,frame);
    let settled=cached_cells(&r,r.scale_display.as_ref().unwrap());
    let visible=pages::coordinates(PixelRect::full(extent),0).count();
    assert_eq!(settled.len(),visible,"an unchanged frame starts with every visible cell's key");
    let before=expected(&r);
    let dab=crate::tests::test_dab([300.,300.],[0.9,0.1,0.3,1.],0.8);
    let batch=dab_batch(target,crate::layer_tests::preset_style(DefaultBrushPreset::GPen),dab.bounds());
    crate::test_support::submit_ready(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
    let cache=r.scale_display.as_ref().unwrap();
    let stroked=cached_cells(&r,cache);let damaged=touched(&batch,&settled);
    assert!(!damaged.is_empty() && damaged.len()<visible);
    assert_eq!(replanned(cache,&settled,&before,&expected(&r)),damaged,"a stroke frame re-plans exactly the {} cells its dabs touch out of {visible}",damaged.len());
    for (region,version) in &stroked {assert_eq!(damaged.contains(region),settled[region].identity()!=version.identity(),"only the damaged cells change their key: {region:?}");}
    assert_cached_keys_are_exact(cache,&expected(&r),&stroked);
}

#[test]
fn panning_by_one_cell_replans_only_the_entering_cells() {
    let doc=document_at([16384,1024]);let extent=doc.composition().size;
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
    frame.view.width_px=2048;frame.view.height_px=1024;frame.view.document_to_surface=[1.,0.,0.,1.,0.,0.];
    crate::test_support::submit_published(&mut r,frame);
    let mut settled=cached_cells(&r,r.scale_display.as_ref().unwrap());
    assert!(!settled.is_empty());
    let mut entered=0;
    for step in 1..=4 {
        let panned=FramePacket {view:layer_render::ViewState {document_to_surface:[1.,0.,0.,1.,-((step*PAGE_SIZE) as f32),0.],..frame.view},..frame};
        begin_frame(&mut r,panned);
        let carried=cached_cells(&r,r.scale_display.as_ref().unwrap());
        assert_eq!(regions(&carried),regions(&settled),"a one-cell pan keeps the key of every planned cell at step {step}");
        crate::test_support::submit_published(&mut r,panned);
        let after=cached_cells(&r,r.scale_display.as_ref().unwrap());
        for (region,version) in &settled {assert_eq!(after[region].identity(),version.identity(),"retained cell {region:?} keeps its key across the pan");}
        entered+=regions(&after).difference(&regions(&settled)).count();
        settled=after;
    }
    assert!(entered>0,"the pans bring new cells into the plan");
}

#[test]
fn a_stroke_frame_composes_its_cells_into_the_window_with_constant_store_work_whatever_the_view_size() {
    let mut visits=Vec::new();
    for extent in [[2048,1024],[4096,1024]] {
        let doc=document_at(extent);let target=primary_target(&doc);
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
        frame.view.width_px=extent[0];frame.view.height_px=extent[1];frame.view.document_to_surface=[1.,0.,0.,1.,0.,0.];
        crate::test_support::submit_settled(&mut r,frame);
        let settled=cached_cells(&r,r.scale_display.as_ref().unwrap());let before_pixels=display_pixels(&r);
        let mut frames=Vec::new();
        for step in 0..3 {
            let dab=crate::tests::test_dab([300.+4.*step as f32,300.],[0.9,0.1,0.3,1.],0.8);
            let batch=dab_batch(target,crate::layer_tests::preset_style(DefaultBrushPreset::GPen),dab.bounds());
            let before=crate::test_support::PassCounts::snapshot();
            crate::test_support::submit_ready(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
            let counts=crate::test_support::PassCounts::snapshot().delta(before);
            let damaged=touched(&batch,&settled).len() as u64;
            assert!(damaged>0 && damaged<settled.len() as u64);
            assert_eq!(counts.composite_commits,0,"stroke frame {step} at {extent:?} commits none of its {damaged} cells to the Store");
            assert_eq!(counts.projected_copies,0,"stroke frame {step} at {extent:?} composes its cells into the window instead of copying Store pages");
            assert!(counts.unit_acceptances<=2,"stroke frame {step} at {extent:?} accepts one source unit and the held parent unit, not one per cell: {}",counts.unit_acceptances);
            frames.push(counts.store_page_visits);
        }
        assert_ne!(display_pixels(&r),before_pixels,"the window shows the stroke at {extent:?}");
        visits.push(frames);
    }
    assert_eq!(visits[0],visits[1],"a stroke frame's Store work does not grow with the window's cell count");
}

#[test]
fn a_stroke_frame_claims_no_store_page_for_the_cells_it_composes_into_the_window() {
    let doc=document_at([2048,1024]);let extent=doc.composition().size;let target=primary_target(&doc);
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
    frame.view.width_px=extent[0];frame.view.height_px=extent[1];frame.view.document_to_surface=[1.,0.,0.,1.,0.,0.];
    crate::test_support::submit_settled(&mut r,frame);
    for step in 0..3 {
        let dab=crate::tests::test_dab([300.+4.*step as f32,300.],[0.9,0.1,0.3,1.],0.8);
        let batch=dab_batch(target,crate::layer_tests::preset_style(DefaultBrushPreset::GPen),dab.bounds());
        crate::test_support::submit_ready(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
        let cache=r.scale_display.as_ref().unwrap();let root=cache.graph.root.clone().unwrap();
        let unwritten=cache.pages.lock().unwrap().retained_pages().into_iter().filter(|page|page.key.expression==root && page.written.is_empty()).count();
        assert_eq!(unwritten,0,"stroke frame {step} claimed {unwritten} Store pages for cells it composed into the window");
    }
}

fn contact_batch(target:SourceTarget,dab:&layer_render::Dab,start:bool,end:bool)->layer_render::DabBatch {
    layer_render::DabBatch {stroke_start:start,stroke_end:end,..dab_batch(target,crate::layer_tests::preset_style(DefaultBrushPreset::GPen),dab.bounds())}
}

#[test]
fn a_contact_frame_keys_no_cells_and_accepts_declares_and_looks_up_nothing() {
    for extent in [[2048,1024],[4096,1024]] {
        let doc=document_at(extent);let target=primary_target(&doc);
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
        frame.view.width_px=extent[0];frame.view.height_px=extent[1];frame.view.document_to_surface=[1.,0.,0.,1.,0.,0.];
        crate::test_support::submit_settled(&mut r,frame);
        let settled=cached_cells(&r,r.scale_display.as_ref().unwrap());
        for step in 0..4 {
            let dab=crate::tests::test_dab([300.+40.*step as f32,300.],[0.9,0.1,0.3,1.],0.8);
            let batch=contact_batch(target,&dab,step==0,false);
            let before=crate::test_support::PassCounts::snapshot();
            crate::test_support::submit_ready(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
            let counts=crate::test_support::PassCounts::snapshot().delta(before);
            if step==0 {continue;}
            let delta=touched(&batch,&settled).len();
            assert_eq!([counts.unit_acceptances,counts.declarations,counts.store_gets],[0;3],
                "contact frame {step} at {extent:?} accepts, declares and looks up nothing");
            assert_eq!(counts.expected_keys,0,"contact frame {step} at {extent:?} keys none of its {delta} composed cells");
            assert_eq!(r.metrics.frame_composited_pages.len(),delta,"contact frame {step} at {extent:?} composes its {delta} cells");
        }
    }
}

#[test]
fn a_pen_pause_during_contact_writes_nothing_back_and_keeps_the_contact() {
    let extent=[2048,1024];let doc=document_at(extent);let target=primary_target(&doc);
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;
    frame.view.width_px=extent[0];frame.view.height_px=extent[1];frame.view.document_to_surface=[1.,0.,0.,1.,0.,0.];
    crate::test_support::submit_settled(&mut r,frame);
    let stroke=|r:&mut WgpuRasterizer,step:usize| {
        let dab=crate::tests::test_dab([300.+40.*step as f32,300.],[0.9,0.1,0.3,1.],0.8);
        let batch=contact_batch(target,&dab,step==0,false);
        let before=crate::test_support::PassCounts::snapshot();
        crate::test_support::submit_ready(r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..frame}).unwrap();
        crate::test_support::PassCounts::snapshot().delta(before)
    };
    stroke(&mut r,0);stroke(&mut r,1);
    for pause in 0..2 {
        let before=crate::test_support::PassCounts::snapshot();
        crate::test_support::submit_ready(&mut r,frame).unwrap();
        let counts=crate::test_support::PassCounts::snapshot().delta(before);
        assert_eq!([counts.composite_commits,counts.unit_acceptances,counts.declarations],[0;3],"pause frame {pause} writes nothing back and accepts nothing");
    }
    let counts=stroke(&mut r,2);
    assert_eq!([counts.unit_acceptances,counts.declarations,counts.store_gets],[0;3],"the stroke continues its contact after the pause");
    assert_eq!(counts.expected_keys,0,"the continued contact keys none of the cells it composes");
}
