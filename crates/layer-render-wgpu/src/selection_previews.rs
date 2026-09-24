//! Display-only saved-mask previews. Their GPU overlay never becomes a working
//! selection and never enters artwork compositing, sampling, or export.
use super::*;
use layer_core::Selection;
use wgpu::util::DeviceExt;

#[derive(Default)]
pub(super) struct SelectionPreviews {
    pub definitions: std::collections::BTreeMap<LayerId, Selection>,
    key: Option<(
        Vec<(Selection, layer_core::SelectionMaskProperties)>,
        [u32; 2],
        bool,
    )>,
    pub buffer: Option<wgpu::Buffer>,
    pub texture: Option<wgpu::TextureView>,
    pipeline: Option<PreviewPipeline>,
}
struct PreviewPipeline {
    layout: wgpu::BindGroupLayout,
    merge: Deferred<wgpu::ComputePipeline>,
    thumbnail: Deferred<wgpu::RenderPipeline>,
}
impl PreviewPipeline {
    fn new(r: &WgpuRasterizer) -> Self {
        let layout = r
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("saved mask display"),
                entries: &[0, 1, 2].map(|binding| wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: if binding == 2 {
                        wgpu::ShaderStages::COMPUTE
                    } else {
                        wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::FRAGMENT
                    },
                    ty: wgpu::BindingType::Buffer {
                        ty: match binding {
                            0 => wgpu::BufferBindingType::Uniform,
                            1 => wgpu::BufferBindingType::Storage { read_only: true },
                            _ => wgpu::BufferBindingType::Storage { read_only: false },
                        },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }),
            });
        let pl = r
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("saved mask display"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let clip = include_str!("selection_clip.wgsl").replace("@group(1)", "@group(0)");
        let shader = |source: &str| {
            r.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("saved mask display"),
                source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[&clip, source])),
            })
        };
        let compute = shader(include_str!("selection_previews.wgsl"));
        let draw = shader(include_str!("selection_thumbnail.wgsl"));
        let merge = {
            let (device, layout) = (r.device.clone(), pl.clone());
            Deferred::pipeline(move |mode| {
                mode.compute(
                    &device,
                    &wgpu::ComputePipelineDescriptor {
                        label: Some("saved mask overlays"),
                        layout: Some(&layout),
                        module: &compute,
                        entry_point: Some("merge"),
                        compilation_options: Default::default(),
                        cache: None,
                    },
                )
            })
        };
        let thumbnail = {
            let (device, layout) = (r.device.clone(), pl);
            Deferred::pipeline(move |mode| {
                mode.render(
                    &device,
                    &wgpu::RenderPipelineDescriptor {
                        label: Some("saved selection thumbnail"),
                        layout: Some(&layout),
                        vertex: wgpu::VertexState {
                            module: &draw,
                            entry_point: Some("vs"),
                            compilation_options: Default::default(),
                            buffers: &[],
                        },
                        fragment: Some(wgpu::FragmentState {
                            module: &draw,
                            entry_point: Some("fs"),
                            compilation_options: Default::default(),
                            targets: &[Some(wgpu::ColorTargetState {
                                format: device.working_format(),
                                blend: None,
                                write_mask: wgpu::ColorWrites::ALL,
                            })],
                        }),
                        primitive: Default::default(),
                        depth_stencil: None,
                        multisample: Default::default(),
                        multiview_mask: None,
                        cache: None,
                    },
                )
            })
        };
        Self {
            layout,
            merge,
            thumbnail,
        }
    }
    fn bind(
        &self,
        r: &WgpuRasterizer,
        extent: [u32; 2],
        protected: bool,
        color: [f32; 4],
        input: &wgpu::Buffer,
        output: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        let params = r
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("saved mask display parameters"),
                contents: &[extent[0], extent[1], u32::from(protected), 0]
                    .into_iter()
                    .chain(color.map(f32::to_bits))
                    .flat_map(u32::to_ne_bytes)
                    .collect::<Vec<_>>(),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        r.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("saved mask display"),
            layout: &self.layout,
            entries: &[(0, &params), (1, input), (2, output)].map(|(binding, b)| {
                wgpu::BindGroupEntry {
                    binding,
                    resource: b.as_entire_binding(),
                }
            }),
        })
    }
    fn ready(&self, r: &WgpuRasterizer, thumbnail: bool) -> Result<bool, GpuRasterError> {
        let Some(startup) = &r.startup else {
            return Ok(true);
        };
        startup.compiler.check()?;
        let mut ready = true;
        for p in [
            &r.selection_clip.crossings,
            &r.selection_clip.fill,
            &r.selection_clip.resample,
        ] {
            startup.compiler.pipeline(p, startup::BRUSH);
            ready &= p.ready();
        }
        if thumbnail {
            startup.compiler.pipeline(&self.thumbnail, startup::BRUSH);
            ready &= self.thumbnail.ready();
        } else {
            startup.compiler.pipeline(&self.merge, startup::BRUSH);
            ready &= self.merge.ready();
        }
        Ok(ready)
    }
}
impl WgpuRasterizer {
    pub(super) fn prepare_selection_previews(
        &mut self,
        layers: &[Layer],
    ) -> Result<(), GpuRasterError> {
        let mut previews = mem::take(&mut self.selection_previews);
        let result = (|| {
            previews.definitions.retain(|id, _| {
                id.0 == 0
                    || layers
                        .iter()
                        .any(|l| l.id == *id && l.kind == LayerKind::Selection)
            });
            for layer in layers.iter().filter(|l| l.kind == LayerKind::Selection) {
                let coverage = layer
                    .selection
                    .as_ref()
                    .ok_or(GpuRasterError::InvalidImage)?
                    .transformed(layer_core::target_transform(layers, layer.id))
                    .map_err(|_| GpuRasterError::InvalidImage)?;
                previews.definitions.insert(layer.id, coverage);
            }
            let Some(options) = self.selection_overlay else {
                previews.key = None;
                previews.buffer = None;
                previews.texture = None;
                return Ok(());
            };
            let masks: Vec<_> = layers
                .iter()
                .rev()
                .filter(|l| {
                    l.kind == LayerKind::Selection && Some(l.id) != options.editing && {
                        let mut current = Some(l.id);
                        let mut visible = true;
                        while let Some(id) = current {
                            let Some(l) = layers.iter().find(|l| l.id == id) else {
                                break;
                            };
                            visible &= l.visible;
                            current = l.properties.parent;
                        }
                        visible
                    }
                })
                .filter_map(|l| {
                    previews.definitions.get(&l.id).cloned().map(|mask| {
                        (
                            mask,
                            l.properties.selection_mask.clone().unwrap_or_default(),
                        )
                    })
                })
                .collect();
            if masks.is_empty() {
                previews.key = None;
                previews.buffer = None;
                previews.texture = None;
                return Ok(());
            }
            let key = (masks, self.document_extent, options.saved_protected);
            if previews.key.as_ref() == Some(&key) {
                return Ok(());
            }
            let gpu = previews
                .pipeline
                .get_or_insert_with(|| PreviewPipeline::new(self));
            if !gpu.ready(self, false)? {
                return Ok(());
            }
            let [w, h] = self.document_extent;
            let row = (w * 4).div_ceil(256) * 256;
            let size = 32 + u64::from(row) * u64::from(h);
            if size > self.device.limits().max_storage_buffer_binding_size
                || h > self.device.limits().max_compute_workgroups_per_dimension
            {
                return Err(GpuRasterError::SizeOverflow);
            }
            let output = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("visible saved-mask overlay"),
                size,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let header = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("saved overlay header"),
                    contents: &[0u32, 0, w, h, 0, 2, 0, 0]
                        .into_iter()
                        .flat_map(u32::to_ne_bytes)
                        .collect::<Vec<_>>(),
                    usage: wgpu::BufferUsages::COPY_SRC,
                });
            let mut encoder = crate::submission::CommandEncoder::new(
                &self.device,
                &wgpu::CommandEncoderDescriptor {
                    label: Some("saved selection previews"),
                },
            );
            encoder.copy_buffer_to_buffer(&header, 0, &output, 0, 32);
            for (mask, properties) in &key.0 {
                self.selection_clip.prepare(
                    &self.device,
                    &mut encoder,
                    self.document_extent,
                    &Arc::new(mask.clone()),
                )?;
                let bind = gpu.bind(
                    self,
                    self.document_extent,
                    options.saved_protected,
                    {
                        let mut color = properties
                            .color
                            .encoded_in(layer_core::color::RgbSpace::Srgb)
                            .map_err(|_| GpuRasterError::InvalidImage)?;
                        color[3] *= properties.opacity;
                        color
                    },
                    self.selection_clip.buffer.as_ref().unwrap(),
                    &output,
                );
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("saved mask overlays"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&gpu.merge);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(w.div_ceil(64), h, 1);
            }
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("saved overlay texture"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R32Uint,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            encoder.copy_buffer_to_texture(
                wgpu::TexelCopyBufferInfo {
                    buffer: &output,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 32,
                        bytes_per_row: Some(row),
                        rows_per_image: Some(h),
                    },
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                texture.size(),
            );
            previews.texture = Some(texture.create_view(&Default::default()));
            self.uploads.finish(&encoder);
            encoder.submit(&self.queue);
            previews.buffer = Some(output);
            previews.key = Some(key);
            Ok(())
        })();
        self.selection_previews = previews;
        result
    }
    /// Poll cold mask preview pipelines without blocking the host UI thread.
    pub fn prepare_selection_thumbnail(&mut self, id: LayerId) -> Result<bool, GpuRasterError> {
        if !self.selection_previews.definitions.contains_key(&id) {
            return Ok(true);
        }
        let gpu = self
            .selection_previews
            .pipeline
            .take()
            .unwrap_or_else(|| PreviewPipeline::new(self));
        let ready = gpu.ready(self, true);
        self.selection_previews.pipeline = Some(gpu);
        ready
    }
    pub(super) fn render_selection_thumbnail(
        &mut self,
        id: LayerId,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<PageSurface, GpuRasterError> {
        let mask = self.selection_previews.definitions[&id].clone();
        self.selection_clip.prepare(
            &self.device,
            encoder,
            self.document_extent,
            &Arc::new(mask),
        )?;
        let gpu = self
            .selection_previews
            .pipeline
            .take()
            .unwrap_or_else(|| PreviewPipeline::new(self));
        let scratch = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("unused preview output"),
            size: 36,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let bind = gpu.bind(
            self,
            self.document_extent,
            false,
            [0.; 4],
            self.selection_clip.buffer.as_ref().unwrap(),
            &scratch,
        );
        let result = create_page_surface(
            &self.device,
            &self.texture_layout,
            &self.sampler,
            [32, 32],
            self.device.working_format(),
            "selection layer thumbnail",
        );
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("selection layer thumbnail"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &result.view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&gpu.thumbnail);
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        self.selection_previews.pipeline = Some(gpu);
        Ok(result)
    }
}
