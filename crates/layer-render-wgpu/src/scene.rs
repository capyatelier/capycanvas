//! Tiled layer composition with explicit cached image boundaries. Pointwise
//! scratch follows nesting depth. Masks never download or rewrite paint.
use super::*;
use layer_core::{SceneView, SourceTarget, LayerKind};
use layer_core::authored::{OccurrenceHandle, MaskUse, CoverageSource};
#[path = "scene_images.rs"]
mod images;
#[path = "filter_previews.rs"]
mod previews;
mod metadata;
pub(super) fn same_metadata(r:&WgpuRasterizer,packet:FramePacket<'_>)->bool {
    r.artwork_frame.as_ref().is_some_and(|old| {
        let scene=old.scene.view().with_scope(&old.scope);
        old.blend_space==packet.blend_space && scene.composition().color==packet.scene.composition().color
            && scene.composition().size==packet.scene.composition().size && scene.order()==packet.scene.order()
            && (old.time==packet.time_seconds || !packet.scene.order().iter().any(|h|packet.scene.effect(*h).is_some_and(|e|e.animated())))
            && packet.scene.order().iter().all(|h|metadata::Metadata::new(scene,*h)==metadata::Metadata::new(packet.scene,*h)
                && packet.scene.effect(*h).is_none_or(|_|effects::Gpu::effect_time(r,scene,*h,old.time).to_bits()
                    ==effects::Gpu::effect_time(r,packet.scene,*h,packet.time_seconds).to_bits())
                && scene.paint_source(*h).zip(packet.scene.paint_source(*h)).is_none_or(|(old,new)|
                    old.color_mode==new.color_mode && old.domain==new.domain && old.operations==new.operations))
    })
}
mod stack;
pub(crate) mod windows;
pub(super) use previews::FilterPreviews;
pub(super) mod sources;
mod placement;
mod objects;
mod object_cache;
mod object_spatial;
mod bake;
pub(crate) mod resample;
pub(crate) mod scale;

#[derive(Clone)]
struct Image {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    plan: display_mips::Plan,
}
impl Image {
    fn new(r: &WgpuRasterizer, plan: display_mips::Plan, label: &'static str) -> Self {
        let (texture, view) = create_color_target(&r.device, plan.size, label);
        Self { texture, view, plan }
    }
    fn bytes(&self) -> u64 { texture_bytes(&self.texture) }
}

struct ColorInput {
    view: wgpu::TextureView,
    lease: Option<Arc<()>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Reduction { Pages, Phased }
#[derive(Clone)]
enum Job {
    Collection(Box<crate::object_sampling::CollectionPreview>),
    PaintBase(Arc<crate::source_access::PaintBaseAssembly>),
    Positions(Arc<paint_transform::mesh::MeshGeometry>, [u32; 2]),
    Placement(Box<placement::PlacementJob>),
    Reduce { binding: wgpu::BindGroup, values: [u32; 20], size: [u32; 2], kernel: Reduction },
    DecodedTile(std::sync::Arc<sources::PendingTile>),
    Effect {
        target: wgpu::TextureView,
        sources: [wgpu::TextureView; 3],
        data: [f32; 32],
        prepared: effects::PreparedEffect,
        // Keep ordinary draw jobs small: only effects carry the mask inputs.
        masks: Box<[wgpu::TextureView; effects::MASK_SLOTS]>,
        source_target: Option<SourceTarget>,
        changed_cells: Option<wgpu::Buffer>,
    },
    Draw {
        target: wgpu::TextureView,
        sources: [wgpu::TextureView; 3],
        data: [f32; 32],
        over: bool,
        clip: Option<PixelRect>,
        source_target: Option<SourceTarget>,
    },
    Clear(wgpu::TextureView, wgpu::Color),
    Watercolor {
        coordinates: (wgpu::BindGroup, u32),
        target: wgpu::TextureView,
        binding: wgpu::BindGroup,
        record: u32,
    },
    Copy {
        source: wgpu::Texture,
        source_origin: [u32; 2],
        destination: wgpu::Texture,
        origin: [u32; 2],
        width: u32,
        height: u32,
    },
}
/// What a draw does to the color it writes (`scene_space` in scene.wgsl).
/// Layer pixels are linear; a Perceptual composite holds encoded values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Convert {
    None,
    Encode,
    Decode,
    Store,
    DecodeStore,
}
impl Convert {
    fn code(self) -> f32 {
        match self {
            Self::None => 0.,
            Self::Encode => 1.,
            Self::Decode => 2.,
            Self::Store => 3.,
            Self::DecodeStore => 4.,
        }
    }
    /// How the composite of `packet` becomes pixels a layer's pages store:
    /// linear, with coverage within 0–1, which filters that spread coverage
    /// can round past.
    pub(super) fn stored(packet: FramePacket<'_>) -> Self {
        if packet.blend_space == layer_core::BlendSpace::Perceptual { Self::DecodeStore } else { Self::Store }
    }
    /// How a layer's own pixels enter the composite of `packet`.
    pub(super) fn layers(packet: FramePacket<'_>) -> Self {
        if packet.blend_space == layer_core::BlendSpace::Perceptual { Self::Encode } else { Self::None }
    }
    /// How the composite of `packet` becomes linear pixels.
    pub(super) fn linear(packet: FramePacket<'_>) -> Self {
        if packet.blend_space == layer_core::BlendSpace::Perceptual { Self::Decode } else { Self::None }
    }
    /// How the composite of `packet` becomes the input of a filter that reads
    /// values in `space`.
    pub(super) fn filter_input(packet: FramePacket<'_>, space: layer_core::EffectSpace) -> Self {
        if space.encoded(packet.blend_space) { Self::None } else { Self::linear(packet) }
    }
}
/// The premultiplied `color` as the composite of `packet` holds it.
fn composite_color(r: &WgpuRasterizer, packet: FramePacket<'_>, [red, green, blue, alpha]: [f32; 4]) -> wgpu::Color {
    let [red, green, blue, alpha] = packet.blend_space.composite(r.device.working_space(), [red * alpha, green * alpha, blue * alpha, alpha]);
    wgpu::Color { r: f64::from(red), g: f64::from(green), b: f64::from(blue), a: f64::from(alpha) }
}
#[derive(Clone, Copy)]
pub(super) enum Output { Artwork(Option<OccurrenceHandle>), OwnerContent(OccurrenceHandle), Objects(OccurrenceHandle), EffectInput(OccurrenceHandle), EffectChannels(OccurrenceHandle), EffectComposite(OccurrenceHandle), Source(SourceTarget), MaskImage(SourceTarget), Display }

pub(super) struct Scene {
    valid: Arc<std::sync::atomic::AtomicBool>,
    placement: [pixel_transform::PixelTransform; 2],
    positions: paint_transform::mesh::Positions,
    position_key: Option<(layer_core::LayerPlacement, [u32; 2])>,
    mesh_geometry: std::cell::RefCell<Vec<(layer_core::LayerPlacement, Arc<paint_transform::mesh::MeshGeometry>)>>,
    material_coordinates: wgpu::BindGroup,
    material_pages: std::collections::VecDeque<placement::MaterialPage>,
    material_bounds: std::collections::HashMap<SourceTarget, DocRect>,
    placement_display: bool,
    object_display: bool,
    object_source_damage: scale::Damage,
    object_live: bool,
    object_query: bool,
    native_reverse: bool,
    scale_sources: scale::Sources,
    object_results: object_cache::ObjectCache,
    exact_object_results: object_cache::ObjectCache,
    object_spatial: object_spatial::SpatialIndex,
    scale_commands: Option<scale::Commands>,
    pool: Vec<PageSurface>,
    used: Vec<bool>,
    jobs: Vec<Job>,
    source_jobs: Vec<std::ops::Range<usize>>,
    // Retain table capacity across tile batches, but release resource handles
    // after encoding so these tables cannot pin evicted paint/source pages.
    source_bindings: RecentBindings<(bool,[wgpu::TextureView; 3])>,
    compute_bindings: RecentBindings<[wgpu::TextureView; 3]>,
    output_bindings: RecentBindings<wgpu::TextureView>,
    watercolor_outputs: RecentBindings<wgpu::TextureView>,
    mask_bindings: RecentBindings<([wgpu::TextureView; effects::MASK_SLOTS],wgpu::Buffer)>,
    effect_outputs: RecentBindings<(usize,[wgpu::TextureView; effects::MASK_SLOTS],wgpu::TextureView)>,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::BindGroupLayout,
    buffer: wgpu::Buffer,
    binding: wgpu::BindGroup,
    reduction_binding: wgpu::BindGroup,
    stride: usize,
    capacity: usize,
    record_count: usize,
    upload: Vec<u8>,
    pipeline: [Deferred<wgpu::RenderPipeline>; 3],
    pub(super) effects: effects::Effects,
    pub effect_passes: u64,
    images: images::ImageStages,
    image_window: Option<DocRect>,
    image_evaluation_origin: [i64; 2],
    stop_before: Option<OccurrenceHandle>,
    image_damage: Option<scale::Damage>,
}

/// Immutable device resources, compiled before input is enabled and shared by
/// live composition, captures and recreated scenes. No canvas pixels retained.
#[derive(Clone)]
pub(super) struct Pipelines {
    pub objects: crate::object_sampling::ObjectSampler,
    pub scale: scale::Pipelines,
    pub resample: resample::Resample,
    uniforms: wgpu::BindGroupLayout,
    layout: wgpu::BindGroupLayout,
    pub pipeline: [Deferred<wgpu::RenderPipeline>; 3],
    pub source: sources::Pipelines,
    pub constant: (wgpu::BindGroupLayout, wgpu::BindGroupLayout, Deferred<wgpu::ComputePipeline>),
}

impl Scene {
    pub(super) fn analysis_changed(&mut self) {
        self.retire_images(|scene| scene.images = images::ImageStages::default());
    }
    fn begin_write(&mut self, r: &WgpuRasterizer) -> crate::submission::CacheWrite {
        if !self.valid.load(std::sync::atomic::Ordering::Acquire) {
            let exact = std::mem::take(&mut self.exact_object_results);
            let display = std::mem::take(&mut self.object_results);
            let objects = std::mem::take(&mut self.object_spatial);
            let damage = std::mem::take(&mut self.object_source_damage);
            let query = self.object_query;
            *self = Self::new(r);
            self.exact_object_results = exact;
            self.object_results = display;
            self.object_spatial = objects;
            self.object_source_damage = damage;
            self.object_query = query;
        }
        crate::submission::CacheWrite::shared(self.valid.clone())
    }
    #[cfg(test)]
    pub fn image_pass_pixels(&self) -> u64 {
        self.images.pass_pixels
    }
    #[cfg(test)]
    pub fn image_work(&self) -> [u64; 2] {
        [self.images.input_updates, self.images.pass_updates]
    }
    #[cfg(test)]
    pub fn image_cache_bytes(&self) -> u64 {
        self.images.storage_bytes()
    }
    #[cfg(test)]
    pub fn placement_cache(&self, id: SourceTarget) -> Option<(wgpu::Texture, u64, u32)> {
        self.scale_sources.cache_info(id)
    }
    #[cfg(test)]
    pub fn placement_cache_at(&self, id: SourceTarget, level: u32) -> Option<(wgpu::Texture, u64, u32)> {
        self.scale_sources.cache_info_at(id, level)
    }
    pub fn scratch_bytes(&self) -> u64 {
        let mut allocations=std::collections::HashSet::new();
        let duplicate_metadata=self.object_spatial.key_allocations().chain(self.object_results.metadata_allocations())
            .chain(self.exact_object_results.metadata_allocations()).filter_map(|(id,bytes)|(!allocations.insert(id)).then_some(bytes)).sum::<u64>();
        self.pool.iter().map(PageSurface::storage_bytes).sum::<u64>()
            + (self.capacity * self.stride) as u64
            + self.effects.storage_bytes() + self.positions.storage_bytes()
            + self.placement.iter().map(pixel_transform::PixelTransform::storage_bytes).sum::<u64>() + 32
            + self.images.storage_bytes() + self.scale_sources.storage_bytes() + self.object_results.bytes() + self.exact_object_results.bytes() + self.object_spatial.storage_bytes() + self.scale_commands.as_ref().map_or(0, scale::Commands::storage_bytes) - duplicate_metadata
    }
    fn submit_source_uploads(
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        r.metrics.source_upload_submissions += 1;
        Self::submit_chunk(r, encoder, "after source upload")
    }
    pub(super) fn submit_chunk(
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        label: &'static str,
    ) -> Result<(), GpuRasterError> {
        let submission = Self::submit_commands(r, encoder, label);
        Self::wait_submission(r, submission)
    }
    pub(super) fn submit_commands(
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        label: &'static str,
    ) -> wgpu::SubmissionIndex {
        let next = crate::submission::CommandEncoder::new(&r.device,
            &wgpu::CommandEncoderDescriptor { label: Some(label) });
        let previous = std::mem::replace(encoder, next);
        r.uploads.finish(&previous);
        r.metrics.command_passes += previous.pass_count();
        previous.submit(&r.queue)
    }
    fn wait_submission(r: &WgpuRasterizer, submission: wgpu::SubmissionIndex) -> Result<(), GpuRasterError> {
        let _trace = crate::performance_trace::Span::new(c"capy.bounded_wait");
        // Native hosts run this work on their render owner. Wait for this exact
        // chunk before releasing/replacing resources charged to its ceiling.
        #[cfg(not(target_arch = "wasm32"))]
        if r.snapshot_worker {
            // A snapshot can share the live device. Do not hold wgpu's resource
            // lock while the background queue waits for a bounded upload batch.
            let (tx, rx) = mpsc::channel();
            r.queue.on_submitted_work_done(move || { let _ = tx.send(Ok(())); });
            crate::raster::wait_mapping(&r.device, &rx).map_err(GpuRasterError::WaitFailed)?;
        } else {
            r.device.poll(wgpu::PollType::Wait {
                submission_index: Some(submission), timeout: Some(READBACK_TIMEOUT),
            }).map_err(|e| GpuRasterError::MapFailed(e.to_string()))?;
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (r, submission); // Browser queue draining needs separate host qualification.
        Ok(())
    }
    pub fn initialize_source_paint(&mut self, r: &mut WgpuRasterizer, scene: SceneView<'_>, encoder: &mut crate::submission::CommandEncoder) -> Result<(), GpuRasterError> {
        self.jobs.clear(); self.source_jobs.clear();
        for &handle in scene.order() {
            let Some(target) = scene.source_target(handle) else { continue; };
            if scene.paint_source(handle).is_none_or(|paint| paint.base.is_none()) && r.native_backing(target).is_none() { continue; }
            let pages = r.paint_layers.iter().filter(|stored| stored.id == target)
                .flat_map(|stored| stored.pages.iter().filter(|page| page.primary_needs_clear)
                    .map(|page| (page.coordinate, page.primary.view.clone()))).collect();
            self.copy_source_pages(r, scene, target, pages)?;
        }
        self.encode_jobs(r, encoder)
    }
    pub fn initialize_source_preview(&mut self, r: &mut WgpuRasterizer, scene: SceneView<'_>, target: SourceTarget,
        damage: PixelRect, encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        self.jobs.clear(); self.source_jobs.clear();
        let stored = r.paint_layers.iter().find(|stored| stored.id == target);
        let pages = r.preview_pages.iter().filter(|page| !page_rect(page.coordinate).intersect(damage).is_empty()
                && !stored.is_some_and(|stored| stored.pages.iter().any(|own| own.coordinate == page.coordinate)))
            .map(|page| (page.coordinate, page.primary.view.clone())).collect();
        self.copy_source_pages(r, scene, target, pages)?;
        self.encode_jobs(r, encoder)
    }
    fn copy_source_pages(&mut self, r: &mut WgpuRasterizer, scene: SceneView<'_>, target: SourceTarget,
        pages: Vec<([u32; 2], wgpu::TextureView)>,
    ) -> Result<(), GpuRasterError> {
        for (coordinate, output) in pages {
            let Some(view) = self.source_tile(r, scene, target, coordinate)? else { continue; };
            let mut data = [0.; 32];
            data[..10].copy_from_slice(&[0., 0., 256., 256., 256., 256., 0., 0., 1., 1.]);
            self.jobs.push(Job::Draw { target: output, sources: [view, r.empty_view.clone(), r.empty_view.clone()], data, over: false, clip: None, source_target:None });
        }
        Ok(())
    }
    fn encode_decode(&mut self, r: &mut WgpuRasterizer, pending: Option<sources::PendingTile>, encoder: &mut crate::submission::CommandEncoder) -> Result<(), GpuRasterError> {
        if let Some(pending) = pending {
            // Each independent query owns its ordered uniform copy. Repeated
            // sampling between frames must not grow the record buffer.
            self.record_count = 0;
            self.jobs.push(Job::DecodedTile(std::sync::Arc::new(pending)));
            self.encode_jobs(r, encoder)?;
        }
        Ok(())
    }
    pub(crate) fn geometry_bytes(scene: Option<&Self>) -> u64 {
        let buffers = scene.map_or(0, |scene| scene.mesh_geometry.borrow().iter().map(|(_, geometry)| geometry.buffer_bytes()).max().unwrap_or(0));
        if buffers == 0 { 0 } else { buffers.saturating_mul(2) + u64::from(PAGE_SIZE + 2).pow(2) * 16 + 48 }
    }
    pub fn begin_frame(&mut self) {
        self.clear_material_pages();
        for pass in &mut self.placement { pass.begin_frame(); }
        self.source_bindings.begin_frame();
        self.compute_bindings.begin_frame();
        self.output_bindings.begin_frame();
        self.watercolor_outputs.begin_frame();
        self.mask_bindings.begin_frame();
        self.effect_outputs.begin_frame();
        self.record_count = 0;
        self.effect_passes = 0;
    }
    pub fn new(r: &WgpuRasterizer) -> Self {
        let device = &r.device;
        let Pipelines {
            uniforms,
            layout,
            pipeline,
            ..
        } = r.scene_pipelines.clone();
        let stride = device.limits().min_uniform_buffer_offset_alignment.max(144) as usize;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene uniform records"),
            size: stride as u64 * 128,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let binding = uniform_binding(device, &uniforms, &buffer);
        let effects = effects::Effects::new(r, &uniforms);
        use wgpu::util::DeviceExt;
        let coordinates = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("placed material neighborhood"),
            contents: target_bytes(&TargetGpu::new([PAGE_SIZE; 2], [PAGE_SIZE; 2], [PAGE_SIZE * 3; 2])),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        Self {
            valid: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            placement_display: false,
            object_display: false,
            object_source_damage: scale::Damage::EMPTY,
            object_live: false,
            object_query: false,
            positions: r.transforms.as_ref().map_or_else(|| paint_transform::mesh::Positions::new(device), paint_transform::mesh::Positions::sharing),
            position_key: None,
            mesh_geometry: Default::default(),
            scale_sources: Default::default(),
            object_results: object_cache::ObjectCache::live(),
            object_spatial: Default::default(),
            exact_object_results: Default::default(),
            scale_commands: None,
            native_reverse: false,
            placement: std::array::from_fn(|i| r.transforms.as_ref().map_or_else(
                || pixel_transform::PixelTransform::staged(device, i == 1).placement_pass(),
                |passes| passes.placement_pass(i == 1),
            )),
            material_coordinates: create_target_bind_group(device, &r.target_layout, &coordinates, &r.unclipped),
            material_pages: Default::default(),
            material_bounds: Default::default(),
            pool: Vec::new(),
            used: Vec::new(),
            jobs: Vec::new(),
            source_jobs: Vec::new(),
            source_bindings: Default::default(),
            compute_bindings: Default::default(),
            output_bindings: Default::default(),
            watercolor_outputs: Default::default(),
            mask_bindings: Default::default(),
            effect_outputs: Default::default(),
            layout,
            uniforms,
            reduction_binding: scale::Commands::record_binding(r, &buffer),
            buffer,
            binding,
            stride,
            capacity: 128,
            record_count: 0,
            upload: Vec::new(),
            pipeline,
            effects,
            effect_passes: 0,
            images: images::ImageStages::default(),
            image_window: None,
            image_evaluation_origin: [0; 2],
            stop_before: None,
            image_damage: None,
        }
    }
    pub(super) fn retire_changed_cells(&mut self,buffers:&[wgpu::Buffer]) {
        if buffers.is_empty() {return;}
        self.mask_bindings.forget(|(_,buffer)|buffers.contains(buffer));
    }
    fn forget_bindings(&mut self, retired: &[wgpu::TextureView]) {
        if retired.is_empty() { return; }
        self.source_bindings.forget(|(_,views)| views.iter().any(|view| retired.contains(view)));
        self.compute_bindings.forget(|views| views.iter().any(|view| retired.contains(view)));
        self.output_bindings.forget(|view| retired.contains(view));
        self.watercolor_outputs.forget(|view| retired.contains(view));
        self.mask_bindings.forget(|views| views.0.iter().any(|view| retired.contains(view)));
        self.effect_outputs.forget(|(_,views,output)|retired.contains(output) || views.iter().any(|view|retired.contains(view)));
    }
    fn alloc(&mut self, r: &WgpuRasterizer, color: wgpu::Color) -> usize {
        let id = self.reserve(r);
        self.jobs
            .push(Job::Clear(self.pool[id].view.clone(), color));
        id
    }
    fn reserve(&mut self, r: &WgpuRasterizer) -> usize {
        self.reserve_format(r, false)
    }
    fn reserve_format(&mut self, r: &WgpuRasterizer, scalar: bool) -> usize {
        let format = if scalar { r.device.scalar_format() } else { r.device.working_format() };
        let id = self
            .used
            .iter()
            .enumerate()
            .position(|(i, v)| !*v && self.pool[i].texture.format() == format)
            .unwrap_or(self.pool.len());
        if id == self.pool.len() {
            self.pool.push(if scalar { r.create_scalar_page_surface("scene reusable scalar") }
                else { r.create_page_surface("scene reusable tile") });
            self.used.push(false);
        }
        self.used[id] = true;
        id
    }
    fn free(&mut self, id: usize) {
        self.used[id] = false;
    }
    fn enqueue_source_decode(&mut self, pending: sources::PendingTile) {
        // Decoding writes only the separate source cache. Keep an adjacent
        // scratch clear beside its first draw so both share one render pass.
        let index = self.jobs.len() - usize::from(matches!(self.jobs.last(), Some(Job::Clear(..))));
        self.jobs.insert(index, Job::DecodedTile(std::sync::Arc::new(pending)));
    }
    fn source_tile(&mut self, r: &WgpuRasterizer, scene: SceneView<'_>, target: SourceTarget, coordinate: [u32; 2]) -> Result<Option<wgpu::TextureView>, GpuRasterError> {
        let _trace = crate::performance_trace::Span::new(c"capy.source_tile");
        let (tile, pending) = if let Some(blob) = r.native_color_tile(target, coordinate)? {
            let space = r.document_color().space;
            r.source_tiles.borrow_mut().plan_raster(r, &blob, space, space)?
        } else {
            let SourceTarget::Paint(paint) = target else { return Ok(None); };
            let Some(base) = scene.artwork().paint.get(paint).and_then(|paint| paint.base.as_ref()) else { return Ok(None); };
            let Some(read) = crate::source_access::plan_paint_base(r, base, coordinate)? else { return Ok(None); };
            for pending in read.pending { self.enqueue_source_decode(pending); }
            if let Some(assembly) = read.assembly { self.jobs.push(Job::PaintBase(Arc::new(assembly))); }
            return Ok(Some(read.tile.view));
        };
        if let Some(pending) = pending { self.enqueue_source_decode(pending); }
        Ok(Some(tile.view))
    }

    /// Consume a bounded group before gathering the next one: these textures
    /// belong to the fixed, queue-ordered source cache.
    pub fn layer_tile_for_query(&mut self, r: &mut WgpuRasterizer, scene: SceneView<'_>, target: SourceTarget,
        coordinate: [u32; 2], encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<crate::source_access::RawTile, GpuRasterError> {
        debug_assert!(self.jobs.is_empty());
        self.used.fill(false); self.begin_frame();
        let extent = scene.target_extent(target);
        let page = match target {
            SourceTarget::Coverage(coverage) => {
                let owner = scene.source_owner(target).ok_or(GpuRasterError::MissingPaintLayer(target))?;
                let (use_, source) = scene.mask(owner).ok_or(GpuRasterError::MissingPaintLayer(target))?;
                debug_assert_eq!(use_.source, coverage);
                self.mask_at(r, use_, source, scene.target_offset(target), coordinate)
            }
            _ => self.placed_raw_plane(r, scene, target, Default::default(), scene.target_offset(target), coordinate, layer_core::raster::RasterPlane::Color, PixelRect::full(extent))?,
        };
        self.encode_jobs(r, encoder)?;
        Ok(crate::source_access::RawTile { texture: self.pool[page].texture.clone(), view: self.pool[page].view.clone() })
    }

    pub(super) fn paint_base_tile_for_query(&mut self, r: &mut WgpuRasterizer, base: &layer_core::authored::PaintBase,
        coordinate: [u32; 2], encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Option<crate::source_access::RawTile>, GpuRasterError> {
        let Some(read) = crate::source_access::plan_paint_base(r, base, coordinate)? else { return Ok(None); };
        for pending in read.pending { self.encode_decode(r, Some(pending), encoder)?; }
        if let Some(assembly) = read.assembly { crate::source_access::encode_paint_base(&assembly, encoder); }
        Ok(Some(read.tile))
    }
    pub fn prepare_native_transfer(&mut self, r: &WgpuRasterizer, space: layer_core::color::RgbSpace) -> Result<crate::native_tiles::NativeTransfer, GpuRasterError> {
        r.source_tiles.borrow_mut().prepare_transfer(&r.device, space)
    }
    pub fn restore_native_tiles(
        &mut self,
        r: &mut WgpuRasterizer,
        requests: &[crate::native_tiles::NativeTileRestore<'_>],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        debug_assert!(self.jobs.is_empty());
        for request in requests {
            sources::validate_raster(request.blob, request.space)?;
        }
        for request in requests {
            let (tile, pending) = r.source_tiles.borrow_mut().plan_raster(r, request.blob, request.space, request.destination)?;
            self.encode_decode(r, pending, encoder)?;
            // Consume this view before a later request can reuse the slot.
            encoder.copy_texture_to_texture(tile.texture.as_image_copy(), request.working.as_image_copy(), tile.texture.size());
        }
        Ok(())
    }
    pub fn restore_native_scalars(
        &mut self,
        r: &mut WgpuRasterizer,
        requests: &[crate::native_tiles::scalar::NativeScalarRestore<'_>],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        for request in requests {
            if r.source_tiles.borrow().uploads_full() {
                Self::submit_source_uploads(r, encoder)?;
            }
            let bytes = crate::native_tiles::scalar::restore_upload(r, request, encoder)?;
            let in_flight = r.source_tiles.borrow().charge_upload(encoder, bytes);
            r.metrics.source_upload_peak_bytes = r.metrics.source_upload_peak_bytes.max(in_flight);
        }
        Ok(())
    }
    pub fn raster_tile_for_query(&mut self, r: &mut WgpuRasterizer, blob: &std::sync::Arc<layer_core::raster::TileBlob>, space: layer_core::color::RgbSpace, encoder: &mut crate::submission::CommandEncoder) -> Result<crate::source_access::RawTile, GpuRasterError> {
        let (tile, pending) = r.source_tiles.borrow_mut().plan_raster(r, blob, space, r.document_color().space)?;
        self.encode_decode(r, pending, encoder)?;
        Ok(tile)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint_page(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        handle: OccurrenceHandle,
        stored: Option<&PaintLayer>,
        c: [u32; 2],
        out: usize,
        rect: [f32; 4],
        convert: Convert,
    ) -> Result<(), GpuRasterError> {
        let target = packet.scene.source_target(handle).unwrap();
        let preview = r.preview_layer_id == Some(target)
            && !r.preview_damage.intersect(page_rect(c)).is_empty();
        let wet_nearby = stored.is_some_and(|stored| r.watercolor_style(target, packet.dab_batches).is_some()
            && stored
                .watercolor_wetness_pages
                .iter()
                .chain(
                    r.preview_watercolor_wetness_pages
                        .iter()
                        .filter(|_| preview),
                )
                .any(|p| {
                    p.coordinate[0].abs_diff(c[0]) <= 1
                        && p.coordinate[1].abs_diff(c[1]) <= 1
                }));
        if wet_nearby {
            let binding = self.watercolor_binding(r, packet.scene, target, stored.unwrap(), c, preview)?;
            // Aligned pages already cover the layer tile. Only a translated
            // or converted page needs an intermediate and placement.
            let page = if rect == [0., 0., 256., 256.] && convert == Convert::None {
                out
            } else {
                self.alloc(r, wgpu::Color::TRANSPARENT)
            };
            self.jobs.push(Job::Watercolor {
                coordinates: (r.target_bind_group.clone(), r.layer_target_offset(target, c)),
                target: self.pool[page].view.clone(),
                binding,
                record: *r.layer_style_records.get(&handle).ok_or(GpuRasterError::MissingPaintLayer(target))?,
            });
            if page != out {
                self.draw(
                    r,
                    out,
                    self.pool[page].view.clone(),
                    None,
                    rect,
                    [1., 1., 0., 0.],
                    true,
                    convert,
                );
                self.free(page);
            }
        } else {
            let [base, flow] = self.color_inputs(r, packet.scene, target, stored, c, preview)?.map(|input| input.map(|i| i.view));
            if preview && r.preview_contribution && let Some(flow)=flow.as_ref() {
                self.draw(r,out,flow.clone(),base,rect,[19.,1.,f32::from(packet.blend_space==layer_core::BlendSpace::Perceptual),0.],true,convert);
                return Ok(());
            }
            match (base, flow) {
                (Some(base), Some(flow)) if convert != Convert::None => {
                    self.draw(r, out, flow, Some(base), rect, [14., 1., 0., 0.], true, convert);
                }
                (base, flow) => {
                    for view in base.into_iter().chain(flow) {
                        self.draw(r, out, view, None, rect, [1., 1., 0., 0.], true, convert);
                    }
                }
            }
        }
        Ok(())
    }
    fn color_inputs(&mut self, r: &WgpuRasterizer, scene: SceneView<'_>, target: SourceTarget, stored: Option<&PaintLayer>, c: [u32; 2], preview: bool) -> Result<[Option<ColorInput>; 2], GpuRasterError> {
        let persistent = stored.and_then(|s| s.pages.iter().find(|p| p.coordinate == c));
        let predicted = preview.then(|| r.preview_page(c)).flatten();
        let base = if let Some(p) = predicted.filter(|_| r.preview_requires_base).or(persistent) {
            Some(ColorInput { view: p.active().view.clone(), lease: None })
        } else {
            self.source_tile(r, scene, target, c)?.map(|view| {
                let lease = r.source_tiles.borrow().lease(&view);
                ColorInput { view, lease }
            })
        };
        let overlay = predicted.filter(|_| !r.preview_requires_base)
            .map(|p| ColorInput { view: p.active().view.clone(), lease: None });
        Ok([base, overlay])
    }
    fn watercolor_binding(
        &mut self,
        r: &WgpuRasterizer,
        scene: SceneView<'_>,
        target: SourceTarget,
        stored: &PaintLayer,
        coordinate: [u32; 2],
        preview: bool,
    ) -> Result<wgpu::BindGroup, GpuRasterError> {
        let mut colors = Vec::with_capacity(5);
        for [dx, dy] in [[0, 0], [-1, 0], [1, 0], [0, -1], [0, 1]] {
            let [x, y] = [coordinate[0] as i32 + dx, coordinate[1] as i32 + dy];
            let mut view = None;
            if x >= 0 && y >= 0 {
                let neighbor = [x as u32, y as u32];
                let predicted = preview
                    .then(|| {
                        r.preview_page(neighbor)
                    })
                    .flatten();
                view = predicted
                    .or_else(|| stored.pages.iter().find(|p| p.coordinate == neighbor))
                    .map(|p| p.active().view.clone());
                if view.is_none() {
                    view = self.source_tile(r, scene, target, neighbor)?;
                }
            }
            colors.push(view.unwrap_or_else(|| r.empty_view.clone()));
        }
        // Source uploads precede the immediately appended watercolor job. No
        // later neighborhood may be planned before that job has its bindings.
        Ok(r.watercolor_binding_with_colors(
            stored,
            coordinate,
            preview,
            &colors.iter().collect::<Vec<_>>(),
        ))
    }
    fn effect(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        indices: &[OccurrenceHandle],
        tile: [u32; 2],
        input: usize,
    ) -> Result<usize, GpuRasterError> {
        let handle = indices[0];
        if packet.scene.effect(handle).unwrap().program.image_boundary() {
            let view = self.images.output(handle).ok_or_else(|| GpuRasterError::Effect("Missing image effect stage".into()))?;
            let out = self.image_tile(r, view, self.images.doc_bounds, tile);
            self.free(input);
            return Ok(out);
        }
        let prepared = self.effects.prepare(r, packet.scene, indices, effects::Execution::Fused, packet.time_seconds, 0, packet.blend_space)?;
        let mask = if indices.len() == 1 && !direct_effect_mask(packet.scene, handle) {
            packet.scene.mask(handle).filter(|(use_, _)| use_.enabled).map(|(use_, source)| {
                self.mask_at(r, use_, source, packet.scene.target_offset(SourceTarget::Coverage(use_.source)), tile)
            })
        } else { None };
        let out = self.reserve(r);
        let mut data = [0.; 32];
        data[..4].copy_from_slice(&[0., 0., 256., 256.]);
        data[4..6].copy_from_slice(&[256., 256.]);
        data[11] = f32::from(mask.is_some());
        let mut masks = Box::new(std::array::from_fn(|_| r.empty_view.clone()));
        let mut present = 0u32;
        let mut inverted = 0u32;
        if mask.is_none() {
            for (i, handle) in indices.iter().enumerate().take(effects::MASK_SLOTS) {
                if let Some((m, _)) = packet.scene.mask(*handle).filter(|(m, _)| m.enabled)
                    && let Some(page) = r.layer_masks.pages.get(&(SourceTarget::Coverage(m.source), tile))
                {
                    masks[i] = page.view.clone();
                    present |= 1 << i;
                    if m.inverted {
                        inverted |= 1 << i;
                    }
                }
            }
        }
        data[6] = present as f32;
        data[7] = inverted as f32;
        data[12..16].copy_from_slice(&[
            (tile[0] * PAGE_SIZE) as f32,
            (tile[1] * PAGE_SIZE) as f32,
            packet.document_extent[0] as f32,
            packet.document_extent[1] as f32,
        ]);
        let mut source_target=None;
        let mut sources = [
            self.pool[input].view.clone(),
            mask.map_or_else(|| r.empty_view.clone(), |m| self.pool[m].view.clone()),
            r.empty_view.clone(),
        ];
        // Lower a source-over operation followed by an adjustment into one
        // shader invocation. No intermediate color tile or pass is necessary.
        // This is program-independent; masks and nonlocal operations delimit it.
        if mask.is_none() && self.jobs.len() >= 2 {
            let n = self.jobs.len();
            if let (
                Job::Clear(clear, bg),
                Job::Draw {
                    target,
                    sources: paint,
                    data: paint_data,
                    over,
                    source_target: ownership,
                    ..
                },
            ) = (&self.jobs[n - 2], &self.jobs[n - 1])
                && *clear == self.pool[input].view
                && target == clear
                && paint_data[..4] == [0., 0., 256., 256.]
                && paint_data[31] == Convert::layers(packet).code()
                && ((*over && (paint_data[8] == 7. || paint_data[8] == 1.))
                    || (!*over && (paint_data[8] == 13. || paint_data[8]==19.)))
            {
                sources = paint.clone();
                source_target = *ownership;
                data[16..20].copy_from_slice(&[
                    paint_data[9],
                    if paint_data[8] == 1. || paint_data[8]==19. {
                        1.
                    } else {
                        paint_data[10]
                    },
                    if paint_data[8]==19. {0.} else {paint_data[11]},
                    if paint_data[8]==19. {3.} else {1.},
                ]);
                data[20..24].copy_from_slice(&[bg.r as f32, bg.g as f32, bg.b as f32, bg.a as f32]);
                self.jobs.truncate(n - 2);
            }
        }
        self.jobs.push(Job::Effect {
            target: self.pool[out].view.clone(),
            sources,
            data,
            prepared,
            masks, source_target, changed_cells:None,
        });
        self.free(input);
        if let Some(m) = mask {
            self.free(m);
        }
        Ok(out)
    }
    #[allow(clippy::too_many_arguments)] // Explicit tile draw operands.
    fn draw(
        &mut self,
        r: &WgpuRasterizer,
        target: usize,
        source: wgpu::TextureView,
        back: Option<wgpu::TextureView>,
        rect: [f32; 4],
        options: [f32; 4],
        mut over: bool,
        convert: Convert,
    ) {
        let mut data = [0.; 32];
        let mut sources = [source, back.unwrap_or_else(|| r.empty_view.clone()), r.empty_view.clone()];
        data[..4].copy_from_slice(&rect);
        data[4..8].copy_from_slice(&[256., 256., 0., 0.]);
        data[8..12].copy_from_slice(&options);
        data[31] = convert.code();
        if over && options[0]==19. && matches!(self.jobs.last(),Some(Job::Clear(clear,color)) if *clear==self.pool[target].view && *color==wgpu::Color::TRANSPARENT) {over=false;}
        // A normal draw over a constant clear supplies its own backdrop. Lower
        // this once when creating the job, for both tiled and direct composition.
        if over && matches!(options[0] as u32,1 | 7)
            && let Some(Job::Clear(clear, color)) = self.jobs.last()
            && *clear == self.pool[target].view
        {
            if options[0]==1. {data[10]=1.;data[11]=0.;}
            data[8] = 13.;
            data[16..20].copy_from_slice(&[
                color.r as f32, color.g as f32, color.b as f32, color.a as f32,
            ]);
            over = false;
        }
        // Two aligned normal layers, including a separate Flow preview, can
        // resolve over their constant backdrop in one destination write. This
        // preserves layer opacity after preview composition and needs neither
        // a scratch color tile nor an attachment destination read.
        if over && rect == [0., 0., 256., 256.]
            && (options[0] == 14. || (options[0] == 7. && options[3] < 2.))
            && let Some(Job::Draw { target: prior_target, sources: prior_sources,
                data: prior, over: false, clip: None, .. }) = self.jobs.last()
            && *prior_target == self.pool[target].view
            && prior[..4] == rect && prior[8] == 13. && prior[11] < 2. && prior[31] == data[31]
        {
            if options[0] == 7. {
                data[9] *= options[2];
                sources[1] = r.empty_view.clone();
            }
            sources[2] = prior_sources[0].clone();
            data[8] = 15.;
            data[16] = prior[9] * prior[10];
            data[20..24].copy_from_slice(&prior[16..20]);
            over = false;
            self.jobs.pop();
        }
        self.jobs.push(Job::Draw {
            target: self.pool[target].view.clone(),
            sources,
            data,
            over,
            clip: None, source_target:None,
        });
    }
    #[allow(clippy::too_many_arguments)] // Explicit composite operands.
    fn combine(
        &mut self,
        r: &WgpuRasterizer,
        front: usize,
        back: usize,
        opacity: f32,
        blend: layer_core::LayerBlend,
        clip: bool,
        space: layer_core::BlendSpace,
    ) -> usize {
        let n = self.jobs.len();
        // Watercolor already outputs premultiplied source-over. A complete,
        // unmasked layer at full opacity can draw straight onto its backdrop.
        if !clip && opacity == 1. && blend == layer_core::LayerBlend::Normal && n >= 2 {
            let clear = matches!(&self.jobs[n - 2], Job::Clear(view, color)
                if *view == self.pool[front].view && *color == wgpu::Color::TRANSPARENT);
            if clear
                && let Job::Watercolor { target, .. } = &mut self.jobs[n - 1]
                && *target == self.pool[front].view
            {
                *target = self.pool[back].view.clone();
                self.jobs.remove(n - 2);
                self.free(front);
                return back;
            }
        }
        // An isolated clipping stack over a constant backdrop needs no color
        // intermediate after its last adjustment. Fold that final composite.
        if !clip && n >= 2 {
            let bg = if let Job::Clear(view, color) = &self.jobs[n - 2] {
                (*view == self.pool[back].view).then_some(*color)
            } else {
                None
            };
            if let Some(bg) = bg
                && let Job::Effect { target, data, .. } = &mut self.jobs[n - 1]
                && *target == self.pool[front].view
                && data[8] == 0.
            {
                data[8] = 1.;
                data[9] = opacity;
                data[10] = crate::blend_code(blend, &r.device, space) as f32;
                if data[19] > 0.5 {
                    data[19] = 2.;
                }
                data[20..24].copy_from_slice(&[bg.r as f32, bg.g as f32, bg.b as f32, bg.a as f32]);
                self.jobs.remove(n - 2);
                self.free(back);
                return front;
            }
        }
        if !clip && blend==layer_core::LayerBlend::Normal
            && let Some(Job::Effect {target,sources,data,prepared,..})=self.jobs.last_mut()
            && *target==self.pool[front].view && prepared.pointwise && prepared.original_independent
            && data[8]==0. && data[11]<=0.5 && data[6]==0. && data[18]<2. && data[19]<=2.5
        {
            sources[1]=self.pool[back].view.clone();
            data[8]=2.;data[9]=opacity;data[10]=crate::blend_code(blend,&r.device,space) as f32;
            self.free(back);
            return front;
        }
        let out = self.alloc(r, wgpu::Color::TRANSPARENT);
        self.draw(
            r,
            out,
            self.pool[front].view.clone(),
            Some(self.pool[back].view.clone()),
            [0., 0., 256., 256.],
            [4., opacity, crate::blend_code(blend, &r.device, space) as f32, f32::from(clip)],
            false,
            Convert::None,
        );
        self.free(front);
        self.free(back);
        out
    }
    /// `tile` drawn through `convert` into a new tile, or `tile` itself when
    /// nothing converts.
    pub(super) fn converted(&mut self, r: &WgpuRasterizer, tile: usize, convert: Convert) -> usize {
        if convert == Convert::None {
            return tile;
        }
        if let Some(Job::Draw { target, data, over: false, clip: None, .. }) = self.jobs.last_mut()
            && *target == self.pool[tile].view && data[..4] == [0., 0., 256., 256.]
            && matches!(data[8] as u32, 13 | 15 | 19) && data[7] == 0.
        {
            if matches!(data[8] as u32,13|19) && data[31]==0. && data[16..20]==[0.;4]
                && matches!(convert,Convert::Encode | Convert::Decode) {
                data[31]=convert.code();
            } else {data[7] = convert.code();}
            return tile;
        }
        let out = self.reserve(r);
        self.draw(r, out, self.pool[tile].view.clone(), None, [0., 0., 256., 256.], [1., 1., 0., 0.], false, convert);
        self.free(tile);
        out
    }
    fn mask_tile(
        &mut self,
        r: &WgpuRasterizer,
        mask: &MaskUse,
        source: &CoverageSource,
        offset: [i64; 2],
        tile: [u32; 2],
    ) -> usize {
        self.mask_tile_input(r, mask, source, offset, tile, None)
    }

    fn mask_tile_input(
        &mut self, r: &WgpuRasterizer, mask: &MaskUse, source: &CoverageSource,
        offset: [i64; 2], tile: [u32; 2], command: Option<layer_masks::CommandCoverage>,
    ) -> usize {
        let default = if mask.inverted {
            1. - source.default_coverage
        } else {
            source.default_coverage
        } as f64;
        let out = self.alloc(
            r,
            wgpu::Color {
                r: default,
                g: default,
                b: default,
                a: default,
            },
        );
        let command_pages = command.into_iter().flat_map(|key| r.layer_masks.command_pages.iter()
            .filter_map(move |((id, c), page)| (*id == key).then_some((*c, page))));
        let authored_pages = command.is_none().then_some(SourceTarget::Coverage(mask.source)).into_iter()
            .flat_map(|key| r.layer_masks.pages.iter().filter_map(move |((id, c), page)| (*id == key).then_some((*c, page))));
        for (c, page) in command_pages.chain(authored_pages) {
            let rect = local_rect(c, offset, tile);
            if !intersects(rect) {
                continue;
            }
            self.draw(
                r,
                out,
                page.view.clone(),
                None,
                rect,
                [2., 1., f32::from(mask.inverted), 0.],
                false,
                Convert::None,
            );
        }
        out
    }
    fn layer(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        index: OccurrenceHandle,
        tile: [u32; 2],
    ) -> Result<usize, GpuRasterError> {
        let layer = packet.scene.occurrence(index).unwrap();
        let out = if layer.kind() == LayerKind::Group {
            self.group(r, packet, Some(index), tile)?
        } else if let Some(color) = packet.scene.effect(index).and_then(|effect| effect.constant_color())
            .filter(|_| (self.image_window.is_none() && packet.scene.evaluation_offset64()==[0.;2]) || constant_frame_tile(packet.scene,tile,self.image_window)) {
            self.alloc(r, composite_color(r, packet, color.linear_in(r.device.working_space()).map_err(GpuRasterError::Color)?))
        } else if layer.kind() == LayerKind::Effect {
            let input = self.alloc(r, wgpu::Color::TRANSPARENT);
            // Generator coverage is applied below with ordinary layer masks.
            self.effect(r, packet, &[index], tile, input)?
        } else if packet.scene.object_layer(index).is_some() {
            self.object_tile(r, packet, index, tile)?
        } else {
            let out = self.paint_tile(r, packet, index, tile)?;
            self.converted(r, out, Convert::layers(packet))
        };
        if let Some((mask, source)) = packet.scene.mask(index).filter(|(mask, _)| mask.enabled) {
            let m = self.mask_at(r, mask, source, packet.scene.target_offset(SourceTarget::Coverage(mask.source)), tile);
            let result = self.alloc(r, wgpu::Color::TRANSPARENT);
            self.draw(
                r,
                result,
                self.pool[out].view.clone(),
                Some(self.pool[m].view.clone()),
                [0., 0., 256., 256.],
                [3., 1., 0., 0.],
                false,
                Convert::None,
            );
            self.free(out);
            self.free(m);
            Ok(result)
        } else {
            Ok(out)
        }
    }
    fn paint_tile(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, index: OccurrenceHandle, tile: [u32; 2]) -> Result<usize, GpuRasterError> {
        let target = packet.scene.source_target(index).unwrap_or_default();
        if world_offset(packet.scene, index, false) != [0; 2]
                && r.watercolor_style(target, packet.dab_batches).is_some() {
            return self.placed_material_tile(r, packet, index, packet.scene.target_offset(target), tile);
        }
        let out = self.alloc(r, wgpu::Color::TRANSPARENT);
        let offset = world_offset(packet.scene, index, false);
        let stored = r.paint_layers.iter().find(|l| l.id == target);
        if stored.is_some() || packet.scene.paint_source(index).is_some_and(|paint| paint.base.is_some()) {
            // A translated output tile intersects at most four native
            // source tiles. Watercolor samples its halo from their bindings;
            // never scan/expand every page in the layer for every output tile.
            let origin: [i64; 2] = std::array::from_fn(|i| i64::from(tile[i] * PAGE_SIZE) - offset[i]);
            let region = DocRect { min: origin, max: origin.map(|v| v + i64::from(PAGE_SIZE)) }.in_frame(packet.scene.local_extent(index));
            for c in page_coordinates(region) {
                let rect = local_rect(c, offset, tile);
                if !intersects(rect) {
                    continue;
                }
                self.paint_page(r, packet, index, stored, c, out, rect, Convert::None)?;
            }
        }
        if offset==[0; 2]
            && r.watercolor_style(target,packet.dab_batches).is_none()
            && let Some(Job::Draw {target:output,source_target,..})=self.jobs.last_mut()
            && *output==self.pool[out].view {
                *source_target=Some(target);
            }
        Ok(out)
    }
    fn group(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        parent: Option<OccurrenceHandle>,
        tile: [u32; 2],
    ) -> Result<usize, GpuRasterError> {
        stack::tile(self, r, packet, parent, tile)
    }

    // A normal paint tile with an aligned scalar mask needs one source-over
    // draw, not separate color, mask, multiplication and blend scratch passes.
    fn draw_normal_layer(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        index: OccurrenceHandle,
        tile: [u32; 2],
        output: usize,
    ) -> Result<bool, GpuRasterError> {
        let layer = packet.scene.occurrence(index).unwrap();
        if layer.blend == layer_core::LayerBlend::Normal
            && !layer.mask.as_ref().is_some_and(|mask| mask.enabled)
            && let Some(color) = packet.scene.effect(index).and_then(|effect| effect.constant_color())
            && ((self.image_window.is_none() && packet.scene.evaluation_offset64()==[0.;2]) || constant_frame_tile(packet.scene,tile,self.image_window))
            && let Some(Job::Clear(view, back)) = self.jobs.last_mut()
            && *view == self.pool[output].view
        {
            let mut color = color.linear_in(r.device.working_space()).map_err(GpuRasterError::Color)?;
            color[3] *= layer.opacity;
            let front = composite_color(r, packet, color);
            let weight = 1. - front.a;
            *back = wgpu::Color { r: front.r + back.r * weight, g: front.g + back.g * weight,
                b: front.b + back.b * weight, a: front.a + back.a * weight };
            return Ok(true);
        }
        let target = packet.scene.source_target(index).unwrap_or_default();
        if layer.kind() != LayerKind::Paint
            || layer.blend != layer_core::LayerBlend::Normal
        {
            return Ok(false);
        }
        let mask = packet.scene.mask(index).filter(|(mask, _)| mask.enabled);
        if world_offset(packet.scene, index, false) != [0; 2] {
            return Ok(false);
        }
        if mask.is_some_and(|(mask, _)| packet.scene.target_offset(SourceTarget::Coverage(mask.source)) != [0; 2])
        {
            return Ok(false);
        }
        let stored = r.paint_layers.iter().find(|l| l.id == target);
        if r.watercolor_style(target, packet.dab_batches).is_some() {
            return Ok(false);
        }
        // Destination-reading previews already contain the complete layer
        // tile. Composite them directly just like persistent paint, instead
        // of allocating a scratch layer and blending it in another pass.
        let predicted = (r.preview_layer_id == Some(target))
            .then(|| r.preview_page(tile))
            .flatten();
        if let Some(preview) = predicted.filter(|_| !r.preview_requires_base) {
            // A Flow preview is a separate transparent paint contribution.
            // Resolve it over persistent paint before applying layer opacity;
            // drawing the two contributions independently changes the result.
            // An enabled mask needs a third source and retains the general path.
            if mask.is_some() {
                return Ok(false);
            }
            let base = if let Some(page) = stored.and_then(|stored| stored.pages.iter().find(|p| p.coordinate == tile)) {
                Some(page.active().view.clone())
            } else {
                self.source_tile(r, packet.scene, target, tile)?
            };
            self.draw(
                r,
                output,
                preview.active().view.clone(),
                base,
                [0., 0., 256., 256.],
                [if r.preview_contribution {19.} else {14.}, layer.opacity,f32::from(r.preview_contribution && packet.blend_space==layer_core::BlendSpace::Perceptual), 0.],
                true,
                Convert::layers(packet),
            );
            if let Some(Job::Draw {source_target,..})=self.jobs.last_mut() {*source_target=Some(target);}
            return Ok(true);
        }
        let view = if let Some(page) = predicted.or_else(|| stored.and_then(|stored| stored.pages.iter().find(|p| p.coordinate == tile))) {
            page.active().view.clone()
        } else if let Some(view) = self.source_tile(r, packet.scene, target, tile)? {
            view
        } else {
            return Ok(true);
        };
        let default = mask.map_or(1., |(m, source)| {
            if m.inverted {
                1. - source.default_coverage
            } else {
                source.default_coverage
            }
        });
        let source = mask.and_then(|(mask, _)| r.layer_masks.pages.get(&(SourceTarget::Coverage(mask.source), tile)));
        self.draw(
            r,
            output,
            view,
            source.map(|p| p.view.clone()),
            [0., 0., 256., 256.],
            [
                7.,
                layer.opacity,
                default,
                source.map_or(0., |_| 2. + f32::from(mask.unwrap().0.inverted)),
            ],
            true,
            Convert::layers(packet),
        );
        if mask.is_none() && let Some(Job::Draw {source_target,..})=self.jobs.last_mut() {*source_target=Some(target);}
        Ok(true)
    }
    /// Run a pending paint operation over `damage`, the batch's pages. Masks
    /// and erases that cover the whole layer settle watercolor and wet
    /// material first, so hidden pigment cannot bring erased content back.
    pub fn apply_operation(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        target: SourceTarget,
        operation_index: usize,
        damage: PixelRect,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        use layer_core::RasterOperationKind;
        self.jobs.clear();self.source_jobs.clear();
        self.clear_material_pages();
        self.used.fill(false);
        let handle = packet.scene.source_owner(target).ok_or(GpuRasterError::MissingPaintLayer(target))?;
        let op = &packet.scene.operations(target).ok_or(GpuRasterError::MissingPaintLayer(target))?[operation_index];
        let extent = packet.scene.target_extent(target);
        if matches!(op.kind, RasterOperationKind::ColorMode(_) | RasterOperationKind::Erase { alpha_locked: true }) {
            return Ok(());
        }
        if let RasterOperationKind::Bake { scene, scope, offset } = &op.kind {
            return self.bake(r, packet, target, operation_index as u32, scene, scope, *offset, damage, &op.coverage, None, encoder);
        }
        if let RasterOperationKind::FrequencyDetail { scene, scope, offset, low } = &op.kind {
            return self.bake(r, packet, target, operation_index as u32, scene, scope, *offset, damage, &op.coverage, Some(SourceTarget::Paint(*low)), encoder);
        }
        if target.is_coverage() {
            let pages: Vec<_> = r.layer_masks.pages.iter().filter(|((id, c), _)| *id == target && !damage.page_local(*c).is_empty())
                .map(|((_, c), page)| (*c, page.clone())).collect();
            for (coordinate, page) in pages {
                let mask = self.command_mask_at(r, &op.coverage, coordinate, (target, operation_index as u32));
                let out = self.reserve_format(r, true);
                self.draw(r, out, page.view.clone(), Some(self.pool[mask].view.clone()),
                    [0., 0., 256., 256.], [22., 1., 0., 0.], false, Convert::None);
                self.copy_window_tile(out, &Image { texture: page.texture, view: page.view,
                    plan: display_mips::Plan::window(extent, 0, page_rect(coordinate).intersect(PixelRect::full(extent))) }, coordinate);
                self.free(mask);
            }
            return self.encode_jobs(r, encoder);
        }
        let Some(stored) = r.paint_layers.iter().find(|l| l.id == target) else {
            return Ok(());
        };
        let settles = match op.kind {
            RasterOperationKind::ApplyMask => true,
            RasterOperationKind::Erase { .. } => damage == PixelRect::full(extent),
            _ => false,
        };
        let pages: Vec<_> = stored
            .pages
            .iter()
            .filter(|p| !damage.page_local(p.coordinate).is_empty())
            .map(|p| {
                (
                    p.coordinate,
                    p.active().view.clone(),
                    p.active().texture.clone(),
                )
            })
            .collect();
        let watercolor = stored.watercolor.is_some() && settles;
        let erase = f32::from(matches!(op.kind, RasterOperationKind::Erase { .. }));
        let gradient = if let RasterOperationKind::Gradient {gradient,..}=&op.kind {Some(crate::gradient::texture(r,gradient)?)} else {None};
        for (c, source, destination) in pages {
            let mask = self.command_mask_at(r, &op.coverage, c, (target, operation_index as u32));
            let out = self.alloc(r, wgpu::Color::TRANSPARENT);
            match op.kind {
                RasterOperationKind::ColorMode(_) | RasterOperationKind::Transform(_) | RasterOperationKind::Bake { .. } | RasterOperationKind::FrequencyDetail { .. }
                    | RasterOperationKind::Coverage => {
                    unreachable!("transforms, bakes and mask coverage run before page operations")
                }
                RasterOperationKind::ApplyMask | RasterOperationKind::Erase { .. } => {
                    let mut resolved = None;
                    if watercolor {
                        let binding = self.watercolor_binding(r, packet.scene, target, stored, c, false)?;
                        let p = self.alloc(r, wgpu::Color::TRANSPARENT);
                        self.jobs.push(Job::Watercolor {
                            coordinates: (r.target_bind_group.clone(), r.layer_target_offset(target, c)),
                            target: self.pool[p].view.clone(),
                            binding,
                            record: *r.layer_style_records.get(&handle).ok_or(GpuRasterError::MissingPaintLayer(target))?,
                        });
                        resolved = Some(p);
                    }
                    self.draw(
                        r,
                        out,
                        resolved.map_or(source, |p| self.pool[p].view.clone()),
                        Some(self.pool[mask].view.clone()),
                        [0., 0., 256., 256.],
                        [3., 1., erase, 0.],
                        false,
                        Convert::None,
                    );
                    if let Some(p) = resolved {
                        self.free(p);
                    }
                }
                RasterOperationKind::Fill { .. }
                | RasterOperationKind::Gradient { .. }
                | RasterOperationKind::Figure(_) => {
                    let (colors, endpoints, options) = match op.kind {
                        RasterOperationKind::Fill {
                            color,
                            alpha_locked,
                        } => ([color; 2], [0.0; 4], [6., 1., 0., f32::from(alpha_locked)]),
                        RasterOperationKind::Gradient {start,end,shape,reverse,opacity,alpha_locked,..} => (
                            [[opacity,0.,0.,0.],[0.;4]],
                            [start.x,start.y,end.x,end.y],
                            [18.,crate::gradient::quantum(r.document_color.depth),shape as u8 as f32+4.*f32::from(reverse),f32::from(alpha_locked)],
                        ),
                        RasterOperationKind::Figure(ref f) => (
                            f.colors,
                            [f.start.x, f.start.y, f.end.x, f.end.y],
                            [
                                11.,
                                f.shape as u32 as f32
                                    + 3. * f.paint as u32 as f32
                                    + 16. * f32::from(f.erase),
                                f.width,
                                f32::from(f.alpha_locked),
                            ],
                        ),
                        _ => unreachable!(),
                    };
                    self.draw(
                        r,
                        out,
                        self.pool[mask].view.clone(),
                        Some(source),
                        [0., 0., 256., 256.],
                        options,
                        false,
                        Convert::None,
                    );
                    if let Some(Job::Draw { data, sources, .. }) = self.jobs.last_mut() {
                        if let Some(gradient)=&gradient {sources[2]=gradient.clone();}
                        data[6..8].copy_from_slice(&c.map(|v| (v * PAGE_SIZE) as f32));
                        data[12..16].copy_from_slice(&colors[0]);
                        data[16..20].copy_from_slice(&colors[1]);
                        data[20..24].copy_from_slice(&endpoints);
                        data[24..30].copy_from_slice(&op.placement.inverse().ok_or(GpuRasterError::InvalidTransform("Invalid paint operation placement"))?.0);
                    }
                }
            }
            let written = if matches!(op.kind, RasterOperationKind::Fill { .. } | RasterOperationKind::Gradient { .. } | RasterOperationKind::Figure(_)) {
                PixelRect::full(extent).page_local(c)
            } else {
                PixelRect::full([PAGE_SIZE; 2])
            };
            if !written.is_empty() {
                let origin = [written.min_x(), written.min_y()];
                self.jobs.push(Job::Copy {
                    source: self.pool[out].texture.clone(),
                    source_origin: origin,
                    destination,
                    origin,
                    width: written.width(),
                    height: written.height(),
                });
            }
            self.free(out);
            self.free(mask);
        }
        self.encode_jobs(r, encoder)?;
        if settles {
            // Appearance is baked before discarding material state. Hidden
            // reservoirs/wet pigment must not bring discarded content back.
            if let Some(stored) = r.paint_layers.iter().find(|l| l.id == target) {
                for p in &stored.watercolor_wetness_pages {
                    r.encode_clear(encoder, &p.primary.view, "clear baked wetness");
                    r.encode_clear(encoder, &p.secondary.view, "clear baked wetness companion");
                }
                for p in &stored.coverage_pages {
                    r.encode_clear(encoder, &p.primary.view, "clear baked stroke coverage");
                    r.encode_clear(encoder, &p.secondary.view, "clear baked coverage companion");
                }
            }
            if let Some(stored) = r.paint_layers.iter_mut().find(|l| l.id == target) {
                stored.watercolor = None;
            }
        }
        Ok(())
    }

    pub(super) fn object_content_bounds(layers:SceneView<'_>,owner:OccurrenceHandle)->DocRect {
        object_spatial::SpatialIndex::default().content(layers,owner).map_or_else(DocRect::default,|content|content.bounds_at(0))
    }
    pub(super) fn capture_window(layers: SceneView<'_>, region: PixelRect, extent: [u32; 2]) -> DocRect {
        if matches!(layers.scope(),Some(layer_core::SceneScope::Raw(_) | layer_core::SceneScope::RawObjects(_))) {region.into()}
        else {images::capture_window(layers, region, extent)}
    }
    pub(super) fn cached_capture_window(&self,layers:SceneView<'_>,region:PixelRect)->DocRect {
        images::capture_window_cached(layers,region,Some(&self.object_spatial))
    }
    pub(super) fn window_plan(layers:SceneView<'_>,extent:[u32;2],limit:u64,maximum_side:u32,scene:Option<&Self>)->Result<Option<windows::Plan>,GpuRasterError> {
        windows::Plan::new_cached(layers,extent,limit,maximum_side,scene.map(|scene|&scene.object_spatial))
    }
    pub(super) fn release_capture_window(&mut self, window: DocRect) {
        // The snapshot owner has completed its previous readback. Release job
        // references and old image windows before restoring the next inputs.
        self.jobs.clear();self.source_jobs.clear();
        if self.images.doc_bounds != window {
            self.retire_images(|scene| scene.images = images::ImageStages::default());
        }
    }
    pub(super) fn capture_image_bound(scene: SceneView<'_>, window: impl Into<DocRect>) -> u64 {
        if matches!(scene.scope(),Some(layer_core::SceneScope::Raw(_))) {return 0;}
        let window = window.into();
        let raw = match scene.scope() { Some(layer_core::SceneScope::RawObjects(owner)) => Some(*owner), _ => None };
        let objects = scene.order().iter().copied()
            .filter(|owner| raw.map_or_else(|| scene.visible(*owner), |raw| raw == *owner) && scene.object_layer(*owner).is_some())
            .map(|owner| Self::object_content_bounds(scene, owner).intersect(window).aligned(PAGE_SIZE).area().saturating_mul(16))
            .fold(0u64, u64::saturating_add);
        if raw.is_some() {return objects;}

        let mut images = 0u64;
        let mut scratch = 0;
        for &handle in scene.order() {
            if !scene.visible(handle) { continue; }
            if let Some(effect) = scene.effect(handle).filter(|effect| effect.program.image_boundary()) {
                images += 2 + u64::from(scene.mask(handle).is_some_and(|(mask, _)| mask.enabled));
                scratch = scratch.max(effect.program.passes.len().saturating_sub(1).min(2) as u64);
            }
        }
        objects.saturating_add(window.area().saturating_mul(16).saturating_mul(images + scratch))
    }

    /// Capture a document-coordinate crop with its finite upstream dependencies.
    /// A larger destination receives the crop in its top-left prefix.
    pub(super) fn capture_region(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        destination: &wgpu::Texture,
        region: PixelRect,
        output: Output,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        self.capture_query_region(r, packet, destination, region, output, false, encoder)
    }

    #[expect(clippy::too_many_arguments, reason = "Query capture keeps output encoding, preview mode, and destination region explicit")]
    pub(super) fn capture_query_region(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, destination: &wgpu::Texture,
        region: PixelRect, output: Output, preview: bool, encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        if region.is_empty() || region.intersect(PixelRect::full(packet.document_extent)) != region
            || destination.width() < region.width() || destination.height() < region.height()
        {
            return Err(GpuRasterError::InvalidExtent);
        }
        let window = if matches!(output,Output::Source(_) | Output::MaskImage(_) | Output::Objects(_)) {region.into()} else {self.cached_capture_window(packet.scene, region)};
        let dirty = match output { Output::Display => PixelRect::EMPTY, _ => window.in_frame(packet.document_extent) };
        self.object_display = self.object_live || matches!(output, Output::Display);
        let raw = match output { Output::Objects(owner) => Some(owner), _ => None };
        if !matches!(output, Output::Source(_) | Output::MaskImage(_)) && !self.prepare_object_pages(r, packet, raw, page_coordinates(window.in_frame(packet.document_extent)))? {
            return Err(GpuRasterError::DeferredObjectWork);
        }
        self.prepare_region(r, packet, window, dirty, matches!(output,Output::Source(_) | Output::MaskImage(_) | Output::Objects(_)), encoder)?;
        let destination = Image { texture: destination.clone(), view: destination.create_view(&Default::default()),
            plan: display_mips::Plan::window(packet.document_extent, 0, region) };
        self.capture_prepared_regions(r, packet, &destination, &[region], output, preview, None, encoder)
    }

    pub(crate) fn capture_region_prepared(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, destination: &wgpu::Texture,
        region: PixelRect, output: Output, encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        if r.snapshot_worker || !packet.scene.order().iter().any(|owner| packet.scene.object_layer(*owner).is_some()) {
            return self.capture_region(r, packet, destination, region, output, encoder);
        }
        r.drain_image_decodes(encoder)?;
        self.object_results.retain(packet.scene,packet.blend_space,r.device.working_space());
        let mut objects = std::mem::take(&mut self.object_results);
        let advanced = objects.advance(r, self, encoder, r.moving_layer.is_some() || !packet.dab_batches.is_empty());
        self.object_results = objects;
        advanced?;
        self.object_query = true;
        let result = self.capture_region(r, packet, destination, region, output, encoder);
        self.object_query = false;
        result
    }

    fn prepare_region(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, window: DocRect,
        dirty: PixelRect, raw: bool, encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        self.begin_frame();
        self.prepare_region_in_frame(r, packet, window, dirty, raw, encoder)
    }

    fn prepare_region_in_frame(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, window: DocRect,
        dirty: PixelRect, raw: bool, encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        let write = self.begin_write(r);
        self.object_spatial.prepare(packet.scene);
        if self.placement_display { self.retire_images(|scene| scene.images = images::ImageStages::default()); }
        self.placement_display = false;
        if let Some(mut transforms) = r.transforms.take() {
            let result = transforms.materialize_region(r, encoder, packet.scene, window);
            r.transforms = Some(transforms);
            result?;
        }
        self.clear_material_pages();
        self.record_count = 0;
        self.effect_passes = 0;
        self.image_window = Some(window);
        self.effects.retain(packet.scene);
        let result = if raw {
            self.retire_images(|scene|scene.images=images::ImageStages::default());
            Ok(())
        } else { self.update_images(r, packet, dirty, encoder).map(|_|()) };
        self.stop_before = None;
        if result.is_ok() || matches!(result, Err(GpuRasterError::DeferredObjectWork)) { write.track(encoder); }
        result.map(|_| ())
    }

    pub(crate) fn release_exact_objects(&mut self,encoder:&crate::submission::CommandEncoder) {
        self.exact_object_results.clear();
        self.exact_object_results.flush_retired(encoder);
    }

    pub(crate) async fn drain_exact_objects_async(&mut self,r:&mut WgpuRasterizer)->Result<(),GpuRasterError> {
        while self.exact_object_results.pending() {
            if r.snapshot_cancelled.as_ref().is_some_and(|cancelled|cancelled.load(std::sync::atomic::Ordering::Relaxed)) {
                return Err(GpuRasterError::Color("Snapshot capture cancelled".into()));
            }
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            r.drain_image_decodes(&mut encoder)?;
            let mut objects=std::mem::take(&mut self.exact_object_results);
            let result=objects.advance(r,self,&mut encoder,false);
            self.exact_object_results=objects;
            let idle=result? == 0 && encoder.pass_count() == 0;
            r.uploads.finish(&encoder);
            encoder.submit(&r.queue);
            if idle && r.image_decode_waiting() { r.await_image_decode().await?; continue; }
            r.metrics.object_drain_submissions += 1;
            crate::local_tone::wait_async(&r.device,&r.queue).await.map_err(GpuRasterError::Color)?;
        }
        if r.snapshot_cancelled.as_ref().is_some_and(|cancelled|cancelled.load(std::sync::atomic::Ordering::Relaxed)) {
            return Err(GpuRasterError::Color("Snapshot capture cancelled".into()));
        }
        Ok(())
    }

    #[expect(clippy::too_many_arguments, reason = "Tiled capture keeps submission size and generic tile consumer explicit")]
    pub(super) async fn capture_query_tiles(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, region: PixelRect, output: Output, preview:bool, tiles_per_submission:u32,
        encoder: &mut crate::submission::CommandEncoder,
        mut consume: impl FnMut(&mut WgpuRasterizer, &wgpu::TextureView, PixelRect, &mut crate::submission::CommandEncoder) -> Result<(), GpuRasterError>,
    ) -> Result<(), GpuRasterError> {
        let window = if matches!(output,Output::Source(_) | Output::MaskImage(_) | Output::Objects(_)) {region.into()} else {self.cached_capture_window(packet.scene, region)};
        loop {
            match self.prepare_region(r, packet, window, window.in_frame(packet.document_extent), matches!(output,Output::Source(_) | Output::MaskImage(_) | Output::Objects(_)), encoder) {
                Err(GpuRasterError::DeferredObjectWork)=> {
                    r.uploads.finish(encoder);
                    std::mem::replace(encoder,crate::submission::CommandEncoder::new(&r.device,&Default::default())).submit(&r.queue);
                    crate::local_tone::wait_async(&r.device,&r.queue).await.map_err(GpuRasterError::Color)?;
                    self.drain_exact_objects_async(r).await?;
                },
                result=> {result?;break;},
            }
        }
        let (texture, view) = create_color_target(&r.device, [PAGE_SIZE;2], "statistics tile");
        let mut tiles=0;
        for tile in page_coordinates(region) {
            let grid = [tile[0]*PAGE_SIZE,tile[1]*PAGE_SIZE,packet.document_extent[0],packet.document_extent[1]];
            if preview && query_grid_size(grid).contains(&0) {continue;}
            let region = page_rect(tile).intersect(region);
            let destination = Image {texture:texture.clone(),view:view.clone(),plan:display_mips::Plan::window(packet.document_extent,0,region)};
            loop {
                match self.capture_prepared_regions(r, packet, &destination, &[region], output, preview, None, encoder) {
                    Err(GpuRasterError::DeferredObjectWork)=> {
                        r.uploads.finish(encoder);
                        std::mem::replace(encoder,crate::submission::CommandEncoder::new(&r.device,&Default::default())).submit(&r.queue);
                        crate::local_tone::wait_async(&r.device,&r.queue).await.map_err(GpuRasterError::Color)?;
                        self.drain_exact_objects_async(r).await?;
                    },
                    result=> {result?;break;},
                }
            }
            consume(r, &view, region, encoder)?;
            tiles+=1;
            if !preview && tiles%tiles_per_submission==0 {
                r.uploads.finish(encoder);
                std::mem::replace(encoder, crate::submission::CommandEncoder::new(&r.device, &Default::default())).submit(&r.queue);
                crate::local_tone::wait_async(&r.device, &r.queue).await.map_err(GpuRasterError::Color)?;
            }
        }
        self.release_exact_objects(encoder);
        Ok(())
    }

    #[expect(clippy::too_many_arguments, reason = "Prepared capture keeps output encoding and independent destination regions explicit")]
    fn capture_prepared_regions(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, destination: &Image,
        regions: &[PixelRect], output: Output, preview: bool, reserved: Option<&[bool]>, encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        self.capture_prepared_regions_reuse(r,packet,destination,regions,output,preview,reserved,None,encoder)
    }
    #[expect(clippy::too_many_arguments, reason = "Retained capture owns explicit initialized output and dependency regions")]
    fn capture_prepared_regions_reuse(
        &mut self,r:&mut WgpuRasterizer,packet:FramePacket<'_>,destination:&Image,
        regions:&[PixelRect],output:Output,preview:bool,reserved:Option<&[bool]>,initialized:Option<&std::collections::BTreeSet<[u32;2]>>,encoder:&mut submission::CommandEncoder,
    )->Result<(),GpuRasterError> {
        self.jobs.clear();self.source_jobs.clear();
        self.object_display = self.object_live || matches!(output, Output::Display);
        self.clear_material_pages();
        self.used.fill(false);
        if let Some(reserved) = reserved { self.used[..reserved.len()].copy_from_slice(reserved); }
        self.stop_before = packet.scene.effect_input();
        let object_pages = packet.scene.order().iter().any(|owner| packet.scene.object_layer(*owner).is_some());
        for tile in regions.iter().flat_map(|r| page_coordinates(*r)) {
            let covered = regions.iter().fold(PixelRect::EMPTY, |bounds, region| bounds.union(page_rect(tile).intersect(*region))).intersect(destination.plan.bounds);
            if covered.is_empty() { continue; }
            let image = match output {
                Output::Artwork(parent) => {
                    let image = self.group(r, packet, parent, tile)?;
                    self.converted(r, image, Convert::linear(packet))
                }
                Output::Objects(id) => {
                    let image = self.object_tile(r, packet, id, tile)?;
                    self.converted(r, image, Convert::linear(packet))
                }
                Output::OwnerContent(id) => {
                    if !packet.scene.eligible_target(id) { return Err(GpuRasterError::InvalidExtent); }
                    let image = if packet.scene.visible(id) { self.layer(r, packet, id, tile)? }
                        else { self.alloc(r, wgpu::Color::TRANSPARENT) };
                    self.converted(r, image, Convert::linear(packet))
                }
                Output::EffectInput(id) | Output::EffectChannels(id) | Output::EffectComposite(id) => {
                    packet.scene.occurrence(id).ok_or(GpuRasterError::InvalidExtent)?;
                    self.stop_before = Some(id);
                    let image = self.group(r, packet, layer_core::composite_input_scope(packet.scene, id), tile)?;
                    self.stop_before = packet.scene.effect_input();
                    let image = if matches!(output, Output::EffectInput(_)) { image } else { self.effect(r, packet, &[id], tile, image)? };
                    if matches!(output, Output::EffectComposite(_)) { image } else { self.converted(r, image, Convert::linear(packet)) }
                }
                Output::Source(target) | Output::MaskImage(target) => {
                    let owner=packet.scene.source_owner(target).ok_or(GpuRasterError::MissingPaintLayer(target))?;
                    match target {
                        SourceTarget::Paint(_) => self.paint_tile(r, packet, owner, tile)?,
                        SourceTarget::Coverage(_) => {
                            let (use_,source)=packet.scene.mask(owner).ok_or(GpuRasterError::MissingPaintLayer(target))?;
                            let mut use_=use_.clone(); use_.inverted=false;
                            let mask = self.mask_at(r,&use_,source,packet.scene.target_offset(target),tile);
                            if matches!(output, Output::MaskImage(_)) {
                                let image = self.alloc(r, wgpu::Color::TRANSPARENT);
                                self.draw(r, image, self.pool[mask].view.clone(), None, [0., 0., 256., 256.], [20., 1., 0., 0.], false, Convert::None);
                                self.free(mask); image
                            } else { mask }
                        }
                        SourceTarget::Selection(_) => return Err(GpuRasterError::MissingPaintLayer(target)),
                    }
                },
                Output::Display => self.display_tile(r, packet, tile)?,
            };
            if let Output::EffectComposite(handle)=output && destination.plan.level>0 {
                let execution=if matches!(self.jobs.last(),Some(Job::Effect {data,..}) if data[19]>2.5) {effects::Execution::ReducedContribution} else {effects::Execution::Reduced};
                let prepared=self.effects.prepare(r,packet.scene,&[handle],execution,packet.time_seconds,0,packet.blend_space)?;
                let Job::Effect {target,data,prepared:effect,source_target,changed_cells,..}=self.jobs.last_mut().unwrap() else {unreachable!()};
                let rect=paint_transform::texel_rect(covered.window_local(destination.plan.bounds),1<<destination.plan.level);
                *target=destination.view.clone();
                data[..4].copy_from_slice(&rect.map(|v|v as f32));
                data[4..6].copy_from_slice(&destination.plan.size.map(|v|v as f32));
                data[24..26].copy_from_slice(&[(covered.max_x()-tile[0]*PAGE_SIZE) as f32,(covered.max_y()-tile[1]*PAGE_SIZE) as f32]);
                data[26]=1.;
                data[27]=(1<<destination.plan.level) as f32;
                data[28..30].copy_from_slice(&[(covered.min_x()-tile[0]*PAGE_SIZE) as f32,(covered.min_y()-tile[1]*PAGE_SIZE) as f32]);
                *effect=prepared;
                if initialized.is_some_and(|pages|pages.contains(&tile)) && !packet.composite_all && r.document_damage.is_empty() {
                    *changed_cells=source_target.and_then(|id|r.changed_cells.as_ref().unwrap().reusable(id,tile)).cloned();
                }
                let cells=u64::from(rect[2])*u64::from(rect[3]);
                if changed_cells.is_some() {r.metrics.native_effect_reuse_candidate_cells+=cells;}
                else {r.metrics.native_effect_forced_cells+=cells;}
                self.free(image);
            } else {self.copy_window_tile(image, destination, tile);}
            if preview || object_pages {
                let result = self.encode_query_jobs(r, encoder, preview.then_some([tile[0]*PAGE_SIZE,tile[1]*PAGE_SIZE,packet.document_extent[0],packet.document_extent[1]]));
                if result.is_err() { self.object_results.reset_used(); self.exact_object_results.reset_used(); return result; }
                self.object_results.flush_retired(encoder); self.exact_object_results.flush_retired(encoder);
            }
        }
        if !preview && self.jobs.len() <= SOURCE_SLOTS * 2 && self.jobs.iter().all(|job| match job {
            Job::DecodedTile(_) => true,
            Job::Effect { target, prepared, .. } => *target == destination.view && prepared.pointwise,
            _ => false,
        }) {
            let mut prefix = 0;
            for i in 0..self.jobs.len() {
                let Job::DecodedTile(pending) = &self.jobs[i] else { continue; };
                if self.jobs[..i].iter().any(|job| match job {
                    Job::DecodedTile(previous) => previous.view == pending.view,
                    Job::Effect { sources, masks, .. } => sources.contains(&pending.view) || masks.contains(&pending.view),
                    _ => true,
                }) { continue; }
                self.jobs[prefix..=i].rotate_right(1);
                prefix += 1;
            }
            self.source_jobs.clear();
        }
        crate::performance_trace::counter(c"Capy native effect reuse candidates",r.metrics.native_effect_reuse_candidate_cells);
        crate::performance_trace::counter(c"Capy native effect forced cells",r.metrics.native_effect_forced_cells);
        let result = self.encode_jobs(r, encoder);
        if result.is_ok() {
            self.object_results.flush_retired(encoder);
            self.exact_object_results.flush_retired(encoder);
        } else {
            self.object_results.reset_used();
            self.exact_object_results.reset_used();
        }
        result
    }

    pub(crate) fn raster_published(&mut self,id:SourceTarget,data:Arc<layer_core::raster::RasterData>) {
        self.scale_sources.published(id,data);
    }

    pub fn compose(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        batch_tiles: &[Vec<brush_tiles::BrushTile>],
        dirty: PixelRect,
        encoder: &mut crate::submission::CommandEncoder,
        tiles: Option<&std::collections::BTreeSet<[u32; 2]>>,
    ) -> Result<PixelRect, GpuRasterError> {
        let write = self.begin_write(r);
        self.object_results.retain(packet.scene,packet.blend_space,r.device.working_space());
        self.object_spatial.prepare(packet.scene);
        if let Some(cache) = &r.scale_display {
            let bounds = self.cached_capture_window(packet.scene, cache.plan.bounds);
            let level = if cache.evaluation == scale::Evaluation::Native { 0 } else { cache.plan.level };
            self.object_results.retain_view(bounds, level);
        }
        let mut objects = std::mem::take(&mut self.object_results);
        let input = !packet.dab_batches.is_empty() || r.moving_layer.is_some();
        let advanced = if input && !r.object_deferred { Ok(0) } else { objects.advance(r, self, encoder, input) };
        let mut refinements = objects.take_damage();
        let mut damage: std::collections::BTreeMap<_, _> = refinements.extract_if(.., |(level, _), _| *level == 0).map(|((_, owner), damage)| (owner, damage)).collect();
        let source = std::mem::take(&mut self.object_source_damage);
        if !source.is_empty() { damage.entry(None).or_insert(scale::Damage::EMPTY).extend(&source); }
        self.scale_sources.object_refinements = refinements;
        self.scale_sources.object_damage = damage;
        self.object_results = objects;
        advanced?;
        let dirty = self.scale_sources.object_damage.values().fold(dirty, |dirty, damage| dirty.union(damage.bounds()));
        self.scale_sources.prepare(r, packet, batch_tiles);
        let mut commands = self.scale_commands.take().unwrap_or_else(|| scale::Commands::new(r));
        commands.begin();
        let mut cache = r.scale_display.take().expect("prepared display cache");
        if self.scale_sources.object_damage.values().chain(self.scale_sources.object_refinements.values()).any(|damage| !damage.is_empty()) { cache.refresh_objects(); }
        cache.submission_valid = Some(self.valid.clone());
        self.object_live = true;
        self.object_display = true;
        let result = (|| {
            cache.prepare_graph(r, packet, self, &commands, tiles)?;
            cache.invalidate_hierarchy(&self.scale_sources, dirty, tiles);
            if cache.evaluation == scale::Evaluation::Native {
                self.image_damage = tiles.filter(|_|!scale::bounded(packet.scene)).map(|tiles|scale::Damage::from_tiles(dirty,Some(tiles)));
                self.scale_sources.retain_levels(&Default::default(), 0);
                cache.render_native(self, r, packet, dirty, &mut scale::Encoding { encoder, commands: &mut commands }, tiles)
            } else {
                self.retire_images(|scene| scene.images.release_window_pixels());
                self.image_window = None;
                if cache.plan.level > 0 && !scale::bounded(packet.scene) {
                    self.clear_material_pages();
                    self.pool.clear();
                    self.used.clear();
                }
                self.placement_display = true;
                self.effects.retain(packet.scene);
                r.telemetry.phase_begin(8, &r.device, &r.queue, encoder);
                self.prepare_display_sources(&cache, &mut commands, r, packet, encoder)?;
                r.telemetry.phase_end(8, encoder);
                cache.render(self, r, packet, dirty, &mut scale::Encoding { encoder, commands: &mut commands }, tiles)
            }
        })();
        self.object_live = false;
        self.object_display = false;
        self.scale_commands = Some(commands);
        r.scale_display = Some(cache);
        if result.is_err() { self.object_results.reset_used(); }
        self.object_results.flush_retired(encoder);
        self.image_damage = None;
        if result.is_ok() || matches!(result, Err(GpuRasterError::DeferredObjectWork)) { write.track(encoder); }
        result
    }

    fn display_tile(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, tile: [u32; 2]) -> Result<usize, GpuRasterError> {
        let mut output = self.group(r, packet, None, tile)?;
        if let Some(handle) = packet.inspect_mask
            && let Some((mask, source)) = packet.scene.mask(handle).filter(|(mask, _)| mask.enabled) {
            let m = self.mask_at(r, mask, source, packet.scene.target_offset(SourceTarget::Coverage(mask.source)), tile);
            let tint = self.alloc(r, wgpu::Color::TRANSPARENT);
            self.draw(
                r,
                tint,
                self.pool[m].view.clone(),
                None,
                [0., 0., 256., 256.],
                [5., 1., 0., 0.],
                false,
                Convert::layers(packet),
            );
            self.free(m);
            output = self.combine(
                r,
                tint,
                output,
                1.,
                layer_core::LayerBlend::Normal,
                false,
                packet.blend_space,
            );
        }
        Ok(output)
    }

    // wgpu handles hash by stable resource identity, not mutable GPU contents.
    #[allow(clippy::mutable_key_type)]
    fn encode_jobs(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        self.encode_query_jobs(r, encoder, None)
    }

    fn encode_query_jobs(&mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder, grid: Option<[u32;4]>) -> Result<(), GpuRasterError> {
        let result = self.encode_jobs_inner(r, encoder, grid);
        if result.is_err() {self.discard_unencoded_jobs();}
        result
    }

    fn discard_unencoded_jobs(&mut self) {
        self.jobs.clear();self.source_jobs.clear();self.clear_material_pages();self.used.fill(false);
        self.object_results.reset_used();self.exact_object_results.reset_used();
    }

    fn encode_jobs_inner(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        grid: Option<[u32;4]>,
    ) -> Result<(), GpuRasterError> {
        let source_jobs = self.source_jobs.clone();
        let job_grid = |i| grid.filter(|_| !source_jobs.iter().any(|range| range.contains(&i)));
        let base = self.record_count;
        self.effects.encode_preparation(encoder);
        self.record_count += self.jobs.len();
        if self.record_count > self.capacity {
            self.capacity = self.record_count.next_power_of_two();
            self.buffer = r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("scene records"),
                size: (self.capacity * self.stride) as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.binding = uniform_binding(&r.device, &self.uniforms, &self.buffer);
            self.reduction_binding = scale::Commands::record_binding(r, &self.buffer);
        }
        self.upload.resize(self.jobs.len() * self.stride, 0);
        for (i, job) in self.jobs.iter().enumerate() {
            let sampling = if matches!(job, Job::Draw {..} | Job::Effect {..}) {job_grid(i).unwrap_or_default()} else {[0;4]};
            self.upload[i*self.stride+128..i*self.stride+144].copy_from_slice(&sampling.into_iter().flat_map(u32::to_ne_bytes).collect::<Vec<_>>());
            let mut captured;
            let data: Option<&[f32]> = match job {
                Job::Reduce { values, .. } => {
                    captured = [0.; 32];
                    for (out, value) in captured.iter_mut().zip(scale::Commands::reduction_record(r, *values)) { *out = f32::from_bits(value); }
                    Some(&captured)
                }
                Job::Draw { data, .. } | Job::Effect { data, .. } => Some(data.as_slice()),
                Job::DecodedTile(pending) => pending.data.as_ref().map(|data| data.as_slice()),
                _ => None,
            };
            if let Some(data) = data {
                for (j, v) in data.iter().enumerate() {
                    self.upload[i * self.stride + j * 4..i * self.stride + j * 4 + 4]
                        .copy_from_slice(&v.to_ne_bytes());
                }
            }
        }
        if !self.upload.is_empty() {
            r.uploads.write_at(
                encoder,
                &self.buffer,
                (base * self.stride) as u64,
                &self.upload,
            )?;
        }
        let is_compute = |job: &Job| {
            if matches!(job,Job::Effect {prepared:effects::PreparedEffect {pipeline:effects::Pipeline::Compute(_),..},..}) {return true;}
            let Job::Draw { data, over: false, clip, .. } = job else { return false; };
            if data[8] == 15. || (data[8] == 13. && clip.is_some()) { return true; }
            if !matches!(data[8] as u32, 1 | 13 | 14 | 17 | 19) { return false; }
            data[..4].iter().all(|v| v.fract() == 0.) && clip.is_none_or(|clip| {
                clip == PixelRect::new(data[0].clamp(0., data[4]) as u32, data[1].clamp(0., data[5]) as u32,
                    (data[0] + data[2]).clamp(0., data[4]) as u32,
                    (data[1] + data[3]).clamp(0., data[5]) as u32)
            })
        };
        let overwritten_clear = |i: usize| {
            let Job::Clear(target, _) = &self.jobs[i] else { return false; };
            self.jobs.get(i + 1).is_some_and(|next|
                matches!(next, Job::Draw { target: next, .. } | Job::Effect { target: next, .. }
                    | Job::Watercolor { target: next, .. } if next == target)
                && (!is_compute(next) || matches!(next, Job::Draw { data, .. } | Job::Effect {data,..}
                    if data[..2] == [0., 0.] && data[2..4] == data[4..6])))
        };
        let is_material_compute = |i: usize| match &self.jobs[i] {
            Job::Placement(_) | Job::Reduce { .. } => true,
            Job::Watercolor { target, .. } => i > 0 && matches!(&self.jobs[i - 1],
                Job::Clear(previous, color) if previous == target && *color == wgpu::Color::TRANSPARENT),
            _ => false,
        };
        let material_clear = |i: usize| matches!(self.jobs[i], Job::Clear(..))
            && matches!(self.jobs.get(i + 1), Some(Job::Placement(_) | Job::Watercolor { .. })) && is_material_compute(i + 1);
        let source_bindings = &mut self.source_bindings;
        let mask_bindings = &mut self.mask_bindings;
        let output_bindings = &mut self.output_bindings;
        let compute_bindings = &mut self.compute_bindings;
        let effect_outputs=&mut self.effect_outputs;
        let mut encoded_through = 0;
        for (i, job) in self.jobs.iter().enumerate() {
            if i < encoded_through {
                continue;
            }
            if is_material_compute(i) {
                let end = (i + 1..self.jobs.len()).find(|&j| !is_material_compute(j) && !material_clear(j)).unwrap_or(self.jobs.len());
                let mut draws = Vec::new();
                for job in &self.jobs[i..end] {
                    if let Job::Placement(job) = job {
                        let plane = usize::from(job.scalar);
                        draws.push((plane, placement::prepare(&mut self.placement[plane], r, encoder, job)?));
                    }
                }
                let mut draws = draws.iter();
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("placed material tiles"), timestamp_writes: None,
                });
                for (j, job) in self.jobs.iter().enumerate().take(end).skip(i) {
                    match job {
                        Job::Placement(_) => {
                            let (plane, draw) = draws.next().unwrap();
                            self.placement[*plane].encode_placement(&mut pass, draw);
                        }
                        Job::Watercolor { coordinates, target, binding, record } => {
                            let (layout, pipeline) = &r.pipelines.watercolor_compute;
                            let output = self.watercolor_outputs.get(target, || crate::bindings::group(
                                &r.device, "watercolor output", layout, [wgpu::BindingResource::TextureView(target)],
                            ));
                            pass.set_pipeline(pipeline);
                            pass.set_bind_group(0, &r.style_bind_group, &[*record * r.style_stride as u32]);
                            pass.set_bind_group(1, &coordinates.0, &[coordinates.1]);
                            pass.set_bind_group(2, binding, &[]);
                            pass.set_bind_group(3, output, &[]);
                            pass.dispatch_workgroups(PAGE_SIZE.div_ceil(8), PAGE_SIZE.div_ceil(8), 1);
                        }
                        Job::Reduce { binding, size, kernel, .. } => {
                            pass.set_pipeline(match kernel {
                                Reduction::Pages => &r.scene_pipelines.scale.reduce,
                                Reduction::Phased => &r.scene_pipelines.scale.reduce_phased,
                            });
                            pass.set_bind_group(0, &self.reduction_binding, &[((base + j) * self.stride) as u32]);
                            pass.set_bind_group(1, binding, &[]);
                            pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
                        }
                        Job::Clear(..) => {}
                        _ => unreachable!(),
                    }
                }
                encoded_through = end;
                continue;
            }
            if is_compute(job) {
                let (layout, inputs, pipeline) = &r.scene_pipelines.constant;
                let end = (i + 1..self.jobs.len()).find(|&j| !is_compute(&self.jobs[j]) && !overwritten_clear(j))
                    .unwrap_or(self.jobs.len());
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("scene pointwise tiles"), timestamp_writes: None,
                });
                self.effect_passes+=u64::from(self.jobs[i..end].iter().any(|job|matches!(job,Job::Effect {..})));
                for (j, job) in self.jobs.iter().enumerate().take(end).skip(i) {
                    if overwritten_clear(j) { continue; }
                    if let Job::Effect {target,sources,data,prepared,masks,..}=job {
                        let effects::Pipeline::Compute(pipeline)=&prepared.pipeline else {unreachable!()};
                        let input=source_bindings.get(&(true,sources.clone()),||source_binding(r,&self.effects.sources,sources,false));
                        let output=effect_outputs.get(&(prepared.mask_slots,masks.as_ref().clone(),target.clone()),||crate::bindings::group(
                            &r.device,"pointwise effect output",&self.effects.outputs[prepared.mask_slots-1],
                            masks.iter().take(prepared.mask_slots).map(wgpu::BindingResource::TextureView).chain([wgpu::BindingResource::TextureView(target)]),
                        ));
                        pass.set_pipeline(pipeline);
                        pass.set_bind_group(0,&self.binding,&[((base+j)*self.stride) as u32]);
                        pass.set_bind_group(1,input,&[]);
                        pass.set_bind_group(2,&prepared.binding,&[]);
                        pass.set_bind_group(3,output,&[]);
                        let size=job_grid(j).map_or([data[2] as u32,data[3] as u32],query_grid_size);
                        debug_assert!(!size.contains(&0), "empty pointwise effect job was queued");
                        pass.dispatch_workgroups(size[0].div_ceil(32),size[1].div_ceil(2),1);
                        continue;
                    }
                    let Job::Draw { target, sources, data, .. } = job else { unreachable!() };
                    pass.set_pipeline(pipeline);
                    let input = compute_bindings.get(sources, || compute_source_binding(r, inputs, sources));
                    // All independent tiles in a complete image share this
                    // destination. Source-specific state belongs in the input
                    // group, not a new destination binding for every tile.
                    let output = output_bindings.get(target, || {
                        crate::bindings::group(&r.device, "scene normal layers destination", layout, [
                            wgpu::BindingResource::TextureView(target),
                        ])
                    });
                    pass.set_bind_group(0, &self.binding, &[((base + j) * self.stride) as u32]);
                    pass.set_bind_group(1, input, &[]);
                    pass.set_bind_group(2, output, &[]);
                    let size = job_grid(j).map_or([data[2] as u32,data[3] as u32], query_grid_size);
                    debug_assert!(!size.contains(&0), "empty pointwise draw job was queued");
                    pass.dispatch_workgroups(size[0].div_ceil(32), size[1].div_ceil(2), 1);
                }
                encoded_through = end;
                continue;
            }
            match job {
                Job::Collection(job) => r.scene_pipelines.objects.clone().preview(&r.device,encoder,&mut r.uploads,job,&r.empty_view)?,
                Job::PaintBase(assembly) => crate::source_access::encode_paint_base(assembly, encoder),
                Job::Placement(_) | Job::Reduce { .. } => unreachable!(),
                Job::Positions(geometry, tile) => {
                    self.positions.upload(r, encoder, geometry)?;
                    self.positions.draw(r, encoder, *tile)?;
                }
                Job::DecodedTile(pending) => {
                    if r.source_tiles.borrow().uploads_full() {
                        Self::submit_source_uploads(r, encoder)?;
                    }
                    let bytes = r.source_tiles.get_mut().encode(&r.device, &mut r.uploads, &r.scene_pipelines.source, encoder, pending, &self.binding, ((base + i) * self.stride) as u32)?;
                    let in_flight = r.source_tiles.borrow().charge_upload(encoder, bytes);
                    r.metrics.source_upload_peak_bytes = r.metrics.source_upload_peak_bytes.max(in_flight);
                }
                Job::Clear(target, color) => {
                    if overwritten_clear(i) { continue; }
                    let attachments = [Some(attachment(target, wgpu::LoadOp::Clear(*color)))];
                    let _pass = encoder.begin_render_pass(&descriptor(&attachments));
                }
                Job::Copy {
                    source,
                    source_origin,
                    destination,
                    origin,
                    width,
                    height,
                } => encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        origin: wgpu::Origin3d { x: source_origin[0], y: source_origin[1], z: 0 },
                        ..source.as_image_copy()
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: destination,
                        origin: wgpu::Origin3d {
                            x: origin[0],
                            y: origin[1],
                            z: 0,
                        },
                        ..source.as_image_copy()
                    },
                    wgpu::Extent3d {
                        width: *width,
                        height: *height,
                        depth_or_array_layers: 1,
                    },
                ),
                Job::Draw { target, .. }
                | Job::Effect { target, .. }
                | Job::Watercolor { target, .. } => {
                    let needs_blend = |job: &Job| r.device.portable_blend()
                        && matches!(job, Job::Draw { over: true, .. } | Job::Watercolor { .. });
                    let end = if needs_blend(job) { i + 1 } else { (i + 1..self.jobs.len())
                        .find(|&j| !matches!(&self.jobs[j],
                            Job::Draw { target: next, .. }
                            | Job::Effect { target: next, .. }
                            | Job::Watercolor { target: next, .. } if next == target) || needs_blend(&self.jobs[j])
                            || is_compute(&self.jobs[j]))
                        .unwrap_or(self.jobs.len()) };
                    let load = if i > 0
                        && let Job::Clear(previous, color) = &self.jobs[i - 1]
                        && previous == target
                    {
                        wgpu::LoadOp::Clear(*color)
                    } else {
                        wgpu::LoadOp::Load
                    };
                    let portable = needs_blend(job)
                        && !matches!(load, wgpu::LoadOp::Clear(color) if color == wgpu::Color::TRANSPARENT);
                    let source = portable.then(|| r.portable_blend.source(&r.device,target,r.device.working_format()));
                    if portable && let wgpu::LoadOp::Clear(color) = load {
                        let attachments = [Some(attachment(target,wgpu::LoadOp::Clear(color)))];
                        let _pass = encoder.begin_render_pass(&descriptor(&attachments));
                    }
                    let effect = self.jobs[i..end].iter().any(|j| matches!(j, Job::Effect { .. }));
                    let extent = [target.texture().width(), target.texture().height()];
                    let spatial = self.jobs[i..end].iter().any(|j| matches!(j, Job::Effect { prepared, .. } if !prepared.pointwise));
                    let span = if spatial { PAGE_SIZE * 2 } else { extent[0].max(extent[1]) };
                    let windows = (0..extent[1]).step_by(span as usize).flat_map(|y|
                        (0..extent[0]).step_by(span as usize).map(move |x|
                            PixelRect::new(x, y, (x + span).min(extent[0]), (y + span).min(extent[1]))));
                    for window in windows {
                        let load = if portable { wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT) }
                            else if window.min_x() == 0 && window.min_y() == 0 { load } else { wgpu::LoadOp::Load };
                        let attachments = [Some(attachment(source.as_ref().unwrap_or(target), load))];
                        let mut pass = encoder.begin_render_pass(&descriptor(&attachments));
                        if effect { self.effect_passes += 1; }
                        for (j, job) in self.jobs.iter().enumerate().take(end).skip(i) {
                            if let Job::Watercolor { coordinates, binding, record, .. } = job {
                                pass.set_pipeline(&r.pipelines.watercolor_composite);
                                pass.set_bind_group(0, &r.style_bind_group, &[*record * r.style_stride as u32]);
                                pass.set_bind_group(1, &coordinates.0, &[coordinates.1]);
                                pass.set_bind_group(2, binding, &[]);
                                pass.set_scissor_rect(0, 0, PAGE_SIZE, PAGE_SIZE);
                                pass.draw(0..3, 0..1);
                                continue;
                            }
                            let sources = match job {
                                Job::Draw { sources, .. } | Job::Effect { sources, .. } => sources,
                                _ => unreachable!(),
                            };
                            let effect=matches!(job,Job::Effect {..});
                            let layout=if effect {&self.effects.sources} else {&self.layout};
                            let binding = source_bindings.get(&(effect,sources.clone()), || source_binding(r, layout, sources, !effect));
                            if let Job::Effect {
                                prepared, masks, changed_cells, ..
                            } = job
                            {
                                let effects::Pipeline::Render(pipeline)=&prepared.pipeline else {unreachable!()};
                                pass.set_pipeline(pipeline);
                                pass.set_bind_group(2, &prepared.binding, &[]);
                                let cells=changed_cells.as_ref().unwrap_or(&r.changed_cells.as_ref().unwrap().disabled);
                                let masks = mask_bindings.get(&(masks.as_ref().clone(),cells.clone()), || {
                                    let mut entries:Vec<_>=masks.iter().enumerate().map(|(i,view)|wgpu::BindGroupEntry {
                                        binding:i as u32,resource:wgpu::BindingResource::TextureView(view),
                                    }).collect();
                                    entries.push(wgpu::BindGroupEntry {binding:effects::MASK_SLOTS as u32,resource:cells.as_entire_binding()});
                                    r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                        label:Some("effect tile masks and changed cells"),layout:&self.effects.masks,entries:&entries,
                                    })
                                });
                                pass.set_bind_group(3, masks, &[]);
                            } else if let Job::Draw { over, data, .. } = job {
                                pass.set_pipeline(&self.pipeline[if matches!(data[8] as u32, 21 | 22) { 2 } else { usize::from(*over) }]);
                            }
                            pass.set_bind_group(0, &self.binding, &[((base + j) * self.stride) as u32]);
                            pass.set_bind_group(1, binding, &[]);
                            let (data, clip) = match job {
                                Job::Draw { data, clip, .. } => (data, *clip),
                                // A fullscreen triangle extends beyond its rectangle.
                                // A final effect can write directly into one region
                                // of a larger composite: clip it to that region or
                                // it overwrites adjacent tiles with out-of-range reads.
                                Job::Effect { data, .. } => (data, Some(PixelRect::new(
                                    data[0].max(0.).floor() as u32,
                                    data[1].max(0.).floor() as u32,
                                    (data[0] + data[2]).min(data[4]).max(0.).ceil() as u32,
                                    (data[1] + data[3]).min(data[5]).max(0.).ceil() as u32,
                                ))),
                                _ => unreachable!(),
                            };
                            let clip = clip.unwrap_or(PixelRect::full([data[4] as u32, data[5] as u32])).intersect(window);
                            if clip.is_empty() { continue; }
                            pass.set_scissor_rect(
                                clip.min_x(),
                                clip.min_y(),
                                clip.width(),
                                clip.height(),
                            );
                            if let Some(grid) = job_grid(j) {
                                let size=query_grid_size(grid);pass.draw(0..6, 0..size[0]*size[1]);
                            } else {pass.draw(0..3, 0..1);}
                        }
                        drop(pass);
                    }
                    if let Some(source) = source {
                        r.portable_blend.apply(&r.device,encoder,&source,target,PixelRect::full([target.texture().width(),target.texture().height()]),0);
                    }
                    encoded_through = end;
                }
            }
        }
        self.jobs.clear();self.source_jobs.clear();
        Ok(())
    }
}

impl Pipelines {
    pub fn scale_pipelines(&self) -> [&Deferred<wgpu::ComputePipeline>; 5] {
        [&self.scale.reduce, &self.scale.reduce_phased, &self.scale.reduce_pair, &self.scale.compose, &self.resample.area]
    }
    pub fn effects(&self, r: &WgpuRasterizer) -> effects::Effects {
        effects::Effects::new(r, &self.uniforms)
    }
    pub fn new(device: &PipelineDevice) -> Self {
        let uniforms = crate::bindings::layout(device, "scene records", &[crate::bindings::buffer(
            0,
            wgpu::ShaderStages::VERTEX_FRAGMENT | wgpu::ShaderStages::COMPUTE,
            wgpu::BufferBindingType::Uniform,
            true,
            NonZeroU64::new(144),
        )]);
        let layout = crate::bindings::layout(device, "scene sources", &[
            texture_entry(0),
            texture_entry(1),
            crate::bindings::sampler(
                2,
                wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE,
                wgpu::SamplerBindingType::Filtering,
            ),
            texture_entry(3),
        ]);
        let shader = Deferred::new({
            let device = device.clone();
            move || {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("layer scene"),
                    source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[&working_color::shader(&device), &crate::view_color::hdr_shader(device.working_space(), layer_core::color::RgbSpace::Srgb), include_str!("blend_modes.wgsl"), include_str!("scene.wgsl"), "@group(1) @binding(3) var scene_extra:texture_2d<f32>; fn gradient_record(base:u32,index:u32)->vec4<f32>{return textureLoad(scene_extra,vec2<i32>(i32(index),0),0);}", include_str!("float_number.wgsl"), crate::gradient::SOURCE, include_str!("scene_constant.wgsl")])),
                })
            }
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene composition"),
            bind_group_layouts: &[Some(&uniforms), Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = [None, Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING), None].into_iter().enumerate().map(|(i, blend)| {
            let (device, pipeline_layout, shader) =
                (device.clone(), pipeline_layout.clone(), shader.clone());
            Deferred::pipeline(move |mode| {
                fullscreen_pipeline_recipe(
                    mode,
                    &device,
                    &pipeline_layout,
                    &shader,
                    "fragment_main",
                    blend,
                    if i == 2 { device.scalar_format() } else { device.working_format() },
                    "tile layer composition",
                )
            })
        }).collect::<Vec<_>>().try_into().ok().unwrap();
        let constant = {
            let inputs = crate::bindings::layout(device, "scene normal stack inputs", &[
                texture_entry(0), texture_entry(1),
                crate::bindings::sampler(2, wgpu::ShaderStages::COMPUTE, wgpu::SamplerBindingType::Filtering),
                texture_entry(3),
            ]);
            let output = crate::bindings::layout(device, "scene constant backdrop output", &[crate::bindings::storage_texture(
                0,
                wgpu::ShaderStages::COMPUTE,
                wgpu::TextureFormat::Rgba32Float,
                wgpu::StorageTextureAccess::WriteOnly,
            )]);
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("scene constant backdrop"),
                bind_group_layouts: &[Some(&uniforms), Some(&inputs), Some(&output)],
                immediate_size: 0,
            });
            let pipeline = Deferred::compute(device, "scene constant backdrop", &layout, &shader, "compose_constant");
            (output, inputs, pipeline)
        };
        Self {
            objects: crate::object_sampling::ObjectSampler::new(device),
            scale: scale::Pipelines::new(device),
            resample: resample::Resample::new(device),
            source: sources::Pipelines::new(device, &uniforms),
            constant,
            uniforms,
            layout,
            pipeline,
        }
    }
}

fn direct_effect_mask(scene: SceneView<'_>, handle: OccurrenceHandle) -> bool {
    scene.mask(handle).is_none_or(|(use_, _)| !use_.enabled || scene.target_offset(SourceTarget::Coverage(use_.source)) == [0; 2])
}
fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    crate::bindings::texture(
        binding,
        wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE,
        true,
    )
}
fn uniform_binding(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    crate::bindings::group(device, "scene uniform binding", layout, [
        wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer, offset: 0, size: NonZeroU64::new(144), }),
    ])
}
fn local_rect(c: [u32; 2], offset: [i64; 2], tile: [u32; 2]) -> [f32; 4] {
    let at = |i: usize| ((i64::from(c[i]) - i64::from(tile[i])) * i64::from(PAGE_SIZE) + offset[i]) as f32;
    [at(0), at(1), PAGE_SIZE as f32, PAGE_SIZE as f32]
}
fn intersects(r: [f32; 4]) -> bool {
    r[0] < 256. && r[1] < 256. && r[0] + r[2] > 0. && r[1] + r[3] > 0.
}
/// A layer's pixels placed in the document, for the samplers that transform
/// previews and pixel commits share.
/// How far right and down of a page corner `origin` lies.
pub(crate) fn page_phase(origin: [i64; 2]) -> [u32; 2] {
    origin.map(|value| value.rem_euclid(i64::from(PAGE_SIZE)) as u32)
}
pub(super) fn world_offset(scene: SceneView<'_>, handle: OccurrenceHandle, mask: bool) -> [i64; 2] {
    if mask { scene.mask(handle).map_or_else(Default::default, |(use_, _)| scene.target_offset(SourceTarget::Coverage(use_.source))) }
    else { scene.occurrence_offset(handle) }
}
fn attachment(
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        resolve_target: None,
        depth_slice: None,
        ops: wgpu::Operations {
            load,
            store: wgpu::StoreOp::Store,
        },
    }
}
fn descriptor<'a>(
    attachments: &'a [Option<wgpu::RenderPassColorAttachment<'a>>],
) -> wgpu::RenderPassDescriptor<'a> {
    wgpu::RenderPassDescriptor {
        label: Some("layer tile scene"),
        color_attachments: attachments,
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    }
}

fn constant_frame_tile(scene:SceneView<'_>,tile:[u32;2],window:Option<DocRect>)->bool {
    let start=scene.evaluation_offset64();let extent=scene.composition().size;
    let low=tile.map(|coordinate|i64::from(coordinate)*i64::from(PAGE_SIZE));
    let region=DocRect {min:low,max:low.map(|coordinate|coordinate+i64::from(PAGE_SIZE))};
    let region=window.map_or(region,|window|region.intersect(window));
    !region.is_empty()&&(0..2).all(|axis|region.min[axis] as f64>=start[axis] && region.max[axis] as f64<=start[axis]+f64::from(extent[axis]))
}
fn fusable_adjustment(scene: SceneView<'_>, handle: OccurrenceHandle) -> bool {
    scene.effect(handle).is_some_and(|effect| effect.program.kind == layer_core::EffectKind::Adjustment && !effect.program.fusion_boundary())
}
fn fuses_after(scene: SceneView<'_>, head: OccurrenceHandle, next: OccurrenceHandle, chain: usize) -> bool {
    let occurrence = scene.occurrence(next).unwrap();
    scene.visible(next) && scene.effect_owner(next) == scene.effect_owner(head)
        && scene.effect(next).is_some_and(|next| scene.effect(head).is_some_and(|head| next.program.space == head.program.space))
        && direct_effect_mask(scene, next)
        && !(chain >= effects::MASK_SLOTS && occurrence.mask.as_ref().is_some_and(|mask| mask.enabled))
        && fusable_adjustment(scene, next)
}
pub(super) fn startup_effect_chains(scene: SceneView<'_>) -> Vec<(Vec<OccurrenceHandle>, effects::Execution)> {
    let mut result = Vec::new();
    let mut parents = std::collections::BTreeSet::new();
    for &handle in scene.order() {
        parents.insert(scene.evaluation_parent(handle));
        if !scene.visible(handle) { continue; }
        if let Some(effect) = scene.effect(handle).filter(|effect| effect.constant_color().is_none() || stack::support(scene,0)!=Some(0)) {
            if effect.program.image_boundary() {
                for pass in 0..effect.program.passes.len().max(1) { result.push((vec![handle], effects::Execution::Image(pass))); }
            } else {
                result.push((vec![handle], effects::Execution::Fused));
                if effect.program.resolution==layer_core::EffectResolution::Native
                    && effect.program.alpha==layer_core::EffectAlpha::Filter && !effect.program.fusion_boundary() {
                    result.push((vec![handle],effects::Execution::Reduced));
                    result.push((vec![handle],effects::Execution::ReducedContribution));
                }
            }
        }
    }
    for parent in parents {
        let mut siblings = scene.members(parent).rev().filter(|handle| scene.occurrence(*handle).unwrap().is_artwork() && scene.visible(*handle)).peekable();
        while let Some(handle) = siblings.next() {
            if !scene.visible(handle) || !direct_effect_mask(scene, handle) || !fusable_adjustment(scene, handle) { continue; }
            let mut chain = vec![handle];
            while let Some(next) = siblings.peek().filter(|next| fuses_after(scene, handle, **next, chain.len())) {
                chain.push(*next); siblings.next();
            }
            if chain.len() > 1 { result.push((chain, effects::Execution::Fused)); }
        }
    }
    result
}

/// Bind groups by the views they bind, kept while the current or previous
/// frame uses them: a drag redraws the same pages every frame. A scene that
/// never begins frames keeps at most BINDINGS entries.
const BINDINGS: usize = 4096;
struct RecentBindings<K> {
    frame: u64,
    entries: std::collections::HashMap<K, (u64, wgpu::BindGroup)>,
}
impl<K> Default for RecentBindings<K> {
    fn default() -> Self {
        Self { frame: 0, entries: Default::default() }
    }
}
#[allow(clippy::mutable_key_type)] // Texture views hash by stable resource identity.
impl<K: std::hash::Hash + Eq + Clone> RecentBindings<K> {
    fn forget(&mut self, matches: impl Fn(&K) -> bool) {
        self.entries.retain(|key, _| !matches(key));
    }
    fn begin_frame(&mut self) {
        self.frame += 1;
        let frame = self.frame;
        self.entries.retain(|_, (used, _)| frame - *used <= 1);
    }
    fn get(&mut self, key: &K, create: impl FnOnce() -> wgpu::BindGroup) -> &wgpu::BindGroup {
        if self.entries.len() >= BINDINGS && !self.entries.contains_key(key) {
            self.frame += 1;
            self.entries.clear();
        }
        let frame = self.frame;
        let entry = self.entries.entry(key.clone()).or_insert_with(|| (frame, create()));
        entry.0 = frame;
        &entry.1
    }
}

fn compute_source_binding(r: &WgpuRasterizer, layout: &wgpu::BindGroupLayout, sources: &[wgpu::TextureView; 3]) -> wgpu::BindGroup {
    crate::bindings::group(&r.device, "scene normal stack inputs", layout, [
        wgpu::BindingResource::TextureView(&sources[0]),
        wgpu::BindingResource::TextureView(&sources[1]),
        wgpu::BindingResource::Sampler(&r.sampler),
        wgpu::BindingResource::TextureView(&sources[2]),
    ])
}

fn source_binding(r: &WgpuRasterizer, layout: &wgpu::BindGroupLayout, sources: &[wgpu::TextureView; 3], extra:bool) -> wgpu::BindGroup {
    let entries=[wgpu::BindGroupEntry {binding:0,resource:wgpu::BindingResource::TextureView(&sources[0])},
        wgpu::BindGroupEntry {binding:1,resource:wgpu::BindingResource::TextureView(&sources[1])},
        wgpu::BindGroupEntry {binding:2,resource:wgpu::BindingResource::Sampler(&r.sampler)},
        wgpu::BindGroupEntry {binding:3,resource:wgpu::BindingResource::TextureView(&sources[2])}];
    r.device.create_bind_group(&wgpu::BindGroupDescriptor {label:Some("scene tile inputs"),layout,entries:&entries[..3+usize::from(extra)]})
}

fn query_grid_size([x,y,w,h]:[u32;4])->[u32;2] {
    [(x,w),(y,h)].map(|(origin,extent)| (0..extent.min(256)).filter(|i| {
        let p=(2*i+1)*extent/(2*extent.min(256));p>=origin && p<origin+PAGE_SIZE
    }).count() as u32)
}

#[cfg(test)]
#[path="scene/display_backup_tests.rs"]
mod display_backup_tests;
