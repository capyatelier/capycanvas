fn relief_contact(dab:Dab,world:vec2<f32>,paper:f32,held:f32,required:bool)->vec2<f32> {
    if dab.texture_sign.x!=2.0 {return relief_segment_contact(dab,world,paper,held,required);}
    let point=completion_exposure(dab,world,paper);
    let radius=min(dab.radii.x,dab.radii.y);
    if radius>=8.0 {return point;}
    let dose=6.0*dab.flow*dab.color.a;
    if dose<=0.0 {return vec2<f32>(0.0);}
    var alpha=vec2<f32>(0.0);
    for(var row=0u;row<3u;row+=1u) {for(var col=0u;col<3u;col+=1u) {
        let offset=(vec2<f32>(f32(col),f32(row))-vec2<f32>(1.0))/3.0;
        alpha+=vec2<f32>(1.0)-exp(-dose*completion_exposure(dab,world+offset,paper));
    }}
    let filtered=-log(max(vec2<f32>(1.0)-alpha/9.0,vec2<f32>(0.000001)))/dose;
    return mix(filtered,point,relief_transfer((radius-4.0)*0.25));
}
