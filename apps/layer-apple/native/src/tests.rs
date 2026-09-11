//! Test editor effects after UI action dispatch, without automating OS menus.
use super::*;
use layer_render::CanvasRenderer;
use serde_json::{Value, json};

struct App(*mut CapyApple);
impl App {
    fn new(platform: u32) -> Self {
        let app = Self(capy_apple_create(platform));
        assert!(!app.0.is_null());
        assert_eq!(unsafe { capy_apple_resize(app.0, 1200, 900, 1.) }, 0);
        app
    }
    fn request(&self, kind: u32, value: Value) -> Option<Value> {
        let text = CString::new(value.to_string()).unwrap();
        let result = unsafe { capy_apple_request(self.0, kind, text.as_ptr()) };
        let error = unsafe { capy_apple_error(self.0) };
        assert!(
            error.is_null(),
            "{}",
            unsafe { CStr::from_ptr(error) }.to_string_lossy()
        );
        if result.is_null() {
            return None;
        }
        let value = serde_json::from_slice(unsafe { CStr::from_ptr(result) }.to_bytes()).unwrap();
        unsafe { capy_apple_string_free(result) };
        Some(value)
    }
    fn action(&self, action: Value) {
        self.request(0, action).unwrap();
    }
    fn invoke(&self, command: &str) {
        self.action(json!({"type": "invoke", "command": command}));
    }
    fn state(&self) -> Value {
        serde_json::to_value(unsafe { &*self.0 }.host.session.state()).unwrap()
    }
    fn draw_frame(&self) {
        let app = unsafe { &mut *self.0 };
        let previous = app.host.session.state().revision;
        let change = app
            .host
            .session
            .frame(2_000_000_000, 2_000_000_000)
            .unwrap();
        app.host.apply_change(previous, change);
    }
    fn pixels(&self) -> Vec<u8> {
        let renderer = unsafe { &mut *self.0 }
            .host
            .session
            .renderer_mut()
            .0
            .as_mut()
            .unwrap();
        renderer.request_readback(1).unwrap();
        renderer
            .device()
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(5)),
            })
            .unwrap();
        renderer
            .take_readback()
            .expect("GPU readback completed")
            .unwrap()
            .bytes
    }
}
impl Drop for App {
    fn drop(&mut self) {
        unsafe { capy_apple_destroy(self.0) }
    }
}

#[test]
fn ui_actions_change_only_the_addressed_apple_session() {
    for platform in [0, 1] {
        let first = App::new(platform);
        let second = App::new(platform);
        let second_before = second.state();
        first.action(json!({"type": "set_brush_size", "value": 42}));
        first.action(json!({"type": "set_brush_opacity", "value": 0.4}));
        assert_eq!(first.state()["brush"]["diameter"], 42.);
        assert!((first.state()["brush"]["opacity"].as_f64().unwrap() - 0.4).abs() < 0.00001);
        let zoom = first.state()["camera"]["zoom"].as_f64().unwrap();
        first.invoke("zoom_in");
        assert!(first.state()["camera"]["zoom"].as_f64().unwrap() > zoom);
        first.invoke("settings");
        assert!(!first.request(3, Value::Null).unwrap()["preferences"].is_null());
        first.action(json!({"type": "close_settings"}));
        assert!(first.request(3, Value::Null).unwrap()["preferences"].is_null());
        assert_eq!(
            second.state(),
            second_before,
            "Actions must stay in their editor session"
        );
    }
}

#[test]
fn apple_paint_undo_redo_restores_exact_document_pixels() {
    for platform in [0, 1] {
        let app = App::new(platform);
        // A real hardware renderer with no window: these are document/output
        // checks, not display timing or native physical-input acceptance.
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().expect("Hardware GPU required"));
        app.action(json!({"type": "set_color", "rgba": [0,0,0,1]}));
        app.action(json!({"type": "set_brush_size", "value": 32}));
        app.draw_frame();
        let initial = app.pixels();
        let revision = unsafe { capy_apple_camera_revision(app.0) };
        let records = [
            500.,
            400.,
            1.,
            0.,
            0.,
            0.,
            0.,
            1_000_000_000.,
            1.,
            600.,
            500.,
            1.,
            0.,
            0.,
            0.,
            0.,
            1_010_000_000.,
            2.,
            650.,
            520.,
            1.,
            0.,
            0.,
            0.,
            0.,
            1_020_000_000.,
            3.,
        ];
        assert_eq!(
            unsafe {
                capy_apple_pointer(app.0, 1, 1, 0, records.as_ptr(), records.len(), 0, revision)
            },
            0
        );
        app.draw_frame();
        let painted = app.pixels();
        assert!(painted != initial, "Pen-up must leave real document pixels");
        app.invoke("undo");
        app.draw_frame();
        assert!(
            app.pixels() == initial,
            "Undo must restore every document byte"
        );
        app.invoke("redo");
        app.draw_frame();
        assert!(
            app.pixels() == painted,
            "Redo must reproduce the committed stroke exactly"
        );
    }
}
