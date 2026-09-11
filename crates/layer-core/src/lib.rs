//! Portable, renderer-agnostic document model for Layer.
//!
//! This crate contains no window, graphics API, inference runtime, async
//! executor, or platform types. Strokes are immutable after commit and use
//! shared point storage so undo/redo moves handles instead of copying samples.

mod effect_catalog;
mod effects;
pub use effect_catalog::*;
mod layers;
pub use effects::*;
mod presets;
pub use layers::*;
mod figures;
pub use figures::{Figure, FigurePaint, FigureShape};

pub use presets::{
    BRISTLE_GRAIN_TEXTURE_ASSET, DefaultBrushPreset, PAINTBRUSH_TEXTURE_ASSET,
    PAPER_GRAIN_TEXTURE_ASSET, PENCIL_TEXTURE_ASSET, WATERCOLOR_TIP_TEXTURE_ASSET,
    WATERCOLOR_TRANSPORT_LONG_BROAD_ASSET, WATERCOLOR_TRANSPORT_LONG_NARROW_ASSET,
    WATERCOLOR_TRANSPORT_SHORT_BROAD_ASSET, WATERCOLOR_TRANSPORT_SHORT_NARROW_ASSET, default_brush,
};

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
};

pub type Revision = u64;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LayerId(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StrokeId(pub u64);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AssetId(pub Arc<str>);

impl From<&str> for AssetId {
    fn from(value: &str) -> Self {
        Self(Arc::from(value))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayerKind {
    Paint,
    ImportedImage,
    AiSuggestion,
    Background,
    Group,
    Effect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub id: LayerId,
    pub name: Arc<str>,
    pub kind: LayerKind,
    pub visible: bool,
    pub opacity: f32,
    /// Front-to-back stroke order for paint layers.
    pub strokes: Vec<StrokeId>,
    pub asset: Option<AssetId>,
    /// Document revision used to generate an AI suggestion.
    pub source_revision: Option<Revision>,
    pub properties: LayerProperties,
    pub mask: Option<LayerMask>,
    pub operations: Vec<LayerOperation>,
    pub effect: Option<Arc<EffectInstance>>,
}

impl Layer {
    /// Composition metadata without copying immutable paint history.
    pub fn composite_snapshot(&self) -> Self {
        Self {
            id: self.id,
            name: "".into(),
            kind: self.kind,
            visible: self.visible,
            opacity: self.opacity,
            strokes: Vec::new(),
            asset: self.asset.clone(),
            source_revision: None,
            properties: self.properties.clone(),
            mask: self.mask.clone(),
            operations: Vec::new(),
            effect: self.effect.clone(),
        }
    }
    pub fn paint(id: LayerId, name: impl Into<Arc<str>>) -> Self {
        Self {
            id,
            name: name.into(),
            kind: LayerKind::Paint,
            visible: true,
            opacity: 1.0,
            strokes: Vec::new(),
            asset: None,
            source_revision: None,
            properties: LayerProperties::default(),
            mask: None,
            operations: Vec::new(),
            effect: None,
        }
    }

    pub fn image(id: LayerId, name: impl Into<Arc<str>>, kind: LayerKind, asset: AssetId) -> Self {
        assert!(matches!(
            kind,
            LayerKind::ImportedImage | LayerKind::AiSuggestion
        ));
        Self {
            id,
            name: name.into(),
            kind,
            visible: true,
            opacity: 1.0,
            strokes: Vec::new(),
            asset: Some(asset),
            source_revision: None,
            properties: LayerProperties::default(),
            mask: None,
            operations: Vec::new(),
            effect: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StrokeTool {
    Brush,
    Eraser,
}

pub const BRUSH_CURVE_SAMPLES: usize = 16;
pub const MAX_BRUSH_DIAMETER: f32 = 32_768.0;
pub const MAX_BRUSH_MAPPINGS: usize = 32;
pub const MAX_BRUSH_SCATTER_DIAMETERS: f32 = 16.0;
pub const MAX_BRUSH_STAMP_COUNT: u8 = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
#[derive(Clone, Copy, Debug, PartialEq)]
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
#[derive(Clone, Copy, Debug, PartialEq)]
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrushTip {
    AnalyticEllipse,
    /// Content-addressed single-channel mask. The render backend prepares it
    /// as an R8 texture before a frame references the brush.
    Mask(AssetId),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum BrushAccumulation {
    #[default]
    Flow,
    Uniform,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum BrushGrainBehavior {
    #[default]
    Moving,
    Canvas,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum ColorMixSpace {
    #[default]
    LinearRgb,
    Oklab,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, PartialEq)]
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushStabilization {
    pub streamline: f32,
    pub pressure_smoothing: f32,
    pub stabilization: f32,
    pub motion_filtering: f32,
    pub expression: f32,
}

impl Default for BrushStabilization {
    fn default() -> Self {
        Self {
            streamline: 0.0,
            pressure_smoothing: 0.0,
            stabilization: 0.0,
            motion_filtering: 0.0,
            expression: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushTaper {
    pub start_distance_diameters: f32,
    pub end_distance_diameters: f32,
    pub start_size: f32,
    pub end_size: f32,
    pub start_opacity: f32,
    pub end_opacity: f32,
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
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
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

#[derive(Clone, Debug, PartialEq)]
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

#[derive(Clone, Debug, PartialEq)]
pub struct DualBrush {
    pub tip: BrushTip,
    pub grain: Option<BrushGrain>,
    pub combine: DualCombineMode,
    pub scale: f32,
    pub aspect: f32,
    pub angle_radians: f32,
    pub offset: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushColorDynamics {
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

#[derive(Clone, Copy, Debug, PartialEq)]
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

#[derive(Clone, Copy, Debug, PartialEq)]
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

#[derive(Clone, Copy, Debug, PartialEq)]
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

#[derive(Clone, Copy, Debug, PartialEq)]
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
#[derive(Clone, Debug, PartialEq)]
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
/// preset creates a new snapshot; old strokes remain deterministic on replay.
#[derive(Clone, Debug, PartialEq)]
pub struct BrushSnapshot {
    pub schema_version: u16,
    pub tip: BrushTip,
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
        if self.schema_version != 4
            || base.iter().any(|value| !value.is_finite())
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
            || finite_nonnegative
                .iter()
                .any(|value| !value.is_finite() || *value < 0.0)
            || taper
                .iter()
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            || colors
                .iter()
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            || bounds.iter().any(|value| !value.is_finite())
            || self.path.jitter_along > MAX_BRUSH_SCATTER_DIAMETERS
            || self.path.jitter_across > MAX_BRUSH_SCATTER_DIAMETERS
            || self.path.continuous_rate_hz > 1_000.0
            || self.shape.count == 0
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
        shape_radius + scatter_radius + 2.0
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

#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub id: StrokeId,
    pub layer_id: LayerId,
    pub tool: StrokeTool,
    pub brush: BrushSnapshot,
    pub points: Arc<[StrokePoint]>,
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
            bounds,
            alpha_locked: false,
            selection: None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub id: Arc<str>,
    pub width: u32,
    pub height: u32,
    /// Front-to-back display order.
    pub layers: Vec<Layer>,
    pub active_layer: LayerId,
    pub active_mask: bool,
    pub selection: Option<Selection>,
    pub reference_layers: BTreeSet<LayerId>,
    pub revision: Revision,
    strokes: BTreeMap<StrokeId, Stroke>,
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
            layers: vec![
                Layer::paint(paint_id, "Current ink"),
                Layer {
                    id: LayerId(2),
                    name: Arc::from("Paper"),
                    kind: LayerKind::Background,
                    visible: true,
                    opacity: 1.0,
                    strokes: Vec::new(),
                    asset: None,
                    source_revision: None,
                    properties: LayerProperties::default(),
                    mask: None,
                    operations: Vec::new(),
                    effect: None,
                },
            ],
            active_layer: paint_id,
            active_mask: false,
            selection: None,
            reference_layers: BTreeSet::new(),
            revision: 0,
            strokes: BTreeMap::new(),
            next_layer_id: 3,
            next_stroke_id: 1,
        }
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|layer| layer.id == id)
    }

    pub fn stroke(&self, id: StrokeId) -> Option<&Stroke> {
        self.strokes.get(&id)
    }

    pub fn strokes(&self) -> impl Iterator<Item = &Stroke> {
        self.strokes.values()
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

    /// Read-only identity for a provisional next-contact cursor.
    pub fn next_stroke_id(&self) -> StrokeId {
        StrokeId(self.next_stroke_id)
    }

    /// Applies one reversible edit and returns its exact inverse.
    pub fn apply(&mut self, edit: Edit) -> Result<Edit, DocumentError> {
        let inverse = match edit {
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
                Edit::SetSelection(std::mem::replace(&mut self.selection, selection))
            }
            Edit::SetReferences(references) => {
                for &id in &references {
                    let layer = self.layer(id).ok_or(DocumentError::MissingLayer(id))?;
                    if !matches!(
                        layer.kind,
                        LayerKind::Paint | LayerKind::ImportedImage | LayerKind::Group
                    ) {
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
                let target = &self.layers[index];
                if target.kind == LayerKind::Background {
                    return Err(DocumentError::ProtectedLayer(id));
                }
                if target.kind == LayerKind::Paint
                    && self
                        .layers
                        .iter()
                        .filter(|layer| layer.kind == LayerKind::Paint)
                        .count()
                        == 1
                {
                    return Err(DocumentError::LastPaintLayer);
                }
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
                        .unwrap_or(self.layers[0].id);
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
            Edit::InsertStroke(stroke) => {
                let target = self
                    .target_owner(stroke.layer_id)
                    .ok_or(DocumentError::MissingLayer(stroke.layer_id))?;
                if target.id == stroke.layer_id && target.kind != LayerKind::Paint {
                    return Err(DocumentError::NotDrawable(stroke.layer_id));
                }
                if self.strokes.contains_key(&stroke.id) {
                    return Err(DocumentError::DuplicateStroke(stroke.id));
                }
                let layer_id = stroke.layer_id;
                let stroke_id = stroke.id;
                self.strokes.insert(stroke_id, *stroke);
                self.target_strokes_mut(layer_id)
                    .expect("target checked above")
                    .push(stroke_id);
                Edit::RemoveStroke { id: stroke_id }
            }
            Edit::RemoveStroke { id } => {
                let stroke = self
                    .strokes
                    .remove(&id)
                    .ok_or(DocumentError::MissingStroke(id))?;
                self.target_strokes_mut(stroke.layer_id)
                    .ok_or(DocumentError::MissingLayer(stroke.layer_id))?
                    .retain(|candidate| *candidate != id);
                Edit::InsertStroke(Box::new(stroke))
            }
        };
        self.revision = self.revision.saturating_add(1);
        Ok(inverse)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    Batch(Vec<Edit>),
    ReplaceLayer(Box<Layer>),
    SetMaskTarget(bool),
    SetSelection(Option<Selection>),
    SetReferences(BTreeSet<LayerId>),
    InsertLayer { index: usize, layer: Layer },
    RemoveLayer { id: LayerId },
    MoveLayer { id: LayerId, to: usize },
    SetLayerOpacity { id: LayerId, opacity: f32 },
    SetLayerVisibility { id: LayerId, visible: bool },
    SetActiveLayer { id: LayerId },
    InsertStroke(Box<Stroke>),
    RemoveStroke { id: StrokeId },
}

#[derive(Debug)]
pub struct Editor {
    document: Document,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
}

impl Editor {
    pub fn new(document: Document) -> Self {
        Self {
            document,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn document(&self) -> &Document {
        &self.document
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

    pub fn allocate_stroke_id(&mut self) -> StrokeId {
        self.document.allocate_stroke_id()
    }

    pub fn perform(&mut self, edit: Edit) -> Result<(), DocumentError> {
        // Selecting the drawing target is navigation. It must neither consume
        // an undo step nor discard redoable painting work.
        let selection_only = matches!(&edit, Edit::SetActiveLayer { .. } | Edit::SetMaskTarget(_));
        let inverse = self.document.apply(edit)?;
        if !selection_only {
            self.undo.push(inverse);
            self.redo.clear();
        }
        Ok(())
    }

    pub fn undo(&mut self) -> Result<bool, DocumentError> {
        let Some(edit) = self.undo.pop() else {
            return Ok(false);
        };
        let inverse = self.document.apply(edit)?;
        self.redo.push(inverse);
        Ok(true)
    }

    pub fn redo(&mut self) -> Result<bool, DocumentError> {
        let Some(edit) = self.redo.pop() else {
            return Ok(false);
        };
        let inverse = self.document.apply(edit)?;
        self.undo.push(inverse);
        Ok(true)
    }

    pub fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// Gesture preview, followed by restoration + one committed edit at release.
    pub fn preview(&mut self, edit: Edit) -> Result<(), DocumentError> {
        self.document.apply(edit).map(|_| ())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DocumentError {
    InvalidLayerOperation(&'static str),
    MissingLayer(LayerId),
    MissingStroke(StrokeId),
    DuplicateLayer(LayerId),
    DuplicateStroke(StrokeId),
    ProtectedLayer(LayerId),
    NotDrawable(LayerId),
    LastPaintLayer,
    EmptyStroke,
    NonFiniteStroke,
    InvalidBrush(BrushError),
}

impl fmt::Display for DocumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLayerOperation(message) => formatter.write_str(message),
            Self::MissingLayer(id) => write!(formatter, "layer {} does not exist", id.0),
            Self::MissingStroke(id) => write!(formatter, "stroke {} does not exist", id.0),
            Self::DuplicateLayer(id) => write!(formatter, "layer {} already exists", id.0),
            Self::DuplicateStroke(id) => write!(formatter, "stroke {} already exists", id.0),
            Self::ProtectedLayer(id) => write!(formatter, "layer {} is protected", id.0),
            Self::NotDrawable(id) => write!(formatter, "layer {} cannot receive strokes", id.0),
            Self::LastPaintLayer => write!(formatter, "a document needs at least one paint layer"),
            Self::EmptyStroke => write!(formatter, "a stroke needs at least one point"),
            Self::NonFiniteStroke => write!(formatter, "stroke contains non-finite input"),
            Self::InvalidBrush(error) => write!(formatter, "invalid stroke brush: {error}"),
        }
    }
}

impl std::error::Error for DocumentError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputRole {
    StyleReference,
    CharacterReference,
    Structure,
    PreviousLayer,
    CurrentLayer,
    PreviousSuggestion,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageInput {
    pub role: InputRole,
    pub asset_id: AssetId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SuggestionRequest {
    pub document_id: Arc<str>,
    pub source_revision: Revision,
    pub width: u32,
    pub height: u32,
    pub inputs: Arc<[ImageInput]>,
    /// `0` explores; `1` follows the current drawing closely.
    pub fidelity: f32,
    /// Stable across one pass so suggestions evolve instead of jumping.
    pub continuity_seed: u64,
}

impl SuggestionRequest {
    pub fn normalized(mut self) -> Self {
        self.fidelity = self.fidelity.clamp(0.0, 1.0);
        self
    }

    pub fn is_stale_for(&self, document: &Document) -> bool {
        self.document_id != document.id || self.source_revision != document.revision
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SuggestionResult {
    pub request_revision: Revision,
    pub asset_id: AssetId,
    pub elapsed_millis: u32,
}

pub trait InferenceBackend: Send + Sync {
    type Error;

    fn suggest(&self, request: SuggestionRequest) -> Result<SuggestionResult, Self::Error>;
    fn cancel_before(&self, document_id: &str, revision: Revision);
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn edit_history_moves_arc_backed_strokes_without_copying_points() {
        let mut editor = Editor::new(Document::new("study", 1024, 1536));
        let stroke = dot(StrokeId(1), LayerId(1));
        let points = stroke.points.clone();
        editor
            .perform(Edit::InsertStroke(Box::new(stroke)))
            .unwrap();
        assert_eq!(Arc::strong_count(&points), 2);
        assert!(editor.undo().unwrap());
        assert!(editor.document().stroke(StrokeId(1)).is_none());
        assert!(editor.redo().unwrap());
        assert!(Arc::ptr_eq(
            &points,
            &editor.document().stroke(StrokeId(1)).unwrap().points
        ));
    }

    #[test]
    fn stale_results_are_detected_by_revision() {
        let document = Document::new("study", 800, 800);
        let request = SuggestionRequest {
            document_id: document.id.clone(),
            source_revision: document.revision + 1,
            width: 800,
            height: 800,
            inputs: Arc::from([]),
            fidelity: 1.4,
            continuity_seed: 7,
        }
        .normalized();
        assert_eq!(request.fidelity, 1.0);
        assert!(request.is_stale_for(&document));
    }

    #[test]
    fn background_and_last_paint_are_protected() {
        let mut document = Document::new("study", 800, 800);
        assert_eq!(
            document.apply(Edit::RemoveLayer { id: LayerId(1) }),
            Err(DocumentError::LastPaintLayer)
        );
        assert_eq!(
            document.apply(Edit::RemoveLayer { id: LayerId(2) }),
            Err(DocumentError::ProtectedLayer(LayerId(2)))
        );
    }
}
