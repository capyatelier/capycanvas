fn relief_contact(dab:Dab,world:vec2<f32>,paper:f32,held:f32,required:bool)->vec2<f32> {
    if dab.texture_sign.x!=2.0 {return relief_segment_contact(dab,world,paper,held,required);}
    let radius=min(dab.radii.x,dab.radii.y);
    let coarse=contact_feature(4096u,style.material_a.x>0.0) || radius>=8.0;
    let dose=6.0*dab.flow*dab.color.a;
    if !coarse && dose<=0.0 {return vec2<f32>(0.0);}
    let count=select(5u,1u,coarse);
    var middle=vec2<f32>(0.0);
    var extent=vec2<f32>(0.0);
    if !coarse {
        let bounds=relief_pixel_box(dab,world-dab.center,vec2<f32>(0.0),1.0);
        if any(bounds.zw<=bounds.xy) {return vec2<f32>(0.0);}
        middle=0.5*(bounds.xy+bounds.zw);
        extent=0.5*(bounds.zw-bounds.xy);
    }
    var point=vec2<f32>(0.0);
    var alpha=vec2<f32>(0.0);
    for(var i=0u;i<count;i+=1u) {
        var offset=vec2<f32>(0.0);
        if i>0u {
            let signs=vec2<f32>(select(-1.0,1.0,(i-1u)%2u!=0u),select(-1.0,1.0,i>=3u));
            offset=middle+extent*0.5773502691896257*signs;
        }
        let exposure=completion_exposure(dab,world+offset,paper);
        if i==0u {point=exposure;}
        else {alpha+=extent.x*extent.y*(vec2<f32>(1.0)-exp(-dose*exposure));}
    }
    if coarse {return point;}
    let filtered=-log(max(vec2<f32>(1.0)-alpha,vec2<f32>(0.000001)))/dose;
    return mix(filtered,point,relief_transfer((radius-4.0)*0.25));
}
