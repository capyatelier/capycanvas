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

fn expand(rect: PixelRect, by: u32, extent: [u32; 2]) -> PixelRect {
    PixelRect::new(
        rect.min_x().saturating_sub(by),
        rect.min_y().saturating_sub(by),
        rect.max_x().saturating_add(by).min(extent[0]),
        rect.max_y().saturating_add(by).min(extent[1]),
    )
}

const PASS_STRIDE: u64 = 256;
const PASS_SIZE: u64 = 32;
const SLACK: u32 = 96;

pub struct BackdropBlur {
    format: wgpu::TextureFormat,
    style: BackdropBlurStyle,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    region: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    levels: Vec<(wgpu::Texture, wgpu::BindGroup, wgpu::TextureView)>,
    extent: [u32; 2],
    regions: Vec<BackdropRegion>,
    valid: Vec<PixelRect>,
    region_buffer: Option<wgpu::Buffer>,
    uploaded: bool,
    computed: u64,
    reused: u64,
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
        let region = pipeline(
            "backdrop blur regions",
            "region_vertex",
            "region_fragment",
            &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<BackdropRegion>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4],
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
            format,
            style: BackdropBlurStyle::default(),
            layout,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("backdrop blur"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            down,
            up,
            region,
            uniforms: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("backdrop blur passes"),
                size: PASS_STRIDE * 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            levels: Vec::new(),
            extent: [0; 2],
            regions: Vec::new(),
            valid: Vec::new(),
            region_buffer: None,
            uploaded: false,
            computed: 0,
            reused: 0,
        }
    }

    pub fn set_style(&mut self, style: BackdropBlurStyle) {
        let style = BackdropBlurStyle { levels: style.levels.clamp(1, 6), ..style };
        if self.style != style {
            self.style = style;
            self.levels.clear();
            self.valid.clear();
            self.uploaded = false;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    pub fn set_regions(&mut self, regions: &[BackdropRegion]) {
        if self.regions == regions {
            return;
        }
        self.regions.clear();
        self.regions.extend_from_slice(regions);
        self.uploaded = false;
    }

    pub fn copy_regions(&mut self, other: &Self) {
        self.set_style(other.style);
        self.set_regions(&other.regions);
    }

    pub fn frames(&self) -> [u64; 2] {
        [self.computed, self.reused]
    }

    fn bind(&self, device: &wgpu::Device, view: &wgpu::TextureView) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("backdrop blur"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
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
        self.regions
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

    fn work_for(&self, dirty: &[PixelRect]) -> Vec<PixelRect> {
        let margin = self.style.reach() + SLACK;
        let mut work: Vec<PixelRect> = Vec::new();
        for rect in dirty {
            let mut area = expand(*rect, margin, self.extent);
            while let Some(i) = work.iter().position(|o| {
                !o.intersect(area).is_empty() && o.union(area).area() <= o.area() + area.area()
            }) {
                area = area.union(work.swap_remove(i));
            }
            work.push(area);
        }
        work
    }

    #[cfg(test)]
    fn work(&self) -> Vec<PixelRect> {
        self.work_for(&self.bounds())
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

    fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, extent: [u32; 2]) {
        if self.extent != extent || self.levels.len() != self.style.levels as usize {
            self.extent = extent;
            self.levels = (1..=self.style.levels)
                .map(|level| {
                    let texture = device.create_texture(&wgpu::TextureDescriptor {
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
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    });
                    let view = texture.create_view(&Default::default());
                    let group = self.bind(device, &view);
                    (texture, group, view)
                })
                .collect();
            self.valid.clear();
            self.uploaded = false;
        }
        if self.uploaded {
            return;
        }
        let size = |level: usize| -> [f32; 2] {
            if level == 0 {
                [extent[0] as f32, extent[1] as f32]
            } else {
                let s = self.levels[level - 1].0.size();
                [s.width as f32, s.height as f32]
            }
        };
        let levels = self.style.levels as usize;
        let mut data = vec![0u8; PASS_STRIDE as usize * (2 * levels)];
        let mut write = |index: usize, target: usize, source: usize| {
            let [tw, th] = size(target);
            let [sw, sh] = size(source);
            let values = [1. / tw, 1. / th, 0.5 / sw, 0.5 / sh, self.style.offset, 0., 0., 0.];
            let bytes: Vec<u8> = values.iter().flat_map(|v: &f32| v.to_ne_bytes()).collect();
            data[index * PASS_STRIDE as usize..][..bytes.len()].copy_from_slice(&bytes);
        };
        for level in 1..=levels {
            write(level - 1, level, level - 1);
        }
        for level in (1..levels).rev() {
            write(levels + level - 1, level, level + 1);
        }
        write(2 * levels - 1, 0, 1);
        queue.write_buffer(&self.uniforms, 0, &data);
        if !self.regions.is_empty() {
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    self.regions.as_ptr().cast::<u8>(),
                    std::mem::size_of_val(self.regions.as_slice()),
                )
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

    pub fn encode(
        &mut self,
        renderer: &crate::WgpuRasterizer,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        target: &wgpu::TextureView,
        extent: [u32; 2],
        damage: Option<&[[u32; 4]]>,
    ) {
        if self.regions.is_empty() || extent[0] == 0 || extent[1] == 0 {
            self.valid.clear();
            return;
        }
        let device = renderer.device();
        self.prepare(device, renderer.queue(), extent);
        let dirty = self.dirty(damage);
        if dirty.is_empty() {
            self.reused += 1;
        } else {
            self.compute(device, encoder, source, dirty);
            self.computed += 1;
        }
        let levels = self.style.levels as usize;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("backdrop regions"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.region);
        pass.set_bind_group(0, &self.levels[0].1, &[((2 * levels - 1) as u64 * PASS_STRIDE) as u32]);
        pass.set_vertex_buffer(0, self.region_buffer.as_ref().unwrap().slice(..));
        pass.draw(0..6, 0..self.regions.len() as u32);
    }

    fn dirty(&mut self, damage: Option<&[[u32; 4]]>) -> Vec<PixelRect> {
        let bounds = self.bounds();
        let Some(damage) = damage else {
            self.valid.clear();
            return bounds;
        };
        let (reach, extent) = (self.style.reach(), self.extent);
        let touched = |area: PixelRect| {
            let near = expand(area, reach, extent);
            damage.iter().any(|&[x0, y0, x1, y1]| !near.intersect(PixelRect::new(x0, y0, x1, y1)).is_empty())
        };
        let contains = |outer: PixelRect, inner: PixelRect| outer.intersect(inner) == inner;
        let mut valid = Vec::with_capacity(self.valid.len());
        for v in &self.valid {
            if touched(*v) {
                valid.extend(bounds.iter().filter(|b| contains(*v, **b) && !touched(**b)).copied());
            } else {
                valid.push(*v);
            }
        }
        self.valid = valid;
        bounds
            .into_iter()
            .filter(|b| touched(*b) || !self.valid.iter().any(|v| contains(*v, *b)))
            .collect()
    }

    fn compute(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        mut dirty: Vec<PixelRect>,
    ) {
        let contains = |outer: PixelRect, inner: PixelRect| outer.intersect(inner) == inner;
        let bounds = self.bounds();
        let work = loop {
            let work = self.work_for(&dirty);
            let kept: Vec<PixelRect> = self
                .valid
                .iter()
                .filter(|v| work.iter().all(|w| w.intersect(**v).is_empty()))
                .copied()
                .chain(work.iter().map(|w| self.interior(*w)))
                .collect();
            let orphans: Vec<PixelRect> = bounds
                .iter()
                .filter(|b| !dirty.contains(b) && !kept.iter().any(|v| contains(*v, **b)))
                .copied()
                .collect();
            if orphans.is_empty() {
                self.valid = kept.into_iter().filter(|v| !v.is_empty()).collect();
                break work;
            }
            dirty.extend(orphans);
        };
        let levels = self.style.levels as usize;
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    pipeline: &wgpu::RenderPipeline,
                    group: &wgpu::BindGroup,
                    uniform: usize,
                    target: usize| {
            let (texture, _, view) = &self.levels[target - 1];
            let shift = target as u32;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("backdrop blur"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[(uniform as u64 * PASS_STRIDE) as u32]);
            let size = texture.size();
            for rect in &work {
                let area = PixelRect::new(
                    rect.min_x() >> shift,
                    rect.min_y() >> shift,
                    rect.max_x().div_ceil(1 << shift).min(size.width),
                    rect.max_y().div_ceil(1 << shift).min(size.height),
                );
                if area.is_empty() {
                    continue;
                }
                pass.set_scissor_rect(area.min_x(), area.min_y(), area.width(), area.height());
                pass.draw(0..3, 0..1);
            }
        };
        let source_group = self.bind(device, source);
        for level in 1..=levels {
            let group = if level == 1 { &source_group } else { &self.levels[level - 2].1 };
            pass(encoder, &self.down, group, level - 1, level);
        }
        for level in (1..levels).rev() {
            pass(encoder, &self.up, &self.levels[level].1, levels + level - 1, level);
        }
    }
}
