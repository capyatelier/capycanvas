struct Params { size: vec2<u32>, tiles: u32, columns: u32, origin: vec2<i32>, pad: vec2<u32> }
struct Tile { origin: vec2<i32>, start: u32, end: u32 }
struct Command { header: u32, start: u32, backdrop: i32, color: u32 }

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> tiles: array<Tile>;
@group(0) @binding(2) var<storage, read> commands: array<Command>;
@group(0) @binding(3) var<storage, read> colors: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read> pool: array<u32>;
@group(0) @binding(5) var output: texture_storage_2d<rgba32float, write>;

const UNIT: f32 = 1.0 / 2048.0;
const LANES: vec4<f32> = vec4<f32>(0.0, 1.0, 2.0, 3.0);

fn area(command: Command, count: u32, even_odd: bool, xy: vec2<f32>) -> vec4<f32> {
    var acc = vec4<f32>(f32(command.backdrop));
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
            let xmin = min(vec4<f32>(min(x0, x1)) - LANES, vec4<f32>(1.0)) - 1.0e-6;
            let xmax = vec4<f32>(max(x0, x1)) - LANES;
            let b = min(xmax, vec4<f32>(1.0));
            let c = max(b, vec4<f32>(0.0));
            let d = max(xmin, vec4<f32>(0.0));
            acc += (b + 0.5 * (d * d - c * c) - xmin) / (xmax - xmin) * dy;
        }
        var edge = 1.0e9;
        if (w0 & 0xffffu) == 0u { edge = p0.y; } else if (w1 & 0xffffu) == 0u { edge = p1.y; }
        acc += vec4<f32>(sign(delta.x) * clamp(xy.y - edge + 1.0, 0.0, 1.0));
    }
    return select(min(abs(acc), vec4<f32>(1.0)), abs(acc - 2.0 * round(0.5 * acc)), even_odd);
}

fn point(index: u32) -> vec3<f32> { return vec3<f32>(bitcast<f32>(pool[index]), bitcast<f32>(pool[index + 1u]), bitcast<f32>(pool[index + 2u])); }

fn stroke(command: Command, count: u32, first: vec2<f32>, coverage: vec4<f32>) -> vec4<f32> {
    var result = coverage;
    var a = point(command.start);
    for (var i = 0u; i < count; i++) {
        let b = point(command.start + (i + 1u) * 3u);
        let reach = max(max(a.z, b.z), 0.5) + 1.0;
        let low = min(a.xy, b.xy) - reach;
        let high = max(a.xy, b.xy) + reach;
        if high.x < first.x - 0.5 || low.x > first.x + 3.5 || high.y < first.y - 0.5 || low.y > first.y + 0.5 { a = b; continue; }
        let span = b.xy - a.xy;
        let span_length = sqrt(dot(span, span));
        let u = select(vec2<f32>(1.0, 0.0), span / span_length, span_length > 0.0);
        let ra = max(a.z, 0.5);
        let rb = max(b.z, 0.5);
        let thin = min(a.z + b.z, 1.0);
        let s = select(0.0, clamp((ra - rb) / span_length, -1.0, 1.0), span_length > 0.0);
        let c = sqrt(max(1.0 - s * s, 0.0));
        let end = span_length * c;
        let dx = vec4<f32>(first.x - a.x) + LANES;
        let dy = first.y - a.y;
        let x = dx * u.x + dy * u.y;
        let y = abs(dx * u.y - dy * u.x);
        let along = x * c - y * s;
        let far = along > vec4<f32>(end);
        let near = x - select(vec4<f32>(0.0), vec4<f32>(span_length), far);
        let round = sqrt(near * near + y * y) - select(vec4<f32>(ra), vec4<f32>(rb), far);
        let side = (along >= vec4<f32>(0.0)) & !far;
        var distance = select(round, x * s + y * c - ra, side);
        if span_length <= abs(ra - rb) {
            let center = select(vec4<f32>(0.0), vec4<f32>(span_length), rb > ra);
            let local = x - center;
            distance = sqrt(local * local + y * y) - max(ra, rb);
        }
        result = max(result, clamp(0.5 - distance, vec4<f32>(0.0), vec4<f32>(1.0)) * thin);
        a = b;
    }
    return result;
}

fn over(color: vec4<f32>, paint: vec4<f32>, coverage: f32) -> vec4<f32> {
    let source = paint * coverage;
    return source + color * (1.0 - source.a);
}

fn texel_inside(texel: vec2<i32>) -> bool { return all(texel >= vec2<i32>(0)) && all(vec2<u32>(texel) < params.size); }

@compute @workgroup_size(4, 16)
fn main(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_id) local: vec3<u32>) {
    let index = group.x + group.y * params.columns;
    if index >= params.tiles { return; }
    let tile = tiles[index];
    let local_origin = vec2<i32>(i32(local.x * 4u), i32(local.y));
    let xy = vec2<f32>(local_origin);
    let texel = tile.origin + local_origin;
    let first = vec2<f32>(texel + params.origin) + 0.5;
    var c0 = vec4<f32>(0.0);
    var c1 = vec4<f32>(0.0);
    var c2 = vec4<f32>(0.0);
    var c3 = vec4<f32>(0.0);
    var coverage = vec4<f32>(0.0);
    for (var i = tile.start; i < tile.end; i++) {
        let command = commands[i];
        let kind = command.header & 15u;
        let count = command.header >> 8u;
        if kind == 3u {
            coverage = stroke(command, count, first, coverage);
            if (command.header & 16u) != 0u { continue; }
        } else if kind == 0u {
            coverage = vec4<f32>(1.0);
        } else {
            coverage = area(command, count, kind == 2u, xy);
        }
        let paint = colors[command.color];
        c0 = over(c0, paint, coverage.x);
        c1 = over(c1, paint, coverage.y);
        c2 = over(c2, paint, coverage.z);
        c3 = over(c3, paint, coverage.w);
        coverage = vec4<f32>(0.0);
    }
    if texel_inside(texel) { textureStore(output, texel, c0); }
    if texel_inside(texel + vec2<i32>(1, 0)) { textureStore(output, texel + vec2<i32>(1, 0), c1); }
    if texel_inside(texel + vec2<i32>(2, 0)) { textureStore(output, texel + vec2<i32>(2, 0), c2); }
    if texel_inside(texel + vec2<i32>(3, 0)) { textureStore(output, texel + vec2<i32>(3, 0), c3); }
}
