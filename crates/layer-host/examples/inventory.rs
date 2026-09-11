//! Emit catalog, initial/settings views and representative dynamic layer menus.
//! These fixtures support parity review; they are not a completeness proof.
use layer_host::NativeHost;
use layer_ui::{Platform, SettingsPage, UiAction};
use serde_json::{Value, json};

fn layer_scenarios(platform: Platform) -> Vec<Value> {
    fn action(host: &mut NativeHost, action: Value) {
        host.dispatch(serde_json::from_value(json!({"type":"layer", "action":action})).unwrap())
            .unwrap();
    }
    fn capture(host: &NativeHost, name: &str) -> Value {
        let state = host.session.state();
        let menus: Vec<_> = state
            .layers
            .iter()
            .flat_map(|layer| {
                [false, true].into_iter().filter_map(move |mask| {
                    if mask && !layer.has_mask {
                        return None;
                    }
                    Some(json!({"id":layer.id,"mask":mask,
                    "menu":host.session.layer_menu(layer.id, mask).unwrap()}))
                })
            })
            .collect();
        json!({"name":name,"layers":state.layers,"tools":state.layer_tools,"menus":menus})
    }
    let mut host = NativeHost::new(platform).unwrap();
    let mut result = vec![capture(&host, "initial-paint-and-paper")];
    let original = host
        .session
        .state()
        .layer_tools
        .editing_layer
        .as_ref()
        .unwrap()
        .id;
    action(&mut host, json!({"op":"new","group":false,"clipped":false}));
    let ink = host
        .session
        .state()
        .layer_tools
        .editing_layer
        .as_ref()
        .unwrap()
        .id;
    action(&mut host, json!({"op":"toggle_selection","id":original}));
    result.push(capture(&host, "multiple-selected-layers"));
    action(&mut host, json!({"op":"reference_selection"}));
    action(&mut host, json!({"op":"clip","id":ink,"value":true}));
    action(&mut host, json!({"op":"add_mask","id":ink,"replace":false}));
    action(&mut host, json!({"op":"copy_mask","id":ink}));
    action(&mut host, json!({"op":"select","id":ink,"mask":true}));
    result.push(capture(
        &host,
        "mask-clipping-references-and-mask-clipboard",
    ));
    action(
        &mut host,
        json!({"op":"enable_mask","id":ink,"value":false}),
    );
    action(&mut host, json!({"op":"link_mask","id":ink,"value":false}));
    action(&mut host, json!({"op":"lock","id":ink,"value":true}));
    result.push(capture(&host, "locked-layer-with-disabled-unlinked-mask"));
    action(&mut host, json!({"op":"lock","id":ink,"value":false}));
    action(&mut host, json!({"op":"clip","id":ink,"value":false}));
    action(&mut host, json!({"op":"new","group":true,"clipped":false}));
    let group = host
        .session
        .state()
        .layer_tools
        .editing_layer
        .as_ref()
        .unwrap()
        .id;
    action(
        &mut host,
        json!({"op":"drop","id":original,"target":group,"fraction":0.5}),
    );
    result.push(capture(&host, "group-with-child"));
    action(&mut host, json!({"op":"collapse","id":group}));
    result.push(capture(&host, "collapsed-group"));
    result
}

fn menu_scenarios(platform: Platform) -> Vec<Value> {
    fn capture(host: &NativeHost, name: &str) -> Value {
        json!({"name":name,"menus":layer_ui::ApplicationMenu::ALL.map(|id|
            json!({"id":id,"model":host.session.application_menu(id)}))})
    }
    let mut host = NativeHost::new(platform).unwrap();
    let mut scenarios = vec![capture(&host, "initial")];
    host.dispatch(UiAction::Invoke { command: layer_ui::CommandId::SelectAll }).unwrap();
    scenarios.push(capture(&host, "pixel-selection"));
    let id = host.session.engine().document().active_layer.0;
    host.dispatch(serde_json::from_value(json!({"type":"layer","action":{"op":"lock","id":id,"value":true}})).unwrap()).unwrap();
    scenarios.push(capture(&host, "locked-target"));
    host.dispatch(UiAction::OpenSettings { page: SettingsPage::Shortcuts }).unwrap();
    host.dispatch(serde_json::from_value(json!({"type":"preferences","action":{"type":"edit_shortcut","id":"command.ZenMode"}})).unwrap()).unwrap();
    scenarios.push(json!({"name":"shortcut-editor","preferences":host.session.preferences()}));
    host.dispatch(serde_json::from_value(json!({"type":"preferences","action":{"type":"begin_shortcut","id":"command.ZenMode"}})).unwrap()).unwrap();
    host.input(serde_json::from_value(json!({"type":"key","key":"z","pressed":true,"modifiers":{"command":true,"shift":false,"alt":false}})).unwrap()).unwrap();
    scenarios.push(json!({"name":"shortcut-conflict","preferences":host.session.preferences()}));
    scenarios
}

fn main() {
    let mut host = NativeHost::new(Platform::Ios).unwrap();
    host.resize(2400, 1800, 2.0).unwrap();
    let initial = host.take_snapshot().unwrap();
    let mut preferences = Vec::new();
    for page in SettingsPage::ALL {
        host.dispatch(UiAction::OpenSettings { page }).unwrap();
        preferences.push(serde_json::to_value(host.session.preferences()).unwrap());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "catalog": layer_ui::ui_catalog(),
            "commands": layer_ui::CommandId::ALL.as_slice(),
            "initial": initial,
            "preferences": preferences,
            "menu_scenarios": { "ios": menu_scenarios(Platform::Ios), "mac": menu_scenarios(Platform::Mac) },
            "layer_scenarios": {
                "ios": layer_scenarios(Platform::Ios),
                "mac": layer_scenarios(Platform::Mac),
            },
        }))
        .unwrap()
    );
}
