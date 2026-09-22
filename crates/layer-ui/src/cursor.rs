//! Shared GPU cursor geometry. Hosts never interpret brush math.
use crate::Camera;
use layer_core::BrushTip;
use layer_render::{CanvasRenderer, CursorSegment, Dab};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorMode {
    #[default]
    BrushSize,
    BrushSizeCross,
    Cross,
    Dot,
    None,
    Triangle,
    SinglePixelDot,
    Sight,
    BrushSizeDot,
    BrushSizeSinglePixelDot,
}
impl CursorMode {
    pub const CHOICES: &'static [(Self, &'static str)] = &[
        (Self::None, "None"),
        (Self::Cross, "Cross"),
        (Self::Triangle, "Triangle"),
        (Self::Dot, "Dot"),
        (Self::SinglePixelDot, "Single-pixel dot"),
        (Self::Sight, "Sight"),
        (Self::BrushSize, "Brush size"),
        (Self::BrushSizeCross, "Brush size and cross"),
        (Self::BrushSizeDot, "Brush size and dot"),
        (
            Self::BrushSizeSinglePixelDot,
            "Brush size and single-pixel dot",
        ),
    ];

    pub const fn has_brush_size(self) -> bool {
        matches!(
            self,
            Self::BrushSize
                | Self::BrushSizeCross
                | Self::BrushSizeDot
                | Self::BrushSizeSinglePixelDot
        )
    }

    pub const fn icon(self) -> &'static str {
        match self {
            Self::None => "cursor-none",
            Self::Cross => "cursor-cross",
            Self::Triangle => "cursor-triangle",
            Self::Dot => "cursor-dot",
            Self::SinglePixelDot => "cursor-single-pixel-dot",
            Self::Sight => "cursor-sight",
            Self::BrushSize => "cursor-brush",
            Self::BrushSizeCross => "cursor-brush-cross",
            Self::BrushSizeDot => "cursor-brush-dot",
            Self::BrushSizeSinglePixelDot => "cursor-brush-single-pixel-dot",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct CanvasCursor {
    /// Logical coordinates, independent of device pixel ratio.
    pub center: [f32; 2],
    pub mode: CursorMode,
    /// GPU presentation shared by native and Web hosts.
    pub segments: Vec<CursorSegment>,
}

#[derive(Default)]
pub(crate) struct Cursor {
    pub event: Option<layer_engine::PenEvent>,
    pub hover: layer_engine::DabGenerator,
    pub origin_ns: u64,
}
impl Cursor {
    #[allow(clippy::too_many_arguments)]
    pub fn view<R: CanvasRenderer>(
        &self,
        renderer: &R,
        tip: &BrushTip,
        dabs: &[Dab],
        camera: &Camera,
        scale: f32,
        mode: CursorMode,
        view: &mut CanvasCursor,
    ) {
        view.mode = mode;
        if let Some(event) = self.event {
            view.center = [
                event.surface_position.x / scale,
                event.surface_position.y / scale,
            ];
        }
        let [cx, cy] = view.center;
        match mode {
            CursorMode::Cross | CursorMode::BrushSizeCross => {
                view.mark([cx, cy], 5.0, 4.0, scale);
            }
            CursorMode::Dot | CursorMode::BrushSizeDot => {
                view.mark([cx, cy], 1.5, 4.0, scale);
            }
            CursorMode::SinglePixelDot | CursorMode::BrushSizeSinglePixelDot => {
                // Snap to one physical pixel, including fractional host scale.
                let x = (cx * scale).floor() / scale;
                let y = (cy * scale).floor() / scale;
                view.segments.push(CursorSegment {
                    from: [x, y],
                    to: [x + 1.0 / scale, y + 1.0 / scale],
                    distance: 0.0,
                    marker: 2.0,
                    scale: 1.0,
                });
            }
            CursorMode::Sight => {
                view.mark([cx, cy], 7.0, 5.0, scale);
            }
            CursorMode::Triangle => {
                view.segments.push(CursorSegment {
                    from: [cx, cy],
                    to: [cx + 10.0, cy + 14.0],
                    distance: 0.0,
                    marker: 3.0,
                    scale: 1.0,
                });
            }
            _ => {}
        }
        if !mode.has_brush_size() {
            return;
        }
        let [a, b, c, d, tx, ty] = camera.document_to_surface().map(|v| v / scale);
        let marker_count = view.segments.len();
        for dab in dabs {
            let [cos, sin] = dab.rotation;
            let x = dab.radii[0] * dab.texture_sign[0];
            let y = dab.radii[1] * dab.texture_sign[1];
            let transform = [
                (a * cos + c * sin) * x,
                (b * cos + d * sin) * x,
                (-a * sin + c * cos) * y,
                (-b * sin + d * cos) * y,
                a * dab.center.x + c * dab.center.y + tx,
                b * dab.center.x + d * dab.center.y + ty,
            ];
            let [a, b, c, d, tx, ty] = transform;
            match tip {
                BrushTip::AnalyticEllipse => {
                    let rx = a.hypot(b);
                    let ry = c.hypot(d);
                    // At most 0.1px chord error, independent of zoom/pressure.
                    let count = (std::f32::consts::PI * (rx.max(ry) / 0.2).sqrt())
                        .ceil()
                        .clamp(24.0, 1024.0) as usize;
                    let points = (0..count).map(|i| {
                        let (sin, cos) =
                            (i as f32 * std::f32::consts::TAU / count as f32).sin_cos();
                        [a * cos + c * sin + tx, b * cos + d * sin + ty]
                    });
                    view.contour(points, false);
                }
                BrushTip::Mask(id) => {
                    if let Some(contours) = renderer.tip_outline(id) {
                        for contour in contours {
                            let points = contour
                                .iter()
                                .map(|&[x, y]| [a * x + c * y + tx, b * x + d * y + ty]);
                            view.contour(points, false);
                        }
                    }
                }
            }
        }
        view.segments.rotate_left(marker_count);
    }
}

impl CanvasCursor {
    fn mark(&mut self, center: [f32; 2], radius: f32, marker: f32, scale: f32) {
        // Align the one-point dark stroke to physical pixels. One composite
        // primitive keeps the pale surround out of intersections and the dot.
        let width = scale.round().max(1.0);
        let center = center.map(|v| ((v * scale).floor() + (width % 2.0) * 0.5) / scale);
        self.segments.push(CursorSegment {
            from: center.map(|v| v - radius),
            to: center.map(|v| v + radius),
            distance: 0.0,
            marker,
            scale: 1.0,
        });
    }
    fn line(&mut self, from: [f32; 2], to: [f32; 2], distance: f32, marker: bool) {
        self.segments.push(CursorSegment {
            from,
            to,
            distance,
            marker: f32::from(marker),
            scale: 1.0,
        });
    }
    fn contour(&mut self, mut points: impl Iterator<Item = [f32; 2]>, marker: bool) {
        let Some(first) = points.next() else {
            return;
        };
        let mut from = first;
        let mut distance = 0.0;
        for to in points.chain(std::iter::once(first)) {
            self.line(from, to, distance, marker);
            distance += (to[0] - from[0]).hypot(to[1] - from[1]);
            from = to;
        }
    }
}
