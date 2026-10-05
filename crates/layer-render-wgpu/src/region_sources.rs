//! Classify raw layers or bounded artwork captures into the region's one-bit mask.
//! Source cache slots may be reused immediately after their dispatch is encoded.
use super::*;
use wgpu::util::DeviceExt;
const BATCH_TILES: usize = 16;
const _: () = assert!(BATCH_TILES <= SOURCE_SLOTS);
const PARAMETER_BYTES: u64 = 64 * BATCH_TILES as u64;
pub(super) const TONAL_PARAMETER_WORDS: usize = 88;

struct Binding {
    views: [wgpu::TextureView; BATCH_TILES],
    selection: wgpu::Buffer,
    group: wgpu::BindGroup,
}

pub(super) struct RawRegions {
    capture: artwork::Capture,
    layout: wgpu::BindGroupLayout,
    cached_layout: wgpu::BindGroupLayout,
    seed_pipeline: Deferred<wgpu::ComputePipeline>,
    tonal_pipeline: Deferred<wgpu::ComputePipeline>,
    tonal_cached_pipeline: Deferred<wgpu::ComputePipeline>,
    #[cfg(test)]
    streaming_pipeline: Deferred<wgpu::ComputePipeline>,
    #[cfg(test)]
    pub streaming_control: bool,
    tonal_cache: Option<TonalCache>,
    empty_cache: [wgpu::TextureView; 4],
    tonal_parameters: wgpu::Buffer,
    pub(super) tonal_statistics: wgpu::Buffer,
    tile_pipeline: Deferred<wgpu::ComputePipeline>,
    seed: wgpu::Buffer,
    empty_selection: wgpu::Buffer,
    uniform: Option<wgpu::Buffer>,
    mask: Option<wgpu::Buffer>,
    parameters: Vec<u8>,
    bindings: std::collections::VecDeque<Binding>,
    progress: Option<Classification>,
}
struct Classification {
    frame: Option<Arc<artwork::Frame>>,
    extent: [u32; 2],
    batch: usize,
}
// Full-precision luminance/alpha replace repeated color decoding/composition.
// Opaque photos pack two luminances per texel, without storing redundant alpha.
// Tiled arrays respect WebGPU's portable resource limits. Never grow past
// 512 MiB; larger images continue through the bounded tile path.
struct TonalCache {
    extent: [u32; 2],
    chunks: [wgpu::Texture; 4],
    views: [wgpu::TextureView; 4],
    chunk_pixels: u32,
    opaque: bool,
    ready: bool,
}
impl RawRegions {
    pub fn new(device: &PipelineDevice) -> Self {
        // Portable individual texture bindings, not a descriptor-indexing feature.
        // A batch fits in the existing sixteen-slot decoded source cache.
        let bindings = crate::texture_switch(0, BATCH_TILES, "tile_load");
        let shader_source = compose_wgsl(&[
            &working_color::shader(device),
            include_str!("region_sources.wgsl"),
            include_str!("tonal.wgsl"),
            include_str!("region_color.wgsl"),
            &bindings,
            &include_str!("selection_clip.wgsl")
                .replace("@group(1) @binding(1)", "@group(0) @binding(19)"),
        ]);
        let shader = Deferred::wgsl(device, "tiled region classification", format!("{shader_source}{}", include_str!("tonal_cache_write.wgsl")));
        let mut entries: Vec<_> = (0..BATCH_TILES as u32)
            .map(|binding| crate::bindings::texture(binding, wgpu::ShaderStages::COMPUTE, false))
            .collect();
        entries.extend((16..22).map(|binding| crate::bindings::buffer(
            binding,
            wgpu::ShaderStages::COMPUTE,
            if binding == 16 || binding == 20 { wgpu::BufferBindingType::Uniform } else { wgpu::BufferBindingType::Storage { read_only: binding == 19, } },
            binding == 16,
            (binding == 16).then(|| std::num::NonZeroU64::new(PARAMETER_BYTES).unwrap()),
        )));
        let mut cached_entries: Vec<_> = entries
            .iter()
            .filter(|e| matches!(e.binding, 18 | 20 | 21))
            .copied()
            .collect();
        for binding in 22..26 {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format: wgpu::TextureFormat::Rg32Float,
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                },
                count: None,
            });
            cached_entries.push(crate::bindings::texture_of(
                binding,
                wgpu::ShaderStages::COMPUTE,
                wgpu::TextureSampleType::Float { filterable: false },
                wgpu::TextureViewDimension::D2Array,
            ));
        }
        let cached_layout = crate::bindings::layout(device, "cached tonal scalars", &cached_entries);
        let cached_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("cached tonal scalars"),
                bind_group_layouts: &[Some(&cached_layout)],
                immediate_size: 0,
            });
        // The cached pipeline samples the same textures written by the tile
        // entry point, without requiring read/write storage-texture features.
        let cached_source = format!("{shader_source}{}", include_str!("tonal_cache_read.wgsl"));
        #[cfg(test)]
        let cached_source = cached_source + include_str!("tonal_streaming_test.wgsl");
        let cached_shader = Deferred::wgsl(device, "cached tonal scalars", cached_source);
        let layout = crate::bindings::layout(device, "tiled region classification", &entries);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tiled region classification"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry| match entry {
            "tonal_cached" | "tonal_streaming_control" => Deferred::compute(device, entry, &cached_pipeline_layout, &cached_shader, entry),
            _ => Deferred::compute(device, entry, &pipeline_layout, &shader, entry),
        };
        Self {
            capture: Default::default(),
            layout,
            cached_layout,
            seed_pipeline: pipeline("sample_seed"),
            tonal_pipeline: pipeline("tonal_tile"),
            tonal_cached_pipeline: pipeline("tonal_cached"),
            #[cfg(test)]
            streaming_pipeline: pipeline("tonal_streaming_control"),
            #[cfg(test)]
            streaming_control: false,
            tonal_cache: None,
            empty_cache: std::array::from_fn(|_| {
                cache_texture(device, 0).create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2Array),
                    ..Default::default()
                })
            }),
            tonal_parameters: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("tonal parameters"),
                size: (TONAL_PARAMETER_WORDS * 4) as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            tonal_statistics: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("tonal probe histogram"),
                size: (tonal::STAT_WORDS * 4) as u64,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            tile_pipeline: pipeline("classify_tile"),
            seed: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("region seed color"),
                size: 16,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
            empty_selection: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("unlimited raw region"),
                size: 48,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
            uniform: None,
            mask: None,
            parameters: Vec::new(),
            bindings: Default::default(),
            progress: None,
        }
    }
    pub fn pipelines(&self) -> impl Iterator<Item = &Deferred<wgpu::ComputePipeline>> {
        [&self.seed_pipeline, &self.tile_pipeline].into_iter()
    }
    pub fn prepare(&self, compiler: &startup::Compiler) -> bool {
        compiler.require(self.pipelines(), startup::BRUSH)
    }
    pub fn prepare_tonal(&self, compiler: &startup::Compiler) -> bool {
        compiler.require(
            [&self.tonal_pipeline, &self.tonal_cached_pipeline],
            startup::BRUSH,
        )
    }
    pub fn storage_bytes(&self) -> u64 {
        self.capture.storage_bytes()
            + self.tonal_parameters.size()
            + self.tonal_statistics.size()
            + self.seed.size()
            + self.empty_selection.size()
            + self.uniform.as_ref().map_or(0, |b| b.size())
            + self.mask.as_ref().map_or(0, |b| b.size())
            + self
                .tonal_cache
                .as_ref()
                .map_or(0, |c| c.chunks.iter().map(texture_bytes).sum())
    }
    /// Submitted document edits may replace paint pages. Query bindings must
    /// not keep those retired pages resident while the user continues painting.
    pub fn clear_bindings(&mut self) {
        self.bindings.clear();
    }
    pub fn invalidate_tonal(&mut self) {
        self.bindings.clear();
        self.tonal_cache = None;
    }
    pub fn release_mask(&mut self) {
        self.bindings.clear();
        self.mask = None;
    }
    pub fn cancel(&mut self) {
        if self.progress.take().is_some() {
            self.invalidate_tonal();
            self.capture = Default::default();
        }
    }
    pub fn extent(&self) -> Option<[u32; 2]> { self.progress.as_ref().map(|progress| progress.extent) }
    pub fn begin(&mut self, r: &WgpuRasterizer, request: &layer_render::RegionRequest) -> Result<(), GpuRasterError> {
        if self.progress.is_some() { return Ok(()); }
        let frame = match request.source.raw_source() {
            layer_render::RegionSource::Composite => Some(r.artwork_frame.clone().ok_or(GpuRasterError::InvalidExtent)?),
            layer_render::RegionSource::Objects(handle) => {
                let mut frame = (**r.artwork_frame.as_ref().ok_or(GpuRasterError::InvalidExtent)?).clone();
                frame.scope = SceneScope::RawObjects(*handle);
                Some(Arc::new(frame))
            },
            layer_render::RegionSource::Scene {snapshot, scope} => {
                let mut frame = (**r.artwork_frame.as_ref().ok_or(GpuRasterError::InvalidExtent)?).clone();
                frame.scene = snapshot.clone(); frame.scope = scope.clone();
                Some(Arc::new(frame))
            },
            layer_render::RegionSource::Source(_) | layer_render::RegionSource::Coverage(_) => None,
            _ => return Err(GpuRasterError::InvalidExtent),
        };
        self.progress = Some(Classification {frame, extent:r.document_extent, batch:0});
        Ok(())
    }
    #[cfg(test)]
    pub fn tonal_cached(&self) -> bool {
        self.tonal_cache.as_ref().is_some_and(|c| c.ready)
    }
    fn encode_cached(
        &self,
        device: &wgpu::Device,
        encoder: &mut crate::submission::CommandEncoder,
        mask: &wgpu::Buffer,
        [w, h]: [u32; 2],
    ) {
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 18,
                resource: mask.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 20,
                resource: self.tonal_parameters.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 21,
                resource: self.tonal_statistics.as_entire_binding(),
            },
        ];
        entries.extend(
            self.tonal_cache
                .as_ref()
                .unwrap()
                .views
                .iter()
                .enumerate()
                .map(|(i, view)| wgpu::BindGroupEntry {
                    binding: 22 + i as u32,
                    resource: wgpu::BindingResource::TextureView(view),
                }),
        );
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cached tonal range"),
            layout: &self.cached_layout,
            entries: &entries,
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("cached tonal range"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.tonal_cached_pipeline);
        #[cfg(test)]
        if self.streaming_control {
            pass.set_pipeline(&self.streaming_pipeline);
        }
        pass.set_bind_group(0, &binding, &[]);
        pass.dispatch_workgroups(w.div_ceil(4).div_ceil(64), h, 1);
    }
    pub fn encode(
        &mut self,
        r: &mut WgpuRasterizer,
        request: &layer_render::RegionRequest,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<wgpu::Buffer, GpuRasterError> {
        self.begin(r, request)?;
        let [w,h] = self.extent().unwrap();
        let mut layer = match request.source.raw_source() {
            layer_render::RegionSource::Source(id) | layer_render::RegionSource::Coverage(id) => {
                Some(*id)
            }
            _ => None,
        };
        let placed = layer.is_some_and(|id| r.artwork_frame.as_ref().is_none_or(|frame|
            !frame.scene.view().target_geometry(id).is_identity() || id.is_coverage()));
        let frame = self.progress.as_ref().unwrap().frame.clone();
        let tone = if let layer_render::RegionSource::Tonal(t) = &request.source {
            Some(t.as_ref())
        } else {
            None
        };
        if tone.is_some()
            && layer.is_none()
            && let Some(frame) = &frame
        {
            layer = opaque_photo(r, frame, [w, h]);
        }
        let opaque = tone.is_some()
            && layer.is_some()
            && matches!(
                request.source.raw_source(),
                layer_render::RegionSource::Composite
            );
        let cacheable = tone.is_some()
            && matches!(
                request.source.raw_source(),
                layer_render::RegionSource::Composite
            )
            && frame.as_ref().is_some_and(|f| f.previews.is_empty());
        let chunk_pixels = 256 * 256 * r.device.limits().max_texture_array_layers.min(256);
        let chunk_bytes = u64::from(chunk_pixels) * 8;
        let cache_bytes = u64::from(if opaque { w.div_ceil(2) } else { w }) * u64::from(h) * 8;
        if !cacheable
            || cache_bytes > chunk_bytes * 4
            || self
                .tonal_cache
                .as_ref()
                .is_some_and(|c| c.extent != [w, h])
        {
            self.invalidate_tonal();
        }
        if cacheable && cache_bytes <= chunk_bytes * 4 && self.tonal_cache.is_none() {
            self.bindings.clear();
            let chunks = std::array::from_fn(|i| {
                cache_texture(
                    &r.device,
                    cache_bytes
                        .saturating_sub(i as u64 * chunk_bytes)
                        .min(chunk_bytes)
                        .div_ceil(8) as u32,
                )
            });
            let views = std::array::from_fn(|i| {
                chunks[i].create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2Array),
                    ..Default::default()
                })
            });
            self.tonal_cache = Some(TonalCache {
                extent: [w, h],
                chunk_pixels,
                opaque,
                ready: false,
                chunks,
                views,
            });
        }
        let cached = self.tonal_cache.as_ref().is_some_and(|c| c.ready);
        let coverage =
            tone.is_some() || matches!(request.source, layer_render::RegionSource::Coverage(_));
        if let Some(t) = tone.filter(|_| self.progress.as_ref().unwrap().batch == 0) {
            let mut data = [0u32; TONAL_PARAMETER_WORDS];
            data[84..88].copy_from_slice(&[
                self.tonal_cache.as_ref().map_or(0, |c| c.chunk_pixels),
                w,
                h,
                u32::from(self.tonal_cache.as_ref().is_some_and(|c| c.opaque)),
            ]);
            for (i, b) in t.bands.iter().enumerate() {
                for (j, v) in [
                    b.lower.unwrap_or(-1000.),
                    b.upper.unwrap_or(1000.),
                    b.falloff[0],
                    b.falloff[1],
                ]
                .into_iter()
                .enumerate()
                {
                    data[i * 4 + j] = v.to_bits();
                }
            }
            for (i, w) in r.document_color.space.to_xyz()[1].iter().enumerate() {
                data[64 + i] = (*w as f32).to_bits();
            }
            if let Some(probe) = t.probe {
                data[68..72].copy_from_slice(&probe.bounds);
            }
            data[72..76].copy_from_slice(&[
                t.bands.len() as u32,
                u32::from(t.invert),
                u32::from(t.probe.is_some()),
                u32::from(t.probe.is_some_and(|p| p.point)),
            ]);
            if let Some(q) = t.probe.and_then(|p| p.quad) {
                data[74] = 2;
                for (i, p) in q.iter().enumerate() {
                    data[76 + i * 2] = p.x.to_bits();
                    data[77 + i * 2] = p.y.to_bits();
                }
            }
            let bytes: Vec<u8> = data.into_iter().flat_map(u32::to_ne_bytes).collect();
            r.queue.write_buffer(&self.tonal_parameters, 0, &bytes);
            encoder.clear_buffer(&self.tonal_statistics, 0, None);
        }
        let limit = r.device.limits();
        // Preflight the subsequent connected-component allocation before any
        // source decoding/submission. Classification does not relax its limit.
        let parent_bytes = u64::from(w) * u64::from(h) * 4;
        if !coverage
            && (parent_bytes > limit.max_storage_buffer_binding_size
                || parent_bytes > limit.max_buffer_size)
        {
            return Err(GpuRasterError::SizeOverflow);
        }
        let mask_bytes = if coverage {
            64 + u64::from(w.div_ceil(4)) * u64::from(h) * 4
        } else {
            u64::from(w.div_ceil(32)) * u64::from(h) * 4
        };
        if mask_bytes > limit.max_storage_buffer_binding_size || mask_bytes > limit.max_buffer_size
        {
            return Err(GpuRasterError::SizeOverflow);
        }
        if self.mask.as_ref().is_none_or(|b| b.size() < mask_bytes) {
            self.bindings.clear();
            self.mask = Some(r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("raw region packed eligibility"),
                size: mask_bytes,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }));
        }
        let mask = self.mask.as_ref().unwrap().clone();
        if coverage {
            let header: Vec<u8> = [0, 0, w, h, 0, 2, 0, 0]
                .into_iter()
                .flat_map(u32::to_ne_bytes)
                .collect();
            let upload = r
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("raw coverage header"),
                    contents: &header,
                    usage: wgpu::BufferUsages::COPY_SRC,
                });
            encoder.copy_buffer_to_buffer(&upload, 0, &mask, 0, 32);
        }
        if cached {
            self.encode_cached(&r.device, encoder, &mask, [w, h]);
            self.progress = None;
            return Ok(mask);
        }
        let fallback = layer.and_then(|target| r.layer_masks.definitions.get(&target)).map_or([0.;4], |mask| [mask.default_coverage;4]);
        let seed_tile = request.position.map(|v| v / PAGE_SIZE);
        let tiles: Vec<_> = page_coordinates(PixelRect::full([w, h])).collect();
        let batches: Vec<_> = (tone.is_none())
            .then_some(std::slice::from_ref(&seed_tile))
            .into_iter()
            .chain(tiles.chunks(if layer.is_some() && !placed { BATCH_TILES } else { 1 }))
            .collect();
        let stride =
            (PARAMETER_BYTES as u32).next_multiple_of(limit.min_uniform_buffer_offset_alignment);
        let mut uniforms = vec![0; stride as usize * batches.len()];
        // Occupancy only, without decoding/copying originals or retaining pages.
        let has_tile = |coordinate: [u32;2]| placed || layer.is_none_or(|id|
            r.paint_layers.iter().find(|l| l.id == id).is_some_and(|l| l.pages.iter().any(|p| p.coordinate == coordinate))
                || r.native_backing(id).is_some_and(|data| data.tiles.contains_key(&layer_core::raster::TileKey {
                    plane:layer_core::raster::RasterPlane::Color,coordinate }))
                || r.tiled_sources.get(&id).is_some_and(|base| source_access::paint_base_contains(base, coordinate)));
        for (batch, tiles) in batches.iter().enumerate() {
            for (i, coordinate) in tiles.iter().enumerate() {
                let [x, y] = coordinate.map(|v| v * PAGE_SIZE);
                let data = [
                    w,
                    h,
                    x,
                    y,
                    request.position[0] % PAGE_SIZE,
                    request.position[1] % PAGE_SIZE,
                    PAGE_SIZE.min(w - x),
                    PAGE_SIZE.min(h - y),
                    fallback[0].to_bits(),
                    fallback[1].to_bits(),
                    fallback[2].to_bits(),
                    fallback[3].to_bits(),
                    request.tolerance.to_bits(),
                    f32::from(has_tile(*coordinate)).to_bits(),
                    (if request.enclosure.is_some() { 3.0f32 } else { f32::from(coverage) }).to_bits(),
                    0,
                ];
                for (word, value) in uniforms[batch * stride as usize + i * 64..][..64]
                    .chunks_exact_mut(4)
                    .zip(data)
                {
                    word.copy_from_slice(&value.to_ne_bytes());
                }
            }
        }
        if self
            .uniform
            .as_ref()
            .is_none_or(|b| b.size() < uniforms.len() as u64)
        {
            self.bindings.clear();
            self.uniform = Some(
                r.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("tiled region parameters"),
                        contents: &uniforms,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    }),
            );
            self.parameters = uniforms;
        } else if self.parameters != uniforms {
            r.queue
                .write_buffer(self.uniform.as_ref().unwrap(), 0, &uniforms);
            self.parameters = uniforms;
        }
        let selection = request
            .limit
            .as_ref()
            .and(r.selection_clip.buffer.as_ref())
            .unwrap_or(&self.empty_selection)
            .clone();
        let cache_entries = || -> Vec<_> {
            (0..4)
                .map(|i| wgpu::BindGroupEntry {
                    binding: 22 + i as u32,
                    resource: wgpu::BindingResource::TextureView(
                        self.tonal_cache
                            .as_ref()
                            .map_or(&self.empty_cache[i], |c| &c.views[i]),
                    ),
                })
                .collect()
        };
        for (i, tiles) in batches.into_iter().enumerate().skip(self.progress.as_ref().unwrap().batch) {
            let mut views: [wgpu::TextureView; BATCH_TILES] =
                std::array::from_fn(|_| r.empty_view.clone());
            let mut leases = Vec::with_capacity(tiles.len());
            for (slot, coordinate) in tiles.iter().enumerate() {
                let source = if let Some(layer) = layer {
                    if opaque {
                        let source = r
                            .tiled_sources
                            .get(&layer)
                            .cloned()
                            .ok_or(GpuRasterError::InvalidExtent)?;
                        r.paint_base_tile(&source, *coordinate, encoder)?
                    } else {
                        if placed { Some(self.capture.layer_tile(r,layer,*coordinate,encoder)?) } else { r.raw_layer_tile(layer,*coordinate,encoder)? }
                    }
                } else {
                    let region = page_rect(*coordinate).intersect(PixelRect::full([w, h]));
                    Some(self.capture.region(
                        r,
                        frame.as_ref().unwrap().packet([w, h]),
                        region,
                        [PAGE_SIZE; 2],
                        encoder,
                    )?)
                };
                if let Some(source) = &source { leases.extend(r.source_tiles.borrow().lease(&source.view)); }
                views[slot] = source.map_or_else(|| r.empty_view.clone(), |t| t.view);
            }
            let hit = self
                .bindings
                .iter()
                .position(|b| b.views == views && b.selection == selection);
            let binding = if let Some(hit) = hit {
                self.bindings.remove(hit).unwrap()
            } else {
                let mut entries: Vec<_> = views
                    .iter()
                    .enumerate()
                    .map(|(binding, view)| wgpu::BindGroupEntry {
                        binding: binding as u32,
                        resource: wgpu::BindingResource::TextureView(view),
                    })
                    .collect();
                entries.extend([
                    wgpu::BindGroupEntry {
                        binding: 16,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: self.uniform.as_ref().unwrap(),
                            offset: 0,
                            size: std::num::NonZeroU64::new(PARAMETER_BYTES),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 17,
                        resource: self.seed.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 18,
                        resource: mask.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 20,
                        resource: self.tonal_parameters.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 21,
                        resource: self.tonal_statistics.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 19,
                        resource: selection.as_entire_binding(),
                    },
                ]);
                entries.extend(cache_entries());
                let group = r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("raw region tile"),
                    layout: &self.layout,
                    entries: &entries,
                });
                Binding {
                    views,
                    selection: selection.clone(),
                    group,
                }
            };
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("raw region tile"),
                timestamp_writes: None,
            });
            pass.set_bind_group(0, &binding.group, &[(i as u32) * stride]);
            if i == 0 && tone.is_none() {
                pass.set_pipeline(&self.seed_pipeline);
                pass.dispatch_workgroups(1, 1, 1);
            } else {
                pass.set_pipeline(if tone.is_some() {
                    &self.tonal_pipeline
                } else {
                    &self.tile_pipeline
                });
                pass.dispatch_workgroups(
                    (PAGE_SIZE / if coverage { 4 } else { 32 } * PAGE_SIZE).div_ceil(64),
                    1,
                    tiles.len() as u32,
                );
            }
            drop(pass);
            if self.bindings.len() == 4 {
                self.bindings.pop_front();
            }
            self.bindings.push_back(binding);
            self.progress.as_mut().unwrap().batch = i + 1;
        }
        if let Some(cache) = &mut self.tonal_cache {
            cache.ready = true;
            // The scalar cache supersedes the query's decoded color scratch.
            self.bindings.clear();
            self.capture = Default::default();
        }
        self.progress = None;
        Ok(mask)
    }
}

fn cache_texture(device: &wgpu::Device, pixels: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tonal luminance and alpha"),
        size: wgpu::Extent3d {
            width: pixels.clamp(1, 256),
            height: pixels.div_ceil(256).clamp(1, 256),
            depth_or_array_layers: pixels.div_ceil(256 * 256).max(1),
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rg32Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

// An unedited, untransformed opaque RGB photograph covering the document is
// already its composite. Feed the existing batched raw-source path; all other
// artwork keeps the exact compositor, including alpha, masks and HDR effects.
fn opaque_photo(r: &WgpuRasterizer, frame: &artwork::Frame, extent: [u32; 2]) -> Option<SourceTarget> {
    if !frame.previews.is_empty() {
        return None;
    }
    let scene = frame.scene.view().with_scope(&frame.scope);
    let mut visible = scene.order().iter().copied().filter(|&h| scene.visible(h)
        && scene.occurrence(h).is_some_and(|o| !matches!(o.content, OccurrenceContent::Selection(_))));
    let handle = visible.next()?; let occurrence = scene.occurrence(handle)?; let target = scene.source_target(handle)?;
    let base = scene.paint_base(target)?;
    let source = &base.image;
    if visible.next().is_some() || occurrence.opacity != 1. || occurrence.mask.is_some() || scene.parent(handle).is_some()
        || occurrence.translation != layer_core::Point::default() || occurrence.placement != layer_core::LayerPlacement::IDENTITY
        || occurrence.attachment.is_clip() || occurrence.blend != layer_core::LayerBlend::Normal
        || source.interpretation.channels != layer_core::color::source::SourceChannels::Rgb
        || base.offset != [0;2] || source.extent[0] < extent[0] || source.extent[1] < extent[1]
        || r.native_backing(target).is_some_and(|d| !d.tiles.is_empty())
        || r.paint_layers.iter().any(|l| l.id == target && !l.pages.is_empty()) { return None; }
    Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artwork_sample_tests::{add_group, effect_draft, set_effect};
    use layer_core::authored::{CoverageSource, MaskUse, PortableId};
    use layer_core::{BlendSpace, Document, DocumentNames, EffectValue, LayerBlend, Point, Projective, Rect, Selection};
    use layer_render::{RegionRequest, RegionSource};

    #[test]
    fn scoped_raw_constant_fill_ignores_nested_appearance_and_preserves_artwork() {
        let extent = [64;2];
        let mut document = Document::new(PortableId::random(), extent[0], extent[1], DocumentNames {paint:"Ink".into(),paper:"Fill".into()});
        document.artwork.compositions.get_mut(document.artwork.root).unwrap().blend = BlendSpace::Linear;
        let fill = document.scene().children(None)[1];
        let color = layer_core::color::RgbColor::from_linear(layer_core::color::RgbSpace::Srgb, [0.5,0.25,0.75,0.5]).unwrap();
        let mut effect = effect_draft(&document, fill);
        effect.set("color", EffectValue::Color(color)).unwrap();
        set_effect(&mut document, fill, effect);
        let inner = add_group(&mut document, vec![fill], 1);
        let outer = add_group(&mut document, vec![inner], 1);
        let coverage = document.artwork.coverage.insert(PortableId::random(), CoverageSource {
            domain:extent, raster:Default::default(), initial:Some(Selection::polygon(Rect::from_extent([32,64]).corners().to_vec()).unwrap()),
            default_coverage:0., operations:Arc::default(),
        }).unwrap();
        let mask = MaskUse {source:coverage,linked:false,enabled:true,inverted:true,translation:Point::default(),placement:Projective::IDENTITY};
        for handle in [fill,inner,outer] {
            let occurrence = document.artwork.occurrences.get_mut(handle).unwrap();
            occurrence.visible = false;
            occurrence.opacity = 0.;
            occurrence.blend = LayerBlend::Multiply;
            occurrence.mask = Some(mask.clone());
            occurrence.translation = Point {x:901.,y:-777.};
        }
        let authored = document.snapshot();
        let mut normalized = authored.as_ref().clone();
        let occurrence = normalized.artwork.occurrences.get_mut(fill).unwrap();
        occurrence.visible = true;
        occurrence.opacity = 1.;
        occurrence.blend = LayerBlend::Normal;
        occurrence.attachment = layer_core::Attachment::None;
        occurrence.mask = None;
        let normalized = Arc::new(normalized);
        let scope = SceneScope::Members(vec![fill].into());
        assert_eq!(normalized.view().with_scope(&scope).evaluation_parent(fill), None);
        assert!(normalized.view().with_scope(&scope).visible(fill));
        assert_eq!(normalized.view().occurrence(fill).unwrap().translation, authored.view().occurrence(fill).unwrap().translation);
        let mut r = WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
        r.submit(FramePacket {blend_space:BlendSpace::Linear,..crate::test_support::packet(document.scene(),extent)}).unwrap();
        let mut capture = r.snapshot_gpu().capture_scene(normalized.clone(), scope.clone(), Default::default()).unwrap();
        let expected = [0.25,0.125,0.375,0.5];
        for pixel in capture.read_region([0,0,extent[0],extent[1]]).unwrap() {
            for channel in 0..4 { assert!((pixel[channel]-expected[channel]).abs()<0.0005, "{pixel:?}"); }
        }
        let result = crate::test_support::receive_request(&mut r, RegionRequest {
            enclosure: None, request_id:42,source:RegionSource::Scene {snapshot:normalized,scope},position:[63,63],tolerance:0.,
            contiguous:true,selection:None,refinement:Default::default(),limit:None,
        });
        assert_eq!(result.pixels.extent(),extent);
        assert_eq!(result.pixels.coverage_format(),1);
        assert_eq!(result.pixels.coverage_bounds(),[0,0,extent[0],extent[1]]);
        let count = result.pixels.pixels_per_word();
        let stride = extent[0].div_ceil(count);
        for y in 0..extent[1] {
            for x in 0..extent[0] {
                let word = result.pixels.words()[(y*stride+x/count) as usize];
                assert_eq!((word >> ((x%count)*4)) & 15,4,"coverage at {x},{y}");
            }
        }
        for handle in [fill,inner,outer] {
            assert_eq!(document.scene().occurrence(handle),authored.view().occurrence(handle));
            assert!(!document.scene().occurrence(handle).unwrap().visible);
        }
        assert_eq!(document.scene().effect(fill).unwrap().constant_color(),Some(color));
    }
}
