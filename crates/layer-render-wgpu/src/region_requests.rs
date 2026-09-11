//! Single-flight region queries. Keep the GPU result for rendering and capture
//! one packed history copy asynchronously, with no CPU flood/raster algorithm.
use super::*;
use flood::{Flood, Region};
use layer_render::{RegionRequest, RegionResult, RegionSource};

pub(super) struct RegionRequests {
    pub(super) flood: Flood,
    source: Option<(wgpu::Texture, wgpu::TextureView)>,
    scene: Option<scene::Scene>,
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
            + self
                .source
                .as_ref()
                .map_or(0, |(t, _)| u64::from(t.width()) * u64::from(t.height()) * 4)
            + self.scene.as_ref().map_or(0, |s| s.scratch_bytes())
            + self.readback.as_ref().map_or(0, |b| b.size())
            + self.pending.as_ref().map_or(0, |r| r.coverage.size())
    }
    pub(super) fn new(device: &PipelineDevice) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            flood: Flood::new(device),
            source: None,
            scene: None,
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
        if self.pending.is_some() || self.waiting.is_some() {
            return Ok(false);
        }
        let extent = r.document_extent;
        if extent.contains(&0)
            || request.position[0] >= extent[0]
            || request.position[1] >= extent[1]
            || !request.tolerance.is_finite()
            || !(0.0..=1.).contains(&request.tolerance)
            || !request.refinement.is_valid()
        {
            return Err(GpuRasterError::InvalidExtent);
        }
        if let Some(startup) = &r.startup {
            startup.compiler.check()?;
            let mut ready = self.flood.prepare(&startup.compiler, request.refinement);
            if request.limit.is_some() {
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
            t.begin(&r.device, &mut encoder);
        }
        if !matches!(request.source, RegionSource::Composite)
            && self
                .source
                .as_ref()
                .is_none_or(|(t, _)| [t.width(), t.height()] != extent)
        {
            self.source = Some(create_color_target(&r.device, extent, "region source"));
        }
        let source = match &request.source {
            RegionSource::Composite => r
                .composite_view
                .as_ref()
                .ok_or(GpuRasterError::InvalidExtent)?
                .clone(),
            RegionSource::Layers(layers) => {
                let (texture, view) = self.source.as_ref().unwrap();
                let scene = self.scene.get_or_insert_with(|| scene::Scene::new(r));
                let background = r
                    .thumbnails
                    .paper
                    .and_then(|(id, mut color)| {
                        let layer = layers.iter().find(|l| l.id == id && l.visible)?;
                        color[3] *= layer.opacity;
                        Some(color)
                    })
                    .unwrap_or([0.; 4]);
                scene.capture(
                    r,
                    FramePacket {
                        time_seconds: r.last_time_seconds,
                        view: layer_render::ViewState {
                            background_rgba_linear: background,
                            width_px: extent[0],
                            height_px: extent[1],
                            document_to_surface: [1., 0., 0., 1., 0., 0.],
                        },
                        document_extent: extent,
                        layers,
                        dabs: &[],
                        dab_batches: &[],
                        reset_layers: false,
                        composite_all: true,
                    },
                    texture,
                    &mut encoder,
                )?;
                view.clone()
            }
            RegionSource::Layer(id) => {
                let (texture, view) = self.source.as_ref().unwrap();
                let color = r
                    .thumbnails
                    .paper
                    .filter(|(paper, _)| paper == id)
                    .map_or([0.; 4], |(_, c)| c);
                {
                    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("region source clear"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: (color[0] * color[3]) as f64,
                                    g: (color[1] * color[3]) as f64,
                                    b: (color[2] * color[3]) as f64,
                                    a: color[3] as f64,
                                }),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                }
                if let Some(layer) = r.paint_layers.iter().find(|l| l.id == *id) {
                    for page in &layer.pages {
                        let [x, y] = page.coordinate.map(|v| v * PAGE_SIZE);
                        if x >= extent[0] || y >= extent[1] {
                            continue;
                        }
                        encoder.copy_texture_to_texture(
                            page.active().texture.as_image_copy(),
                            wgpu::TexelCopyTextureInfo {
                                origin: wgpu::Origin3d { x, y, z: 0 },
                                ..texture.as_image_copy()
                            },
                            wgpu::Extent3d {
                                width: PAGE_SIZE.min(extent[0] - x),
                                height: PAGE_SIZE.min(extent[1] - y),
                                depth_or_array_layers: 1,
                            },
                        );
                    }
                }
                view.clone()
            }
        };
        if let Some(selection) = &request.limit {
            r.selection_clip
                .prepare(&r.device, &mut encoder, extent, selection)?;
        }
        let region = self.flood.encode(
            &r.device,
            &mut encoder,
            &source,
            extent,
            request.position,
            request.tolerance,
            request.limit.as_ref().and(r.selection_clip.buffer.as_ref()),
            request.refinement,
        )?;
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
        encoder.submit(&r.queue);
        #[cfg(test)]
        if let Some(t) = &mut self.timing {
            t.submitted();
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
                        let words: std::sync::Arc<[u32]> = (0..extent[0].div_ceil(8) as usize
                            * extent[1] as usize)
                            .map(|i| read(32 + i * 4))
                            .collect();
                        let bounds = if read(coverage_size as usize + 16) == 0 {
                            [0; 4]
                        } else {
                            std::array::from_fn(|i| read(coverage_size as usize + i * 4))
                        };
                        let pixels = layer_core::SelectionPixels::new(extent, bounds, words)
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
