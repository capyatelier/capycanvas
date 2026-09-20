//! Asynchronous 32px previews. The GPU worker never waits for thumbnail maps.
use super::*;
use wgpu::util::DeviceExt;
pub(super) struct Thumbnails {
    tx: mpsc::Sender<Result<ReadbackImage, GpuRasterError>>,
    rx: mpsc::Receiver<Result<ReadbackImage, GpuRasterError>>,
    pending: usize,
    gpu: Option<PreviewPipeline>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) sources: Option<crate::source_thumbnails::SourceThumbnails>,
    pub paper: Option<(LayerId, [f32; 4])>,
    pub source_placements: std::collections::BTreeMap<LayerId, layer_core::Affine>,
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
            paper: None,
            source_placements: Default::default(),
        }
    }
    pub fn take(&mut self) -> Option<Result<ReadbackImage, GpuRasterError>> {
        let image = self.rx.try_recv().ok()?;
        self.pending = self.pending.saturating_sub(1);
        Some(image)
    }
    pub fn storage_bytes(&self) -> u64 {
        #[cfg(not(target_arch = "wasm32"))]
        return self.sources.as_ref().map_or(0, |s| s.storage_bytes());
        #[cfg(target_arch = "wasm32")]
        0
    }
}
impl WgpuRasterizer {
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
        if self.thumbnails.pending != 0
            || self.canvas_preview_pending()
            || self.filter_previews.is_some()
        {
            return Err(GpuRasterError::Color(
                "Configure preview color before starting UI image jobs".into(),
            ));
        }
        self.ui_preview_space = space;
        self.ui_preview_pipeline = None;
        self.thumbnails = Thumbnails::new();
        self.canvas_preview = crate::canvas_preview::CanvasOverview::new();
        Ok(())
    }

    pub fn thumbnails_pending(&self) -> bool {
        self.thumbnails.pending > 0
    }
    /// Give a cold photo thumbnail bounded background time before requesting
    /// its readback. Hosts can service camera/paint commands between batches.
    /// Existing completed originals are reused; document pixels are unchanged.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn prepare_thumbnail_batch(&mut self, target: LayerId) -> Result<bool, GpuRasterError> {
        if !self.tiled_sources.contains_key(&target) {
            return Ok(true);
        }
        let mut gpu = self
            .thumbnails
            .sources
            .take()
            .unwrap_or_else(|| crate::source_thumbnails::SourceThumbnails::new(self));
        let mut encoder = crate::submission::CommandEncoder::new(
            &self.device,
            &wgpu::CommandEncoderDescriptor {
                label: Some("background photo thumbnail batch"),
            },
        );
        let result = gpu.prepare(self, target, &mut encoder, 4);
        self.thumbnails.sources = Some(gpu);
        let ready = result?;
        self.uploads.finish(&encoder);
        encoder.submit(&self.queue);
        Ok(ready)
    }
    pub(super) fn start_thumbnail(
        &mut self,
        id: u64,
        target: LayerId,
    ) -> Result<(), GpuRasterError> {
        let mut encoder = crate::submission::CommandEncoder::new(
            &self.device,
            &wgpu::CommandEncoderDescriptor {
                label: Some("asynchronous layer preview"),
            },
        );
        #[cfg(not(target_arch = "wasm32"))]
        let source = if self.tiled_sources.contains_key(&target) {
            let mut gpu = self
                .thumbnails
                .sources
                .take()
                .unwrap_or_else(|| crate::source_thumbnails::SourceThumbnails::new(self));
            let result = gpu.render(self, target, &mut encoder);
            self.thumbnails.sources = Some(gpu);
            Some(result?)
        } else {
            None
        };
        #[cfg(target_arch = "wasm32")]
        let source: Option<PageSurface> = None;
        let source = if let Some(source) = source {
            source
        } else {
            let gpu = self
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
    pub fn storage_bytes(&self) -> u64 {
        u64::from(self.texture.height()) * u64::from(self.texture.width() * 4 + self.stride)
    }
    pub fn encode(
        &self,
        encoder: &mut crate::submission::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        source: &wgpu::BindGroup,
    ) {
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("UI image color conversion"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
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
        let ready = self.buffer.clone();
        self.buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let image = result
                    .map_err(|e| GpuRasterError::MapFailed(e.to_string()))
                    .and_then(|_| {
                        let data = ready
                            .slice(..)
                            .get_mapped_range()
                            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?;
                        let mut bytes = Vec::with_capacity((width * height * 4) as usize);
                        for row in data.chunks(stride as usize) {
                            bytes.extend_from_slice(&row[..width as usize * 4]);
                        }
                        drop(data);
                        Ok(ReadbackImage {
                            request_id,
                            width,
                            height,
                            stride: width * 4,
                            bytes,
                        })
                    });
                // A persistent staging buffer must also be reusable after a
                // failed mapped-range access, not only after successful copies.
                ready.unmap();
                reply(image);
            });
    }
}

struct PreviewPipeline {
    records: wgpu::BindGroupLayout,
    write_bounds: wgpu::BindGroupLayout,
    read_bounds: wgpu::BindGroupLayout,
    measure: wgpu::ComputePipeline,
    draw: wgpu::RenderPipeline,
}
impl PreviewPipeline {
    fn new(r: &WgpuRasterizer) -> Self {
        let device = &r.device;
        let bounds_layout = |read_only| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("thumbnail bounds"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: u32::from(read_only),
                    visibility: if read_only {
                        wgpu::ShaderStages::VERTEX
                    } else {
                        wgpu::ShaderStages::COMPUTE
                    },
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only },
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(16),
                    },
                    count: None,
                }],
            })
        };
        let write_bounds = bounds_layout(false);
        let read_bounds = bounds_layout(true);
        let records = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("thumbnail page"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT | wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: NonZeroU64::new(80),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("content-framed thumbnails"),
            source: wgpu::ShaderSource::Wgsl(format!("{}\n{}", crate::view_color::hdr_shader(device.working_space(), layer_core::color::RgbSpace::Srgb), include_str!("thumbnails.wgsl")).into()),
        });
        let layout = |bounds| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("thumbnail pipeline"),
                bind_group_layouts: &[Some(bounds), Some(&records)],
                immediate_size: 0,
            })
        };
        let measure = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("thumbnail alpha bounds"),
            layout: Some(&layout(&write_bounds)),
            module: &shader,
            entry_point: Some("measure"),
            compilation_options: Default::default(),
            cache: None,
        });
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
            measure,
            draw,
        }
    }
    fn render(
        &self,
        r: &mut WgpuRasterizer,
        id: LayerId,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<PageSurface, GpuRasterError> {
        let mask = r.layer_masks.definitions.get(&id).cloned();
        let gray = mask.as_ref().map_or(0., |m| {
            if m.inverted {
                1. - m.default_coverage
            } else {
                m.default_coverage
            }
        });
        let paper = r
            .thumbnails
            .paper
            .filter(|(target, _)| *target == id)
            .map(|(_, color)| color);
        let background = paper.unwrap_or([gray, gray, gray, f32::from(mask.is_some())]);
        let full = paper.is_some() || gray > 0.;
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
        } else if let Some(layer) = r.paint_layers.iter().find(|l| l.id == id) {
            coordinates.extend(layer.pages.iter().map(|p| p.coordinate));
            coordinates.extend(r.native_color_coordinates(id));
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
        for (i, coordinate) in sources.iter().enumerate() {
            let c = coordinate.unwrap_or([0; 2]);
            let mut record = [0u32; 20];
            record[..4].copy_from_slice(&[
                c[0] * PAGE_SIZE,
                c[1] * PAGE_SIZE,
                r.document_extent[0],
                r.document_extent[1],
            ]);
            record[4] = if i == 0 { 2 } else { u32::from(mask.is_some()) };
            record[5] = u32::from(mask.as_ref().is_some_and(|m| m.inverted));
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
        // Decode and consume each bounded set before another cache reservation.
        // Records remain immutable for both native staging and browser writes.
        let inputs = |r: &mut WgpuRasterizer,
                      encoder: &mut crate::submission::CommandEncoder,
                      chunk: &[Option<[u32; 2]>]|
         -> Result<Vec<wgpu::BindGroup>, GpuRasterError> {
            chunk
                .iter()
                .map(|coordinate| {
                    let view = match coordinate {
                        None => r.empty_view.clone(),
                        Some(c) if mask.is_some() => r.layer_masks.pages[&(id, *c)].view.clone(),
                        Some(c) => {
                            r.raw_layer_tile(id, *c, encoder)?
                                .ok_or(GpuRasterError::MissingPaintLayer(id))?
                                .view
                        }
                    };
                    Ok(r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("thumbnail page"),
                        layout: &self.records,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                    buffer: &records,
                                    offset: 0,
                                    size: NonZeroU64::new(80),
                                }),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::TextureView(&view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: wgpu::BindingResource::Sampler(&r.sampler),
                            },
                        ],
                    }))
                })
                .collect()
        };
        if !full && sources.len() > 1 {
            for (batch, chunk) in sources[1..].chunks(SOURCE_SLOTS).enumerate() {
                let bindings = inputs(r, encoder, chunk)?;
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("thumbnail bounds scan"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.measure);
                pass.set_bind_group(0, &write, &[]);
                for (i, source) in bindings.iter().enumerate() {
                    pass.set_bind_group(
                        1,
                        source,
                        &[((1 + batch * SOURCE_SLOTS + i) * stride) as u32],
                    );
                    pass.dispatch_workgroups(8, 8, 1);
                }
            }
        }
        let result = create_page_surface(
            &r.device,
            &r.texture_layout,
            &r.sampler,
            [32, 32],
            r.device.working_format(),
            "layer thumbnail",
        );
        for (batch, chunk) in sources.chunks(SOURCE_SLOTS).enumerate() {
            let bindings = inputs(r, encoder, chunk)?;
            if r.device.portable_blend() {
                if batch == 0 { r.encode_clear(encoder,&result.view,"clear portable thumbnail"); }
                let temporary=r.portable_blend.source(&r.device,&result.view,r.device.working_format());
                for (i,source) in bindings.iter().enumerate() {
                    let mut pass=encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label:Some("portable thumbnail"), color_attachments:&[Some(wgpu::RenderPassColorAttachment {view:&temporary,resolve_target:None,depth_slice:None,ops:wgpu::Operations {load:wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),store:wgpu::StoreOp::Store}})],..Default::default()
                    });
                    pass.set_pipeline(&self.draw);pass.set_bind_group(0,&read,&[]);
                    pass.set_bind_group(1,source,&[((batch*SOURCE_SLOTS+i)*stride) as u32]);pass.draw(0..3,0..1);drop(pass);
                    r.portable_blend.apply(&r.device,encoder,&temporary,&result.view,PixelRect::full([32,32]),0);
                }
                continue;
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("thumbnail framing and checkerboard"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &result.view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: if batch == 0 {
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.draw);
            pass.set_bind_group(0, &read, &[]);
            for (i, source) in bindings.iter().enumerate() {
                pass.set_bind_group(1, source, &[((batch * SOURCE_SLOTS + i) * stride) as u32]);
                pass.draw(0..3, 0..1);
            }
        }
        Ok(result)
    }
}
