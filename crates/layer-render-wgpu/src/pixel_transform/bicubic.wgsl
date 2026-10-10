fn catmull_rom(t:f32)->vec4<f32> {
    let t2=t*t;let t3=t2*t;
    return vec4(-.5*t3+t2-.5*t,1.5*t3-2.5*t2+1.,-1.5*t3+2.*t2+.5*t,.5*t3-.5*t2);
}
fn bicubic(local:vec2<f32>)->vec4<f32> {
    if far(local,2.) {return outside(brush_selection_at(local));}
    let p=local-vec2(.5);let base=vec2<i32>(floor(p));let t=fract(p);
    let wx=catmull_rom(t.x);let wy=catmull_rom(t.y);
    let view=view_of(base-vec2(1),base+vec2(2));
    var sum=vec4(0.);var low=vec4(3.4e38);var high=vec4(-3.4e38);var brightest=vec3(0.);
    for (var j=0;j<4;j++) {
        var row=vec4(0.);
        for (var i=0;i<4;i++) {
            let value=tap(view,base+vec2(i-1,j-1));
            row+=value*wx[i];
            if (i==1 || i==2) && (j==1 || j==2) {
                low=min(low,value);high=max(high,value);
                if value.a>0. {brightest=max(brightest,value.rgb/value.a);}
            }
        }
        sum+=row*wy[j];
    }
    var value=clamp(sum,low,high);
    if scalar {return clamp(value,vec4(0.),vec4(1.));}
    value.a=clamp(value.a,0.,1.);
    return vec4(min(value.rgb,value.a*brightest),value.a);
}
