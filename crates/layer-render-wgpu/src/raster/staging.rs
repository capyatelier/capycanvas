use super::*;

pub(crate) struct BrushPreparation(());
pub(crate) struct HeldBrush {
    stamps:Arc<scene::PreviewStamps>,
    residents:Arc<scene::sources::PreparedBrush>,
    state:Arc<scene::sources::PreparedState>,
    tiles:BTreeMap<(SourceTarget,[u32;2]),Vec<(BrushSlot,usize)>>,
}
#[derive(Clone,Copy,PartialEq)]
enum BrushSlot {Color(bool),Wetness(bool),State(scene::sources::BrushState,bool)}
trait BrushReference {
    fn identity(&self)->usize;
    fn reference(&self)->resident::Resident<crate::source_access::RawTile>;
}
impl<T:resident::TextureStorage> BrushReference for resident::Resident<T> {
    fn identity(&self)->usize {self.allocation_identity()}
    fn reference(&self)->resident::Resident<crate::source_access::RawTile> {self.texture_reference()}
}

const PREFETCH:usize=18;
const UPLOADS:usize=16;

#[derive(PartialEq)]
struct DataKey {contents:[layer_core::raster::ContentStamp;3],watercolor:Option<[u32;3]>}
impl DataKey {
    fn new(data:&RasterData)->Self {Self {contents:[RasterPlane::Color,RasterPlane::Mask,RasterPlane::WatercolorWetness]
        .map(|plane|data.tiles.content(plane).root()),
        watercolor:data.watercolor.map(|w|[w.wet_edge.to_bits(),w.burnt_edge.to_bits(),w.edge_width.to_bits()])}}
}
#[derive(PartialEq)]
struct FrameRoot {target:SourceTarget,revision:u64,published:Option<usize>,restore:Option<u64>,extent:[u32;2]}
#[derive(PartialEq)]
enum Key {
    Frame {color:layer_core::color::DocumentColor,extent:[u32;2],reset:bool,clear_starts:bool,roots:Vec<FrameRoot>,
        required:Vec<(SourceTarget,Vec<[u32;2]>)>,bases:Vec<(SourceTarget,usize,[u32;2])>,outputs:Vec<(SourceTarget,Output)>,restored:Vec<(SourceTarget,TileKey)>},
    Direct {color:layer_core::color::DocumentColor,target:SourceTarget,extent:[u32;2],before:DataKey,after:DataKey},
}
#[derive(Default)]
pub(crate) struct PreparedPages {
    pub colors:Vec<LayerPage>,
    pub masks:Vec<([u32;2],layer_masks::MaskPage)>,
    pub wetness:Vec<WatercolorWetnessPage>,
    pub coverage:Vec<StrokeCoveragePage>,
    pub companions:Vec<([u32;2],resident::Resident<PageSurface>)>,
    pub starts:Vec<([u32;2],layer_core::StrokeId,bool,resident::Resident<PageSurface>)>,
}
impl PreparedPages {
    fn storage_bytes(&self)->u64 {
        self.colors.iter().map(|page|page.primary.storage_bytes()+page.secondary.as_ref().map_or(0,|surface|surface.storage_bytes())
            +page.stroke_start.as_ref().map_or(0,StrokeStart::storage_bytes)).sum::<u64>()
            +self.masks.iter().map(|(_,page)|page.storage_bytes()).sum::<u64>()
            +self.wetness.iter().map(|page|page.primary.storage_bytes()+page.secondary.storage_bytes()).sum::<u64>()
            +self.coverage.iter().map(|page|page.primary.storage_bytes()+page.secondary.storage_bytes()).sum::<u64>()
            +self.companions.iter().map(|(_,page)|page.storage_bytes()).sum::<u64>()
            +self.starts.iter().map(|(_,_,_,page)|page.storage_bytes()).sum::<u64>()
    }
    fn discharge(&mut self) {
        self.colors.iter_mut().for_each(LayerPage::discharge);
        for page in &mut self.wetness {page.primary.discharge();page.secondary.discharge();}
        for page in &mut self.coverage {page.primary.discharge();page.secondary.discharge();}
        for (_,page) in &mut self.companions {page.discharge();}
        for (_,_,_,page) in &mut self.starts {page.discharge();}
    }
}
struct TargetRestore {
    target:SourceTarget,
    extent:[u32;2],
    defaults:[layer_core::raster::ContentVersion;3],
    data:Arc<RasterData>,
    selected:RasterData,
    pending:Vec<SourcePage>,
    next:usize,
    pages:PreparedPages,
    replace:bool,
    scalars:Vec<object_image_mips::PreparedRasterTile>,
    inputs:scene::sources::AcceptedInputs,
}
#[derive(Clone,Copy,PartialEq,Eq,PartialOrd,Ord)]
pub(crate) enum Output {
    Color([u32;2]),Coverage([u32;2],u32,bool),Wetness([u32;2]),Companion([u32;2]),
    StrokeStart([u32;2],layer_core::StrokeId,bool),Mask([u32;2],u32),
    PreviewColor([u32;2]),PreviewCoverage([u32;2],u32,bool),PreviewWetness([u32;2]),PreviewCompanion([u32;2]),
}
impl Output {
    pub(crate) fn native_key(&self)->Option<TileKey> {
        let (plane,coordinate)=match *self {
            Self::Color(c)=>(RasterPlane::Color,c),Self::Mask(c,_)=>(RasterPlane::Mask,c),
            Self::Wetness(c)=>(RasterPlane::WatercolorWetness,c),_=>return None,
        };Some(TileKey {plane,coordinate})
    }
    fn coordinate(&self)->[u32;2] {match self {
        Self::Color(c)|Self::Coverage(c,..)|Self::Wetness(c)|Self::Companion(c)|Self::StrokeStart(c,..)|Self::Mask(c,_)
            |Self::PreviewColor(c)|Self::PreviewCoverage(c,..)|Self::PreviewWetness(c)|Self::PreviewCompanion(c)=>*c,
    }}
    fn preview(&self)->bool {matches!(self,Self::PreviewColor(_)|Self::PreviewCoverage(..)|Self::PreviewWetness(_)|Self::PreviewCompanion(_))}
    fn bytes(&self,r:&WgpuRasterizer,preview_level:u32)->u64 {
        let (format,count)=match self {
            Self::Color(_)|Self::Companion(_)|Self::StrokeStart(..)|Self::PreviewColor(_)|Self::PreviewCompanion(_)=>(r.device.working_format(),1),
            Self::Coverage(_,slabs,rgba)|Self::PreviewCoverage(_,slabs,rgba)=>(if *rgba {wgpu::TextureFormat::Rgba32Float}else {r.device.scalar_format()},2*u64::from(*slabs)),
            Self::Wetness(_)|Self::PreviewWetness(_)=>(r.device.scalar_format(),2),Self::Mask(..)=>(r.device.scalar_format(),1),
        };
        let side=if matches!(self,Self::PreviewColor(_)) {PAGE_SIZE>>preview_level}else {PAGE_SIZE};
        u64::from(side).pow(2)*u64::from(format.block_copy_size(None).unwrap())*count
    }
    fn same_storage(&self,other:&Self)->bool {match (self,other) {
        (Self::Mask(a,_),Self::Mask(b,_))=>a==b,_=>self==other,
    }}
    fn preview_present(&self,r:&WgpuRasterizer,level:u32)->bool {match self {
        Self::PreviewColor(c)=>r.preview_pages.iter().any(|page|page.coordinate==*c && page.primary.size().width==PAGE_SIZE>>level),
        Self::PreviewCoverage(c,slabs,rgba)=>r.preview_coverage_pages.iter().any(|page|page.coordinate==*c
            && page.primary.size().height==PAGE_SIZE*slabs && (page.primary.format()==wgpu::TextureFormat::Rgba32Float)==*rgba),
        Self::PreviewWetness(c)=>r.preview_watercolor_wetness_pages.iter().any(|page|page.coordinate==*c),
        Self::PreviewCompanion(c)=>r.preview_pages.iter().any(|page|page.coordinate==*c && page.secondary.is_some()),
        _=>false,
    }}
}
enum SourcePage {Raster(TileKey,RasterTile),Base([u32;2],layer_core::authored::PaintBase),Output(Output)}
impl SourcePage {
    fn inputs(&self,space:layer_core::color::RgbSpace)->Result<Option<scene::sources::Inputs>,GpuRasterError> {
        let mut inputs=scene::sources::Inputs::default();
        match self {
            Self::Raster(key,tile) if key.plane==RasterPlane::Color=> {
                let Some(blob)=tile.try_backing().transpose().map_err(GpuRasterError::Effect)? else {return Ok(None);};
                if blob.descriptor!=tile.descriptor() {return Err(GpuRasterError::Effect("Raster plane has the wrong pixel representation".into()));}
                inputs.raster(blob,space,space);
            },
            Self::Base(coordinate,base)=>inputs.base(base,*coordinate,space),
            _=>{},
        }
        Ok(Some(inputs))
    }
    fn output(&self)->Output {match self {
        Self::Raster(key,_)=>match key.plane {RasterPlane::Color=>Output::Color(key.coordinate),RasterPlane::Mask=>Output::Mask(key.coordinate,0),RasterPlane::WatercolorWetness=>Output::Wetness(key.coordinate)},
        Self::Base(c,_)=>Output::Color(*c),Self::Output(output)=>output.clone(),
    }}
}
struct Request {target:SourceTarget,extent:[u32;2],before:RasterData,data:Arc<RasterData>,required:BTreeSet<[u32;2]>,base:Option<layer_core::authored::PaintBase>,replace:bool}
impl TargetRestore {
    fn new(r:&mut WgpuRasterizer,request:Request,commit:Option<&paint_transform::PreparedTransformCommit>,restored:&[(SourceTarget,TileKey)])->Result<Self,GpuRasterError> {
        let Request {target,extent,before,data,required,base,replace}=request;
        data.validate_storage_index(extent,target.is_coverage(),r.document_color()).map_err(GpuRasterError::Effect)?;
        let defaults=super::contents::defaults_for(extent,base.as_ref(),None);
        let (before,selected)=if replace {r.select_restore_data(target,&before,&data,&required)}else {(before,RasterData {watercolor:data.watercolor,
            tiles:required.iter().flat_map(|coordinate|super::contents::planes().map(|plane|TileKey {plane,coordinate:*coordinate})).filter_map(|key|data.tiles.get(&key).map(|tile|(key,tile.clone()))).collect()})};
        #[cfg(test)] if !replace {crate::test_support::PassCounts {warm_restore_extra_tiles:before.tiles.keys().chain(selected.tiles.keys()).filter(|key|!required.contains(&key.coordinate)).count() as u64,..Default::default()}.record();}
        let resident=|key:&TileKey|if commit.is_some_and(|commit|commit.contains(target,key.plane,key.coordinate)) || restored.binary_search(&(target,*key)).is_ok() {true}else {match key.plane {
            RasterPlane::Mask=>r.layer_masks.pages.get(&(target,key.coordinate)).is_some_and(|page| {
                let name=(!replace).then(||r.raster_intent(target,key.plane,key.coordinate).ok().map(|intent|intent.version().clone())).flatten()
                    .or_else(||data.tiles.get(key).map(RasterTile::content_version));
                name.is_some_and(|name|page.has_name(&name))
            }),
            RasterPlane::Color|RasterPlane::WatercolorWetness=> {
                let name=(!replace).then(||r.current_raster_version(target,key.plane,key.coordinate).ok()).flatten()
                    .or_else(||data.tiles.get(key).map(RasterTile::content_version))
                    .unwrap_or_else(||super::contents::fallback(&defaults[super::contents::index(key.plane)],key.coordinate));
                r.paint_layers.iter().find(|layer|layer.id==target).is_some_and(|layer|match key.plane {
                    RasterPlane::Color=>layer.pages.iter().find(|page|page.coordinate==key.coordinate).is_some_and(|page|page.active().has_name(&name)),
                    _=>layer.watercolor_wetness_pages.iter().find(|page|page.coordinate==key.coordinate).is_some_and(|page|page.active().has_name(&name)),
                })
            },
        }};
        let mut pending:Vec<_>=selected.tiles.iter().filter(|(key,tile)|!commit.is_some_and(|commit|commit.contains(target,key.plane,key.coordinate)) && if replace {
            !before.tiles.get(key).is_some_and(|old|old.same_capture(tile)) || required.contains(&key.coordinate) && !resident(key)
        }else {!resident(key)})
            .map(|(key,tile)|SourcePage::Raster(*key,tile.clone())).collect();
        if let Some(base)=base {
            pending.extend(required.iter().filter(|coordinate| !commit.is_some_and(|commit|commit.contains(target,RasterPlane::Color,**coordinate)) && (replace || !resident(&TileKey {plane:RasterPlane::Color,coordinate:**coordinate})) && !data.tiles.contains_key(&TileKey {plane:RasterPlane::Color,coordinate:**coordinate})
                && source_access::paint_base_contains(&base,**coordinate)).map(|coordinate|SourcePage::Base(*coordinate,base.clone())));
        }
        Ok(Self {target,extent,defaults,data,selected,pending,next:0,pages:PreparedPages::default(),replace,scalars:Vec::new(),inputs:Default::default()})
    }
    fn awaits(&self,blob:&TileBlob)->bool {
        self.pending.iter().skip(self.next).filter(|source|!matches!(source,SourcePage::Output(_))).take(PREFETCH).any(|source|matches!(source,SourcePage::Raster(key,tile) if key.plane!=RasterPlane::Color
            && tile.try_backing().and_then(Result::ok).is_some_and(|awaited|scene::sources::same_raster_samples(&awaited,blob))))
    }
    fn held(&self,blob:&TileBlob)->Option<&object_image_mips::PreparedRasterTile> {
        self.scalars.iter().find(|prepared|scene::sources::same_raster_samples(&prepared.blob,blob))
    }
    fn projected_bytes(&self,r:&WgpuRasterizer)->Result<u64,GpuRasterError> {
        if self.pages.storage_bytes()!=0 {return Ok(0);}
        self.pending.iter().try_fold(0u64,|bytes,source|bytes.checked_add(source.output().bytes(r,r.preview_level)).ok_or(GpuRasterError::SizeOverflow))
    }
    fn add_outputs(&mut self,r:&WgpuRasterizer,outputs:&[(SourceTarget,Output)],reset:bool,clear_starts:bool,commit:Option<&paint_transform::PreparedTransformCommit>,restored:&[(SourceTarget,TileKey)]) {
        for (_,output) in outputs.iter().filter(|(target,_)|*target==self.target) {
            if output.preview() {continue;}
            if self.pending.iter().any(|source|source.output().same_storage(output)) {continue;}
            let c=output.coordinate();
            let plane=match output {Output::Color(_)=>Some(RasterPlane::Color),Output::Wetness(_)=>Some(RasterPlane::WatercolorWetness),Output::Mask(..)=>Some(RasterPlane::Mask),_=>None};
            if plane.is_some_and(|plane|commit.is_some_and(|commit|commit.contains(self.target,plane,c)) || restored.binary_search(&(self.target,TileKey {plane,coordinate:c})).is_ok()) {continue;}
            let rewritten=self.pending.iter().any(|source|matches!(source.output(),Output::Color(at) if at==c));
            if let Output::StrokeStart(_,id,true)=output && self.pending.iter().any(|source|
                matches!(source,SourcePage::Output(Output::StrokeStart(at,old,false)) if *at==c && old==id)) {continue;}
            let layer=(!reset).then(||r.paint_layers.iter().find(|layer|layer.id==self.target)).flatten();
            let page=layer.and_then(|layer|layer.pages.iter().find(|page|page.coordinate==c)).filter(|page| {
                let key=TileKey {plane:RasterPlane::Color,coordinate:c};
                let name=(!self.replace).then(||r.current_raster_version(self.target,key.plane,c).ok()).flatten()
                    .or_else(||self.selected.tiles.get(&key).map(RasterTile::content_version))
                    .unwrap_or_else(||super::contents::fallback(&self.defaults[0],c));
                !rewritten && page.active().has_name(&name)
            });
            if matches!(output,Output::Companion(_)|Output::StrokeStart(_,_,false)) && page.is_none() && !rewritten {continue;}
            let present=match output {
                Output::Color(_)=>page.is_some(),Output::Companion(_)=>page.is_some_and(|page|page.secondary.is_some()),
                Output::StrokeStart(_,id,_)=>!clear_starts && page.is_some_and(|page|page.stroke_start.as_ref().is_some_and(|start|start.state.stroke==*id)),
                Output::Coverage(_,slabs,rgba)=>!self.replace && layer.is_some_and(|layer|layer.coverage_pages.iter().any(|page|page.coordinate==c
                    && page.primary.size().height==PAGE_SIZE*slabs && (page.primary.format()==wgpu::TextureFormat::Rgba32Float)==*rgba)),
                Output::Wetness(_)=>layer.is_some_and(|layer|layer.watercolor_wetness_pages.iter().any(|page|page.coordinate==c))
                    && (!self.replace || self.selected.tiles.contains_key(&TileKey {plane:RasterPlane::WatercolorWetness,coordinate:c})),
                Output::Mask(_,default)=>!reset && r.layer_masks.pages.get(&(self.target,c)).is_some_and(|page| {
                    let key=TileKey {plane:RasterPlane::Mask,coordinate:c};
                    let name=(!self.replace).then(||r.raster_intent(self.target,key.plane,c).ok().map(|intent|intent.version().clone())).flatten()
                        .or_else(||self.selected.tiles.get(&key).map(RasterTile::content_version))
                        .unwrap_or_else(||super::contents::mask_default(self.extent,f32::from_bits(*default),c));
                    page.has_name(&name)
                }),
                _=>unreachable!(),
            };
            if !present {self.pending.push(SourcePage::Output(output.clone()));}
        }
    }
    fn allocate(&mut self,r:&WgpuRasterizer,encoder:&mut submission::CommandEncoder)->Result<(),GpuRasterError> {
        for source in &self.pending {match source.output() {
            Output::Color(c)=> {
                let mut page=r.create_page(c,"prepared raster color");
                if matches!(source,SourcePage::Output(_)) {
                    let receipt=encoder.submission();let write=page.primary.initialize(&receipt).ok_or(GpuRasterError::Invariant("prepared output was already initialized"))?;
                    write.value().clear(&r.zero_pages,encoder);write.publish(super::contents::fallback(&self.defaults[0],c));page.primary_needs_clear=false;
                }
                self.pages.colors.push(page);
            },
            Output::Coverage(c,slabs,rgba)=>self.pages.coverage.push(r.create_stroke_coverage_page(c,slabs,rgba)),
            Output::Wetness(c)=>self.pages.wetness.push(WatercolorWetnessPage {pending:None,derived:None,coordinate:c,
                primary:resident::Resident::unwritten(r.create_scalar_page_surface(&r.raster_tiers.paint,"prepared wetness primary")),secondary:resident::Resident::unwritten(r.create_scalar_page_surface(&r.raster_tiers.paint,"prepared wetness companion")),
                active_secondary:false,primary_needs_clear:true,secondary_needs_clear:true}),
            Output::Companion(c)=>self.pages.companions.push((c,resident::Resident::unwritten(r.create_page_surface(&r.raster_tiers.paint,"prepared destination companion")))),
            Output::StrokeStart(c,id,preview)=>self.pages.starts.push((c,id,preview,resident::Resident::unwritten(r.create_page_surface(&r.raster_tiers.paint,"prepared stroke start")))),
            Output::Mask(c,default)=> {
                let mut page=layer_masks::MaskPage::new(&r.device);let value=f64::from(f32::from_bits(default));
                if matches!(source,SourcePage::Output(_)) {
                    let receipt=encoder.submission();let write=page.initialize(&receipt)?;
                    {let _clear=encoder.color_pass("prepared default mask",&write.value().view,wgpu::LoadOp::Clear(wgpu::Color {r:value,g:value,b:value,a:value}));}
                    write.publish(super::contents::mask_default(self.extent,f32::from_bits(default),c));
                }
                self.pages.masks.push((c,page));
            },
            _=>unreachable!(),
        }}
        Ok(())
    }
    fn complete(&self)->bool {self.next==self.pending.len()}
    fn source_kinds(&self)->startup::SourceKinds {
        let mut kinds=startup::SourceKinds::default();
        for source in self.pending.iter().skip(self.next).filter(|source|!matches!(source,SourcePage::Output(_))).take(PREFETCH) {
            match source {
                SourcePage::Base(_,base)=>kinds.include(startup::SourceKinds::for_source(base.image.storage())),
                SourcePage::Raster(key,tile) if key.plane==RasterPlane::Color=>kinds.include(startup::SourceKinds::for_descriptor(tile.descriptor())),
                _=>{},
            }
        }
        kinds
    }
    fn ready(&self,r:&WgpuRasterizer)->bool {
        if self.projected_bytes(r).is_ok_and(|bytes|bytes!=0) {return false;}
        let Some(source)=self.pending.get(self.next) else {return false;};
        if let SourcePage::Raster(key,tile)=source && key.plane!=RasterPlane::Color {
            let Some(Ok(blob))=tile.try_backing() else {return false;};
            return self.held(&blob).is_some() && r.source_tiles.borrow().upload_blocked(crate::residency::Class::Authored,native_tiles::scalar::COVERAGE_TILE_BYTES).is_none();
        }
        source.inputs(r.document_color().space).ok().flatten().is_some_and(|inputs|self.inputs.ready_for(&inputs))
    }

    fn advance(&mut self,r:&mut WgpuRasterizer,encoder:&mut submission::CommandEncoder)->Result<(),GpuRasterError> {
        self.inputs.clone().track(encoder);
        let space=r.document_color().space;
        self.inputs.prepare(r)?;
        for source in self.pending.iter().skip(self.next).filter(|source|!matches!(source,SourcePage::Output(_))).take(PREFETCH) {
            if let SourcePage::Raster(key,tile)=source && key.plane!=RasterPlane::Color {
                let Some(blob)=tile.try_backing().transpose().map_err(GpuRasterError::Effect)? else {break;};
                if self.held(&blob).is_none() && !r.request_scalar_decode(blob,space)? {break;}
            }
        }
        let mut uploads=0;
        while let Some(source)=self.pending.get(self.next) {
            if matches!(source,SourcePage::Output(_)) {self.next+=1;continue;}
            if uploads==UPLOADS {break;}
            let Some(inputs)=source.inputs(space)? else {break;};
            if !self.inputs.contains(&inputs) || !self.inputs.ready_for(&inputs) {break;}
            uploads+=1;
            if let SourcePage::Base(coordinate,base)=source {
                let Some(read)=scene::sources::accepted_base(&self.inputs,base,*coordinate)? else {self.next+=1;continue;};
                if !read.encode(r,encoder,crate::residency::Class::Authored)? {break;}
                let page=self.pages.colors.iter_mut().find(|page|page.coordinate==*coordinate).unwrap();page.primary_needs_clear=false;
                let receipt=encoder.submission();let write=page.primary.initialize(&receipt).ok_or(GpuRasterError::Invariant("prepared output was already initialized"))?;
                encoder.copy_texture_to_texture(read.tile.texture.as_image_copy(),write.value().texture.as_image_copy(),write.value().texture.size());
                write.publish(super::contents::fallback(&self.defaults[0],*coordinate));
                self.next+=1;continue;
            }
            let SourcePage::Raster(key,tile)=source else {unreachable!()};
            let Some(blob)=tile.try_backing().transpose().map_err(GpuRasterError::Effect)? else {break;};
            if blob.descriptor!=tile.descriptor() {return Err(GpuRasterError::Effect("Raster plane has the wrong pixel representation".into()));}
            if key.plane==RasterPlane::Color {
                let read=scene::sources::accepted_raster(&self.inputs,&blob,space,space)?;
                if !read.encode(r,encoder,crate::residency::Class::Authored)? {break;}
                let page=self.pages.colors.iter_mut().find(|page|page.coordinate==key.coordinate).unwrap();page.primary_needs_clear=false;
                let receipt=encoder.submission();let write=page.primary.initialize(&receipt).ok_or(GpuRasterError::Invariant("prepared output was already initialized"))?;
                encoder.copy_texture_to_texture(read.tile.texture.as_image_copy(),write.value().texture.as_image_copy(),write.value().texture.size());
                write.publish(tile.content_version());
                self.next+=1;continue;
            }
            let Some(prepared)=self.held(&blob) else {r.request_scalar_decode(blob,space)?;break;};
            let samples=match &prepared.pixels {
                Ok(scene::sources::PreparedSourcePixels::NativeSamples(samples))=>samples.clone(),
                Ok(_)=>return Err(GpuRasterError::Color("Coverage requires native samples".into())),
                Err(error)=>return Err(GpuRasterError::Color(error.clone())),
            };
            if r.source_tiles.borrow().upload_blocked(crate::residency::Class::Authored,native_tiles::scalar::COVERAGE_TILE_BYTES).is_some() {break;}
            if key.plane==RasterPlane::Mask {
                let page=&mut self.pages.masks.iter_mut().find(|(c,_)|*c==key.coordinate).unwrap().1;
                let receipt=encoder.submission();let write=page.initialize(&receipt)?;
                native_tiles::scalar::upload_coverage(r,encoder,&write.value().texture,blob.descriptor.depth(),&samples,4,crate::residency::Class::Authored)?;
                write.publish(tile.content_version());
            } else {
                let page=self.pages.wetness.iter_mut().find(|page|page.coordinate==key.coordinate).unwrap();
                let receipt=encoder.submission();let write=page.primary.initialize(&receipt).ok_or(GpuRasterError::Invariant("prepared output was already initialized"))?;
                native_tiles::scalar::upload_coverage(r,encoder,&write.value().texture,blob.descriptor.depth(),&samples,4,crate::residency::Class::Authored)?;
                write.publish(tile.content_version());page.primary_needs_clear=false;
            }
            self.next+=1;
        }
        let mut scalars=std::mem::take(&mut self.scalars);
        scalars.retain(|prepared|self.awaits(&prepared.blob));
        self.scalars=scalars;
        let mut wanted=scene::sources::Inputs::default();
        for source in self.pending.iter().skip(self.next).filter(|source|!matches!(source,SourcePage::Output(_))).take(PREFETCH) {
            if let Some(inputs)=source.inputs(space)? {wanted.extend(inputs)?;}
        }
        self.inputs.retain(&wanted);
        Ok(())
    }

}
pub(crate) struct RestorePreflight {
    key:Key,
    commit:Option<paint_transform::PreparedTransformCommit>,
    pub(crate) native:Option<super::native_edit::NativeAdmission>,
    targets:Vec<Option<TargetRestore>>,
    failure:Option<GpuRasterError>,
    consumed:bool,
    starts:Vec<(SourceTarget,[u32;2],layer_core::StrokeId,bool,resident::Resident<PageSurface>)>,
    transforms:Vec<((SourceTarget,u32),paint_transform::Plan)>,
    preview:Option<crate::residency::Reservation>,
    refused:Option<crate::residency::Blocked>,
}
impl RestorePreflight {
    pub(crate) fn outputs(&self)->&[(SourceTarget,Output)] {
        match &self.key {Key::Frame {outputs,..}=>outputs,_=>&[]}
    }
    pub(crate) fn dependencies(&self)->&[(SourceTarget,Vec<[u32;2]>)] {
        match &self.key {
            Key::Frame {required,..}=>required,
            _=>&[],
        }
    }
    fn new(r:&mut WgpuRasterizer,key:Key,requests:Vec<Request>,commit:Option<paint_transform::PreparedTransformCommit>,transforms:Vec<((SourceTarget,u32),paint_transform::Plan)>)->Self {
        let mut targets=Vec::new();let mut failure=None;
        let restored=match &key {Key::Frame {restored,..}=>restored.as_slice(),Key::Direct {..}=>&[]};
        for request in requests {
            match TargetRestore::new(r,request,commit.as_ref(),restored) {
                Ok(target)=>targets.push(Some(target)),
                Err(error)=>{failure=Some(error);break;},
            }
        }
        if let Key::Frame {outputs,reset,clear_starts,..}=&key {for target in targets.iter_mut().flatten() {target.add_outputs(r,outputs,*reset,*clear_starts,commit.as_ref(),restored);}}
        Self {key,commit,native:None,targets,failure,consumed:false,starts:Vec::new(),transforms,preview:None,refused:None}
    }
    fn complete(&self)->bool {self.failure.is_some() || self.targets.iter().flatten().all(TargetRestore::complete)}
    fn waiting(&self)->bool {!self.complete() || self.failure.is_none() && self.refused.is_some()}
}
fn rectangular_preview_pages<'a>(outputs:&'a [(SourceTarget,Output)],batch:&'a DabBatch,extent:[u32;2])->impl Iterator<Item=[u32;2]>+'a {
    let damage=batch_pixel_rect(batch,extent);
    outputs.iter().filter_map(move |(target,output)|match output {
        Output::PreviewColor(coordinate) if *target==batch.target && !page_rect(*coordinate).intersect(damage).is_empty()=>Some(*coordinate),_=>None,
    })
}
impl WgpuRasterizer {
    pub(crate) fn accepted_raster_output(&self,target:SourceTarget,output:Output)->Result<(),GpuRasterError> {
        let accepted=self.restore_preflight.as_ref().is_some_and(|restore|match &restore.key {
            Key::Frame {outputs,..}=>outputs.binary_search(&(target,output)).is_ok(),_=>false,
        });
        prepared_raster_output(accepted.then_some(()))
    }
    pub(crate) fn accepted_previews(&self,target:SourceTarget)->Vec<Output> {
        self.restore_preflight.as_ref().map_or_else(Vec::new,|restore|match &restore.key {
            Key::Frame {outputs,..}=>outputs.iter().filter(|(at,output)|*at==target && output.preview()).map(|(_,output)|*output).collect(),_=>Vec::new(),
        })
    }
    pub(crate) fn accepted_transform_plan(&self,target:SourceTarget,operation:u32)->Result<paint_transform::Plan,GpuRasterError> {
        prepared_raster_output(self.restore_preflight.as_ref().and_then(|restore|restore.transforms.iter().find(|(at,_)|*at==(target,operation))).map(|(_,plan)|*plan))
    }
    pub(crate) fn accepted_material_tiles<'a>(&self,batch:&DabBatch,context:BrushEncodingContext<'a>)->Result<std::borrow::Cow<'a,[BrushTile]>,GpuRasterError> {
        let preview=context.target.is_preview();
        let rectangular=context.target==(BrushEncodingTarget::Preview {from_persistent:true}) && !Self::sparse_preview(context.batches);
        let tiles=if rectangular {
            let Some(restore)=&self.restore_preflight else {return prepared_raster_output(None);};
            let Key::Frame {outputs,..}=&restore.key else {return prepared_raster_output(None);};
            std::borrow::Cow::Owned(rectangular_preview_pages(outputs,batch,context.target_extent).map(|coordinate| {
                let planned=context.tiles.iter().find(|tile|tile.coordinate==coordinate);
                BrushTile {coordinate,local:planned.map_or(PixelRect::EMPTY,|tile|tile.local),
                    dabs:planned.map_or(batch.first_dab..batch.first_dab,|tile|tile.dabs.clone()),indices:Vec::new()}
            }).collect())
        } else {std::borrow::Cow::Borrowed(context.tiles)};
        for tile in tiles.iter() {
            self.accepted_raster_output(batch.target,if preview {Output::PreviewColor(tile.coordinate)}else {Output::Color(tile.coordinate)})?;
        }
        Ok(tiles)
    }
    pub(crate) fn clear_preview_pages(&mut self) {
        self.preview_pages.clear();self.preview_coverage_pages.clear();self.preview_watercolor_wetness_pages.clear();self.preview_working=None;self.preview_footprint.clear();
    }
    pub(crate) fn prepare_preview_outputs(&mut self,level:u32)->Result<(),GpuRasterError> {
        let reserved=self.restore_preflight.as_mut().and_then(|restore|restore.preview.take());
        let outputs=self.restore_preflight.as_ref().and_then(|restore|match &restore.key {
            Key::Frame {outputs,..}=>Some(outputs.iter().map(|(_,output)|*output).filter(Output::preview).collect::<BTreeSet<_>>()),_=>None,
        }).unwrap_or_default();
        for output in &outputs {
            if output.preview_present(self,level) {continue;}
            let coordinate=output.coordinate();
            match output {
                Output::PreviewColor(_)=>{
                    if let Some(page)=self.preview_pages.iter_mut().find(|page|!outputs.contains(&Output::PreviewColor(page.coordinate))
                        && page.primary.size().width==PAGE_SIZE>>level) {
                        page.coordinate=coordinate;page.active_secondary=false;page.clear_stroke_start();
                    }
                },
                Output::PreviewCoverage(_,slabs,rgba)=>{
                    if let Some(page)=self.preview_coverage_pages.iter_mut().find(|page|
                        !outputs.contains(&Output::PreviewCoverage(page.coordinate,page.primary.size().height/PAGE_SIZE,page.primary.format()==wgpu::TextureFormat::Rgba32Float))
                        && page.primary.size().height==PAGE_SIZE*slabs && (page.primary.format()==wgpu::TextureFormat::Rgba32Float)==*rgba) {
                        page.coordinate=coordinate;page.owner=None;page.active_secondary=false;
                    }
                },
                Output::PreviewWetness(_)=>{
                    if let Some(page)=self.preview_watercolor_wetness_pages.iter_mut().find(|page|!outputs.contains(&Output::PreviewWetness(page.coordinate))) {
                        page.coordinate=coordinate;page.active_secondary=false;
                    }
                },_=>{},
            }
        }
        let missing=outputs.into_iter().filter(|output|!output.preview_present(self,level)).collect::<Vec<_>>();
        let bytes=missing.iter().map(|output|output.bytes(self,level)).sum();
        if bytes==0 {self.preview_working=reserved;self.preview_pages.retain(|page|page.primary.size().width==PAGE_SIZE>>level);return Ok(());}
        let reservation=match reserved.filter(|reserved|reserved.bytes()>=bytes) {
            Some(reserved)=>reserved,None=>self.admit_raster_working(bytes).map_err(|_|GpuRasterError::NotReady)?,
        };
        for output in missing {match output {
            Output::PreviewColor(coordinate)=>self.preview_pages.push(LayerPage {derived:None,coordinate,
                primary:resident::Resident::unwritten(create_page_surface(&self.device,&self.float_texture_layout,&self.sampler,[PAGE_SIZE>>level;2],self.device.working_format(),"prepared preview color",&self.raster_tiers.paint)),
                secondary:None,active_secondary:false,primary_needs_clear:true,stroke_start:None}),
            Output::PreviewCoverage(coordinate,slabs,rgba)=>self.preview_coverage_pages.push(self.create_stroke_coverage_page(coordinate,slabs,rgba)),
            Output::PreviewWetness(coordinate)=>self.preview_watercolor_wetness_pages.push(WatercolorWetnessPage {coordinate,derived:None,pending:None,
                primary:resident::Resident::unwritten(self.create_scalar_page_surface(&self.raster_tiers.paint,"prepared preview wetness primary")),secondary:resident::Resident::unwritten(self.create_scalar_page_surface(&self.raster_tiers.paint,"prepared preview wetness companion")),
                active_secondary:false,primary_needs_clear:false,secondary_needs_clear:false}),
            Output::PreviewCompanion(coordinate)=>{
                let surface=resident::Resident::unwritten(self.create_page_surface(&self.raster_tiers.paint,"prepared preview companion"));
                let page=prepared_raster_output(self.preview_pages.iter_mut().find(|page|page.coordinate==coordinate))?;
                page.secondary=Some(surface);
            },_=>unreachable!(),
        }}
        self.commit_raster_working(reservation);
        self.preview_pages.retain(|page|page.primary.size().width==PAGE_SIZE>>level);
        Ok(())
    }
    fn raster_outputs(&self,packet:FramePacket<'_>,plans:&[Vec<BrushTile>])->(Vec<(SourceTarget,Output)>,Vec<((SourceTarget,u32),paint_transform::Plan)>) {
        let mut outputs=BTreeSet::new();let mut transforms=Vec::new();
        let from_persistent=self.preview_from_persistent(packet.dab_batches);
        let rectangular=from_persistent && !Self::sparse_preview(packet.dab_batches);
        let previews:Vec<_>=packet.dab_batches.iter().zip(plans).map(|(batch,tiles)|Self::preview_pages(packet.dab_batches,batch,tiles,packet.scene.target_extent(batch.target))).collect();
        for ((batch,tiles),coordinates) in packet.dab_batches.iter().zip(plans).zip(&previews) {
            let target=batch.target;
            let coverage=target.is_coverage().then(||packet.scene.mask(packet.scene.source_owner(target).unwrap()).unwrap().1);
            if let DabBatchKind::RasterOperation(index)=batch.kind && let Some(operation)=packet.scene.operations(target).and_then(|operations|operations.get(index as usize))
                && let layer_core::RasterOperationKind::Transform(transform)=&operation.kind {
                let geometry=[if coverage.is_some() {RasterPlane::Mask}else {RasterPlane::Color},RasterPlane::WatercolorWetness].map(|plane|self.content_pages(packet,target,plane,&outputs));
                let base=coverage.is_none().then(||transform.source_base.as_ref().or_else(||self.tiled_sources.get(&target))).flatten();
                let plan=paint_transform::Plan::of(geometry.each_ref().map(|pages|pages.iter().copied()),base,operation.coverage.selection.as_ref(),packet.scene.target_extent(target));
                let [color,wetness]=plan.writes(transform,packet.scene.target_extent(target),&geometry[1]);
                outputs.extend(color.into_iter().map(|c|(target,coverage.map_or(Output::Color(c),|source|Output::Mask(c,source.default_coverage.to_bits())))));
                outputs.extend(wetness.into_iter().map(|c|(target,Output::Wetness(c))));
                transforms.push(((target,index),plan));continue;
            }
            if let Some(source)=coverage {
                for c in page_coordinates(batch_pixel_rect(batch,source.domain)) {outputs.insert((target,Output::Mask(c,source.default_coverage.to_bits())));}
                continue;
            }
            let pass=BrushPassPlan::for_device(&batch.style,&self.device);
            let destinations=brush_tiles::destinations(tiles,batch_pixel_rect(batch,packet.scene.target_extent(target)),
                batch.kind==DabBatchKind::Preview && rectangular,batch.first_dab);
            if batch.kind==DabBatchKind::Persistent {
                for tile in destinations.iter() {
                    let c=tile.coordinate;outputs.insert((target,Output::Color(c)));
                    if pass.state.coverage {outputs.insert((target,Output::Coverage(c,stroke_coverage_slabs(&batch.style),stable_blender(&batch.style))));}
                    if pass.state.watercolor_wetness {outputs.insert((target,Output::Wetness(c)));}
                }
            }
            for coordinate in coordinates {
                outputs.insert((target,Output::PreviewColor(*coordinate)));
                if pass.state.watercolor_wetness {outputs.insert((target,Output::PreviewWetness(*coordinate)));}
            }
            if !from_persistent {
                for coordinate in page_coordinates(batch_pixel_rect(batch,packet.scene.target_extent(target))).filter(|coordinate|coordinates.contains(coordinate)) {
                    if pass.state.coverage {outputs.insert((target,Output::PreviewCoverage(coordinate,stroke_coverage_slabs(&batch.style),stable_blender(&batch.style))));}
                    if batch.style.execution!=BrushExecution::Liquify && pass.requires_destination() {outputs.insert((target,Output::PreviewCompanion(coordinate)));}
                }
            }
            let planned=match batch.kind {
                DabBatchKind::Persistent=>Self::brush_work_planned(batch,tiles,packet.scene.target_extent(target)),
                DabBatchKind::Preview=>!coordinates.is_empty(),
                _=>false,
            };
            if planned && Self::tracks_stroke_start(batch) {
                let source=if batch.kind==DabBatchKind::Preview {BrushEncodingTarget::Preview {from_persistent}}else {BrushEncodingTarget::Persistent};
                for tile in destinations.iter() {outputs.insert((target,Output::StrokeStart(tile.coordinate,batch.stroke_id,source.material_source().is_preview())));}
            }
            if let DabBatchKind::RasterOperation(index)=batch.kind && let Some(operation)=packet.scene.operations(target).and_then(|operations|operations.get(index as usize)) {
                let masking=matches!(operation.kind,layer_core::RasterOperationKind::ColorMode(_)|layer_core::RasterOperationKind::ApplyMask|layer_core::RasterOperationKind::Erase {..});
                if matches!(operation.kind,layer_core::RasterOperationKind::Fill {..}|layer_core::RasterOperationKind::Gradient {..}|layer_core::RasterOperationKind::Figure(_)|layer_core::RasterOperationKind::Bake {..}|layer_core::RasterOperationKind::FrequencyDetail {..})
                    || masking {
                    let content=masking.then(||self.content_pages(packet,target,RasterPlane::Color,&outputs));
                    let damage=batch_pixel_rect(batch,packet.scene.target_extent(target));
                    for c in page_coordinates(damage) {
                        if content.as_ref().is_none_or(|content|content.contains(&c))
                            || packet.scene.paint_base(target).is_some_and(|base|source_access::paint_base_contains(base,c)) {outputs.insert((target,Output::Color(c)));}
                    }
                    if scene::settles(&operation.kind,damage,packet.scene.target_extent(target)) && self.paint_layers.iter().any(|layer|layer.id==target && layer.watercolor.is_some()) {
                        let wet=self.content_pages(packet,target,RasterPlane::WatercolorWetness,&outputs);
                        outputs.extend(wet.into_iter().map(|c|(target,Output::Wetness(c))));
                    }
                }
            }
        }
        if let Some(fill)=&self.fill_preview {
            let damage=pixel_rect(fill.operation.bounds(packet.scene.target_extent(fill.target)),packet.scene.target_extent(fill.target));
            for coordinate in page_coordinates(damage) {outputs.insert((fill.target,Output::PreviewColor(coordinate)));}
        }
        for (batch,_) in packet.dab_batches.iter().zip(&previews).filter(|(batch,coordinates)|!coordinates.is_empty()
            && BrushPassPlan::for_device(&batch.style,&self.device).state.watercolor_wetness) {
            let extent=packet.scene.target_extent(batch.target);
            let damage=packet.dab_batches.iter().zip(plans).zip(&previews).filter(|((other,_),coordinates)|!coordinates.is_empty() && other.target==batch.target)
                .fold(PixelRect::EMPTY,|damage,((other,tiles),_)|damage.union(Self::brush_display_damage(other,tiles,extent)));
            for coordinate in page_coordinates(damage.expand(WatercolorLayerStyle::from_dab_style(&batch.style).radius(),extent)) {
                outputs.insert((batch.target,Output::PreviewWetness(coordinate)));
            }
        }
        for (target,c) in self.destination_pages(packet.dab_batches,plans) {
            outputs.insert((target,Output::Color(c)));
            outputs.insert((target,Output::Companion(c)));
        }
        (outputs.into_iter().collect(),transforms)
    }
    fn content_pages(&self,packet:FramePacket<'_>,target:SourceTarget,plane:RasterPlane,planned:&BTreeSet<(SourceTarget,Output)>)->BTreeSet<[u32;2]> {
        let data=[packet.scene.raster(target),packet.restore_rasters.iter().find(|(id,_)|*id==target).map(|(_,root)|root)].into_iter().flatten()
            .filter_map(|root|root.try_data().and_then(Result::ok))
            .chain(self.raster.as_ref().and_then(|runtime|runtime.targets.get(&target)).map(|current|current.contents.data().clone()))
            .chain(self.native_backing(target).cloned()).collect::<Vec<_>>();
        let layer=self.paint_layers.iter().find(|layer|layer.id==target);
        let resident=match (plane,layer) {
            (RasterPlane::Mask,_)=>self.layer_masks.pages.keys().filter(|(id,_)|*id==target).map(|(_,c)|*c).collect(),
            (RasterPlane::Color,Some(layer))=>layer.pages.iter().map(|page|page.coordinate).collect(),
            (RasterPlane::WatercolorWetness,Some(layer))=>layer.watercolor_wetness_pages.iter().map(|page|page.coordinate).collect(),
            _=>Vec::new(),
        };
        resident.into_iter().chain(data.iter().flat_map(|data|data.tiles.keys()).filter(|key|key.plane==plane).map(|key|key.coordinate))
            .chain(planned.iter().filter(|(id,_)|*id==target).filter_map(|(_,output)|output.native_key()).filter(|key|key.plane==plane).map(|key|key.coordinate))
            .collect()
    }
    pub(crate) fn retire_restore(&mut self) {
        if let Some(scene)=&mut self.scene {scene.release_operation_scratch();}
        let bytes=self.staged_restore_bytes();
        if bytes==0 {self.restore_preflight=None;return;}
        let restore=self.restore_preflight.take().unwrap();
        let mut pages=restore.targets.into_iter().flatten().map(|target|target.pages).collect::<Vec<_>>();
        let mut starts=restore.starts;
        let charge=DisplayRetirement {_charge:self.raster_buffers.transfer.charge(bytes),#[cfg(not(target_arch="wasm32"))] _scratch:(Vec::new(),Vec::new())};
        pages.iter_mut().for_each(PreparedPages::discharge);starts.iter_mut().for_each(|(_,_,_,_,page)|page.discharge());
        #[cfg(not(target_arch="wasm32"))]
        let retired=self.device.retirement((pages,starts,charge));
        #[cfg(target_arch="wasm32")]
        let retired={drop((pages,starts));charge};
        self.device.on_queue_done(&self.queue,move|| {#[cfg(not(target_arch="wasm32"))] retired.complete();#[cfg(target_arch="wasm32")] drop(retired);});
    }
    pub(crate) fn take_prepared_stroke_start(&mut self,target:SourceTarget,coordinate:[u32;2],stroke:layer_core::StrokeId,preview:bool)->Option<resident::Resident<PageSurface>> {
        let starts=&mut self.restore_preflight.as_mut()?.starts;
        let index=starts.iter().position(|(id,c,old,p,_)|*id==target && *c==coordinate && *old==stroke && *p==preview)?;
        Some(starts.swap_remove(index).4)
    }
    pub(crate) fn admit_preparation_bytes(&mut self,restore:u64,bake:u64)->Result<(),GpuRasterError> {
        self.admit_preparation_with_transforms(restore,bake,None)
    }
    fn retained_preparation_bytes(&self,transforms:Option<&paint_transform::PaintTransforms>)->Result<u64,GpuRasterError> {
        let mut textures=RetainedTextures::default();
        let mut bytes=0u64;
        for layer in &self.paint_layers {
            for page in &layer.pages {
                for surface in std::iter::once(&page.primary).chain(page.secondary.iter()).chain(page.stroke_start.iter().map(|start|&start.surface)) {
                    if surface.account_texture(&mut textures) {bytes=bytes.checked_add(surface.storage_bytes()).ok_or(GpuRasterError::SizeOverflow)?;}
                }
            }
            for page in &layer.watercolor_wetness_pages {
                for surface in [&page.primary,&page.secondary] {
                    if surface.account_texture(&mut textures) {bytes=bytes.checked_add(surface.storage_bytes()).ok_or(GpuRasterError::SizeOverflow)?;}
                }
            }
        }
        if let Some(commit)=self.restore_preflight.as_ref().and_then(|restore|restore.commit.as_ref()) {
            bytes=bytes.checked_add(commit.preparation_bytes(&mut textures)).ok_or(GpuRasterError::SizeOverflow)?;
        }
        let mut masks=std::collections::BTreeSet::new();
        for page in self.layer_masks.pages.values() {if masks.insert(page.allocation_identity()) {bytes=bytes.checked_add(page.storage_bytes()).ok_or(GpuRasterError::SizeOverflow)?;}}
        bytes=bytes.checked_add(self.scale_display.as_ref().map_or(0,|cache|cache.resident_bytes()))
            .and_then(|bytes|bytes.checked_add(transforms.or(self.transforms.as_ref()).map_or(0,|transforms|transforms.preparation_bytes(&mut textures))))
            .ok_or(GpuRasterError::SizeOverflow)?;
        Ok(bytes.saturating_add(self.staged_restore_bytes()))
    }
    pub(crate) fn admit_preparation_with_transforms(&mut self,restore:u64,bake:u64,transforms:Option<&paint_transform::PaintTransforms>)->Result<(),GpuRasterError> {
        if self.native_edit.is_none() {return Ok(());}
        self.refresh_preparation_memory();
        let [scene, paint] = [self.memory.display_grant(), self.memory.raster_grant()];
        let limit=scene.checked_add(paint).ok_or(GpuRasterError::SizeOverflow)?
            .saturating_sub(self.retired_display_bytes.used());
        let projected=restore.checked_add(bake).ok_or(GpuRasterError::SizeOverflow)?;
        let mut retained=self.retained_preparation_bytes(transforms)?;
        let mut bytes=projected.checked_add(retained).ok_or(GpuRasterError::SizeOverflow)?;
        if bytes>limit.max(retained) {
            if self.factory.device_memory_limit().is_some_and(|limit|projected.saturating_add(self.pinned_paint_bytes()).saturating_add(self.source_tiles.borrow().non_slot_bytes())>limit) {
                self.preparation_memory(0);
                return Err(GpuRasterError::SourceWorkingSetExceeded);
            }
            let encoder=submission::CommandEncoder::new(&self.device,&Default::default());
            let trim=bytes>limit.max(retained) && !transforms.is_some_and(|transforms|transforms.has_preview());
            let (mut pages,leases)=if trim {self.evict_color(u64::MAX,&[],&[])} else {Default::default()};
            let retired=pages.iter().map(PageSurface::storage_bytes).sum::<u64>();
            if trim {
                retained=self.retained_preparation_bytes(transforms)?;
                bytes=projected.checked_add(retained).ok_or(GpuRasterError::SizeOverflow)?;
            }
            if retired!=0 {
                let retirement=self.retired_display_bytes.charge(retired);
                pages.iter_mut().for_each(PageSurface::discharge);
                let charge=DisplayRetirement {_charge:retirement,
                    #[cfg(not(target_arch="wasm32"))] _scratch:(pages,Vec::new())};
                #[cfg(not(target_arch="wasm32"))]
                let wake=self.preparation_wake.clone();
                encoder.retire((charge,leases));
                #[cfg(not(target_arch="wasm32"))]
                if let Some(wake)=wake {encoder.on_submitted_work_done(move||wake());}
                self.last_submission=encoder.submit(&self.queue);
                self.publish_memory();
            }
            self.preparation_memory(bytes);
            self.factory.memory_budget().flush_reports();
            let [scene, raster] = self.preparation_memory_limits();
            let retiring=self.retired_display_bytes.used();
            let fresh = scene.checked_add(raster).ok_or(GpuRasterError::SizeOverflow)?.saturating_sub(retiring);
            if bytes>fresh.max(retained) && retiring==0 {
                self.preparation_memory(0);
                return Err(GpuRasterError::SourceWorkingSetExceeded);
            }
            Err(GpuRasterError::NotReady)
        } else {Ok(())}
    }
    pub(crate) fn staged_restore_bytes(&self)->u64 {
        self.restore_preflight.as_ref().map_or(0,|restore|restore.targets.iter().flatten().map(|target|target.pages.storage_bytes()).sum::<u64>()
            +restore.starts.iter().map(|(_,_,_,_,page)|page.storage_bytes()).sum::<u64>())
    }
    pub(crate) fn staged_restore_mask_bytes(&self)->u64 {
        self.restore_preflight.as_ref().map_or(0,|restore|restore.targets.iter().flatten().flat_map(|target|&target.pages.masks).map(|(_,page)|page.storage_bytes()).sum())
    }
    pub(crate) fn projected_restore_bytes(&self)->u64 {self.restore_preflight.as_ref().map_or(0,|restore|restore.targets.iter().flatten()
        .try_fold(self.unreserved_preview_bytes(restore),|bytes,target|bytes.checked_add(target.projected_bytes(self)?).ok_or(GpuRasterError::SizeOverflow)).unwrap_or(u64::MAX))}
    fn unreserved_preview_bytes(&self,restore:&RestorePreflight)->u64 {
        if restore.preview.is_some() {return 0;}
        restore.outputs().iter().filter(|(_,output)|output.preview() && !output.preview_present(self,self.preview_level)).map(|(_,output)|output.bytes(self,0)).sum()
    }
    pub(crate) fn staged_transform_source(&self,target:SourceTarget)->Option<(&Arc<RasterData>,&PreparedPages)> {
        self.restore_preflight.as_ref()?.targets.iter().flatten().find(|entry|entry.replace && entry.target==target && entry.complete()).map(|entry|(&entry.data,&entry.pages))
    }
    pub(crate) fn cancels_transform_preview(&self,packet:FramePacket<'_>)->bool {
        self.transforms.as_ref().is_some_and(|transforms|transforms.has_preview())
            && if self.transform_preview.is_none() {!self.has_transform_commit()}else {!packet.dab_batches.is_empty()}
    }
    pub(crate) fn prepare_transform_cancel(&mut self,packet:FramePacket<'_>)->bool {
        if self.snapshot_worker || !self.cancels_transform_preview(packet) {return true;}
        let mut transforms=self.transforms.take().unwrap();
        let result=self.record_restore_uploads(|r,encoder|transforms.prepare_cancel(r,encoder));
        self.transforms=Some(transforms);
        match result {
            Ok(())=>true,
            Err(error) if error.is_preparation_pending()=>false,
            Err(error)=>{if let Some(restore)=&mut self.restore_preflight {restore.failure=Some(error);}true},
        }
    }
    pub(crate) fn prepare_operation_masks(&mut self,packet:FramePacket<'_>)->bool {
        let operations:Vec<_>=packet.dab_batches.iter().filter_map(|batch| {
            let DabBatchKind::RasterOperation(index)=batch.kind else {return None;};
            let operation=packet.scene.operations(batch.target)?.get(index as usize)?;
            scene::reads_coverage_pages(&operation.kind).then(||(batch.target,index,operation,batch_pixel_rect(batch,packet.scene.target_extent(batch.target))))
        }).collect();
        if self.snapshot_worker || operations.is_empty() {return true;}
        let Some(mut scene)=self.scene.take() else {return true;};
        let result=self.record_restore_uploads(|r,encoder| {
            r.drain_image_decodes(encoder)?;
            let mut ready=true;
            for (target,index,operation,damage) in operations {
                let accepted=scene.declare_operation_masks(r,packet,target,index,operation,damage)?.accept_lane(r)?;
                ready&=accepted.prepare(r)?;
                accepted.track(encoder);
                let outputs=r.restore_preflight.as_ref().map_or(&[][..],RestorePreflight::outputs).iter()
                    .filter_map(|(id,output)|match output {Output::Color(page) if *id==target=>Some(*page),_=>None});
                scene.hold_settling_scratch(r,target,&operation.kind,damage,packet.scene.target_extent(target),outputs)?;
            }
            Ok(ready)
        });
        self.scene=Some(scene);
        result.unwrap_or_else(|error|!error.is_preparation_pending())
    }
    pub(crate) fn prepare_transform_preview(&mut self,packet:FramePacket<'_>,preview:Option<&layer_render::TransformPreview>)->bool {
        if self.snapshot_worker {return true;}
        let Some(preview)=preview else {
            self.transforms.as_mut().unwrap().cancel_preparation();return true;
        };
        let request=match scene::scale::request(self,packet) {Ok(request)=>request,Err(_)=>return true};
        if request.plan.level==0 || request.evaluation==scene::scale::Evaluation::Native {self.transforms.as_mut().unwrap().cancel_preparation();return true;}
        let pipelines=if request.evaluation.fine_capacity().is_some() {&self.scene_pipelines.fine_resample} else {&self.scene_pipelines.resample};
        let entries:&[usize]=if preview.transform.placement.mesh.is_some() {&[0,1,2,3]} else {&[4,5]};
        if !self.compilation.require(entries.iter().map(|&i|&pipelines.mesh[i]),startup::BRUSH) {return false;}
        let mut transforms=self.transforms.take().unwrap();
        let result=self.record_restore_uploads(|r,encoder|transforms.prepare_preview(r,encoder,preview,packet.scene,request.plan.level,request.evaluation.fine_capacity().is_some()));
        self.transforms=Some(transforms);
        match result {
            Ok(())=>true,
            Err(error) if error.is_preparation_pending()=>false,
            Err(error)=>{if let Some(restore)=&mut self.restore_preflight {restore.failure=Some(error);}true},
        }
    }
    pub(crate) fn record_restore_uploads<T>(&mut self,work:impl FnOnce(&mut Self,&mut submission::CommandEncoder)->Result<T,GpuRasterError>)->Result<T,GpuRasterError> {
        let mut encoder=submission::CommandEncoder::new(&self.device,&Default::default());
        std::mem::swap(&mut self.uploads,&mut self.restore_uploads);
        let result=work(self,&mut encoder);
        if encoder.has_work() {
            self.preparation_progress=self.preparation_progress.saturating_add(1);
            #[cfg(not(target_arch="wasm32"))]
            if let Some(wake)=self.preparation_wake.clone() {encoder.on_submitted_work_done(move||wake());}
            self.uploads.finish(&encoder);self.last_submission=encoder.submit(&self.queue);self.metrics.native_restore_submissions+=1;
        }
        std::mem::swap(&mut self.uploads,&mut self.restore_uploads);result
    }
    fn restore_pipelines_ready(&self,kinds:startup::SourceKinds)->bool {
        self.compilation.require(kinds.pipelines(&self.scene_pipelines.source),startup::VALIDATION)
    }
    pub(crate) fn prepare_raster_uploads(&mut self,requests:&[(Arc<TileBlob>,layer_core::color::RgbSpace,layer_core::color::RgbSpace)])->Result<scene::sources::AcceptedInputs,GpuRasterError> {
        let mut kinds=startup::SourceKinds::default();
        let mut requested=scene::sources::Inputs::default();
        for (blob,source,destination) in requests {
            kinds.include(startup::SourceKinds::for_descriptor(blob.descriptor));requested.raster(blob.clone(),*source,*destination);
        }
        if !self.restore_pipelines_ready(kinds) {return Err(GpuRasterError::NotReady);}
        let inputs=requested.accept_query(self,None)?;
        inputs.prepare(self)?;
        let ready=self.record_restore_uploads(|r,encoder| {
            scene::sources::Lease::track(inputs.iter().cloned().collect(),encoder);
            r.drain_image_decodes(encoder)?;let mut ready=true;
            for (blob,source,destination) in requests {
                let read=scene::sources::accepted_raster(&inputs,blob,*source,*destination)?;
                ready&=read.encode(r,encoder,crate::residency::Class::Authored)?;
            }
            Ok(ready)
        })?;
        if ready {Ok(inputs)}else {Err(GpuRasterError::NotReady)}
    }
    pub(crate) fn note_restore_admission<T>(&mut self,admitted:&Result<T,crate::residency::Blocked>) {
        if admitted.is_err() {self.scene_tiers.display.heap().begin_frame();}
        if let Some(restore)=&mut self.restore_preflight {restore.refused=admitted.as_ref().err().cloned();}
    }
    pub(crate) fn restore_preparation(&self)->Option<crate::frame_demand::Outstanding> {
        use crate::frame_demand::{Input,Outstanding};
        let restore=self.restore_preflight.as_ref().filter(|restore|restore.waiting())?;
        Some(if restore.refused.is_some() && self.lane_waits() {Outstanding::Blocked(Input::Residency)}
            else if self.image_decode_waiting() {Outstanding::Blocked(Input::Decode)}else {Outstanding::Runnable})
    }
    pub(crate) fn restore_pages_ready(&self)->bool {self.restore_preflight.as_ref()
        .and_then(|restore|restore.targets.iter().flatten().find(|target|!target.complete())).is_some_and(|target|target.ready(self))}
    fn replace_restore(&mut self,key:Key,requests:Vec<Request>,commit:Option<paint_transform::PreparedTransformCommit>,transforms:Vec<((SourceTarget,u32),paint_transform::Plan)>) {
        if self.restore_preflight.as_ref().is_none_or(|restore|restore.key!=key || restore.consumed) {
            self.retire_restore();
            self.restore_preflight=Some(RestorePreflight::new(self,key,requests,commit,transforms));
        }
    }
    fn accept_restore_inputs(&mut self)->Result<bool,GpuRasterError> {
        self.adopt_preview_working();
        let Some(index)=self.restore_preflight.as_ref().and_then(|restore|restore.targets.iter().position(|target|target.as_ref().is_some_and(|target|!target.complete()))) else {return Ok(self.reserve_preview_outputs());};
        let target=self.restore_preflight.as_ref().unwrap().targets[index].as_ref().unwrap();
        let first=target.next;
        let remaining=PREFETCH-usize::from(!matches!(target.pending[first],SourcePage::Output(_)));
        let indices=std::iter::once(first).chain(target.pending.iter().enumerate().skip(first)
            .filter(|(index,source)|*index!=first && !matches!(source,SourcePage::Output(_))).take(remaining).map(|(index,_)|index)).collect::<Vec<_>>();
        for at in indices {
            let target=self.restore_preflight.as_ref().unwrap().targets[index].as_ref().unwrap();
            let Some(inputs)=target.pending[at].inputs(self.document_color().space)? else {break;};
            if target.pages.storage_bytes()!=0 && target.inputs.contains(&inputs) {continue;}
            let output_bytes=target.projected_bytes(self)?;
            let accepted=inputs.accept(self,&mut (),|_,r,inputs| {
                let bytes=inputs.additional_bytes(&r.source_tiles.borrow());
                let preview=r.restore_preflight.as_ref().map_or(0,|restore|r.unreserved_preview_bytes(restore));
                let admitted=r.admit_group(output_bytes.saturating_add(preview),bytes).map(|(source,mut held)| {
                    if preview!=0 {r.restore_preflight.as_mut().unwrap().preview=held.split(preview);}
                    (source,(output_bytes!=0).then_some(held))
                });
                r.note_restore_admission(&admitted);
                admitted.ok()
            },|_,r| {
                if output_bytes!=0 {r.record_restore_uploads(|r,encoder| {
                    let mut restore=r.restore_preflight.take().unwrap();
                    restore.targets[index].as_mut().unwrap().allocate(r,encoder)?;
                    r.restore_preflight=Some(restore);Ok(())
                })?;}
                Ok(())
            },|_,r,(_,raster)| {
                if let Some(reservation)=raster {r.commit_raster_working(reservation);}
                Ok(())
            });
            match accepted {
                Ok(Some(((),inputs)))=>self.restore_preflight.as_mut().unwrap().targets[index].as_mut().unwrap().inputs.extend(inputs)?,
                Ok(None)=>break,
                Err(error) if error.is_preparation_pending()=>break,
                Err(error)=>return Err(error),
            }
        }
        let target=self.restore_preflight.as_ref().unwrap().targets[index].as_ref().unwrap();
        Ok(target.pending[first].inputs(self.document_color().space)?.is_some_and(|inputs|
            target.inputs.contains(&inputs) && target.projected_bytes(self).is_ok_and(|bytes|bytes==0)) && self.reserve_preview_outputs())
    }
    fn advance_restore(&mut self)->Result<bool,GpuRasterError> {
        if let Some(target)=self.restore_preflight.as_ref().and_then(|restore|restore.targets.iter().flatten().find(|target|!target.complete()))
            && !self.restore_pipelines_ready(target.source_kinds()) {return Ok(false);}
        if self.restore_preflight.is_none() {return Ok(true);}
        if !self.accept_restore_inputs()? {return Ok(false);}
        let result=self.record_restore_uploads(|r,encoder| {
            r.drain_image_decodes(encoder)?;
            let mut restore=r.restore_preflight.take().unwrap();
            let result=restore.targets.iter_mut().flatten().find(|target|!target.complete()).map_or(Ok(()),|target|target.advance(r,encoder));
            r.restore_preflight=Some(restore);result
        });
        let restore=self.restore_preflight.as_mut().unwrap();
        if let Err(error)=result {
            if error.is_preparation_pending() {return Ok(false);}
            restore.failure=Some(error);
        }
        Ok(restore.complete())
    }
    fn adopt_preview_working(&mut self) {
        let Some(restore)=self.restore_preflight.as_ref() else {return;};
        let bytes=self.unreserved_preview_bytes(restore);
        if bytes==0 {return;}
        if let Some(held)=self.preview_working.take().filter(|held|held.bytes()>=bytes) {self.restore_preflight.as_mut().unwrap().preview=Some(held);}
    }
    fn reserve_preview_outputs(&mut self)->bool {
        let Some(restore)=self.restore_preflight.as_ref() else {return true;};
        let bytes=self.unreserved_preview_bytes(restore);
        if bytes==0 {return true;}
        let admitted=self.admit_raster_working(bytes);
        self.note_restore_admission(&admitted);
        self.restore_preflight.as_mut().unwrap().preview=admitted.ok();self.restore_preflight.as_ref().unwrap().preview.is_some()
    }
    fn restore_ready(&mut self)->bool {
        match self.advance_restore() {
            Ok(ready)=>ready,
            Err(error) if error.is_preparation_pending()=>false,
            Err(error)=>{self.restore_preflight.as_mut().unwrap().failure=Some(error);true},
        }
    }
    pub(crate) fn hold_restore_scalar(&mut self,prepared:object_image_mips::PreparedRasterTile) {
        if let Some(target)=self.restore_preflight.as_mut().and_then(|restore|restore.targets.iter_mut().flatten().find(|target|!target.complete()))
            && target.awaits(&prepared.blob) && target.held(&prepared.blob).is_none() {target.scalars.push(prepared);}
    }
    pub(crate) fn prepare_raster_restores(&mut self,packet:FramePacket<'_>)->bool {
        if self.snapshot_worker && packet.dab_batches.is_empty() {return self.raster_restore_backing_ready(packet);}
        let mut plans=match brush_tiles::plan_packet(packet,|target|packet.scene.target_extent(target)) {Ok(plans)=>plans,Err(_)=>return true,};
        liquify::Liquify::extend_required_plans(self,packet,&mut plans);
        self.prepare_raster_outputs(packet,&plans)
    }
    pub(crate) fn prepare_brush_inputs(&mut self,preview:Option<SourceTarget>,contact:Option<Arc<scene::PreviewStamps>>)
        ->Result<scene::sources::AcceptedInputs,GpuRasterError> {
        let held=contact.as_ref().and_then(|stamps|self.held_brush.take().filter(|held|Arc::ptr_eq(&held.stamps,stamps)));
        let restore=self.restore_preflight.as_ref().ok_or(GpuRasterError::Invariant("brush footprint is not prepared"))?;
        let Key::Frame {required,outputs,..}=&restore.key else {return Err(GpuRasterError::Invariant("brush footprint is not a frame"));};
        let mut coordinates:BTreeSet<_>=required.iter().flat_map(|(target,coordinates)|coordinates.iter().map(move |coordinate|(*target,*coordinate))).collect();
        coordinates.extend(outputs.iter().map(|(target,output)|(*target,output.coordinate())));
        coordinates.retain(|(target,_)|!target.is_coverage());
        let (mut residents,mut state,mut tiles)=held.map_or_else(Default::default,|held|(Arc::unwrap_or_clone(held.residents),Arc::unwrap_or_clone(held.state),held.tiles));
        if tiles.keys().any(|tile|!coordinates.contains(tile)) {
            tiles.retain(|tile,_|coordinates.contains(tile));
            residents.retain(|(target,_,coordinate),_|coordinates.contains(&(*target,*coordinate)));
            state.retain(|(target,coordinate,_),_|coordinates.contains(&(*target,*coordinate)));
        }
        let mut identity=Vec::new();
        for (target,coordinate) in coordinates {
            if contact.is_some() {
                identity.clear();
                self.visit_brush_tile(restore,outputs,preview,target,coordinate,&mut |slot,resident|identity.push((slot,resident.map_or(0,BrushReference::identity))))?;
                if tiles.get(&(target,coordinate)).is_some_and(|held|*held==identity) {continue;}
                residents.remove(&(target,RasterPlane::Color,coordinate));residents.remove(&(target,RasterPlane::WatercolorWetness,coordinate));
                state.retain(|(id,at,_),_|(*id,*at)!=(target,coordinate));
                tiles.insert((target,coordinate),identity.clone());
            }
            #[cfg(test)] crate::test_support::PassCounts {brush_acceptances:1,..Default::default()}.record();
            self.visit_brush_tile(restore,outputs,preview,target,coordinate,&mut |slot,resident| {
                let (entry,predicted)=match slot {
                    BrushSlot::Color(predicted)=>(residents.entry((target,RasterPlane::Color,coordinate)).or_default(),predicted),
                    BrushSlot::Wetness(predicted)=>(residents.entry((target,RasterPlane::WatercolorWetness,coordinate)).or_default(),predicted),
                    BrushSlot::State(kind,predicted)=>(state.entry((target,coordinate,kind)).or_default(),predicted),
                };
                let family=if predicted {entry.preview.get_or_insert_with(Vec::new)}else {&mut entry.persistent};
                family.extend(resident.map(BrushReference::reference));
            })?;
        }
        let (residents,state)=(Arc::new(residents),Arc::new(state));
        if let Some(stamps)=contact {self.held_brush=Some(HeldBrush {stamps,residents:residents.clone(),state:state.clone(),tiles});}
        #[cfg(test)] crate::test_support::PassCounts {material_neighborhoods:1,..Default::default()}.record();
        Ok(scene::sources::AcceptedInputs::prepare_brush(&BrushPreparation(()),residents,state))
    }
    fn visit_brush_tile(&self,restore:&RestorePreflight,outputs:&[(SourceTarget,Output)],preview:Option<SourceTarget>,target:SourceTarget,coordinate:[u32;2],
        visit:&mut dyn FnMut(BrushSlot,Option<&dyn BrushReference>))->Result<(),GpuRasterError> {
        let layer=self.paint_layers.iter().find(|layer|layer.id==target);
        let predicted=preview==Some(target);
        let persistent=layer.and_then(|layer|layer.pages.iter().find(|page|page.coordinate==coordinate));
        let preview=self.preview_pages.iter().find(|page|predicted && page.coordinate==coordinate);
        visit(BrushSlot::Color(false),None);
        if let Some(page)=persistent {
            for surface in std::iter::once(&page.primary).chain(page.secondary.as_ref()) {visit(BrushSlot::Color(false),Some(surface));}
        }
        if predicted && outputs.binary_search(&(target,Output::PreviewColor(coordinate))).is_ok() {
            let page=preview.ok_or(GpuRasterError::Invariant("preview brush input is not allocated"))?;
            visit(BrushSlot::Color(true),None);
            for surface in std::iter::once(&page.primary).chain(page.secondary.as_ref()) {visit(BrushSlot::Color(true),Some(surface));}
        }
        visit(BrushSlot::Wetness(false),None);
        if let Some(page)=layer.and_then(|layer|layer.watercolor_wetness_pages.iter().find(|page|page.coordinate==coordinate)) {
            for surface in [&page.primary,&page.secondary] {visit(BrushSlot::Wetness(false),Some(surface));}
        }
        if predicted && outputs.binary_search(&(target,Output::PreviewWetness(coordinate))).is_ok() {
            let page=self.preview_watercolor_wetness_pages.iter().find(|page|page.coordinate==coordinate)
                .ok_or(GpuRasterError::Invariant("preview wetness input is not allocated"))?;
            visit(BrushSlot::Wetness(true),None);
            for surface in [&page.primary,&page.secondary] {visit(BrushSlot::Wetness(true),Some(surface));}
        }
        let coverage=layer.into_iter().flat_map(|layer|&layer.coverage_pages).map(|page|(page,false))
            .chain(self.preview_coverage_pages.iter().filter(|_|predicted).map(|page|(page,true)));
        for (page,predicted) in coverage.filter(|(page,_)|page.coordinate==coordinate) {
            let kind=scene::sources::BrushState::Coverage(page.primary.size().height/PAGE_SIZE,page.primary.format()==wgpu::TextureFormat::Rgba32Float);
            visit(BrushSlot::State(kind,predicted),None);
            for surface in [&page.primary,&page.secondary] {visit(BrushSlot::State(kind,predicted),Some(surface));}
        }
        for (page,predicted) in persistent.map(|page|(page,false)).into_iter().chain(preview.map(|page|(page,true))) {
            if let Some(start)=&page.stroke_start {visit(BrushSlot::State(scene::sources::BrushState::Start(start.state.stroke),predicted),Some(&start.surface));}
        }
        for (_,_,stroke,predicted,surface) in restore.starts.iter().filter(|(id,at,..)|*id==target && *at==coordinate) {
            visit(BrushSlot::State(scene::sources::BrushState::Start(*stroke),*predicted),Some(surface));
        }
        visit(BrushSlot::State(scene::sources::BrushState::Reservoir,false),None);
        for surface in [&self.reservoir.primary,&self.reservoir.secondary] {visit(BrushSlot::State(scene::sources::BrushState::Reservoir,false),Some(surface));}
        Ok(())
    }
    pub(crate) fn prepare_raster_outputs(&mut self,packet:FramePacket<'_>,plans:&[Vec<BrushTile>])->bool {
        if let Some(batch)=packet.dab_batches.last() {self.live_stroke=Some(batch.stroke_id);}
        let retained=source_access::retained_targets(packet.scene);
        let reset=packet.reset_layers||packet.document_extent!=self.document_extent;
        let mut required:BTreeMap<SourceTarget,BTreeSet<[u32;2]>>=BTreeMap::new();
        for (index,(batch,tiles)) in packet.dab_batches.iter().zip(plans).enumerate() {
            if matches!(batch.target,SourceTarget::Paint(_)) {
                let coordinates=required.entry(batch.target).or_default();
                coordinates.extend(tiles.iter().flat_map(|tile|source_access::neighborhood(tile.coordinate,self.material_offsets(batch))));
                coordinates.extend(tiles.iter().map(|tile|tile.coordinate));
                if BrushPassPlan::for_device(&batch.style,&self.device).state.watercolor_wetness && is_last_watercolor_update_batch(packet.dab_batches,index,packet.scene.target_extent(batch.target)) {
                    coordinates.extend(watercolor_update_damages(packet.dab_batches,index,packet.scene.target_extent(batch.target)).into_iter()
                        .flat_map(page_coordinates).flat_map(|coordinate|source_access::neighborhood(coordinate,[[0,0],[-1,0],[1,0],[0,-1],[0,1]].map(Some))));
                }
                if let DabBatchKind::RasterOperation(index)=batch.kind {
                    let extent=packet.scene.target_extent(batch.target);let bounds=batch_pixel_rect(batch,extent);
                    let bounds=packet.scene.operations(batch.target).and_then(|operations|operations.get(index as usize))
                        .map_or(bounds,|operation|scene::operation_read_bounds(operation,bounds,extent));
                    coordinates.extend(page_coordinates(bounds));
                }
            }
        }
        for batch in packet.dab_batches.iter().filter(|batch|batch.target.is_coverage()) {
            required.entry(batch.target).or_default().extend(page_coordinates(batch_pixel_rect(batch,packet.scene.target_extent(batch.target))));
        }
        if let Some(fill)=&self.fill_preview {
            let extent=packet.scene.target_extent(fill.target);
            let bounds=scene::operation_read_bounds(&fill.operation,pixel_rect(fill.operation.bounds(extent),extent),extent);
            required.entry(fill.target).or_default().extend(page_coordinates(bounds));
        }
        liquify::Liquify::required_sources(self,packet,plans,&mut required);
        self.required_retouch_sources(packet,plans,&mut required);
        for batch in packet.dab_batches {
            if let DabBatchKind::RasterOperation(index)=batch.kind
                && let Some(operation)=packet.scene.operations(batch.target).and_then(|operations|operations.get(index as usize))
                && let layer_core::RasterOperationKind::FrequencyDetail {low,..}=&operation.kind {
                required.entry(SourceTarget::Paint(*low)).or_default().extend(page_coordinates(batch_pixel_rect(batch,packet.scene.target_extent(batch.target))));
            }
        }
        let roots=retained.iter().filter_map(|(target,scene)| {
            let root=scene.raster(*target)?;
            Some(FrameRoot {target:*target,revision:root.identity(),published:root.try_data().and_then(Result::ok).map(|data|Arc::as_ptr(&data) as usize),
                restore:packet.restore_rasters.iter().find(|(id,_)|id==target).map(|(_,root)|root.identity()),extent:scene.target_extent(*target)})
        }).collect();
        let bases=retained.iter().filter_map(|(target,scene)|scene.paint_base(*target).map(|base|(*target,Arc::as_ptr(base.image.storage()) as usize,base.offset))).collect();
        let (outputs,transforms)=self.raster_outputs(packet,plans);
        for (target,output) in outputs.iter().filter(|(_,output)|!output.preview()) {required.entry(*target).or_default().insert(output.coordinate());}
        if self.preview_from_persistent(packet.dab_batches) && !Self::sparse_preview(packet.dab_batches) {
            for batch in packet.dab_batches.iter().filter(|batch|batch.kind==DabBatchKind::Preview && matches!(batch.target,SourceTarget::Paint(_))) {
                let offsets=self.material_offsets(batch);
                required.entry(batch.target).or_default().extend(rectangular_preview_pages(&outputs,batch,packet.scene.target_extent(batch.target))
                    .flat_map(|coordinate|source_access::neighborhood(coordinate,offsets)));
            }
        }
        let clear_starts=packet.dab_batches.iter().zip(plans).any(|(batch,tiles)|self.clears_stroke_starts(batch,tiles,packet.scene.target_extent(batch.target)));
        let key=Key::Frame {color:self.document_color(),extent:packet.document_extent,reset,clear_starts,roots,bases,
            required:required.iter().map(|(target,coordinates)|(*target,coordinates.iter().copied().collect())).collect(),outputs,
            restored:if self.cancels_transform_preview(packet) {self.transforms.as_ref().unwrap().cancel_restores(self)}else {Vec::new()}};
        if self.restore_preflight.as_ref().is_some_and(|restore|restore.key==key && !restore.consumed && restore.commit.as_ref().is_none_or(|commit|commit.matches(self,packet))) {
            if !self.prepare_native_admission_for_frame(packet) {return false;}
            self.refresh_memory_limits();return self.restore_ready();}
        self.retire_restore();
        let commit=match self.transforms.as_ref().unwrap().prepare_commit(self,packet) {
            Ok(commit)=>commit,Err(error)=>{self.restore_preflight=Some(RestorePreflight {key,commit:None,native:None,targets:Vec::new(),failure:Some(error),consumed:false,starts:Vec::new(),transforms,preview:None,refused:None});return true;},
        };
        let mut requests=Vec::new();
        for (target,scene) in retained {
            let Some(root)=scene.raster(target) else {continue;};
            let current=self.raster.as_ref().and_then(|runtime|runtime.targets.get(&target));
            let explicit=packet.restore_rasters.iter().find(|(id,_)|*id==target).map(|(_,root)|root);
            let required=required.remove(&target).unwrap_or_default();
            let replace=reset || explicit.is_some() || current.is_none_or(|current|current.revision!=*root);
            let wanted=match root.try_data() {
                Some(Ok(data))=>Some(data),Some(Err(_))=>return true,None=>None,
            };
            let resolved=wanted.is_some();
            let data=if let Some(data)=wanted {
                if !replace && required.is_empty() {continue;}
                data
            } else if let Some(root)=explicit {
                match root.try_data() {Some(Ok(data))=>data,Some(Err(_))=>return true,None=>return false,}
            } else if reset && let Some(current)=current {current.contents.data().clone()
            } else if !required.is_empty() {current.map_or_else(||Arc::new(RasterData::default()),|current|current.contents.data().clone())}else {continue;};
            let replace=if let Some(current)=current {reset || explicit.is_some() || resolved && current.revision!=*root}else {!data.tiles.is_empty()};
            if !replace && required.is_empty() {continue;}
            let before=if reset || !replace {RasterData::default()}else {current.map_or_else(RasterData::default,|current|current.contents.restorable_data())};
            requests.push(Request {target,extent:scene.target_extent(target),before,data,required,base:scene.paint_base(target).cloned(),replace});
        }
        self.replace_restore(key,requests,commit,transforms);
        if !self.prepare_native_admission_for_frame(packet) {return false;}
        self.refresh_memory_limits();self.restore_ready()
    }
    fn prepare_native_admission_for_frame(&mut self,packet:FramePacket<'_>)->bool {
        if self.native_edit.is_none() || self.restore_preflight.as_ref().is_some_and(|restore|restore.native.is_some()) {return true;}
        let outputs=match self.restore_preflight.as_ref().map(|restore|&restore.key) {
            Some(Key::Frame {roots,..}) if roots.iter().all(|root|root.published.is_some())=>return true,
            Some(Key::Frame {outputs,..})=>outputs.clone(),_=>return true,
        };
        match self.prepare_native_admission(packet.scene,&outputs) {
            Ok(admission)=>{self.restore_preflight.as_mut().unwrap().native=Some(admission);true},
            Err(error) if error.is_preparation_pending()=>false,
            Err(error)=>{self.restore_preflight.as_mut().unwrap().failure=Some(error);true},
        }
    }
    pub(crate) fn has_transform_commit(&self)->bool {self.restore_preflight.as_ref().is_some_and(|restore|restore.commit.is_some())}
    pub(crate) fn take_transform_commit(&mut self)->Option<paint_transform::PreparedTransformCommit> {self.restore_preflight.as_mut()?.commit.take()}
    pub(super) fn consume_restore_preflight(&mut self) {if let Some(restore)=&mut self.restore_preflight {restore.consumed=true;}}
    pub(crate) fn take_restore_failure(&mut self)->Option<GpuRasterError> {self.restore_preflight.as_mut().and_then(|restore|restore.failure.take())}
    pub(super) fn validate_restore_publication(&self)->Result<(),GpuRasterError> {
        if let Some(restore)=&self.restore_preflight {
            if !restore.complete() {return Err(GpuRasterError::NotReady);}
            for target in restore.targets.iter().flatten() {
                if !target.target.is_coverage() && !self.paint_layers.iter().any(|layer|layer.id==target.target) {return Err(GpuRasterError::MissingPaintLayer(target.target));}
            }
        }
        Ok(())
    }
    pub(crate) fn validate_raster_outputs(&self)->Result<(),GpuRasterError> {
        let Some(restore)=&self.restore_preflight else {return Ok(());};
        let Key::Frame {outputs,..}=&restore.key else {return Ok(());};
        for (target,output) in outputs {
            let coordinate=output.coordinate();
            let layer=self.paint_layers.iter().find(|layer|layer.id==*target);
            let page=layer.and_then(|layer|layer.pages.iter().find(|page|page.coordinate==coordinate));
            let ready=match output {
                Output::Color(_)=>page.is_some(),
                Output::Companion(_)=>page.is_some_and(|page|page.secondary.is_some()),
                Output::Coverage(_,slabs,rgba)=>layer.is_some_and(|layer|layer.coverage_pages.iter().any(|page|
                    page.coordinate==coordinate && page.primary.size().height==PAGE_SIZE*slabs
                        && (page.primary.format()==wgpu::TextureFormat::Rgba32Float)==*rgba)),
                Output::Wetness(_)=>layer.is_some_and(|layer|layer.watercolor_wetness_pages.iter().any(|page|page.coordinate==coordinate)),
                Output::Mask(..)=>self.layer_masks.pages.contains_key(&(*target,coordinate)),
                Output::StrokeStart(_,stroke,preview)=>!*preview && page.is_none()
                    || page.is_some_and(|page|page.stroke_start.as_ref().is_some_and(|start|start.state.stroke==*stroke))
                    || restore.starts.iter().any(|(id,c,s,p,_)|id==target && *c==coordinate && s==stroke && (*preview || p==preview)),
                output if output.preview()=>output.preview_present(self,self.preview_level),
                _=>unreachable!(),
            };
            prepared_raster_output(ready.then_some(()))?;
        }
        Ok(())
    }
    pub(super) fn publish_prepared_restore(&mut self,target:SourceTarget,data:&Arc<RasterData>)->Result<(),GpuRasterError> {
        if let Some(error)=self.take_restore_failure() {return Err(error);}
        self.validate_restore_publication()?;
        let restore=self.restore_preflight.as_mut().ok_or(GpuRasterError::Invariant("prepared publish prepared restore state is inconsistent"))?;
        let index=restore.targets.iter().position(|entry|entry.as_ref().is_some_and(|entry|entry.replace && entry.target==target && Arc::ptr_eq(&entry.data,data)))
            .ok_or(GpuRasterError::Invariant("prepared publish prepared restore state is inconsistent"))?;
        if !restore.complete() {return Err(GpuRasterError::NotReady);}
        let target=restore.targets[index].take().unwrap();restore.consumed=true;
        self.publish_restored_pages(target.target,&target.selected,target.pages)?;
        if let Some(native)=&mut self.native_edit {native.backing.insert(target.target,data.clone());}
        Ok(())
    }
    pub(super) fn publish_warm_restore_pages(&mut self)->Result<(),GpuRasterError> {
        self.validate_restore_publication()?;
        let Some(restore)=&mut self.restore_preflight else {return Ok(());};
        let outputs=restore.targets.iter_mut().filter_map(|entry|entry.as_ref().is_some_and(|target|!target.replace)
            .then(||entry.take().unwrap())).collect::<Vec<_>>();
        if !outputs.is_empty() {restore.consumed=true;}
        for target in outputs {self.append_prepared_pages(target.target,target.pages)?;}
        Ok(())
    }
    pub(super) fn append_prepared_pages(&mut self,target:SourceTarget,pages:PreparedPages)->Result<(),GpuRasterError> {
        let PreparedPages {colors,masks,wetness,coverage,companions,starts}=pages;
        if !colors.is_empty() || !wetness.is_empty() || !coverage.is_empty() || !companions.is_empty() {
            let layer=self.paint_layers.iter_mut().find(|layer|layer.id==target).ok_or(GpuRasterError::MissingPaintLayer(target))?;
            for page in colors {layer.pages.retain(|old|old.coordinate!=page.coordinate);layer.pages.push(page);}
            for page in wetness {layer.watercolor_wetness_pages.retain(|old|old.coordinate!=page.coordinate);layer.watercolor_wetness_pages.push(page);}
            layer.coverage_pages.extend(coverage);
            for (c,surface) in companions {layer.pages.iter_mut().find(|page|page.coordinate==c).ok_or(GpuRasterError::InvalidExtent)?.secondary=Some(surface);}
        }
        for (c,page) in masks {self.layer_masks.pages.insert((target,c),page);}
        if !starts.is_empty() {self.restore_preflight.as_mut().ok_or(GpuRasterError::InvalidExtent)?.starts.extend(starts.into_iter().map(|(c,id,preview,page)|(target,c,id,preview,page)));}
        Ok(())
    }
    pub(super) fn restore_direct(&mut self,target:SourceTarget,before:&RasterData,data:&RasterData)->Result<(),GpuRasterError> {
        let key=Key::Direct {color:self.document_color(),target,extent:self.target_extent(target),before:DataKey::new(before),after:DataKey::new(data)};
        if self.restore_preflight.as_ref().is_none_or(|restore|restore.key!=key || restore.consumed) {
            self.replace_restore(key,vec![Request {target,extent:self.target_extent(target),before:before.clone(),data:Arc::new(data.clone()),
                required:BTreeSet::new(),base:None,replace:true}],None,Vec::new());
        }
        if !self.advance_restore()? {
            return Err(GpuRasterError::NotReady);
        }
        if let Some(error)=self.take_restore_failure() {return Err(error);}
        let data=self.restore_preflight.as_ref().unwrap().targets.iter().flatten().find(|entry|entry.target==target).unwrap().data.clone();
        self.publish_prepared_restore(target,&data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn restore_pixel(x:u32)->[u8;4] {[32+40*x as u8,64,96,255]}

    fn restore_window(count:u32)->(WgpuRasterizer,Arc<RasterData>) {
        let (mut r,data)=restore_request(count,|color,target,extent,previous,data|Key::Direct {color,target,extent,before:DataKey::new(previous),after:DataKey::new(data)});
        let outputs=r.projected_restore_bytes();
        let reservation=r.admit_raster_working(outputs).unwrap();
        r.record_restore_uploads(|r,encoder| {
            let mut restore=r.restore_preflight.take().unwrap();
            let result=restore.targets[0].as_mut().unwrap().allocate(r,encoder);
            r.restore_preflight=Some(restore);result
        }).unwrap();
        r.commit_raster_working(reservation);
        (r,data)
    }

    fn restore_request(count:u32,key:impl FnOnce(layer_core::color::DocumentColor,SourceTarget,[u32;2],&RasterData,&RasterData)->Key)->(WgpuRasterizer,Arc<RasterData>) {
        let extent=[count*PAGE_SIZE,PAGE_SIZE];
        let document=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
            layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        let color=document.composition().color;let target=document.active_target().unwrap();
        let mut r=crate::memory::memory_tests::renderer(color,layer_core::DeviceMemoryFacts {unified:true,..Default::default()}).unwrap();
        r.ensure_document_metadata(extent,document.scene()).unwrap();
        let tile=|x:u32|RasterTile::backed(TileBlob::encode(RasterPlane::Color.descriptor(color),
            &restore_pixel(x).repeat(PAGE_SIZE.pow(2) as usize)).unwrap());
        let data=Arc::new(RasterData {tiles:(0..count).map(|x|(TileKey {plane:RasterPlane::Color,coordinate:[x,0]},tile(x))).collect(),watercolor:None});
        let warm=tile(count).wait_backing().unwrap();
        pollster::block_on(r.compilation.prepare_all(startup::SourceKinds::for_descriptor(warm.descriptor).pipelines(&r.scene_pipelines.source))).unwrap();
        let pixels=crate::object_image_mips::prepare_raster_tile(&warm,color.space,&r.device.source_samples).unwrap();
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        scene::Scene::decode_prepared_raster(&mut r,&mut encoder,&warm,color.space,color.space,pixels,crate::residency::Class::Visible).unwrap();
        r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
        let previous=RasterData::default();
        r.replace_restore(key(color,target,extent,&previous,&data),
            vec![Request {target,extent,before:previous,data:data.clone(),required:(0..count).map(|x|[x,0]).collect(),base:None,replace:true}],None,Vec::new());
        (r,data)
    }

    fn prepare_restore_prefix(r:&mut WgpuRasterizer,count:usize) {
        let color=r.document_color();
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(30);
        loop {
            let inputs=r.restore_preflight.as_ref().unwrap().targets[0].as_ref().unwrap().inputs.clone();
            inputs.prepare(r).unwrap();
            r.record_restore_uploads(|r,encoder|r.drain_image_decodes(encoder)).unwrap();
            crate::test_support::complete(r);
            let restore=r.restore_preflight.as_ref().unwrap().targets[0].as_ref().unwrap();
            if restore.pending.iter().take(count).all(|source|restore.inputs.ready_for(&source.inputs(color.space).unwrap().unwrap())) {break;}
            assert!(std::time::Instant::now()<deadline,"the admitted prefix's decodes must become ready");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    fn assert_restore_pixels(r:&WgpuRasterizer,data:&RasterData) {
        let color=r.document_color();
        let restore=r.restore_preflight.as_ref().unwrap().targets[0].as_ref().unwrap();
        assert_eq!(restore.pages.colors.len(),data.tiles.len());
        for page in &restore.pages.colors {
            let expected=data.tiles[&TileKey {plane:RasterPlane::Color,coordinate:page.coordinate}].content_version();
            let read=page.primary.read(expected,None).unwrap();let rgba=restore_pixel(page.coordinate[0]);
            let expected=[color.space.decode(f64::from(rgba[0])/255.) as f32,
                color.space.decode(f64::from(rgba[1])/255.) as f32,color.space.decode(f64::from(rgba[2])/255.) as f32,1.];
            for pixel in crate::test_support::float_pixels(r,&read.value().texture) {
                assert!(crate::test_support::max_error(&[pixel],&[expected])<2e-7,"page {:?}: {pixel:?}",page.coordinate);
            }
        }
    }

    #[test]
    fn a_restore_window_only_consumes_its_admitted_ready_prefix() {
        let (mut r,data)=restore_window(4);
        r.publish_memory();let budget=r.factory.memory_budget().clone();budget.flush_reports();
        let allowance=2*scene::sources::NORMALIZED_PAGE_BYTES;
        let pressure=budget.admit_import(budget.import_headroom_bytes().checked_sub(allowance).unwrap()).unwrap();
        assert!(r.accept_restore_inputs().unwrap());
        let restore=r.restore_preflight.as_ref().unwrap().targets[0].as_ref().unwrap();
        assert_eq!(restore.inputs.len(),2,"real Core working admission must accept two of the four source pages");
        assert_eq!(restore.next,0);
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(30);
        prepare_restore_prefix(&mut r,2);
        let before=crate::test_support::PassCounts::snapshot();
        assert!(!r.advance_restore().expect("a refused suffix must wait instead of escaping the restore"));
        let restore=r.restore_preflight.as_ref().unwrap();
        assert!(restore.failure.is_none(),"an unaccepted suffix was consumed: {:?}",restore.failure);
        assert_eq!(restore.targets[0].as_ref().unwrap().next,2,"the accepted prefix must commit before the refused suffix");
        crate::test_support::complete(&r);drop(pressure);
        r.publish_memory();budget.flush_reports();r.refresh_memory_limits();
        loop {
            let complete=r.advance_restore().expect("a refused suffix must wait instead of escaping the restore");
            let restore=r.restore_preflight.as_ref().unwrap();
            assert!(restore.failure.is_none(),"an unaccepted suffix was consumed: {:?}",restore.failure);
            crate::test_support::complete(&r);
            if complete {break;}
            assert!(std::time::Instant::now()<deadline,"the restore must advance across admitted prefixes");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(crate::test_support::PassCounts::snapshot().delta(before).source_input_misses,0);
        let restore=r.restore_preflight.as_ref().unwrap().targets[0].as_ref().unwrap();
        assert_eq!(restore.next,4);
        assert_restore_pixels(&r,&data);
    }

    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn a_two_target_restore_whose_second_target_fits_once_the_first_targets_inputs_drain_finishes() {
        let extent=[PAGE_SIZE;2];
        let mut document=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
            layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        let first=document.active_target().unwrap();
        let second=crate::test_support::add_paint(&mut document.artwork,"Second",extent).1;
        let color=document.composition().color;
        let mut r=crate::memory::memory_tests::renderer(color,layer_core::DeviceMemoryFacts {budget_bytes:Some(1<<30),..Default::default()}).unwrap();
        r.ensure_document_metadata(extent,document.scene()).unwrap();
        let tile=|x:u32|RasterTile::backed(TileBlob::encode(RasterPlane::Color.descriptor(color),&restore_pixel(x).repeat(PAGE_SIZE.pow(2) as usize)).unwrap());
        let data=|x:u32|Arc::new(RasterData {tiles:[(TileKey {plane:RasterPlane::Color,coordinate:[0,0]},tile(x))].into_iter().collect(),watercolor:None});
        let warm=tile(2).wait_backing().unwrap();
        pollster::block_on(r.compilation.prepare_all(startup::SourceKinds::for_descriptor(warm.descriptor).pipelines(&r.scene_pipelines.source))).unwrap();
        let request=|target,data:Arc<RasterData>|Request {target,extent,before:RasterData::default(),data,required:[[0,0]].into_iter().collect(),base:None,replace:true};
        r.replace_restore(Key::Frame {color,extent,reset:false,clear_starts:false,roots:Vec::new(),bases:Vec::new(),
            required:vec![(first,vec![[0,0]]),(second,vec![[0,0]])],outputs:Vec::new(),restored:Vec::new()},vec![request(first,data(0)),request(second,data(1))],None,Vec::new());
        let (send,woken)=std::sync::mpsc::channel();
        r.set_preparation_wake(Arc::new(move||{let _=send.send(());}));
        let restore=r.restore_preflight.as_ref().unwrap();
        let output=restore.targets[0].as_ref().unwrap().projected_bytes(&r).unwrap();
        let input=restore.targets[0].as_ref().unwrap().pending[0].inputs(color.space).unwrap().unwrap().additional_bytes(&r.source_tiles.borrow());
        assert!(r.admit_raster_working(u64::MAX/2).is_err(),"the lane borrows every working reservation the display can lend");
        r.publish_memory();r.factory.memory_budget().flush_reports();r.refresh_memory_limits();
        let held=r.scene_tiers.display.charge(r.current_display_memory_report().pinned_gpu_bytes.saturating_sub(r.scene_tiers.display.used()));
        r.publish_memory();r.factory.memory_budget().flush_reports();r.refresh_memory_limits();
        let room=r.raster_tiers.raster.limit().saturating_sub(r.raster_tiers.raster.used());
        let filler=r.raster_tiers.admitted.admit(crate::residency::Class::Authored,room-2*output-input/2).unwrap();
        let (mut refused,deadline)=(false,std::time::Instant::now()+std::time::Duration::from_secs(30));
        while !r.restore_ready() {
            if r.restore_preparation()==Some(crate::frame_demand::Outstanding::Blocked(crate::frame_demand::Input::Residency)) {
                refused=true;
                woken.recv_timeout(std::time::Duration::from_secs(5)).expect("the first target's drained inputs wake the second target's group");
            }
            let inputs=r.restore_preflight.as_ref().unwrap().targets.iter().flatten().find(|target|!target.complete()).unwrap().inputs.clone();
            inputs.prepare(&mut r).unwrap();
            r.record_restore_uploads(|r,encoder|r.drain_image_decodes(encoder)).unwrap();
            crate::test_support::complete(&r);r.refresh_memory_limits();
            assert!(std::time::Instant::now()<deadline,"the restore must finish once the first target's inputs drain: {:?} refused {refused} targets {:?} raster {} of {} display {} of {}",
                r.restore_preparation(),r.restore_preflight.as_ref().unwrap().targets.iter().flatten().map(|target|(target.next,target.pending.len(),target.pages.storage_bytes())).collect::<Vec<_>>(),
                r.raster_tiers.raster.used(),r.raster_tiers.raster.limit(),r.scene_tiers.display.used(),r.scene_tiers.display.limit());
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(refused,"the second target's group waits while the first target's inputs hold the room");
        let restore=r.restore_preflight.as_ref().unwrap();
        assert!(restore.failure.is_none() && restore.targets.iter().flatten().all(|target|target.complete() && target.pages.colors.len()==1),"{:?}",restore.failure);
        drop((filler,held));
    }

    #[test]
    fn a_restore_reserves_only_the_preview_pages_it_lacks() {
        let (mut r,_)=restore_request(2,|color,target,extent,_,_|Key::Frame {color,extent,reset:false,clear_starts:false,roots:Vec::new(),bases:Vec::new(),
            required:vec![(target,vec![[0,0],[1,0]])],outputs:vec![(target,Output::PreviewColor([0,0]))],restored:Vec::new()});
        let page=Output::PreviewColor([0,0]).bytes(&r,0);
        assert_eq!(r.unreserved_preview_bytes(r.restore_preflight.as_ref().unwrap()),page);
        r.prepare_preview_outputs(r.preview_level).unwrap();
        let restore=r.restore_preflight.as_mut().unwrap();
        let target=restore.targets[0].as_ref().unwrap().target;
        let Key::Frame {outputs,..}=&mut restore.key else {unreachable!()};
        *outputs=vec![(target,Output::PreviewColor([0,0])),(target,Output::PreviewColor([1,0]))];
        assert_eq!(r.unreserved_preview_bytes(r.restore_preflight.as_ref().unwrap()),page,"a stroke's next frame reserves the preview page it adds, not the pages it already holds");
    }

    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn a_refused_restore_group_holds_none_of_its_outputs_and_finishes_once_room_returns() {
        let (mut r,data)=restore_request(2,|color,target,extent,_,_|Key::Frame {color,extent,reset:false,clear_starts:false,roots:Vec::new(),bases:Vec::new(),
            required:vec![(target,vec![[0,0],[1,0]])],outputs:vec![(target,Output::PreviewColor([0,0]))],restored:Vec::new()});
        let (send,woken)=std::sync::mpsc::channel();
        r.set_preparation_wake(Arc::new(move||{let _=send.send(());}));
        let preview=r.unreserved_preview_bytes(r.restore_preflight.as_ref().unwrap());
        assert!(r.admit_raster_working(u64::MAX/2).is_err(),"the lane borrows every working reservation the display can lend");
        let room=r.raster_tiers.raster.limit().saturating_sub(r.raster_tiers.raster.used());
        let filler=r.raster_tiers.admitted.admit(crate::residency::Class::Authored,room-preview-preview/2).unwrap();
        assert!(!r.restore_ready(),"the preview fits beside the held room but the whole group does not");
        assert_eq!(r.restore_preparation(),Some(crate::frame_demand::Outstanding::Blocked(crate::frame_demand::Input::Residency)));
        assert!(r.restore_preflight.as_ref().unwrap().preview.is_none(),"a refused group holds none of its preview outputs");
        assert_eq!(r.raster_tiers.admitted.used(),filler.bytes(),"a refused group holds no raster working memory");
        assert!(r.admit_display(None,crate::residency::Class::Authored,1).is_err(),"display work waits behind the refused lane");
        r.device.retirement_handle().flush().unwrap();
        while woken.try_recv().is_ok() {}
        drop(filler);
        r.device.retirement_handle().flush().unwrap();
        assert!(woken.try_recv().is_ok(),"the released room wakes the refused group");
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(30);
        while !r.restore_ready() {
            let inputs=r.restore_preflight.as_ref().unwrap().targets[0].as_ref().unwrap().inputs.clone();
            inputs.prepare(&mut r).unwrap();
            r.record_restore_uploads(|r,encoder|r.drain_image_decodes(encoder)).unwrap();
            crate::test_support::complete(&r);
            assert!(std::time::Instant::now()<deadline,"the admitted group must finish the restore");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let restore=r.restore_preflight.as_ref().unwrap();
        assert!(restore.failure.is_none() && restore.preview.is_some(),"the group keeps its preview outputs: {:?}",restore.failure);
        assert_restore_pixels(&r,&data);
    }

    #[test]
    fn a_refused_restore_may_evict_the_decodes_its_earlier_steps_consumed() {
        let (mut r,_)=restore_window(2);
        let color=r.document_color();
        let consumed=RasterTile::backed(TileBlob::encode(RasterPlane::Color.descriptor(color),&restore_pixel(5).repeat(PAGE_SIZE.pow(2) as usize)).unwrap()).wait_backing().unwrap();
        let pixels=crate::object_image_mips::prepare_raster_tile(&consumed,color.space,&r.device.source_samples).unwrap();
        let mut requested=scene::sources::Inputs::default();requested.raster(consumed.clone(),color.space,color.space);
        let ((),accepted)=requested.accept(&mut r,&mut (),|_,r,inputs| {
            let bytes=inputs.additional_bytes(&r.source_tiles.borrow());
            r.admit_display(None,crate::residency::Class::Authored,bytes).ok()
        },|_,_|Ok(()),|_,_,_|Ok(())).unwrap().unwrap();
        let read=scene::sources::accepted_raster(&accepted,&consumed,color.space,color.space).unwrap();
        for pending in &read.pending {pending.set_prepared(Some(pixels.clone()));}
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        assert!(read.encode(&mut r,&mut encoder,crate::residency::Class::Authored).unwrap());
        accepted.track(&mut encoder);r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
        let heap=r.scene_tiers.display.heap().clone();
        heap.evict(u64::MAX,crate::residency::Class::Authored);
        let refused=r.scene_tiers.display.blocked(crate::residency::Class::Authored,u64::MAX).unwrap();
        r.note_restore_admission::<()>(&Err(refused));
        assert!(heap.evict(u64::MAX,crate::residency::Class::Authored)>0,"the refused restore cannot take the decode an earlier step consumed");
    }

    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn consumed_restore_inputs_survive_until_their_reading_encoder_completes() {
        let (mut r,data)=restore_window(3);
        assert!(r.accept_restore_inputs().unwrap());
        assert_eq!(r.restore_preflight.as_ref().unwrap().targets[0].as_ref().unwrap().inputs.len(),3);
        prepare_restore_prefix(&mut r,3);
        let validation=r.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let before=crate::test_support::PassCounts::snapshot();
        let mut consumer=submission::CommandEncoder::new(&r.device,&Default::default());
        let mut restore=r.restore_preflight.take().unwrap();
        let result=restore.targets[0].as_mut().unwrap().advance(&mut r,&mut consumer);
        r.restore_preflight=Some(restore);result.unwrap();
        let target=r.restore_preflight.as_ref().unwrap().targets[0].as_ref().unwrap();
        assert_eq!(target.next,3);
        assert!(target.inputs.is_empty(),"the window no longer owns the consumed prefix");
        r.uploads.finish(&consumer);
        crate::test_support::evict_sources(&mut r);
        consumer.submit(&r.queue);crate::test_support::complete(&r);
        let error=pollster::block_on(validation.pop());
        assert!(error.is_none(),"a consumed restore input was destroyed before its reading encoder submitted: {error:?}");
        assert_eq!(crate::test_support::PassCounts::snapshot().delta(before).source_input_misses,0);
        assert_restore_pixels(&r,&data);
    }

    #[test]
    fn a_resident_destination_outside_the_accepted_unit_never_encodes() {
        let extent=[2*PAGE_SIZE,PAGE_SIZE];
        let document=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
            layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        let target=document.active_target().unwrap();
        let mut r=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
        let mut seed=crate::tests::test_dab([PAGE_SIZE as f32,128.],[0.25,0.5,0.75,1.],1.);seed.radii=[300.;2];
        let style=crate::tests::test_style(BrushExecution::Dry);
        let seed_batch=crate::test_support::dab_batch(target,style.clone(),seed.bounds());
        let frame=crate::test_support::packet(document.scene(),extent);
        crate::test_support::submit_ready(&mut r,FramePacket {dabs:&[seed],dab_batches:&[seed_batch],..frame}).unwrap();
        assert!(r.paint_layers.iter().find(|layer|layer.id==target).unwrap().pages.iter().any(|page|page.coordinate==[1,0]));
        let dab=crate::tests::test_dab([64.,64.],[1.,0.,0.,1.],1.);
        let batch=crate::test_support::dab_batch(target,style,dab.bounds());
        let packet=FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),reset_layers:false,composite_all:false,..frame};
        crate::test_support::prepare_raster(&mut r,|r|if r.raster_dependencies_ready(packet,None) {Ok(())}else {Err(GpuRasterError::NotReady)}).unwrap();
        let tile=BrushTile {coordinate:[1,0],local:PixelRect::full([PAGE_SIZE;2]),dabs:0..1,indices:Vec::new()};
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        let before=crate::test_support::PassCounts::snapshot();
        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||r.encode_material_batch(&mut encoder,0,&batch,
            BrushEncodingContext {source:packet.scene,batches:std::slice::from_ref(&batch),dabs:std::slice::from_ref(&dab),tiles:std::slice::from_ref(&tile),target_extent:extent,target:BrushEncodingTarget::Persistent})));
        if cfg!(debug_assertions) {assert!(result.is_err());}
        else {assert!(matches!(result,Ok(Err(GpuRasterError::Invariant(_)))));}
        assert_eq!(crate::test_support::PassCounts::snapshot().delta(before).raster_output_misses,1);
        assert!(!encoder.has_work(),"unaccepted output must wait before any unit commands encode");
    }

    #[test]
    fn cold_authored_wetness_is_restored_instead_of_cleared() {
        let extent=[PAGE_SIZE;2];
        let doc=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],
            layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        let mut r=crate::memory::memory_tests::renderer(doc.composition().color,Default::default()).unwrap();
        r.ensure_document_metadata(extent,doc.scene()).unwrap();
        let target=doc.active_target().unwrap();let coordinate=[0;2];
        let key=TileKey {plane:RasterPlane::WatercolorWetness,coordinate};
        let blob=TileBlob::encode(key.plane.descriptor(doc.composition().color),&vec![93;PAGE_SIZE.pow(2) as usize]).unwrap();
        let data=Arc::new(RasterData {tiles:[(key,RasterTile::backed(blob))].into(),
            watercolor:Some(RasterWatercolor {wet_edge:0.5,burnt_edge:0.5,edge_width:2.})});
        let mut restore=TargetRestore::new(&mut r,Request {target,extent,before:(*data).clone(),data,
            required:[coordinate].into(),base:None,replace:false},None,&[]).unwrap();
        restore.add_outputs(&r,&[(target,Output::Wetness(coordinate))],false,false,None,&[]);
        assert_eq!(restore.pending.len(),1);
        assert!(matches!(&restore.pending[0],SourcePage::Raster(key,_) if key.plane==RasterPlane::WatercolorWetness));
    }
}
