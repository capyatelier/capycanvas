// A moving layer resampled from its copy reduced to a display level: one
// bilinear sample per texel of the pixels that move and, when the selection
// keeps some in place, of those, times opacity, over the premultiplied
// backdrop. Rows x, y and w map a texel center to homogeneous texel
// coordinates of the moved copy, kept_x, kept_y and kept_w to those of the
// kept copy, and clip_x and clip_y to layer pixels, of which the layer keeps
// only those inside its extent.
struct Resample {
    x:vec4<f32>, y:vec4<f32>, w:vec4<f32>,
    kept_x:vec4<f32>, kept_y:vec4<f32>, kept_w:vec4<f32>,
    clip_x:vec4<f32>, clip_y:vec4<f32>, extent:vec4<f32>,
    texels:vec4<u32>, backdrop:vec4<f32>, options:vec4<f32>,
}
@group(0) @binding(0) var moved:texture_2d<f32>;
@group(0) @binding(1) var level:texture_storage_2d<rgba32float,write>;
@group(0) @binding(2) var<uniform> resample:Resample;
@group(0) @binding(3) var kept:texture_2d<f32>;

fn tap(image:texture_2d<f32>,p:vec2<i32>)->vec4<f32> {
    if any(p<vec2(0)) || any(p>=vec2<i32>(textureDimensions(image))) {return vec4(0.);}
    return textureLoad(image,p,0);
}
fn bilinear(image:texture_2d<f32>,x:vec4<f32>,y:vec4<f32>,w:vec4<f32>,h:vec3<f32>)->vec4<f32> {
    let d=dot(w.xyz,h);
    if d<=0. {return vec4(0.);}
    let size=vec2<f32>(textureDimensions(image));
    let s=clamp(vec2(dot(x.xyz,h),dot(y.xyz,h))/d-.5,vec2(-2.),size+1.);
    let base=vec2<i32>(floor(s));
    let f=fract(s);
    return mix(mix(tap(image,base),tap(image,base+vec2(1,0)),f.x),
        mix(tap(image,base+vec2(0,1)),tap(image,base+vec2(1,1)),f.x),f.y);
}
@compute @workgroup_size(8,8)
fn resample_main(@builtin(global_invocation_id) id:vec3<u32>) {
    if any(id.xy>=resample.texels.zw) {return;}
    let t=resample.texels.xy+id.xy;
    let h=vec3(vec2<f32>(t)+.5,1.);
    let layer_pixel=vec2(dot(resample.clip_x.xyz,h),dot(resample.clip_y.xyz,h));
    var color=vec4(0.);
    if all(layer_pixel>=vec2(0.)) && all(layer_pixel<resample.extent.xy) {
        color=bilinear(moved,resample.x,resample.y,resample.w,h);
        if resample.options.y!=0. {
            color+=bilinear(kept,resample.kept_x,resample.kept_y,resample.kept_w,h)*(1.-color.a);
        }
    }
    let layer=color*resample.options.x;
    textureStore(level,vec2<i32>(t),layer+resample.backdrop*(1.-layer.a));
}
