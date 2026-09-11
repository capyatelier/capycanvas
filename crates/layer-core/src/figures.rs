//! Raster figure parameters and guide geometry; pixels are produced by the GPU.
use crate::{Point, Rect};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u32)]
#[serde(rename_all = "snake_case")]
pub enum FigureShape {
    #[default]
    Line,
    Rectangle,
    Ellipse,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u32)]
#[serde(rename_all = "snake_case")]
pub enum FigurePaint {
    #[default]
    Outline,
    Fill,
    Both,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Figure {
    pub shape: FigureShape,
    pub paint: FigurePaint,
    pub start: Point,
    pub end: Point,
    pub width: f32,
    /// Straight linear outline/foreground and interior/background colors.
    pub colors: [[f32; 4]; 2],
    pub alpha_locked: bool,
    pub erase: bool,
}

impl Figure {
    pub fn bounds(&self) -> Rect {
        let padding = 1.
            + if self.paint == FigurePaint::Fill {
                0.
            } else {
                self.width * 0.5
            };
        Rect {
            min: Point {
                x: self.start.x.min(self.end.x) - padding,
                y: self.start.y.min(self.end.y) - padding,
            },
            max: Point {
                x: self.start.x.max(self.end.x) + padding,
                y: self.start.y.max(self.end.y) + padding,
            },
        }
    }
    pub(crate) fn valid(&self) -> bool {
        let dx = (self.end.x - self.start.x).abs();
        let dy = (self.end.y - self.start.y).abs();
        [
            self.start.x,
            self.start.y,
            self.end.x,
            self.end.y,
            dx * dx + dy * dy,
        ]
        .iter()
        .all(|v| v.is_finite())
            && self.width.is_finite()
            && (0.1..=4096.).contains(&self.width)
            && self
                .colors
                .iter()
                .flatten()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            && if self.shape == FigureShape::Line {
                self.paint == FigurePaint::Outline && dx.hypot(dy) >= 0.001
            } else {
                dx >= 0.001 && dy >= 0.001
            }
    }
}
impl FigureShape {
    /// Shift constrains lines to 45-degree steps and boxes to equal sides.
    pub fn constrained_end(self, start: Point, end: Point) -> Point {
        let (dx, dy) = (end.x - start.x, end.y - start.y);
        if self == Self::Line {
            let angle =
                (dy.atan2(dx) / std::f32::consts::FRAC_PI_4).round() * std::f32::consts::FRAC_PI_4;
            let r = dx.hypot(dy);
            Point {
                x: start.x + r * angle.cos(),
                y: start.y + r * angle.sin(),
            }
        } else {
            let size = dx.abs().max(dy.abs());
            Point {
                x: start.x + size * dx.signum(),
                y: start.y + size * dy.signum(),
            }
        }
    }
    /// Display-only rubber-band outline, independent of the raster algorithm.
    pub fn guide(self, start: Point, end: Point, zoom: f32) -> Vec<Point> {
        match self {
            Self::Line => vec![start, end],
            Self::Rectangle => vec![
                start,
                Point {
                    x: end.x,
                    y: start.y,
                },
                end,
                Point {
                    x: start.x,
                    y: end.y,
                },
            ],
            Self::Ellipse => {
                let radii = [(end.x - start.x) * 0.5, (end.y - start.y) * 0.5];
                let n = (std::f32::consts::PI
                    * (radii[0].abs().max(radii[1].abs()) * zoom / 0.25).sqrt())
                .ceil()
                .clamp(16., 512.) as usize;
                (0..n)
                    .map(|i| {
                        let a = i as f32 / n as f32 * std::f32::consts::TAU;
                        Point {
                            x: (start.x + end.x) * 0.5 + radii[0] * a.cos(),
                            y: (start.y + end.y) * 0.5 + radii[1] * a.sin(),
                        }
                    })
                    .collect()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn figure() -> Figure {
        Figure {
            shape: FigureShape::Rectangle,
            paint: FigurePaint::Outline,
            start: Point { x: 20., y: 30. },
            end: Point { x: 80., y: 70. },
            width: 10.,
            colors: [[0., 0., 0., 1.]; 2],
            alpha_locked: false,
            erase: false,
        }
    }
    #[test]
    fn validation_rejects_degenerate_and_nonfinite_figures() {
        let f = figure();
        assert!(f.valid());
        for v in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut invalid = f.clone();
            invalid.start.x = v;
            assert!(!invalid.valid());
            let mut invalid = f.clone();
            invalid.width = v;
            assert!(!invalid.valid());
            let mut invalid = f.clone();
            invalid.colors[0][3] = v;
            assert!(!invalid.valid());
        }
        let mut f = f;
        f.end.x = f.start.x;
        assert!(!f.valid());
        f.shape = FigureShape::Line;
        assert!(f.valid());
        f.paint = FigurePaint::Fill;
        assert!(!f.valid());
        f.paint = FigurePaint::Outline;
        f.end = f.start;
        assert!(!f.valid());
    }
    #[test]
    fn bounds_include_outline_and_antialiasing_but_not_canvas_extent() {
        let mut f = figure();
        assert_eq!(
            f.bounds(),
            Rect {
                min: Point { x: 14., y: 24. },
                max: Point { x: 86., y: 76. }
            }
        );
        let expected = f.bounds();
        std::mem::swap(&mut f.start, &mut f.end);
        assert_eq!(f.bounds(), expected);
        f.paint = FigurePaint::Fill;
        assert_eq!(
            f.bounds(),
            Rect {
                min: Point { x: 19., y: 29. },
                max: Point { x: 81., y: 71. }
            }
        );
    }
    #[test]
    fn constraints_work_in_every_quadrant_and_guides_close_at_requested_bounds() {
        let start = Point { x: 50., y: 50. };
        for x in [-30., 30.] {
            for y in [-10., 10.] {
                for shape in [FigureShape::Rectangle, FigureShape::Ellipse] {
                    let end = shape.constrained_end(
                        start,
                        Point {
                            x: start.x + x,
                            y: start.y + y,
                        },
                    );
                    assert_eq!(
                        end,
                        Point {
                            x: 50. + x,
                            y: 50. + 30. * y.signum()
                        }
                    );
                    let guide = shape.guide(start, end, 1.);
                    assert!(guide.len() >= 4 && guide.len() <= 512);
                    assert!(guide.iter().all(|p| p.x >= start.x.min(end.x)
                        && p.x <= start.x.max(end.x)
                        && p.y >= start.y.min(end.y)
                        && p.y <= start.y.max(end.y)));
                }
                let end = FigureShape::Line.constrained_end(
                    start,
                    Point {
                        x: 50. + x,
                        y: 50. + y,
                    },
                );
                assert!((end.y - 50.).abs() < 0.001);
                assert!(((end.x - 50.).abs() - x.hypot(y)).abs() < 0.001);
            }
        }
    }
}
