//! GPU selection options and Select › Modify: Gaussian feathers, circular
//! resizes and boolean modes, as stages over a grid of cells. A job advances
//! in chunks of bounded GPU work, one submission at a time, so refining a large
//! selection never holds the queue for a whole frame.
use super::*;
use layer_render::{SelectionMode, SelectionModify, SelectionRefinement};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use wgpu::util::DeviceExt;

/// GPU time one submission may take, and a preview's whole job, small enough
/// to share each frame with the canvas and the interface.
const CHUNK_SECONDS: f64 = 0.002;
const PREVIEW_SECONDS: f64 = 0.001;
/// Pixel taps per second before the GPU has timed a refinement: a slow
/// mobile GPU's pace.
const INITIAL_RATE: f64 = 2e9;
/// Timed chunks shorter than this mostly measure overhead.
const MIN_TIMED_TAPS: f64 = 1e5;
/// Previews use cells up to 2^MAX_LEVEL document pixels wide.
pub(super) const MAX_LEVEL: u32 = 4;
/// Every Refine operation changes coverage at most this far from its selection.
const MAX_REACH_TOTAL: f32 = 256.;
/// A preview's result stays small enough to read back within a frame.
const MAX_PREVIEW_CELLS: u64 = 1 << 20;
/// Workgroup storage in `feather_h` holds 256 pixels and this reach each side.
const MAX_REACH: u32 = 150;

const FEATHER: usize = 0;
const COMBINE: usize = 1;
const RESIZE: usize = 2;
const BOUNDS: usize = 3;
const DOWNSAMPLE: usize = 4;

/// The coverage a stage reads: the job's source, the previous stage's
/// result, or coverage the request supplied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Input {
    Source,
    Stage,
    External,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Stage {
    pub resize: i32,
    pub feather: f32,
    pub mode: SelectionMode,
    pub antialias: bool,
    pub keep_canvas_edges: bool,
    /// Maps grid pixels to incoming pixels.
    pub inverse: [f32; 6],
    pub incoming: Input,
    pub previous: Option<Input>,
}
impl Stage {
    pub fn refinement(options: &SelectionRefinement) -> Self {
        Self {
            resize: options.resize,
            feather: options.feather,
            mode: options.mode,
            antialias: options.antialias,
            keep_canvas_edges: options.keep_canvas_edges,
            inverse: options.source_to_document.inverse().map_or([1., 0., 0., 1., 0., 0.], |a| a.0),
            incoming: Input::Source,
            previous: options.previous.as_ref().map(|_| Input::External),
        }
    }
    /// Grid pixels beyond its input that the stage can change.
    fn reach(&self) -> u32 {
        if self.feather > 0. { (self.feather * 1.5).ceil() as u32 } else { self.resize.unsigned_abs() }
    }
    fn levels(&self) -> u32 {
        (self.resize.unsigned_abs() * 2 + 1).ilog2()
    }
    fn horizontal(&self) -> bool {
        self.feather > 0. || self.resize != 0
    }
    /// Taps per active pixel of the horizontal pass and of the final pass.
    fn taps(&self) -> [f64; 2] {
        let reach = f64::from(self.reach());
        if self.feather > 0. {
            [2. * reach + 1., 2. * reach + 2.]
        } else if self.resize != 0 {
            [2. * f64::from(self.levels()), 4. * reach + 2.]
        } else {
            [0., 2.]
        }
    }
}

/// Where a Select › Modify job works: cells `scale` document pixels wide from
/// `origin`, with coverage changing only within `active`. A preview at a
/// coarser scale reads the selection from that `level` of its pyramid.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ModifyPlan {
    pub scale: u32,
    pub level: u32,
    pub origin: [u32; 2],
    pub grid: [u32; 2],
    pub active: [u32; 4],
    pub stages: Vec<Stage>,
}
impl ModifyPlan {
    /// The exact job over the document, or a preview over the window around
    /// the selection on the finest cells whose job fits in `taps` and whose
    /// result stays small. Cells stay within the preview's detail, or half
    /// of every step's radius, where a result drawn bilinearly matches the
    /// exact one. The pyramid holds its first `built` levels already.
    pub fn new(modify: &SelectionModify, extent: [u32; 2], taps: f64, built: u32) -> Self {
        let window = window(modify, extent);
        let Some(detail) = modify.preview else {
            return Self::at(modify, extent, window, 0, false);
        };
        let coarsest = modify
            .steps
            .iter()
            .map(|s| if s.feather > 0. { s.feather } else { s.resize.unsigned_abs() as f32 } / 2.)
            .fold(f32::INFINITY, f32::min)
            .max(detail);
        let region = pyramid_region(modify, extent);
        let area = f64::from(region[2] - region[0]) * f64::from(region[3] - region[1]);
        let mut plan = Self::at(modify, extent, window, 0, true);
        while plan.level < MAX_LEVEL && (2u32 << plan.level) as f32 <= coarsest {
            let pyramid: f64 = (built.max(1)..=plan.level).map(|level| area / 4f64.powi(level as i32 - 1)).sum();
            if plan.cost() + pyramid <= taps && plan.cells() <= MAX_PREVIEW_CELLS {
                break;
            }
            plan = Self::at(modify, extent, window, plan.level + 1, true);
        }
        plan
    }
    fn at(modify: &SelectionModify, extent: [u32; 2], window: [u32; 4], level: u32, preview: bool) -> Self {
        let scale = 1 << level;
        let [x0, y0, x1, y1] = window;
        let (origin, grid, active) = if preview {
            let start = [x0 / scale, y0 / scale];
            let grid = [x1.div_ceil(scale) - start[0], y1.div_ceil(scale) - start[1]];
            (start.map(|v| v * scale), grid, [0, 0, grid[0], grid[1]])
        } else {
            ([0, 0], extent, window)
        };
        let offset = [origin[0] / scale, origin[1] / scale].map(|v| v as f32);
        let stages = modify
            .steps
            .iter()
            .map(|step| Stage {
                resize: if step.resize == 0 {
                    0
                } else {
                    step.resize.signum() * ((step.resize.unsigned_abs() as f32 / scale as f32).round() as i32).max(1)
                },
                feather: step.feather / scale as f32,
                mode: if step.subtract { SelectionMode::Subtract } else { SelectionMode::New },
                antialias: true,
                keep_canvas_edges: step.keep_canvas_edges,
                inverse: if step.chained { [1., 0., 0., 1., 0., 0.] } else { [1., 0., 0., 1., offset[0], offset[1]] },
                incoming: if step.chained { Input::Stage } else { Input::Source },
                previous: step.subtract.then_some(Input::Stage),
            })
            .collect();
        Self { scale, level, origin, grid, active, stages }
    }
    fn cells(&self) -> u64 {
        u64::from(self.grid[0]) * u64::from(self.grid[1])
    }
    fn cost(&self) -> f64 {
        let [x0, y0, x1, y1] = self.active;
        let area = f64::from(x1 - x0) * f64::from(y1 - y0);
        self.stages.iter().map(|s| s.taps().iter().sum::<f64>() * area).sum::<f64>() + self.cells() as f64 / 4.
    }
    /// Maps result pixels onto the document.
    pub fn placement(&self) -> layer_core::Affine {
        let s = self.scale as f32;
        layer_core::Affine([s, 0., 0., s, self.origin[0] as f32, self.origin[1] as f32])
    }
}

/// The document pixels a preview pyramid covers: every window any Refine
/// operation can need, aligned to the coarsest cells.
pub(super) fn pyramid_region(modify: &SelectionModify, [w, h]: [u32; 2]) -> [u32; 4] {
    let cell = 1 << MAX_LEVEL;
    let bounds = modify.selection.bounds();
    if modify.selection.inverted || bounds.is_empty() {
        return [0, 0, w, h];
    }
    let clamp = |v: f32, limit: u32| v.clamp(0., limit as f32) as u32;
    [
        clamp((bounds.min.x - MAX_REACH_TOTAL).floor(), w) / cell * cell,
        clamp((bounds.min.y - MAX_REACH_TOTAL).floor(), h) / cell * cell,
        clamp((bounds.max.x + MAX_REACH_TOTAL).ceil(), w),
        clamp((bounds.max.y + MAX_REACH_TOTAL).ceil(), h),
    ]
}

/// The document pixels whose coverage the steps can change. Every other pixel
/// stays unselected, except around an inverted selection.
fn window(modify: &SelectionModify, [w, h]: [u32; 2]) -> [u32; 4] {
    if modify.selection.inverted {
        return [0, 0, w, h];
    }
    let bounds = modify.selection.bounds();
    let reach: f32 = modify
        .steps
        .iter()
        .map(|s| if s.feather > 0. { (s.feather * 1.5).ceil() } else { s.resize.unsigned_abs() as f32 })
        .sum();
    let clamp = |v: f32, limit: u32| v.clamp(0., limit as f32) as u32;
    let x0 = clamp((bounds.min.x - reach).floor(), w) / 4 * 4;
    let y0 = clamp((bounds.min.y - reach).floor(), h);
    let x1 = clamp((bounds.max.x + reach).ceil(), w);
    let y1 = clamp((bounds.max.y + reach).ceil(), h);
    if bounds.is_empty() || x1 <= x0 || y1 <= y0 {
        return [0, 0, w.min(4), h.min(1)];
    }
    [x0, y0, x1, y1]
}

/// A refinement in progress: its stages over one grid, and their cursor.
pub(super) struct Job {
    extent: [u32; 2],
    active: [u32; 4],
    stages: Vec<Stage>,
    source: wgpu::Buffer,
    external: Option<wgpu::Buffer>,
    outputs: Vec<wgpu::Buffer>,
    downsamples: std::collections::VecDeque<Downsample>,
    stage: usize,
    /// The next rows of the stage's horizontal pass and of its final pass.
    rows: [u32; 2],
    block: u32,
    scratch: Option<(wgpu::Buffer, u32)>,
    weights: Option<wgpu::Buffer>,
    /// The GPU has finished every chunk submitted so far.
    idle: Arc<AtomicBool>,
    /// Pixel taps of the latest chunk.
    taps: f64,
}
/// Halve `input`, whose pixels end at `bounds`, into `output`'s `extent`
/// cells from `origin` in the input's pixels.
pub(super) struct Downsample {
    pub input: wgpu::Buffer,
    pub bounds: [u32; 2],
    pub origin: [u32; 2],
    pub output: wgpu::Buffer,
    pub extent: [u32; 2],
    pub row: u32,
}
impl Job {
    pub fn storage_bytes(&self) -> u64 {
        self.outputs.iter().map(wgpu::Buffer::size).sum::<u64>()
            + self.source.size()
            + self.external.as_ref().map_or(0, wgpu::Buffer::size)
            + self.scratch.as_ref().map_or(0, |(b, _)| b.size())
    }
    pub fn idle(&self) -> bool {
        self.idle.load(Ordering::Acquire)
    }
    pub fn extent(&self) -> [u32; 2] {
        self.extent
    }
    pub fn taps(&self) -> f64 {
        self.taps
    }
    /// Wait for the GPU to finish everything encoded so far before the next chunk.
    pub fn track(&self, encoder: &crate::submission::CommandEncoder) {
        self.idle.store(false, Ordering::Release);
        let idle = self.idle.clone();
        encoder.on_submitted_work_done(move || idle.store(true, Ordering::Release));
    }

}

pub(super) struct SelectionRefiner {
    layout: wgpu::BindGroupLayout,
    bounds_layout: wgpu::BindGroupLayout,
    pipelines: [Deferred<wgpu::ComputePipeline>; 5],
    empty: wgpu::Buffer,
    no_scratch: wgpu::Buffer,
    no_weights: wgpu::Buffer,
    /// Pixel taps per second of recently timed chunks.
    rate: f64,
    /// Chunks with GPU timestamps, by the taps each one took.
    timer: Option<(GpuFrameTimer, std::collections::BTreeMap<u64, f64>, u64)>,
    #[cfg(test)]
    pub budget: Option<f64>,
}
impl SelectionRefiner {
    pub fn new(device: &PipelineDevice, queue: &wgpu::Queue) -> Self {
        let entries: Vec<_> = (0..6)
            .map(|binding| crate::bindings::buffer(
                binding,
                wgpu::ShaderStages::COMPUTE,
                if binding == 0 || binding == 5 { wgpu::BufferBindingType::Uniform } else { wgpu::BufferBindingType::Storage { read_only: binding < 3, } },
                false,
                None,
            ))
            .collect();
        let layout = crate::bindings::layout(device, "selection options", &entries);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("selection options"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let bounds_layout = crate::bindings::layout(device, "selection bounds", &[entries[0], entries[4]]);
        let bounds_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("selection bounds"),
                bind_group_layouts: &[Some(&bounds_layout)],
                immediate_size: 0,
            });
        let shader = Deferred::wgsl(device, "selection feather and modes", include_str!("selection_refine.wgsl"));
        let pipelines = ["feather_h", "combine", "resize_h", "bounds", "downsample"].map(|entry| {
            let layout = if entry == "bounds" { &bounds_pipeline_layout } else { &pipeline_layout };
            Deferred::compute(device, entry, layout, &shader, entry)
        });
        let buffer = |label, size, usage| device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage,
            mapped_at_creation: false,
        });
        Self {
            layout,
            bounds_layout,
            pipelines,
            empty: buffer("no prior selection", 48, wgpu::BufferUsages::STORAGE),
            no_scratch: buffer("no refinement intermediate", 16, wgpu::BufferUsages::STORAGE),
            no_weights: buffer("no Gaussian weights", 608, wgpu::BufferUsages::UNIFORM),
            rate: INITIAL_RATE,
            timer: device
                .features()
                .contains(wgpu::Features::TIMESTAMP_QUERY)
                .then(|| (GpuFrameTimer::new(device, queue), Default::default(), 0)),
            #[cfg(test)]
            budget: None,
        }
    }
    /// Pixel taps one chunk may take.
    pub fn chunk(&self) -> f64 {
        #[cfg(test)]
        if let Some(taps) = self.budget {
            return taps;
        }
        self.rate * CHUNK_SECONDS
    }
    /// Pixel taps a preview's job may take.
    pub fn preview(&self) -> f64 {
        #[cfg(test)]
        if let Some(taps) = self.budget {
            return taps;
        }
        self.rate * PREVIEW_SECONDS
    }
    #[cfg(test)]
    pub fn times_chunks(&self) -> bool {
        self.timer.is_some()
    }
    /// Time the chunk about to be encoded, when the GPU has timestamps.
    pub fn begin_timing(&mut self, encoder: &mut crate::submission::CommandEncoder) -> Option<u64> {
        let (timer, _, next) = self.timer.as_mut()?;
        *next += 1;
        timer.begin_encoded(encoder, *next).then_some(*next)
    }
    pub fn end_timing(&mut self, encoder: &mut crate::submission::CommandEncoder, chunk: Option<u64>, taps: f64) {
        if let (Some(chunk), Some((timer, pending, _))) = (chunk, self.timer.as_mut()) {
            timer.end_encoded(encoder);
            pending.insert(chunk, taps);
        }
    }
    pub fn timed(&mut self, queue: &wgpu::Queue, chunk: Option<u64>) {
        if let (Some(_), Some((timer, _, _))) = (chunk, self.timer.as_mut()) {
            timer.submitted(queue);
        }
    }
    /// Learn the GPU's pace from completed chunks.
    pub fn poll_timing(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let Some((timer, pending, _)) = self.timer.as_mut() else {
            return;
        };
        timer.poll(device, queue);
        let mut samples = [GpuFrameSample::default(); 8];
        let count = timer.take_into(&mut samples);
        for sample in &samples[..count] {
            let Some(taps) = pending.remove(&sample.frame) else {
                continue;
            };
            if sample.status == 1 && sample.elapsed_ns > 0 && taps >= MIN_TIMED_TAPS {
                let rate = taps / (sample.elapsed_ns as f64 * 1e-9);
                self.rate = (0.7 * self.rate + 0.3 * rate).clamp(1e8, 1e13);
            }
        }
        if pending.len() > 64 {
            let stale = pending.keys().copied().take(pending.len() - 64).collect::<Vec<_>>();
            for key in stale {
                pending.remove(&key);
            }
        }
    }
    pub fn prepare_bounds(&self, compiler: &startup::Compiler) -> bool {
        compiler.pipeline(&self.pipelines[BOUNDS], startup::BRUSH);
        self.pipelines[BOUNDS].ready()
    }
    pub fn prepare<'a>(&self, compiler: &startup::Compiler, stages: impl IntoIterator<Item = &'a Stage>, downsample: bool) -> bool {
        let mut needed = [false, true, false, true, downsample];
        for stage in stages {
            needed[FEATHER] |= stage.feather > 0.;
            needed[RESIZE] |= stage.resize != 0;
        }
        compiler.require(
            self.pipelines.iter().zip(needed).filter(|(_, n)| *n).map(|(p, _)| p),
            startup::BRUSH,
        )
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
        let mut data = [0u32; 20];
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
        pass.set_pipeline(&self.pipelines[BOUNDS]);
        pass.set_bind_group(0, &binding, &[]);
        pass.dispatch_workgroups(h, 1, 1);
        drop(pass);
        flood::Region {
            coverage,
            bounds_offset,
        }
    }
    /// Byte coverage for `extent` pixels from `origin`, followed by space for
    /// its bounds.
    pub fn coverage_buffer(
        device: &wgpu::Device,
        encoder: &mut crate::submission::CommandEncoder,
        origin: [u32; 2],
        [w, h]: [u32; 2],
        clear: bool,
        label: &'static str,
    ) -> wgpu::Buffer {
        let words = u64::from(w.div_ceil(4)) * u64::from(h) * 4;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: (32 + words + 32).next_multiple_of(16),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let header: Vec<_> = [origin[0], origin[1], w, h, 0, 2, 0, 0].into_iter().flat_map(u32::to_ne_bytes).collect();
        let header = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("selection coverage header"),
            contents: &header,
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        encoder.copy_buffer_to_buffer(&header, 0, &buffer, 0, 32);
        if clear && words > 0 {
            encoder.clear_buffer(&buffer, 32, Some(words));
        }
        buffer
    }
    /// A job over `stages` of an `extent` grid. Coverage outside `active`
    /// stays unselected. The `downsamples` run first, in order.
    #[allow(clippy::too_many_arguments)]
    pub fn job(
        &self,
        device: &wgpu::Device,
        encoder: &mut crate::submission::CommandEncoder,
        extent: [u32; 2],
        active: [u32; 4],
        stages: Vec<Stage>,
        source: wgpu::Buffer,
        external: Option<wgpu::Buffer>,
        downsamples: Vec<Downsample>,
    ) -> Result<Job, GpuRasterError> {
        if extent.contains(&0) || stages.is_empty() {
            return Err(GpuRasterError::InvalidExtent);
        }
        let [w, h] = extent;
        let limit = device.limits();
        if 64 + u64::from(w.div_ceil(4)) * u64::from(h) * 4 > limit.max_storage_buffer_binding_size
            || w.div_ceil(256) > limit.max_compute_workgroups_per_dimension
            || h > limit.max_compute_workgroups_per_dimension
            || stages.iter().any(|s| s.feather > 0. && s.reach() > MAX_REACH)
        {
            return Err(GpuRasterError::SizeOverflow);
        }
        let clear = active != [0, 0, w, h];
        let outputs = (0..stages.len().min(2))
            .map(|_| Self::coverage_buffer(device, encoder, [0; 2], extent, clear, "8-bit selection history"))
            .collect();
        Ok(Job {
            extent,
            active,
            rows: [active[1]; 2],
            stages,
            source,
            external,
            outputs,
            downsamples: downsamples.into(),
            stage: 0,
            block: 0,
            scratch: None,
            weights: None,
            idle: Arc::new(AtomicBool::new(true)),
            taps: 0.,
        })
    }
    fn dispatch(
        &self,
        device: &wgpu::Device,
        encoder: &mut crate::submission::CommandEncoder,
        pipeline: usize,
        params: &[u32; 20],
        buffers: [&wgpu::Buffer; 5],
        groups: [u32; 2],
    ) {
        let bytes: Vec<_> = params.iter().flat_map(|v| v.to_ne_bytes()).collect();
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("selection options"),
            contents: &bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let entries: Vec<_> = std::iter::once(&params)
            .chain(buffers)
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect();
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("selection options"),
            layout: &self.layout,
            entries: &entries,
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("refine selection"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipelines[pipeline]);
        pass.set_bind_group(0, &binding, &[]);
        pass.dispatch_workgroups(groups[0], groups[1], 1);
    }
    fn weights(device: &wgpu::Device, feather: f32) -> wgpu::Buffer {
        // Symmetric normalized Gaussian, computed once instead of exp() and
        // normalization for every tap of every image pixel. The widest reach
        // needs 151 nonnegative distances, packed into 38 uniform vec4s.
        let mut weights = [0f32; 152];
        let reach = (feather * 1.5).ceil() as usize;
        let sigma = (feather * 0.5).max(0.001);
        for (d, weight) in weights.iter_mut().enumerate().take(reach + 1) {
            *weight = (-0.5 * (d * d) as f32 / (sigma * sigma)).exp();
        }
        let total = weights[0] + 2. * weights[1..].iter().sum::<f32>();
        let bytes: Vec<_> = weights
            .into_iter()
            .flat_map(|v| (v / total).to_ne_bytes())
            .collect();
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("selection Gaussian weights"),
            contents: &bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        })
    }
    /// Encode at most about one chunk of `job`; its result once complete.
    pub fn advance(
        &self,
        device: &wgpu::Device,
        encoder: &mut crate::submission::CommandEncoder,
        job: &mut Job,
    ) -> Result<Option<flood::Region>, GpuRasterError> {
        let budget = self.chunk();
        let mut spent = 0.;
        let result = self.encode_chunk(device, encoder, job, budget, &mut spent);
        job.taps = spent;
        result
    }
    fn encode_chunk(
        &self,
        device: &wgpu::Device,
        encoder: &mut crate::submission::CommandEncoder,
        job: &mut Job,
        budget: f64,
        spent: &mut f64,
    ) -> Result<Option<flood::Region>, GpuRasterError> {
        while let Some(d) = job.downsamples.front_mut() {
            let [w, h] = d.extent;
            let row_taps = f64::from(w) * 4.;
            while d.row < h {
                if *spent >= budget {
                    return Ok(None);
                }
                let rows = (((budget - *spent) / row_taps) as u32).clamp(1, h - d.row);
                let mut params = [0u32; 20];
                params[..2].copy_from_slice(&d.extent);
                params[8] = (d.origin[0] as f32).to_bits();
                params[9] = (d.origin[1] as f32).to_bits();
                params[10] = 2f32.to_bits();
                params[13] = d.row;
                params[14] = d.row + rows;
                params[18] = d.bounds[0];
                params[19] = d.bounds[1];
                self.dispatch(device, encoder, DOWNSAMPLE, &params,
                    [&d.input, &self.empty, &self.no_scratch, &d.output, &self.no_weights],
                    [w.div_ceil(4).div_ceil(64), rows]);
                d.row += rows;
                *spent += f64::from(rows) * row_taps;
            }
            job.downsamples.pop_front();
        }
        let [w, h] = job.extent;
        let [x0, y0, x1, y1] = job.active;
        let width = f64::from(x1 - x0);
        while let Some(stage) = job.stages.get(job.stage).copied() {
            let reach = stage.reach();
            let [h_taps, v_taps] = stage.taps().map(|t| t * width);
            if job.scratch.is_none() {
                job.block = if v_taps > 0. { ((budget / v_taps) as u32).clamp(1, (y1 - y0).max(1)) } else { (y1 - y0).max(1) };
                let ring = job.block + 2 * reach + 1;
                let bytes = if stage.feather > 0. {
                    u64::from(w) * u64::from(ring) * 4 + 16
                } else if stage.resize != 0 {
                    u64::from(stage.levels()) * u64::from(w.div_ceil(4)) * u64::from(ring) * 4
                } else {
                    16
                };
                if bytes > device.limits().max_storage_buffer_binding_size {
                    return Err(GpuRasterError::SizeOverflow);
                }
                job.scratch = Some((device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("selection refinement intermediate"),
                    size: bytes,
                    usage: wgpu::BufferUsages::STORAGE,
                    mapped_at_creation: false,
                }), ring));
                job.weights = (stage.feather > 0.).then(|| Self::weights(device, stage.feather));
            }
            let (scratch, ring) = job.scratch.as_ref().unwrap();
            let input = |input| match input {
                Input::Source => &job.source,
                Input::Stage => &job.outputs[(job.stage + 1) % 2],
                Input::External => job.external.as_ref().unwrap_or(&self.empty),
            };
            let buffers = [
                input(stage.incoming),
                stage.previous.map_or(&self.empty, input),
                scratch,
                &job.outputs[job.stage % 2],
                job.weights.as_ref().unwrap_or(&self.no_weights),
            ];
            let mode = match stage.mode {
                SelectionMode::New => 0,
                SelectionMode::Add => 1,
                SelectionMode::Subtract => 2,
                SelectionMode::Intersect => 3,
            };
            let mut params = [0u32; 20];
            params[..4].copy_from_slice(&[w, h, mode, u32::from(stage.antialias) | u32::from(stage.keep_canvas_edges) << 1]);
            for (dst, v) in params[4..12].iter_mut().zip(stage.inverse.into_iter().chain([stage.feather, stage.resize as f32])) {
                *dst = v.to_bits();
            }
            params[15] = *ring;
            params[16..].copy_from_slice(&job.active);
            let words = (x1 - x0).div_ceil(4);
            let [done, next] = &mut job.rows;
            while *next < y1 {
                if *spent >= budget {
                    return Ok(None);
                }
                let needed = y1.min(*next + job.block + reach);
                if stage.horizontal() && *done < needed {
                    let rows = (((budget - *spent) / h_taps) as u32).clamp(1, needed - *done);
                    params[13] = *done;
                    params[14] = *done + rows;
                    if stage.feather > 0. {
                        self.dispatch(device, encoder, FEATHER, &params, buffers, [(x1 - x0).div_ceil(256), rows]);
                    } else {
                        for level in 1..=stage.levels() {
                            params[12] = level;
                            self.dispatch(device, encoder, RESIZE, &params, buffers, [words.div_ceil(64), rows]);
                        }
                        params[12] = 0;
                    }
                    *done += rows;
                    *spent += f64::from(rows) * h_taps;
                } else {
                    let end = y1.min(*next + job.block);
                    params[13] = *next;
                    params[14] = end;
                    self.dispatch(device, encoder, COMBINE, &params, buffers, [words.div_ceil(64), end - *next]);
                    *spent += f64::from(end - *next) * v_taps;
                    *next = end;
                }
            }
            job.stage += 1;
            job.rows = [y0; 2];
            job.scratch = None;
            job.weights = None;
        }
        if *spent > 0. && *spent + f64::from(w) * f64::from(h) / 4. > budget {
            return Ok(None);
        }
        *spent += f64::from(w) * f64::from(h) / 4.;
        let output = job.outputs[(job.stages.len() + 1) % 2].clone();
        Ok(Some(self.bounds_only(device, encoder, job.extent, output)))
    }
}
