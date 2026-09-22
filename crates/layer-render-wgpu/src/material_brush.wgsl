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

// The pass planner supplies the operation as a pipeline constant so the GPU
// compiler sees only that operation's control flow. Style retains its packed
// operation field for the shared batch layout and reservoir pass.
override MATERIAL_OPERATION: u32;
override CONTACT_FILM_CULL: bool = true;

const OP_DEPOSIT: u32 = 0u;
const OP_COVERAGE: u32 = 1u;
const OP_LIQUIFY: u32 = 2u;
const OP_SMUDGE: u32 = 3u;
const OP_WET: u32 = 4u;
const OP_WATERCOLOR: u32 = 5u;

struct Target {
    origin_extent: vec4<f32>,
    document_extent: vec4<f32>,
}

struct Dab {
    center: vec2<f32>,
    radii: vec2<f32>,
    rotation: vec2<f32>,
    motion: vec2<f32>,
    color: vec4<f32>,
    flow: f32,
    hardness: f32,
    texture_sign: vec2<f32>,
    material: vec4<f32>,
    previous: vec4<f32>,
    contact: vec4<f32>,
    previous_contact: vec4<f32>,
    metric: vec4<f32>,
    invariants: vec4<f32>,
}

@group(0) @binding(0) var<uniform> style: Style;
@group(1) @binding(0) var<uniform> render_target: Target;

@group(2) @binding(0) var source_00: texture_2d<f32>;
@group(2) @binding(1) var source_10: texture_2d<f32>;
@group(2) @binding(2) var source_20: texture_2d<f32>;
@group(2) @binding(3) var source_01: texture_2d<f32>;
@group(2) @binding(4) var source_11: texture_2d<f32>;
@group(2) @binding(5) var source_21: texture_2d<f32>;
@group(2) @binding(6) var source_02: texture_2d<f32>;
@group(2) @binding(7) var source_12: texture_2d<f32>;
@group(2) @binding(8) var source_22: texture_2d<f32>;
@group(2) @binding(9) var<storage, read> dabs: array<Dab>;
@group(2) @binding(10) var stroke_coverage_texture: texture_2d<f32>;
@group(2) @binding(11) var reservoir_texture: texture_2d<f32>;
struct MaterialSources {
    // 0: adjacent pages; 1: disjoint gather pages; 2: gathered sample field.
    // Adjacent pages also provide the ordered dry-contact range in zw.
    header: vec4<u32>,
    // For dry paint, pages[0] is the local dirty rectangle (min xy, max xy).
    pages: array<vec4<u32>, 9>,
}
@group(2) @binding(12) var<uniform> material_sources: MaterialSources;

@group(3) @binding(0) var primary_texture: texture_2d<f32>;
@group(3) @binding(1) var grain_texture: texture_2d<f32>;
@group(3) @binding(2) var dual_texture: texture_2d<f32>;
@group(3) @binding(3) var dual_grain_texture: texture_2d<f32>;
@group(3) @binding(4) var transport_texture: texture_2d<f32>;
@group(3) @binding(5) var brush_sampler: sampler;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    return vec4<f32>(positions[vertex_index], 0.0, 1.0);
}

fn load_neighborhood(index: i32, coordinate: vec2<i32>) -> vec4<f32> {
    switch index {
        case 0: { return textureLoad(source_00, coordinate, 0); }
        case 1: { return textureLoad(source_10, coordinate, 0); }
        case 2: { return textureLoad(source_20, coordinate, 0); }
        case 3: { return textureLoad(source_01, coordinate, 0); }
        case 4: { return textureLoad(source_11, coordinate, 0); }
        case 5: { return textureLoad(source_21, coordinate, 0); }
        case 6: { return textureLoad(source_02, coordinate, 0); }
        case 7: { return textureLoad(source_12, coordinate, 0); }
        default: { return textureLoad(source_22, coordinate, 0); }
    }
}

fn local_canvas_load(document_position: vec2<f32>) -> vec4<f32> {
    if any(document_position < vec2<f32>(0.0))
        || any(document_position >= render_target.document_extent.xy) {
        return vec4<f32>(0.0);
    }
    if material_sources.header.x == 1u {
        let page = vec2<u32>(floor(document_position / 256.0));
        let local = vec2<i32>(floor(document_position)) - vec2<i32>(page * 256u);
        for (var index = 0u; index < material_sources.header.y; index += 1u) {
            if all(page == material_sources.pages[index].xy) {
                return load_neighborhood(i32(index), local);
            }
        }
        return vec4<f32>(0.0);
    }
    let relative = document_position - render_target.origin_extent.xy;
    let page_offset = vec2<i32>(floor(relative / 256.0));
    if any(page_offset < vec2<i32>(-1)) || any(page_offset > vec2<i32>(1)) {
        return vec4<f32>(0.0);
    }
    let local = vec2<i32>(floor(relative - vec2<f32>(page_offset) * 256.0));
    let coordinate = clamp(local, vec2<i32>(0), vec2<i32>(255));
    let index = (page_offset.y + 1) * 3 + page_offset.x + 1;
    return load_neighborhood(index, coordinate);
}

fn canvas_load(brush_position: vec2<f32>) -> vec4<f32> {
    return local_canvas_load(brush_to_layer(brush_position));
}

fn canvas_sample(brush_position: vec2<f32>) -> vec4<f32> {
    let document_position = brush_to_layer(brush_position);
    // Manual bilinear filtering stays correct across sparse page boundaries;
    // filtering each page texture independently would clamp at its edge.
    let base = floor(document_position - vec2<f32>(0.5)) + vec2<f32>(0.5);
    let fraction = clamp(document_position - base, vec2<f32>(0.0), vec2<f32>(1.0));
    let top = mix(
        local_canvas_load(base),
        local_canvas_load(base + vec2<f32>(1.0, 0.0)),
        fraction.x,
    );
    let bottom = mix(
        local_canvas_load(base + vec2<f32>(0.0, 1.0)),
        local_canvas_load(base + vec2<f32>(1.0, 1.0)),
        fraction.x,
    );
    return mix(top, bottom, fraction.y);
}

fn blurred_canvas_load(position: vec2<f32>, amount: f32) -> vec4<f32> {
    let radius = clamp(amount, 0.0, 1.0) * 4.0;
    if radius < 0.01 {
        return canvas_load(position);
    }
    return (canvas_load(position) * 4.0
        + canvas_load(position + vec2<f32>( radius, 0.0))
        + canvas_load(position + vec2<f32>(-radius, 0.0))
        + canvas_load(position + vec2<f32>(0.0,  radius))
        + canvas_load(position + vec2<f32>(0.0, -radius))) / 8.0;
}

fn blurred_canvas_sample(position: vec2<f32>, amount: f32) -> vec4<f32> {
    let radius = clamp(amount, 0.0, 1.0) * 4.0;
    if radius < 0.01 {
        return canvas_sample(position);
    }
    // Preserve smooth subpixel advection at the center, then use nearest
    // cardinal taps for the low-frequency softening. This reduces the sparse
    // atlas path from five manual-bilinear samples (20 texture loads) to one
    // bilinear sample plus four loads (8 total).
    return (canvas_sample(position) * 4.0
        + canvas_load(position + vec2<f32>( radius, 0.0))
        + canvas_load(position + vec2<f32>(-radius, 0.0))
        + canvas_load(position + vec2<f32>(0.0,  radius))
        + canvas_load(position + vec2<f32>(0.0, -radius))) / 8.0;
}

fn watercolor_canvas_sample(position: vec2<f32>, amount: f32) -> vec4<f32> {
    let radius = clamp(amount, 0.0, 1.0) * 4.0;
    if radius < 0.01 {
        return canvas_sample(position);
    }
    // The bilinear center preserves smooth subpixel advection. Four nearest
    // cardinal taps provide the broad watercolor softening without paying for
    // five separate manual-bilinear samples across sparse page boundaries.
    return (canvas_sample(position) * 4.0
        + canvas_load(position + vec2<f32>( radius, 0.0))
        + canvas_load(position + vec2<f32>(-radius, 0.0))
        + canvas_load(position + vec2<f32>(0.0,  radius))
        + canvas_load(position + vec2<f32>(0.0, -radius))) / 8.0;
}

fn contact_coverage(dab: Dab, world: vec2<f32>) -> f32 {
    return contact_coverage_field(dab, world, contact_field(world));
}

fn contact_coverage_field(dab: Dab, world: vec2<f32>, field: vec2<f32>) -> f32 {
    if contact_feature(1u, style.contact_a.x > 0.5) {
        return evolving_contact_prepared(world, dab.center, dab.radii, dab.rotation, dab.motion,
            dab.previous, dab.contact, dab.previous_contact, dab.hardness, field,
            dab.metric, dab.invariants.xy) * brush_selection_at(brush_to_layer(world));
    }
    let delta = world - dab.center;
    let local = rotate(delta, dab.rotation.x, -dab.rotation.y) / max(dab.radii, vec2<f32>(0.005));
    var coverage = tip_coverage(
        style.flags.x > 0.5,
        primary_texture,
        local,
        dab.texture_sign,
        dab.hardness,
        min(dab.radii.x, dab.radii.y),
    );
    if coverage <= 0.0 { return 0.0; }
    if style.flags.y > 0.5 {
        let uv = grain_uv(
            local,
            world,
            style.grain,
            style.advanced.y > 0.5,
            dab.center,
            style.advanced.w,
        );
        coverage *= mix(1.0, textureSampleLevel(grain_texture, brush_sampler, uv, 0.0).r,
            clamp(dab.material.x, 0.0, 1.0));
    }
    if style.flags.w > 0.5 {
        let shifted = local - style.dual_offset_flags.xy;
        let dual_local = rotate(shifted, style.dual.z, -style.dual.w)
            / vec2<f32>(max(style.dual.x * style.dual.y, 0.0001), max(style.dual.x, 0.0001));
        var secondary = tip_coverage(
            style.flags.z > 0.5,
            dual_texture,
            dual_local,
            dab.texture_sign,
            dab.hardness,
            min(dab.radii.x, dab.radii.y) * style.dual.x,
        );
        if style.advanced.x > 0.5 {
            let uv = grain_uv(
                dual_local,
                world,
                style.dual_grain,
                style.advanced.z > 0.5,
                dab.center + vec2<f32>(31.7, 19.3),
                style.edges.w,
            );
            secondary *= mix(1.0, textureSampleLevel(dual_grain_texture, brush_sampler, uv, 0.0).r,
                clamp(style.dual_grain.y, 0.0, 1.0));
        }
        coverage = combine_coverage(coverage, secondary, style.dual_offset_flags.z);
    }
    if coverage < style.dual_offset_flags.w { return 0.0; }
    return coverage * brush_selection_at(brush_to_layer(world));
}

fn contact_segment_progress(dab: Dab, world: vec2<f32>) -> f32 {
    // Stroke-uniform media must cover the segment between consecutive
    // contacts. If it uses isolated footprints, each pixel receives pigment
    // only at the first circular leading edge and the contact cadence appears
    // as concentric displaced-source crescents. Projecting onto the traveled
    // segment produces one continuous swept footprint without extra samples.
    let motion_squared = dot(dab.motion, dab.motion);
    if motion_squared <= 0.000001 {
        return 1.0;
    }
    let previous_center = dab.center - dab.motion;
    return clamp(dot(world - previous_center, dab.motion) / motion_squared, 0.0, 1.0);
}

fn mix_color(a: vec3<f32>, b: vec3<f32>, amount: f32) -> vec3<f32> {
    if WORKING_EXTENDED {
        if amount==0. || all(a==b) {return a;}
        if amount==1. {return b;}
    }
    if style.render_mode.z > 0.5 {
        return working_from_oklab(mix(working_to_oklab(a), working_to_oklab(b), amount));
    }
    return working_mix(a, b, amount);
}

fn blend_color(backdrop: vec3<f32>, source: vec3<f32>, mode: f32) -> vec3<f32> {
    if mode < 0.5 { return source; }
    if mode < 1.5 { return backdrop * source; }
    if mode < 2.5 { return backdrop + source - backdrop * source; }
    if mode < 3.5 { return min(backdrop + source, vec3<f32>(1.0)); }
    if mode < 4.5 { return max(backdrop - source, vec3<f32>(0.0)); }
    if mode < 5.5 { return min(backdrop, source); }
    if mode < 6.5 { return max(backdrop, source); }
    return select(
        2.0 * backdrop * source,
        1.0 - 2.0 * (1.0 - backdrop) * (1.0 - source),
        backdrop >= vec3<f32>(0.5),
    );
}

fn source_over(destination: vec4<f32>, source_color: vec3<f32>, source_alpha: f32) -> vec4<f32> {
    let da = destination.a;
    // Normal blending has no backdrop-color term. Besides avoiding an
    // unassociation and two redundant products, keep this direct form: the
    // expanded general expression produces dark contact edges in specialized
    // shaders on the Wacom's Adreno Vulkan driver (including native saved paint).
    if style.render_mode.x < 0.5 {
        if style.color.a > 0.5 {
            return vec4<f32>(mix(destination.rgb, source_color * da, source_alpha), da);
        }
        return vec4<f32>(
            destination.rgb * (1.0 - source_alpha) + source_color * source_alpha,
            source_alpha + da * (1.0 - source_alpha),
        );
    }
    let backdrop = working_unassociate(destination);
    let blended = blend_color(backdrop, source_color, style.render_mode.x);
    if style.color.a > 0.5 {
        return vec4<f32>(mix(destination.rgb, blended * da, source_alpha), da);
    }
    let rgb = destination.rgb * (1.0 - source_alpha)
        + source_color * source_alpha * (1.0 - da)
        + blended * source_alpha * da;
    return vec4<f32>(rgb, source_alpha + da * (1.0 - source_alpha));
}

fn random_unit(value: vec2<f32>) -> f32 {
    return fract(sin(dot(value, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn transport_conductance(world: vec2<f32>) -> f32 {
    if style.transport_a.x <= 0.0 {
        return 1.0;
    }
    let coordinate = rotate(
        world / 256.0,
        style.transport_a.y,
        style.transport_a.z,
    ) * style.transport_a.x;
    let sampled = textureSampleLevel(transport_texture, brush_sampler, coordinate, 0.0).r;
    let shaped = pow(clamp(sampled, 0.0, 1.0), 0.72);
    return mix(1.0, shaped, style.transport_a.w);
}

fn reservoir_load(dab: Dab, world: vec2<f32>) -> vec4<f32> {
    let delta = world - dab.center;
    let local = rotate(delta, dab.rotation.x, -dab.rotation.y)
        / max(dab.radii, vec2<f32>(0.005));
    let size = vec2<i32>(textureDimensions(reservoir_texture));
    let coordinate = clamp(
        vec2<i32>(floor((local * 0.5 + vec2<f32>(0.5)) * vec2<f32>(size))),
        vec2<i32>(0),
        size - vec2<i32>(1),
    );
    return textureLoad(reservoir_texture, coordinate, 0);
}

fn reservoir_exchange_amount(dab: Dab, coverage: f32) -> f32 {
    let pull = clamp(dab.material.y, 0.0, 1.0);
    return clamp(
        ((1.0 - style.material_a.x) * style.material_b.x + pull * 0.25)
            * coverage * dab.flow,
        0.0,
        1.0,
    );
}

fn replenished_reservoir_alpha(carried: f32, pickup: f32, exchange: f32) -> f32 {
    // Contact with transparent or partially covered canvas cannot remove
    // material from the brush. Per-dab charge already models material loss;
    // pickup only replenishes the reservoir toward a denser destination.
    return max(carried, mix(carried, pickup, exchange));
}

struct SmudgeTrace {
    coordinate: vec2<f32>,
    influence: f32,
}

fn trace_smudge(initial: vec2<f32>, first: u32, count: u32) -> SmudgeTrace {
    // Compose the contacts into one semi-Lagrangian backtrace, then sample the
    // old canvas once. Blending one shifted source copy per contact exposes
    // the dab cadence as repeated hard-edge scallops.
    var coordinate = initial;
    var optical_depth = 0.0;
    var remaining = count;
    loop {
        if remaining == 0u { break; }
        remaining -= 1u;
        let dab = dabs[first + remaining];
        let coverage = contact_coverage(dab, coordinate);
        if coverage <= 0.0 { continue; }
        let contact = clamp(
            coverage * max(style.material_b.x, dab.material.y)
                * mix(0.65, 1.0, dab.flow),
            0.0,
            0.9999,
        );
        // Integrate strength over physical stroke distance. At the canonical
        // four-percent spacing this is identical to one source-over contact;
        // denser or sparser input no longer changes the total smudge merely by
        // changing how many contacts happened to be generated.
        let diameter = max(dab.radii.x * 2.0, 0.01);
        let distance_steps = length(dab.motion) / (diameter * 0.04);
        optical_depth += -log(1.0 - contact) * distance_steps;
        let pull = clamp(dab.material.y, 0.0, 1.0);
        coordinate -= dab.motion * pull * coverage;
    }
    return SmudgeTrace(coordinate, 1.0 - exp(-optical_depth));
}

fn mix_smudged_material(original: vec4<f32>, dragged: vec4<f32>, influence: f32) -> vec4<f32> {
    // A paint blender carries color without cutting transparent holes out of
    // material already on the layer. Empty source contributes no replacement
    // color; dragged paint can still expand into an empty destination.
    let original_color = select(
        working_unassociate(dragged),
        working_unassociate(original),
        working_has_color(original.a),
    );
    let dragged_color = select(
        original_color,
        working_unassociate(dragged),
        working_has_color(dragged.a),
    );
    let alpha = max(original.a, dragged.a * influence);
    let color = mix(original_color, dragged_color, influence * dragged.a);
    return vec4<f32>(color * alpha, alpha);
}

fn watercolor_fragment(
    fragment_position: vec2<f32>,
    world: vec2<f32>,
    first: u32,
    count: u32,
) -> MaterialOutput {
    let original = canvas_load(world);
    let state_coordinate = clamp(
        vec2<i32>(floor(fragment_position)),
        vec2<i32>(0),
        vec2<i32>(255),
    );
    var stroke_coverage = textureLoad(stroke_coverage_texture, state_coordinate, 0).r;
    if style.canvas_opacity.w > 0.5 {
        stroke_coverage = 0.0;
    }

    let prior_coverage = stroke_coverage;
    var pigment_color = dabs[first].color.rgb;
    var strongest_request = 0.0;
    let prior_wetness = textureLoad(reservoir_texture, state_coordinate, 0).r;
    var trace_coordinate = world;
    var remaining = count;
    loop {
        if remaining == 0u { break; }
        remaining -= 1u;
        let dab = dabs[first + remaining];
        let coverage = contact_coverage(dab, world);
        if coverage > 0.0 {
            let requested = clamp(
                coverage * dab.flow * dab.color.a * style.material_a.y
                    * style.material_b.x * dab.material.z,
                0.0,
                1.0,
            );
            if requested > strongest_request {
                strongest_request = requested;
                pigment_color = dab.color.rgb;
            }
            if requested > stroke_coverage {
                stroke_coverage = requested;
            }
        }

        // Reverse-compose a continuous same-layer backtrace for this
        // microbatch. Advection uses a smooth analytic footprint while the
        // artist's ragged mask remains authoritative for deposition. This
        // lets adjacent contacts transport pigment coherently without
        // restarting the sampled color at each microbatch boundary.
        let trace_delta = trace_coordinate - dab.center;
        let trace_local = rotate(trace_delta, dab.rotation.x, -dab.rotation.y)
            / max(dab.radii, vec2<f32>(0.005));
        let trace_coverage = analytic_coverage(
            trace_local,
            dab.hardness,
            min(dab.radii.x, dab.radii.y),
        ) * brush_selection_at(trace_coordinate);
        if trace_coverage > 0.0 {
            trace_coordinate -= dab.motion
                * clamp(dab.material.y, 0.0, 1.0)
                * trace_coverage;
        }
    }
    // Recharge once toward full wetness from the current stroke's absolute
    // uniform-coverage target. This creates a useful pressure differential
    // against older wet paint while remaining independent of dab count and
    // microbatch boundaries. The ragged tip and paper conductance still vary
    // the deposited water spatially, but overlapping dabs cannot form bands.
    let water_load = select(1.0, style.transport_b.w, style.transport_a.x > 0.0);
    let water_charge = clamp(
        stroke_coverage * water_load
            * (0.38 + 0.62 * transport_conductance(world)),
        0.0,
        1.0,
    );
    let deposited_wetness = prior_wetness
        + (1.0 - prior_wetness) * water_charge;
    let coverage_increment = clamp(
        working_ratio(stroke_coverage - prior_coverage, 1.0 - prior_coverage),
        0.0,
        1.0,
    );
    var result = original;
    if working_has_color(coverage_increment) && style.operation.w == 0u {
        // The paint layer itself is the always-wet watercolor field. One
        // motion-directed backtrace samples only that layer. Its exchange
        // strength is independent of the radial tip value: multiplying color
        // transfer by each contact silhouette exposes circular dab bands.
        let dragged = watercolor_canvas_sample(trace_coordinate, style.material_b.z);
        let mixing = clamp(style.material_a.w * coverage_increment, 0.0, 1.0);
        if working_has_color(original.a) && working_has_color(dragged.a) && working_has_color(mixing) {
            // Exchange wet pigment only as new stroke coverage arrives. Doing
            // this for every overlapping contact would reveal microbatch
            // boundaries as concentric color bands even though alpha is
            // uniform.
            let original_color = original.rgb / original.a;
            let dragged_color = dragged.rgb / dragged.a;
            let mixed_color = mix_color(
                original_color,
                dragged_color,
                mixing * dragged.a,
            );
            result = vec4<f32>(mixed_color * original.a, original.a);
        }
        // Apply paint load to both old and new coverage targets before
        // converting their difference to source-over alpha. Scaling the
        // already-converted increment is not associative: pixels that pass
        // through a ragged antialias fringe then become solid retain visible
        // contact bands.
        let paint_load = mix(0.42, 1.0, style.material_a.x);
        let prior_pigment = prior_coverage * paint_load;
        let next_pigment = stroke_coverage * paint_load;
        let pigment = clamp(
            working_ratio(next_pigment - prior_pigment, 1.0 - prior_pigment),
            0.0,
            1.0,
        );
        result = source_over(result, pigment_color, pigment);
    }
    if style.operation.w != 0u {
        result *= 1.0 - coverage_increment;
    }
    return MaterialOutput(
        result,
        vec4<f32>(stroke_coverage, 0.0, 0.0, 1.0),
        vec4<f32>(deposited_wetness, 0.0, 0.0, 1.0),
    );
}

fn deform_coordinate(initial: vec2<f32>, first: u32, count: u32) -> vec2<f32> {
    var coordinate = initial;
    var remaining = count;
    loop {
        if remaining == 0u { break; }
        remaining -= 1u;
        let dab = dabs[first + remaining];
        let coverage = contact_coverage(dab, coordinate);
        if coverage <= 0.0 { continue; }
        let strength = clamp(dab.material.w * coverage, 0.0, 1.0);
        let motion = dab.motion * (1.0 + style.deformation.w);
        let mode = style.deformation.x;
        if mode < 0.5 {
            coordinate -= motion * strength;
        } else if mode < 2.5 {
            let direction = select(-1.0, 1.0, mode < 1.5);
            let angle = direction * strength * 0.75;
            coordinate = dab.center + rotate(coordinate - dab.center, cos(angle), sin(angle));
        } else if mode < 4.5 {
            let direction = select(1.0, -1.0, mode < 3.5);
            coordinate = dab.center + (coordinate - dab.center) * (1.0 + direction * strength * 0.35);
        } else if mode < 5.5 {
            let noise = random_unit(floor(coordinate * 0.25) + dab.center);
            coordinate += (vec2<f32>(noise, fract(noise * 7.13)) - 0.5)
                * strength * style.render_mode.w * 24.0;
        } else {
            let perpendicular = vec2<f32>(-motion.y, motion.x);
            coordinate -= normalize(perpendicular + vec2<f32>(0.0001)) * strength * length(motion);
        }
    }
    return coordinate;
}

struct MaterialOutput {
    @location(0) color: vec4<f32>,
    @location(1) coverage: vec4<f32>,
    @location(2) wetness: vec4<f32>,
}

fn wet_fragment(
    fragment_position: vec2<f32>,
    world: vec2<f32>,
    first: u32,
    count: u32,
) -> MaterialOutput {
    let original = canvas_load(world);
    let state_coordinate = clamp(
        vec2<i32>(floor(fragment_position)),
        vec2<i32>(0),
        vec2<i32>(255),
    );
    var stroke_coverage = textureLoad(stroke_coverage_texture, state_coordinate, 0).r;
    if style.canvas_opacity.w > 0.5 {
        stroke_coverage = 0.0;
    }
    var deposited_wetness = 0.0;
    var batch_color = vec3<f32>(0.0);
    var batch_alpha = 0.0;
    for (var offset = 0u; offset < count; offset += 1u) {
        let dab = dabs[first + offset];
        var contact_dab = dab;
        var contact_progress = 1.0;
        if contact_uniform() {
            contact_progress = contact_segment_progress(dab, world);
            contact_dab.center = dab.center - dab.motion * (1.0 - contact_progress);
        }
        let coverage = contact_coverage(contact_dab, world);
        if coverage <= 0.0 { continue; }
        let wet_jitter = mix(1.0, random_unit(world + dab.center), style.material_b.w);
        var carried = reservoir_load(contact_dab, world);
        if style.canvas_opacity.w > 0.5 {
            carried = vec4<f32>(dab.color.rgb, style.material_a.x * dab.material.z);
        }
        let pull = clamp(dab.material.y, 0.0, 1.0);
        let pickup = blurred_canvas_load(
            world - dab.motion * pull,
            style.material_b.z,
        );
        let empty_color = select(dab.color.rgb, carried.rgb, working_has_color(carried.a));
        let pickup_color = select(
            empty_color,
            working_unassociate(pickup),
            working_has_color(pickup.a),
        );
        let carried_weight = carried.a * (1.0 - style.material_a.w);
        let paint_color = mix_color(pickup_color, carried.rgb, carried_weight);
        let available_material = max(
            pickup.a,
            carried.a * (1.0 - style.material_a.w * 0.5),
        );
        var source_alpha = clamp(
            coverage * dab.flow * dab.color.a * style.material_a.y
                * style.material_b.x * dab.material.z * wet_jitter * available_material,
            0.0,
            1.0,
        );
        deposited_wetness = max(
            deposited_wetness,
            coverage * style.material_a.z * wet_jitter,
        );
        if contact_uniform() {
            let next_coverage = max(stroke_coverage, source_alpha);
            source_alpha = clamp(
                working_ratio(next_coverage - stroke_coverage, 1.0 - stroke_coverage),
                0.0,
                1.0,
            );
            stroke_coverage = next_coverage;
        } else if MATERIAL_OPERATION != OP_DEPOSIT {
            stroke_coverage = max(stroke_coverage, source_alpha);
        }
        batch_color = batch_color * (1.0 - source_alpha) + paint_color * source_alpha;
        batch_alpha = source_alpha + batch_alpha * (1.0 - source_alpha);
    }
    var result = original;
    if style.operation.w != 0u {
        result *= 1.0 - batch_alpha;
    } else if working_has_color(batch_alpha) {
        result = source_over(original, batch_color / batch_alpha, batch_alpha);
    }
    return MaterialOutput(
        result,
        vec4<f32>(stroke_coverage, 0.0, 0.0, 1.0),
        vec4<f32>(deposited_wetness, 0.0, 0.0, 1.0),
    );
}

fn paint_fragment(fragment_position: vec4<f32>) -> MaterialOutput {
    let world = layer_to_brush(render_target.origin_extent.xy + fragment_position.xy);
    let first = style.operation.x;
    let count = style.operation.y;
    if MATERIAL_OPERATION == OP_WATERCOLOR {
        return watercolor_fragment(fragment_position.xy, world, first, count);
    }
    if MATERIAL_OPERATION == OP_WET {
        return wet_fragment(fragment_position.xy, world, first, count);
    }
    if MATERIAL_OPERATION == OP_SMUDGE {
        let original = canvas_load(world);
        let trace = trace_smudge(world, first, count);
        var dragged = vec4<f32>(0.0);
        if material_sources.header.x == 2u {
            dragged = textureLoad(reservoir_texture, vec2<i32>(floor(fragment_position.xy)), 0);
        } else {
            dragged = blurred_canvas_sample(trace.coordinate, style.material_b.z);
        }
        return MaterialOutput(
            mix_smudged_material(original, dragged, trace.influence),
            vec4<f32>(0.0),
            vec4<f32>(0.0),
        );
    }
    if MATERIAL_OPERATION == OP_LIQUIFY {
        if material_sources.header.x == 2u {
            return MaterialOutput(
                textureLoad(reservoir_texture, vec2<i32>(floor(fragment_position.xy)), 0),
                vec4<f32>(0.0), vec4<f32>(0.0),
            );
        }
        return MaterialOutput(
            canvas_sample(deform_coordinate(world, first, count)),
            vec4<f32>(0.0),
            vec4<f32>(0.0),
        );
    }

    // Dry deposition reads exactly the destination texel, even for a placed
    // layer. Avoid the brush-to-layer round trip and neighborhood selection.
    var result = dry_original(vec2<i32>(floor(fragment_position.xy)));
    let state_coordinate = clamp(
        vec2<i32>(floor(fragment_position.xy)),
        vec2<i32>(0),
        vec2<i32>(255),
    );
    var stroke_coverage = textureLoad(stroke_coverage_texture, state_coordinate, 0).r;
    if style.canvas_opacity.w > 0.5 {
        stroke_coverage = 0.0;
    }
    // Fully covered incoming pixels cannot receive more uniform pigment.
    // Keep this outside the contact loop: a loop-carried early break produces
    // dark contact seams on Adreno for large, multi-contact batches.
    let range = material_sources.header.zw;
    let tooth = contact_paper(world);
    if CONTACT_FILM_CULL && contact_uniform() && style.render_mode.x < 0.5 {
        var ceiling = 1.0;
        if contact_feature(1u, style.contact_a.x > 0.5) && range.y > 0u {
            ceiling = dabs[range.x].invariants.z;
            var density = 1.0;
            if contact_feature(64u, style.contact_a.z > 0.0 || style.contact_c.z > 0.0) {
                let bias = clamp(style.contact_a.z + style.contact_c.z, 0.0, 0.95);
                density = mix(1.0, 1.6, bias);
            }
            var response = density;
            if contact_feature(2u, style.contact_a.y > 0.0) {
                let penetration = clamp(dabs[range.x].invariants.w * style.contact_c.x * density, 0.0, 1.0);
                let threshold = 0.62 - penetration * 0.3;
                let paper = smoothstep(threshold - 0.08, threshold + 0.08, tooth);
                response *= mix(1.0, paper, style.contact_a.y);
            }
            ceiling *= min(response, 1.0);
        }
        if stroke_coverage >= ceiling {
            return MaterialOutput(result, vec4<f32>(stroke_coverage, 0.0, 0.0, 1.0), vec4<f32>(0.0));
        }
    }
    let field = contact_field_with_paper(world, tooth);
    for (var offset = 0u; offset < range.y; offset += 1u) {
        let dab = dabs[range.x + offset];
        let coverage = contact_coverage_field(dab, world, field);
        if coverage <= 0.0 { continue; }
        var requested_alpha = clamp(
            (dab.flow * dab.color.a) * coverage,
            0.0,
            1.0,
        );
        if contact_feature(1u, style.contact_a.x > 0.5) && !contact_uniform() {
            let selected = brush_selection_at(brush_to_layer(world));
            let unselected_coverage = coverage / max(selected, 0.000001);
            requested_alpha = (1.0 - exp(-unselected_coverage * dab.flow * dab.color.a * 6.0)) * selected;
        }
        var source_alpha = requested_alpha;
        if contact_uniform() {
            // Attachment-based hosts store R8 coverage. Match that storage
            // before applying its delta so frame boundaries cannot change ink.
            // Native SDR uses R32Float and must retain faint/16-bit coverage.
            if !WORKING_EXTENDED {
                requested_alpha = round(requested_alpha * 255.0) / 255.0;
            }
            let next_coverage = max(stroke_coverage, requested_alpha);
            source_alpha = clamp(
                working_ratio(next_coverage - stroke_coverage, 1.0 - stroke_coverage),
                0.0,
                1.0,
            );
            stroke_coverage = next_coverage;
        } else if MATERIAL_OPERATION == OP_COVERAGE {
            stroke_coverage = max(stroke_coverage, requested_alpha);
        }
        if style.operation.w != 0u { result *= 1.0 - source_alpha; }
        else { result = source_over(result, dab.color.rgb, source_alpha); }
    }
    return MaterialOutput(
        result,
        vec4<f32>(stroke_coverage, 0.0, 0.0, 1.0),
        vec4<f32>(0.0),
    );
}

@fragment
fn gather_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let world = layer_to_brush(render_target.origin_extent.xy + position.xy);
    var value = vec4<f32>(0.0);
    if MATERIAL_OPERATION == OP_SMUDGE {
        let trace = trace_smudge(world, style.operation.x, style.operation.y);
        value = blurred_canvas_sample(trace.coordinate, style.material_b.z);
    } else {
        value = canvas_sample(deform_coordinate(world, style.operation.x, style.operation.y));
    }
    // Each source page occurs in exactly one pass. Linear filtering weights
    // therefore sum exactly without requiring Float32 hardware blending.
    return value + textureLoad(reservoir_texture, vec2<i32>(floor(position.xy)), 0);
}

fn material_result(fragment_position: vec4<f32>) -> MaterialOutput {
    if MATERIAL_OPERATION == OP_DEPOSIT || MATERIAL_OPERATION == OP_COVERAGE {
        let p = vec2<u32>(fragment_position.xy);
        let bounds = material_sources.pages[0];
        if any(p < bounds.xy) || any(p >= bounds.zw) {
            return MaterialOutput(
                dry_original(vec2<i32>(p)),
                vec4<f32>(textureLoad(stroke_coverage_texture, vec2<i32>(p), 0).r, 0.0, 0.0, 1.0),
                vec4<f32>(0.0),
            );
        }
    }
    var result = paint_fragment(fragment_position);
    if style.color.a > 0.5 {
        let world = layer_to_brush(render_target.origin_extent.xy + fragment_position.xy);
        var original: vec4<f32>;
        if MATERIAL_OPERATION == OP_DEPOSIT || MATERIAL_OPERATION == OP_COVERAGE {
            original = dry_original(vec2<i32>(floor(fragment_position.xy)));
        } else { original = canvas_load(world); }
        if style.operation.w != 0u { result.color = original; }
        else {
            result.color = vec4<f32>(working_unassociate(result.color) * original.a, original.a);
        }
        result.wetness *= select(0.0, 1.0, original.a > 0.0);
    }
    return result;
}

@fragment
fn reservoir_fragment(@builtin(position) fragment_position: vec4<f32>) -> @location(0) vec4<f32> {
    let first = style.operation.x;
    let count = style.operation.y;
    if count == 0u {
        return vec4<f32>(0.0);
    }
    let reservoir_size = vec2<f32>(textureDimensions(reservoir_texture));
    let local = fragment_position.xy / reservoir_size * 2.0 - vec2<f32>(1.0);
    var carried = textureLoad(
        reservoir_texture,
        clamp(vec2<i32>(floor(fragment_position.xy)), vec2<i32>(0), vec2<i32>(reservoir_size) - 1),
        0,
    );
    // Advance every contact, not merely the final contact submitted this
    // frame. The reservoir is loaded once at stroke start; selected color is
    // deliberately absent here so canvas color can replace and be carried by
    // the brush instead of being overwritten on every dab.
    for (var offset = 0u; offset < count; offset += 1u) {
        let dab = dabs[first + offset];
        let scaled = local * dab.radii;
        let world = dab.center + rotate(scaled, dab.rotation.x, dab.rotation.y);
        let coverage = contact_coverage(dab, world);
        if coverage <= 0.0 { continue; }
        let pull = clamp(dab.material.y, 0.0, 1.0);
        let pickup = blurred_canvas_load(world - dab.motion * pull, style.material_b.z);
        if !working_has_color(pickup.a) { continue; }
        let exchange = reservoir_exchange_amount(dab, coverage);
        let alpha_exchange = exchange * (1.0 - style.material_a.w * 0.5);
        carried = vec4<f32>(
            mix_color(carried.rgb, pickup.rgb / pickup.a, exchange),
            replenished_reservoir_alpha(carried.a, pickup.a, alpha_exchange),
        );
    }
    return carried;
}

@fragment
fn fragment_main(@builtin(position) fragment_position: vec4<f32>) -> MaterialOutput {
    return material_result(fragment_position);
}
// dry_material::shader supplies material_color_output and dry_original.
@group(0) @binding(2) var material_coverage_output: texture_storage_2d<r32float, write>;
override MATERIAL_IN_PLACE: bool = false;
@compute @workgroup_size(32, 2)
fn compute_color(@builtin(global_invocation_id) id: vec3<u32>) {
    let result = material_result(vec4<f32>(vec2<f32>(id.xy) + 0.5, 0.0, 1.0));
    if !MATERIAL_IN_PLACE || any(result.color != dry_original(vec2<i32>(id.xy))) {
        textureStore(material_color_output, vec2<i32>(id.xy), result.color);
    }
}
@compute @workgroup_size(32, 2)
fn compute_coverage(@builtin(global_invocation_id) id: vec3<u32>) {
    let result = material_result(vec4<f32>(vec2<f32>(id.xy) + 0.5, 0.0, 1.0));
    if !MATERIAL_IN_PLACE || any(result.color != dry_original(vec2<i32>(id.xy))) {
        textureStore(material_color_output, vec2<i32>(id.xy), result.color);
    }
    textureStore(material_coverage_output, vec2<i32>(id.xy), result.coverage);
}
