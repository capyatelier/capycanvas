fn ac_hash(value:u32)->u32 {
    var v=value*747796405u+2891336453u;
    v=((v>>((v>>28u)+4u))^v)*277803737u;
    return (v>>22u)^v;
}
fn ac_key(cell:vec2<i32>,seed:u32)->u32 {
    return ac_hash(bitcast<u32>(cell.x)^ac_hash(bitcast<u32>(cell.y)^ac_hash(seed)));
}
fn ac_unit(key:u32)->f32 {return f32(ac_hash(key)>>8u)*(1./16777216.);}
fn ac_pair(key:u32)->vec2<f32> {return vec2(ac_unit(key^0x243f6a88u),ac_unit(key^0x85a308d3u));}
fn ac_integer(value:f32)->i32 {return i32(clamp(floor(value),-2147483520.,2147483520.));}
fn ac_cell(point:vec2<f32>)->vec2<i32> {return vec2(ac_integer(point.x),ac_integer(point.y));}
fn ac_less(a:vec4<f32>,b:vec4<f32>)->bool {
    let x=dot(a.rgb*.125,FX_LUMA);let y=dot(b.rgb*.125,FX_LUMA);
    if x!=y {return x<y;}
    if a.r!=b.r {return a.r<b.r;}
    if a.g!=b.g {return a.g<b.g;}
    if a.b!=b.b {return a.b<b.b;}
    return a.a<b.a;
}
fn ac_order(cell:vec2<i32>)->u32 {
    let p=bitcast<vec2<u32>>(cell)&vec2(1u);
    return ((p.x^p.y)<<1u)|p.y;
}
fn ac_representative(point:vec2<f32>,size:f32)->vec4<f32> {
    let cell=ac_cell(point/size);
    return fx_sample_nearest((vec2<f32>(cell)+ac_pair(ac_key(cell,19u)))*size);
}
fn capy_dither_spread(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32> {
    let spread=max(fx_parameter(b,0u).x,0.);let size=max(round(fx_parameter(b,1u).x),1.);
    let density=clamp(fx_parameter(b,2u).x*.01,0.,1.);
    if spread==0.||density==0. {return fx_sample_nearest(p);}
    let cell=ac_cell(p/size);
    if ac_unit(ac_key(cell,0xa4093822u))>=density {return fx_sample_nearest(p);}
    let center=(vec2<f32>(cell)+.5)*size;let reach=spread*.70710678118;
    var a=ac_representative(center+vec2(-reach,-reach),size);
    var d=ac_representative(center+vec2(reach,reach),size);
    var e=ac_representative(center+vec2(reach,-reach),size);
    var f=ac_representative(center+vec2(-reach,reach),size);
    if ac_less(e,a) {let v=a;a=e;e=v;}
    if ac_less(d,f) {let v=f;f=d;d=v;}
    if ac_less(f,a) {let v=a;a=f;f=v;}
    if ac_less(d,e) {let v=e;e=d;d=v;}
    if ac_less(f,e) {let v=e;e=f;f=v;}
    let rank=ac_order(cell);
    if rank==0u {return a;}
    if rank==1u {return e;}
    if rank==2u {return f;}
    return d;
}
fn capy_scatter(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32> {
    let spread=max(fx_parameter(b,0u).x,0.);let size=max(round(fx_parameter(b,1u).x),1.);
    let density=clamp(fx_parameter(b,2u).x*.01,0.,1.);let seed=u32(fx_parameter(b,3u).x);
    if spread==0.||density==0. {return fx_sample_nearest(p);}
    let phase_y=floor(ac_unit(seed^0x452821e6u)*size);
    let row=ac_integer((p.y-phase_y)/size);
    let phase_x=floor(ac_unit(bitcast<u32>(row)^ac_hash(seed^0x38d01377u))*size);
    let col=ac_integer((p.x-phase_x)/size);let key=ac_key(vec2(col,row),seed);
    if ac_unit(key^0xbe5466cfu)>=density {return fx_sample_nearest(p);}
    let origin=vec2(f32(col)*size+phase_x,f32(row)*size+phase_y);
    let donor=origin+ac_pair(key)*size+(ac_pair(key^0x34e90c6cu)*2.-1.)*spread;
    return fx_sample_nearest(donor);
}
fn capy_crystallize(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32> {
    let size=max(round(fx_parameter(b,0u).x),1.);let stretch=clamp(fx_parameter(b,1u).x*.01,-1.,1.);
    let sides=size*vec2(1.+max(stretch,0.),1.+max(-stretch,0.));
    let query=p/sides;let cell=ac_cell(query);let local=fract(query);
    var best=4.;var winner=vec2(.5);
    for(var y=-1;y<=1;y+=1) {for(var x=-1;x<=1;x+=1) {
        let offset=vec2(x,y);let site=.25+ac_pair(ac_key(cell+offset,0u))*.5;
        let delta=vec2<f32>(offset)+site-local;let distance=dot(delta,delta);
        if distance<best {best=distance;winner=vec2<f32>(offset)+site;}
    }}
    return fx_sample_nearest((vec2<f32>(cell)+winner)*sides);
}
