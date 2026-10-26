use super::*;

const PREFETCH:usize=18;
const UPLOADS:usize=16;

#[derive(PartialEq)]
struct DataKey {tiles:Vec<(TileKey,u64)>,watercolor:Option<[u32;3]>}
impl DataKey {
    fn new(data:&RasterData)->Self {Self {tiles:data.tiles.iter().map(|(key,tile)|(*key,tile.identity())).collect(),
        watercolor:data.watercolor.map(|w|[w.wet_edge.to_bits(),w.burnt_edge.to_bits(),w.edge_width.to_bits()])}}
}
#[derive(PartialEq)]
struct FrameRoot {target:SourceTarget,revision:u64,published:Option<usize>,restore:Option<u64>,extent:[u32;2]}
#[derive(PartialEq)]
enum Key {
    Frame {color:layer_core::color::DocumentColor,extent:[u32;2],reset:bool,roots:Vec<FrameRoot>,
        required:Vec<(SourceTarget,Vec<[u32;2]>)>,bases:Vec<(SourceTarget,usize,[u32;2])>},
    Direct {color:layer_core::color::DocumentColor,target:SourceTarget,extent:[u32;2],before:DataKey,after:DataKey},
}
#[derive(Default)]
pub(crate) struct PreparedPages {
    pub colors:Vec<LayerPage>,
    pub masks:Vec<([u32;2],layer_masks::MaskPage)>,
    pub wetness:Vec<WatercolorWetnessPage>,
}
struct TargetRestore {
    target:SourceTarget,
    data:Arc<RasterData>,
    selected:RasterData,
    pending:Vec<SourcePage>,
    next:usize,
    pages:PreparedPages,
    replace:bool,
    scalars:Vec<object_image_mips::PreparedRasterTile>,
}
enum SourcePage {Raster(TileKey,RasterTile),Base([u32;2],layer_core::authored::PaintBase)}
struct Request {target:SourceTarget,extent:[u32;2],before:RasterData,data:Arc<RasterData>,required:BTreeSet<[u32;2]>,base:Option<layer_core::authored::PaintBase>,replace:bool}
impl TargetRestore {
    fn new(r:&WgpuRasterizer,request:Request)->Result<Self,GpuRasterError> {
        let Request {target,extent,before,data,required,base,replace}=request;
        data.validate_storage_index(extent,target.is_coverage(),r.document_color()).map_err(GpuRasterError::Effect)?;
        let (before,selected)=r.select_restore_data(target,&before,&data,&required);
        let resident=|coordinate|r.paint_layers.iter().find(|layer|layer.id==target).is_some_and(|layer|layer.pages.iter().any(|page|page.coordinate==coordinate));
        let mut pending:Vec<_>=selected.tiles.iter().filter(|(key,tile)|if replace {
            !before.tiles.get(key).is_some_and(|old|old.same_capture(tile)) || key.plane==RasterPlane::Color && required.contains(&key.coordinate) && !resident(key.coordinate)
        }else {key.plane==RasterPlane::Color && required.contains(&key.coordinate) && !resident(key.coordinate)})
            .map(|(key,tile)|SourcePage::Raster(*key,tile.clone())).collect();
        if let Some(base)=base {
            pending.extend(required.iter().filter(|coordinate| (replace || !resident(**coordinate)) && !data.tiles.contains_key(&TileKey {plane:RasterPlane::Color,coordinate:**coordinate})
                && source_access::paint_base_contains(&base,**coordinate)).map(|coordinate|SourcePage::Base(*coordinate,base.clone())));
        }
        Ok(Self {target,data,selected,pending,next:0,pages:PreparedPages::default(),replace,scalars:Vec::new()})
    }
    fn awaits(&self,blob:&TileBlob)->bool {
        self.pending.iter().skip(self.next).take(PREFETCH).any(|source|matches!(source,SourcePage::Raster(key,tile) if key.plane!=RasterPlane::Color
            && tile.try_backing().and_then(Result::ok).is_some_and(|awaited|scene::sources::same_raster_samples(&awaited,blob))))
    }
    fn held(&self,blob:&TileBlob)->Option<&object_image_mips::PreparedRasterTile> {
        self.scalars.iter().find(|prepared|scene::sources::same_raster_samples(&prepared.blob,blob))
    }
    fn projected_bytes(&self,r:&WgpuRasterizer)->Result<u64,GpuRasterError> {
        self.pending.iter().try_fold(0u64,|bytes,source| {
            let texel=match source {
                SourcePage::Base(..)|SourcePage::Raster(TileKey {plane:RasterPlane::Color,..},_)=>u64::from(r.device.working_format().block_copy_size(None).unwrap()),
                SourcePage::Raster(key,_)=>u64::from(r.device.scalar_format().block_copy_size(None).unwrap())*if key.plane==RasterPlane::WatercolorWetness {2}else {1},
            };
            bytes.checked_add(u64::from(PAGE_SIZE).pow(2)*texel).ok_or(GpuRasterError::SourceWorkingSetExceeded)
        })
    }
    fn complete(&self)->bool {self.next==self.pending.len()}
    fn advance(&mut self,r:&mut WgpuRasterizer,encoder:&mut submission::CommandEncoder)->Result<(),GpuRasterError> {
        let space=r.document_color().space;
        for source in self.pending.iter().skip(self.next).take(PREFETCH) {
            match source {
                SourcePage::Raster(key,tile)=> {
                    let Some(blob)=tile.try_backing().transpose().map_err(GpuRasterError::Effect)? else {break;};
                    let requested=if key.plane==RasterPlane::Color {r.source_tiles.borrow().prepared_raster_view(&blob,space,space).is_some() || r.request_raster_decode(blob,space)?}
                        else {self.held(&blob).is_some() || r.request_scalar_decode(blob,space)?};
                    if !requested {break;}
                },
                SourcePage::Base(coordinate,base)=> {Self::prepare_base(r,base,*coordinate)?;break;},
            }
        }
        for _ in 0..UPLOADS {
            let Some(source)=self.pending.get(self.next) else {break;};
            if let SourcePage::Base(coordinate,base)=source {
                if !Self::prepare_base(r,base,*coordinate)? {break;}
                let Some(read)=source_access::plan_paint_base(r,base,*coordinate)? else {self.next+=1;continue;};
                if !read.pending.is_empty() {return Err(GpuRasterError::DeferredObjectWork);}
                if let Some(assembly)=read.assembly {source_access::encode_paint_base(&assembly,encoder);}
                let mut page=r.create_page(*coordinate,"staged photo base");page.primary_needs_clear=false;
                encoder.copy_texture_to_texture(read.tile.texture.as_image_copy(),page.primary.texture.as_image_copy(),page.primary.texture.size());
                self.pages.colors.push(page);self.next+=1;continue;
            }
            let SourcePage::Raster(key,tile)=source else {unreachable!()};
            let Some(blob)=tile.try_backing().transpose().map_err(GpuRasterError::Effect)? else {break;};
            if blob.descriptor!=tile.descriptor() {return Err(GpuRasterError::Effect("Raster plane has the wrong pixel representation".into()));}
            if key.plane==RasterPlane::Color {
                let Some(view)=r.source_tiles.borrow().prepared_raster_view(&blob,space,space).cloned() else {
                    r.request_raster_decode(blob,space)?;break;
                };
                let mut page=r.create_page(key.coordinate,"staged raster tile");page.primary_needs_clear=false;
                encoder.copy_texture_to_texture(view.texture().as_image_copy(),page.primary.texture.as_image_copy(),page.primary.texture.size());
                self.pages.colors.push(page);self.next+=1;continue;
            }
            let Some(prepared)=self.held(&blob) else {r.request_scalar_decode(blob,space)?;break;};
            let samples=match &prepared.pixels {
                Ok(scene::sources::PreparedSourcePixels::NativeSamples(samples))=>samples.clone(),
                Ok(_)=>return Err(GpuRasterError::Color("Coverage requires native samples".into())),
                Err(error)=>return Err(GpuRasterError::Color(error.clone())),
            };
            if !r.source_tiles.borrow().can_upload_bytes(native_tiles::scalar::COVERAGE_TILE_BYTES) {break;}
            if key.plane==RasterPlane::Mask {
                let page=layer_masks::MaskPage::new(&r.device);
                native_tiles::scalar::upload_coverage(r,encoder,&page.texture,blob.descriptor.depth(),&samples,4)?;
                self.pages.masks.push((key.coordinate,page));
            } else {
                let primary=r.create_scalar_page_surface("staged watercolor wetness");
                native_tiles::scalar::upload_coverage(r,encoder,&primary.texture,blob.descriptor.depth(),&samples,4)?;
                let secondary=r.create_scalar_page_surface("watercolor wetness companion");
                self.pages.wetness.push(WatercolorWetnessPage {coordinate:key.coordinate,primary,secondary,active_secondary:false,
                    primary_needs_clear:false,secondary_needs_clear:true});
            }
            self.next+=1;
        }
        let mut scalars=std::mem::take(&mut self.scalars);
        scalars.retain(|prepared|self.awaits(&prepared.blob));
        self.scalars=scalars;
        Ok(())
    }
    fn prepare_base(r:&mut WgpuRasterizer,base:&layer_core::authored::PaintBase,coordinate:[u32;2])->Result<bool,GpuRasterError> {
        let mut ready=true;
        for coordinate in page_coordinates(source_access::paint_base_source_bounds(base,coordinate)) {
            if r.source_tiles.borrow().prepared_view(base.image.storage(),coordinate).is_none() {
                ready=false;if !r.request_image_decode(base.image.id(),base.image.storage().clone(),coordinate)? {break;}
            }
        }
        Ok(ready)
    }
}
pub(crate) struct RestorePreflight {
    key:Key,
    targets:Vec<Option<TargetRestore>>,
    failure:Option<GpuRasterError>,
    consumed:bool,
}
impl RestorePreflight {
    pub(crate) fn dependencies(&self)->&[(SourceTarget,Vec<[u32;2]>)] {
        match &self.key {
            Key::Frame {required,..} if !self.consumed=>required,
            _=>&[],
        }
    }
    fn new(r:&WgpuRasterizer,key:Key,requests:Vec<Request>)->Self {
        let mut targets=Vec::new();let mut failure=None;
        for request in requests {
            match TargetRestore::new(r,request) {
                Ok(target)=>targets.push(Some(target)),Err(error)=>{failure=Some(error);break;},
            }
        }
        let projected=targets.iter().flatten().try_fold(0u64,|bytes,target|bytes.checked_add(target.projected_bytes(r)?).ok_or(GpuRasterError::SourceWorkingSetExceeded));
        match projected.and_then(|bytes|r.admit_preparation_bytes(bytes,r.bake_preflight.as_ref().map_or(0,|bake|bake.projected_bytes())).map(|()|bytes)) {
            Ok(_)=>(),Err(error)=>failure=Some(error),
        }
        Self {key,targets,failure,consumed:false}
    }
    fn complete(&self)->bool {self.failure.is_some() || self.targets.iter().flatten().all(TargetRestore::complete)}
}
impl WgpuRasterizer {
    pub(crate) fn admit_preparation_bytes(&self,restore:u64,bake:u64)->Result<(),GpuRasterError> {
        self.admit_preparation_with_transforms(restore,bake,self.transforms.as_ref())
    }
    pub(crate) fn admit_preparation_with_transforms(&self,restore:u64,bake:u64,transforms:Option<&paint_transform::PaintTransforms>)->Result<(),GpuRasterError> {
        let Some(native)=&self.native_edit else {return Ok(());};
        let limit=native.image_pixel_allowance().checked_add(native.color_cache_bytes).ok_or(GpuRasterError::SourceWorkingSetExceeded)?;
        let mut textures=RetainedTextures::default();
        let mut bytes=restore.checked_add(bake).ok_or(GpuRasterError::SourceWorkingSetExceeded)?;
        let published=self.paint_layers.iter().flat_map(|layer|layer.pages.iter().flat_map(|page|std::iter::once(&page.primary.texture).chain(page.secondary.iter().map(|surface|&surface.texture)).chain(page.stroke_start.iter().map(|(_,surface)|&surface.texture)))
            .chain(layer.watercolor_wetness_pages.iter().flat_map(|page|[&page.primary.texture,&page.secondary.texture])))
            .chain(self.layer_masks.pages.values().map(|page|&page.texture));
        for texture in published {if textures.insert(texture.clone()) {bytes=bytes.checked_add(texture_bytes(texture)).ok_or(GpuRasterError::SourceWorkingSetExceeded)?;}}
        bytes=bytes.checked_add(self.scale_display.as_ref().map_or(0,|cache|cache.resident_bytes()))
            .and_then(|bytes|bytes.checked_add(transforms.map_or(0,|transforms|transforms.preparation_bytes(&mut textures))))
            .ok_or(GpuRasterError::SourceWorkingSetExceeded)?;
        if bytes>limit {Err(GpuRasterError::SourceWorkingSetExceeded)}else {Ok(())}
    }
    pub(crate) fn projected_restore_bytes(&self)->u64 {self.restore_preflight.as_ref().map_or(0,|restore|restore.targets.iter().flatten().try_fold(0u64,|bytes,target|bytes.checked_add(target.projected_bytes(self)?).ok_or(GpuRasterError::SourceWorkingSetExceeded)).unwrap_or(u64::MAX))}
    pub(crate) fn staged_transform_source(&self,target:SourceTarget)->Option<(&Arc<RasterData>,&PreparedPages)> {
        self.restore_preflight.as_ref()?.targets.iter().flatten().find(|entry|entry.replace && entry.target==target && entry.complete()).map(|entry|(&entry.data,&entry.pages))
    }
    pub(crate) fn prepare_transform_preview(&mut self,packet:FramePacket<'_>,preview:Option<&layer_render::TransformPreview>)->bool {
        if self.snapshot_worker {return true;}
        let Some(preview)=preview else {
            self.transforms.as_mut().unwrap().cancel_preparation();return true;
        };
        let request=match scene::scale::request(self,packet) {Ok(request)=>request,Err(_)=>return true};
        if request.plan.level==0 || request.evaluation==scene::scale::Evaluation::Native {self.transforms.as_mut().unwrap().cancel_preparation();return true;}
        let mut transforms=self.transforms.take().unwrap();
        let result=self.record_restore_uploads(|r,encoder|transforms.prepare_preview(r,encoder,preview,packet.scene,request.plan.level,request.evaluation.fine_capacity().is_some()));
        self.transforms=Some(transforms);
        match result {
            Ok(())=>true,
            Err(error) if error.is_preparation_pending()=>false,
            Err(error)=>{if let Some(restore)=&mut self.restore_preflight {restore.failure=Some(error);}true},
        }
    }
    fn record_restore_uploads<T>(&mut self,work:impl FnOnce(&mut Self,&mut submission::CommandEncoder)->Result<T,GpuRasterError>)->Result<T,GpuRasterError> {
        let mut encoder=submission::CommandEncoder::new(&self.device,&Default::default());
        std::mem::swap(&mut self.uploads,&mut self.restore_uploads);
        let result=work(self,&mut encoder);
        if encoder.has_work() {
            self.uploads.finish(&encoder);self.last_submission=Some(encoder.submit(&self.queue));self.metrics.native_restore_submissions+=1;
        }
        std::mem::swap(&mut self.uploads,&mut self.restore_uploads);result
    }
    fn restore_pipelines_ready(&self)->bool {
        self.startup.as_ref().map(|startup|&startup.compiler).or(self.preparation_compiler.as_ref()).is_none_or(|compiler| {
            let ready=compiler.require([&self.scene_pipelines.source.pipeline],startup::VALIDATION);compiler.start();ready
        })
    }
    pub(crate) fn prepare_raster_uploads(&mut self,requests:&[(Arc<TileBlob>,layer_core::color::RgbSpace,layer_core::color::RgbSpace)])->Result<(),GpuRasterError> {
        if self.snapshot_worker {return Ok(());}
        if !self.restore_pipelines_ready() {return Err(GpuRasterError::DeferredObjectWork);}
        let ready=self.record_restore_uploads(|r,encoder| {
            r.drain_image_decodes(encoder)?;let mut ready=true;
            for (blob,source,destination) in requests {
                if r.source_tiles.borrow().prepared_raster_view(blob,*source,*destination).is_none() {
                    ready=false;if !r.request_raster_decode_to(blob.clone(),*source,*destination)? {break;}
                }
            }
            Ok(ready)
        })?;
        if ready {Ok(())}else {Err(GpuRasterError::DeferredObjectWork)}
    }
    pub(crate) fn restore_preparation_pending(&self)->bool {self.restore_preflight.as_ref().is_some_and(|restore|!restore.complete())}
    fn replace_restore(&mut self,key:Key,requests:Vec<Request>) {
        if self.restore_preflight.as_ref().is_none_or(|restore|restore.key!=key || restore.consumed) {
            self.restore_preflight=Some(RestorePreflight::new(self,key,requests));
        }
    }
    fn advance_restore(&mut self)->Result<bool,GpuRasterError> {
        if self.restore_preparation_pending() && !self.restore_pipelines_ready() {return Ok(false);}
        if self.restore_preflight.is_none() {return Ok(true);}
        let result=self.record_restore_uploads(|r,encoder| {
            r.drain_image_decodes(encoder)?;
            let mut restore=r.restore_preflight.take().unwrap();
            let result=restore.targets.iter_mut().flatten().find(|target|!target.complete()).map_or(Ok(()),|target|target.advance(r,encoder));
            r.restore_preflight=Some(restore);result
        });
        let restore=self.restore_preflight.as_mut().unwrap();
        if let Err(error)=result {restore.failure=Some(error);}
        Ok(restore.complete())
    }
    pub(crate) fn hold_restore_scalar(&mut self,prepared:object_image_mips::PreparedRasterTile) {
        if let Some(target)=self.restore_preflight.as_mut().and_then(|restore|restore.targets.iter_mut().flatten().find(|target|!target.complete()))
            && target.awaits(&prepared.blob) && target.held(&prepared.blob).is_none() {target.scalars.push(prepared);}
    }
    pub(crate) fn prepare_raster_restores(&mut self,packet:FramePacket<'_>)->bool {
        if self.snapshot_worker {return self.raster_restore_backing_ready(packet);}
        let retained=source_access::retained_targets(packet.scene);
        let reset=packet.reset_layers||packet.document_extent!=self.document_extent;
        let mut plans=match brush_tiles::plan_packet(packet,|target|packet.scene.target_extent(target)) {Ok(plans)=>plans,Err(_)=>return true,};
        liquify::Liquify::extend_required_plans(self,packet,&mut plans);
        let mut required:BTreeMap<SourceTarget,BTreeSet<[u32;2]>>=BTreeMap::new();
        for (index,(batch,tiles)) in packet.dab_batches.iter().zip(&plans).enumerate() {
            if matches!(batch.target,SourceTarget::Paint(_)) {
                let coordinates=required.entry(batch.target).or_default();
                coordinates.extend(tiles.iter().flat_map(|tile|source_access::neighborhood(tile.coordinate,self.material_offsets(batch))));
                coordinates.extend(tiles.iter().map(|tile|tile.coordinate));
                if BrushPassPlan::for_device(&batch.style,&self.device).state.watercolor_wetness && is_last_watercolor_update_batch(packet.dab_batches,index) {
                    coordinates.extend(watercolor_update_damages(packet.dab_batches,index,packet.scene.target_extent(batch.target)).into_iter()
                        .flat_map(page_coordinates).flat_map(|coordinate|source_access::neighborhood(coordinate,[[0,0],[-1,0],[1,0],[0,-1],[0,1]].map(Some))));
                }
                if matches!(batch.kind,DabBatchKind::RasterOperation(_)) {coordinates.extend(page_coordinates(batch_pixel_rect(batch,packet.scene.target_extent(batch.target))));}
            }
        }
        liquify::Liquify::required_sources(self,packet,&plans,&mut required);
        self.required_retouch_sources(packet,&plans,&mut required);
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
        let key=Key::Frame {color:self.document_color(),extent:packet.document_extent,reset,roots,bases,
            required:required.iter().map(|(target,coordinates)|(*target,coordinates.iter().copied().collect())).collect()};
        if self.restore_preflight.as_ref().is_some_and(|restore|restore.key==key && !restore.consumed) {return self.advance_restore().unwrap_or(true);}
        self.restore_preflight=None;
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
            } else if reset && let Some(current)=current {current.data.clone()
            } else if !required.is_empty() {current.map_or_else(||Arc::new(RasterData::default()),|current|current.data.clone())}else {continue;};
            let replace=if let Some(current)=current {reset || explicit.is_some() || resolved && current.revision!=*root}else {!data.tiles.is_empty()};
            if !replace && required.is_empty() {continue;}
            let mut before=if reset {RasterData::default()}else {current.map_or_else(RasterData::default,|current|(*current.data).clone())};
            if let Some(current)=current {before.tiles.retain(|key,_|!current.changed.contains(&key.coordinate));}
            requests.push(Request {target,extent:scene.target_extent(target),before,data,required,base:scene.paint_base(target).cloned(),replace});
        }
        self.replace_restore(key,requests);self.advance_restore().unwrap_or(true)
    }
    pub(super) fn consume_restore_preflight(&mut self) {if let Some(restore)=&mut self.restore_preflight {restore.consumed=true;}}
    pub(crate) fn take_restore_failure(&mut self)->Option<GpuRasterError> {self.restore_preflight.as_mut().and_then(|restore|restore.failure.take())}
    pub(super) fn validate_restore_publication(&self)->Result<(),GpuRasterError> {
        if let Some(restore)=&self.restore_preflight {
            if !restore.complete() {return Err(GpuRasterError::DeferredObjectWork);}
            for target in restore.targets.iter().flatten() {
                if !target.target.is_coverage() && !self.paint_layers.iter().any(|layer|layer.id==target.target) {return Err(GpuRasterError::MissingPaintLayer(target.target));}
            }
        }
        Ok(())
    }
    pub(super) fn publish_prepared_restore(&mut self,target:SourceTarget,data:&Arc<RasterData>)->Result<(),GpuRasterError> {
        if let Some(error)=self.take_restore_failure() {return Err(error);}
        self.validate_restore_publication()?;
        let restore=self.restore_preflight.as_mut().ok_or(GpuRasterError::DeferredObjectWork)?;
        let index=restore.targets.iter().position(|entry|entry.as_ref().is_some_and(|entry|entry.replace && entry.target==target && Arc::ptr_eq(&entry.data,data)))
            .ok_or(GpuRasterError::DeferredObjectWork)?;
        if !restore.complete() {return Err(GpuRasterError::DeferredObjectWork);}
        let target=restore.targets[index].take().unwrap();restore.consumed=true;
        self.publish_restored_pages(target.target,&target.selected,target.pages)?;
        if let Some(native)=&mut self.native_edit {native.backing.insert(target.target,data.clone());}
        Ok(())
    }
    pub(super) fn publish_warm_restore_pages(&mut self)->Result<(),GpuRasterError> {
        self.validate_restore_publication()?;
        let Some(restore)=&mut self.restore_preflight else {return Ok(());};
        for entry in &mut restore.targets {
            if entry.as_ref().is_none_or(|target|target.replace) {continue;}
            let target=entry.take().unwrap();restore.consumed=true;
            if target.pages.colors.is_empty() {continue;}
            let layer=self.paint_layers.iter_mut().find(|layer|layer.id==target.target).ok_or(GpuRasterError::MissingPaintLayer(target.target))?;
            for page in target.pages.colors {layer.pages.retain(|old|old.coordinate!=page.coordinate);layer.pages.push(page);}
        }
        Ok(())
    }
    pub(super) fn restore_direct(&mut self,target:SourceTarget,before:&RasterData,data:&RasterData)->Result<(),GpuRasterError> {
        let key=Key::Direct {color:self.document_color(),target,extent:self.target_extent(target),before:DataKey::new(before),after:DataKey::new(data)};
        if self.restore_preflight.as_ref().is_none_or(|restore|restore.key!=key || restore.consumed) {
            self.replace_restore(key,vec![Request {target,extent:self.target_extent(target),before:before.clone(),data:Arc::new(data.clone()),
                required:BTreeSet::new(),base:None,replace:true}]);
        }
        if !self.advance_restore()? {
            return Err(GpuRasterError::DeferredObjectWork);
        }
        if let Some(error)=self.take_restore_failure() {return Err(error);}
        let data=self.restore_preflight.as_ref().unwrap().targets.iter().flatten().find(|entry|entry.target==target).unwrap().data.clone();
        self.publish_prepared_restore(target,&data)
    }
}
