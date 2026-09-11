//! Deterministic shared toolbar views for direct native/browser component captures.
//! This fixture does not establish whole-editor layout or physical-input parity.
use layer_host::NativeHost;
use layer_ui::{
    CommandId, CustomizationAction, Panel, PanelContent, Platform, Theme, TileStyle,
    ToolbarControl, UiAction, WorkspaceState,
};
use serde_json::json;

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let platform = match args.first().map(String::as_str).unwrap_or("mac") {
        "mac" => Platform::Mac,
        "ios" => Platform::Ios,
        "web" => Platform::Web,
        _ => panic!("Expected mac, ios or web"),
    };
    let theme = match args.get(1).map(String::as_str).unwrap_or("light") {
        "light" => Theme::Light,
        "dark" => Theme::Dark,
        _ => panic!("Expected light or dark"),
    };
    let mut host = NativeHost::new(platform).unwrap();
    let mut workspace = WorkspaceState::for_platform(platform);
    let controls = [
        ToolbarControl::Command {
            command: CommandId::Pen,
        },
        ToolbarControl::Command {
            command: CommandId::Undo,
        },
        ToolbarControl::Brush { id: 1 },
        ToolbarControl::Color,
        ToolbarControl::Opacity,
        ToolbarControl::Size { pixels: 64 },
        ToolbarControl::Command {
            command: CommandId::KeyboardShortcuts,
        },
    ];
    let original_count = workspace
        .layout
        .panel(Panel::Commands)
        .unwrap()
        .tiles()
        .len();
    workspace
        .layout
        .insert_tools(Panel::Commands, None, &controls)
        .unwrap();
    let panel = workspace
        .layout
        .panels
        .iter_mut()
        .find(|p| p.id == Panel::Commands)
        .unwrap();
    let PanelContent::Toolbar { tiles, .. } = &mut panel.content else {
        unreachable!()
    };
    tiles.drain(..original_count);
    host.dispatch(UiAction::RestoreWorkspace { workspace })
        .unwrap();
    host.dispatch(UiAction::SetTheme { theme: Some(theme) })
        .unwrap();
    host.dispatch(
        serde_json::from_value(json!({"type":"set_color","rgba":[0.2,0.5,0.8,1]})).unwrap(),
    )
    .unwrap();
    let mut rows = Vec::new();
    for style in [
        TileStyle::Small,
        TileStyle::Medium,
        TileStyle::Large,
        TileStyle::MediumLabeled,
        TileStyle::Labeled,
    ] {
        host.dispatch(UiAction::Customize {
            action: CustomizationAction::SetTileStyle {
                panel: Panel::Commands,
                style,
            },
        })
        .unwrap();
        rows.push(
            json!({"size":style.size(),"panel":host.session.panel_view(Panel::Commands).unwrap()}),
        );
    }
    let snapshot = host.take_snapshot().unwrap();
    println!("{}", serde_json::to_string_pretty(&json!({"schema":1,"width":780,"height":324,
        "theme":theme,"palette":snapshot["state"]["palette"],"color":snapshot["state"]["brush"]["color"],
        "text_size":f32::from(layer_ui::ui_catalog().text_size_pt) * 4.0 / 3.0,"rows":rows})).unwrap());
}
