@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> geometry: vec4<u32>;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vertex_main(@builtin(vertex_index) i: u32) -> Vertex {
    let p = array(vec2(-1.,-1.), vec2(3.,-1.), vec2(-1.,3.))[i];
    return Vertex(vec4(p,0.,1.), vec2((p.x+1.)*.5,(1.-p.y)*.5));
}
@fragment fn fragment_main(v: Vertex) -> @location(0) vec4<f32> {
    // Use document geometry, not the padded coarse texture's normalized UVs.
    // The final edge texel may cover fewer source pixels. Weight its overlap
    // accordingly so a partial tile never stretches or shifts the image.
    let extent = vec2<f32>(geometry.xy) / f32(geometry.z);
    let footprint = fwidth(v.uv);
    let low = clamp((v.uv - footprint * .5) * extent, vec2(0.), extent);
    let high = clamp((v.uv + footprint * .5) * extent, low, extent);
    let start = vec2<u32>(floor(low));
    let end = min(vec2<u32>(ceil(high)), textureDimensions(source));
    var total = vec4<f32>(0.);
    var weight = 0.;
    for (var y = start.y; y < end.y; y++) {
        for (var x = start.x; x < end.x; x++) {
            let p = vec2<f32>(f32(x), f32(y));
            let overlap = max(vec2(0.), min(high, p+1.) - max(low, p));
            let area = overlap.x * overlap.y;
            total += textureLoad(source, vec2<i32>(i32(x), i32(y)), 0) * area;
            weight += area;
        }
    }
    let color = total / max(weight, .0000001);
    return vec4(view_straight(color), color.a);
}
