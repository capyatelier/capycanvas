use super::*;
use crate::submission::ColorPass;

pub(crate) const UNIFORM_BYTES: u64 = 256;

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
    pub fn new(device: &PipelineDevice) -> Self { Self::with_fine(device, false) }
    pub fn with_fine(device: &PipelineDevice, fine: bool) -> Self {
        let layout = crate::bindings::layout(device, "display resample", &[
            crate::bindings::float_texture(0, wgpu::ShaderStages::COMPUTE, device),
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
            crate::bindings::float_texture(3, wgpu::ShaderStages::COMPUTE, device),
            crate::bindings::sampler(4, wgpu::ShaderStages::COMPUTE, wgpu::SamplerBindingType::Filtering),
        ]);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("display resample"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = Deferred::wgsl(device, "display resample", shader_source(device.working_space(),device.float32_filterable(),false));
        let mut mesh_entries = vec![
            crate::bindings::float_texture(0, wgpu::ShaderStages::FRAGMENT, device),
            crate::bindings::buffer(2, wgpu::ShaderStages::VERTEX_FRAGMENT, wgpu::BufferBindingType::Uniform,
                false, wgpu::BufferSize::new(UNIFORM_BYTES)),
            crate::bindings::float_texture(3, wgpu::ShaderStages::FRAGMENT, device),
            crate::bindings::sampler(4, wgpu::ShaderStages::FRAGMENT, wgpu::SamplerBindingType::Filtering),
        ];
        if fine {
            mesh_entries.push(crate::bindings::float_texture(5, wgpu::ShaderStages::FRAGMENT, device));
            mesh_entries.push(crate::bindings::buffer(6, wgpu::ShaderStages::FRAGMENT, wgpu::BufferBindingType::Storage { read_only:true }, false, wgpu::BufferSize::new(48)));
        }
        let mesh_layout = crate::bindings::layout(device, "mesh resample", &mesh_entries);
        let area_shader = shader.clone();
        let shader = if fine {
            Deferred::wgsl(device, "tiled base resample", shader_source(device.working_space(),device.float32_filterable(),true))
        } else { shader.clone() };
        let mesh_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh resample"), bind_group_layouts: &[Some(&mesh_layout)], immediate_size: 0,
        });
        let mesh = ["background_color", "background_color_kept", "mesh_color", "mesh_color_kept", "resample_projective", "resample_projective_kept"].map(|entry| {
            let (device, layout, shader) = (device.for_recipe(), mesh_pipeline_layout.clone(), shader.clone());
            Deferred::pipeline(move |mode| {
                let buffers = [Some(wgpu::VertexBufferLayout {
                    array_stride: 20, step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32],
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
        let area = Deferred::compute(device, "display area resample", &pipeline_layout, &area_shader, "resample_affine_area");
        Self { layout, mesh_layout, sampler, mesh, area }
    }
    pub fn values(request: Request<'_>) -> Result<[u8; UNIFORM_BYTES as usize], GpuRasterError> {
        let Request { moved, kept, clip, extent, texels, display, target, source, max_lod, outside, keep_source, identity } = request;
        let origin = target.doc_bounds.min.map(|n| n as f32 / display.side as f32);
        let local = |[x, y, z]: [f32; 3]| [x, y, z + x * origin[0] + y * origin[1]];
        let rows = |transform: &layer_core::ImageTransform| pixel_transform::inverse_rows(transform)
            .map(|rows| if transform.placement.mesh.is_some() { rows } else { rows.map(local) }).map_err(GpuRasterError::InvalidTransform);
        let [x, y, w] = rows(moved)?;
        let [kx, ky, kw] = rows(kept)?;
        let [a, b, c, d, u, v] = clip.0;
        let rows = [x, y, w, kx, ky, kw, local([a, c, u]), local([b, d, v]), [extent[0] as f32, extent[1] as f32, 0.]];
        let floats = rows.into_iter().flat_map(|row| row.into_iter().chain([0.]));
        let mut values = [0u8; UNIFORM_BYTES as usize];
        for (dst, value) in values[..144].as_chunks_mut::<4>().0.iter_mut().zip(floats) {
            dst.copy_from_slice(&value.to_le_bytes());
        }
        for (dst, value) in values[144..160].as_chunks_mut::<4>().0.iter_mut().zip(texels) {
            dst.copy_from_slice(&value.to_le_bytes());
        }
        let options = display
            .backdrop
            .into_iter()
            .chain([display.opacity, max_lod as f32, if identity { 2. } else { f32::from(keep_source) }, outside]);
        for (dst, value) in values[160..192].as_chunks_mut::<4>().0.iter_mut().zip(options) {
            dst.copy_from_slice(&value.to_le_bytes());
        }
        let sizes = [[source.bounds.width(), source.bounds.height()].map(|n| n as f32 / (1 << source.level) as f32),
            [target.bounds.width(), target.bounds.height()].map(|n| n as f32 / display.side as f32)];
        for (row, size) in values[192..].as_chunks_mut::<16>().0.iter_mut().zip(sizes) {
            for (dst, value) in row.as_chunks_mut::<4>().0.iter_mut().zip(size.into_iter().chain([0.; 2])) {
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

    #[cfg(test)]
    pub fn mesh_binding(&self, device: &wgpu::Device, uniforms: &wgpu::Buffer, views: &[wgpu::TextureView; 2]) -> wgpu::BindGroup {
        self.mesh_binding_at(device, uniforms, 0, views)
    }
    pub fn mesh_binding_at(&self, device: &wgpu::Device, uniforms: &wgpu::Buffer, offset: u64, views: &[wgpu::TextureView; 2]) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("mesh resample"), layout: &self.mesh_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&views[0]) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: uniforms, offset, size: wgpu::BufferSize::new(UNIFORM_BYTES) }) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&views[1]) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        })
    }
    pub fn fine_binding(&self, device: &wgpu::Device, uniforms: &wgpu::Buffer,
        views: &[wgpu::TextureView; 2], atlas: &wgpu::TextureView, directory: &wgpu::Buffer) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("tiled base resample"), layout: &self.mesh_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&views[0]) },
                wgpu::BindGroupEntry { binding: 2, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&views[1]) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(atlas) },
                wgpu::BindGroupEntry { binding: 6, resource: directory.as_entire_binding() },
            ],
        })
    }
    pub fn encode_mesh(&self, encoder: &mut crate::submission::CommandEncoder, binding: &wgpu::BindGroup,
        target: &wgpu::TextureView, texels: [u32; 4], mesh: Option<(&paint_transform::MeshBuffers,std::ops::Range<u32>)>, kept: bool,
    ) {
        let mut pass = encoder.color_pass("mesh resample", target, wgpu::LoadOp::Load);
        let mesh=mesh.map(|(buffers,range)|buffers.drawing(range));
        self.draw_mesh(&mut pass,binding,texels,mesh.as_ref(),kept);
    }
    pub(crate) fn draw_mesh(&self,pass:&mut wgpu::RenderPass<'_>,binding:&wgpu::BindGroup,texels:[u32;4],mesh:Option<&paint_transform::mesh::MeshDraw>,kept:bool) {
        pass.set_scissor_rect(texels[0], texels[1], texels[2], texels[3]);
        pass.set_bind_group(0, binding, &[]);
        pass.set_pipeline(&self.mesh[if mesh.is_some() { 0 } else { 4 } + usize::from(kept)]);
        pass.draw(0..3, 0..1);
        if let Some(mesh) = mesh {
            pass.set_pipeline(&self.mesh[2+usize::from(kept)]);
            mesh.draw(pass);
        }
    }

}

fn shader_source(space:layer_core::color::RgbSpace,filterable:bool,fine:bool)->std::borrow::Cow<'static,str> {
    let color=crate::working_color::source(space);
    let mapped=include_str!("../mapped_sample.wgsl");
    let mapped=if fine {mapped.replace("fn mapped_source(","fn contiguous_mapped_source(")} else {mapped.to_owned()};
    compose_wgsl(&[&color,crate::pipeline_device::float_sampling_wgsl(filterable),include_str!("../area_sample.wgsl"),&mapped,
        if fine {include_str!("../fine_source_sample.wgsl")} else {""},include_str!("../display_resample.wgsl")])
}
#[cfg(test)]
mod program_tests {
    use super::*;
    #[test]
    fn fine_page_programs_validate_in_every_working_space_and_sampling_mode() {
        for space in layer_core::color::RgbSpace::ALL {
            for filterable in [false,true] {
                let source=shader_source(space,filterable,true);
                let module=naga::front::wgsl::parse_str(&source).unwrap_or_else(|e|panic!("{space:?}/{filterable}: {}",e.emit_to_string(&source)));
                naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&module).unwrap();
                for entry in ["background_color","background_color_kept","mesh_color","mesh_color_kept","resample_projective","resample_projective_kept"] {
                    assert!(module.entry_points.iter().any(|point|point.name==entry));
                }
            }
        }
    }
}
