use super::*;

fn customize(app: &App, action: Value) {
    app.action(json!({"type":"customize","action":action}));
}

// Drawer workflows opt into drawers; ordinary collapsed columns open whole members.
fn enable_column_drawers(app: &App, panel: layer_ui::Panel) {
    let layout = &unsafe { &*app.0 }.host.session.state().workspace.layout;
    let group = layout.panel_group(panel).unwrap();
    let column = layout.column_for_group(group).unwrap();
    customize(app, json!({"type":"set_column_drawers","column":column,"drawers":true}));
}

#[test]
fn apple_stack_auto_hide_consumes_native_contact_before_the_next_contact_paints() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
            Some(native_renderer());
        let group = unsafe { &*app.0 }.host.session.state().workspace.layout
            .panel_group(layer_ui::Panel::Brushes).unwrap();
        customize(&app, json!({"type":"set_column_collapsed","group":group,"collapsed":true}));
        let column = unsafe { &*app.0 }.host.session.state().workspace.layout.collapsed_column_for_group(group).unwrap();
        customize(&app, json!({"type":"set_column_drawers","column":column,"drawers":false}));
        customize(&app, json!({"type":"set_column_auto_hide","column":column,"auto_hide":true}));
        customize(&app, json!({"type":"toggle_column_drawer","group":group,"panel":"brushes"}));
        app.draw_until_idle();
        let initial = app.pixels();
        for facts in [
            layer_ui::ChromeFacts { popup_open: true, ..Default::default() },
            layer_ui::ChromeFacts { content_drawer: Some(layer_ui::Bounds { x:850., y:650., width:100., height:100. }), ..Default::default() },
        ] {
            app.request(1, json!(layer_ui::UiInput::Chrome {
                event: layer_ui::ChromeEvent::Contact { position:[900.,700.], canvas:true }, facts, viewport:[1200.,900.],
            }));
            assert!(unsafe { &*app.0 }.host.session.state().workspace.layout.column_stack(column).open_column.is_some());
        }
        app.request(1, json!(layer_ui::UiInput::Chrome {
            event: layer_ui::ChromeEvent::Refresh, facts:Default::default(), viewport:[1200.,900.],
        }));
        // The gesture starts on the canvas outside the expanded member. Its
        // complete native down/move/up sequence belongs to auto-hide dismissal.
        let stroke = || {
            let session = &unsafe { &*app.0 }.host.session;
            let [a,b,c,d,tx,ty] = session.state().camera.document_to_surface();
            let doc = session.engine().document();
            let x = f64::from(tx + (a * doc.width as f32 + c * doc.height as f32) * 0.5);
            let y = f64::from(ty + (b * doc.width as f32 + d * doc.height as f32) * 0.5);
            let records = [x,y,1.,0.,0.,0.,0.,1_000_000_000.,1.,
                x+10.,y+10.,1.,0.,0.,0.,0.,1_010_000_000.,2.,
                x+20.,y+20.,1.,0.,0.,0.,0.,1_020_000_000.,3.];
            assert_eq!(unsafe { capy_apple_pointer(app.0, 1, 1, 0, records.as_ptr(), records.len(), 0, capy_apple_camera_revision(app.0)) }, 0);
            app.draw_until_idle();
        };
        stroke();
        assert!(unsafe { &*app.0 }.host.session.state().workspace.layout.column_stack(column).open_column.is_none());
        assert_eq!(app.pixels(), initial, "Auto-hide must consume the entire native contact without ink");
        // Positive control: dismissal must release input for the next stroke.
        stroke();
        assert_ne!(app.pixels(), initial);
        app.invoke("undo");
        app.draw_until_idle();
        assert_eq!(app.pixels(), initial);
    }
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

#[test]
fn apple_tab_preview_request_and_release_commit_the_same_slot() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let group = unsafe { &*app.0 }.host.session.state().workspace.layout.panel_group(layer_ui::Panel::Brushes).unwrap();
        for panel in ["toolbar", "navigator"] {
            app.action(json!({"type":"move_panel","panel":panel,"target":{"kind":"tab","group":group},"viewport":[1200,900]}));
        }
        let bounds = app.full_snapshot()["layout"]["groups"].as_array().unwrap().iter().find(|g| g["id"] == group).unwrap()["bounds"].clone();
        let (x, y) = (bounds["x"].as_f64().unwrap(), bounds["y"].as_f64().unwrap());
        let tabs = json!([
            {"group":group,"index":2,"bounds":{"x":x+140.,"y":y,"width":60.,"height":36.}},
            {"group":group,"index":0,"bounds":{"x":x,"y":y,"width":40.,"height":36.}},
            {"group":group,"index":1,"bounds":{"x":x+40.,"y":y,"width":100.,"height":36.}}
        ]);
        let drag = |phase: &str, delta: f64| {
            app.action(json!({"type":"drag_workspace","item":{"kind":"panel","panel":"toolbar"},"phase":phase,"position":[x+41.+delta,y+18.],"viewport":[1200,900],"tabs":tabs}))
        };
        drag("down", 0.);
        app.action(json!({"type":"begin_tab_drag","tabs":tabs,"clip":{"x":x+10.,"y":y,"width":190.,"height":36.}}));
        let preview = app.request(2, json!({"type":"workspace_drag_preview","item":{"kind":"panel","panel":"toolbar"},"position":[x+71.,y+18.],"tabs":tabs})).unwrap();
        assert_eq!(preview["drop"]["target"]["index"], 3);
        drag("up", 30.);
        let layout = &unsafe { &*app.0 }.host.session.state().workspace.layout;
        assert_eq!(layout.group_panels(group).unwrap(), &[layer_ui::Panel::Brushes, layer_ui::Panel::Navigator, layer_ui::Panel::Toolbar]);
    }
}


#[test]
fn apple_toolbar_styles_reach_ribbons_and_drawers_with_shared_metrics() {
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
            let view = app.full_snapshot();
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
            assert_eq!(app.state()["workspace"]["zen_mode"], true);
            assert_eq!(config(&app, "commands")["tile_style"], style);
            app.invoke("zen_mode");
            assert_eq!(app.state()["workspace"]["zen_mode"], false);
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
            serde_json::to_value(layer_ui::DockLayout::for_platform(if platform == 0 {
                layer_ui::Platform::Ios
            } else {
                layer_ui::Platform::Mac
            }))
            .unwrap(),
            "Fresh Apple editors use the shared workspace defaults for their platform"
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
                .find(|tile| tile["control"]["command"] == command);
            match tile {
                Some(tile) => app.action(json!({"type":"activate_tile","panel":"toolbar","tile":tile["id"]})),
                None => app.invoke(command.as_str().unwrap()),
            }
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
            Some(native_renderer());
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
fn apple_collapsed_toolbar_drawer_publishes_its_column_and_geometry() {
    for platform in [0, 1] {
        let app = App::new(platform);
        enable_column_drawers(&app, layer_ui::Panel::Brushes);
        let group = unsafe { &*app.0 }
            .host
            .session
            .state()
            .workspace
            .layout
            .panel_group(layer_ui::Panel::Brushes)
            .unwrap();
        app.action(json!({"type":"move_panel","panel":"toolbar","target":{"kind":"tab","group":group},"viewport":[1200,900]}));
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
        let root = app.full_snapshot();
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
        assert_eq!(app.state()["workspace"], collapsed, "Drawer geometry is transient");
    }
}

#[test]
fn apple_current_main_drawers_paper_and_zen_use_shared_actions() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let policy = unsafe { &*app.0 }.host.session.state().platform;
        app.action(json!({"type":"restore_workspace","workspace":layer_ui::WorkspaceState {
            layout: layer_ui::WorkspacePreset::Painter.layout(policy), ..Default::default()
        }}));
        let header = unsafe { &*app.0 }.host.session.state().workspace.layout.header.clone();
        let opener = |control| header.entries().find(|entry|
            entry.item == layer_ui::HeaderItem::Tool { control }).unwrap().id;
        let brush = opener(layer_ui::ToolbarControl::Command { command: layer_ui::CommandId::DrawingBrush });
        let sculpt = opener(layer_ui::ToolbarControl::Command { command: layer_ui::CommandId::Sculpt });
        let filter = opener(layer_ui::ToolbarControl::Panel { panel: layer_ui::Panel::Adjustments });
        let items = [brush,sculpt,filter].map(|id|
            json!({"id":id,"bounds":{"x":300,"y":0,"width":40,"height":60}}));
        app.action(json!({"type":"measure_header","height":60,"items":items}));
        let open = |id| app.action(json!({"type":"activate_header_item","id":id}));
        open(brush);
        assert_eq!(app.state()["customization"]["drawer"]["columns"], json!([["brush_sets"],["tools"],["tool_settings"]]));
        let sets = app.state()["tool_panels"]["brush_sets"]["groups"].as_array().unwrap().clone();
        assert_eq!(sets.len(), 10);
        for set in sets { app.action(set["action"].clone()); }
        app.action(json!({"type":"set_brush_size","value":77}));
        let remembered = app.state()["brush"].clone();
        open(sculpt);
        assert_eq!(app.state()["customization"]["drawer"]["columns"], json!([["sculpt_sets"],["tools"],["tool_settings"]]));
        assert_eq!(app.state()["tool_panels"]["sculpt_sets"]["groups"].as_array().unwrap().len(), 5);
        app.invoke("drawing_brush");
        assert_eq!(app.state()["brush"], remembered);
        open(filter);
        assert_eq!(app.state()["customization"]["drawer"]["columns"], json!([["filter_types"],["adjustments"],["properties"]]));
        app.action(json!({"type":"effect","action":{"op":"insert","effect":"brightness_contrast"}}));
        let selected = app.state()["layer_tools"]["editing_layer"]["id"].clone();
        open(filter); open(filter);
        app.action(json!({"type":"effect","action":{"op":"insert","effect":"curves"}}));
        assert_eq!(app.state()["layer_tools"]["editing_layer"]["id"], selected);
        assert_eq!(app.state()["filter_picker"]["selected"], "curves");
        assert!(app.state()["layers"].as_array().unwrap().iter().any(|l| l["id"] == 1 && l["drawing"] == true));
        app.action(json!({"type":"effect","action":{"op":"cancel_filter"}}));
        assert!(app.state()["customization"]["drawer"].is_null());
        app.invoke("undo");
        assert_eq!(app.state()["layers"][0]["id"], selected);
        app.action(json!({"type":"layer","action":{"op":"select","id":2,"mask":false}}));
        let control = app.state()["layer_properties"]["controls"][0].clone();
        assert_eq!(control["key"], "paper_color");
        assert!(app.state()["layer_tools"]["can_delete"].as_bool().unwrap());
        app.action(json!({"type":"set_color","rgba":[0.2,0.4,0.8,1]}));
        app.action(control["color_action"].clone());
        let paper = app.state()["layer_properties"].clone();
        app.invoke("undo");
        assert_ne!(app.state()["layer_properties"], paper);
        app.invoke("redo");
        assert_eq!(app.state()["layer_properties"], paper);
        let layout = app.state()["workspace"]["layout"].clone();
        app.invoke("zen_mode");
        let result = app.request(1, json!({"type":"chrome","event":{"kind":"refresh"},"facts":{"held":false,"dragging":false,"popup_open":false},"viewport":[1200,900]})).unwrap();
        assert!(result["keep_zen_button"].as_bool().unwrap());
        assert_eq!(app.state()["workspace"]["layout"], layout);
        app.action(json!({"type":"preferences","action":{"type":"edit","id":"zen_show_capy","value":false}}));
        let result = app.request(1, json!({"type":"chrome","event":{"kind":"refresh"},"facts":{"held":false,"dragging":false,"popup_open":false},"viewport":[1200,900]})).unwrap();
        assert!(!result["keep_zen_button"].as_bool().unwrap());
    }
}
