use super::*;
use layer_core::{TiltAxes, TiltCalibration, TiltResponse};
use layer_engine::{PenEvent, PenPhase, SampleFlags, TiltCurve, ToolKind};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PenDeviceInfo {
    pub id:u64,
    pub key:String,
    pub label:String,
    pub native_to_surface:TiltAxes,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag="kind",rename_all="snake_case")]
pub enum TiltCalibrationAction {
    Panel { action:PressureCalibrationAction },
    Axes { axes:TiltAxes },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TiltCalibrationView {
    #[serde(flatten)]
    pub panel:PressureCalibrationView,
    pub device:String,
    pub ready:bool,
    pub axes:TiltAxes,
    pub sensitivity:String,
    pub direction:String,
    pub reverse_horizontal:String,
    pub reverse_vertical:String,
    pub swap_axes:String,
    pub input_direction:Option<[f32;2]>,
    pub adjusted_direction:Option<[f32;2]>,
}

struct Device { info:PenDeviceInfo, curve:TiltCurve }
#[derive(Default)]
pub(crate) struct Calibration {
    pub(super) editor:super::response_calibration::Calibration<TiltResponse>,
    axes:TiltAxes,
    devices:BTreeMap<u64,Device>,
    last:Option<PenDeviceInfo>,
    target:Option<PenDeviceInfo>,
    unbound_response:bool,
    unbound_drag:Option<bool>,
    unbound_axes:bool,
    current:Option<u64>,
    neutral:TiltCurve,
}

impl<R:CanvasRenderer> UiSession<R> {
    pub fn register_pen_device(&mut self,info:&PenDeviceInfo) {
        if info.key.is_empty() || self.tilt_calibration.devices.get(&info.id).is_some_and(|device|device.info==*info) { return; }
        self.tilt_calibration.devices.retain(|id,device|*id==info.id || device.info.key!=info.key);
        if let Some(target)=&mut self.tilt_calibration.target && target.key==info.key {*target=info.clone();}
        if let Some(last)=&mut self.tilt_calibration.last && last.key==info.key {*last=info.clone();}
        if let Some(device)=self.tilt_calibration.devices.get_mut(&info.id) {device.info=info.clone();}
        else {
            let curve=self.tilt_calibration.neutral.clone();
            self.tilt_calibration.devices.insert(info.id,Device{info:info.clone(),curve});
        }
        self.sync_tilt_curves();
        self.refresh_tilt_calibration();
    }
    fn bind_tilt_target(&mut self,info:PenDeviceInfo) {
        let calibration=self.state.settings.tilt_calibrations.get(&info.key).cloned().unwrap_or_default();
        let frame=info.native_to_surface;
        self.tilt_calibration.target=Some(info);
        if self.tilt_calibration.unbound_axes {
            self.tilt_calibration.axes=self.tilt_calibration.axes.in_frame(frame.inverse());
        } else {self.tilt_calibration.axes=calibration.axes;}
        if !self.tilt_calibration.unbound_response {
            self.tilt_calibration.editor.replace_response(&calibration.response);
            self.tilt_calibration.unbound_drag=None;
            self.tilt_calibration.editor.selected=Some(calibration.response.points().len()-1);
        }
        self.tilt_calibration.unbound_response=false;self.tilt_calibration.unbound_axes=false;self.sync_tilt_curves();
    }
    pub(crate) fn open_tilt_calibration(&mut self) {
        self.state.settings_open=false;self.state.preferences=PreferencesState::default();
        if self.tilt_calibration.editor.source().is_some(){return;}
        if self.pressure_calibration.source().is_some() {self.pressure_action(PressureCalibrationAction::Cancel);}
        self.tilt_calibration.axes=TiltAxes::default();self.tilt_calibration.unbound_response=false;self.tilt_calibration.unbound_axes=false;self.tilt_calibration.unbound_drag=None;
        self.tilt_calibration.editor.open(&TiltResponse::default());
        if let Some(info)=self.tilt_calibration.last.clone() {self.bind_tilt_target(info);}
        self.refresh_tilt_calibration();
    }
    pub(crate) fn sync_tilt_curves(&mut self) {
        let draft=self.tilt_calibration.target.as_ref()
            .and_then(|info|self.tilt_calibration.editor.source().map(|_|(info.key.clone(),self.tilt_calibration.editor.curve(),self.tilt_calibration.axes)));
        let neutral=TiltCalibration::default();
        for device in self.tilt_calibration.devices.values_mut() {
            if let Some((_,curve,axes))=draft.as_ref().filter(|(key,_,_)|*key==device.info.key) {
                device.curve=curve.with_axes(*axes,device.info.native_to_surface);
            } else {
                let source=self.state.settings.tilt_calibrations.get(&device.info.key).unwrap_or(&neutral);
                device.curve=if device.curve.response_matches(&source.response) {device.curve.with_axes(source.axes,device.info.native_to_surface)}
                    else {TiltCurve::new(source,device.info.native_to_surface)};
            }
        }
        let curve=self.tilt_calibration.current.and_then(|id|self.tilt_calibration.devices.get(&id))
            .map_or(&self.tilt_calibration.neutral,|device|&device.curve);
        self.engine.set_tilt_curve(curve.clone());
    }
    pub(crate) fn prepare_tilt_input(&mut self,event:PenEvent) {
        if event.tilt_radians.iter().any(|v|!v.is_finite()) || !matches!(event.tool,ToolKind::Pen|ToolKind::Eraser|ToolKind::Pencil|ToolKind::Brush|ToolKind::Airbrush)
            || event.flags.contains(SampleFlags::PREDICTED) || event.flags.contains(SampleFlags::CORRECTION) { return; }
        if let Some(device)=self.tilt_calibration.devices.get(&event.device_id) {
            if self.tilt_calibration.last.as_ref()!=Some(&device.info) {self.tilt_calibration.last=Some(device.info.clone());}
            if self.tilt_calibration.editor.source().is_some() && self.tilt_calibration.target.is_none() && self.tilt_calibration.editor.point_drag.is_none() {
                self.bind_tilt_target(self.tilt_calibration.last.clone().unwrap());self.refresh_tilt_calibration();
            }
        }
        if self.tilt_calibration.current!=Some(event.device_id) {
            self.tilt_calibration.current=Some(event.device_id);
            let curve=self.tilt_calibration.devices.get(&event.device_id).map_or(&self.tilt_calibration.neutral,|device|&device.curve);
            self.engine.set_tilt_curve(curve.clone());
        }
        if self.tilt_calibration.target.as_ref().is_none_or(|target|self.tilt_calibration.devices.get(&event.device_id).is_none_or(|device|device.info.key!=target.key)) {return;}
        let Some(view)=&mut self.state.tilt_calibration else{return};
        let input=self.tilt_calibration.neutral.projected(event.tilt_radians);
        let output=self.tilt_calibration.devices.get(&event.device_id).unwrap().curve.projected(event.tilt_radians);
        let normalized=|v:[f32;2]|v.map(|axis|axis/std::f32::consts::FRAC_PI_2);
        let (input,output)=(normalized(input),normalized(output));
        let marker=if event.phase==PenPhase::Cancel {None}else{Some([input[0].hypot(input[1]),output[0].hypot(output[1])])};
        let direction=if marker.is_some(){Some(input)}else{None};
        let adjusted=if marker.is_some(){Some(output)}else{None};
        if view.panel.editor.marker!=marker || view.input_direction!=direction || view.adjusted_direction!=adjusted {
            view.panel.editor.marker=marker;view.input_direction=direction;view.adjusted_direction=adjusted;
            self.tilt_meter_changed=true;
        }
    }
    pub(crate) fn refresh_tilt_calibration(&mut self) {
        let old=self.state.tilt_calibration.take();
        let editor=&self.tilt_calibration.editor;let l=&self.state.localization;
        let Some(mut panel)=editor.view_as(l,MessageId::TILT_TITLE,MessageId::TILT_LESS_SENSITIVE,MessageId::TILT_MORE_SENSITIVE,CurveDomain::Degrees) else{return};
        let device=self.tilt_calibration.target.as_ref();
        if editor.placement.is_none() {panel.bounds.height=440.;}
        let mut view=TiltCalibrationView {panel,device:device.map_or_else(||l.text(MessageId::TILT_USE_PEN).to_string(),|d|if d.label.is_empty(){l.text(MessageId::COMMAND_PEN).to_string()}else{d.label.clone()}),ready:device.is_some(),
            axes:device.map_or(self.tilt_calibration.axes,|device|self.tilt_calibration.axes.in_frame(device.native_to_surface)),
            sensitivity:l.text(MessageId::TILT_SENSITIVITY).to_string(),
            direction:l.text(MessageId::TILT_DIRECTION).to_string(),reverse_horizontal:l.text(MessageId::TILT_REVERSE_HORIZONTAL).to_string(),
            reverse_vertical:l.text(MessageId::TILT_REVERSE_VERTICAL).to_string(),swap_axes:l.text(MessageId::TILT_SWAP_AXES).to_string(),
            input_direction:None,adjusted_direction:None};
        if let Some(old)=old {
            view.input_direction=old.input_direction;
            if let Some(input)=view.input_direction {
                let radial=input.map(|v|v*std::f32::consts::FRAC_PI_2);
                let output=device.map_or(radial,|d|self.tilt_calibration.editor.curve().with_axes(self.tilt_calibration.axes,d.native_to_surface).map(radial));
                let output=output.map(|v|v/std::f32::consts::FRAC_PI_2);
                view.adjusted_direction=Some(output);view.panel.editor.marker=Some([input[0].hypot(input[1]),output[0].hypot(output[1])]);
            }
        }
        self.state.tilt_calibration=Some(Box::new(view));self.tilt_meter_changed=true;
    }
    pub(crate) fn tilt_curve_action(&mut self,action:CurveEditorAction) {
        if matches!(&action,CurveEditorAction::Key{epoch,key_event,pressed:true,..}if key_event=="Escape"&&*epoch==self.tilt_calibration.editor.epoch)
            && !self.tilt_calibration.editor.input_busy() {
            self.tilt_action(TiltCalibrationAction::Panel{action:PressureCalibrationAction::Cancel});return;
        }
        let old=self.tilt_calibration.editor.source().cloned();let reset=matches!(action,CurveEditorAction::Reset);
        let dragging=self.tilt_calibration.editor.point_drag.is_some();
        let unbound_response=self.tilt_calibration.unbound_response;
        let cancelled=self.tilt_calibration.editor.action(action);
        if !dragging && self.tilt_calibration.editor.point_drag.is_some() {self.tilt_calibration.unbound_drag=Some(unbound_response);}
        if reset {self.tilt_calibration.axes=TiltAxes::default();}
        if reset || self.tilt_calibration.editor.source()!=old.as_ref() {
            self.tilt_calibration.unbound_response|=self.tilt_calibration.target.is_none();
            self.tilt_calibration.unbound_axes|=reset && self.tilt_calibration.target.is_none();
            self.tilt_calibration.editor.compile();self.sync_tilt_curves();
        }
        if self.tilt_calibration.editor.point_drag.is_none()
            && let Some(previous)=self.tilt_calibration.unbound_drag.take()
            && dragging && cancelled && self.tilt_calibration.target.is_none() {self.tilt_calibration.unbound_response=previous;}
        if self.tilt_calibration.target.is_none() && self.tilt_calibration.editor.source().is_some()
            && self.tilt_calibration.editor.point_drag.is_none() && let Some(info)=self.tilt_calibration.last.clone() {self.bind_tilt_target(info);}
        self.refresh_tilt_calibration();
    }
    pub(crate) fn tilt_action(&mut self,action:TiltCalibrationAction)->bool {
        if self.tilt_calibration.editor.source().is_none(){return false;}
        let mut save=false;
        let placement_only=matches!(&action,TiltCalibrationAction::Panel{action:PressureCalibrationAction::Measure{..}|PressureCalibrationAction::Drag{..}});
        match action {
            TiltCalibrationAction::Axes{axes}=>{
                let frame=self.tilt_calibration.target.as_ref().map_or_default(|device|device.native_to_surface);
                self.tilt_calibration.axes=axes.in_frame(frame.inverse());
                self.tilt_calibration.unbound_axes|=self.tilt_calibration.target.is_none();
                self.sync_tilt_curves();
            }
            TiltCalibrationAction::Panel{action}=>match action {
                PressureCalibrationAction::Apply|PressureCalibrationAction::Cancel=>{
                    if matches!(action,PressureCalibrationAction::Apply) && self.tilt_calibration.target.is_none(){return false;}
                    let response=self.tilt_calibration.editor.close().unwrap();
                    if matches!(action,PressureCalibrationAction::Apply) {
                        let device=self.tilt_calibration.target.as_ref().unwrap();
                        self.state.settings.tilt_calibrations.insert(device.key.clone(),TiltCalibration{response,axes:self.tilt_calibration.axes});save=true;
                    }
                    self.tilt_calibration.target=None;self.tilt_calibration.unbound_response=false;self.tilt_calibration.unbound_axes=false;self.tilt_calibration.unbound_drag=None;self.sync_tilt_curves();
                }
                PressureCalibrationAction::Sensitivity{lighter}=>{
                    self.tilt_calibration.unbound_response|=self.tilt_calibration.target.is_none();
                    self.tilt_calibration.editor.sensitivity(lighter);self.sync_tilt_curves();
                },
                PressureCalibrationAction::Measure{extent,viewport}=>{
                    if self.tilt_calibration.editor.placement.is_none() {self.tilt_calibration.editor.placement=self.response_calibration_placement(extent,viewport);}
                    self.tilt_calibration.editor.fit(extent,viewport);
                },
                PressureCalibrationAction::Drag{phase,position,viewport}=>self.tilt_calibration.editor.drag(phase,position,viewport),
            },
        }
        if placement_only {
            if let (Some(view),Some(bounds))=(&mut self.state.tilt_calibration,self.tilt_calibration.editor.placement){view.panel.bounds=bounds;}
        } else {self.refresh_tilt_calibration();}save
    }
}
