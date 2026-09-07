// Shared contact-shape code for direct textured deposition and destination-
// aware materials. Both shader bodies provide `brush_sampler` and the brush
// textures consumed here.

fn rotate(point: vec2<f32>, cosine: f32, sine: f32) -> vec2<f32> {
    return vec2<f32>(
        point.x * cosine - point.y * sine,
        point.x * sine + point.y * cosine,
    );
}

fn analytic_coverage(local: vec2<f32>, hardness: f32, min_radius: f32) -> f32 {
    let radius_squared = dot(local, local);
    if radius_squared >= 1.0 {
        return 0.0;
    }
    let edge = max(
        1.0 - clamp(hardness, 0.0, 1.0),
        1.0 / max(min_radius, 0.005),
    );
    let solid_radius = max(1.0 - edge, 0.0);
    if radius_squared <= solid_radius * solid_radius {
        return 1.0;
    }
    return clamp((1.0 - sqrt(radius_squared)) / edge, 0.0, 1.0);
}

fn tip_coverage(
    analytic: bool,
    texture: texture_2d<f32>,
    local: vec2<f32>,
    sign: vec2<f32>,
    hardness: f32,
    min_radius: f32,
) -> f32 {
    if dot(local, local) >= 1.0 {
        return 0.0;
    }
    if analytic {
        return analytic_coverage(local, hardness, min_radius);
    }
    let uv = local * sign * 0.5 + vec2<f32>(0.5);
    // Brush assets have one mip level. Explicit LOD is valid after per-pixel
    // silhouette rejection and in ordered destination-contact loops on WebGPU.
    return textureSampleLevel(texture, brush_sampler, uv, 0.0).r;
}

fn grain_uv(
    local: vec2<f32>,
    world: vec2<f32>,
    parameters: vec4<f32>,
    canvas_locked: bool,
    seed: vec2<f32>,
    offset_jitter: f32,
) -> vec2<f32> {
    let coordinate = select(local * 0.5, world / 256.0, canvas_locked);
    let random_offset = vec2<f32>(
        fract(sin(dot(seed, vec2<f32>(12.9898, 78.233))) * 43758.5453),
        fract(sin(dot(seed, vec2<f32>(39.3467, 11.135))) * 24634.6345),
    ) - vec2<f32>(0.5);
    return rotate(coordinate, parameters.z, parameters.w) * parameters.x
        + vec2<f32>(0.5) + random_offset * offset_jitter;
}

fn combine_coverage(primary: f32, secondary: f32, mode: f32) -> f32 {
    if mode < 0.5 {
        return primary * secondary;
    }
    if mode < 1.5 {
        return min(primary + secondary, 1.0);
    }
    if mode < 2.5 {
        return max(primary - secondary, 0.0);
    }
    if mode < 3.5 {
        return abs(primary - secondary);
    }
    if mode < 4.5 {
        return min(primary, secondary);
    }
    return max(primary, secondary);
}
