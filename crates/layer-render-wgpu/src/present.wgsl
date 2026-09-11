struct Camera {
    inverse: vec4<f32>,
    offset_document: vec4<f32>,
    viewport: vec4<f32>,
    surround: vec4<f32>,
    selection: vec4<f32>,
    selection_inverse: vec4<f32>,
};
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var canvas: texture_2d<f32>;
@group(0) @binding(2) var canvas_sampler: sampler;
struct Selection { rect: vec4<u32>, info: vec4<u32>, values: array<u32> }
@group(0) @binding(3) var<storage, read> selection: Selection;

fn selected(p: vec2<f32>) -> bool {
    if any(p < vec2<f32>(0.)) || any(p >= camera.offset_document.zw) { return false; }
    let local = vec2<f32>(dot(camera.selection_inverse.xz, p), dot(camera.selection_inverse.yw, p)) + camera.selection.xy;
    let q = vec2<i32>(floor(local)) - vec2<i32>(selection.rect.xy);
    var covered = false;
    if all(q >= vec2<i32>(0)) && all(q < vec2<i32>(selection.rect.zw)) {
        let word = u32(q.y) * ((selection.rect.z+7u)/8u) + u32(q.x)/8u;
        covered = ((selection.values[word] >> ((u32(q.x)%8u)*4u)) & 15u) >= 2u;
    }
    return covered != (camera.selection.w > .5);
}

struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> Vertex {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return Vertex(vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0), uv);
}
fn display_color(rgb: vec3<f32>) -> vec3<f32> {
    return select(rgb * 12.92, 1.055 * pow(max(rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055, rgb > vec3<f32>(0.0031308));
}
@fragment fn fs_main(vertex: Vertex) -> @location(0) vec4<f32> {
    let surface = vertex.uv * camera.viewport.xy;
    let p = vec2<f32>(dot(camera.inverse.xz, surface), dot(camera.inverse.yw, surface)) + camera.offset_document.xy;
    let extent = camera.offset_document.zw;
    // Explicit LOD keeps sampling valid across the finite-canvas boundary.
    let paint = textureSampleLevel(canvas, canvas_sampler, p / extent, 0.0);
    let checker = select(0.80, 0.94, (i32(floor(p.x / 16.0)) + i32(floor(p.y / 16.0))) % 2 == 0);
    var rgb = paint.rgb + vec3<f32>(checker) * (1.0 - paint.a);
    if any(p < vec2<f32>(0.0)) || any(p >= extent) {rgb = camera.surround.rgb;}
    if camera.viewport.z > 0.5 {rgb = display_color(rgb);}
    // Raster-selection outlines are sampled at display resolution, never
    // traced/tessellated on the CPU or baked into the document composition.
    if camera.selection.z > .5 {
        let dx = camera.inverse.xy * .6;
        let dy = camera.inverse.zw * .6;
        if selected(p-dx) != selected(p+dx) || selected(p-dy) != selected(p+dy) {
            rgb = vec3<f32>(select(0., 1., (surface.x+surface.y) % 6. < 3.));
        }
    }
    // Signed distance to the full-window rounded rectangle; no inset/cropping.
    let radius = camera.viewport.w;
    let q = abs(surface - camera.viewport.xy * 0.5) - camera.viewport.xy * 0.5 + radius;
    let distance = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
    let coverage = select(1.0, clamp(0.5 - distance, 0.0, 1.0), radius > 0.0);
    return vec4<f32>(rgb * coverage, coverage);
}

struct CursorVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) line: vec4<f32>,
};
@vertex fn cursor_vertex(@builtin(vertex_index) vertex: u32,
    @location(0) start_point: vec2<f32>, @location(1) end_point: vec2<f32>,
    @location(2) offset: f32, @location(3) marker: f32, @location(4) scale: f32) -> CursorVertex {
    let corners = array<vec2<f32>, 6>(vec2<f32>(0.,-1.), vec2<f32>(1.,-1.), vec2<f32>(0.,1.), vec2<f32>(0.,1.), vec2<f32>(1.,-1.), vec2<f32>(1.,1.));
    if marker > 1.5 {
        let half = abs(end_point - start_point) * 0.5;
        let local = vec2<f32>(corners[vertex].x * 2. - 1., corners[vertex].y) * (half + 0.5);
        let point = (start_point + end_point) * 0.5 + local;
        return CursorVertex(vec4<f32>(point / camera.viewport.xy * vec2<f32>(2.,-2.) + vec2<f32>(-1.,1.),0.,1.), local, vec4<f32>(half, marker, scale));
    }
    let extent = length(end_point-start_point);
    let along = (end_point-start_point) / max(extent, 0.0001);
    let corner = corners[vertex];
    let local = vec2<f32>(corner.x * (extent + 4.0 * scale) - 2.0 * scale, corner.y * 2.5 * scale);
    let point = start_point + along * local.x + vec2<f32>(-along.y, along.x) * local.y;
    return CursorVertex(vec4<f32>(point / camera.viewport.xy * vec2<f32>(2.,-2.) + vec2<f32>(-1.,1.),0.,1.), local, vec4<f32>(extent, offset, marker, scale));
}
@fragment fn cursor_fragment(v: CursorVertex) -> @location(0) vec4<f32> {
    let distance = length(vec2<f32>(max(max(-v.local.x, v.local.x-v.line.x), 0.0), v.local.y));
    let scale = v.line.w;
    var alpha = clamp(select(0.5, 1.5, v.line.z > 0.5) * scale + 0.5 - distance, 0.0, 1.0);
    var white = clamp(0.5 * scale + 0.5 - distance, 0.0, 1.0) * select(select(0.0, 1.0, ((v.local.x + v.line.y) / scale) % 6.0 < 3.0), 1.0, v.line.z > 0.5);
    if v.line.z > 1.5 {
        let edge = abs(v.local) - v.line.xy;
        let d = max(edge.x, edge.y);
        alpha = clamp(0.5 - d, 0., 1.);
        white = clamp(0.5 - scale - d, 0., 1.);
    }
    let radius = camera.viewport.w;
    let q = abs(v.position.xy-camera.viewport.xy*0.5)-camera.viewport.xy*0.5+radius;
    let clip = select(1.0, clamp(0.5-length(max(q,vec2<f32>(0.0)))-min(max(q.x,q.y),0.0)+radius,0.0,1.0), radius > 0.0);
    return vec4<f32>(vec3<f32>(white), alpha) * clip;
}
