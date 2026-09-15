use super::*;

/// Scan every dirty page before reusing any canonical scratch slot. Inputs do
/// not change between this scan and encoding except for their own promotion.
pub(super) struct Validator {
    layout: wgpu::BindGroupLayout,
    pipelines: [wgpu::ComputePipeline; 2],
}
impl Validator {
    pub fn new(device: &PipelineDevice) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("native publication validation"),
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
                crate::native_tiles::buffer_entry(
                    1,
                    wgpu::BufferBindingType::Storage { read_only: false },
                    false,
                    STATUS_BYTES,
                ),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("native publication validation"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipelines = ["color_error(value)", "scalar_error(value.r)"].map(|expression| {
            let source = format!(
                "{}\n{}",
                include_str!("../../native_tiles/validity.wgsl"),
                include_str!("validate.wgsl").replace("VALIDATE", expression)
            );
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("native publication validation"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("native publication validation"),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            })
        });
        Self { layout, pipelines }
    }
    pub fn encode(
        &self,
        r: &WgpuRasterizer,
        encoder: &mut submission::CommandEncoder,
        inputs: &[(&wgpu::Texture, RasterTile)],
        status: &NativeEncodeStatus,
        views: &mut crate::native_tiles::PublicationViews,
    ) -> Result<(), GpuRasterError> {
        // Reject unexpected/aliased live storage before recording any promotion.
        let mut identities = std::collections::HashSet::new();
        for (texture, _) in inputs {
            if !identities.insert((*texture).clone())
                || texture.size()
                    != (wgpu::Extent3d {
                        width: 256,
                        height: 256,
                        depth_or_array_layers: 1,
                    })
                || texture.mip_level_count() != 1
                || texture.sample_count() != 1
                || texture.dimension() != wgpu::TextureDimension::D2
                || !matches!(
                    texture.format(),
                    wgpu::TextureFormat::Rgba32Float | wgpu::TextureFormat::R32Float
                )
                || !texture.usage().contains(
                    wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
                )
            {
                return Err(GpuRasterError::Color(
                    "Invalid native working ownership or layout".into(),
                ));
            }
        }
        for chunk in inputs.chunks(MAX_BATCH_TILES) {
            let jobs: Vec<_> = chunk
                .iter()
                .map(|(texture, _)| {
                    let view = views.get(texture);
                    let binding = r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("native publication validation"),
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
                    (
                        binding,
                        usize::from(texture.format() == wgpu::TextureFormat::R32Float),
                    )
                })
                .collect();
            let mut pass = encoder.begin_compute_pass(&Default::default());
            for (binding, format) in &jobs {
                pass.set_pipeline(&self.pipelines[*format]);
                pass.set_bind_group(0, binding, &[]);
                pass.dispatch_workgroups(32, 32, 1);
            }
        }
        Ok(())
    }
}
