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
// Retain neighboring source tiles independently of staging memory. Immutable
// owners and equal generated tile contents share decoded pixels across revisions.
const DECODED_SLOTS: usize = 64;
const MIN_DECODED_SLOTS: usize = SOURCE_SLOTS + 4;
const DECODERS: usize = 4;
use crate::native_tiles::transfer;

#[derive(Clone, Copy)]
struct SourceLimits {
    slots: usize,
    upload_bytes: u64,
}
impl Default for SourceLimits {
    fn default() -> Self { Self { slots: DECODED_SLOTS, upload_bytes: 16 * 1024 * 1024 } }
}
impl SourceLimits {
    fn admitted(display_allowance: u64) -> Self {
        // Reuse the native renderer's measured headroom admission snapshot.
        // Unknown/small budgets retain the original 64+16 MiB ceilings. Larger
        // devices admit at most 1 GiB of source pixels and 64 MiB in flight;
        // their source pixels use no more than one quarter of that allowance.
        // Admit whole tiles instead of rounding down to power-of-two tiers.
        // The resident cache may outlive an upload window: unchanged source
        // pixels should not be decoded repeatedly during a broad stroke.
        let slots = (display_allowance / (4 * FLOAT_TILE_BYTES))
            .clamp(DECODED_SLOTS as u64, 1024) as usize;
        Self { slots, upload_bytes: (slots as u64 * FLOAT_TILE_BYTES / 4).min(64 * 1024 * 1024) }
    }
}

#[derive(PartialEq, Eq)]
enum RasterIdentity {
    Encoded([u8; 32]),
    Owner(u64),
}
enum Key {
    Image(Weak<SourceImage>, [u32; 2]),
    PaintBase(Weak<SourceImage>, [u32; 2], [u32; 2]),
    Raster(RasterIdentity, RgbSpace, RgbSpace),
}
impl Key {
    fn raster(blob: &TileBlob, space: RgbSpace, destination: RgbSpace) -> Self {
        Self::Raster(blob.encoded_fingerprint().map_or_else(
            || RasterIdentity::Owner(blob.owner_identity()), RasterIdentity::Encoded,
        ), space, destination)
    }
    fn matches(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Image(a, x), Self::Image(b, y)) => a.ptr_eq(b) && x == y,
            (Self::PaintBase(a, x, p), Self::PaintBase(b, y, q)) => a.ptr_eq(b) && x == y && p == q,
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
    lease: Arc<()>,
}
struct RetiredSlot { _slot: Slot, retired: Arc<AtomicU64>, bytes: u64, completed: Arc<std::sync::atomic::AtomicBool> }
impl Drop for RetiredSlot {
    fn drop(&mut self) { self.retired.fetch_sub(self.bytes, Ordering::Release); }
}
struct RetirementCompletion(Arc<std::sync::atomic::AtomicBool>);
impl Drop for RetirementCompletion {
    fn drop(&mut self) { self.0.store(true, Ordering::Release); }
}
enum Pixels {
    Image(Arc<SourceImage>, [u32; 2]),
    Raster(Arc<TileBlob>, RgbSpace),
}
pub enum PreparedSourcePixels { NativeSamples(Arc<Vec<u8>>), PremultipliedWorkingPixels(Arc<Vec<u8>>) }

pub(crate) struct PendingTile {
    pixels: Pixels,
    pub texture: wgpu::Texture,
    pub(super) view: wgpu::TextureView,
    pub data: Option<[f32; 24]>,
    write: crate::submission::CacheWrite,
    pub prepared_pixels: Option<PreparedSourcePixels>,
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
                channels: match tile.descriptor.channels { 1 => SourceChannels::Gray, 2 => SourceChannels::GrayAlpha, _ => SourceChannels::Rgba },
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
pub(crate) struct DecodedTiles {
    destination: RgbSpace,
    limits: SourceLimits,
    slots: Vec<Slot>,
    clock: u64,
    decoders: VecDeque<(Weak<SourceImage>, layer_color::WorkingDecoder)>,
    pixels: Vec<[f32; 4]>,
    inputs: [Option<EncodedInput>; 3],
    transfer: transfer::Tables,
    in_flight: Arc<InFlight>,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    mip_reservation: u64,
    retired_bytes: Arc<AtomicU64>,
    retired_slots: std::cell::RefCell<Vec<RetiredSlot>>,
}
impl DecodedTiles {
    pub fn new(destination: RgbSpace) -> Self {
        Self {
            destination,
            ..Self::default()
        }
    }
    pub fn admit(&mut self, allowance: u64) {
        debug_assert!(self.slots.is_empty(), "source admission precedes pixel allocation");
        self.limits = SourceLimits::admitted(allowance);
    }
    pub fn admitted_bytes(&self) -> [u64; 2] {
        [self.limits.slots as u64 * self.tile_bytes(), self.limits.upload_bytes]
    }
    pub fn mip_budget(&self) -> u64 {
        (self.limits.slots.saturating_sub(MIN_DECODED_SLOTS) as u64) * self.tile_bytes()
    }
    fn slot_limit(&self) -> usize {
        self.limits.slots - self.mip_reservation.div_ceil(self.tile_bytes()) as usize
    }
    pub fn reserve_mips(&mut self, bytes: u64, encoder: &crate::submission::CommandEncoder) -> Result<bool, GpuRasterError> {
        self.reap_retired();
        if bytes > self.mip_budget() { return Err(GpuRasterError::SourceWorkingSetExceeded); }
        self.mip_reservation = bytes;
        while self.slots.len() > self.slot_limit() {
            let Some(index) = self.slots.iter().enumerate().filter(|(_,slot)| Arc::strong_count(&slot.lease)==1)
                .min_by_key(|(_,slot)|slot.used).map(|(index,_)|index) else { return Ok(false); };
            let slot = self.slots.swap_remove(index);
            if slot.key.is_some() { self.evictions += 1; }
            let retired = self.retired_bytes.clone();
            let tile_bytes = self.tile_bytes();
            retired.fetch_add(tile_bytes, Ordering::Relaxed);
            let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
            self.retired_slots.get_mut().push(RetiredSlot { _slot:slot, retired, bytes:tile_bytes, completed:completed.clone() });
            let completion = RetirementCompletion(completed);
            encoder.on_submitted_work_done(move || drop(completion));
        }
        Ok(self.slots.len() as u64 * self.tile_bytes() + self.retired_bytes.load(Ordering::Acquire) + bytes
            <= self.limits.slots as u64 * self.tile_bytes())
    }
    fn tile_bytes(&self) -> u64 {
        FLOAT_TILE_BYTES
    }
    fn reap_retired(&self) {
        self.retired_slots.borrow_mut().retain(|slot| !slot.completed.load(Ordering::Acquire));
    }
    pub fn prepare_transfer(
        &mut self,
        device: &PipelineDevice,
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
    pub fn prepared_base_view(&self, base: &layer_core::authored::PaintBase, coordinate: [u32; 2]) -> Option<&wgpu::TextureView> {
        if !crate::source_access::paint_base_contains(base, coordinate) { return None; }
        if base.offset.iter().all(|v| v % PAGE_SIZE == 0) {
            return self.prepared_view(base.image.storage(), std::array::from_fn(|i| coordinate[i] - base.offset[i] / PAGE_SIZE));
        }
        let key = Key::PaintBase(Arc::downgrade(base.image.storage()), base.offset, coordinate);
        self.slots.iter().find(|s| s.key.as_ref().is_some_and(|k| k.matches(&key)) && s.valid.load(Ordering::Acquire)).map(|s| &s.view)
    }
    pub(crate) fn plan_base(&mut self, r: &WgpuRasterizer, base: &layer_core::authored::PaintBase, coordinate: [u32; 2]) -> Result<(RawTile, Option<crate::submission::CacheWrite>), GpuRasterError> {
        self.plan_key(r, Key::PaintBase(Arc::downgrade(base.image.storage()), base.offset, coordinate))
    }
    pub fn uploads_full(&self) -> bool {
        // Reserve room for one Float32 tile within the bounded staging window.
        // U8/U16 inputs consume less; charge their actual encoded byte size.
        self.in_flight.bytes.load(Ordering::Acquire)
            > self.limits.upload_bytes - FLOAT_TILE_BYTES
    }
    pub fn prepared_raster_view(&self, blob: &Arc<TileBlob>, space: RgbSpace) -> Option<&wgpu::TextureView> {
        let key = Key::raster(blob, space, self.destination);
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
        self.reap_retired();
        self.slots.len() as u64 * self.tile_bytes()
            + self.retired_bytes.load(Ordering::Acquire)
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

    pub(crate) fn plan(
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
            prepared_pixels: None,
        });
        Ok((tile, pending))
    }

    /// Committed native paint shares the original-image cache, transfer tables
    /// and upload ceiling. Cache keys never retain compressed history backing.
    pub(super) fn plan_raster(
        &mut self,
        r: &WgpuRasterizer,
        blob: &Arc<TileBlob>,
        space: RgbSpace,
        destination: RgbSpace,
    ) -> Result<(RawTile, Option<PendingTile>), GpuRasterError> {
        validate_raster(blob, space)?;
        let d = blob.descriptor;
        let depth = d.depth();
        let mut data = rgb_settings(space, destination, depth, [PAGE_SIZE; 2], d.alpha);
        data[18] = f32::from(d.channels == 1);
        let (tile, write) =
            self.plan_key(r, Key::raster(blob, space, destination))?;
        let pending = write.map(|write| PendingTile {
            pixels: Pixels::Raster(blob.clone(), space),
            texture: tile.texture.clone(),
            view: tile.view.clone(),
            data: Some(data),
            write,
            prepared_pixels: None,
        });
        Ok((tile, pending))
    }

    pub fn lease(&self, view: &wgpu::TextureView) -> Option<Arc<()>> {
        self.slots.iter().find(|s| s.view == *view).map(|s| s.lease.clone())
    }

    fn plan_key(
        &mut self,
        r: &WgpuRasterizer,
        key: Key,
    ) -> Result<(RawTile, Option<crate::submission::CacheWrite>), GpuRasterError> {
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
        let index = if self.slots.len() < self.slot_limit() {
            let texture = r.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("bounded source tile"),
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
                lease: Arc::new(()),
            });
            id
        } else {
            self.slots
                .iter()
                .enumerate()
                .filter(|(_, s)| Arc::strong_count(&s.lease) == 1)
                .min_by_key(|(_, s)| s.used)
                .ok_or(GpuRasterError::SourceWorkingSetExceeded)?
                .0
        };
        let slot = &mut self.slots[index];
        if slot.key.is_some() { self.evictions += 1; }
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

    #[allow(clippy::too_many_arguments)]
    pub(super) fn encode(
        &mut self,
        device: &crate::PipelineDevice,
        uploads: &mut crate::Uploads,
        pipelines: &Pipelines,
        encoder: &mut crate::submission::CommandEncoder,
        pending: &PendingTile,
        uniforms: &wgpu::BindGroup,
        offset: u32,
    ) -> Result<u64, GpuRasterError> {
        let bytes = if pending.data.is_some() {
            let samples = pending.pixels.native()?;
            let index = samples.depth.bytes().ilog2() as usize;
            let input = self.inputs[index].get_or_insert_with(|| {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
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
            let table = self.transfer.prepare(device, space)?;
            let binding = input.bindings[transfer::Tables::index(space)].get_or_insert_with(|| {
                crate::bindings::group(device, "native integer source and shared transfer", &pipelines.layout, [
                    wgpu::BindingResource::TextureView( &input.texture.create_view(&Default::default()), ),
                    table.as_entire_binding(),
                ])
            });
            if samples.tile.descriptor != samples.descriptor {
                return Err(GpuRasterError::Color(
                    "Source tile has the wrong sample representation".into(),
                ));
            }
            let decoded = match &pending.prepared_pixels {
                Some(PreparedSourcePixels::NativeSamples(samples)) => samples.clone(),
                Some(PreparedSourcePixels::PremultipliedWorkingPixels(_)) => return Err(GpuRasterError::Color("Working source pixels have native decode settings".into())),
                None => device.source_samples.decode(samples.tile).map_err(GpuRasterError::Color)?,
            };
            let step = samples.depth.bytes();
            let prepared = pending.prepared_pixels.is_some();
            let channels = if prepared { 4 } else { samples.channels.count() };
            if decoded.len() != (PAGE_SIZE * PAGE_SIZE) as usize * channels * step {
                return Err(GpuRasterError::Color("Prepared source samples have the wrong size".into()));
            }
            let bytes = (PAGE_SIZE * PAGE_SIZE) as usize * 4 * step;
            uploads.write_texture(encoder, &input.texture, PAGE_SIZE * 4 * step as u32, |mapped| {
                if prepared || samples.channels == SourceChannels::Rgba {
                    mapped.copy_from_slice(&decoded);
                } else {
                    // Expand channels in one row of scratch. Alpha is coverage;
                    // grayscale uses the declared RGB transfer on equal R/G/B.
                    let mut row = [0u8; PAGE_SIZE as usize * 16];
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
            match &pending.prepared_pixels {
                Some(PreparedSourcePixels::PremultipliedWorkingPixels(bytes)) => {
                    if bytes.len() as u64 != FLOAT_TILE_BYTES { return Err(GpuRasterError::InvalidExtent); }
                    uploads.write_texture(encoder, &pending.texture, PAGE_SIZE * 16, |mapped| mapped.copy_from_slice(bytes))?;
                }
                Some(PreparedSourcePixels::NativeSamples(_)) => return Err(GpuRasterError::Color("Native source samples require decode settings".into())),
                None => self.upload_icc(device, uploads, source, *coordinate, encoder, &pending.texture)?,
            }
            FLOAT_TILE_BYTES
        };
        pending.write.track(encoder);
        Ok(bytes)
    }

    fn upload_icc(
        &mut self,
        device: &crate::PipelineDevice,
        uploads: &mut crate::Uploads,
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
            .decode_tile_cached(source, coordinate, &mut self.pixels, &device.source_samples)
            .map_err(GpuRasterError::Color)?;
        self.decoders.push_back(decoder);
        upload_working_pixels(uploads, encoder, texture, &self.pixels)
    }
}

fn upload_working_pixels(uploads: &mut crate::Uploads, encoder: &mut crate::submission::CommandEncoder,
    texture: &wgpu::Texture, pixels: &[[f32; 4]],
) -> Result<(), GpuRasterError> {
    if pixels.len() != (PAGE_SIZE * PAGE_SIZE) as usize { return Err(GpuRasterError::InvalidExtent); }
        uploads.write_texture(encoder, texture, PAGE_SIZE * 16, |mapped| {
            let mut row = [0u8; PAGE_SIZE as usize * 16];
            for (y, pixels) in pixels.as_chunks::<{ PAGE_SIZE as usize }>().0.iter().enumerate() {
                for (bytes, pixel) in row.as_chunks_mut::<16>().0.iter_mut().zip(pixels) {
                    for (channel, bytes) in bytes.as_chunks_mut::<4>().0.iter_mut().enumerate() {
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

pub(super) fn validate_raster(blob: &TileBlob, space: RgbSpace) -> Result<(), GpuRasterError> {
    let d = blob.descriptor;
    if d.channels == 1 && d.sample == layer_core::color::SampleType::Unsigned
        && matches!(d.bits_per_channel, 8 | 16) && d.encoding == TransferEncoding::Linear
        && d.alpha == AlphaAssociation::None { return Ok(()); }
    if !matches!(d.channels, 2 | 4)
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
pub(crate) fn expand_source_row(input: &[u8], output: &mut [u8], channels: SourceChannels, depth: SampleDepth) {
    match (depth, channels) {
        (SampleDepth::U8, SourceChannels::Rgb) => {
            for (p, o) in input.as_chunks::<3>().0.iter().zip(output.as_chunks_mut::<4>().0.iter_mut()) {
                o.copy_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        (SampleDepth::U8, SourceChannels::Gray) => {
            for (p, o) in input.iter().zip(output.as_chunks_mut::<4>().0.iter_mut()) {
                o.copy_from_slice(&[*p, *p, *p, 255]);
            }
        }
        (SampleDepth::U8, SourceChannels::GrayAlpha) => {
            for (p, o) in input.as_chunks::<2>().0.iter().zip(output.as_chunks_mut::<4>().0.iter_mut()) {
                o.copy_from_slice(&[p[0], p[0], p[0], p[1]]);
            }
        }
        (SampleDepth::U16, SourceChannels::Rgb) => {
            for (p, o) in input.as_chunks::<6>().0.iter().zip(output.as_chunks_mut::<8>().0.iter_mut()) {
                o.copy_from_slice(&[p[0], p[1], p[2], p[3], p[4], p[5], 255, 255]);
            }
        }
        (SampleDepth::U16, SourceChannels::Gray) => {
            for (p, o) in input.as_chunks::<2>().0.iter().zip(output.as_chunks_mut::<8>().0.iter_mut()) {
                o.copy_from_slice(&[p[0], p[1], p[0], p[1], p[0], p[1], 255, 255]);
            }
        }
        (SampleDepth::U16, SourceChannels::GrayAlpha) => {
            for (p, o) in input.as_chunks::<4>().0.iter().zip(output.as_chunks_mut::<8>().0.iter_mut()) {
                o.copy_from_slice(&[p[0], p[1], p[0], p[1], p[0], p[1], p[2], p[3]]);
            }
        }
        (SampleDepth::F32, SourceChannels::Rgb) => {
            for (p, o) in input.as_chunks::<12>().0.iter().zip(output.as_chunks_mut::<16>().0.iter_mut()) {
                o[..12].copy_from_slice(p);
                o[12..].copy_from_slice(&1f32.to_le_bytes());
            }
        }
        (SampleDepth::F16, SourceChannels::Rgb) => {
            for (p,o) in input.as_chunks::<6>().0.iter().zip(output.as_chunks_mut::<8>().0.iter_mut()) { o[..6].copy_from_slice(p); o[6..].copy_from_slice(&0x3c00u16.to_le_bytes()); }
        }
        (SampleDepth::F16 | SampleDepth::F32, SourceChannels::GrayAlpha) => {
            let step = depth.bytes();
            for (p, o) in input.chunks_exact(2 * step).zip(output.chunks_exact_mut(4 * step)) {
                for channel in o[..3 * step].chunks_exact_mut(step) { channel.copy_from_slice(&p[..step]); }
                o[3 * step..].copy_from_slice(&p[step..]);
            }
        }
        (SampleDepth::F16 | SampleDepth::F32, SourceChannels::Gray) => unreachable!("invalid HDR gray source"),
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
        let layout = crate::bindings::layout(device, "integer source samples", &[
            crate::bindings::texture_of(
                0,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::TextureSampleType::Uint,
                wgpu::TextureViewDimension::D2,
            ),
            crate::bindings::buffer(
                1,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::BufferBindingType::Storage { read_only: true },
                false,
                wgpu::BufferSize::new(transfer::TABLE_BYTES),
            ),
        ]);
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
