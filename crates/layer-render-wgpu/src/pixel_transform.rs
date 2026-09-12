//! GPU affine cut-and-place primitive. A host captures the original layer once,
//! then reuses its source binding for previews and commit. No readback, waits,
//! per-update textures or per-tile bindings. Output regions may be canvas tiles.
use super::{Deferred, PipelineDevice, Uploads};
use layer_core::{Affine, ImageTransform, Interpolation};

pub struct TransformSource {
    binding: wgpu::BindGroup,
    origin: [i32; 2],
    pub(crate) background: f32,
}
pub struct TransformTarget<'a> {
    /// Single-sample RGBA8Unorm (or R8Unorm for scalar mode) render attachment,
    /// never aliasing the source.
    pub view: &'a wgpu::TextureView,
    /// Actual view extent and its origin in the same space as the source/matrix.
    pub extent: [u32; 2],
    pub origin: [i32; 2],
    /// x, y, width, height in this target. Unchanged pixels are not touched.
    pub region: [u32; 4],
}

pub struct PixelTransform {
    scalar: bool,
    visibility: bool,
    pub(super) pipeline: Deferred<wgpu::RenderPipeline>,
    layout: wgpu::BindGroupLayout,
    source_layout: wgpu::BindGroupLayout,
    empty_selection: wgpu::Buffer,
    uniforms: Option<(wgpu::Buffer, wgpu::BindGroup)>,
    stride: u32,
    capacity: u64,
    records: Vec<u8>,
    next_record: u64,
}
impl PixelTransform {
    // Standalone numerical tests compile eagerly; application transforms always
    // join the renderer's staged pipeline compiler and shared upload lifecycle.
    #[cfg(test)]
    pub fn new(device: &wgpu::Device) -> Self {
        Self::headless(device, false)
    }
    /// The same resampling kernel for R8 wetness. Overlapping wetness uses max,
    /// not color's source-over; a move must not invent extra water in overlap.
    #[cfg(test)]
    pub fn scalar(device: &wgpu::Device) -> Self {
        Self::headless(device, true)
    }
    #[cfg(test)]
    fn headless(device: &wgpu::Device, scalar: bool) -> Self {
        let pass = Self::staged(&device.clone().into(), scalar);
        pass.pipeline.compile();
        pass
    }
    pub(super) fn staged(device: &PipelineDevice, scalar: bool) -> Self {
        Self::create(device, scalar, false)
    }
    pub(super) fn staged_visibility(device: &PipelineDevice) -> Self {
        Self::create(device, true, true)
    }
    fn create(device: &PipelineDevice, scalar: bool, visibility: bool) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("affine transform parameters"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(48),
                },
                count: None,
            }],
        });
        let source_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("immutable transform source"),
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
                        min_binding_size: wgpu::BufferSize::new(48),
                    },
                    count: None,
                },
            ],
        });
        let compile_device = device.clone();
        let parameters = layout.clone();
        let source = source_layout.clone();
        let pipeline = Deferred::new(move || {
            let device = &compile_device;
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("affine pixels with selection"),
                source: wgpu::ShaderSource::Wgsl(super::compose_wgsl(&[
                    include_str!("pixel_transform.wgsl"),
                    include_str!("selection_clip.wgsl"),
                ])),
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("affine pixels"),
                bind_group_layouts: &[Some(&parameters), Some(&source)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("affine cut and place"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fragment_main"),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[
                            ("scalar", f64::from(scalar)),
                            ("visibility", f64::from(visibility)),
                        ],
                        ..Default::default()
                    },
                    targets: &[Some(wgpu::ColorTargetState {
                        format: if scalar {
                            wgpu::TextureFormat::R8Unorm
                        } else {
                            super::COLOR_FORMAT
                        },
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
        Self {
            scalar,
            visibility,
            pipeline,
            layout,
            source_layout,
            empty_selection: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("transform selects entire layer"),
                size: 48,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
            uniforms: None,
            stride: 48_u32.div_ceil(device.limits().min_uniform_buffer_offset_alignment)
                * device.limits().min_uniform_buffer_offset_alignment,
            capacity: 0,
            records: Vec::new(),
            next_record: 0,
        }
    }
    /// Another concurrent source shares shader recipes/layouts, never uniform
    /// ranges or mutable pixel captures. Compilation is still once per device.
    pub(super) fn fork(&self) -> Self {
        Self {
            scalar: self.scalar,
            visibility: self.visibility,
            pipeline: self.pipeline.clone(),
            layout: self.layout.clone(),
            source_layout: self.source_layout.clone(),
            empty_selection: self.empty_selection.clone(),
            uniforms: None,
            stride: self.stride,
            capacity: 0,
            records: Vec::new(),
            next_record: 0,
        }
    }
    /// Input is linear premultiplied RGBA, optionally cropped to all content.
    /// Selection uses the existing packed brush-coverage buffer (None = all).
    /// Its geometry/offset is in the same coordinates as `origin` and the affine.
    /// The source must not be overwritten by any preview render target.
    pub fn source(
        &self,
        device: &wgpu::Device,
        texture: &wgpu::Texture,
        origin: [i32; 2],
        selection: Option<&wgpu::Buffer>,
    ) -> Result<TransformSource, &'static str> {
        if texture.dimension() != wgpu::TextureDimension::D2
            || texture.depth_or_array_layers() != 1
            || texture.sample_count() != 1
            || if self.scalar {
                texture.format() != wgpu::TextureFormat::R8Unorm
            } else {
                !matches!(
                    texture.format(),
                    wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba16Float
                )
            }
            || !texture
                .usage()
                .contains(wgpu::TextureUsages::TEXTURE_BINDING)
            || !valid_extent(origin, [texture.width(), texture.height()])
        {
            return Err("Invalid transform source");
        }
        if selection.is_some_and(|b| {
            b.size() < 48
                || b.size() > device.limits().max_storage_buffer_binding_size
                || !b.usage().contains(wgpu::BufferUsages::STORAGE)
        }) {
            return Err("Invalid transform selection");
        }
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("affine source and selection"),
            layout: &self.source_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &texture.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: selection
                        .unwrap_or(&self.empty_selection)
                        .as_entire_binding(),
                },
            ],
        });
        Ok(TransformSource {
            binding,
            origin,
            background: 0.,
        })
    }
    /// Begin a submitted frame. Multiple encodes in that frame use distinct
    /// uniform ranges. Submit the previous frame before calling this again.
    pub fn begin_frame(&mut self) {
        self.next_record = 0;
    }
    /// Each region and encode gets a distinct uniform offset. Uploads share the
    /// frame's reusable staging belt and are finished by its submission owner.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        uploads: &mut Uploads,
        encoder: &mut crate::submission::CommandEncoder,
        source: &TransformSource,
        transform: ImageTransform,
        targets: &[TransformTarget<'_>],
    ) -> Result<(), &'static str> {
        let ImageTransform {
            affine: matrix,
            interpolation,
        } = transform;
        if !source.background.is_finite() || !(0.0..=1.0).contains(&source.background) {
            return Err("Invalid transform background");
        }
        let inverse = matrix
            .inverse()
            .ok_or("Transform must be finite and invertible")?
            .0;
        let bytes = (targets.len() as u64)
            .checked_mul(u64::from(self.stride))
            .ok_or("Too many transform regions")?;
        let end = self
            .next_record
            .checked_add(bytes)
            .ok_or("Too many transform regions")?;
        if end > device.limits().max_buffer_size || end > u64::from(u32::MAX) {
            return Err("Too many transform regions");
        }
        for target in targets {
            let [x, y, w, h] = target.region;
            if !valid_extent(target.origin, target.extent)
                || target
                    .extent
                    .iter()
                    .any(|v| *v > device.limits().max_texture_dimension_2d)
                || w == 0
                || h == 0
                || x.checked_add(w).is_none_or(|v| v > target.extent[0])
                || y.checked_add(h).is_none_or(|v| v > target.extent[1])
            {
                return Err("Invalid transform output region");
            }
        }
        if targets.is_empty() {
            return Ok(());
        }
        if end > self.capacity {
            self.capacity = end
                .next_power_of_two()
                .min(device.limits().max_buffer_size)
                .min(u64::from(u32::MAX))
                & !3;
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("affine region uniforms"),
                size: self.capacity,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("affine region uniforms"),
                layout: &self.layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &buffer,
                        offset: 0,
                        size: wgpu::BufferSize::new(48),
                    }),
                }],
            });
            self.uniforms = Some((buffer, binding));
        }
        self.records.resize(bytes as usize, 0);
        for (index, target) in targets.iter().enumerate() {
            let values = [
                inverse[0],
                inverse[1],
                inverse[2],
                inverse[3],
                inverse[4],
                inverse[5],
                source.origin[0] as f32,
                source.origin[1] as f32,
                target.origin[0] as f32,
                target.origin[1] as f32,
                f32::from(interpolation == Interpolation::Linear)
                    + 2. * f32::from(matrix == Affine::IDENTITY),
                source.background,
            ];
            let record = &mut self.records[index * self.stride as usize..][..48];
            for (value, slot) in values.iter().zip(record.as_chunks_mut::<4>().0.iter_mut()) {
                slot.copy_from_slice(&value.to_le_bytes());
            }
        }
        let (buffer, binding) = self.uniforms.as_ref().unwrap();
        uploads.write_at(encoder, queue, buffer, self.next_record, &self.records);
        for (index, target) in targets.iter().enumerate() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("affine changed region"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.view,
                    depth_slice: None,
                    resolve_target: None,
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
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(
                0,
                binding,
                &[self.next_record as u32 + index as u32 * self.stride],
            );
            pass.set_bind_group(1, &source.binding, &[]);
            let [x, y, w, h] = target.region;
            pass.set_scissor_rect(x, y, w, h);
            pass.draw(0..3, 0..1);
        }
        self.next_record = end;
        Ok(())
    }
    /// Retained scratch only; source/targets are owned by the transaction host.
    pub fn storage_bytes(&self) -> u64 {
        self.capacity + 48
    }
}
fn valid_extent(origin: [i32; 2], extent: [u32; 2]) -> bool {
    // Preserve half-pixel centers as well as integer pixel boundaries in f32.
    origin.into_iter().zip(extent).all(|(o, n)| {
        n > 0 && i64::from(o).abs() <= 8_388_607 && (i64::from(o) + i64::from(n)).abs() <= 8_388_607
    })
}

#[cfg(test)]
#[path = "pixel_transform_tests.rs"]
mod tests;
