use super::Device;
use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};
use core::{any::Any, sync::atomic::{AtomicBool, Ordering}, time::Duration};
use crate::{instance::{Adapter, Surface}, lock::{rank, Mutex}};

struct SurfaceAdapter(Box<dyn hal::DynAdapter>);
impl hal::DynResource for SurfaceAdapter {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}
impl hal::DynAdapter for SurfaceAdapter {
    unsafe fn open(&self, features: wgt::Features, limits: &wgt::Limits, hints: &wgt::MemoryHints) -> Result<hal::DynOpenDevice, hal::DeviceError> {
        unsafe { self.0.open(features, limits, hints) }
    }
    unsafe fn texture_format_capabilities(&self, format: wgt::TextureFormat) -> hal::TextureFormatCapabilities {
        unsafe { self.0.texture_format_capabilities(format) }
    }
    unsafe fn surface_capabilities(&self, _: &dyn hal::DynSurface) -> Option<hal::SurfaceCapabilities> {
        Some(hal::SurfaceCapabilities {
            formats: vec![wgt::SurfaceFormatCapabilities { format: wgt::TextureFormat::Rgba8Unorm, color_spaces: wgt::SurfaceColorSpaces::SRGB }],
            maximum_frame_latency: 1..=3, current_extent: None,
            usage: wgt::TextureUses::COLOR_TARGET,
            present_modes: vec![wgt::PresentMode::Fifo],
            composite_alpha_modes: vec![wgt::CompositeAlphaMode::Opaque],
        })
    }
    unsafe fn surface_display_hdr_info(&self, _: &dyn hal::DynSurface) -> Option<wgt::DisplayHdrInfo> { None }
    unsafe fn get_presentation_timestamp(&self) -> wgt::PresentationTimestamp {
        unsafe { self.0.get_presentation_timestamp() }
    }
    fn get_ordered_buffer_usages(&self) -> wgt::BufferUses { self.0.get_ordered_buffer_usages() }
    fn get_ordered_texture_usages(&self) -> wgt::TextureUses { self.0.get_ordered_texture_usages() }
}

struct ProbeSurface(Box<dyn Fn() -> Result<(), hal::SurfaceError> + Send + Sync>);
impl hal::DynResource for ProbeSurface {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}
impl hal::DynSurface for ProbeSurface {
    unsafe fn configure(&self, _: &dyn hal::DynDevice, _: &hal::SurfaceConfiguration) -> Result<(), hal::SurfaceError> { (self.0)() }
    unsafe fn unconfigure(&self, _: &dyn hal::DynDevice) {}
    unsafe fn acquire_texture(&self, _: Option<Duration>, _: &dyn hal::DynFence) -> Result<hal::DynAcquiredSurfaceTexture, hal::SurfaceError> { Err(hal::SurfaceError::Timeout) }
    unsafe fn discard_texture(&self, _: Box<dyn hal::DynSurfaceTexture>) {}
}

fn device() -> (Arc<Device>, Arc<crate::device::queue::Queue>) {
    let mut options = wgt::BackendOptions::default();
    options.noop.enable = true;
    let instance = unsafe { <hal::noop::Context as hal::Instance>::init(&hal::InstanceDescriptor {
        name: "surface admission test", flags: wgt::InstanceFlags::empty(),
        memory_budget_thresholds: Default::default(), backend_options: options,
        telemetry: None, display: None,
    }) }.unwrap();
    let exposed = unsafe { <hal::noop::Context as hal::Instance>::enumerate_adapters(&instance, None) }.remove(0);
    let mut exposed: hal::DynExposedAdapter = exposed.into();
    exposed.adapter = Box::new(SurfaceAdapter(exposed.adapter));
    Arc::new(Adapter::new(exposed)).create_device_and_queue(&Default::default(), wgt::InstanceFlags::empty()).unwrap()
}

fn configuration() -> wgt::SurfaceConfiguration<Vec<wgt::TextureFormat>> {
    wgt::SurfaceConfiguration {
        usage: wgt::TextureUsages::RENDER_ATTACHMENT, format: wgt::TextureFormat::Rgba8Unorm,
        width: 16, height: 16, present_mode: wgt::PresentMode::Fifo,
        desired_maximum_frame_latency: 2, alpha_mode: wgt::CompositeAlphaMode::Opaque,
        view_formats: vec![], color_space: wgt::SurfaceColorSpace::Srgb,
    }
}

fn surface(probe: impl Fn() -> Result<(), hal::SurfaceError> + Send + Sync + 'static) -> Arc<Surface> {
    let raw: Box<dyn hal::DynSurface> = Box::new(ProbeSurface(Box::new(probe)));
    Arc::new(Surface {
        presentation: Mutex::new(rank::SURFACE_PRESENTATION, None),
        surface_per_backend: [(wgt::Backend::Noop, raw)].into_iter().collect(),
    })
}

#[test]
fn configure_excludes_background_submission_and_fires_reentrant_callbacks_on_both_outcomes() {
    for fail in [false, true] {
        let (device, queue) = device();
        queue.submit(&[]).unwrap();
        let called = Arc::new(AtomicBool::new(false));
        let probe_device = device.clone();
        let probe_called = called.clone();
        let surface = surface(move || {
                assert!(!probe_called.load(Ordering::Acquire));
                let concurrent = probe_device.clone();
                let admitted = std::thread::spawn(move || concurrent.command_indices.try_write().is_some()).join().unwrap();
                assert!(!admitted, "a background submission can enter during HAL surface configuration");
                if fail { Err(hal::SurfaceError::Lost) } else { Ok(()) }
        });
        let callback_queue = queue.clone();
        let callback_surface = surface.clone();
        let callback_called = called.clone();
        queue.on_submitted_work_done(Box::new(move || {
            drop(callback_surface.presentation.lock());
            callback_queue.submit(&[]).unwrap();
            callback_called.store(true, Ordering::Release);
        }));
        let result = device.configure_surface(&surface, &configuration());
        assert_eq!(result.is_some(), fail);
        assert!(called.load(Ordering::Acquire), "configure dropped a pending work-done callback");
        assert!(device.command_indices.try_write().is_some());
    }
}

#[test]
fn rejected_live_output_and_invalid_configuration_preserve_presentation_and_callbacks() {
    let (device, queue) = device();
    let surface = surface(|| panic!("invalid configure reached HAL"));
    let (texture, _) = device.create_texture(&wgt::TextureDescriptor {
        label: None, size: wgt::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgt::TextureDimension::D2,
        format: wgt::TextureFormat::Rgba8Unorm, usage: wgt::TextureUsages::COPY_DST,
        view_formats: vec![],
    });
    *surface.presentation.lock() = Some(crate::present::Presentation {
        device: device.clone(), config: configuration(), acquired_texture: Some(texture.clone()),
    });
    for invalid_size in [false, true] {
        let called = Arc::new(AtomicBool::new(false));
        let callback_called = called.clone();
        queue.on_submitted_work_done(Box::new(move || callback_called.store(true, Ordering::Release)));
        let mut config = configuration();
        if invalid_size { config.width = 0; }
        let result = device.configure_surface(&surface, &config);
        assert!(result.is_some());
        assert!(called.load(Ordering::Acquire));
        let presentation = surface.presentation.lock();
        let retained = presentation.as_ref().unwrap();
        assert!(Arc::ptr_eq(retained.acquired_texture.as_ref().unwrap(), &texture));
        assert_eq!(retained.config.width, 16);
    }
}

#[test]
fn configure_device_error_releases_guards_before_synchronous_device_lost_callback() {
    let (device, queue) = device();
    let surface = surface(|| Err(hal::SurfaceError::Device(hal::DeviceError::Lost)));
    let called = Arc::new(AtomicBool::new(false));
    let callback_called = called.clone();
    let callback_device = device.clone();
    let callback_surface = surface.clone();
    *device.device_lost_closure.lock() = Some(Box::new(move |_, _| {
        assert!(callback_device.command_indices.try_write().is_some());
        drop(callback_surface.presentation.lock());
        callback_called.store(true, Ordering::Release);
    }));
    let submitted = Arc::new(AtomicBool::new(false));
    let callback_submitted = submitted.clone();
    queue.on_submitted_work_done(Box::new(move || callback_submitted.store(true, Ordering::Release)));
    assert!(device.configure_surface(&surface, &configuration()).is_some());
    assert!(called.load(Ordering::Acquire));
    assert!(submitted.load(Ordering::Acquire));
    assert!(surface.presentation.lock().is_none());
}

#[test]
fn configure_preserves_ranked_presentation_snatch_submission_order() {
    let (device, _queue) = device();
    let probe_device = device.clone();
    let surface = surface(move || {
        let concurrent = probe_device.clone();
        assert!(!std::thread::spawn(move || concurrent.command_indices.try_write().is_some()).join().unwrap());
        Ok(())
    });
    assert!(device.configure_surface(&surface, &configuration()).is_none());
    assert!(surface.presentation.lock().is_some());
}
