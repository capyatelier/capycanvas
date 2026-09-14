// Shared GPU contact model. Both deposition paths call this exact evaluator.
// No canvas color is sampled: paper, contact geometry and pigment supply are
// independent of the already-painted image. A future brush-state compute pass
// can provide the same pair of contact poses.

fn contact_hash(p: vec2<f32>) -> f32 {
    let cell = vec2<i32>(p);
    var h = bitcast<u32>(cell.x) * 1597334677u ^ bitcast<u32>(cell.y) * 3812015801u;
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

fn contact_exposure(motion: vec2<f32>, radii: vec2<f32>, rotation: vec2<f32>, hardness: f32) -> f32 {
    if dot(motion, motion) < 0.000001 { return 0.08; }
    let axes = max(radii, vec2<f32>(0.005));
    let diameter = 2.0 * sqrt(axes.x * axes.y);
    // A swept ellipse adds a rectangular strip to its footprint. Compensate
    // for that extra area before depositing, so widening the contact spacing
    // cannot add more graphite. Soft tips use their effective contact radius.
    let sweep = length(rotate(motion, rotation.x, -rotation.y) / axes);
    let effective_radius = 0.5 + 0.5 * hardness;
    return length(motion) / diameter / (1.0 + 0.63661977 * sweep / effective_radius);
}

fn evolving_contact(
    world: vec2<f32>, center: vec2<f32>, radii: vec2<f32>, rotation: vec2<f32>,
    motion: vec2<f32>, previous: vec4<f32>, sensors: vec4<f32>,
    previous_sensors: vec4<f32>, hardness: f32,
) -> f32 {
    let distance2 = dot(motion, motion);
    let start = center - motion;
    // Project in the nib's metric, not screen distance. A screen-space closest
    // point leaves scallops between successive narrow, angled nibs.
    let middle_axes = max((previous.xy + radii) * 0.5, vec2<f32>(0.005));
    let middle_angle = previous.zw + rotation;
    let middle_rotation = select(rotation, middle_angle / max(length(middle_angle), 0.00001), dot(middle_angle, middle_angle) > 0.00001);
    let nib_motion = rotate(motion, middle_rotation.x, -middle_rotation.y) / middle_axes;
    let nib_offset = rotate(world - start, middle_rotation.x, -middle_rotation.y) / middle_axes;
    let projected = dot(nib_offset, nib_motion) / max(dot(nib_motion, nib_motion), 0.000001);
    let progress = select(1.0, clamp(projected, 0.0, 1.0), distance2 > 0.000001);
    let axes = max(mix(previous.xy, radii, progress), vec2<f32>(0.005));
    let angle = mix(previous.zw, rotation, progress);
    let orientation = select(rotation, angle / max(length(angle), 0.00001), dot(angle, angle) > 0.00001);
    let local = rotate(world - mix(start, center, progress), orientation.x, -orientation.y) / axes;
    let radius = length(local);
    if radius > 1.45 { return 0.0; }
    let input = mix(previous_sensors, sensors, progress);
    let pressure = clamp(input.x, 0.0, 1.0);
    let tilt = clamp(input.y, 0.0, 1.0);
    let aa = min(1.0 / max(min(axes.x, axes.y), 0.005), 1.0);

    var boundary = 1.0;
    if style.contact_a.w > 0.0 {
        // Stationary, multiscale edge variation; no contact-index randomness.
        let p = world / style.contact_b.x;
        let variation = contact_noise(p) * 0.7 + contact_noise(p * 2.17 + vec2<f32>(17.3)) * 0.3;
        boundary += (variation - 0.5) * style.contact_a.w * (1.5 - pressure * 0.5);
    }
    let pool = style.contact_b.w;
    boundary += pool * 0.12 * (0.3 + 0.7 * pressure);
    let feather = max(1.0 - hardness, aa);
    var coverage = 1.0 - smoothstep(boundary - feather, boundary + aa * 0.5, radius);
    if coverage <= 0.0 { return 0.0; }

    let bias = clamp(style.contact_a.z + style.contact_c.z * tilt, 0.0, 0.95);
    // The point side is -X in the stylus frame; the opposite side is soft.
    let density = mix(1.0, clamp(0.8 - local.x * 0.8, 0.03, 1.6), bias);
    if style.contact_a.y > 0.0 {
        // Paper UV excludes stroke seeds, contact motion and the stylus pose.
        let uv = rotate(world / vec2<f32>(textureDimensions(grain_texture)), style.grain.z, style.grain.w) * style.grain.x;
        let tooth = textureSampleLevel(grain_texture, brush_sampler, uv, 0.0).r;
        let penetration = clamp(pressure * style.contact_c.x * density, 0.0, 1.0);
        let threshold = 0.7 - penetration * 0.58;
        let contact = smoothstep(threshold - 0.12, threshold + 0.12, tooth);
        coverage *= mix(1.0, contact * (0.48 + tooth * 0.52), style.contact_a.y);
    }
    coverage *= density;

    let load = exp(-style.contact_c.y * input.z);
    if style.contact_b.z > 0.0 && sensors.z > 0.0 {
        // Strand identities remain coherent along the stroke. Supply decreases
        // gradually, revealing gaps rather than independently random speckles.
        let across = (local.y * 0.5 + 0.5) * style.contact_b.y;
        let strand = across + (contact_noise(vec2<f32>(across * 0.31, sensors.w)) - 0.5) * 2.5;
        let id = floor(strand);
        let variation = contact_noise(vec2<f32>(id, input.z * 0.35 + sensors.w));
        let width = clamp(0.3 + pressure * 0.45 + load * 0.25 - variation * 0.55, 0.1, 0.9);
        let band = abs(fract(strand) - 0.5) * 2.0;
        let strand_aa = min(style.contact_b.y / max(axes.y * 2.0, 1.0), 0.6);
        let fibers = 1.0 - smoothstep(width - strand_aa, width + strand_aa, band);
        let separation = style.contact_b.z * (1.0 - pressure * 0.45)
            * smoothstep(0.12, 0.78, abs(local.y));
        coverage *= mix(1.0, fibers, separation);
        // A drying bristle loses contact intermittently; it does not turn all
        // of the ink translucent. Strand identity fixes those gaps in the brush.
        let supply = smoothstep(0.08, 0.3, load - variation * 0.65);
        coverage *= mix(1.0, supply, style.contact_b.z);
    }
    // Pooling deepens the continuous perimeter while retaining a translucent
    // body. The uniform deposition path grows this same deposit across dabs.
    // A moving nib pools across the stroke, not around each contact's front
    // cap. Radial cap rings would survive max accumulation as a stamped pattern.
    let transverse = length(nib_offset - nib_motion * projected);
    let rim_radius = select(0.0, transverse, distance2 > 0.000001);
    let rim = smoothstep(0.55, 0.95, rim_radius);
    coverage *= mix(1.0, 0.98 + rim * 0.02, pool)
        * mix(1.0, load, 0.85 * (1.0 - style.contact_b.z));
    return clamp(coverage, 0.0, 1.0);
}
