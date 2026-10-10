const KEEP_SOURCE=256u;
fn remainder_selection(world:vec2<f32>)->f32 {
    if (flags()&KEEP_SOURCE)!=0u {return 0.;}
    return brush_selection_at(world);
}
fn over_remainder(world:vec2<f32>,moved:vec4<f32>)->vec4<f32> {
    let p=vec2<i32>(floor(world));
    let selection=remainder_selection(world);
    if visibility {
        let remainder=mix(original(p).r,background(),selection);
        return vec4(moved.r+remainder*(1.-moved.a));
    }
    if selection>=1. && !scalar {return moved;}
    let remainder=original(p)*(1.-selection);
    if scalar {return max(moved,remainder);}
    return moved+remainder*(1.-moved.a);
}
