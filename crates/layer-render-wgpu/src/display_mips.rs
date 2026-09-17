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
        let scratch = (0..=self.level)
            .map(|level| u64::from(PAGE_SIZE >> level).pow(2) * 16)
            .sum::<u64>();
        u64::from(self.size[0]) * u64::from(self.size[1]) * 16 + scratch
    }
}

pub(super) struct Pipelines {
    layout: wgpu::BindGroupLayout,
    pub image_layout: wgpu::BindGroupLayout,
    pub reduce: Deferred<wgpu::ComputePipeline>,
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
                        has_dynamic_offset: false,
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
        let device = device.clone();
        let reduce = Deferred::new(move || {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("display mip reduction"),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some("reduce"),
                compilation_options: Default::default(),
                cache: None,
            })
        });
        Self {
            layout,
            image_layout,
            reduce,
        }
    }
}

struct Record {
    uniform: wgpu::Buffer,
    binding: wgpu::BindGroup,
}
pub(super) struct Image {
    pub plan: Plan,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub binding: wgpu::BindGroup,
    geometry: wgpu::Buffer,
    scratch: wgpu::Texture,
    views: Vec<wgpu::TextureView>,
    // An image has only four combinations of full/partial tile dimensions.
    // Immutable records preserve command order when the scratch tile is reused.
    records: BTreeMap<[u32; 2], Vec<Record>>,
}
impl Image {
    pub fn new(r: &WgpuRasterizer, pipelines: &Pipelines, plan: Plan) -> Self {
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
            mip_level_count: plan.level + 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let views = (0..=plan.level)
            .map(|level| {
                scratch.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("single display mip"),
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        Self {
            plan,
            texture,
            view,
            binding,
            geometry,
            scratch,
            views,
            records: BTreeMap::new(),
        }
    }
    pub fn storage_bytes(&self) -> u64 {
        self.plan.pixel_bytes()
            + self.geometry.size()
            + self
                .records
                .values()
                .flatten()
                .map(|r| r.uniform.size())
                .sum::<u64>()
    }
    pub fn copy_mip(
        &self,
        encoder: &mut crate::submission::CommandEncoder,
        level: u32,
        coordinate: [u32; 2],
        destination: &wgpu::Texture,
        origin: [u32; 2],
    ) {
        assert!(level <= self.plan.level);
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
            || source_origin
                .into_iter()
                .zip(valid)
                .zip([source.width(), source.height()])
                .any(|((start, n), size)| start.checked_add(n).is_none_or(|end| end > size))
        {
            return Err(GpuRasterError::InvalidExtent);
        }
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
        let records = self.records.entry(valid).or_insert_with(|| {
            (1..=self.plan.level)
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
                pass.set_bind_group(0, &record.binding, &[]);
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
        Ok(())
    }
}

#[cfg(test)]
mod tests;
