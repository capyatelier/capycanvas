//! Classify raw layers or bounded artwork captures into the region's one-bit mask.
//! Source cache slots may be reused immediately after their dispatch is encoded.
use super::*;
use wgpu::util::DeviceExt;
const BATCH_TILES: usize = 16;
const _: () = assert!(BATCH_TILES <= SOURCE_SLOTS);
const PARAMETER_BYTES: u64 = 64 * BATCH_TILES as u64;

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
    empty_cache: wgpu::TextureView,
    tonal_parameters: wgpu::Buffer,
    pub(super) tonal_statistics: wgpu::Buffer,
    tile_pipeline: Deferred<wgpu::ComputePipeline>,
    seed: wgpu::Buffer,
    empty_selection: wgpu::Buffer,
    uniform: Option<wgpu::Buffer>,
    mask: Option<wgpu::Buffer>,
    parameters: Vec<u8>,
    bindings: std::collections::VecDeque<Binding>,
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
        let mut bindings = String::new();
        for i in 0..BATCH_TILES {
            bindings += &format!("@group(0) @binding({i}) var source{i}: texture_2d<f32>;\n");
        }
        bindings += "fn tile_load(i:u32,p:vec2<i32>)->vec4<f32>{switch i {\n";
        for i in 0..BATCH_TILES {
            bindings += &format!("case {i}u:{{return textureLoad(source{i},p,0);}}\n");
        }
        bindings += "default:{return vec4<f32>(0.);}}}\n";
        let shader_source = compose_wgsl(&[
            &working_color::shader(device),
            include_str!("region_sources.wgsl"),
            include_str!("tonal.wgsl"),
            include_str!("region_color.wgsl"),
            &bindings,
            &include_str!("selection_clip.wgsl")
                .replace("@group(1) @binding(1)", "@group(0) @binding(19)"),
        ]);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tiled region classification"),
            source: wgpu::ShaderSource::Wgsl(
                format!("{shader_source}{}", include_str!("tonal_cache_write.wgsl")).into(),
            ),
        });
        let mut entries: Vec<_> = (0..BATCH_TILES as u32)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            })
            .collect();
        entries.extend((16..22).map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: if binding == 16 || binding == 20 {
                    wgpu::BufferBindingType::Uniform
                } else {
                    wgpu::BufferBindingType::Storage {
                        read_only: binding == 19,
                    }
                },
                has_dynamic_offset: binding == 16,
                min_binding_size:
                    (binding == 16).then(|| std::num::NonZeroU64::new(PARAMETER_BYTES).unwrap()),
            },
            count: None,
        }));
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
            cached_entries.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            });
        }
        let cached_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cached tonal scalars"),
            entries: &cached_entries,
        });
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
        let cached_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cached tonal scalars"),
            source: wgpu::ShaderSource::Wgsl(cached_source.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tiled region classification"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tiled region classification"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry| {
            let (device, layout, shader) = (
                device.clone(),
                if entry == "tonal_cached" || entry == "tonal_streaming_control" {
                    cached_pipeline_layout.clone()
                } else {
                    pipeline_layout.clone()
                },
                if entry == "tonal_cached" || entry == "tonal_streaming_control" {
                    cached_shader.clone()
                } else {
                    shader.clone()
                },
            );
            Deferred::pipeline(move |mode| {
                mode.compute(
                    &device,
                    &wgpu::ComputePipelineDescriptor {
                        label: Some(entry),
                        layout: Some(&layout),
                        module: &shader,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        cache: None,
                    },
                )
            })
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
            empty_cache: cache_texture(device, 0).create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            }),
            tonal_parameters: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("tonal parameters"),
                size: 352,
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
        }
    }
    pub fn pipelines(&self) -> impl Iterator<Item = &Deferred<wgpu::ComputePipeline>> {
        [&self.seed_pipeline, &self.tile_pipeline].into_iter()
    }
    pub fn prepare(&self, compiler: &startup::Compiler) -> bool {
        let mut ready = true;
        for pipeline in self.pipelines() {
            compiler.pipeline(pipeline, startup::BRUSH);
            ready &= pipeline.ready();
        }
        ready
    }
    pub fn prepare_tonal(&self, compiler: &startup::Compiler) -> bool {
        compiler.pipeline(&self.tonal_pipeline, startup::BRUSH);
        compiler.pipeline(&self.tonal_cached_pipeline, startup::BRUSH);
        self.tonal_pipeline.ready() && self.tonal_cached_pipeline.ready()
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
    #[cfg(test)]
    pub fn binding_count(&self) -> usize {
        self.bindings.len()
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
        let [w, h] = match request.source.raw_source() {
            layer_render::RegionSource::Layer(id) | layer_render::RegionSource::Coverage(id) => {
                r.target_extent(*id)
            }
            _ => r.document_extent,
        };
        let mut layer = match request.source.raw_source() {
            layer_render::RegionSource::Layer(id) | layer_render::RegionSource::Coverage(id) => {
                Some(*id)
            }
            _ => None,
        };
        let frame = match request.source.raw_source() {
            layer_render::RegionSource::Composite => Some(
                r.artwork_frame
                    .clone()
                    .ok_or(GpuRasterError::InvalidExtent)?,
            ),
            layer_render::RegionSource::Layers(layers) => {
                let mut frame = (**r
                    .artwork_frame
                    .as_ref()
                    .ok_or(GpuRasterError::InvalidExtent)?)
                .clone();
                frame.layers = layers.clone();
                frame.view.background_rgba_linear = layers
                    .iter()
                    .find(|l| l.kind == LayerKind::Background && l.visible)
                    .map(|paper| {
                        let mut color = frame.background;
                        color[3] *= paper.opacity;
                        color
                    })
                    .unwrap_or([0.; 4]);
                Some(Arc::new(frame))
            }
            layer_render::RegionSource::Layer(_) | layer_render::RegionSource::Coverage(_) => None,
            layer_render::RegionSource::Selection(_) | layer_render::RegionSource::Tonal(_) => {
                return Err(GpuRasterError::InvalidExtent);
            }
        };
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
        if let Some(t) = tone {
            let mut data = [0u32; 88];
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
        let stored_mask = matches!(request.source, layer_render::RegionSource::Coverage(_))
            .then(|| {
                layer
                    .and_then(|id| r.layer_masks.definitions.get(&id))
                    .cloned()
            })
            .flatten();
        if stored_mask.is_some() {
            let frame = r
                .artwork_frame
                .clone()
                .ok_or(GpuRasterError::InvalidExtent)?;
            r.layer_masks.prepare(
                &r.device,
                encoder,
                (&frame.layers, &[]),
                r.document_extent,
                false,
                &mut r.selection_clip,
            )?;
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
            return Ok(mask);
        }
        let fallback = if let Some(mask) = &stored_mask {
            [mask.default_coverage; 4]
        } else {
            r.thumbnails
                .paper
                .filter(|(id, _)| Some(*id) == layer)
                .map_or([0.; 4], |(_, c)| {
                    [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]]
                })
        };
        let seed_tile = request.position.map(|v| v / PAGE_SIZE);
        let tiles: Vec<_> = page_coordinates(PixelRect::full([w, h])).collect();
        let batches: Vec<_> = (tone.is_none())
            .then_some(std::slice::from_ref(&seed_tile))
            .into_iter()
            .chain(tiles.chunks(if layer.is_some() { BATCH_TILES } else { 1 }))
            .collect();
        let stride =
            (PARAMETER_BYTES as u32).next_multiple_of(limit.min_uniform_buffer_offset_alignment);
        let mut uniforms = vec![0; stride as usize * batches.len()];
        // Occupancy only, without decoding/copying originals or retaining pages.
        let has_tile = |coordinate: [u32; 2]| {
            let Some(layer) = layer else {
                return true;
            };
            (stored_mask.is_some() && r.layer_masks.pages.contains_key(&(layer, coordinate)))
                || r.paint_layers
                    .iter()
                    .find(|l| l.id == layer)
                    .is_some_and(|l| l.pages.iter().any(|p| p.coordinate == coordinate))
                || r.native_backing(layer).is_some_and(|data| {
                    data.tiles.contains_key(&layer_core::raster::TileKey {
                        plane: layer_core::raster::RasterPlane::Color,
                        coordinate,
                    })
                })
                || r.tiled_sources.get(&layer).is_some_and(|s| {
                    coordinate[0] * PAGE_SIZE < s.extent[0]
                        && coordinate[1] * PAGE_SIZE < s.extent[1]
                })
        };
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
                    if stored_mask.is_some() {
                        2_f32.to_bits()
                    } else {
                        f32::from(coverage).to_bits()
                    },
                    f32::from(stored_mask.as_ref().is_some_and(|m| m.inverted)).to_bits(),
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
                            .map_or(&self.empty_cache, |c| &c.views[i]),
                    ),
                })
                .collect()
        };
        for (i, tiles) in batches.into_iter().enumerate() {
            let mut views: [wgpu::TextureView; BATCH_TILES] =
                std::array::from_fn(|_| r.empty_view.clone());
            for (slot, coordinate) in tiles.iter().enumerate() {
                let source = if let Some(layer) = layer {
                    if opaque {
                        let source = r
                            .tiled_sources
                            .get(&layer)
                            .cloned()
                            .ok_or(GpuRasterError::InvalidExtent)?;
                        Some(self.capture.source_tile(r, &source, *coordinate, encoder)?)
                    } else if stored_mask.is_some() {
                        r.layer_masks.pages.get(&(layer, *coordinate)).map(|p| {
                            source_access::RawTile {
                                texture: p.texture.clone(),
                                view: p.view.clone(),
                            }
                        })
                    } else {
                        r.raw_layer_tile(layer, *coordinate, encoder)?
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
        }
        if let Some(cache) = &mut self.tonal_cache {
            cache.ready = true;
            // The scalar cache supersedes the query's decoded color scratch.
            self.bindings.clear();
            self.capture = Default::default();
        }
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
fn opaque_photo(r: &WgpuRasterizer, frame: &artwork::Frame, extent: [u32; 2]) -> Option<LayerId> {
    if !frame.previews.is_empty() {
        return None;
    }
    let mut layers = frame
        .layers
        .iter()
        .filter(|l| l.visible && !matches!(l.kind, LayerKind::Background | LayerKind::Selection));
    let layer = layers.next()?;
    let source = layer.source.as_ref()?;
    if layers.next().is_some()
        || layer.opacity != 1.
        || layer.mask.is_some()
        || layer.effect.is_some()
        || layer.properties.parent.is_some()
        || layer.properties.offset != layer_core::Point::default()
        || layer.properties.placement != layer_core::Affine::IDENTITY
        || layer.properties.clipped
        || layer.properties.blend != layer_core::LayerBlend::Normal
        || source.interpretation.channels != layer_core::color::source::SourceChannels::Rgb
        || source.extent[0] < extent[0]
        || source.extent[1] < extent[1]
        || r.native_backing(layer.id)
            .is_some_and(|d| !d.tiles.is_empty())
        || r.paint_layers
            .iter()
            .any(|l| l.id == layer.id && !l.pages.is_empty())
    {
        return None;
    }
    Some(layer.id)
}
