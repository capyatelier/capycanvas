use super::*;
use layer_core::raster::{ContentStamp, ContentVersion, RasterPlane};
use std::{hash::{Hash, Hasher}, sync::Mutex};
use crate::{IdMap, IdSet};

#[derive(Clone, Default)]
pub(crate) struct Declared {
    pub name_wake:Option<std::task::Waker>,
    pub rasters: IdMap<SourceTarget, raster::contents::Versions>,
    pub captures: IdMap<SourceTarget, Capture>,
    pub preview_inputs: IdMap<SourceTarget, Input>,
    pub preview: Option<Arc<PreviewDeclaration>>,
    pub fill: Option<layer_render::FillPreview>,
    pub quality: PreviewQuality,
    pub vector_preview: Option<layer_render::VectorPreview>,
    pub vector_motion: Option<layer_render::VectorMotion>,
    pub vector_motion_paths: Option<Arc<[layer_core::authored::vector::PathItem]>>,
}
#[derive(Clone)]
pub(crate) struct Input { pub source:SceneSnapshot, pub version:ContentVersion }
impl Input {
    pub fn capture(r:&WgpuRasterizer,packet:FramePacket<'_>)->Result<Self,GpuRasterError> {
        let reads=Reads::of(packet)?;let mut rasters=IdMap::default();
        for target in &reads.targets {
            #[cfg(test)] record_read();
            match r.raster_versions(packet.scene,*target) {Ok(versions)=>{rasters.insert(*target,versions);},Err(GpuRasterError::NotReady)=>{},Err(error)=>return Err(error)}
        }
        Self::from_reads(packet,rasters,&reads)
    }
    pub fn from_declared(packet:FramePacket<'_>,rasters:IdMap<SourceTarget,raster::contents::Versions>)->Result<Self,GpuRasterError> {
        Self::from_reads(packet,rasters,&Reads::of(packet)?)
    }
    fn from_reads(packet:FramePacket<'_>,rasters:IdMap<SourceTarget,raster::contents::Versions>,reads:&Reads)->Result<Self,GpuRasterError> {
        let frame=Prepared::read(packet,Declared {rasters,..Declared::default()},reads)?;
        Ok(Self {version:frame.authored_version(&reads.node)?,source:frame.snapshot.scene.clone()})
    }
}
struct Reads {node:graph::Node,targets:BTreeSet<SourceTarget>,effects:BTreeSet<OccurrenceHandle>}
impl Reads {
    fn of(packet:FramePacket<'_>)->Result<Self,GpuRasterError> {
        let node=graph::compose(packet,None,None,0,packet.scene.composition().color.space,None,None)?;
        let (mut targets,effects)=node.reads();targets.extend(packet.dab_batches.iter().map(|batch|batch.target));
        Ok(Self {node,targets,effects})
    }
}
#[derive(Clone)]
pub(crate) struct Capture {
    source: SceneSnapshot,
    versions: raster::contents::Versions,
    preview: layer_render::TransformPreview,
    definition: ContentVersion,
    authored: ContentVersion,
    cut: ContentVersion,
    taps:u32,
    mesh: Option<Arc<paint_transform::mesh::MeshGeometry>>,
}
impl Capture {
    pub fn new(source:SceneSnapshot,versions:raster::contents::Versions,preview:layer_render::TransformPreview,
        mesh:Option<Arc<paint_transform::mesh::MeshGeometry>>)->Self {
        let context=recipe(&(source.view().composition().color,source.view().color_mode(preview.target)));
        let definition=ContentVersion::derive(context,&[transform_version(&preview)]);
        let authored=ContentVersion::derive(context,&[transform_version(&layer_render::TransformPreview {moving:false,..preview.clone()})]);
        let cut=ContentVersion::derive(context,&[cut_version(&preview)]);
        let taps=crate::pixel_transform::exact_taps(&preview.drawn());
        let mesh=mesh.or_else(||preview.drawn().placement.mesh.as_ref().map(|mesh|
            Arc::new(paint_transform::mesh::MeshGeometry::new(mesh,preview.drawn().placement.outer,None))));
        Self {source,versions,preview,definition,authored,cut,taps,mesh}
    }
    pub fn with_preview(&self,preview:layer_render::TransformPreview,mesh:Option<Arc<paint_transform::mesh::MeshGeometry>>)->Self {
        Self::new(self.source.clone(),self.versions.clone(),preview,mesh).with_taps(self.taps)
    }
    pub fn with_taps(mut self,taps:u32)->Self {self.taps=taps;self}
    pub fn original(&self,plane:RasterPlane,coordinate:[u32;2])->ContentVersion {self.versions.tile(plane,coordinate)}
    pub fn cut(&self,plane:RasterPlane,coordinate:[u32;2])->ContentVersion {
        ContentVersion::derive(recipe(&raster::contents::index(plane)),&[self.cut.clone(),self.original(plane,coordinate)])
    }
    pub fn matches_context(&self,scene:SceneView<'_>)->bool {
        self.source.view().composition().color==scene.composition().color
            && self.source.view().target_extent(self.preview.target)==scene.target_extent(self.preview.target)
            && self.source.view().color_mode(self.preview.target)==scene.color_mode(self.preview.target)
    }
    fn authored_content(&self)->ContentVersion {
        let plane=source_plane(self.preview.target);let mut inputs=vec![self.authored.clone(),self.versions.root(plane)];
        if plane==RasterPlane::Color {inputs.push(self.versions.root(RasterPlane::WatercolorWetness));}
        ContentVersion::derive(0x63617074757265,&inputs)
    }
    pub fn source_versions(&self)->&raster::contents::Versions {&self.versions}
    pub fn desired_preview(&self)->&layer_render::TransformPreview {&self.preview}
    pub fn matches_source(&self,plane:RasterPlane,current:&raster::contents::Versions)->Option<bool> {self.versions.root(plane).matches(&current.root(plane))}
    pub fn version(&self,plane:RasterPlane,region:DocRect,taps:u32)->Result<ContentVersion,GpuRasterError> {
        Ok(ContentVersion::derive(recipe(&(taps,raster::contents::index(plane))),&[self.region_version(plane,region.translated(self.source.view().target_offset(self.preview.target)),false)?]))
    }
    fn content(&self,region:DocRect,global:bool)->Result<ContentVersion,GpuRasterError> {
        let plane=source_plane(self.preview.target);
        let mut inputs=vec![self.region_version(plane,region,global)?];
        if plane==RasterPlane::Color {inputs.push(self.region_version(RasterPlane::WatercolorWetness,region,global)?);}
        Ok(ContentVersion::derive(recipe(&self.taps),&inputs))
    }
    fn region_version(&self,plane:RasterPlane,region:DocRect,global:bool)->Result<ContentVersion,GpuRasterError> {
        let target=self.preview.target;let scene=self.source.view();let extent=scene.target_extent(target);
        let mut inputs=vec![self.definition.clone()];
        if global {
            inputs.push(self.versions.root(plane));
        } else {
            let region=region.translated(scene.target_offset(target).map(|value|-value)).to_rect();
            let needed=paint_transform::snapshot::source_region(&self.preview.drawn(),region,extent,self.mesh.clone())?;
            inputs.push(self.versions.region(plane,needed));
            if self.preview.selection.is_some() || self.preview.transform.keep_source {
                inputs.push(self.versions.region(plane,pixel_rect(region,extent)));
            }
        }
        Ok(ContentVersion::derive(0x63617074757265,&inputs))
    }
}
#[derive(Clone)]
struct Definition { version:ContentVersion, global:bool }
#[derive(Clone)]
pub(crate) struct Snapshot {
    pub scene: SceneSnapshot,
    rasters: IdMap<SourceTarget, raster::contents::Versions>,
    captures: IdMap<SourceTarget, Capture>,
    preview: Option<Arc<PreviewDeclaration>>,
    vector_preview: Option<(layer_render::VectorPreview,ContentVersion)>,
    vector_motion: Option<(layer_render::VectorMotion,ContentVersion)>,
    effects: IdMap<OccurrenceHandle,Definition>,
    moving: Option<OccurrenceHandle>,
    watercolor: IdMap<SourceTarget, WatercolorLayerStyle>,
    context: ContentStamp,
}
impl Snapshot {
    fn preview_level(&self,id:SourceTarget)->u32 {self.preview.as_ref().filter(|preview|preview.target==id).map_or(0,|preview|preview.quality.level)}
    fn source_level(&self,id:SourceTarget,frame:SourceFrame,extent:[u32;2],region:Option<DocRect>,input_level:u32,moving:bool)->u32 {
        let level=source_level(input_level,frame,moving,0);
        let halo=if frame.aligned() && !moving {0}else {1<<level};
        let footprint=region.map_or(PixelRect::full(extent),|region|source_footprint(region,frame,extent,level,halo+self.watercolor.get(&id).map_or(0,|style|style.radius())));
        self.preview.as_ref().filter(|preview|preview.target==id && preview.quality.level>level && preview.affects(footprint)).map_or(level,|preview|preview.quality.level)
    }
    fn changed_targets(&self,before:&Self)->std::collections::BTreeSet<SourceTarget> {
        let mut changed=before.rasters.keys().chain(self.rasters.keys()).filter(|id|match (before.rasters.get(id),self.rasters.get(id)) {
            (Some(before),Some(after))=>[source_plane(**id),RasterPlane::WatercolorWetness].into_iter().any(|plane|before.root(plane).identity()!=after.root(plane).identity()),
            _=>true,
        }).copied().collect::<std::collections::BTreeSet<_>>();
        if !Option::zip(self.preview.as_ref(),before.preview.as_ref()).is_some_and(|(after,before)|Arc::ptr_eq(after,before)) {
            changed.extend(self.preview.iter().chain(&before.preview).map(|preview|preview.target()));
        }
        changed
    }
    fn continues(&self,before:&Self)->bool {
        let (scene,previous)=(&self.scene,&before.scene);
        let version=|version:&ContentVersion|version.identity();
        scene.owner!=0 && scene.owner==previous.owner && scene.revision==previous.revision && scene.scope==previous.scope
            && scene.offset==previous.offset && scene.context.phases==previous.context.phases && self.context==before.context && self.moving==before.moving
            && self.watercolor==before.watercolor && self.captures.is_empty() && before.captures.is_empty()
            && self.effects.len()==before.effects.len() && self.effects.iter().all(|(handle,definition)|before.effects.get(handle)
                .is_some_and(|previous|previous.global==definition.global && version(&previous.version)==version(&definition.version)))
            && self.vector_preview.as_ref().map(|(_,preview)|version(preview))==before.vector_preview.as_ref().map(|(_,preview)|version(preview))
            && self.vector_motion.as_ref().map(|(_,motion)|version(motion))==before.vector_motion.as_ref().map(|(_,motion)|version(motion))
    }
}
#[derive(Clone)]
struct Expression(graph::Node);
impl PartialEq for Expression {fn eq(&self,other:&Self)->bool {Arc::ptr_eq(&self.0,&other.0)}}
impl Eq for Expression {}
impl Hash for Expression {fn hash<H:Hasher>(&self,state:&mut H) {Arc::as_ptr(&self.0).hash(state);}}
#[derive(Clone, PartialEq, Eq, Hash)]
struct Request {node:Expression,region:[i64;4],level:u32,input_level:u32,global:bool,source_chunk:bool}
impl Request {
    fn new(node:&graph::Node,region:DocRect,level:u32,input_level:u32,global:bool,source_chunk:bool)->Self {
        Self {node:Expression(node.clone()),region:[region.min[0],region.min[1],region.max[0],region.max[1]],level,input_level,global,source_chunk}
    }
    fn region(&self)->DocRect {DocRect {min:[self.region[0],self.region[1]],max:[self.region[2],self.region[3]]}}
}
struct Entry {version:ContentVersion,dependents:Vec<Request>,used:u64}
#[derive(Default)]
struct Keys {entries:IdMap<Request,Entry>,readers:IdMap<SourceTarget,IdMap<Request,PixelRect>>,epoch:u64,kept:usize}
impl Keys {
    fn get(&mut self,request:&Request,parent:Option<&Request>)->Option<ContentVersion> {
        let entry=self.entries.get_mut(request)?;
        entry.used=self.epoch;
        if let Some(parent)=parent && !entry.dependents.contains(parent) {entry.dependents.push(parent.clone());}
        Some(entry.version.clone())
    }
    fn insert(&mut self,request:Request,version:ContentVersion,parent:Option<&Request>,read:Option<(SourceTarget,PixelRect)>) {
        if let Some((id,local))=read {self.readers.entry(id).or_default().insert(request.clone(),local);}
        self.entries.insert(request,Entry {version,dependents:parent.into_iter().cloned().collect(),used:self.epoch});
    }
    fn kept<'a>(entries:&'a IdMap<Request,Entry>,request:&'a Request,epoch:u64,kept:&mut IdMap<&'a Request,bool>)->bool {
        if let Some(keep)=kept.get(request) {return *keep;}
        let keep=entries.get(request).is_some_and(|entry|entry.used==epoch
            || entry.dependents.iter().any(|parent|Self::kept(entries,parent,epoch,kept)));
        kept.insert(request,keep);keep
    }
    fn invalidate(&mut self,id:SourceTarget,changed:impl Fn(PixelRect)->bool,regions:&mut Vec<(u32,DocRect)>) {
        let Some(readers)=self.readers.get_mut(&id) else {return;};
        let mut stale=readers.iter().filter(|(_,local)|changed(**local)).map(|(request,_)|request.clone()).collect::<Vec<_>>();
        for request in &stale {readers.remove(request);}
        while let Some(request)=stale.pop() {
            if let Some(entry)=self.entries.remove(&request) {
                if !request.source_chunk {regions.push((request.level,request.region()));}
                stale.extend(entry.dependents);
            }
        }
    }
    fn advance(&mut self,regions:&mut Vec<(u32,DocRect)>) {
        if self.entries.len()>2*self.kept+256 {
            let mut kept=IdMap::with_capacity_and_hasher(self.entries.len(),Default::default());
            for request in self.entries.keys() {Self::kept(&self.entries,request,self.epoch,&mut kept);}
            let dropped=kept.into_iter().filter(|(_,keep)|!keep).map(|(request,_)|request.clone()).collect::<Vec<_>>();
            for request in &dropped {if self.entries.remove(request).is_some() && !request.source_chunk {regions.push((request.level,request.region()));}}
            let entries=&self.entries;
            self.readers.retain(|_,readers| {readers.retain(|request,_|entries.contains_key(request));!readers.is_empty()});
            self.kept=self.entries.len();
        }
        self.epoch+=1;
    }
}
pub(crate) struct Prepared {
    snapshot: Arc<Snapshot>,
    memo: Mutex<Keys>,
    authored: Mutex<IdMap<Expression,Authored>>,
    name_wake:Option<std::task::Waker>,
}
#[derive(Clone)]
struct Authored {version:ContentVersion,reads:Arc<[SourceTarget]>}
fn read_together(reads:&mut Arc<[SourceTarget]>,child:&Arc<[SourceTarget]>) {
    if reads.is_empty() || reads==child {*reads=child.clone();}
    else if !child.is_empty() {
        let mut union=reads.iter().chain(child.iter()).copied().collect::<Vec<_>>();union.sort();union.dedup();*reads=union.into();
    }
}
pub(super) struct PackingWake(ContentVersion);
pub(super) enum ExpectedKey { Ready(ContentVersion), Unresolved(PackingWake) }
impl ExpectedKey {
    pub fn version(&self)->&ContentVersion {match self {Self::Ready(version)=>version,Self::Unresolved(wait)=>&wait.0}}
    pub(super) fn into_version(self)->ContentVersion {match self {Self::Ready(version)=>version,Self::Unresolved(wait)=>wait.0}}
}
impl Prepared {
    pub fn capture(r:&WgpuRasterizer,packet:FramePacket<'_>,mut declared:Declared)->Result<Self,GpuRasterError> {
        declared.name_wake=r.raster_name_waker();
        let reads=Reads::of(packet)?;
        for target in &reads.targets {
            #[cfg(test)] record_read();
            if !declared.rasters.contains_key(target) {
                match r.raster_versions(packet.scene,*target) {Ok(versions)=>{declared.rasters.insert(*target,versions);},Err(GpuRasterError::NotReady)=>{},Err(error)=>return Err(error)}
            }
        }
        let mut needed=Vec::new();
        if let Some(desired)=r.transform_preview.as_ref() {
            for desired in [Some(desired.clone()),desired.companion(packet.scene)].into_iter().flatten() {
                if declared.captures.contains_key(&desired.target) {continue;}
                match r.transforms.as_ref().ok_or(GpuRasterError::NotReady).and_then(|transforms|transforms.declared_capture(r,packet.scene,&desired)) {
                    Ok(capture)=>{declared.captures.insert(desired.target,capture);},
                    Err(GpuRasterError::NotReady)=>needed.push(desired.target),
                    Err(error)=>return Err(error),
                }
            }
        }
        if declared.preview.is_none() {
            match PreviewDeclaration::capture(r,packet,declared.quality,declared.preview_inputs.clone()) {
                Ok(preview)=>declared.preview=preview.map(Arc::new),
                Err(GpuRasterError::NotReady)=> {
                    if let Some(target)=packet.dab_batches.iter().find(|batch|batch.kind==DabBatchKind::Preview).map(|batch|batch.target).or_else(||r.fill_preview.as_ref().map(|fill|fill.target)) {needed.push(target);}
                },
                Err(error)=>return Err(error),
            }
        }
        declared.fill=declared.fill.or_else(||r.fill_preview.clone());
        declared.vector_preview=declared.vector_preview.or_else(||r.vector_preview.clone());
        if declared.vector_motion.is_none() {
            declared.vector_motion=r.composed_vector_motion().map(|(motion,_)|motion.clone());
            declared.vector_motion_paths=r.vector_motion_paths_drawn.clone().filter(|_|declared.vector_motion.is_some());
        }
        let mut scene=packet.scene.snapshot();
        scene.context.phases=Arc::new(packet.scene.order().iter().filter_map(|handle|packet.scene.effect_handle(*handle)
            .map(|effect|(effect,crate::effects::Gpu::effect_time(r,packet.scene,*handle,packet.time_seconds)))).collect());
        let mut prepared=Self::read(FramePacket {scene:scene.view(),..packet},declared,&reads)?;
        let snapshot=Arc::get_mut(&mut prepared.snapshot).unwrap();snapshot.moving=r.moving_layer;
        for target in needed {snapshot.rasters.remove(&target);snapshot.captures.remove(&target);}
        for target in reads.targets {
            if r.transforms.as_ref().is_some_and(|transforms|transforms.display_source(target)) && !snapshot.captures.contains_key(&target) {snapshot.rasters.remove(&target);}
            if let Some(style)=r.watercolor_style(target,packet.dab_batches) {snapshot.watercolor.insert(target,style);}
        }
        Ok(prepared)
    }
    #[cfg(test)]
    pub fn from_scene(packet:FramePacket<'_>,declared:Declared)->Result<Self,GpuRasterError> {Self::read(packet,declared,&Reads::of(packet)?)}
    fn read(packet:FramePacket<'_>,mut declared:Declared,reads:&Reads)->Result<Self,GpuRasterError> {
        let mut watercolor=IdMap::default();
        for target in reads.targets.iter().copied() {
            #[cfg(test)] record_read();
            if !declared.rasters.contains_key(&target) {
                match raster::contents::declared_versions(packet.scene,target) {Ok(versions)=>{declared.rasters.insert(target,versions);},Err(GpuRasterError::NotReady)=>{},Err(error)=>return Err(error)}
            }
            if let Some(data)=packet.scene.raster(target).and_then(|root|root.try_data()) {
                if let Some(style)=data.map_err(GpuRasterError::Effect)?.watercolor {watercolor.insert(target,WatercolorLayerStyle {wet_edge:style.wet_edge,burnt_edge:style.burnt_edge,edge_width:style.edge_width});}
            }
        }
        let preview=match declared.preview.take() {
            Some(preview)=>Some(preview),
            None=>match PreviewDeclaration::from_scene(packet,declared.rasters.clone(),declared.preview_inputs.clone(),declared.quality,declared.fill.clone(),None) {
                Ok(preview)=>preview.map(Arc::new),
                Err(GpuRasterError::NotReady)=> {
                    if let Some(target)=packet.dab_batches.iter().find(|batch|batch.kind==DabBatchKind::Preview).map(|batch|batch.target).or_else(||declared.fill.as_ref().map(|fill|fill.target)) {
                        declared.rasters.remove(&target);declared.captures.remove(&target);
                    }
                    None
                },
                Err(error)=>return Err(error),
            },
        };
        let mut context=Recipe::new();context.add(&(packet.document_extent,packet.scene.composition().color));context.tag(&packet.blend_space);
        let effects=reads.effects.iter().filter_map(|handle|packet.scene.effect(*handle).map(|effect|(*handle,effect))).map(|(handle,effect)| {
            let mut version=effect_version(effect)?;
            if effect.animated() {version=ContentVersion::derive(u128::from(crate::effects::effective_phase(packet.scene,handle,packet.time_seconds).to_bits()),&[version]);}
            Ok((handle,Definition {version,global:effect.program.analysis().is_some() || effect.program.passes.iter().any(|pass|matches!(pass.sampling,layer_core::EffectSampling::Document))}))
        }).collect::<Result<_,GpuRasterError>>()?;
        let vector_preview=declared.vector_preview.map(|preview| {let version=path_version(&preview.path);(preview,version)});
        let vector_motion=declared.vector_motion.map(|motion| {
            let inputs:Vec<_>=std::iter::once(ContentVersion::derive(recipe(&motion.items),&[])).chain(declared.vector_motion_paths.iter().flat_map(|paths|paths.iter().map(path_version))).collect();
            (motion.clone(),ContentVersion::derive(recipe(&motion.transform.0.map(f64::to_bits)),&inputs))
        });
        let mut prepared=Self::from_snapshot(Arc::new(Snapshot {scene:packet.scene.snapshot(),rasters:declared.rasters,captures:declared.captures,
            preview,vector_preview,vector_motion,effects,moving:None,watercolor,context:context.0}));
        prepared.name_wake=declared.name_wake;Ok(prepared)
    }
    #[cfg(test)]
    pub fn snapshot(&self)->Arc<Snapshot> {self.snapshot.clone()}
    pub fn from_snapshot(snapshot:Arc<Snapshot>)->Self {Self {snapshot,memo:Mutex::new(Keys::default()),authored:Mutex::new(IdMap::default()),name_wake:None}}
    pub(super) fn carry(&self,previous:&Self)->Option<Vec<(u32,DocRect)>> {
        let mut keys=std::mem::take(&mut *previous.memo.lock().unwrap());
        let (s,before)=(&*self.snapshot,&*previous.snapshot);
        if !s.continues(before) {return None;}
        let mut authored=std::mem::take(&mut *previous.authored.lock().unwrap());
        let changed=s.changed_targets(before);
        while {let held=authored.len();authored.retain(|node,entry|Arc::strong_count(&node.0)>1 && !entry.reads.iter().any(|id|changed.contains(id)));authored.len()<held} {}
        *self.authored.lock().unwrap()=authored;
        let mut regions=Vec::new();
        for (id,versions) in &before.rasters {
            let mut changed=Vec::new();
            if s.rasters.get(id).is_some_and(|after|versions.changes(after,|coordinate|changed.push(page_rect(coordinate)))) {
                let bounds=changed.iter().fold(PixelRect::EMPTY,|bounds,tile|bounds.union(*tile));
                keys.invalidate(*id,|local|!local.intersect(bounds).is_empty() && changed.iter().any(|tile|!local.intersect(*tile).is_empty()),&mut regions);
            }else {keys.invalidate(*id,|_|true,&mut regions);}
        }
        if !Option::zip(s.preview.as_ref(),before.preview.as_ref()).is_some_and(|(after,before)|Arc::ptr_eq(after,before)) {
            for preview in s.preview.iter().chain(&before.preview) {keys.invalidate(preview.target(),|local|preview.affects(local),&mut regions);}
        }
        keys.advance(&mut regions);
        *self.memo.lock().unwrap()=keys;
        let mut levels=BTreeMap::<u32,DocRect>::new();
        for (level,region) in regions {let bounds=levels.entry(level).or_default();*bounds=bounds.union(region);}
        Some(levels.into_iter().collect())
    }
    pub(super) fn contact_continues(&self,last:&Self,target:SourceTarget)->Option<std::collections::BTreeSet<[u32;2]>> {
        let (s,before)=(&*self.snapshot,&*last.snapshot);
        if !s.continues(before) || s.rasters.len()!=before.rasters.len() || s.preview_level(target)!=before.preview_level(target) {return None;}
        let unchanged=|plane,before:&raster::contents::Versions,after:&raster::contents::Versions|before.root(plane).identity()==after.root(plane).identity();
        let mut changed=std::collections::BTreeSet::new();
        for (id,versions) in &before.rasters {
            let after=s.rasters.get(id)?;
            if !unchanged(RasterPlane::Mask,versions,after) {return None;}
            if *id!=target {
                if !unchanged(RasterPlane::Color,versions,after) || !unchanged(RasterPlane::WatercolorWetness,versions,after) {return None;}
            }else if !versions.changes(after,|coordinate|{changed.insert(coordinate);}) {return None;}
        }
        if !Option::zip(s.preview.as_ref(),before.preview.as_ref()).is_some_and(|(after,before)|Arc::ptr_eq(after,before)) {
            for preview in s.preview.iter().chain(&before.preview) {
                if preview.target!=target || preview.fill.is_some() {return None;}
                changed.extend(preview.commands.iter().flat_map(|command|command.outputs.iter().copied()));
            }
        }
        Some(changed)
    }
    pub(super) fn watercolor(&self,target:SourceTarget)->bool {self.snapshot.watercolor.contains_key(&target)}
    pub(crate) fn prepare(r:&WgpuRasterizer,packet:FramePacket<'_>)->Result<Arc<Self>,GpuRasterError> {
        Self::capture(r,packet,Declared {preview:r.preview_declaration.clone(),..Default::default()}).map(Arc::new)
    }
    pub(super) fn holds_scene(&self,scene:SceneView<'_>)->bool {
        let (held,artwork)=(&self.snapshot.scene,scene.artwork());
        scene.owner()!=0 && scene.owner()==held.owner && scene.revision()==held.revision && scene.offset()==held.offset
            && scene.scope().map_or(held.scope==Default::default(),|scope|*scope==held.scope)
            && artwork.compositions.same_root(&held.artwork.compositions) && artwork.stacks.same_root(&held.artwork.stacks)
            && artwork.occurrences.same_root(&held.artwork.occurrences) && artwork.paint.same_root(&held.artwork.paint)
            && artwork.objects.same_root(&held.artwork.objects) && artwork.vectors.same_root(&held.artwork.vectors)
            && artwork.coverage.same_root(&held.artwork.coverage) && artwork.effects.same_root(&held.artwork.effects)
    }
    pub(super) fn reader_regions(&self,target:SourceTarget,level:u32)->Vec<(PixelRect,Vec<DocRect>)> {
        let memo=self.memo.lock().unwrap();
        let Some(readers)=memo.readers.get(&target) else {return Vec::new();};
        readers.iter().map(|(request,local)| {
            let mut regions=Vec::new();let mut seen=IdSet::default();let mut stack=vec![request];
            while let Some(request)=stack.pop() {
                if !seen.insert(request) {continue;}
                let Some(entry)=memo.entries.get(request) else {continue;};
                if !request.source_chunk && request.level==level {regions.push(request.region());}
                stack.extend(&entry.dependents);
            }
            (*local,regions)
        }).collect()
    }
    pub(super) fn composed(&self,target:SourceTarget,versions:raster::contents::Versions)->Self {
        let mut snapshot=(*self.snapshot).clone();snapshot.rasters.insert(target,versions);snapshot.preview=None;
        Self {name_wake:self.name_wake.clone(),..Self::from_snapshot(Arc::new(snapshot))}
    }
    fn versions(&self,target:SourceTarget)->Option<&raster::contents::Versions> {self.snapshot.rasters.get(&target)}
    pub(super) fn target_versions(&self,target:SourceTarget)->Option<raster::contents::Versions> {self.versions(target).cloned()}
    pub(crate) fn tile_version(&self,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2])->Option<ContentVersion> {self.versions(target).map(|versions|versions.tile(plane,coordinate))}
    pub fn capture_for(&self,target:SourceTarget)->Option<&Capture> {self.snapshot.captures.get(&target)}
    pub fn preview_for(&self,target:SourceTarget)->Option<&PreviewDeclaration> {self.snapshot.preview.as_deref().filter(|preview|preview.target()==target)}
    fn expected(&self,node:&graph::Node,region:DocRect,level:u32,input_level:u32)->Result<ContentVersion,GpuRasterError> {self.evaluate(Request::new(node,region,level,input_level,false,false),None)}
    fn expected_source(&self,node:&graph::Node,region:DocRect,level:u32,input_level:u32)->Result<ContentVersion,GpuRasterError> {self.evaluate(Request::new(node,region,level,input_level,false,true),None)}
    pub(super) fn source_level(&self,id:SourceTarget,frame:SourceFrame,extent:[u32;2],region:DocRect,input_level:u32,moving:bool)->u32 {
        self.snapshot.source_level(id,frame,extent,Some(region),input_level,moving)
    }
    pub(super) fn expected_key(&self,key:&pages::Key,region:PixelRect,input_level:u32)->Result<ExpectedKey,GpuRasterError> {
        #[cfg(test)]
        crate::test_support::PassCounts {expected_keys:1,..Default::default()}.record();
        let version=if key.source.is_some() {self.expected_source(&key.expression,region.into(),key.level,input_level)?}
            else {self.expected(&key.expression,region.into(),key.level,input_level)?};
        let mut context=Recipe::new();context.add(&(key.color,key.format));context.tag(&key.blend_space);
        let version=ContentVersion::derive(context.0,&[version]);
        if version.resolved().is_some() {return Ok(ExpectedKey::Ready(version));}
        if let Some(wake)=self.packing_wake(&version) {return Ok(ExpectedKey::Unresolved(wake));}
        if version.resolved().is_some() {Ok(ExpectedKey::Ready(version))}else {Err(GpuRasterError::NotReady)}
    }
    fn packing_wake(&self,version:&ContentVersion)->Option<PackingWake> {
        let wake=self.name_wake.as_ref()?;
        version.register_waker(wake).then(||PackingWake(version.clone()))
    }
    pub(super) fn matches(&self,recorded:&ContentVersion,expected:&ExpectedKey)->Option<bool> {
        if let Some(matches)=recorded.matches(expected.version()) {return Some(matches);}
        if matches!(expected,ExpectedKey::Unresolved(_)) || self.packing_wake(recorded).is_some() {None}
        else {recorded.matches(expected.version()).or(Some(false))}
    }
    pub(super) fn scene(&self)->SceneView<'_> {self.snapshot.scene.view()}
    pub(super) fn authored_revision(&self,node:&graph::Node)->Result<ContentStamp,GpuRasterError> {Ok(self.authored_version(node)?.authored())}
    fn evaluate(&self,request:Request,parent:Option<&Request>)->Result<ContentVersion,GpuRasterError> {
        if let Some(version)=self.memo.lock().unwrap().get(&request,parent) {return Ok(version);}
        let (version,read)=self.compute(&request)?;
        self.memo.lock().unwrap().insert(request,version.clone(),parent,read);Ok(version)
    }
    fn source_versions(&self,id:SourceTarget)->Result<&raster::contents::Versions,GpuRasterError> {
        self.snapshot.rasters.get(&id).ok_or(if self.snapshot.scene.view().source_owner(id).is_some() {GpuRasterError::NotReady}else {GpuRasterError::StaleExpression})
    }
    fn source_version(&self,id:SourceTarget,frame:SourceFrame,extent:[u32;2],region:DocRect,input_level:u32,global:bool)->Result<(ContentVersion,PixelRect),GpuRasterError> {
        let s=&self.snapshot;let scene=s.scene.view();let mut inputs=Vec::new();let mut name=Recipe::new();name.add(&(id,frame,extent,scene.color_mode(id)));
        let moving=s.moving==scene.source_owner(id);let level=s.source_level(id,frame,extent,(!global).then_some(region),input_level,moving);name.add(&level);
        let halo=if frame.aligned() && !moving {0}else {1<<level};let watercolor=s.watercolor.get(&id).copied();name.watercolor(watercolor);
        let local=if global {PixelRect::full(extent)}else {source_footprint(region,frame,extent,level,halo+watercolor.map_or(0,|style|style.radius()))};
        if let Some(capture)=s.captures.get(&id) {inputs.push(capture.content(region,global)?);}
        else {
            if local.is_empty() {return Ok((ContentVersion::derive(name.0,&[]),local));}
            let versions=self.source_versions(id)?;let plane=source_plane(id);
            inputs.push(if global {versions.root(plane)}else {versions.region(plane,local)});
            if watercolor.is_some() {inputs.push(if global {versions.root(RasterPlane::WatercolorWetness)}else {versions.region(RasterPlane::WatercolorWetness,local)});}
        }
        if let Some(preview)=&s.preview && preview.target()==id && preview.affects(local) {
            inputs.push(preview.region_version(source_plane(id),local,PreviewStage::Final)?);
            if watercolor.is_some() {inputs.push(preview.region_version(RasterPlane::WatercolorWetness,local,PreviewStage::Final)?);}
        }
        Ok((ContentVersion::derive(name.0,&inputs),local))
    }
    fn selected_placement(&self,node:&graph::Expression)->Result<Option<ContentStamp>,GpuRasterError> {
        let scene=self.snapshot.scene.view();let mut name=Recipe::new();
        match node {
            graph::Expression::Source {id,..}=>name.add(&(scene.target_offset(*id),scene.target_extent(*id))),
            graph::Expression::Objects {content,..}=> {
                let owner=content.owner();let handle=scene.object_handle(owner).ok_or(GpuRasterError::StaleExpression)?;
                let key=super::super::objects::ObjectKey::new(scene,owner,handle);
                name.add(&(key.handle,key.affine,key.nearest));
            },
            graph::Expression::Vector {owner,..}|graph::Expression::VectorPreview {owner}|graph::Expression::VectorMotion {owner}=>
                name.add(&scene.occurrence_offset64(*owner).map(f64::to_bits)),
            _=>return Ok(None),
        }
        Ok(Some(name.0))
    }
    fn compute(&self,request:&Request)->Result<(ContentVersion,Option<(SourceTarget,PixelRect)>),GpuRasterError> {
        use graph::Expression as E;
        let Request {node:Expression(node),level,input_level,global,source_chunk,..}=request;let (level,input_level,global,source_chunk)=(*level,*input_level,*global,*source_chunk);
        let region=request.region();let mut read=None;
        let child=|node:&graph::Node,region:DocRect,global:bool|self.evaluate(Request::new(node,region,level,input_level,global,false),Some(request));
        let s=&self.snapshot;let scene=s.scene.view();let mut inputs=Vec::new();let mut name=Recipe::new();name.add(&(s.context,level,input_level,global));
        if !source_chunk {name.add(&self.selected_placement(node)?);}
        match node.as_ref() {
            E::Color(color)=>name.add(&(0,color)),
            E::Source {id,frame,extent,outside}=>{name.add(&(1,outside));let frame=if source_chunk {SourceFrame {phase:frame.phase,shift:[0;2]}}else {*frame};let (version,local)=self.source_version(*id,frame,*extent,region,input_level,global)?;inputs.push(version);read=Some((*id,local));},
            E::Objects {content,preview}=>{
                let owner=content.owner();name.add(&(2,owner,preview,scene.visible(owner)));
                let keys=if global {content.keys().to_vec()}else {content.query_at(region,input_level)};
                for key in keys {name.add(&(key.handle,key.affine,key.nearest));inputs.push(scene.object(key.handle).ok_or(GpuRasterError::StaleExpression)?.image.content_version());}
            },
            E::Vector {owner,offset,moving}=>{
                #[cfg(test)] crate::test_support::PassCounts {vector_names:1,..Default::default()}.record();
                use layer_core::authored::vector::{Record,overlaps};
                let reach=vector_reach(region,*offset,level,global);
                let chunks:Vec<_>=scene.vector_layer(*owner).ok_or(GpuRasterError::StaleExpression)?.chunks_reaching(reach).collect();
                name.add(&(3,owner,offset,scene.visible(*owner)));inputs.extend(chunks.iter().map(|chunk|chunk.content_version()));
                let excluded:Vec<_>=moving.iter().flat_map(|moving|chunks.iter().flat_map(|chunk|chunk.records()).filter_map(Record::path)
                    .filter(|path|!path.hidden && moving.0.contains(&path.id) && overlaps(path.bounds(),reach)).map(|path|path.id)).collect();
                if !excluded.is_empty() {inputs.push(ContentVersion::derive(recipe(&excluded),&[]));}
            },
            E::VectorPreview {owner}=>{name.add(&(4,owner,scene.visible(*owner)));inputs.push(s.vector_preview.as_ref().filter(|(preview,_)|preview.owner==*owner).map_or_else(||ContentVersion::derive(0,&[]),|(_,version)|version.clone()));},
            E::VectorMotion {owner}=>{name.add(&(5,owner,scene.visible(*owner)));if let Some((_,version))=s.vector_motion.as_ref().filter(|(motion,_)|motion.owner==*owner) {inputs.push(version.clone());inputs.push(scene.vector_layer(*owner).ok_or(GpuRasterError::StaleExpression)?.content_version());}},
            E::Opacity {input,opacity}=>{name.add(&(6,opacity));inputs.push(child(input,region,global)?);},
            E::Combine {blend:0,flags:0,..}=>{
                let mut operands=Vec::new();over_operands(node,&mut operands);
                if let [operand]=operands[..] {return Ok((child(operand,region,global)?,None));}
                name.add(&(7,0u32,0u32));for operand in operands {inputs.push(child(operand,region,global)?);}
            },
            E::Combine {front,back,blend,flags}=>{name.add(&(7,blend,flags));inputs.push(child(front,region,global)?);inputs.push(child(back,region,global)?);},
            E::Effect {input,chain,masks,..}=>{
                name.add(&8);let mut footprint=region;let mut entire=global;
                for (handle,_,flags) in chain {
                    let effect=scene.effect(*handle).ok_or(GpuRasterError::StaleExpression)?;let definition=s.effects.get(handle).ok_or(GpuRasterError::Invariant("effect signature definition is missing"))?;name.add(&(handle,flags,scene.visible(*handle)));inputs.push(definition.version.clone());entire|=definition.global;
                    if !entire && !effect.program.passes.is_empty() {footprint=crate::effects::document_pass_regions(effect,footprint,input.support(scene,scene.composition().size,input_level),input_level).first().copied().ok_or(GpuRasterError::Invariant("effect footprint is missing"))?;}
                }
                inputs.push(child(input,footprint,entire)?);
                for mask in masks {inputs.push(match mask {Some(mask)=>child(mask,region,global)?,None=>ContentVersion::derive(0,&[])});}
            },
        }
        Ok((ContentVersion::derive(name.0,&inputs),read))
    }
    fn authored_version(&self,node:&graph::Node)->Result<ContentVersion,GpuRasterError> {self.authored_entry(node).map(|entry|entry.version)}
    fn authored_entry(&self,node:&graph::Node)->Result<Authored,GpuRasterError> {
        let key=Expression(node.clone());if let Some(entry)=self.authored.lock().unwrap().get(&key) {return Ok(entry.clone());}
        use graph::Expression as E;let s=&self.snapshot;let scene=s.scene.view();let mut name=Recipe::new();name.add(&s.context);name.add(&self.selected_placement(node)?);let mut inputs=Vec::new();
        let mut reads:Arc<[SourceTarget]>=Arc::from([]);
        let mut child=|node:&graph::Node,inputs:&mut Vec<ContentVersion>|->Result<(),GpuRasterError> {let child=self.authored_entry(node)?;read_together(&mut reads,&child.reads);inputs.push(child.version);Ok(())};
        match node.as_ref() {
            E::Color(color)=>name.add(&(0,color)),
            E::Source {id,outside,..}=>{
                reads=Arc::from([*id]);
                name.add(&(1,id,outside,scene.color_mode(*id)));name.watercolor(s.watercolor.get(id).copied());
                if let Some(capture)=s.captures.get(id) {inputs.push(capture.authored_content());}else {
                    let versions=self.source_versions(*id)?;inputs.push(versions.root(source_plane(*id)));
                    if s.watercolor.contains_key(id) {inputs.push(versions.root(RasterPlane::WatercolorWetness));}
                }
                if let Some(preview)=&s.preview && preview.target()==*id {inputs.push(preview.authored_version()?);}
            },
            E::Objects {content,..}=>{
                let owner=content.owner();name.add(&(2,owner,scene.visible(owner)));
                for key in content.keys().iter() {let object=scene.object(key.handle).ok_or(GpuRasterError::StaleExpression)?;name.add(&key.handle);inputs.push(object.image.content_version());}
            },
            E::Vector {owner,..}|E::VectorPreview {owner}|E::VectorMotion {owner}=>{
                name.add(&(3,owner,scene.visible(*owner)));inputs.push(scene.vector_layer(*owner).ok_or(GpuRasterError::StaleExpression)?.content_version());
            },
            E::Opacity {input,opacity}=>{name.add(&(6,opacity));child(input,&mut inputs)?;},
            E::Combine {blend:0,flags:0,..}=>{
                let mut operands=Vec::new();over_operands(node,&mut operands);
                if let [operand]=operands[..] {let entry=self.authored_entry(operand)?;self.authored.lock().unwrap().insert(key,entry.clone());return Ok(entry);}
                name.add(&(7,0u32,0u32));for operand in operands {child(operand,&mut inputs)?;}
            },
            E::Combine {front,back,blend,flags}=>{name.add(&(7,blend,flags));child(front,&mut inputs)?;child(back,&mut inputs)?;},
            E::Effect {input,chain,masks,..}=>{
                name.add(&8);child(input,&mut inputs)?;
                for (handle,_,flags) in chain {name.add(&(handle,flags,scene.visible(*handle)));let definition=s.effects.get(handle).ok_or(if scene.effect(*handle).is_some() {GpuRasterError::Invariant("effect signature definition is missing")}else {GpuRasterError::StaleExpression})?;inputs.push(definition.version.clone());}
                for mask in masks {match mask {Some(mask)=>child(mask,&mut inputs)?,None=>inputs.push(ContentVersion::derive(0,&[]))}}
            },
        }
        let entry=Authored {version:ContentVersion::derive(name.0,&inputs),reads};self.authored.lock().unwrap().insert(key,entry.clone());Ok(entry)
    }
}
fn vector_reach(region:DocRect,offset:[u64;2],level:u32,global:bool)->[f64;4] {
    if global {return [f64::NEG_INFINITY,f64::NEG_INFINITY,f64::INFINITY,f64::INFINITY];}
    let [x,y]=offset.map(f64::from_bits);let pad=f64::from(1u32<<level);
    [region.min[0] as f64-x-pad,region.min[1] as f64-y-pad,region.max[0] as f64-x+pad,region.max[1] as f64-y+pad]
}
fn over_operands<'a>(node:&'a graph::Node,operands:&mut Vec<&'a graph::Node>) {
    match node.as_ref() {
        graph::Expression::Combine {front,back,blend:0,flags:0}=>{over_operands(front,operands);over_operands(back,operands);},
        _ if graph::Expression::is_clear(node)=>{},
        _=>operands.push(node),
    }
}
fn source_plane(target:SourceTarget)->RasterPlane {if matches!(target,SourceTarget::Paint(_)) {RasterPlane::Color}else {RasterPlane::Mask}}
fn source_footprint(region:DocRect,frame:SourceFrame,extent:[u32;2],level:u32,halo:u32)->PixelRect {
    let phased=frame.local(region,extent).expand(halo,frame.extent(extent));
    DocRect::from(phased).aligned(1<<level).translated(frame.phase.map(|value|-(value as i64))).in_frame(extent)
}

fn path_version(path:&layer_core::authored::vector::PathItem)->ContentVersion {
    use layer_core::authored::vector::{Paint,PaintEntry};
    let mut name=Recipe::new();name.add(&(path.fill_rule,path.hidden,path.style.entries.len()));
    let paint=|name:&mut Recipe,paint:&Paint| {name.tag(paint);if let Paint::Color {color,..}=paint {name.color(*color);}};
    for entry in path.style.entries.iter() {
        name.tag(entry);
        match entry {
            PaintEntry::Fill(fill)=>paint(&mut name,&fill.paint),
            PaintEntry::Stroke(stroke)=>{
                paint(&mut name,&stroke.paint);name.add(&(stroke.width.to_bits(),stroke.cap,stroke.join,stroke.miter_limit.to_bits(),stroke.profile.is_some()));
                if let Some(profile)=&stroke.profile {name.add(&profile.knots().len());for knot in profile.knots() {name.add(&knot.segment());name.floats(knot.bits());}}
            },
            PaintEntry::Opaque(_)=>{},
        }
    }
    ContentVersion::derive(name.0,&[path.geometry.content_version()])
}
fn cut_version(preview:&layer_render::TransformPreview)->ContentVersion {
    let transform=preview.drawn();let mut name=Recipe::new();let mut inputs=Vec::new();
    name.add(&(preview.target,transform.keep_source,transform.source_from_owner.map(|map|map.0.map(f32::to_bits))));
    if let Some(base)=&transform.source_base {let mut base_name=Recipe::new();base_name.add(&base.offset);base_name.tag(&base.policy);inputs.push(ContentVersion::derive(base_name.0,&[base.image.content_version()]));}
    if let Some(selection)=&preview.selection {inputs.push(selection.content_version());}
    ContentVersion::derive(name.0,&inputs)
}
fn transform_version(preview:&layer_render::TransformPreview)->ContentVersion {
    let transform=preview.drawn();let mut name=Recipe::new();
    name.floats(transform.placement.outer.0);name.tag(&transform.placement.interpolation);name.add(&transform.placement.mesh.is_some());
    if let Some(mesh)=&transform.placement.mesh {
        name.floats(mesh.frame.0);for breaks in &mesh.breakpoints {name.add(&breaks.len());name.floats(breaks.iter().copied());}
        name.add(&mesh.net.len());for point in mesh.net.iter() {name.floats([point.x,point.y]);}
    }
    ContentVersion::derive(name.0,&[cut_version(preview)])
}
fn preview_version(packet:FramePacket<'_>,batch:&DabBatch,captured:Option<&ContentVersion>)->Result<Option<ContentVersion>,GpuRasterError> {
    #[cfg(test)] crate::test_support::PassCounts {preview_definitions:1,..Default::default()}.record();
    let mut inputs=Vec::new();let style=&batch.style;
    if style.execution.retouches() {let Some(captured)=captured else {return Ok(None);};inputs.push(captured.clone());}
    if let Some(context)=&style.liquify {
        for source in [&context.current,&context.original] {
            inputs.push(match paint_definition(source) {Ok(version)=>version,Err(GpuRasterError::NotReady)=>{let Some(captured)=captured else {return Ok(None);};captured.clone()},Err(error)=>return Err(error)});
        }
        if let Some(field)=&context.field {inputs.push(field.content_version());}
        inputs.push(ContentVersion::derive(recipe(&context.amount.map(f32::to_bits)),&[]));
    }
    if let Some(selection)=&style.selection {inputs.push(selection.content_version());}
    if let Some(paper)=&style.paper {inputs.push(ContentVersion::derive(recipe(&(paper.surface.mapping.0.map(f64::to_bits),paper.target_offset)),&[texture_version(&paper.surface.texture)?]));}
    let dabs=packet.dabs.get(batch.first_dab as usize..(batch.first_dab+batch.dab_count) as usize).ok_or(GpuRasterError::InvalidDabRange)?;
    let mut name=Recipe::new();name.add(&(batch.material_update,batch.preview_end,batch.stroke_id,batch.target,batch.stroke_start,batch.stroke_end));name.style(style);name.add(&dabs.len());
    for dab in dabs {
        name.floats([dab.center.x,dab.center.y]);name.floats(dab.radii);name.floats(dab.rotation);name.floats(dab.motion);name.floats(dab.color_rgba_linear);
        name.floats([dab.flow,dab.hardness]);name.floats(dab.texture_sign);name.floats(dab.material);name.floats(dab.previous);name.floats(dab.contact);name.floats(dab.previous_contact);
    }
    Ok(Some(ContentVersion::derive(name.0,&inputs)))
}
fn paint_definition(source:&layer_core::authored::PaintSource)->Result<ContentVersion,GpuRasterError> {
    let data=source.raster.try_data().ok_or(GpuRasterError::NotReady)?.map_err(GpuRasterError::Effect)?;
    let mut inputs=[RasterPlane::Color,RasterPlane::Mask,RasterPlane::WatercolorWetness].map(|plane|data.tiles.content(plane).root_version()).to_vec();let mut name=Recipe::new();name.add(&(source.domain,source.color_mode));
    if let Some(style)=data.watercolor {name.floats([style.wet_edge,style.burnt_edge,style.edge_width]);}
    if let Some(base)=&source.base {let mut base_name=Recipe::new();base_name.add(&base.offset);base_name.tag(&base.policy);inputs.push(ContentVersion::derive(base_name.0,&[base.image.content_version()]));}
    Ok(ContentVersion::derive(name.0,&inputs))
}
fn texture_version(texture:&layer_core::TextureSource)->Result<ContentVersion,GpuRasterError> {
    match texture {layer_core::TextureSource::Image(image)=>Ok(image.content_version()),layer_core::TextureSource::Program(instance)=>effect_version(layer_core::EffectView::new(&instance.program,&instance.values))}
}
fn value_version(value:&layer_core::EffectValue)->Result<ContentVersion,GpuRasterError> {
    use layer_core::EffectValue as V;let mut name=Recipe::new();name.tag(value);let mut inputs=Vec::new();
    match value {
        V::Number(value)=>name.floats([*value]),V::Toggle(value)=>name.add(value),V::Choice(value)=>name.add(value),V::Color(color)=>name.color(*color),
        V::Curve(points)=>{name.add(&points.len());for point in points {name.floats(*point);}},
        V::Gradient(gradient)=>{name.tag(&gradient.interpolation);name.add(&gradient.stops.len());for stop in &gradient.stops {name.floats([stop.position]);name.color(stop.color);}},
        V::Lut3d(lut)=>{name.add(&lut.is_some());if let Some(lut)=lut {name.add(&(lut.digest(),lut.size()));for domain in lut.domain() {name.floats(domain);}}},
        V::Texture(texture)=>{name.add(&texture.is_some());if let Some(texture)=texture {inputs.push(texture_version(texture)?);}},
    }
    Ok(ContentVersion::derive(name.0,&inputs))
}
fn sampling(name:&mut Recipe,value:&layer_core::EffectSampling) {
    use layer_core::EffectSampling as S;name.tag(value);
    match value {S::Neighborhood {radius}=>name.add(radius),S::Parameter {key,scale,padding}=>name.add(&(key,scale.to_bits(),padding)),S::Sum {parameters,padding}=>{name.add(&(parameters.len(),padding));for parameter in parameters.iter() {name.add(&(&parameter.key,parameter.scale.to_bits()));}},S::Document=>{}}
}
#[cfg(test)]
fn record_read() {crate::test_support::PassCounts {signature_reads:1,..Default::default()}.record();}
fn effect_version(effect:layer_core::EffectView<'_>)->Result<ContentVersion,GpuRasterError> {
    #[cfg(test)] record_read();
    let p=effect.program;let mut inputs=Vec::new();let mut name=Recipe::new();
    name.add(&(p.abi,&p.id,&p.constant_color,&p.entry,p.period.map(f32::to_bits),p.time));name.tag(&p.kind);name.tag(&p.alpha);name.tag(&p.space);name.tag(&p.resolution);
    name.add(&p.passes.len());for pass in p.passes.iter() {name.add(&pass.entry);sampling(&mut name,&pass.sampling);}
    name.add(&p.auxiliary.is_some());if let Some(auxiliary)=&p.auxiliary {name.tag(auxiliary);match auxiliary {layer_core::EffectAuxiliary::Lut3d {resource,color_space}=>name.add(&(resource,color_space)),layer_core::EffectAuxiliary::Analysis {analysis}=>name.add(analysis)}}
    if let Some(spatial)=effect.spatial {name.add(&(spatial.mapping.0.map(f64::to_bits),spatial.extent.map(f64::to_bits)));}
    for shader in p.wgsl.sources().map_err(|error|GpuRasterError::Effect(error.into()))? {inputs.push(shader.content_version());}
    name.add(&p.lookups.len());for lookup in p.lookups.iter() {
        name.add(&(&lookup.entry,&lookup.dependencies,lookup.values,lookup.workgroup_size,lookup.workgroups));
        for shader in lookup.wgsl.sources().map_err(|error|GpuRasterError::Effect(error.into()))? {inputs.push(shader.content_version());}
    }
    name.add(&p.parameters.len());for (index,parameter) in p.parameters.iter().enumerate() {
        name.add(&parameter.key);name.tag(&parameter.dimension);name.tag(&parameter.kind);
        if let layer_core::EffectParameterKind::Choice {options}=&parameter.kind {name.add(&options.len());for option in options.iter() {name.add(&option.value());}}
        inputs.push(value_version(effect.values.get(index).unwrap_or(&parameter.default))?);
    }
    Ok(ContentVersion::derive(name.0,&inputs))
}
struct Recipe(ContentStamp);
impl Recipe {
    fn new()->Self {Self(0x6c62272e07bb014262b821756295c58d)}
    fn add<T:Hash+?Sized>(&mut self,value:&T) {value.hash(self);}
    fn tag<T>(&mut self,value:&T) {self.add(&std::mem::discriminant(value));}
    fn floats(&mut self,values:impl IntoIterator<Item=f32>) {for value in values {self.add(&value.to_bits());}}
    fn color(&mut self,color:layer_core::color::RgbColor) {self.add(&(color.space,color.rgba.map(f32::to_bits),color.linear_rgb.map(|rgb|rgb.map(f32::to_bits))));}
    fn watercolor(&mut self,style:Option<WatercolorLayerStyle>) {self.add(&style.is_some());if let Some(style)=style {self.floats([style.wet_edge,style.burnt_edge,style.edge_width]);}}
    fn style(&mut self,style:&layer_render::DabStyle) {
        self.floats([style.diameter,style.opacity]);self.add(&style.alpha_locked);self.tag(&style.tip);if let BrushTip::Mask(mask)=&style.tip {self.add(mask);}
        self.tag(&style.mode);self.tag(&style.execution);self.tag(&style.blend_space);self.add(&style.grain.map(|grain|grain.depth.to_bits()));
        let r=style.rendering;self.tag(&r.blend_mode);self.tag(&r.accumulation);self.floats([r.alpha_threshold,r.wet_edge,r.burnt_edge,r.edge_width]);self.add(&r.edge_after_stroke);
        let w=style.wet_mix;self.tag(&w.mix_space);self.floats([w.amount_of_paint,w.density,w.charge,w.charge_depletion,w.dilution,w.attack,w.pull,w.blur,w.wetness_jitter,w.wetness,w.average,w.refill,w.stretch]);
        self.add(&style.transport.is_some());if let Some(t)=&style.transport {self.add(&t.conductance);self.floats([t.scale,t.rotation_radians,t.contrast,t.wet_flow,t.dry_flow,t.distance,t.water_load]);}
        let d=style.deform;self.tag(&d.mode);self.floats([d.strength,d.pressure,d.momentum,d.distortion]);
        let b=style.blender;self.tag(&b.kind);self.floats([b.reach,b.rate,b.character,b.scale,b.cell_size,b.pressure_gain,b.engagement,b.recess,b.lift]);
        self.add(&style.contact.is_some());if let Some(c)=style.contact {
            self.floats([c.paper,c.pressure_gain,c.edge_roughness,c.edge_scale,c.fibers,c.fiber_strength,c.pooling,c.depletion]);self.add(&c.linear_edge);
            self.add(&c.bristles.is_some());if let Some(b)=c.bristles {self.floats([b.texture_scale,b.splay,b.load]);self.floats(b.streak_rgba_linear);}
            self.add(&c.dry.is_some());if let Some(d)=c.dry {for pose in d.poses {self.floats(pose.radii);self.floats(pose.relief);}self.floats([d.softness,d.loading,d.corner,d.relief_offset]);}
        }
        self.add(&style.retouch.is_some());if let Some(r)=&style.retouch {self.add(&(r.source,&r.references,r.flip));self.floats(r.offset);}
    }
}
impl Hasher for Recipe {
    fn finish(&self)->u64 {self.0 as u64^(self.0>>64) as u64}
    fn write(&mut self,bytes:&[u8]) {for byte in bytes {self.0=(self.0^u128::from(*byte)).wrapping_mul(0x0000000001000000000000000000013b);}}
}
fn recipe(value:&impl Hash)->ContentStamp {let mut recipe=Recipe::new();recipe.add(value);recipe.0}

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) struct PreviewQuality { pub level:u32, pub contribution:bool, pub requires_base:bool }
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum PreviewStage { Seed, BeforeBatch(usize), AfterBatch(usize), BeforeFill, Final }
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub(crate) enum PreviewPass { Color, Coverage, Wetness, Transport, StrokeStart, Reservoir }
#[derive(Clone)]
pub(crate) struct PreviewVersion {
    declaration:Arc<PreviewDeclaration>,
    input:raster::contents::Intent,
    version:ContentVersion,
    plane:RasterPlane,
    coordinate:[u32;2],
    stage:PreviewStage,
    pass:PreviewPass,
    step:Option<(PreviewPass,u32)>,
}
impl PreviewVersion {
    pub fn input(&self)->&raster::contents::Intent {&self.input}
    pub fn version(&self)->&ContentVersion {&self.version}
    pub fn same_declaration(&self,declaration:&Arc<PreviewDeclaration>)->bool {Arc::ptr_eq(&self.declaration.accepted,&declaration.accepted)}
    pub fn intermediate(&self,pass:PreviewPass,step:u32)->Self {
        if !matches!(self.plane,RasterPlane::Color|RasterPlane::WatercolorWetness) || !self.declaration.transport_output(self.coordinate,self.stage) {return self.clone();}
        Self {version:ContentVersion::derive(recipe(&(pass,step)),std::slice::from_ref(&self.version)),step:Some((pass,step)),..self.clone()}
    }
}
#[derive(Clone)]
struct PreviewCommand {index:usize,batch:DabBatch,dabs:Vec<Dab>,outputs:std::collections::BTreeSet<[u32;2]>,definition:Option<ContentVersion>}
impl PreviewCommand {
    fn intersects(&self,region:PixelRect)->bool {self.outputs.iter().any(|coordinate|!page_rect(*coordinate).intersect(region).is_empty())}
    fn coordinates(&self,region:PixelRect)->impl Iterator<Item=[u32;2]>+'_ {self.outputs.iter().copied().filter(move |coordinate|!page_rect(*coordinate).intersect(region).is_empty())}
}
#[derive(Clone)]
pub(crate) struct PreviewSeed {pub plane:RasterPlane,pub coordinate:[u32;2],pub pass:PreviewPass,pub versions:Vec<ContentVersion>,pub committed:bool}
#[derive(Clone)]
struct PreviewFill {operation:layer_core::RasterOperation,definition:ContentVersion,coverage:Option<raster::contents::Versions>}
#[derive(Clone)]
pub(crate) struct PreviewDeclaration {
    source:SceneSnapshot,
    target:SourceTarget,
    versions:raster::contents::Versions,
    inputs:IdMap<SourceTarget,Input>,
    commands:Vec<PreviewCommand>,
    fill:Option<PreviewFill>,
    quality:PreviewQuality,
    wetness_before:IdMap<usize,usize>,
    transport_outputs:IdMap<usize,std::collections::BTreeSet<[u32;2]>>,
    seed_inputs:IdMap<AcceptedKey,(ContentVersion,bool)>,
    selection:Option<raster::contents::Intent>,
    context:ContentStamp,
    accepted:Arc<Mutex<IdMap<AcceptedKey,CapturedInputs>>>,
    memo:Arc<Mutex<IdMap<PreviewRequest,ContentVersion>>>,
    stamps:Option<(Arc<PreviewStamps>,ContentStamp)>,
}
#[derive(Default)]
pub(crate) struct PreviewStamps {accepted:Mutex<IdSet<[u32;2]>>}
#[derive(Clone,Copy,PartialEq,Eq,Hash)]
struct AcceptedKey {plane:usize,coordinate:[u32;2],stage:PreviewStage,pass:PreviewPass}
struct CapturedInputs {_proofs:Vec<resident::Input>,version:ContentVersion}
#[derive(Clone,Copy,PartialEq,Eq,Hash)]
struct PreviewRequest {plane:usize,bounds:[u32;4],stage:PreviewStage,pass:PreviewPass}
impl PreviewDeclaration {
    pub fn capture(r:&WgpuRasterizer,packet:FramePacket<'_>,quality:PreviewQuality,inputs:IdMap<SourceTarget,Input>)->Result<Option<Self>,GpuRasterError> {
        let plans=brush_tiles::plan_packet(packet,|target|packet.scene.target_extent(target))?;
        let rectangular=r.preview_from_persistent(packet.dab_batches) && !WgpuRasterizer::sparse_preview(packet.dab_batches);
        let plans:Vec<_>=packet.dab_batches.iter().zip(plans).map(|(batch,tiles)|brush_tiles::destinations(&tiles,batch_pixel_rect(batch,packet.scene.target_extent(batch.target)),batch.kind==DabBatchKind::Preview && rectangular,batch.first_dab).into_owned()).collect();
        Self::capture_with_plans(r,packet,quality,inputs,&plans,None)
    }
    pub fn capture_with_plans(r:&WgpuRasterizer,packet:FramePacket<'_>,quality:PreviewQuality,inputs:IdMap<SourceTarget,Input>,plans:&[Vec<BrushTile>],stamps:Option<Arc<PreviewStamps>>)->Result<Option<Self>,GpuRasterError> {
        let target=packet.dab_batches.iter().find(|batch|batch.kind==DabBatchKind::Preview).map(|batch|batch.target).or_else(||r.fill_preview.as_ref().map(|fill|fill.target));
        let Some(target)=target else {return Ok(None);};
        let mut versions=IdMap::default();versions.insert(target,r.raster_versions(packet.scene,target)?);
        let mut declaration=Self::from_plans(packet,versions,inputs,quality,r.fill_preview.clone(),plans,stamps)?;
        if let Some(declaration)=&mut declaration && let Some(fill)=&mut declaration.fill {
            fill.coverage=Some(r.coverage_versions(&fill.operation.coverage)?);
        }
        if let Some(declaration)=&mut declaration {declaration.selection=Some(r.raster_intent(target,source_plane(target),[0;2])?);}
        Ok(declaration)
    }
    pub fn from_scene(packet:FramePacket<'_>,versions:IdMap<SourceTarget,raster::contents::Versions>,inputs:IdMap<SourceTarget,Input>,quality:PreviewQuality,
        fill:Option<layer_render::FillPreview>,stamps:Option<Arc<PreviewStamps>>)->Result<Option<Self>,GpuRasterError> {
        let plans=brush_tiles::plan_packet(packet,|target|packet.scene.target_extent(target))?;
        let rectangular=quality.requires_base && !WgpuRasterizer::sparse_preview(packet.dab_batches);
        let plans:Vec<_>=packet.dab_batches.iter().zip(plans).map(|(batch,tiles)|brush_tiles::destinations(&tiles,batch_pixel_rect(batch,packet.scene.target_extent(batch.target)),batch.kind==DabBatchKind::Preview && rectangular,batch.first_dab).into_owned()).collect();
        Self::from_plans(packet,versions,inputs,quality,fill,&plans,stamps)
    }
    fn from_plans(packet:FramePacket<'_>,mut versions:IdMap<SourceTarget,raster::contents::Versions>,inputs:IdMap<SourceTarget,Input>,quality:PreviewQuality,
        fill:Option<layer_render::FillPreview>,plans:&[Vec<BrushTile>],stamps:Option<Arc<PreviewStamps>>)->Result<Option<Self>,GpuRasterError> {
        let target=packet.dab_batches.iter().find(|batch|batch.kind==DabBatchKind::Preview).map(|batch|batch.target).or_else(||fill.as_ref().map(|fill|fill.target));
        let Some(target)=target else {return Ok(None);};
        if packet.dab_batches.iter().any(|batch|batch.kind==DabBatchKind::Preview && batch.target!=target) || fill.as_ref().is_some_and(|fill|fill.target!=target) {return Err(GpuRasterError::MultiplePreviewLayers);}
        if plans.len()!=packet.dab_batches.len() {return Err(GpuRasterError::InvalidDabRange);}
        let versions=match versions.remove(&target) {Some(versions)=>versions,None=>raster::contents::declared_versions(packet.scene,target)?};
        let stamped=stamps.is_some() && fill.is_none();
        let commands:Vec<PreviewCommand>=packet.dab_batches.iter().enumerate().filter(|(_,batch)|batch.kind==DabBatchKind::Preview).map(|(index,batch)| {
            let end=batch.first_dab.checked_add(batch.dab_count).ok_or(GpuRasterError::InvalidDabRange)?;
            let dabs=packet.dabs.get(batch.first_dab as usize..end as usize).ok_or(GpuRasterError::InvalidDabRange)?;
            let outputs=if target.is_coverage() {page_coordinates(batch_pixel_rect(batch,packet.scene.target_extent(target))).collect()}else {plans[index].iter().map(|tile|tile.coordinate).collect()};
            if stamped {return Ok(PreviewCommand {index,batch:batch.clone(),dabs:Vec::new(),outputs,definition:None});}
            let mut definition=preview_version(packet,batch,inputs.get(&target).map(|input|&input.version))?;
            if let Some(captured)=inputs.get(&target) {definition=definition.map(|definition|ContentVersion::derive(0x696e707574,&[definition,captured.version.clone()]));}
            Ok(PreviewCommand {index,batch:batch.clone(),dabs:dabs.to_vec(),outputs,definition})
        }).collect::<Result<_,GpuRasterError>>()?;
        let fill=fill.map(|fill| {
            let operation=fill.operation;let definition=operation_version(&operation,inputs.get(&target))?;
            let coverage=match raster::contents::declared_coverage_versions(&operation.coverage.source) {Ok(versions)=>Some(versions),Err(GpuRasterError::NotReady)=>None,Err(error)=>return Err(error)};
            Ok(PreviewFill {operation,definition,coverage})
        }).transpose()?;
        let mut wetness_before=IdMap::default();let mut first=0;
        for (index,batch) in packet.dab_batches.iter().enumerate() {
            if index==0 || !same_material_update(batch,&packet.dab_batches[index-1]) {first=index;}
            if batch.kind==DabBatchKind::Preview && batch.style.execution==BrushExecution::Watercolor {wetness_before.insert(index,first);}
        }
        let transport_outputs=commands.iter().filter(|command|command.batch.style.execution==BrushExecution::Watercolor && command.batch.style.transport.as_ref().is_some_and(|transport|transport.distance>=1. && (transport.wet_flow>0. || transport.dry_flow>0.)) && command.batch.dab_count!=0 && is_last_watercolor_update_batch(packet.dab_batches,command.index)).map(|command| {
            let outputs=commands.iter().filter(|other|same_material_update(&other.batch,&command.batch) && wetness_before.get(&other.index)==wetness_before.get(&command.index)).flat_map(|other|other.outputs.iter().copied()).collect();
            (command.index,outputs)
        }).collect();
        let mut context=Recipe::new();context.add(&(packet.document_extent,packet.scene.composition().color,packet.scene.color_mode(target)));context.tag(&packet.blend_space);
        Ok(Some(Self {source:packet.scene.snapshot(),target,versions,inputs,commands,fill,quality,wetness_before,transport_outputs,seed_inputs:IdMap::default(),selection:None,context:context.0,accepted:Arc::default(),memo:Arc::default(),
            stamps:stamps.map(|stamps|(stamps,layer_core::raster::ContentName::fresh().stamp()))}))
    }
    pub fn captured_version(pass:PreviewPass,versions:&[ContentVersion])->ContentVersion {
        let mut distinct=versions.to_vec();distinct.sort_unstable_by_key(ContentVersion::identity);distinct.dedup_by_key(|version|version.identity());
        ContentVersion::derive(recipe(&pass),&distinct)
    }
    pub fn matches_metadata(&self,candidate:&Self)->Result<bool,GpuRasterError> {
        if self.target!=candidate.target || self.quality!=candidate.quality || self.inputs.len()!=candidate.inputs.len()
            || self.wetness_before!=candidate.wetness_before || self.transport_outputs!=candidate.transport_outputs
            || !self.commands.iter().map(|command|(command.index,&command.outputs))
                .eq(candidate.commands.iter().map(|command|(command.index,&command.outputs))) {return Ok(false);}
        if !self.selection.as_ref().zip(candidate.selection.as_ref()).is_some_and(|(held,current)|held.same_selection(current)) {return Ok(false);}
        for (target,held) in &self.inputs {if candidate.inputs.get(target).is_none_or(|input|held.version.matches(&input.version)!=Some(true)) {return Ok(false);}}
        match (self.program_version(),candidate.program_version()) {
            (Ok(held),Ok(wanted))=>Ok(held.matches(&wanted)==Some(true)),
            (Err(GpuRasterError::NotReady),_)|(_,Err(GpuRasterError::NotReady))=>Ok(false),
            (Err(error),_)|(_,Err(error))=>Err(error),
        }
    }
    pub fn matches_history(&self,mut current:impl FnMut(RasterPlane,[u32;2],PreviewPass)->Option<ContentVersion>)->bool {
        self.required_seed_inputs().into_iter().all(|(plane,coordinate,pass)| {
            let key=AcceptedKey {plane:raster::contents::index(plane),coordinate,stage:PreviewStage::Seed,pass};
            self.seed_inputs.get(&key).is_some_and(|(held,_)|current(plane,coordinate,pass).is_some_and(|current|held.matches(&current)==Some(true)))
        })
    }
    pub fn target(&self)->SourceTarget {self.target}
    pub(crate) fn contact_outputs(&self)->Option<impl Iterator<Item=[u32;2]>+'_> {
        self.fill.is_none().then(||self.commands.iter().flat_map(|command|command.outputs.iter().copied()))
    }
    pub(crate) fn level(&self)->u32 {self.quality.level}
    pub fn in_contact(&self)->bool {self.stamps.is_some()}
    pub fn finishes(&self,ticket:&PreviewVersion,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2])->bool {
        if target!=self.target || !Arc::ptr_eq(&ticket.declaration.accepted,&self.accepted) || ticket.plane!=plane || ticket.coordinate!=coordinate
            || ticket.pass!=plane_pass(plane) || ticket.step.is_some() {return false;}
        match ticket.stage {
            PreviewStage::Final=>true,
            PreviewStage::Seed=>self.fill.is_none() && {
                let footprint=self.footprint(page_rect(coordinate).intersect(PixelRect::full(self.source.view().target_extent(target))),PreviewStage::Final);
                !self.commands.iter().any(|command|command.intersects(footprint))
            },
            PreviewStage::AfterBatch(index)=>self.stamps.is_some() && self.fill.is_none() && self.commands.iter().map(|command|command.index).max()==Some(index),
            PreviewStage::BeforeBatch(_)|PreviewStage::BeforeFill=>false,
        }
    }
    pub fn accepted(&self,coordinate:[u32;2])->bool {self.stamps.as_ref().is_some_and(|(stamps,_)|stamps.accepted.lock().unwrap().contains(&coordinate))}
    pub fn accepts(&self,coordinate:[u32;2])->bool {self.stamps.as_ref().is_none_or(|(stamps,_)|stamps.accepted.lock().unwrap().insert(coordinate))}
    pub fn affects(&self,region:PixelRect)->bool {self.fill.is_some() || self.commands.iter().any(|command|command.intersects(region))}
    pub fn required_seed_inputs(&self)->Vec<(RasterPlane,[u32;2],PreviewPass)> {
        let mut keys=std::collections::BTreeSet::new();
        if !self.target.is_coverage() {for command in &self.commands {
            let style=&command.batch.style;let plan=BrushPassPlan::for_style(style);
            let roles=[(plan.state.coverage,PreviewPass::Coverage),(WgpuRasterizer::tracks_stroke_start(&command.batch),PreviewPass::StrokeStart),
                (style.execution==BrushExecution::Wet && !command.batch.stroke_start,PreviewPass::Reservoir)];
            for (needed,pass) in roles {if needed {for coordinate in &command.outputs {keys.insert((raster::contents::index(RasterPlane::Color),*coordinate,pass));}}}
        }}
        keys.into_iter().map(|(plane,coordinate,pass)|([RasterPlane::Color,RasterPlane::Mask,RasterPlane::WatercolorWetness][plane],coordinate,pass)).collect()
    }
    pub fn seed_stage(&self,plane:RasterPlane,coordinate:[u32;2],pass:PreviewPass)->Option<PreviewStage> {
        if plane!=RasterPlane::Color {return None;}
        self.commands.iter().find(|command|command.outputs.contains(&coordinate) && match pass {
            PreviewPass::Coverage=>BrushPassPlan::for_style(&command.batch.style).state.coverage,
            PreviewPass::StrokeStart=>WgpuRasterizer::tracks_stroke_start(&command.batch),
            PreviewPass::Reservoir=>command.batch.style.execution==BrushExecution::Wet && !command.batch.stroke_start,
            _=>false,
        }).map(|command|PreviewStage::BeforeBatch(command.index))
    }
    pub fn committed_seed(&self,coordinate:[u32;2])->bool {
        self.seed_inputs.get(&AcceptedKey {plane:raster::contents::index(RasterPlane::Color),coordinate,stage:PreviewStage::Seed,pass:PreviewPass::StrokeStart}).is_some_and(|(_,committed)|*committed)
    }
    pub fn seed_batch(&self,plane:RasterPlane,coordinate:[u32;2],pass:PreviewPass)->Option<&DabBatch> {
        let PreviewStage::BeforeBatch(index)=self.seed_stage(plane,coordinate,pass)? else {return None;};
        self.commands.iter().find(|command|command.index==index).map(|command|&command.batch)
    }
    pub fn with_seed_inputs(mut self,seeds:Vec<PreviewSeed>)->Result<Self,GpuRasterError> {
        for seed in seeds {
            let key=AcceptedKey {plane:raster::contents::index(seed.plane),coordinate:seed.coordinate,stage:PreviewStage::Seed,pass:seed.pass};
            let version=Self::captured_version(seed.pass,&seed.versions);
            if let Some((previous,_))=self.seed_inputs.insert(key,(version.clone(),seed.committed)) && previous.matches(&version)!=Some(true) {return Err(GpuRasterError::Effect("Preview seed inputs changed after declaration".into()));}
        }
        Ok(self)
    }
    fn transport_output(&self,coordinate:[u32;2],stage:PreviewStage)->bool {
        match stage {PreviewStage::AfterBatch(index)=>self.transport_outputs.get(&index).is_some_and(|outputs|outputs.contains(&coordinate)),_=>false}
    }
    pub fn source_stage(&self,plane:RasterPlane,index:usize)->PreviewStage {
        PreviewStage::BeforeBatch(if plane==RasterPlane::WatercolorWetness {self.wetness_before.get(&index).copied().unwrap_or(index)}else {index})
    }
    fn page_stage(&self,plane:RasterPlane,coordinate:[u32;2],stage:PreviewStage)->PreviewStage {
        let stage=if let PreviewStage::BeforeBatch(index)=stage {self.source_stage(plane,index)}else {stage};
        if stage==PreviewStage::Seed || stage==PreviewStage::Final && self.fill.is_some() {return stage;}
        self.commands.iter().filter(|command|command.outputs.contains(&coordinate) && match stage {
            PreviewStage::Seed=>false,PreviewStage::BeforeBatch(index)=>command.index<index,PreviewStage::AfterBatch(index)=>command.index<=index,PreviewStage::BeforeFill|PreviewStage::Final=>true,
        }).map(|command|command.index).max().map_or(PreviewStage::Seed,PreviewStage::AfterBatch)
    }
    pub fn accept_inputs(&self,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2],stage:PreviewStage,pass:PreviewPass,proofs:Vec<resident::Input>)->Result<(),GpuRasterError> {
        if target!=self.target {return Err(GpuRasterError::MissingPaintLayer(target));}
        let versions=proofs.iter().map(|input|input.version().clone()).collect::<Vec<_>>();
        let version=Self::captured_version(pass,&versions);let key=AcceptedKey {plane:raster::contents::index(plane),coordinate,stage:self.page_stage(plane,coordinate,stage),pass};
        if key.stage==PreviewStage::Seed && let Some((wanted,_))=self.seed_inputs.get(&key) && wanted.matches(&version)!=Some(true) {return Err(GpuRasterError::Effect("Preview seed proof differs from its declaration".into()));}
        let mut accepted=self.accepted.lock().unwrap();
        if let Some(existing)=accepted.get(&key) {
            if existing.version.matches(&version)!=Some(true) {return Err(GpuRasterError::Effect("Preview inputs changed after acceptance".into()));}
        } else {accepted.insert(key,CapturedInputs {_proofs:proofs,version});}
        Ok(())
    }
    pub fn version(&self,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2],stage:PreviewStage)->Result<ContentVersion,GpuRasterError> {
        self.version_for(target,plane,coordinate,stage,plane_pass(plane))
    }
    pub fn version_for(&self,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2],stage:PreviewStage,pass:PreviewPass)->Result<ContentVersion,GpuRasterError> {
        if target!=self.target {return Err(GpuRasterError::MissingPaintLayer(target));}
        self.region_version_for(plane,page_rect(coordinate).intersect(PixelRect::full(self.source.view().target_extent(target))),stage,pass)
    }
    pub fn ticket(self:&Arc<Self>,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2],stage:PreviewStage,input:raster::contents::Intent)->Result<PreviewVersion,GpuRasterError> {
        Ok(PreviewVersion {declaration:self.clone(),version:self.version(target,plane,coordinate,stage)?,input,plane,coordinate,stage,pass:plane_pass(plane),step:None})
    }
    pub fn ticket_for_accepted(self:&Arc<Self>,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2],stage:PreviewStage,pass:PreviewPass,input:raster::contents::Intent)->Result<PreviewVersion,GpuRasterError> {
        Ok(PreviewVersion {declaration:self.clone(),version:self.version_for(target,plane,coordinate,stage,pass)?,input,plane,coordinate,stage,pass,step:None})
    }
    pub fn ticket_for(self:&Arc<Self>,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2],stage:PreviewStage,pass:PreviewPass,input:raster::contents::Intent,proofs:Vec<resident::Input>)->Result<PreviewVersion,GpuRasterError> {
        self.accept_inputs(target,plane,coordinate,stage,pass,proofs)?;
        Ok(PreviewVersion {declaration:self.clone(),version:self.version_for(target,plane,coordinate,stage,pass)?,input,plane,coordinate,stage,pass,step:None})
    }
    pub fn region_version(&self,plane:RasterPlane,region:PixelRect,stage:PreviewStage)->Result<ContentVersion,GpuRasterError> {self.region_version_for(plane,region,stage,plane_pass(plane))}
    fn region_version_for(&self,plane:RasterPlane,region:PixelRect,stage:PreviewStage,pass:PreviewPass)->Result<ContentVersion,GpuRasterError> {
        let stage=if let PreviewStage::BeforeBatch(index)=stage {self.source_stage(plane,index)}else {stage};
        let key=PreviewRequest {plane:raster::contents::index(plane),bounds:region_bounds(region),stage,pass};
        if let Some(version)=self.memo.lock().unwrap().get(&key) {return Ok(version.clone());}
        #[cfg(test)] crate::test_support::record_preview_region((key.plane,key.bounds,key.stage,key.pass));
        let footprint=self.footprint(region,stage);
        let version=match self.stamped(plane,region,footprint,stage,pass) {Some(version)=>version,None=>self.compute_region(plane,region,footprint,stage,pass)?};
        self.memo.lock().unwrap().insert(key,version.clone());Ok(version)
    }
    fn footprint(&self,region:PixelRect,stage:PreviewStage)->PixelRect {
        let extent=self.source.view().target_extent(self.target);
        self.transport_outputs.iter().filter(|(index,outputs)|included_stage(stage,**index) && outputs.iter().any(|coordinate|!page_rect(*coordinate).intersect(region).is_empty()))
            .fold(region,|footprint,_|footprint.union(region.expand(PAGE_SIZE*WATERCOLOR_TRANSPORT_STEPS,extent)))
    }
    fn stamped(&self,plane:RasterPlane,region:PixelRect,footprint:PixelRect,stage:PreviewStage,pass:PreviewPass)->Option<ContentVersion> {
        let (_,frame)=self.stamps.as_ref().filter(|_|self.fill.is_none())?;
        let last=self.commands.iter().filter(|command|included_stage(stage,command.index) && command.intersects(footprint)).map(|command|command.index).max()?;
        Some(ContentVersion::derive(recipe(&(*frame,self.target,raster::contents::index(plane),self.quality,pass,last,region_bounds(region),region_bounds(footprint))),&[]))
    }
    fn compute_region(&self,plane:RasterPlane,region:PixelRect,footprint:PixelRect,stage:PreviewStage,pass:PreviewPass)->Result<ContentVersion,GpuRasterError> {
        #[cfg(test)] if self.commands.iter().any(|command|included_stage(stage,command.index) && command.intersects(footprint)) {crate::test_support::PassCounts {preview_computations:1,..Default::default()}.record();}
        let mut name=Recipe::new();name.add(&(self.context,self.target,raster::contents::index(plane),self.quality,pass));let mut inputs=Vec::new();
        let extent=self.source.view().target_extent(self.target);
        if (self.quality.requires_base || self.fill.is_some()) && matches!(pass,PreviewPass::Color|PreviewPass::Wetness) {inputs.push(self.versions.region(plane,region));}
        for command in &self.commands {
            if !included_stage(stage,command.index) || !command.intersects(footprint) {continue;}
            inputs.push(command.definition.clone().ok_or(GpuRasterError::NotReady)?);
            if self.quality.requires_base || !pointwise(&command.batch.style) {
                let needed=command.coordinates(footprint).fold(PixelRect::EMPTY,|needed,coordinate|needed.union(pixel_rect(material_sources::sample_rect(&command.batch,&command.dabs,coordinate),extent)));
                let needed=if pointwise(&command.batch.style) {needed}else {needed.expand(PAGE_SIZE,extent)};
                inputs.push(self.versions.region(source_plane(self.target),needed));
                if !self.target.is_coverage() {inputs.push(self.versions.region(RasterPlane::WatercolorWetness,needed));}
            }
        }
        if matches!(stage,PreviewStage::Final) && let Some(fill)=&self.fill {
            inputs.push(fill.definition.clone());
            let coverage=&fill.operation.coverage;let local=DocRect::from(region).translated(coverage.use_.offset.map(|value|-value)).in_frame(coverage.source.domain);
            if let Some(versions)=&fill.coverage {inputs.push(versions.region(RasterPlane::Mask,local));}
            else {return Err(GpuRasterError::NotReady);}
        }
        let mut contact=IdSet::default();
        for command in &self.commands {
            let plan=BrushPassPlan::for_style(&command.batch.style);
            if !self.target.is_coverage() && included_stage(stage,command.index) && (plan.state.coverage || WgpuRasterizer::tracks_stroke_start(&command.batch) || matches!(command.batch.style.execution,BrushExecution::Smudge|BrushExecution::Wet|BrushExecution::Watercolor)) {
                for coordinate in command.coordinates(footprint) {contact.insert(AcceptedKey {plane:raster::contents::index(plane),coordinate,stage:PreviewStage::AfterBatch(command.index),pass});}
            }
        }
        if matches!(pass,PreviewPass::Coverage|PreviewPass::StrokeStart|PreviewPass::Reservoir) {for coordinate in page_coordinates(region) {
            contact.insert(AcceptedKey {plane:raster::contents::index(plane),coordinate,stage:self.page_stage(plane,coordinate,stage),pass});
        }}
        let mut contact=contact.into_iter().collect::<Vec<_>>();contact.sort_by_key(|key|(key.coordinate,match key.stage {PreviewStage::AfterBatch(index)=>Some(index),_=>None}));
        let accepted=self.accepted.lock().unwrap();for key in contact {
            inputs.push(accepted.get(&key).ok_or(GpuRasterError::NotReady)?.version.clone());
        }
        drop(accepted);
        name.add(&region_bounds(region));Ok(ContentVersion::derive(name.0,&inputs))
    }
    fn program_version(&self)->Result<ContentVersion,GpuRasterError> {
        let mut inputs=vec![self.versions.root(source_plane(self.target))];
        if !self.target.is_coverage() {inputs.push(self.versions.root(RasterPlane::WatercolorWetness));}
        for command in &self.commands {inputs.push(command.definition.clone().ok_or(GpuRasterError::NotReady)?);}
        if let Some(fill)=&self.fill {inputs.push(fill.definition.clone());inputs.push(fill.coverage.as_ref().ok_or(GpuRasterError::NotReady)?.root(RasterPlane::Mask));}
        Ok(ContentVersion::derive(recipe(&(self.context,self.target)),&inputs))
    }
    pub fn authored_version(&self)->Result<ContentVersion,GpuRasterError> {
        if let Some((_,frame))=self.stamps.as_ref().filter(|_|self.fill.is_none()) {return Ok(ContentVersion::derive(recipe(&(*frame,self.context,self.target)),&[]));}
        let mut inputs=vec![self.program_version()?];
        for (plane,coordinate,pass) in self.required_seed_inputs() {
            let key=AcceptedKey {plane:raster::contents::index(plane),coordinate,stage:PreviewStage::Seed,pass};
            let (seed,_)=self.seed_inputs.get(&key).ok_or(GpuRasterError::NotReady)?;
            inputs.push(ContentVersion::derive(recipe(&(key.plane,key.coordinate,key.pass)),std::slice::from_ref(seed)));
        }
        Ok(ContentVersion::derive(0x73656564696e707574,&inputs))
    }
}
fn included_stage(stage:PreviewStage,index:usize)->bool {match stage {PreviewStage::Seed=>false,PreviewStage::BeforeBatch(before)=>index<before,PreviewStage::AfterBatch(after)=>index<=after,PreviewStage::BeforeFill|PreviewStage::Final=>true}}
fn plane_pass(plane:RasterPlane)->PreviewPass {if plane==RasterPlane::WatercolorWetness {PreviewPass::Wetness}else {PreviewPass::Color}}
fn region_bounds(region:PixelRect)->[u32;4] {[region.min_x(),region.min_y(),region.max_x(),region.max_y()]}
fn operation_version(operation:&layer_core::RasterOperation,captured:Option<&Input>)->Result<ContentVersion,GpuRasterError> {
    use layer_core::RasterOperationKind as O;
    let mut name=Recipe::new();let mut inputs=Vec::new();name.floats(operation.placement.0);name.tag(&operation.kind);
    let coverage=&operation.coverage;name.add(&(coverage.source.domain,coverage.source.default_coverage.to_bits(),coverage.use_.offset,coverage.use_.enabled,coverage.use_.linked,coverage.use_.inverted));
    if let Some(selection)=&coverage.selection {inputs.push(selection.content_version());}
    let texture=|name:&mut Recipe,inputs:&mut Vec<ContentVersion>,texture:&layer_core::FillTexture|->Result<(),GpuRasterError> {
        name.floats([texture.strength]);name.add(&texture.paper.is_some());if let Some(paper)=&texture.paper {name.add(&(paper.surface.mapping.0.map(f64::to_bits),paper.target_offset));inputs.push(texture_version(&paper.surface.texture)?);}Ok(())
    };
    match &operation.kind {
        O::ColorMode(mode)=>name.add(mode),O::ApplyMask|O::Coverage=>{},O::Erase {alpha_locked}=>name.add(alpha_locked),
        O::Transform(transform)=>{
            let preview=layer_render::TransformPreview {transaction:0,moving:false,target:SourceTarget::Coverage(coverage.target),selection:None,transform:transform.clone()};inputs.push(transform_version(&preview));
        },
        O::Figure(figure)=>{name.tag(&figure.shape);name.tag(&figure.paint);name.floats([figure.start.x,figure.start.y,figure.end.x,figure.end.y,figure.width]);for color in figure.colors {name.floats(color);}name.add(&(figure.alpha_locked,figure.erase));},
        O::Fill {texture:fill_texture,color,alpha_locked,edges,underfill}=>{
            texture(&mut name,&mut inputs,fill_texture)?;name.floats(*color);name.add(&(alpha_locked,edges.is_some(),underfill.is_some()));if let Some(selection)=underfill {inputs.push(selection.content_version());}
            if let Some(edges)=edges {name.add(&(edges.translucent_seed,edges.colour_on_solid));name.floats([edges.position.x,edges.position.y,edges.tolerance]);inputs.push(scene_version(&edges.snapshot,&SceneScope::All)?);for line in &edges.lines {name.add(&line.fill_above);inputs.push(scene_version(&line.snapshot,&line.scope)?);}}
        },
        O::Gradient {texture:fill_texture,start,end,gradient,shape,reverse,opacity,alpha_locked}=>{
            texture(&mut name,&mut inputs,fill_texture)?;name.floats([start.x,start.y,end.x,end.y,*opacity]);name.tag(shape);name.add(&(reverse,alpha_locked));inputs.push(value_version(&layer_core::EffectValue::Gradient(gradient.clone()))?);
        },
        O::Bake {scene,scope,offset}|O::FrequencyDetail {scene,scope,offset,..}=>{
            name.floats([offset.x,offset.y]);inputs.push(match captured {Some(captured)=>captured.version.clone(),None=>scene_version(scene,scope)?});
            if let O::FrequencyDetail {low,..}=&operation.kind {name.add(low);}
        },
    }
    Ok(ContentVersion::derive(name.0,&inputs))
}
fn scene_version(scene:&SceneSnapshot,scope:&SceneScope)->Result<ContentVersion,GpuRasterError> {
    let size=scene.view().composition().size;
    let packet=FramePacket {restore_rasters:&[],time_seconds:scene.context.elapsed,view:layer_render::ViewState {width_px:size[0],height_px:size[1],document_to_surface:[1.,0.,0.,1.,0.,0.]},
        document_extent:size,scene:scene.view().with_scope(scope),inspect_mask:None,selection_overlays:None,dabs:&[],dab_batches:&[],reset_layers:false,commit_rasters:true,composite_all:false,blend_space:scene.view().composition().blend};
    let reads=Reads::of(packet)?;
    Prepared::read(packet,Declared::default(),&reads)?.authored_version(&reads.node)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use layer_core::{Document, authored::*, raster::{CanonicalName,RasterData,RasterRevision,RasterTile,TileKey}};
    use crate::test_support::{packet,dab_batch};

    pub(in crate::scene::scale) fn cached_len(frame:&Prepared)->usize {frame.memo.lock().unwrap().entries.len()}
    pub(in crate::scene::scale) fn cached_cells(frame:&Prepared,node:&graph::Node,level:u32)->IdMap<[i64;4],ContentVersion> {
        frame.memo.lock().unwrap().entries.iter().filter(|(request,_)|Arc::ptr_eq(&request.node.0,node) && request.level==level && !request.global && !request.source_chunk)
            .map(|(request,entry)|(request.region,entry.version.clone())).collect()
    }

    fn document()->(Document,SourceTarget) {
        let mut artwork=Artwork::new([1024,512]).unwrap();
        let (_,target)=crate::test_support::add_paint(&mut artwork,"Paint",[1024,512]);
        (Document::from_artwork(artwork).unwrap(),target)
    }
    fn install(document:&mut Document,target:SourceTarget,data:RasterData) {
        let SourceTarget::Paint(handle)=target else {unreachable!()};
        document.artwork.paint.get_mut(handle).unwrap().raster=RasterRevision::backed(data);
    }
    fn source(target:SourceTarget)->graph::Node {
        Arc::new(graph::Expression::Source {id:target,frame:SourceFrame::at([0;2],[0;2]),extent:[1024,512],outside:0})
    }
    fn prepared(document:&Document)->Prepared {Prepared::from_scene(packet(document.scene(),document.composition().size),Declared::default()).unwrap()}
    fn version(prepared:&Prepared,node:&graph::Node)->ContentVersion {prepared.expected(node,PixelRect::new(0,0,256,256).into(),0,0).unwrap()}
    fn tile()->RasterTile {RasterTile::pending(RasterPlane::Color.descriptor(Default::default()))}

    #[test]
    fn a_frame_reads_no_version_backing_or_effect_of_a_layer_nothing_composes() {
        let reads=|unreferenced:usize| {
            use super::super::tests::{document_at,effect_occurrence,insert_occurrence,paint_occurrence};
            let mut doc=document_at([1024,512]);
            for index in 0..unreferenced {
                let hidden=paint_occurrence(&mut doc,&format!("Hidden {index}"),None);
                doc.artwork.occurrences.get_mut(hidden).unwrap().visible=false;insert_occurrence(&mut doc,hidden,0);
                let blur=effect_occurrence(&mut doc,layer_core::EffectInstance::new(crate::tests::fixture("gaussian_blur").program()),"Hidden blur");
                doc.artwork.occurrences.get_mut(blur).unwrap().visible=false;insert_occurrence(&mut doc,blur,0);
                paint_occurrence(&mut doc,&format!("Unplaced {index}"),None);
            }
            let r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
            let counts=crate::test_support::PassCounts::snapshot();
            Prepared::capture(&r,packet(doc.scene(),[1024,512]),Declared::default()).unwrap();
            crate::test_support::PassCounts::snapshot().delta(counts).signature_reads
        };
        assert_eq!(reads(9),reads(0),"a frame reads the versions, backing and effect definitions of the layers it composes, whatever else the document holds");
    }

    #[test]
    fn regional_names_ignore_unrelated_tiles_and_restore_the_undo_name() {
        let (mut document,target)=document();let node=source(target);
        let key=|x|TileKey {plane:RasterPlane::Color,coordinate:[x,0]};
        let mut data=RasterData::default();data.tiles.insert(key(0),tile());data.tiles.insert(key(3),tile());
        install(&mut document,target,data.clone());let before=prepared(&document);let initial=version(&before,&node);
        data.tiles.insert(key(3),tile());install(&mut document,target,data.clone());
        assert_eq!(initial.matches(&version(&prepared(&document),&node)),Some(true));
        data.tiles.insert(key(0),tile());install(&mut document,target,data);
        assert_eq!(initial.matches(&version(&prepared(&document),&node)),Some(false));
        let restored=Prepared::from_snapshot(before.snapshot());
        assert_eq!(initial.matches(&version(&restored,&node)),Some(true));
        assert_eq!(restored.memo.lock().unwrap().entries.len(),1);
        version(&restored,&node);assert_eq!(restored.memo.lock().unwrap().entries.len(),1);
        restored.expected(&node,PixelRect::new(0,0,256,256).into(),0,1).unwrap();
        assert_eq!(restored.memo.lock().unwrap().entries.len(),2,"the input level is part of the per-frame memo key");
    }

    #[test]
    fn pending_packing_comparisons_do_not_rename_or_request_the_page() {
        let (mut document,target)=document();let node=source(target);
        let key=TileKey {plane:RasterPlane::Color,coordinate:[0;2]};let first=tile();
        let mut data=RasterData::default();data.tiles.insert(key,first.clone());install(&mut document,target,data.clone());
        let before=version(&prepared(&document),&node);
        let (packing,comparison)=RasterTile::pending_from_live(first.descriptor(),first.content_version());
        data.tiles.insert(key,packing);install(&mut document,target,data);
        let pending=version(&prepared(&document),&node);let identity=pending.identity();
        assert_eq!(pending.matches(&before),None);
        assert_eq!(pending.authored(),before.authored());
        comparison.resolve(false).unwrap();
        assert_eq!(pending.matches(&before),Some(true));assert_eq!(pending.identity(),identity);
        let other=CanonicalName::new(first.content_version());
        assert_eq!(other.version().matches(&ContentVersion::derive(99,&[])),None);
    }

    fn path()->layer_core::authored::vector::PathItem {
        use layer_core::authored::vector::*;
        let geometry=PathGeometry::new(vec![Subpath::new(&[[0.,0.],[256.,128.]].map(Anchor::corner),false)]).unwrap();
        PathItem::new(ItemId(1),Arc::new(geometry),Arc::new(Style::default()))
    }

    #[test]
    fn selected_vector_inputs_keep_their_name_without_an_active_motion() {
        use layer_core::authored::vector::*;
        let mut editor=layer_core::Editor::new(document().0);
        let object=VectorObject::from_records(vec![Record::Path(path())]);
        let (owner,edit)=editor.document().create_vector_layer_edit("Vector",object,None,0).unwrap();
        editor.perform(edit).unwrap();
        let document=editor.document();let items:Arc<std::collections::BTreeSet<ItemId>>=Arc::new([ItemId(1)].into());
        let node=Arc::new(graph::Expression::Vector {owner,offset:[0;2],moving:Some(crate::scene::vectors::Moving(items.clone()))});
        let selected=version(&prepared(document),&node);
        let whole=Arc::new(graph::Expression::Vector {owner,offset:[0;2],moving:None});
        assert_eq!(selected.matches(&version(&prepared(document),&whole)),Some(false));
        for offset in [5.,-12.] {
            let motion=layer_render::VectorMotion {owner,items:items.clone(),transform:Affine64([1.,0.,0.,1.,offset,0.])};
            let moving=Prepared::from_scene(packet(document.scene(),document.composition().size),Declared {vector_motion:Some(motion),..Default::default()}).unwrap();
            assert_eq!(selected.matches(&version(&moving,&node)),Some(true));
        }
    }

    #[test]
    fn a_posed_or_split_vector_layer_names_the_authored_content_it_shows() {
        use layer_core::authored::vector::*;
        let mut editor=layer_core::Editor::new(document().0);
        let [bottom,middle,top]=["Bottom","Middle","Top"].map(|name| {
            let (owner,edit)=editor.document().create_vector_layer_edit(name,VectorObject::from_records(vec![Record::Path(path())]),None,0).unwrap();
            editor.perform(edit).unwrap();owner
        });
        let items:Arc<std::collections::BTreeSet<ItemId>>=Arc::new([ItemId(1)].into());
        let over=|front:graph::Node,back:graph::Node|->graph::Node {Arc::new(graph::Expression::Combine {front,back,blend:0,flags:0})};
        let layer=|owner,moving:Option<crate::scene::vectors::Moving>|over(Arc::new(graph::Expression::VectorPreview {owner}),
            over(Arc::new(graph::Expression::VectorMotion {owner}),Arc::new(graph::Expression::Vector {owner,offset:[0;2],moving})));
        let clear=||Arc::new(graph::Expression::Color([0;4]));
        let whole=over(layer(top,None),over(layer(middle,None),layer(bottom,None)));
        let split=over(over(over(layer(top,None),layer(middle,None)),clear()),over(layer(bottom,Some(crate::scene::vectors::Moving(items.clone()))),clear()));
        let document=editor.document();let still=prepared(document).authored_revision(&whole).unwrap();
        let posed=Prepared::from_scene(packet(document.scene(),document.composition().size),Declared {
            vector_motion:Some(layer_render::VectorMotion {owner:bottom,items,transform:Affine64([1.,0.,0.,1.,12.,-5.])}),
            vector_preview:Some(layer_render::VectorPreview {owner:bottom,path:Arc::new(path()),revision:1}),..Default::default()}).unwrap();
        assert_eq!(posed.authored_revision(&whole).unwrap(),still,"a pose and a path preview change no authored content");
        assert_eq!(posed.authored_revision(&split).unwrap(),still,"splitting the stack around the moving paths regroups the same content");
        let mut hidden=document.scene().occurrence(middle).unwrap().clone();hidden.visible=false;
        editor.perform(layer_core::Edit::Occurrence(layer_core::RecordChange::replace(&editor.document().artwork.occurrences,middle,Some(hidden)).unwrap())).unwrap();
        assert_ne!(prepared(editor.document()).authored_revision(&split).unwrap(),still,"hiding a layer is an authored change");
    }

    fn left_and_right_chunks()->(layer_core::Editor,OccurrenceHandle) {
        use layer_core::authored::vector::*;
        let line=|id:u64,x:f64|Record::Path(PathItem::new(ItemId(id),Arc::new(PathGeometry::new(vec![Subpath::new(&[[x,10.],[x+4.,40.]].map(Anchor::corner),false)]).unwrap()),Arc::new(Style::default())));
        let records=(1..=64).map(|id|line(id,20.)).chain((65..=128).map(|id|line(id,900.))).collect();
        let mut editor=layer_core::Editor::new(document().0);
        let (owner,edit)=editor.document().create_vector_layer_edit("Vector",VectorObject::from_records(records),None,0).unwrap();editor.perform(edit).unwrap();
        (editor,owner)
    }
    fn vector_node(owner:OccurrenceHandle,moving:Option<u64>)->graph::Node {
        use crate::scene::vectors::Moving;
        Arc::new(graph::Expression::Vector {owner,offset:[0;2],moving:moving.map(|id|Moving(Arc::new([layer_core::authored::vector::ItemId(id)].into())))})
    }
    fn page_name(document:&Document,node:&graph::Node,region:PixelRect)->ContentVersion {prepared(document).expected(node,region.into(),0,0).unwrap()}
    fn sides()->[PixelRect;2] {[PixelRect::new(0,0,256,256),PixelRect::new(768,0,1024,256)]}

    #[test]
    fn a_vector_page_names_only_the_chunks_and_moving_paths_that_reach_it() {
        use layer_core::authored::vector::*;
        let (mut editor,owner)=left_and_right_chunks();let [left,right]=sides();
        let whole=vector_node(owner,None);
        let before=[left,right].map(|region|page_name(editor.document(),&whole,region));
        let object=editor.document().scene().vector_layer(owner).unwrap().clone();let position=object.find(ItemId(100)).unwrap();
        let Some(Record::Path(path))=object.record(position).cloned() else {unreachable!()};
        editor.perform(editor.document().vector_object_edit(owner,object.replace([(position,Some(Record::Path(PathItem {hidden:true,..path})))])).unwrap()).unwrap();
        let document=editor.document();
        assert_eq!(before[0].matches(&page_name(document,&whole,left)),Some(true),"hiding a path on the right keeps the left page's name");
        assert_eq!(before[1].matches(&page_name(document,&whole,right)),Some(false),"hiding a path on the right renames the right page");
        let moving=vector_node(owner,Some(101));
        assert_eq!(page_name(document,&moving,left).matches(&page_name(document,&whole,left)),Some(true),"moving paths that do not reach a page keep its name");
        assert_eq!(page_name(document,&moving,right).matches(&page_name(document,&whole,right)),Some(false),"moving paths that reach a page rename it");
    }

    #[test]
    fn a_normal_over_chain_names_its_operands_whatever_their_grouping() {
        let (editor,owner)=left_and_right_chunks();let [left,right]=sides();
        let document=editor.document();
        let over=|front:graph::Node,back:graph::Node|->graph::Node {Arc::new(graph::Expression::Combine {front,back,blend:0,flags:0})};
        let clear=||Arc::new(graph::Expression::Color([0;4]));
        let tint=||Arc::new(graph::Expression::Color([1,2,3,4]));
        let (whole,moving)=(vector_node(owner,None),vector_node(owner,Some(101)));
        let flat=over(tint(),whole.clone());
        let split=over(over(tint(),clear()),over(moving.clone(),clear()));
        assert_eq!(page_name(document,&split,left).matches(&page_name(document,&flat,left)),Some(true),"a split chain names the same content where no moving path reaches");
        assert_eq!(page_name(document,&split,right).matches(&page_name(document,&flat,right)),Some(false),"a split chain renames the pages moving paths leave");
        assert_eq!(page_name(document,&over(moving,clear()),left).matches(&page_name(document,&whole,left)),Some(true),"a half over a clear colour names its content");
    }

    #[test]
    fn vector_object_parameters_and_visibility_are_authored_inputs() {
        use layer_core::authored::vector::*;
        let (mut document,_)=document();let extent=document.composition().size;let path=path();
        let root=VectorObject::from_records(vec![Record::Path(path.clone())]);let vector=document.artwork.vectors.insert(PortableId::random(),root.clone()).unwrap();
        let owner=document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Vector(vector),"Vector")).unwrap();
        document.artwork.stacks.get_mut(document.composition().result).unwrap().entries.push(owner);
        document=Document::from_artwork(document.artwork.clone()).unwrap();
        let node=Arc::new(graph::Expression::Vector {owner,offset:[0;2],moving:None});let before=version(&prepared(&document),&node);
        let mut hidden=path.clone();hidden.hidden=true;
        *document.artwork.vectors.get_mut(vector).unwrap()=VectorObject::from_records(vec![Record::Path(hidden)]);
        assert_eq!(before.matches(&version(&prepared(&document),&node)),Some(false));
        *document.artwork.vectors.get_mut(vector).unwrap()=root;
        assert_eq!(before.matches(&version(&prepared(&document),&node)),Some(true));
        document.artwork.occurrences.get_mut(owner).unwrap().visible=false;
        assert_eq!(before.matches(&version(&prepared(&document),&node)),Some(false));
        let image=layer_core::authored::Image::new(layer_core::color::source::rgba8_source([1;2],|_,_|[255;4]));
        let object=document.artwork.objects.insert(PortableId::random(),ImageObject::new(image)).unwrap();
        let owner=document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(object),"Object")).unwrap();
        document.artwork.stacks.get_mut(document.composition().result).unwrap().entries.push(owner);
        document=Document::from_artwork(document.artwork.clone()).unwrap();let mut index=object_spatial::SpatialIndex::default();
        let content=index.content(document.scene(),owner).unwrap();let node=Arc::new(graph::Expression::Objects {content,preview:true});
        let before=prepared(&document).expected(&node,PixelRect::full(extent).into(),0,0).unwrap();
        document.artwork.objects.get_mut(object).unwrap().affine.0[4]=5.;
        let content=index.content(document.scene(),owner).unwrap();let node=Arc::new(graph::Expression::Objects {content,preview:true});
        let after=prepared(&document).expected(&node,PixelRect::full(extent).into(),0,0).unwrap();
        assert_eq!(before.matches(&after),Some(false));
    }

    #[test]
    fn effect_parameters_and_actual_shader_definitions_change_the_name() {
        let mut instance=layer_core::EffectInstance::new(layer_core::bundled_effect_catalog().get("exposure").unwrap().program());
        let before=effect_version(layer_core::EffectView::new(&instance.program,&instance.values)).unwrap();
        let index=instance.values.iter().position(|value|matches!(value,layer_core::EffectValue::Number(_))).unwrap();
        instance.values[index]=layer_core::EffectValue::Number(1.);
        let edited=effect_version(layer_core::EffectView::new(&instance.program,&instance.values)).unwrap();assert_eq!(before.matches(&edited),Some(false));
        let mut program=(*instance.program).clone();program.wgsl=layer_core::EffectShader::Code("fn renamed_shader() {}".into());
        assert_eq!(edited.matches(&effect_version(layer_core::EffectView::new(&program,&instance.values)).unwrap()),Some(false));
    }

    #[test]
    fn local_effect_footprints_follow_the_evaluator_halo_while_global_effects_use_the_root() {
        let (mut document,target)=document();let input=source(target);
        let mut data=RasterData::default();let key=|x|TileKey {plane:RasterPlane::Color,coordinate:[x,0]};
        data.tiles.insert(key(0),tile());data.tiles.insert(key(3),tile());install(&mut document,target,data.clone());
        let mut instance=layer_core::EffectInstance::new(layer_core::bundled_effect_catalog().get("gaussian_blur").unwrap().program());
        for (value,parameter) in instance.values.iter_mut().zip(instance.program.parameters.iter()) {if parameter.key.as_ref()=="sigma" {*value=layer_core::EffectValue::Number(1.);}}
        let application=document.artwork.effects.insert(PortableId::random(),EffectApplication::new(instance.program,instance.values,document.composition().size)).unwrap();
        let owner=document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Effect(application),"Blur")).unwrap();
        document.artwork.stacks.get_mut(document.composition().result).unwrap().entries.push(owner);
        document=Document::from_artwork(document.artwork.clone()).unwrap();
        let node=Arc::new(graph::Expression::Effect {input,chain:vec![(owner,0,0)],masks:vec![],radius:Some(3)});
        let before=version(&prepared(&document),&node);
        data.tiles.insert(key(3),tile());install(&mut document,target,data.clone());
        assert_eq!(before.matches(&version(&prepared(&document),&node)),Some(true));
        data.tiles.insert(key(1),tile());install(&mut document,target,data.clone());
        assert_eq!(before.matches(&version(&prepared(&document),&node)),Some(false),"the evaluator reads the neighboring tile through its blur halo");
        let program=Arc::make_mut(&mut document.artwork.effects.get_mut(application).unwrap().program);
        Arc::make_mut(&mut program.passes)[0].sampling=layer_core::EffectSampling::Document;
        let global=version(&prepared(&document),&node);
        data.tiles.insert(key(3),tile());install(&mut document,target,data);
        assert_eq!(global.matches(&version(&prepared(&document),&node)),Some(false));
    }

    #[test]
    fn authored_witness_ignores_camera_quality_and_source_realization_but_tracks_edit_and_undo() {
        let (mut document,target)=document();let mut data=RasterData::default();let key=TileKey {plane:RasterPlane::Color,coordinate:[0;2]};data.tiles.insert(key,tile());install(&mut document,target,data.clone());
        let before=prepared(&document);let root=source(target);let witness=before.authored_revision(&root).unwrap();
        let coarse=Arc::new(graph::Expression::Source {id:target,frame:SourceFrame {phase:[31,7],shift:[-31,-7]},extent:[1024,512],outside:0});
        before.expected(&root,PixelRect::new(0,0,256,256).into(),0,0).unwrap();
        before.expected(&coarse,PixelRect::new(0,0,512,512).into(),3,2).unwrap();
        assert_eq!(witness,before.authored_revision(&coarse).unwrap());
        let frame=packet(document.scene(),document.composition().size);let mut moved=frame;moved.view.document_to_surface=[8.,0.,0.,8.,-300.,-700.];
        assert_eq!(witness,Prepared::from_scene(moved,Declared::default()).unwrap().authored_revision(&coarse).unwrap());
        data.tiles.insert(key,tile());install(&mut document,target,data);
        assert_ne!(witness,prepared(&document).authored_revision(&root).unwrap());
        assert_eq!(witness,Prepared::from_snapshot(before.snapshot()).authored_revision(&root).unwrap());
    }

    #[test]
    fn source_chunk_signatures_remove_the_actual_xy_phase_at_tile_boundaries() {
        for phase in [[17,0],[0,29],[17,29]] {
            let (mut document,target)=document();let mut data=RasterData::default();
            let at=if phase[0]!=0 {[0,1]}else {[1,0]};let key=TileKey {plane:RasterPlane::Color,coordinate:at};data.tiles.insert(key,tile());install(&mut document,target,data.clone());
            let node=Arc::new(graph::Expression::Source {id:target,frame:SourceFrame::at([0;2],phase),extent:[1024,512],outside:0});
            let region=PixelRect::new(256,256,512,512).into();let before=prepared(&document);
            let chunk=before.expected_source(&node,region,1,0).unwrap();let display_node=source(target);let display=before.expected(&display_node,region,1,0).unwrap();
            data.tiles.insert(key,tile());install(&mut document,target,data);let after=prepared(&document);
            assert_eq!(chunk.matches(&after.expected_source(&node,region,1,0).unwrap()),Some(false),"phase={phase:?}");
            assert_eq!(display.matches(&after.expected(&display_node,region,1,0).unwrap()),Some(true),"display and source chunks use distinct coordinate frames: {phase:?}");
        }
    }

    #[test]
    fn captured_transforms_read_regional_versions_from_the_held_source() {
        let (mut document,target)=document();let key=|x|TileKey {plane:RasterPlane::Color,coordinate:[x,0]};let mut data=RasterData::default();data.tiles.insert(key(0),tile());data.tiles.insert(key(3),tile());install(&mut document,target,data.clone());
        let preview=layer_render::TransformPreview {transaction:1,moving:false,target,selection:None,transform:layer_core::ImageTransform::affine(layer_core::Affine::translation(layer_core::Point {x:256.,y:0.}))};
        let capture=|document:&Document|Capture::new(document.scene().snapshot(),prepared(document).versions(target).unwrap().clone(),preview.clone(),None);
        let node=source(target);let region=PixelRect::new(256,0,512,256).into();let prepare=|document:&Document| {let mut declared=Declared::default();declared.captures.insert(target,capture(document));Prepared::from_scene(packet(document.scene(),document.composition().size),declared).unwrap()};
        let initial=prepare(&document);let before=initial.expected(&node,region,0,0).unwrap();
        data.tiles.insert(key(3),tile());install(&mut document,target,data.clone());
        assert_eq!(before.matches(&prepare(&document).expected(&node,region,0,0).unwrap()),Some(true));
        data.tiles.insert(key(0),tile());install(&mut document,target,data);
        assert_eq!(before.matches(&prepare(&document).expected(&node,region,0,0).unwrap()),Some(false));
        assert_eq!(before.matches(&Prepared::from_snapshot(initial.snapshot()).expected(&node,region,0,0).unwrap()),Some(true));
    }

    #[test]
    fn pruned_frames_keep_the_readers_of_kept_roots_so_a_later_tile_write_replans_its_cell() {
        let (mut document,target)=document();let key=|x,y|TileKey {plane:RasterPlane::Color,coordinate:[x,y]};
        let mut data=RasterData::default();for x in 0..4 {for y in 0..2 {data.tiles.insert(key(x,y),tile());}}
        install(&mut document,target,data.clone());
        let root=(0..40).fold(source(target),|node,step|Arc::new(graph::Expression::Opacity {input:node,opacity:(1.-step as f32/64.).to_bits()}));
        let cells=(0..4).flat_map(|x|(0..2).map(move|y|PixelRect::new(x*256,y*256,(x+1)*256,(y+1)*256))).collect::<Vec<_>>();
        let roots=|frame:&Prepared|cells.iter().map(|cell|frame.expected(&root,(*cell).into(),0,0).unwrap().identity()).collect::<Vec<_>>();
        let other=Arc::new(graph::Expression::Opacity {input:source(target),opacity:0.5f32.to_bits()});
        let mut frame=prepared(&document);roots(&frame);
        let mut unrelated=0;
        loop {
            let held=cached_len(&frame);let next=prepared(&document);next.carry(&frame);
            let pruned=cached_len(&next)<held;roots(&next);frame=next;
            if pruned {break;}
            for _ in 0..8 {unrelated+=1;frame.expected(&other,PixelRect::new(unrelated,0,unrelated+1,1).into(),0,0).unwrap();}
            assert!(unrelated<16384,"a frame that reads only the roots eventually prunes the unrelated keys");
        }
        data.tiles.insert(key(1,0),tile());install(&mut document,target,data);
        let written=prepared(&document);written.carry(&frame);
        assert_eq!(roots(&written),roots(&prepared(&document)),"after pruned frames, a tile write re-plans the cell that reads it");
    }

    #[test]
    fn a_prune_reports_one_bounding_region_per_level_whatever_it_drops() {
        let (document,target)=document();
        let other=Arc::new(graph::Expression::Opacity {input:source(target),opacity:0.5f32.to_bits()});
        let read=|frame:&Prepared,keys:std::ops::Range<u32>|for x in keys {frame.expected(&other,PixelRect::new(x,0,x+1,1).into(),0,0).unwrap();};
        let first=prepared(&document);read(&first,0..5000);
        let second=prepared(&document);second.carry(&first).unwrap();read(&second,5000..15300);
        let held=cached_len(&second);let third=prepared(&document);let regions=third.carry(&second).unwrap();
        assert!(cached_len(&third)<held,"the keys the last frame did not read are pruned");
        let levels=regions.iter().map(|(level,_)|*level).collect::<std::collections::BTreeSet<_>>();
        assert_eq!(regions.len(),levels.len(),"a carry reports one bounding region per level, not one per pruned key");
    }

    pub(in crate::scene::scale) fn assert_authored_versions_are_exact(frame:&Prepared) {
        let fresh=Prepared::from_snapshot(frame.snapshot.clone());
        let held=frame.authored.lock().unwrap().keys().cloned().collect::<Vec<_>>();
        for node in held {
            let carried=frame.authored_version(&node.0).map(|version|version.identity());
            assert_eq!(carried.ok(),fresh.authored_version(&node.0).map(|version|version.identity()).ok(),"a held authored version equals a fresh evaluation");
        }
    }

    #[test]
    fn a_carry_keeps_the_authored_versions_of_nodes_that_read_no_changed_target() {
        let mut artwork=Artwork::new([1024,512]).unwrap();
        let (_,a)=crate::test_support::add_paint(&mut artwork,"A",[1024,512]);let (_,b)=crate::test_support::add_paint(&mut artwork,"B",[1024,512]);
        let mut document=Document::from_artwork(artwork).unwrap();
        let tiles=|x|{let mut data=RasterData::default();data.tiles.insert(TileKey {plane:RasterPlane::Color,coordinate:[x,0]},tile());data};
        install(&mut document,a,tiles(0));install(&mut document,b,tiles(1));
        let [source_a,source_b]=[a,b].map(source);
        let faded=Arc::new(graph::Expression::Opacity {input:source_b.clone(),opacity:0.5f32.to_bits()});
        let root=Arc::new(graph::Expression::Combine {front:source_a.clone(),back:faded.clone(),blend:0,flags:0});
        let held=|frame:&Prepared,node:&graph::Node|frame.authored.lock().unwrap().contains_key(&Expression(node.clone()));
        let first=prepared(&document);let before=first.authored_revision(&root).unwrap();
        install(&mut document,a,tiles(2));
        let second=prepared(&document);second.carry(&first).unwrap();
        assert!(held(&second,&source_b) && held(&second,&faded),"nodes that read only unchanged targets keep their authored versions");
        assert!(!held(&second,&source_a) && !held(&second,&root),"nodes that read the changed target are evaluated again");
        assert_ne!(before,second.authored_revision(&root).unwrap(),"the root follows the changed target");
        assert_authored_versions_are_exact(&second);
        install(&mut document,b,tiles(3));
        let third=prepared(&document);third.carry(&second).unwrap();
        assert!(held(&third,&source_a) && !held(&third,&source_b) && !held(&third,&faded) && !held(&third,&root));
        third.authored_revision(&root).unwrap();assert_authored_versions_are_exact(&third);
    }

    #[test]
    fn repeated_root_lookups_use_held_expression_identity_without_revisiting_dependencies() {
        let (document,target)=document();let mut prepared=prepared(&document);let source=source(target);
        let mut root=source.clone();for _ in 0..28 {root=Arc::new(graph::Expression::Combine {front:root,back:source.clone(),blend:0,flags:0});}
        let before=version(&prepared,&root);let authored=prepared.authored_revision(&root).unwrap();let memo=prepared.memo.lock().unwrap().entries.len();let definitions=prepared.authored.lock().unwrap().len();
        Arc::get_mut(&mut prepared.snapshot).unwrap().rasters.clear();
        assert_eq!(before.matches(&version(&prepared,&root)),Some(true));assert_eq!(authored,prepared.authored_revision(&root).unwrap());
        assert_eq!(memo,prepared.memo.lock().unwrap().entries.len());assert_eq!(definitions,prepared.authored.lock().unwrap().len());
        assert!(matches!(prepared.expected(&root,PixelRect::new(256,0,512,256).into(),0,0),Err(GpuRasterError::NotReady)));
    }

    #[test]
    fn unresolved_shader_modules_are_an_explicit_definition_error() {
        let instance=layer_core::EffectInstance::new(layer_core::bundled_effect_catalog().get("exposure").unwrap().program());let mut program=(*instance.program).clone();program.wgsl=layer_core::EffectShader::Modules(vec![Arc::from("missing_module")].into());
        assert!(matches!(effect_version(layer_core::EffectView::new(&program,&instance.values)),Err(GpuRasterError::Effect(_))));
    }

    fn preview_dab(x:f32)->Dab {Dab {center:layer_core::Point {x,y:32.},radii:[8.;2],rotation:[1.,0.],motion:[0.;2],color_rgba_linear:[1.;4],flow:1.,hardness:1.,texture_sign:[1.;2],material:[0.;4],previous:[0.;4],contact:[0.;4],previous_contact:[0.;4]}}
    fn declaration(document:&Document,batches:&[DabBatch],dabs:&[Dab],fill:Option<layer_render::FillPreview>)->PreviewDeclaration {contact_declaration(document,batches,dabs,fill,None)}
    fn contact_declaration(document:&Document,batches:&[DabBatch],dabs:&[Dab],fill:Option<layer_render::FillPreview>,stamps:Option<Arc<PreviewStamps>>)->PreviewDeclaration {
        PreviewDeclaration::from_scene(FramePacket {dab_batches:batches,dabs,..packet(document.scene(),document.composition().size)},IdMap::default(),IdMap::default(),PreviewQuality::default(),fill,stamps).unwrap().unwrap()
    }

    #[test]
    fn producer_indices_distinguish_duplicate_batches_and_ignore_unrelated_damage_hints() {
        let (document,target)=document();let style=layer_render::DabStyle::for_brush(&layer_core::BrushSnapshot::default(),layer_core::StrokeTool::Brush);
        let mut first=dab_batch(target,style,layer_core::Rect::from_extent([64;2]));first.kind=DabBatchKind::Preview;
        let second=DabBatch {first_dab:1,damage:PixelRect::new(680,0,730,64).to_rect(),..first.clone()};let dabs=[preview_dab(32.),preview_dab(700.)];
        let program=declaration(&document,&[first.clone(),second],&dabs,None);
        let after=program.version(target,RasterPlane::Color,[0;2],PreviewStage::AfterBatch(0)).unwrap();
        assert_eq!(after.matches(&program.version(target,RasterPlane::Color,[0;2],PreviewStage::Final).unwrap()),Some(true));
        assert_eq!(after.matches(&program.version(target,RasterPlane::Color,[0;2],PreviewStage::BeforeBatch(0)).unwrap()),Some(false));
        let mut changed_hint=first.clone();changed_hint.damage=layer_core::Rect::from_extent([999;2]);
        assert_eq!(after.matches(&declaration(&document,&[changed_hint],&dabs[..1],None).version(target,RasterPlane::Color,[0;2],PreviewStage::Final).unwrap()),Some(true));
        let duplicate=declaration(&document,&[first.clone(),first.clone()],&dabs[..1],None);
        assert_eq!(duplicate.version(target,RasterPlane::Color,[0;2],PreviewStage::AfterBatch(0)).unwrap().matches(&duplicate.version(target,RasterPlane::Color,[0;2],PreviewStage::AfterBatch(1)).unwrap()),Some(false));
    }


    #[test]
    fn authored_preview_names_use_declared_history_and_never_gather_progress() {
        let (document,target)=document();let mut style=layer_render::DabStyle::for_brush(&layer_core::BrushSnapshot::default(),layer_core::StrokeTool::Brush);
        style.rendering.accumulation=layer_core::BrushAccumulation::Uniform;
        let mut batch=dab_batch(target,style,layer_core::Rect::from_extent([64;2]));batch.kind=DabBatchKind::Preview;
        let program=declaration(&document,&[batch],&[preview_dab(32.)],None);
        assert!(matches!(program.authored_version(),Err(GpuRasterError::NotReady)));
        let histories=program.required_seed_inputs();assert!(!histories.is_empty());
        let initial:ContentVersion=layer_core::raster::ContentName::fresh().into();
        let seeds=histories.iter().map(|(plane,coordinate,pass)|PreviewSeed {plane:*plane,coordinate:*coordinate,pass:*pass,versions:vec![initial.clone()],committed:false}).collect();
        let held=program.clone().with_seed_inputs(seeds).unwrap();let authored=held.authored_version().unwrap();
        held.accepted.lock().unwrap().insert(AcceptedKey {plane:0,coordinate:[0;2],stage:PreviewStage::AfterBatch(0),pass:PreviewPass::Color},CapturedInputs {_proofs:Vec::new(),version:ContentVersion::derive(10,&[])});
        assert_eq!(authored.matches(&held.authored_version().unwrap()),Some(true));
        let mut different_quality=held.clone();different_quality.quality=PreviewQuality {level:3,contribution:true,requires_base:true};
        assert_eq!(authored.matches(&different_quality.authored_version().unwrap()),Some(true));
        let changed=program.with_seed_inputs(histories.iter().map(|(plane,coordinate,pass)|PreviewSeed {plane:*plane,coordinate:*coordinate,pass:*pass,versions:vec![ContentVersion::derive(11,&[])],committed:false}).collect()).unwrap();
        assert_eq!(authored.matches(&changed.authored_version().unwrap()),Some(false));
        assert!(matches!(held.seed_stage(RasterPlane::Color,[0;2],PreviewPass::Coverage),Some(PreviewStage::BeforeBatch(0))));
    }

    #[test]
    fn output_participation_uses_the_existing_tile_plan_without_visual_halo_pages() {
        let (document,target)=document();let mut style=layer_render::DabStyle::for_brush(&layer_core::BrushSnapshot::default(),layer_core::StrokeTool::Brush);style.execution=BrushExecution::Watercolor;
        let mut batch=dab_batch(target,style,PixelRect::new(240,16,255,48).to_rect());batch.kind=DabBatchKind::Preview;
        let program=declaration(&document,&[batch.clone()],&[preview_dab(250.)],None);
        let planned=brush_tiles::plan(&batch,&[preview_dab(250.)],document.scene().target_extent(target));
        assert_eq!(program.commands[0].outputs,planned.into_iter().map(|tile|tile.coordinate).collect());
        assert!(program.affects(page_rect([0;2])));assert!(!program.affects(page_rect([1,0])));
        assert!(matches!(program.page_stage(RasterPlane::Color,[1,0],PreviewStage::Final),PreviewStage::Seed));
        assert!(!program.transport_output([1,0],PreviewStage::AfterBatch(0)));
    }

    #[test]
    fn transport_intermediates_only_rename_actual_group_outputs_including_earlier_batches() {
        let (document,target)=document();let mut style=layer_render::DabStyle::for_brush(&layer_core::BrushSnapshot::default(),layer_core::StrokeTool::Brush);style.execution=BrushExecution::Watercolor;
        style.transport=Some(layer_core::BrushTransport {conductance:layer_core::AssetId("test".into()),scale:1.,rotation_radians:0.,contrast:0.,wet_flow:1.,dry_flow:0.,distance:1.,water_load:1.});
        let mut first=dab_batch(target,style,PixelRect::new(0,0,64,64).to_rect());first.kind=DabBatchKind::Preview;
        let second=DabBatch {first_dab:1,damage:PixelRect::new(256,0,320,64).to_rect(),..first.clone()};
        let program=Arc::new(declaration(&document,&[first,second],&[preview_dab(32.),preview_dab(288.)],None));
        let contents=raster::contents::Contents::new(Arc::new(RasterData::default()));let version=ContentVersion::derive(12,&[]);
        for coordinate in [[0,0],[1,0],[2,0]] {
            let ticket=PreviewVersion {declaration:program.clone(),input:contents.intent(TileKey {plane:RasterPlane::Color,coordinate}),version:version.clone(),plane:RasterPlane::Color,coordinate,stage:PreviewStage::AfterBatch(1),pass:PreviewPass::Color,step:None};
            assert_eq!(ticket.intermediate(PreviewPass::Transport,0).version().matches(ticket.version()),Some(coordinate==[2,0]));
        }
        assert!(!program.transport_output([0,0],PreviewStage::AfterBatch(0)));
    }

    #[test]
    fn fill_uses_actual_operation_definition_and_coverage() {
        let (document,target)=document();let operation=layer_core::RasterOperation {placement:Default::default(),coverage:layer_core::CoverageSnapshot::reveal_all(layer_core::CoverageHandle::from_index(0),document.composition().size,[0;2]),
            kind:layer_core::RasterOperationKind::Fill {texture:Default::default(),color:[1.,0.,0.,1.],alpha_locked:false,edges:None,underfill:None}};
        let fill=layer_render::FillPreview {transaction:3,revision:7,target,operation};let first=declaration(&document,&[],&[],Some(fill.clone()));
        let pixels=first.version(target,RasterPlane::Color,[0;2],PreviewStage::Final).unwrap();
        let mut ids=fill.clone();ids.transaction+=1;ids.revision+=1;
        assert_eq!(pixels.matches(&declaration(&document,&[],&[],Some(ids)).version(target,RasterPlane::Color,[0;2],PreviewStage::Final).unwrap()),Some(true));
        let mut coverage=fill.clone();coverage.operation.coverage.source.default_coverage=0.5;
        assert_eq!(pixels.matches(&declaration(&document,&[],&[],Some(coverage)).version(target,RasterPlane::Color,[0;2],PreviewStage::Final).unwrap()),Some(false));
        let mut colour=fill;if let layer_core::RasterOperationKind::Fill {color,..}=&mut colour.operation.kind {*color=[0.,1.,0.,1.];}
        assert_eq!(pixels.matches(&declaration(&document,&[],&[],Some(colour)).version(target,RasterPlane::Color,[0;2],PreviewStage::Final).unwrap()),Some(false));
    }

    #[test]
    fn missing_required_preview_capture_is_pending_and_never_a_weakened_authored_name() {
        let (document,target)=document();let mut style=layer_render::DabStyle::for_brush(&layer_core::BrushSnapshot::default(),layer_core::StrokeTool::Brush);style.execution=BrushExecution::Clone;style.retouch=Some(Default::default());
        let mut batch=dab_batch(target,style,layer_core::Rect::from_extent([64;2]));batch.kind=DabBatchKind::Preview;
        let program=declaration(&document,&[batch],&[preview_dab(32.)],None);
        assert!(matches!(program.authored_version(),Err(GpuRasterError::NotReady)));
        assert!(matches!(program.version(target,RasterPlane::Color,[0;2],PreviewStage::Final),Err(GpuRasterError::NotReady)));
        assert!(program.memo.lock().unwrap().is_empty());
        assert!(program.version(target,RasterPlane::Color,[0;2],PreviewStage::Seed).is_ok());
    }

    #[test]
    fn wetness_source_stage_holds_the_active_input_until_the_material_update_group_finishes() {
        let (document,target)=document();let mut style=layer_render::DabStyle::for_brush(&layer_core::BrushSnapshot::default(),layer_core::StrokeTool::Brush);style.execution=BrushExecution::Watercolor;
        let persistent=dab_batch(target,style.clone(),layer_core::Rect::EMPTY);
        let mut first=DabBatch {first_dab:1,..persistent.clone()};first.kind=DabBatchKind::Preview;
        let second=DabBatch {first_dab:2,..first.clone()};let third=DabBatch {first_dab:3,material_update:1,..first.clone()};
        let program=declaration(&document,&[persistent,first,second,third],&[preview_dab(32.);4],None);
        assert!(matches!(program.source_stage(RasterPlane::Color,2),PreviewStage::BeforeBatch(2)));
        assert!(matches!(program.source_stage(RasterPlane::WatercolorWetness,2),PreviewStage::BeforeBatch(1)));
        assert!(matches!(program.source_stage(RasterPlane::WatercolorWetness,3),PreviewStage::BeforeBatch(3)));
    }

    #[test]
    fn identical_preview_keeps_its_name_across_packet_offsets_and_camera_changes() {
        let (document,target)=document();let brush=layer_core::BrushSnapshot::default();
        let mut batch=dab_batch(target,layer_render::DabStyle::for_brush(&brush,layer_core::StrokeTool::Brush),layer_core::Rect::from_extent([256;2]));
        batch.kind=DabBatchKind::Preview;
        let dab=Dab {center:Default::default(),radii:[1.;2],rotation:[1.,0.],motion:[0.;2],color_rgba_linear:[1.;4],flow:1.,hardness:1.,
            texture_sign:[1.;2],material:[0.;4],previous:[0.;4],contact:[0.;4],previous_contact:[0.;4]};
        let dabs=[dab;28];
        let frame=packet(document.scene(),document.composition().size);
        let first=Prepared::from_scene(FramePacket {dab_batches:std::slice::from_ref(&batch),dabs:&dabs[..1],..frame},Declared::default()).unwrap();let node=source(target);
        let before=version(&first,&node);let mut moved=frame;moved.view.document_to_surface[4]=111.;
        batch.first_dab=27;
        let second=Prepared::from_scene(FramePacket {dab_batches:std::slice::from_ref(&batch),dabs:&dabs,..moved},Declared::default()).unwrap();
        assert_eq!(before.matches(&version(&second,&node)),Some(true));
        let without=version(&prepared(&document),&node);assert_eq!(before.matches(&without),Some(false));
    }
    #[test]
    fn pending_preview_metadata_keeps_other_targets_ready_without_a_selected_fallback() {
        let (mut document,target)=document();
        let (_,ready)=crate::test_support::add_paint(&mut document.artwork,"Ready",[1024,512]);
        let SourceTarget::Paint(handle)=target else {unreachable!()};
        document.artwork.paint.get_mut(handle).unwrap().raster=RasterRevision::pending();
        let style=layer_render::DabStyle::for_brush(&layer_core::BrushSnapshot::default(),layer_core::StrokeTool::Brush);
        let mut batch=dab_batch(target,style,layer_core::Rect::from_extent([64;2]));batch.kind=DabBatchKind::Preview;
        let dab=preview_dab(32.);
        let packet=FramePacket {dab_batches:std::slice::from_ref(&batch),dabs:std::slice::from_ref(&dab),..packet(document.scene(),document.composition().size)};
        let frame=Prepared::from_scene(packet,Declared::default()).expect("pending metadata is target-local preparation data");
        assert!(frame.preview_for(target).is_none());
        assert!(matches!(frame.expected(&source(target),PixelRect::new(0,0,256,256).into(),0,0),Err(GpuRasterError::NotReady)));
        assert!(frame.expected(&source(ready),PixelRect::new(0,0,256,256).into(),0,0).is_ok());
        let invalid_batch=DabBatch {first_dab:1,..batch.clone()};
        let invalid=FramePacket {dab_batches:std::slice::from_ref(&invalid_batch),..packet};
        assert!(matches!(Prepared::from_scene(invalid,Declared::default()),Err(GpuRasterError::InvalidDabRange)));
    }

    #[test]
    fn selected_object_placement_is_independent_of_image_storage() {
        use layer_core::authored::Image;
        let (mut document,_)=document();
        let image=Image::new(layer_core::color::source::rgba8_source([8;2],|_,_|[10,20,30,255]));
        let handle=document.artwork.objects.insert(PortableId::random(),ImageObject::new(image)).unwrap();
        let owner=document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(handle),"Object")).unwrap();
        document.artwork.stacks.get_mut(document.composition().result).unwrap().entries.push(owner);
        document=Document::from_artwork(document.artwork.clone()).unwrap();
        let mut index=object_spatial::SpatialIndex::default();
        let node=Arc::new(graph::Expression::Objects {content:index.content(document.scene(),owner).unwrap(),preview:false});
        let before=prepared(&document);
        document.artwork.objects.get_mut(handle).unwrap().image=Image::new(layer_core::color::source::rgba8_source([8;2],|_,_|[90,80,70,255]));
        let after=prepared(&document);
        assert_eq!(before.selected_placement(&node).unwrap(),after.selected_placement(&node).unwrap());
        assert_eq!(version(&before,&node).matches(&version(&after,&node)),Some(false));
        assert_ne!(before.authored_revision(&node).unwrap(),after.authored_revision(&node).unwrap());
    }

    #[test]
    fn a_deleted_object_expression_is_a_mismatch_not_preparation_work() {
        use layer_core::authored::Image;
        let (mut document,_)=document();
        let image=Image::new(layer_core::color::source::rgba8_source([8;2],|_,_|[10,20,30,255]));
        let handle=document.artwork.objects.insert(PortableId::random(),ImageObject::new(image)).unwrap();
        let owner=document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(handle),"Object")).unwrap();
        document.artwork.stacks.get_mut(document.composition().result).unwrap().entries.push(owner);
        let mut index=object_spatial::SpatialIndex::default();
        let node=Arc::new(graph::Expression::Objects {content:index.content(document.scene(),owner).unwrap(),preview:false});
        let before=prepared(&document);
        assert!(before.authored_revision(&node).is_ok());
        assert!(before.expected(&node,PixelRect::new(0,0,256,256).into(),0,0).is_ok());
        document.artwork.stacks.get_mut(document.composition().result).unwrap().entries.retain(|entry|*entry!=owner);
        document.artwork.occurrences.remove(owner);document.artwork.objects.remove(handle);
        let after=prepared(&document);
        for result in [after.authored_revision(&node).map(|_|()),after.expected(&node,PixelRect::new(0,0,256,256).into(),0,0).map(|_|())] {
            let error=result.expect_err("a deleted expression cannot supply a current name");
            assert!(!error.is_preparation_pending(),"a deleted expression has no pending producer: {error:?}");
        }
    }

}
