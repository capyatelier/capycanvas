// Shared by exported previews and in-surface overviews. Average premultiplied
// linear color; never filter straight-alpha RGB or gamma-encoded samples.
fn sample_overview(image: texture_2d<f32>, image_sampler: sampler,
    uv: vec2<f32>, footprint: vec2<f32>) -> vec4<f32> {
    var color = vec4(0.);
    for (var y=0u; y<4u; y++) {
        for (var x=0u; x<4u; x++) {
            let offset = (vec2(f32(x),f32(y))+.5)/4.-.5;
            color += textureSampleLevel(image, image_sampler, uv+offset*footprint, 0.);
        }
    }
    return color / 16.;
}
