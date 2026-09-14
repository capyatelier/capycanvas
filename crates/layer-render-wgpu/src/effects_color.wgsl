// The common Float32 path keeps extended values between live adjustments.
// Only zero coverage loses RGB; small positive alpha is not an epsilon cutoff.
fn fx_unassociate(c:vec4<f32>)->vec3<f32> {return working_unassociate(c);}
fn fx_output_range(c:vec3<f32>)->vec3<f32> {
    if FX_EXTENDED {return c;}
    return clamp(c,vec3<f32>(0.),vec3<f32>(1.));
}
fn fx_adjustment_rgb(c:vec4<f32>,adjusted:vec4<f32>,mode:u32)->vec3<f32> {
    if FX_EXTENDED {
        if c.a<=0. {return vec3<f32>(0.);}
        // Normal, alpha-preserving adjustments already have the required
        // association. Do not introduce a round trip at every fused node/pass.
        if mode==0u && c.a==adjusted.a {return adjusted.rgb;}
    }
    return fx_output_range(blend(fx_unassociate(adjusted),fx_unassociate(c),mode))*c.a;
}
fn fx_mix_rgb(original:vec3<f32>,adjusted:vec3<f32>,weight:f32)->vec3<f32> {
    return working_mix(original,adjusted,weight);
}
