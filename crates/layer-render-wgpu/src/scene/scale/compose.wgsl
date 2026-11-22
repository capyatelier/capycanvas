fn composition_pixel(image:texture_2d<f32>,p:vec2<i32>)->vec4<f32> {
    if any(p<vec2(0)) || any(p>=vec2<i32>(textureDimensions(image))) {return vec4(0.);}
    return textureLoad(image,p,0);
}
@compute @workgroup_size(8, 8)
fn compose(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= region.size) { return; }
    let p = vec2<i32>(region.origin + id.xy);
    var below = region.paper;
    if (region.flags & 2u) != 0u {
        below = composition_pixel(base, p + region.base_origin);
        if (region.flags & 512u) != 0u {
            let preview = composition_pixel(base_preview, p + region.base_origin);
            below = preview + below * (1. - preview.a);
        }
        if (region.flags & 2048u) != 0u { below = working_encode(below); }
        if region.opacity.z!=1. {below *= region.opacity.z;}
    }
    let coordinate = p + region.source_origin;
    var color = composition_pixel(source, coordinate);
    if (region.flags & 256u) != 0u {
        let preview = composition_pixel(source_preview, p + region.source_origin);
        color = preview + color * (1. - preview.a);
    }
    if (region.flags & 1024u) != 0u { color = working_encode(color); }
    if region.opacity.x!=1. {color *= region.opacity.x;}
    if (region.flags & 4096u) != 0u {
        textureStore(output, p, display_value(working_decode(color)));
        return;
    }
    if (region.flags & 128u) != 0u {
        textureStore(output, p, display_value(color + below));
        return;
    }
    if (region.flags & 32u) != 0u {
        textureStore(output, p, display_value(color * below.a));
        return;
    }
    if (region.flags & 64u) != 0u {
        color = vec4<f32>(.46, .12, .8, 1.) * ((1. - color.a) * .42);
        if (region.flags & 8192u) != 0u { color = working_encode(color); }
    }
    let mode = u32(region.opacity.y);
    let clipped = (region.flags & 16u) != 0u;
    if mode == 0u && !clipped {
        textureStore(output, p, display_value(color + below * (1. - color.a)));
        return;
    }
    textureStore(output, p, display_value(blend_layer(color, below, mode, clipped)));
}
