//! Visible diagnostics encode bounded, nonblocking timestamps with the drawing.
use crate::frame_timing::{GpuFrameSample, GpuFrameTimer};
use layer_render::{RendererTelemetry, TimingSamples};
use std::sync::Mutex;

#[derive(Default)]
struct GpuSamples {
    timer: Option<GpuFrameTimer>,
    samples: TimingSamples,
}

pub(super) struct Telemetry {
    pub enabled: bool,
    pub cpu: TimingSamples,
    gpu: Mutex<GpuSamples>,
    supported: bool,
}
impl Telemetry {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            enabled: false,
            cpu: TimingSamples::default(),
            gpu: Mutex::default(),
            supported: device.features().contains(wgpu::Features::TIMESTAMP_QUERY)
                && queue.get_timestamp_period() > 0.,
        }
    }
    // Mark the drawing itself without separate start/end queue submissions.
    pub fn begin(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        if !self.enabled || !self.supported {
            return;
        }
        let gpu = self.gpu.get_mut().unwrap();
        let timer = gpu
            .timer
            .get_or_insert_with(|| GpuFrameTimer::new(device, queue));
        timer.poll(device, queue);
        timer.begin_encoded(encoder, 0);
    }
    pub fn end(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if let Some(timer) = &mut self.gpu.get_mut().unwrap().timer {
            timer.end_encoded(encoder);
        }
    }
    pub fn submitted(&mut self, queue: &wgpu::Queue) {
        if let Some(timer) = &mut self.gpu.get_mut().unwrap().timer {
            timer.submitted(queue);
        }
    }
    pub fn snapshot(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> RendererTelemetry {
        let samples = self
            .gpu
            .try_lock()
            .map(|mut gpu| {
                // Panel queries also retire the final observation after drawing
                // sleeps. The host/event loop supplies ordinary device polling.
                if let Some(timer) = &mut gpu.timer {
                    timer.poll(device, queue);
                    let mut ready = [GpuFrameSample::default(); 256];
                    let count = timer.take_into(&mut ready);
                    for sample in &ready[..count] {
                        if sample.status == 1 {
                            gpu.samples.push(sample.elapsed_ns as f32 / 1_000_000.);
                        }
                    }
                }
                gpu.samples.clone()
            })
            .unwrap_or_default();
        RendererTelemetry {
            cpu: self.cpu.clone(),
            gpu: samples,
            gpu_timestamps: self.supported,
            ..Default::default()
        }
    }

    /// Benchmark-only synchronization; ordinary diagnostics never wait.
    #[cfg(test)]
    pub fn completed_snapshot(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> RendererTelemetry {
        for _ in 0..2 {
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(crate::READBACK_TIMEOUT),
                })
                .unwrap();
            self.snapshot(device, queue);
        }
        self.snapshot(device, queue)
    }
}
