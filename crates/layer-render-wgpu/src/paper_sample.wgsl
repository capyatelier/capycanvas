fn paper_sample(image: texture_2d<f32>, sampling: sampler, point: vec2<f32>, grain: vec4<f32>) -> f32 {
    return textureSampleLevel(image, sampling, (point + grain.zw) / grain.xy, 0.0).r;
}
