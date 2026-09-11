//! Asynchronous 32px previews. The GPU worker never waits for thumbnail maps.
use super::*;
use wgpu::util::DeviceExt;
pub(super) struct Thumbnails {
    tx: mpsc::Sender<Result<ReadbackImage, GpuRasterError>>,
    rx: mpsc::Receiver<Result<ReadbackImage, GpuRasterError>>,
    pending: usize,
    gpu: Option<PreviewPipeline>,
    pub paper: Option<(LayerId, [f32; 4])>,
}
impl Thumbnails {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            pending: 0,
            gpu: None,
            paper: None,
        }
    }
    pub fn take(&mut self) -> Option<Result<ReadbackImage, GpuRasterError>> {
        let image = self.rx.try_recv().ok()?;
        self.pending = self.pending.saturating_sub(1);
        Some(image)
    }
}
impl WgpuRasterizer {
    pub fn thumbnails_pending(&self) -> bool {
        self.thumbnails.pending > 0
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
        let gpu = self
            .thumbnails
            .gpu
            .take()
            .unwrap_or_else(|| PreviewPipeline::new(self));
        let source = gpu.render(self, target, &mut encoder);
        self.thumbnails.gpu = Some(gpu);
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
        target.encode(&mut encoder, &self.pipelines.export, source);
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
        let (texture, view) = create_target(device, size, EXPORT_FORMAT, "UI image sRGB");
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
                        min_binding_size: NonZeroU64::new(48),
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
            source: wgpu::ShaderSource::Wgsl(include_str!("thumbnails.wgsl").into()),
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
            COLOR_FORMAT,
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
        r: &WgpuRasterizer,
        id: LayerId,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> PageSurface {
        let mask = r.layer_masks.definitions.get(&id);
        let gray = mask.map_or(0., |m| {
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
        let mut sources = vec![([0, 0], r.empty_view.clone())];
        if mask.is_some() {
            sources.extend(
                r.layer_masks
                    .pages
                    .iter()
                    .filter(|((target, _), _)| *target == id)
                    .map(|((_, c), p)| (*c, p.view.clone())),
            );
        } else if let Some(layer) = r.paint_layers.iter().find(|l| l.id == id) {
            sources.extend(
                layer
                    .pages
                    .iter()
                    .map(|p| (p.coordinate, p.active().view.clone())),
            );
        }
        let stride = r
            .device
            .limits()
            .min_uniform_buffer_offset_alignment
            .max(48) as usize;
        let mut bytes = vec![0u8; stride * sources.len()];
        for (i, (c, _)) in sources.iter().enumerate() {
            let mut record = [0u32; 12];
            record[..4].copy_from_slice(&[
                c[0] * PAGE_SIZE,
                c[1] * PAGE_SIZE,
                r.document_extent[0],
                r.document_extent[1],
            ]);
            record[4] = if i == 0 { 2 } else { u32::from(mask.is_some()) };
            record[5] = u32::from(mask.is_some_and(|m| m.inverted));
            record[8..12].copy_from_slice(&background.map(f32::to_bits));
            for (dst, value) in bytes[i * stride..i * stride + 48]
                .chunks_exact_mut(4)
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
        let sources: Vec<_> = sources
            .iter()
            .map(|(_, view)| {
                r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("thumbnail page"),
                    layout: &self.records,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &records,
                                offset: 0,
                                size: NonZeroU64::new(48),
                            }),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&r.sampler),
                        },
                    ],
                })
            })
            .collect();
        if !full && sources.len() > 1 {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("thumbnail bounds scan"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.measure);
            pass.set_bind_group(0, &write, &[]);
            for (i, source) in sources.iter().enumerate().skip(1) {
                pass.set_bind_group(1, source, &[(i * stride) as u32]);
                pass.dispatch_workgroups(8, 8, 1);
            }
        }
        let result = create_page_surface(
            &r.device,
            &r.texture_layout,
            &r.sampler,
            [32, 32],
            COLOR_FORMAT,
            "layer thumbnail",
        );
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("thumbnail framing and checkerboard"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &result.view,
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
            pass.set_pipeline(&self.draw);
            pass.set_bind_group(0, &read, &[]);
            for (i, source) in sources.iter().enumerate() {
                pass.set_bind_group(1, source, &[(i * stride) as u32]);
                pass.draw(0..3, 0..1);
            }
        }
        result
    }
}
