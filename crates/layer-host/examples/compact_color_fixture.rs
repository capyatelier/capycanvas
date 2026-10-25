use layer_core::color::{RgbColor, RgbSpace, SampleDepth};
use layer_host::Renderer;
use layer_ui::{ColorAction, ColorShape, ColorUiRequest, Platform, Settings, Theme, UiAction, UiSession};
use serde_json::json;
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let output = PathBuf::from(args.next().ok_or("Expected output directory")?);
    let scale: f32 = args.next().ok_or("Expected native rasterization scale")?.parse()?;
    if !scale.is_finite() || !(1.0..=2.0).contains(&scale) { return Err("Scale must be 1..2".into()); }
    fs::create_dir_all(&output)?;
    let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
    let mut fixtures = Vec::new();
    for (theme_name, theme) in [("dark", Theme::Dark), ("light", Theme::Light)] {
        for size in [128_u32, 160, 226, 360] {
            let name = format!("{theme_name}-{size}");
            let mut items = Vec::new();
            for (row, rgb) in [false, true].into_iter().enumerate() {
                for (column, shape) in [ColorShape::Circle, ColorShape::Square, ColorShape::Triangle].into_iter().enumerate() {
                    let mut session = UiSession::blank_localized(Renderer::default(), [1, 1], Platform::Windows, localization.clone())?;
                    assert!(session.engine().backend().0.is_none());
                    for action in [ColorAction::Definition { color: RgbColor::new(RgbSpace::Srgb, [0.24, 0.56, 0.87, 0.65])? }, ColorAction::Shape { shape }] {
                        session.dispatch(UiAction::Color { action })?;
                    }
                    if rgb { session.dispatch(UiAction::Color { action: ColorAction::ToggleReadout })?; }
                    let state = serde_json::to_value(session.state())?;
                    let preview = serde_json::to_value(session.color_preview())?;
                    let model = &preview["view"];
                    assert_eq!(session.state().colors.definition(), RgbColor::new(RgbSpace::Srgb, [0.24, 0.56, 0.87, 0.65])?);
                    assert_eq!(model["shape"], serde_json::to_value(shape)?);
                    assert_eq!(model["readout"], if rgb { "rgb" } else { "shape" });
                    assert_eq!(state["colors"]["slot"], "foreground");
                    assert_eq!(session.engine().document().composition().color.depth, SampleDepth::U8);
                    assert_eq!(model["hdr"], false);
                    assert_eq!(model["document_depth"], "U8");
                    assert!(preview["picker"]["preview"].is_null());
                    assert_eq!(preview["colors"], state["colors"]);
                    assert!(state["layer_tools"]["mask_editing"].is_null());
                    let layout_request = json!({"type":"layout","size":size,"hdr":false});
                    let layout = layer_ui::color_ui_localized(serde_json::from_value::<ColorUiRequest>(layout_request.clone())?, session.localization())?;
                    let minimum_request = json!({"type":"layout","size":128,"hdr":false});
                    let minimum = layer_ui::color_ui_localized(serde_json::from_value::<ColorUiRequest>(minimum_request.clone())?, session.localization())?;
                    assert_eq!(layout["height"].as_f64(), Some(f64::from(size)));
                    assert_eq!(minimum["height"].as_f64(), Some(128.));
                    let hue_request = json!({"type":"hue_stops","shape":shape,"space":model["rgb_space"]});
                    let hue_stops = layer_ui::color_ui_localized(serde_json::from_value::<ColorUiRequest>(hue_request.clone())?, session.localization())?;
                    let field_side = (layout["wheel"][2].as_f64().unwrap() as f32 * if shape == ColorShape::Circle { 1. } else { scale }).ceil() as u32;
                    let colors = session.state().preview_colors();
                    let metadata = serde_json::to_string(&json!({"side":field_side,"colors":colors,
                        "rendition":None::<layer_core::color::hdr::SdrRendition>}))?;
                    let mut pixels = vec![0; field_side as usize * field_side as usize * 4];
                    assert!(colors.render_field(field_side, &mut pixels));
                    assert_eq!(pixels.len(), field_side as usize * field_side as usize * 4);
                    let field_file = format!("{name}-{row}-{column}.rgba");
                    fs::write(output.join(&field_file), pixels)?;
                    items.push(json!({"key":format!("{row}-{column}"),"x":8+column as u32*(size+8),"y":8+row as u32*(size+8),
                        "size":size,"model":model,"colors":state["colors"],"layout":layout,"hue_stops":hue_stops,
                        "state":{"colors":state["colors"],"layer_tools":state["layer_tools"]},"color_preview":preview,
                        "color_ui":[{"request":minimum_request,"response":minimum},{"request":layout_request,"response":layout},{"request":hue_request,"response":hue_stops}],
                        "field_side":field_side,"field_file":field_file,"field_request":metadata,"field_request_kind":"fixed_sdr_fixture_input",
                        "query_context":{"document_color":session.engine().document().composition().color,"platform":Platform::Windows,"language":session.localization().language()}}));
                }
            }
            let fixture = json!({"schema":2,"name":name,"width":3*(size+8)+8,"height":2*(size+8)+8,"scale":scale,
                "theme":theme_name,"palette":Settings::default().palette(theme,Platform::Windows,None),"catalog":layer_ui::ui_catalog_localized(&localization),"items":items});
            fs::write(output.join(format!("{name}.json")), serde_json::to_vec_pretty(&fixture)?)?;
            fixtures.push(fixture);
        }
    }
    fs::write(output.join("manifest.json"), serde_json::to_vec(&json!({"schema":2,"fixtures":fixtures}))?)?;
    Ok(())
}
