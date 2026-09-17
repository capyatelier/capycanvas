// w>1 requires a host-negotiated linear HDR surface and compositor headroom.
@group(0) @binding(9) var<uniform> hdr_view: vec4<f32>;
fn hdr_artwork(paint:vec4<f32>)->vec4<f32> {
    if hdr_view.w<=1. {return hdr_map_sdr(paint,hdr_view);}
    if paint.a<=0. {return paint;}
    let rgb=max(paint.rgb/paint.a,vec3(0.));
    let peak=max(rgb.r,max(rgb.g,rgb.b));
    if peak==0. {return vec4(vec3(0.),paint.a);}
    let knee=hdr_view.w*0.75;
    var mapped=peak;
    if peak>knee {mapped=hdr_view.w-(hdr_view.w-knee)*(hdr_view.w-knee)/(peak+hdr_view.w-2.*knee);}
    return vec4(rgb/peak*mapped*paint.a,paint.a);
}
