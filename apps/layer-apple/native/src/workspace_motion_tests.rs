use super::*;

fn normalize_session_identity(app: &App, snapshot: &mut Value) {
    // Each publication must match its own live state before comparing two
    // sessions, whose immutable raster identities are independently allocated.
    // SnapshotFormatter preserves Value's widened numbers. Compare after the
    // same JSON encode/decode, including its floating-point round trip.
    let encoded = serde_json::to_vec(&app.state()).unwrap();
    let live: Value = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(snapshot["state"], live);
    let state = &mut snapshot["state"];
    for (index, tab) in state["tabs"].as_array_mut().unwrap().iter_mut().enumerate() {
        assert!(!tab["id"].as_str().unwrap().is_empty());
        tab["id"] = json!(index);
    }
    for layer in state["layers"].as_array_mut().unwrap() {
        assert!(layer["paint_revision"].is_u64());
        layer["paint_revision"] = json!(0);
    }
    let editing = &mut state["layer_tools"]["editing_layer"];
    if editing.is_object() {
        assert!(editing["paint_revision"].is_u64());
        editing["paint_revision"] = json!(0);
    }
}

#[test]
fn apple_request_seven_extends_the_shared_layout_update() {
    for platform in [0, 1] {
        let shared = App::new(platform);
        let app = App::new(platform);
        let bytes = unsafe { &mut *shared.0 }.host.take_layout_update_bytes().unwrap().unwrap();
        let mut expected: Value = serde_json::from_slice(&bytes).unwrap();
        let mut published = app.request(7, Value::Null).unwrap();
        let extension = published.as_object_mut().unwrap();
        assert!(extension.remove("display_status").is_some());
        assert!(extension.remove("document_tabs").is_some());
        assert_eq!(extension.remove("language_generation"), Some(json!(0)));
        assert_eq!(extension.remove("bootstrap"), Some(app.request(2, json!({"type":"bootstrap"})).unwrap()));
        assert_eq!(extension.remove("catalog"), Some(app.request(2, json!({"type":"catalog"})).unwrap()));
        normalize_session_identity(&shared, &mut expected);
        normalize_session_identity(&app, &mut published);
        assert_eq!(published, expected);
        assert!(app.request(7, Value::Null).is_none());
    }
}
