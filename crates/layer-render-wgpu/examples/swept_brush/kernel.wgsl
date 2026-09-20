// Research harness: the legacy entry uses the production evolving_contact
// function verbatim. Contact and simple use identical page lists; merging
// changes those lists. All variants share formats, page binning and one store
// per dispatched pixel.
struct Style {
    contact_a: vec4<f32>,
    contact_b: vec4<f32>,
    contact_c: vec4<f32>,
    grain: vec4<f32>,
}
struct Dab {
    center: vec2<f32>, radii: vec2<f32>, rotation: vec2<f32>, motion: vec2<f32>,
    color: vec4<f32>, flow: f32, hardness: f32, texture_sign: vec2<f32>,
    material: vec4<f32>, previous: vec4<f32>, contact: vec4<f32>, previous_contact: vec4<f32>,
}
struct Page { origin: vec2<u32>, first: u32, count: u32 }
@group(0) @binding(0) var<uniform> style: Style;
@group(0) @binding(1) var<storage, read> dabs: array<Dab>;
@group(0) @binding(2) var<storage, read> pages: array<Page>;
@group(0) @binding(3) var grain_texture: texture_2d<f32>;
@group(0) @binding(4) var brush_sampler: sampler;
@group(0) @binding(5) var background: texture_2d<f32>;
@group(0) @binding(6) var output: texture_storage_2d<rgba32float, write>;
override SIMPLE: bool = false;
override PENCIL: bool = false;

fn rotate(p: vec2<f32>, c: f32, s: f32) -> vec2<f32> {
    return vec2<f32>(p.x * c - p.y * s, p.x * s + p.y * c);
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let page = pages[id.z];
    let xy = page.origin + id.xy;
    let world = vec2<f32>(xy) + vec2<f32>(0.5);
    let base = textureLoad(background, vec2<i32>(xy), 0);
    var alpha = 0.0;
    if SIMPLE {
        // Prototype: round swept nib with a nearest-segment material field.
        // Geometry merging is measured separately from shader simplification.
        var best = 1e20;
        var pressure = 1.0;
        var radius = 1.0;
        for (var i = 0u; i < page.count; i += 1u) {
            let d = dabs[page.first + i];
            let start = d.center - d.motion;
            let t = clamp(dot(world - start, d.motion) / max(dot(d.motion, d.motion), 0.000001), 0.0, 1.0);
            let r = mix(d.previous.x, d.radii.x, t);
            let distance = length(world - start - d.motion * t) - r;
            if distance < best {
                best = distance;
                radius = r;
                pressure = mix(d.previous_contact.x, d.contact.x, t);
            }
        }
        let feather = select(1.0, max(1.0, radius * 0.25), PENCIL);
        alpha = 1.0 - smoothstep(-feather, 0.5, best);
        if PENCIL && alpha > 0.0 {
            // Stationary paper, evaluated once after segment selection.
            let uv = world / vec2<f32>(textureDimensions(grain_texture));
            let tooth = textureSampleLevel(grain_texture, brush_sampler, uv, 0.0).r;
            let threshold = 0.7 - pressure * 0.9 * 0.58;
            alpha *= smoothstep(threshold - 0.12, threshold + 0.12, tooth) * (0.48 + tooth * 0.52);
            alpha = 1.0 - exp(-alpha * 1.5);
        }
    } else {
        // Same per-contact geometry, paper and accumulation arithmetic as the
        // production normal dry path; no selection/alpha lock in this fixture.
        for (var i = 0u; i < page.count; i += 1u) {
            let d = dabs[page.first + i];
            let coverage = evolving_contact(world, d.center, d.radii, d.rotation,
                d.motion, d.previous, d.contact, d.previous_contact, d.hardness);
            if coverage <= 0.0 { continue; }
            if PENCIL {
                let exposure = contact_exposure(d.motion, d.radii, d.rotation, d.hardness);
                let a = 1.0 - exp(-coverage * d.flow * d.color.a * exposure * 6.0);
                alpha = alpha + (1.0 - alpha) * a;
            } else {
                alpha = max(alpha, clamp(coverage * d.flow * d.color.a, 0.0, 1.0));
            }
        }
    }
    let ink = vec3<f32>(0.02, 0.02, 0.02);
    textureStore(output, vec2<i32>(xy), vec4<f32>(mix(base.rgb, ink, alpha), 1.0));
}
