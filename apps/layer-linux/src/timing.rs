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
    /// Frame id, acquire/configure elapsed ms, render+present ms, total ms, enqueue ns.
    pub cpu: Vec<[f64; 5]>,
    /// Frame id, composition, encode, queue submit, feedback, present elapsed ms.
    pub cpu_stages: Vec<[f64; 6]>,
    /// Frame id, acquire/configure, composition, encode, submit, feedback, present
    /// actual thread CPU ms. Unlike elapsed time, excludes waiting/descheduling.
    pub thread_cpu: Vec<[f64; 7]>,
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
    start_cpu: f64,
    acquired_cpu: f64,
    queued_ns: u64,
    stages: std::cell::Cell<[[f64; 2]; 4]>,
}

fn thread_cpu_ms() -> f64 {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // Test-only Linux worker instrumentation; no process-wide CPU counters.
    assert_eq!(
        unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) },
        0
    );
    time.tv_sec as f64 * 1000. + time.tv_nsec as f64 / 1_000_000.
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
        let enabled = std::env::var("LAYER_PACING_GPU_TIMESTAMPS").as_deref() != Ok("0")
            && device
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
            start_cpu: 0.,
            acquired_cpu: 0.,
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
        self.start_cpu = thread_cpu_ms();
        self.queued_ns = queued_ns;
        self.stages.set([[0.; 2]; 4]);
    }
    pub fn mark(&self, stage: usize) {
        let mut stages = self.stages.get();
        stages[stage] = [
            self.acquired.elapsed().as_secs_f64() * 1000.,
            thread_cpu_ms() - self.acquired_cpu,
        ];
        self.stages.set(stages);
    }
    pub fn acquired(&mut self, renderer: &WgpuRasterizer) {
        self.acquired = Instant::now();
        self.acquired_cpu = thread_cpu_ms();
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
        let end_cpu = thread_cpu_ms() - self.acquired_cpu;
        let stages = self.stages.get();
        let [compose, encode, submit, feedback] = stages.map(|s| s[0]);
        let mut stats = self.stats.lock().unwrap();
        stats.cpu_stages.push([
            self.id as f64,
            compose,
            encode - compose,
            submit - encode,
            feedback - submit,
            end - feedback,
        ]);
        let [compose, encode, submit, feedback] = stages.map(|s| s[1]);
        stats.thread_cpu.push([
            self.id as f64,
            self.acquired_cpu - self.start_cpu,
            compose,
            encode - compose,
            submit - encode,
            feedback - submit,
            end_cpu - feedback,
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
