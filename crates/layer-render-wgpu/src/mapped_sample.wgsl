struct Resample {
    x:vec4<f32>, y:vec4<f32>, w:vec4<f32>,
    kept_x:vec4<f32>, kept_y:vec4<f32>, kept_w:vec4<f32>,
    clip_x:vec4<f32>, clip_y:vec4<f32>, extent:vec4<f32>,
    texels:vec4<u32>, backdrop:vec4<f32>, options:vec4<f32>,
    source_extent:vec4<f32>, target_extent:vec4<f32>,
    affine_taps:vec4<u32>,
    encoding:vec4<f32>,
}
fn bilinear(image:texture_2d<f32>,x:vec4<f32>,y:vec4<f32>,w:vec4<f32>,h:vec3<f32>,lod:f32)->vec4<f32> {
    let q=vec3(dot(x.xyz,h),dot(y.xyz,h),dot(w.xyz,h));
    if q.z<=0. {return vec4(0.);}
    return bilinear_at(image,q.xy/q.z,lod);
}
fn footprint_lod(dx:vec2<f32>,dy:vec2<f32>)->f32 {
    let stretch=(length(vec2(dx.x+dy.y,dx.y-dy.x))+length(vec2(dx.x-dy.y,dx.y+dy.x)))*.5;
    return log2(max(stretch,1.));
}
fn projective_lod(x:vec4<f32>,y:vec4<f32>,w:vec4<f32>,h:vec3<f32>,dx:vec2<f32>,dy:vec2<f32>)->f32 {
    let q=vec3(dot(x.xyz,h),dot(y.xyz,h),dot(w.xyz,h));
    let denominator=max(q.z*q.z,1e-20);
    let u=(vec2(x.x,y.x)*q.z-q.xy*w.x)/denominator;
    let v=(vec2(x.y,y.y)*q.z-q.xy*w.y)/denominator;
    return footprint_lod(u*dx.x+v*dx.y,u*dy.x+v*dy.y);
}
fn bilinear_at(image:texture_2d<f32>,q:vec2<f32>,lod:f32)->vec4<f32> {
    return pyramid_sample(image,linear_sampler,resample.source_extent.xy,resample.options.w,q,lod,resample.options.y);
}
fn inside(h:vec3<f32>)->bool {
    let layer_pixel=vec2(dot(resample.clip_x.xyz,h),dot(resample.clip_y.xyz,h));
    return all(layer_pixel>=vec2(0.)) && all(layer_pixel<resample.extent.xy);
}
fn positive_footprint(plane:vec3<f32>,center:vec3<f32>,dx:vec2<f32>,dy:vec2<f32>)->bool {
    return dot(plane,center)>=(abs(dot(plane.xy,dx))+abs(dot(plane.xy,dy)))*.25;
}
fn negative_footprint(plane:vec3<f32>,center:vec3<f32>,dx:vec2<f32>,dy:vec2<f32>)->bool {
    return dot(plane,center)<-(abs(dot(plane.xy,dx))+abs(dot(plane.xy,dy)))*.25;
}
fn source_covers(center:vec2<f32>,dx:vec2<f32>,dy:vec2<f32>)->bool {
    let h=vec3(center,1.);
    return positive_footprint(resample.w.xyz,h,dx,dy) && positive_footprint(resample.x.xyz,h,dx,dy)
        && positive_footprint(resample.y.xyz,h,dx,dy)
        && positive_footprint(resample.w.xyz*resample.source_extent.x-resample.x.xyz,h,dx,dy)
        && positive_footprint(resample.w.xyz*resample.source_extent.y-resample.y.xyz,h,dx,dy);
}
fn source_disjoint(center:vec2<f32>,dx:vec2<f32>,dy:vec2<f32>)->bool {
    let h=vec3(center,1.);
    return negative_footprint(resample.w.xyz,h,dx,dy) || negative_footprint(resample.x.xyz,h,dx,dy)
        || negative_footprint(resample.y.xyz,h,dx,dy)
        || negative_footprint(resample.w.xyz*resample.source_extent.x-resample.x.xyz,h,dx,dy)
        || negative_footprint(resample.w.xyz*resample.source_extent.y-resample.y.xyz,h,dx,dy);
}
fn remainder(h:vec3<f32>,lod:f32,keeps:bool)->vec4<f32> {
    var color=vec4(0.);
    if keeps {color=bilinear(kept,resample.kept_x,resample.kept_y,resample.kept_w,h,lod);}
    if resample.options.z!=0. {color+=bilinear(moved,resample.kept_x,resample.kept_y,resample.kept_w,h,lod);}
    return color;
}
const UNCOVERED=-1e38;
fn mapped_color(taps:u32,center:vec2<f32>,dx:vec2<f32>,dy:vec2<f32>,mesh:bool,keeps:bool,q:vec2<f32>,source_dx:vec2<f32>,source_dy:vec2<f32>)->vec4<f32> {
    let count=f32(taps);
    let reach=(1.-1./count)*.5;
    let sample_dx=dx*(reach*4.);let sample_dy=dy*(reach*4.);
    let layer_center=vec2(dot(resample.clip_x.xyz,vec3(center,1.)),dot(resample.clip_y.xyz,vec3(center,1.)));
    let margin=vec2(abs(dot(resample.clip_x.xy,dx))+abs(dot(resample.clip_x.xy,dy)),
        abs(dot(resample.clip_y.xy,dx))+abs(dot(resample.clip_y.xy,dy)))*reach;
    if any(layer_center+margin<vec2(0.)) || any(layer_center-margin>=resample.extent.xy) {return vec4(resample.options.w);}
    var moved_lod=projective_lod(resample.x,resample.y,resample.w,vec3(center,1.),dx,dy);
    if mesh {moved_lod=footprint_lod(source_dx*dx.x+source_dy*dx.y,source_dx*dy.x+source_dy*dy.y);}
    var covered=source_covers(center,sample_dx,sample_dy);
    if mesh {
        let reach=(abs(source_dx*dx.x+source_dy*dx.y)+abs(source_dx*dy.x+source_dy*dy.y))*.25;
        covered=all(q-reach>=vec2(0.)) && all(q+reach<resample.source_extent.xy);
    }
    let inside_clip=all(layer_center-margin>=vec2(0.)) && all(layer_center+margin<resample.extent.xy);
    var original_lod=0.;
    if keeps || resample.options.z!=0. {
        original_lod=projective_lod(resample.kept_x,resample.kept_y,resample.kept_w,vec3(center,1.),dx,dy);
    }
    if covered && inside_clip && resample.options.z!=2. {
        var color=vec4(0.);
        if mesh {
            if q.x>UNCOVERED {color=bilinear_at(moved,q,moved_lod);}
        } else {color=bilinear(moved,resample.x,resample.y,resample.w,vec3(center,1.),moved_lod);}
        if (!keeps && resample.options.z==0.) || color.a==1. {return color;}
        if color.a==0. {return remainder(vec3(center,1.),original_lod,keeps);}
    }
    var empty=source_disjoint(center,sample_dx,sample_dy);
    if mesh {empty=q.x<=UNCOVERED;}
    if inside_clip && (empty || resample.options.z==2.) {
        return remainder(vec3(center,1.),original_lod,keeps);
    }
    var sum=vec4(0.);
    moved_lod=max(moved_lod-log2(count),0.);
    let kept_lod=max(original_lod-log2(count),0.);
    for (var y=0u;y<taps;y++) {for (var x=0u;x<taps;x++) {
        let delta=dx*((f32(x)+.5)/count-.5)+dy*((f32(y)+.5)/count-.5);
        let h=vec3(center+delta,1.);
        var color=vec4(resample.options.w);
        if inside(h) {
            if mesh {
                color=select(bilinear_at(moved,q+source_dx*delta.x+source_dy*delta.y,moved_lod),vec4(0.),q.x<=UNCOVERED);
            } else {color=bilinear(moved,resample.x,resample.y,resample.w,h,moved_lod);}
            let under=remainder(h,kept_lod,keeps);
            if resample.options.z==2. {color=under;} else {color+=under*(1.-color.a);}
        }
        sum+=color;
    }}
    return sum/(count*count);
}
