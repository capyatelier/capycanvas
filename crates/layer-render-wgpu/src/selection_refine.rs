//! GPU selection options, sharing the asynchronous region history capture.
use super::*;
use layer_render::{SelectionMode, SelectionRefinement};
use wgpu::util::DeviceExt;

pub(super) struct SelectionRefiner {
    layout: wgpu::BindGroupLayout,
    bounds_layout: wgpu::BindGroupLayout,
    pipelines: [Deferred<wgpu::ComputePipeline>; 4],
    empty: wgpu::Buffer,
}
impl SelectionRefiner {
    pub fn new(device: &PipelineDevice) -> Self {
        let entries: Vec<_> = (0..6)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 0 || binding == 5 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage {
                            read_only: binding < 3,
                        }
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("selection options"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("selection options"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let bounds_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("selection bounds"),
            entries: &[entries[0], entries[4]],
        });
        let bounds_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("selection bounds"),
                bind_group_layouts: &[Some(&bounds_layout)],
                immediate_size: 0,
            });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("selection feather and modes"),
            source: wgpu::ShaderSource::Wgsl(include_str!("selection_refine.wgsl").into()),
        });
        let pipelines = ["feather_h", "combine", "resize_h", "bounds"].map(|entry| {
            let (device, layout, shader) = (
                device.clone(),
                if entry == "bounds" {
                    bounds_pipeline_layout.clone()
                } else {
                    pipeline_layout.clone()
                },
                shader.clone(),
            );
            Deferred::pipeline(move |mode| {
                mode.compute(
                    &device,
                    &wgpu::ComputePipelineDescriptor {
                        label: Some(entry),
                        layout: Some(&layout),
                        module: &shader,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        cache: None,
                    },
                )
            })
        });
        let empty = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("no prior selection"),
            size: 48,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        Self {
            layout,
            bounds_layout,
            pipelines,
            empty,
        }
    }
    pub fn prepare(&self, compiler: &startup::Compiler, options: &SelectionRefinement) -> bool {
        let mut ready = true;
        for (index, pipeline) in self.pipelines.iter().enumerate() {
            if (index == 0 && options.feather == 0.) || (index == 2 && options.resize == 0) {
                continue;
            }
            compiler.pipeline(pipeline, startup::BRUSH);
            ready &= pipeline.ready();
        }
        ready
    }
    /// A tonal mask is already antialiased byte coverage in document space.
    /// With no feather/combination it needs bounds, not another image allocation
    /// and a bilinear resampling pass over every pixel.
    pub fn bounds_only(
        &self,
        device: &wgpu::Device,
        encoder: &mut crate::submission::CommandEncoder,
        extent: [u32; 2],
        coverage: wgpu::Buffer,
    ) -> flood::Region {
        let [w, h] = extent;
        let bounds_offset = 32 + u64::from(w.div_ceil(4)) * u64::from(h) * 4;
        let bytes: Vec<_> = [w, h, 0, 0, 0, 0, 0, 0]
            .into_iter()
            .flat_map(u32::to_ne_bytes)
            .collect();
        let header = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("tonal bounds initializer"),
            contents: &bytes,
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        encoder.copy_buffer_to_buffer(&header, 0, &coverage, bounds_offset, 32);
        let mut data = [0u32; 16];
        data[..2].copy_from_slice(&extent);
        let bytes: Vec<_> = data.into_iter().flat_map(u32::to_ne_bytes).collect();
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("tonal bounds extent"),
            contents: &bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let entries = [
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: coverage.as_entire_binding(),
            },
        ];
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("selection bounds"),
            layout: &self.bounds_layout,
            entries: &entries,
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("tonal bounds"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipelines[3]);
        pass.set_bind_group(0, &binding, &[]);
        pass.dispatch_workgroups(h, 1, 1);
        drop(pass);
        flood::Region {
            coverage,
            bounds_offset,
        }
    }
    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut crate::submission::CommandEncoder,
        extent: [u32; 2],
        incoming: &wgpu::Buffer,
        previous: Option<&wgpu::Buffer>,
        options: &SelectionRefinement,
    ) -> Result<flood::Region, GpuRasterError> {
        if !options.is_valid() || extent.contains(&0) {
            return Err(GpuRasterError::InvalidExtent);
        }
        let [w, h] = extent;
        let bounds_offset = 32 + u64::from(w.div_ceil(4)) * u64::from(h) * 4;
        let resize_levels = (options.resize.unsigned_abs() * 2 + 1).ilog2();
        let feather_radius = (options.feather * 1.5).ceil() as u32;
        let band_height = if options.feather > 0. { h.min(256) } else { h };
        let scratch_size = if resize_levels > 0 {
            u64::from(w.div_ceil(4)) * u64::from(h) * 4 * u64::from(resize_levels)
        } else if options.feather > 0. {
            u64::from(w) * u64::from(h.min(band_height + 2 * feather_radius)) * 4
        } else {
            4
        };
        let size = (bounds_offset + 32).next_multiple_of(16);
        let limit = device.limits();
        if size.max(scratch_size) > limit.max_storage_buffer_binding_size
            || w.div_ceil(8) > limit.max_compute_workgroups_per_dimension
            || h > limit.max_compute_workgroups_per_dimension
        {
            return Err(GpuRasterError::SizeOverflow);
        }
        let inverse = options.source_to_document.inverse().unwrap().0;
        let mode = match options.mode {
            SelectionMode::New => 0,
            SelectionMode::Add => 1,
            SelectionMode::Subtract => 2,
            SelectionMode::Intersect => 3,
        };
        let data: Vec<u32> = [w, h, mode, u32::from(options.antialias)]
            .into_iter()
            .chain(
                inverse
                    .into_iter()
                    .chain([options.feather, options.resize as f32])
                    .map(f32::to_bits),
            )
            .collect();
        let coverage = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("8-bit selection history"),
            size,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let header: Vec<_> = [0, 0, w, h, 0, 2, 0, 0]
            .into_iter()
            .flat_map(u32::to_ne_bytes)
            .collect();
        let header = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("selection coverage header"),
            contents: &header,
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        encoder.copy_buffer_to_buffer(&header, 0, &coverage, 0, 32);
        let scratch = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("selection refinement intermediate"),
            size: scratch_size,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        // Symmetric normalized Gaussian, computed once instead of exp() and
        // normalization for every tap of every image pixel. MAX_FEATHER=100
        // needs 151 nonnegative distances, packed into 38 uniform vec4s.
        let mut weights = [0f32; 152];
        let sigma = (options.feather * 0.5).max(0.001);
        for (d, weight) in weights
            .iter_mut()
            .enumerate()
            .take(feather_radius as usize + 1)
        {
            *weight = (-0.5 * (d * d) as f32 / (sigma * sigma)).exp();
        }
        let total = weights[0] + 2. * weights[1..].iter().sum::<f32>();
        let bytes: Vec<_> = weights
            .into_iter()
            .flat_map(|v| (v / total).to_ne_bytes())
            .collect();
        let weights = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("selection Gaussian weights"),
            contents: &bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let binding = |level, start: u32, end: u32| {
            let data: Vec<_> = data
                .iter()
                .copied()
                .chain([level, start, end, start.saturating_sub(feather_radius)])
                .flat_map(u32::to_ne_bytes)
                .collect();
            let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("selection options"),
                contents: &data,
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let buffers = [
                &params,
                incoming,
                previous.unwrap_or(&self.empty),
                &scratch,
                &coverage,
                &weights,
            ];
            let entries: Vec<_> = buffers
                .iter()
                .enumerate()
                .map(|(i, b)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: b.as_entire_binding(),
                })
                .collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("selection options"),
                layout: &self.layout,
                entries: &entries,
            })
        };
        let resize_bindings: Vec<_> = (1..=resize_levels)
            .map(|level| binding(level, 0, h))
            .collect();
        for start in (0..h).step_by(band_height as usize) {
            let end = (start + band_height).min(h);
            let final_binding = binding(0, start, end);
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("feather and combine selection"),
                timestamp_writes: None,
            });
            for binding in &resize_bindings {
                pass.set_bind_group(0, binding, &[]);
                pass.set_pipeline(&self.pipelines[2]);
                pass.dispatch_workgroups(w.div_ceil(256), h, 1);
            }
            pass.set_bind_group(0, &final_binding, &[]);
            if options.feather > 0. {
                pass.set_pipeline(&self.pipelines[0]);
                let scratch_rows =
                    (end + feather_radius).min(h) - start.saturating_sub(feather_radius);
                pass.dispatch_workgroups(w.div_ceil(128), scratch_rows, 1);
            }
            pass.set_pipeline(&self.pipelines[1]);
            pass.dispatch_workgroups(w.div_ceil(256), end - start, 1);
            drop(pass);
        }
        Ok(self.bounds_only(device, encoder, extent, coverage))
    }
}
