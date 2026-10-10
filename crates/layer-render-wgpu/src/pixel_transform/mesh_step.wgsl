fn mesh_step(p:vec2<i32>,axis:vec2<i32>,s:vec2<f32>)->vec2<f32> {
    let after=mesh_position(p+axis);
    let before=mesh_position(p-axis);
    let covered=vec2(after.x>UNCOVERED,before.x>UNCOVERED);
    if all(covered) {return (after-before)*.5;}
    if covered.x {return after-s;}
    if covered.y {return s-before;}
    return vec2(0.);
}
