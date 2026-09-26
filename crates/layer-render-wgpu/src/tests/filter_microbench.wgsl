// Diagnostic workload only. Two separable RGBA32F passes; same paired Gaussian.
@group(0) @binding(0) var input: texture_2d<f32>;
@group(0) @binding(1) var sampling:sampler;
@group(0) @binding(2) var<storage,read> taps:array<vec4<f32>>;
@vertex fn vertex(@builtin(vertex_index) i:u32)->@builtin(position) vec4<f32> {
    let xy=array<vec2<f32>,3>(vec2(-1.,-1.),vec2(3.,-1.),vec2(-1.,3.));
    return vec4(xy[i],0.,1.);
}
fn load(p:vec2<i32>)->vec4<f32> {return textureLoad(input,clamp(p,vec2(0),vec2<i32>(textureDimensions(input))-1),0);}
fn sample_manual(point:vec2<f32>)->vec4<f32> {
    let p=point-.5;let origin=vec2<i32>(floor(p));let f=fract(p);
    return mix(mix(load(origin),load(origin+vec2(1,0)),f.x),mix(load(origin+vec2(0,1)),load(origin+vec2(1,1)),f.x),f.y);
}
fn sample_axis(point:vec2<f32>)->vec4<f32> {
    let p=point-.5;let origin=vec2<i32>(floor(p));let f=dot(fract(p),AXIS);
    return mix(load(origin),load(origin+vec2<i32>(AXIS)),f);
}
fn sample_hardware(p:vec2<f32>)->vec4<f32> {return textureSampleLevel(input,sampling,p/vec2<f32>(textureDimensions(input)),0.);}
fn sample_copy(p:vec2<f32>)->vec4<f32>{return load(vec2<i32>(p));}
fn sample_discrete(p:vec2<f32>)->vec4<f32>{return load(vec2<i32>(p));}
fn sample_warp(p:vec2<f32>)->vec4<f32>{return sample_manual(p);}
fn random(p:vec2<f32>,seed:u32)->f32 {
    let q=vec2<u32>(vec2<i32>(floor(p)));var h=q.x*1664525u+q.y*1013904223u+seed*747796405u;
    h=(h^(h>>15u))*2246822519u;return f32(h^(h>>13u))/4294967295.;
}
fn noise(p:vec2<f32>,seed:u32)->f32 {
    let q=floor(p);let f=fract(p);let t=f*f*(3.-2.*f);
    return mix(mix(random(q,seed),random(q+vec2(1.,0.),seed),t.x),mix(random(q+vec2(0.,1.),seed),random(q+1.,seed),t.x),t.y)*2.-1.;
}
fn fbm(p:vec2<f32>,seed:u32)->f32 {
    var q=p;var total=0.;var weight=.5;var norm=0.;
    for(var i=0u;i<3u;i+=1u){total+=noise(q,seed+i)*weight;norm+=weight;q=vec2(q.x*.8-q.y*.6,q.x*.6+q.y*.8)*2.07+7.3;weight*=.5;}
    return total/max(norm,.00001);
}
@fragment fn fragment(@builtin(position) p:vec4<f32>)->@location(0) vec4<f32> {
    // String replacement specializes only the benchmark variant and pass axis.
    if IS_COPY {return sample_copy(p.xy);}
    if IS_WARP {
        let q=p.xy/96.+vec2(.17,-.23);
        let bend=vec2(fbm(q,3u),fbm(q+19.3,31u));
        let warp=vec2(fbm(q+bend*1.4,71u),fbm(q+bend*1.4+7.9,113u));
        return sample_manual(p.xy+warp*24.);
    }
    if IS_DISCRETE {
        var sum=sample_copy(p.xy)*taps[34].x;
        for(var i=1u;i<=u32(taps[0].z);i+=1u){let offset=AXIS*f32(i);sum+=(sample_copy(p.xy-offset)+sample_copy(p.xy+offset))*taps[34u+i].x;}
        return sum;
    }
    var sum=sample_VARIANT(p.xy)*taps[0].x;
    for(var i=1u;i<=u32(taps[0].y);i+=1u){let tap=taps[i];let offset=AXIS*tap.x;sum+=(sample_VARIANT(p.xy-offset)+sample_VARIANT(p.xy+offset))*tap.y;}
    return sum;
}
