struct Params { size: vec2<u32>, tiles: u32, columns: u32, base: u32, pad: u32, origin: vec2<i32> }
struct Tile { origin: vec2<i32>, start: u32, end: u32 }
struct Command { header: u32, start: u32, backdrop: i32, color: u32 }

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> tiles: array<Tile>;
@group(0) @binding(2) var<storage, read> commands: array<Command>;
@group(0) @binding(3) var<storage, read> colors: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read> pool: array<u32>;
@group(0) @binding(5) var base: texture_2d<f32>;
@group(0) @binding(6) var output: texture_storage_2d<rgba32float, write>;

const PIXELS: u32 = 4u;
const UNIT: f32 = 1.0 / 2048.0;

fn area(command: Command, count: u32, even_odd: bool, xy: vec2<f32>) -> array<f32, 4> {
    let backdrop = f32(command.backdrop);
    var acc = array<f32, 4>(backdrop, backdrop, backdrop, backdrop);
    for (var i = 0u; i < count; i++) {
        let w0 = pool[command.start + i * 2u];
        let w1 = pool[command.start + i * 2u + 1u];
        let p0 = vec2<f32>(f32(w0 & 0xffffu), f32(w0 >> 16u)) * UNIT;
        let p1 = vec2<f32>(f32(w1 & 0xffffu), f32(w1 >> 16u)) * UNIT;
        let delta = p1 - p0;
        let y = p0.y - xy.y;
        let y0 = clamp(y, 0.0, 1.0);
        let y1 = clamp(y + delta.y, 0.0, 1.0);
        let dy = y0 - y1;
        if dy != 0.0 {
            let inverse = 1.0 / delta.y;
            let start = p0.x - xy.x;
            let x0 = start + (y0 - y) * inverse * delta.x;
            let x1 = start + (y1 - y) * inverse * delta.x;
            let low = min(x0, x1);
            let high = max(x0, x1);
            for (var k = 0u; k < PIXELS; k++) {
                let offset = f32(k);
                let xmin = min(low - offset, 1.0) - 1.0e-6;
                let xmax = high - offset;
                let b = min(xmax, 1.0);
                let c = max(b, 0.0);
                let d = max(xmin, 0.0);
                acc[k] += (b + 0.5 * (d * d - c * c) - xmin) / (xmax - xmin) * dy;
            }
        }
        var edge = 1.0e9;
        if (w0 & 0xffffu) == 0u { edge = p0.y; } else if (w1 & 0xffffu) == 0u { edge = p1.y; }
        let cover = sign(delta.x) * clamp(xy.y - edge + 1.0, 0.0, 1.0);
        for (var k = 0u; k < PIXELS; k++) { acc[k] += cover; }
    }
    for (var k = 0u; k < PIXELS; k++) {
        acc[k] = select(min(abs(acc[k]), 1.0), abs(acc[k] - 2.0 * round(0.5 * acc[k])), even_odd);
    }
    return acc;
}

fn point(index: u32) -> vec3<f32> { return vec3<f32>(bitcast<f32>(pool[index]), bitcast<f32>(pool[index + 1u]), bitcast<f32>(pool[index + 2u])); }

fn stroke(command: Command, count: u32, xy: vec2<f32>, coverage: ptr<function, array<f32, 4>>) {
    let first = xy + 0.5;
    var a = point(command.start);
    for (var i = 0u; i < count; i++) {
        let b = point(command.start + (i + 1u) * 3u);
        let span = b.xy - a.xy;
        let reach = sqrt(dot(span, span));
        let u = select(vec2<f32>(1.0, 0.0), span / reach, reach > 0.0);
        let ra = max(a.z, 0.5);
        let rb = max(b.z, 0.5);
        let thin = min(a.z + b.z, 1.0);
        let s = select(0.0, clamp((ra - rb) / reach, -1.0, 1.0), reach > 0.0);
        let c = sqrt(max(1.0 - s * s, 0.0));
        let end = reach * c;
        for (var k = 0u; k < PIXELS; k++) {
            let d = first + vec2<f32>(f32(k), 0.0) - a.xy;
            let x = dot(d, u);
            let y = abs(d.x * u.y - d.y * u.x);
            let along = x * c - y * s;
            let far = along > end;
            var distance = length(vec2<f32>(x - select(0.0, reach, far), y)) - select(ra, rb, far);
            if along >= 0.0 && along <= end { distance = x * s + y * c - ra; }
            (*coverage)[k] = max((*coverage)[k], clamp(0.5 - distance, 0.0, 1.0) * thin);
        }
        a = b;
    }
}

@compute @workgroup_size(4, 16)
fn main(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_id) local: vec3<u32>) {
    let index = group.x + group.y * params.columns;
    if index >= params.tiles { return; }
    let tile = tiles[index];
    let local_origin = vec2<i32>(i32(local.x * PIXELS), i32(local.y));
    let xy = vec2<f32>(local_origin);
    let first = tile.origin + local_origin;
    var color = array<vec4<f32>, 4>(vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0));
    if params.base != 0u {
        for (var k = 0u; k < PIXELS; k++) {
            let texel = first + vec2<i32>(i32(k), 0);
            if all(texel >= vec2<i32>(0)) && all(vec2<u32>(texel) < params.size) { color[k] = textureLoad(base, texel, 0); }
        }
    }
    var coverage = array<f32, 4>(0.0, 0.0, 0.0, 0.0);
    for (var i = tile.start; i < tile.end; i++) {
        let command = commands[i];
        let kind = command.header & 15u;
        let count = command.header >> 8u;
        if kind == 3u {
            stroke(command, count, vec2<f32>(first + params.origin), &coverage);
            if (command.header & 16u) != 0u { continue; }
        } else if kind == 0u {
            coverage = array<f32, 4>(1.0, 1.0, 1.0, 1.0);
        } else {
            coverage = area(command, count, kind == 2u, xy);
        }
        let paint = colors[command.color];
        for (var k = 0u; k < PIXELS; k++) {
            let source = paint * coverage[k];
            color[k] = source + color[k] * (1.0 - source.a);
        }
        coverage = array<f32, 4>(0.0, 0.0, 0.0, 0.0);
    }
    for (var k = 0u; k < PIXELS; k++) {
        let texel = first + vec2<i32>(i32(k), 0);
        if all(texel >= vec2<i32>(0)) && all(vec2<u32>(texel) < params.size) { textureStore(output, texel, color[k]); }
    }
}
