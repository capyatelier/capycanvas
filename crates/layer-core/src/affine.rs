//! Document-space affine geometry shared by input, handles and GPU operations.
use crate::{Point, Rect};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    Nearest,
    #[default]
    Linear,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImageTransform {
    pub affine: Affine,
    pub interpolation: Interpolation,
}
impl ImageTransform {
    /// Conservative cut + placement footprint. Expand in source space before
    /// mapping, since scaling also enlarges the interpolation support.
    pub fn affected_bounds(self, source: Rect) -> Rect {
        let [cut, placed] = self.affected_regions(source);
        cut.union(placed)
    }
    /// Keep distant cut/placement regions separate for sparse allocation.
    pub fn affected_regions(self, source: Rect) -> [Rect; 2] {
        if source.is_empty() || self.affine == Affine::IDENTITY {
            return [Rect::EMPTY; 2];
        }
        let padding = f32::from(self.interpolation == Interpolation::Linear);
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
        [source, self.affine.bounds(support)]
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
            affine: Affine([4., 0., 0., 2., 300., 0.]),
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
