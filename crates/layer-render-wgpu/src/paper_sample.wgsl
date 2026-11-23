fn paper_sample(image: texture_2d<f32>, sampling: sampler, point: vec2<f32>, grain: vec4<f32>, axes: vec4<f32>) -> f32 {
    let p = point + grain.zw;
    return textureSampleLevel(image, sampling, vec2<f32>(dot(axes.xz, p), dot(axes.yw, p)) / grain.xy, 0.0).r;
}
