struct Settings {
    rect: vec4<f32>,
    extent: vec4<f32>,
    options: vec4<f32>,
    color: vec4<f32>,
}
@group(0) @binding(0) var<uniform> settings: Settings;
@group(1) @binding(0) var front: texture_2d<f32>;
@group(1) @binding(1) var back: texture_2d<f32>;
@group(1) @binding(2) var sampling: sampler;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vertex_main(@builtin(vertex_index) i: u32) -> Vertex {
    var corners = array<vec2<f32>,3>(vec2<f32>(0.,0.),vec2<f32>(2.,0.),vec2<f32>(0.,2.));
    let uv = corners[i];
    let p = settings.rect.xy + uv*settings.rect.zw;
    var o: Vertex;
    o.position = vec4<f32>(p.x/settings.extent.x*2.-1.,1.-p.y/settings.extent.y*2.,0.,1.);
    o.uv = uv; return o;
}
fn luminance(c: vec3<f32>) -> f32 { return dot(c,vec3<f32>(.3,.59,.11)); }
fn set_luminance(c: vec3<f32>, l: f32) -> vec3<f32> {
    var r = c + l - luminance(c);
    let low = min(r.r,min(r.g,r.b)); let high = max(r.r,max(r.g,r.b));
    if low < 0. { r = vec3<f32>(l)+(r-l)*l/max(l-low,.000001); }
    if high > 1. { r = vec3<f32>(l)+(r-l)*(1.-l)/max(high-l,.000001); }
    return r;
}
fn blend(s: vec3<f32>, d: vec3<f32>, mode: u32) -> vec3<f32> {
    switch mode {
        case 1u: { return s*d; }
        case 2u: { return s+d-s*d; }
        case 3u: { return min(s+d,vec3<f32>(1.)); }
        case 4u: { return select(2.*s*d,1.-2.*(1.-s)*(1.-d),d>vec3<f32>(.5)); }
        case 5u: {
            let curve = select(((16.*d-12.)*d+4.)*d,sqrt(d),d>vec3<f32>(.25));
            return select(d-(1.-2.*s)*d*(1.-d),d+(2.*s-1.)*(curve-d),s>vec3<f32>(.5));
        }
        case 6u: { return set_luminance(s,luminance(d)); }
        default: { return s; }
    }
}
@fragment fn fragment_main(v: Vertex) -> @location(0) vec4<f32> {
    if any(v.uv<vec2<f32>(0.)) || any(v.uv>vec2<f32>(1.)) { discard; }
    let op = u32(settings.options.x);
    if op == 0u { return settings.color; }
    let raw = textureSample(front,sampling,v.uv);
    if op == 8u { return vec4<f32>(raw.rgb * raw.a, raw.a); }
    if op == 6u { let a = raw.r*settings.color.a; return vec4<f32>(settings.color.rgb*a,a); }
    if op == 1u { return raw*settings.options.y; }
    if op == 2u { let m = mix(raw.r,1.-raw.r,settings.options.z); return vec4<f32>(m,m,m,1.); }
    let dst = textureSample(back,sampling,v.uv);
    if op == 7u {
        let m = select(settings.options.z, mix(dst.r,1.-dst.r,settings.options.w-2.), settings.options.w>=2.);
        return raw * m * settings.options.y;
    }
    if op == 3u { return raw*dst.r; }
    if op == 5u { let a = (1.-raw.r)*.42; return vec4<f32>(.46,.12,.8,1.)*a; }
    let src = raw*settings.options.y;
    let s = src.rgb/max(src.a,.000001); let d = dst.rgb/max(dst.a,.000001);
    let b = blend(s,d,u32(settings.options.z));
    if settings.options.w > .5 {
        return vec4<f32>(mix(dst.rgb,b*dst.a,src.a),dst.a);
    }
    let rgb = (1.-src.a)*dst.rgb + (1.-dst.a)*src.rgb + src.a*dst.a*b;
    return vec4<f32>(rgb,src.a+dst.a*(1.-src.a));
}
