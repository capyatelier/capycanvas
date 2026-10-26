@group(0) @binding(0) var<storage, read_write> panel_profiles: array<vec2<f32>>;

fn panel_arc(u: f32, radius: f32) -> vec2<f32> {
    let u2 = u*u;
    let inner = sqrt(8.*u2*u2+2.*radius*radius*radius*radius);
    let value = sqrt(max(inner-3.*u2, 1e-12));
    return vec2(value, (-3.*u+8.*u*u2/inner)/value);
}

fn panel_base(u: f32, delta: f32) -> vec2<f32> {
    let root = 1.41421356237;
    let radius = 1./3.;
    if delta == 0. { return vec2(u, 1.); }
    var center = vec2(-radius, delta-radius);
    var touch = vec2(0., delta-radius);
    if delta < 1.+radius {
        let sum = 1.+radius;
        let difference = sum-delta;
        let width = sqrt(sqrt(max(sum*sum*sum*sum-difference*difference*difference*difference, 0.)));
        center.x = 1.-width;
        touch = vec2(1.-width/sum, delta/sum);
    }
    let start = (center.x-delta)/root;
    if u <= start { return vec2(u+root*delta, 1.); }
    if u <= (touch.x-touch.y)/root {
        let local = u-(center.x-center.y)/root;
        let arc = panel_arc(local, radius);
        return vec2((center.x+center.y)/root+arc.x, arc.y);
    }
    if delta >= 1.+radius && u < -1./root { return vec2(-u, -1.); }
    if u < 1./root {
        let arc = panel_arc(u, 1.);
        return vec2(root-arc.x, -arc.y);
    }
    return vec2(u, 1.);
}

@compute @workgroup_size(8,8)
fn panel_profile(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= 512u || id.y >= 257u { return; }
    let u = -5.+8.*f32(id.x)/511.;
    let delta = 4.*f32(id.y)/256.;
    var result = vec2(0.);
    var weight = 0.;
    for (var i=0u; i<32u; i++) {
        let offset = -1.+2.*(f32(i)+.5)/32.;
        let k = (1.-offset*offset)*(1.-offset*offset);
        result += k*panel_base(u+offset/12., delta);
        weight += k;
    }
    panel_profiles[id.y*512u+id.x] = result/weight;
}
