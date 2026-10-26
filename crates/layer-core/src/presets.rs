use crate::{
    AssetId, BrushAccumulation, BrushBlendMode, BrushColorDynamics, BrushCombine, BrushContact, BrushCurve,
    BrushDeform, BrushExecution, BrushGrain, BrushMapping, BrushPath,
    BrushRendering, BrushSensor, BrushShape, BrushSnapshot, BrushStabilization, BrushTarget, BrushTip, BrushTransport,
    BrushWetMix, BlenderKind, BrushBlender, ColorMixSpace, LiquifyMode,
};
use std::sync::Arc;

pub const WATERCOLOR_TIP_TEXTURE_ASSET: &str = "builtin:brush-tip/watercolor-ragged-v1";
pub const WATERCOLOR_TRANSPORT_LONG_NARROW_ASSET: &str = "builtin:brush-transport/long-narrow-v1";
pub const WATERCOLOR_TRANSPORT_LONG_BROAD_ASSET: &str = "builtin:brush-transport/long-broad-v1";
pub const WATERCOLOR_TRANSPORT_SHORT_NARROW_ASSET: &str = "builtin:brush-transport/short-narrow-v1";
pub const WATERCOLOR_TRANSPORT_SHORT_BROAD_ASSET: &str = "builtin:brush-transport/short-broad-v1";

/// Stable identifiers for the small built-in brush set exposed across FFI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum DefaultBrushPreset {
    GPen = 1,
    Pencil = 2,
    Eraser = 3,
    Paintbrush = 4,
    Airbrush = 5,
    Chalk = 6,
    Marker = 7,
    Spray = 8,
    DualTexture = 9,
    Smudge = 10,
    WetRound = 11,
    LiquifyPush = 12,
    LiquifyTwirl = 13,
    MultiplyGlaze = 14,
    TexturedFlat = 15,
    DryScumble = 16,
    PastelBlock = 17,
    TransparentGlaze = 18,
    OpaqueGouache = 19,
    WatercolorWash = 20,
    WetWatercolor = 21,
    LoadedOil = 22,
    PaletteKnife = 23,
    NaturalBlender = 24,
    PointyPencil = 25,
    ShadingPencil = 26,
    Charcoal = 27,
    RoughGPen = 28,
    CalligraphyPen = 29,
    AntiquePen = 30,
    RealisticPen = 31,
    WetInk = 32,
    BlottyInk = 33,
    BrushedInk = 34,
    BristlePaintbrush = 35,
    LiquifyTwirlClockwise = 36,
    LiquifyPinch = 37,
    LiquifyExpand = 38,
    LiquifyCrystals = 39,
    CloneStamp = 40,
    HealingBrush = 41,
    SpotHealingBrush = 42,
    Crayon = 43,
    Monoline = 44,
    SoftBlender = 45,
    Blur = 46,
    BristleBlender = 47,
    GrainyBlender = 48,
    ScatterBlender = 49,
    PainterlyBlender = 50,
    CrystalBlender = 51,
    Marbling = 52,
    CombBlender = 53,
    DitherBlender = 55,
    BlendingKnife = 56,
    WetBlender = 57,
    ColorMixer = 58,
    MixingBrush = 59,
    Sharpen = 60,
    TextureBlender = 61,
    HueBlender = 62,
    LiquifyEdge = 63,
    LiquifyReconstruct = 64,
    BlendingStump = 65,
    Tissue = 66,
}

impl DefaultBrushPreset {
    pub const ALL: [Self; 65] = [
        Self::GPen, Self::Pencil, Self::Eraser, Self::Paintbrush, Self::Airbrush, Self::Chalk, Self::Marker,
        Self::Spray, Self::DualTexture, Self::Smudge, Self::WetRound, Self::LiquifyPush, Self::LiquifyTwirl,
        Self::MultiplyGlaze, Self::TexturedFlat, Self::DryScumble, Self::PastelBlock, Self::TransparentGlaze,
        Self::OpaqueGouache, Self::WatercolorWash, Self::WetWatercolor, Self::LoadedOil, Self::PaletteKnife,
        Self::NaturalBlender, Self::PointyPencil, Self::ShadingPencil, Self::Charcoal, Self::RoughGPen,
        Self::CalligraphyPen, Self::AntiquePen, Self::RealisticPen, Self::WetInk, Self::BlottyInk, Self::BrushedInk,
        Self::BristlePaintbrush, Self::LiquifyTwirlClockwise, Self::LiquifyPinch, Self::LiquifyExpand,
        Self::LiquifyCrystals, Self::CloneStamp, Self::HealingBrush, Self::SpotHealingBrush, Self::Crayon,
        Self::Monoline, Self::SoftBlender, Self::Blur, Self::BristleBlender, Self::GrainyBlender,
        Self::ScatterBlender, Self::PainterlyBlender, Self::CrystalBlender, Self::Marbling, Self::CombBlender,
        Self::DitherBlender, Self::BlendingKnife, Self::WetBlender, Self::ColorMixer,
        Self::MixingBrush, Self::Sharpen, Self::TextureBlender, Self::HueBlender,
        Self::LiquifyEdge, Self::LiquifyReconstruct, Self::BlendingStump, Self::Tissue,
    ];
}


/// Returns a complete immutable preset snapshot. Callers may override color,
/// diameter, and opacity before beginning a stroke.
pub fn default_brush(preset: DefaultBrushPreset) -> BrushSnapshot {
    match preset {
        DefaultBrushPreset::Pencil | DefaultBrushPreset::PointyPencil | DefaultBrushPreset::ShadingPencil
        | DefaultBrushPreset::Charcoal | DefaultBrushPreset::Chalk | DefaultBrushPreset::PastelBlock
        | DefaultBrushPreset::Crayon => crate::contact_presets::dry_brush(preset),
        DefaultBrushPreset::BristlePaintbrush => crate::contact_presets::bristle_paintbrush(),
        DefaultBrushPreset::GPen
        | DefaultBrushPreset::Monoline
        | DefaultBrushPreset::RoughGPen
        | DefaultBrushPreset::CalligraphyPen
        | DefaultBrushPreset::AntiquePen
        | DefaultBrushPreset::RealisticPen
        | DefaultBrushPreset::WetInk
        | DefaultBrushPreset::BlottyInk
        | DefaultBrushPreset::BrushedInk => crate::contact_presets::contact_brush(preset),
        DefaultBrushPreset::Eraser => BrushSnapshot {
            contact: Some(crate::contact_presets::dry_material(preset)),
            color_rgba_linear: [0.0, 0.0, 0.0, 1.0],
            diameter: 180.0,
            opacity: 1.0,
            hardness: 0.88,
            flow: 1.0,
            spacing: 0.08,
            seed: 0x4552_4153,
            ..BrushSnapshot::default()
        },
        DefaultBrushPreset::Paintbrush => BrushSnapshot {
            contact: Some(crate::contact_presets::dry_material(preset)),
            color_rgba_linear: [0.12, 0.025, 0.012, 1.0],
            diameter: 460.0,
            opacity: 0.95,
            hardness: 0.9,
            flow: 1.0,
            spacing: 0.20,
            aspect: 0.68,
            grain: Some(paper_grain(1.)),
            rendering: BrushRendering {
                accumulation: BrushAccumulation::Uniform,
                ..Default::default()
            },
            seed: 0x5041_494e,
            mappings: Arc::from([
                BrushMapping::pressure_size(),
                pressure_flow(0.18, 0.82),
                direction_rotation(),
            ]),
            ..BrushSnapshot::default()
        },
        DefaultBrushPreset::Airbrush => BrushSnapshot {
            contact: Some(crate::contact_presets::dry_material(preset)),
            color_rgba_linear: [0.04, 0.12, 0.42, 1.0],
            diameter: 320.0,
            hardness: 0.05,
            flow: 0.18,
            spacing: 0.055,
            seed: 0x4149_5242,
            path: BrushPath {
                continuous_rate_hz: 60.0,
                ..BrushPath::default()
            },
            mappings: Arc::from([BrushMapping::pressure_size(), pressure_flow(0.08, 0.72)]),
            ..BrushSnapshot::default()
        },
        DefaultBrushPreset::Marker => BrushSnapshot {
            contact: Some(crate::contact_presets::dry_material(preset)),
            color_rgba_linear: [0.72, 0.06, 0.16, 0.82],
            diameter: 180.0,
            hardness: 1.0,
            flow: 0.72,
            spacing: 0.055,
            aspect: 0.42,
            angle_radians: -0.45,
            rendering: BrushRendering {
                accumulation: BrushAccumulation::Uniform,
                ..Default::default()
            },
            ..BrushSnapshot::default()
        },
        DefaultBrushPreset::Spray => BrushSnapshot {
            color_rgba_linear: [0.08, 0.38, 0.12, 1.0],
            diameter: 34.0,
            hardness: 0.65,
            flow: 0.28,
            spacing: 0.34,
            seed: 0x5350_5241,
            path: BrushPath {
                spacing_jitter: 0.32,
                jitter_along: 4.5,
                jitter_across: 4.5,
                ..BrushPath::default()
            },
            shape: BrushShape {
                count: 7,
                count_jitter: 0.45,
                rotation_jitter: 1.0,
                ..BrushShape::default()
            },
            mappings: Arc::from([BrushMapping::pressure_size(), pressure_flow(0.1, 0.7)]),
            ..BrushSnapshot::default()
        },
        DefaultBrushPreset::DualTexture => BrushSnapshot {
            contact: Some(crate::contact_presets::dry_material(preset)),
            color_rgba_linear: [0.08, 0.025, 0.42, 1.0],
            diameter: 260.0,
            flow: 0.7,
            spacing: 0.16,
            aspect: 0.72,
            grain: Some(paper_grain(1.)),
            mappings: Arc::from([BrushMapping::pressure_size(), direction_rotation()]),
            ..BrushSnapshot::default()
        },
        DefaultBrushPreset::Smudge => BrushSnapshot {
            hardness: 0.42,
            flow: 0.72,
            mappings: Arc::from([BrushMapping::pressure_size()]),
            ..blender(BlenderKind::Soft, BlenderSpec { reach: 0.05, rate: 40.0, pull: 0.95, paper: Some(1.0), tip: Tip::FINGER, ..BlenderSpec::BASE })
        },
        DefaultBrushPreset::WetRound => BrushSnapshot {
            color_rgba_linear: [0.015, 0.18, 0.52, 1.0],
            diameter: 240.0,
            hardness: 0.54,
            flow: 0.48,
            spacing: 0.05,
            execution: BrushExecution::Wet,
            wet_mix: BrushWetMix {
                amount_of_paint: 0.42,
                density: 0.76,
                charge: 0.88,
                charge_depletion: 0.16,
                dilution: 0.24,
                attack: 0.68,
                pull: 0.64,
                blur: 0.18,
                wetness_jitter: 0.12,
                wetness: 0.68,
                mix_space: ColorMixSpace::Oklab,
                average: 0.0,
                refill: 0.0,
                stretch: 0.0,
            },
            mappings: Arc::from([BrushMapping::pressure_size(), pressure_flow(0.18, 0.72)]),
            ..BrushSnapshot::default()
        },
        DefaultBrushPreset::LiquifyPush => liquify(420.0, 0.34, 0.08, BrushDeform {
            strength: 0.82, momentum: 0.28, ..Default::default()
        }),
        DefaultBrushPreset::LiquifyTwirl | DefaultBrushPreset::LiquifyTwirlClockwise => liquify(620.0, 0.22, 0.22, BrushDeform {
            mode: if preset == DefaultBrushPreset::LiquifyTwirl { LiquifyMode::TwirlCounterclockwise } else { LiquifyMode::TwirlClockwise },
            strength: 0.72, ..Default::default()
        }),
        DefaultBrushPreset::LiquifyPinch | DefaultBrushPreset::LiquifyExpand => liquify(520.0, 0.3, 0.16, BrushDeform {
            mode: if preset == DefaultBrushPreset::LiquifyPinch { LiquifyMode::Pinch } else { LiquifyMode::Expand },
            ..Default::default()
        }),
        DefaultBrushPreset::LiquifyCrystals => liquify(360.0, 0.4, 0.1, BrushDeform {
            mode: LiquifyMode::Crystals, strength: 0.7, distortion: 0.6, ..Default::default()
        }),
        DefaultBrushPreset::SoftBlender => blender(BlenderKind::Soft, BlenderSpec { reach: 0.15, rate: 4.5, pull: 0.6, paper: Some(0.0), ..BlenderSpec::BASE }),
        DefaultBrushPreset::Blur => blender(BlenderKind::Blur, BlenderSpec { reach: 0.2, rate: 6.0, ..BlenderSpec::BASE }),
        DefaultBrushPreset::Sharpen => blender(BlenderKind::Sharpen, BlenderSpec { reach: 0.06, rate: 3.0, character: 0.8, ..BlenderSpec::BASE }),
        DefaultBrushPreset::BristleBlender => blender(BlenderKind::Bristle,
            BlenderSpec { reach: 0.03, rate: 30.0, pull: 0.85, character: 1.0, scale: 0.5, paper: Some(1.0), tip: Tip::BRUSH, ..BlenderSpec::BASE }),
        DefaultBrushPreset::GrainyBlender => blender(BlenderKind::Grainy, BlenderSpec { reach: 0.2, rate: 4.0, pull: 0.3, paper: Some(1.0), ..BlenderSpec::BASE }),
        DefaultBrushPreset::TextureBlender => blender(BlenderKind::TextureKeeping,
            BlenderSpec { reach: 0.18, rate: 4.0, pull: 0.4, character: 0.9, scale: 0.05, ..BlenderSpec::BASE }),
        DefaultBrushPreset::HueBlender => blender(BlenderKind::Hue, BlenderSpec { reach: 0.2, rate: 4.0, pull: 0.2, ..BlenderSpec::BASE }),
        DefaultBrushPreset::BlendingKnife => BrushSnapshot { aspect: 0.25, flow: 0.95,
            mappings: Arc::from([direction_rotation()]),
            ..blender(BlenderKind::Knife, BlenderSpec { rate: 20.0, pull: 0.9, character: 0.5, scale: 0.15, hardness: 0.95,
                paper: Some(1.0), tip: Tip::BLADE, ..BlenderSpec::BASE }) },
        DefaultBrushPreset::BlendingStump => BrushSnapshot { diameter: 40.0,
            ..blender(BlenderKind::Soft, BlenderSpec { reach: 0.06, rate: 14.0, pull: 0.35, hardness: 0.7, paper: Some(1.0), tip: Tip::STUMP, ..BlenderSpec::BASE }) },
        DefaultBrushPreset::Tissue => BrushSnapshot { diameter: 120.0,
            ..blender(BlenderKind::Soft, BlenderSpec { reach: 0.3, rate: 3.0, pull: 0.5, hardness: 0.2, paper: Some(1.0), tip: Tip::TISSUE, lift: 0.15, ..BlenderSpec::BASE }) },
        DefaultBrushPreset::ScatterBlender => blender(BlenderKind::Scatter, BlenderSpec { reach: 0.2, rate: 3.0, pull: 0.2, character: 1.0, ..BlenderSpec::BASE }),
        DefaultBrushPreset::PainterlyBlender => blender(BlenderKind::Painterly, BlenderSpec { reach: 0.25, rate: 8.0, pull: 0.6, character: 2.0, ..BlenderSpec::BASE }),
        DefaultBrushPreset::CrystalBlender => blender(BlenderKind::Crystal, BlenderSpec { reach: 0.06, rate: 4.0, scale: 0.18, ..BlenderSpec::BASE }),
        DefaultBrushPreset::Marbling => blender(BlenderKind::Marbling, BlenderSpec { rate: 30.0, pull: 0.4, character: 1.0, scale: 0.45, ..BlenderSpec::BASE }),
        DefaultBrushPreset::CombBlender => blender(BlenderKind::Comb, BlenderSpec { reach: 0.35, rate: 14.0, ..BlenderSpec::BASE }),
        DefaultBrushPreset::DitherBlender => blender(BlenderKind::Dither, BlenderSpec { reach: 0.08, rate: 6.0, hardness: 0.9, ..BlenderSpec::BASE }),
        DefaultBrushPreset::WetBlender => wet_blender(0.55, 0.0, 0.3),
        DefaultBrushPreset::ColorMixer => {
            let mut brush = wet_blender(0.3, 1.0, 0.0);
            brush.wet_mix.stretch = 0.6;
            brush
        }
        DefaultBrushPreset::MixingBrush => {
            let mut brush = wet_blender(0.4, 0.0, 0.0);
            brush.color_rgba_linear = [0.9, 0.86, 0.78, 1.0];
            brush.hardness = 0.6;
            brush.wet_mix.amount_of_paint = 0.5;
            brush.wet_mix.dilution = 0.1;
            brush.wet_mix.refill = 0.5;
            brush.mappings = Arc::from([pressure_flow(0.35, 0.65)]);
            brush
        }
        DefaultBrushPreset::LiquifyEdge => liquify(520., 0.3, 0.16, BrushDeform {
            mode: LiquifyMode::Edge, ..Default::default()
        }),
        DefaultBrushPreset::LiquifyReconstruct => liquify(420., 0.34, 0.12, BrushDeform {
            mode: LiquifyMode::Reconstruct, ..Default::default()
        }),
        DefaultBrushPreset::CloneStamp => retouching(BrushExecution::Clone, 160.0, 0.5),
        DefaultBrushPreset::HealingBrush => retouching(BrushExecution::Heal, 120.0, 0.5),
        DefaultBrushPreset::SpotHealingBrush => retouching(BrushExecution::SpotHeal, 60.0, 0.6),
        DefaultBrushPreset::MultiplyGlaze => BrushSnapshot {
            color_rgba_linear: [0.12, 0.28, 0.72, 1.0],
            diameter: 240.0,
            hardness: 0.62,
            flow: 0.22,
            spacing: 0.07,
            rendering: BrushRendering {
                blend_mode: BrushBlendMode::Multiply,
                ..BrushRendering::default()
            },
            mappings: Arc::from([BrushMapping::pressure_size(), pressure_flow(0.1, 0.72)]),
            ..BrushSnapshot::default()
        },
        DefaultBrushPreset::TexturedFlat => BrushSnapshot {
            contact: Some(crate::contact_presets::dry_material(preset)),
            ..painter_brush(PainterBrushSpec {
                diameter: 360.0,
                aspect: 0.62,
                flow: 1.0,
                opacity: 0.9,
                spacing: 0.16,
                hardness: 0.97,
                execution: BrushExecution::Dry,
                grain: Some(paper_grain(1.)),
                rendering: BrushRendering { accumulation: BrushAccumulation::Uniform, ..Default::default() },
                wet_mix: BrushWetMix::default(),
            })
        },
        DefaultBrushPreset::DryScumble => BrushSnapshot {
            contact: Some(crate::contact_presets::dry_material(preset)),
            diameter: 430.0,
            aspect: 0.72,
            flow: 0.95,
            opacity: 0.86,
            spacing: 0.22,
            hardness: 0.52,
            execution: BrushExecution::Dry,
            grain: Some(paper_grain(1.)),
            rendering: BrushRendering {
                accumulation: BrushAccumulation::Uniform,
                alpha_threshold: 0.08,
                ..BrushRendering::default()
            },
            wet_mix: BrushWetMix::default(),
            color_dynamics: BrushColorDynamics {
                stroke_saturation_jitter: 0.035,
                ..Default::default()
            },
            shape: BrushShape {
                follow_direction: 1.,
                ..Default::default()
            },
            mappings: Arc::from([BrushMapping::pressure_size()]),
            ..Default::default()
        },
        DefaultBrushPreset::TransparentGlaze => BrushSnapshot {
            contact: Some(crate::contact_presets::dry_material(preset)),
            ..painter_brush(PainterBrushSpec {
                diameter: 520.0,
                aspect: 0.52,
                flow: 0.25,
                opacity: 0.62,
                spacing: 0.08,
                hardness: 0.68,
                execution: BrushExecution::Dry,
                grain: Some(paper_grain(1.)),
                rendering: BrushRendering { accumulation: BrushAccumulation::Uniform, ..Default::default() },
                wet_mix: BrushWetMix::default(),
            })
        },
        DefaultBrushPreset::OpaqueGouache => painter_brush(PainterBrushSpec {
            diameter: 410.0,
            aspect: 0.68,
            flow: 0.76,
            opacity: 0.84,
            spacing: 0.05,
            hardness: 0.54,
            execution: BrushExecution::Wet,
            grain: Some(paper_grain(0.48)),
            rendering: BrushRendering::default(),
            wet_mix: wet_mix(0.78, 0.12, 0.035, 0.18, 0.16, 0.22),
        }),
        DefaultBrushPreset::WatercolorWash => watercolor_brush(
            PainterBrushSpec {
                diameter: 620.0,
                aspect: 0.92,
                flow: 0.54,
                opacity: 0.58,
                spacing: 0.07,
                hardness: 1.0,
                execution: BrushExecution::Watercolor,
                // The artist-controlled tip owns the ragged footprint. Keeping
                // paper grain out of deposition avoids repeating internal texture.
                grain: None,
                rendering: BrushRendering {
                    accumulation: BrushAccumulation::Uniform,
                    wet_edge: 0.72,
                    burnt_edge: 0.18,
                    edge_width: 10.0,
                    ..BrushRendering::default()
                },
                wet_mix: watercolor_mix(0.72, 0.22, 0.26, 0.12),
            },
            BrushTransport {
                conductance: AssetId::from(WATERCOLOR_TRANSPORT_LONG_BROAD_ASSET),
                scale: 0.25,
                rotation_radians: 0.0,
                contrast: 0.75,
                wet_flow: 0.52,
                dry_flow: 0.16,
                distance: 26.0,
                water_load: 0.86,
            },
        ),
        DefaultBrushPreset::WetWatercolor => watercolor_brush(
            PainterBrushSpec {
                diameter: 540.0,
                aspect: 0.88,
                flow: 0.62,
                opacity: 0.66,
                spacing: 0.08,
                hardness: 1.0,
                execution: BrushExecution::Watercolor,
                grain: None,
                rendering: BrushRendering {
                    accumulation: BrushAccumulation::Uniform,
                    wet_edge: 0.56,
                    burnt_edge: 0.12,
                    edge_width: 8.0,
                    ..BrushRendering::default()
                },
                wet_mix: watercolor_mix(0.66, 0.52, 0.44, 0.22),
            },
            BrushTransport {
                conductance: AssetId::from(WATERCOLOR_TRANSPORT_LONG_NARROW_ASSET),
                scale: 0.25,
                rotation_radians: 0.0,
                contrast: 0.82,
                wet_flow: 0.76,
                dry_flow: 0.28,
                distance: 28.0,
                water_load: 0.92,
            },
        ),
        DefaultBrushPreset::LoadedOil => painter_brush(PainterBrushSpec {
            diameter: 480.0,
            aspect: 0.56,
            flow: 0.74,
            opacity: 0.92,
            spacing: 0.055,
            hardness: 0.48,
            execution: BrushExecution::Wet,
            grain: Some(paper_grain(0.72)),
            rendering: BrushRendering::default(),
            wet_mix: wet_mix(0.72, 0.38, 0.060, 0.22, 0.28, 0.12),
        }),
        DefaultBrushPreset::PaletteKnife => painter_brush(PainterBrushSpec {
            diameter: 560.0,
            aspect: 0.24,
            flow: 0.78,
            opacity: 0.96,
            spacing: 0.19,
            hardness: 0.32,
            execution: BrushExecution::Wet,
            grain: Some(paper_grain(0.86)),
            rendering: BrushRendering::default(),
            wet_mix: wet_mix(0.82, 0.88, 0.025, 0.16, 0.18, 0.08),
        }),
        DefaultBrushPreset::NaturalBlender => BrushSnapshot {
            blender: BrushBlender { kind: BlenderKind::Soft, reach: 0.12, rate: 5.5, character: 0.55, scale: 0.6,
                cell_size: 1.0, pressure_gain: Tip::BRUSH.pressure_gain, engagement: Tip::BRUSH.engagement, recess: Tip::BRUSH.recess, lift: 0.0 },
            ..painter_brush(PainterBrushSpec {
                diameter: 440.0,
                aspect: 0.64,
                flow: 0.78,
                opacity: 1.0,
                spacing: 0.04,
                hardness: 0.66,
                execution: BrushExecution::Smudge,
                grain: Some(paper_grain(1.0)),
                rendering: BrushRendering::default(),
                wet_mix: BrushWetMix { pull: 0.5, mix_space: ColorMixSpace::Oklab, ..BrushWetMix::default() },
            })
        },
    }
}

#[derive(Clone, Copy)]
struct BlenderSpec {
    reach: f32,
    rate: f32,
    pull: f32,
    character: f32,
    scale: f32,
    hardness: f32,
    paper: Option<f32>,
    tip: Tip,
    lift: f32,
}

impl BlenderSpec {
    const BASE: Self = Self { reach: 0.0, rate: 0.0, pull: 0.0, character: 0.0, scale: 0.0, hardness: 0.5, paper: None, tip: Tip::ROUND, lift: 0.0 };
}

/// How a blender's tip meets the paper, in the dry-media contact terms.
#[derive(Clone, Copy)]
struct Tip {
    pressure_gain: f32,
    engagement: f32,
    recess: f32,
}

impl Tip {
    const ROUND: Self = Self { pressure_gain: 1.4, engagement: 0.1, recess: 0.4 };
    const FINGER: Self = Self { pressure_gain: 0.93, engagement: 0.08, recess: 0.0 };
    const BRUSH: Self = Self { pressure_gain: 1.0, engagement: 0.1, recess: 0.4 };
    const BLADE: Self = Self { pressure_gain: 1.2, engagement: 0.05, recess: 0.0 };
    const STUMP: Self = Self { pressure_gain: 1.6, engagement: 0.08, recess: 0.4 };
    const TISSUE: Self = Self { pressure_gain: 1.0, engagement: 0.2, recess: 0.0 };
}

fn blender(kind: BlenderKind, spec: BlenderSpec) -> BrushSnapshot {
    BrushSnapshot {
        color_rgba_linear: [0.0, 0.0, 0.0, 1.0],
        diameter: 220.0,
        hardness: spec.hardness,
        flow: 0.8,
        spacing: 0.04,
        execution: BrushExecution::Smudge,
        wet_mix: BrushWetMix {
            amount_of_paint: 0.0,
            density: 0.92,
            attack: 0.82,
            pull: spec.pull,
            mix_space: ColorMixSpace::Oklab,
            ..BrushWetMix::default()
        },
        grain: spec.paper.map(paper_grain),
        blender: BrushBlender { kind, reach: spec.reach, rate: spec.rate, character: spec.character, scale: spec.scale,
            cell_size: match kind { BlenderKind::Dither => 4.0, BlenderKind::Scatter => 3.0, _ => 1.0 },
            pressure_gain: spec.tip.pressure_gain, engagement: spec.tip.engagement, recess: spec.tip.recess, lift: spec.lift },
        mappings: Arc::from([pressure_flow(0.15, 0.85)]),
        ..BrushSnapshot::default()
    }
}

fn wet_blender(pull: f32, average: f32, charge_depletion: f32) -> BrushSnapshot {
    BrushSnapshot {
        execution: BrushExecution::Wet,
        wet_mix: BrushWetMix {
            amount_of_paint: 0.0,
            density: 0.92,
            charge_depletion,
            attack: 0.82,
            pull,
            blur: 0.1,
            mix_space: ColorMixSpace::Oklab,
            average,
            ..BrushWetMix::default()
        },
        ..blender(BlenderKind::Soft, BlenderSpec::BASE)
    }
}

/// A retouching brush: a round tip whose stroke coverage is uniform, so each
/// pixel takes the source once however many dabs cross it.
fn liquify(diameter: f32, hardness: f32, spacing: f32, deform: BrushDeform) -> BrushSnapshot {
    let twirl = matches!(deform.mode, LiquifyMode::TwirlCounterclockwise | LiquifyMode::TwirlClockwise);
    BrushSnapshot { diameter, hardness, spacing, deform, flow: 1.,
        path: BrushPath { continuous_rate_hz: if matches!(deform.mode, LiquifyMode::Push | LiquifyMode::Crystals) { 0. } else { 20. }, ..Default::default() },
        mappings: if twirl { Arc::from([BrushMapping { sensor: BrushSensor::Twist, target: BrushTarget::DeformStrength,
            combine: BrushCombine::Multiply, input_min: 0., input_max: 1., output_scale: 1., output_bias: 0., curve: BrushCurve::LINEAR }]) } else { Arc::default() },
        execution: BrushExecution::Liquify, ..Default::default() }
}

fn retouching(execution: BrushExecution, diameter: f32, hardness: f32) -> BrushSnapshot {
    BrushSnapshot {
        contact: Some(BrushContact { linear_edge: true, ..BrushContact::default() }),
        diameter,
        hardness,
        flow: 1.0,
        spacing: 0.08,
        execution,
        rendering: BrushRendering {
            accumulation: BrushAccumulation::Uniform,
            ..BrushRendering::default()
        },
        stabilization: BrushStabilization { pressure_fall_micros: 80_000, ..BrushStabilization::default() },
        ..BrushSnapshot::default()
    }
}

struct PainterBrushSpec {
    diameter: f32,
    aspect: f32,
    flow: f32,
    opacity: f32,
    spacing: f32,
    hardness: f32,
    execution: BrushExecution,
    grain: Option<BrushGrain>,
    rendering: BrushRendering,
    wet_mix: BrushWetMix,
}

fn painter_brush(spec: PainterBrushSpec) -> BrushSnapshot {
    BrushSnapshot {
        // The tip owns only the contact silhouette. Reusing a detailed bristle
        // image here stamps the same internal lines at every contact, exposing
        // the dab cadence as diagonal bands when the stroke turns. Material
        // texture belongs in the independently sampled grain channel below.
        tip: BrushTip::AnalyticEllipse,
        diameter: spec.diameter,
        aspect: spec.aspect,
        flow: spec.flow,
        opacity: spec.opacity,
        spacing: spec.spacing,
        hardness: spec.hardness,
        execution: spec.execution,
        grain: spec.grain,
        rendering: spec.rendering,
        wet_mix: spec.wet_mix,
        color_dynamics: BrushColorDynamics {
            stroke_saturation_jitter: 0.035,
            ..BrushColorDynamics::default()
        },
        mappings: Arc::from([
            BrushMapping::pressure_size(),
            pressure_flow(0.22, 0.78),
            direction_rotation(),
        ]),
        ..BrushSnapshot::default()
    }
}

fn watercolor_brush(spec: PainterBrushSpec, transport: BrushTransport) -> BrushSnapshot {
    let mut brush = painter_brush(spec);
    brush.tip = BrushTip::Mask(AssetId::from(WATERCOLOR_TIP_TEXTURE_ASSET));
    brush.shape = BrushShape {
        size_jitter: 0.035,
        rotation_jitter: 1.0,
        ..BrushShape::default()
    };
    brush.transport = Some(transport);
    brush
}

pub(crate) fn paper_grain(depth: f32) -> BrushGrain {
    BrushGrain { depth }
}

fn wet_mix(
    amount_of_paint: f32,
    pull: f32,
    charge_depletion: f32,
    dilution: f32,
    wetness: f32,
    blur: f32,
) -> BrushWetMix {
    BrushWetMix {
        amount_of_paint,
        density: 0.88,
        charge: 1.0,
        charge_depletion,
        dilution,
        attack: 0.82,
        pull,
        blur,
        wetness_jitter: 0.10,
        wetness,
        mix_space: ColorMixSpace::Oklab,
        average: 0.0,
        refill: 0.0,
        stretch: 0.0,
    }
}

fn watercolor_mix(amount_of_paint: f32, pull: f32, dilution: f32, blur: f32) -> BrushWetMix {
    BrushWetMix {
        amount_of_paint,
        density: 0.88,
        dilution,
        attack: 0.82,
        pull,
        blur,
        // Watercolor owns its dedicated transport wetness channel and does
        // not allocate the generic wet-brush field or a brush reservoir.
        wetness: 0.0,
        mix_space: ColorMixSpace::Oklab,
        ..BrushWetMix::default()
    }
}

const fn pressure_flow(bias: f32, scale: f32) -> BrushMapping {
    BrushMapping {
        sensor: BrushSensor::Pressure,
        target: BrushTarget::Flow,
        combine: BrushCombine::Multiply,
        input_min: 0.0,
        input_max: 1.0,
        output_scale: scale,
        output_bias: bias,
        curve: BrushCurve::LINEAR,
    }
}

const fn direction_rotation() -> BrushMapping {
    BrushMapping {
        sensor: BrushSensor::Direction,
        target: BrushTarget::Rotation,
        combine: BrushCombine::Replace,
        input_min: 0.0,
        input_max: 1.0,
        output_scale: std::f32::consts::TAU,
        output_bias: 0.0,
        curve: BrushCurve::LINEAR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liquify_presets_use_their_modes_and_crystals_distorts() {
        for (preset, mode) in [
            (DefaultBrushPreset::LiquifyPush, LiquifyMode::Push),
            (DefaultBrushPreset::LiquifyTwirl, LiquifyMode::TwirlCounterclockwise),
            (DefaultBrushPreset::LiquifyTwirlClockwise, LiquifyMode::TwirlClockwise),
            (DefaultBrushPreset::LiquifyPinch, LiquifyMode::Pinch),
            (DefaultBrushPreset::LiquifyExpand, LiquifyMode::Expand),
            (DefaultBrushPreset::LiquifyCrystals, LiquifyMode::Crystals),
        ] {
            let brush = default_brush(preset);
            assert_eq!(brush.execution, BrushExecution::Liquify, "{preset:?}");
            assert_eq!(brush.deform.mode, mode, "{preset:?}");
            assert!(brush.deform.strength > 0., "{preset:?}");
        }
        assert!(default_brush(DefaultBrushPreset::LiquifyCrystals).deform.distortion > 0.);
    }

    #[test]
    fn every_builtin_is_valid() {
        for preset in DefaultBrushPreset::ALL {
            default_brush(preset).validate().unwrap();
        }
    }

    #[test]
    fn dry_authoring_values_roundtrip_and_reject_undefined_contact_fields() {
        let mut brush = default_brush(DefaultBrushPreset::Charcoal);
        let bytes = serde_json::to_vec(&brush).unwrap();
        let decoded: BrushSnapshot = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, brush);
        let model = brush.contact.unwrap().dry.unwrap();
        assert_eq!(model.pose(0.), model.poses[0]);
        assert_eq!(model.pose(40f32.to_radians()), model.poses[1]);
        assert_eq!(model.pose(60f32.to_radians()), model.poses[2]);
        for (softness, relief) in [(0., 0.), (f32::NAN, 0.), (0.07, -0.01), (0.07, f32::INFINITY)] {
            let dry = brush.contact.as_mut().unwrap().dry.as_mut().unwrap();
            dry.softness = softness;
            dry.poses[1].relief[0] = relief;
            assert!(brush.validate().is_err());
        }
        let mut smudging = default_brush(DefaultBrushPreset::Charcoal);
        smudging.execution = BrushExecution::Smudge;
        assert!(smudging.validate().is_err());
    }

    #[test]
    fn painter_presets_do_not_restart_texture_at_each_dab() {
        for preset in [
            DefaultBrushPreset::TexturedFlat,
            DefaultBrushPreset::DryScumble,
            DefaultBrushPreset::PastelBlock,
            DefaultBrushPreset::TransparentGlaze,
            DefaultBrushPreset::OpaqueGouache,
            DefaultBrushPreset::LoadedOil,
            DefaultBrushPreset::PaletteKnife,
            DefaultBrushPreset::NaturalBlender,
        ] {
            let brush = default_brush(preset);
            if let Some(dry) = brush.contact.and_then(|c| c.dry) {
                assert!(dry.corner > 0., "{preset:?}: flat stick contact");
            } else {
                assert_eq!(brush.tip, BrushTip::AnalyticEllipse, "{preset:?}");
            }
            let grain = brush.grain.expect("painter preset must provide grain");
            assert!(grain.depth > 0.0, "{preset:?}");
        }
    }

    #[test]
    fn watercolor_uses_transport_wetness_and_a_ragged_contact_silhouette() {
        for preset in [
            DefaultBrushPreset::WatercolorWash,
            DefaultBrushPreset::WetWatercolor,
        ] {
            let brush = default_brush(preset);
            assert_eq!(brush.execution, BrushExecution::Watercolor, "{preset:?}",);
            assert_eq!(
                brush.tip,
                BrushTip::Mask(AssetId::from(WATERCOLOR_TIP_TEXTURE_ASSET)),
                "{preset:?}",
            );
            assert!(brush.grain.is_none(), "{preset:?}");
            assert_eq!(brush.rendering.accumulation, BrushAccumulation::Uniform);
            assert!(!brush.rendering.edge_after_stroke);
            assert_eq!(brush.wet_mix.wetness, 0.0);
            assert!(brush.shape.size_jitter > 0.0);
            assert!(brush.shape.rotation_jitter > 0.0);
            let transport = brush.transport.expect("watercolor needs transport state");
            assert!(transport.wet_flow > transport.dry_flow, "{preset:?}");
            assert!(transport.distance > 0.0, "{preset:?}");
        }
        assert_eq!(
            default_brush(DefaultBrushPreset::WatercolorWash)
                .rendering
                .edge_width,
            10.0,
        );
        assert_eq!(
            default_brush(DefaultBrushPreset::WetWatercolor)
                .rendering
                .edge_width,
            8.0,
        );
    }
}
