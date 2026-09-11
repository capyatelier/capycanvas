//! Native Metal presentation. Accessed only on the session's serial owner.
use layer_host::NativeHost;
use layer_render_wgpu::{ViewportPresenter, WgpuRasterizer};
use layer_ui::CanvasCursor;
use std::{ffi::c_void, time::Instant};

struct Surface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    presenter: ViewportPresenter,
}

#[derive(Default)]
pub struct MetalHost {
    surface: Option<Surface>,
    instance: Option<wgpu::Instance>,
    cursor: CanvasCursor,
    blank_presented: bool,
}

fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl MetalHost {
    /// The platform retains its layer until detach has completed on this owner.
    #[cfg(target_vendor = "apple")]
    pub unsafe fn attach(
        &mut self,
        host: &mut NativeHost,
        layer: *mut c_void,
        cache: &std::path::Path,
    ) -> Result<(), String> {
        self.detach();
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
            host.startup = Default::default();
            self.blank_presented = false;
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
                    required_features: adapter.features()
                        & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::PIPELINE_CACHE),
                    required_limits: limits,
                    ..Default::default()
                }))
                .map_err(error)?;
            host.session.renderer_mut().0 = Some(
                WgpuRasterizer::from_wgpu_staged_cached(adapter, device, queue, cache)
                    .map_err(error)?,
            );
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
        surface.configure(gpu.device(), &config);
        let presenter = ViewportPresenter::for_renderer(gpu, config.format);
        self.surface = Some(Surface {
            surface,
            config,
            presenter,
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

    /// Times are CPU stage durations, not GPU completion or presentation latency.
    pub fn frame(
        &mut self,
        host: &mut NativeHost,
        now: u64,
        presentation: u64,
    ) -> Result<(bool, [u64; 5]), String> {
        let mut costs = [0; 5];
        if (!host.dirty && host.startup.complete) || self.surface.is_none() {
            return Ok((false, costs));
        }
        let clock = Instant::now();
        host.prepare_canvas_frame(now, presentation, self.blank_presented)?;
        let view = host.session.state().camera.view();
        let surround = host.session.state().palette.surround_linear;
        let scale = view.width_px as f32 / host.logical[0];
        host.session.update_canvas_cursor(&mut self.cursor, false);
        host.session.append_layer_overlay(&mut self.cursor.segments);
        let surface = self.surface.as_mut().unwrap();
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
        surface
            .presenter
            .set_cursor(gpu.device(), &self.cursor.segments, scale);
        surface.presenter.present(
            gpu,
            &target.texture.create_view(&Default::default()),
            view,
            surround,
        );
        costs[2] = clock.elapsed().as_nanos() as u64 - costs[..2].iter().sum::<u64>();
        gpu.queue().present(target);
        self.blank_presented = true;
        costs[3] = clock.elapsed().as_nanos() as u64 - costs[..3].iter().sum::<u64>();
        gpu.device().poll(wgpu::PollType::Poll).map_err(error)?;
        costs[4] = clock.elapsed().as_nanos() as u64 - costs[..4].iter().sum::<u64>();
        Ok((host.dirty, costs))
    }
}
