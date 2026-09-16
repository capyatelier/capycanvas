//! Worker-owned exact document capture. The immutable snapshot shares backing
//! with saving; only the requested region and its dependencies become GPU pixels.
use super::*;
use layer_core::color::source::{SourceBuilder, SourceChannels, SourceInterpretation};
use layer_core::raster::{RasterData, RasterPlane};
use layer_core::{Project, ProjectAssetFormat, ProjectLimits};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Conservative request planning ceiling, separate from codec/output buffers,
/// retained compressed sources and driver/pipeline memory. This is not a device
/// memory qualification; hosts must also enforce their measured process budget.
#[derive(Clone, Copy, Debug)]
pub struct CaptureLimits {
    pub planned_pixel_bytes: u64,
}
impl Default for CaptureLimits {
    fn default() -> Self {
        Self {
            planned_pixel_bytes: 512 * 1024 * 1024,
        }
    }
}

/// Share before worker initialization so cancellation also applies to setup.
#[derive(Clone, Default)]
pub struct CaptureControl {
    cancelled: Arc<AtomicBool>,
    output_rows: Arc<AtomicU32>,
    allocation_peaks: Option<Arc<std::sync::Mutex<CaptureAllocationPeaks>>>,
}
/// Allocator observations at capture allocation boundaries, including readback
/// staging. Driver-private memory and retained CPU sources are separate charges.
/// With `SnapshotGpu`, these cover the entire shared device; do not add its live
/// canvas allocations a second time when computing an aggregate process bound.
#[derive(Clone, Copy, Debug, Default)]
pub struct CaptureAllocationPeaks {
    pub observations: u64,
    pub allocated_bytes: u64,
    pub reserved_bytes: u64,
}
impl CaptureControl {
    /// Opt-in diagnostics; ordinary capture does not query the GPU allocator.
    pub fn with_allocation_tracking() -> Self {
        Self {
            allocation_peaks: Some(Default::default()),
            ..Self::default()
        }
    }
    pub fn allocation_peaks(&self) -> Option<CaptureAllocationPeaks> {
        self.allocation_peaks.as_ref().map(|p| *p.lock().unwrap())
    }
    fn observe_allocations(&self, device: &wgpu::Device) {
        if let Some(peaks) = &self.allocation_peaks
            && let Some(report) = device.generate_allocator_report()
        {
            let mut peaks = peaks.lock().unwrap();
            peaks.observations += 1;
            peaks.allocated_bytes = peaks.allocated_bytes.max(report.total_allocated_bytes);
            peaks.reserved_bytes = peaks.reserved_bytes.max(report.total_reserved_bytes);
        }
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
    pub fn output_rows(&self) -> u32 {
        self.output_rows.load(Ordering::Relaxed)
    }
    fn check(&self) -> Result<(), GpuRasterError> {
        if self.is_cancelled() {
            Err(GpuRasterError::Color("Snapshot capture cancelled".into()))
        } else {
            Ok(())
        }
    }
}

/// Sendable device ownership for a snapshot worker. Shares the live device and
/// pipeline cache, never its mutable paint pages, scene buffers or input state.
#[derive(Clone)]
pub struct SnapshotGpu {
    #[cfg(target_arch = "wasm32")]
    encoder: Option<raster::BrowserRasterEncoder>,
    adapter: wgpu::Adapter,
    device: PipelineDevice,
    queue: wgpu::Queue,
}
impl WgpuRasterizer {
    /// CPU exact source samples shared with this canvas's snapshot workers.
    /// These allocations are separate from GPU allocator reports.
    pub fn source_sample_cache_stats(&self) -> layer_core::raster::DecodedTileCacheStats {
        self.device.source_samples.stats()
    }
    pub fn snapshot_gpu(&self) -> SnapshotGpu {
        SnapshotGpu {
            #[cfg(target_arch = "wasm32")]
            encoder: self.browser_raster_encoder(),
            adapter: self.adapter.clone(),
            device: self.device.clone(),
            queue: self.queue.clone(),
        }
    }
}
impl SnapshotGpu {
    /// Run on the file/inspection worker. Cloned handles keep the device alive
    /// through this job even if its canvas closes; loss still fails the job.
    pub fn capture(
        &self,
        project: Project,
        background: [f32; 4],
        time: f32,
        limits: CaptureLimits,
        control: CaptureControl,
    ) -> Result<SnapshotRenderer, GpuRasterError> {
        SnapshotRenderer::construct(project, background, time, limits, control, Some(self))
    }
}

pub struct SnapshotRenderer {
    renderer: WgpuRasterizer,
    layers: Vec<Layer>,
    backing: HashMap<LayerId, Arc<RasterData>>,
    resident: HashMap<LayerId, RasterData>,
    extent: [u32; 2],
    #[cfg(not(target_arch = "wasm32"))]
    output_extent: [u32; 2],
    #[cfg(not(target_arch = "wasm32"))]
    output_resolution: Option<layer_core::ImageResolution>,
    background: [f32; 4],
    time: f32,
    limits: CaptureLimits,
    control: CaptureControl,
}
impl SnapshotRenderer {
    /// Run on a worker: resolves pending immutable raster backing and prepares a
    /// native Float32 device. No full composite or full paint image is created.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(
        project: Project,
        background: [f32; 4],
        time: f32,
        limits: CaptureLimits,
    ) -> Result<Self, GpuRasterError> {
        Self::with_control(project, background, time, limits, CaptureControl::default())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_control(
        project: Project,
        background: [f32; 4],
        time: f32,
        limits: CaptureLimits,
        control: CaptureControl,
    ) -> Result<Self, GpuRasterError> {
        Self::construct(project, background, time, limits, control, None)
    }
    fn construct(
        project: Project,
        background: [f32; 4],
        time: f32,
        limits: CaptureLimits,
        control: CaptureControl,
        gpu: Option<&SnapshotGpu>,
    ) -> Result<Self, GpuRasterError> {
        control.check()?;
        project
            .validate(ProjectLimits::default())
            .map_err(GpuRasterError::Color)?;
        if background.iter().any(|v| !v.is_finite())
            || !(0.0..=1.).contains(&background[3])
            || !time.is_finite()
        {
            return Err(GpuRasterError::Color(
                "Invalid snapshot viewing state".into(),
            ));
        }
        let extent = [project.document.width, project.document.height];
        let mut layers: Vec<_> = project
            .document
            .layers
            .iter()
            .map(Layer::composite_snapshot)
            .collect();
        let mut sources = HashMap::new();
        // Retire packed legacy image upload for this consumer. Both retained
        // originals and legacy project images use the same tiled source decoder.
        for layer in &mut layers {
            control.check()?;
            if layer.source.is_some() {
                continue;
            }
            let Some(asset_id) = layer.asset.take() else {
                continue;
            };
            if !sources.contains_key(&asset_id) {
                let asset = project
                    .assets
                    .get(&asset_id)
                    .ok_or(GpuRasterError::InvalidImage)?;
                if asset.format != ProjectAssetFormat::Rgba8Srgb {
                    return Err(GpuRasterError::InvalidImage);
                }
                let mut builder = SourceBuilder::new(
                    asset.extent,
                    SourceInterpretation {
                        channels: SourceChannels::Rgba,
                        depth: layer_core::color::IntegerDepth::U8,
                        profile: Default::default(),
                        profile_assumed: false,
                    },
                    ProjectLimits::default().asset_bytes as usize,
                )
                .map_err(GpuRasterError::Color)?;
                for row in asset.bytes.chunks_exact(asset.extent[0] as usize * 4) {
                    control.check()?;
                    builder.push_row(row).map_err(GpuRasterError::Color)?;
                }
                sources.insert(
                    asset_id.clone(),
                    Arc::new(builder.finish().map_err(GpuRasterError::Color)?),
                );
            }
            layer.source = sources.get(&asset_id).cloned();
        }
        let mut backing = HashMap::new();
        for layer in &layers {
            for (id, raster) in std::iter::once((layer.id, &layer.raster))
                .chain(layer.masks().map(|m| (m.id, &m.raster)))
            {
                control.check()?;
                backing.insert(id, raster.wait_data().map_err(GpuRasterError::Color)?);
            }
        }
        control.check()?;
        let mut renderer = match gpu {
            Some(gpu) => WgpuRasterizer::native_capture_on_gpu(
                gpu.adapter.clone(),
                gpu.device.clone(),
                gpu.queue.clone(),
                project.document.color,
            )?,
            #[cfg(not(target_arch = "wasm32"))]
            None => WgpuRasterizer::new_native_capture(project.document.color)?,
            #[cfg(target_arch = "wasm32")]
            None => {
                return Err(GpuRasterError::Color(
                    "Browser capture requires the canvas device".into(),
                ));
            }
        };
        #[cfg(target_arch = "wasm32")]
        if let Some(encoder) = gpu.and_then(|gpu| gpu.encoder.clone()) {
            renderer.set_browser_raster_encoder(encoder);
        }
        renderer.ensure_document_metadata(extent, &layers)?;
        let mut background = background;
        if let Some(paper) = layers.iter().find(|l| l.kind == LayerKind::Background) {
            background[3] *= if paper.visible { paper.opacity } else { 0. };
        }
        Ok(Self {
            renderer,
            layers,
            backing,
            resident: HashMap::new(),
            extent,
            #[cfg(not(target_arch = "wasm32"))]
            output_extent: extent,
            #[cfg(not(target_arch = "wasm32"))]
            output_resolution: project.document.resolution,
            background,
            time,
            limits,
            control,
        })
    }

    pub fn identity_source(
        &self,
        target: &SourceInterpretation,
    ) -> Option<Arc<layer_core::color::source::SourceImage>> {
        if self.background[3] != 0. {
            return None;
        }
        let mut visible = self
            .layers
            .iter()
            .filter(|l| l.visible && l.opacity > 0. && l.kind != LayerKind::Background);
        let layer = visible.next()?;
        if visible.next().is_some()
            || layer.kind != LayerKind::Paint
            || layer.opacity != 1.
            || layer.properties.parent.is_some()
            || layer.properties.offset != layer_core::Point::default()
            || layer.properties.blend != layer_core::LayerBlend::Normal
            || layer.properties.clipped
            || layer.mask.as_ref().is_some_and(|m| m.enabled)
            || layer.effect.is_some()
            || !self.backing[&layer.id].tiles.is_empty()
        {
            return None;
        }
        let source = layer.source.as_ref()?;
        (source.extent == self.extent
            && source.interpretation.channels == target.channels
            && source.interpretation.depth == target.depth
            && source.interpretation.profile == target.profile)
            .then(|| source.clone())
    }

    pub fn extent(&self) -> [u32; 2] {
        self.extent
    }
    pub fn color(&self) -> layer_core::color::DocumentColor {
        self.renderer.document_color()
    }
    pub fn control(&self) -> CaptureControl {
        self.control.clone()
    }
    /// Full-resolution committed composite, including visible paper but never
    /// checkerboard, proof, monitor conversion, selection or warning overlays.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn histogram(&mut self) -> Result<layer_core::color::histogram::Histogram, GpuRasterError> {
        let mut result = layer_core::color::histogram::Histogram::new(self.color());
        let mut y = 0;
        while y < self.extent[1] {
            self.check_cancelled()?;
            let (height, pixels) = self.read_band(y)?;
            result
                .add(&pixels)
                .map_err(|e| GpuRasterError::Color(e.into()))?;
            y += height;
        }
        self.check_cancelled()?;
        Ok(result)
    }
    #[cfg(target_arch = "wasm32")]
    pub async fn histogram_async(
        &mut self,
    ) -> Result<layer_core::color::histogram::Histogram, GpuRasterError> {
        let mut result = layer_core::color::histogram::Histogram::new(self.color());
        let mut y = 0;
        while y < self.extent[1] {
            self.check_cancelled()?;
            let (height, pixels) = self.read_band_async(y).await?;
            result
                .add(&pixels)
                .map_err(|e| GpuRasterError::Color(e.into()))?;
            y += height;
        }
        self.check_cancelled()?;
        Ok(result)
    }
    #[cfg(target_arch = "wasm32")]
    pub async fn preview_document_async(
        &mut self,
        bounds: [u32; 2],
        space: layer_core::color::RgbSpace,
    ) -> Result<SnapshotPreview, GpuRasterError> {
        let mut preview =
            layer_color::AreaPreview::new(self.extent, bounds).map_err(GpuRasterError::Color)?;
        let mut y = 0;
        while y < self.extent[1] {
            self.check_cancelled()?;
            let (height, pixels) = self.read_band_async(y).await?;
            for row in pixels.chunks_exact(self.extent[0] as usize) {
                preview.push(row).map_err(GpuRasterError::Color)?;
            }
            y += height;
        }
        let (extent, mut pixels) = preview.finish().map_err(GpuRasterError::Color)?;
        let matrix = self.color().space.linear_transform(space);
        for pixel in &mut pixels {
            let rgb = layer_core::color::rgb::apply(
                matrix,
                [pixel[0], pixel[1], pixel[2]].map(f64::from),
            );
            pixel[..3].copy_from_slice(&rgb.map(|v| v as f32));
        }
        self.check_cancelled()?;
        Ok(SnapshotPreview {
            extent,
            space,
            pixels,
        })
    }
    fn check_cancelled(&self) -> Result<(), GpuRasterError> {
        self.control.check()
    }

    /// Capture complete tile rows when the dependency budget permits. Sixteen
    /// separate captures of one tile row repeat composition, source decoding and
    /// mapping. The CPU band is at most 32 MiB; planning includes its GPU target
    /// and readback copy. Complex dependencies shrink the band before GPU work.
    #[cfg(not(target_arch = "wasm32"))]
    fn read_band(&mut self, y: u32) -> Result<(u32, Vec<[f32; 4]>), GpuRasterError> {
        let [width, height] = self.extent;
        if y >= height {
            return Err(GpuRasterError::InvalidExtent);
        }
        let maximum = (32 * 1024 * 1024 / (width * 16)).clamp(16, PAGE_SIZE);
        let mut rows = maximum.min(height - y);
        loop {
            match self.read_region([0, y, width, rows]) {
                Ok(pixels) => return Ok((rows, pixels)),
                Err(GpuRasterError::CaptureBudget { .. }) if rows > 16 => {
                    rows = (rows / 2).max(16);
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Exact linear-premultiplied document RGB. No display conversion, proof,
    /// mask-area tint, checkerboard or UI overlays participate. Waits on this
    /// capture only; call from the owning file/inspection worker.
    fn prepare_region(
        &mut self,
        [x, y, width, height]: [u32; 4],
    ) -> Result<RegionReadback, GpuRasterError> {
        self.check_cancelled()?;
        let region = PixelRect::new(
            x,
            y,
            x.checked_add(width).ok_or(GpuRasterError::SizeOverflow)?,
            y.checked_add(height).ok_or(GpuRasterError::SizeOverflow)?,
        );
        if region.is_empty() || region.intersect(PixelRect::full(self.extent)) != region {
            return Err(GpuRasterError::InvalidExtent);
        }
        let window = scene::Scene::capture_window(&self.layers, region, self.extent);
        // Composition operates in page-sized tiles, including translated masks
        // and neighboring watercolor pigment. Restore their complete footprints.
        let pages = page_coordinates(window)
            .fold(PixelRect::EMPTY, |r, c| r.union(page_rect(c)))
            .intersect(PixelRect::full(self.extent));
        let mut selected = HashMap::new();
        let mut masks = HashMap::new();
        let mut planned = scene::Scene::capture_image_bound(&self.layers, window)
            .saturating_add(region.area().saturating_mul(32)) // output and mapping
            .saturating_add((self.layers.len() as u64 * 3 + 32) * 256 * 256 * 16);
        for layer in &self.layers {
            for (id, mask) in
                std::iter::once((layer.id, false)).chain(layer.masks().map(|m| (m.id, true)))
            {
                let offset = scene::world_offset(&self.layers, layer.id, mask);
                let local = pixel_rect(
                    layer_core::Rect {
                        min: layer_core::Point {
                            x: pages.min_x() as f32 - offset.x,
                            y: pages.min_y() as f32 - offset.y,
                        },
                        max: layer_core::Point {
                            x: pages.max_x() as f32 - offset.x,
                            y: pages.max_y() as f32 - offset.y,
                        },
                    },
                    self.extent,
                )
                .expand(if mask { 1 } else { PAGE_SIZE }, self.extent);
                if mask {
                    masks.insert(id, local);
                    if layer.mask.as_ref().is_some_and(|m| m.initial.is_some()) {
                        planned = planned
                            .saturating_add(self.extent[0] as u64 * self.extent[1] as u64 / 2 + 64);
                        planned = planned
                            .saturating_add(page_coordinates(local).count() as u64 * 256 * 256 * 5);
                    }
                }
                let original = &self.backing[&id];
                let data = RasterData {
                    watercolor: original.watercolor,
                    tiles: original
                        .tiles
                        .iter()
                        .filter(|(key, _)| !page_rect(key.coordinate).intersect(local).is_empty())
                        .map(|(k, t)| (*k, t.clone()))
                        .collect(),
                };
                for key in data.tiles.keys() {
                    planned = planned.saturating_add(
                        256 * 256
                            * match key.plane {
                                RasterPlane::Color => 32,
                                RasterPlane::WatercolorWetness => 16,
                                _ => 8,
                            },
                    );
                }
                selected.insert(id, data);
            }
        }
        if planned > self.limits.planned_pixel_bytes {
            return Err(GpuRasterError::CaptureBudget {
                required: planned,
                limit: self.limits.planned_pixel_bytes,
            });
        }
        let r = &mut self.renderer;
        if let Some(scene) = &mut r.scene {
            scene.release_capture_window(window);
        }
        // These are disposable read-only caches. Evict obsolete pages before
        // allocating replacements, instead of temporarily retaining both windows.
        let retained = |id: LayerId, plane, coordinate| {
            selected.get(&id).is_some_and(|data| {
                data.tiles
                    .contains_key(&layer_core::raster::TileKey { plane, coordinate })
            })
        };
        for layer in &mut r.paint_layers {
            layer
                .pages
                .retain(|p| retained(layer.id, RasterPlane::Color, p.coordinate));
            layer
                .material_pages
                .retain(|p| retained(layer.id, RasterPlane::Wetness, p.coordinate));
            layer
                .watercolor_wetness_pages
                .retain(|p| retained(layer.id, RasterPlane::WatercolorWetness, p.coordinate));
        }
        r.layer_masks
            .pages
            .retain(|(id, coordinate), _| retained(*id, RasterPlane::Mask, *coordinate));
        for (id, data) in &mut self.resident {
            data.tiles
                .retain(|key, _| retained(*id, key.plane, key.coordinate));
        }
        // A completed earlier capture must not pin a larger selection buffer.
        // Mask pages retain their pixels independently of this staging buffer.
        r.selection_clip.reset();
        for (id, data) in &selected {
            if self.control.is_cancelled() {
                return Err(GpuRasterError::Color("Snapshot capture cancelled".into()));
            }
            if self
                .layers
                .iter()
                .any(|l| l.id == *id && l.kind != LayerKind::Paint)
            {
                continue;
            }
            r.restore_raster(
                *id,
                self.resident.get(id).unwrap_or(&RasterData::default()),
                data,
            )?;
            // Restoration is atomic per target. Keep the cache index current
            // even if a later target fails or this worker is cancelled.
            self.resident.insert(*id, data.clone());
        }
        self.resident = selected;
        let packet = FramePacket {
            view: layer_render::ViewState {
                width_px: width,
                height_px: height,
                document_to_surface: [1., 0., 0., 1., 0., 0.],
                background_rgba_linear: self.background,
            },
            document_extent: self.extent,
            layers: &self.layers,
            time_seconds: self.time,
            dabs: &[],
            dab_batches: &[],
            restore_rasters: &[],
            reset_layers: false,
            composite_all: true,
        };
        let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
        r.prepare_uploads(packet, true, &mut encoder)?;
        r.layer_masks.prepare_regions(
            &r.device,
            &mut encoder,
            (&self.layers, &[]),
            self.extent,
            false,
            &mut r.selection_clip,
            Some(&masks),
        )?;
        let (target, _) = create_color_target(&r.device, [width, height], "snapshot region");
        let mut scene = r.scene.take().unwrap_or_else(|| scene::Scene::new(r));
        let captured = scene.capture_region(r, packet, &target, region, None, &mut encoder);
        r.scene = Some(scene);
        captured?;
        let stride = (width * 16).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let size = stride as u64 * height as u64;
        let buffer = r.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("snapshot region readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: None,
                },
            },
            target.size(),
        );
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        self.control.observe_allocations(&r.device);
        Ok(RegionReadback {
            buffer,
            stride,
            width,
            height,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_region(&mut self, region: [u32; 4]) -> Result<Vec<[f32; 4]>, GpuRasterError> {
        let readback = self.prepare_region(region)?;
        let (tx, rx) = mpsc::channel();
        readback
            .buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result.map_err(|e| e.to_string()));
            });
        crate::raster::wait_mapping(&self.renderer.device, &rx)
            .map_err(GpuRasterError::MapFailed)?;
        self.finish_region(readback)
    }

    /// Yields to WebGPU while mapping one bounded Float32 region. Callers await
    /// raster backing before creating the immutable capture, and await each
    /// region before submitting the next; no blocking browser device poll.
    #[cfg(target_arch = "wasm32")]
    pub async fn read_region_async(
        &mut self,
        region: [u32; 4],
    ) -> Result<Vec<[f32; 4]>, GpuRasterError> {
        let readback = self.prepare_region(region)?;
        let (tx, rx) = futures_channel::oneshot::channel();
        readback
            .buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result.map_err(|e| e.to_string()));
            });
        rx.await
            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?
            .map_err(GpuRasterError::MapFailed)?;
        self.finish_region(readback)
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn read_band_async(
        &mut self,
        y: u32,
    ) -> Result<(u32, Vec<[f32; 4]>), GpuRasterError> {
        let [width, height] = self.extent;
        if y >= height {
            return Err(GpuRasterError::InvalidExtent);
        }
        let maximum = (32 * 1024 * 1024 / (width * 16)).clamp(16, PAGE_SIZE);
        let mut rows = maximum.min(height - y);
        loop {
            match self.read_region_async([0, y, width, rows]).await {
                Ok(pixels) => return Ok((rows, pixels)),
                Err(GpuRasterError::CaptureBudget { .. }) if rows > 16 => rows = (rows / 2).max(16),
                Err(error) => return Err(error),
            }
        }
    }

    fn finish_region(&mut self, readback: RegionReadback) -> Result<Vec<[f32; 4]>, GpuRasterError> {
        let RegionReadback {
            buffer,
            stride,
            width,
            height,
        } = readback;
        let bytes = buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?;
        let mut pixels = Vec::with_capacity(width as usize * height as usize);
        for row in bytes.chunks_exact(stride as usize) {
            for pixel in row[..width as usize * 16].chunks_exact(16) {
                pixels.push(std::array::from_fn(|c| {
                    f32::from_le_bytes(pixel[c * 4..c * 4 + 4].try_into().unwrap())
                }));
            }
        }
        drop(bytes);
        buffer.unmap();
        self.renderer.refresh_storage_metrics();
        self.check_cancelled()?;
        Ok(pixels)
    }
}

struct RegionReadback {
    buffer: wgpu::Buffer,
    stride: u32,
    width: u32,
    height: u32,
}

#[cfg(not(target_arch = "wasm32"))]
mod output;
#[cfg(not(target_arch = "wasm32"))]
mod preview;
/// Linear premultiplied viewing pixels. Hosts apply their view-only checkerboard
/// and encode/tag the presentation texture in this explicitly declared space.
pub struct SnapshotPreview {
    pub extent: [u32; 2],
    pub space: layer_core::color::RgbSpace,
    pub pixels: Vec<[f32; 4]>,
}

#[cfg(test)]
mod tests;

mod color_candidate;
pub use color_candidate::ColorCanvas;

impl SnapshotPreview {
    /// Bounded UI transport only; premultiplied extended working values never
    /// pass through this 8-bit presentation conversion during editing or export.
    pub fn srgb_bytes(&self) -> Result<Vec<u8>, String> {
        let target = SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: layer_core::color::IntegerDepth::U8,
            profile: layer_core::color::ColorProfile::Builtin(layer_core::color::RgbSpace::Srgb),
            profile_assumed: false,
        };
        let encoder = layer_color::WorkingEncoder::new(self.space, &target, Default::default())?;
        let mut bytes = vec![0; self.pixels.len() * 4];
        encoder.encode_premultiplied(&self.pixels, &mut bytes, None, [0, 0])?;
        Ok(bytes)
    }
}
