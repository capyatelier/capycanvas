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
mod telemetry;
pub use outline::{TipOutline, mask_outline};
pub use telemetry::{RendererTelemetry, TimingSamples};

/// Display-only overlay primitive in logical viewport pixels.
/// Kept separate from brush dabs: cursors never touch document textures.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct CursorSegment {
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub distance: f32,
    /// 0: dashed line, 1: solid line, 2: filled rectangular handle (`from`/`to`
    /// are opposite corners). Handles have a one-pixel contrasting border.
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

pub use layer_core::ProjectAssetFormat as PixelFormat;

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
    pub selection: Option<std::sync::Arc<layer_core::Selection>>,
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
    /// Material-update identity within a stroke. Replay can contain several
    /// updates in one packet; live rendering need not submit extra GPU work.
    pub material_update: u32,
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
    /// Monotonic seconds since this editor session started; never wall time.
    pub time_seconds: f32,
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

/// Small idle-time picker request; paint stays in renderer-owned GPU storage.
#[derive(Clone, Debug)]
pub struct FilterPreviewRequest {
    pub request_id: u64,
    pub target: LayerId,
    pub size: [u32; 2],
    pub extent: [u32; 2],
    pub view: ViewState,
    pub layers: Vec<Layer>,
    pub filters: Vec<std::sync::Arc<layer_core::EffectInstance>>,
}

/// Rows of equal-sized previews packed vertically in a single small image.
#[derive(Clone, Debug)]
pub struct FilterPreviewImage {
    pub image: ReadbackImage,
    pub filters: Vec<std::sync::Arc<str>>,
}

/// Cold-path validation, separate from painting. `programs` are changed
/// definitions; `namespace` includes programs they may be composed alongside.
#[derive(Clone, Debug)]
pub struct EffectValidationRequest {
    pub request_id: u64,
    pub programs: Vec<std::sync::Arc<layer_core::EffectProgram>>,
    pub namespace: Vec<std::sync::Arc<layer_core::EffectProgram>>,
}
#[derive(Clone, Debug)]
pub struct EffectValidationResult {
    pub request_id: u64,
    pub result: Result<(), String>,
}

/// Small cached canvas overview; no image means the requested revision is current.
pub struct CanvasPreview {
    pub revision: u64,
    pub image: Option<ReadbackImage>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSampleSource {
    Composite,
    /// Raw paint color, before layer opacity, masks and clipping.
    Layer(LayerId),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorSampleRequest {
    pub request_id: u64,
    pub source: ColorSampleSource,
    /// Document coordinates for Composite, layer-local coordinates for Layer.
    pub position: [u32; 2],
}
#[derive(Clone, Copy, Debug)]
pub struct ColorSample {
    pub request_id: u64,
    /// Straight linear RGBA. Alpha zero means no paint at the requested point.
    pub rgba: [f32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub enum RegionSource {
    Composite,
    /// Raw paint, in layer-local coordinates.
    Layer(LayerId),
    /// Composition snapshot with original indices and selected visibility.
    Layers(Vec<Layer>),
}
#[derive(Clone, Debug)]
pub struct RegionRequest {
    pub request_id: u64,
    pub source: RegionSource,
    pub position: [u32; 2],
    pub tolerance: f32,
    /// Optional limit, expressed in the source's coordinates.
    pub limit: Option<std::sync::Arc<layer_core::Selection>>,
}
#[derive(Clone, Debug)]
pub struct RegionResult {
    pub request_id: u64,
    /// Immutable, GPU-generated mask in the source's coordinates.
    pub pixels: std::sync::Arc<layer_core::SelectionPixels>,
}

/// Absolute, disposable transform of one immutable layer-local source. Keep the
/// transaction id and selection fixed while dragging; a new id captures anew.
#[derive(Clone, Debug, PartialEq)]
pub struct TransformPreview {
    pub transaction: u64,
    pub layer: LayerId,
    pub selection: Option<layer_core::Selection>,
    pub transform: layer_core::ImageTransform,
}
impl TransformPreview {
    /// A linked paint/mask pair shares one world-space transform, but each has
    /// its own local origin and immutable selection. Other layer kinds have no
    /// raster pigment target to transform alongside their mask.
    pub fn companion(&self, layers: &[Layer]) -> Option<Self> {
        let owner = layers
            .iter()
            .find(|l| l.id == self.layer || l.mask.as_ref().is_some_and(|m| m.id == self.layer))?;
        let mask = owner.mask.as_ref().filter(|m| m.linked)?;
        if owner.kind != layer_core::LayerKind::Paint {
            return None;
        }
        let target = if self.layer == owner.id {
            mask.id
        } else {
            owner.id
        };
        let a = layer_core::target_offset(layers, self.layer);
        let b = layer_core::target_offset(layers, target);
        let delta = layer_core::Point {
            x: a.x - b.x,
            y: a.y - b.y,
        };
        let to = layer_core::Affine::translation(delta);
        let from = layer_core::Affine::translation(layer_core::Point {
            x: -delta.x,
            y: -delta.y,
        });
        Some(Self {
            layer: target,
            selection: self.selection.as_ref().map(|s| s.translated(delta)),
            transform: layer_core::ImageTransform {
                affine: from.then(self.transform.affine).then(to),
                ..self.transform
            },
            ..self.clone()
        })
    }
}

/// GPU command boundary implemented by the renderer owned by each platform.
///
/// `submit` consumes the borrowed frame without retaining it and enqueues GPU
/// work without waiting. Readback is explicit and never occurs implicitly in
/// live ink. Production implementations rasterize, blend, and compose into GPU
/// resources; the trait intentionally exposes no host pixel target.
pub trait CanvasRenderer {
    type Error: std::error::Error + 'static;
    /// Applied by the next submit. None restores the captured original before
    /// subsequent paint/operations. This performs no readback or blocking wait.
    fn set_transform_preview(
        &mut self,
        _preview: Option<&TransformPreview>,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    /// Display-only selection. It never changes paint, export or sampling input.
    fn set_selection_outline(
        &mut self,
        _selection: Option<&layer_core::Selection>,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    fn set_telemetry_enabled(&mut self, _enabled: bool) {}
    fn telemetry(&self) -> RendererTelemetry {
        RendererTelemetry::default()
    }
    fn request_effect_validation(
        &mut self,
        _request: EffectValidationRequest,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }
    fn take_effect_validation(&mut self) -> Option<EffectValidationResult> {
        None
    }

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
    /// Bounded document overview, sampled from the current GPU composition.
    /// An accepted request always produces a reply; unchanged revisions carry
    /// no image and perform no GPU work. Only one request may be in flight.
    fn request_canvas_preview(
        &mut self,
        _known_revision: Option<u64>,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }
    fn take_canvas_preview(&mut self) -> Option<Result<CanvasPreview, Self::Error>> {
        None
    }
    /// One texel, asynchronous and single-flight. Does not recomposite the scene.
    fn request_color_sample(&mut self, _request: ColorSampleRequest) -> Result<bool, Self::Error> {
        Ok(false)
    }
    fn take_color_sample(&mut self) -> Option<Result<ColorSample, Self::Error>> {
        None
    }
    /// Single-flight connected region. GPU work is asynchronous; the result is
    /// retained once in host memory for history/recovery, not rasterized there.
    fn request_region(&mut self, _request: RegionRequest) -> Result<bool, Self::Error> {
        Ok(false)
    }
    fn take_region(&mut self) -> Option<Result<RegionResult, Self::Error>> {
        None
    }
    fn request_filter_previews(
        &mut self,
        _request: FilterPreviewRequest,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }
    fn take_filter_previews(&mut self) -> Option<Result<FilterPreviewImage, Self::Error>> {
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
    fn linked_transform_maps_both_origins_into_the_same_world_motion() {
        use layer_core::{Affine, ImageTransform, LayerMask, Point, Selection};
        let mut parent = Layer::paint(LayerId(3), "parent");
        parent.kind = layer_core::LayerKind::Group;
        parent.properties.offset = Point { x: 19., y: -11. };
        let mut paint = Layer::paint(LayerId(1), "paint");
        paint.properties.parent = Some(parent.id);
        paint.properties.offset = Point { x: 12., y: 8. };
        paint.mask = Some(LayerMask::reveal_all(LayerId(9), Point { x: -7., y: 23. }));
        let mut layers = vec![paint, parent];
        let selection = Selection::polygon(vec![
            Point { x: 0., y: 0. },
            Point { x: 12., y: 0. },
            Point { x: 6., y: 8. },
        ])
        .unwrap();
        for primary in [LayerId(1), LayerId(9)] {
            let request = TransformPreview {
                transaction: 7,
                layer: primary,
                selection: Some(selection.clone()),
                transform: ImageTransform {
                    affine: Affine::around(
                        Point { x: 44., y: 12. },
                        [-1.3, 0.7],
                        0.6,
                        Point { x: 2., y: -6. },
                    ),
                    ..Default::default()
                },
            };
            let other = request.companion(&layers).unwrap();
            assert_ne!(other.layer, primary);
            let a = layer_core::target_offset(&layers, primary);
            let b = layer_core::target_offset(&layers, other.layer);
            let delta = Point {
                x: a.x - b.x,
                y: a.y - b.y,
            };
            assert_eq!(other.selection, Some(selection.translated(delta)));
            for point in [Point::default(), Point { x: 50., y: 90. }] {
                let p = request.transform.affine.map(point);
                let q = other.transform.affine.map(Point {
                    x: point.x + delta.x,
                    y: point.y + delta.y,
                });
                assert!((p.x + a.x - q.x - b.x).abs() < 0.0001);
                assert!((p.y + a.y - q.y - b.y).abs() < 0.0001);
            }
        }
        layers[0].mask.as_mut().unwrap().linked = false;
        let request = TransformPreview {
            transaction: 1,
            layer: LayerId(1),
            selection: None,
            transform: Default::default(),
        };
        assert!(request.companion(&layers).is_none());
    }

    #[test]
    fn brush_contact_layout_is_a_gpu_friendly_80_bytes() {
        assert_eq!(std::mem::size_of::<Dab>(), 80);
        assert_eq!(std::mem::align_of::<Dab>(), 4);
    }
}
