// Float32 counterpart of layer_core::color::hdr::sdr. Default: BT.2390; browser option: Skia RWTMO.
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
// ITU-R BT.2390 (2016), section 5.4. Matches the shared Float32 mapper.
fn hdr_sdr_pq_encode(nits:f32)->f32 {
    let p=pow(nits/10000.,2610./16384.);
    return pow((3424./4096.+2413./128.*p)/(1.+2392./128.*p),2523./32.);
}
fn hdr_sdr_pq_decode(code:f32)->f32 {
    let p=pow(code,32./2523.);
    return 10000.*pow(max(p-3424./4096.,0.)/(2413./128.-2392./128.*p),16384./2610.);
}
fn hdr_bt2390(x:f32,headroom:f32)->f32 {
    if x<=0. {return 0.;}
    let peak=exp2(headroom);
    if peak==1. || x>=peak {return min(x,1.);}
    let pq_peak=hdr_sdr_pq_encode(peak*203.);
    let output=hdr_sdr_pq_encode(203.)/pq_peak;
    let knee=max(2.*output-1.,0.);
    let q=hdr_sdr_pq_encode(x*203.)/pq_peak;
    if q<=knee {return x;}
    let t=(q-knee)/(1.-knee);
    let t2=t*t;
    let t3=t2*t;
    let mapped=(2.*t3-3.*t2+1.)*knee+(t3-2.*t2+t)*(1.-knee)+(-2.*t3+3.*t2)*output;
    return clamp(hdr_sdr_pq_decode(mapped*pq_peak)/203.,0.,1.);
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
    if options.w==4. {mapped=hdr_bt2390(x,options.z);}
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
