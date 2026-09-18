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
    if proof_options.x>=2u && (proof_options.z!=0u || proof_options.w!=0u) {paint=hdr_map_proof(original,hdr_view); }
    if paint.a <= 0. || proof_options.x < 2u || (proof_options.z == 0u && proof_options.w == 0u) { return paint; }
    let encoded = sdr_encode(paint.rgb / paint.a, proof_options.y & 255u);
    let outside = any(encoded < vec3(0.)) || any(encoded > vec3(1.));
    var bounded = clamp(encoded, vec3(0.), vec3(1.));
    if (proof_options.y & 256u) != 0u { bounded = sqrt(bounded); }
    let coordinate = bounded * f32(proof_options.x - 1u);
    var low = min(vec3<u32>(coordinate), vec3(proof_options.x - 2u));
    let t = coordinate - vec3<f32>(low);
    var axes = vec3<u32>(0u, 1u, 2u);
    if t[axes.x] < t[axes.y] { axes = axes.yxz; }
    if t[axes.y] < t[axes.z] { axes = axes.xzy; }
    if t[axes.x] < t[axes.y] { axes = axes.yxz; }
    var previous = proof_at(low);
    var rgb = previous.rgb;
    var distances = previous.distances;
    for (var i = 0u; i < 3u; i += 1u) {
        let axis = axes[i];
        low[axis] += 1u;
        let next = proof_at(low);
        rgb += t[axis] * (next.rgb - previous.rgb);
        distances += t[axis] * (next.distances - previous.distances);
        previous = next;
    }
    if proof_options.z == 0u { rgb = paint.rgb / paint.a; }
    let score = select(distances.x / max(distances.y, 0.000001), distances.x, distances.y < 5.);
    if proof_options.w != 0u && (outside || score > 5.) { rgb = vec3(0.5); }
    return vec4(rgb * paint.a, paint.a);
}
