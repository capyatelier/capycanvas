//! Color-wheel display oracle: pick each normalized sample through shared Rust
//! policy. Used by native screenshot checks, not by the production raster path.
use layer_ui::{ColorAction, ColorSpace, ColorState, ColorWheelGeometry, ColorWheelPart};
use serde::Deserialize;
use serde_json::json;
use std::io::Read;

#[derive(Deserialize)]
struct Request {
    space: ColorSpace,
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
    if let Some(value) = request.hue {
        state.apply(ColorAction::Component { index: 0, value })?;
    }
    let geometry = ColorWheelGeometry::new(1.).unwrap();
    let samples: Vec<_> = request
        .points
        .into_iter()
        .map(|point| {
            let part = geometry.hit(point, request.space);
            let mut sample = state.clone();
            if let Some(part) = part {
                // The hue ring always displays fully saturated, opaque colors.
                if part == ColorWheelPart::Hue {
                    sample.set_rgba([1., 0., 0., 1.]).unwrap();
                }
                sample
                    .apply(ColorAction::Pick {
                        part,
                        point,
                        size: 1.,
                    })
                    .unwrap();
            }
            json!({"part":part,"rgba":sample.rgba()})
        })
        .collect();
    println!("{}", json!({"model":state.view(),"samples":samples}));
    Ok(())
}
