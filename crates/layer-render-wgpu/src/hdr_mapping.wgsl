// Same saved-rendition transform as layer_core::color::hdr::SdrRendition.
// options.w=0 disables mapping; alpha and artwork storage are unchanged.
fn hdr_map_sdr(paint:vec4<f32>,options:vec4<f32>)->vec4<f32> {
    if options.w==0. || paint.a<=0. {return paint;}
    let rgb=max(paint.rgb/paint.a,vec3(0.));
    let peak=max(rgb.r,max(rgb.g,rgb.b));
    if peak==0. {return vec4(vec3(0.),paint.a);}
    let x=0.18*pow(peak*exp2(options.x)/0.18,options.y);
    let knee=options.z;
    var mapped=x;
    if x>knee {mapped=1.-(1.-knee)*(1.-knee)/(x+1.-2.*knee);}
    return vec4(rgb/peak*mapped*paint.a,paint.a);
}
