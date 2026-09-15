//! Queue-ordered adoption of canonical working samples. The publication-wide
//! status is read on the GPU after every color/scalar encoding batch, so no
//! input-owner readback is needed before subsequent edits see committed values.
use super::{MAX_BATCH_TILES, NativeEncodeStatus, STATUS_BYTES};
use crate::{GpuRasterError, PipelineDevice};
use wgpu::util::DeviceExt;

const FULL_REGION: [u32; 4] = [0, 0, 256, 256];

pub struct NativePromotion<'a> {
    pub canonical: &'a wgpu::Texture,
    pub working: &'a wgpu::Texture,
    /// x, y, width, height, in tile pixels. Pixels outside remain untouched.
    pub region: [u32; 4],
}

struct Job {
    binding: wgpu::BindGroup,
    format: usize,
    groups: [u32; 2],
}
pub struct NativePromotionBatch(Vec<Job>);
impl NativePromotionBatch {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub(crate) fn pass_count(&self) -> usize {
        usize::from(!self.is_empty())
    }
}

pub struct NativePromoter {
    layouts: [wgpu::BindGroupLayout; 2],
    pipelines: [wgpu::ComputePipeline; 2],
    full_region: wgpu::Buffer,
}
impl NativePromoter {
    pub(crate) fn storage_bytes(&self) -> u64 {
        self.full_region.size()
    }

    /// Prepare with native encoding pipelines before interaction. Working
    /// destinations require write-only storage access in their Float32 format.
    pub fn new(device: &wgpu::Device) -> Self {
        Self::with_device(&device.clone().into())
    }
    pub(crate) fn with_device(device: &PipelineDevice) -> Self {
        let formats = [
            wgpu::TextureFormat::Rgba32Float,
            wgpu::TextureFormat::R32Float,
        ];
        let layouts = formats.map(|format| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("native canonical promotion"),
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
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: std::num::NonZeroU64::new(STATUS_BYTES),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format,
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                    super::buffer_entry(3, wgpu::BufferBindingType::Uniform, false, 16),
                ],
            })
        });
        let pipelines = std::array::from_fn(|index| {
            let source = include_str!("promote.wgsl").replace(
                "FORMAT",
                if index == 0 {
                    "rgba32float"
                } else {
                    "r32float"
                },
            );
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("native canonical promotion"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("native canonical promotion"),
                bind_group_layouts: &[Some(&layouts[index])],
                immediate_size: 0,
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("native canonical promotion"),
                layout: Some(&layout),
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            })
        });
        // Whole-tile publication is the interactive path. Reuse this immutable
        // record instead of allocating parameter uploads for each commit chunk.
        let full_region = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("native full-tile promotion region"),
            contents: FULL_REGION.map(u32::to_le_bytes).as_flattened(),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        Self {
            layouts,
            pipelines,
            full_region,
        }
    }

    /// Preflight the whole batch before recording writes. All canonical inputs
    /// and working destinations must have distinct ownership across the whole
    /// publication; this checks aliases within this batch. Reset status once,
    /// encode every color/scalar batch, then promote every batch before resetting
    /// status again. A failed status leaves all destination pixels unchanged.
    /// The caller still handles failure/recovery of the provisional edit and
    /// publishes CPU backing only after the same status succeeds in capture.
    pub fn prepare(
        &self,
        device: &wgpu::Device,
        requests: &[NativePromotion<'_>],
        status: &NativeEncodeStatus,
    ) -> Result<NativePromotionBatch, GpuRasterError> {
        self.prepare_with_views(device, requests, status, &mut Default::default())
    }
    pub(crate) fn prepare_with_views(
        &self,
        device: &wgpu::Device,
        requests: &[NativePromotion<'_>],
        status: &NativeEncodeStatus,
        views: &mut crate::native_tiles::PublicationViews,
    ) -> Result<NativePromotionBatch, GpuRasterError> {
        if requests.len() > MAX_BATCH_TILES {
            return Err(GpuRasterError::Color("Too many native promotions".into()));
        }
        for (i, r) in requests.iter().enumerate() {
            let valid = |t: &wgpu::Texture| {
                t.size()
                    == (wgpu::Extent3d {
                        width: 256,
                        height: 256,
                        depth_or_array_layers: 1,
                    })
                    && t.dimension() == wgpu::TextureDimension::D2
                    && t.mip_level_count() == 1
                    && t.sample_count() == 1
                    && matches!(
                        t.format(),
                        wgpu::TextureFormat::Rgba32Float | wgpu::TextureFormat::R32Float
                    )
            };
            let [x, y, width, height] = r.region;
            if !valid(r.working)
                || !valid(r.canonical)
                || r.working.format() != r.canonical.format()
                || !r
                    .working
                    .usage()
                    .contains(wgpu::TextureUsages::STORAGE_BINDING)
                || !r
                    .canonical
                    .usage()
                    .contains(wgpu::TextureUsages::TEXTURE_BINDING)
                || x > 256
                || y > 256
                || width > 256 - x
                || height > 256 - y
                || r.working == r.canonical
                || requests[..i].iter().any(|old| {
                    old.working == r.working
                        || old.canonical == r.canonical
                        || old.working == r.canonical
                        || old.canonical == r.working
                })
            {
                return Err(GpuRasterError::Color(
                    "Invalid or aliased native promotion".into(),
                ));
            }
        }
        if requests
            .iter()
            .all(|r| r.region[2] == 0 || r.region[3] == 0)
        {
            return Ok(NativePromotionBatch(Vec::new()));
        }
        let full = requests.iter().all(|r| r.region == FULL_REGION);
        let stride = 16u32.next_multiple_of(device.limits().min_uniform_buffer_offset_alignment);
        let parameters = if full {
            self.full_region.clone()
        } else {
            let parameters = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("batched native promotion regions"),
                size: u64::from(stride) * requests.len() as u64,
                usage: wgpu::BufferUsages::UNIFORM,
                mapped_at_creation: true,
            });
            {
                let mut bytes = parameters
                    .get_mapped_range_mut(..)
                    .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?;
                for (i, r) in requests.iter().enumerate() {
                    bytes
                        .slice(i * stride as usize..i * stride as usize + 16)
                        .copy_from_slice(r.region.map(u32::to_le_bytes).as_flattened());
                }
            }
            parameters.unmap();
            parameters
        };
        let jobs = requests
            .iter()
            .enumerate()
            .filter(|(_, r)| r.region[2] != 0 && r.region[3] != 0)
            .map(|(i, r)| {
                let format = usize::from(r.working.format() == wgpu::TextureFormat::R32Float);
                let view = views.get(r.canonical);
                let target = views.get(r.working);
                let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("native canonical promotion"),
                    layout: &self.layouts[format],
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: status.buffer().as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&target),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &parameters,
                                offset: if full {
                                    0
                                } else {
                                    i as u64 * u64::from(stride)
                                },
                                size: wgpu::BufferSize::new(16),
                            }),
                        },
                    ],
                });
                Job {
                    binding,
                    format,
                    groups: [r.region[2].div_ceil(8), r.region[3].div_ceil(8)],
                }
            })
            .collect();
        Ok(NativePromotionBatch(jobs))
    }

    pub fn encode(&self, commands: &mut wgpu::CommandEncoder, batch: &NativePromotionBatch) {
        if batch.is_empty() {
            return;
        }
        let mut pass = commands.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("batched native canonical promotion"),
            timestamp_writes: None,
        });
        for job in &batch.0 {
            pass.set_pipeline(&self.pipelines[job.format]);
            pass.set_bind_group(0, &job.binding, &[]);
            pass.dispatch_workgroups(job.groups[0], job.groups[1], 1);
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
