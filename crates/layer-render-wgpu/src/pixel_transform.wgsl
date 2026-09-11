// Original premultiplied layer pixels are immutable for the whole transaction.
// Sample color * selection together, never filter them independently (halos).
struct Transform { linear:vec4<f32>, translation_source_origin:vec4<f32>, target_flags:vec4<f32> }
@group(0) @binding(0) var<uniform> transform:Transform;
@group(1) @binding(0) var source:texture_2d<f32>;
// group 1, binding 1 and brush_selection_at come from selection_clip.wgsl.

@vertex fn vertex_main(@builtin(vertex_index) index:u32)->@builtin(position) vec4<f32> {
    let p=array<vec2<f32>,3>(vec2(-1.,-1.),vec2(3.,-1.),vec2(-1.,3.));
    return vec4(p[index],0.,1.);
}
fn original(p:vec2<i32>)->vec4<f32> {
    if any(p<vec2(0)) || any(p>=vec2<i32>(textureDimensions(source))) {return vec4(0.);}
    return textureLoad(source,p,0);
}
fn selected(p:vec2<i32>)->vec4<f32> {
    return original(p)*brush_selection_at(vec2<f32>(p)+transform.translation_source_origin.zw+vec2(.5));
}
fn transformed(local:vec2<f32>)->vec4<f32> {
    // Test before float->integer conversion; arbitrarily distant transforms
    // never create out-of-range integer coordinates or repeat edge texels.
    if any(local<vec2(-.5)) || any(local>vec2<f32>(textureDimensions(source))+vec2(.5)) {return vec4(0.);}
    if transform.target_flags.z==0. {return selected(vec2<i32>(floor(local)));}
    let p=local-vec2(.5);let base=vec2<i32>(floor(p));let t=fract(p);
    return mix(mix(selected(base),selected(base+vec2(1,0)),t.x),
        mix(selected(base+vec2(0,1)),selected(base+vec2(1,1)),t.x),t.y);
}
@fragment fn fragment_main(@builtin(position) position:vec4<f32>)->@location(0) vec4<f32> {
    let world=position.xy+transform.target_flags.xy;
    let local=world-transform.translation_source_origin.zw;
    let base=original(vec2<i32>(floor(local)));
    // Exact no-op must not cut and recomposite fractional selection coverage.
    if transform.target_flags.w!=0. {return base;}
    let m=transform.linear;
    let source_position=vec2(m.x*world.x+m.z*world.y,m.y*world.x+m.w*world.y)
        +transform.translation_source_origin.xy-transform.translation_source_origin.zw;
    let moved=transformed(source_position);
    let remainder=base*(1.-brush_selection_at(world));
    return moved+remainder*(1.-moved.a);
}
