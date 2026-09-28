// Layer blend modes on straight colors. A blend code holds the LayerBlend
// discriminant in bits 0-7, the Perceptual blend space in bit 8 and float
// documents in bit 9. Float documents clamp no result; Unit modes clamp their
// operands to [0, 1].
const BLEND_PERCEPTUAL: u32 = 256u;
const BLEND_FLOAT: u32 = 512u;
const BLEND_DIVISOR: f32 = .00006103515625;
fn blend_mode(code: u32) -> u32 { return code & 255u; }
fn blend_luma(code: u32) -> vec3<f32> {
    return select(WORKING_LUMA, vec3<f32>(.3,.59,.11), (code & BLEND_PERCEPTUAL) != 0u);
}
fn blend_set_lum(c: vec3<f32>, l: f32, weights: vec3<f32>, extended: bool) -> vec3<f32> {
    var r = c + l - dot(c,weights);
    let low = min(r.r,min(r.g,r.b)); let high = max(r.r,max(r.g,r.b));
    if low < 0. { r = vec3<f32>(l)+(r-l)*l/max(l-low,.000001); }
    if !extended && high > 1. { r = vec3<f32>(l)+(r-l)*(1.-l)/max(high-l,.000001); }
    return r;
}
fn blend_sat(c: vec3<f32>) -> f32 { return max(c.r,max(c.g,c.b)) - min(c.r,min(c.g,c.b)); }
fn blend_set_sat(c: vec3<f32>, s: f32) -> vec3<f32> {
    let low = min(c.r,min(c.g,c.b)); let high = max(c.r,max(c.g,c.b));
    if high <= low { return vec3<f32>(0.); }
    return (c-low)*s/(high-low);
}
fn blend_hard_light(s: vec3<f32>, d: vec3<f32>, top: vec3<f32>) -> vec3<f32> {
    return select(2.*s*d,1.-2.*(1.-s)*(1.-d),top>vec3<f32>(.5));
}
fn blend_color_burn(s: vec3<f32>, d: vec3<f32>) -> vec3<f32> {
    let burned = 1.-min(vec3<f32>(1.),(1.-d)/max(s,vec3<f32>(BLEND_DIVISOR)));
    return select(select(burned,vec3<f32>(0.),s<=vec3<f32>(0.)),vec3<f32>(1.),d>=vec3<f32>(1.));
}
fn blend_color_dodge(s: vec3<f32>, d: vec3<f32>) -> vec3<f32> {
    let dodged = min(vec3<f32>(1.),d/max(1.-s,vec3<f32>(BLEND_DIVISOR)));
    return select(select(dodged,vec3<f32>(1.),s>=vec3<f32>(1.)),vec3<f32>(0.),d<=vec3<f32>(0.));
}
fn blend(s: vec3<f32>, d: vec3<f32>, code: u32) -> vec3<f32> {
    let extended = (code & BLEND_FLOAT) != 0u;
    let weights = blend_luma(code);
    let zero = vec3<f32>(0.); let one = vec3<f32>(1.);
    let su = clamp(s,zero,one); let du = clamp(d,zero,one);
    switch blend_mode(code) {
        case 1u: { return s*d; }
        case 2u: {
            if extended { return s+d-min(s,one)*min(d,one); }
            return s+d-s*d;
        }
        case 3u: {
            if extended { return s+d; }
            return min(s+d,one);
        }
        case 4u: { return blend_hard_light(su,du,du); }
        case 5u: {
            let curve = select(((16.*du-12.)*du+4.)*du,sqrt(du),du>vec3<f32>(.25));
            return select(du-(1.-2.*su)*du*(1.-du),du+(2.*su-1.)*(curve-du),su>vec3<f32>(.5));
        }
        case 6u: { return blend_set_lum(s,dot(d,weights),weights,extended); }
        case 7u: { return min(s,d); }
        case 8u: { return max(s,d); }
        case 9u: { return blend_color_burn(su,du); }
        case 10u: { return max(s+d-1.,zero); }
        case 11u: { return blend_color_dodge(su,du); }
        case 12u: { return blend_hard_light(su,du,su); }
        case 13u: {
            return select(blend_color_dodge(2.*su-1.,du),blend_color_burn(2.*su,du),su<vec3<f32>(.5));
        }
        case 14u: {
            let r = d+2.*s-1.;
            if extended { return max(r,zero); }
            return clamp(r,zero,one);
        }
        case 15u: {
            let r = select(max(d,2.*s-1.),min(d,2.*s),s<vec3<f32>(.5));
            if extended { return r; }
            return clamp(r,zero,one);
        }
        case 16u: { return select(zero,one,su+du>=one); }
        case 17u: { return abs(d-s); }
        case 18u: { return su+du-2.*su*du; }
        case 19u: { return max(d-s,zero); }
        case 20u: {
            let divided = d/max(s,vec3<f32>(BLEND_DIVISOR));
            if extended { return divided; }
            return select(min(divided,one),select(zero,one,d>zero),s<=zero);
        }
        case 21u: { return blend_set_lum(blend_set_sat(s,blend_sat(d)),dot(d,weights),weights,extended); }
        case 22u: { return blend_set_lum(blend_set_sat(d,blend_sat(s)),dot(d,weights),weights,extended); }
        case 23u: { return blend_set_lum(d,dot(s,weights),weights,extended); }
        default: { return s; }
    }
}
// Premultiplied source over a premultiplied backdrop.
fn blend_composite(src: vec4<f32>, dst: vec4<f32>, code: u32) -> vec4<f32> {
    let b = blend(working_unassociate(src),working_unassociate(dst),code);
    return vec4<f32>((1.-src.a)*dst.rgb+(1.-dst.a)*src.rgb+src.a*dst.a*b,src.a+dst.a*(1.-src.a));
}
// A clipped source blends only where its base has coverage, keeping that alpha.
fn blend_clip(src: vec4<f32>, dst: vec4<f32>, code: u32) -> vec4<f32> {
    let b = blend(working_unassociate(src),working_unassociate(dst),code);
    return vec4<f32>(working_mix(dst.rgb,b*dst.a,src.a),dst.a);
}
