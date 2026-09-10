// Brush coverage, independent of texture bindings (material brushes use all 16).
struct BrushSelection { rect: vec4<u32>, info: vec4<u32>, values: array<u32> }
@group(1) @binding(1) var<storage, read> brush_selection: BrushSelection;
fn brush_selection_at(world: vec2<f32>) -> f32 {
    if brush_selection.info.y == 0u { return 1.; }
    let p = vec2<i32>(floor(world)) - vec2<i32>(brush_selection.rect.xy);
    var coverage = 0.;
    if all(p >= vec2<i32>(0)) && all(p < vec2<i32>(brush_selection.rect.zw)) {
        let word = u32(p.y) * ((brush_selection.rect.z + 7u) / 8u) + u32(p.x) / 8u;
        coverage = f32((brush_selection.values[word] >> ((u32(p.x) % 8u) * 4u)) & 15u) * .25;
    }
    return select(coverage, 1.-coverage, brush_selection.info.x != 0u);
}
