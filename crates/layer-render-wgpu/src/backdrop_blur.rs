use crate::pixel_rect::PixelRect;
use layer_core::{Affine, Point};
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
        Self { levels: 3, offset: 3.4 }
    }
}

impl BackdropBlurStyle {
    fn reach(self) -> u32 {
        let levels = self.levels as f32;
        let down = (0.5 * self.offset + 1.) * (levels.exp2() - 4.);
        let up = (self.offset + 1.) * ((levels + 1.).exp2() - 8.);
        (12. + down + up).ceil() as u32
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
const PASS_SIZE: u64 = 56;
const REPROJECTION_OFFSET: u64 = 32;
const SLACK: u32 = 96;
const MOTION_SLACK: u32 = 48;
const REFRESH_FRAMES: u32 = 4;

fn surface_rotation(turns: u32, [width, height]: [f32; 2]) -> Affine {
    match turns {
        1 => Affine([0., 1., -1., 0., height, 0.]),
        2 => Affine([-1., 0., 0., -1., width, height]),
        3 => Affine([0., -1., 1., 0., 0., width]),
        _ => Affine::IDENTITY,
    }
}

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
    cache: Option<(wgpu::Texture, wgpu::BindGroup)>,
    levels: Vec<(wgpu::Texture, wgpu::BindGroup)>,
    extent: [u32; 2],
    regions: Vec<BackdropRegion>,
    placed: Vec<BackdropRegion>,
    drawn: Vec<PixelRect>,
    interiors: Vec<PixelRect>,
    valid: Vec<PixelRect>,
    hold: bool,
    held: Vec<PixelRect>,
    moving: bool,
    document: Option<Affine>,
    reprojection: Affine,
    age: u32,
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
            cache: None,
            levels: Vec::new(),
            extent: [0; 2],
            regions: Vec::new(),
            placed: Vec::new(),
            drawn: Vec::new(),
            interiors: Vec::new(),
            valid: Vec::new(),
            hold: false,
            held: Vec::new(),
            moving: false,
            document: None,
            reprojection: Affine::IDENTITY,
            age: 0,
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
        let style = BackdropBlurStyle { levels: style.levels.clamp(3, 6), ..style };
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

    fn level_size(&self, shift: u32) -> [u32; 2] {
        self.extent.map(|v| v.div_ceil(1 << shift).max(1))
    }

    fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, extent: [u32; 2]) {
        let levels = self.style.levels;
        if self.extent != extent || self.levels.len() + 1 != levels as usize {
            self.extent = extent;
            let format = self.format;
            let texture = |shift: u32| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("backdrop blur level"),
                    size: wgpu::Extent3d {
                        width: extent[0].div_ceil(1 << shift).max(1),
                        height: extent[1].div_ceil(1 << shift).max(1),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
            };
            let cache = texture(2);
            let group = self.bind(device, &cache);
            self.cache = Some((cache, group));
            self.levels = (2..=levels)
                .map(|shift| {
                    let texture = texture(shift);
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
        let mut data = vec![0u8; PASS_STRIDE as usize * (2 * levels as usize - 3)];
        let quarter = self.level_size(2);
        let reprojection = self.reprojection.0;
        let mut write = |index: u32, target: [u32; 2], source: [u32; 2]| {
            let values = [
                1. / target[0] as f32,
                1. / target[1] as f32,
                0.5 / source[0] as f32,
                0.5 / source[1] as f32,
                self.style.offset,
                0.,
                0.25 / quarter[0] as f32,
                0.25 / quarter[1] as f32,
            ]
            .into_iter()
            .chain(reprojection);
            let bytes: Vec<u8> = values.flat_map(f32::to_ne_bytes).collect();
            data[index as usize * PASS_STRIDE as usize..][..bytes.len()].copy_from_slice(&bytes);
        };
        write(0, extent, quarter);
        for shift in 3..=levels {
            write(shift - 2, self.level_size(shift), self.level_size(shift - 1));
        }
        for shift in 2..levels {
            write(levels + shift - 3, self.level_size(shift), self.level_size(shift + 1));
        }
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
        camera: Affine,
        moved: bool,
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
            self.cache = None;
            self.valid.clear();
            self.held.clear();
            self.moving = false;
            self.document = None;
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
        let document = camera.then(surface_rotation(turns, size));
        let settled = damage.is_some() && self.document.is_some_and(|d| d != document);
        if (damage.is_none() && moved && self.reproject(renderer.queue(), document, &bounds)) || (settled && self.hold) {
            self.frames[1] += 1;
            return false;
        }
        let damage = if settled { None } else { damage };
        self.age = 0;
        self.document = Some(document);
        if self.reprojection != Affine::IDENTITY {
            self.reprojection = Affine::IDENTITY;
            self.write_reprojection(renderer.queue());
        }
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
        let slack = if damage.is_none() && self.moving && !settled { MOTION_SLACK } else { SLACK };
        self.moving = damage.is_none();
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
        let whole = self.work_for(&fresh, reach + slack);
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

    fn write_reprojection(&self, queue: &wgpu::Queue) {
        let bytes: Vec<u8> = self.reprojection.0.into_iter().flat_map(f32::to_ne_bytes).collect();
        queue.write_buffer(&self.uniforms, REPROJECTION_OFFSET, &bytes);
    }

    fn reproject(&mut self, queue: &wgpu::Queue, document: Affine, bounds: &[PixelRect]) -> bool {
        let Some(relative) = self
            .document
            .filter(|_| self.age + 1 < REFRESH_FRAMES)
            .and_then(|cached| Some(document.inverse()?.then(cached)))
        else {
            return false;
        };
        let beyond = MOTION_SLACK as f32;
        let [width, height] = self.extent.map(|v| v as f32);
        let covered = bounds.iter().all(|b| {
            let corners = [[b.min_x(), b.min_y()], [b.max_x(), b.min_y()], [b.min_x(), b.max_y()], [b.max_x(), b.max_y()]]
                .map(|[x, y]| relative.map(Point { x: x as f32, y: y as f32 }));
            let (x0, x1) = corners.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
            let (y0, y1) = corners.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
            let low = |edge: u32| if edge == 0 { -beyond } else { edge as f32 };
            let high = |edge: u32, end: f32| if edge as f32 == end { end + beyond } else { edge as f32 };
            self.valid.iter().any(|v| {
                x0 >= low(v.min_x()) && y0 >= low(v.min_y()) && x1 <= high(v.max_x(), width) && y1 <= high(v.max_y(), height)
            })
        });
        if !covered {
            return false;
        }
        self.age += 1;
        self.reprojection = relative;
        self.write_reprojection(queue);
        true
    }

    fn compute(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        work: &[PixelRect],
        viewport: impl Fn(&mut wgpu::RenderPass),
        timestamps: Option<wgpu::RenderPassTimestampWrites>,
    ) {
        let levels = self.style.levels;
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    texture: &wgpu::Texture,
                    rects: &[PixelRect],
                    shift: u32,
                    load: wgpu::LoadOp<wgpu::Color>,
                    timestamps: Option<wgpu::RenderPassTimestampWrites>,
                    draw: &dyn Fn(&mut wgpu::RenderPass)| {
            let size = texture.size();
            let view = texture.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("backdrop blur"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: timestamps,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for rect in rects {
                let area = PixelRect::new(rect.min_x() >> shift, rect.min_y() >> shift,
                    rect.max_x().div_ceil(1 << shift).min(size.width), rect.max_y().div_ceil(1 << shift).min(size.height));
                if !area.is_empty() {
                    pass.set_scissor_rect(area.min_x(), area.min_y(), area.width(), area.height());
                    draw(&mut pass);
                }
            }
        };
        let filter = |pipeline: &wgpu::RenderPipeline, source: &wgpu::BindGroup, uniform: u32, pass: &mut wgpu::RenderPass| {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, source, &[uniform * PASS_STRIDE as u32]);
            pass.draw(0..3, 0..1);
        };
        let level = |shift: u32| &self.levels[shift as usize - 2];
        let clear = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        pass(encoder, &level(2).0, work, 2, clear, timestamps, &viewport);
        for shift in 3..=levels {
            pass(encoder, &level(shift).0, work, shift, clear, None, &|p| filter(&self.down, &level(shift - 1).1, shift - 2, p));
        }
        for shift in (3..levels).rev() {
            pass(encoder, &level(shift).0, work, shift, clear, None, &|p| filter(&self.up, &level(shift + 1).1, levels + shift - 3, p));
        }
        let finished: Vec<PixelRect> = work
            .iter()
            .map(|w| self.interior(*w))
            .filter(|i| !i.is_empty())
            .map(|i| i.expand(4, self.extent))
            .collect();
        let cache = &self.cache.as_ref().unwrap().0;
        pass(encoder, cache, &finished, 2, wgpu::LoadOp::Load, None, &|p| filter(&self.up, &level(3).1, levels - 1, p));
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass, repaint: PixelRect) {
        if self.drawn.is_empty() {
            return;
        }
        pass.set_bind_group(0, &self.cache.as_ref().unwrap().1, &[0]);
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
