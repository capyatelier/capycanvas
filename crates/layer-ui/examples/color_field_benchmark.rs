//! CPU-only compact color-field and hue-guide generation. Excludes host allocation/upload/presentation;
//! this does not establish an editor frame rate. Run in Release mode.
use std::{hint::black_box, time::Instant};
fn main() {
    for shape in [
        layer_ui::ColorShape::Circle,
        layer_ui::ColorShape::Square,
        layer_ui::ColorShape::Triangle,
    ] {
        for guide in [false, true] {
            // Representative physical raster sizes for compact Apple panels.
            for side in [184u32, 260, 396] {
                let mut pixels = vec![0; side as usize * side as usize * 4];
                let mut milliseconds = Vec::new();
                for step in 0..1100 {
                    let start = Instant::now();
                    let hue = black_box((step % 360) as f32);
                    let pixels = black_box(&mut pixels);
                    assert!(if guide {
                        layer_ui::render_hue_guide(side, shape, pixels)
                    } else {
                        match shape {
                            layer_ui::ColorShape::Circle => {
                                layer_ui::render_okhsv_disc(side, hue, pixels)
                            }
                            layer_ui::ColorShape::Square => {
                                layer_ui::render_hsv_field(side, hue, pixels)
                            }
                            layer_ui::ColorShape::Triangle => {
                                layer_ui::render_hls_field(side, hue, pixels)
                            }
                        }
                    });
                    let elapsed = start.elapsed().as_secs_f64() * 1000.;
                    black_box(&pixels);
                    if step >= 100 {
                        milliseconds.push(elapsed);
                    }
                }
                milliseconds.sort_by(f64::total_cmp);
                println!(
                    "shape={shape:?} guide={guide} side={side} bytes={} samples={} p50_ms={:.6} p95_ms={:.6} p99_ms={:.6} max_ms={:.6}",
                    pixels.len(),
                    milliseconds.len(),
                    milliseconds[499],
                    milliseconds[949],
                    milliseconds[989],
                    milliseconds[999]
                );
            }
        }
    }
}
