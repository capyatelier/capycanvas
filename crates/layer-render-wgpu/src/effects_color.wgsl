// The common Float32 path keeps extended values between live adjustments.
// Only zero coverage loses RGB; small positive alpha is not an epsilon cutoff.
fn fx_unassociate(c:vec4<f32>)->vec3<f32> {
    if FX_EXTENDED {
        if c.a>0. {return c.rgb/c.a;}
        return vec3<f32>(0.);
    }
    return c.rgb/max(c.a,.000001);
}
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
    if FX_EXTENDED {
        // mix may lower to original + weight*(adjusted-original). Cancellation
        // then loses bits even at weight 1 when exposure changes by many stops.
        if weight==0. {return original;}
        if weight==1. {return adjusted;}
    }
    return mix(original,adjusted,weight);
}
// Full-precision interpolation at image-pass boundaries. Hardware filtering has
// insufficient fractional precision for the native integer16 sample contract.
fn fx_sample_float(image:texture_2d<f32>,point:vec2<f32>)->vec4<f32> {
    let p=point-.5;let origin=vec2<i32>(floor(p));let f=fract(p);
    let maximum=vec2<i32>(textureDimensions(image))-1;
    let a=textureLoad(image,clamp(origin,vec2<i32>(0),maximum),0);
    let b=textureLoad(image,clamp(origin+vec2<i32>(1,0),vec2<i32>(0),maximum),0);
    let c=textureLoad(image,clamp(origin+vec2<i32>(0,1),vec2<i32>(0),maximum),0);
    let d=textureLoad(image,clamp(origin+vec2<i32>(1,1),vec2<i32>(0),maximum),0);
    return mix(mix(a,b,f.x),mix(c,d,f.x),f.y);
}
