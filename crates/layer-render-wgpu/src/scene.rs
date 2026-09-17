//! Tiled layer composition with explicit cached image boundaries. Pointwise
//! scratch follows nesting depth. Masks never download or rewrite paint.
use super::*;
use wgpu::util::DeviceExt;
#[path = "scene_images.rs"]
mod images;
#[path = "filter_previews.rs"]
mod previews;
mod metadata;
pub(crate) mod windows;
pub(super) use previews::FilterPreviews;
mod sources;

#[derive(Clone)]
enum Job {
    DecodedTile(std::sync::Arc<sources::PendingTile>),
    SourceUpload {
        buffer: wgpu::Buffer,
        texture: wgpu::Texture,
    },
    Effect {
        target: wgpu::TextureView,
        sources: [wgpu::TextureView; 2],
        data: [f32; 24],
        prepared: effects::PreparedEffect,
        // Keep ordinary draw jobs small: only effects carry the mask inputs.
        masks: Box<[wgpu::TextureView; effects::MASK_SLOTS]>,
    },
    Draw {
        target: wgpu::TextureView,
        sources: [wgpu::TextureView; 2],
        data: [f32; 24],
        over: bool,
        clip: Option<PixelRect>,
    },
    Clear(wgpu::TextureView, wgpu::Color),
    Watercolor {
        target: wgpu::TextureView,
        binding: wgpu::BindGroup,
        record: u32,
        coordinate: [u32; 2],
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
pub(super) struct Scene {
    source_tiles: sources::DecodedTiles,
    pool: Vec<PageSurface>,
    used: Vec<bool>,
    jobs: Vec<Job>,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::BindGroupLayout,
    buffer: wgpu::Buffer,
    binding: wgpu::BindGroup,
    stride: usize,
    capacity: usize,
    record_count: usize,
    upload: Vec<u8>,
    pipeline: [wgpu::RenderPipeline; 2],
    pub(super) effects: effects::Effects,
    pub effect_passes: u64,
    images: images::ImageStages,
    image_window: Option<PixelRect>,
    stop_before: Option<(usize, bool)>,
    #[cfg(test)]
    tiled_composition: bool,
}

/// Immutable device resources, compiled before input is enabled and shared by
/// live composition, captures and recreated scenes. No canvas pixels retained.
#[derive(Clone)]
pub(super) struct Pipelines {
    uniforms: wgpu::BindGroupLayout,
    layout: wgpu::BindGroupLayout,
    pub pipeline: [Deferred<wgpu::RenderPipeline>; 2],
    pub source: sources::Pipelines,
}

impl Scene {
    #[cfg(test)]
    pub fn set_tiled_composition(&mut self, enabled: bool) {
        self.tiled_composition = enabled;
    }
    fn cached_composition(&self) -> bool {
        #[cfg(test)]
        if self.tiled_composition {
            return false;
        }
        true
    }
    #[cfg(test)]
    pub fn force_image_rebuild(&mut self) {
        self.images = images::ImageStages::default();
    }
    #[cfg(test)]
    pub fn image_pass_pixels(&self) -> u64 {
        self.images.pass_pixels
    }
    #[cfg(test)]
    pub fn image_mask_pixels(&self) -> u64 {
        self.images.mask_pixels
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
    pub fn composition_work(&self) -> [u64; 4] {
        [
            self.images.backdrop_updates,
            self.images.backdrop_pixels,
            self.images.composition_pixels,
            self.images.composition_builds,
        ]
    }
    pub fn source_cache_work(&self) -> [u64; 2] {
        [self.source_tiles.hits, self.source_tiles.misses]
    }
    pub fn scratch_bytes(&self) -> u64 {
        let mut bytes = self.pool.iter().map(PageSurface::storage_bytes).sum::<u64>()
            + (self.capacity * self.stride) as u64
            + self.effects.storage_bytes()
            + self.images.storage_bytes();
        { bytes += self.source_tiles.gpu_bytes(); }
        bytes
    }
    pub fn initialize_images(
        &mut self,
        r: &mut WgpuRasterizer,
        layers: &[Layer],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        // Cold import only. Reuse one encoded source tile and cap native upload
        // buffers at 16 MiB. Final paint stays in the existing raster pages.
        const UPLOAD_TILES: usize = 64;
        self.jobs.clear();
        let mut source_tile = None;
        let mut pixels = Vec::new();
        let mut uploads = 0;
        for layer in layers {
            if r.has_raster_source(layer.id) { continue; }
            let Some(source) = layer.asset.as_ref().and_then(|a| r.image_sources.get(a)).cloned() else {
                continue;
            };
            let Some(stored) = r.paint_layers.iter().find(|p| p.id == layer.id) else { continue; };
            let pages: Vec<_> = stored.pages.iter()
                .map(|p| (p.coordinate, p.active().view.clone())).collect();
            let (texture, view) = source_tile.get_or_insert_with(|| {
                create_target(&r.device, [PAGE_SIZE; 2], wgpu::TextureFormat::Rgba8Unorm,
                    "bounded encoded source tile")
            });
            pixels.resize(PAGE_BYTES as usize, 0);
            for (coordinate, target) in pages {
                pixels.fill(0);
                let [x, y] = coordinate.map(|v| v * PAGE_SIZE);
                let width = source.extent[0].saturating_sub(x).min(PAGE_SIZE) as usize;
                let height = source.extent[1].saturating_sub(y).min(PAGE_SIZE) as usize;
                if width > 0 {
                    for row in 0..height {
                        let offset = ((y as usize + row) * source.extent[0] as usize + x as usize) * 4;
                        let packed = row * PAGE_SIZE as usize * 4;
                        pixels[packed..packed + width * 4].copy_from_slice(&source.bytes[offset..offset + width * 4]);
                    }
                }
                // Mapped-at-creation buffers preserve copy/draw ordering on both
                // native and WebGPU; Queue::write_texture would run all uploads
                // before the draws and make every tile see the final source.
                let buffer = r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("source tile upload"), contents: &pixels,
                    usage: wgpu::BufferUsages::COPY_SRC,
                });
                self.jobs.push(Job::SourceUpload { buffer, texture: texture.clone() });
                let mut data = [0.; 24];
                data[..8].copy_from_slice(&[0., 0., 256., 256., 256., 256., 0., 0.]);
                data[8] = 8.;
                self.jobs.push(Job::Draw {
                    target, sources: [view.clone(), r.empty_view.clone()],
                    data, over: false, clip: None,
                });
                uploads += 1;
                r.metrics.source_upload_peak_bytes = r.metrics.source_upload_peak_bytes
                    .max((uploads as u64 + 2) * PAGE_BYTES);
                if uploads == UPLOAD_TILES {
                    self.encode_jobs(r, encoder)?;
                    self.jobs.clear();
                    Self::submit_source_uploads(r, encoder)?;
                    uploads = 0;
                }
            }
        }
        self.encode_jobs(r, encoder)?;
        self.jobs.clear();
        if uploads != 0 {
            Self::submit_source_uploads(r, encoder)?;
        }
        Ok(())
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
        let next = crate::submission::CommandEncoder::new(&r.device,
            &wgpu::CommandEncoderDescriptor { label: Some(label) });
        let previous = std::mem::replace(encoder, next);
        r.uploads.finish(&previous);
        let submission = previous.submit(&r.queue);
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
        let _ = submission; // Browser queue draining needs separate host qualification.
        Ok(())
    }
    pub fn initialize_source_paint(&mut self, r: &mut WgpuRasterizer, layers: &[Layer], encoder: &mut crate::submission::CommandEncoder) -> Result<(), GpuRasterError> {
        self.jobs.clear();
        for layer in layers.iter().filter(|l| l.source.is_some() || r.native_backing(l.id).is_some()) {
            let pages: Vec<_> = r.paint_layers.iter().filter(|p| p.id == layer.id)
                .flat_map(|p| p.pages.iter().filter(|p| p.primary_needs_clear)
                    .map(|p| (p.coordinate, p.primary.view.clone()))).collect();
            for (coordinate, target) in pages {
                let Some(view) = self.source_tile(r, layer, coordinate)? else { continue; };
                let mut data = [0.; 24];
                data[..8].copy_from_slice(&[0., 0., 256., 256., 256., 256., 0., 0.]);
                data[8] = 1.;
                data[9] = 1.;
                self.jobs.push(Job::Draw { target, sources: [view, r.empty_view.clone()], data, over: false, clip: None });
            }
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
        self.jobs.clear();
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
        for (coordinate, target) in pages {
            let Some(view) = self.source_tile(r, layer, coordinate)? else {
                continue;
            };
            let mut data = [0.; 24];
            data[..8].copy_from_slice(&[0., 0., 256., 256., 256., 256., 0., 0.]);
            data[8] = 1.;
            data[9] = 1.;
            self.jobs.push(Job::Draw {
                target,
                sources: [view, r.empty_view.clone()],
                data,
                over: false,
                clip: None,
            });
        }
        self.encode_jobs(r, encoder)
    }
    pub fn begin_frame(&mut self) {
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
        let pipeline = pipeline.map(|p| p.compile().clone());
        let stride = device.limits().min_uniform_buffer_offset_alignment.max(96) as usize;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene uniform records"),
            size: stride as u64 * 128,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let binding = uniform_binding(device, &uniforms, &buffer);
        let effects = effects::Effects::new(r, &uniforms, &layout);
        Self {
            source_tiles: sources::DecodedTiles::new(r.document_color().space),
            pool: Vec::new(),
            used: Vec::new(),
            jobs: Vec::new(),
            layout,
            uniforms,
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
            #[cfg(test)]
            tiled_composition: false,
        }
    }
    fn alloc(&mut self, r: &WgpuRasterizer, color: wgpu::Color) -> usize {
        let id = self.reserve(r);
        self.jobs
            .push(Job::Clear(self.pool[id].view.clone(), color));
        id
    }
    fn reserve(&mut self, r: &WgpuRasterizer) -> usize {
        let id = self
            .used
            .iter()
            .position(|v| !*v)
            .unwrap_or(self.pool.len());
        if id == self.pool.len() {
            self.pool.push(r.create_page_surface("scene reusable tile"));
            self.used.push(false);
        }
        self.used[id] = true;
        id
    }
    fn free(&mut self, id: usize) {
        self.used[id] = false;
    }
    fn source_tile(&mut self, r: &WgpuRasterizer, layer: &Layer, coordinate: [u32; 2]) -> Result<Option<wgpu::TextureView>, GpuRasterError> {
        if let Some(blob) = r.native_color_tile(layer.id, coordinate)? {
            let space = r.document_color().space;
            let (tile, pending) = self.source_tiles.plan_raster(r, &blob, space, space)?;
            if let Some(pending) = pending { self.jobs.push(Job::DecodedTile(std::sync::Arc::new(pending))); }
            return Ok(Some(tile.view));
        }
        let Some(source) = &layer.source else { return Ok(None); };
        if coordinate[0] >= source.extent[0].div_ceil(PAGE_SIZE) || coordinate[1] >= source.extent[1].div_ceil(PAGE_SIZE) {
            return Ok(None);
        }
        {
            let (tile, pending) = self.source_tiles.plan(r, source, coordinate)?;
            if let Some(pending) = pending { self.jobs.push(Job::DecodedTile(std::sync::Arc::new(pending))); }
            Ok(Some(tile.view))
        }
    }
    /// Consume a bounded group before gathering the next one: these textures
    /// belong to the fixed, queue-ordered source cache.
    pub fn source_tile_for_query(
        &mut self,
        r: &mut WgpuRasterizer,
        source: &std::sync::Arc<layer_core::color::source::SourceImage>,
        coordinate: [u32; 2],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<crate::source_access::RawTile, GpuRasterError> {
        {
            debug_assert!(self.jobs.is_empty());
            let (tile, pending) = self.source_tiles.plan(r, source, coordinate)?;
            if let Some(pending) = pending {
                // Each independent query owns its ordered uniform copy. Repeated
                // sampling between frames must not grow the record buffer.
                self.record_count = 0;
                self.jobs.push(Job::DecodedTile(std::sync::Arc::new(pending)));
                self.encode_jobs(r, encoder)?;
            }
            Ok(tile)
        }
    }
    pub fn prepare_native_transfer(&mut self, r: &WgpuRasterizer, space: layer_core::color::RgbSpace) -> Result<crate::native_tiles::NativeTransfer, GpuRasterError> {
        self.source_tiles.prepare_transfer(&r.device, space)
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
            let (tile, pending) = self.source_tiles.plan_raster(r, request.blob, request.space, request.destination)?;
            if let Some(pending) = pending {
                self.record_count = 0;
                self.jobs.push(Job::DecodedTile(std::sync::Arc::new(pending)));
                self.encode_jobs(r, encoder)?;
            }
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
            if self.source_tiles.uploads_full() {
                Self::submit_source_uploads(r, encoder)?;
            }
            let bytes = crate::native_tiles::scalar::restore_upload(r, request, encoder)?;
            let in_flight = self.source_tiles.charge_upload(encoder, bytes);
            r.metrics.source_upload_peak_bytes = r.metrics.source_upload_peak_bytes.max(in_flight);
        }
        Ok(())
    }
    pub fn prepared_source_view(
        &self,
        source: &std::sync::Arc<layer_core::color::source::SourceImage>,
        coordinate: [u32; 2],
    ) -> Option<&wgpu::TextureView> {
        self.source_tiles.prepared_view(source, coordinate)
    }
    pub fn prepared_raster_view(&self, blob: &std::sync::Arc<layer_core::raster::TileBlob>, space: layer_core::color::RgbSpace) -> Option<&wgpu::TextureView> {
        self.source_tiles.prepared_raster_view(blob, space)
    }
    pub fn raster_tile_for_query(&mut self, r: &mut WgpuRasterizer, blob: &std::sync::Arc<layer_core::raster::TileBlob>, space: layer_core::color::RgbSpace, encoder: &mut crate::submission::CommandEncoder) -> Result<crate::source_access::RawTile, GpuRasterError> {
        let (tile, pending) = self.source_tiles.plan_raster(r, blob, space, r.document_color().space)?;
        if let Some(pending) = pending {
            self.record_count = 0;
            self.jobs.push(Job::DecodedTile(std::sync::Arc::new(pending)));
            self.encode_jobs(r, encoder)?;
        }
        Ok(tile)
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
                        r.preview_pages.iter().find(|p| {
                            p.coordinate == neighbor
                                && !r.preview_damage.intersect(page_rect(neighbor)).is_empty()
                        })
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
                .prepare(r, &layers, effects::Execution::Fused, packet.time_seconds)?;
        let mask =
            if indices.len() == 1 && !direct_effect_mask(packet.layers, layer) {
                layer.mask.as_ref().filter(|m| m.enabled).map(|m| {
                    self.mask_tile(r, m, world_offset(packet.layers, layer.id, true), tile)
                })
            } else {
                None
            };
        let out = self.reserve(r);
        let mut data = [0.; 24];
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
                    over: true,
                    ..
                },
            ) = (&self.jobs[n - 2], &self.jobs[n - 1])
                && *clear == self.pool[input].view
                && target == clear
                && paint_data[..4] == [0., 0., 256., 256.]
                && (paint_data[8] == 7. || paint_data[8] == 1.)
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
        over: bool,
    ) {
        let mut data = [0.; 24];
        data[..4].copy_from_slice(&rect);
        data[4..8].copy_from_slice(&[256., 256., 0., 0.]);
        data[8..12].copy_from_slice(&options);
        self.jobs.push(Job::Draw {
            target: self.pool[target].view.clone(),
            sources: [source, back.unwrap_or_else(|| r.empty_view.clone())],
            data,
            over,
            clip: None,
        });
    }
    fn combine(
        &mut self,
        r: &WgpuRasterizer,
        front: usize,
        back: usize,
        opacity: f32,
        blend: layer_core::LayerBlend,
        clip: bool,
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
                data[10] = blend as u32 as f32;
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
            [4., opacity, blend as u32 as f32, f32::from(clip)],
            false,
        );
        self.free(front);
        self.free(back);
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
                a: 1.,
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
                    layer.source.as_ref().map_or(r.document_extent, |source| {
                        std::array::from_fn(|i| r.document_extent[i].max(source.extent[i]))
                    }),
                );
                for c in page_coordinates(region) {
                    let rect = local_rect(c, offset, tile);
                    if !intersects(rect) {
                        continue;
                    }
                    let preview = r.preview_layer_id == Some(layer.id)
                        && !r.preview_damage.intersect(page_rect(c)).is_empty();
                    let watercolor_preview = preview && packet.dab_batches.iter().any(|b|
                        b.layer_id == layer.id && b.kind == DabBatchKind::Preview
                            && b.style.execution == BrushExecution::Watercolor);
                    let wet_nearby = stored.is_some_and(|stored| (stored.watercolor.is_some() || watercolor_preview)
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
                        // Aligned pages already cover the layer tile. Only a
                        // translated page needs an intermediate and placement.
                        let page = if rect == [0., 0., 256., 256.] {
                            out
                        } else {
                            self.alloc(r, wgpu::Color::TRANSPARENT)
                        };
                        self.jobs.push(Job::Watercolor {
                            target: self.pool[page].view.clone(),
                            binding,
                            record: *r.layer_style_records.get(&layer.id).ok_or(GpuRasterError::MissingPaintLayer(layer.id))?,
                            coordinate: c,
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
                            );
                            self.free(page);
                        }
                    } else {
                        let persistent = stored.and_then(|s| s.pages.iter().find(|p| p.coordinate == c));
                        let predicted = if preview {
                            r.preview_pages.iter().find(|p| p.coordinate == c)
                        } else {
                            None
                        };
                        if let Some(p) =
                            predicted.filter(|_| r.preview_requires_base).or(persistent)
                        {
                            self.draw(
                                r,
                                out,
                                p.active().view.clone(),
                                None,
                                rect,
                                [1., 1., 0., 0.],
                                true,
                            );
                        } else if let Some(view) = self.source_tile(r, layer, c)? {
                            self.draw(r, out, view, None, rect, [1., 1., 0., 0.], true);
                        }
                        if let Some(p) = predicted.filter(|_| !r.preview_requires_base) {
                            self.draw(
                                r,
                                out,
                                p.active().view.clone(),
                                None,
                                rect,
                                [1., 1., 0., 0.],
                                true,
                            );
                        }
                    }
                }
            }
            out
        };
        if let Some(mask) = layer.mask.as_ref().filter(|m| m.enabled) {
            let m = self.mask_tile(r, mask, world_offset(packet.layers, layer.id, true), tile);
            let result = self.alloc(r, wgpu::Color::TRANSPARENT);
            self.draw(
                r,
                result,
                self.pool[out].view.clone(),
                Some(self.pool[m].view.clone()),
                [0., 0., 256., 256.],
                [3., 1., 0., 0.],
                false,
            );
            self.free(out);
            self.free(m);
            Ok(result)
        } else {
            Ok(out)
        }
    }
    fn group(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        parent: Option<LayerId>,
        tile: [u32; 2],
    ) -> Result<usize, GpuRasterError> {
        let mut output = self.alloc(
            r,
            if parent.is_none() {
                let c = packet.view.background_rgba_linear;
                wgpu::Color {
                    r: (c[0] * c[3]) as f64,
                    g: (c[1] * c[3]) as f64,
                    b: (c[2] * c[3]) as f64,
                    a: c[3] as f64,
                }
            } else {
                wgpu::Color::TRANSPARENT
            },
        );
        let mut stack: Option<(usize, usize)> = None;
        // An image boundary already contains its full input stack, final mask
        // and layer properties. Start above the latest completed boundary.
        let checkpoint = packet.layers.iter().enumerate().find(|(i, l)| {
            self.cached_composition()
                && l.visible
                && l.properties.parent == parent
                && l.effect
                    .as_ref()
                    .is_some_and(|e| e.program.kind == layer_core::EffectKind::Adjustment)
                && self.stop_before.is_none_or(|(stop, _)| *i > stop)
                && self.images.checkpoint(*i, l).is_some()
        });
        if let Some((i, l)) = checkpoint {
            self.free(output);
            self.jobs.pop(); // Discard the unused initial clear as well.
            let (pixels, pending_stack) = self.images.checkpoint(i, l).unwrap();
            output = self.image_tile(r, pixels, self.images.bounds, tile);
            stack = pending_stack.map(|(pixels, base)| {
                (
                    self.image_tile(r, pixels, self.images.bounds, tile),
                    base,
                )
            });
        }
        let mut siblings = packet
            .layers
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, l)| l.properties.parent == parent && l.kind != LayerKind::Background)
            .filter(|(i, _)| checkpoint.is_none_or(|(cut, _)| *i < cut))
            .peekable();
        while let Some((i, layer)) = siblings.next() {
            if let Some((stop, clipped)) = self.stop_before
                && stop == i
            {
                if clipped {
                    self.free(output);
                    return Ok(stack.map_or_else(
                        || self.alloc(r, wgpu::Color::TRANSPARENT),
                        |(pixels, _)| pixels,
                    ));
                }
                if let Some((pixels, base)) = stack {
                    let b = &packet.layers[base];
                    output = self.combine(r, pixels, output, b.opacity, b.properties.blend, false);
                }
                return Ok(output);
            }
            if layer
                .effect
                .as_ref()
                .is_some_and(|e| e.program.kind == layer_core::EffectKind::Adjustment)
            {
                if !layer.properties.clipped
                    && let Some((pixels, base)) = stack.take()
                {
                    let b = &packet.layers[base];
                    output = self.combine(r, pixels, output, b.opacity, b.properties.blend, false);
                }
                if !layer.visible {
                    continue;
                }
                let mut chain = vec![i];
                if direct_effect_mask(packet.layers, layer)
                    && !layer.effect.as_ref().unwrap().program.image_boundary()
                {
                    while let Some((j, next)) = siblings.peek() {
                        if self.stop_before.is_some_and(|(stop, _)| *j <= stop)
                            || !next.visible
                            || next.properties.clipped != layer.properties.clipped
                            || !direct_effect_mask(packet.layers, next)
                            || (chain.len() >= effects::MASK_SLOTS
                                && next.mask.as_ref().is_some_and(|m| m.enabled))
                            || !next.effect.as_ref().is_some_and(|e| {
                                e.program.kind == layer_core::EffectKind::Adjustment
                                    && !e.program.image_boundary()
                            })
                        {
                            break;
                        }
                        chain.push(*j);
                        siblings.next();
                    }
                }
                if layer.properties.clipped {
                    if let Some((pixels, base)) = stack.take() {
                        stack = Some((self.effect(r, packet, &chain, tile, pixels)?, base));
                    }
                } else {
                    output = self.effect(r, packet, &chain, tile, output)?;
                }
                continue;
            }
            if !layer.properties.clipped {
                if let Some((pixels, base)) = stack.take() {
                    let b = &packet.layers[base];
                    output = self.combine(r, pixels, output, b.opacity, b.properties.blend, false);
                }
                let clips_above = packet.layers[..i]
                    .iter()
                    .rev()
                    .find(|l| l.properties.parent == parent)
                    .is_some_and(|l| l.properties.clipped);
                if layer.visible
                    && !clips_above
                    && self.draw_normal_layer(r, packet, i, tile, output)?
                {
                    continue;
                }
                if layer.visible
                    && (matches!(layer.kind, LayerKind::Group | LayerKind::Effect)
                        || layer.source.is_some()
                        || r.native_color_coordinates(layer.id).next().is_some()
                        || r.paint_layers
                            .iter()
                            .any(|l| l.id == layer.id && !l.pages.is_empty())
                        || r.preview_layer_id == Some(layer.id))
                {
                    stack = Some((self.layer(r, packet, i, tile)?, i));
                }
            } else if layer.visible
                && let Some((pixels, base)) = stack.take()
            {
                let source = self.layer(r, packet, i, tile)?;
                stack = Some((
                    self.combine(
                        r,
                        source,
                        pixels,
                        layer.opacity,
                        layer.properties.blend,
                        true,
                    ),
                    base,
                ));
            }
        }
        if let Some((pixels, base)) = stack {
            let b = &packet.layers[base];
            output = self.combine(r, pixels, output, b.opacity, b.properties.blend, false);
        }
        Ok(output)
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
            || world_offset(packet.layers, layer.id, false) != layer_core::Point::default()
            || r.preview_layer_id == Some(layer.id)
        {
            return Ok(false);
        }
        let mask = layer.mask.as_ref().filter(|m| m.enabled);
        if mask.is_some()
            && world_offset(packet.layers, layer.id, true) != layer_core::Point::default()
        {
            return Ok(false);
        }
        let Some(stored) = r.paint_layers.iter().find(|l| l.id == layer.id) else {
            return Ok(true);
        };
        if stored.watercolor.is_some() {
            return Ok(false);
        }
        let view = if let Some(page) = stored.pages.iter().find(|p| p.coordinate == tile) {
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
        );
        Ok(true)
    }
    pub fn apply_operation(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        layer_index: usize,
        operation_index: usize,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        use layer_core::LayerOperationKind;
        self.jobs.clear();
        self.used.fill(false);
        let layer = &packet.layers[layer_index];
        let op = &layer.pending_operations[operation_index];
        let damage = pixel_rect(op.bounds(packet.document_extent), packet.document_extent);
        let Some(stored) = r.paint_layers.iter().find(|l| l.id == layer.id) else {
            return Ok(());
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
        let watercolor = stored.watercolor.is_some() && op.kind == LayerOperationKind::ApplyMask;
        for (c, source, destination) in pages {
            let mask = self.mask_tile(r, &op.coverage, op.coverage.offset, c);
            let out = self.alloc(r, wgpu::Color::TRANSPARENT);
            match op.kind {
                LayerOperationKind::Transform(_) => {
                    unreachable!("transforms execute against immutable captures")
                }
                LayerOperationKind::ApplyMask => {
                    let mut resolved = None;
                    if watercolor {
                        let binding = self.watercolor_binding(r, layer, stored, c, false)?;
                        let p = self.alloc(r, wgpu::Color::TRANSPARENT);
                        self.jobs.push(Job::Watercolor {
                            target: self.pool[p].view.clone(),
                            binding,
                            record: *r.layer_style_records.get(&layer.id).ok_or(GpuRasterError::MissingPaintLayer(layer.id))?,
                            coordinate: c,
                        });
                        resolved = Some(p);
                    }
                    self.draw(
                        r,
                        out,
                        resolved.map_or(source, |p| self.pool[p].view.clone()),
                        Some(self.pool[mask].view.clone()),
                        [0., 0., 256., 256.],
                        [3., 1., 0., 0.],
                        false,
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
                    );
                    if let Some(Job::Draw { data, .. }) = self.jobs.last_mut() {
                        data[6..8].copy_from_slice(&c.map(|v| (v * PAGE_SIZE) as f32));
                        data[12..16].copy_from_slice(&colors[0]);
                        data[16..20].copy_from_slice(&colors[1]);
                        data[20..24].copy_from_slice(&endpoints);
                    }
                }
            }
            self.jobs.push(Job::Copy {
                source: self.pool[out].texture.clone(),
                source_origin: [0; 2],
                destination,
                origin: [0, 0],
                width: 256,
                height: 256,
            });
            self.free(out);
            self.free(mask);
        }
        self.encode_jobs(r, encoder)?;
        if op.kind == LayerOperationKind::ApplyMask {
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
        self.jobs.clear();
        if self.images.bounds != window {
            self.images = images::ImageStages::default();
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
        parent: Option<LayerId>,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        if region.is_empty() || region.intersect(PixelRect::full(packet.document_extent)) != region
            || destination.width() < region.width() || destination.height() < region.height()
        {
            return Err(GpuRasterError::InvalidExtent);
        }
        let window = images::capture_window(packet.layers, region, packet.document_extent);
        self.begin_frame();
        self.image_window = Some(window);
        self.effects.retain(packet.layers);
        let result = (|| {
            self.update_images(r, packet, window, encoder)?;
            self.jobs.clear();
            self.used.fill(false);
            self.stop_before = None;
            for tile in page_coordinates(region) {
                let output = self.group(r, packet, parent, tile)?;
                self.copy_window_tile(output, destination, tile, region);
            }
            self.encode_jobs(r, encoder)
        })();
        self.stop_before = None;
        result
    }

    pub fn compose(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        dirty: PixelRect,
        encoder: &mut crate::submission::CommandEncoder,
        overlay: bool,
        tiles: Option<&std::collections::BTreeSet<[u32; 2]>>,
    ) -> Result<(), GpuRasterError> {
        if let Some(native) = &r.native_edit
            && let Some(plan) = windows::Plan::new(packet.layers, packet.document_extent, native.image_pixel_bytes)?
        {
            return self.compose_windows(r, packet, dirty, encoder, overlay, plan);
        }
        self.image_window = None;
        self.effects.retain(packet.layers);
        let dirty = self.update_images(r, packet, dirty, encoder)?;
        self.compose_pixels(r, packet, dirty, encoder, overlay, tiles)
    }

    fn compose_pixels(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        dirty: PixelRect,
        encoder: &mut crate::submission::CommandEncoder,
        overlay: bool,
        tiles: Option<&std::collections::BTreeSet<[u32; 2]>>,
    ) -> Result<(), GpuRasterError> {
        if dirty.is_empty() {
            return Ok(());
        }
        self.jobs.clear();
        self.used.fill(false);
        // Completed image boundaries include their surrounding composition.
        // Copy only the changed region; clipping uses the same final path.
        if r.live_display.is_none() && self.cached_composition()
            && let Some(top) = packet.layers.iter().find(|l| {
                l.visible && l.properties.parent.is_none() && l.kind != LayerKind::Background
            })
            && top
                .effect
                .as_ref()
                .is_some_and(|e| e.program.kind == layer_core::EffectKind::Adjustment)
            && (!overlay
                || !packet
                    .layers
                    .iter()
                    .any(|l| l.mask.as_ref().is_some_and(|m| m.enabled && m.show_area)))
            && let Some(source) = self.images.scene_texture(top)
        {
            let origin = wgpu::Origin3d {
                x: dirty.min_x(),
                y: dirty.min_y(),
                z: 0,
            };
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: source,
                    origin: wgpu::Origin3d {
                        x: dirty.min_x() - self.images.bounds.min_x(),
                        y: dirty.min_y() - self.images.bounds.min_y(),
                        z: 0,
                    },
                    ..source.as_image_copy()
                },
                wgpu::TexelCopyTextureInfo {
                    texture: r.composite_texture.as_ref().unwrap(),
                    origin,
                    ..source.as_image_copy()
                },
                wgpu::Extent3d {
                    width: dirty.width(),
                    height: dirty.height(),
                    depth_or_array_layers: 1,
                },
            );
            r.metrics.composited_pixels += dirty.area();
            return Ok(());
        }
        let mut composited = 0;
        let mut display_tiles = 0;
        for tile in page_coordinates(dirty) {
            if tiles.is_some_and(|tiles| !tiles.contains(&tile)) {
                continue;
            }
            if display_tiles == SOURCE_SLOTS {
                // Bound command/driver storage before encoding another batch.
                // Submit the final batch with the frame, without a CPU wait.
                Self::submit_chunk(r, encoder, "bounded display composition")?;
                r.metrics.display_composition_submissions += 1;
                display_tiles = 0;
            }
            composited += page_rect(tile).intersect(dirty).area();
            let mut output = self.group(r, packet, None, tile)?;
            if overlay {
                for layer in packet.layers {
                    if let Some(mask) = layer.mask.as_ref().filter(|m| m.enabled && m.show_area) {
                        let m = self.mask_tile(
                            r,
                            mask,
                            world_offset(packet.layers, layer.id, true),
                            tile,
                        );
                        let tint = self.alloc(r, wgpu::Color::TRANSPARENT);
                        self.draw(
                            r,
                            tint,
                            self.pool[m].view.clone(),
                            None,
                            [0., 0., 256., 256.],
                            [5., 1., 0., 0.],
                            false,
                        );
                        self.free(m);
                        output = self.combine(
                            r,
                            tint,
                            output,
                            1.,
                            layer_core::LayerBlend::Normal,
                            false,
                        );
                    }
                }
            }
            let origin = [tile[0] * PAGE_SIZE, tile[1] * PAGE_SIZE];
            if r.live_display.is_some() {
                self.encode_jobs(r, encoder)?;
                let mut cache = r.live_display.take().unwrap();
                let result = cache.write_tile(r, r.display_pipelines.as_ref().unwrap(), encoder,
                    &self.pool[output].texture, [0; 2], tile);
                r.live_display = Some(cache);
                self.free(output);
                result?;
                display_tiles += 1;
                continue;
            }
            // The last effect already writes every pixel. Write directly into
            // the composite region instead of copying its scratch result.
            if let Some(Job::Effect { target, data, .. }) = self.jobs.last_mut()
                && *target == self.pool[output].view
            {
                *target = r.composite_view.as_ref().unwrap().clone();
                data[..6].copy_from_slice(&[
                    origin[0] as f32,
                    origin[1] as f32,
                    PAGE_SIZE.min(packet.document_extent[0] - origin[0]) as f32,
                    PAGE_SIZE.min(packet.document_extent[1] - origin[1]) as f32,
                    packet.document_extent[0] as f32,
                    packet.document_extent[1] as f32,
                ]);
                self.free(output);
                continue;
            }
            self.jobs.push(Job::Copy {
                source: self.pool[output].texture.clone(),
                source_origin: [0; 2],
                destination: r.composite_texture.as_ref().unwrap().clone(),
                origin,
                width: PAGE_SIZE.min(packet.document_extent[0] - origin[0]),
                height: PAGE_SIZE.min(packet.document_extent[1] - origin[1]),
            });
            self.free(output);
        }
        self.encode_jobs(r, encoder)?;
        r.metrics.composited_pixels += composited;
        Ok(())
    }

    // wgpu handles hash by stable resource identity, not mutable GPU contents.
    #[allow(clippy::mutable_key_type)]
    fn encode_jobs(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        let result = self.encode_jobs_inner(r, encoder);
        if result.is_err() {
            // Dropping unencoded reservations invalidates their source keys.
            self.jobs.clear();
        }
        result
    }

    fn encode_jobs_inner(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
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
        }
        self.upload.resize(self.jobs.len() * self.stride, 0);
        for (i, job) in self.jobs.iter().enumerate() {
            let data = match job {
                Job::Draw { data, .. } | Job::Effect { data, .. } => Some(data),
                Job::DecodedTile(pending) => pending.data.as_ref(),
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
                &r.queue,
                &self.buffer,
                (base * self.stride) as u64,
                &self.upload,
            )?;
        }
        let mut source_bindings = std::collections::HashMap::new();
        let mut mask_bindings = std::collections::HashMap::new();
        let mut encoded_through = 0;
        for (i, job) in self.jobs.iter().enumerate() {
            if i < encoded_through {
                continue;
            }
            match job {
                Job::DecodedTile(pending) => {
                    if self.source_tiles.uploads_full() {
                        Self::submit_source_uploads(r, encoder)?;
                    }
                    let bytes = self.source_tiles.encode(r, encoder, pending, &self.binding, ((base + i) * self.stride) as u32)?;
                    let in_flight = self.source_tiles.charge_upload(encoder, bytes);
                    r.metrics.source_upload_peak_bytes = r.metrics.source_upload_peak_bytes.max(in_flight);
                }
                Job::SourceUpload { buffer, texture } => encoder.copy_buffer_to_texture(
                    wgpu::TexelCopyBufferInfo {
                        buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0, bytes_per_row: Some(PAGE_SIZE * 4), rows_per_image: Some(PAGE_SIZE),
                        },
                    },
                    texture.as_image_copy(), texture.size(),
                ),
                Job::Clear(target, color) => {
                    if self.jobs.get(i+1).is_some_and(|next|matches!(next,Job::Draw{target:next,..}|Job::Effect{target:next,..}|Job::Watercolor{target:next,..} if next==target)) {continue;}
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
                Job::Draw { target, .. } | Job::Effect { target, .. } => {
                    let end=(i+1..self.jobs.len()).find(|&j|!matches!(&self.jobs[j],Job::Draw{target:next,..}|Job::Effect{target:next,..} if next==target)).unwrap_or(self.jobs.len());
                    let load = if i > 0
                        && let Job::Clear(previous, color) = &self.jobs[i - 1]
                        && previous == target
                    {
                        wgpu::LoadOp::Clear(*color)
                    } else {
                        wgpu::LoadOp::Load
                    };
                    let attachments = [Some(attachment(target, load))];
                    let mut pass = encoder.begin_render_pass(&descriptor(&attachments));
                    if self.jobs[i..end]
                        .iter()
                        .any(|j| matches!(j, Job::Effect { .. }))
                    {
                        self.effect_passes += 1;
                    }
                    for (j, job) in self.jobs.iter().enumerate().take(end).skip(i) {
                        let sources = match job {
                            Job::Draw { sources, .. } | Job::Effect { sources, .. } => sources,
                            _ => unreachable!(),
                        };
                        let binding = source_bindings.entry(sources).or_insert_with(|| {
                            r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                label: Some("scene tile inputs"),
                                layout: &self.layout,
                                entries: &[
                                    wgpu::BindGroupEntry {
                                        binding: 0,
                                        resource: wgpu::BindingResource::TextureView(&sources[0]),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 1,
                                        resource: wgpu::BindingResource::TextureView(&sources[1]),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 2,
                                        resource: wgpu::BindingResource::Sampler(&r.sampler),
                                    },
                                ],
                            })
                        });
                        if let Job::Effect {
                            prepared, masks, ..
                        } = job
                        {
                            pass.set_pipeline(&prepared.pipeline);
                            pass.set_bind_group(2, &prepared.binding, &[]);
                            let masks = mask_bindings.entry(masks.as_ref()).or_insert_with(|| {
                                r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                    label: Some("effect tile masks"),
                                    layout: &self.effects.masks,
                                    entries: &masks
                                        .iter()
                                        .enumerate()
                                        .map(|(i, m)| wgpu::BindGroupEntry {
                                            binding: i as u32,
                                            resource: wgpu::BindingResource::TextureView(m),
                                        })
                                        .collect::<Vec<_>>(),
                                })
                            });
                            pass.set_bind_group(3, &*masks, &[]);
                        } else if let Job::Draw { over, .. } = job {
                            pass.set_pipeline(&self.pipeline[usize::from(*over)]);
                        }
                        pass.set_bind_group(0, &self.binding, &[((base + j) * self.stride) as u32]);
                        pass.set_bind_group(1, &*binding, &[]);
                        let (data, clip) = match job {
                            Job::Draw { data, clip, .. } => (data, *clip),
                            Job::Effect { data, .. } => (data, None),
                            _ => unreachable!(),
                        };
                        let clip =
                            clip.unwrap_or(PixelRect::full([data[4] as u32, data[5] as u32]));
                        pass.set_scissor_rect(
                            clip.min_x(),
                            clip.min_y(),
                            clip.width(),
                            clip.height(),
                        );
                        pass.draw(0..3, 0..1);
                    }
                    encoded_through = end;
                }
                Job::Watercolor {
                    target,
                    binding,
                    record,
                    coordinate,
                } => {
                    let load = match i.checked_sub(1).and_then(|j| self.jobs.get(j)) {
                        Some(Job::Clear(previous, color)) if previous == target => {
                            wgpu::LoadOp::Clear(*color)
                        }
                        _ => wgpu::LoadOp::Load,
                    };
                    let attachments = [Some(attachment(target, load))];
                    let mut pass = encoder.begin_render_pass(&descriptor(&attachments));
                    pass.set_pipeline(&r.pipelines.watercolor_composite);
                    pass.set_bind_group(0, &r.style_bind_group, &[*record * r.style_stride as u32]);
                    pass.set_bind_group(1, &r.target_bind_group, &[r.target_offset(*coordinate)]);
                    pass.set_bind_group(2, binding, &[]);
                    pass.draw(0..3, 0..1);
                }
            }
        }
        self.jobs.clear();
        Ok(())
    }
}

impl Pipelines {
    pub fn effects(&self, r: &WgpuRasterizer) -> effects::Effects {
        effects::Effects::new(r, &self.uniforms, &self.layout)
    }
    pub fn new(device: &PipelineDevice) -> Self {
        let uniforms = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene records"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(96),
                },
                count: None,
            }],
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene sources"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let shader = Deferred::new({
            let device = device.clone();
            move || {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("layer scene"),
                    source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[&working_color::shader(&device), include_str!("scene.wgsl")])),
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
            Deferred::new(move || {
                fullscreen_pipeline(
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
        Self {
            source: sources::Pipelines::new(device, &uniforms),
            uniforms,
            layout,
            pipeline,
        }
    }
}
#[cfg(test)]
#[test]
fn new_scenes_reuse_compiled_device_pipelines_without_retaining_pixels() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let a = Scene::new(&r);
    let b = Scene::new(&r);
    assert_eq!(
        a.pipeline,
        r.scene_pipelines
            .pipeline
            .clone()
            .map(|p| p.compile().clone())
    );
    assert_eq!(a.pipeline, b.pipeline);
    assert_eq!(a.uniforms, b.uniforms);
    assert_eq!(a.layout, b.layout);
    assert_ne!(a.buffer, b.buffer, "mutable records are not shared");
    assert!(a.pool.is_empty() && b.pool.is_empty());
}

fn direct_effect_mask(layers: &[Layer], layer: &Layer) -> bool {
    layer.mask.as_ref().is_none_or(|m| {
        !m.enabled || world_offset(layers, layer.id, true) == layer_core::Point::default()
    })
}
fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}
fn uniform_binding(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("scene uniform binding"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer,
                offset: 0,
                size: NonZeroU64::new(96),
            }),
        }],
    })
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

/// Compile only programs referenced by this document, including the fused
/// sibling chains used by compose_group. Catalog previews are a later stage.
pub(super) fn startup_effect_chains(layers: &[Layer]) -> Vec<(Vec<Layer>, effects::Execution)> {
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
                    result.push((vec![layer.clone()], effects::Execution::Image(pass)));
                }
            } else {
                result.push((vec![layer.clone()], effects::Execution::Fused));
            }
        }
    }
    for parent in parents {
        let mut siblings = layers
            .iter()
            .rev()
            .filter(|l| l.properties.parent == parent && l.kind != LayerKind::Background)
            .peekable();
        while let Some(layer) = siblings.next() {
            if !layer.visible
                || !direct_effect_mask(layers, layer)
                || !layer.effect.as_ref().is_some_and(|e| {
                    e.program.kind == layer_core::EffectKind::Adjustment
                        && !e.program.image_boundary()
                })
            {
                continue;
            }
            let mut chain = vec![layer.clone()];
            while let Some(next) = siblings.peek() {
                if !next.visible
                    || next.properties.clipped != layer.properties.clipped
                    || !direct_effect_mask(layers, next)
                    || (chain.len() >= effects::MASK_SLOTS
                        && next.mask.as_ref().is_some_and(|m| m.enabled))
                    || !next.effect.as_ref().is_some_and(|e| {
                        e.program.kind == layer_core::EffectKind::Adjustment
                            && !e.program.image_boundary()
                    })
                {
                    break;
                }
                chain.push((*next).clone());
                siblings.next();
            }
            if chain.len() > 1 {
                result.push((chain, effects::Execution::Fused));
            }
        }
    }
    result
}
