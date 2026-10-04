//! Portable, renderer-agnostic artwork and editor state.
//!
//! This crate contains no window, graphics API, inference runtime, async
//! executor, or window types. Immutable raster revisions back committed edits,
//! undo/redo and project snapshots. Live contacts retain bounded shared samples.

#[cfg(unix)]
mod atomic_file;
#[cfg(unix)]
pub use atomic_file::{atomic_write, atomic_write_checked};
mod cancellable;
pub use cancellable::Cancellable;

use std::collections::BTreeSet;
pub mod color;
pub mod authored;
pub mod package;
pub mod binary_payload;
mod image_metadata;
pub use image_metadata::{ImageResolution, PhotoMetadata, ResolutionUnit};
mod contact;
pub use contact::{BrushBristles, BrushContact};
mod contact_presets;
pub use contact_presets::CONTACT_BRUSH_PRESETS;
mod effect_catalog;
mod effects;
mod gradient;
pub mod lut3d;
pub use lut3d::Lut3d;
#[cfg(test)]
mod lut3d_tests;
pub mod raster;
pub mod raster_storage;
pub use effect_catalog::*;
mod layers;
mod retouch;
pub use retouch::{CloneSource, Retouch, RetouchSource};
mod selection;
pub mod tonal;
pub mod levels;
pub mod curves;
#[cfg(test)]
mod curves_tests;
#[cfg(test)]
mod levels_tests;
#[cfg(test)]
mod color_adjustment_schema_tests;
pub use selection::*;
pub use effects::*;
pub use gradient::*;
mod presets;
pub use layers::*;
mod figures;
pub use figures::{Figure, FigurePaint, FigureShape, ellipse_outline};
mod rulers;
pub use rulers::{Ruler, RulerConstraint, RulerGeometry, RulerKind, choose_ruler};
mod affine;
pub use affine::{Affine, ImageTransform, Interpolation, LayerPlacement};
mod projective;
pub use projective::{Projective, clip_convex};
mod warp;
pub use warp::{MeshMap, Tessellation};
mod project;
mod transform_pixels;
pub use transform_pixels::{TransformPixelsPlan, TransformPixelsRefusal, TransformPixelsScope};
mod canvas_geometry;
pub use canvas_geometry::{CanvasGeometry, CanvasGeometryError, CanvasGeometryPlan, CanvasRect, GeometryLimits, ImageOrientation};
mod content_bounds;
pub use content_bounds::{ContentBoundsCache, ContentBoundsRequest, ContentScope};
mod artwork_query;
pub use artwork_query::{ARTWORK_SAMPLE_WIDTHS, ArtworkSample, ArtworkSampleRequest, ArtworkQuery, ArtworkStatisticsRequest, ArtworkSource, SnapshotSource, EffectInputKey, white_balance_neutral};
#[cfg(test)]
mod operation_test_support;
#[cfg(test)]
mod artwork_query_tests;
mod merge;
pub use merge::{MergeDown, MergeKind, MergePlan, MergeRefusal};
mod retouch_layers;
pub use retouch_layers::{RetouchLayerPlan, RetouchLayerRefusal, SeparationFilters};
mod history_budget;
mod color_edit;
mod color_history;
pub use color_history::{ColorTransition, PreparedColorTransition};
pub use project::{ProjectAsset, ProjectAssetFormat, ProjectLimits};

pub use presets::{
    CONTACT_PAPER_TEXTURE_ASSET, DefaultBrushPreset,
    PAPER_GRAIN_TEXTURE_ASSET,
    WATERCOLOR_TIP_TEXTURE_ASSET, WATERCOLOR_TRANSPORT_LONG_BROAD_ASSET,
    WATERCOLOR_TRANSPORT_LONG_NARROW_ASSET, WATERCOLOR_TRANSPORT_SHORT_BROAD_ASSET,
    WATERCOLOR_TRANSPORT_SHORT_NARROW_ASSET, default_brush,
};

use std::{
    collections::BTreeMap,
    fmt,
    sync::Arc,
};

pub type Revision = u64;

/// Largest canvas or layer extent, in pixels per side.
pub const MAX_EXTENT: u32 = 32768;

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct StrokeId(pub u64);

#[derive(
    Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub struct AssetId(pub Arc<str>);

impl From<&str> for AssetId {
    fn from(value: &str) -> Self {
        Self(Arc::from(value))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub struct Rect {
    pub min: Point,
    pub max: Point,
}

impl Rect {
    #[inline]
    pub fn from_extent(extent: [u32; 2]) -> Self {
        Self { min: Point::default(), max: Point { x: extent[0] as f32, y: extent[1] as f32 } }
    }

    pub const EMPTY: Self = Self {
        min: Point {
            x: f32::INFINITY,
            y: f32::INFINITY,
        },
        max: Point {
            x: f32::NEG_INFINITY,
            y: f32::NEG_INFINITY,
        },
    };

    pub const UNBOUNDED: Self = Self {
        min: Point {
            x: f32::NEG_INFINITY,
            y: f32::NEG_INFINITY,
        },
        max: Point {
            x: f32::INFINITY,
            y: f32::INFINITY,
        },
    };

    pub fn is_empty(self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y
    }

    pub fn include_circle(&mut self, center: Point, radius: f32) {
        self.min.x = self.min.x.min(center.x - radius);
        self.min.y = self.min.y.min(center.y - radius);
        self.max.x = self.max.x.max(center.x + radius);
        self.max.y = self.max.y.max(center.y + radius);
    }

    pub fn around(points: impl IntoIterator<Item = Point>) -> Self {
        let mut bounds = Self::EMPTY;
        for p in points {
            bounds.include_circle(p, 0.);
        }
        bounds
    }

    pub fn outset(self, amount: f32) -> Self {
        Self {
            min: Point {
                x: self.min.x - amount,
                y: self.min.y - amount,
            },
            max: Point {
                x: self.max.x + amount,
                y: self.max.y + amount,
            },
        }
    }

    /// Top-left, top-right, bottom-right, then bottom-left.
    pub fn corners(self) -> [Point; 4] {
        [
            self.min,
            Point {
                x: self.max.x,
                y: self.min.y,
            },
            self.max,
            Point {
                x: self.min.x,
                y: self.max.y,
            },
        ]
    }

    /// The overlap of both, or empty when they don't overlap.
    pub fn intersect(self, other: Self) -> Self {
        let overlap = Self {
            min: Point { x: self.min.x.max(other.min.x), y: self.min.y.max(other.min.y) },
            max: Point { x: self.max.x.min(other.max.x), y: self.max.y.min(other.max.y) },
        };
        if overlap.is_empty() { Self::EMPTY } else { overlap }
    }

    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        Self {
            min: Point {
                x: self.min.x.min(other.min.x),
                y: self.min.y.min(other.min.y),
            },
            max: Point {
                x: self.max.x.max(other.max.x),
                y: self.max.y.max(other.max.y),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum LayerKind {
    Paint,
    Group,
    Effect,
    /// Named reusable coverage; never participates in artwork composition.
    Selection,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub struct StrokePoint {
    pub position: Point,
    /// Normalized device pressure in the inclusive range `0..=1`.
    pub pressure: f32,
    /// Normalized pen tilt vector. Zero when unavailable.
    pub tilt: [f32; 2],
    /// Barrel rotation in radians. Zero when unavailable.
    pub twist: f32,
    /// Time since stroke start. Platform timestamps are normalized at input.
    pub elapsed_micros: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum StrokeTool {
    Brush,
    Eraser,
}

pub const BRUSH_CURVE_SAMPLES: usize = 16;
pub const MAX_BRUSH_DIAMETER: f32 = 32_768.0;
pub const MAX_BRUSH_MAPPINGS: usize = 32;
pub const MAX_BRUSH_SCATTER_DIAMETERS: f32 = 16.0;
pub const MAX_BRUSH_STAMP_COUNT: u8 = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum BrushSensor {
    Pressure,
    Speed,
    Direction,
    TiltMagnitude,
    TiltDirection,
    Twist,
    StrokeDistance,
    StrokeTime,
    Random,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum BrushTarget {
    Diameter,
    Opacity,
    Flow,
    Hardness,
    Spacing,
    Aspect,
    Rotation,
    ScatterAlong,
    ScatterAcross,
    Hue,
    Saturation,
    Lightness,
    SecondaryColor,
    GrainDepth,
    Pull,
    Deposit,
    DeformStrength,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum BrushCombine {
    Replace,
    Add,
    Multiply,
}

/// A compact response curve sampled uniformly over `0..=1`.
///
/// Preset editors may expose arbitrary control points, but compile them to this
/// bounded representation before a stroke begins. It is cheap to evaluate on
/// the CPU before resolved dabs are submitted to the renderer.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub struct BrushCurve {
    pub samples: [f32; BRUSH_CURVE_SAMPLES],
}

impl BrushCurve {
    pub const LINEAR: Self = Self {
        samples: [
            0.0,
            1.0 / 15.0,
            2.0 / 15.0,
            3.0 / 15.0,
            4.0 / 15.0,
            5.0 / 15.0,
            6.0 / 15.0,
            7.0 / 15.0,
            8.0 / 15.0,
            9.0 / 15.0,
            10.0 / 15.0,
            11.0 / 15.0,
            12.0 / 15.0,
            13.0 / 15.0,
            14.0 / 15.0,
            1.0,
        ],
    };

    pub fn sample(self, input: f32) -> f32 {
        let position = input.clamp(0.0, 1.0) * (BRUSH_CURVE_SAMPLES - 1) as f32;
        let lower = position.floor() as usize;
        let upper = (lower + 1).min(BRUSH_CURVE_SAMPLES - 1);
        let fraction = position - lower as f32;
        self.samples[lower] + (self.samples[upper] - self.samples[lower]) * fraction
    }
}

/// One bounded instruction in the brush dynamics program.
///
/// Sensor values are normalized from `input_min..input_max`; the curve is then
/// sampled and transformed by `output = curve * scale + bias` before combining
/// it with the selected target register. Mappings are evaluated in order.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushMapping {
    pub sensor: BrushSensor,
    pub target: BrushTarget,
    pub combine: BrushCombine,
    pub input_min: f32,
    pub input_max: f32,
    pub output_scale: f32,
    pub output_bias: f32,
    pub curve: BrushCurve,
}

impl BrushMapping {
    pub const fn pressure_size() -> Self {
        Self {
            sensor: BrushSensor::Pressure,
            target: BrushTarget::Diameter,
            combine: BrushCombine::Multiply,
            input_min: 0.0,
            input_max: 1.0,
            output_scale: 0.85,
            output_bias: 0.15,
            curve: BrushCurve::LINEAR,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum BrushTip {
    AnalyticEllipse,
    /// Content-addressed single-channel mask. The render backend prepares it
    /// as an R8 texture before a frame references the brush.
    Mask(AssetId),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum BrushExecution {
    #[default]
    Dry,
    Smudge,
    Wet,
    /// Layer-wide wet watercolor. Pigment interacts only with color already
    /// present on the same paint layer; its live edge is derived at composite
    /// time rather than baked into layer pixels.
    Watercolor,
    Liquify,
    /// Copies the stroke's retouching source through its offset instead of
    /// laying down a color.
    Clone,
    /// Clones while the pen is down; at pen-up the copy takes on the tone and
    /// color around the stroke.
    Heal,
    /// Tints while the pen is down; at pen-up the stroke is replaced with
    /// nearby pixels healed into the stroke's surroundings.
    SpotHeal,
}

impl BrushExecution {
    /// Retouching executions paint from the stroke's source, not a color.
    pub fn retouches(self) -> bool {
        matches!(self, Self::Clone | Self::Heal | Self::SpotHeal)
    }

    /// Retouching executions that copy through an offset from a source point.
    pub fn copies_from_source(self) -> bool {
        matches!(self, Self::Clone | Self::Heal)
    }

    /// Executions blended into the stroke's surroundings at pen-up.
    pub fn heals(self) -> bool {
        matches!(self, Self::Heal | Self::SpotHeal)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum BrushBlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Add,
    Subtract,
    Darken,
    Lighten,
    Overlay,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum BrushAccumulation {
    #[default]
    Flow,
    Uniform,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum BrushGrainBehavior {
    #[default]
    Moving,
    Canvas,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum ColorMixSpace {
    #[default]
    LinearRgb,
    Oklab,
    /// Encoded values under the document's transfer curve, as Clip Studio Paint mixes.
    Classic,
}

impl ColorMixSpace {
    pub const ALL: [Self; 3] = [Self::LinearRgb, Self::Oklab, Self::Classic];
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum LiquifyMode {
    #[default]
    Push,
    /// Turns content counterclockwise on the y-down canvas. The name follows
    /// the rotation applied to the sampled coordinate.
    TwirlClockwise,
    /// Turns content clockwise on the y-down canvas.
    TwirlCounterClockwise,
    /// Shrinks content toward the dab centre.
    Pinch,
    /// Bulges content away from the dab centre.
    Expand,
    /// Scatters content in 4 px cells by up to `12 * distortion` pixels.
    Crystals,
    Edge,
    Reconstruct,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushPath {
    /// Random spacing deviation as a fraction of resolved spacing.
    pub spacing_jitter: f32,
    /// Base along/across scatter in resolved brush diameters.
    pub jitter_along: f32,
    pub jitter_across: f32,
    /// Fade distance in brush diameters. Zero disables falloff.
    pub falloff_distance: f32,
    /// Stationary contact rate. Zero disables continuous spraying.
    pub continuous_rate_hz: f32,
}

impl Default for BrushPath {
    fn default() -> Self {
        Self {
            spacing_jitter: 0.0,
            jitter_along: 0.0,
            jitter_across: 0.0,
            falloff_distance: 0.0,
            continuous_rate_hz: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushStabilization {
    pub streamline: f32,
    pub pressure_smoothing: f32,
    /// Minimum time for modeled pressure to fall by one full unit. Its fall
    /// velocity eases with a 4 ms response to smooth repeated sensor values.
    /// Zero disables the limiter. Uses input time, independent of zoom and DPI.
    pub pressure_fall_micros: u32,
    pub stabilization: f32,
    pub motion_filtering: f32,
    pub expression: f32,
}

impl Default for BrushStabilization {
    fn default() -> Self {
        Self {
            streamline: 0.0,
            pressure_smoothing: 0.0,
            pressure_fall_micros: 0,
            stabilization: 0.0,
            motion_filtering: 0.0,
            expression: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushTaper {
    pub start_distance_diameters: f32,
    pub end_distance_diameters: f32,
    pub start_size: f32,
    pub end_size: f32,
    pub start_opacity: f32,
    pub end_opacity: f32,
    /// Shape of the taper: 1 preserves the smooth envelope; higher values
    /// narrow more quickly toward the endpoint. Independent of taper length.
    pub tip_sharpness: f32,
}

impl Default for BrushTaper {
    fn default() -> Self {
        Self {
            start_distance_diameters: 0.0,
            end_distance_diameters: 0.0,
            start_size: 1.0,
            end_size: 1.0,
            start_opacity: 1.0,
            end_opacity: 1.0,
            tip_sharpness: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushShape {
    pub count: u8,
    pub count_jitter: f32,
    /// Symmetric fractional radius variation applied independently to every
    /// contact. A value of 0.1 resolves to a scale in [0.9, 1.1].
    pub size_jitter: f32,
    /// Fraction of one full turn applied randomly per contact.
    pub rotation_jitter: f32,
    pub follow_direction: f32,
    pub follow_tilt: f32,
    pub follow_twist: f32,
    pub flip_x_probability: f32,
    pub flip_y_probability: f32,
}

impl Default for BrushShape {
    fn default() -> Self {
        Self {
            count: 1,
            count_jitter: 0.0,
            size_jitter: 0.0,
            rotation_jitter: 0.0,
            follow_direction: 0.0,
            follow_tilt: 0.0,
            follow_twist: 0.0,
            flip_x_probability: 0.0,
            flip_y_probability: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushGrain {
    pub asset: AssetId,
    pub behavior: BrushGrainBehavior,
    /// Repeats per brush diameter for moving grain, or per 256 document pixels
    /// for canvas grain.
    pub scale: f32,
    pub depth: f32,
    pub rotation_radians: f32,
    pub offset_jitter: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushColorDynamics {
    /// Straight linear document RGB and independent coverage alpha. Preset
    /// owners convert profiled colors into document coordinates before use.
    pub secondary_color_rgba_linear: [f32; 4],
    pub stamp_hue_jitter: f32,
    pub stamp_saturation_jitter: f32,
    pub stamp_lightness_jitter: f32,
    pub stamp_secondary_jitter: f32,
    pub stroke_hue_jitter: f32,
    pub stroke_saturation_jitter: f32,
    pub stroke_lightness_jitter: f32,
    pub stroke_secondary_jitter: f32,
}

impl Default for BrushColorDynamics {
    fn default() -> Self {
        Self {
            secondary_color_rgba_linear: [0.02, 0.02, 0.018, 1.0],
            stamp_hue_jitter: 0.0,
            stamp_saturation_jitter: 0.0,
            stamp_lightness_jitter: 0.0,
            stamp_secondary_jitter: 0.0,
            stroke_hue_jitter: 0.0,
            stroke_saturation_jitter: 0.0,
            stroke_lightness_jitter: 0.0,
            stroke_secondary_jitter: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushRendering {
    pub blend_mode: BrushBlendMode,
    pub accumulation: BrushAccumulation,
    pub alpha_threshold: f32,
    pub wet_edge: f32,
    pub burnt_edge: f32,
    pub edge_width: f32,
    pub edge_after_stroke: bool,
}

impl Default for BrushRendering {
    fn default() -> Self {
        Self {
            blend_mode: BrushBlendMode::Normal,
            accumulation: BrushAccumulation::Flow,
            alpha_threshold: 0.0,
            wet_edge: 0.0,
            burnt_edge: 0.0,
            edge_width: 0.0,
            edge_after_stroke: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushWetMix {
    pub amount_of_paint: f32,
    pub density: f32,
    pub charge: f32,
    /// Exponential paint-load loss per brush diameter traveled. Zero keeps the
    /// initial charge for the whole stroke; the reservoir can still pick up
    /// canvas color between contacts.
    pub charge_depletion: f32,
    pub dilution: f32,
    pub attack: f32,
    pub pull: f32,
    pub blur: f32,
    pub wetness_jitter: f32,
    /// Water/material state deposited into the sparse canvas material surface.
    /// Dry brushes leave this at zero and allocate no material pages.
    pub wetness: f32,
    pub mix_space: ColorMixSpace,
}

impl Default for BrushWetMix {
    fn default() -> Self {
        Self {
            amount_of_paint: 1.0,
            density: 1.0,
            charge: 1.0,
            charge_depletion: 0.0,
            dilution: 0.0,
            attack: 1.0,
            pull: 0.0,
            blur: 0.0,
            wetness_jitter: 0.0,
            wetness: 0.0,
            mix_space: ColorMixSpace::LinearRgb,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushDeform {
    pub mode: LiquifyMode,
    pub strength: f32,
    pub pressure: f32,
    pub momentum: f32,
    pub distortion: f32,
}

impl Default for BrushDeform {
    fn default() -> Self {
        Self {
            mode: LiquifyMode::Push,
            strength: 0.5,
            pressure: 1.0,
            momentum: 0.0,
            distortion: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushBounds {
    pub minimum_size: f32,
    pub maximum_size: f32,
    pub minimum_opacity: f32,
    pub maximum_opacity: f32,
}

/// Event-driven capillary transport owned by a brush preset.
///
/// The conductance texture is tiled in document space. Each submitted input
/// update deposits watercolor wetness and runs one bounded GPU exchange over
/// the new dabs' affected neighborhoods; there is no frame-driven fluid
/// simulation.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushTransport {
    pub conductance: AssetId,
    /// Texture repeats per 256 document pixels.
    pub scale: f32,
    pub rotation_radians: f32,
    /// Shapes the texture response. `0` is uniform paper; `1` follows the
    /// conductance texture at full contrast.
    pub contrast: f32,
    /// Exchange rate when both endpoints were already wet before this dab.
    pub wet_flow: f32,
    /// Exchange rate when the dab reaches an initially dry endpoint.
    pub dry_flow: f32,
    /// Maximum transport hop in document pixels.
    pub distance: f32,
    /// Water deposited into the persistent R8 wetness field.
    pub water_load: f32,
}

impl Default for BrushBounds {
    fn default() -> Self {
        Self {
            minimum_size: 0.01,
            maximum_size: MAX_BRUSH_DIAMETER,
            minimum_opacity: 0.0,
            maximum_opacity: 1.0,
        }
    }
}

/// Exact, immutable brush semantics captured when a stroke begins.
///
/// Dynamic mappings are Arc-backed so strokes share preset data. Editing a
/// preset creates a new snapshot; the active contact retains its own settings.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushSnapshot {
    pub tip: BrushTip,
    /// Straight linear document RGB; coverage alpha is bounded independently.
    pub color_rgba_linear: [f32; 4],
    pub diameter: f32,
    /// Constant alpha multiplier applied to every dab.
    pub opacity: f32,
    /// Analytic-tip edge hardness, applied independently to each emitted dab.
    /// Mask tips define their own coverage and ignore this value.
    pub hardness: f32,
    pub flow: f32,
    /// Dab distance as a fraction of pressure-adjusted diameter.
    pub spacing: f32,
    /// Width divided by height for the unrotated brush tip.
    pub aspect: f32,
    pub angle_radians: f32,
    /// Seed mixed with the stroke ID for deterministic per-dab variation.
    pub seed: u32,
    pub mappings: Arc<[BrushMapping]>,
    pub execution: BrushExecution,
    pub path: BrushPath,
    pub stabilization: BrushStabilization,
    pub taper: BrushTaper,
    pub shape: BrushShape,
    pub grain: Option<BrushGrain>,
    pub color_dynamics: BrushColorDynamics,
    pub rendering: BrushRendering,
    pub wet_mix: BrushWetMix,
    pub transport: Option<BrushTransport>,
    pub deform: BrushDeform,
    pub bounds: BrushBounds,
    /// Optional coherent GPU contact model.
    pub contact: Option<BrushContact>,
}

impl Default for BrushSnapshot {
    fn default() -> Self {
        Self {
            tip: BrushTip::AnalyticEllipse,
            color_rgba_linear: [0.02, 0.02, 0.018, 1.0],
            diameter: 12.0,
            opacity: 1.0,
            hardness: 0.82,
            flow: 0.9,
            spacing: 0.12,
            aspect: 1.0,
            angle_radians: 0.0,
            seed: 0x4c41_5952,
            mappings: Arc::from([BrushMapping::pressure_size()]),
            execution: BrushExecution::Dry,
            path: BrushPath::default(),
            stabilization: BrushStabilization::default(),
            taper: BrushTaper::default(),
            shape: BrushShape::default(),
            grain: None,
            color_dynamics: BrushColorDynamics::default(),
            rendering: BrushRendering::default(),
            wet_mix: BrushWetMix::default(),
            transport: None,
            deform: BrushDeform::default(),
            bounds: BrushBounds::default(),
            contact: None,
        }
    }
}

impl BrushSnapshot {
    pub fn validate(&self) -> Result<(), BrushError> {
        let base = [
            self.color_rgba_linear[0],
            self.color_rgba_linear[1],
            self.color_rgba_linear[2],
            self.color_rgba_linear[3],
            self.diameter,
            self.opacity,
            self.hardness,
            self.flow,
            self.spacing,
            self.aspect,
            self.angle_radians,
        ];
        if base.iter().any(|value| !value.is_finite())
            || !(0.0..=1.0).contains(&self.color_rgba_linear[3])
            || !(0.01..=MAX_BRUSH_DIAMETER).contains(&self.diameter)
            || !(0.0..=1.0).contains(&self.opacity)
            || !(0.0..=1.0).contains(&self.hardness)
            || !(0.0..=1.0).contains(&self.flow)
            || !(0.005..=10.0).contains(&self.spacing)
            || !(0.02..=50.0).contains(&self.aspect)
        {
            return Err(BrushError::InvalidBase);
        }
        self.validate_advanced()?;
        if let Some(contact) = self.contact {
            contact.validate()?;
            if contact.paper > 0.0 && self.grain.is_none() {
                return Err(BrushError::InvalidAdvanced);
            }
        }
        if self.mappings.len() > MAX_BRUSH_MAPPINGS {
            return Err(BrushError::TooManyMappings);
        }
        for (index, mapping) in self.mappings.iter().enumerate() {
            let scalars = [
                mapping.input_min,
                mapping.input_max,
                mapping.output_scale,
                mapping.output_bias,
            ];
            if scalars.iter().any(|value| !value.is_finite())
                || (mapping.input_max - mapping.input_min).abs() <= f32::EPSILON
                || mapping.curve.samples.iter().any(|value| !value.is_finite())
            {
                return Err(BrushError::InvalidMapping(index));
            }
        }
        Ok(())
    }

    pub fn execution_class(&self) -> BrushExecution {
        match self.execution {
            BrushExecution::Dry
                if self.wet_mix.pull > 0.0
                    || self.wet_mix.blur > 0.0
                    || self.wet_mix.dilution > 0.0 =>
            {
                BrushExecution::Wet
            }
            execution => execution,
        }
    }

    fn validate_advanced(&self) -> Result<(), BrushError> {
        let unit = [
            self.path.spacing_jitter,
            self.stabilization.streamline,
            self.stabilization.pressure_smoothing,
            self.stabilization.stabilization,
            self.stabilization.motion_filtering,
            self.stabilization.expression,
            self.shape.count_jitter,
            self.shape.size_jitter,
            self.shape.rotation_jitter,
            self.shape.follow_direction,
            self.shape.follow_tilt,
            self.shape.follow_twist,
            self.shape.flip_x_probability,
            self.shape.flip_y_probability,
            self.color_dynamics.stamp_hue_jitter,
            self.color_dynamics.stamp_saturation_jitter,
            self.color_dynamics.stamp_lightness_jitter,
            self.color_dynamics.stamp_secondary_jitter,
            self.color_dynamics.stroke_hue_jitter,
            self.color_dynamics.stroke_saturation_jitter,
            self.color_dynamics.stroke_lightness_jitter,
            self.color_dynamics.stroke_secondary_jitter,
            self.rendering.alpha_threshold,
            self.rendering.wet_edge,
            self.rendering.burnt_edge,
            self.wet_mix.amount_of_paint,
            self.wet_mix.density,
            self.wet_mix.charge,
            self.wet_mix.charge_depletion,
            self.wet_mix.dilution,
            self.wet_mix.attack,
            self.wet_mix.pull,
            self.wet_mix.blur,
            self.wet_mix.wetness_jitter,
            self.wet_mix.wetness,
            self.deform.strength,
            self.deform.pressure,
            self.deform.momentum,
            self.deform.distortion,
        ];
        let finite_nonnegative = [
            self.path.jitter_along,
            self.path.jitter_across,
            self.path.falloff_distance,
            self.path.continuous_rate_hz,
            self.taper.start_distance_diameters,
            self.taper.end_distance_diameters,
            self.rendering.edge_width,
        ];
        let taper = [
            self.taper.start_size,
            self.taper.end_size,
            self.taper.start_opacity,
            self.taper.end_opacity,
        ];
        let colors = self.color_dynamics.secondary_color_rgba_linear;
        let bounds = [
            self.bounds.minimum_size,
            self.bounds.maximum_size,
            self.bounds.minimum_opacity,
            self.bounds.maximum_opacity,
        ];
        let transport_invalid = self.transport.as_ref().is_some_and(|transport| {
            let unit = [
                transport.contrast,
                transport.wet_flow,
                transport.dry_flow,
                transport.water_load,
            ];
            unit.iter()
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
                || !transport.scale.is_finite()
                || !(0.125..=16.0).contains(&transport.scale)
                || !transport.rotation_radians.is_finite()
                || !transport.distance.is_finite()
                || !(0.0..=96.0).contains(&transport.distance)
        });
        if unit
            .iter()
            .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            || self.stabilization.pressure_fall_micros > 1_000_000
            || finite_nonnegative
                .iter()
                .any(|value| !value.is_finite() || *value < 0.0)
            || taper
                .iter()
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            || colors
                .iter()
                .any(|value| !value.is_finite())
            || !(0.0..=1.0).contains(&colors[3])
            || bounds.iter().any(|value| !value.is_finite())
            || self.path.jitter_along > MAX_BRUSH_SCATTER_DIAMETERS
            || self.path.jitter_across > MAX_BRUSH_SCATTER_DIAMETERS
            || self.path.continuous_rate_hz > 1_000.0
            || self.shape.count == 0
            || !self.taper.tip_sharpness.is_finite()
            || !(0.25..=4.0).contains(&self.taper.tip_sharpness)
            || self.shape.count > MAX_BRUSH_STAMP_COUNT
            || transport_invalid
            || self.bounds.minimum_size < 0.01
            || self.bounds.maximum_size > MAX_BRUSH_DIAMETER
            || self.bounds.minimum_size > self.bounds.maximum_size
            || !(0.0..=self.bounds.maximum_opacity).contains(&self.bounds.minimum_opacity)
            || self.bounds.maximum_opacity > 1.0
        {
            return Err(BrushError::InvalidAdvanced);
        }
        if let Some(grain) = &self.grain {
            validate_grain(grain)?;
        }
        Ok(())
    }

    /// Conservative document-space radius for validation, culling, and damage
    /// before exact dabs are available. Runtime evaluation applies the same
    /// hard safety limits, so a malformed preset cannot create unbounded work.
    pub fn conservative_radius(&self) -> f32 {
        let (_, diameter_max) = self.target_range(BrushTarget::Diameter);
        let (aspect_min, aspect_max) = self.target_range(BrushTarget::Aspect);
        let diameter = diameter_max.clamp(0.01, MAX_BRUSH_DIAMETER);
        let aspect_min = aspect_min.clamp(0.02, 50.0);
        let aspect_max = aspect_max.clamp(0.02, 50.0);
        let shape_radius = diameter * 0.5 * (1.0 / aspect_min).max(aspect_max).max(1.0);
        let scatter = [BrushTarget::ScatterAlong, BrushTarget::ScatterAcross].map(|target| {
            let (minimum, maximum) = self.target_range(target);
            minimum.abs().max(maximum.abs())
        });
        let scatter_radius = scatter[0]
            .min(MAX_BRUSH_SCATTER_DIAMETERS)
            .hypot(scatter[1].min(MAX_BRUSH_SCATTER_DIAMETERS))
            * diameter;
        let contact_extent = self.contact.map_or(1.0, |c| (1.0 + c.tilt_spread) * 1.5);
        shape_radius * contact_extent + scatter_radius + 2.0
    }

    fn target_range(&self, target: BrushTarget) -> (f32, f32) {
        let base = match target {
            BrushTarget::Diameter => self.diameter,
            BrushTarget::Opacity => self.opacity,
            BrushTarget::Flow => self.flow,
            BrushTarget::Hardness => self.hardness,
            BrushTarget::Spacing => self.spacing,
            BrushTarget::Aspect => self.aspect,
            BrushTarget::Rotation => self.angle_radians,
            BrushTarget::ScatterAlong
            | BrushTarget::ScatterAcross
            | BrushTarget::Hue
            | BrushTarget::Saturation
            | BrushTarget::Lightness
            | BrushTarget::SecondaryColor => 0.0,
            BrushTarget::GrainDepth => self.grain.as_ref().map_or(0.0, |grain| grain.depth),
            BrushTarget::Pull => self.wet_mix.pull,
            BrushTarget::Deposit => self.wet_mix.attack,
            BrushTarget::DeformStrength => self.deform.strength,
        };
        let mut range = (base, base);
        for mapping in self
            .mappings
            .iter()
            .take(MAX_BRUSH_MAPPINGS)
            .filter(|item| item.target == target)
        {
            let mut curve_min = f32::INFINITY;
            let mut curve_max = f32::NEG_INFINITY;
            for value in mapping.curve.samples {
                if value.is_finite() {
                    curve_min = curve_min.min(value);
                    curve_max = curve_max.max(value);
                }
            }
            if !curve_min.is_finite() {
                continue;
            }
            let outputs = [
                curve_min.mul_add(mapping.output_scale, mapping.output_bias),
                curve_max.mul_add(mapping.output_scale, mapping.output_bias),
            ];
            let contribution = (outputs[0].min(outputs[1]), outputs[0].max(outputs[1]));
            range = match mapping.combine {
                BrushCombine::Replace => contribution,
                BrushCombine::Add => (range.0 + contribution.0, range.1 + contribution.1),
                BrushCombine::Multiply => {
                    let products = [
                        range.0 * contribution.0,
                        range.0 * contribution.1,
                        range.1 * contribution.0,
                        range.1 * contribution.1,
                    ];
                    products
                        .into_iter()
                        .fold((f32::INFINITY, f32::NEG_INFINITY), |bounds, value| {
                            (bounds.0.min(value), bounds.1.max(value))
                        })
                }
            };
        }
        range
    }
}

fn validate_grain(grain: &BrushGrain) -> Result<(), BrushError> {
    let values = [
        grain.scale,
        grain.depth,
        grain.rotation_radians,
        grain.offset_jitter,
    ];
    if values.iter().any(|value| !value.is_finite())
        || !(0.01..=64.0).contains(&grain.scale)
        || !(0.0..=1.0).contains(&grain.depth)
        || !(0.0..=1.0).contains(&grain.offset_jitter)
    {
        return Err(BrushError::InvalidAdvanced);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrushError {
    InvalidBase,
    InvalidAdvanced,
    TooManyMappings,
    InvalidMapping(usize),
}

impl fmt::Display for BrushError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBase => formatter.write_str("invalid brush base settings"),
            Self::InvalidAdvanced => formatter.write_str("invalid advanced brush settings"),
            Self::TooManyMappings => write!(
                formatter,
                "brush exceeds the limit of {MAX_BRUSH_MAPPINGS} dynamics mappings"
            ),
            Self::InvalidMapping(index) => {
                write!(formatter, "brush dynamics mapping {index} is invalid")
            }
        }
    }
}

impl std::error::Error for BrushError {}

/// Bounded contact data for live dynamics, taper and late sensor corrections.
/// Never part of document persistence or undo history.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub id: StrokeId,
    pub target: SourceTarget,
    pub tool: StrokeTool,
    pub brush: BrushSnapshot,
    pub points: Arc<[StrokePoint]>,
    /// Exclusive point ends of material updates. Watercolor transports pigment
    /// after each update, so replay must preserve these boundaries, not merge
    /// an entire stroke into one transport step. Empty means one update.
    pub material_updates: Arc<[u32]>,
    pub bounds: Rect,
    /// Captured at contact start; replay must not use today's alpha lock.
    pub alpha_locked: bool,
    /// The pen measured barrel rotation; otherwise twist is only the view angle.
    pub barrel_twist: bool,
    /// Immutable layer-local coverage captured at stroke start, including for
    /// mask painting. Later selection edits must not change stroke replay.
    pub selection: Option<Arc<Selection>>,
    /// A retouching stroke's source, captured at stroke start.
    pub retouch: Option<Retouch>,
    /// How its dabs lay over paint, captured at contact start.
    pub blend_space: BlendSpace,
}

impl Stroke {
    pub fn new(
        id: StrokeId,
        target: SourceTarget,
        tool: StrokeTool,
        brush: BrushSnapshot,
        points: impl Into<Arc<[StrokePoint]>>,
    ) -> Result<Self, DocumentError> {
        brush.validate().map_err(DocumentError::InvalidBrush)?;
        let mut points = points.into();
        if points.is_empty() {
            return Err(DocumentError::EmptyStroke);
        }
        if points.iter().any(|point| {
            !point.position.x.is_finite()
                || !point.position.y.is_finite()
                || !point.pressure.is_finite()
        }) {
            return Err(DocumentError::NonFiniteStroke);
        }
        if points
            .iter()
            .any(|point| !(0.0..=1.0).contains(&point.pressure))
        {
            let mut owned = points.to_vec();
            for point in &mut owned {
                point.pressure = point.pressure.clamp(0.0, 1.0);
            }
            points = owned.into();
        }
        let mut bounds = Rect::EMPTY;
        let radius = brush.conservative_radius();
        for point in points.iter() {
            bounds.include_circle(point.position, radius);
        }
        Ok(Self {
            id,
            target,
            tool,
            brush,
            points,
            material_updates: Arc::default(),
            bounds,
            alpha_locked: false,
            barrel_twist: false,
            selection: None,
            retouch: None,
            blend_space: BlendSpace::Linear,
        })
    }
}

pub use authored::{
    Attachment, Artwork, ArtworkCapture, CaptureCheckpoint, Composition, CompositionHandle, CoverageHandle,
    CoverageSource, Definition, DefinitionHandle, EffectApplication, EffectHandle,
    EvaluationContext, Guides, Handle, MaskUse, Occurrence, OccurrenceContent, OccurrenceDropPlan, OccurrenceDropPosition, OccurrenceHandle,
    Output, OutputHandle, PaintHandle, PaintSource, PortableId, RecordChange, SavedSelection,
    SceneIndex, SceneScope, SceneSnapshot, SceneView, SelectionHandle, SourceTarget, Stack,
    StackHandle, Store, WorkingState,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub artwork: Artwork,
    pub working: WorkingState,
    pub revision: Revision,
    pub owner: u64,
    pub(crate) scene_index: Arc<SceneIndex>,
    next_stroke_id: u64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DocumentNames {
    pub paint: Arc<str>,
    pub paper: Arc<str>,
}

impl Document {
    pub fn new(id: PortableId, width: u32, height: u32, names: DocumentNames) -> Self {
        let mut artwork = Artwork::new([width, height]).expect("valid composition extent");
        artwork.id = id;
        let paint = artwork
            .paint
            .insert(
                PortableId::random(),
                PaintSource {
                    domain: [width, height],
                    raster: Default::default(),
                    original: None,
                    operations: Default::default(),
                },
            )
            .expect("new paint store");
        let ink = artwork
            .occurrences
            .insert(
                PortableId::random(),
                Occurrence::new(OccurrenceContent::Paint(paint), names.paint),
            )
            .expect("new occurrence store");
        let mut fill = EffectInstance::new(bundled_effect_catalog().get("solid_color").unwrap().program());
        fill.set("color", EffectValue::Color(color::RgbColor::WHITE)).expect("valid fill color");
        let definition = artwork.definitions.insert(PortableId::random(), Definition {
            program: fill.program,
        }).expect("new definition store");
        let effect = artwork.effects.insert(PortableId::random(), EffectApplication {
            definition, values: fill.values, domain: [width, height],
        }).expect("new effect store");
        let paper = artwork
            .occurrences
            .insert(
                PortableId::random(),
                Occurrence::new(
                    OccurrenceContent::Effect(effect),
                    names.paper,
                ),
            )
            .expect("new occurrence store");
        let stack = artwork.compositions.get(artwork.root).unwrap().result;
        artwork.stacks.get_mut(stack).unwrap().entries = vec![ink, paper];
        let mut document = Self::from_artwork(artwork).expect("new artwork is editable");
        document.working.occurrence = Some(ink);
        document.working.target = Some(SourceTarget::Paint(paint));
        document.working.layer_selection.insert(ink);
        document.working.layer_anchor = Some(ink);
        document
    }
    pub fn from_artwork(artwork: Artwork) -> Result<Self, DocumentError> {
        static OWNERS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let scene_index =
            Arc::new(SceneIndex::build(&artwork).map_err(DocumentError::InvalidArtwork)?);
        let owner = OWNERS
            .try_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |n| n.checked_add(1),
            )
            .map_err(|_| DocumentError::InvalidLayerOperation("Document owner exhausted"))?;
        let document = Self {
            artwork,
            working: WorkingState::default(),
            revision: 0,
            owner,
            scene_index,
            next_stroke_id: 1,
        };
        document.validate_payloads()?;
        Ok(document)
    }
    pub fn composition(&self) -> &Composition {
        self.artwork
            .compositions
            .get(self.artwork.root)
            .expect("published composition")
    }
    pub fn output(&self) -> &Output {
        self.artwork
            .outputs
            .get(self.artwork.default_output)
            .expect("published output")
    }
    pub fn scene(&self) -> SceneView<'_> {
        SceneView::new(&self.artwork, &self.scene_index).with_owner(self.owner, self.revision)
    }
    pub fn snapshot(&self) -> Arc<SceneSnapshot> {
        self.snapshot_with_context(self.output().context.clone())
    }
    pub fn snapshot_with_context(&self, context: EvaluationContext) -> Arc<SceneSnapshot> {
        Arc::new(SceneSnapshot::new(
            self.artwork.clone(),
            self.scene_index.clone(),
            self.owner,
            self.revision,
            context,
        ))
    }
    pub fn has_animated_effects(&self) -> bool {
        let scene = self.scene();
        scene.order().iter().copied().any(|h| scene.visible(h) && scene.effect(h).is_some_and(|e| e.animated()))
    }
    pub fn allocate_stroke_id(&mut self) -> StrokeId {
        let id = StrokeId(self.next_stroke_id);
        self.next_stroke_id = self
            .next_stroke_id
            .checked_add(1)
            .expect("stroke identity exhausted");
        id
    }
    pub fn next_stroke_id(&self) -> StrokeId {
        StrokeId(self.next_stroke_id)
    }
    pub fn allocate_coverage_handle(&mut self) -> CoverageHandle {
        self.artwork
            .coverage
            .reserve(PortableId::random())
            .expect("coverage identity exhausted")
    }
    pub fn target_raster(&self, target: SourceTarget) -> Option<&raster::RasterRevision> {
        match target {
            SourceTarget::Paint(h) => self.artwork.paint.get(h).map(|s| &s.raster),
            SourceTarget::Coverage(h) => self.artwork.coverage.get(h).map(|s| &s.raster),
            SourceTarget::Selection(_) => None,
        }
    }
    pub fn target_raster_mut(
        &mut self,
        target: SourceTarget,
    ) -> Option<&mut raster::RasterRevision> {
        match target {
            SourceTarget::Paint(h) => self.artwork.paint.get_mut(h).map(|s| &mut s.raster),
            SourceTarget::Coverage(h) => self.artwork.coverage.get_mut(h).map(|s| &mut s.raster),
            SourceTarget::Selection(_) => None,
        }
    }
    pub fn apply(&mut self, edit: Edit) -> Result<Edit, DocumentError> {
        let next_revision =
            self.revision
                .checked_add(1)
                .ok_or(DocumentError::InvalidLayerOperation(
                    "Document revision exhausted",
                ))?;
        if let Edit::SetRaster { target, revision } = &edit {
            if let Some(Ok(data)) = revision.try_data() {
                data.validate_index(
                    self.scene().target_extent(*target),
                    matches!(target, SourceTarget::Coverage(_)),
                    self.composition().color,
                )
                .map_err(DocumentError::InvalidArtwork)?;
            }
            let raster = self
                .target_raster_mut(*target)
                .ok_or(DocumentError::MissingTarget(*target))?;
            let inverse = Edit::SetRaster {
                target: *target,
                revision: std::mem::replace(raster, revision.clone()),
            };
            self.revision = next_revision;
            return Ok(inverse);
        }
        let mut candidate = self.clone();
        let relationships = edit.changes_relationships(self);
        let before_working = self.working.clone();
        let mut inverse = candidate.apply_records(edit)?;
        if relationships {
            candidate.scene_index = Arc::new(
                SceneIndex::build(&candidate.artwork).map_err(DocumentError::InvalidArtwork)?,
            );
        }
        candidate.validate_payloads()?;
        if inverse.contains_working()
            && let Some(h) = candidate.working.occurrence
            && candidate.scene().occurrence(h).is_none()
        {
            return Err(DocumentError::MissingOccurrence(h));
        }
        candidate.repair_working()?;
        if candidate.working != before_working {
            candidate.working.generation = before_working.generation.checked_add(1).ok_or(
                DocumentError::InvalidLayerOperation("Working state generation exhausted"),
            )?;
            if !inverse.contains_working() {
                inverse = Edit::Batch(vec![inverse, Edit::Working(before_working)]);
            }
        }
        candidate.revision = next_revision;
        *self = candidate;
        Ok(inverse)
    }
    fn apply_records(&mut self, edit: Edit) -> Result<Edit, DocumentError> {
        macro_rules! change {
            ($store:ident,$variant:ident,$change:expr) => {{
                let c = $change;
                let value = self
                    .artwork
                    .$store
                    .change(c.handle, c.id, c.value)
                    .map_err(DocumentError::InvalidLayerOperation)?;
                Edit::$variant(RecordChange {
                    handle: c.handle,
                    id: c.id,
                    value,
                })
            }};
        }
        Ok(match edit {
            Edit::Composition(c) => change!(compositions, Composition, c),
            Edit::Stack(c) => change!(stacks, Stack, c),
            Edit::Occurrence(c) => change!(occurrences, Occurrence, c),
            Edit::Paint(c) => change!(paint, Paint, c),
            Edit::Coverage(c) => change!(coverage, Coverage, c),
            Edit::Effect(c) => change!(effects, Effect, c),
            Edit::Definition(c) => change!(definitions, Definition, c),
            Edit::SavedSelection(c) => change!(selections, SavedSelection, c),
            Edit::Guides(c) => change!(guides, Guides, c),
            Edit::Output(c) => change!(outputs, Output, c),
            Edit::Working(w) => Edit::Working(std::mem::replace(&mut self.working, w)),
            Edit::SetRaster { target, revision } => {
                let raster = self
                    .target_raster_mut(target)
                    .ok_or(DocumentError::MissingTarget(target))?;
                Edit::SetRaster {
                    target,
                    revision: std::mem::replace(raster, revision),
                }
            }
            Edit::Batch(edits) => {
                let mut inverses = Vec::with_capacity(edits.len());
                for edit in edits {
                    inverses.push(self.apply_records(edit)?);
                }
                inverses.reverse();
                Edit::Batch(inverses)
            }
        })
    }
    pub fn effective_visibility(&self,id:OccurrenceHandle)->bool {
        let Some(occurrence)=self.artwork.occurrences.get(id)else{return false;};
        if matches!(occurrence.content,OccurrenceContent::Selection(_)) {
            self.working.selection_visibility.get(&id).copied().unwrap_or(occurrence.visible)
        }else{occurrence.visible}
    }
    fn repair_working(&mut self) -> Result<(), DocumentError> {
        if let Some(selection) = &self.working.selection {
            selection.validate()?;
        }
        let scene = self.scene();
        let occurrence = self
            .working
            .occurrence
            .filter(|h| scene.order().contains(h));
        let occurrence =
            if self.working.occurrence.is_none() {
                None
            } else {
                occurrence
                    .or_else(|| {
                        scene.order().iter().copied().find(|h| {
                            matches!(scene.source_target(*h), Some(SourceTarget::Paint(_)))
                        })
                    })
                    .or_else(|| scene.order().first().copied())
            };
        let target = match (occurrence, self.working.target) {
            (Some(h), Some(SourceTarget::Coverage(c)))
                if scene.mask(h).is_some_and(|(m, _)| m.source == c) =>
            {
                Some(SourceTarget::Coverage(c))
            }
            (Some(h), _) => scene.source_target(h),
            _ => None,
        };
        let inspect = self.working.inspect_mask.filter(|h| {
            Some(*h) == occurrence
                && scene.mask(*h).is_some()
                && matches!(target, Some(SourceTarget::Coverage(_)))
        });
        self.working.occurrence = occurrence;
        self.working.target = target;
        self.working.inspect_mask = inspect;
        self.working.layer_selection.retain(|h| self.artwork.occurrences.get(*h).is_some());
        self.working.layer_anchor = self.working.layer_anchor.filter(|h| self.artwork.occurrences.get(*h).is_some());
        if let Some(visibility) = &mut self.working.solo_visibility {
            visibility.retain(|h, _| self.artwork.occurrences.get(*h).is_some());
        }
        self.working.selection_visibility.retain(|h,_|self.artwork.occurrences.get(*h).is_some_and(|o|matches!(o.content,OccurrenceContent::Selection(_))));
        Ok(())
    }
    fn validate_payloads(&self) -> Result<(), DocumentError> {
        let invalid = DocumentError::InvalidLayerOperation;
        let extent = |size: [u32; 2]| {
            if size.contains(&0) || size.iter().any(|n| *n > MAX_EXTENT) {
                Err(invalid("Invalid source extent"))
            } else {
                Ok(())
            }
        };
        for (_, _, c) in self.artwork.compositions.iter() {
            extent(c.size)?;
            if !c.origin.x.is_finite() || !c.origin.y.is_finite() {
                return Err(invalid("Invalid composition origin"));
            }
            if c.blend == BlendSpace::Perceptual
                && let Some(reason) = BlendSpace::unavailable_reason(c.color.depth)
            {
                return Err(invalid(reason));
            }
            if let Some(r) = c.resolution {
                r.validate().map_err(DocumentError::InvalidArtwork)?;
            }
        }
        self.artwork
            .metadata
            .validate()
            .map_err(DocumentError::InvalidArtwork)?;
        let color = self.composition().color;
        for (_, _, s) in self.artwork.paint.iter() {
            extent(s.domain)?;
            if let Some(original) = &s.original {
                original.validate().map_err(DocumentError::InvalidArtwork)?;
                if !original.is_original()
                    && (original.interpretation.profile_assumed
                        || original.interpretation.depth != color.depth
                        || original.interpretation.profile
                            != color::ColorProfile::Builtin(color.space))
                {
                    return Err(invalid(
                        "Rasterized image interpretation differs from the document",
                    ));
                }
            }
            if let Some(Ok(data)) = s.raster.try_data() {
                data.validate_index(s.domain, false, color)
                    .map_err(DocumentError::InvalidArtwork)?;
            }
            for operation in s.operations.iter() {
                operation.validate()?;
            }
        }
        for (_, _, s) in self.artwork.coverage.iter() {
            extent(s.domain)?;
            s.validate()?;
            if !s.default_coverage.is_finite() || !(0.0..=1.).contains(&s.default_coverage) {
                return Err(invalid("Invalid default coverage"));
            }
            if let Some(initial) = &s.initial {
                initial.validate()?;
            }
            if let Some(Ok(data)) = s.raster.try_data() {
                data.validate_index(s.domain, true, color)
                    .map_err(DocumentError::InvalidArtwork)?;
            }
            for operation in s.operations.iter() {
                operation.validate()?;
            }
        }
        for (h, _, o) in self.artwork.occurrences.iter() {
            if !o.opacity.is_finite()
                || !(0.0..=1.).contains(&o.opacity)
                || !o.translation.x.is_finite()
                || !o.translation.y.is_finite()
            {
                return Err(invalid("Invalid occurrence value"));
            }
            o.placement
                .validate_for(Rect::from_extent(self.scene().local_extent(h)))?;
            if o.blend == LayerBlend::PassThrough
                && !matches!(o.content, OccurrenceContent::Stack(_))
            {
                return Err(invalid("Only groups can use Pass Through"));
            }
            if o.attachment!=Attachment::None && self.scene().attachment_target(h).is_none(){return Err(invalid("An attachment needs a target in its stack"));}
            if o.isolated_blend==LayerBlend::PassThrough {return Err(invalid("The retained isolated blend cannot be Pass Through"));}
            if o.passes_through() && (o.attachment!=Attachment::None || !self.scene().attached_effects(h).is_empty() || self.scene().order().iter().any(|other|self.scene().clipping_base(*other)==Some(h))) {return Err(invalid("Release attachments before switching to Pass Through"));}
            if let OccurrenceContent::Selection(_) = o.content
                && (o.mask.is_some() || o.attachment != Attachment::None || o.alpha_locked || o.blend != LayerBlend::Normal || o.opacity != 1.)
            {
                return Err(invalid("Selection Layers cannot contain artwork"));
            }
            if let Some(mask) = &o.mask {
                let source =
                    self.artwork
                        .coverage
                        .get(mask.source)
                        .ok_or(DocumentError::MissingTarget(SourceTarget::Coverage(
                            mask.source,
                        )))?;
                if !mask.translation.x.is_finite()
                    || !mask.translation.y.is_finite()
                    || mask.placement.inverse().is_none()
                    || !mask.placement.covers(Rect::from_extent(source.domain))
                {
                    return Err(invalid("Invalid mask placement"));
                }
            }
        }
        for (_, _, e) in self.artwork.effects.iter() {
            extent(e.domain)?;
            let definition = self
                .artwork
                .definitions
                .get(e.definition)
                .ok_or(invalid("Missing effect definition"))?;
            EffectView::new(&definition.program, &e.values)
                .validate()
                .map_err(invalid)?;
        }
        for (_, _, d) in self.artwork.definitions.iter() {
            EffectInstance::new(d.program.clone())
                .validate()
                .map_err(invalid)?;
        }
        for (_, _, s) in self.artwork.selections.iter() {
            s.selection.validate()?;
            s.display.validate()?;
        }
        crate::rulers::validate_rulers(&self.rulers().collect::<Vec<_>>())?;
        for (_, _, o) in self.artwork.outputs.iter() {
            o.sdr
                .validate()
                .map_err(|_| invalid("Invalid SDR rendition"))?;
            if o.scale.iter().any(|v| !v.is_finite() || *v <= 0.)
                || !o.context.elapsed.is_finite()
                || o.context
                    .phases
                    .iter()
                    .any(|(h, p)| self.artwork.effects.get(*h).is_none() || !p.is_finite())
            {
                return Err(invalid("Invalid output context"));
            }
            if let Some(p) = &o.proof {
                p.validate().map_err(|_| invalid("Invalid proof recipe"))?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    Composition(RecordChange<Composition>),
    Stack(RecordChange<Stack>),
    Occurrence(RecordChange<Occurrence>),
    Paint(RecordChange<PaintSource>),
    Coverage(RecordChange<CoverageSource>),
    Effect(RecordChange<EffectApplication>),
    Definition(RecordChange<Definition>),
    SavedSelection(RecordChange<SavedSelection>),
    Guides(RecordChange<Guides>),
    Output(RecordChange<Output>),
    Working(WorkingState),
    Batch(Vec<Edit>),
    SetRaster {
        target: SourceTarget,
        revision: raster::RasterRevision,
    },
}
impl Edit {
    pub fn resulting_color(&self, current: color::DocumentColor) -> color::DocumentColor {
        match self {
            Self::Composition(c) => c.value.as_ref().map_or(current, |v| v.color),
            Self::Batch(es) => es.iter().fold(current, |c, e| e.resulting_color(c)),
            _ => current,
        }
    }
    pub fn canvas_origin_from(&self, current: Point) -> Option<[i32; 2]> {
        let origin = self.resulting_origin(current);
        (origin != current)
            .then_some([(origin.x - current.x) as i32, (origin.y - current.y) as i32])
    }
    fn resulting_origin(&self, current: Point) -> Point {
        match self {
            Self::Composition(c) => c.value.as_ref().map_or(current, |c| c.origin),
            Self::Batch(es) => es
                .iter()
                .fold(current, |origin, e| e.resulting_origin(origin)),
            _ => current,
        }
    }
    pub fn only_raster_updates(&self) -> bool {
        match self {
            Self::SetRaster { .. } => true,
            Self::Batch(es) => !es.is_empty() && es.iter().all(Self::only_raster_updates),
            _ => false,
        }
    }
    fn changes_project(&self) -> bool {
        match self {
            Self::Working(_) => false,
            Self::Batch(es) => es.iter().any(Self::changes_project),
            _ => true,
        }
    }
    pub fn changes_image(&self,document:&Document)->bool {
        match self {
            Self::Guides(_)|Self::Output(_)|Self::SavedSelection(_)|Self::Working(_)=>false,
            Self::Composition(c)=>c.value.as_ref().zip(document.artwork.compositions.get(c.handle)).is_none_or(|(a,b)|a.size!=b.size||a.origin!=b.origin||a.color!=b.color||a.blend!=b.blend||a.result!=b.result),
            Self::Occurrence(c)=>c.value.as_ref().zip(document.artwork.occurrences.get(c.handle)).is_none_or(|(a,b)|a.content!=b.content||a.visible!=b.visible||a.opacity!=b.opacity||a.blend!=b.blend||a.attachment!=b.attachment||a.translation!=b.translation||a.placement!=b.placement||a.mask!=b.mask),
            Self::Batch(es)=>{
                let mut current=document.clone();
                for edit in es {
                    if edit.changes_image(&current) || current.apply_records(edit.clone()).is_err() {return true;}
                }
                false
            },
            _=>true,
        }
    }
    fn contains_working(&self) -> bool {
        match self {
            Self::Working(_) => true,
            Self::Batch(es) => es.iter().any(Self::contains_working),
            _ => false,
        }
    }
    fn navigation(&self, document: &Document) -> bool {
        match self {
            Self::Working(w) => w.selection == document.working.selection && w.solo_visibility == document.working.solo_visibility,
            Self::Batch(es) => !es.is_empty() && es.iter().all(|e| e.navigation(document)),
            _ => false,
        }
    }
    fn changes_relationships(&self, document: &Document) -> bool {
        match self {
            Self::Stack(_) => true,
            Self::Composition(c) => c
                .value
                .as_ref()
                .zip(document.artwork.compositions.get(c.handle))
                .is_none_or(|(a, b)| a.result != b.result),
            Self::Occurrence(c) => c
                .value
                .as_ref()
                .zip(document.artwork.occurrences.get(c.handle))
                .is_none_or(|(a, b)| {
                    a.content != b.content
                        || a.attachment != b.attachment
                        || a.passes_through() != b.passes_through()
                        || a.mask.as_ref().map(|m| m.source) != b.mask.as_ref().map(|m| m.source)
                }),
            Self::Paint(c) => c.value.is_none() || document.artwork.paint.get(c.handle).is_none(),
            Self::Coverage(c) => {
                c.value.is_none() || document.artwork.coverage.get(c.handle).is_none()
            }
            Self::Effect(c) => c
                .value
                .as_ref()
                .zip(document.artwork.effects.get(c.handle))
                .is_none_or(|(a, b)| a.definition != b.definition),
            Self::Definition(c) => c.value.as_ref().zip(document.artwork.definitions.get(c.handle)).is_none_or(|(a,b)|a.program.kind!=b.program.kind),
            Self::SavedSelection(c) => {
                c.value.is_none() || document.artwork.selections.get(c.handle).is_none()
            }
            Self::Guides(c) => c.value.is_none() || document.artwork.guides.get(c.handle).is_none(),
            Self::Output(c) => c
                .value
                .as_ref()
                .zip(document.artwork.outputs.get(c.handle))
                .is_none_or(|(a, b)| a.composition != b.composition),
            Self::Batch(es) => es.iter().any(|e| e.changes_relationships(document)),
            _ => false,
        }
    }
    fn requires_history_admission(&self, document: &Document) -> bool {
        macro_rules! previous {
            ($store:ident,$variant:ident,$change:expr) => {{
                let c=$change;
                Edit::$variant(RecordChange {handle:c.handle,id:c.id,value:document.artwork.$store.get(c.handle).cloned()})
            }};
        }
        let previous=match self {
            Self::SetRaster {..}=>return false,
            Self::Batch(edits)=>{
                let mut candidate=document.clone();
                for edit in edits {
                    if edit.requires_history_admission(&candidate){return true;}
                    if candidate.apply_records(edit.clone()).is_err(){return true;}
                }
                return false;
            },
            Self::Paint(c)=>previous!(paint,Paint,c),
            Self::Coverage(c)=>previous!(coverage,Coverage,c),
            Self::Effect(c)=>previous!(effects,Effect,c),
            Self::Definition(c)=>previous!(definitions,Definition,c),
            Self::Occurrence(c)=>previous!(occurrences,Occurrence,c),
            Self::SavedSelection(c)=>previous!(selections,SavedSelection,c),
            Self::Output(c)=>previous!(outputs,Output,c),
            Self::Working(_)=>Self::Working(document.working.clone()),
            Self::Composition(_)|Self::Stack(_)|Self::Guides(_)=>return false,
        };
        let mut before=RootInventory::default();previous.roots(&mut before);
        let mut after=RootInventory::default();self.roots(&mut after);
        before.ownership()!=after.ownership()
    }
    pub(crate) fn roots<'a>(&'a self, out: &mut RootInventory<'a>) {
        match self {
            Self::Paint(c) => {
                if let Some(s) = &c.value {
                    out.paint(s);
                }
            }
            Self::Coverage(c) => {
                if let Some(s) = &c.value {
                    out.coverage(s);
                }
            }
            Self::Effect(c) => {
                if let Some(e) = &c.value {
                    out.values(&e.values);
                }
            }
            Self::Definition(c) => {
                if let Some(d) = &c.value {
                    out.program(&d.program);
                }
            }
            Self::Occurrence(c) => {
                if let Some(o) = &c.value {
                    out.meshes.extend(o.placement.mesh.iter());
                }
            }
            Self::SavedSelection(c) => {
                if let Some(s) = &c.value {
                    out.selections.push(&s.selection);
                }
            }
            Self::Working(w) => out.selections.extend(w.selection.iter()),
            Self::SetRaster { revision, .. } => out.rasters.push(revision),
            Self::Output(c) => {
                if let Some(o) = &c.value {
                    out.proof(o.proof.as_ref());
                }
            }
            Self::Batch(es) => {
                for e in es {
                    e.roots(out);
                }
            }
            _ => (),
        }
    }
    fn raster_roots<'a>(&'a self, out: &mut Vec<&'a raster::RasterRevision>) {
        let mut roots = RootInventory::default();
        self.roots(&mut roots);
        out.extend(roots.rasters);
    }
    fn resource_roots<'a>(&'a self, out: &mut Vec<&'a Arc<Lut3d>>) {
        let mut roots = RootInventory::default();
        self.roots(&mut roots);
        out.extend(roots.resources);
    }

}

#[derive(Default)]
pub(crate) struct RootInventory<'a> {
    pub rasters: Vec<&'a raster::RasterRevision>,
    pub sources: Vec<&'a Arc<color::source::SourceImage>>,
    pub selections: Vec<&'a Selection>,
    pub resources: Vec<&'a Arc<Lut3d>>,
    pub meshes: Vec<&'a Arc<MeshMap>>,
    pub profiles: Vec<&'a color::ColorProfile>,
    pub programs: Vec<&'a Arc<EffectProgram>>,
    pub extensions: Vec<&'a Arc<authored::Extensions>>,
    operations: Vec<&'a Arc<Vec<RasterOperation>>>,
    authored_only: bool,
}
impl<'a> RootInventory<'a> {
    fn ownership(&self)->std::collections::BTreeSet<(u8,u64)> {
        let mut owners=std::collections::BTreeSet::new();
        owners.extend(self.rasters.iter().filter(|r|!r.is_empty()).map(|r|(0,r.identity())));
        owners.extend(self.extensions.iter().map(|e|(9,Arc::as_ptr(e) as usize as u64)));
        owners.extend(self.operations.iter().map(|ops|(8,Arc::as_ptr(ops) as usize as u64)));
        owners.extend(self.sources.iter().map(|s|(1,Arc::as_ptr(s) as usize as u64)));
        owners.extend(self.resources.iter().filter_map(|r|r.storage().map(|s|(2,s.as_ptr() as usize as u64))));
        owners.extend(self.meshes.iter().map(|m|(3,Arc::as_ptr(m) as usize as u64)));
        owners.extend(self.programs.iter().map(|p|(4,Arc::as_ptr(p) as usize as u64)));
        owners.extend(self.profiles.iter().filter_map(|p|if let color::ColorProfile::Icc(s)=p{Some((5,s.as_ptr() as usize as u64))}else{None}));
        for selection in &self.selections {
            match &selection.shape {
                SelectionShape::Pixels(p)=>{owners.insert((6,p.words().as_ptr() as usize as u64));},
                SelectionShape::Contours(paths)=>owners.extend(paths.iter().map(|p|(7,p.as_ptr() as usize as u64))),
            }
        }
        owners
    }

    pub fn document(&mut self, document: &'a Document) {
        self.artwork(&document.artwork);
        self.selections.extend(document.working.selection.iter());
    }
    pub fn artwork(&mut self, artwork: &'a Artwork) {
        if !artwork.extensions.records.is_empty() || !artwork.extensions.resources.is_empty() { self.extensions.push(&artwork.extensions); }
        for (_, _, s) in artwork.paint.iter() {
            self.paint(s);
        }
        for (_, _, s) in artwork.coverage.iter() {
            self.coverage(s);
        }
        for (_, _, o) in artwork.occurrences.iter() {
            self.meshes.extend(o.placement.mesh.iter());
        }
        for (_, _, e) in artwork.effects.iter() {
            self.values(&e.values);
        }
        for (_, _, d) in artwork.definitions.iter() {
            self.program(&d.program);
        }
        for (_, _, s) in artwork.selections.iter() {
            self.selections.push(&s.selection);
        }
        for (_, _, o) in artwork.outputs.iter() {
            self.proof(o.proof.as_ref());
        }
    }
    fn paint(&mut self, s: &'a PaintSource) {
        self.rasters.push(&s.raster);
        self.sources.extend(s.original.iter());
        if !s.operations.is_empty() {self.operations.push(&s.operations);}
        for op in s.operations.iter() {
            self.operation(op);
        }
    }
    fn coverage(&mut self, s: &'a CoverageSource) {
        self.rasters.push(&s.raster);
        self.selections.extend(s.initial.iter());
        if !s.operations.is_empty() {self.operations.push(&s.operations);}
        for op in s.operations.iter() {
            self.operation(op);
        }
    }
    fn values(&mut self, values: &'a [EffectValue]) {
        self.resources.extend(values.iter().filter_map(|v| {
            if let EffectValue::Lut3d(Some(r)) = v {
                Some(r)
            } else {
                None
            }
        }));
    }
    fn program(&mut self, program: &'a Arc<EffectProgram>) {
        self.programs.push(program);
        for p in program.parameters.iter() {
            self.values(std::slice::from_ref(&p.default));
        }
    }
    fn proof(&mut self, proof: Option<&'a color::ProofRecipe>) {
        if let Some(p) = proof {
            self.profiles.push(&p.profile);
        }
    }
    fn operation(&mut self, operation: &'a RasterOperation) {
        if self.authored_only {return;}
        self.coverage(&operation.coverage.source);
        match &operation.kind {
            RasterOperationKind::Transform(t) => self.meshes.extend(t.placement.mesh.iter()),
            RasterOperationKind::Bake { scene, .. }
            | RasterOperationKind::FrequencyDetail { scene, .. } => self.artwork(&scene.artwork),
            _ => (),
        }
    }
}

#[derive(Debug)]
pub struct Editor {
    document: Document,
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    checkpoint: u64,
    next_checkpoint: u64,
}
pub(crate) fn json_len(value: &impl serde::Serialize) -> usize {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len());
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count(0);
    match serde_json::to_writer(&mut count, value) {
        Ok(()) => count.0,
        Err(_) => usize::MAX,
    }
}
#[derive(Debug)]
struct HistoryEntry {
    edit: Edit,
    checkpoint: u64,
    metadata_bytes: usize,
}
impl HistoryEntry {
    fn new(edit: Edit, checkpoint: u64) -> Self {
        let metadata_bytes = edit_metadata(&edit);
        Self {
            edit,
            checkpoint,
            metadata_bytes,
        }
    }
}
fn edit_metadata(edit: &Edit) -> usize {
    let extra = match edit {
        Edit::Batch(es) => es
            .iter()
            .map(edit_metadata)
            .fold(0usize, usize::saturating_add),
        Edit::Working(w)=>w.selection_visibility.len().saturating_add(w.layer_selection.len()).saturating_add(w.solo_visibility.as_ref().map_or(0, |v| v.len())).saturating_mul(64),
        Edit::Stack(c) => c.value.as_ref().map_or(0, |s| {
            s.entries.len() * std::mem::size_of::<OccurrenceHandle>()
        }),
        Edit::Occurrence(c) => c.value.as_ref().map_or(0, |o| o.name.len()),
        Edit::Effect(c) => c
            .value
            .as_ref()
            .map_or(0, |e| json_len(&e.values).saturating_mul(4)),
        Edit::Guides(c) => c.value.as_ref().map_or(0, |g| {
            g.rulers
                .iter()
                .map(|(_, g)| json_len(g).saturating_mul(4))
                .sum()
        }),
        Edit::Output(c) => c.value.as_ref().map_or(0, |o| {
            o.name.len()
                + o.proof.as_ref().map_or(0, |p| p.name.len())
                + o.context.phases.len() * 16
        }),
        _ => 0,
    };
    std::mem::size_of::<Edit>().saturating_add(extra)
}
impl Editor {
    pub fn new(document: Document) -> Self {
        Self {
            document,
            undo: Vec::new(),
            redo: Vec::new(),
            checkpoint: 0,
            next_checkpoint: 1,
        }
    }
    pub fn document(&self) -> &Document {
        &self.document
    }
    pub fn checkpoint(&self) -> u64 {
        self.checkpoint
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn next_history_edit(&self, redo: bool) -> Option<&Edit> {
        (if redo { &self.redo } else { &self.undo })
            .last()
            .map(|e| &e.edit)
    }
    pub fn allocate_stroke_id(&mut self) -> StrokeId {
        self.document.allocate_stroke_id()
    }
    pub fn allocate_coverage_handle(&mut self) -> CoverageHandle {
        self.document.allocate_coverage_handle()
    }
    pub fn capture(
        &self,
        session_generation: u64,
        mut context: EvaluationContext,
    ) -> Result<ArtworkCapture, DocumentError> {
        context.retain_effects(&self.document.artwork);
        if !context.elapsed.is_finite()
            || context
                .phases
                .iter()
                .any(|(_, p)| !p.is_finite())
        {
            return Err(DocumentError::InvalidLayerOperation(
                "Invalid capture context",
            ));
        }
        let mut artwork = self.document.artwork.clone();
        artwork
            .outputs
            .get_mut(artwork.default_output)
            .expect("published output")
            .context = context;
        artwork
            .capture(CaptureCheckpoint {
                document: artwork.id,
                owner: self.document.owner,
                session_generation,
                artwork_generation: self.document.revision,
                working_generation: self.document.working.generation,
                edit_checkpoint: self.checkpoint,
            })
            .map_err(DocumentError::InvalidLayerOperation)
    }
    pub fn perform(&mut self, edit: Edit) -> Result<(), DocumentError> {
        self.perform_with_history_budget(edit, history_budget::BYTE_BUDGET)
    }
    pub fn validate_edit(&self, edit: &Edit) -> Result<(), DocumentError> {
        self.prepare_history_edit(edit.clone(), history_budget::BYTE_BUDGET)
            .map(|_| ())
    }
    fn prepare_history_edit(
        &self,
        edit: Edit,
        budget: usize,
    ) -> Result<(Document, HistoryEntry), DocumentError> {
        let mut candidate = self.document.clone();
        let inverse = HistoryEntry::new(candidate.apply(edit)?, self.checkpoint);
        let mut restored = candidate.clone();
        let forward = HistoryEntry::new(restored.apply(inverse.edit.clone())?, self.checkpoint);
        let mut current = RootInventory::default();
        current.document(&self.document);
        let mut resources = Vec::new();
        forward.edit.resource_roots(&mut resources);
        let mut accounting = history_budget::Accounting::default();
        for r in current.resources {
            accounting.charge_resource(r);
        }
        if resources
            .into_iter()
            .any(|r| accounting.charge_resource(r) != 0)
        {
            let mut roots = RootInventory::default();
            roots.document(&candidate);
            let mut accounting = history_budget::Accounting::default();
            let bytes = roots
                .resources
                .into_iter()
                .map(|r| accounting.charge_resource(r))
                .fold(0usize, usize::saturating_add);
            if bytes as u64 > ProjectLimits::default().asset_bytes {
                return Err(DocumentError::InvalidLayerOperation(
                    "Effect resources exceed the project memory limit",
                ));
            }
        }
        if history_budget::Accounting::for_admission(&self.document).charge(&forward) > budget
            || history_budget::Accounting::for_admission(&candidate).charge(&inverse) > budget
        {
            return Err(DocumentError::InvalidLayerOperation(
                "This edit exceeds the Undo/Redo memory limit",
            ));
        }
        Ok((candidate, inverse))
    }
    fn perform_with_history_budget(
        &mut self,
        edit: Edit,
        budget: usize,
    ) -> Result<(), DocumentError> {
        fn empty(e: &Edit) -> bool {
            matches!(e,Edit::Batch(es) if es.iter().all(empty))
        }
        if empty(&edit) {
            return Ok(());
        }
        if edit.navigation(&self.document) {
            self.document.apply(edit)?;
            return Ok(());
        }
        let changes = edit.changes_project();
        let next = if changes {
            Some(self.next_checkpoint.checked_add(1).ok_or(
                DocumentError::InvalidLayerOperation("Document history exhausted"),
            )?)
        } else {
            None
        };
        let inverse = if edit.requires_history_admission(&self.document) {
            let (candidate, inverse) = self.prepare_history_edit(edit, budget)?;
            self.document = candidate;
            inverse
        } else {
            HistoryEntry::new(self.document.apply(edit)?, self.checkpoint)
        };
        self.undo.push(inverse);
        self.redo.clear();
        if let Some(next) = next {
            self.checkpoint = self.next_checkpoint;
            self.next_checkpoint = next;
        }
        self.trim_history(budget);
        Ok(())
    }
    fn trim_history(&mut self, budget: usize) {
        let mut accounting = history_budget::Accounting::new(&self.document);
        let mut bytes = 0usize;
        for history in [&mut self.undo, &mut self.redo] {
            let mut keep = 0;
            for entry in history.iter().rev().take(history_budget::ENTRY_BUDGET) {
                bytes = bytes.saturating_add(accounting.charge(entry));
                if bytes > budget {
                    break;
                }
                keep += 1;
            }
            history.drain(..history.len().saturating_sub(keep));
        }
    }
    pub fn undo(&mut self) -> Result<bool, DocumentError> {
        self.step(false)
    }
    pub fn redo(&mut self) -> Result<bool, DocumentError> {
        self.step(true)
    }
    fn step(&mut self, redo: bool) -> Result<bool, DocumentError> {
        let Some(edit) = self.next_history_edit(redo).cloned() else {
            return Ok(false);
        };
        let inverse = self.document.apply(edit)?;
        let (from, to) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        let entry = from.pop().unwrap();
        to.push(HistoryEntry::new(inverse, self.checkpoint));
        self.checkpoint = entry.checkpoint;
        Ok(true)
    }
    pub fn refine_selection(
        &mut self,
        target: SelectionTarget,
        coverage: Selection,
        revision: u64,
    ) -> Result<(), DocumentError> {
        let previous = self.last_selection_operation(target, revision)?;
        let edit = self.document.selection_edit(target, coverage)?;
        let (candidate, _) = self.prepare_history_edit(edit, history_budget::BYTE_BUDGET)?;
        if history_budget::Accounting::for_admission(&candidate).charge(previous)
            > history_budget::BYTE_BUDGET
        {
            return Err(DocumentError::InvalidLayerOperation(
                "This edit exceeds the Undo/Redo memory limit",
            ));
        }
        let next = if matches!(target, SelectionTarget::Saved(_)) {
            Some(self.next_checkpoint.checked_add(1).ok_or(
                DocumentError::InvalidLayerOperation("Document history exhausted"),
            )?)
        } else {
            None
        };
        self.document = candidate;
        if let Some(next) = next {
            self.checkpoint = self.next_checkpoint;
            self.next_checkpoint = next;
        }
        self.trim_history(history_budget::BYTE_BUDGET);
        Ok(())
    }
    pub fn withdraw_selection(
        &mut self,
        target: SelectionTarget,
        revision: u64,
    ) -> Result<(), DocumentError> {
        let edit = self
            .last_selection_operation(target, revision)?
            .edit
            .clone();
        self.document.apply(edit)?;
        self.checkpoint = self.undo.pop().unwrap().checkpoint;
        Ok(())
    }
    fn last_selection_operation(
        &self,
        target: SelectionTarget,
        revision: u64,
    ) -> Result<&HistoryEntry, DocumentError> {
        let changed = DocumentError::InvalidLayerOperation("The selection operation has changed");
        let entry = self
            .undo
            .last()
            .filter(|e| match (&e.edit, target) {
                (Edit::Working(_), SelectionTarget::Current) => true,
                (Edit::SavedSelection(c), SelectionTarget::Saved(h)) => {
                    self.document.scene().source_target(h)
                        == Some(SourceTarget::Selection(c.handle))
                }
                _ => false,
            })
            .ok_or(changed.clone())?;
        if self.document.revision != revision || !self.redo.is_empty() {
            return Err(changed);
        }
        Ok(entry)
    }
    pub fn recover_failed_rasters(&mut self) -> Result<usize, DocumentError> {
        fn failed(d: &Document) -> bool {
            let mut roots = RootInventory::default();
            roots.document(d);
            roots.rasters.iter().any(|r| r.failed())
        }
        if !failed(&self.document) {
            self.prune_failed_raster_history();
            return Ok(0);
        }
        let mut candidate = self.document.clone();
        let mut count = 0;
        let mut checkpoint = self.checkpoint;
        for entry in self.undo.iter().rev() {
            candidate.apply(entry.edit.clone())?;
            count += 1;
            checkpoint = entry.checkpoint;
            if !failed(&candidate) {
                break;
            }
        }
        if failed(&candidate) {
            return Err(DocumentError::InvalidLayerOperation(
                "No intact raster checkpoint remains in history",
            ));
        }
        self.document = candidate;
        self.checkpoint = checkpoint;
        self.undo.truncate(self.undo.len() - count);
        self.redo.clear();
        self.prune_failed_raster_history();
        Ok(count)
    }
    fn prune_failed_raster_history(&mut self) {
        for history in [&mut self.undo, &mut self.redo] {
            if let Some(index) = history.iter().rposition(|e| {
                let mut roots = Vec::new();
                e.edit.raster_roots(&mut roots);
                roots.iter().any(|r| r.failed())
            }) {
                history.drain(..=index);
            }
        }
    }
    pub fn finish_raster_submission(&mut self) {
        let paint: Vec<_> = self
            .document
            .artwork
            .paint
            .iter()
            .filter(|(_, _, s)| !s.operations.is_empty())
            .map(|(h, _, _)| h)
            .collect();
        for h in paint {
            self.document.artwork.paint.get_mut(h).unwrap().operations = Default::default();
        }
        let coverage: Vec<_> = self
            .document
            .artwork
            .coverage
            .iter()
            .filter(|(_, _, s)| !s.operations.is_empty())
            .map(|(h, _, _)| h)
            .collect();
        for h in coverage {
            self.document
                .artwork
                .coverage
                .get_mut(h)
                .unwrap()
                .operations = Default::default();
        }
    }
    pub fn amend_raster(
        &mut self,
        target: SourceTarget,
        revision: raster::RasterRevision,
    ) -> Result<(), DocumentError> {
        let next =
            self.next_checkpoint
                .checked_add(1)
                .ok_or(DocumentError::InvalidLayerOperation(
                    "Document history exhausted",
                ))?;
        self.document.apply(Edit::SetRaster { target, revision })?;
        self.checkpoint = self.next_checkpoint;
        self.next_checkpoint = next;
        Ok(())
    }
    pub fn preview(&mut self, edit: Edit) -> Result<(), DocumentError> {
        self.document.apply(edit).map(|_| ())
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DocumentError {
    InvalidRuler(&'static str),
    InvalidLayerOperation(&'static str),
    InvalidArtwork(String),
    MissingOccurrence(OccurrenceHandle),
    MissingTarget(SourceTarget),
    ProtectedOccurrence(OccurrenceHandle),
    NotDrawable(OccurrenceHandle),
    EmptyStroke,
    NonFiniteStroke,
    InvalidBrush(BrushError),
}
impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRuler(m) | Self::InvalidLayerOperation(m) => f.write_str(m),
            Self::InvalidArtwork(m) => f.write_str(m),
            Self::MissingOccurrence(h) => write!(f, "occurrence {} does not exist", h.index()),
            Self::MissingTarget(t) => write!(f, "source target {t:?} does not exist"),
            Self::ProtectedOccurrence(h) => write!(f, "occurrence {} is protected", h.index()),
            Self::NotDrawable(h) => write!(f, "occurrence {} cannot receive strokes", h.index()),
            Self::EmptyStroke => f.write_str("a stroke needs at least one point"),
            Self::NonFiniteStroke => f.write_str("stroke contains non-finite input"),
            Self::InvalidBrush(e) => write!(f, "invalid stroke brush: {e}"),
        }
    }
}
impl std::error::Error for DocumentError {}

impl From<&'static str> for DocumentError {
    fn from(value: &'static str) -> Self {
        Self::InvalidLayerOperation(value)
    }
}

fn artwork_metadata(artwork: &Artwork) -> usize {
    let fixed = std::mem::size_of::<Artwork>()
        .saturating_add(artwork.compositions.len() * std::mem::size_of::<Composition>())
        .saturating_add(artwork.paint.len() * std::mem::size_of::<PaintSource>())
        .saturating_add(artwork.coverage.len() * std::mem::size_of::<CoverageSource>())
        .saturating_add(artwork.selections.len() * std::mem::size_of::<SavedSelection>());
    let extra = artwork
        .stacks
        .iter()
        .map(|(_, _, v)| v.entries.len() * std::mem::size_of::<OccurrenceHandle>())
        .chain(
            artwork
                .occurrences
                .iter()
                .map(|(_, _, v)| std::mem::size_of::<Occurrence>() + v.name.len()),
        )
        .chain(
            artwork
                .effects
                .iter()
                .map(|(_, _, v)| json_len(&v.values).saturating_mul(4)),
        )
        .chain(artwork.guides.iter().map(|(_, _, v)| {
            v.rulers
                .iter()
                .map(|(_, g)| json_len(g).saturating_mul(4))
                .sum()
        }))
        .chain(artwork.outputs.iter().map(|(_, _, v)| {
            std::mem::size_of::<Output>() + v.name.len() + v.context.phases.len() * 16
        }))
        .fold(0usize, usize::saturating_add);
    fixed.saturating_add(extra).saturating_add(extension_metadata(&artwork.extensions))
}
fn extension_record_metadata(extensions:&authored::Extensions)->usize {
    extensions.records.values().map(|record|json_len(record).saturating_mul(4)).fold(0usize,usize::saturating_add)
}
fn opaque_resource_metadata(resource:&authored::OpaqueResource)->usize {
    std::mem::size_of::<authored::OpaqueResource>().saturating_add(resource.kind.len()).saturating_add(resource.encoding.len())
        .saturating_add(json_len(&resource.data).saturating_mul(4)).saturating_add(json_len(&resource.extra_fields).saturating_mul(4))
}
fn extension_metadata(extensions:&authored::Extensions)->usize {
    extensions.resources.values().fold(extension_record_metadata(extensions),|bytes,resource|bytes.saturating_add(opaque_resource_metadata(resource)))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn document(size: [u32; 2]) -> Document {
        Document::new(
            PortableId::random(),
            size[0],
            size[1],
            DocumentNames {
                paint: "Current ink".into(),
                paper: "Paper".into(),
            },
        )
    }
    fn opacity(document: &Document, h: OccurrenceHandle, value: f32) -> Edit {
        let mut o = document.artwork.occurrences.get(h).unwrap().clone();
        o.opacity = value;
        Edit::Occurrence(RecordChange::replace(&document.artwork.occurrences, h, Some(o)).unwrap())
    }
    fn blend(document: &Document, value: BlendSpace) -> Edit {
        let mut c = document.composition().clone();
        c.blend = value;
        Edit::Composition(
            RecordChange::replace(
                &document.artwork.compositions,
                document.artwork.root,
                Some(c),
            )
            .unwrap(),
        )
    }
    #[test]
    fn changing_the_blend_space_is_one_undo_step_that_keeps_every_pixel() {
        let d = document([64; 2]);
        let mut editor = Editor::new(d.clone());
        let edit = blend(&d, BlendSpace::Perceptual);
        assert!(edit.changes_image(&d));
        editor.perform(edit).unwrap();
        assert_eq!(
            editor.document().composition().blend,
            BlendSpace::Perceptual
        );
        assert_eq!(editor.document().artwork.paint, d.artwork.paint);
        editor.undo().unwrap();
        assert_eq!(editor.document().artwork, d.artwork);
        assert!(!editor.can_undo());
        let mut float = document([64; 2]);
        float
            .artwork
            .compositions
            .get_mut(float.artwork.root)
            .unwrap()
            .color
            .depth = color::SampleDepth::F16;
        let edit = blend(&float, BlendSpace::Perceptual);
        assert!(Editor::new(float).perform(edit).is_err());
    }
    fn point(pressure: f32) -> StrokePoint {
        StrokePoint {
            position: Point { x: 12., y: 14. },
            pressure,
            tilt: [0.; 2],
            twist: 0.,
            elapsed_micros: 0,
        }
    }
    #[test]
    fn stroke_clamps_pressure_and_precomputes_bounds() {
        let stroke = Stroke::new(
            StrokeId(1),
            SourceTarget::Paint(Handle::from_index(0)),
            StrokeTool::Brush,
            BrushSnapshot::default(),
            vec![point(2.)],
        )
        .unwrap();
        assert_eq!(stroke.points[0].pressure, 1.);
        assert!(!stroke.bounds.is_empty());
    }
    #[test]
    fn edit_history_restores_exact_revision_identity() {
        let mut editor = Editor::new(document([1024, 1536]));
        let target = editor.document().working.target.unwrap();
        let before = editor.document().target_raster(target).unwrap().clone();
        let after = raster::RasterRevision::pending();
        editor
            .perform(Edit::SetRaster {
                target,
                revision: after.clone(),
            })
            .unwrap();
        editor.undo().unwrap();
        assert_eq!(editor.document().target_raster(target), Some(&before));
        editor.redo().unwrap();
        assert_eq!(editor.document().target_raster(target), Some(&after));
        let snapshot = editor.document().clone();
        editor
            .amend_raster(target, raster::RasterRevision::pending())
            .unwrap();
        assert_eq!(snapshot.target_raster(target), Some(&after));
        editor.undo().unwrap();
        assert_eq!(editor.document().target_raster(target), Some(&before));
    }
    #[test]
    fn history_is_bounded_and_keeps_the_newest_exact_states() {
        let mut editor = Editor::new(document([256; 2]));
        let h = editor.document().working.occurrence.unwrap();
        for i in 0..400 {
            editor
                .perform(opacity(editor.document(), h, i as f32 / 400.))
                .unwrap();
        }
        assert_eq!(editor.undo.len(), 256);
        assert_eq!(
            editor
                .document()
                .artwork
                .occurrences
                .get(h)
                .unwrap()
                .opacity,
            399. / 400.
        );
        for _ in 0..256 {
            assert!(editor.undo().unwrap());
        }
        assert!(!editor.undo().unwrap());
        assert_eq!(
            editor
                .document()
                .artwork
                .occurrences
                .get(h)
                .unwrap()
                .opacity,
            143. / 400.
        );
        for _ in 0..256 {
            assert!(editor.redo().unwrap());
        }
        assert_eq!(
            editor
                .document()
                .artwork
                .occurrences
                .get(h)
                .unwrap()
                .opacity,
            399. / 400.
        );
        let target = editor.document().working.target.unwrap();
        for _ in 0..8 {
            editor
                .perform(Edit::SetRaster {
                    target,
                    revision: raster::RasterRevision::pending(),
                })
                .unwrap();
        }
        assert!(editor.undo.len() <= 2);
    }
    #[test]
    fn project_checkpoint_tracks_undo_branches_not_navigation() {
        let mut editor = Editor::new(document([64; 2]));
        let initial = editor.checkpoint();
        let target = editor.document().working.target.unwrap();
        editor
            .perform(Edit::SetRaster {
                target,
                revision: raster::RasterRevision::pending(),
            })
            .unwrap();
        let saved = editor.checkpoint();
        assert_ne!(initial, saved);
        let mut working = editor.document().working.clone();
        working.occurrence = Some(editor.document().scene().order()[1]);
        working.target = None;
        editor.perform(Edit::Working(working)).unwrap();
        let mut working = editor.document().working.clone();
        working.selection = Some(Selection::full());
        editor.perform(Edit::Working(working)).unwrap();
        assert_eq!(editor.checkpoint(), saved);
        editor.undo().unwrap();
        assert_eq!(editor.checkpoint(), saved);
        editor.undo().unwrap();
        assert_eq!(editor.checkpoint(), initial);
        editor.redo().unwrap();
        assert_eq!(editor.checkpoint(), saved);
        editor.undo().unwrap();
        let h = editor.document().scene().order()[0];
        let mut occurrence = editor
            .document()
            .artwork
            .occurrences
            .get(h)
            .unwrap()
            .clone();
        occurrence.reference = true;
        editor
            .perform(Edit::Occurrence(
                RecordChange::replace(&editor.document().artwork.occurrences, h, Some(occurrence))
                    .unwrap(),
            ))
            .unwrap();
        assert_ne!(editor.checkpoint(), saved);
        assert_ne!(editor.checkpoint(), initial);
    }
    #[test]
    fn saved_selection_navigation_keeps_authored_visibility_and_restores_working_overrides() {
        let mut d=document([64;2]);let paint=d.working.occurrence.unwrap();
        let saved=RecordChange::insert(&d.artwork.selections,SavedSelection {selection:Selection::full(),display:Default::default()});
        let mut value=Occurrence::new(OccurrenceContent::Selection(saved.handle),"Saved coverage");value.visible=false;
        let occurrence=RecordChange::insert(&d.artwork.occurrences,value);let handle=occurrence.handle;
        let stack=d.composition().result;let mut membership=d.artwork.stacks.get(stack).unwrap().clone();membership.entries.insert(0,handle);
        let membership=RecordChange::replace(&d.artwork.stacks,stack,Some(membership)).unwrap();
        d.apply(Edit::Batch(vec![Edit::SavedSelection(saved),Edit::Occurrence(occurrence),Edit::Stack(membership)])).unwrap();
        let mut editor=Editor::new(d);editor.perform(opacity(editor.document(),paint,0.7)).unwrap();editor.undo().unwrap();
        let authored=editor.document().artwork.clone();let checkpoint=editor.checkpoint();
        editor.perform(editor.document().select_occurrence_edit(handle).unwrap()).unwrap();
        assert!(editor.document().effective_visibility(handle));
        assert!(!editor.document().artwork.occurrences.get(handle).unwrap().visible);
        assert_eq!(editor.document().artwork,authored);assert_eq!(editor.checkpoint(),checkpoint);assert!(editor.can_redo());assert!(!editor.can_undo());
        assert_eq!(*editor.capture(0,EvaluationContext::default()).unwrap().artwork,authored);
        let mut working=editor.document().working.clone();working.selection_visibility.insert(paint,false);
        editor.perform(Edit::Working(working)).unwrap();assert!(!editor.document().working.selection_visibility.contains_key(&paint));
        editor.perform(editor.document().select_occurrence_edit(paint).unwrap()).unwrap();
        assert!(!editor.document().effective_visibility(handle));assert_eq!(editor.document().artwork,authored);assert_eq!(editor.checkpoint(),checkpoint);assert!(editor.can_redo());
        editor.perform(editor.document().delete_layers_edit(&[handle]).unwrap()).unwrap();
        assert!(!editor.document().working.selection_visibility.contains_key(&handle));
        editor.undo().unwrap();
        assert_eq!(editor.document().working.selection_visibility.get(&handle),Some(&false));
        assert_eq!(editor.document().artwork,authored);
    }
    #[test]
    fn every_occurrence_can_be_deleted_and_restored() {
        let mut d = document([800; 2]);
        let initial = d.artwork.clone();
        let active = d.working.clone();
        let occurrences = d.scene().order().to_vec();
        let paint = d.apply(d.delete_layers_edit(&[occurrences[0]]).unwrap()).unwrap();
        assert_eq!(d.working.occurrence,Some(occurrences[1]));
        let paper = d.apply(d.delete_layers_edit(&[occurrences[1]]).unwrap()).unwrap();
        assert!(d.scene().order().is_empty());
        assert_eq!(d.working.occurrence,None);
        assert_eq!(d.working.target,None);
        d.apply(paper).unwrap();
        d.apply(paint).unwrap();
        assert_eq!(d.artwork,initial);
        assert_eq!(d.working.occurrence,active.occurrence);
        assert_eq!(d.working.target,active.target);
    }
    #[test]
    fn batch_admission_is_atomic_and_rebuilds_scene_once() {
        let mut d = document([64; 2]);
        let before = d.clone();
        let h = d.scene().order()[0];
        let invalid = RecordChange {
            handle: h,
            id: PortableId::random(),
            value: None,
        };
        assert!(
            d.apply(Edit::Batch(vec![
                opacity(&d, h, 0.3),
                Edit::Occurrence(invalid)
            ]))
            .is_err()
        );
        assert_eq!(d, before);
        let index = d.scene_index.clone();
        d.apply(opacity(&d, h, 0.3)).unwrap();
        assert!(Arc::ptr_eq(&d.scene_index, &index));
    }
    #[test]
    fn foreign_same_identity_documents_have_distinct_runtime_owners_and_capture_roots() {
        let mut editor = Editor::new(document([64; 2]));
        let first = editor.capture(7, EvaluationContext::default()).unwrap();
        assert!(first.artwork.paint.same_root(&editor.document().artwork.paint));
        assert_eq!(first.checkpoint.owner,editor.document().owner);
        let other = Document::from_artwork(editor.document().artwork.clone()).unwrap();
        assert_eq!(editor.document().artwork.id, other.artwork.id);
        assert_ne!(editor.document().owner, other.owner);
        let target = editor.document().working.target.unwrap();
        editor
            .perform(Edit::SetRaster {
                target,
                revision: raster::RasterRevision::pending(),
            })
            .unwrap();
        let SourceTarget::Paint(h) = target else {
            panic!()
        };
        assert_ne!(
            first.artwork.paint.get(h).unwrap().raster,
            *editor.document().target_raster(target).unwrap()
        );
        assert_ne!(first.checkpoint.edit_checkpoint, editor.checkpoint());
        assert!(other.working.selection.is_none());
    }
    #[test]
    fn canvas_origin_history_uses_relative_signed_displacements() {
        let mut d=document([64;2]);
        for shift in [[11.,-5.],[7.,3.]] {
            let before=d.composition().origin;
            let mut composition=d.composition().clone();
            composition.origin=Point{x:before.x+shift[0],y:before.y+shift[1]};
            let edit=Edit::Composition(RecordChange::replace(&d.artwork.compositions,d.artwork.root,Some(composition)).unwrap());
            assert_eq!(edit.canvas_origin_from(before),Some(shift.map(|v|v as i32)));
            let inverse=d.apply(edit).unwrap();
            assert_eq!(inverse.canvas_origin_from(d.composition().origin),Some(shift.map(|v|-v as i32)));
        }
        assert_eq!(blend(&d,BlendSpace::Perceptual).canvas_origin_from(d.composition().origin),None);
    }
    #[test]
    fn brush_colors_preserve_finite_extended_rgb_and_validate_coverage_separately() {
        let mut brush = BrushSnapshot { color_rgba_linear: [-0.3, 1.4, 0.7, 0.37], ..Default::default() };
        brush.color_dynamics.secondary_color_rgba_linear = [1.2, -0.1, 0.8, 1. / 65535.];
        brush.validate().unwrap();
        for secondary in [false, true] {
            for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut bad = brush.clone();
                if secondary {
                    bad.color_dynamics.secondary_color_rgba_linear[0] = invalid;
                } else {
                    bad.color_rgba_linear[0] = invalid;
                }
                assert!(bad.validate().is_err());
            }
            for invalid in [-0.01, 1.01, f32::NAN] {
                let mut bad = brush.clone();
                if secondary {
                    bad.color_dynamics.secondary_color_rgba_linear[3] = invalid;
                } else {
                    bad.color_rgba_linear[3] = invalid;
                }
                assert!(bad.validate().is_err());
            }
        }
    }
}
#[cfg(test)]
mod retained_geometry_tests;
