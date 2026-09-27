//! Static layers above and below a moving transform, composed once at the
//! display level a view samples, so each drag frame draws only the moving
//! layer and places it between them.
use super::*;

/// Document tiles of each static stack composed per frame while building.
const BUILD_TILES: usize = 96;

pub(crate) struct LayerComposite {
    layout: wgpu::BindGroupLayout,
    pub pipeline: Deferred<wgpu::ComputePipeline>,
}
impl LayerComposite {
    pub fn new(device: &PipelineDevice) -> Self {
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("layered display composite"),
            entries: &[
                texture(0),
                texture(1),
                texture(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba32Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(32),
                    },
                    count: None,
                },
            ],
        });
        let (compile_device, parameters) = (device.clone(), layout.clone());
        let pipeline = Deferred::pipeline(move |mode| {
            let device = &compile_device;
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("layered display composite"),
                source: wgpu::ShaderSource::Wgsl(crate::compose_wgsl(&[
                    &crate::working_color::shader(device),
                    include_str!("../blend_modes.wgsl"),
                    include_str!("../display_layers.wgsl"),
                ])),
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("layered display composite"),
                bind_group_layouts: &[Some(&parameters)],
                immediate_size: 0,
            });
            mode.compute(
                device,
                &wgpu::ComputePipelineDescriptor {
                    label: Some("layered display composite"),
                    layout: Some(&layout),
                    module: &shader,
                    entry_point: Some("composite_main"),
                    compilation_options: Default::default(),
                    cache: None,
                },
            )
        });
        Self { layout, pipeline }
    }
}

/// The transaction, layer and display level a layered display serves.
pub(crate) type LayeredKey = (u64, LayerId, u32);

pub(crate) struct LayeredDisplay {
    pub key: LayeredKey,
    below: display_mips::Image,
    above: Option<display_mips::Image>,
    pending: Vec<[u32; 2]>,
    moving: (wgpu::Texture, wgpu::TextureView),
    uniforms: wgpu::Buffer,
    binding: Option<(wgpu::TextureView, wgpu::BindGroup)>,
}
impl LayeredDisplay {
    pub fn new(r: &WgpuRasterizer, extent: [u32; 2], key: LayeredKey, above: bool) -> Self {
        let plan = display_mips::Plan::at_level(extent, key.2);
        Self {
            key,
            below: display_mips::Image::new(r, plan),
            above: above.then(|| display_mips::Image::new(r, plan)),
            pending: page_coordinates(PixelRect::full(extent)).rev().collect(),
            moving: create_color_target(&r.device, plan.size, "moving layer display level"),
            uniforms: r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("layered display region"),
                size: 32,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            binding: None,
        }
    }
    pub fn ready(&self) -> bool {
        self.pending.is_empty()
    }
    pub fn storage_bytes(&self) -> u64 {
        self.below.storage_bytes()
            + self.above.as_ref().map_or(0, display_mips::Image::storage_bytes)
            + texture_bytes(&self.moving.0)
    }
    /// The level texture the moving layer is drawn into, over nothing.
    pub fn moving(&self) -> &wgpu::TextureView {
        &self.moving.1
    }
    /// Compose the next document tiles of the layers below `index` and, with
    /// a transparent paper, of those above it.
    pub fn build(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        index: usize,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        let tiles: Vec<_> = (0..BUILD_TILES).map_while(|_| self.pending.pop()).collect();
        let mut scene = r.scene.take().unwrap_or_else(|| scene::Scene::new(r));
        let result = (|| {
            scene.compose_image_tiles(r, packet, Some(index), &tiles, &mut self.below, encoder)?;
            if let Some(above) = &mut self.above {
                let transparent = FramePacket {
                    layers: &packet.layers[..index],
                    view: layer_render::ViewState {
                        background_rgba_linear: [0.; 4],
                        ..packet.view
                    },
                    ..packet
                };
                scene.compose_image_tiles(r, transparent, None, &tiles, above, encoder)?;
            }
            Ok(())
        })();
        r.scene = Some(scene);
        result
    }
    /// Place the moving layer's `texels` over the layers below with `blend`,
    /// under the layers above, into the display `level`.
    pub fn composite(
        &mut self,
        r: &mut WgpuRasterizer,
        pass: &LayerComposite,
        encoder: &mut crate::submission::CommandEncoder,
        level: &wgpu::TextureView,
        texels: [u32; 4],
        blend: layer_core::LayerBlend,
    ) -> Result<(), GpuRasterError> {
        let values = [
            texels[0],
            texels[1],
            texels[2],
            texels[3],
            blend as u32,
            u32::from(self.above.is_some()),
            0,
            0,
        ];
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        r.uploads.write(encoder, &self.uniforms, &bytes)?;
        if self.binding.as_ref().is_none_or(|(view, _)| view != level) {
            let above = self.above.as_ref().unwrap_or(&self.below);
            let binding = r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("layered display composite"),
                layout: &pass.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&self.moving.1),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&self.below.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&above.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(level),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: self.uniforms.as_entire_binding(),
                    },
                ],
            });
            self.binding = Some((level.clone(), binding));
        }
        let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("layered display composite"),
            timestamp_writes: None,
        });
        compute.set_pipeline(&pass.pipeline);
        compute.set_bind_group(0, &self.binding.as_ref().unwrap().1, &[]);
        compute.dispatch_workgroups(texels[2].div_ceil(8), texels[3].div_ceil(8), 1);
        Ok(())
    }
}
