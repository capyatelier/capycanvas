struct PublicationStatus { invalid:u32, clipped:u32 }
TEXTURES
@group(0) @binding(STATUS_BINDING) var<storage,read> status:PublicationStatus;
@group(0) @binding(REGION_BINDING) var<uniform> region:vec4<u32>;

@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) gid:vec3<u32>) {
    if status.invalid!=0u || any(gid.xy>=region.zw) {return;}
    let p=vec2<i32>(region.xy+gid.xy);
    switch gid.z {
        COPY_TILES
        default: {return;}
    }
}
