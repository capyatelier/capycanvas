//! Static layers above and below a moving layer, composed once at the
//! display level a view samples, so each drag frame draws only the moving
//! layer and places it between them.
use super::*;

pub(crate) struct LayerComposite {
    layout: wgpu::BindGroupLayout,
    pub pipeline: Deferred<wgpu::ComputePipeline>,
}
impl LayerComposite {
    pub fn new(device: &PipelineDevice) -> Self {
        let texture = |binding| crate::bindings::texture(binding, wgpu::ShaderStages::COMPUTE, false);
        let layout = crate::bindings::layout(device, "layered display composite", &[
            texture(0),
            texture(1),
            texture(2),
            crate::bindings::storage_texture(
                3,
                wgpu::ShaderStages::COMPUTE,
                wgpu::TextureFormat::Rgba32Float,
                wgpu::StorageTextureAccess::WriteOnly,
            ),
            crate::bindings::buffer(
                4,
                wgpu::ShaderStages::COMPUTE,
                wgpu::BufferBindingType::Uniform,
                false,
                wgpu::BufferSize::new(32),
            ),
        ]);
        let shader = Deferred::wgsl(device, "layered display composite", crate::compose_wgsl(&[
            &crate::working_color::shader(device),
            include_str!("../blend_modes.wgsl"),
            include_str!("../display_layers.wgsl"),
        ]));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("layered display composite"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline =
            Deferred::compute(device, "layered display composite", &pipeline_layout, &shader, "composite_main");
        Self { layout, pipeline }
    }
}

/// The moving layer and the display level a layered display serves.
pub(crate) type LayeredKey = (LayerId, u32);

pub(crate) struct LayeredDisplay {
    pub key: LayeredKey,
    below: display_mips::Image,
    above: Option<display_mips::Image>,
    pending: Vec<[u32; 2]>,
    moving: (wgpu::Texture, wgpu::TextureView),
    uniforms: wgpu::Buffer,
    binding: crate::bindings::CachedBinding<wgpu::TextureView>,
}
impl LayeredDisplay {
    pub fn new(r: &WgpuRasterizer, extent: [u32; 2], key: LayeredKey, above: bool) -> Self {
        let plan = display_mips::Plan::at(extent, key.1);
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
            binding: Default::default(),
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
    /// Compose, within this frame's preparation, the next document tiles of
    /// the layers below `index` and, with a transparent paper, of those above
    /// it.
    pub fn build(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        index: usize,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        if self.ready() {
            return Ok(());
        }
        let transparent = FramePacket {
            layers: &packet.layers[..index],
            view: layer_render::ViewState {
                background_rgba_linear: [0.; 4],
                ..packet.view
            },
            ..packet
        };
        r.prepare(encoder, Work::Layers, |r, encoder| {
            let Some(tile) = self.pending.pop() else {
                return Ok(false);
            };
            let mut scene = r.scene.take().unwrap_or_else(|| scene::Scene::new(r));
            let mut composed = scene.compose_image_tile(r, packet, Some(index), tile, &mut self.below, encoder);
            if let Some(above) = &mut self.above
                && composed.is_ok()
            {
                composed = scene.compose_image_tile(r, transparent, None, tile, above, encoder);
            }
            r.scene = Some(scene);
            composed.map(|()| true)
        })
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
        let values = [texels[0], texels[1], texels[2], texels[3], crate::blend_code(blend, &r.device), u32::from(self.above.is_some()), 0, 0];
        let mut bytes = [0; 32];
        for (dst, value) in bytes.chunks_exact_mut(4).zip(values) {
            dst.copy_from_slice(&value.to_le_bytes());
        }
        r.uploads.write(encoder, &self.uniforms, &bytes)?;
        let above = self.above.as_ref().unwrap_or(&self.below);
        let binding = self.binding.get(level.clone(), || {
            crate::bindings::group(&r.device, "layered display composite", &pass.layout, [
                wgpu::BindingResource::TextureView(&self.moving.1),
                wgpu::BindingResource::TextureView(&self.below.view),
                wgpu::BindingResource::TextureView(&above.view),
                wgpu::BindingResource::TextureView(level),
                self.uniforms.as_entire_binding(),
            ])
        });
        let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("layered display composite"),
            timestamp_writes: None,
        });
        compute.set_pipeline(&pass.pipeline);
        compute.set_bind_group(0, &binding, &[]);
        compute.dispatch_workgroups(texels[2].div_ceil(8), texels[3].div_ceil(8), 1);
        Ok(())
    }
}
