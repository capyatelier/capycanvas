// Two ordinary image passes sharing the prepared table. Premultiplied linear
// color and alpha are filtered together, including transparent input pixels.
fn tent_sample(p:vec2<f32>,b:u32,axis:vec2<f32>)->vec4<f32> {
    let head=fx_lookup(b,0u,0u);
    var value=fx_sample(p)*head.x;
    for(var i=1u;i<=u32(head.y);i+=1u) {
        let offset=axis*f32(i);
        value+=(fx_sample(p-offset)+fx_sample(p+offset))*fx_lookup(b,0u,i).x;
    }
    return value;
}
fn tent_horizontal(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32> {return tent_sample(p,b,vec2<f32>(1.,0.));}
fn tent_vertical(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32> {return tent_sample(p,b,vec2<f32>(0.,1.));}
