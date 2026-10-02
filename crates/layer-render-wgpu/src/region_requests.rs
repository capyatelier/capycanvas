//! Single-flight region queries. Keep the GPU result for rendering and capture
//! one packed history copy asynchronously, with no CPU flood/raster algorithm.
//! Refinements run across frames, one bounded chunk per poll.
use super::*;
use flood::{Flood, Region};
use layer_render::{RegionRequest, RegionResult};
use selection_refine::{Downsample, Job, ModifyPlan, SelectionRefiner, Stage};
use std::sync::{Arc, Weak};

pub(super) struct RegionRequests {
    pub(super) flood: Flood,
    pub(super) raw: region_sources::RawRegions,
    refiner: Option<selection_refine::SelectionRefiner>,
    readback: Option<wgpu::Buffer>,
    pending: Option<Region>,
    waiting: Option<RegionRequest>,
    running: Option<Running>,
    /// Rasterizes a warp mesh for selections resampled through it.
    positions: Option<paint_transform::mesh::Positions>,
    rx: Option<mpsc::Receiver<Result<RegionResult, GpuRasterError>>>,
    modify_source: Option<ModifySource>,
}
/// The selection Select › Modify refines, prepared at document resolution,
/// and the levels of its preview pyramid built so far, each halving the last
/// over `region`.
struct ModifySource {
    selection: Weak<layer_core::Selection>,
    extent: [u32; 2],
    document: wgpu::Buffer,
    region: [u32; 4],
    levels: Vec<wgpu::Buffer>,
}
struct Running {
    job: Job,
    request_id: u64,
    probe: Option<layer_render::TonalProbe>,
    placement: layer_core::Affine,
    /// Pyramid levels this job builds, for later previews.
    levels: Vec<wgpu::Buffer>,
}
impl RegionRequests {
    pub fn storage_bytes(&self) -> u64 {
        self.flood.storage_bytes()
            + self.raw.storage_bytes()
            + self.readback.as_ref().map_or(0, |b| b.size())
            + self.pending.as_ref().map_or(0, |r| r.coverage.size())
            + self.running.as_ref().map_or(0, |r| r.job.storage_bytes())
            + self.modify_source.as_ref().map_or(0, |s| s.document.size() + s.levels.iter().map(wgpu::Buffer::size).sum::<u64>())
    }
    pub(super) fn new(device: &PipelineDevice) -> Self {
        Self {
            flood: Flood::new(device),
            raw: region_sources::RawRegions::new(device),
            refiner: None,
            readback: None,
            pending: None,
            waiting: None,
            running: None,
            positions: None,
            rx: None,
            modify_source: None,
        }
    }
    /// Drop the request in progress; a readback already in flight is ignored.
    pub(super) fn cancel(&mut self) {
        self.waiting = None;
        self.running = None;
        if self.pending.take().is_some() {
            self.rx = None;
            self.readback = None;
        }
    }
    fn refiner(&mut self, r: &WgpuRasterizer) -> &mut selection_refine::SelectionRefiner {
        self.refiner.get_or_insert_with(|| selection_refine::SelectionRefiner::new(&r.device, &r.queue))
    }
    fn positions(&mut self, r: &WgpuRasterizer) -> &mut paint_transform::mesh::Positions {
        self.positions.get_or_insert_with(|| {
            r.transforms.as_ref().map_or_else(
                || paint_transform::mesh::Positions::new(&r.device),
                paint_transform::mesh::Positions::sharing,
            )
        })
    }
    /// A pixel selection's coverage resampled through `map` into `extent`,
    /// through a warp mesh a window of positions at a time.
    fn resample_mapped(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        extent: [u32; 2],
        selection: &layer_core::Selection,
        map: &layer_core::LayerPlacement,
    ) -> Result<wgpu::Buffer, GpuRasterError> {
        use paint_transform::mesh::{MeshGeometry, WINDOW_PAGES};
        let source = r.selection_clip.mapped_source(&r.device, selection, map)?;
        let [w, h] = extent;
        let bytes = 32 + u64::from(w.div_ceil(4)) * u64::from(h) * 4 + 32;
        let limits = r.device.limits();
        let mesh = map.mesh.as_ref();
        if extent.contains(&0)
            || bytes > limits.max_storage_buffer_binding_size
            || (mesh.is_none() && h > limits.max_compute_workgroups_per_dimension)
        {
            return Err(GpuRasterError::SizeOverflow);
        }
        let output = r.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mapped selection coverage"),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut header = [0u8; 32];
        for (dst, value) in header.chunks_exact_mut(4).zip([0, 0, w, h, 0, 2, 0, 0]) {
            dst.copy_from_slice(&value.to_ne_bytes());
        }
        r.uploads.write(encoder, &output, &header)?;
        let Some(mesh) = mesh else {
            r.selection_clip.resample_window(&r.device, encoder, &source, &output, PixelRect::full(extent), None);
            return Ok(output);
        };
        let geometry = std::sync::Arc::new(MeshGeometry::new(mesh, map.outer, None));
        let side = WINDOW_PAGES * PAGE_SIZE;
        let positions = self.positions(r);
        positions.upload(r, encoder, &geometry)?;
        let view = positions.view(&r.device, [side; 2]);
        let drawn = pixel_rect(map.forward_bounds(mesh.bounds()), extent);
        for y in (drawn.min_y() / side..drawn.max_y().div_ceil(side)).map(|n| n * side) {
            for x in (drawn.min_x() / side..drawn.max_x().div_ceil(side)).map(|n| n * side) {
                positions.draw(r, encoder, [x / PAGE_SIZE, y / PAGE_SIZE])?;
                let rect = PixelRect::new(x, y, (x + side).min(w), (y + side).min(h));
                r.selection_clip.resample_window(&r.device, encoder, &source, &output, rect, Some((&view, w)));
            }
        }
        Ok(output)
    }
    fn start(
        &mut self,
        r: &mut WgpuRasterizer,
        request: RegionRequest,
    ) -> Result<bool, GpuRasterError> {
        if self.pending.is_some() || self.waiting.is_some() || self.running.is_some() {
            return Ok(false);
        }
        if let layer_render::RegionSource::Modify(modify) = &request.source {
            let modify = modify.clone();
            return self.start_modify(r, request, &modify);
        }
        let extent = match request.source.raw_source() { layer_render::RegionSource::TransformedSelection {layer,..} => r.target_extent(*layer), _ => r.document_extent };
        let mapped = matches!(
            request.source,
            layer_render::RegionSource::TransformedSelection { .. }
        );
        let tone = if let layer_render::RegionSource::Tonal(t) = &request.source {
            Some(t.as_ref())
        } else {
            None
        };
        if tone.is_some_and(|t| !t.valid(extent))
            || extent.contains(&0)
            || request.position[0] >= extent[0]
            || request.position[1] >= extent[1]
            || !request.tolerance.is_finite()
            || !(0.0..=1.).contains(&request.tolerance)
            || !request.refinement.is_valid()
            || request.selection.as_ref().is_some_and(|s| !s.is_valid())
            || (matches!(
                request.source,
                layer_render::RegionSource::Selection(_)
                    | layer_render::RegionSource::Coverage(_)
                    | layer_render::RegionSource::Tonal(_)
            ) && request.selection.is_none())
            || (mapped && (request.selection.is_some() || request.limit.is_some()))
        {
            return Err(GpuRasterError::InvalidExtent);
        }
        if mapped || request.selection.is_some() {
            self.refiner(r);
        }
        if let Some(startup) = &r.startup {
            startup.compiler.check()?;
            let mut ready = true;
            if let layer_render::RegionSource::TransformedSelection { map, .. } = &request.source {
                if map.mesh.is_some() {
                    ready &= startup.compiler.require([&self.positions(r).pipeline], startup::BRUSH);
                }
                ready &= startup.compiler.require([&r.selection_clip.resample], startup::BRUSH);
                ready &= self.refiner.as_ref().unwrap().prepare_bounds(&startup.compiler);
            } else if tone.is_some() {
                ready &= self.raw.prepare_tonal(&startup.compiler);
            } else if !matches!(request.source, layer_render::RegionSource::Selection(_)) {
                if !matches!(request.source, layer_render::RegionSource::Coverage(_)) {
                    ready &= self.flood.prepare(&startup.compiler, request.refinement);
                }
                ready &= self.raw.prepare(&startup.compiler);
            }
            if let Some(options) = &request.selection {
                ready &= self
                    .refiner
                    .as_ref()
                    .unwrap()
                    .prepare(&startup.compiler, [&Stage::refinement(options)], false);
            }
            if request.limit.is_some() || request.selection.is_some() {
                ready &= startup
                    .compiler
                    .require(r.selection_clip.pipelines(), startup::BRUSH);
            }
            if !ready {
                self.waiting = Some(request);
                return Ok(true);
            }
        }
        let mut encoder = crate::submission::CommandEncoder::new(
            &r.device,
            &wgpu::CommandEncoderDescriptor {
                label: Some("connected region request"),
            },
        );
        if let Some(selection) = &request.limit {
            r.selection_clip
                .prepare(&r.device, &mut encoder, extent, selection)?;
        }
        let input = if let layer_render::RegionSource::TransformedSelection {
            selection, map, ..
        } = &request.source
        {
            flood::Region {
                coverage: self.resample_mapped(r, &mut encoder, extent, selection, map)?,
                bounds_offset: 0,
            }
        } else if let layer_render::RegionSource::Selection(selection) = &request.source {
            r.selection_clip
                .prepare(&r.device, &mut encoder, extent, selection)?;
            let buffer = r.selection_clip.buffer.as_ref().unwrap();
            // The next prepare may reuse the clip allocation for the old mask.
            let copy = r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("incoming selection"),
                size: buffer.size(),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(buffer, 0, &copy, 0, buffer.size());
            flood::Region {
                coverage: copy,
                bounds_offset: 0,
            }
        } else {
            let classified = self.raw.encode(r, &request, &mut encoder)?;
            if tone.is_some() || matches!(request.source, layer_render::RegionSource::Coverage(_)) {
                flood::Region {
                    coverage: classified,
                    bounds_offset: 0,
                }
            } else {
                self.flood.encode_input(
                    &r.device,
                    &mut encoder,
                    extent,
                    request.position,
                    request.tolerance,
                    request.limit.as_ref().and(r.selection_clip.buffer.as_ref()),
                    request.refinement,
                    &classified,
                    request.contiguous,
                )?
            }
        };
        let direct_tonal = tone.is_some()
            && extent == r.document_extent
            && request.selection.as_ref().is_some_and(|s| {
                s.mode == layer_core::SelectionMode::New
                    && s.antialias
                    && s.feather == 0.
                    && s.resize == 0
                    && s.source_to_document == layer_core::Affine::IDENTITY
            });
        let (region, extent, byte_coverage) = if mapped {
            (
                self.refiner.as_ref().unwrap().bounds_only(
                    &r.device,
                    &mut encoder,
                    extent,
                    input.coverage,
                ),
                extent,
                true,
            )
        } else if direct_tonal {
            self.raw.release_mask();
            (
                self.refiner.as_ref().unwrap().bounds_only(
                    &r.device,
                    &mut encoder,
                    extent,
                    input.coverage,
                ),
                extent,
                true,
            )
        } else if let Some(options) = &request.selection {
            let extent = r.document_extent;
            let external = match &options.previous {
                Some(previous) => {
                    r.selection_clip.prepare(&r.device, &mut encoder, extent, previous)?;
                    Some(Self::copy(&r.device, &mut encoder, r.selection_clip.buffer.as_ref().unwrap(), "previous selection"))
                }
                None => None,
            };
            let [w, h] = extent;
            let job = self.refiner.as_ref().unwrap().job(
                &r.device,
                &mut encoder,
                extent,
                [0, 0, w, h],
                vec![Stage::refinement(options)],
                input.coverage,
                external,
                Vec::new(),
            )?;
            let probe = tone.and_then(|t| t.probe);
            let running = Running { job, request_id: request.request_id, probe, placement: layer_core::Affine::IDENTITY, levels: Vec::new() };
            self.advance(r, encoder, running)?;
            return Ok(true);
        } else {
            (input, extent, false)
        };
        let probe = tone.and_then(|t| t.probe);
        self.finish(r, encoder, region, extent, byte_coverage, request.request_id, probe, layer_core::Affine::IDENTITY);
        Ok(true)
    }
    fn copy(device: &wgpu::Device, encoder: &mut crate::submission::CommandEncoder, buffer: &wgpu::Buffer, label: &'static str) -> wgpu::Buffer {
        let copy = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: buffer.size(),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(buffer, 0, &copy, 0, buffer.size());
        copy
    }
    fn start_modify(
        &mut self,
        r: &mut WgpuRasterizer,
        request: RegionRequest,
        modify: &Arc<layer_render::SelectionModify>,
    ) -> Result<bool, GpuRasterError> {
        let extent = r.document_extent;
        if !modify.is_valid() || extent.contains(&0) {
            return Err(GpuRasterError::InvalidExtent);
        }
        let selection = &modify.selection;
        if self
            .modify_source
            .as_ref()
            .is_some_and(|s| s.extent != extent || s.selection.upgrade().is_none_or(|s| !Arc::ptr_eq(&s, selection)))
        {
            self.modify_source = None;
        }
        let built = self.modify_source.as_ref().map_or(0, |s| s.levels.len() as u32);
        let preview = self.refiner(r).preview();
        let plan = ModifyPlan::new(modify, extent, preview, built);
        if let Some(startup) = &r.startup {
            startup.compiler.check()?;
            let ready = self.refiner.as_ref().unwrap().prepare(&startup.compiler, &plan.stages, plan.level > built)
                & startup.compiler.require(r.selection_clip.pipelines(), startup::BRUSH);
            if !ready {
                self.waiting = Some(request);
                return Ok(true);
            }
        }
        let mut encoder = crate::submission::CommandEncoder::new(
            &r.device,
            &wgpu::CommandEncoderDescriptor {
                label: Some("modify selection"),
            },
        );
        if self.modify_source.is_none() {
            r.selection_clip.prepare(&r.device, &mut encoder, extent, selection)?;
            self.modify_source = Some(ModifySource {
                selection: Arc::downgrade(selection),
                extent,
                document: Self::copy(&r.device, &mut encoder, r.selection_clip.buffer.as_ref().unwrap(), "selection to modify"),
                region: selection_refine::pyramid_region(modify, extent),
                levels: Vec::new(),
            });
        }
        let source = self.modify_source.as_ref().unwrap();
        let [x0, y0, x1, y1] = source.region;
        let mut downsamples: Vec<Downsample> = Vec::new();
        for level in built + 1..=plan.level {
            let scale = 1 << level;
            let origin = [x0 / scale, y0 / scale];
            let cells = [x1.div_ceil(scale) - origin[0], y1.div_ceil(scale) - origin[1]];
            let (input, bounds, from) = match level {
                1 => (source.document.clone(), extent, [x0, y0]),
                _ => (
                    downsamples.last().map_or_else(|| source.levels[level as usize - 2].clone(), |d| d.output.clone()),
                    extent.map(|v| v.div_ceil(scale / 2)),
                    origin.map(|v| v * 2),
                ),
            };
            downsamples.push(Downsample {
                input,
                bounds,
                origin: from,
                output: SelectionRefiner::coverage_buffer(&r.device, &mut encoder, origin, cells, false, "selection pyramid level"),
                extent: cells,
                row: 0,
            });
        }
        let levels: Vec<_> = downsamples.iter().map(|d| d.output.clone()).collect();
        let input = match plan.level {
            0 => source.document.clone(),
            level => levels.last().cloned().unwrap_or_else(|| source.levels[level as usize - 1].clone()),
        };
        let job = self.refiner.as_ref().unwrap().job(
            &r.device,
            &mut encoder,
            plan.grid,
            plan.active,
            plan.stages.clone(),
            input,
            None,
            downsamples,
        )?;
        let running = Running {
            job,
            request_id: request.request_id,
            probe: None,
            placement: plan.placement(),
            levels,
        };
        self.advance(r, encoder, running)?;
        Ok(true)
    }
    /// Encode and submit the next chunk of `running`, or its readback once
    /// the job is complete.
    fn advance(
        &mut self,
        r: &mut WgpuRasterizer,
        mut encoder: crate::submission::CommandEncoder,
        mut running: Running,
    ) -> Result<(), GpuRasterError> {
        let refiner = self.refiner.as_mut().unwrap();
        let timing = refiner.begin_timing(&mut encoder);
        let region = match refiner.advance(&r.device, &mut encoder, &mut running.job) {
            Ok(region) => region,
            Err(error) => {
                refiner.timed(&r.queue, timing);
                return Err(error);
            }
        };
        refiner.end_timing(&mut encoder, timing, running.job.taps());
        let Some(region) = region else {
            running.job.track(&encoder);
            r.uploads.finish(&encoder);
            encoder.submit(&r.queue);
            self.refiner.as_mut().unwrap().timed(&r.queue, timing);
            self.running = Some(running);
            return Ok(());
        };
        if let Some(source) = &mut self.modify_source {
            source.levels.extend(running.levels);
        }
        let extent = running.job.extent();
        self.finish(r, encoder, region, extent, true, running.request_id, running.probe, running.placement);
        self.refiner.as_mut().unwrap().timed(&r.queue, timing);
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn finish(
        &mut self,
        r: &mut WgpuRasterizer,
        mut encoder: crate::submission::CommandEncoder,
        region: Region,
        extent: [u32; 2],
        byte_coverage: bool,
        request_id: u64,
        probe: Option<layer_render::TonalProbe>,
        placement: layer_core::Affine,
    ) {
        let coverage_size = region.bounds_offset;
        let mask_size = coverage_size + 32;
        let size = mask_size
            + if probe.is_some() {
                self.raw.tonal_statistics.size()
            } else {
                0
            };
        if self.readback.as_ref().is_none_or(|b| b.size() < size) {
            self.readback = Some(r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("region history snapshot"),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let readback = self.readback.as_ref().unwrap();
        encoder.copy_buffer_to_buffer(&region.coverage, 0, readback, 0, mask_size);
        if probe.is_some() {
            encoder.copy_buffer_to_buffer(
                &self.raw.tonal_statistics,
                0,
                readback,
                mask_size,
                self.raw.tonal_statistics.size(),
            );
        }
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        self.pending = Some(region);
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        selection_readback::capture_selection(
            readback,
            size,
            coverage_size,
            extent,
            byte_coverage,
            request_id,
            tx,
            move |mut result, _, extra| {
                result.tonal_sample = probe.and_then(|p| tonal::sample(extra, p));
                result.placement = placement;
                result
            },
        );
    }
}
impl WgpuRasterizer {
    pub fn region_pending(&self) -> bool {
        self.regions
            .as_ref()
            .is_some_and(|r| r.pending.is_some() || r.waiting.is_some() || r.running.is_some())
    }
    /// Limit refinement chunks and previews to `taps`, or with None restore
    /// this GPU's budgets.
    #[cfg(test)]
    pub(super) fn set_refine_chunk(&mut self, taps: Option<f64>) {
        let mut regions = self.regions.take().unwrap_or_else(|| RegionRequests::new(&self.device));
        regions.refiner(self).budget = taps;
        self.regions = Some(regions);
    }
    /// The pixel taps a refinement chunk takes on this GPU, once timed.
    #[cfg(test)]
    pub(super) fn refine_chunk(&self) -> Option<(f64, bool)> {
        self.regions.as_ref()?.refiner.as_ref().map(|r| (r.chunk(), r.times_chunks()))
    }
    pub(super) fn cancel_region(&mut self) {
        if let Some(regions) = &mut self.regions {
            regions.cancel();
        }
        self.refresh_storage_metrics();
    }
    pub(super) fn start_region(&mut self, request: RegionRequest) -> Result<bool, GpuRasterError> {
        let mut regions = self
            .regions
            .take()
            .unwrap_or_else(|| RegionRequests::new(&self.device));
        let result = regions.start(self, request);
        self.regions = Some(regions);
        self.refresh_storage_metrics();
        result
    }
    pub(super) fn poll_region(&mut self) -> Option<Result<RegionResult, GpuRasterError>> {
        if self.regions.as_ref()?.waiting.is_some() {
            let mut requests = self.regions.take().unwrap();
            let request = requests.waiting.take().unwrap();
            let result = requests.start(self, request);
            self.regions = Some(requests);
            if let Err(error) = result {
                return Some(Err(error));
            }
        }
        if let Some(refiner) = self.regions.as_mut()?.refiner.as_mut() {
            refiner.poll_timing(&self.device, &self.queue);
        }
        if self.regions.as_ref()?.running.as_ref().is_some_and(|r| r.job.idle()) {
            let mut requests = self.regions.take().unwrap();
            let running = requests.running.take().unwrap();
            let encoder = crate::submission::CommandEncoder::new(
                &self.device,
                &wgpu::CommandEncoderDescriptor { label: Some("refine selection") },
            );
            let result = requests.advance(self, encoder, running);
            self.regions = Some(requests);
            self.refresh_storage_metrics();
            if let Err(error) = result {
                return Some(Err(error));
            }
        }
        let requests = self.regions.as_mut()?;
        if requests.running.is_some() {
            let _ = self.device.poll(wgpu::PollType::Poll);
            return None;
        }
        requests.pending.as_ref()?;
        let _ = self.device.poll(wgpu::PollType::Poll);
        let result = requests.rx.as_ref()?.try_recv().ok()?;
        let region = requests.pending.take().unwrap();
        if let Ok(result) = &result {
            self.selection_clip
                .remember_pixels(&result.pixels, region.coverage);
        }
        self.refresh_storage_metrics();
        Some(result)
    }
}
