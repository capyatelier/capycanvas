use super::*;

struct Published(std::cell::RefCell<Value>);
impl Published {
    fn new() -> Self {
        Self(std::cell::RefCell::new(Value::Null))
    }
    fn view(&self, app: &App) -> Value {
        let text = unsafe { capy_apple_request(app.0, 3, std::ptr::null()) };
        if !text.is_null() {
            *self.0.borrow_mut() = serde_json::from_slice(unsafe { CStr::from_ptr(text) }.to_bytes()).unwrap();
            unsafe { capy_apple_string_free(text) };
        }
        let view = self.0.borrow().clone();
        assert!(view.is_object(), "a snapshot was published");
        view
    }
    fn tile(&self, app: &App, kind: &str) -> (Value, Value) {
        self.view(app)["panels"].as_array().unwrap().iter().find_map(|panel| {
            panel["tiles"].as_array()?.iter()
                .find(|tile| tile["control"]["kind"] == kind)
                .map(|tile| (panel.clone(), tile.clone()))
        }).unwrap_or_else(|| panic!("{kind} is published"))
    }
}

fn restore(app: &App, preset: layer_ui::WorkspacePreset) {
    let policy = unsafe { &*app.0 }.host.session.state().platform;
    app.action(json!({"type":"restore_workspace","workspace":layer_ui::WorkspaceState {
        layout: preset.layout(policy), ..Default::default()
    }}));
}

fn try_action(app: &App, action: Value) -> Result<(), String> {
    let text = CString::new(action.to_string()).unwrap();
    let result = unsafe { capy_apple_request(app.0, 0, text.as_ptr()) };
    if !result.is_null() {
        unsafe { capy_apple_string_free(result) };
    }
    let error = unsafe { capy_apple_error(app.0) };
    if error.is_null() {
        Ok(())
    } else {
        Err(unsafe { CStr::from_ptr(error) }.to_string_lossy().into_owned())
    }
}

fn stateless(request: &str) -> Value {
    let source = CString::new(request).unwrap();
    let output = unsafe { capy_apple_toolbar_ui(source.as_ptr()) };
    assert!(!output.is_null());
    let value = serde_json::from_slice(unsafe { CStr::from_ptr(output) }.to_bytes()).unwrap();
    unsafe { capy_apple_string_free(output) };
    value
}

#[test]
fn apple_toolbar_queries_match_the_shared_transport_without_a_session() {
    for request in [
        json!({"type":"slider_layout","width":44,"height":176,"axis":"vertical"}),
        json!({"type":"slider_spec","control":{"kind":"brush_size_slider"}}),
        json!({"type":"style","style":"medium"}),
        json!({"type":"slider_preview","control":{"kind":"brush_opacity_slider"},"style":"medium","value":0.5,"length":176,"extent":64}),
    ] {
        let expected = layer_ui::toolbar_ui(serde_json::from_value(request.clone()).unwrap()).unwrap();
        assert_eq!(stateless(&request.to_string()), expected, "{request}");
    }
    assert!(stateless("{").get("error").is_some());
    assert!(stateless(r#"{"type":"slider_spec","control":{"kind":"color"}}"#).get("error").is_some());
    let output = unsafe { capy_apple_toolbar_ui(std::ptr::null()) };
    let missing: Value = serde_json::from_slice(unsafe { CStr::from_ptr(output) }.to_bytes()).unwrap();
    unsafe { capy_apple_string_free(output) };
    assert_eq!(missing["error"], "Missing toolbar request");
}

#[test]
fn apple_sketch_sliders_edit_bookmark_and_preview_with_their_original_context() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let published = Published::new();
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
        app.draw_until_idle();
        restore(&app, layer_ui::WorkspacePreset::Painter);
        app.invoke("brush");
        let (_, size) = published.tile(&app, "brush_size_slider");
        let (_, opacity) = published.tile(&app, "brush_opacity_slider");
        assert_eq!(size["component"]["numeric"]["id"], "size");
        assert_eq!(opacity["component"]["numeric"]["id"], "opacity");
        let context = size["component"]["context"].clone();
        let edit = |action: Value| json!({"type":"toolbar_edit","context":context,"action":action});
        app.action(edit(json!({"type":"set_tool_setting","id":"size","value":37})));
        assert_eq!(published.tile(&app, "brush_size_slider").1["component"]["numeric"]["value"], 37.0);

        app.action(edit(json!({"type":"toggle_slider_bookmark","control":{"kind":"brush_size_slider"}})));
        let marks = published.tile(&app, "brush_size_slider").1["component"]["bookmarks"].clone();
        assert!(marks.as_array().unwrap().iter().any(|m| m["selected"] == true && m["value"] == 37.0), "{marks}");
        app.action(edit(json!({"type":"toggle_slider_bookmark","control":{"kind":"brush_size_slider"}})));
        assert!(published.tile(&app, "brush_size_slider").1["component"]["bookmarks"].as_array().unwrap().is_empty());

        let stamp = app.request(2, json!({"type":"toolbar_stamp","context":context})).unwrap();
        let side = stamp["size"].as_u64().unwrap() as usize;
        let alpha = stamp["alpha"].as_array().unwrap();
        assert!(side > 0 && alpha.len() == side * side && stamp["extent"].as_f64().unwrap() > 0.);
        assert!(alpha.iter().any(|a| a.as_u64().unwrap() > 0));

        app.invoke("eraser");
        let fresh = published.tile(&app, "brush_size_slider").1["component"]["context"].clone();
        assert_ne!(fresh, context);
        let before = app.state()["workspace"].clone();
        assert!(try_action(&app, edit(json!({"type":"set_tool_setting","id":"size","value":12}))).is_err());
        assert_eq!(app.state()["workspace"], before);
        assert_ne!(published.tile(&app, "brush_size_slider").1["component"]["numeric"]["value"], 12.0);
    }
}

#[test]
fn apple_photo_tool_options_follow_the_active_tool_and_open_its_drawer() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let published = Published::new();
        restore(&app, layer_ui::WorkspacePreset::Photographer);
        app.invoke("lasso");
        let (panel, tile) = published.tile(&app, "tool_options");
        let options = tile["component"]["options"].as_array().unwrap().clone();
        let choice = options.iter().find_map(|o| o.get("Choice")).expect("lasso publishes a choice");
        let items = choice["items"].as_array().unwrap();
        let unselected = items.iter().find(|i| i["selected"] != true).unwrap();
        app.action(json!({"type":"toolbar_edit","context":tile["component"]["context"],"action":unselected["action"]}));
        let (_, next) = published.tile(&app, "tool_options");
        let updated = next["component"]["options"].as_array().unwrap().iter()
            .find_map(|o| o.get("Choice").filter(|c| c["id"] == choice["id"])).unwrap().clone();
        assert!(updated["items"].as_array().unwrap().iter().any(|i| i["selected"] == true && i["label"] == unselected["label"]));
        app.action(json!({"type":"activate_tile","panel":panel["id"],"tile":tile["id"]}));
        let drawer = &app.state()["customization"]["drawer"];
        assert_eq!(drawer["anchor"]["kind"], "tile", "{drawer}");
        assert_eq!(drawer["anchor"]["tile"], tile["id"]);
    }
}

#[test]
fn apple_sketch_brush_toolbar_docks_to_compact_edges_with_one_history_step() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let published = Published::new();
        restore(&app, layer_ui::WorkspacePreset::Painter);
        let (panel, _) = published.tile(&app, "brush_size_slider");
        let id = panel["id"].clone();
        fn holds(node: &layer_ui::DockNode, panel: layer_ui::Panel) -> bool {
            match node {
                layer_ui::DockNode::Tabs { panels, .. } => panels.contains(&panel),
                layer_ui::DockNode::Split { first, second, .. } => holds(first, panel) || holds(second, panel),
            }
        }
        let band = || {
            let layout = unsafe { &*app.0 }.host.session.state().workspace.layout.clone();
            let panel: layer_ui::Panel = serde_json::from_value(id.clone()).unwrap();
            let band = layout.bands.iter().find(|b| holds(&b.root, panel)).unwrap();
            (band.edge, band.alignment)
        };
        assert_eq!(band(), (layer_ui::Edge::Left, Some(layer_ui::EdgeAlignment::Center)));
        let view = published.view(&app);
        let bounds = &view["layout"]["groups"].as_array().unwrap().iter()
            .find(|g| g["panels"].as_array().unwrap().contains(&id)).unwrap()["bounds"];
        let start = [bounds["x"].as_f64().unwrap() + bounds["width"].as_f64().unwrap() / 2., bounds["y"].as_f64().unwrap() + 4.];
        let before = app.state()["workspace"].clone();
        let drag = |phase: &str, point: [f64; 2]| app.action(json!({"type":"drag_workspace",
            "item":{"kind":"panel","panel":id},"phase":phase,"position":point,"viewport":[1200,900],"tabs":[]}));
        drag("down", start);
        for step in 1..=24 {
            let t = step as f64 / 24.;
            drag("move", [start[0] + (1190. - start[0]) * t, start[1] + (450. - start[1]) * t]);
        }
        drag("up", [1190., 450.]);
        assert_eq!(band(), (layer_ui::Edge::Right, Some(layer_ui::EdgeAlignment::Center)));
        let docked = app.state()["workspace"].clone();
        app.invoke("undo_workspace");
        assert_eq!(app.state()["workspace"], before);
        app.invoke("redo_workspace");
        assert_eq!(app.state()["workspace"], docked);
    }
}
