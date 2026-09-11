use super::*;

fn snapshot(app: &App) -> Value {
    // Read the next bridge publication after a state change.
    let text = unsafe { capy_apple_request(app.0, 3, std::ptr::null()) };
    assert!(!text.is_null());
    let value = serde_json::from_slice(unsafe { CStr::from_ptr(text) }.to_bytes()).unwrap();
    unsafe { capy_apple_string_free(text) };
    value
}
fn customize(app: &App, action: Value) {
    app.action(json!({"type":"customize","action":action}));
}
fn config(app: &App, id: &str) -> Value {
    app.state()["workspace"]["layout"]["panels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id)
        .unwrap()
        .clone()
}
fn toolbar_named(app: &App, name: &str) -> Value {
    app.state()["workspace"]["layout"]["panels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["content"]["name"] == name)
        .expect("Created toolbar is in the registry")
        .clone()
}
fn menu_action(menu: &Value, operation: &str) -> Value {
    menu["sections"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s.as_array().unwrap())
        .find(|i| i["action"]["action"]["type"] == operation)
        .unwrap()["action"]
        .clone()
}

#[test]
fn toolbar_picker_naming_duplication_manager_and_history_preserve_artwork() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().unwrap());
        app.draw_frame();
        app.stroke();
        app.draw_frame();
        let pixels = app.pixels();
        app.invoke("new_toolbar");
        let picker = snapshot(&app)["picker"].clone();
        assert_eq!(picker["can_confirm"], false);
        for control in [
            json!({"kind":"command","command":"undo"}),
            json!({"kind":"divider"}),
            json!({"kind":"command","command":"redo"}),
        ] {
            assert!(
                picker["choices"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["control"] == control)
            );
            customize(
                &app,
                json!({"type":"picker_select","control":control,"selected":true}),
            );
        }
        customize(&app, json!({"type":"picker_name","name":""}));
        assert_eq!(snapshot(&app)["picker"]["can_confirm"], false);
        customize(&app, json!({"type":"picker_name","name":"Quick tools"}));
        customize(&app, json!({"type":"confirm_tools"}));
        let original = toolbar_named(&app, "Quick tools");
        let original_id = original["id"].as_str().unwrap();
        assert_eq!(original["content"]["tiles"].as_array().unwrap().len(), 3);
        let menu = app
            .request(
                2,
                json!({"type":"context","target":{"kind":"ribbon","panel":original_id}}),
            )
            .unwrap();
        app.action(menu_action(&menu, "rename_toolbar"));
        customize(&app, json!({"type":"toolbar_name","name":"Renamed tools"}));
        customize(&app, json!({"type":"confirm_toolbar"}));
        assert_eq!(
            config(&app, original_id)["content"]["name"],
            "Renamed tools"
        );
        app.action(menu_action(&menu, "duplicate_toolbar"));
        customize(&app, json!({"type":"toolbar_name","name":"Renamed tools"}));
        assert_eq!(snapshot(&app)["toolbar_prompt"]["can_confirm"], false);
        customize(&app, json!({"type":"toolbar_name","name":"Copy tools"}));
        customize(&app, json!({"type":"confirm_toolbar"}));
        let copy = toolbar_named(&app, "Copy tools");
        let copy_id = copy["id"].as_str().unwrap();
        let controls = |p: &Value| {
            p["content"]["tiles"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["control"].clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(controls(&copy), controls(&original));
        app.invoke("manage_toolbars");
        customize(
            &app,
            json!({"type":"select_managed_toolbar","panel":copy_id}),
        );
        let delete = snapshot(&app)["toolbar_manager"]["delete_action"].clone();
        customize(&app, delete.clone());
        customize(&app, json!({"type":"cancel_toolbar"}));
        assert_eq!(config(&app, copy_id), copy);
        customize(&app, delete);
        customize(&app, json!({"type":"confirm_toolbar"}));
        customize(&app, json!({"type":"close_toolbar_manager"}));
        assert!(
            !app.state()["workspace"]["layout"]["panels"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["id"] == copy_id)
        );
        app.invoke("undo_workspace");
        assert_eq!(config(&app, copy_id), copy);
        app.draw_frame();
        assert_eq!(app.pixels(), pixels);
    }
}

#[test]
fn panel_drag_and_resize_use_cancelable_shared_history_without_changing_pixels() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().unwrap());
        app.draw_frame();
        app.stroke();
        app.draw_frame();
        let pixels = app.pixels();
        let baseline = app.state()["workspace"].clone();
        let group = snapshot(&app)["layout"]["groups"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["active"] == "sizes")
            .unwrap()
            .clone();
        let press = [
            group["bounds"]["x"].as_f64().unwrap() + 50.,
            group["bounds"]["y"].as_f64().unwrap() + 15.,
        ];
        let drag = |phase, point| {
            app.action(json!({"type":"drag_workspace","item":{"kind":"panel","panel":"sizes"},"phase":phase,"position":point,"viewport":[1200,900],"tabs":[]}))
        };
        drag("down", press);
        drag("move", [650., 450.]);
        assert_eq!(
            app.state()["workspace"]["layout"]["floating"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        drag("cancel", [650., 450.]);
        assert_eq!(app.state()["workspace"], baseline);
        drag("down", press);
        drag("move", [650., 450.]);
        drag("up", [650., 450.]);
        let floated = app.state()["workspace"].clone();
        let group = snapshot(&app)["layout"]["groups"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["active"] == "sizes")
            .unwrap()
            .clone();
        let end = [
            group["bounds"]["x"].as_f64().unwrap() + group["bounds"]["width"].as_f64().unwrap(),
            group["bounds"]["y"].as_f64().unwrap() + group["bounds"]["height"].as_f64().unwrap(),
        ];
        let resize = |phase, point| {
            app.action(json!({"type":"resize_floating","group":group["id"],"edge":"bottom_right","phase":phase,"position":point,"viewport":[1200,900]}))
        };
        resize("down", end);
        resize("move", [end[0] + 60., end[1] + 40.]);
        resize("up", [end[0] + 60., end[1] + 40.]);
        assert_ne!(app.state()["workspace"], floated);
        app.invoke("undo_workspace");
        assert_eq!(app.state()["workspace"], floated);
        app.invoke("undo_workspace");
        assert_eq!(app.state()["workspace"], baseline);
        app.draw_frame();
        assert_eq!(app.pixels(), pixels);
    }
}

#[test]
fn expanded_toolbar_geometry_and_final_drop_action_match_the_shared_preview() {
    for platform in [0, 1] {
        let app = App::new(platform);
        customize(&app, json!({"type":"show_all_controls","panel":"toolbar"}));
        let baseline = app.state()["workspace"].clone();
        let expansion = app
            .request(
                2,
                json!({"type":"expansion","panel":"toolbar","heights":[0,500],"progress":1}),
            )
            .unwrap();
        assert_eq!(app.state()["workspace"], baseline);
        let layout = unsafe { &*app.0 }.host.session.layout([1200., 900.]);
        let group = layout
            .groups
            .iter()
            .find(|g| g.active == layer_ui::Panel::Toolbar)
            .unwrap();
        let p = expansion["preview"].clone();
        let config = unsafe { &*app.0 }
            .host
            .session
            .state()
            .workspace
            .layout
            .panel(layer_ui::Panel::Toolbar)
            .unwrap();
        let expected = layer_ui::toolbar_tile_layout(
            p["width"].as_f64().unwrap() as f32,
            (p["height"].as_f64().unwrap() - expansion["configuration"]["y"].as_f64().unwrap())
                as f32,
            group.axis,
            config.tiles(),
            !group.tabs_visible,
            config.tile_style,
        );
        assert_eq!(expansion["tiles"], serde_json::to_value(expected).unwrap());
        let tile = &expansion["tiles"]["tiles"][3];
        let point = [
            expansion["bounds"]["x"].as_f64().unwrap()
                + p["x"].as_f64().unwrap()
                + tile["x"].as_f64().unwrap()
                + 4.,
            expansion["bounds"]["y"].as_f64().unwrap()
                + tile["y"].as_f64().unwrap()
                + tile["height"].as_f64().unwrap() * 0.5,
        ];
        let hint=app.request(2,json!({"type":"drop","item":{"kind":"tile","panel":"toolbar","tile":1},"position":point,"tabs":[],"expansion":expansion})).unwrap();
        assert_eq!(hint["action"]["type"], "move_tile");
        assert_eq!(hint["action"]["target"], hint["target"]);
        app.action(hint["action"].clone());
        assert_ne!(app.state()["workspace"], baseline);
        app.invoke("undo_workspace");
        assert_eq!(app.state()["workspace"], baseline);
    }
}

#[test]
fn apple_drawer_dismissal_consumes_the_entire_canvas_contact_then_allows_painting() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let scale = if platform == 0 { 2. } else { 1. };
        assert_eq!(
            unsafe {
                capy_apple_resize(
                    app.0,
                    (1200. * scale) as u32,
                    (900. * scale) as u32,
                    scale as f32,
                )
            },
            0
        );
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(layer_render_wgpu::WgpuRasterizer::new_headless().unwrap());
        app.draw_frame();
        let paper = app.pixels();
        app.action(json!({"type":"activate_tile","panel":"toolbar","tile":1}));
        if app.state()["customization"]["drawer"].is_null() {
            app.action(json!({"type":"activate_tile","panel":"toolbar","tile":1}));
        }
        assert!(app.state()["customization"]["drawer"].is_object());
        let geometry = app
            .request(
                2,
                json!({"type":"drawer","column":null,"heights":[400,250],"progress":1}),
            )
            .unwrap();
        let facts = json!({"held":false,"dragging":false,"popup_open":false,"content_drawer":geometry["placement"]["bounds"],"drawer_connection":geometry["connection"]["bounds"]});
        app.request(
            1,
            json!({"type":"chrome","event":{"kind":"refresh"},"facts":facts,"viewport":[1200,900]}),
        )
        .unwrap();
        let revision = app.state()["camera"]["revision"].as_u64().unwrap();
        // Separate ABI batches, including visual prediction, cannot leak a move
        // after the initial down was used to dismiss the native drawer.
        for (phase, predicted) in [(1., 0), (2., 1), (2., 0), (3., 0)] {
            let record = [
                800. * scale,
                650. * scale,
                1.,
                0.,
                0.,
                0.,
                0.,
                2_000_000_000. + phase * 10_000_000.,
                phase,
            ];
            assert_eq!(
                unsafe {
                    capy_apple_pointer(
                        app.0,
                        77,
                        1,
                        0,
                        record.as_ptr(),
                        record.len(),
                        predicted,
                        revision,
                    )
                },
                0
            );
        }
        assert!(app.state()["customization"]["drawer"].is_null());
        app.draw_frame();
        assert_eq!(app.pixels(), paper);
        assert!(unsafe { &*app.0 }.dismissed_contacts.is_empty());
        assert_eq!(unsafe { capy_apple_resize(app.0, 1200, 900, 1.) }, 0);
        app.stroke();
        app.draw_frame();
        assert_ne!(app.pixels(), paper);
        app.invoke("undo");
        app.draw_frame();
        assert_eq!(app.pixels(), paper);
    }
}

#[test]
fn apple_collapsed_toolbar_child_drawers_follow_live_tiles_and_preserve_topology() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let group = unsafe { &*app.0 }
            .host
            .session
            .state()
            .workspace
            .layout
            .panel_group(layer_ui::Panel::Brushes)
            .unwrap();
        app.action(json!({"type":"move_panel","panel":"toolbar","target":{"kind":"tab","group":group},"viewport":[1200,900]}));
        let baseline = app.state()["workspace"].clone();
        customize(
            &app,
            json!({"type":"set_column_collapsed","group":group,"collapsed":true}),
        );
        customize(
            &app,
            json!({"type":"toggle_column_drawer","group":group,"panel":"toolbar"}),
        );
        let column = unsafe { &*app.0 }
            .host
            .session
            .state()
            .workspace
            .layout
            .collapsed_column_for_group(group)
            .unwrap();
        let root = snapshot(&app);
        assert!(
            root["layout"]["collapsed"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["id"] == column)
        );
        assert_eq!(
            root["state"]["customization"]["column_drawers"][0]["tabs"]["active"],
            "toolbar"
        );
        let collapsed = app.state()["workspace"].clone();
        let geometry = app
            .request(
                2,
                json!({"type":"drawer_toolbar","panel":"toolbar","width":272,"height":800}),
            )
            .unwrap();
        assert!(geometry["content_height"].as_f64().unwrap() > 0.);
        let measure = |y, height| {
            app.action(json!({"type":"measure_drawer_tiles","measurements":[{"column":column,"anchor":{"panel":"toolbar","tile":1},"bounds":{"x":80.,"y":y,"width":36.,"height":height}}]}))
        };
        measure(100., 36.);
        app.action(json!({"type":"activate_tile","panel":"toolbar","tile":1}));
        if app.state()["customization"]["drawer"].is_null() {
            app.action(json!({"type":"activate_tile","panel":"toolbar","tile":1}));
        }
        let query = json!({"type":"drawer","column":null,"heights":[700,250],"progress":1});
        let first = app.request(2, query.clone()).unwrap();
        assert_eq!(first["placement"]["anchor"]["y"], 100.);
        measure(60., 20.);
        let next = app.request(2, query.clone()).unwrap();
        assert_eq!(next["placement"]["anchor"]["height"], 20.);
        assert!(next["connection"].is_object());
        app.action(json!({"type":"measure_drawer_tiles","measurements":[]}));
        assert!(app.request(2, query).unwrap().is_null());
        assert_eq!(
            app.state()["workspace"],
            collapsed,
            "Drawer scrolling and geometry are transient"
        );
        customize(&app, json!({"type":"close_expanded"}));
        customize(
            &app,
            json!({"type":"toggle_column_drawer","group":group,"panel":"toolbar"}),
        );
        app.invoke("undo_workspace");
        assert_eq!(
            app.state()["workspace"],
            baseline,
            "Only the collapse changed durable topology"
        );
        app.invoke("undo_workspace"); // Restore the outward-facing lone toolbar.
        let zen_layout = app.state()["workspace"].clone();
        app.invoke("zen_mode");
        let zen = snapshot(&app);
        assert_eq!(zen["partial_zen"], true);
        assert!(
            !zen["zen_toolbars"]["sections"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(app.state()["workspace"]["layout"], zen_layout["layout"]);
        app.invoke("zen_mode");
        app.invoke("new_toolbar");
        assert!(
            snapshot(&app)["picker"]["choices"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["control"] == json!({"kind":"panel","panel":"navigator"}))
        );
    }
}
