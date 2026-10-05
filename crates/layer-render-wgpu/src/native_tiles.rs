//! Bounded native SDR writeback primitives. Working pixels remain Float32 until
//! a publication boundary. The caller checks the shared status before publishing
//! any captured tiles and retains the previous revision if the batch failed.
use crate::{GpuRasterError, PipelineDevice};
use layer_core::color::{AlphaAssociation, SampleDepth, PixelDescriptor, TransferEncoding};
pub mod promote;
pub mod scalar;
pub(crate) mod transfer;
pub use transfer::NativeTransfer;

pub const MAX_BATCH_TILES: usize = 16;
pub const STATUS_BYTES: u64 = 4;

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
pub struct NativeTileRestore<'a> {
    pub blob: &'a std::sync::Arc<layer_core::raster::TileBlob>,
    pub space: layer_core::color::RgbSpace,
    pub destination: layer_core::color::RgbSpace,
    pub working: &'a wgpu::Texture,
}
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
            self.metrics.native_restore_submissions += 1;
        }
        result
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum NativeEncoded<'a> { Texture(&'a wgpu::Texture), Buffer(&'a wgpu::Buffer) }
impl<'a> From<&'a wgpu::Texture> for NativeEncoded<'a> { fn from(value: &'a wgpu::Texture) -> Self { Self::Texture(value) } }
impl<'a> From<&'a wgpu::Buffer> for NativeEncoded<'a> { fn from(value: &'a wgpu::Buffer) -> Self { Self::Buffer(value) } }

/// A working tile and its two publication candidates. Only `region` is written.
/// Initialize both destinations from their previous pixels to preserve others,
/// and publish neither until the status succeeds. The canonical working result
/// agrees with decoding the native result, so save/reopen cannot reveal extra
/// precision that survived only in a live cache.
pub struct NativeTileRequest<'a> {
    pub working: &'a wgpu::Texture,
    pub encoded: NativeEncoded<'a>,
    pub mode: layer_core::color::LayerColorMode,
    pub space: layer_core::color::RgbSpace,
    pub canonical: &'a wgpu::Texture,
    pub transfer: &'a NativeTransfer,
    pub depth: SampleDepth,
    pub alpha: AlphaAssociation,
    /// x, y, width, height, in tile pixels.
    pub region: [u32; 4],
}
impl NativeTileRequest<'_> {
    pub fn descriptor(&self) -> PixelDescriptor {
        PixelDescriptor {
            channels: if self.mode == layer_core::color::LayerColorMode::FullColor { 4 } else { 2 },
            bits_per_channel: self.depth.bits(),
            sample: if self.depth.is_float() { layer_core::color::SampleType::Float } else { layer_core::color::SampleType::Unsigned },
            encoding: if self.depth.is_float() { TransferEncoding::Linear } else { TransferEncoding::Profile },
            alpha: self.alpha,
        }
    }
}

/// Shared across every batch in a publication. Reset once before recording them,
/// and read after their completion, before publishing any native tile.
pub struct NativeEncodeStatus(wgpu::Buffer);
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
    pub fn decode(bytes: &[u8]) -> Result<(), GpuRasterError> {
        if bytes.len() != STATUS_BYTES as usize {
            return Err(GpuRasterError::Color(
                "Invalid native encoding status".into(),
            ));
        }
        if bytes != [0; 4] {
            return Err(GpuRasterError::Color(
                "Color processing produced non-finite values, invalid coverage or RGB outside the selected storage range".into(),
            ));
        }
        Ok(())
    }
}
struct Job {
    binding: wgpu::BindGroup,
    format: usize,
    offset: u32,
    groups: [u32; 3],
}
pub struct NativeTileBatch {
    jobs: Vec<Job>,
}
impl NativeTileBatch {
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }
}

/// Compile while preparing the document mode, never on a brush/commit hot path.
/// Pipelines are independent of working primaries, transfer curve and alpha.
pub struct NativeTileEncoder {
    in_place: bool,
    layouts: Vec<wgpu::BindGroupLayout>,
    pub(crate) pipelines: Vec<crate::Deferred<wgpu::ComputePipeline>>,
    tiles_per_dispatch: usize,
    gray_tiles_per_dispatch: usize,
    full_parameters: wgpu::Buffer,
    parameter_stride: u32,
}
impl NativeTileEncoder {
    pub(crate) fn pipelines_for_depth(&self, depth: SampleDepth) -> impl Iterator<Item = &crate::Deferred<wgpu::ComputePipeline>> {
        let start = depth.bytes().ilog2() as usize * 2 * self.tiles_per_dispatch;
        self.pipelines[start..start + 2 * self.tiles_per_dispatch].iter().chain(self.pipelines[6 * self.tiles_per_dispatch..].iter())
    }
    #[cfg(test)]
    pub(crate) fn with_device(device: &PipelineDevice) -> Self {
        Self::with_mode(device, false, false)
    }
    pub(crate) fn prevalidated(device: &PipelineDevice, in_place: bool) -> Self {
        assert!(!in_place || device.features().contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES));
        Self::with_mode(device, in_place, true)
    }
    fn with_mode(device: &PipelineDevice, in_place: bool, prevalidated: bool) -> Self {
        let tiles_per_dispatch = 2usize
            .min(device.limits().max_sampled_textures_per_shader_stage as usize)
            .min(device.limits().max_storage_textures_per_shader_stage as usize / 2)
            .min(device.limits().max_storage_buffers_per_shader_stage.saturating_sub(2) as usize);
        let gray_tiles_per_dispatch=tiles_per_dispatch
            .min(device.limits().max_storage_buffers_per_shader_stage.saturating_sub(2) as usize/2);
        assert!(tiles_per_dispatch > 0);
        assert!(gray_tiles_per_dispatch>0);
        let mut layouts = Vec::new();
        let mut pipelines = Vec::new();
        for (output_format, output_name) in [
            (Some(wgpu::TextureFormat::Rgba8Uint), "rgba8uint"),
            (Some(wgpu::TextureFormat::Rgba16Uint), "rgba16uint"),
            (Some(wgpu::TextureFormat::Rgba32Uint), "rgba32uint"),
            (None, "gray_alpha"),
        ] {
            for tracked in [false, true] {
                for count in 1..=if output_format.is_some() {tiles_per_dispatch} else {gray_tiles_per_dispatch} {
                    let (entries,source)=Self::pipeline_source(in_place,prevalidated,tracked,count,output_format,output_name);
                    let layout = crate::bindings::layout(device, "native tile encoder inputs", &entries);
                    let pipeline_layout =
                        device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                            label: Some("native SDR tile writeback"),
                            bind_group_layouts: &[Some(&layout)],
                            immediate_size: 0,
                        });
                    pipelines.push(crate::Deferred::compute(device, "native SDR tile writeback", &pipeline_layout, &crate::Deferred::wgsl(device, "native SDR tile writeback", source), "main"));
                    layouts.push(layout);
                }
            }
        }
        let records = std::array::from_fn::<_, 96, _>(|i| {
            use layer_core::color::{RgbSpace, LayerColorMode};
            let depth = [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32][i / 2 % 4];
            let mode = if i < 32 { LayerColorMode::FullColor } else if i < 64 { LayerColorMode::Grayscale } else { LayerColorMode::TwoTone };
            let space = RgbSpace::ALL[(i % 32) / 8];
            let weights = space.to_xyz()[1].map(|v| (v as f32).to_bits());
            [if depth.is_float() { 0 } else { depth.maximum() },
                if depth.is_float() { depth.bits() as u32 } else { 65535 / depth.maximum() },
                if i < 32 { (i / 8) as u32 } else { match space { RgbSpace::Srgb | RgbSpace::DisplayP3 => 0, RgbSpace::AdobeRgb => 2, RgbSpace::ProPhoto => 3 } },
                (i % 2) as u32, 0, 0, 256, 256,
                weights[0], weights[1], weights[2],
                (match mode { LayerColorMode::FullColor => 0f32, LayerColorMode::Grayscale => 1f32, LayerColorMode::TwoTone => 2f32 }).to_bits()]
        });
        let (full_parameters, parameter_stride) = full_parameters(device, &records);
        Self {
            in_place,
            layouts,
            pipelines,
            full_parameters,
            parameter_stride,
            tiles_per_dispatch, gray_tiles_per_dispatch,
        }
    }
    fn pipeline_source(in_place:bool,prevalidated:bool,tracked:bool,count:usize,output_format:Option<wgpu::TextureFormat>,output_name:&str)
        -> (Vec<wgpu::BindGroupLayoutEntry>,String) {
        let bindings=if in_place {2} else {3};
        let mut entries = Vec::new();
        let mut textures = String::new();
        let mut loads = String::new();
        let mut stores = String::new();
        let mut packed_stores = String::new();
        for i in 0..count as u32 {
            let base = i * bindings;
            if in_place {
                entries.push(crate::bindings::storage_texture(
                    base,
                    wgpu::ShaderStages::COMPUTE,
                    wgpu::TextureFormat::Rgba32Float,
                    wgpu::StorageTextureAccess::ReadWrite,
                ));
                textures.push_str(&format!("@group(0) @binding({base}) var working{i}:texture_storage_2d<rgba32float,read_write>;\n"));
                loads.push_str(&format!(
                    "case {i}u: {{ return textureLoad(working{i},pixel); }}\n"
                ));
            } else {
                entries.push(sampled_entry(base));
                entries.push(storage_texture_entry(
                    base + 2,
                    wgpu::TextureFormat::Rgba32Float,
                ));
                textures.push_str(&format!("@group(0) @binding({base}) var working{i}:texture_2d<f32>;\n@group(0) @binding({}) var canonical{i}:texture_storage_2d<rgba32float,write>;\n", base + 2));
                loads.push_str(&format!(
                    "case {i}u: {{ return textureLoad(working{i},pixel,0); }}\n"
                ));
            }
            let destination = if in_place { "working" } else { "canonical" };
            if let Some(format) = output_format {
                entries.push(storage_texture_entry(base + 1, format));
                textures.push_str(&format!("@group(0) @binding({}) var encoded{i}:texture_storage_2d<{output_name},write>;\n", base + 1));
                stores.push_str(&format!("case {i}u: {{ textureStore(encoded{i},pixel,result); textureStore({destination}{i},pixel,linear); }}\n"));
            } else {
                entries.push(buffer_entry(base + 1, wgpu::BufferBindingType::Storage { read_only: false }, false, 256 * 256 * 2));
                textures.push_str(&format!("@group(0) @binding({}) var<storage,read_write> encoded{i}:array<u32>;\n", base + 1));
                stores.push_str(&format!("case {i}u: {{ let index=pixel.y*256u+pixel.x; if settings.maximum==255u {{ let shift=(index%2u)*16u; packed_mask|=65535u<<shift; packed_value|=(result.r|(result.a<<8u))<<shift; }} else if settings.maximum==0u && settings.scale==32u {{ encoded{i}[2u*index]=result.r; encoded{i}[2u*index+1u]=result.a; }} else {{ encoded{i}[index]=result.r|(result.a<<16u); }} textureStore({destination}{i},pixel,linear); }}\n"));
                packed_stores.push_str(&format!("case {i}u: {{ if packed_mask==0xffffffffu {{ encoded{i}[index]=packed_value; }} else {{ encoded{i}[index]=(encoded{i}[index]&~packed_mask)|packed_value; }} }}\n"));
            }
        }
        let shared = count as u32 * bindings;
        for i in 0..if tracked {count as u32} else {0} {
            entries.push(buffer_entry(shared+3+i,wgpu::BufferBindingType::Storage {read_only:false},false,20));
            textures.push_str(&format!("@group(0) @binding({}) var<storage,read_write> changes{i}:ChangedCells;\n",shared+3+i));
        }
        let changed_cases=(0..if tracked {count} else {0}).map(|i|format!("case {i}u: {{if changes{i}.enabled!=0u && any(bitcast<vec4<u32>>(linear)!=bitcast<vec4<u32>>(original)) {{let c=pixel/changes{i}.side;atomicStore(&changes{i}.cells[c.y*(256u/changes{i}.side)+c.x],1u);}} }}")).collect::<String>();
        textures.push_str(&format!("fn mark_canonical(tile:u32,pixel:vec2<u32>,linear:vec4<f32>) {{switch tile {{{changed_cases} default: {{}} }} }}\n"));
        entries.extend([
            buffer_entry(
                shared,
                wgpu::BufferBindingType::Storage { read_only: true },
                false,
                transfer::TABLE_BYTES,
            ),
            buffer_entry(shared + 1, wgpu::BufferBindingType::Uniform, true, 48),
            buffer_entry(
                shared + 2,
                wgpu::BufferBindingType::Storage { read_only: prevalidated && output_format.is_some() },
                false,
                STATUS_BYTES,
            ),
        ]);
        let readonly=prevalidated && output_format.is_some();
        let mut body = include_str!("native_tiles/encode.wgsl")
            .replace("STATUS_TYPE",if readonly {"u32"} else {"atomic<u32>"})
            .replace("STATUS_ACCESS",if readonly {"read"} else {"read_write"})
            .replace("PUBLICATION_GUARD",if readonly {"if status.invalid!=0u {return;}"} else if in_place {"if atomicLoad(&status.invalid)!=0u {return;}"} else {""})
            .replace("FLOAT32_VALIDATION",if readonly {""} else {"let error=float32_color_error(value); if error!=0u {atomicOr(&status.invalid,error);return;}"})
            .replace("HALF_VALIDATION",if readonly {""} else {"let error=hdr_color_error(value); if error!=0u {atomicOr(&status.invalid,error);return;}"})
            .replace("SDR_VALIDATION",if readonly {""} else {"let error=color_error(value); if error!=0u {atomicOr(&status.invalid,error);store_result(invocation.z,pixel,vec4(0u));return;}"})
            .replace("MODE_PROJECTION", if output_format.is_none() { "
                var source_error=color_error(value);
                if settings.maximum==0u {
                    if settings.scale==32u {source_error=float32_color_error(value);}
                    else {source_error=hdr_color_error(value);}
                }
                if source_error!=0u {atomicOr(&status.invalid,source_error);return;}
                value=layer_color(value,settings.weights.rgb,settings.weights.a,select(sdr_decode_component(0.5,settings.curve),0.5,settings.maximum==0u));
            " } else { "" })
            .replace("TEXTURES", &textures)
            .replace("LOADS", &loads)
            .replace("STORES", &stores)
            .replace("TRANSFER_BINDING", &shared.to_string())
            .replace("SETTINGS_BINDING", &(shared + 1).to_string())
            .replace("STATUS_BINDING", &(shared + 2).to_string());
        body.push_str(&match output_format {
            Some(_) => include_str!("native_tiles/encode_main.wgsl").to_string(),
            None => include_str!("native_tiles/gray_alpha.wgsl").replace("PACKED_STORES", &packed_stores),
        });
        let source = format!(
            "{}\n{}\n{}\n{}\n{}",
            include_str!("sdr_color.wgsl"),
            include_str!("native_tiles/color_mode.wgsl"),
            include_str!("native_tiles/validity.wgsl"),
            include_str!("native_tiles/coverage.wgsl"),
            body
        );
        (entries,source)
    }
    pub(crate) fn storage_bytes(&self) -> u64 {
        self.full_parameters.size()
    }
    /// Prepare and validate the whole batch before recording any tile writes.
    /// Bindings and parameters can be reused while these resources/regions remain.
    pub(crate) fn prepare(
        &self,
        device: &wgpu::Device,
        requests: &[NativeTileRequest<'_>],
        status: &NativeEncodeStatus,
        views: &mut crate::native_tiles::PublicationViews,
    ) -> Result<NativeTileBatch, GpuRasterError> {
        self.prepare_tracked(device,requests,status,views,&[])
    }
    pub(crate) fn prepare_tracked(
        &self, device:&wgpu::Device, requests:&[NativeTileRequest<'_>], status:&NativeEncodeStatus,
        views:&mut PublicationViews, changes:&[Option<&wgpu::Buffer>],
    )->Result<NativeTileBatch,GpuRasterError> {
        if !changes.is_empty() && changes.len()!=requests.len() {return Err(GpuRasterError::InvalidExtent);}
        if requests.len() > MAX_BATCH_TILES {
            return Err(GpuRasterError::Color(
                "Too many native tiles in one batch".into(),
            ));
        }
        for (i, request) in requests.iter().enumerate() {
            validate(request, self.in_place)?;
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
            });
        }
        let full = requests.iter().all(|r| r.region == [0, 0, 256, 256]);
        let stride = self.parameter_stride;
        let parameters = if full {
            self.full_parameters.clone()
        } else {
            let size = u64::from(stride) * requests.len() as u64;
            let mut bytes = vec![0; size as usize];
            for (i, r) in requests.iter().enumerate() {
                let values = [
                    if r.depth.is_float() { 0 } else { r.depth.maximum() },
                    if r.depth.is_float() { r.depth.bits() as u32 } else { 65535 / r.depth.maximum() },
                    r.transfer.curve,
                    u32::from(r.alpha == AlphaAssociation::Straight),
                    r.region[0],
                    r.region[1],
                    r.region[2],
                    r.region[3],
                    (r.space.to_xyz()[1][0] as f32).to_bits(),
                    (r.space.to_xyz()[1][1] as f32).to_bits(),
                    (r.space.to_xyz()[1][2] as f32).to_bits(),
                    (match r.mode { layer_core::color::LayerColorMode::FullColor => 0f32, layer_core::color::LayerColorMode::Grayscale => 1f32, layer_core::color::LayerColorMode::TwoTone => 2f32 }).to_bits(),
                ];
                for (slot, value) in bytes[i * stride as usize..][..48]
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
            parameters
        };
        let mut jobs = Vec::new();
        let mut first = 0;
        while first < requests.len() {
            let r = &requests[first];
            let tracked=changes.get(first).copied().flatten().is_some();
            let capacity=if r.mode==layer_core::color::LayerColorMode::FullColor {self.tiles_per_dispatch} else {self.gray_tiles_per_dispatch};
            let count = requests[first..]
                .iter()
                .take(capacity)
                .enumerate()
                .take_while(|(index,next)| {
                    changes.get(first + index).copied().flatten().is_some() == tracked
                        && next.region == r.region
                        && next.depth == r.depth
                        && next.alpha == r.alpha
                        && next.transfer == r.transfer
                        && next.mode == r.mode && next.space == r.space
                })
                .count();
            let depth = match r.depth { SampleDepth::U8 => 0, SampleDepth::U16 => 1, SampleDepth::F16 => 2, SampleDepth::F32 => 3 };
            let format = if r.mode == layer_core::color::LayerColorMode::FullColor {
                (r.depth.bytes().ilog2() as usize * 2 + usize::from(tracked)) * self.tiles_per_dispatch + count - 1
            } else {6 * self.tiles_per_dispatch + usize::from(tracked) * self.gray_tiles_per_dispatch + count - 1};
            let tile_views: Vec<_> = requests[first..first + count]
                .iter()
                .map(|r| {
                    let mut tile = vec![Some(views.get(r.working)), match r.encoded { NativeEncoded::Texture(t) => Some(views.get(t)), NativeEncoded::Buffer(_) => None }];
                    if !self.in_place { tile.push(Some(views.get(r.canonical))); }
                    tile
                })
                .collect();
            let mut entries: Vec<_> = tile_views
                .iter()
                .flatten()
                .enumerate()
                .map(|(i, view)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: if let Some(view) = view { wgpu::BindingResource::TextureView(view) } else { let NativeEncoded::Buffer(buffer) = requests[first + i / if self.in_place { 2 } else { 3 }].encoded else { unreachable!() }; buffer.as_entire_binding() },
                })
                .collect();
            let shared = count as u32 * if self.in_place { 2 } else { 3 };
            entries.extend([
                wgpu::BindGroupEntry {
                    binding: shared,
                    resource: r.transfer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: shared + 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &parameters,
                        offset: 0,
                        size: wgpu::BufferSize::new(48),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: shared + 2,
                    resource: status.0.as_entire_binding(),
                },
            ]);
            if tracked {
                entries.extend((0..count).map(|i|wgpu::BindGroupEntry {binding:shared+3+i as u32,
                    resource:changes[first+i].unwrap().as_entire_binding()}));
            }
            let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("native SDR tile writeback"),
                layout: &self.layouts[format],
                entries: &entries,
            });
            jobs.push(Job {
                binding,
                format,
                offset: if full {
                    ((match r.mode {
                        layer_core::color::LayerColorMode::FullColor => r.transfer.curve * 8,
                        layer_core::color::LayerColorMode::Grayscale | layer_core::color::LayerColorMode::TwoTone => {
                            let mode = if r.mode == layer_core::color::LayerColorMode::Grayscale { 1 } else { 2 };
                            mode * 32 + layer_core::color::RgbSpace::ALL.iter().position(|s| *s == r.space).unwrap() as u32 * 8
                        }
                    }) + depth as u32 * 2
                        + u32::from(r.alpha == AlphaAssociation::Straight))
                        * stride
                } else {
                    first as u32 * stride
                },
                groups: [
                    if r.mode != layer_core::color::LayerColorMode::FullColor && r.depth == SampleDepth::U8 { (r.region[2] + r.region[0] % 2).div_ceil(16) } else { r.region[2].div_ceil(8) },
                    r.region[3].div_ceil(8),
                    count as u32,
                ],
            });
            first += count;
        }
        Ok(NativeTileBatch {
            jobs,
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
            pass.dispatch_workgroups(job.groups[0], job.groups[1], job.groups[2]);
        }
    }
}
/// Immutable full-tile records prepared with the mode's pipelines. Partial
/// rectangles still own their bounded parameter uploads; common publications
/// reuse these records across tiles, batches and frames.
fn full_parameters<const N: usize>(device: &wgpu::Device, records: &[[u32; N]]) -> (wgpu::Buffer, u32) {
    use wgpu::util::DeviceExt;
    let stride = (N as u32 * 4).next_multiple_of(device.limits().min_uniform_buffer_offset_alignment);
    let mut bytes = vec![0; stride as usize * records.len()];
    for (i, record) in records.iter().enumerate() {
        bytes[i * stride as usize..i * stride as usize + N * 4]
            .copy_from_slice(record.map(u32::to_le_bytes).as_flattened());
    }
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("immutable native full-tile parameters"),
        contents: &bytes,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    (buffer, stride)
}

fn sampled_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    crate::bindings::texture(binding, wgpu::ShaderStages::COMPUTE, false)
}
fn storage_texture_entry(binding: u32, format: wgpu::TextureFormat) -> wgpu::BindGroupLayoutEntry {
    crate::bindings::storage_texture(
        binding,
        wgpu::ShaderStages::COMPUTE,
        format,
        wgpu::StorageTextureAccess::WriteOnly,
    )
}

/// Optional native feature for exact in-place SDR publication. Check formats as
/// well as the extension bit; hosts without it retain separate candidates.
pub fn native_in_place_features(adapter: &wgpu::Adapter) -> wgpu::Features {
    let feature = wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
    if adapter.features().contains(feature)
        && [
            wgpu::TextureFormat::Rgba32Float,
            wgpu::TextureFormat::R32Float,
        ]
        .into_iter()
        .all(|format| {
            let caps = adapter.get_texture_format_features(format);
            caps.allowed_usages
                .contains(wgpu::TextureUsages::STORAGE_BINDING)
                && caps
                    .flags
                    .contains(wgpu::TextureFormatFeatureFlags::STORAGE_READ_WRITE)
        })
    {
        feature
    } else {
        wgpu::Features::empty()
    }
}

pub(crate) fn buffer_entry(
    binding: u32,
    ty: wgpu::BufferBindingType,
    dynamic: bool,
    bytes: u64,
) -> wgpu::BindGroupLayoutEntry {
    crate::bindings::buffer(
        binding,
        wgpu::ShaderStages::COMPUTE,
        ty,
        dynamic,
        wgpu::BufferSize::new(bytes),
    )
}
fn validate(r: &NativeTileRequest<'_>, in_place: bool) -> Result<(), GpuRasterError> {
    let dimensions = |t: &wgpu::Texture| {
        t.dimension() == wgpu::TextureDimension::D2
            && t.width() == 256
            && t.height() == 256
            && t.depth_or_array_layers() == 1
            && t.sample_count() == 1
            && t.mip_level_count() == 1
    };
    let format = if r.depth == SampleDepth::U8 {
        wgpu::TextureFormat::Rgba8Uint
    } else if r.depth == SampleDepth::F32 {
        wgpu::TextureFormat::Rgba32Uint
    } else {
        wgpu::TextureFormat::Rgba16Uint
    };
    if !dimensions(r.working)
        || !dimensions(r.canonical)
        || r.working.format() != wgpu::TextureFormat::Rgba32Float
        || r.canonical.format() != wgpu::TextureFormat::Rgba32Float
        || (r.canonical == r.working) != in_place
        || !r.working.usage().contains(if in_place {
            wgpu::TextureUsages::STORAGE_BINDING
        } else {
            wgpu::TextureUsages::TEXTURE_BINDING
        })
        || match r.encoded {
            NativeEncoded::Texture(t) => r.mode != layer_core::color::LayerColorMode::FullColor || !dimensions(t) || t.format() != format || !t.usage().contains(wgpu::TextureUsages::STORAGE_BINDING),
            NativeEncoded::Buffer(b) => r.mode == layer_core::color::LayerColorMode::FullColor || b.size() < r.descriptor().byte_len([256; 2]).unwrap() as u64 || !b.usage().contains(wgpu::BufferUsages::STORAGE),
        }
        || !r
            .canonical
            .usage()
            .contains(wgpu::TextureUsages::STORAGE_BINDING)
        || (r.depth.is_float() && r.alpha != AlphaAssociation::Straight)
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
