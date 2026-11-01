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

fn completion_strip(o:vec2<f32>,dx:vec2<f32>,direction:vec2<f32>,slice:vec2<f32>)->vec2<f32> {
    if dot(direction,direction)<0.00000001 {return vec2<f32>(slice.y);}
    let transverse=o.x*direction.y-o.y*direction.x;
    let velocity=dx.x*direction.y-dx.y*direction.x;
    var bounds=slice;
    if abs(velocity)<0.000001 {
        if abs(transverse)>1.0 {return vec2<f32>(slice.y);}
    } else {
        let slab=relief_slab(transverse,velocity,1.0/velocity,1.0);
        bounds=vec2<f32>(max(bounds.x,slab.x),min(bounds.y,slab.y));
    }
    let along=dot(o,direction);let slope=dot(dx,direction);
    if abs(slope)<0.000001 {
        if along<0.0 {return vec2<f32>(slice.y);}
    } else if slope>0.0 {bounds.x=max(bounds.x,-along/slope);}
    else {bounds.y=min(bounds.y,-along/slope);}
    return vec2<f32>(bounds.x,max(bounds.x,bounds.y));
}

fn completion_joint_pixel(dab:Dab,a:Dab,b:Dab,world:vec2<f32>,paper:f32,dose:f32)->f32 {
    var support=relief_pixel_support(a,world,paper);
    if a.contact.x<=0.0 {support=relief_pixel_support(b,world,paper);}
    let normal=support.xy;
    let tangent=vec2<f32>(normal.y,-normal.x);
    let dx=vec2<f32>(dot(tangent,dab.metric.xy),dot(tangent,dab.metric.zw));
    if a.contact.x>0.0 && b.contact.x>0.0 {
        let half=0.5*(abs(normal.x)+abs(normal.y));support.z=-half;support.w=half;
    }
    if support.w<=support.z {return 0.0;}
    let taps=select(4u,2u,min(dab.radii.x,dab.radii.y)>=4.0);
    var alpha=0.0;
    for(var row_piece=0u;row_piece<3u;row_piece+=1u) {
        let vertical=relief_pixel_span(normal,support.zw,row_piece);
        if vertical.y<=vertical.x {continue;}
        for(var row=0u;row<taps;row+=1u) {
            let node=relief_pixel_node(row,taps);
            let y=0.5*(vertical.x+vertical.y)+0.5*(vertical.y-vertical.x)*node.x;
            let slice=relief_pixel_slice(normal,y);
            let position=world+normal*y;
            let relative=position-dab.center;
            let o=vec2<f32>(dot(relative,dab.metric.xy),dot(relative,dab.metric.zw));
            var spans=array<vec2<f32>,3>(relief_range(relief_ray(o,dx),1.0,slice),
                completion_strip(o,dx,-dab.invariants.xy,slice),completion_strip(o,dx,-dab.invariants.zw,slice));
            for(var i=0u;i<2u;i+=1u) {for(var j=i+1u;j<3u;j+=1u) {
                if spans[j].x<spans[i].x {let temp=spans[i];spans[i]=spans[j];spans[j]=temp;}
            }}
            var end=slice.x;
            for(var piece=0u;piece<3u;piece+=1u) {
                let span=vec2<f32>(max(end,spans[piece].x),min(slice.y,spans[piece].y));
                end=max(end,span.y);
                if span.y<=span.x {continue;}
                for(var col=0u;col<4u;col+=1u) {
                    let column=relief_pixel_node(col,4u);
                    let x=0.5*(span.x+span.y)+0.5*(span.y-span.x)*column.x;
                    let wx=0.5*(span.y-span.x)*column.y;
                    let sample=position+tangent*x;
                    let exposure=relief_evaluate(a,sample,paper,false,false,0.0).x+relief_evaluate(b,sample,paper,false,false,0.0).x
                        +completion_exposure(dab,sample,paper).x;
                    alpha+=0.5*(vertical.y-vertical.x)*node.y*wx*(1.0-exp(-dose*exposure));
                }
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
    let filtered=completion_joint_pixel(dab,completion_incident(dab,dab.previous_contact.xy),completion_incident(dab,dab.previous_contact.zw),world,paper,dose);
    let incoming=relief_segment_contact(completion_incident(dab,dab.previous_contact.xy),world,paper,0.0,true);
    let outgoing=relief_segment_contact(completion_incident(dab,dab.previous_contact.zw),world,paper,0.0,true);
    let point=relief_evaluate(completion_incident(dab,dab.previous_contact.xy),world,paper,false,false,0.0).x+relief_evaluate(completion_incident(dab,dab.previous_contact.zw),world,paper,false,false,0.0).x+coarse.x;
    let joint=mix(filtered,point,blend);
    return vec2<f32>(max(0.0,joint-incoming.x-outgoing.x),max(joint,max(incoming.y,outgoing.y)));
}
