fn bilinear(local:vec2<f32>)->vec4<f32> {
    if far(local,.5) {return outside(brush_selection_at(local));}
    let p=local-vec2(.5);let base=vec2<i32>(floor(p));let t=fract(p);
    let view=view_of(base,base+vec2(1));
    return mix(mix(tap(view,base),tap(view,base+vec2(1,0)),t.x),
        mix(tap(view,base+vec2(0,1)),tap(view,base+vec2(1,1)),t.x),t.y);
}
