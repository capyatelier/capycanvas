use std::{collections::VecDeque, sync::{Arc, atomic::{AtomicBool, Ordering}}};

#[derive(Default)]
pub(crate) struct FramePacing {
    in_flight: VecDeque<Arc<AtomicBool>>,
}

impl FramePacing {
    pub(crate) const IN_FLIGHT: usize = 1;

    pub(crate) fn pending(&self) -> bool {
        self.in_flight.iter().any(|done|!done.load(Ordering::Acquire))
    }

    pub(crate) fn ready(&mut self) -> bool {
        while self.in_flight.front().is_some_and(|done| done.load(Ordering::Acquire)) { self.in_flight.pop_front(); }
        !self.saturated()
    }

    pub(crate) fn occupy(&mut self, wake: Arc<dyn Fn() + Send + Sync>) -> impl FnOnce() + Send + 'static + use<> {
        let done = Arc::new(AtomicBool::new(false));
        self.in_flight.push_back(done.clone());
        move || { done.store(true, Ordering::Release); wake(); }
    }

    fn saturated(&self) -> bool {
        self.in_flight.len() >= Self::IN_FLIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_reads_completion_flags_without_requiring_another_frame() {
        let done=Arc::new(AtomicBool::new(false));
        let pacing=FramePacing {in_flight:[done.clone()].into()};
        assert!(pacing.pending());
        done.store(true,Ordering::Release);
        assert!(!pacing.pending(),"completed work is idle even before acquire removes its old receipt");
        assert_eq!(pacing.in_flight.len(),1,"the diagnostic read must not mutate pacing");
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn completed_drawing_opens_its_slot_while_later_queue_work_is_held() {
        use windows::{core::Interface, Win32::Graphics::Direct3D12::{ID3D12Device, ID3D12Fence, D3D12_FENCE_FLAG_NONE}};
        struct Gate(ID3D12Fence);
        impl Drop for Gate { fn drop(&mut self) { let _ = unsafe { self.0.Signal(1) }; } }
        let mut gpu = crate::test_support::d3d12_renderer();
        let (wake, woken) = std::sync::mpsc::channel();
        let wake = std::sync::Mutex::new(wake);
        let mut pacing = FramePacing::default();
        let drawing = gpu.device().create_command_encoder(&Default::default());
        gpu.on_encoder_done(&drawing, pacing.occupy(Arc::new(move || { let _ = wake.lock().unwrap().send(()); })));
        gpu.submit_commands([drawing.finish()]).unwrap();
        let gate = {
            let native = unsafe { gpu.device().as_hal::<wgpu::hal::api::Dx12>() }.unwrap();
            let device: ID3D12Device = native.raw_device().cast().unwrap();
            let gate = Gate(unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.unwrap());
            unsafe { native.raw_queue().Wait(&gate.0, 1) }.unwrap();
            gate
        };
        gpu.queue().submit([]);
        let services = Arc::new(AtomicBool::new(false));
        let serviced = services.clone();
        gpu.on_gpu_done(move || serviced.store(true, Ordering::Release));
        woken.recv_timeout(std::time::Duration::from_secs(10)).expect("the drawing's completion wakes the host");
        assert!(pacing.ready(), "the next frame starts once its drawing completes");
        assert!(pacing.in_flight.is_empty());
        assert!(!services.load(Ordering::Acquire), "the queue's later work is still held");
        drop(gate);
        gpu.wait_idle().unwrap();
        drop(gpu);
        layer_render_wgpu::finish_shader_compiler_shutdown();
    }

    #[test]
    fn a_saturated_frame_returns_at_once_and_its_completion_wakes_the_next() {
        #[cfg(target_os = "windows")]
        let gpu = crate::test_support::d3d12_renderer();
        #[cfg(not(target_os = "windows"))]
        let gpu = layer_render_wgpu::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let (wake, woken) = std::sync::mpsc::channel();
        let wake = std::sync::Mutex::new(wake);
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || { let _ = wake.lock().unwrap().send(()); });
        let device = gpu.device();
        let size = 32 << 20;
        let buffer = |usage| device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage, mapped_at_creation: false });
        let (source, target) = (buffer(wgpu::BufferUsages::COPY_SRC), buffer(wgpu::BufferUsages::COPY_DST));
        let mut pacing = FramePacing::default();
        for _ in 0..FramePacing::IN_FLIGHT {
            assert!(pacing.ready());
            let mut encoder = device.create_command_encoder(&Default::default());
            for _ in 0..2 { encoder.copy_buffer_to_buffer(&source, 0, &target, 0, size); }
            gpu.on_encoder_done(&encoder, pacing.occupy(wake.clone()));
            gpu.submit_commands([encoder.finish()]).unwrap();
        }
        let start = std::time::Instant::now();
        let ready = pacing.ready();
        assert!(start.elapsed() < std::time::Duration::from_millis(20), "pacing must not wait on the GPU");
        if !ready { woken.recv_timeout(std::time::Duration::from_secs(10)).expect("the frame's completion wakes the host"); }
        assert!(pacing.ready(), "completed frames release their slots");
        assert!(pacing.in_flight.is_empty());
        drop(gpu);
        layer_render_wgpu::finish_shader_compiler_shutdown();
    }
}
