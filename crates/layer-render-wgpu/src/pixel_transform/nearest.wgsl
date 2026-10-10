fn nearest(local:vec2<f32>)->vec4<f32> {
    if far(local,.5) {return outside(brush_selection_at(local));}
    return selected(vec2<i32>(floor(local)));
}
