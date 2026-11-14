//! Visible diagnostics encode bounded, nonblocking timestamps with the drawing.
use crate::frame_timing::{GpuFrameSample, GpuFrameTimer};
use layer_render::{RendererTelemetry, TimingSamples};
use std::sync::Mutex;

const PHASE_NAMES: [&std::ffi::CStr; 19] = [
    c"Capy GPU paint ns", c"Capy GPU prediction ns", c"Capy GPU composition ns",
    c"Capy GPU heal candidates ns", c"Capy GPU heal seed ns", c"Capy GPU heal apply ns",
    c"Capy GPU heal pyramid ns", c"Capy GPU heal relaxation ns",
    c"Capy GPU display sources ns", c"Capy GPU main composition ns", c"Capy GPU overview composition ns",
    c"Capy GPU main mips ns", c"Capy GPU overview mips ns", c"Capy GPU Navigator ns",
    c"Capy GPU native capture ns", c"Capy GPU native encoding ns",
    c"Capy GPU Liquify map ns", c"Capy GPU Liquify originals ns", c"Capy GPU Liquify reconstruct ns",
];

#[derive(Default)]
struct GpuSamples {
    timer: Option<GpuFrameTimer>,
    frame: u64,
    samples: TimingSamples,
    phases: Vec<GpuFrameTimer>,
}
impl GpuSamples {
    fn timers(&mut self) -> impl Iterator<Item = &mut GpuFrameTimer> {
        self.timer.iter_mut().chain(&mut self.phases)
    }
    fn collect(&mut self) {
        let mut ready = [GpuFrameSample::default(); 256];
        if let Some(timer) = &self.timer {
            let count = timer.take_into(&mut ready);
            for sample in &ready[..count] {
                if sample.status == 1 {
                    crate::performance_trace::counter(c"Capy GPU observation", sample.frame);
                    crate::performance_trace::counter(c"Capy GPU elapsed ns", sample.elapsed_ns);
                    self.samples.push(sample.elapsed_ns as f32 / 1_000_000.);
                }
            }
        }
        for (index, timer) in self.phases.iter().enumerate() {
            let count = timer.take_into(&mut ready);
            for sample in &ready[..count] {
                if sample.status == 1 {
                    crate::performance_trace::counter(c"Capy GPU phase observation", sample.frame);
                    crate::performance_trace::counter(PHASE_NAMES[index], sample.elapsed_ns);
                }
            }
        }
    }
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
        device: &crate::PipelineDevice,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: u64,
    ) {
        if !(self.enabled || crate::performance_trace::enabled()) || !self.supported {
            return;
        }
        let gpu = self.gpu.get_mut().unwrap();
        for timer in gpu.timers() {
            timer.resolve_completed(encoder);
        }
        gpu.collect();
        gpu.frame = frame;
        let timer = gpu
            .timer
            .get_or_insert_with(|| GpuFrameTimer::with_device(device, queue));
        timer.begin_encoded(encoder, frame);
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
        for timer in &mut self.gpu.get_mut().unwrap().phases {
            timer.submitted(queue);
        }
    }
    // Diagnostic-only GPU intervals. They include inter-submission scheduling
    // gaps, not hardware occupancy. Slots drop observations instead of waiting.
    pub fn phase_begin(
        &mut self,
        index: usize,
        device: &crate::PipelineDevice,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
    ) -> bool {
        if !self.supported || !crate::performance_trace::enabled() {
            return false;
        }
        let gpu = self.gpu.get_mut().unwrap();
        if gpu.phases.is_empty() {
            gpu.phases = PHASE_NAMES.map(|_| GpuFrameTimer::with_device(device, queue)).into();
        }
        let frame = gpu.frame;
        gpu.phases[index].begin_encoded(encoder, frame)
    }
    pub fn phase_end(&mut self, index: usize, encoder: &mut wgpu::CommandEncoder) {
        if let Some(timer) = self.gpu.get_mut().unwrap().phases.get_mut(index) {
            timer.end_encoded(encoder);
        }
    }
    pub fn snapshot(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> RendererTelemetry {
        let samples = self
            .gpu
            .try_lock()
            .map(|mut gpu| {
                for timer in gpu.timers() {
                    timer.poll(device, queue);
                }
                gpu.collect();
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

    #[cfg(test)]
    pub fn completed_snapshot(
        &self,
        device: &crate::PipelineDevice,
        queue: &wgpu::Queue,
    ) -> RendererTelemetry {
        for _ in 0..2 {
            crate::test_support::complete_device(device);
            self.snapshot(device, queue);
        }
        self.snapshot(device, queue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_timers_resolve_inside_the_next_drawing_submission() {
        let instance = crate::WgpuRasterizer::headless_instance();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
            .expect("Hardware adapter required");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        })).unwrap();
        let device=crate::PipelineDevice::from(device);device.bind_queue(&queue);
        #[cfg(not(target_arch="wasm32"))]
        device.initialize_poll_owner().unwrap();
        let wait=||crate::test_support::complete_device(&device);
        let mut telemetry = Telemetry::new(&device, &queue);
        telemetry.enabled = true;
        let gpu=telemetry.gpu.get_mut().unwrap();
        gpu.timer=Some(GpuFrameTimer::with_device(&device,&queue));
        gpu.phases=(0..2).map(|_|GpuFrameTimer::with_device(&device,&queue)).collect();
        for timer in gpu.timer.iter().chain(&gpu.phases) {pollster::block_on(timer.prepare()).unwrap();}
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {label: None, size: 4096, usage: wgpu::BufferUsages::COPY_DST, mapped_at_creation: false});
        let mut drawing = device.create_command_encoder(&Default::default());
        telemetry.begin(&device, &queue, &mut drawing, 1);
        for timer in &mut telemetry.gpu.get_mut().unwrap().phases {
            assert!(timer.begin_encoded(&mut drawing, 1));
            drawing.clear_buffer(&buffer, 0, None);
            timer.end_encoded(&mut drawing);
        }
        telemetry.end(&mut drawing);
        queue.submit([drawing.finish()]);
        telemetry.submitted(&queue);
        wait();
        let deadline=std::time::Instant::now()+crate::READBACK_TIMEOUT;
        let completed=|gpu:&GpuSamples|gpu.timer.iter().chain(&gpu.phases).map(GpuFrameTimer::completed).sum::<usize>();
        while completed(telemetry.gpu.get_mut().unwrap())<3 {assert!(std::time::Instant::now()<deadline,"the first drawing completes");wait();}
        let ready=|gpu:&GpuSamples|gpu.timer.iter().chain(&gpu.phases).map(|timer|timer.take_into(&mut [GpuFrameSample::default();4])).sum::<usize>();
        let mut drawing = device.create_command_encoder(&Default::default());
        telemetry.begin(&device, &queue, &mut drawing, 2);
        wait();wait();
        assert_eq!(ready(telemetry.gpu.get_mut().unwrap()),0,"completed timers resolve in the next drawing, not in submissions of their own");
        telemetry.end(&mut drawing);
        queue.submit([drawing.finish()]);
        telemetry.submitted(&queue);
        let mut delivered=0;
        while delivered<3 {assert!(std::time::Instant::now()<deadline,"the drawing submission resolves the frame timer and both phases");wait();delivered+=ready(telemetry.gpu.get_mut().unwrap());}
        assert_eq!(delivered,3);
    }

    #[test]
    fn sustained_diagnostics_retire_frame_and_phase_samples_without_panel_queries() {
        let instance = crate::WgpuRasterizer::headless_instance();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
            .expect("Hardware adapter required");
        assert_ne!(adapter.get_info().device_type, wgpu::DeviceType::Cpu);
        assert!(adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY));
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        })).unwrap();
        let device=crate::PipelineDevice::from(device);device.bind_queue(&queue);
        #[cfg(not(target_arch="wasm32"))]
        device.initialize_poll_owner().unwrap();
        let mut telemetry = Telemetry::new(&device, &queue);
        telemetry.enabled = true;
        let gpu=telemetry.gpu.get_mut().unwrap();
        gpu.timer=Some(GpuFrameTimer::with_device(&device,&queue));
        gpu.phases=(0..2).map(|_|GpuFrameTimer::with_device(&device,&queue)).collect();
        for timer in gpu.timer.iter().chain(&gpu.phases) {pollster::block_on(timer.prepare()).unwrap();}
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 4096,
            usage: wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        for frame in 1..=300 {
            let mut encoder = device.create_command_encoder(&Default::default());
            telemetry.begin(&device, &queue, &mut encoder, frame);
            for timer in &mut telemetry.gpu.get_mut().unwrap().phases {
                assert!(timer.begin_encoded(&mut encoder, frame));
                encoder.clear_buffer(&buffer, 0, None);
                timer.end_encoded(&mut encoder);
            }
            telemetry.end(&mut encoder);
            queue.submit([encoder.finish()]);
            telemetry.submitted(&queue);
            crate::test_support::complete_device(&device);
            let gpu = telemetry.gpu.get_mut().unwrap();
            gpu.timer.as_mut().unwrap().poll(&device, &queue);
            for timer in &mut gpu.phases { timer.poll(&device, &queue); }
            crate::test_support::complete_device(&device);
        }
        let samples = telemetry.completed_snapshot(&device, &queue);
        assert_eq!(samples.gpu.count, 300);
        assert_eq!(samples.gpu.ordered().len(), 120);
        let gpu = telemetry.gpu.get_mut().unwrap();
        for timer in std::iter::once(gpu.timer.as_ref().unwrap()).chain(gpu.phases.iter()) {
            let stats = timer.stats();
            assert_eq!(stats.requested, 300);
            assert_eq!(stats.skipped, 0);
            assert_eq!(stats.invalid, 0);
            assert_eq!(stats.pending, 0);
            assert_eq!(timer.take_into(&mut [GpuFrameSample::default(); 1]), 0);
        }
    }
}
