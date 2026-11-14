//! Native Metal presentation. Accessed only on the session's serial owner.
use layer_host::{
    NativeHost,
    scene::{Glass, Navigators, SceneDisplay},
};
use layer_render::CanvasRenderer;
use layer_render_wgpu::SdrSurfaceColor;
use layer_render_wgpu::{
    GpuFrameSample, GpuFrameTimer, GpuFrameTimingStats, ViewportPresenter, WgpuRasterizer,
};
use layer_ui::CanvasCursor;
use std::{ffi::c_void, time::Instant};
#[cfg(any(target_abi = "sim", test))]
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

struct Surface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    presenter: ViewportPresenter,
    working_color: layer_core::color::DocumentColor,
    #[cfg(target_abi = "sim")]
    launch: LaunchReceipt,
}

#[cfg(any(target_abi = "sim", test))]
#[derive(Default)]
struct LaunchReceipt(Option<(u64, Arc<AtomicBool>)>);

#[cfg(any(target_abi = "sim", test))]
impl LaunchReceipt {
    fn observe(&mut self, frame: Option<u64>) {
        if self.0.as_ref().is_some_and(|(pending, _)| Some(*pending) != frame) { self.0 = None; }
    }
    fn submit(&mut self, frame: u64) -> Option<Arc<AtomicBool>> {
        self.observe((frame != 0).then_some(frame));
        if frame == 0 || self.0.is_some() { return None; }
        let completed = Arc::new(AtomicBool::new(false));
        self.0 = Some((frame, completed.clone()));
        Some(completed)
    }
    fn take_completed(&mut self, frame: Option<u64>) -> Option<u64> {
        self.observe(frame);
        if !self.0.as_ref()?.1.load(Ordering::Acquire) { return None; }
        self.0.take().map(|(frame, _)| frame)
    }
}

#[derive(Default)]
pub struct MetalHost {
    surface: Option<Surface>,
    pub(crate) local_tone: layer_host::tone::ToneService,
    headroom: f32,
    instance: Option<wgpu::Instance>,
    cursor: CanvasCursor,
    blank_presented: bool,
    timing_enabled: bool,
    timing: Option<GpuFrameTimer>,
    pub(crate) navigators: Navigators,
    pub(crate) glass: Glass,
    presented_panel_revision: Option<u64>,
    watch: layer_host::DeviceWatch,
    screen_presented: Option<Instant>,
}

const SCREEN_CHECK_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl MetalHost {
    pub(crate) fn document_changed(&mut self) {
        self.local_tone.clear();
        self.cursor=Default::default(); self.timing=None;
    }
    pub(crate) fn set_headroom(&mut self,host:&mut NativeHost,headroom:f32)->Result<(),String>{
        if !headroom.is_finite() || !(1. ..=100.).contains(&headroom){return Err("Invalid display headroom".into());}
        if self.headroom!=headroom || host.session.state().hdr_display_available != (headroom>1.) {
            self.headroom=headroom;host.session.set_hdr_display_available(headroom>1.);
            host.invalidate_snapshot();host.dirty=true;
        }
        Ok(())
    }
    pub(crate) fn screen_tick(&mut self, host: &mut NativeHost) -> bool {
        let clipped = {
            let (Some(surface), Some(gpu)) = (self.surface.as_mut(), host.session.engine().backend().0.as_deref()) else {
                return false;
            };
            match surface.presenter.screen_check_result() {
                Some(result) => result.ok(),
                None => {
                    if !host.dirty && self.screen_presented.is_some_and(|at| at.elapsed() >= SCREEN_CHECK_DELAY) {
                        surface.presenter.check_screen(gpu);
                    }
                    return false;
                }
            }
        };
        let changed = host.session.set_screen_clipped(clipped);
        host.dirty |= changed;
        changed
    }
    pub(crate) fn poll_color(&mut self,host:&mut NativeHost)->Result<bool,String>{
        let changed=self.local_tone.tick(host)?;
        if changed {host.dirty=true; host.invalidate_snapshot();}
        Ok(changed)
    }
    #[cfg(target_abi = "sim")]
    pub(crate) fn observe_launch(&mut self, host: &NativeHost) {
        if let Some(surface) = &mut self.surface { surface.launch.observe(host.launch_frame()); }
    }
    #[cfg(target_abi = "sim")]
    pub(crate) fn poll_launch(&mut self, host: &mut NativeHost) -> bool {
        let frame = host.launch_frame();
        self.surface.as_mut().and_then(|surface| surface.launch.take_completed(frame))
            .is_some_and(|frame| host.launch_presented(frame))
    }
    pub(crate) fn poll_presentation(&mut self, host: &mut NativeHost) -> Result<bool, String> {
        let Some(surface) = self.surface.as_mut() else { return Ok(false); };
        Self::poll_presenter(host, &mut surface.presenter)
    }
    pub(super) fn poll_presenter(host: &mut NativeHost, presenter: &mut ViewportPresenter) -> Result<bool, String> {
        let before = (host.dirty, host.startup);
        host.poll_canvas_presentation(presenter)?;
        Ok((!before.0 && host.dirty) || before.1 != host.startup)
    }
    pub(crate) fn display_status(&self, host: &NativeHost) -> serde_json::Value {
        let s = &host.session;
        let hdr = s.engine().document().composition().color.depth.is_float();
        let hdr_output = hdr && self.surface.is_some() && self.headroom > 1. && s.hdr_presentation_allowed();
        let retained = self.local_tone.current(host).is_some();
        serde_json::json!({"hdr":hdr,"hdr_output":hdr_output,"retained":retained,"error":self.local_tone.error,
            "glass_regions":self.glass.count(),
            "panel_revision":self.presented_panel_revision,
            "backdrop_frames":self.surface.as_ref().map_or([0; 2], |s| s.presenter.backdrop_frames())})
    }
    fn encoding(color:layer_core::color::DocumentColor)->SdrSurfaceColor {
        if color.depth.is_float(){SdrSurfaceColor::ExtendedLinearSrgb}else{SdrSurfaceColor::DisplayP3}
    }

    /// Each device records failures separately; a retired callback cannot stop
    /// its replacement. All session changes still happen on the serial owner.
    pub(crate) fn install_renderer(&mut self, host: &mut NativeHost, renderer: Box<WgpuRasterizer>) -> Result<(), String> {
        let watch = layer_host::DeviceWatch::for_device(renderer.device());
        let retired = host.replace_renderer(layer_host::Renderer(Some(renderer)))?;
        self.local_tone.clear();
        self.watch = watch;
        self.timing = None;
        self.presented_panel_revision = None;
        self.cursor = Default::default();
        host.startup = Default::default();
        if retired.0.is_some() {
            std::thread::Builder::new().name("capy-retired-gpu".into())
                .spawn(move || drop(retired)).map_err(error)?;
        }
        Ok(())
    }

    pub(crate) fn observe_failure(&mut self, host: &mut NativeHost) {
        if host.session.engine().backend().0.is_some()
            && let Some(message) = self.watch.failure()
        {
            self.stop(host, message);
        }
    }

    pub(crate) fn stop(&mut self, host: &mut NativeHost, message: String) {
        let message = match host.session.engine().backend().0.as_ref() {
            Some(gpu) => format!("{message}\nGPU: {:?}\nCanvas: {:?}", gpu.adapter().get_info(), gpu.document_color()),
            None => message,
        };
        eprintln!("CapyCanvas GPU stopped: {message}");
        // Retire capture/encoder resources on a worker. Their completion can
        // wait, but already captured immutable rasters remain saveable.
        let suspension = host.suspend_renderer();
        self.local_tone.clear();
        self.surface = None;
        host.reset_canvas_frame_rate();
        self.timing = None;
        let retired = host.session.renderer_mut().0.take();
        self.instance = None;
        self.blank_presented = false;
        self.presented_panel_revision = None;
        self.cursor = Default::default();
        host.document_adopted();
        host.dirty = false;
        let retirement = if retired.is_some() {
            std::thread::Builder::new().name("capy-retired-gpu".into())
                .spawn(move || drop(retired)).map(|_| ())
        } else { Ok(()) };
        host.error = Some(match (suspension, retirement) {
            (Err(error), _) => format!("{message}\nRecovery could not finish: {error}"),
            (_, Err(error)) => format!("{message}\nGPU retirement failed: {error}"),
            _ => message,
        });
    }

    /// The platform retains its layer until detach has completed on this owner.
    #[cfg(target_vendor = "apple")]
    pub unsafe fn attach(
        &mut self,
        host: &mut NativeHost,
        layer: *mut c_void,
    ) -> Result<(), String> {
        self.detach();
        // Keep the device when replacing a layer: document textures remain live.
        let instance = self.instance.get_or_insert_with(|| {
            let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
            descriptor.backends = wgpu::Backends::METAL;
            layer_host::gpu::native_instance(descriptor)
        });
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer))
        }
        .map_err(|e| format!("Metal surface creation: {e}"))?;
        if host.session.engine().backend().0.is_none() {
            let gpu = pollster::block_on(layer_host::gpu::process_gpu(
                instance, &surface, host.session.localization(), wgpu::PowerPreference::None, &|_| None,
            ))?;
            let watch = gpu.watch;
            let context = layer_host::GpuContext::from_factory(gpu.factory);
            let (color, options) = (host.session.engine().document().composition().color, host.renderer_options());
            let renderer = std::thread::scope(|scope| -> Result<Box<WgpuRasterizer>, String> {
                std::thread::Builder::new()
                    .name("capy-renderer".into())
                    .stack_size(8 * 1024 * 1024)
                    .spawn_scoped(scope, || context.rasterizer(color, &options, false))
                    .map_err(|e| e.to_string())?
                    .join()
                    .map_err(|payload| layer_host::panic_diagnostic(&*payload))?
            }).map_err(|e| format!("Metal renderer creation ({:?}): {e}", context.adapter.get_info()))?;
            if let Some(failure) = watch.failure() {
                return Err(format!("Metal renderer creation ({:?}): {failure}", context.adapter.get_info()));
            }
            self.install_renderer(host, renderer)?;
        }
        let [width, height] = host.session.state().camera.viewport;
        let gpu = host.session.renderer_mut().0.as_ref().unwrap();
        let mut config = surface
            .get_default_config(gpu.adapter(), width, height)
            .ok_or("The Metal device cannot present to this layer")?;
        let caps = surface.get_capabilities(gpu.adapter());
        config.present_mode = wgpu::PresentMode::Fifo;
        config.desired_maximum_frame_latency = 2;
        if let Some(format) = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
        {
            config.format = format;
        }
        // Metal advertises P3 SDR for its native formats on both Apple hosts.
        // The compositor handles destination-profile changes without touching artwork.
        let encoding=Self::encoding(gpu.document_color());
        if gpu.document_color().depth.is_float(){config.format=wgpu::TextureFormat::Rgba16Float;}
        config.color_space = encoding.surface_color_space();
        surface.configure(gpu.device(), &config);
        let mut presenter = ViewportPresenter::for_surface(gpu, config.format, encoding)
            .map_err(|e| format!("Metal viewport creation: {e}"))?;
        presenter.prepare_chrome(gpu);
        self.surface = Some(Surface {
            surface,
            config,
            presenter,
            working_color: gpu.document_color(),
            #[cfg(target_abi = "sim")]
            launch: LaunchReceipt::default(),
        });
        host.error = None;
        host.dirty = true;
        Ok(())
    }

    #[cfg(not(target_vendor = "apple"))]
    pub unsafe fn attach(
        &mut self,
        _: &mut NativeHost,
        _: *mut c_void,
    ) -> Result<(), String> {
        Err("Metal presentation requires an Apple target".into())
    }

    pub fn detach(&mut self) {
        self.surface = None;
        self.presented_panel_revision = None;
    }

    pub fn set_timing_enabled(&mut self, enabled: bool) {
        self.timing_enabled = enabled;
    }

    pub fn take_timing(
        &mut self,
        host: &mut NativeHost,
        samples: &mut [GpuFrameSample],
    ) -> Result<(usize, GpuFrameTimingStats), String> {
        let Some(timing) = &mut self.timing else {
            return Ok((0, GpuFrameTimingStats::default()));
        };
        if let Some(gpu) = &host.session.renderer_mut().0 {
            timing.poll(gpu.device(), gpu.queue());
        }
        Ok((timing.take_into(samples), timing.stats()))
    }

    /// Times are CPU stage durations, not GPU completion or presentation latency.
    pub fn frame(
        &mut self,
        host: &mut NativeHost,
        now: u64,
        presentation: u64,
        launch_frame: &mut u64,
    ) -> Result<(bool, [u64; 5]), String> {
        *launch_frame = 0;
        host.poll_preparation();
        self.observe_failure(host);
        self.poll_color(host)?;
        #[cfg(target_abi = "sim")]
        self.poll_launch(host);
        if host.frame_demand() == layer_render::FrameDemand::Wait && host.startup.complete { self.poll_presentation(host)?; }
        if host.session.rendering_suspended() { return Ok((false, [0; 5])); }
        if (host.frame_demand() == layer_render::FrameDemand::Wait && host.startup.complete && host.launch_frame().is_none()) || self.surface.is_none() {
            return Ok((false, [0; 5]));
        }
        // Own the timer locally so a panic cannot strand an active query in the
        // host. Normal errors/early returns still close their GPU span.
        let mut timing = self.timing.take();
        if self.timing_enabled
            && let Some(gpu) = &host.session.renderer_mut().0 {
                timing
                    .get_or_insert_with(|| GpuFrameTimer::new(gpu))
                    .begin(gpu.device(), gpu.queue(), now);
            }
        let result = self.frame_inner(host, now, presentation, launch_frame);
        if let (Some(timing), Some(gpu)) = (&mut timing, &host.session.renderer_mut().0) {
            timing.end(gpu.device(), gpu.queue());
        }
        self.timing = timing;
        result
    }

    fn frame_inner(
        &mut self,
        host: &mut NativeHost,
        now: u64,
        presentation: u64,
        launch_frame: &mut u64,
    ) -> Result<(bool, [u64; 5]), String> {
        let mut costs = [0; 5];
        let clock = Instant::now();
        host.prepare_canvas_frame(now, presentation, self.blank_presented)?;
        let view = host.session.state().camera.view();
        let surround = host.session.state().palette.surround_linear;
        let scale = view.width_px as f32 / host.logical[0];
        let tone_guide = self.local_tone.current(host);
        let surface = self.surface.as_mut().unwrap();
        #[cfg(target_abi = "sim")]
        let launch_wake = host.wake_callback();
        let gpu = host
            .session
            .renderer_mut()
            .0
            .as_ref()
            .ok_or("Missing Metal renderer")?;
        if surface.working_color != gpu.document_color() {
            let encoding=Self::encoding(gpu.document_color());
            surface.config.format=if gpu.document_color().depth.is_float(){wgpu::TextureFormat::Rgba16Float}else{wgpu::TextureFormat::Bgra8UnormSrgb};
            surface.config.color_space=encoding.surface_color_space();
            surface.surface.configure(gpu.device(),&surface.config);
            surface.presenter = ViewportPresenter::for_surface(gpu, surface.config.format, encoding).map_err(error)?;
            surface.presenter.prepare_chrome(gpu);
            self.presented_panel_revision = None;
            surface.working_color = gpu.document_color();
        }
        surface.presenter.set_panel_surfaces(gpu, self.glass.panels(scale));
        let display = SceneDisplay { scale, headroom: self.headroom, blank_presented: self.blank_presented, compositor_hdr: false };
        if !host.compose_scene(&mut surface.presenter, &mut self.cursor, &self.navigators,
            self.glass.regions(scale), tone_guide, display)?.ready { return Ok((true, costs)); }
        let ready_frame = host.launch_frame().unwrap_or(0);
        let hdr_output = surface.working_color.depth.is_float() && self.headroom > 1. && host.session.hdr_presentation_allowed();
        let screen = &host.session.state().screen;
        let screen_check = layer_render_wgpu::ScreenCheck::for_view(&screen.assessment,
            Self::encoding(surface.working_color).primaries(), hdr_output, screen.show_clipped);
        let gpu = host
            .session
            .renderer_mut()
            .0
            .as_ref()
            .ok_or("Missing Metal renderer")?;
        surface.presenter.set_screen_check(gpu, screen_check);
        if [view.width_px, view.height_px] != [surface.config.width, surface.config.height] {
            surface.config.width = view.width_px;
            surface.config.height = view.height_px;
            surface.surface.configure(gpu.device(), &surface.config);
        }
        costs[0] = clock.elapsed().as_nanos() as u64;
        let target = match surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                surface.surface.configure(gpu.device(), &surface.config);
                host.dirty = true;
                return Ok((true, costs));
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                host.dirty = true;
                return Ok((true, costs));
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                host.dirty = true;
                return Ok((false, costs));
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("Metal surface validation failed".into());
            }
        };
        costs[1] = clock.elapsed().as_nanos() as u64 - costs[0];
        surface.presenter.present(
            gpu,
            &target.texture.create_view(&Default::default()),
            view,
            surround,
        ).map_err(error)?;
        costs[2] = clock.elapsed().as_nanos() as u64 - costs[..2].iter().sum::<u64>();
        gpu.queue().present(target);
        #[cfg(target_abi = "sim")]
        if let Some(completed) = surface.launch.submit(ready_frame) {
            gpu.on_gpu_done(move || {
                completed.store(true, Ordering::Release);
                launch_wake();
            });
        }
        *launch_frame = ready_frame;
        host.canvas_frame_rate.submitted();
        self.blank_presented = true;
        self.screen_presented = Some(Instant::now());
        costs[3] = clock.elapsed().as_nanos() as u64 - costs[..3].iter().sum::<u64>();
        costs[4] = clock.elapsed().as_nanos() as u64 - costs[..4].iter().sum::<u64>();
        host.session.renderer_mut().0.as_mut().unwrap().after_present();
        let revision = Some(self.glass.panel_revision());
        if self.presented_panel_revision != revision {
            self.presented_panel_revision = revision;
            host.invalidate_snapshot();
        }
        Ok((host.frame_demand() != layer_render::FrameDemand::Wait, costs))
    }
}

#[cfg(test)]
mod launch_receipt_tests {
    use super::*;

    #[test]
    fn launch_receipt_keeps_its_submission_until_gpu_completion() {
        let mut receipt = LaunchReceipt::default();
        assert!(receipt.submit(0).is_none());
        let first = receipt.submit(7).unwrap();
        assert!(receipt.submit(7).is_none());
        assert_eq!(receipt.take_completed(Some(7)), None);
        first.store(true, Ordering::Release);
        assert_eq!(receipt.take_completed(Some(7)), Some(7));
        assert_eq!(receipt.take_completed(Some(7)), None);
        let next = receipt.submit(8).unwrap();
        assert_eq!(receipt.take_completed(Some(8)), None);
        next.store(true, Ordering::Release);
        assert_eq!(receipt.take_completed(Some(8)), Some(8));
    }

    #[test]
    fn retired_surface_completion_cannot_complete_its_replacement_receipt() {
        let mut old = LaunchReceipt::default();
        let retired = old.submit(7).unwrap();
        drop(old);
        let mut replacement = LaunchReceipt::default();
        let current = replacement.submit(7).unwrap();
        retired.store(true, Ordering::Release);
        assert_eq!(replacement.take_completed(Some(7)), None);
        current.store(true, Ordering::Release);
        assert_eq!(replacement.take_completed(Some(7)), Some(7));
    }

    #[test]
    fn readiness_flap_cancels_completion_before_the_same_digest_returns() {
        let mut receipt = LaunchReceipt::default();
        let old = receipt.submit(7).unwrap();
        old.store(true, Ordering::Release);
        assert_eq!(receipt.take_completed(None), None);
        let fresh = receipt.submit(7).unwrap();
        assert!(!Arc::ptr_eq(&old, &fresh));
        old.store(true, Ordering::Release);
        assert_eq!(receipt.take_completed(Some(7)), None);
        fresh.store(true, Ordering::Release);
        assert_eq!(receipt.take_completed(Some(7)), Some(7));
    }

    #[test]
    fn decreasing_digest_cancels_the_old_receipt_without_ordering_tickets() {
        let mut receipt = LaunchReceipt::default();
        let old = receipt.submit(99).unwrap();
        let fresh = receipt.submit(3).unwrap();
        old.store(true, Ordering::Release);
        assert_eq!(receipt.take_completed(Some(3)), None);
        fresh.store(true, Ordering::Release);
        assert_eq!(receipt.take_completed(Some(3)), Some(3));
    }
}
