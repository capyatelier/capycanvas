struct Status { invalid:atomic<u32>, clipped:atomic<u32> }
TEXTURES
@group(0) @binding(STATUS_BINDING) var<storage,read_write> status:Status;
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) invocation:vec3<u32>) {
    var value:vec4<f32>;
    switch invocation.z {
        LOADS
        default: { return; }
    }
    let error=VALIDATE;
    if error!=0u {atomicOr(&status.invalid,error);}
}
