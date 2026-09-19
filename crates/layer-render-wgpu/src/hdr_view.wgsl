// headroom.x>1 requires a host-negotiated linear HDR surface and compositor headroom.
struct HdrView { rendition: vec4<f32>, headroom: vec4<f32> }
@group(0) @binding(9) var<uniform> hdr_view: HdrView;
struct LocalToneGuide { size:vec4<u32>, samples:array<vec4<f32>> }
@group(0) @binding(10) var<storage,read> local_tone:LocalToneGuide;
fn local_tone_artwork(paint:vec4<f32>,position:vec2<f32>)->vec4<f32> {
    if hdr_view.rendition.w==0. || paint.a<=0. {return paint;}
    if local_tone.size.x==0u {return hdr_tone_sdr(paint,hdr_view.rendition,vec4(hdr_view.headroom.yz,0.,0.));}
    let y=dot(paint.rgb/paint.a,HDR_WORKING_LUMA);
    if y<=0. {return paint;}
    let log_y=log2(max(y,0.000000059604645));
    let q=clamp(position*vec2<f32>(local_tone.size.xy)/vec2<f32>(local_tone.size.zw)-.5,vec2(0.),vec2<f32>(local_tone.size.xy-1u));
    let low=vec2<u32>(floor(q));let t=fract(q);
    var total=0.;var value=0.;
    for(var dy=0u;dy<2u;dy++){for(var dx=0u;dx<2u;dx++){
        let xy=min(low+vec2(dx,dy),local_tone.size.xy-1u);
        let p=local_tone.samples[xy.y*local_tone.size.x+xy.x];
        let d=(p.x-log_y)/1.5;
        let weight=select(1.-t.x,t.x,dx==1u)*select(1.-t.y,t.y,dy==1u)*p.z/(1.+d*d*d*d);
        total+=weight;value+=weight*p.y;
    }}
    var base=log_y;if total>1e-12 {base=value/total;}
    return hdr_tone_sdr_base(paint,hdr_view.rendition,vec4(hdr_view.headroom.yz,0.,0.),base);
}
fn hdr_artwork(paint:vec4<f32>)->vec4<f32> {
    if hdr_view.headroom.x<=1. {return hdr_map_sdr(paint,hdr_view.rendition,vec4(hdr_view.headroom.yz,0.,0.));}
    if paint.a<=0. {return paint;}
    // Signed scRGB channels carry wide-gamut colors outside the sRGB cube.
    // Scale them together; clipping negative channels would change chromaticity.
    let rgb=paint.rgb/paint.a;
    let magnitude=abs(rgb);
    let peak=max(magnitude.r,max(magnitude.g,magnitude.b));
    if peak==0. {return vec4(vec3(0.),paint.a);}
    let knee=hdr_view.headroom.x*0.75;
    var mapped=peak;
    if peak>knee {mapped=hdr_view.headroom.x-(hdr_view.headroom.x-knee)*(hdr_view.headroom.x-knee)/(peak+hdr_view.headroom.x-2.*knee);}
    return vec4(rgb/peak*mapped*paint.a,paint.a);
}
