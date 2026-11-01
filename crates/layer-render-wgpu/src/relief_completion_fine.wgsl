fn completion_rounded_pixel(dab:Dab,world:vec2<f32>,paper:f32,dose:f32)->vec2<f32> {
    var alpha=vec2<f32>(0.0);
    for(var row=0u;row<4u;row+=1u) {
        let outer=row==0u || row==3u;
        let row_node:f32=select(0.3399810435848563,0.8611363115940526,outer);
        let y=0.5*row_node*select(-1.0,1.0,row>=2u);
        let row_weight:f32=select(0.6521451548625461,0.3478548451374538,outer);
        let wy=0.5*row_weight;
        for(var col=0u;col<4u;col+=1u) {
            let edge=col==0u || col==3u;
            let col_node:f32=select(0.3399810435848563,0.8611363115940526,edge);
            let x=0.5*col_node*select(-1.0,1.0,col>=2u);
            let col_weight:f32=select(0.6521451548625461,0.3478548451374538,edge);
            let wx=0.5*col_weight;
            alpha+=wx*wy*(1.0-exp(-dose*completion_exposure(dab,world+vec2<f32>(x,y),paper)));
        }
    }
    return -log(max(vec2<f32>(1.0)-alpha,vec2<f32>(0.000001)))/dose;
}

fn completion_incident(dab:Dab,direction:vec2<f32>)->Dab {
    var body=dab;
    body.texture_sign=vec2<f32>(1.0);
    body.previous_contact=dab.contact;
    if dot(direction,direction)<0.00000001 {
        body.contact=vec4<f32>(0.0);body.previous_contact=vec4<f32>(0.0);
        return body;
    }
    let velocity=vec2<f32>(dot(direction,dab.metric.xy),dot(direction,dab.metric.zw));
    let speed=4.0+2.0/min(dab.radii.x,dab.radii.y);
    let axis=normalize(velocity);
    body.motion=direction*speed/length(velocity);
    body.center=dab.center+body.motion;
    body.metric=vec4<f32>(axis.x*dab.metric.xy+axis.y*dab.metric.zw,
        -axis.y*dab.metric.xy+axis.x*dab.metric.zw);
    body.invariants=vec4<f32>(speed,1.0/speed,speed*speed,1.0/(speed*speed));
    body.material.y=axis.x*dab.material.y;
    body.material.z=-axis.y*dab.material.y;
    return body;
}

fn completion_strip(o:vec2<f32>,dx:vec2<f32>,direction:vec2<f32>)->vec2<f32> {
    if dot(direction,direction)<0.00000001 {return vec2<f32>(0.5);}
    let transverse=o.x*direction.y-o.y*direction.x;
    let velocity=dx.x*direction.y-dx.y*direction.x;
    var bounds=vec2<f32>(-0.5,0.5);
    if abs(velocity)<0.000001 {
        if abs(transverse)>1.0 {return vec2<f32>(0.5);}
    } else {
        let slab=relief_slab(transverse,velocity,1.0/velocity,1.0);
        bounds=vec2<f32>(max(bounds.x,slab.x),min(bounds.y,slab.y));
    }
    let along=dot(o,direction);let slope=dot(dx,direction);
    if abs(slope)<0.000001 {
        if along<0.0 {return vec2<f32>(0.5);}
    } else if slope>0.0 {bounds.x=max(bounds.x,-along/slope);}
    else {bounds.y=min(bounds.y,-along/slope);}
    return vec2<f32>(bounds.x,max(bounds.x,bounds.y));
}

fn completion_joint_pixel(dab:Dab,a:Dab,b:Dab,world:vec2<f32>,paper:f32,dose:f32)->f32 {
    let dx=vec2<f32>(dab.metric.x,dab.metric.z);
    let dy=vec2<f32>(dab.metric.y,dab.metric.w);
    let relative=world-dab.center;
    let half=length(dx)/abs(dab.metric.x*dab.metric.w-dab.metric.y*dab.metric.z);
    var vertical=vec2<f32>(max(-0.5,-relative.y-half),min(0.5,-relative.y+half));
    if max(dab.previous_contact.y,dab.previous_contact.w)>0.000001 {vertical.y=0.5;}
    if min(dab.previous_contact.y,dab.previous_contact.w)<-0.000001 {vertical.x=-0.5;}
    if vertical.y<=vertical.x {return 0.0;}
    var alpha=0.0;
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
        let o=dx*relative.x+dy*(relative.y+y);
        var spans=array<vec2<f32>,3>(relief_range(relief_ray(o,dx),1.0,vec2<f32>(-0.5,0.5)),
            completion_strip(o,dx,-dab.invariants.xy),completion_strip(o,dx,-dab.invariants.zw));
        for(var i=0u;i<2u;i+=1u) {for(var j=i+1u;j<3u;j+=1u) {
            if spans[j].x<spans[i].x {let temp=spans[i];spans[i]=spans[j];spans[j]=temp;}
        }}
        var end=-0.5;
        for(var piece=0u;piece<3u;piece+=1u) {
            let span=vec2<f32>(max(end,spans[piece].x),min(0.5,spans[piece].y));
            end=max(end,span.y);
            if span.y<=span.x {continue;}
            for(var col=0u;col<4u;col+=1u) {
                let outer=col==0u || col==3u;
                let x=0.5*(span.x+span.y)+0.5*(span.y-span.x)
                    *select(0.3399810435848563,0.8611363115940526,outer)*select(-1.0,1.0,col>=2u);
                let wx=0.5*(span.y-span.x)*select(0.6521451548625461,0.3478548451374538,outer);
                let sample=world+vec2<f32>(x,y);
                let exposure=relief_evaluate(a,sample,paper,false,false,0.0).x+relief_evaluate(b,sample,paper,false,false,0.0).x
                    +completion_exposure(dab,sample,paper).x;
                alpha+=0.5*(vertical.y-vertical.x)*weight*wx*(1.0-exp(-dose*exposure));
            }
        }
    }
    return -log(max(1.0-alpha,0.000001))/dose;
}

fn relief_contact(dab:Dab,world:vec2<f32>,paper:f32,held:f32,required:bool)->vec2<f32> {
    if dab.texture_sign.x!=2.0 {return relief_segment_contact(dab,world,paper,held,required);}
    let radius=min(dab.radii.x,dab.radii.y);
    let coarse=completion_exposure(dab,world,paper);
    if radius>=8.0 {return coarse;}
    let dose=6.0*dab.flow*dab.color.a;
    if dose<=0.0 {return vec2<f32>(0.0);}
    let blend=relief_transfer((radius-4.0)*0.25);
    if contact_feature(4096u,style.material_a.x>0.0) {
        let filtered=completion_rounded_pixel(dab,world,paper,dose);
        return mix(vec2<f32>(filtered.x,max(filtered.y,coarse.y-coarse.x+filtered.x)),coarse,blend);
    }
    let determinant=abs(dab.metric.x*dab.metric.w-dab.metric.y*dab.metric.z);
    let extent=vec2<f32>(length(vec2<f32>(dab.metric.y,dab.metric.w)),
        length(vec2<f32>(dab.metric.x,dab.metric.z)))/determinant;
    if any(abs(world-dab.center)>extent+vec2<f32>(0.5)) {return vec2<f32>(0.0);}
    let a=completion_incident(dab,dab.previous_contact.xy);
    let b=completion_incident(dab,dab.previous_contact.zw);
    let point=relief_evaluate(a,world,paper,false,false,0.0).x+relief_evaluate(b,world,paper,false,false,0.0).x+coarse.x;
    let joint=mix(completion_joint_pixel(dab,a,b,world,paper,dose),point,blend);
    let incoming=relief_segment_contact(a,world,paper,0.0,true);
    let outgoing=relief_segment_contact(b,world,paper,0.0,true);
    return vec2<f32>(max(0.0,joint-incoming.x-outgoing.x),max(joint,max(incoming.y,outgoing.y)));
}
