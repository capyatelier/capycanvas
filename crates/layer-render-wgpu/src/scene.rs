//! Tiled layer composition with explicit cached image boundaries. Pointwise
//! scratch follows nesting depth. Masks never download or rewrite paint.
use super::*;
#[path = "scene_images.rs"]
mod images;
#[path = "filter_previews.rs"]
mod previews;
pub(super) use previews::FilterPreviews;

#[derive(Clone)]
enum Job {
    Effect {
        target: wgpu::TextureView,
        sources: [wgpu::TextureView; 2],
        data: [f32; 24],
        prepared: effects::PreparedEffect,
        // Keep ordinary draw jobs small: only effects carry the mask inputs.
        masks: Box<[wgpu::TextureView; effects::MASK_SLOTS]>,
    },
    Draw {
        target: wgpu::TextureView,
        sources: [wgpu::TextureView; 2],
        data: [f32; 24],
        over: bool,
    },
    Clear(wgpu::TextureView, wgpu::Color),
    Watercolor {
        target: wgpu::TextureView,
        binding: wgpu::BindGroup,
        record: u32,
        coordinate: [u32; 2],
    },
    Copy {
        source: wgpu::Texture,
        destination: wgpu::Texture,
        origin: [u32; 2],
        width: u32,
        height: u32,
    },
}
pub(super) struct Scene {
    pub style_base: usize,
    pool: Vec<PageSurface>,
    used: Vec<bool>,
    jobs: Vec<Job>,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::BindGroupLayout,
    buffer: wgpu::Buffer,
    binding: wgpu::BindGroup,
    stride: usize,
    capacity: usize,
    record_count: usize,
    upload: Vec<u8>,
    pipeline: [wgpu::RenderPipeline; 2],
    pub(super) effects: effects::Effects,
    pub effect_passes: u64,
    images: images::ImageStages,
    stop_before: Option<(usize, bool)>,
}
impl Scene {
    #[cfg(test)]
    pub fn image_work(&self) -> [u64; 2] {
        [self.images.input_updates, self.images.pass_updates]
    }
    #[cfg(test)]
    pub fn image_cache_bytes(&self) -> u64 {
        self.images.storage_bytes()
    }
    pub fn scratch_bytes(&self) -> u64 {
        self.pool.len() as u64 * PAGE_SIZE as u64 * PAGE_SIZE as u64 * 4
            + (self.capacity * self.stride) as u64
            + self.effects.storage_bytes()
            + self.images.storage_bytes()
    }
    pub fn initialize_images(
        &mut self,
        r: &mut WgpuRasterizer,
        layers: &[Layer],
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        self.jobs.clear();
        for layer in layers {
            let Some((source, extent)) = layer.asset.as_ref().and_then(|a| r.images.get(a)) else {
                continue;
            };
            let Some(stored) = r.paint_layers.iter().find(|p| p.id == layer.id) else {
                continue;
            };
            for page in &stored.pages {
                let mut data = [0.; 24];
                data[..4].copy_from_slice(&[
                    -((page.coordinate[0] * PAGE_SIZE) as f32),
                    -((page.coordinate[1] * PAGE_SIZE) as f32),
                    extent[0] as f32,
                    extent[1] as f32,
                ]);
                data[4..8].copy_from_slice(&[256., 256., 0., 0.]);
                data[8] = 8.;
                self.jobs.push(Job::Draw {
                    target: page.active().view.clone(),
                    sources: [source.clone(), r.empty_view.clone()],
                    data,
                    over: false,
                });
            }
        }
        self.encode_jobs(r, encoder)
    }
    pub fn begin_frame(&mut self) {
        self.record_count = 0;
        self.effect_passes = 0;
    }
    pub fn new(r: &WgpuRasterizer) -> Self {
        let device = &r.device;
        let uniforms = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene records"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(96),
                },
                count: None,
            }],
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene sources"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("layer scene"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene composition"),
            bind_group_layouts: &[Some(&uniforms), Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = [None, Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING)].map(|blend| {
            fullscreen_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "fragment_main",
                blend,
                COLOR_FORMAT,
                "tile layer composition",
            )
        });
        let stride = device.limits().min_uniform_buffer_offset_alignment.max(96) as usize;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene uniform records"),
            size: stride as u64 * 128,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let binding = uniform_binding(device, &uniforms, &buffer);
        let effects = effects::Effects::new(r, &uniforms, &layout);
        Self {
            style_base: 0,
            pool: Vec::new(),
            used: Vec::new(),
            jobs: Vec::new(),
            layout,
            uniforms,
            buffer,
            binding,
            stride,
            capacity: 128,
            record_count: 0,
            upload: Vec::new(),
            pipeline,
            effects,
            effect_passes: 0,
            images: images::ImageStages::default(),
            stop_before: None,
        }
    }
    fn alloc(&mut self, r: &WgpuRasterizer, color: wgpu::Color) -> usize {
        let id = self.reserve(r);
        self.jobs
            .push(Job::Clear(self.pool[id].view.clone(), color));
        id
    }
    fn reserve(&mut self, r: &WgpuRasterizer) -> usize {
        let id = self
            .used
            .iter()
            .position(|v| !*v)
            .unwrap_or(self.pool.len());
        if id == self.pool.len() {
            self.pool.push(r.create_page_surface("scene reusable tile"));
            self.used.push(false);
        }
        self.used[id] = true;
        id
    }
    fn free(&mut self, id: usize) {
        self.used[id] = false;
    }
    fn effect(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        indices: &[usize],
        tile: [u32; 2],
        input: usize,
    ) -> Result<usize, GpuRasterError> {
        let layers: Vec<_> = indices.iter().map(|i| &packet.layers[*i]).collect();
        let layer = layers[0];
        if layer.effect.as_ref().unwrap().program.image_boundary() {
            let view = self
                .images
                .output(layer.id)
                .ok_or_else(|| GpuRasterError::Effect("Missing image effect stage".into()))?;
            let out = self.image_tile(r, view, packet.document_extent, tile);
            self.free(input);
            return Ok(out);
        }
        let prepared =
            self.effects
                .prepare(r, &layers, effects::Execution::Fused, packet.time_seconds)?;
        let mask =
            if indices.len() == 1 && !direct_effect_mask(packet.layers, layer) {
                layer.mask.as_ref().filter(|m| m.enabled).map(|m| {
                    self.mask_tile(r, m, world_offset(packet.layers, layer.id, true), tile)
                })
            } else {
                None
            };
        let out = self.reserve(r);
        let mut data = [0.; 24];
        data[..4].copy_from_slice(&[0., 0., 256., 256.]);
        data[4..6].copy_from_slice(&[256., 256.]);
        data[11] = f32::from(mask.is_some());
        let mut masks = Box::new(std::array::from_fn(|_| r.empty_view.clone()));
        let mut present = 0u32;
        let mut inverted = 0u32;
        if mask.is_none() {
            for (i, l) in layers.iter().enumerate().take(effects::MASK_SLOTS) {
                if let Some(m) = l.mask.as_ref().filter(|m| m.enabled)
                    && let Some(page) = r.layer_masks.pages.get(&(m.id, tile))
                {
                    masks[i] = page.view.clone();
                    present |= 1 << i;
                    if m.inverted {
                        inverted |= 1 << i;
                    }
                }
            }
        }
        data[6] = present as f32;
        data[7] = inverted as f32;
        data[12..16].copy_from_slice(&[
            (tile[0] * PAGE_SIZE) as f32,
            (tile[1] * PAGE_SIZE) as f32,
            packet.document_extent[0] as f32,
            packet.document_extent[1] as f32,
        ]);
        let mut sources = [
            self.pool[input].view.clone(),
            mask.map_or_else(|| r.empty_view.clone(), |m| self.pool[m].view.clone()),
        ];
        // Lower a source-over operation followed by an adjustment into one
        // shader invocation. No intermediate color tile or pass is necessary.
        // This is program-independent; masks and nonlocal operations delimit it.
        if mask.is_none() && self.jobs.len() >= 2 {
            let n = self.jobs.len();
            if let (
                Job::Clear(clear, bg),
                Job::Draw {
                    target,
                    sources: paint,
                    data: paint_data,
                    over: true,
                },
            ) = (&self.jobs[n - 2], &self.jobs[n - 1])
                && *clear == self.pool[input].view
                && target == clear
                && paint_data[..4] == [0., 0., 256., 256.]
                && (paint_data[8] == 7. || paint_data[8] == 1.)
            {
                sources = paint.clone();
                data[16..20].copy_from_slice(&[
                    paint_data[9],
                    if paint_data[8] == 1. {
                        1.
                    } else {
                        paint_data[10]
                    },
                    paint_data[11],
                    1.,
                ]);
                data[20..24].copy_from_slice(&[bg.r as f32, bg.g as f32, bg.b as f32, bg.a as f32]);
                self.jobs.truncate(n - 2);
            }
        }
        self.jobs.push(Job::Effect {
            target: self.pool[out].view.clone(),
            sources,
            data,
            prepared,
            masks,
        });
        self.free(input);
        if let Some(m) = mask {
            self.free(m);
        }
        Ok(out)
    }
    #[allow(clippy::too_many_arguments)] // Explicit tile draw operands.
    fn draw(
        &mut self,
        r: &WgpuRasterizer,
        target: usize,
        source: wgpu::TextureView,
        back: Option<wgpu::TextureView>,
        rect: [f32; 4],
        options: [f32; 4],
        over: bool,
    ) {
        let mut data = [0.; 24];
        data[..4].copy_from_slice(&rect);
        data[4..8].copy_from_slice(&[256., 256., 0., 0.]);
        data[8..12].copy_from_slice(&options);
        self.jobs.push(Job::Draw {
            target: self.pool[target].view.clone(),
            sources: [source, back.unwrap_or_else(|| r.empty_view.clone())],
            data,
            over,
        });
    }
    fn combine(
        &mut self,
        r: &WgpuRasterizer,
        front: usize,
        back: usize,
        opacity: f32,
        blend: layer_core::LayerBlend,
        clip: bool,
    ) -> usize {
        // An isolated clipping stack over a constant backdrop needs no color
        // intermediate after its last adjustment. Fold that final composite.
        let n = self.jobs.len();
        if !clip && n >= 2 {
            let bg = if let Job::Clear(view, color) = &self.jobs[n - 2] {
                (*view == self.pool[back].view).then_some(*color)
            } else {
                None
            };
            if let Some(bg) = bg
                && let Job::Effect { target, data, .. } = &mut self.jobs[n - 1]
                && *target == self.pool[front].view
                && data[8] == 0.
            {
                data[8] = 1.;
                data[9] = opacity;
                data[10] = blend as u32 as f32;
                if data[19] > 0.5 {
                    data[19] = 2.;
                }
                data[20..24].copy_from_slice(&[bg.r as f32, bg.g as f32, bg.b as f32, bg.a as f32]);
                self.jobs.remove(n - 2);
                self.free(back);
                return front;
            }
        }
        let out = self.alloc(r, wgpu::Color::TRANSPARENT);
        self.draw(
            r,
            out,
            self.pool[front].view.clone(),
            Some(self.pool[back].view.clone()),
            [0., 0., 256., 256.],
            [4., opacity, blend as u32 as f32, f32::from(clip)],
            false,
        );
        self.free(front);
        self.free(back);
        out
    }
    fn mask_tile(
        &mut self,
        r: &WgpuRasterizer,
        mask: &layer_core::LayerMask,
        offset: layer_core::Point,
        tile: [u32; 2],
    ) -> usize {
        let default = if mask.inverted {
            1. - mask.default_coverage
        } else {
            mask.default_coverage
        } as f64;
        let out = self.alloc(
            r,
            wgpu::Color {
                r: default,
                g: default,
                b: default,
                a: 1.,
            },
        );
        for ((id, c), page) in &r.layer_masks.pages {
            if *id != mask.id {
                continue;
            }
            let rect = local_rect(*c, offset, tile);
            if !intersects(rect) {
                continue;
            }
            self.draw(
                r,
                out,
                page.view.clone(),
                None,
                rect,
                [2., 1., f32::from(mask.inverted), 0.],
                false,
            );
        }
        out
    }
    fn layer(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        index: usize,
        tile: [u32; 2],
    ) -> Result<usize, GpuRasterError> {
        let layer = &packet.layers[index];
        let out = if layer.kind == LayerKind::Group {
            self.group(r, packet, Some(layer.id), tile)?
        } else if layer.kind == LayerKind::Effect {
            let input = self.alloc(r, wgpu::Color::TRANSPARENT);
            // Generator coverage is applied below with ordinary layer masks.
            self.effect(r, packet, &[index], tile, input)?
        } else {
            let out = self.alloc(r, wgpu::Color::TRANSPARENT);
            let offset = world_offset(packet.layers, layer.id, false);
            if let Some(stored) = r.paint_layers.iter().find(|l| l.id == layer.id) {
                // Resolve watercolor in its native page coordinates before translating.
                let mut coordinates: std::collections::BTreeSet<_> =
                    stored.pages.iter().map(|p| p.coordinate).collect();
                if r.preview_layer_id == Some(layer.id) {
                    coordinates.extend(r.preview_pages.iter().map(|p| p.coordinate));
                }
                if stored.watercolor.is_some() {
                    let previous = coordinates.clone();
                    for c in previous {
                        for y in c[1].saturating_sub(1)..=c[1] + 1 {
                            for x in c[0].saturating_sub(1)..=c[0] + 1 {
                                coordinates.insert([x, y]);
                            }
                        }
                    }
                }
                for c in coordinates {
                    if c[0] * PAGE_SIZE >= r.document_extent[0]
                        || c[1] * PAGE_SIZE >= r.document_extent[1]
                    {
                        continue;
                    }
                    let rect = local_rect(c, offset, tile);
                    if !intersects(rect) {
                        continue;
                    }
                    let preview = r.preview_layer_id == Some(layer.id)
                        && !r.preview_damage.intersect(page_rect(c)).is_empty();
                    if stored.watercolor.is_some() {
                        if let Some(binding) =
                            r.watercolor_neighborhood_bind_group(stored, c, preview)
                        {
                            let page = self.alloc(r, wgpu::Color::TRANSPARENT);
                            self.jobs.push(Job::Watercolor {
                                target: self.pool[page].view.clone(),
                                binding,
                                record: (self.style_base + index) as u32,
                                coordinate: c,
                            });
                            self.draw(
                                r,
                                out,
                                self.pool[page].view.clone(),
                                None,
                                rect,
                                [1., 1., 0., 0.],
                                true,
                            );
                            self.free(page);
                        }
                    } else {
                        let persistent = stored.pages.iter().find(|p| p.coordinate == c);
                        let predicted = if preview {
                            r.preview_pages.iter().find(|p| p.coordinate == c)
                        } else {
                            None
                        };
                        if let Some(p) =
                            predicted.filter(|_| r.preview_requires_base).or(persistent)
                        {
                            self.draw(
                                r,
                                out,
                                p.active().view.clone(),
                                None,
                                rect,
                                [1., 1., 0., 0.],
                                true,
                            );
                        }
                        if let Some(p) = predicted.filter(|_| !r.preview_requires_base) {
                            self.draw(
                                r,
                                out,
                                p.active().view.clone(),
                                None,
                                rect,
                                [1., 1., 0., 0.],
                                true,
                            );
                        }
                    }
                }
            }
            out
        };
        if let Some(mask) = layer.mask.as_ref().filter(|m| m.enabled) {
            let m = self.mask_tile(r, mask, world_offset(packet.layers, layer.id, true), tile);
            let result = self.alloc(r, wgpu::Color::TRANSPARENT);
            self.draw(
                r,
                result,
                self.pool[out].view.clone(),
                Some(self.pool[m].view.clone()),
                [0., 0., 256., 256.],
                [3., 1., 0., 0.],
                false,
            );
            self.free(out);
            self.free(m);
            Ok(result)
        } else {
            Ok(out)
        }
    }
    fn group(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        parent: Option<LayerId>,
        tile: [u32; 2],
    ) -> Result<usize, GpuRasterError> {
        let mut output = self.alloc(
            r,
            if parent.is_none() {
                let c = packet.view.background_rgba_linear;
                wgpu::Color {
                    r: (c[0] * c[3]) as f64,
                    g: (c[1] * c[3]) as f64,
                    b: (c[2] * c[3]) as f64,
                    a: c[3] as f64,
                }
            } else {
                wgpu::Color::TRANSPARENT
            },
        );
        let mut stack: Option<(usize, usize)> = None;
        // An image boundary already contains its full input stack, final mask
        // and layer properties. Start above the latest completed boundary.
        let checkpoint = packet.layers.iter().enumerate().find(|(i, l)| {
            l.visible
                && l.properties.parent == parent
                && !l.properties.clipped
                && l.effect
                    .as_ref()
                    .is_some_and(|e| e.program.kind == layer_core::EffectKind::Adjustment)
                && self.stop_before.is_none_or(|(stop, _)| *i > stop)
                && self.images.output(l.id).is_some()
        });
        if let Some((_, l)) = checkpoint {
            self.free(output);
            output = self.image_tile(
                r,
                self.images.output(l.id).unwrap(),
                packet.document_extent,
                tile,
            );
        }
        let mut siblings = packet
            .layers
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, l)| l.properties.parent == parent && l.kind != LayerKind::Background)
            .filter(|(i, _)| checkpoint.is_none_or(|(cut, _)| *i < cut))
            .peekable();
        while let Some((i, layer)) = siblings.next() {
            if let Some((stop, clipped)) = self.stop_before
                && stop == i
            {
                if clipped {
                    self.free(output);
                    return Ok(stack.map_or_else(
                        || self.alloc(r, wgpu::Color::TRANSPARENT),
                        |(pixels, _)| pixels,
                    ));
                }
                if let Some((pixels, base)) = stack {
                    let b = &packet.layers[base];
                    output = self.combine(r, pixels, output, b.opacity, b.properties.blend, false);
                }
                return Ok(output);
            }
            if layer
                .effect
                .as_ref()
                .is_some_and(|e| e.program.kind == layer_core::EffectKind::Adjustment)
            {
                if !layer.properties.clipped
                    && let Some((pixels, base)) = stack.take()
                {
                    let b = &packet.layers[base];
                    output = self.combine(r, pixels, output, b.opacity, b.properties.blend, false);
                }
                if !layer.visible {
                    continue;
                }
                let mut chain = vec![i];
                if direct_effect_mask(packet.layers, layer)
                    && !layer.effect.as_ref().unwrap().program.image_boundary()
                {
                    while let Some((j, next)) = siblings.peek() {
                        if self.stop_before.is_some_and(|(stop, _)| *j <= stop)
                            || !next.visible
                            || next.properties.clipped != layer.properties.clipped
                            || !direct_effect_mask(packet.layers, next)
                            || (chain.len() >= effects::MASK_SLOTS
                                && next.mask.as_ref().is_some_and(|m| m.enabled))
                            || !next.effect.as_ref().is_some_and(|e| {
                                e.program.kind == layer_core::EffectKind::Adjustment
                                    && !e.program.image_boundary()
                            })
                        {
                            break;
                        }
                        chain.push(*j);
                        siblings.next();
                    }
                }
                if layer.properties.clipped {
                    if let Some((pixels, base)) = stack.take() {
                        stack = Some((self.effect(r, packet, &chain, tile, pixels)?, base));
                    }
                } else {
                    output = self.effect(r, packet, &chain, tile, output)?;
                }
                continue;
            }
            if !layer.properties.clipped {
                if let Some((pixels, base)) = stack.take() {
                    let b = &packet.layers[base];
                    output = self.combine(r, pixels, output, b.opacity, b.properties.blend, false);
                }
                let clips_above = packet.layers[..i]
                    .iter()
                    .rev()
                    .find(|l| l.properties.parent == parent)
                    .is_some_and(|l| l.properties.clipped);
                if layer.visible
                    && !clips_above
                    && self.draw_normal_layer(r, packet, i, tile, output)
                {
                    continue;
                }
                if layer.visible
                    && (matches!(layer.kind, LayerKind::Group | LayerKind::Effect)
                        || r.paint_layers
                            .iter()
                            .any(|l| l.id == layer.id && !l.pages.is_empty())
                        || r.preview_layer_id == Some(layer.id))
                {
                    stack = Some((self.layer(r, packet, i, tile)?, i));
                }
            } else if layer.visible
                && let Some((pixels, base)) = stack.take()
            {
                let source = self.layer(r, packet, i, tile)?;
                stack = Some((
                    self.combine(
                        r,
                        source,
                        pixels,
                        layer.opacity,
                        layer.properties.blend,
                        true,
                    ),
                    base,
                ));
            }
        }
        if let Some((pixels, base)) = stack {
            let b = &packet.layers[base];
            output = self.combine(r, pixels, output, b.opacity, b.properties.blend, false);
        }
        Ok(output)
    }

    // A normal paint tile with an aligned scalar mask needs one source-over
    // draw, not separate color, mask, multiplication and blend scratch passes.
    fn draw_normal_layer(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        index: usize,
        tile: [u32; 2],
        target: usize,
    ) -> bool {
        let layer = &packet.layers[index];
        if layer.kind != LayerKind::Paint
            || layer.properties.blend != layer_core::LayerBlend::Normal
            || world_offset(packet.layers, layer.id, false) != layer_core::Point::default()
            || r.preview_layer_id == Some(layer.id)
        {
            return false;
        }
        let mask = layer.mask.as_ref().filter(|m| m.enabled);
        if mask.is_some()
            && world_offset(packet.layers, layer.id, true) != layer_core::Point::default()
        {
            return false;
        }
        let Some(stored) = r.paint_layers.iter().find(|l| l.id == layer.id) else {
            return true;
        };
        if stored.watercolor.is_some() {
            return false;
        }
        let Some(page) = stored.pages.iter().find(|p| p.coordinate == tile) else {
            return true;
        };
        let default = mask.map_or(1., |m| {
            if m.inverted {
                1. - m.default_coverage
            } else {
                m.default_coverage
            }
        });
        let source = mask.and_then(|m| r.layer_masks.pages.get(&(m.id, tile)));
        self.draw(
            r,
            target,
            page.active().view.clone(),
            source.map(|p| p.view.clone()),
            [0., 0., 256., 256.],
            [
                7.,
                layer.opacity,
                default,
                source.map_or(0., |_| 2. + f32::from(mask.unwrap().inverted)),
            ],
            true,
        );
        true
    }
    pub fn apply_operation(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        layer_index: usize,
        operation_index: usize,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        use layer_core::LayerOperationKind;
        self.jobs.clear();
        self.used.fill(false);
        let layer = &packet.layers[layer_index];
        let op = &layer.operations[operation_index];
        let Some(stored) = r.paint_layers.iter().find(|l| l.id == layer.id) else {
            return Ok(());
        };
        let pages: Vec<_> = stored
            .pages
            .iter()
            .map(|p| {
                (
                    p.coordinate,
                    p.active().view.clone(),
                    p.active().texture.clone(),
                )
            })
            .collect();
        let watercolor = stored.watercolor.is_some() && op.kind == LayerOperationKind::ApplyMask;
        for (c, source, destination) in pages {
            let mask = self.mask_tile(r, &op.coverage, op.coverage.offset, c);
            let out = self.alloc(r, wgpu::Color::TRANSPARENT);
            match op.kind {
                LayerOperationKind::ApplyMask => {
                    let mut resolved = None;
                    if watercolor
                        && let Some(binding) =
                            r.watercolor_neighborhood_bind_group(stored, c, false)
                    {
                        let p = self.alloc(r, wgpu::Color::TRANSPARENT);
                        self.jobs.push(Job::Watercolor {
                            target: self.pool[p].view.clone(),
                            binding,
                            record: (self.style_base + layer_index) as u32,
                            coordinate: c,
                        });
                        resolved = Some(p);
                    }
                    self.draw(
                        r,
                        out,
                        resolved.map_or(source, |p| self.pool[p].view.clone()),
                        Some(self.pool[mask].view.clone()),
                        [0., 0., 256., 256.],
                        [3., 1., 0., 0.],
                        false,
                    );
                    if let Some(p) = resolved {
                        self.free(p);
                    }
                }
                LayerOperationKind::Fill {
                    color,
                    alpha_locked,
                } => {
                    self.draw(
                        r,
                        out,
                        self.pool[mask].view.clone(),
                        None,
                        [0., 0., 256., 256.],
                        [6., 1., 0., 0.],
                        false,
                    );
                    if let Some(Job::Draw { data, .. }) = self.jobs.last_mut() {
                        data[12..16].copy_from_slice(&color);
                    }
                    let combined = self.alloc(r, wgpu::Color::TRANSPARENT);
                    self.draw(
                        r,
                        combined,
                        self.pool[out].view.clone(),
                        Some(source),
                        [0., 0., 256., 256.],
                        [4., 1., 0., f32::from(alpha_locked)],
                        false,
                    );
                    self.jobs.push(Job::Copy {
                        source: self.pool[combined].texture.clone(),
                        destination,
                        origin: [0, 0],
                        width: 256,
                        height: 256,
                    });
                    self.free(combined);
                    self.free(out);
                    self.free(mask);
                    continue;
                }
            }
            self.jobs.push(Job::Copy {
                source: self.pool[out].texture.clone(),
                destination,
                origin: [0, 0],
                width: 256,
                height: 256,
            });
            self.free(out);
            self.free(mask);
        }
        self.encode_jobs(r, encoder)?;
        if op.kind == LayerOperationKind::ApplyMask {
            // Appearance is baked before discarding material state. Hidden
            // reservoirs/wet pigment must not bring discarded content back.
            if let Some(stored) = r.paint_layers.iter().find(|l| l.id == layer.id) {
                for p in &stored.watercolor_wetness_pages {
                    r.encode_clear(encoder, &p.primary.view, "clear baked wetness");
                    r.encode_clear(encoder, &p.secondary.view, "clear baked wetness companion");
                }
                for p in &stored.material_pages {
                    r.encode_clear(encoder, &p.wetness.view, "clear baked material");
                }
                for p in &stored.coverage_pages {
                    r.encode_clear(encoder, &p.primary.view, "clear baked stroke coverage");
                    r.encode_clear(encoder, &p.secondary.view, "clear baked coverage companion");
                }
            }
            if let Some(stored) = r.paint_layers.iter_mut().find(|l| l.id == layer.id) {
                stored.watercolor = None;
            }
        }
        Ok(())
    }

    pub fn compose(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        dirty: PixelRect,
        encoder: &mut wgpu::CommandEncoder,
        overlay: bool,
    ) -> Result<(), GpuRasterError> {
        self.effects.retain(packet.layers);
        let dirty = self.update_images(r, packet, dirty, encoder)?;
        if dirty.is_empty() {
            return Ok(());
        }
        self.jobs.clear();
        self.used.fill(false);
        // A topmost full-image checkpoint is already the final composition.
        // One region copy replaces a tile draw + copy for every document tile.
        if let Some(top) = packet
            .layers
            .iter()
            .find(|l| l.visible && l.properties.parent.is_none() && l.kind != LayerKind::Background)
            && !top.properties.clipped
            && top
                .effect
                .as_ref()
                .is_some_and(|e| e.program.kind == layer_core::EffectKind::Adjustment)
            && (!overlay
                || !packet
                    .layers
                    .iter()
                    .any(|l| l.mask.as_ref().is_some_and(|m| m.enabled && m.show_area)))
            && let Some(source) = self.images.output_texture(top.id)
        {
            let origin = wgpu::Origin3d {
                x: dirty.min_x,
                y: dirty.min_y,
                z: 0,
            };
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: source,
                    origin,
                    ..source.as_image_copy()
                },
                wgpu::TexelCopyTextureInfo {
                    texture: r.composite_texture.as_ref().unwrap(),
                    origin,
                    ..source.as_image_copy()
                },
                wgpu::Extent3d {
                    width: dirty.width(),
                    height: dirty.height(),
                    depth_or_array_layers: 1,
                },
            );
            r.metrics.composited_pixels += dirty.area();
            return Ok(());
        }
        for tile in page_coordinates(dirty) {
            let mut output = self.group(r, packet, None, tile)?;
            if overlay {
                for layer in packet.layers {
                    if let Some(mask) = layer.mask.as_ref().filter(|m| m.enabled && m.show_area) {
                        let m = self.mask_tile(
                            r,
                            mask,
                            world_offset(packet.layers, layer.id, true),
                            tile,
                        );
                        let tint = self.alloc(r, wgpu::Color::TRANSPARENT);
                        self.draw(
                            r,
                            tint,
                            self.pool[m].view.clone(),
                            None,
                            [0., 0., 256., 256.],
                            [5., 1., 0., 0.],
                            false,
                        );
                        self.free(m);
                        output = self.combine(
                            r,
                            tint,
                            output,
                            1.,
                            layer_core::LayerBlend::Normal,
                            false,
                        );
                    }
                }
            }
            let origin = [tile[0] * PAGE_SIZE, tile[1] * PAGE_SIZE];
            // The last effect already writes every pixel. Write directly into
            // the composite region instead of copying its scratch result.
            if let Some(Job::Effect { target, data, .. }) = self.jobs.last_mut()
                && *target == self.pool[output].view
            {
                *target = r.composite_view.as_ref().unwrap().clone();
                data[..6].copy_from_slice(&[
                    origin[0] as f32,
                    origin[1] as f32,
                    PAGE_SIZE.min(packet.document_extent[0] - origin[0]) as f32,
                    PAGE_SIZE.min(packet.document_extent[1] - origin[1]) as f32,
                    packet.document_extent[0] as f32,
                    packet.document_extent[1] as f32,
                ]);
                self.free(output);
                continue;
            }
            self.jobs.push(Job::Copy {
                source: self.pool[output].texture.clone(),
                destination: r.composite_texture.as_ref().unwrap().clone(),
                origin,
                width: PAGE_SIZE.min(packet.document_extent[0] - origin[0]),
                height: PAGE_SIZE.min(packet.document_extent[1] - origin[1]),
            });
            self.free(output);
        }
        self.encode_jobs(r, encoder)?;
        r.metrics.composited_pixels += dirty.area();
        Ok(())
    }

    // wgpu handles hash by stable resource identity, not mutable GPU contents.
    #[allow(clippy::mutable_key_type)]
    fn encode_jobs(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        let base = self.record_count;
        self.record_count += self.jobs.len();
        if self.record_count > self.capacity {
            self.capacity = self.record_count.next_power_of_two();
            self.buffer = r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("scene records"),
                size: (self.capacity * self.stride) as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.binding = uniform_binding(&r.device, &self.uniforms, &self.buffer);
        }
        self.upload.resize(self.jobs.len() * self.stride, 0);
        for (i, job) in self.jobs.iter().enumerate() {
            if let Job::Draw { data, .. } | Job::Effect { data, .. } = job {
                for (j, v) in data.iter().enumerate() {
                    self.upload[i * self.stride + j * 4..i * self.stride + j * 4 + 4]
                        .copy_from_slice(&v.to_ne_bytes());
                }
            }
        }
        if !self.upload.is_empty() {
            r.uploads.write_at(
                encoder,
                &r.queue,
                &self.buffer,
                (base * self.stride) as u64,
                &self.upload,
            );
        }
        let mut source_bindings = std::collections::HashMap::new();
        let mut mask_bindings = std::collections::HashMap::new();
        let mut encoded_through = 0;
        for (i, job) in self.jobs.iter().enumerate() {
            if i < encoded_through {
                continue;
            }
            match job {
                Job::Clear(target, color) => {
                    if self.jobs.get(i+1).is_some_and(|next|matches!(next,Job::Draw{target:next,..}|Job::Effect{target:next,..} if next==target)) {continue;}
                    let attachments = [Some(attachment(target, wgpu::LoadOp::Clear(*color)))];
                    let _pass = encoder.begin_render_pass(&descriptor(&attachments));
                }
                Job::Copy {
                    source,
                    destination,
                    origin,
                    width,
                    height,
                } => encoder.copy_texture_to_texture(
                    source.as_image_copy(),
                    wgpu::TexelCopyTextureInfo {
                        texture: destination,
                        origin: wgpu::Origin3d {
                            x: origin[0],
                            y: origin[1],
                            z: 0,
                        },
                        ..source.as_image_copy()
                    },
                    wgpu::Extent3d {
                        width: *width,
                        height: *height,
                        depth_or_array_layers: 1,
                    },
                ),
                Job::Draw { target, .. } | Job::Effect { target, .. } => {
                    let end=(i+1..self.jobs.len()).find(|&j|!matches!(&self.jobs[j],Job::Draw{target:next,..}|Job::Effect{target:next,..} if next==target)).unwrap_or(self.jobs.len());
                    let load = if i > 0
                        && let Job::Clear(previous, color) = &self.jobs[i - 1]
                        && previous == target
                    {
                        wgpu::LoadOp::Clear(*color)
                    } else {
                        wgpu::LoadOp::Load
                    };
                    let attachments = [Some(attachment(target, load))];
                    let mut pass = encoder.begin_render_pass(&descriptor(&attachments));
                    if self.jobs[i..end]
                        .iter()
                        .any(|j| matches!(j, Job::Effect { .. }))
                    {
                        self.effect_passes += 1;
                    }
                    for (j, job) in self.jobs.iter().enumerate().take(end).skip(i) {
                        let sources = match job {
                            Job::Draw { sources, .. } | Job::Effect { sources, .. } => sources,
                            _ => unreachable!(),
                        };
                        let binding = source_bindings.entry(sources).or_insert_with(|| {
                            r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                label: Some("scene tile inputs"),
                                layout: &self.layout,
                                entries: &[
                                    wgpu::BindGroupEntry {
                                        binding: 0,
                                        resource: wgpu::BindingResource::TextureView(&sources[0]),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 1,
                                        resource: wgpu::BindingResource::TextureView(&sources[1]),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 2,
                                        resource: wgpu::BindingResource::Sampler(&r.sampler),
                                    },
                                ],
                            })
                        });
                        if let Job::Effect {
                            prepared, masks, ..
                        } = job
                        {
                            pass.set_pipeline(&prepared.pipeline);
                            pass.set_bind_group(2, &prepared.binding, &[]);
                            let masks = mask_bindings.entry(masks.as_ref()).or_insert_with(|| {
                                r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                    label: Some("effect tile masks"),
                                    layout: &self.effects.masks,
                                    entries: &masks
                                        .iter()
                                        .enumerate()
                                        .map(|(i, m)| wgpu::BindGroupEntry {
                                            binding: i as u32,
                                            resource: wgpu::BindingResource::TextureView(m),
                                        })
                                        .collect::<Vec<_>>(),
                                })
                            });
                            pass.set_bind_group(3, &*masks, &[]);
                        } else if let Job::Draw { over, .. } = job {
                            pass.set_pipeline(&self.pipeline[usize::from(*over)]);
                        }
                        pass.set_bind_group(0, &self.binding, &[((base + j) * self.stride) as u32]);
                        pass.set_bind_group(1, &*binding, &[]);
                        pass.draw(0..3, 0..1);
                    }
                    encoded_through = end;
                }
                Job::Watercolor {
                    target,
                    binding,
                    record,
                    coordinate,
                } => {
                    let attachments = [Some(attachment(target, wgpu::LoadOp::Load))];
                    let mut pass = encoder.begin_render_pass(&descriptor(&attachments));
                    pass.set_pipeline(&r.pipelines.watercolor_composite);
                    pass.set_bind_group(0, &r.style_bind_group, &[*record * r.style_stride as u32]);
                    pass.set_bind_group(1, &r.target_bind_group, &[r.target_offset(*coordinate)]);
                    pass.set_bind_group(2, binding, &[]);
                    pass.draw(0..3, 0..1);
                }
            }
        }
        Ok(())
    }
}
fn direct_effect_mask(layers: &[Layer], layer: &Layer) -> bool {
    layer.mask.as_ref().is_none_or(|m| {
        !m.enabled || world_offset(layers, layer.id, true) == layer_core::Point::default()
    })
}
fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}
fn uniform_binding(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("scene uniform binding"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer,
                offset: 0,
                size: NonZeroU64::new(96),
            }),
        }],
    })
}
fn local_rect(c: [u32; 2], offset: layer_core::Point, tile: [u32; 2]) -> [f32; 4] {
    [
        (c[0] as f32 - tile[0] as f32) * 256. + offset.x,
        (c[1] as f32 - tile[1] as f32) * 256. + offset.y,
        256.,
        256.,
    ]
}
fn intersects(r: [f32; 4]) -> bool {
    r[0] < 256. && r[1] < 256. && r[0] + r[2] > 0. && r[1] + r[3] > 0.
}
pub(super) fn world_offset(layers: &[Layer], id: LayerId, mask: bool) -> layer_core::Point {
    let Some(layer) = layers.iter().find(|l| l.id == id) else {
        return Default::default();
    };
    let mut offset = if mask {
        layer.mask.as_ref().unwrap().offset
    } else {
        layer.properties.offset
    };
    let mut parent = layer.properties.parent;
    for _ in 0..layers.len() {
        let Some(p) = parent.and_then(|id| layers.iter().find(|l| l.id == id)) else {
            break;
        };
        offset.x += p.properties.offset.x;
        offset.y += p.properties.offset.y;
        parent = p.properties.parent;
    }
    offset
}
fn attachment(
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        resolve_target: None,
        depth_slice: None,
        ops: wgpu::Operations {
            load,
            store: wgpu::StoreOp::Store,
        },
    }
}
fn descriptor<'a>(
    attachments: &'a [Option<wgpu::RenderPassColorAttachment<'a>>],
) -> wgpu::RenderPassDescriptor<'a> {
    wgpu::RenderPassDescriptor {
        label: Some("layer tile scene"),
        color_attachments: attachments,
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    }
}
