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
        let extent = match request.source { layer_render::RegionSource::Layer(id) => r.target_extent(id), _ => r.document_extent };
        if extent.contains(&0)
            || request.position[0] >= extent[0]
            || request.position[1] >= extent[1]
            || !request.tolerance.is_finite()
            || !(0.0..=1.).contains(&request.tolerance)
            || !request.refinement.is_valid()
            || request.selection.as_ref().is_some_and(|s| !s.is_valid())
            || (matches!(request.source, layer_render::RegionSource::Selection(_)) && request.selection.is_none())
        {
            return Err(GpuRasterError::InvalidExtent);
        }
        if request.selection.is_some() && self.refiner.is_none() {
            self.refiner = Some(selection_refine::SelectionRefiner::new(&r.device));
        }
        if let Some(startup) = &r.startup {
            startup.compiler.check()?;
            let mut ready = true;
            if !matches!(request.source, layer_render::RegionSource::Selection(_)) {
                ready &= self.flood.prepare(&startup.compiler, request.refinement);
                ready &= self.raw.prepare(&startup.compiler);
            }
            if request.selection.is_some() { ready &= self.refiner.as_ref().unwrap().prepare(&startup.compiler); }
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
            r.selection_clip.prepare(&r.device, &mut encoder, extent, selection)?;
            let buffer = r.selection_clip.buffer.as_ref().unwrap();
            // The next prepare may reuse the clip allocation for the old mask.
            let copy = r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("incoming selection"), size: buffer.size(),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(buffer,0,&copy,0,buffer.size());
            flood::Region { coverage: copy, bounds_offset: 0 }
        } else {
            let classified = self.raw.encode(r, &request, &mut encoder)?;
            self.flood.encode_input(
                &r.device, &mut encoder, &r.empty_view, extent, request.position,
                request.tolerance, request.limit.as_ref().and(r.selection_clip.buffer.as_ref()),
                request.refinement, Some(&classified), request.contiguous,
            )?
        };
        #[cfg(test)]
        let source_ms = trace.map(|t| t.elapsed().as_secs_f64() * 1000.);
        let (region, extent, byte_coverage) = if let Some(options) = &request.selection {
            let extent = r.document_extent;
            if let Some(previous) = &options.previous {
                r.selection_clip.prepare(&r.device, &mut encoder, extent, previous)?;
            }
            let previous = options.previous.as_ref().and(r.selection_clip.buffer.as_ref());
            (self.refiner.as_ref().unwrap().encode(&r.device, &mut encoder, extent, &input.coverage, previous, options)?, extent, true)
        } else { (input, extent, false) };
        #[cfg(test)]
        let flood_ms = trace.map(|t| t.elapsed().as_secs_f64() * 1000.);
        let coverage_size = region.bounds_offset;
        let size = coverage_size + 32;
        if self.readback.as_ref().is_none_or(|b| b.size() < size) {
            self.readback = Some(r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("region history snapshot"),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let readback = self.readback.as_ref().unwrap();
        encoder.copy_buffer_to_buffer(&region.coverage, 0, readback, 0, size);
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
        let ready = readback.clone();
        let tx = self.tx.clone();
        self.pending = Some(region);
        readback
            .slice(..size)
            .map_async(wgpu::MapMode::Read, move |result| {
                let result = result
                    .map_err(|e| GpuRasterError::MapFailed(e.to_string()))
                    .and_then(|_| {
                        let bytes = ready
                            .slice(..size)
                            .get_mapped_range()
                            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?;
                        let read = |offset: usize| {
                            u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
                        };
                        let words: std::sync::Arc<[u32]> = (0..extent[0].div_ceil(if byte_coverage { 4 } else { 8 }) as usize
                            * extent[1] as usize)
                            .map(|i| read(32 + i * 4))
                            .collect();
                        let bounds = if read(coverage_size as usize + 16) == 0 {
                            [0; 4]
                        } else {
                            std::array::from_fn(|i| read(coverage_size as usize + i * 4))
                        };
                        let pixels = if byte_coverage { layer_core::SelectionPixels::bytes(extent, bounds, words) }
                            else { layer_core::SelectionPixels::new(extent, bounds, words) }
                            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?;
                        Ok(RegionResult {
                            request_id: request.request_id,
                            pixels: std::sync::Arc::new(pixels),
                        })
                    });
                ready.unmap();
                let _ = tx.send(result);
            });
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
        Some(result)
    }
}
