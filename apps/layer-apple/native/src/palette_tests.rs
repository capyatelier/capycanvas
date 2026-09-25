use super::*;
use std::io::{Read as _, Seek as _};
use std::os::fd::AsRawFd;

struct Published(std::cell::RefCell<Value>);
impl Published {
    fn new() -> Self { Self(std::cell::RefCell::new(Value::Null)) }
    fn panel(&self, app: &App) -> Value {
        let text = unsafe { capy_apple_request(app.0, 3, std::ptr::null()) };
        if !text.is_null() {
            *self.0.borrow_mut() = serde_json::from_slice(unsafe { CStr::from_ptr(text) }.to_bytes()).unwrap();
            unsafe { capy_apple_string_free(text) };
        }
        let panel = self.0.borrow()["palette_panel"].clone();
        assert!(panel.is_object(), "Palettes are published");
        panel
    }
}

fn palette_file(request: Value, bytes: &[u8], output: Option<&std::fs::File>) -> Value {
    let text = CString::new(request.to_string()).unwrap();
    let result = unsafe {
        capy_palette_file(text.as_ptr(), if bytes.is_empty() { std::ptr::null() } else { bytes.as_ptr() }, bytes.len(),
            output.map_or(-1, |file| file.as_raw_fd()))
    };
    let value = serde_json::from_slice(unsafe { CStr::from_ptr(result) }.to_bytes()).unwrap();
    unsafe { capy_apple_string_free(result) };
    value
}

fn ids(view: &Value) -> Vec<u64> {
    view["swatches"].as_array().unwrap().iter().map(|s| s["id"].as_u64().unwrap()).collect()
}

#[test]
fn apple_palettes_publish_starters_after_color_and_in_the_sketch_drawer() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let published = Published::new();
        let panel = published.panel(&app);
        assert_eq!(panel["palettes"].as_array().unwrap().len(), 10, "Apple installs the ten starters");
        let swatch = &panel["swatches"][0];
        assert_eq!(swatch["rgba"].as_array().unwrap().len(), 4);
        assert!(swatch["detail"].as_str().unwrap().contains(" · #"));
        let policy = unsafe { &*app.0 }.host.session.state().platform;
        for preset in [layer_ui::WorkspacePreset::Illustrator, layer_ui::WorkspacePreset::Photographer] {
            let layout = preset.layout(policy);
            for (panel, anchor) in [(layer_ui::Panel::Palettes, layer_ui::Panel::Color),
                (layer_ui::Panel::Proof, layer_ui::Panel::Navigator), (layer_ui::Panel::Stats, layer_ui::Panel::Brushes)] {
                let panels = layout.group_panels(layout.panel_group(anchor).unwrap()).unwrap();
                let index = panels.iter().position(|p| *p == anchor).unwrap();
                assert_eq!(panels[index + 1], panel, "{preset:?} places {panel:?} after {anchor:?}");
                assert_eq!(layout.active_panel(panel), Some(anchor));
            }
        }
        app.action(json!({"type":"restore_workspace","workspace":layer_ui::WorkspaceState {
            layout: layer_ui::WorkspacePreset::Painter.layout(policy), ..Default::default()
        }}));
        let header = unsafe { &*app.0 }.host.session.state().workspace.layout.header.clone();
        let id = header.entries().find(|entry| entry.item == layer_ui::HeaderItem::Tool { control: layer_ui::ToolbarControl::Color })
            .expect("Sketch has a color tool").id;
        app.action(json!({"type":"measure_header","height":60,"items":[{"id":id,"bounds":{"x":900,"y":0,"width":40,"height":60}}]}));
        app.action(json!({"type":"activate_header_item","id":id}));
        let drawer = &app.state()["customization"]["drawer"];
        assert_eq!(drawer["columns"], json!([["color", "palettes"]]), "Palettes sit below the Sketch wheel");
    }
}

#[test]
fn apple_palette_queries_preview_validate_commit_and_undo_one_reorder() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let published = Published::new();
        let view = |app: &App| published.panel(app);
        let panel = view(&app);
        let palette = panel["palette"].as_u64().unwrap();
        let before = ids(&panel);
        let preview = app.request(2, json!({"type":"palette_reorder_preview","palette":palette,"id":before[0],"slot":2})).unwrap();
        assert_eq!(preview["order"][2], before[0]);
        assert_eq!(ids(&view(&app)), before, "previews never edit");
        let duplicate = json!({"op":"rename","id":before[0],"name":panel["swatches"][1]["name"]});
        let error = app.request(2, json!({"type":"palette_action","action":duplicate,"dry_run":true})).unwrap();
        assert!(error["error"].as_str().unwrap().contains("already"), "{error}");
        app.action(json!({"type":"color","action":{"op":"library","action":preview["action"]}}));
        let moved = view(&app);
        assert_eq!(moved["swatches"][2]["id"], before[0]);
        assert_eq!(moved["can_undo"], true);
        let menu = app.request(2, json!({"type":"palette_menu","target":{"kind":"color","id":before[0]}})).unwrap();
        let labels: Vec<_> = menu.as_array().unwrap().iter().flat_map(|s| s.as_array().unwrap().iter()
            .map(|i| (i["label"].as_str().unwrap().to_owned(), i["enabled"].as_bool().unwrap()))).collect();
        assert_eq!(labels, [("Rename Color…".into(), true), ("Remove Color".into(), true),
            ("Undo Color Reorder".into(), true), ("Redo Color Reorder".into(), false)]);
        app.action(json!({"type":"color","action":{"op":"library","action":{"op":"undo_reorder","palette":palette}}}));
        assert_eq!(ids(&view(&app)), before, "one undo restores the order");
        let library = app.request(2, json!({"type":"palette_menu","target":{"kind":"library"}})).unwrap();
        assert_eq!(library[0][1]["command"]["command"], "import_palette");
        let menu = app.request(2, json!({"type":"palette_menu","target":{"kind":"palette","id":palette}})).unwrap();
        assert_eq!(menu[0][1]["sections"][1][0]["command"], json!({"command":"export_palette","id":palette,"format":"aco"}));
    }
}

#[test]
fn apple_palette_reveal_selects_the_tab_in_paint() {
    let app = App::new(0);
    let policy = unsafe { &*app.0 }.host.session.state().platform;
    app.action(json!({"type":"restore_workspace","workspace":layer_ui::WorkspaceState {
        layout: layer_ui::WorkspacePreset::Illustrator.layout(policy), ..Default::default()
    }}));
    let active = |app: &App| unsafe { &*app.0 }.host.session.state().workspace.layout.active_panel(layer_ui::Panel::Palettes);
    assert_eq!(active(&app), Some(layer_ui::Panel::Color));
    app.request(2, json!({"type":"reveal_panel","panel":"palettes"}));
    assert_eq!(active(&app), Some(layer_ui::Panel::Palettes));
}

#[test]
fn apple_palette_file_codec_round_trips_every_format_and_rejects_damage() {
    let limits = palette_file(json!({"type":"limits"}), &[], None);
    assert_eq!(limits["read_bytes"], 1024 * 1024 + 1);
    assert!(limits["extensions"].as_array().unwrap().contains(&json!("aco")));
    let app = App::new(1);
    let library = &app.state()["colors"]["library"];
    let palette = library["palettes"].as_array().unwrap().iter().find(|p| p["swatches"].as_array().unwrap().len() > 3).unwrap().clone();
    let count = palette["swatches"].as_array().unwrap().len();
    for format in ["capycolor", "aco", "swatches", "ase", "gpl"] {
        let dry = palette_file(json!({"type":"export","palette":palette,"format":format}), &[], None);
        assert!(dry["file_name"].as_str().unwrap().ends_with(&format!(".{format}")), "{dry}");
        let mut file = tempfile();
        let metadata = palette_file(json!({"type":"export","palette":palette,"format":format}), &[], Some(&file));
        assert_eq!(metadata["file_name"], dry["file_name"]);
        let mut bytes = Vec::new();
        file.rewind().unwrap();
        file.read_to_end(&mut bytes).unwrap();
        assert!(!bytes.is_empty());
        let imported = palette_file(json!({"type":"import","file_name":metadata["file_name"]}), &bytes, None);
        let action = &imported["action"];
        assert_eq!(action["op"], "import");
        assert_eq!(action["swatches"].as_array().unwrap().len(), count.min(30), "{format}");
        let before = app.state()["colors"]["library"]["palettes"].as_array().unwrap().len();
        app.action(json!({"type":"color","action":{"op":"library","action":action}}));
        assert_eq!(app.state()["colors"]["library"]["palettes"].as_array().unwrap().len(), before + 1);
    }
    let damaged = palette_file(json!({"type":"import","file_name":"Broken.aco"}), &[0, 1, 0, 9, 0], None);
    assert!(damaged["error"].is_string(), "{damaged}");
    let oversized = vec![b' '; 1024 * 1024 + 2];
    assert!(palette_file(json!({"type":"import","file_name":"Huge.gpl"}), &oversized, None)["error"].is_string());
}

#[test]
fn apple_automatic_tab_names_use_the_stateless_toolbar_query() {
    let request = CString::new(json!({"type":"automatic_tab_names","available":192.,"widths":[[120.,36.],[90.,36.],[140.,36.]]}).to_string()).unwrap();
    let result = unsafe { capy_apple_toolbar_ui(request.as_ptr()) };
    let value: Value = serde_json::from_slice(unsafe { CStr::from_ptr(result) }.to_bytes()).unwrap();
    unsafe { capy_apple_string_free(result) };
    assert_eq!(value, json!([true, false, false]));
}

fn tempfile() -> std::fs::File {
    let path = std::env::temp_dir().join(format!("capy-palette-{}-{:?}", std::process::id(), std::thread::current().id()));
    let file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    file
}
