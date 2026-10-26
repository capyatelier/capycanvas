struct Pass {
    inverse_target: vec2<f32>,
    half_texel: vec2<f32>,
    offset: f32,
    surface_uv: vec2<f32>,
    reprojection: mat3x2<f32>,
};

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var linear: sampler;
@group(0) @binding(2) var<uniform> pass_data: Pass;

@vertex
fn fullscreen(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv * vec2<f32>(2., -2.) + vec2<f32>(-1., 1.), 0., 1.);
}

fn tap(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(source, linear, uv, 0.);
}

fn upsample(uv: vec2<f32>) -> vec4<f32> {
    let h = pass_data.half_texel * pass_data.offset;
    var sum = tap(uv + vec2<f32>(-2. * h.x, 0.));
    sum += tap(uv + vec2<f32>(-h.x, h.y)) * 2.;
    sum += tap(uv + vec2<f32>(0., 2. * h.y));
    sum += tap(uv + vec2<f32>(h.x, h.y)) * 2.;
    sum += tap(uv + vec2<f32>(2. * h.x, 0.));
    sum += tap(uv + vec2<f32>(h.x, -h.y)) * 2.;
    sum += tap(uv + vec2<f32>(0., -2. * h.y));
    sum += tap(uv + vec2<f32>(-h.x, -h.y)) * 2.;
    return sum / 12.;
}

@fragment
fn down(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = position.xy * pass_data.inverse_target;
    let h = pass_data.half_texel * pass_data.offset;
    var sum = tap(uv) * 4.;
    sum += tap(uv - h);
    sum += tap(uv + h);
    sum += tap(uv + vec2<f32>(h.x, -h.y));
    sum += tap(uv - vec2<f32>(h.x, -h.y));
    return sum / 8.;
}

@fragment
fn up(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return upsample(position.xy * pass_data.inverse_target);
}

@fragment
fn fill(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let blurred = tap(pass_data.reprojection * vec3<f32>(position.xy, 1.) * pass_data.surface_uv);
    return vec4<f32>(blurred.rgb / max(blurred.a, 1e-4), 1.);
}

struct Region {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) bounds: vec4<f32>,
    @location(1) @interpolate(flat) radii: vec4<f32>,
    @location(2) @interpolate(flat) shape: vec4<f32>,
};

@vertex
fn region_vertex(
    @builtin(vertex_index) index: u32,
    @location(0) bounds: vec4<f32>,
    @location(1) radii: vec4<f32>,
    @location(2) shape: vec4<f32>,
    @location(3) area: vec4<f32>,
) -> Region {
    let quad = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u);
    let c = vec2<f32>(f32(quad[index] & 1u), f32((quad[index] >> 1u) & 1u));
    let pixel = area.xy + c * area.zw;
    let ndc = pixel * pass_data.inverse_target * vec2<f32>(2., -2.) + vec2<f32>(-1., 1.);
    var out: Region;
    out.position = vec4<f32>(ndc, 0., 1.);
    out.bounds = bounds;
    out.radii = radii;
    out.shape = shape;
    return out;
}

fn superellipse(v: vec2<f32>, n: f32) -> f32 {
    let a = abs(v);
    return pow(pow(a.x, n) + pow(a.y, n), 1. / n);
}

fn convex_coverage(p: vec2<f32>, bounds: vec4<f32>, radii: vec4<f32>, n: f32) -> f32 {
    let half = bounds.zw * .5;
    let q = p - (bounds.xy + half);
    let r = max(select(select(radii.w, radii.z, q.x > 0.), select(radii.x, radii.y, q.x > 0.), q.y < 0.), 0.);
    let d = abs(q) - half + vec2<f32>(r);
    var distance = max(d.x, d.y) - r;
    if d.x > 0. && d.y > 0. {
        distance = select(length(d), (superellipse(d / r, n) - 1.) * r, r > 0.);
    }
    return clamp(.5 - distance, 0., 1.);
}

fn concave_coverage(p: vec2<f32>, corner: vec2<f32>, radius: f32, n: f32) -> f32 {
    if radius >= 0. {
        return 1.;
    }
    let r = -radius;
    return clamp(.5 + (superellipse((p - corner) / r, n) - 1.) * r, 0., 1.);
}

fn arc_coverage(p: vec2<f32>, arc: vec4<f32>, shape: vec4<f32>) -> f32 {
    let v = p - arc.xy;
    let turn = 6.2831853;
    let offset = atan2(v.y, v.x) - shape.z;
    var distance = abs(length(v) - arc.z);
    if offset - floor(offset / turn) * turn > shape.w {
        let ends = vec4<f32>(cos(shape.z), sin(shape.z), cos(shape.z + shape.w), sin(shape.z + shape.w)) * arc.z;
        distance = min(length(v - ends.xy), length(v - ends.zw));
    }
    return clamp(.5 - distance + arc.w, 0., 1.);
}

fn color_bubble_coverage(p: vec2<f32>, bounds: vec4<f32>, radii: vec4<f32>, axis: vec2<f32>) -> f32 {
    let center = bounds.xy + bounds.zw * .5 - axis * ((radii.y + radii.z - radii.x) * .5);
    let v = p - center;
    let q = vec2<f32>(dot(v, vec2<f32>(axis.y, -axis.x)), dot(v, axis));
    let angle = clamp(atan2(max(q.y, 0.), q.x), radii.w, 3.14159265 - radii.w);
    let arc_distance = length(q - vec2<f32>(cos(angle), sin(angle)) * radii.y) - radii.z;
    let distance = min(arc_distance, length(q) - radii.x);
    return clamp(.5 - distance, 0., 1.);
}

@fragment
fn region_fragment(in: Region) -> @location(0) vec4<f32> {
    let p = in.position.xy;
    let b = in.bounds;
    let n = in.shape.x;
    var coverage: f32;
    if n <= -2. {
        let points=array<vec2<f32>,3>(in.radii.xy,in.radii.zw,in.shape.zw);
        let flags=u32(round((-n-2.)*8.));
        coverage=1.;
        for (var i=0u;i<3u;i++) {
            let a=points[i];let edge=points[(i+1u)%3u]-a;let v=p-a;
            let distance=(edge.x*v.y-edge.y*v.x)/max(length(edge),0.0001);
            if (flags&(1u<<i))==0u {if distance< -0.0001 {coverage=0.;}}
            else {
                let along=clamp(dot(v,edge)/max(dot(edge,edge),0.00000001),0.,1.);
                let boundary=length(v-along*edge)*select(-1.,1.,distance>=0.);
                coverage=min(coverage,clamp(.5+boundary,0.,1.));
            }
        }
    } else if n == -1. {
        coverage = color_bubble_coverage(p, b, in.radii, in.shape.zw);
    } else if n == 0. {
        coverage = arc_coverage(p, in.radii, in.shape);
    } else {
        coverage = convex_coverage(p, b, in.radii, n);
        coverage *= concave_coverage(p, b.xy, in.radii.x, n);
        coverage *= concave_coverage(p, b.xy + vec2<f32>(b.z, 0.), in.radii.y, n);
        coverage *= concave_coverage(p, b.xy + b.zw, in.radii.z, n);
        coverage *= concave_coverage(p, b.xy + vec2<f32>(0., b.w), in.radii.w, n);
    }
    let blurred = tap(pass_data.reprojection * vec3<f32>(p, 1.) * pass_data.surface_uv);
    let color = blurred.rgb / max(blurred.a, 1e-4);
    return vec4<f32>(color, 1.) * coverage * in.shape.y;
}
