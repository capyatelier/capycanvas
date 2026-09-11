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
        app.host
            .prepare_canvas_frame(2_000_000_000, 2_000_000_000, true)
            .unwrap();
    }
    fn layer_action(&self, action: Value) {
        self.action(json!({"type": "layer", "action": action}));
    }
    fn layer(&self, id: u64) -> Value {
        self.state()["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["id"] == id)
            .unwrap()
            .clone()
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

#[test]
fn layer_panel_actions_preserve_targets_masks_hierarchy_and_menu_policy() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let original = app.state()["layer_tools"]["editing_layer"]["id"]
            .as_u64()
            .unwrap();
        app.layer_action(json!({"op":"new","group":false,"clipped":false}));
        let id = app.state()["layer_tools"]["editing_layer"]["id"]
            .as_u64()
            .unwrap();
        app.layer_action(json!({"op":"begin_rename","id":id}));
        assert_eq!(app.state()["layer_tools"]["rename_layer"], id);
        app.layer_action(json!({"op":"rename","id":id,"name":"Test ink"}));
        assert_eq!(app.layer(id)["label"], "Test ink");
        app.layer_action(json!({"op":"blend","id":id,"value":2}));
        assert_eq!(app.layer(id)["blend"], 2);
        app.action(json!({"type":"set_layer_opacity","opacity":0.35}));
        assert!((app.layer(id)["opacity"].as_f64().unwrap() - 0.35).abs() < 0.00001);
        app.layer_action(json!({"op":"alpha_lock","id":id,"value":true}));
        assert_eq!(app.layer(id)["alpha_locked"], true);
        app.layer_action(json!({"op":"lock","id":id,"value":true}));
        assert_eq!(app.state()["layer_tools"]["controls"]["opacity"], false);
        let locked = app
            .request(2, json!({"type":"layer_menu","id":id,"mask":false}))
            .unwrap();
        let rename = locked["sections"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s.as_array().unwrap())
            .find(|item| item["action"]["action"]["op"] == "begin_rename")
            .unwrap();
        assert_eq!(
            rename["enabled"], false,
            "Menu capabilities must come from shared policy"
        );
        app.layer_action(json!({"op":"lock","id":id,"value":false}));
        app.layer_action(json!({"op":"toggle_selection","id":original}));
        assert_eq!(app.state()["layer_tools"]["editing_layer"]["id"], id);
        assert_eq!(app.layer(original)["selected"], true);
        app.layer_action(json!({"op":"context","id":id,"mask":false}));
        assert_eq!(
            app.layer(original)["selected"],
            true,
            "Context on a selected row keeps checked selection"
        );
        app.layer_action(json!({"op":"reference_selection"}));
        assert_eq!(app.layer(original)["reference"], true);
        assert_eq!(app.layer(id)["reference"], true);
        app.layer_action(json!({"op":"add_mask","id":id,"replace":false}));
        assert_eq!(app.layer(id)["has_mask"], true);
        app.layer_action(json!({"op":"select","id":id,"mask":true}));
        assert_eq!(app.layer(id)["mask_selected"], true);
        app.layer_action(json!({"op":"link_mask","id":id,"value":false}));
        app.layer_action(json!({"op":"enable_mask","id":id,"value":false}));
        assert_eq!(app.layer(id)["mask_linked"], false);
        let menu = app
            .request(2, json!({"type":"layer_menu","id":id,"mask":true}))
            .unwrap();
        let enabled = menu["sections"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s.as_array().unwrap())
            .find(|item| item["label"] == "Enable mask")
            .unwrap();
        assert_eq!(enabled["selected"], false);
        assert_eq!(enabled["enabled"], true);
        app.layer_action(json!({"op":"delete_mask","id":id}));
        assert_eq!(app.layer(id)["has_mask"], false);
        app.invoke("undo");
        assert_eq!(app.layer(id)["has_mask"], true);
        app.layer_action(json!({"op":"new","group":true,"clipped":false}));
        let group = app.state()["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["group"] == true)
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        app.layer_action(json!({"op":"drop","id":original,"target":group,"fraction":0.5}));
        assert_eq!(app.layer(original)["depth"], 1);
        app.layer_action(json!({"op":"collapse","id":group}));
        assert!(
            !app.state()["layers"]
                .as_array()
                .unwrap()
                .iter()
                .any(|l| l["id"] == original)
        );
        app.layer_action(json!({"op":"collapse","id":group}));
        assert_eq!(app.layer(original)["depth"], 1);
    }
}

#[test]
fn image_import_changes_gpu_pixels_is_undoable_and_produces_a_thumbnail() {
    use std::time::{Duration, Instant};
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().unwrap());
        app.draw_frame();
        let paper = app.pixels();
        let name = CString::new("Test image").unwrap();
        let rgba = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 128, 0, 0, 0, 0];
        let before = app.state();
        assert_eq!(
            unsafe { capy_apple_import_layer(app.0, name.as_ptr(), 2, 2, rgba.as_ptr(), 15) },
            -1
        );
        assert_eq!(
            app.state(),
            before,
            "Incomplete pixels cannot mutate the document"
        );
        assert_eq!(
            unsafe {
                capy_apple_import_layer(app.0, name.as_ptr(), 2, 2, rgba.as_ptr(), rgba.len())
            },
            0
        );
        let id = app.state()["layer_tools"]["editing_layer"]["id"]
            .as_u64()
            .unwrap();
        assert_eq!(app.layer(id)["label"], "Test image");
        app.draw_frame();
        let imported = app.pixels();
        assert!(imported != paper, "Import must reach the GPU document");
        let mut reply = app
            .request(2, json!({"type":"layer_thumbnails","requests":[[99,id]]}))
            .unwrap();
        assert_eq!(reply["accepted"], json!([99]));
        let deadline = Instant::now() + Duration::from_secs(5);
        while reply["images"].as_array().unwrap().is_empty() {
            assert!(Instant::now() < deadline, "Thumbnail readback timed out");
            std::thread::sleep(Duration::from_millis(1));
            reply = app
                .request(2, json!({"type":"layer_thumbnails","requests":[]}))
                .unwrap();
        }
        let image = &reply["images"][0];
        assert_eq!(image[0], 99);
        assert_eq!(
            image[3].as_array().unwrap().len(),
            image[1].as_u64().unwrap() as usize * image[2].as_u64().unwrap() as usize * 4
        );
        app.invoke("undo");
        app.draw_frame();
        assert!(app.pixels() == paper);
        app.invoke("redo");
        app.draw_frame();
        assert!(app.pixels() == imported);
    }
}

#[test]
fn stateless_numeric_input_uses_shared_policy_without_a_session() {
    let control = serde_json::to_value(layer_ui::ui_catalog()).unwrap()["layer_opacity"].clone();
    let resolve = |operation| {
        let json =
            CString::new(json!({"control":control,"value":1.,"operation":operation}).to_string())
                .unwrap();
        let output = unsafe { capy_apple_numeric(json.as_ptr()) };
        assert!(!output.is_null());
        let result: Value =
            serde_json::from_slice(unsafe { CStr::from_ptr(output) }.to_bytes()).unwrap();
        unsafe { capy_apple_string_free(output) };
        result
    };
    assert_eq!(
        resolve(json!({"type":"expression","text":"25+25"}))["value"],
        0.5
    );
    assert_eq!(
        resolve(json!({"type":"position","position":0.25}))["value"],
        0.25
    );
    assert_eq!(resolve(json!({"type":"format"}))["text"], "100");
    assert!(resolve(json!({"type":"expression","text":"invalid"}))["error"].is_string());
}

#[test]
fn apple_tool_panels_edit_every_visible_brush_setting_through_the_abi() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let snapshot = app.request(3, Value::Null).unwrap();
        let menu_action = snapshot["workspace_menu"]["sections"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s.as_array().unwrap())
            .find(|item| item["action"]["action"]["panel"] == "tool_settings")
            .expect("Tool Settings must be reachable from Workspace")["action"]
            .clone();
        app.action(menu_action);
        let snapshot = app.request(3, Value::Null).unwrap();
        assert!(
            snapshot["panels"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["id"] == "tool_settings"
                    && p["controls"][0]["control"] == "tool_settings")
        );
        let catalog = app.request(2, json!({"type":"catalog"})).unwrap();
        let mut edited = 0;
        for category in catalog["brush_categories"].as_array().unwrap() {
            for brush in category["brushes"].as_array().unwrap() {
                app.action(json!({"type":"select_brush","id":brush["id"]}));
                let state = app.state();
                assert!(
                    state["tool_set"]["subtools"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|item| item["preview"] == brush["id"] && item["selected"] == true)
                );
                for setting in state["tool_settings"].as_array().unwrap() {
                    // Re-select to prevent a previous edit changing the schema.
                    app.action(json!({"type":"select_brush","id":brush["id"]}));
                    let resolved = app
                        .request(
                            4,
                            json!({"control":setting["numeric"],
                        "value":setting["value"],"operation":{"type":"position","position":0.37}}),
                        )
                        .unwrap();
                    app.action(json!({"type":"set_tool_setting","id":setting["id"],"value":resolved["value"]}));
                    let after = app.state();
                    let actual = after["tool_settings"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|s| s["id"] == setting["id"])
                        .unwrap();
                    assert_eq!(
                        actual["value"].as_f64().unwrap() as f32,
                        resolved["value"].as_f64().unwrap() as f32,
                        "{} {}",
                        brush["label"],
                        setting["id"]
                    );
                    edited += 1;
                }
            }
        }
        assert!(
            edited > 100,
            "Exercise the complete catalog, including wet and liquify controls"
        );
        app.invoke("ruler");
        for id in ["snap_rulers", "show_rulers"] {
            let before = app.state();
            assert!(
                before["tool_actions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|a| a["command"] == id && a["checkable"] == true)
            );
            let checked = before["commands"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == id)
                .unwrap()["selected"]
                .clone();
            app.invoke(id);
            let after = app.state();
            assert_ne!(
                after["commands"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|c| c["id"] == id)
                    .unwrap()["selected"],
                checked
            );
        }
    }
}

#[test]
fn apple_transform_settings_and_actions_preserve_pixel_transactions() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().expect("Hardware GPU required"));
        app.draw_frame();
        app.stroke();
        app.draw_frame();
        let painted = app.pixels();
        app.invoke("scale_rotate");
        assert_eq!(app.state()["tool_settings"].as_array().unwrap().len(), 5);
        for command in ["transform_aspect", "apply_transform", "cancel_transform"] {
            assert!(
                app.state()["tool_actions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|a| a["command"] == command)
            );
        }
        app.action(json!({"type":"set_tool_setting","id":"transform_x","value":48}));
        app.draw_frame();
        assert!(
            app.pixels() != painted,
            "Numeric edits must update the live GPU preview"
        );
        app.invoke("cancel_transform");
        app.draw_frame();
        assert!(
            app.pixels() == painted,
            "Cancel must restore all original document pixels"
        );
        app.invoke("scale_rotate");
        app.action(json!({"type":"set_tool_setting","id":"transform_x","value":48}));
        let before = app.state()["tool_settings"].clone();
        let invalid = CString::new(
            json!({"type":"set_tool_setting","id":"transform_width","value":0}).to_string(),
        )
        .unwrap();
        assert!(unsafe { capy_apple_request(app.0, 0, invalid.as_ptr()) }.is_null());
        assert!(
            !unsafe { capy_apple_error(app.0) }.is_null(),
            "Semantic rejection must return field feedback"
        );
        assert_eq!(
            app.state()["tool_settings"],
            before,
            "Rejected scale must preserve accepted fields"
        );
        app.invoke("apply_transform");
        app.draw_frame();
        let transformed = app.pixels();
        assert!(transformed != painted);
        app.invoke("undo");
        app.draw_frame();
        assert!(app.pixels() == painted);
        app.invoke("redo");
        app.draw_frame();
        assert!(app.pixels() == transformed);
    }
}

#[test]
fn optional_gpu_timing_has_explicit_uninitialized_state_and_bounded_abi() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let mut stats = layer_render_wgpu::GpuFrameTimingStats::default();
        unsafe {
            assert_eq!(capy_apple_gpu_timing(app.0, 1), 0);
            assert_eq!(capy_apple_frame(app.0, 1, 1, std::ptr::null_mut()), 0);
            assert_eq!(
                capy_apple_take_gpu_timing(app.0, std::ptr::null_mut(), 0, &mut stats),
                0
            );
            assert_eq!(
                stats.support, 0,
                "No renderer must not report available timestamps"
            );
            assert_eq!(stats.requested, 0);
            assert_eq!(
                capy_apple_take_gpu_timing(app.0, std::ptr::null_mut(), 1, &mut stats),
                -1
            );
            assert_eq!(
                capy_apple_take_gpu_timing(app.0, std::ptr::null_mut(), 257, &mut stats),
                -1
            );
            assert_eq!(capy_apple_gpu_timing(app.0, 2), -1);
            assert_eq!(capy_apple_gpu_timing(app.0, 0), 0);
        }
    }
}
