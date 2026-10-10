const SELECTED=32u;
const KEPT=64u;
const ENCODED=512u;
@group(2) @binding(0) var display_level:texture_storage_2d<rgba32float,write>;
fn display_pixel(world:vec2<f32>)->vec4<f32> {
    let p=vec2<i32>(floor(world));
    if (flags()&(UNMOVED|SELECTED))==(UNMOVED|SELECTED) {return selected(p);}
    if (flags()&(UNMOVED|KEPT))==(UNMOVED|KEPT) {return original(p)*(1.-remainder_selection(world));}
    return layer_pixel(world);
}
var<workgroup> display_pixels:array<vec4<f32>,256>;
@compute @workgroup_size(16,16)
fn display_main(@builtin(workgroup_id) group:vec3<u32>,@builtin(local_invocation_id) local:vec3<u32>) {
    let side=u32(transform.display.x);
    let first=vec2<u32>(transform.texels.xy)+group.xy*(16u/side);
    let end=vec2<u32>(transform.texels.xy+transform.texels.zw);
    let world=vec2<f32>(first*side+local.xy)+.5+transform.attachment.xy;
    let inside=all(world<transform.display.zw);
    var value=vec4(0.);
    if inside && all(first+local.xy/side<end) {value=display_pixel(world);}
    display_pixels[local.y*16u+local.x]=select(vec4(-1.),value,inside);
    workgroupBarrier();
    let texel=first+local.xy/side;
    if any(local.xy%side!=vec2(0u)) || any(texel>=end) {return;}
    var sum=vec4(0.);
    var count=0.;
    for (var j=0u;j<side;j++) {
        for (var i=0u;i<side;i++) {
            let pixel=display_pixels[(local.y+j)*16u+local.x+i];
            if pixel.a>=0. {
                sum+=pixel;
                count+=1.;
            }
        }
    }
    var layer=sum*(transform.display.y/max(count,1.));
    if (flags()&ENCODED)!=0u {layer=working_encode(sum/max(count,1.))*transform.display.y;}
    textureStore(display_level,vec2<i32>(texel),layer+transform.backdrop*(1.-layer.a));
}
