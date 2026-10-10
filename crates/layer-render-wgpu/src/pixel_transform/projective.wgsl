fn source_position(world:vec2<f32>)->vec3<f32> {
    let h=vec3(world,1.);
    let w=dot(transform.w.xyz,h);
    return vec3(vec2(dot(transform.x.xyz,h),dot(transform.y.xyz,h))/w,w);
}
