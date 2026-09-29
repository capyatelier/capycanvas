struct Resample {
    x:vec4<f32>, y:vec4<f32>, w:vec4<f32>,
    kept_x:vec4<f32>, kept_y:vec4<f32>, kept_w:vec4<f32>,
    clip_x:vec4<f32>, clip_y:vec4<f32>, extent:vec4<f32>,
    texels:vec4<u32>, backdrop:vec4<f32>, options:vec4<f32>,
    source_extent:vec4<f32>, target_extent:vec4<f32>,
    affine_taps:vec4<u32>,
    encoding:vec4<f32>,
}
@group(0) @binding(0) var moved:texture_2d<f32>;
@group(0) @binding(1) var level:texture_storage_2d<rgba32float,write>;
@group(0) @binding(2) var<uniform> resample:Resample;
@group(0) @binding(3) var kept:texture_2d<f32>;
@group(0) @binding(4) var linear_sampler:sampler;

fn bilinear(image:texture_2d<f32>,x:vec4<f32>,y:vec4<f32>,w:vec4<f32>,h:vec3<f32>,lod:f32)->vec4<f32> {
    let q=vec3(dot(x.xyz,h),dot(y.xyz,h),dot(w.xyz,h));
    if q.z<=0. {return vec4(0.);}
    return bilinear_at(image,q.xy/q.z,lod);
}
fn footprint_lod(dx:vec2<f32>,dy:vec2<f32>)->f32 {
    let stretch=(length(vec2(dx.x+dy.y,dx.y-dy.x))+length(vec2(dx.x-dy.y,dx.y+dy.x)))*.5;
    return log2(max(stretch,1.));
}
fn projective_lod(x:vec4<f32>,y:vec4<f32>,w:vec4<f32>,h:vec3<f32>,footprint:vec2<f32>)->f32 {
    let q=vec3(dot(x.xyz,h),dot(y.xyz,h),dot(w.xyz,h));
    let denominator=max(q.z*q.z,1e-20);
    return footprint_lod((vec2(x.x,y.x)*q.z-q.xy*w.x)*footprint.x/denominator,
        (vec2(x.y,y.y)*q.z-q.xy*w.y)*footprint.y/denominator);
}
fn bilinear_at(image:texture_2d<f32>,q:vec2<f32>,lod:f32)->vec4<f32> {
    return pyramid_sample(image,linear_sampler,resample.source_extent.xy,resample.options.w,q,lod,resample.options.y);
}
fn composite_color(color:vec4<f32>)->vec4<f32> {
    var layer=color*resample.options.x;
    if resample.encoding.x!=0. {layer=working_encode(color)*resample.options.x;}
    return layer+resample.backdrop*(1.-layer.a);
}
fn inside(h:vec3<f32>)->bool {
    let layer_pixel=vec2(dot(resample.clip_x.xyz,h),dot(resample.clip_y.xyz,h));
    return all(layer_pixel>=vec2(0.)) && all(layer_pixel<resample.extent.xy);
}
fn positive_footprint(plane:vec3<f32>,center:vec3<f32>,radius:vec2<f32>)->bool {
    return dot(plane,center)>=dot(abs(plane.xy),radius);
}
fn negative_footprint(plane:vec3<f32>,center:vec3<f32>,radius:vec2<f32>)->bool {
    return dot(plane,center)<-dot(abs(plane.xy),radius);
}
fn source_covers(center:vec2<f32>,footprint:vec2<f32>)->bool {
    let h=vec3(center,1.);let radius=footprint*.25;
    return positive_footprint(resample.w.xyz,h,radius) && positive_footprint(resample.x.xyz,h,radius)
        && positive_footprint(resample.y.xyz,h,radius)
        && positive_footprint(resample.w.xyz*resample.source_extent.x-resample.x.xyz,h,radius)
        && positive_footprint(resample.w.xyz*resample.source_extent.y-resample.y.xyz,h,radius);
}
fn source_disjoint(center:vec2<f32>,footprint:vec2<f32>)->bool {
    let h=vec3(center,1.);let radius=footprint*.25;
    return negative_footprint(resample.w.xyz,h,radius) || negative_footprint(resample.x.xyz,h,radius)
        || negative_footprint(resample.y.xyz,h,radius)
        || negative_footprint(resample.w.xyz*resample.source_extent.x-resample.x.xyz,h,radius)
        || negative_footprint(resample.w.xyz*resample.source_extent.y-resample.y.xyz,h,radius);
}
fn remainder(h:vec3<f32>,lod:f32,keeps:bool)->vec4<f32> {
    var color=vec4(0.);
    if keeps {color=bilinear(kept,resample.kept_x,resample.kept_y,resample.kept_w,h,lod);}
    if resample.options.z!=0. {color+=bilinear(moved,resample.kept_x,resample.kept_y,resample.kept_w,h,lod);}
    return color;
}
const UNCOVERED=-1e38;
fn resample_color(t:vec2<u32>,mesh:bool,keeps:bool,source:vec2<f32>,dx:vec2<f32>,dy:vec2<f32>)->vec4<f32> {
    let footprint=min(vec2(1.),resample.target_extent.xy-vec2<f32>(t));
    let center=vec2<f32>(t)+footprint*.5;
    let q=source+dx*(footprint.x-1.)*.5+dy*(footprint.y-1.)*.5;
    var moved_lod=projective_lod(resample.x,resample.y,resample.w,vec3(center,1.),footprint);
    if mesh {moved_lod=footprint_lod(dx*footprint.x,dy*footprint.y);}
    let layer_center=vec2(dot(resample.clip_x.xyz,vec3(center,1.)),dot(resample.clip_y.xyz,vec3(center,1.)));
    let margin=vec2(dot(abs(resample.clip_x.xy),footprint),dot(abs(resample.clip_y.xy),footprint))*.25;
    var covered=source_covers(center,footprint);
    if mesh {
        let reach=(abs(dx)*footprint.x+abs(dy)*footprint.y)*.25;
        covered=all(q-reach>=vec2(0.)) && all(q+reach<resample.source_extent.xy);
    }
    let inside_clip=all(layer_center-margin>=vec2(0.)) && all(layer_center+margin<resample.extent.xy);
    var original_lod=0.;
    if keeps || resample.options.z!=0. {
        original_lod=projective_lod(resample.kept_x,resample.kept_y,resample.kept_w,vec3(center,1.),footprint);
    }
    if covered && inside_clip && resample.options.z!=2. {
        var color=vec4(0.);
        if mesh {
            if q.x>UNCOVERED {color=bilinear_at(moved,q,moved_lod);}
        } else {color=bilinear(moved,resample.x,resample.y,resample.w,vec3(center,1.),moved_lod);}
        if (!keeps && resample.options.z==0.) || color.a==1. {return composite_color(color);}
        if color.a==0. {return composite_color(remainder(vec3(center,1.),original_lod,keeps));}
    }
    var empty=source_disjoint(center,footprint);
    if mesh {empty=q.x<=UNCOVERED;}
    if inside_clip && (empty || resample.options.z==2.) {
        return composite_color(remainder(vec3(center,1.),original_lod,keeps));
    }
    var sum=vec4(0.);
    moved_lod=max(moved_lod-1.,0.);
    let kept_lod=max(original_lod-1.,0.);
    for (var y=0u;y<2u;y++) {for (var x=0u;x<2u;x++) {
        let delta=(vec2<f32>(f32(x),f32(y))-.5)*footprint*.5;
        let h=vec3(center+delta,1.);
        var color=vec4(resample.options.w);
        if inside(h) {
            if mesh {
                color=select(bilinear_at(moved,q+dx*delta.x+dy*delta.y,moved_lod),vec4(0.),q.x<=UNCOVERED);
            } else {color=bilinear(moved,resample.x,resample.y,resample.w,h,moved_lod);}
            let under=remainder(h,kept_lod,keeps);
            if resample.options.z==2. {color=under;} else {color+=under*(1.-color.a);}
        }
        sum+=color;
    }}
    return composite_color(sum*.25);
}
fn resample_area(id:vec3<u32>,keeps:bool) {
    if any(id.xy>=resample.texels.zw) {return;}
    let t=resample.texels.xy+id.xy;
    textureStore(level,vec2<i32>(t),resample_color(t,false,keeps,vec2(0.),vec2(0.),vec2(0.)));
}
@compute @workgroup_size(8,8)
fn resample_projective(@builtin(global_invocation_id) id:vec3<u32>) {resample_area(id,false);}
@compute @workgroup_size(8,8)
fn resample_projective_kept(@builtin(global_invocation_id) id:vec3<u32>) {resample_area(id,true);}

struct MeshVertex { @builtin(position) position:vec4<f32>, @location(0) @interpolate(linear) source:vec2<f32> }
@vertex fn mesh_vertex(@location(0) destination:vec2<f32>, @location(1) source:vec2<f32>)->MeshVertex {
    let x=resample.clip_x;let y=resample.clip_y;
    let p=destination-vec2(x.z,y.z);
    let output=vec2(y.y*p.x-x.y*p.y,x.x*p.y-y.x*p.x)/(x.x*y.y-x.y*y.x);
    let normalized=output/ceil(resample.target_extent.xy);
    return MeshVertex(vec4(normalized.x*2.-1.,1.-normalized.y*2.,0.,1.),source*resample.source_extent.xy/resample.extent.xy);
}
@vertex fn background_vertex(@builtin(vertex_index) index:u32)->@builtin(position) vec4<f32> {
    let p=vec2(f32((index<<1u)&2u),f32(index&2u));
    return vec4(p*2.-1.,0.,1.);
}
@fragment fn mesh_color(vertex:MeshVertex)->@location(0) vec4<f32> {
    return resample_color(vec2<u32>(vertex.position.xy),true,false,vertex.source,dpdx(vertex.source),dpdy(vertex.source));
}
@fragment fn mesh_color_kept(vertex:MeshVertex)->@location(0) vec4<f32> {
    return resample_color(vec2<u32>(vertex.position.xy),true,true,vertex.source,dpdx(vertex.source),dpdy(vertex.source));
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
    let h=vec3(vec2<f32>(t)+min(vec2(1.),resample.target_extent.xy-vec2<f32>(t))*.5,1.);
    if !inside(h) {textureStore(level,vec2<i32>(t),composite_color(vec4(resample.options.w)));return;}
    let footprint=min(vec2(1.),resample.target_extent.xy-vec2<f32>(t));
    let center=vec2(dot(resample.x.xyz,h),dot(resample.y.xyz,h));
    let dx=vec2(resample.x.x,resample.y.x)*footprint.x;
    let dy=vec2(resample.x.y,resample.y.y)*footprint.y;
    let taps=resample.affine_taps.x;
    var color=vec4(0.);
    if taps==2u {
        color=area_sample(moved,linear_sampler,resample.source_extent.xy,resample.options.w,center,dx*.25,dy*.25);
    } else {
        for (var y=0u;y<taps;y++) {for (var x=0u;x<taps;x++) {
            let delta=(vec2<f32>(f32(x),f32(y))+.5)/f32(taps)-.5;
            color+=border_sample(moved,linear_sampler,resample.source_extent.xy,resample.options.w,center+dx*delta.x+dy*delta.y);
        }}
        color/=f32(taps*taps);
    }
    textureStore(level,vec2<i32>(t),composite_color(color));
}
