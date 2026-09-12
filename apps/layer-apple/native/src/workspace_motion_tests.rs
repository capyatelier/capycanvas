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
