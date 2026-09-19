//! Native linear coverage for masks, stroke coverage and wet-state planes.
//! Packed buffers keep native payloads at one/two bytes per pixel without
//! requiring optional single-channel integer storage texture formats.
use super::{MAX_BATCH_TILES, NativeEncodeStatus, STATUS_BYTES, buffer_entry};
use crate::{GpuRasterError, PipelineDevice};
use layer_core::color::{AlphaAssociation, SampleDepth, PixelDescriptor, TransferEncoding};

/// Exact native coverage restored into a private R32Float working candidate.
pub struct NativeScalarRestore<'a> {
    pub blob: &'a layer_core::raster::TileBlob,
    pub working: &'a wgpu::Texture,
}
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
            if d.sample != layer_core::color::SampleType::Unsigned
                || !scalar_dimensions(r.working)
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
    pub depth: SampleDepth,
    pub region: [u32; 4],
}
impl NativeScalarRequest<'_> {
    pub fn descriptor(&self) -> PixelDescriptor {
        PixelDescriptor {
            sample: layer_core::color::SampleType::Unsigned,
            channels: 1,
            bits_per_channel: self.depth.bits(),
            encoding: TransferEncoding::Linear,
            alpha: AlphaAssociation::None,
        }
    }
}
pub struct NativeScalarBatch {
    jobs: Vec<(wgpu::BindGroup, usize, u32, [u32; 3])>,
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
    in_place: bool,
    layouts: Vec<wgpu::BindGroupLayout>,
    pub(crate) pipelines: Vec<crate::Deferred<wgpu::ComputePipeline>>,
    tiles_per_dispatch: usize,
    full_parameters: wgpu::Buffer,
    parameter_stride: u32,
}
impl NativeScalarEncoder {
    /// Prepare alongside the color encoder, before interaction.
    pub fn new(device: &wgpu::Device) -> Self {
        let encoder = Self::with_device(&device.clone().into());
        for pipeline in &encoder.pipelines {
            pipeline.compile();
        }
        encoder
    }
    pub(crate) fn with_device(device: &PipelineDevice) -> Self {
        Self::with_mode(device, false)
    }
    /// The caller must first scan the complete publication with the same status.
    /// Each invocation then owns every pixel of one complete packed native word.
    pub(crate) fn validated_in_place(device: &PipelineDevice) -> Self {
        assert!(
            device
                .features()
                .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
        );
        Self::with_mode(device, true)
    }
    fn with_mode(device: &PipelineDevice, in_place: bool) -> Self {
        let bindings = if in_place { 2 } else { 3 };
        let tiles_per_dispatch = 2usize
            .min(device.limits().max_sampled_textures_per_shader_stage as usize)
            .min(device.limits().max_storage_textures_per_shader_stage as usize)
            .min(
                device
                    .limits()
                    .max_storage_buffers_per_shader_stage
                    .saturating_sub(1) as usize,
            );
        assert!(tiles_per_dispatch > 0);
        let mut layouts = Vec::new();
        let mut pipelines = Vec::new();
        for count in 1..=tiles_per_dispatch {
            let mut entries = Vec::new();
            let mut textures = String::new();
            let mut loads = String::new();
            let mut load_words = String::new();
            let mut store_words = String::new();
            let mut stores = String::new();
            for i in 0..count as u32 {
                let base = i * bindings;
                if in_place {
                    entries.push(super::read_write_texture_entry(
                        base,
                        wgpu::TextureFormat::R32Float,
                    ));
                    textures.push_str(&format!("@group(0) @binding({base}) var working{i}:texture_storage_2d<r32float,read_write>;\n"));
                    loads.push_str(&format!(
                        "case {i}u: {{ return textureLoad(working{i},pixel).r; }}\n"
                    ));
                } else {
                    entries.push(super::sampled_entry(base));
                    entries.push(super::storage_texture_entry(
                        base + 2,
                        wgpu::TextureFormat::R32Float,
                    ));
                    textures.push_str(&format!("@group(0) @binding({base}) var working{i}:texture_2d<f32>;\n@group(0) @binding({}) var canonical{i}:texture_storage_2d<r32float,write>;\n", base + 2));
                    loads.push_str(&format!(
                        "case {i}u: {{ return textureLoad(working{i},pixel,0).r; }}\n"
                    ));
                }
                entries.push(buffer_entry(
                    base + 1,
                    wgpu::BufferBindingType::Storage { read_only: false },
                    false,
                    65536,
                ));
                textures.push_str(&format!(
                    "@group(0) @binding({}) var<storage,read_write> encoded{i}:array<u32>;\n",
                    base + 1
                ));
                load_words.push_str(&format!("case {i}u: {{ return encoded{i}[address]; }}\n"));
                store_words.push_str(&format!("case {i}u: {{ encoded{i}[address]=word; }}\n"));
                let destination = if in_place { "working" } else { "canonical" };
                stores.push_str(&format!(
                    "case {i}u: {{ textureStore({destination}{i},pixel,value); }}\n"
                ));
            }
            let shared = count as u32 * bindings;
            entries.extend([
                buffer_entry(shared, wgpu::BufferBindingType::Uniform, true, 32),
                buffer_entry(
                    shared + 1,
                    wgpu::BufferBindingType::Storage { read_only: false },
                    false,
                    STATUS_BYTES,
                ),
            ]);
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("native scalar encoder inputs"),
                entries: &entries,
            });
            let body = include_str!("scalar.wgsl")
                .replace(
                    "PUBLICATION_GUARD",
                    if in_place {
                        "if atomicLoad(&status.invalid)!=0u {return;}"
                    } else {
                        ""
                    },
                )
                .replace("TEXTURES", &textures)
                .replace("LOAD_WORDS", &load_words)
                .replace("STORE_WORDS", &store_words)
                .replace("LOADS", &loads)
                .replace("STORES", &stores)
                .replace("SETTINGS_BINDING", &shared.to_string())
                .replace("STATUS_BINDING", &(shared + 1).to_string());
            let source = format!(
                "{}\n{}\n{}",
                include_str!("coverage.wgsl"),
                include_str!("validity.wgsl"),
                body
            );
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("native scalar writeback"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            pipelines.push({
                let (device, pipeline_layout) = (device.clone(), pipeline_layout.clone());
                crate::Deferred::pipeline(move |mode| {
                    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("native scalar writeback"),
                        source: wgpu::ShaderSource::Wgsl(source.into()),
                    });
                    mode.compute(
                        &device,
                        &wgpu::ComputePipelineDescriptor {
                            label: Some("native scalar writeback"),
                            layout: Some(&pipeline_layout),
                            module: &shader,
                            entry_point: Some("main"),
                            compilation_options: Default::default(),
                            cache: None,
                        },
                    )
                })
            });
            layouts.push(layout);
        }
        let records = [SampleDepth::U8, SampleDepth::U16].map(|depth| {
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
            in_place,
            layouts,
            pipelines,
            tiles_per_dispatch,
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
            validate(r, self.in_place)?;
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
        let mut jobs = Vec::new();
        let mut first = 0;
        while first < requests.len() {
            let r = &requests[first];
            let count = requests[first..]
                .iter()
                .take(self.tiles_per_dispatch)
                .take_while(|next| next.region == r.region && next.depth == r.depth)
                .count();
            let tile_views: Vec<_> = requests[first..first + count]
                .iter()
                .map(|r| [views.get(r.working), views.get(r.canonical)])
                .collect();
            let bindings = if self.in_place { 2 } else { 3 };
            let mut entries = Vec::new();
            for (i, (request, views)) in requests[first..first + count]
                .iter()
                .zip(&tile_views)
                .enumerate()
            {
                entries.extend([
                    wgpu::BindGroupEntry {
                        binding: i as u32 * bindings,
                        resource: wgpu::BindingResource::TextureView(&views[0]),
                    },
                    wgpu::BindGroupEntry {
                        binding: i as u32 * bindings + 1,
                        resource: request.encoded.as_entire_binding(),
                    },
                ]);
                if !self.in_place {
                    entries.push(wgpu::BindGroupEntry {
                        binding: i as u32 * bindings + 2,
                        resource: wgpu::BindingResource::TextureView(&views[1]),
                    });
                }
            }
            let shared = count as u32 * bindings;
            entries.extend([
                wgpu::BindGroupEntry {
                    binding: shared,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &parameters,
                        offset: 0,
                        size: wgpu::BufferSize::new(32),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: shared + 1,
                    resource: status.buffer().as_entire_binding(),
                },
            ]);
            let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("native scalar writeback"),
                layout: &self.layouts[count - 1],
                entries: &entries,
            });
            let components = 4 / r.depth.bytes() as u32;
            let words = (r.region[0] + r.region[2]).div_ceil(components) - r.region[0] / components;
            jobs.push((
                binding,
                count - 1,
                if full {
                    u32::from(r.depth == SampleDepth::U16) * stride
                } else {
                    first as u32 * stride
                },
                [
                    if r.region[2] == 0 {
                        0
                    } else {
                        words.div_ceil(8)
                    },
                    r.region[3].div_ceil(8),
                    count as u32,
                ],
            ));
            first += count;
        }
        Ok(NativeScalarBatch {
            jobs,
            parameter_bytes: size,
        })
    }
    pub fn encode(&self, pass: &mut wgpu::ComputePass<'_>, batch: &NativeScalarBatch) {
        if batch.is_empty() {
            return;
        }
        for (binding, pipeline, offset, groups) in &batch.jobs {
            if groups.contains(&0) {
                continue;
            }
            pass.set_pipeline(&self.pipelines[*pipeline]);
            pass.set_bind_group(0, binding, &[*offset]);
            pass.dispatch_workgroups(groups[0], groups[1], groups[2]);
        }
    }
}
fn validate(r: &NativeScalarRequest<'_>, in_place: bool) -> Result<(), GpuRasterError> {
    if r.depth.is_float()
        || !scalar_dimensions(r.working)
        || !scalar_dimensions(r.canonical)
        || (r.working == r.canonical) != in_place
        || !r.working.usage().contains(if in_place {
            wgpu::TextureUsages::STORAGE_BINDING
        } else {
            wgpu::TextureUsages::TEXTURE_BINDING
        })
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
