//! Queue-ordered adoption of canonical working samples. The publication-wide
//! status is read on the GPU after every color/scalar encoding batch, so no
//! input-owner readback is needed before subsequent edits see committed values.
use super::{MAX_BATCH_TILES, NativeEncodeStatus, STATUS_BYTES};
use crate::{GpuRasterError, PipelineDevice};

pub struct NativePromotion<'a> {
    pub canonical: &'a wgpu::Texture,
    pub working: &'a wgpu::Texture,
    /// x, y, width, height, in tile pixels. Pixels outside remain untouched.
    pub region: [u32; 4],
}

struct Job {
    binding: wgpu::BindGroup,
    target: wgpu::TextureView,
    format: usize,
    region: [u32; 4],
}
pub struct NativePromotionBatch(Vec<Job>);
impl NativePromotionBatch {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub(crate) fn pass_count(&self) -> usize {
        self.0.len()
    }
}

pub struct NativePromoter {
    layout: wgpu::BindGroupLayout,
    pipelines: [wgpu::RenderPipeline; 2],
}
impl NativePromoter {
    /// Prepare with native encoding pipelines before interaction. Existing
    /// working attachments need no additional storage-texture usage.
    pub fn new(device: &wgpu::Device) -> Self {
        Self::with_device(&device.clone().into())
    }
    pub(crate) fn with_device(device: &PipelineDevice) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("native canonical promotion"),
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
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(STATUS_BYTES),
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("native canonical promotion"),
            source: wgpu::ShaderSource::Wgsl(include_str!("promote.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("native canonical promotion"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipelines = [
            wgpu::TextureFormat::Rgba32Float,
            wgpu::TextureFormat::R32Float,
        ]
        .map(|format| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("native canonical promotion"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
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
        Self { layout, pipelines }
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
                    .contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
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
        let jobs = requests
            .iter()
            .filter(|r| r.region[2] != 0 && r.region[3] != 0)
            .map(|r| {
                let view = r.canonical.create_view(&Default::default());
                let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("native canonical promotion"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: status.buffer().as_entire_binding(),
                        },
                    ],
                });
                Job {
                    binding,
                    target: r.working.create_view(&Default::default()),
                    format: usize::from(r.working.format() == wgpu::TextureFormat::R32Float),
                    region: r.region,
                }
            })
            .collect();
        Ok(NativePromotionBatch(jobs))
    }

    pub fn encode(&self, commands: &mut wgpu::CommandEncoder, batch: &NativePromotionBatch) {
        for job in &batch.0 {
            let mut pass = commands.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("native canonical promotion"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &job.target,
                    resolve_target: None,
                    depth_slice: None,
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
            pass.set_pipeline(&self.pipelines[job.format]);
            pass.set_bind_group(0, &job.binding, &[]);
            let [x, y, width, height] = job.region;
            pass.set_scissor_rect(x, y, width, height);
            pass.draw(0..3, 0..1);
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
