fn fx_cube_transform(v:vec3<f32>,space:u32,forward:bool)->vec3<f32> {
    if forward {
        switch space {case 0u:{return cube_to_0(v);} case 1u:{return cube_to_1(v);} case 2u:{return cube_to_2(v);} default:{return cube_to_3(v);}}
    }
    switch space {case 0u:{return cube_from_0(v);} case 1u:{return cube_from_1(v);} case 2u:{return cube_from_2(v);} default:{return cube_from_3(v);}}
}
fn fx_cube_limit(space:u32)->f32 {
    switch space {case 0u:{return CUBE_LIMIT_0;} case 1u:{return CUBE_LIMIT_1;} case 2u:{return CUBE_LIMIT_2;} default:{return CUBE_LIMIT_3;}}
}
fn fx_cube_max(v:vec3<f32>)->f32 {return max(max(abs(v.x),abs(v.y)),abs(v.z));}
fn fx_cube_coordinate(value:f32,axis:u32)->f32 {
    let lo=fx_auxiliary(1u)[axis]; let hi=fx_auxiliary(2u)[axis];
    if value<=lo {return 0.;} if value>=hi {return 1.;}
    return clamp((ldexp(value,i32(fx_auxiliary(3u)[axis]))-fx_auxiliary(4u)[axis])*fx_auxiliary(5u)[axis],0.,1.);
}
fn fx_cube_component(index:u32)->f32 {return fx_auxiliary(6u+index/4u)[index%4u];}
fn fx_cube_at(p:vec3<u32>,size:u32)->vec3<f32> {
    let index=((p.z*size+p.y)*size+p.x)*3u;
    return vec3(fx_cube_component(index),fx_cube_component(index+1u),fx_cube_component(index+2u));
}
fn fx_cube(encoded:vec3<f32>)->vec3<f32> {
    let size=u32(fx_auxiliary(0u).x);
    let coordinate=vec3(fx_cube_coordinate(encoded.x,0u),fx_cube_coordinate(encoded.y,1u),fx_cube_coordinate(encoded.z,2u))*f32(size-1u);
    let low=min(vec3<u32>(coordinate),vec3(size-2u));
    let tetra=tetrahedron(coordinate-vec3<f32>(low));
    let a=fx_cube_at(low,size);let b=fx_cube_at(low+tetra.first,size);
    let c=fx_cube_at(low+tetra.first+tetra.second,size);let d=fx_cube_at(low+vec3(1u),size);
    return a+tetra.weights.x*(b-a)+tetra.weights.y*(c-b)+tetra.weights.z*(d-c);
}
