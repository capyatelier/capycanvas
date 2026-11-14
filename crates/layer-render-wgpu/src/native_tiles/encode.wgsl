struct Settings { maximum:u32, scale:u32, curve:u32, flags:u32, region:vec4<u32>, weights:vec4<f32> }
struct ChangedCells {side:u32,enabled:u32,padding0:u32,padding1:u32,cells:array<atomic<u32>>}
var<private> original:vec4<f32>;
fn comparison_enabled()->bool {return (settings.flags&2u)!=0u;}
TEXTURES
@group(0) @binding(TRANSFER_BINDING) var<storage,read> transfer:array<vec2<f32>>;
@group(0) @binding(SETTINGS_BINDING) var<uniform> settings:Settings;
fn load_working(tile:u32,pixel:vec2<i32>)->vec4<f32> {
    switch tile { LOADS default: { return vec4(0.); } }
}
fn store_outputs(tile:u32,pixel:vec2<u32>,result:vec4<u32>,linear:vec4<f32>) {
    mark_canonical(tile,pixel,linear);
    switch tile { STORES default: { return; } }
}
fn decode_quantized(code:u32)->f32 {
    if code==0u {return 0.;}
    if code==65535u {return 1.;}
    return transfer[code].x;
}
fn store_result(tile:u32,pixel:vec2<u32>, result:vec4<u32>) {
    let alpha=select(f32(result.a)/f32(settings.maximum),1.,result.a==settings.maximum);
    let code=result.rgb*settings.scale;
    var linear=vec3(decode_quantized(code.r),decode_quantized(code.g),decode_quantized(code.b));
    if (settings.flags&1u)!=0u {linear*=alpha;}
    store_outputs(tile,pixel,result,vec4(linear,alpha));
}
fn boundary(code:u32)->f32 {
    return transfer[code*settings.scale+(settings.scale-1u)/2u].y;
}
fn bracket(value:f32,code:u32)->bool {
    if code>0u && value<boundary(code-1u) {return false;}
    if code>=settings.maximum {return true;}
    return value<boundary(code);
}
fn quantize(value:f32)->u32 {
    if value==0. {return 0u;}
    if value==1. {return settings.maximum;}
    var code=u32(floor(clamp(sdr_encode_component(value,settings.curve),0.,1.)*f32(settings.maximum)+0.5));
    for(var step=0u;step<2u;step++) {
        if bracket(value,code) {return code;}
        if code>0u && value<boundary(code-1u) {code--;} else {code++;}
    }
    if bracket(value,code) {return code;}
    var low=0u;var high=settings.maximum;
    for(var step=0u;step<16u && low<high;step++) {
        let mid=(low+high)/2u;
        if value>=boundary(mid) {low=mid+1u;} else {high=mid;}
    }
    return low;
}
fn encode_pixel(invocation:vec3<u32>) {
    if any(invocation.xy>=settings.region.zw) {return;}
    let pixel=invocation.xy+settings.region.xy;
    original=load_working(invocation.z,vec2<i32>(pixel));
    var value=publishable(original);
    MODE_PROJECTION
    FLOAT_PUBLICATION
    let alpha=quantize_coverage(value.a,settings.maximum);
    if alpha==0u {store_result(invocation.z,pixel,vec4(0u));return;}
    var rgb=value.rgb;
    let limit=select(1.,value.a,(settings.flags&1u)!=0u);
    // Clamp only at the declared native SDR publication boundary. Clamp before
    // division so finite extended RGB cannot overflow while unassociating.
    rgb=clamp(rgb,vec3(0.),vec3(limit));
    if (settings.flags&1u)!=0u {rgb/=value.a;}
    let result=vec4(quantize(rgb.r),quantize(rgb.g),quantize(rgb.b),alpha);
    store_result(invocation.z,pixel,result);
}
