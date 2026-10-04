//! Idle-time GPU previews: a chunked source probe per revision, shared source
//! crops and pipelines, and small asynchronous image readbacks.
use super::metadata::PreviewMetadata;
use super::*;
use layer_render::{FilterPreviewImage, FilterPreviewRequest, FilterPreviewSource};
use std::{collections::HashMap, sync::Arc};
use wgpu::util::DeviceExt;

type Image = (wgpu::Texture, wgpu::TextureView);
enum Ready {
    Point(Result<u32, GpuRasterError>),
    ProbeNext(Result<u32, GpuRasterError>),
    Pixels(Result<ReadbackImage, GpuRasterError>),
}
type SourceKey = (u64, FilterPreviewSource, [u32; 2], layer_core::BlendSpace);
pub(crate) struct FilterPreviews {
    scene: Scene,
    source_scene: Scene,
    probe_next: u32,
    probe_winner: Option<wgpu::Buffer>,
    cancelled: bool,
    programs: HashMap<Arc<str>, Layer>,
    probe: Deferred<wgpu::ComputePipeline>,
    mask: Image,
    source: Option<Image>,
    key: Option<SourceKey>,
    source_layers: Vec<PreviewMetadata>,
    point: Option<[u32; 2]>,
    scratch: Vec<Image>,
    scratch_size: [u32; 2],
    size: [u32; 2],
    rows: HashMap<Arc<str>, Vec<u8>>,
    request: Option<FilterPreviewRequest>,
    rendering: Vec<Arc<str>>,
    analyses: Vec<Arc<crate::effect_analysis::Prepared>>,
    analysis: Option<crate::effect_analysis::Job>,
    analysis_queries: Vec<layer_core::ArtworkQuery>,
    preparing: bool,
    tx: mpsc::Sender<Ready>,
    rx: mpsc::Receiver<Ready>,
}
impl FilterPreviews {
    pub(crate) fn rendition_changed(&mut self) {
        self.rows.clear();
        self.cancelled |= self.request.is_some();
    }
    fn new(r: &mut WgpuRasterizer) -> Result<Self, GpuRasterError> {
        let scene = Scene::new(r);
        let shader = r.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("filter preview content probe"),
            source: wgpu::ShaderSource::Wgsl(include_str!("filter_probe.wgsl").into()),
        });
        let device = r.device.clone();
        let probe = Deferred::pipeline(move |mode| mode.compute(&device, &wgpu::ComputePipelineDescriptor {
                label: Some("filter preview content probe"),
                layout: None,
                module: &shader,
                entry_point: Some("measure"),
                compilation_options: Default::default(),
                cache: None,
            }));
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
        let mask = create_target(
            &r.device,
            [info.width, info.height],
            wgpu::TextureFormat::Rgba8UnormSrgb,
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
            source_scene: Scene::new(r),
            probe_next: 0,
            probe_winner: None,
            cancelled: false,
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
            analyses: Vec::new(), analysis: None, analysis_queries: Vec::new(), preparing: false,
            tx,
            rx,
        })
    }
    pub(crate) fn note_frame(&mut self, packet: FramePacket<'_>, epoch: u64) {
        if let Some(request) = &self.request {
            self.cancelled |= self
                .key
                .is_none_or(|key| key.0 != epoch || key.2 != packet.document_extent || key.3 != packet.blend_space)
                || source_scope(packet.layers, request.source).is_none_or(|(_, mut scope)| {
                    let mut old = self.source_layers.iter();
                    !scope.all(|(layer, _)| {
                        old.next()
                            .is_some_and(|old| *old == PreviewMetadata::new(layer))
                    }) || old.next().is_some()
                });
        }
    }
    fn start(
        &mut self,
        r: &mut WgpuRasterizer,
        request: FilterPreviewRequest,
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
        if let Some(startup) = &r.startup {
            // Visible rows prepare just their own preview variants. No draw or
            // readback is admitted until their asynchronous pipelines are ready.
            for effect in &request.filters {
                self.scene.effects.prepare(r, &[&self.programs[&effect.program.id]], effects::Execution::Preview, 0., 0, preview_space(effect, request.blend_space))?;
            }
            let source_layers = source_scope(&request.layers, request.source)
                .map_or_else(Vec::new, |(_, layers)| layers.map(|(l, _)| l.clone()).collect());
            for (layers, execution) in scene::startup_effect_chains(&source_layers) {
                self.source_scene.effects.prepare(r, &layers, execution, 0., 0, request.blend_space)?;
            }
            let mut ready = self.scene.effects.enqueue(&startup.compiler, startup::OTHER);
            ready &= self.source_scene.effects.enqueue(&startup.compiler, startup::OTHER);
            startup.compiler.pipeline(&self.probe, startup::OTHER);
            // Preview readback uses the same conversion as document export.
            ready &= r.ui_readback_ready() && self.probe.ready();
            startup.compiler.start();
            if !ready { return Ok(false); }
        }
        self.cancelled = false;
        let key = (
            r.filter_source_epoch,
            request.source,
            request.extent,
            request.blend_space,
        );
        let resized = self.size != request.size;
        if resized {
            self.rows.clear();
            self.size = request.size;
        }
        let source_layers: Vec<_> =
            source_scope(&request.layers, request.source).map_or_else(Vec::new, |(_, scope)| {
                scope
                    .map(|(layer, _)| PreviewMetadata::new(layer))
                    .collect()
            });
        let changed = self.key != Some(key) || self.source_layers != source_layers || resized;
        // The common UI driver retains delivered rows. Keep only the current
        // bounded request here, rather than a second catalog-sized pixel cache.
        self.rows.retain(|id, _| request.filters.iter().any(|f| f.program.id == *id));
        self.request = Some(request);
        if changed {
            self.clear_analysis();
            self.source_layers = source_layers;
            self.rows.clear();
            self.key = Some(key);
            self.point = None;
            self.probe_next = 0;
            self.probe_winner = Some(r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("preview content coordinate"),
                size: 8,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        self.queue_analyses(r)?;
        self.preparing = true;
        self.prepare_source(r)?;
        Ok(true)
    }
    fn with_analyses<T>(&mut self, r: &mut WgpuRasterizer, operation: impl FnOnce(&mut Self, &mut WgpuRasterizer) -> T) -> T {
        let live = std::mem::replace(&mut r.effect_analyses, self.analyses.clone());
        let result = operation(self, r);
        r.effect_analyses = live;
        result
    }
    fn queue_analyses(&mut self, r: &WgpuRasterizer) -> Result<(), GpuRasterError> {
        let request = self.request.as_ref().unwrap();
        let (parent, layers) = source_layers(&request.layers, request.source)
            .ok_or_else(|| GpuRasterError::Effect("Missing filter insertion layer".into()))?;
        let mut document = layer_core::Document::new("", request.extent[0], request.extent[1],
            layer_core::DocumentNames {paint:"".into(), paper:"".into()});
        document.layers = layers;document.color = r.document_color;document.blend_space = request.blend_space;
        for layer in &document.layers {
            if layer_core::layer_is_visible(&document.layers, layer.id)
                && layer.effect.as_ref().is_some_and(|effect| effect.program.analysis().is_some())
                && !self.analyses.iter().any(|entry| entry.layer() == layer.id) {
                self.analysis_queries.push(layer_core::ArtworkQuery::new(&document, layer_core::ArtworkSource::EffectInput(layer.id)));
            }
        }
        for effect in &request.filters {
            if effect.program.analysis().is_none() {continue;}
            let mut layer = self.programs[&effect.program.id].clone();
            layer.kind = LayerKind::Effect;layer.properties.parent = parent;
            if self.analyses.iter().any(|entry| entry.layer() == layer.id) {continue;}
            let mut source = document.clone();source.layers.insert(0,layer.clone());
            let query = layer_core::ArtworkQuery::new(&source, layer_core::ArtworkSource::EffectInput(layer.id));
            self.analysis_queries.push(query);
        }
        Ok(())
    }
    fn prepare_source(&mut self, r: &mut WgpuRasterizer) -> Result<(), GpuRasterError> {
        if let Some(job) = &mut self.analysis {
            let Some(result) = job.take() else {return Ok(());};
            self.analysis = None;
            for entry in result.map_err(GpuRasterError::Effect)?.entries {
                self.analyses.retain(|old| old.layer() != entry.layer());self.analyses.push(entry);
            }
            self.source_scene.analysis_changed();
        }
        while let Some(query) = self.analysis_queries.pop() {
            let layer = match query.source {layer_core::ArtworkSource::EffectInput(id)=>id,_=>unreachable!()};
            if self.analyses.iter().any(|entry| entry.layer() == layer) {continue;}
            let kind = query.document.layer(layer)
                .and_then(|layer| layer.effect.as_ref()).and_then(|effect| effect.program.analysis()).unwrap();
            if self.programs.values().any(|program| program.id == layer)
                && let Some(resource) = self.analyses.iter().find(|entry| entry.kind == kind
                    && self.programs.values().any(|program| program.id == entry.layer())).map(|entry| entry.resource.clone()) {
                self.analyses.push(Arc::new(crate::effect_analysis::Prepared {query,kind,resource}));continue;
            }
            let document = &query.document;
            let input = crate::effect_analysis::BakeInput {members:document.layers.clone().into(), offset:Default::default(),
                extent:[document.width,document.height],color:document.color,blend:document.blend_space,time:query.time};
            self.analysis = Some(self.with_analyses(r, |_,r| crate::effect_analysis::Job::frame(r.snapshot_gpu(), input))
                .map_err(GpuRasterError::Effect)?);
            return Ok(());
        }
        self.preparing = false;
        if self.probe_winner.is_some() {self.with_analyses(r,Self::probe_batch)}
        else if self.missing_rows() {self.with_analyses(r,Self::render)} else {Ok(())}
    }
    /// Scan four source tiles per completion. Only one chunk can be in flight;
    /// the retained source and spatial boundaries cover the tile plus its halo.
    fn probe_batch(&mut self, r: &mut WgpuRasterizer) -> Result<(), GpuRasterError> {
        let request = self.request.as_ref().unwrap();
        let extent = request.extent;
        let columns = extent[0].div_ceil(PAGE_SIZE);
        let count = columns * extent[1].div_ceil(PAGE_SIZE);
        // Every tile uses the same queue-ordered window. Allocating a texture
        // per tile leaves gigabytes awaiting browser GC on large documents,
        // even though Rust retains only the most recent handle. Edge windows
        // occupy the top-left prefix; probe samples stay inside the captured
        // region (or outside the document, where the shader rejects them).
        let size = std::array::from_fn(|i| extent[i].min(PAGE_SIZE + request.size[i]));
        if self.source.as_ref().is_none_or(|(texture, _)| {
            [texture.width(), texture.height()] != size
        }) {
            self.source = Some(create_color_target(&r.device, size, "filter probe window"));
        }
        let winner = self.probe_winner.as_ref().unwrap();
        let mut encoder = crate::submission::CommandEncoder::new(
            &r.device,
            &wgpu::CommandEncoderDescriptor {
                label: Some("filter content probe chunk"),
            },
        );
        if self.probe_next == 0 {
            encoder.clear_buffer(winner, 0, None);
        }
        let mut backdrop = [0.; 4];
        let (parent, layers) = source_layers(&request.layers, request.source)
            .ok_or_else(|| GpuRasterError::Effect("Missing filter insertion layer".into()))?;
        if parent.is_none() {
            for layer in layer_core::constant_backdrop(&layers).iter().rev().filter(|l| l.visible) {
                let [red, green, blue, alpha] = layer.effect.as_ref().unwrap().constant_color().unwrap()
                    .linear_in(r.device.working_space()).map_err(GpuRasterError::Color)?;
                let alpha = alpha * layer.opacity;
                let color = request.blend_space.composite(r.device.working_space(), [red * alpha, green * alpha, blue * alpha, alpha]);
                backdrop = std::array::from_fn(|c| color[c] + backdrop[c] * (1. - alpha));
            }
        }
        for _ in 0..4 {
            if self.probe_next == count {
                break;
            }
            let tile = [self.probe_next % columns, self.probe_next / columns];
            let core = page_rect(tile).intersect(PixelRect::full(extent));
            let region = PixelRect::new(
                core.min_x().saturating_sub(request.size[0] / 2),
                core.min_y().saturating_sub(request.size[1] / 2),
                core.max_x()
                    .saturating_add(request.size[0] / 2)
                    .min(extent[0]),
                core.max_y()
                    .saturating_add(request.size[1] / 2)
                    .min(extent[1]),
            );
            let (texture, view) = self.source.as_ref().unwrap();
            self.source_scene
                .capture_filter_source(r, request, texture, region, &mut encoder)?;
            let mut data = Vec::with_capacity(64);
            for v in backdrop { data.extend(v.to_le_bytes()); }
            for v in [
                request.size[0],
                request.size[1],
                extent[0],
                extent[1],
                region.min_x(),
                region.min_y(),
                core.min_x(),
                core.min_y(),
                core.width(),
                core.height(),
                0,
                0,
            ] {
                data.extend(v.to_le_bytes());
            }
            let uniform = r
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("preview probe window"),
                    contents: &data,
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let binding = crate::bindings::group(&r.device, "preview probe", &self.probe.get_bind_group_layout(0), [
                wgpu::BindingResource::TextureView(view),
                uniform.as_entire_binding(),
                winner.as_entire_binding(),
            ]);
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("find preview content"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.probe);
                pass.set_bind_group(0, &binding, &[]);
                pass.dispatch_workgroups(core.width().div_ceil(16), core.height().div_ceil(16), 1);
            }
            let next = crate::submission::CommandEncoder::new(
                &r.device,
                &wgpu::CommandEncoderDescriptor {
                    label: Some("next filter probe window"),
                },
            );
            let encoded = std::mem::replace(&mut encoder, next);
            r.uploads.finish(&encoded);
            encoded.submit(&r.queue);
            self.probe_next += 1;
        }
        let last = self.probe_next == count;
        let read = r.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("preview coordinate pair"),
            size: 8,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(winner, 0, &read, 0, 8);
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        let tx = self.tx.clone();
        crate::raster::map_then(
            &read,
            8,
            |bytes| {
                let preferred = u32::from_le_bytes(bytes[..4].try_into().unwrap());
                Ok(if preferred > 0 {
                    preferred
                } else {
                    u32::from_le_bytes(bytes[4..8].try_into().unwrap())
                })
            },
            move |result| {
                let _ = tx.send(if last {
                    Ready::Point(result)
                } else {
                    Ready::ProbeNext(result)
                });
            },
        );
        Ok(())
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
        let fallback_source = if self.point.is_some() {
            r.empty_view.clone()
        } else {
            fallback = create_color_target(&r.device, extent, "empty document filter sample");
            let mut data = [0.; 32];
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
                sources: [r.empty_view.clone(), r.empty_view.clone(), r.empty_view.clone()],
                data,
                over: false,
                clip: None,
            });
            fallback.1.clone()
        };
        let pad = self.rendering.iter().try_fold(0u32, |pad, id| {
            Some(pad.max(self.programs[id].effect.as_ref()?.damage_radius()?))
        });
        let source_bounds = pad.map_or(PixelRect::full(extent), |pad| {
            PixelRect::new(
                origin[0],
                origin[1],
                (origin[0] + width).min(extent[0]),
                (origin[1] + height).min(extent[1]),
            )
            .expand(pad, extent)
        });
        let source_grid = display_mips::Plan::window(extent, 0,
            if self.point.is_some() { source_bounds } else { PixelRect::full(extent) });
        let source = if self.point.is_some() {
            self.source = Some(create_color_target(
                &r.device,
                [source_bounds.width(), source_bounds.height()],
                "filter preview source crop",
            ));
            let (texture, view) = self.source.as_ref().unwrap();
            self.source_scene.capture_filter_source(
                r,
                request,
                texture,
                source_bounds,
                &mut encoder,
            )?;
            view.clone()
        } else {
            fallback_source
        };
        let spaces: Vec<_> = self.rendering.iter().map(|id| preview_space(self.programs[id].effect.as_ref().unwrap(), request.blend_space)).collect();
        let encoded;
        let encoded_source = if spaces.contains(&layer_core::BlendSpace::Perceptual) {
            let size = if self.point.is_some() { [source_bounds.width(), source_bounds.height()] } else { extent };
            encoded = create_color_target(&r.device, size, "encoded filter preview source");
            let [width, height] = size.map(|v| v as f32);
            let mut data = [0.; 32];
            data[..6].copy_from_slice(&[0., 0., width, height, width, height]);
            data[8..10].copy_from_slice(&[1., 1.]);
            data[31] = Convert::Encode.code();
            self.scene.jobs.push(Job::Draw {
                target: encoded.1.clone(),
                sources: [source.clone(), r.empty_view.clone(), r.empty_view.clone()],
                data,
                over: false,
                clip: None,
            });
            encoded.1.clone()
        } else {
            source.clone()
        };
        let atlas = create_color_target(
            &r.device,
            [width, height * self.rendering.len() as u32],
            "filter preview atlas",
        );
        for (row, id) in self.rendering.iter().enumerate() {
            let space = spaces[row];
            let source = if space == layer_core::BlendSpace::Perceptual { encoded_source.clone() } else { source.clone() };
            // Compile only requested rows. A catalog-wide dynamic switch would
            // compile every expensive kernel before showing even the first row.
            let prepared = self.scene.effects.prepare(
                r,
                &[&self.programs[id]],
                effects::Execution::Preview,
                0.,
                0,
                space,
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
                // Declared support can cover the entire document. Replicated
                // edge samples need no allocation beyond that input extent.
                [
                    width
                        .saturating_add(pad.saturating_mul(2))
                        .min(extent[0].max(width)),
                    height
                        .saturating_add(pad.saturating_mul(2))
                        .min(extent[1].max(height)),
                ]
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
            let grid = display_mips::Plan::window(extent, 0,
                PixelRect::new(crop[0], crop[1], crop[0] + size[0], crop[1] + size[1]));
            let previous = self.scene.preview_passes(r, prepared, grid, (source, source_grid), &self.scratch, count);
            let mut data = [0.; 32];
            data[..6].copy_from_slice(&[
                0.,
                (row as u32 * height) as f32,
                width as f32,
                height as f32,
                width as f32,
                (height * self.rendering.len() as u32) as f32,
            ]);
            data[8] = 10.;
            data[24..32].copy_from_slice(&r.ui_rendition_parameters());
            data[12..16].copy_from_slice(&[
                (origin[0] - crop[0]) as f32,
                (origin[1] - crop[1]) as f32,
                width as f32,
                height as f32,
            ]);
            self.scene.jobs.push(Job::Draw {
                target: atlas.1.clone(),
                sources: [previous, self.mask.1.clone(), r.empty_view.clone()],
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
        Ok(())
    }
    fn take(
        &mut self,
        r: &mut WgpuRasterizer,
    ) -> Option<Result<FilterPreviewImage, GpuRasterError>> {
        self.request.as_ref()?;
        if self.cancelled {self.clear_analysis();self.request=None;self.key=None;return Some(Err(GpuRasterError::FilterPreviewCancelled));}
        if self.preparing {
            if let Err(error) = self.prepare_source(r) {self.clear_analysis();self.request = None;self.key = None;return Some(Err(error));}
            return None;
        }
        // Even if the next completion arrives during encoding, yield to the
        // host after one chunk. A fast device must not turn polling into an
        // unbounded scan on the input/render owner.
        if let Ok(ready) = self.rx.try_recv() {
            let result = if self.cancelled {
                Err(GpuRasterError::FilterPreviewCancelled)
            } else {
                match ready {
                    Ready::ProbeNext(result) => result.and_then(|_| self.with_analyses(r,Self::probe_batch)),
                    Ready::Point(value) => value.and_then(|score| {
                        self.probe_winner = None;
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
                        self.with_analyses(r,Self::render)
                    }),
                    Ready::Pixels(image) => image.map(|image| {
                        let row_bytes = (self.size[0] * self.size[1] * 4) as usize;
                        for (id, bytes) in
                            self.rendering.drain(..).zip(image.bytes.chunks(row_bytes))
                        {
                            self.rows.insert(id, bytes.to_vec());
                        }
                    }),
                }
            };
            if let Err(error) = result {
                self.clear_analysis();
                self.request = None;
                self.probe_winner = None;
                self.key = None;
                return Some(Err(error));
            }
        }
        if self.request.is_none() || self.missing_rows() {
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
    fn preview_passes(&mut self, r: &WgpuRasterizer, prepared: effects::PreparedEffect,
        grid: display_mips::Plan, (source, source_grid): (wgpu::TextureView, display_mips::Plan),
        targets: &[Image], count: usize,
    ) -> wgpu::TextureView {
        let mut previous = source.clone();
        for stage in 0..count {
            let (texture, target) = &targets[stage % targets.len()];
            let mut data = effects::image_grid(grid, if stage == 0 { source_grid } else { grid }, source_grid);
            data[4..8].copy_from_slice(&[texture.width() as f32, texture.height() as f32, 0., stage as f32]);
            self.jobs.push(Job::Effect { target: target.clone(), sources: [previous, source.clone(), r.empty_view.clone()],
                data, prepared: prepared.clone(), masks: Box::new(std::array::from_fn(|_| r.empty_view.clone())) });
            previous = target.clone();
        }
        previous
    }

    pub(crate) fn generator_preview(&mut self, r: &mut WgpuRasterizer, layer: &Layer, grid: display_mips::Plan,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Option<Vec<Image>>, GpuRasterError> {
        self.begin_frame();
        self.jobs.clear();
        let frame = r.artwork_frame.as_ref().ok_or(GpuRasterError::InvalidExtent)?;
        self.effects.retain(&frame.layers);
        let prepared = self.effects.prepare(r, &[layer], effects::Execution::Preview, frame.time, grid.level,
            preview_space(layer.effect.as_ref().unwrap(), frame.blend_space))?;
        if let Some(startup) = &r.startup {
            let ready = self.effects.enqueue(&startup.compiler, startup::VALIDATION);
            startup.compiler.start();
            if !ready { return Ok(None); }
        }
        let count = layer.effect.as_ref().unwrap().program.passes.len().max(1);
        let targets: Vec<_> = (0..count.min(2)).map(|_| create_color_target(&r.device, grid.size, "generator thumbnail")).collect();
        self.preview_passes(r, prepared, grid, (r.empty_view.clone(), grid), &targets, count);
        self.encode_jobs(r, encoder)?;
        Ok(Some(targets))
    }
}
/// The blend space a filter's preview compiles for: the document's when the
/// filter reads the document's encoded values, otherwise Linear.
fn preview_space(effect: &layer_core::EffectInstance, blend: layer_core::BlendSpace) -> layer_core::BlendSpace {
    if effect.program.space.encoded(blend) { blend } else { layer_core::BlendSpace::Linear }
}
// Keep only the insertion scope and its ancestors: the target, its subtree
// and what composites below it, through Pass Through groups. An excluded
// global effect above the target must not force a full-document dependency.
fn source_scope(
    layers: &[Layer],
    source: FilterPreviewSource,
) -> Option<(Option<LayerId>, impl Iterator<Item = (&Layer, bool)>)> {
    let (target, include) = match source {
        FilterPreviewSource::LayerStack(id) => (id, true),
        FilterPreviewSource::EffectInput(id) => (id, false),
    };
    let index = layers.iter().position(|l| l.id == target)?;
    let parent = layers[index].properties.parent;
    let parent_of = |id: LayerId| {
        layers
            .iter()
            .find(|l| l.id == id)
            .and_then(|l| l.properties.parent)
    };
    let mut below = vec![false; layers.len()];
    for i in layer_core::backdrop_layers(layers, index) {
        below[i] = true;
    }
    let scope = layers.iter().enumerate().filter_map(move |(i, layer)| {
        if std::iter::successors(parent, |&id| parent_of(id)).any(|id| id == layer.id) {
            return Some((layer, true));
        }
        (below[i] || include && (i == index || layer_core::descends_from(layers, layer, Some(target)))).then_some((layer, false))
    });
    Some((layer_core::isolated_scope(layers, parent), scope))
}
fn source_layers(layers: &[Layer], source: FilterPreviewSource) -> Option<(Option<LayerId>, Vec<Layer>)> {
    let (parent, scope) = source_scope(layers, source)?;
    Some((parent, scope.map(|(layer, ancestor)| {
        let mut layer = layer.clone();
        if ancestor {
            layer.effect = None;
            if layer.passes_through() {layer.opacity = 1.;layer.mask = None;}
        }
        layer
    }).collect()))
}
impl Scene {
    fn capture_filter_source(
        &mut self,
        r: &mut WgpuRasterizer,
        request: &FilterPreviewRequest,
        destination: &wgpu::Texture,
        region: PixelRect,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        let (parent, layers) = source_layers(&request.layers, request.source)
            .ok_or_else(|| GpuRasterError::Effect("Missing filter insertion layer".into()))?;
        self.jobs.clear();
        self.used.fill(false);
        let packet = FramePacket {
            commit_rasters: true,
            time_seconds: 0.,
            view: request.view,
            document_extent: request.extent,
            layers: &layers,
            dabs: &[],
            dab_batches: &[],
            restore_rasters: &[],
            reset_layers: false,
            composite_all: false,
            blend_space: request.blend_space,
        };
        self.capture_region(r, packet, destination, region, scene::Output::Artwork(parent), encoder)
    }
}
impl WgpuRasterizer {
    pub(crate) fn cancel_filter_preview_request(&mut self) {
        let Some(previews) = self.filter_previews.as_mut() else { return; };
        previews.request = None;previews.clear_analysis();
        // Old callbacks retain only their sender. Dropping the receiver keeps
        // them from advancing or completing the next request, without a wait.
        (previews.tx, previews.rx) = mpsc::channel();
        previews.probe_winner = None;
        previews.rendering.clear();
        previews.key = None;
        previews.point = None;
        previews.rows.clear();
    }
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
            previews.clear_analysis();
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
        if previews.missing_rows() {
            let _ = self.device.poll(wgpu::PollType::Poll);
        }
        let result = previews.take(self);
        self.filter_previews = Some(previews);
        result
    }
}

impl FilterPreviews {
    fn clear_analysis(&mut self) {
        self.analysis = None;self.analysis_queries.clear();self.analyses.clear();self.preparing = false;
        self.scene.jobs.clear();self.source_scene.jobs.clear();
        self.scene.effects.retain(&[]);self.source_scene.effects.retain(&[]);
    }
    fn missing_rows(&self) -> bool {
        self.request.as_ref().is_some_and(|r| r.filters.iter().any(|f| !self.rows.contains_key(&f.program.id)))
    }
    pub(crate) fn storage_bytes(&self) -> u64 {
        let bytes = |(texture, _): &Image| texture_bytes(texture);
        self.source.as_ref().map_or(0, bytes)
            + bytes(&self.mask)
            + self.scratch.iter().map(bytes).sum::<u64>()
            + self.scene.scratch_bytes()
            + self.source_scene.scratch_bytes()
            + self.probe_winner.as_ref().map_or(0, wgpu::Buffer::size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::fixture;

    fn finish(r: &mut WgpuRasterizer) -> FilterPreviewImage {
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        loop {
            r.device.poll(wgpu::PollType::Wait {submission_index:None,timeout:Some(Duration::from_secs(10))}).unwrap();
            if let Some(result) = r.take_filter_previews() {return result.unwrap();}
            assert!(std::time::Instant::now() < deadline, "preview did not complete");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn pattern(id: LayerId) -> Layer {
        let mut pattern = Layer::paint(id, "source");
        let mut program = (*fixture("exposure").program()).clone();
        program.kind = layer_core::EffectKind::Generator;
        program.entry = "pattern".into();
        program.wgsl = "fn pattern(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(p.x/fx_extent().x,p.y/fx_extent().y,.2,1.);}".into();
        pattern.effect = Some(Arc::new(layer_core::EffectInstance::new(Arc::new(program))));
        pattern.kind = LayerKind::Effect;
        pattern
    }
    #[test]
    fn replacing_the_bottom_fill_captures_transparent_input() {
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let layers = vec![Layer::solid_color(LayerId(1), "Fill", layer_core::color::RgbColor::WHITE)];
        r.submit(crate::test_support::packet(&layers, [64; 2])).unwrap();
        let source = FilterPreviewSource::EffectInput(LayerId(1));
        assert!(source_layers(&layers, source).unwrap().1.is_empty());
        r.request_filter_previews(FilterPreviewRequest {
            request_id: 1, source, size: [120, 40], extent: [64; 2], view: crate::test_support::view([64; 2]),
            blend_space: Default::default(), layers, filters: vec![Arc::new(fixture("exposure").preview().unwrap())],
        }).unwrap();
        let atlas = finish(&mut r);
        assert_eq!(atlas.image.bytes.len(), 120 * 40 * 4);
        assert!(atlas.image.bytes.chunks_exact(4).any(|p| p[3] != 0));
        assert!(atlas.image.bytes.chunks_exact(4).any(|p| p[3] == 0));
    }
    #[test]
    fn a_filter_that_follows_the_documents_blending_previews_its_live_result() {
        let extent = [300, 200];
        let view = layer_render::ViewState {
            width_px: extent[0],
            height_px: extent[1],
            document_to_surface: [1., 0., 0., 1., 0., 0.],
        };
        let mut stripes = Layer::paint(LayerId(1), "stripes");
        stripes.source = Some(layer_core::color::source::rgba8_source(extent, |x, y| {
            let v = if (x / 9) % 2 == 0 { 20 } else { 235 };
            [v, (y / 2) as u8, 255 - v, 255]
        }));
        let definition = fixture("gaussian_blur");
        assert_eq!(definition.program().space, layer_core::EffectSpace::Blending);
        for space in layer_core::BlendSpace::ALL {
            let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
            let layers = [Layer::paint(LayerId(7), "target"), stripes.clone()];
            r.submit(FramePacket { view, blend_space: space, ..crate::test_support::packet(&layers, extent) }).unwrap();
            let size = [120, 40];
            r.request_filter_previews(FilterPreviewRequest {
                request_id: 1,
                source: FilterPreviewSource::LayerStack(LayerId(7)),
                size,
                extent,
                view,
                blend_space: space,
                layers: layers.iter().map(Layer::composite_snapshot).collect(),
                filters: vec![Arc::new(definition.preview().unwrap())],
            })
            .unwrap();
            let preview = finish(&mut r).image.bytes;
            let point = r.filter_previews.as_ref().unwrap().point.unwrap();
            let origin: [u32; 2] = std::array::from_fn(|i| (point[i] - size[i] / 2).min(extent[i] - size[i]));
            let mut blurred = layers.clone();
            blurred[0].kind = LayerKind::Effect;
            blurred[0].effect = Some(Arc::new(definition.preview().unwrap()));
            r.submit(FramePacket { view, blend_space: space, ..crate::test_support::packet(&blurred, extent) }).unwrap();
            let live = r.readback_srgb_rgba8().unwrap();
            let mut compared = 0;
            for y in 0..size[1] {
                for x in 0..size[0] {
                    let shown = &preview[((y * size[0] + x) * 4) as usize..][..4];
                    if shown[3] < 255 {
                        continue;
                    }
                    let document = &live[(((origin[1] + y) * extent[0] + origin[0] + x) * 4) as usize..][..3];
                    for c in 0..3 {
                        assert!(shown[c].abs_diff(document[c]) <= 1, "{space:?} at {x},{y}: preview {shown:?}, live {document:?}");
                    }
                    compared += 1;
                }
            }
            assert!(compared > 500, "{space:?}: {compared} opaque preview pixels");
        }
    }
    #[test]
    fn a_target_in_a_pass_through_group_previews_the_layers_below_the_group() {
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let extent = [300, 200];
        let view = layer_render::ViewState {
            width_px: extent[0],
            height_px: extent[1],
            document_to_surface: [1., 0., 0., 1., 0., 0.],
        };
        let mut group = Layer::paint(LayerId(5), "Pass Through");
        group.kind = LayerKind::Group;
        group.properties.blend = layer_core::LayerBlend::PassThrough;
        group.opacity = 0.5;
        let mut above = Layer::paint(LayerId(6), "above the target");
        above.kind = LayerKind::Effect;
        above.effect = Some(Arc::new(fixture("black_white").preview().unwrap()));
        above.properties.parent = Some(group.id);
        let mut target = Layer::paint(LayerId(7), "target");
        target.properties.parent = Some(group.id);
        let grouped = vec![group.clone(), above, target.clone(), pattern(LayerId(1))];
        target.properties.parent = None;
        let flat = vec![target, pattern(LayerId(1))];
        group.properties.blend = layer_core::LayerBlend::Normal;
        let isolated = [vec![group], grouped[1..].to_vec()].concat();
        let mut preview = |layers: &[Layer], request_id| {
            r.submit(FramePacket { view, ..crate::test_support::packet(layers, extent) }).unwrap();
            r.request_filter_previews(FilterPreviewRequest {
                request_id,
                source: FilterPreviewSource::LayerStack(LayerId(7)),
                size: [120, 40],
                extent,
                view,
                blend_space: Default::default(),
                layers: layers.iter().map(Layer::composite_snapshot).collect(),
                filters: vec![Arc::new(fixture("exposure").preview().unwrap())],
            })
            .unwrap();
            finish(&mut r).image.bytes
        };
        let passing = preview(&grouped, 1);
        assert_eq!(passing, preview(&flat, 2), "the source is what lies below, without the group's fade");
        assert_ne!(passing, preview(&isolated, 3), "an isolated group's source holds only its own layers");
    }
    #[test]
    fn filter_probe_chunks_bound_sources_and_cancel_changed_documents() {
        use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: SampleDepth::U16,
        })
        .unwrap();
        let extent = [2049, 513]; // 27 tiles, including partial right/bottom edges.
        let pattern = pattern(LayerId(1));
        let mut blur = Layer::paint(LayerId(2), "spatial source");
        let mut program = (*fixture("exposure").program()).clone();
        program.entry = "blur".into();
        program.wgsl = "fn blur(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return (fx_sample(p+vec2<f32>(3.,0.))+c+fx_sample(p-vec2<f32>(3.,0.)))/3.;}".into();
        program.passes = vec![layer_core::EffectPass {
            entry: "blur".into(),
            sampling: layer_core::EffectSampling::Neighborhood { radius: 3 },
        }]
        .into();
        blur.effect = Some(Arc::new(layer_core::EffectInstance::new(Arc::new(program))));
        blur.kind = LayerKind::Effect;
        let mut upper = Layer::paint(LayerId(3), "excluded upper correction");
        upper.kind = LayerKind::Effect;
        upper.effect = Some(Arc::new(fixture("black_white").preview().unwrap()));
        Arc::make_mut(&mut Arc::make_mut(upper.effect.as_mut().unwrap()).program).passes =
            vec![layer_core::EffectPass {
                entry: upper.effect.as_ref().unwrap().program.entry.clone(),
                sampling: layer_core::EffectSampling::Document,
            }]
            .into();
        let mut paper = Layer::solid_color(LayerId(4), "partial paper",
            layer_core::color::RgbColor::from_linear(layer_core::color::RgbSpace::Srgb, [0.1, 0.2, 0.3, 1.]).unwrap());
        paper.opacity = 0.5;
        let mut layers = vec![upper, blur, pattern, paper];
        let view = layer_render::ViewState {
            width_px: extent[0],
            height_px: extent[1],
            document_to_surface: [1., 0., 0., 1., 0., 0.],
        };
        let frame = |r: &mut WgpuRasterizer, layers: &[Layer]| {
            r.submit(FramePacket {
                commit_rasters: true,
                time_seconds: 0.,
                view,
                document_extent: extent,
                layers,
                dabs: &[],
                dab_batches: &[],
                restore_rasters: &[],
                reset_layers: false,
                composite_all: true,
                blend_space: Default::default(),
            })
            .unwrap()
        };
        frame(&mut r, &layers);
        let request = |layers: &[Layer], id| FilterPreviewRequest {
            request_id: id,
            source: FilterPreviewSource::LayerStack(LayerId(2)),
            size: [200, 40],
            extent,
            view,
            blend_space: Default::default(),
            layers: layers.iter().map(Layer::composite_snapshot).collect(),
            filters: vec![Arc::new(fixture("exposure").preview().unwrap())],
        };
        r.request_filter_previews(request(&layers[1..], 1)).unwrap();
        let p = r.filter_previews.as_ref().unwrap();
        assert_eq!(p.probe_next, 4, "first call submits one bounded chunk");
        assert!(p.request.is_some());
        let probe_texture = p.source.as_ref().unwrap().0.clone();
        // A view-only frame must compare the caller's paper color, before the
        // compositor applies paper opacity. It must not cancel this scan.
        frame(&mut r, &layers);
        let mut callbacks = 0;
        let mut previous_probe_next = 4;
        loop {
            r.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(READBACK_TIMEOUT),
                })
                .unwrap();
            let result = r.take_filter_previews();
            let p = r.filter_previews.as_ref().unwrap();
            assert!(p.probe_next - previous_probe_next <= 4, "one poll advances at most one chunk");
            previous_probe_next = p.probe_next;
            let (texture, _) = p.source.as_ref().unwrap();
            if p.point.is_none() {
                assert_eq!(texture, &probe_texture, "all probe chunks reuse one texture");
            }
            assert!(texture.width() <= PAGE_SIZE + 200 && texture.height() <= PAGE_SIZE + 40);
            assert!(
                p.source_scene.image_cache_bytes()
                    <= 3 * (PAGE_SIZE + 206) as u64 * (PAGE_SIZE + 46) as u64 * 16
            );
            callbacks += 1;
            if let Some(result) = result {
                assert_eq!(result.unwrap().image.request_id, 1);
                break;
            }
            assert!(callbacks < 50);
        }
        let p = r.filter_previews.as_ref().unwrap();
        assert_eq!(p.probe_next, 27);
        assert_eq!(p.point, Some([1024, 256]));
        assert!(callbacks >= 7);
        assert!(p.probe_winner.is_none());
        // A metadata edit between chunks cancels the old request after its
        // in-flight callback. It cannot combine different document revisions.
        r.filter_previews.as_mut().unwrap().key = None;
        r.request_filter_previews(request(&layers, 2)).unwrap();
        layers[1].opacity = 0.5;
        frame(&mut r, &layers);
        r.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(READBACK_TIMEOUT),
            })
            .unwrap();
        assert!(matches!(r.take_filter_previews(), Some(Err(GpuRasterError::FilterPreviewCancelled))));
        assert!(!r.filter_previews_pending());
        assert_eq!(r.filter_previews.as_ref().unwrap().probe_next, 4);
        r.request_filter_previews(request(&layers, 3)).unwrap();
        assert_eq!(finish(&mut r).image.request_id, 3);
        // Explicit hide/input cancellation never waits for the old callback.
        // A replacement request can start immediately on a fresh channel.
        r.filter_previews.as_mut().unwrap().key = None;
        r.request_filter_previews(request(&layers, 30)).unwrap();
        let old_sender = r.filter_previews.as_ref().unwrap().tx.clone();
        r.cancel_filter_previews();
        assert!(!r.filter_previews_pending());
        assert!(r.take_filter_previews().is_none());
        assert!(old_sender.send(Ready::ProbeNext(Ok(0))).is_err());
        r.request_filter_previews(request(&layers, 31)).unwrap();
        assert_eq!(finish(&mut r).image.request_id, 31);
        // A valid conservative support declaration can exceed the adapter's
        // texture dimension. Its actual dependency still ends at the document.
        let mut wide = request(&layers, 4);
        let mut program = (*fixture("exposure").program()).clone();
        program.id = "test:wide-support".into();
        program.passes = (0..3)
            .map(|_| layer_core::EffectPass {
                entry: program.entry.clone(),
                sampling: layer_core::EffectSampling::Neighborhood { radius: 4096 },
            })
            .collect::<Vec<_>>()
            .into();
        wide.filters = vec![Arc::new(layer_core::EffectInstance::new(Arc::new(program)))];
        r.request_filter_previews(wide).unwrap();
        assert_eq!(finish(&mut r).image.request_id, 4);
        assert_eq!(r.filter_previews.as_ref().unwrap().scratch_size, extent);
    }

    #[test]
    fn p25_full_source_thumbnail_guides_reuse_invalidate_and_cancel() {
        let extent = [512, 256];
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut source = Layer::paint(LayerId(1), "source");
        source.source = Some(layer_core::color::source::rgba8_source(extent, |x,y| {
            let v = if x < 260 {12 + ((x/11+y/9)%2) as u8*55} else {160+(x%71) as u8};
            [v,v.saturating_add(8),v.saturating_add(16),255]
        }));
        let mut layers = vec![Layer::paint(LayerId(7), "picker target"), source];
        let frame = |r: &mut WgpuRasterizer, layers: &[Layer]| {r.submit(crate::test_support::packet(layers,extent)).unwrap();};
        frame(&mut r, &layers);
        let request = |layers: &[Layer], request_id, zero: bool| layer_render::FilterPreviewRequest {
            request_id,source: FilterPreviewSource::LayerStack(LayerId(7)),size:[120,40],extent,view:crate::test_support::packet(layers,extent).view,
            blend_space:Default::default(),layers:layers.iter().map(Layer::composite_snapshot).collect(),
            filters:[("shadows_highlights","shadows"),("clarity","amount")].into_iter().map(|(id,key)| {
                let mut effect=fixture(id).preview().unwrap();effect.set(key,layer_core::EffectValue::Number(if zero {0.} else {50.})).unwrap();Arc::new(effect)
            }).collect(),
        };
        assert!(r.request_filter_previews(request(&layers,1,false)).unwrap());
        let nonzero=finish(&mut r).image.bytes;
        let previews=r.filter_previews.as_ref().unwrap();
        let entries:Vec<_>=previews.analyses.iter().filter(|entry|previews.programs.values().any(|layer|layer.id==entry.layer())).collect();
        assert_eq!(entries.len(),2);assert!(Arc::ptr_eq(&entries[0].resource,&entries[1].resource));
        let resource=Arc::downgrade(&entries[0].resource);
        let origin=previews.point.unwrap();assert!(origin[0]>0 || origin[1]>0);
        assert!(r.effect_analyses.is_empty(),"private preview guides never replace live document guides");
        assert!(r.request_filter_previews(request(&layers,2,true)).unwrap());
        let identity=finish(&mut r).image.bytes;
        for row in 0..2 {
            let start=row*120*40*4;let end=start+120*40*4;
            assert_ne!(&nonzero[start..end],&identity[start..end],"row{row} must use source-aware guide");
        }
        let current=&r.filter_previews.as_ref().unwrap().analyses[0].resource;
        assert!(std::sync::Weak::ptr_eq(&resource,&Arc::downgrade(current)),"same full source reuses immutable guide");
        layers[1].source=Some(layer_core::color::source::rgba8_source(extent,|x,y|[20+(x%101) as u8,35+(y%113) as u8,25,255]));
        frame(&mut r,&layers);
        assert!(r.request_filter_previews(request(&layers,3,false)).unwrap());
        let edited=finish(&mut r).image.bytes;
        assert_ne!(nonzero,edited,"source edit invalidates rows");
        let current=&r.filter_previews.as_ref().unwrap().analyses[0].resource;
        assert!(!std::sync::Weak::ptr_eq(&resource,&Arc::downgrade(current)),"source edit replaces guide");
        let released=Arc::downgrade(current);
        r.cancel_filter_previews();
        assert!(!r.filter_previews_pending());
        assert!(r.filter_previews.as_ref().unwrap().analyses.is_empty());
        assert!(released.upgrade().is_none(),"hide cancels and releases completed private guide resources");
        assert!(r.request_filter_previews(request(&layers,4,false)).unwrap());
        r.cancel_filter_previews();
        assert!(!r.filter_previews_pending());
        assert!(r.filter_previews.as_ref().unwrap().analysis.is_none());
        assert!(r.take_filter_previews().is_none());
    }

    #[test]
    fn p25_source_aware_thumbnail_uses_insertion_group_scope() {
        let extent = [300,200];
        for id in ["shadows_highlights", "clarity"] {
            let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
            let mut group = Layer::paint(LayerId(5), "group");
            group.kind = LayerKind::Group; group.properties.blend = layer_core::LayerBlend::PassThrough; group.opacity = 0.5;
            let mut target = Layer::paint(LayerId(7), "target"); target.properties.parent = Some(group.id);
            let mut above = Layer::paint(LayerId(6), "excluded upper effect"); above.kind = LayerKind::Effect;
            above.effect = Some(Arc::new(fixture("invert").preview().unwrap())); above.properties.parent = Some(group.id);
            let source = pattern(LayerId(1));
            let grouped = vec![group.clone(),above,target.clone(),source.clone()];
            target.properties.parent = None;
            let flat = vec![target,source];
            group.properties.blend = layer_core::LayerBlend::Normal;
            let isolated = [vec![group],grouped[1..].to_vec()].concat();
            let mut preview = |layers: &[Layer], request_id| {
                let packet = crate::test_support::packet(layers,extent); let view = packet.view;
                r.submit(packet).unwrap();
                assert!(r.request_filter_previews(FilterPreviewRequest {request_id,source: FilterPreviewSource::LayerStack(LayerId(7)),size:[120,40],extent,view,
                    blend_space:Default::default(),layers:layers.iter().map(Layer::composite_snapshot).collect(),
                    filters:vec![Arc::new(fixture(id).preview().unwrap())]}).unwrap());
                finish(&mut r).image.bytes
            };
            let passing = preview(&grouped,1);
            assert_eq!(passing,preview(&flat,2),"{id}: full-source guide ignores upper effects and pass-through fade");
            assert_ne!(passing,preview(&isolated,3),"{id}: isolated insertion source excludes outside backdrop");
        }
    }

    #[test]
    fn p25_source_aware_thumbnail_preserves_selected_clipping_base() {
        let extent = [300,200];
        let generated = |id, code: &str| {
            let mut layer = pattern(id);
            let mut effect = (**layer.effect.as_ref().unwrap()).clone();
            Arc::make_mut(&mut effect.program).wgsl = code.into();
            layer.effect = Some(Arc::new(effect)); layer
        };
        let upper_code = "fn pattern(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(.08+p.x/fx_extent().x*.15,.04+p.y/fx_extent().y*.12,.1,1.);}";
        let flat_code = "fn pattern(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{if p.x>=fx_extent().x*.5{return vec4<f32>(0.);}return vec4<f32>(.08+p.x/fx_extent().x*.15,.04+p.y/fx_extent().y*.12,.1,1.);}";
        let base_code = "fn pattern(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{if p.x>=fx_extent().x*.5{return vec4<f32>(0.);}return vec4<f32>(0.,0.,0.,1.);}";
        let mut selected = generated(LayerId(7),upper_code);selected.properties.clipped = true;
        let clipped = vec![selected,generated(LayerId(1),base_code)];
        let flat = vec![Layer::paint(LayerId(7),"target"),generated(LayerId(1),flat_code)];
        for id in ["shadows_highlights","clarity"] {
            let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
            let mut preview = |layers: &[Layer], request_id| {
                let packet = crate::test_support::packet(layers,extent);let view = packet.view;r.submit(packet).unwrap();
                assert!(r.request_filter_previews(FilterPreviewRequest {request_id,source: FilterPreviewSource::LayerStack(LayerId(7)),size:[120,40],extent,view,
                    blend_space:Default::default(),layers:layers.iter().map(Layer::composite_snapshot).collect(),
                    filters:vec![Arc::new(fixture(id).preview().unwrap())]}).unwrap());
                finish(&mut r).image.bytes
            };
            assert_eq!(preview(&clipped,1),preview(&flat,2),"{id}: clipping input and equivalent composed source match");
        }
    }

}
