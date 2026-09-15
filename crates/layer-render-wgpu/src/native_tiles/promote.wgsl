struct PublicationStatus { invalid: u32, clipped: u32 }
@group(0) @binding(0) var canonical: texture_2d<f32>;
@group(0) @binding(1) var<storage, read> status: PublicationStatus;
@group(0) @binding(2) var working: texture_storage_2d<FORMAT, write>;
@group(0) @binding(3) var<uniform> region: vec4<u32>;

@compute @workgroup_size(8, 8) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if status.invalid != 0u || any(gid.xy >= region.zw) { return; }
    let p = vec2<i32>(region.xy + gid.xy);
    textureStore(working, p, textureLoad(canonical, p, 0));
}
