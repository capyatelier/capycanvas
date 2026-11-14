//! GPU decoded tile slots have fixed ownership. Jobs defer decoding until their
//! ordered upload; eviction never creates another retained GPU tile. Built-in
//! profiles decode on the GPU, while embedded profiles use the native CMM.
use super::*;
use crate::source_access::{RawTile, TileTexture};
use crate::residency::Pin;
use layer_core::color::{
    AlphaAssociation, ColorProfile, SampleDepth, PixelDescriptor, RgbSpace, TransferEncoding,
    source::{SourceChannels, SourceImage},
};
use layer_core::raster::TileBlob;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Weak,
        atomic::Ordering,
    },
};
pub(crate) type RequiredSource = (Arc<SourceImage>, [u32; 2]);
#[derive(Clone,Copy,PartialEq,Eq)]
pub(crate) enum SourceUse { Display, Exact }
pub(crate) const NORMALIZED_PAGE_BYTES: u64 = PAGE_SIZE as u64 * PAGE_SIZE as u64 * 16;
const ARRAY_BYTES: u64 = 16 << 20;
const DECODERS: usize = 4;
use crate::native_tiles::transfer;

#[derive(Clone,PartialEq,Eq,Hash)]
enum RasterIdentity {
    Encoded([u8; 32]),
    Owner(u64),
}
impl RasterIdentity {
    fn of(blob: &TileBlob) -> Self {
        blob.encoded_fingerprint().map_or_else(|| Self::Owner(blob.owner_identity()), Self::Encoded)
    }
}
pub(crate) fn same_raster_samples(a: &TileBlob, b: &TileBlob) -> bool { RasterIdentity::of(a) == RasterIdentity::of(b) }
#[derive(Clone)]
enum Key {
    Image(Weak<SourceImage>, [u32; 2]),
    PaintBase(Weak<SourceImage>, [u32; 2], [u32; 2]),
    Raster(RasterIdentity, RgbSpace, RgbSpace, PixelDescriptor),
}
impl PartialEq for Key {fn eq(&self,other:&Self)->bool {self.matches(other)}}
impl Eq for Key {}
impl std::hash::Hash for Key {
    fn hash<H:std::hash::Hasher>(&self,state:&mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Image(source,coordinate)=>(source.as_ptr(),coordinate).hash(state),
            Self::PaintBase(source,offset,coordinate)=>(source.as_ptr(),offset,coordinate).hash(state),
            Self::Raster(identity,space,destination,descriptor)=> {
                (identity,space,destination,descriptor.channels,descriptor.bits_per_channel).hash(state);
                std::mem::discriminant(&descriptor.sample).hash(state);
                std::mem::discriminant(&descriptor.encoding).hash(state);
                std::mem::discriminant(&descriptor.alpha).hash(state);
            },
        }
    }
}
#[cfg(test)]
std::thread_local! {
    static READS:std::cell::RefCell<Vec<Key>>=const {std::cell::RefCell::new(Vec::new())};
    static KEY_COMPARISONS:std::cell::Cell<Option<u64>>=const {std::cell::Cell::new(None)};
}
#[cfg(test)]
std::thread_local! {
    static ACCEPTED_RESIDENT_SET:std::cell::RefCell<Vec<wgpu::TextureView>>=const {std::cell::RefCell::new(Vec::new())};
    static RESIDENT_READS:std::cell::RefCell<Vec<wgpu::TextureView>>=const {std::cell::RefCell::new(Vec::new())};
    static ACCEPTED_READ_SET:std::cell::RefCell<Vec<Key>>=const {std::cell::RefCell::new(Vec::new())};
    static ACCEPTED_READ_RESULTS:std::cell::RefCell<Option<Vec<(usize,Vec<String>)>>>=const {std::cell::RefCell::new(None)};
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) struct InputUnit {area:PixelRect,input_level:u32}
impl InputUnit {
    pub(crate) fn new(area:PixelRect,input_level:u32)->Self {Self {area,input_level}}
    pub(crate) fn tile(coordinate:[u32;2])->Self {Self::new(page_rect(coordinate),0)}
}
fn merge_unit(held:&mut Option<InputUnit>,next:Option<InputUnit>)->Result<(),GpuRasterError> {
    if let (Some(held),Some(next))=(*held,next) {
        debug_assert_eq!(held,next,"incompatible accepted source units");
        if held!=next {return Err(GpuRasterError::Invariant("incompatible accepted source units"));}
    }
    if held.is_none() {*held=next;}
    Ok(())
}
pub(crate) type Address=(SourceTarget,RasterPlane,[u32;2]);
#[derive(Clone)]
enum Selected {
    Resident(RawTile,crate::resident::Input),
    Decoded(Key),
}
impl Selected {
    fn resident<T:crate::resident::TextureStorage>(read:crate::resident::Read<'_,T>)->Self {
        Self::Resident(RawTile {texture:read.value().texture().clone().into(),view:read.value().view().clone()},read.input())
    }
    fn matches(&self,other:&Self)->bool {
        match (self,other) {
            (Self::Resident(a,x),Self::Resident(b,y))=>a.view==b.view && x.same(y),
            (Self::Decoded(a),Self::Decoded(b))=>a.matches(b),
            _=>false,
        }
    }
    fn level(&self)->u32 {match self {Self::Resident(tile,_)=>PAGE_SIZE.ilog2().saturating_sub(tile.texture.width().ilog2()),Self::Decoded(_)=>0}}
    fn is_read<T:crate::resident::TextureStorage>(&self,read:&crate::resident::Read<'_,T>)->bool {
        matches!(self,Self::Resident(tile,witness) if tile.view==*read.value().view() && read.witnessed_by(witness))
    }
}
#[derive(Clone,Default)]
struct Decisions(std::collections::BTreeMap<Address,(bool,[Option<Selected>;2])>);
fn resident_witnesses(choices:impl Iterator<Item=(bool,[Option<Selected>;2])>)->Vec<crate::resident::Input> {
    choices.flat_map(|(_,choices)|choices).filter_map(|choice|match choice {Some(Selected::Resident(_,witness))=>Some(witness),_=>None}).collect()
}
impl Decisions {
    fn insert(&mut self,address:Address,raw:bool,selected:[Option<Selected>;2])->Result<(),GpuRasterError> {
        if let Some((kind,held))=self.0.get(&address) {
            let compatible=*kind==raw && held.iter().zip(&selected).all(|(a,b)|match (a,b) {(None,None)=>true,(Some(a),Some(b))=>a.matches(b),_=>false});
            debug_assert!(compatible,"incompatible accepted source decisions");
            if !compatible {return Err(GpuRasterError::Invariant("incompatible accepted source decisions"));}
        }else {self.0.insert(address,(raw,selected));}
        Ok(())
    }
    fn extend(&mut self,other:Self)->Result<(),GpuRasterError> {
        for (address,(raw,selected)) in other.0 {self.insert(address,raw,selected)?;}Ok(())
    }
    fn choose_resident<T:crate::resident::TextureStorage>(&mut self,address:Address,read:Option<crate::resident::Read<'_,T>>)->&[Option<Selected>;2] {
        let held=self.0.entry(address).or_insert((true,[None,None]));
        let kept=held.0 && held.1[1].is_none() && match (&held.1[0],&read) {(None,None)=>true,(Some(held),Some(read))=>held.is_read(read),_=>false};
        if !kept {*held=(true,[read.map(Selected::resident),None]);}
        &held.1
    }
}
fn finished(r:&WgpuRasterizer,derived:Option<&PreviewVersion>,surface:&crate::resident::Resident<crate::PageSurface>,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2],
    consumer:&crate::submission::Submission,
)->Option<Selected> {
    let ticket=derived.filter(|ticket|r.preview_declaration.as_ref().is_some_and(|declaration|declaration.finishes(ticket,target,plane,coordinate)))?;
    surface.read(ticket.version(),Some(consumer)).map(Selected::resident)
}
fn written_choices(r:&WgpuRasterizer,scene:SceneView<'_>,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2],persistent:bool,consumer:&crate::submission::Submission)
    ->Option<[Option<Selected>;2]> {
    let layer=r.paint_layers.iter().find(|layer|layer.id==target);
    let current=|surface:Option<&crate::resident::Resident<crate::PageSurface>>| {
        surface.filter(|_|persistent)?.read(r.raster_version_for(scene,target,plane,coordinate).ok()?,Some(consumer)).map(Selected::resident)
    };
    if plane==RasterPlane::Color {
        let preview=(r.preview_layer_id==Some(target)).then(||r.preview_page(coordinate)).flatten();
        let overlay=match preview {Some(page)=>Some(finished(r,page.derived.as_ref(),page.active(),target,plane,coordinate,consumer)?),None=>None};
        if overlay.is_some() && r.preview_requires_base {return Some([overlay,None]);}
        let base=current(layer.and_then(|layer|layer.pages.iter().find(|page|page.coordinate==coordinate)).map(crate::LayerPage::active))?;
        return Some([Some(base),overlay]);
    }
    let input=match r.preview_wetness_page(target,coordinate) {
        Some(page)=>finished(r,page.derived.as_ref(),page.active(),target,plane,coordinate,consumer),
        None=>current(layer.and_then(|layer|layer.watercolor_wetness_pages.iter().find(|page|page.coordinate==coordinate)).map(crate::WatercolorWetnessPage::active)),
    }?;
    Some([Some(input),None])
}
fn resolve(leases:&[Lease],choices:&[Option<Selected>;2])->Result<[Option<source_access::SourceRead>;2],GpuRasterError> {
    let resolve=|choice:&Option<Selected>|match choice {
        None=>Ok(None),
        Some(Selected::Resident(tile,_))=>{
            #[cfg(test)] if ACCEPTED_READ_RESULTS.with(|results|results.borrow().is_some()) {RESIDENT_READS.with(|reads|reads.borrow_mut().push(tile.view.clone()));}
            Ok(Some(source_access::SourceRead {tile:tile.clone(),pending:Vec::new(),assembly:None}))
        },
        Some(Selected::Decoded(key))=>accepted(leases,key.clone()).map(Some),
    };
    Ok([resolve(&choices[0])?,resolve(&choices[1])?])
}
#[derive(Clone)]
struct RawChoice {preview:Option<(super::PreviewStage,Option<(super::PreviewPass,u32)>)>,expected:Option<crate::raster::contents::Versions>}
#[derive(Clone)]
pub(crate) struct Declaration {
    wanted:Option<Arc<super::SignatureFrame>>,
    consumer:Option<crate::submission::Submission>,
    raw:Option<RawChoice>,
    failure:Option<GpuRasterError>,
}
impl Declaration {
    pub(crate) fn scene(wanted:Arc<super::SignatureFrame>,consumer:Option<crate::submission::Submission>)->Self {
        Self {wanted:Some(wanted),consumer,raw:None,failure:None}
    }
    pub(crate) fn current(consumer:Option<crate::submission::Submission>)->Self {Self {wanted:None,consumer,raw:None,failure:None}}
    pub(crate) fn raw(preview:Option<(super::PreviewStage,Option<(super::PreviewPass,u32)>)>,expected:Option<crate::raster::contents::Versions>,
        consumer:Option<crate::submission::Submission>,
    )->Self {Self {wanted:None,consumer,raw:Some(RawChoice {preview,expected}),failure:None}}
    pub(crate) fn with_raw_layers(mut self)->Self {self.raw=Some(RawChoice {preview:None,expected:None});self}
    fn wanted(&self)->Result<&super::SignatureFrame,GpuRasterError> {
        self.wanted.as_deref().ok_or(GpuRasterError::Invariant("source declaration has no signature frame"))
    }
    fn raw_color(&self,inputs:&mut Inputs,r:&WgpuRasterizer,target:SourceTarget,coordinate:[u32;2])->Result<(),GpuRasterError> {
        let (preview,expected)=self.raw.as_ref().map_or((None,None),|raw|(raw.preview,raw.expected.as_ref().map(|versions|versions.tile(RasterPlane::Color,coordinate))));
        let expected=expected.or_else(||self.wanted.as_ref().and_then(|frame|frame.tile_version(target,RasterPlane::Color,coordinate)));
        inputs.raw_color(r,target,coordinate,preview,expected.as_ref(),self.consumer.as_ref())
    }
}
#[cfg(test)]
std::thread_local! {static DECLARING:std::cell::Cell<bool>=const {std::cell::Cell::new(false)};}
#[cfg(test)]
struct DeclarePass(bool);
#[cfg(test)]
impl Drop for DeclarePass {fn drop(&mut self) {DECLARING.with(|flag|flag.set(self.0));}}
pub(crate) fn declare(inputs:Inputs,declaration:Declaration,traverse:impl FnOnce(&mut AcceptedInputs)->Result<(),GpuRasterError>)->Result<Inputs,GpuRasterError> {
    let mut accepted=AcceptedInputs::declare(inputs,declaration);
    let result={
        #[cfg(test)]
        let _pass=DeclarePass(DECLARING.with(|flag|flag.replace(true)));
        traverse(&mut accepted)
    };
    match result {Ok(())|Err(GpuRasterError::NotReady)=>accepted.declared(),Err(error)=>Err(error)}
}
#[cfg(test)]
pub(crate) fn assert_reading(entry:&str) {assert!(!DECLARING.with(std::cell::Cell::get),"{entry} is reachable in declare mode");}
#[cfg(test)]
std::thread_local! {pub(crate) static DECLARED_GPU_OBJECTS:std::cell::Cell<u64>=const {std::cell::Cell::new(0)};}
#[cfg(test)]
pub(crate) fn gpu_object_created() {if DECLARING.with(std::cell::Cell::get) {DECLARED_GPU_OBJECTS.with(|count|count.set(count.get()+1));}}
pub(in crate::scene) type Placements=std::collections::BTreeMap<SourceTarget,Arc<super::placement::Placement>>;
#[derive(Clone,Default)]
pub(crate) struct PlacementSet {frame:Arc<Placements>,overrides:Placements}
impl PlacementSet {
    pub(in crate::scene) fn new(frame:Arc<Placements>,overrides:Placements)->Self {Self {frame,overrides}}
    fn get(&self,id:&SourceTarget)->Option<&Arc<super::placement::Placement>> {self.overrides.get(id).or_else(||self.frame.get(id))}
    fn iter(&self)->impl Iterator<Item=(&SourceTarget,&Arc<super::placement::Placement>)> {self.overrides.iter().chain(self.frame.iter().filter(|(id,_)|!self.overrides.contains_key(id)))}
    fn len(&self)->usize {self.iter().count()}
    fn compatible(held:&super::placement::Placement,placement:&super::placement::Placement)->Result<(),GpuRasterError> {
        debug_assert!(held.matches(placement),"incompatible source placements");
        if held.matches(placement) {Ok(())}else {Err(GpuRasterError::Invariant("incompatible source placements"))}
    }
    fn insert(&mut self,id:SourceTarget,placement:Arc<super::placement::Placement>)->Result<(),GpuRasterError> {
        match self.get(&id) {Some(held)=>Self::compatible(held,&placement),None=>{self.overrides.insert(id,placement);Ok(())}}
    }
    fn extend(&mut self,other:Self)->Result<(),GpuRasterError> {
        if self.frame.is_empty() {self.frame=other.frame.clone();}
        if !Arc::ptr_eq(&self.frame,&other.frame) {return other.iter().try_for_each(|(id,placement)|self.insert(*id,placement.clone()));}
        for (id,placement) in &self.overrides {
            if let Some(framed)=other.frame.get(id).filter(|_|!other.overrides.contains_key(id)) {Self::compatible(framed,placement)?;}
        }
        other.overrides.into_iter().try_for_each(|(id,placement)|self.insert(id,placement))
    }
    fn retain(&mut self,kept:&Self) {
        self.overrides.retain(|id,_|kept.get(id).is_some());
        if !Arc::ptr_eq(&self.frame,&kept.frame) {self.frame=Arc::new(self.frame.iter().filter(|(id,_)|kept.get(id).is_some()).map(|(id,placement)|(*id,placement.clone())).collect());}
    }
}
#[derive(Clone,Default)]
pub(crate) struct Inputs {
    unit:Option<InputUnit>,
    tiles: Vec<(Key,Option<SourceColor>,Option<Pixels>,SourceUse)>,
    bases: Vec<(layer_core::authored::PaintBase,[u32;2])>,
    placements:PlacementSet,
    decisions:Decisions,
    objects:Vec<super::objects::CollectionJob>,
}
impl Inputs {
    pub(crate) fn for_unit(mut self,unit:InputUnit)->Result<Self,GpuRasterError> {merge_unit(&mut self.unit,Some(unit))?;Ok(self)}
    pub(crate) fn unit(&self)->Option<InputUnit> {self.unit}
    pub(crate) fn extend(&mut self,other:Self)->Result<(),GpuRasterError> {
        merge_unit(&mut self.unit,other.unit)?;
        for (base,coordinate) in other.bases {
            if !self.tiles.iter().any(|(key,_,_,_)|key.matches(&Key::base(&base,coordinate))) {self.bases.push((base,coordinate));}
        }
        for (key,color,pixels,use_) in other.tiles {self.insert_for(key,color,pixels,use_);}
        for request in other.objects {self.declare_object(request);}
        self.placements.extend(other.placements)?;
        self.decisions.extend(other.decisions)
    }
    fn declare_object(&mut self,request:super::objects::CollectionJob) {
        if !self.objects.iter().any(|known|known.same(&request)) {self.objects.push(request);}
    }
    pub(in crate::scene) fn objects(&self)->&[super::objects::CollectionJob] {&self.objects}
    pub(in crate::scene) fn resident(&self)->bool {self.tiles.is_empty() && self.bases.is_empty() && self.objects.is_empty()}
    pub(in crate::scene) fn placed(placements:PlacementSet)->Self {Self {placements,..Default::default()}}
    pub(in crate::scene) fn source_placement(&mut self,id:SourceTarget,placement:Arc<super::placement::Placement>)->Result<(),GpuRasterError> {self.placements.insert(id,placement)}
    pub(crate) fn accept<C,T,R:Admitted>(&self,r:&mut WgpuRasterizer,context:&mut C,
        reserve:impl FnOnce(&mut C,&mut WgpuRasterizer,&Self)->Option<R>,
        materialize:impl FnOnce(&mut C,&mut WgpuRasterizer)->Result<T,GpuRasterError>,
        commit:impl FnOnce(&mut C,&mut WgpuRasterizer,R)->Result<(),GpuRasterError>,
    )->Result<Option<(T,AcceptedInputs)>,GpuRasterError> {
        let Some(reservation)=reserve(context,r,self) else {return Ok(None);};
        let result=self.acquire(r,reservation.class()).and_then(|inputs|materialize(context,r).map(|output|(output,inputs)));
        commit(context,r,reservation)?;
        result.map(Some)
    }
    pub(crate) fn accept_query(&self,r:&mut WgpuRasterizer,scene:Option<&Scene>)->Result<AcceptedInputs,GpuRasterError> {
        self.accept(r,&mut (),|_,r,inputs| {
            let bytes=inputs.additional_bytes(&r.source_tiles.borrow());
            r.admit_display(scene,crate::residency::Class::Visible,bytes).ok()
        },|_,_|Ok(()),|_,_,_|Ok(()))?
            .map(|((),inputs)|inputs).ok_or(GpuRasterError::NotReady)
    }
    fn insert(&mut self,key:Key,color:Option<SourceColor>,pixels:Option<Pixels>) {
        self.insert_for(key,color,pixels,SourceUse::Exact);
    }
    fn insert_for(&mut self,key:Key,color:Option<SourceColor>,pixels:Option<Pixels>,use_:SourceUse) {
        if let Some((_,_,_,held))=self.tiles.iter_mut().find(|(stored,_,_,_)|stored.matches(&key)) {
            if use_==SourceUse::Exact {*held=use_;}
        }else {self.tiles.push((key,color,pixels,use_));}
    }
    pub(crate) fn image(&mut self,source:&Arc<SourceImage>,coordinate:[u32;2],destination:RgbSpace) {
        self.image_for(source,coordinate,destination,SourceUse::Exact);
    }
    pub(crate) fn image_for(&mut self,source:&Arc<SourceImage>,coordinate:[u32;2],destination:RgbSpace,use_:SourceUse) {
        self.insert_for(Key::Image(Arc::downgrade(source),coordinate),builtin_settings(source,coordinate,destination),Some(Pixels::Image(source.clone(),coordinate)),use_);
    }
    pub(crate) fn raster(&mut self,blob:Arc<TileBlob>,space:RgbSpace,destination:RgbSpace) {
        let mut color=rgb_settings(space,destination,blob.descriptor.depth(),[PAGE_SIZE;2],blob.descriptor.alpha);
        color.scalar=u32::from(blob.descriptor.channels==1);color.scalar_scale=if color.scalar!=0 {1./blob.descriptor.depth().maximum() as f32}else {0.};
        self.insert(Key::raster(&blob,space,destination),Some(color),Some(Pixels::Raster(blob,space,destination)));
    }
    pub(crate) fn base(&mut self,base:&layer_core::authored::PaintBase,coordinate:[u32;2],destination:RgbSpace) {
        let bounds=crate::source_access::paint_base_source_bounds(base,coordinate);
        if bounds.is_empty() {return;}
        if base.offset.iter().any(|offset|offset%PAGE_SIZE!=0) {
            let key=Key::PaintBase(Arc::downgrade(base.image.storage()),base.offset,coordinate);
            if !self.tiles.iter().any(|(stored,_,_,_)|stored.matches(&key)) {self.bases.push((base.clone(),coordinate));}
            self.insert(key,None,None);
        }
        for coordinate in page_coordinates(bounds) {self.image(base.image.storage(),coordinate,destination);}
    }
    fn fallback(&mut self,r:&WgpuRasterizer,scene:SceneView<'_>,id:SourceTarget,plane:RasterPlane,coordinate:[u32;2])->Result<Option<Selected>,GpuRasterError> {
        let space=r.document_color().space;
        if let Some(blob)=r.raster_tile_for(scene,id,plane,coordinate)? {
            let key=Key::raster(&blob,space,space);self.raster(blob,space,space);return Ok(Some(Selected::Decoded(key)));
        }
        if plane==RasterPlane::Color && let Some(base)=scene.paint_base(id) && source_access::paint_base_contains(base,coordinate) {
            self.base(base,coordinate,space);return Ok(Some(Selected::Decoded(Key::base(base,coordinate))));
        }
        Ok(None)
    }
    fn raw_scalar(&mut self,r:&WgpuRasterizer,id:SourceTarget,plane:RasterPlane,coordinate:[u32;2],wanted:Option<&super::SignatureFrame>,consumer:Option<&crate::submission::Submission>)->Result<(),GpuRasterError> {
        let framed=wanted.and_then(|frame|frame.tile_version(id,plane,coordinate));
        let resident=if plane==RasterPlane::Mask {
            let name=framed.map_or_else(||r.current_mask_version(id,coordinate),Ok)?;
            r.layer_masks.pages.get(&(id,coordinate)).and_then(|page|page.read(name,consumer).ok()).map(Selected::resident)
        }else {
            let name=framed.map_or_else(||r.current_raster_version(id,plane,coordinate),Ok)?;
            r.paint_layers.iter().find(|layer|layer.id==id).and_then(|layer|layer.watercolor_wetness_pages.iter().find(|page|page.coordinate==coordinate))
                .and_then(|page|page.active().read(name,consumer)).map(Selected::resident)
        };
        let selected=if let Some(read)=resident {Some(read)}else if let Some(blob)=r.native_plane_tile(id,plane,coordinate)? {
            let space=r.document_color().space;let key=Key::raster(&blob,space,space);self.raster(blob,space,space);Some(Selected::Decoded(key))
        }else {None};
        self.decisions.insert((id,plane,coordinate),true,[selected,None])
    }
    fn raw_color(&mut self,r:&WgpuRasterizer,id:SourceTarget,coordinate:[u32;2],preview:Option<(PreviewStage,Option<(PreviewPass,u32)>)>,expected:Option<&layer_core::raster::ContentVersion>,consumer:Option<&crate::submission::Submission>)->Result<(),GpuRasterError> {
        if let Some(expected)=expected && r.current_raster_version(id,RasterPlane::Color,coordinate)?.matches(expected)!=Some(true) {return Err(GpuRasterError::NotReady);}
        let predicted=preview.filter(|_|r.preview_layer_id==Some(id)).and_then(|_|r.preview_page(coordinate));
        let persistent=r.paint_layers.iter().find(|layer|layer.id==id).and_then(|layer|layer.pages.iter().find(|page|page.coordinate==coordinate));
        let resident=if let Some(page)=predicted {
            let (stage,intermediate)=preview.unwrap();
            let version=if let Some((pass,step))=intermediate {r.preview_version(id,RasterPlane::Color,coordinate,stage)?.intermediate(pass,step).version().clone()}
                else {r.preview_expected(id,RasterPlane::Color,coordinate,stage)?};
            Some(Selected::resident(page.active().read(version,consumer).ok_or(GpuRasterError::NotReady)?))
        }else if let Some(page)=persistent {
            let version=match expected {Some(version)=>version.clone(),None=>r.current_raster_version(id,RasterPlane::Color,coordinate)?};
            page.active().read(version,consumer).map(Selected::resident)
        }else {None};
        let selected=if let Some(read)=resident {Some(read)}else {
            let space=r.document_color().space;
            if let Some(blob)=r.native_color_tile(id,coordinate)? {
                let key=Key::raster(&blob,space,space);self.raster(blob,space,space);Some(Selected::Decoded(key))
            }else if let Some(base)=r.tiled_sources.get(&id) && source_access::paint_base_contains(base,coordinate) {
                self.base(base,coordinate,space);Some(Selected::Decoded(Key::base(base,coordinate)))
            }else {None}
        };
        self.decisions.insert((id,RasterPlane::Color,coordinate),true,[selected,None])
    }
    fn record_capture(&mut self,r:&WgpuRasterizer,scene:SceneView<'_>,wanted:Option<&super::SignatureFrame>,consumer:Option<&crate::submission::Submission>,address:Address)->Result<bool,GpuRasterError> {
        let (id,plane,coordinate)=address;
        let Some(capture)=wanted.and_then(|frame|frame.capture_for(id)) else {return Ok(false);};
        let transforms=r.transforms.as_ref().ok_or(GpuRasterError::NotReady)?;
        let selected=match transforms.read_native(r,id,plane,coordinate,capture,consumer)? {
            crate::paint_transform::NativeRead::Resident(read)=>Some(Selected::resident(read)),
            crate::paint_transform::NativeRead::Original(expected)=>{
                if r.raster_version_for(scene,id,plane,coordinate)?.matches(&expected)!=Some(true) {return Err(GpuRasterError::NotReady);}
                self.fallback(r,scene,id,plane,coordinate)?
            },
            crate::paint_transform::NativeRead::Display=>return Ok(false),
        };
        self.decisions.insert(address,false,[selected,None])?;
        Ok(true)
    }
    fn color(&mut self,r:&WgpuRasterizer,scene:SceneView<'_>,wanted:Option<&super::SignatureFrame>,consumer:Option<&crate::submission::Submission>,id:SourceTarget,coordinate:[u32;2])->Result<(),GpuRasterError> {
        if self.record_capture(r,scene,wanted,consumer,(id,RasterPlane::Color,coordinate))? {return Ok(());}
        let persistent=r.paint_layers.iter().find(|layer|layer.id==id).and_then(|layer|layer.pages.iter().find(|page|page.coordinate==coordinate));
        let preview=(r.preview_layer_id==Some(id)).then(||r.preview_page(coordinate)).flatten();
        let page=preview.filter(|_|r.preview_requires_base).or(persistent);
        let resident=page.map(|page|r.read_scene_color_input(scene,id,page,preview.is_some_and(|p|std::ptr::eq(p,page)),wanted,consumer))
            .transpose()?.flatten().map(Selected::resident);
        let base=match resident {Some(read)=>Some(read),None=>self.fallback(r,scene,id,RasterPlane::Color,coordinate)?};
        let overlay=preview.filter(|_|!r.preview_requires_base).map(|page|r.read_scene_color_input(scene,id,page,true,wanted,consumer))
            .transpose()?.flatten().map(Selected::resident);
        self.decisions.insert((id,RasterPlane::Color,coordinate),false,[base,overlay])
    }
    fn mask(&mut self,r:&WgpuRasterizer,wanted:&super::SignatureFrame,consumer:Option<&crate::submission::Submission>,id:SourceTarget,source:&CoverageSource,coordinate:[u32;2])->Result<(),GpuRasterError> {
        let resident=r.read_mask(id,source,coordinate,Some(wanted),consumer)?.map(Selected::resident);
        let input=if let Some(read)=resident {Some(read)}else {
            let data=r.mask_data(id,source)?;
            if let Some(tile)=data.tiles.get(&layer_core::raster::TileKey {plane:RasterPlane::Mask,coordinate}) {
                if tile.content_version().matches(&r.mask_version(id,source,coordinate)?)!=Some(true) {return Err(GpuRasterError::NotReady);}
                let blob=tile.try_backing().ok_or(GpuRasterError::NotReady)?.map_err(GpuRasterError::Effect)?;
                let space=r.document_color().space;let key=Key::raster(&blob,space,space);self.raster(blob,space,space);Some(Selected::Decoded(key))
            }else {None}
        };
        self.decisions.insert((id,RasterPlane::Mask,coordinate),false,[input,None])
    }
    fn scalar(&mut self,r:&WgpuRasterizer,scene:SceneView<'_>,wanted:Option<&super::SignatureFrame>,consumer:Option<&crate::submission::Submission>,id:SourceTarget,plane:RasterPlane,coordinate:[u32;2])->Result<(),GpuRasterError> {
        if self.record_capture(r,scene,wanted,consumer,(id,plane,coordinate))? {return Ok(());}
        let predicted=r.preview_wetness_page(id,coordinate);
        let page=predicted.or_else(||r.paint_layers.iter().find(|layer|layer.id==id)
            .and_then(|layer|layer.watercolor_wetness_pages.iter().find(|page|page.coordinate==coordinate)));
        let resident=page.map(|page|r.read_scene_wetness_input(scene,id,page,predicted.is_some(),wanted,consumer))
            .transpose()?.flatten().map(Selected::resident);
        let input=match resident {Some(read)=>Some(read),None=>self.fallback(r,scene,id,plane,coordinate)?};
        self.decisions.insert((id,plane,coordinate),false,[input,None])
    }
    pub(crate) fn additional_bytes(&self,cache:&DecodedTiles)->u64 {
        let mut bytes=self.tiles.iter().map(|(key,color,_,use_)| {
            if cache.keyed(key).is_some_and(|slot|slot.valid.load(Ordering::Acquire)
                && (*use_==SourceUse::Display || slot.layer.format()==wgpu::TextureFormat::Rgba32Float)) {return 0;}
            let format=if *use_==SourceUse::Display {color.map_or(wgpu::TextureFormat::Rgba32Float,SourceColor::input_format)}else {wgpu::TextureFormat::Rgba32Float};
            u64::from(PAGE_SIZE).pow(2)*u64::from(format.block_copy_size(None).unwrap())
        }).sum::<u64>();
        let mut formats=std::collections::HashSet::new();let mut tables=std::collections::BTreeSet::new();
        for (key,color,_,use_) in &self.tiles {
            if cache.keyed(key).is_some_and(|slot|slot.layer.format()==wgpu::TextureFormat::Rgba32Float && slot.ready()) {continue;}
            let Some(color)=color else {continue;};
            let format=color.input_format();
            if *use_==SourceUse::Exact && !cache.inputs.iter().any(|input|input.texture.format()==format) && formats.insert(format) {bytes+=u64::from(PAGE_SIZE).pow(2)*u64::from(format.block_copy_size(None).unwrap());}
            let space=color.space();
            if !cache.transfer.contains(space) && tables.insert(transfer::Tables::index(space)) {bytes+=transfer::TABLE_BYTES;}
        }
        bytes
    }
    #[cfg(test)]
    pub(in crate::scene) fn take_reads(&self)->(usize,usize) {
        READS.with(|reads| {let reads=std::mem::take(&mut *reads.borrow_mut());
            (reads.len(),reads.iter().filter(|read|!self.tiles.iter().any(|(key,_,_,_)|key.matches(read))).count())})
    }
    #[cfg(test)]
    pub(in crate::scene) fn begin_accepted_reads() {
        RESIDENT_READS.with(|reads|reads.borrow_mut().clear());
        ACCEPTED_RESIDENT_SET.with(|set|set.borrow_mut().clear());
        ACCEPTED_READ_RESULTS.with(|results|*results.borrow_mut()=Some(Vec::new()));
    }
    #[cfg(test)]
    pub(in crate::scene) fn take_accepted_read_results()->Vec<(usize,Vec<String>)> {
        let results=ACCEPTED_READ_RESULTS.with(|results|results.borrow_mut().take().unwrap_or_default());
        RESIDENT_READS.with(|reads|reads.borrow_mut().clear());
        ACCEPTED_RESIDENT_SET.with(|set|set.borrow_mut().clear());results
    }
    pub(crate) fn acquire(&self,r:&WgpuRasterizer,class:crate::residency::Class)->Result<AcceptedInputs,GpuRasterError> {
        let mut accepted=Vec::new();
        for (key,color,pixels,use_) in &self.tiles {
            let mut cache=r.source_tiles.borrow_mut();
            if *use_==SourceUse::Exact && let Some(color)=color && !cache.keyed(key).is_some_and(|slot|slot.layer.format()==wgpu::TextureFormat::Rgba32Float
                && slot.ready()) {cache.prepare_color(&r.device,*color)?;}
            let (tile,pending)=match pixels {
                Some(Pixels::Image(source,coordinate))=>cache.plan_image(r,source,*coordinate,*use_)?,
                Some(Pixels::Raster(blob,space,destination))=>cache.plan_raster(r,blob,*space,*destination)?,
                _=>continue,
            };
            let (tile,promotion)=if *use_==SourceUse::Exact {cache.promote(r,tile)?}else {(tile,None)};
            cache.stamp(&tile.view,class);
            let mut lease=cache.lease(&tile.view).unwrap();
            lease.read=Some(Arc::new(Read {key:key.clone(),tile,pending:pending.into_iter().chain(promotion).collect(),assembly:None}));
            accepted.push(lease);
        }
        for (base,coordinate) in &self.bases {
            let Some(read)=plan_paint_base(r,base,*coordinate)? else {continue;};
            let cache=r.source_tiles.borrow();
            read.assembly.iter().flat_map(|assembly|assembly.sources()).chain([&read.tile.view]).for_each(|view|cache.stamp(view,class));
            let mut lease=cache.lease(&read.tile.view).unwrap();drop(cache);
            lease.read=Some(Arc::new(Read {key:Key::base(base,*coordinate),tile:read.tile,pending:read.pending,assembly:read.assembly}));
            accepted.push(lease);
        }
        Ok(AcceptedInputs {brush:None,brush_state:None,unit:self.unit,leases:accepted,placements:self.placements.clone(),decisions:self.decisions.clone(),declaring:None,objects_refusal:None})
    }
}
pub(crate) trait Admitted {fn class(&self)->crate::residency::Class;}
impl Admitted for crate::residency::Reservation {fn class(&self)->crate::residency::Class {crate::residency::Reservation::class(self)}}
impl<T> Admitted for (crate::residency::Reservation,T) {fn class(&self)->crate::residency::Class {self.0.class()}}
fn accepted(inputs:&[Lease],key:Key)->Result<source_access::SourceRead,GpuRasterError> {
    #[cfg(test)]
    if ACCEPTED_READ_RESULTS.with(|results|results.borrow().is_some()) {READS.with(|reads|reads.borrow_mut().push(key.clone()));}
    let read=inputs.iter().filter_map(|input|input.read.as_ref()).find(|read|read.key.matches(&key));
    #[cfg(test)]
    if read.is_none() {crate::test_support::PassCounts {source_input_misses:1,..Default::default()}.record();}
    debug_assert!(read.is_some(),"accepted input is missing");
    let read=read.ok_or(GpuRasterError::Invariant("accepted source input is missing"))?;
    Ok(read.source())
}
pub(crate) fn accepted_image(inputs:&[Lease],source:&Arc<SourceImage>,coordinate:[u32;2])->Result<source_access::SourceRead,GpuRasterError> {
    accepted(inputs,Key::Image(Arc::downgrade(source),coordinate))
}
pub(crate) fn accepted_raster(inputs:&[Lease],blob:&Arc<TileBlob>,space:RgbSpace,destination:RgbSpace)->Result<source_access::SourceRead,GpuRasterError> {
    accepted(inputs,Key::raster(blob,space,destination))
}
pub(crate) fn accepted_base(inputs:&[Lease],base:&layer_core::authored::PaintBase,coordinate:[u32;2])->Result<Option<source_access::SourceRead>,GpuRasterError> {
    if !source_access::paint_base_contains(base,coordinate) {return Ok(None);}
    accepted(inputs,Key::base(base,coordinate)).map(Some)
}
#[cfg(test)]
pub(in crate::scene) fn declare_colors(r:&WgpuRasterizer,scene:SceneView<'_>,wanted:Arc<super::SignatureFrame>,consumer:Option<crate::submission::Submission>,
    id:SourceTarget,area:PixelRect,
)->Result<Inputs,GpuRasterError> {
    declare(Inputs::default(),Declaration::scene(wanted,consumer),|accepted|page_coordinates(area).try_for_each(|c|accepted.color(r,scene,id,c).map(drop)))
}
#[cfg(test)]
pub(crate) struct Recording(Option<(Vec<Key>,Vec<Key>,Vec<wgpu::TextureView>,Vec<wgpu::TextureView>)>);
#[cfg(test)]
pub(crate) fn record_reads(inputs:&AcceptedInputs)->Recording {
    if inputs.declaring() || !ACCEPTED_READ_RESULTS.with(|results|results.borrow().is_some()) {return Recording(None);}
    let wanted=inputs.iter().filter_map(|lease|lease.read.as_ref().map(|read|read.key.clone())).collect();
    let previous=ACCEPTED_READ_SET.with(|set|std::mem::replace(&mut *set.borrow_mut(),wanted));
    let reads=READS.with(|reads|std::mem::take(&mut *reads.borrow_mut()));
    let resident_views=inputs.decisions.0.values().flat_map(|(_,choices)|choices).filter_map(|choice|match choice {Some(Selected::Resident(tile,_))=>Some(tile.view.clone()),_=>None})
        .chain(inputs.brush.iter().flat_map(|inputs|inputs.values()).chain(inputs.brush_state.iter().flat_map(|inputs|inputs.values()))
            .flat_map(|input|input.persistent.iter().chain(input.preview.iter().flatten())).map(|input|input.value_view().clone())).collect();
    let previous_residents=ACCEPTED_RESIDENT_SET.with(|set|std::mem::replace(&mut *set.borrow_mut(),resident_views));
    let resident_reads=RESIDENT_READS.with(|reads|std::mem::take(&mut *reads.borrow_mut()));
    Recording(Some((previous,reads,previous_residents,resident_reads)))
}
#[cfg(test)]
impl Drop for Recording {
    fn drop(&mut self) {
        let Some((previous,reads_before,previous_residents,resident_reads_before))=self.0.take() else {return;};
        let residents=RESIDENT_READS.with(|reads|std::mem::replace(&mut *reads.borrow_mut(),resident_reads_before));
        let wanted=ACCEPTED_RESIDENT_SET.with(|set|std::mem::replace(&mut *set.borrow_mut(),previous_residents));
        ACCEPTED_READ_SET.with(|set|READS.with(|reads| {
            let reads=std::mem::replace(&mut *reads.borrow_mut(),reads_before);
            let mut unexpected:Vec<_>=reads.iter().filter(|read|!set.borrow().iter().any(|key|key.matches(read)))
                .map(|key|match key {
                    Key::Image(_,coordinate)=>format!("image {coordinate:?}"),
                    Key::PaintBase(_,offset,coordinate)=>format!("base offset={offset:?} tile={coordinate:?}"),
                    Key::Raster(_,_,_,descriptor)=>format!("raster {descriptor:?}"),
                }).collect();
            unexpected.extend(residents.iter().filter(|view|!wanted.contains(view)).map(|_|"resident view outside accepted record".to_string()));
            *set.borrow_mut()=previous;
            ACCEPTED_READ_RESULTS.with(|results|if let Some(results)=results.borrow_mut().as_mut() {results.push((reads.len()+residents.len(),unexpected));});
        }));
    }
}

impl Key {
    fn base(base:&layer_core::authored::PaintBase,coordinate:[u32;2])->Self {
        if base.offset.iter().all(|value|value%PAGE_SIZE==0) {Self::Image(Arc::downgrade(base.image.storage()),std::array::from_fn(|axis|coordinate[axis]-base.offset[axis]/PAGE_SIZE))}
        else {Self::PaintBase(Arc::downgrade(base.image.storage()),base.offset,coordinate)}
    }
    fn raster(blob: &TileBlob, space: RgbSpace, destination: RgbSpace) -> Self {
        Self::Raster(RasterIdentity::of(blob), space, destination, blob.descriptor)
    }
    fn matches(&self, other: &Self) -> bool {
        #[cfg(test)]
        KEY_COMPARISONS.with(|count|if let Some(value)=count.get() {count.set(Some(value+1));});
        match (self, other) {
            (Self::Image(a, x), Self::Image(b, y)) => a.ptr_eq(b) && x == y,
            (Self::PaintBase(a, x, p), Self::PaintBase(b, y, q)) => a.ptr_eq(b) && x == y && p == q,
            (Self::Raster(a, x, d, p), Self::Raster(b, y, e, q)) => a == b && x == y && d == e && p == q,
            _ => false,
        }
    }
    fn samples(&self, destination: RgbSpace) -> Option<Samples> {
        match self {
            Self::Image(source, coordinate) => {
                let source = source.upgrade()?;
                let ColorProfile::Builtin(space) = source.interpretation.profile else { return None; };
                if source.interpretation.channels == SourceChannels::Cmyk || source.interpretation.depth.is_float() { return None; }
                Some(Samples { data: builtin_settings(&source, *coordinate, destination)?, space })
            }
            Self::Raster(_, space, destination, descriptor) if descriptor.channels > 1 && !descriptor.depth().is_float() => {
                Some(Samples { data: rgb_settings(*space, *destination, descriptor.depth(), [PAGE_SIZE; 2], descriptor.alpha), space: *space })
            }
            _ => None,
        }
    }
}
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Representation { Unpacked, Unorm8, Packed16, Srgb8 }
impl Representation {
    fn for_format(format:wgpu::TextureFormat)->Self {
        match format {wgpu::TextureFormat::Rgba8Unorm=>Self::Unorm8,wgpu::TextureFormat::Rg32Uint=>Self::Packed16,_=>Self::Unpacked}
    }
    fn format(self)->wgpu::TextureFormat {
        match self {Self::Unorm8|Self::Srgb8=>wgpu::TextureFormat::Rgba8Unorm,Self::Packed16=>wgpu::TextureFormat::Rg32Uint,Self::Unpacked=>wgpu::TextureFormat::Rgba32Float}
    }
}
#[repr(u32)]
#[derive(Clone, Copy)]
enum TransferCurve { Srgb=0, AdobeRgb=2, ProPhoto=3 }
impl From<RgbSpace> for TransferCurve {
    fn from(space:RgbSpace)->Self {match space {RgbSpace::Srgb|RgbSpace::DisplayP3=>Self::Srgb,RgbSpace::AdobeRgb=>Self::AdobeRgb,RgbSpace::ProPhoto=>Self::ProPhoto}}
}
#[repr(C,align(16))]
#[derive(Clone, Copy)]
pub(crate) struct SourceColor {
    red:[f32;4],green:[f32;4],blue:[f32;4],
    premultiplied:u32,integer_max:f32,extent:[f32;2],
    float_bits:u32,identity:u32,scalar:u32,scalar_scale:f32,
    representation:Representation,display:u32,transfer:TransferCurve,padding:u32,
}
impl SourceColor {
    fn space(self)->RgbSpace {match self.transfer {TransferCurve::AdobeRgb=>RgbSpace::AdobeRgb,TransferCurve::ProPhoto=>RgbSpace::ProPhoto,_=>RgbSpace::Srgb}}
    fn input_format(self)->wgpu::TextureFormat {
        match self.representation {
            Representation::Unorm8 if self.scalar==0=>wgpu::TextureFormat::Rgba8Unorm,
            Representation::Unorm8=>wgpu::TextureFormat::Rgba8Uint,
            Representation::Packed16 if self.scalar==0=>wgpu::TextureFormat::Rg32Uint,
            Representation::Packed16=>wgpu::TextureFormat::Rgba16Uint,
            _ if self.float_bits!=0=>wgpu::TextureFormat::Rgba32Uint,
            _=>wgpu::TextureFormat::Rgba16Uint,
        }
    }
    pub(crate) fn write_uniform(self,bytes:&mut [u8]) {
        let mut words=[0;24];
        for (output,value) in words[..12].iter_mut().zip(self.red.into_iter().chain(self.green).chain(self.blue)) {*output=value.to_bits();}
        words[12..].copy_from_slice(&[self.premultiplied,self.integer_max.to_bits(),self.extent[0].to_bits(),self.extent[1].to_bits(),
            self.float_bits,self.identity,self.scalar,self.scalar_scale.to_bits(),self.representation as u32,self.display,self.transfer as u32,self.padding]);
        for (output,word) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(words) {*output=word.to_ne_bytes();}
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Samples { data:SourceColor,space:RgbSpace }
impl Samples {
    pub(crate) fn for_source(source:&SourceImage,coordinate:[u32;2],destination:RgbSpace)->Option<Self> {
        let ColorProfile::Builtin(space)=source.interpretation.profile else {return None;};
        if source.interpretation.depth.is_float() {return None;}
        Some(Self {data:builtin_settings(source,coordinate,destination)?,space})
    }
    pub(crate) fn format(self)->wgpu::TextureFormat {self.data.representation.format()}
    fn parameters(mut self,format:wgpu::TextureFormat)->SourceColor {
        self.data.representation=Representation::for_format(format);
        self.data
    }
    pub(crate) fn display_parameters(mut self)->SourceColor {
        self.data.display=1;
        if self.srgb_view() {self.data.representation=Representation::Srgb8;}
        self.data
    }
    pub(crate) fn srgb_view(self)->bool {self.data.representation==Representation::Unorm8 && matches!(self.space,RgbSpace::Srgb|RgbSpace::DisplayP3)}
}

#[derive(Clone)]
pub(crate) struct Lease { owner:Arc<dyn Send+Sync>,_encoded:Option<Arc<Pin>>,read:Option<Arc<Read>> }
#[derive(Clone,Default)]
pub(crate) struct BrushResidents {
    pub persistent:Vec<crate::resident::Resident<RawTile>>,
    pub preview:Option<Vec<crate::resident::Resident<RawTile>>>,
}
#[derive(Clone,Copy,PartialEq,Eq,PartialOrd,Ord)]
pub(crate) enum BrushState {Coverage(u32,bool),Start(layer_core::StrokeId),Reservoir}
pub(crate) type PreparedState=std::collections::BTreeMap<(SourceTarget,[u32;2],BrushState),BrushResidents>;
pub(crate) type PreparedBrush=std::collections::BTreeMap<Address,BrushResidents>;
#[derive(Clone,Default)]
pub(crate) struct AcceptedInputs {brush:Option<Arc<PreparedBrush>>,brush_state:Option<Arc<PreparedState>>,unit:Option<InputUnit>,leases:Vec<Lease>,placements:PlacementSet,decisions:Decisions,declaring:Option<Box<(Inputs,Declaration)>>,objects_refusal:Option<crate::residency::Blocked>}
impl std::ops::Deref for AcceptedInputs {type Target=[Lease];fn deref(&self)->&Self::Target {&self.leases}}
impl IntoIterator for AcceptedInputs {
    type Item=Lease;type IntoIter=std::vec::IntoIter<Lease>;
    fn into_iter(self)->Self::IntoIter {self.leases.into_iter()}
}
impl From<Vec<Lease>> for AcceptedInputs {fn from(leases:Vec<Lease>)->Self {Self {leases,..Default::default()}}}
impl AcceptedInputs {
    pub(crate) fn prepare_brush(_: &crate::raster::staging::BrushPreparation,residents:Arc<PreparedBrush>,state:Arc<PreparedState>)->Self {
        Self {brush:Some(residents),brush_state:Some(state),..Default::default()}
    }
    pub(crate) fn brush_color(&mut self,r:&WgpuRasterizer,target:SourceTarget,coordinate:[u32;2],
        preview:Option<(PreviewStage,Option<(PreviewPass,u32)>)>,receipt:&crate::submission::Submission,
    )->Result<[Option<source_access::SourceRead>;2],GpuRasterError> {
        let address=(target,RasterPlane::Color,coordinate);
        let inputs=self.brush.as_ref().and_then(|inputs|inputs.get(&address))
            .ok_or(GpuRasterError::Invariant("brush input is not prepared"))?;
        let predicted=preview.filter(|_|inputs.preview.is_some());
        let family=predicted.and(inputs.preview.as_ref()).unwrap_or(&inputs.persistent);
        let read=if family.is_empty() {None}else {
            let expected=match predicted {
                Some((stage,Some((pass,step))))=>r.preview_version(target,RasterPlane::Color,coordinate,stage)
                    .map(|version|version.intermediate(pass,step).version().clone()),
                Some((stage,None))=>r.preview_expected(target,RasterPlane::Color,coordinate,stage),
                None=>r.current_raster_version(target,RasterPlane::Color,coordinate),
            }.map_err(crate::prepared_brush_error)?;
            Some(family.iter().find_map(|resident|resident.read(&expected,Some(receipt)))
                .ok_or(GpuRasterError::Invariant("prepared brush input version is not readable"))?)
        };
        resolve(&self.leases,self.decisions.choose_resident(address,read))
    }
    pub(crate) fn brush_wetness(&mut self,r:&WgpuRasterizer,target:SourceTarget,coordinate:[u32;2],
        preview:Option<(PreviewStage,Option<(PreviewPass,u32)>)>,receipt:&crate::submission::Submission,
    )->Result<Option<source_access::SourceRead>,GpuRasterError> {
        let address=(target,RasterPlane::WatercolorWetness,coordinate);
        let inputs=self.brush.as_ref().and_then(|inputs|inputs.get(&address))
            .ok_or(GpuRasterError::Invariant("brush wetness input is not prepared"))?;
        let family=if preview.is_some() {inputs.preview.as_deref().unwrap_or(&[])}else {&inputs.persistent};
        let read=if family.is_empty() {None}else {
            let expected=match preview {
                Some((stage,Some((pass,step))))=>r.preview_version(target,RasterPlane::WatercolorWetness,coordinate,stage)
                    .map(|version|version.intermediate(pass,step).version().clone()),
                Some((stage,None))=>r.preview_expected(target,RasterPlane::WatercolorWetness,coordinate,stage),
                None=>r.current_raster_version(target,RasterPlane::WatercolorWetness,coordinate),
            }.map_err(crate::prepared_brush_error)?;
            Some(family.iter().find_map(|resident|resident.read(&expected,Some(receipt)))
                .ok_or(GpuRasterError::Invariant("prepared brush wetness version is not readable"))?)
        };
        resolve(&self.leases,self.decisions.choose_resident(address,read)).map(|[base,_]|base)
    }
    pub(crate) fn brush_state(&self,target:SourceTarget,coordinate:[u32;2],state:BrushState,preview:bool,
        expected:&layer_core::raster::ContentVersion,receipt:&crate::submission::Submission,
    )->Result<(RawTile,crate::resident::Input),GpuRasterError> {
        let input=self.brush_state.as_ref().and_then(|inputs|inputs.get(&(target,coordinate,state)))
            .ok_or(GpuRasterError::Invariant("brush state input is not prepared"))?;
        let family=if preview {input.preview.as_ref().unwrap_or(&input.persistent)}else {&input.persistent};
        let read=family.iter().find_map(|resident|resident.read(expected,Some(receipt)))
            .ok_or(GpuRasterError::Invariant("prepared brush state version is not readable"))?;
        #[cfg(test)] if ACCEPTED_READ_RESULTS.with(|results|results.borrow().is_some()) {RESIDENT_READS.with(|reads|reads.borrow_mut().push(read.value().view.clone()));}
        Ok((read.value().clone(),read.input()))
    }
    pub(crate) fn for_unit(mut self,unit:InputUnit)->Result<Self,GpuRasterError> {merge_unit(&mut self.unit,Some(unit))?;Ok(self)}
    pub(crate) fn unit(&self)->Option<InputUnit> {self.unit}
    pub(crate) fn check_unit(&self,unit:InputUnit)->Result<(),GpuRasterError> {
        debug_assert_eq!(self.unit,Some(unit),"accepted source unit differs from its consumer");
        if self.unit==Some(unit) {Ok(())}else {Err(GpuRasterError::Invariant("accepted source unit differs from its consumer"))}
    }
    pub(crate) fn take_for(slot:&mut Option<Self>,unit:InputUnit)->Option<Self> {slot.take().filter(|inputs|inputs.unit==Some(unit))}
    pub(crate) fn declare(inputs:Inputs,declaration:Declaration)->Self {Self {declaring:Some(Box::new((inputs,declaration))),..Default::default()}}
    pub(crate) fn declaring(&self)->bool {self.declaring.is_some()}
    pub(in crate::scene) fn declare_object(&mut self,request:super::objects::CollectionJob) {
        if let Some(declaring)=self.declaring.as_deref_mut() {declaring.0.declare_object(request);}
    }
    pub(in crate::scene) fn fall_back_objects(&mut self,refusal:crate::residency::Blocked) {self.objects_refusal=Some(refusal);}
    pub(in crate::scene) fn objects_fall_back(&self)->bool {self.objects_refusal.is_some()}
    pub(in crate::scene) fn objects_refusal(&self)->Option<&crate::residency::Blocked> {self.objects_refusal.as_ref()}
    pub(crate) fn declared(self)->Result<Inputs,GpuRasterError> {
        let (inputs,declaration)=*self.declaring.ok_or(GpuRasterError::Invariant("source declaration is missing"))?;
        declaration.failure.map_or(Ok(inputs),Err)
    }
    fn placeholder(r:&WgpuRasterizer,plane:RasterPlane)->source_access::SourceRead {
        let tile=if plane==RasterPlane::Color {RawTile {texture:r.empty_texture.clone().into(),view:r.empty_view.clone()}}
            else {RawTile {texture:r.empty_scalar_texture.clone().into(),view:r.empty_scalar_view.clone()}};
        source_access::SourceRead {tile,pending:Vec::new(),assembly:None}
    }
    fn decide(&mut self,r:&WgpuRasterizer,address:Address,raw:Option<bool>,decide:impl FnOnce(&mut Inputs,&Declaration)->Result<(),GpuRasterError>)
        ->Result<[Option<source_access::SourceRead>;2],GpuRasterError> {
        let Some(declaring)=self.declaring.as_deref_mut() else {return self.reads(address);};
        let (inputs,declaration)=declaring;
        let raw=raw.unwrap_or(declaration.raw.is_some());
        let result=match inputs.decisions.0.get(&address) {
            Some((kind,_))=> {debug_assert_eq!(*kind,raw,"an accepted source decision is read by another decider");
                if *kind==raw {Ok(())}else {Err(GpuRasterError::Invariant("an accepted source decision is read by another decider"))}},
            None=>decide(inputs,declaration),
        };
        if let Err(error)=result {declaration.failure.get_or_insert_with(||error.clone());return Err(error);}
        let choices=inputs.decisions.0.get(&address).map(|(_,choices)|choices)
            .ok_or(GpuRasterError::Invariant("accepted source input is missing"))?;
        Ok(choices.each_ref().map(|choice|choice.as_ref().map(|_|Self::placeholder(r,address.1))))
    }
    fn decisions(&self)->&Decisions {self.declaring.as_ref().map_or(&self.decisions,|declaring|&declaring.0.decisions)}
    pub(in crate::scene) fn declare_watercolor_stencils(&mut self,r:&WgpuRasterizer,scene:SceneView<'_>,target:SourceTarget)->Result<(),GpuRasterError> {
        let tiles=self.decisions().0.keys().filter(|(id,plane,_)|*id==target && *plane==RasterPlane::Color).map(|(_,_,coordinate)|*coordinate).collect::<Vec<_>>();
        for coordinate in tiles.into_iter().flat_map(super::watercolor_stencil).flatten() {
            self.color(r,scene,target,coordinate)?;
            self.scalar(r,scene,target,RasterPlane::WatercolorWetness,coordinate)?;
        }
        Ok(())
    }
    pub(crate) fn witnesses(&self,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2])->Vec<crate::resident::Input> {
        self.decisions().0.get(&(target,plane,coordinate)).into_iter().flat_map(|(_,choices)|choices)
            .filter_map(|choice|match choice {Some(Selected::Resident(_,witness))=>Some(witness.clone()),_=>None}).collect()
    }
    pub(crate) fn level(&self,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2])->u32 {
        self.decisions().0.get(&(target,plane,coordinate)).and_then(|(_,[base,overlay])|overlay.as_ref().or(base.as_ref())).map_or(0,Selected::level)
    }
    pub(crate) fn color(&mut self,r:&WgpuRasterizer,scene:SceneView<'_>,target:SourceTarget,coordinate:[u32;2])->Result<[Option<source_access::SourceRead>;2],GpuRasterError> {
        self.decide(r,(target,RasterPlane::Color,coordinate),None,|inputs,declaration|match declaration.raw {
            Some(_)=>declaration.raw_color(inputs,r,target,coordinate),
            None=>inputs.color(r,scene,declaration.wanted.as_deref(),declaration.consumer.as_ref(),target,coordinate),
        })
    }
    pub(crate) fn raw_color(&mut self,r:&WgpuRasterizer,target:SourceTarget,coordinate:[u32;2])->Result<[Option<source_access::SourceRead>;2],GpuRasterError> {
        self.decide(r,(target,RasterPlane::Color,coordinate),Some(true),|inputs,declaration|declaration.raw_color(inputs,r,target,coordinate))
    }
    pub(crate) fn raw_scalar(&mut self,r:&WgpuRasterizer,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2])->Result<Option<source_access::SourceRead>,GpuRasterError> {
        self.decide(r,(target,plane,coordinate),Some(true),|inputs,declaration|inputs.raw_scalar(r,target,plane,coordinate,declaration.wanted.as_deref(),declaration.consumer.as_ref())).map(|[input,_]|input)
    }
    pub(crate) fn scalar(&mut self,r:&WgpuRasterizer,scene:SceneView<'_>,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2])->Result<Option<source_access::SourceRead>,GpuRasterError> {
        self.decide(r,(target,plane,coordinate),None,|inputs,declaration|match declaration.raw {
            Some(_)=>inputs.raw_scalar(r,target,plane,coordinate,declaration.wanted.as_deref(),declaration.consumer.as_ref()),
            None=>inputs.scalar(r,scene,declaration.wanted.as_deref(),declaration.consumer.as_ref(),target,plane,coordinate),
        }).map(|[input,_]|input)
    }
    pub(crate) fn mask(&mut self,r:&WgpuRasterizer,target:SourceTarget,source:&CoverageSource,coordinate:[u32;2])->Result<Option<source_access::SourceRead>,GpuRasterError> {
        self.decide(r,(target,RasterPlane::Mask,coordinate),Some(false),|inputs,declaration|inputs.mask(r,declaration.wanted()?,declaration.consumer.as_ref(),target,source,coordinate))
            .map(|[input,_]|input)
    }
    pub(crate) fn image(&mut self,r:&WgpuRasterizer,source:&Arc<SourceImage>,coordinate:[u32;2])->Result<source_access::SourceRead,GpuRasterError> {
        let Some(declaring)=self.declaring.as_deref_mut() else {return accepted_image(&self.leases,source,coordinate);};
        declaring.0.image(source,coordinate,r.document_color().space);
        Ok(Self::placeholder(r,RasterPlane::Color))
    }
    fn selection(&self,address:Address)->Result<&[Option<Selected>;2],GpuRasterError> {
        let selected=self.decisions.0.get(&address);
        #[cfg(test)]
        if selected.is_none() {crate::test_support::PassCounts {source_input_misses:1,..Default::default()}.record();}
        debug_assert!(selected.is_some(),"accepted input is missing");
        selected.map(|(_,choices)|choices).ok_or(GpuRasterError::Invariant("accepted source input is missing"))
    }
    fn reads(&self,address:Address)->Result<[Option<source_access::SourceRead>;2],GpuRasterError> {
        resolve(&self.leases,self.selection(address)?)
    }
    pub(crate) fn track(self,encoder:&mut crate::submission::CommandEncoder) {
        Lease::track(self.leases,encoder);
        let placements=self.placements;
        let witnesses=resident_witnesses(self.decisions.0.into_values());
        encoder.hold((placements,witnesses));
    }
    #[expect(clippy::too_many_arguments,reason="A contact refresh names its scene, target, planes, tiles and the brush's persistent writes")]
    pub(in crate::scene) fn refresh_contact(&mut self,r:&WgpuRasterizer,scene:SceneView<'_>,target:SourceTarget,planes:&[RasterPlane],tiles:std::collections::BTreeSet<[u32;2]>,
        persistent:&std::collections::BTreeSet<[u32;2]>,encoder:&mut crate::submission::CommandEncoder,
    )->bool {
        let consumer=encoder.submission();
        let mut written=Vec::new();
        let unwritten:Vec<_>=tiles.into_iter().filter(|coordinate| {
            let decided:Option<Vec<_>>=planes.iter().map(|plane|written_choices(r,scene,target,*plane,*coordinate,persistent.contains(coordinate),&consumer)
                .map(|choices|((target,*plane,*coordinate),(false,choices)))).collect();
            decided.map(|decided|written.extend(decided)).is_none()
        }).collect();
        let declared=declare(Default::default(),Declaration::current(Some(consumer)),|accepted| {
            for coordinate in &unwritten {
                #[cfg(test)] crate::test_support::PassCounts {contact_declarations:1,..Default::default()}.record();
                for plane in planes {
                    if *plane==RasterPlane::Color {accepted.color(r,scene,target,*coordinate)?;}else {accepted.scalar(r,scene,target,*plane,*coordinate)?;}
                }
            }
            Ok(())
        });
        let Some(fresh)=declared.ok().filter(Inputs::resident) else {return false;};
        let replaced=resident_witnesses(fresh.decisions.0.into_iter().chain(written).filter_map(|(address,choice)|self.decisions.0.insert(address,choice)));
        encoder.hold(replaced);
        true
    }
    #[cfg(test)]
    pub(in crate::scene) fn differing(&self,declared:&Inputs)->(usize,Vec<String>) {
        let same=|held:&[Option<Selected>;2],wanted:&[Option<Selected>;2]|held.iter().zip(wanted).all(|(a,b)|match (a,b) {(None,None)=>true,(Some(a),Some(b))=>a.matches(b),_=>false});
        (declared.decisions.0.len(),declared.decisions.0.iter().filter(|(address,(raw,wanted))|!self.decisions.0.get(address).is_some_and(|(held_raw,held)|held_raw==raw && same(held,wanted)))
            .map(|((_,plane,coordinate),_)|format!("{plane:?} {coordinate:?}")).collect())
    }
    pub(in crate::scene) fn overlay(&mut self,current:&Self) {
        for (address,choice) in &mut self.decisions.0 {if let Some(held)=current.decisions.0.get(address) {choice.clone_from(held);}}
    }
    pub(crate) fn hold<T:Send+Sync+'static>(&mut self,owner:Arc<T>) {
        let owner:Arc<dyn Send+Sync>=owner;
        if !self.leases.iter().any(|old|Arc::ptr_eq(&old.owner,&owner)) {self.leases.push(Lease {owner,_encoded:None,read:None});}
    }
    pub(in crate::scene) fn placement(&self,target:SourceTarget)->Result<Arc<super::placement::Placement>,GpuRasterError> {
        self.declaring.as_ref().map_or(&self.placements,|declaring|&declaring.0.placements).get(&target).cloned()
            .ok_or(GpuRasterError::Invariant("accepted source placement is missing"))
    }
    pub(crate) fn extend(&mut self,other:Self)->Result<(),GpuRasterError> {
        merge_unit(&mut self.unit,other.unit)?;
        if self.objects_refusal.is_none() {self.objects_refusal=other.objects_refusal;}
        self.placements.extend(other.placements)?;
        self.decisions.extend(other.decisions)?;
        for input in other.leases {
            if let Some(held)=self.leases.iter_mut().find(|held|Arc::ptr_eq(&held.owner,&input.owner)) {
                if let (Some(a),Some(b))=(&held.read,&input.read) {
                    let compatible=a.key.matches(&b.key) && a.tile.view==b.tile.view && a.tile.texture.format()==b.tile.texture.format();
                    debug_assert!(compatible,"incompatible accepted source storage");
                    if !compatible {return Err(GpuRasterError::Invariant("incompatible accepted source storage"));}
                }else if held.read.is_none() && input.read.is_some() {*held=input;}
            }else {self.leases.push(input);}
        }
        Ok(())
    }
    pub(crate) fn cpu_bytes(&self)->u64 {
        (self.leases.capacity()*std::mem::size_of::<Lease>()+self.decisions.0.len()*std::mem::size_of::<(Address,(bool,[Option<Selected>;2]))>()+self.placements.len()*std::mem::size_of::<(SourceTarget,Arc<super::placement::Placement>)>()
            +self.placements.len()*std::mem::size_of::<super::placement::Placement>()+self.iter().filter_map(|input|input.read.as_ref()).map(|read|
            std::mem::size_of::<Read>()+2*std::mem::size_of::<usize>()+read.pending.capacity()*std::mem::size_of::<Arc<PendingTile>>()).sum::<usize>()) as u64
    }
    pub(crate) fn contains(&self,inputs:&Inputs)->bool {
        inputs.tiles.iter().all(|(key,_,_,_)|self.iter().any(|input|input.read.as_ref().is_some_and(|read|read.key.matches(key))))
            && inputs.decisions.0.iter().all(|(address,(raw,choices))|self.decisions.0.get(address).is_some_and(|(kind,held)|raw==kind && choices.iter().zip(held).all(|(a,b)|match (a,b) {(None,None)=>true,(Some(a),Some(b))=>a.matches(b),_=>false})))
    }
    pub(crate) fn retain(&mut self,inputs:&Inputs) {
        self.leases.retain(|input|input.read.as_ref().is_some_and(|read|inputs.tiles.iter().any(|(key,_,_,_)|read.key.matches(key))));
        self.placements.retain(&inputs.placements);
        self.decisions.0.retain(|key,_|inputs.decisions.0.contains_key(key));
    }
    pub(crate) fn holds_image(&self,source:&Arc<SourceImage>,coordinate:[u32;2])->bool {
        let key=Key::Image(Arc::downgrade(source),coordinate);
        self.iter().filter_map(|input|input.read.as_ref()).any(|read|read.key.matches(&key))
    }
    pub(crate) fn finish_image(&mut self,source:&Arc<SourceImage>,coordinate:[u32;2]) {
        let key=Key::Image(Arc::downgrade(source),coordinate);
        self.leases.retain(|input|input.read.as_ref().is_none_or(|read|!read.key.matches(&key)));
    }
    pub(crate) fn ready_for(&self,inputs:&Inputs)->bool {
        inputs.tiles.iter().all(|(key,_,_,_)|self.iter().filter_map(|input|input.read.as_ref())
            .any(|read|read.key.matches(key) && read.pending.iter().all(|pending|pending.ready())))
    }
    pub(crate) fn encode(&self,r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,class:crate::residency::Class)->Result<bool,GpuRasterError> {
        for read in self.iter().filter_map(|lease|lease.read.as_ref()) {if !read.source().encode(r,encoder,class)? {return Ok(false);}}
        Ok(true)
    }
    pub(crate) fn prepare(&self,r:&mut WgpuRasterizer)->Result<bool,GpuRasterError> {
        if r.snapshot_worker && cfg!(not(target_arch="wasm32")) {return Ok(true);}
        let mut ready=true;
        for read in self.iter().filter_map(|input|input.read.as_ref()) {
            for pending in &read.pending {match pending.prepare(r,crate::residency::Class::Visible) {
                Ok(())=>{},Err(error) if error.is_preparation_pending()=>ready=false,Err(error)=>return Err(error),
            }}
        }
        Ok(ready && self.iter().filter_map(|input|input.read.as_ref()).all(|read|read.pending.iter().all(|pending|pending.ready())))
    }
}
struct PaintBaseCopy {
    source: RawTile,
    source_origin: [u32; 2],
    origin: [u32; 2],
    extent: [u32; 2],
}

pub(crate) struct PaintBaseAssembly {
    tile: RawTile,
    copies: Vec<PaintBaseCopy>,
    dependencies:Vec<Arc<PendingTile>>,
    _leases:Vec<Lease>,
    write: crate::submission::CacheWrite,
}

impl PaintBaseAssembly {
    pub(super) fn recorded(&self)->bool {self.write.recorded()}
    pub(super) fn sources(&self)->impl Iterator<Item=&wgpu::TextureView> {
        std::iter::once(&self.tile.view).chain(self.copies.iter().map(|copy|&copy.source.view))
    }
    pub(crate) fn encode(&self,r:&WgpuRasterizer,encoder: &mut crate::submission::CommandEncoder) {
        if self.recorded() {return;}
        { let _pass = encoder.color_pass("paint base window", &self.tile.view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)); }
        for copy in &self.copies {
            encoder.copy_texture_to_texture(
                copy.source.texture.copy_at(copy.source_origin),
                self.tile.texture.copy_at(copy.origin),
                wgpu::Extent3d {width:copy.extent[0],height:copy.extent[1],depth_or_array_layers:1},
            );
        }
        self.write.track(encoder);
        r.source_tiles.borrow_mut().finish_base(&self.tile.view);
    }

}


fn plan_paint_base(r: &WgpuRasterizer, base: &layer_core::authored::PaintBase, coordinate: [u32; 2]) -> Result<Option<source_access::SourceRead>, GpuRasterError> {
    let bounds = source_access::paint_base_bounds(base).intersect(page_rect(coordinate));
    if bounds.is_empty() { return Ok(None); }
    let mut cache = r.source_tiles.borrow_mut();
    if base.offset.iter().all(|v| v % PAGE_SIZE == 0) {
        let source_coordinate = std::array::from_fn(|i| coordinate[i] - base.offset[i] / PAGE_SIZE);
        let (tile, pending) = cache.plan_exact(r, base.image.storage(), source_coordinate)?;
        let (tile, promotion) = cache.promote(r, tile)?;
        return Ok(Some(source_access::SourceRead { tile, pending: pending.into_iter().chain(promotion).collect(), assembly: None }));
    }
    let (tile, write) = cache.plan_base(r, base, coordinate)?;
    let Some(write)=write else {
        let assembly=cache.base_pending(&tile.view);
        let pending=assembly.as_ref().map_or_else(Vec::new,|assembly|assembly.dependencies.clone());
        return Ok(Some(source_access::SourceRead {tile,pending,assembly}));
    };
    let _target=cache.lease(&tile.view).unwrap();
    let mut leases=Vec::new();
    let source_bounds = source_access::paint_base_source_bounds(base,coordinate);
    let mut pending = Vec::with_capacity(4);
    let mut copies = Vec::with_capacity(4);
    for source_coordinate in page_coordinates(source_bounds) {
        let overlap = source_bounds.intersect(page_rect(source_coordinate));
        let (source, decode) = cache.plan_exact(r, base.image.storage(), source_coordinate)?;
        let (source, promotion) = cache.promote(r, source)?;
        leases.push(cache.lease(&source.view).unwrap());
        pending.extend(decode.into_iter().chain(promotion));
        copies.push(PaintBaseCopy {
            source, source_origin: [overlap.min_x() % PAGE_SIZE, overlap.min_y() % PAGE_SIZE],
            origin: [overlap.min_x() + base.offset[0] - coordinate[0] * PAGE_SIZE, overlap.min_y() + base.offset[1] - coordinate[1] * PAGE_SIZE],
            extent: [overlap.width(),overlap.height()],
        });
    }
    let assembly=Arc::new(PaintBaseAssembly {tile:RawTile {texture:tile.texture.clone(),view:tile.view.clone()},copies,write,dependencies:pending.clone(),_leases:leases});
    cache.remember_base(&tile.view,assembly.clone());
    Ok(Some(source_access::SourceRead {tile,pending,assembly:Some(assembly)}))
}

struct Read {key:Key,tile:RawTile,pending:Vec<Arc<PendingTile>>,assembly:Option<Arc<PaintBaseAssembly>>}
impl Read {
    fn source(&self)->source_access::SourceRead {
        source_access::SourceRead {tile:RawTile {texture:self.tile.texture.clone(),view:self.tile.view.clone()},pending:self.pending.clone(),assembly:self.assembly.clone()}
    }
}
impl std::ops::Deref for Lease {type Target=Arc<dyn Send+Sync>;fn deref(&self)->&Self::Target {&self.owner}}
impl From<Arc<()>> for Lease {fn from(owner:Arc<()>)->Self {Self {owner,_encoded:None,read:None}}}
impl Lease {
    pub(crate) fn track(leases:Vec<Self>,encoder:&mut crate::submission::CommandEncoder) {
        let pins=leases.into_iter().map(|lease|(lease.owner,lease._encoded)).collect::<Vec<_>>();
        encoder.hold(pins);
    }
}
struct Slot {
    charge: crate::residency::Charge,
    key: Option<Key>,
    layer: Layer,
    view: wgpu::TextureView,
    created: u64,
    valid: Arc<std::sync::atomic::AtomicBool>,
    lease: Arc<crate::residency::Lease>,
    pin: std::sync::Mutex<Weak<SlotPin>>,
    encoded:Option<std::sync::Mutex<Weak<Pin>>>,
    samples: Option<Samples>,
    pending: Option<Arc<PendingTile>>,
    assembly: Option<Arc<PaintBaseAssembly>>,
    used: std::sync::atomic::AtomicU64,
}
struct SlotPin(std::sync::Mutex<crate::residency::Charge>);
struct Layers { texture: wgpu::Texture, free: std::sync::Mutex<Option<Vec<u32>>>, pass: std::sync::atomic::AtomicU64 }
impl Layers {
    fn free(&self) -> Option<usize> { self.free.lock().unwrap().as_ref().map(Vec::len).filter(|free| *free != 0) }
}
struct Layer { array: Arc<Layers>, index: u32 }
impl Layer {
    fn texture(&self) -> TileTexture { TileTexture::layer_of(self.array.texture.clone(), self.index) }
    fn format(&self) -> wgpu::TextureFormat { self.array.texture.format() }
    fn take(array: &Arc<Layers>) -> Option<Self> {
        let index = array.free.lock().unwrap().as_mut()?.pop()?;
        Some(Self { array: array.clone(), index })
    }
}
impl Drop for Layer {
    fn drop(&mut self) {
        let mut free = self.array.free.lock().unwrap();
        let Some(layers) = free.as_mut() else { return; };
        layers.push(self.index);
        if layers.len() as u32 == self.array.texture.depth_or_array_layers() { *free = None; self.array.texture.destroy(); }
    }
}
struct RetiredSlot { _layer:Layer, encoded:Weak<Pin>, _charge:[crate::residency::Charge;2], completed:Arc<std::sync::atomic::AtomicBool> }
impl RetiredSlot {
    fn ready(&self)->bool {self.completed.load(Ordering::Acquire) && self.encoded.strong_count()==0}
}
impl Slot {
    fn touch(&self,epoch:u64) {self.used.store(epoch,Ordering::Relaxed);}
    fn leased(&self)->bool {self.pin.lock().unwrap().strong_count()!=0}
    fn bytes(&self)->u64 {self.charge.bytes()}
    fn offer(&self) {
        let value=if self.key.is_some() {crate::residency::rebuild_value(super::object_cache::PASS_TAPS,self.bytes())}else {0};
        self.lease.offer(self.bytes(),value);
    }
    fn guard(&self) {
        if let Some(pin)=self.pin.lock().unwrap().upgrade() {let guard:Arc<dyn Send+Sync>=pin;self.lease.guard(Arc::downgrade(&guard));}
    }
    fn ready(&self)->bool {self.valid.load(Ordering::Acquire) && self.pending.as_ref().is_none_or(|pending|pending.recorded())
        && self.assembly.as_ref().is_none_or(|assembly|assembly.recorded())}
}
struct RetirementCompletion(Arc<std::sync::atomic::AtomicBool>,Arc<dyn Fn()+Send+Sync>);
#[cfg(not(target_arch="wasm32"))]
type SourceRetirement=crate::device_poll::Retirement<RetirementCompletion>;
#[cfg(target_arch="wasm32")]
type SourceRetirement=RetirementCompletion;
impl Drop for RetirementCompletion {
    fn drop(&mut self) {self.0.store(true,Ordering::Release);(self.1)();}
}
#[derive(Clone)]
enum Pixels {
    Image(Arc<SourceImage>, [u32; 2]),
    Raster(Arc<TileBlob>, RgbSpace,RgbSpace),
    Resident,
}
#[derive(Clone)]
pub enum PreparedSourcePixels { NativeSamples(Arc<Vec<u8>>), PremultipliedWorkingPixels(Arc<Vec<u8>>) }

pub(crate) struct PendingTile {
    pixels: Pixels,
    pub texture: TileTexture,
    pub(super) view: wgpu::TextureView,
    pub data: Option<SourceColor>,
    write: crate::submission::CacheWrite,
    receipt: std::sync::Mutex<Option<PreparedSourcePixels>>,
    input: Option<(TileTexture, RgbSpace, std::sync::Mutex<Option<SourceRetirement>>)>,
    dependency: Option<Arc<PendingTile>>,
}
impl PendingTile {
    pub fn prepared(&self)->Option<PreparedSourcePixels> {self.receipt.lock().unwrap().clone()}
    pub fn set_prepared(&self,pixels:Option<PreparedSourcePixels>) {
        if let Some(input)=self.dependency() {input.set_prepared(pixels);}else {*self.receipt.lock().unwrap()=pixels;}
    }
    pub fn recorded(&self)->bool {self.write.recorded()}
    pub(super) fn dependency(&self)->Option<&Arc<Self>> {self.dependency.as_ref().filter(|input|!input.recorded())}
    pub fn ready(&self) -> bool {self.dependency().is_none_or(|input|input.ready()) && (self.input.is_some() || self.prepared().is_some())}
    pub(super) fn validate_prepared(&self)->Result<(),GpuRasterError> {
        if self.input.is_some() {return Ok(());}
        if self.data.is_some() {
            let samples=self.pixels.native()?;
            if samples.tile.descriptor!=samples.descriptor {return Err(GpuRasterError::Color("Source tile has the wrong sample representation".into()));}
            match self.prepared() {
                Some(PreparedSourcePixels::NativeSamples(bytes)) if bytes.len()!=(PAGE_SIZE*PAGE_SIZE) as usize*4*samples.depth.bytes()=>
                    return Err(GpuRasterError::Color("Prepared source samples have the wrong size".into())),
                Some(PreparedSourcePixels::PremultipliedWorkingPixels(_))=>return Err(GpuRasterError::Color("Working source pixels have native decode settings".into())),
                _=>{},
            }
        } else {
            match self.prepared() {
                Some(PreparedSourcePixels::PremultipliedWorkingPixels(bytes)) if bytes.len() as u64!=NORMALIZED_PAGE_BYTES=>return Err(GpuRasterError::InvalidExtent),
                Some(PreparedSourcePixels::NativeSamples(_))=>return Err(GpuRasterError::Color("Native source samples require decode settings".into())),
                _=>{},
            }
        }
        Ok(())
    }
    fn input_format(&self)->Option<wgpu::TextureFormat> {
        let color=self.data?;
        if self.texture.format()!=wgpu::TextureFormat::Rgba32Float {return None;}
        Some(color.input_format())
    }
    pub(crate) fn normalization_parameters(&self)->Option<SourceColor> {
        self.data.map(|mut color| {
            if self.input.is_none() && let Some(format)=self.input_format() {color.representation=Representation::for_format(format);}
            color
        })
    }
    pub(crate) fn pipeline<'a>(&self, pipelines: &'a Pipelines) -> Option<&'a Deferred<wgpu::RenderPipeline>> {
        if let Some((input, ..)) = &self.input { Some(pipelines.for_format(input.format())) }
        else if let Some(format)=self.input_format() {Some(pipelines.for_format(format))}
        else { None }
    }
    pub fn matches_image(&self,source:&Arc<SourceImage>,coordinate:[u32;2])->bool {matches!(&self.pixels,Pixels::Image(stored,c) if Arc::ptr_eq(stored,source) && *c==coordinate)}
    pub fn matches_raster(&self,blob:&Arc<TileBlob>,space:RgbSpace,destination:RgbSpace)->bool {matches!(&self.pixels,Pixels::Raster(stored,from,to) if Arc::ptr_eq(stored,blob) && *from==space && *to==destination)}
    pub fn accept_image(&self,source:&Arc<SourceImage>,coordinate:[u32;2],pixels:&PreparedSourcePixels)->bool {
        if !self.matches_image(source,coordinate) {return false;}
        *self.receipt.lock().unwrap()=Some(pixels.clone());true
    }
    pub fn accept_raster(&self,blob:&Arc<TileBlob>,space:RgbSpace,destination:RgbSpace,pixels:&PreparedSourcePixels)->bool {
        if !self.matches_raster(blob,space,destination) {return false;}
        *self.receipt.lock().unwrap()=Some(pixels.clone());true
    }
    pub fn prepare(&self,r:&mut WgpuRasterizer,class:crate::residency::Class)->Result<(),GpuRasterError> {
        if self.input.is_some() { return Ok(()); }
        if self.prepared().is_some() {return Ok(());}
        match &self.pixels {
            Pixels::Image(source,coordinate)=> {
                let id=source.tiles.get(coordinate).ok_or(GpuRasterError::InvalidImage)?.resource_id();
                if self.texture.format()==wgpu::TextureFormat::Rgba32Float {r.request_exact_image_decode(id,source.clone(),*coordinate,class)?;}
                else {r.request_image_decode(id,source.clone(),*coordinate,class)?;}
            },
            Pixels::Raster(blob,space,destination)=> {r.request_raster_decode_to(blob.clone(),*space,*destination,class)?;},
            Pixels::Resident => unreachable!(),
        }
        Ok(())
    }
}
struct NativeSamples<'a> {
    tile: &'a Arc<TileBlob>,
    descriptor: PixelDescriptor,
    channels: SourceChannels,
    depth: SampleDepth,
    space: RgbSpace,
}
impl Pixels {
    fn native(&self) -> Result<NativeSamples<'_>, GpuRasterError> {
        match self {
            Self::Image(source, coordinate) => {
                let interpretation = &source.interpretation;
                let ColorProfile::Builtin(space) = interpretation.profile else {
                    unreachable!()
                };
                let tile = source
                    .tiles
                    .get(coordinate)
                    .ok_or_else(|| GpuRasterError::Color("Missing source tile".into()))?;
                Ok(NativeSamples {
                    tile,
                    descriptor: interpretation.descriptor(),
                    channels: interpretation.channels,
                    depth: interpretation.depth,
                    space,
                })
            }
            Self::Raster(tile, space,_) => Ok(NativeSamples {
                tile,
                descriptor: tile.descriptor,
                channels: match tile.descriptor.channels { 1 => SourceChannels::Gray, 2 => SourceChannels::GrayAlpha, _ => SourceChannels::Rgba },
                depth: tile.descriptor.depth(),
                space: *space,
            }),
            Self::Resident => unreachable!(),
        }
    }
}
struct EncodedInput {
    texture: wgpu::Texture,
}
fn page_bytes(format:wgpu::TextureFormat)->u64 {u64::from(PAGE_SIZE).pow(2)*u64::from(format.block_copy_size(None).unwrap())}
#[cfg(target_arch="wasm32")]
struct WebRetirement {
    send:futures_channel::mpsc::UnboundedSender<RetiredSlot>,
    wake:Arc<futures_util::task::AtomicWaker>,
}
#[cfg(target_arch="wasm32")]
impl WebRetirement {
    fn new()->Self {
        use futures_util::{Stream,stream::FusedStream};
        let (send,mut receive)=futures_channel::mpsc::unbounded::<RetiredSlot>();
        let wake=Arc::new(futures_util::task::AtomicWaker::new());let observed=wake.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let mut pending=Vec::<RetiredSlot>::new();
            std::future::poll_fn(move|context| {
                observed.register(context.waker());
                while !receive.is_terminated() {match std::pin::Pin::new(&mut receive).poll_next(context) {
                    std::task::Poll::Ready(Some(value))=>pending.push(value),_=>break,
                }}
                pending.retain(|value|!value.ready());
                if receive.is_terminated() && pending.is_empty() {std::task::Poll::Ready(())}else {std::task::Poll::Pending}
            }).await;
        });
        Self {send,wake}
    }
    fn release_wake(&self)->Arc<dyn Fn()+Send+Sync> {let wake=self.wake.clone();Arc::new(move||wake.wake())}
}
pub(crate) struct DecodedTiles {
    destination: RgbSpace,
    slots: Vec<Slot>,
    arrays: Vec<Arc<Layers>>,
    views:std::collections::HashMap<wgpu::TextureView,usize>,
    keys:std::collections::HashMap<Key,usize>,
    clock: u64,
    decoders: VecDeque<(Weak<SourceImage>, layer_color::WorkingDecoder)>,
    pixels: Vec<[f32; 4]>,
    inputs: Vec<EncodedInput>,
    transfer: transfer::Tables,
    decoded: Arc<crate::residency::Tier>,
    input_bytes: crate::residency::Charge,
    table_bytes: crate::residency::Charge,
    evicted: Arc<std::sync::atomic::AtomicBool>,
    passes: std::sync::atomic::AtomicU64,
    in_flight: Arc<crate::residency::Tier>,
    leased: Arc<crate::residency::Tier>,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    retired_bytes: Arc<crate::residency::Tier>,
    retiring_bytes: Option<Arc<crate::residency::Tier>>,
    #[cfg(not(target_arch="wasm32"))]
    retirement:Option<crate::device_poll::RetirementHandle>,
    #[cfg(target_arch="wasm32")]
    retirement:Option<WebRetirement>,
    retiring: Vec<SourceRetirement>,
}
impl DecodedTiles {
    fn recharge(slot:&Slot) {
        if let Some(pin)=slot.pin.lock().unwrap().upgrade() {pin.0.lock().unwrap().resize(slot.bytes());}
    }
    fn keyed(&self,key:&Key)->Option<&Slot> {self.keys.get(key).map(|index|&self.slots[*index])}
    fn viewed(&mut self,view:&wgpu::TextureView)->Option<&mut Slot> {self.views.get(view).map(|index|&mut self.slots[*index])}
    fn set_key(&mut self,index:usize,key:Option<Key>) {
        if let Some(old)=self.slots[index].key.take() {self.keys.remove(&old);}
        if let Some(key)=&key {self.keys.insert(key.clone(),index);}
        let lease=self.decoded.heap().lease(&self.evicted);
        let slot=&mut self.slots[index];
        slot.key=key;slot.lease=lease;slot.guard();slot.offer();
    }
    fn stamp(&self,view:&wgpu::TextureView,class:crate::residency::Class) {
        if let Some(index)=self.views.get(view) {self.slots[*index].lease.stamp(class);}
    }
    fn remove_slot(&mut self,index:usize)->Slot {
        let slot=self.slots.swap_remove(index);self.views.remove(&slot.view);
        if let Some(key)=&slot.key {self.keys.remove(key);}
        if let Some(moved)=self.slots.get(index) {
            self.views.insert(moved.view.clone(),index);
            if let Some(key)=&moved.key {self.keys.insert(key.clone(),index);}
        }
        slot
    }
    pub fn within(display: &Arc<crate::residency::Tier>, destination: RgbSpace) -> Self {
        let decoded=crate::residency::Tier::within(display);
        Self {
            destination, slots: Vec::new(), arrays: Vec::new(), views: Default::default(), keys: Default::default(), clock: 0, decoders: VecDeque::new(), pixels: Vec::new(),
            inputs: Vec::new(), transfer: Default::default(), input_bytes: decoded.charge(0), table_bytes: decoded.charge(0), evicted: Default::default(), passes: Default::default(),
            in_flight: Default::default(), leased: Default::default(), hits: 0, misses: 0, evictions: 0,
            retired_bytes: crate::residency::Tier::within(&decoded), retiring_bytes: None, decoded, retirement: None, retiring: Vec::new(),
        }
    }
    pub fn admit_limits(&mut self, upload_bytes: u64) {
        self.in_flight.set_limit(upload_bytes, upload_bytes / 2);
    }
    pub fn upload_limit(&self) -> u64 { self.in_flight.limit() }
    pub(crate) fn demand_images(&self, tiles: impl IntoIterator<Item = RequiredSource>) {
        let demanded = tiles.into_iter().filter_map(|(source, coordinate)| self.keys.get(&Key::Image(Arc::downgrade(&source), coordinate)).copied()).collect::<Vec<_>>();
        let epoch = self.decoded.heap().epoch();
        for index in demanded.into_iter().rev() {
            let slot = &self.slots[index];
            slot.touch(epoch);slot.offer();
            slot.lease.stamp(crate::residency::Class::Prefetch);
        }
    }
    pub(crate) fn release_unused(&self, keep: u64) {
        let epoch = self.decoded.heap().epoch();
        let unused = |slot: &&Slot| slot.used.load(Ordering::Relaxed).saturating_add(1) < epoch && slot.ready() && !slot.leased();
        let begin = || {
            let pass = self.passes.fetch_add(2, Ordering::Relaxed) + 2;
            for slot in self.slots.iter().filter(|slot| !unused(slot)) { slot.layer.array.pass.store(pass, Ordering::Relaxed); }
            move |slot: &Slot| if slot.layer.array.pass.swap(pass + 1, Ordering::Relaxed) >= pass { 0 } else { crate::texture_bytes(&slot.layer.array.texture) }
        };
        let cost = begin();
        if self.slots.iter().filter(unused).map(&cost).sum::<u64>() <= keep { return; }
        let mut unused = self.slots.iter().filter(unused).collect::<Vec<_>>();
        unused.sort_by_key(|slot| std::cmp::Reverse(slot.used.load(Ordering::Relaxed)));
        let (cost, mut kept) = (begin(), 0u64);
        for slot in unused {
            kept = kept.saturating_add(cost(slot));
            if kept > keep { slot.lease.evict(); }
        }
    }
    pub(crate) fn retire_evicted(&mut self) -> u64 {
        if !self.evicted.swap(false, Ordering::AcqRel) { return 0; }
        let (mut bytes, mut index) = (0, 0);
        while index < self.slots.len() {
            let slot = &self.slots[index];
            if !slot.lease.evicted() { index += 1; continue; }
            if slot.leased() { slot.lease.restore(); slot.offer(); index += 1; continue; }
            let slot = self.remove_slot(index);
            bytes += slot.bytes();
            self.evictions += u64::from(slot.valid.load(Ordering::Acquire));
            let completion = self.retire(slot);
            self.retiring.push(completion);
        }
        bytes
    }
    pub(crate) fn admit_upload(&self, class: crate::residency::Class, bytes: u64) -> Result<crate::residency::Reservation, crate::residency::Blocked> {
        self.in_flight.admit(class, bytes)
    }
    pub(crate) fn upload_blocked(&self, class: crate::residency::Class, bytes: u64) -> Option<crate::residency::Blocked> {
        self.in_flight.blocked(class, bytes)
    }
    fn slot_lease(&self,slot:&Slot)->Lease {
        let encoded=slot.encoded.as_ref().map(|weak| {
            let mut weak=weak.lock().unwrap();
            weak.upgrade().unwrap_or_else(|| {
                let pin=Pin::new(self.retirement.as_ref().unwrap().release_wake());
                *weak=Arc::downgrade(&pin);pin
            })
        });
        let mut lease=slot.pin.lock().unwrap();
        let owner=lease.upgrade().unwrap_or_else(|| {
            let pin=Arc::new(SlotPin(std::sync::Mutex::new(self.leased.charge(slot.bytes()))));
            let guard:Arc<dyn Send+Sync>=pin.clone();slot.lease.guard(Arc::downgrade(&guard));
            *lease=Arc::downgrade(&pin);pin
        });
        Lease {owner,_encoded:encoded,read:None}
    }
    fn retire(&mut self,slot:Slot)->SourceRetirement {
        let completed=Arc::new(std::sync::atomic::AtomicBool::new(false));
        let value=RetiredSlot {_charge:[self.retired_bytes.charge(slot.bytes()),self.retiring_bytes.as_ref().unwrap().charge(slot.bytes())],_layer:slot.layer,encoded:slot.encoded.map(|weak|weak.into_inner().unwrap()).unwrap_or_default(),completed:completed.clone()};
        let owner=self.retirement.as_ref().unwrap();
        let completion=RetirementCompletion(completed,owner.release_wake());
        #[cfg(not(target_arch="wasm32"))]
        owner.dispose_when(value,RetiredSlot::ready);
        #[cfg(target_arch="wasm32")]
        owner.send.unbounded_send(value).unwrap_or_else(|_|unreachable!("source retirement receiver remains alive"));
        #[cfg(not(target_arch="wasm32"))]
        let completion=owner.retirement(completion);
        completion
    }
    pub(crate) fn flush_retired(&mut self, encoder: &mut crate::submission::CommandEncoder) {
        if self.retiring.is_empty() { return; }
        encoder.retain_resources();
        for completion in self.retiring.drain(..) { encoder.on_submitted_work_done(move || {
            #[cfg(not(target_arch="wasm32"))]
            completion.complete();
            #[cfg(target_arch="wasm32")]
            drop(completion);
        }); }
    }
    pub fn prepare_transfer(
        &mut self,
        device: &PipelineDevice,
        space: RgbSpace,
    ) -> Result<crate::native_tiles::NativeTransfer, GpuRasterError> {
        let transfer = self.transfer.prepare(device, space)?.clone();
        self.table_bytes.resize(self.transfer.gpu_bytes());
        Ok(transfer)
    }
    pub(crate) fn invalidate(&mut self) {for slot in &self.slots {slot.valid.store(false,Ordering::Release);}}
    pub(crate) fn free_slots(&self) -> usize {
        let evictable = self.decoded.used().saturating_sub(self.pinned_bytes()).saturating_sub(self.non_slot_bytes());
        (self.decoded.headroom(crate::residency::Class::Visible).saturating_add(evictable) / NORMALIZED_PAGE_BYTES) as usize
    }
    #[cfg(test)]
    pub(crate) fn allocated_slots(&self)->usize {self.slots.len()}
    #[cfg(test)]
    pub(crate) fn leased_bytes(&self)->u64 {self.leased.used()}
    pub fn prepared_view(
        &self,
        source: &Arc<SourceImage>,
        coordinate: [u32; 2],
    ) -> Option<&wgpu::TextureView> {
        self.prepared_slot(&Key::Image(Arc::downgrade(source), coordinate))
    }
    pub fn prepared_base_view(&self, base: &layer_core::authored::PaintBase, coordinate: [u32; 2]) -> Option<&wgpu::TextureView> {
        if !crate::source_access::paint_base_contains(base, coordinate) { return None; }
        if base.offset.iter().all(|v| v % PAGE_SIZE == 0) {
            return self.prepared_view(base.image.storage(), std::array::from_fn(|i| coordinate[i] - base.offset[i] / PAGE_SIZE))
                .filter(|view| view.texture().format() == wgpu::TextureFormat::Rgba32Float);
        }
        self.prepared_slot(&Key::PaintBase(Arc::downgrade(base.image.storage()), base.offset, coordinate))
    }
    fn prepared_slot(&self, key: &Key) -> Option<&wgpu::TextureView> {
        let slot = self.keyed(key).filter(|slot| slot.ready())?;
        slot.touch(self.decoded.heap().epoch());
        Some(&slot.view)
    }
    fn plan_base(&mut self, r: &WgpuRasterizer, base: &layer_core::authored::PaintBase, coordinate: [u32; 2]) -> Result<(RawTile, Option<crate::submission::CacheWrite>), GpuRasterError> {
        self.plan_key(r, Key::PaintBase(Arc::downgrade(base.image.storage()), base.offset, coordinate))
    }
    pub(crate) fn uploads_full(&self, class: crate::residency::Class) -> bool {
        self.upload_blocked(class, NORMALIZED_PAGE_BYTES).is_some()
    }
    pub fn prepared_raster_view(&self, blob: &Arc<TileBlob>, space: RgbSpace,destination:RgbSpace) -> Option<&wgpu::TextureView> {
        self.prepared_raster_any_view(blob,space,destination).filter(|view|view.texture().format()==wgpu::TextureFormat::Rgba32Float)
    }
    pub(crate) fn prepared_raster_any_view(&self, blob: &Arc<TileBlob>, space: RgbSpace,destination:RgbSpace) -> Option<&wgpu::TextureView> {
        self.prepared_slot(&Key::raster(blob, space, destination))
    }

    pub(crate) fn charge_upload(&self, encoder: &crate::submission::CommandEncoder, reservation: crate::residency::Reservation, bytes: u64) -> u64 {
        let charge = reservation.charge(bytes);
        encoder.hold(charge);
        self.in_flight.used()
    }
    pub(crate) fn non_slot_bytes(&self) -> u64 { self.input_bytes.bytes() + self.table_bytes.bytes() }
    pub fn gpu_bytes(&self) -> u64 { self.decoded.used() }
    pub fn cpu_bytes(&self) -> u64 {
        self.pixels.capacity() as u64 * std::mem::size_of::<[f32; 4]>() as u64
            + self.slots.capacity() as u64 * std::mem::size_of::<Slot>() as u64
            + self.views.capacity() as u64 * std::mem::size_of::<(wgpu::TextureView,usize)>() as u64
            + self.keys.capacity() as u64 * std::mem::size_of::<(Key,usize)>() as u64
            + self.inputs.capacity() as u64 * std::mem::size_of::<EncodedInput>() as u64
            + self.decoders.capacity() as u64 * std::mem::size_of::<(Weak<SourceImage>, layer_color::WorkingDecoder)>() as u64
    }
    pub fn pinned_bytes(&self) -> u64 {
        self.leased.used() + self.retired_bytes.used()
    }

    #[cfg(test)]
    pub(in crate::scene) fn plan(
        &mut self,
        r: &WgpuRasterizer,
        source: &Arc<SourceImage>,
        coordinate: [u32; 2],
    ) -> Result<(RawTile, Option<Arc<PendingTile>>), GpuRasterError> {
        self.plan_image(r,source,coordinate,SourceUse::Display)
    }
    pub(in crate::scene) fn plan_exact(&mut self,r:&WgpuRasterizer,source:&Arc<SourceImage>,coordinate:[u32;2])->Result<(RawTile,Option<Arc<PendingTile>>),GpuRasterError> {
        self.plan_image(r,source,coordinate,SourceUse::Exact)
    }
    #[cfg(test)]
    pub(crate) fn fixture_image_plan(&mut self,r:&WgpuRasterizer,source:&Arc<SourceImage>,coordinate:[u32;2],use_:SourceUse)
        ->Result<(RawTile,Option<Arc<PendingTile>>),GpuRasterError> {
        self.plan_image(r,source,coordinate,use_)
    }
    fn plan_image(&mut self,r:&WgpuRasterizer,source:&Arc<SourceImage>,coordinate:[u32;2],use_:SourceUse)->Result<(RawTile,Option<Arc<PendingTile>>),GpuRasterError> {
        let (tile,write)=self.plan_key_for(r,Key::Image(Arc::downgrade(source),coordinate),use_)?;
        let pending = write.map(|write| PendingTile {
            pixels: Pixels::Image(source.clone(), coordinate),
            texture: tile.texture.clone(),
            view: tile.view.clone(),
            data: builtin_settings(source, coordinate, self.destination),
            write,
            receipt:Default::default(),input:None,dependency:None,
        });
        let pending=self.pending(&tile.view,pending);
        Ok((tile, pending))
    }

    pub(super) fn plan_raster(
        &mut self,
        r: &WgpuRasterizer,
        blob: &Arc<TileBlob>,
        space: RgbSpace,
        destination: RgbSpace,
    ) -> Result<(RawTile, Option<Arc<PendingTile>>), GpuRasterError> {
        validate_raster(blob, space)?;
        let d = blob.descriptor;
        let depth = d.depth();
        let mut data = rgb_settings(space, destination, depth, [PAGE_SIZE; 2], d.alpha);
        data.scalar = u32::from(d.channels == 1);
        data.scalar_scale = if d.channels == 1 { 1. / depth.maximum() as f32 } else { 0. };
        let (tile, write) =
            self.plan_key_for(r,Key::raster(blob,space,destination),SourceUse::Exact)?;
        let pending = write.map(|write| PendingTile {
            pixels: Pixels::Raster(blob.clone(), space,destination),
            texture: tile.texture.clone(),
            view: tile.view.clone(),
            data: Some(data),
            write,
            receipt:Default::default(),input:None,dependency:None,
        });
        let pending=self.pending(&tile.view,pending);
        Ok((tile, pending))
    }

    fn pending(&mut self,view:&wgpu::TextureView,pending:Option<PendingTile>)->Option<Arc<PendingTile>> {
        let slot=self.viewed(view).unwrap();
        if let Some(pending)=pending {slot.pending=Some(Arc::new(pending));}
        slot.pending.as_ref().filter(|pending|!pending.recorded()).cloned()
    }
    fn base_pending(&self,view:&wgpu::TextureView)->Option<Arc<PaintBaseAssembly>> {
        self.slots[*self.views.get(view)?].assembly.as_ref().filter(|assembly|!assembly.recorded()).cloned()
    }
    fn remember_base(&mut self,view:&wgpu::TextureView,assembly:Arc<PaintBaseAssembly>) {
        self.viewed(view).unwrap().assembly=Some(assembly);
    }
    pub(crate) fn finish_base(&mut self,view:&wgpu::TextureView) {
        if let Some(slot)=self.viewed(view) {slot.assembly=None;}
    }
    pub fn batch_checkpoint(&self) -> u64 { self.clock }
    pub fn release_batch(&mut self, checkpoint:u64) {
        for index in 0..self.slots.len() {
            if self.slots[index].created<=checkpoint {continue;}
            self.set_key(index,None);
            let slot=&mut self.slots[index];
            slot.valid=Arc::new(std::sync::atomic::AtomicBool::new(false));
            slot.created=0;
        }
    }
    pub fn lease(&self, view: &wgpu::TextureView) -> Option<Lease> {
        self.views.get(view).map(|index|self.slot_lease(&self.slots[*index]))
    }

    fn layer(&mut self, r: &WgpuRasterizer, format: wgpu::TextureFormat) -> (Layer, wgpu::TextureView) {
        self.arrays.retain(|array| array.free.lock().unwrap().is_some());
        let layer = self.arrays.iter().filter(|array| array.texture.format() == format)
            .filter_map(|array| Some((array.free()?, array))).min_by_key(|(free, _)| *free).and_then(|(_, array)| Layer::take(array))
            .unwrap_or_else(|| {
                let layers = (ARRAY_BYTES / page_bytes(format)).clamp(1, u64::from(r.device.limits().max_texture_array_layers)) as u32;
                let array = Arc::new(Layers { texture: Self::texture(r, format, layers), free: std::sync::Mutex::new(Some((0..layers).rev().collect())), pass: Default::default() });
                self.arrays.push(array.clone());
                Layer::take(&array).unwrap()
            });
        let view = layer.texture().create_view(&Default::default());
        (layer, view)
    }
    fn texture(r: &WgpuRasterizer, format: wgpu::TextureFormat, layers: u32) -> wgpu::Texture {
        r.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bounded source tile"), size: wgpu::Extent3d { width: PAGE_SIZE, height: PAGE_SIZE, depth_or_array_layers: layers },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC
                | if format == wgpu::TextureFormat::Rgba32Float { wgpu::TextureUsages::RENDER_ATTACHMENT } else { wgpu::TextureUsages::empty() },
            view_formats: if format == wgpu::TextureFormat::Rgba8Unorm { &[wgpu::TextureFormat::Rgba8UnormSrgb] } else { &[] },
        })
    }
    fn allocate_slot(&mut self, r: &WgpuRasterizer, format: wgpu::TextureFormat, samples: Option<Samples>) -> usize {
        self.retirement.get_or_insert_with(|| {
            #[cfg(not(target_arch="wasm32"))]
            {r.device.retirement_handle()}
            #[cfg(target_arch="wasm32")]
            {WebRetirement::new()}
        });
        self.retiring_bytes.get_or_insert_with(|| r.scene_tiers.retiring.clone());
        let (layer, view) = self.layer(r, format);
        self.views.insert(view.clone(),self.slots.len());
        self.slots.push(Slot { charge:self.decoded.charge(page_bytes(format)),key: None, layer, view, created: 0,
            valid: Arc::new(std::sync::atomic::AtomicBool::new(false)), lease: self.decoded.heap().lease(&self.evicted), pin: Default::default(),
            encoded:(format!=wgpu::TextureFormat::Rgba32Float).then(Default::default), samples,pending:None,assembly:None,used:Default::default() });
        self.slots.len() - 1
    }
    fn plan_key(&mut self, r: &WgpuRasterizer, key: Key) -> Result<(RawTile, Option<crate::submission::CacheWrite>), GpuRasterError> {
        self.plan_key_for(r,key,SourceUse::Display)
    }
    fn plan_key_for(&mut self,r:&WgpuRasterizer,key:Key,use_:SourceUse)->Result<(RawTile,Option<crate::submission::CacheWrite>),GpuRasterError> {
        #[cfg(test)]
        if ACCEPTED_READ_RESULTS.with(|results|results.borrow().is_some()) {READS.with(|reads|reads.borrow_mut().push(key.clone()));}
        self.clock = self.clock.wrapping_add(1);
        let epoch = self.decoded.heap().epoch();
        if let Some(&index) = self.keys.get(&key) {
            let slot = &mut self.slots[index];
            if slot.valid.load(Ordering::Acquire) {
                self.hits += 1;slot.touch(epoch);slot.lease.restore();slot.offer();
                return Ok((RawTile { texture: slot.layer.texture(), view: slot.view.clone() }, None));
            }
            self.set_key(index,None);
        }
        let mut samples=key.samples(self.destination);
        let format=if use_==SourceUse::Exact {samples=None;wgpu::TextureFormat::Rgba32Float}else{samples.map_or(wgpu::TextureFormat::Rgba32Float,Samples::format)};
        let unused = self.slots.iter().position(|slot| !slot.leased() && !slot.valid.load(Ordering::Acquire) && slot.layer.format() == format);
        let index = unused.unwrap_or_else(|| self.allocate_slot(r, format, samples));
        self.set_key(index,Some(key));
        let slot = &mut self.slots[index];
        slot.pending=None;slot.assembly=None;slot.touch(epoch);slot.samples = samples; slot.created = self.clock;
        let write = crate::submission::CacheWrite::new(); slot.valid = write.validity();
        Ok((RawTile { texture: slot.layer.texture(), view: slot.view.clone() }, Some(write)))
    }
    pub(crate) fn promote(&mut self, r: &WgpuRasterizer, tile: RawTile) -> Result<(RawTile, Option<Arc<PendingTile>>), GpuRasterError> {
        if tile.texture.format() == wgpu::TextureFormat::Rgba32Float { return Ok((tile, None)); }
        let index = *self.views.get(&tile.view).ok_or(GpuRasterError::InvalidImage)?;
        let samples = self.slots[index].samples.ok_or(GpuRasterError::InvalidImage)?;
        self.prepare_transfer(&r.device,samples.space)?;
        let (layer,view)=self.layer(r,wgpu::TextureFormat::Rgba32Float);
        let texture=layer.texture();
        let write = crate::submission::CacheWrite::new();
        let charge=self.decoded.charge(NORMALIZED_PAGE_BYTES);
        let slot = &mut self.slots[index];
        self.views.remove(&slot.view);self.views.insert(view.clone(),index);
        let pin=std::mem::take(&mut slot.pin);
        let replacement = Slot { charge,key: slot.key.take(), layer, view: view.clone(),
            created: slot.created, valid: write.validity(), lease: slot.lease.clone(), pin,encoded:None, samples: None, pending:None,assembly:None,used:std::sync::atomic::AtomicU64::new(slot.used.load(Ordering::Relaxed)) };
        let old = std::mem::replace(slot, replacement);
        Self::recharge(slot);slot.offer();
        let dependency=old.pending.clone();
        let completion = self.retire(old);
        let data = samples.parameters(tile.texture.format());
        let pending=PendingTile {
            pixels: Pixels::Resident, texture:texture.clone(), view:view.clone(), data: Some(data), write, receipt: Default::default(), dependency,
            input: Some((tile.texture, samples.space, std::sync::Mutex::new(Some(completion)))),
        };
        let pending=self.pending(&view,Some(pending));
        Ok((RawTile {texture,view},pending))
    }
    pub(crate) fn display(&mut self, r: &WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        samples:Samples,input:&TileTexture,output:&wgpu::TextureView,uniforms:&wgpu::BindGroup,offset:u32,
    ) -> Result<(), GpuRasterError> {
        let pipelines = &r.scene_pipelines.source;
        if !r.compilation.require([pipelines.for_format(input.format())], startup::VALIDATION) { return Err(GpuRasterError::NotReady); }
        self.flush_retired(encoder);
        let srgb=samples.srgb_view();
        let transfer = &self.prepare_transfer(&r.device, samples.space)?;
        let view = input.create_view(&wgpu::TextureViewDescriptor { format: srgb.then_some(wgpu::TextureFormat::Rgba8UnormSrgb), ..Default::default() });
        pipelines.normalize(&r.device,encoder,&view,output,transfer,uniforms,offset);
        Ok(())
    }
    fn prepare_color(&mut self,device:&crate::PipelineDevice,color:SourceColor)->Result<(),GpuRasterError> {
        let format=color.input_format();
        if !self.inputs.iter().any(|input|input.texture.format()==format) {
            self.input_bytes.merge(self.decoded.charge(page_bytes(format)));
            self.inputs.push(EncodedInput {texture:device.create_texture(&wgpu::TextureDescriptor {
            label:Some("bounded exact source input"),size:wgpu::Extent3d {width:PAGE_SIZE,height:PAGE_SIZE,depth_or_array_layers:1},mip_level_count:1,sample_count:1,
            dimension:wgpu::TextureDimension::D2,format,usage:wgpu::TextureUsages::TEXTURE_BINDING|wgpu::TextureUsages::COPY_DST,view_formats:&[],
        })});}
        self.prepare_transfer(device,color.space())?;Ok(())
    }
    pub(super) fn preflight(&mut self,device:&crate::PipelineDevice,pending:&PendingTile)->Result<(),GpuRasterError> {
        pending.validate_prepared()?;
        if let Some((_,space,_))=&pending.input {self.prepare_transfer(device,*space)?;}
        else if pending.input_format().is_some() && let Some(color)=pending.data {self.prepare_color(device,color)?;}
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn encode(
        &mut self,
        device: &crate::PipelineDevice,
        uploads: &mut crate::Uploads,
        pipelines: &Pipelines,
        encoder: &mut crate::submission::CommandEncoder,
        pending: &PendingTile,
        snapshot_worker:bool,
        uniforms: Option<&wgpu::BindGroup>,
    ) -> Result<u64, GpuRasterError> {
        if pending.recorded() {return Ok(0);}
        if let Some((input, space, retirement)) = &pending.input {
            let transfer = self.prepare_transfer(device, *space)?;
            pipelines.normalize(device, encoder, &input.create_view(&Default::default()), &pending.view, &transfer,
                uniforms.ok_or(GpuRasterError::InvalidExtent)?, 0);
            pending.write.track(encoder);
            let epoch=self.decoded.heap().epoch();if let Some(slot)=self.viewed(&pending.view) {slot.pending=None;slot.touch(epoch);}
            if let Some(completion) = retirement.lock().unwrap().take() { encoder.on_submitted_work_done(move || {
            #[cfg(not(target_arch="wasm32"))]
            completion.complete();
            #[cfg(target_arch="wasm32")]
            drop(completion);
        }); }
            return Ok(0);
        }
        let prepared_pixels=pending.prepared();
        if (!snapshot_worker || cfg!(target_arch="wasm32")) && prepared_pixels.is_none() {return Err(GpuRasterError::NotReady);}
        let bytes = if pending.data.is_some() {
            let samples = pending.pixels.native()?;
            let target = if pending.texture.format() == wgpu::TextureFormat::Rgba32Float {
                let format=pending.input_format().ok_or(GpuRasterError::InvalidImage)?;
                self.inputs.iter().find(|input|input.texture.format()==format).ok_or(GpuRasterError::InvalidImage)?.texture.clone().into()
            } else { pending.texture.clone() };
            let decoded = match &prepared_pixels {
                Some(PreparedSourcePixels::NativeSamples(samples)) => samples.clone(),
                Some(PreparedSourcePixels::PremultipliedWorkingPixels(_)) => return Err(GpuRasterError::Color("Working source pixels have native decode settings".into())),
                None => {
                    #[cfg(not(target_arch="wasm32"))]
                    {device.source_samples.decode_on_worker(samples.tile).map_err(GpuRasterError::Color)?}
                    #[cfg(target_arch="wasm32")]
                    {return Err(GpuRasterError::NotReady);}
                },
            };
            let step = samples.depth.bytes();
            let prepared = prepared_pixels.is_some();
            let channels = if prepared { 4 } else { samples.channels.count() };
            if !prepared && decoded.len() != (PAGE_SIZE * PAGE_SIZE) as usize * channels * step {
                return Err(GpuRasterError::Color("Prepared source samples have the wrong size".into()));
            }
            let bytes = (PAGE_SIZE * PAGE_SIZE) as usize * 4 * step;
            uploads.write_tile(encoder, &target, PAGE_SIZE * 4 * step as u32, |mapped| {
                if prepared || samples.channels == SourceChannels::Rgba {
                    mapped.copy_from_slice(&decoded);
                } else {
                    // Expand channels in one row of scratch. Alpha is coverage;
                    // grayscale uses the declared RGB transfer on equal R/G/B.
                    let mut row = [0u8; PAGE_SIZE as usize * 16];
                    let bpp = samples.channels.count() * step;
                    let row_bytes = PAGE_SIZE as usize * 4 * step;
                    for (y, input) in decoded.chunks_exact(PAGE_SIZE as usize * bpp).enumerate() {
                        expand_source_row(input, &mut row[..row_bytes], samples.channels, samples.depth);
                        mapped
                            .slice(y * row_bytes..(y + 1) * row_bytes)
                            .copy_from_slice(&row[..row_bytes]);
                    }
                }
            })?;
            if pending.texture.format() == wgpu::TextureFormat::Rgba32Float {
                let transfer = self.prepare_transfer(device, samples.space)?;
                pipelines.normalize(device, encoder, &target.create_view(&Default::default()), &pending.view, &transfer,
                    uniforms.ok_or(GpuRasterError::InvalidExtent)?, 0);
            }
            bytes as u64
        } else {
            let Pixels::Image(source, coordinate) = &pending.pixels else {
                unreachable!()
            };
            match &prepared_pixels {
                Some(PreparedSourcePixels::PremultipliedWorkingPixels(bytes)) => {
                    uploads.write_tile(encoder, &pending.texture, PAGE_SIZE * 16, |mapped| mapped.copy_from_slice(bytes))?;
                }
                Some(PreparedSourcePixels::NativeSamples(_)) => return Err(GpuRasterError::Color("Native source samples require decode settings".into())),
                None => self.upload_icc(device, uploads, source, *coordinate, encoder, &pending.texture)?,
            }
            NORMALIZED_PAGE_BYTES
        };
        pending.write.track(encoder);
        let epoch=self.decoded.heap().epoch();if let Some(slot)=self.viewed(&pending.view) {slot.pending=None;slot.touch(epoch);}
        self.misses += 1;
        Ok(bytes)
    }

    fn upload_icc(
        &mut self,
        device: &crate::PipelineDevice,
        uploads: &mut crate::Uploads,
        source: &Arc<SourceImage>,
        coordinate: [u32; 2],
        encoder: &mut crate::submission::CommandEncoder,
        texture: &TileTexture,
    ) -> Result<(), GpuRasterError> {
        let weak = Arc::downgrade(source);
        let index = if let Some(index) = self.decoders.iter().position(|(s, _)| s.ptr_eq(&weak)) {
            index
        } else {
            // Keep the source native; decode into the document's working primaries.
            let decoder = layer_color::WorkingDecoder::new(
                &source.interpretation,
                self.destination,
                Default::default(),
            )
            .map_err(GpuRasterError::Color)?;
            if self.decoders.len() == DECODERS {
                self.decoders.pop_front();
            }
            self.decoders.push_back((weak, decoder));
            self.decoders.len() - 1
        };
        let decoder = self.decoders.remove(index).unwrap();
        self.pixels
            .resize((PAGE_SIZE * PAGE_SIZE) as usize, [0.; 4]);
        decoder
            .1
            .decode_tile_cached(source, coordinate, &mut self.pixels, &device.source_samples)
            .map_err(GpuRasterError::Color)?;
        self.decoders.push_back(decoder);
        upload_working_pixels(uploads, encoder, texture, &self.pixels)
    }
}

fn upload_working_pixels(uploads: &mut crate::Uploads, encoder: &mut crate::submission::CommandEncoder,
    texture: &TileTexture, pixels: &[[f32; 4]],
) -> Result<(), GpuRasterError> {
    if pixels.len() != (PAGE_SIZE * PAGE_SIZE) as usize { return Err(GpuRasterError::InvalidExtent); }
        uploads.write_tile(encoder, texture, PAGE_SIZE * 16, |mapped| {
            let mut row = [0u8; PAGE_SIZE as usize * 16];
            for (y, pixels) in pixels.as_chunks::<{ PAGE_SIZE as usize }>().0.iter().enumerate() {
                for (bytes, pixel) in row.as_chunks_mut::<16>().0.iter_mut().zip(pixels) {
                    for (channel, bytes) in bytes.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                        let value = if channel == 3 {
                            pixel[3]
                        } else {
                            pixel[channel] * pixel[3]
                        };
                        bytes.copy_from_slice(&value.to_ne_bytes());
                    }
                }
                mapped
                    .slice(y * row.len()..(y + 1) * row.len())
                    .copy_from_slice(&row);
            }
        })
}

pub(crate) fn validate_raster(blob: &TileBlob, space: RgbSpace) -> Result<(), GpuRasterError> {
    let d = blob.descriptor;
    if d.channels == 1 && d.sample == layer_core::color::SampleType::Unsigned
        && matches!(d.bits_per_channel, 8 | 16) && d.encoding == TransferEncoding::Linear
        && d.alpha == AlphaAssociation::None { return Ok(()); }
    if !matches!(d.channels, 2 | 4)
        || d.bytes_per_pixel().is_none()
        || !matches!(
            d.alpha,
            AlphaAssociation::Straight | AlphaAssociation::PremultipliedLinear
        )
        || !(d.sample == layer_core::color::SampleType::Float && d.encoding == TransferEncoding::Linear && matches!(d.bits_per_channel, 16 | 32)
            || d.encoding == TransferEncoding::Profile
            || (d.encoding == TransferEncoding::Srgb && space == RgbSpace::Srgb))
    {
        return Err(GpuRasterError::Color(
            "Invalid native raster decode representation".into(),
        ));
    }
    Ok(())
}

/// Expand integer source channels without changing their codes or byte order.
/// Fixed pixel widths keep the hot upload path out of per-channel dynamic copies.
pub(crate) fn expand_source_row(input: &[u8], output: &mut [u8], channels: SourceChannels, depth: SampleDepth) {
    match (depth, channels) {
        (SampleDepth::U8, SourceChannels::Rgb) => {
            for (p, o) in input.as_chunks::<3>().0.iter().zip(output.as_chunks_mut::<4>().0.iter_mut()) {
                o.copy_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        (SampleDepth::U8, SourceChannels::Gray) => {
            for (p, o) in input.iter().zip(output.as_chunks_mut::<4>().0.iter_mut()) {
                o.copy_from_slice(&[*p, *p, *p, 255]);
            }
        }
        (SampleDepth::U8, SourceChannels::GrayAlpha) => {
            for (p, o) in input.as_chunks::<2>().0.iter().zip(output.as_chunks_mut::<4>().0.iter_mut()) {
                o.copy_from_slice(&[p[0], p[0], p[0], p[1]]);
            }
        }
        (SampleDepth::U16, SourceChannels::Rgb) => {
            for (p, o) in input.as_chunks::<6>().0.iter().zip(output.as_chunks_mut::<8>().0.iter_mut()) {
                o.copy_from_slice(&[p[0], p[1], p[2], p[3], p[4], p[5], 255, 255]);
            }
        }
        (SampleDepth::U16, SourceChannels::Gray) => {
            for (p, o) in input.as_chunks::<2>().0.iter().zip(output.as_chunks_mut::<8>().0.iter_mut()) {
                o.copy_from_slice(&[p[0], p[1], p[0], p[1], p[0], p[1], 255, 255]);
            }
        }
        (SampleDepth::U16, SourceChannels::GrayAlpha) => {
            for (p, o) in input.as_chunks::<4>().0.iter().zip(output.as_chunks_mut::<8>().0.iter_mut()) {
                o.copy_from_slice(&[p[0], p[1], p[0], p[1], p[0], p[1], p[2], p[3]]);
            }
        }
        (SampleDepth::F32, SourceChannels::Rgb) => {
            for (p, o) in input.as_chunks::<12>().0.iter().zip(output.as_chunks_mut::<16>().0.iter_mut()) {
                o[..12].copy_from_slice(p);
                o[12..].copy_from_slice(&1f32.to_le_bytes());
            }
        }
        (SampleDepth::F16, SourceChannels::Rgb) => {
            for (p,o) in input.as_chunks::<6>().0.iter().zip(output.as_chunks_mut::<8>().0.iter_mut()) { o[..6].copy_from_slice(p); o[6..].copy_from_slice(&0x3c00u16.to_le_bytes()); }
        }
        (SampleDepth::F16 | SampleDepth::F32, SourceChannels::GrayAlpha) => {
            let step = depth.bytes();
            for (p, o) in input.chunks_exact(2 * step).zip(output.chunks_exact_mut(4 * step)) {
                for channel in o[..3 * step].chunks_exact_mut(step) { channel.copy_from_slice(&p[..step]); }
                o[3 * step..].copy_from_slice(&p[step..]);
            }
        }
        (SampleDepth::F16 | SampleDepth::F32, SourceChannels::Gray) => unreachable!("invalid HDR gray source"),
        (_, SourceChannels::Rgba | SourceChannels::Cmyk) => unreachable!("RGBA copies directly; CMYK uses its ICC transform"),
    }
}

fn builtin_settings(
    source: &SourceImage,
    coordinate: [u32; 2],
    destination: RgbSpace,
) -> Option<SourceColor> {
    let ColorProfile::Builtin(space) = source.interpretation.profile else {
        return None;
    };
    if source.interpretation.channels == SourceChannels::Cmyk {
        return None;
    }
    Some(rgb_settings(
        space,
        destination,
        source.interpretation.depth,
        std::array::from_fn(|i| {
            source.extent[i]
                .saturating_sub(coordinate[i] * PAGE_SIZE)
                .min(PAGE_SIZE)
        }),
        AlphaAssociation::Straight,
    ))
}
fn rgb_settings(
    space: RgbSpace,
    destination: RgbSpace,
    depth: SampleDepth,
    extent: [u32; 2],
    alpha: AlphaAssociation,
) -> SourceColor {
    let rows=space.linear_transform(destination).map(|row|[row[0] as f32,row[1] as f32,row[2] as f32,0.]);
    SourceColor {red:rows[0],green:rows[1],blue:rows[2],premultiplied:u32::from(alpha==AlphaAssociation::PremultipliedLinear),
        integer_max:if depth.is_float() {0.}else{depth.maximum() as f32},extent:extent.map(|side|side as f32),
        float_bits:u32::from(depth==SampleDepth::F32),identity:u32::from(space==destination),scalar:0,scalar_scale:0.,
        representation:match depth {SampleDepth::U8=>Representation::Unorm8,SampleDepth::U16=>Representation::Packed16,_=>Representation::Unpacked},
        display:0,transfer:space.into(),padding:0}

}

#[derive(Clone)]
pub(crate) struct Pipelines {
    layouts: [wgpu::BindGroupLayout; 2],
    pub pipeline: Deferred<wgpu::RenderPipeline>,
    pub integer: Deferred<wgpu::RenderPipeline>,
}
impl Pipelines {
    pub fn new(device: &PipelineDevice, uniforms: &wgpu::BindGroupLayout) -> Self {
        let layouts = [false, true].map(|integer| crate::bindings::layout(device, "source normalization", &[
            crate::bindings::texture_of(0, wgpu::ShaderStages::FRAGMENT,
                if integer { wgpu::TextureSampleType::Uint } else { wgpu::TextureSampleType::Float { filterable: false } }, wgpu::TextureViewDimension::D2),
            crate::bindings::buffer(1, wgpu::ShaderStages::FRAGMENT, wgpu::BufferBindingType::Storage { read_only: true }, false,
                wgpu::BufferSize::new(transfer::TABLE_BYTES)),
        ]));
        let [pipeline, integer] = std::array::from_fn(|index| {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("source normalization"), bind_group_layouts: &[Some(uniforms), Some(&layouts[index])], immediate_size: 0,
            });
            let input = if index == 0 { r#"
@group(1) @binding(0) var encoded: texture_2d<f32>;
fn native_samples(p: vec2<i32>) -> vec4<u32> { return vec4<u32>(round(clamp(textureLoad(encoded,p,0),vec4(0.),vec4(1.))*255.)); }
fn display_samples(p: vec2<i32>) -> vec4<f32> { return textureLoad(encoded,p,0); }
"# } else { r#"
@group(1) @binding(0) var encoded: texture_2d<u32>;
fn native_samples(p: vec2<i32>) -> vec4<u32> {
    let value=textureLoad(encoded,p,0);
    if settings.representation==SOURCE_PACKED16 { return vec4(value.r&65535u,value.r>>16u,value.g&65535u,value.g>>16u); }
    return value;
}
fn display_samples(p: vec2<i32>) -> vec4<f32> { let code=native_samples(p); return vec4(vec3<f32>(code.rgb)/settings.integer_max, select(f32(code.a)/settings.integer_max,1.,code.a==u32(settings.integer_max))); }
"# };
            let shader = Deferred::wgsl(device, "source normalization", format!("{}\n{}\n{}",
                include_str!("../native_tiles/validity.wgsl"), include_str!("source_color.wgsl"), include_str!("source_decode.wgsl").replace("SOURCE_INPUT", input)));
            let device = device.for_recipe();
            Deferred::pipeline(move |mode| fullscreen_pipeline_recipe(mode, &device, &layout, &shader, "fragment_main", None,
                wgpu::TextureFormat::Rgba32Float, "normalize source tile"))
        });
        Self { layouts, pipeline, integer }
    }
    fn index(texture: &wgpu::Texture) -> usize {
        usize::from(Self::integer_format(texture.format()))
    }
    fn integer_format(format: wgpu::TextureFormat) -> bool { matches!(format, wgpu::TextureFormat::Rg32Uint
        | wgpu::TextureFormat::Rgba8Uint | wgpu::TextureFormat::Rgba16Uint | wgpu::TextureFormat::Rgba32Uint) }
    pub(crate) fn for_format(&self, format: wgpu::TextureFormat) -> &Deferred<wgpu::RenderPipeline> {
        if Self::integer_format(format) { &self.integer } else { &self.pipeline }
    }
    fn normalize(&self, device: &PipelineDevice, encoder: &mut crate::submission::CommandEncoder, input: &wgpu::TextureView,
        output: &wgpu::TextureView, transfer: &transfer::NativeTransfer, uniforms: &wgpu::BindGroup, offset: u32,
    ) {
        let index = Self::index(input.texture());
        let binding = crate::bindings::group(device, "source normalization input", &self.layouts[index], [
            wgpu::BindingResource::TextureView(input), transfer.as_entire_binding(),
        ]);
        let attachments = [Some(attachment(output, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)))];
        #[cfg(test)]
        crate::test_support::PassCounts {normalizations:1,..Default::default()}.record();
        let mut pass = encoder.begin_render_pass(&descriptor(&attachments));
        pass.set_pipeline(if index == 0 { &self.pipeline } else { &self.integer });
        pass.set_bind_group(0, uniforms, &[offset]); pass.set_bind_group(1, &binding, &[]); pass.draw(0..3,0..1);
    }
}

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
mod grant_tests {
    use super::*;
    #[test]
    fn source_color_uniform_matches_the_shader_layout() {
        assert_eq!(std::mem::size_of::<SourceColor>(),96);
        assert_eq!(std::mem::align_of::<SourceColor>(),16);
        assert_eq!(std::mem::offset_of!(SourceColor,integer_max),52);
        assert_eq!(std::mem::offset_of!(SourceColor,float_bits),64);
        assert_eq!(std::mem::offset_of!(SourceColor,representation),80);
        assert_eq!(std::mem::offset_of!(SourceColor,transfer),88);
    }
    #[test]
    fn tiny_upload_grants_defer_a_float_tile_without_underflow() {
        let mut cache = DecodedTiles::within(&Default::default(), Default::default());
        cache.admit_limits(NORMALIZED_PAGE_BYTES - 1);
        assert!(cache.uploads_full(crate::residency::Class::Visible));
        assert!(cache.admit_upload(crate::residency::Class::Visible, NORMALIZED_PAGE_BYTES / 4).is_ok());
        assert!(cache.admit_upload(crate::residency::Class::Visible, NORMALIZED_PAGE_BYTES).is_err());
        cache.admit_limits(NORMALIZED_PAGE_BYTES);
        assert!(!cache.uploads_full(crate::residency::Class::Visible));
    }
}

pub(crate) fn encode_pending(r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
    pending: Arc<PendingTile>, class: crate::residency::Class,
) -> Result<bool, GpuRasterError> {
        #[cfg(test)]
        assert_reading("source upload encoding");
        if pending.recorded() {return Ok(true);}
        let admission = r.source_tiles.get_mut().preflight(&r.device, &pending);
        r.source_tiles.get_mut().flush_retired(encoder);
        admission?;
        if let Some(pipeline) = pending.pipeline(&r.scene_pipelines.source)
            && !r.compilation.require([pipeline],startup::VALIDATION) { return Err(GpuRasterError::NotReady); }
        if let Some(input)=pending.dependency() && !encode_pending(r,encoder,input.clone(),class)? {return Ok(false);}
        let Some(reservation)=r.reserve_upload(class,if pending.input.is_some() {0}else {NORMALIZED_PAGE_BYTES}) else {return Ok(false);};
        let binding = if pending.pipeline(&r.scene_pipelines.source).is_some() {
            let records = r.device.create_buffer(&wgpu::BufferDescriptor { label: Some("source normalization records"),
                size: 144, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            let binding = uniform_binding(&r.device, &r.scene_pipelines.uniforms, &records);
            let mut data = [0; 144];
            if let Some(values)=pending.normalization_parameters() {values.write_uniform(&mut data);}
            r.uploads.write_at(encoder, &records, 0, &data)?;
            Some(binding)
        } else { None };
        let bytes = r.source_tiles.get_mut().encode(&r.device, &mut r.uploads, &r.scene_pipelines.source, encoder, &pending,r.snapshot_worker, binding.as_ref())?;
        r.charge_upload(encoder, reservation, bytes);
    Ok(true)
}
