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
@group(2) @binding(0) var color_center: texture_2d<f32>;
@group(2) @binding(1) var color_left: texture_2d<f32>;
@group(2) @binding(2) var color_right: texture_2d<f32>;
@group(2) @binding(3) var color_up: texture_2d<f32>;
@group(2) @binding(4) var color_down: texture_2d<f32>;
@group(2) @binding(5) var wetness_00: texture_2d<f32>;
@group(2) @binding(6) var wetness_10: texture_2d<f32>;
@group(2) @binding(7) var wetness_20: texture_2d<f32>;
@group(2) @binding(8) var wetness_01: texture_2d<f32>;
@group(2) @binding(9) var wetness_11: texture_2d<f32>;
@group(2) @binding(10) var wetness_21: texture_2d<f32>;
@group(2) @binding(11) var wetness_02: texture_2d<f32>;
@group(2) @binding(12) var wetness_12: texture_2d<f32>;
@group(2) @binding(13) var wetness_22: texture_2d<f32>;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    var coordinates = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(2.0, 0.0),
        vec2<f32>(0.0, 2.0),
    );
    let document_position = render_target.origin_extent.xy
        + coordinates[vertex_index] * render_target.origin_extent.zw;
    if style.color.r > 0.5 {
        let uv = coordinates[vertex_index];
        return vec4<f32>(uv.x*2.-1.,1.-uv.y*2.,0.,1.);
    }
    let extent = render_target.document_extent.xy;
    return vec4<f32>(
        document_position.x / extent.x * 2.0 - 1.0,
        1.0 - document_position.y / extent.y * 2.0,
        0.0,
        1.0,
    );
}

fn load_color(index: i32, coordinate: vec2<i32>) -> vec4<f32> {
    switch index {
        case 0: { return textureLoad(color_center, coordinate, 0); }
        case 1: { return textureLoad(color_left, coordinate, 0); }
        case 2: { return textureLoad(color_right, coordinate, 0); }
        case 3: { return textureLoad(color_up, coordinate, 0); }
        default: { return textureLoad(color_down, coordinate, 0); }
    }
}

fn load_wetness(index: i32, coordinate: vec2<i32>) -> f32 {
    switch index {
        case 0: { return textureLoad(wetness_00, coordinate, 0).r; }
        case 1: { return textureLoad(wetness_10, coordinate, 0).r; }
        case 2: { return textureLoad(wetness_20, coordinate, 0).r; }
        case 3: { return textureLoad(wetness_01, coordinate, 0).r; }
        case 4: { return textureLoad(wetness_11, coordinate, 0).r; }
        case 5: { return textureLoad(wetness_21, coordinate, 0).r; }
        case 6: { return textureLoad(wetness_02, coordinate, 0).r; }
        case 7: { return textureLoad(wetness_12, coordinate, 0).r; }
        default: { return textureLoad(wetness_22, coordinate, 0).r; }
    }
}

fn color_at(document_position: vec2<f32>) -> vec4<f32> {
    if any(document_position < vec2<f32>(0.0))
        || any(document_position >= render_target.document_extent.xy) {
        return vec4<f32>(0.0);
    }
    let relative = document_position - render_target.origin_extent.xy;
    let page_offset = vec2<i32>(floor(relative / 256.0));
    let local = vec2<i32>(floor(relative - vec2<f32>(page_offset) * 256.0));
    let coordinate = clamp(local, vec2<i32>(0), vec2<i32>(255));
    if all(page_offset == vec2<i32>(0, 0)) {
        return load_color(0, coordinate);
    }
    if all(page_offset == vec2<i32>(-1, 0)) {
        return load_color(1, coordinate);
    }
    if all(page_offset == vec2<i32>(1, 0)) {
        return load_color(2, coordinate);
    }
    if all(page_offset == vec2<i32>(0, -1)) {
        return load_color(3, coordinate);
    }
    if all(page_offset == vec2<i32>(0, 1)) {
        return load_color(4, coordinate);
    }
    return vec4<f32>(0.0);
}

fn wetness_at(document_position: vec2<f32>) -> f32 {
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
    return load_wetness((page_offset.y + 1) * 3 + page_offset.x + 1, coordinate);
}

fn occupied(wetness: f32) -> f32 {
    // R8 wetness is sampled with textureLoad. Ignore only the bottom quantized
    // fringe so a nearly empty texel cannot create an amplified outside halo.
    return select(0.0, 1.0, wetness >= (2.0 / 255.0));
}

struct Band {
    minimum: f32,
    maximum: f32,
    strongest_pigment: f32,
    source: vec4<f32>,
}

fn sample_band(
    world: vec2<f32>,
    center_occupied: f32,
    center_color: vec4<f32>,
    radius: f32,
) -> Band {
    var directions = array<vec2<f32>, 4>(
        vec2<f32>( 1.0,  0.0), vec2<f32>(-1.0,  0.0),
        vec2<f32>( 0.0,  1.0), vec2<f32>( 0.0, -1.0),
    );
    var band = Band(center_occupied, center_occupied, center_color.a, center_color);
    for (var index = 0u; index < 4u; index += 1u) {
        let sample_position = world + directions[index] * radius;
        let sample_wetness = wetness_at(sample_position);
        let sample_occupied = occupied(sample_wetness);
        band.minimum = min(band.minimum, sample_occupied);
        band.maximum = max(band.maximum, sample_occupied);
        // The outside band borrows pigment only from the wet union. Nearby
        // opaque dry paint must not recolor or strengthen a watercolor halo.
        if sample_occupied > 0.5 {
            let sample_color = color_at(sample_position);
            if sample_color.a > band.strongest_pigment {
                band.strongest_pigment = sample_color.a;
                band.source = sample_color;
            }
        }
    }
    return band;
}

@fragment
fn fragment_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let world = position.xy + select(vec2<f32>(0.), render_target.origin_extent.xy, style.color.r > 0.5);
    let center = color_at(world);
    let center_wetness = wetness_at(world);
    let center_occupied = occupied(center_wetness);
    let width = clamp(style.edges.z, 1.0, 16.0);
    let deep = sample_band(world, center_occupied, center, width * 2.0);

    // A stable watercolor interior, including every overlap-density change,
    // and a region with no watercolor wetness need no near-band samples.
    if (center_occupied > 0.5 && deep.minimum > 0.5)
        || (center_occupied < 0.5 && deep.maximum < 0.5) {
        return center * style.canvas_opacity.z;
    }
    let near = sample_band(world, center_occupied, center, width);

    // Morphology uses only the unioned watercolor wetness mask. Pigment alpha can
    // vary through pressure, glazing, or overlapping strokes without becoming
    // an edge. The original RGBA remains the independent pigment field.
    let rim = center_occupied * (1.0 - near.minimum);
    let inner_band = center_occupied * near.minimum * (1.0 - deep.minimum);
    let outer_band = (1.0 - center_occupied)
        * max(near.maximum, deep.maximum * 0.45);
    let edge_strength = clamp(style.edges.x, 0.0, 1.0);

    var density = center.a * (1.0 - inner_band * edge_strength * 0.30);
    density *= 1.0 + rim * edge_strength * 0.65;
    let outside_source = select(deep.source, near.source, near.maximum > 0.5);
    let outside_density = select(
        outside_source.a * outer_band * edge_strength * 0.13,
        0.0,
        center.a > 0.000001,
    );
    density = clamp(max(density, outside_density), 0.0, 1.0);
    if density <= 0.000001 {
        return vec4<f32>(0.0);
    }

    let source = select(outside_source, center, center.a > 0.000001);
    if source.a <= 0.000001 {
        return center * style.canvas_opacity.z;
    }
    let straight = source.rgb / max(source.a, 0.000001);
    let darken = clamp(
        rim * (edge_strength * 0.16 + clamp(style.edges.y, 0.0, 1.0) * 0.62),
        0.0,
        0.78,
    );
    let alpha = density * style.canvas_opacity.z;
    return vec4<f32>(straight * (1.0 - darken) * alpha, alpha);
}
