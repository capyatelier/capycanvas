//! Shared hardware fixtures. Run removal tests serially in their own process.
use crate::device::D3d12Watch;
use layer_host::{DeviceWatch, Renderer};
use layer_render_wgpu::WgpuRasterizer;
use std::time::{Duration, Instant};
use windows::{Win32::Graphics::Direct3D12::ID3D12Device5, core::Interface};

pub(crate) fn renderer() -> (Renderer, DeviceWatch) {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::DX12;
    descriptor.flags.remove(wgpu::InstanceFlags::DEBUG);
    let instance = wgpu::Instance::new(descriptor);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .unwrap();
        assert_eq!(adapter.get_info().backend, wgpu::Backend::Dx12);
        if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
            assert!(
                Instant::now() < deadline,
                "Removed hardware device is still retained"
            );
            drop(adapter);
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .unwrap();
        let state = DeviceWatch::observe(&device);
        #[allow(deprecated)]
        let gpu = WgpuRasterizer::from_wgpu(adapter, device, queue).unwrap();
        assert!(!state.is_lost(Some(gpu.device())));
        state.check().unwrap();
        return (Renderer(Some(gpu.into())), state);
    }
}

pub(crate) fn remove_device(renderer: &Renderer, state: &DeviceWatch) {
    let gpu = renderer.0.as_ref().unwrap();
    {
        let native = unsafe { gpu.device().as_hal::<wgpu::hal::api::Dx12>() }.unwrap();
        let device: ID3D12Device5 = native.raw_device().cast().unwrap();
        unsafe { device.RemoveDevice() };
    }
    let _ = gpu.device().poll(wgpu::PollType::Poll);
    assert!(state.is_lost(Some(gpu.device())));
}
