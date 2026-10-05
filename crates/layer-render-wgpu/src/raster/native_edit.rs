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
    pub(super) backing: BTreeMap<SourceTarget, Arc<RasterData>>,
    pub(crate) color_cache_bytes: u64,
    /// Optional view/source caches keep the host's existing fast-residency policy.
    pub(crate) display_complete_bytes: u64,
    /// Shared allowance for retained filter images and display levels.
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
    preview_scalars: Option<(NativeEncodeStatus, Vec<wgpu::Buffer>)>,
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
        let display_complete_bytes = crate::display_memory::complete_budget(&r.device, 0);
        #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows", target_vendor = "apple")))]
        let display_complete_bytes = 0;
        Self {
            backing: BTreeMap::new(),
            color_cache_bytes: 256 * 1024 * 1024,
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
            preview_scalars: None,
            color: NativeTileEncoder::prevalidated(&r.device, in_place),
            scalar: if in_place {
                NativeScalarEncoder::validated_in_place(&r.device)
            } else {
                NativeScalarEncoder::with_device(&r.device)
            },
            promoter: (!in_place).then(|| NativePromoter::with_device(&r.device)),
            validator: validate::Validator::new(&r.device, r.document_color().depth),
        }
    }
    #[cfg(test)]
    pub(crate) fn pipelines(&self) -> impl Iterator<Item = &Deferred<wgpu::ComputePipeline>> {
        self.color
            .pipelines
            .iter()
            .chain(&self.scalar.pipelines)
            .chain(self.promoter.iter().flat_map(|p| p.pipelines.iter()))
            .chain(&self.validator.pipelines)
    }
    pub(crate) fn required_pipelines(&self, depth: layer_core::color::SampleDepth) -> impl Iterator<Item = &Deferred<wgpu::ComputePipeline>> {
        self.color.pipelines_for_depth(depth)
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
            + self.preview_scalars.as_ref().map_or(0, |(_, buffers)| STATUS_BYTES + buffers.iter().map(wgpu::Buffer::size).sum::<u64>())
        // Transfer storage is owned/accounted by the shared scene decoder cache.
    }
    pub(crate) fn image_pixel_budget(&self, resident: u64) -> u64 {
        #[cfg(test)]
        if let Some(bytes) = self.image_pixel_bytes { return bytes; }
        let display = crate::scene::scale::CACHE_BYTES;
        let floor = crate::scene::windows::DEFAULT_IMAGE_PIXEL_BYTES + display;
        self.composition_bytes.max(floor).saturating_sub(display.saturating_add(resident))
    }

}

struct Publication {
    id: SourceTarget,
    revision: RasterRevision,
    data: RasterData,
}
pub(crate) struct NativeFrame {
    capture: Option<NativeCapture>,
    publications: Vec<Publication>,
    pub(crate) canonical_pages: Vec<(SourceTarget, [u32; 2])>,
}
pub(crate) struct NativeJob {
    pub frame: NativeFrame,
    inputs: Vec<(wgpu::Texture, RasterTile, layer_core::color::LayerColorMode)>,
    views: crate::native_tiles::PublicationViews,
    changes: Vec<Option<wgpu::Buffer>>,
    validated: usize,
    encoded: usize,
    started: bool,
}
impl NativeJob {
    const TIMING_PASSES: usize = 6;
    pub fn complete(&self) -> bool { self.started && self.encoded == self.inputs.len() }
    fn pass_count(&self, native: &NativeEdit) -> usize {
        self.inputs.chunks(MAX_BATCH_TILES).map(|chunk| {
            native.validator.pass_count(chunk.iter().map(|(texture, _, _)| texture.format()))
                + 1 + usize::from(native.promoter.is_some())
        }).sum()
    }
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
    pub(crate) fn canonicalize_preview_watercolor(&mut self, encoder: &mut submission::CommandEncoder) -> Result<(), GpuRasterError> {
        let depth = self.document_color().depth.coverage();
        let Some(native) = self.native_edit.as_mut() else { return Ok(()); };
        let textures = self.preview_watercolor_wetness_pages.iter()
            .filter(|page| !self.preview_damage.intersect(page_rect(page.coordinate)).is_empty())
            .map(|page| &page.active().texture).collect::<Vec<_>>();
        if textures.is_empty() { return Ok(()); }
        let (status, buffers) = native.preview_scalars.get_or_insert_with(|| (NativeEncodeStatus::new(&self.device), Vec::new()));
        while buffers.len() < textures.len().min(MAX_BATCH_TILES) {
            buffers.push(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("watercolor preview native coverage"),
                size: u64::from(PAGE_SIZE).pow(2) * depth.bytes() as u64,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }));
        }
        status.reset(encoder);
        let mut views = Default::default();
        for chunk in textures.chunks(MAX_BATCH_TILES) {
            let requests = chunk.iter().enumerate().map(|(index, &texture)| NativeScalarRequest {
                working: texture, encoded: &buffers[index], canonical: native.scalars.get(index).unwrap_or(texture),
                depth, region: [0, 0, PAGE_SIZE, PAGE_SIZE],
            }).collect::<Vec<_>>();
            let scalar = native.scalar.prepare(&self.device, &requests, status, &mut views)?;
            let promotions = requests.iter().map(|request| NativePromotion {
                canonical: request.canonical, working: request.working, region: request.region,
            }).collect::<Vec<_>>();
            let promotions = native.promoter.as_ref().map(|promoter|
                promoter.prepare(&self.device, &promotions, status, &mut views)).transpose()?;
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                native.scalar.encode(&mut pass, &scalar);
            }
            if let (Some(promoter), Some(promotions)) = (&native.promoter, promotions) {
                encoder.reserve_passes(promotions.pass_count());
                promoter.encode(encoder, &promotions);
            }
        }
        Ok(())
    }

    /// Admitted display, decoded-source and in-flight upload ceilings in bytes.
    pub fn display_memory_limits(&self) -> [u64; 3] {
        let display = self.native_edit.as_ref().map_or(0, |n| n.display_complete_bytes);
        let [sources, uploads] = self.source_tiles.borrow().admitted_bytes();
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
            self.scale_display = None;
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
        directory: Option<&std::path::Path>,
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
        directory: Option<&std::path::Path>,
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
            .require_float32()?
            .with_working_space(color.space).with_depth(color.depth);
        let mut r = Self::from_wgpu_inner(adapter, device, queue, Initialization::Interactive)?;
        r.startup.as_mut().unwrap().host_catalog_pending = !cfg!(target_arch = "wasm32");
        r.initialize_native(color)?;
        Ok(r)
    }

    /// Native document renderer for headless workflow qualification.
    /// Dab RGB values are linear coordinates in `color.space`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_native_headless(color: DocumentColor) -> Result<Self, GpuRasterError> {
        let mut r = pollster::block_on(Self::headless(color.space, Initialization::Headless))?;
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
            .require_float32()?
            .with_working_space(color.space).with_depth(color.depth);
        let mut r = Self::from_wgpu_inner(adapter, device, queue, Initialization::Snapshot)?;
        r.initialize_native(color)?;
        Ok(r)
    }

    fn initialize_native(&mut self, color: DocumentColor) -> Result<(), GpuRasterError> {
        self.document_color = color;
        self.device = self.device.clone().with_depth(color.depth);
        self.ui_rendition = color.depth.is_float().then_some(Default::default());
        self.scene = None;
        self.source_tiles = std::cell::RefCell::new(crate::scene::sources::DecodedTiles::new(color.space));
        let transfer = self.prepare_native_transfer(color.space)?;
        let native = NativeEdit::new(self, transfer);
        let allowance = native.display_complete_bytes;
        #[cfg(not(target_arch = "wasm32"))]
        let allowance = if self.snapshot_worker { 0 } else { allowance };
        self.source_tiles.get_mut().admit(allowance);
        self.native_edit = Some(native);
        Ok(())
    }

    pub(crate) fn encode_native_rasters(&mut self, scene: SceneView<'_>, encoder: &mut submission::CommandEncoder) -> Result<Option<NativeFrame>, GpuRasterError> {
        let Some(mut job) = self.prepare_native_rasters(scene)? else { return Ok(None); };
        let timed = if crate::performance_trace::enabled() {
            let passes = job.pass_count(self.native_edit.as_ref().unwrap());
            let timed = encoder.can_fit_passes(passes + NativeJob::TIMING_PASSES)
                && self.telemetry.phase_begin(14, &self.device, &self.queue, encoder);
            if !timed {
                crate::performance_trace::counter(c"Capy GPU native capture omitted", self.metrics.submissions);
            }
            timed
        } else { false };
        if timed { encoder.reserve_passes(2); }
        let preflight = timed && self.telemetry.phase_begin(15, &self.device, &self.queue, encoder);
        if preflight { encoder.reserve_passes(2); }
        else if timed { crate::performance_trace::counter(c"Capy GPU native preflight omitted", self.metrics.submissions); }
        let result = (|| -> Result<(), GpuRasterError> {
            while job.validated < job.inputs.len() { self.step_native_rasters(&mut job, encoder)?; }
            Ok(())
        })();
        if preflight { self.telemetry.phase_end(15, encoder); }
        let encoding = timed && result.is_ok() && self.telemetry.phase_begin(16, &self.device, &self.queue, encoder);
        if encoding { encoder.reserve_passes(2); }
        else if timed { crate::performance_trace::counter(c"Capy GPU native encoding omitted", self.metrics.submissions); }
        let result = result.and_then(|()| {
            while self.step_native_rasters(&mut job, encoder)? {}
            Ok(())
        });
        if encoding { self.telemetry.phase_end(16, encoder); }
        if timed { self.telemetry.phase_end(14, encoder); }
        result?;
        Ok(Some(job.frame))
    }

    pub(crate) fn prepare_native_rasters(&mut self, scene: SceneView<'_>) -> Result<Option<NativeJob>, GpuRasterError> {
        if self.native_edit.is_none()
            || !source_access::placed_targets(scene).any(|target| scene.raster(target).is_some_and(|r| r.try_data().is_none()))
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
        runtime.ensure_worker(&self.device, &self.raster_buffers)?;
        let runtime = self.raster.as_ref().unwrap();
        let mut inputs = Vec::new();
        for target in source_access::placed_targets(scene).filter(|t| scene.raster(*t).is_some()) {
            for (id, revision) in std::iter::once((target, scene.raster(target).unwrap()))
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
                        let tile = RasterTile::pending(key.plane.descriptor_for(self.document_color(), scene.color_mode(id)));
                        inputs.push((texture.clone(), tile.clone(), scene.color_mode(id)));
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
        self.native_capture_job(frame, inputs).map(Some)
    }

    fn native_capture_job(&self, mut frame: NativeFrame, inputs: Vec<(wgpu::Texture, RasterTile, layer_core::color::LayerColorMode)>) -> Result<NativeJob, GpuRasterError> {
        let output_bytes: u64 = STATUS_BYTES
            + inputs
                .iter()
                .map(|(_, tile, _)| tile.descriptor().byte_len([PAGE_SIZE; 2]).unwrap() as u64)
                .sum::<u64>();
        if output_bytes > layer_core::raster::MAX_PUBLICATION_BYTES {
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
        frame.capture = Some(NativeCapture {
            outputs: Vec::with_capacity(inputs.len()),
            status: NativeEncodeStatus::new(&self.device),
            pool: self.raster_buffers.clone(),
            device: (*self.device).clone(),
            queue: self.queue.clone(),
        });
        validate::Validator::validate(&inputs.iter().map(|(t, tile, _)| (t, tile.clone())).collect::<Vec<_>>())?;
        let changes=frame.canonical_pages.iter().map(|&(id,coordinate)|self.changed_cells.as_ref().and_then(|c|c.buffer(id,coordinate)).cloned()).collect();
        Ok(NativeJob { frame, inputs, views: Default::default(), changes, validated: 0, encoded: 0, started: false })
    }

    pub(crate) fn encode_private_tiles(&mut self, inputs: Vec<(wgpu::Texture, RasterTile, layer_core::color::LayerColorMode)>, encoder: &mut submission::CommandEncoder)
        -> Result<NativeCapture, GpuRasterError> {
        let frame = NativeFrame { capture: None, publications: Vec::new(), canonical_pages: Vec::new() };
        let mut job = self.native_capture_job(frame, inputs)?;
        while self.step_native_rasters(&mut job, encoder)? {}
        Ok(job.frame.capture.take().unwrap())
    }

    pub(crate) fn step_native_rasters(&mut self, job: &mut NativeJob, encoder: &mut submission::CommandEncoder) -> Result<bool, GpuRasterError> {
        let native = self.native_edit.as_ref().unwrap();
        let capture = job.frame.capture.as_mut().unwrap();
        let status = &capture.status;
        if !job.started { status.reset(encoder); job.started = true; }
        else if job.encoded == job.inputs.len() { return Ok(false); }
        let views = &mut job.views;
        if job.validated < job.inputs.len() {
            let end = (job.validated + MAX_BATCH_TILES).min(job.inputs.len());
            let inputs = job.inputs[job.validated..end].iter().map(|(t, tile, _)| (t, tile.clone())).collect::<Vec<_>>();
            native.validator.encode(self, encoder, &inputs, status, views)?;
            job.validated = end;
            return Ok(true);
        }
        let end = (job.encoded + MAX_BATCH_TILES).min(job.inputs.len());
        let chunk = &job.inputs[job.encoded..end];
        if !chunk.is_empty() {
            let first = capture.outputs.len();
            capture.outputs.extend(chunk.iter().map(|(_, tile, _)| {
                NativeOutput {
                    resource: self
                        .raster_buffers
                        .take_native(&self.device, tile.descriptor()),
                    tile: tile.clone(),
                }
            }));
            let mut color = Vec::new();
            let mut scalar = Vec::new();
            let mut color_changes=Vec::new();
            let mut promotions = Vec::new();
            for (index,((texture, tile, mode), output)) in chunk.iter().zip(&capture.outputs[first..]).enumerate() {
                let canonical = if tile.descriptor().channels != 1 {
                    let canonical = native.colors.get(color.len()).unwrap_or(texture);
                    let encoded = match &output.resource { pool::Resource::Texture(t) => t.into(), pool::Resource::Buffer(b) => b.into() };
                    color_changes.push(job.changes.get(job.encoded+index).and_then(Option::as_ref));
                    color.push(NativeTileRequest {
                        mode: *mode, space: self.document_color().space,
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
            let color=if color_changes.iter().any(Option::is_some) {
                native.color.prepare_tracked(&self.device,&color,status,views,&color_changes)?
            } else {native.color.prepare(&self.device,&color,status,views)?};
            let scalar =
                native
                    .scalar
                    .prepare(&self.device, &scalar, status, views)?;
            let promotions = native
                .promoter
                .as_ref()
                .map(|promoter| {
                    promoter.prepare(&self.device, &promotions, status, views)
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
        job.encoded = end;
        Ok(true)
    }

    pub(crate) fn finish_native_rasters(
        &mut self,
        mut frame: NativeFrame,
        commit: bool,
    ) -> Result<(), GpuRasterError> {
        let runtime = self.raster.as_mut().unwrap();
        if let Some(capture) = frame.capture.take()
            && !capture.outputs.is_empty() {
                runtime
                    .worker
                    .as_ref()
                    .unwrap()
                    .submit_batch(capture)?;
            }
        while let Some(publication) = frame.publications.pop() {
            let data = if commit {
                publication.revision.publish(Ok(publication.data)).map_err(GpuRasterError::Effect)?;
                publication.revision.wait_data().map_err(GpuRasterError::Effect)?
            } else {
                Arc::new(publication.data)
            };
            let current = runtime.targets.get_mut(&publication.id).unwrap();
            current.revision = publication.revision;
            current.data = data;
            self.native_edit
                .as_mut()
                .unwrap()
                .backing
                .insert(publication.id, current.data.clone());
            if commit && let Some(scene)=&mut self.scene {scene.raster_published(publication.id,current.data.clone());}
            current.changed.clear();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
