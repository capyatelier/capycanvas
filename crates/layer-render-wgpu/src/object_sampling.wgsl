struct Parameters {
    x: vec4<f32>, y: vec4<f32>, inverse_stretch: vec4<f32>, radius: vec4<f32>,
    source: vec4<i32>, window: vec4<u32>, work: vec4<u32>,
}
struct Accumulation { color: vec4<f32>, weight: vec4<f32> }
@group(0) @binding(0) var<uniform> parameters: Parameters;
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var<storage,read_write> accumulation: array<Accumulation>;
@group(0) @binding(3) var output: texture_storage_2d<rgba32float,write>;
@group(0) @binding(4) var<storage,read> nearest_coordinates: array<vec2<i32>>;
fn centre(p: vec2<u32>) -> vec2<f32> {
    let h = vec3(vec2<f32>(p-parameters.work.xy)+vec2(0.5)-vec2<f32>(parameters.work.zw)*0.5,1.);
    return vec2(dot(parameters.x.xyz,h),dot(parameters.y.xyz,h));
}
fn weight(p: vec2<i32>, q: vec2<f32>) -> f32 {
    let d = vec2<f32>(p)+vec2(0.5)-q;
    let s = parameters.inverse_stretch.xyz;
    let r = vec2(s.x*d.x+s.y*d.y,s.y*d.x+s.z*d.y);
    let w = max(vec2(0.),vec2(1.)-abs(r));
    return w.x*w.y;
}
@compute @workgroup_size(8,8) fn denominator(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy>=parameters.work.zw) { return; }
    let position=id.xy+parameters.work.xy;
    let index = position.y*parameters.window.x+position.x;
    if parameters.radius.z!=0. || parameters.radius.w!=0. { accumulation[index].weight.x=1.; return; }
    let q = centre(position);
    let low = vec2<i32>(floor(q-parameters.radius.xy-vec2(0.5)));
    let high = vec2<i32>(ceil(q+parameters.radius.xy-vec2(0.5)));
    let begin = low.y+i32(parameters.window.z);
    let end = min(high.y+1,begin+i32(parameters.window.w));
    var total = 0.;
    for(var y=begin;y<end;y++) { for(var x=low.x;x<=high.x;x++) { total+=weight(vec2(x,y),q); } }
    accumulation[index].weight.x+=total;
}
@compute @workgroup_size(8,8) fn contribute(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy>=parameters.work.zw) { return; }
    let position=id.xy+parameters.work.xy;
    let index = position.y*parameters.window.x+position.x;
    let origin = parameters.source.xy;
    let extent = parameters.source.zw;
    if parameters.radius.z!=0. {
        let p = nearest_coordinates[index];
        if all(p>=origin) && all(p<origin+extent) { accumulation[index].color=textureLoad(source,p-origin,0); }
        return;
    }
    let q = centre(position);
    if parameters.radius.w!=0. {
        let point=q-vec2(0.5);
        let low=vec2<i32>(floor(point));
        let fraction=fract(point);
        var sum=vec4(0.);
        for(var y=0;y<2;y++) {for(var x=0;x<2;x++) {
            let p=low+vec2(x,y);
            if all(p>=origin) && all(p<origin+extent) {
                let w=select(1.-fraction.x,fraction.x,x==1)*select(1.-fraction.y,fraction.y,y==1);
                if w>0. {sum+=textureLoad(source,p-origin,0)*w;}
            }
        }}
        accumulation[index].color+=sum;
        return;
    }
    let kernel_low = vec2<i32>(floor(q-parameters.radius.xy-vec2(0.5)));
    let low = max(origin,kernel_low);
    let high = min(origin+extent,vec2<i32>(ceil(q+parameters.radius.xy-vec2(0.5)))+vec2(1));
    let begin = max(low.y,kernel_low.y+i32(parameters.window.z));
    let end = min(high.y,kernel_low.y+i32(parameters.window.z+parameters.window.w));
    var sum = vec4(0.);
    for(var y=begin;y<end;y++) { for(var x=low.x;x<high.x;x++) {
        let p = vec2(x,y);
        let w = weight(p,q);
        if w>0. { sum+=textureLoad(source,p-origin,0)*w; }
    }}
    accumulation[index].color+=sum;
}
@compute @workgroup_size(8,8) fn normalize(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy>=parameters.work.zw) { return; }
    let position=id.xy+parameters.work.xy;
    let value = accumulation[position.y*parameters.window.x+position.x];
    textureStore(output,vec2<i32>(position),value.color/max(value.weight.x,1e-30));
}
