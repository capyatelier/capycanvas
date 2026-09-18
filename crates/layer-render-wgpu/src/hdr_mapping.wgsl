// Float32 counterpart of layer_core::color::hdr. Matrix/luminance constants are
// compiled for the source and output RGB spaces; display state is never artwork.
fn hdr_luma(rgb:vec3<f32>, y:vec3<f32>)->f32 {
    return rgb.g+y.r*(rgb.r-rgb.g)+y.b*(rgb.b-rgb.g);
}
fn hdr_gamut(rgb:vec3<f32>, y:vec3<f32>)->vec3<f32> {
    let light=clamp(hdr_luma(rgb,y),0.,1.);
    if light<=0. || light>=1. {return vec3(light);}
    let distance=select((vec3(light)-rgb)/light,(rgb-vec3(light))/(1.-light),rgb>vec3(light));
    let extent=max(distance.r,max(distance.g,distance.b));
    if extent<=.8 {return rgb;}
    let compressed=1.-.04/(extent-.6);
    return clamp(vec3(light)+(rgb-vec3(light))*(compressed/extent),vec3(0.),vec3(1.));
}
fn hdr_tone_sdr(paint:vec4<f32>,options:vec4<f32>)->vec4<f32> {
    if options.w==0. || paint.a<=0. {return paint;}
    let rgb=paint.rgb/paint.a;
    let light=max(hdr_luma(rgb,HDR_SOURCE_Y),0.);
    if light<=0. {return vec4(vec3(0.),paint.a);}
    let x=.18*exp2(clamp(options.y*(log2(light/.18)+options.x),-126.,120.));
    let highlights=clamp((options.z-.75)/select(.2,.5,options.z<.75),-1.,1.);
    let shape=exp2(highlights*2.);
    var mapped=x;
    if x>.18 {mapped=.18+.82*(1.-pow(1.+(x-.18)/(.82*shape),-shape));}
    return vec4(rgb/light*mapped*paint.a,paint.a);
}
fn hdr_map_sdr(paint:vec4<f32>,options:vec4<f32>)->vec4<f32> {
    let p=hdr_tone_sdr(paint,options);
    if options.w==0. || p.a<=0. {return p;}
    return vec4(hdr_from_output(hdr_gamut(hdr_to_output(p.rgb/p.a),HDR_OUTPUT_Y))*p.a,p.a);
}
// Print LUTs are indexed by bounded working RGB; ICC delivery uses the same
// source-gamut preparation before the profile transform.
fn hdr_map_proof(paint:vec4<f32>,options:vec4<f32>)->vec4<f32> {
    let p=hdr_tone_sdr(paint,options);
    if options.w==0. || p.a<=0. {return p;}
    return vec4(hdr_gamut(p.rgb/p.a,HDR_SOURCE_Y)*p.a,p.a);
}
