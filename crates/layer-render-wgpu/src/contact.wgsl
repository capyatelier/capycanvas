// Shared GPU contact model. Both deposition paths call this exact evaluator.
// No canvas color is sampled: paper, contact geometry and pigment supply are
// independent of the already-painted image. A future brush-state compute pass
// can provide the same pair of contact poses.

// Dry compute specializes the same evaluator by material features. Other
// consumers retain the dynamic evaluator; custom combinations fall back to it.
override CONTACT_FLAGS: u32 = 4294967295u;
fn contact_feature(bit: u32, dynamic: bool) -> bool {
    return select((CONTACT_FLAGS & bit) != 0u, dynamic, CONTACT_FLAGS == 4294967295u);
}

fn contact_hash(p: vec2<f32>) -> f32 {
    let cell = vec2<i32>(p);
    var h = (bitcast<u32>(cell.x) * 1597334677u) ^ (bitcast<u32>(cell.y) * 3812015801u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return f32(h ^ (h >> 16u)) / 4294967295.0;
}

fn contact_noise(p: vec2<f32>) -> f32 {
    let cell = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(contact_hash(cell), contact_hash(cell + vec2<f32>(1.0, 0.0)), u.x),
        mix(contact_hash(cell + vec2<f32>(0.0, 1.0)), contact_hash(cell + vec2<f32>(1.0)), u.x), u.y);
}

// Stationary material fields are shared by all spans touching this pixel.
fn contact_field(world: vec2<f32>) -> vec2<f32> {
    var field = vec2<f32>(0.5, 1.0);
    if contact_feature(4u, style.contact_a.w > 0.0) {
        let p = world / style.contact_b.x;
        field.x = contact_noise(p) * 0.7 + contact_noise(p * 2.17 + vec2<f32>(17.3)) * 0.3;
    }
    if contact_feature(2u, style.contact_a.y > 0.0) {
        let uv = rotate(world / vec2<f32>(textureDimensions(grain_texture)), style.grain.z, style.grain.w) * style.grain.x;
        field.y = textureSampleLevel(grain_texture, brush_sampler, uv, 0.0).r;
    }
    return field;
}

fn evolving_contact_prepared(
    world: vec2<f32>, center: vec2<f32>, radii: vec2<f32>, rotation: vec2<f32>,
    motion: vec2<f32>, previous: vec4<f32>, sensors: vec4<f32>,
    previous_sensors: vec4<f32>, hardness: f32, field: vec2<f32>,
    metric: vec4<f32>, invariants: vec2<f32>,
) -> f32 {
    let distance2 = dot(motion, motion);
    let start = center - motion;
    // The upload resolves the midpoint nib metric once for all touched pixels.
    let nib_motion = vec2<f32>(dot(motion, metric.xy), dot(motion, metric.zw));
    let nib_offset = vec2<f32>(dot(world - start, metric.xy), dot(world - start, metric.zw));
    let projected = dot(nib_offset, nib_motion) / max(dot(nib_motion, nib_motion), 0.000001);
    var progress = select(1.0, clamp(projected, 0.0, 1.0), distance2 > 0.000001);
    if distance2 > 0.000001 && any(abs(previous.xy - radii) > vec2<f32>(0.000001)) {
        let middle_axes = max((previous.xy + radii) * 0.5, vec2<f32>(0.005));
        // Minimize |offset - motion*t|² / (r0 + (r1-r0)*t)².
        // Center-line projection ignores changing radius and leaves necks
        // between tapered spans. This is exact for a fixed-aspect nib; a
        // changing aspect/orientation uses the same mean nib metric above.
        let relative_start = previous.xy / middle_axes;
        let relative_end = radii / middle_axes;
        let r0 = max((relative_start.x + relative_start.y) * 0.5, 0.000001);
        let r1 = max((relative_end.x + relative_end.y) * 0.5, 0.000001);
        let slope = r1 - r0;
        let a = dot(nib_offset, nib_motion);
        let b = dot(nib_motion, nib_motion);
        let c = dot(nib_offset, nib_offset);
        let denominator = b * r0 + slope * a;
        if denominator > 0.000001 {
            progress = clamp((a * r0 + slope * c) / denominator, 0.0, 1.0);
        } else {
            // A stationary point here is a maximum; choose an endpoint.
            progress = select(0.0, 1.0, (c - 2.0 * a + b) / (r1 * r1) < c / (r0 * r0));
        }
    }
    var axes = max(radii, vec2<f32>(0.005));
    var orientation = rotation;
    if any(previous != vec4<f32>(radii, rotation)) {
        axes = max(mix(previous.xy, radii, progress), vec2<f32>(0.005));
        let angle = mix(previous.zw, rotation, progress);
        orientation = select(rotation, angle / max(length(angle), 0.00001), dot(angle, angle) > 0.00001);
    }
    let local = rotate(world - mix(start, center, progress), orientation.x, -orientation.y) / axes;
    let radius = length(local);
    if radius > 1.45 { return 0.0; }
    let input = mix(previous_sensors, sensors, progress);
    let pressure = clamp(input.x, 0.0, 1.0);
    let tilt = clamp(input.y, 0.0, 1.0);
    let aa = min(1.0 / max(min(axes.x, axes.y), 0.005), 1.0);

    var boundary = 1.0;
    if contact_feature(4u, style.contact_a.w > 0.0) {
        // Stationary, multiscale edge variation; no contact-index randomness.
        boundary += (field.x - 0.5) * style.contact_a.w * (1.5 - pressure * 0.5);
    }
    let pool = select(0.0, style.contact_b.w, contact_feature(16u, style.contact_b.w > 0.0));
    boundary += pool * 0.12 * (0.3 + 0.7 * pressure);
    let feather = max(1.0 - hardness, aa);
    var coverage = 1.0 - smoothstep(boundary - feather, boundary + aa * 0.5, radius);
    if style.render_mode.y < 0.5 {
        // Integrate a compact parabolic pigment kernel along the actual span.
        // Adjacent spans partition the integral, so there are no overlapping
        // cap deposits or periodic dots as contact spacing changes.
        // Use the closest contact pose for a changing nib. Adjacent spans
        // then meet at the same pose rather than at two different midpoints.
        var integral_motion = nib_motion;
        var integral_offset = nib_offset;
        if any(previous != vec4<f32>(radii, rotation)) {
            integral_motion = rotate(motion, orientation.x, -orientation.y) / axes;
            integral_offset = rotate(world - start, orientation.x, -orientation.y) / axes;
        }
        let integral_projected = dot(integral_offset, integral_motion)
            / max(dot(integral_motion, integral_motion), 0.000001);
        let travel = length(integral_motion);
        let q = max(boundary * boundary - dot(integral_offset - integral_motion * integral_projected,
            integral_offset - integral_motion * integral_projected), 0.0);
        let reach = sqrt(q);
        let a = clamp(-integral_projected * travel, -reach, reach);
        let b = clamp((1.0 - integral_projected) * travel, -reach, reach);
        let parabolic = max(q * (b - a) - (b*b*b - a*a*a) / 3.0, 0.0);
        // Graphite side/point contacts have a pressure distribution that falls
        // to zero at the edge. Even a small constant-density kernel component
        // leaves faint hard facets when a broad pencil changes tilt or size.
        let firmness = select(invariants.y, 0.0,
            contact_feature(64u, style.contact_a.z > 0.0 || style.contact_c.z > 0.0));
        let hard_weight = firmness * 0.7;
        var integral = mix(parabolic, b - a, hard_weight);
        if contact_feature(64u, style.contact_a.z > 0.0 || style.contact_c.z > 0.0) {
            // Hardness still controls graphite's pressure profile. Interpolate
            // two kernels that both vanish at the edge, rather than adding a
            // discontinuous flat component to a soft pencil's deposit.
            let a3 = a * a * a;
            let b3 = b * b * b;
            let quartic = max(q * q * (b - a) - (2.0 / 3.0) * q * (b3 - a3)
                + (b3 * b * b - a3 * a * a) * 0.2, 0.0);
            integral = mix(quartic, parabolic, hardness);
        }
        let normalization = invariants.x / max(travel * 2.0 * sqrt(axes.x * axes.y), 0.000001);
        // A firm, pigment-fed nib delivers an even strip across its width.
        // Softer media retain the rounded density profile. Normalize by the
        // complete transverse integral, not the current segment's length.
        let section = 2.0 * reach * mix(q * (2.0 / 3.0), 1.0, hard_weight);
        let feed = mix(1.0, 1.0 / max(section, 0.001), firmness);
        // A fully fed chisel supplies one transverse dose in every direction.
        // Keeping the soft-tip area factor here makes a held marker darken
        // solely because its direction changes, like a shaded plastic ribbon.
        let directional_feed = mix(normalization, 1.0, firmness);
        coverage = integral * directional_feed * feed * smoothstep(0.0, aa * 2.0, q);
        if distance2 <= 0.000001 {
            coverage = 0.08 * max(1.0 - radius * radius, 0.0);
        }
    }
    if coverage <= 0.0 { return 0.0; }

    let bias = select(0.0, clamp(style.contact_a.z + style.contact_c.z * tilt, 0.0, 0.95),
        contact_feature(64u, style.contact_a.z > 0.0 || style.contact_c.z > 0.0));
    // The point side is -X in the stylus frame; the opposite side is soft.
    let density = mix(1.0, clamp(0.8 - local.x * 0.8, 0.03, 1.6), bias);
    if contact_feature(2u, style.contact_a.y > 0.0) {
        // Paper UV excludes stroke seeds, contact motion and the stylus pose.
        let tooth = field.y;
        let penetration = clamp(pressure * style.contact_c.x * density, 0.0, 1.0);
        // Pressure fills stationary tooth gradually; uniform dry deposits
        // catch the peaks without a translucent film over the whole paper.
        var contact = smoothstep(0.5 - penetration * 0.5, 0.85 - penetration * 0.35, tooth)
            * (0.3 + penetration * 0.7);
        if style.render_mode.y > 0.5 {
            let threshold = 0.62 - penetration * 0.3;
            contact = smoothstep(threshold - 0.08, threshold + 0.08, tooth);
        }
        coverage *= mix(1.0, contact, style.contact_a.y);
    }
    coverage *= density;

    var load = 1.0;
    if contact_feature(32u, style.contact_c.y > 0.0) { load = exp(-style.contact_c.y * input.z); }
    if contact_feature(8u, style.contact_b.z > 0.0) && sensors.z > 0.0 {
        // Strand identities remain coherent along the stroke. Supply decreases
        // gradually, revealing gaps rather than independently random speckles.
        var strand_y = local.y;
        if distance2 > 0.000001 {
            // Continue a turning nib's hairs along its local curvature through
            // the end caps. Straight tangent extensions intersect as wedges
            // when neighboring wide contacts meet on a curved path.
            let turn = previous.z * rotation.y - previous.w * rotation.x;
            let along_nib = local.x * axes.x;
            strand_y -= 0.5 * turn * along_nib * along_nib
                / max(invariants.x * axes.y, 0.000001);
        }
        let across = (strand_y * 0.5 + 0.5) * style.contact_b.y;
        // Extend stroke distance through the end caps. Finite loading
        // patches keep turning bristles from leaving crossing wedges.
        let along = previous_sensors.z + dot(world - start, motion)
            * (sensors.z - previous_sensors.z) / max(distance2, 0.000001);
        let variation = contact_noise(vec2<f32>(across,
            along * style.contact_b.y * 0.08 + sensors.w));
        let fibers = smoothstep(0.1, 0.72 - pressure * 0.12, variation);
        let separation = style.contact_b.z * (1.0 - pressure * 0.45)
            * smoothstep(0.12, 0.78, abs(local.y));
        coverage *= mix(1.0, fibers, separation);
        // A drying bristle loses contact intermittently; it does not turn all
        // of the ink translucent. Strand identity fixes those gaps in the brush.
        let supply = smoothstep(0.08, 0.3, load - variation * 0.65);
        coverage *= mix(1.0, supply, style.contact_b.z);
    }
    coverage *= mix(1.0, load, 0.85 * (1.0 - style.contact_b.z));
    return clamp(coverage, 0.0, 1.0);
}
