struct Settings { maximum:u32, components:u32, padding:vec2<u32>, region:vec4<u32> }
struct Status { invalid:atomic<u32>, clipped:atomic<u32> }
@group(0) @binding(0) var working:texture_2d<f32>;
@group(0) @binding(1) var<storage,read_write> encoded:array<u32>;
@group(0) @binding(2) var canonical:texture_storage_2d<r32float,write>;
@group(0) @binding(3) var<uniform> settings:Settings;
@group(0) @binding(4) var<storage,read_write> status:Status;

// Each invocation owns a complete packed word, including preserved components
// outside the region. Odd region edges therefore need neither atomics nor a
// read/modify/write race with their neighboring pixels.
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) invocation:vec3<u32>) {
    let first=settings.region.x/settings.components;
    let end=(settings.region.x+settings.region.z+settings.components-1u)/settings.components;
    let word_x=first+invocation.x;
    if word_x>=end || invocation.y>=settings.region.w {return;}
    let y=settings.region.y+invocation.y;
    let address=y*(256u/settings.components)+word_x;
    var word=encoded[address];
    let depth=32u/settings.components;
    for(var c=0u;c<settings.components;c++) {
        let x=word_x*settings.components+c;
        if x<settings.region.x || x>=settings.region.x+settings.region.z {continue;}
        let pixel=vec2(x,y);
        let value=textureLoad(working,vec2<i32>(pixel),0).r;
        let bits=bitcast<u32>(value);
        let magnitude=bits&0x7fffffffu;
        var code=0u;
        if magnitude>=0x7f800000u {
            atomicOr(&status.invalid,1u);
        } else if magnitude>0x3f800000u || ((bits>>31u)!=0u && magnitude!=0u) {
            atomicOr(&status.invalid,2u);
        } else {
            code=quantize_coverage(value,settings.maximum);
        }
        let shift=c*depth;
        word=(word&~(settings.maximum<<shift))|(code<<shift);
        textureStore(canonical,pixel,vec4(f32(code)/f32(settings.maximum),0.,0.,1.));
    }
    encoded[address]=word;
}
