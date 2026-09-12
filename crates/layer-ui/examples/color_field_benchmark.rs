//! CPU-only HLS field generation. Excludes host allocation/upload/presentation;
//! this does not establish an editor frame rate. Run in Release mode.
use std::{hint::black_box, time::Instant};
fn main() {
    for side in [320, 452, 768] {
        let mut pixels = vec![0; side as usize * side as usize * 4];
        let mut milliseconds = Vec::new();
        for step in 0..1100 {
            let start = Instant::now();
            assert!(layer_ui::render_hls_field(
                side,
                black_box((step % 360) as f32),
                black_box(&mut pixels)
            ));
            let elapsed = start.elapsed().as_secs_f64() * 1000.;
            black_box(&pixels);
            if step >= 100 {
                milliseconds.push(elapsed);
            }
        }
        milliseconds.sort_by(f64::total_cmp);
        println!(
            "side={side} bytes={} samples={} p50_ms={:.6} p95_ms={:.6} p99_ms={:.6} max_ms={:.6}",
            pixels.len(),
            milliseconds.len(),
            milliseconds[499],
            milliseconds[949],
            milliseconds[989],
            milliseconds[999]
        );
    }
}
