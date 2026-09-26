//! Color-wheel display oracle: pick each normalized sample through shared Rust
//! policy. Used by native screenshot checks, not by the production raster path.
use layer_core::color::RgbSpace;
use layer_ui::{
    ColorAction, ColorShape, ColorSpace, ColorState, ColorWheelGeometry, ColorWheelPart,
};
use serde::Deserialize;
use serde_json::json;
use std::io::Read;

#[derive(Deserialize)]
struct Request {
    space: ColorSpace,
    #[serde(default)]
    shape: Option<ColorShape>,
    rgba: [f32; 4],
    /// Achromatic paint retains a hue that cannot be recovered from RGBA alone.
    #[serde(default)]
    hue: Option<f32>,
    points: Vec<[f32; 2]>,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut source = String::new();
    std::io::stdin().read_to_string(&mut source)?;
    let request: Request = serde_json::from_str(&source)?;
    let shape = request.shape.unwrap_or(match request.space {
        ColorSpace::Hsv => ColorShape::Square,
        ColorSpace::Hls => ColorShape::Triangle,
    });
    let mut state = ColorState::default();
    state.set_rgba(request.rgba)?;
    state.apply(ColorAction::Shape { shape })?;
    let geometry = ColorWheelGeometry::new(1.).unwrap();
    if let Some(value) = request.hue {
        state.apply(ColorAction::PickWheel {
            part: ColorWheelPart::Hue,
            point: state.wheel_hue_marker(&geometry, value),
            size: 1.,
        })?;
    }
    let samples: Vec<_> = request
        .points
        .into_iter()
        .map(|point| {
            let part = geometry.hit_shape(point, shape);
            let mut sample = state.clone();
            if let Some(part) = part {
                // The hue ring displays its opaque guide, independently of the paint.
                if part == ColorWheelPart::Hue {
                    let hue = state
                        .wheel_hue_color_in(state.wheel_hue_at(&geometry, point), RgbSpace::Srgb);
                    return json!({"part":part,"rgba":[hue[0], hue[1], hue[2], 1.]});
                }
                sample
                    .apply(ColorAction::PickWheel {
                        part,
                        point,
                        size: 1.,
                    })
                    .unwrap();
            }
            json!({"part":part,"rgba":sample.rgba()})
        })
        .collect();
    let mut model = serde_json::to_value(state.view())?;
    model["wheel_hue_stops"] = serde_json::to_value(state.wheel_hue_stops())?;
    println!("{}", json!({"model":model,"samples":samples}));
    Ok(())
}
