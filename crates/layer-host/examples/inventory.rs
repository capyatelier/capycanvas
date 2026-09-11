//! Emit an authoritative UI inventory and initial state for host parity tools.
use layer_host::NativeHost;
use layer_ui::{Platform, SettingsPage, UiAction};
use serde_json::json;
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
        }))
        .unwrap()
    );
}
