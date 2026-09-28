//! Drag frames of a transform resample a copy of the layer reduced to the
//! display level the view samples, one bilinear sample per texel.
use super::*;

const UNIFORM_BYTES: u64 = 240;

pub(crate) struct Request<'a> {
    pub moved: &'a layer_core::ImageTransform,
    pub kept: &'a layer_core::ImageTransform,
    pub clip: layer_core::Affine,
    pub extent: [u32; 2],
    pub texels: [u32; 4],
    pub display: pixel_transform::DisplayLevel,
    pub keeps_pixels: bool,
    pub mesh: bool,
    pub source: display_mips::Plan,
    pub outside: f32,
}

pub(crate) enum Sampling { Linear, AffineArea }

pub(crate) struct Resample {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pub pipeline: Deferred<wgpu::ComputePipeline>,
    pub area: Deferred<wgpu::ComputePipeline>,
}
impl Resample {
    pub fn new(device: &PipelineDevice) -> Self {
        let layout = crate::bindings::layout(device, "display resample", &[
            crate::bindings::texture(0, wgpu::ShaderStages::COMPUTE, true),
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
            crate::bindings::texture(3, wgpu::ShaderStages::COMPUTE, true),
            crate::bindings::texture(4, wgpu::ShaderStages::COMPUTE, false),
            crate::bindings::sampler(5, wgpu::ShaderStages::COMPUTE, wgpu::SamplerBindingType::Filtering),
        ]);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("display resample"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = Deferred::wgsl(device, "display resample", compose_wgsl(&[&crate::working_color::shader(device), include_str!("../area_sample.wgsl"), include_str!("../display_resample.wgsl")]));
        let pipeline = Deferred::compute(device, "display resample", &pipeline_layout, &shader, "resample_main");
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("display resample"),
            min_filter: wgpu::FilterMode::Linear,
            mag_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let area = Deferred::compute(device, "display area resample", &pipeline_layout, &shader, "resample_affine_area");
        Self { layout, sampler, pipeline, area }
    }
    pub fn values(request: Request<'_>) -> Result<[u8; UNIFORM_BYTES as usize], GpuRasterError> {
        let Request { moved, kept, clip, extent, texels, display, keeps_pixels, mesh, source, outside } = request;
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
            .chain([display.opacity, f32::from(u8::from(keeps_pixels)), f32::from(u8::from(mesh)), outside]);
        for (dst, value) in values[160..192].chunks_exact_mut(4).zip(options) {
            dst.copy_from_slice(&value.to_le_bytes());
        }
        let sizes = [source.extent.map(|n| n as f32 / (1 << source.level) as f32),
            display.extent.map(|n| n as f32 / display.side as f32)];
        for (row, size) in values[192..].chunks_exact_mut(16).zip(sizes) {
            for (dst, value) in row.chunks_exact_mut(4).zip(size.into_iter().chain([0.; 2])) {
                dst.copy_from_slice(&value.to_le_bytes());
            }
        }
        values[200..204].copy_from_slice(&f32::from(kept.keep_source).to_le_bytes());
        values[224..228].copy_from_slice(&f32::from(u8::from(display.encode)).to_le_bytes());
        Ok(values)
    }
    pub fn binding(&self, device: &wgpu::Device, uniforms: &wgpu::Buffer, offset: u64,
        views: [&wgpu::TextureView; 4],
    ) -> wgpu::BindGroup {
        let [source, target, kept, positions] = views;
        crate::bindings::group(device, "display resample", &self.layout, [
            wgpu::BindingResource::TextureView(source), wgpu::BindingResource::TextureView(target),
            wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: uniforms, offset, size: wgpu::BufferSize::new(UNIFORM_BYTES) }),
            wgpu::BindingResource::TextureView(kept), wgpu::BindingResource::TextureView(positions),
            wgpu::BindingResource::Sampler(&self.sampler),
        ])
    }
    pub fn encode(&self, encoder: &mut crate::submission::CommandEncoder, binding: &wgpu::BindGroup, texels: [u32; 4], sampling: Sampling) {
        let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("display resample"), timestamp_writes: None,
        });
        compute.set_pipeline(match sampling { Sampling::Linear => &self.pipeline, Sampling::AffineArea => &self.area });
        compute.set_bind_group(0, binding, &[]);
        compute.dispatch_workgroups(texels[2].div_ceil(8), texels[3].div_ceil(8), 1);
    }

}

/// A transaction's layer reduced to one display level from its originals a
/// few `pending` regions per frame, and what resampling it binds. With a
/// selection that keeps some pixels in place, `image` holds the pixels that
/// move and `kept` the others, and each region is a block drawn exactly.
/// Otherwise each is a page reduced whole.
pub(crate) struct Reduced {
    pub transaction: u64,
    pub level: u32,
    pub image: display_mips::Image,
    pub kept: Option<display_mips::Image>,
    pub pending: Vec<PixelRect>,
    uniforms: wgpu::Buffer,
    binding: crate::bindings::CachedBinding<[wgpu::TextureView; 2]>,
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
        let plan = display_mips::Plan::at(extent, level);
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
            binding: Default::default(),
        }
    }
    pub fn storage_bytes(&self) -> u64 {
        self.image.storage_bytes() + self.kept.as_ref().map_or(0, display_mips::Image::storage_bytes) + UNIFORM_BYTES
    }
    /// Draw `texels` of the display `level` from this layer. `moved` maps
    /// reduced layer texels of the pixels that move to display texels, or
    /// with a warp mesh `positions` holds them for each texel, and `kept`
    /// maps those of the pixels kept in place, which include the moved pixels
    /// too when it keeps its source. `clip` maps display texels to layer
    /// pixels, which the layer keeps within `extent`.
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
        let values = Resample::values(Request { moved, kept, clip, extent, texels, display,
            keeps_pixels: self.kept.is_some(), mesh: positions.is_some(), source: self.image.plan, outside: 0. })?;
        r.uploads.write(encoder, &self.uniforms, &values)?;
        let kept = self.kept.as_ref().unwrap_or(&self.image);
        let positions = positions.unwrap_or(&r.empty_view);
        let binding = self.binding.get([level.clone(), positions.clone()], || {
            pass.binding(&r.device, &self.uniforms, 0, [&self.image.view, level, &kept.view, positions])
        });
        pass.encode(encoder, &binding, texels, Sampling::Linear);
        Ok(())
    }
}
