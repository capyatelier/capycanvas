// ABI 2 library: premultiplied linear input/output, document-pixel position,
// vec4 parameter base. fx_parameter/fx_lut are supplied by the host wrapper.
fn fx_encode(c: vec3<f32>) -> vec3<f32> {
    let v=max(c,vec3<f32>(0.));
    return select(1.055*pow(v,vec3<f32>(1./2.4))-.055,12.92*v,v<=vec3<f32>(.0031308));
}
fn fx_decode(c: vec3<f32>) -> vec3<f32> {
    let v=clamp(c,vec3<f32>(0.),vec3<f32>(1.));
    return select(pow((v+.055)/1.055,vec3<f32>(2.4)),v/12.92,v<=vec3<f32>(.04045));
}
fn fx_rgb(c:vec4<f32>) -> vec3<f32> { return fx_encode(c.rgb/max(c.a,.000001)); }
fn fx_rgba(c:vec3<f32>,a:f32) -> vec4<f32> { return vec4<f32>(fx_decode(c)*a,a); }
fn fx_luma(c:vec3<f32>) -> f32 { return dot(c,vec3<f32>(.2126,.7152,.0722)); }
fn fx_preserve_luma(c:vec3<f32>,l:f32) -> vec3<f32> {
    var v=c+l-fx_luma(c); let low=min(v.r,min(v.g,v.b));
    if low<0. { v=vec3<f32>(l)+(v-l)*l/max(l-low,.000001); }
    let high=max(v.r,max(v.g,v.b));
    if high>1. { v=vec3<f32>(l)+(v-l)*(1.-l)/max(high-l,.000001); }
    return v;
}
fn capy_curves(c:vec4<f32>,position:vec2<f32>,base:u32) -> vec4<f32> {
    let rgb=fx_rgb(c);
    let channel=vec3<f32>(fx_lut(base,256u,rgb.r).r,fx_lut(base,512u,rgb.g).r,fx_lut(base,768u,rgb.b).r);
    return fx_rgba(vec3<f32>(fx_lut(base,0u,channel.r).r,fx_lut(base,0u,channel.g).r,fx_lut(base,0u,channel.b).r),c.a);
}
fn capy_levels(c:vec4<f32>,position:vec2<f32>,base:u32) -> vec4<f32> {
    let low=fx_parameter(base,0u).x; let high=fx_parameter(base,1u).x;
    let v=pow(clamp((fx_rgb(c)-low)/max(high-low,.000001),vec3<f32>(0.),vec3<f32>(1.)),vec3<f32>(1./fx_parameter(base,2u).x));
    return fx_rgba(mix(vec3<f32>(fx_parameter(base,3u).x),vec3<f32>(fx_parameter(base,4u).x),v),c.a);
}
fn capy_brightness_contrast(c:vec4<f32>,position:vec2<f32>,base:u32) -> vec4<f32> {
    let v=(fx_rgb(c)-.5)*exp2(fx_parameter(base,1u).x/50.)+.5+fx_parameter(base,0u).x/100.;
    return fx_rgba(v,c.a);
}
fn fx_hsl(c:vec3<f32>) -> vec3<f32> {
    let low=min(c.r,min(c.g,c.b)); let high=max(c.r,max(c.g,c.b)); let d=high-low; let l=(high+low)*.5;
    if d<.000001 { return vec3<f32>(0.,0.,l); }
    var h=(c.g-c.b)/d;
    if high==c.g { h=(c.b-c.r)/d+2.; } else if high==c.b { h=(c.r-c.g)/d+4.; }
    return vec3<f32>(fract(h/6.+1.),d/max(1.-abs(2.*l-1.),.000001),l);
}
fn fx_hsl_rgb(hsl:vec3<f32>) -> vec3<f32> {
    let hue=clamp(abs(fract(hsl.x+vec3<f32>(0.,2./3.,1./3.))*6.-3.)-1.,vec3<f32>(0.),vec3<f32>(1.));
    return hsl.z+(hue-.5)*(1.-abs(2.*hsl.z-1.))*hsl.y;
}
fn capy_hue_saturation(c:vec4<f32>,position:vec2<f32>,base:u32) -> vec4<f32> {
    var hsl=fx_hsl(fx_rgb(c));
    hsl.x=fract(hsl.x+fx_parameter(base,0u).x/360.+1.);
    let s=fx_parameter(base,1u).x/100.; hsl.y=clamp(hsl.y*(1.+s),0.,1.);
    let l=fx_parameter(base,2u).x/100.; hsl.z=select(hsl.z*(1.+l),mix(hsl.z,1.,l),l>=0.);
    return fx_rgba(fx_hsl_rgb(hsl),c.a);
}
fn capy_color_balance(c:vec4<f32>,position:vec2<f32>,base:u32) -> vec4<f32> {
    let rgb=fx_rgb(c); let l=fx_luma(rgb);
    let shadow=1.-smoothstep(0.,.5,l); let high=smoothstep(.5,1.,l); let mid=1.-shadow-high;
    let s=vec3<f32>(fx_parameter(base,0u).x,fx_parameter(base,1u).x,fx_parameter(base,2u).x);
    let m=vec3<f32>(fx_parameter(base,3u).x,fx_parameter(base,4u).x,fx_parameter(base,5u).x);
    let h=vec3<f32>(fx_parameter(base,6u).x,fx_parameter(base,7u).x,fx_parameter(base,8u).x);
    var out=rgb+(s*shadow+m*mid+h*high)/200.;
    if fx_parameter(base,9u).x>.5 { out=fx_preserve_luma(out,l); }
    return fx_rgba(out,c.a);
}
// Exposure is scene-linear; unlike artistic tone controls it must not operate
// on encoded sRGB. +1 EV doubles linear radiance, not the byte value.
fn capy_exposure(c:vec4<f32>,position:vec2<f32>,base:u32) -> vec4<f32> {
    let linear=c.rgb/max(c.a,.000001)*exp2(fx_parameter(base,0u).x)+fx_parameter(base,1u).x;
    let out=pow(clamp(linear,vec3<f32>(0.),vec3<f32>(1.)),vec3<f32>(1./fx_parameter(base,2u).x));
    return vec4<f32>(out*c.a,c.a);
}
fn capy_vibrance(c:vec4<f32>,position:vec2<f32>,base:u32) -> vec4<f32> {
    var hsl=fx_hsl(fx_rgb(c));
    var amount=fx_parameter(base,0u).x/100.;
    if amount>0. && fx_parameter(base,2u).x>.5 {
        let hue_distance=min(abs(hsl.x-.08),1.-abs(hsl.x-.08));
        amount*=mix(.35,1.,smoothstep(.025,.14,hue_distance));
    }
    hsl.y=clamp(hsl.y*(1.+amount*(1.-hsl.y))*(1.+fx_parameter(base,1u).x/100.),0.,1.);
    return fx_rgba(fx_hsl_rgb(hsl),c.a);
}
fn capy_black_white(c:vec4<f32>,position:vec2<f32>,base:u32) -> vec4<f32> {
    let rgb=fx_rgb(c); let hsl=fx_hsl(rgb); let hue=hsl.x*6.; let sector=u32(floor(hue))%6u;
    let weight=mix(fx_parameter(base,sector).x,fx_parameter(base,(sector+1u)%6u).x,fract(hue))/100.;
    let low=min(rgb.r,min(rgb.g,rgb.b)); let high=max(rgb.r,max(rgb.g,rgb.b));
    let value=clamp(low+(high-low)*weight,0.,1.); var out=vec3<f32>(value);
    if fx_parameter(base,6u).x>.5 {
        let tint=fx_hsl(fx_parameter(base,7u).rgb);out=fx_hsl_rgb(vec3<f32>(tint.xy,value));
    }
    return fx_rgba(out,c.a);
}
fn capy_gradient_map(c:vec4<f32>,position:vec2<f32>,base:u32) -> vec4<f32> {
    let rgb=fx_rgb(c);let l=fx_luma(rgb);
    let x=select(l,1.-l,fx_parameter(base,256u).x>.5);
    let mapped=fx_lut(base,0u,x);
    return fx_rgba(mix(rgb,mapped.rgb,mapped.a*fx_parameter(base,257u).x/100.),c.a);
}
fn capy_posterize(c:vec4<f32>,position:vec2<f32>,base:u32) -> vec4<f32> {
    let levels=max(2.,round(fx_parameter(base,0u).x))-1.;
    return fx_rgba(round(fx_rgb(c)*levels)/levels,c.a);
}
