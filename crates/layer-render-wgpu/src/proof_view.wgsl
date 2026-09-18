// Viewing only: lookup the final, unassociated artwork color. Coverage and UI
// overlays never enter the print transform. Samples are five packed Float32s.
@group(0) @binding(7) var<storage, read> proof_samples: array<f32>;
@group(0) @binding(8) var<uniform> proof_options: vec4<u32>;
struct ProofPoint { rgb: vec3<f32>, distances: vec2<f32> }
fn proof_at(p: vec3<u32>) -> ProofPoint {
    let edge = proof_options.x;
    let i = ((p.z * edge + p.y) * edge + p.x) * 5u;
    return ProofPoint(vec3(proof_samples[i], proof_samples[i+1u], proof_samples[i+2u]),
        vec2(proof_samples[i+3u], proof_samples[i+4u]));
}
fn proof_artwork(original: vec4<f32>) -> vec4<f32> {
    var paint=hdr_artwork(original);
    if proof_options.x>=2u && (proof_options.z!=0u || proof_options.w!=0u) {paint=hdr_map_proof(original,hdr_view.rendition,hdr_view.headroom.yz); }
    if paint.a <= 0. || proof_options.x < 2u || (proof_options.z == 0u && proof_options.w == 0u) { return paint; }
    let encoded = sdr_encode(paint.rgb / paint.a, proof_options.y & 255u);
    let outside = any(encoded < vec3(0.)) || any(encoded > vec3(1.));
    var bounded = clamp(encoded, vec3(0.), vec3(1.));
    if (proof_options.y & 256u) != 0u { bounded = sqrt(bounded); }
    let coordinate = bounded * f32(proof_options.x - 1u);
    let low = min(vec3<u32>(coordinate), vec3(proof_options.x - 2u));
    let t = coordinate - vec3<f32>(low);
    // Spell out the six tetrahedra. Dynamically indexing and updating vectors
    // in the three-step loop is substantially slower on Android Chrome/Dawn.
    // Also avoid dynamic vector l-values, which Windows FXC cannot address.
    // Keep the CPU interpolation's stable x/y/z tie order and accumulation.
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
    let p0 = proof_at(low);
    let p1 = proof_at(low + first);
    let p2 = proof_at(low + first + second);
    let p3 = proof_at(low + vec3<u32>(1u));
    var rgb = p0.rgb + a * (p1.rgb - p0.rgb) + b * (p2.rgb - p1.rgb) + c * (p3.rgb - p2.rgb);
    let distances = p0.distances + a * (p1.distances - p0.distances)
        + b * (p2.distances - p1.distances) + c * (p3.distances - p2.distances);
    if proof_options.z == 0u { rgb = paint.rgb / paint.a; }
    let score = select(distances.x / max(distances.y, 0.000001), distances.x, distances.y < 5.);
    if proof_options.w != 0u && (outside || score > 5.) { rgb = vec3(0.5); }
    return vec4(rgb * paint.a, paint.a);
}
