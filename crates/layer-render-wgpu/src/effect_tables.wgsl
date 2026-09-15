// ABI 3 parameter tables: exact Hermite segments and gradient stops. Bounded
// binary search replaces the old 256-sample approximation at control knots.
fn fx_lut(base:u32,offset:u32,value:f32)->vec4<f32> {
    let start=base+1u+offset;let header=effect_data[start];
    let count=u32(header.x);
    var x=value;
    if !FX_EXTENDED {x=clamp(x,0.,1.);}
    if header.z==1. {
        if header.y==1. {return vec4<f32>(x,0.,0.,0.);}
        var low=0u;var high=count-1u;
        // Find the first segment whose upper endpoint contains x.
        for(var step=0u;step<5u;step++) {
            if low>=high {break;}
            let mid=(low+high)/2u;
            if x<=effect_data[start+1u+2u*mid].y {high=mid;} else {low=mid+1u;}
        }
        let b=effect_data[start+1u+2u*low];let c=effect_data[start+2u+2u*low];
        let t=(x-b.x)/(b.y-b.x);
        var y:f32;
        if t<0. {y=c.x+t*c.y;}
        else if t>1. {y=b.z+(t-1.)*b.w;}
        else if t==0. {y=c.x;}
        else if t==1. {y=b.z;}
        else {y=clamp(((c.w*t+c.z)*t+c.y)*t+c.x,min(c.x,b.z),max(c.x,b.z));}
        return vec4<f32>(y,0.,0.,0.);
    }
    // Gradients deliberately extend endpoint colors. Stops interpolate exact
    // encoded document RGB and alpha, with no resampled table between them.
    x=clamp(x,0.,1.);
    var low=0u;var high=count-2u;
    for(var step=0u;step<5u;step++) {
        if low>=high {break;}
        let mid=(low+high)/2u;
        if x<=effect_data[start+3u+2u*mid].x {high=mid;} else {low=mid+1u;}
    }
    let a=effect_data[start+1u+2u*low];let b=effect_data[start+3u+2u*low];
    let ca=vec4<f32>(a.yzw,effect_data[start+2u+2u*low].x);
    let cb=vec4<f32>(b.yzw,effect_data[start+4u+2u*low].x);
    let t=clamp((x-a.x)/(b.x-a.x),0.,1.);
    if t==0. {return ca;} if t==1. {return cb;}
    return mix(ca,cb,t);
}
