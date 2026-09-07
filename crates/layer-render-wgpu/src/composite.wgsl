struct Style {
    color: vec4<f32>,
    canvas_opacity: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> style: Style;

@group(1) @binding(0)
var source_texture: texture_2d<f32>;

@group(1) @binding(1)
var source_sampler: sampler;

struct Target {
    origin_extent: vec4<f32>,
    document_extent: vec4<f32>,
}

@group(2) @binding(0)
var<uniform> render_target: Target;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var coordinates = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(2.0, 0.0),
        vec2<f32>(0.0, 2.0),
    );
    let uv = coordinates[vertex_index];
    let document_position = render_target.origin_extent.xy + uv * render_target.origin_extent.zw;
    let extent = render_target.document_extent.xy;
    var output: VertexOutput;
    output.position = vec4<f32>(
        document_position.x / extent.x * 2.0 - 1.0,
        1.0 - document_position.y / extent.y * 2.0,
        0.0,
        1.0,
    );
    output.uv = uv;
    return output;
}

@fragment
fn background_fragment() -> @location(0) vec4<f32> {
    let alpha = style.color.a;
    return vec4<f32>(style.color.rgb * alpha, alpha);
}

@fragment
fn layer_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(source_texture, source_sampler, input.uv) * style.canvas_opacity.z;
}
