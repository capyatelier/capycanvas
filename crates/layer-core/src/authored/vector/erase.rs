use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Eraser { pub from: [f64; 2], pub to: [f64; 2], pub radius: f64 }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EraseMode { #[default] Touched, Whole, ToIntersection }

impl Eraser {
    pub fn bounds(&self) -> [f64; 4] {
        let r = self.radius;
        [self.from[0].min(self.to[0]) - r, self.from[1].min(self.to[1]) - r, self.from[0].max(self.to[0]) + r, self.from[1].max(self.to[1]) + r]
    }
    fn distance_squared(&self, p: [f64; 2]) -> f64 {
        let d = [self.to[0] - self.from[0], self.to[1] - self.from[1]];
        let length = d[0] * d[0] + d[1] * d[1];
        let t = if length > 0. { (((p[0] - self.from[0]) * d[0] + (p[1] - self.from[1]) * d[1]) / length).clamp(0., 1.) } else { 0. };
        (self.from[0] + d[0] * t - p[0]).powi(2) + (self.from[1] + d[1] * t - p[1]).powi(2)
    }
    fn distance(&self, p: [f64; 2]) -> f64 { self.distance_squared(p).sqrt() }
    fn covers(&self, p: [f64; 2], reach: f64) -> bool {
        let radius = self.radius + reach;
        radius >= 0. && self.distance_squared(p) <= radius * radius
    }
    fn meets(&self, a: [f64; 2], b: [f64; 2], reach: f64) -> bool {
        let cross = |o: [f64; 2], p: [f64; 2], q: [f64; 2]| (p[0] - o[0]) * (q[1] - o[1]) - (p[1] - o[1]) * (q[0] - o[0]);
        let crossing = cross(a, b, self.from) * cross(a, b, self.to) < 0. && cross(self.from, self.to, a) * cross(self.from, self.to, b) < 0.;
        let chord = Eraser { from: a, to: b, radius: 0. };
        crossing || [a, b].iter().any(|p| self.covers(*p, reach)) || [self.from, self.to].iter().any(|p| chord.covers(*p, self.radius + reach))
    }
    fn cap_contact(&self, center: [f64; 2], normal: [f64; 2], radius: f64, cap: Cap) -> bool {
        if cap == Cap::Round { return self.covers(center, radius); }
        let offset = |side: f64, along: f64| [center[0] + normal[0] * side - normal[1] * along, center[1] + normal[1] * side + normal[0] * along];
        if cap == Cap::Butt { return self.meets(offset(radius, 0.), offset(-radius, 0.), 0.); }
        let ring = [offset(radius, radius), offset(-radius, radius), offset(-radius, -radius), offset(radius, -radius)];
        [self.from, self.to].iter().any(|q| {
            let d = [q[0] - center[0], q[1] - center[1]];
            (d[0] * normal[0] + d[1] * normal[1]).abs() <= radius && (d[1] * normal[0] - d[0] * normal[1]).abs() <= radius
        }) || (0..4).any(|i| self.meets(ring[i], ring[(i + 1) % 4], 0.))
    }
    fn cuts(&self, segment: Segment, widths: [[f64; 2]; 2], cap: Cap, from: f64, to: f64, out: &mut Vec<(f64, f64)>) {
        fn append(out: &mut Vec<(f64, f64)>, from: f64, to: f64) {
            match out.last_mut() { Some(last) if from <= last.1 + 1e-12 => last.1 = to, _ => out.push((from, to)) }
        }
        fn visit(eraser: &Eraser, (segment, widths): (Segment, [[f64; 2]; 2]), cap: Cap, flat: bool, [from, to]: [f64; 2], depth: u32, out: &mut Vec<(f64, f64)>) {
            let points = [segment.from, segment.control[0], segment.control[1], segment.to];
            let radii = widths.map(|[l, r]| (l + r) * 0.5);
            let shifts = widths.map(|[l, r]| (l - r) * 0.5);
            let symmetric = shifts == [0.; 2];
            let reach = if cap == Cap::Square { std::f64::consts::SQRT_2 } else { 1. };
            let (q, error, normal) = if symmetric && cap == Cap::Round { (points, 0., [0.; 2]) } else {
                let d = [segment.to[0] - segment.from[0], segment.to[1] - segment.from[1]];
                let length = d[0].hypot(d[1]);
                let normal = if length > 1e-12 { [d[1] / length, -d[0] / length] } else { [0., -1.] };
                let mut error: f64 = 0.;
                for pair in points.windows(2).filter(|_| !flat) {
                    let tangent = [pair[1][0] - pair[0][0], pair[1][1] - pair[0][1]];
                    let l = tangent[0].hypot(tangent[1]);
                    if l <= 1e-12 { continue; }
                    let cosine = if length > 1e-12 { (tangent[0] * d[0] + tangent[1] * d[1]) / (l * length) } else { -1. };
                    error = error.max(if cosine > 0. { (2. * (1. - cosine.min(1.))).sqrt() } else { 2. });
                }
                error *= shifts[0].abs().max(shifts[1].abs()) + if cap == Cap::Round { 0. } else { radii[0].max(radii[1]) * reach };
                let q = std::array::from_fn(|i| {
                    let shift = shifts[0] + (shifts[1] - shifts[0]) * i as f64 / 3.;
                    [points[i][0] + normal[0] * shift, points[i][1] + normal[1] * shift]
                });
                (q, error, normal)
            };
            let inside = if cap == Cap::Round {
                q.iter().enumerate().all(|(i, p)| eraser.covers(*p, radii[0] + (radii[1] - radii[0]) * i as f64 / 3. - error))
            } else if eraser.radius >= error {
                let inner = Eraser { radius: eraser.radius - error, ..*eraser };
                q.iter().enumerate().all(|(i, p)| inner.cap_contact(*p, normal, radii[0] + (radii[1] - radii[0]) * i as f64 / 3., cap))
            } else { false };
            if inside { append(out, from, to); return; }
            if cap != Cap::Round {
                for (axis, reach) in [([-normal[1], normal[0]], if cap == Cap::Square { radii[0].max(radii[1]) } else { 0. }), (normal, radii[0].max(radii[1]))] {
                    let project = |p: [f64; 2]| p[0] * axis[0] + p[1] * axis[1];
                    let (low, high) = q.iter().map(|p| project(*p)).fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), p| (low.min(p), high.max(p)));
                    let [a, b] = [eraser.from, eraser.to].map(project);
                    let reach = reach + error + eraser.radius;
                    if a.max(b) < low - reach || a.min(b) > high + reach { return; }
                }
            }
            let chord = Eraser { from: q[0], to: q[3], radius: 0. };
            let bend = if flat && symmetric { 0. } else { q[1..3].iter().map(|p| chord.distance(*p)).fold(0., f64::max) };
            if !eraser.meets(q[0], q[3], radii[0].max(radii[1]) * reach + bend + error) { return; }
            let tiny = points.iter().all(|p| (p[0] - points[0][0]).abs().max((p[1] - points[0][1]).abs()) <= 1. / (GRID * 4.));
            if tiny || depth == 40 {
                let p = segment.point(0.5);
                let (center, mid_normal) = if symmetric && cap == Cap::Round { (p, normal) } else {
                    let tangent = [points[3][0] + points[2][0] - points[1][0] - points[0][0], points[3][1] + points[2][1] - points[1][1] - points[0][1]];
                    let tangent_length = tangent[0].hypot(tangent[1]);
                    let mid_normal = if tangent_length > 1e-12 { [tangent[1] / tangent_length, -tangent[0] / tangent_length] } else { normal };
                    let shift = (shifts[0] + shifts[1]) * 0.5;
                    ([p[0] + mid_normal[0] * shift, p[1] + mid_normal[1] * shift], mid_normal)
                };
                if eraser.cap_contact(center, mid_normal, (radii[0] + radii[1]) * 0.5, cap) { append(out, from, to); }
            } else {
                let (left, right) = segment.split(0.5);
                let middle = (from + to) * 0.5;
                let width = std::array::from_fn(|i| (widths[0][i] + widths[1][i]) * 0.5);
                visit(eraser, (left, [widths[0], width]), cap, flat, [from, middle], depth + 1, out);
                visit(eraser, (right, [width, widths[1]]), cap, flat, [middle, to], depth + 1, out);
            }
        }
        let d = [segment.to[0] - segment.from[0], segment.to[1] - segment.from[1]];
        let length = d[0] * d[0] + d[1] * d[1];
        let flat = length > 0. && segment.control.iter().all(|p| {
            let q = [p[0] - segment.from[0], p[1] - segment.from[1]];
            let projection = q[0] * d[0] + q[1] * d[1];
            projection >= 0. && projection <= length && (q[0] * d[1] - q[1] * d[0]).abs() <= 1e-8 * length.sqrt()
        });
        visit(self, (segment, widths), cap, flat, [from, to], 0, out);
    }

}

#[derive(Clone, Copy)]
struct Origin { segment: u32, from: f64, to: f64 }
type Pieces = Vec<Vec<(usize, f64, f64)>>;

fn portion(segment: Segment, from: f64, to: f64) -> Segment {
    let right = if from > 0. { segment.split(from).1 } else { segment };
    if to < 1. { right.split((to - from) / (1. - from)).0 } else { right }
}

fn apart(segment: &Segment, area: [f64; 4], reach: f64) -> bool {
    let [x0, y0, x1, y1] = [segment.from, segment.control[0], segment.control[1], segment.to].iter()
        .fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])]);
    area[2] < x0 - reach || area[0] > x1 + reach || area[3] < y0 - reach || area[1] > y1 + reach
}

fn removed(path: &PathItem, subpath: &Subpath, base: u32, eraser: &Eraser) -> Vec<(f64, f64)> {
    let maximum = path.style.strokes().map(|s| s.width * 0.5 * s.profile.as_ref().map_or(1., |p| p.max_factor()) * if s.cap == Cap::Square { std::f64::consts::SQRT_2 } else { 1. }).fold(0., f64::max);
    let area = eraser.bounds();
    let mut cuts = Vec::new();
    for (index, segment) in subpath.segments().enumerate().filter(|(_, segment)| !apart(segment, area, maximum)) {
        let global = base + index as u32;
        for stroke in path.style.strokes() {
            let width = |t| stroke.profile.as_ref().map_or([stroke.width * 0.5; 2], |p| p.at(global, t).map(|v| v * stroke.width * 0.5));
            let mut from = 0.;
            let knots = stroke.profile.as_ref().map_or(&[][..], |p| p.segment_knots(global));
            let mut intervals = Vec::new();
            for to in knots.iter().map(WidthKnot::t).filter(|t| *t > 0. && *t < 1.).chain([1.]) {
                eraser.cuts(portion(segment, from, to), [width(from), width(to)], stroke.cap, from, to, &mut intervals);
                from = to;
            }
            cuts.extend(intervals.into_iter().map(|(a, b)| (index as f64 + a, index as f64 + b)));
        }
    }
    cuts

}

fn reach_stops(removed: Vec<(f64, f64)>, stops: &[f64], length: f64, closed: bool) -> Vec<(f64, f64)> {
    if closed && stops.is_empty() && !removed.is_empty() { return vec![(0., length)]; }
    let mut out = Vec::new();
    for (a, b) in removed {
        let before = stops.iter().copied().filter(|s| *s <= a).fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.max(s))));
        let after = stops.iter().copied().filter(|s| *s >= b).fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.min(s))));
        let from = before.unwrap_or_else(|| if closed { stops.iter().copied().fold(f64::MIN, f64::max) - length } else { 0. });
        let to = after.unwrap_or_else(|| if closed { stops.iter().copied().fold(f64::MAX, f64::min) + length } else { length });
        if from < 0. { out.extend([(from + length, length), (0., to.min(length))]); }
        else if to > length { out.extend([(from, length), (0., to - length)]); }
        else { out.push((from, to)); }
    }
    out
}

fn cut(path: &PathItem, subpath: &Subpath, base: u32, eraser: &Eraser) -> Option<Vec<(f64, f64)>> {
    if subpath.segment_count() == 0 {
        let p = subpath.anchor(0).point();
        return path.style.strokes().any(|stroke| {
            let [left, right] = stroke.profile.as_ref().map_or([1.; 2], |profile| profile.at(base, 0.));
            let r = stroke.width * 0.25 * (left + right);
            stroke.cap != Cap::Butt && eraser.cap_contact(p, [0., -1.], r, stroke.cap)
        }).then(Vec::new);
    }
    Some(removed(path, subpath, base, eraser)).filter(|removed| !removed.is_empty())
}

fn kept(subpath: &Subpath, mut removed: Vec<(f64, f64)>, stops: Option<&[f64]>) -> Pieces {
    if subpath.segment_count() == 0 { return Vec::new(); }
    let length = subpath.segment_count() as f64;
    if let Some(stops) = stops { removed = reach_stops(removed, stops, length, subpath.closed); }
    removed.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut spans: Vec<(f64, f64)> = Vec::new();
    let mut at = 0.;
    for (a, b) in removed {
        if a > at + 1e-9 { spans.push((at, a)); }
        at = f64::max(at, b);
    }
    if at < length - 1e-9 { spans.push((at, length)); }
    let entries = |(u, v): (f64, f64)| -> Vec<(usize, f64, f64)> {
        let first = u.floor() as usize;
        let last = ((v.ceil() as usize).max(first + 1)).min(subpath.segment_count());
        (first..last).map(|i| (i, (u - i as f64).clamp(0., 1.), (v - i as f64).clamp(0., 1.))).filter(|(_, from, to)| to > from).collect()
    };
    let mut pieces: Pieces = spans.into_iter().map(entries).filter(|piece| !piece.is_empty()).collect();
    let wraps = pieces.first().is_some_and(|piece| piece[0].0 == 0 && piece[0].1 == 0.)
        && pieces.last().is_some_and(|piece| piece.last().is_some_and(|last| last.0 + 1 == subpath.segment_count() && last.2 == 1.));
    if subpath.closed && wraps && pieces.len() > 1 {
        let first = pieces.remove(0);
        pieces.last_mut().unwrap().extend(first);
    }
    pieces
}

fn polyline(subpath: &Subpath, tolerance: f64) -> Vec<([f64; 2], f64)> {
    let mut points = vec![(subpath.anchor(0).point(), 0.)];
    for (index, segment) in subpath.segments().enumerate() { segment.flatten(tolerance, |p, t| points.push((p, index as f64 + t))); }
    points
}

fn trim_cap(path: &PathItem, subpath: &Subpath, base: u32, piece: &mut Vec<(usize, f64, f64)>, end: bool) -> bool {
    let Some(&(index, from, to)) = (if end { piece.last() } else { piece.first() }) else { return false };
    let radius = |index: usize, t: f64| path.style.strokes().filter(|stroke| stroke.cap != Cap::Butt).map(|stroke| {
        let [left, right] = stroke.profile.as_ref().map_or([1.; 2], |p| p.at(base + index as u32, t));
        stroke.width * 0.25 * (left + right)
    }).fold(0., f64::max);
    if radius(index, if end { to } else { from }) == 0. { return true; }
    let mut points = Vec::new();
    for (k, &(index, from, to)) in piece.iter().enumerate() {
        let segment = subpath.segment(index);
        points.push((segment.point(from), k, from));
        portion(segment, from, to).flatten(1. / GRID, |p, t| points.push((p, k, from + (to - from) * t)));
    }
    if end { points.reverse(); }
    let origin = points[0].0;
    let Some(next) = points.iter().map(|p| p.0).find(|p| (p[0] - origin[0]).hypot(p[1] - origin[1]) > 1e-9) else { return false };
    let length = (next[0] - origin[0]).hypot(next[1] - origin[1]);
    let direction = [(next[0] - origin[0]) / length, (next[1] - origin[1]) / length];
    let distance = |k: usize, t: f64| {
        let index = piece[k].0;
        let p = subpath.segment(index).point(t);
        (p[0] - origin[0]) * direction[0] + (p[1] - origin[1]) * direction[1] - radius(index, t)
    };
    for pair in points.windows(2) {
        let (_, k, to) = pair[1];
        if distance(k, to) < 0. { continue; }
        let mut from = if pair[0].1 == k { pair[0].2 } else if end { piece[k].2 } else { piece[k].1 };
        let mut to = to;
        for _ in 0..40 { let middle = (from + to) * 0.5; if distance(k, middle) < 0. { from = middle } else { to = middle } }
        if end { piece.truncate(k + 1); piece[k].2 = to; }
        else { piece.drain(..k); piece[0].1 = to; }
        return true;
    }
    false
}

fn refined_crossing(a: &Edge, b: &Edge, mut t: f64, mut u: f64) -> Option<f64> {
    t = a.from + (a.to - a.from) * t;
    u = b.from + (b.to - b.from) * u;
    let derivative = |s: &Segment, t: f64| std::array::from_fn::<_, 2, _>(|i| {
        3. * ((s.control[0][i] - s.from[i]) * (1. - t).powi(2)
            + 2. * (s.control[1][i] - s.control[0][i]) * (1. - t) * t
            + (s.to[i] - s.control[1][i]) * t * t)
    });
    let cross = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
    for _ in 0..24 {
        let (p, q) = (a.segment.point(t), b.segment.point(u));
        let error = [p[0] - q[0], p[1] - q[1]];
        if error[0].hypot(error[1]) <= 1. / (GRID * 32.) { return Some(a.base + t); }
        let (da, db) = (derivative(&a.segment, t), derivative(&b.segment, u));
        let denominator = cross(da, db);
        if denominator.abs() < 1e-12 { return None; }
        t = (t - cross(error, db) / denominator).clamp(0., 1.);
        u = (u - cross(error, da) / denominator).clamp(0., 1.);
    }
    None
}

fn crossing(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> Option<f64> {
    let r = [b[0] - a[0], b[1] - a[1]];
    let s = [d[0] - c[0], d[1] - c[1]];
    let denominator = r[0] * s[1] - r[1] * s[0];
    if denominator.abs() < 1e-12 { return None; }
    let q = [c[0] - a[0], c[1] - a[1]];
    let t = (q[0] * s[1] - q[1] * s[0]) / denominator;
    let u = (q[0] * r[1] - q[1] * r[0]) / denominator;
    ((0. ..=1.).contains(&t) && (0. ..=1.).contains(&u)).then_some(t)
}

#[derive(Clone, Copy)]
pub(super) struct Edge { pub a: [f64; 2], pub b: [f64; 2], pub segment: Segment, pub from: f64, pub to: f64, pub base: f64, pub subpath: usize, pub index: usize }
pub(super) fn path_edges(subpath: &Subpath, id: usize, tolerance: f64) -> Vec<Edge> {
    if subpath.segments().all(|segment|segment.is_line()) {
        return subpath.segments().enumerate().map(|(index,segment)|Edge {
            a:segment.from,b:segment.to,segment,from:0.,to:1.,base:index as f64,subpath:id,index,
        }).collect();
    }
    polyline(subpath, tolerance).windows(2).enumerate().map(|(index, pair)| {
        let base = (pair[0].1.floor() as usize).min(subpath.segment_count().saturating_sub(1));
        Edge { a: pair[0].0, b: pair[1].0, segment: subpath.segment(base), from: pair[0].1 - base as f64, to: pair[1].1 - base as f64, base: base as f64, subpath: id, index }
    }).collect()
}
pub(super) struct Edges { bounds: [f64; 4], edges: Arc<[Edge]>, indices: Vec<usize>, children: Option<Box<[Edges; 2]>> }
impl Edges {
    pub(super) fn new(edges: Vec<Edge>) -> Self {
        let indices=(0..edges.len()).collect();Self::partition(edges.into(),indices)
    }
    fn partition(edges:Arc<[Edge]>,mut indices:Vec<usize>)->Self {
        let bounds=indices.iter().flat_map(|i|[edges[*i].a,edges[*i].b]).fold([f64::INFINITY,f64::INFINITY,f64::NEG_INFINITY,f64::NEG_INFINITY],|b,p|
            [b[0].min(p[0]),b[1].min(p[1]),b[2].max(p[0]),b[3].max(p[1])]);
        let children=if indices.len()>8 {
            let axis=usize::from(bounds[3]-bounds[1]>bounds[2]-bounds[0]);let middle=indices.len()/2;
            indices.select_nth_unstable_by(middle,|a,b|(edges[*a].a[axis]+edges[*a].b[axis]).total_cmp(&(edges[*b].a[axis]+edges[*b].b[axis])));
            let right=indices.split_off(middle);
            Some(Box::new([Self::partition(edges.clone(),std::mem::take(&mut indices)),Self::partition(edges.clone(),right)]))
        }else{None};
        Self {bounds,edges,indices,children}
    }
    pub(super) fn near(&self,a:[f64;2],b:[f64;2],emit:&mut impl FnMut(&Edge)) {
        if a[0].min(b[0])>self.bounds[2] || a[0].max(b[0])<self.bounds[0] || a[1].min(b[1])>self.bounds[3] || a[1].max(b[1])<self.bounds[1] {return;}
        if let Some(children)=&self.children {for child in children.iter() {child.near(a,b,emit);}}
        else {for &i in &self.indices {let edge=&self.edges[i];
            if edge.a[0].min(edge.b[0])<=a[0].max(b[0]) && edge.a[0].max(edge.b[0])>=a[0].min(b[0])
                && edge.a[1].min(edge.b[1])<=a[1].max(b[1]) && edge.a[1].max(edge.b[1])>=a[1].min(b[1]) {emit(edge);}
        }}
    }
}

pub(super) fn crossing_stops(lines: &[Vec<Edge>], others: &[Edge]) -> Vec<Vec<f64>> {
    let tree=Edges::new(lines.iter().flatten().copied().chain(others.iter().copied()).collect());
    lines.iter().map(|line| {
        let mut stops=Vec::new();
        let closed=line.first().zip(line.last()).is_some_and(|(a,b)|a.a==b.b);
        let last=line.len().saturating_sub(1);
        for edge in line {
            tree.near(edge.a,edge.b,&mut |other| {
                let (k,m)=(edge.index,other.index);
                if other.subpath==edge.subpath && (m.abs_diff(k)<=1 || closed && (k==0 && m==last || m==0 && k==last)) {return;}
                if let (Some(t),Some(u))=(crossing(edge.a,edge.b,other.a,other.b),crossing(other.a,other.b,edge.a,edge.b)) {
                    if let Some(place)=refined_crossing(edge,other,t,u) {stops.push(place);}
                } else {
                    let d=[edge.b[0]-edge.a[0],edge.b[1]-edge.a[1]];let length=d[0]*d[0]+d[1]*d[1];
                    if length==0. {return;}
                    for p in [other.a,other.b] {
                        let q=[p[0]-edge.a[0],p[1]-edge.a[1]];
                        if (q[0]*d[1]-q[1]*d[0]).abs()>1e-9*length.sqrt() {continue;}
                        let t=(q[0]*d[0]+q[1]*d[1])/length;
                        if (0. ..=1.).contains(&t) {
                            let (at,_)=super::points::closest_segment(edge.segment,p,edge.from,edge.to);
                            stops.push(edge.base+at);
                        }
                    }
                }
            });
        }
        stops.sort_by(f64::total_cmp);stops.dedup_by(|a,b|(*a-*b).abs()<1e-9);stops
    }).collect()
}

fn build(subpath: &Subpath, piece: &[(usize, f64, f64)]) -> Subpath {
    let segments: Vec<Segment> = piece.iter().map(|&(index, from, to)| portion(subpath.segment(index), from, to)).collect();
    let relative = |handle: [f64; 2], point: [f64; 2]| [handle[0] - point[0], handle[1] - point[1]];
    let anchors: Vec<Anchor> = (0..=segments.len()).map(|k| {
        let point = if k < segments.len() { segments[k].from } else { segments[k - 1].to };
        let incoming = if k > 0 { relative(segments[k - 1].control[1], point) } else { [0.; 2] };
        let outgoing = if k < segments.len() { relative(segments[k].control[0], point) } else { [0.; 2] };
        let cut = (k == 0 && piece[0].1 > 0.) || (k == segments.len() && piece[k - 1].2 < 1.);
        let original = if k < segments.len() { piece[k].0 } else { (piece[k - 1].0 + 1) % subpath.len() };
        let kind = match subpath.anchor(original).kind {
            _ if cut || k == 0 || k == segments.len() => AnchorKind::CORNER,
            AnchorKind::SYMMETRIC if (incoming[0].hypot(incoming[1]) - outgoing[0].hypot(outgoing[1])).abs() > 1e-6 => AnchorKind::SMOOTH,
            kind => kind,
        };
        Anchor::new(point, incoming, outgoing, kind)
    }).collect();
    Subpath::new(&anchors, false)
}

fn remap(profile: &WidthProfile, origins: &[Origin], pinned: &[bool]) -> Option<WidthProfile> {
    let mut knots: Vec<WidthKnot> = Vec::new();
    let mut push = |knot: WidthKnot| if knots.last().is_none_or(|last| (last.segment(), last.t()) < (knot.segment(), knot.t())) { knots.push(knot); };
    for (j, (origin, &pin)) in origins.iter().zip(pinned).enumerate() {
        let j = j as u32;
        let span = origin.to - origin.from;
        let pin = pin || origin.from > 0. || origin.to < 1.;
        if pin { let [l, r] = profile.at(origin.segment, origin.from); push(WidthKnot::new(j, 0., l, r)); }
        for knot in profile.segment_knots(origin.segment).iter().filter(|k| k.t() >= origin.from && k.t() <= origin.to) {
            push(WidthKnot::new(j, ((knot.t() - origin.from) / span).clamp(0., 1.), knot.left(), knot.right()));
        }
        if pin { let [l, r] = profile.at(origin.segment, origin.to); push(WidthKnot::new(j, 1., l, r)); }
    }
    if knots.is_empty() { let [l, r] = profile.at(origins.first()?.segment, origins[0].from); knots.push(WidthKnot::new(0, 0., l, r)); }
    WidthProfile::new(knots).ok()
}

impl PathItem {
    pub fn touched_by(&self, eraser: &Eraser) -> bool {
        let [x0, y0, x1, y1] = self.bounds();
        let area = eraser.bounds();
        if self.hidden || area[2] < x0 || area[0] > x1 || area[3] < y0 || area[1] > y1 { return false; }
        self.style.strokes().any(|stroke| {
            if stroke.cap == Cap::Round && stroke.join == Join::Round {
                outline::capsules(self, stroke, (eraser.radius * 0.05).clamp(1. / GRID, 0.05), |a, b| {
                    if eraser.covers(a.p, a.r) || eraser.covers(b.p, b.r)
                        || outline::capsule_distance(eraser.from, a, b) <= eraser.radius || outline::capsule_distance(eraser.to, a, b) <= eraser.radius { return true; }
                    let d = [b.p[0] - a.p[0], b.p[1] - a.p[1]];
                    let length = d[0].hypot(d[1]);
                    if length > (a.r - b.r).abs() {
                        let u = d.map(|v| v / length);
                        let s = (a.r - b.r) / length;
                        let c = (1. - s * s).sqrt();
                        for sign in [-1., 1.] {
                            let n = [u[0] * s + u[1] * c * sign, u[1] * s - u[0] * c * sign];
                            if eraser.meets([a.p[0] + n[0] * a.r, a.p[1] + n[1] * a.r], [b.p[0] + n[0] * b.r, b.p[1] + n[1] * b.r], 0.) { return true; }
                        }
                    }
                    false
                })
            } else { self.touches(eraser.from, stroke, 0., 0.05) || self.touches(eraser.to, stroke, 0., 0.05) || self.stroke_outline(stroke, 1. / GRID).iter().any(|ring| ring.iter().zip(ring.iter().cycle().skip(1)).any(|(a, b)| eraser.meets(*a, *b, 0.))) }
        }) || self.style.entries.iter().any(|entry| matches!(entry, PaintEntry::Fill(_))) && (self.winds(eraser.from, 1. / GRID) || self.winds(eraser.to, 1. / GRID) || self.geometry.subpaths().iter().any(|subpath| {
            let mut last = subpath.anchor(0).point();
            let first = last;
            let mut hit = false;
            for segment in subpath.segments() { segment.flatten(1. / GRID, |p, _| { hit |= eraser.meets(last, p, 0.); last = p; }); }
            hit || eraser.meets(last, first, 0.)
        }))
    }
    pub fn erased(&self, eraser: &Eraser, crossed: Option<&[&PathItem]>) -> Option<Vec<PathItem>> {
        self.erased_up_to(eraser, crossed.map(|others| move || others.to_vec()))
    }
    fn erased_up_to<'a>(&self, eraser: &Eraser, crossed: Option<impl FnOnce() -> Vec<&'a PathItem>>) -> Option<Vec<PathItem>> {
        let [x0, y0, x1, y1] = self.bounds();
        let area = eraser.bounds();
        if area[2] < x0 || area[0] > x1 || area[3] < y0 || area[1] > y1 { return None; }
        if self.style.strokes().any(|s| s.cap != Cap::Round || s.join != Join::Round) && !self.touched_by(eraser) { return None; }
        let mut base = 0u32;
        let cuts: Vec<Option<Vec<(f64, f64)>>> = self.geometry.subpaths().iter().scan(0u32, |base, subpath| {
            let at = *base; *base += subpath.segment_count() as u32; Some(cut(self, subpath, at, eraser))
        }).collect();
        if cuts.iter().all(Option::is_none) { return None; }
        let stops = crossed.map(|others| {
            let lines=self.geometry.subpaths().iter().enumerate().map(|(i,s)|path_edges(s,i,0.05)).collect::<Vec<_>>();
            let others=others().into_iter().filter(|path|!path.hidden && path.style.strokes().next().is_some())
                .flat_map(|path|path.geometry.subpaths().iter().flat_map(|s|path_edges(s,usize::MAX,0.05))).collect::<Vec<_>>();
            crossing_stops(&lines,&others)
        });
        let results: Vec<Option<Pieces>> = self.geometry.subpaths().iter().zip(cuts).enumerate()
            .map(|(i, (subpath, cut))| cut.map(|removed| kept(subpath, removed, stops.as_ref().map(|stops| stops[i].as_slice())))).collect();
        let compound = self.geometry.subpaths().len() > 1;
        let mut outputs: Vec<(Vec<Subpath>, Vec<Origin>, Vec<bool>)> = vec![Default::default()];
        for (subpath, result) in self.geometry.subpaths().iter().zip(results) {
            match result {
                None => {
                    let out = outputs.last_mut().unwrap();
                    out.0.push(subpath.clone());
                    for i in 0..subpath.segment_count() { out.1.push(Origin { segment: base + i as u32, from: 0., to: 1. }); out.2.push(i == 0 || i + 1 == subpath.segment_count()); }
                }
                Some(pieces) => for mut piece in pieces {
                    if stops.is_some() {
                        let trim_start = subpath.closed || piece[0].0 != 0 || piece[0].1 != 0.;
                        let last = piece.last().unwrap();
                        let trim_end = subpath.closed || last.0 + 1 != subpath.segment_count() || last.2 != 1.;
                        if trim_start && !trim_cap(self, subpath, base, &mut piece, false) || trim_end && !trim_cap(self, subpath, base, &mut piece, true) { continue; }
                    }
                    if !compound && !outputs.last().unwrap().0.is_empty() { outputs.push(Default::default()); }
                    let out = outputs.last_mut().unwrap();
                    out.0.push(build(subpath, &piece));
                    let last = piece.len() - 1;
                    for (k, &(index, from, to)) in piece.iter().enumerate() { out.1.push(Origin { segment: base + index as u32, from, to }); out.2.push(k == 0 || k == last); }
                },
            }
            base += subpath.segment_count() as u32;
        }
        Some(outputs.into_iter().filter(|(subpaths, _, _)| !subpaths.is_empty()).filter_map(|(subpaths, origins, pinned)| {
            let geometry = PathGeometry::new(subpaths).ok()?;
            let entries = self.style.entries.iter().map(|entry| match entry {
                PaintEntry::Stroke(stroke) => PaintEntry::Stroke(Stroke { profile: stroke.profile.as_ref().and_then(|p| remap(p, &origins, &pinned)).map(Arc::new), ..stroke.clone() }),
                other => other.clone(),
            }).collect();
            Some(PathItem { style: Arc::new(Style::new(entries)), ..self.with_geometry(Arc::new(geometry)) })
        }).collect())
    }
}

impl VectorObject {
    pub fn erase(&self, index: &mut VectorIndex, eraser: &Eraser, mode: EraseMode) -> Option<Self> {
        self.erase_along(index, std::slice::from_ref(eraser), mode)
    }
    pub fn erase_along(&self, index: &mut VectorIndex, erasers: &[Eraser], mode: EraseMode) -> Option<Self> {
        let mut changes: BTreeMap<Position, Vec<Record>> = BTreeMap::new();
        let mut fresh = self.allocate_ids();
        for eraser in erasers {
            let mut erased = Vec::new();
            for position in index.near(self, eraser.bounds()) {
                let current = changes.get(&position).map_or_else(|| self.record(position).map(std::slice::from_ref).unwrap_or_default(), Vec::as_slice);
                let mut records = Vec::with_capacity(current.len());
                let mut cut = false;
                for record in current {
                    let Some(path) = record.path().filter(|path| path.pickable()) else { records.push(record.clone()); continue };
                    let replacement = match mode {
                        EraseMode::Whole => path.touched_by(eraser).then(Vec::new),
                        EraseMode::Touched => path.erased(eraser, None),
                        EraseMode::ToIntersection => path.erased_up_to(eraser, Some(|| index.near(self, path.bounds()).into_iter()
                            .flat_map(|p| changes.get(&p).map_or_else(|| self.record(p).map(std::slice::from_ref).unwrap_or_default(), Vec::as_slice))
                            .filter_map(Record::path).filter(|other| other.id != path.id).collect())),
                    };
                    let Some(pieces) = replacement else { records.push(record.clone()); continue };
                    cut = true;
                    records.extend(pieces.into_iter().enumerate().map(|(i, piece)| {
                        let id = if i == 0 { path.id } else { fresh.next().unwrap_or_default() };
                        Record::Path(PathItem { id, ..piece })
                    }));
                }
                if cut { erased.push((position, records)); }
            }
            changes.extend(erased);
        }
        (!changes.is_empty()).then(|| self.splice(changes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn line(points: &[[f64; 2]], width: f64, cap: Cap) -> PathItem {
        let mut stroke = Stroke::new(Paint::color(crate::color::RgbColor::BLACK), width);
        stroke.cap = cap; stroke.join = Join::Round;
        PathItem::new(ItemId(1), Arc::new(PathGeometry::new(vec![Subpath::new(&points.iter().copied().map(Anchor::corner).collect::<Vec<_>>(), false)]).unwrap()), Arc::new(Style::new(vec![PaintEntry::Stroke(stroke)])))
    }
    fn eraser(at: [f64; 2], radius: f64) -> Eraser { Eraser { from: at, to: at, radius } }
    #[test]
    fn small_erasers_touch_visible_edges_and_leave_visible_gaps_in_thick_ink() {
        let path = line(&[[0., 50.], [200., 50.]], 40., Cap::Round);
        assert!(path.erased(&eraser([100., 65.], 3.), None).is_some(), "the visible edge is erasable");
        let pieces = path.erased(&eraser([100., 50.], 5.), None).unwrap();
        for x in 96..105 { assert!(!pieces.iter().any(|p| p.hit([f64::from(x), 50.], 0.)), "ink remains at {x}"); }
        for x in [0., 50., 150., 200.] { assert!(pieces.iter().any(|p| p.hit([x, 50.], 0.)), "untouched ink at {x} remains"); }
        assert!(pieces.iter().all(|p| p.style == path.style));
    }
    #[test]
    fn erasing_flat_and_square_ended_ink_clears_the_brush_and_keeps_nearby_ink() {
        for cap in [Cap::Butt, Cap::Square] {
            let path = line(&[[0., 50.], [200., 50.]], 40., cap);
            for y in [50., 65.] {
                let pieces = path.erased(&eraser([100., y], 3.), None).unwrap();
                assert!(!pieces.iter().any(|p| p.hit([100., y], 0.)), "{cap:?}: ink remains beneath the eraser at {y}");
                for x in [95., 105.] { assert!(pieces.iter().any(|p| p.hit([x, y], 0.)), "{cap:?}: untouched ink disappeared at {x},{y}"); }
            }
            let long = line(&[[0., 50.], [10_000., 50.]], 40., cap);
            assert!(long.erased(&Eraser { from: [0., 65.], to: [10_000., 65.], radius: 1. }, None).unwrap().is_empty());
        }
    }
    #[test]
    fn a_small_eraser_never_steps_over_a_long_straight_path() {
        let path = line(&[[0., 0.], [10_000., 0.]], 1., Cap::Butt);
        for k in 0..40 { assert!(path.erased(&eraser([5000. + f64::from(k) * 0.25, 0.], 0.5), None).is_some(), "contact {k} was skipped"); }
    }
    #[test]
    fn erasing_a_loop_does_not_treat_its_coincident_ends_as_a_flat_segment() {
        let mut path = line(&[[0., 0.], [0., 0.]], 2., Cap::Round);
        path.geometry = Arc::new(PathGeometry::new(vec![Subpath::new(&[
            Anchor::new([0., 0.], [0.; 2], [100., 100.], AnchorKind::CORNER),
            Anchor::new([0., 0.], [-100., 100.], [0.; 2], AnchorKind::CORNER),
        ], false)]).unwrap());
        assert_eq!(path.erased(&eraser([0., 75.], 1.), None).unwrap().len(), 2);
    }
    #[test]
    fn whole_path_erasing_uses_local_width_and_swept_fills() {
        let mut path = line(&[[0., 50.], [200., 50.]], 40., Cap::Round);
        assert!(path.touched_by(&eraser([100., 50.], 1.)));
        if let PaintEntry::Stroke(stroke) = &mut Arc::make_mut(&mut path.style).entries[0] {
            stroke.profile = Some(Arc::new(WidthProfile::new(vec![WidthKnot::new(0, 0., 0.1, 0.1), WidthKnot::new(0, 1., 1., 1.)]).unwrap()));
        }
        assert!(!path.touched_by(&eraser([10., 65.], 1.)));
        assert!(path.touched_by(&eraser([190., 65.], 1.)));
        let fill = line(&[[100., 100.], [200., 100.], [200., 200.], [100., 200.]], 0., Cap::Butt)
            .with_style(Arc::new(Style::new(vec![PaintEntry::Fill(Fill { paint: Paint::color(crate::color::RgbColor::BLACK), unknown: None })])));
        assert!(fill.touched_by(&Eraser { from: [50., 150.], to: [250., 150.], radius: 5. }));
        assert!(fill.touched_by(&eraser([95., 150.], 6.)));
        assert!(!fill.touched_by(&eraser([90., 150.], 6.)));
    }
    #[test]
    fn trimming_uses_the_same_curve_crossing_at_every_eraser_size() {
        let mut path = line(&[[0., 0.], [200., 0.]], 4., Cap::Butt);
        path.geometry = Arc::new(PathGeometry::new(vec![Subpath::new(&[
            Anchor::new([0., 0.], [0.; 2], [0., 200.], AnchorKind::CORNER),
            Anchor::new([200., 0.], [0., 200.], [0.; 2], AnchorKind::CORNER),
        ], false)]).unwrap());
        let other = line(&[[-100., 100.], [300., 100.]], 2., Cap::Butt);
        let mut expected = None;
        for radius in [0.5, 2., 5., 10., 20., 30.] {
            let pieces = path.erased(&eraser([100., 150.], radius), Some(&[&other])).unwrap();
            let ends: Vec<_> = pieces.iter().map(|p| p.open_ends().unwrap()).collect();
            assert!((ends[0][1][1] - 100.).abs() <= 1. / GRID);
            assert!((ends[1][0][1] - 100.).abs() <= 1. / GRID);
            if let Some(expected) = &expected { assert_eq!(&ends, expected); } else { expected = Some(ends); }
        }
        let mut curved = other;
        curved.geometry = Arc::new(PathGeometry::new(vec![Subpath::new(&[
            Anchor::new([-100., 80.], [0.; 2], [120., -50.], AnchorKind::CORNER),
            Anchor::new([300., 120.], [-120., 50.], [0.; 2], AnchorKind::CORNER),
        ], false)]).unwrap());
        let pieces = path.erased(&eraser([100., 150.], 2.), Some(&[&curved])).unwrap();
        assert_eq!(pieces.len(), 2);
        for point in [pieces[0].open_ends().unwrap()[1], pieces[1].open_ends().unwrap()[0]] {
            assert!(curved.nearest_segment(point, 0.001).unwrap().distance <= 1. / GRID, "{point:?} lies at the curved crossing");
        }
    }
    #[test]
    fn trimmed_round_caps_do_not_leave_ink_beyond_the_crossing() {
        let path = line(&[[0., 50.], [200., 50.]], 20., Cap::Round);
        let other = line(&[[100., 0.], [100., 100.]], 10., Cap::Round);
        let pieces = path.erased(&eraser([160., 50.], 2.), Some(&[&other])).unwrap();
        assert!(pieces.iter().any(|p| p.hit([95., 50.], 0.)));
        assert!(!pieces.iter().any(|p| p.hit([105., 50.], 0.)));
        assert!(pieces.iter().any(|p| p.hit([0., 50.], 0.)));
    }
    #[test]
    fn picking_and_marquees_follow_caps_joins_and_visible_bounds() {
        let flat = line(&[[0., 0.], [100., 0.]], 20., Cap::Butt);
        assert!(!flat.hit([108., 0.], 0.));
        let mut sharp = line(&[[50., 100.], [100., 50.], [150., 100.]], 20., Cap::Butt);
        if let PaintEntry::Stroke(stroke) = &mut Arc::make_mut(&mut sharp.style).entries[0] { stroke.join = Join::Miter; }
        assert!(sharp.hit([100., 37.], 0.));
        let object = VectorObject::from_records(vec![Record::Path(flat)]);
        assert_eq!(VectorIndex::default().within(&object, [-1., -11., 101., 11.]), [ItemId(1)].into());
    }
    #[test]
    fn round_pressure_tapers_include_the_entire_rendered_end_disk() {
        let mut path = line(&[[0., 0.], [10., 0.]], 200., Cap::Round);
        if let PaintEntry::Stroke(stroke) = &mut Arc::make_mut(&mut path.style).entries[0] {
            stroke.profile = Some(Arc::new(WidthProfile::new(vec![WidthKnot::new(0, 0., 0., 0.), WidthKnot::new(0, 1., 1., 1.)]).unwrap()));
        }
        assert!(path.hit([-50., 0.], 0.));
        assert!(path.touched_by(&eraser([-50., 0.], 1.)));
        assert_eq!(path.visual_bounds(), [-90., -100., 110., 100.]);
    }
    #[test]
    fn asymmetric_ink_erases_only_on_its_visible_side() {
        let mut path = line(&[[0., 0.], [10_000., 0.]], 40., Cap::Round);
        if let PaintEntry::Stroke(stroke) = &mut Arc::make_mut(&mut path.style).entries[0] {
            stroke.profile = Some(Arc::new(WidthProfile::new(vec![WidthKnot::new(0, 0., 0., 1.)]).unwrap()));
        }
        assert!(path.erased(&eraser([5000., -10.], 1.), None).is_none());
        assert!(path.erased(&Eraser { from: [0., 10.], to: [10_000., 10.], radius: 1. }, None).unwrap().is_empty());
        let pieces = path.erased(&Eraser { from: [2000., 10.], to: [8000., -10.], radius: 1. }, None).unwrap();
        assert!(pieces.iter().any(|p| p.hit([7000., 10.], 0.)));
        let flat = line(&[[0., 0.], [100., 0.]], 40., Cap::Butt);
        assert!(flat.erased(&eraser([110., 0.], 1.), None).is_none());
        let mut dot = line(&[[0., 0.]], 40., Cap::Round);
        if let PaintEntry::Stroke(stroke) = &mut Arc::make_mut(&mut dot.style).entries[0] {
            stroke.profile = Some(Arc::new(WidthProfile::new(vec![WidthKnot::new(0, 0., 0., 1.)]).unwrap()));
        }
        assert!(dot.erased(&eraser([15., 0.], 1.), None).is_none());
        assert!(dot.erased(&eraser([10., 0.], 1.), None).unwrap().is_empty());
    }
}
