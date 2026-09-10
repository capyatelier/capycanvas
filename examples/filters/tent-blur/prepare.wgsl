// Original triangular (non-Gaussian) kernel, normalized once per radius edit.
fn prepare_tent(local:vec3<u32>, global:vec3<u32>) {
    let radius=u32(prep_parameter(0u,0u).x);
    let i=global.x;
    if i>16u {return;}
    let width=f32(radius+1u);
    let weight=max(width-f32(i),0.)/(width*width);
    prep_store(i,vec4<f32>(weight,f32(radius),0.,0.));
}
