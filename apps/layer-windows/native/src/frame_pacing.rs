use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Default)]
pub(crate) struct FramePacing {
    submitted: u64,
    completed: Arc<AtomicU64>,
}

impl FramePacing {
    const IN_FLIGHT: u64 = 2;

    pub(crate) fn ready(&self, device: &wgpu::Device) -> Result<bool, wgpu::PollError> {
        if self.saturated() {
            device.poll(wgpu::PollType::Poll)?;
        }
        Ok(!self.saturated())
    }

    pub(crate) fn submitted(&mut self, queue: &wgpu::Queue) {
        self.submitted += 1;
        let frame = self.submitted;
        let completed = self.completed.clone();
        queue.on_submitted_work_done(move || {
            completed.fetch_max(frame, Ordering::Release);
        });
    }

    fn saturated(&self) -> bool {
        self.submitted.saturating_sub(self.completed.load(Ordering::Acquire)) >= Self::IN_FLIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_third_frame_waits_until_the_gpu_finishes_an_earlier_one() {
        let gpu = layer_render_wgpu::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut pacing = FramePacing::default();
        assert!(pacing.ready(gpu.device()).unwrap());
        pacing.submitted(gpu.queue());
        assert!(pacing.ready(gpu.device()).unwrap());
        pacing.submitted(gpu.queue());
        gpu.device().poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert!(pacing.ready(gpu.device()).unwrap(), "completed frames release their slots");
        pacing.submitted(gpu.queue());
        pacing.submitted(gpu.queue());
        assert!(pacing.saturated(), "two unfinished frames hold back the next one until completion is observed");
        gpu.device().poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert!(!pacing.saturated());
        drop(gpu);
        layer_render_wgpu::finish_shader_compiler_shutdown();
    }
}
