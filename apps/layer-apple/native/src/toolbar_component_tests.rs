use super::*;

#[test]
fn apple_toolbar_queries_match_the_shared_transport_without_a_session() {
    for request in [
        json!({"type":"slider_layout","width":44,"height":176,"axis":"vertical"}),
        json!({"type":"slider_spec","control":{"kind":"brush_size_slider"}}),
        json!({"type":"style","style":"medium"}),
        json!({"type":"slider_preview","control":{"kind":"brush_opacity_slider"},"style":"medium","value":0.5,"length":176,"extent":64}),
    ] {
        let expected = layer_ui::toolbar_ui(serde_json::from_value(request.clone()).unwrap()).unwrap();
        assert_eq!(stateless(capy_apple_toolbar_ui, request.to_string()), expected, "{request}");
    }
    assert!(stateless(capy_apple_toolbar_ui, "{").get("error").is_some());
    assert!(stateless(capy_apple_toolbar_ui, r#"{"type":"slider_spec","control":{"kind":"color"}}"#).get("error").is_some());
    let output = unsafe { capy_apple_toolbar_ui(std::ptr::null()) };
    let missing: Value = serde_json::from_slice(unsafe { CStr::from_ptr(output) }.to_bytes()).unwrap();
    unsafe { capy_apple_string_free(output) };
    assert_eq!(missing["error"], "Missing toolbar request");
}
