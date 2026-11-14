use super::*;
use layer_ui::{CommandId,Platform,PreferenceAction,PreferenceId,PreferenceValue,UiAction,UiSession};

struct UiRestore {
    session:UiSession<WgpuRasterizer>,
    clock:std::time::Instant,
}
impl UiRestore {
    fn new(document:layer_core::Document)->Self {
        let renderer=bounded_renderer(document.composition().color).unwrap();
        let mut session=UiSession::new(renderer,document,[900,600],Platform::Gtk).unwrap();
        session.dispatch(UiAction::Preferences {action:PreferenceAction::Edit {id:PreferenceId::Feedback,value:PreferenceValue::Bool(false)}}).unwrap();
        session.dispatch(UiAction::SelectBrush {id:layer_core::DefaultBrushPreset::GPen as u32}).unwrap();
        session.dispatch(UiAction::SetBrushSize {value:40.}).unwrap();
        Self {session,clock:std::time::Instant::now()}
    }
    fn step(&mut self) {
        let now=1_000_000_000+self.clock.elapsed().as_nanos() as u64;
        self.session.frame(now,now).unwrap();crate::test_support::complete(self.session.renderer_mut());
    }
    fn finish(&mut self) {
        let deadline=std::time::Instant::now()+READBACK_TIMEOUT;
        loop {
            self.step();let engine=self.session.engine();
            if !self.session.holding_input() && !engine.has_pending_input() && !engine.has_pending_document_edits()
                && !engine.raster_backing_pending() && !engine.has_active_stroke() && self.session.require_document_idle().is_ok()
                && paint_at(engine.document(),0).raster.try_data().is_some() {return;}
            assert!(std::time::Instant::now()<deadline,"real UiSession contact and selected raster must finish");
            std::thread::yield_now();
        }
    }
    fn color(&mut self,color:[f32;4]) {self.session.dispatch(UiAction::SetColor {rgba:color}).unwrap();}
    fn pen(&mut self,sequence:u64,phase:PenPhase,point:[f32;2]) {
        let [a,b,c,d,x,y]=self.session.camera().document_to_surface();
        let event=PenEvent {timestamp_ns:sequence*100_000_000,view_revision:self.session.camera().revision,
            pressure:if phase==PenPhase::Up {0.}else {1.},
            ..crate::test_support::pen(sequence,phase,[a*point[0]+c*point[1]+x,b*point[0]+d*point[1]+y],SampleFlags::PRIMARY)};
        self.session.pen(event);
    }
    fn stroke(&mut self,first:u64,y:f32) {
        self.pen(first,PenPhase::Down,[112.,y]);self.pen(first+1,PenPhase::Move,[256.,y]);self.pen(first+2,PenPhase::Up,[400.,y]);
        self.finish();
    }
    fn undo(&mut self) {self.session.dispatch(UiAction::Invoke {command:CommandId::Undo}).unwrap();}
    fn contents(&self)->Contents {
        paint_at(self.session.engine().document(),0).raster.wait_data().unwrap().tiles.iter()
            .map(|(key,tile)|(*key,tile.wait_backing().unwrap().decode_on_worker().unwrap())).collect()
    }
}

#[test]
fn a_real_ui_contact_waiting_eight_seconds_after_undo_keeps_every_sample_and_raster_byte() {
    let document=backed_document();let mut actual=UiRestore::new(document.clone());let mut replay=UiRestore::new(document);
    actual.finish();replay.finish();
    for session in [&mut actual,&mut replay] {session.color([1.,0.,0.,1.]);session.stroke(1,96.);}
    let a=actual.contents();assert!(pixel(&a,256,96)[0]>pixel(&replay.contents(),256,120)[0]);
    replay.undo();replay.finish();
    let color=replay.session.engine().document().composition().color;
    let (retired,_)=replay.session.replace_renderer(bounded_renderer(color).unwrap()).unwrap();drop(retired);replay.finish();
    let history=actual.session.engine().retained_tiles();let store=crate::test_support::GatedStore::new(&history,false);
    {
        let r=actual.session.renderer_mut();let samples=&r.device.source_samples;let limit=samples.stats().limit_bytes;
        samples.set_limit(0);samples.trim_unused(usize::MAX);samples.set_limit(limit);
        crate::test_support::evict_sources(r);
    }
    actual.undo();actual.step();store.gate.entered();
    assert!(actual.session.engine().has_pending_document_edits(),"the selected S0 destination must really remain unmaterialized");
    assert!(actual.session.paint_ready(),"this is a real restore gate, not the Recorder-only brush-readiness gate");
    let before=actual.session.engine().metrics();let submitted=actual.session.renderer_mut().submitted_updates();
    actual.color([0.,0.,1.,1.]);actual.pen(4,PenPhase::Down,[112.,120.]);actual.pen(5,PenPhase::Move,[256.,120.]);
    assert!(!actual.session.holding_input(),"plain paint enters the Engine queue while real raster preparation is pending");
    let held=std::time::Instant::now();
    while held.elapsed()<std::time::Duration::from_secs(8) {
        actual.step();
        assert!(actual.session.engine().has_pending_input());
        assert_eq!(actual.session.engine().metrics().input_events,before.input_events);
        assert_eq!(actual.session.renderer_mut().submitted_updates(),submitted);
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    actual.pen(6,PenPhase::Up,[400.,120.]);store.gate.release();actual.finish();
    replay.color([0.,0.,1.,1.]);let replay_before=replay.session.engine().metrics();replay.stroke(4,120.);
    let metrics=actual.session.engine().metrics();
    assert_eq!(metrics.input_events-before.input_events,3);assert_eq!(metrics.committed_strokes-before.committed_strokes,1);
    assert_eq!(replay.session.engine().metrics().input_events-replay_before.input_events,3);
    let expected=replay.contents();let result=actual.contents();
    assert_eq!(result.keys().collect::<Vec<_>>(),expected.keys().collect::<Vec<_>>());assert_contents(&result,&expected);
    assert!(pixel(&result,256,120)[2]>pixel(&result,256,96)[2]);
}
