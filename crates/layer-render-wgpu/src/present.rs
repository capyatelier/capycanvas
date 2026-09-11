//! GPU-only viewport presentation shared by toolkit surfaces and WebGPU.

use crate::{Uploads, WgpuRasterizer};
use layer_render::{CursorSegment, ViewState};

/// A native UI's document overview, rendered into the existing canvas surface.
/// Bounds and work-area corners use physical surface pixels. The host leaves
/// this image rectangle transparent; native controls still paint above it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverviewPlacement {
    pub bounds: [f32; 4],
    pub work_area: [[f32; 2]; 4],
    pub outline_linear: [f32; 3],
    pub background_linear: [f32; 3],
    pub scale: f32,
    pub opacity: f32,
}

impl OverviewPlacement {
    fn packed(self) -> Option<[f32; 20]> {
        let [x, y, w, h] = self.bounds;
        let [[ax, ay], [bx, by], [cx, cy], [dx, dy]] = self.work_area;
        let [r, g, b] = self.outline_linear;
        let [br, bg, bb] = self.background_linear;
        let opacity = self.opacity.clamp(0., 1.);
        let data = [
            x, y, w, h, ax, ay, bx, by, cx, cy, dx, dy, r, g, b, opacity, br, bg, bb, self.scale,
        ];
        (w > 0.
            && h > 0.
            && self.scale > 0.
            && self.opacity > 0.
            && data.iter().all(|v| v.is_finite()))
        .then_some(data)
    }
}

pub struct ViewportPresenter {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    bind_group: Option<wgpu::BindGroup>,
    selection_buffer: Option<wgpu::Buffer>,
    document_extent: [u32; 2],
    encode_srgb: bool,
    corner_radius: f32,
    cursor_pipeline: wgpu::RenderPipeline,
    cursor_buffer: wgpu::Buffer,
    cursor_vertices: Vec<CursorSegment>,
    uploads: Uploads,
    camera_data: Option<[f32; 24]>,
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    overview_pipeline: Option<wgpu::RenderPipeline>,
    overview_buffer: Option<wgpu::Buffer>,
    overviews: Vec<[f32; 20]>,
    overviews_changed: bool,
}

impl ViewportPresenter {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self::with_device(&device.clone().into(), format)
    }

    /// Shares the renderer's optional startup cache with presentation shaders.
    pub fn for_renderer(renderer: &WgpuRasterizer, format: wgpu::TextureFormat) -> Self {
        Self::with_device(&renderer.device, format)
    }

    fn with_device(device: &crate::PipelineDevice, format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewport bindings"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewport shader"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("overview_sample.wgsl"),
                    "\n",
                    include_str!("present.wgsl")
                )
                .into(),
            ),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewport presentation"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let cursor_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("display-only cursor"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("cursor_vertex"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<CursorSegment>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32, 3 => Float32, 4 => Float32],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("cursor_fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let cursor_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cursor segments"),
            size: 256 * std::mem::size_of::<CursorSegment>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport camera"),
            size: 96,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            layout,
            uniform,
            bind_group: None,
            selection_buffer: None,
            document_extent: [0; 2],
            encode_srgb: !format.is_srgb(),
            corner_radius: 0.0,
            cursor_pipeline,
            cursor_buffer,
            cursor_vertices: Vec::with_capacity(256),
            uploads: Uploads::new(device, 16 * 1024),
            camera_data: None,
            shader,
            pipeline_layout,
            format,
            overview_pipeline: None,
            overview_buffer: None,
            overviews: Vec::new(),
            overviews_changed: false,
        }
    }

    /// Opt-in startup preparation. Hosts without in-surface overviews do not
    /// compile this pipeline or allocate overview buffers.
    pub fn prepare_overviews(&mut self, renderer: &WgpuRasterizer) {
        if self.overview_pipeline.is_some() {
            return;
        }
        let device = &renderer.device;
        self.overview_pipeline = Some(device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("in-surface document overviews"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("overview_vertex"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 80,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("overview_fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING.color,
                        // The canvas already owns window coverage. Reapplying
                        // alpha blending here would thicken antialiased corners.
                        alpha: wgpu::BlendComponent { src_factor:wgpu::BlendFactor::Zero, dst_factor:wgpu::BlendFactor::One, operation:wgpu::BlendOperation::Add },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        }));
    }

    /// Reuses the composition, bindings and current presentation pass. An
    /// unchanged placement uploads nothing; painting never exports preview pixels.
    pub fn set_overviews(&mut self, renderer: &WgpuRasterizer, placements: &[OverviewPlacement]) {
        let data = placements.iter().filter_map(|p| p.packed());
        if self.overviews.iter().copied().eq(data.clone()) {
            return;
        }
        self.overviews.clear();
        self.overviews.extend(data);
        self.overviews_changed = true;
        if !self.overviews.is_empty() {
            self.prepare_overviews(renderer);
        }
        let device = &renderer.device;
        let size = std::mem::size_of_val(self.overviews.as_slice()) as u64;
        if size > 0
            && self
                .overview_buffer
                .as_ref()
                .is_none_or(|b| b.size() < size)
        {
            self.overview_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("overview placements"),
                size: size.next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
    }

    /// Native child surfaces do not inherit the parent window's rounded clip.
    /// Zero (the default on web) keeps an opaque rectangular viewport.
    pub fn set_corner_radius(&mut self, physical_pixels: f32) {
        self.corner_radius = physical_pixels.max(0.0);
    }

    pub fn set_cursor(&mut self, device: &wgpu::Device, segments: &[CursorSegment], scale: f32) {
        self.cursor_vertices.clear();
        if segments.is_empty() {
            return;
        }
        self.cursor_vertices
            .extend(segments.iter().map(|s| CursorSegment {
                from: s.from.map(|v| v * scale),
                to: s.to.map(|v| v * scale),
                distance: s.distance * scale,
                marker: s.marker,
                scale,
            }));
        let size = std::mem::size_of_val(self.cursor_vertices.as_slice()) as u64;
        if self.cursor_buffer.size() < size {
            self.cursor_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("cursor segments"),
                size: size.next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
    }

    /// The target is a toolkit-owned framebuffer or acquired surface texture.
    /// No pixel readback, GPU completion wait, or document re-rasterization.
    pub fn present(
        &mut self,
        renderer: &WgpuRasterizer,
        target: &wgpu::TextureView,
        view: ViewState,
        surround_linear: [f32; 4],
    ) {
        let mut encoder = renderer
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("viewport presentation"),
            });
        self.encode(renderer, &mut encoder, target, view, surround_linear);
        renderer.queue.submit([encoder.finish()]);
    }

    /// Encode into the host's submission, allowing native GPU interop barriers
    /// and completion signals to surround the same viewport pass.
    pub fn encode(
        &mut self,
        renderer: &WgpuRasterizer,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        view: ViewState,
        surround_linear: [f32; 4],
    ) {
        let Some(composite) = renderer.composite_view.as_ref() else {
            return;
        };
        let device = &renderer.device;
        let selection = renderer.display_selection.as_ref();
        let coverage = selection.map_or(&renderer.unclipped, |(_, buffer)| buffer);
        if self.bind_group.is_none()
            || self.document_extent != renderer.document_extent
            || self.selection_buffer.as_ref() != Some(coverage)
        {
            self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("viewport composite"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(composite),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&renderer.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: coverage.as_entire_binding(),
                    },
                ],
            }));
            self.document_extent = renderer.document_extent;
            self.selection_buffer = Some(coverage.clone());
        }
        let [a, b, c, d, tx, ty] = view.document_to_surface;
        let det = a * d - b * c;
        if !det.is_finite() || det.abs() < 1.0e-12 {
            return;
        }
        let inverse = selection
            .map_or(layer_core::Affine::IDENTITY, |(s, _)| {
                s.affine.inverse().expect("selection placement validated")
            })
            .0;
        let data: [f32; 24] = [
            d / det,
            -b / det,
            -c / det,
            a / det,
            (c * ty - d * tx) / det,
            (b * tx - a * ty) / det,
            self.document_extent[0] as f32,
            self.document_extent[1] as f32,
            view.width_px as f32,
            view.height_px as f32,
            f32::from(self.encode_srgb),
            self.corner_radius,
            surround_linear[0],
            surround_linear[1],
            surround_linear[2],
            surround_linear[3],
            inverse[4],
            inverse[5],
            f32::from(selection.is_some()),
            selection.map_or(0., |(s, _)| f32::from(s.inverted)),
            inverse[0],
            inverse[1],
            inverse[2],
            inverse[3],
        ];
        // A fixed f32 array has no padding or uninitialized bytes.
        let bytes = unsafe {
            std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), std::mem::size_of_val(&data))
        };
        if self.camera_data != Some(data) {
            self.uploads
                .write(encoder, &renderer.queue, &self.uniform, bytes);
            self.camera_data = Some(data);
        }
        if !self.cursor_vertices.is_empty() {
            // repr(C) contains only initialized f32s, without padding.
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    self.cursor_vertices.as_ptr().cast::<u8>(),
                    std::mem::size_of_val(self.cursor_vertices.as_slice()),
                )
            };
            self.uploads
                .write(encoder, &renderer.queue, &self.cursor_buffer, bytes);
        }
        if self.overviews_changed && !self.overviews.is_empty() {
            // Fixed initialized f32 arrays, with no struct padding.
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    self.overviews.as_ptr().cast::<u8>(),
                    std::mem::size_of_val(self.overviews.as_slice()),
                )
            };
            self.uploads.write(
                encoder,
                &renderer.queue,
                self.overview_buffer.as_ref().unwrap(),
                bytes,
            );
            self.overviews_changed = false;
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, self.bind_group.as_ref().unwrap(), &[]);
            pass.draw(0..3, 0..1);
            if !self.overviews.is_empty() {
                pass.set_pipeline(self.overview_pipeline.as_ref().unwrap());
                pass.set_vertex_buffer(0, self.overview_buffer.as_ref().unwrap().slice(..));
                pass.draw(0..6, 0..self.overviews.len() as u32);
            }
            if !self.cursor_vertices.is_empty() {
                pass.set_pipeline(&self.cursor_pipeline);
                pass.set_vertex_buffer(0, self.cursor_buffer.slice(..));
                pass.draw(0..6, 0..self.cursor_vertices.len() as u32);
            }
        }
        // Return upload chunks only after this encoder's GPU work completes.
        // Works both for present() and hosts submitting encode() themselves.
        self.uploads.finish(encoder);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overview_records_reject_invalid_geometry() {
        let p = OverviewPlacement {
            bounds: [0., 0., 100., 75.],
            work_area: [[0.; 2]; 4],
            outline_linear: [0.; 3],
            background_linear: [0.; 3],
            scale: 1.,
            opacity: 1.,
        };
        assert!(p.packed().is_some());
        assert!(
            OverviewPlacement {
                bounds: [0., 0., 0., 75.],
                ..p
            }
            .packed()
            .is_none()
        );
        assert!(
            OverviewPlacement {
                scale: f32::NAN,
                ..p
            }
            .packed()
            .is_none()
        );
        assert!(
            OverviewPlacement {
                work_area: [[f32::INFINITY, 0.]; 4],
                ..p
            }
            .packed()
            .is_none()
        );
    }

    #[test]
    fn overview_resources_are_opt_in_and_reused() {
        let r = WgpuRasterizer::new_headless().unwrap();
        let mut presenter = ViewportPresenter::for_renderer(&r, wgpu::TextureFormat::Rgba8Unorm);
        assert!(presenter.overview_pipeline.is_none());
        assert!(presenter.overview_buffer.is_none());
        let mut p = OverviewPlacement {
            bounds: [0., 0., 100., 75.],
            work_area: [[0.; 2]; 4],
            outline_linear: [0.; 3],
            background_linear: [0.; 3],
            scale: 1.,
            opacity: 1.,
        };
        presenter.set_overviews(&r, &[p]);
        let pipeline = presenter.overview_pipeline.clone();
        let buffer = presenter.overview_buffer.clone();
        assert_eq!(buffer.as_ref().unwrap().size(), 128);
        presenter.overviews_changed = false;
        presenter.set_overviews(&r, &[p]);
        assert!(
            !presenter.overviews_changed,
            "unchanged geometry is not uploaded"
        );
        p.work_area[0][0] = 1.;
        presenter.set_overviews(&r, &[p]);
        assert!(presenter.overviews_changed);
        assert_eq!(presenter.overview_buffer, buffer, "panning reuses storage");
        presenter.set_overviews(&r, &[OverviewPlacement { opacity: 0., ..p }]);
        assert!(
            presenter.overviews.is_empty(),
            "hidden overview performs no draw"
        );
        presenter.set_overviews(&r, &[p]);
        assert_eq!(
            presenter.overview_buffer, buffer,
            "reopening reuses storage"
        );
        assert_eq!(
            presenter.overview_pipeline, pipeline,
            "no redundant pipeline preparation"
        );
    }
}
