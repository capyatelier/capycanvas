use crate::pixel_rect::PixelRect;
#[cfg(test)]
#[path = "backdrop_blur_tests.rs"]
mod tests;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BackdropRegion {
    pub bounds: [f32; 4],
    pub radii: [f32; 4],
    pub shape: [f32; 4],
}

impl BackdropRegion {
    pub const CIRCULAR: [f32; 4] = [2., 0., 0., 0.];
    pub const SQUIRCLE: [f32; 4] = [4., 0., 0., 0.];

    pub fn rounded(bounds: [f32; 4], radii: [f32; 4], shape: [f32; 4]) -> Self {
        let [_, _, w, h] = bounds;
        let [tl, tr, br, bl] = radii;
        let factor = [w / (tl + tr), w / (bl + br), h / (tl + bl), h / (tr + br)]
            .into_iter()
            .filter(|f| f.is_finite())
            .fold(1f32, f32::min);
        Self { bounds, radii: radii.map(|r| r * factor), shape }
    }

    fn rotated(self, turns: u32, [width, height]: [f32; 2]) -> Self {
        let [x, y, w, h] = self.bounds;
        let bounds = match turns {
            1 => [height - y - h, x, h, w],
            2 => [width - x - w, height - y - h, w, h],
            3 => [y, width - x - w, h, w],
            _ => self.bounds,
        };
        let radii = std::array::from_fn(|i| self.radii[(i + 4 - turns as usize) % 4]);
        Self { bounds, radii, ..self }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Edge {
    region: BackdropRegion,
    area: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BackdropBlurStyle {
    pub levels: u32,
    pub offset: f32,
}

impl Default for BackdropBlurStyle {
    fn default() -> Self {
        Self { levels: 3, offset: 3. }
    }
}

impl BackdropBlurStyle {
    fn reach(self) -> u32 {
        let levels = self.levels as f32;
        let down = (0.5 * self.offset + 1.) * (levels.exp2() - 1.);
        let up = (self.offset + 1.) * ((levels + 1.).exp2() - 2.);
        (down + up).ceil() as u32
    }
}

fn contains(outer: PixelRect, inner: PixelRect) -> bool {
    outer.intersect(inner) == inner
}

fn merge(rects: &mut Vec<PixelRect>, mut rect: PixelRect) {
    while let Some(i) = rects.iter().position(|r| !r.intersect(rect).is_empty()) {
        rect = rect.union(rects.swap_remove(i));
    }
    rects.push(rect);
}

const PASS_STRIDE: u64 = 256;
const PASS_SIZE: u64 = 32;
const SLACK: u32 = 96;

pub(crate) struct BackdropBlur {
    style: BackdropBlurStyle,
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    fill: wgpu::RenderPipeline,
    region: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    scratch: Option<wgpu::Texture>,
    levels: Vec<(wgpu::Texture, wgpu::BindGroup)>,
    extent: [u32; 2],
    regions: Vec<BackdropRegion>,
    placed: Vec<BackdropRegion>,
    drawn: Vec<PixelRect>,
    interiors: Vec<PixelRect>,
    valid: Vec<PixelRect>,
    hold: bool,
    held: Vec<PixelRect>,
    edges: u32,
    region_buffer: Option<wgpu::Buffer>,
    uploaded: bool,
    frames: [u64; 2],
}

impl BackdropBlur {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("backdrop blur"),
            source: wgpu::ShaderSource::Wgsl(include_str!("backdrop_blur.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("backdrop blur"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(PASS_SIZE),
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("backdrop blur"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |label, vertex, fragment, buffers: &[Option<wgpu::VertexBufferLayout>], blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex),
                    compilation_options: Default::default(),
                    buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let down = pipeline("backdrop blur down", "fullscreen", "down", &[], None);
        let up = pipeline("backdrop blur up", "fullscreen", "up", &[], None);
        let fill = pipeline("backdrop glass interiors", "fullscreen", "fill", &[], None);
        let region = pipeline(
            "backdrop blur regions",
            "region_vertex",
            "region_fragment",
            &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Edge>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4],
            })],
            Some(wgpu::BlendState {
                color: wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING.color,
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Zero,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
            }),
        );
        Self {
            style: BackdropBlurStyle::default(),
            format,
            layout,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("backdrop blur"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            down,
            up,
            fill,
            region,
            uniforms: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("backdrop blur passes"),
                size: PASS_STRIDE * 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            scratch: None,
            levels: Vec::new(),
            extent: [0; 2],
            regions: Vec::new(),
            placed: Vec::new(),
            drawn: Vec::new(),
            interiors: Vec::new(),
            valid: Vec::new(),
            hold: false,
            held: Vec::new(),
            edges: 0,
            region_buffer: None,
            uploaded: false,
            frames: [0; 2],
        }
    }

    pub fn style(&self) -> BackdropBlurStyle {
        self.style
    }

    pub fn regions(&self) -> &[BackdropRegion] {
        &self.regions
    }

    pub fn set_style(&mut self, style: BackdropBlurStyle) {
        let style = BackdropBlurStyle { levels: style.levels.clamp(2, 6), ..style };
        if self.style != style {
            self.style = style;
            self.levels.clear();
            self.valid.clear();
            self.uploaded = false;
        }
    }

    pub fn set_regions(&mut self, regions: &[BackdropRegion]) {
        if self.regions != regions {
            self.regions.clear();
            self.regions.extend_from_slice(regions);
        }
    }

    pub fn set_hold(&mut self, hold: bool) {
        self.hold = hold;
    }

    pub fn interiors(&self) -> &[PixelRect] {
        &self.interiors
    }

    pub fn frames(&self) -> [u64; 2] {
        self.frames
    }

    fn bind(&self, device: &wgpu::Device, texture: &wgpu::Texture) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("backdrop blur"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture.create_view(&Default::default())),
                },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.uniforms,
                        offset: 0,
                        size: wgpu::BufferSize::new(PASS_SIZE),
                    }),
                },
            ],
        })
    }

    fn bounds(&self) -> Vec<PixelRect> {
        self.placed
            .iter()
            .map(|r| {
                let [x, y, w, h] = r.bounds;
                let clamp = |v: f32, max: u32| v.clamp(0., max as f32) as u32;
                PixelRect::new(
                    clamp(x.floor(), self.extent[0]),
                    clamp(y.floor(), self.extent[1]),
                    clamp((x + w).ceil(), self.extent[0]),
                    clamp((y + h).ceil(), self.extent[1]),
                )
            })
            .filter(|b| !b.is_empty())
            .collect()
    }

    fn work_for(&self, dirty: &[PixelRect], margin: u32) -> Vec<PixelRect> {
        let mut work: Vec<PixelRect> = Vec::new();
        for rect in dirty {
            let mut area = rect.expand(margin, self.extent);
            while let Some(i) = work.iter().position(|o| {
                !o.intersect(area).is_empty() && o.union(area).area() <= o.area() + area.area()
            }) {
                area = area.union(work.swap_remove(i));
            }
            work.push(area);
        }
        work
    }

    fn interior(&self, work: PixelRect) -> PixelRect {
        let reach = self.style.reach();
        let [width, height] = self.extent;
        PixelRect::new(
            if work.min_x() == 0 { 0 } else { work.min_x() + reach },
            if work.min_y() == 0 { 0 } else { work.min_y() + reach },
            if work.max_x() == width { width } else { work.max_x().saturating_sub(reach) },
            if work.max_y() == height { height } else { work.max_y().saturating_sub(reach) },
        )
    }

    fn level_size(&self, level: usize) -> [u32; 2] {
        if level == 0 {
            self.extent
        } else {
            let size = self.levels[level - 1].0.size();
            [size.width, size.height]
        }
    }

    fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, extent: [u32; 2]) {
        if self.extent != extent || self.levels.len() != self.style.levels as usize {
            self.extent = extent;
            let texture = |level: u32| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("backdrop blur level"),
                    size: wgpu::Extent3d {
                        width: extent[0].div_ceil(1 << level).max(1),
                        height: extent[1].div_ceil(1 << level).max(1),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC
                        | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                })
            };
            self.scratch = Some(texture(1));
            self.levels = (1..=self.style.levels)
                .map(|level| {
                    let texture = texture(level);
                    let group = self.bind(device, &texture);
                    (texture, group)
                })
                .collect();
            self.valid.clear();
            self.uploaded = false;
        }
        if self.uploaded {
            return;
        }
        let levels = self.style.levels as usize;
        let mut data = vec![0u8; PASS_STRIDE as usize * (2 * levels)];
        let mut write = |index: usize, target: [u32; 2], source: [u32; 2]| {
            let values = [
                1. / target[0] as f32,
                1. / target[1] as f32,
                0.5 / source[0] as f32,
                0.5 / source[1] as f32,
                self.style.offset,
                0.,
                0.,
                0.,
            ];
            let bytes: Vec<u8> = values.iter().flat_map(|v: &f32| v.to_ne_bytes()).collect();
            data[index * PASS_STRIDE as usize..][..bytes.len()].copy_from_slice(&bytes);
        };
        for level in 3..=levels {
            write(level - 1, self.level_size(level), self.level_size(level - 1));
        }
        for level in (1..levels).rev() {
            write(levels + level - 1, self.level_size(level), self.level_size(level + 1));
        }
        write(0, self.level_size(0), self.level_size(1));
        queue.write_buffer(&self.uniforms, 0, &data);
        self.interiors.clear();
        let mut edges = Vec::with_capacity(8 * self.placed.len());
        for region in &self.placed {
            let [x, y, w, h] = region.bounds;
            let round = region.radii.iter().fold(0f32, |a, b| a.max(*b)).ceil() + 1.;
            let lines = |start: f32, size: f32, max: u32| {
                let clamp = |v: f32| v.clamp(0., max as f32);
                [start - 1., (start + 2.).ceil(), (start + round).ceil(), (start + size - round).floor(),
                    (start + size - 2.).floor(), start + size + 1.].map(clamp)
            };
            let [xs, ys] = [lines(x, w, extent[0]), lines(y, h, extent[1])];
            if region.radii.iter().any(|r| *r < 0.) || xs.windows(2).chain(ys.windows(2)).any(|p| p[0] > p[1]) {
                edges.push(Edge { region: *region, area: [xs[0], ys[0], xs[5] - xs[0], ys[5] - ys[0]] });
                continue;
            }
            for row in 0..5 {
                let covered = |column: usize| (column == 2 && (1..4).contains(&row)) || (row == 2 && (1..4).contains(&column));
                let mut column = 0;
                while column < 5 {
                    let run = (column..5).take_while(|c| covered(*c) == covered(column)).count();
                    let [x0, x1, y0, y1] = [xs[column], xs[column + run], ys[row], ys[row + 1]];
                    if covered(column) {
                        self.interiors.push(PixelRect::new(x0 as u32, y0 as u32, x1 as u32, y1 as u32));
                    } else if x1 > x0 && y1 > y0 {
                        edges.push(Edge { region: *region, area: [x0, y0, x1 - x0, y1 - y0] });
                    }
                    column += run;
                }
            }
        }
        self.interiors.retain(|r| !r.is_empty());
        self.edges = edges.len() as u32;
        if !edges.is_empty() {
            let bytes = unsafe {
                std::slice::from_raw_parts(edges.as_ptr().cast::<u8>(), std::mem::size_of_val(edges.as_slice()))
            };
            if self.region_buffer.as_ref().is_none_or(|b| b.size() < bytes.len() as u64) {
                self.region_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("backdrop regions"),
                    size: (bytes.len() as u64).next_power_of_two(),
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            queue.write_buffer(self.region_buffer.as_ref().unwrap(), 0, bytes);
        }
        self.uploaded = true;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &mut self,
        renderer: &crate::WgpuRasterizer,
        encoder: &mut wgpu::CommandEncoder,
        logical: [u32; 2],
        turns: u32,
        damage: Option<&[PixelRect]>,
        viewport: impl Fn(&mut wgpu::RenderPass),
        timestamps: Option<wgpu::RenderPassTimestampWrites>,
        repaint: &mut Vec<PixelRect>,
    ) -> bool {
        let extent = if turns % 2 == 0 { logical } else { [logical[1], logical[0]] };
        let size = logical.map(|v| v as f32);
        let placed: Vec<BackdropRegion> = self.regions.iter().map(|r| r.rotated(turns, size)).collect();
        if placed != self.placed {
            self.placed = placed;
            self.uploaded = false;
        }
        if self.placed.is_empty() || extent[0] == 0 || extent[1] == 0 {
            repaint.append(&mut self.drawn);
            self.interiors.clear();
            self.levels.clear();
            self.valid.clear();
            self.held.clear();
            self.extent = [0; 2];
            return false;
        }
        self.prepare(renderer.device(), renderer.queue(), extent);
        let bounds = self.bounds();
        if self.drawn != bounds {
            repaint.append(&mut self.drawn);
            repaint.extend_from_slice(&bounds);
            self.drawn.clone_from(&bounds);
        }
        let reach = self.style.reach();
        let damage = match damage {
            None => {
                self.held.clear();
                None
            }
            Some(damage) if self.hold => {
                for rect in damage {
                    merge(&mut self.held, *rect);
                }
                Some(Vec::new())
            }
            Some(damage) => {
                let mut all = std::mem::take(&mut self.held);
                all.extend_from_slice(damage);
                Some(all)
            }
        };
        let old = if damage.is_some() { std::mem::take(&mut self.valid) } else { Vec::new() };
        let cached_before = |b: &PixelRect| old.iter().any(|v| contains(*v, *b));
        let (local, stale): (Vec<PixelRect>, Vec<PixelRect>) = damage.into_iter().flatten().partition(|d| {
            bounds.iter().any(|b| cached_before(b) && !d.expand(reach, extent).intersect(*b).is_empty())
        });
        let affected: Vec<PixelRect> = local
            .iter()
            .flat_map(|d| bounds.iter().map(move |b| d.expand(reach, extent).intersect(b.expand(2, extent))))
            .filter(|a| !a.is_empty())
            .collect();
        let stale: Vec<PixelRect> = stale.iter().map(|d| d.expand(reach, extent)).collect();
        let mut valid = Vec::with_capacity(old.len());
        for v in &old {
            if stale.iter().any(|d| !d.intersect(*v).is_empty()) {
                valid.extend(bounds.iter().filter(|b| contains(*v, **b)).copied());
            } else {
                valid.push(*v);
            }
        }
        let (cached, fresh): (Vec<PixelRect>, Vec<PixelRect>) = bounds.iter().partition(|b| valid.iter().any(|v| contains(*v, **b)));
        let changed: Vec<PixelRect> = local.iter().map(|d| d.expand(reach, extent)).collect();
        let whole = self.work_for(&fresh, reach + SLACK);
        valid.extend(whole.iter().map(|w| self.interior(*w)).filter(|v| !v.is_empty()));
        valid.dedup();
        self.valid = valid;
        let work: Vec<PixelRect> = whole.into_iter().chain(self.work_for(&affected, reach)).collect();
        if work.is_empty() {
            self.frames[1] += 1;
            return false;
        }
        self.frames[0] += 1;
        self.compute(encoder, &work, viewport, timestamps.clone());
        for b in fresh {
            if !repaint.contains(&b) {
                repaint.push(b);
            }
        }
        for b in cached {
            repaint.extend(changed.iter().map(|c| c.intersect(b)).filter(|r| !r.is_empty()));
        }
        timestamps.is_some()
    }

    fn compute(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        work: &[PixelRect],
        viewport: impl Fn(&mut wgpu::RenderPass),
        timestamps: Option<wgpu::RenderPassTimestampWrites>,
    ) {
        let levels = self.style.levels as usize;
        let pass = |encoder: &mut wgpu::CommandEncoder, texture: &wgpu::Texture, shift: u32, timestamps: Option<wgpu::RenderPassTimestampWrites>, draw: &dyn Fn(&mut wgpu::RenderPass)| {
            let size = texture.size();
            let areas: Vec<PixelRect> = work
                .iter()
                .map(|rect| PixelRect::new(rect.min_x() >> shift, rect.min_y() >> shift,
                    rect.max_x().div_ceil(1 << shift).min(size.width), rect.max_y().div_ceil(1 << shift).min(size.height)))
                .filter(|a| !a.is_empty())
                .collect();
            let view = texture.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("backdrop blur"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: timestamps,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for area in areas {
                pass.set_scissor_rect(area.min_x(), area.min_y(), area.width(), area.height());
                draw(&mut pass);
            }
        };
        let filter = |pipeline: &wgpu::RenderPipeline, source: &wgpu::BindGroup, uniform: usize, pass: &mut wgpu::RenderPass| {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, source, &[(uniform as u64 * PASS_STRIDE) as u32]);
            pass.draw(0..3, 0..1);
        };
        let scratch = self.scratch.as_ref().unwrap();
        pass(encoder, &self.levels[1].0, 2, timestamps, &viewport);
        for level in 3..=levels {
            pass(encoder, &self.levels[level - 1].0, level as u32, None, &|p| filter(&self.down, &self.levels[level - 2].1, level - 1, p));
        }
        for level in (1..levels).rev() {
            let into = if level == 1 { scratch } else { &self.levels[level - 1].0 };
            pass(encoder, into, level as u32, None, &|p| filter(&self.up, &self.levels[level].1, levels + level - 1, p));
        }
        for interior in work.iter().map(|w| self.interior(*w)) {
            let area = PixelRect::new(interior.min_x().div_ceil(2), interior.min_y().div_ceil(2), interior.max_x() >> 1, interior.max_y() >> 1);
            if area.is_empty() {
                continue;
            }
            let origin = wgpu::Origin3d { x: area.min_x(), y: area.min_y(), z: 0 };
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo { texture: scratch, mip_level: 0, origin, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyTextureInfo { texture: &self.levels[0].0, mip_level: 0, origin, aspect: wgpu::TextureAspect::All },
                wgpu::Extent3d { width: area.width(), height: area.height(), depth_or_array_layers: 1 },
            );
        }
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass, repaint: PixelRect) {
        if self.drawn.is_empty() {
            return;
        }
        pass.set_bind_group(0, &self.levels[0].1, &[0]);
        pass.set_pipeline(&self.fill);
        for area in self.interiors.iter().map(|i| i.intersect(repaint)).filter(|a| !a.is_empty()) {
            pass.set_scissor_rect(area.min_x(), area.min_y(), area.width(), area.height());
            pass.draw(0..3, 0..1);
        }
        pass.set_scissor_rect(repaint.min_x(), repaint.min_y(), repaint.width(), repaint.height());
        if let Some(buffer) = self.region_buffer.as_ref().filter(|_| self.edges > 0) {
            pass.set_pipeline(&self.region);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..6, 0..self.edges);
        }
    }
}
