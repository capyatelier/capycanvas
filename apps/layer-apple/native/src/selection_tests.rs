//! Shared selection tools and masks through the Apple action/pointer ABI and Metal.
use super::*;
use super::fixtures::{drag, selection_app, selection_bounds};
use std::time::{Duration, Instant};

fn key(app: &App, key: &str, pressed: bool, shift: bool, alt: bool) {
    app.request(1, json!({"type":"key","key":key,"pressed":pressed,"repeat":false,
        "modifiers":{"command":false,"shift":shift,"alt":alt}}));
}

fn assert_bounds(app: &App, expected: [f32; 4]) {
    let actual = selection_bounds(app).expect("selection");
    for (a, e) in actual.into_iter().zip(expected) {
        assert!((a - e).abs() <= 1.5, "selection bounds {actual:?}, expected {expected:?}");
    }
}

#[test]
fn apple_selection_tools_latch_modifiers_and_keep_one_step_history() {
    for platform in [0, 1] {
        let app = selection_app(platform);
        let state = app.state();
        for id in ["select", "rectangle_select", "ellipse_select", "polygon_select", "color_select", "tonal_select", "quick_mask"] {
            let command = state["commands"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap();
            assert_eq!(command["enabled"], true, "{id} is available on Apple");
        }
        for device in [0, 1] {
            app.invoke("rectangle_select");
            drag(&app, 10 + device as u64, device, [8., 8.], [24., 24.]);
            assert_bounds(&app, [8., 8., 24., 24.]);
            key(&app, "Shift", true, true, false);
            drag(&app, 20 + device as u64, device, [30., 30.], [50., 48.]);
            key(&app, "Shift", false, false, false);
            assert_bounds(&app, [8., 8., 50., 48.]);
            app.invoke("undo");
            app.draw_until_idle();
            assert_bounds(&app, [8., 8., 24., 24.]);
            app.invoke("redo");
            app.draw_until_idle();
            assert_bounds(&app, [8., 8., 50., 48.]);
            key(&app, "Alt", true, false, true);
            drag(&app, 30 + device as u64, device, [28., 28.], [52., 52.]);
            key(&app, "Alt", false, false, false);
            assert_bounds(&app, [8., 8., 24., 24.]);
            app.invoke("ellipse_select");
            drag(&app, 40 + device as u64, device, [28., 28.], [52., 52.]);
            assert_bounds(&app, [28., 28., 52., 52.]);
            app.invoke("deselect");
            app.draw_until_idle();
            assert!(selection_bounds(&app).is_none());
        }
    }
}

#[test]
fn apple_grow_and_shrink_apply_once_and_cancel_without_changes() {
    for platform in [0, 1] {
        let app = selection_app(platform);
        app.invoke("rectangle_select");
        drag(&app, 1, 1, [16., 16.], [40., 40.]);
        assert_bounds(&app, [16., 16., 40., 40.]);
        app.action(json!({"type":"selection","action":{"op":"begin_refine","kind":"grow","layer":null}}));
        assert!(!app.state()["layer_tools"]["selection_resize"].is_null());
        app.action(json!({"type":"selection","action":{"op":"resize_radius","radius":4}}));
        app.action(json!({"type":"selection","action":{"op":"apply_resize"}}));
        let deadline = Instant::now() + Duration::from_secs(10);
        while selection_bounds(&app).is_some_and(|b| b[0] > 13.) {
            assert!(Instant::now() < deadline, "Grow did not complete");
            app.draw_until_prepared(true);
        }
        app.draw_until_idle();
        assert!(app.state()["layer_tools"]["selection_resize"].is_null());
        assert_bounds(&app, [12., 12., 44., 44.]);
        app.invoke("undo");
        app.draw_until_idle();
        assert_bounds(&app, [16., 16., 40., 40.]);
        app.action(json!({"type":"selection","action":{"op":"begin_refine","kind":"shrink","layer":null}}));
        app.action(json!({"type":"selection","action":{"op":"resize_radius","radius":6}}));
        app.action(json!({"type":"selection","action":{"op":"cancel_resize"}}));
        app.draw_until_idle();
        assert!(app.state()["layer_tools"]["selection_resize"].is_null());
        assert_bounds(&app, [16., 16., 40., 40.]);
    }
}

#[test]
fn apple_quick_mask_rows_colors_thumbnails_and_saved_layers_use_shared_models() {
    for platform in [0, 1] {
        let app = selection_app(platform);
        let artwork = app.state()["colors"].clone();
        app.invoke("quick_mask");
        let state = app.state();
        let row = &state["layers"][0];
        assert_eq!(row["id"], 0);
        assert_eq!(row["quick_mask"], true);
        assert_eq!(row["selection_layer"], true);
        assert_eq!(row["can_rename"], false);
        assert!(!state["layer_tools"]["mask_editing"].is_null());
        app.action(json!({"type":"color","action":{"op":"quick_color","white":false}}));
        let state = app.state();
        assert_eq!(state["colors"], artwork, "Mask painting colors never change artwork colors");
        assert_ne!(state["layer_tools"]["mask_editing"]["colors"]["slot"], json!(null));
        drag(&app, 1, 0, [10., 10.], [50., 50.]);
        let mut reply = app.request(2, json!({"type":"layer_thumbnails","requests":[[7,0]]})).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while reply["accepted"].as_array().unwrap().is_empty() {
            assert!(Instant::now() < deadline, "Quick Mask thumbnail was not accepted");
            app.draw_frame();
            reply = app.request(2, json!({"type":"layer_thumbnails","requests":[[7,0]]})).unwrap();
        }
        while reply["images"].as_array().unwrap().is_empty() {
            assert!(Instant::now() < deadline, "Quick Mask thumbnail readback timed out");
            std::thread::sleep(Duration::from_millis(1));
            reply = app.request(2, json!({"type":"layer_thumbnails","requests":[]})).unwrap();
        }
        assert_eq!(reply["images"][0][0], 7);
        let menu = app.request(2, json!({"type":"layer_menu","id":0,"mask":false})).unwrap();
        assert!(!menu["sections"].as_array().unwrap().is_empty());
        app.invoke("save_selection_layer");
        app.draw_until_idle();
        let state = app.state();
        assert_ne!(state["layers"][0]["id"], 0, "Saving exits Quick Mask");
        let saved = state["layers"].as_array().unwrap().iter()
            .find(|l| l["selection_layer"] == true).expect("saved selection layer").clone();
        assert_eq!(saved["can_rename"], true);
        assert!(!saved["load_selection_tooltip"].as_str().unwrap().is_empty());
        let menu = app.request(2, json!({"type":"selection_menu","kind":"selection"})).unwrap();
        assert!(!menu["sections"].as_array().unwrap().is_empty());
        app.action(json!({"type":"selection","action":{"op":"load_layer","id":saved["id"],"mode":"new","inverted":false}}));
        app.draw_until_idle();
        assert!(selection_bounds(&app).is_some());
    }
}
