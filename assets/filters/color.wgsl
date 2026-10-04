// ABI 5 library: premultiplied input/output, linear unless FX_ENCODED (a
// filter that follows the document's Blending in a Perceptual document),
// document-pixel position, vec4 parameter base. fx_parameter/fx_lut are
// supplied by the host wrapper.
fn fx_encode(c: vec3<f32>) -> vec3<f32> {
    if FX_EXTENDED {return sdr_encode(c,FX_SPACE);}
    let v=max(c,vec3<f32>(0.));
    return select(1.055*pow(v,vec3<f32>(1./2.4))-.055,12.92*v,v<=vec3<f32>(.0031308));
}
fn fx_decode(c: vec3<f32>) -> vec3<f32> {
    if FX_EXTENDED {return sdr_decode(c,FX_SPACE);}
    let v=clamp(c,vec3<f32>(0.),vec3<f32>(1.));
    return select(pow((v+.055)/1.055,vec3<f32>(2.4)),v/12.92,v<=vec3<f32>(.04045));
}
fn fx_rgb(c:vec4<f32>) -> vec3<f32> {
    if FX_ENCODED {return fx_unassociate(c);}
    return fx_encode(fx_unassociate(c));
}
fn fx_rgba(c:vec3<f32>,a:f32) -> vec4<f32> {
    if FX_ENCODED {return vec4<f32>(c*a,a);}
    return vec4<f32>(fx_decode(c)*a,a);
}
fn fx_luma(c:vec3<f32>) -> f32 { if FX_EXTENDED {return dot(c,FX_LUMA);} return dot(c,vec3<f32>(.2126,.7152,.0722)); }
fn fx_preserve_luma(c:vec3<f32>,l:f32) -> vec3<f32> {
    if FX_EXTENDED {return c+(l-fx_luma(c));}
    var v=c+l-fx_luma(c); let low=min(v.r,min(v.g,v.b));
    if low<0. { v=vec3<f32>(l)+(v-l)*l/max(l-low,.000001); }
    let high=max(v.r,max(v.g,v.b));
    if high>1. { v=vec3<f32>(l)+(v-l)*(1.-l)/max(high-l,.000001); }
    return v;
}
