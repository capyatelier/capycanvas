//! Perspective maps between layer-local pixel spaces.
use crate::{Affine, Point, Rect};
use std::sync::Arc;

/// Forward homography from source to destination layer-local pixels, row-major:
/// `[x', y', w'] = M [x, y, 1]`, mapping to `(x'/w', y'/w')`. Only points with
/// `w' > 0` have an image.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Projective(pub [f32; 9]);

/// The smallest w' a covered rectangle may reach, relative to its largest.
/// Beyond this the image is numerically a triangle.
const MIN_WEIGHT_RATIO: f64 = 1. / 1024.;

impl Projective {
    pub const IDENTITY: Self = Self([1., 0., 0., 0., 1., 0., 0., 0., 1.]);

    pub fn from_affine(affine: Affine) -> Self {
        let [a, b, c, d, x, y] = affine.0;
        Self([a, c, x, b, d, y, 0., 0., 1.])
    }
    /// The same map when it has no perspective.
    pub fn as_affine(self) -> Option<Affine> {
        let [a, b, c, d, e, f, g, h, i] = self.0;
        (g == 0. && h == 0. && i > 0.).then(|| Affine([a / i, d / i, b / i, e / i, c / i, f / i]))
    }
    /// The map taking `rect`'s top-left, top-right, bottom-right and bottom-left
    /// corners to `quad`. None unless the quad is convex, unfolded and finite.
    pub fn rect_to_quad(rect: Rect, quad: [Point; 4]) -> Option<Self> {
        let [width, height] = [rect.max.x - rect.min.x, rect.max.y - rect.min.y].map(f64::from);
        if !(width > 0. && height > 0. && width.is_finite() && height.is_finite()) {
            return None;
        }
        let [p0, p1, p2, p3] = quad.map(|p| [f64::from(p.x), f64::from(p.y)]);
        let delta = |axis: usize| {
            [
                p1[axis] - p2[axis],
                p3[axis] - p2[axis],
                p0[axis] - p1[axis] + p2[axis] - p3[axis],
            ]
        };
        let ([dx1, dx2, dx3], [dy1, dy2, dy3]) = (delta(0), delta(1));
        let determinant = dx1 * dy2 - dx2 * dy1;
        let (g, h) = if dx3 == 0. && dy3 == 0. {
            (0., 0.)
        } else if determinant == 0. {
            return None;
        } else {
            (
                (dx3 * dy2 - dx2 * dy3) / determinant,
                (dx1 * dy3 - dx3 * dy1) / determinant,
            )
        };
        let square = [
            p1[0] - p0[0] + g * p1[0],
            p3[0] - p0[0] + h * p3[0],
            p0[0],
            p1[1] - p0[1] + g * p1[1],
            p3[1] - p0[1] + h * p3[1],
            p0[1],
            g,
            h,
            1.,
        ];
        let [x0, y0] = [f64::from(rect.min.x), f64::from(rect.min.y)];
        let unit = [
            1. / width,
            0.,
            -x0 / width,
            0.,
            1. / height,
            -y0 / height,
            0.,
            0.,
            1.,
        ];
        let map = Self::narrow(multiply(square, unit))?;
        (map.inverse().is_some() && map.covers(rect)).then_some(map)
    }
    pub fn map(self, p: Point) -> Option<Point> {
        let [x, y, w] = self.homogeneous(p);
        let point = Point {
            x: (x / w) as f32,
            y: (y / w) as f32,
        };
        (w > 0. && point.x.is_finite() && point.y.is_finite()).then_some(point)
    }
    pub fn inverse(self) -> Option<Self> {
        Self::invert(self.wide()).and_then(Self::narrow)
    }
    /// The adjugate over the determinant, without the positive rescaling that
    /// `inverse` applies to the stored matrix.
    pub fn invert(m: [f64; 9]) -> Option<[f64; 9]> {
        let [a, b, c, d, e, f, g, h, i] = m;
        let adjugate = [
            e * i - f * h,
            c * h - b * i,
            b * f - c * e,
            f * g - d * i,
            a * i - c * g,
            c * d - a * f,
            d * h - e * g,
            b * g - a * h,
            a * e - b * d,
        ];
        let determinant = a * adjugate[0] + b * adjugate[3] + c * adjugate[6];
        (determinant != 0. && determinant.is_finite()).then(|| adjugate.map(|v| v / determinant))
    }
    /// Apply self, then next (not the reverse). None when the product is not finite.
    pub fn then(self, next: Self) -> Option<Self> {
        Self::narrow(multiply(next.wide(), self.wide()))
    }
    /// Every point of `rect` has an image, so its image is one convex,
    /// unfolded quadrilateral.
    pub fn covers(self, rect: Rect) -> bool {
        self.weight_ratio(rect).is_some_and(|ratio| ratio > MIN_WEIGHT_RATIO)
            && rect.corners().iter().all(|p| self.map(*p).is_some())
    }
    pub fn weight_ratio(self, rect: Rect) -> Option<f64> {
        if rect.is_empty() || self.0.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let weights = rect.corners().map(|p| self.homogeneous(p)[2]);
        let largest = weights.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let smallest = weights.iter().copied().fold(f64::INFINITY, f64::min);
        (smallest > 0.).then_some(smallest / largest)
    }
    /// Bounds of the mapped rectangle, when the map covers it.
    pub fn bounds(self, rect: Rect) -> Option<Rect> {
        self.covers(rect)
            .then(|| Rect::around(rect.corners().into_iter().filter_map(|p| self.map(p))))
    }

    pub fn magnification(self, bounds: Rect) -> f32 {
        if let Some(affine) = self.as_affine() { return affine.magnification(); }
        let [a,b,c,d,e,f,g,h,i] = self.0.map(f64::from);
        let mut weight = f64::INFINITY;
        let mut numerator = [0f64; 4];
        for p in bounds.corners() {
            let [x,y] = [f64::from(p.x), f64::from(p.y)];
            weight = weight.min(g*x + h*y + i);
            let n = [(a*h-b*g)*y+a*i-c*g, (d*h-e*g)*y+d*i-f*g, (b*g-a*h)*x+b*i-c*h, (e*g-d*h)*x+e*i-f*h];
            for (bound,value) in numerator.iter_mut().zip(n) { *bound = bound.max(value.abs()); }
        }
        if weight <= 0. { return f32::INFINITY; }
        let [a,b,c,d] = numerator.map(|n| (n/weight.powi(2)) as f32);
        Affine([a,b,c,d,0.,0.]).magnification()
    }
    /// Map closed polygons exactly, first clipping away the parts with no image
    /// (w' below a floor relative to their largest w'). Even-odd interiors
    /// survive the clip because it is convex. Polygons that vanish are dropped.
    pub(crate) fn map_polygons(self, polygons: &[Arc<[Point]>]) -> Vec<Arc<[Point]>> {
        let weight = |[x, y]: [f64; 2]| self.homogeneous(Point { x: x as f32, y: y as f32 })[2];
        let largest = polygons
            .iter()
            .flat_map(|ring| ring.iter())
            .map(|p| weight([p.x, p.y].map(f64::from)))
            .fold(f64::NEG_INFINITY, f64::max);
        if largest.partial_cmp(&0.) != Some(std::cmp::Ordering::Greater) {
            return Vec::new();
        }
        let floor = largest * MIN_WEIGHT_RATIO;
        polygons
            .iter()
            .filter_map(|ring| {
                let ring: Vec<_> = ring.iter().map(|p| [p.x, p.y].map(f64::from)).collect();
                let mapped: Option<Vec<Point>> = clip_convex(&ring, |p| weight(p) - floor)
                    .into_iter()
                    .map(|[x, y]| self.map(Point { x: x as f32, y: y as f32 }))
                    .collect();
                mapped.filter(|ring| ring.len() >= 3).map(Into::into)
            })
            .collect()
    }

    fn wide(self) -> [f64; 9] {
        self.0.map(f64::from)
    }
    fn homogeneous(self, p: Point) -> [f64; 3] {
        let m = self.wide();
        let [x, y] = [f64::from(p.x), f64::from(p.y)];
        [0, 3, 6].map(|row| m[row] * x + m[row + 1] * y + m[row + 2])
    }
    /// Store with a positive scale, so the sign of w' is preserved.
    fn narrow(m: [f64; 9]) -> Option<Self> {
        let scale = if m[8] > 0. {
            m[8]
        } else {
            m[6..].iter().fold(0f64, |s, v| s.max(v.abs()))
        };
        let values = m.map(|v| (v / scale) as f32);
        (scale > 0. && scale.is_finite() && values.iter().all(|v| v.is_finite()))
            .then_some(Self(values))
    }
}

fn multiply(a: [f64; 9], b: [f64; 9]) -> [f64; 9] {
    std::array::from_fn(|i| {
        let (row, column) = (i / 3, i % 3);
        (0..3).map(|k| a[row * 3 + k] * b[k * 3 + column]).sum()
    })
}

/// The part of a convex polygon where `side` is non-negative (Sutherland-Hodgman).
pub fn clip_convex(polygon: &[[f64; 2]], side: impl Fn([f64; 2]) -> f64) -> Vec<[f64; 2]> {
    let mut kept = Vec::with_capacity(polygon.len() + 2);
    for (i, p) in polygon.iter().enumerate() {
        let q = polygon[(i + 1) % polygon.len()];
        let [a, b] = [side(*p), side(q)];
        if a >= 0. {
            kept.push(*p);
        }
        if (a >= 0.) != (b >= 0.) {
            let t = a / (a - b);
            kept.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::affine::tests::{near, rect};

    #[test]
    fn affine_maps_behave_identically_as_projective_maps() {
        let source = rect(-20., 10., 300., 180.);
        let points = [
            Point::default(),
            Point {
                x: 123.5,
                y: -40.25,
            },
            Point { x: 300., y: 180. },
        ];
        for affine in [
            Affine::IDENTITY,
            Affine::translation(Point { x: 17.5, y: -3. }),
            Affine::around(
                Point { x: 40., y: 20. },
                [1.7, -0.6],
                0.8,
                Point { x: -9., y: 30. },
            ),
            Affine::around(
                Point::default(),
                [0.01, 250.],
                -2.4,
                Point { x: 1e4, y: 0. },
            ),
        ] {
            let projective = Projective::from_affine(affine);
            assert_eq!(projective.as_affine(), Some(affine));
            let inverse = affine.inverse().unwrap();
            let other = Affine::around(
                Point { x: 5., y: 6. },
                [0.5, 2.],
                0.3,
                Point { x: 2., y: 1. },
            );
            for p in points {
                near(
                    projective.map(p).unwrap(),
                    affine.map(p),
                    1e-3 * affine.map(p).x.abs().max(1.),
                );
                near(
                    projective.inverse().unwrap().map(p).unwrap(),
                    inverse.map(p),
                    1e-2,
                );
                let composed = projective.then(Projective::from_affine(other)).unwrap();
                near(
                    composed.map(p).unwrap(),
                    affine.then(other).map(p),
                    1e-2 * affine.then(other).map(p).x.abs().max(1.),
                );
            }
            let [a, b] = [projective.bounds(source).unwrap(), affine.bounds(source)];
            near(a.min, b.min, 1e-2 * b.min.x.abs().max(1.));
            near(a.max, b.max, 1e-2 * b.max.x.abs().max(1.));
        }
        let quad = [
            Point { x: 5., y: 7. },
            Point { x: 45., y: 17. },
            Point { x: 35., y: 57. },
            Point { x: -5., y: 47. },
        ];
        let parallelogram = Projective::rect_to_quad(rect(0., 0., 20., 10.), quad).unwrap();
        assert_eq!(
            &parallelogram.0[6..],
            &[0., 0., 1.],
            "a parallelogram needs no perspective"
        );
    }

    #[test]
    fn quads_map_exactly_and_invert() {
        let source = rect(100., 50., 700., 450.);
        let quad = [
            Point { x: 120., y: 80. },
            Point { x: 640., y: 30. },
            Point { x: 760., y: 520. },
            Point { x: 90., y: 430. },
        ];
        let map = Projective::rect_to_quad(source, quad).unwrap();
        for (corner, target) in source.corners().into_iter().zip(quad) {
            near(map.map(corner).unwrap(), target, 1e-3);
        }
        let inverse = map.inverse().unwrap();
        for p in [
            Point { x: 300., y: 200. },
            Point { x: 100.5, y: 449. },
            Point { x: 1000., y: -100. },
        ] {
            near(inverse.map(map.map(p).unwrap()).unwrap(), p, 2e-3);
            near(map.then(inverse).unwrap().map(p).unwrap(), p, 2e-3);
        }
        let mid = Point { x: 400., y: 250. };
        let crossing = |[a, b, c, d]: [Point; 4]| {
            let t = ((c.x - a.x) * (d.y - c.y) - (c.y - a.y) * (d.x - c.x))
                / ((b.x - a.x) * (d.y - c.y) - (b.y - a.y) * (d.x - c.x));
            Point {
                x: a.x + t * (b.x - a.x),
                y: a.y + t * (b.y - a.y),
            }
        };
        near(
            map.map(mid).unwrap(),
            crossing([quad[0], quad[2], quad[1], quad[3]]),
            1e-2,
        );
        let bounds = map.bounds(source).unwrap();
        near(bounds.min, Point { x: 90., y: 30. }, 1e-3);
        near(bounds.max, Point { x: 760., y: 520. }, 1e-3);
        let mirrored = [quad[1], quad[0], quad[3], quad[2]];
        assert!(
            Projective::rect_to_quad(source, mirrored).is_some(),
            "a flip is a valid convex quad"
        );
    }

    #[test]
    fn nonconvex_folded_and_degenerate_quads_are_refused() {
        let source = rect(0., 0., 100., 100.);
        let square = [
            Point { x: 0., y: 0. },
            Point { x: 100., y: 0. },
            Point { x: 100., y: 100. },
            Point { x: 0., y: 100. },
        ];
        assert!(Projective::rect_to_quad(source, square).is_some());
        let mut dart = square;
        dart[2] = Point { x: 30., y: 30. };
        let mut bowtie = square;
        bowtie.swap(2, 3);
        let mut collapsed = square;
        collapsed[1] = collapsed[0];
        let mut triangle = square;
        triangle[2] = Point { x: 50., y: 50. };
        for quad in [dart, bowtie, collapsed, triangle, [Point::default(); 4]] {
            assert!(Projective::rect_to_quad(source, quad).is_none(), "{quad:?}");
        }
        assert!(Projective::rect_to_quad(rect(0., 0., 0., 10.), square).is_none());
        let mut nan = square;
        nan[0].x = f32::NAN;
        assert!(Projective::rect_to_quad(source, nan).is_none());
    }

    #[test]
    fn points_beyond_the_horizon_have_no_image() {
        let source = rect(0., 0., 100., 100.);
        let quad = [
            Point { x: 40., y: 0. },
            Point { x: 60., y: 0. },
            Point { x: 100., y: 100. },
            Point { x: 0., y: 100. },
        ];
        let map = Projective::rect_to_quad(source, quad).unwrap();
        assert!(
            map.map(Point { x: 50., y: 1000. }).is_none(),
            "beyond the line that maps to infinity"
        );
        assert!(map.map(Point { x: 50., y: 120. }).is_some());
        assert!(!map.covers(rect(0., 0., 100., 1000.)));
        assert!(map.bounds(rect(0., 0., 100., 1000.)).is_none());
        let distant = map.map(Point { x: 50., y: -1e6 }).unwrap();
        near(distant, Point { x: 50., y: -25. }, 0.01);
        assert!(
            map.covers(rect(0., -1000., 100., 100.)),
            "points toward the vanishing point stay finite"
        );
        let singular = Projective([1., 0., 0., 2., 0., 0., 0., 0., 1.]);
        assert!(singular.inverse().is_none());
        assert!(Projective([f32::INFINITY; 9]).inverse().is_none());
        assert!(map.then(Projective([f32::NAN; 9])).is_none());
    }
}
