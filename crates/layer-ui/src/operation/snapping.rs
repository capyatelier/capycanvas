use super::*;
use layer_core::{Ruler, RulerConstraint, RulerGeometry};

#[derive(Clone)]
pub(super) struct Snapping {
    targets: Arc<[(Option<OccurrenceHandle>, Rect)]>,
    rulers: Arc<[Ruler]>,
    axes: [Option<(usize, usize, usize)>; 2],
    ruler: Option<RulerConstraint>,
    pub guides: Vec<[Point; 2]>,
}
impl Snapping {
    pub fn new(mut targets: Vec<(Option<OccurrenceHandle>, Rect)>, mut rulers: Vec<Ruler>) -> Self {
        targets.sort_unstable_by_key(|(id, _)| *id);
        rulers.sort_unstable_by_key(|ruler| ruler.id);
        Self { targets: targets.into(), rulers: rulers.into(), axes: [None; 2], ruler: None, guides: Vec::new() }
    }
    pub fn retain_guides(&mut self, bounds: Rect, units: f32) {
        if let Some(ruler) = self.ruler {
            let position = center(bounds); let offset = sub(ruler.project(position), position);
            if offset.x.hypot(offset.y) > units * 0.25 { self.ruler = None; self.guides.clear(); }
            return;
        }
        for axis in 0..2 {
            if let Some((target, edge, source)) = self.axes[axis] {
                let value = |bounds: Rect, index: usize| {
                    let [min, max] = if axis == 0 { [bounds.min.x, bounds.max.x] } else { [bounds.min.y, bounds.max.y] };
                    [min, (min + max) * 0.5, max][index]
                };
                if (value(bounds, source) - value(self.targets[target].1, edge)).abs() > units * 0.25 { self.axes[axis] = None; }
            }
        }
        self.guides.retain(|[from, to]| self.axes[usize::from(from.x != to.x)].is_some());
    }
    pub fn correction(&mut self, bounds: Rect, origin: Point, direction: Option<Point>, units: f32) -> Point {
        let position = center(bounds);
        let components = |bounds: Rect, axis: usize| {
            let [min, max] = if axis == 0 { [bounds.min.x, bounds.max.x] } else { [bounds.min.y, bounds.max.y] };
            [min, (min + max) * 0.5, max]
        };
        let length = |point: Point| point.x.hypot(point.y);
        let direction = direction.map(|point| { let norm = length(point).max(f32::MIN_POSITIVE); Point { x: point.x / norm, y: point.y / norm } });
        let mut delta = [0.; 2];
        self.guides.clear();
        let guide_bounds = self.targets.iter().fold(bounds, |all, (_, bounds)| all.union(*bounds)).outset(24. * units);
        for (axis, change) in delta.iter_mut().enumerate() {
            let from = components(bounds, axis);
            let factor = direction.map_or(1., |direction| if axis == 0 { direction.x } else { direction.y });
            if factor.abs() < 1e-5 { self.axes[axis] = None; continue; }
            let distance = |(target, edge, source): (usize, usize, usize)| (components(self.targets[target].1, axis)[edge] - from[source]) / factor;
            let held = self.axes[axis].filter(|key| distance(*key).abs() <= 10. * units);
            self.axes[axis] = held.or_else(|| {
                let mut best = None;
                let mut nearest = 6. * units;
                for target in 0..self.targets.len() { for edge in 0..3 { for source in 0..3 {
                    let key = (target, edge, source);
                    let distance = distance(key).abs();
                    if distance <= nearest && (best.is_none() || distance < nearest) { nearest = distance; best = Some(key); }
                }}}
                best
            });
            if let Some(key) = self.axes[axis] { *change = distance(key); }
        }
        let offset = if let Some(direction) = direction {
            let axis = (0..2).filter(|axis| self.axes[*axis].is_some()).min_by(|a, b| delta[*a].abs().total_cmp(&delta[*b].abs()));
            axis.map_or(Point::default(), |axis| {
                if (delta[1-axis] - delta[axis]).abs() > units * 0.25 { self.axes[1-axis] = None; }
                Point { x: direction.x * delta[axis], y: direction.y * delta[axis] }
            })
        } else { Point { x: delta[0], y: delta[1] } };
        let project = |constraint: RulerConstraint| {
            let projected = constraint.project(position);
            let correction = sub(projected, position);
            let Some(direction) = direction else { return Some(correction); };
            let normal = sub(direction, sub(constraint.project(add(position, direction)), projected));
            let denominator = normal.x * normal.x + normal.y * normal.y;
            if denominator < 1e-8 { return (length(correction) <= units * 0.25).then_some(Point::default()); }
            let along = (correction.x * normal.x + correction.y * normal.y) / denominator;
            Some(Point { x: direction.x * along, y: direction.y * along })
        };
        let held = self.ruler.map(|mut ruler| { ruler.resolve(position); ruler })
            .filter(|ruler| project(*ruler).is_some_and(|offset| length(offset) <= 10. * units));
        let candidate = held.or_else(|| {
            self.rulers.iter().filter_map(|ruler| {
                let at = if matches!(ruler.geometry, RulerGeometry::Straight { .. }) { position } else { origin };
                let mut constraint = layer_core::choose_ruler(std::slice::from_ref(ruler), at, 6. * units)?;
                constraint.resolve(position);
                let distance = length(project(constraint)?);
                (distance <= 6. * units).then_some((distance, ruler.id, constraint))
            }).min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1))).map(|(_, _, constraint)| constraint)
        });
        self.ruler = candidate.filter(|ruler| held.is_some() || self.axes.iter().all(Option::is_none)
            || project(*ruler).is_some_and(|point| length(point) < length(offset)));
        if let Some(ruler) = self.ruler {
            self.axes = [None; 2];
            self.guides.extend(ruler.clipped(guide_bounds));
            project(ruler).unwrap_or_default()
        } else {
            for (axis, key) in self.axes.iter().enumerate() {
                if let Some((target, edge, _)) = key {
                    let at = components(self.targets[*target].1, axis)[*edge];
                    self.guides.push(if axis == 0 { [Point { x: at, y: guide_bounds.min.y }, Point { x: at, y: guide_bounds.max.y }] }
                        else { [Point { x: guide_bounds.min.x, y: at }, Point { x: guide_bounds.max.x, y: at }] });
                }
            }
            offset
        }
    }

}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn transform_snapping(&self) -> Option<Snapping> {
        if !self.operation.snapping { return None; }
        let doc = self.engine.document();
        let mut targets = vec![(None, Rect::from_extent(doc.composition().size))];
        targets.extend(self.measured_snap_bounds().into_iter().map(|(h, b)| (Some(h), b)));
        Some(Snapping::new(targets, if self.rulers.visible { doc.rulers().collect() } else { Vec::new() }))
    }
}

#[cfg(test)] mod regression {
    use super::*;
    fn rect(x:f32,y:f32)->Rect {Rect {min:Point{x,y},max:Point{x:x+20.,y:y+20.}}}
    #[test] fn constrained_axis_snap_keeps_direction_and_uses_logical_distance() {
        for units in [0.25,1.,4.] {
            let mut snap=Snapping::new(vec![(Some(OccurrenceHandle::from_index(1)),Rect{min:Point{x:0.,y:1000.},max:Point{x:1000.,y:2000.}})],vec![]);
            let offset=snap.correction(rect(4.*units,100.),Point::default(),Some(Point{x:1.,y:1.}),units);
            assert!((offset.x-offset.y).abs()<1e-5);
            assert!((offset.x+4.*units).abs()<1e-5);
            assert!(!snap.guides.is_empty());
        }
    }
    #[test] fn snap_capture_and_release_have_six_and_ten_pixel_hysteresis() {
        let mut snap=Snapping::new(vec![(Some(OccurrenceHandle::from_index(1)),Rect{min:Point{x:0.,y:1000.},max:Point{x:1000.,y:2000.}})],vec![]);
        assert_eq!(snap.correction(rect(5.,100.),Point::default(),Some(Point{x:1.,y:0.}),1.).x,-5.);
        assert_eq!(snap.correction(rect(9.,100.),Point::default(),Some(Point{x:1.,y:0.}),1.).x,-9.);
        assert_eq!(snap.correction(rect(11.,100.),Point::default(),Some(Point{x:1.,y:0.}),1.),Point::default());
        assert!(snap.guides.is_empty());
    }
    #[test] fn ruler_intersection_respects_the_motion_constraint() {
        let ruler=Ruler{id:layer_core::authored::PortableId::random(),geometry:RulerGeometry::Straight{start:Point{x:0.,y:0.},end:Point{x:0.,y:100.}}};
        let mut snap=Snapping::new(vec![],vec![ruler]);
        let bounds=Rect{min:Point{x:-8.,y:30.},max:Point{x:12.,y:50.}};
        let offset=snap.correction(bounds,Point::default(),Some(Point{x:1.,y:1.}),1.);
        assert!((offset.x+2.).abs()<1e-5 && (offset.y+2.).abs()<1e-5);
        assert!(!snap.guides.is_empty());
    }
}
