//! Idle-time GPU previews: one source capture/probe per revision, one shared
//! preview pipeline, bounded scratch and small asynchronous image readbacks.
use super::*;
use layer_render::{FilterPreviewImage, FilterPreviewRequest};
use std::{collections::HashMap, sync::Arc};
use wgpu::util::DeviceExt;

type Image = (wgpu::Texture, wgpu::TextureView);
enum Ready {
    Point(Result<u32, GpuRasterError>),
    Pixels(Result<ReadbackImage, GpuRasterError>),
}
pub(crate) struct FilterPreviews {
    scene: Scene,
    programs: HashMap<Arc<str>, Layer>,
    probe: wgpu::ComputePipeline,
    mask: Image,
    source: Option<Image>,
    key: Option<(u64, LayerId, [u32; 2], [f32; 4])>,
    source_layers: Vec<Layer>,
    point: Option<[u32; 2]>,
    scratch: Vec<Image>,
    scratch_size: [u32; 2],
    size: [u32; 2],
    rows: HashMap<Arc<str>, Vec<u8>>,
    request: Option<FilterPreviewRequest>,
    rendering: Vec<Arc<str>>,
    tx: mpsc::Sender<Ready>,
    rx: mpsc::Receiver<Ready>,
    pub source_updates: u64,
    pub rendered_rows: u64,
}
impl FilterPreviews {
    fn new(r: &mut WgpuRasterizer) -> Result<Self, GpuRasterError> {
        let scene = Scene::new(r);
        let shader = r.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("filter preview content probe"),
            source: wgpu::ShaderSource::Wgsl(include_str!("filter_probe.wgsl").into()),
        });
        let probe = r
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("filter preview content probe"),
                layout: None,
                module: &shader,
                entry_point: Some("measure"),
                compilation_options: Default::default(),
                cache: None,
            });
        // Reuse the original GPU-rendered G-Pen preview's alpha, not a second
        // approximation of its silhouette. Decode/upload only once.
        let png = png::Decoder::new(std::io::Cursor::new(include_bytes!(
            "../../../apps/layer-web/brush-previews/1-light.png"
        )));
        let mut reader = png
            .read_info()
            .map_err(|e| GpuRasterError::Effect(e.to_string()))?;
        let mut pixels = vec![0; reader.output_buffer_size()];
        let info = reader
            .next_frame(&mut pixels)
            .map_err(|e| GpuRasterError::Effect(e.to_string()))?;
        let mask = create_color_target(
            &r.device,
            [info.width, info.height],
            "G-Pen preview silhouette",
        );
        r.queue.write_texture(
            mask.0.as_image_copy(),
            &pixels[..info.buffer_size()],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(info.width * 4),
                rows_per_image: Some(info.height),
            },
            mask.0.size(),
        );
        let (tx, rx) = mpsc::channel();
        Ok(Self {
            scene,
            programs: HashMap::new(),
            probe,
            mask,
            source: None,
            key: None,
            source_layers: Vec::new(),
            point: None,
            scratch: Vec::new(),
            scratch_size: [0, 0],
            size: [0, 0],
            rows: HashMap::new(),
            request: None,
            rendering: Vec::new(),
            tx,
            rx,
            source_updates: 0,
            rendered_rows: 0,
        })
    }
    fn start(
        &mut self,
        r: &mut WgpuRasterizer,
        mut request: FilterPreviewRequest,
    ) -> Result<bool, GpuRasterError> {
        if self.request.is_some() {
            return Ok(false);
        }
        if request.filters.is_empty()
            || request.filters.len() > 8
            || request.size.contains(&0)
            || request.size[0] > 512
            || request.size[1] > 128
            || request.extent != r.document_extent
        {
            return Ok(false);
        }
        for effect in &request.filters {
            effect
                .validate()
                .map_err(|e| GpuRasterError::Effect(e.into()))?;
            let id = effect.program.id.clone();
            let count = self.programs.len();
            let layer = self
                .programs
                .entry(id.clone())
                .or_insert_with(|| Layer::paint(LayerId(u64::MAX - count as u64), ""));
            if layer.effect.as_ref() != Some(effect) {
                self.rows.remove(&id);
                layer.effect = Some(effect.clone());
            }
        }
        if let Some(paper) = request
            .layers
            .iter()
            .find(|l| l.kind == LayerKind::Background)
        {
            request.view.background_rgba_linear[3] *=
                if paper.visible { paper.opacity } else { 0. };
        }
        if request
            .layers
            .iter()
            .any(|l| l.id == request.target && l.properties.parent.is_some())
        {
            request.view.background_rgba_linear = [0.; 4];
        }
        let key = (
            r.filter_source_epoch,
            request.target,
            request.extent,
            request.view.background_rgba_linear,
        );
        let resized = self.size != request.size;
        if resized {
            self.rows.clear();
            self.size = request.size;
        }
        let changed = self.key != Some(key) || self.source_layers != request.layers || resized;
        self.request = Some(request);
        if changed {
            self.source_layers = self.request.as_ref().unwrap().layers.clone();
            self.rows.clear();
            self.key = Some(key);
            self.point = None;
            let request = self.request.as_ref().unwrap();
            let mut encoder = crate::submission::CommandEncoder::new(
                &r.device,
                &wgpu::CommandEncoderDescriptor {
                    label: Some("filter preview source"),
                },
            );
            if self
                .source
                .as_ref()
                .is_none_or(|s| [s.0.width(), s.0.height()] != request.extent)
            {
                self.source = Some(create_color_target(
                    &r.device,
                    request.extent,
                    "filter insertion source",
                ));
            }
            let mut scene = r.scene.take().unwrap_or_else(|| Scene::new(r));
            scene.begin_frame();
            scene.style_base = r.last_style_base;
            let captured = scene.capture_filter_source(
                r,
                request,
                &self.source.as_ref().unwrap().0,
                &mut encoder,
            );
            r.scene = Some(scene);
            captured?;
            let uniform = r
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("preview paper"),
                    contents: &request
                        .view
                        .background_rgba_linear
                        .into_iter()
                        .chain([request.size[0] as f32, request.size[1] as f32, 0., 0.])
                        .flat_map(f32::to_le_bytes)
                        .collect::<Vec<_>>(),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let winner = r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("preview content coordinate"),
                size: 8,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.clear_buffer(&winner, 0, None);
            let binding = r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("preview probe"),
                layout: &self.probe.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(
                            &self.source.as_ref().unwrap().1,
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: winner.as_entire_binding(),
                    },
                ],
            });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("find preview content"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.probe);
                pass.set_bind_group(0, &binding, &[]);
                pass.dispatch_workgroups(
                    request.extent[0].div_ceil(16),
                    request.extent[1].div_ceil(16),
                    1,
                );
            }
            let read = r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("preview coordinate pair"),
                size: 8,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(&winner, 0, &read, 0, 8);
            r.uploads.finish(&encoder);
            encoder.submit(&r.queue);
            let buffer = read.clone();
            let tx = self.tx.clone();
            read.slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let result = result
                        .map_err(|e| GpuRasterError::MapFailed(e.to_string()))
                        .and_then(|_| {
                            let bytes = buffer
                                .slice(..)
                                .get_mapped_range()
                                .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?;
                            let preferred = u32::from_le_bytes(bytes[..4].try_into().unwrap());
                            let value = if preferred > 0 {
                                preferred
                            } else {
                                u32::from_le_bytes(bytes[4..8].try_into().unwrap())
                            };
                            drop(bytes);
                            buffer.unmap();
                            Ok(value)
                        });
                    let _ = tx.send(Ready::Point(result));
                });
            self.source_updates += 1;
        } else if self
            .request
            .as_ref()
            .unwrap()
            .filters
            .iter()
            .any(|f| !self.rows.contains_key(&f.program.id))
        {
            self.render(r)?;
        }
        Ok(true)
    }
    fn render(&mut self, r: &mut WgpuRasterizer) -> Result<(), GpuRasterError> {
        let request = self.request.as_ref().unwrap();
        self.rendering = request
            .filters
            .iter()
            .map(|f| f.program.id.clone())
            .filter(|f| !self.rows.contains_key(f))
            .collect();
        if self.rendering.is_empty() {
            return Ok(());
        }
        let mut encoder = crate::submission::CommandEncoder::new(
            &r.device,
            &wgpu::CommandEncoderDescriptor {
                label: Some("filter picker previews"),
            },
        );
        self.scene.begin_frame();
        self.scene.jobs.clear();
        let [width, height] = request.size;
        let extent = if self.point.is_some() {
            request.extent
        } else {
            [width, height]
        };
        let center = self.point.unwrap_or([width / 2, height / 2]);
        let origin = std::array::from_fn::<_, 2, _>(|i| {
            center[i]
                .saturating_sub(request.size[i] / 2)
                .min(extent[i].saturating_sub(request.size[i]))
        });
        let fallback;
        let source = if self.point.is_some() {
            self.source.as_ref().unwrap().1.clone()
        } else {
            fallback = create_color_target(&r.device, extent, "empty document filter sample");
            let mut data = [0.; 24];
            data[..6].copy_from_slice(&[
                0.,
                0.,
                width as f32,
                height as f32,
                width as f32,
                height as f32,
            ]);
            data[8] = 9.;
            self.scene.jobs.push(Job::Draw {
                target: fallback.1.clone(),
                sources: [r.empty_view.clone(), r.empty_view.clone()],
                data,
                over: false,
                clip: None,
            });
            fallback.1.clone()
        };
        let atlas = create_color_target(
            &r.device,
            [width, height * self.rendering.len() as u32],
            "filter preview atlas",
        );
        for (row, id) in self.rendering.iter().enumerate() {
            // Compile only requested rows. A catalog-wide dynamic switch would
            // compile every expensive kernel before showing even the first row.
            let prepared = self.scene.effects.prepare(
                r,
                &[&self.programs[id]],
                effects::Execution::Preview,
                0.,
            )?;
            let program = self.programs[id].effect.as_ref().unwrap().program.clone();
            // A document-remapping pass after another pass genuinely needs its
            // complete input. Bounded programs otherwise render only crop+halo.
            let whole = program
                .passes
                .iter()
                .skip(1)
                .any(|p| p.sampling == layer_core::EffectSampling::Document);
            let pad = self.programs[id]
                .effect
                .as_ref()
                .unwrap()
                .damage_radius()
                .unwrap_or(0);
            let crop = if whole {
                [0, 0]
            } else {
                [origin[0].saturating_sub(pad), origin[1].saturating_sub(pad)]
            };
            let size = if whole {
                extent
            } else {
                [width + pad * 2, height + pad * 2]
            };
            let count = program.passes.len().max(1);
            if size[0] > self.scratch_size[0] || size[1] > self.scratch_size[1] {
                self.scratch_size = [
                    size[0].max(self.scratch_size[0]),
                    size[1].max(self.scratch_size[1]),
                ];
                self.scratch.clear();
            }
            while self.scratch.len() < count.min(2) {
                self.scratch.push(create_color_target(
                    &r.device,
                    self.scratch_size,
                    "reused preview crop",
                ));
            }
            let mut previous = source.clone();
            for stage in 0..count {
                let target = self.scratch[stage % 2].1.clone();
                let mut data = [0.; 24];
                data[..8].copy_from_slice(&[
                    0.,
                    0.,
                    size[0] as f32,
                    size[1] as f32,
                    self.scratch_size[0] as f32,
                    self.scratch_size[1] as f32,
                    0.,
                    stage as f32,
                ]);
                data[12..16].copy_from_slice(&[
                    crop[0] as f32,
                    crop[1] as f32,
                    extent[0] as f32,
                    extent[1] as f32,
                ]);
                if stage > 0 {
                    data[16..20].copy_from_slice(&[
                        crop[0] as f32,
                        crop[1] as f32,
                        self.scratch_size[0] as f32,
                        self.scratch_size[1] as f32,
                    ]);
                }
                self.scene.jobs.push(Job::Effect {
                    target: target.clone(),
                    sources: [previous, source.clone()],
                    data,
                    prepared: prepared.clone(),
                    masks: Box::new(std::array::from_fn(|_| r.empty_view.clone())),
                });
                previous = target;
            }
            let mut data = [0.; 24];
            data[..6].copy_from_slice(&[
                0.,
                (row as u32 * height) as f32,
                width as f32,
                height as f32,
                width as f32,
                (height * self.rendering.len() as u32) as f32,
            ]);
            data[8] = 10.;
            data[12..16].copy_from_slice(&[
                (origin[0] - crop[0]) as f32,
                (origin[1] - crop[1]) as f32,
                width as f32,
                height as f32,
            ]);
            self.scene.jobs.push(Job::Draw {
                target: atlas.1.clone(),
                sources: [previous, self.mask.1.clone()],
                data,
                over: false,
                clip: None,
            });
        }
        self.scene.encode_jobs(r, &mut encoder)?;
        let image_height = height * self.rendering.len() as u32;
        let binding = create_texture_bind_group(
            &r.device,
            &r.texture_layout,
            &atlas.1,
            &r.sampler,
            "filter preview export",
        );
        let tx = self.tx.clone();
        r.submit_ui_readback(
            encoder,
            &binding,
            [width, image_height],
            request.request_id,
            move |image| {
                let _ = tx.send(Ready::Pixels(image));
            },
        );
        self.rendered_rows += self.rendering.len() as u64;
        Ok(())
    }
    fn take(
        &mut self,
        r: &mut WgpuRasterizer,
    ) -> Option<Result<FilterPreviewImage, GpuRasterError>> {
        self.request.as_ref()?;
        while let Ok(ready) = self.rx.try_recv() {
            let result = match ready {
                Ready::Point(value) => value.and_then(|score| {
                    if score != 0 {
                        let rank = u32::MAX - score;
                        let extent = self.request.as_ref().unwrap().extent;
                        let decode = |v: u32| {
                            if v & 1 == 0 {
                                (v / 2) as i32
                            } else {
                                -((v / 2) as i32)
                            }
                        };
                        self.point = Some(
                            [
                                (extent[0] / 2) as i32 + decode(rank & 65535),
                                (extent[1] / 2) as i32 + decode(rank >> 16),
                            ]
                            .map(|v| v as u32),
                        );
                    }
                    self.render(r)
                }),
                Ready::Pixels(image) => image.map(|image| {
                    let row_bytes = (self.size[0] * self.size[1] * 4) as usize;
                    for (id, bytes) in self.rendering.drain(..).zip(image.bytes.chunks(row_bytes)) {
                        self.rows.insert(id, bytes.to_vec());
                    }
                }),
            };
            if let Err(error) = result {
                self.request = None;
                return Some(Err(error));
            }
        }
        let request = self.request.as_ref()?;
        if request
            .filters
            .iter()
            .any(|f| !self.rows.contains_key(&f.program.id))
        {
            return None;
        }
        let request = self.request.take().unwrap();
        let mut bytes = Vec::with_capacity(
            self.size[0] as usize * self.size[1] as usize * 4 * request.filters.len(),
        );
        for effect in &request.filters {
            bytes.extend_from_slice(&self.rows[&effect.program.id]);
        }
        Some(Ok(FilterPreviewImage {
            image: ReadbackImage {
                request_id: request.request_id,
                width: self.size[0],
                height: self.size[1] * request.filters.len() as u32,
                stride: self.size[0] * 4,
                bytes,
            },
            filters: request
                .filters
                .iter()
                .map(|f| f.program.id.clone())
                .collect(),
        }))
    }
}
impl Scene {
    fn capture_filter_source(
        &mut self,
        r: &mut WgpuRasterizer,
        request: &FilterPreviewRequest,
        destination: &wgpu::Texture,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        let index = request
            .layers
            .iter()
            .position(|l| l.id == request.target)
            .ok_or_else(|| GpuRasterError::Effect("Missing filter insertion layer".into()))?;
        let parent = request.layers[index].properties.parent;
        if parent.is_none()
            && !request.layers[..index]
                .iter()
                .any(|l| l.visible && l.properties.parent.is_none())
            && !request
                .layers
                .iter()
                .any(|l| l.mask.as_ref().is_some_and(|m| m.enabled && m.show_area))
            && let Some(composite) = &r.composite_texture
        {
            encoder.copy_texture_to_texture(
                composite.as_image_copy(),
                destination.as_image_copy(),
                destination.size(),
            );
            return Ok(());
        }
        self.stop_before = request.layers[..index]
            .iter()
            .rposition(|l| l.properties.parent == parent)
            .map(|i| (i, false));
        self.jobs.clear();
        self.used.fill(false);
        let packet = FramePacket {
            time_seconds: 0.,
            view: request.view,
            document_extent: request.extent,
            layers: &request.layers,
            dabs: &[],
            dab_batches: &[],
            reset_layers: false,
            composite_all: false,
        };
        let result = (|| {
            for tile in page_coordinates(PixelRect::full(request.extent)) {
                let input = self.group(r, packet, parent, tile)?;
                self.copy_tile(input, destination, tile, request.extent);
            }
            self.encode_jobs(r, encoder)
        })();
        self.stop_before = None;
        result
    }
}
impl WgpuRasterizer {
    pub fn filter_previews_pending(&self) -> bool {
        self.filter_previews
            .as_ref()
            .is_some_and(|p| p.request.is_some())
    }
    pub(crate) fn start_filter_previews(
        &mut self,
        request: FilterPreviewRequest,
    ) -> Result<bool, GpuRasterError> {
        let mut previews = match self.filter_previews.take() {
            Some(p) => p,
            None => FilterPreviews::new(self)?,
        };
        let result = previews.start(self, request);
        if result.is_err() {
            previews.request = None;
            previews.key = None;
        }
        self.filter_previews = Some(previews);
        result
    }
    pub(crate) fn poll_filter_previews(
        &mut self,
    ) -> Option<Result<FilterPreviewImage, GpuRasterError>> {
        let mut previews = self.filter_previews.take()?;
        if previews.request.as_ref().is_some_and(|request| {
            request
                .filters
                .iter()
                .any(|effect| !previews.rows.contains_key(&effect.program.id))
        }) {
            let _ = self.device.poll(wgpu::PollType::Poll);
        }
        let result = previews.take(self);
        self.filter_previews = Some(previews);
        result
    }
}

impl FilterPreviews {
    pub(crate) fn storage_bytes(&self) -> u64 {
        let bytes = |(texture, _): &Image| texture.width() as u64 * texture.height() as u64 * 4;
        self.source.as_ref().map_or(0, bytes)
            + bytes(&self.mask)
            + self.scratch.iter().map(bytes).sum::<u64>()
            + self.scene.scratch_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{fixture, fixtures};

    fn finish(r: &mut WgpuRasterizer) -> FilterPreviewImage {
        for _ in 0..100 {
            r.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(Duration::from_secs(10)),
                })
                .unwrap();
            if let Some(result) = r.take_filter_previews() {
                return result.unwrap();
            }
        }
        panic!("preview did not complete");
    }
    #[test]
    fn filter_previews_capture_insertion_pixels_and_cache_independently_of_view() {
        let mut r = WgpuRasterizer::new_headless().unwrap();
        let asset = AssetId("test:preview-source".into());
        let mut bytes = vec![0u8; 512 * 256 * 4];
        for y in 105..145 {
            for x in 300..340 {
                let p = (y * 512 + x) * 4;
                bytes[p..p + 4].copy_from_slice(&[30, 100, 230, 255]);
            }
        }
        r.prepare_asset(
            &asset,
            HostImage {
                width: 512,
                height: 256,
                stride: 512 * 4,
                format: PixelFormat::Rgba8Srgb,
                bytes: &bytes,
            },
        )
        .unwrap();
        let mut base = Layer::paint(LayerId(1), "Source");
        base.asset = Some(asset);
        let mut upper = Layer::paint(LayerId(2), "Not part of preview");
        upper.effect = Some(Arc::new(fixture("black_white").preview().unwrap()));
        upper.properties.clipped = true;
        let layers = vec![upper, base];
        let view = layer_render::ViewState {
            width_px: 512,
            height_px: 256,
            document_to_surface: [1., 0., 0., 1., 0., 0.],
            background_rgba_linear: [0.; 4],
        };
        r.submit(FramePacket {
            time_seconds: 0.,
            view,
            document_extent: [512, 256],
            layers: &layers,
            dabs: &[],
            dab_batches: &[],
            reset_layers: true,
            composite_all: true,
        })
        .unwrap();
        assert!(
            r.readback_srgb_rgba8()
                .unwrap()
                .chunks_exact(4)
                .any(|p| p[3] > 0),
            "test artwork must be rendered before capture"
        );
        let mut request = FilterPreviewRequest {
            request_id: 1,
            target: LayerId(1),
            size: [200, 40],
            extent: [512, 256],
            view,
            layers: layers.iter().map(Layer::composite_snapshot).collect(),
            filters: vec![
                Arc::new(fixture("curves").preview().unwrap()),
                Arc::new(fixture("black_white").preview().unwrap()),
            ],
        };
        assert!(r.request_filter_previews(request.clone()).unwrap());
        let first = finish(&mut r);
        let preview = r.filter_previews.as_ref().unwrap();
        assert_eq!(preview.source_updates, 1);
        assert_eq!(preview.rendered_rows, 2);
        assert_eq!(
            preview.scene.effects.compilations, 2,
            "compile only the two requested filters"
        );
        let p = preview.point.unwrap();
        assert!((300..340).contains(&p[0]) && (105..145).contains(&p[1]));
        let colors: Vec<_> = first.image.bytes[..200 * 40 * 4]
            .chunks_exact(4)
            .filter(|p| p[3] > 100)
            .collect();
        assert!(colors.len() > 100, "must find actual nonempty content");
        assert!(
            colors.iter().all(|p| p[2] > p[0] + 20),
            "must not include the clipped grayscale filter above insertion point"
        );
        // The original 40px mark stays 40px wide (never thumbnail-scaled).
        for row in first.image.bytes[..200 * 40 * 4].chunks_exact(200 * 4) {
            assert!(row.chunks_exact(4).filter(|p| p[3] > 0).count() <= 42);
        }
        request.request_id = 2;
        request.view.document_to_surface = [2., 0., 0., 2., 90., 100.];
        assert!(r.request_filter_previews(request.clone()).unwrap());
        let second = r.take_filter_previews().unwrap().unwrap();
        assert_eq!(second.image.bytes, first.image.bytes);
        assert_eq!(second.image.request_id, 2);
        assert_eq!(r.filter_previews.as_ref().unwrap().rendered_rows, 2);
        request.filters = vec![Arc::new(fixture("exposure").preview().unwrap())];
        request.request_id = 3;
        assert!(r.request_filter_previews(request).unwrap());
        finish(&mut r);
        assert_eq!(r.filter_previews.as_ref().unwrap().source_updates, 1);
        assert_eq!(r.filter_previews.as_ref().unwrap().rendered_rows, 3);
        assert_eq!(
            r.filter_previews
                .as_ref()
                .unwrap()
                .scene
                .effects
                .compilations,
            3
        );
    }
    #[test]
    fn empty_document_filter_previews_have_a_masked_color_sample() {
        let mut r = WgpuRasterizer::new_headless().unwrap();
        let layers = vec![Layer::paint(LayerId(1), "Empty")];
        let view = layer_render::ViewState {
            width_px: 512,
            height_px: 256,
            document_to_surface: [1., 0., 0., 1., 0., 0.],
            background_rgba_linear: [1.; 4],
        };
        r.submit(FramePacket {
            time_seconds: 0.,
            view,
            document_extent: [512, 256],
            layers: &layers,
            dabs: &[],
            dab_batches: &[],
            reset_layers: true,
            composite_all: true,
        })
        .unwrap();
        r.request_filter_previews(FilterPreviewRequest {
            request_id: 1,
            target: LayerId(1),
            size: [200, 40],
            extent: [512, 256],
            view,
            layers,
            filters: vec![
                Arc::new(fixture("curves").preview().unwrap()),
                Arc::new(fixture("black_white").preview().unwrap()),
            ],
        })
        .unwrap();
        let image = finish(&mut r).image;
        assert!(r.filter_previews.as_ref().unwrap().point.is_none());
        let pixels: Vec<_> = image.bytes.chunks_exact(4).collect();
        assert!(pixels.iter().filter(|p| p[3] > 100).count() > 1000);
        assert!(pixels.iter().filter(|p| p[3] == 0).count() > 1000);
        assert_ne!(&image.bytes[..200 * 40 * 4], &image.bytes[200 * 40 * 4..]);
        let directory = std::path::Path::new("../../artifacts/ui/filter-picker");
        std::fs::create_dir_all(directory).unwrap();
        let file = std::fs::File::create(directory.join("gpu-preview-sample.png")).unwrap();
        let mut png = png::Encoder::new(file, image.width, image.height);
        png.set_color(png::ColorType::Rgba);
        png.set_depth(png::BitDepth::Eight);
        png.write_header()
            .unwrap()
            .write_image_data(&image.bytes)
            .unwrap();
    }

    #[test]
    #[ignore = "GPU completion benchmark; run alone in release mode"]
    fn filter_preview_latency() {
        let mut r = WgpuRasterizer::new_headless().unwrap();
        let mut program = (*fixture("brightness_contrast").program()).clone();
        program.kind = layer_core::EffectKind::Generator;
        program.entry = "sample_art".into();
        program.wgsl="fn sample_art(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(.2+.6*fract(p.x/211.),.1+.5*fract(p.y/127.),.5+.4*sin(p.x*.01),1.);}".into();
        let mut layer = Layer::paint(LayerId(1), "Test artwork");
        layer.kind = LayerKind::Effect;
        layer.effect = Some(Arc::new(layer_core::EffectInstance::new(Arc::new(program))));
        let layers = vec![layer];
        let view = layer_render::ViewState {
            width_px: 4096,
            height_px: 4096,
            document_to_surface: [1., 0., 0., 1., 0., 0.],
            background_rgba_linear: [1.; 4],
        };
        r.submit(FramePacket {
            time_seconds: 0.,
            view,
            document_extent: [4096, 4096],
            layers: &layers,
            dabs: &[],
            dab_batches: &[],
            reset_layers: true,
            composite_all: true,
        })
        .unwrap();
        r.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(30)),
            })
            .unwrap();
        let request = FilterPreviewRequest {
            request_id: 1,
            target: LayerId(1),
            size: [400, 80],
            extent: [4096, 4096],
            view,
            layers,
            filters: fixtures()[..8]
                .iter()
                .map(|f| Arc::new(f.preview().unwrap()))
                .collect(),
        };
        let started = web_time::Instant::now();
        r.request_filter_previews(request.clone()).unwrap();
        finish(&mut r);
        let cold = started.elapsed().as_secs_f64() * 1000.;
        let mut report =
            format!("case,median_ms,p95_ms,p99_ms\ncold,{cold:.6},{cold:.6},{cold:.6}\n");
        for case in ["new source", "new preview rows", "cache hit"] {
            let mut samples = Vec::new();
            for i in 0..65 {
                if case == "new source" {
                    r.filter_previews.as_mut().unwrap().key = None;
                }
                if case == "new preview rows" {
                    r.filter_previews.as_mut().unwrap().rows.clear();
                }
                let before = r.filter_previews.as_ref().unwrap().rendered_rows;
                let started = web_time::Instant::now();
                assert!(r.request_filter_previews(request.clone()).unwrap());
                if case == "cache hit" {
                    assert!(r.take_filter_previews().unwrap().is_ok());
                    assert_eq!(r.filter_previews.as_ref().unwrap().rendered_rows, before);
                } else {
                    finish(&mut r);
                }
                if i >= 5 {
                    samples.push(started.elapsed().as_secs_f64() * 1000.);
                }
            }
            samples.sort_by(f64::total_cmp);
            report.push_str(&format!(
                "{case},{:.6},{:.6},{:.6}\n",
                samples[samples.len() / 2],
                samples[samples.len() * 95 / 100],
                samples[samples.len() * 99 / 100]
            ));
        }
        eprintln!(
            "{report}resident preview bytes: {}",
            r.filter_previews.as_ref().unwrap().storage_bytes()
        );
        std::fs::create_dir_all("../../artifacts/benchmarks").unwrap();
        std::fs::write("../../artifacts/benchmarks/filter-previews.csv", report).unwrap();
    }

    #[test]
    fn cropped_multipass_preview_matches_full_resolution_canvas() {
        use layer_core::{EffectInstance, EffectPass, EffectSampling};
        let mut r = WgpuRasterizer::new_headless().unwrap();
        let asset = AssetId("test:preview-seam".into());
        let bytes: Vec<u8> = (0..512 * 256)
            .flat_map(|i| {
                if i % 512 < 256 {
                    [230, 30, 60, 255]
                } else {
                    [30, 80, 230, 255]
                }
            })
            .collect();
        r.prepare_asset(
            &asset,
            HostImage {
                width: 512,
                height: 256,
                stride: 2048,
                format: PixelFormat::Rgba8Srgb,
                bytes: &bytes,
            },
        )
        .unwrap();
        let mut base = Layer::paint(LayerId(1), "Paint");
        base.asset = Some(asset);
        let mut program = (*fixture("brightness_contrast").program()).clone();
        program.entry = "preview_h".into();
        program.wgsl="fn preview_h(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return (c+fx_sample(p+vec2<f32>(-2.,0.))+fx_sample(p+vec2<f32>(2.,0.)))/3.;} fn preview_v(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return (c+fx_sample(p+vec2<f32>(0.,-2.))+fx_sample(p+vec2<f32>(0.,2.)))/3.;}".into();
        program.passes = ["preview_h", "preview_v"]
            .map(|entry| EffectPass {
                entry: entry.into(),
                sampling: EffectSampling::Neighborhood { radius: 2 },
            })
            .into();
        let effect = Arc::new(EffectInstance::new(Arc::new(program)));
        let mut filter = Layer::paint(LayerId(2), "Blur");
        filter.kind = LayerKind::Effect;
        filter.effect = Some(effect.clone());
        let mut layers = vec![filter, base];
        let view = layer_render::ViewState {
            width_px: 512,
            height_px: 256,
            document_to_surface: [1., 0., 0., 1., 0., 0.],
            background_rgba_linear: [0.; 4],
        };
        r.submit(FramePacket {
            time_seconds: 0.,
            view,
            document_extent: [512, 256],
            layers: &layers,
            dabs: &[],
            dab_batches: &[],
            reset_layers: true,
            composite_all: true,
        })
        .unwrap();
        let full = r.readback_srgb_rgba8().unwrap();
        r.request_filter_previews(FilterPreviewRequest {
            request_id: 1,
            target: LayerId(1),
            size: [200, 40],
            extent: [512, 256],
            view,
            layers: layers.iter().map(Layer::composite_snapshot).collect(),
            filters: vec![effect],
        })
        .unwrap();
        let preview = finish(&mut r).image;
        assert_eq!(r.filter_previews.as_ref().unwrap().point, Some([256, 128]));
        let mut compared = 0;
        for y in 0..40 {
            for x in 0..200 {
                let small = &preview.bytes[(y * 200 + x) * 4..][..4];
                if small[3] == 255 {
                    let large = &full[((y + 108) * 512 + x + 156) * 4..][..4];
                    for c in 0..4 {
                        assert!(
                            (small[c] as i16 - large[c] as i16).abs() <= 1,
                            "({x},{y}) preview={small:?} canvas={large:?}"
                        );
                    }
                    compared += 1;
                }
            }
        }
        assert!(compared > 500);
        assert!(
            r.filter_previews.as_ref().unwrap().scratch_size[0] < 512,
            "bounded passes render crop plus halo, not the whole document"
        );
        // Every built-in preview executes the exact canvas algorithm, including
        // document-coordinate warps and original-input reads in later passes.
        r.filter_previews = None;
        for id in fixtures() {
            layers[0].effect = Some(Arc::new(id.preview().unwrap()));
            r.submit(FramePacket {
                time_seconds: 0.,
                view,
                document_extent: [512, 256],
                layers: &layers,
                dabs: &[],
                dab_batches: &[],
                reset_layers: false,
                composite_all: true,
            })
            .unwrap();
            let full = r.readback_srgb_rgba8().unwrap();
            r.request_filter_previews(FilterPreviewRequest {
                request_id: 2,
                target: LayerId(1),
                size: [200, 40],
                extent: [512, 256],
                view,
                layers: layers.iter().map(Layer::composite_snapshot).collect(),
                filters: vec![Arc::new(id.preview().unwrap())],
            })
            .unwrap();
            let preview = finish(&mut r).image;
            let mut compared = 0;
            for y in 0..40 {
                for x in 0..200 {
                    let small = &preview.bytes[(y * 200 + x) * 4..][..4];
                    if small[3] == 255 {
                        let large = &full[((y + 108) * 512 + x + 156) * 4..][..4];
                        assert!(
                            small
                                .iter()
                                .zip(large)
                                .all(|(&a, &b)| (a as i16 - b as i16).abs() <= 1),
                            "{} ({x},{y}): preview={small:?} canvas={large:?}",
                            id.label()
                        );
                        compared += 1;
                    }
                }
            }
            assert!(compared > 500, "{} opaque preview coverage", id.label());
        }
    }
}
