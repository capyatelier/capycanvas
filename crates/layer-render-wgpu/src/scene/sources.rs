//! GPU source slots have fixed ownership. Jobs defer decoding until their
//! ordered upload; eviction never creates another retained GPU tile. Built-in
//! profiles decode on the GPU, while embedded profiles use the native CMM.
use super::*;
use crate::source_access::RawTile;
use layer_core::color::{
    ColorProfile, IntegerDepth, RgbSpace,
    source::{SourceChannels, SourceImage},
};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};

pub(super) const FLOAT_TILE_BYTES: u64 = PAGE_SIZE as u64 * PAGE_SIZE as u64 * 16;
const DECODERS: usize = 4;

struct Slot {
    source: Weak<SourceImage>,
    coordinate: [u32; 2],
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    used: u64,
    valid: Arc<std::sync::atomic::AtomicBool>,
}
pub(super) struct PendingSource {
    pub source: Arc<SourceImage>,
    pub coordinate: [u32; 2],
    pub texture: wgpu::Texture,
    pub data: Option<[f32; 24]>,
    write: crate::submission::CacheWrite,
}
struct EncodedInput {
    texture: wgpu::Texture,
    binding: wgpu::BindGroup,
}
#[derive(Default)]
struct InFlight {
    count: AtomicUsize,
    bytes: AtomicU64,
}
// Dropping an unsubmitted encoder also releases its charge. A failed frame
// cannot strand capacity while device-loss recovery replaces the renderer.
struct UploadCharge(Arc<InFlight>, u64);
impl Drop for UploadCharge {
    fn drop(&mut self) {
        self.0.bytes.fetch_sub(self.1, Ordering::Relaxed);
        self.0.count.fetch_sub(1, Ordering::Release);
    }
}
#[derive(Default)]
pub(super) struct SourceTiles {
    destination: RgbSpace,
    slots: Vec<Slot>,
    clock: u64,
    decoders: VecDeque<(Weak<SourceImage>, layer_color::WorkingDecoder)>,
    pixels: Vec<[f32; 4]>,
    inputs: [Option<EncodedInput>; 2],
    in_flight: Arc<InFlight>,
    pub hits: u64,
    pub misses: u64,
}
impl SourceTiles {
    pub fn prepared_view(
        &self,
        source: &Arc<SourceImage>,
        coordinate: [u32; 2],
    ) -> Option<&wgpu::TextureView> {
        let weak = Arc::downgrade(source);
        self.slots
            .iter()
            .find(|s| {
                s.coordinate == coordinate
                    && s.source.ptr_eq(&weak)
                    && s.valid.load(Ordering::Acquire)
            })
            .map(|s| &s.view)
    }
    pub fn uploads_full(&self) -> bool {
        self.in_flight.count.load(Ordering::Acquire) >= SOURCE_SLOTS
    }

    pub fn charge_upload(&self, encoder: &crate::submission::CommandEncoder, bytes: u64) -> u64 {
        self.in_flight.count.fetch_add(1, Ordering::Relaxed);
        let total = self.in_flight.bytes.fetch_add(bytes, Ordering::Relaxed) + bytes;
        let charge = UploadCharge(self.in_flight.clone(), bytes);
        encoder.on_submitted_work_done(move || drop(charge));
        total
    }
    pub fn gpu_bytes(&self) -> u64 {
        self.slots.len() as u64 * FLOAT_TILE_BYTES
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
    ) -> Result<(RawTile, Option<PendingSource>), GpuRasterError> {
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
        let weak = Arc::downgrade(source);
        if let Some(slot) = self.slots.iter_mut().find(|s| {
            s.source.ptr_eq(&weak) && s.coordinate == coordinate && s.valid.load(Ordering::Acquire)
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
        let index = if self.slots.len() < SOURCE_SLOTS {
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
                source: Weak::new(),
                coordinate,
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
        slot.source = weak;
        slot.coordinate = coordinate;
        slot.used = self.clock;
        let write = crate::submission::CacheWrite::new();
        slot.valid = write.validity();
        Ok((
            RawTile {
                texture: slot.texture.clone(),
                view: slot.view.clone(),
            },
            Some(PendingSource {
                source: source.clone(),
                coordinate,
                texture: slot.texture.clone(),
                data: builtin_settings(source, coordinate, self.destination),
                write,
            }),
        ))
    }

    pub fn encode(
        &mut self,
        r: &WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        pending: &PendingSource,
        uniforms: &wgpu::BindGroup,
        offset: u32,
    ) -> Result<u64, GpuRasterError> {
        let bytes = if pending.data.is_some() {
            let interpretation = &pending.source.interpretation;
            let index = usize::from(interpretation.depth == IntegerDepth::U16);
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
                    ][index],
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let binding = r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("integer source decoder"),
                    layout: &pipelines.layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(
                            &texture.create_view(&Default::default()),
                        ),
                    }],
                });
                EncodedInput { texture, binding }
            });
            let tile = pending
                .source
                .tiles
                .get(&pending.coordinate)
                .ok_or_else(|| GpuRasterError::Color("Missing source tile".into()))?;
            if tile.descriptor != interpretation.descriptor() {
                return Err(GpuRasterError::Color(
                    "Source tile has the wrong sample representation".into(),
                ));
            }
            let decoded = tile.decode().map_err(GpuRasterError::Color)?;
            let step = interpretation.depth.bytes();
            let bytes = (PAGE_SIZE * PAGE_SIZE) as usize * 4 * step;
            let buffer = r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("bounded integer source upload"),
                size: bytes as u64,
                usage: wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: true,
            });
            {
                let mut mapped = buffer
                    .get_mapped_range_mut(..)
                    .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?;
                if interpretation.channels == SourceChannels::Rgba {
                    mapped.copy_from_slice(&decoded);
                } else {
                    // Expand channels in one row of scratch. Alpha is coverage;
                    // grayscale uses the declared RGB transfer on equal R/G/B.
                    let mut row = [0u8; PAGE_SIZE as usize * 8];
                    let bpp = interpretation.pixel_bytes();
                    let row_bytes = PAGE_SIZE as usize * 4 * step;
                    for (y, input) in decoded.chunks_exact(PAGE_SIZE as usize * bpp).enumerate() {
                        for (pixel, rgba) in input
                            .chunks_exact(bpp)
                            .zip(row[..row_bytes].chunks_exact_mut(4 * step))
                        {
                            for c in 0..3 {
                                let channel = if interpretation.channels == SourceChannels::Rgb {
                                    c
                                } else {
                                    0
                                };
                                rgba[c * step..(c + 1) * step]
                                    .copy_from_slice(&pixel[channel * step..(channel + 1) * step]);
                            }
                            if interpretation.channels.has_alpha() {
                                rgba[3 * step..].copy_from_slice(&pixel[bpp - step..]);
                            } else {
                                rgba[3 * step..].fill(255);
                            }
                        }
                        mapped
                            .slice(y * row_bytes..(y + 1) * row_bytes)
                            .copy_from_slice(&row[..row_bytes]);
                    }
                }
            }
            buffer.unmap();
            copy_upload(
                encoder,
                &buffer,
                &input.texture,
                PAGE_SIZE * 4 * step as u32,
            );
            let target = pending.texture.create_view(&Default::default());
            let attachments = [Some(attachment(
                &target,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            ))];
            let mut pass = encoder.begin_render_pass(&descriptor(&attachments));
            pass.set_pipeline(pipelines.pipeline.compile());
            pass.set_bind_group(0, uniforms, &[offset]);
            pass.set_bind_group(1, &input.binding, &[]);
            pass.draw(0..3, 0..1);
            bytes as u64
        } else {
            let buffer = self.upload_icc(r, pending)?;
            copy_upload(encoder, &buffer, &pending.texture, PAGE_SIZE * 16);
            FLOAT_TILE_BYTES
        };
        pending.write.track(encoder);
        Ok(bytes)
    }

    fn upload_icc(
        &mut self,
        r: &WgpuRasterizer,
        pending: &PendingSource,
    ) -> Result<wgpu::Buffer, GpuRasterError> {
        let weak = Arc::downgrade(&pending.source);
        let index = if let Some(index) = self.decoders.iter().position(|(s, _)| s.ptr_eq(&weak)) {
            index
        } else {
            // Current exposed documents are sRGB8. The source stays native;
            // future document working-space selection supplies this destination.
            let decoder = layer_color::WorkingDecoder::new(
                &pending.source.interpretation,
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
            .decode_tile(&pending.source, pending.coordinate, &mut self.pixels)
            .map_err(GpuRasterError::Color)?;
        self.decoders.push_back(decoder);
        let buffer = r.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bounded linear source upload"),
            size: FLOAT_TILE_BYTES,
            usage: wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: true,
        });
        {
            let mut mapped = buffer
                .get_mapped_range_mut(..)
                .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?;
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
        }
        buffer.unmap();
        Ok(buffer)
    }
}

fn copy_upload(
    encoder: &mut crate::submission::CommandEncoder,
    buffer: &wgpu::Buffer,
    texture: &wgpu::Texture,
    bytes_per_row: u32,
) {
    encoder.copy_buffer_to_texture(
        wgpu::TexelCopyBufferInfo {
            buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(PAGE_SIZE),
            },
        },
        texture.as_image_copy(),
        texture.size(),
    );
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
    let mut data = [0.; 24];
    for (i, row) in space.linear_transform(destination).iter().enumerate() {
        data[i * 4..i * 4 + 3].copy_from_slice(&row.map(|v| v as f32));
    }
    data[12] = RgbSpace::ALL.iter().position(|s| *s == space).unwrap() as f32;
    data[13] = source.interpretation.depth.maximum() as f32;
    data[14] = source.extent[0]
        .saturating_sub(coordinate[0] * PAGE_SIZE)
        .min(PAGE_SIZE) as f32;
    data[15] = source.extent[1]
        .saturating_sub(coordinate[1] * PAGE_SIZE)
        .min(PAGE_SIZE) as f32;
    Some(data)
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
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("source decode"),
            bind_group_layouts: &[Some(uniforms), Some(&layout)],
            immediate_size: 0,
        });
        let device = device.clone();
        let pipeline = Deferred::new(move || {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("integer SDR to Float32"),
                source: wgpu::ShaderSource::Wgsl(
                    format!(
                        "{}\n{}",
                        include_str!("../sdr_color.wgsl"),
                        include_str!("source_decode.wgsl")
                    )
                    .into(),
                ),
            });
            fullscreen_pipeline(
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
