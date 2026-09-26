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

struct Surface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    presenter: ViewportPresenter,
    working_color: layer_core::color::DocumentColor,
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
    watch: layer_host::DeviceWatch,
    pub(crate) cache: Option<std::path::PathBuf>,
}

fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl MetalHost {
    pub(crate) fn document_changed(&mut self) {
        self.local_tone.clear();
        self.cursor=Default::default(); self.blank_presented=false; self.timing=None;
    }
    pub(crate) fn set_headroom(&mut self,host:&mut NativeHost,headroom:f32)->Result<(),String>{
        if !headroom.is_finite() || !(1. ..=100.).contains(&headroom){return Err("Invalid display headroom".into());}
        if self.headroom!=headroom || host.session.state().hdr_display_available != (headroom>1.) {
            self.headroom=headroom;host.session.set_hdr_display_available(headroom>1.);
            host.invalidate_snapshot();host.dirty=true;
        }
        Ok(())
    }
    pub(crate) fn poll_color(&mut self,host:&mut NativeHost)->Result<bool,String>{
        let changed=self.local_tone.tick(host)?;
        if changed {host.dirty=true; host.invalidate_snapshot();}
        Ok(changed)
    }
    pub(crate) fn display_status(&self, host: &NativeHost) -> serde_json::Value {
        let s = &host.session;
        let hdr = s.engine().document().color.depth.is_float();
        let hdr_output = hdr && self.surface.is_some() && self.headroom > 1. && s.hdr_presentation_allowed();
        let retained = self.local_tone.current(host).is_some();
        let label = if hdr_output { "HDR" } else if self.local_tone.error.is_some() { "SDR preview unavailable" }
            else if !retained { "Preparing SDR…" } else if s.state().soft_proof { "Print proof" }
            else if self.headroom > 1. { "SDR preview" } else { "Showing SDR" };
        serde_json::json!({"hdr":hdr,"hdr_output":hdr_output,"label":label,"headroom":self.headroom.max(1.),
            "retained":retained,"error":self.local_tone.error,"reference_white":203,
            "glass_regions":self.glass.count(),
            "backdrop_frames":self.surface.as_ref().map_or([0; 2], |s| s.presenter.backdrop_frames())})
    }
    fn encoding(color:layer_core::color::DocumentColor)->SdrSurfaceColor {
        if color.depth.is_float(){SdrSurfaceColor::ExtendedLinearSrgb}else{SdrSurfaceColor::DisplayP3}
    }

    /// Each device records failures separately; a retired callback cannot stop
    /// its replacement. All session changes still happen on the serial owner.
    pub(crate) fn install_renderer(&mut self, host: &mut NativeHost, renderer: Box<WgpuRasterizer>) -> Result<(), String> {
        let watch = layer_host::DeviceWatch::observe(renderer.device());
        let previous = host.session.state().revision;
        let (retired, change) = host.session.replace_renderer(layer_host::Renderer(Some(renderer)))?;
        host.apply_change(previous, change);
        self.local_tone.clear();
        self.watch = watch;
        self.timing = None;
        self.blank_presented = false;
        self.cursor = Default::default();
        host.startup = Default::default();
        if retired.0.is_some() {
            std::thread::Builder::new().name("capy-retired-gpu".into())
                .spawn(move || drop(retired)).map_err(error)?;
        }
        Ok(())
    }

    pub(crate) fn observe_failure(&mut self, host: &mut NativeHost, poll: bool) {
        if poll && let Some(gpu) = &host.session.engine().backend().0
            && let Err(error) = gpu.device().poll(wgpu::PollType::Poll)
        {
            self.watch.report_error(error.to_string());
        }
        if host.session.engine().backend().0.is_some()
            && let Some(message) = self.watch.failure()
        {
            self.stop(host, message);
        }
    }

    pub(crate) fn stop(&mut self, host: &mut NativeHost, message: String) {
        eprintln!("CapyCanvas GPU stopped: {message}");
        // Retire capture/encoder resources on a worker. Their completion can
        // wait, but already captured immutable rasters remain saveable.
        let suspension = host.suspend_renderer();
        self.local_tone.clear();
        self.surface = None;
        self.timing = None;
        let retired = host.session.renderer_mut().0.take();
        self.instance = None;
        self.blank_presented = false;
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
        cache: &std::path::Path,
    ) -> Result<(), String> {
        self.detach();
        self.cache = Some(cache.to_path_buf());
        // Keep the device when replacing a layer: document textures remain live.
        let instance = self.instance.get_or_insert_with(|| {
            let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
            descriptor.backends = wgpu::Backends::METAL;
            wgpu::Instance::new(descriptor)
        });
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer))
        }
        .map_err(error)?;
        if host.session.engine().backend().0.is_none() {
            let adapter =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                    power_preference: wgpu::PowerPreference::None,
                    apply_limit_buckets: false,
                }))
                .map_err(error)?;
            let limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
            let (device, queue) =
                pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                    label: Some("Capy Canvas Apple"),
                    required_features: (adapter.features()
                        & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::PIPELINE_CACHE
                            | wgpu::Features::FLOAT32_FILTERABLE | wgpu::Features::FLOAT32_BLENDABLE))
                        | layer_render_wgpu::native_tiles::native_in_place_features(&adapter),
                    required_limits: limits,
                    ..Default::default()
                }))
                .map_err(error)?;
            let renderer = layer_host::GpuContext { adapter, device, queue }.rasterizer(
                host.session.engine().document().color, &host.renderer_options(self.cache.clone()), false)?;
            self.install_renderer(host, renderer.into())?;
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
        let presenter = ViewportPresenter::for_surface(gpu, config.format, encoding).map_err(error)?;
        self.surface = Some(Surface {
            surface,
            config,
            presenter,
            working_color: gpu.document_color(),
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
        _: &std::path::Path,
    ) -> Result<(), String> {
        Err("Metal presentation requires an Apple target".into())
    }

    pub fn detach(&mut self) {
        self.surface = None;
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
            gpu.device().poll(wgpu::PollType::Poll).map_err(error)?;
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
    ) -> Result<(bool, [u64; 5]), String> {
        self.observe_failure(host, true);
        self.poll_color(host)?;
        if host.session.rendering_suspended() { return Ok((false, [0; 5])); }
        if (!host.dirty && host.startup.complete) || self.surface.is_none() {
            return Ok((false, [0; 5]));
        }
        // Own the timer locally so a panic cannot strand an active query in the
        // host. Normal errors/early returns still close their GPU span.
        let mut timing = self.timing.take();
        if self.timing_enabled {
            if let Some(gpu) = &host.session.renderer_mut().0 {
                timing
                    .get_or_insert_with(|| GpuFrameTimer::new(gpu.device(), gpu.queue()))
                    .begin(gpu.device(), gpu.queue(), now);
            }
        }
        let result = self.frame_inner(host, now, presentation);
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
    ) -> Result<(bool, [u64; 5]), String> {
        let mut costs = [0; 5];
        let clock = Instant::now();
        host.prepare_canvas_frame(now, presentation, self.blank_presented)?;
        let view = host.session.state().camera.view();
        let surround = host.session.state().palette.surround_linear;
        let scale = view.width_px as f32 / host.logical[0];
        let tone_guide = self.local_tone.current(host);
        let surface = self.surface.as_mut().unwrap();
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
            surface.working_color = gpu.document_color();
        }
        let display = SceneDisplay { scale, headroom: self.headroom, blank_presented: self.blank_presented, compositor_hdr: false };
        host.compose_scene(&mut surface.presenter, &mut self.cursor, &self.navigators,
            self.glass.regions(scale), tone_guide, display)?;
        let gpu = host
            .session
            .renderer_mut()
            .0
            .as_ref()
            .ok_or("Missing Metal renderer")?;
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
        self.blank_presented = true;
        costs[3] = clock.elapsed().as_nanos() as u64 - costs[..3].iter().sum::<u64>();
        gpu.device().poll(wgpu::PollType::Poll).map_err(error)?;
        costs[4] = clock.elapsed().as_nanos() as u64 - costs[..4].iter().sum::<u64>();
        Ok((host.dirty, costs))
    }
}
