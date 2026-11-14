use super::*;
use layer_engine::{CanvasEngine,InputProducer,InstantFeedbackConfig,PenEvent,PenPhase,SampleFlags,ViewTransform,input_queue};
use layer_core::raster::{RasterPlane,TileKey};

pub(super) type Contents=std::collections::BTreeMap<TileKey,Vec<u8>>;
pub(super) struct Harness {
    pub(super) input:InputProducer<PenEvent>,
    pub(super) engine:CanvasEngine<WgpuRasterizer>,
    presenter:Option<crate::ViewportPresenter>,
    clock:u64,
    view_revision:u64,
}
pub(super) fn blank_document(extent:[u32;2])->layer_core::Document {
    let mut document=layer_core::Document::new(PortableId::random(),extent[0],extent[1],layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    composition_mut(&mut document).color=DocumentColor {space:RgbSpace::Srgb,depth:SampleDepth::U8};document
}
impl Harness {
    pub(super) fn new(document:layer_core::Document)->Self {
        let (input,consumer)=input_queue(64);let color=document.composition().color;
        let mut engine=CanvasEngine::new(bounded_renderer(color).unwrap(),document,consumer,
            view([1.,0.,0.,1.,0.,0.]),ViewTransform::IDENTITY).unwrap();
        engine.set_instant_feedback(InstantFeedbackConfig {enabled:false,..Default::default()}).unwrap();
        Self {input,engine,presenter:None,clock:0,view_revision:0}
    }
    pub(super) fn step(&mut self) {
        self.clock=self.clock.saturating_add(16_666_667);
        prepare_engine_frame(&mut self.engine);self.engine.render_frame_at(self.clock).unwrap();
        self.engine.backend_mut().wait_idle().unwrap();
    }
    pub(super) fn finish(&mut self) {
        let deadline=std::time::Instant::now()+READBACK_TIMEOUT;
        loop {
            self.step();let ready=paint_at(self.engine.document(),0).raster.try_data().is_some();
            if !self.engine.has_pending_input() && !self.engine.has_pending_document_edits() && !self.engine.raster_backing_pending()
                && !self.engine.has_active_stroke() && ready {return;}
            assert!(std::time::Instant::now()<deadline,"the selected committed raster must finish its accepted capture");
            std::thread::yield_now();
        }
    }
    pub(super) fn settle(&mut self) {
        self.finish();let deadline=std::time::Instant::now()+READBACK_TIMEOUT;
        while self.engine.backend().next_frame()==layer_render::FrameDemand::Work {
            self.step();assert!(std::time::Instant::now()<deadline,"selected authored picture did not settle");
        }
    }
    pub(super) fn queue(&mut self,first:u64,points:&[[f32;2]],brush:layer_core::BrushSnapshot) {
        assert!(points.len()>=2);self.engine.set_brush(brush).unwrap();
        let [a,b,c,d,x,y]=self.engine.view().document_to_surface;
        for (offset,point) in points.iter().enumerate() {
            let sequence=first+offset as u64;let phase=if offset==0 {PenPhase::Down}else if offset+1==points.len() {PenPhase::Up}else {PenPhase::Move};
            self.input.push(PenEvent {timestamp_ns:sequence*100_000_000,pressure:if phase==PenPhase::Up {0.}else {1.},view_revision:self.view_revision,
                ..crate::test_support::pen(sequence,phase,[a*point[0]+c*point[1]+x,b*point[0]+d*point[1]+y],SampleFlags::PRIMARY)}).unwrap();
        }
        self.clock=self.clock.max((first+points.len() as u64)*100_000_000);
    }
    pub(super) fn gpen(&mut self,first:u64,y:f32,color:[f32;4]) {
        let mut brush=layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);brush.diameter=40.;brush.color_rgba_linear=color;
        self.queue(first,&[[112.,y],[256.,y],[400.,y]],brush);
    }
    pub(super) fn committed_bytes(&self)->Contents {
        paint_at(self.engine.document(),0).raster.wait_data().unwrap().tiles.iter().filter(|(key,_)|key.plane==RasterPlane::Color)
            .map(|(key,tile)|(*key,tile.wait_backing().unwrap().decode_on_worker().unwrap())).collect()
    }
    pub(super) fn assert_resident(&self,points:&[[u32;2]]) {
        let target=source_at(self.engine.document(),0);let layer=self.engine.backend().paint_layers.iter().find(|layer|layer.id==target).unwrap();
        for point in points {let coordinate=point.map(|value|value/PAGE_SIZE);
            assert!(layer.pages.iter().any(|page|page.coordinate==coordinate),"A must remain GPU resident before Undo at {coordinate:?}");}
    }
    pub(super) fn frame(&mut self,matrix:[f32;6]) {
        let [a,_,_,d,x,y]=matrix;self.view_revision+=1;
        self.engine.set_view(ViewState {width_px:512,height_px:256,document_to_surface:matrix},
            ViewTransform {revision:self.view_revision,surface_to_document:[1./a,0.,0.,1./d,-x/a,-y/d]});
    }
    pub(super) fn zoom(&mut self,scale:f32) {
        let extent=self.engine.document().composition().size;self.view_revision+=1;
        self.engine.set_view(ViewState {width_px:(extent[0] as f32*scale).ceil() as u32,height_px:(extent[1] as f32*scale).ceil() as u32,
            document_to_surface:[scale,0.,0.,scale,0.,0.]},ViewTransform {revision:self.view_revision,surface_to_document:[1./scale,0.,0.,1./scale,0.,0.]});
    }
    pub(super) fn present(&mut self)->Vec<[f32;4]> {
        let renderer=self.engine.backend();let presenter=self.presenter.get_or_insert_with(||crate::test_support::float_presenter(renderer));
        super::present(renderer,presenter,self.engine.view())
    }
}
pub(super) fn pixel(tiles:&Contents,x:u32,y:u32)->[u8;4] {
    let key=TileKey {plane:RasterPlane::Color,coordinate:[x/PAGE_SIZE,y/PAGE_SIZE]};
    tiles.get(&key).map_or([0;4],|bytes| {let index=((y%PAGE_SIZE)*PAGE_SIZE+x%PAGE_SIZE) as usize*4;bytes[index..index+4].try_into().unwrap()})
}
pub(super) fn assert_contents(actual:&Contents,expected:&Contents) {
    let keys=actual.keys().chain(expected.keys()).copied().collect::<std::collections::BTreeSet<_>>();let empty=vec![0;(PAGE_SIZE*PAGE_SIZE*4) as usize];
    for key in keys {let a=actual.get(&key).unwrap_or(&empty);let b=expected.get(&key).unwrap_or(&empty);
        let difference=a.iter().zip(b).position(|(a,b)|a!=b);assert_eq!(a.len(),b.len());
        assert!(difference.is_none(),"selected committed tile {key:?} differs at byte {difference:?}: actual={:?} expected={:?}",difference.map(|i|a[i]),difference.map(|i|b[i]));}
}

#[test]
fn undo_then_overlapping_stroke_commits_only_selected_raster_contents() {
    let document=blank_document([512,256]);let mut actual=Harness::new(document.clone());let mut reference=Harness::new(document);
    actual.finish();reference.finish();actual.gpen(1,96.,[1.,0.,0.,1.]);actual.finish();
    let stroke_a=actual.committed_bytes();assert!(pixel(&stroke_a,256,96)[3]>0);actual.assert_resident(&[[256,96]]);
    assert!(actual.engine.undo().unwrap());assert!(actual.committed_bytes().values().all(|bytes|bytes.iter().all(|byte|*byte==0)));
    let before=crate::test_support::PassCounts::snapshot();
    actual.gpen(4,120.,[0.,0.,1.,1.]);actual.finish();
    assert_eq!(crate::test_support::PassCounts::snapshot().delta(before).raster_write_defers,0,"the ordinary post-Undo stroke must not defer a typed raster write");
    reference.gpen(4,120.,[0.,0.,1.,1.]);reference.finish();
    let expected=reference.committed_bytes();let result=actual.committed_bytes();
    assert_eq!(pixel(&expected,256,96),[0;4]);assert!(pixel(&expected,256,120)[3]>0);
    assert!(pixel(&stroke_a,256,108)[3]>0 && pixel(&expected,256,108)[3]>0);assert_contents(&result,&expected);
    assert_eq!(pixel(&result,256,96),[0;4]);assert_eq!(pixel(&result,256,120),pixel(&expected,256,120));
}

#[test]
fn a_warm_stroke_restores_only_its_required_tiles_and_keeps_the_others_through_undo_and_redo() {
    use layer_core::raster::{RasterData,RasterRevision,RasterTile,TileBlob};
    let mut document=blank_document([1024,512]);let descriptor=document.composition().color.paint_descriptor();
    let tiles=(0..8u32).map(|index|(TileKey {plane:RasterPlane::Color,coordinate:[index%4,index/4]},
        RasterTile::backed(TileBlob::encode(descriptor,&[(index*30) as u8,48,90,255].repeat((PAGE_SIZE*PAGE_SIZE) as usize)).unwrap()))).collect();
    paint_mut(&mut document,0).raster=RasterRevision::backed(RasterData {tiles,..Default::default()});
    let pages=|harness:&Harness| {
        let target=source_at(harness.engine.document(),0);let layer=harness.engine.backend().paint_layers.iter().find(|layer|layer.id==target).unwrap();
        [[0,1],[1,1]].map(|coordinate|layer.pages.iter().find(|page|page.coordinate==coordinate).map(|page|(page.active().allocation_identity(),page.active().name())))
    };
    let mut actual=Harness::new(document);actual.zoom(1.);actual.settle();
    actual.gpen(1,352.,[0.,0.,1.,1.]);actual.settle();
    let resident=pages(&actual);assert!(resident.iter().all(Option::is_some),"the first stroke leaves its tiles resident");
    let counts=crate::test_support::PassCounts::snapshot();
    let mut brush=layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);brush.diameter=40.;brush.color_rgba_linear=[1.,0.,0.,1.];
    actual.engine.set_brush(brush).unwrap();let [a,b,c,d,x,y]=actual.engine.view().document_to_surface;
    let push=|harness:&mut Harness,sequence:u64,phase:PenPhase,point:[f32;2]|harness.input.push(PenEvent {timestamp_ns:sequence*100_000_000,pressure:if phase==PenPhase::Up {0.}else {1.},
        view_revision:harness.view_revision,..crate::test_support::pen(sequence,phase,[a*point[0]+c*point[1]+x,b*point[0]+d*point[1]+y],SampleFlags::PRIMARY)}).unwrap();
    for (sequence,phase,point) in [(4,PenPhase::Down,[112.,96.]),(5,PenPhase::Move,[256.,96.]),(6,PenPhase::Move,[400.,96.])] {push(&mut actual,sequence,phase,point);}
    actual.clock=actual.clock.max(700_000_000);
    for frame in 0..6 {actual.step();assert!(actual.engine.has_active_stroke());
        assert_eq!(pages(&actual),resident,"contact frame {frame}: no resident tile outside the stroke is re-uploaded or dropped");}
    push(&mut actual,7,PenPhase::Up,[400.,96.]);actual.settle();
    let extra=crate::test_support::PassCounts::snapshot().delta(counts).warm_restore_extra_tiles;
    for step in 0..3 {
        if step==1 {assert!(actual.engine.undo().unwrap());actual.settle();}
        if step==2 {assert!(actual.engine.redo().unwrap());actual.settle();}
        let mut reference=Harness::new(actual.engine.document().clone());reference.zoom(1.);reference.settle();
        assert_picture(&mut actual,&mut reference);
    }
    assert_eq!(extra,0,"a warm stroke frame selects only the tiles it requires");
}

fn backed_document()->layer_core::Document {
    use layer_core::raster::{RasterData,RasterRevision,RasterTile,TileBlob};
    let mut document=blank_document([512,256]);let descriptor=document.composition().color.paint_descriptor();
    let tiles=(0..2).map(|x|(TileKey {plane:RasterPlane::Color,coordinate:[x,0]},
        RasterTile::backed(TileBlob::encode(descriptor,&[16,48,0,255].repeat((PAGE_SIZE*PAGE_SIZE) as usize)).unwrap()))).collect();
    paint_mut(&mut document,0).raster=RasterRevision::backed(RasterData {tiles,..Default::default()});document
}
fn assert_picture(actual:&mut Harness,reference:&mut Harness) {
    close(&actual.present(),&reference.present());
    let a=actual.engine.backend();let b=reference.engine.backend();
    let ac=a.scale_display.as_ref().unwrap();let bc=b.scale_display.as_ref().unwrap();assert_eq!(ac.plan,bc.plan);
    close(&pixels(a,ac.texture()),&pixels(b,bc.texture()));
    if ac.plan.level>0 {
        let source=source_at(actual.engine.document(),0);let other=source_at(reference.engine.document(),0);
        let plan=display_mips::Plan::at(ac.plan.extent,ac.plan.level);
        let ai=source_image(a,ac,&a.scene.as_ref().unwrap().scale_sources.entries[&source],source,plan);
        let bi=source_image(b,bc,&b.scene.as_ref().unwrap().scale_sources.entries[&other],other,plan);
        close(&pixels(a,&ai.texture),&pixels(b,&bi.texture));
    }
    close(&retained_level_pixels(a,ac,ac.plan.level),&retained_level_pixels(b,bc,bc.plan.level));
}
fn pending_restore_after_history(redo:bool,zoom:bool) {
    use std::sync::atomic::Ordering;
    let document=backed_document();let mut actual=Harness::new(document.clone());let mut reference=Harness::new(document);
    actual.zoom(0.25);reference.zoom(0.25);actual.settle();reference.settle();
    actual.gpen(1,96.,[1.,0.,0.,1.]);actual.settle();actual.assert_resident(&[[128,96],[256,96]]);
    if redo {assert!(actual.engine.undo().unwrap());actual.settle();assert!(actual.engine.redo().unwrap());actual.settle();
        actual.assert_resident(&[[128,96],[256,96]]);}
    if zoom {actual.zoom(1.);actual.settle();}
    let history=actual.engine.retained_tiles();let store=crate::test_support::GatedStore::new(&history,false);
    {let r=actual.engine.backend_mut();let samples=&r.device.source_samples;let limit=samples.stats().limit_bytes;
        samples.set_limit(0);samples.trim_unused(usize::MAX);samples.set_limit(limit);
        crate::test_support::evict_sources(r);
    }
    assert!(actual.engine.undo().unwrap());if zoom {actual.zoom(0.25);}actual.step();store.gate.entered();
    assert!(actual.engine.has_pending_document_edits(),"selected restore must remain pending while its source read is gated");
    actual.gpen(4,120.,[0.,0.,1.,1.]);let submitted=actual.engine.backend().submitted_updates();
    actual.step();assert!(actual.engine.has_pending_input(),"B must wait behind the selected pending restore");
    assert_eq!(actual.engine.backend().submitted_updates(),submitted,"pending restore must block B evaluation");
    assert!(store.gate.reads.load(Ordering::Acquire)>0);store.gate.release();actual.settle();
    reference.gpen(4,120.,[0.,0.,1.,1.]);reference.settle();
    assert_contents(&actual.committed_bytes(),&reference.committed_bytes());assert_picture(&mut actual,&mut reference);
    actual.zoom(1.);reference.zoom(1.);actual.settle();reference.settle();assert_picture(&mut actual,&mut reference);
}

#[test]
fn cached_cell_keys_compose_like_an_uncached_full_plan_after_strokes_layer_edits_undo_and_a_pan() {
    fn edit(harness:&mut Harness,index:usize,change:impl FnOnce(&mut Occurrence)) {
        let document=harness.engine.document();let id=document.scene().order()[index];
        let mut occurrence=document.artwork.occurrences.get(id).unwrap().clone();change(&mut occurrence);
        let edit=layer_core::Edit::Occurrence(layer_core::RecordChange::replace(&document.artwork.occurrences,id,Some(occurrence)).unwrap());
        harness.engine.apply_edit(edit).unwrap();
    }
    fn pan(harness:&mut Harness,x:f32) {
        harness.view_revision+=1;
        harness.engine.set_view(ViewState {width_px:512,height_px:256,document_to_surface:[1.,0.,0.,1.,-x,0.]},
            ViewTransform {revision:harness.view_revision,surface_to_document:[1.,0.,0.,1.,x,0.]});
    }
    let mut document=blank_document([1024,256]);
    let layer=paint_occurrence(&mut document,"Wash",Some(rgba8_source([1024,256],|x,y|[(x/4) as u8,90,(y/2) as u8,140]).into()));
    insert_occurrence(&mut document,layer,1);
    let mut actual=Harness::new(document);pan(&mut actual,0.);actual.settle();
    actual.gpen(1,96.,[1.,0.,0.,1.]);actual.settle();actual.gpen(4,160.,[0.,0.,1.,1.]);actual.settle();
    edit(&mut actual,1,|occurrence|occurrence.visible=false);actual.settle();
    edit(&mut actual,1,|occurrence|occurrence.visible=true);actual.settle();
    edit(&mut actual,0,|occurrence|occurrence.opacity=0.5);actual.settle();
    actual.gpen(7,128.,[0.,1.,0.,1.]);actual.settle();
    assert!(actual.engine.undo().unwrap());actual.settle();
    assert!(actual.engine.redo().unwrap());actual.settle();
    actual.gpen(10,200.,[1.,1.,0.,1.]);actual.settle();
    pan(&mut actual,256.);actual.settle();
    actual.gpen(13,64.,[0.,1.,1.,1.]);actual.settle();
    let mut reference=Harness::new(actual.engine.document().clone());pan(&mut reference,256.);reference.settle();
    assert_picture(&mut actual,&mut reference);
}

#[test]
fn undo_then_overlapping_stroke_waits_for_selected_nonempty_restore() {pending_restore_after_history(false,false);}
#[test]
fn undo_then_zoom_and_overlapping_stroke_use_only_selected_history() {pending_restore_after_history(false,true);}
#[test]
fn redo_then_undo_and_overlapping_stroke_use_only_selected_history() {pending_restore_after_history(true,false);}

#[path="downstream_author_tests.rs"]
mod downstream_authors;

#[path="one_pass_tests.rs"]
mod one_pass;

#[path="ui_held_restore_tests.rs"]
mod ui_held_restore;
