//! GPU connected regions with immutable packed coverage. Readback is a single
//! asynchronous history snapshot per request, never part of live brush work.
use super::*;
use layer_render::RegionRefinement;
use wgpu::util::DeviceExt;

#[path = "flood_lines.rs"]
mod lines;
pub(super) use lines::Input;
const WINDOW: u32 = 2048;
const PARAMETER_BYTES: u64 = 96;

pub(super) struct Flood {
    layout: wgpu::BindGroupLayout,
    pipelines: std::collections::HashMap<&'static str, Deferred<wgpu::ComputePipeline>>,
    labels: Option<wgpu::Buffer>,
    capacity: u64,
    empty: wgpu::Buffer,
    lines: lines::Lines,
}
const STAGES: [&str; 13] = ["initialize", "merge", "mark_exterior", "export_border", "seam_merge", "propagate_flags", "resolve_seed", "component_mask", "finish_attempt", "save_base", "restore_base", "restore_components", "pack"];
fn stages(refinement: RegionRefinement, enclosed: bool, contiguous: bool) -> impl Iterator<Item = &'static str> {
    STAGES.into_iter().filter(move |entry| match *entry {
        "resolve_seed" | "finish_attempt" => contiguous && !enclosed,
        "save_base" | "restore_base" => refinement.close_gaps && contiguous,
        "restore_components"=>enclosed,
        _=>true,
    })
}
pub(super) struct Region {
    pub coverage: wgpu::Buffer,
    pub bounds_offset: u64,
    pub underfill: Option<(wgpu::Buffer, u64)>,
}
impl Region {
    pub fn storage_bytes(&self) -> u64 {
        self.coverage.size() + self.underfill.as_ref().filter(|(buffer,_)|buffer!=&self.coverage).map_or(0,|(buffer,_)|buffer.size())
    }
}
/// Word offsets of the label storage: window parents or a bit plane, border
/// slots, exterior flags, window marks and the seed.
#[derive(Clone, Copy)]
struct Labels { side: u32, grid: [u32; 2], slots: u64, flags: u64, marks: u64, seed: u64, words: u64 }
impl Labels {
    fn new(extent: [u32; 2], limits: &wgpu::Limits) -> Option<Self> {
        let [w, h] = extent;
        let limit = limits.max_storage_buffer_binding_size.min(limits.max_buffer_size);
        let side = (5..=WINDOW.ilog2()).rev().map(|n| 1u32 << n).find(|side| *side>160 && u64::from(*side).pow(2) * 32 <= limit)? - 160;
        let grid = extent.map(|v| v.div_ceil(side));
        let plane = u64::from(w.div_ceil(32)) * u64::from(h);
        let slots = plane.max(u64::from(side).pow(2));
        let flags = slots + 3 * u64::from(grid[0]) * u64::from(grid[1]) * 4 * u64::from(side);
        let marks = flags + 2 * (flags - slots).div_ceil(32);
        let seed = marks + 2 * u64::from(side).pow(2) / 32;
        let labels = Self { side, grid, slots, flags, marks, seed, words: seed + 25 };
        let packed = u64::from(w.div_ceil(8)) * u64::from(h);
        let coverage = (96 + packed.max(4 * plane) * 4 + packed * 4).next_multiple_of(16);
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
        && lines::fits(extent,limits)
}
impl Flood {
    pub fn storage_bytes(&self) -> u64 {
        self.capacity + self.empty.size() + self.lines.storage_bytes()
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
            lines:lines::Lines::new(device),
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
    pub fn prepare(&self, compiler: &startup::Compiler, refinement: RegionRefinement, enclosed: bool, contiguous: bool, priority:u8) -> bool {
        self.lines.prepare(compiler,priority) & compiler.require(stages(refinement, enclosed, contiguous).map(|entry| &self.pipelines[entry]), priority)
    }
    pub fn build_lines(&mut self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,extent:[u32;2],paper:([f32;4],u32,bool),budget:f64,demand:bool)->Result<(bool,f64),GpuRasterError> {
        let device=input.device;
        let labels=Labels::new(extent,&device.limits()).ok_or(GpuRasterError::SizeOverflow)?;
        let windows:Vec<_>=labels.windows(extent).collect();
        if !self.lines.building() {self.lines.allocate(device,extent,labels.side,&windows)?;}
        self.lines.build(input,encoder,paper,budget,demand)
    }
    pub fn needs_seals(&self,close:bool)->bool {self.lines.needs_seals(close)}
    pub fn prepare_seals(&mut self,device:&PipelineDevice,encoder:&mut crate::submission::CommandEncoder,paper:&wgpu::TextureView,close:bool,budget:f64)->Result<(bool,f64),GpuRasterError> {self.lines.prepare_seals(device,encoder,paper,close,budget)}
    pub fn begin_tap(&mut self) {self.lines.begin_tap();}
    pub fn lines_ready(&mut self,demand:bool)->Result<bool,GpuRasterError> {self.lines.ready(demand)}
    pub fn lines_submitted(&self)->bool {self.lines.submitted()}
    pub fn lines_discarded(&self)->bool {self.lines.discarded()}
    pub fn cancel_lines(&mut self) {self.lines.cancel();}
    pub fn check_seeds(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,old:&wgpu::Buffer,new:&wgpu::Buffer,position:[u32;2]) {
        self.lines.check_seeds(input,encoder,old,new,position);
    }
    pub fn copy_stale(&self,encoder:&mut crate::submission::CommandEncoder,output:&wgpu::Buffer,offset:u64) {self.lines.copy_stale(encoder,output,offset);}
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
        paper: &wgpu::TextureView,
        stop: Option<&wgpu::Buffer>,
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
        let scratch_words = words.max(mask_bytes);
        let band = matches!(refinement.edges,layer_render::RegionEdges::LineMiddle);
        let bounds_offset = 32 + scratch_words * 4;
        let coverage = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("connected coverage"),
                size: (bounds_offset + 32 + if band {32+words*4}else{0}).next_multiple_of(16),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        let region = Region {
            underfill: band.then(||(coverage.clone(),bounds_offset+64)),
            coverage,
            bounds_offset,
        };
        let summary: Vec<u8> = [0, 0, w, h, 0, 1, 0, 0, w, h, 0, 0, 0].into_iter().flat_map(u32::to_ne_bytes).collect();
        let summary = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("flood summary"),
            contents: &summary,
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        encoder.copy_buffer_to_buffer(&summary, 0, &region.coverage, 0, 32);
        encoder.copy_buffer_to_buffer(&summary, 32, &region.coverage, region.bounds_offset, 20);
        if band {encoder.copy_buffer_to_buffer(&summary,32,&region.coverage,region.bounds_offset+32,20);}
        let stride = PARAMETER_BYTES.next_multiple_of(u64::from(limit.min_uniform_buffer_offset_alignment));
        let windows: Vec<_> = labels.windows(extent).collect();
        let attempts:u32=if refinement.close_gaps && contiguous && self.lines.has_seals() {3}else{1};
        let mut data = vec![0u8; (stride * windows.len() as u64 * u64::from(attempts) * 4) as usize];
        let offsets = [labels.slots, labels.flags, labels.marks, labels.seed].map(|v| v as u32);
        for (parameter,((index, window), entry)) in windows.iter().cycle().zip(data.chunks_mut(stride as usize)).enumerate() {
            let params = [
                w, h, seed[0], seed[1],
                tolerance.to_bits(), if band {(scratch_words+16) as u32}else{0}, 0, f32::from(refinement.smooth_edges).to_bits(),
                (parameter/windows.len()/attempts as usize) as u32, u32::from(!contiguous), u32::from(enclosed), (parameter/windows.len()) as u32%attempts,
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
        let make_group=|selection:&wgpu::Buffer,coverage:&wgpu::Buffer|device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("flood request"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniform, offset: 0, size: wgpu::BufferSize::new(PARAMETER_BYTES) }) },
                wgpu::BindGroupEntry { binding: 2, resource: store.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: coverage.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: selection.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: classified.as_entire_binding() },
            ],
        });
        let group=make_group(selection.unwrap_or(&self.empty),&region.coverage);
        let pack_group=enclosed.then(||make_group(&self.empty,&region.coverage));
        let base_group=(attempts>1).then(||make_group(&self.empty,self.lines.window_state()));
        let enabled: Vec<_> = stages(refinement, enclosed, contiguous).collect();
        let dimension = limit.max_compute_workgroups_per_dimension;
        let pipelines = &self.pipelines;
        let run = |encoder: &mut crate::submission::CommandEncoder, mode:usize, attempt:usize, at: usize, entries: &[&'static str]| {
            let [_, _, ww, wh] = windows[at].1;
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("connected region"),
                timestamp_writes: None,
            });
            for entry in entries.iter().filter(|entry| enabled.contains(entry)) {
                let binding=match *entry {"pack"=>pack_group.as_ref().unwrap_or(&group),"save_base"|"restore_base"=>base_group.as_ref().unwrap(),_=>&group};
                pass.set_bind_group(0,binding,&[(stride*((mode*attempts as usize+attempt)*windows.len()+at) as u64) as u32]);
                pass.set_pipeline(&pipelines[entry]);
                let invocations = match *entry {
                    "initialize" => { pass.dispatch_workgroups(ww.div_ceil(if mode==1 {512}else{16}), wh.div_ceil(16), 1); continue; }
                    "resolve_seed" | "finish_attempt" => { pass.dispatch_workgroups(1, 1, 1); continue; }
                    "merge" if mode==1 => u64::from(ww.div_ceil(32))*u64::from(wh),
                    "save_base"|"restore_base" => u64::from(ww)*u64::from(wh),
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
        let input=Input {device,mask:classified,paper};
        encoder.clear_buffer(store,labels.seed*4,Some(25*4));
        encoder.clear_buffer(store,labels.flags*4,Some((labels.marks-labels.flags)*4));
        for attempt in 0..attempts {
            self.lines.barriers(&input,encoder,refinement.close_gaps,16*(attempt+1),[0,0,(labels.seed+8*u64::from(attempt)) as u32,u32::from(enclosed)],store);
            if let Some(stop)=stop {self.lines.intersect(&input,encoder,stop);}
            self.lines.snap_seed(&input,encoder,store,(labels.seed+8*u64::from(attempt)) as u32,seed);
            if attempts>1 {self.lines.barrier_plane(&input,encoder,&region.coverage,store,attempt,0);}
        }
        let last=windows.len()-1;
        for at in 0..=last {
            run(encoder,if attempts>1 {3}else{0},0,at,&["initialize","merge"]);
            if attempts>1 {run(encoder,3,0,at,&["save_base"]);}
            for attempt in 0..attempts as usize {
                encoder.clear_buffer(store,labels.marks*4,Some((labels.seed-labels.marks)*4));
                if attempts>1 {run(encoder,1,attempt,at,&["restore_base","initialize","merge"]);}
                run(encoder,if attempts>1 {1}else{0},attempt,at,&["mark_exterior","export_border"]);
            }
        }
        for attempt in 0..attempts as usize {run(encoder,if attempts>1 {1}else{0},attempt,0,&["seam_merge"]);}
        run(encoder,0,0,0,&["propagate_flags"]);
        for attempt in (0..attempts as usize).rev() {run(encoder,if attempts>1 {1}else{0},attempt,0,&["resolve_seed","finish_attempt"]);}
        if attempts>1 && !enclosed {self.lines.barrier_plane(&input,encoder,&region.coverage,store,(labels.seed+24) as u32,1);}
        for attempt in 0..if enclosed {attempts as usize}else{1} {
            if attempts>1 && enclosed && attempt==2 {encoder.clear_buffer(&region.coverage,32,Some(mask_bytes));}
            if attempts==1 {run(encoder,0,attempt,last,&["component_mask"]);}
            for at in (0..if attempts==1 {last}else{windows.len()}).rev() {
                encoder.clear_buffer(store,labels.marks*4,Some((labels.seed-labels.marks)*4));
                run(encoder,if attempts>1 {2}else{0},attempt,at,&["initialize","merge","mark_exterior","component_mask"]);
            }
        }
        self.lines.thin(&input,encoder,&region.coverage,if attempts>1 && !enclosed {3}else{0});
        if enclosed {run(encoder,if attempts>1 {2}else{0},0,0,&["restore_components"]);}
        if band {
            self.lines.barrier_plane(&input,encoder,&region.coverage,store,0,2);
            self.lines.assign(&input,encoder);
        }
        if let layer_render::RegionEdges::GrowShrink(value)=refinement.edges && value!=0 {self.lines.grow(&input,encoder,value);}
        if let Some(stop)=stop {self.lines.intersect(&input,encoder,stop);}
        run(encoder, 0, 0, 0, &["pack"]);
        Ok(region)
    }
}

fn dispatch_linear(pass: &mut wgpu::ComputePass<'_>, invocations: u64, limit: u32) {
    let groups = invocations.div_ceil(64) as u32;
    pass.dispatch_workgroups(groups.min(limit), groups.div_ceil(limit).max(1), 1);
}
