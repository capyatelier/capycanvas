// Shared analytic footprint for artwork, masks, and selections.
fn analytic_coverage(local: vec2<f32>, hardness: f32, min_radius: f32) -> f32 {
    let radius_squared = dot(local, local);
    if radius_squared >= 1.0 {
        return 0.0;
    }
    let edge = max(
        1.0 - clamp(hardness, 0.0, 1.0),
        1.0 / max(min_radius, 0.005),
    );
    let solid_radius = max(1.0 - edge, 0.0);
    if radius_squared <= solid_radius * solid_radius {
        return 1.0;
    }
    return clamp((1.0 - sqrt(radius_squared)) / edge, 0.0, 1.0);
}
