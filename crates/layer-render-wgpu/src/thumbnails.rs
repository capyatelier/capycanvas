//! Asynchronous 32px previews. The GPU worker never waits for thumbnail maps.
use super::*;
use wgpu::util::DeviceExt;
use layer_render::ThumbnailTarget;
pub(super) struct Thumbnails {
    tx: mpsc::Sender<Result<ReadbackImage, GpuRasterError>>,
    rx: mpsc::Receiver<Result<ReadbackImage, GpuRasterError>>,
    pending: usize,
    gpu: Option<PreviewPipeline>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) sources: Option<crate::source_thumbnails::SourceThumbnails>,
    pub source_placements: std::collections::BTreeMap<SourceTarget, layer_core::LayerPlacement>,
}
impl Thumbnails {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            pending: 0,
            gpu: None,
            #[cfg(not(target_arch = "wasm32"))]
            sources: None,
            source_placements: Default::default(),
        }
    }
    pub fn take(&mut self) -> Option<Result<ReadbackImage, GpuRasterError>> {
        let image = self.rx.try_recv().ok()?;
        self.pending = self.pending.saturating_sub(1);
        Some(image)
    }
    pub fn storage_bytes(&self) -> u64 {
        let paint = self.gpu.as_ref().map_or(0, |gpu| gpu.prepared.iter()
            .map(|p| p.records.size() + 16 + texture_bytes(&p.result.texture)
                + p.generated.iter().map(|(texture, _)| texture_bytes(texture)).sum::<u64>()).sum::<u64>()
            + gpu.generators.as_ref().map_or(0, scene::Scene::scratch_bytes));
        #[cfg(not(target_arch = "wasm32"))]
        return paint + self.sources.as_ref().map_or(0, |s| s.storage_bytes());
        #[cfg(target_arch = "wasm32")]
        paint
    }
}
impl WgpuRasterizer {
    pub fn ui_readback_ready(&self) -> bool {
        if let Some(startup) = &self.startup {
            startup.compiler.pipeline(&self.pipelines.export, startup::VALIDATION);
            startup.compiler.start();
            return self.pipelines.export.ready();
        }
        true
    }
    pub fn set_ui_rendition(&mut self, rendition: Option<layer_core::color::hdr::SdrRendition>) -> Result<(), GpuRasterError> {
        if let Some(recipe) = rendition { recipe.validate().map_err(|e| GpuRasterError::Color(e.into()))?; }
        if self.ui_rendition != rendition {
            if let Some(previews) = &mut self.filter_previews {
                previews.rendition_changed();
            }
            self.ui_rendition = rendition;
        }
        Ok(())
    }
    pub(super) fn ui_rendition_parameters(&self) -> [f32; 8] {
        self.ui_rendition.map_or([0.; 8], |r| r.parameters())
    }

    /// Configure the display-only byte outputs before requesting any previews.
    /// Native saves, exact sampling and exports retain their own color contracts.
    pub fn configure_ui_previews(
        &mut self,
        space: layer_core::color::RgbSpace,
    ) -> Result<(), GpuRasterError> {
        if self.thumbnails.pending != 0 || self.filter_previews.is_some() {
            return Err(GpuRasterError::Color(
                "Configure preview color before starting UI image jobs".into(),
            ));
        }
        self.ui_preview_space = space;
        self.ui_preview_pipeline = None;
        self.thumbnails = Thumbnails::new();
        Ok(())
    }

    fn validate_thumbnail(&self, target: ThumbnailTarget) -> Result<(), GpuRasterError> {
        let Some(frame) = &self.artwork_frame else { return Err(GpuRasterError::ThumbnailUnavailable(target)); };
        let scene = frame.scene.view();
        let available = match target {
            ThumbnailTarget::Occurrence(handle) => scene.occurrence(handle).is_some(),
            ThumbnailTarget::Source(SourceTarget::Paint(handle)) => scene.paint(handle).is_some(),
            ThumbnailTarget::Source(SourceTarget::Coverage(handle)) => scene.coverage(handle).is_some(),
            ThumbnailTarget::Source(SourceTarget::Selection(handle)) => scene.artwork().selections.get(handle).is_some(),
            ThumbnailTarget::QuickMask => true,
        };
        if available { Ok(()) } else { Err(GpuRasterError::ThumbnailUnavailable(target)) }
    }
    pub(super) fn thumbnail_source(&self, target: ThumbnailTarget) -> Option<SourceTarget> {
        match target {
            ThumbnailTarget::Source(source) => Some(source),
            ThumbnailTarget::QuickMask => Some(SourceTarget::Selection(layer_core::authored::SelectionHandle::INVALID)),
            ThumbnailTarget::Occurrence(handle) => self.artwork_frame.as_ref()?.scene.view().source_target(handle),
        }
    }
    pub fn thumbnails_pending(&self) -> bool {
        self.thumbnails.pending > 0
    }
    /// Prepare at most four source or paint pages before requesting readback.
    pub fn prepare_thumbnail_batch(&mut self, target: ThumbnailTarget) -> Result<bool, GpuRasterError> {
        self.validate_thumbnail(target)?;
        let source_target = self.thumbnail_source(target);
        if let Some(source) = source_target && !self.prepare_selection_thumbnail(source)? { return Ok(false); }
        let mut encoder = crate::submission::CommandEncoder::new(
            &self.device,
            &wgpu::CommandEncoderDescriptor {
                label: Some("background photo thumbnail batch"),
            },
        );
        #[cfg(not(target_arch = "wasm32"))]
        let source = if source_target.is_some_and(|source| self.tiled_sources.contains_key(&source)) && source_target.and_then(|source| self.thumbnails.source_placements.get(&source)).is_some_and(|p| p.as_affine().is_some()) {
            let mut gpu = self.thumbnails.sources.take()
                .unwrap_or_else(|| crate::source_thumbnails::SourceThumbnails::new(self));
            let result = gpu.prepare(self, source_target.unwrap(), &mut encoder, 4);
            self.thumbnails.sources = Some(gpu);
            Some(result)
        } else { None };
        #[cfg(target_arch = "wasm32")]
        let source = None;
        let result = if source_target.is_some_and(|source| self.selection_previews.definitions.contains_key(&source)) { Ok(true) }
        else if let Some(source) = source { source }
        else {
            let mut gpu = self.thumbnails.gpu.take().unwrap_or_else(|| PreviewPipeline::new(self));
            let result = gpu.prepare(self, target, &mut encoder, 4);
            self.thumbnails.gpu = Some(gpu);
            result
        };
        let ready = result?;
        self.uploads.finish(&encoder);
        encoder.submit(&self.queue);
        Ok(ready)
    }
    pub(super) fn start_thumbnail(
        &mut self,
        id: u64,
        target: ThumbnailTarget,
    ) -> Result<(), GpuRasterError> {
        self.validate_thumbnail(target)?;
        let source_target = self.thumbnail_source(target);
        let mut encoder = crate::submission::CommandEncoder::new(
            &self.device,
            &wgpu::CommandEncoderDescriptor {
                label: Some("asynchronous layer preview"),
            },
        );
        #[cfg(not(target_arch = "wasm32"))]
        let source = if source_target.is_some_and(|source| self.tiled_sources.contains_key(&source)) && source_target.and_then(|source| self.thumbnails.source_placements.get(&source)).is_some_and(|p| p.as_affine().is_some()) {
            let mut gpu = self
                .thumbnails
                .sources
                .take()
                .unwrap_or_else(|| crate::source_thumbnails::SourceThumbnails::new(self));
            let result = gpu.render(self, source_target.unwrap(), &mut encoder);
            self.thumbnails.sources = Some(gpu);
            Some(result?)
        } else {
            None
        };
        #[cfg(target_arch = "wasm32")]
        let source: Option<PageSurface> = None;
        let source = if source_target.is_some_and(|source| self.selection_previews.definitions.contains_key(&source)) {
            self.render_selection_thumbnail(source_target.unwrap(), &mut encoder)?
        } else if let Some(source) = source {
            source
        } else {
            let mut gpu = self
                .thumbnails
                .gpu
                .take()
                .unwrap_or_else(|| PreviewPipeline::new(self));
            let source = gpu.render(self, target, &mut encoder);
            self.thumbnails.gpu = Some(gpu);
            source?
        };
        let tx = self.thumbnails.tx.clone();
        self.thumbnails.pending += 1;
        self.submit_ui_readback(
            encoder,
            &source.texture_bind_group,
            [32, 32],
            id,
            move |image| {
                let _ = tx.send(image);
            },
        );
        Ok(())
    }

    /// Shared color conversion and nonblocking readback for small UI images.
    /// Neither thumbnails nor filter previews synchronously wait for the GPU.
    pub(super) fn submit_ui_readback(
        &mut self,
        mut encoder: crate::submission::CommandEncoder,
        source: &wgpu::BindGroup,
        [width, height]: [u32; 2],
        request_id: u64,
        reply: impl FnOnce(Result<ReadbackImage, GpuRasterError>) + Send + 'static,
    ) {
        let target = UiImageTarget::new(&self.device, [width, height]);
        let pipeline = if self.ui_preview_space == layer_core::color::RgbSpace::Srgb {
            &*self.pipelines.export
        } else {
            self.ui_preview_pipeline.get_or_insert_with(|| {
                let shader = self
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("managed UI image conversion"),
                        source: wgpu::ShaderSource::Wgsl(
                            format!(
                                "{}\n{}",
                                view_color::shader(
                                    self.device.working_space(),
                                    self.ui_preview_space
                                ),
                                include_str!("export.wgsl")
                            )
                            .into(),
                        ),
                    });
                let layout = self
                    .device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("managed UI image layout"),
                        bind_group_layouts: &[Some(&self.texture_layout)],
                        immediate_size: 0,
                    });
                fullscreen_pipeline(
                    &self.device,
                    &layout,
                    &shader,
                    "fragment_main",
                    None,
                    EXPORT_FORMAT,
                    "managed UI image",
                )
            })
        };
        target.encode(&mut encoder, pipeline, source);
        self.uploads.finish(&encoder);
        encoder.submit(&self.queue);
        target.map(request_id, reply);
    }
}

/// Reusable bounded output and staging buffer. The owner must consume the map
/// completion before encoding into it again; no fences or blocking waits.
pub(super) struct UiImageTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    buffer: wgpu::Buffer,
    stride: u32,
}
impl UiImageTarget {
    pub fn new(device: &wgpu::Device, size: [u32; 2]) -> Self {
        let (texture, view) = create_target(device, size, EXPORT_FORMAT, "profiled UI image");
        let stride = (size[0] * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("small UI image readback"),
            size: stride as u64 * size[1] as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Self {
            texture,
            view,
            buffer,
            stride,
        }
    }
    pub fn size(&self) -> [u32; 2] {
        [self.texture.width(), self.texture.height()]
    }
    pub fn encode(
        &self,
        encoder: &mut crate::submission::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        source: &wgpu::BindGroup,
    ) {
        {
            let mut pass = encoder.color_pass(
                "UI image color conversion",
                &self.view,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            );
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, source, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.stride),
                    rows_per_image: Some(self.texture.height()),
                },
            },
            self.texture.size(),
        );
    }
    pub fn map(
        &self,
        request_id: u64,
        reply: impl FnOnce(Result<ReadbackImage, GpuRasterError>) + Send + 'static,
    ) {
        let [width, height] = self.size();
        let stride = self.stride;
        crate::raster::map_then(
            &self.buffer,
            self.buffer.size(),
            move |data| {
                Ok(ReadbackImage {
                    request_id,
                    width,
                    height,
                    stride: width * 4,
                    bytes: crate::raster::unpadded_rows(data, stride, width * 4),
                })
            },
            reply,
        );
    }
}

struct PreviewPipeline {
    bounds: Arc<BoundsPipeline>,
    records: wgpu::BindGroupLayout,
    write_bounds: wgpu::BindGroupLayout,
    read_bounds: wgpu::BindGroupLayout,
    draw: wgpu::RenderPipeline,
    prepared: std::collections::VecDeque<PreparedPreview>,
    capture: artwork::Capture,
    generators: Option<scene::Scene>,
}
struct PreparedPreview {
    id: ThumbnailTarget,
    revision: (u64, u64),
    rendition: [f32; 8],
    sources: Vec<Option<[u32; 2]>>,
    mask: bool,
    placed: bool,
    generator: Option<OccurrenceHandle>,
    generated: Vec<(wgpu::Texture, wgpu::TextureView)>,
    records: wgpu::Buffer,
    stride: u32,
    read: wgpu::BindGroup,
    write: wgpu::BindGroup,
    result: PageSurface,
    measure: usize,
    draw: usize,
    valid: Arc<std::sync::atomic::AtomicBool>,
}
impl PreviewPipeline {
    fn new(r: &WgpuRasterizer) -> Self {
        let device = &r.device;
        let bounds_layout = |read_only| {
            crate::bindings::layout(device, "thumbnail bounds", &[crate::bindings::buffer(
                u32::from(read_only),
                if read_only { wgpu::ShaderStages::VERTEX } else { wgpu::ShaderStages::COMPUTE },
                wgpu::BufferBindingType::Storage { read_only },
                false,
                NonZeroU64::new(16),
            )])
        };
        let bounds = BoundsPipeline::new(&r.device);
        let write_bounds = bounds.write.clone();
        let read_bounds = bounds_layout(true);
        let records = bounds.records.clone();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("content-framed thumbnails"),
            source: wgpu::ShaderSource::Wgsl(format!("{}\n{}", crate::view_color::hdr_shader(device.working_space(), layer_core::color::RgbSpace::Srgb), BoundsPipeline::shader()).into()),
        });
        let layout = |bounds| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("thumbnail pipeline"),
                bind_group_layouts: &[Some(bounds), Some(&records)],
                immediate_size: 0,
            })
        };
        let draw = fullscreen_pipeline(
            device,
            &layout(&read_bounds),
            &shader,
            "fragment_main",
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            device.working_format(),
            "cropped thumbnail",
        );
        Self {
            records,
            write_bounds,
            read_bounds,
            bounds,
            draw,
            prepared: Default::default(),
            capture: Default::default(),
            generators: None,
        }
    }
    fn begin(
        &self,
        r: &WgpuRasterizer,
        target: ThumbnailTarget,
    ) -> PreparedPreview {
        let source = r.thumbnail_source(target);
        let id = source.unwrap_or_default();
        let mask = r.layer_masks.definitions.get(&id).cloned();
        let inverted=r.artwork_frame.as_ref().and_then(|f| f.scene.view().source_owner(id).and_then(|h| f.scene.view().mask(h))).is_some_and(|(use_,_)| use_.inverted);
        let gray = mask.as_ref().map_or(0., |m| {
            if inverted {
                1. - m.default_coverage
            } else {
                m.default_coverage
            }
        });
        let generator = match target {
            ThumbnailTarget::Occurrence(handle) => r.artwork_frame.as_ref().and_then(|frame|
                frame.scene.view().effect(handle).filter(|e| e.program.kind == layer_core::EffectKind::Generator).map(|_| handle)),
            _ => None,
        };
        let background = [gray, gray, gray, f32::from(mask.is_some())];
        let full = generator.is_some() || gray > 0.;
        let bounds = r
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("GPU thumbnail bounds"),
                contents: &if full {
                    [0, 0, r.document_extent[0], r.document_extent[1]]
                } else {
                    [u32::MAX, u32::MAX, 0, 0]
                }
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let binding = |layout, index| {
            r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("thumbnail bounds"),
                layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: index,
                    resource: bounds.as_entire_binding(),
                }],
            })
        };
        let write = binding(&self.write_bounds, 0);
        let read = binding(&self.read_bounds, 1);
        let mut coordinates = std::collections::BTreeSet::new();
        if mask.is_some() {
            coordinates.extend(
                r.layer_masks
                    .pages
                    .iter()
                    .filter(|((target, _), _)| *target == id)
                    .map(|((_, c), _)| *c),
            );
        } else if generator.is_some() {
            coordinates.insert([0; 2]);
        } else if let Some(layer) = r.paint_layers.iter().find(|l| l.id == id) {
            coordinates.extend(layer.pages.iter().map(|p| p.coordinate));
            coordinates.extend(r.native_color_coordinates(id));
        }
        let geometry = r.artwork_frame.as_ref().map(|frame| frame.scene.view().target_geometry(id));
        let mut placed = generator.is_none() && geometry.as_ref().is_some_and(|geometry| !geometry.is_identity());
        if source.is_none() && generator.is_none() {
            coordinates.extend(page_coordinates(PixelRect::full(r.document_extent))); placed = true;
        }
        if let Some(geometry) = geometry.filter(|_| placed) {
            let mut local = coordinates.iter().fold(PixelRect::EMPTY, |bounds,c| bounds.union(page_rect(*c)));
            if let Some(source) = r.tiled_sources.get(&id) { local = local.union(PixelRect::full(source.extent)); }
            coordinates = page_coordinates(pixel_rect(geometry.forward_bounds(local.to_rect()),r.document_extent)).collect();
        }
        let sources: Vec<_> = std::iter::once(None)
            .chain(coordinates.into_iter().map(Some))
            .collect();
        let alignment = r
            .device
            .limits()
            .min_uniform_buffer_offset_alignment as usize;
        let stride = 80usize.div_ceil(alignment) * alignment;
        let mut bytes = vec![0u8; stride * sources.len()];
        let footprint = if generator.is_some() { 1 << generator_grid(r.document_extent).level } else { 0 };
        for (i, coordinate) in sources.iter().enumerate() {
            let c = coordinate.unwrap_or([0; 2]);
            let mut record = [0u32; 20];
            record[..4].copy_from_slice(&[
                c[0] * PAGE_SIZE,
                c[1] * PAGE_SIZE,
                r.document_extent[0],
                r.document_extent[1],
            ]);
            record[4] = if i == 0 { 2 } else if generator.is_some() { 4 } else { u32::from(mask.is_some()) };
            record[6] = footprint;
            record[5] = u32::from(!placed && inverted);
            record[8..12].copy_from_slice(&background.map(f32::to_bits));
            if mask.is_none() { record[12..20].copy_from_slice(&r.ui_rendition_parameters().map(f32::to_bits)); }
            for (dst, value) in bytes[i * stride..i * stride + 80]
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(record)
            {
                dst.copy_from_slice(&value.to_le_bytes());
            }
        }
        let records = r
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("thumbnail page records"),
                contents: &bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let result = create_page_surface(&r.device, &r.texture_layout, &r.sampler, [32, 32],
            r.device.working_format(), "layer thumbnail");
        PreparedPreview { id:target, revision: (r.artwork_revision, r.selection_paint_revision),
            rendition: r.ui_rendition_parameters(), measure: if full { sources.len() } else { 1 },
            sources, mask: mask.is_some(), placed, generator, generated: Vec::new(), records, stride: stride as u32, read, write, result, draw: 0,
            valid: Arc::new(std::sync::atomic::AtomicBool::new(true)) }
    }
    fn prepare(&mut self, r: &mut WgpuRasterizer, target: ThumbnailTarget,
        encoder: &mut crate::submission::CommandEncoder, mut limit: usize,
    ) -> Result<bool, GpuRasterError> {
        let revision = (r.artwork_revision, r.selection_paint_revision);
        let rendition = r.ui_rendition_parameters();
        self.prepared.retain(|p| p.revision == revision && p.rendition == rendition
            && p.valid.load(std::sync::atomic::Ordering::Acquire));
        let id = r.thumbnail_source(target).unwrap_or_default();
        let cached = self.prepared.iter().position(|p| p.id == target);
        let mut prepared = if let Some(i) = cached { self.prepared.remove(i).unwrap() } else { self.begin(r, target) };
        if let Some(generator) = prepared.generator.filter(|_| prepared.generated.is_empty()) {
            if self.generators.is_none() { self.generators = Some(scene::Scene::new(r)); }
            let Some(generated) = self.generators.as_mut().unwrap().generator_preview(r, generator, generator_grid(r.document_extent), encoder)? else {
                self.prepared.push_back(prepared);
                return Ok(false);
            };
            prepared.generated = generated;
        }
        let write = crate::submission::CacheWrite::new();
        let capture = &mut self.capture;
        let mut inputs = |r: &mut WgpuRasterizer,
                      encoder: &mut crate::submission::CommandEncoder,
                      chunk: &[Option<[u32; 2]>]|
         -> Result<Vec<wgpu::BindGroup>, GpuRasterError> {
            chunk
                .iter()
                .map(|coordinate| {
                    let view = match coordinate {
                        None => r.empty_view.clone(),
                        Some(_) if prepared.generator.is_some() => {
                            let count = r.artwork_frame.as_ref().unwrap().scene.view().effect(prepared.generator.unwrap()).unwrap().program.passes.len().max(1);
                            prepared.generated[(count-1)%prepared.generated.len()].1.clone()
                        },
                        Some(c) if prepared.placed => capture.thumbnail_tile(r,target,*c,encoder)?.view,
                        Some(c) if prepared.mask => r.layer_masks.pages[&(id, *c)].view.clone(),
                        Some(c) => {
                            r.raw_layer_tile(id, *c, encoder)?
                                .ok_or(GpuRasterError::MissingPaintLayer(id))?
                                .view
                        }
                    };
                    Ok(crate::bindings::group(&r.device, "thumbnail page", &self.records, [
                        wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &prepared.records, offset: 0, size: NonZeroU64::new(80), }),
                        wgpu::BindingResource::TextureView(&view),
                        wgpu::BindingResource::Sampler(&r.sampler),
                        self.bounds.empty_selection.as_entire_binding(),
                    ]))
                })
                .collect()
        };
        while limit > 0 && prepared.measure < prepared.sources.len() {
            let end = (prepared.measure + limit.min(if prepared.placed { 1 } else { SOURCE_SLOTS })).min(prepared.sources.len());
            let chunk = &prepared.sources[prepared.measure..end];
            let bindings = inputs(r, encoder, chunk)?;
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("thumbnail bounds scan"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.bounds.measure);
            pass.set_bind_group(0, &prepared.write, &[]);
            for (i, source) in bindings.iter().enumerate() {
                pass.set_bind_group(
                    1,
                    source,
                    &[(prepared.measure + i) as u32 * prepared.stride],
                );
                pass.dispatch_workgroups(8, 8, 1);
            }
            limit -= end - prepared.measure;
            prepared.measure = end;
        }
        while limit > 0 && prepared.draw < prepared.sources.len() {
            let end = (prepared.draw + limit.min(if prepared.placed { 1 } else { SOURCE_SLOTS })).min(prepared.sources.len());
            let chunk = &prepared.sources[prepared.draw..end];
            let bindings = inputs(r, encoder, chunk)?;
            if r.device.portable_blend() {
                if prepared.draw == 0 { r.encode_clear(encoder,&prepared.result.view,"clear portable thumbnail"); }
                let temporary=r.portable_blend.source(&r.device,&prepared.result.view,r.device.working_format());
                for (i,source) in bindings.iter().enumerate() {
                    let mut pass=encoder.color_pass("portable thumbnail",&temporary,wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
                    pass.set_pipeline(&self.draw);pass.set_bind_group(0,&prepared.read,&[]);
                    pass.set_bind_group(1,source,&[(prepared.draw+i) as u32*prepared.stride]);pass.draw(0..3,0..1);drop(pass);
                    r.portable_blend.apply(&r.device,encoder,&temporary,&prepared.result.view,PixelRect::full([32,32]),0);
                }
            } else {
                let mut pass = encoder.color_pass(
                    "thumbnail framing and checkerboard",
                    &prepared.result.view,
                    if prepared.draw == 0 { wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT) } else { wgpu::LoadOp::Load },
                );
                pass.set_pipeline(&self.draw);
                pass.set_bind_group(0, &prepared.read, &[]);
                for (i, source) in bindings.iter().enumerate() {
                    pass.set_bind_group(1, source, &[(prepared.draw + i) as u32 * prepared.stride]);
                    pass.draw(0..3, 0..1);
                }
            }
            limit -= end - prepared.draw;
            prepared.draw = end;
        }
        write.track(encoder);
        prepared.valid = write.validity();
        let ready = prepared.draw == prepared.sources.len();
        self.prepared.push_back(prepared);
        while self.prepared.len() > 8 { self.prepared.pop_front(); }
        Ok(ready)
    }
    fn render(&mut self, r: &mut WgpuRasterizer, target: ThumbnailTarget,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<PageSurface, GpuRasterError> {
        if !self.prepare(r, target, encoder, usize::MAX)? {
            return Err(GpuRasterError::Effect("Thumbnail shader preparation pending".into()));
        }
        Ok(self.prepared.pop_back().unwrap().result)
    }
}

fn generator_grid(extent: [u32; 2]) -> display_mips::Plan {
    display_mips::Plan::at(extent, extent.into_iter().max().unwrap().div_ceil(32).next_power_of_two().ilog2())
}


pub(super) struct BoundsPipeline {
    pub records: wgpu::BindGroupLayout,
    pub write: wgpu::BindGroupLayout,
    pub measure: wgpu::ComputePipeline,
    empty_selection: wgpu::Buffer,
    sampler: wgpu::Sampler,
}
impl BoundsPipeline {
    fn shader() -> String {
        format!("{}\n{}\n{}", include_str!("selection_clip.wgsl").replace("@binding(1)", "@binding(3)"),
            include_str!("area_sample.wgsl"), include_str!("thumbnails.wgsl"))
    }
    pub fn new(device: &PipelineDevice) -> Arc<Self> {
        device.bounds_pipeline.get_or_init(|| Arc::new(Self::build(device))).clone()
    }
    fn build(device: &PipelineDevice) -> Self {
        let write = crate::bindings::layout(device, "content bounds", &[crate::bindings::buffer(
            0, wgpu::ShaderStages::COMPUTE,
            wgpu::BufferBindingType::Storage { read_only: false }, false, NonZeroU64::new(16),
        )]);
        let records = crate::bindings::layout(device, "content bounds page", &[
            crate::bindings::buffer(0, wgpu::ShaderStages::VERTEX_FRAGMENT | wgpu::ShaderStages::COMPUTE,
                wgpu::BufferBindingType::Uniform, true, NonZeroU64::new(80)),
            crate::bindings::texture(1, wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE, true),
            crate::bindings::sampler(2, wgpu::ShaderStages::FRAGMENT, wgpu::SamplerBindingType::Filtering),
            crate::bindings::buffer(3, wgpu::ShaderStages::COMPUTE, wgpu::BufferBindingType::Storage { read_only: true }, false, NonZeroU64::new(48)),
        ]);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("content bounds reduction"),
            source: wgpu::ShaderSource::Wgsl(format!("{}\n{}",
                crate::view_color::hdr_shader(device.working_space(), layer_core::color::RgbSpace::Srgb),
                BoundsPipeline::shader()).into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("content bounds reduction"), bind_group_layouts: &[Some(&write), Some(&records)], immediate_size: 0,
        });
        let measure = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("content bounds reduction"), layout: Some(&layout), module: &shader,
            entry_point: Some("measure"), compilation_options: Default::default(), cache: None,
        });
        let empty_selection = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bounds without selection"), contents: &[0; 48], usage: wgpu::BufferUsages::STORAGE,
        });
        Self { records, write, measure, empty_selection, sampler: device.create_sampler(&Default::default()) }
    }
    pub fn buffer(device: &PipelineDevice) -> wgpu::Buffer {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("content bounds"),
            contents: &[u32::MAX, u32::MAX, 0, 0].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>(),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        })
    }
    #[expect(clippy::too_many_arguments, reason = "Thumbnail reduction keeps texture region and independent mask bindings explicit")]
    pub fn reduce(&self, device: &PipelineDevice, encoder: &mut crate::submission::CommandEncoder,
        output: &wgpu::Buffer, texture: &wgpu::Texture, origin: [u32; 2], extent: [u32; 2],
        mask: Option<(bool, Option<f32>)>, selection: Option<&wgpu::Buffer>,
    ) {
        let mut values = [0u32; 20];
        values[..4].copy_from_slice(&[origin[0], origin[1], extent[0], extent[1]]);
        if let Some((inverted, constant)) = mask {
            values[4] = if constant.is_some() { 3 } else { 1 };
            values[5] = u32::from(inverted);
            values[8] = constant.unwrap_or(0.).to_bits();
        }
        let record = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("content bounds page"),
            contents: &values.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>(),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let view = texture.create_view(&Default::default());
        let record = crate::bindings::group(device, "content bounds page", &self.records, [
            record.as_entire_binding(), wgpu::BindingResource::TextureView(&view), wgpu::BindingResource::Sampler(&self.sampler),
            selection.unwrap_or(&self.empty_selection).as_entire_binding(),
        ]);
        let output = crate::bindings::group(device, "content bounds result", &self.write, [output.as_entire_binding()]);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
        pass.set_pipeline(&self.measure);
        pass.set_bind_group(0, &output, &[]);
        pass.set_bind_group(1, &record, &[0]);
        pass.dispatch_workgroups(extent[0].saturating_sub(origin[0]).min(256).div_ceil(32), extent[1].saturating_sub(origin[1]).min(256).div_ceil(32), 1);
    }
}
