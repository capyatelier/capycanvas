// Pen-up healing. The stroke's copy S takes on its surroundings through a
// membrane h that matches D = B - S where the stroke leaves the image
// uncovered (weight w = 1 - coverage), B being the source composite at the
// destination. h is interpolated by pull-push over a pyramid whose cells are
// kept per damaged page, then relaxed at the finest level in red-black
// sweeps. Everything is Float32, in fixed order, with no atomics.
const NONE: u32 = 0xffffffffu;

struct Params {
    level: u32,
    parity: u32,
    tile: u32,
    scale: u32,
    window: vec4<i32>,
    candidate: u32,
    pages: u32,
    candidates: u32,
    flags: u32,
    scores: u32,
    choice: u32,
    windows: u32,
    boxes: u32,
}

@group(0) @binding(0) var<uniform> p: Params;
// Per level, eight words: cells per tile side, tiles across and down, then
// the offsets of its tile table and tile list and of its first cell, and its
// tile count. Tables map a tile position to its index; lists pack each
// tile's position as x | y << 16. From p.windows, each finest tile's window
// of cells, [min, max), then running counts of their cells and of the 16 x 16
// blocks that cover them, so a dispatch visits only the windows.
@group(0) @binding(1) var<storage, read> pyramid: array<u32>;
@group(0) @binding(2) var<storage, read_write> values: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> membrane: array<vec4<f32>>;
// Each cell's weight as f32 bits, then from p.scores the judged candidate's
// score per 32 x 32 block of each page, then from p.choice the best cost so
// far and its index, and a dispatch-indirect record per page.
@group(0) @binding(4) var<storage, read_write> words: array<u32>;
@group(0) @binding(5) var<storage, read> healed: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read> known: array<u32>;
@group(1) @binding(0) var destination: texture_2d<f32>;
@group(1) @binding(1) var source: texture_2d<f32>;
@group(1) @binding(2) var coverage: texture_2d<f32>;
@group(1) @binding(3) var painted: texture_2d<f32>;
@group(1) @binding(4) var start: texture_2d<f32>;
@group(1) @binding(5) var healed_page: texture_storage_2d<rgba32float, write>;
// Spot Healing's candidate source for one page: the target and reference
// pages its mapping reads, the page's first pixel in the target, and where
// the chosen candidate is laid down.
struct Candidate {
    origin: vec4<f32>,
    mapping: array<vec4<u32>, 4>,
}
@group(2) @binding(0) var candidate_target0: texture_2d<f32>;
@group(2) @binding(1) var candidate_target1: texture_2d<f32>;
@group(2) @binding(2) var candidate_target2: texture_2d<f32>;
@group(2) @binding(3) var candidate_target3: texture_2d<f32>;
@group(2) @binding(4) var candidate_reference0: texture_2d<f32>;
@group(2) @binding(5) var candidate_reference1: texture_2d<f32>;
@group(2) @binding(6) var candidate_reference2: texture_2d<f32>;
@group(2) @binding(7) var candidate_reference3: texture_2d<f32>;
@group(2) @binding(8) var<uniform> candidate: Candidate;
@group(3) @binding(0) var chosen: texture_storage_2d<rgba32float, write>;

fn retouch_target_load(page: i32, texel: vec2<i32>) -> vec4<f32> {
    switch page {
        case 0: { return textureLoad(candidate_target0, texel, 0); }
        case 1: { return textureLoad(candidate_target1, texel, 0); }
        case 2: { return textureLoad(candidate_target2, texel, 0); }
        default: { return textureLoad(candidate_target3, texel, 0); }
    }
}

fn retouch_reference_load(page: i32, texel: vec2<i32>) -> vec4<f32> {
    switch page {
        case 0: { return textureLoad(candidate_reference0, texel, 0); }
        case 1: { return textureLoad(candidate_reference1, texel, 0); }
        case 2: { return textureLoad(candidate_reference2, texel, 0); }
        default: { return textureLoad(candidate_reference3, texel, 0); }
    }
}

// The candidate's source at page pixel `texel`.
fn candidate_at(texel: vec2<i32>) -> vec4<f32> {
    return retouch_source(retouch_mapping(candidate.mapping), candidate.origin.xy + vec2<f32>(texel) + 0.5);
}

struct Level {
    tile: u32,
    grid: vec2<u32>,
    table: u32,
    list: u32,
    base: u32,
    count: u32,
}

fn level(index: u32) -> Level {
    let i = index * 8u;
    return Level(
        pyramid[i],
        vec2<u32>(pyramid[i + 1u], pyramid[i + 2u]),
        pyramid[i + 3u],
        pyramid[i + 4u],
        pyramid[i + 5u],
        pyramid[i + 6u],
    );
}

// The plane index of cell `g` of level `l`, or NONE outside its tiles.
fn cell(l: Level, g: vec2<i32>) -> u32 {
    if any(g < vec2<i32>(0)) {
        return NONE;
    }
    let tile = vec2<u32>(g) / l.tile;
    if any(tile >= l.grid) {
        return NONE;
    }
    let t = pyramid[l.table + tile.y * l.grid.x + tile.x];
    if t == NONE {
        return NONE;
    }
    let local = vec2<u32>(g) % l.tile;
    return l.base + t * l.tile * l.tile + local.y * l.tile + local.x;
}

fn tile_origin(l: Level, t: u32) -> vec2<u32> {
    let packed = pyramid[l.list + t];
    return vec2<u32>(packed & 0xffffu, packed >> 16u) * l.tile;
}

// The level position of cell `index`, counted from the level's first cell.
fn position(l: Level, index: u32) -> vec2<i32> {
    let area = l.tile * l.tile;
    let local = index % area;
    return vec2<i32>(tile_origin(l, index / area) + vec2<u32>(local % l.tile, local / l.tile));
}

fn weight_of(texel: vec2<i32>) -> f32 {
    return 1.0 - clamp(textureLoad(coverage, texel, 0).r, 0.0, 1.0);
}

// Whether page pixel `texel` lies in the window around the stroke, whose
// pixels the fields hold from its corner.
fn windowed(texel: vec2<i32>) -> bool {
    return all(texel >= p.window.xy) && all(texel < p.window.zw);
}

fn field(texture: texture_2d<f32>, texel: vec2<i32>) -> vec4<f32> {
    return textureLoad(texture, texel - p.window.xy, 0);
}

fn perceptual() -> bool {
    return (p.flags & 8u) != 0u;
}

fn blend_space(c: vec4<f32>) -> vec4<f32> {
    if perceptual() {
        return working_encode(c);
    }
    return c;
}

// Level 0 over one damaged page: each cell averages the difference D over its
// scale x scale pixels in the window, weighted by how uncovered each pixel
// is. A cell outside the window takes a negative weight: it holds no
// membrane.
@compute @workgroup_size(16, 16)
fn seed(@builtin(global_invocation_id) id: vec3<u32>) {
    let l = level(0u);
    if any(id.xy >= vec2<u32>(l.tile)) {
        return;
    }
    var sum = vec4<f32>(0.0);
    var weight = 0.0;
    var inside = 0.0;
    for (var y = 0u; y < p.scale; y++) {
        for (var x = 0u; x < p.scale; x++) {
            let texel = vec2<i32>(id.xy * p.scale + vec2<u32>(x, y));
            if !windowed(texel) {
                continue;
            }
            let w = weight_of(texel);
            if w > 0.0 {
                sum += w * (blend_space(field(destination, texel)) - blend_space(field(source, texel)));
                weight += w;
            }
            inside += 1.0;
        }
    }
    let i = l.base + p.tile * l.tile * l.tile + id.y * l.tile + id.x;
    values[i] = select(vec4<f32>(0.0), sum / weight, weight > 0.0);
    words[i] = bitcast<u32>(select(-1.0, weight / inside, inside > 0.0));
}

fn cells_of(index_level: u32) -> u32 {
    let l = level(index_level);
    return l.count * l.tile * l.tile;
}

// Pull level p.level from the level below with a [1 3 3 1] kernel. Missing
// cells carry no weight; a quarter of known weight saturates a coarse cell.
@compute @workgroup_size(256)
fn pull(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= cells_of(p.level) {
        return;
    }
    let coarse = level(p.level);
    let fine = level(p.level - 1u);
    let g = position(coarse, id.x);
    let taps = array<f32, 4>(1.0, 3.0, 3.0, 1.0);
    var sum = vec4<f32>(0.0);
    var weight = 0.0;
    for (var y = 0; y < 4; y++) {
        for (var x = 0; x < 4; x++) {
            let i = cell(fine, 2 * g + vec2<i32>(x - 1, y - 1));
            if i == NONE {
                continue;
            }
            let w = max(bitcast<f32>(words[i]), 0.0) * taps[x] * taps[y] / 64.0;
            sum += w * values[i];
            weight += w;
        }
    }
    let at = coarse.base + id.x;
    values[at] = select(vec4<f32>(0.0), sum / weight, weight > 0.0);
    words[at] = bitcast<u32>(min(1.0, 4.0 * weight));
}

// The membrane of the level above, bilinearly at cell `g` of the level below.
fn expanded(coarse: Level, g: vec2<i32>) -> vec4<f32> {
    let at = (vec2<f32>(g) + 0.5) * 0.5 - 0.5;
    let base = vec2<i32>(floor(at));
    let f = at - floor(at);
    var sum = vec4<f32>(0.0);
    var weight = 0.0;
    for (var y = 0; y < 2; y++) {
        for (var x = 0; x < 2; x++) {
            let i = cell(coarse, base + vec2<i32>(x, y));
            if i == NONE {
                continue;
            }
            let k = select(1.0 - f.x, f.x, x == 1) * select(1.0 - f.y, f.y, y == 1);
            sum += k * values[i];
            weight += k;
        }
    }
    return select(vec4<f32>(0.0), sum / weight, weight > 0.0);
}

// The finest tile holding the index-th cell or block counted by the running
// counts at `counts`, and its index within that tile's window.
fn counted(counts: u32, index: u32) -> vec2<u32> {
    var lo = 0u;
    var hi = p.pages;
    while lo + 1u < hi {
        let middle = (lo + hi) / 2u;
        if pyramid[counts + middle] <= index {
            lo = middle;
        } else {
            hi = middle;
        }
    }
    return vec2<u32>(lo, index - pyramid[counts + lo]);
}

fn window_of(t: u32) -> vec4<u32> {
    let i = p.windows + 4u * t;
    return vec4<u32>(pyramid[i], pyramid[i + 1u], pyramid[i + 2u], pyramid[i + 3u]);
}

// Push the membrane down to level p.level. Above level 0 it replaces every
// cell's values; at level 0 it starts the membrane in the windows, and D stays
// for the sweeps.
@compute @workgroup_size(256)
fn push(@builtin(global_invocation_id) id: vec3<u32>) {
    if p.level > 0u {
        if id.x >= cells_of(p.level) {
            return;
        }
        let l = level(p.level);
        let at = l.base + id.x;
        let w = bitcast<f32>(words[at]);
        values[at] = w * values[at] + (1.0 - w) * expanded(level(p.level + 1u), position(l, id.x));
        return;
    }
    let l = level(0u);
    let cells = p.windows + 4u * p.pages;
    if id.x >= pyramid[cells + p.pages] {
        return;
    }
    let found = counted(cells, id.x);
    let window = window_of(found.x);
    let width = window.z - window.x;
    let local = window.xy + vec2<u32>(found.y % width, found.y / width);
    let at = found.x * l.tile * l.tile + local.y * l.tile + local.x;
    let w = max(bitcast<f32>(words[at]), 0.0);
    membrane[at] = w * values[at] + (1.0 - w) * expanded(level(1u), vec2<i32>(tile_origin(l, found.x) + local));
}

// Whether finest cell `i` holds a membrane: it lies in a window.
fn inside(i: u32) -> bool {
    return i != NONE && bitcast<f32>(words[i]) >= 0.0;
}

const BLOCK: u32 = 16u;
const BLOCK_SWEEPS: u32 = 8u;
const BLOCK_PITCH: u32 = BLOCK + 2u;
var<workgroup> block_values: array<vec4<f32>, 324>;
var<workgroup> block_inside: array<u32, 324>;

@compute @workgroup_size(8, 16)
fn relax(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_id) lid: vec3<u32>) {
    let l = level(0u);
    let blocks = p.windows + 5u * p.pages + 1u;
    if group.x >= pyramid[blocks + p.pages] {
        return;
    }
    let found = counted(blocks, group.x);
    let window = window_of(found.x);
    let across = (window.z + BLOCK - 1u) / BLOCK - window.x / BLOCK;
    let block = vec2<u32>(window.x / BLOCK + found.y % across, window.y / BLOCK + found.y / across);
    let origin = tile_origin(l, found.x);
    if ((origin.x + origin.y) / BLOCK + block.x + block.y) % 2u != p.parity {
        return;
    }
    let block_origin = vec2<i32>(origin + block * BLOCK);
    for (var i = lid.y * (BLOCK / 2u) + lid.x; i < BLOCK_PITCH * BLOCK_PITCH; i += BLOCK * BLOCK / 2u) {
        let offset = vec2<u32>(i % BLOCK_PITCH, i / BLOCK_PITCH);
        let corner = (offset.x == 0u || offset.x == BLOCK_PITCH - 1u) && (offset.y == 0u || offset.y == BLOCK_PITCH - 1u);
        var valid = false;
        var value = vec4<f32>(0.0);
        if !corner {
            let source = cell(l, block_origin + vec2<i32>(offset) - 1);
            valid = inside(source);
            if valid { value = membrane[source]; }
        }
        block_values[i] = value;
        block_inside[i] = u32(valid);
    }
    workgroupBarrier();
    let local = block * BLOCK + vec2<u32>(2u * lid.x, lid.y);
    let at = found.x * l.tile * l.tile + local.y * l.tile + local.x;
    var weight = vec2<f32>(-1.0);
    var fixed0 = vec4<f32>(0.0);
    var fixed1 = vec4<f32>(0.0);
    if all(local >= window.xy) && all(local < window.zw) {
        weight.x = bitcast<f32>(words[at]);
        fixed0 = weight.x * values[at];
    }
    if all(local + vec2<u32>(1u, 0u) >= window.xy) && all(local + vec2<u32>(1u, 0u) < window.zw) {
        weight.y = bitcast<f32>(words[at + 1u]);
        fixed1 = weight.y * values[at + 1u];
    }
    let first = (lid.y + 1u) * BLOCK_PITCH + 2u * lid.x + 1u;
    let count0 = f32(block_inside[first - 1u] + block_inside[first + 1u]
        + block_inside[first - BLOCK_PITCH] + block_inside[first + BLOCK_PITCH]);
    let count1 = f32(block_inside[first] + block_inside[first + 2u]
        + block_inside[first + 1u - BLOCK_PITCH] + block_inside[first + 1u + BLOCK_PITCH]);
    let inverse_count = vec2<f32>(1.0) / max(vec2<f32>(count0, count1), vec2<f32>(1.0));
    for (var sweep = 0u; sweep < 2u * BLOCK_SWEEPS; sweep++) {
        let parity = (lid.y + sweep) % 2u;
        let center = first + parity;
        let w = select(weight.x, weight.y, parity == 1u);
        if w >= 0.0 && w < 1.0 {
            let sum = block_values[center - 1u] + block_values[center + 1u]
                + block_values[center - BLOCK_PITCH] + block_values[center + BLOCK_PITCH];
            let count = select(count0, count1, parity == 1u);
            let average = select(block_values[center], sum * select(inverse_count.x, inverse_count.y, parity == 1u), count > 0.0);
            block_values[center] = select(fixed0, fixed1, parity == 1u) + (1.0 - w) * average;
        }
        workgroupBarrier();
    }
    if weight.x >= 0.0 && weight.x < 1.0 { membrane[at] = block_values[first]; }
    if weight.y >= 0.0 && weight.y < 1.0 { membrane[at + 1u] = block_values[first + 1u]; }
}

var<workgroup> partial: array<vec2<f32>, 256>;

// Spot Healing works on 32 x 32 blocks of a page, a workgroup of 8 x 8
// lanes each taking every eighth pixel of the block.
const SCORE_BLOCK: u32 = 32u;
const SCORE_LANES: u32 = 8u;
const LANE_PIXELS: u32 = (SCORE_BLOCK / SCORE_LANES) * (SCORE_BLOCK / SCORE_LANES);
const PAGE_BLOCKS: u32 = 256u / SCORE_BLOCK;
var<workgroup> lanes: array<vec2<f32>, SCORE_LANES * SCORE_LANES>;

// The block a workgroup covers, counted from the block holding the window's
// corner, and the page pixel its lane takes at step `i`.
fn window_block(group: vec3<u32>) -> vec2<u32> {
    return vec2<u32>(p.window.xy) / SCORE_BLOCK + group.xy;
}
fn block_texel(block: vec2<u32>, lid: vec3<u32>, i: u32) -> vec2<i32> {
    let steps = SCORE_BLOCK / SCORE_LANES;
    return vec2<i32>(block * SCORE_BLOCK + lid.xy + SCORE_LANES * vec2<u32>(i % steps, i / steps));
}

// Spot Healing: how far candidate p.candidate's source B(x + offset) is from
// B(x) over the uncovered part of p.window, for one block of a page.
@compute @workgroup_size(8, 8)
fn score(
    @builtin(workgroup_id) group: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(local_invocation_index) lane: u32,
) {
    let block = window_block(group);
    var sum = vec2<f32>(0.0);
    for (var i = 0u; i < LANE_PIXELS; i++) {
        let texel = block_texel(block, lid, i);
        if windowed(texel) {
            let w = weight_of(texel);
            if w > 0.0 {
                let d = field(destination, texel) - candidate_at(texel);
                sum += vec2<f32>(w * dot(d, d), w);
            }
        }
    }
    lanes[lane] = sum;
    workgroupBarrier();
    for (var stride = 32u; stride > 0u; stride /= 2u) {
        if lane < stride {
            lanes[lane] += lanes[lane + stride];
        }
        workgroupBarrier();
    }
    if lane == 0u {
        let blocks = PAGE_BLOCKS * PAGE_BLOCKS;
        let at = p.scores + 2u * (p.tile * blocks + block.y * PAGE_BLOCKS + block.x);
        words[at] = bitcast<u32>(lanes[0].x);
        words[at + 1u] = bitcast<u32>(lanes[0].y);
    }
}

var<workgroup> improved: bool;

// Sum candidate p.candidate's blocks into its mean difference plus its
// penalty. Candidates are judged in order and only a strictly better one
// replaces the best so far, so the first wins a tie. Each page's
// dispatch-indirect record lays this candidate down over the page's window
// only when it is the best so far.
@compute @workgroup_size(256)
fn judge(@builtin(local_invocation_index) lane: u32) {
    var sum = vec2<f32>(0.0);
    for (var block = lane; block < p.pages * PAGE_BLOCKS * PAGE_BLOCKS; block += 256u) {
        let at = p.scores + 2u * block;
        sum += vec2<f32>(bitcast<f32>(words[at]), bitcast<f32>(words[at + 1u]));
    }
    partial[lane] = sum;
    workgroupBarrier();
    for (var stride = 128u; stride > 0u; stride /= 2u) {
        if lane < stride {
            partial[lane] += partial[lane + stride];
        }
        workgroupBarrier();
    }
    if lane == 0u {
        let cost = partial[0].x / max(partial[0].y, 1e-6) + bitcast<f32>(pyramid[p.candidates + p.candidate * 3u + 2u]);
        improved = p.candidate == 0u || cost < bitcast<f32>(words[p.choice]);
        if improved {
            words[p.choice] = bitcast<u32>(cost);
            words[p.choice + 1u] = p.candidate;
        }
    }
    let lay = workgroupUniformLoad(&improved);
    for (var page = lane; page < p.pages; page += 256u) {
        let args = p.choice + 4u + 4u * page;
        let size = vec2<u32>(pyramid[p.boxes + 2u * page], pyramid[p.boxes + 2u * page + 1u]);
        words[args] = select(0u, size.x, lay);
        words[args + 1u] = select(0u, size.y, lay);
        words[args + 2u] = 1u;
    }
}

// Lay the chosen candidate's source into the page's window, from its corner.
@compute @workgroup_size(8, 8)
fn pick(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_id) lid: vec3<u32>) {
    let block = window_block(group);
    for (var i = 0u; i < LANE_PIXELS; i++) {
        let texel = block_texel(block, lid, i);
        if windowed(texel) {
            textureStore(chosen, texel - p.window.xy, candidate_at(texel));
        }
    }
}

// The level 0 membrane at page pixel `texel`, bilinear between cells when
// they span several pixels.
fn membrane_at(texel: vec2<i32>) -> vec4<f32> {
    let l = level(0u);
    if p.scale == 1u {
        return healed[p.tile * l.tile * l.tile + u32(texel.y) * l.tile + u32(texel.x)];
    }
    let at = (vec2<f32>(texel) + 0.5) / f32(p.scale) - 0.5 + vec2<f32>(tile_origin(l, p.tile));
    let base = vec2<i32>(floor(at));
    let f = at - floor(at);
    var sum = vec4<f32>(0.0);
    var weight = 0.0;
    for (var y = 0; y < 2; y++) {
        for (var x = 0; x < 2; x++) {
            let i = cell(l, base + vec2<i32>(x, y));
            if i == NONE || bitcast<f32>(known[i]) < 0.0 {
                continue;
            }
            let k = select(1.0 - f.x, f.x, x == 1) * select(1.0 - f.y, f.y, y == 1);
            sum += k * healed[i];
            weight += k;
        }
    }
    return select(vec4<f32>(0.0), sum / weight, weight > 0.0);
}

// `x` laid over `below` with coverage `c`, keeping `below`'s alpha when locked.
fn over(x: vec4<f32>, below: vec4<f32>, c: f32, locked: bool) -> vec4<f32> {
    if locked {
        return vec4<f32>(below.rgb * (1.0 - x.a * c) + x.rgb * below.a * c, below.a);
    }
    return x * c + below * (1.0 - x.a * c);
}

// Rewrite one damaged page as (S + h) over the page as the stroke found it.
// Healing's page already holds S over it, so only h's share is added; Spot
// Healing's holds a tint, so S is laid down from the source field. Where h
// changes a pixel of an integer document, its color stays within its alpha.
fn applied(texel: vec2<i32>) -> vec4<f32> {
    let current = textureLoad(painted, texel, 0);
    let c = 1.0 - weight_of(texel);
    if c <= 0.0 || !windowed(texel) {
        return current;
    }
    let below = blend_space(textureLoad(start, min(texel, vec2<i32>(textureDimensions(start)) - 1), 0));
    let h = membrane_at(texel);
    let locked = (p.flags & 2u) != 0u;
    let spot = (p.flags & 1u) != 0u;
    let share = select(
        h * c - below * (h.a * c),
        vec4<f32>(h.rgb * (below.a * c) - below.rgb * (h.a * c), 0.0),
        locked,
    );
    if !spot && perceptual() && all(share == vec4<f32>(0.0)) {
        return current;
    }
    var copied = blend_space(current);
    if spot {
        copied = over(blend_space(field(source, texel)), below, c, locked);
    }
    let result = copied + share;
    let alpha = clamp(result.a, 0.0, 1.0);
    var rgb = max(result.rgb, vec3<f32>(0.0));
    if (p.flags & 4u) != 0u && any(share != vec4<f32>(0.0)) {
        rgb = min(rgb, vec3<f32>(alpha));
    }
    if perceptual() {
        return working_decode(vec4<f32>(rgb, alpha));
    }
    return vec4<f32>(rgb, alpha);
}

@compute @workgroup_size(8, 8)
fn apply_main(@builtin(global_invocation_id) id: vec3<u32>) {
    if all(id.xy < textureDimensions(healed_page)) {
        textureStore(healed_page, vec2<i32>(id.xy), applied(vec2<i32>(id.xy)));
    }
}
