use crate::{Bounds, ContactPhase, CurveEditorAction, CurveDomain, CurveAxisView, CurveControls,
    CurveEditorView, Localizer, MessageId, PressureCalibrationView, UiSession};
use layer_core::{PressureResponse, TiltResponse};
use layer_engine::{PressureCurve, TiltCurve};

pub(crate) trait EditableResponse: Clone + PartialEq + Default {
    type Compiled: Clone + Default;
    fn points(&self) -> Vec<[f32;2]>;
    fn set_point(&mut self, index:usize, point:[f32;2]) -> bool;
    fn insert(&mut self, point:[f32;2]) -> Option<usize>;
    fn remove(&mut self, index:usize) -> bool;
    fn compile(&self) -> Self::Compiled;
    fn map(compiled:&Self::Compiled,input:f32)->f32;
    fn sensitivity_enabled(&self,more:bool)->bool;
    fn sensitivity(&mut self,more:bool)->usize;
}
impl EditableResponse for PressureResponse {
    type Compiled=PressureCurve;
    fn points(&self)->Vec<[f32;2]> { self.points().to_vec() }
    fn set_point(&mut self,index:usize,point:[f32;2])->bool { self.set_point(index,point) }
    fn insert(&mut self,point:[f32;2])->Option<usize> { self.insert(point) }
    fn remove(&mut self,index:usize)->bool { self.remove(index) }
    fn compile(&self)->PressureCurve { self.clone().into() }
    fn map(compiled:&PressureCurve,input:f32)->f32 { compiled.map(input) }
    fn sensitivity_enabled(&self,more:bool)->bool { if more { self.points()[0][1]<self.points()[1][1] } else { self.points()[0][1]>0. } }
    fn sensitivity(&mut self,more:bool)->usize {
        let mut first=self.points()[0];first[1]+=if more {0.025}else{-0.025};self.set_point(0,first);0
    }
}
impl EditableResponse for TiltResponse {
    type Compiled=TiltCurve;
    fn points(&self)->Vec<[f32;2]> { self.points() }
    fn set_point(&mut self,index:usize,point:[f32;2])->bool { self.set_point(index,point) }
    fn insert(&mut self,point:[f32;2])->Option<usize> { self.insert(point) }
    fn remove(&mut self,index:usize)->bool { self.remove(index) }
    fn compile(&self)->TiltCurve { TiltCurve::new(&layer_core::TiltCalibration{response:self.clone(),axes:Default::default()},Default::default()) }
    fn map(compiled:&TiltCurve,input:f32)->f32 { compiled.response(input) }
    fn sensitivity_enabled(&self,more:bool)->bool { self.sensitivity_enabled(more) }
    fn sensitivity(&mut self,more:bool)->usize { self.sensitivity(more);self.points().len()-1 }
}

pub(super) struct PointDrag<R> { baseline: R, index: Option<usize>, press: [f32; 2], extent: [f32; 2] }

pub(crate) struct Calibration<R: EditableResponse> {
    pub(super) draft: Option<R>,
    pub(super) compiled: R::Compiled,
    pub(super) selected: Option<usize>,
    pub(super) epoch: u64,
    pub(super) point_drag: Option<PointDrag<R>>,
    pub(super) placement: Option<Bounds>,
    pub(super) frame_drag: Option<(Bounds, [f32; 2])>,
}

impl<R: EditableResponse> Default for Calibration<R> {
    fn default() -> Self { Self { draft:None, compiled:Default::default(), selected:None, epoch:0, point_drag:None, placement:None, frame_drag:None } }
}

impl<R: EditableResponse> Calibration<R> {
    pub fn sensitivity(&mut self,more:bool) {
        if let Some(draft)=&mut self.draft { self.selected=Some(draft.sensitivity(more));self.compile(); }
    }
    pub fn drag(&mut self,phase:ContactPhase,position:[f32;2],area:Bounds) {
        if position.iter().chain(&[area.x,area.y,area.width,area.height]).any(|v|!v.is_finite()) { return; }
        match phase {
            ContactPhase::Down=>self.frame_drag=self.placement.map(|b|(b,position)),
            ContactPhase::Move|ContactPhase::Up=>if let Some((mut b,press))=self.frame_drag {
                b.x+=position[0]-press[0];b.y+=position[1]-press[1];self.placement=Some(b);
                self.fit([b.width,b.height],area);
                if phase==ContactPhase::Up {self.frame_drag=None;}
            },
            ContactPhase::Cancel=>if let Some((b,_))=self.frame_drag.take() {self.placement=Some(b);},
        }
    }
    pub fn view_as(&self,l:&Localizer,title:MessageId,less:MessageId,more:MessageId,domain:CurveDomain)->Option<PressureCalibrationView> {
        let draft=self.source()?;let points=draft.points();
        let axis=|id|CurveAxisView {label:l.text(id).to_string(),minimum:domain.axis_text(0.),maximum:domain.axis_text(1.),white:None};
        Some(PressureCalibrationView {bounds:self.placement.unwrap_or(Bounds{x:500.,y:64.,width:384.,height:360.}),
            axis_gutter:CurveDomain::Percent.axis_text(1.),
            title:l.text(title).to_string(),close:l.text(MessageId::COMMON_CLOSE).to_string(),firmer:l.text(less).to_string(),lighter:l.text(more).to_string(),
            reset:l.text(MessageId::COMMON_RESET).to_string(),cancel:l.text(MessageId::COMMON_CANCEL).to_string(),apply:l.text(MessageId::COMMON_APPLY).to_string(),
            firmer_enabled:draft.sensitivity_enabled(false),lighter_enabled:draft.sensitivity_enabled(true),
            editor:CurveEditorView {controls:CurveControls {control_polygon:true,coordinate_readouts:false,inset:8.,epoch:self.epoch,numeric:domain.numeric(),selected:self.selected,
                input:None,output:None,axes:[axis(MessageId::RESOURCES_SECTION_LEVELS_INPUT),axis(MessageId::RESOURCES_SECTION_LEVELS_OUTPUT)],domain,
                help:l.text(MessageId::PRESSURE_CURVE_HELP).to_string(),reset_label:l.text(MessageId::COMMON_RESET).to_string()},
                points,plot:(0..=256).map(|i|{let x=i as f32/256.;[x,R::map(&self.compiled,x)]}).collect(),marker:None,modified:draft!=&R::default()}})
    }
    pub fn input_busy(&self) -> bool { self.point_drag.is_some() || self.frame_drag.is_some() }
    pub fn source(&self) -> Option<&R> { self.draft.as_ref() }
    pub fn curve(&self) -> R::Compiled { self.compiled.clone() }
    pub fn open(&mut self, current: &R) {
        if self.draft.is_some() { return; }
        self.epoch = self.epoch.wrapping_add(1);
        self.placement=None;
        self.replace_response(current);
    }
    pub fn replace_response(&mut self,current:&R) {
        self.draft = Some(current.clone()); self.compiled = current.compile(); self.selected = Some(0);
        self.point_drag=None;
    }
    pub fn close(&mut self) -> Option<R> {
        self.point_drag = None;
        if let Some((bounds,_))=self.frame_drag.take() { self.placement=Some(bounds); }
        self.draft.take()
    }
    pub fn fit(&mut self, extent: [f32; 2], area: Bounds) {
        if extent.iter().chain(&[area.width,area.height]).any(|v| !v.is_finite() || *v <= 0.) { return; }
        let mut b = self.placement.unwrap_or_default();
        b.width = extent[0].min(area.width); b.height = extent[1].min(area.height);
        b.x = b.x.clamp(area.x, area.x + area.width - b.width); b.y = b.y.clamp(area.y, area.y + area.height - b.height);
        self.placement = Some(b);
    }
    pub fn action(&mut self, action: CurveEditorAction) -> bool {
        let Some(draft) = &mut self.draft else { return false; };
        let mut cancelled=false;
        match action {
            CurveEditorAction::Contact { epoch, phase, point, extent } => {
                if epoch != self.epoch || point.iter().any(|v| !v.is_finite()) { return false; }
                match phase {
                    ContactPhase::Down => {
                        if extent.iter().any(|v| !v.is_finite() || *v <= 0.) { return false; }
                        let baseline = draft.clone();
                        let index = super::effects::curves::hit(&draft.points(), point, extent)
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
                    ContactPhase::Cancel => if let Some(drag) = self.point_drag.take() { *draft = drag.baseline; self.selected = None; cancelled=true; },
                }
            }
            CurveEditorAction::RemoveAt { epoch, point, extent, point_count } => {
                if epoch != self.epoch { return false; }
                if point_count.is_some_and(|count| count != draft.points().len()) {
                    if let Some(drag) = self.point_drag.take() { *draft = drag.baseline; cancelled=true; }
                    self.selected = None;
                } else if let Some(index) = super::effects::curves::hit(&draft.points(), point, extent) && draft.remove(index) {
                    self.selected = None; self.point_drag = None;
                }
            }
            CurveEditorAction::Key { epoch, key_event, pressed, modifiers, .. } => {
                if epoch != self.epoch || !pressed { return false; }
                if key_event == "Escape" {
                    if let Some(drag) = self.point_drag.take() { *draft = drag.baseline; self.selected = None; cancelled=true; }
                    else if let Some((bounds,_))=self.frame_drag.take() { self.placement=Some(bounds); }
                } else if let Some(index) = self.selected.filter(|i| *i < draft.points().len()) {
                    if key_event == "Delete" || key_event == "Backspace" { if draft.remove(index) { self.selected = None; } }
                    else {
                        let step = if modifiers.shift { 0.05 } else { 0.005 };
                        let mut point = draft.points()[index];
                        match key_event.as_str() { "ArrowLeft" => point[0] -= step, "ArrowRight" => point[0] += step,
                            "ArrowUp" => point[1] += step, "ArrowDown" => point[1] -= step, _ => return false }
                        draft.set_point(index, point);
                    }
                }
            }
            CurveEditorAction::Reset => { *draft = R::default(); self.selected = Some(0); self.point_drag = None; },
            CurveEditorAction::Number { .. } | CurveEditorAction::Gesture { .. } => {},
        }
        cancelled
    }
    pub fn compile(&mut self) { if let Some(draft) = &self.draft { self.compiled = draft.compile(); } }
}

impl<R: layer_render::CanvasRenderer> UiSession<R> {
    pub(crate) fn response_calibration_area(&self, viewport: [f32; 2]) -> Bounds {
        self.phone_layout().map_or(Bounds { x: 0., y: 0., width: viewport[0], height: viewport[1] }, |phone| phone.work_area)
    }
    pub(crate) fn response_calibration_placement(&self,extent:[f32;2],viewport:[f32;2])->Option<Bounds> {
        if extent.iter().chain(&viewport).any(|v|!v.is_finite() || *v<=0.) {return None;}
        if let Some(phone) = self.phone_layout() { return crate::DialogPresentation::CanvasPanel(phone.work_area).bounds(extent); }
        let layout=self.layout(viewport);let mut area=layout.work_area;
        if layout.status.height>0. {area.height=area.height.min((layout.status.y-area.y).max(0.));}
        let mut obstacles:Vec<_>=layout.groups.iter().map(|g|g.bounds)
            .chain(layout.collapsed.iter().filter_map(|c|c.open.as_ref().map(|o|o.bounds))).collect();
        let customization=&self.state.customization;
        for drawer in customization.column_drawers.iter().chain(customization.drawer.iter()) {
            let measured=if let crate::DrawerAnchor::Column{group,..}=drawer.anchor {
                customization.column_drawer_bounds.iter().find(|m|m.group==group).map(|m|m.bounds)
            } else {None};
            if let Some(bounds)=measured.or_else(||customization.drawer_placement(drawer,&self.layout_resolve_input(),viewport,&vec![viewport[1];drawer.columns.len()]).map(|p|p.bounds)) {
                obstacles.push(bounds);
            }
        }
        if let Some(panel)=customization.expanded
            && let Some(expansion)=self.state.workspace.layout.expanded_panel(viewport,panel,[0.;2],1.) {obstacles.push(expansion.bounds);}
        Some(place_response_calibration(area,&obstacles,extent,viewport))
    }
}

fn place_response_calibration(area:Bounds,obstacles:&[Bounds],extent:[f32;2],viewport:[f32;2])->Bounds {
    let margin=16.;let [width,height]=[extent[0].min(viewport[0]),extent[1].min(viewport[1])];
    let range=|start:f32,length:f32,size:f32,limit:f32| {
        let low=(start+margin).clamp(0.,limit-size);let high=(start+length-size-margin).clamp(0.,limit-size);
        if low<=high {(low,high)} else {let center=(start+(length-size)*0.5).clamp(0.,limit-size);(center,center)}
    };
    let (left,right)=range(area.x,area.width,width,viewport[0]);let (top,bottom)=range(area.y,area.height,height,viewport[1]);
    let padded:Vec<_>=obstacles.iter().map(|b|Bounds{x:b.x-margin,y:b.y-margin,width:b.width+2.*margin,height:b.height+2.*margin}).collect();
    let mut xs=vec![right,left];let mut ys=vec![top,bottom];
    for b in &padded {xs.extend([(b.x-width).clamp(left,right),(b.x+b.width).clamp(left,right)]);ys.extend([(b.y-height).clamp(top,bottom),(b.y+b.height).clamp(top,bottom)]);}
    let score=|b:Bounds| {
        let overlap:f32=padded.iter().filter_map(|o|b.intersection(*o)).map(|b|b.width*b.height).sum();
        (overlap,(b.x-right).powi(2)+(b.y-top).powi(2))
    };
    xs.into_iter().flat_map(|x|ys.iter().map(move |&y|Bounds{x,y,width,height}))
        .min_by(|a,b|{let (oa,da)=score(*a);let (ob,db)=score(*b);oa.total_cmp(&ob).then_with(||da.total_cmp(&db))}).unwrap()
}

#[cfg(test)]
mod placement_tests {
    use super::*;
    #[test]
    fn response_placement_avoids_floating_panels_and_drawers_with_a_common_margin() {
        let area=Bounds{x:300.,y:100.,width:1000.,height:800.};let viewport=[1600.,1000.];let extent=[384.,600.];
        let panel=Bounds{x:1000.,y:100.,width:280.,height:700.};
        let placed=place_response_calibration(area,&[panel],extent,viewport);
        assert_eq!(placed.x+placed.width,panel.x-16.);assert_eq!(placed.y,area.y+16.);
        let drawer=Bounds{x:800.,y:100.,width:300.,height:80.};
        let placed=place_response_calibration(area,&[drawer],extent,viewport);
        assert_eq!(placed.y,drawer.y+drawer.height+16.);assert_eq!(placed.x+placed.width,area.x+area.width-16.);
    }
    #[test]
    fn response_placement_keeps_controls_inside_a_window_when_the_canvas_is_too_small() {
        for viewport in [[320.,360.],[640.,480.],[1600.,1000.]] {
            let area=Bounds{x:viewport[0]-100.,y:60.,width:80.,height:viewport[1]-100.};
            let placed=place_response_calibration(area,&[],[384.,622.],viewport);
            assert!(placed.x>=0. && placed.y>=0. && placed.x+placed.width<=viewport[0] && placed.y+placed.height<=viewport[1]);
        }
    }
}
