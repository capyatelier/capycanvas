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
    seed_pipeline: Deferred<wgpu::ComputePipeline>,
    tile_pipeline: Deferred<wgpu::ComputePipeline>,
    seed: wgpu::Buffer,
    empty_selection: wgpu::Buffer,
    uniform: Option<wgpu::Buffer>,
    mask: Option<wgpu::Buffer>,
    parameters: Vec<u8>,
    bindings: std::collections::VecDeque<Binding>,
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
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tiled region classification"),
            source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[
                &working_color::shader(device),
                include_str!("region_sources.wgsl"),
                include_str!("region_color.wgsl"),
                &bindings,
                &include_str!("selection_clip.wgsl")
                    .replace("@group(1) @binding(1)", "@group(0) @binding(19)"),
            ])),
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
        entries.extend((16..20).map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: if binding == 16 {
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
            let (device, layout, shader) =
                (device.clone(), pipeline_layout.clone(), shader.clone());
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
            seed_pipeline: pipeline("sample_seed"),
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
    pub fn storage_bytes(&self) -> u64 {
        self.capture.storage_bytes()
            + self.seed.size()
            + self.empty_selection.size()
            + self.uniform.as_ref().map_or(0, |b| b.size())
            + self.mask.as_ref().map_or(0, |b| b.size())
    }
    /// Submitted document edits may replace paint pages. Query bindings must
    /// not keep those retired pages resident while the user continues painting.
    pub fn clear_bindings(&mut self) {
        self.bindings.clear();
    }
    #[cfg(test)]
    pub fn binding_count(&self) -> usize {
        self.bindings.len()
    }
    pub fn encode(
        &mut self,
        r: &mut WgpuRasterizer,
        request: &layer_render::RegionRequest,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<wgpu::Buffer, GpuRasterError> {
        let [w, h] = match request.source { layer_render::RegionSource::Layer(id) | layer_render::RegionSource::Coverage(id) => r.target_extent(id), _ => r.document_extent };
        let layer = match &request.source {
            layer_render::RegionSource::Layer(id) | layer_render::RegionSource::Coverage(id) => Some(*id),
            _ => None,
        };
        let frame = match &request.source {
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
            layer_render::RegionSource::Selection(_) => return Err(GpuRasterError::InvalidExtent),
        };
        let coverage = matches!(request.source, layer_render::RegionSource::Coverage(_));
        let stored_mask = coverage.then(|| layer.and_then(|id| r.layer_masks.definitions.get(&id)).cloned()).flatten();
        if stored_mask.is_some() {
            let frame = r.artwork_frame.clone().ok_or(GpuRasterError::InvalidExtent)?;
            r.layer_masks.prepare(&r.device, encoder, (&frame.layers, &[]), r.document_extent, false, &mut r.selection_clip)?;
        }
        let limit = r.device.limits();
        // Preflight the subsequent connected-component allocation before any
        // source decoding/submission. Classification does not relax its limit.
        let parent_bytes = u64::from(w) * u64::from(h) * 4;
        if !coverage && (parent_bytes > limit.max_storage_buffer_binding_size
            || parent_bytes > limit.max_buffer_size)
        {
            return Err(GpuRasterError::SizeOverflow);
        }
        let mask_bytes = if coverage { 32 + u64::from(w.div_ceil(4)) * u64::from(h) * 4 }
            else { u64::from(w.div_ceil(32)) * u64::from(h) * 4 };
        if mask_bytes > limit.max_storage_buffer_binding_size || mask_bytes > limit.max_buffer_size { return Err(GpuRasterError::SizeOverflow); }
        if self.mask.as_ref().is_none_or(|b| b.size() < mask_bytes) {
            self.bindings.clear();
            self.mask = Some(r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("raw region packed eligibility"),
                size: mask_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let mask = self.mask.as_ref().unwrap().clone();
        if coverage {
            let header: Vec<u8> = [0,0,w,h,0,2,0,0].into_iter().flat_map(u32::to_ne_bytes).collect();
            let upload = r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("raw coverage header"), contents: &header, usage: wgpu::BufferUsages::COPY_SRC,
            });
            encoder.copy_buffer_to_buffer(&upload,0,&mask,0,32);
        }
        let fallback = if let Some(mask) = &stored_mask { [mask.default_coverage;4] } else { r
            .thumbnails
            .paper
            .filter(|(id, _)| Some(*id) == layer)
            .map_or([0.; 4], |(_, c)| {
                [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]]
            }) };
        let seed_tile = request.position.map(|v| v / PAGE_SIZE);
        let tiles: Vec<_> = page_coordinates(PixelRect::full([w, h])).collect();
        let batches: Vec<_> = std::iter::once(std::slice::from_ref(&seed_tile))
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
            (stored_mask.is_some() && r.layer_masks.pages.contains_key(&(layer, coordinate))) || r.paint_layers
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
                    if stored_mask.is_some() { 2_f32.to_bits() } else { f32::from(coverage).to_bits() },
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
        for (i, tiles) in batches.into_iter().enumerate() {
            let mut views: [wgpu::TextureView; BATCH_TILES] =
                std::array::from_fn(|_| r.empty_view.clone());
            for (slot, coordinate) in tiles.iter().enumerate() {
                let source = if let Some(layer) = layer {
                    if stored_mask.is_some() {
                        r.layer_masks.pages.get(&(layer,*coordinate)).map(|p| source_access::RawTile {texture:p.texture.clone(),view:p.view.clone()})
                    } else { r.raw_layer_tile(layer, *coordinate, encoder)? }
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
                        binding: 19,
                        resource: selection.as_entire_binding(),
                    },
                ]);
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
            if i == 0 {
                pass.set_pipeline(&self.seed_pipeline);
                pass.dispatch_workgroups(1, 1, 1);
            } else {
                pass.set_pipeline(&self.tile_pipeline);
                pass.dispatch_workgroups(
                    (PAGE_SIZE / if coverage {4} else {32} * PAGE_SIZE).div_ceil(64),
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
        Ok(mask)
    }
}
