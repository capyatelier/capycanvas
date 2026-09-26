//! Affine cut-and-place over a bounded set of immutable source views. Manual
//! Float32 interpolation applies selection and premultiplied color together.
use super::{Deferred, PipelineDevice, Uploads};
use layer_core::{Affine, ImageTransform, Interpolation};
use std::hash::{Hash, Hasher};

pub(super) const TRANSFORM_SLOTS: usize = 16;
const SOURCE_RECORD_BYTES: u64 = (1 + TRANSFORM_SLOTS as u64) * 16;

pub(super) struct TransformTile<'a> {
    pub view: &'a wgpu::TextureView,
    pub origin: [i32; 2],
    pub extent: [u32; 2],
}

pub struct TransformSource {
    binding: wgpu::BindGroup,
}
pub struct TransformTarget<'a> {
    /// Single-sample attachment matching the prepared color/scalar working format,
    /// never aliasing the source.
    pub view: &'a wgpu::TextureView,
    /// Actual view extent and its origin in the same space as the source/matrix.
    pub extent: [u32; 2],
    pub origin: [i32; 2],
    /// x, y, width, height in this target. Unchanged pixels are not touched.
    pub region: [u32; 4],
}

pub(super) struct TiledTransformRecord<'a> {
    pub target: [u32; 2],
    pub sources: &'a [[u32; 2]],
    pub source_size: [u32; 2],
}
struct SourceBinding {
    used: u64,
    views: Vec<wgpu::TextureView>,
    selection: wgpu::Buffer,
    binding: wgpu::BindGroup,
}

pub struct PixelTransform {
    placement: bool,
    scalar: bool,
    visibility: bool,
    pub(super) pipeline: Deferred<wgpu::RenderPipeline>,
    layout: wgpu::BindGroupLayout,
    source_layout: wgpu::BindGroupLayout,
    empty_selection: wgpu::Buffer,
    source_records: wgpu::Buffer,
    source_capacity: u64,
    source_stride: u32,
    source_next_record: u64,
    source_upload: Vec<u8>,
    bindings: std::collections::HashMap<u64, Vec<SourceBinding>>,
    binding_count: usize,
    binding_clock: u64,
    uniforms: Option<(wgpu::Buffer, wgpu::BindGroup)>,
    stride: u32,
    capacity: u64,
    records: Vec<u8>,
    next_record: u64,
}
impl PixelTransform {
    pub(super) fn staged(device: &PipelineDevice, scalar: bool) -> Self {
        Self::create(device, scalar, false)
    }
    pub(super) fn staged_visibility(device: &PipelineDevice) -> Self {
        Self::create(device, true, true)
    }
    fn create(device: &PipelineDevice, scalar: bool, visibility: bool) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("affine transform parameters"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(48),
                },
                count: None,
            }],
        });
        let mut entries: Vec<_> = (0..TRANSFORM_SLOTS as u32)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            })
            .collect();
        entries.extend([
            wgpu::BindGroupLayoutEntry {
                binding: 16,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(SOURCE_RECORD_BYTES),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 17,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(48),
                },
                count: None,
            },
        ]);
        let source_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bounded immutable transform inputs"),
            entries: &entries,
        });
        let compile_device = device.clone();
        let parameters = layout.clone();
        let source = source_layout.clone();
        let pipeline = Deferred::pipeline(move |mode| {
            let device = &compile_device;
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("affine pixels with selection"),
                source: wgpu::ShaderSource::Wgsl(super::compose_wgsl(&[
                    include_str!("pixel_transform.wgsl"),
                    &source_shader(),
                    &include_str!("selection_clip.wgsl")
                        .replace("@group(1) @binding(1)", "@group(1) @binding(17)"),
                ])),
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("affine pixels"),
                bind_group_layouts: &[Some(&parameters), Some(&source)],
                immediate_size: 0,
            });
            mode.render(&device, &wgpu::RenderPipelineDescriptor {
                label: Some("affine cut and place"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fragment_main"),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[
                            ("scalar", f64::from(scalar)),
                            ("visibility", f64::from(visibility)),
                        ],
                        ..Default::default()
                    },
                    targets: &[Some(wgpu::ColorTargetState {
                        format: if scalar {
                            device.scalar_format()
                        } else {
                            device.working_format()
                        },
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        });
        Self {
            placement: false,
            scalar,
            visibility,
            pipeline,
            layout,
            source_layout,
            empty_selection: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("transform selects entire layer"),
                size: 48,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
            source_records: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ordered affine source records"),
                size: SOURCE_RECORD_BYTES,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            source_capacity: SOURCE_RECORD_BYTES,
            source_stride: (SOURCE_RECORD_BYTES as u32)
                .next_multiple_of(device.limits().min_uniform_buffer_offset_alignment),
            source_next_record: 0,
            source_upload: Vec::new(),
            bindings: Default::default(),
            binding_count: 0,
            binding_clock: 0,
            uniforms: None,
            stride: 48_u32.div_ceil(device.limits().min_uniform_buffer_offset_alignment)
                * device.limits().min_uniform_buffer_offset_alignment,
            capacity: 0,
            records: Vec::new(),
            next_record: 0,
        }
    }
    /// Transactions share shader recipes and the ordered source-record buffer.
    /// Their region uniform ranges remain independent.
    pub(super) fn fork(&self) -> Self {
        Self {
            placement: self.placement,
            scalar: self.scalar,
            visibility: self.visibility,
            pipeline: self.pipeline.clone(),
            layout: self.layout.clone(),
            source_layout: self.source_layout.clone(),
            empty_selection: self.empty_selection.clone(),
            source_records: self.source_records.clone(),
            source_capacity: self.source_capacity,
            source_stride: self.source_stride,
            source_next_record: 0,
            source_upload: Vec::new(),
            bindings: Default::default(),
            binding_count: 0,
            binding_clock: 0,
            uniforms: None,
            stride: self.stride,
            capacity: 0,
            records: Vec::new(),
            next_record: 0,
        }
    }
    pub(super) fn placement_pass(&self) -> Self {
        let mut pass = self.fork();
        pass.placement = true;
        pass
    }

    /// Inputs are consumed before another source-cache neighborhood is prepared.
    /// `bounds` and view origins are in document coordinates; missing texels use
    /// the declared background, including sparse gaps inside those bounds.
    pub(super) fn source_views(
        &mut self,
        device: &wgpu::Device,
        tiles: &[TransformTile<'_>],
        bounds: [i32; 4],
        selection: Option<&wgpu::Buffer>,
        fallback: &wgpu::TextureView,
    ) -> Result<TransformSource, &'static str> {
        if tiles.len() > TRANSFORM_SLOTS
            || bounds[2] <= 0
            || bounds[3] <= 0
            || !valid_extent([bounds[0], bounds[1]], [bounds[2] as u32, bounds[3] as u32])
            || tiles.iter().any(|t| !valid_extent(t.origin, t.extent))
        {
            return Err("Invalid transform source views");
        }
        if selection.is_some_and(|b| {
            b.size() < 48
                || b.size() > device.limits().max_storage_buffer_binding_size
                || !b.usage().contains(wgpu::BufferUsages::STORAGE)
        }) {
            return Err("Invalid transform selection");
        }
        let selection = selection.unwrap_or(&self.empty_selection);
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        selection.hash(&mut hash);
        for tile in tiles {
            tile.view.hash(&mut hash);
        }
        let key = hash.finish();
        self.binding_clock = self.binding_clock.wrapping_add(1);
        let binding = if let Some(cached) = self.bindings.get_mut(&key).and_then(|bucket| {
            bucket.iter_mut().find(|b| {
                b.selection == *selection
                    && b.views.len() == tiles.len()
                    && b.views
                        .iter()
                        .zip(tiles)
                        .all(|(view, tile)| *view == *tile.view)
            })
        }) {
            cached.used = self.binding_clock;
            cached.binding.clone()
        } else {
            let mut entries = Vec::with_capacity(TRANSFORM_SLOTS + 2);
            for i in 0..TRANSFORM_SLOTS {
                entries.push(wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: wgpu::BindingResource::TextureView(
                        tiles.get(i).map_or(fallback, |t| t.view),
                    ),
                });
            }
            entries.push(wgpu::BindGroupEntry {
                binding: 16,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &self.source_records,
                    offset: 0,
                    size: wgpu::BufferSize::new(SOURCE_RECORD_BYTES),
                }),
            });
            entries.push(wgpu::BindGroupEntry {
                binding: 17,
                resource: selection.as_entire_binding(),
            });
            let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("affine source views and selection"),
                layout: &self.source_layout,
                entries: &entries,
            });
            if self.binding_count == 256 {
                let (key, index, _) = self
                    .bindings
                    .iter()
                    .flat_map(|(key, bucket)| {
                        bucket
                            .iter()
                            .enumerate()
                            .map(move |(i, b)| (*key, i, b.used))
                    })
                    .min_by_key(|v| v.2)
                    .unwrap();
                let bucket = self.bindings.get_mut(&key).unwrap();
                bucket.swap_remove(index);
                if bucket.is_empty() {
                    self.bindings.remove(&key);
                }
                self.binding_count -= 1;
            }
            self.bindings.entry(key).or_default().push(SourceBinding {
                used: self.binding_clock,
                views: tiles.iter().map(|t| t.view.clone()).collect(),
                selection: selection.clone(),
                binding: binding.clone(),
            });
            self.binding_count += 1;
            binding
        };
        Ok(TransformSource {
            binding,
        })
    }
    /// Begin a submitted frame. Multiple encodes in that frame use distinct
    /// uniform ranges. Submit the previous frame before calling this again.
    pub fn begin_frame(&mut self) {
        self.next_record = 0;
        self.source_next_record = 0;
    }
    // wgpu views hash by stable resource identity, not mutable texel contents.
    #[allow(clippy::mutable_key_type)]
    pub fn retain_source_bindings(
        &mut self,
        views: &[&wgpu::TextureView],
        selection: Option<&wgpu::Buffer>,
    ) {
        let allowed: std::collections::HashSet<_> = views.iter().copied().collect();
        self.bindings.retain(|_, bucket| {
            bucket.retain(|b| {
                (b.selection == self.empty_selection
                    || selection.is_some_and(|s| *s == b.selection))
                    && b.views.iter().all(|v| allowed.contains(v))
            });
            !bucket.is_empty()
        });
        self.binding_count = self.bindings.values().map(Vec::len).sum();
    }
    pub fn shared_source_bytes(&self, other: &Self) -> u64 {
        if self.source_records == other.source_records {
            self.source_capacity
        } else {
            0
        }
    }
    fn reserve_regions(&mut self, device: &wgpu::Device, end: u64) {
        if end > self.capacity {
            self.capacity = end
                .next_power_of_two()
                .min(device.limits().max_buffer_size)
                .min(u64::from(u32::MAX))
                & !3;
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("affine region uniforms"),
                size: self.capacity,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("affine region uniforms"),
                layout: &self.layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &buffer,
                        offset: 0,
                        size: wgpu::BufferSize::new(48),
                    }),
                }],
            });
            self.uniforms = Some((buffer, binding));
        }
    }
    pub fn prepare_tiled(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        uploads: &mut Uploads,
        encoder: &mut crate::submission::CommandEncoder,
        bounds: [i32; 4],
        background: f32,
        transform: &ImageTransform,
        jobs: &[TiledTransformRecord<'_>],
    ) -> Result<[u32; 2], &'static str> {
        let affine = transform.as_affine().ok_or("Unsupported transform")?;
        let inverse = affine
            .inverse()
            .ok_or("Transform must be finite and invertible")?
            .0;
        let source_stride = self.source_stride;
        let bytes = jobs.len() as u64 * 2 * u64::from(self.stride);
        let source_bytes = jobs.len() as u64 * u64::from(source_stride);
        let end = self.next_record + bytes;
        let source_end = self.source_next_record + source_bytes;
        if end.max(source_end) > device.limits().max_buffer_size.min(u64::from(u32::MAX)) {
            return Err("Too many transform regions");
        }
        let offsets = [self.next_record as u32, self.source_next_record as u32];
        if jobs.is_empty() {
            return Ok(offsets);
        }
        self.reserve_regions(device, end);
        if source_end > self.source_capacity {
            self.bindings.clear();
            self.binding_count = 0;
            self.source_capacity = source_end
                .next_power_of_two()
                .min(device.limits().max_buffer_size)
                .min(u64::from(u32::MAX))
                & !3;
            self.source_records = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("batched affine source records"),
                size: self.source_capacity,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        self.records.resize(bytes as usize, 0);
        self.source_upload.resize(source_bytes as usize, 0);
        for (i, job) in jobs.iter().enumerate() {
            let metadata = source_metadata(
                bounds,
                job.sources.iter().map(|c| {
                    (
                        c.map(|v| (v * super::PAGE_SIZE) as i32),
                        job.source_size,
                    )
                }),
            );
            for (dst, value) in self.source_upload[i * source_stride as usize..]
                [..SOURCE_RECORD_BYTES as usize]
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(metadata)
            {
                *dst = value.to_le_bytes();
            }
            for identity in [false, true] {
                let values = [
                    inverse[0],
                    inverse[1],
                    inverse[2],
                    inverse[3],
                    inverse[4],
                    inverse[5],
                    0.,
                    0.,
                    (job.target[0] * super::PAGE_SIZE) as f32,
                    (job.target[1] * super::PAGE_SIZE) as f32,
                    f32::from(transform.interpolation == Interpolation::Linear)
                        + 2. * f32::from(identity || affine == Affine::IDENTITY)
                        + 4. * f32::from(self.placement),
                    background,
                ];
                let offset = (i * 2 + usize::from(identity)) * self.stride as usize;
                for (dst, value) in self.records[offset..][..48]
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(values)
                {
                    *dst = value.to_le_bytes();
                }
            }
        }
        uploads
            .write_at(
                encoder,
                queue,
                &self.uniforms.as_ref().unwrap().0,
                self.next_record,
                &self.records,
            )
            .map_err(|_| "Could not upload transform uniforms")?;
        uploads
            .write_at(
                encoder,
                queue,
                &self.source_records,
                self.source_next_record,
                &self.source_upload,
            )
            .map_err(|_| "Could not upload transform sources")?;
        self.next_record = end;
        self.source_next_record = source_end;
        Ok(offsets)
    }
    pub fn encode_prepared(
        &self,
        encoder: &mut crate::submission::CommandEncoder,
        source: &TransformSource,
        offsets: [u32; 2],
        index: usize,
        identity: bool,
        target: &TransformTarget<'_>,
    ) {
        debug_assert!(valid_extent(target.origin, target.extent));
        debug_assert!(
            target.region[0] + target.region[2] <= target.extent[0]
                && target.region[1] + target.region[3] <= target.extent[1]
        );
        let source_stride = self.source_stride;
        self.draw(
            encoder,
            source,
            offsets[0] + (index as u32 * 2 + u32::from(identity)) * self.stride,
            offsets[1] + index as u32 * source_stride,
            target,
        );
    }
    fn draw(
        &self,
        encoder: &mut crate::submission::CommandEncoder,
        source: &TransformSource,
        region_offset: u32,
        source_offset: u32,
        target: &TransformTarget<'_>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("affine changed region"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.uniforms.as_ref().unwrap().1, &[region_offset]);
        pass.set_bind_group(1, &source.binding, &[source_offset]);
        let [x, y, w, h] = target.region;
        pass.set_scissor_rect(x, y, w, h);
        pass.draw(0..3, 0..1);
    }
    /// Retained scratch only; source/targets are owned by the transaction host.
    pub fn storage_bytes(&self) -> u64 {
        self.capacity + 48 + self.source_capacity
    }
}
fn source_metadata(
    bounds: [i32; 4],
    tiles: impl Iterator<Item = ([i32; 2], [u32; 2])>,
) -> [i32; (1 + TRANSFORM_SLOTS) * 4] {
    let mut metadata = [0; (1 + TRANSFORM_SLOTS) * 4];
    metadata[..4].copy_from_slice(&bounds);
    for (i, (origin, extent)) in tiles.enumerate() {
        metadata[(i + 1) * 4..(i + 2) * 4].copy_from_slice(&[
            origin[0],
            origin[1],
            extent[0] as i32,
            extent[1] as i32,
        ]);
    }
    metadata
}

fn source_shader() -> String {
    let mut shader = String::from(
        "struct SourceInfo { bounds:vec4<i32>, views:array<vec4<i32>,16> }\n@group(1) @binding(16) var<uniform> source_info:SourceInfo;\n",
    );
    for i in 0..TRANSFORM_SLOTS {
        shader += &format!("@group(1) @binding({i}) var source{i}:texture_2d<f32>;\n");
    }
    shader += "fn source_load(i:u32,p:vec2<i32>)->vec4<f32>{switch i {\n";
    for i in 0..TRANSFORM_SLOTS {
        shader += &format!("case {i}u:{{return textureLoad(source{i},p,0);}}\n");
    }
    shader += "default:{return vec4(0.);}}}\n";
    shader
}

fn valid_extent(origin: [i32; 2], extent: [u32; 2]) -> bool {
    // Preserve half-pixel centers as well as integer pixel boundaries in f32.
    origin.into_iter().zip(extent).all(|(o, n)| {
        n > 0 && i64::from(o).abs() <= 8_388_607 && (i64::from(o) + i64::from(n)).abs() <= 8_388_607
    })
}

