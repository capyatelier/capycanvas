//! Queue-ordered exact tile capture; mapping/compression runs on a worker.
use super::*;
use crate::native_tiles::{NativeEncodeStatus, STATUS_BYTES};
use layer_core::color::{AlphaAssociation, PixelDescriptor, TransferEncoding};
use layer_core::raster::{
    RasterData, RasterPlane, RasterRevision, RasterTile, RasterWatercolor, TileBlob, TileKey,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

const CAPTURE_CHUNK: u64 = 16 * 1024 * 1024;
use layer_core::raster::MAX_CAPTURE_BYTES;

#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(target_arch = "wasm32")]
pub use browser::BrowserRasterEncoder;
#[cfg(target_arch = "wasm32")]
use browser::CaptureWorker;

// Reuse unmapped staging allocations; allocation/zeroing at pen-up can cost
// several milliseconds even when the queue copy itself is cheap.
#[derive(Default)]
pub(super) struct BufferPool {
    buffers: std::sync::Mutex<Vec<wgpu::Buffer>>,
    bytes: AtomicU64,
    working: AtomicU64,
}
impl BufferPool {
    fn take(&self, device: &wgpu::Device, size: u64) -> wgpu::Buffer {
        let mut buffers = self.buffers.lock().unwrap();
        if let Some(index) = buffers.iter().position(|b| b.size() == size) {
            self.bytes.fetch_sub(size, Ordering::Relaxed);
            return buffers.remove(index);
        }
        drop(buffers);
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bounded raster capture chunk"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        })
    }
    fn put(&self, buffer: wgpu::Buffer) {
        let mut buffers = self.buffers.lock().unwrap();
        if buffer.size() == STATUS_BYTES
            && buffers.iter().filter(|b| b.size() == STATUS_BYTES).count() >= 16
        {
            return;
        }
        while self.bytes.load(Ordering::Relaxed) + buffer.size() > 64 * 1024 * 1024 {
            if buffers.is_empty() {
                break;
            }
            // Retain recently used sizes. Keeping four large startup spares
            // forever prevents all smaller tiles/status buffers from entering
            // the cache and forces a pinned allocation at every small commit.
            let old = buffers.remove(0);
            self.bytes.fetch_sub(old.size(), Ordering::Relaxed);
        }
        if self.bytes.load(Ordering::Relaxed) + buffer.size() <= 64 * 1024 * 1024 {
            self.bytes.fetch_add(buffer.size(), Ordering::Relaxed);
            buffers.push(buffer);
        }
    }
}
fn capture_allocation(bytes: u64) -> u64 {
    let tail = bytes % CAPTURE_CHUNK;
    bytes / CAPTURE_CHUNK * CAPTURE_CHUNK
        + if tail == 0 {
            0
        } else {
            tail.next_power_of_two()
        }
}

struct Target {
    source: Option<AssetId>,
    revision: RasterRevision,
    data: Arc<RasterData>,
    changed: BTreeSet<[u32; 2]>,
}

fn restored_damage(
    before: &RasterData,
    after: &RasterData,
    changed: &BTreeSet<[u32; 2]>,
    extent: [u32; 2],
) -> Vec<PixelRect> {
    let mut coordinates = changed.clone();
    let style_changed = before.watercolor != after.watercolor;
    for key in before.tiles.keys().chain(after.tiles.keys()) {
        let same = before
            .tiles
            .get(key)
            .zip(after.tiles.get(key))
            .is_some_and(|(a, b)| a.same_capture(b));
        if !same || style_changed {
            coordinates.insert(key.coordinate);
        }
    }
    // Preserve holes between changed pages. Watercolor neighbors expand each
    // actual footprint, rather than expanding one large enclosing rectangle.
    let radius = if before.watercolor.is_some() || after.watercolor.is_some() {
        PAGE_SIZE
    } else {
        0
    };
    coordinates
        .into_iter()
        .flat_map(|coordinate| {
            page_coordinates(
                page_rect(coordinate)
                    .intersect(PixelRect::full(extent))
                    .expand(radius, extent),
            )
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|coordinate| page_rect(coordinate).intersect(PixelRect::full(extent)))
        .collect()
}

#[derive(Default)]
pub(super) struct RasterRuntime {
    targets: BTreeMap<LayerId, Target>,
    worker: Option<CaptureWorker>,
    #[cfg(target_arch = "wasm32")]
    encoder: Option<BrowserRasterEncoder>,
}
#[cfg(not(target_arch = "wasm32"))]
struct CaptureWorker {
    sender: Option<mpsc::SyncSender<Vec<RasterCapture>>>,
    pending: Arc<AtomicUsize>,
    thread: Option<std::thread::JoinHandle<()>>,
    staging: Arc<AtomicU64>,
    error: Arc<std::sync::Mutex<Option<String>>>,
    prepared: Arc<std::sync::atomic::AtomicBool>,
}
#[cfg(not(target_arch = "wasm32"))]
impl CaptureWorker {
    fn new(device: wgpu::Device, pool: Arc<BufferPool>) -> Result<Self, GpuRasterError> {
        let (sender, receiver) = mpsc::sync_channel::<Vec<RasterCapture>>(16);
        let pending = Arc::new(AtomicUsize::new(0));
        let count = pending.clone();
        let staging = Arc::new(AtomicU64::new(0));
        let bytes = staging.clone();
        let error = Arc::new(std::sync::Mutex::new(None));
        let failure = error.clone();
        let prepared = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let preparation = prepared.clone();
        let thread = std::thread::Builder::new()
            .name("capy-raster-backing".into())
            .spawn(move || {
                // Establish the bounded spare pool on its worker. A burst of
                // small commits must not allocate pinned memory at every pen-up.
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    for _ in 0..4 {
                        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("raster staging spare"),
                            size: CAPTURE_CHUNK,
                            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                            mapped_at_creation: false,
                        });
                        pool.put(buffer);
                    }
                }))
                .is_err()
                {
                    *failure.lock().unwrap() =
                        Some("Could not prepare raster staging buffers".into());
                }
                preparation.store(true, Ordering::Release);
                while let Ok(captures) = receiver.recv() {
                    let size: u64 = captures.iter().map(|c| c.staging_bytes).sum();
                    // Dropped tickets publish failures even if a driver callback or
                    // compression panics; never leave backpressure permanently set.
                    let result = if failure.lock().unwrap().is_some() {
                        drop(captures);
                        Ok(())
                    } else {
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            for capture in captures {
                                capture.finish()?;
                            }
                            Ok::<_, String>(())
                        }))
                        .unwrap_or_else(|_| Err("Raster backing worker panicked".into()))
                    };
                    if let Err(message) = result {
                        *failure.lock().unwrap() = Some(message);
                    }
                    bytes.fetch_sub(size, Ordering::Release);
                    count.fetch_sub(1, Ordering::Release);
                }
            })
            .map_err(|e| GpuRasterError::Effect(e.to_string()))?;
        Ok(Self {
            sender: Some(sender),
            pending,
            staging,
            error,
            prepared,
            thread: Some(thread),
        })
    }
    fn ready(&self) -> bool {
        // Reserve room for the largest legal next capture. The total staging
        // ceiling remains 512 MiB, while small edits can share that allowance.
        self.prepared.load(Ordering::Acquire)
            && self.pending.load(Ordering::Acquire) < 16
            && self.staging.load(Ordering::Acquire) <= MAX_CAPTURE_BYTES
    }
    fn submit(&self, captures: Vec<RasterCapture>) -> Result<(), GpuRasterError> {
        let size = captures.iter().map(|c| c.staging_bytes).sum();
        self.staging.fetch_add(size, Ordering::Release);
        self.pending.fetch_add(1, Ordering::Release);
        if self.sender.as_ref().unwrap().try_send(captures).is_err() {
            self.pending.fetch_sub(1, Ordering::Release);
            self.staging.fetch_sub(size, Ordering::Release);
            return Err(GpuRasterError::Effect(
                "Raster backing queue is full or stopped".into(),
            ));
        }
        Ok(())
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl Drop for CaptureWorker {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Entry {
    #[cfg(not(target_arch = "wasm32"))]
    offset: u64,
    size: u64,
    descriptor: PixelDescriptor,
    tile: RasterTile,
}
struct Chunk {
    buffer: wgpu::Buffer,
    entries: Vec<Entry>,
    #[cfg(not(target_arch = "wasm32"))]
    ready: mpsc::Receiver<Result<(), String>>,
    #[cfg(target_arch = "wasm32")]
    ready: futures_channel::oneshot::Receiver<Result<(), String>>,
}
impl Chunk {
    fn map(buffer: wgpu::Buffer, entries: Vec<Entry>) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let (tx, ready) = mpsc::channel();
        #[cfg(target_arch = "wasm32")]
        let (tx, ready) = futures_channel::oneshot::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result.map_err(|e| e.to_string()));
            });
        Self {
            buffer,
            entries,
            ready,
        }
    }
}

pub(crate) struct TileCapture<'a> {
    pub descriptor: PixelDescriptor,
    pub texture: &'a wgpu::Texture,
    pub tile: RasterTile,
}
impl TileCapture<'_> {
    fn byte_len(&self) -> Result<u64, GpuRasterError> {
        let t = self.texture;
        let d = self.descriptor;
        let rgba = d.channels == 4
            && matches!(
                d.alpha,
                AlphaAssociation::Straight | AlphaAssociation::PremultipliedLinear
            );
        let valid_format = match t.format() {
            wgpu::TextureFormat::R8Unorm => d == PixelDescriptor::COVERAGE8,
            wgpu::TextureFormat::Rgba8UnormSrgb => {
                rgba && d.bits_per_channel == 8 && d.encoding == TransferEncoding::Srgb
            }
            wgpu::TextureFormat::Rgba8Uint | wgpu::TextureFormat::Rgba16Uint => {
                rgba && d.bits_per_channel
                    == if t.format() == wgpu::TextureFormat::Rgba8Uint {
                        8
                    } else {
                        16
                    }
                    && matches!(
                        d.encoding,
                        TransferEncoding::Srgb | TransferEncoding::Profile
                    )
            }
            _ => false,
        };
        if !valid_format
            || t.width() != PAGE_SIZE
            || t.height() != PAGE_SIZE
            || t.depth_or_array_layers() != 1
            || t.mip_level_count() != 1
            || t.sample_count() != 1
            || t.dimension() != wgpu::TextureDimension::D2
            || !t.usage().contains(wgpu::TextureUsages::COPY_SRC)
        {
            return Err(GpuRasterError::Color(
                "Invalid raster capture representation".into(),
            ));
        }
        Ok(d.byte_len([PAGE_SIZE; 2]).unwrap() as u64)
    }
}

pub struct RasterCapture {
    #[cfg(not(target_arch = "wasm32"))]
    device: wgpu::Device,
    submission: wgpu::SubmissionIndex,
    chunks: Vec<Chunk>,
    validation: Option<Chunk>,
    pool: Arc<BufferPool>,
    pub staging_bytes: u64,
}
impl RasterCapture {
    /// Worker only. Cached readback scratch is bounded to four 16 MiB chunks.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn finish(mut self) -> Result<(), String> {
        let result: Result<(), String> = (|| {
            self.device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(self.submission.clone()),
                    timeout: Some(READBACK_TIMEOUT),
                })
                .map_err(|e| e.to_string())?;
            if let Some(validation) = &self.validation {
                validation
                    .ready
                    .recv_timeout(READBACK_TIMEOUT)
                    .map_err(|e| e.to_string())??;
                self.accept_validation()?;
            }
            fn finish_chunk(chunk: &Chunk, pool: &BufferPool, lanes: usize) -> Result<(), String> {
                chunk
                    .ready
                    .recv_timeout(READBACK_TIMEOUT)
                    .map_err(|e| e.to_string())??;
                let mapped = chunk
                    .buffer
                    .slice(..)
                    .get_mapped_range()
                    .map_err(|e| e.to_string())?;
                // Mapped readback memory is expensive for a compressor's repeated
                // accesses. Copy once into cached memory, then return the GPU
                // allocation immediately. At most four 16 MiB chunks exist here.
                struct Scratch<'a> {
                    bytes: Vec<u8>,
                    pool: &'a BufferPool,
                }
                impl Drop for Scratch<'_> {
                    fn drop(&mut self) {
                        self.pool
                            .working
                            .fetch_sub(self.bytes.len() as u64, Ordering::Relaxed);
                    }
                }
                let bytes = Scratch {
                    bytes: mapped.to_vec(),
                    pool,
                };
                pool.working
                    .fetch_add(bytes.bytes.len() as u64, Ordering::Relaxed);
                drop(mapped);
                chunk.buffer.unmap();
                pool.put(chunk.buffer.clone());
                let encode = |entries: &[Entry]| -> Result<(), String> {
                    for entry in entries {
                        let begin = entry.offset as usize;
                        entry.tile.publish(TileBlob::encode(
                            entry.descriptor,
                            &bytes.bytes[begin..begin + entry.size as usize],
                        ))?;
                    }
                    Ok(())
                };
                if lanes > 1 && chunk.entries.len() >= 8 {
                    std::thread::scope(|scope| {
                        let mut jobs = Vec::new();
                        for entries in chunk.entries.chunks(chunk.entries.len().div_ceil(lanes)) {
                            jobs.push(scope.spawn(move || encode(entries)));
                        }
                        for job in jobs {
                            job.join()
                                .map_err(|_| "Raster compression worker panicked")??;
                        }
                        Ok::<_, String>(())
                    })?;
                } else {
                    encode(&chunk.entries)?;
                }
                Ok(())
            }
            if self.chunks.len() == 1 {
                finish_chunk(&self.chunks[0], &self.pool, 4)?;
            } else {
                let group_size = self.chunks.len().div_ceil(4);
                let lanes = 4 / self.chunks.len().min(4);
                std::thread::scope(|scope| {
                    let mut jobs = Vec::new();
                    for group in self.chunks.chunks_mut(group_size) {
                        let pool = &self.pool;
                        jobs.push(scope.spawn(move || {
                            for chunk in group {
                                finish_chunk(chunk, pool, lanes)?;
                            }
                            Ok::<_, String>(())
                        }));
                    }
                    for job in jobs {
                        job.join()
                            .map_err(|_| "Raster compression worker panicked")??;
                    }
                    Ok::<_, String>(())
                })?;
            }
            Ok(())
        })();
        if let Err(error) = &result {
            self.fail(error);
        }
        result
    }
    fn accept_validation(&self) -> Result<(), String> {
        let Some(validation) = &self.validation else {
            return Ok(());
        };
        let mapped = validation
            .buffer
            .get_mapped_range(..)
            .map_err(|e| e.to_string())?;
        let result = NativeEncodeStatus::decode(&mapped).map_err(|e| e.to_string());
        drop(mapped);
        validation.buffer.unmap();
        self.pool.put(validation.buffer.clone());
        result.map(|_| ())
    }
    fn fail(&self, error: &str) {
        for chunk in &self.chunks {
            for entry in &chunk.entries {
                if entry.tile.try_backing().is_none() {
                    let _ = entry.tile.publish(Err(error.into()));
                }
            }
        }
    }
}
impl Drop for RasterCapture {
    fn drop(&mut self) {
        self.fail("Raster capture was abandoned before host backing completed");
    }
}

impl WgpuRasterizer {
    pub(super) fn raster_restore_ready(&self, packet: FramePacket<'_>) -> bool {
        let runtime = self.raster.as_ref();
        let ready = |current: Option<&Target>, data: &RasterData| {
            data.tiles.iter().all(|(key, tile)| {
                let retained = !packet.reset_layers
                    && current.is_some_and(|t| {
                        !t.changed.contains(&key.coordinate)
                            && t.data
                                .tiles
                                .get(key)
                                .is_some_and(|old| old.same_capture(tile))
                    });
                retained || tile.try_backing().is_some()
            })
        };
        for (id, root) in packet.restore_rasters {
            let current = runtime.and_then(|r| r.targets.get(id));
            match root.try_data() {
                None => return false,
                Some(Ok(data)) if !ready(current, &data) => return false,
                _ => {}
            }
        }
        for layer in packet.layers {
            for (id, root) in std::iter::once((layer.id, &layer.raster))
                .chain(layer.mask.iter().map(|m| (m.id, &m.raster)))
            {
                let source = if id == layer.id {
                    layer.asset.as_ref()
                } else {
                    None
                };
                let current = runtime
                    .and_then(|r| r.targets.get(&id))
                    .filter(|t| t.source.as_ref() == source);
                match root.try_data() {
                    Some(Ok(data))
                        if packet.reset_layers || current.is_none_or(|t| t.revision != *root) =>
                    {
                        if !ready(current, &data) {
                            return false;
                        }
                    }
                    None if packet.reset_layers => {
                        if let Some(current) = current
                            && !ready(Some(current), &current.data)
                        {
                            return false;
                        }
                    }
                    _ => {}
                }
            }
        }
        true
    }

    /// Browser hosts supply a worker encoder; GPU mappings remain asynchronous
    /// on their owning event loop. All candidates must retain this transport.
    #[cfg(target_arch = "wasm32")]
    pub fn set_browser_raster_encoder(&mut self, encoder: BrowserRasterEncoder) {
        self.raster.get_or_insert_with(Default::default).encoder = Some(encoder);
    }

    pub fn raster_ready(&self) -> bool {
        self.raster
            .as_ref()
            .and_then(|r| r.worker.as_ref())
            .is_none_or(CaptureWorker::ready)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn prepare_source_backing(&mut self) -> Result<(), GpuRasterError> {
        let runtime = self.raster.get_or_insert_with(Default::default);
        if runtime.worker.is_none() {
            runtime.worker = Some(CaptureWorker::new(
                (*self.device).clone(),
                self.raster_buffers.clone(),
            )?);
        }
        Ok(())
    }

    pub(super) fn reconcile_rasters(
        &mut self,
        packet: FramePacket<'_>,
        reset: bool,
    ) -> Result<Vec<(LayerId, PixelRect)>, GpuRasterError> {
        let mut runtime = self.raster.take().unwrap_or_default();
        let result = (|| {
            let mut damage = Vec::new();
            if let Some(error) = runtime
                .worker
                .as_ref()
                .and_then(|w| w.error.lock().unwrap().clone())
            {
                return Err(GpuRasterError::Effect(error));
            }
            runtime.targets.retain(|id, _| {
                packet
                    .layers
                    .iter()
                    .any(|l| l.id == *id || l.masks().any(|m| m.id == *id))
            });
            for (id, revision) in packet.restore_rasters {
                if let Some(current) = runtime.targets.get_mut(id) {
                    let data = revision.wait_data().map_err(GpuRasterError::Effect)?;
                    damage.extend(
                        restored_damage(
                            &current.data,
                            &data,
                            &current.changed,
                            packet.document_extent,
                        )
                        .into_iter()
                        .map(|rect| (*id, rect)),
                    );
                    let mut before = if reset {
                        RasterData::default()
                    } else {
                        (*current.data).clone()
                    };
                    before
                        .tiles
                        .retain(|key, _| !current.changed.contains(&key.coordinate));
                    self.restore_raster(*id, &before, &data)?;
                    current.revision = revision.clone();
                    current.data = data;
                    current.changed.clear();
                }
            }
            for layer in packet.layers {
                for (id, revision) in std::iter::once((layer.id, &layer.raster))
                    .chain(layer.masks().map(|m| (m.id, &m.raster)))
                {
                    // Backgrounds, groups and generators have no editable color pages.
                    if id == layer.id && !self.paint_layers.iter().any(|l| l.id == id) {
                        continue;
                    }
                    let source = if id == layer.id {
                        layer.asset.clone()
                    } else {
                        None
                    };
                    if runtime.targets.get(&id).is_some_and(|t| t.source != source) {
                        runtime.targets.remove(&id);
                    }
                    let wanted = match revision.try_data() {
                        Some(Ok(data)) => Some(data),
                        Some(Err(error)) => return Err(GpuRasterError::Effect(error)),
                        None => None,
                    };
                    if let Some(current) = runtime.targets.get_mut(&id) {
                        if reset || (wanted.is_some() && current.revision != *revision) {
                            let data = wanted.unwrap_or_else(|| current.data.clone());
                            damage.extend(
                                restored_damage(
                                    &current.data,
                                    &data,
                                    &current.changed,
                                    packet.document_extent,
                                )
                                .into_iter()
                                .map(|rect| (id, rect)),
                            );
                            let mut before = if reset {
                                RasterData::default()
                            } else {
                                (*current.data).clone()
                            };
                            before
                                .tiles
                                .retain(|key, _| !current.changed.contains(&key.coordinate));
                            self.restore_raster(id, &before, &data)?;
                            current.data = data;
                            current.changed.clear();
                            if revision.try_data().is_some() {
                                current.revision = revision.clone();
                            }
                        }
                    } else {
                        let data = wanted.unwrap_or_default();
                        if !data.tiles.is_empty() {
                            damage.extend(
                                restored_damage(
                                    &RasterData::default(),
                                    &data,
                                    &BTreeSet::new(),
                                    packet.document_extent,
                                )
                                .into_iter()
                                .map(|rect| (id, rect)),
                            );
                            self.restore_raster(id, &RasterData::default(), &data)?;
                        }
                        runtime.targets.insert(
                            id,
                            Target {
                                source,
                                revision: revision.clone(),
                                data,
                                changed: BTreeSet::new(),
                            },
                        );
                    }
                }
            }
            for batch in packet
                .dab_batches
                .iter()
                .filter(|b| b.kind != DabBatchKind::Preview)
            {
                if let Some(target) = runtime.targets.get_mut(&batch.layer_id) {
                    // Transport is included in batch damage. Terminal edge work
                    // touches only this contact's coverage; earlier batches have
                    // already accumulated their changed pages in this target.
                    // Operation damage already includes selection bounds and
                    // transformed source/destination footprints.
                    let damage = batch_pixel_rect(batch, packet.document_extent);
                    target.changed.extend(page_coordinates(damage));
                    if batch.stroke_end
                        && batch.style.rendering.edge_after_stroke
                        && let Some(layer) =
                            self.paint_layers.iter().find(|l| l.id == batch.layer_id)
                    {
                        target.changed.extend(
                            layer
                                .coverage_pages
                                .iter()
                                .filter(|p| p.owner == Some(batch.stroke_id))
                                .map(|p| p.coordinate),
                        );
                    }
                }
            }
            Ok(damage)
        })();
        self.raster = Some(runtime);
        result
    }

    pub(super) fn commit_rasters(&mut self, layers: &[Layer]) -> Result<(), GpuRasterError> {
        if !layers.iter().any(|l| {
            l.raster.try_data().is_none() || l.masks().any(|m| m.raster.try_data().is_none())
        }) {
            return Ok(());
        }
        let mut runtime = self.raster.take().unwrap_or_default();
        let result = (|| {
            if runtime.worker.as_ref().is_some_and(|w| !w.ready()) {
                return Err(GpuRasterError::Effect(
                    "Raster backing queue is full".into(),
                ));
            }
            let mut staging = 0;
            for layer in layers {
                for (id, revision) in std::iter::once((layer.id, &layer.raster))
                    .chain(layer.mask.iter().map(|m| (m.id, &m.raster)))
                {
                    if revision.try_data().is_some() {
                        continue;
                    }
                    let Some(current) = runtime.targets.get(&id) else {
                        continue;
                    };
                    let (textures, _) = self.raster_textures(id);
                    let mut target_bytes = 0;
                    for key in textures.keys() {
                        if !current.data.tiles.contains_key(key)
                            || current.changed.contains(&key.coordinate)
                        {
                            target_bytes +=
                                key.plane.descriptor().byte_len([PAGE_SIZE; 2]).unwrap() as u64;
                        }
                    }
                    staging += capture_allocation(target_bytes);
                }
            }
            if staging > MAX_CAPTURE_BYTES {
                return Err(GpuRasterError::Effect(
                    "Raster frame exceeds the 256 MiB staging budget".into(),
                ));
            }
            if staging > 0 && runtime.worker.is_none() {
                runtime.worker = Some(CaptureWorker::new(
                    (*self.device).clone(),
                    self.raster_buffers.clone(),
                    #[cfg(target_arch = "wasm32")]
                    runtime.encoder.clone().ok_or_else(|| {
                        GpuRasterError::Effect("Browser raster worker is unavailable".into())
                    })?,
                )?);
            }
            let mut captures = Vec::new();
            for layer in layers {
                for (id, revision) in std::iter::once((layer.id, &layer.raster))
                    .chain(layer.mask.iter().map(|m| (m.id, &m.raster)))
                {
                    if revision.try_data().is_some() {
                        continue;
                    }
                    let Some(current) = runtime.targets.get_mut(&id) else {
                        continue;
                    };
                    if let Some(capture) =
                        self.capture_raster(id, &current.data, &current.changed, revision)?
                    {
                        // Include capture copies in GPU-completed frame timings.
                        self.last_submission = Some(capture.submission.clone());
                        captures.push(capture);
                    }
                    current.revision = revision.clone();
                    current.data = revision.wait_data().map_err(GpuRasterError::Effect)?;
                    current.changed.clear();
                }
            }
            if !captures.is_empty() {
                runtime.worker.as_ref().unwrap().submit(captures)?;
            }
            Ok(())
        })();
        self.raster = Some(runtime);
        result
    }

    pub(super) fn has_raster_source(&self, target: LayerId) -> bool {
        self.raster
            .as_ref()
            .and_then(|r| r.targets.get(&target))
            .is_some_and(|t| !t.data.tiles.is_empty())
    }

    pub(super) fn raster_staging_bytes(&self) -> u64 {
        self.raster_buffers.bytes.load(Ordering::Relaxed)
            + self.raster_buffers.working.load(Ordering::Relaxed)
            + self
                .raster
                .as_ref()
                .and_then(|r| r.worker.as_ref())
                .map_or(0, |w| w.staging.load(Ordering::Acquire))
    }

    fn raster_textures(
        &self,
        target: LayerId,
    ) -> (BTreeMap<TileKey, &wgpu::Texture>, Option<RasterWatercolor>) {
        let mut textures = BTreeMap::new();
        let mut watercolor = None;
        if let Some(layer) = self.paint_layers.iter().find(|l| l.id == target) {
            for page in &layer.pages {
                textures.insert(
                    TileKey {
                        plane: RasterPlane::Color,
                        coordinate: page.coordinate,
                    },
                    &page.active().texture,
                );
            }
            for page in &layer.material_pages {
                textures.insert(
                    TileKey {
                        plane: RasterPlane::Wetness,
                        coordinate: page.coordinate,
                    },
                    &page.wetness.texture,
                );
            }
            for page in &layer.watercolor_wetness_pages {
                textures.insert(
                    TileKey {
                        plane: RasterPlane::WatercolorWetness,
                        coordinate: page.coordinate,
                    },
                    &page.active().texture,
                );
            }
            watercolor = layer.watercolor.map(|w| RasterWatercolor {
                wet_edge: w.wet_edge,
                burnt_edge: w.burnt_edge,
                edge_width: w.edge_width,
            });
        } else {
            for ((id, coordinate), page) in &self.layer_masks.pages {
                if *id == target {
                    textures.insert(
                        TileKey {
                            plane: RasterPlane::Mask,
                            coordinate: *coordinate,
                        },
                        &page.texture,
                    );
                }
            }
        }
        (textures, watercolor)
    }

    /// Capture only changed physical pages. The caller reserves bounded worker
    /// capacity before issuing this request and owns the returned ticket until
    /// every tile is host-backed. Unchanged captures are reused by identity.
    pub fn capture_raster(
        &self,
        target: LayerId,
        previous: &RasterData,
        changed: &BTreeSet<[u32; 2]>,
        revision: &RasterRevision,
    ) -> Result<Option<RasterCapture>, GpuRasterError> {
        let (textures, watercolor) = self.raster_textures(target);
        let mut data = RasterData {
            tiles: BTreeMap::new(),
            watercolor,
        };
        let mut copies = Vec::new();
        let mut total = 0;
        for (key, texture) in textures {
            if let Some(tile) = previous
                .tiles
                .get(&key)
                .filter(|_| !changed.contains(&key.coordinate))
            {
                data.tiles.insert(key, tile.clone());
            } else {
                let size = key.plane.descriptor().byte_len([PAGE_SIZE; 2]).unwrap() as u64;
                total += size;
                if total > MAX_CAPTURE_BYTES {
                    return Err(GpuRasterError::Effect(
                        "Raster capture exceeds the 256 MiB staging budget".into(),
                    ));
                }
                let tile = RasterTile::default();
                data.tiles.insert(key, tile.clone());
                copies.push(TileCapture {
                    descriptor: key.plane.descriptor(),
                    texture,
                    tile,
                });
            }
        }
        if copies.is_empty() {
            revision.publish(Ok(data)).map_err(GpuRasterError::Effect)?;
            return Ok(None);
        }
        let capture = self.capture_tiles(&copies, None)?;
        revision.publish(Ok(data)).map_err(GpuRasterError::Effect)?;
        Ok(Some(capture))
    }

    /// Capture already queue-ordered native candidates through the same bounded
    /// worker as ordinary paint. Validate every input and the actual rounded
    /// staging allocation before recording copies. A publication-wide status,
    /// when present, is checked before any tile backing is published.
    pub(crate) fn capture_tiles(
        &self,
        copies: &[TileCapture<'_>],
        status: Option<&NativeEncodeStatus>,
    ) -> Result<RasterCapture, GpuRasterError> {
        if copies.is_empty() || copies.len() as u64 > MAX_CAPTURE_BYTES / SCALAR_PAGE_BYTES {
            return Err(GpuRasterError::Color(
                "Invalid raster capture tile count".into(),
            ));
        }
        let mut identities = std::collections::HashSet::new();
        let sizes: Vec<_> = copies
            .iter()
            .map(|copy| {
                if !identities.insert(copy.tile.identity()) || copy.tile.try_backing().is_some() {
                    return Err(GpuRasterError::Color(
                        "Raster capture reuses a publication ticket".into(),
                    ));
                }
                copy.byte_len()
            })
            .collect::<Result<_, _>>()?;
        let mut ranges = Vec::new();
        let mut start = 0;
        let mut staging_bytes = if status.is_some() { STATUS_BYTES } else { 0 };
        while start < copies.len() {
            let mut end = start;
            let mut bytes = 0;
            while end < copies.len() && bytes + sizes[end] <= CAPTURE_CHUNK {
                bytes += sizes[end];
                end += 1;
            }
            let allocation = bytes.next_power_of_two();
            staging_bytes += allocation;
            if staging_bytes > MAX_CAPTURE_BYTES {
                return Err(GpuRasterError::Effect(
                    "Raster capture exceeds the 256 MiB staging budget".into(),
                ));
            }
            ranges.push((start..end, allocation));
            start = end;
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("immutable raster revision capture"),
            });
        let mut chunks = Vec::with_capacity(ranges.len());
        for (range, allocation) in ranges {
            let buffer = self.raster_buffers.take(&self.device, allocation);
            let mut entries = Vec::with_capacity(range.len());
            let mut offset = 0;
            for index in range {
                let copy = &copies[index];
                let size = sizes[index];
                encoder.copy_texture_to_buffer(
                    copy.texture.as_image_copy(),
                    wgpu::TexelCopyBufferInfo {
                        buffer: &buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset,
                            bytes_per_row: Some((size / u64::from(PAGE_SIZE)) as u32),
                            rows_per_image: Some(PAGE_SIZE),
                        },
                    },
                    copy.texture.size(),
                );
                entries.push(Entry {
                    #[cfg(not(target_arch = "wasm32"))]
                    offset,
                    size,
                    descriptor: copy.descriptor,
                    tile: copy.tile.clone(),
                });
                offset += size;
            }
            chunks.push((buffer, entries));
        }
        let validation = status.map(|status| {
            let buffer = self.raster_buffers.take(&self.device, STATUS_BYTES);
            encoder.copy_buffer_to_buffer(status.buffer(), 0, &buffer, 0, STATUS_BYTES);
            buffer
        });
        let submission = self.queue.submit([encoder.finish()]);
        Ok(RasterCapture {
            #[cfg(not(target_arch = "wasm32"))]
            device: (*self.device).clone(),
            submission,
            chunks: chunks.into_iter().map(|(b, e)| Chunk::map(b, e)).collect(),
            validation: validation.map(|b| Chunk::map(b, Vec::new())),
            staging_bytes,
            pool: self.raster_buffers.clone(),
        })
    }

    /// Restore changed pages only, from exact backing. Called on the GPU owner
    /// after backing is ready; no historical dabs are generated.
    pub fn restore_raster(
        &mut self,
        target: LayerId,
        previous: &RasterData,
        data: &RasterData,
    ) -> Result<(), GpuRasterError> {
        let index = self.paint_layers.iter().position(|l| l.id == target);
        let mask = index.is_none();
        data.validate_index(self.document_extent, mask)
            .map_err(GpuRasterError::Effect)?;
        // Stage GPU pages before publishing the revision. Keep only one decoded
        // tile on the CPU, rather than a second full decoded document. A failed
        // tile still leaves every live page (including removed pages) intact.
        enum Replacement {
            Color(LayerPage),
            Mask([u32; 2], layer_masks::MaskPage),
            Wetness(CanvasMaterialPage),
            Watercolor(WatercolorWetnessPage),
        }
        let mut replacements = Vec::new();
        for (key, tile) in &data.tiles {
            if previous
                .tiles
                .get(key)
                .is_some_and(|old| old.same_capture(tile))
            {
                continue;
            }
            let blob = tile.wait_backing().map_err(GpuRasterError::Effect)?;
            if blob.descriptor != key.plane.descriptor() {
                return Err(GpuRasterError::Effect(
                    "Raster plane has the wrong pixel representation".into(),
                ));
            }
            let bytes = blob.decode().map_err(GpuRasterError::Effect)?;
            let (replacement, texture) = match key.plane {
                RasterPlane::Color => {
                    let mut page = self.create_page(key.coordinate, "restored raster tile");
                    page.primary_needs_clear = false;
                    let texture = page.primary.texture.clone();
                    (Replacement::Color(page), texture)
                }
                RasterPlane::Mask => {
                    let page = layer_masks::MaskPage::new(&self.device);
                    let texture = page.texture.clone();
                    (Replacement::Mask(key.coordinate, page), texture)
                }
                RasterPlane::Wetness => {
                    let wetness = self.create_scalar_page_surface("restored wetness tile");
                    let texture = wetness.texture.clone();
                    (
                        Replacement::Wetness(CanvasMaterialPage {
                            coordinate: key.coordinate,
                            wetness,
                            needs_clear: false,
                        }),
                        texture,
                    )
                }
                RasterPlane::WatercolorWetness => {
                    let primary = self.create_scalar_page_surface("restored watercolor wetness");
                    let secondary = self.create_scalar_page_surface("watercolor wetness companion");
                    let texture = primary.texture.clone();
                    (
                        Replacement::Watercolor(WatercolorWetnessPage {
                            coordinate: key.coordinate,
                            primary,
                            secondary,
                            active_secondary: false,
                            primary_needs_clear: false,
                            secondary_needs_clear: true,
                        }),
                        texture,
                    )
                }
            };
            self.queue.write_texture(
                texture.as_image_copy(),
                &bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some((bytes.len() / PAGE_SIZE as usize) as u32),
                    rows_per_image: Some(PAGE_SIZE),
                },
                texture.size(),
            );
            replacements.push(replacement);
        }
        if let Some(index) = index {
            let layer = &mut self.paint_layers[index];
            layer.pages.retain(|p| {
                data.tiles.contains_key(&TileKey {
                    plane: RasterPlane::Color,
                    coordinate: p.coordinate,
                })
            });
            layer.material_pages.retain(|p| {
                data.tiles.contains_key(&TileKey {
                    plane: RasterPlane::Wetness,
                    coordinate: p.coordinate,
                })
            });
            layer.watercolor_wetness_pages.retain(|p| {
                data.tiles.contains_key(&TileKey {
                    plane: RasterPlane::WatercolorWetness,
                    coordinate: p.coordinate,
                })
            });
            layer.coverage_pages.clear();
            layer.watercolor = data.watercolor.map(|w| WatercolorLayerStyle {
                wet_edge: w.wet_edge,
                burnt_edge: w.burnt_edge,
                edge_width: w.edge_width,
            });
        } else {
            self.layer_masks.pages.retain(|(id, coordinate), _| {
                *id != target
                    || data.tiles.contains_key(&TileKey {
                        plane: RasterPlane::Mask,
                        coordinate: *coordinate,
                    })
            });
        }
        for replacement in replacements {
            match replacement {
                Replacement::Color(page) => {
                    let pages = &mut self.paint_layers[index.unwrap()].pages;
                    pages.retain(|p| p.coordinate != page.coordinate);
                    pages.push(page);
                }
                Replacement::Mask(coordinate, page) => {
                    self.layer_masks.pages.insert((target, coordinate), page);
                }
                Replacement::Wetness(page) => {
                    let pages = &mut self.paint_layers[index.unwrap()].material_pages;
                    pages.retain(|p| p.coordinate != page.coordinate);
                    pages.push(page);
                }
                Replacement::Watercolor(page) => {
                    let pages = &mut self.paint_layers[index.unwrap()].watercolor_wetness_pages;
                    pages.retain(|p| p.coordinate != page.coordinate);
                    pages.push(page);
                }
            }
        }
        self.preview_pages.clear();
        self.preview_coverage_pages.clear();
        self.preview_watercolor_wetness_pages.clear();
        self.preview_damage = PixelRect::EMPTY;
        self.preview_layer_id = None;
        if let Some(scene) = &mut self.scene {
            scene.begin_frame();
        }
        Ok(())
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod native_tests;
