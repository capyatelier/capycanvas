fn border_tap(image:texture_2d<f32>,extent:vec2<f32>,outside:f32,p:vec2<i32>)->vec4<f32> {
    if any(p<vec2(0)) || any(p>=vec2<i32>(textureDimensions(image))) {return vec4(outside);}
    let covered=clamp(extent-vec2<f32>(p),vec2(0.),vec2(1.));
    return mix(vec4(outside),textureLoad(image,p,0),covered.x*covered.y);
}
fn border_sample(image:texture_2d<f32>,linear_sampler:sampler,extent:vec2<f32>,outside:f32,q:vec2<f32>)->vec4<f32> {
    let size=vec2<f32>(textureDimensions(image));
    if any(q<=vec2(-.5)) || any(q>=size+.5) {return vec4(outside);}
    if all(q>=vec2(.5)) && all(q<=floor(extent)-.5) {
        return textureSampleLevel(image,linear_sampler,q/size,0.);
    }
    let s=clamp(q-.5,vec2(-2.),size+1.);
    let base=vec2<i32>(floor(s));
    let f=fract(s);
    return mix(mix(border_tap(image,extent,outside,base),border_tap(image,extent,outside,base+vec2(1,0)),f.x),
        mix(border_tap(image,extent,outside,base+vec2(0,1)),border_tap(image,extent,outside,base+vec2(1,1)),f.x),f.y);
}
fn area_sample(image:texture_2d<f32>,linear_sampler:sampler,extent:vec2<f32>,outside:f32,
    center:vec2<f32>,dx:vec2<f32>,dy:vec2<f32>)->vec4<f32> {
    let a=center-dx-dy;let b=center+dx-dy;let c=center-dx+dy;let d=center+dx+dy;
    let low=min(min(a,b),min(c,d));let high=max(max(a,b),max(c,d));
    var sum:vec4<f32>;
    if all(low>=vec2(.5)) && all(high<=floor(extent)-.5) {
        let size=vec2<f32>(textureDimensions(image));
        sum=textureSampleLevel(image,linear_sampler,a/size,0.)
            +textureSampleLevel(image,linear_sampler,b/size,0.)
            +textureSampleLevel(image,linear_sampler,c/size,0.)
            +textureSampleLevel(image,linear_sampler,d/size,0.);
    } else {
        sum=border_sample(image,linear_sampler,extent,outside,a)+border_sample(image,linear_sampler,extent,outside,b)+border_sample(image,linear_sampler,extent,outside,c)+border_sample(image,linear_sampler,extent,outside,d);
    }
    return sum*.25;
}
