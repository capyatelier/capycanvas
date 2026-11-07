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
pub(crate) type RequiredSource = (Arc<SourceImage>, [u32; 2]);
#[derive(Clone,Copy,PartialEq,Eq)]
pub(crate) enum SourceUse { Display, Exact }
pub(crate) const NORMALIZED_PAGE_BYTES: u64 = PAGE_SIZE as u64 * PAGE_SIZE as u64 * 16;
const DECODERS: usize = 4;
use crate::native_tiles::transfer;

#[derive(Clone, Copy, Default)]
struct SourceLimits {
    bytes: u64,
    upload_bytes: u64,
}
impl SourceLimits {
    fn admitted(decoded_bytes: u64, upload_bytes: u64) -> Self {
        Self { bytes: decoded_bytes, upload_bytes }
    }
}

#[derive(PartialEq, Eq)]
enum RasterIdentity {
    Encoded([u8; 32]),
    Owner(u64),
}
impl RasterIdentity {
    fn of(blob: &TileBlob) -> Self {
        blob.encoded_fingerprint().map_or_else(|| Self::Owner(blob.owner_identity()), Self::Encoded)
    }
}
pub(crate) fn same_raster_samples(a: &TileBlob, b: &TileBlob) -> bool { RasterIdentity::of(a) == RasterIdentity::of(b) }
enum Key {
    Image(Weak<SourceImage>, [u32; 2]),
    PaintBase(Weak<SourceImage>, [u32; 2], [u32; 2]),
    Raster(RasterIdentity, RgbSpace, RgbSpace, PixelDescriptor),
}
impl Key {
    fn raster(blob: &TileBlob, space: RgbSpace, destination: RgbSpace) -> Self {
        Self::Raster(RasterIdentity::of(blob), space, destination, blob.descriptor)
    }
    fn matches(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Image(a, x), Self::Image(b, y)) => a.ptr_eq(b) && x == y,
            (Self::PaintBase(a, x, p), Self::PaintBase(b, y, q)) => a.ptr_eq(b) && x == y && p == q,
            (Self::Raster(a, x, d, p), Self::Raster(b, y, e, q)) => a == b && x == y && d == e && p == q,
            _ => false,
        }
    }
    fn samples(&self, destination: RgbSpace) -> Option<Samples> {
        match self {
            Self::Image(source, coordinate) => {
                let source = source.upgrade()?;
                let ColorProfile::Builtin(space) = source.interpretation.profile else { return None; };
                if source.interpretation.channels == SourceChannels::Cmyk || source.interpretation.depth.is_float() { return None; }
                Some(Samples { data: builtin_settings(&source, *coordinate, destination)?, space })
            }
            Self::Raster(_, space, destination, descriptor) if descriptor.channels > 1 && !descriptor.depth().is_float() => {
                Some(Samples { data: rgb_settings(*space, *destination, descriptor.depth(), [PAGE_SIZE; 2], descriptor.alpha), space: *space })
            }
            _ => None,
        }
    }
}
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Representation { Unpacked, Unorm8, Packed16, Srgb8 }
impl Representation {
    fn for_format(format:wgpu::TextureFormat)->Self {
        match format {wgpu::TextureFormat::Rgba8Unorm=>Self::Unorm8,wgpu::TextureFormat::Rg32Uint=>Self::Packed16,_=>Self::Unpacked}
    }
    fn format(self)->wgpu::TextureFormat {
        match self {Self::Unorm8|Self::Srgb8=>wgpu::TextureFormat::Rgba8Unorm,Self::Packed16=>wgpu::TextureFormat::Rg32Uint,Self::Unpacked=>wgpu::TextureFormat::Rgba32Float}
    }
}
#[repr(u32)]
#[derive(Clone, Copy)]
enum TransferCurve { Srgb=0, AdobeRgb=2, ProPhoto=3 }
impl From<RgbSpace> for TransferCurve {
    fn from(space:RgbSpace)->Self {match space {RgbSpace::Srgb|RgbSpace::DisplayP3=>Self::Srgb,RgbSpace::AdobeRgb=>Self::AdobeRgb,RgbSpace::ProPhoto=>Self::ProPhoto}}
}
#[repr(C,align(16))]
#[derive(Clone, Copy)]
pub(crate) struct SourceColor {
    red:[f32;4],green:[f32;4],blue:[f32;4],
    premultiplied:u32,integer_max:f32,extent:[f32;2],
    float_bits:u32,identity:u32,scalar:u32,scalar_scale:f32,
    representation:Representation,display:u32,transfer:TransferCurve,padding:u32,
}
impl SourceColor {
    pub(crate) fn write_uniform(self,bytes:&mut [u8]) {
        let mut words=[0;24];
        for (output,value) in words[..12].iter_mut().zip(self.red.into_iter().chain(self.green).chain(self.blue)) {*output=value.to_bits();}
        words[12..].copy_from_slice(&[self.premultiplied,self.integer_max.to_bits(),self.extent[0].to_bits(),self.extent[1].to_bits(),
            self.float_bits,self.identity,self.scalar,self.scalar_scale.to_bits(),self.representation as u32,self.display,self.transfer as u32,self.padding]);
        for (output,word) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(words) {*output=word.to_ne_bytes();}
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Samples { data:SourceColor,space:RgbSpace }
impl Samples {
    pub(crate) fn for_source(source:&SourceImage,coordinate:[u32;2],destination:RgbSpace)->Option<Self> {
        let ColorProfile::Builtin(space)=source.interpretation.profile else {return None;};
        if source.interpretation.depth.is_float() {return None;}
        Some(Self {data:builtin_settings(source,coordinate,destination)?,space})
    }
    pub(crate) fn format(self)->wgpu::TextureFormat {self.data.representation.format()}
    fn parameters(mut self,format:wgpu::TextureFormat)->SourceColor {
        self.data.representation=Representation::for_format(format);
        self.data
    }
    pub(crate) fn display_parameters(mut self)->SourceColor {
        self.data.display=1;
        if self.srgb_view() {self.data.representation=Representation::Srgb8;}
        self.data
    }
    pub(crate) fn srgb_view(self)->bool {self.data.representation==Representation::Unorm8 && matches!(self.space,RgbSpace::Srgb|RgbSpace::DisplayP3)}
}

#[derive(Clone)]
pub(crate) struct Lease { owner:Arc<()>,_encoded:Option<Arc<()>> }
impl std::ops::Deref for Lease {type Target=Arc<()>;fn deref(&self)->&Self::Target {&self.owner}}
impl From<Arc<()>> for Lease {fn from(owner:Arc<()>)->Self {Self {owner,_encoded:None}}}
struct Slot {
    key: Option<Key>,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    used: u64,
    created: u64,
    valid: Arc<std::sync::atomic::AtomicBool>,
    lease: Arc<()>,
    encoded:Option<Arc<()>>,
    samples: Option<Samples>,
}
struct RetiredSlot { slot: Slot, retired: Arc<AtomicU64>, bytes: u64, completed: Arc<std::sync::atomic::AtomicBool> }
impl Drop for RetiredSlot {
    fn drop(&mut self) { self.retired.fetch_sub(self.bytes, Ordering::Release); }
}
struct RetirementCompletion(Arc<std::sync::atomic::AtomicBool>);
impl Drop for RetirementCompletion {
    fn drop(&mut self) { self.0.store(true, Ordering::Release); }
}
enum Pixels {
    Image(Arc<SourceImage>, [u32; 2]),
    Raster(Arc<TileBlob>, RgbSpace,RgbSpace),
    Resident,
}
#[derive(Clone)]
pub enum PreparedSourcePixels { NativeSamples(Arc<Vec<u8>>), PremultipliedWorkingPixels(Arc<Vec<u8>>) }

pub(crate) struct PendingTile {
    pixels: Pixels,
    pub texture: wgpu::Texture,
    pub(super) view: wgpu::TextureView,
    pub data: Option<SourceColor>,
    write: crate::submission::CacheWrite,
    pub prepared_pixels: Option<PreparedSourcePixels>,
    receipt: std::sync::Mutex<Option<PreparedSourcePixels>>,
    input: Option<(wgpu::Texture, RgbSpace, std::sync::Mutex<Option<RetirementCompletion>>)>,
}
impl PendingTile {
    pub fn prepared(&self)->Option<PreparedSourcePixels> {self.prepared_pixels.clone().or_else(||self.receipt.lock().unwrap().clone())}
    pub fn ready(&self) -> bool { self.input.is_some() || self.prepared().is_some() }
    fn input_format(&self)->Option<wgpu::TextureFormat> {
        let color=self.data?;
        if self.texture.format()!=wgpu::TextureFormat::Rgba32Float {return None;}
        Some(match color.representation {
            Representation::Unorm8 if color.scalar==0=>wgpu::TextureFormat::Rgba8Unorm,
            Representation::Unorm8=>wgpu::TextureFormat::Rgba8Uint,
            Representation::Packed16 if color.scalar==0=>wgpu::TextureFormat::Rg32Uint,
            Representation::Packed16=>wgpu::TextureFormat::Rgba16Uint,
            _ if color.float_bits!=0=>wgpu::TextureFormat::Rgba32Uint,
            _=>wgpu::TextureFormat::Rgba16Uint,
        })
    }
    pub(crate) fn normalization_parameters(&self)->Option<SourceColor> {
        self.data.map(|mut color| {
            if self.input.is_none() && let Some(format)=self.input_format() {color.representation=Representation::for_format(format);}
            color
        })
    }
    pub(crate) fn pipeline<'a>(&self, pipelines: &'a Pipelines) -> Option<&'a Deferred<wgpu::RenderPipeline>> {
        if let Some((input, ..)) = &self.input { Some(pipelines.for_format(input.format())) }
        else if let Some(format)=self.input_format() {Some(pipelines.for_format(format))}
        else { None }
    }
    pub fn matches_image(&self,source:&Arc<SourceImage>,coordinate:[u32;2])->bool {matches!(&self.pixels,Pixels::Image(stored,c) if Arc::ptr_eq(stored,source) && *c==coordinate)}
    pub fn matches_raster(&self,blob:&Arc<TileBlob>,space:RgbSpace,destination:RgbSpace)->bool {matches!(&self.pixels,Pixels::Raster(stored,from,to) if Arc::ptr_eq(stored,blob) && *from==space && *to==destination)}
    pub fn accept_image(&self,source:&Arc<SourceImage>,coordinate:[u32;2],pixels:&PreparedSourcePixels)->bool {
        if !self.matches_image(source,coordinate) {return false;}
        *self.receipt.lock().unwrap()=Some(pixels.clone());true
    }
    pub fn accept_raster(&self,blob:&Arc<TileBlob>,space:RgbSpace,destination:RgbSpace,pixels:&PreparedSourcePixels)->bool {
        if !self.matches_raster(blob,space,destination) {return false;}
        *self.receipt.lock().unwrap()=Some(pixels.clone());true
    }
    pub fn prepare(&self,r:&mut WgpuRasterizer)->Result<(),GpuRasterError> {
        if self.input.is_some() { return Ok(()); }
        if self.prepared().is_some() {return Ok(());}
        match &self.pixels {
            Pixels::Image(source,coordinate)=> {
                let id=source.tiles.get(coordinate).ok_or(GpuRasterError::InvalidImage)?.resource_id();
                if self.texture.format()==wgpu::TextureFormat::Rgba32Float {r.request_exact_image_decode(id,source.clone(),*coordinate)?;}
                else {r.request_image_decode(id,source.clone(),*coordinate)?;}
            },
            Pixels::Raster(blob,space,destination)=> {r.request_raster_decode_to(blob.clone(),*space,*destination)?;},
            Pixels::Resident => unreachable!(),
        }
        Ok(())
    }
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
            Self::Raster(tile, space,_) => Ok(NativeSamples {
                tile,
                descriptor: tile.descriptor,
                channels: match tile.descriptor.channels { 1 => SourceChannels::Gray, 2 => SourceChannels::GrayAlpha, _ => SourceChannels::Rgba },
                depth: tile.descriptor.depth(),
                space: *space,
            }),
            Self::Resident => unreachable!(),
        }
    }
}
struct EncodedInput {
    texture: wgpu::Texture,
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
    inputs: Vec<EncodedInput>,
    transfer: transfer::Tables,
    in_flight: Arc<InFlight>,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    mip_allowance: u64,
    required: Vec<Key>,
    retired_bytes: Arc<AtomicU64>,
    retired_slots: std::cell::RefCell<Vec<RetiredSlot>>,
    retiring: Vec<RetirementCompletion>,
    unit: Option<u64>,
    overflowed: bool,
}
impl DecodedTiles {
    pub fn new(destination: RgbSpace) -> Self {
        Self {
            destination,
            ..Self::default()
        }
    }
    pub fn admit_limits(&mut self, decoded_bytes: u64, upload_bytes: u64, mip_bytes: u64) {
        self.reap_retired();
        self.limits = SourceLimits::admitted(decoded_bytes, upload_bytes);
        self.mip_allowance = mip_bytes;
    }
    pub fn admitted_bytes(&self) -> [u64; 2] {
        [self.limits.bytes.saturating_sub(self.non_slot_bytes()), self.limits.upload_bytes]
    }
    pub fn mip_budget(&self) -> u64 { self.mip_allowance }
    pub(crate) fn require_images(&mut self, tiles: impl IntoIterator<Item = RequiredSource>) {
        self.required.clear();
        for (source, coordinate) in tiles {
            if self.required.len() >= self.slot_limit().saturating_sub(1) { break; }
            let key = Key::Image(Arc::downgrade(&source), coordinate);
            if !self.required.iter().any(|stored| stored.matches(&key)) { self.required.push(key); }
        }
    }
    fn protected_sources(&self, reserved:u64) -> usize {
        self.reap_retired();
        let leased=self.slots.iter().filter(|slot|Arc::strong_count(&slot.lease)>1 && !self.required(slot,self.required.len()))
            .map(|slot|texture_bytes(&slot.texture)).sum::<u64>();
        (self.limits.bytes.saturating_sub(self.non_slot_bytes()).saturating_sub(self.retired_bytes.load(Ordering::Acquire))
            .saturating_sub(leased).saturating_sub(reserved)/NORMALIZED_PAGE_BYTES) as usize
    }
    fn required(&self, slot: &Slot, count: usize) -> bool {
        slot.key.as_ref().is_some_and(|key| self.required.iter().take(count).any(|required| required.matches(key)))
    }
    pub fn can_upload_bytes(&self, bytes: u64) -> bool {
        self.in_flight.bytes.load(Ordering::Acquire).checked_add(bytes).is_some_and(|n| n <= self.limits.upload_bytes)
    }
    fn slot_limit(&self) -> usize {
        self.reap_retired();
        (self.limits.bytes.saturating_sub(self.non_slot_bytes()).saturating_sub(self.retired_bytes.load(Ordering::Acquire)) / NORMALIZED_PAGE_BYTES) as usize
    }
    pub fn reserve_mips(&mut self, bytes: u64, _encoder: &crate::submission::CommandEncoder) -> Result<bool, GpuRasterError> {
        Ok(bytes <= self.mip_allowance)
    }
    pub fn trim_unused(&mut self, encoder: &crate::submission::CommandEncoder) -> u64 {
        self.reap_retired();
        let protected = self.protected_sources(NORMALIZED_PAGE_BYTES);
        let mut released = 0;
        for _ in 0..SOURCE_SLOTS {
            if self.slots.iter().map(|slot|texture_bytes(&slot.texture)).sum::<u64>().saturating_add(self.non_slot_bytes()) <= self.limits.bytes { break; }
            let Some(index) = self.slots.iter().enumerate().filter(|(_,slot)| Arc::strong_count(&slot.lease)==1 && !self.required(slot, protected))
                .min_by_key(|(_,slot)|slot.used).map(|(index,_)|index) else { break; };
            let slot = self.slots.swap_remove(index);
            if slot.valid.load(Ordering::Acquire) { self.evictions += 1; }
            let bytes = texture_bytes(&slot.texture);
            let completion = self.retire(slot);
            encoder.on_submitted_work_done(move || drop(completion));
            released += bytes;
        }
        released
    }
    fn reap_retired(&self) {
        self.retired_slots.borrow_mut().retain(|slot| !slot.completed.load(Ordering::Acquire)
            || slot.slot.encoded.as_ref().is_some_and(|encoded|Arc::strong_count(encoded)>1));
    }
    fn retire(&mut self, slot: Slot) -> RetirementCompletion {
        let bytes = texture_bytes(&slot.texture);
        let retired = self.retired_bytes.clone();
        retired.fetch_add(bytes, Ordering::Relaxed);
        let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.retired_slots.get_mut().push(RetiredSlot { slot, retired, bytes, completed: completed.clone() });
        RetirementCompletion(completed)
    }
    pub(crate) fn flush_retired(&mut self, encoder: &mut crate::submission::CommandEncoder) {
        if self.retiring.is_empty() { return; }
        encoder.retain_resources();
        for completion in self.retiring.drain(..) { encoder.on_submitted_work_done(move || drop(completion)); }
    }
    pub fn prepare_transfer(
        &mut self,
        device: &PipelineDevice,
        space: RgbSpace,
    ) -> Result<crate::native_tiles::NativeTransfer, GpuRasterError> {
        self.transfer.prepare(device, space).cloned()
    }
    pub(crate) fn invalidate(&mut self) {for slot in &self.slots {slot.valid.store(false,Ordering::Release);}}
    pub(crate) fn free_slots(&self) -> usize {
        self.reap_retired();
        let leased=self.slots.iter().filter(|slot|Arc::strong_count(&slot.lease)>1).map(|slot|texture_bytes(&slot.texture)).sum::<u64>();
        (self.limits.bytes.saturating_sub(self.non_slot_bytes()).saturating_sub(self.retired_bytes.load(Ordering::Acquire))
            .saturating_sub(leased)/NORMALIZED_PAGE_BYTES) as usize
    }
    pub(crate) fn begin_unit(&mut self) { self.unit = Some(self.clock); self.overflowed = false; }
    pub(crate) fn end_unit(&mut self) -> bool { self.unit = None; std::mem::take(&mut self.overflowed) }
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
            return self.prepared_view(base.image.storage(), std::array::from_fn(|i| coordinate[i] - base.offset[i] / PAGE_SIZE))
                .filter(|view| view.texture().format() == wgpu::TextureFormat::Rgba32Float);
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
        !self.can_upload_bytes(NORMALIZED_PAGE_BYTES)
    }
    pub fn prepared_raster_view(&self, blob: &Arc<TileBlob>, space: RgbSpace,destination:RgbSpace) -> Option<&wgpu::TextureView> {
        self.prepared_raster_any_view(blob,space,destination).filter(|view|view.texture().format()==wgpu::TextureFormat::Rgba32Float)
    }
    pub(crate) fn prepared_raster_any_view(&self, blob: &Arc<TileBlob>, space: RgbSpace,destination:RgbSpace) -> Option<&wgpu::TextureView> {
        let key = Key::raster(blob, space, destination);
        self.slots.iter().find(|s| s.key.as_ref().is_some_and(|k| k.matches(&key)) && s.valid.load(Ordering::Acquire))
            .map(|s| &s.view)
    }

    pub fn charge_upload(&self, encoder: &crate::submission::CommandEncoder, bytes: u64) -> u64 {
        let total = self.in_flight.bytes.fetch_add(bytes, Ordering::Relaxed) + bytes;
        let charge = UploadCharge(self.in_flight.clone(), bytes);
        encoder.on_submitted_work_done(move || drop(charge));
        total
    }
    pub(crate) fn non_slot_bytes(&self) -> u64 {
        self.transfer.gpu_bytes()
            + self
                .inputs
                .iter()
                .map(|input| {
                    PAGE_SIZE as u64
                        * PAGE_SIZE as u64
                        * input.texture.format().block_copy_size(None).unwrap() as u64
                })
                .sum::<u64>()
    }
    pub fn gpu_bytes(&self) -> u64 {
        self.reap_retired();
        self.slots.iter().map(|slot| texture_bytes(&slot.texture)).sum::<u64>()
            + self.retired_bytes.load(Ordering::Acquire)
            + self.non_slot_bytes()
    }
    pub fn cpu_bytes(&self) -> u64 {
        self.pixels.capacity() as u64 * std::mem::size_of::<[f32; 4]>() as u64
            + self.slots.capacity() as u64 * std::mem::size_of::<Slot>() as u64
            + self.inputs.capacity() as u64 * std::mem::size_of::<EncodedInput>() as u64
            + self.required.capacity() as u64 * std::mem::size_of::<Key>() as u64
            + self.decoders.capacity() as u64 * std::mem::size_of::<(Weak<SourceImage>, layer_color::WorkingDecoder)>() as u64
            + self.retired_slots.borrow().capacity() as u64 * std::mem::size_of::<RetiredSlot>() as u64
    }
    pub fn pinned_bytes(&self) -> u64 {
        let protected = self.protected_sources(NORMALIZED_PAGE_BYTES);
        self.slots.iter().filter(|slot| Arc::strong_count(&slot.lease) > 1 || self.required(slot, protected))
            .map(|slot| texture_bytes(&slot.texture)).sum::<u64>()
            + self.retired_bytes.load(Ordering::Acquire)
    }

    pub(crate) fn plan(
        &mut self,
        r: &WgpuRasterizer,
        source: &Arc<SourceImage>,
        coordinate: [u32; 2],
    ) -> Result<(RawTile, Option<PendingTile>), GpuRasterError> {
        self.plan_image(r,source,coordinate,SourceUse::Display)
    }
    pub(crate) fn plan_exact(&mut self,r:&WgpuRasterizer,source:&Arc<SourceImage>,coordinate:[u32;2])->Result<(RawTile,Option<PendingTile>),GpuRasterError> {
        self.plan_image(r,source,coordinate,SourceUse::Exact)
    }
    fn plan_image(&mut self,r:&WgpuRasterizer,source:&Arc<SourceImage>,coordinate:[u32;2],use_:SourceUse)->Result<(RawTile,Option<PendingTile>),GpuRasterError> {
        let (tile,write)=self.plan_key_for(r,Key::Image(Arc::downgrade(source),coordinate),use_)?;
        let pending = write.map(|write| PendingTile {
            pixels: Pixels::Image(source.clone(), coordinate),
            texture: tile.texture.clone(),
            view: tile.view.clone(),
            data: builtin_settings(source, coordinate, self.destination),
            write,
            prepared_pixels: None,receipt:Default::default(),input:None,
        });
        Ok((tile, pending))
    }

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
        data.scalar = u32::from(d.channels == 1);
        data.scalar_scale = if d.channels == 1 { 1. / depth.maximum() as f32 } else { 0. };
        let (tile, write) =
            self.plan_key_for(r,Key::raster(blob,space,destination),SourceUse::Exact)?;
        let pending = write.map(|write| PendingTile {
            pixels: Pixels::Raster(blob.clone(), space,destination),
            texture: tile.texture.clone(),
            view: tile.view.clone(),
            data: Some(data),
            write,
            prepared_pixels: None,receipt:Default::default(),input:None,
        });
        Ok((tile, pending))
    }

    pub fn batch_checkpoint(&self) -> u64 { self.clock }
    pub fn release_batch(&mut self, checkpoint:u64) {
        for slot in self.slots.iter_mut().filter(|slot|slot.created>checkpoint) {
            slot.key=None;
            slot.valid=Arc::new(std::sync::atomic::AtomicBool::new(false));
            slot.created=0;
            slot.used=0;
        }
    }
    pub fn lease(&self, view: &wgpu::TextureView) -> Option<Lease> {
        self.slots.iter().find(|s|s.view==*view).map(|s|Lease {owner:s.lease.clone(),_encoded:s.encoded.clone()})
    }

    fn make_room(&mut self, bytes: u64, except: Option<&wgpu::TextureView>) -> Result<(), GpuRasterError> {
        self.reap_retired();
        if bytes == 0 { return Ok(()); }
        if self.gpu_bytes().saturating_add(bytes) <= self.limits.bytes { return Ok(()); }
        if let Some(start)=self.unit {
            let current=self.slots.iter().filter(|slot|slot.used>start).map(|slot|texture_bytes(&slot.texture)).sum::<u64>()
                .saturating_add(self.retired_slots.borrow().iter().filter(|retired|retired.slot.used>start).map(|retired|retired.bytes).sum::<u64>());
            self.overflowed|=current.saturating_add(self.non_slot_bytes()).saturating_add(bytes)>self.limits.bytes;
        }
        let input=except.and_then(|view|self.slots.iter().find(|slot|slot.view==*view)).map_or(0,|slot|texture_bytes(&slot.texture));
        let protected = self.protected_sources(bytes.saturating_add(input));
        while self.slots.iter().map(|slot| texture_bytes(&slot.texture)).sum::<u64>()
            .saturating_add(self.non_slot_bytes()).saturating_add(bytes) > self.limits.bytes {
            let Some(index) = self.slots.iter().enumerate()
                .filter(|(_, slot)| Arc::strong_count(&slot.lease) == 1 && except != Some(&slot.view) && !self.required(slot, protected))
                .min_by_key(|(_, slot)| slot.used).map(|(index, _)| index) else { break; };
            if self.unit.is_some_and(|start|self.slots[index].used>start) {self.overflowed=true;break;}
            let slot = self.slots.remove(index);
            self.evictions += u64::from(slot.valid.load(Ordering::Acquire));
            let completion = self.retire(slot);
            self.retiring.push(completion);
        }
        Err(if self.unit.is_some() && self.overflowed || !self.retiring.is_empty() || self.retired_bytes.load(Ordering::Acquire) != 0 {
            GpuRasterError::DeferredObjectWork
        } else { GpuRasterError::SourceWorkingSetExceeded })
    }
    fn texture(r: &WgpuRasterizer, format: wgpu::TextureFormat) -> wgpu::Texture {
        r.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bounded source tile"), size: wgpu::Extent3d { width: PAGE_SIZE, height: PAGE_SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC
                | if format == wgpu::TextureFormat::Rgba32Float { wgpu::TextureUsages::RENDER_ATTACHMENT } else { wgpu::TextureUsages::empty() },
            view_formats: if format == wgpu::TextureFormat::Rgba8Unorm { &[wgpu::TextureFormat::Rgba8UnormSrgb] } else { &[] },
        })
    }
    fn allocate_slot(&mut self, r: &WgpuRasterizer, format: wgpu::TextureFormat, samples: Option<Samples>) -> usize {
        let texture = Self::texture(r, format);
        let view = texture.create_view(&Default::default());
        self.slots.push(Slot { key: None, texture, view, used: 0, created: 0,
            valid: Arc::new(std::sync::atomic::AtomicBool::new(false)), lease: Arc::new(()),encoded:(format!=wgpu::TextureFormat::Rgba32Float).then(||Arc::new(())), samples });
        self.slots.len() - 1
    }
    fn plan_key(&mut self, r: &WgpuRasterizer, key: Key) -> Result<(RawTile, Option<crate::submission::CacheWrite>), GpuRasterError> {
        self.plan_key_for(r,key,SourceUse::Display)
    }
    fn plan_key_for(&mut self,r:&WgpuRasterizer,key:Key,use_:SourceUse)->Result<(RawTile,Option<crate::submission::CacheWrite>),GpuRasterError> {
        self.clock = self.clock.wrapping_add(1);
        if let Some(slot) = self.slots.iter_mut().find(|slot| slot.key.as_ref().is_some_and(|stored| stored.matches(&key))
            && slot.valid.load(Ordering::Acquire)) {
            slot.used = self.clock; self.hits += 1;
            return Ok((RawTile { texture: slot.texture.clone(), view: slot.view.clone() }, None));
        }
        for slot in self.slots.iter_mut().filter(|slot|!slot.valid.load(Ordering::Acquire)
            && slot.key.as_ref().is_some_and(|stored|stored.matches(&key))) {slot.key=None;}
        let mut samples=key.samples(self.destination);
        let format=if use_==SourceUse::Exact {samples=None;wgpu::TextureFormat::Rgba32Float}else{samples.map_or(wgpu::TextureFormat::Rgba32Float,Samples::format)};
        let bytes = u64::from(PAGE_SIZE).pow(2) * u64::from(format.block_copy_size(None).unwrap());
        let protected = self.protected_sources(NORMALIZED_PAGE_BYTES);
        let unused = self.slots.iter().position(|slot| Arc::strong_count(&slot.lease) == 1 && !slot.valid.load(Ordering::Acquire)
            && slot.texture.format() == format && (!self.required(slot, protected) || slot.key.as_ref().is_some_and(|stored| stored.matches(&key))));
        let index = if let Some(index) = unused { index } else if self.gpu_bytes().saturating_add(bytes) <= self.limits.bytes {
            self.allocate_slot(r, format, samples)
        } else {
            let Some(index) = self.slots.iter().enumerate().filter(|(_, slot)| Arc::strong_count(&slot.lease) == 1 && !self.required(slot, protected))
                .min_by_key(|(_, slot)| slot.used).map(|(index, _)| index) else {
                self.overflowed|=self.unit.is_some();
                return Err(if self.unit.is_some() {GpuRasterError::DeferredObjectWork}else{GpuRasterError::SourceWorkingSetExceeded});
            };
            if self.unit.is_some_and(|start|self.slots[index].used>start) {self.overflowed=true;return Err(GpuRasterError::DeferredObjectWork);}
            if self.slots[index].texture.format() != format {
                self.make_room(bytes, None)?;
                self.allocate_slot(r, format, samples)
            } else {
                self.overflowed |= self.unit.is_some_and(|start| self.slots[index].used > start);
                index
            }
        };
        let slot = &mut self.slots[index];
        self.evictions += u64::from(slot.valid.load(Ordering::Acquire));
        slot.key = Some(key); slot.samples = samples; slot.used = self.clock; slot.created = self.clock;
        let write = crate::submission::CacheWrite::new(); slot.valid = write.validity();
        Ok((RawTile { texture: slot.texture.clone(), view: slot.view.clone() }, Some(write)))
    }
    pub(crate) fn promote(&mut self, r: &WgpuRasterizer, tile: RawTile) -> Result<(RawTile, Option<PendingTile>), GpuRasterError> {
        if tile.texture.format() == wgpu::TextureFormat::Rgba32Float { return Ok((tile, None)); }
        let index = self.slots.iter().position(|slot| slot.view == tile.view).ok_or(GpuRasterError::InvalidImage)?;
        let samples = self.slots[index].samples.ok_or(GpuRasterError::InvalidImage)?;
        let table_bytes = if self.transfer.contains(samples.space) { 0 } else { transfer::TABLE_BYTES };
        let protected=self.protected_sources(NORMALIZED_PAGE_BYTES.saturating_add(texture_bytes(&tile.texture)));
        let reusable=(table_bytes==0 && self.gpu_bytes().saturating_add(NORMALIZED_PAGE_BYTES)>self.limits.bytes).then(||
            self.slots.iter().enumerate().filter(|(_,slot)|slot.texture.format()==wgpu::TextureFormat::Rgba32Float
                && Arc::strong_count(&slot.lease)==1 && !self.required(slot,protected))
                .min_by_key(|(_,slot)|slot.used).map(|(index,_)|index)).flatten();
        let (texture,view)=if let Some(index)=reusable {
            if self.unit.is_some_and(|start|self.slots[index].used>start) {self.overflowed=true;return Err(GpuRasterError::DeferredObjectWork);}
            let slot=self.slots.remove(index);
            self.evictions+=u64::from(slot.valid.load(Ordering::Acquire));
            (slot.texture,slot.view)
        } else {
            self.make_room(NORMALIZED_PAGE_BYTES + table_bytes, Some(&tile.view))?;
            let texture=Self::texture(r,wgpu::TextureFormat::Rgba32Float);
            let view=texture.create_view(&Default::default());
            (texture,view)
        };
        self.transfer.prepare(&r.device,samples.space)?;
        let index=self.slots.iter().position(|slot|slot.view==tile.view).ok_or(GpuRasterError::InvalidImage)?;
        let write = crate::submission::CacheWrite::new();
        let slot = &mut self.slots[index];
        let replacement = Slot { key: slot.key.take(), texture: texture.clone(), view: view.clone(), used: slot.used,
            created: slot.created, valid: write.validity(), lease: slot.lease.clone(),encoded:None, samples: None };
        let old = std::mem::replace(slot, replacement);
        let completion = self.retire(old);
        let data = samples.parameters(tile.texture.format());
        Ok((RawTile { texture: texture.clone(), view: view.clone() }, Some(PendingTile {
            pixels: Pixels::Resident, texture, view, data: Some(data), write, prepared_pixels: None, receipt: Default::default(),
            input: Some((tile.texture, samples.space, std::sync::Mutex::new(Some(completion)))),
        })))
    }
    pub(crate) fn display(&mut self, r: &WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        samples:Samples,input:&wgpu::Texture,output:&wgpu::TextureView,uniforms:&wgpu::BindGroup,offset:u32,
    ) -> Result<(), GpuRasterError> {
        let pipelines = &r.scene_pipelines.source;
        if !pipelines.require(r, pipelines.for_format(input.format())) { return Err(GpuRasterError::DeferredObjectWork); }
        let admission=self.make_room(if self.transfer.contains(samples.space) {0}else{transfer::TABLE_BYTES},
            self.slots.iter().find(|slot|slot.texture==*input).map(|slot|slot.view.clone()).as_ref());
        self.flush_retired(encoder);
        admission?;
        let srgb=samples.srgb_view();
        let transfer = self.transfer.prepare(&r.device, samples.space)?;
        let view = input.create_view(&wgpu::TextureViewDescriptor { format: srgb.then_some(wgpu::TextureFormat::Rgba8UnormSrgb), ..Default::default() });
        pipelines.normalize(&r.device,encoder,&view,output,transfer,uniforms,offset);
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn encode(
        &mut self,
        device: &crate::PipelineDevice,
        uploads: &mut crate::Uploads,
        pipelines: &Pipelines,
        encoder: &mut crate::submission::CommandEncoder,
        pending: &PendingTile,
        snapshot_worker:bool,
        uniforms: Option<&wgpu::BindGroup>,
        offset: u32,
    ) -> Result<u64, GpuRasterError> {
        if let Some((input, space, retirement)) = &pending.input {
            let transfer = self.transfer.prepare(device, *space)?.clone();
            pipelines.normalize(device, encoder, &input.create_view(&Default::default()), &pending.view, &transfer,
                uniforms.ok_or(GpuRasterError::InvalidExtent)?, offset);
            pending.write.track(encoder);
            if let Some(completion) = retirement.lock().unwrap().take() { encoder.on_submitted_work_done(move || drop(completion)); }
            return Ok(0);
        }
        let prepared_pixels=pending.prepared();
        if !snapshot_worker && prepared_pixels.is_none() {return Err(GpuRasterError::DeferredObjectWork);}
        let bytes = if pending.data.is_some() {
            let samples = pending.pixels.native()?;
            let target = if pending.texture.format() == wgpu::TextureFormat::Rgba32Float {
                let format=pending.input_format().ok_or(GpuRasterError::InvalidImage)?;
                let index=self.inputs.iter().position(|input|input.texture.format()==format);
                let extra=if index.is_none() {u64::from(PAGE_SIZE).pow(2)*u64::from(format.block_copy_size(None).unwrap())}else{0}
                    +if self.transfer.contains(samples.space) {0}else{transfer::TABLE_BYTES};
                let admission=self.make_room(extra,Some(&pending.view));
                self.flush_retired(encoder);admission?;
                let index=index.unwrap_or_else(|| {
                    self.inputs.push(EncodedInput {texture:device.create_texture(&wgpu::TextureDescriptor {
                        label:Some("bounded exact source input"),size:pending.texture.size(),mip_level_count:1,sample_count:1,
                        dimension:wgpu::TextureDimension::D2,format,usage:wgpu::TextureUsages::TEXTURE_BINDING|wgpu::TextureUsages::COPY_DST,view_formats:&[],
                    })});
                    self.inputs.len()-1
                });
                self.inputs[index].texture.clone()
            } else { pending.texture.clone() };
            if samples.tile.descriptor != samples.descriptor {
                return Err(GpuRasterError::Color("Source tile has the wrong sample representation".into()));
            }
            let decoded = match &prepared_pixels {
                Some(PreparedSourcePixels::NativeSamples(samples)) => samples.clone(),
                Some(PreparedSourcePixels::PremultipliedWorkingPixels(_)) => return Err(GpuRasterError::Color("Working source pixels have native decode settings".into())),
                None => {
                    #[cfg(not(target_arch="wasm32"))]
                    {device.source_samples.decode_on_worker(samples.tile).map_err(GpuRasterError::Color)?}
                    #[cfg(target_arch="wasm32")]
                    {
                    device.source_samples.poll_decode_on_worker(samples.tile).map_err(GpuRasterError::Color)?.ok_or(GpuRasterError::DeferredObjectWork)?
                    }
                },
            };
            let step = samples.depth.bytes();
            let prepared = prepared_pixels.is_some();
            let channels = if prepared { 4 } else { samples.channels.count() };
            if decoded.len() != (PAGE_SIZE * PAGE_SIZE) as usize * channels * step {
                return Err(GpuRasterError::Color("Prepared source samples have the wrong size".into()));
            }
            let bytes = (PAGE_SIZE * PAGE_SIZE) as usize * 4 * step;
            uploads.write_texture(encoder, &target, PAGE_SIZE * 4 * step as u32, |mapped| {
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
            if pending.texture.format() == wgpu::TextureFormat::Rgba32Float {
                let transfer = self.transfer.prepare(device, samples.space)?.clone();
                pipelines.normalize(device, encoder, &target.create_view(&Default::default()), &pending.view, &transfer,
                    uniforms.ok_or(GpuRasterError::InvalidExtent)?, offset);
            }
            bytes as u64
        } else {
            let Pixels::Image(source, coordinate) = &pending.pixels else {
                unreachable!()
            };
            match &prepared_pixels {
                Some(PreparedSourcePixels::PremultipliedWorkingPixels(bytes)) => {
                    if bytes.len() as u64 != NORMALIZED_PAGE_BYTES { return Err(GpuRasterError::InvalidExtent); }
                    uploads.write_texture(encoder, &pending.texture, PAGE_SIZE * 16, |mapped| mapped.copy_from_slice(bytes))?;
                }
                Some(PreparedSourcePixels::NativeSamples(_)) => return Err(GpuRasterError::Color("Native source samples require decode settings".into())),
                None => self.upload_icc(device, uploads, source, *coordinate, encoder, &pending.texture)?,
            }
            NORMALIZED_PAGE_BYTES
        };
        pending.write.track(encoder);
        self.misses += 1;
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

pub(crate) fn validate_raster(blob: &TileBlob, space: RgbSpace) -> Result<(), GpuRasterError> {
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
) -> Option<SourceColor> {
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
) -> SourceColor {
    let rows=space.linear_transform(destination).map(|row|[row[0] as f32,row[1] as f32,row[2] as f32,0.]);
    SourceColor {red:rows[0],green:rows[1],blue:rows[2],premultiplied:u32::from(alpha==AlphaAssociation::PremultipliedLinear),
        integer_max:if depth.is_float() {0.}else{depth.maximum() as f32},extent:extent.map(|side|side as f32),
        float_bits:u32::from(depth==SampleDepth::F32),identity:u32::from(space==destination),scalar:0,scalar_scale:0.,
        representation:match depth {SampleDepth::U8=>Representation::Unorm8,SampleDepth::U16=>Representation::Packed16,_=>Representation::Unpacked},
        display:0,transfer:space.into(),padding:0}

}

#[derive(Clone)]
pub(crate) struct Pipelines {
    layouts: [wgpu::BindGroupLayout; 2],
    pub pipeline: Deferred<wgpu::RenderPipeline>,
    pub integer: Deferred<wgpu::RenderPipeline>,
}
impl Pipelines {
    pub fn new(device: &PipelineDevice, uniforms: &wgpu::BindGroupLayout) -> Self {
        let layouts = [false, true].map(|integer| crate::bindings::layout(device, "source normalization", &[
            crate::bindings::texture_of(0, wgpu::ShaderStages::FRAGMENT,
                if integer { wgpu::TextureSampleType::Uint } else { wgpu::TextureSampleType::Float { filterable: false } }, wgpu::TextureViewDimension::D2),
            crate::bindings::buffer(1, wgpu::ShaderStages::FRAGMENT, wgpu::BufferBindingType::Storage { read_only: true }, false,
                wgpu::BufferSize::new(transfer::TABLE_BYTES)),
        ]));
        let [pipeline, integer] = std::array::from_fn(|index| {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("source normalization"), bind_group_layouts: &[Some(uniforms), Some(&layouts[index])], immediate_size: 0,
            });
            let input = if index == 0 { r#"
@group(1) @binding(0) var encoded: texture_2d<f32>;
fn native_samples(p: vec2<i32>) -> vec4<u32> { return vec4<u32>(round(clamp(textureLoad(encoded,p,0),vec4(0.),vec4(1.))*255.)); }
fn display_samples(p: vec2<i32>) -> vec4<f32> { return textureLoad(encoded,p,0); }
"# } else { r#"
@group(1) @binding(0) var encoded: texture_2d<u32>;
fn native_samples(p: vec2<i32>) -> vec4<u32> {
    let value=textureLoad(encoded,p,0);
    if settings.representation==SOURCE_PACKED16 { return vec4(value.r&65535u,value.r>>16u,value.g&65535u,value.g>>16u); }
    return value;
}
fn display_samples(p: vec2<i32>) -> vec4<f32> { let code=native_samples(p); return vec4(vec3<f32>(code.rgb)/settings.integer_max, select(f32(code.a)/settings.integer_max,1.,code.a==u32(settings.integer_max))); }
"# };
            let shader = Deferred::wgsl(device, "source normalization", format!("{}\n{}\n{}",
                include_str!("../native_tiles/validity.wgsl"), include_str!("source_color.wgsl"), include_str!("source_decode.wgsl").replace("SOURCE_INPUT", input)));
            let device = device.clone();
            Deferred::pipeline(move |mode| fullscreen_pipeline_recipe(mode, &device, &layout, &shader, "fragment_main", None,
                wgpu::TextureFormat::Rgba32Float, "normalize source tile"))
        });
        Self { layouts, pipeline, integer }
    }
    fn index(texture: &wgpu::Texture) -> usize {
        usize::from(Self::integer_format(texture.format()))
    }
    fn integer_format(format: wgpu::TextureFormat) -> bool { matches!(format, wgpu::TextureFormat::Rg32Uint
        | wgpu::TextureFormat::Rgba8Uint | wgpu::TextureFormat::Rgba16Uint | wgpu::TextureFormat::Rgba32Uint) }
    pub(crate) fn for_format(&self, format: wgpu::TextureFormat) -> &Deferred<wgpu::RenderPipeline> {
        if Self::integer_format(format) { &self.integer } else { &self.pipeline }
    }
    pub(crate) fn require(&self, r: &WgpuRasterizer, pipeline: &Deferred<wgpu::RenderPipeline>) -> bool {
        if pipeline.ready() { return true; }
        if let Some(compiler) = r.startup.as_ref().map(|startup| &startup.compiler).or(r.preparation_compiler.as_ref()) {
            let ready = compiler.require([pipeline], startup::VALIDATION); compiler.start(); ready
        } else {
            #[cfg(not(target_arch = "wasm32"))]
            { pipeline.compile(); true }
            #[cfg(target_arch = "wasm32")]
            { false }
        }
    }
    fn normalize(&self, device: &PipelineDevice, encoder: &mut crate::submission::CommandEncoder, input: &wgpu::TextureView,
        output: &wgpu::TextureView, transfer: &transfer::NativeTransfer, uniforms: &wgpu::BindGroup, offset: u32,
    ) {
        let index = Self::index(input.texture());
        let binding = crate::bindings::group(device, "source normalization input", &self.layouts[index], [
            wgpu::BindingResource::TextureView(input), transfer.as_entire_binding(),
        ]);
        let attachments = [Some(attachment(output, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)))];
        #[cfg(test)]
        crate::test_support::PassCounts {normalizations:1,..Default::default()}.record();
        let mut pass = encoder.begin_render_pass(&descriptor(&attachments));
        pass.set_pipeline(if index == 0 { &self.pipeline } else { &self.integer });
        pass.set_bind_group(0, uniforms, &[offset]); pass.set_bind_group(1, &binding, &[]); pass.draw(0..3,0..1);
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod grant_tests {
    use super::*;
    #[test]
    fn source_color_uniform_matches_the_shader_layout() {
        assert_eq!(std::mem::size_of::<SourceColor>(),96);
        assert_eq!(std::mem::align_of::<SourceColor>(),16);
        assert_eq!(std::mem::offset_of!(SourceColor,integer_max),52);
        assert_eq!(std::mem::offset_of!(SourceColor,float_bits),64);
        assert_eq!(std::mem::offset_of!(SourceColor,representation),80);
        assert_eq!(std::mem::offset_of!(SourceColor,transfer),88);
    }
    #[test]
    fn decoded_slots_round_down_without_manufacturing_a_working_grant() {
        for (bytes, slots) in [(0, 0), (NORMALIZED_PAGE_BYTES - 1, 0), (NORMALIZED_PAGE_BYTES, 1), (3 * NORMALIZED_PAGE_BYTES + 1, 3)] {
            let limits = SourceLimits::admitted(bytes, 0);
            assert_eq!(limits.bytes / NORMALIZED_PAGE_BYTES, slots);
            assert!(limits.bytes <= bytes);
            assert_eq!(limits.upload_bytes, 0);
        }
    }
    #[test]
    fn tiny_upload_grants_defer_a_float_tile_without_underflow() {
        let mut cache = DecodedTiles::default();
        cache.admit_limits(2 * NORMALIZED_PAGE_BYTES, NORMALIZED_PAGE_BYTES - 1, 0);
        assert!(cache.uploads_full());
        assert!(cache.can_upload_bytes(NORMALIZED_PAGE_BYTES / 4));
        assert!(!cache.can_upload_bytes(NORMALIZED_PAGE_BYTES));
        cache.admit_limits(2 * NORMALIZED_PAGE_BYTES, NORMALIZED_PAGE_BYTES, 0);
        assert!(!cache.uploads_full());
    }
    #[test]
    fn mip_grants_do_not_consume_decoded_tile_slots() {
        let mut cache = DecodedTiles::default();
        cache.admit_limits(3 * NORMALIZED_PAGE_BYTES, NORMALIZED_PAGE_BYTES, 100 * NORMALIZED_PAGE_BYTES);
        assert_eq!(cache.slot_limit(), 3);
        assert_eq!(cache.mip_budget(), 100 * NORMALIZED_PAGE_BYTES);
        cache.admit_limits(NORMALIZED_PAGE_BYTES, NORMALIZED_PAGE_BYTES, 2 * NORMALIZED_PAGE_BYTES);
        assert_eq!(cache.slot_limit(), 1);
        assert_eq!(cache.mip_budget(), 2 * NORMALIZED_PAGE_BYTES);
    }
}
