@group(0) @binding(0) var moved:texture_2d<f32>;
@group(0) @binding(1) var level:texture_storage_2d<rgba32float,write>;
@group(0) @binding(2) var<uniform> resample:Resample;
@group(0) @binding(3) var kept:texture_2d<f32>;
@group(0) @binding(4) var linear_sampler:sampler;

fn composite_color(color:vec4<f32>)->vec4<f32> {
    var layer=color*resample.options.x;
    if resample.encoding.x!=0. {layer=working_encode(color)*resample.options.x;}
    return layer+resample.backdrop*(1.-layer.a);
}
fn resample_color(t:vec2<u32>,mesh:bool,keeps:bool,source:vec2<f32>,dx:vec2<f32>,dy:vec2<f32>)->vec4<f32> {
    let footprint=min(vec2(1.),resample.target_extent.xy-vec2<f32>(t));
    let center=vec2<f32>(t)+footprint*.5;
    let q=source+dx*(footprint.x-1.)*.5+dy*(footprint.y-1.)*.5;
    return composite_color(mapped_color(2u,center,vec2(footprint.x,0.),vec2(0.,footprint.y),mesh,keeps,q,dx,dy));
}
@fragment fn resample_projective(@builtin(position) p:vec4<f32>)->@location(0) vec4<f32> {
    return resample_color(vec2<u32>(p.xy),false,false,vec2(0.),vec2(0.),vec2(0.));
}
@fragment fn resample_projective_kept(@builtin(position) p:vec4<f32>)->@location(0) vec4<f32> {
    return resample_color(vec2<u32>(p.xy),false,true,vec2(0.),vec2(0.),vec2(0.));
}

struct MeshVertex { @builtin(position) position:vec4<f32>, @location(0) source:vec2<f32> }
@vertex fn mesh_vertex(@location(0) destination:vec2<f32>, @location(1) source:vec2<f32>, @location(2) weight:f32)->MeshVertex {
    let x=resample.clip_x;let y=resample.clip_y;
    let p=destination-vec2(x.z,y.z);
    let output=vec2(y.y*p.x-x.y*p.y,x.x*p.y-y.x*p.x)/(x.x*y.y-x.y*y.x);
    let normalized=output/ceil(resample.target_extent.xy);
    return MeshVertex(vec4((normalized.x*2.-1.)*weight,(1.-normalized.y*2.)*weight,0.,weight),source);
}
@vertex fn background_vertex(@builtin(vertex_index) index:u32)->@builtin(position) vec4<f32> {
    let p=vec2(f32((index<<1u)&2u),f32(index&2u));
    return vec4(p*2.-1.,0.,1.);
}
fn mesh_source(owner:vec2<f32>)->vec2<f32> {
    let h=vec3(owner,1.);
    let w=dot(resample.w.xyz,h);
    if w<=0. {return vec2(UNCOVERED);}
    return vec2(dot(resample.x.xyz,h),dot(resample.y.xyz,h))/w;
}
@fragment fn mesh_color(vertex:MeshVertex)->@location(0) vec4<f32> {
    let source=mesh_source(vertex.source);
    return resample_color(vec2<u32>(vertex.position.xy),true,false,source,dpdx(source),dpdy(source));
}
@fragment fn mesh_color_kept(vertex:MeshVertex)->@location(0) vec4<f32> {
    let source=mesh_source(vertex.source);
    return resample_color(vec2<u32>(vertex.position.xy),true,true,source,dpdx(source),dpdy(source));
}
@fragment fn background_color(@builtin(position) position:vec4<f32>)->@location(0) vec4<f32> {
    if resample.options.z!=0. {return resample_color(vec2<u32>(position.xy),true,false,vec2(UNCOVERED),vec2(0.),vec2(0.));}
    return resample.backdrop;
}
@fragment fn background_color_kept(@builtin(position) position:vec4<f32>)->@location(0) vec4<f32> {
    return resample_color(vec2<u32>(position.xy),true,true,vec2(UNCOVERED),vec2(0.),vec2(0.));
}
@compute @workgroup_size(8,8)
fn resample_affine_area(@builtin(global_invocation_id) id:vec3<u32>) {
    if any(id.xy>=resample.texels.zw) {return;}
    let t=resample.texels.xy+id.xy;
    if resample.w.x!=0. || resample.w.y!=0. || resample.w.z!=1. {
        textureStore(level,vec2<i32>(t),resample_color(t,false,false,vec2(0.),vec2(0.),vec2(0.)));
        return;
    }
    let h=vec3(vec2<f32>(t)+min(vec2(1.),resample.target_extent.xy-vec2<f32>(t))*.5,1.);
    if !inside(h) {textureStore(level,vec2<i32>(t),composite_color(vec4(resample.options.w)));return;}
    let footprint=min(vec2(1.),resample.target_extent.xy-vec2<f32>(t));
    let center=vec2(dot(resample.x.xyz,h),dot(resample.y.xyz,h));
    let dx=vec2(resample.x.x,resample.y.x)*footprint.x;
    let dy=vec2(resample.x.y,resample.y.y)*footprint.y;
    let taps=vec2<u32>(clamp(floor(vec2(length(dx),length(dy))+.5),vec2(1.),vec2(f32(resample.affine_taps.x))));
    var color=vec4(0.);
    if all(taps==vec2(2u)) {
        color=area_sample(moved,linear_sampler,resample.source_extent.xy,resample.options.w,center,dx*.25,dy*.25);
    } else {
        for (var y=0u;y<taps.y;y++) {for (var x=0u;x<taps.x;x++) {
            let delta=(vec2<f32>(f32(x),f32(y))+.5)/vec2<f32>(taps)-.5;
            color+=border_sample(moved,linear_sampler,resample.source_extent.xy,resample.options.w,center+dx*delta.x+dy*delta.y);
        }}
        color/=f32(taps.x*taps.y);
    }
    textureStore(level,vec2<i32>(t),composite_color(color));
}
