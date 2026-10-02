use super::*;
use crate::submission::ColorPass;

pub(crate) const UNIFORM_BYTES: u64 = 256;

pub(crate) struct Mapped {
    pub view: wgpu::TextureView,
    pub values: [u8; UNIFORM_BYTES as usize],
}

pub(crate) struct Request<'a> {
    pub moved: &'a layer_core::ImageTransform,
    pub kept: &'a layer_core::ImageTransform,
    pub clip: layer_core::Affine,
    pub extent: [u32; 2],
    pub texels: [u32; 4],
    pub display: pixel_transform::DisplayLevel,
    pub target: display_mips::Plan,
    pub source: display_mips::Plan,
    pub max_lod: u32,
    pub outside: f32,
    pub keep_source: bool,
    pub identity: bool,
}

#[derive(Clone)]
pub(crate) struct Resample {
    layout: wgpu::BindGroupLayout,
    mesh_layout: wgpu::BindGroupLayout,
    pub(super) sampler: wgpu::Sampler,
    pub mesh: [Deferred<wgpu::RenderPipeline>; 6],
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
            crate::bindings::sampler(4, wgpu::ShaderStages::COMPUTE, wgpu::SamplerBindingType::Filtering),
        ]);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("display resample"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = Deferred::wgsl(device, "display resample", compose_wgsl(&[&crate::working_color::shader(device), include_str!("../area_sample.wgsl"), include_str!("../mapped_sample.wgsl"), include_str!("../display_resample.wgsl")]));
        let mesh_layout = crate::bindings::layout(device, "mesh resample", &[
            crate::bindings::texture(0, wgpu::ShaderStages::FRAGMENT, true),
            crate::bindings::buffer(2, wgpu::ShaderStages::VERTEX_FRAGMENT, wgpu::BufferBindingType::Uniform,
                false, wgpu::BufferSize::new(UNIFORM_BYTES)),
            crate::bindings::texture(3, wgpu::ShaderStages::FRAGMENT, true),
            crate::bindings::sampler(4, wgpu::ShaderStages::FRAGMENT, wgpu::SamplerBindingType::Filtering),
        ]);
        let mesh_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh resample"), bind_group_layouts: &[Some(&mesh_layout)], immediate_size: 0,
        });
        let mesh = ["background_color", "background_color_kept", "mesh_color", "mesh_color_kept", "resample_projective", "resample_projective_kept"].map(|entry| {
            let (device, layout, shader) = (device.clone(), mesh_pipeline_layout.clone(), shader.clone());
            Deferred::pipeline(move |mode| {
                let buffers = [Some(wgpu::VertexBufferLayout {
                    array_stride: 16, step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                })];
                let mesh = entry.starts_with("mesh");
                mode.render(&device, &wgpu::RenderPipelineDescriptor {
                    label: Some(entry), layout: Some(&layout),
                    vertex: wgpu::VertexState { module: &shader,
                        entry_point: Some(if mesh { "mesh_vertex" } else { "background_vertex" }),
                        compilation_options: Default::default(), buffers: if mesh { &buffers } else { &[] },
                    },
                    fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some(entry),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba32Float,
                            blend: None, write_mask: wgpu::ColorWrites::ALL })],
                    }),
                    primitive: Default::default(), depth_stencil: None, multisample: Default::default(),
                    multiview_mask: None, cache: None,
                })
            })
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("display resample"),
            min_filter: wgpu::FilterMode::Linear,
            mag_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let area = Deferred::compute(device, "display area resample", &pipeline_layout, &shader, "resample_affine_area");
        Self { layout, mesh_layout, sampler, mesh, area }
    }
    pub fn values(request: Request<'_>) -> Result<[u8; UNIFORM_BYTES as usize], GpuRasterError> {
        let Request { moved, kept, clip, extent, texels, display, target, source, max_lod, outside, keep_source, identity } = request;
        let origin = [target.bounds.min_x(), target.bounds.min_y()].map(|n| n as f32 / display.side as f32);
        let local = |[x, y, z]: [f32; 3]| [x, y, z + x * origin[0] + y * origin[1]];
        let rows = |transform| pixel_transform::inverse_rows(transform).map(|rows| rows.map(local)).map_err(GpuRasterError::InvalidTransform);
        let [x, y, w] = rows(moved)?;
        let [kx, ky, kw] = rows(kept)?;
        let [a, b, c, d, u, v] = clip.0;
        let rows = [x, y, w, kx, ky, kw, local([a, c, u]), local([b, d, v]), [extent[0] as f32, extent[1] as f32, 0.]];
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
            .chain([display.opacity, max_lod as f32, if identity { 2. } else { f32::from(keep_source) }, outside]);
        for (dst, value) in values[160..192].chunks_exact_mut(4).zip(options) {
            dst.copy_from_slice(&value.to_le_bytes());
        }
        let sizes = [[source.bounds.width(), source.bounds.height()].map(|n| n as f32 / (1 << source.level) as f32),
            [target.bounds.width(), target.bounds.height()].map(|n| n as f32 / display.side as f32)];
        for (row, size) in values[192..].chunks_exact_mut(16).zip(sizes) {
            for (dst, value) in row.chunks_exact_mut(4).zip(size.into_iter().chain([0.; 2])) {
                dst.copy_from_slice(&value.to_le_bytes());
            }
        }
        values[224..228].copy_from_slice(&pixel_transform::exact_taps(moved).min(pixel_transform::PREVIEW_TAPS).to_le_bytes());
        values[240..244].copy_from_slice(&f32::from(display.encode).to_le_bytes());
        Ok(values)
    }
    pub fn binding(&self, device: &wgpu::Device, uniforms: &wgpu::Buffer, offset: u64,
        views: [&wgpu::TextureView; 3],
    ) -> wgpu::BindGroup {
        let [source, target, kept] = views;
        crate::bindings::group(device, "display resample", &self.layout, [
            wgpu::BindingResource::TextureView(source), wgpu::BindingResource::TextureView(target),
            wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: uniforms, offset, size: wgpu::BufferSize::new(UNIFORM_BYTES) }),
            wgpu::BindingResource::TextureView(kept),
            wgpu::BindingResource::Sampler(&self.sampler),
        ])
    }
    #[cfg(test)]
    pub fn encode(&self, encoder: &mut crate::submission::CommandEncoder, binding: &wgpu::BindGroup, texels: [u32; 4]) {
        let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("display resample"), timestamp_writes: None,
        });
        compute.set_pipeline(&self.area);
        compute.set_bind_group(0, binding, &[]);
        compute.dispatch_workgroups(texels[2].div_ceil(8), texels[3].div_ceil(8), 1);
    }

    pub fn mesh_binding(&self, device: &wgpu::Device, uniforms: &wgpu::Buffer, views: &[wgpu::TextureView; 2]) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("mesh resample"), layout: &self.mesh_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&views[0]) },
                wgpu::BindGroupEntry { binding: 2, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&views[1]) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        })
    }
    pub fn encode_mesh(&self, encoder: &mut crate::submission::CommandEncoder, binding: &wgpu::BindGroup,
        target: &wgpu::TextureView, texels: [u32; 4], mesh: Option<&paint_transform::MeshBuffers>, kept: bool,
    ) {
        let mut pass = encoder.color_pass("mesh resample", target, wgpu::LoadOp::Load);
        pass.set_scissor_rect(texels[0], texels[1], texels[2], texels[3]);
        pass.set_bind_group(0, binding, &[]);
        pass.set_pipeline(&self.mesh[if mesh.is_some() { 0 } else { 4 } + usize::from(kept)]);
        pass.draw(0..3, 0..1);
        if let Some(mesh) = mesh {
            pass.set_pipeline(&self.mesh[2+usize::from(kept)]);
            mesh.draw(&mut pass, 0..mesh.count());
        }
    }

}
