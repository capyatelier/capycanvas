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

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LayerPlacement {
    pub outer: Projective,
    pub mesh: Option<Arc<MeshMap>>,
    pub interpolation: Interpolation,
}
impl Default for LayerPlacement { fn default() -> Self { Self::IDENTITY } }
impl LayerPlacement {
    pub const IDENTITY: Self = Self { outer: Projective::IDENTITY, mesh: None, interpolation: Interpolation::Linear };
    pub fn from_affine(map: Affine) -> Self { Self::from_projective(Projective::from_affine(map)) }
    pub fn from_projective(outer: Projective) -> Self { Self { outer, ..Self::IDENTITY } }
    pub fn as_affine(&self) -> Option<Affine> { self.mesh.is_none().then(|| self.outer.as_affine()).flatten() }
    pub fn projective(&self) -> Option<Projective> { self.mesh.is_none().then_some(self.outer) }
    pub fn map(&self, p: Point) -> Option<Point> { self.outer.map(match &self.mesh { Some(mesh) => mesh.map(p)?, None => p }) }
    pub fn source_at(&self, point:Point, tolerance:f32)->Option<Point> {let inner=self.outer.inverse()?.map(point)?;match &self.mesh{Some(mesh)=>mesh.source_at(inner,tolerance/self.outer.magnification(mesh.drawn_bounds()).max(1e-6)),None=>Some(inner)}}
    pub fn post(&self, map: Projective) -> Option<Self> { Some(Self { outer: self.outer.then(map)?, ..self.clone() }) }
    pub fn validate_for(&self, source: Rect) -> Result<(), DocumentError> {
        let covered = match &self.mesh { Some(mesh) if mesh.valid() => mesh.drawn_bounds(), Some(_) => return Err(DocumentError::InvalidLayerOperation("Invalid mesh")), None => source };
        if self.outer.inverse().is_some() && self.outer.covers(covered) { Ok(()) }
        else { Err(DocumentError::InvalidLayerOperation("Invalid layer placement")) }
    }
    pub fn forward_bounds(&self, source: Rect) -> Rect {
        if source.is_empty() { return Rect::EMPTY; }
        self.outer.bounds(self.mesh.as_ref().map_or(source, |m| m.drawn_bounds())).unwrap_or(Rect::UNBOUNDED)
    }
    pub fn magnification(&self, source: Rect) -> f32 {
        match &self.mesh { Some(mesh) => mesh.magnification() * self.outer.magnification(mesh.drawn_bounds()), None => self.outer.magnification(source) }
    }
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImageTransform {
    pub placement: LayerPlacement,
    pub source_from_owner: Option<Projective>,
    pub keep_source: bool,
}
impl ImageTransform {
    pub fn affine(map: Affine) -> Self { Self { placement: LayerPlacement::from_affine(map), ..Default::default() } }
    pub fn projective(&self) -> Option<Projective> {
        self.source_from_owner.map_or(Some(Projective::IDENTITY), Projective::inverse)?.then(self.placement.projective()?)
    }
    pub fn as_affine(&self) -> Option<Affine> { self.projective()?.as_affine() }
    pub fn map(&self, p: Point) -> Option<Point> {
        self.placement.map(match self.source_from_owner { Some(map) => map.inverse()?.map(p)?, None => p })
    }
    pub fn source_at(&self,point:Point,tolerance:f32)->Option<Point> {let owner=self.placement.source_at(point,tolerance)?;match self.source_from_owner{Some(m)=>m.map(owner),None=>Some(owner)}}
    pub fn is_identity(&self) -> bool { self.as_affine() == Some(Affine::IDENTITY) }
    pub fn validate(&self) -> Result<(), DocumentError> {
        if self.placement.outer.inverse().is_none() || self.placement.mesh.as_ref().is_some_and(|m| !m.valid())
            || self.source_from_owner.is_some_and(|m| m.inverse().is_none()) {
            Err(DocumentError::InvalidLayerOperation("Invalid transform"))
        } else { Ok(()) }
    }
    pub fn validate_for(&self, source: Rect) -> Result<(), DocumentError> {
        self.validate()?;
        let domain = self.source_from_owner.map_or(Some(source), |m| m.inverse()?.bounds(source))
            .ok_or(DocumentError::InvalidLayerOperation("Invalid source placement"))?;
        self.placement.validate_for(domain)
    }
    pub fn conjugate(&self, to: Affine) -> Option<Self> {
        to.inverse()?;
        let to = Projective::from_affine(to);
        let outer = self.placement.outer.then(to)?;
        let source_from_owner = self.source_from_owner.unwrap_or(Projective::IDENTITY).then(to)?;
        let mut result = Self { placement: LayerPlacement { outer, ..self.placement.clone() }, source_from_owner: Some(source_from_owner), keep_source: self.keep_source };
        if result.placement.mesh.is_none() { result.placement.outer = result.projective()?; result.source_from_owner = None; }
        Some(result)
    }
    pub fn forward_bounds(&self, source: Rect) -> Rect {
        if source.is_empty() { return Rect::EMPTY; }
        let source = match self.source_from_owner { Some(map) => match map.inverse().and_then(|m| m.bounds(source)) { Some(r) => r, None => return Rect::UNBOUNDED }, None => source };
        self.placement.forward_bounds(source)
    }
    pub fn magnification(&self, source: Rect) -> f32 {
        match self.source_from_owner { Some(map) => match map.inverse() { Some(inverse) => inverse.bounds(source).map_or(f32::INFINITY, |r| inverse.magnification(source) * self.placement.magnification(r)), None => f32::INFINITY }, None => self.placement.magnification(source) }
    }
    pub fn affected_bounds(&self, source: Rect) -> Rect { let [cut, placed] = self.affected_regions(source); cut.union(placed) }
    pub fn affected_regions(&self, source: Rect) -> [Rect; 2] {
        if source.is_empty() || self.is_identity() { return [Rect::EMPTY; 2]; }
        [if self.keep_source { Rect::EMPTY } else { source }, self.forward_bounds(source.outset(self.placement.interpolation.support() as f32))]
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
    pub fn magnification(self) -> f32 {
        let [a, b, c, d, _, _] = self.0;
        let sum = a * a + b * b + c * c + d * d;
        let det = (a * d - b * c).abs();
        ((sum + (sum * sum - 4. * det * det).max(0.).sqrt()) * 0.5).sqrt()
    }
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
        let mut transform = ImageTransform { placement: { let mut placement = LayerPlacement::from_affine(Affine([4., 0., 0., 2., 300., 0.])); placement.interpolation = Interpolation::Nearest; placement }, ..Default::default() };
        let [cut, moved] = transform.affected_regions(source);
        assert_eq!(cut, source);
        assert_eq!(moved, rect(340., 40., 420., 80.));
        transform.placement.interpolation = Interpolation::Linear;
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
            LayerPlacement::from_affine(affine),
            LayerPlacement::from_projective(Projective::rect_to_quad(source, quad).unwrap()),
            LayerPlacement { mesh: Some(Arc::new(mesh.clone())), ..Default::default() },
        ] {
            let transform = ImageTransform { placement: { let mut placement = map.clone(); placement.interpolation = Interpolation::Bicubic; placement }, ..Default::default() };
            assert!(transform.validate().is_ok() && !transform.is_identity());
            let bounds = transform.forward_bounds(source).outset(1e-3);
            let moved = transform.conjugate(to).unwrap();
            assert_eq!(moved.placement.interpolation, transform.placement.interpolation);
            assert_eq!(moved.placement.mesh.is_some(), map.mesh.is_some());
            for p in samples(source) {
                let q = map.map(p).unwrap();
                assert!(q.x >= bounds.min.x && q.y >= bounds.min.y && q.x <= bounds.max.x && q.y <= bounds.max.y);
                near(moved.map(to.map(p)).unwrap(), to.map(q), 1e-2);
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
        for map in [LayerPlacement::from_affine(Affine::IDENTITY), LayerPlacement::from_projective(Projective::IDENTITY)] {
            assert!(ImageTransform { placement: map, ..Default::default() }.is_identity());
        }
        let broken = MeshMap {
            net: mesh.net[1..].into(),
            ..mesh
        };
        for map in [
            LayerPlacement::from_affine(Affine([0.; 6])),
            LayerPlacement::from_projective(Projective([1., 0., 0., 1., 0., 0., 0., 0., 0.])),
            LayerPlacement { mesh: Some(Arc::new(broken)), ..Default::default() },
        ] {
            assert!(ImageTransform { placement: map, ..Default::default() }.validate().is_err());
        }
    }
    #[test]
    fn perspective_transforms_bound_their_padded_source() {
        let source = rect(10., 10., 110., 60.);
        let quad = [[40., 0.], [90., 20.], [130., 90.], [0., 70.]].map(|[x, y]| Point { x, y });
        let mut transform = ImageTransform { placement: { let mut placement = LayerPlacement::from_projective(Projective::rect_to_quad(source, quad).unwrap()); placement.interpolation = Interpolation::Nearest; placement }, ..Default::default() };
        let [cut, moved] = transform.affected_regions(source);
        assert_eq!(cut, source);
        near(moved.min, Point { x: 0., y: 0. }, 2e-3);
        near(moved.max, Point { x: 130., y: 90. }, 2e-3);
        transform.placement.interpolation = Interpolation::Linear;
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
