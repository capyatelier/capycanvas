// Float32 counterpart of layer_core::color::hdr::sdr. Default: Skia RWTMO.
// Adapted from Skia (Copyright 2025 Google LLC, BSD-3-Clause).
// See THIRD_PARTY_NOTICES.md. Artwork values and alpha remain unchanged.
fn hdr_rwtmo(x:f32, headroom:f32)->f32 {
    let peak=exp2(headroom);
    if peak==1. {return min(x,1.);}
    let white=1.-.5*min(headroom/2.3004484,1.);
    if x<=1. {return x*white;}
    if x>=peak {return 1.;}
    let mid=vec2(.35+.65/white,.35*white+.65);
    let a=vec2(1.,white)-2.*mid+vec2(peak,1.);
    let b=2.*mid-2.*vec2(1.,white);
    // Evaluate the underlying monotonic Bezier, avoiding the browser spline's
    // overshoot at extreme HDR ranges. Same stable quadratic inversion as CPU.
    let t=2.*(x-1.)/(b.x+sqrt(max(b.x*b.x+4.*a.x*(x-1.),0.)));
    return white+t*(b.y+t*a.y);
}
fn hdr_tone_sdr(paint:vec4<f32>,options:vec4<f32>)->vec4<f32> {
    if options.w==0. || paint.a<=0. {return paint;}
    let rgb=paint.rgb/paint.a;
    let rec=hdr_to_rec2020(rgb);
    let peak=max(0.,max(rec.r,max(rec.g,rec.b)));
    if peak<=0. {return vec4(vec3(0.),paint.a);}
    var x=peak*exp2(options.x);
    if options.y!=1. {x=.18*exp2(clamp(options.y*log2(peak/.18)+options.x,-126.,120.));}
    var mapped=x;
    if options.w==1. {mapped=hdr_rwtmo(x,options.z);}
    if options.w==2. {mapped=x/exp2(options.z);}
    return vec4(rgb/peak*mapped*paint.a,paint.a);
}
fn hdr_map_sdr(paint:vec4<f32>,options:vec4<f32>)->vec4<f32> {
    let p=hdr_tone_sdr(paint,options);
    if options.w==0. || p.a<=0. {return p;}
    return vec4(hdr_from_output(clamp(hdr_to_output(p.rgb/p.a),vec3(0.),vec3(1.)))*p.a,p.a);
}
// Print LUTs have a bounded working-RGB input. ICC delivery uses the same
// preparation before the profile transform; no perceptual desaturation stage.
fn hdr_map_proof(paint:vec4<f32>,options:vec4<f32>)->vec4<f32> {
    let p=hdr_tone_sdr(paint,options);
    if options.w==0. || p.a<=0. {return p;}
    return vec4(clamp(p.rgb/p.a,vec3(0.),vec3(1.))*p.a,p.a);
}
