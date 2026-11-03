use layer_core::{Point, StrokePoint};

pub fn points(speed: f32, seconds: f32, jitter: f32, pressure_noise: f32) -> Vec<StrokePoint> {
    let mut seed = 0x1234_5678u32;
    let mut random = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed >> 8) as f32 / 16777216. * 2. - 1.
    };
    let (mut x, mut y) = (0., 0.);
    (0..=(seconds / 0.004) as u32)
        .map(|i| {
            x = 0.9 * x + jitter * random();
            y = 0.9 * y + jitter * random();
            StrokePoint {
                position: Point {
                    x: 100. + speed * (i as f32 * 0.004) + x,
                    y: 100. + y,
                },
                pressure: (0.7 + pressure_noise * random()).clamp(0., 1.),
                tilt: [0.5, 0.3],
                twist: 0.,
                elapsed_micros: i * 4000,
            }
        })
        .collect()
}
