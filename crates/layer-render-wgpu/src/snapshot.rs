//! Worker-owned exact document capture. The immutable snapshot shares backing
//! with saving; only the requested region and its dependencies become GPU pixels.
use super::*;
use layer_core::color::source::{SourceBuilder, SourceChannels, SourceInterpretation};
use layer_core::raster::{RasterData, RasterPlane};
use layer_core::{Project, ProjectAssetFormat, ProjectLimits};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

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

pub struct SnapshotRenderer {
    renderer: WgpuRasterizer,
    layers: Vec<Layer>,
    backing: HashMap<LayerId, Arc<RasterData>>,
    resident: HashMap<LayerId, RasterData>,
    extent: [u32; 2],
    background: [f32; 4],
    time: f32,
    limits: CaptureLimits,
    cancelled: Arc<AtomicBool>,
}
impl SnapshotRenderer {
    /// Run on a worker: resolves pending immutable raster backing and prepares a
    /// native Float32 device. No full composite or full paint image is created.
    pub fn new(
        project: Project,
        background: [f32; 4],
        time: f32,
        limits: CaptureLimits,
    ) -> Result<Self, GpuRasterError> {
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
                backing.insert(id, raster.wait_data().map_err(GpuRasterError::Color)?);
            }
        }
        let mut renderer = WgpuRasterizer::new_native_headless(project.document.color)?;
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
            background,
            time,
            limits,
            cancelled: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn extent(&self) -> [u32; 2] {
        self.extent
    }
    pub fn color(&self) -> layer_core::color::DocumentColor {
        self.renderer.document_color()
    }
    pub fn cancellation(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }
    fn check_cancelled(&self) -> Result<(), GpuRasterError> {
        if self.cancelled.load(Ordering::Relaxed) {
            Err(GpuRasterError::Color("Snapshot capture cancelled".into()))
        } else {
            Ok(())
        }
    }

    /// Exact linear-premultiplied document RGB. No display conversion, proof,
    /// mask-area tint, checkerboard or UI overlays participate. Waits on this
    /// capture only; call from the owning file/inspection worker.
    pub fn read_region(
        &mut self,
        [x, y, width, height]: [u32; 4],
    ) -> Result<Vec<[f32; 4]>, GpuRasterError> {
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
            return Err(GpuRasterError::Color(format!(
                "Snapshot dependency plan requires {planned} bytes; limit is {}",
                self.limits.planned_pixel_bytes
            )));
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
            if self.cancelled.load(Ordering::Relaxed) {
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
        r.last_style_base = 0;
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
        let submission = encoder.submit(&r.queue);
        let (tx, rx) = mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        r.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(READBACK_TIMEOUT),
            })
            .map_err(|e| GpuRasterError::WaitFailed(e.to_string()))?;
        rx.recv()
            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?
            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?;
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
        r.refresh_storage_metrics();
        self.check_cancelled()?;
        Ok(pixels)
    }

    /// Profiled output writes a copy. Callers publish their temporary file only
    /// after success; cancellation, codec or capture failure may leave it partial.
    pub fn write_png(
        &mut self,
        output: impl std::io::Write,
        target: &SourceInterpretation,
        options: layer_core::color::ConversionOptions,
        matte: Option<[f32; 3]>,
    ) -> Result<layer_color::OutputStatistics, String> {
        self.write_rows(target, options, matte, |extent, target, row| {
            layer_color::photo::write_png_rows(output, extent, target, row)
        })
    }
    pub fn write_tiff(
        &mut self,
        output: impl std::io::Write + std::io::Seek,
        target: &SourceInterpretation,
        options: layer_core::color::ConversionOptions,
        matte: Option<[f32; 3]>,
    ) -> Result<layer_color::OutputStatistics, String> {
        self.write_rows(target, options, matte, |extent, target, row| {
            layer_color::photo::write_tiff_rows(output, extent, target, row)
        })
    }
    fn identity_source(
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
    fn write_rows(
        &mut self,
        target: &SourceInterpretation,
        options: layer_core::color::ConversionOptions,
        matte: Option<[f32; 3]>,
        write: impl FnOnce(
            [u32; 2],
            &SourceInterpretation,
            &mut dyn FnMut(u32, &mut [u8]) -> Result<(), String>,
        ) -> Result<(), String>,
    ) -> Result<layer_color::OutputStatistics, String> {
        self.check_cancelled().map_err(|e| e.to_string())?;
        let encoder = layer_color::WorkingEncoder::new(self.color().space, target, options)?;
        let extent = self.extent;
        if options == Default::default()
            && matte.is_none()
            && let Some(source) = self.identity_source(target)
        {
            // Preserve exact integer samples, including hidden straight RGB,
            // when delivery does not require compositing or color conversion.
            let mut rows = source.rows();
            write(extent, encoder.interpretation(), &mut |y, row| {
                self.check_cancelled().map_err(|e| e.to_string())?;
                rows.read(y, row)
            })?;
            return Ok(Default::default());
        }
        let mut band = Vec::new();
        let mut first = 0;
        let mut end = 0;
        let mut stats = layer_color::OutputStatistics::default();
        write(extent, encoder.interpretation(), &mut |y, row| {
            self.check_cancelled().map_err(|e| e.to_string())?;
            if y >= end {
                // A small strip bounds output and host mappings independently
                // of photo height. Filter support determines the input window.
                band = Vec::new();
                first = y;
                end = (y + 16).min(extent[1]);
                band = self
                    .read_region([0, y, extent[0], end - y])
                    .map_err(|e| e.to_string())?;
            }
            let start = (y - first) as usize * extent[0] as usize;
            stats.clipped_channels += encoder
                .encode_premultiplied(&band[start..start + extent[0] as usize], row, matte)?
                .clipped_channels;
            Ok(())
        })?;
        Ok(stats)
    }
}

#[cfg(test)]
mod tests;
