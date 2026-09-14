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
            let view = app.request(3, Value::Null).unwrap()["header"].clone();
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
