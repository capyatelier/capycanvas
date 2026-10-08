//! Worker-owned exact document capture. The immutable snapshot shares backing
//! with saving; only the requested region and its dependencies become GPU pixels.
use super::*;
use layer_core::color::source::{SourceChannels, SourceInterpretation};
use layer_core::raster::{RasterData, RasterPlane};
use layer_core::authored::{ArtworkCapture, SceneIndex};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Conservative request planning ceiling, separate from codec/output buffers,
/// retained compressed sources and driver/pipeline memory. This is not a device
/// memory qualification; hosts must also enforce their measured process budget.
const PLANNED_PIXEL_BYTES: u64 = 512 * 1024 * 1024;

/// Share before worker initialization so cancellation also applies to setup.
#[derive(Clone, Default)]
pub struct CaptureControl {
    cancelled: Arc<AtomicBool>,
    #[cfg(target_arch = "wasm32")]
    decode_wake: Arc<std::sync::Mutex<Option<std::task::Waker>>>,
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
        #[cfg(target_arch = "wasm32")]
        {
            let wake = self.decode_wake.lock().unwrap().take();
            if let Some(waker) = wake { waker.wake(); }
        }
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
    /// Share cancellation with a source decoder without borrowing its worker job.
    pub fn cancellation_flag(&self) -> &AtomicBool {
        &self.cancelled
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
    #[cfg(target_arch = "wasm32")]
    image_decoder: Option<crate::BrowserImageDecoder>,
    #[cfg(target_arch = "wasm32")]
    nearest_coordinate_decoder: Option<crate::BrowserNearestCoordinateDecoder>,
    analyses: Vec<Arc<crate::effect_analysis::Prepared>>,
    #[cfg(target_arch = "wasm32")]
    analysis_backing_waiter: Option<crate::effect_analysis::BackingWaiter>,
    scene_pipelines: scene::Pipelines,
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
    /// Hosts can prewarm immutable samples in bounded batches before first
    /// presentation. Rendering and snapshot workers reuse the same cache.
    pub fn prepare_source_sample(&self, tile: &Arc<layer_core::raster::TileBlob>) -> Result<(), GpuRasterError> {
        self.device.source_samples.decode(tile).map(|_| ()).map_err(GpuRasterError::Color)
    }
    pub fn snapshot_gpu(&self) -> SnapshotGpu {
        SnapshotGpu {
            analyses: self.effect_analyses.clone(),
            #[cfg(target_arch = "wasm32")]
            analysis_backing_waiter: self.analysis_backing_waiter.clone(),
            scene_pipelines: self.scene_pipelines.clone(),
            #[cfg(target_arch = "wasm32")]
            encoder: self.browser_raster_encoder(),
            #[cfg(target_arch = "wasm32")]
            image_decoder: self.browser_image_decoder.clone(),
            #[cfg(target_arch = "wasm32")]
            nearest_coordinate_decoder: self.browser_nearest_coordinate_decoder.clone(),
            adapter: self.adapter.clone(),
            device: self.device.clone(),
            queue: self.queue.clone(),
        }
    }
}
impl SnapshotGpu {
    /// A host observation for stale asynchronous candidates; policy lives in
    /// the shared workflow, while surface/device lifetime remains host-owned.
    pub fn same_device(&self, other: &Self) -> bool {
        *self.device == *other.device
    }

    /// The largest canvas side the shared device can compose.
    pub fn max_document_dimension(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }
    pub fn preflight_image_object_affine(&self,scene:SceneView<'_>,object:layer_core::ImageObjectHandle,
        affine:layer_core::Affine64,view:layer_render::ViewState,
    )->Result<(),GpuRasterError> {
        crate::object_sampling::preflight_object_affine(scene,object,affine,view,self.device.limits().max_storage_buffer_binding_size)
    }

    /// Run on the file/inspection worker. Cloned handles keep the device alive
    /// through this job even if its canvas closes; loss still fails the job.
    pub fn capture(&self, capture: ArtworkCapture, control: CaptureControl) -> Result<SnapshotRenderer, GpuRasterError> {
        let index = Arc::new(SceneIndex::build(&capture.artwork).map_err(GpuRasterError::Color)?);
        let context = capture.artwork.outputs.get(capture.artwork.default_output).ok_or(GpuRasterError::InvalidExtent)?.context.clone();
        let scene = Arc::new(SceneSnapshot::new((*capture.artwork).clone(), index, capture.checkpoint.owner,
            capture.checkpoint.artwork_generation, context));
        SnapshotRenderer::construct(scene, SceneScope::All, control, self)
    }
    pub fn scoped_transfer_artwork(scene:&SceneSnapshot,additional:&[SourceTarget])->layer_core::Artwork {
        let mut required=capture_targets(scene.view(),&scene.scope);
        required.extend_from_slice(additional);
        scene.scoped_transfer_artwork(&required)
    }
    pub fn capture_scene(&self, scene: Arc<SceneSnapshot>, scope: SceneScope, control: CaptureControl) -> Result<SnapshotRenderer, GpuRasterError> {
        SnapshotRenderer::construct(scene, scope, control, self)
    }
}

pub(super) fn capture_targets(scene: SceneView<'_>, scope: &SceneScope) -> Vec<SourceTarget> {
    let contributors = match scope {
        SceneScope::Raw(target) => return vec![*target],
        SceneScope::EffectInput(before) => layer_core::composite_input_layers(scene, *before),
        _ => scene.order().iter().copied().filter(|&h| scene.visible(h)).collect(),
    };
    scene.targets().filter(|target| {
        let Some(owner) = scene.source_owner(*target) else { return false; };
        let needed = contributors.contains(&owner) || target.is_coverage() && contributors.iter().any(|&h| layer_core::descends_from(scene, h, Some(owner)));
        needed && scene.visible(owner) && (!target.is_coverage() || scene.mask(owner).is_some_and(|(use_,_)| use_.enabled))
    }).collect()
}

pub struct SnapshotRenderer {
    scene: Arc<SceneSnapshot>,
    scope: SceneScope,
    offset: layer_core::Point,
    raw_plan: Option<layer_core::TransformPixelsPlan>,
    analysis_ready: std::collections::HashSet<OccurrenceHandle>,
    pub(crate) sdr_rendition: Option<layer_core::color::hdr::SdrRendition>,
    local_tone: Option<Arc<layer_core::color::hdr::LocalToneGuide>>,
    gpu_local_tone: Option<Arc<crate::local_tone::GpuToneGuide>>,
    renderer: WgpuRasterizer,
    backing: HashMap<SourceTarget, Arc<RasterData>>,
    resident: HashMap<SourceTarget, RasterData>,
    extent: [u32; 2],
    #[cfg(not(target_arch = "wasm32"))]
    output_extent: [u32; 2],
    #[cfg(not(target_arch = "wasm32"))]
    output_metadata: layer_color::photo::DeliveryMetadata,
    blend_space: layer_core::BlendSpace,
    time: f32,
    planned_pixel_bytes: u64,
    control: CaptureControl,
}
impl SnapshotRenderer {
    fn construct(scene: Arc<SceneSnapshot>, scope: SceneScope, control: CaptureControl, gpu: &SnapshotGpu) -> Result<Self, GpuRasterError> {
        control.check()?;
        let time = scene.context.elapsed;
        if !time.is_finite() || scene.context.phases.iter().any(|(_,phase)|!phase.is_finite()) {
            return Err(GpuRasterError::Color("Invalid snapshot viewing state".into()));
        }
        let view = scene.view().with_scope(&scope);
        let composition = view.composition();
        let extent = composition.size;
        let color = composition.color;
        let mut backing = HashMap::new();
        for target in capture_targets(view, &scope) {
            if let Some(root) = view.raster(target) {
                control.check()?;
                backing.insert(target, root.wait_data_cancellable(control.cancellation_flag()).map_err(GpuRasterError::Color)?);
            }
        }
        control.check()?;
        let mut renderer = WgpuRasterizer::native_capture_on_gpu(gpu.adapter.clone(), gpu.device.clone(), gpu.queue.clone(), color)?;
        if gpu.device.working_space() == color.space && gpu.device.hdr() == color.depth.is_float() {
            renderer.scene_pipelines = gpu.scene_pipelines.clone(); renderer.scene = None;
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(encoder) = gpu.encoder.clone() { renderer.set_browser_raster_encoder(encoder); }
        #[cfg(target_arch = "wasm32")]
        if let Some(decoder) = gpu.image_decoder.clone() { renderer.set_browser_image_decoder(decoder); }
        #[cfg(target_arch = "wasm32")]
        if let Some(decoder) = gpu.nearest_coordinate_decoder.clone() { renderer.set_browser_nearest_coordinate_decoder(decoder); }

        for &handle in view.order() {
            let Some(effect) = view.effect(handle) else { continue; };
            if let Some(phase) = view.effect_handle(handle).and_then(|h| scene.context.phases.iter().find(|(target, _)| *target == h).map(|(_, phase)| *phase)) {
                renderer.effect_clocks.insert(handle, (effect.program.id.clone(), layer_core::EffectClock::at(effect, time, phase)));
            }
        }
        renderer.submitted_context = Some(scene.context.clone());
        renderer.effect_analyses = gpu.analyses.clone();
        renderer.snapshot_cancelled = Some(control.cancelled.clone());
        #[cfg(target_arch = "wasm32")]
        { renderer.snapshot_decode_wake = Some(control.decode_wake.clone()); }
        renderer.ensure_document_metadata(extent, view)?;
        let sdr_rendition = color.depth.is_float().then_some(view.output().sdr);
        let blend_space = composition.blend;
        #[cfg(not(target_arch = "wasm32"))]
        let output_metadata = layer_color::photo::DeliveryMetadata {resolution: composition.resolution, photo: (*scene.artwork.metadata).clone(), policy: Default::default()};
        Ok(Self {scene, scope, offset: Default::default(), raw_plan: None, analysis_ready: Default::default(), sdr_rendition, local_tone: None, gpu_local_tone: None,
            renderer, backing, resident: HashMap::new(), extent,
            #[cfg(not(target_arch = "wasm32"))] output_extent: extent,
            #[cfg(not(target_arch = "wasm32"))] output_metadata,
            blend_space, time, planned_pixel_bytes: PLANNED_PIXEL_BYTES, control})
    }

    /// One source/composite decision for native streaming writers and browser
    /// worker transport. Exact delivery preserves hidden straight RGB too.
    pub fn output_source(
        &self,
        extent: [u32; 2],
        target: &SourceInterpretation,
        options: layer_core::color::OutputEncoding,
        matte: Option<[f32; 3]>,
    ) -> Option<Arc<layer_core::color::source::SourceImage>> {
        if (self.sdr_rendition.is_some() && !target.depth.is_float()) || extent != self.extent || options.conversion != Default::default() || matte.is_some() {
            return None;
        }
        self.identity_source(target)
    }

    pub fn identity_source(
        &self,
        target: &SourceInterpretation,
    ) -> Option<Arc<layer_core::color::source::SourceImage>> {
        let scene = self.scene.view().with_scope(&self.scope).with_offset(self.offset);
        let mut visible = scene.order().iter().copied().filter(|&h| scene.visible(h)
            && scene.occurrence(h).is_some_and(|o| o.opacity > 0.));
        let handle = visible.next()?;
        let occurrence = scene.occurrence(handle)?;
        let source_target = scene.source_target(handle)?;
        if self.offset != layer_core::Point::default() || visible.next().is_some() || !matches!(source_target, SourceTarget::Paint(_)) || occurrence.opacity != 1.
            || scene.parent(handle).is_some() || occurrence.offset != [0; 2] || occurrence.blend != layer_core::LayerBlend::Normal
            || occurrence.attachment.is_clip() || occurrence.mask.as_ref().is_some_and(|m| m.enabled) || !self.backing[&source_target].tiles.is_empty() { return None; }
        let base = scene.paint_base(source_target)?;
        let source = base.image.storage();
        (base.offset == [0;2] && source.extent == self.extent
            && source.interpretation.channels == target.channels
            && source.interpretation.depth == target.depth
            && source.interpretation.profile == target.profile)
            .then(|| source.clone())
    }

    pub fn extent(&self) -> [u32; 2] {
        self.extent
    }
    pub fn capture_window(&mut self, origin: [i64; 2], extent: [u32; 2]) -> Result<(), GpuRasterError> {
        Arc::make_mut(&mut self.scene).offset = origin.map(|v| -(v as f64));
        self.extent = extent;
        #[cfg(not(target_arch = "wasm32"))]
        { self.output_extent = extent; }
        self.renderer.ensure_document_metadata(extent, self.scene.view().with_scope(&self.scope).with_offset(self.offset)).map(|_| ())
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
        pollster::block_on(self.histogram_async())
    }
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

    #[cfg(not(target_arch = "wasm32"))]
    fn read_band(&mut self, y: u32) -> Result<(u32, Vec<[f32; 4]>), GpuRasterError> {
        pollster::block_on(self.read_band_async(y))
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn read_window_band(&mut self, window: [u32; 4], y: u32) -> Result<(u32, Vec<[f32; 4]>), GpuRasterError> {
        pollster::block_on(self.read_window_band_async(window, y))
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn selection_coverage(
        &mut self,
        selection: &Arc<layer_core::Selection>,
        window: [u32; 4],
    ) -> Result<SelectionCoverage, GpuRasterError> {
        pollster::block_on(self.selection_coverage_async(selection, window))
    }

    /// A file worker shares the canvas queue. Complete at most two tile
    /// columns before yielding it through readback; a full 8K-wide effect band
    /// otherwise blocks presentation for several refresh intervals. Preserve
    /// the row-band cache and exact pixel/halo semantics of read_region.
    #[cfg(not(target_arch = "wasm32"))]
    fn read_interactive_band(&mut self, [left, width]: [u32; 2], y: u32, rows: u32) -> Result<Vec<[f32; 4]>, GpuRasterError> {
        let band_bytes = u64::from(width) * u64::from(rows) * 16;
        if band_bytes > self.planned_pixel_bytes {
            return Err(GpuRasterError::CaptureBudget { required: band_bytes, limit: self.planned_pixel_bytes });
        }
        let mut band = vec![[0.; 4]; width as usize * rows as usize];
        for x in (0..width).step_by(512) {
            self.check_cancelled()?;
            let columns = 512.min(width - x);
            // The assembled CPU band stays alive beside each bounded GPU job.
            self.planned_pixel_bytes -= band_bytes;
            let result = self.read_region([left + x, y, columns, rows]);
            self.planned_pixel_bytes += band_bytes;
            let pixels = result?;
            for (row, source) in pixels.chunks_exact(columns as usize).enumerate() {
                let start = row * width as usize + x as usize;
                band[start..start + columns as usize].copy_from_slice(source);
            }
        }
        Ok(band)
    }

    /// Exact linear-premultiplied document RGB. No display conversion, proof,
    /// mask-area tint, checkerboard or UI overlays participate. Waits on this
    /// capture only; call from the owning file/inspection worker.
    async fn capture_region_gpu<T>(
        &mut self,
        [x, y, width, height]: [u32; 4],
        reserved_bytes: u64,
        consume: impl FnOnce(&PipelineDevice, &wgpu::Texture, &mut submission::CommandEncoder) -> T,
    ) -> Result<T, GpuRasterError> {
        self.capture_output_region_gpu([x, y, width, height], scene::Output::Artwork(None), reserved_bytes, consume).await
    }
    async fn capture_output_region_gpu<T>(&mut self, [x,y,width,height]: [u32;4], output: scene::Output, reserved_bytes:u64,
        consume: impl FnOnce(&PipelineDevice, &wgpu::Texture, &mut submission::CommandEncoder) -> T,
    ) -> Result<T, GpuRasterError> {
        let output=match (&self.scope,output) {
            (SceneScope::Raw(target),scene::Output::Artwork(None))=>scene::Output::Source(*target),
            (SceneScope::RawObjects(handle),scene::Output::Artwork(None))=>scene::Output::Objects(*handle),_=>output,
        };
        let mut consume=Some(consume);
        self.with_region_gpu([x, y, width, height], reserved_bytes, |r, packet, region, encoder| {
            let (target, _) = create_color_target(&r.device, [width, height], "snapshot region");
            let mut scene = r.scene.take().unwrap_or_else(|| scene::Scene::new(r));
            let captured = scene.capture_region(r, packet, &target, region, output, encoder);
            r.scene = Some(scene);
            captured?;
            Ok(consume.take().unwrap()(&r.device, &target, encoder))
        }).await
    }

    async fn with_region_gpu<T>(
        &mut self, region: [u32; 4], reserved_bytes: u64,
        mut consume: impl FnMut(&mut WgpuRasterizer, FramePacket<'_>, PixelRect, &mut submission::CommandEncoder) -> Result<T, GpuRasterError>,
    ) -> Result<T, GpuRasterError> {
        loop {
            let result={
                let (r, packet, region, mut encoder)=self.prepare_region_gpu(region,reserved_bytes)?;
                match consume(r,packet,region,&mut encoder) {
                    Ok(result)=> {
                        if let Some(scene)=r.scene.as_mut() {scene.release_exact_objects(&encoder);}
                        r.uploads.finish(&encoder);
                        encoder.submit(&r.queue);
                        Ok(result)
                    },
                    Err(GpuRasterError::DeferredObjectWork)=> {
                        r.uploads.finish(&encoder);
                        encoder.submit(&r.queue);
                        crate::local_tone::wait_async(&r.device,&r.queue).await.map_err(GpuRasterError::Color)?;
                        Err(GpuRasterError::DeferredObjectWork)
                    },
                    Err(error)=>Err(error),
                }
            };
            match result {
                Err(GpuRasterError::DeferredObjectWork)=> {
                    let r=&mut self.renderer;
                    let mut scene=r.scene.take().unwrap_or_else(||scene::Scene::new(r));
                    let result=scene.drain_exact_objects_async(r).await;
                    r.scene=Some(scene);
                    result?;
                    self.control.check()?;
                },
                result=> {
                    self.control.observe_allocations(&self.renderer.device);
                    return result;
                },
            }
        }
    }

    fn prepare_region_gpu(&mut self, [x,y,width,height]:[u32;4], reserved_bytes:u64)
        -> Result<(&mut WgpuRasterizer, FramePacket<'_>, PixelRect, submission::CommandEncoder),GpuRasterError> {
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
        let view = self.scene.view().with_scope(&self.scope).with_offset(self.offset);
        let window = scene::Scene::capture_window(view, region, self.extent);
        // Composition operates in page-sized tiles, including translated masks
        // and neighboring watercolor pigment. Restore their complete footprints.
        let mut selected = HashMap::new();
        let mut masks = HashMap::new();
        let material_pages = if self.backing.values().any(|data| data.watercolor.is_some()) {
            scene::Scene::MATERIAL_CACHE_PAGES as u64
        } else { 0 };
        let mut planned = scene::Scene::capture_image_bound(view, window)
            .saturating_add(scene::Scene::geometry_bytes(self.renderer.scene.as_ref()))
            .saturating_add(reserved_bytes)
            .saturating_add(self.renderer.analysis_bytes())
            .saturating_add(region.area().saturating_mul(32)) // output and mapping
            .saturating_add((view.order().len() as u64 * 3 + 32 + material_pages) * 256 * 256 * 16);
        for id in self.backing.keys().copied() {
                let mask = id.is_coverage();
                let extent = view.target_extent(id);
                let original = &self.backing[&id];
                let halo = if mask { 0. } else { original.watercolor.map_or(0., |style| 2. * style.edge_width.clamp(1., 16.)) };
                let local = match &self.raw_plan {
                    Some(plan) => {
                        let geometry = plan.plane_geometry(id);
                        paint_transform::snapshot::source_region(&geometry, window.to_rect().outset(halo), extent,
                            self.renderer.scene.as_ref().and_then(|scene| scene.mesh_geometry(&geometry)))?
                    }
                    None => window.expand(halo.ceil() as u32).translated(view.target_offset(id).map(|v| -v)).in_frame(extent),
                }.expand(if mask { 1 } else { PAGE_SIZE }, extent);
                if mask {
                    masks.insert(id, local);
                }
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
        if planned > self.planned_pixel_bytes {
            return Err(GpuRasterError::CaptureBudget {
                required: planned,
                limit: self.planned_pixel_bytes,
            });
        }
        let r = &mut self.renderer;
        if let Some(scene) = &mut r.scene {
            scene.release_capture_window(window);
        }
        // These are disposable read-only caches. Evict obsolete pages before
        // allocating replacements, instead of temporarily retaining both windows.
        let retained = |id: SourceTarget, plane, coordinate| {
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
            for tile in data.tiles.values() {
                tile.wait_backing_cancellable(self.control.cancellation_flag()).map_err(GpuRasterError::Color)?;
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
            commit_rasters: true,
            view: layer_render::ViewState {
                width_px: width,
                height_px: height,
                document_to_surface: [1., 0., 0., 1., 0., 0.],
            },
            document_extent: self.extent,
            scene: view,
            selection_overlays: None,
            inspect_mask: None,
            time_seconds: self.time,
            dabs: &[],
            dab_batches: &[],
            restore_rasters: &[],
            reset_layers: false,
            composite_all: true,
            blend_space: self.blend_space,
        };
        let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
        r.prepare_uploads(packet, &mut [], &mut encoder)?;
        r.layer_masks.prepare_regions(
            &r.device,
            &mut encoder,
            (view, &[]),
            self.extent,
            false,
            &mut r.selection_clip,
            Some(&masks),
        )?;
        Ok((r, packet, region, encoder))
    }

    async fn capture_query_gpu(&mut self, output:scene::Output, preview:bool, tiles_per_submission:u32, reserved:u64,
        selection:Option<&Arc<layer_core::Selection>>,
        mut consume:impl FnMut(&mut WgpuRasterizer,&wgpu::TextureView,PixelRect,&mut submission::CommandEncoder)->Result<(),GpuRasterError>,
    )->Result<(),GpuRasterError> {
        self.prepare_effect_analysis_async(output).await.map_err(GpuRasterError::Effect)?;
        let extent=self.extent;let control=self.control.clone();
        for y in (0..extent[1]).step_by(1024) {for x in (0..extent[0]).step_by(1024) {
            let mut regions=vec![[x,y,(extent[0]-x).min(1024),(extent[1]-y).min(1024)]];
            while let Some(region)=regions.pop() {
                control.check()?;
                let work=async {
                    let (r,packet,region,mut encoder)=self.prepare_region_gpu(region,reserved)?;
                    if let Some(selection)=selection {r.selection_clip.prepare_region(&r.device,&mut encoder,extent,selection,Some(region))?;}
                    let mut scene=r.scene.take().unwrap_or_else(||scene::Scene::new(r));
                    let result=scene.capture_query_tiles(r,packet,region,output,preview,tiles_per_submission,&mut encoder,|r,view,region,encoder| {
                        control.check()?;consume(r,view,region,encoder)
                    }).await;
                    r.scene=Some(scene);result?;
                    r.uploads.finish(&encoder);encoder.submit(&r.queue);
                    control.observe_allocations(&r.device);
                    crate::local_tone::wait_async(&r.device,&r.queue).await.map_err(GpuRasterError::Color)
                }.await;
                match work {
                    Err(GpuRasterError::CaptureBudget {..}) if region[2].max(region[3])>16=> {
                        let axis=usize::from(region[3]>region[2]);let mut first=region;first[axis+2]/=2;
                        let mut second=region;second[axis]+=first[axis+2];second[axis+2]-=first[axis+2];regions.extend([second,first]);
                    },
                    result=>result?,
                }
            }
        }}
        control.check()
    }

    async fn prepare_region(&mut self, region: [u32; 4]) -> Result<RegionReadback, GpuRasterError> {
        let [_, _, width, height] = region;
        self.capture_region_gpu(region, 0, |device, target, encoder| {
            let stride = (width * 16).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
            let size = stride as u64 * height as u64;
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
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
            RegionReadback { buffer, stride, width, height }
        }).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_region(&mut self, region: [u32; 4]) -> Result<Vec<[f32; 4]>, GpuRasterError> {
        pollster::block_on(self.read_region_async(region))
    }

    /// Yields to WebGPU while mapping one bounded Float32 region. Callers await
    /// raster backing before creating the immutable capture, and await each
    /// region before submitting the next; no blocking browser device poll.
    pub async fn read_region_async(
        &mut self,
        region: [u32; 4],
    ) -> Result<Vec<[f32; 4]>, GpuRasterError> {
        self.prepare_effect_analysis_async(scene::Output::Artwork(None)).await.map_err(GpuRasterError::Effect)?;
        let readback = self.prepare_region(region).await?;
        #[cfg(not(target_arch = "wasm32"))]
        let (tx, rx) = mpsc::channel();
        #[cfg(target_arch = "wasm32")]
        let (tx, rx) = futures_channel::oneshot::channel();
        readback
            .buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result.map_err(|e| e.to_string()));
            });
        #[cfg(not(target_arch = "wasm32"))]
        crate::raster::wait_mapping(&self.renderer.device, &rx)
            .map_err(GpuRasterError::MapFailed)?;
        #[cfg(target_arch = "wasm32")]
        rx.await
            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?
            .map_err(GpuRasterError::MapFailed)?;
        self.finish_region(readback)
    }

    /// Capture complete tile rows when the dependency budget permits. Sixteen
    /// separate captures of one tile row repeat composition, source decoding and
    /// mapping. The CPU band is at most 32 MiB; planning includes its GPU target
    /// and readback copy. Complex dependencies shrink the band before GPU work.
    pub async fn read_band_async(
        &mut self,
        y: u32,
    ) -> Result<(u32, Vec<[f32; 4]>), GpuRasterError> {
        let [width, height] = self.extent;
        self.read_window_band_async([0, 0, width, height], y).await
    }

    /// `read_band_async` within `[x, y, width, height]` of the document;
    /// `y` is a document row inside the window.
    pub async fn read_window_band_async(
        &mut self,
        [left, top, width, height]: [u32; 4],
        y: u32,
    ) -> Result<(u32, Vec<[f32; 4]>), GpuRasterError> {
        let bottom = top.checked_add(height).ok_or(GpuRasterError::SizeOverflow)?;
        if y < top || y >= bottom || width == 0 || left.saturating_add(width) > self.extent[0] || bottom > self.extent[1] {
            return Err(GpuRasterError::InvalidExtent);
        }
        let maximum = if self.color().depth.is_float() {
            (4 * 1024 * 1024 / (width * 16)).clamp(16, 64)
        } else { (32 * 1024 * 1024 / (width * 16)).clamp(16, PAGE_SIZE) };
        let mut rows = maximum.min(bottom - y);
        loop {
            #[cfg(not(target_arch = "wasm32"))]
            let result = if width > 512 {
                self.read_interactive_band([left, width], y, rows)
            } else {
                self.read_region_async([left, y, width, rows]).await
            };
            #[cfg(target_arch = "wasm32")]
            let result = self.read_region_async([left, y, width, rows]).await;
            match result {
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
            for pixel in row[..width as usize * 16].as_chunks::<16>().0.iter() {
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

impl SnapshotRenderer {
    /// The selection's coverage of `[x, y, width, height]`, rasterized by the
    /// same GPU rules that clip brushes and clear pixels.
    pub async fn selection_coverage_async(
        &mut self,
        selection: &Arc<layer_core::Selection>,
        [x, y, width, height]: [u32; 4],
    ) -> Result<SelectionCoverage, GpuRasterError> {
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
        let r = &mut self.renderer;
        let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
        r.selection_clip.prepare_region(&r.device, &mut encoder, self.extent, selection, Some(region))?;
        let packed = r.selection_clip.buffer.as_ref().ok_or(GpuRasterError::InvalidExtent)?;
        let readback = r.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("selection coverage readback"),
            size: packed.size(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(packed, 0, &readback, 0, packed.size());
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        r.selection_clip.reset();
        #[cfg(not(target_arch = "wasm32"))]
        let (tx, rx) = mpsc::channel();
        #[cfg(target_arch = "wasm32")]
        let (tx, rx) = futures_channel::oneshot::channel();
        readback.slice(..).map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result.map_err(|e| e.to_string()));
        });
        #[cfg(not(target_arch = "wasm32"))]
        crate::raster::wait_mapping(&self.renderer.device, &rx).map_err(GpuRasterError::MapFailed)?;
        #[cfg(target_arch = "wasm32")]
        rx.await
            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?
            .map_err(GpuRasterError::MapFailed)?;
        let words: Vec<u32> = readback
            .slice(..)
            .get_mapped_range()
            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?
            .as_chunks::<4>().0.iter()
            .map(|b| u32::from_ne_bytes(*b))
            .collect();
        readback.unmap();
        self.check_cancelled()?;
        SelectionCoverage::new(words).ok_or(GpuRasterError::InvalidExtent)
    }
}

/// Packed selection coverage read back from the GPU, sampled exactly as
/// `brush_selection_at` samples it.
pub struct SelectionCoverage {
    rect: [u32; 4],
    inverted: bool,
    bytes: bool,
    offset: [f32; 2],
    stride: usize,
    words: Vec<u32>,
}
impl SelectionCoverage {
    fn new(mut words: Vec<u32>) -> Option<Self> {
        let header: [u32; 8] = words.get(..8)?.try_into().ok()?;
        let bytes = header[5] == 2;
        let count = if bytes { 4 } else { 8 };
        let stride = header[2].div_ceil(count) as usize;
        words.drain(..8);
        words.truncate(stride * header[3] as usize);
        (words.len() == stride * header[3] as usize).then_some(Self {
            rect: header[..4].try_into().ok()?,
            inverted: header[4] != 0,
            bytes,
            offset: [f32::from_bits(header[6]), f32::from_bits(header[7])],
            stride,
            words,
        })
    }
    /// Coverage of the document pixel whose top-left corner is `[x, y]`.
    pub fn at(&self, x: u32, y: u32) -> f32 {
        let [left, top, width, height] = self.rect.map(i64::from);
        let px = (x as f32 + 0.5 - self.offset[0]).floor() as i64 - left;
        let py = (y as f32 + 0.5 - self.offset[1]).floor() as i64 - top;
        let mut coverage = 0.;
        if (0..width).contains(&px) && (0..height).contains(&py) {
            let (shift, bits, mask, scale) = if self.bytes { (2, 8, 255, 1. / 255.) } else { (3, 4, 15, 0.25) };
            let word = self.words[py as usize * self.stride + (px >> shift) as usize];
            coverage = ((word >> ((px as u32 & ((1 << shift) - 1)) * bits)) & mask) as f32 * scale;
        }
        if self.inverted { 1. - coverage } else { coverage }
    }
    /// Multiply premultiplied pixels of document row `y`, starting at column `x`.
    pub fn apply(&self, x: u32, y: u32, row: &mut [[f32; 4]]) {
        for (i, pixel) in row.iter_mut().enumerate() {
            let coverage = self.at(x + i as u32, y);
            for channel in pixel {
                *channel *= coverage;
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod clip;
#[cfg(not(target_arch = "wasm32"))]
mod flatten;
#[cfg(not(target_arch = "wasm32"))]
mod output;
mod tone;
mod package_preview;
mod analysis;
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
        self.encoded_bytes(layer_core::color::RgbSpace::Srgb)
    }
    /// Straight-alpha UI bytes; the host must tag the image with `space`.
    pub fn encoded_bytes(&self, space: layer_core::color::RgbSpace) -> Result<Vec<u8>, String> {
        let target = SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: layer_core::color::SampleDepth::U8,
            profile: layer_core::color::ColorProfile::Builtin(space),
            profile_assumed: false,
        };
        let encoder = layer_color::WorkingEncoder::new(self.space, &target, Default::default())?;
        let mut bytes = vec![0; self.pixels.len() * 4];
        encoder.encode_premultiplied(&self.pixels, &mut bytes, None, [0, 0])?;
        Ok(bytes)
    }
}

mod bounds;
pub(crate) mod sample;
pub(crate) mod statistics;
pub(crate) mod levels;
mod transform_pixels;
mod image_capture;
mod resample_image;
mod jobs;
#[cfg(test)]
#[path="query_capture_tests.rs"]
mod query_capture_tests;
pub use jobs::SnapshotJob;
#[cfg(target_arch = "wasm32")]
pub use jobs::BrowserSnapshot;
