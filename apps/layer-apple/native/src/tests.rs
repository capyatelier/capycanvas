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
    fn stroke(&self) {
        let revision = unsafe { capy_apple_camera_revision(self.0) };
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
                capy_apple_pointer(
                    self.0,
                    1,
                    1,
                    0,
                    records.as_ptr(),
                    records.len(),
                    0,
                    revision,
                )
            },
            0
        );
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
        app.stroke();
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

#[test]
fn staged_paper_preserves_pending_ink_and_reaches_brush_readiness() {
    use layer_render_wgpu::WgpuRasterizer;
    use std::time::{Duration, Instant};
    for platform in [0, 1] {
        let app = App::new(platform);
        let reference = WgpuRasterizer::new_headless().expect("Hardware GPU required");
        let staged = WgpuRasterizer::from_wgpu_staged(
            reference.adapter().clone(),
            reference.device().clone(),
            reference.queue().clone(),
        )
        .unwrap();
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(reference);
        app.action(json!({"type": "set_color", "rgba": [0,0,0,1]}));
        // Accepted engine work must survive the first paper-only submission.
        app.stroke();
        {
            let host = &mut unsafe { &mut *app.0 }.host;
            host.session.renderer_mut().0 = Some(staged);
            host.startup = Default::default();
            host.prepare_canvas_frame(2_000_000_000, 2_000_000_000, false)
                .unwrap();
            assert!(!host.startup.canvas_ready);
            assert!(host.dirty, "Startup must keep the frame driver awake");
        }
        let paper = app.pixels();
        app.action(json!({"type": "set_brush_opacity", "value": 0.4}));
        assert!((app.state()["brush"]["opacity"].as_f64().unwrap() - 0.4).abs() < 0.00001);
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let host = &mut unsafe { &mut *app.0 }.host;
            host.prepare_canvas_frame(2_100_000_000, 2_100_000_000, true)
                .unwrap();
            if host.startup.brush_ready {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Staged brush readiness timed out"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            app.pixels() != paper,
            "The initial paper frame must retain pending stroke work"
        );
        app.invoke("undo");
        app.draw_frame();
        assert!(
            app.pixels() == paper,
            "Replayed ink remains exactly undoable"
        );
    }
}
