//! Optional GPU queue spans for native presenters. Three nonblocking slots;
//! timestamps bracket submitted GPU work, including gaps between submissions.
//! This is neither CPU work time nor drawable presentation/input latency.
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct GpuFrameSample {
    pub frame: u64,
    pub elapsed_ns: u64,
    /// 1: valid, 2: map/read failure, 3: invalid timestamp order/range.
    pub status: u64,
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct GpuFrameTimingStats {
    /// 0: not initialized, 1: supported, 2: unavailable on this device.
    pub support: u64,
    pub requested: u64,
    pub skipped: u64,
    pub invalid: u64,
    pub pending: u64,
}

// Metal can omit timestamp writes for empty passes. Keep a tiny storage write
// in each marker pass. Created only when telemetry is enabled; its GPU overhead
// is part of the measured span, and must be calibrated by the caller.
struct TimestampMarker {
    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
}
impl TimestampMarker {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("timestamp marker"),
            source: wgpu::ShaderSource::Wgsl("@group(0) @binding(0) var<storage, read_write> marker: u32; @compute @workgroup_size(1) fn main() { marker = marker + 1u; }".into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("timestamp marker"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("timestamp marker"),
            size: 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("timestamp marker"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        Self {
            pipeline,
            bind_group,
        }
    }
    pub fn write(&self, encoder: &mut wgpu::CommandEncoder, query: &wgpu::QuerySet, index: u32) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("timestamp marker"),
            timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                query_set: query,
                beginning_of_pass_write_index: Some(index),
                end_of_pass_write_index: None,
            }),
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
}

struct Slot {
    query: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    busy: Arc<AtomicBool>,
    gpu_done: Arc<AtomicBool>,
    awaiting_resolve: Option<u64>,
}

pub struct GpuFrameTimer {
    slots: Vec<Slot>,
    active: Option<(usize, u64)>,
    period: f64,
    marker: Option<TimestampMarker>,
    ready: Arc<Mutex<VecDeque<GpuFrameSample>>>,
    invalid: Arc<AtomicU64>,
    omitted: Arc<AtomicU64>,
    requested: u64,
    skipped: u64,
}
impl GpuFrameTimer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let period = if device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            f64::from(queue.get_timestamp_period())
        } else {
            0.
        };
        let slots = if period > 0. {
            (0..3)
                .map(|_| Slot {
                    query: device.create_query_set(&wgpu::QuerySetDescriptor {
                        label: Some("frame GPU timestamps"),
                        ty: wgpu::QueryType::Timestamp,
                        count: 2,
                    }),
                    resolve: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("frame timestamp resolve"),
                        size: 256,
                        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                        mapped_at_creation: false,
                    }),
                    read: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("frame timestamp readback"),
                        size: 16,
                        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                    busy: Arc::new(AtomicBool::new(false)),
                    gpu_done: Arc::new(AtomicBool::new(false)),
                    awaiting_resolve: None,
                })
                .collect()
        } else {
            Vec::new()
        };
        Self {
            slots,
            active: None,
            period,
            marker: (period > 0.).then(|| TimestampMarker::new(device)),
            ready: Arc::new(Mutex::new(VecDeque::with_capacity(256))),
            invalid: Arc::default(),
            omitted: Arc::default(),
            requested: 0,
            skipped: 0,
        }
    }
    /// Pair every successful begin with end, including an aborted render attempt.
    /// Saturation drops the observation; it never waits for a readback slot.
    pub fn begin(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: u64) -> bool {
        self.requested += 1;
        let slot = if self.active.is_none() {
            self.slots
                .iter()
                .enumerate()
                .find(|(_, slot)| !slot.busy.load(Ordering::Acquire))
        } else {
            None
        };
        let Some((index, slot)) = slot else {
            self.skipped += 1;
            return false;
        };
        slot.busy.store(true, Ordering::Release);
        self.active = Some((index, frame));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame GPU start"),
        });
        self.marker
            .as_ref()
            .unwrap()
            .write(&mut encoder, &slot.query, 0);
        queue.submit([encoder.finish()]);
        true
    }
    pub fn end(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let Some((index, frame)) = self.active.take() else {
            return;
        };
        let slot = &self.slots[index];
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame GPU end"),
        });
        self.marker
            .as_ref()
            .unwrap()
            .write(&mut encoder, &slot.query, 1);
        queue.submit([encoder.finish()]);
        let done = slot.gpu_done.clone();
        queue.on_submitted_work_done(move || {
            done.store(true, Ordering::Release);
        });
        self.slots[index].awaiting_resolve = Some(frame);
    }
    /// Resolve only after the marker submission has completed. On Metal,
    /// resolving counters in the marker's command buffer returned stale values.
    /// This state machine never waits and retains the slot until mapped/drained.
    pub fn poll(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        for slot in &mut self.slots {
            if slot.awaiting_resolve.is_none() || !slot.gpu_done.load(Ordering::Acquire) {
                continue;
            }
            let frame = slot.awaiting_resolve.take().unwrap();
            slot.gpu_done.store(false, Ordering::Release);
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("completed frame timestamps"),
            });
            encoder.resolve_query_set(&slot.query, 0..2, &slot.resolve, 0);
            encoder.copy_buffer_to_buffer(&slot.resolve, 0, &slot.read, 0, 16);
            queue.submit([encoder.finish()]);
            let buffer = slot.read.clone();
            let busy = slot.busy.clone();
            let ready = self.ready.clone();
            let invalid = self.invalid.clone();
            let omitted = self.omitted.clone();
            let period = self.period;
            slot.read.map_async(wgpu::MapMode::Read, .., move |result| {
                let mut sample = GpuFrameSample {
                    frame,
                    elapsed_ns: 0,
                    status: 2,
                };
                if result.is_ok() {
                    if let Ok(bytes) = buffer.get_mapped_range(..) {
                        let start = u64::from_le_bytes(bytes[..8].try_into().unwrap());
                        let end = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
                        let elapsed = end.saturating_sub(start) as f64 * period;
                        if start > 0
                            && end > start
                            && elapsed.is_finite()
                            && elapsed < u64::MAX as f64
                        {
                            sample.elapsed_ns = elapsed.round() as u64;
                            sample.status = 1;
                        } else {
                            sample.status = 3;
                        }
                    }
                    buffer.unmap();
                }
                if sample.status != 1 {
                    invalid.fetch_add(1, Ordering::Relaxed);
                }
                // A profiler must not make drawing wait for its consumer.
                if let Ok(mut ready) = ready.try_lock() {
                    if ready.len() < 256 {
                        ready.push_back(sample);
                    } else {
                        omitted.fetch_add(1, Ordering::Relaxed);
                    }
                } else {
                    omitted.fetch_add(1, Ordering::Relaxed);
                }
                busy.store(false, Ordering::Release);
            });
        }
    }
    pub fn take_into(&self, output: &mut [GpuFrameSample]) -> usize {
        let Ok(mut ready) = self.ready.try_lock() else {
            return 0;
        };
        let count = output.len().min(ready.len());
        for sample in &mut output[..count] {
            *sample = ready.pop_front().unwrap();
        }
        count
    }
    pub fn stats(&self) -> GpuFrameTimingStats {
        GpuFrameTimingStats {
            support: if self.period > 0. { 1 } else { 2 },
            requested: self.requested,
            skipped: self.skipped + self.omitted.load(Ordering::Relaxed),
            invalid: self.invalid.load(Ordering::Relaxed),
            pending: self
                .slots
                .iter()
                .filter(|s| s.busy.load(Ordering::Acquire))
                .count() as u64,
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn hardware_spans_keep_identity_bound_pending_and_reuse_completed_slots() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
            .expect("Hardware adapter required");
        assert!(
            adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY),
            "Timestamp-capable GPU required"
        );
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        }))
        .unwrap();
        let mut timer = GpuFrameTimer::new(&device, &queue);
        let mut expected = Vec::new();
        let mut samples = [GpuFrameSample::default(); 16];
        let mut observed = Vec::new();
        for round in 0..3 {
            let before = expected.len();
            for frame in (round * 8 + 1)..=(round * 8 + 8) {
                if timer.begin(&device, &queue, frame) {
                    // A rejected overlapping begin must preserve the original ID.
                    assert!(!timer.begin(&device, &queue, 10_000 + frame));
                    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                        label: None,
                        size: 4096,
                        usage: wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    let mut encoder = device.create_command_encoder(&Default::default());
                    encoder.clear_buffer(&buffer, 0, None);
                    queue.submit([encoder.finish()]);
                    timer.end(&device, &queue);
                    expected.push(frame);
                }
                assert!(timer.stats().pending <= 3);
            }
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(std::time::Duration::from_secs(5)),
                })
                .unwrap();
            assert_eq!(
                expected.len() - before,
                3,
                "A fourth pending observation must be skipped"
            );
            timer.poll(&device, &queue);
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(std::time::Duration::from_secs(5)),
                })
                .unwrap();
            let count = timer.take_into(&mut samples);
            for sample in &samples[..count] {
                assert_eq!(sample.status, 1, "Invalid hardware timestamps: {sample:?}");
                assert!(
                    sample.elapsed_ns > 0,
                    "Empty marker passes must not yield false zero timings"
                );
                observed.push(sample.frame);
            }
            assert_eq!(timer.stats().pending, 0);
        }
        expected.sort_unstable();
        observed.sort_unstable();
        assert_eq!(observed, expected);
        assert!(
            observed.len() >= 3,
            "Completed slots must be reusable across rounds"
        );
        assert_eq!(timer.stats().invalid, 0);
        assert_eq!(
            timer.stats().requested,
            observed.len() as u64 + timer.stats().skipped
        );
        assert_eq!(
            timer.take_into(&mut samples),
            0,
            "Each result is delivered once"
        );
    }
}
