// Float32 counterpart of layer_core::color::hdr::sdr. Photographic: luminance tone + gamut compression; legacy BT.2390/RWTMO retained.
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
fn hdr_tone_sdr(paint:vec4<f32>,options:vec4<f32>,highlight_color:f32)->vec4<f32> {
    if options.w==0. || paint.a<=0. {return paint;}
    let rgb=paint.rgb/paint.a;
    let rec=hdr_to_rec2020(rgb);
    let peak=max(0.,max(rec.r,max(rec.g,rec.b)));
    if peak<=0. {return vec4(vec3(0.),paint.a);}
    if options.w==5. {
        let y=dot(rec,vec3(0.2627002,0.6779981,0.0593017));
        if y<=0. {return vec4(vec3(0.),paint.a);}
        let bright=hdr_bt2390(hdr_sdr_adjust(y,options),options.z);
        let colorful=hdr_bt2390(hdr_sdr_adjust(peak,options),options.z)*min(y/peak,1.);
        let mapped=bright+highlight_color*(colorful-bright);
        return vec4(rgb/y*mapped*paint.a,paint.a);
    }
    let x=hdr_sdr_adjust(peak,options);
    var mapped=x;
    if options.w==1. {mapped=hdr_rwtmo(x,options.z);}
    if options.w==2. {mapped=x/exp2(options.z);}
    if options.w==4. {mapped=hdr_bt2390(x,options.z);}
    return vec4(rgb/peak*mapped*paint.a,paint.a);
}
fn hdr_map_sdr(paint:vec4<f32>,options:vec4<f32>,highlight_color:f32)->vec4<f32> {
    let p=hdr_tone_sdr(paint,options,highlight_color);
    if options.w==0. || p.a<=0. {return p;}
    var rgb=hdr_to_output(p.rgb/p.a);
    if options.w==5. {rgb=hdr_compress_gamut(rgb,HDR_OUTPUT_LUMA);}
    else {rgb=clamp(rgb,vec3(0.),vec3(1.));}
    return vec4(hdr_from_output(rgb)*p.a,p.a);
}
// Print LUTs have a bounded working-RGB input. ICC delivery uses the same
// preparation before the profile transform, including Photographic gamut mapping.
fn hdr_map_proof(paint:vec4<f32>,options:vec4<f32>,highlight_color:f32)->vec4<f32> {
    let p=hdr_tone_sdr(paint,options,highlight_color);
    if options.w==0. || p.a<=0. {return p;}
    if options.w==5. {return vec4(hdr_compress_gamut(p.rgb/p.a,HDR_WORKING_LUMA)*p.a,p.a);}
    return vec4(clamp(p.rgb/p.a,vec3(0.),vec3(1.))*p.a,p.a);
}

fn hdr_sdr_adjust(v:f32,options:vec4<f32>)->f32 {
    if options.y==1. {return v*exp2(options.x);}
    return .18*exp2(clamp(options.y*log2(v/.18)+options.x,-126.,120.));
}
fn hdr_compress_gamut(rgb:vec3<f32>,weights:vec3<f32>)->vec3<f32> {
    let y=dot(rgb,weights);
    if y<=0. {return vec3(0.);}
    if y>=1. {return vec3(1.);}
    let chroma=rgb-vec3(y);
    let e=select(-chroma/y,chroma/(1.-y),chroma>vec3(0.));
    let extent=max(e.r,max(e.g,e.b));
    if extent<=.98 {return rgb;}
    let compressed=.98+.02*(extent-.98)/(extent-.96);
    return clamp(vec3(y)+chroma*(compressed/extent),vec3(0.),vec3(1.));
}
