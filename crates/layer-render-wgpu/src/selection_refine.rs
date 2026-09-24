//! GPU selection options, sharing the asynchronous region history capture.
use super::*;
use layer_render::{SelectionMode, SelectionRefinement};
use wgpu::util::DeviceExt;

pub(super) struct SelectionRefiner {
    layout: wgpu::BindGroupLayout,
    pipelines: [Deferred<wgpu::ComputePipeline>; 3],
    empty: wgpu::Buffer,
}
impl SelectionRefiner {
    pub fn new(device: &PipelineDevice) -> Self {
        let entries: Vec<_> = (0..5)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 0 {
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
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("selection feather and modes"),
            source: wgpu::ShaderSource::Wgsl(include_str!("selection_refine.wgsl").into()),
        });
        let pipelines = ["feather_h", "combine", "resize_h"].map(|entry| {
            let (device, layout, shader) =
                (device.clone(), pipeline_layout.clone(), shader.clone());
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
        let scratch_size = if resize_levels > 0 {
            u64::from(w.div_ceil(4)) * u64::from(h) * 4 * u64::from(resize_levels)
        } else if options.feather > 0. {
            u64::from(w) * u64::from(h) * 4
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
        let header: Vec<_> = [0, 0, w, h, 0, 2, 0, 0, w, h, 0, 0, 0, 0, 0, 0]
            .into_iter()
            .flat_map(u32::to_ne_bytes)
            .collect();
        let header = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("selection bounds initializer"),
            contents: &header,
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        encoder.copy_buffer_to_buffer(&header, 0, &coverage, 0, 32);
        encoder.copy_buffer_to_buffer(&header, 32, &coverage, bounds_offset, 32);
        let scratch = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("selection refinement intermediate"),
            size: scratch_size,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let binding = |level| {
            let data: Vec<_> = data
                .iter()
                .copied()
                .chain([level, 0, 0, 0])
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
        let final_binding = binding(0);
        let resize_bindings: Vec<_> = (1..=resize_levels).map(binding).collect();
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
            pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
        }
        pass.set_pipeline(&self.pipelines[1]);
        pass.dispatch_workgroups(w.div_ceil(256), h, 1);
        drop(pass);
        Ok(flood::Region {
            coverage,
            bounds_offset,
        })
    }
}
