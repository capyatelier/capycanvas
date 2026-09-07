#![cfg(target_arch = "wasm32")]

use layer_core::Point;
use layer_engine::{PenEvent, PenPhase, SampleFlags, ToolKind};
use layer_render_wgpu::{ViewportPresenter, WgpuRasterizer};
use layer_ui::{UiAction, UiSession, ui_catalog};
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct WebApp {
    session: UiSession<WgpuRasterizer>,
    instance: wgpu::Instance,
    canvas: web_sys::HtmlCanvasElement,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    presenter: ViewportPresenter,
    sequence: u64,
}

fn js(value: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&value.to_string())
}
fn serialize(value: &impl Serialize) -> Result<JsValue, JsValue> {
    value
        .serialize(
            &serde_wasm_bindgen::Serializer::new()
                .serialize_large_number_types_as_bigints(true)
                .serialize_maps_as_objects(true),
        )
        .map_err(js)
}
fn phase(value: u8) -> Result<PenPhase, JsValue> {
    match value {
        0 => Ok(PenPhase::Hover),
        1 => Ok(PenPhase::Down),
        2 => Ok(PenPhase::Move),
        3 => Ok(PenPhase::Up),
        4 => Ok(PenPhase::Cancel),
        _ => Err(js("Invalid pointer phase")),
    }
}

#[wasm_bindgen]
impl WebApp {
    /// Display-only hover data, independent of the high-rate paint queue.
    pub fn cursor_input(&mut self, sample: &[f64]) {
        let event = (sample.len() == 7).then(|| PenEvent {
            device_id: 0,
            sequence: 0,
            timestamp_ns: (sample[6] * 1_000_000.0) as u64,
            view_revision: self.session.state().camera.revision,
            surface_position: Point {
                x: sample[0] as f32,
                y: sample[1] as f32,
            },
            pressure: sample[2] as f32,
            tilt_radians: [sample[3] as f32, sample[4] as f32],
            twist_radians: sample[5] as f32,
            distance: 0.0,
            phase: PenPhase::Hover,
            tool: ToolKind::Pen,
            flags: SampleFlags::PRIMARY,
        });
        self.session.cursor_input(event);
    }
    pub fn canvas_cursor(&mut self) -> Result<JsValue, JsValue> {
        serialize(&self.session.canvas_cursor())
    }
    pub async fn create(canvas: web_sys::HtmlCanvasElement) -> Result<WebApp, JsValue> {
        console_error_panic_hook::set_once();
        let width = canvas.width().max(1);
        let height = canvas.height().max(1);
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::BROWSER_WEBGPU;
        let instance = wgpu::Instance::new(descriptor);
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(js)?;
        let options = wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::None,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        };
        // Chrome can return null while its hardware GPU process initializes.
        // Retry once; an unavailable GPU still becomes an explicit startup error.
        let adapter = match instance.request_adapter(&options).await {
            Ok(adapter) => adapter,
            Err(_) => instance.request_adapter(&options).await.map_err(js)?,
        };
        let config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| js("WebGPU canvas format unavailable"))?;
        let limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("web canvas GPU"),
                required_limits: limits,
                ..Default::default()
            })
            .await
            .map_err(js)?;
        surface.configure(&device, &config);
        device.on_uncaptured_error(std::sync::Arc::new(|error| {
            web_sys::console::error_1(&js(error))
        }));
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let presenter = ViewportPresenter::new(&device, config.format);
        let renderer = WgpuRasterizer::from_wgpu(adapter, device, queue).map_err(js)?;
        if let Some(error) = validation.pop().await {
            return Err(js(error));
        }
        let mut session = UiSession::blank(renderer, [width, height]).map_err(js)?;
        session.set_platform(layer_ui::Platform::Web);
        Ok(Self {
            session,
            instance,
            canvas,
            surface,
            config,
            presenter,
            sequence: 0,
        })
    }

    pub fn state(&self) -> Result<JsValue, JsValue> {
        serialize(self.session.state())
    }
    pub fn catalog(&self) -> Result<JsValue, JsValue> {
        serialize(&ui_catalog())
    }
    pub fn preferences(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.preferences())
    }
    pub fn dispatch(&mut self, action: JsValue) -> Result<JsValue, JsValue> {
        let action: UiAction = serde_wasm_bindgen::from_value(action).map_err(js)?;
        serialize(&self.session.dispatch(action).map_err(js)?)
    }
    pub fn input(&mut self, input: JsValue) -> Result<JsValue, JsValue> {
        serialize(
            &self
                .session
                .input(serde_wasm_bindgen::from_value(input).map_err(js)?)
                .map_err(js)?,
        )
    }
    pub fn layout(&self, width: f32, height: f32) -> Result<JsValue, JsValue> {
        serialize(&self.session.layout([width, height]))
    }
    pub fn drop_hint(
        &self,
        width: f32,
        height: f32,
        x: f32,
        y: f32,
        tabs: JsValue,
        item: JsValue,
    ) -> Result<JsValue, JsValue> {
        let tabs: Vec<layer_ui::TabHit> = serde_wasm_bindgen::from_value(tabs).map_err(js)?;
        let item: layer_ui::DockItem = serde_wasm_bindgen::from_value(item).map_err(js)?;
        match self.session.drop_hint([width, height], [x, y], &tabs, item) {
            Some(hint) => serialize(&hint),
            None => Ok(JsValue::NULL),
        }
    }
    pub fn scroll(
        &mut self,
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
        dpi: f32,
        modifiers: u8,
    ) -> Result<JsValue, JsValue> {
        serialize(
            &self
                .session
                .scroll(
                    [x, y],
                    [dx, dy],
                    dpi,
                    modifiers & 1 != 0,
                    modifiers & 2 != 0,
                )
                .map_err(js)?,
        )
    }
    pub fn viewport(
        &mut self,
        logical_width: f32,
        logical_height: f32,
        width: u32,
        height: u32,
    ) -> Result<JsValue, JsValue> {
        let change = self
            .session
            .set_viewport([logical_width, logical_height], [width, height])
            .map_err(js)?;
        if [width, height] != [self.config.width, self.config.height] {
            self.config.width = width;
            self.config.height = height;
            self.surface
                .configure(self.session.engine().backend().device(), &self.config);
        }
        serialize(&change)
    }

    /// Packed history records: id, phase, x, y, pressure, tilt x/y, twist,
    /// timestamp milliseconds, flags, device kind (0 pen / 1 mouse / 2 eraser).
    /// Returns consumed record count if the bounded input queue fills.
    pub fn pen(&mut self, records: &[f64], view_revision: u64) -> Result<u32, JsValue> {
        if !records.len().is_multiple_of(11) {
            return Err(js("Invalid pen batch length"));
        }
        for (index, item) in records.chunks_exact(11).enumerate() {
            if !item.iter().all(|n| n.is_finite()) {
                return Err(js("Invalid pen sample"));
            }
            let event = PenEvent {
                device_id: item[0] as u64,
                sequence: self.sequence + 1,
                timestamp_ns: (item[8].max(0.0) * 1_000_000.0) as u64,
                view_revision,
                surface_position: Point {
                    x: item[2] as f32,
                    y: item[3] as f32,
                },
                pressure: item[4] as f32,
                tilt_radians: [item[5] as f32, item[6] as f32],
                twist_radians: item[7] as f32,
                distance: 0.0,
                phase: phase(item[1] as u8)?,
                flags: SampleFlags(item[9] as u16),
                tool: match item[10] as u8 {
                    1 => ToolKind::Mouse,
                    2 => ToolKind::Eraser,
                    _ => ToolKind::Pen,
                },
            };
            if self.session.pen(event).is_err() {
                return Ok(index as u32);
            }
            self.sequence += 1;
        }
        Ok((records.len() / 11) as u32)
    }
    pub fn gesture(
        &mut self,
        from_x: f32,
        from_y: f32,
        to_x: f32,
        to_y: f32,
        scale: f32,
        rotation: f32,
    ) -> Result<JsValue, JsValue> {
        serialize(
            &self
                .session
                .gesture([from_x, from_y], [to_x, to_y], scale, rotation)
                .map_err(js)?,
        )
    }
    pub fn frame(&mut self, now_ms: f64, presentation_ms: f64) -> Result<JsValue, JsValue> {
        let change = self
            .session
            .frame(
                (now_ms * 1_000_000.0) as u64,
                (presentation_ms * 1_000_000.0) as u64,
            )
            .map_err(js)?;
        let target = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(target)
            | wgpu::CurrentSurfaceTexture::Suboptimal(target) => target,
            wgpu::CurrentSurfaceTexture::Lost => {
                self.surface = self
                    .instance
                    .create_surface(wgpu::SurfaceTarget::Canvas(self.canvas.clone()))
                    .map_err(js)?;
                self.surface
                    .configure(self.session.engine().backend().device(), &self.config);
                return serialize(&layer_ui::UiChange {
                    canvas_wake: true,
                    ..change
                });
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface
                    .configure(self.session.engine().backend().device(), &self.config);
                return serialize(&layer_ui::UiChange {
                    canvas_wake: true,
                    ..change
                });
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                return serialize(&layer_ui::UiChange {
                    canvas_wake: true,
                    ..change
                });
            }
            wgpu::CurrentSurfaceTexture::Occluded => return serialize(&change),
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(js("WebGPU surface validation failed"));
            }
        };
        let surround = self.session.state().theme.canvas_surround();
        self.presenter.present(
            self.session.engine().backend(),
            &target.texture.create_view(&Default::default()),
            self.session.state().camera.view(),
            surround,
        );
        self.session.engine().backend().queue().present(target);
        serialize(&change)
    }
}
