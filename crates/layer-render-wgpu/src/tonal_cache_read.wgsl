@group(0) @binding(22) var tone_cache0: texture_2d_array<f32>;
@group(0) @binding(23) var tone_cache1: texture_2d_array<f32>;
@group(0) @binding(24) var tone_cache2: texture_2d_array<f32>;
@group(0) @binding(25) var tone_cache3: texture_2d_array<f32>;
fn cache_store(index:u32, value:vec2<f32>) {}
fn cache_load(world:vec2<u32>)->vec2<f32> {
    let index=select(world.y*tonal.cache.y+world.x,world.y*((tonal.cache.y+1u)/2u)+world.x/2u,tonal.cache.w!=0u);
    let slot=index/tonal.cache.x; let p=index%tonal.cache.x;
    let xy=vec2<i32>(i32(p%256u),i32((p/256u)%256u)); let z=i32(p/65536u);
    var value=vec2<f32>(0.);
    switch slot {
        case 0u:{value=textureLoad(tone_cache0,xy,z,0).xy;}
        case 1u:{value=textureLoad(tone_cache1,xy,z,0).xy;}
        case 2u:{value=textureLoad(tone_cache2,xy,z,0).xy;}
        default:{value=textureLoad(tone_cache3,xy,z,0).xy;}
    }
    if tonal.cache.w!=0u {return vec2<f32>(value[world.x%2u],1.);}
    return value;
}
