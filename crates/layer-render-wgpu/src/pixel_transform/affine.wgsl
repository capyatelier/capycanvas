fn source_position(world:vec2<f32>)->vec3<f32> {
    let h=vec3(world,1.);
    return vec3(dot(transform.x.xyz,h),dot(transform.y.xyz,h),1.);
}
