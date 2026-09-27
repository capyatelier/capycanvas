//! Drag frames of a transform resample a copy of the layer reduced to the
//! display level the view samples, one bilinear sample per texel.
use super::*;

const UNIFORM_BYTES: u64 = 192;

pub(crate) struct Resample {
    layout: wgpu::BindGroupLayout,
    pub pipeline: Deferred<wgpu::ComputePipeline>,
    mesh_layout: wgpu::BindGroupLayout,
    /// Resamples a warp mesh from the positions rasterized for its texels.
    pub mesh_pipeline: Deferred<wgpu::ComputePipeline>,
}
impl Resample {
    pub fn new(device: &PipelineDevice) -> Self {
        let entries = [
            crate::bindings::texture(0, wgpu::ShaderStages::COMPUTE, false),
            crate::bindings::storage_texture(
                1,
                wgpu::ShaderStages::COMPUTE,
                wgpu::TextureFormat::Rgba32Float,
                wgpu::StorageTextureAccess::WriteOnly,
            ),
            crate::bindings::buffer(
                2,
                wgpu::ShaderStages::COMPUTE,
                wgpu::BufferBindingType::Uniform,
                false,
                wgpu::BufferSize::new(UNIFORM_BYTES),
            ),
            crate::bindings::texture(3, wgpu::ShaderStages::COMPUTE, false),
            crate::bindings::texture(4, wgpu::ShaderStages::COMPUTE, false),
        ];
        let layout = crate::bindings::layout(device, "display resample", &entries[..4]);
        let mesh_layout = crate::bindings::layout(device, "display mesh resample", &entries);
        let pipeline_layout = |layout: &wgpu::BindGroupLayout| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("display resample"),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            })
        };
        let shader = Deferred::wgsl(device, "display resample", include_str!("../display_resample.wgsl"));
        let pipeline = Deferred::compute(device, "display resample", &pipeline_layout(&layout), &shader, "resample_main");
        let mesh_pipeline = Deferred::compute(
            device,
            "display mesh resample",
            &pipeline_layout(&mesh_layout),
            &shader,
            "mesh_resample_main",
        );
        Self { layout, pipeline, mesh_layout, mesh_pipeline }
    }
}

/// A transaction's layer reduced to one display level, drawn from its
/// originals a few blocks per frame, and what resampling it binds. With a
/// selection that keeps some pixels in place, `image` holds the pixels that
/// move and `kept` the others.
pub(crate) struct Reduced {
    pub transaction: u64,
    pub level: u32,
    pub image: display_mips::Image,
    pub kept: Option<display_mips::Image>,
    pub pending: Vec<PixelRect>,
    uniforms: wgpu::Buffer,
    binding: Option<(wgpu::TextureView, wgpu::BindGroup)>,
    mesh_binding: Option<([wgpu::TextureView; 2], wgpu::BindGroup)>,
}
impl Reduced {
    pub fn new(
        r: &WgpuRasterizer,
        transaction: u64,
        level: u32,
        extent: [u32; 2],
        pending: Vec<PixelRect>,
        kept: bool,
    ) -> Self {
        let plan = display_mips::Plan::at_level(extent, level);
        Self {
            transaction,
            level,
            image: display_mips::Image::new(r, plan),
            kept: kept.then(|| display_mips::Image::new(r, plan)),
            pending,
            uniforms: r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("display resample"),
                size: UNIFORM_BYTES,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            binding: None,
            mesh_binding: None,
        }
    }
    pub fn storage_bytes(&self) -> u64 {
        self.image.storage_bytes() + self.kept.as_ref().map_or(0, display_mips::Image::storage_bytes) + UNIFORM_BYTES
    }
    /// Draw `texels` of the display `level` from this layer. `moved` maps
    /// reduced layer texels of the pixels that move to display texels, or
    /// with a warp mesh `positions` holds them for each texel, and `kept`
    /// maps those of the pixels kept in place. `clip` maps display texels to
    /// layer pixels, which the layer keeps within `extent`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        r: &mut WgpuRasterizer,
        pass: &Resample,
        encoder: &mut crate::submission::CommandEncoder,
        level: &wgpu::TextureView,
        moved: &layer_core::ImageTransform,
        kept: &layer_core::ImageTransform,
        clip: layer_core::Affine,
        extent: [u32; 2],
        texels: [u32; 4],
        display: pixel_transform::DisplayLevel,
        positions: Option<&wgpu::TextureView>,
    ) -> Result<(), GpuRasterError> {
        let rows = |transform| pixel_transform::inverse_rows(transform).map_err(GpuRasterError::InvalidTransform);
        let [x, y, w] = rows(moved)?;
        let [kx, ky, kw] = rows(kept)?;
        let [a, b, c, d, u, v] = clip.0;
        let rows = [x, y, w, kx, ky, kw, [a, c, u], [b, d, v], [extent[0] as f32, extent[1] as f32, 0.]];
        let floats = rows.into_iter().flat_map(|row| row.into_iter().chain([0.]));
        let mut values = [0u8; UNIFORM_BYTES as usize];
        for (dst, value) in values[..144].chunks_exact_mut(4).zip(floats) {
            dst.copy_from_slice(&value.to_le_bytes());
        }
        for (dst, value) in values[144..160].chunks_exact_mut(4).zip(texels) {
            dst.copy_from_slice(&value.to_le_bytes());
        }
        let options = display
            .backdrop
            .into_iter()
            .chain([display.opacity, f32::from(u8::from(self.kept.is_some())), 0., 0.]);
        for (dst, value) in values[160..].chunks_exact_mut(4).zip(options) {
            dst.copy_from_slice(&value.to_le_bytes());
        }
        r.uploads.write(encoder, &self.uniforms, &values)?;
        let kept = self.kept.as_ref().unwrap_or(&self.image);
        let binding = if let Some(positions) = positions {
            if self
                .mesh_binding
                .as_ref()
                .is_none_or(|([bound, placed], _)| bound != level || placed != positions)
            {
                let binding = crate::bindings::group(&r.device, "display mesh resample", &pass.mesh_layout, [
                    wgpu::BindingResource::TextureView(&self.image.view),
                    wgpu::BindingResource::TextureView(level),
                    self.uniforms.as_entire_binding(),
                    wgpu::BindingResource::TextureView(&kept.view),
                    wgpu::BindingResource::TextureView(positions),
                ]);
                self.mesh_binding = Some(([level.clone(), positions.clone()], binding));
            }
            &self.mesh_binding.as_ref().unwrap().1
        } else {
            if self.binding.as_ref().is_none_or(|(view, _)| view != level) {
                let binding = crate::bindings::group(&r.device, "display resample", &pass.layout, [
                    wgpu::BindingResource::TextureView(&self.image.view),
                    wgpu::BindingResource::TextureView(level),
                    self.uniforms.as_entire_binding(),
                    wgpu::BindingResource::TextureView(&kept.view),
                ]);
                self.binding = Some((level.clone(), binding));
            }
            &self.binding.as_ref().unwrap().1
        };
        let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("display resample"),
            timestamp_writes: None,
        });
        compute.set_pipeline(if positions.is_some() { &pass.mesh_pipeline } else { &pass.pipeline });
        compute.set_bind_group(0, binding, &[]);
        compute.dispatch_workgroups(texels[2].div_ceil(8), texels[3].div_ceil(8), 1);
        Ok(())
    }
}
