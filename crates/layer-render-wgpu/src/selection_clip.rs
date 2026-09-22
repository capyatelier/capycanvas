//! One reusable, GPU-generated selection. Legacy masks use coverage nibbles;
//! feathered masks use bytes. Neither consumes a brush texture slot.
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
    region: Option<PixelRect>,
    extent: Option<[u32; 2]>,
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
            Deferred::pipeline(move |mode| {
                mode.compute(
                    &device,
                    &wgpu::ComputePipelineDescriptor {
                        label: Some("rasterize packed selection"),
                        layout: Some(&pipeline_layout),
                        module: &shader,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        cache: None,
                    },
                )
            })
        };
        let resample = {
            let (device, layout) = (device.clone(), pipeline_layout.clone());
            Deferred::pipeline(move |mode| {
                let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("affine selection coverage"),
                    source: wgpu::ShaderSource::Wgsl(
                        include_str!("selection_resample.wgsl").into(),
                    ),
                });
                mode.compute(
                    &device,
                    &wgpu::ComputePipelineDescriptor {
                        label: Some("resample packed selection"),
                        layout: Some(&layout),
                        module: &shader,
                        entry_point: Some("resample"),
                        compilation_options: Default::default(),
                        cache: None,
                    },
                )
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
            region: None,
            extent: None,
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
        self.region = None;
        self.extent = None;
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
        let mut bytes: Vec<_> = [0, 0, w, h, 0, pixels.coverage_format(), 0, 0]
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
        self.prepare_region(device, encoder, extent, geometry, None)
    }
    /// Preserve the ordinary GPU coverage rules while limiting preparation to
    /// a document-coordinate window. Packed source coverage is copied by rows;
    /// polygon rasterization and affine resampling remain on the GPU.
    pub fn prepare_region(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut crate::submission::CommandEncoder,
        extent: [u32; 2],
        geometry: &Arc<layer_core::Selection>,
        region: Option<PixelRect>,
    ) -> Result<(), GpuRasterError> {
        if self.extent == Some(extent) && self.region == region && self.geometry.as_ref().is_some_and(|old| old == geometry) {
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
        let packing = match &geometry.shape { layer_core::SelectionShape::Pixels(p) => p.pixels_per_word(), _ => 8 };
        let format = match &geometry.shape { layer_core::SelectionShape::Pixels(p) => p.coverage_format(), _ => 1 };
        let requested = region
            .unwrap_or(PixelRect::full(extent))
            .intersect(PixelRect::full(extent));
        let source_region = match &geometry.shape {
            layer_core::SelectionShape::Pixels(pixels) if region.is_some() => {
                let rect = layer_core::Rect {
                    min: layer_core::Point {
                        x: requested.min_x() as f32,
                        y: requested.min_y() as f32,
                    },
                    max: layer_core::Point {
                        x: requested.max_x() as f32,
                        y: requested.max_y() as f32,
                    },
                };
                let mut source = layer_core::Affine(inverse).bounds(rect);
                source.min.x -= 2.;
                source.min.y -= 2.;
                source.max.x += 2.;
                source.max.y += 2.;
                let bounds = if requested.is_empty() {
                    PixelRect::EMPTY
                } else {
                    pixel_rect(source, pixels.extent())
                };
                // Retain complete source words. No coverage is rasterized or
                // interpolated on the CPU, including partial right-edge words.
                PixelRect::new(
                    bounds.min_x() / packing * packing,
                    bounds.min_y(),
                    bounds
                        .max_x()
                        .div_ceil(packing)
                        .saturating_mul(packing)
                        .min(pixels.extent()[0]),
                    bounds.max_y(),
                )
            }
            layer_core::SelectionShape::Pixels(pixels) => PixelRect::full(pixels.extent()),
            _ => PixelRect::EMPTY,
        };
        let bounds = match &geometry.shape {
            layer_core::SelectionShape::Pixels(_) if translation => source_region,
            _ => pixel_rect(geometry.bounds(), extent).intersect(requested),
        };
        let words = (u64::from(bounds.width().div_ceil(packing)) * u64::from(bounds.height())).max(1);
        let bytes = (32 + words * 4).next_multiple_of(16);
        let source_bytes = match &geometry.shape {
            layer_core::SelectionShape::Pixels(_) => (32
                + u64::from(source_region.width().div_ceil(packing))
                    * u64::from(source_region.height())
                    * 4)
            .max(36)
            .next_multiple_of(16),
            _ => edges.len() as u64 * 4,
        };
        if bytes.max(source_bytes) > device.limits().max_storage_buffer_binding_size
            || edges.len() as u32 / 4 > device.limits().max_compute_workgroups_per_dimension
            || bounds.height() > device.limits().max_compute_workgroups_per_dimension
            || bounds.width().div_ceil(512) > device.limits().max_compute_workgroups_per_dimension
        {
            return Err(GpuRasterError::SizeOverflow);
        }
        if bytes > self.bytes || self.buffer.is_none() || (region.is_some() && bytes != self.bytes)
        {
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
            bounds.min_x(),
            bounds.min_y(),
            bounds.width(),
            bounds.height(),
            u32::from(geometry.inverted),
            format,
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
            let source = if region.is_some() {
                pixel_region_buffer(device, pixels, source_region)
            } else {
                self.pixel_buffer(device, pixels)
            };
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
                pass.dispatch_workgroups(bounds.width().div_ceil(packing * 64), bounds.height(), 1);
            }
            self.extent = Some(extent);
            self.geometry = Some(geometry.clone());
            self.region = region;
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
        self.extent = Some(extent);
        self.geometry = Some(geometry.clone());
        self.region = region;
        self.generations += 1;
        Ok(())
    }
}
fn pixel_region_buffer(
    device: &wgpu::Device,
    pixels: &layer_core::SelectionPixels,
    bounds: PixelRect,
) -> wgpu::Buffer {
    let packing = pixels.pixels_per_word();
    let stride = pixels.extent()[0].div_ceil(packing) as usize;
    let width = bounds.width().div_ceil(packing) as usize;
    let mut bytes = Vec::with_capacity(32 + width * bounds.height() as usize * 4);
    for value in [
        bounds.min_x(),
        bounds.min_y(),
        bounds.width(),
        bounds.height(),
        0,
        pixels.coverage_format(),
        0,
        0,
    ] {
        bytes.extend(value.to_ne_bytes());
    }
    for y in bounds.min_y() as usize..bounds.max_y() as usize {
        let start = y * stride + bounds.min_x() as usize / packing as usize;
        for word in &pixels.words()[start..start + width] {
            bytes.extend(word.to_ne_bytes());
        }
    }
    bytes.resize(bytes.len().max(36).next_multiple_of(16), 0);
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("windowed selection pixels"),
        contents: &bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    })
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
