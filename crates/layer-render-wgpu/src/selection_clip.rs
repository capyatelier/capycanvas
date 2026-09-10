//! One reusable, GPU-generated selection. Four coverage samples fit in a nibble;
//! the buffer uses half a byte per pixel without consuming a brush texture slot.
use super::*;
use std::sync::Arc;
use wgpu::util::DeviceExt;

pub(super) struct SelectionClip {
    crossings: wgpu::ComputePipeline,
    fill: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    buffer: Option<wgpu::Buffer>,
    pub binding: Option<wgpu::BindGroup>,
    geometry: Option<Arc<layer_core::Selection>>,
    pub generations: u64,
    pub bytes: u64,
}
impl SelectionClip {
    pub fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("selection raster inputs"),
            entries: &[
                buffer_entry(0, wgpu::BufferBindingType::Uniform),
                buffer_entry(1, wgpu::BufferBindingType::Storage { read_only: true }),
                buffer_entry(2, wgpu::BufferBindingType::Storage { read_only: false }),
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("packed selection coverage"),
            source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[
                include_str!("selection_clip_init.wgsl"),
                include_str!("selection_geometry.wgsl"),
            ])),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("selection raster layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("rasterize packed selection"),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            crossings: pipeline("crossings"),
            fill: pipeline("fill"),
            layout,
            buffer: None,
            binding: None,
            geometry: None,
            generations: 0,
            bytes: 0,
        }
    }
    pub fn reset(&mut self) {
        self.buffer = None;
        self.binding = None;
        self.geometry = None;
        self.bytes = 0;
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target_layout: &wgpu::BindGroupLayout,
        target: &wgpu::Buffer,
        extent: [u32; 2],
        geometry: &Arc<layer_core::Selection>,
    ) -> Result<(), GpuRasterError> {
        if self.geometry.as_ref().is_some_and(|old| old == geometry) {
            return Ok(());
        }
        let mut bounds = layer_core::Rect::EMPTY;
        let mut edges = Vec::<f32>::new();
        for contour in geometry.contours.iter() {
            for (a, b) in contour
                .iter()
                .zip(contour.iter().cycle().skip(1))
                .take(contour.len())
            {
                bounds.include_circle(*a, 1.);
                edges.extend([a.x, a.y, b.x, b.y]);
            }
        }
        let bounds = pixel_rect(bounds, extent);
        let words = (u64::from(bounds.width().div_ceil(8)) * u64::from(bounds.height())).max(1);
        let bytes = (32 + words * 4).next_multiple_of(16);
        if bytes.max(edges.len() as u64 * 4) > device.limits().max_storage_buffer_binding_size
            || edges.len() as u32 / 4 > device.limits().max_compute_workgroups_per_dimension
        {
            return Err(GpuRasterError::SizeOverflow);
        }
        if bytes > self.bytes || self.buffer.is_none() {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("packed brush selection"),
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.binding = Some(create_target_bind_group(
                device,
                target_layout,
                target,
                &buffer,
            ));
            self.buffer = Some(buffer);
            self.bytes = bytes;
        }
        let header = [
            bounds.min_x,
            bounds.min_y,
            bounds.width(),
            bounds.height(),
            u32::from(geometry.inverted),
            1,
            edges.len() as u32 / 4,
            words as u32,
        ];
        let header_bytes: Vec<_> = header.iter().flat_map(|v| v.to_ne_bytes()).collect();
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("selection raster geometry header"),
            contents: &header_bytes,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_SRC,
        });
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
        pass.dispatch_workgroups(header[6], 1, 1);
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
