const END_WORDS:u32=20u;
const HEADER_WORDS:u32=5u;
struct LineParams {
    extent: vec4<u32>,
    window: vec4<u32>,
    core: vec4<u32>,
    offsets: vec4<u32>,
    options: vec4<u32>,
    paper: vec4<f32>,
    job: vec4<u32>,
}
@group(0) @binding(1) var<uniform> line: LineParams;
@group(0) @binding(2) var<storage, read_write> first: array<u32>;
@group(0) @binding(3) var<storage, read_write> second: array<u32>;
@group(0) @binding(4) var<storage, read_write> cached: array<atomic<u32>>;
@group(0) @binding(5) var<storage, read_write> eligible: array<atomic<u32>>;
@group(0) @binding(6) var paper_height: texture_2d<f32>;
const INVALID = 0xffffffffu;
const AROUND = array<vec2<i32>,8>(vec2(0,-1),vec2(1,-1),vec2(1,0),vec2(1,1),vec2(0,1),vec2(-1,1),vec2(-1,0),vec2(-1,-1));
fn coord(value: u32) -> vec2<u32> { return vec2(value & 65535u,value >> 16u); }
fn point(p: vec2<u32>) -> u32 { return p.x | (p.y << 16u); }
fn valid(p: vec2<i32>) -> bool { return all(p >= vec2<i32>(0)) && all(p < vec2<i32>(line.extent.xy)); }
fn raw_ink(p: vec2<i32>) -> bool {
    if !valid(p) { return false; }
    let i=u32(p.y)*line.extent.z+u32(p.x)/32u;
    return (atomicLoad(&eligible[i]) & (1u << (u32(p.x)%32u))) == 0u;
}
fn active_barrier(p:vec2<i32>) -> bool {
    if !valid(p){return false;}
    let base=line.offsets.z+line.offsets.w*END_WORDS;
    return (atomicLoad(&cached[base+u32(p.y)*line.extent.z+u32(p.x)/32u]) & (1u<<(u32(p.x)%32u))) != 0u;
}
fn ink(p: vec2<i32>) -> bool {
    if !valid(p) { return false; }
    return (atomicLoad(&cached[u32(p.y)*line.extent.z+u32(p.x)/32u]) & (1u << (u32(p.x)%32u))) != 0u;
}
fn distance(p: vec2<i32>) -> u32 {
    if !valid(p) { return 0u; }
    return max(2u,(atomicLoad(&cached[line.offsets.x+u32(p.y)*line.extent.w+u32(p.x)/4u]) >> ((u32(p.x)%4u)*8u)) & 255u);
}
fn scaled_fraction(numerator:u32,denominator:u32,multiplier:u32)->u32 {
    var quotient=0u;var remainder=0u;
    for(var bit=i32(31u-countLeadingZeros(multiplier));bit>=0;bit--){
        quotient*=2u;
        if remainder>=denominator-remainder {remainder-=denominator-remainder;quotient++;}else{remainder*=2u;}
        if (multiplier&(1u<<u32(bit)))!=0u {
            if remainder>=denominator-numerator {remainder-=denominator-numerator;quotient++;}else{remainder+=numerator;}
        }
    }
    return quotient;
}
fn paper_axis(value:u32,offset:u32,period:u32,size:u32)->u32 {
    var at=value%period;let shift=offset%period;
    if at>=period-shift {at-=period-shift;}else{at+=shift;}
    return scaled_fraction(at,period,size*256u)+(size*128u)/period;
}
fn paper_texel(p:vec2<i32>,size:vec2<i32>)->u32 {
    let at=(p%size+size)%size;
    return u32(round(textureLoad(paper_height,at,0).r*255.));
}
fn height(p:vec2<i32>)->u32 {
    let size=vec2<i32>(textureDimensions(paper_height));let period=max(vec2<u32>(1),vec2<u32>(line.paper.xy));
    let fixed=vec2<i32>(vec2<u32>(paper_axis(u32(p.x),u32(line.paper.z),period.x,u32(size.x)),paper_axis(u32(p.y),u32(line.paper.w),period.y,u32(size.y))))-vec2<i32>(128);
    let texel=fixed>>vec2<u32>(8);let fraction=vec2<u32>(fixed-texel*256);
    let top=paper_texel(texel,size)*(256u-fraction.x)+paper_texel(texel+vec2(1,0),size)*fraction.x;
    let bottom=paper_texel(texel+vec2(0,1),size)*(256u-fraction.x)+paper_texel(texel+vec2(1,1),size)*fraction.x;
    return (top*(256u-fraction.y)+bottom*fraction.y+128u)>>8u;
}
fn grain(p: vec2<i32>) -> bool {
    if raw_ink(p) { return true; }
    if line.options.y==0u {return raw_ink(p+AROUND[0]) && raw_ink(p+AROUND[2]) && raw_ink(p+AROUND[4]) && raw_ink(p+AROUND[6]);}
    for(var axis=0u;axis<4u;axis++) {
        let d=AROUND[axis];
        var a=0u;var b=0u;
        for(var step=1u;step<=line.options.x;step++) {
            if a==0u && raw_ink(p+d*i32(step)) { a=step; }
            if b==0u && raw_ink(p-d*i32(step)) { b=step; }
        }
        if a!=0u && b!=0u && height(p)<min(height(p+d*i32(a)),height(p-d*i32(b))) { return true; }
    }
    return false;
}
@compute @workgroup_size(64)
fn grain_bits(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups:vec3<u32>) {
    let index=id.x+id.y*groups.x*64u+line.job.x;
    let p=vec2<u32>(index%line.core.z,index/line.core.z);
    if p.y>=line.core.w {return;}
    let q=line.core.xy+p;
    let word=q.y*line.extent.z+q.x/32u;let bit=1u<<(q.x%32u);
    if grain(vec2<i32>(q)) {atomicOr(&cached[word],bit);}else{atomicAnd(&cached[word],~bit);}
}
fn line_block(index:u32,bounds:vec4<u32>,barrier:bool)->vec3<u32> {
    let left=bounds.x/32u;let width=(bounds.x+bounds.z+31u)/32u-left;
    let x=left+index%width;let y=bounds.y+index/width;
    if y>=bounds.y+bounds.w {return vec3<u32>(0);}
    let base=select(0u,line.offsets.z+line.offsets.w*END_WORDS,barrier);
    return vec3(x*32u,y,atomicLoad(&cached[base+y*line.extent.z+x]));
}
fn ink_word(x:i32,y:i32)->u32 {
    if x<0 || x>=i32(line.extent.z) || y<0 || y>=i32(line.extent.y) {return 0u;}
    return atomicLoad(&cached[u32(y)*line.extent.z+u32(x)]);
}
fn skeleton_block(index:u32)->vec3<u32> {
    let block=line_block(index,line.window,false);
    if block.z==0u {return block;}
    let x=i32(block.x/32u);let y=i32(block.y);let centre=block.z;
    let west=(centre<<1u)|(ink_word(x-1,y)>>31u);
    let east=(centre>>1u)|(ink_word(x+1,y)<<31u);
    let bits=centre|(ink_word(x,y-1)&ink_word(x,y+1)&west&east);
    return vec3(block.xy,bits);
}
fn local_point(i: u32) -> vec2<u32> { return vec2(i%line.window.z,i/line.window.z); }
fn local_index(p: vec2<u32>) -> u32 { return p.y*line.window.z+p.x; }
fn input(i: u32) -> u32 { if line.options.w==0u {return first[i];}return second[i]; }
fn output(i: u32,value: u32) {if line.options.w==0u {second[i]=value;}else{first[i]=value;}}
fn square(a: vec2<u32>,b: vec2<u32>) -> u32 {let d=vec2<i32>(a)-vec2<i32>(b);return u32(d.x*d.x+d.y*d.y);}
fn root(value: u32) -> u32 {
    var x=value;var result=0u;var bit=1u<<30u;
    loop {if bit<=x {break;}bit>>=2u;}
    loop {if bit==0u {break;}if x>=result+bit {x-=result+bit;result=(result>>1u)+bit;}else{result>>=1u;}bit>>=2u;}
    return result;
}
@compute @workgroup_size(64)
fn distance_seeds(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups:vec3<u32>) {
    let index=id.x+id.y*groups.x*64u+line.job.x;
    let p=local_point(index);if p.y>=line.window.w {return;}
    let q=line.window.xy+p;
    let value=select(INVALID,point(q),!ink(vec2<i32>(q)));first[index]=value;second[index]=value;
}
fn jump_point(p:vec2<u32>) {
    let index=local_index(p);
    let q=line.window.xy+p;if line.options.y==0u && !ink(vec2<i32>(q)){return;}
    var best=input(index);var cost=0xffffffffu;
    if best==point(q){if line.options.y!=4u{output(index,best);}return;}
    if best!=INVALID {cost=square(q,coord(best));}
    for(var y=-1;y<=1;y++){for(var x=-1;x<=1;x++){
        let at=vec2<i32>(p)+vec2(x,y)*i32(line.options.z);
        if any(at<vec2<i32>(0)) || any(at>=vec2<i32>(line.window.zw)) {continue;}
        let candidate=input(local_index(vec2<u32>(at)));if candidate==INVALID {continue;}
        let d=square(q,coord(candidate));if d<cost || (d==cost && candidate<best) {cost=d;best=candidate;}
    }}
    output(index,best);
}
@compute @workgroup_size(64)
fn jump_distance(@builtin(global_invocation_id) id:vec3<u32>,@builtin(num_workgroups) groups:vec3<u32>) {
    if line.options.y==4u && atomicLoad(&cached[line.offsets.y+3u])==0u {return;}
    let index=id.x+id.y*groups.x*64u+line.job.x;
    if line.options.y!=0u && line.options.y!=4u {
        let p=local_point(index);if p.y<line.window.w {jump_point(p);}return;
    }
    let block=line_block(index,line.window,line.options.y==4u);var bits=block.z;
    loop {
        if bits==0u {break;}let bit=countTrailingZeros(bits);bits&=bits-1u;
        let q=block.xy+vec2(bit,0u);
        if all(q>=line.window.xy) && all(q<line.window.xy+line.window.zw) {jump_point(q-line.window.xy);}
    }
}

@compute @workgroup_size(64)
fn store_distance(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups:vec3<u32>) {
    let index=id.x+id.y*groups.x*64u+line.job.x;
    let p=vec2(index%line.core.z,index/line.core.z);if p.y>=line.core.w{return;}
    let q=line.core.xy+p;
    var d=0u;
    if ink(vec2<i32>(q)) {
        let nearest=input(local_index(q-line.window.xy));d=line.options.x;
        if nearest!=INVALID {d=min(line.options.x,root(4u*square(q,coord(nearest))));}
    }
    let word=line.offsets.x+q.y*line.extent.w+q.x/4u;let shift=(q.x%4u)*8u;
    atomicAnd(&cached[word],~(255u<<shift));atomicOr(&cached[word],d<<shift);
}
fn initial_skeleton(q:vec2<i32>)->bool {
    let solid=ink(q) || (ink(q+AROUND[0]) && ink(q+AROUND[2]) && ink(q+AROUND[4]) && ink(q+AROUND[6]));
    return solid && distance(q)<=32u;
}
@compute @workgroup_size(64)
fn skeleton_seeds(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups:vec3<u32>) {
    if line.job.y!=0u && atomicLoad(&cached[line.offsets.y+3u])==0u{return;}
    let index=id.x+id.y*groups.x*64u+line.job.x;
    let p=local_point(index);if p.y>=line.window.w{return;}
    let q=vec2<i32>(line.window.xy+p);
    let value=u32(initial_skeleton(q));first[index]=value;second[index]=value;
}
fn skeleton(p: vec2<i32>) -> u32 {
    if any(p<vec2<i32>(0)) || any(p>=vec2<i32>(line.window.zw)){return 0u;}
    return input(local_index(vec2<u32>(p)));
}
fn skeleton_ridges_point(p:vec2<u32>) {
    let index=local_index(p);
    let q=line.window.xy+p;if !initial_skeleton(vec2<i32>(q)){return;}
    var value=input(index);
    if value!=0u && ((q.x&1u)|((q.y&1u)<<1u))==line.options.z && distance(vec2<i32>(q))<=line.options.x {
        var around:array<u32,8>;var count=0u;var crossings=0u;
        for(var i=0u;i<8u;i++){around[i]=skeleton(vec2<i32>(p)+AROUND[i]);count+=around[i];}
        for(var i=0u;i<8u;i++){crossings+=u32(around[i]==0u && around[(i+1u)%8u]!=0u);}
        if count>=2u && crossings==1u && around[0]*around[2]*around[4]*around[6]==0u {value=0u;}
    }
    output(index,value);
}
@compute @workgroup_size(64)
fn skeleton_ridges(@builtin(global_invocation_id) id:vec3<u32>,@builtin(num_workgroups) groups:vec3<u32>) {
    if line.job.y!=0u && atomicLoad(&cached[line.offsets.y+3u])==0u {return;}
    let block=skeleton_block(id.x+id.y*groups.x*64u+line.job.x);var bits=block.z;
    loop {
        if bits==0u {break;}let bit=countTrailingZeros(bits);bits&=bits-1u;
        let q=block.xy+vec2(bit,0u);
        if all(q>=line.window.xy) && all(q<line.window.xy+line.window.zw) {skeleton_ridges_point(q-line.window.xy);}
    }
}

fn connected(p:vec2<i32>,d:vec2<i32>)->bool {
    return skeleton(p+d)!=0u && (d.x==0 || d.y==0 || (skeleton(p+vec2(d.x,0))==0u && skeleton(p+vec2(0,d.y))==0u));
}
fn crossing_number(p: vec2<i32>) -> u32 {
    var count=0u;for(var i=0u;i<8u;i++){count+=u32(!connected(p,AROUND[i]) && connected(p,AROUND[(i+1u)%8u]));}return count;
}
fn next_pixel(p:vec2<i32>,previous:vec2<i32>) -> vec2<i32> {
    var best=vec2<i32>(-1);var rank=0x7fffffffi;
    for(var i=0u;i<8u;i++){
        let q=p+AROUND[i];if all(q==previous) || !connected(p,AROUND[i]) {continue;}
        let cost=-select(0,i32(square(vec2<u32>(q),vec2<u32>(previous)))*8,all(previous>=vec2<i32>(0)))+i32(i);
        if cost<rank {rank=cost;best=q;}
    }
    return best;
}
fn branch_end(at:vec2<i32>,previous:vec2<i32>,limit:u32,stop_at_junction:bool)->vec4<u32> {
    var p=at;var prior=previous;var steps=1u;var crossings=0u;
    loop {
        crossings=crossing_number(p);
        if crossings<2u || (stop_at_junction && crossings>2u) || steps>=limit {break;}
        let next=next_pixel(p,prior);if any(next<vec2<i32>(0)){crossings=1u;break;}
        prior=p;p=next;steps++;
    }
    return vec4(point(vec2<u32>(p)),point(vec2<u32>(prior)),steps,crossings);
}
@compute @workgroup_size(64)
fn skeleton_prune(@builtin(global_invocation_id) id:vec3<u32>,@builtin(num_workgroups) groups:vec3<u32>) {
    if line.job.y!=0u && atomicLoad(&cached[line.offsets.y+3u])==0u{return;}
    let index=id.x+id.y*groups.x*64u+line.job.x;let p=local_point(index);if p.y>=line.window.w{return;}
    if !initial_skeleton(vec2<i32>(line.window.xy+p)){return;}
    var value=input(index);if value==0u{output(index,0u);return;}
    let at=vec2<i32>(p);let crossings=crossing_number(at);
    if crossings<=2u {
        var endpoint=crossings==1u;var junction=vec4<u32>(0);var length=0u;
        for(var i=0u;i<8u;i++) {
            if !connected(at,AROUND[i]){continue;}
            let end=branch_end(at+AROUND[i],at,32u,true);length+=end.z;
            endpoint=endpoint || end.w==1u;if end.w>2u{junction=end;}
        }
        if endpoint && junction.w>2u {
            let q=vec2<i32>(coord(junction.x));let previous=vec2<i32>(coord(junction.y));
            let width=max(1u,distance(vec2<i32>(line.window.xy)+q));var longer=0u;
            if length<2u*width {
                for(var i=0u;i<8u;i++) {
                    let next=q+AROUND[i];if all(next==previous) || !connected(q,AROUND[i]){continue;}
                    let end=branch_end(next,q,length,false);longer+=u32(end.z>=length);
                }
                if longer>=2u{value=0u;}
            }
        }
    }
    output(index,value);
}
@compute @workgroup_size(64)
fn line_ends(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups:vec3<u32>) {
    if line.job.y!=0u && atomicLoad(&cached[line.offsets.y+3u])==0u{return;}
    let index=id.x+id.y*groups.x*64u+line.job.x;
    let p=local_point(index);if p.y>=line.window.w{return;}
    let q=line.window.xy+p;
    if all(q>=line.core.xy) && all(q<line.core.xy+line.core.zw) {
        let word=line.offsets.z+line.offsets.w*END_WORDS+q.y*line.extent.z+q.x/32u;let bit=1u<<(q.x%32u);
        if skeleton(vec2<i32>(p))!=0u {atomicOr(&cached[word],bit);atomicOr(&cached[word+line.offsets.x],bit);}
        else{atomicAnd(&cached[word],~bit);atomicAnd(&cached[word+line.offsets.x],~bit);}
    }
    if any(q<line.core.xy) || any(q>=line.core.xy+line.core.zw) || skeleton(vec2<i32>(p))==0u || crossing_number(vec2<i32>(p))!=1u {return;}
    var path:array<vec2<i32>,33>;path[0]=vec2<i32>(p);
    var at=vec2<i32>(p);var previous=vec2<i32>(-1);var length=0u;var used=0u;
    var width=max(1u,distance(vec2<i32>(q))-1u);
    for(var step=0u;step<32u;step++){
        let next=next_pixel(at,previous);if any(next<vec2<i32>(0)){break;}
        previous=at;at=next;length++;path[length]=at;used=length;
        width=max(width,distance(vec2<i32>(line.window.xy)+at)-1u);
        if crossing_number(at)>2u {break;}
    }
    if used<1u {return;}
    width=min(width,line.options.y);
    let axis=end_axis(&path,used+1u);let position=vec2<i32>(q)+axis[0];let delta=axis[1];
    if !valid(position) || all(delta==vec2<i32>(0)) {return;}
    let slot=atomicAdd(&cached[line.offsets.y],1u);
    if slot>=line.offsets.w {atomicStore(&cached[line.offsets.y+1u],1u);return;}
    let base=line.offsets.z+slot*END_WORDS;
    atomicStore(&cached[base],point(vec2<u32>(position)));
    atomicStore(&cached[base+1u],(u32(delta.x)&65535u)|(u32(delta.y)<<16u));
    atomicStore(&cached[base+2u],width|(length<<16u));
    atomicStore(&cached[base+11u],1u);atomicStore(&cached[base+15u],point(q));
    let page=(q.y/128u)*((line.extent.x+127u)/128u)+q.x/128u;
    atomicStore(&cached[base+3u],atomicExchange(&cached[line.offsets.y+HEADER_WORDS+page],slot+1u));
}
fn tangent(value:u32)->vec2<i32>{return vec2<i32>(i32(value<<16u)>>16,i32(value)>>16);}
fn middle(p:vec2<i32>)->vec2<i32>{
    var q=p;
    for(var step=0u;step<16u;step++){
        var best=q;var d=distance(q);
        for(var n=0u;n<8u;n++){let at=q+AROUND[n];let value=distance(at);if ink(at) && value>d{d=value;best=at;}}
        if all(best==q){break;}q=best;
    }
    return q;
}
fn required_reach(squared:u32,allowance:u32)->u32 {
    let d=root(squared);let length=d+u32(d*d<squared);let gap=max(1u,length-min(length,allowance));
    return 16u*min(4u,(gap+15u)/16u);
}
fn nearby_own_stroke(start:vec2<i32>,hit:vec2<i32>,reach:u32)->bool {
    var nodes:array<vec2<u32>,256>;nodes[0]=vec2(point(vec2<u32>(start)),0u);var head=0u;var tail=1u;
    let limit=3u*reach;
    loop {
        if head==tail {break;}
        var best=head;var rank=0xffffffffu;
        for(var k=head;k<tail;k++){
            let delta=abs(vec2<i32>(coord(nodes[k].x))-hit);let cost=nodes[k].y+u32(max(0,max(delta.x,delta.y)-2));
            if cost<rank || (cost==rank && nodes[k].x<nodes[best].x) {best=k;rank=cost;}
        }
        let swap=nodes[head];nodes[head]=nodes[best];nodes[best]=swap;
        let node=nodes[head];head++;let at=vec2<i32>(coord(node.x));
        if square(vec2<u32>(at),vec2<u32>(hit))<=4u{return true;}if node.y>=limit{continue;}
        for(var n=0u;n<8u;n++){
            let q=at+AROUND[n];let left=i32(limit-node.y-1u);if !active_barrier(q) || max(abs(q.x-hit.x),abs(q.y-hit.y))>left+2 {continue;}let key=point(vec2<u32>(q));
            var visited=INVALID;for(var k=0u;k<tail;k++){if key==nodes[k].x{visited=k;break;}}
            if visited!=INVALID {if visited>=head{nodes[visited].y=min(nodes[visited].y,node.y+1u);}continue;}
            if tail==256u{return false;}nodes[tail]=vec2(key,node.y+1u);tail++;
        }
    }
    return false;
}
const RAYS=array<vec2<i32>,41>(vec2<i32>(12551,-10531),vec2<i32>(12911,-10087),vec2<i32>(13255,-9630),vec2<i32>(13583,-9162),vec2<i32>(13894,-8682),vec2<i32>(14189,-8192),vec2<i32>(14466,-7692),vec2<i32>(14726,-7182),vec2<i32>(14968,-6664),vec2<i32>(15191,-6138),vec2<i32>(15396,-5604),vec2<i32>(15582,-5063),vec2<i32>(15749,-4516),vec2<i32>(15897,-3964),vec2<i32>(16026,-3406),vec2<i32>(16135,-2845),vec2<i32>(16225,-2280),vec2<i32>(16294,-1713),vec2<i32>(16344,-1143),vec2<i32>(16374,-572),vec2<i32>(16384,0),vec2<i32>(16374,572),vec2<i32>(16344,1143),vec2<i32>(16294,1713),vec2<i32>(16225,2280),vec2<i32>(16135,2845),vec2<i32>(16026,3406),vec2<i32>(15897,3964),vec2<i32>(15749,4516),vec2<i32>(15582,5063),vec2<i32>(15396,5604),vec2<i32>(15191,6138),vec2<i32>(14968,6664),vec2<i32>(14726,7182),vec2<i32>(14466,7692),vec2<i32>(14189,8192),vec2<i32>(13894,8682),vec2<i32>(13583,9162),vec2<i32>(13255,9630),vec2<i32>(12911,10087),vec2<i32>(12551,10531));
fn determinant(a:vec2<i32>,b:vec2<i32>)->i32{return a.x*b.y-a.y*b.x;}
fn in_cone(a:vec2<i32>,b:vec2<i32>)->bool {let forward=dot(a,b);return forward>0 && abs(determinant(a,b))*1000<=forward*839;}
fn gap_on_segment(a:vec2<i32>,b:vec2<i32>)->bool {
    let d=b-a;let steps=max(abs(d.x),abs(d.y));
    for(var i=1;i<steps;i++){if !ink(a+rounded_div(d*i,max(1,steps))){return true;}}
    return false;
}
fn rounded_div(value:vec2<i32>,divisor:i32)->vec2<i32>{return (value+sign(value)*(divisor/2))/divisor;}
fn path_axis(path:ptr<function,array<vec2<i32>,33>>,start:u32,count:u32)->array<vec2<i32>,2> {
    var sum=vec2<i32>(0);var weighted=vec2<i32>(0);let time=i32(count*(count-1u)/2u);
    for(var i=0u;i<count;i++){let p=(*path)[start+i]-(*path)[start];sum+=p;weighted+=p*i32(i);}
    let numerator=weighted*i32(count)-sum*time;let scale=max(abs(numerator.x),abs(numerator.y));
    if scale==0 {return array<vec2<i32>,2>(vec2<i32>(0),vec2<i32>(0));}
    let direction=rounded_div(numerator*64,scale);let centre=rounded_div(sum*256,i32(count));
    let offset=centre+rounded_div(direction*dot(-centre,direction),dot(direction,direction));
    return array<vec2<i32>,2>(rounded_div(offset,256),-direction);
}
fn axis_cost(path:ptr<function,array<vec2<i32>,33>>,start:u32,count:u32,axis:array<vec2<i32>,2>)->u32 {
    let centre=(*path)[start]+axis[0];let divisor=max(1,dot(axis[1],axis[1]));var cost=0u;
    for(var i=0u;i<count;i++){let residual=determinant((*path)[start+i]-centre,axis[1]);cost+=u32(residual*residual/divisor);}
    return cost;
}
fn end_axis(path:ptr<function,array<vec2<i32>,33>>,count:u32)->array<vec2<i32>,2> {
    var best=path_axis(path,0u,count);var cost=axis_cost(path,0u,count,best);
    for(var split=3u;split+3u<count;split++){
        let first=path_axis(path,0u,split+1u);let second=path_axis(path,split,count-split);let forward=dot(first[1],second[1]);
        if forward>0 && abs(determinant(first[1],second[1]))<=forward {continue;}
        let candidate=axis_cost(path,0u,split+1u,first)+axis_cost(path,split,count-split,second)+16u;
        if candidate<cost{cost=candidate;best=first;}
    }
    return best;
}
fn end_width(base:u32)->u32{return atomicLoad(&cached[base+2u])&65535u;}
@compute @workgroup_size(64)
fn refine_ends(@builtin(global_invocation_id) id:vec3<u32>,@builtin(num_workgroups) groups:vec3<u32>){
    if line.job.y!=0u && atomicLoad(&cached[line.offsets.y+3u])==0u{return;}
    let index=id.x+id.y*groups.x*64u+line.job.x;
    let count=min(atomicLoad(&cached[line.offsets.y]),line.offsets.w);if index>=count{return;}
    let base=line.offsets.z+index*END_WORDS;let start=vec2<i32>(coord(atomicLoad(&cached[base])));let original=tangent(atomicLoad(&cached[base+1u]));
    if line.options.y==1u && atomicLoad(&cached[base+11u])==0u {return;}
    var direction=original;var best=0xffffffffu;var key=INVALID;
    if atomicLoad(&cached[base+2u])>>16u<16u {
        let lower=vec2<u32>(max(start-vec2<i32>(64),vec2<i32>(0)))/128u;
        let upper=vec2<u32>(min(start+vec2<i32>(64),vec2<i32>(line.extent.xy)-vec2<i32>(1)))/128u;
        for(var y=lower.y;y<=upper.y;y++){for(var x=lower.x;x<=upper.x;x++){
            var link=atomicLoad(&cached[line.offsets.y+HEADER_WORDS+y*((line.extent.x+127u)/128u)+x]);
            loop {
                if link==0u{break;}let other=line.offsets.z+(link-1u)*END_WORDS;link=atomicLoad(&cached[other+3u]);
                if atomicLoad(&cached[other+2u])>>16u<16u {continue;}
                let candidate=atomicLoad(&cached[other]);let at=vec2<i32>(coord(candidate));let axis=tangent(atomicLoad(&cached[other+1u]));let delta=start-at;
                let forward=dot(axis,original);let aligned=select(-axis,axis,forward>0 || (forward==0 && determinant(axis,original)>0));
                if square(vec2<u32>(start),vec2<u32>(at))>64u*64u || !in_cone(axis,delta) || (atomicLoad(&cached[base+2u])>>16u>=3u && !in_cone(aligned,original)){continue;}
                let score=square(vec2<u32>(start),vec2<u32>(at))*4096u+u32(abs(determinant(axis,delta)))*16u;
                if score<best || (score==best && candidate<key) {best=score;key=candidate;direction=aligned;}
            }
        }}
    }
    atomicStore(&cached[base+7u],(u32(direction.x)&65535u)|(u32(direction.y)<<16u));
}
fn supported_hit(p:vec2<i32>)->bool {
    if distance(p)>line.options.x{return true;}
    for(var n=0u;n<8u;n++){if active_barrier(p+AROUND[n]){return true;}}
    return false;
}
@compute @workgroup_size(64)
fn seal_candidates(@builtin(global_invocation_id) id:vec3<u32>, @builtin(num_workgroups) groups:vec3<u32>){
    if line.job.y!=0u && atomicLoad(&cached[line.offsets.y+3u])==0u{return;}
    let index=id.x+id.y*groups.x*64u+line.job.x;
    let count=min(atomicLoad(&cached[line.offsets.y]),line.offsets.w);if index>=count{return;}
    let base=line.offsets.z+index*END_WORDS;let start=vec2<i32>(coord(atomicLoad(&cached[base])));let dir=tangent(atomicLoad(&cached[base+7u]));
    if line.options.y==1u && atomicLoad(&cached[base+11u])==0u {return;}
    var best:array<u32,4>;var scores:array<u32,4>;var bends:array<u32,4>;
    for(var bucket=0u;bucket<4u;bucket++){best[bucket]=INVALID;scores[bucket]=0xffffffffu;bends[bucket]=INVALID;}
    let lower=vec2<u32>(max(start-vec2<i32>(80),vec2<i32>(0)))/128u;
    let upper=vec2<u32>(min(start+vec2<i32>(80),vec2<i32>(line.extent.xy)-vec2<i32>(1)))/128u;
    for(var y=lower.y;y<=upper.y;y++){for(var x=lower.x;x<=upper.x;x++){
        var link=atomicLoad(&cached[line.offsets.y+HEADER_WORDS+y*((line.extent.x+127u)/128u)+x]);
        loop {
            if link==0u {break;}
            let other=line.offsets.z+(link-1u)*END_WORDS;let same=link-1u==index;link=atomicLoad(&cached[other+3u]);if same{continue;}
            let key=atomicLoad(&cached[other]);let end=vec2<i32>(coord(key));let d=end-start;
            if square(vec2<u32>(start),vec2<u32>(end))>64u*64u {continue;}
            let facing=tangent(atomicLoad(&cached[other+7u]));var denominator=determinant(dir,facing);var a=determinant(d,facing);var b=determinant(d,dir);
            if denominator<0 {denominator=-denominator;a=-a;b=-b;}
            var bend=INVALID;var length=0u;
            let straight=in_cone(dir,d) && in_cone(facing,-d);
            if denominator!=0 && (!straight || !in_cone(dir,-facing))
                && a*64>= -denominator*i32(end_width(base)) && b*64>= -denominator*i32(end_width(other)) {
                let q=start+rounded_div(dir*a,denominator);
                if valid(q) {
                    length=root(square(vec2<u32>(start),vec2<u32>(q)))+root(square(vec2<u32>(end),vec2<u32>(q)));
                    if length<=64u && (gap_on_segment(start,q)||gap_on_segment(q,end)){bend=point(vec2<u32>(q));}
                }
            }
            if bend==INVALID {
                if !straight || !gap_on_segment(start,end){continue;}
                length=root(square(vec2<u32>(start),vec2<u32>(end)));
            }
            if nearby_own_stroke(start,end,required_reach(square(vec2<u32>(start),vec2<u32>(end)),(end_width(base)+end_width(other))/2u)){continue;}
            var alignment=0u;
            if bend==INVALID {alignment=u32(abs(determinant(dir,d))*1024/max(1,dot(dir,d))+abs(determinant(facing,-d))*1024/max(1,dot(facing,-d)));}
            let score=(length*length+alignment*4u)*2048u;
            for(var bucket=0u;bucket<4u;bucket++){
                let reach=16u*(bucket+1u)+(end_width(base)+end_width(other))/2u;
                if square(vec2<u32>(start),vec2<u32>(end))<=reach*reach && (score<scores[bucket] || (score==scores[bucket] && (key<best[bucket] || (key==best[bucket] && bend<bends[bucket])))) {scores[bucket]=score;best[bucket]=key;bends[bucket]=bend;}
            }
        }
    }}
    for(var ray=0u;ray<41u;ray++){
        let rotation=RAYS[ray];let v=vec2(dir.x*rotation.x-dir.y*rotation.y,dir.x*rotation.y+dir.y*rotation.x);
        let divisor=max(abs(v.x),abs(v.y));if divisor==0{continue;}
        var free=false;
        for(var step=1;step<=80;step++){
            let q=start+rounded_div(v*step,divisor);let d=q-start;let length=u32(d.x*d.x+d.y*d.y);if length>(64u+end_width(base))*(64u+end_width(base)){break;}
            if !ink(q){free=true;continue;}
            if !free{continue;}
            if !in_cone(dir,d){break;}
            let hit=middle(q);if !supported_hit(hit){continue;}let key=point(vec2<u32>(hit));let score=0x80000000u+root(length*65536u)*(100u+3u*u32(abs(i32(ray)-20)));
            if nearby_own_stroke(start,hit,required_reach(length,end_width(base))){break;}
            for(var bucket=0u;bucket<4u;bucket++){
                let reach=16u*(bucket+1u);
                if length<=(reach+end_width(base))*(reach+end_width(base)) && (score<scores[bucket] || (score==scores[bucket] && key<best[bucket])){scores[bucket]=score;best[bucket]=key;bends[bucket]=INVALID;}
            }
            break;
        }
    }
    for(var bucket=0u;bucket<4u;bucket++){
        atomicStore(&cached[base+4u+bucket*4u],best[bucket]);atomicStore(&cached[base+5u+bucket*4u],scores[bucket]);atomicStore(&cached[base+6u+bucket*4u],bends[bucket]);
    }
}
fn active_line_attempt()->bool{return line.job.w!=0u || line.job.y==0u || first[line.job.z+5u]==0u;}
fn clear_open(p:vec2<i32>){if valid(p){atomicAnd(&eligible[u32(p.y)*line.extent.z+u32(p.x)/32u],~(1u<<(u32(p.x)%32u)));}}
@compute @workgroup_size(64)
fn apply_seals(@builtin(global_invocation_id) id:vec3<u32>, @builtin(num_workgroups) groups:vec3<u32>){
    if !active_line_attempt(){return;}
    let index=id.x+id.y*groups.x*64u+line.job.x;
    let count=min(atomicLoad(&cached[line.offsets.y]),line.offsets.w);if index>=count{return;}
    let base=line.offsets.z+index*END_WORDS;
    let hit=atomicLoad(&cached[base+4u+line.options.z*4u]);
    if hit==INVALID || atomicLoad(&cached[base+19u])!=0u{return;}
    let start=vec2<i32>(coord(atomicLoad(&cached[base])));
    if line.core.z!=0u && (any(start<vec2<i32>(line.core.xy)) || any(start>=vec2<i32>(line.core.xy+line.core.zw))){return;}
    let end=vec2<i32>(coord(hit));let bend=atomicLoad(&cached[base+6u+line.options.z*4u]);
    let write=line.options.w==0u;
    var touches=false;
    if bend!=INVALID {touches=seal_segment(start,vec2<i32>(coord(bend)),write);touches=seal_segment(vec2<i32>(coord(bend)),end,write)||touches;}
    else{touches=seal_segment(start,end,write);}
    if !write && touches {
        let at=line.options.w-1u+index*5u;
        first[at]=atomicLoad(&cached[base+15u]);first[at+1u]=point(vec2<u32>(start));
        first[at+2u]=hit;first[at+3u]=bend;first[at+4u]=16u*(line.options.z+1u);
    }
}
fn seal_segment(start:vec2<i32>,end:vec2<i32>,write:bool)->bool {
    var p=start;let delta=abs(end-p);let step=select(vec2(-1),vec2(1),p<end);var error=delta.x-delta.y;
    loop{
        if write {clear_open(p);}else{
            for(var axis=0u;axis<8u;axis+=2u){let q=p+AROUND[axis];if valid(q) && !raw_ink(q){return true;}}
        }
        if all(p==end){break;}let twice=2*error;
        if twice> -delta.y{error-=delta.y;p.x+=step.x;}
        if twice<delta.x{error+=delta.x;p.y+=step.y;}
    }
    return false;
}
@compute @workgroup_size(64)
fn open_bits(@builtin(global_invocation_id) id:vec3<u32>, @builtin(num_workgroups) groups:vec3<u32>){
    let index=id.x+id.y*groups.x*64u+line.job.x;
    let active_now=active_line_attempt();if index==0u{atomicStore(&cached[line.offsets.y+3u],u32(active_now));}
    if !active_now{return;}
    if index<line.offsets.x{atomicStore(&eligible[index],~atomicLoad(&cached[index]));}}
@compute @workgroup_size(64)
fn region_seeds(@builtin(global_invocation_id) id:vec3<u32>, @builtin(num_workgroups) groups:vec3<u32>){
    let index=id.x+id.y*groups.x*64u+line.job.x;
    if line.options.y==5u {
        let left=line.window.x/32u;let width=(line.window.x+line.window.z+31u)/32u-left;
        let y=index/width;if y>=line.window.w{return;}
        let word=(line.window.y+y)*line.extent.z+left+index%width;
        let barrier=line.offsets.z+line.offsets.w*END_WORDS;
        if (atomicLoad(&eligible[word])&~atomicLoad(&cached[barrier+word]))!=0u {atomicOr(&cached[line.offsets.y+3u],1u);}
        return;
    }
    if line.options.y==4u && atomicLoad(&cached[line.offsets.y+3u])==0u{return;}
    let p=local_point(index);if p.y>=line.window.w{return;}
    let q=line.window.xy+p;
    let open=select(!active_barrier(vec2<i32>(q)),active_barrier(vec2<i32>(q))== (line.options.y==2u),line.options.y>=2u);
    first[index]=select(INVALID,point(q),open);
    if line.options.y==4u && open {second[index]=point(q);}
}
fn assign_nearest(q:vec2<u32>)->u32 {
    if !active_barrier(vec2<i32>(q)) || (ink(vec2<i32>(q)) && distance(vec2<i32>(q))>line.options.x) {return INVALID;}
    return input(local_index(q-line.window.xy));
}
fn deep_beyond(p:vec2<i32>,edge:vec2<u32>,depth:u32)->bool {
    let v=vec2<f32>(p-vec2<i32>(edge));let t=p+vec2<i32>(round(v*(f32((line.options.z+5u-depth)/2u)/length(v))));
    return ink(t) && distance(t)>line.options.z;
}
fn assign_point(q:vec2<u32>) {
    let nearest=assign_nearest(q);if nearest==INVALID{return;}
    let at=coord(nearest);if (atomicLoad(&eligible[at.y*line.extent.z+at.x/32u])&(1u<<(at.x%32u)))==0u {return;}
    let p=vec2<i32>(q);let depth=distance(p);
    let flags=atomicLoad(&cached[line.offsets.y+4u]);
    let kept=(line.job.y!=0u && (flags&1u)!=0u) || (flags&2u)!=0u;
    if ink(p) && !kept && depth>line.options.y && (depth>line.options.z || deep_beyond(p,at,depth) || distance(middle(p))>line.options.z) {return;}
    atomicOr(&eligible[q.y*line.extent.z+q.x/32u],1u<<(q.x%32u));
}
@compute @workgroup_size(64)
fn assign_region(@builtin(global_invocation_id) id:vec3<u32>,@builtin(num_workgroups) groups:vec3<u32>) {
    if atomicLoad(&cached[line.offsets.y+3u])==0u {return;}
    let block=line_block(id.x+id.y*groups.x*64u+line.job.x,line.core,true);var bits=block.z;
    loop {
        if bits==0u {break;}let bit=countTrailingZeros(bits);bits&=bits-1u;
        let q=block.xy+vec2(bit,0u);
        if all(q>=line.core.xy) && all(q<line.core.xy+line.core.zw) {assign_point(q);}
    }
}


@compute @workgroup_size(64)
fn hold_barriers(@builtin(global_invocation_id) id:vec3<u32>,@builtin(num_workgroups) groups:vec3<u32>){
    if !active_line_attempt(){return;}
    let index=id.x+id.y*groups.x*64u+line.job.x;
    if index<line.offsets.x{atomicStore(&cached[line.offsets.z+line.offsets.w*END_WORDS+index],select(~atomicLoad(&eligible[index]),atomicLoad(&eligible[index]),line.options.x!=0u));}
}
@compute @workgroup_size(64)
fn grow_region(@builtin(global_invocation_id) id:vec3<u32>,@builtin(num_workgroups) groups:vec3<u32>){
    let index=id.x+id.y*groups.x*64u+line.job.x;
    let p=vec2(index%line.core.z,index/line.core.z);if p.y>=line.core.w{return;}
    let q=line.core.xy+p;let nearest=input(local_index(q-line.window.xy));
    let within=nearest!=INVALID && square(q,coord(nearest))<=line.options.x*line.options.x;
    let selected=select(within,!within,line.options.y==3u);
    let word=q.y*line.extent.z+q.x/32u;let bit=1u<<(q.x%32u);
    if selected{atomicOr(&eligible[word],bit);}else{atomicAnd(&eligible[word],~bit);}
}

@compute @workgroup_size(64)
fn intersect_bits(@builtin(global_invocation_id) id:vec3<u32>,@builtin(num_workgroups) groups:vec3<u32>){
    let index=id.x+id.y*groups.x*64u+line.job.x;
    if index<line.offsets.x {atomicAnd(&eligible[index],first[index]);}
}

@compute @workgroup_size(64)
fn remember_thin(@builtin(global_invocation_id) id:vec3<u32>,@builtin(num_workgroups) groups:vec3<u32>){
    let index=id.x+id.y*groups.x*64u+line.job.x;
    if index<line.offsets.x {atomicOr(&cached[line.offsets.z+line.offsets.w*END_WORDS+index],first[8u+line.options.x*line.offsets.x+index]);}
}

@compute @workgroup_size(64)
fn check_seeds(@builtin(global_invocation_id) id:vec3<u32>,@builtin(num_workgroups) groups:vec3<u32>){
    let index=id.x+id.y*groups.x*64u+line.job.x;
    if line.options.w!=0u {
        if index==0u {atomicAnd(&cached[line.offsets.y+4u],2u);atomicOr(&cached[line.offsets.y+4u],u32(line.options.x!=0u || (line.options.y!=0u && first[3u]==bitcast<u32>(1.))));}return;
    }
    if index==0u {
        var empty=2u;
        for(var alpha=3u;alpha<arrayLength(&second);alpha+=4u){if bitcast<f32>(second[alpha])>bitcast<f32>(line.job.z){empty=0u;}}
        atomicAnd(&cached[line.offsets.y+4u],1u);atomicOr(&cached[line.offsets.y+4u],empty);
    }
    if index>=arrayLength(&first){return;}
    let p=vec2<i32>(line.options.xy);
    if ink(p) && distance(middle(p))<=line.options.z {return;}
    if ((first[index/4u*4u+3u]|second[index/4u*4u+3u])&0x7fffffffu)==0u {return;}
    let a=first[index];let b=second[index];
    if a!=b && ((a|b)&0x7fffffffu)!=0u {atomicOr(&cached[line.offsets.y+2u],1u);}
}

fn trail_point(index:u32)->vec2<i32> {
    var low=0u;var high=line.options.z;
    loop {if low>=high {break;}let mid=(low+high)/2u;if second[mid*6u+4u]<=index {low=mid+1u;}else{high=mid;}}
    let at=low*6u;var previous=0u;if low>0u {previous=second[at-2u];}
    let start=vec2(bitcast<f32>(second[at]),bitcast<f32>(second[at+1u]));
    let end=vec2(bitcast<f32>(second[at+2u]),bitcast<f32>(second[at+3u]));
    return vec2<i32>(floor(mix(start,end,f32(index-previous)/f32(second[at+5u]))));
}
@compute @workgroup_size(64)
fn barrier_plane(@builtin(global_invocation_id) id:vec3<u32>,@builtin(num_workgroups) groups:vec3<u32>){
    let index=id.x+id.y*groups.x*64u+line.job.x;
    if line.options.x==6u {
        if index>=line.options.y {return;}let p=trail_point(index);if !valid(p){return;}
        if line.core.y!=0u {
            let base=line.core.x;let q=vec2<i32>(floor(vec2<f32>(p)+.5-bitcast<vec2<f32>>(vec2(second[base+6u],second[base+7u]))))-vec2<i32>(vec2(second[base],second[base+1u]));
            let size=vec2(second[base+2u],second[base+3u]);let bytes=second[base+5u]==2u;
            let count=select(8u,4u,bytes);let bits=select(4u,8u,bytes);var covered=false;
            if all(q>=vec2<i32>(0)) && all(q<vec2<i32>(size)) {
                let word=u32(q.y)*((size.x+count-1u)/count)+u32(q.x)/count;
                covered=((second[base+8u+word]>>((u32(q.x)%count)*bits))&select(15u,255u,bytes))!=0u;
            }
            if covered==(second[base+4u]!=0u) {return;}
        }
        if ink(p) && distance(middle(p))<=line.options.w {return;}
        let at=u32(p.y)*line.extent.z+u32(p.x)/32u;
        if (first[8u+at]&(1u<<(u32(p.x)%32u)))==0u {atomicMin(&cached[line.offsets.y+3u],index);}
        return;
    }
    if line.options.x==7u {
        if index==0u {let next=atomicLoad(&cached[line.offsets.y+3u]);first[0u]=INVALID;if next!=INVALID {first[0u]=point(vec2<u32>(trail_point(next)));}}
        return;
    }
    if index>=line.offsets.x{return;}
    if line.options.x==5u {
        if line.options.y!=second[line.options.z] {return;}
        let bits=first[8u+index]|atomicLoad(&eligible[index]);first[8u+index]=bits;atomicStore(&eligible[index],bits);return;
    }
    if line.options.x==3u {
        let bits=atomicLoad(&eligible[index]);
        for(var bit=0u;bit<32u;bit++) {
            if (bits&(1u<<bit))==0u {continue;}
            let p=vec2<i32>(i32((index%line.extent.z)*32u+bit),i32(index/line.extent.z));
            if !valid(p){continue;}
            for(var axis=0u;axis<8u;axis+=2u) {
                let q=p+AROUND[axis];
                if ink(q) && distance(middle(q))<=line.options.z {atomicOr(&cached[line.offsets.y+3u],1u);return;}
            }
        }
        return;
    }
    if line.options.x==0u {first[8u+line.options.y*line.offsets.x+index]=atomicLoad(&eligible[index]);}
    else if line.options.x==2u {second[index]=atomicLoad(&eligible[index]);}
    else {
        let plane=select(second[line.options.y],line.options.y,line.options.x==4u);
        let bits=first[8u+plane*line.offsets.x+index];
        atomicStore(&eligible[index],bits);atomicStore(&cached[line.offsets.z+line.offsets.w*END_WORDS+index],~bits);
    }
}

@compute @workgroup_size(1)
fn snap_seed(){
    let p=vec2<i32>(line.options.xy);var best=point(line.options.xy);var cost=0xffffffffu;
    for(var y=-16;y<=16;y++){for(var x=-16;x<=16;x++){
        let q=p+vec2(x,y);if !valid(q) || raw_ink(q){continue;}
        let d=u32(x*x+y*y);let key=point(vec2<u32>(q));
        if d<cost || (d==cost && key<best) {cost=d;best=key;}
    }}
    let q=coord(best);first[line.options.z+6u]=q.x|(u32(ink(p))<<31u);first[line.options.z+7u]=q.y;
}

@compute @workgroup_size(64)
fn reset_ends(@builtin(global_invocation_id) id:vec3<u32>,@builtin(num_workgroups) groups:vec3<u32>){
    let index=id.x+id.y*groups.x*64u+line.job.x;
    if line.options.x==7u {
        if index<line.offsets.x {let base=line.offsets.z+line.offsets.w*END_WORDS+index;atomicStore(&cached[base],atomicLoad(&cached[base+line.offsets.x]));}
        return;
    }
    if line.options.x>=3u {
        if index>=line.options.y {return;}
        let at=index*END_WORDS;
        if line.options.x==8u {
            let key=atomicLoad(&cached[line.offsets.z+at+15u]);var low=0u;var high=line.options.w;
            loop {if low>=high {break;}let middle=(low+high)/2u;if second[middle]<key {low=middle+1u;}else{high=middle;}}
            var found=false;if low<line.options.w {found=second[low]==key;}
            atomicStore(&cached[line.offsets.z+at+19u],u32(found));return;
        }
        if line.options.x==3u {
            for(var word=0u;word<END_WORDS;word++){first[at+word]=atomicLoad(&cached[line.offsets.z+at+word]);}
            first[at+11u]=0u;
        }else if line.options.x==6u {
            if first[at+15u]==INVALID {return;}
            let slot=atomicAdd(&cached[line.offsets.y],1u);let base=line.offsets.z+slot*END_WORDS;
            for(var word=0u;word<END_WORDS;word++){atomicStore(&cached[base+word],first[at+word]);}
            let q=coord(first[at+15u]);let page=(q.y/128u)*((line.extent.x+127u)/128u)+q.x/128u;
            atomicStore(&cached[base+3u],atomicExchange(&cached[line.offsets.y+HEADER_WORDS+page],slot+1u));
        }else{
            let q=coord(first[at+15u]);
            if all(q>=line.core.xy) && all(q<line.core.xy+line.core.zw) {
                if line.options.x==4u {first[at+15u]=INVALID;}else{first[at+11u]=1u;}
            }
        }
        return;
    }
    if line.options.x==2u {if index==0u{atomicStore(&cached[line.offsets.y+3u],select(0u,INVALID,line.options.z!=0u));}return;}
    if index==0u {atomicStore(&cached[line.offsets.y],0u);atomicStore(&cached[line.offsets.y+1u],0u);}
    if index<line.offsets.z-line.offsets.y-HEADER_WORDS {atomicStore(&cached[line.offsets.y+HEADER_WORDS+index],0u);}
}

@compute @workgroup_size(64)
fn assign_tokens(@builtin(global_invocation_id) id:vec3<u32>,@builtin(num_workgroups) groups:vec3<u32>) {
    if atomicLoad(&cached[line.offsets.y+3u])==0u {return;}
    let block=line_block(id.x+id.y*groups.x*64u+line.job.x,line.core,true);var bits=block.z;
    loop {
        if bits==0u {break;}let bit=countTrailingZeros(bits);bits&=bits-1u;let q=block.xy+vec2(bit,0u);
        if any(q<line.core.xy) || any(q>=line.core.xy+line.core.zw) {continue;}
        let nearest=assign_nearest(q);if nearest==INVALID {continue;}
        let source=8u+local_index(coord(nearest)-line.window.xy);let output_index=8u+local_index(q-line.window.xy);
        if line.options.w==0u {second[output_index]=second[source];}else{first[output_index]=first[source];}
    }
}
@group(0) @binding(7) var area_output:texture_storage_2d<rgba32float,write>;
@compute @workgroup_size(64)
fn colour_tokens(@builtin(global_invocation_id) id:vec3<u32>) {
    let index=id.x+id.y*256u*64u;let p=vec2(index%line.job.z,index/line.job.z);
    if p.y>=line.job.z {return;}let q=line.job.xy+p;
    if any(q<line.core.xy) || any(q>=line.core.xy+line.core.zw) || any(q>=line.extent.xy) {return;}
    let token=first[8u+local_index(q-line.window.xy)];var colour=vec4(0.);
    if token!=0u {
        let hash=token*0x9e3779b9u;let hue=f32(hash>>8u)/16777216.;
        let lightness=array<f32,3>(.52,.62,.72)[((hash>>6u)&3u)%3u];
        let chroma=(1.-abs(2.*lightness-1.))*.65;
        let phase=abs(fract(vec3(hue)+vec3(0.,2./3.,1./3.))*6.-vec3(3.));
        colour=vec4(vec3(lightness)+chroma*(clamp(phase-vec3(1.),vec3(0.),vec3(1.))-vec3(.5)),1.);
        colour=vec4(sdr_decode(colour.rgb,WORKING_SPACE),1.);
    }
    textureStore(area_output,vec2<i32>(p),colour);
}
