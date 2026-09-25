//! Tonal range selection through the Apple action/pointer ABI and Metal.
use super::*;

fn tonal_app(platform: u32) -> App {
    let app = App::new(platform);
    unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
    app.draw_until_idle();
    let project = ProjectJob::new(&app, true);
    assert_eq!(project.create([64, 64]), 0);
    assert_eq!(unsafe { capy_apple_project_adopt(app.0, project.0, c"Tonal check".as_ptr(), c"".as_ptr()) }, 0);
    app.draw_until_idle();
    app.action(json!({"type":"set_color","rgba":[0.02,0.02,0.02,1.]}));
    app.draw_until_idle();
    app.invoke("rectangle_select");
    drag(&app, 21, [8., 8.], [24., 24.]);
    for command in ["brush", "fill_selection", "deselect"] { app.invoke(command); }
    app.draw_until_idle();
    app
}

fn surface(app: &App, [x, y]: [f64; 2]) -> [f64; 2] {
    let m = unsafe { &*app.0 }.host.session.state().camera.document_to_surface();
    let [x, y] = [x as f32, y as f32];
    [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]].map(f64::from)
}

fn drag(app: &App, id: u64, from: [f64; 2], to: [f64; 2]) {
    let points = [from, [(from[0] + to[0]) / 2., (from[1] + to[1]) / 2.], to];
    let records: Vec<f64> = points.iter().enumerate().flat_map(|(i, &point)| {
        let [x, y] = surface(app, point);
        [x, y, 1., 0., 0., 0., 0., (1_000_000_000 + id * 10_000_000 + i as u64 * 1_000_000) as f64, (i + 1) as f64]
    }).collect();
    assert_eq!(unsafe { capy_apple_pointer(app.0, id, 1, 0, records.as_ptr(), records.len(), 0, capy_apple_camera_revision(app.0)) }, 0);
    app.draw_until_idle();
}

fn settle(app: &App, what: &str, done: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !done() {
        assert!(std::time::Instant::now() < deadline, "{what}");
        app.draw_frame();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    app.draw_until_idle();
}

fn selection_bounds(app: &App) -> Option<[u32; 4]> {
    let document = unsafe { &*app.0 }.host.session.engine().document();
    document.selection.as_ref().and_then(|selection| match &selection.shape {
        layer_core::SelectionShape::Pixels(mask) => Some(mask.bounds()),
        _ => None,
    })
}

fn try_action(app: &App, action: Value) -> Result<(), String> {
    let text = CString::new(action.to_string()).unwrap();
    let result = unsafe { capy_apple_request(app.0, 0, text.as_ptr()) };
    if !result.is_null() { unsafe { capy_apple_string_free(result) }; }
    let error = unsafe { capy_apple_error(app.0) };
    if error.is_null() { Ok(()) } else { Err(unsafe { CStr::from_ptr(error) }.to_string_lossy().into_owned()) }
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
        settle(&app, "mid-shadows mask", || selection_bounds(&app).is_some_and(|b| b[2] > b[0]));
        let shadows = selection_bounds(&app).unwrap();
        assert!(shadows[0] >= 7 && shadows[1] >= 7 && shadows[2] <= 25 && shadows[3] <= 25 && shadows[2] >= 23,
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
        assert!(try_action(&app, json!({"type":"tonal","action":{"kind":"preset","index":6}})).is_err(), "Bright HDR needs a float document");
    }
}

#[test]
fn apple_tonal_custom_range_toolbar_edits_clamp_and_reject_stale_context() {
    for platform in [0, 1] {
        let app = tonal_app(platform);
        let policy = unsafe { &*app.0 }.host.session.state().platform;
        app.action(json!({"type":"restore_workspace","workspace":layer_ui::WorkspaceState {
            layout: layer_ui::WorkspacePreset::Photographer.layout(policy), ..Default::default()
        }}));
        app.invoke("tonal_select");
        preset(&app, 7);
        app.draw_until_idle();
        let options = serde_json::to_value(unsafe { &*app.0 }.host.session.state().tool_options()).unwrap();
        let range = options.as_array().unwrap().iter().find_map(|o| o.get("Range")).expect("Custom publishes one range field").clone();
        assert_eq!(range["id"], "tonal");
        assert_eq!(range["bounds"][0]["id"], "tonal_lower");
        assert_eq!(range["bounds"][1]["id"], "tonal_upper");
        assert!(!options.as_array().unwrap().iter().any(|o| o.get("Numeric").is_some_and(|n| n["id"] == "tonal_lower")));
        let context = serde_json::to_value(unsafe { &*app.0 }.host.session.state().toolbar_context()).unwrap();
        let edit = |id: &str, value: f64| json!({"type":"toolbar_edit","context":context,"action":{"type":"set_tool_setting","id":id,"value":value}});
        app.action(edit("tonal_lower", -20.));
        app.action(edit("tonal_upper", 12.));
        assert_eq!(bounds(&app), [-20., 12.]);
        app.action(edit("tonal_lower", 13.));
        assert_eq!(bounds(&app), [12., 12.], "Crossing bounds clamp in shared Rust");
        app.action(json!({"type":"reset_tool_setting","id":"tonal_upper"}));
        assert_eq!(bounds(&app), [12., 12.]);
        app.invoke("brush");
        let before = app.state()["workspace"].clone();
        assert!(try_action(&app, edit("tonal_lower", -4.)).is_err(), "Edits from the previous tool are rejected");
        assert_eq!(app.state()["workspace"], before);
    }
}

#[test]
fn apple_tonal_samples_visible_artwork_and_refines_inside_quick_mask() {
    for platform in [0, 1] {
        let app = tonal_app(platform);
        app.invoke("tonal_select");
        preset(&app, 4);
        settle(&app, "highlights mask", || selection_bounds(&app).is_some());
        app.invoke("quick_mask");
        let highlights = selection_bounds(&app);
        setting(&app, "tonal_softness", 0.25);
        app.draw_until_idle();
        assert_eq!(app.state()["layer_tools"]["quick_mask"], true, "Refining stays in Quick Mask");
        assert!(selection_bounds(&app).is_some());
        let [x, y] = surface(&app, [16., 16.]);
        let records = [x, y, 1., 0., 0., 0., 0., 2_000_000_000., 1., x, y, 1., 0., 0., 0., 0., 2_010_000_000., 3.];
        assert_eq!(unsafe { capy_apple_pointer(app.0, 40, 1, 0, records.as_ptr(), records.len(), 0, capy_apple_camera_revision(app.0)) }, 0);
        settle(&app, "point sample", || app.state()["tool_extra"][0]["Choice"]["items"].as_array().unwrap().last().unwrap()["selected"] == true);
        let [lower, upper] = bounds(&app);
        assert!(lower > -6. && upper < -3. && (upper - lower - 1.).abs() < 0.01,
            "A point sample reads the dark artwork one stop wide, not the Quick Mask overlay: {lower} {upper}");
        assert_ne!(selection_bounds(&app), highlights);
    }
}
