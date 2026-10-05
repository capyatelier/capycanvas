var<private> packed_value:u32;
var<private> packed_mask:u32;
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) invocation:vec3<u32>) {
    if settings.maximum!=255u {encode_pixel(invocation);return;}
    if invocation.y>=settings.region.w {return;}
    let x=(settings.region.x/2u+invocation.x)*2u;
    let end=settings.region.x+settings.region.z;
    if x>=end {return;}
    if x>=settings.region.x {encode_pixel(vec3(x-settings.region.x,invocation.y,invocation.z));}
    if x+1u<end {encode_pixel(vec3(x+1u-settings.region.x,invocation.y,invocation.z));}
    if packed_mask==0u {return;}
    let index=((settings.region.y+invocation.y)*256u+x)/2u;
    switch invocation.z { PACKED_STORES default: {return;} }
}
