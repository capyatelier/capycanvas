@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) invocation:vec3<u32>) {encode_pixel(invocation);}
