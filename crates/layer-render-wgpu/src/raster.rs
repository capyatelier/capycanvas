//! Queue-ordered exact tile capture; mapping/compression runs on a worker.
use super::*;
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

struct Target {
    source: Option<AssetId>,
    revision: RasterRevision,
    data: Arc<RasterData>,
    changed: BTreeSet<[u32; 2]>,
}
#[derive(Default)]
pub(super) struct RasterRuntime {
    targets: BTreeMap<LayerId, Target>,
    worker: Option<CaptureWorker>,
}
struct CaptureWorker {
    sender: Option<mpsc::SyncSender<Vec<RasterCapture>>>,
    pending: Arc<AtomicUsize>,
    thread: Option<std::thread::JoinHandle<()>>,
    staging: Arc<AtomicU64>,
}
impl CaptureWorker {
    fn new() -> Result<Self, GpuRasterError> {
        let (sender, receiver) = mpsc::sync_channel::<Vec<RasterCapture>>(2);
        let pending = Arc::new(AtomicUsize::new(0));
        let count = pending.clone();
        let staging = Arc::new(AtomicU64::new(0));
        let bytes = staging.clone();
        let thread = std::thread::Builder::new()
            .name("capy-raster-backing".into())
            .spawn(move || {
                while let Ok(captures) = receiver.recv() {
                    let size: u64 = captures.iter().map(|c| c.staging_bytes).sum();
                    // Dropped tickets publish failures even if a driver callback or
                    // compression panics; never leave backpressure permanently set.
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        for capture in captures {
                            let _ = capture.finish();
                        }
                    }));
                    bytes.fetch_sub(size, Ordering::Release);
                    count.fetch_sub(1, Ordering::Release);
                }
            })
            .map_err(|e| GpuRasterError::Effect(e.to_string()))?;
        Ok(Self {
            sender: Some(sender),
            pending,
            staging,
            thread: Some(thread),
        })
    }
    fn ready(&self) -> bool {
        self.pending.load(Ordering::Acquire) < 2
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
impl Drop for CaptureWorker {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Entry {
    offset: u64,
    size: u64,
    key: TileKey,
    tile: RasterTile,
}
struct Chunk {
    buffer: wgpu::Buffer,
    entries: Vec<Entry>,
    ready: mpsc::Receiver<Result<(), String>>,
}
pub struct RasterCapture {
    device: wgpu::Device,
    submission: wgpu::SubmissionIndex,
    chunks: Vec<Chunk>,
    pub staging_bytes: u64,
}
impl RasterCapture {
    /// File/recovery worker only. Each decoded temporary is at most one tile.
    pub fn finish(self) -> Result<(), String> {
        let result: Result<(), String> = (|| {
            self.device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(self.submission.clone()),
                    timeout: Some(READBACK_TIMEOUT),
                })
                .map_err(|e| e.to_string())?;
            for chunk in &self.chunks {
                chunk
                    .ready
                    .recv_timeout(READBACK_TIMEOUT)
                    .map_err(|e| e.to_string())??;
                let mapped = chunk
                    .buffer
                    .slice(..)
                    .get_mapped_range()
                    .map_err(|e| e.to_string())?;
                for entry in &chunk.entries {
                    let begin = entry.offset as usize;
                    let blob = TileBlob::encode(
                        entry.key.plane.descriptor(),
                        &mapped[begin..begin + entry.size as usize],
                    );
                    entry.tile.publish(blob)?;
                }
                drop(mapped);
                chunk.buffer.unmap();
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
    pub fn raster_ready(&self) -> bool {
        self.raster
            .as_ref()
            .and_then(|r| r.worker.as_ref())
            .is_none_or(CaptureWorker::ready)
    }

    pub(super) fn reconcile_rasters(
        &mut self,
        packet: FramePacket<'_>,
        reset: bool,
    ) -> Result<(), GpuRasterError> {
        let mut runtime = self.raster.take().unwrap_or_default();
        let result = (|| {
            runtime.targets.retain(|id, _| {
                packet
                    .layers
                    .iter()
                    .any(|l| l.id == *id || l.masks().any(|m| m.id == *id))
            });
            for (id, revision) in packet.restore_rasters {
                if let Some(current) = runtime.targets.get_mut(id) {
                    let data = revision.wait_data().map_err(GpuRasterError::Effect)?;
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
                    let damage = if matches!(batch.kind, DabBatchKind::LayerOperation(_)) {
                        PixelRect::full(packet.document_extent)
                    } else {
                        batch_pixel_rect(batch, packet.document_extent)
                    };
                    target.changed.extend(page_coordinates(damage));
                    if batch.stroke_end && batch.style.rendering.edge_after_stroke {
                        if let Some(layer) =
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
            }
            Ok(())
        })();
        self.raster = Some(runtime);
        result
    }

    pub(super) fn commit_rasters(&mut self, layers: &[Layer]) -> Result<(), GpuRasterError> {
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
                    for key in textures.keys() {
                        if !current.data.tiles.contains_key(key)
                            || current.changed.contains(&key.coordinate)
                        {
                            staging +=
                                key.plane.descriptor().byte_len([PAGE_SIZE; 2]).unwrap() as u64;
                        }
                    }
                }
            }
            if staging > MAX_CAPTURE_BYTES {
                return Err(GpuRasterError::Effect(
                    "Raster frame exceeds the 256 MiB staging budget".into(),
                ));
            }
            if staging > 0 && runtime.worker.is_none() {
                runtime.worker = Some(CaptureWorker::new()?);
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
        self.raster
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
                copies.push((key, texture, tile, size));
            }
        }
        if copies.is_empty() {
            revision.publish(Ok(data)).map_err(GpuRasterError::Effect)?;
            return Ok(None);
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("immutable raster revision capture"),
            });
        let mut chunks = Vec::new();
        let mut index = 0;
        while index < copies.len() {
            let start = index;
            let mut size = 0;
            while index < copies.len() && size + copies[index].3 <= CAPTURE_CHUNK {
                size += copies[index].3;
                index += 1;
            }
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("bounded raster capture chunk"),
                size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut entries = Vec::new();
            let mut offset = 0;
            for (key, texture, tile, count) in &copies[start..index] {
                encoder.copy_texture_to_buffer(
                    texture.as_image_copy(),
                    wgpu::TexelCopyBufferInfo {
                        buffer: &buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset,
                            bytes_per_row: Some((*count / u64::from(PAGE_SIZE)) as u32),
                            rows_per_image: Some(PAGE_SIZE),
                        },
                    },
                    wgpu::Extent3d {
                        width: PAGE_SIZE,
                        height: PAGE_SIZE,
                        depth_or_array_layers: 1,
                    },
                );
                entries.push(Entry {
                    offset,
                    size: *count,
                    key: *key,
                    tile: tile.clone(),
                });
                offset += count;
            }
            chunks.push((buffer, entries));
        }
        let submission = self.queue.submit([encoder.finish()]);
        let chunks = chunks
            .into_iter()
            .map(|(buffer, entries)| {
                let (tx, ready) = mpsc::channel();
                buffer
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        let _ = tx.send(result.map_err(|e| e.to_string()));
                    });
                Chunk {
                    buffer,
                    entries,
                    ready,
                }
            })
            .collect();
        let capture = RasterCapture {
            device: (*self.device).clone(),
            submission,
            chunks,
            staging_bytes: total,
        };
        revision.publish(Ok(data)).map_err(GpuRasterError::Effect)?;
        Ok(Some(capture))
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
        data.validate(self.document_extent, mask)
            .map_err(GpuRasterError::Effect)?;
        // Decode every replacement before mutating live storage; corruption or
        // failed capture cannot leave half a revision installed.
        let mut replacements = Vec::new();
        for (key, tile) in &data.tiles {
            if previous
                .tiles
                .get(key)
                .is_some_and(|old| old.same_capture(tile))
            {
                continue;
            }
            let bytes = tile
                .wait_backing()
                .and_then(|b| b.decode())
                .map_err(GpuRasterError::Effect)?;
            replacements.push((*key, bytes));
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
        for (key, bytes) in replacements {
            let texture = match key.plane {
                RasterPlane::Color => {
                    let mut page = self.create_page(key.coordinate, "restored raster tile");
                    page.primary_needs_clear = false;
                    let texture = page.primary.texture.clone();
                    let pages = &mut self.paint_layers[index.unwrap()].pages;
                    pages.retain(|p| p.coordinate != key.coordinate);
                    pages.push(page);
                    texture
                }
                RasterPlane::Mask => {
                    let page = layer_masks::MaskPage::new(&self.device);
                    let texture = page.texture.clone();
                    self.layer_masks
                        .pages
                        .insert((target, key.coordinate), page);
                    texture
                }
                RasterPlane::Wetness => {
                    let wetness = self.create_scalar_page_surface("restored wetness tile");
                    let texture = wetness.texture.clone();
                    let pages = &mut self.paint_layers[index.unwrap()].material_pages;
                    pages.retain(|p| p.coordinate != key.coordinate);
                    pages.push(CanvasMaterialPage {
                        coordinate: key.coordinate,
                        wetness,
                        needs_clear: false,
                    });
                    texture
                }
                RasterPlane::WatercolorWetness => {
                    let primary = self.create_scalar_page_surface("restored watercolor wetness");
                    let secondary = self.create_scalar_page_surface("watercolor wetness companion");
                    let texture = primary.texture.clone();
                    let pages = &mut self.paint_layers[index.unwrap()].watercolor_wetness_pages;
                    pages.retain(|p| p.coordinate != key.coordinate);
                    pages.push(WatercolorWetnessPage {
                        coordinate: key.coordinate,
                        primary,
                        secondary,
                        active_secondary: false,
                        primary_needs_clear: false,
                        secondary_needs_clear: true,
                    });
                    texture
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
                wgpu::Extent3d {
                    width: PAGE_SIZE,
                    height: PAGE_SIZE,
                    depth_or_array_layers: 1,
                },
            );
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
