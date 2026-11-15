use super::*;

#[test]
fn apple_stamp_bytes_match_the_canonical_mask_and_known_receipts_omit_pixels() {
    let app = fixtures::drawing_app(0, [64, 64]);
    let catalog = app.request(2, json!({"type":"catalog"})).unwrap();
    let brush = catalog["brush_categories"].as_array().unwrap().iter()
        .flat_map(|category| category["brushes"].as_array().unwrap()).find(|brush| brush["label"] == "G-Pen").unwrap()["id"].clone();
    app.action(json!({"type":"select_brush","id":brush}));
    app.action(json!({"type":"set_brush_size","value":40}));
    app.draw_until_idle();
    let context = app.with(|app| app.host.session.state().toolbar_context());
    let query = json!({"type":"toolbar_stamp","context":context});
    let mut canonical = Value::Null;
    fixtures::until(&app, "canonical G-Pen stamp", || {
        canonical = app.request(2, query.clone()).unwrap_or(Value::Null);
        canonical["alpha"].as_array().is_some_and(|alpha| !alpha.is_empty()) && canonical["pending"] == false
    });
    let expected: Vec<u8> = serde_json::from_value(canonical["alpha"].clone()).unwrap();
    assert!(expected.iter().any(|&alpha| alpha != 0));
    for explicit in [true, false] {
        let source = CString::new(serde_json::to_string(&context).unwrap()).unwrap();
        let source = if explicit { source.as_ptr() } else { std::ptr::null() };
        let mut stamp = 0;
        let mut info = CapyBrushStampInfo::default();
        fixtures::until(&app, "typed G-Pen stamp", || {
            let next = unsafe { capy_apple_toolbar_stamp(app.0, source, 0) };
            if next == 0 { return false; }
            assert!(unsafe { capy_brush_stamp_read(next, &mut info) });
            if info.pending || info.count == 0 { unsafe { capy_brush_stamp_free(next) }; false }
            else { stamp = next; true }
        });
        let actual = copied_bytes(info.count, |offset, output, length| unsafe { capy_brush_stamp_copy(stamp, offset, output, length) });
        assert_eq!(actual, expected);
        assert_eq!(info.size, canonical["size"].as_u64().unwrap() as u32);
        assert_eq!(info.extent, canonical["extent"].as_f64().unwrap() as f32);
        let request = info.request;
        unsafe { capy_brush_stamp_free(stamp) };
        assert!(!unsafe { capy_brush_stamp_read(stamp, &mut info) });
        let known = unsafe { capy_apple_toolbar_stamp(app.0, source, request) };
        let mut receipt = CapyBrushStampInfo::default();
        assert!(unsafe { capy_brush_stamp_read(known, &mut receipt) });
        assert_eq!(receipt.request, request);
        assert!(!receipt.pending);
        assert_eq!(receipt.count, 0);
        assert!(unsafe { capy_brush_stamp_copy(known, 0, std::ptr::null_mut(), 0) });
        unsafe { capy_brush_stamp_free(known) };
    }
    let mut stale = context;
    stale.generation += 1;
    let stale = CString::new(serde_json::to_string(&stale).unwrap()).unwrap();
    assert_eq!(unsafe { capy_apple_toolbar_stamp(app.0, stale.as_ptr(), 0) }, 0);
    let error = unsafe { capy_apple_error(app.0) };
    assert_ne!(error, 0);
    assert!(handles::take_text(error).to_string_lossy().contains("Obsolete brush preview"));
    assert_eq!(unsafe { capy_apple_toolbar_stamp(0, std::ptr::null(), 0) }, 0);
}

#[test]
fn apple_phone_queries_round_trip_without_a_session() {
    let dock = layer_ui::WorkspacePreset::Painter.layout(layer_ui::Platform::Gtk);
    let layout = layer_ui::PhoneLayout::resolve(&dock, [402., 874.], [0.; 4], [0.; 2], [0; 2], false, false).unwrap();
    let hit = layout.tool_hits[0];
    for request in [
        json!({"kind":"slider_stretch_distance","layout":layout}),
        json!({"kind":"bar_begin","layout":layout,"tiles":dock.phone,"point":[hit.x+hit.width/2.,hit.y+hit.height/2.]}),
    ] {
        let source = request.to_string();
        assert_eq!(stateless(capy_apple_phone_ui, source.clone()), layer_ui::phone_ui(&source).unwrap());
    }
    assert!(stateless(capy_apple_phone_ui, "{")["error"].is_string());
    let output = unsafe { capy_apple_phone_ui(std::ptr::null()) };
    let missing: Value = serde_json::from_slice(handles::take_text(output).to_bytes()).unwrap();
    assert_eq!(missing["error"], "Missing phone request");
}

#[test]
fn apple_dialog_queries_round_trip_without_a_session() {
    for request in [
        json!({"kind":"header","intent":layer_ui::DialogIntent::editor(Some(layer_ui::DialogPrimary::Action)),"subpage":false}),
        json!({"kind":"header","intent":layer_ui::DialogIntent::editor(Some(layer_ui::DialogPrimary::Close)),"subpage":true}),
        json!({"kind":"escape","subpage":true,"search_focused":true,"search_has_text":true}),
        json!({"kind":"escape","subpage":true,"search_focused":true,"search_has_text":false}),
        json!({"kind":"escape","subpage":false,"search_focused":false,"search_has_text":false}),
        json!({"kind":"color_editor_layout","width":320,"minimum":[128,128]}),
        json!({"kind":"phone_window","viewport":[402,874]}),
    ] {
        let source = request.to_string();
        assert_eq!(stateless(capy_apple_dialog_ui, source.clone()), layer_ui::dialog_ui(&source).unwrap());
    }
    assert!(stateless(capy_apple_dialog_ui, "{")["error"].is_string());
    let output = unsafe { capy_apple_dialog_ui(std::ptr::null()) };
    let missing: Value = serde_json::from_slice(handles::take_text(output).to_bytes()).unwrap();
    assert_eq!(missing["error"], "Missing dialog request");
}

#[test]
fn apple_phone_menu_contact_retires_press_on_scroll_and_route_change() {
    let route = json!({"page":{"kind":"application","menu":"file"},"path":[]});
    let hit = json!({"kind":"row","position":[0,0]});
    let input = |contact: Value, route: &Value, input: Value| stateless(capy_apple_phone_ui,
        json!({"kind":"menu_input","contact":contact,"route":route,"can_back":true,"input":input}).to_string());
    let pressed = input(json!({}), &route, json!({"kind":"press","hit":hit}));
    assert_eq!(input(pressed["contact"].clone(), &route, json!({"kind":"release","hit":hit,"inside":true}))["reply"]["accepted"], true);
    let scrolled = input(pressed["contact"].clone(), &route, json!({"kind":"cancel_press"}));
    assert_eq!(input(scrolled["contact"].clone(), &route, json!({"kind":"release","hit":hit,"inside":true}))["reply"]["accepted"], false);
    assert_eq!(input(pressed["contact"].clone(), &json!({"page":null,"path":[]}), json!({"kind":"release","hit":hit,"inside":true}))["reply"]["accepted"], false);
}

#[test]
fn apple_dialog_pages_and_alerts_preserve_visible_bounds() {
    for (platform,viewport) in [(0,[402,874]),(0,[874,402]),(0,[1024,768]),(1,[402,874])] {
        let app = App::new(platform);
        let policy = app.borrow().host.session.state().platform;
        app.action(json!({"type":"restore_workspace","workspace":layer_ui::WorkspaceState {
            layout:layer_ui::WorkspacePreset::Painter.layout(policy),..Default::default()
        }}));
        assert_eq!(unsafe { capy_apple_resize(app.0, viewport[0], viewport[1], 1.) }, 0);
        app.action(json!({"type":"phone","action":{"kind":"metrics","native":[0,0],"native_counts":[0,0],"insets":[3,7,5,11],"corners":[0,0,0,0]}}));
        let visible = layer_ui::Bounds { x:3.,y:7.,width:viewport[0] as f32-6.,height:viewport[1] as f32-140. };
        let state = app.state();
        for role in [layer_ui::DialogRole::Editor,layer_ui::DialogRole::Alert,layer_ui::DialogRole::CanvasPanel] {
            let expected = app.with(|app| app.host.session.state().dialog_presentation_in(role,app.host.logical,visible));
            assert_eq!(app.request(2,json!({"type":"dialog_presentation","role":role,"visible":visible})).unwrap(),json!(expected));
            assert_eq!(app.request(2,json!({"type":"dialog_bounds","role":role,"visible":visible,"measured":[600.,200.]})).unwrap(),json!(expected.bounds(|| [600.,200.])));
            if role == layer_ui::DialogRole::Editor {
                assert_eq!(expected.single_pane(), platform == 0 && viewport[0].min(viewport[1]) < 480);
            }
            if role == layer_ui::DialogRole::Alert { assert_eq!(expected,layer_ui::DialogPresentation::Floating); }
        }
        assert_eq!(app.state(),state);
    }
}

#[test]
fn apple_device_class_admits_phone_and_tablet_finger_input() {
    for phone in [true, false] {
        let app = App::new(1);
        assert!(!app.with(|app| app.host.session.finger_drawing()));
        assert_eq!(unsafe { capy_apple_device_class(app.0, phone) }, 0);
        assert!(app.with(|app| app.host.session.finger_drawing()));
    }
    assert_eq!(unsafe { capy_apple_device_class(0, true) }, -1);
}

#[test]
fn apple_preferences_publish_trigger_pages_with_touch_and_pen_settings() {
    for platform in [0, 1] {
        let app = App::new(platform);
        for theme in ["light", "dark"] {
            app.action(json!({"type":"set_theme","theme":theme}));
            app.invoke("settings");
            let preferences = app.full_snapshot()["preferences"].clone();
            let pages = preferences["pages"].as_array().unwrap();
            assert_eq!(pages.iter().map(|page| page["id"].as_str().unwrap()).collect::<Vec<_>>(),
                ["appearance", "color", "touch", "input", "shortcuts", "about"]);
            let touch = pages.iter().find(|page| page["id"] == "touch").unwrap();
            assert_eq!(touch["title"], "Touch");
            let groups = touch["groups"].as_array().unwrap();
            assert_eq!(groups.iter().map(|group| group["title"].as_str().unwrap()).collect::<Vec<_>>(),
                if platform == 0 { vec!["Touch gestures", "Navigation"] } else { vec!["Navigation"] });
            let rows = groups.iter().flat_map(|group| group["rows"].as_array().unwrap()).collect::<Vec<_>>();
            assert!(rows.iter().all(|row| row["visible"] == true));
            let mut expected = vec!["rotate_with_two_fingers", "quick_pinch_fit", "quick_pinch_restore"];
            if platform == 0 { expected.insert(0, "finger_drawing"); }
            assert_eq!(rows.iter().map(|row| row["id"].as_str().unwrap()).collect::<Vec<_>>(), expected);
            let input = pages.iter().find(|page| page["id"] == "input").unwrap();
            assert_eq!(input["title"], "Pen & Mouse");
            assert!(input["groups"].as_array().unwrap().iter().flat_map(|group| group["rows"].as_array().unwrap())
                .all(|row| row["id"] != "finger_drawing"));
            assert_eq!(input["groups"][0]["title"], "Pointer");
            assert_eq!(input["groups"][0]["rows"].as_array().unwrap().iter()
                .map(|row| row["id"].as_str().unwrap()).collect::<Vec<_>>(),
                ["cursor", "hide_cursor_while_drawing", "wheel_behavior", "pan_speed", "zoom_speed"]);
            let color = pages.iter().find(|page| page["id"] == "color").unwrap();
            let layers = color["groups"].as_array().unwrap().last().unwrap();
            assert_eq!(layers["title"], "Layers");
            assert_eq!(layers["rows"][0]["id"], "pass_through_groups");
            let triggers = preferences["shortcut_page"]["triggers"].as_array().unwrap();
            let fingers = triggers.iter().filter(|row| row["id"].as_str().unwrap().starts_with("touch.")).collect::<Vec<_>>();
            let pen = triggers.iter().filter(|row| row["id"].as_str().unwrap().starts_with("pen.button.")).collect::<Vec<_>>();
            assert_eq!(fingers.len(), if platform == 0 { 3 } else { 0 });
            assert_eq!(pen.len(), if platform == 1 { 2 } else { 0 });
            assert!(fingers.iter().all(|row| row["page"] == "touch"));
            assert!(pen.iter().all(|row| row["page"] == "input"));
            assert!(fingers.iter().all(|row| row["section"] == groups[0]["title"]));
            assert_eq!(fingers.iter().map(|row| row["id"].as_str().unwrap()).collect::<Vec<_>>(),
                if platform == 0 { vec!["touch.tap.2", "touch.tap.3", "touch.tap.4"] } else { vec![] });
            assert!(pen.iter().all(|row| row["section"] == "Pen buttons"));
            let mut searches = vec![("pinch", "Quick pinch to fit", "touch"),
                ("pan speed", "Scroll pan speed", "input"), ("pass through", "Use Pass Through for new groups", "color")];
            if platform == 0 { searches.push(("two-finger tap", "Two-finger tap", "touch")); }
            for (query, title, page) in searches {
                app.action(json!({"type":"preferences","action":{"type":"search","query":query}}));
                let results = app.full_snapshot()["preferences"]["search_results"].clone();
                let results = results.as_array().unwrap();
                let result = results.iter().find(|row| row["title"] == title).unwrap_or_else(|| panic!("{platform}/{query}: {results:?}"));
                assert_eq!(result["action"], json!({"type":"page","page":page}), "{platform}/{query}");
                app.action(json!({"type":"preferences","action":result["action"]}));
                assert_eq!(app.full_snapshot()["preferences"]["page"], page);
            }
            if platform == 1 {
                app.action(json!({"type":"preferences","action":{"type":"search","query":"two-finger tap"}}));
                assert!(app.full_snapshot()["preferences"]["search_results"].as_array().unwrap().iter()
                    .all(|row| row["title"] != "Two-finger tap"));
            }
            app.action(json!({"type":"close_settings"}));
        }
    }
}

#[test]
fn apple_phone_one_finger_choices_keep_the_static_icon_and_open_touch_settings() {
    for theme in ["light", "dark"] {
        for mode in 0..4 {
            let app = App::new(0);
            app.action(json!({"type":"set_theme","theme":theme}));
            app.action(json!({"type":"restore_workspace","workspace":layer_ui::WorkspaceState {
                layout:layer_ui::WorkspacePreset::Illustrator.layout(layer_ui::Platform::Ios),..Default::default()
            }}));
            app.action(json!({"type":"preferences","action":{"type":"edit","id":"finger_drawing","value":mode}}));
            let snapshot = app.full_snapshot();
            let commands = snapshot["panels"].as_array().unwrap().iter().find(|panel| panel["id"] == "commands").unwrap();
            assert_eq!(commands["tiles"][0]["icon"], "finger-draw");
            assert_eq!(unsafe { capy_apple_resize(app.0, 412, 891, 1.) }, 0);
            assert_eq!(unsafe { capy_apple_device_class(app.0, true) }, 0);
            app.action(json!({"type":"phone","action":{"kind":"capy"}}));
            let menu = app.request(2, json!({"type":"document_tabs","op":"phone_menu"})).unwrap();
            assert_eq!(menu["strip"][2]["icon"], "layer-finger-draw-symbolic");
            let finger = menu["pages"].as_array().unwrap().iter().find(|page| page["id"]["page"]["kind"] == "one_finger_drag").unwrap();
            assert_eq!(finger["sections"].as_array().unwrap().len(), 2);
            let choices = finger["sections"][0].as_array().unwrap();
            let labels = ["Auto", "Draw", "Move canvas", "Off"];
            assert_eq!(choices.iter().map(|row| row["label"].as_str().unwrap()).collect::<Vec<_>>(), labels);
            assert_eq!(choices.iter().filter(|row| row["selected"] == true).count(), 1);
            assert_eq!(choices[mode]["selected"], true);
            let more = finger["sections"][1].as_array().unwrap();
            assert_eq!(more.len(), 1);
            assert_eq!(more[0]["label"], "More Touch Settings…");
            app.action(json!({"type":"phone","action":{"kind":"menu_select","route":finger["id"]}}));
            app.action(more[0]["action"].clone());
            assert_eq!(app.full_snapshot()["preferences"]["page"], "touch");
        }
    }
}

#[test]
fn apple_enclose_fill_flat_choices_and_source_use_native_transport() {
    for platform in [0, 1] {
        for theme in ["light", "dark"] {
            let app = App::new(platform);
            app.action(json!({"type":"set_theme","theme":theme}));
            let label = |command: &str| app.state()["commands"].as_array().unwrap().iter()
                .find(|item| item["id"] == command).unwrap()["label"].clone();
            let policy = app.borrow().host.session.state().platform;
            app.action(json!({"type":"restore_workspace","workspace":layer_ui::WorkspaceState {
                layout: layer_ui::WorkspacePreset::Illustrator.layout(policy), ..Default::default()
            }}));
            let fill = label("fill");
            let lasso = label("lasso_fill");
            let enclose = label("enclose_fill");
            let unfilled = label("paint_unfilled");
            let choose = |section: &str, label: &Value| app.action(app.state()["tool_set"][section]
                .as_array().unwrap().iter().find(|item| item["label"] == *label).unwrap()["action"].clone());
            let selected = |command: &str| app.state()["commands"].as_array().unwrap().iter()
                .find(|item| item["id"] == command).unwrap()["selected"] == true;
            app.invoke("fill");
            choose("subtools", &lasso);
            choose("subtools", &enclose);
            choose("subtools", &fill);
            assert!(selected("fill"));
            choose("subtools", &lasso);
            assert!(selected("lasso_fill"));
            choose("subtools", &enclose);
            let state = app.full_snapshot()["state"].clone();
            assert_eq!(state["tool_set"]["subtools"].as_array().unwrap().iter().map(|item| item["label"].clone()).collect::<Vec<_>>(), [fill, lasso.clone(), enclose.clone(), unfilled]);
            assert!(state["tool_set"]["groups"].as_array().unwrap().is_empty());
            assert!(selected("enclose_fill"));
            for source in ["selection_visible", "selection_editing", "selection_reference"] {
                let state = app.state();
                let choice = &state["tool_extra"].as_array().unwrap().iter()
                    .find(|item| item["Choice"]["id"] == "selection-source").unwrap()["Choice"];
                assert_eq!(choice["segmented"], true);
                assert_eq!(choice["items"].as_array().unwrap().iter().map(|item| item["label"].clone()).collect::<Vec<_>>(),
                    [label("selection_reference"), label("selection_visible"), label("selection_editing")]);
                app.action(choice["items"].as_array().unwrap().iter()
                    .find(|item| item["action"]["command"] == source).unwrap()["action"].clone());
                assert!(selected(source));
                assert!(selected("enclose_fill"));
                assert_eq!(app.state()["tool_extra"].as_array().unwrap().iter()
                    .any(|item| item["Checklist"]["id"] == "reference-layers"), source == "selection_reference");
            }
            choose("subtools", &lasso);
            assert!(selected("lasso_fill"));
            assert!(!app.state()["tool_extra"].as_array().unwrap().iter()
                .any(|item| item["Choice"]["id"] == "selection-source" || item["Checklist"]["id"] == "reference-layers"));
        }
    }
}

#[test]
fn apple_geometry_metrics_do_not_require_a_prepared_language() {
    let request = CString::new(r#"{"language":"th","request":{"type":"metrics"}}"#).unwrap();
    let output = unsafe { capy_apple_toolbar_ui(request.as_ptr()) };
    let metrics: Value = serde_json::from_slice(handles::take_text(output).to_bytes()).unwrap();
    assert_eq!(metrics, layer_ui::toolbar_metrics());
    assert!(metrics["surface_radius"].as_f64().unwrap() > 0.);
    assert!(metrics["layer_edge_inset"].as_f64().unwrap() > 0.);
}

#[test]
fn apple_toolbar_queries_match_the_shared_transport_without_a_session() {
    let launch = CString::new(json!({"saved":"", "preferred_languages":["en"]}).to_string()).unwrap();
    let prepared = App::from_handle(unsafe { launch_json(0, launch.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut()) });
    assert!(!(prepared.0==0));
    for request in [
        json!({"type":"slider_layout","width":44,"height":176,"axis":"vertical","style":"small","slider_style":{"show_value":false,"expand":false}}),
        json!({"type":"slider_layout","width":54,"height":400,"axis":"vertical","style":"medium","slider_style":{"show_value":true,"expand":true}}),
        json!({"type":"slider_readout","style":"medium","axis":"vertical","value":430,
            "control":layer_ui::ToolbarNumericBinding::BrushSize.numeric()}),
        json!({"type":"slider_spec","control":{"kind":"brush_size_slider"}}),
        json!({"type":"style","style":"medium"}),
        json!({"type":"choice_style","labeled":true}),
        json!({"type":"choice_style","labeled":true,"style":"large"}),
        json!({"type":"choice_style","labeled":false,"style":"large"}),
        json!({"type":"slider_preview","control":{"kind":"brush_strength_slider"},"style":"medium","value":0.5,"length":176,"extent":64}),
    ] {
        let expected = layer_ui::toolbar_ui(serde_json::from_value(request.clone()).unwrap(), &prepared.with(|native|native.host.session.localization().clone())).unwrap();
        assert_eq!(localized_stateless(capy_apple_toolbar_ui, request.to_string()), expected, "{request}");
    }
    assert!(localized_stateless(capy_apple_toolbar_ui, "{").get("error").is_some());
    assert!(localized_stateless(capy_apple_toolbar_ui, r#"{"type":"slider_spec","control":{"kind":"color"}}"#).get("error").is_some());
    let output = unsafe { capy_apple_toolbar_ui(std::ptr::null()) };
    let missing: Value = serde_json::from_slice(handles::take_text(output).to_bytes()).unwrap();

    assert_eq!(missing["error"], "Missing toolbar request");
}
