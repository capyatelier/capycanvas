@compute @workgroup_size(8, 8)
fn compose(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= region.size) { return; }
    let p = vec2<i32>(region.origin + id.xy);
    var below = region.paper;
    if (region.flags & 2u) != 0u { below = textureLoad(base, p, 0) * region.opacity.z; }
    var color = textureLoad(source, p, 0) * region.opacity.x;
    if (region.flags & 128u) != 0u {
        textureStore(output, p, color + below);
        return;
    }
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
    if clipped {
        textureStore(output, p, blend_clip(color, below, mode));
    } else {
        textureStore(output, p, blend_composite(color, below, mode));
    }
}
