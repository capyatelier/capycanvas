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
@group(2) @binding(5) var wet_center: texture_2d<f32>;
@group(2) @binding(6) var wet_left: texture_2d<f32>;
@group(2) @binding(7) var wet_right: texture_2d<f32>;
@group(2) @binding(8) var wet_up: texture_2d<f32>;
@group(2) @binding(9) var wet_down: texture_2d<f32>;
@group(3) @binding(0) var conductance_texture: texture_2d<f32>;
@group(3) @binding(1) var conductance_sampler: sampler;

override TRANSPORT_PASS: u32 = 0u;
const TRANSPORT_STEP_SCALES: array<f32, 3> = array<f32, 3>(
    0.51,
    0.31,
    0.18,
);
// Two R8 levels are the persistent watercolor-material floor used by live
// edge composition. Values above it are the local water amount.
const MIN_WETNESS: f32 = 2.0 / 255.0;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    return vec4<f32>(positions[vertex_index], 0.0, 1.0);
}

fn rotate(point: vec2<f32>, cosine: f32, sine: f32) -> vec2<f32> {
    return vec2<f32>(
        point.x * cosine - point.y * sine,
        point.x * sine + point.y * cosine,
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

fn load_wet(index: i32, coordinate: vec2<i32>) -> f32 {
    switch index {
        case 0: { return textureLoad(wet_center, coordinate, 0).r; }
        case 1: { return textureLoad(wet_left, coordinate, 0).r; }
        case 2: { return textureLoad(wet_right, coordinate, 0).r; }
        case 3: { return textureLoad(wet_up, coordinate, 0).r; }
        default: { return textureLoad(wet_down, coordinate, 0).r; }
    }
}

fn page_sample(document_position: vec2<f32>, wetness: bool) -> vec4<f32> {
    if any(document_position < vec2<f32>(0.0))
        || any(document_position >= render_target.document_extent.xy) {
        return vec4<f32>(0.0);
    }
    let relative = document_position - render_target.origin_extent.xy;
    let page_offset = vec2<i32>(floor(relative / 256.0));
    let local = vec2<i32>(floor(relative - vec2<f32>(page_offset) * 256.0));
    let coordinate = clamp(local, vec2<i32>(0), vec2<i32>(255));
    var index = -1;
    if all(page_offset == vec2<i32>(0, 0)) { index = 0; }
    if all(page_offset == vec2<i32>(-1, 0)) { index = 1; }
    if all(page_offset == vec2<i32>(1, 0)) { index = 2; }
    if all(page_offset == vec2<i32>(0, -1)) { index = 3; }
    if all(page_offset == vec2<i32>(0, 1)) { index = 4; }
    if index < 0 { return vec4<f32>(0.0); }
    if wetness { return vec4<f32>(load_wet(index, coordinate)); }
    return load_color(index, coordinate);
}

fn conductance(world: vec2<f32>) -> f32 {
    let coordinate = rotate(
        world / 256.0,
        style.transport_a.y,
        style.transport_a.z,
    ) * style.transport_a.x;
    let sampled = textureSampleLevel(conductance_texture, conductance_sampler, coordinate, 0.0).r;
    // Preserve the low shoulders around each bright fiber. Thresholding them
    // created large perfectly flat cells, so the derived gradient fell back to
    // one canvas axis and exposed horizontal whiskers around each update.
    let shaped = pow(clamp(sampled, 0.0, 1.0), 0.72);
    return mix(1.0, shaped, style.transport_a.w);
}

fn orientation_hash(world: vec2<f32>) -> f32 {
    return fract(sin(dot(floor(world * 0.125), vec2<f32>(12.9898, 78.233)))
        * 43758.5453);
}

fn path_conductance(start: vec2<f32>, end: vec2<f32>, start_conductance: f32) -> f32 {
    let finish = conductance(end);
    // Each aligned hop is at most 32 document pixels, so its two ends are
    // sufficient for the broad, document-scaled fiber. Avoiding a midpoint
    // lookup saves four filtered texture samples per pixel per stage.
    let average_path = (start_conductance + finish) * 0.5;
    return 0.02 + 0.98 * average_path * average_path;
}

struct Exchange {
    candidate_wetness: f32,
    candidate_pigment: vec4<f32>,
    pigment_relaxation: vec4<f32>,
    relaxation_weight: f32,
}

fn exchange(
    world: vec2<f32>,
    center: vec4<f32>,
    center_wet: f32,
    center_conductance: f32,
    offset: vec2<f32>,
) -> Exchange {
    let neighbor_position = world + offset;
    let neighbor_selection = brush_selection_at(neighbor_position);
    let neighbor_wet = page_sample(neighbor_position, true).x;
    let neighbor = page_sample(neighbor_position, false);
    // The destination's wetness selects the artist-facing mode. Watercolor can
    // favor wet paper while ink can favor dry fibers with the same kernel.
    let recipient_is_watercolor = center_wet >= MIN_WETNESS;
    let rate = select(
        style.transport_b.y,
        style.transport_b.x,
        recipient_is_watercolor,
    ) * neighbor_selection;
    let path = path_conductance(world, neighbor_position, center_conductance);

    // Wetness is a capillary activation field rather than a conserved fluid
    // volume. A wetter neighbor advances a decaying front through conductive
    // paper. This reaches useful distances in a handful of event-driven passes
    // without the vanishing tail produced by a Gaussian diffusion stencil.
    let transmission = clamp(rate * (0.06 + 0.92 * path), 0.0, 0.94);
    let candidate_wetness = neighbor_wet * transmission;
    // Pigment stains remain coherent along a conductive fiber while dying off
    // rapidly across the paper body. Tying pigment only to water magnitude
    // made every multi-hop result disappear in the 8-bit layer target before
    // the wet front reached a visible branch.
    let pigment_transmission = clamp(
        mix(0.04, 0.98, path) * mix(0.42, 1.0, rate),
        0.0,
        0.98,
    );
    let candidate_pigment = neighbor * pigment_transmission;

    // Equal-wetness regions no longer have a front, but their colors should
    // still settle locally. Keep this weaker than front transport so it mixes
    // adjacent washes without turning the entire wet layer into a blur.
    let shared_wetness = smoothstep(
        MIN_WETNESS,
        0.45,
        min(center_wet, neighbor_wet),
    );
    // The same selection boundary limits both front transport and wet mixing.
    let relaxation_weight = style.transport_b.x * shared_wetness * path * 0.16
        * neighbor_selection;
    return Exchange(
        candidate_wetness,
        candidate_pigment,
        (neighbor - center) * relaxation_weight,
        relaxation_weight,
    );
}

struct TransportOutput {
    @location(0) color: vec4<f32>,
    @location(1) wetness: vec4<f32>,
}

@fragment
fn fragment_main(@builtin(position) position: vec4<f32>) -> TransportOutput {
    let world = render_target.origin_extent.xy + position.xy;
    // The destination pixel is always in the center page. Direct loads avoid
    // three general cross-page address/switch sequences per fragment; only the
    // four offset endpoints need neighborhood routing.
    let coordinate = clamp(
        vec2<i32>(position.xy),
        vec2<i32>(0),
        vec2<i32>(255),
    );
    let center = textureLoad(color_center, coordinate, 0);
    let center_wet = textureLoad(wet_center, coordinate, 0).r;
    let clip = brush_selection_at(world);
    if clip <= 0. { return TransportOutput(center, vec4<f32>(center_wet, 0., 0., 1.)); }
    let center_conductance = conductance(world);
    // Incommensurate coarse-to-fine hops cover the requested radius without
    // landing every pass on one visible pixel lattice. The damage rect remains
    // the hard bound on the effect.
    let step_distance = clamp(
        style.transport_b.z * TRANSPORT_STEP_SCALES[TRANSPORT_PASS],
        1.0,
        32.0,
    );
    var best_wetness = 0.0;
    var best_pigment = vec4<f32>(0.0);
    var pigment_relaxation = vec4<f32>(0.0);
    var relaxation_weight = 0.0;
    // A scalar conductance field provides a stable shoulder gradient and ridge
    // curvature. The long pair follows their tangent; the short normal pair
    // lets fresh deposition enter a nearby fiber. This avoids both a stored
    // direction channel and canvas-axis rails.
    let probe = clamp(step_distance * 0.42, 1.0, 3.0);
    let diagonal = probe * 0.70710678;
    let conductance_x0 = conductance(world - vec2<f32>(probe, 0.0));
    let conductance_x1 = conductance(world + vec2<f32>(probe, 0.0));
    let conductance_y0 = conductance(world - vec2<f32>(0.0, probe));
    let conductance_y1 = conductance(world + vec2<f32>(0.0, probe));
    let conductance_pp = conductance(world + vec2<f32>(diagonal, diagonal));
    let conductance_mm = conductance(world - vec2<f32>(diagonal, diagonal));
    let gradient = vec2<f32>(
        conductance_x1 - conductance_x0,
        conductance_y1 - conductance_y0,
    );
    let curvature_x = conductance_x1 - 2.0 * center_conductance + conductance_x0;
    let curvature_y = conductance_y1 - 2.0 * center_conductance + conductance_y0;
    let diagonal_curvature = conductance_pp - 2.0 * center_conductance
        + conductance_mm;
    let curvature_xy = diagonal_curvature
        - 0.5 * (curvature_x + curvature_y);
    var tangent_angle = orientation_hash(world) * 6.2831853;
    if dot(gradient, gradient) > 0.000025 {
        // On a fiber shoulder, the scalar gradient is its normal. Following
        // the perpendicular is more stable than a curvature estimate and
        // naturally bends with the connected ridge.
        tangent_angle = atan2(gradient.x, -gradient.y);
    } else if abs(curvature_x) + abs(curvature_y) + abs(curvature_xy) > 0.002 {
        // At the crest the gradient vanishes, so use the Hessian eigenvector
        // with the weaker curvature: the direction along the ridge.
        tangent_angle = 0.5 * atan2(
            2.0 * curvature_xy,
            curvature_x - curvature_y,
        );
    }
    let tangent = vec2<f32>(cos(tangent_angle), sin(tangent_angle));
    let normal = vec2<f32>(-tangent.y, tangent.x);
    let cross_distance = min(step_distance * 0.38, 3.0);
    var directions = array<vec2<f32>, 4>(
        tangent * step_distance,
        -tangent * step_distance,
        normal * cross_distance,
        -normal * cross_distance,
    );
    for (var index = 0u; index < 4u; index += 1u) {
        let contribution = exchange(
            world, center, center_wet, center_conductance, directions[index],
        );
        if contribution.candidate_wetness > best_wetness {
            best_wetness = contribution.candidate_wetness;
            best_pigment = contribution.candidate_pigment;
        }
        pigment_relaxation += contribution.pigment_relaxation;
        relaxation_weight += contribution.relaxation_weight;
    }

    let decayed_wetness = select(
        0.0,
        MIN_WETNESS + (center_wet - MIN_WETNESS) * 0.984,
        center_wet >= MIN_WETNESS,
    );
    let next_wetness = clamp(max(decayed_wetness, best_wetness), 0.0, 1.0);
    let relaxation_stability = select(
        1.0,
        0.62 / max(relaxation_weight, 0.000001),
        relaxation_weight > 0.62,
    );
    let relaxed_pigment = clamp(
        center + pigment_relaxation * relaxation_stability,
        vec4<f32>(0.0),
        vec4<f32>(1.0),
    );
    var combined_pigment = relaxed_pigment;
    if best_wetness > decayed_wetness + MIN_WETNESS && best_pigment.a > MIN_WETNESS {
        let arrival = clamp(
            sqrt(
                (best_wetness - decayed_wetness)
                    / max(best_wetness, MIN_WETNESS),
            ),
            0.0,
            1.0,
        );
        let alpha = max(relaxed_pigment.a, best_pigment.a);
        let center_color = relaxed_pigment.rgb / max(relaxed_pigment.a, 0.000001);
        let arriving_color = best_pigment.rgb / max(best_pigment.a, 0.000001);
        let color_weight = arrival * clamp(
            best_pigment.a / max(alpha, 0.000001),
            0.0,
            1.0,
        );
        combined_pigment = vec4<f32>(
            mix(center_color, arriving_color, color_weight) * alpha,
            alpha,
        );
    }
    let unclamped_pigment = clamp(combined_pigment, vec4<f32>(0.0), vec4<f32>(1.0));
    var next_pigment = vec4<f32>(
        min(unclamped_pigment.rgb, vec3<f32>(unclamped_pigment.a)),
        unclamped_pigment.a,
    );
    if style.color.a > 0.5 {
        next_pigment = vec4<f32>(next_pigment.rgb / max(next_pigment.a, 0.000001) * center.a, center.a);
    }
    return TransportOutput(
        mix(center, next_pigment, clip),
        vec4<f32>(mix(center_wet, select(next_wetness, 0.0, style.color.a > 0.5 && center.a == 0.0), clip), 0.0, 0.0, 1.0),
    );
}
