struct Camera {
    inverse: vec4<f32>,
    offset_document: vec4<f32>,
    viewport: vec4<f32>,
    surround: vec4<f32>,
    selection: vec4<f32>,
    selection_inverse: vec4<f32>,
    rotation: vec4<f32>,
    overlay: vec4<f32>,
    crop: vec4<f32>,
    crop_offset: vec4<f32>,
    placed_x: vec4<f32>, placed_y: vec4<f32>, placed_extent: vec4<f32>,
    placed_options: vec4<f32>, placed_backdrop: vec4<f32>,
    composite: vec4<f32>,
    mapped: Resample,
};
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var moved: texture_2d<f32>;
@group(0) @binding(2) var linear_sampler: sampler;
struct Selection { rect: vec4<u32>, info: vec4<u32>, values: array<u32> }
@group(0) @binding(3) var<storage, read> selection: Selection;
@group(0) @binding(11) var saved_selection: texture_2d<u32>;
@group(0) @binding(4) var coarse: texture_2d<f32>;
struct DisplayCache { scale: u32, coarse_scale: u32, next_scale: u32, padding: u32, window: vec4<u32> }
@group(0) @binding(5) var<storage, read> cache: DisplayCache;
@group(0) @binding(6) var kept: texture_2d<f32>;
@group(0) @binding(13) var navigator: texture_2d<f32>;

// The backing buffer follows the display's native orientation. All artwork,
// cursor and UI geometry continues to use the host's logical viewport.
fn logical_surface(p: vec2<f32>) -> vec2<f32> {
    switch u32(camera.rotation.x) {
        case 1u: { return vec2(p.y, camera.viewport.y - p.x); }
        case 2u: { return camera.viewport.xy - p; }
        case 3u: { return vec2(camera.viewport.x - p.y, p.x); }
        default: { return p; }
    }
}
fn surface_clip(p: vec2<f32>) -> vec4<f32> {
    let clip = p / camera.viewport.xy * vec2(2., -2.) + vec2(-1., 1.);
    switch u32(camera.rotation.x) {
        case 1u: { return vec4(clip.y, -clip.x, 0., 1.); }
        case 2u: { return vec4(-clip, 0., 1.); }
        case 3u: { return vec4(-clip.y, clip.x, 0., 1.); }
        default: { return vec4(clip, 0., 1.); }
    }
}

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
    let scale = f32(cache.coarse_scale);
    let q = vec2(mip_coordinate(p.x, extent.x, scale), mip_coordinate(p.y, extent.y, scale));
    let low = vec2<u32>(floor(q));
    let high = min(low+1u, textureDimensions(coarse)-1u);
    let t = fract(q);
    return mix(mix(textureLoad(coarse, vec2<i32>(low), 0), textureLoad(coarse, vec2<i32>(i32(high.x), i32(low.y)), 0), t.x),
        mix(textureLoad(coarse, vec2<i32>(i32(low.x), i32(high.y)), 0), textureLoad(coarse, vec2<i32>(high), 0), t.x), t.y);
}
fn detail_point(p: vec2<f32>) -> vec4<f32> {
    let extent = camera.offset_document.zw;
    let scale = f32(cache.scale);
    let q = vec2(mip_coordinate(p.x, extent.x, scale), mip_coordinate(p.y, extent.y, scale));
    if any(q < vec2<f32>(cache.window.xy)) || any(q > vec2<f32>(cache.window.zw - 1u)) { return coarse_point(p); }
    return textureSampleLevel(moved, linear_sampler, (q - vec2<f32>(cache.window.xy) + .5) / vec2<f32>(textureDimensions(moved)), 0.);
}
fn placed_at(p:vec2<f32>,dx:vec2<f32>,dy:vec2<f32>,linear:bool)->vec4<f32> {
    let span=abs(dx)+abs(dy);
    let low=max(p-span*.5,vec2(0.));
    let high=min(p+span*.5,camera.offset_document.zw);
    let fraction=(high-low)/max(span,vec2(1e-20));
    let h=vec3((low+high)*.5,1.);
    let center=vec2(dot(camera.placed_x.xyz,h),dot(camera.placed_y.xyz,h));
    let u=vec2(dot(camera.placed_x.xy,dx*fraction),dot(camera.placed_y.xy,dx*fraction))*.25;
    let v=vec2(dot(camera.placed_x.xy,dy*fraction),dot(camera.placed_y.xy,dy*fraction))*.25;
    let footprint=max(length(u),length(v));
    let extent=camera.placed_extent.xy;
    let outside=camera.placed_options.z;
    var color:vec4<f32>;
    if footprint<=1. {
        color=area_sample(moved,linear_sampler,extent,outside,center,u,v);
    } else if footprint<=camera.placed_extent.w {
        let ratio=camera.placed_extent.w;
        color=area_sample(kept,linear_sampler,extent/ratio,outside,center/ratio,u/ratio,v/ratio);
    } else {
        let ratio=camera.placed_extent.z;
        color=area_sample(coarse,linear_sampler,extent/ratio,outside,center/ratio,u/ratio,v/ratio);
    }
    return presentation_color(color,camera.placed_options.x,camera.placed_options.w,camera.placed_backdrop,linear);
}
fn presentation_color(color:vec4<f32>,opacity:f32,encode:f32,backdrop:vec4<f32>,linear:bool)->vec4<f32> {
    if linear && encode!=0. && (color.a*opacity==1. || backdrop.a==0.) {return color*opacity;}
    var layer=color;
    if encode!=0. && color.a>0. {layer=vec4(sdr_encode(color.rgb/color.a,CANVAS_SPACE)*color.a,color.a);}
    layer*=opacity;
    let result=layer+backdrop*(1.-layer.a);
    if linear {return canvas_linear(result);}
    return result;
}
fn mapped_at(p:vec2<f32>,dx:vec2<f32>,dy:vec2<f32>,linear:bool)->vec4<f32> {
    let span=abs(dx)+abs(dy);
    let low=max(p-span*.5,vec2(0.));
    let high=min(p+span*.5,camera.offset_document.zw);
    let fraction=(high-low)/max(span,vec2(1e-20));
    let color=mapped_color(8u,(low+high)*.5,dx*fraction,dy*fraction,false,false,vec2(0.),vec2(0.),vec2(0.));
    return presentation_color(color,camera.mapped.options.x,camera.mapped.encoding.x,camera.mapped.backdrop,linear);
}
fn artwork_at(p: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    if camera.placed_options.y==2. {return mapped_at(p,dx,dy,false);}
    if camera.placed_options.y!=0. { return placed_at(p,dx,dy,false); }
    return grid_artwork_at(p,dx,dy);
}
fn grid_artwork_at(p: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    let scale = f32(cache.scale);
    let footprint = max(length(dx), length(dy));
    if footprint <= scale { return detail_point(p); }
    let next_scale = f32(cache.next_scale);
    let extent = camera.offset_document.zw;
    let q = vec2(mip_coordinate(p.x, extent.x, next_scale), mip_coordinate(p.y, extent.y, next_scale));
    let origin = vec2<f32>(cache.window.xy) * (scale / next_scale);
    let reduced = textureSampleLevel(kept, linear_sampler, (q - origin + .5) / vec2<f32>(textureDimensions(kept)), 0.);
    return mix(detail_point(p), reduced, clamp(log2(footprint / scale), 0., 1.));
}
// Linear premultiplied artwork from the composite, which holds the
// document's encoded values when it blends perceptually (composite.x).
fn canvas_linear(c: vec4<f32>) -> vec4<f32> {
    if camera.composite.x == 0. || c.a <= 0. { return c; }
    return vec4<f32>(sdr_decode(c.rgb / c.a, CANVAS_SPACE) * c.a, c.a);
}

fn selection_at(q: vec2<i32>) -> f32 {
    if any(q < vec2<i32>(0)) || any(q >= vec2<i32>(selection.rect.zw)) { return 0.; }
    let bytes = selection.info.y == 2u;
    let count = select(8u,4u,bytes);
    let word = u32(q.y) * ((selection.rect.z+count-1u)/count) + u32(q.x)/count;
    return f32((selection.values[word] >> ((u32(q.x)%count)*(32u/count))) & select(15u,255u,bytes))/select(4.,255.,bytes);
}
// Clipping resamples coverage placed with a scale or rotation bilinearly, so
// the outline does too; translated pixels keep their exact edges.
fn resampled_selection() -> bool {
    return any(camera.selection_inverse != vec4<f32>(1., 0., 0., 1.));
}
// Bilinear coverage at document point p and its document-space gradient.
fn selection_sample(p: vec2<f32>) -> vec3<f32> {
    let local = vec2<f32>(dot(camera.selection_inverse.xz, p), dot(camera.selection_inverse.yw, p)) + camera.selection.xy;
    let q = local - .5 - vec2<f32>(selection.rect.xy);
    let base = vec2<i32>(floor(q));
    let f = fract(q);
    let a = selection_at(base);
    let b = selection_at(base + vec2<i32>(1, 0));
    let c = selection_at(base + vec2<i32>(0, 1));
    let d = selection_at(base + vec2<i32>(1, 1));
    let cell = vec2<f32>(mix(b - a, d - c, f.y), mix(c - a, d - b, f.x));
    let gradient = vec2<f32>(dot(cell, camera.selection_inverse.xy), dot(cell, camera.selection_inverse.zw));
    return vec3<f32>(mix(mix(a, b, f.x), mix(c, d, f.x), f.y), gradient);
}
fn selection_coverage(p: vec2<f32>) -> f32 {
    if any(p < vec2<f32>(0.)) || any(p >= camera.offset_document.zw) { return 0.; }
    var covered = 0.;
    if resampled_selection() {
        covered = selection_sample(p).x;
    } else {
        let local = vec2<f32>(dot(camera.selection_inverse.xz, p), dot(camera.selection_inverse.yw, p)) + camera.selection.xy;
        covered = selection_at(vec2<i32>(floor(local)) - vec2<i32>(selection.rect.xy));
    }
    return select(covered,1.-covered,camera.selection.w > .5);
}
fn selected(p: vec2<f32>) -> bool { return selection_coverage(p) >= .5; }
// The outline crosses a pixel where half coverage lies within dx or dy of p.
// Resampled coverage is linear enough within a pixel to find that crossing
// from one sample and its gradient, away from the document's edges.
fn outlined(p: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> bool {
    let reach = abs(dx) + abs(dy);
    if resampled_selection() && all(p - reach >= vec2<f32>(0.)) && all(p + reach < camera.offset_document.zw) {
        let sample = selection_sample(p);
        return abs(sample.x - .5) < max(abs(dot(sample.yz, dx)), abs(dot(sample.yz, dy)));
    }
    return selected(p-dx) != selected(p+dx) || selected(p-dy) != selected(p+dy);
}

// The crop tool's shield. `crop` maps document pixels onto the crop's unit
// square; `crop_offset.z` is the shield opacity and `.w` enables it.
fn inside_crop(p: vec2<f32>) -> bool {
    let q = vec2<f32>(dot(camera.crop.xz, p), dot(camera.crop.yw, p)) + camera.crop_offset.xy;
    return all(q >= vec2<f32>(0.)) && all(q <= vec2<f32>(1.));
}

struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> Vertex {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return Vertex(vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0), uv);
}
// Extended sRGB mirrors the transfer below zero. Preserve signed gamut
// coordinates and above-white values for the browser's color transform.
fn display_color(rgb: vec3<f32>) -> vec3<f32> {
    return sdr_encode(select(max(rgb, vec3(0.)), rgb, VIEW_EXTENDED_SRGB), 0u);
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
@fragment fn fs_main(vertex: Vertex) -> @location(0) vec4<f32> {return surface_color(vertex,0u);}
@fragment fn fs_placed(vertex: Vertex) -> @location(0) vec4<f32> {return surface_color(vertex,1u);}
@fragment fn fs_mapped(vertex: Vertex) -> @location(0) vec4<f32> {return surface_color(vertex,2u);}
fn surface_color(vertex:Vertex,source:u32)->vec4<f32> {
    let footprint = camera.rotation.w;
    let surface = logical_surface(vertex.position.xy * footprint);
    let p = vec2<f32>(dot(camera.inverse.xz, surface), dot(camera.inverse.yw, surface)) + camera.offset_document.xy;
    let extent = camera.offset_document.zw;
    let cropping = camera.crop_offset.w > .5;
    if any(p < vec2<f32>(0.)) || any(p >= extent) {
        var rgb = view_ui_rgb(camera.surround.rgb);
        if cropping && inside_crop(p) {
            rgb = vec3<f32>(select(0.80, 0.94, (i32(floor(p.x / 16.0)) + i32(floor(p.y / 16.0))) % 2 == 0));
        }
        if camera.viewport.z > .5 { rgb = display_color(rgb); }
        let coverage = window_coverage(surface);
        return view_store(vec4<f32>(rgb * coverage, coverage));
    }
    // Explicit LOD keeps sampling valid across the finite-canvas boundary.
    var artwork: vec4<f32>;
    if source==2u { artwork = mapped_at(p, camera.inverse.xy * footprint, camera.inverse.zw * footprint, true); }
    else if source==1u { artwork = placed_at(p, camera.inverse.xy * footprint, camera.inverse.zw * footprint, true); }
    else { artwork = canvas_linear(grid_artwork_at(p, camera.inverse.xy * footprint, camera.inverse.zw * footprint)); }
    let paint = proof_artwork(artwork,p);
    let checker = select(0.80, 0.94, (i32(floor(p.x / 16.0)) + i32(floor(p.y / 16.0))) % 2 == 0);
    var rgb = screen_marked(paint, view_working_rgb(paint.rgb) + vec3<f32>(checker) * (1.0 - paint.a), checker);
    if (bitcast<u32>(artwork.a)&0x7fffffffu)!=0u {
        let bits=bitcast<vec3<u32>>(artwork.rgb);
        let shadows=camera.composite.z!=0. && (any((bits&vec3(0x7fffffffu))==vec3(0u)) || any((bits>>vec3(31u))!=vec3(0u)));
        let highlights=camera.composite.w!=0. && any(select(vec3(false), bits>=vec3(bitcast<u32>(artwork.a)), bits<vec3(0x80000000u)));
        let stripe=(u32(surface.x+surface.y)/4u)%2u==0u;
        if shadows && highlights {rgb=vec3(select(0.,1.,stripe));}
        else if shadows {rgb=select(vec3(0.02,0.05,0.5),vec3(0.12,0.3,1.),stripe);}
        else if highlights {rgb=select(vec3(0.5,0.02,0.02),vec3(1.,0.2,0.05),stripe);}
    }
    if camera.viewport.z > 0.5 {rgb = display_color(rgb);}
    // Raster-selection outlines are sampled at display resolution, never
    // traced/tessellated on the CPU or baked into the document composition.
    var tint = 0.;
    if camera.rotation.z > .5 {
        let q=vec2<u32>(p);
        let saved=unpack4x8unorm(textureLoad(saved_selection,vec2<i32>(q),0).r);
        if saved.a > 0. { rgb=mix(rgb,view_ui_rgb(saved.rgb/saved.a),saved.a); }
    }
    if camera.selection.z > .5 && camera.rotation.y > .5 {
        let coverage = selection_coverage(p);
        tint = max(tint,select(coverage,1.-coverage,camera.rotation.y > 1.5));
    }
    rgb = mix(rgb,view_ui_rgb(camera.overlay.rgb),clamp(tint*camera.overlay.a,0.,1.));
    if camera.selection.z > .5 && camera.rotation.y < .5 {
        let dx = camera.inverse.xy * .6;
        let dy = camera.inverse.zw * .6;
        if outlined(p, dx, dy) {
            rgb = vec3<f32>(select(0., 1., (surface.x+surface.y) % 6. < 3.));
        }
    }
    if cropping && !inside_crop(p) {
        let keep = 1. - camera.crop_offset.z;
        rgb = select(rgb * keep, sdr_encode(sdr_decode(rgb, 0u) * keep, 0u), camera.viewport.z > .5);
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
        let margin = select(0.5, 0.75 * scale + 0.5, marker > 3.5);
        let local = vec2<f32>(corners[vertex].x * 2. - 1., corners[vertex].y) * (half + margin);
        let point = (start_point + end_point) * 0.5 + local;
        return CursorVertex(surface_clip(point), local, vec4<f32>(half, marker, scale));
    }
    let extent = length(end_point-start_point);
    let along = (end_point-start_point) / max(extent, 0.0001);
    let corner = corners[vertex];
    let local = vec2<f32>(corner.x * (extent + 4.0 * scale) - 2.0 * scale, corner.y * 2.5 * scale);
    let point = start_point + along * local.x + vec2<f32>(-along.y, along.x) * local.y;
    return CursorVertex(surface_clip(point), local, vec4<f32>(extent, offset, marker, scale));
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
    if v.line.z > 2.5 && v.line.z < 3.5 {
        // Clockwise triangle, with its upper-left tip at the input position.
        let a = -v.line.xy;
        let b = vec2<f32>(0., v.line.y);
        let c = vec2<f32>(v.line.x, v.line.y * 0.4);
        let ab = b - a;
        let bc = c - b;
        let ca = a - c;
        let pa = v.local - a;
        let pb = v.local - b;
        let pc = v.local - c;
        let d = max(max((ab.x * pa.y - ab.y * pa.x) / length(ab),
            (bc.x * pb.y - bc.y * pb.x) / length(bc)),
            (ca.x * pc.y - ca.y * pc.x) / length(ca));
        alpha = clamp(0.5 - d, 0., 1.);
        white = alpha - clamp(0.5 - scale - d, 0., 1.);
    }
    if v.line.z > 3.5 {
        let p = abs(v.local);
        let half_stroke = max(1., round(scale)) * 0.5;
        var d = min(max(p.x - v.line.x, p.y - half_stroke),
            max(p.y - v.line.y, p.x - half_stroke));
        if v.line.z > 4.5 {
            // Four square-ended arms with a clear center gap and a dark dot.
            let arm = abs(p - vec2<f32>(5. * scale)) - vec2<f32>(2. * scale);
            d = min(min(max(arm.x, p.y - half_stroke),
                max(arm.y, p.x - half_stroke)), max(p.x, p.y) - half_stroke);
        }
        alpha = clamp(0.75 * scale + 0.5 - d, 0., 1.);
        let dark = clamp(0.5 - d, 0., 1.);
        white = alpha - dark;
    }
    let clip = window_coverage(logical_surface(v.position.xy));
    if v.line.z > 5.5 {
        let radius = v.line.x - scale;
        let ring = abs(length(v.local) - radius) - scale;
        let slash = max(abs(v.local.x + v.local.y) * 0.70710678 - scale,
            length(v.local) - radius);
        let d = min(ring, slash);
        let ink = clamp(0.5 - d, 0., 1.);
        let halo = clamp(scale + 0.5 - d, 0., 1.);
        return view_store(vec4<f32>(vec3<f32>(0.75, 0.025, 0.02) * ink
            + vec3<f32>(halo - ink), halo) * clip);
    }
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
    return OverviewVertex(surface_clip(point), uv, ab, cd, outline, background_scale, clip, halo);
}
fn segment_distance(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let d = b-a;
    return length(p-a-d*clamp(dot(p-a,d)/max(dot(d,d),.000001),0.,1.));
}
@fragment fn overview_fragment(v: OverviewVertex) -> @location(0) vec4<f32> {
    let p = logical_surface(v.position.xy);
    if any(p < v.clip.xy) || any(p >= v.clip.xy+v.clip.zw) { discard; }
    let extent = camera.offset_document.zw;
    let point = v.uv * extent;
    let q = vec2(mip_coordinate(point.x, extent.x, camera.composite.y), mip_coordinate(point.y, extent.y, camera.composite.y));
    let size = vec2<f32>(textureDimensions(navigator));
    let uv = (q + .5) / size;
    let footprint = fwidth(v.uv);
    var sample = canvas_linear(textureSampleLevel(navigator, linear_sampler, uv, 0.));
    if max(footprint.x * size.x, footprint.y * size.y) > 4. {
        sample = vec4(0.);
        for (var y = 0u; y < 4u; y++) { for (var x = 0u; x < 4u; x++) {
            let offset = (vec2(f32(x), f32(y)) + .5) / 4. - .5;
            sample += canvas_linear(textureSampleLevel(navigator, linear_sampler, uv + offset * footprint, 0.)) / 16.;
        }}
    }
    let paint = proof_artwork(sample, point);
    var rgb = view_working_rgb(paint.rgb) + view_ui_rgb(v.background_scale.rgb) * (1.-paint.a);
    let edge = min(min(segment_distance(p,v.ab.xy,v.ab.zw),segment_distance(p,v.ab.zw,v.cd.xy)),
                   min(segment_distance(p,v.cd.xy,v.cd.zw),segment_distance(p,v.cd.zw,v.ab.xy)));
    let scale = v.background_scale.w;
    rgb = mix(rgb,vec3(v.halo),clamp(1.5*scale+.5-edge,0.,1.));
    rgb = mix(rgb,view_ui_rgb(v.outline.rgb),clamp(.75*scale+.5-edge,0.,1.));
    if camera.viewport.z > .5 { rgb = display_color(rgb); }
    let opacity = v.outline.a;
    // Keep destination window alpha; only mix color inside its coverage.
    return view_store(vec4(rgb * opacity * window_coverage(p),opacity));
}

struct PickerVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) geometry: vec4<f32>,
    @location(2) @interpolate(flat) sample: vec4<f32>,
    @location(3) @interpolate(flat) original: vec4<f32>,
    @location(4) @interpolate(flat) candidate: vec4<f32>,
};
@vertex fn picker_vertex(@builtin(vertex_index) index: u32,
    @location(0) geometry: vec4<f32>, @location(1) sample: vec4<f32>,
    @location(2) original: vec4<f32>, @location(3) candidate: vec4<f32>) -> PickerVertex {
    let corners = array(vec2(-1.,-1.),vec2(1.,-1.),vec2(-1.,1.),vec2(-1.,1.),vec2(1.,-1.),vec2(1.,1.));
    let local = corners[index] * 48.;
    return PickerVertex(surface_clip(geometry.xy + local * geometry.z), local, geometry, sample, original, candidate);
}
// A small stacked-layer glyph accompanies the aim mark for raw-layer sampling.
fn picker_layer_mark(p: vec2<f32>, center: vec2<f32>) -> f32 {
    let q = p - center;
    let top = min(segment_distance(q,vec2(-3.5,0.),vec2(0.,-2.)),segment_distance(q,vec2(0.,-2.),vec2(3.5,0.)));
    let bottom = min(segment_distance(q,vec2(-3.5,0.),vec2(0.,2.)),segment_distance(q,vec2(0.,2.),vec2(3.5,0.)));
    let back = min(segment_distance(q,vec2(-3.5,2.8),vec2(0.,4.8)),segment_distance(q,vec2(0.,4.8),vec2(3.5,2.8)));
    return min(min(top,bottom),back)-.32;
}
@fragment fn picker_fragment(v: PickerVertex) -> @location(0) vec4<f32> {
    let p = v.local;
    let aa = 1. / v.geometry.z;
    if v.geometry.w > .5 {
        // Pipette tip is exactly at the sampled point, with contrasting outlines.
        let q = vec2((p.x-p.y)*.70710678, (p.x+p.y)*.70710678);
        let barrel = max(abs(q.x-10.)-6., abs(q.y)-2.5);
        let cap = max(abs(q.x-18.)-2., abs(q.y)-5.);
        let tip = segment_distance(p,vec2(0.),vec2(4.,-4.))-.65;
        var d = min(min(abs(barrel)-.65,cap), tip);
        if v.sample.z > .5 { d = min(d,picker_layer_mark(p,vec2(9.,8.))); }
        let coverage = clamp((1.2+aa-d)/aa,0.,1.);
        let ink = clamp((aa*.5-d)/aa,0.,1.);
        return view_store(vec4(vec3(coverage-ink),coverage));
    }
    let radius = length(p);
    let coverage = clamp((46.+aa*.5-radius)/aa,0.,1.);
    if coverage <= 0. { discard; }
    let sample_surface = v.sample.xy + p * v.geometry.z * .5;
    let point = vec2(dot(camera.inverse.xz,sample_surface),dot(camera.inverse.yw,sample_surface)) + camera.offset_document.xy;
    var rgb = view_ui_rgb(camera.surround.rgb);
    if v.sample.w > .5 {
        let checker = select(.80,.94,(i32(floor(p.x/8.))+i32(floor(p.y/8.)))%2==0);
        rgb = select(vec3(checker),view_working_rgb(v.candidate.rgb),v.candidate.a>0.);
    } else if all(point >= vec2(0.)) && all(point < camera.offset_document.zw) {
        let paint = proof_artwork(canvas_linear(artwork_at(point,camera.inverse.xy*.5,camera.inverse.zw*.5)),point);
        let checker = select(.80,.94,(i32(floor(point.x/16.))+i32(floor(point.y/16.)))%2==0);
        rgb = view_working_rgb(paint.rgb) + vec3(checker)*(1.-paint.a);
    }
    var ring = view_working_rgb(select(v.candidate.rgb,v.original.rgb,p.y>=0.));
    let light = clamp(-p.y/46.*.5+.5,0.,1.);
    // Hairline edge reflections stay inside the glass, with no outer stroke or shadow.
    let outer_light = exp(-pow((radius-45.55)/.38,2.));
    let outer_dark = exp(-pow((radius-45.95)/.22,2.));
    let inner_light = exp(-pow((radius-32.25)/.42,2.));
    ring = mix(ring,vec3(1.),outer_light*(.08+.12*light));
    ring = mix(ring,vec3(.04),outer_dark*.12);
    ring = mix(ring,vec3(1.-light),inner_light*.12);
    // One physical pixel of analytic coverage also smooths the interior circle.
    rgb = mix(rgb,ring,clamp((radius-32.+aa*.5)/aa,0.,1.));
    let cross = min(segment_distance(p,vec2(-2.8,0.),vec2(2.8,0.)),
                    segment_distance(p,vec2(0.,-2.8),vec2(0.,2.8)))-.32;
    var mark = cross;
    if v.sample.z > .5 { mark = min(mark,picker_layer_mark(p,vec2(9.,-8.))); }
    // A fine white keyline, rather than a heavy halo, separates the aim from artwork.
    rgb = mix(rgb,vec3(1.),clamp((.35+aa*.5-mark)/aa,0.,1.));
    rgb = mix(rgb,vec3(.035),clamp((aa*.5-mark)/aa,0.,1.));
    if camera.viewport.z > .5 { rgb = display_color(rgb); }
    return view_store(vec4(rgb*coverage,coverage));
}
