struct Settings { maximum:u32, scale:u32, curve:u32, straight:u32, region:vec4<u32> }
struct Status { invalid:atomic<u32>, clipped:atomic<u32> }
TEXTURES
@group(0) @binding(TRANSFER_BINDING) var<storage,read> transfer:array<vec2<f32>>;
@group(0) @binding(SETTINGS_BINDING) var<uniform> settings:Settings;
@group(0) @binding(STATUS_BINDING) var<storage,read_write> status:Status;
fn load_working(tile:u32,pixel:vec2<i32>)->vec4<f32> {
    switch tile { LOADS default: { return vec4(0.); } }
}
fn store_outputs(tile:u32,pixel:vec2<u32>,result:vec4<u32>,linear:vec4<f32>) {
    switch tile { STORES default: { return; } }
}
fn store_result(tile:u32,pixel:vec2<u32>, result:vec4<u32>) {
    let alpha=select(f32(result.a)/f32(settings.maximum),1.,result.a==settings.maximum);
    let code=result.rgb*settings.scale;
    var linear=vec3(transfer[code.r].x,transfer[code.g].x,transfer[code.b].x);
    if settings.straight!=0u {linear*=alpha;}
    store_outputs(tile,pixel,result,vec4(linear,alpha));
}
fn boundary(code:u32)->f32 {
    return transfer[code*settings.scale+(settings.scale-1u)/2u].y;
}
fn bracket(value:f32,code:u32)->bool {
    if code>0u && value<boundary(code-1u) {return false;}
    return code==settings.maximum || value<boundary(code);
}
fn quantize(value:f32)->u32 {
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
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) invocation:vec3<u32>) {
    PUBLICATION_GUARD
    if any(invocation.xy>=settings.region.zw) {return;}
    let pixel=invocation.xy+settings.region.xy;
    let value=load_working(invocation.z,vec2<i32>(pixel));
    if settings.maximum==0u {
        if settings.scale==32u {
            let error=float32_color_error(value);
            if error!=0u {atomicOr(&status.invalid,error);return;}
            if value.a==0. {store_outputs(invocation.z,pixel,vec4(0u),vec4(0.));return;}
            let straight=vec4(value.rgb/value.a,value.a);
            store_outputs(invocation.z,pixel,bitcast<vec4<u32>>(straight),vec4(straight.rgb*straight.a,straight.a));
            return;
        }
        let error=hdr_color_error(value);
        if error!=0u {atomicOr(&status.invalid,error);return;}
        let alpha_bits=half_bits(value.a);
        if alpha_bits==0u {store_outputs(invocation.z,pixel,vec4(0u),vec4(0.));return;}
        let rgb=value.rgb/value.a;
        let bits=vec4(half_bits(rgb.r),half_bits(rgb.g),half_bits(rgb.b),alpha_bits);
        let alpha=half_value(alpha_bits);
        store_outputs(invocation.z,pixel,bits,vec4(vec3(half_value(bits.r),half_value(bits.g),half_value(bits.b))*alpha,alpha));
        return;
    }
    let error=color_error(value);
    if error!=0u {atomicOr(&status.invalid,error);store_result(invocation.z,pixel,vec4(0u));return;}
    let alpha=quantize_coverage(value.a,settings.maximum);
    if alpha==0u {store_result(invocation.z,pixel,vec4(0u));return;}
    var rgb=value.rgb;
    let limit=select(1.,value.a,settings.straight!=0u);
    if any(rgb<vec3(0.)) || any(rgb>vec3(limit)) {atomicAdd(&status.clipped,1u);}
    // Clamp only at the declared native SDR publication boundary. Clamp before
    // division so finite extended RGB cannot overflow while unassociating.
    rgb=clamp(rgb,vec3(0.),vec3(limit));
    if settings.straight!=0u {rgb/=value.a;}
    let result=vec4(quantize(rgb.r),quantize(rgb.g),quantize(rgb.b),alpha);
    store_result(invocation.z,pixel,result);
}
