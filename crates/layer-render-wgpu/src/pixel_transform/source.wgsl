struct Transform {
    x:vec4<f32>, y:vec4<f32>, w:vec4<f32>, attachment:vec4<f32>,
    display:vec4<f32>, texels:vec4<f32>, backdrop:vec4<f32>,
    bounds:vec4<i32>, views:array<vec4<i32>,16>,
}
@group(0) @binding(0) var<uniform> transform:Transform;
const NO_VIEW=16u;
const UNCOVERED=-1e38;
fn flags()->u32 {return u32(transform.attachment.z);}
fn background()->f32 {return transform.attachment.w;}
fn original(p:vec2<i32>)->vec4<f32> {
    if any(p<transform.bounds.xy) || any(p>=transform.bounds.xy+transform.bounds.zw) {return vec4(background());}
    for (var i=0u;i<16u;i++) {
        let view=transform.views[i];
        let local=p-view.xy;
        if all(local>=vec2(0)) && all(local<view.zw) {
            let color=source_load(i,local);
            if scalar {return vec4(color.r);}
            return color;
        }
    }
    return vec4(background());
}
fn covered(value:vec4<f32>,p:vec2<i32>)->vec4<f32> {
    var v=value;
    if visibility {v.a=1.;}
    return v*brush_selection_at(vec2<f32>(p)+vec2(.5));
}
fn selected(p:vec2<i32>)->vec4<f32> {return covered(original(p),p);}
fn view_of(low:vec2<i32>,high:vec2<i32>)->u32 {
    if any(low<transform.bounds.xy) || any(high>=transform.bounds.xy+transform.bounds.zw) {return NO_VIEW;}
    for (var i=0u;i<16u;i++) {
        let view=transform.views[i];
        if all(low>=view.xy) && all(high<view.xy+view.zw) {return i;}
    }
    return NO_VIEW;
}
fn tap(view:u32,p:vec2<i32>)->vec4<f32> {
    if view==NO_VIEW {return selected(p);}
    var color=source_load(view,p-transform.views[view].xy);
    if scalar {color=vec4(color.r);}
    return covered(color,p);
}
fn outside(selection:f32)->vec4<f32> {
    if placement {return vec4(background());}
    if visibility {return vec4(vec3(background()),1.)*selection;}
    return vec4(0.);
}
fn beyond_horizon()->vec4<f32> {
    return outside(horizon_selection());
}
fn far(local:vec2<f32>,margin:f32)->bool {
    return any(local<vec2<f32>(transform.bounds.xy)-margin)
        || any(local>vec2<f32>(transform.bounds.xy+transform.bounds.zw)+margin);
}
