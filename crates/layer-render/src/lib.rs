//! Minimal batched contract between the shared engine and a GPU canvas renderer.
//!
//! A production renderer owns GPU texture storage and any presentation surface.
//! The shared engine submits resolved dabs and document-space damage; it never
//! sees textures, tiles, queues, fences, or presentation objects. There is no
//! host-memory raster contract.

use layer_core::{
    AssetId, BrushDeform, BrushExecution, BrushGrain, BrushRendering, BrushTip, BrushTransport,
    BrushWetMix, DualBrush, Layer, LayerId, Point, Rect, StrokeId,
};
use std::fmt;
mod outline;
pub use outline::{TipOutline, mask_outline};

/// Display-only cursor line, already transformed to logical viewport pixels.
/// Kept separate from brush dabs: cursors never touch document textures.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct CursorSegment {
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub distance: f32,
    pub marker: f32,
    pub scale: f32,
}

/// Fully resolved brush contact consumed directly by a GPU renderer.
/// Pressure curves, filtering, spacing, and randomness have already been
/// evaluated by `layer-engine`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Dab {
    pub center: Point,
    pub radii: [f32; 2],
    /// Precomputed `(cos(angle), sin(angle))` for direct vertex expansion.
    pub rotation: [f32; 2],
    /// Document-space movement since the preceding primary contact.
    pub motion: [f32; 2],
    /// Resolved straight linear color; alpha includes per-contact opacity.
    pub color_rgba_linear: [f32; 4],
    pub flow: f32,
    pub hardness: f32,
    /// Pre-resolved horizontal and vertical tip flips, each `-1` or `1`.
    pub texture_sign: [f32; 2],
    /// Resolved grain depth, pull, deposit, and deformation strength.
    pub material: [f32; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PixelFormat {
    R8Unorm,
    Rgba8Srgb,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct ViewState {
    pub width_px: u32,
    pub height_px: u32,
    /// Affine document-to-surface transform `[a, b, c, d, tx, ty]`.
    pub document_to_surface: [f32; 6],
    pub background_rgba_linear: [f32; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum DabMode {
    Paint,
    Erase,
}

/// State shared by a contiguous dab batch. Per-dab geometry remains a compact
/// instance record; color and texture identity are submitted only once.
#[derive(Clone, Debug, PartialEq)]
pub struct DabStyle {
    pub alpha_locked: bool,
    pub tip: BrushTip,
    pub mode: DabMode,
    pub execution: BrushExecution,
    pub grain: Option<BrushGrain>,
    pub dual: Option<std::sync::Arc<DualBrush>>,
    pub rendering: BrushRendering,
    pub wet_mix: BrushWetMix,
    pub transport: Option<BrushTransport>,
    pub deform: BrushDeform,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DabBatchKind {
    /// Ordered fill / destructive mask application between committed strokes.
    LayerOperation(u32),
    /// Incrementally changes the persistent active-layer image.
    Persistent,
    /// Replaces renderer-owned predicted-input preview state for this frame.
    Preview,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DabBatch {
    /// Stable stroke identity. Stateful GPU resources use this to distinguish
    /// adjacent strokes that happen to share the same brush style.
    pub stroke_id: StrokeId,
    pub layer_id: LayerId,
    pub kind: DabBatchKind,
    /// This batch contains the first committed or provisional contacts of the
    /// stroke. A batch may carry only a boundary and contain zero dabs.
    pub stroke_start: bool,
    /// The stroke ended after this batch. Renderers finalize stroke-scoped
    /// accumulation and edge work after its contacts.
    pub stroke_end: bool,
    pub first_dab: u32,
    pub dab_count: u32,
    pub style: DabStyle,
    pub damage: Rect,
}

/// Borrowed for one synchronous renderer call. A deferred renderer must copy
/// the small records it needs after return; canvas pixels remain GPU-owned.
#[derive(Clone, Copy, Debug)]
pub struct FramePacket<'a> {
    pub view: ViewState,
    /// Finite raster-canvas extent in document pixels. Renderers clip damage and
    /// storage allocation to this bound.
    pub document_extent: [u32; 2],
    /// Canonical front-to-back layer order and properties. This is borrowed
    /// directly from the document so empty and image-backed layers cannot be
    /// lost when a renderer is recreated.
    pub layers: &'a [Layer],
    pub dabs: &'a [Dab],
    pub dab_batches: &'a [DabBatch],
    /// Clear renderer-owned paint storage before applying persistent batches.
    pub reset_layers: bool,
    /// Re-composite the visible surface without re-rasterizing unchanged layers.
    pub composite_all: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct HostImage<'a> {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: PixelFormat,
    pub bytes: &'a [u8],
}

/// Completed whole-document RGBA8 readback for an explicit export request.
///
/// This owned allocation is a cold-path result and never participates in live
/// drawing or presentation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadbackImage {
    pub request_id: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bytes: Vec<u8>,
}

/// GPU command boundary implemented by the renderer owned by each platform.
///
/// `submit` consumes the borrowed frame without retaining it and enqueues GPU
/// work without waiting. Readback is explicit and never occurs implicitly in
/// live ink. Production implementations rasterize, blend, and compose into GPU
/// resources; the trait intentionally exposes no host pixel target.
pub trait CanvasRenderer {
    type Error: std::error::Error + 'static;

    /// Cached source-asset geometry for UI cursors; no GPU work or readback.
    fn tip_outline(&self, _asset: &AssetId) -> Option<&TipOutline> {
        None
    }

    fn resize_surface(&mut self, width: u32, height: u32) -> Result<(), Self::Error>;
    fn prepare_asset(&mut self, asset: &AssetId, image: HostImage<'_>) -> Result<(), Self::Error>;
    fn release_asset(&mut self, asset: &AssetId);
    fn submit(&mut self, packet: FramePacket<'_>) -> Result<(), Self::Error>;
    /// Small asynchronous UI previews, never full-resolution paint readback.
    fn request_thumbnail(&mut self, _request_id: u64, _target: LayerId) -> Result<(), Self::Error> {
        Ok(())
    }
    fn take_thumbnail(&mut self) -> Option<Result<ReadbackImage, Self::Error>> {
        None
    }
    fn request_readback(&mut self, request_id: u64) -> Result<(), Self::Error>;
    fn take_readback(&mut self) -> Option<Result<ReadbackImage, Self::Error>>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendError(pub &'static str);

impl fmt::Display for BackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for BackendError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brush_contact_layout_is_a_gpu_friendly_80_bytes() {
        assert_eq!(std::mem::size_of::<Dab>(), 80);
        assert_eq!(std::mem::align_of::<Dab>(), 4);
    }
}
