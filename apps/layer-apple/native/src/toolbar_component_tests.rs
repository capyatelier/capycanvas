use super::*;

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
            let choose = |section: &str, label: &Value| app.action(app.state()["tool_set"][section]
                .as_array().unwrap().iter().find(|item| item["label"] == *label).unwrap()["action"].clone());
            let selected = |command: &str| app.state()["commands"].as_array().unwrap().iter()
                .find(|item| item["id"] == command).unwrap()["selected"] == true;
            app.invoke("fill");
            choose("groups", &lasso);
            choose("groups", &enclose);
            choose("groups", &fill);
            assert!(selected("fill"));
            choose("groups", &lasso);
            assert!(selected("lasso_fill"));
            choose("groups", &enclose);
            let state = app.full_snapshot()["state"].clone();
            assert_eq!(state["tool_set"]["groups"].as_array().unwrap().iter().map(|item| item["label"].clone()).collect::<Vec<_>>(), [fill, lasso.clone(), enclose.clone()]);
            assert!(state["tool_set"]["subtools"].as_array().unwrap().is_empty());
            assert!(selected("enclose_fill"));
            for source in ["selection_visible", "selection_editing", "selection_reference"] {
                let command = app.state()["tool_actions"].as_array().unwrap().iter()
                    .find(|item| item["command"] == source).unwrap()["command"].clone();
                app.action(json!({"type":"invoke","command":command}));
                assert!(selected(source));
                assert!(selected("enclose_fill"));
            }
            choose("groups", &lasso);
            assert!(selected("lasso_fill"));
            assert!(!app.state()["tool_actions"].as_array().unwrap().iter()
                .any(|action| ["selection_visible", "selection_editing", "selection_reference"].iter().any(|source| action["command"] == *source)));
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
    let prepared = App(unsafe { launch_json(0, launch.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut()) });
    assert!(!(prepared.0==0));
    for request in [
        json!({"type":"slider_layout","width":44,"height":176,"axis":"vertical"}),
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
