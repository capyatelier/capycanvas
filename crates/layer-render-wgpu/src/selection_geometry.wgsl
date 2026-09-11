// Canonical crossing used by the packed polygon coverage initializer.
fn selection_crossing(e: vec4<f32>, y: f32) -> f32 {
    // Canonical endpoint order keeps reversed shared edges identical and avoids
    // cancellation at a vertex lying exactly on an antialias sample.
    let low = select(e.xy, e.zw, e.y > e.w);
    let high = select(e.zw, e.xy, e.y > e.w);
    return (high.x-low.x)*((y-low.y)/(high.y-low.y))+low.x;
}
