//! Native linear coverage for masks, stroke coverage and wet-state planes.
//! Packed buffers keep native payloads at one/two bytes per pixel without
//! requiring optional single-channel integer storage texture formats.
use super::{MAX_BATCH_TILES, NativeEncodeStatus, STATUS_BYTES, buffer_entry};
use crate::{GpuRasterError, PipelineDevice};
use layer_core::color::{AlphaAssociation, IntegerDepth, PixelDescriptor, TransferEncoding};

/// Exact native coverage restored into a private R32Float working candidate.
#[cfg(not(target_arch = "wasm32"))]
pub struct NativeScalarRestore<'a> {
    pub blob: &'a layer_core::raster::TileBlob,
    pub working: &'a wgpu::Texture,
}
#[cfg(not(target_arch = "wasm32"))]
impl crate::WgpuRasterizer {
    /// Cold restore work shares the source/raster upload ceiling. Call outside
    /// input handling. No live page or revision is replaced here; discard all
    /// private candidates on error, including any earlier drained batches.
    pub fn restore_native_scalars(
        &mut self,
        requests: &[NativeScalarRestore<'_>],
    ) -> Result<(), GpuRasterError> {
        if requests.len() > MAX_BATCH_TILES {
            return Err(GpuRasterError::Color(
                "Too many native scalar restorations".into(),
            ));
        }
        for (i, r) in requests.iter().enumerate() {
            let d = r.blob.descriptor;
            if !scalar_dimensions(r.working)
                || !r.working.usage().contains(wgpu::TextureUsages::COPY_DST)
                || requests[..i].iter().any(|old| old.working == r.working)
                || d.channels != 1
                || !matches!(d.bits_per_channel, 8 | 16)
                || d.encoding != TransferEncoding::Linear
                || d.alpha != AlphaAssociation::None
            {
                return Err(GpuRasterError::Color(
                    "Invalid native scalar restoration".into(),
                ));
            }
        }
        if requests.is_empty() {
            return Ok(());
        }
        let mut scene = self
            .scene
            .take()
            .unwrap_or_else(|| crate::scene::Scene::new(self));
        let mut encoder = crate::submission::CommandEncoder::new(&self.device, &Default::default());
        let result = scene.restore_native_scalars(self, requests, &mut encoder);
        self.scene = Some(scene);
        self.uploads.finish(&encoder);
        if result.is_ok() {
            self.last_submission = Some(encoder.submit(&self.queue));
        }
        result
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn restore_upload(
    r: &crate::WgpuRasterizer,
    request: &NativeScalarRestore<'_>,
    encoder: &mut crate::submission::CommandEncoder,
) -> Result<u64, GpuRasterError> {
    let bytes = request.blob.decode().map_err(GpuRasterError::Color)?;
    let step = usize::from(request.blob.descriptor.bits_per_channel / 8);
    let maximum = if step == 1 { 255. } else { 65535. };
    let buffer = r.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bounded scalar restore upload"),
        size: 256 * 256 * 4,
        usage: wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: true,
    });
    {
        let mut mapped = buffer
            .get_mapped_range_mut(..)
            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?;
        let mut row = [0u8; 1024];
        for (y, source) in bytes.chunks_exact(256 * step).enumerate() {
            for (target, sample) in row
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(source.chunks_exact(step))
            {
                let code = if step == 1 {
                    sample[0] as f32
                } else {
                    u16::from_le_bytes(sample.try_into().unwrap()) as f32
                };
                *target = (code / maximum).to_le_bytes();
            }
            mapped.slice(y * 1024..(y + 1) * 1024).copy_from_slice(&row);
        }
    }
    buffer.unmap();
    encoder.copy_buffer_to_texture(
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(1024),
                rows_per_image: Some(256),
            },
        },
        request.working.as_image_copy(),
        request.working.size(),
    );
    Ok(buffer.size())
}

/// Private publication candidates. Initialize both outputs from the previous
/// revision before partial writes. Check the shared status before adopting any
/// output; invalid scalar values reject the publication rather than clipping.
pub struct NativeScalarRequest<'a> {
    pub working: &'a wgpu::Texture,
    /// Exactly 256×256 native samples, little-endian, packed into storage words.
    pub encoded: &'a wgpu::Buffer,
    pub canonical: &'a wgpu::Texture,
    pub depth: IntegerDepth,
    pub region: [u32; 4],
}
impl NativeScalarRequest<'_> {
    pub fn descriptor(&self) -> PixelDescriptor {
        PixelDescriptor {
            channels: 1,
            bits_per_channel: self.depth.bits(),
            encoding: TransferEncoding::Linear,
            alpha: AlphaAssociation::None,
        }
    }
}
pub struct NativeScalarBatch {
    jobs: Vec<(wgpu::BindGroup, u32, [u32; 2])>,
    parameter_bytes: u64,
}
impl NativeScalarBatch {
    pub fn parameter_bytes(&self) -> u64 {
        self.parameter_bytes
    }
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }
}
pub struct NativeScalarEncoder {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
    full_parameters: wgpu::Buffer,
    parameter_stride: u32,
}
impl NativeScalarEncoder {
    /// Prepare alongside the color encoder, before interaction.
    pub fn new(device: &wgpu::Device) -> Self {
        Self::with_device(&device.clone().into())
    }
    pub(crate) fn with_device(device: &PipelineDevice) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("native scalar encoder inputs"),
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
                buffer_entry(
                    1,
                    wgpu::BufferBindingType::Storage { read_only: false },
                    false,
                    65536,
                ),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::R32Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                buffer_entry(3, wgpu::BufferBindingType::Uniform, true, 32),
                buffer_entry(
                    4,
                    wgpu::BufferBindingType::Storage { read_only: false },
                    false,
                    STATUS_BYTES,
                ),
            ],
        });
        let source = format!(
            "{}\n{}\n{}",
            include_str!("coverage.wgsl"),
            include_str!("validity.wgsl"),
            include_str!("scalar.wgsl")
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("native scalar writeback"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("native scalar writeback"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("native scalar writeback"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let records = [IntegerDepth::U8, IntegerDepth::U16].map(|depth| {
            [
                depth.maximum(),
                4 / depth.bytes() as u32,
                0,
                0,
                0,
                0,
                256,
                256,
            ]
        });
        let (full_parameters, parameter_stride) = super::full_parameters(device, &records);
        Self {
            layout,
            pipeline,
            full_parameters,
            parameter_stride,
        }
    }
    pub(crate) fn storage_bytes(&self) -> u64 {
        self.full_parameters.size()
    }
    pub fn prepare(
        &self,
        device: &wgpu::Device,
        requests: &[NativeScalarRequest<'_>],
        status: &NativeEncodeStatus,
    ) -> Result<NativeScalarBatch, GpuRasterError> {
        self.prepare_with_views(device, requests, status, &mut Default::default())
    }
    pub(crate) fn prepare_with_views(
        &self,
        device: &wgpu::Device,
        requests: &[NativeScalarRequest<'_>],
        status: &NativeEncodeStatus,
        views: &mut crate::native_tiles::PublicationViews,
    ) -> Result<NativeScalarBatch, GpuRasterError> {
        if requests.len() > MAX_BATCH_TILES {
            return Err(GpuRasterError::Color(
                "Too many native scalar tiles in one batch".into(),
            ));
        }
        for (i, r) in requests.iter().enumerate() {
            validate(r)?;
            if requests[..i].iter().any(|old| {
                old.encoded == r.encoded
                    || old.canonical == r.canonical
                    || old.working == r.canonical
                    || old.canonical == r.working
            }) {
                return Err(GpuRasterError::Color(
                    "Aliased native scalar publication candidates".into(),
                ));
            }
        }
        if requests.is_empty() {
            return Ok(NativeScalarBatch {
                jobs: Vec::new(),
                parameter_bytes: 0,
            });
        }
        let full = requests.iter().all(|r| r.region == [0, 0, 256, 256]);
        let stride = self.parameter_stride;
        let (parameters, size) = if full {
            (self.full_parameters.clone(), 0)
        } else {
            let size = u64::from(stride) * requests.len() as u64;
            let parameters = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("batched native scalar parameters"),
                size,
                usage: wgpu::BufferUsages::UNIFORM,
                mapped_at_creation: true,
            });
            {
                let mut bytes = parameters
                    .get_mapped_range_mut(..)
                    .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?;
                for (i, r) in requests.iter().enumerate() {
                    let values = [
                        r.depth.maximum(),
                        4 / r.depth.bytes() as u32,
                        0,
                        0,
                        r.region[0],
                        r.region[1],
                        r.region[2],
                        r.region[3],
                    ];
                    let data: Vec<_> = values.into_iter().flat_map(u32::to_le_bytes).collect();
                    let start = i * stride as usize;
                    bytes.slice(start..start + 32).copy_from_slice(&data);
                }
            }
            parameters.unmap();
            (parameters, size)
        };
        let jobs = requests
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let source = views.get(r.working);
                let canonical = views.get(r.canonical);
                let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("native scalar writeback"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&source),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: r.encoded.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&canonical),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &parameters,
                                offset: 0,
                                size: wgpu::BufferSize::new(32),
                            }),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: status.buffer().as_entire_binding(),
                        },
                    ],
                });
                let components = 4 / r.depth.bytes() as u32;
                let words =
                    (r.region[0] + r.region[2]).div_ceil(components) - r.region[0] / components;
                (
                    binding,
                    if full {
                        u32::from(r.depth == IntegerDepth::U16) * stride
                    } else {
                        i as u32 * stride
                    },
                    [
                        if r.region[2] == 0 {
                            0
                        } else {
                            words.div_ceil(8)
                        },
                        r.region[3].div_ceil(8),
                    ],
                )
            })
            .collect();
        Ok(NativeScalarBatch {
            jobs,
            parameter_bytes: size,
        })
    }
    pub fn encode(&self, pass: &mut wgpu::ComputePass<'_>, batch: &NativeScalarBatch) {
        if batch.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        for (binding, offset, groups) in &batch.jobs {
            if groups.contains(&0) {
                continue;
            }
            pass.set_bind_group(0, binding, &[*offset]);
            pass.dispatch_workgroups(groups[0], groups[1], 1);
        }
    }
}
fn validate(r: &NativeScalarRequest<'_>) -> Result<(), GpuRasterError> {
    if !scalar_dimensions(r.working)
        || !scalar_dimensions(r.canonical)
        || r.working == r.canonical
        || !r
            .working
            .usage()
            .contains(wgpu::TextureUsages::TEXTURE_BINDING)
        || !r
            .canonical
            .usage()
            .contains(wgpu::TextureUsages::STORAGE_BINDING)
        || !r.encoded.usage().contains(wgpu::BufferUsages::STORAGE)
        || r.encoded.size() != 65536 * r.depth.bytes() as u64
        || r.region[0].checked_add(r.region[2]).is_none_or(|v| v > 256)
        || r.region[1].checked_add(r.region[3]).is_none_or(|v| v > 256)
    {
        return Err(GpuRasterError::Color(
            "Invalid native scalar writeback request".into(),
        ));
    }
    Ok(())
}
fn scalar_dimensions(t: &wgpu::Texture) -> bool {
    t.dimension() == wgpu::TextureDimension::D2
        && t.width() == 256
        && t.height() == 256
        && t.depth_or_array_layers() == 1
        && t.sample_count() == 1
        && t.mip_level_count() == 1
        && t.format() == wgpu::TextureFormat::R32Float
}

#[cfg(test)]
mod tests;
