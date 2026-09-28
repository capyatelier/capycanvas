// The common Float32 path keeps extended values between live adjustments.
// Only zero coverage loses RGB; small positive alpha is not an epsilon cutoff.
fn fx_unassociate(c:vec4<f32>)->vec3<f32> {return working_unassociate(c);}
fn fx_output_range(c:vec3<f32>)->vec3<f32> {return c;}
// An effect layer blends its result over its input, weighted by opacity and mask.
fn fx_adjustment(c:vec4<f32>,adjusted:vec4<f32>,code:u32,weight:f32)->vec4<f32> {
    if code==0u && c.a==adjusted.a && c.a>0. {return vec4<f32>(fx_mix_rgb(c.rgb,adjusted.rgb,weight),c.a);}
    return blend_clip(vec4<f32>(fx_unassociate(adjusted)*weight,weight),c,code);
}
fn fx_mix_rgb(original:vec3<f32>,adjusted:vec3<f32>,weight:f32)->vec3<f32> {
    return working_mix(original,adjusted,weight);
}
