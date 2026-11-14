//! Queue-ordered exact tile capture; mapping/compression runs on a worker.
use super::*;
use layer_core::color::{AlphaAssociation, PixelDescriptor, TransferEncoding};
use layer_core::raster::{
    RasterData, RasterPlane, RasterRevision, RasterTile, RasterWatercolor, TileBlob, TileKey,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    Arc,
    atomic::Ordering,
};

const CAPTURE_CHUNK: u64 = 16 * 1024 * 1024;
use layer_core::raster::MAX_CAPTURE_BYTES;

#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(target_arch = "wasm32")]
pub use browser::BrowserRasterEncoder;
#[cfg(target_arch = "wasm32")]
use browser::CaptureWorker;

pub(crate) mod pool;
pub(super) use pool::BufferPool;
mod deferred;
pub(crate) mod contents;
pub(crate) use deferred::NativeCapture;
use deferred::NativeOutput;

struct Target {
    revision: RasterRevision,
    contents: contents::Contents,
}



#[derive(Default)]
struct PreparationWake {
    callback: std::sync::Mutex<Option<Arc<dyn Fn()+Send+Sync>>>,
}
impl std::task::Wake for PreparationWake {
    fn wake(self:Arc<Self>) {self.wake_by_ref();}
    fn wake_by_ref(self:&Arc<Self>) {
        let callback=self.callback.lock().unwrap().clone();
        if let Some(callback)=callback {callback();}
    }
}

#[derive(Default)]
pub(super) struct RasterRuntime {
    targets: BTreeMap<SourceTarget, Target>,
    generation: u64,
    worker: Option<CaptureWorker>,
    preparation_wake: Arc<PreparationWake>,
    #[cfg(target_arch = "wasm32")]
    encoder: Option<BrowserRasterEncoder>,
}
impl RasterRuntime {
    fn ensure_worker(&mut self) -> Result<(), GpuRasterError> {
        if self.worker.is_none() {
            self.worker = Some(CaptureWorker::new(
                #[cfg(target_arch = "wasm32")]
                self.encoder.clone().ok_or_else(|| GpuRasterError::Effect("Browser raster worker is unavailable".into()))?,
                self.preparation_wake.clone(),
            )?);
        }
        Ok(())
    }
}
#[cfg(not(target_arch = "wasm32"))]
struct CaptureWorker {
    sender: Option<mpsc::SyncSender<(NativeCapture,[crate::residency::Charge;2])>>,
    pending: Arc<crate::residency::Tier>,
    thread: Option<std::thread::JoinHandle<()>>,
    staging: Arc<crate::residency::Tier>,
    error: Arc<std::sync::Mutex<Option<String>>>,
}
#[cfg(not(target_arch = "wasm32"))]
impl CaptureWorker {
    fn new(wake:Arc<PreparationWake>)->Result<Self,GpuRasterError> {
        let (sender, receiver) = mpsc::sync_channel::<(NativeCapture,[crate::residency::Charge;2])>(16);
        let pending = Arc::default();
        let staging = Arc::default();
        let error = Arc::new(std::sync::Mutex::new(None));
        let failure = error.clone();
        let thread = std::thread::Builder::new()
            .name("capy-raster-backing".into())
            .spawn(move || {
                while let Ok((captures,charges)) = receiver.recv() {
                    // Dropped tickets publish failures even if a driver callback or
                    // compression panics; never leave backpressure permanently set.
                    let result = if failure.lock().unwrap().is_some() {
                        drop(captures);
                        Ok(())
                    } else {
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            captures.finish()
                        }))
                        .unwrap_or_else(|_| Err("Raster backing worker panicked".into()))
                    };
                    if let Err(message) = result {
                        *failure.lock().unwrap() = Some(message);
                    }
                    drop(charges);
                    std::task::Wake::wake_by_ref(&wake);
                }
            })
            .map_err(|e| GpuRasterError::Effect(e.to_string()))?;
        Ok(Self {
            sender: Some(sender),
            pending,
            staging,
            error,
            thread: Some(thread),
        })
    }
    fn ready(&self) -> bool {
        // Backpressure keeps earlier outputs below 240 MiB before admitting
        // another publication. Including the next native output (<= 1 GiB)
        // and one active 16 MiB transfer, live pending storage is <= 1.25 GiB.
        // The mapped-only path retains its 512 MiB ceiling. Spare pool memory
        // is accounted separately; these limits allocate nothing eagerly.
        self.pending.used() < 16
            && self.staging.used() <= MAX_CAPTURE_BYTES - CAPTURE_CHUNK
    }
    fn submit_batch(&self, captures: NativeCapture) -> Result<(), GpuRasterError> {
        let charges = [self.staging.charge(captures.storage_bytes()), self.pending.charge(1)];
        if self.sender.as_ref().unwrap().try_send((captures, charges)).is_err() {
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
    comparison:Option<(u64,crate::native_tiles::comparison::CanonicalReadback)>,
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
    fn map(device:&PipelineDevice,buffer: wgpu::Buffer, entries: Vec<Entry>) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let (tx, ready) = mpsc::channel();
        #[cfg(target_arch = "wasm32")]
        let (tx, ready) = futures_channel::oneshot::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, {let callback=move |result:Result<(),wgpu::BufferAsyncError>| {
                let _ = tx.send(result.map_err(|e| e.to_string()));
            }; #[cfg(not(target_arch="wasm32"))] let callback=device.callback(callback);callback});
        device.request_poll();
        Self {buffer,entries,ready}
    }
}

#[derive(Clone, Copy)]
pub enum CaptureSource<'a> {
    /// Exact encoded color or coverage samples in a validated native format.
    Texture(&'a wgpu::Texture),
    /// One tile of little-endian coverage or gray-alpha samples.
    Packed(&'a wgpu::Buffer),
}

/// One unpublished backing ticket and its queue-ordered native samples. The
/// descriptor is validated against the source before any copies are recorded.
pub struct TileCapture<'a> {
    pub source: CaptureSource<'a>,
    pub tile: RasterTile,
}
impl TileCapture<'_> {
    fn byte_len(&self) -> Result<u64, GpuRasterError> {
        let d = self.tile.descriptor();
        let t = match self.source {
            CaptureSource::Texture(t) => t,
            CaptureSource::Packed(buffer) => {
                let size = d.byte_len([PAGE_SIZE; 2]);
                if !(d.channels == 1 && d.encoding == TransferEncoding::Linear && d.alpha == AlphaAssociation::None
                    || d.channels == 2 && d.alpha == AlphaAssociation::Straight && d.bytes_per_pixel().is_some()
                        && (d.encoding == TransferEncoding::Profile || d.sample == layer_core::color::SampleType::Float && d.encoding == TransferEncoding::Linear))
                    || size.is_none_or(|n| n as u64 != buffer.size())
                    || !buffer.usage().contains(wgpu::BufferUsages::COPY_SRC)
                {
                    return Err(GpuRasterError::Color(
                        "Invalid packed raster capture representation".into(),
                    ));
                }
                return Ok(size.unwrap() as u64);
            }
        };
        let rgba = d.channels == 4
            && matches!(
                d.alpha,
                AlphaAssociation::Straight | AlphaAssociation::PremultipliedLinear
            );
        let valid_format = match t.format() {
            wgpu::TextureFormat::Rgba8Uint | wgpu::TextureFormat::Rgba16Uint | wgpu::TextureFormat::Rgba32Uint => {
                rgba && d.bits_per_channel
                    == if t.format() == wgpu::TextureFormat::Rgba8Uint {
                        8
                    } else if t.format() == wgpu::TextureFormat::Rgba32Uint {
                        32
                    } else {
                        16
                    }
                    && matches!(
                        d.encoding,
                        TransferEncoding::Srgb | TransferEncoding::Profile | TransferEncoding::Linear
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

/// Copies recorded in an owner's command stream. Mapping starts only after
/// that stream has been submitted; dropping an unsubmitted frame wakes waiters.
pub(super) struct PreparedCapture {
    chunks: Vec<(wgpu::Buffer, Vec<Entry>)>,
    pool: Arc<BufferPool>,
}
impl PreparedCapture {
    fn submitted_device(mut self, _device: &PipelineDevice) -> RasterCapture {
        RasterCapture {
            #[cfg(not(target_arch = "wasm32"))]
            device: _device.for_resource(),
            chunks: self
                .chunks
                .drain(..)
                .map(|(b, e)| Chunk::map(_device,b,e))
                .collect(),
            pool: self.pool.clone(),
        }
    }
}
impl Drop for PreparedCapture {
    fn drop(&mut self) {
        for (_, entries) in &self.chunks {
            for entry in entries {
                let _ = entry
                    .tile
                    .publish(Err("Raster frame was abandoned before submission".into()));
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn wait_mapping(device:&PipelineDevice,ready:&mpsc::Receiver<Result<(),String>>)->Result<(),String> {
    let deadline=std::time::Instant::now()+READBACK_TIMEOUT;
    device.request_poll();
    ready.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())).map_err(|error|match error {
        mpsc::RecvTimeoutError::Disconnected=>"Raster mapping was abandoned".to_owned(),
        mpsc::RecvTimeoutError::Timeout=>"Raster mapping timed out".to_owned(),
    })?
}

/// Maps the first `size` bytes, parses them and unmaps before replying, so a
/// persistent staging buffer stays reusable after a failed mapped-range access.
pub(crate) fn map_then<T: 'static>(
    device:&PipelineDevice,
    encoder: Option<&wgpu::CommandEncoder>,
    buffer: &wgpu::Buffer,
    size: wgpu::BufferAddress,
    parse: impl FnOnce(&[u8]) -> Result<T, GpuRasterError> + Send + 'static,
    reply: impl FnOnce(Result<T, GpuRasterError>) + Send + 'static,
) {
    let ready = buffer.clone();
    let reply = Reply(Some(reply), std::marker::PhantomData);
    let mapped = move |result:Result<(),wgpu::BufferAsyncError>| {
        let parsed = result
            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))
            .and_then(|()| {
                let data = ready
                    .slice(..size)
                    .get_mapped_range()
                    .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?;
                parse(&data)
            });
        ready.unmap();
        reply.send(parsed);
    };
    #[cfg(not(target_arch="wasm32"))]
    let mapped=device.callback(mapped);
    if let Some(encoder)=encoder {encoder.map_buffer_on_submit(buffer,wgpu::MapMode::Read,..size,mapped);}
    else {buffer.slice(..size).map_async(wgpu::MapMode::Read,mapped);device.request_poll();}
}

struct Reply<T,F:FnOnce(Result<T,GpuRasterError>)>(Option<F>,std::marker::PhantomData<fn(T)>);
impl<T,F:FnOnce(Result<T,GpuRasterError>)> Reply<T,F> {
    fn send(mut self,result:Result<T,GpuRasterError>) {if let Some(reply)=self.0.take() {reply(result);}}
}
impl<T,F:FnOnce(Result<T,GpuRasterError>)> Drop for Reply<T,F> {
    fn drop(&mut self) {if let Some(reply)=self.0.take() {reply(Err(GpuRasterError::WaitFailed("GPU device service retired".into())));}}
}

pub(crate) fn unpadded_rows(bytes: &[u8], stride: u32, row_bytes: u32) -> Vec<u8> {
    bytes
        .chunks_exact(stride as usize)
        .flat_map(|row| &row[..row_bytes as usize])
        .copied()
        .collect()
}

pub struct RasterCapture {
    #[cfg(not(target_arch = "wasm32"))]
    device: PipelineDevice,
    chunks: Vec<Chunk>,
    pool: Arc<BufferPool>,
}
impl RasterCapture {
    /// Worker only. Cached readback scratch is bounded to four 16 MiB chunks.
    #[cfg(all(test,not(target_arch="wasm32")))]
    pub fn finish(self) -> Result<(), String> {
        let pool=self.pool.clone();self.finish_recycling(move|buffer|pool.put(buffer))
    }
    #[cfg(not(target_arch="wasm32"))]
    fn finish_recycling(mut self,recycle:impl Fn(wgpu::Buffer)+Sync)->Result<(),String> {
        let result: Result<(), String> = (|| {
            fn finish_chunk(device:&PipelineDevice,chunk:&Chunk,pool:&BufferPool,lanes:usize,recycle:&(impl Fn(wgpu::Buffer)+Sync))->Result<(),String> {
                wait_mapping(device, &chunk.ready)?;
                let mapped = chunk
                    .buffer
                    .slice(..)
                    .get_mapped_range()
                    .map_err(|e| e.to_string())?;
                // Mapped readback memory is expensive for a compressor's repeated
                // accesses. Copy once into cached memory, then return the GPU
                // allocation immediately. At most four 16 MiB chunks exist here.
                let bytes = mapped.to_vec();
                let _scratch = pool.working.charge(bytes.len() as u64);
                drop(mapped);
                chunk.buffer.unmap();
                recycle(chunk.buffer.clone());
                let encode = |entries: &[Entry]| -> Result<(), String> {
                    for entry in entries {
                        if let Some((offset,comparison))=&entry.comparison {comparison.resolve(&bytes[*offset as usize..*offset as usize+crate::native_tiles::comparison::RESULT_BYTES as usize])?;}
                        let begin = entry.offset as usize;
                        entry.tile.publish(TileBlob::encode(
                            entry.descriptor,
                            &bytes[begin..begin + entry.size as usize],
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
                finish_chunk(&self.device,&self.chunks[0],&self.pool,4,&recycle)?;
            } else {
                let group_size = self.chunks.len().div_ceil(4);
                let lanes = 4 / self.chunks.len().min(4);
                std::thread::scope(|scope| {
                    let mut jobs = Vec::new();
                    for group in self.chunks.chunks_mut(group_size) {
                        let pool = &self.pool;
                        let device = &self.device;
                        let recycle=&recycle;
                        jobs.push(scope.spawn(move || {
                            for chunk in group {
                                finish_chunk(device,chunk,pool,lanes,recycle)?;
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
    pub(super) fn raster_restore_backing_ready(&self, packet: FramePacket<'_>) -> bool {
        let runtime = self.raster.as_ref();
        let ready = |current: Option<&Target>, data: &RasterData| {
            data.tiles.iter().all(|(key, tile)| {
                let retained = !packet.reset_layers
                    && current.is_some_and(|t| {
                        !t.contents.modified(key.coordinate)
                            && t.contents.data()
                                .tiles
                                .get(key)
                                .is_some_and(|old| old.same_capture(tile))
                    });
                retained || match tile.try_backing() {
                    None => false,
                    Some(Ok(blob)) => !cfg!(target_arch="wasm32") || !matches!(blob.poll_compressed(),Ok(layer_core::package::RangeState::Pending)),
                    // Submit reports concrete capture/storage errors.
                    Some(Err(_)) => true,
                }
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
        for target in source_access::placed_targets(packet.scene).filter(|t| packet.scene.raster(*t).is_some()) {
            if cfg!(target_arch="wasm32") && packet.scene.original(target).is_some_and(|source| source.tiles.values().any(|tile| matches!(tile.poll_compressed(),Ok(layer_core::package::RangeState::Pending)))) {
                return false;
            }
            for (id, root) in std::iter::once((target, packet.scene.raster(target).unwrap()))
            {
                let current = runtime.and_then(|r| r.targets.get(&id));
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
                            && !ready(Some(current), current.contents.data())
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
    pub(crate) fn browser_raster_encoder(&self) -> Option<BrowserRasterEncoder> {
        self.raster.as_ref().and_then(|r| r.encoder.clone())
    }
    #[cfg(target_arch = "wasm32")]
    pub fn set_browser_raster_encoder(&mut self, encoder: BrowserRasterEncoder) {
        self.raster.get_or_insert_with(Default::default).encoder = Some(encoder);
    }

    pub fn set_raster_preparation_wake(&mut self,wake:Arc<dyn Fn()+Send+Sync>) {
        *self.raster.get_or_insert_with(Default::default).preparation_wake.callback.lock().unwrap()=Some(wake);
    }
    pub(crate) fn raster_name_waker(&self)->Option<std::task::Waker> {
        self.raster.as_ref().map(|runtime|std::task::Waker::from(runtime.preparation_wake.clone()))
    }
    #[cfg(not(target_arch="wasm32"))]
    pub(crate) fn adopt_raster_preparation_wake(&mut self,previous:&Self) {
        if let Some(previous)=&previous.raster {
            self.raster.get_or_insert_with(Default::default).preparation_wake=previous.preparation_wake.clone();
        }
    }
    #[cfg(all(test,not(target_arch="wasm32")))]
    pub(crate) fn raster_publication_pending(&self)->bool {
        self.raster.as_ref().and_then(|runtime|runtime.worker.as_ref())
            .is_some_and(|worker|worker.pending.used()!=0)
    }

    pub fn raster_ready(&self) -> bool {
        self.raster
            .as_ref()
            .and_then(|r| r.worker.as_ref())
            .is_none_or(CaptureWorker::ready)
    }

    pub(super) fn prepare_source_backing(&mut self) -> Result<(), GpuRasterError> {
        self.raster.get_or_insert_with(Default::default).ensure_worker()
    }

    fn restore_target(
        &mut self,
        id: SourceTarget,
        current: &mut Target,
        data: Arc<RasterData>,
        reset: bool,
        damage: &mut Vec<(SourceTarget, PixelRect)>,
    ) -> Result<(), GpuRasterError> {
        let extent = self.target_extent(id);
        damage.extend(current.contents.selection_regions(&data,extent).into_iter().map(|rect|(id,rect)));
        let before=if reset {RasterData::default()}else {current.contents.restorable_data()};
        self.restore_live_raster(id, &before, &data)?;
        current.contents.select(data);
        Ok(())
    }

    pub(super) fn reconcile_rasters(
        &mut self,
        packet: FramePacket<'_>,
        reset: bool,
        _tiles: &[Vec<BrushTile>],
    ) -> Result<Vec<(SourceTarget, PixelRect)>, GpuRasterError> {
        if let Some(error)=self.take_restore_failure() {return Err(error);}
        self.validate_restore_publication()?;
        self.publish_warm_restore_pages()?;
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
            let retained = source_access::retained_targets(packet.scene);
            let count=runtime.targets.len();
            runtime.targets.retain(|id, _| retained.contains_key(id));
            if runtime.targets.len()!=count {runtime.generation+=1;}
            for (&target, scene) in retained.iter().filter(|(t, scene)| scene.raster(**t).is_some()) {
                for (id, revision) in std::iter::once((target, scene.raster(target).unwrap()))
                {
                    // Backgrounds, groups and generators have no editable color pages.
                    if matches!(id, SourceTarget::Paint(_)) && !self.paint_layers.iter().any(|l| l.id == id) {
                        continue;
                    }
                    let wanted = match revision.try_data() {
                        Some(Ok(data)) => Some(data),
                        Some(Err(error)) => return Err(GpuRasterError::Effect(error)),
                        None => None,
                    };
                    let wanted = if wanted.is_none()
                        && let Some((_, restored)) = packet.restore_rasters.iter().find(|(target, _)| *target == id) {
                        Some(restored.try_data().ok_or(GpuRasterError::NotReady)?.map_err(GpuRasterError::Effect)?)
                    } else { wanted };
                    if let Some(current) = runtime.targets.get_mut(&id) {
                        if wanted.is_none() && current.revision!=*revision {current.revision=revision.clone();runtime.generation+=1;}
                        if reset || (wanted.is_some() && (current.revision != *revision || packet.restore_rasters.iter().any(|(target,_)|*target==id))) {
                            let data = wanted.unwrap_or_else(|| current.contents.data().clone());
                            self.restore_target(id, current, data, reset, &mut damage)?;
                            current.revision = revision.clone();runtime.generation+=1;
                        }
                    } else {
                        // A target inserted with pending pixels, such as a
                        // layer copy with an operation, starts from its source.
                        let data = wanted.unwrap_or_default();
                        if !data.tiles.is_empty() {
                            damage.extend(contents::Contents::new(Arc::new(RasterData::default()))
                                .selection_regions(&data,self.target_extent(id)).into_iter().map(|rect|(id,rect)));
                            self.restore_live_raster(id, &RasterData::default(), &data)?;
                        }
                        runtime.generation+=1;
                        runtime.targets.insert(
                            id,
                            Target {
                                revision: revision.clone(),
                                contents: contents::Contents::new(data),
                            },
                        );
                    }
                    runtime.targets.get_mut(&id).unwrap().contents.configure(*scene, id);
                }
            }
            Ok(damage)
        })();
        self.raster = Some(runtime);
        if result.is_ok() {self.consume_restore_preflight();}
        result
    }

    pub(super) fn raster_staging_bytes(&self) -> u64 {
        self.raster_buffers.retained.used()
            + self.raster_buffers.working.used()
            + self.raster_buffers.transfer.used()
    }

    fn raster_inputs(
        &self,target:SourceTarget,submission:Option<&submission::Submission>,include:impl Fn(TileKey)->bool,
    )->Result<(BTreeMap<TileKey,crate::resident::Resident<source_access::RawTile>>,Option<RasterWatercolor>),GpuRasterError> {
        fn alias<T:crate::resident::TextureStorage>(read:crate::resident::Read<'_,T>)
            ->Result<crate::resident::Resident<source_access::RawTile>,GpuRasterError> {
            let pixels=source_access::RawTile {texture:read.value().texture().clone().into(),view:read.value().view().clone()};
            read.alias(pixels).ok_or(GpuRasterError::Invariant("snapshot aliases different physical storage"))
        }
        let mut inputs=BTreeMap::new();let mut watercolor=None;
        if let Some(layer)=self.paint_layers.iter().find(|layer|layer.id==target) {
            for page in &layer.pages {
                let key=TileKey {plane:RasterPlane::Color,coordinate:page.coordinate};if !include(key) {continue;}
                let expected=self.current_raster_version(target,key.plane,key.coordinate)?;
                let read=page.active().read(expected,submission).ok_or(GpuRasterError::NotReady)?;
                inputs.insert(key,alias(read)?);
            }
            for page in &layer.watercolor_wetness_pages {
                let key=TileKey {plane:RasterPlane::WatercolorWetness,coordinate:page.coordinate};if !include(key) {continue;}
                inputs.insert(key,alias(self.read_wetness(target,page,submission)?)?);
            }
            watercolor=layer.watercolor.map(|w|RasterWatercolor {wet_edge:w.wet_edge,burnt_edge:w.burnt_edge,edge_width:w.edge_width});
        }else {
            for ((id,coordinate),page) in &self.layer_masks.pages {
                let key=TileKey {plane:RasterPlane::Mask,coordinate:*coordinate};if *id!=target || !include(key) {continue;}
                let expected=self.current_raster_version(target,key.plane,key.coordinate)?;
                inputs.insert(key,alias(page.read(expected,submission)?)?);
            }
        }
        Ok((inputs,watercolor))
    }

    /// Capture already queue-ordered native candidates through the same bounded
    /// worker as ordinary paint. Validate every input and the actual rounded
    /// staging allocation before recording copies.
    #[cfg(test)]
    pub fn capture_tiles(
        &self,
        copies: &[TileCapture<'_>],
    ) -> Result<RasterCapture, GpuRasterError> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("immutable raster revision capture"),
            });
        let capture=prepare_capture(&self.raster_buffers,&mut encoder,copies,|bytes|self.raster_buffers.take(&self.device,bytes))?;
        self.queue.submit([encoder.finish()]);
        Ok(capture.submitted_device(&self.device))
    }

    #[cfg(not(target_arch="wasm32"))]
    fn restore_colour_batch(&mut self,batch:&mut Vec<(Arc<TileBlob>,layer_core::raster::ContentVersion,LayerPage)>,colors:&mut Vec<LayerPage>)->Result<(),GpuRasterError> {
        if batch.is_empty() {return Ok(());}
        let space=self.document_color().space;
        let inputs=self.prepare_raster_uploads(&batch.iter().map(|(blob,_,_)|(blob.clone(),space,space)).collect::<Vec<_>>())?;
        self.submit_restore(|r,scene,encoder| {
            let receipt=encoder.submission();let mut outputs=Vec::with_capacity(batch.len());
            for (_,name,page) in batch.iter_mut() {
                let write=page.primary.initialize(&receipt).ok_or(GpuRasterError::Invariant("prepared output was already initialized"))?;
                outputs.push((write.value().texture.clone(),write.encoded(name.clone())));
            }
            let requests:[_;crate::native_tiles::MAX_BATCH_TILES]=std::array::from_fn(|index| {
                let index=index.min(batch.len()-1);
                crate::native_tiles::NativeTileRestore {blob:&batch[index].0,space,destination:space,working:&outputs[index].0}
            });
            scene.restore_native_tiles(r,&requests[..batch.len()],&inputs,encoder)?;
            inputs.track(encoder);
            let receipt=encoder.submission();
            for (_,output) in outputs {output.publish(&receipt);}
            Ok(())
        })?;
        colors.extend(batch.drain(..).map(|(_,_,page)|page));Ok(())
    }

    /// Restore changed pages only, from exact backing. Called on the GPU owner
    /// after backing is ready; no historical dabs are generated.
    pub fn restore_raster(
        &mut self,
        target: SourceTarget,
        previous: &RasterData,
        data: &RasterData,
    ) -> Result<(), GpuRasterError> {
        #[cfg(target_arch="wasm32")]
        {self.restore_direct(target,previous,data)}
        #[cfg(not(target_arch="wasm32"))]
        {
        if !self.snapshot_worker {return self.restore_direct(target,previous,data);}
        let _trace = crate::performance_trace::Span::new(c"capy.restore");
        let index = self.paint_layers.iter().position(|l| l.id == target);
        let mask = index.is_none();
        data.validate_storage_index(self.target_extent(target), mask, self.document_color())
            .map_err(GpuRasterError::Effect)?;
        // Stage GPU pages before publishing the revision. Keep only one decoded
        // tile on the CPU, rather than a second full decoded document. A failed
        // tile still leaves every live page (including removed pages) intact.
        let (mut colors, mut masks, mut wetness) = (Vec::new(), Vec::new(), Vec::new());
        let mut native_batch=Vec::new();
        for (key,tile) in &data.tiles {
            if previous.tiles.get(key).is_some_and(|old|old.same_capture(tile)) {continue;}
            let blob=tile.wait_backing().map_err(GpuRasterError::Effect)?;
            if blob.descriptor!=tile.descriptor() {return Err(GpuRasterError::Effect("Raster plane has the wrong pixel representation".into()));}
            match key.plane {
                RasterPlane::Color=> {
                    let space=self.document_color().space;
                    let cached=self.source_tiles.borrow().prepared_raster_view(&blob,space,space).is_some();
                    if !cached {self.restore_colour_batch(&mut native_batch,&mut colors)?;}
                    let mut page=self.create_page(key.coordinate,"restored raster tile");page.primary_needs_clear=false;
                    native_batch.push((blob,tile.content_version(),page));
                    if !cached || native_batch.len()==crate::native_tiles::MAX_BATCH_TILES {self.restore_colour_batch(&mut native_batch,&mut colors)?;}
                },
                RasterPlane::Mask=> {
                    self.restore_colour_batch(&mut native_batch,&mut colors)?;
                    masks.push((key.coordinate,self.restore_mask_page(&blob,tile.content_version())?));
                },
                RasterPlane::WatercolorWetness=> {
                    self.restore_colour_batch(&mut native_batch,&mut colors)?;
                    let mut primary=resident::Resident::unwritten(self.create_scalar_page_surface(&self.raster_tiers.paint,"restored watercolor wetness"));
                    self.submit_restore(|r,scene,encoder| {
                        let receipt=encoder.submission();let write=primary.initialize(&receipt).ok_or(GpuRasterError::Invariant("prepared output was already initialized"))?;
                        scene.restore_native_scalars(r,&[crate::native_tiles::scalar::NativeScalarRestore {blob:&blob,working:&write.value().texture}],encoder)?;
                        write.publish(tile.content_version());Ok(())
                    })?;
                    wetness.push(WatercolorWetnessPage {pending:None,derived:None,coordinate:key.coordinate,primary,
                        secondary:resident::Resident::unwritten(self.create_scalar_page_surface(&self.raster_tiers.paint,"watercolor wetness companion")),
                        active_secondary:false,primary_needs_clear:false,secondary_needs_clear:true});
                },
            }
        }
        self.restore_colour_batch(&mut native_batch,&mut colors)?;
        self.publish_restored_pages(target,data,staging::PreparedPages {colors,masks,wetness,..Default::default()})
        }
    }

    pub(super) fn publish_restored_pages(&mut self,target:SourceTarget,data:&RasterData,pages:staging::PreparedPages)->Result<(),GpuRasterError> {
        let index=self.paint_layers.iter().position(|layer|layer.id==target);
        if index.is_none() && (!pages.colors.is_empty() || !pages.wetness.is_empty()) {return Err(GpuRasterError::MissingPaintLayer(target));}
        if let Some(index) = index {
            let layer = &mut self.paint_layers[index];
            layer.pages.retain(|p| {
                data.tiles.contains_key(&TileKey {
                    plane: RasterPlane::Color,
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
        self.append_prepared_pages(target,pages)?;
        self.clear_preview_pages();
        self.preview_damage = PixelRect::EMPTY;
        self.preview_layer_id = None;
        if let Some(native) = &mut self.native_edit {
            native.backing.insert(target, Arc::new(data.clone()));
        }
        if let Some(scene) = &mut self.scene {
            scene.begin_frame();
        }
        Ok(())
    }
}

#[cfg(test)]
mod native_tests;

#[cfg(test)]
mod restore_tests;

pub(super) mod native_edit;
mod residency;
pub(super) mod staging;

fn prepare_capture(
    pool:&Arc<BufferPool>,
    encoder:&mut wgpu::CommandEncoder,
    copies:&[TileCapture<'_>],
    buffer:impl FnMut(u64)->wgpu::Buffer,
) -> Result<PreparedCapture, GpuRasterError> {
    prepare_capture_compared(pool,encoder,copies,&[],buffer)
}
fn prepare_capture_compared(
    pool:&Arc<BufferPool>,encoder:&mut wgpu::CommandEncoder,copies:&[TileCapture<'_>],
    comparisons:&[Option<crate::native_tiles::comparison::CanonicalReadback>],
    mut buffer:impl FnMut(u64)->wgpu::Buffer,
)->Result<PreparedCapture,GpuRasterError> {
    if !comparisons.is_empty() && comparisons.len()!=copies.len() {return Err(GpuRasterError::InvalidExtent);}
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
    let mut staging_bytes = 0;
    while start < copies.len() {
        let mut end = start;
        let mut bytes = 0;
        while end < copies.len() && bytes + sizes[end] + crate::native_tiles::comparison::RESULT_BYTES*u64::from(comparisons.get(end).is_some_and(Option::is_some)) <= CAPTURE_CHUNK {
            bytes += sizes[end]+crate::native_tiles::comparison::RESULT_BYTES*u64::from(comparisons.get(end).is_some_and(Option::is_some));
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
    let mut chunks = Vec::with_capacity(ranges.len());
    for (range, allocation) in ranges {
        let buffer=buffer(allocation);
        let mut entries = Vec::with_capacity(range.len());
        let mut offset = 0;
        let mut status_offset=range.clone().map(|i|sizes[i]).sum::<u64>();
        for index in range {
            let copy = &copies[index];
            let size = sizes[index];
            match copy.source {
                CaptureSource::Texture(texture) => encoder.copy_texture_to_buffer(
                    texture.as_image_copy(),
                    wgpu::TexelCopyBufferInfo {
                        buffer: &buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset,
                            bytes_per_row: Some((size / u64::from(PAGE_SIZE)) as u32),
                            rows_per_image: Some(PAGE_SIZE),
                        },
                    },
                    texture.size(),
                ),
                CaptureSource::Packed(source) => {
                    encoder.copy_buffer_to_buffer(source, 0, &buffer, offset, size)
                }
            }
            let comparison=comparisons.get(index).cloned().flatten().map(|comparison| {
                let offset=status_offset;comparison.copy(encoder,&buffer,offset);status_offset+=crate::native_tiles::comparison::RESULT_BYTES;(offset,comparison)
            });
            entries.push(Entry {
                #[cfg(not(target_arch = "wasm32"))]
                offset,
                size,
                descriptor: copy.tile.descriptor(),
                tile: copy.tile.clone(),
                comparison,
            });
            offset += size;
        }
        chunks.push((buffer, entries));
    }
    Ok(PreparedCapture {
        chunks,
        pool: pool.clone(),
    })
}

#[cfg(all(test,not(target_arch="wasm32")))]
mod terminal_mapping_tests {
    use super::*;
    #[test]
    fn retirement_abandons_a_registered_map_on_a_valid_device() {
        let r=crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let binding=crate::pipeline_device::DeviceRecipes::from(&r.device).poll_binding();
        r.device.initialize_poll_owner().unwrap();
        let buffer=r.device.create_buffer(&wgpu::BufferDescriptor {label:None,size:4,usage:wgpu::BufferUsages::MAP_READ|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
        let encoder=r.device.create_command_encoder(&Default::default());
        let (send,ready)=mpsc::channel();
        let callback=move|result:Result<(),wgpu::BufferAsyncError>| {let _=send.send(result.map_err(|error|error.to_string()));};
        let callback=r.device.callback(callback);
        encoder.map_buffer_on_submit(&buffer,wgpu::MapMode::Read,..,callback);
        binding.retire_lost(||{});
        let outcome=ready.recv_timeout(std::time::Duration::from_secs(1));
        drop(encoder);
        assert!(matches!(outcome,Err(mpsc::RecvTimeoutError::Disconnected)),"registered map was not abandoned: {outcome:?}");
    }
    #[test]
    fn an_abandoned_readback_delivers_a_terminal_error_to_a_receiver_its_owner_keeps() {
        let r=crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let binding=crate::pipeline_device::DeviceRecipes::from(&r.device).poll_binding();
        r.device.initialize_poll_owner().unwrap();
        let buffer=r.device.create_buffer(&wgpu::BufferDescriptor {label:None,size:4,usage:wgpu::BufferUsages::MAP_READ|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
        let encoder=r.device.create_command_encoder(&Default::default());
        let (send,ready)=mpsc::channel();let retained=send.clone();
        map_then(&r.device,Some(&encoder),&buffer,4,|_|Ok(()),move|result|{let _=send.send(result);});
        binding.retire_lost(||{});
        let outcome=ready.recv_timeout(std::time::Duration::from_secs(1));
        drop(encoder);drop(retained);
        assert!(matches!(outcome,Ok(Err(GpuRasterError::WaitFailed(_)))),"abandoned readback left its request pending: {outcome:?}");
    }
    #[test]
    fn a_readback_completing_while_the_host_waits_wakes_the_host() {
        let r=crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let (wake,woken)=mpsc::channel();
        let buffer=r.device.create_buffer(&wgpu::BufferDescriptor {label:None,size:4,usage:wgpu::BufferUsages::MAP_READ|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
        let mut encoder=r.device.create_command_encoder(&Default::default());encoder.clear_buffer(&buffer,0,None);
        let (send,ready)=mpsc::channel();
        map_then(&r.device,Some(&encoder),&buffer,4,|_|Ok(()),move|result|{let _=send.send(result);});
        r.device.submit(&r.queue,[encoder.finish()]).unwrap();
        let wake:Arc<dyn Fn()+Send+Sync>=Arc::new(move||{let _=wake.send(());});
        loop {
            r.device.retirement_handle().on_next_completion(&wake);
            if let Ok(result)=ready.try_recv() {result.unwrap();break;}
            woken.recv_timeout(std::time::Duration::from_secs(5)).expect("the completed readback never woke the waiting host");
        }
    }
    #[test]
    fn callback_registered_after_retirement_is_dropped_without_running() {
        struct Abandoned(mpsc::Sender<()>);
        impl Drop for Abandoned {fn drop(&mut self) {let _=self.0.send(());}}
        let r=crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let binding=crate::pipeline_device::DeviceRecipes::from(&r.device).poll_binding();
        r.device.initialize_poll_owner().unwrap();binding.retire_lost(||{});
        let (send,ready)=mpsc::channel();let guard=Abandoned(send);
        let (ran,executed)=mpsc::channel();
        let callback=r.device.callback(move|()| {let _=ran.send(());drop(guard);});
        let abandoned=ready.recv_timeout(std::time::Duration::from_secs(1));
        drop(callback);
        assert!(abandoned.is_ok(),"retired callback retained its publisher: {abandoned:?}");
        assert!(executed.try_recv().is_err(),"retired callback ran instead of being abandoned");
    }
}
