fn bilinear_at(world:vec2<f32>)->vec4<f32> {
    let s=source_position(world);
    if s.z<=0. {return beyond_horizon();}
    return bilinear(s.xy);
}
fn filtered(world:vec2<f32>,s:vec2<f32>,dx:vec2<f32>,dy:vec2<f32>)->vec4<f32> {
    let count=vec2<u32>(clamp(floor(vec2(length(dx),length(dy))+.5),vec2(1.),vec2(max(transform.x.w,1.))));
    if any(count>vec2(1u)) {
        var sum=vec4(0.);
        for (var j=0u;j<count.y;j++) {
            for (var i=0u;i<count.x;i++) {
                let t=(vec2(f32(i),f32(j))+.5)/vec2<f32>(count);
                if MESH {
                    let o=t-.5;
                    sum+=bilinear(s+dx*o.x+dy*o.y);
                } else {
                    sum+=bilinear_at(world+t-.5);
                }
            }
        }
        return sum/f32(count.x*count.y);
    }
    return interpolate(s);
}
