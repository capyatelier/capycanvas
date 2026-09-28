//! Document-space affine geometry shared by input, handles and GPU operations.
use crate::{DocumentError, MeshMap, Point, Projective, Rect};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    Nearest,
    /// Bilinear, averaging a grid of taps where the map minifies.
    #[default]
    Linear,
    /// Catmull-Rom with overshoot clamped to the nearest taps, averaging a
    /// grid of bilinear taps where the map minifies.
    Bicubic,
    /// Lanczos-3 over 6×6 taps, with overshoot clamped like Bicubic.
    Lanczos,
}
impl Interpolation {
    /// Source pixels beyond a sample position that the filter can read.
    pub fn support(self) -> u32 {
        match self {
            Self::Nearest => 0,
            Self::Linear => 1,
            Self::Bicubic => 2,
            Self::Lanczos => 3,
        }
    }
}

/// Source-to-destination geometry of one layer-local pixel transform.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TransformMap {
    Affine(Affine),
    Projective(Projective),
    Mesh(Arc<MeshMap>),
}
impl Default for TransformMap {
    fn default() -> Self {
        Self::Affine(Affine::IDENTITY)
    }
}
impl TransformMap {
    /// The destination of a source point, if it has one.
    pub fn map(&self, p: Point) -> Option<Point> {
        match self {
            Self::Affine(affine) => Some(affine.map(p)),
            Self::Projective(projective) => projective.map(p),
            Self::Mesh(mesh) => mesh.map(p),
        }
    }
    /// The map as a homography, unless it is a mesh.
    pub fn projective(&self) -> Option<Projective> {
        match self {
            Self::Affine(affine) => Some(Projective::from_affine(*affine)),
            Self::Projective(projective) => Some(*projective),
            Self::Mesh(_) => None,
        }
    }
}
impl From<Projective> for TransformMap {
    fn from(map: Projective) -> Self {
        map.as_affine().map_or(Self::Projective(map), Self::Affine)
    }
}

/// Transient pixel-transform command. Holders never persist it.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImageTransform {
    pub map: TransformMap,
    pub interpolation: Interpolation,
    /// Place a copy of the selected pixels and leave the originals in place.
    #[serde(default)]
    pub keep_source: bool,
}
impl ImageTransform {
    pub fn affine(affine: Affine) -> Self {
        Self {
            map: TransformMap::Affine(affine),
            ..Default::default()
        }
    }
    pub fn as_affine(&self) -> Option<Affine> {
        match self.map {
            TransformMap::Affine(affine) => Some(affine),
            _ => None,
        }
    }
    pub fn is_identity(&self) -> bool {
        self.map.projective().and_then(Projective::as_affine) == Some(Affine::IDENTITY)
    }
    /// Finite and invertible geometry that the renderer can resample. A
    /// perspective map resamples only the source it covers, and a mesh only
    /// its rectangle.
    pub fn validate(&self) -> Result<(), DocumentError> {
        let valid = match &self.map {
            TransformMap::Mesh(mesh) => mesh.valid(),
            map => map.projective().and_then(Projective::inverse).is_some(),
        };
        if valid {
            Ok(())
        } else {
            Err(DocumentError::InvalidLayerOperation("Invalid transform"))
        }
    }
    /// The same motion expressed in another layer-local space, where `to` maps
    /// this transform's space into it.
    pub fn conjugate(&self, to: Affine) -> Option<Self> {
        let from = to.inverse()?;
        let map = match &self.map {
            TransformMap::Affine(affine) => TransformMap::Affine(from.then(*affine).then(to)),
            TransformMap::Projective(projective) => TransformMap::Projective(
                Projective::from_affine(from)
                    .then(*projective)?
                    .then(Projective::from_affine(to))?,
            ),
            TransformMap::Mesh(mesh) => TransformMap::Mesh(Arc::new(MeshMap {
                frame: mesh.frame.then(to),
                ..mesh.post(to)
            })),
        };
        Some(Self { map, interpolation: self.interpolation, keep_source: self.keep_source })
    }
    /// Bounds of the mapped source, without interpolation support. Unbounded
    /// when part of the source has no image.
    pub fn forward_bounds(&self, source: Rect) -> Rect {
        if source.is_empty() {
            return Rect::EMPTY;
        }
        match &self.map {
            TransformMap::Mesh(mesh) => mesh.drawn_bounds(),
            map => map
                .projective()
                .and_then(|projective| projective.bounds(source))
                .unwrap_or(Rect::UNBOUNDED),
        }
    }
    /// Conservative cut + placement footprint. Expand in source space before
    /// mapping, since scaling also enlarges the interpolation support.
    pub fn affected_bounds(&self, source: Rect) -> Rect {
        let [cut, placed] = self.affected_regions(source);
        cut.union(placed)
    }
    /// Keep distant cut/placement regions separate for sparse allocation. A
    /// transform that keeps its source cuts nothing.
    pub fn affected_regions(&self, source: Rect) -> [Rect; 2] {
        if source.is_empty() || self.is_identity() {
            return [Rect::EMPTY; 2];
        }
        let support = source.outset(self.interpolation.support() as f32);
        let cut = if self.keep_source { Rect::EMPTY } else { source };
        [cut, self.forward_bounds(support)]
    }
}

/// Columns followed by translation: x'=a*x+c*y+tx, y'=b*x+d*y+ty.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Affine(pub [f32; 6]);
impl Default for Affine {
    fn default() -> Self {
        Self::IDENTITY
    }
}
impl Affine {
    pub const IDENTITY: Self = Self([1., 0., 0., 1., 0., 0.]);
    pub fn translation(p: Point) -> Self {
        Self([1., 0., 0., 1., p.x, p.y])
    }
    /// Scale and rotate about a pivot, then translate. Negative scales flip.
    pub fn around(pivot: Point, scale: [f32; 2], radians: f32, offset: Point) -> Self {
        let (s, c) = radians.sin_cos();
        Self::translation(Point {
            x: -pivot.x,
            y: -pivot.y,
        })
        .then(Self([
            c * scale[0],
            s * scale[0],
            -s * scale[1],
            c * scale[1],
            0.,
            0.,
        ]))
        .then(Self::translation(Point {
            x: pivot.x + offset.x,
            y: pivot.y + offset.y,
        }))
    }
    pub fn map(self, p: Point) -> Point {
        let [a, b, c, d, x, y] = self.0;
        Point {
            x: a * p.x + c * p.y + x,
            y: b * p.x + d * p.y + y,
        }
    }
    /// Apply self, then next (not the reverse).
    pub fn then(self, next: Self) -> Self {
        let [a, b, c, d, x, y] = self.0;
        let [e, f, g, h, u, v] = next.0;
        Self([
            e * a + g * b,
            f * a + h * b,
            e * c + g * d,
            f * c + h * d,
            e * x + g * y + u,
            f * x + h * y + v,
        ])
    }
    /// Reject nonfinite/singular matrices before packing shader parameters.
    pub fn inverse(self) -> Option<Self> {
        if self.0.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let [a, b, c, d, x, y] = self.0.map(f64::from);
        let det = a * d - b * c;
        if det == 0. {
            return None;
        }
        let inverse = [
            d / det,
            -b / det,
            -c / det,
            a / det,
            (c * y - d * x) / det,
            (b * x - a * y) / det,
        ]
        .map(|v| v as f32);
        inverse
            .iter()
            .all(|v| v.is_finite())
            .then_some(Self(inverse))
    }
    pub fn bounds(self, b: Rect) -> Rect {
        if b.is_empty() {
            return Rect::EMPTY;
        }
        Rect::around(b.corners().map(|p| self.map(p)))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{Selection, SelectionPixels};
    pub(crate) fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect {
            min: Point { x: x0, y: y0 },
            max: Point { x: x1, y: y1 },
        }
    }
    pub(crate) fn distance(a: Point, b: Point) -> f32 {
        (a.x - b.x).hypot(a.y - b.y)
    }
    pub(crate) fn near(a: Point, b: Point, tolerance: f32) {
        assert!(distance(a, b) <= tolerance, "{a:?} != {b:?}");
    }
    pub(crate) fn samples(bounds: Rect) -> impl Iterator<Item = Point> {
        (0..=12).flat_map(move |j| {
            (0..=12).map(move |i| Point {
                x: bounds.min.x + (bounds.max.x - bounds.min.x) * i as f32 / 12.,
                y: bounds.min.y + (bounds.max.y - bounds.min.y) * j as f32 / 12.,
            })
        })
    }
    #[test]
    fn damage_keeps_cut_and_placement_separate_and_scales_filter_support() {
        let source = rect(10., 20., 30., 40.);
        let mut transform = ImageTransform {
            map: TransformMap::Affine(Affine([4., 0., 0., 2., 300., 0.])),
            interpolation: Interpolation::Nearest,
            ..Default::default()
        };
        let [cut, moved] = transform.affected_regions(source);
        assert_eq!(cut, source);
        assert_eq!(moved, rect(340., 40., 420., 80.));
        transform.interpolation = Interpolation::Linear;
        assert_eq!(transform.affected_regions(source)[1], rect(336., 38., 424., 82.));
        assert_eq!(
            ImageTransform::default().affected_bounds(source),
            Rect::EMPTY
        );
        assert_eq!(transform.affected_bounds(Rect::EMPTY), Rect::EMPTY);
        transform.keep_source = true;
        assert_eq!(transform.affected_regions(source), [Rect::EMPTY, rect(336., 38., 424., 82.)]);
        assert!(transform.conjugate(Affine::translation(Point { x: 3., y: 4. })).unwrap().keep_source);
    }
    #[test]
    fn every_map_kind_validates_conjugates_bounds_and_carries_selections() {
        let source = rect(100., 50., 500., 350.);
        let affine = Affine::around(Point { x: 300., y: 200. }, [1.2, -0.8], 0.25, Point { x: 30., y: 10. });
        let quad = [[140., 40.], [520., 90.], [470., 380.], [60., 300.]].map(|[x, y]| Point { x, y });
        let mesh = MeshMap::from_affine(source, [3, 3], affine)
            .unwrap()
            .move_node(5, Point { x: 25., y: -15. })
            .unwrap();
        let to = Affine::around(Point::default(), [2., -1.], 0.3, Point { x: 5., y: 9. });
        let placement = Affine::translation(Point { x: 20., y: 5. });
        let ring = [[120., 80.], [400., 70.], [450., 300.], [150., 320.]].map(|[x, y]| Point { x, y });
        let pixels = Selection::pixels(Arc::new(
            SelectionPixels::new([8, 1], [0, 0, 8, 1], vec![0x4444]).unwrap(),
        ));
        for map in [
            TransformMap::Affine(affine),
            TransformMap::Projective(Projective::rect_to_quad(source, quad).unwrap()),
            TransformMap::Mesh(Arc::new(mesh.clone())),
        ] {
            let transform = ImageTransform {
                map: map.clone(),
                interpolation: Interpolation::Bicubic,
                ..Default::default()
            };
            assert!(transform.validate().is_ok() && !transform.is_identity());
            let bounds = transform.forward_bounds(source).outset(1e-3);
            let moved = transform.conjugate(to).unwrap();
            assert_eq!(moved.interpolation, transform.interpolation);
            assert_eq!(std::mem::discriminant(&moved.map), std::mem::discriminant(&map));
            for p in samples(source) {
                let q = map.map(p).unwrap();
                assert!(q.x >= bounds.min.x && q.y >= bounds.min.y && q.x <= bounds.max.x && q.y <= bounds.max.y);
                near(moved.map.map(to.map(p)).unwrap(), to.map(q), 1e-2);
            }
            assert!(transform.conjugate(Affine([0.; 6])).is_none());
            let mut selection = Selection::polygon(ring.to_vec()).unwrap().transformed(placement).unwrap();
            selection.inverted = true;
            let mapped = selection.mapped(&map).unwrap();
            let [contour] = mapped.contours() else { panic!("one ring") };
            assert!(mapped.inverted);
            for v in ring {
                let expected = map.map(placement.map(v)).unwrap();
                assert!(contour.iter().any(|p| distance(mapped.affine.map(*p), expected) < 1e-2), "{map:?}");
            }
            assert!(!selection.needs_resample(&map));
            assert_eq!(pixels.needs_resample(&map), pixels.mapped(&map).is_err());
        }
        for map in [TransformMap::Affine(Affine::IDENTITY), TransformMap::Projective(Projective::IDENTITY)] {
            assert!(ImageTransform { map, ..Default::default() }.is_identity());
        }
        let broken = MeshMap {
            net: mesh.net[1..].into(),
            ..mesh
        };
        for map in [
            TransformMap::Affine(Affine([0.; 6])),
            TransformMap::Projective(Projective([1., 0., 0., 1., 0., 0., 0., 0., 0.])),
            TransformMap::Mesh(Arc::new(broken)),
        ] {
            assert!(ImageTransform { map, ..Default::default() }.validate().is_err());
        }
    }
    #[test]
    fn perspective_transforms_bound_their_padded_source() {
        let source = rect(10., 10., 110., 60.);
        let quad = [[40., 0.], [90., 20.], [130., 90.], [0., 70.]].map(|[x, y]| Point { x, y });
        let mut transform = ImageTransform {
            map: TransformMap::Projective(Projective::rect_to_quad(source, quad).unwrap()),
            interpolation: Interpolation::Nearest,
            ..Default::default()
        };
        let [cut, moved] = transform.affected_regions(source);
        assert_eq!(cut, source);
        near(moved.min, Point { x: 0., y: 0. }, 2e-3);
        near(moved.max, Point { x: 130., y: 90. }, 2e-3);
        transform.interpolation = Interpolation::Linear;
        let padded = transform.affected_regions(source)[1];
        assert!(padded.min.x < moved.min.x && padded.max.y > moved.max.y);
        assert_eq!(transform.forward_bounds(rect(10., 10., 110., 1e5)), Rect::UNBOUNDED);
    }
    #[test]
    fn affine_composition_pivots_bounds_and_inverse_agree() {
        let pivot = Point { x: 64., y: 48. };
        let offset = Point { x: -10., y: 21. };
        for scale in [[1., 1.], [-2., 0.5], [0.05, -3.]] {
            for angle in [0., 0.7, std::f32::consts::FRAC_PI_2] {
                let m = Affine::around(pivot, scale, angle, offset);
                near(
                    m.map(pivot),
                    Point {
                        x: pivot.x + offset.x,
                        y: pivot.y + offset.y,
                    },
                    3e-3,
                );
                let inverse = m.inverse().unwrap();
                for p in [Point::default(), pivot, Point { x: -250., y: 100. }] {
                    near(inverse.map(m.map(p)), p, 3e-3);
                    near(m.then(inverse).map(p), p, 3e-3);
                }
                let r = rect(0., 0., 128., 96.);
                let b = m.bounds(r);
                for p in [r.min, r.max, pivot] {
                    let p = m.map(p);
                    assert!(p.x >= b.min.x && p.x <= b.max.x && p.y >= b.min.y && p.y <= b.max.y);
                }
            }
        }
        for m in [
            [0.; 6],
            [1., 2., 2., 4., 0., 0.],
            [f32::NAN; 6],
            [f32::INFINITY; 6],
        ] {
            assert!(Affine(m).inverse().is_none());
        }
        assert_eq!(Affine::IDENTITY.bounds(Rect::EMPTY), Rect::EMPTY);
    }
}
