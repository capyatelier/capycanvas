// Separable and luminosity layer blend modes on straight colors, by
// LayerBlend discriminant.
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
            let curve = select(((16.*d-12.)*d+4.)*d,sqrt(max(d,vec3<f32>(0.))),d>vec3<f32>(.25));
            return select(d-(1.-2.*s)*d*(1.-d),d+(2.*s-1.)*(curve-d),s>vec3<f32>(.5));
        }
        case 6u: { return set_luminance(s,luminance(d)); }
        default: { return s; }
    }
}
