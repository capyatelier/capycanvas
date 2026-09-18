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
    groups: [u32; 3],
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
    layouts: Vec<wgpu::BindGroupLayout>,
    pub(crate) pipelines: Vec<crate::Deferred<wgpu::ComputePipeline>>,
    tiles_per_dispatch: usize,
    full_region: wgpu::Buffer,
}
impl NativePromoter {
    pub(crate) fn storage_bytes(&self) -> u64 {
        self.full_region.size()
    }

    /// Prepare with native encoding pipelines before interaction. Working
    /// destinations require write-only storage access in their Float32 format.
    pub fn new(device: &wgpu::Device) -> Self {
        let encoder = Self::with_device(&device.clone().into());
        for pipeline in &encoder.pipelines {
            pipeline.compile();
        }
        encoder
    }
    pub(crate) fn with_device(device: &PipelineDevice) -> Self {
        let formats = [
            wgpu::TextureFormat::Rgba32Float,
            wgpu::TextureFormat::R32Float,
        ];
        let tiles_per_dispatch = 4usize
            .min(device.limits().max_sampled_textures_per_shader_stage as usize)
            .min(device.limits().max_storage_textures_per_shader_stage as usize);
        assert!(tiles_per_dispatch > 0);
        let mut layouts = Vec::new();
        let mut pipelines = Vec::new();
        for (format, name) in formats.into_iter().zip(["rgba32float", "r32float"]) {
            // Exact-size layouts avoid unused writable aliases and dummy pixel
            // allocations when the final group has fewer than four tiles.
            for count in 1..=tiles_per_dispatch {
                let mut entries = Vec::new();
                let mut textures = String::new();
                let mut copies = String::new();
                for i in 0..count as u32 {
                    entries.push(wgpu::BindGroupLayoutEntry {
                        binding: i * 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    });
                    entries.push(wgpu::BindGroupLayoutEntry {
                        binding: i * 2 + 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format,
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    });
                    textures.push_str(&format!(
                        "@group(0) @binding({}) var canonical{i}:texture_2d<f32>;\n@group(0) @binding({}) var working{i}:texture_storage_2d<{name},write>;\n",
                        i * 2, i * 2 + 1,
                    ));
                    copies.push_str(&format!("case {i}u: {{ textureStore(working{i},p,textureLoad(canonical{i},p,0)); }}\n"));
                }
                let status_binding = count as u32 * 2;
                entries.push(super::buffer_entry(
                    status_binding,
                    wgpu::BufferBindingType::Storage { read_only: true },
                    false,
                    STATUS_BYTES,
                ));
                entries.push(super::buffer_entry(
                    status_binding + 1,
                    wgpu::BufferBindingType::Uniform,
                    false,
                    16,
                ));
                let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("native canonical promotion"),
                    entries: &entries,
                });
                let source = include_str!("promote.wgsl")
                    .replace("TEXTURES", &textures)
                    .replace("COPY_TILES", &copies)
                    .replace("STATUS_BINDING", &status_binding.to_string())
                    .replace("REGION_BINDING", &(status_binding + 1).to_string());
                let pipeline_layout =
                    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("native canonical promotion"),
                        bind_group_layouts: &[Some(&layout)],
                        immediate_size: 0,
                    });
                pipelines.push({
                    let (device, pipeline_layout) = (device.clone(), pipeline_layout.clone());
                    crate::Deferred::pipeline(move |mode| {
                        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                            label: Some("native canonical promotion"),
                            source: wgpu::ShaderSource::Wgsl(source.into()),
                        });
                        mode.compute(
                            &device,
                            &wgpu::ComputePipelineDescriptor {
                                label: Some("native canonical promotion"),
                                layout: Some(&pipeline_layout),
                                module: &shader,
                                entry_point: Some("main"),
                                compilation_options: Default::default(),
                                cache: None,
                            },
                        )
                    })
                });
                layouts.push(layout);
            }
        }
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
            tiles_per_dispatch,
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
        let mut jobs = Vec::new();
        let mut first = 0;
        while first < requests.len() {
            let r = &requests[first];
            if r.region[2] == 0 || r.region[3] == 0 {
                first += 1;
                continue;
            }
            let count = requests[first..]
                .iter()
                .take(self.tiles_per_dispatch)
                .take_while(|next| {
                    next.region == r.region && next.working.format() == r.working.format()
                })
                .count();
            let format = usize::from(r.working.format() == wgpu::TextureFormat::R32Float)
                * self.tiles_per_dispatch
                + count
                - 1;
            let tile_views: Vec<_> = requests[first..first + count]
                .iter()
                .map(|r| [views.get(r.canonical), views.get(r.working)])
                .collect();
            let mut entries: Vec<_> = tile_views
                .iter()
                .flatten()
                .enumerate()
                .map(|(i, view)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: wgpu::BindingResource::TextureView(view),
                })
                .collect();
            entries.push(wgpu::BindGroupEntry {
                binding: count as u32 * 2,
                resource: status.buffer().as_entire_binding(),
            });
            entries.push(wgpu::BindGroupEntry {
                binding: count as u32 * 2 + 1,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &parameters,
                    offset: if full {
                        0
                    } else {
                        first as u64 * u64::from(stride)
                    },
                    size: wgpu::BufferSize::new(16),
                }),
            });
            let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("native canonical promotion"),
                layout: &self.layouts[format],
                entries: &entries,
            });
            jobs.push(Job {
                binding,
                format,
                groups: [
                    r.region[2].div_ceil(8),
                    r.region[3].div_ceil(8),
                    count as u32,
                ],
            });
            first += count;
        }
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
            pass.dispatch_workgroups(job.groups[0], job.groups[1], job.groups[2]);
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
