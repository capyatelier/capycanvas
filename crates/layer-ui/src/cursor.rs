//! Shared cursor presentation. Hosts paint vectors, never interpret brush math.
use crate::Camera;
use layer_core::BrushTip;
use layer_render::{CanvasRenderer, CursorSegment, Dab};
use serde::{Deserialize, Serialize};
use std::fmt::Write;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorMode {
    #[default]
    BrushSize,
    BrushSizeCross,
    Cross,
    Dot,
    None,
}
impl CursorMode {
    pub const CHOICES: &'static [(Self, &'static str)] = &[
        (Self::BrushSize, "Brush size"),
        (Self::BrushSizeCross, "Brush size + cross"),
        (Self::Cross, "Cross"),
        (Self::Dot, "Dot"),
        (Self::None, "None"),
    ];
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CanvasCursor {
    /// Logical coordinates, independent of device pixel ratio.
    pub center: [f32; 2],
    pub mode: CursorMode,
    /// SVG path syntax for the web overlay.
    /// Coordinates are already transformed, keeping the stroke a thin 1px.
    pub outline: String,
    pub marker: String,
    /// Native GPU presentation; web consumes the equivalent SVG paths above.
    #[serde(skip)]
    pub segments: Vec<CursorSegment>,
}

#[derive(Default)]
pub(crate) struct Cursor {
    pub event: Option<layer_engine::PenEvent>,
    pub hover: layer_engine::DabGenerator,
    pub origin_ns: u64,
}
impl Cursor {
    pub fn view<R: CanvasRenderer>(
        &self,
        renderer: &R,
        tip: &BrushTip,
        dabs: &[Dab],
        camera: &Camera,
        scale: f32,
        mode: CursorMode,
    ) -> CanvasCursor {
        let mut view = CanvasCursor {
            mode,
            ..CanvasCursor::default()
        };
        if let Some(event) = self.event {
            view.center = [
                event.surface_position.x / scale,
                event.surface_position.y / scale,
            ];
        }
        let [cx, cy] = view.center;
        view.marker = match mode {
            CursorMode::Cross | CursorMode::BrushSizeCross => {
                format!("M{} {}h12M{} {}v12", cx - 6.0, cy, cx, cy - 6.0)
            }
            CursorMode::Dot => format!("M{} {}h2v2h-2Z", cx - 1.0, cy - 1.0),
            _ => String::new(),
        };
        match mode {
            CursorMode::Cross | CursorMode::BrushSizeCross => {
                view.line([cx - 6.0, cy], [cx + 6.0, cy], 0.0, true);
                view.line([cx, cy - 6.0], [cx, cy + 6.0], 0.0, true);
            }
            CursorMode::Dot => {
                view.contour(
                    &[
                        [cx - 1.0, cy - 1.0],
                        [cx + 1.0, cy - 1.0],
                        [cx + 1.0, cy + 1.0],
                        [cx - 1.0, cy + 1.0],
                    ],
                    true,
                );
            }
            _ => {}
        }
        if !matches!(mode, CursorMode::BrushSize | CursorMode::BrushSizeCross) {
            return view;
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
                    let angle = b.atan2(a).to_degrees();
                    // At most 0.1px chord error, independent of zoom/pressure.
                    let count = (std::f32::consts::PI * (rx.max(ry) / 0.2).sqrt())
                        .ceil()
                        .clamp(24.0, 1024.0) as usize;
                    let points: Vec<_> = (0..count)
                        .map(|i| {
                            let (sin, cos) =
                                (i as f32 * std::f32::consts::TAU / count as f32).sin_cos();
                            [a * cos + c * sin + tx, b * cos + d * sin + ty]
                        })
                        .collect();
                    view.contour(&points, false);
                    let _ = write!(
                        view.outline,
                        "M{} {}A{rx} {ry} {angle} 1 1 {} {}A{rx} {ry} {angle} 1 1 {} {}Z",
                        tx + a,
                        ty + b,
                        tx - a,
                        ty - b,
                        tx + a,
                        ty + b
                    );
                }
                BrushTip::Mask(id) => {
                    if let Some(contours) = renderer.tip_outline(id) {
                        for contour in contours {
                            let points: Vec<_> = contour
                                .iter()
                                .map(|&[x, y]| [a * x + c * y + tx, b * x + d * y + ty])
                                .collect();
                            view.contour(&points, false);
                            for (i, &[x, y]) in contour.iter().enumerate() {
                                let _ = write!(
                                    view.outline,
                                    "{}{:.2} {:.2}",
                                    if i == 0 { 'M' } else { 'L' },
                                    a * x + c * y + tx,
                                    b * x + d * y + ty
                                );
                            }
                            view.outline.push('Z');
                        }
                    }
                }
            }
        }
        view.segments.rotate_left(marker_count);
        view
    }
}

impl CanvasCursor {
    fn line(&mut self, from: [f32; 2], to: [f32; 2], distance: f32, marker: bool) {
        self.segments.push(CursorSegment {
            from,
            to,
            distance,
            marker: f32::from(marker),
            scale: 1.0,
        });
    }
    fn contour(&mut self, points: &[[f32; 2]], marker: bool) {
        let mut distance = 0.0;
        for (from, to) in points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .take(points.len())
        {
            self.line(*from, *to, distance, marker);
            distance += (to[0] - from[0]).hypot(to[1] - from[1]);
        }
    }
}
