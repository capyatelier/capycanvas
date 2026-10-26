const BLEND_SOFT: u32 = 0u;
const BLEND_BLUR: u32 = 1u;
const BLEND_SHARPEN: u32 = 2u;
const BLEND_BRISTLE: u32 = 3u;
const BLEND_GRAINY: u32 = 4u;
const BLEND_TEXTURE: u32 = 5u;
const BLEND_HUE: u32 = 6u;
const BLEND_KNIFE: u32 = 7u;
const BLEND_PAINTERLY: u32 = 9u;
const BLEND_CRYSTAL: u32 = 10u;
const BLEND_MARBLING: u32 = 11u;
const BLEND_COMB: u32 = 12u;
const BLEND_REACH_LIMIT: f32 = 112.0;
const SMUDGE_TRAVEL_LIMIT: f32 = 128.0;

struct BlendContact {
    exposure: f32,
    coverage: f32,
    diameter: f32,
    pressure: f32,
    travel: f32,
    direction: vec2<f32>,
    center: vec2<f32>,
    axis: vec2<f32>,
    trace: vec2<f32>,
    seed: u32,
    salt: u32,
}

struct Blended {
    paint: vec4<f32>,
    influence: f32,
}

fn blender_reach() -> f32 { return clamp(style.deformation.x, 0.0, 1.0); }
fn blender_rate() -> f32 { return max(style.deformation.y, 0.0); }
fn blender_character() -> f32 { return clamp(style.deformation.z, 0.0, 4.0); }
fn blender_scale() -> f32 { return clamp(style.deformation.w, 0.0, 1.0); }
fn blender_paper_strength() -> f32 { return max(style.transport_a.x, 0.0); }
fn blender_pressure_gain() -> f32 { return max(style.transport_a.y, 0.0); }
fn blender_engagement() -> f32 { return max(style.transport_a.z, 0.001); }
fn blender_recess() -> f32 { return clamp(style.transport_a.w, 0.0, 1.0); }
fn blender_lift() -> f32 { return max(style.transport_b.x, 0.0); }

fn hash_u32(value: u32) -> u32 {
    var v = value * 747796405u + 2891336453u;
    v = ((v >> ((v >> 28u) + 4u)) ^ v) * 277803737u;
    return (v >> 22u) ^ v;
}

fn hash_cell(cell: vec2<i32>, salt: u32) -> u32 {
    return hash_u32(bitcast<u32>(cell.x) ^ hash_u32(bitcast<u32>(cell.y) ^ salt));
}

fn hash_unit(cell: vec2<i32>, salt: u32) -> f32 {
    return f32(hash_cell(cell, salt) >> 8u) / 16777216.0;
}

fn hash_pair(cell: vec2<i32>, salt: u32) -> vec2<f32> {
    let h = hash_cell(cell, salt);
    return vec2<f32>(f32(h & 65535u), f32(h >> 16u)) / 65536.0;
}

fn gradient_noise(world: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(floor(world), vec2<f32>(0.06711056, 0.00583715))));
}

fn bristle_profile(lateral: f32, bristles: f32, salt: u32) -> f32 {
    let position = lateral * bristles;
    let cell = i32(floor(position));
    let t = fract(position);
    let a = hash_unit(vec2<i32>(cell, 7), salt);
    let b = hash_unit(vec2<i32>(cell + 1, 7), salt);
    return mix(a, b, t * t * (3.0 - 2.0 * t));
}

fn noise_gradient(point: vec2<f32>, salt: u32) -> vec3<f32> {
    let cell = vec2<i32>(floor(point));
    let f = fract(point);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let du = 30.0 * f * f * (f * (f - 2.0) + 1.0);
    let ga = hash_pair(cell, salt) * 2.0 - 1.0;
    let gb = hash_pair(cell + vec2<i32>(1, 0), salt) * 2.0 - 1.0;
    let gc = hash_pair(cell + vec2<i32>(0, 1), salt) * 2.0 - 1.0;
    let gd = hash_pair(cell + vec2<i32>(1, 1), salt) * 2.0 - 1.0;
    let va = dot(ga, f);
    let vb = dot(gb, f - vec2<f32>(1.0, 0.0));
    let vc = dot(gc, f - vec2<f32>(0.0, 1.0));
    let vd = dot(gd, f - vec2<f32>(1.0, 1.0));
    let value = va + u.x * (vb - va) + u.y * (vc - va) + u.x * u.y * (va - vb - vc + vd);
    let gradient = ga + u.x * (gb - ga) + u.y * (gc - ga) + u.x * u.y * (ga - gb - gc + gd)
        + du * (vec2<f32>(u.y, u.x) * (va - vb - vc + vd) + vec2<f32>(vb, vc) - va);
    return vec3<f32>(value, gradient);
}

fn curl(point: vec2<f32>, salt: u32) -> vec2<f32> {
    let gradient = noise_gradient(point, salt).yz + noise_gradient(point * 2.13 + vec2<f32>(5.2, 1.3), salt ^ 0x9e3779b9u).yz * 0.5;
    return vec2<f32>(gradient.y, -gradient.x);
}

fn lane_pull(world: vec2<f32>, dab: Dab, diameter: f32) -> f32 {
    if BLENDER != BLEND_BRISTLE && BLENDER != BLEND_KNIFE { return 1.0; }
    let moved = length(dab.motion);
    if moved <= 0.000001 { return 1.0; }
    let lateral = dot(world - dab.center, vec2<f32>(-dab.motion.y, dab.motion.x) / moved) / diameter;
    let lane = bristle_profile(lateral, max(blender_scale() * 64.0, 1.0), u32(dab.contact.w));
    if BLENDER == BLEND_BRISTLE { return 1.0 - min(blender_character(), 1.0) * 0.75 * lane; }
    return 1.0 - min(blender_character(), 1.0) * 0.35 * lane;
}

fn blend_contact(world: vec2<f32>, first: u32, count: u32) -> BlendContact {
    var result = BlendContact(0.0, 0.0, 0.0, 0.0, 0.0, vec2<f32>(1.0, 0.0), world, vec2<f32>(1.0, 0.0), world, 0u, 0u);
    var motion = vec2<f32>(0.0);
    var swirl = vec2<f32>(0.0);
    let anchor = dabs[first];
    result.salt = hash_u32(bitcast<u32>(anchor.center.x) ^ hash_u32(bitcast<u32>(anchor.center.y)));
    let field = contact_field(world);
    var remaining = count;
    loop {
        if remaining == 0u { break; }
        remaining -= 1u;
        let dab = dabs[first + remaining];
        let diameter = max(max(dab.radii.x, dab.radii.y) * 2.0, 0.01);
        let moved = length(dab.motion);
        let coverage = contact_coverage_field(dab, world, field);
        if coverage <= 0.0 { continue; }
        let strength = coverage * dab.flow;
        result.exposure += strength * moved / max(min(dab.radii.x, dab.radii.y) * 2.0, 0.01);
        if result.coverage <= 0.0 {
            result.center = dab.center;
            result.axis = select(dab.rotation, vec2<f32>(-dab.rotation.y, dab.rotation.x), dab.radii.y > dab.radii.x);
            result.seed = u32(dab.contact.w);
        }
        result.coverage = max(result.coverage, coverage);
        result.diameter = max(result.diameter, diameter);
        result.pressure = max(result.pressure, dab.contact.x);
        result.travel = max(result.travel, dab.contact.z);
        motion += dab.motion * coverage;
        result.trace -= dab.motion * clamp(dab.material.y, 0.0, 1.0) * strength * lane_pull(world, dab, diameter);
        if BLENDER == BLEND_MARBLING {
            swirl += curl(result.trace / max(blender_scale() * diameter, 2.0), u32(dab.contact.w)) * moved * blender_character() * strength;
        }
    }
    if BLEND_PAPER && result.coverage > 0.0 {
        var pressure = result.pressure;
        if BLENDER == BLEND_KNIFE { pressure *= exp(-max(result.travel, 0.0) * blender_scale() * 0.12); }
        let touch = paper_contact(paper_at(world), pressure, blender_recess() * (1.0 - result.coverage));
        result.exposure *= touch;
        result.trace = world - (world - result.trace) * touch;
    }
    let pulled = world - result.trace;
    result.trace = world - pulled * min(1.0, SMUDGE_TRAVEL_LIMIT / max(length(pulled), 0.000001));
    result.trace -= swirl * min(1.0, BLEND_REACH_LIMIT / max(length(swirl), 0.000001));
    let moved = length(motion);
    if moved > 0.000001 { result.direction = motion / moved; }
    return result;
}

fn paper_contact(paper: f32, pressure: f32, recess: f32) -> f32 {
    let depth = 0.5 - clamp(0.5 + blender_paper_strength() * (paper - 0.5), 0.0, 1.0);
    let threshold = recess + (1.0 + 0.25 * recess) * depth;
    let x = clamp((0.5 * clamp(pressure * blender_pressure_gain(), 0.0, 1.0) - threshold) / blender_engagement(), 0.0, 1.0);
    return x * x * (3.0 - 2.0 * x);
}

fn blend_amount(contact: BlendContact) -> f32 {
    return 1.0 - exp(-blender_rate() * contact.exposure);
}

fn blend_radius(contact: BlendContact, limit: f32) -> f32 {
    let exposure = blender_rate() * contact.exposure;
    let amount = 1.0 - exp(-exposure);
    let spread = select(1.0, sqrt(exposure / amount), amount > 0.0001);
    return min(blender_reach() * contact.diameter * spread, limit);
}

fn blend_tap(point: vec2<f32>) -> vec4<f32> {
    return mix_space_tap(canvas_load(point)) * brush_selection_at(point);
}

fn blend_sample(point: vec2<f32>) -> vec4<f32> {
    return mix_space_tap(canvas_sample(point)) * brush_selection_at(point);
}

fn disc(center: vec2<f32>, radius: f32, count: u32, rotation: f32) -> vec4<f32> {
    var sum = vec4<f32>(0.0);
    for (var index = 0u; index < count; index += 1u) {
        sum += blend_tap(center + vogel(index, count, rotation) * radius);
    }
    return sum / f32(count);
}

fn chunk_rotation(contact: BlendContact) -> f32 {
    return f32(contact.salt & 1023u) * 0.00614;
}

fn paper_at(point: vec2<f32>) -> f32 {
    return textureSampleLevel(grain_texture, brush_sampler, paper_uv(point), 0.0).r;
}

fn grainy_field(world: vec2<f32>, contact: BlendContact) -> vec4<f32> {
    let radius = blend_radius(contact, BLEND_REACH_LIMIT);
    if !BLEND_PAPER { return disc(contact.trace, radius, BLEND_TAPS, chunk_rotation(contact)); }
    var color = vec4<f32>(0.0);
    var paper = 0.0;
    var cross = vec4<f32>(0.0);
    var square = 0.0;
    var low = 1.0;
    var high = 0.0;
    for (var index = 0u; index < BLEND_TAPS; index += 1u) {
        let point = contact.trace + vogel(index, BLEND_TAPS, chunk_rotation(contact)) * radius;
        let tap = blend_tap(point);
        let height = paper_at(point);
        color += tap;
        paper += height;
        cross += tap * height;
        square += height * height;
        low = min(low, height);
        high = max(high, height);
    }
    let taps = f32(BLEND_TAPS);
    color /= taps;
    paper /= taps;
    let spread = max(square / taps - paper * paper, 0.0);
    let delta = paper_at(world) - paper;
    let extent = select(high - paper, paper - low, delta >= 0.0);
    let gain = delta / max(max(spread, 0.002), abs(delta) * extent) * (spread / (spread + 0.0005));
    var result = color + (cross / taps - color * paper) * gain;
    if u32(style.render_mode.z) == MIX_OKLAB && working_has_color(color.a) {
        result = vec4<f32>(result.r, mix(color.gb * (result.a / color.a), result.gb, 0.3), result.a);
    }
    return result;
}

fn texture_field(world: vec2<f32>, original: vec4<f32>, contact: BlendContact) -> vec4<f32> {
    let field = disc(contact.trace, blend_radius(contact, BLEND_REACH_LIMIT), BLEND_TAPS, chunk_rotation(contact));
    let fine = clamp(blender_scale() * contact.diameter, 1.5, BLEND_REACH_LIMIT / 3.0);
    let center = mix_space_tap(original);
    var local = center;
    var wide = vec4<f32>(0.0);
    for (var index = 0u; index < 6u; index += 1u) {
        let angle = f32(index) * 1.0471976 + 0.5;
        let offset = vec2<f32>(cos(angle), sin(angle)) * fine;
        local += blend_tap(world + offset);
        wide += blend_tap(world + offset * 3.0);
    }
    if !working_has_color(center.a) { return field; }
    let near = local.rgb / local.a;
    let shift = (wide.rgb + local.rgb) / (wide.a + local.a) - near;
    let steady = 0.0016 / (0.0016 + dot(shift, shift));
    return vec4<f32>(field.rgb + (center.rgb / center.a - near) * blender_character() * steady * field.a, field.a);
}

fn hue_paint(original: vec4<f32>, field: vec4<f32>) -> vec4<f32> {
    if !working_has_color(original.a) || !working_has_color(field.a) { return mix_space_tap(original); }
    let own = working_to_oklab(working_unassociate(original));
    let blended = working_to_oklab(from_mix_space(field.rgb / field.a));
    let lab = vec3<f32>(mix(own.x, blended.x, min(blender_character(), 1.0)), blended.yz);
    return mix_space_tap(vec4<f32>(working_from_oklab(lab) * original.a, original.a));
}

fn sharpen_paint(world: vec2<f32>, original: vec4<f32>, contact: BlendContact) -> vec4<f32> {
    if !working_has_color(original.a) { return mix_space_tap(original); }
    let radius = blend_radius(contact, BLEND_REACH_LIMIT);
    let center = to_mix_space(working_unassociate(original));
    var mean = vec3<f32>(0.0);
    var low = center;
    var high = center;
    for (var index = 0u; index < 8u; index += 1u) {
        let angle = f32(index) * 0.7853982 + chunk_rotation(contact);
        let tap = canvas_sample(world + vec2<f32>(cos(angle), sin(angle)) * radius);
        let color = select(center, to_mix_space(working_unassociate(tap)), working_has_color(tap.a));
        mean += color;
        low = min(low, color);
        high = max(high, color);
    }
    let sharpened = clamp(center + (center - mean / 8.0) * blender_character(), low, high);
    return vec4<f32>(sharpened * original.a, original.a);
}

fn cubic_weights(t: f32) -> vec4<f32> {
    let t2 = t * t;
    let t3 = t2 * t;
    return vec4<f32>(-0.5 * t3 + t2 - 0.5 * t, 1.5 * t3 - 2.5 * t2 + 1.0, -1.5 * t3 + 2.0 * t2 + 0.5 * t, 0.5 * t3 - 0.5 * t2);
}

fn cubic_canvas_sample(position: vec2<f32>) -> vec4<f32> {
    let base = floor(position - 0.5);
    let f = position - 0.5 - base;
    let wx = cubic_weights(f.x);
    let wy = cubic_weights(f.y);
    var sum = vec4<f32>(0.0);
    var low = vec4<f32>(1e9);
    var high = vec4<f32>(-1e9);
    for (var y = 0; y < 4; y += 1) {
        var row = vec4<f32>(0.0);
        for (var x = 0; x < 4; x += 1) {
            let tap = local_canvas_load(base + vec2<f32>(f32(x) - 0.5, f32(y) - 0.5));
            row += tap * wx[x];
            if (x == 1 || x == 2) && (y == 1 || y == 2) {
                low = min(low, tap);
                high = max(high, tap);
            }
        }
        sum += row * wy[y];
    }
    return clamp(sum, low, high);
}

fn painterly_field(contact: BlendContact, world: vec2<f32>) -> vec4<f32> {
    let stretch = 1.0 + blender_character();
    let radius = blend_radius(contact, BLEND_REACH_LIMIT / sqrt(stretch));
    let u = contact.direction * radius * sqrt(stretch);
    let v = vec2<f32>(-contact.direction.y, contact.direction.x) * radius / sqrt(stretch);
    let rotation = (gradient_noise(world) - 0.5) * 0.4;
    var best = vec4<f32>(0.0);
    var lowest = 0.0;
    for (var sector = 0u; sector < 8u; sector += 1u) {
        let heading = f32(sector) * 0.7853982 + rotation;
        var first = vec4<f32>(0.0);
        var shift = vec4<f32>(0.0);
        var square = 0.0;
        for (var tap = 0u; tap < 3u; tap += 1u) {
            let angle = heading + (f32(tap) - 1.0) * 0.3;
            let distance = select(0.45, 0.9, tap == 1u);
            let sample = blend_tap(contact.trace + (u * cos(angle) + v * sin(angle)) * distance);
            if tap == 0u { first = sample; }
            let deviation = sample - first;
            shift += deviation;
            square += dot(deviation, deviation);
        }
        shift /= 3.0;
        let variance = (max(square / 3.0 - dot(shift, shift), 0.0) + 0.00002) * (1.0 - 0.5 * cos(heading));
        if sector == 0u || variance < lowest {
            lowest = variance;
            best = first + shift;
        }
    }
    return best;
}

fn crystal_field(world: vec2<f32>, contact: BlendContact) -> vec4<f32> {
    let stretch = 1.0 + blender_character();
    let size = clamp(blender_scale() * contact.diameter, 2.0, BLEND_REACH_LIMIT / (2.0 * stretch + 1.0));
    let along = contact.axis;
    let across = vec2<f32>(-along.y, along.x);
    let local = vec2<f32>(dot(world, along) / stretch, dot(world, across)) / size;
    let cell = vec2<i32>(floor(local));
    var best = vec2<f32>(0.0);
    var nearest = 1e9;
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            let neighbor = cell + vec2<i32>(x, y);
            let site = vec2<f32>(neighbor) + 0.1 + 0.8 * hash_pair(neighbor, contact.seed * 2654435761u);
            let distance = dot(site - local, site - local);
            if distance < nearest {
                nearest = distance;
                best = site;
            }
        }
    }
    let point = best * size;
    return disc(along * point.x * stretch + across * point.y, min(blender_reach() * contact.diameter, size), 8u, 0.0);
}

struct Strand {
    tangent: vec2<f32>,
    coherence: f32,
}

fn luma_at(point: vec2<f32>) -> f32 {
    let paint = canvas_load(point);
    return dot(paint.rgb, WORKING_LUMA) + (1.0 - paint.a) * 0.5;
}

fn strand(world: vec2<f32>, step: f32) -> Strand {
    var tensor = vec3<f32>(0.0);
    for (var cell = 0u; cell < 4u; cell += 1u) {
        let corner = world + (vec2<f32>(f32(cell & 1u), f32(cell >> 1u)) - 1.0) * step;
        let a = luma_at(corner);
        let b = luma_at(corner + vec2<f32>(step, 0.0));
        let c = luma_at(corner + vec2<f32>(0.0, step));
        let d = luma_at(corner + vec2<f32>(step, step));
        let gx = (b + d - a - c) * 0.5;
        let gy = (c + d - a - b) * 0.5;
        tensor += vec3<f32>(gx * gx, gy * gy, gx * gy);
    }
    let difference = tensor.x - tensor.y;
    let root = sqrt(difference * difference + 4.0 * tensor.z * tensor.z);
    var tangent = vec2<f32>(tensor.y - tensor.x + root, -2.0 * tensor.z);
    if dot(tangent, tangent) < 0.000001 {
        tangent = select(vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), tensor.y >= tensor.x);
    }
    let strength = clamp((tensor.x + tensor.y) * 60.0, 0.0, 1.0);
    return Strand(normalize(tangent), clamp(root / (tensor.x + tensor.y + 0.000001), 0.0, 1.0) * strength);
}

fn comb_field(world: vec2<f32>, contact: BlendContact) -> vec4<f32> {
    let step = clamp(blender_reach() * contact.diameter * 0.08, 1.0, 3.0);
    let start = strand(world, step);
    let fade = smoothstep(0.15, 0.6, start.coherence);
    let origin = blend_tap(world);
    if fade <= 0.0 { return origin; }
    let stride = clamp(blender_reach() * contact.diameter, 3.0, 32.0) * 0.5 * (1.0 + start.coherence) / 6.0;
    let jitter = gradient_noise(world) - 0.5;
    let stop = mix(0.45, 0.15, min(blender_character(), 1.0));
    var sum = origin;
    var weights = 1.0;
    for (var side = 0u; side < 2u; side += 1u) {
        var heading = select(-start.tangent, start.tangent, side == 0u);
        var position = world + heading * stride * jitter;
        for (var index = 1u; index <= 6u; index += 1u) {
            if index == 4u {
                let bend = strand(position + heading * stride * 0.5, step);
                if bend.coherence < 0.1 { break; }
                heading = select(-bend.tangent, bend.tangent, dot(bend.tangent, heading) >= 0.0);
            }
            position += heading * stride;
            let sample = blend_sample(position);
            if length(sample - origin) > stop { break; }
            let weight = 1.0 - f32(index) / 7.0;
            sum += sample * weight;
            weights += weight;
        }
    }
    return mix(origin, sum / weights, fade);
}

fn bristle_lane(world: vec2<f32>, contact: BlendContact) -> f32 {
    let lateral = dot(world - contact.center, vec2<f32>(-contact.direction.y, contact.direction.x)) / max(contact.diameter, 0.01);
    return min(blender_character(), 1.0) * bristle_profile(lateral, max(blender_scale() * 40.0, 1.0), contact.seed ^ 0x2545f491u);
}

fn blended_by(world: vec2<f32>, original: vec4<f32>, contact: BlendContact) -> Blended {
    let amount = blend_amount(contact);
    let random = hash_unit(vec2<i32>(floor(world)), contact.salt);
    let radius = blend_radius(contact, BLEND_REACH_LIMIT);
    switch BLENDER {
        case BLEND_BLUR: {
            return Blended(disc(world, radius, BLEND_TAPS, chunk_rotation(contact)), amount);
        }
        case BLEND_SHARPEN: {
            return Blended(sharpen_paint(world, original, contact), amount);
        }
        case BLEND_BRISTLE: {
            let width = vec2<f32>(-contact.direction.y, contact.direction.x) * radius;
            let field = (blend_tap(contact.trace - width * random) + blend_tap(contact.trace + width * (1.0 - random))
                + blend_sample(contact.trace)) / 3.0;
            return Blended(field, amount * (1.0 - 0.6 * bristle_lane(world, contact)));
        }
        case BLEND_GRAINY: {
            return Blended(grainy_field(world, contact), amount);
        }
        case BLEND_TEXTURE: {
            return Blended(texture_field(world, original, contact), amount);
        }
        case BLEND_HUE: {
            return Blended(hue_paint(original, disc(contact.trace, radius, BLEND_TAPS, chunk_rotation(contact))), amount);
        }
        case BLEND_KNIFE: {
            return Blended(mix_space_tap(cubic_canvas_sample(contact.trace)) * brush_selection_at(contact.trace), amount);
        }
        case BLEND_PAINTERLY: {
            return Blended(painterly_field(contact, world), amount);
        }
        case BLEND_CRYSTAL: {
            return Blended(crystal_field(world, contact), amount);
        }
        case BLEND_MARBLING: {
            return Blended(blend_sample(contact.trace), amount);
        }
        case BLEND_COMB: {
            return Blended(comb_field(world, contact), amount);
        }
        default: {
            return Blended(disc(contact.trace, radius, BLEND_TAPS, chunk_rotation(contact)), amount * (1.0 - bristle_lane(world, contact)));
        }
    }
}

fn blended_paint(original: vec4<f32>, paint: vec4<f32>, influence: f32) -> vec4<f32> {
    let painted = working_has_color(paint.a);
    if influence <= 0.0 || !painted { return original; }
    let dragged = paint.rgb / paint.a;
    let own = select(dragged, to_mix_space(working_unassociate(original)), working_has_color(original.a));
    let alpha = clamp(original.a + max(paint.a - original.a, 0.0) * influence, 0.0, 1.0);
    let color = from_mix_space(mix(own, dragged, working_ratio(influence * min(paint.a, 1.0), alpha)));
    return vec4<f32>(color * alpha, alpha);
}

fn smudge_fragment(fragment_position: vec2<f32>, world: vec2<f32>, first: u32, count: u32) -> MaterialOutput {
    let original = canvas_load(world);
    let contact = blend_contact(world, first, count);
    var influence = 0.0;
    var paint = vec4<f32>(0.0);
    if contact.coverage > 0.0 {
        let blended = blended_by(world, original, contact);
        influence = blended.influence;
        paint = blended.paint;
    }
    var coverage = 0.0;
    if style.canvas_opacity.z < 1.0 {
        var previous = dry_coverage(vec2<i32>(floor(fragment_position)));
        if style.canvas_opacity.w > 0.5 { previous = 0.0; }
        coverage = previous + (1.0 - previous) * influence;
        influence = stroke_increment(previous, coverage);
    }
    var kept = 1.0;
    if BLEND_LIFT { kept = 1.0 - (1.0 - exp(-blender_lift() * contact.exposure)) * min(style.canvas_opacity.z, 1.0); }
    return MaterialOutput(blended_paint(original, paint, influence) * kept, vec4<f32>(coverage, 0.0, 0.0, 1.0), vec4<f32>(0.0));
}
