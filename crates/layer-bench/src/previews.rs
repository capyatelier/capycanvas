//! Offline swatches rendered by the real GPU presets. Both frontends ship the
//! same small PNGs; opening the brush selector does not run extra brush jobs.
use super::*;

pub fn generate(directory: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(directory)?;
    for theme in ["dark", "light"] {
        for choice in layer_ui::brush_catalog() {
            let mut canvas = Canvas::configured(LayerCanvasConfig {
                document_width: 400,
                document_height: 80,
                surface_width: 400,
                surface_height: 80,
                background_rgba_linear: [0.0; 4],
                ..LayerCanvasConfig::default()
            })?;
            canvas.set_feedback(false)?;
            // Destination-reading tools need existing color to demonstrate
            // their effect. These seeds are on the same editable paint layer.
            if choice.category == "Blend" || choice.category == "Shape" || choice.id == 3 {
                canvas.set_brush(brush(1, 44.0, 0.9, [0.06, 0.3, 0.65, 1.0]))?;
                stroke(&mut canvas, 0.0)?;
                canvas.set_brush(brush(1, 20.0, 0.9, [0.85, 0.35, 0.06, 1.0]))?;
                stroke(&mut canvas, 8.0)?;
            }
            let color = if theme == "dark" {
                [0.9, 0.9, 0.9, 1.0]
            } else {
                [0.012, 0.012, 0.014, 1.0]
            };
            canvas.set_brush(brush(choice.id, 42.0, 1.0, color))?;
            stroke(&mut canvas, -3.0)?;
            canvas.write_png(&directory.join(format!("{}-{theme}.png", choice.id)))?;
            eprintln!("swatch: {} ({theme})", choice.label);
        }
    }
    Ok(())
}

fn stroke(canvas: &mut Canvas, offset: f32) -> Result<(), String> {
    for i in 0..=120 {
        let t = i as f32 / 120.0;
        let sample = Sample {
            x: 26.0 + t * 348.0,
            y: 40.0 + (t * std::f32::consts::TAU).sin() * 10.0 + offset,
            pressure: 0.16 + 0.84 * (t * std::f32::consts::PI).sin().max(0.0).powf(0.65),
        };
        let event = canvas.next_event(
            sample,
            if i == 0 {
                1
            } else if i == 120 {
                3
            } else {
                2
            },
        );
        canvas.submit(&[event])?;
        if i % 8 == 0 || i == 120 {
            canvas.draw()?;
        }
    }
    canvas.wait_idle()
}
