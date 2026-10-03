struct Tetrahedron { first:vec3<u32>, second:vec3<u32>, weights:vec3<f32> }
fn tetrahedron(t:vec3<f32>)->Tetrahedron {
    var first = vec3<u32>(1u, 0u, 0u);
    var second = vec3<u32>(0u, 1u, 0u);
    var a = t.x;
    var b = t.y;
    var c = t.z;
    if t.x >= t.y {
        if t.y >= t.z { }
        else if t.x >= t.z {
            second = vec3<u32>(0u, 0u, 1u);
            b = t.z; c = t.y;
        } else {
            first = vec3<u32>(0u, 0u, 1u);
            second = vec3<u32>(1u, 0u, 0u);
            a = t.z; b = t.x; c = t.y;
        }
    } else {
        if t.x >= t.z {
            first = vec3<u32>(0u, 1u, 0u);
            second = vec3<u32>(1u, 0u, 0u);
            a = t.y; b = t.x;
        } else if t.y >= t.z {
            first = vec3<u32>(0u, 1u, 0u);
            second = vec3<u32>(0u, 0u, 1u);
            a = t.y; b = t.z; c = t.x;
        } else {
            first = vec3<u32>(0u, 0u, 1u);
            a = t.z; c = t.x;
        }
    }
    return Tetrahedron(first,second,vec3(a,b,c));
}
