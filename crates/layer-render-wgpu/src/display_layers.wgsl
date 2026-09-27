// A moving layer, already drawn at a display level, between the static
// layers below and above it composed at the same level.
struct Region { texels:vec4<u32>, options:vec4<u32> }
@group(0) @binding(0) var moving:texture_2d<f32>;
@group(0) @binding(1) var below:texture_2d<f32>;
@group(0) @binding(2) var above:texture_2d<f32>;
@group(0) @binding(3) var level:texture_storage_2d<rgba32float,write>;
@group(0) @binding(4) var<uniform> region:Region;

@compute @workgroup_size(8,8)
fn composite_main(@builtin(global_invocation_id) id:vec3<u32>) {
    if any(id.xy>=region.texels.zw) {return;}
    let p=vec2<i32>(region.texels.xy+id.xy);
    let src=textureLoad(moving,p,0);
    let dst=textureLoad(below,p,0);
    var color=src+dst*(1.-src.a);
    if region.options.x!=0u {
        let b=blend(working_unassociate(src),working_unassociate(dst),region.options.x);
        color=vec4((1.-src.a)*dst.rgb+(1.-dst.a)*src.rgb+src.a*dst.a*b,src.a+dst.a*(1.-src.a));
    }
    if region.options.y!=0u {
        let over=textureLoad(above,p,0);
        color=over+color*(1.-over.a);
    }
    textureStore(level,p,color);
}
