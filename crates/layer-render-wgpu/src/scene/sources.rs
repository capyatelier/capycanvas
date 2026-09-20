//! GPU decoded tile slots have fixed ownership. Jobs defer decoding until their
//! ordered upload; eviction never creates another retained GPU tile. Built-in
//! profiles decode on the GPU, while embedded profiles use the native CMM.
use super::*;
use crate::source_access::RawTile;
use layer_core::color::{
    AlphaAssociation, ColorProfile, SampleDepth, PixelDescriptor, RgbSpace, TransferEncoding,
    source::{SourceChannels, SourceImage},
};
use layer_core::raster::TileBlob;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

pub(super) const FLOAT_TILE_BYTES: u64 = PAGE_SIZE as u64 * PAGE_SIZE as u64 * 16;
// Resident decoded pixels and in-flight uploads have different lifetimes.
// Retain neighboring layer tiles independently of staging memory. Equal native
// tile contents share decoded pixels even across distinct document revisions.
const DECODED_SLOTS: usize = 64;
const DECODERS: usize = 4;
use crate::native_tiles::transfer;

enum Key {
    Image(Weak<SourceImage>, [u32; 2]),
    Raster([u8; 32], RgbSpace, RgbSpace),
}
impl Key {
    fn matches(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Image(a, x), Self::Image(b, y)) => a.ptr_eq(b) && x == y,
            (Self::Raster(a, x, d), Self::Raster(b, y, e)) => a == b && x == y && d == e,
            _ => false,
        }
    }
}
struct Slot {
    key: Option<Key>,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    used: u64,
    valid: Arc<std::sync::atomic::AtomicBool>,
}
enum Pixels {
    Image(Arc<SourceImage>, [u32; 2]),
    Raster(Arc<TileBlob>, RgbSpace),
}
pub(super) struct PendingTile {
    pixels: Pixels,
    pub texture: wgpu::Texture,
    view: wgpu::TextureView,
    pub data: Option<[f32; 24]>,
    write: crate::submission::CacheWrite,
}
struct NativeSamples<'a> {
    tile: &'a Arc<TileBlob>,
    descriptor: PixelDescriptor,
    channels: SourceChannels,
    depth: SampleDepth,
    space: RgbSpace,
}
impl Pixels {
    fn native(&self) -> Result<NativeSamples<'_>, GpuRasterError> {
        match self {
            Self::Image(source, coordinate) => {
                let interpretation = &source.interpretation;
                let ColorProfile::Builtin(space) = interpretation.profile else {
                    unreachable!()
                };
                let tile = source
                    .tiles
                    .get(coordinate)
                    .ok_or_else(|| GpuRasterError::Color("Missing source tile".into()))?;
                Ok(NativeSamples {
                    tile,
                    descriptor: interpretation.descriptor(),
                    channels: interpretation.channels,
                    depth: interpretation.depth,
                    space,
                })
            }
            Self::Raster(tile, space) => Ok(NativeSamples {
                tile,
                descriptor: tile.descriptor,
                channels: SourceChannels::Rgba,
                depth: tile.descriptor.depth(),
                space: *space,
            }),
        }
    }
}
struct EncodedInput {
    texture: wgpu::Texture,
    bindings: [Option<wgpu::BindGroup>; 3],
}
#[derive(Default)]
struct InFlight {
    bytes: AtomicU64,
}
// Dropping an unsubmitted encoder also releases its charge. A failed frame
// cannot strand capacity while device-loss recovery replaces the renderer.
struct UploadCharge(Arc<InFlight>, u64);
impl Drop for UploadCharge {
    fn drop(&mut self) {
        self.0.bytes.fetch_sub(self.1, Ordering::Release);
    }
}
#[derive(Default)]
pub(super) struct DecodedTiles {
    destination: RgbSpace,
    slots: Vec<Slot>,
    clock: u64,
    decoders: VecDeque<(Weak<SourceImage>, layer_color::WorkingDecoder)>,
    pixels: Vec<[f32; 4]>,
    inputs: [Option<EncodedInput>; 3],
    transfer: transfer::Tables,
    in_flight: Arc<InFlight>,
    pub hits: u64,
    pub misses: u64,
}
impl DecodedTiles {
    pub fn new(destination: RgbSpace) -> Self {
        Self {
            destination,
            ..Self::default()
        }
    }
    pub fn prepare_transfer(
        &mut self,
        device: &wgpu::Device,
        space: RgbSpace,
    ) -> Result<crate::native_tiles::NativeTransfer, GpuRasterError> {
        self.transfer.prepare(device, space).cloned()
    }
    pub fn prepared_view(
        &self,
        source: &Arc<SourceImage>,
        coordinate: [u32; 2],
    ) -> Option<&wgpu::TextureView> {
        let key = Key::Image(Arc::downgrade(source), coordinate);
        self.slots
            .iter()
            .find(|s| {
                s.key.as_ref().is_some_and(|k| k.matches(&key)) && s.valid.load(Ordering::Acquire)
            })
            .map(|s| &s.view)
    }
    pub fn uploads_full(&self) -> bool {
        // Preserve the 16 MiB worst-case staging ceiling, reserving room for
        // one Float32 tile. U8/U16 inputs consume less; charging them as full
        // Float32 uploads needlessly stalls ordinary layered strokes.
        self.in_flight.bytes.load(Ordering::Acquire)
            > (SOURCE_SLOTS as u64 - 1) * FLOAT_TILE_BYTES
    }
    pub fn prepared_raster_view(&self, blob: &Arc<TileBlob>, space: RgbSpace) -> Option<&wgpu::TextureView> {
        let key = Key::Raster(blob.digest, space, self.destination);
        self.slots.iter().find(|s| s.key.as_ref().is_some_and(|k| k.matches(&key)) && s.valid.load(Ordering::Acquire))
            .map(|s| &s.view)
    }

    pub fn charge_upload(&self, encoder: &crate::submission::CommandEncoder, bytes: u64) -> u64 {
        let total = self.in_flight.bytes.fetch_add(bytes, Ordering::Relaxed) + bytes;
        let charge = UploadCharge(self.in_flight.clone(), bytes);
        encoder.on_submitted_work_done(move || drop(charge));
        total
    }
    pub fn gpu_bytes(&self) -> u64 {
        self.slots.len() as u64 * FLOAT_TILE_BYTES
            + self.transfer.gpu_bytes()
            + self
                .inputs
                .iter()
                .flatten()
                .map(|input| {
                    PAGE_SIZE as u64
                        * PAGE_SIZE as u64
                        * input.texture.format().block_copy_size(None).unwrap() as u64
                })
                .sum::<u64>()
    }

    pub fn plan(
        &mut self,
        r: &WgpuRasterizer,
        source: &Arc<SourceImage>,
        coordinate: [u32; 2],
    ) -> Result<(RawTile, Option<PendingTile>), GpuRasterError> {
        let (tile, write) = self.plan_key(r, Key::Image(Arc::downgrade(source), coordinate))?;
        let pending = write.map(|write| PendingTile {
            pixels: Pixels::Image(source.clone(), coordinate),
            texture: tile.texture.clone(),
            view: tile.view.clone(),
            data: builtin_settings(source, coordinate, self.destination),
            write,
        });
        Ok((tile, pending))
    }

    /// Committed native paint shares the original-image cache, transfer tables
    /// and upload ceiling. Cache keys never retain compressed history backing.
    pub fn plan_raster(
        &mut self,
        r: &WgpuRasterizer,
        blob: &Arc<TileBlob>,
        space: RgbSpace,
        destination: RgbSpace,
    ) -> Result<(RawTile, Option<PendingTile>), GpuRasterError> {
        validate_raster(blob, space)?;
        let d = blob.descriptor;
        let depth = d.depth();
        let (tile, write) =
            self.plan_key(r, Key::Raster(blob.digest, space, destination))?;
        let pending = write.map(|write| PendingTile {
            pixels: Pixels::Raster(blob.clone(), space),
            texture: tile.texture.clone(),
            view: tile.view.clone(),
            data: Some(rgb_settings(
                space,
                destination,
                depth,
                [PAGE_SIZE; 2],
                d.alpha,
            )),
            write,
        });
        Ok((tile, pending))
    }

    fn plan_key(
        &mut self,
        r: &WgpuRasterizer,
        key: Key,
    ) -> Result<(RawTile, Option<crate::submission::CacheWrite>), GpuRasterError> {
        if !r
            .device
            .features()
            .contains(wgpu::Features::FLOAT32_FILTERABLE)
        {
            return Err(GpuRasterError::Color(
                "This device cannot filter Float32 source tiles".into(),
            ));
        }
        self.clock = self.clock.wrapping_add(1);
        if let Some(slot) = self.slots.iter_mut().find(|s| {
            s.key.as_ref().is_some_and(|k| k.matches(&key)) && s.valid.load(Ordering::Acquire)
        }) {
            slot.used = self.clock;
            self.hits += 1;
            return Ok((
                RawTile {
                    texture: slot.texture.clone(),
                    view: slot.view.clone(),
                },
                None,
            ));
        }
        self.misses += 1;
        let index = if self.slots.len() < DECODED_SLOTS {
            let texture = r.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("bounded Float32 source tile"),
                size: wgpu::Extent3d {
                    width: PAGE_SIZE,
                    height: PAGE_SIZE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let id = self.slots.len();
            self.slots.push(Slot {
                key: None,
                texture,
                view,
                used: 0,
                valid: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            });
            id
        } else {
            self.slots
                .iter()
                .enumerate()
                .min_by_key(|(_, s)| s.used)
                .unwrap()
                .0
        };
        let slot = &mut self.slots[index];
        slot.key = Some(key);
        slot.used = self.clock;
        let write = crate::submission::CacheWrite::new();
        slot.valid = write.validity();
        Ok((
            RawTile {
                texture: slot.texture.clone(),
                view: slot.view.clone(),
            },
            Some(write),
        ))
    }

    pub fn encode(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        pending: &PendingTile,
        uniforms: &wgpu::BindGroup,
        offset: u32,
    ) -> Result<u64, GpuRasterError> {
        let bytes = if pending.data.is_some() {
            let samples = pending.pixels.native()?;
            let index = samples.depth.bytes().ilog2() as usize;
            let pipelines = &r.scene_pipelines.source;
            let input = self.inputs[index].get_or_insert_with(|| {
                let texture = r.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("bounded integer source input"),
                    size: pending.texture.size(),
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: [
                        wgpu::TextureFormat::Rgba8Uint,
                        wgpu::TextureFormat::Rgba16Uint,
                        wgpu::TextureFormat::Rgba32Uint,
                    ][index],
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                EncodedInput {
                    texture,
                    bindings: Default::default(),
                }
            });
            let space = samples.space;
            let table = self.transfer.prepare(&r.device, space)?;
            let binding = input.bindings[transfer::Tables::index(space)].get_or_insert_with(|| {
                r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("native integer source and shared transfer"),
                    layout: &pipelines.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(
                                &input.texture.create_view(&Default::default()),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: table.as_entire_binding(),
                        },
                    ],
                })
            });
            if samples.tile.descriptor != samples.descriptor {
                return Err(GpuRasterError::Color(
                    "Source tile has the wrong sample representation".into(),
                ));
            }
            let decoded = r.device.source_samples.decode(samples.tile).map_err(GpuRasterError::Color)?;
            let step = samples.depth.bytes();
            let bytes = (PAGE_SIZE * PAGE_SIZE) as usize * 4 * step;
            r.uploads.write_texture(encoder, &input.texture, PAGE_SIZE * 4 * step as u32, |mapped| {
                if samples.channels == SourceChannels::Rgba {
                    mapped.copy_from_slice(&decoded);
                } else {
                    // Expand channels in one row of scratch. Alpha is coverage;
                    // grayscale uses the declared RGB transfer on equal R/G/B.
                    let mut row = [0u8; PAGE_SIZE as usize * 8];
                    let bpp = samples.channels.count() * step;
                    let row_bytes = PAGE_SIZE as usize * 4 * step;
                    for (y, input) in decoded.chunks_exact(PAGE_SIZE as usize * bpp).enumerate() {
                        expand_source_row(input, &mut row[..row_bytes], samples.channels, samples.depth);
                        mapped
                            .slice(y * row_bytes..(y + 1) * row_bytes)
                            .copy_from_slice(&row[..row_bytes]);
                    }
                }
            })?;
            let attachments = [Some(attachment(
                &pending.view,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            ))];
            let mut pass = encoder.begin_render_pass(&descriptor(&attachments));
            pass.set_pipeline(pipelines.pipeline.compile());
            pass.set_bind_group(0, uniforms, &[offset]);
            pass.set_bind_group(1, &*binding, &[]);
            pass.draw(0..3, 0..1);
            bytes as u64
        } else {
            let Pixels::Image(source, coordinate) = &pending.pixels else {
                unreachable!()
            };
            self.upload_icc(r, source, *coordinate, encoder, &pending.texture)?;
            FLOAT_TILE_BYTES
        };
        pending.write.track(encoder);
        Ok(bytes)
    }

    fn upload_icc(
        &mut self,
        r: &mut WgpuRasterizer,
        source: &Arc<SourceImage>,
        coordinate: [u32; 2],
        encoder: &mut crate::submission::CommandEncoder,
        texture: &wgpu::Texture,
    ) -> Result<(), GpuRasterError> {
        let weak = Arc::downgrade(source);
        let index = if let Some(index) = self.decoders.iter().position(|(s, _)| s.ptr_eq(&weak)) {
            index
        } else {
            // Keep the source native; decode into the document's working primaries.
            let decoder = layer_color::WorkingDecoder::new(
                &source.interpretation,
                self.destination,
                Default::default(),
            )
            .map_err(GpuRasterError::Color)?;
            if self.decoders.len() == DECODERS {
                self.decoders.pop_front();
            }
            self.decoders.push_back((weak, decoder));
            self.decoders.len() - 1
        };
        let decoder = self.decoders.remove(index).unwrap();
        self.pixels
            .resize((PAGE_SIZE * PAGE_SIZE) as usize, [0.; 4]);
        decoder
            .1
            .decode_tile_cached(source, coordinate, &mut self.pixels, &r.device.source_samples)
            .map_err(GpuRasterError::Color)?;
        self.decoders.push_back(decoder);
        r.uploads.write_texture(encoder, texture, PAGE_SIZE * 16, |mapped| {
            let mut row = [0u8; PAGE_SIZE as usize * 16];
            for (y, pixels) in self.pixels.chunks_exact(PAGE_SIZE as usize).enumerate() {
                for (bytes, pixel) in row.chunks_exact_mut(16).zip(pixels) {
                    for (channel, bytes) in bytes.chunks_exact_mut(4).enumerate() {
                        let value = if channel == 3 {
                            pixel[3]
                        } else {
                            pixel[channel] * pixel[3]
                        };
                        bytes.copy_from_slice(&value.to_ne_bytes());
                    }
                }
                mapped
                    .slice(y * row.len()..(y + 1) * row.len())
                    .copy_from_slice(&row);
            }
        })
    }
}

pub(super) fn validate_raster(blob: &TileBlob, space: RgbSpace) -> Result<(), GpuRasterError> {
    let d = blob.descriptor;
    if d.channels != 4
        || d.bytes_per_pixel().is_none()
        || !matches!(
            d.alpha,
            AlphaAssociation::Straight | AlphaAssociation::PremultipliedLinear
        )
        || !(d.sample == layer_core::color::SampleType::Float && d.encoding == TransferEncoding::Linear && matches!(d.bits_per_channel, 16 | 32)
            || d.encoding == TransferEncoding::Profile
            || (d.encoding == TransferEncoding::Srgb && space == RgbSpace::Srgb))
    {
        return Err(GpuRasterError::Color(
            "Invalid native raster decode representation".into(),
        ));
    }
    Ok(())
}

/// Expand integer source channels without changing their codes or byte order.
/// Fixed pixel widths keep the hot upload path out of per-channel dynamic copies.
fn expand_source_row(input: &[u8], output: &mut [u8], channels: SourceChannels, depth: SampleDepth) {
    match (depth, channels) {
        (SampleDepth::U8, SourceChannels::Rgb) => {
            for (p, o) in input.chunks_exact(3).zip(output.chunks_exact_mut(4)) {
                o.copy_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        (SampleDepth::U8, SourceChannels::Gray) => {
            for (p, o) in input.iter().zip(output.chunks_exact_mut(4)) {
                o.copy_from_slice(&[*p, *p, *p, 255]);
            }
        }
        (SampleDepth::U8, SourceChannels::GrayAlpha) => {
            for (p, o) in input.chunks_exact(2).zip(output.chunks_exact_mut(4)) {
                o.copy_from_slice(&[p[0], p[0], p[0], p[1]]);
            }
        }
        (SampleDepth::U16, SourceChannels::Rgb) => {
            for (p, o) in input.chunks_exact(6).zip(output.chunks_exact_mut(8)) {
                o.copy_from_slice(&[p[0], p[1], p[2], p[3], p[4], p[5], 255, 255]);
            }
        }
        (SampleDepth::U16, SourceChannels::Gray) => {
            for (p, o) in input.chunks_exact(2).zip(output.chunks_exact_mut(8)) {
                o.copy_from_slice(&[p[0], p[1], p[0], p[1], p[0], p[1], 255, 255]);
            }
        }
        (SampleDepth::U16, SourceChannels::GrayAlpha) => {
            for (p, o) in input.chunks_exact(4).zip(output.chunks_exact_mut(8)) {
                o.copy_from_slice(&[p[0], p[1], p[0], p[1], p[0], p[1], p[2], p[3]]);
            }
        }
        (SampleDepth::F32, SourceChannels::Rgb) => {
            for (p, o) in input.chunks_exact(12).zip(output.chunks_exact_mut(16)) {
                o[..12].copy_from_slice(p);
                o[12..].copy_from_slice(&1f32.to_le_bytes());
            }
        }
        (SampleDepth::F16, SourceChannels::Rgb) => {
            for (p,o) in input.chunks_exact(6).zip(output.chunks_exact_mut(8)) { o[..6].copy_from_slice(p); o[6..].copy_from_slice(&0x3c00u16.to_le_bytes()); }
        }
        (SampleDepth::F16 | SampleDepth::F32, SourceChannels::Gray | SourceChannels::GrayAlpha) => unreachable!("invalid HDR gray"),
        (_, SourceChannels::Rgba | SourceChannels::Cmyk) => unreachable!("RGBA copies directly; CMYK uses its ICC transform"),
    }
}

fn builtin_settings(
    source: &SourceImage,
    coordinate: [u32; 2],
    destination: RgbSpace,
) -> Option<[f32; 24]> {
    let ColorProfile::Builtin(space) = source.interpretation.profile else {
        return None;
    };
    if source.interpretation.channels == SourceChannels::Cmyk {
        return None;
    }
    Some(rgb_settings(
        space,
        destination,
        source.interpretation.depth,
        std::array::from_fn(|i| {
            source.extent[i]
                .saturating_sub(coordinate[i] * PAGE_SIZE)
                .min(PAGE_SIZE)
        }),
        AlphaAssociation::Straight,
    ))
}
fn rgb_settings(
    space: RgbSpace,
    destination: RgbSpace,
    depth: SampleDepth,
    extent: [u32; 2],
    alpha: AlphaAssociation,
) -> [f32; 24] {
    let mut data = [0.; 24];
    for (i, row) in space.linear_transform(destination).iter().enumerate() {
        data[i * 4..i * 4 + 3].copy_from_slice(&row.map(|v| v as f32));
    }
    data[12] = f32::from(alpha == AlphaAssociation::PremultipliedLinear);
    data[13] = if depth.is_float() { 0. } else { depth.maximum() as f32 };
    data[16] = f32::from(depth == SampleDepth::F32);
    data[17] = f32::from(space == destination);
    data[14] = extent[0] as f32;
    data[15] = extent[1] as f32;
    data
}

#[derive(Clone)]
pub(crate) struct Pipelines {
    layout: wgpu::BindGroupLayout,
    pub pipeline: Deferred<wgpu::RenderPipeline>,
}
impl Pipelines {
    pub fn new(device: &PipelineDevice, uniforms: &wgpu::BindGroupLayout) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("integer source samples"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
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
                        min_binding_size: wgpu::BufferSize::new(transfer::TABLE_BYTES),
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("source decode"),
            bind_group_layouts: &[Some(uniforms), Some(&layout)],
            immediate_size: 0,
        });
        let device = device.clone();
        let pipeline = Deferred::pipeline(move |mode| {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("integer SDR to Float32"),
                source: wgpu::ShaderSource::Wgsl(format!("{}\n{}", include_str!("../native_tiles/validity.wgsl"), include_str!("source_decode.wgsl")).into()),
            });
            fullscreen_pipeline_recipe(
                mode,
                &device,
                &pipeline_layout,
                &shader,
                "fragment_main",
                None,
                wgpu::TextureFormat::Rgba32Float,
                "decode source tile",
            )
        });
        Self { layout, pipeline }
    }
}

#[cfg(test)]
mod tests;
