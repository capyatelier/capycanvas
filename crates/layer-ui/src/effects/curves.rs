use crate::{NumericControl, NumericKind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag="kind",rename_all="snake_case")]
pub enum CurveDomain { Encoded, LogHdr { stops:f32 } }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum CurveAxis { Input, Output }
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CurveCoordinateControl {
    pub value:f64,
    pub text:String,
    pub ev:Option<String>,
    pub read_only:bool,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CurveAxisView { pub label:String, pub minimum:String, pub maximum:String, pub white:Option<f32> }
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CurveControls {
    pub epoch:u64,
    pub numeric:NumericControl,
    pub selected:Option<usize>,
    pub input:Option<CurveCoordinateControl>,
    pub output:Option<CurveCoordinateControl>,
    pub axes:[CurveAxisView;2],
    pub domain:CurveDomain,
    pub help:String,
    pub reset_label:String,
}
impl CurveDomain {
    pub fn decode(self,x:f64)->f64 {
        match self {
            Self::Encoded=>x,
            Self::LogHdr{stops}=>layer_core::log_curve_decode(x,f64::from(stops))
        }
    }
    pub fn encode(self,value:f64)->f64 {
        match self {
            Self::Encoded=>value,
            Self::LogHdr{stops}=>layer_core::log_curve_encode(value,f64::from(stops))
        }
    }
    pub fn numeric(self)->NumericControl {
        match self {
            Self::Encoded=>{let mut n=NumericControl::number(0.,1.,1./255.,3);n.kind=NumericKind::Number;n.scale=255.;n.resolution=1./255000.;n},
            Self::LogHdr{stops}=>{let mut n=NumericControl::number(0.,f64::from(stops).exp2(),0.01,3);n.kind=NumericKind::Number;n.resolution=2f64.powi(-149);n}
        }
    }
    pub fn axis_text(self,x:f32)->String {
        let value=self.decode(f64::from(x));
        match self {
            Self::Encoded=>format!("{:.0}",value*255.),
            Self::LogHdr{..}=>if value!=0. && (value.abs()<1e-4 || value.abs()>=1e6) {format!("{value:.3e}")}else{format!("{value:.3}").trim_end_matches('0').trim_end_matches('.').to_string()},
        }
    }
    pub fn text(self,x:f32)->String {
        let value=self.decode(f64::from(x));
        match self {
            Self::Encoded=>format!("{:.3}",value*255.),
            Self::LogHdr{..}=>if value!=0. && (value.abs()<1e-4 || value.abs()>=1e6) {format!("{value:e}")}else{value.to_string()},
        }
    }
}
pub(super) use layer_core::curves::curve_point_between as point_between;
pub(super) fn hit(points:&[[f32;2]],point:[f32;2],extent:[f32;2])->Option<usize> {
    if extent.iter().any(|x|!x.is_finite() || *x<=0.) || point.iter().any(|x|!x.is_finite()) {return None;}
    points.iter().enumerate().filter_map(|(index,p)| {
        let dx=p[0]*extent[0]-point[0];let dy=(1.-p[1])*extent[1]-point[1];let distance=dx*dx+dy*dy;
        (distance<=64.).then_some((index,distance))
    }).min_by(|a,b|a.1.total_cmp(&b.1).then(a.0.cmp(&b.0))).map(|(index,_)|index)
}
pub(super) fn numeric_point(points:&[[f32;2]],index:usize,axis:CurveAxis,domain:CurveDomain,value:f64)->Option<[f32;2]> {
    let mut point=*points.get(index)?;
    if !value.is_finite(){return None;}
    let graph=domain.encode(value);
    if !graph.is_finite(){return None;}
    match axis {
        CurveAxis::Input=>{if index==0 || index+1==points.len(){return None;}point[0]=point_between(graph as f32,points[index-1][0],points[index+1][0])?;},
        CurveAxis::Output=>point[1]=graph.clamp(0.,1.) as f32,
    }Some(point)
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn log_toe_and_physical_text_preserve_small_positive_values() {
        let domain=CurveDomain::LogHdr{stops:127.};
        for physical in [2f64.powi(-149),1e-20,0.001,1.,2f64.powi(127)] {
            let graph=domain.encode(physical);let decoded=domain.decode(graph);assert!((decoded/physical-1.).abs()<1e-12);
        }
        let x=domain.encode(1e-20) as f32;let text=domain.text(x);assert!(text.contains('e'));assert!(text.parse::<f64>().unwrap()>0.);
        assert_eq!(domain.decode(0.),0.);
    }
    #[test] fn axis_labels_are_compact_without_changing_edit_text() {
        assert_eq!(CurveDomain::Encoded.axis_text(1.),"255");
        let domain=CurveDomain::LogHdr{stops:127.};assert_eq!(domain.axis_text(1.),"1.701e38");assert_eq!(domain.axis_text(0.),"0");
        assert_ne!(domain.text(1.),domain.axis_text(1.));
    }
    #[test] fn neighbors_require_a_representable_interior() {
        let x=0.5f32;assert_eq!(point_between(x,x,x.next_up()),None);
        assert_eq!(point_between(0.,x,x.next_up().next_up()),Some(x.next_up()));
    }
    #[test] fn closest_hit_uses_logical_pixels_and_lower_tie_index() {
        let p=[[0.4,0.5],[0.6,0.5]];assert_eq!(hit(&p,[50.,50.],[100.,100.]),None);
        assert_eq!(hit(&p,[5.,5.],[10.,10.]),Some(0));assert_eq!(hit(&p,[60.,50.],[100.,100.]),Some(1));
    }
    #[test] fn numeric_endpoint_input_is_refused_without_mutating_points() {
        let points=[[0.,0.],[0.5,0.5],[1.,1.]];assert!(numeric_point(&points,0,CurveAxis::Input,CurveDomain::Encoded,0.1).is_none());
        assert_eq!(numeric_point(&points,1,CurveAxis::Output,CurveDomain::Encoded,2.),Some([0.5,1.]));
    }
}

struct CurveCapture { key:String,index:usize,extent:[f32;2],press:[f32;2],graph:[f32;2] }
#[derive(Default)]
pub(crate) struct PropertyEditorState {
    document:Option<u64>,
    layer:Option<u64>,
    revision:u64,
    pages:Vec<String>,
    page:Option<String>,
    pub epoch:u64,
    selected:Option<(String,usize)>,
    contact:Option<CurveCapture>,
    pressed_key:Option<String>,
}
impl PropertyEditorState {
    pub fn sync(&mut self,document:u64,layer:Option<u64>,revision:u64,pages:Vec<String>,gesture_active:bool) {
        if self.document.as_ref()!=Some(&document) || self.layer!=layer || self.pages!=pages {
            self.document=Some(document);
            self.layer=layer;self.pages=pages;self.page=self.pages.first().cloned();self.selected=None;
            self.invalidate();
        } else if self.revision!=revision && !gesture_active {self.invalidate();}
        self.revision=revision;
    }
    fn invalidate(&mut self) {self.epoch=self.epoch.wrapping_add(1);self.contact=None;self.pressed_key=None;}
    pub fn accepts(&self,layer:u64,epoch:u64)->bool {self.layer==Some(layer) && self.epoch==epoch}
    pub fn commit_revision(&mut self,revision:u64) {self.revision=revision;}
    pub fn page(&self)->Option<&str> {self.page.as_deref()}
    pub fn select_page(&mut self,layer:u64,page:&str)->bool {
        if self.layer!=Some(layer) || !self.pages.iter().any(|p|p==page) || self.page.as_deref()==Some(page){return false;}
        self.page=Some(page.into());self.selected=None;self.invalidate();true
    }
    pub fn selected(&mut self,key:&str,points:&[[f32;2]])->Option<usize> {
        match self.selected.as_ref() {
            Some((old,index)) if old==key && *index<points.len()=>Some(*index),
            Some((old,_)) if old==key=>{self.selected=None;None},
            _=>None,
        }
    }
    pub fn select(&mut self,layer:u64,epoch:u64,key:&str,index:Option<usize>,points:&[[f32;2]])->bool {
        if !self.accepts(layer,epoch) || index.is_some_and(|i|i>=points.len()){return false;}
        self.selected=index.map(|index|(key.into(),index));true
    }
    pub fn clear_selection(&mut self,key:&str) {if self.selected.as_ref().is_some_and(|(selected,_)|selected==key){self.selected=None;}}
    pub fn contact_index(&self,key:&str)->Option<usize> {self.contact.as_ref().filter(|capture|capture.key==key).map(|capture|capture.index)}
    pub fn begin_contact(&mut self,key:&str,index:usize,position:[f32;2],extent:[f32;2],graph:[f32;2]) {
        self.contact=Some(CurveCapture{key:key.into(),index,extent,press:position,graph});
        self.selected=Some((key.into(),index));
    }
    pub fn contact_point(&self,key:&str,position:[f32;2])->Option<[f32;2]> {
        let capture=self.contact.as_ref().filter(|capture|capture.key==key)?;
        Some([capture.graph[0]+(position[0]-capture.press[0])/capture.extent[0],capture.graph[1]-(position[1]-capture.press[1])/capture.extent[1]])
    }
    pub fn end_contact(&mut self) {self.contact=None;}
    pub fn key(&self)->Option<&str>{self.pressed_key.as_deref()}
    pub fn press_key(&mut self,key:&str){self.pressed_key=Some(key.into());}
    pub fn release_key(&mut self,key:&str)->bool {
        if self.pressed_key.as_deref()!=Some(key){return false;}self.pressed_key=None;true
    }
}
#[cfg(test)] mod state_tests {
    use super::*;
    #[test] fn page_selection_and_stale_contact_are_transient() {
        let mut state=PropertyEditorState::default();state.sync(1,Some(3),1,vec!["rgb".into(),"red".into()],false);
        let old=state.epoch;assert!(state.select(3,old,"rgb",Some(1),&[[0.,0.],[1.,1.]]));
        assert!(state.select_page(3,"red"));assert!(!state.accepts(3,old));assert_eq!(state.page(),Some("red"));
        state.sync(1,Some(3),2,vec!["rgb".into(),"red".into()],false);assert!(!state.accepts(3,old));
        state.sync(1,Some(4),2,vec!["rgb".into(),"red".into()],false);assert_eq!(state.page(),Some("rgb"));
    }
    #[test] fn replacing_document_invalidates_same_layer_revision_and_pages() {
        let mut state=PropertyEditorState::default();state.sync(1,Some(3),1,vec!["rgb".into()],false);let epoch=state.epoch;
        state.sync(2,Some(3),1,vec!["rgb".into()],false);assert!(!state.accepts(3,epoch));
    }
    #[test] fn native_key_release_matches_current_repeat_gesture() {
        let mut state=PropertyEditorState::default();state.press_key("ArrowUp");assert!(!state.release_key("ArrowDown"));assert!(state.release_key("ArrowUp"));assert_eq!(state.key(),None);
    }
    #[test] fn revision_replacement_rejects_stale_point_and_closes_key_capture() {
        let mut state=PropertyEditorState::default();
        state.sync(1,Some(3),1,vec!["rgb".into()],false);
        let epoch=state.epoch;
        state.select(3,epoch,"curve",Some(1),&[[0.,0.],[0.5,0.5],[1.,1.]]);
        state.begin_contact("curve",1,[50.,50.],[100.,100.],[0.5,0.5]);
        state.press_key("ArrowUp");
        state.sync(1,Some(3),2,vec!["rgb".into()],false);
        assert!(!state.accepts(3,epoch));
        assert_eq!(state.contact_index("curve"),None);
        assert_eq!(state.key(),None);
        assert!(!state.select(3,epoch,"curve",Some(0),&[[0.,0.],[1.,1.]]));
    }
    #[test] fn own_live_preview_keeps_epoch_until_external_edit() {
        let mut state=PropertyEditorState::default();
        state.sync(1,Some(3),1,vec!["rgb".into()],false);
        let epoch=state.epoch;
        state.press_key("ArrowRight");
        state.sync(1,Some(3),2,vec!["rgb".into()],true);
        assert!(state.accepts(3,epoch));
        assert_eq!(state.key(),Some("ArrowRight"));
        assert!(state.release_key("ArrowRight"));
        state.sync(1,Some(3),3,vec!["rgb".into()],false);
        assert!(!state.accepts(3,epoch));
    }

}
