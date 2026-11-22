struct FineDirectory { phase:vec2<i32>, page_side:u32, table_mask:u32, reserved:vec4<u32>, pages:array<vec4<i32>>, }
@group(0) @binding(5) var fine_pages:texture_2d<f32>;
@group(0) @binding(6) var<storage,read> fine_directory:FineDirectory;
fn fine_entry(plane:u32,page:vec2<u32>)->vec2<i32> {
    let key=vec2(page.x,page.y|(plane<<31u));
    var slot=(key.x*1664525u+key.y*1013904223u)&fine_directory.table_mask;
    for(var visited=0u;visited<=fine_directory.table_mask;visited++) {
        let entry=fine_directory.pages[slot];
        if entry.x<0 {return vec2(-1);}
        if all(bitcast<vec2<u32>>(entry.xy)==key) {return entry.zw;}
        slot=(slot+1u)&fine_directory.table_mask;
    }
    return vec2(-1);
}
fn fine_sample(plane:u32,q:vec2<f32>,extent:vec2<f32>)->vec4<f32> {
    let coordinate=clamp(q,vec2(.5),ceil(extent)-.5)-.5;
    let low=vec2<i32>(floor(coordinate));let weight=fract(coordinate);
    let last=vec2<i32>(ceil(extent))-1;
    var t0=vec4(0.);var t1=vec4(0.);var t2=vec4(0.);var t3=vec4(0.);
    for (var i=0u;i<4u;i++) {
        let tap=clamp(low+vec2(i32(i&1u),i32(i>>1u)),vec2(0),last)-fine_directory.phase;
        if any(tap<vec2(0)) {continue;}
        let page=vec2<u32>(tap)/fine_directory.page_side;
        let origin=fine_entry(plane,page);
        if origin.x<0 {continue;}
        let color=textureLoad(fine_pages,origin+vec2<i32>(vec2<u32>(tap)%fine_directory.page_side),0);
        if i==0u {t0=color;} else if i==1u {t1=color;} else if i==2u {t2=color;} else {t3=color;}
    }
    return mix(mix(t0,t1,weight.x),mix(t2,t3,weight.x),weight.y);
}
fn fine_pyramid_sample(plane:u32,image:texture_2d<f32>,linear_sampler:sampler,extent:vec2<f32>,outside:f32,q:vec2<f32>,lod:f32,max_lod:f32,origin:vec2<f32>)->vec4<f32> {
    if any(q<vec2(0.)) || any(q>=extent) {return vec4(outside);}
    let level=clamp(lod,0.,max_lod);
    var low=vec4(0.);var weight=1.;
    if level<1. {
        low=fine_sample(plane,q,extent);
        let side=exp2(ceil(level));
        if level==0. || (!all(q<=(floor(extent/side)-.5)*side) && level<0.00001) {return low;}
        weight=level;
    }
    return mix(low,pyramid_sample(image,linear_sampler,extent*.5,outside,q*.5,max(level-1.,0.),max_lod-1.,origin),weight);
}
fn mapped_source(plane:u32,q:vec2<f32>,lod:f32)->vec4<f32> {
    if plane==1u {return fine_pyramid_sample(plane,kept,linear_sampler,resample.source_extent.xy,resample.options.w,q,lod,resample.options.y,resample.kept_origin);}
    return fine_pyramid_sample(plane,moved,linear_sampler,resample.source_extent.xy,resample.options.w,q,lod,resample.options.y,resample.moved_origin);
}
