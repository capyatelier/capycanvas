//! Tiled layer composition with explicit cached image boundaries. Pointwise
//! scratch follows nesting depth. Masks never download or rewrite paint.
use super::*;
#[path = "scene_images.rs"]
mod images;
#[path = "filter_previews.rs"]
mod previews;
mod metadata;
mod stack;
pub(crate) mod windows;
pub(super) use previews::FilterPreviews;
pub(super) mod sources;
mod placement;
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

#[derive(Clone)]
enum Job {
    Positions(Arc<paint_transform::mesh::MeshGeometry>, [u32; 2], [i32;2]),
    Placement(Box<placement::PlacementJob>),
    Reduce { binding: wgpu::BindGroup, values: [u32; 20], size: [u32; 2] },
    DecodedTile(std::sync::Arc<sources::PendingTile>),
    Effect {
        target: wgpu::TextureView,
        sources: [wgpu::TextureView; 3],
        data: [f32; 32],
        prepared: effects::PreparedEffect,
        // Keep ordinary draw jobs small: only effects carry the mask inputs.
        masks: Box<[wgpu::TextureView; effects::MASK_SLOTS]>,
    },
    Draw {
        target: wgpu::TextureView,
        sources: [wgpu::TextureView; 3],
        data: [f32; 32],
        over: bool,
        clip: Option<PixelRect>,
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
pub(super) enum Output { Artwork(Option<LayerId>), EffectInput(LayerId), EffectChannels(LayerId), LayerContent(LayerId), Display }

pub(super) struct Scene {
    valid: Arc<std::sync::atomic::AtomicBool>,
    placement: [pixel_transform::PixelTransform; 2],
    positions: paint_transform::mesh::Positions,
    display_mesh: paint_transform::MeshBuffers,
    position_key: Option<(layer_core::LayerPlacement, [u32; 2], [i32;2])>,
    mesh_geometry: std::cell::RefCell<Vec<(layer_core::LayerPlacement, Arc<paint_transform::mesh::MeshGeometry>)>>,
    material_coordinates: wgpu::BindGroup,
    material_pages: std::collections::VecDeque<placement::MaterialPage>,
    material_bounds: std::collections::HashMap<LayerId, placement::MaterialBounds>,
    placement_display: bool,
    scale_sources: scale::Sources,
    scale_commands: Option<scale::Commands>,
    pool: Vec<PageSurface>,
    used: Vec<bool>,
    jobs: Vec<Job>,
    source_jobs: Vec<std::ops::Range<usize>>,
    // Retain table capacity across tile batches, but release resource handles
    // after encoding so these tables cannot pin evicted paint/source pages.
    source_bindings: RecentBindings<[wgpu::TextureView; 3]>,
    compute_bindings: RecentBindings<[wgpu::TextureView; 3]>,
    output_bindings: RecentBindings<wgpu::TextureView>,
    watercolor_outputs: RecentBindings<wgpu::TextureView>,
    mask_bindings: std::collections::HashMap<[wgpu::TextureView; effects::MASK_SLOTS], wgpu::BindGroup>,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::BindGroupLayout,
    buffer: wgpu::Buffer,
    binding: wgpu::BindGroup,
    reduction_binding: wgpu::BindGroup,
    stride: usize,
    capacity: usize,
    record_count: usize,
    upload: Vec<u8>,
    pipeline: [Deferred<wgpu::RenderPipeline>; 2],
    pub(super) effects: effects::Effects,
    pub effect_passes: u64,
    images: images::ImageStages,
    image_window: Option<PixelRect>,
    stop_before: Option<(usize, bool)>,
}

/// Immutable device resources, compiled before input is enabled and shared by
/// live composition, captures and recreated scenes. No canvas pixels retained.
#[derive(Clone)]
pub(super) struct Pipelines {
    pub scale: scale::Pipelines,
    pub resample: resample::Resample,
    uniforms: wgpu::BindGroupLayout,
    layout: wgpu::BindGroupLayout,
    pub pipeline: [Deferred<wgpu::RenderPipeline>; 2],
    pub source: sources::Pipelines,
    pub constant: (wgpu::BindGroupLayout, wgpu::BindGroupLayout, Deferred<wgpu::ComputePipeline>),
}

impl Scene {
    fn begin_write(&mut self, r: &WgpuRasterizer) -> crate::submission::CacheWrite {
        if !self.valid.load(std::sync::atomic::Ordering::Acquire) { *self = Self::new(r); }
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
    pub fn placement_cache(&self, id: LayerId) -> Option<(wgpu::Texture, u64, u32)> {
        self.scale_sources.cache_info(id)
    }
    #[cfg(test)]
    pub fn placement_cache_at(&self, id: LayerId, level: u32) -> Option<(wgpu::Texture, u64, u32)> {
        self.scale_sources.cache_info_at(id, level)
    }
    pub fn scratch_bytes(&self) -> u64 {
        self.pool.iter().map(PageSurface::storage_bytes).sum::<u64>()
            + (self.capacity * self.stride) as u64
            + self.effects.storage_bytes() + self.positions.storage_bytes() + self.display_mesh.storage_bytes()
            + self.placement.iter().map(pixel_transform::PixelTransform::storage_bytes).sum::<u64>() + 32
            + self.images.storage_bytes() + self.scale_sources.storage_bytes() + self.scale_commands.as_ref().map_or(0, scale::Commands::storage_bytes)
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
    pub fn initialize_source_paint(&mut self, r: &mut WgpuRasterizer, layers: &[Layer], encoder: &mut crate::submission::CommandEncoder) -> Result<(), GpuRasterError> {
        self.jobs.clear();self.source_jobs.clear();
        for layer in layers {
            if layer.source.is_none() && r.native_backing(layer.id).is_none() {
                continue;
            }
            let pages: Vec<_> = r.paint_layers.iter().filter(|p| p.id == layer.id)
                .flat_map(|p| p.pages.iter().filter(|p| p.primary_needs_clear)
                    .map(|p| (p.coordinate, p.primary.view.clone()))).collect();
            self.copy_source_pages(r, layer, pages)?;
        }
        self.encode_jobs(r, encoder)
    }
    pub fn initialize_source_preview(
        &mut self,
        r: &mut WgpuRasterizer,
        layer: &Layer,
        damage: PixelRect,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        self.jobs.clear();self.source_jobs.clear();
        let stored = r.paint_layers.iter().find(|p| p.id == layer.id);
        let pages: Vec<_> = r
            .preview_pages
            .iter()
            .filter(|p| {
                !page_rect(p.coordinate).intersect(damage).is_empty()
                    && !stored.is_some_and(|l| l.pages.iter().any(|s| s.coordinate == p.coordinate))
            })
            .map(|p| (p.coordinate, p.primary.view.clone()))
            .collect();
        self.copy_source_pages(r, layer, pages)?;
        self.encode_jobs(r, encoder)
    }
    fn copy_source_pages(&mut self, r: &mut WgpuRasterizer, layer: &Layer, pages: Vec<([u32; 2], wgpu::TextureView)>) -> Result<(), GpuRasterError> {
        for (coordinate, target) in pages {
            let Some(view) = self.source_tile(r, layer, coordinate)? else { continue };
            let mut data = [0.; 32];
            data[..10].copy_from_slice(&[0., 0., 256., 256., 256., 256., 0., 0., 1., 1.]);
            self.jobs.push(Job::Draw { target, sources: [view, r.empty_view.clone(), r.empty_view.clone()], data, over: false, clip: None });
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
    pub(crate) fn geometry_bytes(layers: &[Layer], scene: Option<&Self>) -> u64 {
        if let Some(scene)=scene {
            scene.mesh_geometry.borrow_mut().retain(|(key,_)|layers.iter().any(|layer|layer.properties.placement.mesh.is_some()
                && layer_core::target_geometry(layers,layer.id).placement==*key));
        }
        let buffers = layers.iter().filter(|layer|layer.properties.placement.mesh.is_some()).map(|layer| {
            let geometry=layer_core::target_geometry(layers,layer.id);
            scene.map_or_else(||paint_transform::mesh::MeshGeometry::new(geometry.placement.mesh.as_ref().unwrap(),geometry.placement.outer,None).buffer_bytes(),
                |scene|scene.mesh_geometry(&geometry).unwrap().buffer_bytes())
        }).max().unwrap_or(0);
        if buffers == 0 { 0 } else { buffers.saturating_mul(2) + u64::from(PAGE_SIZE+2).pow(2)*16 + 48 }
    }
    pub fn begin_frame(&mut self) {
        self.clear_material_pages();
        for pass in &mut self.placement { pass.begin_frame(); }
        self.source_bindings.begin_frame();
        self.compute_bindings.begin_frame();
        self.output_bindings.begin_frame();
        self.watercolor_outputs.begin_frame();
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
        let effects = effects::Effects::new(r, &uniforms, &layout);
        use wgpu::util::DeviceExt;
        let coordinates = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("placed material neighborhood"),
            contents: target_bytes(&TargetGpu::new([PAGE_SIZE; 2], [PAGE_SIZE; 2], [PAGE_SIZE * 3; 2])),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        Self {
            valid: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            placement_display: false,
            positions: r.transforms.as_ref().map_or_else(|| paint_transform::mesh::Positions::new(device), paint_transform::mesh::Positions::sharing),
            position_key: None,
            mesh_geometry: Default::default(),
            display_mesh: Default::default(),
            scale_sources: Default::default(),
            scale_commands: None,
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
            stop_before: None,
        }
    }
    fn forget_bindings(&mut self, retired: &[wgpu::TextureView]) {
        if retired.is_empty() { return; }
        self.source_bindings.forget(|views| views.iter().any(|view| retired.contains(view)));
        self.compute_bindings.forget(|views| views.iter().any(|view| retired.contains(view)));
        self.output_bindings.forget(|view| retired.contains(view));
        self.watercolor_outputs.forget(|view| retired.contains(view));
        self.mask_bindings.retain(|views, _| !views.iter().any(|view| retired.contains(view)));
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
    fn source_tile(&mut self, r: &WgpuRasterizer, layer: &Layer, coordinate: [u32; 2]) -> Result<Option<wgpu::TextureView>, GpuRasterError> {
        let _trace = crate::performance_trace::Span::new(c"capy.source_tile");
        let (tile, pending) = if let Some(blob) = r.native_color_tile(layer.id, coordinate)? {
            let space = r.document_color().space;
            r.source_tiles.borrow_mut().plan_raster(r, &blob, space, space)?
        } else {
            let Some(source) = &layer.source else { return Ok(None); };
            if coordinate[0] >= source.extent[0].div_ceil(PAGE_SIZE) || coordinate[1] >= source.extent[1].div_ceil(PAGE_SIZE) {
                return Ok(None);
            }
            r.source_tiles.borrow_mut().plan(r, source, coordinate)?
        };
        if let Some(pending) = pending { self.enqueue_source_decode(pending); }
        Ok(Some(tile.view))
    }

    /// Consume a bounded group before gathering the next one: these textures
    /// belong to the fixed, queue-ordered source cache.
    pub fn layer_tile_for_query(&mut self, r: &mut WgpuRasterizer, layers: &[Layer], id: LayerId,
        coordinate: [u32; 2], encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<crate::source_access::RawTile, GpuRasterError> {
        debug_assert!(self.jobs.is_empty());
        self.used.fill(false);
        self.begin_frame();
        let layer = layers.iter().find(|layer| layer.id == id || layer.mask.as_ref().is_some_and(|m| m.id == id))
            .ok_or(GpuRasterError::MissingPaintLayer(id))?;
        let geometry = layer_core::target_geometry(layers,id);
        let extent = layer.local_extent(r.document_extent);
        let page = if layer.id == id {
            self.placed_raw_plane(r, layer, geometry, coordinate, layer_core::raster::RasterPlane::Color, extent)?
        } else {
            let mask = layer.mask.as_ref().unwrap();
            self.mask_at(r, mask, geometry, mask.local_extent(extent), coordinate)?
        };
        self.encode_jobs(r,encoder)?;
        Ok(crate::source_access::RawTile { texture:self.pool[page].texture.clone(), view:self.pool[page].view.clone() })
    }

    pub fn source_tile_for_query(
        &mut self,
        r: &mut WgpuRasterizer,
        source: &std::sync::Arc<layer_core::color::source::SourceImage>,
        coordinate: [u32; 2],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<crate::source_access::RawTile, GpuRasterError> {
        debug_assert!(self.jobs.is_empty());
        let (tile, pending) = r.source_tiles.borrow_mut().plan(r, source, coordinate)?;
        self.encode_decode(r, pending, encoder)?;
        Ok(tile)
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
        layer: &Layer,
        stored: Option<&PaintLayer>,
        c: [u32; 2],
        out: usize,
        rect: [f32; 4],
        convert: Convert,
    ) -> Result<(), GpuRasterError> {
        let preview = r.preview_layer_id == Some(layer.id)
            && !r.preview_damage.intersect(page_rect(c)).is_empty();
        let wet_nearby = stored.is_some_and(|stored| r.watercolor_style(layer.id, packet.dab_batches).is_some()
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
            let binding = self.watercolor_binding(r, layer, stored.unwrap(), c, preview)?;
            // Aligned pages already cover the layer tile. Only a translated
            // or converted page needs an intermediate and placement.
            let page = if rect == [0., 0., 256., 256.] && convert == Convert::None {
                out
            } else {
                self.alloc(r, wgpu::Color::TRANSPARENT)
            };
            self.jobs.push(Job::Watercolor {
                coordinates: (r.target_bind_group.clone(), r.layer_target_offset(layer.id, c)),
                target: self.pool[page].view.clone(),
                binding,
                record: *r.layer_style_records.get(&layer.id).ok_or(GpuRasterError::MissingPaintLayer(layer.id))?,
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
            let [base, flow] = self.color_inputs(r, layer, stored, c, preview)?.map(|input| input.map(|i| i.view));
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
    fn color_inputs(&mut self, r: &WgpuRasterizer, layer: &Layer, stored: Option<&PaintLayer>, c: [u32; 2], preview: bool) -> Result<[Option<ColorInput>; 2], GpuRasterError> {
        let persistent = stored.and_then(|s| s.pages.iter().find(|p| p.coordinate == c));
        let predicted = preview.then(|| r.preview_page(c)).flatten();
        let base = if let Some(p) = predicted.filter(|_| r.preview_requires_base).or(persistent) {
            Some(ColorInput { view: p.active().view.clone(), lease: None })
        } else {
            self.source_tile(r, layer, c)?.map(|view| {
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
        layer: &Layer,
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
                    view = self.source_tile(r, layer, neighbor)?;
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
        indices: &[usize],
        tile: [u32; 2],
        input: usize,
    ) -> Result<usize, GpuRasterError> {
        let layers: Vec<_> = indices.iter().map(|i| &packet.layers[*i]).collect();
        let layer = layers[0];
        if layer.effect.as_ref().unwrap().program.image_boundary() {
            let view = self
                .images
                .output(layer.id)
                .ok_or_else(|| GpuRasterError::Effect("Missing image effect stage".into()))?;
            let out = self.image_tile(r, view, self.images.bounds, tile);
            self.free(input);
            return Ok(out);
        }
        let prepared =
            self.effects
                .prepare(r, &layers, effects::Execution::Fused, packet.time_seconds, 0, packet.blend_space)?;
        let mask =
            if indices.len() == 1 && !direct_effect_mask(packet.layers, layer) {
                layer.mask.as_ref().filter(|m| m.enabled).map(|m| {
                    self.mask_at(r, m, layer_core::target_geometry(packet.layers, m.id), m.local_extent(layer.local_extent(packet.document_extent)), tile)
                }).transpose()?
            } else {
                None
            };
        let out = self.reserve(r);
        let mut data = [0.; 32];
        data[..4].copy_from_slice(&[0., 0., 256., 256.]);
        data[4..6].copy_from_slice(&[256., 256.]);
        data[11] = f32::from(mask.is_some());
        let mut masks = Box::new(std::array::from_fn(|_| r.empty_view.clone()));
        let mut present = 0u32;
        let mut inverted = 0u32;
        if mask.is_none() {
            for (i, l) in layers.iter().enumerate().take(effects::MASK_SLOTS) {
                if let Some(m) = l.mask.as_ref().filter(|m| m.enabled)
                    && let Some(page) = r.layer_masks.pages.get(&(m.id, tile))
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
                    ..
                },
            ) = (&self.jobs[n - 2], &self.jobs[n - 1])
                && *clear == self.pool[input].view
                && target == clear
                && paint_data[..4] == [0., 0., 256., 256.]
                && paint_data[31] == Convert::layers(packet).code()
                && ((*over && (paint_data[8] == 7. || paint_data[8] == 1.))
                    || (!*over && paint_data[8] == 13.))
            {
                sources = paint.clone();
                data[16..20].copy_from_slice(&[
                    paint_data[9],
                    if paint_data[8] == 1. {
                        1.
                    } else {
                        paint_data[10]
                    },
                    paint_data[11],
                    1.,
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
            masks,
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
        // A normal draw over a constant clear supplies its own backdrop. Lower
        // this once when creating the job, for both tiled and direct composition.
        if over && options[0] == 7.
            && let Some(Job::Clear(clear, color)) = self.jobs.last()
            && *clear == self.pool[target].view
        {
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
                data: prior, over: false, clip: None }) = self.jobs.last()
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
            clip: None,
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
            && matches!(data[8] as u32, 13 | 15) && data[7] == 0.
        {
            data[7] = convert.code();
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
        mask: &layer_core::LayerMask,
        offset: layer_core::Point,
        tile: [u32; 2],
    ) -> usize {
        let default = if mask.inverted {
            1. - mask.default_coverage
        } else {
            mask.default_coverage
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
        for ((id, c), page) in &r.layer_masks.pages {
            if *id != mask.id {
                continue;
            }
            let rect = local_rect(*c, offset, tile);
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
        index: usize,
        tile: [u32; 2],
    ) -> Result<usize, GpuRasterError> {
        let layer = &packet.layers[index];
        let out = if layer.kind == LayerKind::Group {
            self.group(r, packet, Some(layer.id), tile)?
        } else if layer.kind == LayerKind::Effect {
            let input = self.alloc(r, wgpu::Color::TRANSPARENT);
            // Generator coverage is applied below with ordinary layer masks.
            self.effect(r, packet, &[index], tile, input)?
        } else {
            let out = self.paint_tile(r, packet, index, tile, scale::placement_level(packet.layers, layer.id))?;
            self.converted(r, out, Convert::layers(packet))
        };
        if let Some(mask) = layer.mask.as_ref().filter(|m| m.enabled) {
            let m = self.mask_at(r, mask, layer_core::target_geometry(packet.layers, mask.id), mask.local_extent(layer.local_extent(packet.document_extent)), tile)?;
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
    fn paint_tile(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, index: usize, tile: [u32; 2], source_level: u32) -> Result<usize, GpuRasterError> {
        let layer = &packet.layers[index];
        if layer.properties.placement != layer_core::LayerPlacement::IDENTITY
            || (world_offset(packet.layers, layer.id, false) != layer_core::Point::default()
                && r.watercolor_style(layer.id, packet.dab_batches).is_some()) {
            return self.placed_tile(r, packet, index, tile, source_level);
        }
        let out = self.alloc(r, wgpu::Color::TRANSPARENT);
        let offset = world_offset(packet.layers, layer.id, false);
        let stored = r.paint_layers.iter().find(|l| l.id == layer.id);
        if stored.is_some() || layer.source.is_some() {
            // A translated output tile intersects at most four native
            // source tiles. Watercolor samples its halo from their bindings;
            // never scan/expand every page in the layer for every output tile.
            let origin = layer_core::Point {
                x: (tile[0] * PAGE_SIZE) as f32 - offset.x,
                y: (tile[1] * PAGE_SIZE) as f32 - offset.y,
            };
            let region = pixel_rect(
                layer_core::Rect {
                    min: origin,
                    max: layer_core::Point {
                        x: origin.x + PAGE_SIZE as f32,
                        y: origin.y + PAGE_SIZE as f32,
                    },
                },
                layer.local_extent(r.document_extent),
            );
            for c in page_coordinates(region) {
                let rect = local_rect(c, offset, tile);
                if !intersects(rect) {
                    continue;
                }
                self.paint_page(r, packet, layer, stored, c, out, rect, Convert::None)?;
            }
        }
        Ok(out)
    }
    fn group(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        parent: Option<LayerId>,
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
        index: usize,
        tile: [u32; 2],
        target: usize,
    ) -> Result<bool, GpuRasterError> {
        let layer = &packet.layers[index];
        if layer.kind != LayerKind::Paint
            || layer.properties.blend != layer_core::LayerBlend::Normal
        {
            return Ok(false);
        }
        let mask = layer.mask.as_ref().filter(|m| m.enabled);
        if mask.is_none() && self.placement_display
            && let Some(affine) = layer_core::target_geometry(packet.layers, layer.id).as_affine()
            && r.watercolor_style(layer.id, packet.dab_batches).is_none()
            && scale::placement_level(packet.layers, layer.id) > 0
            && let Some((plan, view)) = self.scale_sources.sample(layer.id, scale::placement_level(packet.layers, layer.id))
        {
            let scale = (1 << plan.level) as f32;
            let transform = layer_core::Affine([scale, 0., 0., scale, plan.bounds.min_x() as f32, plan.bounds.min_y() as f32])
                .then(affine);
            let inverse = transform.inverse().ok_or(GpuRasterError::InvalidTransform(
                "Transform must be finite and invertible"))?.0;
            let view = view.clone();
            self.draw(r, target, view, None, [0., 0., 256., 256.],
                [12., layer.opacity, 0., 0.], true, Convert::layers(packet));
            let Some(Job::Draw { data, .. }) = self.jobs.last_mut() else { unreachable!() };
            data[12..14].copy_from_slice(&tile.map(|n| (n * PAGE_SIZE) as f32));
            data[24..28].copy_from_slice(&inverse[..4]);
            data[28..30].copy_from_slice(&inverse[4..]);
            return Ok(true);
        }
        if layer.properties.placement != layer_core::LayerPlacement::IDENTITY
            || world_offset(packet.layers, layer.id, false) != layer_core::Point::default()
        {
            return Ok(false);
        }
        if mask.is_some_and(|m| !layer_core::target_geometry(packet.layers, m.id).is_identity())
        {
            return Ok(false);
        }
        let Some(stored) = r.paint_layers.iter().find(|l| l.id == layer.id) else {
            return Ok(true);
        };
        if r.watercolor_style(layer.id, packet.dab_batches).is_some() {
            return Ok(false);
        }
        // Destination-reading previews already contain the complete layer
        // tile. Composite them directly just like persistent paint, instead
        // of allocating a scratch layer and blending it in another pass.
        let predicted = (r.preview_layer_id == Some(layer.id))
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
            let base = if let Some(page) = stored.pages.iter().find(|p| p.coordinate == tile) {
                Some(page.active().view.clone())
            } else {
                self.source_tile(r, layer, tile)?
            };
            self.draw(
                r,
                target,
                preview.active().view.clone(),
                base,
                [0., 0., 256., 256.],
                [14., layer.opacity, 0., 0.],
                true,
                Convert::layers(packet),
            );
            return Ok(true);
        }
        let view = if let Some(page) = predicted.or_else(|| stored.pages.iter().find(|p| p.coordinate == tile)) {
            page.active().view.clone()
        } else if let Some(view) = self.source_tile(r, layer, tile)? {
            view
        } else {
            return Ok(true);
        };
        let default = mask.map_or(1., |m| {
            if m.inverted {
                1. - m.default_coverage
            } else {
                m.default_coverage
            }
        });
        let source = mask.and_then(|m| r.layer_masks.pages.get(&(m.id, tile)));
        self.draw(
            r,
            target,
            view,
            source.map(|p| p.view.clone()),
            [0., 0., 256., 256.],
            [
                7.,
                layer.opacity,
                default,
                source.map_or(0., |_| 2. + f32::from(mask.unwrap().inverted)),
            ],
            true,
            Convert::layers(packet),
        );
        Ok(true)
    }
    /// Run a pending paint operation over `damage`, the batch's pages. Masks
    /// and erases that cover the whole layer settle watercolor and wet
    /// material first, so hidden pigment cannot bring erased content back.
    pub fn apply_operation(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        layer_index: usize,
        operation_index: usize,
        damage: PixelRect,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        use layer_core::LayerOperationKind;
        self.jobs.clear();self.source_jobs.clear();
        self.clear_material_pages();
        self.used.fill(false);
        let layer = &packet.layers[layer_index];
        let op = &layer.pending_operations[operation_index];
        let extent = layer.local_extent(packet.document_extent);
        if matches!(op.kind, LayerOperationKind::Erase { alpha_locked: true }) {
            return Ok(());
        }
        if let LayerOperationKind::Bake { members, offset } = &op.kind {
            return self.bake(r, packet, layer, members, *offset, damage, &op.coverage, None, encoder);
        }
        if let LayerOperationKind::FrequencyDetail { members, offset, low } = &op.kind {
            let low = packet.layers.iter().find(|layer| layer.id == *low).ok_or(GpuRasterError::MissingPaintLayer(*low))?;
            return self.bake(r, packet, layer, members, *offset, damage, &op.coverage, Some(low), encoder);
        }
        let Some(stored) = r.paint_layers.iter().find(|l| l.id == layer.id) else {
            return Ok(());
        };
        let settles = match op.kind {
            LayerOperationKind::ApplyMask => true,
            LayerOperationKind::Erase { .. } => damage == PixelRect::full(extent),
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
        let erase = f32::from(matches!(op.kind, LayerOperationKind::Erase { .. }));
        for (c, source, destination) in pages {
            let mask = self.mask_at(r, &op.coverage, layer_core::ImageTransform { placement: layer_core::LayerPlacement::from_projective(op.coverage.placement.then(layer_core::Projective::from_affine(layer_core::Affine::translation(op.coverage.offset))).ok_or(GpuRasterError::InvalidTransform("Invalid mask placement"))?), ..Default::default() }, layer.local_extent(packet.document_extent), c)?;
            let out = self.alloc(r, wgpu::Color::TRANSPARENT);
            match op.kind {
                LayerOperationKind::Transform(_) | LayerOperationKind::Bake { .. } | LayerOperationKind::FrequencyDetail { .. } => {
                    unreachable!("transforms and bakes run before page operations")
                }
                LayerOperationKind::ApplyMask | LayerOperationKind::Erase { .. } => {
                    let mut resolved = None;
                    if watercolor {
                        let binding = self.watercolor_binding(r, layer, stored, c, false)?;
                        let p = self.alloc(r, wgpu::Color::TRANSPARENT);
                        self.jobs.push(Job::Watercolor {
                            coordinates: (r.target_bind_group.clone(), r.layer_target_offset(layer.id, c)),
                            target: self.pool[p].view.clone(),
                            binding,
                            record: *r.layer_style_records.get(&layer.id).ok_or(GpuRasterError::MissingPaintLayer(layer.id))?,
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
                LayerOperationKind::Fill { .. }
                | LayerOperationKind::Gradient { .. }
                | LayerOperationKind::Figure(_) => {
                    let (colors, endpoints, options) = match op.kind {
                        LayerOperationKind::Fill {
                            color,
                            alpha_locked,
                        } => ([color; 2], [0.0; 4], [6., 1., 0., f32::from(alpha_locked)]),
                        LayerOperationKind::Gradient {
                            start,
                            end,
                            colors,
                            radial,
                            alpha_locked,
                        } => (
                            colors,
                            [start.x, start.y, end.x, end.y],
                            [6., 1., f32::from(radial), f32::from(alpha_locked)],
                        ),
                        LayerOperationKind::Figure(ref f) => (
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
                    if let Some(Job::Draw { data, .. }) = self.jobs.last_mut() {
                        data[6..8].copy_from_slice(&c.map(|v| (v * PAGE_SIZE) as f32));
                        data[12..16].copy_from_slice(&colors[0]);
                        data[16..20].copy_from_slice(&colors[1]);
                        data[20..24].copy_from_slice(&endpoints);
                        data[24..30].copy_from_slice(&op.placement.inverse().ok_or(GpuRasterError::InvalidTransform("Invalid paint operation placement"))?.0);
                    }
                }
            }
            let written = if matches!(op.kind, LayerOperationKind::Fill { .. } | LayerOperationKind::Gradient { .. } | LayerOperationKind::Figure(_)) {
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
            if let Some(stored) = r.paint_layers.iter().find(|l| l.id == layer.id) {
                for p in &stored.watercolor_wetness_pages {
                    r.encode_clear(encoder, &p.primary.view, "clear baked wetness");
                    r.encode_clear(encoder, &p.secondary.view, "clear baked wetness companion");
                }
                for p in &stored.material_pages {
                    r.encode_clear(encoder, &p.wetness.view, "clear baked material");
                }
                for p in &stored.coverage_pages {
                    r.encode_clear(encoder, &p.primary.view, "clear baked stroke coverage");
                    r.encode_clear(encoder, &p.secondary.view, "clear baked coverage companion");
                }
            }
            if let Some(stored) = r.paint_layers.iter_mut().find(|l| l.id == layer.id) {
                stored.watercolor = None;
            }
        }
        Ok(())
    }

    pub(super) fn capture_window(layers: &[Layer], region: PixelRect, extent: [u32; 2]) -> PixelRect {
        images::capture_window(layers, region, extent)
    }
    pub(super) fn release_capture_window(&mut self, window: PixelRect) {
        // The snapshot owner has completed its previous readback. Release job
        // references and old image windows before restoring the next inputs.
        self.jobs.clear();self.source_jobs.clear();
        if self.images.bounds != window {
            self.retire_images(|scene| scene.images = images::ImageStages::default());
        }
    }
    pub(super) fn capture_image_bound(layers: &[Layer], window: PixelRect) -> u64 {
        let mut images = 0u64;
        let mut scratch = 0;
        for layer in layers {
            if !images::visible(layers, layer) { continue; }
            if let Some(effect) = &layer.effect && effect.program.image_boundary() {
                images += 2 + u64::from(layer.mask.as_ref().is_some_and(|m| m.enabled))
                    + if layer.properties.clipped { 2 } else { 0 };
                scratch = scratch.max(effect.program.passes.len().saturating_sub(1).min(2) as u64);
            }
        }
        window.area().saturating_mul(16).saturating_mul(images + scratch)
    }

    /// Capture a document-coordinate crop. Neighborhood dependencies share one
    /// conservative window expanded by every visible spatial pass. A global
    /// dependency retains its full input; it must never silently sample a crop.
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

    pub(super) fn capture_query_region(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, destination: &wgpu::Texture,
        region: PixelRect, output: Output, preview: bool, encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        if region.is_empty() || region.intersect(PixelRect::full(packet.document_extent)) != region
            || destination.width() < region.width() || destination.height() < region.height()
        {
            return Err(GpuRasterError::InvalidExtent);
        }
        let window = images::capture_window(packet.layers, region, packet.document_extent);
        let dirty = match output { Output::Display => PixelRect::EMPTY, _ => window };
        self.prepare_region(r, packet, window, dirty, encoder)?;
        let destination = Image { texture: destination.clone(), view: destination.create_view(&Default::default()),
            plan: display_mips::Plan::window(packet.document_extent, 0, region) };
        self.capture_prepared_regions(r, packet, &destination, &[region], output, preview, encoder)
    }

    fn prepare_region(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, window: PixelRect,
        dirty: PixelRect, encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        let write = self.begin_write(r);
        if self.placement_display { self.retire_images(|scene| scene.images = images::ImageStages::default()); }
        self.placement_display = false;
        if let Some(mut transforms) = r.transforms.take() {
            let result = transforms.materialize_region(r, encoder, packet.layers, window);
            r.transforms = Some(transforms);
            result?;
        }
        self.begin_frame();
        self.image_window = Some(window);
        self.effects.retain(packet.layers);
        let result = self.update_images(r, packet, dirty, encoder);
        self.stop_before = None;
        if result.is_ok() { write.track(encoder); }
        result.map(|_| ())
    }

    pub(super) async fn capture_query_tiles(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, region: PixelRect, output: Output, preview:bool, tiles_per_submission:u32,
        encoder: &mut crate::submission::CommandEncoder,
        mut consume: impl FnMut(&mut WgpuRasterizer, &wgpu::TextureView, PixelRect, &mut crate::submission::CommandEncoder) -> Result<(), GpuRasterError>,
    ) -> Result<(), GpuRasterError> {
        let window = images::capture_window(packet.layers, region, packet.document_extent);
        self.prepare_region(r, packet, window, window, encoder)?;
        let (texture, view) = create_color_target(&r.device, [PAGE_SIZE;2], "statistics tile");
        let mut tiles=0;
        for tile in page_coordinates(region) {
            let grid = [tile[0]*PAGE_SIZE,tile[1]*PAGE_SIZE,packet.document_extent[0],packet.document_extent[1]];
            if preview && query_grid_size(grid).contains(&0) {continue;}
            let region = page_rect(tile).intersect(region);
            let destination = Image {texture:texture.clone(),view:view.clone(),plan:display_mips::Plan::window(packet.document_extent,0,region)};
            self.capture_prepared_regions(r, packet, &destination, &[region], output, preview, encoder)?;
            consume(r, &view, region, encoder)?;
            tiles+=1;
            if !preview && tiles%tiles_per_submission==0 {
                r.uploads.finish(encoder);
                std::mem::replace(encoder, crate::submission::CommandEncoder::new(&r.device, &Default::default())).submit(&r.queue);
                crate::local_tone::wait_async(&r.device, &r.queue).await.map_err(GpuRasterError::Color)?;
            }
        }
        Ok(())
    }

    fn capture_prepared_region(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, destination: &Image,
        region: PixelRect, output: Output, encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        self.capture_prepared_regions(r, packet, destination, &[region], output, false, encoder)
    }

    fn capture_prepared_regions(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, destination: &Image,
        regions: &[PixelRect], output: Output, preview: bool, encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        self.jobs.clear();self.source_jobs.clear();
        self.clear_material_pages();
        self.used.fill(false);
        self.stop_before = None;
        for tile in regions.iter().flat_map(|r| page_coordinates(*r)) {
            let image = match output {
                Output::Artwork(parent) => {
                    let image = self.group(r, packet, parent, tile)?;
                    self.converted(r, image, Convert::linear(packet))
                }
                Output::EffectInput(id) | Output::EffectChannels(id) => {
                    let index = packet.layers.iter().position(|layer| layer.id == id).ok_or(GpuRasterError::InvalidExtent)?;
                    let layer = &packet.layers[index];
                    self.stop_before = Some((index, layer.properties.clipped));
                    let image = self.group(r, packet, images::input_scope(packet.layers, layer), tile)?;
                    self.stop_before = None;
                    let image = if matches!(output,Output::EffectChannels(_)) {self.effect(r,packet,&[index],tile,image)?} else {image};
                    self.converted(r, image, Convert::linear(packet))
                }
                Output::LayerContent(id) => {
                    let index = packet.layers.iter().position(|layer| layer.id == id).ok_or(GpuRasterError::InvalidExtent)?;
                    self.paint_tile(r, packet, index, tile, 0)?
                }
                Output::Display => self.display_tile(r, packet, tile)?,
            };
            self.copy_window_tile(image, destination, tile);
            if preview { self.encode_query_jobs(r, encoder, Some([tile[0]*PAGE_SIZE,tile[1]*PAGE_SIZE,packet.document_extent[0],packet.document_extent[1]]))?; }
        }
        self.encode_jobs(r, encoder)
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
        self.scale_sources.prepare(r, packet, batch_tiles);
        let mut commands = self.scale_commands.take().unwrap_or_else(|| scale::Commands::new(r));
        commands.begin();
        let mut cache = r.scale_display.take().expect("prepared display cache");
        cache.submission_valid = Some(self.valid.clone());
        let result = (|| {
            cache.prepare_graph(r, packet, self, &commands, tiles.filter(|_| scale::bounded(packet.layers)))?;
            cache.invalidate_hierarchy(&self.scale_sources, dirty, tiles.filter(|_| scale::bounded(packet.layers)));
            if cache.evaluation == scale::Evaluation::Native {
                self.scale_sources.retain_levels(&Default::default(), 0);
                cache.render_native(self, r, packet, dirty, &mut scale::Encoding { encoder, commands: &mut commands }, tiles)
            } else {
                self.retire_images(|scene| scene.images.release_window_pixels());
                self.image_window = None;
                if cache.plan.level > 0 && !scale::bounded(packet.layers) {
                    self.clear_material_pages();
                    self.pool.clear();
                    self.used.clear();
                }
                self.placement_display = true;
                self.effects.retain(packet.layers);
                r.telemetry.phase_begin(8, &r.device, &r.queue, encoder);
                self.prepare_display_sources(&cache, &mut commands, r, packet, encoder)?;
                r.telemetry.phase_end(8, encoder);
                cache.render(self, r, packet, dirty, &mut scale::Encoding { encoder, commands: &mut commands }, tiles)
            }
        })();
        self.scale_commands = Some(commands);
        r.scale_display = Some(cache);
        if result.is_ok() { write.track(encoder); }
        result
    }

    fn display_tile(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, tile: [u32; 2]) -> Result<usize, GpuRasterError> {
        let mut output = self.group(r, packet, None, tile)?;
        for layer in packet.layers {
            if let Some(mask) = layer.mask.as_ref().filter(|m| m.enabled && m.show_area) {
                let m = self.mask_at(r, mask, layer_core::target_geometry(packet.layers, mask.id), mask.local_extent(layer.local_extent(packet.document_extent)), tile)?;
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
        self.mask_bindings.clear();
        if result.is_err() {
            // Dropping unencoded reservations invalidates their source keys.
            self.jobs.clear();self.source_jobs.clear();
        }
        result
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
                Job::Effect { data, .. } if r.capture_frame.is_some() => {
                    let (origin, extent) = r.capture_frame.unwrap();
                    captured = *data;
                    captured[12] += origin[0];
                    captured[13] += origin[1];
                    captured[14..16].copy_from_slice(&extent.map(|n| n as f32));
                    captured[30..32].copy_from_slice(&origin);
                    Some(&captured)
                }
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
            let Job::Draw { data, over: false, clip, .. } = job else { return false; };
            if data[8] == 15. || (data[8] == 13. && clip.is_some()) { return true; }
            if !matches!(data[8] as u32, 1 | 13 | 14 | 17) { return false; }
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
                && (!is_compute(next) || matches!(next, Job::Draw { data, .. }
                    if data[..2] == [0., 0.] && data[2..4] == data[4..6])))
        };
        let is_material_compute = |i: usize| match &self.jobs[i] {
            Job::Placement(_) | Job::Reduce { .. } => true,
            Job::Watercolor { target, .. } => i > 0 && matches!(&self.jobs[i - 1],
                Job::Clear(previous, color) if previous == target && *color == wgpu::Color::TRANSPARENT),
            _ => false,
        };
        let material_clear = |i: usize| matches!(self.jobs[i], Job::Clear(..))
            && i + 1 < self.jobs.len() && is_material_compute(i + 1);
        let source_bindings = &mut self.source_bindings;
        let mask_bindings = &mut self.mask_bindings;
        let output_bindings = &mut self.output_bindings;
        let compute_bindings = &mut self.compute_bindings;
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
                        Job::Reduce { binding, size, .. } => {
                            pass.set_pipeline(&r.scene_pipelines.scale.reduce);
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
                pass.set_pipeline(pipeline);
                for (j, job) in self.jobs.iter().enumerate().take(end).skip(i) {
                    if overwritten_clear(j) { continue; }
                    let Job::Draw { target, sources, data, .. } = job else { unreachable!() };
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
                    pass.dispatch_workgroups(size[0].div_ceil(32), size[1].div_ceil(2), 1);
                }
                encoded_through = end;
                continue;
            }
            match job {
                Job::Placement(_) | Job::Reduce { .. } => unreachable!(),
                Job::Positions(geometry, tile, offset) => {
                    self.positions.upload(r, encoder, geometry)?;
                    self.positions.draw_offset(r, encoder, *tile, *offset)?;
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
                            let binding = source_bindings.get(sources, || source_binding(r, &self.layout, sources));
                            if let Job::Effect {
                                prepared, masks, ..
                            } = job
                            {
                                pass.set_pipeline(&prepared.pipeline);
                                pass.set_bind_group(2, &prepared.binding, &[]);
                                let masks = mask_bindings.entry(masks.as_ref().clone()).or_insert_with(|| {
                                    r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                        label: Some("effect tile masks"),
                                        layout: &self.effects.masks,
                                        entries: &std::array::from_fn::<_, { effects::MASK_SLOTS }, _>(|i| wgpu::BindGroupEntry {
                                                binding: i as u32,
                                                resource: wgpu::BindingResource::TextureView(&masks[i]),
                                            }),
                                    })
                                });
                                pass.set_bind_group(3, &*masks, &[]);
                            } else if let Job::Draw { over, .. } = job {
                                pass.set_pipeline(&self.pipeline[usize::from(*over)]);
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
    pub fn effects(&self, r: &WgpuRasterizer) -> effects::Effects {
        effects::Effects::new(r, &self.uniforms, &self.layout)
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
        ]);
        let shader = Deferred::new({
            let device = device.clone();
            move || {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("layer scene"),
                    source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[&working_color::shader(&device), &crate::view_color::hdr_shader(device.working_space(), layer_core::color::RgbSpace::Srgb), include_str!("blend_modes.wgsl"), include_str!("scene.wgsl"), include_str!("scene_constant.wgsl")])),
                })
            }
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene composition"),
            bind_group_layouts: &[Some(&uniforms), Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = [None, Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING)].map(|blend| {
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
                    device.working_format(),
                    "tile layer composition",
                )
            })
        });
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

fn direct_effect_mask(layers: &[Layer], layer: &Layer) -> bool {
    layer.mask.as_ref().is_none_or(|m| {
        !m.enabled || layer_core::target_geometry(layers, m.id).is_identity()
    })
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
fn local_rect(c: [u32; 2], offset: layer_core::Point, tile: [u32; 2]) -> [f32; 4] {
    [
        (c[0] as f32 - tile[0] as f32) * 256. + offset.x,
        (c[1] as f32 - tile[1] as f32) * 256. + offset.y,
        256.,
        256.,
    ]
}
fn intersects(r: [f32; 4]) -> bool {
    r[0] < 256. && r[1] < 256. && r[0] + r[2] > 0. && r[1] + r[3] > 0.
}
pub(super) fn world_offset(layers: &[Layer], id: LayerId, mask: bool) -> layer_core::Point {
    let Some(layer) = layers.iter().find(|l| l.id == id) else {
        return Default::default();
    };
    let mut offset = if mask {
        layer.mask.as_ref().unwrap().offset
    } else {
        layer.properties.offset
    };
    let mut parent = layer.properties.parent;
    for _ in 0..layers.len() {
        let Some(p) = parent.and_then(|id| layers.iter().find(|l| l.id == id)) else {
            break;
        };
        offset.x += p.properties.offset.x;
        offset.y += p.properties.offset.y;
        parent = p.properties.parent;
    }
    offset
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

fn fusable_adjustment(layer: &Layer) -> bool {
    layer.effect.as_ref().is_some_and(|e| e.program.kind == layer_core::EffectKind::Adjustment && !e.program.image_boundary())
}
fn fuses_after(layers: &[Layer], head: &Layer, next: &Layer, chain: usize) -> bool {
    next.visible
        && next.properties.clipped == head.properties.clipped
        && direct_effect_mask(layers, next)
        && !(chain >= effects::MASK_SLOTS && next.mask.as_ref().is_some_and(|m| m.enabled))
        && fusable_adjustment(next)
}
/// Compile only programs referenced by this document, including the fused
/// sibling chains used by compose_group. Catalog previews are a later stage.
pub(super) fn startup_effect_chains(layers: &[Layer]) -> Vec<(Vec<&Layer>, effects::Execution)> {
    let mut result = Vec::new();
    let mut parents = Vec::new();
    for layer in layers {
        if !parents.contains(&layer.properties.parent) {
            parents.push(layer.properties.parent);
        }
        if !layer.visible {
            continue;
        }
        if let Some(effect) = &layer.effect {
            if effect.program.image_boundary() {
                for pass in 0..effect.program.passes.len().max(1) {
                    result.push((vec![layer], effects::Execution::Image(pass)));
                }
            } else {
                result.push((vec![layer], effects::Execution::Fused));
            }
        }
    }
    for parent in parents {
        let mut siblings = layers
            .iter()
            .rev()
            .filter(|l| l.properties.parent == parent && l.kind != LayerKind::Background && l.is_artwork())
            .peekable();
        while let Some(layer) = siblings.next() {
            if !layer.visible || !direct_effect_mask(layers, layer) || !fusable_adjustment(layer) {
                continue;
            }
            let mut chain = vec![layer];
            while let Some(next) = siblings.peek().filter(|next| fuses_after(layers, layer, next, chain.len())) {
                chain.push(*next);
                siblings.next();
            }
            if chain.len() > 1 {
                result.push((chain, effects::Execution::Fused));
            }
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

fn source_binding(r: &WgpuRasterizer, layout: &wgpu::BindGroupLayout, sources: &[wgpu::TextureView; 3]) -> wgpu::BindGroup {
    crate::bindings::group(&r.device, "scene tile inputs", layout, [
        wgpu::BindingResource::TextureView(&sources[0]),
        wgpu::BindingResource::TextureView(&sources[1]),
        wgpu::BindingResource::Sampler(&r.sampler),
    ])
}

fn query_grid_size([x,y,w,h]:[u32;4])->[u32;2] {
    [(x,w),(y,h)].map(|(origin,extent)| (0..extent.min(256)).filter(|i| {
        let p=(2*i+1)*extent/(2*extent.min(256));p>=origin && p<origin+PAGE_SIZE
    }).count() as u32)
}
