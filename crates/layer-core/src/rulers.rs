//! Document-space drawing guides. Snapping is geometry, never pixel rendering.
use crate::{DocumentError, Point, Rect};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RulerKind {
    #[default]
    Straight,
    Parallel,
    Radial,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RulerGeometry {
    Straight { start: Point, end: Point },
    Parallel { start: Point, end: Point },
    Radial { center: Point },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ruler {
    pub id: u64,
    pub geometry: RulerGeometry,
}

/// A stroke owns this snapshot, so moving/changing guides cannot bend old ink.
/// Only a radial stroke starting exactly at its center has no direction yet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RulerConstraint {
    pub origin: Point,
    pub direction: Option<Point>,
}

fn sub(a: Point, b: Point) -> Point {
    Point {
        x: a.x - b.x,
        y: a.y - b.y,
    }
}
fn unit(d: Point) -> Option<Point> {
    let length = d.x.hypot(d.y);
    (length.is_finite() && length >= 0.001).then(|| Point {
        x: d.x / length,
        y: d.y / length,
    })
}
fn dot(a: Point, b: Point) -> f32 {
    a.x * b.x + a.y * b.y
}

impl RulerGeometry {
    pub fn kind(self) -> RulerKind {
        match self {
            Self::Straight { .. } => RulerKind::Straight,
            Self::Parallel { .. } => RulerKind::Parallel,
            Self::Radial { .. } => RulerKind::Radial,
        }
    }
    pub fn handles(self) -> (Point, Option<Point>) {
        match self {
            Self::Straight { start, end } | Self::Parallel { start, end } => (start, Some(end)),
            Self::Radial { center } => (center, None),
        }
    }
    pub fn from_drag(kind: RulerKind, start: Point, end: Point) -> Self {
        match kind {
            RulerKind::Straight => Self::Straight { start, end },
            RulerKind::Parallel => Self::Parallel { start, end },
            RulerKind::Radial => Self::Radial { center: start },
        }
    }
    pub fn translated(self, delta: Point) -> Self {
        let (a, b) = self.handles();
        let shift = |p: Point| Point {
            x: p.x + delta.x,
            y: p.y + delta.y,
        };
        Self::from_drag(self.kind(), shift(a), shift(b.unwrap_or(a)))
    }
    pub fn validate(self) -> Result<(), DocumentError> {
        let (a, b) = self.handles();
        if [a, b.unwrap_or(a)]
            .iter()
            .any(|p| !(p.x * p.x + p.y * p.y).is_finite())
        {
            return Err(DocumentError::InvalidRuler("Invalid ruler coordinates"));
        }
        if b.is_some_and(|b| unit(sub(b, a)).is_none()) {
            return Err(DocumentError::InvalidRuler(
                "A ruler needs two distinct points",
            ));
        }
        Ok(())
    }
    /// Distance to a visible ruler body; endpoints are tested separately by UI.
    pub fn distance(self, point: Point) -> f32 {
        let (a, b) = self.handles();
        let p = sub(point, a);
        b.and_then(|b| unit(sub(b, a)))
            .map_or(p.x.hypot(p.y), |d| (p.x * d.y - p.y * d.x).abs())
    }
}

impl RulerConstraint {
    /// Predicted input can preview a ray but cannot choose its durable direction.
    pub fn resolve(&mut self, point: Point) {
        if self.direction.is_none() {
            self.direction = unit(sub(point, self.origin));
        }
    }
    pub fn project(self, point: Point) -> Point {
        let Some(d) = self.direction else {
            return point;
        };
        let t = dot(sub(point, self.origin), d);
        Point {
            x: self.origin.x + t * d.x,
            y: self.origin.y + t * d.y,
        }
    }
    /// Compose the projection with a surface-to-document affine transform.
    /// Pressure, tilt, timestamps and prediction flags are unaffected.
    pub fn transform(self, m: [f32; 6]) -> [f32; 6] {
        let Some(d) = self.direction else { return m };
        let x = dot(d, Point { x: m[0], y: m[1] });
        let y = dot(d, Point { x: m[2], y: m[3] });
        let t = self.project(Point { x: m[4], y: m[5] });
        [d.x * x, d.y * x, d.x * y, d.y * y, t.x, t.y]
    }
    /// Clip the infinite guide to document bounds for the presentation overlay.
    pub fn clipped(self, bounds: Rect) -> Option<[Point; 2]> {
        let d = self.direction?;
        let mut range = [f32::NEG_INFINITY, f32::INFINITY];
        for (o, v, lo, hi) in [
            (self.origin.x, d.x, bounds.min.x, bounds.max.x),
            (self.origin.y, d.y, bounds.min.y, bounds.max.y),
        ] {
            if v.abs() < 0.000001 {
                if o < lo || o > hi {
                    return None;
                }
            } else {
                let (a, b) = ((lo - o) / v, (hi - o) / v);
                range[0] = range[0].max(a.min(b));
                range[1] = range[1].min(a.max(b));
            }
        }
        (range[0] <= range[1]).then(|| {
            range.map(|t| Point {
                x: self.origin.x + t * d.x,
                y: self.origin.y + t * d.y,
            })
        })
    }
}

/// A nearby straight guide wins over global parallel/radial assistants. Among
/// global assistants, choose the closest anchor. This runs once per stroke.
pub fn choose_ruler(rulers: &[Ruler], point: Point, reach: f32) -> Option<RulerConstraint> {
    if !point.x.is_finite() || !point.y.is_finite() || !reach.is_finite() || reach < 0. {
        return None;
    }
    rulers
        .iter()
        .rev()
        .filter_map(|r| {
            let (a, b) = r.geometry.handles();
            let delta = sub(point, a);
            let (distance, origin, direction) = match r.geometry {
                RulerGeometry::Straight { .. } => {
                    let distance = r.geometry.distance(point);
                    if distance > reach {
                        return None;
                    }
                    (distance, a, unit(sub(b?, a)))
                }
                RulerGeometry::Parallel { .. } => {
                    (reach + delta.x.hypot(delta.y), point, unit(sub(b?, a)))
                }
                RulerGeometry::Radial { .. } => (reach + delta.x.hypot(delta.y), a, unit(delta)),
            };
            Some((distance, RulerConstraint { origin, direction }))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, snap)| snap)
}

pub(crate) fn validate_rulers(rulers: &[Ruler]) -> Result<(), DocumentError> {
    if rulers.len() > 1024 {
        return Err(DocumentError::InvalidRuler("Too many rulers"));
    }
    let mut ids = std::collections::BTreeSet::new();
    for r in rulers {
        if r.id == 0 || !ids.insert(r.id) {
            return Err(DocumentError::InvalidRuler("Invalid or duplicate ruler ID"));
        }
        r.geometry.validate()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, Edit, Editor};
    fn p(x: f32, y: f32) -> Point {
        Point { x, y }
    }
    fn near(a: Point, b: Point) {
        assert!((a.x - b.x).hypot(a.y - b.y) < 0.0001, "{a:?} vs {b:?}");
    }
    #[test]
    fn constraints_are_local_stable_and_compose_with_affine_views() {
        let line = Ruler {
            id: 1,
            geometry: RulerGeometry::Straight {
                start: p(0., 10.),
                end: p(100., 10.),
            },
        };
        assert!(choose_ruler(&[line], p(30., 40.), 12.).is_none());
        let snap = choose_ruler(&[line], p(30., 14.), 12.).unwrap();
        near(snap.project(p(200., 60.)), p(200., 10.));
        let parallel = Ruler {
            id: 2,
            geometry: RulerGeometry::Parallel {
                start: p(0., 0.),
                end: p(10., 10.),
            },
        };
        let snap = choose_ruler(&[parallel, line], p(30., 40.), 12.).unwrap();
        near(snap.project(p(40., 40.)), p(35., 45.));
        let snap = choose_ruler(&[parallel, line], p(30., 14.), 12.).unwrap();
        near(snap.project(p(40., 80.)), p(40., 10.));
        let radial = Ruler {
            id: 3,
            geometry: RulerGeometry::Radial { center: p(5., 5.) },
        };
        let mut snap = choose_ruler(&[radial], p(5., 5.), 12.).unwrap();
        assert_eq!(snap.direction, None);
        snap.resolve(p(15., 15.));
        snap.resolve(p(0., 50.));
        near(snap.project(p(25., 5.)), p(15., 15.));
        for m in [
            [1., 0., 0., 1., 0., 0.],
            [0., -2., 2., 0., 40., 30.],
            [-0.5, 0., 0., 0.5, -3., 9.],
        ] {
            let transform = |m: [f32; 6], q: Point| {
                p(
                    m[0] * q.x + m[2] * q.y + m[4],
                    m[1] * q.x + m[3] * q.y + m[5],
                )
            };
            near(
                transform(snap.transform(m), p(42., -17.)),
                snap.project(transform(m, p(42., -17.))),
            );
        }
        let clipped = snap
            .clipped(Rect {
                min: p(0., 0.),
                max: p(100., 80.),
            })
            .unwrap();
        near(clipped[0], p(0., 0.));
        near(clipped[1], p(80., 80.));
    }
    #[test]
    fn ruler_history_is_validated_atomic_and_does_not_change_paint() {
        let mut editor = Editor::new(Document::new("rulers", 128, 128));
        let paint = editor.document().layers.clone();
        let ruler = Ruler {
            id: 1,
            geometry: RulerGeometry::Straight {
                start: p(0., 0.),
                end: p(10., 20.),
            },
        };
        editor.perform(Edit::SetRulers(vec![ruler])).unwrap();
        assert!(!editor.undo_changes_image());
        editor.undo().unwrap();
        assert!(editor.document().rulers.is_empty());
        assert!(!editor.redo_changes_image());
        editor.redo().unwrap();
        assert_eq!(editor.document().rulers, [ruler]);
        let before = editor.document().clone();
        for bad in [
            vec![ruler, ruler],
            vec![Ruler { id: 0, ..ruler }],
            vec![Ruler {
                id: 2,
                geometry: RulerGeometry::Straight {
                    start: p(0., 0.),
                    end: p(0., 0.),
                },
            }],
            vec![Ruler {
                id: 2,
                geometry: RulerGeometry::Radial {
                    center: p(f32::NAN, 0.),
                },
            }],
        ] {
            assert!(
                editor
                    .perform(Edit::Batch(vec![
                        Edit::SetRulers(Vec::new()),
                        Edit::SetRulers(bad)
                    ]))
                    .is_err()
            );
            assert_eq!(editor.document(), &before);
        }
        assert_eq!(editor.document().layers, paint);
    }
}
