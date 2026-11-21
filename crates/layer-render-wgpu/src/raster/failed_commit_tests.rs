use super::*;
use layer_engine::{PenEvent,PenPhase,SampleFlags};
use layer_ui::{CommandId,Platform,PreferenceAction,PreferenceId,PreferenceValue,UiAction,UiSession};

type Contents=BTreeMap<TileKey,Vec<u8>>;
struct Canvas {session:UiSession<WgpuRasterizer>,clock:std::time::Instant}
impl Canvas {
    fn new()->Self {
        let document=layer_core::Document::new(layer_core::PortableId::random(),512,256,layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        let mut session=UiSession::new(Self::renderer(&document),document,[900,600],Platform::Gtk).unwrap();
        session.dispatch(UiAction::Preferences {action:PreferenceAction::Edit {id:PreferenceId::Feedback,value:PreferenceValue::Bool(false)}}).unwrap();
        session.dispatch(UiAction::SelectBrush {id:layer_core::DefaultBrushPreset::GPen as u32}).unwrap();
        session.dispatch(UiAction::SetBrushSize {value:40.}).unwrap();
        let mut canvas=Self {session,clock:std::time::Instant::now()};canvas.finish();canvas
    }
    fn renderer(document:&layer_core::Document)->WgpuRasterizer {crate::memory::memory_tests::legacy_scene_renderer(document.composition().color)}
    fn frame(&mut self)->Result<(),String> {
        let now=1_000_000_000+self.clock.elapsed().as_nanos() as u64;
        let result=self.session.frame(now,now).map(drop);crate::test_support::complete(self.session.renderer_mut());result
    }
    fn finish(&mut self) {
        crate::test_support::wait_until("the canvas must settle",|| {
            self.frame().unwrap();let engine=self.session.engine();
            if !self.session.holding_input() && !engine.has_pending_input() && !engine.has_pending_document_edits()
                && !engine.raster_backing_pending() && !engine.has_active_stroke() && self.session.require_document_idle().is_ok() {Ok(())} else {Err(())}
        });
    }
    fn pen(&mut self,sequence:u64,phase:PenPhase,point:[f32;2]) {
        let [a,b,c,d,x,y]=self.session.camera().document_to_surface();
        self.session.pen(PenEvent {timestamp_ns:sequence*100_000_000,view_revision:self.session.camera().revision,
            pressure:if phase==PenPhase::Up {0.}else {1.},
            ..crate::test_support::pen(sequence,phase,[a*point[0]+c*point[1]+x,b*point[0]+d*point[1]+y],SampleFlags::PRIMARY)});
    }
    fn fail_raster_backing(&mut self) {
        let runtime=self.session.renderer_mut().raster.as_ref().unwrap();
        *runtime.worker.as_ref().unwrap().error.lock().unwrap()=Some("Raster backing failed".into());
    }
    fn fail_frame(&mut self) {
        crate::test_support::wait_until("the frame after a raster backing failure must fail",||self.frame().err().ok_or(()));
    }
    fn restart(&mut self) {
        self.session.suspend_renderer().unwrap();
        let renderer=Self::renderer(self.session.engine().document());
        drop(self.session.replace_renderer(renderer).unwrap());self.finish();
    }
    fn contents(&self)->Vec<Contents> {
        self.session.engine().document().artwork.paint.iter().map(|(_,_,source)|source.raster.wait_data().unwrap().tiles.iter()
            .map(|(key,tile)|(*key,tile.wait_backing().unwrap().decode_on_worker().unwrap())).collect()).collect()
    }
}

#[test]
fn a_restart_after_a_failed_stroke_commit_restores_the_pixels_from_before_the_stroke() {
    let mut canvas=Canvas::new();
    canvas.pen(1,PenPhase::Down,[112.,96.]);canvas.pen(2,PenPhase::Move,[256.,96.]);canvas.pen(3,PenPhase::Up,[400.,96.]);canvas.finish();
    let before=canvas.contents();let checkpoint=canvas.session.engine().checkpoint();
    assert!(before.iter().any(|layer|!layer.is_empty()));
    canvas.pen(4,PenPhase::Down,[112.,160.]);canvas.pen(5,PenPhase::Move,[256.,160.]);canvas.frame().unwrap();
    canvas.fail_raster_backing();
    canvas.pen(6,PenPhase::Up,[400.,160.]);canvas.fail_frame();
    assert_eq!(canvas.session.engine().metrics().committed_strokes,2,"the failed frame is the one that commits the stroke");
    canvas.restart();
    let after=canvas.contents();
    assert!(after==before,"the layer keeps the pixels from before the failed stroke: {:?} tiles became {:?}",
        before.iter().map(Contents::len).collect::<Vec<_>>(),after.iter().map(Contents::len).collect::<Vec<_>>());
    assert_eq!(canvas.session.engine().checkpoint(),checkpoint,"the stroke whose commit failed leaves history");
}

#[test]
fn a_restart_after_a_failed_bake_restores_the_layers_from_before_the_bake() {
    let mut canvas=Canvas::new();
    canvas.pen(1,PenPhase::Down,[112.,96.]);canvas.pen(2,PenPhase::Move,[256.,96.]);canvas.pen(3,PenPhase::Up,[400.,96.]);canvas.finish();
    let before=canvas.contents();let checkpoint=canvas.session.engine().checkpoint();
    canvas.fail_raster_backing();
    canvas.session.dispatch(UiAction::Invoke {command:CommandId::MergeVisible}).unwrap();
    assert_ne!(canvas.session.engine().checkpoint(),checkpoint);
    canvas.fail_frame();
    canvas.restart();
    let after=canvas.contents();
    assert!(after==before,"the layers keep the pixels from before the failed bake: {:?} tiles became {:?}",
        before.iter().map(Contents::len).collect::<Vec<_>>(),after.iter().map(Contents::len).collect::<Vec<_>>());
    assert_eq!(canvas.session.engine().checkpoint(),checkpoint,"the bake whose commit failed leaves history");
}
