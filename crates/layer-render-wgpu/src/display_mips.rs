//! Derived display pixels. A reusable 256-pixel mip chain reduces completed
//! working tiles into a bounded coarse image; no editing consumer reads it.
use super::*;
use std::collections::BTreeMap;
use wgpu::util::DeviceExt;

const MIP_COUNT: u32 = PAGE_SIZE.ilog2() + 1;
pub(super) const MAX_SIDE: u32 = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Plan {
    pub extent: [u32; 2],
    pub size: [u32; 2],
    pub level: u32,
}
impl Plan {
    pub fn new(extent: [u32; 2]) -> Result<Self, GpuRasterError> {
        if extent.contains(&0) {
            return Err(GpuRasterError::InvalidExtent);
        }
        let level = (0..MIP_COUNT)
            .find(|level| {
                extent
                    .into_iter()
                    .all(|v| v.div_ceil(1 << level) <= MAX_SIDE)
            })
            .ok_or(GpuRasterError::ExtentUnsupported)?;
        Ok(Self {
            extent,
            size: extent.map(|v| v.div_ceil(1 << level)),
            level,
        })
    }
    pub fn pixel_bytes(self) -> u64 {
        self.pixel_bytes_through(self.level)
    }
    pub fn pixel_bytes_through(self, last: u32) -> u64 {
        let scratch = (0..=last)
            .map(|level| u64::from(PAGE_SIZE >> level).pow(2) * 16)
            .sum::<u64>();
        scratch + (self.level..=last).map(|level| {
            self.extent.map(|n| u64::from(n.div_ceil(1 << level))).into_iter().product::<u64>() * 16
        }).sum::<u64>()
    }
}

pub(super) struct Pipelines {
    layout: wgpu::BindGroupLayout,
    fused_layout: wgpu::BindGroupLayout,
    pub image_layout: wgpu::BindGroupLayout,
    pub reduce: Deferred<wgpu::ComputePipeline>,
    pub fused_reduce: Deferred<wgpu::ComputePipeline>,
}
impl Pipelines {
    pub fn new(device: &PipelineDevice) -> Self {
        let image_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("coarse display sampling"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(16),
                    },
                    count: None,
                },
            ],
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("display mip reduction"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: std::num::NonZeroU64::new(16),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba32Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("display mip reduction"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("display mip reduction"),
            source: wgpu::ShaderSource::Wgsl(include_str!("display_mips.wgsl").into()),
        });
        let fused_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("four display mip levels"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2, multisampled: false,
                    }, count: None,
                },
                native_tiles::buffer_entry(1, wgpu::BufferBindingType::Uniform, true, 16),
                mip_output_entry(2), mip_output_entry(3), mip_output_entry(4), mip_output_entry(5),
            ],
        });
        let fused_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("four display mip levels"), bind_group_layouts: &[Some(&fused_layout)], immediate_size: 0,
        });
        let fused_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("four display mip levels"),
            source: wgpu::ShaderSource::Wgsl(include_str!("display_mips_fused.wgsl").into()),
        });
        let fused_device = device.clone();
        let fused_reduce = Deferred::pipeline(move |mode| mode.compute(&fused_device,
            &wgpu::ComputePipelineDescriptor {
                label: Some("four display mip levels"), layout: Some(&fused_pipeline_layout),
                module: &fused_shader, entry_point: Some("reduce_four"),
                compilation_options: Default::default(), cache: None,
            }));
        let device = device.clone();
        let reduce = Deferred::pipeline(move |mode| {
            mode.compute(
                &device,
                &wgpu::ComputePipelineDescriptor {
                    label: Some("display mip reduction"),
                    layout: Some(&pipeline_layout),
                    module: &shader,
                    entry_point: Some("reduce"),
                    compilation_options: Default::default(),
                    cache: None,
                },
            )
        });
        Self {
            layout,
            fused_layout,
            image_layout,
            reduce,
            fused_reduce,
        }
    }
}

fn mip_output_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding, visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format: wgpu::TextureFormat::Rgba32Float,
            view_dimension: wgpu::TextureViewDimension::D2,
        }, count: None,
    }
}

struct Record {
    uniform: wgpu::Buffer,
    binding: wgpu::BindGroup,
}

/// Update an already admitted complete pyramid in place. Immutable coordinates
/// avoid per-frame uniform allocations and remain valid across submissions.
/// Each dispatch writes one tile region; each level reads the completed finer
/// level. The same reduction kernel also serves the bounded scratch path.
pub(super) struct CompleteUpdates {
    records: wgpu::Buffer,
    bindings: Vec<wgpu::BindGroup>,
    fused_bindings: Vec<wgpu::BindGroup>,
    pipeline: Deferred<wgpu::ComputePipeline>,
    fused_pipeline: Deferred<wgpu::ComputePipeline>,
    columns: u32,
    stride: u32,
    pending: Vec<[u32; 2]>,
}
impl CompleteUpdates {
    pub(super) const BATCH: usize = 128;

    pub fn record_bytes(device: &wgpu::Device, plan: Plan) -> u64 {
        u64::from(plan.extent[0].div_ceil(PAGE_SIZE))
            * u64::from(plan.extent[1].div_ceil(PAGE_SIZE))
            * u64::from(plan.level)
            * u64::from(device.limits().min_uniform_buffer_offset_alignment.max(16))
    }

    pub fn new(device: &PipelineDevice, pipelines: &Pipelines, plan: Plan,
        views: &[&wgpu::TextureView]) -> Self {
        assert_eq!(views.len(), plan.level as usize + 1);
        let stride = device.limits().min_uniform_buffer_offset_alignment.max(16);
        let columns = plan.extent[0].div_ceil(PAGE_SIZE);
        let mut bytes = vec![0; Self::record_bytes(device, plan) as usize];
        for y in 0..plan.extent[1].div_ceil(PAGE_SIZE) {
            for x in 0..columns {
                for level in 1..=plan.level {
                    let offset = ((y * columns + x) * plan.level + level - 1) * stride;
                    for (i, value) in [plan.extent[0], plan.extent[1], 1 << (level - 1), x | (y << 16)]
                        .into_iter().enumerate() {
                        bytes[offset as usize + i * 4..offset as usize + i * 4 + 4]
                            .copy_from_slice(&value.to_le_bytes());
                    }
                }
            }
        }
        let records = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("retained display tile coordinates"), contents: &bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bindings = views.windows(2).map(|pair| device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("retained display reduction"), layout: &pipelines.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(pair[0]) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &records, offset: 0, size: NonZeroU64::new(16),
                }) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(pair[1]) },
            ],
        })).collect();
        let fused_bindings = (0..plan.level as usize / 4).map(|chunk| {
            let first = chunk * 4;
            let mut entries = vec![
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(views[first]) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &records, offset: 0, size: NonZeroU64::new(16),
                }) },
            ];
            entries.extend((0..4).map(|i| wgpu::BindGroupEntry {
                binding: i as u32 + 2, resource: wgpu::BindingResource::TextureView(views[first + i + 1]),
            }));
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("four retained display mip levels"), layout: &pipelines.fused_layout, entries: &entries,
            })
        }).collect();
        Self { records, bindings, fused_bindings, pipeline: pipelines.reduce.clone(),
            fused_pipeline: pipelines.fused_reduce.clone(), columns, stride,
            pending: Vec::with_capacity(Self::BATCH) }
    }

    pub fn storage_bytes(&self) -> u64 { self.records.size() }

    pub fn discard_pending(&mut self) { self.pending.clear(); }

    pub fn tile(&mut self, encoder: &mut crate::submission::CommandEncoder, coordinate: [u32; 2]) {
        self.pending.push(coordinate);
        if self.pending.len() == Self::BATCH { self.flush(encoder); }
    }

    pub fn flush(&mut self, encoder: &mut crate::submission::CommandEncoder) {
        if self.pending.is_empty() { return; }
        let _trace = crate::performance_trace::Span::new(c"capy.mip_encode");
        // Same pixels and per-level dependency order, fewer driver commands.
        self.pending.sort_unstable_by_key(|&[x, y]| (y, x));
        self.pending.dedup();
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("reduce retained display tiles"), timestamp_writes: None,
        });
        let mut level = 0;
        while level < self.bindings.len() {
            let fused = level % 4 == 0 && level / 4 < self.fused_bindings.len();
            let binding = if fused { &self.fused_bindings[level / 4] } else { &self.bindings[level] };
            pass.set_pipeline(if fused { &self.fused_pipeline } else { &self.pipeline });
            let side = PAGE_SIZE >> (level + 1);
            let mut first = 0;
            while first < self.pending.len() {
                let [x, y] = self.pending[first];
                let mut end = first + 1;
                while end < self.pending.len()
                    && self.pending[end] == [x + (end - first) as u32, y] {
                    end += 1;
                }
                let offset = ((y * self.columns + x) * self.bindings.len() as u32 + level as u32) * self.stride;
                pass.set_bind_group(0, binding, &[offset]);
                pass.dispatch_workgroups(side.div_ceil(8), side.div_ceil(8), (end - first) as u32);
                first = end;
            }
            level += if fused { 4 } else { 1 };
        }
        self.pending.clear();
    }
}

pub(super) struct Image {
    pub plan: Plan,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub binding: wgpu::BindGroup,
    geometry: wgpu::Buffer,
    scratch: wgpu::Texture,
    views: Vec<wgpu::TextureView>,
    reduced: Vec<(wgpu::Texture, wgpu::TextureView)>,
    // An image has only four combinations of full/partial tile dimensions.
    // Immutable records preserve command order when the scratch tile is reused.
    records: BTreeMap<[u32; 2], Vec<Record>>,
}
impl Image {
    pub fn new(r: &WgpuRasterizer, pipelines: &Pipelines, plan: Plan) -> Self {
        Self::with_mips(r, pipelines, plan, plan.level)
    }
    /// Retain optional coarser levels from the same tile reduction. Consumers
    /// select an existing level; source pixels and editing precision are intact.
    pub fn with_mips(r: &WgpuRasterizer, pipelines: &Pipelines, plan: Plan, last: u32) -> Self {
        assert!(last >= plan.level && last < MIP_COUNT);
        let (texture, view) = create_target(
            &r.device,
            plan.size,
            wgpu::TextureFormat::Rgba32Float,
            "coarse display image",
        );
        let data = [plan.extent[0], plan.extent[1], 1 << plan.level, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        let geometry = r
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("coarse display geometry"),
                contents: &data,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let binding = r.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("coarse display image"),
            layout: &pipelines.image_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: geometry.as_entire_binding(),
                },
            ],
        });
        let scratch = r.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("reusable display mip tile"),
            size: wgpu::Extent3d {
                width: PAGE_SIZE,
                height: PAGE_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: last + 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let views = (0..=last)
            .map(|level| {
                scratch.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("single display mip"),
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let reduced = (plan.level + 1..=last).map(|level| create_target(
            &r.device, plan.extent.map(|n| n.div_ceil(1 << level)),
            wgpu::TextureFormat::Rgba32Float, "retained image mip",
        )).collect();
        Self {
            plan,
            texture,
            view,
            binding,
            geometry,
            scratch,
            views,
            reduced,
            records: BTreeMap::new(),
        }
    }
    pub fn storage_bytes(&self) -> u64 {
        self.plan.pixel_bytes_through(self.last_level())
            + self.geometry.size()
            + self
                .records
                .values()
                .flatten()
                .map(|r| r.uniform.size())
                .sum::<u64>()
    }
    pub fn last_level(&self) -> u32 { self.views.len() as u32 - 1 }
    pub fn sample(&self, requested: u32) -> (u32, &wgpu::TextureView, [u32; 2]) {
        let level = requested.clamp(self.plan.level, self.last_level());
        let view = if level == self.plan.level { &self.view }
            else { &self.reduced[(level - self.plan.level - 1) as usize].1 };
        (level, view, self.plan.extent.map(|n| n.div_ceil(1 << level)))
    }
    pub fn copy_mip(
        &self,
        encoder: &mut crate::submission::CommandEncoder,
        level: u32,
        coordinate: [u32; 2],
        destination: &wgpu::Texture,
        origin: [u32; 2],
    ) {
        assert!(level <= self.last_level());
        let valid: [u32; 2] = std::array::from_fn(|i| {
            (self.plan.extent[i] - coordinate[i] * PAGE_SIZE)
                .min(PAGE_SIZE)
                .div_ceil(1 << level)
        });
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                mip_level: level,
                ..self.scratch.as_image_copy()
            },
            wgpu::TexelCopyTextureInfo {
                origin: wgpu::Origin3d {
                    x: origin[0],
                    y: origin[1],
                    z: 0,
                },
                ..destination.as_image_copy()
            },
            wgpu::Extent3d {
                width: valid[0],
                height: valid[1],
                depth_or_array_layers: 1,
            },
        );
    }
    pub fn tile_target(&self) -> (&wgpu::Texture, &wgpu::TextureView) {
        (&self.scratch, &self.views[0])
    }

    /// Consume one completed full-resolution composition tile. `source_origin`
    /// supports both a temporary tile and the current full composite during its
    /// replacement. Every output pixel averages all valid document pixels in its
    /// power-of-two footprint, including partial right/bottom edge footprints.
    pub fn write_tile(
        &mut self,
        device: &PipelineDevice,
        pipelines: &Pipelines,
        encoder: &mut crate::submission::CommandEncoder,
        source: &wgpu::Texture,
        source_origin: [u32; 2],
        coordinate: [u32; 2],
    ) -> Result<(), GpuRasterError> {
        if coordinate
            .into_iter()
            .zip(self.plan.extent)
            .any(|(c, n)| c >= n.div_ceil(PAGE_SIZE))
        {
            return Err(GpuRasterError::InvalidExtent);
        }
        let origin = coordinate.map(|v| v * PAGE_SIZE);
        let valid = std::array::from_fn(|i| (self.plan.extent[i] - origin[i]).min(PAGE_SIZE));
        if source.format() != wgpu::TextureFormat::Rgba32Float
            || (source == &self.scratch && source_origin != [0; 2])
            || source_origin
                .into_iter()
                .zip(valid)
                .zip([source.width(), source.height()])
                .any(|((start, n), size)| start.checked_add(n).is_none_or(|end| end > size))
        {
            return Err(GpuRasterError::InvalidExtent);
        }
        if source != &self.scratch {
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    origin: wgpu::Origin3d {
                        x: source_origin[0],
                        y: source_origin[1],
                        z: 0,
                    },
                    ..source.as_image_copy()
                },
                self.scratch.as_image_copy(),
                wgpu::Extent3d {
                    width: valid[0],
                    height: valid[1],
                    depth_or_array_layers: 1,
                },
            );
        }
        let last = self.last_level();
        let records = self.records.entry(valid).or_insert_with(|| {
            (1..=last)
                .map(|level| {
                    let data = [valid[0], valid[1], 1 << (level - 1), 0]
                        .into_iter()
                        .flat_map(u32::to_le_bytes)
                        .collect::<Vec<_>>();
                    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("display mip footprint"),
                        contents: &data,
                        usage: wgpu::BufferUsages::UNIFORM,
                    });
                    let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("display mip source"),
                        layout: &pipelines.layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(
                                    &self.views[level as usize - 1],
                                ),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: uniform.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: wgpu::BindingResource::TextureView(&self.views[level as usize]),
                            },
                        ],
                    });
                    Record { uniform, binding }
                })
                .collect()
        });
        if !records.is_empty() {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("reduce display tile"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipelines.reduce);
            for (index, record) in records.iter().enumerate() {
                pass.set_bind_group(0, &record.binding, &[0]);
                let side = PAGE_SIZE >> (index + 1);
                pass.dispatch_workgroups(side.div_ceil(8), side.div_ceil(8), 1);
            }
        }
        let scale = 1 << self.plan.level;
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                mip_level: self.plan.level,
                ..self.scratch.as_image_copy()
            },
            wgpu::TexelCopyTextureInfo {
                origin: wgpu::Origin3d {
                    x: origin[0] / scale,
                    y: origin[1] / scale,
                    z: 0,
                },
                ..self.texture.as_image_copy()
            },
            wgpu::Extent3d {
                width: valid[0].div_ceil(scale),
                height: valid[1].div_ceil(scale),
                depth_or_array_layers: 1,
            },
        );
        for (index, (texture, _)) in self.reduced.iter().enumerate() {
            let level = self.plan.level + index as u32 + 1;
            self.copy_mip(encoder, level, coordinate, texture, origin.map(|v| v >> level));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
