//! GPU connected regions with immutable packed coverage. Readback is a single
//! asynchronous history snapshot per request, never part of live brush work.
use super::*;
use layer_render::RegionRefinement;
use wgpu::util::DeviceExt;

pub(super) struct Flood {
    layout: wgpu::BindGroupLayout,
    pipelines: std::collections::HashMap<&'static str, Deferred<wgpu::ComputePipeline>>,
    parents: Option<wgpu::Buffer>,
    capacity: u64,
    empty: wgpu::Buffer,
}
const STAGES: [&str; 10] = [
    "close_h",
    "close_v",
    "reopen_h",
    "reopen_v",
    "initialize",
    "merge",
    "component_mask",
    "expand_h",
    "expand_v",
    "pack",
];
fn stages(refinement: RegionRefinement) -> impl Iterator<Item = &'static str> {
    STAGES.into_iter().filter(move |entry| match *entry {
        "close_h" | "close_v" | "reopen_h" | "reopen_v" => refinement.gap_closing != 0,
        "component_mask" => refinement.expansion != 0 || refinement.smoothing != 0.,
        "expand_h" | "expand_v" => refinement.expansion != 0,
        _ => true,
    })
}
pub(super) struct Region {
    pub coverage: wgpu::Buffer,
    pub bounds_offset: u64,
}
impl Flood {
    pub fn storage_bytes(&self) -> u64 {
        self.capacity + self.empty.size()
    }
    pub fn new(device: &PipelineDevice) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("connected region"),
            source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[
                include_str!("flood.wgsl"),
                include_str!("region_refine.wgsl"),
                &include_str!("selection_clip.wgsl")
                    .replace("@group(1) @binding(1)", "@group(0) @binding(4)"),
            ])),
        });
        let entries: Vec<_> = (1..6).map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: if binding == 1 {
                    wgpu::BufferBindingType::Uniform
                } else {
                    wgpu::BufferBindingType::Storage {
                        read_only: binding == 4,
                    }
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }).collect();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("connected region"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("connected region"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipelines = STAGES
            .into_iter()
            .map(|entry| {
                let (device, layout, shader) =
                    (device.clone(), pipeline_layout.clone(), shader.clone());
                (
                    entry,
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
                    }),
                )
            })
            .collect();
        Self {
            layout,
            pipelines,
            parents: None,
            capacity: 0,
            empty: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("unlimited region"),
                size: 48,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
        }
    }
    #[cfg(test)]
    pub fn pipelines(&self) -> impl Iterator<Item = &Deferred<wgpu::ComputePipeline>> {
        STAGES.into_iter().map(|entry| &self.pipelines[entry])
    }
    pub fn prepare(&self, compiler: &startup::Compiler, refinement: RegionRefinement) -> bool {
        compiler.require(stages(refinement).map(|entry| &self.pipelines[entry]), startup::BRUSH)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn encode_input(
        &mut self,
        device: &PipelineDevice,
        encoder: &mut crate::submission::CommandEncoder,
        extent: [u32; 2],
        seed: [u32; 2],
        tolerance: f32,
        selection: Option<&wgpu::Buffer>,
        refinement: RegionRefinement,
        classified: &wgpu::Buffer,
        contiguous: bool,
    ) -> Result<Region, GpuRasterError> {
        let [w, h] = extent;
        if w == 0
            || h == 0
            || seed[0] >= w
            || seed[1] >= h
            || !tolerance.is_finite()
            || !(0.0..=1.0).contains(&tolerance)
            || !refinement.is_valid()
        {
            return Err(GpuRasterError::InvalidExtent);
        }
        let limit = device.limits();
        if w > limit.max_texture_dimension_2d || h > limit.max_texture_dimension_2d {
            return Err(GpuRasterError::SizeOverflow);
        }
        let bytes = u64::from(w) * u64::from(h) * 4;
        let words = u64::from(w.div_ceil(8)) * u64::from(h);
        let boundaries =
            u64::from(w) * u64::from((h - 1) / 16) + u64::from(h) * u64::from((w - 1) / 16);
        if bytes > limit.max_storage_buffer_binding_size
            || bytes > limit.max_buffer_size
            || words.max(boundaries).div_ceil(64)
                > u64::from(limit.max_compute_workgroups_per_dimension).pow(2)
        {
            return Err(GpuRasterError::SizeOverflow);
        }
        if bytes > self.capacity {
            self.parents = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("flood reusable parents"),
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }));
            self.capacity = bytes;
        }
        let mask_bytes = u64::from(w.div_ceil(32)) * u64::from(h) * 4;
        if classified.size() < mask_bytes {
            return Err(GpuRasterError::SizeOverflow);
        }
        let region = Region {
            coverage: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("connected coverage"),
                size: (64 + words * 4).next_multiple_of(16),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            bounds_offset: 32 + words * 4,
        };
        let params = [
            w,
            h,
            seed[0],
            seed[1],
            tolerance.to_bits(),
            (refinement.gap_closing as f32).to_bits(),
            (refinement.expansion as f32).to_bits(),
            refinement.smoothing.to_bits(),
            1, u32::from(!contiguous), 0, 0,
        ];
        let data: Vec<_> = params.into_iter().flat_map(u32::to_ne_bytes).collect();
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("flood request"),
            contents: &data,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let entries: Vec<_> = [
            &uniform,
            self.parents.as_ref().unwrap(),
            &region.coverage,
            selection.unwrap_or(&self.empty),
            classified,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, b)| wgpu::BindGroupEntry {
            binding: i as u32 + 1,
            resource: b.as_entire_binding(),
        })
        .collect();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("flood request"),
            layout: &self.layout,
            entries: &entries,
        });
        // All morphology is bit-packed. The parent allocation doubles as the
        // other ping-pong plane before/after (never during) component labeling.
        // Interactive hosts prepare these recipes on the startup compiler.
        // Headless callers compile only what they use. Disabled refinements
        // keep the original three dispatches.
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("connected region"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &group, &[]);
        for entry in stages(refinement) {
            pass.set_pipeline(&self.pipelines[entry]);
            if entry == "merge" && !contiguous { continue; }
            if entry == "initialize" {
                pass.dispatch_workgroups(w.div_ceil(16), h.div_ceil(16), 1);
            } else {
                let invocations = match entry {
                    "merge" => boundaries,
                    "pack" => words,
                    _ => mask_bytes / 4,
                };
                dispatch_linear(
                    &mut pass,
                    invocations,
                    limit.max_compute_workgroups_per_dimension,
                );
            }
        }
        Ok(region)
    }
}

fn dispatch_linear(pass: &mut wgpu::ComputePass<'_>, invocations: u64, limit: u32) {
    let groups = invocations.div_ceil(64) as u32;
    pass.dispatch_workgroups(groups.min(limit), groups.div_ceil(limit).max(1), 1);
}
