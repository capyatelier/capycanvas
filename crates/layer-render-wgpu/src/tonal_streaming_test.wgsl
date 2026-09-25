// Hardware control: identical scalar reads, mask writes, bounds and readback,
// with trivial arithmetic instead of tonal classification. Test builds only.
@compute @workgroup_size(64)
fn tonal_streaming_control(@builtin(global_invocation_id) id:vec3<u32>) {
    let extent=tonal.cache.yz;
    let stride=(extent.x+3u)/4u;
    if id.x>=stride || id.y>=extent.y {return;}
    var packed=0u;
    for(var i=0u;i<4u && id.x*4u+i<extent.x;i++) {
        let value=cache_load(vec2<u32>(id.x*4u+i,id.y));
        packed|=u32(round(clamp(value.x*value.y,0.,1.)*255.))<<(8u*i);
    }
    eligibility[8u+id.y*stride+id.x]=packed;
}
