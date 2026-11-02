fn relief_tap_pixel(dab:Dab,world:vec2<f32>,paper:f32,dose:f32)->f32 {
    let input=dab.previous_contact;
    let pressure=clamp(input.x,0.0,1.0);
    if pressure<=0.0 {return 0.0;}
    let profile=input.yzw;
    let depth=0.5-clamp(0.5+style.contact_a.y*(paper-0.5),0.0,1.0);
    let gap=0.5*pressure-depth;
    let k=1.0+0.25*depth;
    var outer=relief_inverse(gap/k,profile);
    if !contact_feature(32768u,style.contact_c.z!=0.0) {outer=min(1.0,outer);}
    if outer<=0.0 {return 0.0;}
    let inner=min(outer,relief_inverse((gap-1.0/style.material_a.y)/k,profile));
    let loading=1.0-style.material_a.z+style.material_a.z*pressure*pressure;
    let dx=vec2<f32>(dab.metric.x,dab.metric.z);
    let dy=vec2<f32>(dab.metric.y,dab.metric.w);
    let relative=world-dab.center+dab.motion;
    let physical=dx*relative.x+dy*relative.y;
    let origin=relief_origin(dab,physical);
    let a=origin+0.5*(dx+dy);
    let b=origin+0.5*(dx-dy);
    let c=origin-0.5*(dx+dy);
    let d=origin-0.5*(dx-dy);
    var inside=max(max(dot(a,a),dot(b,b)),max(dot(c,c),dot(d,d)))<=inner;
    if inside && contact_feature(32768u,style.contact_c.z!=0.0) {
        let aa=physical+0.5*(dx+dy);let bb=physical+0.5*(dx-dy);
        let cc=physical-0.5*(dx+dy);let dd=physical-0.5*(dx-dy);
        inside=max(max(dot(aa,aa),dot(bb,bb)),max(dot(cc,cc),dot(dd,dd)))<=1.0;
    }
    if inside {return loading;}
    let half=sqrt(outer)*length(dx)/abs(dab.metric.x*dab.metric.w-dab.metric.y*dab.metric.z);
    var center_y=relative.y;
    if contact_feature(32768u,style.contact_c.z!=0.0) {
        center_y+=(dab.metric.x*dab.material.z-dab.metric.z*dab.material.y)/(dab.metric.x*dab.metric.w-dab.metric.y*dab.metric.z);
    }
    var vertical=vec2<f32>(max(-0.5,-center_y-half),min(0.5,-center_y+half));
    if contact_feature(32768u,style.contact_c.z!=0.0) {
        let physical_half=length(dx)/abs(dab.metric.x*dab.metric.w-dab.metric.y*dab.metric.z);
        vertical=vec2<f32>(max(vertical.x,-relative.y-physical_half),min(vertical.y,-relative.y+physical_half));
    }
    if vertical.y<=vertical.x {return 0.0;}
    var integral=vec2<f32>(0.0);
    for(var row=0u;row<8u;row+=1u) {
        var node=0.1834346424956498;
        var weight=0.3626837833783620;
        switch row%4u {
            case 1u: {node=0.5255324099163290;weight=0.3137066458778873;}
            case 2u: {node=0.7966664774136267;weight=0.2223810344533745;}
            case 3u: {node=0.9602898564975363;weight=0.1012285362903763;}
            default: {}
        }
        let y=0.5*(vertical.x+vertical.y)+0.5*(vertical.y-vertical.x)*node*select(-1.0,1.0,row>=4u);
        let ray=relief_ray(origin+dy*y,dx);
        var bounds=vec2<f32>(-0.5,0.5);
        if contact_feature(32768u,style.contact_c.z!=0.0) {bounds=relief_range(relief_ray(physical+dy*y,dx),1.0,bounds);}
        let support=relief_range(ray,outer,bounds);
        if support.y<=support.x {continue;}
        let full=relief_range(ray,inner,support);
        var sum=vec2<f32>(loading,1.0-exp(-dose*loading))*(full.y-full.x);
        for(var edge=0u;edge<2u;edge+=1u) {
            let span=select(vec2<f32>(support.x,full.x),vec2<f32>(full.y,support.y),edge==1u);
            if span.y<=span.x {continue;}
            for(var column=0u;column<4u;column+=1u) {
                let extreme=column==0u || column==3u;
                let x=0.5*(span.x+span.y)+0.5*(span.y-span.x)
                    *select(0.3399810435848563,0.8611363115940526,extreme)*select(-1.0,1.0,column>=2u);
                let point=origin+dy*y+dx*x;
                let q=dot(point,point);
                let contact=loading*relief_transfer((gap-k*(profile.x+q*(profile.y+profile.z*q)))*style.material_a.y);
                sum+=vec2<f32>(contact,1.0-exp(-dose*contact))*0.5*(span.y-span.x)
                    *select(0.6521451548625461,0.3478548451374538,extreme);
            }
        }
        integral+=sum*0.5*(vertical.y-vertical.x)*weight;
    }
    if contact_uniform() {return integral.x;}
    return -log(max(1.0-integral.y,0.000001))/dose;
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
        if all(dab.motion == vec2<f32>(0.0)) {filtered=vec2<f32>(relief_tap_pixel(dab,world,paper,dose));}
        else {filtered=relief_swept_pixel(dab,world,paper,dose,radius);}
        blend=relief_transfer((radius-4.0)*0.25);
    }
    if blend>0.0 {filtered=mix(filtered,relief_exposure(dab,world,paper,held,required || fine),blend);}
    return filtered;
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

fn relief_pixel_span(normal:vec2<f32>,bounds:vec2<f32>,piece:u32)->vec2<f32> {
    let half=0.5*(abs(normal.x)+abs(normal.y));
    let flat=0.5*abs(abs(normal.x)-abs(normal.y));
    var span=vec2<f32>(-half,-flat);
    if piece==1u {span=vec2<f32>(-flat,flat);}
    if piece==2u {span=vec2<f32>(flat,half);}
    return vec2<f32>(max(span.x,bounds.x),min(span.y,bounds.y));
}

fn relief_pixel_node(i:u32,taps:u32)->vec2<f32> {
    let outer=taps==4u && (i==0u || i==3u);
    let abscissa:f32=select(select(0.3399810435848563,0.8611363115940526,outer),0.5773502691896257,taps==2u);
    let weight:f32=select(select(0.6521451548625461,0.3478548451374538,outer),1.0,taps==2u);
    return vec2<f32>(abscissa*select(-1.0,1.0,i>=taps/2u),weight);
}

fn relief_swept_pixel(dab:Dab,world:vec2<f32>,paper:f32,dose:f32,radius:f32)->vec2<f32> {
    let support=relief_pixel_support(dab,world,paper);
    if support.w<=support.z {return vec2<f32>(0.0);}
    let normal=support.xy;
    let a=max(abs(normal.x),abs(normal.y));
    let b=min(abs(normal.x),abs(normal.y));
    let half=0.5*(a+b);
    let flat=0.5*(a-b);
    let taps=select(4u,2u,radius>=4.0);
    var exposure=vec2<f32>(0.0);
    var alpha=0.0;
    for(var piece=0u;piece<3u;piece+=1u) {
        let span=relief_pixel_span(normal,support.zw,piece);
        if span.y<=span.x {continue;}
        for(var i=0u;i<taps;i+=1u) {
            let node=relief_pixel_node(i,taps);
            let position=0.5*(span.x+span.y)+0.5*(span.y-span.x)*node.x;
            let probability=select(1.0/a,(half-abs(position))/max(a*b,0.00000001),abs(position)>flat);
            let weight=0.5*(span.y-span.x)*node.y*probability;
            let sample=world+normal*position;
            let contact=relief_evaluate(dab,sample,paper,!contact_uniform(),false,0.0);
            exposure+=weight*contact;
            alpha+=weight*(1.0-exp(-dose*contact.y));
        }
    }
    if contact_uniform() {return vec2<f32>(exposure.x,0.0);}
    if exposure.y<=0.0 {return vec2<f32>(0.0);}
    let full=-log(max(1.0-alpha,0.000001))/dose;
    return vec2<f32>(exposure.x*full/exposure.y,full);
}
