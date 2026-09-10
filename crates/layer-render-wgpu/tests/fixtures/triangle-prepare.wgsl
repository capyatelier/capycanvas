// Original test kernel. Editable at runtime; same paired-tap consumer ABI.
fn triangle(local:vec3<u32>, global:vec3<u32>) {
    let radius=min(u32(ceil(prep_parameter(0u,0u).x)),63u);
    let total=f32((radius+1u)*(radius+1u));
    prep_store(0u,vec4<f32>(f32(radius+1u)/total,f32((radius+1u)/2u),0.,0.));
    for(var pair=0u;pair<32u;pair+=1u) {
        let j=pair*2u+1u;
        var value=vec4<f32>(0.);
        if j<=radius {
            let a=f32(radius+1u-j);let b=max(a-1.,0.);
            value=vec4<f32>(f32(j)+b/(a+b),(a+b)/total,0.,0.);
        }
        prep_store(pair+1u,value);
    }
}
