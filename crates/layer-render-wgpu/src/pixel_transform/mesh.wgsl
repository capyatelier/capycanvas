fn mesh_position(p:vec2<i32>)->vec2<f32> {
    let size=vec2<i32>(textureDimensions(source15));
    let owner=textureLoad(source15,clamp(p,vec2(0),size-1),0).xy;
    if owner.x<=UNCOVERED {return vec2(UNCOVERED);}
    let s=source_position(owner);
    if s.z<=0. {return vec2(UNCOVERED);}
    return s.xy;
}
