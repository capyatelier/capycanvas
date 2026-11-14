//! Native commit ownership for the Float32 renderer. Each frame quantizes every
//! changed page into immutable native outputs and canonical Float32 working
//! pixels. Optional in-place storage avoids candidate scratch;
//! other devices promote through bounded scratch. Readback follows presentation.
use super::*;
use crate::native_tiles::{
    MAX_BATCH_TILES, NativeTileEncoder, NativeTileRequest, NativeTransfer,
    promote::{NativePromoter, NativePromotion},
    scalar::{NativeScalarEncoder, NativeScalarRequest},
};
use layer_core::color::DocumentColor;

pub(crate) fn native_capture_candidates(
    contents:&super::contents::Contents, planned:impl IntoIterator<Item=TileKey>,
    mut descriptor:impl FnMut(TileKey)->layer_core::color::PixelDescriptor,
)->BTreeSet<TileKey> {
    let mut keys:BTreeSet<_>=planned.into_iter().collect();
    for coordinate in contents.modified_coordinates() {
        keys.extend([RasterPlane::Color,RasterPlane::Mask,RasterPlane::WatercolorWetness]
            .map(|plane|TileKey {plane,coordinate}));
    }
    keys.retain(|key| {
        let expected=descriptor(*key);
        contents.version(*key).matches(&contents.selected_version(*key))!=Some(true)
            || contents.data().tiles.get(key).is_some_and(|tile|tile.descriptor()!=expected)
    });keys
}

pub(crate) struct Recipes {
    in_place: bool,
    color: Arc<NativeTileEncoder>,
    scalar: Arc<NativeScalarEncoder>,
    promoter: Option<Arc<NativePromoter>>,
}
impl Recipes {
    fn shared(device: &PipelineDevice, in_place: bool) -> Arc<Self> {
        let mut cache = device.native_publication.lock().unwrap();
        if let Some(recipes) = cache.iter().find(|p| p.in_place == in_place) { return recipes.clone(); }
        let device = device.for_recipe();
        let recipes = Arc::new(Self { in_place,
            color: Arc::new(NativeTileEncoder::new(&device, in_place)),
            scalar: Arc::new(if in_place { NativeScalarEncoder::in_place(&device) } else { NativeScalarEncoder::with_device(&device) }),
            promoter: (!in_place).then(|| Arc::new(NativePromoter::with_device(&device))),
        });
        cache.push(recipes.clone());
        recipes
    }
}

pub(crate) struct NativeEdit {
    pub(super) backing: BTreeMap<SourceTarget, Arc<RasterData>>,
    #[cfg(test)]
    pub image_pixel_bytes: Option<u64>,
    transfer: NativeTransfer,
    color: Arc<NativeTileEncoder>,
    scalar: Arc<NativeScalarEncoder>,
    promoter: Option<Arc<NativePromoter>>,
    colors: Vec<wgpu::Texture>,
    scalars: Vec<wgpu::Texture>,
    preview_scalars: Vec<wgpu::Buffer>,
}
impl NativeEdit {
    fn new(r: &WgpuRasterizer, transfer: NativeTransfer) -> Self {
        let in_place = !crate::native_tiles::native_in_place_features(&r.adapter).is_empty()
            && r.device
                .features()
                .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);
        Self::with_admission(r,transfer,in_place)
    }
    #[cfg(test)]
    fn with_mode(r: &WgpuRasterizer, transfer: NativeTransfer, in_place: bool) -> Self {Self::with_admission(r,transfer,in_place)}
    fn with_admission(r:&WgpuRasterizer,transfer:NativeTransfer,in_place:bool)->Self {
        let scratch_count = if in_place || r.snapshot_worker { 0 } else { MAX_BATCH_TILES };
        let colors = (0..scratch_count)
            .map(|_| Self::scratch_texture(&r.device, wgpu::TextureFormat::Rgba32Float))
            .collect();
        let scalars = (0..scratch_count)
            .map(|_| Self::scratch_texture(&r.device, wgpu::TextureFormat::R32Float))
            .collect();
        let recipes = Recipes::shared(&r.device, in_place);
        Self {
            backing: BTreeMap::new(),
            #[cfg(test)]
            image_pixel_bytes: None,
            transfer,
            colors,
            scalars,
            preview_scalars: Vec::new(),
            color: recipes.color.clone(), scalar: recipes.scalar.clone(),
            promoter: recipes.promoter.clone(),
        }
    }
    fn scratch_texture(device: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bounded native commit scratch"),
            size: wgpu::Extent3d { width: 256, height: 256, depth_or_array_layers: 1 },
            dimension: wgpu::TextureDimension::D2,
            format,
            mip_level_count: 1,
            sample_count: 1,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }
    fn reserve_scratch(&mut self, device: &wgpu::Device, inputs: &[(wgpu::Texture, RasterTile, layer_core::color::LayerColorMode)]) {
        if self.promoter.is_none() { return; }
        for (format, textures) in [(wgpu::TextureFormat::Rgba32Float, &mut self.colors), (wgpu::TextureFormat::R32Float, &mut self.scalars)] {
            let count = inputs.chunks(MAX_BATCH_TILES).map(|batch| batch.iter().filter(|(t, _, _)| t.format() == format).count()).max().unwrap_or(0);
            textures.extend((textures.len()..count).map(|_| Self::scratch_texture(device, format)));
        }
    }
    #[cfg(test)]
    pub(crate) fn pipelines(&self) -> impl Iterator<Item = &Deferred<wgpu::ComputePipeline>> {
        self.color
            .pipelines
            .iter()
            .chain(&self.scalar.pipelines)
            .chain(self.promoter.iter().flat_map(|p| p.pipelines.iter()))
    }
    pub(crate) fn required_pipelines(&self, depth: layer_core::color::SampleDepth, gray:bool) -> impl Iterator<Item = &Deferred<wgpu::ComputePipeline>> {
        self.color.pipelines_for_depth(depth,gray)
            .chain(&self.scalar.pipelines)
            .chain(self.promoter.iter().flat_map(|p| p.pipelines.iter()))
    }
    pub(crate) fn private_pipelines(&self, depth: layer_core::color::SampleDepth, mode: layer_core::color::LayerColorMode, planes: &[RasterPlane]) -> Vec<Deferred<wgpu::ComputePipeline>> {
        let mut pipelines = Vec::new();
        if planes.contains(&RasterPlane::Color) {
            pipelines.push(self.color.private_pipeline(depth, mode));
        }
        let scalar = planes.iter().filter(|&&p| p != RasterPlane::Color).count();
        if scalar != 0 { pipelines.push(self.scalar.private_pipeline(scalar)); }
        pipelines
    }
    pub fn storage_bytes(&self) -> u64 {
        self.promoter
            .as_ref()
            .map_or(0, |p| p.storage_bytes())
            + self.color.storage_bytes()
            + self.scalar.storage_bytes()
            + self.colors.iter().map(texture_bytes).sum::<u64>()
            + self.scalars.iter().map(texture_bytes).sum::<u64>()
            + self.preview_scalars.iter().map(wgpu::Buffer::size).sum::<u64>()
        // Transfer storage is owned/accounted by the shared scene decoder cache.
    }

}

struct Publication {
    id: SourceTarget,
    revision: RasterRevision,
    data: RasterData,
}
pub(crate) struct NativeFrame {
    capture: Option<NativeCapture>,
    publications: Vec<Publication>,
    pub(crate) canonical_pages: Vec<(SourceTarget, [u32; 2])>,
}
enum NativeAccess<'a> {
    Write(crate::resident::RasterWrite<'a,source_access::RawTile>),
    Read(crate::resident::Read<'a,source_access::RawTile>),
    Private(&'a wgpu::Texture),
}
impl NativeAccess<'_> {
    fn texture(&self)->&wgpu::Texture {match self {Self::Write(write)=>crate::resident::TextureStorage::texture(write.value()),Self::Read(read)=>crate::resident::TextureStorage::texture(read.value()),Self::Private(texture)=>texture}}
    fn encoded(self,name:layer_core::raster::ContentVersion)->Option<crate::resident::Encoding> {
        match self {Self::Write(write)=>Some(write.encoded(name)),_=>None,}
    }
}
pub(crate) struct NativeAdmission {
    allocation:crate::residency::Reservation,
    comparisons:Option<crate::native_tiles::comparison::CanonicalComparisons>,
    capacity:usize,
    bytes:u64,
    readback:u64,
}
impl NativeAdmission {
    fn whole(&self)->bool {self.allocation.bytes()>=self.bytes.saturating_add(self.readback)}
}
fn tile_output_bytes(tile:&RasterTile)->u64 {tile.descriptor().byte_len([PAGE_SIZE;2]).unwrap() as u64}
fn capture_readback(bytes:u64,largest:u64,admitted:u64)->u64 {
    let minimum=(largest+crate::native_tiles::comparison::RESULT_BYTES).next_power_of_two();
    let full=bytes.min(CAPTURE_CHUNK).next_power_of_two().max(minimum);
    if admitted>=full.saturating_add(bytes) {return full;}
    full.min(1<<(admitted.saturating_sub(largest)/8).max(minimum).ilog2())
}
pub(crate) struct NativeJob {
    pub frame: NativeFrame,
    inputs: Vec<(wgpu::Texture, RasterTile, layer_core::color::LayerColorMode)>,
    views: crate::native_tiles::PublicationViews,
    changes: Vec<Option<wgpu::Buffer>>,
    comparisons:Option<crate::native_tiles::comparison::CanonicalComparisons>,
    canonical:Vec<Option<Arc<layer_core::raster::CanonicalName>>>,
    intents:Vec<Option<crate::raster::contents::Intent>>,
    working_inputs:Vec<Option<crate::resident::Resident<source_access::RawTile>>>,
    encoded: usize,
    started: bool,
}
impl NativeJob {
    const TIMING_PASSES: usize = 4;
    pub fn complete(&self) -> bool { self.started && self.encoded == self.inputs.len() }
    fn pass_count(&self, native: &NativeEdit) -> usize {
        self.inputs.len().div_ceil(MAX_BATCH_TILES) * (1 + usize::from(native.promoter.is_some()))
    }
}
impl Drop for NativeFrame {
    fn drop(&mut self) {
        for publication in &self.publications {
            if publication.revision.try_data().is_none() {
                let _ = publication
                    .revision
                    .publish(Err("Native raster frame was abandoned".into()));
            }
        }
    }
}

impl WgpuRasterizer {
    pub(crate) fn canonicalize_preview_watercolor(&mut self, encoder: &mut submission::CommandEncoder) -> Result<(), GpuRasterError> {
        let depth=self.document_color().depth.coverage();
        if self.native_edit.is_none() {return Ok(());}
        let target=self.preview_declaration.as_ref().ok_or(GpuRasterError::Invariant("prepared canonicalize preview watercolor state is inconsistent"))?.target();
        let coordinates=self.preview_watercolor_wetness_pages.iter().filter(|page|!self.preview_damage.intersect(page_rect(page.coordinate)).is_empty())
            .map(|page|page.coordinate).collect::<Vec<_>>();
        let mut outputs=Vec::with_capacity(coordinates.len());
        for coordinate in coordinates {
            let ticket=self.preview_version(target,RasterPlane::WatercolorWetness,coordinate,crate::scene::PreviewStage::Final)?;
            let receipt=encoder.submission();
            let page=self.preview_watercolor_wetness_pages.iter_mut().find(|page|page.coordinate==coordinate).unwrap();
            let active=page.active_secondary;
            let write=page.surface_mut(active).revise(ticket.version(),&receipt).ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
            let texture=write.value().texture.clone();
            outputs.push((coordinate,texture,ticket.clone(),write.encoded(ticket.version())));
        }
        if outputs.is_empty() {return Ok(());}
        let native=self.native_edit.as_mut().unwrap();let buffers=&mut native.preview_scalars;
        while buffers.len()<outputs.len().min(MAX_BATCH_TILES) {
            buffers.push(self.device.create_buffer(&wgpu::BufferDescriptor {label:Some("watercolor preview native coverage"),
                size:u64::from(PAGE_SIZE).pow(2)*depth.bytes() as u64,usage:wgpu::BufferUsages::STORAGE,mapped_at_creation:false}));
        }
        let mut views=Default::default();
        for chunk in outputs.chunks(MAX_BATCH_TILES) {
            let requests=chunk.iter().enumerate().map(|(index,(_,texture,_,_))|NativeScalarRequest {
                working:texture,encoded:&buffers[index],canonical:native.scalars.get(index).unwrap_or(texture),depth,region:[0,0,PAGE_SIZE,PAGE_SIZE],
            }).collect::<Vec<_>>();
            let scalar=native.scalar.prepare(&self.device,&requests,&mut views)?;
            let promotions=requests.iter().map(|request|NativePromotion {canonical:request.canonical,working:request.working,region:request.region}).collect::<Vec<_>>();
            let promotions=native.promoter.as_ref().map(|promoter|promoter.prepare(&self.device,&promotions,&mut views)).transpose()?;
            {let mut pass=encoder.begin_compute_pass(&Default::default());native.scalar.encode(&mut pass,&scalar);}
            if let (Some(promoter),Some(promotions))=(&native.promoter,promotions) {
                encoder.reserve_passes(promotions.pass_count());promoter.encode(encoder,&promotions);
            }
        }
        let actual=encoder.submission();
        for (coordinate,_,ticket,encoding) in outputs {
            encoding.publish(&actual);
            self.preview_watercolor_wetness_pages.iter_mut().find(|page|page.coordinate==coordinate).unwrap().derived=Some(ticket);
        }
        Ok(())
    }

    /// Admitted display, decoded-source and in-flight upload ceilings in bytes.
    pub fn display_memory_limits(&self) -> [u64; 3] {
        [self.memory.display_grant(), self.scene_tiers.display.limit(), self.source_tiles.borrow().upload_limit()]
    }
    pub(crate) fn image_pixel_budget(&self, resident: u64) -> u64 {
        #[cfg(test)]
        if let Some(bytes) = self.native_edit.as_ref().and_then(|native| native.image_pixel_bytes) { return bytes; }
        self.memory.display_grant().saturating_sub(resident)
    }
    pub(crate) fn preparation_memory_limits(&self) -> [u64; 2] {
        [self.memory.display_grant(), crate::memory::retained_limit(self.memory.raster_grant(), self.raster_buffers.memory_bytes()[1])]
    }

    /// A private candidate shares immutable source samples with the current
    /// canvas and its workers, so preview/adoption cannot double that cache.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn color_candidate_staged(
        &self,
        color: DocumentColor,
    ) -> Result<Self, GpuRasterError> {
        let mut device = self.factory.pipeline_device(color);
        device.source_samples = self.device.source_samples.clone();
        device.sample_memory = self.device.sample_memory.clone();
        Self::native_staged_on_factory(self.factory.clone(), device, color).map(|renderer| *renderer)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_wgpu_native_staged_boxed(
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        color: DocumentColor,
    ) -> Result<Box<Self>, GpuRasterError> {
        Self::native_staged_on_device(adapter, device.into(), queue, color)
    }

    /// Portable native integer SDR backing with Float32 working pixels.
    pub fn from_wgpu_native_staged(
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        color: DocumentColor,
    ) -> Result<Self, GpuRasterError> {
        Self::native_staged_on_device(adapter, device.into(), queue, color).map(|renderer| *renderer)
    }

    pub(crate) fn native_staged_on_device(
        adapter: wgpu::Adapter,
        device: PipelineDevice,
        queue: wgpu::Queue,
        color: DocumentColor,
    ) -> Result<Box<Self>, GpuRasterError> {
        Self::native_staged_on_factory(RendererFactory::with_device(adapter, &device, queue), device, color)
    }
    pub(crate) fn native_staged_on_factory(factory: RendererFactory, device: PipelineDevice, color: DocumentColor) -> Result<Box<Self>, GpuRasterError> {
        let device = device.with_working_space(color.space).with_depth(color.depth);
        let mut r = Self::from_factory_inner(factory, device, Initialization::Interactive)?;
        r.startup.as_mut().unwrap().host_catalog_pending = !cfg!(target_arch = "wasm32");
        r.initialize_native(color)?;
        Ok(r)
    }

    /// Native document renderer for headless workflow qualification.
    /// Dab RGB values are linear coordinates in `color.space`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_native_headless(color: DocumentColor) -> Result<Self, GpuRasterError> {
        Self::native_headless_without(color, wgpu::Features::empty())
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn native_headless_without(color: DocumentColor, omitted: wgpu::Features) -> Result<Self, GpuRasterError> {
        let mut r = pollster::block_on(Self::headless(color, Initialization::Headless, omitted))?;
        r.initialize_native(color)?;
        Ok(r)
    }

    #[cfg(test)]
    pub(crate) fn native_capture_on_gpu(
        adapter: wgpu::Adapter,
        device: PipelineDevice,
        queue: wgpu::Queue,
        color: DocumentColor,
    ) -> Result<Self, GpuRasterError> {
        Self::native_capture_on_factory(RendererFactory::with_device(adapter, &device, queue), device, color)
    }
    pub(crate) fn native_capture_on_factory(factory: RendererFactory, device: PipelineDevice, color: DocumentColor) -> Result<Self, GpuRasterError> {
        let device = device.with_working_space(color.space).with_depth(color.depth);
        let mut r = Self::from_factory_inner(factory, device, Initialization::Snapshot)?;
        r.initialize_native(color)?;
        Ok(*r)
    }

    pub(crate) fn native_preparation(&self)->Result<Self,GpuRasterError> {
        let mut renderer=Self::from_factory_inner(self.factory.clone(),self.device.clone(),Initialization::Snapshot)?;
        renderer.initialize_native_from(self.document_color(),self.native_edit.as_ref())?;
        renderer.snapshot_worker=false;
        renderer.source_tiles=std::cell::RefCell::new(crate::scene::sources::DecodedTiles::within(&renderer.scene_tiers.display,self.document_color().space));
        renderer.memory.invalidate_limits();
        renderer.refresh_memory_limits();
        renderer.compilation=self.compilation.clone();
        #[cfg(not(target_arch="wasm32"))]
        if let Some(wake)=&self.preparation_wake {renderer.set_preparation_wake(wake.clone());}
        #[cfg(target_arch="wasm32")]
        {
            renderer.browser_image_decoder=self.browser_image_decoder.clone();
            renderer.browser_raster_decoder=self.browser_raster_decoder.clone();
            renderer.browser_nearest_coordinate_decoder=self.browser_nearest_coordinate_decoder.clone();
        }
        Ok(*renderer)
    }

    pub(crate) fn initialize_native(&mut self, color: DocumentColor) -> Result<(), GpuRasterError> {
        self.initialize_native_from(color,None)
    }
    fn initialize_native_from(&mut self,color:DocumentColor,source:Option<&NativeEdit>)->Result<(),GpuRasterError> {
        self.restore_preflight=None;self.bake_preflight=None;self.source_receipt=None;self.source_results.clear();self.moving_decode=None;
        #[cfg(target_arch="wasm32")]
        {self.browser_image_result=Default::default();self.browser_image_wake=Default::default();}
        self.document_color = color;
        self.device = self.device.clone().with_depth(color.depth);
        self.ui_rendition = color.depth.is_float().then_some(Default::default());
        self.scene = None;
        self.source_tiles = std::cell::RefCell::new(crate::scene::sources::DecodedTiles::within(&self.scene_tiers.display,color.space));
        let transfer=if let Some(source)=source {source.transfer.clone()}else {self.prepare_native_transfer(color.space)?};
        self.native_edit = Some(NativeEdit::new(self,transfer));
        self.memory.invalidate_limits();
        self.refresh_memory_limits();
        Ok(())
    }

    pub(crate) fn encode_native_rasters(&mut self, scene: SceneView<'_>, encoder: &mut submission::CommandEncoder) -> Result<Option<NativeFrame>, GpuRasterError> {
        if self.restore_preflight.as_ref().and_then(|restore|restore.native.as_ref()).is_some_and(|admission|!admission.whole()) {return Err(GpuRasterError::NotReady);}
        let submission=encoder.submission();
        let Some(mut job) = self.prepare_native_rasters(scene,Some(&submission))? else { return Ok(None); };
        let timed = if crate::performance_trace::enabled() {
            let passes = job.pass_count(self.native_edit.as_ref().unwrap());
            let timed = encoder.can_fit_passes(passes + NativeJob::TIMING_PASSES)
                && self.telemetry.phase_begin(14, &self.device, &self.queue, encoder);
            if !timed {
                crate::performance_trace::counter(c"Capy GPU native capture omitted", self.metrics.submissions);
            }
            timed
        } else { false };
        if timed { encoder.reserve_passes(2); }
        let encoding = timed && self.telemetry.phase_begin(15, &self.device, &self.queue, encoder);
        if encoding { encoder.reserve_passes(2); }
        else if timed { crate::performance_trace::counter(c"Capy GPU native encoding omitted", self.metrics.submissions); }
        let result = (|| -> Result<(), GpuRasterError> {
            while self.step_native_rasters(&mut job, encoder)? {}
            Ok(())
        })();
        if encoding { self.telemetry.phase_end(15, encoder); }
        if timed { self.telemetry.phase_end(14, encoder); }
        result?;
        Ok(Some(job.frame))
    }

    fn native_capture_keys(&self,scene:SceneView<'_>,target:SourceTarget,planned:impl IntoIterator<Item=TileKey>)->BTreeSet<TileKey> {
        let Some(current)=self.raster.as_ref().and_then(|runtime|runtime.targets.get(&target)) else {return BTreeSet::new();};
        native_capture_candidates(&current.contents,planned,|key| {
            key.plane.descriptor_for(self.document_color(),scene.color_mode(target))
        })
    }

    pub(crate) fn prepare_native_admission(&mut self,scene:SceneView<'_>,outputs:&[(SourceTarget,RasterOutput)])
        ->Result<NativeAdmission,GpuRasterError> {
        let pending:BTreeSet<_>=source_access::placed_targets(scene).filter(|target|scene.raster(*target).is_some_and(|root|root.try_data().is_none())).collect();
        if !pending.is_empty() {
            let runtime=self.raster.get_or_insert_with(Default::default);
            runtime.ensure_worker()?;
            if runtime.worker.as_ref().is_some_and(|worker|!worker.ready()) {return Err(GpuRasterError::NotReady);}
        }
        let mut keys=BTreeSet::new();
        for &(target,ref output) in outputs {
            if !pending.contains(&target) {continue;}
            if let Some(key)=output.native_key() {keys.insert((target,key));}
        }
        for &target in &pending {keys.extend(self.native_capture_keys(scene,target,std::iter::empty()).into_iter().map(|key|(target,key)));}
        let mut bytes=0u64;let mut largest=0;
        for &(target,key) in &keys {
            let size=key.plane.descriptor_for(self.document_color(),scene.color_mode(target)).byte_len([PAGE_SIZE;2]).unwrap() as u64;
            bytes=bytes.checked_add(size).ok_or(GpuRasterError::SizeOverflow)?;largest=largest.max(size);
        }
        if bytes>layer_core::raster::MAX_PUBLICATION_BYTES {return Err(GpuRasterError::SourceWorkingSetExceeded);}
        let capacity=keys.len();
        let [readback,least]=if capacity==0 {[0;2]}else {[capture_readback(bytes,largest,u64::MAX),capture_readback(largest,largest,0)]};
        let status=if capacity==0 {0}else {crate::native_tiles::comparison::CanonicalComparisons::bytes(capacity)?};
        let total=bytes.checked_add(readback).and_then(|n|n.checked_add(status)).ok_or(GpuRasterError::SizeOverflow)?;
        let allocation=self.admit_raster_window(least.saturating_add(status).saturating_add(largest),total,self.projected_restore_bytes());self.note_restore_admission(&allocation);
        let mut allocation=allocation.map_err(|_|GpuRasterError::NotReady)?;
        let comparisons=if capacity==0 {None}else {Some(crate::native_tiles::comparison::CanonicalComparisons::reserved(self,capacity,&mut allocation)?)};
        let readback=if capacity==0 {0}else {capture_readback(bytes,largest,allocation.bytes())};
        Ok(NativeAdmission {allocation,comparisons,capacity,bytes,readback})
    }

    pub(crate) fn prepare_native_rasters(&mut self, scene: SceneView<'_>, submission:Option<&submission::Submission>) -> Result<Option<NativeJob>, GpuRasterError> {
        if self.native_edit.is_none()
            || !source_access::placed_targets(scene).any(|target| scene.raster(target).is_some_and(|r| r.try_data().is_none()))
        {
            if let Some(restore)=&mut self.restore_preflight {restore.native=None;}
            return Ok(None);
        }
        let mut frame = NativeFrame {
            capture: None,
            publications: Vec::new(),
            canonical_pages: Vec::new(),
        };
        // Reserve the ordinary backing worker before borrowing live textures.
        let runtime = self.raster.get_or_insert_with(Default::default);
        if runtime.worker.as_ref().is_some_and(|w| !w.ready()) {
            return Err(GpuRasterError::Effect(
                "Raster backing queue is full".into(),
            ));
        }
        runtime.ensure_worker()?;
        let runtime = self.raster.as_ref().unwrap();
        let mut inputs = Vec::new();
        let mut canonical=Vec::new();let mut intents=Vec::new();let mut working_inputs=Vec::new();
        for target in source_access::placed_targets(scene).filter(|t| scene.raster(*t).is_some()) {
            for (id, revision) in std::iter::once((target, scene.raster(target).unwrap()))
            {
                if revision.try_data().is_some() {
                    continue;
                }
                let Some(current) = runtime.targets.get(&id) else {
                    continue;
                };
                if current.revision!=*revision {frame.publications.clear();return Err(GpuRasterError::Invariant("prepared prepare native rasters state is inconsistent"));}
                let planned=self.restore_preflight.as_ref().map_or(&[][..],|restore|restore.outputs());
                let needed=self.native_capture_keys(scene,id,planned.iter().filter_map(|(target,output)|(*target==id).then(||output.native_key()).flatten()));
                let (physical,watercolor)=self.raster_inputs(id,submission,|key|needed.contains(&key))?;
                if physical.len()!=needed.len() {frame.publications.clear();return Err(GpuRasterError::Invariant("prepared prepare native rasters state is inconsistent"));}
                let mut data = RasterData {
                    // Cold, losslessly backed tiles remain part of every new
                    // revision even when they own no working GPU surface.
                    tiles: current.contents.data().tiles.clone(),
                    watercolor,
                };
                for (key,pixels) in physical {
                    let intent=current.contents.intent(key);
                    let read=pixels.read(intent.version(),submission).ok_or(GpuRasterError::Invariant("accepted input is not readable"))?;
                    let texture=crate::resident::TextureStorage::texture(read.value()).clone();
                    let (tile,name)=RasterTile::pending_from_live(key.plane.descriptor_for(self.document_color(),scene.color_mode(id)),intent.version().clone());
                    canonical.push(Some(name));intents.push(Some(intent));working_inputs.push(Some(pixels));
                    inputs.push((texture,tile.clone(),scene.color_mode(id)));
                    frame.canonical_pages.push((id,key.coordinate));
                    data.tiles.insert(key,tile);
                }
                frame.publications.push(Publication {
                    id,
                    revision: revision.clone(),
                    data,
                });
            }
        }
        if frame.publications.is_empty() {
            return Ok(None);
        }
        self.native_capture_job(frame, inputs,canonical,intents,working_inputs).map(Some)
    }

    fn native_capture_job(&mut self, mut frame: NativeFrame, inputs: Vec<(wgpu::Texture, RasterTile, layer_core::color::LayerColorMode)>,
        canonical:Vec<Option<Arc<layer_core::raster::CanonicalName>>>,intents:Vec<Option<crate::raster::contents::Intent>>,
        working_inputs:Vec<Option<crate::resident::Resident<source_access::RawTile>>>) -> Result<NativeJob, GpuRasterError> {
        let output_bytes: u64 = inputs
            .iter()
            .map(|(_, tile, _)| tile.descriptor().byte_len([PAGE_SIZE; 2]).unwrap() as u64)
            .sum();
        if output_bytes > layer_core::raster::MAX_PUBLICATION_BYTES {
            return Err(GpuRasterError::Effect(
                "Raster frame exceeds the 1 GiB native output budget".into(),
            ));
        }
        validate_working_inputs(&inputs)?;
        let live=!frame.publications.is_empty();
        let (allocation,comparisons,readback)=if live {
            let admission=self.restore_preflight.as_mut().and_then(|restore|restore.native.take());
            let Some(admission)=admission else {frame.publications.clear();return Err(GpuRasterError::Invariant("prepared native capture job state is inconsistent"));};
            if inputs.len()>admission.capacity || output_bytes>admission.bytes {frame.publications.clear();return Err(GpuRasterError::Invariant("prepared native capture job state is inconsistent"));}
            (admission.allocation,admission.comparisons,admission.readback)
        }else {
            let readback=if output_bytes==0 {0}else {output_bytes.min(CAPTURE_CHUNK).next_power_of_two()};
            let Ok(allocation)=self.admit_raster_working(output_bytes.saturating_add(readback)) else {return Err(GpuRasterError::NotReady);};
            (allocation,None,readback)
        };
        for publication in &frame.publications {
            let pending_bytes = publication.data.tiles.values()
                .filter(|tile| tile.try_backing().is_none())
                .map(|tile| layer_core::raster::TileBlob::max_compressed_len(tile.descriptor()).unwrap() as u64)
                .sum::<u64>();
            publication.revision.reserve_pending_bytes(pending_bytes);
        }
        frame.capture = Some(NativeCapture {
            outputs: Vec::with_capacity(inputs.len()),
            pool: self.raster_buffers.clone(),
            device: self.device.for_resource(),
            queue: self.queue.clone(),
            readback:None,
            allocation,
            owner:self.memory.raster_owner(),
        });
        frame.capture.as_mut().unwrap().prepare_readback(readback);
        drop(frame.capture.as_mut().unwrap().allocation.split(readback).ok_or(GpuRasterError::Invariant("native readback exceeds its working reservation"))?);
        self.native_edit.as_mut().unwrap().reserve_scratch(&self.device, &inputs);
        let changes=frame.canonical_pages.iter().map(|&(id,coordinate)|self.changed_cells.as_ref().and_then(|c|c.buffer(id,coordinate)).cloned()).collect();
        Ok(NativeJob {frame,inputs,views:Default::default(),changes,comparisons,canonical,intents,working_inputs,encoded:0,started:false})
    }

    pub(crate) fn encode_private_tiles(&mut self, inputs: Vec<(wgpu::Texture, RasterTile, layer_core::color::LayerColorMode)>, encoder: &mut submission::CommandEncoder)
        -> Result<NativeCapture, GpuRasterError> {
        #[cfg(test)]
        crate::scene::sources::assert_reading("private tile capture");
        let frame = NativeFrame { capture: None, publications: Vec::new(), canonical_pages: Vec::new() };
        let count=inputs.len();
        let mut job = self.native_capture_job(frame, inputs,vec![None;count],vec![None;count],vec![None;count])?;
        while self.step_native_rasters(&mut job, encoder)? {}
        Ok(job.frame.capture.take().unwrap())
    }

    pub(crate) fn submit_private_tiles(&self,capture:NativeCapture)->Result<(),GpuRasterError> {
        self.raster.as_ref().unwrap().worker.as_ref().unwrap().submit_batch(capture)
    }

    pub(crate) fn admit_native_window(&mut self, job: &mut NativeJob) -> Result<(), GpuRasterError> {
        let capture=job.frame.capture.as_mut().ok_or(GpuRasterError::Invariant("a native job holds its capture"))?;
        if job.inputs.get(job.encoded).is_none_or(|(_,tile,_)|tile_output_bytes(tile)<=capture.allocation.bytes()) {return Ok(());}
        if !capture.outputs.is_empty() {
            let worker=self.raster.as_ref().and_then(|runtime|runtime.worker.as_ref()).ok_or(GpuRasterError::Invariant("a native capture has a backing worker"))?;
            if !worker.ready() {return Err(GpuRasterError::NotReady);}
            worker.submit_batch(capture.take_window())?;
        }
        let (bytes,largest)=job.inputs[job.encoded..].iter().map(|(_,tile,_)|tile_output_bytes(tile)).fold((0u64,0u64),|(bytes,largest),size|(bytes.saturating_add(size),largest.max(size)));
        let allocation=self.admit_raster_window(capture_readback(largest,largest,0).saturating_add(largest),capture_readback(bytes,largest,u64::MAX).saturating_add(bytes),0).map_err(|_|GpuRasterError::NotReady)?;
        let readback=capture_readback(bytes,largest,allocation.bytes());
        let capture=job.frame.capture.as_mut().unwrap();
        capture.allocation=allocation;capture.prepare_readback(readback);
        drop(capture.allocation.split(readback).ok_or(GpuRasterError::Invariant("native readback exceeds its working reservation"))?);
        Ok(())
    }

    pub(crate) fn step_native_rasters(&mut self, job: &mut NativeJob, encoder: &mut submission::CommandEncoder) -> Result<bool, GpuRasterError> {
        let native = self.native_edit.as_ref().unwrap();
        let capture = job.frame.capture.as_mut().unwrap();
        if !job.started { job.started = true; }
        else if job.encoded == job.inputs.len() { return Ok(false); }
        let views = &mut job.views;
        let mut room = capture.allocation.bytes();
        let end = job.encoded + job.inputs[job.encoded..(job.encoded + MAX_BATCH_TILES).min(job.inputs.len())].iter()
            .take_while(|(_, tile, _)| room.checked_sub(tile_output_bytes(tile)).inspect(|left| room = *left).is_some()).count();
        if end == job.encoded && end < job.inputs.len() { return Err(GpuRasterError::Invariant("native outputs exceed their working reservation")); }
        let chunk = &job.inputs[job.encoded..end];
        let submission=encoder.submission();
        let mut owned_inputs=job.working_inputs[job.encoded..end].to_vec();
        let mutate=!self.snapshot_worker || native.promoter.is_none();
        let access=owned_inputs.iter_mut().zip(chunk).enumerate().map(|(index,(pixels,(texture,_,_)))| {
            match pixels {
                Some(pixels)=> {
                    let intent=job.intents[job.encoded+index].as_ref().ok_or(GpuRasterError::Invariant("prepared step native rasters state is inconsistent"))?;
                    let target=job.frame.canonical_pages[job.encoded+index].0;self.validate_raster_intent(target,intent)?;
                    if !pixels.holds_texture(texture) {return Err(GpuRasterError::Invariant("prepared step native rasters state is inconsistent"));}
                    if mutate {pixels.write(intent,&submission).map(NativeAccess::Write).ok_or(GpuRasterError::Invariant("accepted destination is not writable"))}
                    else {pixels.read(intent.version(),Some(&submission)).map(NativeAccess::Read).ok_or(GpuRasterError::Invariant("accepted input is not readable"))}
                },
                None if job.intents[job.encoded+index].is_none()=>Ok(NativeAccess::Private(texture)),
                None=>Err(GpuRasterError::Invariant("prepared step native rasters state is inconsistent")),
            }
        }).collect::<Result<Vec<_>,GpuRasterError>>()?;
        if !chunk.is_empty() {
            let first = capture.outputs.len();
            capture.outputs.extend(chunk.iter().map(|(_,tile,_)| {
                let resource=self.raster_buffers.take_native(&self.device,tile.descriptor());
                let charge=self.raster_buffers.charge(resource.bytes());
                NativeOutput::new(resource,tile.clone(),charge)
            }));
            let writes = capture.outputs[first..].iter_mut().map(|output| output.initialize(&submission)
                .ok_or(GpuRasterError::Invariant("prepared step native rasters state is inconsistent"))).collect::<Result<Vec<_>, _>>()?;
            let mut color = Vec::new();
            let mut scalar = Vec::new();
            let mut color_changes=Vec::new();
            let mut promotions = Vec::new();
            for (index,((_, tile, mode), output)) in chunk.iter().zip(&writes).enumerate() {
                let texture=access[index].texture();
                let canonical = if tile.descriptor().channels != 1 {
                    let canonical = native.colors.get(color.len()).unwrap_or(texture);
                    let encoded = match output.value() { pool::Resource::Texture(t) => t.into(), pool::Resource::Buffer(b) => b.into() };
                    color_changes.push(job.changes.get(job.encoded+index).and_then(Option::as_ref));
                    color.push(NativeTileRequest {
                        mode: *mode, space: self.document_color().space,
                        working: texture,
                        encoded,
                        canonical,
                        transfer: &native.transfer,
                        depth: self.document_color().depth,
                        alpha: tile.descriptor().alpha,
                        region: [0, 0, 256, 256],
                    });
                    canonical
                } else {
                    let canonical = native.scalars.get(scalar.len()).unwrap_or(texture);
                    let pool::Resource::Buffer(encoded) = output.value() else {
                        unreachable!()
                    };
                    scalar.push(NativeScalarRequest {
                        working: texture,
                        encoded,
                        canonical,
                        depth: self.document_color().depth.coverage(),
                        region: [0, 0, 256, 256],
                    });
                    canonical
                };
                promotions.push(NativePromotion {
                    canonical,
                    working: texture,
                    region: [0, 0, 256, 256],
                });
            }
            let color_comparisons=job.comparisons.as_ref().filter(|_|!color.is_empty()).map(|c|c.window(job.encoded,color.len()));
            let scalar_comparisons=job.comparisons.as_ref().filter(|_|!scalar.is_empty()).map(|c|c.window(job.encoded+color.len(),scalar.len()));
            let color_count=color.len();
            let color=if color_comparisons.is_none() && color_changes.iter().all(Option::is_none) {
                native.color.prepare(&self.device,&color,views)?
            }else {native.color.prepare_compared(&self.device,&color,views,&color_changes,color_comparisons.as_ref())?};
            let scalar=if scalar_comparisons.is_none() {native.scalar.prepare(&self.device,&scalar,views)?}
                else {native.scalar.prepare_compared(&self.device,&scalar,views,scalar_comparisons.as_ref())?};
            let promotions = native
                .promoter
                .as_ref()
                .filter(|_| !self.snapshot_worker)
                .map(|promoter| {
                    promoter.prepare(&self.device, &promotions, views)
                })
                .transpose()?;
            let output_encodings=writes.into_iter().zip(chunk).map(|(write,(_,tile,_))|write.encoded(tile.capture_name())).collect::<Vec<_>>();
            let working_encodings=access.into_iter().zip(chunk).map(|(pixels,(_,tile,_))|pixels.encoded(tile.content_version())).collect::<Vec<_>>();
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                native.color.encode(&mut pass, &color);
                native.scalar.encode(&mut pass, &scalar);
            }
            let encoded_submission=encoder.submission();
            for encoding in output_encodings {encoding.publish(&encoded_submission);}
            if let (Some(promoter), Some(promotions)) = (&native.promoter, promotions) {
                encoder.reserve_passes(promotions.pass_count());
                promoter.encode(encoder, &promotions);
            }
            let mut color_index=0;let mut scalar_index=0;
            for (index,(_,tile,_)) in chunk.iter().enumerate() {
                let slot=if tile.descriptor().channels==1 {let n=color_count+scalar_index;scalar_index+=1;n}else {let n=color_index;color_index+=1;n};
                if let (Some(comparisons),Some(name))=(&job.comparisons,&job.canonical[job.encoded+index]) {
                    capture.outputs[first+index].comparison=Some(comparisons.result(job.encoded+slot,name.clone()));
                }
            }

            let working_submission=encoder.submission();
            for (index,encoding) in working_encodings.into_iter().enumerate() {
                if let Some(encoding)=encoding {
                    let target=job.frame.canonical_pages[job.encoded+index].0;
                    self.record_raster_write(target,encoding.publish(&working_submission))?;
                }
            }
        }
        job.encoded=end;
        let bytes=job.inputs[end-chunk.len()..end].iter().map(|(_,tile,_)|tile_output_bytes(tile)).sum();
        drop(capture.allocation.split(bytes).ok_or(GpuRasterError::Invariant("native outputs exceed their working reservation"))?);
        Ok(true)
    }

    pub(crate) fn finish_native_rasters(
        &mut self,
        mut frame: NativeFrame,
        commit: bool,
    ) -> Result<(), GpuRasterError> {
        let runtime = self.raster.as_mut().unwrap();
        if let Some(capture) = frame.capture.take()
            && !capture.outputs.is_empty() {
                runtime
                    .worker
                    .as_ref()
                    .unwrap()
                    .submit_batch(capture)?;
            }
        while let Some(publication) = frame.publications.pop() {
            let data = if commit {
                publication.revision.publish(Ok(publication.data)).map_err(GpuRasterError::Effect)?;
                publication.revision.wait_data().map_err(GpuRasterError::Effect)?
            } else {
                Arc::new(publication.data)
            };
            let current = runtime.targets.get_mut(&publication.id).unwrap();
            current.revision = publication.revision;
            current.contents.published(data);
            self.native_edit
                .as_mut()
                .unwrap()
                .backing
                .insert(publication.id, current.contents.data().clone());
        }
        Ok(())
    }
}

#[expect(clippy::mutable_key_type, reason = "Texture equality and hashing use immutable GPU handle identities")]
fn validate_working_inputs(inputs: &[(wgpu::Texture, RasterTile, layer_core::color::LayerColorMode)]) -> Result<(), GpuRasterError> {
    let mut identities = std::collections::HashSet::new();
    for (texture, _, _) in inputs {
        if !identities.insert(texture.clone())
            || texture.size() != (wgpu::Extent3d { width: 256, height: 256, depth_or_array_layers: 1 })
            || texture.mip_level_count() != 1
            || texture.sample_count() != 1
            || texture.dimension() != wgpu::TextureDimension::D2
            || !matches!(texture.format(), wgpu::TextureFormat::Rgba32Float | wgpu::TextureFormat::R32Float)
            || !texture.usage().contains(wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING)
        {
            return Err(GpuRasterError::Color("Invalid native working ownership or layout".into()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
