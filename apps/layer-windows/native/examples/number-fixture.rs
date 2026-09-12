//! Synthetic review sheet using production Windows models and numeric policy.
//! No GPU or user storage; the WinUI fixture supplies actual geometry and pixels.
use layer_host::NativeHost;
use layer_ui::{CommandId, NumericControl, NumericOperation, Platform, UiAction};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

fn row(label: &str, control: &NumericControl, value: f64, enabled: bool) -> Value {
    json!({
        "label": label, "control": control, "value": value, "enabled": enabled,
        "formatted": control.resolve(value, NumericOperation::Format).unwrap(),
        "minimum": control.resolve(control.min, NumericOperation::Format).unwrap()
    })
}

fn main() {
    let output = PathBuf::from(std::env::args_os().nth(1).expect("Output directory"));
    fs::create_dir_all(&output).unwrap();
    let catalog = serde_json::to_value(layer_ui::ui_catalog()).unwrap();
    let opacity: NumericControl = serde_json::from_value(catalog["opacity"].clone()).unwrap();
    let size: NumericControl = serde_json::from_value(catalog["brush_size"].clone()).unwrap();
    let mut host = NativeHost::new(Platform::Windows).unwrap();
    host.dispatch(UiAction::Invoke {
        command: CommandId::AutoSelect,
    })
    .unwrap();
    let state = serde_json::to_value(host.session.state()).unwrap();
    let gap = state["tool_settings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["id"] == "gap_closing")
        .expect("Auto select gap control");
    let spin: NumericControl = serde_json::from_value(gap["numeric"].clone()).unwrap();
    let title = gap["label"].as_str().unwrap();
    let rows = vec![
        row("Brush opacity", &opacity, opacity.min, true),
        row("Brush opacity", &opacity, 0.5, true),
        row("Brush opacity", &opacity, opacity.max, true),
        row("Brush size", &size, 24., true),
        row("Long brush diameter setting label", &size, size.max, true),
        row(title, &spin, spin.min, true),
        row(title, &spin, 12., true),
        row(title, &spin, spin.max, true),
        row(title, &spin, 12., false),
        row("Brush opacity", &opacity, 0.5, false),
    ];
    for theme in ["light", "dark"] {
        host.dispatch(serde_json::from_value(json!({"type":"set_theme","theme":theme})).unwrap())
            .unwrap();
        let state = serde_json::to_value(host.session.state()).unwrap();
        let fixture = json!({
            "schema":1, "name":format!("windows-{theme}"), "width":734, "height":652,
            "require_geometry_parity":true,
            "column_widths":[160,226,320], "theme":theme, "palette":state["palette"],
            "text_size":catalog["text_size_pt"].as_f64().unwrap()*4./3.,
            "catalog":catalog, "rows":rows,
            "scope":"Synthetic shared numeric models for production WinUI control captures; no user storage or GPU painting."
        });
        fs::write(
            output.join(format!("fixture-windows-{theme}.json")),
            serde_json::to_vec_pretty(&fixture).unwrap(),
        )
        .unwrap();
    }
}
