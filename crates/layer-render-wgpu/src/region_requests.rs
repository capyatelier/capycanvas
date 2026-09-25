//! Single-flight region queries. Keep the GPU result for rendering and capture
//! one packed history copy asynchronously, with no CPU flood/raster algorithm.
use super::*;
use flood::{Flood, Region};
use layer_render::{RegionRequest, RegionResult};

pub(super) struct RegionRequests {
    pub(super) flood: Flood,
    pub(super) raw: region_sources::RawRegions,
    refiner: Option<selection_refine::SelectionRefiner>,
    readback: Option<wgpu::Buffer>,
    pending: Option<Region>,
    waiting: Option<RegionRequest>,
    tx: mpsc::Sender<Result<RegionResult, GpuRasterError>>,
    rx: mpsc::Receiver<Result<RegionResult, GpuRasterError>>,
    #[cfg(test)]
    pub timing: Option<telemetry::Telemetry>,
}
impl RegionRequests {
    pub fn storage_bytes(&self) -> u64 {
        self.flood.storage_bytes()
            + self.raw.storage_bytes()
            + self.readback.as_ref().map_or(0, |b| b.size())
            + self.pending.as_ref().map_or(0, |r| r.coverage.size())
    }
    pub(super) fn new(device: &PipelineDevice) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            flood: Flood::new(device),
            raw: region_sources::RawRegions::new(device),
            refiner: None,
            readback: None,
            pending: None,
            waiting: None,
            tx,
            rx,
            #[cfg(test)]
            timing: None,
        }
    }
    fn start(
        &mut self,
        r: &mut WgpuRasterizer,
        request: RegionRequest,
    ) -> Result<bool, GpuRasterError> {
        #[cfg(test)]
        let trace = self.timing.as_ref().map(|_| std::time::Instant::now());
        if self.pending.is_some() || self.waiting.is_some() {
            return Ok(false);
        }
        let extent = match request.source.raw_source() {
            layer_render::RegionSource::Layer(id) | layer_render::RegionSource::Coverage(id) => {
                r.target_extent(*id)
            }
            _ => r.document_extent,
        };
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
        {
            return Err(GpuRasterError::InvalidExtent);
        }
        if request.selection.is_some() && self.refiner.is_none() {
            self.refiner = Some(selection_refine::SelectionRefiner::new(&r.device));
        }
        if let Some(startup) = &r.startup {
            startup.compiler.check()?;
            let mut ready = true;
            if tone.is_some() {
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
                    .prepare(&startup.compiler, options);
            }
            if request.limit.is_some() || request.selection.is_some() {
                for pipeline in [
                    &r.selection_clip.crossings,
                    &r.selection_clip.fill,
                    &r.selection_clip.resample,
                ] {
                    startup.compiler.pipeline(pipeline, startup::BRUSH);
                    ready &= pipeline.ready();
                }
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
        #[cfg(test)]
        if let Some(t) = &mut self.timing {
            t.begin(&r.device, &r.queue, &mut encoder);
        }
        if let Some(selection) = &request.limit {
            r.selection_clip
                .prepare(&r.device, &mut encoder, extent, selection)?;
        }
        let input = if let layer_render::RegionSource::Selection(selection) = &request.source {
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
                    &r.empty_view,
                    extent,
                    request.position,
                    request.tolerance,
                    request.limit.as_ref().and(r.selection_clip.buffer.as_ref()),
                    request.refinement,
                    Some(&classified),
                    request.contiguous,
                )?
            }
        };
        #[cfg(test)]
        let source_ms = trace.map(|t| t.elapsed().as_secs_f64() * 1000.);
        let direct_tonal = tone.is_some()
            && extent == r.document_extent
            && request.selection.as_ref().is_some_and(|s| {
                s.mode == layer_core::SelectionMode::New
                    && s.antialias
                    && s.feather == 0.
                    && s.resize == 0
                    && s.source_to_document == layer_core::Affine::IDENTITY
            });
        let (region, extent, byte_coverage) = if direct_tonal {
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
            if let Some(previous) = &options.previous {
                r.selection_clip
                    .prepare(&r.device, &mut encoder, extent, previous)?;
            }
            let previous = options
                .previous
                .as_ref()
                .and(r.selection_clip.buffer.as_ref());
            (
                self.refiner.as_ref().unwrap().encode(
                    &r.device,
                    &mut encoder,
                    extent,
                    &input.coverage,
                    previous,
                    options,
                )?,
                extent,
                true,
            )
        } else {
            (input, extent, false)
        };
        #[cfg(test)]
        let flood_ms = trace.map(|t| t.elapsed().as_secs_f64() * 1000.);
        let coverage_size = region.bounds_offset;
        let probe = tone.and_then(|t| t.probe);
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
        #[cfg(test)]
        if let Some(t) = &mut self.timing {
            t.end(&mut encoder);
        }
        r.uploads.finish(&encoder);
        #[cfg(test)]
        let encode_ms = trace.map(|t| t.elapsed().as_secs_f64() * 1000.);
        #[cfg(test)]
        let submission = if trace.is_some() {
            encoder.submit_timed(&r.queue)
        } else {
            encoder.submit(&r.queue);
            [0.; 2]
        };
        #[cfg(not(test))]
        encoder.submit(&r.queue);
        #[cfg(test)]
        if let Some(trace) = trace {
            let total = trace.elapsed().as_secs_f64() * 1000.;
            if total > 0.6 {
                eprintln!(
                    "region timing request={} cumulative source/flood/encode/submit={:.3}/{:.3}/{:.3}/{:.3}ms; finish/queue={:.3}/{:.3}",
                    request.request_id,
                    source_ms.unwrap(),
                    flood_ms.unwrap(),
                    encode_ms.unwrap(),
                    total,
                    submission[0],
                    submission[1]
                );
            }
        }
        #[cfg(test)]
        if let Some(t) = &mut self.timing {
            t.submitted(&r.queue);
        }
        self.pending = Some(region);
        selection_readback::capture_selection(
            readback,
            size,
            coverage_size,
            extent,
            byte_coverage,
            request.request_id,
            self.tx.clone(),
            move |mut result, _, extra| {
                result.tonal_sample = probe.and_then(|p| tonal::sample(extra, p));
                result
            },
        );
        Ok(true)
    }
}
impl WgpuRasterizer {
    pub fn region_pending(&self) -> bool {
        self.regions
            .as_ref()
            .is_some_and(|r| r.pending.is_some() || r.waiting.is_some())
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
        let requests = self.regions.as_mut()?;
        requests.pending.as_ref()?;
        let _ = self.device.poll(wgpu::PollType::Poll);
        let result = requests.rx.try_recv().ok()?;
        let region = requests.pending.take().unwrap();
        if let Ok(result) = &result {
            self.selection_clip
                .remember_pixels(&result.pixels, region.coverage);
        }
        self.refresh_storage_metrics();
        Some(result)
    }
}
