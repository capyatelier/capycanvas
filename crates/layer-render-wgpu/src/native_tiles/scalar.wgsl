struct Settings { maximum:u32, components:u32, padding:vec2<u32>, region:vec4<u32> }
struct Status { invalid:atomic<u32>, clipped:atomic<u32> }
TEXTURES
@group(0) @binding(SETTINGS_BINDING) var<uniform> settings:Settings;
@group(0) @binding(STATUS_BINDING) var<storage,read_write> status:Status;
fn load_working(tile:u32,pixel:vec2<i32>)->f32 {
    switch tile { LOADS default: { return 0.; } }
}
fn load_word(tile:u32,address:u32)->u32 {
    switch tile { LOAD_WORDS default: { return 0u; } }
}
fn store_word(tile:u32,address:u32,word:u32) {
    switch tile { STORE_WORDS default: { return; } }
}
fn store_canonical(tile:u32,pixel:vec2<u32>,value:vec4<f32>) {
    switch tile { STORES default: { return; } }
}

// Each invocation owns a complete packed word, including preserved components
// outside the region. Odd region edges therefore need neither atomics nor a
// read/modify/write race with their neighboring pixels.
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) invocation:vec3<u32>) {
    PUBLICATION_GUARD
    let first=settings.region.x/settings.components;
    let end=(settings.region.x+settings.region.z+settings.components-1u)/settings.components;
    let word_x=first+invocation.x;
    if word_x>=end || invocation.y>=settings.region.w {return;}
    let y=settings.region.y+invocation.y;
    let address=y*(256u/settings.components)+word_x;
    var word=load_word(invocation.z,address);
    let depth=32u/settings.components;
    for(var c=0u;c<settings.components;c++) {
        let x=word_x*settings.components+c;
        if x<settings.region.x || x>=settings.region.x+settings.region.z {continue;}
        let pixel=vec2(x,y);
        let value=load_working(invocation.z,vec2<i32>(pixel));
        let error=scalar_error(value);
        var code=0u;
        if error!=0u {atomicOr(&status.invalid,error);} else {
            code=quantize_coverage(value,settings.maximum);
        }
        let shift=c*depth;
        word=(word&~(settings.maximum<<shift))|(code<<shift);
        store_canonical(invocation.z,pixel,vec4(f32(code)/f32(settings.maximum),0.,0.,1.));
    }
    store_word(invocation.z,address,word);
}
