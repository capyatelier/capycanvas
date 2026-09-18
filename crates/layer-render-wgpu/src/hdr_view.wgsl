// headroom.x>1 requires a host-negotiated linear HDR surface and compositor headroom.
struct HdrView { rendition: vec4<f32>, headroom: vec4<f32> }
@group(0) @binding(9) var<uniform> hdr_view: HdrView;
fn hdr_artwork(paint:vec4<f32>)->vec4<f32> {
    if hdr_view.headroom.x<=1. {return hdr_map_sdr(paint,hdr_view.rendition,hdr_view.headroom.yz);}
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
