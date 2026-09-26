use super::*;

fn request(app: &App, value: Value) -> Value {
    let action = matches!(value["op"].as_str(), Some("finish" | "step"));
    let reply = app
        .request(2, json!({"type":"header","request":value}))
        .unwrap();
    if action && !reply.is_null() {
        app.action(reply.clone());
    }
    reply
}
fn edit(app: &App, action: Value) {
    app.action(json!({"type":"customize","action":{"type":"header","action":action}}));
}
fn geometry(app: &App, platform: u32) -> Value {
    request(
        app,
        json!({"op":"geometry","width":1200,"insets":[if platform == 1 {90} else {0},0],"metrics":[]}),
    )
}
fn begin(app: &App, platform: u32, source: Value, press: Value, grab: Value) {
    assert_eq!(
        request(
            app,
            json!({"op":"begin","source":source,"width":1200,
        "insets":[if platform == 1 {90} else {0},0],"metrics":[],"press":press,"grab":grab})
        ),
        true
    );
}

#[test]
fn apple_header_drag_preview_commits_one_edit_and_preserves_artwork() {
    for platform in [0, 1] {
        for size in ["small", "medium", "large"] {
            let app = App::new(platform);
            edit(&app, json!({"type":"set_size","size":size}));
            let initial = app.state();
            app.invoke("customize_workspace_ui");
            let view = app.full_snapshot()["header"].clone();
            assert_eq!(view["editing"], true);
            assert_eq!(view["model"]["size"], size);
            assert!(view["components"].as_array().unwrap().iter().all(|item| {
                let kind = item["item"]["kind"].as_str().unwrap();
                kind != "fullscreen" && (platform == 0 || !["menu", "menu_labels"].contains(&kind))
            }));
            let bounds = geometry(&app, platform);
            let held = &bounds["items"][0];
            let id = held["id"].clone();
            let point = json!([held["bounds"]["x"].as_f64().unwrap() + 8., 20.]);
            begin(
                &app,
                platform,
                json!({"kind":"item","value":id}),
                point,
                held["bounds"].clone(),
            );
            let end = json!([
                bounds["zones"][2]["x"].as_f64().unwrap()
                    + bounds["zones"][2]["width"].as_f64().unwrap()
                    - 1.,
                20.
            ]);
            let unchanged = app.state()["workspace"].clone();
            let preview = request(&app, json!({"op":"preview","position":end}));
            assert_eq!(preview["detached"], false);
            assert!(!preview["action"].is_null());
            assert_eq!(
                app.state()["workspace"],
                unchanged,
                "Held motion must not publish workspace edits"
            );
            request(&app, json!({"op":"finish","position":end,"cancel":false}));
            let arranged = app.state()["workspace"]["layout"].clone();
            assert_ne!(arranged, initial["workspace"]["layout"]);
            assert_eq!(
                arranged["header"]["zones"][2]
                    .as_array()
                    .unwrap()
                    .last()
                    .unwrap()["id"],
                id
            );
            edit(&app, json!({"type":"edit","editing":false}));
            app.invoke("undo_workspace");
            assert_eq!(
                app.state()["workspace"]["layout"],
                initial["workspace"]["layout"]
            );
            app.invoke("redo_workspace");
            assert_eq!(app.state()["workspace"]["layout"], arranged);
            assert_eq!(app.state()["layers"], initial["layers"]);
            assert_eq!(app.state()["brush"], initial["brush"]);
        }
    }
}

#[test]
fn apple_header_capture_cancels_on_resize_blur_and_source_replacement() {
    for platform in [0, 1] {
        for interruption in ["cancel", "resize", "blur", "replace"] {
            let app = App::new(platform);
            app.invoke("customize_workspace_ui");
            let saved = app.state()["workspace"]["layout"].clone();
            let bounds = geometry(&app, platform);
            let held = &bounds["items"][0];
            begin(
                &app,
                platform,
                json!({"kind":"item","value":held["id"]}),
                json!([held["bounds"]["x"].as_f64().unwrap() + 8., 20.]),
                held["bounds"].clone(),
            );
            let outside = json!([700., 200.]);
            assert_eq!(
                request(&app, json!({"op":"preview","position":outside}))["detached"],
                true
            );
            match interruption {
                "resize" => {
                    assert_eq!(unsafe { capy_apple_resize(app.0, 900, 900, 1.) }, 0);
                }
                "blur" => {
                    app.request(1, json!({"type":"blur"}));
                }
                "replace" => {
                    app.action(
                        json!({"type":"restore_workspace","workspace":app.state()["workspace"]}),
                    );
                }
                _ => {}
            }
            request(
                &app,
                json!({"op":"finish","position":outside,"cancel":interruption=="cancel"}),
            );
            assert_eq!(
                app.state()["workspace"]["layout"],
                saved,
                "{platform}/{interruption}"
            );
        }
    }
}

#[test]
fn apple_header_joins_adjacent_icon_controls_into_bars_outside_customization() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let policy = unsafe { &*app.0 }.host.session.state().platform;
        app.action(json!({"type":"restore_workspace","workspace":layer_ui::WorkspaceState {
            layout: layer_ui::WorkspacePreset::Painter.layout(policy), ..Default::default()
        }}));
        let view = geometry(&app, platform);
        let kinds = |bar: &Value| -> Vec<String> {
            let header = &unsafe { &*app.0 }.host.session.state().workspace.layout.header;
            bar["items"].as_array().unwrap().iter().map(|id| {
                let entry = header.entry(id.as_u64().unwrap() as u32).unwrap();
                serde_json::to_value(entry.item).unwrap()["kind"].as_str().unwrap().to_owned()
            }).collect()
        };
        let bars = view["bars"].as_array().unwrap();
        let groups: Vec<_> = bars.iter().map(kinds).collect();
        assert_eq!(groups[0], ["capy"]);
        assert_eq!(groups.last().unwrap(), &["tool"; 5]);
        let menu = if platform == 0 { vec!["menu"] } else { vec![] };
        assert_eq!(groups[1], [menu, vec!["tool"; 3]].concat());
        let gap = f64::from(unsafe { &*app.0 }.host.session.state().workspace.layout.header.size.gap());
        for bar in bars {
            let members: Vec<_> = view["items"].as_array().unwrap().iter()
                .filter(|item| bar["items"].as_array().unwrap().contains(&item["id"])).collect();
            for pair in members.windows(2) {
                let end = pair[0]["bounds"]["x"].as_f64().unwrap() + pair[0]["bounds"]["width"].as_f64().unwrap();
                assert_eq!(end + gap, pair[1]["bounds"]["x"].as_f64().unwrap(), "members sit one tile gap apart");
            }
            let tile = members[0]["bounds"]["height"].as_f64().unwrap();
            assert_eq!(bar["bounds"]["height"].as_f64().unwrap(), tile);
            assert_eq!(bar["bounds"]["y"].as_f64().unwrap(), members[0]["bounds"]["y"].as_f64().unwrap());
        }
        app.invoke("customize_workspace_ui");
        assert!(geometry(&app, platform)["bars"].as_array().unwrap().is_empty(), "Customization keeps items separate");
    }
}

#[test]
fn apple_palettes_publish_the_shared_selection_roles() {
    for platform in [0, 1] {
        let app = App::new(platform);
        for (theme, selection, header) in [("dark", "#40546e", "#40546e"), ("light", "#c0d7f6", "#afc6e5")] {
            app.action(json!({"type":"set_theme","theme":theme}));
            let palette = &app.state()["palette"];
            assert_eq!(palette["selection"], selection, "{theme}");
            assert_eq!(palette["header_selection"], header, "{theme}");
            assert_eq!(palette["accent"], "#3584e4");
            assert_eq!(palette["checker_light"], "#dcdcdc");
        }
        app.action(json!({"type":"preferences","action":{"type":"edit","id":"accent","value":"#e62d42"}}));
        assert_eq!(app.state()["palette"]["accent"], "#e62d42");
        assert_ne!(app.state()["palette"]["selection"], "#c0d7f6", "Selection follows the accent");
    }
}
