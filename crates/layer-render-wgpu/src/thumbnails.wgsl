// Bounds and framing stay on the GPU. Only the finished 32px image is mapped.
struct Record { tile: vec4<u32>, options: vec4<u32>, color: vec4<f32> }
@group(1) @binding(0) var<uniform> record: Record;
@group(1) @binding(1) var pixels: texture_2d<f32>;
@group(1) @binding(2) var sampling: sampler;
struct AtomicBounds { x: atomic<u32>, y: atomic<u32>, right: atomic<u32>, bottom: atomic<u32> }
@group(0) @binding(0) var<storage, read_write> output_bounds: AtomicBounds;
@group(0) @binding(1) var<storage, read> bounds: vec4<u32>;
var<workgroup> partial: array<vec4<u32>, 64>;
@compute @workgroup_size(8, 8)
fn measure(@builtin(local_invocation_index) i: u32, @builtin(global_invocation_id) gid: vec3<u32>) {
    var b = vec4<u32>(0xffffffffu, 0xffffffffu, 0u, 0u);
    for (var y = 0u; y < 4u; y++) { for (var x = 0u; x < 4u; x++) {
        let p = gid.xy * 4u + vec2<u32>(x, y);
        let world = p + record.tile.xy;
        if all(p < vec2<u32>(256u)) && all(world < record.tile.zw) {
            let raw = textureLoad(pixels, vec2<i32>(p), 0);
            var a = raw.a;
            if record.options.x == 1u { a = select(raw.r, 1. - raw.r, record.options.y == 1u); }
            if a > 0.0001 { b = vec4<u32>(min(b.xy, world), max(b.zw, world + 1u)); }
        }
    }}
    partial[i] = b;
    workgroupBarrier();
    for (var stride = 32u; stride > 0u; stride /= 2u) {
        if i < stride { partial[i] = vec4<u32>(min(partial[i].xy, partial[i+stride].xy), max(partial[i].zw, partial[i+stride].zw)); }
        workgroupBarrier();
    }
    if i == 0u && partial[0].x != 0xffffffffu {
        atomicMin(&output_bounds.x, partial[0].x); atomicMin(&output_bounds.y, partial[0].y);
        atomicMax(&output_bounds.right, partial[0].z); atomicMax(&output_bounds.bottom, partial[0].w);
    }
}
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vertex_main(@builtin(vertex_index) i: u32) -> Vertex {
    let uv = array<vec2<f32>, 3>(vec2<f32>(0.,0.),vec2<f32>(2.,0.),vec2<f32>(0.,2.))[i];
    var p = uv * 32.;
    if record.options.x < 2u {
        let low = vec2<f32>(bounds.xy);
        let size = max(vec2<f32>(bounds.zw) - low, vec2<f32>(1.));
        let scale = 32. / max(size.x, size.y);
        p = (vec2<f32>(record.tile.xy) + uv * 256. - low) * scale + (32. - size * scale) * .5;
    }
    return Vertex(vec4<f32>(p.x/16.-1., 1.-p.y/16., 0., 1.), uv);
}
@fragment fn fragment_main(v: Vertex) -> @location(0) vec4<f32> {
    if record.options.x >= 2u {
        // Neutral checker colors in linear light (sRGB #bbb / #eee).
        let square = (u32(v.position.x)/4u + u32(v.position.y)/4u) % 2u;
        let gray = select(.497, .855, square == 0u);
        return vec4<f32>(record.color.rgb * record.color.a + gray * (1.-record.color.a), 1.);
    }
    if any(v.uv < vec2<f32>(0.)) || any(v.uv > vec2<f32>(1.)) { discard; }
    if any(vec2<f32>(record.tile.xy) + v.uv * 256. >= vec2<f32>(record.tile.zw)) { discard; }
    let raw = textureSample(pixels, sampling, v.uv);
    if record.options.x == 1u {
        let gray = select(raw.r, 1.-raw.r, record.options.y == 1u);
        return vec4<f32>(gray, gray, gray, 1.);
    }
    return raw;
}
