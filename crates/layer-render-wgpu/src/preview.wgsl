const PREVIEW_END_NONE:u32=4294967295u;

fn preview_end_index()->u32 {
    return style.operation.y-1u;
}

fn preview_enabled()->bool {
    return contact_feature(65536u,style.contact_a.z>0.5 && !contact_uniform() && preview_end_index()!=PREVIEW_END_NONE);
}

fn preview_contact_q(center:vec2<f32>,metric:vec4<f32>,rim:vec2<f32>,world:vec2<f32>)->f32 {
    let delta=world-center;
    let o=vec2<f32>(dot(delta,metric.xy),dot(delta,metric.zw));
    if contact_feature(4096u,style.material_a.x>0.0) {
        let edge=max(abs(o)-(vec2<f32>(1.0)-rim),vec2<f32>(0.0))/rim;
        return (edge.x+edge.y)*(edge.x+edge.y);
    }
    return dot(o,o);
}

fn preview_skip_tag(world:vec2<f32>)->u32 {
    var skip_tag:u32=4294967295u;
    if preview_enabled() {
        skip_tag=bitcast<u32>(-2.0);
        let index=preview_end_index();
        if index!=PREVIEW_END_NONE {
            var rim=vec2<f32>(0.0);
            if contact_feature(4096u,style.material_a.x>0.0) {rim=dabs[index].material.yz;}
            skip_tag=select(skip_tag,bitcast<u32>(-3.0),preview_contact_q(dabs[index].center,dabs[index].metric,rim,world)<=1.0);
        }
    }
    return skip_tag;
}

fn preview_skips(code:f32,skip_tag:u32)->bool {
    return bitcast<u32>(code)==skip_tag;
}
