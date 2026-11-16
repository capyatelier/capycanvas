fn scene_convert(c:vec4<f32>,convert:f32)->vec4<f32> {
    if convert==1. {return working_encode(c);}
    var r=c;
    if convert==2. || convert==4. {r=working_decode(c);}
    if convert==3. || convert==4. {r=working_clamp(r);}
    return r;
}
