//! GPU connected regions with immutable packed coverage. Readback is a single
//! asynchronous history snapshot per request, never part of live brush work.
use super::*;
use layer_render::RegionRefinement;
use wgpu::util::DeviceExt;

const WINDOW: u32 = 2048;
const PARAMETER_BYTES: u64 = 96;

pub(super) struct Flood {
    layout: wgpu::BindGroupLayout,
    pipelines: std::collections::HashMap<&'static str, Deferred<wgpu::ComputePipeline>>,
    labels: Option<wgpu::Buffer>,
    capacity: u64,
    empty: wgpu::Buffer,
}
const STAGES: [&str; 15] = [
    "close_h",
    "close_v",
    "reopen_h",
    "reopen_v",
    "initialize",
    "merge",
    "mark_exterior",
    "export_border",
    "seam_merge",
    "propagate_flags",
    "resolve_seed",
    "component_mask",
    "expand_h",
    "expand_v",
    "pack",
];
fn stages(refinement: RegionRefinement, enclosed: bool, contiguous: bool) -> impl Iterator<Item = &'static str> {
    STAGES.into_iter().filter(move |entry| match *entry {
        "close_h" | "close_v" | "reopen_h" | "reopen_v" => refinement.gap_closing != 0,
        "initialize" | "merge" | "export_border" | "seam_merge" | "component_mask" => contiguous,
        "mark_exterior" | "propagate_flags" => enclosed,
        "resolve_seed" => contiguous && !enclosed,
        "expand_h" | "expand_v" => refinement.expansion != 0,
        _ => true,
    })
}
pub(super) struct Region {
    pub coverage: wgpu::Buffer,
    pub bounds_offset: u64,
}
/// Word offsets of the label storage: window parents or a bit plane, border
/// slots, exterior flags, window marks and the seed.
#[derive(Clone, Copy)]
struct Labels { side: u32, grid: [u32; 2], slots: u64, flags: u64, marks: u64, seed: u64, words: u64 }
impl Labels {
    fn new(extent: [u32; 2], limits: &wgpu::Limits) -> Option<Self> {
        let [w, h] = extent;
        let limit = limits.max_storage_buffer_binding_size.min(limits.max_buffer_size);
        let side = (5..=WINDOW.ilog2()).rev().map(|n| 1u32 << n).find(|side| u64::from(*side).pow(2) * 32 <= limit)?;
        let grid = extent.map(|v| v.div_ceil(side));
        let plane = u64::from(w.div_ceil(32)) * u64::from(h);
        let slots = plane.max(u64::from(side).pow(2));
        let flags = slots + u64::from(grid[0]) * u64::from(grid[1]) * 4 * u64::from(side);
        let marks = flags + (flags - slots).div_ceil(32);
        let seed = marks + u64::from(side).pow(2) / 32;
        let labels = Self { side, grid, slots, flags, marks, seed, words: seed + 4 };
        let coverage = (64 + u64::from(w.div_ceil(8)) * u64::from(h) * 4).next_multiple_of(16);
        (w <= limits.max_texture_dimension_2d && h <= limits.max_texture_dimension_2d
            && labels.words * 4 <= limit && labels.words < u64::from(u32::MAX) && coverage <= limit
            && (u64::from(w.div_ceil(8)) * u64::from(h)).div_ceil(64) <= u64::from(limits.max_compute_workgroups_per_dimension).pow(2))
            .then_some(labels)
    }
    fn windows(&self, extent: [u32; 2]) -> impl Iterator<Item = (u32, [u32; 4])> + '_ {
        (0..self.grid[1]).flat_map(move |row| (0..self.grid[0]).map(move |column| {
            let origin = [column * self.side, row * self.side];
            (row * self.grid[0] + column, [origin[0], origin[1], self.side.min(extent[0] - origin[0]), self.side.min(extent[1] - origin[1])])
        }))
    }
}
fn perimeter([w, h]: [u32; 2]) -> u64 {
    u64::from(if h == 1 { w } else if w == 1 { h } else { 2 * w + 2 * (h - 2) })
}
/// Whether a connected region of `extent` fits the device's buffers.
pub(super) fn fits(extent: [u32; 2], limits: &wgpu::Limits) -> bool {
    !extent.contains(&0) && Labels::new(extent, limits).is_some()
}
impl Flood {
    pub fn storage_bytes(&self) -> u64 {
        self.capacity + self.empty.size()
    }
    pub fn new(device: &PipelineDevice) -> Self {
        let shader = Deferred::wgsl(device, "connected region", compose_wgsl(&[
            include_str!("flood.wgsl"),
            include_str!("region_refine.wgsl"),
            &include_str!("selection_clip.wgsl")
                .replace("@group(1) @binding(1)", "@group(0) @binding(4)"),
        ]));
        let entries: Vec<_> = (1..6).map(|binding| crate::bindings::buffer(
            binding,
            wgpu::ShaderStages::COMPUTE,
            if binding == 1 { wgpu::BufferBindingType::Uniform } else { wgpu::BufferBindingType::Storage { read_only: binding == 4 } },
            binding == 1,
            if binding == 1 { wgpu::BufferSize::new(PARAMETER_BYTES) } else { None },
        )).collect();
        let layout = crate::bindings::layout(device, "connected region", &entries);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("connected region"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipelines = STAGES
            .into_iter()
            .map(|entry| (entry, Deferred::compute(device, entry, &pipeline_layout, &shader, entry)))
            .collect();
        Self {
            layout,
            pipelines,
            labels: None,
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
    pub fn prepare(&self, compiler: &startup::Compiler, refinement: RegionRefinement, enclosed: bool, contiguous: bool) -> bool {
        compiler.require(stages(refinement, enclosed, contiguous).map(|entry| &self.pipelines[entry]), startup::BRUSH)
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
        enclosed: bool,
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
        let labels = Labels::new(extent, &limit).ok_or(GpuRasterError::SizeOverflow)?;
        let mask_bytes = u64::from(w.div_ceil(32)) * u64::from(h) * 4;
        if classified.size() < mask_bytes {
            return Err(GpuRasterError::SizeOverflow);
        }
        if labels.words * 4 > self.capacity {
            self.labels = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("flood reusable labels"),
                size: labels.words * 4,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.capacity = labels.words * 4;
        }
        let words = u64::from(w.div_ceil(8)) * u64::from(h);
        let region = Region {
            coverage: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("connected coverage"),
                size: (64 + words * 4).next_multiple_of(16),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            bounds_offset: 32 + words * 4,
        };
        let summary: Vec<u8> = [0, 0, w, h, 0, 1, 0, 0, w, h, 0, 0, 0].into_iter().flat_map(u32::to_ne_bytes).collect();
        let summary = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("flood summary"),
            contents: &summary,
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        encoder.copy_buffer_to_buffer(&summary, 0, &region.coverage, 0, 32);
        encoder.copy_buffer_to_buffer(&summary, 32, &region.coverage, region.bounds_offset, 20);
        let stride = PARAMETER_BYTES.next_multiple_of(u64::from(limit.min_uniform_buffer_offset_alignment));
        let windows: Vec<_> = labels.windows(extent).collect();
        let mut data = vec![0u8; (stride * windows.len() as u64) as usize];
        let offsets = [labels.slots, labels.flags, labels.marks, labels.seed].map(|v| v as u32);
        for ((index, window), entry) in windows.iter().zip(data.chunks_mut(stride as usize)) {
            let params = [
                w, h, seed[0], seed[1],
                tolerance.to_bits(), (refinement.gap_closing as f32).to_bits(), (refinement.expansion as f32).to_bits(), f32::from(refinement.smooth_edges).to_bits(),
                1, u32::from(!contiguous), u32::from(enclosed), 0,
                window[0], window[1], window[2], window[3],
                *index, labels.grid[0], labels.grid[1], labels.side,
                offsets[0], offsets[1], offsets[2], offsets[3],
            ];
            for (word, value) in entry.as_chunks_mut::<4>().0.iter_mut().zip(params) { word.copy_from_slice(&value.to_ne_bytes()); }
        }
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("flood request"),
            contents: &data,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let store = self.labels.as_ref().unwrap();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("flood request"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniform, offset: 0, size: wgpu::BufferSize::new(PARAMETER_BYTES) }) },
                wgpu::BindGroupEntry { binding: 2, resource: store.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: region.coverage.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: selection.unwrap_or(&self.empty).as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: classified.as_entire_binding() },
            ],
        });
        let enabled: Vec<_> = stages(refinement, enclosed, contiguous).collect();
        let dimension = limit.max_compute_workgroups_per_dimension;
        let pipelines = &self.pipelines;
        let run = |encoder: &mut crate::submission::CommandEncoder, at: usize, entries: &[&'static str]| {
            let [_, _, ww, wh] = windows[at].1;
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("connected region"),
                timestamp_writes: None,
            });
            pass.set_bind_group(0, &group, &[(stride * at as u64) as u32]);
            for entry in entries.iter().filter(|entry| enabled.contains(entry)) {
                pass.set_pipeline(&pipelines[entry]);
                let invocations = match *entry {
                    "initialize" => { pass.dispatch_workgroups(ww.div_ceil(16), wh.div_ceil(16), 1); continue; }
                    "resolve_seed" => { pass.dispatch_workgroups(1, 1, 1); continue; }
                    "merge" => u64::from(ww) * u64::from((wh - 1) / 16) + u64::from(wh) * u64::from((ww - 1) / 16),
                    "mark_exterior" | "component_mask" => u64::from(ww.div_ceil(32)) * u64::from(wh),
                    "export_border" => perimeter([ww, wh]),
                    "seam_merge" => u64::from(labels.grid[0] - 1) * u64::from(h) + u64::from(labels.grid[1] - 1) * u64::from(w),
                    "propagate_flags" => labels.flags - labels.slots,
                    "pack" => words,
                    _ => mask_bytes / 4,
                };
                if invocations > 0 { dispatch_linear(&mut pass, invocations, dimension); }
            }
        };
        run(encoder, 0, &["close_h", "close_v", "reopen_h", "reopen_v"]);
        if contiguous {
            if enclosed { encoder.clear_buffer(store, labels.flags * 4, Some((labels.marks - labels.flags) * 4)); }
            let last = windows.len() - 1;
            for at in 0..=last {
                if enclosed { encoder.clear_buffer(store, labels.marks * 4, Some((labels.seed - labels.marks) * 4)); }
                let joined: &[&str] = if at == last { &["seam_merge", "propagate_flags", "resolve_seed", "component_mask"] } else { &[] };
                run(encoder, at, &[&["initialize", "merge", "mark_exterior", "export_border"][..], joined].concat());
            }
            for at in (0..last).rev() {
                if enclosed { encoder.clear_buffer(store, labels.marks * 4, Some((labels.seed - labels.marks) * 4)); }
                run(encoder, at, &["initialize", "merge", "mark_exterior", "component_mask"]);
            }
        }
        run(encoder, 0, &["expand_h", "expand_v", "pack"]);
        Ok(region)
    }
}

fn dispatch_linear(pass: &mut wgpu::ComputePass<'_>, invocations: u64, limit: u32) {
    let groups = invocations.div_ceil(64) as u32;
    pass.dispatch_workgroups(groups.min(limit), groups.div_ceil(limit).max(1), 1);
}
