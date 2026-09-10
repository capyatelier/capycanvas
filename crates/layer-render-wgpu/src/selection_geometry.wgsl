// Shared even/odd polygon coverage: holes, disjoint islands and crossings.
fn selection_crossing(e: vec4<f32>, y: f32) -> f32 {
    // Canonical endpoint order keeps reversed shared edges identical and avoids
    // cancellation at a vertex lying exactly on an antialias sample.
    let low = select(e.xy, e.zw, e.y > e.w);
    let high = select(e.zw, e.xy, e.y > e.w);
    return (high.x-low.x)*((y-low.y)/(high.y-low.y))+low.x;
}
fn selection_inside(p: vec2<f32>, count: u32, inverted: bool) -> f32 {
    var value = inverted;
    for (var i = 0u; i < count; i++) {
        let e = edges[i];
        if (e.y > p.y) != (e.w > p.y) {
            if p.x < selection_crossing(e, p.y) { value = !value; }
        }
    }
    return select(0., 1., value);
}
fn selection_coverage(p: vec2<f32>, count: u32, inverted: bool) -> f32 {
    return (selection_inside(p+vec2<f32>(-.25,-.25), count, inverted)
        +selection_inside(p+vec2<f32>(.25,-.25), count, inverted)
        +selection_inside(p+vec2<f32>(-.25,.25), count, inverted)
        +selection_inside(p+vec2<f32>(.25,.25), count, inverted))*.25;
}
