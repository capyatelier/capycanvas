struct PaperExport { tile: vec4<f32>, light: vec4<f32>, origin: vec4<f32>, state: vec4<f32>, axes: vec4<f32>, slope: vec4<f32> };
@group(0) @binding(0) var<uniform> export_shade: PaperExport;
@group(0) @binding(1) var export_source: texture_2d<f32>;
@group(0) @binding(2) var export_slope: texture_2d<f32>;
@group(0) @binding(3) var export_sampler: sampler;
@group(0) @binding(4) var export_coverage: texture_2d<f32>;

@vertex
fn vertex_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn paper_export(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(floor(position.xy));
    let color = textureLoad(export_source, texel, 0);
    let coverage = textureLoad(export_coverage, texel, 0).a;
    let weight = select(1., 1. - coverage * (1. - export_shade.light.w), export_shade.state.y != 0.);
    let p = export_shade.origin.xy + position.xy;
    let axes = mat2x2<f32>(export_shade.axes.xy, export_shade.axes.zw);
    let slope = textureSampleLevel(export_slope, export_sampler, axes * (p + export_shade.tile.zw) / export_shade.tile.xy, 0.).xy;
    return paper_lit(color, paper_shade_factor(mat2x2<f32>(export_shade.slope.xy, export_shade.slope.zw) * slope, export_shade.light), weight);
}
