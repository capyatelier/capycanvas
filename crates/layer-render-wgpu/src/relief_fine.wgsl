fn relief_pixel_box(dab:Dab,relative:vec2<f32>,field:vec2<f32>,outer:f32)->vec4<f32> {
    let inverse=vec4<f32>(dab.metric.w,-dab.metric.y,-dab.metric.z,dab.metric.x)
        /(dab.metric.x*dab.metric.w-dab.metric.y*dab.metric.z);
    let center=-vec2<f32>(dot(inverse.xy,field),dot(inverse.zw,field))-relative;
    let half=sqrt(outer)*vec2<f32>(length(inverse.xy),length(inverse.zw));
    return vec4<f32>(max(vec2<f32>(-0.5),center-half),min(vec2<f32>(0.5),center+half));
}

fn relief_tap_bounds(dab:Dab,world:vec2<f32>,paper:f32)->vec4<f32> {
    let pressure=clamp(dab.previous_contact.x,0.0,1.0);
    if pressure<=0.0 {return vec4<f32>(0.0);}
    let depth=0.5-clamp(0.5+style.contact_a.y*(paper-0.5),0.0,1.0);
    var outer=relief_inverse((0.5*pressure-depth)/(1.0+0.25*depth),dab.previous_contact.yzw);
    let shifted=contact_feature(32768u,style.contact_c.z!=0.0);
    if !shifted {outer=min(1.0,outer);}
    if outer<=0.0 {return vec4<f32>(0.0);}
    let relative=world-dab.center+dab.motion;
    let bounds=relief_pixel_box(dab,relative,select(vec2<f32>(0.0),dab.material.yz,shifted),outer);
    if !shifted {return bounds;}
    let physical=relief_pixel_box(dab,relative,vec2<f32>(0.0),1.0);
    return vec4<f32>(max(bounds.xy,physical.xy),min(bounds.zw,physical.zw));
}

fn relief_pixel_support(dab:Dab,world:vec2<f32>,paper:f32)->vec4<f32> {
    let relative=world-dab.center+dab.motion;
    let transverse=dot(relative,dab.metric.zw);
    let pressure=max(dab.previous_contact.x,dab.contact.x);
    let profile=min(dab.previous_contact.yzw,dab.contact.yzw);
    let depth=0.5-clamp(0.5+style.contact_a.y*(paper-0.5),0.0,1.0);
    var limit=relief_inverse((0.5*clamp(pressure,0.0,1.0)-depth)/(1.0+0.25*depth),profile);
    if !contact_feature(32768u,style.contact_c.z!=0.0) {limit=min(1.0,limit);}
    if limit<=0.0 {return vec4<f32>(0.0);}
    let normal=normalize(dab.metric.zw);
    var field_transverse=transverse;
    if contact_feature(32768u,style.contact_c.z!=0.0) {field_transverse+=dab.material.z;}
    var bounds=vec2<f32>(-sqrt(limit)-field_transverse,sqrt(limit)-field_transverse)/length(dab.metric.zw);
    if contact_feature(32768u,style.contact_c.z!=0.0) {
        let physical=vec2<f32>(-1.0-transverse,1.0-transverse)/length(dab.metric.zw);
        bounds=vec2<f32>(max(bounds.x,physical.x),min(bounds.y,physical.y));
    }
    return vec4<f32>(normal,bounds);
}

fn relief_pixel_cdf(position:vec2<f32>,projection:vec4<f32>)->vec2<f32> {
    let center=clamp(vec2<f32>(0.5)+position/projection.x,vec2<f32>(0.0),vec2<f32>(1.0));
    let product=projection.x*projection.y;
    if product<=0.00000001 {return center;}
    let distance=max(vec2<f32>(projection.z)-abs(position),vec2<f32>(0.0));
    let tail=0.5*distance*distance/product;
    return select(select(tail,vec2<f32>(1.0)-tail,position>=vec2<f32>(0.0)),center,abs(position)<=vec2<f32>(projection.w));
}

fn relief_supersampled(dab:Dab,world:vec2<f32>,paper:f32,dose:f32)->vec2<f32> {
    let still=all(dab.motion==vec2<f32>(0.0));
    let count=select(2u,4u,still);
    var bounds=vec4<f32>(0.0);
    var normal=vec2<f32>(0.0);
    var projection=vec4<f32>(0.0);
    if still {
        bounds=relief_tap_bounds(dab,world,paper);
    } else {
        let support=relief_pixel_support(dab,world,paper);
        normal=support.xy;
        let a=max(abs(normal.x),abs(normal.y));
        let b=min(abs(normal.x),abs(normal.y));
        let half=0.5*(a+b);
        projection=vec4<f32>(a,b,half,0.5*(a-b));
        bounds=vec4<f32>(max(-half,support.z),0.0,min(half,support.w),1.0);
    }
    if any(bounds.zw<=bounds.xy) {return vec2<f32>(0.0);}
    let middle=0.5*(bounds.xy+bounds.zw);
    let extent=0.5*(bounds.zw-bounds.xy);
    var positions=vec2<f32>(0.0);
    var weights=vec2<f32>(0.0);
    if !still {
        positions=vec2<f32>(middle.x)+extent.x*0.5773502691896257*vec2<f32>(-1.0,1.0);
        let density=select(vec2<f32>(1.0/projection.x),(vec2<f32>(projection.z)-abs(positions))/max(projection.x*projection.y,0.00000001),abs(positions)>vec2<f32>(projection.w));
        let total=density.x+density.y;
        let cdf=relief_pixel_cdf(bounds.xz,projection);
        weights=(cdf.y-cdf.x)*select(vec2<f32>(0.5),density/select(1.0,total,total>0.0),total>0.0);
    }
    var exposure=vec2<f32>(0.0);
    var alpha=0.0;
    for(var i=0u;i<count;i+=1u) {
        var offset=vec2<f32>(0.0);
        var weight=0.0;
        if still {
            let signs=vec2<f32>(select(-1.0,1.0,i%2u!=0u),select(-1.0,1.0,i>=2u));
            offset=middle+extent*0.5773502691896257*signs;
            weight=extent.x*extent.y;
        } else {
            offset=normal*positions[i];
            weight=weights[i];
        }
        let contact=relief_evaluate(dab,world+offset,paper,!contact_uniform() && !still,false,0.0);
        exposure+=weight*contact;
        alpha+=weight*(1.0-exp(-dose*select(contact.y,contact.x,still)));
    }
    if contact_uniform() {return vec2<f32>(exposure.x,0.0);}
    let full=-log(max(1.0-alpha,0.000001))/dose;
    if still {return vec2<f32>(full);}
    if exposure.y<=0.0 {return vec2<f32>(0.0);}
    return vec2<f32>(exposure.x*full/exposure.y,full);
}

fn relief_contact(dab:Dab,world:vec2<f32>,paper:f32,held:f32,required:bool)->vec2<f32> {
    let axes=0.5*(dab.radii+dab.previous.xy);
    let radius=min(axes.x,axes.y);
    var filtered=vec2<f32>(0.0);
    var blend=1.0;
    let fine=contact_feature(16384u,radius<8.0) && radius<8.0 && !contact_feature(4096u,style.material_a.x>0.0);
    if fine {
        let dose=6.0*dab.flow*dab.color.a;
        if dose<=0.0 {return vec2<f32>(0.0);}
        filtered=relief_supersampled(dab,world,paper,dose);
        blend=relief_transfer((radius-4.0)*0.25);
    }
    if blend>0.0 {filtered=mix(filtered,relief_exposure(dab,world,paper,held,required || fine),blend);}
    return filtered;
}
