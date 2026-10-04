struct Params {size:vec4<u32>,aux:vec4<u32>,values:vec4<f32>,weights:vec4<f32>}
@group(0) @binding(0) var<uniform> p:Params;
@group(0) @binding(1) var<storage,read> a:array<vec4<u32>>;
@group(0) @binding(2) var<storage,read> b:array<vec4<u32>>;
@group(0) @binding(3) var<storage,read> c:array<vec4<u32>>;
@group(0) @binding(4) var<storage,read_write> dst:array<vec4<u32>>;
@group(0) @binding(5) var source:texture_2d<f32>;
fn load_a(i:u32)->vec4<f32>{return bitcast<vec4<f32>>(a[i]);}
fn load_b(i:u32)->vec4<f32>{return bitcast<vec4<f32>>(b[i]);}
fn load_c(i:u32)->vec4<f32>{return bitcast<vec4<f32>>(c[i]);}
fn load_dst(i:u32)->vec4<f32>{return bitcast<vec4<f32>>(dst[i]);}
fn store(i:u32,value:vec4<f32>){dst[i]=bitcast<vec4<u32>>(value);}
const DEHAZE_LUMA=vec3(.2126390058715104,.715168678767756,.07219231536073371);
fn dehaze_to_srgb(rgb:vec3<f32>)->vec3<f32>{return vec3(dot(load_c(0).xyz,rgb),dot(load_c(1).xyz,rgb),dot(load_c(2).xyz,rgb));}
fn index_at(point:vec2<i32>)->u32{let at=vec2<u32>(clamp(point,vec2(0),vec2<i32>(p.size.xy)-1));return at.y*p.size.x+at.x;}
fn cell(i:u32)->vec4<f32>{return load_a(2u*i);}
fn cell_exponent(i:u32)->f32{return load_a(2u*i+1u).y;}

@compute @workgroup_size(8,8) fn reduce_source(@builtin(global_invocation_id) id:vec3<u32>){
    let xy=id.xy+p.size.zw;if any(xy>=p.size.xy){return;}
    let i=xy.y*p.size.x+xy.x;let first=xy*p.aux.zw;let last=(xy+1u)*p.aux.zw;
    let low=max(first/p.size.xy,p.aux.xy);let high=min((last+p.size.xy-1u)/p.size.xy,p.aux.xy+textureDimensions(source));
    let old=load_dst(2u*i);let metadata=load_dst(2u*i+1u);var scale=select(-149,i32(metadata.x),old.a>0.);var invalid=metadata.z;
    for(var y=low.y;y<high.y;y++){for(var x=low.x;x<high.x;x++){
        let pixel=textureLoad(source,vec2<i32>(vec2(x,y)-p.aux.xy),0);let measure=guide_luminance(pixel,DEHAZE_LUMA);
        if measure.w==4.{invalid=1.;continue;}if measure.w==0. || measure.w==2.{continue;}
        let rgb=bounded_source(pixel);let bits=bitcast<vec3<u32>>(rgb)&vec3<u32>(0x7fffffffu);let largest=max(max(bits.r,bits.g),bits.b);
        if largest!=0u{scale=max(scale,float_number(bitcast<f32>(largest)).exponent);}
    }}
    var previous=vec3(0.);if old.a>0.{previous=ldexp(old.rgb,vec3<i32>(i32(metadata.x)-scale));}
    var means=array<vec3<f32>,3>(vec3(previous.r,old.a,metadata.y),vec3(previous.g,old.a,metadata.y),vec3(previous.b,old.a,metadata.y));
    for(var y=low.y;y<high.y;y++){for(var x=low.x;x<high.x;x++){
        let pixel=textureLoad(source,vec2<i32>(vec2(x,y)-p.aux.xy),0);let measure=guide_luminance(pixel,DEHAZE_LUMA);
        if measure.w==0. || measure.w==2. || measure.w==4.{continue;}
        let overlap=min(last,(vec2(x,y)+1u)*p.size.xy)-max(first,vec2(x,y)*p.size.xy);
        let weight=f32(overlap.x*overlap.y)/f32(p.aux.z*p.aux.w);let rgb=scaled_bounded_source(pixel,scale);let alpha=float_number(pixel.a);
        for(var channel=0u;channel<3u;channel++){means[channel]=guide_mean(means[channel],vec3(rgb[channel],f32(alpha.mantissa)/8388608.,f32(alpha.exponent)),weight);}
    }}
    store(2u*i,vec4(means[0].x,means[1].x,means[2].x,means[0].y));store(2u*i+1u,vec4(f32(scale),means[0].z,invalid,0.));
}
@compute @workgroup_size(8,8) fn normalize(@builtin(global_invocation_id) id:vec3<u32>){
    if any(id.xy>=p.size.xy){return;}let i=id.y*p.size.x+id.x;let value=cell(i);let metadata=load_a(2u*i+1u);
    store(2u*i,vec4(packed_scaled(value.r,i32(metadata.x)),packed_scaled(value.g,i32(metadata.x)),packed_scaled(value.b,i32(metadata.x)),value.a));
    store(2u*i+1u,metadata);
}
@compute @workgroup_size(8,8) fn minimum(@builtin(global_invocation_id) id:vec3<u32>){
    if any(id.xy>=p.size.xy){return;}var minimum=0x7f800000u;
    for(var k=-7;k<=7;k++){
        let delta=select(vec2(0,k),vec2(k,0),p.aux.x==0u);let i=index_at(vec2<i32>(id.xy)+delta);
        if p.aux.x==0u {
            let value=cell(i);if value.a==0.{continue;}var rgb=value.rgb;
            if p.aux.y!=0u {rgb=vec3(unassociated_component(rgb.r,float_number(load_c(0).r)),unassociated_component(rgb.g,float_number(load_c(0).g)),unassociated_component(rgb.b,float_number(load_c(0).b)));}
            let keys=bitcast<vec3<u32>>(rgb);minimum=min(minimum,min(min(keys.r,keys.g),keys.b));
        }else{minimum=min(minimum,b[i].x);}
    }
    dst[id.y*p.size.x+id.x]=vec4(minimum,0u,0u,0u);
}

var<workgroup> counts:array<atomic<u32>,256>;
@compute @workgroup_size(256) fn rank_histogram(@builtin(local_invocation_index) lane:u32,@builtin(workgroup_id) group:vec3<u32>){
    atomicStore(&counts[lane],0u);workgroupBarrier();
    let state=c[0];let threshold=c[1].x;
    for(var i=group.x*256u+lane;i<p.size.x*p.size.y;i+=64u*256u){
        if cell(i).a==0.{continue;}let dark=b[i].x;
        if p.aux.y!=0u && dark!=threshold{continue;}
        let key=select(dark,i,p.aux.y!=0u);if (key&state.y)!=state.x{continue;}
        atomicAdd(&counts[(key>>p.aux.x)&255u],1u);
    }
    workgroupBarrier();
    for(var j=lane;j<64u;j+=256u){let first=j*4u;dst[group.x*64u+j]=vec4(atomicLoad(&counts[first]),atomicLoad(&counts[first+1u]),atomicLoad(&counts[first+2u]),atomicLoad(&counts[first+3u]));}
}
var<workgroup> totals:array<u32,256>;
@compute @workgroup_size(256) fn rank_select(@builtin(local_invocation_index) lane:u32){
    var count=0u;for(var group=0u;group<64u;group++){count+=a[group*64u+lane/4u][lane%4u];}totals[lane]=count;workgroupBarrier();
    if lane!=0u{return;}var state=b[0];var extra=b[1];
    if p.aux.x==24u && p.aux.y==0u {var total=0u;for(var i=0u;i<256u;i++){total+=totals[i];}state.z=max(1u,(total+999u)/1000u);state.w=total;}
    var remaining=state.z;var selected=0u;
    for(var i=0u;i<256u;i++){let bucket=select(255u-i,i,p.aux.y!=0u);if remaining<=totals[bucket]{selected=bucket;break;}remaining-=totals[bucket];}
    state.x|=selected<<p.aux.x;state.y|=255u<<p.aux.x;state.z=remaining;
    if p.aux.x==0u {if p.aux.y==0u{extra.x=state.x;state.x=0u;state.y=0u;}else{extra.y=state.x;}}
    dst[0]=state;dst[1]=extra;
}
var<workgroup> chosen:array<vec4<u32>,256>;
fn choose_air(left:vec4<u32>,right:vec4<u32>)->vec4<u32>{
    var best=left;if right.x>left.x || (right.x==left.x && right.y<left.y){best=right;}best.z=left.z|right.z;return best;
}
@compute @workgroup_size(256) fn air_reduce(@builtin(local_invocation_index) lane:u32,@builtin(workgroup_id) group:vec3<u32>){
    var value=vec4(0u,0xffffffffu,0u,0u);let i=group.x*256u+lane;
    if i<p.aux.z {
        if p.aux.x==0u {
            let sample=cell(i);let threshold=c[1];let dark=b[i].x;
            if sample.a>0. && (dark>threshold.x || (dark==threshold.x && i<=threshold.y)){value.x=bitcast<u32>(safe_luma(sample.rgb));value.y=i;}
            value.z=u32(load_a(2u*i+1u).z);
        }else{value=a[i];}
    }
    chosen[lane]=value;workgroupBarrier();
    for(var stride=128u;stride>0u;stride/=2u){if lane<stride{chosen[lane]=choose_air(chosen[lane],chosen[lane+stride]);}workgroupBarrier();}
    if lane==0u{dst[group.x]=chosen[0];}
}
@compute @workgroup_size(1) fn air_finish(){
    let chosen=b[0];var air=vec4(0.);if chosen.y!=0xffffffffu && chosen.x!=0u{air=vec4(max(cell(chosen.y).rgb,vec3(exp2(-16.))),1.);}
    store(0,air);dst[1]=chosen;
}

struct Moment {mean:vec2<f32>,central:vec2<f32>,coverage:vec2<f32>}
fn combine(left:Moment,right:Moment)->Moment{
    if right.coverage.x==0.{return left;}if left.coverage.x==0.{return right;}
    let exponent=max(left.coverage.y,right.coverage.y);
    let a=ldexp(left.coverage.x,i32(max(left.coverage.y-exponent,-126.)));let b=ldexp(right.coverage.x,i32(max(right.coverage.y-exponent,-126.)));
    let total=frexp(a+b);let ratio=b/(a+b);let delta=right.mean-left.mean;
    return Moment(left.mean+delta*ratio,left.central*(1.-ratio)+right.central*ratio+delta.x*delta*(ratio*(1.-ratio)),vec2(total.fract,exponent+f32(total.exp)));
}
@compute @workgroup_size(8,8) fn moments(@builtin(global_invocation_id) id:vec3<u32>){
    if any(id.xy>=p.size.xy){return;}var result=Moment(vec2(0.),vec2(0.),vec2(0.));
    for(var k=-8;k<=8;k++){
        let i=index_at(vec2<i32>(id.xy)+select(vec2(0,k),vec2(k,0),p.aux.x==0u));
        var next:Moment;
        if p.aux.x==0u {let sample=cell(i);next=Moment(vec2(safe_luma(sample.rgb),select(load_b(i).x,0.,sample.a==0.)),vec2(0.),vec2(sample.a,cell_exponent(i)));}
        else{next=Moment(load_b(2u*i).xy,load_b(2u*i).zw,load_b(2u*i+1u).xy);}
        result=combine(result,next);
    }
    let i=id.y*p.size.x+id.x;
    if p.aux.x==0u{store(2u*i,vec4(result.mean,result.central));store(2u*i+1u,vec4(result.coverage,0.,0.));}
    else{var coefficients=vec2(0.);if result.coverage.x>0.{let slope=result.central.y/(max(0.,result.central.x)+.001);coefficients=vec2(slope,result.mean.y-slope*result.mean.x);}
        store(i,vec4(coefficients,cell(i).a,cell_exponent(i)));}
}
@compute @workgroup_size(8,8) fn refine(@builtin(global_invocation_id) id:vec3<u32>){
    if any(id.xy>=p.size.xy){return;}var first=vec3(0.);var second=vec3(0.);
    for(var k=-8;k<=8;k++){let i=index_at(vec2<i32>(id.xy)+select(vec2(0,k),vec2(k,0),p.aux.x==0u));let value=load_b(i);first=guide_mean(first,value.xzw,1.);second=guide_mean(second,value.yzw,1.);}
    let i=id.y*p.size.x+id.x;
    if p.aux.x==0u{store(i,vec4(first.x,second.x,first.yz));}
    else{let y=safe_luma(cell(i).rgb);var darkness=0.;if first.y>0.{darkness=clamp(first.x*y+second.x,0.,1.);}
        store(i+2u,vec4(log2(max(y,exp2(-24.))),darkness,cell(i).a,cell_exponent(i)));
        if i==0u{dst[0]=vec4(p.size.xy,p.aux.zw);store(1,load_c(0));}}
}
