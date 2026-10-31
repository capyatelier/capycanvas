fn relief_inverse(height: f32, profile: vec3<f32>) -> f32 {
    let h = height-profile.x;
    if h <= 0.0 { return -1.0; }
    if contact_feature(2048u,true) && profile.z > 0.000001 {
        return 2.0*h/(profile.y+sqrt(profile.y*profile.y+4.0*profile.z*h));
    }
    return h/max(profile.y, 0.000001);
}

fn relief_ray(o: vec2<f32>, v: vec2<f32>) -> vec4<f32> {
    let speed2 = dot(v,v);
    if speed2 < 0.00000001 { return vec4<f32>(0.0,dot(o,o),0.0,0.0); }
    let middle = -dot(o,v)/speed2;
    let nearest = o+v*middle;
    return vec4<f32>(middle,dot(nearest,nearest),speed2,1.0/speed2);
}

fn relief_range(ray: vec4<f32>, limit: f32, bounds: vec2<f32>) -> vec2<f32> {
    if ray.z < 0.00000001 {
        return select(vec2<f32>(bounds.x), bounds, ray.y <= limit);
    }
    let reach2 = (limit-ray.y)*ray.w;
    if reach2 <= 0.0 { return vec2<f32>(bounds.x); }
    let reach = sqrt(reach2);
    let lo = clamp(ray.x-reach,bounds.x,bounds.y);
    return vec2<f32>(lo,max(lo,clamp(ray.x+reach,bounds.x,bounds.y)));
}

fn relief_polynomial_pair(t2: f32, z: vec4<f32>, z4: f32) -> f32 {
    let even = z.x+t2*(z.z+t2*z4);
    let odd = z.y+t2*z.w;
    return even*even*(3.0-2.0*even)+3.0*t2*odd*odd*(1.0-2.0*even);
}

fn relief_partial(o: vec2<f32>, v: vec2<f32>, interval: vec2<f32>, gap: f32,
    k: f32, profile: vec3<f32>) -> f32 {
    let half = (interval.y-interval.x)*0.5;
    if half <= 0.0 { return 0.0; }
    let center = o+v*((interval.x+interval.y)*0.5);
    let velocity = v*half;
    let q0 = dot(center,center);
    let q1 = 2.0*dot(center,velocity);
    let q2 = dot(velocity,velocity);
    let slope = profile.y+2.0*profile.z*q0;
    let scale = -k*style.material_a.y;
    let z0 = (gap-k*(profile.x+q0*(profile.y+profile.z*q0)))*style.material_a.y;
    let z1 = scale*slope*q1;
    let z2 = scale*(slope*q2+profile.z*q1*q1);
    var average = 0.0;
    if !contact_feature(2048u,true) || profile.z <= 0.000001 {
        average = z0*z0*(3.0-2.0*z0)+2.0*z0*z2*(1.0-z0)+z1*z1*(1.0-2.0*z0)
            +z2*z2*(0.6-1.2*z0)-1.2*z1*z1*z2-(2.0/7.0)*z2*z2*z2;
    } else {
        let z3 = scale*2.0*profile.z*q1*q2;
        let z4 = scale*profile.z*q2*q2;
        let z = vec4<f32>(z0,z1,z2,z3);
        average = 0.2089795918367347*z0*z0*(3.0-2.0*z0)
            +0.3818300505051189*relief_polynomial_pair(0.1647102868965424,z,z4)
            +0.2797053914892767*relief_polynomial_pair(0.5498684992164435,z,z4)
            +0.1294849661688697*relief_polynomial_pair(0.9008058292716293,z,z4);
    }
    return clamp(average,0.0,1.0)*half*2.0;
}

fn relief_piece(o: vec2<f32>, v: vec2<f32>, ray: vec4<f32>, bounds: vec2<f32>, gap: f32,
    k: f32, profile: vec3<f32>, whole: bool) -> vec2<f32> {
    let support = bounds;
    let full = support.y-support.x;
    if full <= 0.0 { return vec2<f32>(0.0); }
    let stroke = select(vec2<f32>(max(support.x,0.0),min(support.y,1.0)),support,whole);
    if stroke.y <= stroke.x { return vec2<f32>(0.0,full); }
    if ray.z < 0.00000001 || profile.y+profile.z <= 0.000001 {
        let q = dot(o,o);
        let c = relief_transfer((gap-k*(profile.x+q*(profile.y+profile.z*q)))*style.material_a.y);
        return vec2<f32>(c*(stroke.y-stroke.x),full);
    }
    let a = o+v*stroke.x;
    let b = o+v*stroke.y;
    let qmax = max(dot(a,a),dot(b,b));
    if gap-k*(profile.x+qmax*(profile.y+profile.z*qmax)) >= 1.0/style.material_a.y {
        return vec2<f32>(stroke.y-stroke.x,full);
    }
    let half = 0.5*(stroke.y-stroke.x);
    if ray.z*half*half < style.material_b.w && !contact_feature(4096u,style.material_a.x>0.0) {
        let m = o+v*(stroke.x+half);
        let near = v*(0.5384693101056831*half);
        let far = v*(0.9061798459386640*half);
        let q = vec4<f32>(dot(m-far,m-far),dot(m-near,m-near),dot(m+near,m+near),dot(m+far,m+far));
        let qm = dot(m,m);
        let z = clamp((gap-k*(profile.x+q*(profile.y+profile.z*q)))*style.material_a.y,vec4<f32>(0.0),vec4<f32>(1.0));
        let zm = clamp((gap-k*(profile.x+qm*(profile.y+profile.z*qm)))*style.material_a.y,0.0,1.0);
        let c = z*z*(3.0-2.0*z);
        let sum = 0.2369268850561891*(c.x+c.w)+0.4786286704993665*(c.y+c.z)+0.5688888888888889*zm*zm*(3.0-2.0*zm);
        return vec2<f32>(sum*half,full);
    }
    let outer = relief_range(ray,relief_inverse(gap/k,profile),stroke);
    if outer.y <= outer.x { return vec2<f32>(0.0,full); }
    let inner = relief_range(ray,relief_inverse((gap-1.0/style.material_a.y)/k,profile),outer);
    let integral = inner.y-inner.x
        +relief_partial(o,v,vec2<f32>(outer.x,inner.x),gap,k,profile)
        +relief_partial(o,v,vec2<f32>(inner.y,outer.y),gap,k,profile);
    return vec2<f32>(integral,full);
}

fn relief_slab(o:f32,v:f32,inverse:f32,extent:f32)->vec2<f32> {
    if abs(v)<0.000001 {
        return select(vec2<f32>(1.0,-1.0),vec2<f32>(-1000000.0,1000000.0),abs(o)<=extent);
    }
    let a=(-extent-o)*inverse;
    let b=(extent-o)*inverse;
    return vec2<f32>(min(a,b),max(a,b));
}
fn relief_beveled_range(o:vec2<f32>,v:vec2<f32>,rim:vec2<f32>,inverse:vec4<f32>)->vec2<f32> {
    let face=vec2<f32>(1.0)-rim;
    let oo=o/rim;
    let vv=v/rim;
    let diagonal=1.0+face.x/rim.x+face.y/rim.y;
    let ax=relief_slab(o.x,v.x,inverse.x,1.0);
    let ay=relief_slab(o.y,v.y,inverse.y,1.0);
    let plus=relief_slab(oo.x+oo.y,vv.x+vv.y,inverse.z,diagonal);
    let minus=relief_slab(oo.x-oo.y,vv.x-vv.y,inverse.w,diagonal);
    let lo=max(max(ax.x,ay.x),max(plus.x,minus.x));
    let hi=min(min(ax.y,ay.y),min(plus.y,minus.y));
    let tolerance=0.0000005*max(1.0,max(abs(o.x),abs(o.y)));
    return vec2<f32>(lo,select(lo,max(lo,hi),(hi-lo)*length(v)>tolerance));
}

fn relief_origin(dab:Dab,origin:vec2<f32>)->vec2<f32> {
    if contact_feature(32768u,style.contact_c.z!=0.0) {return origin+dab.material.yz;}
    return origin;
}

fn relief_evaluate(dab: Dab, world: vec2<f32>, paper: f32, paired: bool) -> vec2<f32> {
    let relative = world-dab.center+dab.motion;
    let o = vec2<f32>(dot(relative,dab.metric.xy),dot(relative,dab.metric.zw));
    let corner = style.material_a.x;
    let rounded = contact_feature(4096u,corner>0.0);
    var v = vec2<f32>(-dab.invariants.x,0.0);
    var ray = vec4<f32>(o.x*dab.invariants.y,o.y*o.y,dab.invariants.z,dab.invariants.w);
    if rounded { v=dab.invariants.xy; ray=relief_ray(o,v); }
    else if contact_uniform() { ray=relief_ray(o,v); }
    else if ray.z<0.00000001 { ray=vec4<f32>(0.0,dot(o,o),0.0,0.0); }
    let progress = clamp(-dot(o,v)/max(dot(v,v),0.000001),0.0,1.0);
    let input = mix(dab.previous_contact,dab.contact,progress);
    let p = clamp(input.x,0.0,1.0);
    if p <= 0.0 { return vec2<f32>(0.0); }
    let d = 0.5-clamp(0.5+style.contact_a.y*(paper-0.5),0.0,1.0);
    let gap = 0.5*p-d;
    let k = 1.0+0.25*d;
    let profile = input.yzw;
    if gap <= k*profile.x { return vec2<f32>(0.0); }
    var engaged = true;
    var full_engaged = paired;
    if !rounded {
        let nearest = o+v*progress;
        let q = dot(nearest,nearest);
        engaged = q <= 1.0;
        if !contact_feature(32768u,style.contact_c.z!=0.0) {
            engaged = engaged && gap > k*(profile.x+q*(profile.y+profile.z*q));
        }
        if paired {
            let full_nearest = o+v*ray.x;
            let full_q = dot(full_nearest,full_nearest);
            full_engaged = full_q <= 1.0;
            if !contact_feature(32768u,style.contact_c.z!=0.0) {
                full_engaged = full_engaged && gap > k*(profile.x+full_q*(profile.y+profile.z*full_q));
            }
        }
        if !engaged && !full_engaged { return vec2<f32>(0.0); }
    }
    var sum = vec2<f32>(0.0);
    var full_integral = 0.0;
    if all(dab.motion == vec2<f32>(0.0)) {
        var q = dot(o,o);
        if rounded {
            let axes = max(dab.radii,vec2<f32>(0.005));
            let rim = vec2<f32>(corner*min(axes.x,axes.y))/axes;
            let edge = max(abs(o)-(vec2<f32>(1.0)-rim),vec2<f32>(0.0))/rim;
            q = (edge.x+edge.y)*(edge.x+edge.y);
        }
        if q > 1.0 { return vec2<f32>(0.0); }
        if contact_feature(32768u,style.contact_c.z!=0.0) {
            let point=relief_origin(dab,o);q=dot(point,point);
        }
        sum = vec2<f32>(relief_transfer((gap-k*(profile.x+q*(profile.y+profile.z*q)))*style.material_a.y),1.0);
    } else if !rounded {
        let support=relief_range(ray,1.0,vec2<f32>(-1000000.0,1000000.0));
        let origin=relief_origin(dab,o);
        var field_ray=ray;
        if contact_feature(32768u,style.contact_c.z!=0.0) {
            field_ray=vec4<f32>(origin.x*dab.invariants.y,origin.y*origin.y,ray.z,ray.w);
            if ray.z<0.00000001 {field_ray=vec4<f32>(0.0,dot(origin,origin),0.0,0.0);}
        }
        sum.y = support.y-support.x;
        if engaged {sum.x = relief_piece(origin,v,field_ray,support,gap,k,profile,false).x;}
        if full_engaged {full_integral = relief_piece(origin,v,field_ray,support,gap,k,profile,true).x;}
    } else {
        var rim = dab.invariants.zw;
        if contact_uniform() {
            let axes = max((dab.radii+dab.previous.xy)*0.5,vec2<f32>(0.005));
            rim = vec2<f32>(corner*min(axes.x,axes.y))/axes;
        }
        let support = relief_beveled_range(o,v,rim,dab.material);
        if support.y <= support.x { return vec2<f32>(0.0); }
        let bounds = vec2<f32>(max(support.x,0.0),min(support.y,1.0));
        if bounds.y <= bounds.x { return vec2<f32>(0.0); }
        sum.y = support.y-support.x;
        let face = vec2<f32>(1.0)-rim;
        if all(max(abs(o+v*bounds.x),abs(o+v*bounds.y))<=face) {
            let contact=relief_transfer((gap-k*profile.x)*style.material_a.y);
            return vec2<f32>(contact*(bounds.y-bounds.x)/sum.y*(1.0-style.material_a.z+style.material_a.z*p*p),0.0);
        }
        let moving = abs(v)>vec2<f32>(0.000001);
        let safe = select(vec2<f32>(1.0),v,moving);
        let cuts = vec4<f32>(select(vec2<f32>(bounds.y),(-face-o)/safe,moving),
            select(vec2<f32>(bounds.y),(face-o)/safe,moving));
        var lo = bounds.x;
        for (var i = 0u; i < 5u; i += 1u) {
            let next = select(vec4<f32>(bounds.y),cuts,cuts>vec4<f32>(lo+0.000001));
            let hi = min(min(next.x,next.y),min(next.z,next.w));
            let mid = o+v*((lo+hi)*0.5);
            let engaged = abs(mid)>face;
            let edge_o = select(vec2<f32>(0.0),(sign(mid)*o-face)/rim,engaged);
            let po = vec2<f32>(edge_o.x+edge_o.y,0.0);
            let edge_v = select(vec2<f32>(0.0),sign(mid)*v/rim,engaged);
            let pv = vec2<f32>(edge_v.x+edge_v.y,0.0);
            sum.x += relief_piece(po,pv,relief_ray(po,pv),vec2<f32>(lo,hi),gap,k,profile,false).x;
            lo = hi;
            if hi >= bounds.y { break; }
        }
    }
    return clamp(vec2<f32>(sum.x,full_integral)/max(sum.y,0.000001),vec2<f32>(0.0),vec2<f32>(1.0))*(1.0-style.material_a.z+style.material_a.z*p*p);
}
