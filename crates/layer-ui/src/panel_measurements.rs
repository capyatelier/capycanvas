use crate::{Panel,PanelMeasurement,PanelScrollMeasurement};
use serde::{Deserialize,Serialize};

#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
pub struct PanelMeasurementIdentity {
    pub panel:Panel,
    pub tab:String,
    pub body:String,
    pub cacheable:bool,
}

#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
pub struct PanelMeasurementRequest {
    #[serde(flatten)]
    pub identity:PanelMeasurementIdentity,
    pub width:f32,
    pub split_filters:bool,
    pub tab_measured:bool,
    pub body_measured:bool,
}

#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
pub struct PanelMetricContext {
    pub generation:u64,
    pub scale:f32,
    pub text_scale:f32,
    pub style:String,
}
impl PanelMetricContext {
    fn same_metrics(&self,other:&Self)->bool {
        self.generation==other.generation&&self.scale==other.scale&&self.text_scale==other.text_scale&&self.style==other.style
    }
    fn validate(&self)->Result<(),String> {
        if self.generation==0||self.generation>9_007_199_254_740_991
            ||![self.scale,self.text_scale].into_iter().all(|v|v.is_finite()&&(0.1..=16.).contains(&v))
            ||self.style.is_empty()||self.style.len()>256 {return Err("Invalid panel metric context".into());}
        Ok(())
    }
}

#[derive(Clone,Copy,Debug,PartialEq,Serialize,Deserialize)]
pub struct PanelBodyMeasurement {
    pub actual_width:f32,
    pub content_height:f32,
    pub scroll:Option<PanelScrollMeasurement>,
}

#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
pub struct PanelMeasurementReport {
    pub request:PanelMeasurementRequest,
    pub context:PanelMetricContext,
    pub tab_width:Option<f32>,
    pub body:Option<PanelBodyMeasurement>,
}

#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelMeasurementBatch {
    pub context:PanelMetricContext,
    pub reports:Vec<PanelMeasurementReport>,
}

#[derive(Clone,Debug,Default,PartialEq,Serialize,Deserialize)]
pub(crate) struct PanelMeasurementCache {
    pub identities:Vec<PanelMeasurementIdentity>,
    pub context:Option<PanelMetricContext>,
    tabs:Vec<MeasuredTab>,
    bodies:Vec<MeasuredBody>,
}

#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
struct MeasuredTab {panel:Panel,key:String,context:PanelMetricContext,width:f32}
#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
struct MeasuredBody {panel:Panel,key:String,context:PanelMetricContext,width:f32,split_filters:bool,body:Option<PanelBodyMeasurement>,verified:bool}

pub(crate) fn panel_measurement_width(width:f32)->f32 {(width*64.).round()/64.}

impl PanelMeasurementCache {
    pub fn for_publication(mut self)->Self {for body in &mut self.bodies {body.verified=false;}self}
    pub fn with_context(&self,context:PanelMetricContext)->Result<Self,String> {
        context.validate()?;
        if let Some(current)=&self.context {
            if context.generation<current.generation {return Ok(self.clone());}
            if context.generation==current.generation&&!context.same_metrics(current) {
                return Err("Panel metrics changed without advancing their generation".into());
            }
        }
        let mut cache=self.clone();
        if cache.context.as_ref()!=Some(&context) {cache.tabs.clear();cache.bodies.clear();}
        cache.context=Some(context);Ok(cache)
    }
    pub fn accept(&self,batch:PanelMeasurementBatch)->Result<Self,String> {
        self.with_context(batch.context)?.with_reports(batch.reports)
    }
    pub fn with_reports(&self,reports:Vec<PanelMeasurementReport>)->Result<Self,String> {
        if reports.len()>256 {return Err("Too many panel measurements".into());}
        if reports.iter().enumerate().any(|(index,report)|reports[..index].iter().any(|previous|previous.request.identity.panel==report.request.identity.panel)) {
            return Err("Duplicate panel measurement".into());
        }
        let mut cache=self.clone();
        for mut report in reports {
            if report.request.identity.tab.len()>64||report.request.identity.body.len()>64 {return Err("Invalid panel measurement identity".into());}
            report.context.validate()?;
            let finite=|v:f32|v.is_finite()&&(0.0..1_000_000.0).contains(&v);
            if !finite(report.request.width)||report.request.width<=0.
                ||report.tab_width.is_some_and(|v|!finite(v)||v<=0.)
                ||report.body.is_some_and(|b|!finite(b.actual_width)||!finite(b.content_height)
                    ||b.scroll.is_some_and(|s|!finite(s.fixed_height)||!finite(s.unit_height)||s.fixed_height>b.content_height)) {
                return Err("Invalid panel measurement".into());
            }
            if cache.context.as_ref().is_some_and(|c|!report.context.same_metrics(c)){continue;}
            let Some(identity)=cache.identities.iter().find(|i|i.panel==report.request.identity.panel) else {continue;};
            if report.request.identity.tab!=identity.tab {report.tab_width=None;}
            if report.request.identity.body!=identity.body||report.body.is_some_and(|body|body.actual_width==0.
                ||(body.actual_width-report.request.width).abs()*report.context.scale>1.) {report.body=None;}
            if report.tab_width.is_none()&&report.body.is_none(){continue;}
            cache.context=Some(report.context.clone());
            report.request.width=panel_measurement_width(report.request.width);
            if let Some(body)=&mut report.body {body.actual_width=panel_measurement_width(body.actual_width);}
            let panel=identity.panel;
            if let Some(width)=report.tab_width {
                cache.tabs.retain(|entry|entry.panel!=panel||entry.key!=identity.tab||!entry.context.same_metrics(&report.context));
                cache.tabs.push(MeasuredTab {panel,key:identity.tab.clone(),context:report.context.clone(),width});
                if cache.tabs.len()>192 {cache.tabs.remove(0);}
            }
            if let Some(body)=report.body {
                let previous=cache.bodies.iter().position(|entry|entry.panel==panel&&entry.key==identity.body
                    &&entry.width==report.request.width&&entry.split_filters==report.request.split_filters
                    &&entry.context.same_metrics(&report.context));
                if let Some(index)=previous {cache.bodies.remove(index);}
                cache.bodies.push(MeasuredBody {panel,key:identity.body.clone(),context:report.context,
                    width:report.request.width,split_filters:report.request.split_filters,body:Some(body),verified:true});
                if cache.bodies.iter().filter(|entry|entry.panel==panel).count()>8 {
                    let first=cache.bodies.iter().position(|entry|entry.panel==panel).unwrap();cache.bodies.remove(first);
                }
                if cache.bodies.len()>192 {cache.bodies.remove(0);}
            }
        }
        Ok(cache)
    }
    pub fn request(&self,identity:&PanelMeasurementIdentity,width:f32,split_filters:bool)->PanelMeasurementRequest {
        let mut request=PanelMeasurementRequest {identity:identity.clone(),width:panel_measurement_width(width),split_filters,
            tab_measured:false,body_measured:false};
        request.tab_measured=self.tab(&request).is_some();request.body_measured=self.body_entry(&request,Some(request.width)).is_some_and(|entry|identity.cacheable||entry.verified);request
    }
    fn tab(&self,request:&PanelMeasurementRequest)->Option<f32> {
        let context=self.context.as_ref()?;
        let identity=self.identities.iter().find(|identity|identity.panel==request.identity.panel)?;
        if identity.tab!=request.identity.tab {return None;}
        Some(self.tabs.iter().rev().find(|entry|entry.panel==request.identity.panel
            &&entry.key==request.identity.tab&&entry.context.same_metrics(context))?.width)
    }
    fn body_entry(&self,request:&PanelMeasurementRequest,width:Option<f32>)->Option<&MeasuredBody> {
        let context=self.context.as_ref()?;
        let identity=self.identities.iter().find(|identity|identity.panel==request.identity.panel)?;
        if identity.body!=request.identity.body {return None;}
        self.bodies.iter().rev().find(|entry|entry.panel==request.identity.panel
            &&entry.key==request.identity.body&&entry.body.is_some()&&width.is_none_or(|width|entry.width==width)
            &&entry.split_filters==request.split_filters&&entry.context.same_metrics(context))
    }
    fn body(&self,request:&PanelMeasurementRequest)->Option<PanelBodyMeasurement> {self.body_entry(request,Some(request.width)).or_else(||self.body_entry(request,None))?.body}
    pub fn project(&self,requests:&[PanelMeasurementRequest])->Vec<PanelMeasurement> {
        requests.iter().filter_map(|request| {
            let body=self.body(request);let tab=self.tab(request);
            if body.is_none()&&tab.is_none(){return None;}
            Some(PanelMeasurement {panel:request.identity.panel,tab_width:tab.unwrap_or(0.),
                content_height:body.map_or(0.,|b|b.content_height),scroll:body.and_then(|b|b.scroll)})
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cache()->PanelMeasurementCache {
        PanelMeasurementCache {identities:vec![PanelMeasurementIdentity {panel:Panel::Histogram,tab:"tab".into(),body:"scope".into(),cacheable:true}],..Default::default()}
    }
    fn request(cache:&PanelMeasurementCache,width:f32)->PanelMeasurementRequest {cache.request(&cache.identities[0],width,false)}
    fn report(cache:&PanelMeasurementCache,width:f32,height:f32)->PanelMeasurementReport {
        PanelMeasurementReport {request:request(cache,width),context:PanelMetricContext {generation:1,scale:1.25,text_scale:1.,style:"native".into()},
            tab_width:Some(110.),body:Some(PanelBodyMeasurement {actual_width:width,content_height:height,scroll:None})}
    }
    #[test]
    fn width_changes_select_only_matching_body_facts_and_keep_tabs() {
        let cache=cache();let cache=cache.with_reports(vec![report(&cache,200.,600.)]).unwrap();
        let different=request(&cache,300.);
        assert!(different.tab_measured);assert!(!different.body_measured);
        assert_eq!(cache.project(&[different])[0].content_height,600.);
        let cache=cache.with_reports(vec![report(&cache,300.,450.)]).unwrap();
        assert_eq!(cache.project(&[request(&cache,300.)])[0].content_height,450.);
        assert_eq!(cache.project(&[request(&cache,200.)])[0].content_height,600.);
        let late=report(&cache,200.,610.);let cache=cache.with_reports(vec![late]).unwrap();
        assert_eq!(cache.project(&[request(&cache,300.)])[0].content_height,450.);
    }
    #[test]
    fn continuous_width_changes_keep_the_last_body_until_remeasured() {
        let mut cache=cache();cache.identities[0].panel=Panel::Color;
        cache=cache.with_reports(vec![report(&cache,280.,244.)]).unwrap();
        let mut heights=vec![244.];
        for width in (200..280).rev().map(|width|width as f32) {
            let pending=request(&cache,width);
            assert!(!pending.body_measured);
            let projected=cache.project(&[pending]);
            assert_eq!(projected[0].content_height,*heights.last().unwrap());
            heights.push(projected[0].content_height);
            cache=cache.with_reports(vec![report(&cache,width,width-36.)]).unwrap();
            heights.push(cache.project(&[request(&cache,width)])[0].content_height);
        }
        assert!(heights.iter().all(|height|*height>0.));
        assert!(heights.windows(2).all(|pair|pair[1]<=pair[0]));
    }
    #[test]
    fn content_and_metric_changes_invalidate_without_retagging_old_facts() {
        let cache=cache();let old=report(&cache,200.,600.);let mut cache=cache.with_reports(vec![old.clone()]).unwrap();
        cache.identities[0].body="new schema".into();
        let cache=cache.with_reports(vec![old]).unwrap();
        assert!(!request(&cache,200.).body_measured);assert!(request(&cache,200.).tab_measured);
        let mut fresh=report(&cache,200.,500.);fresh.context.generation=2;fresh.context.text_scale=1.5;
        let cache=cache.with_context(fresh.context.clone()).unwrap().with_reports(vec![fresh]).unwrap();
        let old=report(&cache,200.,600.);let cache=cache.with_reports(vec![old]).unwrap();
        assert_eq!(cache.project(&[request(&cache,200.)])[0].content_height,500.);
        assert_eq!(cache.context.unwrap().text_scale,1.5);
    }
    #[test]
    fn width_tolerance_keeps_independent_tabs_and_previous_valid_bodies() {
        let cache=cache();let mut measured=report(&cache,200.,600.);measured.context.scale=2.;
        let cache=cache.with_reports(vec![measured.clone()]).unwrap();
        for (actual_width,expected) in [(0.,600.),(196.,600.),(200.5,620.),(200.515625,600.)] {
            let mut next=measured.clone();next.tab_width=Some(120.);
            next.body=Some(PanelBodyMeasurement {actual_width,content_height:620.,scroll:None});
            let updated=cache.with_reports(vec![next.clone()]).unwrap().with_reports(vec![next]).unwrap();
            let projected=updated.project(&[request(&updated,200.)]);
            assert_eq!(projected[0].content_height,expected);assert_eq!(projected[0].tab_width,120.);
        }
    }
    #[test]
    fn invalid_reports_are_transactional_and_cache_is_bounded() {
        let cache=cache();let mut bad=report(&cache,200.,600.);bad.body.as_mut().unwrap().actual_width=f32::NAN;
        assert!(cache.with_reports(vec![report(&cache,200.,600.),bad]).is_err());
        assert!(cache.context.is_none());assert!(cache.bodies.is_empty());
        let mut cache=cache;
        for width in 200..220 {cache=cache.with_reports(vec![report(&cache,width as f32,600.)]).unwrap();}
        assert_eq!(cache.bodies.len(),8);assert!(!request(&cache,200.).body_measured);assert!(request(&cache,219.).body_measured);
    }
    #[test]
    fn current_host_metrics_select_geometry_before_any_new_report() {
        let cache=cache();let cache=cache.with_reports(vec![report(&cache,200.,600.)]).unwrap();
        let current=PanelMetricContext {generation:2,scale:1.25,text_scale:1.5,style:"native".into()};
        let cache=cache.with_context(current).unwrap();
        assert!(!request(&cache,200.).body_measured);assert!(!request(&cache,200.).tab_measured);
        let old=report(&cache,200.,600.);let cache=cache.with_reports(vec![old]).unwrap();
        assert_eq!(cache.context.as_ref().unwrap().text_scale,1.5);
        assert!(!request(&cache,200.).body_measured);
    }
    #[test]
    fn workspace_widths_are_final_on_first_resolve_and_late_reports_cannot_reflow_them() {
        use crate::{DockTarget,LayoutResolveInput,Platform,WorkspacePreset};
        let viewport=[1800.,1400.];
        let mut a=WorkspacePreset::Photographer.layout(Platform::Windows);
        a.open_default_columns(Platform::Windows);
        a.move_panel(viewport,Panel::Histogram,DockTarget::Float {position:[600.,200.]}).unwrap();
        let floating=a.floating.iter_mut().find(|group|group.root.group_for(Panel::Histogram).is_some()).unwrap();
        floating.width=280.;floating.height=None;
        let mut b=a.clone();b.floating.iter_mut().find(|group|group.root.group_for(Panel::Histogram).is_some()).unwrap().width=420.;
        let input=|layout:&crate::DockLayout,cache:PanelMeasurementCache|LayoutResolveInput::new(layout,None).with_cache(cache).resolve(viewport);
        let cache=cache();
        let request_a=input(&a,cache.clone()).measurement_requests.into_iter().find(|request|request.identity.panel==Panel::Histogram).unwrap();
        let mut measured_a=report(&cache,request_a.width,500.);measured_a.request=request_a;
        let cache=cache.accept(PanelMeasurementBatch {context:measured_a.context.clone(),reports:vec![measured_a.clone()]}).unwrap();
        a.measurements=cache.project(&input(&a,cache.clone()).measurement_requests);
        b.adopt_runtime(&a);assert!(b.measurements.is_empty());
        let request_b=input(&b,cache.clone()).measurement_requests.into_iter().find(|request|request.identity.panel==Panel::Histogram).unwrap();
        assert_ne!(measured_a.request.width,request_b.width);assert!(!request_b.body_measured);
        let mut measured_b=report(&cache,request_b.width,260.);measured_b.request=request_b;
        let cache=cache.accept(PanelMeasurementBatch {context:measured_b.context.clone(),reports:vec![measured_b.clone()]}).unwrap();
        let first_a=input(&a,cache.clone());let first_b=input(&b,cache.clone());
        let bounds=|layout:&crate::ResolvedLayout|layout.groups.iter().find(|group|group.active==Panel::Histogram).unwrap().bounds;
        assert!((bounds(&first_a).height-536.).abs()<0.01);assert!((bounds(&first_b).height-296.).abs()<0.01);
        let cache=cache.accept(PanelMeasurementBatch {context:measured_a.context.clone(),reports:vec![measured_a]}).unwrap();
        assert_eq!(bounds(&input(&b,cache.clone())),bounds(&first_b));
        let cache=cache.accept(PanelMeasurementBatch {context:measured_b.context.clone(),reports:vec![measured_b]}).unwrap();
        assert_eq!(bounds(&input(&a,cache.clone())),bounds(&first_a));
        assert_eq!(bounds(&input(&b,cache.clone())),bounds(&first_b));
        let prepared=LayoutResolveInput::new(&a,None).with_cache(cache);
        let wire=serde_json::to_value(&prepared).unwrap();
        let round_trip:LayoutResolveInput=serde_json::from_value(wire).unwrap();
        assert_eq!(bounds(&round_trip.resolve(viewport)),bounds(&first_a));
        let durable=serde_json::to_value(&a).unwrap();assert!(durable.get("measurements").is_none());assert!(durable.get("cache").is_none());
    }
    #[test]
    fn native_only_content_requires_verification_in_each_publication_without_changing_its_identity() {
        let mut cache=cache();cache.identities[0].cacheable=false;
        let measured=report(&cache,200.,400.);
        let cache=cache.accept(PanelMeasurementBatch {context:measured.context.clone(),reports:vec![measured.clone()]}).unwrap();
        assert!(request(&cache,200.).body_measured);
        let published=cache.for_publication();assert!(!request(&published,200.).body_measured);
        assert_eq!(published.project(&[request(&published,200.)])[0].content_height,400.);
        let verified=published.accept(PanelMeasurementBatch {context:measured.context.clone(),reports:vec![measured]}).unwrap();
        assert!(request(&verified,200.).body_measured);
    }
    #[test]
    fn font_generation_invalidates_even_when_metric_values_are_identical() {
        let cache=cache();let measured=report(&cache,200.,400.);
        let cache=cache.accept(PanelMeasurementBatch {context:measured.context.clone(),reports:vec![measured.clone()]}).unwrap();
        let mut context=measured.context.clone();context.generation+=1;
        let cache=cache.accept(PanelMeasurementBatch {context,reports:vec![measured]}).unwrap();
        assert!(!request(&cache,200.).body_measured);assert!(!request(&cache,200.).tab_measured);
        assert!(cache.project(&[request(&cache,200.)]).is_empty());
    }

    #[test]
    fn fresh_native_measurement_verifies_only_its_own_width_in_the_publication() {
        let mut cache=cache();cache.identities[0].cacheable=false;
        for width in [200.,300.] {
            let measured=report(&cache,width,400.);
            cache=cache.accept(PanelMeasurementBatch {context:measured.context.clone(),reports:vec![measured]}).unwrap();
        }
        let cache=cache.for_publication();let measured=report(&cache,200.,450.);
        let cache=cache.accept(PanelMeasurementBatch {context:measured.context.clone(),reports:vec![measured]}).unwrap();
        assert!(request(&cache,200.).body_measured);assert!(!request(&cache,300.).body_measured);
    }

}
