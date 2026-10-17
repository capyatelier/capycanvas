use layer_core::authored::vector::{Cap, FillRule, Join, PaintEntry, PathItem, Segment, Stroke, Subpath};

pub(crate) const TILE: i32 = 16;
const UNITS: f64 = 2048.;
const SPAN: i64 = TILE as i64 * 2048;
pub(crate) const TOLERANCE: f64 = 0.25;

pub(crate) const SOLID: u32 = 0;
pub(crate) const NONZERO: u32 = 1;
pub(crate) const EVEN_ODD: u32 = 2;
pub(crate) const STROKE: u32 = 3;

pub(crate) const CONTINUE: u32 = 16;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ItemTile { pub tile: [i32; 2], pub entry: u16, pub kind: u32, pub backdrop: i32, pub start: u32, pub count: u32 }

#[derive(Clone, Debug, Default)]
pub(crate) struct LevelGeometry { pub tiles: Vec<ItemTile>, pub words: Vec<u32> }
impl LevelGeometry {
    pub fn bytes(&self) -> usize { self.tiles.len() * std::mem::size_of::<ItemTile>() + self.words.len() * 4 + 64 }
    pub fn tile_range(&self, rows: std::ops::Range<i32>, columns: std::ops::Range<i32>) -> impl Iterator<Item = &ItemTile> {
        let start = self.tiles.partition_point(|t| (t.tile[1], t.tile[0]) < (rows.start, i32::MIN));
        self.tiles[start..].iter().take_while(move |t| t.tile[1] < rows.end).filter(move |t| columns.contains(&t.tile[0]))
    }
}

pub(crate) struct Frame { pub scale: f64, pub origin: [f64; 2], pub clip: [i32; 4] }
impl Frame {
    fn map(&self, p: [f64; 2]) -> [f64; 2] { [(p[0] + self.origin[0]) * self.scale, (p[1] + self.origin[1]) * self.scale] }
}

pub(crate) fn derive(path: &PathItem, frame: &Frame) -> LevelGeometry {
    let mut out = LevelGeometry::default();
    if path.hidden { return out; }
    for (entry, paint) in path.style.entries.iter().enumerate() {
        match paint {
            PaintEntry::Fill(_) => fill(&mut out, entry as u16, path, frame),
            PaintEntry::Stroke(stroke) if stroke.width > 0. => self::stroke(&mut out, entry as u16, path, stroke, frame),
            _ => {}
        }
    }
    out.tiles.sort_by_key(|t| (t.tile[1], t.tile[0], t.entry));
    out
}

pub(crate) fn flatten(segment: &Segment, map: impl Fn([f64; 2]) -> [f64; 2], tolerance: f64, mut emit: impl FnMut([f64; 2], f64)) {
    let [p0, p1, p2, p3] = [segment.from, segment.control[0], segment.control[1], segment.to].map(&map);
    let second = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| ((a[0] - 2. * b[0] + c[0]).powi(2) + (a[1] - 2. * b[1] + c[1]).powi(2)).sqrt();
    let bend = second(p0, p1, p2).max(second(p1, p2, p3));
    let count = if segment.is_line() { 1 } else { ((0.75 * bend / tolerance).sqrt().ceil() as usize).clamp(1, 4096) };
    let mapped = Segment { from: p0, control: [p1, p2], to: p3 };
    for i in 1..count { let t = i as f64 / count as f64; emit(mapped.point(t), t); }
    emit(p3, 1.);
}

fn polylines(path: &PathItem, frame: &Frame) -> Vec<Vec<[f64; 2]>> {
    path.geometry.subpaths().iter().map(|subpath| {
        let mut points = vec![frame.map(subpath.anchor(0).point)];
        for segment in subpath.segments() { flatten(&segment, |p| frame.map(p), TOLERANCE, |p, _| points.push(p)); }
        points
    }).collect()
}

fn odd(value: f64) -> i64 { 2 * ((value - 1.) * 0.5).round() as i64 + 1 }
fn quantize(p: [f64; 2]) -> [i64; 2] { p.map(|v| odd(v.clamp(-1e12, 1e12) * UNITS)) }

struct FillBins { pieces: Vec<([i32; 2], [u32; 2])>, crossings: Vec<(i32, i64, i32)> }
fn clip_edge(bins: &mut FillBins, a: [i64; 2], b: [i64; 2], clip: [i32; 4]) {
    let [x0, y0] = a.map(|v| v as f64);
    let [dx, dy] = [(b[0] - a[0]) as f64, (b[1] - a[1]) as f64];
    let (lo_y, hi_y) = (a[1].min(b[1]), a[1].max(b[1]));
    let (lo_x, hi_x) = (a[0].min(b[0]), a[0].max(b[0]));
    let [cx0, cy0, cx1, cy1] = clip.map(|v| i64::from(v) * SPAN);
    if hi_y <= cy0 || lo_y >= cy1 { return; }
    let mut events: Vec<(f64, [i64; 2])> = Vec::new();
    let first_row = lo_y.div_euclid(SPAN) + 1;
    let last_row = (hi_y - 1).div_euclid(SPAN);
    for row in first_row.max(clip[1] as i64)..=last_row.min(clip[3] as i64) {
        let y = row * SPAN;
        let t = (y as f64 - y0) / dy;
        let x = odd(x0 + t * dx);
        bins.crossings.push((row as i32, x, if dy < 0. { 1 } else { -1 }));
        events.push((t, [x, y]));
    }
    if hi_x > cx0 && lo_x < cx1 {
        let first = lo_x.div_euclid(SPAN) + 1;
        let last = (hi_x - 1).div_euclid(SPAN);
        for column in first.max(clip[0] as i64)..=last.min(clip[2] as i64) {
            let x = column * SPAN;
            let t = (x as f64 - x0) / dx;
            events.push((t, [x, odd(y0 + t * dy)]));
        }
    }
    events.sort_by(|p, q| p.0.total_cmp(&q.0));
    let mut previous = a;
    for point in events.into_iter().map(|(_, p)| p).chain(std::iter::once(b)) {
        let mid = [(previous[0] + point[0]).div_euclid(2), (previous[1] + point[1]).div_euclid(2)];
        let tile = mid.map(|v| v.div_euclid(SPAN) as i32);
        if (clip[0]..clip[2]).contains(&tile[0]) && (clip[1]..clip[3]).contains(&tile[1]) {
            let origin = tile.map(|v| i64::from(v) * SPAN);
            let local = |p: [i64; 2]| [(p[0] - origin[0]).clamp(0, SPAN) as u32, (p[1] - origin[1]).clamp(0, SPAN) as u32];
            let [s, e] = [local(previous), local(point)];
            bins.pieces.push((tile, [s[0] | s[1] << 16, e[0] | e[1] << 16]));
        }
        previous = point;
    }
}

fn emit_fill(out: &mut LevelGeometry, entry: u16, rule: FillRule, mut bins: FillBins, columns: [i32; 2]) {
    bins.pieces.sort_by_key(|(tile, _)| (tile[1], tile[0]));
    bins.crossings.sort_unstable();
    let rows: std::collections::BTreeSet<i32> = bins.pieces.iter().map(|(t, _)| t[1]).chain(bins.crossings.iter().map(|c| c.0)).collect();
    let (mut piece, mut crossing) = (0usize, 0usize);
    let kind = if rule == FillRule::EvenOdd { EVEN_ODD } else { NONZERO };
    for row in rows {
        while crossing < bins.crossings.len() && bins.crossings[crossing].0 < row { crossing += 1; }
        while piece < bins.pieces.len() && bins.pieces[piece].0[1] < row { piece += 1; }
        let mut winding = 0;
        for column in columns[0]..columns[1] {
            let left = i64::from(column) * SPAN;
            while crossing < bins.crossings.len() && bins.crossings[crossing].0 == row && bins.crossings[crossing].1 < left {
                winding += bins.crossings[crossing].2;
                crossing += 1;
            }
            let start = out.words.len() as u32;
            while piece < bins.pieces.len() && bins.pieces[piece].0 == [column, row] { out.words.extend_from_slice(&bins.pieces[piece].1); piece += 1; }
            let count = (out.words.len() as u32 - start) / 2;
            let inside = if rule == FillRule::EvenOdd { winding % 2 != 0 } else { winding != 0 };
            if count > 0 { out.tiles.push(ItemTile { tile: [column, row], entry, kind, backdrop: winding, start, count }); }
            else if inside { out.tiles.push(ItemTile { tile: [column, row], entry, kind: SOLID, backdrop: winding, start, count: 0 }); }
        }
        while crossing < bins.crossings.len() && bins.crossings[crossing].0 == row { crossing += 1; }
    }
}

fn fill(out: &mut LevelGeometry, entry: u16, path: &PathItem, frame: &Frame) {
    let lines = polylines(path, frame);
    let mut bins = FillBins { pieces: Vec::new(), crossings: Vec::new() };
    let mut bounds = [f64::INFINITY, f64::NEG_INFINITY];
    for line in &lines {
        let points: Vec<[i64; 2]> = line.iter().map(|p| quantize(*p)).collect();
        for p in line { bounds = [bounds[0].min(p[0]), bounds[1].max(p[0])]; }
        for i in 0..points.len() {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            if a != b { clip_edge(&mut bins, a, b, frame.clip); }
        }
    }
    if bounds[0] > bounds[1] { return; }
    let columns = [((bounds[0] / f64::from(TILE)).floor() as i32).max(frame.clip[0]), ((bounds[1] / f64::from(TILE)).floor() as i32 + 1).min(frame.clip[2])];
    if columns[0] < columns[1] { emit_fill(out, entry, path.fill_rule, bins, columns); }
}

fn stroke(out: &mut LevelGeometry, entry: u16, path: &PathItem, stroke: &Stroke, frame: &Frame) {
    if stroke.join != Join::Round || stroke.cap != Cap::Round {
        let outline = super::outline::outline(path, stroke, frame);
        let mut bins = FillBins { pieces: Vec::new(), crossings: Vec::new() };
        let mut bounds = [f64::INFINITY, f64::NEG_INFINITY];
        for polygon in &outline {
            let points: Vec<[i64; 2]> = polygon.iter().map(|p| quantize(*p)).collect();
            for p in polygon { bounds = [bounds[0].min(p[0]), bounds[1].max(p[0])]; }
            for i in 0..points.len() {
                let (a, b) = (points[i], points[(i + 1) % points.len()]);
                if a != b { clip_edge(&mut bins, a, b, frame.clip); }
            }
        }
        if bounds[0] > bounds[1] { return; }
        let columns = [((bounds[0] / f64::from(TILE)).floor() as i32).max(frame.clip[0]), ((bounds[1] / f64::from(TILE)).floor() as i32 + 1).min(frame.clip[2])];
        if columns[0] < columns[1] { emit_fill(out, entry, FillRule::NonZero, bins, columns); }
        return;
    }
    let half = stroke.width * 0.5 * frame.scale;
    let mut touches: Vec<([i32; 2], u32)> = Vec::new();
    let mut solids: Vec<[i32; 2]> = Vec::new();
    let base = out.words.len() as u32;
    let mut point = 0u32;
    let mut segment_base = 0u32;
    for subpath in path.geometry.subpaths() {
        let centerline = centerline(subpath, segment_base, stroke, half, frame);
        segment_base += subpath.segment_count() as u32;
        let first = point;
        for (i, (p, [left, right])) in centerline.iter().enumerate() {
            let before = centerline[i.saturating_sub(1)].0;
            let after = centerline[(i + 1).min(centerline.len() - 1)].0;
            let d = [after[0] - before[0], after[1] - before[1]];
            let length = (d[0] * d[0] + d[1] * d[1]).sqrt();
            let normal = if length > 0. { [d[1] / length, -d[0] / length] } else { [0.; 2] };
            let shift = (left - right) * 0.5;
            let center = [p[0] + normal[0] * shift, p[1] + normal[1] * shift];
            out.words.extend_from_slice(&[(center[0] as f32).to_bits(), (center[1] as f32).to_bits(), (((left + right) * 0.5) as f32).to_bits()]);
            point += 1;
        }
        if centerline.len() == 1 {
            out.words.extend_from_slice(&out.words[out.words.len() - 3..].to_vec());
            point += 1;
        }
        for capsule in first..point - 1 { capsule_tiles(&out.words[(base + capsule * 3) as usize..], capsule, frame.clip, &mut touches, &mut solids); }
    }
    solids.sort_unstable_by_key(|t| (t[1], t[0]));
    solids.dedup();
    touches.sort_unstable_by_key(|(t, c)| (t[1], t[0], *c));
    let mut index = 0;
    while index < touches.len() {
        let tile = touches[index].0;
        let mut runs: Vec<(u32, u32)> = Vec::new();
        while index < touches.len() && touches[index].0 == tile {
            let capsule = touches[index].1;
            match runs.last_mut() { Some((start, count)) if *start + *count == capsule => *count += 1, _ => runs.push((capsule, 1)) }
            index += 1;
        }
        if solids.binary_search_by_key(&(tile[1], tile[0]), |t| (t[1], t[0])).is_ok() {
            out.tiles.push(ItemTile { tile, entry, kind: SOLID, backdrop: 0, start: 0, count: 0 });
            continue;
        }
        let last = runs.len() - 1;
        for (i, (start, count)) in runs.into_iter().enumerate() {
            out.tiles.push(ItemTile { tile, entry, kind: STROKE | if i < last { CONTINUE } else { 0 }, backdrop: 0, start: base + start * 3, count });
        }
    }
}

fn centerline(subpath: &Subpath, segment_base: u32, stroke: &Stroke, half: f64, frame: &Frame) -> Vec<([f64; 2], [f64; 2])> {
    let width = |segment: u32, t: f64| stroke.profile.as_ref().map_or([half; 2], |p| p.at(segment, t).map(|f| f * half));
    let mut points = vec![(frame.map(subpath.anchor(0).point), width(segment_base, 0.))];
    for (index, segment) in subpath.segments().enumerate() {
        let global = segment_base + index as u32;
        flatten(&segment, |p| frame.map(p), TOLERANCE, |p, t| points.push((p, width(global, t))));
    }
    points
}

pub(crate) fn capsule(words: &[u32]) -> [f64; 8] {
    let [ax, ay, ra, bx, by, rb] = std::array::from_fn(|i| f64::from(f32::from_bits(words[i])));
    let d = [bx - ax, by - ay];
    let length = (d[0] * d[0] + d[1] * d[1]).sqrt();
    let u = if length > 0. { [d[0] / length, d[1] / length] } else { [1., 0.] };
    [ax, ay, u[0], u[1], length, ra, rb, 0.]
}
fn capsule_tiles(words: &[u32], index: u32, clip: [i32; 4], touches: &mut Vec<([i32; 2], u32)>, solids: &mut Vec<[i32; 2]>) {
    let c = capsule(words);
    let (start, u, length) = ([c[0], c[1]], [c[2], c[3]], c[4]);
    let reach = c[5].max(c[6]).max(0.5) + 1.;
    let end = [start[0] + u[0] * length, start[1] + u[1] * length];
    let lo = [start[0].min(end[0]) - reach, start[1].min(end[1]) - reach];
    let hi = [start[0].max(end[0]) + reach, start[1].max(end[1]) + reach];
    let t0 = [((lo[0] / f64::from(TILE)).floor() as i32).max(clip[0]), ((lo[1] / f64::from(TILE)).floor() as i32).max(clip[1])];
    let t1 = [((hi[0] / f64::from(TILE)).floor() as i32 + 1).min(clip[2]), ((hi[1] / f64::from(TILE)).floor() as i32 + 1).min(clip[3])];
    let half_diagonal = f64::from(TILE) * std::f64::consts::FRAC_1_SQRT_2;
    let wide = c[5].min(c[6]) > half_diagonal;
    for y in t0[1]..t1[1] {
        for x in t0[0]..t1[0] {
            let origin = [f64::from(x * TILE), f64::from(y * TILE)];
            let center = [origin[0] + 0.5 * f64::from(TILE), origin[1] + 0.5 * f64::from(TILE)];
            if segment_distance(center, start, u, length) > reach + half_diagonal { continue; }
            touches.push(([x, y], index));
            if wide && covers_tile(&c, origin) { solids.push([x, y]); }
        }
    }
}
fn segment_distance(p: [f64; 2], a: [f64; 2], u: [f64; 2], length: f64) -> f64 {
    let d = [p[0] - a[0], p[1] - a[1]];
    let t = (d[0] * u[0] + d[1] * u[1]).clamp(0., length);
    ((d[0] - u[0] * t).powi(2) + (d[1] - u[1] * t).powi(2)).sqrt()
}
pub(crate) fn capsule_distance(p: [f64; 2], c: &[f64; 8]) -> f64 {
    let (a, u, length, ra, rb) = ([c[0], c[1]], [c[2], c[3]], c[4], c[5].max(0.5), c[6].max(0.5));
    let d = [p[0] - a[0], p[1] - a[1]];
    let x = d[0] * u[0] + d[1] * u[1];
    let y = (d[0] * u[1] - d[1] * u[0]).abs();
    if length <= 0. { return (x * x + y * y).sqrt() - ra; }
    let s = (ra - rb) / length;
    let cos = (1. - s * s).max(0.).sqrt();
    let k = x * cos - y * s;
    if k < 0. { (x * x + y * y).sqrt() - ra }
    else if k > length * cos { ((x - length).powi(2) + y * y).sqrt() - rb }
    else { x * s + y * cos - ra }
}
fn covers_tile(c: &[f64; 8], origin: [f64; 2]) -> bool {
    if c[5].min(c[6]) < 0.5 { return false; }
    let side = f64::from(TILE);
    [[0., 0.], [side, 0.], [0., side], [side, side]].iter().all(|o| capsule_distance([origin[0] + o[0], origin[1] + o[1]], c) <= -0.5)
}
