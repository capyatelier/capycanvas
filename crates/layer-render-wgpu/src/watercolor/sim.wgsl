const SLOT: i32 = 64;
const ATLAS_SLOTS: u32 = 32u;
const MISSING: u32 = 0xffffffffu;

const CAP: f32 = 0.3;
const WET: f32 = 0.002;
const PIN: f32 = 0.05;
const DRY_WICK: f32 = 0.02;
const EVAPORATION: f32 = 0.09;
const EDGE_EVAPORATION: f32 = 12.0;
const DEPOSIT: f32 = 0.55;
const GRANULATION: f32 = 1.4;
const LIFT: f32 = 1.5;
const SETTLE: f32 = 0.012;
const GAIN_CAP: f32 = 16.0;
const WET_DARKENING: f32 = 0.35;
const EDGE_REACH: i32 = 8;
const ACTIVITY: u32 = 4u;

struct Slot { tile: vec2<i32>, generation: u32, pad: u32, axis: vec4<u32>, diagonal: vec4<u32> }

struct Params {
    dt: f32,
    blocking: f32,
    evaporation: f32,
    deposit: f32,
    wet_edge: f32,
    rim_decay: f32,
    lift: f32,
    serial: u32,
    grain: vec4<f32>,
    axes: vec4<f32>,
}

struct Dab { center: vec2<f32>, radius: f32, water: f32, absorbance: vec3<f32>, pickup: f32, flow: f32, pad0: f32, pad1: f32, pad2: f32 }

@group(0) @binding(0) var<storage, read> slots: array<Slot>;
@group(0) @binding(1) var<storage, read> dispatch: array<u32>;
@group(0) @binding(2) var<uniform> params: Params;
@group(0) @binding(3) var state_in: texture_2d<f32>;
@group(0) @binding(4) var pops_in: texture_2d<f32>;
@group(0) @binding(5) var dep_in: texture_2d<f32>;
@group(0) @binding(6) var words_in: texture_2d<u32>;
@group(0) @binding(7) var link: texture_2d<u32>;
@group(0) @binding(8) var paper: texture_2d<f32>;
@group(0) @binding(9) var state_out: texture_storage_2d<rgba16float, write>;
@group(0) @binding(10) var pops_out: texture_storage_2d<rgba16float, write>;
@group(0) @binding(11) var dep_out: texture_storage_2d<rgba16float, write>;
@group(0) @binding(12) var words_out: texture_storage_2d<r32uint, write>;
@group(0) @binding(13) var link_out: texture_storage_2d<r32uint, write>;
@group(0) @binding(14) var paper_out: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(15) var<storage, read_write> activity: array<atomic<u32>>;
@group(0) @binding(16) var<storage, read> dabs: array<Dab>;
@group(0) @binding(17) var base: texture_2d<f32>;
@group(0) @binding(18) var paint_out: texture_storage_2d<rgba32float, write>;
@group(0) @binding(19) var paper_source: texture_2d<f32>;
@group(0) @binding(20) var paper_sampler: sampler;

fn origin(index: u32) -> vec2<i32> { return vec2<i32>(i32(index % ATLAS_SLOTS), i32(index / ATLAS_SLOTS)) * SLOT; }

fn neighbour(slot: Slot, step: vec2<i32>) -> u32 {
    if step.y == 0 { return select(slot.axis.z, slot.axis.x, step.x > 0); }
    if step.x == 0 { return select(slot.axis.w, slot.axis.y, step.y > 0); }
    if step.y > 0 { return select(slot.diagonal.y, slot.diagonal.x, step.x > 0); }
    return select(slot.diagonal.z, slot.diagonal.w, step.x > 0);
}

struct Cell { at: vec2<i32>, slot: u32, local: vec2<i32>, inside: bool }

fn cell(index: u32, local: vec2<i32>) -> Cell {
    let step = vec2<i32>(select(0, -1, local.x < 0) + select(0, 1, local.x >= SLOT), select(0, -1, local.y < 0) + select(0, 1, local.y >= SLOT));
    if all(step == vec2<i32>(0)) { return Cell(origin(index) + local, index, local, true); }
    let other = neighbour(slots[index], step);
    if other == MISSING {
        let clamped = clamp(local, vec2<i32>(0), vec2<i32>(SLOT - 1));
        return Cell(origin(index) + clamped, index, clamped, false);
    }
    let wrapped = local - step * SLOT;
    return Cell(origin(other) + wrapped, other, wrapped, true);
}

fn covered(word: u32) -> u32 { return countOneBits(word & 0x55555555u) + 2u * countOneBits(word & 0xAAAAAAAAu); }
fn gate_of(word: u32) -> f32 { return select(0.0, 1.0, covered(word) >= 12u); }
fn level_at(word: u32, pixel: vec2<i32>) -> u32 { return (word >> (2u * u32(pixel.y * 4 + pixel.x))) & 3u; }
fn code(word: u32, index: u32) -> f32 { return f32((word >> (6u * index)) & 63u) / 63.0; }
fn height_of(word: u32) -> f32 { return 1.0 - code(word, 0u); }
fn capacity(paper_height: f32) -> f32 { return CAP * (0.6 + 0.8 * (1.0 - paper_height)); }
fn kappa(word: u32, index: u32) -> f32 { return clamp(params.blocking * (0.7 + 0.6 * code(word, index)), 0.0, 0.95); }
fn conc_of(s: vec4<f32>) -> vec3<f32> { return s.yzw / max(s.x, 1e-5); }
fn pcg(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}
fn dither(index: u32, local: vec2<i32>) -> vec4<f32> {
    let global = vec2<u32>(slots[index].tile * SLOT + local);
    let h = pcg(pcg(global.x | (global.y << 16u)) ^ params.serial);
    return (vec4<f32>(vec4<u32>(h, h >> 8u, h >> 16u, h >> 24u) & vec4<u32>(255u)) + 0.5) / 256.0;
}
fn half(v: vec4<f32>, r: vec4<f32>) -> vec4<f32> {
    let exponent = max((bitcast<vec4<u32>>(v) >> vec4<u32>(23u)) & vec4<u32>(255u), vec4<u32>(113u)) - vec4<u32>(10u);
    let ulp = bitcast<vec4<f32>>(exponent << vec4<u32>(23u));
    let inverse = bitcast<vec4<f32>>((vec4<u32>(254u) - exponent) << vec4<u32>(23u));
    return floor(v * inverse + r) * ulp;
}

fn paper_row(at: vec2<i32>, row: i32) -> vec4<f32> { return textureLoad(paper, vec2<i32>(at.x, at.y * 4 + row), 0); }
fn paper_pixel(at: vec2<i32>, pixel: vec2<i32>) -> f32 { return paper_row(at, pixel.y)[pixel.x]; }
fn cell_paper_mean(at: vec2<i32>) -> f32 {
    return dot(paper_row(at, 0) + paper_row(at, 1) + paper_row(at, 2) + paper_row(at, 3), vec4<f32>(1.0)) / 16.0;
}

@compute @workgroup_size(16, 16)
fn fill_paper(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = dispatch[id.z];
    let texel = vec2<i32>(id.xy);
    if texel.x >= SLOT || texel.y >= SLOT * 4 { return; }
    let tile = slots[index].tile;
    let row = vec2<f32>(tile * 256) + vec2<f32>(f32(texel.x * 4), f32(texel.y)) + 0.5;
    var heights = vec4<f32>(0.0);
    for (var i = 0; i < 4; i += 1) {
        let p = row + vec2<f32>(f32(i), 0.0) + params.grain.zw;
        heights[i] = textureSampleLevel(paper_source, paper_sampler, vec2<f32>(dot(params.axes.xz, p), dot(params.axes.yw, p)) / params.grain.xy, 0.0).r;
    }
    let o = origin(index);
    textureStore(paper_out, vec2<i32>(o.x + texel.x, o.y * 4 + texel.y), heights);
}

fn paper_code(c: Cell) -> u32 { return u32(round((1.0 - cell_paper_mean(c.at)) * 63.0)); }

@compute @workgroup_size(16, 16)
fn links(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = dispatch[id.z];
    let local = vec2<i32>(id.xy);
    let own = cell(index, local);
    var word = paper_code(own);
    let e = cell(index, local + vec2<i32>(1, 0));
    let n = cell(index, local + vec2<i32>(0, 1));
    let w = cell(index, local - vec2<i32>(1, 0));
    let s = cell(index, local - vec2<i32>(0, 1));
    word |= select(63u, paper_code(e), e.inside) << 6u;
    word |= select(63u, paper_code(n), n.inside) << 12u;
    word |= select(63u, paper_code(w), w.inside) << 18u;
    word |= select(63u, paper_code(s), s.inside) << 24u;
    textureStore(link_out, own.at, vec4<u32>(word, 0u, 0u, 0u));
}

struct Neighbour { state: vec4<f32>, inside: bool }

fn neighbour_state(c: Cell) -> Neighbour { return Neighbour(textureLoad(state_in, c.at, 0), c.inside); }

fn receive_moved(own_wet: bool, incoming: f32, threshold: f32, k_own: f32) -> f32 {
    return (1.0 - select(k_own, 1.0 - DRY_WICK, !own_wet && incoming <= threshold)) * incoming;
}

fn send_back(n: Neighbour, outgoing: f32, threshold: f32, k_receiver: f32) -> f32 {
    if !n.inside { return outgoing; }
    return select(k_receiver, 1.0 - DRY_WICK, !(n.state.x > WET) && outgoing <= threshold) * outgoing;
}

fn dry_count(e: Neighbour, n: Neighbour, w: Neighbour, s: Neighbour) -> f32 {
    let wet = vec4<f32>(select(0.0, 1.0, e.inside && e.state.x > WET), select(0.0, 1.0, n.inside && n.state.x > WET),
        select(0.0, 1.0, w.inside && w.state.x > WET), select(0.0, 1.0, s.inside && s.state.x > WET));
    return dot(vec4<f32>(1.0) - wet, vec4<f32>(1.0));
}

fn edge_flags(local: vec2<i32>) -> u32 {
    return select(0u, 1u, local.x >= SLOT - EDGE_REACH) | select(0u, 2u, local.y >= SLOT - EDGE_REACH)
        | select(0u, 4u, local.x < EDGE_REACH) | select(0u, 8u, local.y < EDGE_REACH);
}

@compute @workgroup_size(16, 16)
fn step(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = dispatch[id.z];
    let local = vec2<i32>(id.xy);
    let me = cell(index, local);
    let c = textureLoad(state_in, me.at, 0);
    let own = textureLoad(pops_in, me.at, 0);
    let word = textureLoad(link, me.at, 0).x;
    let ce = cell(index, local + vec2<i32>(1, 0));
    let cn = cell(index, local + vec2<i32>(0, 1));
    let cw = cell(index, local - vec2<i32>(1, 0));
    let cs = cell(index, local - vec2<i32>(0, 1));
    let e = neighbour_state(ce);
    let n = neighbour_state(cn);
    let w = neighbour_state(cw);
    let s = neighbour_state(cs);
    let pe = textureLoad(pops_in, ce.at, 0).z;
    let pn = textureLoad(pops_in, cn.at, 0).w;
    let pw = textureLoad(pops_in, cw.at, 0).x;
    let ps = textureLoad(pops_in, cs.at, 0).y;
    let gates = vec4<f32>(gate_of(textureLoad(words_in, ce.at, 0).x), gate_of(textureLoad(words_in, cn.at, 0).x),
        gate_of(textureLoad(words_in, cw.at, 0).x), gate_of(textureLoad(words_in, cs.at, 0).x));
    let own_gate = gate_of(textureLoad(words_in, me.at, 0).x);
    let wet = c.x > WET;
    let k = kappa(word, 0u);
    let t = PIN / 6.0;
    let m1 = select(0.0, receive_moved(wet, pw, t, k), w.inside);
    let m2 = select(0.0, receive_moved(wet, ps, t, k), s.inside);
    let m3 = select(0.0, receive_moved(wet, pe, t, k), e.inside);
    let m4 = select(0.0, receive_moved(wet, pn, t, k), n.inside);
    let b1 = send_back(e, own.x, t, kappa(word, 1u));
    let b2 = send_back(n, own.y, t, kappa(word, 2u));
    let b3 = send_back(w, own.z, t, kappa(word, 3u));
    let b4 = send_back(s, own.w, t, kappa(word, 4u));
    let rest = max(c.x - dot(own, vec4<f32>(1.0)), 0.0);
    let conc = conc_of(c);
    let sent = dot(own - vec4<f32>(b1, b2, b3, b4), gates);
    let arriving = m1 * conc_of(w.state) + m2 * conc_of(s.state) + m3 * conc_of(e.state) + m4 * conc_of(n.state);
    let pigment = max(c.yzw - sent * conc + own_gate * arriving, vec3<f32>(0.0));
    let g = vec4<f32>(m1 + b3, m2 + b4, m3 + b1, m4 + b2);
    let rho = rest + dot(g, vec4<f32>(1.0));
    var u = vec2<f32>(g.x - g.z, g.y - g.w) / max(rho, 1e-5);
    let speed = length(u);
    if speed > 0.3 { u *= 0.3 / max(speed, 1e-9); }
    let feq = (rho / 6.0) * (vec4<f32>(1.0) + 3.0 * vec4<f32>(u.x, u.y, -u.x, -u.y));
    let rest_new = rho / 3.0;
    let pops = max(feq, vec4<f32>(0.0));
    let total = rest_new + dot(pops, vec4<f32>(1.0));
    let scale = select(0.0, rho / max(total, 1e-9), total > 1e-9);
    let streamed = total * scale;

    let deposit = textureLoad(dep_in, me.at, 0);
    let paper_height = height_of(word);
    let edge = select(0.0, dry_count(e, n, w, s) / 4.0, streamed > WET);
    let w2 = max(streamed - params.dt * params.evaporation * EVAPORATION * (1.0 + EDGE_EVAPORATION * edge), 0.0);
    let cap = capacity(paper_height);
    let film = max(w2 - cap, 0.0);
    let thin = clamp(1.0 - film / (CAP * 0.6), 0.08, 1.0);
    let arrival = clamp((w2 - c.x) / CAP, 0.0, 1.0) * select(0.0, 1.0, film <= 0.02);
    let rim = deposit.w + arrival;
    let decayed = rim * params.rim_decay;
    let base_rate = clamp(params.dt * params.deposit * DEPOSIT * thin * (1.0 + GRANULATION * (0.5 - paper_height)), 0.0, 1.0);
    let rate = 1.0 - (1.0 - base_rate) * exp(-params.wet_edge * decayed);
    let moved = select(pigment * rate, pigment, w2 < SETTLE);
    let lift = deposit.xyz * clamp(params.dt * LIFT * params.lift * clamp(film / CAP, 0.0, 1.0), 0.0, 1.0);
    let ratio = select(0.0, w2 / max(streamed, 1e-7), streamed > 1e-7);
    let r = dither(index, local);
    textureStore(state_out, me.at, half(vec4<f32>(w2, max(pigment - moved + lift, vec3<f32>(0.0))), r));
    textureStore(pops_out, me.at, half(pops * scale * ratio, r.yzwx));
    textureStore(dep_out, me.at, half(vec4<f32>(max(deposit.xyz + moved - lift, vec3<f32>(0.0)), rim - decayed), r.zwxy));
    if w2 > 0.0 { atomicMax(&activity[index * ACTIVITY], bitcast<u32>(w2)); }
    if w2 > WET { atomicOr(&activity[index * ACTIVITY + 1u], edge_flags(local)); }
}

fn grown_pixel(word: u32, pixel: vec2<i32>, c: f32, wx: f32, wy: f32, height: f32, scale: f32) -> u32 {
    let o = abs(2.0 * vec2<f32>(pixel) - 3.0) / 8.0;
    let wp = c + o.x * (wx - c) + o.y * (wy - c);
    return select(word, word | (3u << (2u * u32(pixel.y * 4 + pixel.x))), wp * scale + 0.5 * (height - 0.5) > 1.0);
}

@compute @workgroup_size(16, 16)
fn grow(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = dispatch[id.z];
    let local = vec2<i32>(id.xy);
    let me = cell(index, local);
    let word = textureLoad(words_in, me.at, 0).x;
    let c = textureLoad(state_in, me.at, 0).x;
    let e = textureLoad(state_in, cell(index, local + vec2<i32>(1, 0)).at, 0).x;
    let n = textureLoad(state_in, cell(index, local + vec2<i32>(0, 1)).at, 0).x;
    let w = textureLoad(state_in, cell(index, local - vec2<i32>(1, 0)).at, 0).x;
    let s = textureLoad(state_in, cell(index, local - vec2<i32>(0, 1)).at, 0).x;
    let scale = 1.0 / (0.7 * capacity(height_of(textureLoad(link, me.at, 0).x)));
    var out = word;
    if word != 0xFFFFFFFFu && max(c, max(max(e, n), max(w, s))) * scale + 0.25 > 1.0 {
        for (var y = 0; y < 4; y += 1) {
            let row = paper_row(me.at, y);
            let wy = select(s, n, y >= 2);
            out = grown_pixel(out, vec2<i32>(0, y), c, w, wy, row.x, scale);
            out = grown_pixel(out, vec2<i32>(1, y), c, w, wy, row.y, scale);
            out = grown_pixel(out, vec2<i32>(2, y), c, e, wy, row.z, scale);
            out = grown_pixel(out, vec2<i32>(3, y), c, e, wy, row.w, scale);
        }
    }
    textureStore(words_out, me.at, vec4<u32>(out, 0u, 0u, 0u));
}

fn coverage_at(d: Dab, point: vec2<f32>, height: f32) -> f32 {
    let distance = length(point - d.center) / max(d.radius, 1e-3);
    let dryness = clamp((0.45 - d.water) / 0.3, 0.0, 1.0);
    let raw = clamp((1.0 - distance + 0.12 * (height - 0.5)) * 10.0, 0.0, 1.0) * clamp(1.0 - dryness * (1.6 - 2.4 * height), 0.0, 1.0);
    return round(raw * 3.0) / 3.0;
}

@compute @workgroup_size(16, 16)
fn inject(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = dispatch[id.z];
    let local = vec2<i32>(id.xy);
    let at = origin(index) + local;
    var state = textureLoad(state_in, at, 0);
    var pops = textureLoad(pops_in, at, 0);
    var word = textureLoad(words_in, at, 0).x;
    let corner = vec2<f32>((slots[index].tile * SLOT + local) * 4);
    for (var i = 0u; i < arrayLength(&dabs); i += 1u) {
        let d = dabs[i];
        let centre = corner + 2.0;
        if length(centre - d.center) > d.radius * 1.07 + 4.0 { continue; }
        var sum = 0.0;
        for (var y = 0; y < 4; y += 1) {
            let row = paper_row(at, y);
            for (var x = 0; x < 4; x += 1) {
                let level = coverage_at(d, corner + vec2<f32>(f32(x), f32(y)) + 0.5, row[x]);
                sum += level;
                let shift = 2u * u32(y * 4 + x);
                let stored = (word >> shift) & 3u;
                word = (word & ~(3u << shift)) | (max(stored, u32(round(level * 3.0))) << shift);
            }
        }
        let c = sum / 16.0 * d.flow;
        if c <= 0.0 { continue; }
        let water = max(state.x + (d.water - state.x) * c, 0.0);
        let added = max(water - state.x, 0.0) * d.absorbance;
        let exchanged = (d.absorbance * water - state.yzw) * c;
        let pigment = max(state.yzw + select(max(exchanged, added), exchanged * d.pickup, exchanged < vec3<f32>(0.0)), vec3<f32>(0.0));
        pops = select(vec4<f32>(water / 6.0), pops * (water / max(state.x, 1e-7)), state.x > 1e-7);
        state = vec4<f32>(water, pigment);
    }
    let r = dither(index, local).wzyx;
    textureStore(state_out, at, half(state, r));
    textureStore(pops_out, at, half(pops, r.yzwx));
    textureStore(words_out, at, vec4<u32>(word, 0u, 0u, 0u));
}

struct Values { state: vec4<f32>, deposit: vec3<f32>, gran_c: f32, norm: f32 }

fn bilinear(index: u32, pixel: vec2<f32>) -> Values {
    let position = (pixel + 0.5) / 4.0 - 0.5;
    let o = vec2<i32>(floor(position));
    let f = position - vec2<f32>(o);
    let c00 = cell(index, o);
    let c10 = cell(index, o + vec2<i32>(1, 0));
    let c01 = cell(index, o + vec2<i32>(0, 1));
    let c11 = cell(index, o + vec2<i32>(1, 1));
    let weights = vec4<f32>((1.0 - f.x) * (1.0 - f.y), f.x * (1.0 - f.y), (1.0 - f.x) * f.y, f.x * f.y);
    let state = weights.x * textureLoad(state_in, c00.at, 0) + weights.y * textureLoad(state_in, c10.at, 0)
        + weights.z * textureLoad(state_in, c01.at, 0) + weights.w * textureLoad(state_in, c11.at, 0);
    let deposit = weights.x * textureLoad(dep_in, c00.at, 0).xyz + weights.y * textureLoad(dep_in, c10.at, 0).xyz
        + weights.z * textureLoad(dep_in, c01.at, 0).xyz + weights.w * textureLoad(dep_in, c11.at, 0).xyz;
    let heights = vec4<f32>(height_of(textureLoad(link, c00.at, 0).x), height_of(textureLoad(link, c10.at, 0).x),
        height_of(textureLoad(link, c01.at, 0).x), height_of(textureLoad(link, c11.at, 0).x));
    let cover = vec4<f32>(f32(covered(textureLoad(words_in, c00.at, 0).x)), f32(covered(textureLoad(words_in, c10.at, 0).x)),
        f32(covered(textureLoad(words_in, c01.at, 0).x)), f32(covered(textureLoad(words_in, c11.at, 0).x))) / 48.0;
    return Values(state, deposit, 1.0 + GRANULATION * 0.5 * (0.5 - dot(weights, heights)), dot(weights, cover));
}

fn absorbance(v: Values, mask: f32, height: f32) -> vec3<f32> {
    let gain = clamp(mask / max(v.norm, 0.05), 0.0, GAIN_CAP);
    let gran = 1.0 + GRANULATION * 0.5 * (0.5 - height);
    return v.deposit * (gain * gran / max(v.gran_c, 1e-3)) + v.state.yzw * gain * (1.0 + WET_DARKENING * clamp(v.state.x / CAP, 0.0, 1.0));
}

fn laid(existing: vec4<f32>, transmittance: vec3<f32>, minimum: f32) -> vec4<f32> {
    let clear = 1.0 - existing.a;
    return vec4<f32>(existing.rgb * transmittance + clear * (transmittance - vec3<f32>(minimum)), 1.0 - clear * minimum);
}

@compute @workgroup_size(16, 16)
fn bake(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = dispatch[id.z];
    let pixel = vec2<i32>(id.xy);
    if pixel.x >= SLOT * 4 || pixel.y >= SLOT * 4 { return; }
    let local = pixel / 4;
    let me = cell(index, local);
    let v = bilinear(index, vec2<f32>(pixel));
    let within = pixel - local * 4;
    let mask = f32(level_at(textureLoad(words_in, me.at, 0).x, within)) / 3.0;
    let t = exp(-absorbance(v, mask, paper_pixel(me.at, within)));
    let target_pixel = vec2<i32>(i32(id.z) * SLOT * 4, 0) + pixel;
    textureStore(paint_out, target_pixel, laid(textureLoad(base, target_pixel, 0), t, min(t.r, min(t.g, t.b))));
}
