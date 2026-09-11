//! Optional GPU timestamps with three reusable asynchronous readback slots.
//! Dropping a sample is preferable to delaying the drawing queue.
use layer_render::{RendererTelemetry, TimingSamples};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
struct Slot {
    query: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    busy: Arc<AtomicBool>,
}
pub(super) struct Telemetry {
    pub enabled: bool,
    pub cpu: TimingSamples,
    gpu: Arc<Mutex<TimingSamples>>,
    slots: Vec<Slot>,
    active: Option<usize>,
    period: f32,
}
impl Telemetry {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            enabled: false,
            cpu: TimingSamples::default(),
            gpu: Arc::default(),
            slots: Vec::new(),
            active: None,
            period: if device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
                queue.get_timestamp_period()
            } else {
                0.
            },
        }
    }
    pub fn begin(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder) {
        self.active = None;
        if !self.enabled || self.period == 0. {
            return;
        }
        if self.slots.is_empty() {
            for _ in 0..3 {
                self.slots.push(Slot {
                    query: device.create_query_set(&wgpu::QuerySetDescriptor {
                        label: Some("renderer timing"),
                        ty: wgpu::QueryType::Timestamp,
                        count: 2,
                    }),
                    resolve: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("timestamp resolve"),
                        size: 256,
                        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                        mapped_at_creation: false,
                    }),
                    read: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("timestamp readback"),
                        size: 16,
                        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                    busy: Arc::new(AtomicBool::new(false)),
                });
            }
        }
        if let Some((i, slot)) = self
            .slots
            .iter()
            .enumerate()
            .find(|(_, s)| !s.busy.load(Ordering::Acquire))
        {
            slot.busy.store(true, Ordering::Release);
            self.active = Some(i);
            let _pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("renderer timestamp start"),
                timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                    query_set: &slot.query,
                    beginning_of_pass_write_index: Some(0),
                    end_of_pass_write_index: None,
                }),
            });
        }
    }
    pub fn end(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(i) = self.active else {
            return;
        };
        let slot = &self.slots[i];
        {
            let _pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("renderer timestamp end"),
                timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                    query_set: &slot.query,
                    beginning_of_pass_write_index: None,
                    end_of_pass_write_index: Some(1),
                }),
            });
        }
        encoder.resolve_query_set(&slot.query, 0..2, &slot.resolve, 0);
        encoder.copy_buffer_to_buffer(&slot.resolve, 0, &slot.read, 0, 16);
    }
    pub fn submitted(&mut self) {
        let Some(i) = self.active.take() else {
            return;
        };
        let slot = &self.slots[i];
        let buffer = slot.read.clone();
        let busy = slot.busy.clone();
        let samples = self.gpu.clone();
        let period = self.period;
        slot.read.map_async(wgpu::MapMode::Read, .., move |result| {
            if result.is_ok() {
                if let Ok(bytes) = buffer.get_mapped_range(..) {
                    let begin = u64::from_le_bytes(bytes[..8].try_into().unwrap());
                    let end = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
                    if begin > 0
                        && end > begin
                        && let Ok(mut samples) = samples.lock()
                    {
                        samples.push((end - begin) as f32 * period / 1_000_000.);
                    }
                }
                buffer.unmap();
            }
            busy.store(false, Ordering::Release);
        });
    }
    pub fn snapshot(&self) -> RendererTelemetry {
        RendererTelemetry {
            cpu: self.cpu.clone(),
            gpu: self.gpu.try_lock().map(|g| g.clone()).unwrap_or_default(),
            gpu_timestamps: self.period > 0.,
            ..Default::default()
        }
    }
}
