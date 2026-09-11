//! Hardware-test instrumentation only; absent from application builds.
use layer_render_wgpu::WgpuRasterizer;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;

#[derive(Default)]
pub struct Stats {
    pub input_cpu: Vec<f64>,
    pub input_handler_cpu: Vec<f64>,
    pub frame_handler_cpu: Vec<f64>,
    pub wake_lateness: Vec<f64>,
    /// Frame id, acquire/configure ms, render+present CPU ms, total ms, enqueue ns.
    pub cpu: Vec<[f64; 5]>,
    /// Frame id, composition, encode, queue submit, feedback, present ms.
    pub cpu_stages: Vec<[f64; 6]>,
    /// Frame id, GPU elapsed ms (timestamps, not callback arrival time).
    pub gpu: Vec<[f64; 2]>,
    /// Frame id, presentation ns, refresh ns, presented=1/discarded=0.
    pub presented: Vec<[u64; 4]>,
    pub overview_revisions: Vec<u64>,
    pub overview_frames: usize,
}
struct Slot {
    query: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    free: Arc<AtomicBool>,
}
pub struct Timing {
    slots: Vec<Slot>,
    active: Option<usize>,
    stats: Arc<Mutex<Stats>>,
    id: u64,
    start: Instant,
    acquired: Instant,
    queued_ns: u64,
    stages: std::cell::Cell<[f64; 4]>,
}
impl Timing {
    pub fn overview(&self, revision: Option<u64>) {
        if let Some(revision) = revision {
            let mut stats = self.stats.lock().unwrap();
            stats.overview_frames += 1;
            if stats.overview_revisions.last() != Some(&revision) {
                stats.overview_revisions.push(revision);
            }
        }
    }
    pub fn new(device: &wgpu::Device, stats: Arc<Mutex<Stats>>) -> Self {
        let enabled = device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);
        let slots = (0..if enabled { 4 } else { 0 })
            .map(|_| Slot {
                query: device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("benchmark timestamps"),
                    ty: wgpu::QueryType::Timestamp,
                    count: 2,
                }),
                resolve: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("benchmark resolve"),
                    size: 16,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }),
                readback: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("benchmark timestamps only"),
                    size: 16,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                free: Arc::new(AtomicBool::new(true)),
            })
            .collect();
        Self {
            slots,
            active: None,
            stats,
            id: 0,
            start: Instant::now(),
            acquired: Instant::now(),
            queued_ns: 0,
            stages: Default::default(),
        }
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn begin(&mut self, queued_ns: u64) {
        self.id += 1;
        self.start = Instant::now();
        self.queued_ns = queued_ns;
        self.stages.set([0.; 4]);
    }
    pub fn mark(&self, stage: usize) {
        let mut stages = self.stages.get();
        stages[stage] = self.acquired.elapsed().as_secs_f64() * 1000.;
        self.stages.set(stages);
    }
    pub fn acquired(&mut self, renderer: &WgpuRasterizer) {
        self.acquired = Instant::now();
        self.active = self
            .slots
            .iter()
            .position(|s| s.free.swap(false, Ordering::AcqRel));
        if let Some(index) = self.active {
            let mut encoder = renderer
                .device()
                .create_command_encoder(&Default::default());
            encoder.write_timestamp(&self.slots[index].query, 0);
            renderer.queue().submit([encoder.finish()]);
        }
    }
    pub fn encoded(&self, encoder: &mut wgpu::CommandEncoder) {
        if let Some(index) = self.active {
            let slot = &self.slots[index];
            encoder.write_timestamp(&slot.query, 1);
            encoder.resolve_query_set(&slot.query, 0..2, &slot.resolve, 0);
            encoder.copy_buffer_to_buffer(&slot.resolve, 0, &slot.readback, 0, 16);
        }
    }
    pub fn end(&self, renderer: &WgpuRasterizer) {
        let end = self.acquired.elapsed().as_secs_f64() * 1000.;
        let [compose, encode, submit, feedback] = self.stages.get();
        let mut stats = self.stats.lock().unwrap();
        stats.cpu_stages.push([
            self.id as f64,
            compose,
            encode - compose,
            submit - encode,
            feedback - submit,
            end - feedback,
        ]);
        stats.cpu.push([
            self.id as f64,
            self.acquired.duration_since(self.start).as_secs_f64() * 1000.0,
            end,
            self.start.elapsed().as_secs_f64() * 1000.0,
            self.queued_ns as f64,
        ]);
        drop(stats);
        if let Some(index) = self.active {
            let slot = &self.slots[index];
            let buffer = slot.readback.clone();
            let stats = self.stats.clone();
            let free = slot.free.clone();
            let period = renderer.queue().get_timestamp_period() as f64;
            let id = self.id;
            slot.readback
                .map_async(wgpu::MapMode::Read, .., move |result| {
                    if result.is_ok() {
                        let bytes = buffer.get_mapped_range(..).unwrap();
                        let start = u64::from_le_bytes(bytes[..8].try_into().unwrap());
                        let end = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
                        stats.lock().unwrap().gpu.push([
                            id as f64,
                            end.saturating_sub(start) as f64 * period / 1_000_000.0,
                        ]);
                        drop(bytes);
                        buffer.unmap();
                    }
                    free.store(true, Ordering::Release);
                });
        }
    }
    pub fn presented(&self, samples: Vec<[u64; 4]>) {
        if !samples.is_empty() {
            self.stats.lock().unwrap().presented.extend(samples);
        }
    }
}
