use super::*;

/// Scan every dirty page before reusing any canonical scratch slot. Inputs do
/// not change between this scan and encoding except for their own promotion.
pub(super) struct Validator {
    layout: wgpu::BindGroupLayout,
    pipelines: [wgpu::ComputePipeline; 2],
    tiles_per_dispatch: usize,
}
impl Validator {
    pub fn new(device: &PipelineDevice) -> Self {
        let tiles_per_dispatch =
            MAX_BATCH_TILES.min(device.limits().max_sampled_textures_per_shader_stage as usize);
        assert!(tiles_per_dispatch > 0);
        let mut entries: Vec<_> = (0..tiles_per_dispatch)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding: binding as u32,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            })
            .collect();
        entries.push(crate::native_tiles::buffer_entry(
            tiles_per_dispatch as u32,
            wgpu::BufferBindingType::Storage { read_only: false },
            false,
            STATUS_BYTES,
        ));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("native publication validation"),
            entries: &entries,
        });
        let textures: String = (0..tiles_per_dispatch)
            .map(|i| format!("@group(0) @binding({i}) var working{i}:texture_2d<f32>;\n"))
            .collect();
        let loads: String = (0..tiles_per_dispatch)
            .map(|i| {
                format!(
                    "case {i}u: {{ value=textureLoad(working{i},vec2<i32>(invocation.xy),0); }}\n"
                )
            })
            .collect();
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("native publication validation"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipelines = ["color_error(value)", "scalar_error(value.r)"].map(|expression| {
            let source = format!(
                "{}\n{}",
                include_str!("../../native_tiles/validity.wgsl"),
                include_str!("validate.wgsl")
                    .replace("TEXTURES", &textures)
                    .replace("LOADS", &loads)
                    .replace("STATUS_BINDING", &tiles_per_dispatch.to_string())
                    .replace("VALIDATE", expression)
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
        Self {
            layout,
            pipelines,
            tiles_per_dispatch,
        }
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
        // Group read-only inputs by their validity rule. Each z workgroup sees
        // one complete tile. Repeated padding views are never dispatched and
        // add no writable aliases; the same status accumulates across all groups.
        for (format, texture_format) in [
            wgpu::TextureFormat::Rgba32Float,
            wgpu::TextureFormat::R32Float,
        ]
        .into_iter()
        .enumerate()
        {
            let selected: Vec<_> = inputs
                .iter()
                .filter(|(texture, _)| texture.format() == texture_format)
                .map(|(texture, _)| *texture)
                .collect();
            for chunk in selected.chunks(self.tiles_per_dispatch) {
                let tile_views: Vec<_> = chunk.iter().map(|texture| views.get(texture)).collect();
                let mut entries: Vec<_> = (0..self.tiles_per_dispatch)
                    .map(|index| wgpu::BindGroupEntry {
                        binding: index as u32,
                        resource: wgpu::BindingResource::TextureView(
                            tile_views.get(index).unwrap_or(&tile_views[0]),
                        ),
                    })
                    .collect();
                entries.push(wgpu::BindGroupEntry {
                    binding: self.tiles_per_dispatch as u32,
                    resource: status.buffer().as_entire_binding(),
                });
                let binding = r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("native publication validation"),
                    layout: &self.layout,
                    entries: &entries,
                });
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&self.pipelines[format]);
                pass.set_bind_group(0, &binding, &[]);
                pass.dispatch_workgroups(32, 32, chunk.len() as u32);
            }
        }
        Ok(())
    }
}
