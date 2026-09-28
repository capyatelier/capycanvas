//! Tonal range selection through the Apple action/pointer ABI and Metal.
use super::*;
use super::fixtures::{drag, selection_app, selection_bounds, surface, until};

fn tonal_app(platform: u32) -> App {
    let app = selection_app(platform);
    app.invoke("blend_linear");
    app.action(json!({"type":"set_color","rgba":[0.02,0.02,0.02,1.]}));
    app.draw_until_idle();
    app.invoke("rectangle_select");
    drag(&app, 21, 1, [8., 8.], [24., 24.]);
    for command in ["brush", "fill_selection", "deselect"] { app.invoke(command); }
    app.draw_until_idle();
    app
}

fn preset(app: &App, index: u32) {
    app.action(json!({"type":"tonal","action":{"kind":"preset","index":index}}));
}

fn setting(app: &App, id: &str, value: f64) {
    app.action(json!({"type":"set_tool_setting","id":id,"value":value}));
}

fn bounds(app: &App) -> [f64; 2] {
    let settings = app.state()["tool_settings"].clone();
    ["tonal_lower", "tonal_upper"].map(|id| settings.as_array().unwrap().iter().find(|s| s["id"] == id).unwrap()["value"].as_f64().unwrap())
}

#[test]
fn apple_tonal_presets_refine_in_one_history_step_through_metal() {
    for platform in [0, 1] {
        let app = tonal_app(platform);
        app.invoke("tonal_select");
        let state = app.state();
        let subtools = state["tool_set"]["subtools"].as_array().unwrap();
        assert_eq!(subtools.len(), 8);
        assert_eq!(subtools[7]["icon"], "tonal-select");
        let tones = &state["tool_extra"][0]["Choice"];
        assert_eq!(tones["id"], "tonal-tones");
        let icons: Vec<_> = tones["items"].as_array().unwrap().iter().map(|i| i["icon"].as_str().unwrap().to_owned()).collect();
        assert_eq!(icons, ["tonal-shadows", "tonal-mid-shadows", "tonal-midtones", "tonal-mid-highlights", "tonal-highlights", "tonal-custom"]);
        let ids: Vec<_> = state["tool_settings"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap().to_owned()).collect();
        assert_eq!(ids, ["tonal_softness", "selection_feather"]);
        assert!(selection_bounds(&app).is_none());
        preset(&app, 1);
        until(&app, "mid-shadows mask", || selection_bounds(&app).is_some_and(|b| b[2] > b[0]));
        app.draw_until_idle();
        let shadows = selection_bounds(&app).unwrap();
        assert!(shadows[0] >= 7. && shadows[1] >= 7. && shadows[2] <= 25. && shadows[3] <= 25. && shadows[2] >= 23.,
            "Mid-shadows cover the dark patch only: {shadows:?}");
        setting(&app, "tonal_softness", 0.5);
        setting(&app, "selection_feather", 2.);
        app.draw_until_idle();
        let refined = selection_bounds(&app).unwrap();
        assert!(refined[0] < shadows[0] || refined[2] > shadows[2], "Feather refines the same operation: {refined:?}");
        app.invoke("undo");
        app.draw_until_idle();
        assert!(selection_bounds(&app).is_none(), "One undo restores the baseline");
        app.invoke("redo");
        app.draw_until_idle();
        assert_eq!(selection_bounds(&app), Some(refined));
        assert!(app.try_request(0, &json!({"type":"tonal","action":{"kind":"preset","index":6}})).is_err(), "Bright HDR needs a float document");
    }
}

#[test]
fn apple_tonal_samples_visible_artwork_and_refines_inside_quick_mask() {
    for platform in [0, 1] {
        let app = tonal_app(platform);
        app.invoke("tonal_select");
        preset(&app, 4);
        until(&app, "highlights mask", || selection_bounds(&app).is_some());
        app.draw_until_idle();
        app.invoke("quick_mask");
        let highlights = selection_bounds(&app);
        setting(&app, "tonal_softness", 0.25);
        app.draw_until_idle();
        assert_eq!(app.state()["layer_tools"]["quick_mask"], true, "Refining stays in Quick Mask");
        assert!(selection_bounds(&app).is_some());
        let [x, y] = surface(&app, [16., 16.]);
        let records = [x, y, 1., 0., 0., 0., 0., 2_000_000_000., 1., x, y, 1., 0., 0., 0., 0., 2_010_000_000., 3.];
        assert_eq!(unsafe { capy_apple_pointer(app.0, 40, 1, 0, records.as_ptr(), records.len(), 0, capy_apple_camera_revision(app.0)) }, 0);
        until(&app, "point sample", || app.state()["tool_extra"][0]["Choice"]["items"].as_array().unwrap().last().unwrap()["selected"] == true);
        app.draw_until_idle();
        let [lower, upper] = bounds(&app);
        assert!(lower > -6. && upper < -3. && (upper - lower - 1.).abs() < 0.01,
            "A point sample reads the dark artwork one stop wide, not the Quick Mask overlay: {lower} {upper}");
        assert_ne!(selection_bounds(&app), highlights);
    }
}
