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
}
impl Interpolation {
    /// Source pixels beyond a sample position that the filter can read.
    pub fn support(self) -> u32 {
        match self {
            Self::Nearest => 0,
            Self::Linear => 1,
            Self::Bicubic => 2,
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
            Self::Mesh(_) => None,
        }
    }
}

/// Transient pixel-transform command. Holders never persist it.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImageTransform {
    pub map: TransformMap,
    pub interpolation: Interpolation,
}
impl ImageTransform {
    pub fn affine(affine: Affine) -> Self {
        Self {
            map: TransformMap::Affine(affine),
            interpolation: Interpolation::default(),
        }
    }
    pub fn as_affine(&self) -> Option<Affine> {
        match self.map {
            TransformMap::Affine(affine) => Some(affine),
            _ => None,
        }
    }
    pub fn is_identity(&self) -> bool {
        match &self.map {
            TransformMap::Affine(affine) => *affine == Affine::IDENTITY,
            TransformMap::Projective(projective) => {
                projective.as_affine() == Some(Affine::IDENTITY)
            }
            TransformMap::Mesh(_) => false,
        }
    }
    /// Finite and invertible geometry that the renderer can resample. A
    /// perspective map resamples only the source it covers.
    pub fn validate(&self) -> Result<(), DocumentError> {
        let valid = match &self.map {
            TransformMap::Affine(affine) => affine.inverse().is_some(),
            TransformMap::Projective(projective) => projective.inverse().is_some(),
            TransformMap::Mesh(_) => {
                return Err(DocumentError::InvalidLayerOperation(
                    "Unsupported transform",
                ));
            }
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
            TransformMap::Projective(projective) => {
                TransformMap::Projective(projective.conjugate(to)?)
            }
            TransformMap::Mesh(_) => return None,
        };
        Some(Self {
            map,
            interpolation: self.interpolation,
        })
    }
    /// Bounds of the mapped source, without interpolation support. Unbounded
    /// when part of the source has no image.
    pub fn forward_bounds(&self, source: Rect) -> Rect {
        if source.is_empty() {
            return Rect::EMPTY;
        }
        match &self.map {
            TransformMap::Affine(affine) => affine.bounds(source),
            TransformMap::Projective(projective) => {
                projective.bounds(source).unwrap_or(Rect::UNBOUNDED)
            }
            TransformMap::Mesh(_) => Rect::UNBOUNDED,
        }
    }
    /// Conservative cut + placement footprint. Expand in source space before
    /// mapping, since scaling also enlarges the interpolation support.
    pub fn affected_bounds(&self, source: Rect) -> Rect {
        let [cut, placed] = self.affected_regions(source);
        cut.union(placed)
    }
    /// Keep distant cut/placement regions separate for sparse allocation.
    pub fn affected_regions(&self, source: Rect) -> [Rect; 2] {
        if source.is_empty() || self.is_identity() {
            return [Rect::EMPTY; 2];
        }
        let padding = self.interpolation.support() as f32;
        let support = Rect {
            min: Point {
                x: source.min.x - padding,
                y: source.min.y - padding,
            },
            max: Point {
                x: source.max.x + padding,
                y: source.max.y + padding,
            },
        };
        [source, self.forward_bounds(support)]
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
        let mut out = Rect::EMPTY;
        for p in [
            b.min,
            Point {
                x: b.max.x,
                y: b.min.y,
            },
            b.max,
            Point {
                x: b.min.x,
                y: b.max.y,
            },
        ] {
            out.include_circle(self.map(p), 0.);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn near(a: Point, b: Point) {
        assert!(
            (a.x - b.x).abs() < 0.002 && (a.y - b.y).abs() < 0.002,
            "{a:?} != {b:?}"
        );
    }
    #[test]
    fn damage_keeps_cut_and_placement_separate_and_scales_filter_support() {
        let source = Rect {
            min: Point { x: 10., y: 20. },
            max: Point { x: 30., y: 40. },
        };
        let mut transform = ImageTransform {
            map: TransformMap::Affine(Affine([4., 0., 0., 2., 300., 0.])),
            interpolation: Interpolation::Nearest,
        };
        let [cut, moved] = transform.affected_regions(source);
        assert_eq!(cut, source);
        assert_eq!(
            moved,
            Rect {
                min: Point { x: 340., y: 40. },
                max: Point { x: 420., y: 80. }
            }
        );
        transform.interpolation = Interpolation::Linear;
        let moved = transform.affected_regions(source)[1];
        assert_eq!(
            moved,
            Rect {
                min: Point { x: 336., y: 38. },
                max: Point { x: 424., y: 82. }
            }
        );
        assert_eq!(
            ImageTransform::default().affected_bounds(source),
            Rect::EMPTY
        );
        assert_eq!(transform.affected_bounds(Rect::EMPTY), Rect::EMPTY);
    }
    #[test]
    fn image_transforms_validate_and_conjugate_their_geometry() {
        let affine = Affine::around(
            Point { x: 20., y: 10. },
            [1.5, -0.5],
            0.4,
            Point { x: 3., y: -7. },
        );
        let transform = ImageTransform::affine(affine);
        assert_eq!(transform.as_affine(), Some(affine));
        assert!(transform.validate().is_ok() && !transform.is_identity());
        assert!(ImageTransform::default().is_identity());
        assert!(ImageTransform::affine(Affine([0.; 6])).validate().is_err());
        let to =
            Affine::translation(Point { x: 40., y: -12. }).then(Affine([0., 2., -2., 0., 0., 0.]));
        let moved = transform.conjugate(to).unwrap();
        assert_eq!(moved.interpolation, transform.interpolation);
        for p in [Point::default(), Point { x: 31., y: -4. }] {
            near(
                moved.as_affine().unwrap().map(to.map(p)),
                to.map(affine.map(p)),
            );
        }
        assert!(transform.conjugate(Affine([0.; 6])).is_none());
    }
    #[test]
    fn perspective_transforms_bound_their_padded_source() {
        let source = Rect {
            min: Point { x: 10., y: 10. },
            max: Point { x: 110., y: 60. },
        };
        let quad = [
            Point { x: 40., y: 0. },
            Point { x: 90., y: 20. },
            Point { x: 130., y: 90. },
            Point { x: 0., y: 70. },
        ];
        let projective = Projective::rect_to_quad(source, quad).unwrap();
        let mut transform = ImageTransform {
            map: TransformMap::Projective(projective),
            interpolation: Interpolation::Nearest,
        };
        assert!(transform.validate().is_ok() && !transform.is_identity());
        let [cut, moved] = transform.affected_regions(source);
        assert_eq!(cut, source);
        near(moved.min, Point { x: 0., y: 0. });
        near(moved.max, Point { x: 130., y: 90. });
        transform.interpolation = Interpolation::Linear;
        let padded = transform.affected_regions(source)[1];
        assert!(padded.min.x < moved.min.x && padded.max.y > moved.max.y);
        let far = Rect {
            min: source.min,
            max: Point { x: 110., y: 1e5 },
        };
        assert_eq!(transform.forward_bounds(far), Rect::UNBOUNDED);
        let to = Affine::around(Point::default(), [2., -1.], 0.3, Point { x: 5., y: 9. });
        let conjugate = transform.conjugate(to).unwrap();
        let TransformMap::Projective(moved_map) = conjugate.map else {
            panic!("projective")
        };
        let p = Point { x: 50., y: 30. };
        let [a, b] = [
            moved_map.map(to.map(p)).unwrap(),
            to.map(projective.map(p).unwrap()),
        ];
        assert!((a.x - b.x).abs() < 1e-2 && (a.y - b.y).abs() < 1e-2);
        let identity = ImageTransform {
            map: TransformMap::Projective(Projective::IDENTITY),
            ..Default::default()
        };
        assert!(identity.is_identity());
        let singular = TransformMap::Projective(Projective([1., 0., 0., 1., 0., 0., 0., 0., 0.]));
        assert!(
            ImageTransform {
                map: singular,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
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
                );
                let inverse = m.inverse().unwrap();
                for p in [Point::default(), pivot, Point { x: -250., y: 100. }] {
                    near(inverse.map(m.map(p)), p);
                    near(m.then(inverse).map(p), p);
                }
                let r = Rect {
                    min: Point::default(),
                    max: Point { x: 128., y: 96. },
                };
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
