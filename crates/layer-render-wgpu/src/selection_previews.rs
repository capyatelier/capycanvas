//! Display-only saved-mask previews. Their GPU overlay never becomes a working
//! selection and never enters artwork compositing, sampling, or export.
use super::*;
use layer_core::Selection;
use wgpu::util::DeviceExt;

type PreviewKey = (Vec<(Selection, layer_core::SelectionMaskProperties)>, [u32; 2], bool);

#[derive(Default)]
pub(super) struct SelectionPreviews {
    pub definitions: std::collections::BTreeMap<SourceTarget, Selection>,
    key: Option<PreviewKey>,
    pub texture: Option<wgpu::TextureView>,
    pub pending: bool,
    pipeline: Option<PreviewPipeline>,
    charge: Option<crate::residency::Charge>,
}
impl SelectionPreviews {
    fn reset(&mut self) {
        (self.key, self.texture, self.pending, self.charge) = (None, None, false, None);
    }
}
struct PreviewPipeline {
    layout: wgpu::BindGroupLayout,
    merge: Deferred<wgpu::ComputePipeline>,
    thumbnail: Deferred<wgpu::RenderPipeline>,
}
impl PreviewPipeline {
    fn new(r: &WgpuRasterizer) -> Self {
        let layout = crate::bindings::layout(&r.device, "saved mask display", &[0, 1, 2].map(|binding| crate::bindings::buffer(
            binding,
            if binding == 2 { wgpu::ShaderStages::COMPUTE } else { wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::FRAGMENT },
            match binding { 0 => wgpu::BufferBindingType::Uniform, 1 => wgpu::BufferBindingType::Storage { read_only: true }, _ => wgpu::BufferBindingType::Storage { read_only: false }, },
            false,
            None,
        )));
        let pl = r
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("saved mask display"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let clip = include_str!("selection_clip.wgsl").replace("@group(1)", "@group(0)");
        let shader = |source: &str| Deferred::wgsl(&r.device, "saved mask display", compose_wgsl(&[&clip, source]).into_owned());
        let compute = shader(include_str!("selection_previews.wgsl"));
        let draw = shader(include_str!("selection_thumbnail.wgsl"));
        let merge = Deferred::compute(&r.device, "saved mask overlays", &pl, &compute, "merge");
        let thumbnail = {
            let (device, layout) = (r.device.for_recipe(), pl);
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
    #[expect(clippy::too_many_arguments, reason = "Overlay bands keep their extent, first row and mask rule explicit")]
    fn bind(
        &self,
        r: &WgpuRasterizer,
        extent: [u32; 2],
        top: u32,
        protected: bool,
        color: [f32; 4],
        input: &wgpu::Buffer,
        output: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        let params = r
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("saved mask display parameters"),
                contents: &[extent[0], extent[1], u32::from(protected), top]
                    .into_iter()
                    .chain(color.map(f32::to_bits))
                    .flat_map(u32::to_ne_bytes)
                    .collect::<Vec<_>>(),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        crate::bindings::group(&r.device, "saved mask display", &self.layout, [&params, input, output].map(|b| b.as_entire_binding()))
    }
    fn ready(&self, r: &WgpuRasterizer, thumbnail: bool) -> Result<bool, GpuRasterError> {
        r.compilation.check()?;
        let clip = r.compilation
            .require(r.selection_clip.pipelines(), startup::BRUSH);
        Ok(clip
            & if thumbnail {
                r.compilation.require([&self.thumbnail], startup::BRUSH)
            } else {
                r.compilation.require([&self.merge], startup::BRUSH)
            })
    }
}
impl WgpuRasterizer {
    pub(super) fn prepare_selection_previews(
        &mut self,
        packet: FramePacket<'_>,
    ) -> Result<(), GpuRasterError> {
        let scene = packet.scene;
        let mut previews = mem::take(&mut self.selection_previews);
        let result = (|| {
            previews.definitions.retain(|target, _| *target == SourceTarget::Selection(layer_core::authored::SelectionHandle::INVALID)
                || scene.targets().any(|current| current == *target));
            for &handle in scene.order() {
                let Some(SourceTarget::Selection(target)) = scene.source_target(handle) else { continue; };
                let saved = scene.artwork().selections.get(target).ok_or(GpuRasterError::InvalidImage)?;
                let coverage = saved.selection.translated(layer_core::offsets::point(scene.target_offset(SourceTarget::Selection(target))));
                previews.definitions.insert(SourceTarget::Selection(target), coverage);
            }
            let Some(options) = self.selection_overlay else { previews.reset(); return Ok(()); };
            let masks: Vec<_> = scene.order().iter().rev().filter_map(|&handle| {
                let target = scene.source_target(handle)?;
                let SourceTarget::Selection(saved) = target else { return None; };
                let visible = packet.selection_overlays.and_then(|overlays| overlays.visibility.get(&handle)).copied()
                    .unwrap_or_else(|| scene.occurrence(handle).is_some_and(|occurrence| occurrence.visible));
                if Some(handle) == options.editing || !visible || scene.parent(handle).is_some_and(|parent| !scene.visible(parent)) { return None; }
                let properties = packet.selection_overlays.and_then(|overlays| overlays.properties.get(&saved)).cloned().unwrap_or_default();
                previews.definitions.get(&target).cloned().map(|coverage| (coverage, properties))
            }).collect();
            if masks.is_empty() {
                previews.reset();
                return Ok(());
            }
            let key = (masks, self.document_extent, options.saved_protected);
            if previews.key.as_ref() == Some(&key) {
                return Ok(());
            }
            let gpu = previews
                .pipeline
                .get_or_insert_with(|| PreviewPipeline::new(self));
            previews.pending = !gpu.ready(self, false)?;
            if previews.pending { return Ok(()); }
            let [w, h] = self.document_extent;
            let row = (w * 4).div_ceil(256) * 256;
            let limits = self.device.limits();
            let band = (limits.max_storage_buffer_binding_size.saturating_sub(32) / u64::from(row))
                .min(u64::from(limits.max_compute_workgroups_per_dimension))
                .min(u64::from(h)) as u32;
            if band == 0 {
                return Err(GpuRasterError::SizeOverflow);
            }
            let output = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("visible saved-mask overlay band"),
                size: 32 + u64::from(row) * u64::from(band),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
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
            let mut encoder = crate::submission::CommandEncoder::new(
                &self.device,
                &wgpu::CommandEncoderDescriptor {
                    label: Some("saved selection previews"),
                },
            );
            for top in (0..h).step_by(band as usize) {
                let rows = band.min(h - top);
                encoder.clear_buffer(&output, 0, None);
                for (mask, properties) in &key.0 {
                    self.selection_clip.prepare_region(
                        &self.device,
                        &mut encoder,
                        self.document_extent,
                        &Arc::new(mask.clone()),
                        Some(PixelRect::new(0, top, w, top + rows)),
                    )?;
                    let bind = gpu.bind(
                        self,
                        [w, rows],
                        top,
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
                    pass.dispatch_workgroups(w.div_ceil(64), rows, 1);
                }
                encoder.copy_buffer_to_texture(
                    wgpu::TexelCopyBufferInfo {
                        buffer: &output,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 32,
                            bytes_per_row: Some(row),
                            rows_per_image: Some(rows),
                        },
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x: 0, y: top, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d { width: w, height: rows, depth_or_array_layers: 1 },
                );
            }
            previews.texture = Some(texture.create_view(&Default::default()));
            previews.charge = Some(self.raster_tiers.paint.charge(texture_bytes(&texture)));
            self.uploads.finish(&encoder);
            encoder.submit(&self.queue);
            previews.key = Some(key);
            Ok(())
        })();
        self.selection_previews = previews;
        result
    }
    /// Poll cold mask preview pipelines without blocking the host UI thread.
    pub fn prepare_selection_thumbnail(&mut self, id: SourceTarget) -> Result<bool, GpuRasterError> {
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
        id: SourceTarget,
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
            0,
            false,
            [0.; 4],
            self.selection_clip.buffer.as_ref().unwrap(),
            &scratch,
        );
        let result = create_page_surface(
            &self.device,
            &self.float_texture_layout,
            &self.sampler,
            [32, 32],
            self.device.working_format(),
            "selection layer thumbnail",
            &self.raster_tiers.paint,
        );
        let mut pass = encoder.color_pass(
            "selection layer thumbnail",
            &result.view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        );
        pass.set_pipeline(&gpu.thumbnail);
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        self.selection_previews.pipeline = Some(gpu);
        Ok(result)
    }
}
