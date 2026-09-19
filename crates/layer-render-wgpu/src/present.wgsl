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
@group(0) @binding(4) var coarse: texture_2d<f32>;
struct DisplayCache { info: vec4<u32>, window: vec4<u32>, grid: vec4<u32>, pages: array<u32> }
@group(0) @binding(5) var<storage, read> cache: DisplayCache;
@group(0) @binding(6) var next_mip: texture_2d<f32>;

// Coordinates of mip-cell centers. The final cell can represent less than a
// full footprint; preserve its actual position instead of stretching the image.
fn mip_coordinate(p: f32, extent: f32, scale: f32) -> f32 {
    let last = ceil(extent / scale) - 1.;
    if last <= 0. { return 0.; }
    let previous = (last - .5) * scale;
    if p > previous {
        let distance = .5 * (scale + extent - last * scale);
        return last - 1. + clamp((p - previous) / distance, 0., 1.);
    }
    return clamp(p / scale - .5, 0., last);
}
fn coarse_point(p: vec2<f32>) -> vec4<f32> {
    let extent = camera.offset_document.zw;
    let scale = f32(cache.info.y);
    let q = vec2(mip_coordinate(p.x, extent.x, scale), mip_coordinate(p.y, extent.y, scale));
    let low = vec2<u32>(floor(q));
    let high = min(low+1u, textureDimensions(coarse)-1u);
    let t = fract(q);
    return mix(mix(textureLoad(coarse, vec2<i32>(low), 0), textureLoad(coarse, vec2<i32>(i32(high.x), i32(low.y)), 0), t.x),
        mix(textureLoad(coarse, vec2<i32>(i32(low.x), i32(high.y)), 0), textureLoad(coarse, vec2<i32>(high), 0), t.x), t.y);
}
fn detail_coordinate(p: vec2<u32>) -> vec2<i32> {
    if cache.grid.z == 0u { return vec2<i32>(p); }
    let page = p / 256u;
    let entry = cache.pages[page.y * cache.info.w + page.x];
    if entry == 0u { return vec2<i32>(-1); }
    let width = cache.grid.x / 256u;
    let origin = vec2((entry-1u) % width, (entry-1u) / width) * 256u;
    return vec2<i32>(origin + p % 256u);
}
fn detail_point(p: vec2<f32>) -> vec4<f32> {
    if cache.info.z == 0u { return coarse_point(p); }
    let extent = camera.offset_document.zw;
    let scale = f32(cache.info.z);
    let q = vec2(mip_coordinate(p.x, extent.x, scale), mip_coordinate(p.y, extent.y, scale));
    // Retained levels are contiguous textures. The filtering unit can sample
    // these directly; only the page atlas needs explicit cross-page gathers.
    if cache.grid.z == 0u {
        return textureSampleLevel(canvas, canvas_sampler, (q + .5) / vec2<f32>(textureDimensions(canvas)), 0.);
    }
    let low = vec2<u32>(floor(q));
    let high = min(low+1u, vec2<u32>(ceil(extent / scale))-1u);
    if any(low < cache.window.xy) || any(high >= cache.window.zw) { return coarse_point(p); }
    let a = detail_coordinate(low);
    let b = detail_coordinate(vec2(high.x, low.y));
    let c = detail_coordinate(vec2(low.x, high.y));
    let d = detail_coordinate(high);
    if a.x < 0 || b.x < 0 || c.x < 0 || d.x < 0 { return coarse_point(p); }
    let t = fract(q);
    return mix(mix(textureLoad(canvas, a, 0), textureLoad(canvas, b, 0), t.x),
        mix(textureLoad(canvas, c, 0), textureLoad(canvas, d, 0), t.x), t.y);
}
fn artwork_at(p: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    if cache.info.x == 0u { return textureSampleLevel(canvas, canvas_sampler, p / camera.offset_document.zw, 0.); }
    let scale = f32(select(cache.info.z, cache.info.y, cache.info.z == 0u));
    let footprint = max(length(dx), length(dy));
    if footprint <= scale { return detail_point(p); }
    if cache.grid.w != 0u {
        // Adjacent levels contain the completed full-resolution composition.
        // Trilinear display sampling avoids supersampling every screen pixel;
        // no source/filter input or editable pixel is reduced.
        let next_scale = f32(cache.grid.w);
        let extent = camera.offset_document.zw;
        let q = vec2(mip_coordinate(p.x, extent.x, next_scale), mip_coordinate(p.y, extent.y, next_scale));
        let reduced = textureSampleLevel(next_mip, canvas_sampler, (q + .5) / vec2<f32>(textureDimensions(next_mip)), 0.);
        return mix(detail_point(p), reduced, clamp(log2(footprint / scale), 0., 1.));
    }
    var color = vec4(0.);
    for (var y = 0u; y < 4u; y++) {
        for (var x = 0u; x < 4u; x++) {
            let offset = (vec2(f32(x), f32(y))+.5)/4.-.5;
            color += detail_point(p + dx * offset.x + dy * offset.y);
        }
    }
    return color / 16.;
}
fn coarse_area(uv: vec2<f32>, footprint: vec2<f32>) -> vec4<f32> {
    if cache.info.x == 0u { return sample_overview(canvas, canvas_sampler, uv, footprint); }
    let extent = camera.offset_document.zw / f32(cache.info.y);
    let low = clamp((uv-footprint*.5)*extent, vec2(0.), extent);
    let high = clamp((uv+footprint*.5)*extent, low, extent);
    let start = vec2<u32>(floor(low));
    let end = min(vec2<u32>(ceil(high)), textureDimensions(coarse));
    var color = vec4(0.);
    var weight = 0.;
    for (var y = start.y; y < end.y; y++) {
        for (var x = start.x; x < end.x; x++) {
            let p = vec2(f32(x), f32(y));
            let overlap = max(vec2(0.), min(high, p+1.) - max(low, p));
            let area = overlap.x * overlap.y;
            color += textureLoad(coarse, vec2<i32>(i32(x), i32(y)), 0) * area;
            weight += area;
        }
    }
    return color / max(weight, .0000001);
}

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
    if VIEW_EXTENDED_SRGB {
        // Extended sRGB mirrors the transfer below zero. Preserve signed gamut
        // coordinates and above-white values for the browser's color transform.
        let magnitude=abs(rgb);
        return sign(rgb)*select(magnitude*12.92,1.055*pow(magnitude,vec3(1./2.4))-.055,magnitude>vec3(.0031308));
    }
    return select(rgb * 12.92, 1.055 * pow(max(rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055, rgb > vec3<f32>(0.0031308));
}
// Some attachment conversions truncate instead of rounding to nearest. Select
// the representable Float16 value explicitly; this is display-only quantization.
fn view_half(value:f32)->f32 {
    let magnitude=min(abs(value),65504.);
    if magnitude<0.00006103515625 {
        let scaled=magnitude*16777216.;let low=floor(scaled);let fraction=scaled-low;
        let rounded=low+select(0.,1.,fraction>.5 || (fraction==.5 && (u32(low)&1u)!=0u));
        return sign(value)*rounded/16777216.;
    }
    let bits=bitcast<u32>(magnitude);
    let rounded=(bits+4095u+((bits>>13u)&1u))&0xffffe000u;
    return sign(value)*bitcast<f32>(rounded);
}
fn view_store(original:vec4<f32>)->vec4<f32> {
    var c=vec4(original.rgb*VIEW_WHITE_SCALE,original.a);
    if VIEW_PQ {
        if original.a<=0. {return vec4(0.);}
        // PQ describes a bounded display derivative. The compositor maps this
        // BT.2020 signal to the monitor; extended editing samples stay intact.
        let rgb=view_bt2020(original.rgb/original.a);
        let p=pow(clamp(rgb*0.0203,vec3(0.),vec3(1.)),vec3(2610./16384.));
        let pq=pow((vec3(3424./4096.)+(2413./128.)*p)/(vec3(1.)+(2392./128.)*p),vec3(2523./32.));
        c=vec4(pq*original.a,original.a);
    }
    if VIEW_FLOAT16 {return vec4<f32>(view_half(c.r),view_half(c.g),view_half(c.b),view_half(c.a));}
    return c;
}
fn window_coverage(surface: vec2<f32>) -> f32 {
    let radius = camera.viewport.w;
    let q = abs(surface - camera.viewport.xy * 0.5) - camera.viewport.xy * 0.5 + radius;
    let distance = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
    return select(1.0, clamp(0.5 - distance, 0.0, 1.0), radius > 0.0);
}
@fragment fn fs_main(vertex: Vertex) -> @location(0) vec4<f32> {
    let surface = vertex.position.xy;
    let p = vec2<f32>(dot(camera.inverse.xz, surface), dot(camera.inverse.yw, surface)) + camera.offset_document.xy;
    let extent = camera.offset_document.zw;
    if any(p < vec2<f32>(0.)) || any(p >= extent) {
        var rgb = view_ui_rgb(camera.surround.rgb);
        if camera.viewport.z > .5 { rgb = display_color(rgb); }
        let coverage = window_coverage(surface);
        return view_store(vec4<f32>(rgb * coverage, coverage));
    }
    // Explicit LOD keeps sampling valid across the finite-canvas boundary.
    let paint = proof_artwork(artwork_at(p, camera.inverse.xy, camera.inverse.zw),p);
    let checker = select(0.80, 0.94, (i32(floor(p.x / 16.0)) + i32(floor(p.y / 16.0))) % 2 == 0);
    var rgb = view_working_rgb(paint.rgb) + vec3<f32>(checker) * (1.0 - paint.a);
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
    let coverage = window_coverage(surface);
    return view_store(vec4<f32>(rgb * coverage, coverage));
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
    let clip = window_coverage(v.position.xy);
    return view_store(vec4<f32>(vec3<f32>(white), alpha) * clip);
}

struct OverviewVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) ab: vec4<f32>,
    @location(2) @interpolate(flat) cd: vec4<f32>,
    @location(3) @interpolate(flat) outline: vec4<f32>,
    @location(4) @interpolate(flat) background_scale: vec4<f32>,
    @location(5) @interpolate(flat) clip: vec4<f32>,
    @location(6) @interpolate(flat) halo: f32,
};
@vertex fn overview_vertex(@builtin(vertex_index) index: u32,
    @location(0) bounds: vec4<f32>, @location(1) ab: vec4<f32>, @location(2) cd: vec4<f32>,
    @location(3) outline: vec4<f32>, @location(4) background_scale: vec4<f32>,
    @location(5) clip: vec4<f32>) -> OverviewVertex {
    let corners = array(vec2(0.,0.), vec2(1.,0.), vec2(0.,1.), vec2(0.,1.), vec2(1.,0.), vec2(1.,1.));
    let uv = corners[index];
    let point = bounds.xy + uv * bounds.zw;
    // Choose the more contrasting black/white surround once per vertex. A
    // constant white halo disappears with a light-colored outline in dark UI.
    let halo = select(1., 0., dot(outline.rgb, vec3(.2126,.7152,.0722)) > .179);
    return OverviewVertex(vec4(point / camera.viewport.xy * vec2(2.,-2.) + vec2(-1.,1.),0.,1.), uv, ab, cd, outline, background_scale, clip, halo);
}
fn overview_edge(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let d = b-a;
    return length(p-a-d*clamp(dot(p-a,d)/max(dot(d,d),.000001),0.,1.));
}
@fragment fn overview_fragment(v: OverviewVertex) -> @location(0) vec4<f32> {
    let footprint = fwidth(v.uv);
    let p = v.position.xy;
    if any(p < v.clip.xy) || any(p >= v.clip.xy+v.clip.zw) { discard; }
    let paint = proof_artwork(coarse_area(v.uv, footprint),v.uv*camera.offset_document.zw);
    var rgb = view_working_rgb(paint.rgb) + view_ui_rgb(v.background_scale.rgb) * (1.-paint.a);
    let edge = min(min(overview_edge(p,v.ab.xy,v.ab.zw),overview_edge(p,v.ab.zw,v.cd.xy)),
                   min(overview_edge(p,v.cd.xy,v.cd.zw),overview_edge(p,v.cd.zw,v.ab.xy)));
    let scale = v.background_scale.w;
    rgb = mix(rgb,vec3(v.halo),clamp(1.5*scale+.5-edge,0.,1.));
    rgb = mix(rgb,view_ui_rgb(v.outline.rgb),clamp(.75*scale+.5-edge,0.,1.));
    if camera.viewport.z > .5 { rgb = display_color(rgb); }
    let opacity = v.outline.a;
    // Keep destination window alpha; only mix color inside its coverage.
    return view_store(vec4(rgb * opacity * window_coverage(p),opacity));
}
