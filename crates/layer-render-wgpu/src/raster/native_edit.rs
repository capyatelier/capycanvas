//! Native commit ownership for the Float32 renderer. Each frame validates every
//! changed page, then quantizes, promotes and copies batches through fixed scratch.
//! Capture mapping and history publication start only after frame submission.
use super::*;
use crate::native_tiles::{
    MAX_BATCH_TILES, NativeTileEncoder, NativeTileRequest, NativeTransfer,
    promote::{NativePromoter, NativePromotion},
    scalar::{NativeScalarEncoder, NativeScalarRequest},
};
use layer_core::color::DocumentColor;
mod validate;

struct ColorSlot {
    encoded: wgpu::Texture,
    canonical: wgpu::Texture,
}
struct ScalarSlot {
    encoded: wgpu::Buffer,
    canonical: wgpu::Texture,
}
pub(crate) struct NativeEdit {
    pub(super) backing: BTreeMap<LayerId, Arc<RasterData>>,
    pub(crate) color_cache_bytes: u64,
    pub(crate) display_dense_bytes: u64,
    pub(crate) display_cache_bytes: u64,
    /// Provisional ceiling for live physical-filter pixel allocations, separate
    /// from source, paint and composite residency. Qualify the host budget before
    /// enabling native photo documents in GTK.
    pub image_pixel_bytes: u64,
    transfer: NativeTransfer,
    color: NativeTileEncoder,
    scalar: NativeScalarEncoder,
    promoter: NativePromoter,
    validator: validate::Validator,
    status: NativeEncodeStatus,
    colors: Vec<ColorSlot>,
    scalars: Vec<ScalarSlot>,
}
impl NativeEdit {
    fn new(r: &WgpuRasterizer, transfer: NativeTransfer) -> Self {
        let color = r.document_color();
        let texture = |format| {
            r.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("bounded native commit scratch"),
                size: wgpu::Extent3d {
                    width: 256,
                    height: 256,
                    depth_or_array_layers: 1,
                },
                dimension: wgpu::TextureDimension::D2,
                format,
                mip_level_count: 1,
                sample_count: 1,
                usage: wgpu::TextureUsages::STORAGE_BINDING
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        };
        let colors = (0..MAX_BATCH_TILES)
            .map(|_| ColorSlot {
                encoded: texture(if color.depth.bits() == 16 {
                    wgpu::TextureFormat::Rgba16Uint
                } else {
                    wgpu::TextureFormat::Rgba8Uint
                }),
                canonical: texture(wgpu::TextureFormat::Rgba32Float),
            })
            .collect();
        let scalars = (0..MAX_BATCH_TILES)
            .map(|_| ScalarSlot {
                encoded: r.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("bounded native coverage scratch"),
                    size: 65536 * color.depth.bytes() as u64,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }),
                canonical: texture(wgpu::TextureFormat::R32Float),
            })
            .collect();
        Self {
            backing: BTreeMap::new(),
            color_cache_bytes: 256 * 1024 * 1024,
            display_dense_bytes: crate::live_display::DENSE_BYTES,
            display_cache_bytes: crate::live_display::CACHE_BYTES,
            image_pixel_bytes: crate::scene::windows::DEFAULT_IMAGE_PIXEL_BYTES,
            transfer,
            colors,
            scalars,
            color: NativeTileEncoder::with_device(&r.device),
            scalar: NativeScalarEncoder::with_device(&r.device),
            promoter: NativePromoter::with_device(&r.device),
            validator: validate::Validator::new(&r.device),
            status: NativeEncodeStatus::new(&r.device),
        }
    }
    pub fn storage_bytes(&self) -> u64 {
        STATUS_BYTES
            + self
                .colors
                .iter()
                .map(|s| texture_bytes(&s.encoded) + texture_bytes(&s.canonical))
                .sum::<u64>()
            + self
                .scalars
                .iter()
                .map(|s| s.encoded.size() + texture_bytes(&s.canonical))
                .sum::<u64>()
        // Transfer storage is owned/accounted by the shared scene decoder cache.
    }
}

struct Publication {
    id: LayerId,
    revision: RasterRevision,
    data: RasterData,
}
pub(crate) struct NativeFrame {
    captures: Vec<PreparedCapture>,
    publications: Vec<Publication>,
}
impl Drop for NativeFrame {
    fn drop(&mut self) {
        for publication in &self.publications {
            if publication.revision.try_data().is_none() {
                let _ = publication
                    .revision
                    .publish(Err("Native raster frame was abandoned".into()));
            }
        }
    }
}

impl WgpuRasterizer {
    /// Native document renderer for headless workflow qualification. GTK enables
    /// this mode only after its UI, managed presentation and workload gates pass.
    /// Dab RGB values are linear coordinates in `color.space`.
    pub fn new_native_headless(color: DocumentColor) -> Result<Self, GpuRasterError> {
        let mut r = pollster::block_on(Self::headless_with_working_format(
            wgpu::TextureFormat::Rgba32Float,
            color.space,
        ))?;
        r.document_color = color;
        r.scene = None;
        let transfer = r.prepare_native_transfer(color.space)?;
        r.native_edit = Some(NativeEdit::new(&r, transfer));
        Ok(r)
    }

    pub(crate) fn encode_native_rasters(
        &mut self,
        layers: &[Layer],
        encoder: &mut submission::CommandEncoder,
    ) -> Result<Option<NativeFrame>, GpuRasterError> {
        if self.native_edit.is_none()
            || !layers.iter().any(|l| {
                l.raster.try_data().is_none() || l.masks().any(|m| m.raster.try_data().is_none())
            })
        {
            return Ok(None);
        }
        let mut frame = NativeFrame {
            captures: Vec::new(),
            publications: Vec::new(),
        };
        // Reserve the ordinary backing worker before borrowing live textures.
        let runtime = self.raster.get_or_insert_with(Default::default);
        if runtime.worker.as_ref().is_some_and(|w| !w.ready()) {
            return Err(GpuRasterError::Effect(
                "Raster backing queue is full".into(),
            ));
        }
        if runtime.worker.is_none() {
            runtime.worker = Some(CaptureWorker::new(
                (*self.device).clone(),
                self.raster_buffers.clone(),
            )?);
        }
        let runtime = self.raster.as_ref().unwrap();
        let mut inputs = Vec::new();
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
                let (textures, watercolor) = self.raster_textures(id);
                let mut data = RasterData {
                    // Cold, losslessly backed tiles remain part of every new
                    // revision even when they own no working GPU surface.
                    tiles: current.data.tiles.clone(),
                    watercolor,
                };
                for (key, texture) in textures {
                    let tile = if let Some(tile) = current
                        .data
                        .tiles
                        .get(&key)
                        .filter(|_| !current.changed.contains(&key.coordinate))
                    {
                        tile.clone()
                    } else {
                        let tile = RasterTile::pending(key.plane.descriptor(self.document_color()));
                        inputs.push((texture, tile.clone()));
                        tile
                    };
                    data.tiles.insert(key, tile);
                }
                frame.publications.push(Publication {
                    id,
                    revision: revision.clone(),
                    data,
                });
            }
        }
        if frame.publications.is_empty() {
            return Ok(None);
        }
        // Check actual chunk rounding and per-chunk status allocation, not just
        // payload size, before recording writes or reserving readback memory.
        let staging: u64 = inputs
            .chunks(MAX_BATCH_TILES)
            .map(|chunk| {
                let bytes: u64 = chunk
                    .iter()
                    .map(|(_, tile)| tile.descriptor().byte_len([PAGE_SIZE; 2]).unwrap() as u64)
                    .sum();
                bytes.next_power_of_two() + STATUS_BYTES
            })
            .sum();
        if staging > MAX_CAPTURE_BYTES {
            return Err(GpuRasterError::Effect(
                "Raster frame exceeds the 256 MiB staging budget".into(),
            ));
        }
        let native = self.native_edit.as_ref().unwrap();
        native.status.reset(encoder);
        native
            .validator
            .encode(self, encoder, &inputs, &native.status)?;
        // All validation passes precede every promotion, including mixed planes
        // and targets. Scratch can then be reused without retaining dirty-photo
        // sized canonical/encoded copies. A capture copy precedes each reuse.
        for chunk in inputs.chunks(MAX_BATCH_TILES) {
            let mut color = Vec::new();
            let mut scalar = Vec::new();
            let mut promotions = Vec::new();
            let mut copies = Vec::new();
            for (texture, tile) in chunk {
                let canonical = if tile.descriptor().channels == 4 {
                    let slot = &native.colors[color.len()];
                    color.push(NativeTileRequest {
                        working: texture,
                        encoded: &slot.encoded,
                        canonical: &slot.canonical,
                        transfer: &native.transfer,
                        depth: self.document_color().depth,
                        alpha: tile.descriptor().alpha,
                        region: [0, 0, 256, 256],
                    });
                    copies.push(TileCapture {
                        source: CaptureSource::Texture(&slot.encoded),
                        tile: tile.clone(),
                    });
                    &slot.canonical
                } else {
                    let slot = &native.scalars[scalar.len()];
                    scalar.push(NativeScalarRequest {
                        working: texture,
                        encoded: &slot.encoded,
                        canonical: &slot.canonical,
                        depth: self.document_color().depth,
                        region: [0, 0, 256, 256],
                    });
                    copies.push(TileCapture {
                        source: CaptureSource::Packed(&slot.encoded),
                        tile: tile.clone(),
                    });
                    &slot.canonical
                };
                promotions.push(NativePromotion {
                    canonical,
                    working: texture,
                    region: [0, 0, 256, 256],
                });
            }
            let color = native.color.prepare(&self.device, &color, &native.status)?;
            let scalar = native
                .scalar
                .prepare(&self.device, &scalar, &native.status)?;
            let promotions = native
                .promoter
                .prepare(&self.device, &promotions, &native.status)?;
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                native.color.encode(&mut pass, &color);
                native.scalar.encode(&mut pass, &scalar);
            }
            encoder.reserve_passes(promotions.pass_count());
            native.promoter.encode(encoder, &promotions);
            frame
                .captures
                .push(self.prepare_capture(encoder, &copies, Some(&native.status))?);
        }
        Ok(Some(frame))
    }

    pub(crate) fn finish_native_rasters(
        &mut self,
        mut frame: NativeFrame,
        submission: wgpu::SubmissionIndex,
    ) -> Result<(), GpuRasterError> {
        let captures: Vec<_> = frame
            .captures
            .drain(..)
            .map(|capture| capture.submitted(self, submission.clone()))
            .collect();
        let runtime = self.raster.as_mut().unwrap();
        if !captures.is_empty() {
            runtime.worker.as_ref().unwrap().submit(captures)?;
        }
        for publication in &frame.publications {
            publication
                .revision
                .publish(Ok(publication.data.clone()))
                .map_err(GpuRasterError::Effect)?;
            let current = runtime.targets.get_mut(&publication.id).unwrap();
            current.revision = publication.revision.clone();
            current.data = publication
                .revision
                .wait_data()
                .map_err(GpuRasterError::Effect)?;
            self.native_edit.as_mut().unwrap().backing.insert(publication.id, current.data.clone());
            current.changed.clear();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
