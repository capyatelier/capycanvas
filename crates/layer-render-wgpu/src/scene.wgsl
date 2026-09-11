struct Settings {
    rect: vec4<f32>,
    extent: vec4<f32>,
    options: vec4<f32>,
    color: vec4<f32>,
    source_over: vec4<f32>,
    backdrop: vec4<f32>,
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
// Closest point on an ellipse from the Lagrange multiplier equation:
// sum((axis * point / (axis^2 + t))^2) = 1. Bounded bisection avoids
// eccentricity-dependent convergence and keeps outline width in image pixels.
fn ellipse_distance(point: vec2<f32>, axes: vec2<f32>) -> f32 {
    let swap = axes.y > axes.x;
    let r = select(axes, axes.yx, swap);
    let p = abs(select(point, point.yx, swap));
    let inside = dot(p/r,p/r) < 1.;
    if r.x-r.y < r.x*.000001 { return length(p)-r.x; }
    let r2 = r*r;
    var q: vec2<f32>;
    if p.y <= r.y*.000001 {
        let x = min(r.x,r2.x*p.x/(r2.x-r2.y));
        q = vec2<f32>(x,r.y*sqrt(max(0.,1.-x*x/r2.x)));
    } else {
        var low = r.y*p.y-r2.y;
        var high = 0.;
        if !inside { low = max(0.,low); high = length(r*p); }
        for (var i=0u;i<24u;i++) {
            let t = (low+high)*.5;
            let v = r*p/max(r2+t,vec2<f32>(.000000000001));
            if dot(v,v)>1. { low=t; } else { high=t; }
        }
        q = r2*p/max(r2+(low+high)*.5,vec2<f32>(.000000000001));
    }
    return select(length(p-q),-length(p-q),inside);
}
fn figure_color(p: vec2<f32>) -> vec4<f32> {
    let code = u32(settings.options.y)%16u;
    let shape = code%3u;
    let paint = code/3u;
    let a = settings.backdrop.xy;
    let b = settings.backdrop.zw;
    let half_width = settings.options.z*.5;
    var d: f32;
    if shape==0u {
        let delta=b-a;
        let t=clamp(dot(p-a,delta)/max(dot(delta,delta),.000001),0.,1.);
        d=length(p-a-delta*t)-half_width;
    } else {
        let radius=abs(b-a)*.5;
        let local=p-(a+b)*.5;
        if shape==1u {
            let q=abs(local)-radius;
            d=length(max(q,vec2<f32>(0.)))+min(max(q.x,q.y),0.);
        } else { d=ellipse_distance(local,radius); }
    }
    let foreground=vec4<f32>(settings.color.rgb*settings.color.a,settings.color.a);
    if shape==0u { return foreground*clamp(.5-d,0.,min(1.,settings.options.z)); }
    if paint==1u { return foreground*clamp(.5-d,0.,1.); }
    let outer=clamp(.5+half_width-d,0.,1.);
    let inner=clamp(.5-half_width-d,0.,1.);
    let outline=outer-inner;
    if paint==0u { return foreground*outline; }
    let interior=inner;
    let background=vec4<f32>(settings.source_over.rgb*settings.source_over.a,settings.source_over.a);
    return foreground*outline+background*interior;
}
@fragment fn fragment_main(v: Vertex) -> @location(0) vec4<f32> {
    if any(v.uv<vec2<f32>(0.)) || any(v.uv>vec2<f32>(1.)) { discard; }
    let op = u32(settings.options.x);
    if op == 0u { return settings.color; }
    if op == 9u {
        let p=v.uv;
        let bands=.5+.5*cos(vec3<f32>(0.,2.,4.)+p.x*8.);
        let light=.25+.65*p.y;
        let detail=select(.8,1.,(u32(p.x*48.)+u32(p.y*12.))%2u==0u);
        return vec4<f32>(bands*light*detail,1.);
    }
    if op == 10u {
        let p=settings.color.xy+v.uv*settings.color.zw;
        let ink=textureSampleLevel(front,sampling,p/vec2<f32>(textureDimensions(front)),0.);
        let mask=textureSampleLevel(back,sampling,v.uv,0.).a;
        return ink*mask;
    }
    if op == 8u {
        // Image initialization copies source texels 1:1 into paint pages. Decode
        // encoded bytes explicitly before linear storage: hardware sRGB decode
        // precision differs by backend and can change a rounded paint value.
        let encoded = textureLoad(front, vec2<i32>(floor(v.uv * vec2<f32>(textureDimensions(front)))), 0);
        let linear = select(pow((encoded.rgb + .055) / 1.055, vec3<f32>(2.4)), encoded.rgb / 12.92,
            encoded.rgb <= vec3<f32>(.04045));
        return vec4<f32>(linear * encoded.a, encoded.a);
    }
    let raw = textureSample(front,sampling,v.uv);
    if op == 6u || op == 11u {
        // Constant fills are the degenerate case (equal endpoint colors).
        let p = settings.extent.zw + v.uv * settings.rect.zw;
        var src: vec4<f32>;
        if op == 11u { src = figure_color(p)*raw.r; }
        else {
            let delta = settings.backdrop.zw - settings.backdrop.xy;
            let length2 = max(dot(delta, delta), .000001);
            let relative = p - settings.backdrop.xy;
            let t = clamp(select(dot(relative, delta) / length2,
                length(relative) * inverseSqrt(length2), settings.options.z > .5), 0., 1.);
            let first = vec4<f32>(settings.color.rgb * settings.color.a, settings.color.a);
            let last = vec4<f32>(settings.source_over.rgb * settings.source_over.a, settings.source_over.a);
            src = mix(first, last, t) * raw.r;
        }
        let dst = textureSample(back, sampling, v.uv);
        if op==11u && settings.options.y>=16. {
            return select(dst*(1.-src.a),dst,settings.options.w>.5);
        }
        if settings.options.w > .5 {
            return vec4<f32>(src.rgb * dst.a + dst.rgb * (1. - src.a), dst.a);
        }
        return src + dst * (1. - src.a);
    }
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
