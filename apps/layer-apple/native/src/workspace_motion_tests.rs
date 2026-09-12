use super::*;

// Separate sessions keep the compatibility and incremental acknowledgement
// policies independent while identical actions establish the expected models.
#[test]
fn incremental_apple_abi_moves_retained_models_and_preserves_completion() {
    for platform in [0, 1] {
        let legacy = App::new(platform);
        let app = App::new(platform);
        let publish = |app: &App, kind| {
            let text = unsafe { capy_apple_request(app.0, kind, std::ptr::null()) };
            assert!(!text.is_null());
            let bytes = unsafe { CStr::from_ptr(text) }.to_bytes();
            let result = (serde_json::from_slice::<Value>(bytes).unwrap(), bytes.len());
            unsafe { capy_apple_string_free(text) };
            result
        };
        let action = |value: Value| {
            legacy.action(value.clone());
            app.action(value);
        };
        let full = || {
            let (old, _) = publish(&legacy, 3);
            let (mut next, _) = publish(&app, 5);
            assert!(next["workspace_update"].is_object());
            let motion = next
                .as_object_mut()
                .unwrap()
                .remove("workspace_update")
                .unwrap();
            assert_eq!(
                next, old,
                "Full models must preserve the compatibility schema"
            );
            (next, motion)
        };
        let (initial, _) = full();
        let saved = initial["state"]["workspace"].clone();
        let group = initial["layout"]["groups"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["active"] == "brushes")
            .unwrap();
        let start = [
            group["bounds"]["x"].as_f64().unwrap() + 10.,
            group["bounds"]["y"].as_f64().unwrap() + 10.,
        ];
        let drag = |phase: &str, point: [f64; 2]| {
            action(json!({"type":"drag_workspace",
            "item":{"kind":"panel","panel":"brushes"},"phase":phase,"position":point,"viewport":[1200,900],"tabs":[]}))
        };
        for cancel in [true, false] {
            drag("down", start);
            full();
            drag("move", [600., 400.]);
            let (_, detached) = full();
            let retained_revision = detached["model_revision"].clone();
            let mut legacy_bytes = 0;
            let mut motion_bytes = 0;
            for step in 1..=32 {
                drag("move", [600. + step as f64, 400. + step as f64]);
                let (old, old_size) = publish(&legacy, 3);
                let (next, next_size) = publish(&app, 5);
                legacy_bytes += old_size;
                motion_bytes += next_size;
                assert!(next.get("state").is_none() && next.get("workspace_persistence").is_none());
                assert_eq!(
                    next["workspace_update"]["model_revision"],
                    retained_revision
                );
                let position = &next["workspace_update"]["drag"]["group"];
                let actual = old["layout"]["groups"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|g| g["id"] == position["id"])
                    .unwrap();
                assert_eq!(position["bounds"], actual["bounds"]);
                assert_eq!(app.state()["workspace"], legacy.state()["workspace"]);
            }
            assert!(motion_bytes < legacy_bytes / 10);
            println!(
                "Apple platform {platform}, cancel {cancel}: 32 moves, compatibility {legacy_bytes} bytes, incremental {motion_bytes} bytes"
            );
            // Final input is newer than the last published move.
            drag(if cancel { "cancel" } else { "up" }, [660., 460.]);
            let (completed, update) = full();
            assert!(update["drag"].is_null());
            if cancel {
                assert_eq!(completed["state"]["workspace"], saved);
            } else {
                assert!(completed.get("workspace_persistence").is_some());
                let committed = completed["state"]["workspace"].clone();
                action(json!({"type":"invoke","command":"undo_workspace"}));
                assert_eq!(full().0["state"]["workspace"], saved);
                action(json!({"type":"invoke","command":"redo_workspace"}));
                assert_eq!(full().0["state"]["workspace"], committed);
            }
        }
    }
}

#[test]
fn layout_apple_abi_resizes_retained_content_and_matches_legacy_geometry() {
    fn publish(app: &App, request: u32) -> Option<(Value, usize)> {
        let text = unsafe { capy_apple_request(app.0, request, std::ptr::null()) };
        assert!(unsafe { capy_apple_error(app.0) }.is_null());
        if text.is_null() {
            return None;
        }
        let bytes = unsafe { CStr::from_ptr(text) }.to_bytes();
        let result = (serde_json::from_slice(bytes).unwrap(), bytes.len());
        unsafe { capy_apple_string_free(text) };
        Some(result)
    }
    for platform in [0, 1] {
        for floating in [false, true] {
            for cancel in [false, true] {
                let legacy = App::new(platform);
                let app = App::new(platform);
                let action = |value: Value| {
                    legacy.action(value.clone());
                    app.action(value);
                };
                if floating {
                    action(json!({"type":"move_panel","panel":"navigator",
                        "target":{"kind":"float","position":[600,300]},"viewport":[1200,900]}));
                }
                let full = || {
                    let old = publish(&legacy, 3).unwrap().0;
                    let mut next = publish(&app, 7).unwrap().0;
                    let update = next
                        .as_object_mut()
                        .unwrap()
                        .remove("workspace_update")
                        .unwrap();
                    assert_eq!(
                        next, old,
                        "Full reflow boundaries must retain the compatibility schema"
                    );
                    (next, update)
                };
                let (initial, mut update) = full();
                let saved = initial["state"]["workspace"].clone();
                let item = if floating {
                    initial["layout"]["groups"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|g| g["floating"] == true && g["active"] == "navigator")
                        .unwrap()
                } else {
                    initial["layout"]["dividers"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|d| {
                            let b = &d["bounds"];
                            b["height"].as_f64().unwrap() > 100.
                                && b["width"].as_f64().unwrap() <= 6.
                                && b["x"].as_f64().unwrap() > 100.
                                && b["x"].as_f64().unwrap() < 600.
                        })
                        .unwrap()
                };
                let b = &item["bounds"];
                let x = b["x"].as_f64().unwrap() + b["width"].as_f64().unwrap();
                let y = b["y"].as_f64().unwrap() + b["height"].as_f64().unwrap() / 2.;
                let resize = |phase: &str, delta: f64| {
                    let value = if floating {
                        json!({"type":"resize_floating","group":item["id"],"edge":"right",
                            "phase":phase,"position":[x+delta,y],"viewport":[1200,900]})
                    } else {
                        json!({"type":"drag_divider","id":item["id"],"phase":phase,
                            "position":[x+delta,y],"viewport":[1200,900]})
                    };
                    action(value);
                };
                resize("down", 0.);
                let _ = publish(&legacy, 3);
                if let Some((down, _)) = publish(&app, 7) {
                    update = down["workspace_update"].clone();
                }
                let retained = update["content_revision"].clone();
                let mut legacy_bytes = 0;
                let mut reflow_bytes = 0;
                for step in 1..=16 {
                    resize("move", f64::from(step * 2));
                    let (old, old_size) = publish(&legacy, 3).unwrap();
                    let (next, next_size) = publish(&app, 7).unwrap();
                    legacy_bytes += old_size;
                    reflow_bytes += next_size;
                    assert!(
                        next.get("state").is_none() && next.get("workspace_persistence").is_none()
                    );
                    assert_eq!(next["workspace_update"]["content_revision"], retained);
                    assert_eq!(next["layout"], old["layout"]);
                    assert_eq!(next["camera"], old["state"]["camera"]);
                    assert_eq!(next["panel_measurements"], old["panel_measurements"]);
                    for field in ["bands", "floating", "collapsed", "fit_tab_groups"] {
                        assert_eq!(
                            next["workspace_layout"][field],
                            old["state"]["workspace"]["layout"][field]
                        );
                    }
                    assert_eq!(old["panels"], initial["panels"]);
                    assert_eq!(app.state()["workspace"], legacy.state()["workspace"]);
                    assert!(publish(&app, 7).is_none());
                }
                assert!(reflow_bytes < legacy_bytes / 2);
                resize(if cancel { "cancel" } else { "up" }, 36.);
                let completed = full().0;
                if cancel {
                    assert_eq!(completed["state"]["workspace"], saved);
                } else {
                    assert!(completed.get("workspace_persistence").is_some());
                    action(json!({"type":"invoke","command":"undo_workspace"}));
                    assert_eq!(full().0["state"]["workspace"], saved);
                    action(json!({"type":"invoke","command":"redo_workspace"}));
                    assert_eq!(
                        full().0["state"]["workspace"],
                        completed["state"]["workspace"]
                    );
                }
                println!(
                    "Apple {platform}, floating {floating}, cancel {cancel}: 16 resize moves; legacy {legacy_bytes} bytes, retained reflow {reflow_bytes} bytes"
                );
            }
        }
    }
}
