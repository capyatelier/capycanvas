use crate::{Bounds, ContactPhase, CurveDomain, CurveEditorAction, CurveEditorView, Localizer, MessageId, UiSession};
use layer_engine::{PenEvent,PenPhase,SampleFlags,ToolKind};
use layer_core::PressureResponse;
use layer_render::CanvasRenderer;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PressureCalibrationAction {
    Apply,
    Cancel,
    Sensitivity { lighter: bool },
    Drag { phase: ContactPhase, position: [f32; 2], viewport: [f32; 2] },
    Measure { extent: [f32; 2], viewport: [f32; 2] },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PressureCalibrationView {
    pub dialog: crate::DialogIntent,
    pub bounds: Bounds,
    pub axis_gutter:String,
    pub title: String,
    pub close: String,
    pub firmer: String,
    pub lighter: String,
    pub reset: String,
    pub cancel: String,
    pub apply: String,
    pub firmer_enabled: bool,
    pub lighter_enabled: bool,
    pub editor: CurveEditorView,
}

pub(crate) type Calibration = super::response_calibration::Calibration<PressureResponse>;

impl Calibration {
    pub fn view(&self,l:&Localizer)->Option<PressureCalibrationView> {
        self.view_as(l,MessageId::PRESSURE_TITLE,MessageId::PRESSURE_FIRMER,MessageId::PRESSURE_LIGHTER,CurveDomain::Percent)
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub fn calibration_input(&mut self,event:PenEvent) {
        if !event.is_finite() {return;}
        self.prepare_tilt_input(event);
        self.prepare_pressure_input(event);
    }
    pub(crate) fn prepare_pressure_input(&mut self,event:PenEvent) {
        if let Some(view)=&mut self.state.pressure_calibration
            && !event.flags.contains(SampleFlags::PREDICTED) && !event.flags.contains(SampleFlags::CORRECTION)
            && matches!(event.tool,ToolKind::Pen|ToolKind::Eraser|ToolKind::Pencil|ToolKind::Brush|ToolKind::Airbrush) {
            let marker=if !event.pressure.is_finite() || matches!(event.phase,PenPhase::Up|PenPhase::Cancel) {None}
                else {let raw=event.pressure.clamp(0.,1.);Some([raw,self.pressure_calibration.curve().map(raw)])};
            if view.editor.marker!=marker {view.editor.marker=marker;self.pressure_meter_changed=true;}
        }
    }
    pub(crate) fn refresh_pressure_calibration(&mut self) {
        let marker = self.state.pressure_calibration.as_ref().and_then(|v| v.editor.marker);
        self.state.pressure_calibration = self.pressure_calibration.view(&self.state.localization);
        if let Some(view) = &mut self.state.pressure_calibration { view.editor.marker = marker.map(|p| [p[0], self.pressure_calibration.compiled.map(p[0])]); }
    }
    pub(crate) fn pressure_curve_action(&mut self, action: CurveEditorAction) {
        if matches!(&action, CurveEditorAction::Key { epoch, key_event, pressed: true, .. } if key_event == "Escape" && *epoch==self.pressure_calibration.epoch)
            && !self.pressure_calibration.input_busy() {
            self.pressure_action(PressureCalibrationAction::Cancel); return;
        }
        let old = self.pressure_calibration.source().cloned();
        self.pressure_calibration.action(action);
        if self.pressure_calibration.source() != old.as_ref() {
            self.pressure_calibration.compile(); self.engine.set_pressure_curve(self.pressure_calibration.curve());
        }
        self.refresh_pressure_calibration();
    }
    pub(crate) fn pressure_action(&mut self, action: PressureCalibrationAction) -> bool {
        if self.pressure_calibration.source().is_none() { return false; }
        let save = matches!(action, PressureCalibrationAction::Apply);
        let placement_only = matches!(action, PressureCalibrationAction::Measure { .. } | PressureCalibrationAction::Drag { .. });
        match action {
            PressureCalibrationAction::Apply | PressureCalibrationAction::Cancel => {
                let draft = self.pressure_calibration.close().unwrap();
                if save { self.state.settings.pressure_curve = draft; }
                self.engine.set_pressure_curve(self.state.settings.pressure_curve.clone().into());
            }
            PressureCalibrationAction::Sensitivity { lighter } => {
                self.pressure_calibration.sensitivity(lighter); self.engine.set_pressure_curve(self.pressure_calibration.curve());
            }
            PressureCalibrationAction::Measure { extent, viewport } => {
                if self.pressure_calibration.placement.is_none() {self.pressure_calibration.placement=self.response_calibration_placement(extent,viewport);}
                let area = self.response_calibration_area(viewport); self.pressure_calibration.fit(extent, area);
            },
            PressureCalibrationAction::Drag { phase, position, viewport } => { let area = self.response_calibration_area(viewport); self.pressure_calibration.drag(phase,position,area); },
        }
        if placement_only {
            if let (Some(view), Some(bounds)) = (&mut self.state.pressure_calibration, self.pressure_calibration.placement) { view.bounds = bounds; }
        } else { self.refresh_pressure_calibration(); }
        save
    }
}
