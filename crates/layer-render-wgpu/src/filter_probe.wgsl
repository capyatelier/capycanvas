// Find a real painted pixel near the center, not the center of an empty bounding
// box. One global atomic per workgroup; only the winning coordinate is read back.
@group(0) @binding(0) var source: texture_2d<f32>;
struct Probe { background:vec4<f32>, crop:vec4<u32>, window:vec4<u32>, core:vec4<u32> }
@group(0) @binding(1) var<uniform> probe: Probe;
@group(0) @binding(2) var<storage,read_write> winner: array<atomic<u32>,2>;
var<workgroup> local_winner: array<atomic<u32>,2>;
fn painted(p:vec2<i32>)->bool {
    let extent=vec2<i32>(probe.crop.zw);
    if any(p<vec2<i32>(0)) || any(p>=extent) {return false;}
    let local=p-vec2<i32>(probe.window.xy);
    if any(local<vec2<i32>(0)) || any(local>=vec2<i32>(textureDimensions(source))) {return false;}
    let paper=vec4<f32>(probe.background.rgb*probe.background.a,probe.background.a);
    return any(abs(textureLoad(source,local,0)-paper)>vec4<f32>(2./255.));
}
@compute @workgroup_size(16,16)
fn measure(@builtin(global_invocation_id) id:vec3<u32>, @builtin(local_invocation_index) local:u32) {
    if local==0u {atomicStore(&local_winner[0],0u);atomicStore(&local_winner[1],0u);}
    workgroupBarrier();
    let extent=probe.crop.zw;
    let pixel=id.xy+probe.window.zw;
    if all(id.xy<probe.core.xy) {
        if painted(vec2<i32>(pixel)) {
            let delta=vec2<i32>(pixel)-vec2<i32>(extent/2u);
            let zig=vec2<u32>(abs(delta))*2u+vec2<u32>(delta<vec2<i32>(0));
            let score=0xffffffffu-((zig.y<<16u)|zig.x);
            atomicMax(&local_winner[1],score);
            let p=vec2<i32>(pixel);let half=vec2<i32>(probe.crop.xy/2u);
            if painted(p-half) && painted(p+half) && painted(p+vec2<i32>(half.x,-half.y)) && painted(p+vec2<i32>(-half.x,half.y)) {
                atomicMax(&local_winner[0],score);
            }
        }
    }
    workgroupBarrier();
    if local==0u {atomicMax(&winner[0],atomicLoad(&local_winner[0]));atomicMax(&winner[1],atomicLoad(&local_winner[1]));}
}
