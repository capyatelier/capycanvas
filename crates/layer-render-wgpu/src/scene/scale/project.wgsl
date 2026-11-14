struct Projection {
    origin: vec2<u32>,
    size: vec2<u32>,
    source_origin: vec2<f32>,
    scale: f32,
    source_space: u32,
    row0: vec4<f32>,
    row1: vec4<f32>,
    row2: vec4<f32>,
    source_perceptual: u32,
    output_perceptual: u32,
    padding: vec2<u32>,
}
@group(0) @binding(0) var<uniform> projection: Projection;
@group(1) @binding(0) var source: texture_2d<f32>;
@group(1) @binding(2) var output: texture_storage_2d<rgba32float, write>;

@compute @workgroup_size(8, 8)
fn project(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= projection.size) {return;}
    let point = projection.source_origin + (vec2<f32>(id.xy) + .5) * projection.scale;
    let value = working_sample_float(source, point);
    var color = working_unassociate(value);
    if projection.source_perceptual != 0u {color = sdr_decode(color, projection.source_space);}
    color = vec3(dot(projection.row0.xyz, color), dot(projection.row1.xyz, color), dot(projection.row2.xyz, color));
    if projection.output_perceptual != 0u {color = sdr_encode(color, WORKING_SPACE);}
    textureStore(output, vec2<i32>(projection.origin + id.xy), vec4(color * value.a, value.a));
}
