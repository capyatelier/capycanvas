fn lanczos3(d:f32)->f32 {
    let x=abs(d);
    if x<1e-4 {return 1.;}
    if x>=3. {return 0.;}
    let p=3.14159265*x;
    return 3.*sin(p)*sin(p/3.)/(p*p);
}
fn lanczos(local:vec2<f32>)->vec4<f32> {
    if far(local,3.) {return outside(brush_selection_at(local));}
    let p=local-vec2(.5);let base=vec2<i32>(floor(p));let t=fract(p);
    var wx:array<f32,6>;var wy:array<f32,6>;
    var total=vec2(0.);
    for (var i=0;i<6;i++) {
        wx[i]=lanczos3(t.x-f32(i-2));wy[i]=lanczos3(t.y-f32(i-2));
        total+=vec2(wx[i],wy[i]);
    }
    let view=view_of(base-vec2(2),base+vec2(3));
    var sum=vec4(0.);var low=vec4(3.4e38);var high=vec4(-3.4e38);var brightest=vec3(0.);
    for (var j=0;j<6;j++) {
        var row=vec4(0.);
        for (var i=0;i<6;i++) {
            let value=tap(view,base+vec2(i-2,j-2));
            row+=value*wx[i];
            if (i==2 || i==3) && (j==2 || j==3) {
                low=min(low,value);high=max(high,value);
                if value.a>0. {brightest=max(brightest,value.rgb/value.a);}
            }
        }
        sum+=row*wy[j];
    }
    var value=clamp(sum/(total.x*total.y),low,high);
    if scalar {return clamp(value,vec4(0.),vec4(1.));}
    value.a=clamp(value.a,0.,1.);
    return vec4(min(value.rgb,value.a*brightest),value.a);
}
