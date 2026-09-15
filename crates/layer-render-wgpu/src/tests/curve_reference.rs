// Independent Float64 Hermite basis reference, retaining the documented secant
// endpoint and weighted harmonic interior tangents. This does not read packed
// GPU polynomial coefficients or sample a 256-entry table.
pub(super) fn evaluate(points: &[[f32; 2]], x: f64) -> f64 {
    let points: Vec<_> = points.iter().map(|p| p.map(f64::from)).collect();
    let i = points
        .partition_point(|p| p[0] < x)
        .saturating_sub(1)
        .min(points.len() - 2);
    let slope = |i: usize| (points[i + 1][1] - points[i][1]) / (points[i + 1][0] - points[i][0]);
    let tangent = |i: usize| {
        if i == 0 {
            return slope(0);
        }
        if i == points.len() - 1 {
            return slope(i - 1);
        }
        let a = slope(i - 1);
        let b = slope(i);
        if a * b <= 0. {
            return 0.;
        }
        let h0 = points[i][0] - points[i - 1][0];
        let h1 = points[i + 1][0] - points[i][0];
        let w0 = 2. * h1 + h0;
        let w1 = h1 + 2. * h0;
        (w0 + w1) / (w0 / a + w1 / b)
    };
    let h = points[i + 1][0] - points[i][0];
    let t = (x - points[i][0]) / h;
    if t < 0. {
        return points[i][1] + t * h * tangent(i);
    }
    if t > 1. {
        return points[i + 1][1] + (t - 1.) * h * tangent(i + 1);
    }
    (2. * t * t * t - 3. * t * t + 1.) * points[i][1]
        + (t * t * t - 2. * t * t + t) * h * tangent(i)
        + (-2. * t * t * t + 3. * t * t) * points[i + 1][1]
        + (t * t * t - t * t) * h * tangent(i + 1)
}
