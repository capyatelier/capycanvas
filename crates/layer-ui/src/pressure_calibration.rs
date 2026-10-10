use crate::{Bounds, ContactPhase, CurveAxisView, CurveControls,
    CurveDomain, CurveEditorAction, CurveEditorView, Localizer, MessageId, UiSession};
use layer_core::PressureResponse;
use layer_engine::PressureCurve;
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
    pub bounds: Bounds,
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

struct PointDrag { baseline: PressureResponse, index: Option<usize>, press: [f32; 2], extent: [f32; 2] }

#[derive(Default)]
pub(crate) struct Calibration {
    draft: Option<PressureResponse>,
    compiled: PressureCurve,
    selected: Option<usize>,
    epoch: u64,
    point_drag: Option<PointDrag>,
    placement: Option<Bounds>,
    frame_drag: Option<(Bounds, [f32; 2])>,
}

impl Calibration {
    pub fn input_busy(&self) -> bool { self.point_drag.is_some() || self.frame_drag.is_some() }
    pub fn source(&self) -> Option<&PressureResponse> { self.draft.as_ref() }
    pub fn curve(&self) -> PressureCurve { self.compiled.clone() }
    pub fn open(&mut self, current: &PressureResponse) {
        if self.draft.is_some() { return; }
        self.draft = Some(current.clone()); self.compiled = current.clone().into(); self.selected = Some(0);
        self.epoch = self.epoch.wrapping_add(1);
    }
    pub fn close(&mut self) -> Option<PressureResponse> {
        self.point_drag = None;
        if let Some((bounds,_))=self.frame_drag.take() { self.placement=Some(bounds); }
        self.draft.take()
    }
    pub fn fit(&mut self, extent: [f32; 2], viewport: [f32; 2]) {
        if extent.iter().chain(&viewport).any(|v| !v.is_finite() || *v <= 0.) { return; }
        let mut b = self.placement.unwrap_or(Bounds { x: viewport[0] - extent[0] - 24., y: 64., width: extent[0], height: extent[1] });
        b.width = extent[0].min(viewport[0]); b.height = extent[1].min(viewport[1]);
        b.x = b.x.clamp(0., viewport[0] - b.width); b.y = b.y.clamp(0., viewport[1] - b.height);
        self.placement = Some(b);
    }
    pub fn action(&mut self, action: CurveEditorAction) {
        let Some(draft) = &mut self.draft else { return; };
        match action {
            CurveEditorAction::Contact { epoch, phase, point, extent } => {
                if epoch != self.epoch || point.iter().any(|v| !v.is_finite()) { return; }
                match phase {
                    ContactPhase::Down => {
                        if extent.iter().any(|v| !v.is_finite() || *v <= 0.) { return; }
                        let baseline = draft.clone();
                        let index = super::effects::curves::hit(draft.points(), point, extent)
                            .or_else(|| draft.insert([point[0] / extent[0], 1. - point[1] / extent[1]]));
                        self.selected = index;
                        if let Some(index) = index { self.point_drag = Some(PointDrag { baseline, index: Some(index), press: point, extent }); }
                    }
                    ContactPhase::Move | ContactPhase::Up => {
                        if let Some(drag) = &mut self.point_drag && let Some(index) = drag.index {
                            let origin = drag.baseline.points().get(index).copied()
                                .filter(|_| drag.baseline.points().len() == draft.points().len())
                                .unwrap_or([drag.press[0] / drag.extent[0], 1. - drag.press[1] / drag.extent[1]]);
                            draft.set_point(index, [origin[0] + (point[0] - drag.press[0]) / drag.extent[0],
                                origin[1] - (point[1] - drag.press[1]) / drag.extent[1]]);
                            if crate::curve_editor::dragged_outside(point, drag.extent) && draft.remove(index) {
                                self.selected = None; drag.index = None;
                            }
                        }
                        if phase == ContactPhase::Up { self.point_drag = None; }
                    }
                    ContactPhase::Cancel => if let Some(drag) = self.point_drag.take() { *draft = drag.baseline; self.selected = None; },
                }
            }
            CurveEditorAction::RemoveAt { epoch, point, extent, point_count } => {
                if epoch != self.epoch { return; }
                if point_count.is_some_and(|count| count != draft.points().len()) {
                    if let Some(drag) = self.point_drag.take() { *draft = drag.baseline; }
                    self.selected = None;
                } else if let Some(index) = super::effects::curves::hit(draft.points(), point, extent) && draft.remove(index) {
                    self.selected = None; self.point_drag = None;
                }
            }
            CurveEditorAction::Key { epoch, key_event, pressed, modifiers, .. } => {
                if epoch != self.epoch || !pressed { return; }
                if key_event == "Escape" {
                    if let Some(drag) = self.point_drag.take() { *draft = drag.baseline; self.selected = None; }
                    else if let Some((bounds,_))=self.frame_drag.take() { self.placement=Some(bounds); }
                } else if let Some(index) = self.selected.filter(|i| *i < draft.points().len()) {
                    if key_event == "Delete" || key_event == "Backspace" { if draft.remove(index) { self.selected = None; } }
                    else {
                        let step = if modifiers.shift { 0.05 } else { 0.005 };
                        let mut point = draft.points()[index];
                        match key_event.as_str() { "ArrowLeft" => point[0] -= step, "ArrowRight" => point[0] += step,
                            "ArrowUp" => point[1] += step, "ArrowDown" => point[1] -= step, _ => return }
                        draft.set_point(index, point);
                    }
                }
            }
            CurveEditorAction::Reset => { *draft = PressureResponse::default(); self.selected = Some(0); self.point_drag = None; },
            CurveEditorAction::Number { .. } | CurveEditorAction::Gesture { .. } => {},
        }
    }
    pub fn compile(&mut self) { if let Some(draft) = &self.draft { self.compiled = draft.clone().into(); } }
    pub fn view(&self, l: &Localizer) -> Option<PressureCalibrationView> {
        let draft = self.draft.as_ref()?; let points = draft.points();
        let domain = CurveDomain::Percent;
        let axis = |id| CurveAxisView { label: l.text(id).to_string(), minimum: "0%".into(), maximum: "100%".into(), white: None };
        Some(PressureCalibrationView { bounds: self.placement.unwrap_or(Bounds { x: 500., y: 64., width: 336., height: 360. }),
            title: l.text(MessageId::PRESSURE_TITLE).to_string(), close: l.text(MessageId::COMMON_CLOSE).to_string(),
            firmer: l.text(MessageId::PRESSURE_FIRMER).to_string(), lighter: l.text(MessageId::PRESSURE_LIGHTER).to_string(),
            reset: l.text(MessageId::COMMON_RESET).to_string(), cancel: l.text(MessageId::COMMON_CANCEL).to_string(), apply: l.text(MessageId::COMMON_APPLY).to_string(),
            firmer_enabled: points[0][1] > 0., lighter_enabled: points[0][1] < points[1][1],
            editor: CurveEditorView { controls: CurveControls { control_polygon:true,coordinate_readouts:false,inset:8.,epoch: self.epoch, numeric: domain.numeric(), selected: self.selected,
                input: None, output: None, axes: [axis(MessageId::RESOURCES_SECTION_LEVELS_INPUT), axis(MessageId::RESOURCES_SECTION_LEVELS_OUTPUT)],
                domain, help: l.text(MessageId::PRESSURE_CURVE_HELP).to_string(), reset_label: l.text(MessageId::COMMON_RESET).to_string() },
                points: points.to_vec(), plot: (0..=256).map(|i| { let x = i as f32 / 256.; [x, self.compiled.map(x)] }).collect(),
                marker: None, modified: draft != &PressureResponse::default() } })
    }
}

impl<R: CanvasRenderer> UiSession<R> {
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
                let draft = self.pressure_calibration.draft.as_mut().unwrap(); let mut first = draft.points()[0];
                first[1] += if lighter { 0.025 } else { -0.025 };
                draft.set_point(0, first); self.pressure_calibration.selected = Some(0);
                self.pressure_calibration.compile(); self.engine.set_pressure_curve(self.pressure_calibration.curve());
            }
            PressureCalibrationAction::Measure { extent, viewport } => self.pressure_calibration.fit(extent, viewport),
            PressureCalibrationAction::Drag { phase, position, viewport } => {
                if position.iter().chain(&viewport).any(|v| !v.is_finite()) { return false; }
                match phase {
                    ContactPhase::Down => { self.pressure_calibration.frame_drag = self.pressure_calibration.placement.map(|b| (b, position)); }
                    ContactPhase::Move | ContactPhase::Up => if let Some((mut b, press)) = self.pressure_calibration.frame_drag {
                        b.x += position[0] - press[0]; b.y += position[1] - press[1]; self.pressure_calibration.placement = Some(b);
                        if phase == ContactPhase::Up { self.pressure_calibration.fit([b.width, b.height], viewport); self.pressure_calibration.frame_drag = None; }
                    },
                    ContactPhase::Cancel => if let Some((b, _)) = self.pressure_calibration.frame_drag.take() { self.pressure_calibration.placement = Some(b); },
                }
            }
        }
        if placement_only {
            if let (Some(view), Some(bounds)) = (&mut self.state.pressure_calibration, self.pressure_calibration.placement) { view.bounds = bounds; }
        } else { self.refresh_pressure_calibration(); }
        save
    }
}
