//! Bounded native SDR writeback primitives. Working pixels remain Float32 until
//! a publication boundary. The caller checks the shared status before publishing
//! any captured tiles and retains the previous revision if the batch failed.
use crate::{GpuRasterError, PipelineDevice};
use layer_core::color::{AlphaAssociation, IntegerDepth, PixelDescriptor, TransferEncoding};
pub(crate) mod transfer;
pub use transfer::NativeTransfer;

pub const MAX_BATCH_TILES: usize = 16;
pub const STATUS_BYTES: u64 = 8;

/// A 256×256 working tile and its two publication candidates. Only `region` is
/// written. Initialize both destinations from their previous pixels to preserve
/// others, and publish neither until the status succeeds. The canonical working
/// result agrees with decoding the native result, so save/reopen cannot reveal
/// extra precision that survived only in a live cache.
pub struct NativeTileRequest<'a> {
    pub working: &'a wgpu::Texture,
    pub encoded: &'a wgpu::Texture,
    pub canonical: &'a wgpu::Texture,
    pub transfer: &'a NativeTransfer,
    pub depth: IntegerDepth,
    pub alpha: AlphaAssociation,
    /// x, y, width, height, in tile pixels.
    pub region: [u32; 4],
}
impl NativeTileRequest<'_> {
    pub fn descriptor(&self) -> PixelDescriptor {
        PixelDescriptor {
            channels: 4,
            bits_per_channel: self.depth.bits(),
            encoding: TransferEncoding::Profile,
            alpha: self.alpha,
        }
    }
}

/// Shared across every batch in a publication. Reset once before recording them,
/// and read after their completion, before publishing any native tile.
pub struct NativeEncodeStatus(wgpu::Buffer);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeEncodingStats {
    pub clipped_pixels: u32,
}
impl NativeEncodeStatus {
    pub fn new(device: &wgpu::Device) -> Self {
        Self(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("native tile publication status"),
            size: STATUS_BYTES,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }))
    }
    pub fn reset(&self, commands: &mut wgpu::CommandEncoder) {
        commands.clear_buffer(&self.0, 0, None);
    }
    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.0
    }
    pub fn decode(bytes: &[u8]) -> Result<NativeEncodingStats, GpuRasterError> {
        if bytes.len() != STATUS_BYTES as usize {
            return Err(GpuRasterError::Color(
                "Invalid native encoding status".into(),
            ));
        }
        let invalid = u32::from_le_bytes(bytes[..4].try_into().unwrap());
        if invalid != 0 {
            return Err(GpuRasterError::Color(
                "Color processing produced non-finite values or invalid coverage".into(),
            ));
        }
        Ok(NativeEncodingStats {
            clipped_pixels: u32::from_le_bytes(bytes[4..].try_into().unwrap()),
        })
    }
}
struct Job {
    binding: wgpu::BindGroup,
    format: usize,
    offset: u32,
    groups: [u32; 2],
}
pub struct NativeTileBatch {
    jobs: Vec<Job>,
    parameter_bytes: u64,
}
impl NativeTileBatch {
    pub fn parameter_bytes(&self) -> u64 {
        self.parameter_bytes
    }
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }
}

/// Compile while preparing the document mode, never on a brush/commit hot path.
/// Pipelines are independent of working primaries, transfer curve and alpha.
pub struct NativeTileEncoder {
    layouts: [wgpu::BindGroupLayout; 2],
    pipelines: [wgpu::ComputePipeline; 2],
}
impl NativeTileEncoder {
    pub fn new(device: &wgpu::Device) -> Self {
        Self::with_device(&device.clone().into())
    }
    pub(crate) fn with_device(device: &PipelineDevice) -> Self {
        let layouts = std::array::from_fn(|index| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("native tile encoder inputs"),
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
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format: if index == 0 {
                                wgpu::TextureFormat::Rgba8Uint
                            } else {
                                wgpu::TextureFormat::Rgba16Uint
                            },
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                    buffer_entry(
                        2,
                        wgpu::BufferBindingType::Storage { read_only: true },
                        false,
                        transfer::TABLE_BYTES,
                    ),
                    buffer_entry(3, wgpu::BufferBindingType::Uniform, true, 32),
                    buffer_entry(
                        4,
                        wgpu::BufferBindingType::Storage { read_only: false },
                        false,
                        STATUS_BYTES,
                    ),
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format: wgpu::TextureFormat::Rgba32Float,
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                ],
            })
        });
        let pipelines = std::array::from_fn(|index| {
            let source = format!(
                "{}\n{}",
                include_str!("sdr_color.wgsl"),
                include_str!("native_tiles/encode.wgsl").replace(
                    "OUTPUT_FORMAT",
                    if index == 0 {
                        "rgba8uint"
                    } else {
                        "rgba16uint"
                    }
                )
            );
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("native SDR tile writeback"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("native SDR tile writeback"),
                bind_group_layouts: &[Some(&layouts[index])],
                immediate_size: 0,
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("native SDR tile writeback"),
                layout: Some(&layout),
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            })
        });
        Self { layouts, pipelines }
    }
    /// Prepare and validate the whole batch before recording any tile writes.
    /// Bindings and parameters can be reused while these resources/regions remain.
    pub fn prepare(
        &self,
        device: &wgpu::Device,
        requests: &[NativeTileRequest<'_>],
        status: &NativeEncodeStatus,
    ) -> Result<NativeTileBatch, GpuRasterError> {
        if requests.len() > MAX_BATCH_TILES {
            return Err(GpuRasterError::Color(
                "Too many native tiles in one batch".into(),
            ));
        }
        for request in requests {
            validate(request)?;
        }
        if requests.is_empty() {
            return Ok(NativeTileBatch {
                jobs: Vec::new(),
                parameter_bytes: 0,
            });
        }
        let stride = 32u32.next_multiple_of(device.limits().min_uniform_buffer_offset_alignment);
        let size = u64::from(stride) * requests.len() as u64;
        let mut bytes = vec![0; size as usize];
        for (i, r) in requests.iter().enumerate() {
            let values = [
                r.depth.maximum(),
                65535 / r.depth.maximum(),
                r.transfer.curve,
                u32::from(r.alpha == AlphaAssociation::Straight),
                r.region[0],
                r.region[1],
                r.region[2],
                r.region[3],
            ];
            for (slot, value) in bytes[i * stride as usize..][..32]
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(values)
            {
                *slot = value.to_le_bytes();
            }
        }
        let parameters = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("batched native tile parameters"),
            size,
            usage: wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: true,
        });
        parameters
            .get_mapped_range_mut(..)
            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?
            .copy_from_slice(&bytes);
        parameters.unmap();
        let jobs = requests
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let format = usize::from(r.depth == IntegerDepth::U16);
                let source = r.working.create_view(&Default::default());
                let target = r.encoded.create_view(&Default::default());
                let canonical = r.canonical.create_view(&Default::default());
                let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("native SDR tile writeback"),
                    layout: &self.layouts[format],
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&source),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&target),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: r.transfer.as_entire_binding(),
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
                            resource: status.0.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: wgpu::BindingResource::TextureView(&canonical),
                        },
                    ],
                });
                Job {
                    binding,
                    format,
                    offset: i as u32 * stride,
                    groups: [r.region[2].div_ceil(8), r.region[3].div_ceil(8)],
                }
            })
            .collect();
        Ok(NativeTileBatch {
            jobs,
            parameter_bytes: size,
        })
    }
    /// The submission owner starts the compute pass so its normal chunking,
    /// capture lifetimes and cancellation handling also apply to this work.
    pub fn encode(&self, pass: &mut wgpu::ComputePass<'_>, batch: &NativeTileBatch) {
        for job in &batch.jobs {
            if job.groups.contains(&0) {
                continue;
            }
            pass.set_pipeline(&self.pipelines[job.format]);
            pass.set_bind_group(0, &job.binding, &[job.offset]);
            pass.dispatch_workgroups(job.groups[0], job.groups[1], 1);
        }
    }
}
fn buffer_entry(
    binding: u32,
    ty: wgpu::BufferBindingType,
    dynamic: bool,
    bytes: u64,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset: dynamic,
            min_binding_size: wgpu::BufferSize::new(bytes),
        },
        count: None,
    }
}
fn validate(r: &NativeTileRequest<'_>) -> Result<(), GpuRasterError> {
    let dimensions = |t: &wgpu::Texture| {
        t.dimension() == wgpu::TextureDimension::D2
            && t.width() == 256
            && t.height() == 256
            && t.depth_or_array_layers() == 1
            && t.sample_count() == 1
            && t.mip_level_count() == 1
    };
    let format = if r.depth == IntegerDepth::U8 {
        wgpu::TextureFormat::Rgba8Uint
    } else {
        wgpu::TextureFormat::Rgba16Uint
    };
    if !dimensions(r.working)
        || !dimensions(r.encoded)
        || !dimensions(r.canonical)
        || r.working.format() != wgpu::TextureFormat::Rgba32Float
        || r.canonical.format() != wgpu::TextureFormat::Rgba32Float
        || r.canonical == r.working
        || r.encoded.format() != format
        || !r
            .working
            .usage()
            .contains(wgpu::TextureUsages::TEXTURE_BINDING)
        || !r
            .encoded
            .usage()
            .contains(wgpu::TextureUsages::STORAGE_BINDING)
        || !r
            .canonical
            .usage()
            .contains(wgpu::TextureUsages::STORAGE_BINDING)
        || !matches!(
            r.alpha,
            AlphaAssociation::Straight | AlphaAssociation::PremultipliedLinear
        )
        || r.region[0].checked_add(r.region[2]).is_none_or(|v| v > 256)
        || r.region[1].checked_add(r.region[3]).is_none_or(|v| v > 256)
    {
        return Err(GpuRasterError::Color(
            "Invalid native tile writeback request".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
