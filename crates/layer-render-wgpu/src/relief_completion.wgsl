fn completion_piece(o:vec2<f32>,v:vec2<f32>,bounds:vec2<f32>,gap:f32,k:f32,profile:vec3<f32>)->vec2<f32> {
    let ray=relief_ray(o,v);
    let full=relief_piece(o,v,ray,bounds,gap,k,profile,true,false).x;
    if bounds.x>=0.0 {return vec2<f32>(full);}
    if bounds.y<=0.0 {return vec2<f32>(0.0,full);}
    let half=vec2<f32>(max(bounds.x,0.0),max(bounds.y,0.0));
    return vec2<f32>(relief_piece(o,v,ray,half,gap,k,profile,true,false).x,full);
}

fn completion_radial_line(o:vec2<f32>,v:vec2<f32>,gap:f32,k:f32,profile:vec3<f32>,limits:vec2<f32>)->vec2<f32> {
    let ray=relief_ray(o,v);
    let reach=sqrt(max(0.0,(1.0-ray.y)*ray.w));
    if reach<=0.0 {return vec2<f32>(0.0);}
    let radii=sqrt(max(vec2<f32>(0.0),(limits-vec2<f32>(ray.y))*ray.w));
    let outer=min(reach,radii.x);
    let inner=min(outer,radii.y);
    let center=o+v*ray.x;
    let full=inner+relief_partial(center,v,vec2<f32>(inner,outer),gap,k,profile);
    let distance=min(abs(ray.x),outer);
    var partial=min(distance,inner);
    if distance>inner {
        partial+=relief_partial(center,v,vec2<f32>(inner,distance),gap,k,profile);
    }
    return vec2<f32>(full+sign(ray.x)*partial,2.0*full)/(2.0*reach);
}

fn completion_line(dab:Dab,o:vec2<f32>,v:vec2<f32>,gap:f32,k:f32,profile:vec3<f32>,limits:vec2<f32>)->vec2<f32> {
    if dot(v,v)<0.00000001 {return vec2<f32>(0.0);}
    let rounded=contact_feature(4096u,style.material_a.x>0.0);
    var support=vec2<f32>(0.0);
    if !rounded {
        if !contact_feature(32768u,style.contact_c.z!=0.0) {
            return completion_radial_line(o,v,gap,k,profile,limits);
        }
        support=relief_range(relief_ray(o,v),1.0,vec2<f32>(-1000000.0,1000000.0));
        if support.y<=support.x {return vec2<f32>(0.0);}
        return completion_piece(relief_origin(dab,o),v,support,gap,k,profile)/(support.y-support.x);
    }
    let rim=dab.material.yz;
    let scaled=v/rim;
    let speeds=vec4<f32>(v,scaled.x+scaled.y,scaled.x-scaled.y);
    let inverse=select(vec4<f32>(0.0),1.0/select(vec4<f32>(1.0),speeds,abs(speeds)>vec4<f32>(0.000001)),abs(speeds)>vec4<f32>(0.000001));
    support=relief_beveled_range(o,v,rim,inverse);
    if support.y<=support.x {return vec2<f32>(0.0);}
    let face=vec2<f32>(1.0)-rim;
    let moving=abs(v)>vec2<f32>(0.000001);
    let safe=select(vec2<f32>(1.0),v,moving);
    let cuts=vec4<f32>(select(vec2<f32>(support.y),(-face-o)/safe,moving),
        select(vec2<f32>(support.y),(face-o)/safe,moving));
    var lo=support.x;
    var sum=vec2<f32>(0.0);
    for(var i=0u;i<5u;i+=1u) {
        let next=select(vec4<f32>(support.y),cuts,cuts>vec4<f32>(lo+0.000001));
        let hi=min(min(next.x,next.y),min(next.z,next.w));
        let mid=o+v*((lo+hi)*0.5);
        let engaged=abs(mid)>face;
        let edge_o=select(vec2<f32>(0.0),(sign(mid)*o-face)/rim,engaged);
        let edge_v=select(vec2<f32>(0.0),sign(mid)*v/rim,engaged);
        sum+=completion_piece(vec2<f32>(edge_o.x+edge_o.y,0.0),vec2<f32>(edge_v.x+edge_v.y,0.0),vec2<f32>(lo,hi),gap,k,profile);
        lo=hi;
        if hi>=support.y {break;}
    }
    return sum/(support.y-support.x);
}

fn completion_exposure(dab:Dab,world:vec2<f32>,paper:f32)->vec2<f32> {
    let p=clamp(dab.contact.x,0.0,1.0);
    if p<=0.0 {return vec2<f32>(0.0);}
    let delta=world-dab.center;
    let o=vec2<f32>(dot(delta,dab.metric.xy),dot(delta,dab.metric.zw));
    let rounded=contact_feature(4096u,style.material_a.x>0.0);
    var q=dot(o,o);
    if rounded {
        let edge=max(abs(o)-(vec2<f32>(1.0)-dab.material.yz),vec2<f32>(0.0))/dab.material.yz;
        q=(edge.x+edge.y)*(edge.x+edge.y);
    }
    if q>1.0 {return vec2<f32>(0.0);}
    if !rounded && !contact_feature(32768u,style.contact_c.z!=0.0)
        && dot(dab.invariants.xy,dab.invariants.xy)>0.5 && dot(dab.invariants.zw,dab.invariants.zw)>0.5
        && dot(o,dab.invariants.xy+dab.invariants.zw)<=0.0 {return vec2<f32>(0.0);}
    if !rounded {let shifted=relief_origin(dab,o);q=dot(shifted,shifted);}
    let d=0.5-clamp(0.5+style.contact_a.y*(paper-0.5),0.0,1.0);
    let gap=0.5*p-d;
    let k=1.0+0.25*d;
    let profile=dab.contact.yzw;
    let window=relief_transfer((gap-k*(profile.x+q*(profile.y+profile.z*q)))*style.material_a.y);
    if window<=0.0 {return vec2<f32>(0.0);}
    var limits=vec2<f32>(0.0);
    if !rounded && !contact_feature(32768u,style.contact_c.z!=0.0) {
        limits=vec2<f32>(relief_inverse(gap/k,profile),relief_inverse((gap-1.0/style.material_a.y)/k,profile));
    }
    let a=completion_line(dab,o,dab.invariants.xy,gap,k,profile,limits);
    let b=completion_line(dab,o,dab.invariants.zw,gap,k,profile,limits);
    let exposure=dab.texture_sign.y*window*max(0.0,max(a.y,b.y)-a.x-b.x);
    return vec2<f32>(exposure,max(max(a.y,b.y),a.x+b.x+exposure))
        *(1.0-style.material_a.z+style.material_a.z*p*p);
}
