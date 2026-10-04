struct Area { region:vec4<u32>, flags:vec4<u32>, weights:vec2<f32>, waveform:u32, padding:u32 }
struct Selection { rect:vec4<u32>, info:vec4<u32>, values:array<u32> }
@group(0) @binding(0) var pixels:texture_2d<f32>;
@group(0) @binding(1) var<uniform> area:Area;
@group(0) @binding(2) var<storage,read> boundaries:array<vec4<u32>>;
@group(0) @binding(3) var<storage,read_write> shards:array<atomic<u32>>;
@group(0) @binding(4) var<storage,read_write> summary:array<u32>;
@group(0) @binding(5) var<storage,read> selection:Selection;
const PENDING:u32=66752u;
const WAVE_OFFSET:u32=132289u;
const WAVE_WORDS:u32=262144u;
fn waveform(index:u32,channel:u32,shard:u32,amount:u32) {
    if amount!=0u {atomicAdd(&shards[WAVE_OFFSET+(shard%8u)*WAVE_WORDS+channel*65536u+index],amount);}
}
var<workgroup> counts:array<atomic<u32>,1043>;

fn product(a:u32,b:u32)->vec2<u32> {
    let low=(a&65535u)*(b&65535u);
    let cross=(a>>16u)*(b&65535u)+(low>>16u);
    let middle=(a&65535u)*(b>>16u)+(cross&65535u);
    return vec2<u32>((middle<<16u)|(low&65535u),(a>>16u)*(b>>16u)+(cross>>16u)+(middle>>16u));
}
fn at_least(p:FloatNumber,a:FloatNumber,b:vec4<u32>)->bool {
    if p.negative || p.mantissa==0u {return false;}
    let difference=p.exponent-a.exponent-bitcast<i32>(b.z);
    if difference>=2 {return true;}
    if difference<=-2 {return false;}
    let shift=u32(52+difference);
    let lhs=vec3<u32>(0u,p.mantissa<<(shift-32u),p.mantissa>>(64u-shift));
    let low=product(a.mantissa,b.x);let high=product(a.mantissa,b.y);
    let middle=low.y+high.x;
    let rhs=vec3<u32>(low.x,middle,high.y+u32(middle<low.y));
    return lhs.z>rhs.z || (lhs.z==rhs.z && (lhs.y>rhs.y || (lhs.y==rhs.y && lhs.x>=rhs.x)));
}
fn bin(p:FloatNumber,a:FloatNumber,channel:u32)->u32 {
    if p.mantissa==0u || p.negative {return 0u;}
    let logarithm=log2(f32(p.mantissa)/f32(a.mantissa))+f32(p.exponent-a.exponent);
    var low=select(0u,1u,area.flags.w!=0u);var high=255u;
    let offset=select(0u,257u,channel==3u);
    while low<high {
        let middle=(low+high+1u)/2u;
        if logarithm>=bitcast<f32>(boundaries[offset+middle].w) {low=middle;} else {high=middle-1u;}
    }
    while low<255u && at_least(p,a,boundaries[offset+low+1u]) {low+=1u;}
    let minimum=select(0u,1u,area.flags.w!=0u);
    while low>minimum && !at_least(p,a,boundaries[offset+low]) {low-=1u;}
    return low;
}
fn classification(value:FloatNumber,alpha:FloatNumber,channel:u32)->vec2<u32> {
    let zero=value.mantissa==0u;let below=value.negative && !zero;
    let equal= !value.negative && value.mantissa==alpha.mantissa && value.exponent==alpha.exponent;
    let white=at_least(value,alpha,vec4<u32>(0u,0x100000u,0u,0u));
    return vec2<u32>(bin(value,alpha,channel),u32(below)|(u32(white && !equal)<<1u)|(u32(below || zero)<<2u)|(u32(white)<<3u));
}
fn normalized(value:FloatNumber,exponent:i32)->f32 {
    if value.mantissa==0u {return 0.;}
    return select(1.,-1.,value.negative)*f32(value.mantissa)*exp2(f32(value.exponent-exponent-23));
}
fn approximate_luminance(rgb:array<FloatNumber,3>,alpha:FloatNumber)->vec2<u32> {
    let exponent=max(max(select(-149,rgb[0].exponent,rgb[0].mantissa!=0u),select(-149,rgb[1].exponent,rgb[1].mantissa!=0u)),select(-149,rgb[2].exponent,rgb[2].mantissa!=0u));
    let r=normalized(rgb[0],exponent);let g=normalized(rgb[1],exponent);let b=normalized(rgb[2],exponent);
    let value=g+area.weights.x*(r-g)+area.weights.y*(b-g);
    var low=float_number(value-0.000003814697265625);var high=float_number(value+0.000003814697265625);
    low.exponent+=exponent;high.exponent+=exponent;
    let first=classification(low,alpha,3u);let last=classification(high,alpha,3u);
    if all(first==last) {return first;}
    return vec2<u32>(256u,0u);
}
const LIMBS:u32=13u;
struct Wide { words:array<u32,13>, negative:bool }
fn multiply(a:u32,b:vec4<u32>)->vec3<u32> {
    let low=product(a,b.x);let high=product(a,b.y);let middle=low.y+high.x;
    return vec3<u32>(low.x,middle,high.y+u32(middle<low.y));
}
fn wide(value:vec3<u32>,shift:i32)->Wide {
    var result:Wide;result.negative=false;
    var rounded=false;
    for(var i=0;i<3;i++) {
        let position=shift+i*32;
        if position>=0 {
            let index=u32(position)/32u;let bits=u32(position)%32u;
            if index<LIMBS {result.words[index]|=value[i]<<bits;}
            if bits!=0u && index+1u<LIMBS {result.words[index+1u]|=value[i]>>(32u-bits);}
        } else if position> -32 {
            result.words[0]|=value[i]>>u32(-position);
            rounded=rounded || (value[i]<<u32(32+position))!=0u;
        } else {rounded=rounded || value[i]!=0u;}
    }
    if rounded {result.words[0]+=1u;}
    return result;
}
fn wide_add(a:Wide,b:Wide)->Wide {
    var result:Wide;var carry=0u;
    for(var i=0u;i<LIMBS;i++) {
        let sum=a.words[i]+b.words[i];let total=sum+carry;
        carry=u32(sum<a.words[i] || total<sum);result.words[i]=total;
    }
    return result;
}
fn wide_compare(a:Wide,b:Wide)->i32 {
    for(var i=i32(LIMBS)-1;i>=0;i--) {
        if a.words[i]!=b.words[i] {return select(-1,1,a.words[i]>b.words[i]);}
    }
    return 0;
}
fn wide_subtract(a:Wide,b:Wide)->Wide {
    var result:Wide;var borrow=0u;
    for(var i=0u;i<LIMBS;i++) {
        let value=a.words[i]-b.words[i];let remainder=value-borrow;
        borrow=u32(a.words[i]<b.words[i] || value<borrow);result.words[i]=remainder;
    }
    return result;
}
fn luminance(rgb:array<FloatNumber,3>)->Wide {
    var positive:Wide;var negative:Wide;
    for(var i=0u;i<3u;i++) {
        let value=rgb[i];let coefficient=boundaries[514u+i];
        let term=wide(multiply(value.mantissa,coefficient),value.exponent+bitcast<i32>(coefficient.z)+181);
        if value.negative {negative=wide_add(negative,term);} else {positive=wide_add(positive,term);}
    }
    if wide_compare(positive,negative)>=0 {return wide_subtract(positive,negative);}
    var result=wide_subtract(negative,positive);result.negative=true;return result;
}
fn boundary_product(alpha:FloatNumber,b:vec4<u32>)->Wide {
    return wide(multiply(alpha.mantissa,b),alpha.exponent+bitcast<i32>(b.z)+181);
}
fn luminance_bin(value:Wide,alpha:FloatNumber)->u32 {
    if value.negative {return 0u;}
    var zero:Wide;if wide_compare(value,zero)==0 {return 0u;}
    var low=select(0u,1u,area.flags.w!=0u);var high=255u;
    while low<high {
        let middle=(low+high+1u)/2u;
        if wide_compare(value,boundary_product(alpha,boundaries[257u+middle]))>=0 {low=middle;} else {high=middle-1u;}
    }
    return low;
}
fn selected(world:vec2<u32>)->bool {
    if selection.info.y==0u {return true;}
    let p=vec2<i32>(floor(vec2<f32>(world)+vec2<f32>(0.5)-bitcast<vec2<f32>>(selection.info.zw)))-vec2<i32>(selection.rect.xy);
    var covered=false;
    if all(p>=vec2<i32>(0)) && all(p<vec2<i32>(selection.rect.zw)) {
        let bytes=selection.info.y==2u;let shift=select(3u,2u,bytes);let count=1u<<shift;
        let bits=select(4u,8u,bytes);let word=u32(p.y)*((selection.rect.z+count-1u)>>shift)+(u32(p.x)>>shift);
        covered=((selection.values[word]>>((u32(p.x)&(count-1u))*bits))&select(15u,255u,bytes))!=0u;
        if selection.info.x!=0u {
            return ((selection.values[word]>>((u32(p.x)&(count-1u))*bits))&select(15u,255u,bytes))<select(4u,255u,bytes);
        }
    }
    return covered || selection.info.x!=0u;
}
fn grid_begin(p:vec2<u32>)->vec2<u32> {
    let extent=area.flags.xy;let size=min(extent,vec2(256u));
    return min((2u*p*size+extent-1u)/(2u*extent),size);
}
@compute @workgroup_size(64)
fn count(@builtin(local_invocation_index) lane:u32,@builtin(workgroup_id) group:vec3<u32>) {
    let offset=group.x*1043u;
    for(var i=lane;i<1043u;i+=64u) {atomicStore(&counts[i],0u);}
    workgroupBarrier();
    var size=area.region.zw;var begin=vec2(0u);
    if area.flags.z!=0u {begin=grid_begin(area.region.xy);size=grid_begin(area.region.xy+area.region.zw)-begin;}
    var wave_keys=vec4(0u);var wave_counts=vec4(0u);
    for(var index=group.x*64u+lane;index<size.x*size.y;index+=4096u) {
        var world=area.region.xy+vec2(index%size.x,index/size.x);
        if area.flags.z!=0u {world=((2u*(begin+vec2(index%size.x,index/size.x))+1u)*area.flags.xy)/(2u*min(area.flags.xy,vec2(256u)));}
        let local=world-area.region.xy;
        if !selected(world) {continue;}
        let color=textureLoad(pixels,vec2<i32>(local),0);let bits=bitcast<vec4<u32>>(color);let absolute=bits&vec4<u32>(0x7fffffffu);
        if any(absolute>=vec4<u32>(0x7f800000u)) || absolute.a>0x3f800000u || (bits.a>>31u!=0u && absolute.a!=0u) {atomicOr(&counts[1042u],1u);continue;}
        if absolute.a==0u {atomicAdd(&counts[1041u],1u);continue;}
        atomicAdd(&counts[1040u],1u);
        let alpha=float_number(color.a);let rgb=array<FloatNumber,3>(float_number(color.r),float_number(color.g),float_number(color.b));
        let neutral=rgb[0].mantissa==rgb[1].mantissa && rgb[1].mantissa==rgb[2].mantissa
            && rgb[0].exponent==rgb[1].exponent && rgb[1].exponent==rgb[2].exponent
            && rgb[0].negative==rgb[1].negative && rgb[1].negative==rgb[2].negative;
        for(var channel=0u;channel<4u;channel++) {
            var classified:vec2<u32>;
            if channel==3u && !neutral {
                classified=approximate_luminance(rgb,alpha);
                if classified.x==256u {
                    atomicStore(&shards[PENDING+1u+atomicAdd(&shards[PENDING],1u)],index);continue;
                }
            } else {classified=classification(rgb[min(channel,2u)],alpha,channel);}
            atomicAdd(&counts[channel*256u+classified.x],1u);
            if area.waveform!=0u {
                let key=classified.x*256u+min(world.x*256u/area.flags.x,255u);
                if key!=wave_keys[channel] {waveform(wave_keys[channel],channel,group.x,wave_counts[channel]);wave_counts[channel]=0u;}
                wave_keys[channel]=key;wave_counts[channel]++;
            }
            for(var counter=0u;counter<4u;counter++) {if (classified.y&(1u<<counter))!=0u {atomicAdd(&counts[1024u+channel*4u+counter],1u);}}
        }
    }
    if area.waveform!=0u {for(var channel=0u;channel<4u;channel++) {waveform(wave_keys[channel],channel,group.x,wave_counts[channel]);}}
    workgroupBarrier();
    for(var i=lane;i<1043u;i+=64u) {let value=atomicLoad(&counts[i]);if value!=0u {atomicAdd(&shards[offset+i],value);}}
}
@compute @workgroup_size(64)
fn resolve(@builtin(local_invocation_index) lane:u32,@builtin(workgroup_id) group:vec3<u32>) {
    let offset=group.x*1043u;
    var size=area.region.zw;var begin=vec2(0u);
    if area.flags.z!=0u {begin=grid_begin(area.region.xy);size=grid_begin(area.region.xy+area.region.zw)-begin;}
    let total=atomicLoad(&shards[PENDING]);
    for(var i=group.x*64u+lane;i<total;i+=4096u) {
        let index=atomicLoad(&shards[PENDING+1u+i]);
        var world=area.region.xy+vec2(index%size.x,index/size.x);
        if area.flags.z!=0u {world=((2u*(begin+vec2(index%size.x,index/size.x))+1u)*area.flags.xy)/(2u*min(area.flags.xy,vec2(256u)));}
        let color=textureLoad(pixels,vec2<i32>(world-area.region.xy),0);
        let alpha=float_number(color.a);let value=luminance(array(float_number(color.r),float_number(color.g),float_number(color.b)));
        var zero:Wide;let black=value.negative || wide_compare(value,zero)==0;
        let relation=wide_compare(value,boundary_product(alpha,vec4<u32>(0u,0x100000u,0u,0u)));
        let flags=u32(value.negative)|(u32(!black && relation>0)<<1u)|(u32(black)<<2u)|(u32(!black && relation>=0)<<3u);
        let bin=luminance_bin(value,alpha);
        atomicAdd(&shards[offset+768u+bin],1u);
        if area.waveform!=0u {waveform(bin*256u+min(world.x*256u/area.flags.x,255u),3u,group.x,1u);}
        for(var counter=0u;counter<4u;counter++) {if (flags&(1u<<counter))!=0u {atomicAdd(&shards[offset+1036u+counter],1u);}}
    }
}
@compute @workgroup_size(64)
fn fold(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x>=1043u {
        let index=id.x-1043u;
        if area.waveform==0u || index>=WAVE_WORDS {return;}
        var count=0u;
        for(var shard=0u;shard<8u;shard++) {count+=atomicLoad(&shards[WAVE_OFFSET+shard*WAVE_WORDS+index]);}
        summary[id.x]=count;return;
    }
    var count=0u;
    for(var shard=0u;shard<64u;shard++) {count+=atomicLoad(&shards[shard*1043u+id.x]);}
    summary[id.x]=count;
}
