struct Style {
    color: vec4<f32>,
    canvas_opacity: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> style: Style;

struct Target {
    origin_extent: vec4<f32>,
    document_extent: vec4<f32>,
}

@group(1) @binding(0)
var<uniform> render_target: Target;

@group(2) @binding(0)
var tip_texture: texture_2d<f32>;

@group(2) @binding(1)
var tip_sampler: sampler;

struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
    @location(0) center: vec2<f32>,
    @location(1) radii: vec2<f32>,
    @location(2) rotation: vec2<f32>,
    @location(3) motion: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) flow_hardness: vec2<f32>,
    @location(6) texture_sign: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) flow_hardness: vec2<f32>,
    @location(2) min_radius: f32,
    @location(3) color: vec4<f32>,
    @location(4) texture_sign: vec2<f32>,
}

@vertex
fn vertex_main(input: VertexInput) -> VertexOutput {
    var corners = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0,  1.0),
    );
    let local = corners[input.vertex_index];
    let scaled = local * input.radii;
    let world = input.center + vec2<f32>(
        scaled.x * input.rotation.x - scaled.y * input.rotation.y,
        scaled.x * input.rotation.y + scaled.y * input.rotation.x,
    );
    let origin = render_target.origin_extent.xy;
    let extent = render_target.origin_extent.zw;

    var output: VertexOutput;
    output.position = vec4<f32>(
        (world.x - origin.x) / extent.x * 2.0 - 1.0,
        1.0 - (world.y - origin.y) / extent.y * 2.0,
        0.0,
        1.0,
    );
    output.local = local;
    output.flow_hardness = input.flow_hardness;
    output.min_radius = min(input.radii.x, input.radii.y);
    output.color = input.color;
    output.texture_sign = input.texture_sign;
    return output;
}

fn brush_color(coverage: f32, input: VertexOutput) -> vec4<f32> {
    let alpha = clamp(
        coverage * input.flow_hardness.x * style.canvas_opacity.z * input.color.a,
        0.0,
        1.0,
    );
    return vec4<f32>(input.color.rgb * alpha, alpha);
}

@fragment
fn analytic_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let radius_squared = dot(input.local, input.local);
    if radius_squared >= 1.0 {
        discard;
    }
    let edge = max(
        1.0 - clamp(input.flow_hardness.y, 0.0, 1.0),
        1.0 / max(input.min_radius, 0.005),
    );
    let solid_radius = max(1.0 - edge, 0.0);
    var coverage = 1.0;
    if radius_squared > solid_radius * solid_radius {
        coverage = clamp((1.0 - sqrt(radius_squared)) / edge, 0.0, 1.0);
    }
    return brush_color(coverage, input);
}

@fragment
fn mask_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    if dot(input.local, input.local) >= 1.0 {
        discard;
    }
    let tip_uv = input.local * input.texture_sign * 0.5 + vec2<f32>(0.5);
    let coverage = textureSample(tip_texture, tip_sampler, tip_uv).r;
    if coverage <= 0.0 {
        discard;
    }
    return brush_color(coverage, input);
}
