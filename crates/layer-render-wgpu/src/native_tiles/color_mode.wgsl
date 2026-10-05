fn layer_color(value:vec4<f32>,weights:vec3<f32>,mode:f32,threshold:f32)->vec4<f32> {
    if mode==0. {return value;}
    let gray=dot(value.rgb,weights);
    if mode==2. {
        let alpha=select(0.,1.,value.a>=0.5);
        let tone=select(0.,1.,gray>=threshold*value.a);
        return vec4(vec3(tone*alpha),alpha);
    }
    return vec4(vec3(gray),value.a);
}
