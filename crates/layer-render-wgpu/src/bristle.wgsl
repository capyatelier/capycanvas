const BRISTLE_TABLE_WIDTH: f32 = 2048.0;
const BRISTLE_TABLE_LEVELS: f32 = 10.0;
const BRISTLE_TABLE_BLOCKS: f32 = 2.0;
const BRISTLE_FAN_TEXELS: f32 = 512.0;
const BRISTLE_FIELD_SIZE: f32 = 512.0;

fn bristles_enabled() -> bool {
    return contact_feature(256u, style.bristles.x > 0.5);
}

fn bristle_table(block: f32, texel: f32, level: f32) -> f32 {
    let row = block * BRISTLE_TABLE_LEVELS + clamp(level, 0.0, BRISTLE_TABLE_LEVELS - 1.0) + 0.5;
    return textureSampleLevel(primary_texture, brush_sampler,
        vec2<f32>(texel / BRISTLE_TABLE_WIDTH, row / (BRISTLE_TABLE_LEVELS * BRISTLE_TABLE_BLOCKS)), 0.0).r;
}

fn bristle_field(position: vec2<f32>) -> f32 {
    return smoothstep(0.2, 0.8,
        textureSampleLevel(transport_texture, brush_sampler, position / BRISTLE_FIELD_SIZE, 0.0).r);
}

fn bristle_cross(a: vec2<f32>, b: vec2<f32>) -> f32 {
    return a.x * b.y - a.y * b.x;
}

struct BristlePose {
    offset: vec2<f32>,
    motion: vec2<f32>,
    start_normal: vec2<f32>,
    end_normal: vec2<f32>,
    half_width: vec2<f32>,
    depth: vec2<f32>,
    start_heading: vec2<f32>,
    end_heading: vec2<f32>,
}

struct BristleLens {
    pressure: f32,
    distance: f32,
    reach: f32,
    slope: f32,
    half_width: f32,
    depth: f32,
    heading: vec2<f32>,
}

// How far along the fan the hair tips reach the paper. Light pressure engages
// the central hairs; pressure spreads contact toward the full width.
fn bristle_reach(pressure: f32) -> f32 {
    return 1.2 * mix(0.3, 1.0, pow(clamp(pressure, 0.0, 1.0), 0.7)) - 0.12;
}

fn bristle_lens(dab: Dab, pose: BristlePose, t: f32) -> BristleLens {
    var lens: BristleLens;
    lens.pressure = clamp(mix(dab.previous_contact.x, dab.contact.x, t), 0.0, 1.0);
    lens.distance = mix(dab.previous_contact.z, dab.contact.z, t);
    lens.reach = bristle_reach(lens.pressure);
    lens.slope = mix(dab.previous_contact.y, dab.contact.y, t);
    lens.half_width = mix(pose.half_width.x, pose.half_width.y, t);
    lens.depth = mix(pose.depth.x, pose.depth.y, t);
    let heading = mix(pose.start_heading, pose.end_heading, t);
    lens.heading = select(pose.end_heading, normalize(heading), dot(heading, heading) > 0.000001);
    return lens;
}

// How far the most ragged hairs fall short of the smooth contact, in pixels,
// at the fan's ends and at its tips. A light touch leaves them uneven;
// pressure bends the hairs so they even out. At the ends it scales with the
// fan's width, so even the thin first contact of a stroke has an irregular
// outline; at the tips it stays within the depth, so no hair goes missing.
// Past the first half diameter of travel the tips only decide when the
// trailing edge releases a pixel, where they would open gaps at span joins,
// so they even out there.
fn bristle_ragged(lens: BristleLens) -> vec2<f32> {
    let light = mix(1.0, 0.65, smoothstep(0.1, 0.8, lens.pressure));
    let tips = 1.0 - smoothstep(0.15, 0.5, lens.distance);
    return light * vec2<f32>(0.22 * lens.half_width, 0.55 * lens.depth * tips);
}

// How ragged the hairs along the pixel's track are, from 0 to 1: their tip
// lengths in the hair table, which drift slowly across the fan as the brush
// travels. It follows the pixel's track, so every span agrees where the edge is.
fn bristle_rough(point: vec2<f32>, lens: BristleLens, origin: f32) -> f32 {
    let track = bristle_cross(point * vec2<f32>(lens.half_width, lens.depth), lens.heading) / lens.half_width;
    let per_half_width = BRISTLE_FAN_TEXELS / max(style.bristles.y, 0.01);
    let hair = origin + track * per_half_width + lens.distance * 40.0;
    let level = log2(max(per_half_width / max(lens.half_width, 0.5) * f32(style.operation.z), 1.0));
    return clamp((bristle_table(1.0, hair, level) - 0.2) / 0.6, 0.0, 1.0);
}

// Approximate signed distance in pixels inside the ellipse of engaged hairs,
// clipped to the fan's width, less the hairs' shortfall.
fn bristle_margin(point: vec2<f32>, lens: BristleLens, shortfall: vec2<f32>) -> f32 {
    let room = lens.reach + lens.slope * lens.slope / 3.6;
    if room <= 0.0 { return -1.0e9; }
    let half_width = lens.half_width;
    let depth = lens.depth;
    let reach = sqrt(room / 0.9) * half_width;
    let offset = vec2<f32>((point.x + lens.slope / 1.8) * half_width, point.y * depth);
    let scaled = offset / vec2<f32>(reach, depth);
    let radius = length(scaled);
    let slope = length(scaled / vec2<f32>(reach, depth)) / max(radius, 0.000001);
    let ellipse = (1.0 - radius) / max(slope, 0.000001);
    let end = scaled.x * scaled.x / max(dot(scaled, scaled), 0.000001);
    return min(ellipse, (1.0 - abs(point.x)) * half_width) - mix(shortfall.y, shortfall.x, end);
}

// Light pressure presses less paint from the hairs, far less than a brush
// running dry: some of them skip along `along`, their track or, in a pressed
// imprint, the hairs themselves.
fn bristle_starved(texel: f32, along: f32, pressure: f32) -> f32 {
    let light = 1.0 - smoothstep(0.05, 0.55, pressure);
    if light <= 0.0 { return 0.0; }
    let skip = bristle_field(vec2<f32>(texel * 0.35 + 311.0, along));
    return light * 0.8 * smoothstep(0.5, 0.85, skip);
}

fn bristle_engaged(u: f32, lens: BristleLens) -> f32 {
    let across = clamp(u, -1.0, 1.0);
    let height = 0.9 * across * across + lens.slope * across;
    return clamp((lens.reach - height) / min(0.35, lens.reach + lens.slope * lens.slope / 3.6), 0.0, 1.0);
}

// The pixel's straight path from a (time 0) to b (time 1) against the lens,
// in the metric that makes the lens a unit circle.
struct BristleLine {
    strip: vec2<f32>,
    nearest: f32,
    half_chord: f32,
    offset_squared: f32,
    length_squared: f32,
}

fn bristle_line(a: vec2<f32>, b: vec2<f32>, centre: vec2<f32>, metric: vec2<f32>) -> BristleLine {
    var line: BristleLine;
    let du = b.x - a.x;
    line.strip = select(vec2<f32>(1.0e9, -1.0e9), vec2<f32>(-1.0e9, 1.0e9), abs(a.x) <= 1.0);
    if abs(du) > 0.000001 {
        let enter = (-1.0 - a.x) / du;
        let leave = (1.0 - a.x) / du;
        line.strip = vec2<f32>(min(enter, leave), max(enter, leave));
    }
    let p = (a - centre) * metric;
    let step = (b - centre) * metric - p;
    line.length_squared = max(dot(step, step), 0.000001);
    line.nearest = -dot(p, step) / line.length_squared;
    let offset = p + step * line.nearest;
    line.offset_squared = dot(offset, offset);
    line.half_chord = sqrt(max(1.0 - line.offset_squared, 0.0) / line.length_squared);
    return line;
}

// Where the path, limited to times low to high, passes through the lens: its
// entry and exit times, or twice the time it passes nearest when it misses.
fn bristle_chord(line: BristleLine, low: f32, high: f32) -> vec3<f32> {
    let lower = max(low, line.strip.x);
    let upper = min(high, line.strip.y);
    let crosses = lower <= upper;
    let enter = max(line.nearest - line.half_chord, lower);
    let leave = min(line.nearest + line.half_chord, upper);
    if crosses && line.half_chord > 0.0 && enter <= leave { return vec3<f32>(enter, leave, 1.0); }
    let closest = clamp(line.nearest, select(low, lower, crosses), select(high, upper, crosses));
    let gap = closest - line.nearest;
    return vec3<f32>(closest, closest, -(line.offset_squared + gap * gap * line.length_squared));
}

struct BristlePass {
    point: vec2<f32>,
    time: f32,
    exit: f32,
    gap: f32,
}

// Where the pixel's straight path, limited to times low to high, passes
// through the lens: the middle of its chord and when it leaves, or the time
// it passes nearest when it misses.
fn bristle_pass(line: BristleLine, first: vec2<f32>, last: vec2<f32>, low: f32, high: f32) -> BristlePass {
    let chord = bristle_chord(line, low, high);
    var crossing: BristlePass;
    crossing.time = 0.5 * (chord.x + chord.y);
    crossing.exit = chord.y;
    crossing.gap = -chord.z;
    crossing.point = mix(first, last, crossing.time);
    return crossing;
}

// Paint for one pixel from one span. A span commits the paint its trailing
// edge leaves: `reached` marks pixels the hairs leave during the span, which
// replaces what an earlier pass left there; `rim` marks antialiasing just
// outside the hairs. Pixels still under the hairs are only `held`: the stroke
// keeps their coverage until the hairs leave them, so a contact that shrinks
// as the pen lifts leaves full paint where it once covered. Lifting the fan
// commits everything still under it.
struct BristleDeposit {
    color: vec3<f32>,
    opacity: f32,
    coverage: f32,
    reached: bool,
    rim: bool,
    held: bool,
}

struct BristleTrack {
    origin: f32,
    texel: f32,
    per_pixel: f32,
    level: f32,
    lengthwise: f32,
    distance: f32,
    streaking: f32,
    pressure: f32,
    engaged: f32,
    across: f32,
    along: f32,
}

fn bristle_track(dab: Dab, pose: BristlePose, lens: BristleLens, closest: vec2<f32>, leaving: f32,
    departure: vec2<f32>, origin: f32) -> BristleTrack {
    var track: BristleTrack;
    track.origin = origin;
    track.pressure = lens.pressure;
    track.distance = max(mix(dab.previous_contact.z, dab.contact.z, leaving), 0.0);
    let spread = 1.0 - 0.5 * style.bristles.w * (1.0 - sqrt(lens.pressure));
    track.per_pixel = BRISTLE_FAN_TEXELS / max(style.bristles.y, 0.01) / (lens.half_width / max(spread, 0.05));
    track.level = log2(max(track.per_pixel * f32(style.operation.z), 1.0));
    track.engaged = bristle_engaged(closest.x, lens);
    let left = bristle_lens(dab, pose, leaving);
    let heading = left.heading;
    track.lengthwise = abs(heading.y) / max(abs(heading.x) + abs(heading.y), 0.000001);
    track.texel = origin + bristle_cross(heading, departure * vec2<f32>(left.half_width, left.depth)) * track.per_pixel;
    track.streaking = mix(dab.material.y, dab.material.w, leaving);
    track.across = clamp(departure.x, -1.0, 1.0) * left.half_width;
    track.along = departure.y * left.depth;
    return track;
}

fn bristle_material(track: BristleTrack, color: vec3<f32>, pressed: f32, paper: f32, coverage: f32,
    margin: f32) -> BristleDeposit {
    var deposit: BristleDeposit;
    let texel = track.texel;
    let distance = track.distance;
    let streaking = track.streaking;
    let strand = bristle_field(vec2<f32>(texel / 2.0 + 53.0, distance * 12.0));
    let dashes = bristle_field(vec2<f32>(texel * 0.5 + 199.0, distance * 90.0));

    let load = style.bristles.z;
    let rate = 0.75 + 0.5 * strand;
    let remaining = 10.0 * load - distance * rate;
    let residue = 0.3 * exp(min(remaining, 0.0) / 3.0);
    let supply = max(smoothstep(-0.5, 2.0, remaining), residue) * min(0.5 + load, 1.0);
    var dragged = 0.0;
    if streaking > 0.0 {
        let band = bristle_field(vec2<f32>(texel / 24.0, distance * 7.0));
        let overlap = log2(1.0 / max(track.lengthwise, 0.02));
        let hair_load = bristle_table(0.0, texel, track.level + overlap) * 2.0;
        let contact = sqrt(track.engaged);
        let plateau = pow(supply, 0.7) * (0.2 + 0.2 * track.pressure) * contact * (0.3 + 1.4 * band);
        let ridge = pow(hair_load, 1.3) * (0.3 + 1.4 * strand) * (0.4 + 0.8 * band) * (0.3 + 0.7 * supply) * (0.5 + 0.5 * contact);
        dragged = plateau + ridge * 0.75;
    }
    var stamped = 0.0;
    if streaking < 1.0 {
        let hair = track.origin + track.across * track.per_pixel;
        let tip_load = bristle_table(0.0, hair, track.level) * 2.0;
        let blotch = bristle_field(vec2<f32>(hair * 0.7 + 23.0, 40.0 + track.along * track.per_pixel * 0.5));
        stamped = 0.3 * pow(supply, 0.7) + 0.35 * blotch * (0.5 + 0.5 * tip_load);
    }

    let offered = mix(stamped, dragged, streaking);
    let dry = 1.0 - supply;
    let threshold = pow(dry, 1.5) * (0.1 + 0.5 * dashes)
        + 1.2 * style.contact_a.y * dry * (0.55 - paper)
        + 0.45 * (1.0 - track.engaged) * (1.0 - track.engaged) * streaking
        + bristle_starved(texel, distance * 4.0 + (1.0 - streaking) * track.along * track.per_pixel * 0.06,
            select(pressed, track.pressure, streaking > 0.5));
    let wet = clamp((offered - threshold) / 0.03 + 0.5, 0.0, 1.0);
    let thickness = offered * wet;
    deposit.opacity = smoothstep(0.03, 0.12, thickness);
    deposit.coverage = coverage;
    if deposit.opacity <= 0.0 { return deposit; }

    let pigment = max(color, vec3<f32>(0.0));
    let ridge = sqrt(max(style.bristle_streak.rgb, vec3<f32>(0.0)));
    let body = mix(pow(pigment, vec3<f32>(0.45)), ridge, smoothstep(0.2, 1.3, thickness));
    deposit.color = body * body;
    deposit.reached = margin > 0.0;
    deposit.rim = margin <= 0.0;
    return deposit;
}

fn bristle_lift(dab: Dab, pose: BristlePose, last: vec2<f32>, origin: f32, paper: f32) -> BristleDeposit {
    var deposit: BristleDeposit;
    let lens = bristle_lens(dab, pose, 1.0);
    let margin = bristle_margin(last, lens, bristle_ragged(lens) * bristle_rough(last, lens, origin));
    let coverage = select(clamp(margin + 0.5, 0.0, 1.0), 1.0, margin > 0.0);
    if coverage <= 0.0 { return deposit; }
    let track = bristle_track(dab, pose, lens, last, 1.0, last, origin);
    return bristle_material(track, dab.color.rgb, dab.material.z, paper, coverage, margin);
}

fn bristle_paint(dab: Dab, world: vec2<f32>, paper: f32) -> BristleDeposit {
    var deposit: BristleDeposit;
    if style.bristles.z <= 0.0 { return deposit; }
    let start = dab.center - dab.motion;
    var pose: BristlePose;
    pose.half_width = max(vec2<f32>(dab.previous.y, dab.radii.y), vec2<f32>(0.5));
    pose.depth = max(vec2<f32>(dab.previous.x, dab.radii.x), vec2<f32>(0.5));
    let bound = max(pose.half_width.x, pose.half_width.y) + max(pose.depth.x, pose.depth.y) + 2.0;
    if any(world < min(start, dab.center) - bound) || any(world > max(start, dab.center) + bound) {
        return deposit;
    }
    pose.offset = world - start;
    pose.motion = dab.motion;
    pose.start_normal = dab.previous.zw;
    pose.end_normal = dab.rotation;
    let origin = fract(dab.contact.w * 0.61803399) * BRISTLE_TABLE_WIDTH;
    let last = vec2<f32>(bristle_cross(pose.end_normal, world - dab.center) / pose.half_width.y,
        dot(world - dab.center, pose.end_normal) / pose.depth.y);
    pose.end_heading = vec2<f32>(sin(dab.texture_sign.y), cos(dab.texture_sign.y));
    if dab.material.x > 1.5 {
        pose.start_heading = pose.end_heading;
        return bristle_lift(dab, pose, last, origin, paper);
    }
    var lens_end: BristleLens;
    lens_end.pressure = clamp(dab.contact.x, 0.0, 1.0);
    lens_end.distance = dab.contact.z;
    lens_end.reach = bristle_reach(dab.contact.x);
    lens_end.slope = dab.contact.y;
    lens_end.half_width = pose.half_width.y;
    lens_end.depth = pose.depth.y;
    lens_end.heading = pose.end_heading;
    deposit.held = true;
    let inside = bristle_margin(last, lens_end, bristle_ragged(lens_end));
    deposit.coverage = clamp(inside + 0.5, 0.0, 1.0);
    if inside > 0.0 { return deposit; }
    let rough = bristle_rough(last, lens_end, origin);
    if bristle_margin(last, lens_end, vec2<f32>(0.0)) > 0.0 {
        let held = bristle_margin(last, lens_end, bristle_ragged(lens_end) * rough);
        deposit.coverage = clamp(held + 0.5, 0.0, 1.0);
        if held > 0.0 { return deposit; }
    }
    deposit.held = false;
    deposit.coverage = 0.0;
    let first = vec2<f32>(bristle_cross(pose.start_normal, pose.offset) / pose.half_width.x,
        dot(pose.offset, pose.start_normal) / pose.depth.x);
    let clear = 1.0 + 0.5 / vec2<f32>(min(pose.half_width.x, pose.half_width.y), min(pose.depth.x, pose.depth.y));
    if any(min(first, last) > clear) || any(max(first, last) < -clear) {
        return deposit;
    }
    pose.start_heading = vec2<f32>(sin(dab.texture_sign.x), cos(dab.texture_sign.x));
    let lens_middle = bristle_lens(dab, pose, 0.5);
    let centre = vec2<f32>(-lens_middle.slope / 1.8, 0.0);
    let lowest = -lens_middle.slope * lens_middle.slope / 3.6;
    let semi_axis = sqrt(max(lens_middle.reach - lowest, 0.02) / 0.9);
    let line = bristle_line(first, last, centre, vec2<f32>(1.0 / semi_axis, 1.0));
    let inner = bristle_pass(line, first, last, 0.0, 1.0);
    let clearance = 1.3 + 1.0 / min(semi_axis * lens_middle.half_width, lens_middle.depth);
    let lens_start = bristle_lens(dab, pose, 0.0);
    if inner.gap > clearance * clearance && bristle_margin(first, lens_start, vec2<f32>(0.0)) <= -0.5 { return deposit; }
    let travel = max(length(dab.motion), 0.0001);
    let lens_inner = bristle_lens(dab, pose, inner.time);
    let rough_start = bristle_rough(first, lens_start, origin);
    let reached = bristle_ragged(lens_inner) * mix(rough_start, rough, clamp(inner.time, 0.0, 1.0));
    let margin = max(bristle_margin(inner.point, lens_inner, reached),
        bristle_margin(first, lens_start, bristle_ragged(lens_start) * rough_start));
    let soon = min(2.0 / travel, 0.5);
    let near = bristle_pass(line, first, last, -soon, 1.0 + soon);
    let reach = bristle_margin(near.point, bristle_lens(dab, pose, clamp(near.time, 0.0, 1.0)), reached);
    if margin <= 0.0 && reach > 0.0 { return deposit; }
    let settle = min(max(pose.depth.x, pose.depth.y) / travel, 32.0);
    let far = bristle_pass(line, first, last, -settle, 1.0 + settle);
    let lens = bristle_lens(dab, pose, clamp(far.time, 0.0, 1.0));
    let deepest = max(reach, bristle_margin(far.point, lens, reached));
    let coverage = clamp(select(margin, max(margin, deepest), margin > 0.0) + 0.5, 0.0, 1.0);
    if coverage <= 0.0 { return deposit; }
    let leaving = clamp(inner.exit, 0.0, 1.0);
    let track = bristle_track(dab, pose, lens, far.point, leaving, mix(first, last, leaving), origin);
    return bristle_material(track, dab.color.rgb, dab.material.z, paper, coverage, margin);
}
