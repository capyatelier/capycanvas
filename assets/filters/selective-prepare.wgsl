fn capy_prepare_selective(local:vec3<u32>,global:vec3<u32>) {
    var sections=vec2(false);
    for(var page=0u;page<9u;page++) {
        let i=page*4u;
        let correction=vec4(prep_parameter(i,0u).x,prep_parameter(i+1u,0u).x,prep_parameter(i+2u,0u).x,prep_parameter(i+3u,0u).x)*.01;
        prep_store(page+1u,correction);
        if any(correction!=vec4(0.)) {
            if page<6u {sections.x=true;}else{sections.y=true;}
        }
    }
    prep_store(0u,vec4(select(0.,1.,sections.x),select(0.,1.,sections.y),0.,0.));
}
