use crate::input::{StrokeBuilder, to_stroke_point};
use crate::feedback::{PredictionState, PreviewTip, TipInput, finalized_count, tip_times};
use crate::{CanvasEngine, DabGenerator, InstantFeedbackConfig, PenEvent, PenPhase, PressureCurve, SampleFlags, ViewTransform};
use layer_core::authored::vector::{Anchor, AnchorKind, PathGeometry, Subpath, WidthKnot, WidthProfile};
use layer_core::{BrushSnapshot, RulerConstraint, StrokeId, StrokePoint};
use layer_render::{CanvasRenderer, Dab};
use std::collections::BTreeMap;

pub type Sample = ([f64; 2], f64);

pub struct VectorStroke {
    brush: BrushSnapshot,
    generator: DabGenerator,
    curve: PressureCurve,
    offset: [f32; 2],
    ruler: Option<RulerConstraint>,
    builder: StrokeBuilder,
    finalized: usize,
    estimates: BTreeMap<(u64, u64), (Vec<usize>, ViewTransform)>,
    tip: PreviewTip,
    feedback: InstantFeedbackConfig,
    surface: [f32; 6],
    dabs: Vec<Dab>,
    fit: LineFit,
    id: u64,
}
impl VectorStroke {
    fn new(id: u64, brush: BrushSnapshot, curve: PressureCurve, offset: [f64; 2], feedback: InstantFeedbackConfig, surface: [f32; 6], tolerance: f64) -> Self {
        let mut generator = DabGenerator::new(layer_core::color::RgbSpace::Srgb);
        generator.reset_for_stroke(StrokeId(id), &brush);
        let fit = LineFit::new(tolerance, (tolerance * 8.).max(f64::from(brush.diameter) * 0.5));
        Self { brush, generator, curve, offset: offset.map(|v| v as f32), ruler: None, builder: StrokeBuilder::default(), finalized: 0, estimates: BTreeMap::new(),
            tip: PreviewTip::default(), feedback, surface, dabs: Vec::new(), fit, id }
    }
    pub fn brush(&self) -> &BrushSnapshot { &self.brush }
    pub fn awaits_corrections(&self) -> bool { !self.estimates.is_empty() }
    pub fn corrects(&self, event: &PenEvent) -> bool {
        event.flags.contains(SampleFlags::CORRECTION) && self.estimates.contains_key(&(event.device_id, event.sequence))
    }
    fn push(&mut self, event: PenEvent, transform: ViewTransform) -> bool {
        let position = transform.map(event.surface_position);
        if !position.x.is_finite() || !position.y.is_finite() || self.builder.real_points().len() >= crate::canvas::MAX_CONTACT_POINTS { return false; }
        if event.phase == PenPhase::Down { self.builder.begin(event, transform, self.curve.clone()); } else { self.builder.push(event, transform, self.curve.clone()); }
        if event.flags.contains(SampleFlags::PREDICTED) { return !self.builder.predicted_points().is_empty(); }
        let Some(index) = self.builder.real_points().len().checked_sub(1) else { return false };
        if self.feedback.enabled { self.tip.prediction.observe(event); }
        if event.flags.contains(SampleFlags::ESTIMATED) {
            self.estimates.entry((event.device_id, event.sequence)).or_insert_with(|| (Vec::new(), transform)).0.push(index);
        }
        self.finalize();
        true
    }
    fn finalize(&mut self) {
        let real = self.builder.real_points();
        let count = if self.feedback.enabled { finalized_count(real, self.finalized) } else { real.len() };
        let ending = count == real.len() && self.builder.ending();
        self.generator.append_points(
            &real[self.finalized..count],
            &self.brush,
            ending,
            &mut self.dabs,
        );
        self.finalized = count;
    }
    fn correct(&mut self, event: PenEvent) -> bool {
        let key = (event.device_id, event.sequence);
        let Some((indices, transform)) = self.estimates.get(&key) else { return false };
        let corrected = to_stroke_point(event, *transform, self.curve.clone(), 0);
        let mut changed = false;
        for &index in indices {
            let point = StrokePoint { elapsed_micros: self.builder.real_points()[index].elapsed_micros, ..corrected };
            changed |= self.builder.real_points()[index] != point;
            self.builder.replace_real(index, point);
        }
        if !event.flags.contains(SampleFlags::ESTIMATED) { self.estimates.remove(&key); }
        if changed {
            self.tip.prediction = PredictionState::default();
            self.generator.reset_for_stroke(StrokeId(self.id), &self.brush);
            self.dabs.clear();
            self.finalized = 0;
            self.finalize();
            self.fit.extend(&samples(self.dabs.iter()));
        }
        changed
    }
    pub fn preview(&mut self, timestamp_ns: Option<u64>, presentation_timestamp_ns: Option<u64>) -> Vec<Sample> {
        let mut tail = Vec::new();
        if let Some(latest) = self.builder.real_points().last().copied() {
            let builder = &self.builder;
            let (now, requested) = tip_times(
                self.feedback,
                latest,
                |timestamp| {
                    builder
                        .elapsed_micros_at(timestamp)
                        .unwrap_or(latest.elapsed_micros)
                },
                timestamp_ns,
                presentation_timestamp_ns,
            );
            let real = builder.real_points();
            self.tip.build(
                &self.generator,
                TipInput {
                    real,
                    pending: &real[self.finalized..],
                    platform: builder.predicted_points(),
                    brush: &self.brush,
                    feedback: self.feedback,
                    transform: self.surface,
                    now,
                    requested,
                    ending: builder.ending().then_some(latest.elapsed_micros),
                },
                0,
                &mut tail,
            );
        }
        let mut shown = samples(self.dabs.iter());
        self.fit.advance(&shown, FRAME_WORK);
        extend_samples(&mut shown, &tail);
        shown
    }
    pub fn fitted(&self) -> Option<(PathGeometry, Option<WidthProfile>, f64)> { self.fit.fitted(&self.samples()) }
    pub fn samples(&self) -> Vec<Sample> {
        if self.brush.taper.end_distance_diameters > 0. && !self.builder.real_points().is_empty()
            && let Ok(mut stroke) = layer_core::Stroke::new(
                StrokeId(self.id),
                layer_core::SourceTarget::Paint(layer_core::authored::PaintHandle::from_index(0)),
                layer_core::StrokeTool::Brush, self.brush.clone(), self.builder.real_points().to_vec()) {
            let mut dabs = Vec::new();
            stroke.pen_up = self.builder.ending();
            DabGenerator::generate(&stroke, layer_core::color::RgbSpace::Srgb, &mut dabs);
            return samples(dabs.iter());
        }
        let mut tail = Vec::new();
        let mut generator = self.generator.clone();
        generator.append_points(
            &self.builder.real_points()[self.finalized..],
            &self.brush,
            self.builder.ending(),
            &mut tail,
        );
        generator.finish(&self.brush, &mut tail);
        samples(self.dabs.iter().chain(&tail))
    }
}

impl<B: CanvasRenderer> CanvasEngine<B> {
    pub fn begin_vector_stroke(&self, id: u64, event: PenEvent, offset: [f64; 2], tolerance: f64) -> VectorStroke {
        let mut stroke = VectorStroke::new(id, self.stroke_brush(&event), self.configured_pressure_curve().clone(), offset,
            self.instant_feedback(), self.view().document_to_surface, tolerance);
        self.push_vector_stroke(&mut stroke, event);
        stroke
    }
    pub fn push_vector_stroke(&self, stroke: &mut VectorStroke, event: PenEvent) -> bool {
        if event.flags.contains(SampleFlags::CORRECTION) { return stroke.correct(event); }
        if !matches!(event.phase, PenPhase::Down | PenPhase::Move | PenPhase::Up) { return false; }
        let mut transform = self.event_transform(&event);
        stroke.ruler = self.snap_to_ruler(&event, &mut transform, stroke.ruler);
        transform.surface_to_document[4] -= stroke.offset[0];
        transform.surface_to_document[5] -= stroke.offset[1];
        stroke.push(event, transform)
    }
}

pub(crate) fn samples<'a>(dabs: impl Iterator<Item = &'a Dab>) -> Vec<Sample> {
    let mut out: Vec<Sample> = Vec::new();
    extend_samples(&mut out, dabs);
    out
}
fn extend_samples<'a>(out: &mut Vec<Sample>, dabs: impl IntoIterator<Item = &'a Dab>) {
    for dab in dabs {
        let sample = ([f64::from(dab.center.x), f64::from(dab.center.y)], f64::from(dab.radii[0].max(0.)));
        if let Some(last) = out.last_mut() && distance(last.0, sample.0) < 1e-6 { *last = sample; continue; }
        out.push(sample);
    }
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] { [a[0] - b[0], a[1] - b[1]] }
fn add(a: [f64; 2], b: [f64; 2]) -> [f64; 2] { [a[0] + b[0], a[1] + b[1]] }
fn scale(a: [f64; 2], s: f64) -> [f64; 2] { [a[0] * s, a[1] * s] }
fn dot(a: [f64; 2], b: [f64; 2]) -> f64 { a[0] * b[0] + a[1] * b[1] }
fn distance(a: [f64; 2], b: [f64; 2]) -> f64 { let d = sub(a, b); dot(d, d).sqrt() }
fn unit(v: [f64; 2]) -> [f64; 2] { let l = dot(v, v).sqrt(); if l > 1e-12 { scale(v, 1. / l) } else { [0.; 2] } }

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase { Turn, Samples, Fit, Anchors, Done }

pub struct PolylineFit {
    points: Vec<[f64; 2]>,
    closed: bool,
    fit: LineFit,
    cursor: usize,
    first: usize,
    turn: f64,
    phase: Phase,
    anchors: Vec<Anchor>,
    result: Option<Subpath>,
}
impl PolylineFit {
    pub fn new(mut points: Vec<[f64; 2]>, tolerance: f64, closed: bool) -> Self {
        if closed && points.first() == points.last() { points.pop(); }
        let invalid = !tolerance.is_finite() || tolerance <= 0. || points.len() < if closed { 3 } else { 2 };
        Self { points, closed, fit: LineFit::new(tolerance, tolerance * 8.), cursor: 0, first: 0, turn: f64::INFINITY,
            phase: if invalid { Phase::Done } else { Phase::Turn }, anchors: Vec::new(), result: None }
    }
    pub fn advance(&mut self, mut quota: usize) -> bool {
        while quota > 0 {
            match self.phase {
                Phase::Turn => {
                    quota -= 1;
                    let n = self.points.len();
                    let i = self.cursor;
                    if !self.points[i].iter().all(|v| v.is_finite()) { self.phase = Phase::Done; return true; }
                    if self.closed {
                        let a = unit(sub(self.points[i], self.points[(i + n - 1) % n]));
                        let b = unit(sub(self.points[(i + 1) % n], self.points[i]));
                        let turn = dot(a, b);
                        if turn < self.turn { self.turn = turn; self.first = i; }
                    }
                    self.cursor += 1;
                    if self.cursor == n { self.phase = Phase::Samples; self.cursor = 0; }
                }
                Phase::Samples => {
                    quota -= 1;
                    let n = self.points.len();
                    let point = self.points[(self.first + self.cursor) % n];
                    self.fit.push_sample((point, 1.));
                    self.cursor += 1;
                    if self.cursor == n + usize::from(self.closed) {
                        if self.closed && let Some(last) = self.fit.kept.last_mut() { last.0 = self.points[self.first]; *self.fit.points.last_mut().unwrap() = last.0; }
                        if self.fit.kept.len() < 2 { self.phase = Phase::Done; return true; }
                        self.phase = Phase::Fit;
                    }
                }
                Phase::Fit => {
                    let (ready, spent) = self.fit.prepare(false, quota);
                    quota = quota.saturating_sub(spent);
                    if !ready { break; }
                    quota = quota.saturating_sub(self.fit.progress(false, quota));
                    if self.fit.pieces.last().is_none_or(|piece| piece.end + 1 != self.fit.kept.len()) { break; }
                    self.phase = Phase::Anchors; self.cursor = 0;
                }
                Phase::Anchors => {
                    quota -= 1;
                    self.fit.push_anchors(self.cursor, &mut self.anchors);
                    self.cursor += 1;
                    if self.cursor == self.fit.pieces.len() {
                        if self.closed {
                            let last = self.anchors.pop().unwrap();
                            let first = &mut self.anchors[0];
                            *first = Anchor::new(first.point(), last.in_handle(), first.out_handle(), first.kind);
                        }
                        if self.anchors.len() >= 2 { self.result = Some(Subpath::new(&self.anchors, self.closed)); }
                        self.phase = Phase::Done; return true;
                    }
                }
                Phase::Done => return true,
            }
        }
        self.phase == Phase::Done
    }
    pub fn take(&mut self) -> Option<Subpath> { self.result.take() }
}

pub fn fit_polyline(points: &[[f64; 2]], tolerance: f64, closed: bool) -> Option<Subpath> {
    let mut fit = PolylineFit::new(points.to_vec(), tolerance, closed);
    while !fit.advance(20_000) {}
    fit.take()
}

pub fn polyline(samples: &[Sample]) -> Option<(PathGeometry, Option<WidthProfile>, f64)> {
    let half = samples.iter().map(|s| s.1).fold(0., f64::max);
    if samples.is_empty() || half <= 0. { return None; }
    let anchors: Vec<Anchor> = samples.iter().map(|s| Anchor::corner(s.0)).collect();
    let geometry = PathGeometry::new(vec![Subpath::new(&anchors, false)]).ok()?;
    let segments = samples.len().saturating_sub(1).max(1);
    let knots = samples.iter().enumerate().map(|(i, s)| {
        let (segment, t) = if i < segments { (i as u32, 0.) } else { ((segments - 1) as u32, 1.) };
        WidthKnot::new(segment, t, s.1 / half, s.1 / half)
    }).collect();
    Some((geometry, WidthProfile::new(knots).ok(), half * 2.))
}

struct Segment { cubic: [[f64; 2]; 4], u: Vec<f64>, error: f64 }

fn chord_parameters(points: &[[f64; 2]]) -> Vec<f64> {
    let mut u = vec![0.; points.len()];
    for i in 1..points.len() { u[i] = u[i - 1] + distance(points[i], points[i - 1]); }
    let total = *u.last().unwrap();
    if total > 0. { for v in &mut u { *v /= total; } }
    u
}
fn bezier(c: &[[f64; 2]; 4], t: f64) -> [f64; 2] {
    let s = 1. - t;
    let [a, b, c2, d] = [s * s * s, 3. * s * s * t, 3. * s * t * t, t * t * t];
    std::array::from_fn(|i| a * c[0][i] + b * c[1][i] + c2 * c[2][i] + d * c[3][i])
}
fn solve<const N: usize>(mut a: [[f64; N]; N], mut b: [f64; N]) -> Option<[f64; N]> {
    let scale = a.iter().enumerate().map(|(i, row)| row[i].abs()).fold(0., f64::max);
    for column in 0..N {
        let pivot = (column..N).max_by(|&i, &j| a[i][column].abs().total_cmp(&a[j][column].abs()))?;
        if a[pivot][column].abs() <= scale * 1e-12 { return None; }
        a.swap(column, pivot);
        b.swap(column, pivot);
        let (pivot_row, pivot_value) = (a[column], b[column]);
        for (row, value) in a.iter_mut().zip(b.iter_mut()).skip(column + 1) {
            let f = row[column] / pivot_row[column];
            for (entry, pivot) in row.iter_mut().zip(pivot_row).skip(column) { *entry -= f * pivot; }
            *value -= f * pivot_value;
        }
    }
    let mut x = [0.; N];
    for row in (0..N).rev() { x[row] = (b[row] - (row + 1..N).map(|k| a[row][k] * x[k]).sum::<f64>()) / a[row][row]; }
    Some(x)
}
fn least_squares(points: &[[f64; 2]], u: &[f64], weights: &[f64], left: Option<[f64; 2]>, right: Option<[f64; 2]>) -> Option<[[f64; 2]; 4]> {
    let (first, last) = (points[0], points[points.len() - 1]);
    let n = 2 + usize::from(left.is_none()) + usize::from(right.is_none());
    let mut normal = [[0.; 4]; 4];
    let mut rhs = [0.; 4];
    for ((p, &t), &weight) in points.iter().zip(u).zip(weights) {
        let s = 1. - t;
        let (b1, b2) = (3. * s * s * t, 3. * s * t * t);
        let mut fixed = add(scale(first, s * s * s), scale(last, t * t * t));
        let mut columns = [[0.; 2]; 4];
        let mut k = 0;
        match left {
            Some(d) => { fixed = add(fixed, scale(first, b1)); columns[k] = scale(d, b1); k += 1; }
            None => { columns[k] = [b1, 0.]; columns[k + 1] = [0., b1]; k += 2; }
        }
        match right {
            Some(d) => { fixed = add(fixed, scale(last, b2)); columns[k] = scale(d, -b2); }
            None => { columns[k] = [b2, 0.]; columns[k + 1] = [0., b2]; }
        }
        let r = sub(*p, fixed);
        for i in 0..n {
            for j in 0..n { normal[i][j] += weight * dot(columns[i], columns[j]); }
            rhs[i] += weight * dot(columns[i], r);
        }
    }
    let x = match n {
        2 => solve([[normal[0][0], normal[0][1]], [normal[1][0], normal[1][1]]], [rhs[0], rhs[1]]).map(|x| [x[0], x[1], 0., 0.]),
        3 => solve(std::array::from_fn(|i| std::array::from_fn(|j| normal[i][j])), [rhs[0], rhs[1], rhs[2]]).map(|x| [x[0], x[1], x[2], 0.]),
        _ => solve(normal, rhs),
    }?;
    let (c1, k) = match left { Some(d) => { if x[0] <= 0. { return None; } (add(first, scale(d, x[0])), 1) } None => ([x[0], x[1]], 2) };
    let c2 = match right { Some(d) => { if x[k] <= 0. { return None; } sub(last, scale(d, x[k])) } None => [x[k], x[k + 1]] };
    let reach: f64 = points.windows(2).map(|w| distance(w[0], w[1])).sum::<f64>() * 2.;
    (distance(c1, first) <= reach && distance(c2, last) <= reach).then_some([first, c1, c2, last])
}
fn project(c: &[[f64; 2]; 4], points: &[[f64; 2]], u: &[f64]) -> Vec<f64> {
    let d1: [[f64; 2]; 3] = std::array::from_fn(|i| scale(sub(c[i + 1], c[i]), 3.));
    let d2: [[f64; 2]; 2] = std::array::from_fn(|i| scale(sub(d1[i + 1], d1[i]), 2.));
    let newton = |p: [f64; 2], t: f64| {
        let q = sub(bezier(c, t), p);
        let s = 1. - t;
        let q1 = add(add(scale(d1[0], s * s), scale(d1[1], 2. * s * t)), scale(d1[2], t * t));
        let q2 = add(scale(d2[0], s), scale(d2[1], t));
        let denominator = dot(q1, q1) + dot(q, q2);
        if denominator.abs() < 1e-12 { t } else { (t - dot(q, q1) / denominator).clamp(0., 1.) }
    };
    let mut next: Vec<f64> = points.iter().zip(u).map(|(p, t)| newton(*p, newton(*p, *t))).collect();
    (next[0], *next.last_mut().unwrap()) = (0., 1.);
    for i in 1..next.len() { next[i] = next[i].max(next[i - 1]); }
    next
}
fn chord_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let ab = sub(b, a);
    let length = dot(ab, ab);
    let t = if length > 0. { (dot(sub(p, a), ab) / length).clamp(0., 1.) } else { 0. };
    distance(p, add(a, scale(ab, t)))
}
fn bulge(c: &[[f64; 2]; 4], a: [f64; 2], b: [f64; 2], from: f64, to: f64) -> f64 {
    let along = unit(sub(b, a));
    let [q0, q1, q2, q3] = c.map(|p| along[0] * p[1] - along[1] * p[0]);
    let (linear, square, cube) = (3. * (q1 - q0), 3. * (q2 - 2. * q1 + q0), q3 - 3. * q2 + 3. * q1 - q0);
    let discriminant = square * square - 3. * cube * linear;
    let turns = if cube.abs() > 1e-12 && discriminant >= 0. {
        let root = discriminant.sqrt();
        [(-square - root) / (3. * cube), (-square + root) / (3. * cube)]
    } else if square.abs() > 1e-12 { [-linear / (2. * square); 2] } else { [(from + to) * 0.5; 2] };
    let mut turns = turns.into_iter().filter(|t| *t > from && *t < to).peekable();
    if turns.peek().is_none() { return 0.; }
    turns.map(|t| chord_distance(bezier(c, t), a, b)).fold(0., f64::max)
}
fn worst(points: &[[f64; 2]], u: &[f64], c: &[[f64; 2]; 4]) -> f64 {
    let sharp = |i: usize| i > 0 && i + 1 < points.len() && dot(unit(sub(points[i], points[i - 1])), unit(sub(points[i + 1], points[i]))) < 0.85;
    let vertices = (1..points.len() - 1).map(|i| distance(bezier(c, u[i]), points[i]));
    let chords = (0..points.len() - 1).map(|i| {
        let overshoot = if sharp(i) || sharp(i + 1) {
            [0.25, 0.75].map(|f| chord_distance(bezier(c, u[i] + (u[i + 1] - u[i]) * f), points[i], points[i + 1])).into_iter().fold(0., f64::max)
        } else { 0. };
        bulge(c, points[i], points[i + 1], u[i], u[i + 1]).max(overshoot)
    });
    vertices.chain(chords).fold(0., f64::max)
}
fn segment(points: &[[f64; 2]], left: Option<[f64; 2]>, right: Option<[f64; 2]>, tolerance: f64) -> Option<Segment> {
    let (first, last) = (points[0], points[points.len() - 1]);
    let mut u = chord_parameters(points);
    let weights = vec![1.; points.len()];
    let mut best: Option<Segment> = None;
    for iteration in 0..6 {
        let Some(cubic) = least_squares(points, &u, &weights, left, right) else { break };
        u = project(&cubic, points, &u);
        let error = worst(points, &u, &cubic);
        if best.as_ref().is_none_or(|b| error < b.error) { best = Some(Segment { cubic, u: u.clone(), error }); }
        if error <= tolerance || (iteration > 0 && error > tolerance * 4.) { break; }
    }
    if best.is_none() && points.len() <= 3 {
        let start = left.unwrap_or_else(|| unit(sub(points[1], first)));
        let end = right.unwrap_or_else(|| unit(sub(last, points[points.len() - 2])));
        let handled = |span: f64| [first, add(first, scale(start, span)), sub(last, scale(end, span)), last];
        let mut span = distance(first, last) / 3.;
        while span > tolerance * 1e-3 && worst(points, &u, &handled(span)) > tolerance { span *= 0.5; }
        let cubic = handled(span);
        return Some(Segment { error: worst(points, &u, &cubic), cubic, u });
    }
    best
}
fn start_direction(c: &[[f64; 2]; 4]) -> [f64; 2] {
    unit([sub(c[1], c[0]), sub(c[2], c[0]), sub(c[3], c[0])].into_iter().find(|v| dot(*v, *v) > 1e-18).unwrap_or([1., 0.]))
}
fn end_direction(c: &[[f64; 2]; 4]) -> [f64; 2] {
    unit([sub(c[3], c[2]), sub(c[3], c[1]), sub(c[3], c[0])].into_iter().find(|v| dot(*v, *v) > 1e-18).unwrap_or([1., 0.]))
}
fn refined(points: &[[f64; 2]], left: Option<[f64; 2]>, right: Option<[f64; 2]>, tolerance: f64) -> Option<Segment> {
    let mut found = segment(points, left, right, tolerance)?;
    if found.error <= tolerance || found.error > tolerance * 2. || points.len() <= 3 { return Some(found); }
    let (mut cubic, mut u) = (found.cubic, found.u.clone());
    let mut weights = vec![1.; points.len()];
    for _ in 0..12 {
        let errors: Vec<f64> = points.iter().zip(&u).map(|(p, t)| distance(bezier(&cubic, *t), *p)).collect();
        let bound = (weights.iter().zip(&errors).map(|(w, e)| w * e * e).sum::<f64>() / weights.iter().sum::<f64>()).sqrt();
        if bound > tolerance { break; }
        let total = weights.iter().zip(&errors).map(|(w, e)| w * e).sum::<f64>().max(1e-12);
        for (w, e) in weights.iter_mut().zip(&errors) { *w = (*w * e * points.len() as f64 / total).max(1e-4); }
        let Some(next) = least_squares(points, &u, &weights, left, right) else { break };
        cubic = next;
        u = project(&cubic, points, &u);
        let error = worst(points, &u, &cubic);
        if error < found.error { found = Segment { cubic, u: u.clone(), error }; }
        if found.error <= tolerance { break; }
    }
    Some(found)
}
fn fits(points: &[[f64; 2]], left: Option<[f64; 2]>, right: Option<[f64; 2]>, tolerance: f64) -> bool {
    refined(points, left, right, tolerance).is_some_and(|s| s.error <= tolerance)
}

const BACKS: [usize; 5] = [0, 1, 2, 4, 8];
const FRAME_WORK: usize = 512;
const BLENDS: [f64; 5] = [0., 0.25, 0.5, 0.75, 1.];

#[derive(Clone, Debug)]
struct Piece { start: usize, end: usize, cubic: [[f64; 2]; 4], u: Vec<f64>, next: [f64; 2], hint: usize, seen: usize }

#[derive(Clone, Copy, Debug)]
struct Search { start: usize, left: Option<[f64; 2]>, from: usize, good: Option<usize>, step: usize, high: Option<usize>, retry: bool }
impl Search {
    fn new(start: usize, from: usize, left: Option<[f64; 2]>, retry: bool) -> Self { Self { start, left, from, good: None, step: 1, high: None, retry } }
}

#[derive(Clone, Debug)]
struct Decision {
    first: usize, start: usize, left: Option<[f64; 2]>, longest: Search, reach: Option<usize>, free: Option<Search>, leaving: Option<usize>, candidate: usize,
    sides: Option<(usize, [f64; 2], [f64; 2])>, trial: Option<(usize, [f64; 2], Search)>, best: Option<(usize, usize, [f64; 2])>, seen: usize,
}

struct Run<'a> { points: &'a [[f64; 2]], tolerance: f64, seen: std::cell::Cell<usize>, work: std::cell::Cell<usize> }
impl Run<'_> {
    fn last(&self) -> usize { self.points.len() - 1 }
    fn span(&self, start: usize, end: usize) -> &[[f64; 2]] {
        self.seen.set(self.seen.get().max(end));
        self.work.set(self.work.get() + end - start + 1);
        &self.points[start..=end]
    }
    fn beyond(&self, index: usize) -> bool {
        let beyond = index > self.last();
        if beyond { self.seen.set(self.last()); }
        beyond
    }
    fn fits(&self, start: usize, end: usize, left: Option<[f64; 2]>, right: Option<[f64; 2]>) -> bool { fits(self.span(start, end), left, right, self.tolerance) }
    fn search(&self, search: &mut Search) -> Option<Option<usize>> {
        let (last, start, left) = (self.last(), search.start, search.left);
        match (search.good, search.high) {
            (None, _) => {
                let from = search.from.clamp(start + 1, last);
                if from == start + 1 || self.fits(start, from, left, None) { search.good = Some(from); }
                else if search.retry { search.from = start + 1 + (from - start - 1) / 2; }
                else { return Some(None); }
            }
            (Some(good), None) => {
                if good == last { return Some(Some(good)); }
                let end = (good + search.step).min(last);
                if self.fits(start, end, left, None) { (search.good, search.step) = (Some(end), search.step * 2); } else { search.high = Some(end); }
            }
            (Some(good), Some(high)) => {
                if high - good <= 1 { return Some(Some(good)); }
                let middle = (good + high) / 2;
                if self.fits(start, middle, left, None) { search.good = Some(middle); } else { search.high = Some(middle); }
            }
        }
        None
    }
    fn step(&self, decision: &mut Decision) -> Option<Piece> {
        let (last, start, left) = (self.last(), decision.start, decision.left);
        let Some(longest) = decision.reach else {
            decision.reach = self.search(&mut decision.longest).flatten();
            return None;
        };
        if longest < last {
            let Some(free) = decision.leaving else {
                let search = decision.free.get_or_insert(Search::new(longest, 2 * longest - start, None, true));
                decision.leaving = self.search(search).flatten();
                return None;
            };
            if let Some((end, tangent, search)) = decision.trial.as_mut() {
                if let Some(found) = self.search(search) {
                    if let Some(next) = found { decision.best = Some((next, *end, *tangent)); }
                    decision.trial = None;
                }
                return None;
            }
            while decision.candidate < BACKS.len() * BLENDS.len() {
                let (back, blend) = (decision.candidate / BLENDS.len(), BLENDS[decision.candidate % BLENDS.len()]);
                decision.candidate += 1;
                let Some(end) = longest.checked_sub(BACKS[back]).filter(|end| *end > start) else { continue };
                let (arriving, leaving) = match decision.sides {
                    Some((at, arriving, leaving)) if at == end => (arriving, leaving),
                    _ => {
                        let Some(incoming) = segment(self.span(start, end), left, None, self.tolerance) else {
                            decision.candidate = (back + 1) * BLENDS.len();
                            continue;
                        };
                        let arriving = end_direction(&incoming.cubic);
                        let leaving = segment(self.span(end, free.max(end + 2).min(last)), None, None, self.tolerance).map_or(arriving, |s| start_direction(&s.cubic));
                        decision.sides = Some((end, arriving, leaving));
                        (arriving, leaving)
                    }
                };
                let tangent = unit(add(scale(leaving, 1. - blend), scale(arriving, blend)));
                let floor = decision.best.map_or(end + 1, |(next, _, _)| next + 1);
                if !self.beyond(floor) && self.fits(start, end, left, Some(tangent)) { decision.trial = Some((end, tangent, Search::new(end, floor, Some(tangent), false))); }
                return None;
            }
        }
        let (end, right, hint) = decision.best.map_or((longest, None, longest + 1), |(next, end, tangent)| (end, Some(tangent), next));
        let found = refined(self.span(start, end), left, right, self.tolerance).unwrap();
        Some(Piece { start, end, next: right.unwrap_or_else(|| end_direction(&found.cubic)), hint, cubic: found.cubic, u: found.u, seen: self.seen.get() })
    }
}

fn tangent_at(points: &[[f64; 2]], from: usize, toward: isize, reach: f64) -> [f64; 2] {
    let mut j = from as isize;
    loop {
        let next = j + toward;
        if next < 0 || next as usize >= points.len() { break; }
        j = next;
        if distance(points[j as usize], points[from]) >= reach { break; }
    }
    unit(sub(points[j as usize], points[from]))
}
fn ahead(points: &[[f64; 2]], from: usize, reach: f64, open: bool) -> Option<([f64; 2], usize)> {
    let found = (from + 1..points.len()).find(|&j| distance(points[j], points[from]) >= reach);
    let j = if open { found? } else { found.unwrap_or(points.len() - 1) };
    Some((unit(sub(points[j], points[from])), j))
}

#[derive(Clone, Debug)]
struct LineFit {
    tolerance: f64, reach: f64, kept: Vec<Sample>, points: Vec<[f64; 2]>, group: usize, angles: Vec<(f64, usize)>, corners: Vec<usize>, examined: usize,
    pieces: Vec<Piece>, pending: Option<Decision>, wait: usize,
}
impl LineFit {
    fn new(tolerance: f64, reach: f64) -> Self {
        Self { tolerance, reach, kept: Vec::new(), points: Vec::new(), group: 0, angles: vec![(0., 0)], corners: vec![0], examined: 1, pieces: Vec::new(), pending: None, wait: 0 }
    }
    fn take(&mut self, samples: &[Sample]) {
        if self.group > 0 && self.group >= samples.len() { return self.extend(samples); }
        self.kept.pop();
        self.points.pop();
        for (i, sample) in samples.iter().enumerate().skip(self.group) {
            if self.push_sample(*sample) { self.group = i; }
        }
    }
    fn push_sample(&mut self, sample: Sample) -> bool {
        if let Some(last) = self.kept.last_mut() && distance(last.0, sample.0) < self.tolerance * 0.25 { last.1 = last.1.max(sample.1); return false; }
        self.kept.push(sample); self.points.push(sample.0); true
    }
    fn push_anchors(&self, i: usize, anchors: &mut Vec<Anchor>) {
        let [from, out, into, to] = self.pieces[i].cubic;
        match anchors.last_mut() {
            Some(anchor) => *anchor = anchor.with_out_handle(sub(out, from)),
            None => anchors.push(Anchor::new(from, [0.; 2], sub(out, from), AnchorKind::CORNER)),
        }
        let kind = if self.pieces.get(i + 1).is_none_or(|next| self.corners.contains(&next.start)) { AnchorKind::CORNER } else { AnchorKind::SMOOTH };
        anchors.push(Anchor::new(to, sub(into, to), [0.; 2], kind));
    }
    fn extend(&mut self, samples: &[Sample]) {
        let earlier = std::mem::take(&mut self.kept);
        self.points.clear();
        self.group = 0;
        self.take(samples);
        let same = earlier.iter().zip(&self.kept).take_while(|(a, b)| a.0 == b.0).count();
        if same < earlier.len() { self.rollback(same); }
    }
    fn rollback(&mut self, changed: usize) {
        let angled = self.angles.iter().position(|&(_, seen)| seen >= changed).unwrap_or(self.angles.len()).min(changed);
        self.angles.truncate(angled.max(1));
        self.examined = self.examined.min(self.angles.len().saturating_sub(1).max(1));
        let examined = self.examined;
        self.corners.retain(|&corner| corner < examined);
        let cutoff = changed.min(examined);
        let valid = self.pieces.iter().position(|piece| piece.seen >= cutoff).unwrap_or(self.pieces.len());
        self.pieces.truncate(valid);
        if self.pending.as_ref().is_some_and(|decision| decision.first + decision.seen >= cutoff) { self.pending = None; }
        self.wait = 0;
    }
    fn advance(&mut self, samples: &[Sample], budget: usize) {
        self.take(samples);
        self.progress(true, budget);
    }
    fn prepare(&mut self, open: bool, budget: usize) -> (bool, usize) {
        let points = &self.points;
        let visible = if open { points.len().saturating_sub(1) } else { points.len() };
        let mut spent = 0;
        while self.angles.len() < visible && spent < budget {
            spent += 1;
            let i = self.angles.len();
            if i + 1 == visible && !open { self.angles.push((0., i)); continue; }
            let Some((forward, seen)) = ahead(&points[..visible], i, self.reach, open) else { break };
            let backward = tangent_at(points, i, -1, self.reach);
            self.angles.push(((-dot(backward, forward)).clamp(-1., 1.).acos(), seen));
        }
        let limit = 55f64.to_radians();
        while self.examined + 1 < self.angles.len() && spent < budget {
            spent += 1;
            let i = self.examined;
            let angle = self.angles[i].0;
            let previous = *self.corners.last().unwrap();
            if angle > limit && angle >= self.angles[i - 1].0 && angle > self.angles[i + 1].0 && distance(points[i], points[previous]) > self.reach * 0.5 { self.corners.push(i); }
            self.examined += 1;
        }
        (self.angles.len() >= visible && self.examined + 1 >= self.angles.len(), spent)
    }
    fn progress(&mut self, open: bool, budget: usize) -> usize {
        self.prepare(open, usize::MAX);
        let points = std::mem::take(&mut self.points);
        let visible = if open { points.len().saturating_sub(1) } else { points.len() };
        let mut spent = 0;
        while spent < budget && (!open || points.len() >= self.wait) {
            let cursor = self.pieces.last().map_or(0, |piece| piece.end);
            let first = *self.corners.iter().rev().find(|&&corner| corner <= cursor).unwrap();
            let (last, closed) = match self.corners.iter().find(|&&corner| corner > cursor) {
                Some(&corner) => (corner, true),
                None if open => (self.examined - 1, false),
                None => (visible.saturating_sub(1), true),
            };
            if last <= cursor { break; }
            let mut decision = match self.pending.take() {
                Some(decision) if decision.first == first && decision.start + first == cursor => decision,
                _ => {
                    let (left, hint) = match self.pieces.last() { Some(piece) if piece.end > first => (Some(piece.next), piece.hint), _ => (None, cursor + 1) };
                    Decision { first, start: cursor - first, left, longest: Search::new(cursor - first, hint - first, left, true), reach: None, free: None, leaving: None,
                        candidate: 0, sides: None, trial: None, best: None, seen: 0 }
                }
            };
            let run = Run { points: &points[first..=last], tolerance: self.tolerance, seen: std::cell::Cell::new(decision.seen), work: std::cell::Cell::new(0) };
            let piece = loop {
                let piece = run.step(&mut decision);
                if piece.is_some() || (!closed && run.seen.get() >= run.last()) || spent + run.work.get() >= budget { break piece; }
            };
            spent += run.work.get();
            decision.seen = run.seen.get();
            if !closed && decision.seen >= run.last() {
                self.wait = points.len() + 4.max((points.len() - cursor) / 4);
                break;
            }
            let Some(piece) = piece else {
                self.pending = Some(decision);
                break;
            };
            self.pieces.push(Piece { start: piece.start + first, end: piece.end + first, hint: piece.hint + first, seen: piece.seen + first, ..piece });
        }
        self.points = points;
        spent
    }
    fn fitted(&self, samples: &[Sample]) -> Option<(PathGeometry, Option<WidthProfile>, f64)> {
        let half = samples.iter().map(|s| s.1).fold(0., f64::max);
        if samples.is_empty() || half <= 0. { return None; }
        let mut fit = self.clone();
        fit.extend(samples);
        let last = fit.kept.len() - 1;
        let end = samples[samples.len() - 1].0;
        if last > 0 && fit.kept[last].0 != end {
            fit.rollback(last);
            fit.kept[last].0 = end;
            fit.points[last] = end;
        }
        if last == 0 {
            let geometry = PathGeometry::new(vec![Subpath::new(&[Anchor::corner(fit.kept[0].0)], false)]).ok()?;
            return Some((geometry, None, half * 2.));
        }
        fit.progress(false, usize::MAX);
        let mut anchors: Vec<Anchor> = Vec::with_capacity(fit.pieces.len() + 1);
        let mut params = vec![(0, 0.); fit.kept.len()];
        for (i, piece) in fit.pieces.iter().enumerate() {
            fit.push_anchors(i, &mut anchors);
            for (k, t) in piece.u.iter().enumerate().skip(1) { params[piece.start + k] = (i, *t); }
        }
        let geometry = PathGeometry::new(vec![Subpath::new(&anchors, false)]).ok()?;
        let widths: Vec<f64> = fit.kept.iter().map(|s| s.1).collect();
        let (low, high) = widths.iter().fold((f64::INFINITY, 0f64), |(l, h), w| (l.min(*w), h.max(*w)));
        if high - low <= fit.tolerance { return Some((geometry, None, high * 2.)); }
        let values: Vec<[f64; 2]> = params.iter().zip(&widths).map(|((segment, t), w)| [*segment as f64 + t, *w]).collect();
        let knots = fewest_knots(&values, fit.tolerance).into_iter().map(|i| {
            let (segment, t) = params[i];
            WidthKnot::new(segment as u32, t.clamp(0., 1.), widths[i] / high, widths[i] / high)
        }).collect();
        Some((geometry, WidthProfile::new(knots).ok(), high * 2.))
    }
}

fn fewest_knots(values: &[[f64; 2]], tolerance: f64) -> Vec<usize> {
    let mut places: Vec<(usize, [f64; 2])> = Vec::with_capacity(values.len());
    for (i, value) in values.iter().enumerate() {
        match places.last_mut() {
            Some((index, place)) if value[0] <= place[0] => if value[1] > place[1] { (*index, place[1]) = (i, value[1]) },
            _ => places.push((i, *value)),
        }
    }
    let n = places.len();
    let (mut count, mut previous) = (vec![usize::MAX; n], vec![0; n]);
    count[0] = 0;
    for i in 0..n - 1 {
        let [x, y] = places[i].1;
        let (mut low, mut high) = (f64::NEG_INFINITY, f64::INFINITY);
        for j in i + 1..n {
            let [xj, yj] = places[j].1;
            let run = xj - x;
            if (low..=high).contains(&((yj - y) / run)) && count[i] + 1 < count[j] { (count[j], previous[j]) = (count[i] + 1, i); }
            low = low.max((yj - tolerance - y) / run);
            high = high.min((yj + tolerance - y) / run);
            if low > high { break; }
        }
    }
    let mut kept = vec![n - 1];
    while let Some(&j) = kept.last() && j > 0 { kept.push(previous[j]); }
    kept.into_iter().rev().map(|j| places[j].0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_vector_preview_freezes_without_samples_and_keeps_a_later_real_move() {
        for feedback in [false, true] {
            let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil);
            brush.diameter = 36.;
            brush.stabilization.strength = 0.;
            let mut stroke = VectorStroke::new(
                1,
                brush,
                PressureCurve::default(),
                [0.; 2],
                InstantFeedbackConfig {
                    enabled: feedback,
                    use_platform_prediction: false,
                    ..Default::default()
                },
                [1., 0., 0., 1., 0., 0.],
                0.5,
            );
            for i in 0..=13 {
                let mut pen = crate::test_support::event(
                    i + 1,
                    if i == 0 {
                        PenPhase::Down
                    } else {
                        PenPhase::Move
                    },
                    if i == 13 { 80.4 } else { 20. + i as f32 * 5. },
                );
                pen.timestamp_ns = 1_000_000_000 + i * 4_000_000;
                pen.pressure = 0.6;
                stroke.push(pen, ViewTransform::IDENTITY);
            }
            let frozen = stroke.preview(Some(1_152_000_000), Some(1_168_000_000));
            assert_eq!(frozen.last().unwrap().0[0], 80.);
            assert_eq!(
                stroke.preview(Some(3_000_000_000), Some(3_016_000_000)),
                frozen
            );
            stroke.brush.taper.end_distance_diameters = 2.;
            stroke.brush.taper.end_size = 0.;
            let mut pen = crate::test_support::event(15, PenPhase::Move, 240.);
            pen.timestamp_ns = 4_000_000_000;
            pen.pressure = 0.6;
            stroke.push(pen, ViewTransform::IDENTITY);
            assert!(stroke.samples().iter().any(|sample| sample.0[0] > 200.));
        }
    }

    fn fit_line(samples: &[Sample], tolerance: f64) -> Option<(PathGeometry, Option<WidthProfile>, f64)> {
        let half = samples.iter().map(|s| s.1).fold(0., f64::max);
        LineFit::new(tolerance, (tolerance * 8.).max(half)).fitted(samples)
    }
    fn sample_curve(n: usize, radius: impl Fn(f64) -> f64) -> Vec<Sample> {
        (0..n).map(|i| { let t = i as f64 / (n - 1) as f64; ([t * 300., 80. * (t * 6.).sin() + t * t * 40.], radius(t)) }).collect()
    }
    fn deviation(geometry: &PathGeometry, samples: &[Sample]) -> f64 {
        let curve: Vec<[f64; 2]> = geometry.segments().flat_map(|s| (0..=1024).map(move |i| s.point(i as f64 / 1024.))).collect();
        samples.iter().map(|(p, _)| curve.iter().map(|q| distance(*p, *q)).fold(f64::INFINITY, f64::min)).fold(0., f64::max)
    }
    fn hand(kind: usize) -> Vec<PenEvent> {
        let mut seed = 0x2545_f491_4f6c_dd1du64 + kind as u64;
        let mut noise = move || { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; (seed % 10_000) as f64 / 10_000. - 0.5 };
        let (seconds, path): (f64, Box<dyn Fn(f64) -> [f64; 2]>) = match kind {
            0 => (1.1, Box::new(|t: f64| [100. + 360. * t, 300. + 110. * (t * std::f64::consts::TAU).sin()])),
            1 => (1.3, Box::new(|t: f64| { let a = t * 5.9; [400. + 130. * a.cos() * (1. + 0.08 * t), 300. + 110. * a.sin()] })),
            2 => (0.14, Box::new(|t: f64| [200. + 160. * t, 200. + 30. * t + 6. * (t * 3.1).sin()])),
            _ => (1.2, Box::new(|t: f64| { let k = (t * 4.).min(3.9999); let up = k.floor() as i32 % 2 == 0; [100. + 80. * k, if up { 100. + 120. * k.fract() } else { 220. - 120. * k.fract() }] })),
        };
        let n = (seconds * 240.) as usize;
        (0..=n).map(|i| {
            let t = i as f64 / n as f64;
            let [x, y] = path(t);
            let pressure = ((t * std::f64::consts::PI).sin().powf(0.3) * 0.75 + 0.05 * noise()).clamp(0.02, 1.);
            PenEvent {
                device_id: 1, sequence: i as u64, timestamp_ns: (t * seconds * 1e9) as u64 + 1, view_revision: 0,
                surface_position: layer_core::Point { x: (x + 0.3 * noise()) as f32, y: (y + 0.3 * noise()) as f32 },
                pressure: pressure as f32, tilt_radians: [0.; 2], twist_radians: 0., distance: 0.,
                phase: if i == 0 { PenPhase::Down } else if i == n { PenPhase::Up } else { PenPhase::Move },
                tool: crate::ToolKind::Pen, flags: SampleFlags::PRIMARY,
            }
        }).collect()
    }
    fn drawn(events: &[PenEvent], strength: f32) -> VectorStroke {
        let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        brush.stabilization.strength = strength;
        let mut stroke = VectorStroke::new(1, brush, PressureCurve::default(), [0.; 2], InstantFeedbackConfig::default(), [1., 0., 0., 1., 0., 0.], 0.5);
        for event in events {
            if event.flags.contains(SampleFlags::CORRECTION) { stroke.correct(*event); } else { stroke.push(*event, ViewTransform::IDENTITY); }
        }
        stroke
    }
    fn width_error(geometry: &PathGeometry, profile: Option<&WidthProfile>, width: f64, samples: &[Sample]) -> f64 {
        let fitted: Vec<([f64; 2], f64)> = geometry.segments().enumerate()
            .flat_map(|(s, seg)| { let n = 4096; (0..=n).map(move |i| (s, i as f64 / n as f64, seg.point(i as f64 / n as f64))) })
            .map(|(s, t, p)| (p, profile.map_or(1., |profile| profile.at(s as u32, t)[0]) * width * 0.5)).collect();
        let ends = [samples[0].0, samples[samples.len() - 1].0];
        let widest = |points: &mut dyn Iterator<Item = ([f64; 2], f64)>, at: [f64; 2]| points.filter(|(p, _)| distance(*p, at) <= 1.).map(|(_, r)| r).fold(0., f64::max);
        let tips = ends.map(|at| (widest(&mut samples.iter().copied(), at) - widest(&mut fitted.iter().copied(), at)).abs());
        samples.iter().filter(|(p, _)| ends.iter().all(|end| distance(*p, *end) > 1.)).map(|(p, r)| {
            let nearest = fitted.iter().min_by(|a, b| distance(a.0, *p).total_cmp(&distance(b.0, *p))).unwrap();
            (nearest.1 - r).abs()
        }).chain(tips).fold(0., f64::max)
    }
    #[test]
    fn hand_drawn_lines_keep_few_anchors_within_tolerance() {
        let tolerance = 0.5;
        for (kind, most) in [(0, 5), (1, 4), (2, 2), (3, 11)] {
            for strength in [0., 0.06, 0.4] {
                let samples = drawn(&hand(kind), strength).samples();
                let (geometry, profile, width) = fit_line(&samples, tolerance).unwrap();
                let knots = profile.as_ref().map_or(0, |p| p.knots().len());
                let label = format!("stroke {kind}, smoothing {strength}: {} anchors, {knots} knots, error {:.3}, width error {:.3}", geometry.anchor_count(), deviation(&geometry, &samples), width_error(&geometry, profile.as_ref(), width, &samples));
                assert!(geometry.anchor_count() <= most, "{label}");
                assert!(knots <= 9, "{label}");
                assert!(deviation(&geometry, &samples) <= tolerance * 1.02, "{label}");
                assert!(width_error(&geometry, profile.as_ref(), width, &samples) <= tolerance * 1.2, "{label}");
            }
        }
    }
    #[test]
    fn fitting_while_drawing_matches_fitting_at_pen_up() {
        for kind in 0..4 {
            for (strength, late) in [(0., 0), (0.06, 0), (0.4, 0), (0.06, 12), (0.06, 80)] {
                let events = hand(kind);
                let estimated = SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::ESTIMATED.0);
                let mut stroke = drawn(&[], strength);
                for (i, event) in events.iter().enumerate() {
                    if late > 0 { stroke.push(PenEvent { pressure: event.pressure * 0.4, flags: estimated, ..*event }, ViewTransform::IDENTITY); } else { stroke.push(*event, ViewTransform::IDENTITY); }
                    if late > 0 && i >= late { stroke.correct(PenEvent { flags: SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::CORRECTION.0), ..events[i - late] }); }
                    if i % 2 == 1 { stroke.preview(Some(event.timestamp_ns), Some(event.timestamp_ns + 8_000_000)); }
                }
                for event in events.iter().skip(events.len().saturating_sub(late)).filter(|_| late > 0) { stroke.correct(PenEvent { flags: SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::CORRECTION.0), ..*event }); }
                let label = format!("stroke {kind}, smoothing {strength}, pressure {late} samples late: {} pieces before pen-up", stroke.fit.pieces.len());
                assert!(late > 12 || stroke.fit.pieces.len() + 2 >= stroke.fitted().unwrap().0.anchor_count() / 2, "{label}");
                assert_eq!(stroke.fitted(), LineFit::new(0.5, stroke.fit.reach).fitted(&stroke.samples()), "{label}");
            }
        }
    }
    #[test]
    fn recorded_lines_stay_within_tolerance_between_samples() {
        let records = crate::recording::read(std::fs::File::open(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/movink14.capystrokes")).unwrap()).unwrap();
        let mut contact: Vec<(PenEvent, ViewTransform)> = Vec::new();
        let mut checked = 0;
        for record in records {
            let crate::recording::Record::Raw { event, transform, .. } = record else { continue };
            if event.phase == PenPhase::Hover || event.flags.contains(SampleFlags::PREDICTED) { continue; }
            if event.phase == PenPhase::Down { contact.clear(); }
            contact.push((event, transform));
            if event.phase != PenPhase::Up || contact[0].0.phase != PenPhase::Down || contact.len() > 250 { continue; }
            let tolerance = 0.5 * f64::from(transform.surface_to_document[0]).hypot(f64::from(transform.surface_to_document[1]));
            let mut stroke = drawn(&[], 0.);
            stroke.fit = LineFit::new(tolerance, stroke.fit.reach.max(tolerance * 8.));
            for (event, transform) in &contact {
                if event.flags.contains(SampleFlags::CORRECTION) { stroke.correct(*event); } else { stroke.push(*event, *transform); }
            }
            let samples = stroke.samples();
            let (geometry, _, _) = stroke.fitted().unwrap();
            let curve: Vec<[f64; 2]> = geometry.segments().flat_map(|s| (0..=128).map(move |i| s.point(i as f64 / 128.))).collect();
            let away = curve.iter().map(|q| samples.windows(2).map(|w| chord_distance(*q, w[0].0, w[1].0)).fold(f64::INFINITY, f64::min)).fold(0., f64::max);
            let label = format!("recorded contact ending at sample {}: {} segments, {away:.3} from the drawn line, {:.3} to the farthest sample, tolerance {tolerance:.3}", event.sequence, geometry.segment_count(), deviation(&geometry, &samples));
            assert!(away <= tolerance * 1.02 && deviation(&geometry, &samples) <= tolerance * 1.25, "{label}");
            checked += 1;
        }
        assert!(checked > 40, "{checked}");
    }
    #[test]
    fn late_pressure_corrections_replay_the_line() {
        let events = hand(0);
        let estimated: Vec<PenEvent> = events.iter().map(|e| PenEvent { pressure: e.pressure * 0.3, flags: SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::ESTIMATED.0), ..*e }).collect();
        let mut corrections: Vec<PenEvent> = events.iter().map(|e| PenEvent { flags: SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::CORRECTION.0), ..*e }).collect();
        let half = corrections.len() / 2;
        let mut stroke = drawn(&estimated, 0.06);
        assert!(stroke.awaits_corrections());
        let unknown = PenEvent { sequence: 10_000, ..corrections[0] };
        assert!(!stroke.corrects(&unknown) && !stroke.correct(unknown));
        for event in corrections.drain(..half) { assert!(stroke.corrects(&event)); stroke.correct(event); }
        assert!(stroke.awaits_corrections());
        for event in corrections { stroke.correct(event); }
        assert!(!stroke.awaits_corrections());
        assert_eq!(stroke.samples(), drawn(&events, 0.06).samples());
        assert_ne!(drawn(&estimated, 0.06).samples(), drawn(&events, 0.06).samples());
    }
    #[test]
    fn fitted_lines_stay_within_tolerance_with_few_anchors() {
        let samples = sample_curve(400, |t| 2. + 3. * (t * std::f64::consts::PI).sin());
        let (geometry, profile, width) = fit_line(&samples, 0.25).unwrap();
        let error = deviation(&geometry, &samples);
        assert!(error < 0.26, "{error}");
        assert!(geometry.anchor_count() <= 8, "{}", geometry.anchor_count());
        assert!((width - 10.).abs() < 0.2);
        let profile = profile.unwrap();
        assert!(profile.knots().len() < 60);
        for (p, r) in samples.iter().step_by(23) {
            let nearest = geometry.segments().enumerate().flat_map(|(s, seg)| (0..=64).map(move |i| (s, i as f64 / 64., seg.point(i as f64 / 64.))))
                .min_by(|a, b| distance(a.2, *p).total_cmp(&distance(b.2, *p))).unwrap();
            let fitted = profile.at(nearest.0 as u32, nearest.1)[0] * width * 0.5;
            assert!((fitted - r).abs() < 0.6, "width {fitted} vs {r}");
        }
    }
    #[test]
    fn corners_stay_sharp_and_constant_widths_drop_the_profile() {
        let mut samples: Vec<Sample> = (0..100).map(|i| ([i as f64, 0.], 3.)).collect();
        samples.extend((1..100).map(|i| ([99., i as f64], 3.)));
        let (geometry, profile, _) = fit_line(&samples, 0.25).unwrap();
        assert!(profile.is_none());
        let corner = geometry.subpaths()[0].anchors().any(|a| a.kind == AnchorKind::CORNER && distance(a.point(), [99., 0.]) < 1.5);
        assert!(corner, "{:?}", geometry.subpaths()[0].to_vec());
        assert!(deviation(&geometry, &samples) < 0.5);
        let (dot, _, width) = fit_line(&[([5., 5.], 2.)], 0.25).unwrap();
        assert_eq!((dot.anchor_count(), width), (1, 4.));
    }
    fn coverage_disc(size: u32, radius: f64) -> layer_core::SelectionPixels {
        let stride = size.div_ceil(4) as usize;
        let mut words = vec![0; stride * size as usize];
        for y in 0..size { for x in 0..size {
            let d = (f64::from(x) + 0.5 - f64::from(size) * 0.5).hypot(f64::from(y) + 0.5 - f64::from(size) * 0.5);
            let value = ((radius + 0.5 - d).clamp(0., 1.) * 255.).round() as u32;
            words[y as usize * stride + (x / 4) as usize] |= value << (x % 4 * 8);
        } }
        layer_core::SelectionPixels::bytes([size; 2], [0, 0, size, size], words).unwrap()
    }
    fn traced(pixels: layer_core::SelectionPixels) -> Vec<Vec<[f64; 2]>> {
        let mut trace = layer_core::authored::vector::ContourTrace::new(pixels);
        loop { if let Some(contours) = trace.advance(262_144) { return contours; } }
    }
    #[test]
    fn fitted_contours_preserve_disc_accuracy_and_closed_handles() {
        let contours = traced(coverage_disc(128, 50.));
        assert_eq!(contours.len(), 1);
        let subpath = fit_polyline(&contours[0], 0.25, true).unwrap();
        assert!(subpath.closed);
        assert_ne!(subpath.anchor(0).point(), subpath.anchor(subpath.len() - 1).point());
        assert_ne!(subpath.anchor(0).in_handle(), [0.; 2]);
        let error = (0..subpath.segment_count()).flat_map(|i| (0..=256).map(move |j| (i, f64::from(j) / 256.)))
            .map(|(i, t)| { let p = subpath.segment(i).point(t); ((p[0] - 64.).hypot(p[1] - 64.) - 50.).abs() }).fold(0., f64::max);
        assert!(error <= 0.25, "circle error {error}");
        let path = layer_core::authored::vector::PathItem::new(layer_core::authored::vector::ItemId(1), std::sync::Arc::new(PathGeometry::new(vec![subpath]).unwrap()), std::sync::Arc::new(layer_core::authored::vector::Style::new(Vec::new())));
        let error = contours[0].iter().map(|p| path.nearest_segment(*p, 0.001).unwrap().distance).fold(0., f64::max);
        assert!(error <= 0.25, "iso-line error {error}");
        let error = path.geometry.segments().flat_map(|segment| (0..=256).map(move |i| segment.point(f64::from(i) / 256.)))
            .map(|p| contours[0].iter().zip(contours[0].iter().cycle().skip(1)).map(|(a, b)| chord_distance(p, *a, *b)).fold(f64::INFINITY, f64::min)).fold(0., f64::max);
        assert!(error <= 0.25, "curve to iso-line error {error}");
    }
    #[test]
    fn polyline_fit_quotas_keep_sharp_corners_and_the_same_geometry() {
        let mut points = Vec::new();
        for i in 0..100 { points.push([f64::from(i), 0.]); }
        for i in 0..100 { points.push([99., f64::from(i)]); }
        for i in 0..100 { points.push([99. - f64::from(i), 99.]); }
        for i in 0..100 { points.push([0., 99. - f64::from(i)]); }
        let expected = fit_polyline(&points, 0.25, true).unwrap();
        for quota in [1, 37, 20_000] {
            let mut fit = PolylineFit::new(points.clone(), 0.25, true);
            while !fit.advance(quota) {}
            assert_eq!(fit.take(), Some(expected.clone()));
        }
        for corner in [[0., 0.], [99., 0.], [99., 99.], [0., 99.]] {
            assert!(expected.anchors().any(|a| distance(a.point(), corner) <= 0.25));
        }
        assert!(fit_polyline(&[], 0.25, true).is_none());
        assert!(fit_polyline(&[[f64::NAN, 0.], [1., 1.]], 0.25, false).is_none());
    }
    #[test]
    fn trace_and_fit_large_region_budgeted() {
        let extent = [2000, 2000];
        let pixels = layer_core::SelectionPixels::new(extent, [0, 0, 2000, 2000], vec![0x4444_4444; 250 * 2000]).unwrap();
        let first_begin = std::time::Instant::now();
        let mut trace = Some(layer_core::authored::vector::ContourTrace::new(pixels));
        let mut first_begin = Some(first_begin);
        let mut calls = 0;
        let mut maximum = 0;
        let mut contours = loop {
            let begin = first_begin.take().unwrap_or_else(std::time::Instant::now);
            let result = trace.as_mut().unwrap().advance(262_144);
            let result = result.map(std::collections::VecDeque::from);
            if result.is_some() { trace = None; }
            let elapsed = begin.elapsed().as_micros();
            maximum = maximum.max(elapsed); calls += 1;
            eprintln!("trace_and_fit_large_region_budgeted phase=trace call={calls} quota=262144 elapsed_us={elapsed}");
            if let Some(contours) = result { break contours; }
        };
        assert_eq!(contours.len(), 1);
        let mut fit = None;
        let mut subpaths = Vec::new();
        let geometry = loop {
            let begin = std::time::Instant::now();
            let current = fit.get_or_insert_with(|| PolylineFit::new(contours.pop_front().unwrap(), 0.25, true));
            let done = current.advance(20_000);
            let mut geometry = None;
            if done {
                subpaths.push(current.take().unwrap());
                fit = None;
                if contours.is_empty() { geometry = Some(PathGeometry::new(std::mem::take(&mut subpaths)).unwrap()); }
            }
            let elapsed = begin.elapsed().as_micros();
            maximum = maximum.max(elapsed); calls += 1;
            eprintln!("trace_and_fit_large_region_budgeted phase=fit call={calls} quota=20000 elapsed_us={elapsed}");
            if let Some(geometry) = geometry { break geometry; }
        };
        assert_eq!(geometry.subpaths().len(), 1);
        assert!(geometry.subpaths()[0].closed);
        let iso = [[0., 0.5], [0.5, 0.], [1999.5, 0.], [2000., 0.5], [2000., 1999.5], [1999.5, 2000.], [0.5, 2000.], [0., 1999.5]];
        let error = geometry.segments().flat_map(|segment| (0..=1024).map(move |i| segment.point(f64::from(i) / 1024.)))
            .map(|p| iso.iter().zip(iso.iter().cycle().skip(1)).map(|(a, b)| chord_distance(p, *a, *b)).fold(f64::INFINITY, f64::min)).fold(0., f64::max);
        assert!(error <= 0.25, "square iso-line error {error}");
        eprintln!("trace_and_fit_large_region_budgeted complete calls={calls} maximum_us={maximum}");
    }

    #[test]
    fn small_closed_polylines_finish_within_one_available_quota() {
        let points = vec![[0., 0.], [3., 0.], [3., 3.], [0., 3.]];
        let expected = fit_polyline(&points, 0.25, true).unwrap();
        let mut fit = PolylineFit::new(points, 0.25, true);
        assert!(fit.advance(20_000));
        assert_eq!(fit.take(), Some(expected));
    }
    #[test]
    fn closed_polyline_retains_two_anchor_cubic_loops() {
        let points = (0..16).map(|i| { let angle = f64::from(i) * std::f64::consts::TAU / 16.; [8. + 3. * angle.cos(), 8. + 3. * angle.sin()] }).collect::<Vec<_>>();
        let subpath = fit_polyline(&points, 0.25, true).unwrap();
        assert!(subpath.closed);
        assert_eq!(subpath.len(), 2);
        let curve = subpath.segments().flat_map(|segment| (0..256).map(move |i| segment.point(f64::from(i) / 256.))).collect::<Vec<_>>();
        let area = curve.iter().zip(curve.iter().cycle().skip(1)).map(|(a, b)| a[0] * b[1] - b[0] * a[1]).sum::<f64>().abs() * 0.5;
        assert!(area >= 4.);
        let error = curve.iter().map(|p| points.iter().zip(points.iter().cycle().skip(1)).map(|(a, b)| chord_distance(*p, *a, *b)).fold(f64::INFINITY, f64::min)).fold(0., f64::max);
        assert!(error <= 0.25, "two-anchor curve error {error}");
        let path = layer_core::authored::vector::PathItem::new(layer_core::authored::vector::ItemId(1), std::sync::Arc::new(PathGeometry::new(vec![subpath]).unwrap()), std::sync::Arc::new(layer_core::authored::vector::Style::new(Vec::new())));
        assert!(points.iter().all(|p| path.nearest_segment(*p, 0.001).unwrap().distance <= 0.25));
    }

}
