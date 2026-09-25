@group(0) @binding(22) var tone_cache0: texture_storage_2d_array<rg32float,write>;
@group(0) @binding(23) var tone_cache1: texture_storage_2d_array<rg32float,write>;
@group(0) @binding(24) var tone_cache2: texture_storage_2d_array<rg32float,write>;
@group(0) @binding(25) var tone_cache3: texture_storage_2d_array<rg32float,write>;
fn cache_store(index:u32, value:vec2<f32>) {
    let slot=index/tonal.cache.x; let p=index%tonal.cache.x;
    let xy=vec2<i32>(i32(p%256u),i32((p/256u)%256u)); let z=i32(p/65536u);
    let v=vec4<f32>(value,0.,0.);
    switch slot { case 0u:{textureStore(tone_cache0,xy,z,v);} case 1u:{textureStore(tone_cache1,xy,z,v);} case 2u:{textureStore(tone_cache2,xy,z,v);} default:{textureStore(tone_cache3,xy,z,v);} }
}
fn cache_load(world:vec2<u32>)->vec2<f32> { return vec2<f32>(0.); }
