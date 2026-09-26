use super::*;
use super::fixtures::tempfile;
use std::io::{Read as _, Seek as _};
use std::os::fd::AsRawFd;

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

#[test]
fn apple_palettes_publish_starters_and_sit_in_the_sketch_drawer() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let panel = app.full_snapshot()["palette_panel"].clone();
        assert_eq!(panel["palettes"].as_array().unwrap().len(), 10, "Apple installs the ten starters");
        let swatch = &panel["swatches"][0];
        assert_eq!(swatch["rgba"].as_array().unwrap().len(), 4);
        assert!(swatch["detail"].as_str().unwrap().contains(" · #"));
        let policy = unsafe { &*app.0 }.host.session.state().platform;
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
