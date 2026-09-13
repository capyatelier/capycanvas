//! Color-wheel display oracle: pick each normalized sample through shared Rust
//! policy. Used by native screenshot checks, not by the production raster path.
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
    let mut state = ColorState::default();
    state.set_rgba(request.rgba)?;
    state.apply(ColorAction::Space {
        space: request.space,
    })?;
    if let Some(shape) = request.shape {
        state.apply(ColorAction::Shape { shape })?;
    }
    if let Some(value) = request.hue {
        if request.shape.is_some() {
            let g = ColorWheelGeometry::new(1.).unwrap();
            state.apply(ColorAction::PickWheel {
                part: ColorWheelPart::Hue,
                point: g.hue_marker(value),
                size: 1.,
            })?;
        } else {
            state.apply(ColorAction::Component { index: 0, value })?;
        }
    }
    let geometry = ColorWheelGeometry::new(1.).unwrap();
    let samples: Vec<_> = request
        .points
        .into_iter()
        .map(|point| {
            let part = if let Some(shape) = request.shape {
                geometry.hit_shape(point, shape)
            } else {
                geometry.hit(point, request.space)
            };
            let mut sample = state.clone();
            if let Some(part) = part {
                // The hue ring always displays fully saturated, opaque colors.
                if part == ColorWheelPart::Hue {
                    let hue = if request.shape.is_some() {
                        state.wheel_hue_color(geometry.hue_at(point))
                    } else {
                        layer_ui::hue_color(geometry.hue_at(point))
                    };
                    return json!({"part":part,"rgba":[hue[0], hue[1], hue[2], 1.]});
                }
                sample
                    .apply(if request.shape.is_some() {
                        ColorAction::PickWheel {
                            part,
                            point,
                            size: 1.,
                        }
                    } else {
                        ColorAction::Pick {
                            part,
                            point,
                            size: 1.,
                        }
                    })
                    .unwrap();
            }
            json!({"part":part,"rgba":sample.rgba()})
        })
        .collect();
    let mut model = state.view();
    if request.shape.is_some() {
        model.field_marker = state.wheel_marker(&geometry);
        model.hue_marker = geometry.hue_marker(state.wheel_components()[0]);
    }
    let mut model = serde_json::to_value(model)?;
    model["wheel_hue_stops"] = serde_json::to_value(state.wheel_hue_stops())?;
    println!("{}", json!({"model":model,"samples":samples}));
    Ok(())
}
