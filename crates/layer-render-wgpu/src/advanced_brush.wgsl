struct Style {
    color: vec4<f32>,
    canvas_opacity: vec4<f32>,
    grain: vec4<f32>,
    dual: vec4<f32>,
    dual_offset_flags: vec4<f32>,
    flags: vec4<f32>,
    dual_grain: vec4<f32>,
    advanced: vec4<f32>,
    edges: vec4<f32>,
    material_a: vec4<f32>,
    material_b: vec4<f32>,
    operation: vec4<u32>,
    deformation: vec4<f32>,
    render_mode: vec4<f32>,
    transport_a: vec4<f32>,
    transport_b: vec4<f32>,
    contact_a: vec4<f32>,
    contact_b: vec4<f32>,
    contact_c: vec4<f32>,
    brush_to_layer_linear: vec4<f32>,
    brush_to_layer_offset: vec4<f32>,
    layer_to_brush_linear: vec4<f32>,
    layer_to_brush_offset: vec4<f32>,
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
var primary_texture: texture_2d<f32>;
@group(2) @binding(1)
var grain_texture: texture_2d<f32>;
@group(2) @binding(2)
var dual_texture: texture_2d<f32>;
@group(2) @binding(3)
var dual_grain_texture: texture_2d<f32>;
@group(2) @binding(4)
var transport_texture: texture_2d<f32>;
@group(2) @binding(5)
var brush_sampler: sampler;

struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
    @location(0) center: vec2<f32>,
    @location(1) radii: vec2<f32>,
    @location(2) rotation: vec2<f32>,
    @location(3) motion: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) flow_hardness: vec2<f32>,
    @location(6) texture_sign: vec2<f32>,
    @location(7) material: vec4<f32>,
    @location(8) previous: vec4<f32>,
    @location(9) contact: vec4<f32>,
    @location(10) previous_contact: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) flow_hardness: vec2<f32>,
    @location(2) min_radius: f32,
    @location(3) color: vec4<f32>,
    @location(4) texture_sign: vec2<f32>,
    @location(5) world: vec2<f32>,
    @location(6) material: vec4<f32>,
    @location(7) center: vec2<f32>,
    @location(8) @interpolate(flat) previous: vec4<f32>,
    @location(9) @interpolate(flat) contact: vec4<f32>,
    @location(10) @interpolate(flat) previous_contact: vec4<f32>,
    @location(11) @interpolate(flat) geometry: vec4<f32>,
    @location(12) @interpolate(flat) motion: vec2<f32>,
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
    var world = input.center + vec2<f32>(
        scaled.x * input.rotation.x - scaled.y * input.rotation.y,
        scaled.x * input.rotation.y + scaled.y * input.rotation.x,
    );
    if style.contact_a.x > 0.5 {
        let radius = max(max(input.radii.x, input.radii.y), max(input.previous.x, input.previous.y)) * 1.5 + 1.0;
        let start = input.center - input.motion;
        world = mix(min(start, input.center) - vec2<f32>(radius), max(start, input.center) + vec2<f32>(radius), local * 0.5 + vec2<f32>(0.5));
    }
    let placed = brush_to_layer(world);
    let origin = render_target.origin_extent.xy;
    let extent = render_target.origin_extent.zw;

    var output: VertexOutput;
    output.position = vec4<f32>(
        (placed.x - origin.x) / extent.x * 2.0 - 1.0,
        1.0 - (placed.y - origin.y) / extent.y * 2.0,
        0.0,
        1.0,
    );
    output.local = local;
    output.flow_hardness = input.flow_hardness;
    output.min_radius = min(input.radii.x, input.radii.y);
    output.color = input.color;
    output.texture_sign = input.texture_sign;
    output.world = world;
    output.material = input.material;
    output.center = input.center;
    output.previous = input.previous;
    output.contact = input.contact;
    output.previous_contact = input.previous_contact;
    output.geometry = vec4<f32>(input.radii, input.rotation);
    output.motion = input.motion;
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    if style.contact_a.x > 0.5 {
        let coverage = evolving_contact(input.world, input.center, input.geometry.xy, input.geometry.zw,
            input.motion, input.previous, input.contact, input.previous_contact, input.flow_hardness.y);
        if coverage <= 0.0 { discard; }
        let exposure = contact_exposure(input.motion, input.geometry.xy, input.geometry.zw, input.flow_hardness.y);
        let alpha = (1.0 - exp(-coverage * input.flow_hardness.x * input.color.a * exposure * 6.0))
            * brush_selection_at(brush_to_layer(input.world));
        return vec4<f32>(input.color.rgb * alpha, alpha);
    }
    var coverage = tip_coverage(
        style.flags.x > 0.5,
        primary_texture,
        input.local,
        input.texture_sign,
        input.flow_hardness.y,
        input.min_radius,
    );
    if coverage <= 0.0 {
        discard;
    }

    if style.flags.y > 0.5 {
        let uv = grain_uv(
            input.local,
            input.world,
            style.grain,
            style.advanced.y > 0.5,
            input.center,
            style.advanced.w,
        );
        let grain = textureSampleLevel(grain_texture, brush_sampler, uv, 0.0).r;
        coverage *= mix(1.0, grain, clamp(input.material.x, 0.0, 1.0));
    }

    if style.flags.w > 0.5 {
        let shifted = input.local - style.dual_offset_flags.xy;
        let inverse_rotated = rotate(shifted, style.dual.z, -style.dual.w);
        let dual_scale = max(style.dual.x, 0.0001);
        let dual_aspect = max(style.dual.y, 0.0001);
        let dual_local = inverse_rotated / vec2<f32>(dual_scale * dual_aspect, dual_scale);
        var secondary = tip_coverage(
            style.flags.z > 0.5,
            dual_texture,
            dual_local,
            input.texture_sign,
            input.flow_hardness.y,
            input.min_radius * dual_scale,
        );
        if style.advanced.x > 0.5 {
            let uv = grain_uv(
                dual_local,
                input.world,
                style.dual_grain,
                style.advanced.z > 0.5,
                input.center + vec2<f32>(31.7, 19.3),
                style.edges.w,
            );
            let grain = textureSampleLevel(dual_grain_texture, brush_sampler, uv, 0.0).r;
            secondary *= mix(1.0, grain, clamp(style.dual_grain.y, 0.0, 1.0));
        }
        coverage = combine_coverage(coverage, secondary, style.dual_offset_flags.z);
    }

    let edge_width = clamp(style.edges.z, 0.0001, 1.0);
    let edge_band = smoothstep(0.0, edge_width, coverage)
        * (1.0 - smoothstep(edge_width, min(edge_width * 2.0, 1.0), coverage));
    coverage = clamp(coverage + edge_band * style.edges.x, 0.0, 1.0);
    if coverage < style.dual_offset_flags.w {
        discard;
    }

    let alpha = clamp(
        coverage * brush_selection_at(brush_to_layer(input.world))
            * input.flow_hardness.x * style.canvas_opacity.z * input.color.a,
        0.0,
        1.0,
    );
    let burnt = clamp(style.edges.y * edge_band, 0.0, 1.0);
    let rgb = input.color.rgb * (1.0 - burnt * 0.55);
    return vec4<f32>(rgb * alpha, alpha);
}
