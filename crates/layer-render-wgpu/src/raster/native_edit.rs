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
    /// Optional view/source caches keep the host's existing fast-residency policy.
    pub(crate) display_complete_bytes: u64,
    /// Shared allowance for retained filter images and completed display pixels.
    /// Zero uses the original bounded display + filter-window allocation.
    pub(crate) composition_bytes: u64,
    #[cfg(test)]
    pub image_pixel_bytes: Option<u64>,
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
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "windows", target_vendor = "apple"))]
        let display_complete_bytes = crate::display_memory::complete_budget(&r.device);
        #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows", target_vendor = "apple")))]
        let display_complete_bytes = 0;
        Self {
            backing: BTreeMap::new(),
            color_cache_bytes: 256 * 1024 * 1024,
            display_dense_bytes: crate::live_display::DENSE_BYTES,
            display_cache_bytes: crate::live_display::CACHE_BYTES,
            display_complete_bytes,
            composition_bytes: {
                #[cfg(any(target_os = "linux", target_os = "android", target_os = "windows", target_vendor = "apple"))]
                { crate::display_memory::composition_budget(&r.device, display_complete_bytes) }
                #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows", target_vendor = "apple")))]
                { 0 }
            },
            #[cfg(test)]
            image_pixel_bytes: None,
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
            validator: validate::Validator::new(&r.device, r.document_color().depth),
        }
    }
    pub(crate) fn pipelines(&self) -> impl Iterator<Item = &Deferred<wgpu::ComputePipeline>> {
        self.color
            .pipelines
            .iter()
            .chain(&self.scalar.pipelines)
            .chain(self.promoter.iter().flat_map(|p| p.pipelines.iter()))
            .chain(&self.validator.pipelines)
    }
    pub(crate) fn required_pipelines(&self, depth: layer_core::color::SampleDepth) -> impl Iterator<Item = &Deferred<wgpu::ComputePipeline>> {
        self.color.pipelines_for_depth(depth).iter()
            .chain(&self.scalar.pipelines)
            .chain(self.promoter.iter().flat_map(|p| p.pipelines.iter()))
            .chain(&self.validator.pipelines)
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
    pub(crate) fn display_allowance(&self, layers: &[Layer], extent: [u32; 2]) -> u64 {
        if crate::scene::Scene::capture_image_bound(layers, PixelRect::full(extent)) == 0 {
            self.display_complete_bytes
        } else {
            self.composition_bytes.saturating_sub(crate::scene::windows::DEFAULT_IMAGE_PIXEL_BYTES)
        }
    }
    pub(crate) fn image_pixel_budget(&self, r: &WgpuRasterizer, layers: &[Layer], extent: [u32; 2]) -> Result<u64, GpuRasterError> {
        #[cfg(test)]
        if let Some(bytes) = self.image_pixel_bytes { return Ok(bytes); }
        let pixels = u64::from(extent[0]) * u64::from(extent[1]) * 16;
        let display = if pixels <= self.display_dense_bytes { pixels } else {
            crate::live_display::Cache::allocation_bound(r, extent, self.display_cache_bytes,
                self.display_allowance(layers, extent))?
        };
        // Borrow the allowance left by this document's actual display plan.
        // A fixed, independent image cap evicted reusable filter inputs even
        // when the much larger display allowance was mostly unoccupied.
        let floor = crate::scene::windows::DEFAULT_IMAGE_PIXEL_BYTES + self.display_cache_bytes;
        Ok(self.composition_bytes.max(floor).saturating_sub(display))
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
    pub(crate) canonical_pages: Vec<(LayerId, [u32; 2])>,
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
    /// Admitted display, decoded-source and in-flight upload ceilings in bytes.
    pub fn display_memory_limits(&self) -> [u64; 3] {
        let display = self.native_edit.as_ref().map_or(0, |n| n.display_complete_bytes);
        let [sources, uploads] = self.scene.as_ref().map_or([0; 2], |s| s.source_cache_limits());
        [display, sources, uploads]
    }

    /// Host allowance for completed display pixels. Together with the minimum
    /// filter-window reserve, this bounds retained filters and display pixels.
    /// Zero selects the fixed window/tile fallback. This is not a reservation.
    pub fn set_complete_display_allowance(&mut self, bytes: u64) {
        if let Some(native) = &mut self.native_edit
            && (native.display_complete_bytes != bytes
                || native.composition_bytes != bytes.saturating_add(crate::scene::windows::DEFAULT_IMAGE_PIXEL_BYTES))
        {
            native.display_complete_bytes = bytes;
            native.composition_bytes = bytes.saturating_add(crate::scene::windows::DEFAULT_IMAGE_PIXEL_BYTES);
            self.live_display = None;
        }
    }

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
        let device = PipelineDevice::cached(device, &adapter, directory);
        Self::native_staged_on_device(adapter, device, queue, color)
    }

    /// A private candidate shares immutable source samples with the current
    /// canvas and its workers, so preview/adoption cannot double that cache.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn color_candidate_staged_cached(
        &self,
        directory: &std::path::Path,
        color: DocumentColor,
    ) -> Result<Self, GpuRasterError> {
        let mut device = PipelineDevice::cached(self.device().clone(), &self.adapter, directory);
        device.source_samples = self.device.source_samples.clone();
        Self::native_staged_on_device(self.adapter.clone(), device, self.queue.clone(), color)
    }

    /// Portable native integer SDR backing with Float32 working pixels.
    pub fn from_wgpu_native_staged(
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        color: DocumentColor,
    ) -> Result<Self, GpuRasterError> {
        Self::native_staged_on_device(adapter, device.into(), queue, color)
    }

    pub(crate) fn native_staged_on_device(
        adapter: wgpu::Adapter,
        device: PipelineDevice,
        queue: wgpu::Queue,
        color: DocumentColor,
    ) -> Result<Self, GpuRasterError> {
        let device = device
            .with_working_format(wgpu::TextureFormat::Rgba32Float)?
            .with_working_space(color.space).with_hdr(color.depth.is_float());
        let mut r = Self::from_wgpu_inner(adapter, device, queue, Initialization::Interactive)?;
        r.startup.as_mut().unwrap().host_catalog_pending = !cfg!(target_arch = "wasm32");
        r.initialize_native(color)?;
        Ok(r)
    }

    /// Native document renderer for headless workflow qualification.
    /// Dab RGB values are linear coordinates in `color.space`.
    #[cfg(not(target_arch = "wasm32"))]
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
    #[cfg(not(target_arch = "wasm32"))]
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
            .with_working_space(color.space).with_hdr(color.depth.is_float());
        let mut r = Self::from_wgpu_inner(adapter, device, queue, Initialization::Snapshot)?;
        r.initialize_native(color)?;
        Ok(r)
    }

    fn initialize_native(&mut self, color: DocumentColor) -> Result<(), GpuRasterError> {
        self.document_color = color;
        self.device = self.device.clone().with_hdr(color.depth.is_float());
        self.ui_rendition = color.depth.is_float().then_some(Default::default());
        self.scene = None;
        let transfer = self.prepare_native_transfer(color.space)?;
        let native = NativeEdit::new(self, transfer);
        if self.startup.is_none() {
            for pipeline in native.pipelines() {
                pipeline.compile();
            }
        }
        // Transfer-table preparation creates the scene before NativeEdit's
        // headroom snapshot exists. Admit live source storage now, while that
        // new scene still owns no decoded pixels. Background snapshots retain
        // the original smaller source/upload ceilings.
        let allowance = native.display_complete_bytes;
        #[cfg(not(target_arch = "wasm32"))]
        let allowance = if self.snapshot_worker { 0 } else { allowance };
        if let Some(scene) = &mut self.scene {
            scene.admit_native_sources(allowance);
        }
        self.native_edit = Some(native);
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
            canonical_pages: Vec::new(),
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
                #[cfg(target_arch = "wasm32")]
                runtime.encoder.clone().ok_or_else(|| GpuRasterError::Effect("Browser raster worker is unavailable".into()))?,
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
                        frame.canonical_pages.push((id, key.coordinate));
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
        let output_bytes: u64 = STATUS_BYTES
            + inputs
                .iter()
                .map(|(_, tile)| tile.descriptor().byte_len([PAGE_SIZE; 2]).unwrap() as u64)
                .sum::<u64>();
        if output_bytes > MAX_NATIVE_OUTPUT_BYTES {
            return Err(GpuRasterError::Effect(
                "Raster frame exceeds the 1 GiB native output budget".into(),
            ));
        }
        for publication in &frame.publications {
            let pending_bytes = publication.data.tiles.values()
                .filter(|tile| tile.try_backing().is_none())
                .map(|tile| layer_core::raster::TileBlob::max_compressed_len(tile.descriptor()).unwrap() as u64)
                .sum::<u64>();
            publication.revision.reserve_pending_bytes(pending_bytes);
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
                        depth: self.document_color().depth.coverage(),
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
