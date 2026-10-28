use std::{collections::VecDeque, time::Duration};

#[derive(Default)]
pub(crate) struct FramePacing {
    in_flight: VecDeque<wgpu::SubmissionIndex>,
}

impl FramePacing {
    pub(crate) const IN_FLIGHT: usize = 1;
    const WAIT: Duration = Duration::from_millis(100);

    pub(crate) fn ready(&mut self, device: &wgpu::Device) -> Result<bool, wgpu::PollError> {
        while self.saturated() {
            let oldest = self.in_flight[0].clone();
            match device.poll(wgpu::PollType::Wait { submission_index: Some(oldest), timeout: Some(Self::WAIT) }) {
                Ok(_) => { self.in_flight.pop_front(); }
                Err(wgpu::PollError::Timeout) => return Ok(false),
                Err(error) => return Err(error),
            }
        }
        Ok(true)
    }

    pub(crate) fn submitted(&mut self, submission: wgpu::SubmissionIndex) {
        self.in_flight.push_back(submission);
    }

    fn saturated(&self) -> bool {
        self.in_flight.len() >= Self::IN_FLIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "windows")]
    fn completed_drawing_does_not_wait_for_unrelated_queue_work() {
        use windows::{core::Interface, Win32::Graphics::Direct3D12::{ID3D12Device, ID3D12Fence, D3D12_FENCE_FLAG_NONE}};
        struct Gate(ID3D12Fence);
        impl Drop for Gate { fn drop(&mut self) { let _ = unsafe { self.0.Signal(1) }; } }
        let gpu = crate::test_support::d3d12_renderer();
        let submission = gpu.queue().submit([]);
        gpu.device().poll(wgpu::PollType::Wait { submission_index: Some(submission.clone()), timeout: Some(Duration::from_secs(10)) }).unwrap();
        let gate = {
            let native = unsafe { gpu.device().as_hal::<wgpu::hal::api::Dx12>() }.unwrap();
            let device: ID3D12Device = native.raw_device().cast().unwrap();
            let gate = Gate(unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.unwrap());
            unsafe { native.raw_queue().Wait(&gate.0, 1) }.unwrap();
            gate
        };
        gpu.queue().submit([]);
        let mut pacing = FramePacing::default();
        pacing.submitted(submission);
        assert!(pacing.ready(gpu.device()).unwrap());
        assert!(pacing.in_flight.is_empty());
        drop(gate);
        gpu.device().poll(wgpu::PollType::wait_indefinitely()).unwrap();
        drop(gpu);
        layer_render_wgpu::finish_shader_compiler_shutdown();
    }

    #[test]
    fn a_second_frame_waits_until_the_gpu_finishes_the_first() {
        #[cfg(target_os = "windows")]
        let gpu = crate::test_support::d3d12_renderer();
        #[cfg(not(target_os = "windows"))]
        let gpu = layer_render_wgpu::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let device = gpu.device();
        let size = 32 << 20;
        let buffer = |usage| device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage, mapped_at_creation: false });
        let (source, target) = (buffer(wgpu::BufferUsages::COPY_SRC), buffer(wgpu::BufferUsages::COPY_DST));
        let mut pacing = FramePacing::default();
        for _ in 0..FramePacing::IN_FLIGHT {
            assert!(pacing.ready(device).unwrap());
            let mut encoder = device.create_command_encoder(&Default::default());
            for _ in 0..2 { encoder.copy_buffer_to_buffer(&source, 0, &target, 0, size); }
            let submission = gpu.queue().submit([encoder.finish()]);
            pacing.submitted(submission);
        }
        assert!(pacing.saturated(), "an unfinished frame holds back the next one until completion is observed");
        let start = std::time::Instant::now();
        let ready = pacing.ready(device).unwrap();
        assert!(ready || start.elapsed() >= FramePacing::WAIT / 2, "a busy GPU delays the next frame instead of skipping it");
        assert_eq!(pacing.saturated(), !ready);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert!(pacing.ready(device).unwrap(), "completed frames release their slots");
        assert!(pacing.in_flight.is_empty());
        drop(gpu);
        layer_render_wgpu::finish_shader_compiler_shutdown();
    }
}
