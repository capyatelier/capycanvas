struct Style {
    color: vec4<f32>,
    canvas_opacity: vec4<f32>,
    grain: vec4<f32>,
    flags: vec4<f32>,
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
@group(2) @binding(5) var wetness_center: texture_2d<f32>;
@group(2) @binding(6) var wetness_left: texture_2d<f32>;
@group(2) @binding(7) var wetness_right: texture_2d<f32>;
@group(2) @binding(8) var wetness_up: texture_2d<f32>;
@group(2) @binding(9) var wetness_down: texture_2d<f32>;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    var coordinates = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(2.0, 0.0),
        vec2<f32>(0.0, 2.0),
    );
    let uv = coordinates[vertex_index];
    return vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
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
        case 0: { return textureLoad(wetness_center, coordinate, 0).r; }
        case 1: { return textureLoad(wetness_left, coordinate, 0).r; }
        case 2: { return textureLoad(wetness_right, coordinate, 0).r; }
        case 3: { return textureLoad(wetness_up, coordinate, 0).r; }
        default: { return textureLoad(wetness_down, coordinate, 0).r; }
    }
}

fn neighbor_index(page_offset: vec2<i32>) -> i32 {
    if all(page_offset == vec2<i32>(0, 0)) {
        return 0;
    }
    if all(page_offset == vec2<i32>(-1, 0)) {
        return 1;
    }
    if all(page_offset == vec2<i32>(1, 0)) {
        return 2;
    }
    if all(page_offset == vec2<i32>(0, -1)) {
        return 3;
    }
    if all(page_offset == vec2<i32>(0, 1)) {
        return 4;
    }
    return -1;
}

fn color_at(document_position: vec2<f32>) -> vec4<f32> {
    if any(document_position < vec2<f32>(0.0))
        || any(document_position >= render_target.document_extent.xy) {
        return vec4<f32>(0.0);
    }
    let relative = document_position - render_target.origin_extent.xy;
    let page_offset = vec2<i32>(floor(relative / 256.0));
    let index = neighbor_index(page_offset);
    if index < 0 { return vec4<f32>(0.0); }
    let local = vec2<i32>(floor(relative - vec2<f32>(page_offset) * 256.0));
    return load_color(index, clamp(local, vec2<i32>(0), vec2<i32>(255)));
}

fn wetness_at(document_position: vec2<f32>) -> f32 {
    if any(document_position < vec2<f32>(0.0))
        || any(document_position >= render_target.document_extent.xy) {
        return 0.0;
    }
    let relative = document_position - render_target.origin_extent.xy;
    let page_offset = vec2<i32>(floor(relative / 256.0));
    let index = neighbor_index(page_offset);
    if index < 0 { return 0.0; }
    let local = vec2<i32>(floor(relative - vec2<f32>(page_offset) * 256.0));
    let coordinate = clamp(local, vec2<i32>(0), vec2<i32>(255));
    return load_wetness(index, coordinate);
}

fn occupied(wetness: f32) -> f32 {
    return select(0.0, 1.0, wetness >= WATERCOLOR_FLOOR);
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
    let borrow_pigment = center_occupied < 0.5 && !working_has_color(center_color.a);
    for (var index = 0u; index < 4u; index += 1u) {
        let sample_position = world + directions[index] * radius;
        let sample_wetness = wetness_at(sample_position);
        let sample_occupied = occupied(sample_wetness);
        band.minimum = min(band.minimum, sample_occupied);
        band.maximum = max(band.maximum, sample_occupied);
        // The outside band borrows pigment only from the wet union. Nearby
        // opaque dry paint must not recolor or strengthen a watercolor halo.
        if borrow_pigment && sample_occupied > 0.5 {
            let sample_color = color_at(sample_position);
            if sample_color.a > band.strongest_pigment {
                band.strongest_pigment = sample_color.a;
                band.source = sample_color;
            }
        }
    }
    return band;
}

fn watercolor(position: vec2<f32>) -> vec4<f32> {
    let world = position + render_target.origin_extent.xy;
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
        working_has_color(center.a),
    );
    density = clamp(max(density, outside_density), 0.0, 1.0);
    if !working_has_color(density) {
        return vec4<f32>(0.0);
    }

    let source = select(outside_source, center, working_has_color(center.a));
    if !working_has_color(source.a) {
        return center * style.canvas_opacity.z;
    }
    let straight = working_unassociate(source);
    let darken = clamp(
        rim * (edge_strength * 0.16 + clamp(style.edges.y, 0.0, 1.0) * 0.62),
        0.0,
        0.78,
    );
    let alpha = density * style.canvas_opacity.z;
    return vec4<f32>(straight * (1.0 - darken) * alpha, alpha);
}

@fragment
fn fragment_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return watercolor(position.xy);
}
@group(3) @binding(0) var output: texture_storage_2d<rgba32float, write>;
@compute @workgroup_size(8, 8)
fn composite(@builtin(global_invocation_id) pixel: vec3<u32>) {
    if any(pixel.xy >= textureDimensions(output)) { return; }
    textureStore(output, pixel.xy, watercolor(vec2<f32>(pixel.xy) + .5));
}
