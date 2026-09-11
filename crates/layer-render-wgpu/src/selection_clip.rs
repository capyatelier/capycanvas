//! One reusable, GPU-generated selection. Four coverage samples fit in a nibble;
//! the buffer uses half a byte per pixel without consuming a brush texture slot.
use super::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};
use wgpu::util::DeviceExt;

pub(super) struct SelectionClip {
    pub(super) crossings: Deferred<wgpu::ComputePipeline>,
    pub(super) fill: Deferred<wgpu::ComputePipeline>,
    pub(super) resample: Deferred<wgpu::ComputePipeline>,
    layout: wgpu::BindGroupLayout,
    pub buffer: Option<wgpu::Buffer>,
    pub binding: Option<wgpu::BindGroup>,
    geometry: Option<Arc<layer_core::Selection>>,
    pub generations: u64,
    pub bytes: u64,
    pixels: BTreeMap<usize, (Weak<layer_core::SelectionPixels>, wgpu::Buffer)>,
    pixels_bytes: u64,
}
impl SelectionClip {
    pub fn new(device: &PipelineDevice) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("selection raster inputs"),
            entries: &[
                buffer_entry(0, wgpu::BufferBindingType::Uniform),
                buffer_entry(1, wgpu::BufferBindingType::Storage { read_only: true }),
                buffer_entry(2, wgpu::BufferBindingType::Storage { read_only: false }),
            ],
        });
        let shader = {
            let device = device.clone();
            Deferred::new(move || {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("packed selection coverage"),
                    source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[
                        include_str!("selection_clip_init.wgsl"),
                        include_str!("selection_geometry.wgsl"),
                    ])),
                })
            })
        };
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("selection raster layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry| {
            let (device, pipeline_layout, shader) =
                (device.clone(), pipeline_layout.clone(), shader.clone());
            Deferred::new(move || {
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("rasterize packed selection"),
                    layout: Some(&pipeline_layout),
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                })
            })
        };
        let resample = {
            let (device, layout) = (device.clone(), pipeline_layout.clone());
            Deferred::new(move || {
                let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("affine selection coverage"),
                    source: wgpu::ShaderSource::Wgsl(
                        include_str!("selection_resample.wgsl").into(),
                    ),
                });
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("resample packed selection"),
                    layout: Some(&layout),
                    module: &shader,
                    entry_point: Some("resample"),
                    compilation_options: Default::default(),
                    cache: None,
                })
            })
        };
        Self {
            crossings: pipeline("crossings"),
            fill: pipeline("fill"),
            resample,
            layout,
            buffer: None,
            binding: None,
            geometry: None,
            generations: 0,
            bytes: 0,
            pixels: BTreeMap::new(),
            pixels_bytes: 0,
        }
    }
    pub fn compile_all(&self) {
        self.crossings.compile();
        self.fill.compile();
        self.resample.compile();
    }

    pub fn reset(&mut self) {
        self.buffer = None;
        self.binding = None;
        self.geometry = None;
        self.bytes = 0;
        self.prune_pixels();
    }
    pub fn storage_bytes(&self) -> u64 {
        self.bytes + self.pixels_bytes
    }
    fn prune_pixels(&mut self) {
        self.pixels
            .retain(|_, (owner, _)| owner.strong_count() != 0);
        self.pixels_bytes = self.pixels.values().map(|(_, b)| b.size()).sum();
    }
    /// Retain GPU-produced coverage across history and consumers. Only replay
    /// after renderer recreation needs an upload from the durable core copy.
    pub fn remember_pixels(
        &mut self,
        pixels: &Arc<layer_core::SelectionPixels>,
        buffer: wgpu::Buffer,
    ) {
        self.pixels
            .retain(|_, (owner, _)| owner.strong_count() != 0);
        self.pixels.insert(
            Arc::as_ptr(pixels) as usize,
            (Arc::downgrade(pixels), buffer),
        );
        self.pixels_bytes = self.pixels.values().map(|(_, b)| b.size()).sum();
    }
    pub fn pixel_buffer(
        &mut self,
        device: &wgpu::Device,
        pixels: &Arc<layer_core::SelectionPixels>,
    ) -> wgpu::Buffer {
        if let Some((_, buffer)) = self.pixels.get(&(Arc::as_ptr(pixels) as usize)) {
            return buffer.clone();
        }
        let [w, h] = pixels.extent();
        let mut bytes: Vec<_> = [0, 0, w, h, 0, 1, 0, 0]
            .into_iter()
            .chain(pixels.words().iter().copied())
            .flat_map(u32::to_ne_bytes)
            .collect();
        bytes.resize(bytes.len().next_multiple_of(16), 0);
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("restored selection pixels"),
            contents: &bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        self.remember_pixels(pixels, buffer.clone());
        buffer
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut crate::submission::CommandEncoder,
        extent: [u32; 2],
        geometry: &Arc<layer_core::Selection>,
    ) -> Result<(), GpuRasterError> {
        if self.geometry.as_ref().is_some_and(|old| old == geometry) {
            return Ok(());
        }
        let inverse = geometry
            .affine
            .inverse()
            .ok_or(GpuRasterError::InvalidTransform(
                "Invalid selection transform",
            ))?
            .0;
        // Keep the zero-resample translation path (including layer offsets).
        // Other affine placements are prepared once, not evaluated per brush dab.
        let translation = geometry.affine.0[..4] == [1., 0., 0., 1.];
        let mut edges = Vec::<f32>::new();
        for contour in geometry.contours() {
            for (a, b) in contour
                .iter()
                .zip(contour.iter().cycle().skip(1))
                .take(contour.len())
            {
                let a = geometry.affine.map(*a);
                let b = geometry.affine.map(*b);
                edges.extend([a.x, a.y, b.x, b.y]);
            }
        }
        let bounds = match &geometry.shape {
            layer_core::SelectionShape::Pixels(pixels) if translation => {
                PixelRect::full(pixels.extent())
            }
            _ => pixel_rect(geometry.bounds(), extent),
        };
        let words = (u64::from(bounds.width().div_ceil(8)) * u64::from(bounds.height())).max(1);
        let bytes = (32 + words * 4).next_multiple_of(16);
        let source_bytes = match &geometry.shape {
            layer_core::SelectionShape::Pixels(pixels) => {
                (32 + pixels.words().len() as u64 * 4).next_multiple_of(16)
            }
            _ => edges.len() as u64 * 4,
        };
        if bytes.max(source_bytes) > device.limits().max_storage_buffer_binding_size
            || edges.len() as u32 / 4 > device.limits().max_compute_workgroups_per_dimension
            || bounds.height() > device.limits().max_compute_workgroups_per_dimension
            || bounds.width().div_ceil(512) > device.limits().max_compute_workgroups_per_dimension
        {
            return Err(GpuRasterError::SizeOverflow);
        }
        if bytes > self.bytes || self.buffer.is_none() {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("packed brush selection"),
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            self.binding = None;
            self.buffer = Some(buffer);
            self.bytes = bytes;
        }
        let offset = match geometry.shape {
            layer_core::SelectionShape::Pixels(_) if translation => layer_core::Point {
                x: geometry.affine.0[4],
                y: geometry.affine.0[5],
            },
            _ => layer_core::Point::default(),
        };
        let header = [
            bounds.min_x,
            bounds.min_y,
            bounds.width(),
            bounds.height(),
            u32::from(geometry.inverted),
            1,
            offset.x.to_bits(),
            offset.y.to_bits(),
        ];
        let mut header_bytes: Vec<_> = header.iter().flat_map(|v| v.to_ne_bytes()).collect();
        if !translation && matches!(geometry.shape, layer_core::SelectionShape::Pixels(_)) {
            header_bytes.extend(
                inverse
                    .into_iter()
                    .chain([0., 0.])
                    .flat_map(f32::to_ne_bytes),
            );
        }
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("selection raster geometry header"),
            contents: &header_bytes,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_SRC,
        });
        if let layer_core::SelectionShape::Pixels(pixels) = &geometry.shape {
            let source = self.pixel_buffer(device, pixels);
            let buffer = self.buffer.as_ref().unwrap();
            encoder.copy_buffer_to_buffer(&params, 0, buffer, 0, 32);
            if translation {
                encoder.copy_buffer_to_buffer(&source, 32, buffer, 32, words * 4);
            } else if !bounds.is_empty() {
                let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("affine selection inputs"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: params.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: source.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: buffer.as_entire_binding(),
                        },
                    ],
                });
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("resample selection placement"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.resample);
                pass.set_bind_group(0, &binding, &[]);
                pass.dispatch_workgroups(bounds.width().div_ceil(512), bounds.height(), 1);
            }
            self.geometry = Some(geometry.clone());
            self.generations += 1;
            return Ok(());
        }
        let edge_count = edges.len() as u32 / 4;
        if edges.is_empty() {
            edges.resize(4, 0.);
        }
        let edge_bytes: Vec<_> = edges.iter().flat_map(|v| v.to_ne_bytes()).collect();
        let edge_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("selection contours"),
            contents: &edge_bytes,
            usage: wgpu::BufferUsages::STORAGE,
        });
        let buffer = self.buffer.as_ref().unwrap();
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("selection raster inputs"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: edge_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: buffer.as_entire_binding(),
                },
            ],
        });
        // Encoder-ordered writes are essential: replay can use several different
        // selections in one submission, including in the browser backend.
        encoder.copy_buffer_to_buffer(&params, 0, buffer, 0, 32);
        encoder.clear_buffer(buffer, 32, Some(bytes - 32));
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("selection coverage initialization"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &binding, &[]);
        pass.set_pipeline(&self.crossings);
        pass.dispatch_workgroups(edge_count, 1, 1);
        pass.set_pipeline(&self.fill);
        pass.dispatch_workgroups(bounds.height(), 1, 1);
        self.geometry = Some(geometry.clone());
        self.generations += 1;
        Ok(())
    }
}
fn buffer_entry(binding: u32, ty: wgpu::BufferBindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}
