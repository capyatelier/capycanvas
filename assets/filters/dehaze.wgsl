const DEHAZE_LUMA=vec3(.2126390058715104,.715168678767756,.07219231536073371);
fn dehaze_to_srgb(rgb:vec3<f32>)->vec3<f32>{return hdr_to_output(rgb);}
fn capy_dehaze(c:vec4<f32>,position:vec2<f32>,base:u32)->vec4<f32> {
    let amount=fx_parameter(base,0u).x;
    if amount==0. || (bitcast<u32>(c.a)&0x7fffffffu)==0u || fx_auxiliary_words(0u).x==0u {return c;}
    let atmosphere=fx_auxiliary(1u);
    if atmosphere.a<=0. {return c;}
    if !dehaze_direct_math(c) {
        let measure=guide_luminance(c,FX_LUMA);
        if measure.w==2. || measure.w==4. {return c;}
    }
    let bounded=bounded_source(c);let y=dot(bounded,DEHAZE_LUMA);
    let darkness=fx_guide_value(position,log2(max(y,exp2(-24.))),2u,0.);
    let transmission=max(.1,1.-.95*abs(amount)*.01*darkness);
    if transmission==1. || all(bounded==atmosphere.rgb) {return c;}
    var corrected=bounded*transmission+atmosphere.rgb*(1.-transmission);
    if amount>0. {
        let raw=(bounded-atmosphere.rgb)/transmission+atmosphere.rgb;
        let chroma=max(max(bounded.r,bounded.g),bounded.b)-min(min(bounded.r,bounded.g),bounded.b);
        let white=smoothstep(.5,.9,y)*(1.-smoothstep(.1,.35,chroma));
        corrected=bounded+(raw-bounded)*(1.-white);
        var stable=bounded;if y>0. {stable=(bounded/y)*max(dot(corrected,DEHAZE_LUMA),0.);}
        let weight=(1.-smoothstep(.02,.12,chroma))*(1.-smoothstep(.05,.18,dot(raw,DEHAZE_LUMA)));
        corrected+=weight*(stable-corrected);
    }
    let correction=hdr_from_output(corrected-bounded);
    let added=vec3(premult_correction(correction.r,c.a),premult_correction(correction.g,c.a),premult_correction(correction.b,c.a));
    return vec4(exact_add(c.r,added.r),exact_add(c.g,added.g),exact_add(c.b,added.b),c.a);
}
