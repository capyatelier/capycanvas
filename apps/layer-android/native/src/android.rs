//! JNI and Vulkan window ownership. Every method except creation/destruction is
//! invoked by the same Android render Looper, never by a Compose callback.
use crate::app::App;
use jni::{
    JNIEnv,
    objects::{JClass, JDoubleArray, JObject, JString},
    sys::{jboolean, jfloat, jint, jlong, jstring},
};
use layer_render::CanvasRenderer;
use layer_render_wgpu::{ViewportPresenter, WgpuRasterizer};
use raw_window_handle::{
    AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use std::ptr::NonNull;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

struct Window(NonNull<ndk_sys::ANativeWindow>);
impl Drop for Window {
    fn drop(&mut self) {
        unsafe { ndk_sys::ANativeWindow_release(self.0.as_ptr()) };
    }
}
pub(crate) struct Surface {
    // Drop the swapchain before releasing its native-window reference.
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    presenter: ViewportPresenter,
    first_frame_complete: Option<Arc<AtomicBool>>,
    _instance: wgpu::Instance,
    _window: Window,
}
pub(crate) fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(serde::Deserialize)]
pub(crate) struct OverviewSlot {
    bounds: [f32; 4],
    clip: [f32; 4],
    order: i32,
}

impl App {
    pub(crate) fn project_adopted(&mut self) {
        self.cursor = Default::default();
        // The file worker has already rendered this candidate and waited for
        // its canvas/current-brush shaders. Do not discard the first contact
        // merely because the adopted window has not ticked another frame yet.
        self.host.startup = layer_render_wgpu::StartupProgress {
            canvas_ready: true,
            brush_ready: true,
            complete: false,
        };
        if let Some(surface) = &mut self.surface {
            surface.presenter = ViewportPresenter::for_renderer(
                self.host.session.renderer_mut().0.as_ref().unwrap(),
                surface.config.format,
            );
        }
        self.blank_presented = true;
    }

    fn attach(
        &mut self,
        env: &JNIEnv,
        surface: JObject,
        cache_directory: &str,
    ) -> Result<(), String> {
        self.surface = None;
        self.cache_directory = cache_directory.into();
        let window = NonNull::new(unsafe {
            ndk_sys::ANativeWindow_fromSurface(
                env.get_native_interface().cast(),
                surface.as_raw().cast(),
            )
        })
        .ok_or("Android did not provide a canvas surface")?;
        let window = Window(window);
        // Replacement surfaces must use the original Vulkan instance/device.
        let instance = self
            .instance
            .get_or_insert_with(|| {
                let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
                descriptor.backends = wgpu::Backends::VULKAN;
                wgpu::Instance::new(descriptor)
            })
            .clone();
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(RawDisplayHandle::Android(AndroidDisplayHandle::new())),
                raw_window_handle: RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(
                    window.0.cast(),
                )),
            })
        }
        .map_err(error)?;
        let [width, height] = self.host.session.state().camera.viewport;
        if self.host.session.engine().backend().0.is_none() {
            // Readiness belongs to this GPU initialization, including hosts
            // whose shared state otherwise defaults to eager rendering.
            self.host.startup = Default::default();
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
                    label: Some("Capy Canvas Android"),
                    required_features: adapter.features()
                        & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::PIPELINE_CACHE),
                    required_limits: limits,
                    ..Default::default()
                }))
                .map_err(error)?;
            self.host.session.renderer_mut().0 = Some(
                WgpuRasterizer::from_wgpu_staged_cached(
                    adapter,
                    device,
                    queue,
                    std::path::Path::new(cache_directory),
                )
                .map_err(error)?,
            );
        }
        let gpu = self.host.session.renderer_mut().0.as_ref().unwrap();
        let mut config = surface
            .get_default_config(gpu.adapter(), width, height)
            .ok_or("The Vulkan device cannot present to this surface")?;
        let caps = surface.get_capabilities(gpu.adapter());
        config.present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::Fifo
        };
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
            first_frame_complete: None,
            _instance: instance,
            _window: window,
        });
        self.host.error = None;
        self.host.dirty = true;
        Ok(())
    }
    fn render(&mut self, now: u64, presentation: u64) -> Result<bool, String> {
        self.frame_cost = [0; 5];
        if (!self.host.dirty && self.host.startup.complete) || self.surface.is_none() {
            return Ok(false);
        }
        let clock = self.profiling.then(std::time::Instant::now);
        let elapsed = || clock.map_or(0, |c| c.elapsed().as_nanos() as i64);
        self.host
            .prepare_canvas_frame(now, presentation, self.blank_presented)?;
        let view = self.host.session.state().camera.view();
        let surround = self.host.session.state().palette.surround_linear;
        let scale = self.host.session.state().camera.viewport[0] as f32 / self.host.logical[0];
        self.host
            .session
            .update_canvas_cursor(&mut self.cursor, false);
        self.host
            .session
            .append_layer_overlay(&mut self.cursor.segments);
        let state = self.host.session.state();
        let document = self.host.session.engine().document();
        let fg = state.palette.text.linear();
        let bg = state.palette.panel.linear();
        let overviews: Vec<_> = self
            .overviews
            .iter()
            // Keep the first paper presentation ahead of optional overview
            // pipeline creation, just like other staged startup work.
            .filter(|_| self.blank_presented && self.host.startup.canvas_ready)
            .filter_map(|slot| {
                let [x, y, w, h] = slot.bounds;
                let g = layer_ui::NavigatorGeometry::new(
                    &state.camera,
                    [document.width, document.height],
                    [w / scale, h / scale],
                )?;
                Some(layer_render_wgpu::OverviewPlacement {
                    bounds: [
                        x + g.image.x * scale,
                        y + g.image.y * scale,
                        g.image.width * scale,
                        g.image.height * scale,
                    ],
                    clip: Some(slot.clip),
                    work_area: g.work_area.map(|[a, b]| [x + a * scale, y + b * scale]),
                    outline_linear: [fg[0], fg[1], fg[2]],
                    background_linear: [bg[0], bg[1], bg[2]],
                    scale,
                    opacity: 1.,
                })
            })
            .collect();
        let surface = self.surface.as_mut().unwrap();
        let gpu = self.host.session.renderer_mut().0.as_ref().unwrap();
        let extent = [view.width_px, view.height_px];
        if extent != [surface.config.width, surface.config.height] {
            surface.config.width = extent[0];
            surface.config.height = extent[1];
            surface.surface.configure(gpu.device(), &surface.config);
        }
        self.frame_cost[0] = elapsed();
        let target = match surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                surface.surface.configure(gpu.device(), &surface.config);
                self.host.dirty = true;
                return Ok(true);
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                self.host.dirty = true;
                return Ok(true);
            }
            wgpu::CurrentSurfaceTexture::Occluded => return Ok(false),
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("Vulkan surface validation failed".into());
            }
        };
        self.frame_cost[1] = elapsed() - self.frame_cost[0];
        surface
            .presenter
            .set_cursor(gpu.device(), &self.cursor.segments, scale);
        surface.presenter.set_overviews(gpu, &overviews);
        surface.presenter.present(
            gpu,
            &target.texture.create_view(&Default::default()),
            view,
            surround,
        );
        self.frame_cost[2] = elapsed() - self.frame_cost[0] - self.frame_cost[1];
        gpu.queue().present(target);
        if surface.first_frame_complete.is_none() {
            let complete = Arc::new(AtomicBool::new(false));
            surface.first_frame_complete = Some(complete.clone());
            gpu.queue()
                .on_submitted_work_done(move || complete.store(true, Ordering::Release));
        }
        self.blank_presented = true;
        self.frame_cost[3] = elapsed() - self.frame_cost[..3].iter().sum::<i64>();
        gpu.device().poll(wgpu::PollType::Poll).map_err(error)?;
        self.frame_cost[4] = elapsed() - self.frame_cost[..4].iter().sum::<i64>();
        Ok(self.host.dirty)
    }
}

// The Kotlin host owns this handle and never exposes it to UI callers. Calls
// are serialized on its render Looper, including lifecycle and final disposal.
pub(crate) unsafe fn app<'a>(handle: jlong) -> &'a mut App {
    unsafe { &mut *(handle as *mut App) }
}
pub(crate) fn fail(env: &mut JNIEnv, result: Result<(), String>) {
    if let Err(message) = result {
        let _ = env.throw_new("java/lang/IllegalStateException", message);
    }
}
fn string(env: &mut JNIEnv, result: Result<String, String>) -> jstring {
    match result {
        Ok(value) => match env.new_string(value) {
            Ok(value) => value.into_raw(),
            Err(e) => {
                fail(env, Err(error(e)));
                std::ptr::null_mut()
            }
        },
        Err(e) => {
            fail(env, Err(e));
            std::ptr::null_mut()
        }
    }
}
pub(crate) fn read(env: &mut JNIEnv, value: &JString) -> Result<String, String> {
    env.get_string(value).map(Into::into).map_err(error)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_frameCost(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    array: jni::objects::JLongArray,
) {
    let result = env
        .set_long_array_region(&array, 0, &unsafe { app(handle) }.frame_cost)
        .map_err(error);
    fail(&mut env, result);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_create(
    mut env: JNIEnv,
    _: JClass,
    profiling: jboolean,
) -> jlong {
    match App::new() {
        Ok(mut app) => {
            app.profiling = profiling != 0;
            Box::into_raw(Box::new(app)) as jlong
        }
        Err(e) => {
            fail(&mut env, Err(e));
            0
        }
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_destroy(_: JNIEnv, _: JClass, handle: jlong) {
    if handle != 0 {
        unsafe { drop(Box::from_raw(handle as *mut App)) };
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_attach(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    surface: JObject,
    cache_directory: JString,
) {
    let app = unsafe { app(handle) };
    let result = read(&mut env, &cache_directory)
        .and_then(|directory| app.attach(&env, surface, &directory));
    if let Err(e) = &result {
        app.host.error = Some(e.clone());
    }
    fail(&mut env, result);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_finishStartupCache(
    _: JNIEnv,
    _: JClass,
    handle: jlong,
) {
    if let Some(gpu) = &mut unsafe { app(handle) }.host.session.renderer_mut().0 {
        gpu.finish_startup_cache();
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_detach(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) {
    let app = unsafe { app(handle) };
    fail(
        &mut env,
        app.host.input(layer_ui::UiInput::Blur).map(|_| ()),
    );
    app.surface = None;
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_resize(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    width: jint,
    height: jint,
    density: jfloat,
) {
    fail(
        &mut env,
        unsafe { app(handle) }
            .host
            .resize(width.max(0) as u32, height.max(0) as u32, density),
    );
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_scroll(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    x: jfloat,
    y: jfloat,
    dx: jfloat,
    dy: jfloat,
    zoom: jboolean,
    horizontal: jboolean,
) {
    let app = unsafe { app(handle) };
    let dpi = app.host.session.state().camera.viewport[0] as f32 / app.host.logical[0];
    let previous = app.host.session.state().revision;
    let result = app
        .host
        .session
        .scroll([x, y], [dx, dy], dpi, zoom != 0, horizontal != 0)
        .map(|change| {
            app.host.apply_change(previous, change);
        });
    fail(&mut env, result);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_dispatch(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    action: JString,
) {
    let result = read(&mut env, &action)
        .and_then(|s| serde_json::from_str(&s).map_err(error))
        .and_then(|action| unsafe { app(handle) }.host.dispatch(action));
    fail(&mut env, result);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_input(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    input: JString,
) -> jstring {
    let result = read(&mut env, &input)
        .and_then(|s| serde_json::from_str(&s).map_err(error))
        .and_then(|input| unsafe { app(handle) }.host.input(input))
        .and_then(|reply| serde_json::to_string(&reply).map_err(error));
    string(&mut env, result)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_pointer(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    id: jlong,
    tool: jint,
    button: jint,
    records: JDoubleArray,
    count: jint,
    predicted: jboolean,
) {
    let result = (|| {
        if count <= 0
            || count > 9 * 8192
            || count > env.get_array_length(&records).map_err(error)?
        {
            return Err("Invalid Android pointer batch length".into());
        }
        let app = unsafe { app(handle) };
        let mut data = std::mem::take(&mut app.pointer_records);
        data.resize(count as usize, 0.0);
        let result = env
            .get_double_array_region(&records, 0, &mut data)
            .map_err(error)
            .and_then(|()| {
                app.host.pointer(
                    id.max(0) as u64,
                    tool as u8,
                    button as u8,
                    &data,
                    predicted != 0,
                )
            });
        app.pointer_records = data;
        result
    })();
    fail(&mut env, result);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_frame(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    now: jlong,
    presentation: jlong,
) -> jboolean {
    let app = unsafe { app(handle) };
    match app.render(now.max(0) as u64, presentation.max(now).max(0) as u64) {
        Ok(wake) => wake as jboolean,
        Err(e) => {
            app.host.error = Some(e.clone());
            fail(&mut env, Err(e));
            0
        }
    }
}
/// Poll only during the first frame of each surface. The callback belongs to
/// that surface, so completion from a destroyed surface cannot reveal a new one.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_surfaceReady(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) -> jboolean {
    let app = unsafe { app(handle) };
    let Some(complete) = app
        .surface
        .as_ref()
        .and_then(|s| s.first_frame_complete.as_ref())
    else {
        return 0;
    };
    if let Some(gpu) = app.host.session.renderer_mut().0.as_ref() {
        if let Err(e) = gpu.device().poll(wgpu::PollType::Poll) {
            fail(&mut env, Err(error(e)));
            return 0;
        }
    }
    complete.load(Ordering::Acquire) as jboolean
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_snapshot(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) -> jstring {
    match unsafe { app(handle) }.host.take_update_bytes() {
        Ok(Some(bytes)) => string(&mut env, String::from_utf8(bytes).map_err(error)),
        Ok(None) => std::ptr::null_mut(),
        Err(e) => string(&mut env, Err(error(e))),
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_query(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    query: JString,
) -> jstring {
    let result = read(&mut env, &query)
        .and_then(|s| serde_json::from_str(&s).map_err(error))
        .and_then(|query| unsafe { app(handle) }.host.query(query))
        .map(|v| v.to_string());
    string(&mut env, result)
}

/// Native layout only. Navigator samples the live composition in the canvas
/// presentation pass, including while pen input or camera gestures are active.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_navigatorPlacements(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    value: JString,
) {
    let result = (|| {
        let mut slots: Vec<OverviewSlot> =
            serde_json::from_str(&read(&mut env, &value)?).map_err(error)?;
        if slots.len() > 32
            || slots
                .iter()
                .any(|s| !s.bounds.iter().chain(&s.clip).all(|v| v.is_finite()))
        {
            return Err("Invalid Navigator geometry".into());
        }
        slots.sort_by_key(|s| s.order);
        let a = unsafe { app(handle) };
        a.overviews = slots;
        a.host.dirty = true;
        Ok(())
    })();
    fail(&mut env, result);
}

/// One small metadata record and one packed RGBA array, not JSON per channel.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_takeFilterPreviews(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) -> jni::sys::jobjectArray {
    let result = (|| {
        let renderer = unsafe { app(handle) }.host.session.renderer_mut();
        if let Some(gpu) = &renderer.0 {
            gpu.device().poll(wgpu::PollType::Poll).map_err(error)?;
        }
        let Some(image) = renderer.take_filter_previews() else {
            return Ok(std::ptr::null_mut());
        };
        let atlas = image.map_err(error)?;
        let image = atlas.image;
        let header =
            serde_json::json!([image.request_id, image.width, image.height, atlas.filters]);
        let values = env
            .new_object_array(2, "java/lang/Object", JObject::null())
            .map_err(error)?;
        let header = env.new_string(header.to_string()).map_err(error)?;
        let pixels = env.byte_array_from_slice(&image.bytes).map_err(error)?;
        env.set_object_array_element(&values, 0, header)
            .map_err(error)?;
        env.set_object_array_element(&values, 1, pixels)
            .map_err(error)?;
        Ok(values.into_raw())
    })();
    match result {
        Ok(value) => value,
        Err(e) => {
            fail(&mut env, Err(e));
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_importLayer(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    name: JString,
    width: jint,
    height: jint,
    rgba: jni::objects::JByteArray,
) {
    let result = (|| {
        let name = read(&mut env, &name)?;
        let bytes = env.convert_byte_array(&rgba).map_err(error)?;
        let app = unsafe { app(handle) };
        app.host.import_layer_image(
            &name,
            layer_render::HostImage {
                width: width as u32,
                height: height as u32,
                stride: (width as u32).saturating_mul(4),
                format: layer_render::PixelFormat::Rgba8Srgb,
                bytes: &bytes,
            },
        )?;
        Ok(())
    })();
    fail(&mut env, result);
}

/// Stateless numeric math is independent of the render-owned session. Safe to
/// call on the UI thread; no renderer lock, I/O or expression compilation loop.
#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_number(
    mut env: JNIEnv,
    _: JClass,
    request: JString,
) -> jstring {
    let result = read(&mut env, &request)
        .and_then(|s| serde_json::from_str::<layer_ui::NumericRequest>(&s).map_err(error))
        .and_then(|request| request.resolve())
        .and_then(|value| serde_json::to_string(&value).map_err(error));
    string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_colorWheelHit(
    mut env: JNIEnv,
    _: JClass,
    request: JString,
) -> jstring {
    #[derive(serde::Deserialize)]
    struct Hit {
        size: f32,
        point: [f32; 2],
        space: layer_ui::ColorSpace,
    }
    let result = read(&mut env, &request)
        .and_then(|s| serde_json::from_str::<Hit>(&s).map_err(error))
        .and_then(|hit| {
            serde_json::to_string(
                &layer_ui::ColorWheelGeometry::new(hit.size)
                    .and_then(|geometry| geometry.hit(hit.point, hit.space)),
            )
            .map_err(error)
        });
    string(&mut env, result)
}
