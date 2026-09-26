//! Portable, renderer-agnostic document model for Layer.
//!
//! This crate contains no window, graphics API, inference runtime, async
//! executor, or window types. Immutable raster revisions back committed edits,
//! undo/redo and project snapshots. Live contacts retain bounded shared samples.

#[cfg(unix)]
mod atomic_file;
#[cfg(unix)]
pub use atomic_file::{atomic_write, atomic_write_checked};

pub mod color;
pub mod binary_payload;
mod image_metadata;
pub use image_metadata::{ImageResolution, ResolutionUnit};
mod contact;
pub use contact::BrushContact;
mod contact_presets;
pub use contact_presets::CONTACT_BRUSH_PRESETS;
mod effect_catalog;
mod effects;
pub mod raster;
pub mod raster_storage;
pub use effect_catalog::*;
mod layers;
mod selection;
pub mod tonal;
pub use selection::*;
pub use effects::*;
mod presets;
pub use layers::*;
mod figures;
pub use figures::{Figure, FigurePaint, FigureShape};
mod rulers;
pub use rulers::{Ruler, RulerConstraint, RulerGeometry, RulerKind, choose_ruler};
mod affine;
pub use affine::{Affine, ImageTransform, Interpolation, TransformMap};
mod projective;
pub use projective::Projective;
mod warp;
pub use warp::MeshMap;
mod project;
mod project_storage;
pub use project_storage::SelectionIndex as ProjectSelections;
mod history_budget;
mod color_edit;
mod color_history;
pub use color_history::{ColorTransition, PreparedColorTransition};
pub use project::{Project, ProjectAsset, ProjectAssetFormat, ProjectLimits};

pub use presets::{
    CONTACT_PAPER_TEXTURE_ASSET, DefaultBrushPreset,
    PAPER_GRAIN_TEXTURE_ASSET,
    WATERCOLOR_TIP_TEXTURE_ASSET, WATERCOLOR_TRANSPORT_LONG_BROAD_ASSET,
    WATERCOLOR_TRANSPORT_LONG_NARROW_ASSET, WATERCOLOR_TRANSPORT_SHORT_BROAD_ASSET,
    WATERCOLOR_TRANSPORT_SHORT_NARROW_ASSET, default_brush,
};

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
};

pub type Revision = u64;

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
pub struct LayerId(pub u64);

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
    Background,
    Group,
    Effect,
    /// Named reusable coverage; never participates in artwork composition.
    Selection,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Layer {
    pub id: LayerId,
    pub name: Arc<str>,
    pub kind: LayerKind,
    pub visible: bool,
    pub opacity: f32,
    /// Exact committed pixels, shared with undo and in-flight save snapshots.
    /// The indexed project container serializes backing separately from metadata.
    #[serde(skip)]
    pub raster: raster::RasterRevision,
    /// Immutable original samples and interpretation, shared by duplication,
    /// history and save snapshots. Raster tiles override edited source regions.
    /// The indexed project container stores source/profile payload separately.
    #[serde(skip)]
    pub source: Option<Arc<color::source::SourceImage>>,
    pub asset: Option<AssetId>,
    pub properties: LayerProperties,
    pub mask: Option<LayerMask>,
    #[serde(skip)]
    pub pending_operations: Vec<LayerOperation>,
    pub effect: Option<Arc<EffectInstance>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selection>,
}

impl Layer {
    fn selection_roots<'a>(&'a self, out: &mut Vec<&'a Selection>) {
        out.extend(self.selection.iter());
        for mask in self.mask.iter().chain(self.pending_operations.iter().map(|op| &op.coverage)) {
            out.extend(mask.initial.iter());
            for op in mask.pending_operations.iter() { out.extend(op.coverage.initial.iter()); }
        }
    }
    /// Composition metadata without copying immutable paint history.
    pub fn composite_snapshot(&self) -> Self {
        Self {
            id: self.id,
            name: "".into(),
            kind: self.kind,
            visible: self.visible,
            opacity: self.opacity,
            raster: self.raster.clone(),
            source: self.source.clone(),
            asset: self.asset.clone(),
            properties: self.properties.clone(),
            mask: self.mask.clone(),
            pending_operations: Vec::new(),
            effect: self.effect.clone(),
            selection: self.selection.clone(),
        }
    }
    pub fn paint(id: LayerId, name: impl Into<Arc<str>>) -> Self {
        Self {
            id,
            name: name.into(),
            kind: LayerKind::Paint,
            visible: true,
            opacity: 1.0,
            raster: Default::default(),
            source: None,
            asset: None,
            properties: LayerProperties::default(),
            mask: None,
            pending_operations: Vec::new(),
            effect: None,
            selection: None,
        }
    }
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
pub enum DualCombineMode {
    #[default]
    Multiply,
    Add,
    Subtract,
    Difference,
    Min,
    Max,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum ColorMixSpace {
    #[default]
    LinearRgb,
    Oklab,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum LiquifyMode {
    #[default]
    Push,
    TwirlClockwise,
    TwirlCounterClockwise,
    Pinch,
    Expand,
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
    #[serde(default)]
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
#[serde(default)]
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

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DualBrush {
    pub tip: BrushTip,
    pub grain: Option<BrushGrain>,
    pub combine: DualCombineMode,
    pub scale: f32,
    pub aspect: f32,
    pub angle_radians: f32,
    pub offset: [f32; 2],
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
    pub schema_version: u16,
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
    pub dual: Option<Arc<DualBrush>>,
    pub color_dynamics: BrushColorDynamics,
    pub rendering: BrushRendering,
    pub wet_mix: BrushWetMix,
    pub transport: Option<BrushTransport>,
    pub deform: BrushDeform,
    pub bounds: BrushBounds,
    /// Optional coherent GPU contact model. Omitted in legacy snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact: Option<BrushContact>,
}

impl Default for BrushSnapshot {
    fn default() -> Self {
        Self {
            schema_version: 4,
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
            dual: None,
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
        if !matches!(self.schema_version, 4 | 5)
            || (self.schema_version == 4 && self.contact.is_some())
            || base.iter().any(|value| !value.is_finite())
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
        if let Some(dual) = &self.dual {
            let values = [
                dual.scale,
                dual.aspect,
                dual.angle_radians,
                dual.offset[0],
                dual.offset[1],
            ];
            if values.iter().any(|value| !value.is_finite())
                || !(0.01..=16.0).contains(&dual.scale)
                || !(0.02..=50.0).contains(&dual.aspect)
            {
                return Err(BrushError::InvalidAdvanced);
            }
            if let Some(grain) = &dual.grain {
                validate_grain(grain)?;
            }
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
            Self::InvalidBase => formatter.write_str("invalid brush base settings or schema"),
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
    pub layer_id: LayerId,
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
    /// Immutable layer-local coverage captured at stroke start, including for
    /// mask painting. Later selection edits must not change stroke replay.
    pub selection: Option<Arc<Selection>>,
}

impl Stroke {
    pub fn new(
        id: StrokeId,
        layer_id: LayerId,
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
            layer_id,
            tool,
            brush,
            points,
            material_updates: Arc::default(),
            bounds,
            alpha_locked: false,
            selection: None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Document {
    pub id: Arc<str>,
    pub width: u32,
    pub height: u32,
    pub color: color::DocumentColor,
    pub resolution: Option<ImageResolution>,
    /// Saved independently of delivery, with binary profile bytes deduplicated
    /// by the project source/profile index. Temporary view toggles live in UI.
    #[serde(skip)]
    pub proof: Option<color::ProofRecipe>,
    /// Authored delivery mapping. Display capability and preview toggles are view state.
    pub sdr_rendition: color::hdr::SdrRendition,
    /// Front-to-back display order.
    pub layers: Vec<Layer>,
    pub active_layer: LayerId,
    pub active_mask: bool,
    pub selection: Option<Selection>,
    pub reference_layers: BTreeSet<LayerId>,
    /// Global, non-raster guides; edits are durable and share document undo.
    pub rulers: Vec<Ruler>,
    pub revision: Revision,
    next_layer_id: u64,
    next_stroke_id: u64,
}

impl Document {
    pub fn has_animated_effects(&self) -> bool {
        self.layers.iter().any(|l| {
            if !l.visible || !l.effect.as_ref().is_some_and(|e| e.animated()) {
                return false;
            }
            let mut parent = l.properties.parent;
            while let Some(id) = parent {
                let Some(group) = self.layer(id) else {
                    return false;
                };
                if !group.visible {
                    return false;
                }
                parent = group.properties.parent;
            }
            true
        })
    }
    pub fn new(id: impl Into<Arc<str>>, width: u32, height: u32) -> Self {
        let paint_id = LayerId(1);
        Self {
            id: id.into(),
            width,
            height,
            color: color::DocumentColor::default(),
            resolution: None,
            proof: None,
            sdr_rendition: Default::default(),
            layers: vec![
                Layer::paint(paint_id, "Current ink"),
                Layer {
                    id: LayerId(2),
                    name: Arc::from("Paper"),
                    kind: LayerKind::Background,
                    visible: true,
                    opacity: 1.0,
                    raster: Default::default(),
                    source: None,
                    asset: None,
                    properties: LayerProperties::default(),
                    mask: None,
                    pending_operations: Vec::new(),
                    effect: None,
                    selection: None,
                },
            ],
            active_layer: paint_id,
            active_mask: false,
            selection: None,
            reference_layers: BTreeSet::new(),
            rulers: Vec::new(),
            revision: 0,
            next_layer_id: 3,
            next_stroke_id: 1,
        }
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|layer| layer.id == id)
    }

    pub fn allocate_layer_id(&mut self) -> LayerId {
        let id = LayerId(self.next_layer_id);
        self.next_layer_id = self.next_layer_id.saturating_add(1);
        id
    }

    pub fn allocate_stroke_id(&mut self) -> StrokeId {
        let id = StrokeId(self.next_stroke_id);
        self.next_stroke_id = self.next_stroke_id.saturating_add(1);
        id
    }

    /// Read-only identity for planning an insertion before admission.
    pub fn next_layer_id(&self) -> LayerId {
        LayerId(self.next_layer_id)
    }

    /// Read-only identity for a provisional next-contact cursor.
    pub fn next_stroke_id(&self) -> StrokeId {
        StrokeId(self.next_stroke_id)
    }

    /// Applies one reversible edit and returns its exact inverse.
    pub fn apply(&mut self, edit: Edit) -> Result<Edit, DocumentError> {
        let inverse = match edit {
            Edit::SetColor { color, layers } => self.apply_color_edit(color, layers)?,
            Edit::SetSdrRendition(recipe) => {
                recipe.validate().map_err(|_| DocumentError::InvalidLayerOperation("Invalid SDR rendition"))?;
                Edit::SetSdrRendition(std::mem::replace(&mut self.sdr_rendition, recipe))
            }
            Edit::SetProof(recipe) => {
                if let Some(recipe) = &recipe {
                    recipe.validate().map_err(|_| DocumentError::InvalidLayerOperation("Invalid proof recipe"))?;
                    if let color::ColorProfile::Icc(bytes) = &recipe.profile
                        && (bytes.is_empty() || bytes.len() > color::source::MAX_PROFILE_BYTES)
                    {
                        return Err(DocumentError::InvalidLayerOperation("Invalid proof profile size"));
                    }
                }
                Edit::SetProof(std::mem::replace(&mut self.proof, recipe))
            }
            Edit::SetRaster { target, revision } => {
                let raster = self
                    .target_raster_mut(target)
                    .ok_or(DocumentError::MissingLayer(target))?;
                Edit::SetRaster {
                    target,
                    revision: std::mem::replace(raster, revision),
                }
            }
            Edit::Batch(edits) => {
                let before = self.clone();
                let mut inverses = Vec::with_capacity(edits.len());
                for edit in edits {
                    match self.apply(edit) {
                        Ok(inverse) => inverses.push(inverse),
                        Err(error) => {
                            *self = before;
                            return Err(error);
                        }
                    }
                }
                inverses.reverse();
                Edit::Batch(inverses)
            }
            Edit::ReplaceLayer(layer) => {
                self.validate_layer(&layer)?;
                let id = layer.id;
                let restore_mask_target =
                    self.active_mask && self.active_layer == layer.id && layer.mask.is_none();
                let target = self
                    .layers
                    .iter_mut()
                    .find(|l| l.id == layer.id)
                    .ok_or(DocumentError::MissingLayer(layer.id))?;
                let inverse = Edit::ReplaceLayer(Box::new(std::mem::replace(target, *layer)));
                if restore_mask_target {
                    self.active_mask = false;
                    Edit::Batch(vec![
                        inverse,
                        Edit::SetActiveLayer { id },
                        Edit::SetMaskTarget(true),
                    ])
                } else {
                    inverse
                }
            }
            Edit::SetMaskTarget(active) => {
                if active
                    && self
                        .layer(self.active_layer)
                        .is_none_or(|l| l.mask.is_none())
                {
                    return Err(DocumentError::InvalidLayerOperation(
                        "This layer has no mask",
                    ));
                }
                if !active
                    && let Some(mask) = self
                        .layers
                        .iter_mut()
                        .find(|l| l.id == self.active_layer)
                        .and_then(|l| l.mask.as_mut())
                {
                    mask.show_area = false;
                }
                Edit::SetMaskTarget(std::mem::replace(&mut self.active_mask, active))
            }
            Edit::SetSelection(selection) => {
                if let Some(selection) = &selection { selection.validate()?; }
                Edit::SetSelection(std::mem::replace(&mut self.selection, selection))
            }
            Edit::SetSavedSelection { id, selection } => {
                selection.validate()?;
                let layer = self.layers.iter_mut().find(|l| l.id == id)
                    .ok_or(DocumentError::MissingLayer(id))?;
                if layer.kind != LayerKind::Selection {
                    return Err(DocumentError::InvalidLayerOperation("Choose a Selection Layer"));
                }
                let coverage = layer.selection.as_mut()
                    .ok_or(DocumentError::InvalidLayerOperation("Selection Layer has no coverage"))?;
                let previous = std::mem::replace(coverage, selection);
                Edit::SetSavedSelection { id, selection: previous }
            }
            Edit::SetRulers(rulers) => {
                rulers::validate_rulers(&rulers)?;
                Edit::SetRulers(std::mem::replace(&mut self.rulers, rulers))
            }
            Edit::SetReferences(references) => {
                for &id in &references {
                    let layer = self.layer(id).ok_or(DocumentError::MissingLayer(id))?;
                    if !matches!(layer.kind, LayerKind::Paint | LayerKind::Group) {
                        return Err(DocumentError::NotDrawable(id));
                    }
                }
                Edit::SetReferences(std::mem::replace(&mut self.reference_layers, references))
            }
            Edit::InsertLayer { index, layer } => {
                if self.layer(layer.id).is_some() {
                    return Err(DocumentError::DuplicateLayer(layer.id));
                }
                let id = layer.id;
                self.validate_layer(&layer)?;
                if self.layers.is_empty() { self.active_layer = id; self.active_mask = false; }
                let bottom = self
                    .layers
                    .iter()
                    .position(|l| l.kind == LayerKind::Background)
                    .unwrap_or(self.layers.len());
                self.layers.insert(index.min(bottom), layer);
                Edit::RemoveLayer { id }
            }
            Edit::RemoveLayer { id } => {
                let index = self
                    .layers
                    .iter()
                    .position(|layer| layer.id == id)
                    .ok_or(DocumentError::MissingLayer(id))?;
                let removed = self.layers.remove(index);
                let selected = self.active_layer == id;
                let mask_selected = self.active_mask;
                let references = self.reference_layers.clone();
                let reference = self.reference_layers.remove(&id);
                if self.active_layer == id {
                    self.active_mask = false;
                    self.active_layer = self
                        .layers
                        .iter()
                        .find(|layer| layer.kind == LayerKind::Paint)
                        .map(|layer| layer.id)
                        .or_else(|| self.layers.first().map(|layer| layer.id))
                        .unwrap_or(LayerId(0));
                }
                let mut inverse = vec![Edit::InsertLayer {
                    index,
                    layer: removed,
                }];
                if selected {
                    inverse.extend([
                        Edit::SetActiveLayer { id },
                        Edit::SetMaskTarget(mask_selected),
                    ]);
                }
                if reference {
                    inverse.push(Edit::SetReferences(references));
                }
                Edit::Batch(inverse)
            }
            Edit::MoveLayer { id, to } => {
                let from = self
                    .layers
                    .iter()
                    .position(|layer| layer.id == id)
                    .ok_or(DocumentError::MissingLayer(id))?;
                if self.layers[from].kind == LayerKind::Background {
                    return Err(DocumentError::ProtectedLayer(id));
                }
                let layer = self.layers.remove(from);
                let bottom = self
                    .layers
                    .iter()
                    .position(|l| l.kind == LayerKind::Background)
                    .unwrap_or(self.layers.len());
                self.layers.insert(to.min(bottom), layer);
                Edit::MoveLayer { id, to: from }
            }
            Edit::SetLayerOpacity { id, opacity } => {
                if !opacity.is_finite() {
                    return Err(DocumentError::InvalidLayerOperation("Invalid opacity"));
                }
                let layer = self
                    .layers
                    .iter_mut()
                    .find(|layer| layer.id == id)
                    .ok_or(DocumentError::MissingLayer(id))?;
                let previous = layer.opacity;
                if layer.kind == LayerKind::Selection {
                    return Err(DocumentError::InvalidLayerOperation("Selection Layers have no artwork opacity"));
                }
                layer.opacity = opacity.clamp(0.0, 1.0);
                Edit::SetLayerOpacity {
                    id,
                    opacity: previous,
                }
            }
            Edit::SetLayerVisibility { id, visible } => {
                let layer = self
                    .layers
                    .iter_mut()
                    .find(|layer| layer.id == id)
                    .ok_or(DocumentError::MissingLayer(id))?;
                let previous = layer.visible;
                layer.visible = visible;
                Edit::SetLayerVisibility {
                    id,
                    visible: previous,
                }
            }
            Edit::SetActiveLayer { id } => {
                // Selection is not permission to paint (Paper has properties too).
                self.layer(id).ok_or(DocumentError::MissingLayer(id))?;
                let previous = self.active_layer;
                let mask = self.active_mask;
                self.active_mask = false;
                self.active_layer = id;
                for layer in &mut self.layers {
                    // Selection overlays follow the drawing target, like the
                    // visibility-mask preview below. Navigation has no undo step.
                    if layer.kind == LayerKind::Selection {
                        if layer.id == id {
                            layer.visible = true;
                        } else if layer.id == previous {
                            layer.visible = false;
                        }
                    }
                    if layer.id != id
                        && let Some(mask) = &mut layer.mask
                    {
                        mask.show_area = false;
                    }
                }
                Edit::Batch(vec![
                    Edit::SetActiveLayer { id: previous },
                    Edit::SetMaskTarget(mask),
                ])
            }
        };
        self.revision = self.revision.saturating_add(1);
        Ok(inverse)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// Metadata only; no raster conversion or composite invalidation.
    SetProof(Option<color::ProofRecipe>),
    SetSdrRendition(color::hdr::SdrRendition),
    /// One atomic interpretation/backing change. Structure and properties stay
    /// intact; all native color and scalar replacements must be host-backed.
    SetColor {
        color: color::DocumentColor,
        layers: Vec<Layer>,
    },
    SetRaster {
        target: LayerId,
        revision: raster::RasterRevision,
    },
    Batch(Vec<Edit>),
    ReplaceLayer(Box<Layer>),
    SetMaskTarget(bool),
    SetSelection(Option<Selection>),
    SetSavedSelection { id: LayerId, selection: Selection },
    SetReferences(BTreeSet<LayerId>),
    SetRulers(Vec<Ruler>),
    InsertLayer {
        index: usize,
        layer: Layer,
    },
    RemoveLayer {
        id: LayerId,
    },
    MoveLayer {
        id: LayerId,
        to: usize,
    },
    SetLayerOpacity {
        id: LayerId,
        opacity: f32,
    },
    SetLayerVisibility {
        id: LayerId,
        visible: bool,
    },
    SetActiveLayer {
        id: LayerId,
    },
}

impl Edit {
    /// Final interpretation after this transaction, including ordered batches.
    /// Hosts prepare the matching renderer before publishing color history.
    pub fn resulting_color(&self, current: color::DocumentColor) -> color::DocumentColor {
        match self {
            Self::SetColor { color, .. } => *color,
            Self::Batch(edits) => edits.iter().fold(current, |color, edit| edit.resulting_color(color)),
            _ => current,
        }
    }

    fn requires_history_admission(&self, document: &Document) -> bool {
        match self {
            Self::SetColor { .. } | Self::SetProof(_) | Self::SetSdrRendition(_) => true,
            Self::SetSavedSelection { .. } => true,
            Self::Batch(edits) => edits.iter().any(|e| e.requires_history_admission(document)),
            Self::InsertLayer { layer, .. } => layer.source.is_some() || layer.selection.is_some(),
            Self::RemoveLayer { id } => document.layer(*id).is_some_and(|l| l.source.is_some() || l.selection.is_some()),
            Self::ReplaceLayer(layer) => {
                if layer.selection.is_some() || document.layer(layer.id).is_some_and(|l| l.selection.is_some()) { return true; }
                match (document.layer(layer.id).and_then(|l| l.source.as_ref()), &layer.source) {
                    (None, None) => false,
                    (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
                    _ => true,
                }
            }
            _ => false,
        }
    }

    fn only_raster_updates(&self) -> bool {
        match self {
            Self::SetRaster { .. } => true,
            Self::Batch(edits) => !edits.is_empty() && edits.iter().all(Self::only_raster_updates),
            _ => false,
        }
    }
    fn source_roots<'a>(&'a self, out: &mut Vec<&'a Arc<color::source::SourceImage>>) {
        match self {
            Self::SetColor { layers, .. } => out.extend(layers.iter().filter_map(|l| l.source.as_ref())),
            Self::Batch(edits) => edits.iter().for_each(|edit| edit.source_roots(out)),
            Self::ReplaceLayer(layer) => out.extend(layer.source.as_ref()),
            Self::InsertLayer { layer, .. } => out.extend(layer.source.as_ref()),
            _ => (),
        }
    }
    fn selection_roots<'a>(&'a self, out: &mut Vec<&'a Selection>) {
        match self {
            Self::SetSelection(selection) => out.extend(selection.iter()),
            Self::SetSavedSelection { selection, .. } => out.push(selection),
            Self::ReplaceLayer(layer) => layer.selection_roots(out),
            Self::InsertLayer { layer, .. } => layer.selection_roots(out),
            Self::SetColor { layers, .. } => layers.iter().for_each(|l| l.selection_roots(out)),
            Self::Batch(edits) => edits.iter().for_each(|edit| edit.selection_roots(out)),
            _ => (),
        }
    }
    fn raster_roots<'a>(&'a self, out: &mut Vec<&'a raster::RasterRevision>) {
        match self {
            Self::SetColor { layers, .. } => {
                for layer in layers {
                    out.push(&layer.raster);
                    out.extend(layer.masks().map(|m| &m.raster));
                }
            }
            Self::SetRaster { revision, .. } => out.push(revision),
            Self::Batch(edits) => edits.iter().for_each(|edit| edit.raster_roots(out)),
            Self::ReplaceLayer(layer) => {
                out.push(&layer.raster);
                out.extend(layer.masks().map(|m| &m.raster));
            }
            Self::InsertLayer { layer, .. } => {
                out.push(&layer.raster);
                out.extend(layer.masks().map(|m| &m.raster));
            }
            _ => (),
        }
    }
    /// Navigation and selection are retained by a project snapshot, but do not
    /// themselves make artwork unsaved. Rulers and references are document edits.
    fn changes_project(&self) -> bool {
        match self {
            Self::SetActiveLayer { .. } | Self::SetMaskTarget(_) | Self::SetSelection(_) => false,
            Self::Batch(edits) => edits.iter().any(Self::changes_project),
            _ => true,
        }
    }
    /// Guide-only edits affect presentation, never committed raster pixels.
    pub fn changes_image(&self) -> bool {
        match self {
            Self::SetRulers(_) | Self::SetProof(_) | Self::SetSdrRendition(_)
                | Self::SetSavedSelection { .. } => false,
            Self::Batch(edits) => edits.iter().any(Self::changes_image),
            _ => true,
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

#[derive(Debug)]
struct HistoryEntry {
    edit: Edit,
    checkpoint: u64,
    metadata_bytes: usize,
}
impl HistoryEntry {
    fn new(edit: Edit, checkpoint: u64) -> Self {
        fn serialized(value: &impl serde::Serialize) -> usize {
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
            if serde_json::to_writer(&mut count, value).is_err() {
                return usize::MAX;
            }
            // Conservative allowance for allocations/nodes and binary scalars;
            // shared metadata is charged repeatedly rather than undercounted.
            count.0.saturating_mul(4)
        }
        fn layer_metadata(layer: &Layer) -> usize {
            let mut metadata = layer.clone();
            metadata.selection = None; // Shared coverage is charged by identity.
            if let Some(mask) = &mut metadata.mask { mask.initial = None; }
            serialized(&metadata)
        }
        fn size(edit: &Edit) -> usize {
            std::mem::size_of::<Edit>().saturating_add(match edit {
                Edit::Batch(edits) => edits.iter().map(size).fold(0usize, usize::saturating_add),
                Edit::SetColor { layers, .. } => layers.iter().map(layer_metadata).fold(0usize, usize::saturating_add),
                Edit::ReplaceLayer(layer) => layer_metadata(layer),
                Edit::InsertLayer { layer, .. } => layer_metadata(layer),
                Edit::SetSelection(_) | Edit::SetSavedSelection { .. } => std::mem::size_of::<Selection>(),
                Edit::SetRulers(rulers) => serialized(rulers),
                Edit::SetReferences(ids) => serialized(ids),
                Edit::SetProof(recipe) => recipe.as_ref().map_or(0, |recipe| {
                    recipe.name.len().saturating_add(match &recipe.profile {
                        color::ColorProfile::Builtin(_) => 0,
                        color::ColorProfile::Icc(bytes) => bytes.len(),
                    })
                }),
                _ => 0,
            })
        }
        let metadata_bytes = size(&edit);
        Self {
            edit,
            checkpoint,
            metadata_bytes,
        }
    }
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

    /// Identity of the current persistent undo state, not a monotonic revision.
    /// A host can save this token with a snapshot while later edits continue.
    pub fn checkpoint(&self) -> u64 {
        self.checkpoint
    }

    pub fn allocate_layer_id(&mut self) -> LayerId {
        self.document.allocate_layer_id()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_color(&self) -> color::DocumentColor {
        self.undo.last().map_or(self.document.color, |entry| entry.edit.resulting_color(self.document.color))
    }

    pub fn redo_color(&self) -> color::DocumentColor {
        self.redo.last().map_or(self.document.color, |entry| entry.edit.resulting_color(self.document.color))
    }

    pub fn undo_changes_image(&self) -> bool {
        self.undo
            .last()
            .is_some_and(|entry| entry.edit.changes_image())
    }
    pub fn redo_changes_image(&self) -> bool {
        self.redo
            .last()
            .is_some_and(|entry| entry.edit.changes_image())
    }
    pub fn undo_only_updates_rasters(&self) -> bool {
        self.undo.last().is_some_and(|entry| entry.edit.only_raster_updates())
    }
    pub fn redo_only_updates_rasters(&self) -> bool {
        self.redo.last().is_some_and(|entry| entry.edit.only_raster_updates())
    }

    pub fn allocate_stroke_id(&mut self) -> StrokeId {
        self.document.allocate_stroke_id()
    }

    pub fn perform(&mut self, edit: Edit) -> Result<(), DocumentError> {
        self.perform_with_history_budget(edit, history_budget::BYTE_BUDGET)
    }

    /// Admit a worker-prepared edit without changing document or history.
    pub fn validate_edit(&self, edit: &Edit) -> Result<(), DocumentError> {
        self.prepare_history_edit(edit.clone(), history_budget::BYTE_BUDGET).map(|_| ())
    }

    /// Refine the last selection operation without accumulating slider steps.
    /// The exact document revision and target guard against unrelated edits,
    /// navigation, and Undo/Redo. Retain the original inverse and admit both
    /// directions before publishing the replacement.
    pub fn refine_selection(&mut self, target: SelectionTarget, coverage: Selection, revision: u64) -> Result<(), DocumentError> {
        let previous = self.undo.last().filter(|entry| match (&entry.edit, target) {
            (Edit::SetSelection(_), SelectionTarget::Current) => true,
            (Edit::SetSavedSelection { id, .. }, SelectionTarget::Saved(target)) => *id == target,
            _ => false,
        }).ok_or(DocumentError::InvalidLayerOperation("The selection operation has changed"))?;
        if self.document.revision != revision || !self.redo.is_empty() {
            return Err(DocumentError::InvalidLayerOperation("The selection operation has changed"));
        }
        let edit = self.document.selection_edit(target, coverage)?;
        let (candidate, _) = self.prepare_history_edit(edit, history_budget::BYTE_BUDGET)?;
        if history_budget::Accounting::new(&candidate).charge(previous) > history_budget::BYTE_BUDGET {
            return Err(DocumentError::InvalidLayerOperation("This edit exceeds the Undo/Redo memory limit"));
        }
        self.document = candidate;
        if matches!(target, SelectionTarget::Saved(_)) {
            self.checkpoint = self.next_checkpoint;
            self.next_checkpoint = self.next_checkpoint.checked_add(1).expect("document history exhausted");
        }
        self.trim_history(history_budget::BYTE_BUDGET);
        Ok(())
    }

    fn prepare_history_edit(&self, edit: Edit, budget: usize) -> Result<(Document, HistoryEntry), DocumentError> {
        let mut candidate = self.document.clone();
        let inverse = HistoryEntry::new(candidate.apply(edit)?, self.checkpoint);
        // The actual inverse can restore targets/references in addition to the
        // requested edit. Account the canonical Redo produced by Undo, too.
        let mut restored = candidate.clone();
        let forward = HistoryEntry::new(restored.apply(inverse.edit.clone())?, self.checkpoint);
        if history_budget::Accounting::new(&self.document).charge(&forward) > budget
            || history_budget::Accounting::new(&candidate).charge(&inverse) > budget
        {
            return Err(DocumentError::InvalidLayerOperation(
                "This edit exceeds the Undo/Redo memory limit",
            ));
        }
        Ok((candidate, inverse))
    }

    fn perform_with_history_budget(&mut self, edit: Edit, budget: usize) -> Result<(), DocumentError> {
        // Selecting the drawing target is navigation. It must neither consume
        // an undo step nor discard redoable painting work.
        if matches!(&edit, Edit::SetActiveLayer { .. } | Edit::SetMaskTarget(_)) {
            self.document.apply(edit)?;
            return Ok(());
        }
        let changes_project = edit.changes_project();
        // Source and color jobs publish completed ownership. Check both directions before
        // publication. Live raster transactions retain the existing capture
        // reservation path: their pending roots do not yet identify shared tiles.
        // Standalone selection publication has completed backing. A selection
        // inside an artwork transform batch still uses that raster transaction's
        // pending capture reservation; it must not force eager raster admission.
        let inverse = if matches!(edit, Edit::SetSelection(_)) || edit.requires_history_admission(&self.document) {
            let (candidate, inverse) = self.prepare_history_edit(edit, budget)?;
            self.document = candidate;
            inverse
        } else {
            HistoryEntry::new(self.document.apply(edit)?, self.checkpoint)
        };
        self.undo.push(inverse);
        self.redo.clear();
        if changes_project {
            self.checkpoint = self.next_checkpoint;
            self.next_checkpoint = self
                .next_checkpoint
                .checked_add(1)
                .expect("document history exhausted");
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
        let Some(entry) = self.undo.last() else {
            return Ok(false);
        };
        let inverse = self.document.apply(entry.edit.clone())?;
        let entry = self.undo.pop().unwrap();
        self.redo.push(HistoryEntry::new(inverse, self.checkpoint));
        self.checkpoint = entry.checkpoint;
        Ok(true)
    }

    pub fn redo(&mut self) -> Result<bool, DocumentError> {
        let Some(entry) = self.redo.last() else {
            return Ok(false);
        };
        let inverse = self.document.apply(entry.edit.clone())?;
        let entry = self.redo.pop().unwrap();
        self.undo.push(HistoryEntry::new(inverse, self.checkpoint));
        self.checkpoint = entry.checkpoint;
        Ok(true)
    }

    /// Roll back the suffix whose raster producers failed. Run after the host
    /// retires those producers; pending captures are not evidence of failure.
    /// Validate a candidate first so a missing recovery boundary cannot partly
    /// mutate the document. Earlier undo remains available; failed redo does not.
    pub fn recover_failed_rasters(&mut self) -> Result<usize, DocumentError> {
        fn failed(document: &Document) -> bool {
            document.layers.iter().any(|layer| {
                std::iter::once(&layer.raster)
                    .chain(layer.masks().map(|m| &m.raster))
                    .any(|r| r.failed())
            })
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
        // An undone capture can fail after Undo. Keep the reachable safe part
        // of each branch, so recovery cannot expose failed pixels through Redo.
        for history in [&mut self.undo, &mut self.redo] {
            if let Some(index) = history.iter().rposition(|entry| {
                let mut roots = Vec::new();
                entry.edit.raster_roots(&mut roots);
                roots.into_iter().any(|r| r.failed())
            }) {
                history.drain(..=index);
            }
        }
    }
    /// Raster operation recipes live only until their queue-ordered submission.
    /// Undo entries retain pixel revisions and pre-operation metadata.
    pub fn finish_raster_submission(&mut self) {
        for layer in &mut self.document.layers {
            layer.pending_operations.clear();
            if let Some(mask) = &mut layer.mask {
                mask.pending_operations = Default::default();
            }
        }
    }

    /// Amend only the latest contact, before another document edit or history
    /// navigation. The existing inverse remains its original pre-contact state.
    pub fn amend_raster(
        &mut self,
        target: LayerId,
        revision: raster::RasterRevision,
    ) -> Result<(), DocumentError> {
        self.document.apply(Edit::SetRaster { target, revision })?;
        self.checkpoint = self.next_checkpoint;
        self.next_checkpoint = self
            .next_checkpoint
            .checked_add(1)
            .expect("document history exhausted");
        Ok(())
    }

    /// Gesture preview, followed by restoration + one committed edit at release.
    pub fn preview(&mut self, edit: Edit) -> Result<(), DocumentError> {
        self.document.apply(edit).map(|_| ())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DocumentError {
    InvalidRuler(&'static str),
    InvalidLayerOperation(&'static str),
    MissingLayer(LayerId),
    DuplicateLayer(LayerId),
    ProtectedLayer(LayerId),
    NotDrawable(LayerId),
    EmptyStroke,
    NonFiniteStroke,
    InvalidBrush(BrushError),
}

impl fmt::Display for DocumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRuler(message) => formatter.write_str(message),
            Self::InvalidLayerOperation(message) => formatter.write_str(message),
            Self::MissingLayer(id) => write!(formatter, "layer {} does not exist", id.0),
            Self::DuplicateLayer(id) => write!(formatter, "layer {} already exists", id.0),
            Self::ProtectedLayer(id) => write!(formatter, "layer {} is protected", id.0),
            Self::NotDrawable(id) => write!(formatter, "layer {} cannot receive strokes", id.0),
            Self::EmptyStroke => write!(formatter, "a stroke needs at least one point"),
            Self::NonFiniteStroke => write!(formatter, "stroke contains non-finite input"),
            Self::InvalidBrush(error) => write!(formatter, "invalid stroke brush: {error}"),
        }
    }
}

impl std::error::Error for DocumentError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brush_colors_preserve_finite_extended_rgb_and_validate_coverage_separately() {
        let mut brush = BrushSnapshot::default();
        brush.color_rgba_linear = [-0.3, 1.4, 0.7, 0.37];
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

    fn point(pressure: f32) -> StrokePoint {
        StrokePoint {
            position: Point { x: 12.0, y: 14.0 },
            pressure,
            tilt: [0.0; 2],
            twist: 0.0,
            elapsed_micros: 0,
        }
    }

    fn dot(id: StrokeId, layer_id: LayerId) -> Stroke {
        Stroke::new(
            id,
            layer_id,
            StrokeTool::Brush,
            BrushSnapshot::default(),
            vec![point(2.0)],
        )
        .unwrap()
    }

    #[test]
    fn stroke_clamps_pressure_and_precomputes_bounds() {
        let stroke = dot(StrokeId(1), LayerId(1));
        assert_eq!(stroke.points[0].pressure, 1.0);
        assert!(!stroke.bounds.is_empty());
    }

    #[test]
    fn edit_history_restores_exact_revision_identity() {
        let mut editor = Editor::new(Document::new("study", 1024, 1536));
        let before = editor.document().layers[0].raster.clone();
        let after = raster::RasterRevision::pending();
        editor
            .perform(Edit::SetRaster {
                target: LayerId(1),
                revision: after.clone(),
            })
            .unwrap();
        editor.undo().unwrap();
        assert_eq!(editor.document().layers[0].raster, before);
        editor.redo().unwrap();
        assert_eq!(editor.document().layers[0].raster, after);
        let snapshot = editor.document().clone();
        editor
            .amend_raster(LayerId(1), raster::RasterRevision::pending())
            .unwrap();
        assert_eq!(snapshot.layers[0].raster, after);
        editor.undo().unwrap();
        assert_eq!(editor.document().layers[0].raster, before);
    }

    #[test]
    fn history_is_bounded_and_keeps_the_newest_exact_states() {
        let mut editor = Editor::new(Document::new("bounded", 256, 256));
        for i in 0..400 {
            editor
                .perform(Edit::SetLayerOpacity {
                    id: LayerId(1),
                    opacity: i as f32 / 400.,
                })
                .unwrap();
        }
        assert_eq!(editor.undo.len(), 256);
        assert_eq!(editor.document().layers[0].opacity, 399. / 400.);
        for _ in 0..256 {
            assert!(editor.undo().unwrap());
        }
        assert!(!editor.undo().unwrap());
        assert_eq!(editor.document().layers[0].opacity, 143. / 400.);
        for _ in 0..256 {
            assert!(editor.redo().unwrap());
        }
        assert_eq!(editor.document().layers[0].opacity, 399. / 400.);
        // Unpublished GPU work is reserved at its full allowed staging size.
        for _ in 0..8 {
            editor
                .perform(Edit::SetRaster {
                    target: LayerId(1),
                    revision: raster::RasterRevision::pending(),
                })
                .unwrap();
        }
        assert!(editor.undo.len() <= 2);
    }

    #[test]
    fn project_checkpoint_tracks_undo_branches_not_navigation() {
        let mut editor = Editor::new(Document::new("checkpoint", 64, 64));
        let initial = editor.checkpoint();
        editor
            .perform(Edit::SetRaster {
                target: LayerId(1),
                revision: raster::RasterRevision::pending(),
            })
            .unwrap();
        let saved = editor.checkpoint();
        assert_ne!(initial, saved);
        editor
            .perform(Edit::SetActiveLayer { id: LayerId(2) })
            .unwrap();
        editor.perform(Edit::SetSelection(None)).unwrap();
        assert_eq!(editor.checkpoint(), saved);
        editor.undo().unwrap(); // selection is undoable, but not a persistent edit
        assert_eq!(editor.checkpoint(), saved);
        editor.undo().unwrap();
        assert_eq!(editor.checkpoint(), initial);
        editor.redo().unwrap();
        assert_eq!(editor.checkpoint(), saved);
        editor.undo().unwrap();
        editor
            .perform(Edit::SetReferences(BTreeSet::from([LayerId(1)])))
            .unwrap();
        assert_ne!(editor.checkpoint(), saved); // a same-depth branch is not saved
        assert_ne!(editor.checkpoint(), initial);
        let branch = editor.checkpoint();
        assert!(
            editor
                .perform(Edit::SetActiveLayer { id: LayerId(999) })
                .is_err()
        );
        assert_eq!(editor.checkpoint(), branch);
    }

    #[test]
    fn every_layer_can_be_deleted_and_restored() {
        let mut document = Document::new("study", 800, 800);
        let paint = document.apply(document.delete_layers_edit(&[LayerId(1)]).unwrap()).unwrap();
        assert_eq!(document.active_layer, LayerId(2));
        let paper = document.apply(document.delete_layers_edit(&[LayerId(2)]).unwrap()).unwrap();
        assert!(document.layers.is_empty());
        assert_eq!(document.active_layer, LayerId(0));
        document.apply(paper).unwrap();
        document.apply(paint).unwrap();
        assert_eq!(document.layers.len(), 2);
        assert_eq!(document.active_layer, LayerId(1));
    }
}
