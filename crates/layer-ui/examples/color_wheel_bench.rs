//! CPU raster cost only, excluding host UI work and presentation.
//! Run with: cargo run --locked --release -p layer-ui --example color_wheel_bench
use std::{hint::black_box, time::Instant};

fn main() {
    let mut results = Vec::new();
    for side in [108, 198, 236, 312, 347, 636] {
        let mut pixels = vec![0; side as usize * side as usize * 4];
        let mut milliseconds = Vec::new();
        for i in 0..128 {
            let start = Instant::now();
            assert!(layer_ui::render_okhsv_disc(
                side,
                black_box(i as f32 * 2.9),
                black_box(&mut pixels),
            ));
            black_box(&pixels);
            if i >= 8 {
                milliseconds.push(start.elapsed().as_secs_f64() * 1000.);
            }
        }
        milliseconds.sort_by(f64::total_cmp);
        results.push(serde_json::json!({
            "side": side, "samples": milliseconds.len(),
            "median_ms": milliseconds[60], "p95_ms": milliseconds[114],
            "maximum_ms": milliseconds.last().unwrap(),
        }));
    }
    println!("{}", serde_json::to_string_pretty(&results).unwrap());
}
