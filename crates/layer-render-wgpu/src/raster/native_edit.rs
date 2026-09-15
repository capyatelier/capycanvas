//! Native commit ownership for the Float32 renderer. Each frame validates every
//! changed page, then quantizes into immutable native outputs and canonical
//! Float32 working pixels. Optional in-place storage avoids candidate scratch;
//! other devices promote through bounded scratch. Readback follows presentation.
use super::*;
use crate::native_tiles::{
    MAX_BATCH_TILES, NativeTileEncoder, NativeTileRequest, NativeTransfer,
    promote::{NativePromoter, NativePromotion},
    scalar::{NativeScalarEncoder, NativeScalarRequest},
};
use layer_core::color::DocumentColor;
mod validate;

pub(crate) struct NativeEdit {
    pub(super) backing: BTreeMap<LayerId, Arc<RasterData>>,
    pub(crate) color_cache_bytes: u64,
    pub(crate) display_dense_bytes: u64,
    pub(crate) display_cache_bytes: u64,
    /// Provisional ceiling for live physical-filter pixel allocations, separate
    /// from source, paint and composite residency. Host release qualification
    /// must establish the combined workload budget as well.
    pub image_pixel_bytes: u64,
    transfer: NativeTransfer,
    color: NativeTileEncoder,
    scalar: NativeScalarEncoder,
    promoter: Option<NativePromoter>,
    validator: validate::Validator,
    colors: Vec<wgpu::Texture>,
    scalars: Vec<wgpu::Texture>,
}
impl NativeEdit {
    fn new(r: &WgpuRasterizer, transfer: NativeTransfer) -> Self {
        let in_place = !crate::native_tiles::native_in_place_features(&r.adapter).is_empty()
            && r.device
                .features()
                .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);
        Self::with_mode(r, transfer, in_place)
    }
    fn with_mode(r: &WgpuRasterizer, transfer: NativeTransfer, in_place: bool) -> Self {
        let scratch_count = if in_place { 0 } else { MAX_BATCH_TILES };
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
        let colors = (0..scratch_count)
            .map(|_| texture(wgpu::TextureFormat::Rgba32Float))
            .collect();
        let scalars = (0..scratch_count)
            .map(|_| texture(wgpu::TextureFormat::R32Float))
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
            color: if in_place {
                NativeTileEncoder::validated_in_place(&r.device)
            } else {
                NativeTileEncoder::with_device(&r.device)
            },
            scalar: if in_place {
                NativeScalarEncoder::validated_in_place(&r.device)
            } else {
                NativeScalarEncoder::with_device(&r.device)
            },
            promoter: (!in_place).then(|| NativePromoter::with_device(&r.device)),
            validator: validate::Validator::new(&r.device),
        }
    }
    pub fn storage_bytes(&self) -> u64 {
        self.promoter
            .as_ref()
            .map_or(0, NativePromoter::storage_bytes)
            + self.color.storage_bytes()
            + self.scalar.storage_bytes()
            + self.colors.iter().map(texture_bytes).sum::<u64>()
            + self.scalars.iter().map(texture_bytes).sum::<u64>()
        // Transfer storage is owned/accounted by the shared scene decoder cache.
    }
}

struct Publication {
    id: LayerId,
    revision: RasterRevision,
    data: RasterData,
}
pub(crate) struct NativeFrame {
    capture: Option<NativeCapture>,
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
    /// Surface-compatible native SDR renderer. Configure the working format
    /// before creating any pipeline recipes, including deferred startup jobs.
    /// Construction belongs to the host's GPU owner, outside the input thread.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_wgpu_native_staged_cached(
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        directory: &std::path::Path,
        color: DocumentColor,
    ) -> Result<Self, GpuRasterError> {
        let device = PipelineDevice::cached(device, &adapter, directory)
            .with_working_format(wgpu::TextureFormat::Rgba32Float)?
            .with_working_space(color.space);
        let mut r = Self::from_wgpu_inner(adapter, device, queue, Initialization::Interactive)?;
        r.startup.as_mut().unwrap().host_catalog_pending = true;
        r.initialize_native(color)?;
        Ok(r)
    }

    /// Native document renderer for headless workflow qualification.
    /// Dab RGB values are linear coordinates in `color.space`.
    pub fn new_native_headless(color: DocumentColor) -> Result<Self, GpuRasterError> {
        let mut r = pollster::block_on(Self::headless_with_working_format(
            wgpu::TextureFormat::Rgba32Float,
            color.space,
            Initialization::Warm,
        ))?;
        r.initialize_native(color)?;
        Ok(r)
    }

    /// Capture is already on a worker and does not draw new strokes. Retain
    /// lazy pipeline recipes instead of warming every brush, tip and transform.
    pub(crate) fn new_native_capture(color: DocumentColor) -> Result<Self, GpuRasterError> {
        let mut r = pollster::block_on(Self::headless_with_working_format(
            wgpu::TextureFormat::Rgba32Float,
            color.space,
            Initialization::Snapshot,
        ))?;
        r.initialize_native(color)?;
        Ok(r)
    }

    pub(crate) fn native_capture_on_gpu(
        adapter: wgpu::Adapter,
        device: PipelineDevice,
        queue: wgpu::Queue,
        color: DocumentColor,
    ) -> Result<Self, GpuRasterError> {
        let device = device
            .with_working_format(wgpu::TextureFormat::Rgba32Float)?
            .with_working_space(color.space);
        let mut r = Self::from_wgpu_inner(adapter, device, queue, Initialization::Snapshot)?;
        r.initialize_native(color)?;
        Ok(r)
    }

    fn initialize_native(&mut self, color: DocumentColor) -> Result<(), GpuRasterError> {
        self.document_color = color;
        self.scene = None;
        let transfer = self.prepare_native_transfer(color.space)?;
        self.native_edit = Some(NativeEdit::new(self, transfer));
        Ok(())
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
            capture: None,
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
        // Immutable native outputs replace frame-sized mapped staging. Admission
        // reserves one separate bounded transfer in the backing worker.
        let staging: u64 = STATUS_BYTES
            + inputs
                .iter()
                .map(|(_, tile)| tile.descriptor().byte_len([PAGE_SIZE; 2]).unwrap() as u64)
                .sum::<u64>();
        if staging > MAX_CAPTURE_BYTES {
            return Err(GpuRasterError::Effect(
                "Raster frame exceeds the 256 MiB native output budget".into(),
            ));
        }
        // Full views live only through this publication's command recording;
        // the view cache never pins working or encoded pixels between frames.
        let mut views = crate::native_tiles::PublicationViews::default();
        let native = self.native_edit.as_ref().unwrap();
        frame.capture = Some(NativeCapture {
            outputs: Vec::with_capacity(inputs.len()),
            status: NativeEncodeStatus::new(&self.device),
            pool: self.raster_buffers.clone(),
            device: (*self.device).clone(),
            queue: self.queue.clone(),
        });
        let capture = frame.capture.as_mut().unwrap();
        let status = &capture.status;
        status.reset(encoder);
        native
            .validator
            .encode(self, encoder, &inputs, status, &mut views)?;
        // Every input is validated before any working write. On supporting
        // devices, quantization writes back in place; otherwise only canonical
        // Float32 scratch is reused. Encoded samples belong to this publication.
        for chunk in inputs.chunks(MAX_BATCH_TILES) {
            let first = capture.outputs.len();
            capture.outputs.extend(chunk.iter().map(|(_, tile)| {
                NativeOutput {
                    resource: self
                        .raster_buffers
                        .take_native(&self.device, tile.descriptor()),
                    tile: tile.clone(),
                }
            }));
            let mut color = Vec::new();
            let mut scalar = Vec::new();
            let mut promotions = Vec::new();
            for ((texture, tile), output) in chunk.iter().zip(&capture.outputs[first..]) {
                let canonical = if tile.descriptor().channels == 4 {
                    let canonical = native.colors.get(color.len()).unwrap_or(texture);
                    let pool::Resource::Texture(encoded) = &output.resource else {
                        unreachable!()
                    };
                    color.push(NativeTileRequest {
                        working: texture,
                        encoded,
                        canonical,
                        transfer: &native.transfer,
                        depth: self.document_color().depth,
                        alpha: tile.descriptor().alpha,
                        region: [0, 0, 256, 256],
                    });
                    canonical
                } else {
                    let canonical = native.scalars.get(scalar.len()).unwrap_or(texture);
                    let pool::Resource::Buffer(encoded) = &output.resource else {
                        unreachable!()
                    };
                    scalar.push(NativeScalarRequest {
                        working: texture,
                        encoded,
                        canonical,
                        depth: self.document_color().depth,
                        region: [0, 0, 256, 256],
                    });
                    canonical
                };
                promotions.push(NativePromotion {
                    canonical,
                    working: texture,
                    region: [0, 0, 256, 256],
                });
            }
            let color =
                native
                    .color
                    .prepare_with_views(&self.device, &color, status, &mut views)?;
            let scalar =
                native
                    .scalar
                    .prepare_with_views(&self.device, &scalar, status, &mut views)?;
            let promotions = native
                .promoter
                .as_ref()
                .map(|promoter| {
                    promoter.prepare_with_views(&self.device, &promotions, status, &mut views)
                })
                .transpose()?;
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                native.color.encode(&mut pass, &color);
                native.scalar.encode(&mut pass, &scalar);
            }
            if let (Some(promoter), Some(promotions)) = (&native.promoter, promotions) {
                encoder.reserve_passes(promotions.pass_count());
                promoter.encode(encoder, &promotions);
            }
        }
        Ok(Some(frame))
    }

    pub(crate) fn finish_native_rasters(
        &mut self,
        mut frame: NativeFrame,
        _submission: wgpu::SubmissionIndex,
    ) -> Result<(), GpuRasterError> {
        let runtime = self.raster.as_mut().unwrap();
        if let Some(capture) = frame.capture.take() {
            if !capture.outputs.is_empty() {
                runtime
                    .worker
                    .as_ref()
                    .unwrap()
                    .submit_batch(CaptureBatch::Native(capture))?;
            }
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
            self.native_edit
                .as_mut()
                .unwrap()
                .backing
                .insert(publication.id, current.data.clone());
            current.changed.clear();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
