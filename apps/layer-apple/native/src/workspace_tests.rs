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
fn apple_tab_preview_uses_frozen_geometry_and_commits_the_same_slot() {
    for platform in [0, 1] {
        for collapsed in [false, true] {
            let app = App::new(platform);
            let group = unsafe { &*app.0 }
                .host
                .session
                .state()
                .workspace
                .layout
                .panel_group(layer_ui::Panel::Brushes)
                .unwrap();
            for panel in ["toolbar", "navigator"] {
                app.action(json!({"type":"move_panel","panel":panel,"target":{"kind":"tab","group":group},"viewport":[1200,900]}));
            }
            let bounds = if collapsed {
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
                let bounds = app
                    .request(
                        2,
                        json!({"type":"drawer","column":column,"heights":[450],"progress":1}),
                    )
                    .unwrap()["placement"]["bounds"]
                    .clone();
                app.action(json!({"type":"measure_column_drawers","measurements":[{"group":group,"bounds":bounds}]}));
                bounds
            } else {
                snapshot(&app)["layout"]["groups"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|g| g["id"] == group)
                    .unwrap()["bounds"]
                    .clone()
            };
            let x = bounds["x"].as_f64().unwrap();
            let y = bounds["y"].as_f64().unwrap();
            let tabs = json!([
                {"group":group,"index":2,"bounds":{"x":x+140.,"y":y,"width":60.,"height":36.}},
                {"group":group,"index":0,"bounds":{"x":x,"y":y,"width":40.,"height":36.}},
                {"group":group,"index":1,"bounds":{"x":x+40.,"y":y,"width":100.,"height":36.}}
            ]);
            let before = app.state()["workspace"].clone();
            let drag = |phase: &str, delta: f64| {
                app.action(json!({"type":"drag_workspace","item":{"kind":"panel","panel":"toolbar"},"phase":phase,"position":[x+41.+delta,y+18.],"viewport":[1200,900],"tabs":tabs}))
            };
            drag("down", 0.);
            app.action(json!({"type":"begin_tab_drag","tabs":tabs,"clip":{"x":x+10.,"y":y,"width":190.,"height":36.}}));
            let preview = |delta: f64| {
                app.request(2, json!({"type":"workspace_drag_preview","item":{"kind":"panel","panel":"toolbar"},"position":[x+41.+delta,y+18.],"tabs":tabs})).unwrap()
            };
            assert_eq!(preview(29.)["tab"]["insertion"], 1);
            let shifted = preview(30.);
            assert_eq!(shifted["tab"]["insertion"], 3);
            assert_eq!(shifted["drop"]["target"]["index"], 3);
            assert_eq!(shifted["tab"]["offsets"][2]["x"], -100.);
            assert_eq!(preview(1000.)["tab"]["bounds"]["x"], x + 100.);
            assert_eq!(preview(29.)["tab"]["insertion"], 1);
            // Commit a newer release point without any preceding move there.
            drag("up", 30.);
            assert!(preview(30.)["tab"].is_null());
            let panels = unsafe { &*app.0 }
                .host
                .session
                .state()
                .workspace
                .layout
                .group_panels(group)
                .unwrap();
            assert_eq!(
                panels,
                &[
                    layer_ui::Panel::Brushes,
                    layer_ui::Panel::Navigator,
                    layer_ui::Panel::Toolbar
                ]
            );
            let after = app.state()["workspace"].clone();
            app.invoke("undo_workspace");
            assert_eq!(app.state()["workspace"], before);
            app.invoke("redo_workspace");
            assert_eq!(app.state()["workspace"], after);
        }
    }
}

#[test]
fn apple_column_drawer_drags_preserve_history_and_accept_measured_drop_targets() {
    for platform in [0, 1] {
        for whole in [false, true] {
            let app = App::new(platform);
            let layout = || {
                unsafe { &*app.0 }
                    .host
                    .session
                    .state()
                    .workspace
                    .layout
                    .clone()
            };
            let group = layout().panel_group(layer_ui::Panel::Brushes).unwrap();
            app.action(json!({"type":"move_panel","panel":"toolbar","target":{"kind":"tab","group":group},"viewport":[1200,900]}));
            customize(
                &app,
                json!({"type":"set_column_collapsed","group":group,"collapsed":true}),
            );
            customize(
                &app,
                json!({"type":"toggle_column_drawer","group":group,"panel":"toolbar"}),
            );
            let column = layout().collapsed_column_for_group(group).unwrap();
            let drawer = app.state()["customization"]["column_drawers"][0].clone();
            let bounds = app
                .request(
                    2,
                    json!({"type":"drawer","column":column,"heights":[450],"progress":1}),
                )
                .unwrap()["placement"]["bounds"]
                .clone();
            let measure = || {
                app.action(json!({"type":"measure_column_drawers","measurements":[{"group":group,"bounds":bounds}]}))
            };
            measure();
            // Native tab preferences arrive in dictionary order, including the
            // destination's later tab before its first tab.
            let left = bounds["x"].as_f64().unwrap();
            let top = bounds["y"].as_f64().unwrap();
            let tabs = json!([
                {"group":group,"index":1,"bounds":{"x":left+96.,"y":top,"width":76.5,"height":36.}},
                {"group":group,"index":0,"bounds":{"x":left,"y":top,"width":96.,"height":36.}}
            ]);
            for (phase, point) in [
                ("down", [left + 134.25, top + 18.]),
                ("move", [left + 12., top + 18.]),
                ("up", [left + 12., top + 18.]),
            ] {
                app.action(json!({"type":"drag_workspace","item":{"kind":"panel","panel":"toolbar"},"phase":phase,"position":point,"viewport":[1200,900],"tabs":tabs}));
            }
            assert_eq!(
                app.state()["customization"]["column_drawers"][0]["tabs"]["panels"],
                json!(["toolbar", "brushes"])
            );
            app.invoke("undo_workspace");
            customize(
                &app,
                json!({"type":"toggle_column_drawer","group":group,"panel":"toolbar"}),
            );
            measure();
            let baseline = app.state();
            assert!(
                baseline["customization"]
                    .get("column_drawer_bounds")
                    .is_none()
            );
            let press = [
                bounds["x"].as_f64().unwrap() + 20.,
                bounds["y"].as_f64().unwrap() + 18.,
            ];
            let away = [650., 450.];
            let item = if whole {
                json!({"kind":"group","group":group})
            } else {
                json!({"kind":"panel","panel":"toolbar"})
            };
            let drag = |phase, position| {
                app.action(json!({"type":"drag_workspace","item":item,"phase":phase,"position":position,"viewport":[1200,900],"tabs":[]}))
            };
            drag("down", press);
            drag("move", [press[0] + 12., press[1]]);
            assert_eq!(app.state()["workspace"], baseline["workspace"]);
            drag("move", away);
            assert_eq!(layout().floating.len(), 1);
            drag("cancel", away);
            assert_eq!(app.state()["workspace"], baseline["workspace"]);
            assert_eq!(app.state()["customization"]["column_drawers"][0], drawer);
            measure();
            drag("down", press);
            drag("move", away);
            drag("up", away);
            let floated = app.state()["workspace"].clone();
            assert_eq!(layout().floating.len(), 1);
            let floating_group = layout().panel_group(layer_ui::Panel::Toolbar).unwrap();
            assert_eq!(
                layout()
                    .group_panels(floating_group)
                    .unwrap()
                    .contains(&layer_ui::Panel::Brushes),
                whole
            );
            app.invoke("undo_workspace");
            assert_eq!(app.state()["workspace"], baseline["workspace"]);
            app.invoke("redo_workspace");
            assert_eq!(app.state()["workspace"], floated);

            // Open a second collapsed group and dock the floating source into
            // its measured tab bar through the same drop preview used by Swift.
            let target = layout().panel_group(layer_ui::Panel::Layers).unwrap();
            customize(
                &app,
                json!({"type":"set_column_collapsed","group":target,"collapsed":true}),
            );
            customize(
                &app,
                json!({"type":"toggle_column_drawer","group":target,"panel":"layers"}),
            );
            let target_column = layout().collapsed_column_for_group(target).unwrap();
            let target_bounds = app
                .request(
                    2,
                    json!({"type":"drawer","column":target_column,"heights":[450],"progress":1}),
                )
                .unwrap()["placement"]["bounds"]
                .clone();
            app.action(json!({"type":"measure_column_drawers","measurements":[{"group":target,"bounds":target_bounds}]}));
            let destination = [
                target_bounds["x"].as_f64().unwrap()
                    + target_bounds["width"].as_f64().unwrap() * 0.5,
                target_bounds["y"].as_f64().unwrap() + 18.,
            ];
            let moving = json!({"kind":"group","group":floating_group});
            let before_dock = app.state()["workspace"].clone();
            app.action(json!({"type":"drag_workspace","item":moving,"phase":"down","position":away,"viewport":[1200,900],"tabs":[]}));
            let hint = app
                .request(
                    2,
                    json!({"type":"drop","item":moving,"position":destination,"tabs":[]}),
                )
                .unwrap();
            assert_eq!(hint["target"]["kind"], "tab");
            assert_eq!(hint["target"]["group"], target);
            for phase in ["move", "up"] {
                app.action(json!({"type":"drag_workspace","item":moving,"phase":phase,"position":destination,"viewport":[1200,900],"tabs":[]}));
            }
            assert!(layout().floating.is_empty());
            assert_eq!(layout().panel_group(layer_ui::Panel::Toolbar), Some(target));
            let docked = app.state()["workspace"].clone();
            app.invoke("undo_workspace");
            assert_eq!(app.state()["workspace"], before_dock);
            app.invoke("redo_workspace");
            assert_eq!(app.state()["workspace"], docked);
            assert_eq!(app.state()["brush"], baseline["brush"]);
            assert_eq!(app.state()["layers"], baseline["layers"]);
            layout().validate().unwrap();
        }
    }
}

#[test]
fn apple_toolbar_styles_reach_ribbons_drawers_and_zen_with_shared_metrics() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let before = app.state();
        for (style, size, icon, lines, bold) in [
            ("medium", [54., 54.], 24, 0, false),
            ("large", [72., 72.], 32, 0, false),
            ("medium_labeled", [108., 54.], 16, 2, false),
            ("labeled", [108., 72.], 16, 3, true),
            ("small", [36., 36.], 16, 0, false),
        ] {
            let previous = config(&app, "commands")["tile_style"].clone();
            let menu = app
                .request(
                    2,
                    json!({"type":"context","target":{"kind":"ribbon","panel":"commands"}}),
                )
                .unwrap();
            let action = menu["sections"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|s| s.as_array().unwrap())
                .find(|item| item["action"]["action"]["style"] == style)
                .unwrap()["action"]
                .clone();
            app.action(action);
            let view = snapshot(&app);
            let panel = view["panels"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["id"] == "commands")
                .unwrap();
            assert_eq!(panel["tile_icon_size"], icon);
            assert_eq!(panel["tile_label_lines"], lines);
            assert_eq!(panel["tile_label_bold"], bold);
            let ribbon = view["layout"]["groups"]
                .as_array()
                .unwrap()
                .iter()
                .find(|g| g["active"] == "commands")
                .unwrap();
            let drawer = app
                .request(
                    2,
                    json!({"type":"drawer_toolbar","panel":"commands","width":232,"height":800}),
                )
                .unwrap();
            for geometry in [&ribbon["tiles"], &drawer] {
                assert_eq!(
                    geometry["tiles"].as_array().unwrap().len(),
                    panel["tiles"].as_array().unwrap().len()
                );
                for (bounds, tile) in geometry["tiles"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(panel["tiles"].as_array().unwrap())
                {
                    if tile["control"]["kind"] != "divider" {
                        assert_eq!(bounds["width"].as_f64().unwrap(), size[0]);
                        assert_eq!(bounds["height"].as_f64().unwrap(), size[1]);
                    }
                }
            }
            app.invoke("undo_workspace");
            assert_eq!(config(&app, "commands")["tile_style"], previous);
            app.invoke("redo_workspace");
            assert_eq!(config(&app, "commands")["tile_style"], style);
            app.invoke("zen_mode");
            let zen = snapshot(&app);
            let sections: Vec<_> = zen["zen_toolbars"]["sections"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|s| s["panel"] == "commands")
                .collect();
            assert!(!sections.is_empty());
            for section in sections {
                assert_eq!(section["style"], style);
            }
            app.invoke("zen_mode");
            assert_eq!(app.state()["brush"], before["brush"]);
            assert_eq!(app.state()["layers"], before["layers"]);
        }
    }
}

#[test]
fn apple_default_workspace_reaches_every_grouped_brush_without_changing_artwork() {
    use std::collections::BTreeSet;
    for platform in [0, 1] {
        let app = App::new(platform);
        assert_eq!(
            app.state()["workspace"]["layout"],
            serde_json::to_value(layer_ui::DockLayout::for_platform(layer_ui::Platform::Web))
                .unwrap(),
            "Fresh Apple editors use the same workspace as the web editor"
        );
        let catalog = app.request(2, json!({"type":"catalog"})).unwrap();
        let expected: BTreeSet<_> = catalog["brush_categories"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|category| category["brushes"].as_array().unwrap())
            .map(|brush| brush["id"].as_u64().unwrap())
            .collect();
        let mut reachable = BTreeSet::new();
        let color = app.state()["brush"]["color"].clone();
        let layers = app.state()["layers"].clone();
        for tool in layer_ui::Tool::ALL {
            let command = serde_json::to_value(tool.command()).unwrap();
            let panel = config(&app, "toolbar");
            let tile = panel["content"]["tiles"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tile| tile["control"]["command"] == command)
                .unwrap();
            app.action(json!({"type":"activate_tile","panel":"toolbar","tile":tile["id"]}));
            let groups = app.state()["tool_set"]["groups"]
                .as_array()
                .unwrap()
                .clone();
            for group in groups {
                app.action(group["action"].clone());
                let subtools = app.state()["tool_set"]["subtools"]
                    .as_array()
                    .unwrap()
                    .clone();
                assert!(!subtools.is_empty());
                for subtool in subtools {
                    app.action(subtool["action"].clone());
                    let id = subtool["preview"].as_u64().unwrap();
                    assert_eq!(app.state()["brush"]["preset"], id);
                    assert_eq!(app.state()["brush"]["color"], color);
                    assert_eq!(app.state()["layers"], layers);
                    reachable.insert(id);
                }
            }
        }
        assert_eq!(
            reachable, expected,
            "Grouped controls must retain every catalog brush"
        );
        let mut saved = layer_ui::WorkspaceState::default();
        saved.layout.bands[0].extent = 275.;
        let saved = serde_json::to_value(saved).unwrap();
        app.action(json!({"type":"restore_workspace","workspace":saved}));
        assert_eq!(
            app.state()["workspace"],
            saved,
            "Restoring an older customized workspace must not replace it with the new preset"
        );
    }
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
        let sizes_group = unsafe { &*app.0 }
            .host
            .session
            .state()
            .workspace
            .layout
            .panel_group(layer_ui::Panel::Sizes)
            .unwrap();
        app.action(json!({"type":"select_panel_tab","group":sizes_group,"panel":"sizes"}));
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
