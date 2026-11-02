use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnchorRef { pub subpath: u32, pub anchor: u32 }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SegmentHit { pub subpath: u32, pub segment: u32, pub t: f64, pub distance: f64 }

fn offset(a: [f64; 2], b: [f64; 2]) -> [f64; 2] { [b[0] - a[0], b[1] - a[1]] }

pub(super) fn closest_segment(segment:Segment,point:[f64;2],from:f64,to:f64)->(f64,f64) {
    let distance=|t| {let p=segment.point(t);(p[0]-point[0]).hypot(p[1]-point[1])};
    let (mut lo,mut hi)=(from,to);
    for _ in 0..40 {
        let (a,b)=(lo+(hi-lo)/3.,hi-(hi-lo)/3.);
        if distance(a)<distance(b) {hi=b;}else{lo=a;}
    }
    let at=[from,(lo+hi)*0.5,to].into_iter().min_by(|a,b|distance(*a).total_cmp(&distance(*b))).unwrap();
    (at,distance(at))
}

impl PathItem {
    fn bases(&self) -> Vec<u32> {
        self.geometry.subpaths().iter().scan(0u32, |base, subpath| { let at = *base; *base += subpath.segment_count() as u32; Some(at) }).collect()
    }
    fn restyled_profiles(&self, remap: impl Fn(&WidthProfile) -> Option<WidthProfile>) -> Arc<Style> {
        if self.style.strokes().all(|stroke| stroke.profile.is_none()) { return self.style.clone(); }
        Arc::new(Style::new(self.style.entries.iter().map(|entry| match entry {
            PaintEntry::Stroke(stroke) => PaintEntry::Stroke(Stroke { profile: stroke.profile.as_deref().and_then(&remap).map(Arc::new), ..stroke.clone() }),
            other => other.clone(),
        }).collect()))
    }
    pub fn nearest_segment(&self, point: [f64; 2], tolerance: f64) -> Option<SegmentHit> {
        let mut best: Option<SegmentHit> = None;
        for (s, subpath) in self.geometry.subpaths().iter().enumerate() {
            for (index, segment) in subpath.segments().enumerate() {
                let (mut last, mut last_t) = (segment.from, 0.);
                segment.flatten(tolerance, |p, t| {
                    let d = offset(last, p);
                    let length = d[0] * d[0] + d[1] * d[1];
                    let u = if length > 0. { (((point[0] - last[0]) * d[0] + (point[1] - last[1]) * d[1]) / length).clamp(0., 1.) } else { 0. };
                    let distance = (last[0] + d[0] * u - point[0]).hypot(last[1] + d[1] * u - point[1]);
                    if best.is_none_or(|b| distance - tolerance <= b.distance) {
                        let (at,distance)=closest_segment(segment,point,last_t,t);
                        if best.is_none_or(|b| distance < b.distance) { best = Some(SegmentHit { subpath: s as u32, segment: index as u32, t: at, distance }); }
                    }
                    (last, last_t) = (p, t);
                });
            }
        }
        best
    }
    pub fn with_anchors(&self, changes: impl IntoIterator<Item = (AnchorRef, Anchor)>) -> Result<Self, &'static str> {
        let mut subpaths = self.geometry.subpaths().to_vec();
        for (at, anchor) in changes {
            let subpath = subpaths.get_mut(at.subpath as usize).ok_or("Unknown subpath")?;
            if at.anchor as usize >= subpath.len() { return Err("Unknown anchor"); }
            *subpath = subpath.with_anchor(at.anchor as usize, anchor);
        }
        let geometry = PathGeometry::new(subpaths)?;
        Ok(if geometry == *self.geometry { self.clone() } else { self.with_geometry(Arc::new(geometry)) })
    }
    pub fn with_anchor_inserted(&self, subpath: u32, segment: u32, t: f64) -> Result<Self, &'static str> {
        let source = self.geometry.subpaths().get(subpath as usize).ok_or("Unknown subpath")?;
        if segment as usize >= source.segment_count() || !(t > 0. && t < 1.) { return Err("Choose a point inside a segment"); }
        let (left, right) = source.segment(segment as usize).split(t);
        let mut anchors = source.to_vec();
        let (a, b) = (segment as usize, (segment as usize + 1) % anchors.len());
        anchors[a] = anchors[a].with_out_handle(offset(left.from, left.control[0]));
        anchors[b] = Anchor::new(anchors[b].point(), offset(right.to, right.control[1]), anchors[b].out_handle(), anchors[b].kind);
        anchors.insert(a + 1, Anchor::new(left.to, offset(left.to, left.control[1]), offset(left.to, right.control[0]), AnchorKind::SMOOTH));
        let mut subpaths = self.geometry.subpaths().to_vec();
        subpaths[subpath as usize] = Subpath::new(&anchors, source.closed);
        let split = self.bases()[subpath as usize] + segment;
        let style = self.restyled_profiles(|profile| {
            let [l, r] = profile.at(split, t);
            let mut knots: Vec<WidthKnot> = profile.knots().iter().map(|k| match k.segment().cmp(&split) {
                std::cmp::Ordering::Less => *k,
                std::cmp::Ordering::Greater => WidthKnot::new(k.segment() + 1, k.t(), k.left(), k.right()),
                std::cmp::Ordering::Equal if k.t() < t => WidthKnot::new(split, k.t() / t, k.left(), k.right()),
                std::cmp::Ordering::Equal => WidthKnot::new(split + 1, (k.t() - t) / (1. - t), k.left(), k.right()),
            }).collect();
            knots.extend([WidthKnot::new(split, 1., l, r), WidthKnot::new(split + 1, 0., l, r)]);
            knots.sort_by(|a, b| (a.segment(), a.t()).partial_cmp(&(b.segment(), b.t())).unwrap());
            knots.dedup_by(|a, b| (a.segment(), a.t()) == (b.segment(), b.t()));
            WidthProfile::new(knots).ok()
        });
        Ok(PathItem { style, ..self.with_geometry(Arc::new(PathGeometry::new(subpaths)?)) })
    }
    pub fn without_anchors(&self, removed: &BTreeSet<AnchorRef>) -> Result<Option<Self>, &'static str> {
        let bases = self.bases();
        let pieces: Vec<(&Subpath, u32, Vec<usize>, bool)> = self.geometry.subpaths().iter().enumerate().filter_map(|(s, subpath)| {
            let kept: Vec<usize> = (0..subpath.len()).filter(|i| !removed.contains(&AnchorRef { subpath: s as u32, anchor: *i as u32 })).collect();
            let whole = kept.len() == subpath.len();
            let closed = subpath.closed && (kept.len() > 2 || whole);
            (kept.len() >= 2 || whole).then(|| (subpath, bases[s], kept, closed))
        }).collect();
        if pieces.is_empty() { return Ok(None); }
        let geometry = PathGeometry::new(pieces.iter().map(|(subpath, _, kept, closed)| Subpath::new(&kept.iter().map(|&i| *subpath.anchor(i)).collect::<Vec<_>>(), *closed)).collect())?;
        let style = self.restyled_profiles(|profile| {
            let mut knots: Vec<WidthKnot> = Vec::new();
            let mut push = |knot: WidthKnot| if knots.last().is_none_or(|last| (last.segment(), last.t()) < (knot.segment(), knot.t())) { knots.push(knot); };
            let mut base = 0u32;
            for (subpath, origin, kept, closed) in &pieces {
                let n = subpath.len();
                let width = |k: usize| {
                    let k = k % n;
                    let (segment, t) = if subpath.closed || k + 1 < n || k == 0 { (origin + k as u32, 0.) } else { (origin + k as u32 - 1, 1.) };
                    profile.at(segment, t)
                };
                let count = if *closed { kept.len() } else { kept.len() - 1 };
                for i in 0..count {
                    let (from, to) = (kept[i], kept.get(i + 1).copied().unwrap_or(kept[0] + n));
                    let segment = base + i as u32;
                    let [l, r] = width(from);
                    push(WidthKnot::new(segment, 0., l, r));
                    for q in from..to {
                        for knot in profile.knots().iter().filter(|k| k.segment() == origin + (q % n) as u32 && (q > from || k.t() > 0.) && (q + 1 < to || k.t() < 1.)) {
                            push(WidthKnot::new(segment, ((q - from) as f64 + knot.t()) / (to - from) as f64, knot.left(), knot.right()));
                        }
                    }
                    let [l, r] = width(to);
                    push(WidthKnot::new(segment, 1., l, r));
                }
                base += count as u32;
            }
            if let (true, Some((_, origin, kept, _))) = (knots.is_empty(), pieces.first()) {
                let [l, r] = profile.at(origin + kept[0] as u32, 0.);
                knots.push(WidthKnot::new(0, 0., l, r));
            }
            WidthProfile::new(knots).ok()
        });
        Ok(Some(PathItem { style, ..self.with_geometry(Arc::new(geometry)) }))
    }
    pub fn with_anchor_kind_switched(&self, at: AnchorRef) -> Result<Self, &'static str> {
        let subpath = self.geometry.subpaths().get(at.subpath as usize).ok_or("Unknown subpath")?;
        let i = at.anchor as usize;
        let anchor = *subpath.anchors().nth(i).ok_or("Unknown anchor")?;
        let flat = anchor.in_handle() == [0.; 2] && anchor.out_handle() == [0.; 2];
        let switched = if anchor.kind == AnchorKind::CORNER && !flat {
            let (incoming, outgoing) = (anchor.in_handle(), anchor.out_handle());
            let (a, b) = (incoming[0].hypot(incoming[1]), outgoing[0].hypot(outgoing[1]));
            let direction = if b > 0. { [outgoing[0] / b, outgoing[1] / b] } else { [-incoming[0] / a, -incoming[1] / a] };
            Anchor::new(anchor.point(), [-direction[0] * a, -direction[1] * a], [direction[0] * b, direction[1] * b], AnchorKind::SMOOTH)
        } else if flat {
            let n = subpath.len();
            let neighbor = |step: isize| -> Option<[f64; 2]> {
                let j = i as isize + step;
                if subpath.closed { Some(subpath.anchor(j.rem_euclid(n as isize) as usize).point()) } else { (0..n as isize).contains(&j).then(|| subpath.anchor(j as usize).point()) }
            };
            let p = anchor.point();
            let (previous, next) = (neighbor(-1), neighbor(1));
            let direction = match (previous, next) { (Some(a), Some(b)) => offset(a, b), (Some(a), None) => offset(a, p), (None, Some(b)) => offset(p, b), (None, None) => return Ok(self.clone()) };
            let length = direction[0].hypot(direction[1]);
            if length == 0. { return Ok(self.clone()); }
            let unit = [direction[0] / length, direction[1] / length];
            let reach = |q: Option<[f64; 2]>| q.map_or(0., |q| offset(p, q)[0].hypot(offset(p, q)[1]) / 3.);
            Anchor::new(p, [-unit[0] * reach(previous), -unit[1] * reach(previous)], [unit[0] * reach(next), unit[1] * reach(next)], AnchorKind::SMOOTH)
        } else { Anchor::corner(anchor.point()) };
        self.with_anchors([(at, switched)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve() -> PathItem {
        let anchors = [Anchor::new([0., 0.], [0.; 2], [30., -40.], AnchorKind::CORNER), Anchor::new([100., 0.], [-30., -30.], [30., 30.], AnchorKind::SYMMETRIC), Anchor::new([200., 40.], [-20., 0.], [0.; 2], AnchorKind::CORNER)];
        let mut stroke = Stroke::new(Paint::color(crate::color::RgbColor::BLACK), 8.);
        stroke.profile = Some(Arc::new(WidthProfile::new(vec![WidthKnot::new(0, 0., 0.2, 0.2), WidthKnot::new(0, 0.5, 1., 1.), WidthKnot::new(1, 1., 0.4, 0.4)]).unwrap()));
        PathItem::new(ItemId(1), Arc::new(PathGeometry::new(vec![Subpath::new(&anchors, false)]).unwrap()), Arc::new(Style::new(vec![PaintEntry::Stroke(stroke)])))
    }
    fn sample(path: &PathItem, steps: u32) -> Vec<[f64; 2]> { path.geometry.segments().flat_map(|s| (0..=steps).map(move |k| s.point(f64::from(k) / f64::from(steps)))).collect() }

    #[test]
    fn anchors_are_inserted_at_the_pointer_on_straight_segments() {
        let geometry = PathGeometry::new(vec![Subpath::new(&[Anchor::corner([0., 0.]), Anchor::corner([100., 0.])], false)]).unwrap();
        let path = curve().with_geometry(Arc::new(geometry)).with_style(Arc::new(Style::default()));
        for x in [10., 25., 75., 90.] {
            let hit = path.nearest_segment([x, 0.], 0.05).unwrap();
            let inserted = path.with_anchor_inserted(hit.subpath, hit.segment, hit.t).unwrap();
            assert!((inserted.geometry.subpaths()[0].anchor(1).point()[0] - x).abs() < 1. / GRID);
        }
    }

    #[test]
    fn a_handled_corner_becomes_smooth_without_retracting_its_handles() {
        let original = curve();
        let switched = original.with_anchor_kind_switched(AnchorRef { subpath: 0, anchor: 0 }).unwrap();
        let anchor = switched.geometry.subpaths()[0].anchor(0);
        assert_eq!(anchor.kind, AnchorKind::SMOOTH);
        assert_eq!(anchor.out_handle(), original.geometry.subpaths()[0].anchor(0).out_handle());
    }

    #[test]
    fn inserting_an_anchor_keeps_the_curve_and_its_widths() {
        let path = curve();
        let hit = path.nearest_segment([50., -20.], 0.05).unwrap();
        let inserted = path.with_anchor_inserted(hit.subpath, hit.segment, hit.t).unwrap();
        assert_eq!(inserted.geometry.anchor_count(), 4);
        let before = sample(&path, 2000);
        for p in sample(&inserted, 20) { assert!(before.iter().any(|q| (q[0] - p[0]).hypot(q[1] - p[1]) < 0.2), "{p:?} left the curve"); }
        let (old, new) = (path.style.strokes().next().unwrap().profile.clone().unwrap(), inserted.style.strokes().next().unwrap().profile.clone().unwrap());
        assert!((new.at(0, 1.)[0] - old.at(hit.segment, hit.t)[0]).abs() < 1e-3 && (new.at(2, 1.)[0] - 0.4).abs() < 1e-3);
        inserted.validate().unwrap();
    }

    #[test]
    fn deleting_anchors_keeps_widths_at_the_rest_and_removes_empty_paths() {
        let path = curve();
        let shorter = path.without_anchors(&[AnchorRef { subpath: 0, anchor: 1 }].into()).unwrap().unwrap();
        assert_eq!(shorter.geometry.anchor_count(), 2);
        let profile = shorter.style.strokes().next().unwrap().profile.clone().unwrap();
        assert!((profile.at(0, 0.)[0] - 0.2).abs() < 1e-6 && (profile.at(0, 1.)[0] - 0.4).abs() < 1e-6);
        shorter.validate().unwrap();
        assert!(path.without_anchors(&[0, 1].map(|anchor| AnchorRef { subpath: 0, anchor }).into()).unwrap().is_none(), "one anchor left is no path");
    }

    #[test]
    fn deleting_anchors_keeps_the_widths_and_closing_of_untouched_parts() {
        let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4;
        let line: Vec<Anchor> = [[0., 0.], [100., 0.], [200., 40.], [300., 0.]].into_iter().map(Anchor::corner).collect();
        let lens = Subpath::new(&[Anchor::new([0., 100.], [0., 30.], [0., -30.], AnchorKind::SMOOTH), Anchor::new([60., 100.], [0., -30.], [0., 30.], AnchorKind::SMOOTH)], true);
        let mut stroke = Stroke::new(Paint::color(crate::color::RgbColor::BLACK), 8.);
        stroke.profile = Some(Arc::new(WidthProfile::new(vec![WidthKnot::new(0, 0., 0.2, 0.2), WidthKnot::new(0, 0.5, 1., 0.8), WidthKnot::new(1, 0.5, 0.3, 0.3), WidthKnot::new(2, 0.5, 0.9, 0.9), WidthKnot::new(3, 0.5, 0.6, 0.6)]).unwrap()));
        let path = PathItem::new(ItemId(1), Arc::new(PathGeometry::new(vec![Subpath::new(&line, false), lens]).unwrap()), Arc::new(Style::new(vec![PaintEntry::Stroke(stroke)])));
        let before = path.style.strokes().next().unwrap().profile.clone().unwrap();
        let shorter = path.without_anchors(&[AnchorRef { subpath: 0, anchor: 2 }].into()).unwrap().unwrap();
        shorter.validate().unwrap();
        assert!(shorter.geometry.subpaths()[1].closed, "an untouched closed part stays closed");
        let after = shorter.style.strokes().next().unwrap().profile.clone().unwrap();
        for t in [0.25, 0.5, 0.75] { assert!(near(after.at(0, t), before.at(0, t)), "an untouched segment keeps its widths at {t}"); }
        assert!(near(after.at(1, 0.25), before.at(1, 0.5)) && near(after.at(1, 0.75), before.at(2, 0.5)) && near(after.at(1, 1.), before.at(2, 1.)), "a joined segment keeps the widths along both parts");
        for t in [0., 0.5, 1.] { assert!(near(after.at(2, t), before.at(3, t)), "the closed part keeps its widths at {t}"); }
    }

    #[test]
    fn switching_kinds_pulls_handles_along_the_neighbours_or_retracts_them() {
        let path = curve();
        let corner = path.with_anchor_kind_switched(AnchorRef { subpath: 0, anchor: 1 }).unwrap();
        let anchor = corner.geometry.subpaths()[0].anchor(1);
        assert_eq!((anchor.kind, anchor.in_handle(), anchor.out_handle()), (AnchorKind::CORNER, [0.; 2], [0.; 2]));
        let smooth = corner.with_anchor_kind_switched(AnchorRef { subpath: 0, anchor: 1 }).unwrap();
        let anchor = smooth.geometry.subpaths()[0].anchor(1);
        assert_eq!(anchor.kind, AnchorKind::SMOOTH);
        let (i, o) = (anchor.in_handle(), anchor.out_handle());
        assert!((i[0] * o[1] - i[1] * o[0]).abs() < 1e-3 * i[0].hypot(i[1]) * o[0].hypot(o[1]) && i[0] < 0. && o[0] > 0., "handles are collinear and opposite");
    }
}
