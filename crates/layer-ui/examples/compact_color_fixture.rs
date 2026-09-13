//! Production compact Color models and CPU fields for matched host captures.
//! Usage: cargo run -p layer-ui --example compact_color_fixture -- OUTPUT SCALE
use layer_ui::{ColorAction, ColorPanelLayout, ColorShape, ColorState, Platform, Settings, Theme};
use serde_json::json;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let output = PathBuf::from(args.next().ok_or("Expected output directory")?);
    let scale: f32 = args
        .next()
        .ok_or("Expected native rasterization scale")?
        .parse()?;
    if !scale.is_finite() || !(1.0..=2.0).contains(&scale) {
        return Err("Scale must be 1..2".into());
    }
    fs::create_dir_all(&output)?;
    let mut fixtures = Vec::new();
    for (theme_name, theme) in [("dark", Theme::Dark), ("light", Theme::Light)] {
        for size in [128_u32, 160, 226, 360] {
            let name = format!("{theme_name}-{size}");
            let layout = ColorPanelLayout::new(size as f32).unwrap();
            let mut items = Vec::new();
            for (row, rgb) in [false, true].into_iter().enumerate() {
                for (column, shape) in
                    [ColorShape::Circle, ColorShape::Square, ColorShape::Triangle]
                        .into_iter()
                        .enumerate()
                {
                    let mut color = ColorState::default();
                    color.set_rgba([0.24, 0.56, 0.87, 0.65])?;
                    color.apply(ColorAction::Shape { shape })?;
                    if rgb {
                        color.apply(ColorAction::ToggleReadout)?;
                    }
                    let model = color.view();
                    let field_side = (layout.wheel[2]
                        * if shape == ColorShape::Circle {
                            1.
                        } else {
                            scale
                        })
                    .ceil() as u32;
                    let field_file = format!("{name}-{row}-{column}.rgba");
                    if shape != ColorShape::Square {
                        let mut pixels = vec![0; (field_side * field_side * 4) as usize];
                        let render = if shape == ColorShape::Circle {
                            layer_ui::render_okhsv_disc
                        } else {
                            layer_ui::render_hls_field
                        };
                        assert!(render(field_side, model.wheel_components[0], &mut pixels));
                        fs::write(output.join(&field_file), pixels)?;
                    }
                    items.push(json!({"key":format!("{row}-{column}"),"x":8+column as u32*(size+8),"y":8+row as u32*(size+8),
                        "size":size,"model":model,"colors":color,"layout":layout,"hue_stops":color.wheel_hue_stops(),
                        "field_side":field_side,"field_file":if shape==ColorShape::Square {None}else{Some(field_file)}}));
                }
            }
            let fixture = json!({"schema":2,"name":name,"width":3*(size+8)+8,"height":2*(size+8)+8,"scale":scale,
                "theme":theme_name,"palette":Settings::default().palette(theme,Platform::Windows),"catalog":layer_ui::ui_catalog(),"items":items});
            fs::write(
                output.join(format!("{name}.json")),
                serde_json::to_vec_pretty(&fixture)?,
            )?;
            fixtures.push(fixture);
        }
    }
    fs::write(
        output.join("manifest.json"),
        serde_json::to_vec(&json!({"schema":2,"fixtures":fixtures}))?,
    )?;
    Ok(())
}
