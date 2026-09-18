//! Reproducible linear samples for the SDR highlight review charts.
//! cargo run --release -p layer-core --example sdr_highlights > samples.json
use layer_core::color::{RgbSpace, hdr::SdrRendition};

fn main() {
    let colors = [
        ("Red", [1., 0., 0.]), ("Green", [0., 1., 0.]),
        ("Blue", [0., 0., 1.]), ("Orange", [1., 0.15, 0.015]),
        ("Skin", [0.65, 0.32, 0.2]), ("Gray", [0.18; 3]),
    ];
    let recipes = [
        ("Previous", SdrRendition::legacy_default()),
        ("Photographic · White", SdrRendition::default()),
        ("Photographic · 50%", SdrRendition {highlight_color:0.5,..Default::default()}),
        ("Photographic · Color", SdrRendition {highlight_color:1.,..Default::default()}),
    ];
    let mut result = Vec::new();
    for (name, color) in colors {
        for (method, recipe) in recipes {
            let samples = (0..257).map(|i| {
                let ev = -4. + i as f32 * 12. / 256.;
                let input = color.map(|c| c * ev.exp2());
                let output = recipe.map_rgb(input, RgbSpace::Srgb);
                serde_json::json!({"ev":ev,"input":input,"linear":output,"display":output.map(|c|RgbSpace::Srgb.encode(f64::from(c)))})
            }).collect::<Vec<_>>();
            result.push(serde_json::json!({"color":name,"method":method,"recipe":recipe,"samples":samples}));
        }
    }
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
}
