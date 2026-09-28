@compute @workgroup_size(8, 8)
fn compose(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= region.size) { return; }
    let p = vec2<i32>(region.origin + id.xy);
    var below = region.paper;
    if (region.flags & 2u) != 0u { below = textureLoad(base, p, 0) * region.opacity.z; }
    var color = textureLoad(source, p, 0) * region.opacity.x;
    if (region.flags & 32u) != 0u {
        textureStore(output, p, color * below.a);
        return;
    }
    if (region.flags & 64u) != 0u {
        color = vec4<f32>(.46, .12, .8, 1.) * ((1. - color.a) * .42);
    }
    let mode = u32(region.opacity.y);
    let clipped = (region.flags & 16u) != 0u;
    if mode == 0u && !clipped {
        textureStore(output, p, color + below * (1. - color.a));
        return;
    }
    let mixed = blend(working_unassociate(color), working_unassociate(below), mode);
    if clipped {
        textureStore(output, p, vec4<f32>(mix(below.rgb, mixed * below.a, color.a), below.a));
    } else {
        let rgb = (1. - color.a) * below.rgb + (1. - below.a) * color.rgb + color.a * below.a * mixed;
        textureStore(output, p, vec4<f32>(rgb, color.a + below.a * (1. - color.a)));
    }
}
