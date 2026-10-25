fn blender_cell_coverage(dab: Dab, cell_origin: vec2<f32>, size: f32) -> f32 {
    let origin = max(cell_origin, vec2<f32>(0.0));
    let end = min(cell_origin + size, render_target.document_extent.xy);
    if any(end <= origin) { return 0.0; }
    let low = origin + 0.5;
    let high = end - 0.5;
    if all(low == high) { return contact_coverage(dab, low); }
    if brush_selection.info.y == 0u && style.flags.x > 0.5 && !bristles_enabled()
        && !contact_feature(1u, style.contact_a.x > 0.5) && !contact_feature(64u, style.contact_a.z > 0.5) {
        return min(min(contact_coverage(dab, low), contact_coverage(dab, high)),
            min(contact_coverage(dab, vec2<f32>(low.x, high.y)), contact_coverage(dab, vec2<f32>(high.x, low.y))));
    }
    var coverage = 1.0;
    var selected = false;
    for (var y = low.y; y <= high.y; y += 1.0) {
        for (var x = low.x; x <= high.x; x += 1.0) {
            let point = vec2<f32>(x, y);
            if brush_selection_at(point) <= 0.0 { continue; }
            selected = true;
            coverage = min(coverage, contact_coverage(dab, point));
            if coverage <= 0.0 { return 0.0; }
        }
    }
    return select(0.0, coverage, selected);
}
