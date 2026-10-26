struct MapSample { displacement: vec2<f32>, footprint: mat2x2<f32> }
struct Trace { position: vec2<f32>, footprint: mat2x2<f32>, restore: f32, gradient: vec2<f32> }
struct Influence { value: f32, gradient: vec2<f32> }

const IDENTITY = mat2x2<f32>(vec2(1., 0.), vec2(0., 1.));

fn outer(a: vec2<f32>, b: vec2<f32>) -> mat2x2<f32> {
    return mat2x2<f32>(a * b.x, a * b.y);
}

fn finite_bfloat(value: f32) -> u32 {
    let bounded = select(0., clamp(value, -3.38953139e38, 3.38953139e38), value == value);
    let bits = bitcast<u32>(bounded);
    return (bits + 32767u + ((bits >> 16u) & 1u)) >> 16u;
}

fn pack_footprint(v: vec2<f32>) -> u32 {
    return finite_bfloat(v.x) | (finite_bfloat(v.y) << 16u);
}

fn unpack_footprint(v: u32) -> vec2<f32> {
    return bitcast<vec2<f32>>(vec2(v << 16u, v & 4294901760u));
}

fn pack_map(value: MapSample) -> vec4<u32> {
    return vec4(bitcast<vec2<u32>>(value.displacement), pack_footprint(value.footprint[0]), pack_footprint(value.footprint[1]));
}

fn unpack_map(value: vec4<u32>) -> MapSample {
    return MapSample(bitcast<vec2<f32>>(value.xy), mat2x2(unpack_footprint(value.z), unpack_footprint(value.w)));
}

fn liquify_hash(input: u32) -> u32 {
    var value = input;
    value = (value ^ (value >> 16u)) * 2146121005u;
    value = (value ^ (value >> 15u)) * 2221713035u;
    return value ^ (value >> 16u);
}

fn crystal_direction(at: vec2<f32>, dab: Dab) -> vec2<f32> {
    let cell = vec2<i32>(floor(at * .25));
    let seed = liquify_hash(bitcast<u32>(cell.x) ^ liquify_hash(bitcast<u32>(cell.y))
        ^ u32(dab.contact.w) ^ liquify_hash(u32(dab.previous_contact.y)));
    return vec2(f32(liquify_hash(seed ^ 2654435769u) >> 8u),
        f32(liquify_hash(seed ^ 2246822507u) >> 8u)) * (1. / 16777216.);
}

fn liquify_influence(dab: Dab, at: vec2<f32>) -> Influence {
    if LIQUIFY_ANALYTIC {
        let local = rotate(at - dab.center, dab.rotation.x, -dab.rotation.y) / dab.radii;
        let squared = dot(local, local);
        if squared >= 1. { return Influence(0., vec2(0.)); }
        let edge = max(1. - dab.hardness, 1. / min(dab.radii.x, dab.radii.y));
        let solid = max(1. - edge, 0.);
        var shape = 1.;
        var gradient = vec2(0.);
        if squared > solid * solid {
            let radius = sqrt(squared);
            shape = clamp((1. - radius) / edge, 0., 1.);
            gradient = -rotate(local / dab.radii, dab.rotation.x, dab.rotation.y) / (edge * radius);
        }
        if shape < style.edges.w { return Influence(0., vec2(0.)); }
        let value = clamp(dab.material.w * shape, 0., 1.);
        if value <= 0. || value >= 1. { return Influence(value, vec2(0.)); }
        return Influence(value, gradient * dab.material.w);
    }
    let coverage = contact_coverage(dab, at);
    let value = clamp(dab.material.w * coverage, 0., 1.);
    if value <= 0. || value >= 1. { return Influence(value, vec2(0.)); }
    var gradient = vec2(0.);
    if style.flags.x > .5 && style.flags.y <= .5 {
        let delta = at - dab.center;
        let local = rotate(delta, dab.rotation.x, -dab.rotation.y) / dab.radii;
        let radius = length(local);
        let edge = max(1. - dab.hardness, 1. / min(dab.radii.x, dab.radii.y));
        if radius > 1. - edge && radius < 1. {
            gradient = -rotate(local / dab.radii, dab.rotation.x, dab.rotation.y) / (edge * radius);
        }
        let mask = brush_selection_at(at);
        let mask_gradient = vec2(brush_selection_at(at + vec2(.5, 0.)) - brush_selection_at(at - vec2(.5, 0.)),
            brush_selection_at(at + vec2(0., .5)) - brush_selection_at(at - vec2(0., .5)));
        let shape = analytic_coverage(local, dab.hardness, min(dab.radii.x, dab.radii.y));
        gradient = gradient * mask + shape * mask_gradient;
    } else {
        gradient = vec2(contact_coverage(dab, at + vec2(.5, 0.)) - contact_coverage(dab, at - vec2(.5, 0.)),
            contact_coverage(dab, at + vec2(0., .5)) - contact_coverage(dab, at - vec2(0., .5)));
    }
    return Influence(value, gradient * dab.material.w);
}

fn trace_liquify(initial: vec2<f32>) -> Trace {
    var result = Trace(initial, IDENTITY, 1., vec2(0.));
    if style.deformation.z >= 0. { result.restore = style.deformation.z; return result; }
    var remaining = style.operation.y;
    loop {
        if remaining == 0u { break; }
        remaining -= 1u;
        let dab = dabs[style.operation.x + remaining];
        let steps = max(1u, u32(dab.previous_contact.x));
        let inverse_steps = 1. / f32(steps);
        for (var step = 0u; step < steps; step++) {
            let influence = liquify_influence(dab, result.position);
            if influence.value <= 0. { continue; }
            let s = influence.value;
            let gradient = influence.gradient;
            let delta = result.position - dab.center;
            let mode = LIQUIFY_MODE;
            var jacobian = IDENTITY;
            if mode == 0u {
                let motion = dab.motion * inverse_steps;
                jacobian -= outer(motion, gradient);
                result.position -= s * motion;
            } else if mode <= 2u {
                let amount = select(-.75, .75, mode == 1u);
                let angle = amount * s;
                let rotation = mat2x2(vec2(cos(angle), sin(angle)), vec2(-sin(angle), cos(angle)));
                jacobian = rotation * (IDENTITY + outer(vec2(-delta.y, delta.x), gradient * amount));
                result.position = dab.center + rotation * delta;
            } else if mode <= 4u {
                let amount = select(-.35, .35, mode == 3u) * inverse_steps;
                jacobian = IDENTITY * (1. + amount * s) + outer(delta, gradient * amount);
                result.position += delta * (amount * s);
            } else if mode == 5u {
                result.position += floor(24. * style.render_mode.w * s * (crystal_direction(result.position, dab) - .5) + .5);
            } else if mode == 6u {
                let normal = dab.previous_contact.zw;
                if dot(normal, normal) > 0. {
                    let amount = .35 * inverse_steps;
                    let distance = dot(normal, delta);
                    jacobian += outer(normal, amount * (s * normal + distance * gradient));
                    result.position += normal * (amount * s * distance);
                }
            } else {
                result.gradient = (1. - s) * result.gradient - result.restore * gradient;
                result.restore *= 1. - s;
            }
            result = Trace(result.position, jacobian * result.footprint, result.restore, result.gradient);
        }
    }
    return result;
}

fn compose_liquify(initial: vec2<f32>, trace: Trace, stored: MapSample) -> MapSample {
    let displacement = trace.position - initial + stored.displacement;
    let footprint = stored.footprint * trace.footprint;
    return MapSample(displacement * trace.restore,
        IDENTITY * (1. - trace.restore) + footprint * trace.restore + outer(displacement, trace.gradient));
}
