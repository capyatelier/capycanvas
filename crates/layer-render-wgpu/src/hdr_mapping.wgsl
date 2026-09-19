// Float32 counterpart of the shared fixed-baseline macro/micro SDR mapper.
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
    var knee=max(2.*output-1.,0.);
    let q=hdr_sdr_pq_encode(x*203.)/pq_peak;
    if q<=knee {return x;}
    let t=(q-knee)/(1.-knee);
    let t2=t*t;
    let t3=t2*t;
    let mapped=(2.*t3-3.*t2+1.)*knee+(t3-2.*t2+t)*(1.-knee)+(-2.*t3+3.*t2)*output;
    return clamp(hdr_sdr_pq_decode(mapped*pq_peak)/203.,0.,1.);
}

fn hdr_log_odds(y:f32)->f32 {return log2(y/(1.-y));}
fn hdr_tone_sdr_base(paint:vec4<f32>,options:vec4<f32>,appearance:vec4<f32>,base:f32)->vec4<f32> {
    if options.w==0. || paint.a<=0. {return paint;}
    let rgb=paint.rgb/paint.a;
    let y=dot(rgb,HDR_WORKING_LUMA);
    if y<=0. {return vec4(vec3(0.),paint.a);}
    let log_y=log2(max(y,0.000000059604645));
    let broad=-2.473931+.4*(base+2.473931);
    let headroom=max(options.z*.4-.6*2.473931,0.);
    let baseline=hdr_bt2390(exp2(broad+log_y-base),headroom);
    let gains=options.y*vec2(exp2(-.5*appearance.y),exp2(.5*appearance.y));
    var mapped=baseline;
    if baseline>0. && baseline<1. && (any(gains!=vec2(1.)) || options.x!=0.) {
        let u=hdr_log_odds(baseline);
        let b=hdr_log_odds(clamp(hdr_bt2390(exp2(broad),headroom),0.000000059604645,0.999999940395355));
        let odds=-2.187627+gains.x*(b+2.187627)+gains.y*(u-b)+options.x;
        let v=exp2(clamp(odds,-126.,120.));mapped=v/(1.+v);
    }
    return vec4(rgb/y*mapped*paint.a,paint.a);
}
// Point-color estimate for isolated swatches; artwork supplies the spatial base.
fn hdr_tone_sdr(paint:vec4<f32>,options:vec4<f32>,appearance:vec4<f32>)->vec4<f32> {
    if options.w==0. || paint.a<=0. {return paint;}
    let base=log2(max(dot(paint.rgb/paint.a,HDR_WORKING_LUMA),0.000000059604645));
    return hdr_tone_sdr_base(paint,options,appearance,base);
}
fn hdr_gamut_sdr(p:vec4<f32>,color:f32)->vec4<f32> {
    if p.a<=0. {return p;}
    let rgb=hdr_unified_gamut(hdr_to_output(p.rgb/p.a),HDR_OUTPUT_LUMA,color);
    return vec4(hdr_from_output(rgb)*p.a,p.a);
}
fn hdr_gamut_proof(p:vec4<f32>,color:f32)->vec4<f32> {
    if p.a<=0. {return p;}
    return vec4(hdr_unified_gamut(p.rgb/p.a,HDR_WORKING_LUMA,color)*p.a,p.a);
}
fn hdr_map_sdr(paint:vec4<f32>,options:vec4<f32>,appearance:vec4<f32>)->vec4<f32> {
    if options.w==0. {return paint;}
    return hdr_gamut_sdr(hdr_tone_sdr(paint,options,appearance),appearance.x);
}
fn hdr_map_proof(paint:vec4<f32>,options:vec4<f32>,appearance:vec4<f32>)->vec4<f32> {
    if options.w==0. {return paint;}
    return hdr_gamut_proof(hdr_tone_sdr(paint,options,appearance),appearance.x);
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

fn hdr_unified_gamut(rgb:vec3<f32>,weights:vec3<f32>,color:f32)->vec3<f32> {
    let white=hdr_compress_gamut(rgb,weights);
    if color==0. {return white;}
    let y=dot(rgb,weights);
    if y<=0. {return vec3(0.);}
    let low=min(0.,min(rgb.r,min(rgb.g,rgb.b)));
    let positive=max(vec3(y)+(rgb-vec3(y))*(y/(y-low)),vec3(0.));
    let peak=max(1.,max(positive.r,max(positive.g,positive.b)));
    return clamp(white+color*(positive/peak-white),vec3(0.),vec3(1.));
}
