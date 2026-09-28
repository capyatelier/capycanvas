//! The shared canvas action bar through the Apple action, query and pointer ABI.
use super::*;
use super::fixtures::{selection_app, surface};

fn contact(app: &App, point: [f64; 2], phase: f64) {
    let [x, y] = surface(app, point);
    let records = [x, y, 1., 0., 0., 0., 0., 1_000_000_000. + phase * 1_000_000., phase];
    assert_eq!(
        unsafe { capy_apple_pointer(app.0, 7, 0, 0, records.as_ptr(), records.len(), 0, capy_apple_camera_revision(app.0)) },
        0
    );
}

fn layout(app: &App, bar: &Value) -> Value {
    let widths = |key: &str| vec![40.0; bar[key].as_array().unwrap().len()];
    app.request(2, json!({"type": "canvas_bar_layout", "measure": {"context": bar["context"], "label": 0.0,
        "items": widths("items"), "completion": widths("completion"), "more": 40.0, "height": 52.0, "gap": 4.0, "padding": 6.0}}))
        .unwrap()
}

fn edit(app: &App, bar: &Value, command: &str) -> Result<Option<Value>, String> {
    app.try_request(0, &json!({"type": "canvas_bar_edit", "context": bar["context"], "action": {"type": "invoke", "command": command}}))
}

#[test]
fn canvas_bar_places_edits_and_hides_during_contacts_through_the_apple_abi() {
    for platform in [0, 1] {
        let app = selection_app(platform);
        app.invoke("move");
        app.invoke("select_all");
        let selection = app.state()["canvas_bar"].clone();
        assert_eq!(selection["context"]["kind"], "selection", "{platform}: {selection}");
        let placed = layout(&app, &selection);
        assert!(placed["bounds"]["width"].as_f64().unwrap() > 0., "{platform}: {placed}");
        let menu = app.request(2, json!({"type": "canvas_bar_menu", "context": selection["context"], "shown": placed["items"]})).unwrap();
        let toggle = menu["sections"].as_array().unwrap().last().unwrap()[0].clone();
        assert_eq!(toggle["action"]["command"], "show_canvas_action_bar", "{platform}: {menu}");

        let transform = selection["items"].as_array().unwrap().iter()
            .map(|item| &item["option"]["Action"]["state"]).find(|state| state["id"] == "scale_rotate").unwrap();
        assert!(transform["disabled_reason"].as_str().is_some_and(|reason| !reason.is_empty()),
            "an empty layer explains Transform: {transform}");
        let menus: Vec<_> = selection["items"].as_array().unwrap().iter().filter_map(|item| item["menu"].as_str()).collect();
        for id in &menus {
            let menu = app.request(2, json!({"type": "canvas_bar_choice_menu", "context": selection["context"], "id": id})).unwrap();
            assert!(!menu["sections"].as_array().unwrap().is_empty(), "{platform}: the {id} menu lists actions: {menu}");
        }
        assert!(menus.contains(&"adjust") && menus.contains(&"refine"), "{platform}: selection menus {menus:?}");
        edit(&app, &selection, "fill_selection").unwrap();
        app.draw_until_idle();
        let selection = app.state()["canvas_bar"].clone();
        edit(&app, &selection, "scale_rotate").unwrap();
        let transform = app.state()["canvas_bar"].clone();
        assert_eq!(transform["context"]["kind"], "transform", "{platform}: {transform}");
        assert_eq!(transform["placement"], "near_object");
        assert!(edit(&app, &selection, "invert_selection").unwrap_err().contains("previous"), "stale bar edits are refused");
        assert!(!layout(&app, &transform).is_null());

        let hold = || unsafe { capy_apple_canvas_bar_hold(app.0) };
        let before = hold();
        assert_eq!(before % 2, 0);
        contact(&app, [32., 32.], 1.);
        assert_eq!(hold() % 2, 1, "a canvas contact hides a bar beside the object");
        contact(&app, [36., 34.], 3.);
        assert_eq!(hold() % 2, 0, "the bar may return when the contact ends");

        app.invoke("show_canvas_action_bar");
        let completion = app.state()["canvas_bar"].clone();
        assert!(completion["items"].as_array().unwrap().is_empty(), "{platform}: {completion}");
        assert_eq!(completion["placement"], "bottom_edge");
        edit(&app, &completion, "cancel_transform").unwrap();
        assert!(app.state()["canvas_bar"].is_null(), "no selection bar while the toggle is off");
        app.invoke("show_canvas_action_bar");
        assert_eq!(app.state()["canvas_bar"]["context"]["kind"], "selection");
    }
}
