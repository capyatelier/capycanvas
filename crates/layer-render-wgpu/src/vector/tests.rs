use super::tiles::{self, Frame, LevelGeometry, capsule, capsule_distance};
use super::{Arena, Batch, Kernel, Layer, Pool};
use layer_core::authored::vector::*;
use layer_core::color::RgbColor;
use std::sync::Arc;

const SIZE: u32 = 256;

impl Batch {
    pub fn command_count(&self) -> usize { self.commands.len() }
    pub fn bytes(&self) -> usize { 32 + self.tiles.len() * 16 + self.commands.len() * 16 + self.colors.len() * 16 }
}
impl Pool {
    pub fn new(limit_bytes: u64) -> Self { Self::charged(limit_bytes, &Default::default()) }
    pub fn used_words(&self) -> u32 { self.capacity - self.free.values().sum::<u32>() }
    pub fn storage_bytes(&self) -> u64 { self.buffer.as_ref().map_or(0, wgpu::Buffer::size) }
}
impl Arena {
    pub fn storage_bytes(&self) -> u64 { self.buffer.as_ref().map_or(0, wgpu::Buffer::size) }
}

fn paint(rgba: [f32; 4]) -> Paint { Paint::color(RgbColor::new(Default::default(), rgba).unwrap()) }
fn smooth(points: &[[f64; 2]], closed: bool) -> Subpath {
    let n = points.len();
    let anchors: Vec<_> = (0..n).map(|i| {
        let previous = points[if i == 0 { if closed { n - 1 } else { 0 } } else { i - 1 }];
        let next = points[if i + 1 == n { if closed { 0 } else { n - 1 } } else { i + 1 }];
        let tangent = [(next[0] - previous[0]) / 6., (next[1] - previous[1]) / 6.];
        Anchor::new(points[i], [-tangent[0], -tangent[1]], tangent, AnchorKind::SMOOTH)
    }).collect();
    Subpath::new(&anchors, closed)
}
fn path(subpaths: Vec<Subpath>, entries: Vec<PaintEntry>, rule: FillRule) -> PathItem {
    let mut item = PathItem::new(ItemId(1), Arc::new(PathGeometry::new(subpaths).unwrap()), Arc::new(Style::new(entries)));
    item.fill_rule = rule;
    item
}
fn frame(level: u32) -> Frame { let scale = 1. / f64::from(1 << level); Frame { scale, origin: [0.; 2], clip: [-4, -4, 64, 64] } }
fn stroke(width: f64, cap: Cap, join: Join, profile: Option<WidthProfile>) -> PaintEntry {
    let mut stroke = Stroke::new(paint([0.1, 0.2, 0.9, 1.]), width);
    stroke.cap = cap; stroke.join = join; stroke.profile = profile.map(Arc::new);
    PaintEntry::Stroke(stroke)
}
fn round_stroke(width: f64, profile: Option<WidthProfile>) -> PaintEntry { stroke(width, Cap::Round, Join::Round, profile) }

fn scenes() -> Vec<(&'static str, PathItem)> {
    let star: Vec<[f64; 2]> = (0..10).map(|i| { let a = i as f64 * std::f64::consts::PI / 5.; let r = if i % 2 == 0 { 100. } else { 38. }; [128. + r * a.sin(), 130. - r * a.cos()] }).collect();
    let corners = |points: &[[f64; 2]], closed| Subpath::new(&points.iter().map(|p| Anchor::corner(*p)).collect::<Vec<_>>(), closed);
    let fill = |rgba| PaintEntry::Fill(Fill { paint: paint(rgba), unknown: None });
    let blob = smooth(&[[40., 60.], [200., 30.], [230., 190.], [120., 240.], [20., 170.]], true);
    let hole = smooth(&[[90., 110.], [150., 100.], [160., 160.], [100., 170.]], true);
    let open_point = || corners(&[[0., 0.]], false);
    vec![
        ("nonzero star", path(vec![corners(&star, true)], vec![fill([0.8, 0.3, 0.1, 1.])], FillRule::NonZero)),
        ("even-odd pentagram", path(vec![corners(&(0..5).map(|i| star[(i * 4) % 10]).collect::<Vec<_>>(), true)], vec![fill([0.2, 0.6, 0.3, 0.8])], FillRule::EvenOdd)),
        ("curved compound", path(vec![blob.clone(), hole], vec![fill([0.3, 0.3, 0.7, 1.])], FillRule::EvenOdd)),
        ("hairline and wide strokes", path(vec![smooth(&[[10., 20.], [120., 60.], [240., 30.]], false), smooth(&[[20., 200.], [128., 140.], [236., 210.]], false)],
            vec![round_stroke(0.4, None)], FillRule::NonZero)),
        ("tapered ink", path(vec![smooth(&[[30., 40.], [90., 200.], [170., 60.], [230., 220.]], false)],
            vec![round_stroke(14., Some(WidthProfile::new(vec![WidthKnot::new(0, 0., 0., 0.), WidthKnot::new(1, 0.5, 1., 1.), WidthKnot::new(2, 1., 0.05, 0.05)]).unwrap()))], FillRule::NonZero)),
        ("straight pressure peak", path(vec![corners(&[[30., 128.], [220., 128.]], false)],
            vec![round_stroke(40., Some(WidthProfile::new(vec![WidthKnot::new(0, 0., 0.1, 0.1), WidthKnot::new(0, 0.5, 1., 1.), WidthKnot::new(0, 1., 0.1, 0.1)]).unwrap()))], FillRule::NonZero)),
        ("thick round stroke over fill", path(vec![blob], vec![fill([0.9, 0.9, 0.2, 1.]), round_stroke(40., None)], FillRule::NonZero)),
        ("miter outline", path(vec![corners(&[[30., 200.], [128., 40.], [226., 200.]], false)], vec![PaintEntry::Stroke(Stroke::new(paint([0., 0., 0., 1.]), 12.))], FillRule::NonZero)),
        ("bevel zigzag with square caps", path(vec![corners(&[[20., 60.], [80., 200.], [140., 40.], [200., 210.], [240., 90.]], false)], vec![stroke(14., Cap::Square, Join::Bevel, None)], FillRule::NonZero)),
        ("closed miter star", path(vec![corners(&star, true)], vec![stroke(10., Cap::Butt, Join::Miter, None)], FillRule::NonZero)),
        ("rounded rectangle outline", PathItem::new(ItemId(1), Arc::new(PathGeometry::new(vec![open_point()]).unwrap()), Arc::new(Style::new(vec![fill([0.9, 0.8, 0.3, 0.9]), stroke(12., Cap::Butt, Join::Miter, None)])))
            .with_shape(LiveShape { kind: ShapeKind::Rectangle { radii: [0., 20., 40., 8.] }, center: [128., 128.], size: [180., 120.], rotation: 0.3 }).unwrap()),
        ("one-sided round caps on miter joins", path(vec![corners(&[[30., 50.], [120., 210.], [150., 60.], [230., 120.]], false), smooth(&[[20., 230.], [200., 236.]], false)],
            vec![stroke(16., Cap::Round, Join::Miter, Some(WidthProfile::new(vec![WidthKnot::new(0, 0., 0., 1.), WidthKnot::new(2, 1., 1., 0.2), WidthKnot::new(3, 1., 0.5, 0.5)]).unwrap()))], FillRule::NonZero)),
        ("round joins with butt caps", path(vec![corners(&[[30., 200.], [90., 40.], [170., 220.], [230., 60.]], false)], vec![stroke(18., Cap::Butt, Join::Round, None)], FillRule::NonZero)),
        ("round dots in one path", path(vec![corners(&[[60., 60.]], false), corners(&[[180., 90.]], false), corners(&[[120., 200.], [120., 200.]], false)], vec![round_stroke(30., None)], FillRule::NonZero)),
        ("square dot and a closed two-anchor lens", path(vec![corners(&[[50., 50.]], false), smooth(&[[80., 190.], [200., 140.]], true)], vec![stroke(12., Cap::Square, Join::Miter, None)], FillRule::NonZero)),
        ("a corner past its miter limit", path(vec![corners(&[[30., 60.], [230., 90.], [30., 120.]], false)], vec![PaintEntry::Stroke(Stroke { miter_limit: 1.5, ..Stroke::new(paint([0.4, 0.1, 0.6, 1.]), 14.) })], FillRule::NonZero)),
        ("fill under a zero-width outline", path(vec![smooth(&[[60., 60.], [200., 80.], [150., 220.]], true)], vec![fill([0.2, 0.7, 0.7, 1.]), round_stroke(0., None)], FillRule::NonZero)),
    ]
}

fn colors(item: &PathItem) -> Vec<[f32; 4]> {
    item.style.entries.iter().map(|e| {
        let rgba = match e { PaintEntry::Fill(f) => f.paint.rgb(), PaintEntry::Stroke(s) => s.paint.rgb(), _ => None }.unwrap().rgba;
        [rgba[0] * rgba[3], rgba[1] * rgba[3], rgba[2] * rgba[3], rgba[3]]
    }).collect()
}

fn emulate(batch: &Batch, pool: &[u32]) -> Vec<[f64; 4]> {
    let mut out = vec![[0.; 4]; (SIZE * SIZE) as usize];
    for tile in &batch.tiles {
        for py in 0..16 { for px in 0..16 {
            let mut color = [0f64; 4];
            let mut stroke = 0f64;
            for command in &batch.commands[tile[2] as usize..tile[3] as usize] {
                let [header, start, backdrop, palette] = *command;
                let (kind, count) = (header & 15, header >> 8);
                let coverage = match kind {
                    0 => 1.,
                    3 => {
                        for i in 0..count {
                            let c = capsule(&pool[(start + i * 3) as usize..]);
                            let thin = (c[5] + c[6]).min(1.);
                            let at = [(i64::from(tile[0] + px) + batch.origin[0]) as f64 + 0.5, (i64::from(tile[1] + py) + batch.origin[1]) as f64 + 0.5];
                            stroke = stroke.max((0.5 - capsule_distance(at, &c)).clamp(0., 1.) * thin);
                        }
                        if header & tiles::CONTINUE != 0 { continue; }
                        std::mem::take(&mut stroke)
                    }
                    _ => {
                        let mut acc = f64::from(backdrop as i32);
                        for i in 0..count {
                            let packed = [pool[(start + i * 2) as usize], pool[(start + i * 2 + 1) as usize]];
                            let p0 = [f64::from(packed[0] & 0xffff) / 2048., f64::from(packed[0] >> 16) / 2048.];
                            let p1 = [f64::from(packed[1] & 0xffff) / 2048., f64::from(packed[1] >> 16) / 2048.];
                            let d = [p1[0] - p0[0], p1[1] - p0[1]];
                            let y = p0[1] - py as f64;
                            let (y0, y1) = (y.clamp(0., 1.), (y + d[1]).clamp(0., 1.));
                            let dy = y0 - y1;
                            if dy != 0. {
                                let (t0, t1) = ((y0 - y) / d[1], (y1 - y) / d[1]);
                                let start = p0[0] - px as f64;
                                let (x0, x1) = (start + t0 * d[0], start + t1 * d[0]);
                                let (xmin, xmax) = (x0.min(x1).min(1.) - 1e-9, x0.max(x1));
                                let b = xmax.min(1.); let c = b.max(0.); let dd = xmin.max(0.);
                                acc += (b + 0.5 * (dd * dd - c * c) - xmin) / (xmax - xmin) * dy;
                            }
                            let edge = if packed[0] & 0xffff == 0 { p0[1] } else if packed[1] & 0xffff == 0 { p1[1] } else { 1e9 };
                            acc += d[0].signum() * (py as f64 - edge + 1.).clamp(0., 1.) * f64::from(u8::from(d[0] != 0.));
                        }
                        if kind == 2 { (acc - 2. * (0.5 * acc).round()).abs() } else { acc.abs().min(1.) }
                    }
                };
                let source = batch.colors[palette as usize].map(|v| f64::from(v) * coverage);
                for k in 0..4 { color[k] = source[k] + color[k] * (1. - source[3]); }
            }
            let (x, y) = (tile[0] + px, tile[1] + py);
            if (0..SIZE as i32).contains(&x) && (0..SIZE as i32).contains(&y) { out[(y as u32 * SIZE + x as u32) as usize] = color; }
        }}
    }
    out
}

type FineGeometry = (Vec<Vec<[f64; 2]>>, Vec<(Vec<([f64; 2], [f64; 2])>, bool)>);
fn fine(item: &PathItem, level: u32) -> FineGeometry {
    let scale = 1. / f64::from(1 << level);
    let map = |p: [f64; 2]| [p[0] * scale, p[1] * scale];
    let stroke = item.style.strokes().next();
    let mut rings = Vec::new();
    let mut lines = Vec::new();
    let mut base = 0u32;
    for subpath in item.geometry.subpaths() {
        let width = |s: u32, t: f64| stroke.map_or([0.; 2], |k| k.profile.as_ref().map_or([1.; 2], |w| w.at(s, t)).map(|f| f * k.width * 0.5 * scale));
        let mut ring = vec![map(subpath.anchor(0).point())];
        let mut line = vec![(map(subpath.anchor(0).point()), width(base, 0.))];
        for (i, segment) in subpath.segments().enumerate() {
            let mapped = segment.map(map);
            let mut parameters = Vec::new();
            mapped.flatten(0.002, |p, t| { ring.push(p); parameters.push(t); });
            if let Some(profile) = stroke.and_then(|s| s.profile.as_ref()) {
                parameters.extend(profile.segment_knots(base + i as u32).iter().map(WidthKnot::t));
                parameters.extend((1..64).map(|k| f64::from(k) / 64.));
                parameters.sort_by(f64::total_cmp);
                parameters.dedup();
            }
            for t in parameters { line.push((mapped.point(t), width(base + i as u32, t))); }
        }
        base += subpath.segment_count() as u32;
        rings.push(ring);
        lines.push((line, subpath.closed));
    }
    (rings, lines)
}
fn winding(rings: &[Vec<[f64; 2]>], p: [f64; 2]) -> i32 {
    let mut w = 0;
    for ring in rings { for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        if (a[1] <= p[1]) != (b[1] <= p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if x < p[0] { w += if b[1] < a[1] { 1 } else { -1 }; }
        }
    }}
    w
}

enum Piece { Polygon(Vec<[f64; 2]>), Disc([f64; 2], f64) }
impl Piece {
    fn bounds(&self) -> [f64; 4] {
        match self {
            Piece::Polygon(points) => points.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])]),
            Piece::Disc(c, r) => [c[0] - r, c[1] - r, c[0] + r, c[1] + r],
        }
    }
    fn contains(&self, p: [f64; 2]) -> bool {
        match self {
            Piece::Polygon(points) => winding(std::slice::from_ref(points), p) != 0,
            Piece::Disc(c, r) => (p[0] - c[0]).hypot(p[1] - c[1]) <= *r,
        }
    }
}
fn offset(p: [f64; 2], n: [f64; 2], s: f64) -> [f64; 2] { [p[0] + n[0] * s, p[1] + n[1] * s] }
fn sector(center: [f64; 2], from: [f64; 2], sweep: f64, radius: f64) -> Piece {
    let start = from[1].atan2(from[0]);
    Piece::Polygon(std::iter::once(center).chain((0..=64).map(|k| { let a = start + sweep * f64::from(k) / 64.; [center[0] + radius * a.cos(), center[1] + radius * a.sin()] })).collect())
}
fn stroke_pieces(line: &[([f64; 2], [f64; 2])], closed: bool, stroke: &Stroke) -> Vec<Piece> {
    let mut points = line.to_vec();
    points.dedup_by(|a, b| (a.0[0] - b.0[0]).hypot(a.0[1] - b.0[1]) < 1e-9);
    if closed && points.len() > 2 && (points[0].0[0] - points[points.len() - 1].0[0]).hypot(points[0].0[1] - points[points.len() - 1].0[1]) < 1e-9 { points.pop(); }
    let n = points.len();
    let cap = |(p, [l, r]): ([f64; 2], [f64; 2]), o: [f64; 2]| {
        let (normal, h) = ([o[1], -o[0]], (l + r) * 0.5);
        match stroke.cap {
            Cap::Butt => None,
            Cap::Square => Some(Piece::Polygon(vec![offset(p, normal, l), offset(offset(p, normal, l), o, h), offset(offset(p, normal, -r), o, h), offset(p, normal, -r)])),
            Cap::Round => Some(sector(offset(p, normal, (l - r) * 0.5), normal, std::f64::consts::PI, h)),
        }
    };
    if n == 1 {
        let (p, [l, r]) = points[0];
        let h = (l + r) * 0.5;
        return match stroke.cap {
            Cap::Butt => Vec::new(),
            Cap::Square => vec![Piece::Polygon(vec![[p[0] - h, p[1] - h], [p[0] + h, p[1] - h], [p[0] + h, p[1] + h], [p[0] - h, p[1] + h]])],
            Cap::Round => vec![Piece::Disc(p, h)],
        };
    }
    let closed = closed && n > 2;
    let direction = |i: usize| { let (a, b) = (points[i % n].0, points[(i + 1) % n].0); let l = (b[0] - a[0]).hypot(b[1] - a[1]); [(b[0] - a[0]) / l, (b[1] - a[1]) / l] };
    let left = |d: [f64; 2]| [d[1], -d[0]];
    let mut pieces = Vec::new();
    for i in 0..if closed { n } else { n - 1 } {
        let ((a, wa), (b, wb)) = (points[i], points[(i + 1) % n]);
        let normal = left(direction(i));
        pieces.push(Piece::Polygon(vec![offset(a, normal, wa[0]), offset(b, normal, wb[0]), offset(b, normal, -wb[1]), offset(a, normal, -wa[1])]));
    }
    for i in if closed { 0..n } else { 1..n - 1 } {
        let (p, w) = points[i];
        let (d0, d1) = (direction(i + n - 1), direction(i));
        let (n0, n1) = (left(d0), left(d1));
        let side = if d1[0] * n0[0] + d1[1] * n0[1] < 0. { 1. } else { -1. };
        let (width, inner) = if side > 0. { (w[0], w[1]) } else { (w[1], w[0]) };
        let (e0, e1) = (offset(p, n0, side * width), offset(p, n1, side * width));
        pieces.push(Piece::Polygon(vec![p, offset(p, n0, -side * inner), offset(p, n1, -side * inner)]));
        let cos = 1. + n0[0] * n1[0] + n0[1] * n1[1];
        pieces.push(match stroke.join {
            Join::Round => sector(p, [n0[0] * side, n0[1] * side], (n0[0] * n1[1] - n0[1] * n1[0]).atan2(n0[0] * n1[0] + n0[1] * n1[1]), width),
            Join::Miter if (2. / cos).sqrt() <= stroke.miter_limit => Piece::Polygon(vec![p, e0, offset(p, [n0[0] + n1[0], n0[1] + n1[1]], side * width / cos), e1]),
            _ => Piece::Polygon(vec![p, e0, e1]),
        });
    }
    if !closed {
        let (end, start) = (direction(n - 2), direction(0));
        pieces.extend(cap(points[n - 1], end));
        pieces.extend(cap((points[0].0, [points[0].1[1], points[0].1[0]]), [-start[0], -start[1]]));
    }
    pieces
}
fn inside_stroke(segments: &[[([f64; 2], [f64; 2]); 2]], p: [f64; 2]) -> bool {
    segments.iter().any(|w| {
        let (a, b) = (w[0].0, w[1].0);
        let d = [b[0] - a[0], b[1] - a[1]];
        let l2 = d[0] * d[0] + d[1] * d[1];
        let t = if l2 > 0. { (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0., 1.) } else { 0. };
        let q = [a[0] + d[0] * t - p[0], a[1] + d[1] * t - p[1]];
        (q[0] * q[0] + q[1] * q[1]).sqrt() <= w[0].1[0] + (w[1].1[0] - w[0].1[0]) * t
    })
}
fn reference(item: &PathItem, level: u32) -> Vec<[f64; 4]> {
    let (rings, lines) = fine(item, level);
    let colors = colors(item);
    let pieces: Vec<(Piece, [f64; 4])> = item.style.strokes().next().filter(|s| s.join != Join::Round || s.cap != Cap::Round)
        .map(|s| lines.iter().flat_map(|(line, closed)| stroke_pieces(line, *closed, s)).map(|piece| { let b = piece.bounds(); (piece, b) }).collect()).unwrap_or_default();
    let segments: Vec<_> = lines.iter().flat_map(|(line, _)| {
        if line.len() == 1 { vec![[line[0], line[0]]] } else { line.windows(2).map(|w| [w[0], w[1]]).collect() }
    }).collect();
    let n = 8;
    (0..SIZE * SIZE).map(|i| {
        let (x, y) = (f64::from(i % SIZE), f64::from(i / SIZE));
        let near: Vec<&Piece> = pieces.iter().filter(|(_, b)| b[0] <= x + 1. && b[2] >= x && b[1] <= y + 1. && b[3] >= y).map(|(piece, _)| piece).collect();
        let near_strokes: Vec<_> = segments.iter().copied().filter(|w| {
            let r = w[0].1[0].max(w[1].1[0]);
            w[0].0[0].min(w[1].0[0]) - r <= x + 1. && w[0].0[0].max(w[1].0[0]) + r >= x
                && w[0].0[1].min(w[1].0[1]) - r <= y + 1. && w[0].0[1].max(w[1].0[1]) + r >= y
        }).collect();
        let mut color = [0f64; 4];
        for (entry, paint) in item.style.entries.iter().enumerate() {
            let mut hits = 0;
            for sy in 0..n { for sx in 0..n {
                let p = [x + (sx as f64 + 0.5) / n as f64, y + (sy as f64 + 0.5) / n as f64];
                let inside = match paint {
                    PaintEntry::Fill(_) => { let w = winding(&rings, p); if item.fill_rule == FillRule::EvenOdd { w % 2 != 0 } else { w != 0 } }
                    _ if pieces.is_empty() => inside_stroke(&near_strokes, p),
                    _ => near.iter().any(|piece| piece.contains(p)),
                };
                hits += usize::from(inside);
            }}
            let coverage = hits as f64 / (n * n) as f64;
            let source = colors[entry].map(|v| f64::from(v) * coverage);
            for k in 0..4 { color[k] = source[k] + color[k] * (1. - source[3]); }
        }
        color
    }).collect()
}
fn batch(item: &PathItem, level: u32) -> (LevelGeometry, Batch) {
    let geometry = tiles::derive(item, &frame(level));
    let colors = colors(item);
    let mut batch = Batch::default();
    batch.build(&[Layer { geometry: &geometry, base: 0, colors: &colors }], [0, 0], [SIZE; 2]);
    (geometry, batch)
}
fn compare(a: &[[f64; 4]], b: &[[f64; 4]]) -> (f64, f64) {
    let errors: Vec<f64> = a.iter().zip(b).map(|(a, b)| (a[3] - b[3]).abs()).collect();
    (errors.iter().copied().fold(0., f64::max), errors.iter().sum::<f64>() / errors.len() as f64)
}

#[test]
fn binned_coverage_matches_the_exact_reference() {
    let mut failures = Vec::new();
    for level in [0, 1] {
        for (name, item) in scenes() {
            let (geometry, batch) = batch(&item, level);
            let emulated = emulate(&batch, &geometry.words);
            let expected = reference(&item, level);
            let (max, mean) = compare(&emulated, &expected);
            let mut worst: Vec<(f64, u32)> = emulated.iter().zip(&expected).enumerate().map(|(i, (a, b))| ((a[3] - b[3]).abs(), i as u32)).collect();
            worst.sort_by(|a, b| b.0.total_cmp(&a.0));
            eprintln!("{name} level {level} worst: {:?}", worst[..4].iter().map(|(e, i)| (format!("{e:.3}"), i % SIZE, i / SIZE, format!("{:.3}/{:.3}", emulated[*i as usize][3], expected[*i as usize][3]))).collect::<Vec<_>>());
            eprintln!("{name} level {level}: max {max:.4} mean {mean:.5} tiles {} commands {}", batch.tile_count(), batch.command_count());
            if !(mean < 0.004 && max < 0.35) { failures.push(format!("{name} level {level}: max {max} mean {mean}")); }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn gpu_kernel_matches_its_emulation() {
    let r = crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let kernel = Kernel::new(&r.device);
    pollster::block_on(r.compilation.prepare_all(kernel.pipelines())).unwrap();
    let mut uploads = crate::Uploads::new(&r.device, 1 << 20);
    let mut pool = Pool::new(64 << 20);
    let mut arena = super::Arena::default();
    for (name, item) in scenes() {
        let geometry = Arc::new(tiles::derive(&item, &frame(0)));
        let base = pool.allocate(&geometry).unwrap();
        let colors = colors(&item);
        let mut batch = Batch::default();
        batch.build(&[Layer { geometry: &geometry, base, colors: &colors }], [0, 0], [SIZE; 2]);
        let output = crate::test_support::page_texture(&r, wgpu::TextureFormat::Rgba32Float);
        let view = output.create_view(&Default::default());
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let buffer = pool.flush(&r.device, &mut encoder, &mut uploads).unwrap();
        kernel.encode(&r.device, &mut encoder, &mut uploads, &mut arena, &batch, &buffer, &view).unwrap();
        uploads.finish(&encoder);
        encoder.submit(&r.queue);
        let gpu = crate::test_support::float_pixels(&r, &output);
        let mut words = vec![0u32; base as usize];
        words.extend_from_slice(&geometry.words);
        let expected = emulate(&batch, &words);
        let max = gpu.iter().zip(&expected).flat_map(|(a, b)| (0..4).map(move |k| (f64::from(a[k]) - b[k]).abs())).fold(0., f64::max);
        eprintln!("{name}: gpu vs emulation max {max:.6}");
        assert!(max < 2e-3, "{name}: {max}");
    }
}

#[test]
#[ignore = "writes SPIR-V of the vector coverage kernel for the Mali Offline Compiler"]
fn coverage_kernel_spirv() {
    let out = std::env::var("CAPY_KERNEL_SPIRV").expect("CAPY_KERNEL_SPIRV names the output file");
    let module = naga::front::wgsl::parse_str(include_str!("coverage.wgsl")).unwrap();
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&module).unwrap();
    let pipeline = naga::back::spv::PipelineOptions { shader_stage: naga::ShaderStage::Compute, entry_point: "main".into() };
    let options = naga::back::spv::Options { lang_version: (1, 3), ..Default::default() };
    let words = naga::back::spv::write_vec(&module, &info, &options, Some(&pipeline)).unwrap();
    std::fs::write(out, words.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>()).unwrap();
}
