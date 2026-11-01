use super::*;
use layer_render::{RegionRequest,RegionSource};

pub(super) struct Cache {
    pub source: RegionSource,
    pub paper: Option<layer_core::paper::PaperBinding>,
    tolerance: u32,
    enclosed: bool,
    pub seeds: wgpu::Buffer,
    pub ready: bool,
    pub lastwrite:Option<crate::submission::Submission>,
    pub classified:bool,
    frame:Option<std::sync::Arc<artwork::Frame>>,
}
impl Cache {
    pub fn new(r:&WgpuRasterizer,request:&RegionRequest,source:RegionSource,enclosed:bool)->Self {
        let seeds=r.device.create_buffer(&wgpu::BufferDescriptor {label:Some("cached reference seed keys"),size:source.sources().len() as u64*16,
            usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
        Self {source,paper:request.paper.clone(),tolerance:request.tolerance.to_bits(),enclosed,seeds,ready:false,lastwrite:None,classified:false,frame:r.artwork_frame.clone()}
    }
    pub fn submitted(&self)->bool {self.lastwrite.as_ref().is_none_or(|owner|owner.submitted())}
    pub fn pending(&self)->bool {self.lastwrite.as_ref().is_some_and(|owner|!owner.submitted() && !owner.discarded())}
    pub fn reads(&self,target:SourceTarget)->bool {
        self.source.sources().iter().any(|source|match source {
            RegionSource::Scene {snapshot,scope}=>{
                let scene=snapshot.view();
                match scope {
                    SceneScope::Raw(raw)=>*raw==target,
                    SceneScope::RawObjects(_)=>false,
                    SceneScope::Members(members)=>dependencies(scene,members).iter().any(|&id|scene.source_target(id)==Some(target)
                        || scene.occurrence(id).and_then(|node|node.mask.as_ref()).is_some_and(|mask|SourceTarget::Coverage(mask.source)==target)),
                    _=>scene.source_owner(target).is_some(),
                }
            },
            _=>true,
        })
    }
    pub fn matches(&self,r:&WgpuRasterizer,request:&RegionRequest,source:&RegionSource,enclosed:bool)->bool {
        self.lastwrite.as_ref().is_none_or(|owner|!owner.discarded())
            && self.frame.as_ref().zip(r.artwork_frame.as_ref()).is_none_or(|(old,new)|old.same_evaluation(new.packet(r.document_extent))
            && old.scene.view().order().iter().filter(|&&id|old.scene.view().effect(id).is_some()).all(|&id|
                new.scene.view().effect(id).is_some() && effects::effective_phase(old.scene.view(),id,old.time).to_bits()==effects::effective_phase(new.scene.view(),id,new.time).to_bits()))
            && self.tolerance==request.tolerance.to_bits() && self.paper==request.paper && self.enclosed==enclosed
            && self.source.sources().len()==source.sources().len()
            && self.source.sources().iter().zip(source.sources()).all(|(old,new)|same_source(r,old,new))
    }
}
fn dependencies(scene:layer_core::authored::SceneView<'_>,members:&[OccurrenceHandle])->Vec<OccurrenceHandle> {
    let mut required=members.to_vec();let mut index=0;
    while index<required.len() {
        let id=required[index];index+=1;
        for next in [scene.parent(id),scene.clipping_base(id)].into_iter().flatten() {
            if !required.contains(&next) {required.push(next);}
        }
    }
    required
}
fn same_source(r:&WgpuRasterizer,old:&RegionSource,new:&RegionSource)->bool {
    match (old,new) {
        (RegionSource::Scene {snapshot:a,scope:x},RegionSource::Scene {snapshot:b,scope:y}) if x==y=>{
            let a=a.view();let b=b.view();
            if a.owner()!=b.owner() || !a.same_composition(b) {return false;}
            match x {
                SceneScope::Members(members)=>dependencies(a,members).iter().all(|&id|a.same_occurrence(b,id,true)),
                SceneScope::Raw(target)=>a.target_offset(*target)==b.target_offset(*target) && a.source_owner(*target).is_some_and(|id|a.same_occurrence(b,id,true)),
                SceneScope::RawObjects(id)=>a.same_occurrence(b,*id,true),
                _=>a.same_artwork(b),
            }
        },
        _=>old==new && !matches!(old,RegionSource::Composite|RegionSource::Source(_)|RegionSource::Objects(_))
            && r.artwork_frame.is_some(),
    }
}
pub(super) fn source(r:&WgpuRasterizer,request:&RegionRequest)->RegionSource {
    let sources=request.source.sources();let count=request.reference_count.unwrap_or(sources.len());
    let mut references:Vec<_>=sources[..count].iter().cloned().map(|source|match source {
        RegionSource::Composite=>r.artwork_frame.as_ref().map_or(RegionSource::Composite,|frame|RegionSource::Scene {snapshot:frame.scene.clone(),scope:SceneScope::All}),
        RegionSource::Source(target)=>r.artwork_frame.as_ref().map_or(RegionSource::Source(target),|frame|RegionSource::Scene {snapshot:frame.scene.clone(),scope:SceneScope::Raw(target)}),
        RegionSource::Objects(id)=>r.artwork_frame.as_ref().map_or(RegionSource::Objects(id),|frame|RegionSource::Scene {snapshot:frame.scene.clone(),scope:SceneScope::RawObjects(id)}),
        source=>source,
    }).collect();
    if references.len()==1 {references.pop().unwrap()}else{RegionSource::Intersection(references)}
}
