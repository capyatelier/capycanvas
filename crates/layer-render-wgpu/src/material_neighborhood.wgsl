fn blurred_canvas_load(position: vec2<f32>) -> vec4<f32> {
    let radius = pickup_radius();
    if radius < 0.01 {
        return canvas_load(position);
    }
    return (canvas_load(position) * 4.0
        + canvas_load(position + vec2<f32>( radius, 0.0))
        + canvas_load(position + vec2<f32>(-radius, 0.0))
        + canvas_load(position + vec2<f32>(0.0,  radius))
        + canvas_load(position + vec2<f32>(0.0, -radius))) / 8.0;
}

fn pulled_pickup(fragment_position: vec2<f32>, world: vec2<f32>, dab: Dab, row: u32) -> vec4<f32> {
    return blurred_canvas_load(pulled_position(world, dab));
}

fn traced_sample(fragment_position: vec2<f32>, position: vec2<f32>) -> vec4<f32> {
    let radius = pickup_radius();
    if radius < 0.01 {
        return canvas_sample(position);
    }
    return (canvas_sample(position) * 4.0
        + canvas_load(position + vec2<f32>( radius, 0.0))
        + canvas_load(position + vec2<f32>(-radius, 0.0))
        + canvas_load(position + vec2<f32>(0.0,  radius))
        + canvas_load(position + vec2<f32>(0.0, -radius))) / 8.0;
}

fn exchange_pickup(dab: Dab, offset: u32, texel: vec2<u32>, world: vec2<f32>) -> vec4<f32> {
    return blurred_canvas_load(pulled_position(world, dab));
}

fn footprint_tap(dab: Dab, offset: u32, tap: u32, world: vec2<f32>) -> vec4<f32> {
    return canvas_load(world);
}
