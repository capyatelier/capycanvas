//! Incremental selection painting uses resolved brush contacts, never a CPU raster.
use crate::{Dab, DabStyle};
use layer_core::Selection;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionPaintMode {
    Add,
    Subtract,
    Gray,
}

/// Repeated updates with the same ID accumulate one gesture. Finish captures a
/// single immutable result; cancellation drops provisional coverage. The caller
/// drains that result before starting a gesture based on the resulting selection.
#[derive(Clone, Debug)]
pub struct SelectionPaint {
    pub id: u64,
    pub before: Arc<Selection>,
    pub mode: SelectionPaintMode,
    pub opacity: f32,
    pub gray: f32,
    pub style: DabStyle,
    pub gradient: Option<SelectionGradient>,
    pub dabs: Vec<Dab>,
    /// Enclosed Selection Brush area or an explicitly filled mask region.
    pub enclosed: Option<Arc<Selection>>,
    pub finish: bool,
    /// Replace an already displayed provisional footprint (final end taper).
    pub restart: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct SelectionGradient {
    pub start: layer_core::Point,
    pub end: layer_core::Point,
    pub background: f32,
    pub radial: bool,
    pub transparent: bool,
}
impl SelectionPaint {
    pub fn is_valid(&self) -> bool {
        [self.opacity, self.gray]
            .into_iter()
            .all(|v| v.is_finite() && (0. ..=1.).contains(&v))
            && self.before.validate().is_ok()
            && self.enclosed.as_ref().is_none_or(|s| s.validate().is_ok())
            && self.gradient.is_none_or(|g| {
                [g.start.x, g.start.y, g.end.x, g.end.y]
                    .into_iter()
                    .all(f32::is_finite)
                    && g.background.is_finite()
                    && (0. ..=1.).contains(&g.background)
            })
            && self.style.execution == layer_core::BrushExecution::Dry
            && self.style.brush_to_layer == layer_core::Affine::IDENTITY
            && self.style.selection.is_none()
            && self.dabs.iter().all(|d| {
                d.center.x.is_finite()
                    && d.center.y.is_finite()
                    && d.radii.iter().all(|v| v.is_finite() && *v > 0.)
                    && d.rotation
                        .iter()
                        .chain(&d.texture_sign)
                        .chain(&d.motion)
                        .chain(&d.previous)
                        .chain(&d.material)
                        .chain(&d.contact)
                        .chain(&d.previous_contact)
                        .all(|v| v.is_finite())
                    && [d.flow, d.hardness, d.color_rgba_linear[3]]
                        .into_iter()
                        .all(|v| v.is_finite() && (0. ..=1.).contains(&v))
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionOverlay {
    pub active: bool,
    pub editing: Option<layer_core::LayerId>,
    /// Display-encoded color; independent of artwork space and mask strength.
    pub color: [f32; 4],
    pub protected: bool,
}

#[derive(Clone, Debug)]
pub struct SelectionPaintResult {
    pub request_id: u64,
    pub pixels: Arc<layer_core::SelectionPixels>,
    /// Compared against normalized starting coverage on the GPU. An unchanged
    /// gesture preserves both the original representation and its undo history.
    pub changed: bool,
}
