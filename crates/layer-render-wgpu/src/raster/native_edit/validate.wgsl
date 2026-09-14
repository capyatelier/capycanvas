struct Status { invalid:atomic<u32>, clipped:atomic<u32> }
@group(0) @binding(0) var working:texture_2d<f32>;
@group(0) @binding(1) var<storage,read_write> status:Status;
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) invocation:vec3<u32>) {
    let value=textureLoad(working,vec2<i32>(invocation.xy),0);
    let error=VALIDATE;
    if error!=0u {atomicOr(&status.invalid,error);}
}
