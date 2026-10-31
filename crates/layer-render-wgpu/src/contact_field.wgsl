
override CONTACT_FLAGS: u32 = 4294967295u;
override CONTACT_ABSENT: u32 = 0u;
fn contact_feature(bit: u32, dynamic: bool) -> bool {
    return (CONTACT_ABSENT & bit) == 0u && select((CONTACT_FLAGS & bit) != 0u, dynamic, CONTACT_FLAGS == 4294967295u);
}

fn contact_uniform() -> bool {
    return contact_feature(128u, style.render_mode.y > 0.5);
}

fn contact_hash(p: vec2<f32>) -> f32 {
    let cell = vec2<i32>(p);
    var h = (bitcast<u32>(cell.x) * 1597334677u) ^ (bitcast<u32>(cell.y) * 3812015801u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return f32(h ^ (h >> 16u)) / 4294967295.0;
}

fn contact_noise(p: vec2<f32>) -> f32 {
    let cell = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(contact_hash(cell), contact_hash(cell + vec2<f32>(1.0, 0.0)), u.x),
        mix(contact_hash(cell + vec2<f32>(0.0, 1.0)), contact_hash(cell + vec2<f32>(1.0)), u.x), u.y);
}

fn contact_paper(world: vec2<f32>) -> f32 {
    if contact_feature(2u, style.contact_a.y > 0.0) {
        return paper_sample(grain_texture, brush_sampler, world, style.grain);
    }
    return 1.0;
}

fn contact_field_with_paper(world: vec2<f32>, paper: f32) -> vec2<f32> {
    var field = vec2<f32>(0.5, paper);
    if contact_feature(4u, style.contact_a.w > 0.0) {
        let p = world / style.contact_b.x;
        field.x = contact_noise(p) * 0.7 + contact_noise(p * 2.17 + vec2<f32>(17.3)) * 0.3;
    }
    return field;
}

fn contact_field(world: vec2<f32>) -> vec2<f32> {
    return contact_field_with_paper(world, contact_paper(world));
}
