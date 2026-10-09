use crate::{Document,Selection,authored::*};
use serde::{Deserialize,Serialize};
use std::{sync::Arc,borrow::Cow};

#[derive(Serialize, Deserialize)]
pub enum SnapshotSource {
    Visible,
    Source(crate::SourceTarget),
    Objects(crate::OccurrenceHandle),
    EffectInput(crate::OccurrenceHandle),
    EffectChannels(crate::OccurrenceHandle),
}
impl SnapshotSource {
    pub fn artwork_source(self) -> crate::ArtworkSource {
        match self {
            Self::Visible => crate::ArtworkSource::Visible,
            Self::Source(target) => crate::ArtworkSource::Source(target),
            Self::Objects(target) => crate::ArtworkSource::Objects(target),
            Self::EffectInput(target) => crate::ArtworkSource::EffectInput(target),
            Self::EffectChannels(target) => crate::ArtworkSource::EffectChannels(target),
        }
    }
}

pub const ARTWORK_SAMPLE_WIDTHS:[u32;5]=[1,5,15,51,101];
#[derive(Clone,Debug,PartialEq)]
pub enum ArtworkSource {Visible,Source(SourceTarget),Objects(OccurrenceHandle),Reference,EffectInput(OccurrenceHandle),EffectChannels(OccurrenceHandle),EffectBaseline(EffectBaseline)}
#[derive(Clone,Debug)]
pub struct ArtworkQuery {
    pub snapshot:Arc<SceneSnapshot>,
    pub source:ArtworkSource,
    pub selection:Option<Selection>,
    input:Option<EffectInputKey>,
}
#[derive(Clone,Debug)]
pub struct EffectInputKey {snapshot:Arc<SceneSnapshot>,target:OccurrenceHandle,input:Vec<OccurrenceHandle>,ancestors:Vec<OccurrenceHandle>}
impl EffectInputKey {
    pub fn new(snapshot:Arc<SceneSnapshot>,target:OccurrenceHandle)->Option<Self>{
        let scene=snapshot.view();scene.effect(target)?;
        let input=Self::ordered_input(scene,target);let ancestors=Self::ancestors(scene,target);
        Some(Self{snapshot,target,input,ancestors})
    }
    fn ordered_input(scene:SceneView<'_>,target:OccurrenceHandle)->Vec<OccurrenceHandle>{let mut input=crate::composite_input_layers(scene,target);input.sort_by_key(|h|scene.position(*h));input}
    pub fn occurrence(&self)->OccurrenceHandle{self.target}
    pub fn scene(&self)->SceneView<'_>{self.snapshot.view()}
    pub fn contributors(&self)->impl Iterator<Item=OccurrenceHandle>+'_ {self.input.iter().copied()}
    fn ancestors(scene:SceneView<'_>,h:OccurrenceHandle)->Vec<OccurrenceHandle>{let mut result=Vec::new();let mut parent=scene.parent(h);while let Some(h)=parent{result.push(h);parent=scene.parent(h);}result}
    pub fn matches_source(&self,document:&Document)->bool{self.matches(document.scene(),true,false)}
    pub fn matches_identity(&self,document:&Document)->bool{self.matches(document.scene(),false,false)}
    fn matches(&self,scene:SceneView<'_>,values:bool,channels:bool)->bool {
        let old=self.scene();
        if self.snapshot.owner!=scene.owner() || !old.same_composition(scene){return false;}
        let (Some(a),Some(b))=(old.occurrence(self.target),scene.occurrence(self.target)) else{return false;};
        if a.kind()!=b.kind() || old.parent(self.target)!=scene.parent(self.target) || a.attachment!=b.attachment{return false;}
        let (Some(a),Some(b))=(old.effect(self.target),scene.effect(self.target)) else{return false;};
        if a.program!=b.program{return false;}
        if channels && values && !a.program.parameters.iter().zip(a.values.iter().zip(b.values)).all(|(p,(a,b))|p.page.as_deref()==Some("rgb")||a==b){return false;}
        let input=Self::ordered_input(scene,self.target);let ancestors=Self::ancestors(scene,self.target);
        self.input==input && self.ancestors==ancestors && input.iter().all(|h|old.same_occurrence(scene,*h,values))
            && ancestors.iter().all(|h|{
                let (Some(a),Some(b))=(old.occurrence(*h),scene.occurrence(*h)) else{return false;};
                a.kind()==b.kind() && a.visible==b.visible && a.attachment==b.attachment && a.passes_through()==b.passes_through()
                    && a.offset==b.offset && old.local_extent(*h)==scene.local_extent(*h)
            })
    }
}

#[derive(Clone, Debug)]
pub struct ArtworkSampleRequest {
    pub query: ArtworkQuery,
    pub position: [f32; 2],
    pub width: u32,
}
impl std::ops::Deref for ArtworkSampleRequest {
    type Target = ArtworkQuery;
    fn deref(&self) -> &ArtworkQuery { &self.query }
}
impl std::ops::DerefMut for ArtworkSampleRequest {
    fn deref_mut(&mut self) -> &mut ArtworkQuery { &mut self.query }
}

#[derive(Clone, Debug)]
pub struct ArtworkStatisticsRequest {
    pub waveform: bool,
    pub query: ArtworkQuery,
    pub preview: bool,
    pub selection: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ArtworkSample {
    Color([f32; 4]),
    Empty,
    Outside,
}

impl ArtworkSampleRequest {
    pub fn new(document:&Document,source:ArtworkSource,position:[f32;2],width:u32)->Self{Self{query:ArtworkQuery::new(document,source),position,width}}
    pub fn validate(&self)->Result<(),String>{if !ARTWORK_SAMPLE_WIDTHS.contains(&self.width)||!self.position.into_iter().all(f32::is_finite){return Err("Invalid artwork sample".into());}self.query.validate()}
}
impl ArtworkQuery {
    pub fn new(document:&Document,source:ArtworkSource)->Self{Self::from_snapshot(document.snapshot(),source,document.working.selection.clone())}
    pub fn from_snapshot(snapshot:Arc<SceneSnapshot>,source:ArtworkSource,selection:Option<Selection>)->Self {
        let input=match source {ArtworkSource::EffectInput(h)|ArtworkSource::EffectChannels(h)=>EffectInputKey::new(snapshot.clone(),h),_=>None};
        Self{snapshot,source,selection,input}
    }
    fn input_key(&self,target:OccurrenceHandle)->Option<Cow<'_,EffectInputKey>> {
        if let Some(key)=&self.input && key.target==target && Arc::ptr_eq(&key.snapshot,&self.snapshot) {return Some(Cow::Borrowed(key));}
        EffectInputKey::new(self.snapshot.clone(),target).map(Cow::Owned)
    }
    pub fn set_context(&mut self,context:EvaluationContext){Arc::make_mut(&mut self.snapshot).context=context;}
    pub fn validate(&self)->Result<(),String>{
        let scene=self.snapshot.view();let context=&self.snapshot.context;
        if !context.elapsed.is_finite()||context.phases.iter().any(|(_,v)|!v.is_finite())||scene.composition().size.iter().any(|v|*v>crate::MAX_EXTENT){return Err("Invalid artwork sample".into());}
        if scene.targets().any(|t|scene.operations(t).is_some_and(|ops|!ops.is_empty())){return Err("Wait for the current edit before sampling".into());}
        match &self.source {
            ArtworkSource::Objects(h) if scene.object_layer(*h).is_none()=>Err("The object layer to sample was removed".into()),
            ArtworkSource::Source(t) if !matches!(t,SourceTarget::Paint(_))||scene.original(*t).is_none()&&scene.raster(*t).is_none()=>Err("This source has no color content to sample".into()),
            ArtworkSource::EffectChannels(h) if scene.effect(*h).is_none_or(|e|!matches!(e.program.id.as_ref(),"curves"|"levels")||e.program.kind!=crate::EffectKind::Adjustment)=>Err("This adjustment has no channel statistics".into()),
            ArtworkSource::EffectInput(h) if scene.effect(*h).is_none_or(|e|e.program.kind!=crate::EffectKind::Adjustment)=>Err("The adjustment to sample was removed".into()),
            ArtworkSource::EffectBaseline(b) if scene.effect_handle(b.occurrence)!=Some(b.effect)=>Err("The adjustment to compare was removed".into()),_=>Ok(())
        }
    }
    pub fn matches_snapshot(&self,snapshot:&SceneSnapshot)->bool {
        if self.snapshot.owner!=snapshot.owner{return false;}
        let phase=|s:&SceneSnapshot,h:EffectHandle|{
            let effect=s.view().effect_by_handle(h)?;
            effect.program.time.then(||s.context.phases.iter().find(|(id,_)|*id==h).map_or_else(||effect.time_seconds(s.context.elapsed),|(_,v)|*v).to_bits())
        };
        if let ArtworkSource::Objects(h)=self.source {return self.snapshot.view().same_composition(snapshot.view()) && self.snapshot.view().same_objects(snapshot.view(),h) && self.snapshot.view().occurrence_offset64(h)==snapshot.view().occurrence_offset64(h);}
        if let ArtworkSource::EffectInput(h)|ArtworkSource::EffectChannels(h)=self.source {
            let Some(key)=self.input_key(h) else{return false;};
            return key.matches(snapshot.view(),true,matches!(self.source,ArtworkSource::EffectChannels(_)))
                && key.contributors().filter_map(|h|self.snapshot.view().effect_handle(h)).all(|h|phase(&self.snapshot,h)==phase(snapshot,h));
        }
        self.snapshot.view().same_artwork(snapshot.view())
            && (matches!(self.source,ArtworkSource::Source(_))
                || self.snapshot.view().order().iter().filter_map(|h|self.snapshot.view().effect_handle(*h)).all(|h|phase(&self.snapshot,h)==phase(snapshot,h)))
    }
    pub fn matches_artwork(&self,document:&Document)->bool{self.matches(document,false,true)}
    pub fn matches_source(&self,document:&Document)->bool{self.matches(document,true,true)}
    pub fn matches_source_identity(&self,document:&Document)->bool{self.matches(document,true,false)}
    fn matches(&self,document:&Document,source_only:bool,values:bool)->bool {
        if self.snapshot.owner!=document.owner{return false;}
        if let ArtworkSource::Objects(h)=self.source {return self.snapshot.view().same_composition(document.scene()) && self.snapshot.view().same_objects(document.scene(),h) && self.snapshot.view().occurrence_offset64(h)==document.scene().occurrence_offset64(h);}
        if source_only && let ArtworkSource::EffectInput(h)|ArtworkSource::EffectChannels(h)=self.source {
            let channels=matches!(self.source,ArtworkSource::EffectChannels(_));
            return self.input_key(h).is_some_and(|k|k.matches(document.scene(),values,channels));
        }
        let old=self.snapshot.view();let scene=document.scene();
        old.same_composition(scene) && old.order()==scene.order()
            && (!matches!(self.source,ArtworkSource::Reference)||old.references()==scene.references())
            && old.order().iter().all(|h|old.same_occurrence(scene,*h,values||!source_only))
    }
}
impl SceneView<'_> {
    pub fn same_composition(self,other:SceneView<'_>)->bool {
        let a=self.composition();let b=other.composition();
        a.size==b.size && a.color==b.color && a.blend==b.blend && a.result==b.result
    }
    pub fn same_artwork(self,other:SceneView<'_>)->bool {
        self.artwork().id==other.artwork().id && self.same_composition(other) && self.order()==other.order()
            && self.order().iter().all(|h|self.same_occurrence(other,*h,true))
    }
    pub fn same_objects(self,other:SceneView<'_>,h:OccurrenceHandle)->bool {
        match (self.object_layer(h),other.object_layer(h)) {
            (None,None)=>true,
            (Some(a),Some(b))=>{
                if self.artwork().objects.same_root(&other.artwork().objects)
                    && self.occurrence(h).map(|owner|&owner.content)==other.occurrence(h).map(|owner|&owner.content) {return true;}
                a == b
            },
            _=>false,
        }
    }
    pub fn same_occurrence(self,other:SceneView<'_>,h:OccurrenceHandle,values:bool)->bool {
        let (Some(a),Some(b))=(self.occurrence(h),other.occurrence(h)) else{return false;};
        if a.content!=b.content||a.visible!=b.visible||a.opacity!=b.opacity||a.blend!=b.blend||a.attachment!=b.attachment||a.offset!=b.offset||a.mask!=b.mask||self.parent(h)!=other.parent(h){return false;}
        if let Some(target)=self.source_target(h) {
            if self.color_mode(target)!=other.color_mode(target)||self.raster(target)!=other.raster(target)||self.operations(target)!=other.operations(target)||self.target_extent(target)!=other.target_extent(target){return false;}
            if self.paint_base(target)!=other.paint_base(target) {return false;}
            if match (self.original(target),other.original(target)){(Some(a),Some(b))=>!Arc::ptr_eq(a,b),(None,None)=>false,_=>true}{return false;}
        }
        if !self.same_objects(other,h) {return false;}
        if let Some(mask)=&a.mask && self.coverage(mask.source)!=other.coverage(mask.source){return false;}
        match (self.effect(h),other.effect(h)){(Some(a),Some(b))=>a.program==b.program&&a.spatial==b.spatial&&(!values||a.values==b.values),(None,None)=>true,_=>false}
    }
}

pub fn white_balance_neutral(rgb: [f32; 3], space: crate::color::RgbSpace, preserve_luminance: bool) -> Result<[f32; 2], &'static str> {
    if rgb.iter().any(|v| !v.is_finite() || *v <= 0.) { return Err("Choose a point with positive red, green and blue values"); }
    let [r, g, b] = rgb.map(|v| f64::from(v).log2());
    let values = [62.5 * (b-r), (100. / 1.5) * (2.*g-r-b)];
    if values[0].abs() > 1000. || values[1].abs() > 800. { return Err("This color is outside the White Balance range"); }
    let values = values.map(|v| v as f32);
    let [t, q] = values.map(|v| v / 100.);
    let gain = [0.8*t+0.25*q, -0.5*q, -0.8*t+0.25*q].map(f32::exp2);
    let mut output = std::array::from_fn::<_, 3, _>(|i| rgb[i]*gain[i]);
    if preserve_luminance {
        let weights = space.to_xyz()[1].map(|v| v as f32);
        let luma = |c: [f32; 3]| c[0]*weights[0]+c[1]*weights[1]+c[2]*weights[2];
        let corrected = luma(output);
        if corrected != 0. { output = output.map(|v| v*(luma(rgb)/corrected)); }
    }
    let low = output.into_iter().fold(f32::INFINITY, f32::min);
    let high = output.into_iter().fold(f32::NEG_INFINITY, f32::max);
    if !output.into_iter().all(f32::is_finite) || high-low > 2e-6f32.max(2e-4*high.abs().max(low.abs())) {
        return Err("This color cannot be made neutral with White Balance");
    }
    Ok(values)
}
