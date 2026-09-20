//! Deterministic assessment of the shipped 768-edge guide and 2 Hz refresh.
//! This measures mapping discontinuity; it is not a display flicker experiment.
use layer_core::color::{
    RgbSpace,
    hdr::{LocalToneGuide, SdrRendition},
};

fn main() {
    let extent = [1536, 128];
    let recipe = SdrRendition::default();
    let mapper = recipe.mapper(RgbSpace::Srgb, RgbSpace::Srgb);
    let mut held: Option<LocalToneGuide> = None;
    let mut max_stale_linear = 0f32;
    let mut max_refresh_linear = 0f32;
    let mut max_refresh_srgb_codes = 0f64;
    let mut reports = Vec::new();
    for frame in 0..60 {
        // Moving 6-stop light crossing a stationary background and fine texture.
        let left = 300 + frame * 12;
        let pixels: Vec<[f32; 4]> = (0..extent[1])
            .flat_map(|y| {
                (0..extent[0]).map(move |x| {
                    let base = 0.04 + (x as f32 / extent[0] as f32) * 0.3;
                    let fine = if (x + y) % 2 == 0 { 0.9 } else { 1.1 };
                    let v = if (left..left + 96).contains(&x) {
                        16.
                    } else {
                        base * fine
                    };
                    [v, v, v, 1.]
                })
            })
            .collect();
        let fresh = layer_color::build_local_tone_guide(
            extent,
            RgbSpace::Srgb,
            || false,
            |y, row| {
                let start = (y * extent[0]) as usize;
                row.copy_from_slice(&pixels[start..start + extent[0] as usize]);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(fresh.extent, [768, 64]);
        assert_eq!(fresh.samples.len() * 16, 786432);
        let prior = held.as_ref().unwrap_or(&fresh);
        let refresh = frame % 15 == 0;
        let mut stale = 0f32;
        let mut jump = 0f32;
        let mut codes = 0f64;
        // Compare the same current-frame pixel through old/new guides. Thus
        // motion of the light itself cannot be mistaken for guide flicker.
        for y in (0..extent[1]).step_by(4) {
            for x in (0..extent[0]).step_by(4) {
                let p = pixels[(y * extent[0] + x) as usize];
                let at = [x as f32 + 0.5, y as f32 + 0.5];
                let a = mapper.map_local_premultiplied(p, at, prior)[0];
                let b = mapper.map_local_premultiplied(p, at, &fresh)[0];
                stale = stale.max((a - b).abs());
                if refresh {
                    jump = jump.max((a - b).abs());
                    codes = codes.max(
                        (RgbSpace::Srgb.encode(a as f64) - RgbSpace::Srgb.encode(b as f64)).abs()
                            * 255.,
                    );
                }
            }
        }
        max_stale_linear = max_stale_linear.max(stale);
        max_refresh_linear = max_refresh_linear.max(jump);
        max_refresh_srgb_codes = max_refresh_srgb_codes.max(codes);
        reports.push(
            serde_json::json!({"frame": frame, "refresh": refresh, "stale_guide_max_linear": stale,
            "refresh_max_linear": jump, "refresh_max_srgb_codes": codes}),
        );
        if refresh {
            held = Some(fresh);
        }
    }
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "scope": "synthetic 30 fps, ideal 2 Hz publication; real worker delay can increase staleness",
        "extent": extent, "guide_extent": [768, 64], "guide_bytes": 786432,
        "maximum_guide_bytes": 768*768*16, "master_storage": "unchanged Float16",
        "frames": reports, "max_stale_linear": max_stale_linear,
        "max_refresh_linear": max_refresh_linear, "max_refresh_srgb_codes": max_refresh_srgb_codes,
        "flicker_free_claim": false
    })).unwrap());
}
