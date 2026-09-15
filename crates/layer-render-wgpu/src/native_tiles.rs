//! Bounded native SDR writeback primitives. Working pixels remain Float32 until
//! a publication boundary. The caller checks the shared status before publishing
//! any captured tiles and retains the previous revision if the batch failed.
use crate::{GpuRasterError, PipelineDevice};
use layer_core::color::{AlphaAssociation, IntegerDepth, PixelDescriptor, TransferEncoding};
pub mod promote;
pub mod scalar;
pub(crate) mod transfer;
pub use transfer::NativeTransfer;

pub const MAX_BATCH_TILES: usize = 16;
pub const STATUS_BYTES: u64 = 8;

/// Full default views shared only while recording one bounded publication.
/// This owns no pixel allocation and is dropped before returning to input; it
/// cannot keep cold working pages resident between frames. Always construct
/// views here so caller-provided subresource/format views cannot bypass preflight.
#[derive(Default)]
pub(crate) struct PublicationViews(std::collections::HashMap<wgpu::Texture, wgpu::TextureView>);
impl PublicationViews {
    pub(crate) fn get(&mut self, texture: &wgpu::Texture) -> wgpu::TextureView {
        self.0
            .entry(texture.clone())
            .or_insert_with(|| texture.create_view(&Default::default()))
            .clone()
    }
}

/// Restore a committed integer tile into a private 256×256 RGBA32Float candidate.
/// Original images and native paint share the renderer's bounded decode cache.
/// The profile identities are explicit, independent of the integer descriptor.
#[cfg(not(target_arch = "wasm32"))]
pub struct NativeTileRestore<'a> {
    pub blob: &'a std::sync::Arc<layer_core::raster::TileBlob>,
    pub space: layer_core::color::RgbSpace,
    pub destination: layer_core::color::RgbSpace,
    pub working: &'a wgpu::Texture,
}
#[cfg(not(target_arch = "wasm32"))]
impl crate::WgpuRasterizer {
    /// Prepare and share the same transfer buffer used by source/raster decode
    /// with native writeback. Call during mode preparation, before interaction.
    pub fn prepare_native_transfer(
        &mut self,
        space: layer_core::color::RgbSpace,
    ) -> Result<NativeTransfer, GpuRasterError> {
        let mut scene = self
            .scene
            .take()
            .unwrap_or_else(|| crate::scene::Scene::new(self));
        let result = scene.prepare_native_transfer(self, space);
        self.scene = Some(scene);
        result
    }
    /// Queue at most sixteen restorations into caller-owned private candidates.
    /// This may drain pending decode uploads to preserve the staging ceiling;
    /// schedule cold batches outside input handling. On error discard all
    /// candidates. No document revision is published by this operation.
    pub fn restore_native_tiles(
        &mut self,
        requests: &[NativeTileRestore<'_>],
    ) -> Result<(), GpuRasterError> {
        if requests.len() > MAX_BATCH_TILES {
            return Err(GpuRasterError::Color(
                "Native restore batch exceeds sixteen tiles".into(),
            ));
        }
        for (i, request) in requests.iter().enumerate() {
            let t = request.working;
            if t.size()
                != (wgpu::Extent3d {
                    width: 256,
                    height: 256,
                    depth_or_array_layers: 1,
                })
                || t.dimension() != wgpu::TextureDimension::D2
                || t.mip_level_count() != 1
                || t.sample_count() != 1
                || t.format() != wgpu::TextureFormat::Rgba32Float
                || !t.usage().contains(wgpu::TextureUsages::COPY_DST)
                || requests[..i].iter().any(|old| old.working == t)
            {
                return Err(GpuRasterError::Color(
                    "Invalid or duplicate native restore destination".into(),
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
        let result = scene.restore_native_tiles(self, requests, &mut encoder);
        self.scene = Some(scene);
        self.uploads.finish(&encoder);
        if result.is_ok() {
            self.last_submission = Some(encoder.submit(&self.queue));
        }
        result
    }
}

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
    full_parameters: wgpu::Buffer,
    parameter_stride: u32,
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
                "{}\n{}\n{}\n{}",
                include_str!("sdr_color.wgsl"),
                include_str!("native_tiles/validity.wgsl"),
                include_str!("native_tiles/coverage.wgsl"),
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
        let records = std::array::from_fn::<_, 16, _>(|i| {
            let depth = if i / 2 % 2 == 0 {
                IntegerDepth::U8
            } else {
                IntegerDepth::U16
            };
            [
                depth.maximum(),
                65535 / depth.maximum(),
                (i / 4) as u32,
                (i % 2) as u32,
                0,
                0,
                256,
                256,
            ]
        });
        let (full_parameters, parameter_stride) = full_parameters(device, &records);
        Self {
            layouts,
            pipelines,
            full_parameters,
            parameter_stride,
        }
    }
    pub(crate) fn storage_bytes(&self) -> u64 {
        self.full_parameters.size()
    }
    /// Prepare and validate the whole batch before recording any tile writes.
    /// Bindings and parameters can be reused while these resources/regions remain.
    pub fn prepare(
        &self,
        device: &wgpu::Device,
        requests: &[NativeTileRequest<'_>],
        status: &NativeEncodeStatus,
    ) -> Result<NativeTileBatch, GpuRasterError> {
        self.prepare_with_views(device, requests, status, &mut Default::default())
    }
    pub(crate) fn prepare_with_views(
        &self,
        device: &wgpu::Device,
        requests: &[NativeTileRequest<'_>],
        status: &NativeEncodeStatus,
        views: &mut crate::native_tiles::PublicationViews,
    ) -> Result<NativeTileBatch, GpuRasterError> {
        if requests.len() > MAX_BATCH_TILES {
            return Err(GpuRasterError::Color(
                "Too many native tiles in one batch".into(),
            ));
        }
        for (i, request) in requests.iter().enumerate() {
            validate(request)?;
            if requests[..i].iter().any(|old| {
                old.encoded == request.encoded
                    || old.canonical == request.canonical
                    || old.working == request.canonical
                    || old.canonical == request.working
            }) {
                return Err(GpuRasterError::Color(
                    "Aliased native color publication candidates".into(),
                ));
            }
        }
        if requests.is_empty() {
            return Ok(NativeTileBatch {
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
            (parameters, size)
        };
        let jobs = requests
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let format = usize::from(r.depth == IntegerDepth::U16);
                let source = views.get(r.working);
                let target = views.get(r.encoded);
                let canonical = views.get(r.canonical);
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
                    offset: if full {
                        (r.transfer.curve * 4
                            + format as u32 * 2
                            + u32::from(r.alpha == AlphaAssociation::Straight))
                            * stride
                    } else {
                        i as u32 * stride
                    },
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
/// Immutable full-tile records prepared with the mode's pipelines. Partial
/// rectangles still own their bounded parameter uploads; common publications
/// reuse these records across tiles, batches and frames.
fn full_parameters(device: &wgpu::Device, records: &[[u32; 8]]) -> (wgpu::Buffer, u32) {
    use wgpu::util::DeviceExt;
    let stride = 32u32.next_multiple_of(device.limits().min_uniform_buffer_offset_alignment);
    let mut bytes = vec![0; stride as usize * records.len()];
    for (i, record) in records.iter().enumerate() {
        bytes[i * stride as usize..i * stride as usize + 32]
            .copy_from_slice(record.map(u32::to_le_bytes).as_flattened());
    }
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("immutable native full-tile parameters"),
        contents: &bytes,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    (buffer, stride)
}

pub(crate) fn buffer_entry(
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
