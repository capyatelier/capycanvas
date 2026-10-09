use super::*;

#[test]
fn apple_enclose_fill_category_subtool_and_source_use_native_transport() {
    for platform in [0, 1] {
        for theme in ["light", "dark"] {
            let app = App::new(platform);
            app.action(json!({"type":"set_theme","theme":theme}));
            let label = |command: &str| app.state()["commands"].as_array().unwrap().iter()
                .find(|item| item["id"] == command).unwrap()["label"].clone();
            let fill = label("fill");
            let lasso = label("lasso_fill");
            let enclose = label("enclose_fill");
            let choose = |section: &str, label: &Value| app.action(app.state()["tool_set"][section]
                .as_array().unwrap().iter().find(|item| item["label"] == *label).unwrap()["action"].clone());
            let selected = |command: &str| app.state()["commands"].as_array().unwrap().iter()
                .find(|item| item["id"] == command).unwrap()["selected"] == true;
            app.invoke("fill");
            choose("groups", &lasso);
            choose("subtools", &enclose);
            choose("groups", &fill);
            choose("groups", &lasso);
            let state = app.full_snapshot()["state"].clone();
            assert_eq!(state["tool_set"]["groups"].as_array().unwrap().iter().map(|item| item["label"].clone()).collect::<Vec<_>>(), [fill, lasso.clone()]);
            assert_eq!(state["tool_set"]["subtools"].as_array().unwrap().iter().map(|item| item["label"].clone()).collect::<Vec<_>>(), [lasso.clone(), enclose.clone()]);
            assert!(selected("enclose_fill"));
            for source in ["selection_visible", "selection_editing", "selection_reference"] {
                let command = app.state()["tool_actions"].as_array().unwrap().iter()
                    .find(|item| item["command"] == source).unwrap()["command"].clone();
                app.action(json!({"type":"invoke","command":command}));
                assert!(selected(source));
                assert!(selected("enclose_fill"));
            }
            choose("subtools", &lasso);
            assert!(selected("lasso_fill"));
            assert!(!app.state()["tool_actions"].as_array().unwrap().iter()
                .any(|action| ["selection_visible", "selection_editing", "selection_reference"].iter().any(|source| action["command"] == *source)));
        }
    }
}

#[test]
fn apple_toolbar_queries_match_the_shared_transport_without_a_session() {
    let launch = CString::new(json!({"saved":"", "preferred_languages":["en"]}).to_string()).unwrap();
    let prepared = App(unsafe { capy_apple_launch(0, launch.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut()) });
    assert!(!prepared.0.is_null());
    for request in [
        json!({"type":"slider_layout","width":44,"height":176,"axis":"vertical"}),
        json!({"type":"slider_spec","control":{"kind":"brush_size_slider"}}),
        json!({"type":"style","style":"medium"}),
        json!({"type":"slider_preview","control":{"kind":"brush_opacity_slider"},"style":"medium","value":0.5,"length":176,"extent":64}),
    ] {
        let expected = layer_ui::toolbar_ui(serde_json::from_value(request.clone()).unwrap(), unsafe { (*prepared.0).host.session.localization() }).unwrap();
        assert_eq!(localized_stateless(capy_apple_toolbar_ui, request.to_string()), expected, "{request}");
    }
    assert!(localized_stateless(capy_apple_toolbar_ui, "{").get("error").is_some());
    assert!(localized_stateless(capy_apple_toolbar_ui, r#"{"type":"slider_spec","control":{"kind":"color"}}"#).get("error").is_some());
    let output = unsafe { capy_apple_toolbar_ui(std::ptr::null()) };
    let missing: Value = serde_json::from_slice(unsafe { CStr::from_ptr(output) }.to_bytes()).unwrap();
    unsafe { capy_apple_string_free(output) };
    assert_eq!(missing["error"], "Missing toolbar request");
}
