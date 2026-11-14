//! One GPU context, renderer factory and device-failure watch for native hosts.
use layer_core::color::{DocumentColor, RgbSpace};
use layer_render_wgpu::{RendererFactory, WgpuRasterizer};
use std::sync::{Arc, OnceLock};

/// UI preview colors: mapped through the SDR rendition, or tagged in a display space.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum UiColor {
    #[default]
    Mapped,
    Tagged(RgbSpace),
}
impl UiColor {
    pub fn preview_space(self) -> RgbSpace {
        match self {
            Self::Mapped => RgbSpace::Srgb,
            Self::Tagged(space) => space,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RendererOptions {
    pub ui_color: UiColor,
}

#[derive(Clone)]
pub struct GpuContext {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    factory: RendererFactory,
}
impl GpuContext {
    pub fn new(adapter: wgpu::Adapter, device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let factory = RendererFactory::new(adapter.clone(), device.clone(), queue.clone());
        Self { adapter, device, queue, factory }
    }
    pub fn from_factory(factory: RendererFactory) -> Self {
        Self { adapter: factory.adapter().clone(), device: factory.device().clone(), queue: factory.queue().clone(), factory }
    }
    pub fn of(gpu: &WgpuRasterizer) -> Self {
        Self { adapter: gpu.adapter().clone(), device: gpu.device().clone(), queue: gpu.queue().clone(), factory: gpu.renderer_factory() }
    }
    /// Secondary renderers (tabs, open, color candidates) pass `finish_catalog = true`;
    /// the window renderer passes false and calls `finish_startup_catalog` after its catalog.
    pub fn rasterizer(
        &self,
        color: DocumentColor,
        options: &RendererOptions,
        finish_catalog: bool,
    ) -> Result<Box<WgpuRasterizer>, String> {
        let mut gpu = self.factory.rasterizer(color).map_err(|e| e.to_string())?;
        gpu.configure_ui_previews(options.ui_color.preview_space())
            .map_err(|e| e.to_string())?;
        if finish_catalog {
            gpu.finish_startup_catalog();
        }
        Ok(gpu)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn native_instance(descriptor: wgpu::InstanceDescriptor) -> wgpu::Instance {
    static INSTANCE: OnceLock<wgpu::Instance> = OnceLock::new();
    INSTANCE.get_or_init(|| wgpu::Instance::new(descriptor)).clone()
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub struct ProcessGpu {
    pub factory: RendererFactory,
    pub watch: DeviceWatch,
}

#[cfg(not(target_arch = "wasm32"))]
static PROCESS_GPU: std::sync::Mutex<Option<ProcessGpu>> = std::sync::Mutex::new(None);
#[cfg(not(target_arch = "wasm32"))]
static INITIALIZING: futures_util::lock::Mutex<()> = futures_util::lock::Mutex::new(());

#[cfg(not(target_arch = "wasm32"))]
fn healthy_cached<T: Clone>(cache: &std::sync::Mutex<Option<T>>, healthy: impl FnOnce(&T) -> bool, retire: impl FnOnce(T) -> Result<(), String>) -> Result<Option<T>, String> {
    let mut cached = cache.lock().map_err(|e| e.to_string())?;
    if cached.as_ref().is_some_and(healthy) { return Ok(cached.clone()); }
    let retired = cached.take();
    drop(cached);
    if let Some(retired) = retired { retire(retired)?; }
    Ok(None)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn finish_process_gpu() -> Result<(), String> { finish_cached_gpu(&PROCESS_GPU) }

#[cfg(not(target_arch = "wasm32"))]
fn finish_cached_gpu(cache: &std::sync::Mutex<Option<ProcessGpu>>) -> Result<(), String> {
    let retirement = (|| {
        let gpu = cache.lock().map_err(|error| error.to_string())?.take();
        if let Some(gpu) = gpu { gpu.factory.retire_device_services().map_err(|error| error.to_string())?; }
        Ok(())
    })();
    layer_render_wgpu::finish_shader_compiler_shutdown();
    retirement
}

#[cfg(not(target_arch = "wasm32"))]
fn preferred_candidate<T>(candidates: impl IntoIterator<Item = T>, admitted: impl Fn(&T) -> bool, prefer: impl Fn(&T) -> bool, required: bool) -> Option<T> {
    let mut first = None;
    for candidate in candidates {
        if !admitted(&candidate) { continue; }
        if prefer(&candidate) { return Some(candidate); }
        if first.is_none() { first = Some(candidate); }
    }
    if required { None } else { first }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy)]
pub enum AdapterChoice<'a> {
    Request(wgpu::PowerPreference),
    Prefer(&'a dyn Fn(&wgpu::Adapter) -> bool),
    Require(&'a dyn Fn(&wgpu::Adapter) -> bool),
}

#[cfg(not(target_arch = "wasm32"))]
impl From<wgpu::PowerPreference> for AdapterChoice<'_> {
    fn from(preference: wgpu::PowerPreference) -> Self { Self::Request(preference) }
}

#[cfg(not(target_arch = "wasm32"))]
pub type SurfaceRefusal<'a> = &'a dyn Fn(&wgpu::SurfaceCapabilities) -> Option<String>;

#[cfg(not(target_arch = "wasm32"))]
fn surface_refusal(adapter: &wgpu::Adapter, surface: &wgpu::Surface<'_>, refusal: SurfaceRefusal<'_>) -> Option<String> {
    if !adapter.is_surface_supported(surface) {
        return Some(format!("{} cannot present this window's surface", adapter.get_info().name));
    }
    refusal(&surface.get_capabilities(adapter))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn surface_adapter(instance: &wgpu::Instance, surface: &wgpu::Surface<'_>, choice: AdapterChoice<'_>,
    refusal: SurfaceRefusal<'_>) -> Result<wgpu::Adapter, String> {
    let power_preference = match choice {
        AdapterChoice::Request(preference) => preference,
        AdapterChoice::Prefer(preferred) | AdapterChoice::Require(preferred) => {
            let required = matches!(choice, AdapterChoice::Require(_));
            if let Some(adapter) = preferred_candidate(instance.enumerate_adapters(wgpu::Backends::all()).await,
                |adapter| unsupported_adapter(adapter, &layer_render_wgpu::rendering_limits(adapter.limits())).is_none()
                    && surface_refusal(adapter, surface, refusal).is_none(), preferred, required) { return Ok(adapter); }
            if required { return Err("The canvas GPU is still releasing its removed device".into()); }
            wgpu::PowerPreference::None
        }
    };
    instance.request_adapter(&wgpu::RequestAdapterOptions {
        compatible_surface: Some(surface), power_preference, force_fallback_adapter: false, apply_limit_buckets: false,
    }).await.map_err(|error| error.to_string())
}

#[cfg(not(target_arch = "wasm32"))]
const LAUNCHER_GPU_REQUESTS: [&str; 4] = ["DRI_PRIME", "MESA_VK_DEVICE_SELECT", "__NV_PRIME_RENDER_OFFLOAD", "VK_LOADER_DEVICE_SELECT"];

#[cfg(not(target_arch = "wasm32"))]
pub fn launcher_chose_gpu(var: impl Fn(&str) -> Option<std::ffi::OsString>) -> bool {
    LAUNCHER_GPU_REQUESTS.into_iter().any(|name| var(name).is_some_and(|value| !value.is_empty()))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn prefers_compositor_adapter(info: &wgpu::AdapterInfo, compositor_bus: Option<&str>, launcher_choice: bool) -> bool {
    launcher_choice || compositor_bus.is_some_and(|bus| !bus.is_empty() && info.device_pci_bus_id == bus)
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn process_gpu<'a>(instance: &wgpu::Instance, surface: &wgpu::Surface<'_>,
    localization: &layer_ui::Localizer, choice: impl Into<AdapterChoice<'a>>, refusal: SurfaceRefusal<'_>) -> Result<ProcessGpu, String> {
    let _initializing = INITIALIZING.lock().await;
    let refused = |adapter: &wgpu::Adapter| surface_refusal(adapter, surface, refusal)
        .map_or(Ok(()), |cause| Err(layer_ui::gpu_unsupported(localization, &cause)));
    if let Some(gpu) = healthy_cached(&PROCESS_GPU, |gpu| gpu.watch.failure().is_none(), |gpu| gpu.factory.retire_device_services().map_err(|error| error.to_string()))? {
        return refused(gpu.factory.adapter()).map(|()| gpu);
    }
    let adapter = surface_adapter(instance, surface, choice.into(), refusal).await
        .map_err(|e| layer_ui::gpu_unsupported(localization, &e))?;
    let limits = layer_render_wgpu::rendering_limits(adapter.limits());
    admit_adapter(localization, &adapter, &limits)?;
    refused(&adapter)?;
    let saved = std::thread::spawn({
        let info = adapter.get_info();
        move || layer_render_wgpu::SavedPipelines::read_file(&info)
    });
    let (device, queue, watch) = request_device(&adapter, &wgpu::DeviceDescriptor {
        label: Some("Capy Canvas process GPU"),
        required_features: layer_render_wgpu::rendering_features(&adapter) | adapter.features()
            & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS),
        required_limits: limits, memory_hints: layer_render_wgpu::memory_hints(), ..Default::default()
    }).await.map_err(|e| e.to_string())?;
    let data = saved.join().unwrap_or(None);
    let saved = layer_render_wgpu::SavedPipelines::open(&device, &adapter.get_info(), data);
    let gpu = ProcessGpu { factory: RendererFactory::new_saved(adapter, device, queue, saved), watch };
    *PROCESS_GPU.lock().map_err(|e| e.to_string())? = Some(gpu.clone());
    Ok(gpu)
}

pub fn unsupported_adapter(adapter: &wgpu::Adapter, limits: &wgpu::Limits) -> Option<String> {
    let info = adapter.get_info();
    if info.device_type == wgpu::DeviceType::Cpu && !layer_render_wgpu::software_adapter_tests() {
        return Some(format!("{} is a software renderer, not a hardware GPU", info.name));
    }
    let mut short = Vec::new();
    limits.check_limits_with_fail_fn(&adapter.limits(), false, |name, required, allowed| {
        short.push(format!("{name} is {allowed}, needs {required}"))
    });
    (!short.is_empty()).then(|| format!("{} does not meet the canvas limits: {}", info.name, short.join(", ")))
}

pub fn admit_adapter(localization: &layer_ui::Localizer, adapter: &wgpu::Adapter, limits: &wgpu::Limits) -> Result<(), String> {
    unsupported_adapter(adapter, limits).map_or(Ok(()), |cause| Err(layer_ui::gpu_unsupported(localization, &cause)))
}

pub async fn request_device(adapter: &wgpu::Adapter, descriptor: &wgpu::DeviceDescriptor<'_>)
    -> Result<(wgpu::Device, wgpu::Queue, DeviceWatch), wgpu::RequestDeviceError> {
    let (device, queue) = adapter.request_device(descriptor).await?;
    let watch = DeviceWatch::observe(&device);
    Ok((device, queue, watch))
}

#[derive(Clone, Default)]
pub struct DeviceWatch {
    lost: Arc<OnceLock<String>>,
    error: Arc<OnceLock<String>>,
}
impl DeviceWatch {
    pub fn for_device(device: &wgpu::Device) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(watch) = PROCESS_GPU.lock().unwrap().as_ref()
            .filter(|gpu| gpu.factory.device() == device).map(|gpu| gpu.watch.clone()) { return watch; }
        Self::observe(device)
    }
    pub fn observe(device: &wgpu::Device) -> Self {
        let watch = Self::default();
        let lost = watch.lost.clone();
        device.set_device_lost_callback(move |reason, message| {
            lost.get_or_init(|| format!("Canvas GPU stopped ({reason:?}): {message}"));
        });
        let error = watch.error.clone();
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            error.get_or_init(|| e.to_string());
        }));
        watch
    }
    pub fn lost(&self) -> Option<&str> {
        self.lost.get().map(String::as_str)
    }
    pub fn error(&self) -> Option<&str> {
        self.error.get().map(String::as_str)
    }
    pub fn failure(&self) -> Option<String> {
        self.lost().or(self.error()).map(str::to_owned)
    }
    pub fn report_lost(&self, message: impl Into<String>) {
        self.lost.get_or_init(|| message.into());
    }
    pub fn report_error(&self, message: impl Into<String>) {
        self.error.get_or_init(|| message.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_render::CanvasRenderer;

    #[test]
    fn adapter_order_prefers_only_admitted_candidates() {
        assert_eq!(preferred_candidate([1, 2, 3], |_| true, |candidate| *candidate > 1, false), Some(2));
        assert_eq!(preferred_candidate([1, 2, 3], |candidate| *candidate != 1, |candidate| *candidate == 1, false), Some(2));
        assert_eq!(preferred_candidate([1, 2, 3], |_| false, |_| true, false), None);
        assert_eq!(preferred_candidate([1, 2, 3], |_| true, |candidate| *candidate > 1, true), Some(2));
        assert_eq!(preferred_candidate([1, 2, 3], |candidate| *candidate != 1, |candidate| *candidate == 1, true), None);
        assert_eq!(preferred_candidate([1, 2, 3], |_| true, |_| false, true), None);
    }

    #[test]
    fn adapters_follow_the_launcher_request_then_the_compositor_gpu() {
        let adapter = |name: &'static str, device_type, bus: &str| wgpu::AdapterInfo {
            name: name.into(), device_pci_bus_id: bus.into(), ..wgpu::AdapterInfo::new(device_type, wgpu::Backend::Vulkan)
        };
        let intel = adapter("Intel", wgpu::DeviceType::IntegratedGpu, "0000:00:02.0");
        let nvidia = adapter("NVIDIA", wgpu::DeviceType::DiscreteGpu, "0000:3c:00.0");
        let llvmpipe = adapter("llvmpipe", wgpu::DeviceType::Cpu, "");
        let choose = |listed: &[&wgpu::AdapterInfo], refused: &[&str], compositor: Option<&str>, launcher: bool| {
            preferred_candidate(listed.iter().copied(),
                |info| info.device_type != wgpu::DeviceType::Cpu && !refused.contains(&info.name.as_str()),
                |info| prefers_compositor_adapter(info, compositor, launcher), false).map(|info| info.name.clone())
        };
        let compositor = Some("0000:00:02.0");
        assert_eq!(choose(&[&intel, &nvidia, &llvmpipe], &[], compositor, false).as_deref(), Some("Intel"));
        assert_eq!(choose(&[&nvidia, &intel, &llvmpipe], &[], compositor, false).as_deref(), Some("Intel"));
        assert_eq!(choose(&[&nvidia, &intel, &llvmpipe], &[], compositor, true).as_deref(), Some("NVIDIA"));
        assert_eq!(choose(&[&nvidia], &[], compositor, true).as_deref(), Some("NVIDIA"));
        assert_eq!(choose(&[&nvidia, &intel], &["Intel"], compositor, false).as_deref(), Some("NVIDIA"));
        assert_eq!(choose(&[&nvidia, &intel, &llvmpipe], &[], None, false).as_deref(), Some("NVIDIA"));
        assert_eq!(choose(&[&nvidia, &intel], &[], Some("0000:01:00.0"), false).as_deref(), Some("NVIDIA"));
        assert_eq!(choose(&[&llvmpipe, &intel], &[], Some(""), false).as_deref(), Some("Intel"));
        assert_eq!(choose(&[&llvmpipe], &[], compositor, true), None);
    }

    #[test]
    fn launcher_gpu_requests_are_read_from_the_vulkan_and_prime_variables() {
        let chose = |set: &[(&str, &str)]| launcher_chose_gpu(|name| set.iter()
            .find(|(key, _)| *key == name).map(|(_, value)| std::ffi::OsString::from(value)));
        assert!(!chose(&[]));
        assert!(!chose(&[("DRI_PRIME", ""), ("__VK_LAYER_NV_optimus", "NVIDIA_only"), ("WGPU_POWER_PREF", "high")]));
        for name in ["DRI_PRIME", "MESA_VK_DEVICE_SELECT", "__NV_PRIME_RENDER_OFFLOAD", "VK_LOADER_DEVICE_SELECT"] {
            assert!(chose(&[(name, "1")]), "{name}");
        }
    }

    #[test]
    fn failed_process_cache_releases_its_owner_before_replacement_outside_the_lock() {
        use std::sync::{Mutex, atomic::{AtomicBool, Ordering}};
        struct Owner { released: Arc<AtomicBool>, unlocked: Box<dyn Fn() + Send + Sync> }
        impl Drop for Owner {
            fn drop(&mut self) { (self.unlocked)(); self.released.store(true, Ordering::SeqCst); }
        }
        let cache = Arc::new(Mutex::new(None));
        let released = Arc::new(AtomicBool::new(false));
        let weak_cache = Arc::downgrade(&cache);
        *cache.lock().unwrap() = Some(Arc::new(Owner { released: released.clone(), unlocked: Box::new(move || {
            if let Some(cache) = weak_cache.upgrade() { assert!(cache.try_lock().is_ok()); }
        }) }));
        assert!(healthy_cached(&cache, |_| false, |retired| { drop(retired); Ok(()) }).unwrap().is_none());
        assert!(cache.lock().unwrap().is_none());
        assert!(released.load(Ordering::SeqCst));
    }

    #[test]
    fn healthy_process_cache_reuses_its_owner_and_eviction_preserves_siblings() {
        let owner = Arc::new(());
        let cache = std::sync::Mutex::new(Some(owner.clone()));
        let reused = healthy_cached(&cache, |_| true, |retired| { drop(retired); Ok(()) }).unwrap().unwrap();
        assert!(Arc::ptr_eq(&owner, &reused));
        assert_eq!(Arc::strong_count(&owner), 3);
        assert!(healthy_cached(&cache, |_| false, |retired| { drop(retired); Ok(()) }).unwrap().is_none());
        assert_eq!(Arc::strong_count(&owner), 2);
        assert!(Arc::ptr_eq(&owner, &reused));
    }

    #[test]
    fn failed_process_cache_schedules_retirement_before_admitting_its_replacement() {
        use std::sync::{Mutex, atomic::{AtomicBool, Ordering}};
        let old = Arc::new(());
        let sibling = old.clone();
        let cache = Arc::new(Mutex::new(Some(old)));
        let scheduled = Arc::new(AtomicBool::new(false));
        let next = scheduled.clone();
        assert!(healthy_cached(&cache, |_| false, |retired| {
            assert!(cache.try_lock().is_ok());
            assert!(Arc::ptr_eq(&sibling, &retired));
            next.store(true, Ordering::Release);
            Ok(())
        }).unwrap().is_none());
        assert!(scheduled.load(Ordering::Acquire));
        *cache.lock().unwrap() = Some(Arc::new(()));
        assert!(!Arc::ptr_eq(cache.lock().unwrap().as_ref().unwrap(), &sibling));
    }

    fn copied_gpu_bytes(factory: &RendererFactory) -> Vec<u8> {
        let device = factory.device();
        let bytes = [17, 43, 97, 255, 1, 2, 3, 4, 211, 37, 81, 63, 5, 6, 7, 8];
        let source = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 16,
            usage: wgpu::BufferUsages::COPY_SRC, mapped_at_creation: true });
        source.slice(..).get_mapped_range_mut().unwrap().copy_from_slice(&bytes);
        source.unmap();
        let output = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 16,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&source, 0, &output, 0, 16);
        factory.submit([encoder.finish()]);
        let (send, received) = std::sync::mpsc::channel();
        output.slice(..).map_async(wgpu::MapMode::Read, move |result| { let _ = send.send(result); });
        factory.submit(std::iter::empty());
        received.recv_timeout(std::time::Duration::from_secs(2)).unwrap().unwrap();
        let actual = output.slice(..).get_mapped_range().unwrap().to_vec();
        output.unmap();
        assert_eq!(actual, bytes);
        actual
    }

    #[test]
    fn failed_process_gpu_replacement_finishes_with_an_old_raw_device_held_by_a_straggler() {
        use std::sync::{Mutex, mpsc};
        use std::time::Duration;
        struct HeldDevice { device: Option<wgpu::Device>, dropped: mpsc::Sender<std::thread::ThreadId> }
        impl Drop for HeldDevice {
            fn drop(&mut self) {
                drop(self.device.take());
                let _ = self.dropped.send(std::thread::current().id());
            }
        }
        let old = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let replacement = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        assert_ne!(old.device(), replacement.device());
        assert_ne!(old.adapter().get_info().device_type, wgpu::DeviceType::Cpu);
        assert_ne!(replacement.adapter().get_info().device_type, wgpu::DeviceType::Cpu);
        let old_factory = old.renderer_factory();
        let replacement_factory = replacement.renderer_factory();
        let expected = copied_gpu_bytes(&replacement_factory);
        old_factory.submit(std::iter::empty());
        let watch = DeviceWatch::for_device(old.device());
        watch.report_error("process GPU replacement fixture");
        let (dropped, drop_seen) = mpsc::channel();
        let held = HeldDevice { device: Some(old.device().clone()), dropped };
        let (entered, holding) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let consumer = std::thread::spawn(move || {
            entered.send(std::thread::current().id()).unwrap();
            released.recv_timeout(Duration::from_secs(10)).unwrap();
            drop(held);
        });
        let consumer_thread = holding.recv_timeout(Duration::from_secs(10)).unwrap();
        drop((old, replacement));
        let cache = Mutex::new(Some(ProcessGpu { factory: old_factory, watch }));
        assert!(healthy_cached(&cache, |gpu| gpu.watch.failure().is_none(), |gpu| {
            assert!(cache.try_lock().is_ok());
            gpu.factory.retire_device_services().map_err(|error| error.to_string())
        }).unwrap().is_none());
        *cache.lock().unwrap() = Some(ProcessGpu { factory: replacement_factory.clone(),
            watch: DeviceWatch::for_device(replacement_factory.device()) });
        let (finished, finish_seen) = mpsc::channel();
        let finishing = std::thread::spawn(move || {
            let result = finish_cached_gpu(&Mutex::new(None));
            finished.send((std::thread::current().id(), result)).unwrap();
        });
        let completed_with_consumer = finish_seen.recv_timeout(Duration::from_secs(2));
        let consumer_was_held = matches!(drop_seen.try_recv(), Err(mpsc::TryRecvError::Empty));
        release.send(()).unwrap();
        consumer.join().unwrap();
        finishing.join().unwrap();
        let (finishing_thread, result) = completed_with_consumer.expect("device retirement waited for a raw consumer clone");
        result.unwrap();
        assert_ne!(finishing_thread, std::thread::current().id());
        assert!(consumer_was_held);
        assert_eq!(drop_seen.recv_timeout(Duration::from_secs(2)).unwrap(), consumer_thread);
        assert_eq!(copied_gpu_bytes(&replacement_factory), expected,
            "retiring the old root cannot close the replacement root's service");
        finish_cached_gpu(&cache).unwrap();
        assert!(cache.lock().unwrap().is_none());
    }

    #[test]
    fn native_renderer_factory_and_frames_leave_stack_for_their_caller() {
        let color = DocumentColor::default();
        let reference = WgpuRasterizer::new_native_headless(color).unwrap();
        let context = GpuContext::of(&reference);
        let renderer = std::thread::Builder::new()
            .name("native-renderer-factory".into())
            .stack_size(640 * 1024)
            .spawn(move || {
                let renderer = context.rasterizer(color, &RendererOptions::default(), false).unwrap();
                assert_eq!(renderer.document_color(), color);
                assert!(renderer.device() == &context.device);
                renderer
            })
            .unwrap()
            .join()
            .unwrap();
        let mut host = Box::new(crate::NativeHost::new(layer_ui::Platform::Mac).unwrap());
        host.replace_renderer(crate::Renderer(Some(renderer))).unwrap();
        host.resize(640, 480, 1.).unwrap();
        host.startup = Default::default();
        let mut host = std::thread::Builder::new()
            .name("native-renderer-preparation".into())
            .stack_size(384 * 1024)
            .spawn(move || {
                host.prepare_canvas_frame(0, 0, false).unwrap();
                let engine=host.session.engine();
                assert!(!engine.backend().0.as_ref().unwrap().startup_needs_update(engine.document(),engine.brush(),false));
                host
            })
            .unwrap()
            .join()
            .unwrap();
        host.session.renderer_mut().0.as_mut().unwrap().finish_startup_catalog();
        host.startup = Default::default();
        let clock = std::time::Instant::now();
        while !host.startup.canvas_ready || !host.startup.brush_ready {
            let now = clock.elapsed().as_nanos() as u64;
            host.prepare_canvas_frame(now, now, true).unwrap();
            assert!(clock.elapsed() < std::time::Duration::from_secs(30), "Blank canvas startup timed out");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        std::thread::Builder::new()
            .name("native-renderer-frame".into())
            .stack_size(384 * 1024)
            .spawn(move || {
                let submitted = host.session.engine().backend().0.as_ref().unwrap().submitted_updates();
                host.resize(641, 480, 1.).unwrap();
                let now = clock.elapsed().as_nanos() as u64;
                host.prepare_canvas_frame(now, now, true).unwrap();
                assert!(host.startup.canvas_ready);
                assert!(host.session.engine().backend().0.as_ref().unwrap().submitted_updates() > submitted);
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn renderer_factory_admits_input_and_follows_host_ui_color() {
        let color = DocumentColor::default();
        let headless = WgpuRasterizer::new_native_headless(color).unwrap();
        let mut host = crate::NativeHost::new(layer_ui::Platform::Mac).unwrap();
        assert_eq!(
            host.renderer_options().ui_color.preview_space(),
            RgbSpace::Srgb
        );
        host.ui_color = UiColor::Tagged(RgbSpace::DisplayP3);
        let options = host.renderer_options();
        assert_eq!(options.ui_color.preview_space(), RgbSpace::DisplayP3);
        let gpu = GpuContext::of(&headless)
            .rasterizer(color, &options, true)
            .unwrap();
        assert_eq!(gpu.document_color(), color);
        assert!(gpu.device() == headless.device());
        gpu.shader_input();
        assert!(gpu.shader_wait_ms() > 0.);
    }

    #[test]
    fn adapters_short_of_canvas_limits_are_refused_with_a_plain_reason() {
        let gpu = WgpuRasterizer::new_native_headless(DocumentColor::default()).unwrap();
        let adapter = gpu.adapter();
        let limits = layer_render_wgpu::rendering_limits(adapter.limits());
        assert_eq!(unsupported_adapter(adapter, &limits), None);
        let beyond = wgpu::Limits { max_bind_groups: adapter.limits().max_bind_groups + 1, ..limits.clone() };
        let cause = unsupported_adapter(adapter, &beyond).unwrap();
        assert!(cause.contains("max_bind_groups"), "{cause}");
        let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
        let message = layer_ui::gpu_unsupported(&localization, &cause);
        assert!(message.starts_with("This device’s graphics processor (GPU) can’t run Capy Canvas."), "{message}");
        assert!(message.ends_with(&cause));
        for language in [layer_ui::UiLanguage::English, layer_ui::UiLanguage::Japanese] {
            let localization = layer_ui::Localizer::shared(language);
            assert_eq!(admit_adapter(&localization, adapter, &limits), Ok(()));
            assert_eq!(admit_adapter(&localization, adapter, &beyond), Err(layer_ui::gpu_unsupported(&localization, &cause)));
        }
    }

    #[test]
    fn a_restarted_gpu_is_never_mistaken_for_the_retired_one() {
        let color = DocumentColor::default();
        let retired = WgpuRasterizer::new_native_headless(color).unwrap();
        let restarted = WgpuRasterizer::new_native_headless(color).unwrap();
        assert!(retired.device() != restarted.device());
        assert!(retired.device() == retired.device());
    }
}
