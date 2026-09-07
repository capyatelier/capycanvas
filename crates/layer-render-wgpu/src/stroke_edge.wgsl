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
}

struct Target {
    origin_extent: vec4<f32>,
    document_extent: vec4<f32>,
}

@group(0) @binding(0) var<uniform> style: Style;
@group(1) @binding(0) var<uniform> render_target: Target;
@group(2) @binding(0) var color_source: texture_2d<f32>;
@group(2) @binding(1) var coverage_00: texture_2d<f32>;
@group(2) @binding(2) var coverage_10: texture_2d<f32>;
@group(2) @binding(3) var coverage_20: texture_2d<f32>;
@group(2) @binding(4) var coverage_01: texture_2d<f32>;
@group(2) @binding(5) var coverage_11: texture_2d<f32>;
@group(2) @binding(6) var coverage_21: texture_2d<f32>;
@group(2) @binding(7) var coverage_02: texture_2d<f32>;
@group(2) @binding(8) var coverage_12: texture_2d<f32>;
@group(2) @binding(9) var coverage_22: texture_2d<f32>;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    return vec4<f32>(positions[vertex_index], 0.0, 1.0);
}

fn load_coverage(index: i32, coordinate: vec2<i32>) -> f32 {
    switch index {
        case 0: { return textureLoad(coverage_00, coordinate, 0).r; }
        case 1: { return textureLoad(coverage_10, coordinate, 0).r; }
        case 2: { return textureLoad(coverage_20, coordinate, 0).r; }
        case 3: { return textureLoad(coverage_01, coordinate, 0).r; }
        case 4: { return textureLoad(coverage_11, coordinate, 0).r; }
        case 5: { return textureLoad(coverage_21, coordinate, 0).r; }
        case 6: { return textureLoad(coverage_02, coordinate, 0).r; }
        case 7: { return textureLoad(coverage_12, coordinate, 0).r; }
        default: { return textureLoad(coverage_22, coordinate, 0).r; }
    }
}

fn coverage_at(document_position: vec2<f32>) -> f32 {
    if any(document_position < vec2<f32>(0.0))
        || any(document_position >= render_target.document_extent.xy) {
        return 0.0;
    }
    let relative = document_position - render_target.origin_extent.xy;
    let page_offset = vec2<i32>(floor(relative / 256.0));
    if any(page_offset < vec2<i32>(-1)) || any(page_offset > vec2<i32>(1)) {
        return 0.0;
    }
    let local = vec2<i32>(floor(relative - vec2<f32>(page_offset) * 256.0));
    let coordinate = clamp(local, vec2<i32>(0), vec2<i32>(255));
    return load_coverage((page_offset.y + 1) * 3 + page_offset.x + 1, coordinate);
}

@fragment
fn fragment_main(@builtin(position) fragment_position: vec4<f32>) -> @location(0) vec4<f32> {
    let coordinate = clamp(vec2<i32>(floor(fragment_position.xy)), vec2<i32>(0), vec2<i32>(255));
    let color = textureLoad(color_source, coordinate, 0);
    let world = render_target.origin_extent.xy + fragment_position.xy;
    let center = coverage_at(world);
    if center <= 0.0 || color.a <= 0.0 {
        return color;
    }
    let radius = clamp(style.edges.z, 1.0, 8.0);
    var outside = 1.0;
    outside = min(outside, coverage_at(world + vec2<f32>( radius, 0.0)));
    outside = min(outside, coverage_at(world + vec2<f32>(-radius, 0.0)));
    outside = min(outside, coverage_at(world + vec2<f32>(0.0,  radius)));
    outside = min(outside, coverage_at(world + vec2<f32>(0.0, -radius)));
    outside = min(outside, coverage_at(world + vec2<f32>( radius,  radius) * 0.7071));
    outside = min(outside, coverage_at(world + vec2<f32>(-radius,  radius) * 0.7071));
    outside = min(outside, coverage_at(world + vec2<f32>( radius, -radius) * 0.7071));
    outside = min(outside, coverage_at(world + vec2<f32>(-radius, -radius) * 0.7071));
    let band = clamp(center - outside, 0.0, 1.0);
    let added_alpha = style.edges.x * band * (1.0 - color.a) * 0.45;
    let alpha = clamp(color.a + added_alpha, 0.0, 1.0);
    let straight = color.rgb / max(color.a, 0.000001);
    let darken = clamp((style.edges.x * 0.18 + style.edges.y * 0.62) * band, 0.0, 0.8);
    let edge_color = straight * (1.0 - darken);
    return vec4<f32>(mix(straight, edge_color, band) * alpha, alpha);
}
