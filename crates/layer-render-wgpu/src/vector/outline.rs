use super::tiles::{Frame, TOLERANCE, flatten};
use layer_core::authored::vector::{Cap, Join, PathItem, Stroke};

struct Point { p: [f64; 2], left: f64, right: f64 }

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] { [a[0] - b[0], a[1] - b[1]] }
fn add(a: [f64; 2], b: [f64; 2], s: f64) -> [f64; 2] { [a[0] + b[0] * s, a[1] + b[1] * s] }
fn unit(v: [f64; 2]) -> Option<[f64; 2]> { let l = (v[0] * v[0] + v[1] * v[1]).sqrt(); (l > 1e-12).then(|| [v[0] / l, v[1] / l]) }
fn left_of(d: [f64; 2]) -> [f64; 2] { [d[1], -d[0]] }
fn cross(a: [f64; 2], b: [f64; 2]) -> f64 { a[0] * b[1] - a[1] * b[0] }
fn dot(a: [f64; 2], b: [f64; 2]) -> f64 { a[0] * b[0] + a[1] * b[1] }

fn arc(out: &mut Vec<[f64; 2]>, center: [f64; 2], from: [f64; 2], to: [f64; 2], radius: f64, clockwise_in_y_down: bool) {
    let a0 = from[1].atan2(from[0]);
    let mut sweep = to[1].atan2(to[0]) - a0;
    if clockwise_in_y_down { while sweep < 0. { sweep += std::f64::consts::TAU; } } else { while sweep > 0. { sweep -= std::f64::consts::TAU; } }
    let step = if radius > TOLERANCE { 2. * (1. - TOLERANCE / radius).acos() } else { std::f64::consts::PI };
    let count = ((sweep.abs() / step.max(1e-3)).ceil() as usize).clamp(1, 256);
    for i in 1..count { let a = a0 + sweep * i as f64 / count as f64; out.push([center[0] + radius * a.cos(), center[1] + radius * a.sin()]); }
}

fn side(points: &[Point], closed: bool, stroke: &Stroke, scale: f64, left: bool) -> Vec<[f64; 2]> {
    let n = points.len();
    let segments = if closed { n } else { n - 1 };
    let direction = |i: usize| unit(sub(points[(i + 1) % n].p, points[i].p)).unwrap_or([1., 0.]);
    let width = |i: usize| if left { points[i % n].left } else { points[i % n].right };
    let sign = if left { 1. } else { -1. };
    let mut out = Vec::with_capacity(segments * 2 + 8);
    for i in 0..segments {
        let d = direction(i);
        let normal = left_of(d);
        let (a, b) = (i, (i + 1) % n);
        let start = add(points[a].p, normal, sign * width(a));
        let end = add(points[b].p, normal, sign * width(b));
        if i == 0 && !closed { out.push(start); }
        if i > 0 || closed {
            let previous = direction((i + segments - 1) % segments.max(1));
            let pn = left_of(previous);
            let w = width(a);
            let previous_end = add(points[a].p, pn, sign * w);
            let turn = cross(previous, d) * sign;
            if dot(previous, d) > 0.9995 {
                let m = unit([pn[0] + normal[0], pn[1] + normal[1]]).unwrap_or(normal);
                out.push(add(points[a].p, m, sign * w / dot(m, normal).max(0.5)));
            } else if turn > 0. {
                out.push(previous_end); out.push(points[a].p); out.push(start);
            } else {
                out.push(previous_end);
                match stroke.join {
                    Join::Round => arc(&mut out, points[a].p, sub(previous_end, points[a].p), sub(start, points[a].p), w, !left),
                    Join::Miter => {
                        let m = unit([pn[0] + normal[0], pn[1] + normal[1]]).unwrap_or(normal);
                        let ratio = 1. / dot(m, normal).max(1e-9);
                        if ratio <= stroke.miter_limit { out.push(add(points[a].p, m, sign * w * ratio)); }
                    }
                    Join::Bevel => {}
                }
                out.push(start);
            }
        }
        out.push(end);
    }
    let _ = scale;
    out
}

fn cap(out: &mut Vec<[f64; 2]>, at: &Point, outward: [f64; 2], cap: Cap) {
    let normal = left_of(outward);
    let from = add(at.p, normal, -at.right);
    let to = add(at.p, normal, at.left);
    match cap {
        Cap::Butt => {}
        Cap::Square => { out.push(add(from, outward, at.right)); out.push(add(to, outward, at.left)); }
        Cap::Round => {
            let radius = (at.left + at.right) * 0.5;
            let center = add(at.p, normal, (at.left - at.right) * 0.5);
            arc(out, center, sub(from, center), sub(to, center), radius, false);
        }
    }
}

pub(crate) fn outline(path: &PathItem, stroke: &Stroke, frame: &Frame) -> Vec<Vec<[f64; 2]>> {
    let half = stroke.width * 0.5 * frame.scale;
    let width = |segment: u32, t: f64| stroke.profile.as_ref().map_or([half; 2], |p| p.at(segment, t).map(|f| f * half));
    let map = |p: [f64; 2]| [(p[0] + frame.origin[0]) * frame.scale, (p[1] + frame.origin[1]) * frame.scale];
    let mut polygons = Vec::new();
    let mut base = 0u32;
    for subpath in path.geometry.subpaths() {
        let [l, r] = width(base, 0.);
        let mut points = vec![Point { p: map(subpath.anchor(0).point), left: l, right: r }];
        for (index, segment) in subpath.segments().enumerate() {
            let global = base + index as u32;
            flatten(&segment, map, TOLERANCE, |p, t| { let [left, right] = width(global, t); points.push(Point { p, left, right }); });
        }
        base += subpath.segment_count() as u32;
        points.dedup_by(|a, b| sub(a.p, b.p).iter().all(|v| v.abs() < 1e-9));
        if subpath.closed && points.len() > 2 && sub(points[0].p, points[points.len() - 1].p).iter().all(|v| v.abs() < 1e-9) { points.pop(); }
        if points.len() == 1 {
            if stroke.cap == Cap::Butt { continue; }
            let p = &points[0];
            let mut ring = Vec::new();
            let radius = (p.left + p.right) * 0.5;
            if stroke.cap == Cap::Round {
                arc(&mut ring, p.p, [radius, 0.], [radius - 1e-9, -1e-6], radius, true);
                ring.insert(0, add(p.p, [1., 0.], radius));
            } else {
                for o in [[-1., -1.], [1., -1.], [1., 1.], [-1., 1.]] { ring.push([p.p[0] + o[0] * radius, p.p[1] + o[1] * radius]); }
            }
            polygons.push(ring);
            continue;
        }
        let closed = subpath.closed && points.len() > 2;
        let left = side(&points, closed, stroke, frame.scale, true);
        let mut right = side(&points, closed, stroke, frame.scale, false);
        right.reverse();
        if closed { polygons.push(left); polygons.push(right); continue; }
        let mut ring = left;
        let last = points.len() - 1;
        cap(&mut ring, &points[last], unit(sub(points[last].p, points[last - 1].p)).unwrap_or([1., 0.]), stroke.cap);
        ring.extend(right);
        cap(&mut ring, &points[0], unit(sub(points[0].p, points[1].p)).unwrap_or([-1., 0.]), stroke.cap);
        polygons.push(ring);
    }
    polygons
}
