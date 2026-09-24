//! Real-contact selection gestures share brush sampling with artwork. Loop
//! detection indexes centerline segments, never the expanded brush footprint.
use crate::input::to_stroke_point;
use crate::{DabGenerator, PenEvent, PenPhase, PressureCurve, SampleFlags, ViewTransform};
use layer_core::{BrushSnapshot, Point, Selection, StrokeId};
use layer_render::Dab;
use std::collections::{BTreeMap, BTreeSet};

pub struct SelectionStroke {
    id: u64,
    replay_points: Vec<layer_core::StrokePoint>,
    real_points: usize,
    brush: BrushSnapshot,
    generator: DabGenerator,
    transform: ViewTransform,
    curve: PressureCurve,
    start_ns: u64,
    loops: Option<Loops>,
}
impl SelectionStroke {
    pub fn new(
        id: u64,
        brush: BrushSnapshot,
        transform: ViewTransform,
        curve: PressureCurve,
        close_distance: Option<f32>,
    ) -> Self {
        let mut generator = DabGenerator::new(layer_core::color::RgbSpace::Srgb);
        generator.reset_for_stroke(StrokeId(id), &brush);
        let [a, b, c, d, _, _] = transform.surface_to_document;
        let snap = close_distance.unwrap_or(0.) * a.hypot(b).max(c.hypot(d));
        Self {
            id,
            replay_points: Vec::new(),
            real_points: 0,
            brush,
            generator,
            transform,
            curve,
            start_ns: 0,
            loops: close_distance.map(|_| Loops::new(snap)),
        }
    }
    pub fn input_budget_exhausted(&self) -> bool {
        self.real_points >= crate::canvas::MAX_CONTACT_POINTS
    }
    pub fn style(&self) -> layer_render::DabStyle {
        crate::canvas::style_for(&self.brush, layer_core::StrokeTool::Brush)
    }
    /// Predicted samples and late correction deliveries cannot close a loop.
    pub fn push(&mut self, event: PenEvent, dabs: &mut Vec<Dab>) -> Vec<Selection> {
        if event.flags.contains(SampleFlags::PREDICTED)
            || event.flags.contains(SampleFlags::CORRECTION)
            || !matches!(event.phase, PenPhase::Down | PenPhase::Move | PenPhase::Up)
        {
            return Vec::new();
        }
        if event.phase == PenPhase::Down {
            self.start_ns = event.timestamp_ns;
        }
        let point = to_stroke_point(event, self.transform, self.curve, self.start_ns);
        if !point.position.x.is_finite() || !point.position.y.is_finite() {
            return Vec::new();
        }
        if self.brush.taper.end_distance_diameters > 0. {
            self.replay_points.push(point);
        }
        self.real_points += 1;
        self.generator.append(point, &self.brush, dabs);
        // Masks accumulate real contacts without artwork's disposable tail
        // preview. Publish the swept endpoint on every sample so large nibs
        // do not wait half a diameter before their coverage refreshes.
        self.generator.finish(&self.brush, dabs);
        self.loops
            .as_mut()
            .map_or_else(Vec::new, |loops| loops.push(point.position))
    }
    /// End taper depends on completed path length. The caller replaces the
    /// provisional footprint, retaining the same immutable starting coverage.
    pub fn finished_replay(&mut self) -> Option<Vec<Dab>> {
        if self.replay_points.is_empty() {
            return None;
        }
        let stroke = layer_core::Stroke::new(
            StrokeId(self.id),
            layer_core::LayerId(0),
            layer_core::StrokeTool::Brush,
            self.brush.clone(),
            std::mem::take(&mut self.replay_points),
        )
        .ok()?;
        let mut dabs = Vec::new();
        let mut generator = DabGenerator::new(layer_core::color::RgbSpace::Srgb);
        generator.reset_for_replay(&stroke);
        for point in stroke.points.iter().copied() {
            generator.append(point, &stroke.brush, &mut dabs);
            generator.finish(&stroke.brush, &mut dabs);
        }
        Some(dabs)
    }
    pub fn cursor(&self, event: PenEvent) -> Vec<Dab> {
        let mut point = to_stroke_point(event, self.transform, self.curve, self.start_ns);
        if event.phase == PenPhase::Hover {
            point.pressure = 1.;
        }
        self.generator.clone().cursor_contacts(point, &self.brush)
    }
}

struct Loops {
    points: Vec<Point>,
    grid: BTreeMap<[i32; 2], Vec<usize>>,
    large: Vec<usize>,
    snap: f32,
    left_start: bool,
}
impl Loops {
    fn new(snap: f32) -> Self {
        Self {
            points: Vec::new(),
            grid: BTreeMap::new(),
            large: Vec::new(),
            snap,
            left_start: false,
        }
    }
    fn cells(a: Point, b: Point) -> Option<Vec<[i32; 2]>> {
        let lo = [
            (a.x.min(b.x) / 64.).floor() as i32,
            (a.y.min(b.y) / 64.).floor() as i32,
        ];
        let hi = [
            (a.x.max(b.x) / 64.).floor() as i32,
            (a.y.max(b.y) / 64.).floor() as i32,
        ];
        if (i64::from(hi[0]) - i64::from(lo[0]) + 1)
            .saturating_mul(i64::from(hi[1]) - i64::from(lo[1]) + 1)
            > 4096
        {
            return None;
        }
        Some(
            (lo[0]..=hi[0])
                .flat_map(|x| (lo[1]..=hi[1]).map(move |y| [x, y]))
                .collect(),
        )
    }
    fn index(&mut self, i: usize) {
        if let Some(cells) = Self::cells(self.points[i], self.points[i + 1]) {
            for cell in cells {
                self.grid.entry(cell).or_default().push(i);
            }
        } else {
            self.large.push(i);
        }
    }
    fn rebuild(&mut self) {
        self.grid.clear();
        self.large.clear();
        for i in 0..self.points.len().saturating_sub(1) {
            self.index(i);
        }
    }
    fn push(&mut self, end: Point) -> Vec<Selection> {
        let Some(mut start) = self.points.last().copied() else {
            self.points.push(end);
            return Vec::new();
        };
        if distance(start, end) < 0.001 {
            return Vec::new();
        }
        let mut areas = Vec::new();
        // Each crossing removes a closed section, leaving the open tail. Bound
        // retained input independently of zoom and pathological pen streams.
        while self.points.len() >= 3 {
            let candidates: BTreeSet<usize> = if let Some(cells) = Self::cells(start, end) {
                cells
                    .iter()
                    .filter_map(|cell| self.grid.get(cell))
                    .flatten()
                    .copied()
                    .chain(self.large.iter().copied())
                    .collect()
            } else {
                (0..self.points.len() - 2).collect()
            };
            let hit = candidates
                .into_iter()
                .filter(|i| *i + 2 < self.points.len())
                .filter_map(|i| {
                    crossing(start, end, self.points[i], self.points[i + 1]).map(|(t, p)| (t, i, p))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let Some((_, i, p)) = hit else {
                break;
            };
            let mut polygon = vec![p];
            polygon.extend_from_slice(&self.points[i + 1..]);
            if let Some(area) = polygon_selection(polygon) {
                areas.push(area);
            }
            self.points.truncate(i + 1);
            self.points.push(p);
            self.rebuild();
            start = p;
        }
        let first = self.points[0];
        if distance(first, end) > self.snap * 2. {
            self.left_start = true;
        }
        if self.left_start && self.points.len() >= 3 && distance(first, end) <= self.snap {
            if let Some(area) = polygon_selection(self.points.clone()) {
                areas.push(area);
            }
            self.points.clear();
            self.grid.clear();
            self.large.clear();
            self.points.push(end);
            self.left_start = false;
        } else if self.points.len() < 131072 {
            self.points.push(end);
            self.index(self.points.len() - 2);
        }
        areas
    }
}
fn distance(a: Point, b: Point) -> f32 {
    (a.x - b.x).hypot(a.y - b.y)
}
fn crossing(a: Point, b: Point, c: Point, d: Point) -> Option<(f64, Point)> {
    let cross = |x: [f64; 2], y: [f64; 2]| x[0] * y[1] - x[1] * y[0];
    let r = [
        f64::from(b.x) - f64::from(a.x),
        f64::from(b.y) - f64::from(a.y),
    ];
    let s = [
        f64::from(d.x) - f64::from(c.x),
        f64::from(d.y) - f64::from(c.y),
    ];
    let q = [
        f64::from(c.x) - f64::from(a.x),
        f64::from(c.y) - f64::from(a.y),
    ];
    let denominator = cross(r, s);
    if denominator.abs() < 1e-9 {
        return None;
    }
    let t = cross(q, s) / denominator;
    let u = cross(q, r) / denominator;
    (t > 1e-6 && t <= 1. && (0. ..=1.).contains(&u)).then(|| {
        (
            t,
            Point {
                x: (f64::from(a.x) + t * r[0]) as f32,
                y: (f64::from(a.y) + t * r[1]) as f32,
            },
        )
    })
}
fn polygon_selection(points: Vec<Point>) -> Option<Selection> {
    if points.len() < 3 {
        return None;
    }
    let area: f64 = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| f64::from(a.x) * f64::from(b.y) - f64::from(b.x) * f64::from(a.y))
        .sum();
    (area.abs() > 0.0001)
        .then(|| Selection::polygon(points).ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn large_gpen_publishes_every_real_endpoint_and_replays_the_same_path() {
        let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        brush.diameter = 800.;
        brush.taper.end_distance_diameters = 1.;
        brush.taper.end_size = 0.;
        let mut stroke = SelectionStroke::new(
            1,
            brush,
            ViewTransform::IDENTITY,
            PressureCurve::default(),
            None,
        );
        let mut live = Vec::new();
        for i in 0..=10 {
            let before = live.len();
            let event = PenEvent {
                device_id: 1,
                sequence: i,
                timestamp_ns: i * 8_000_000,
                view_revision: 0,
                surface_position: p(100. + i as f32 * 2., 100.),
                pressure: 0.7,
                tilt_radians: [0.; 2],
                twist_radians: 0.,
                distance: 0.,
                phase: if i == 0 {
                    PenPhase::Down
                } else {
                    PenPhase::Move
                },
                tool: crate::ToolKind::Pen,
                flags: SampleFlags::PRIMARY,
            };
            stroke.push(event, &mut live);
            assert!(
                live.len() > before,
                "sample {i} must advance coverage before half-diameter spacing"
            );
            assert_eq!(live.last().unwrap().center, event.surface_position);
            let real_count = live.len();
            stroke.push(
                PenEvent {
                    flags: SampleFlags::PREDICTED,
                    ..event
                },
                &mut live,
            );
            assert_eq!(live.len(), real_count);
        }
        let replay = stroke.finished_replay().unwrap();
        assert_eq!(
            live.iter().map(|d| d.center).collect::<Vec<_>>(),
            replay.iter().map(|d| d.center).collect::<Vec<_>>()
        );
        assert!(
            replay.last().unwrap().radii[0] < live.last().unwrap().radii[0],
            "end taper is still resolved on lift"
        );
    }
    fn p(x: f32, y: f32) -> Point {
        Point { x, y }
    }
    #[test]
    fn open_lines_and_retraces_do_not_close_but_near_start_does() {
        let mut loops = Loops::new(6.);
        for point in [
            p(10., 10.),
            p(50., 10.),
            p(10., 10.),
            p(50., 10.),
            p(50., 50.),
        ] {
            assert!(loops.push(point).is_empty());
        }
        assert_eq!(loops.push(p(11., 12.)).len(), 1);
        assert!(loops.push(p(90., 90.)).is_empty());
    }
    #[test]
    fn self_crossing_closes_only_the_loop_and_retains_open_tail() {
        let mut loops = Loops::new(1.);
        for point in [p(0., 0.), p(100., 0.), p(100., 100.), p(50., 100.)] {
            assert!(loops.push(point).is_empty());
        }
        let areas = loops.push(p(50., -50.));
        assert_eq!(areas.len(), 1);
        assert_eq!(loops.points, vec![p(0., 0.), p(50., 0.), p(50., -50.)]);
    }
}
