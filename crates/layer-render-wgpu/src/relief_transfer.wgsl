fn relief_transfer(x: f32) -> f32 {
    let t = clamp(x, 0.0, 1.0);
    return t*t*(3.0-2.0*t);
}
