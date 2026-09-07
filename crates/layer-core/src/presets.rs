use crate::{
    AssetId, BrushAccumulation, BrushBlendMode, BrushColorDynamics, BrushCombine, BrushCurve,
    BrushDeform, BrushExecution, BrushGrain, BrushGrainBehavior, BrushMapping, BrushPath,
    BrushRendering, BrushSensor, BrushShape, BrushSnapshot, BrushTarget, BrushTip, BrushTransport,
    BrushWetMix, ColorMixSpace, DualBrush, DualCombineMode, LiquifyMode,
};
use std::sync::Arc;

pub const PENCIL_TEXTURE_ASSET: &str = "builtin:brush-tip/pencil-grain-v1";
pub const PAINTBRUSH_TEXTURE_ASSET: &str = "builtin:brush-tip/paint-bristles-v1";
pub const PAPER_GRAIN_TEXTURE_ASSET: &str = "builtin:brush-grain/paper-v1";
pub const BRISTLE_GRAIN_TEXTURE_ASSET: &str = "builtin:brush-grain/bristle-v1";
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
}

/// Returns a complete immutable preset snapshot. Callers may override color,
/// diameter, and opacity before beginning a stroke.
pub fn default_brush(preset: DefaultBrushPreset) -> BrushSnapshot {
    match preset {
        DefaultBrushPreset::GPen => BrushSnapshot {
            tip: BrushTip::AnalyticEllipse,
            color_rgba_linear: [0.006, 0.006, 0.005, 1.0],
            diameter: 18.0,
            opacity: 1.0,
            hardness: 0.94,
            flow: 1.0,
            spacing: 0.08,
            mappings: Arc::from([BrushMapping::pressure_size()]),
            ..BrushSnapshot::default()
        },
        DefaultBrushPreset::Pencil => BrushSnapshot {
            tip: BrushTip::Mask(AssetId::from(PENCIL_TEXTURE_ASSET)),
            color_rgba_linear: [0.018, 0.018, 0.016, 1.0],
            diameter: 56.0,
            opacity: 0.82,
            hardness: 1.0,
            flow: 0.22,
            spacing: 0.075,
            seed: 0x5045_4e43,
            mappings: Arc::from([BrushMapping::pressure_size(), pressure_flow(0.12, 0.88)]),
            ..BrushSnapshot::default()
        },
        DefaultBrushPreset::Eraser => BrushSnapshot {
            tip: BrushTip::AnalyticEllipse,
            color_rgba_linear: [0.0, 0.0, 0.0, 1.0],
            diameter: 180.0,
            opacity: 1.0,
            hardness: 0.88,
            flow: 1.0,
            spacing: 0.08,
            seed: 0x4552_4153,
            mappings: Arc::from([BrushMapping::pressure_size()]),
            ..BrushSnapshot::default()
        },
        DefaultBrushPreset::Paintbrush => BrushSnapshot {
            tip: BrushTip::Mask(AssetId::from(PAINTBRUSH_TEXTURE_ASSET)),
            color_rgba_linear: [0.12, 0.025, 0.012, 1.0],
            diameter: 460.0,
            opacity: 0.88,
            hardness: 1.0,
            flow: 0.32,
            spacing: 0.20,
            aspect: 0.68,
            seed: 0x5041_494e,
            mappings: Arc::from([
                BrushMapping::pressure_size(),
                pressure_flow(0.18, 0.82),
                direction_rotation(),
            ]),
            ..BrushSnapshot::default()
        },
        DefaultBrushPreset::Airbrush => BrushSnapshot {
            color_rgba_linear: [0.04, 0.12, 0.42, 1.0],
            diameter: 320.0,
            hardness: 0.05,
            flow: 0.08,
            spacing: 0.055,
            seed: 0x4149_5242,
            path: BrushPath {
                continuous_rate_hz: 60.0,
                ..BrushPath::default()
            },
            mappings: Arc::from([BrushMapping::pressure_size(), pressure_flow(0.08, 0.72)]),
            ..BrushSnapshot::default()
        },
        DefaultBrushPreset::Chalk => BrushSnapshot {
            tip: BrushTip::Mask(AssetId::from(PENCIL_TEXTURE_ASSET)),
            color_rgba_linear: [0.62, 0.12, 0.045, 1.0],
            diameter: 110.0,
            flow: 0.32,
            spacing: 0.11,
            seed: 0x4348_414c,
            grain: Some(BrushGrain {
                asset: AssetId::from(PAPER_GRAIN_TEXTURE_ASSET),
                behavior: BrushGrainBehavior::Canvas,
                scale: 3.0,
                depth: 0.72,
                rotation_radians: 0.17,
                offset_jitter: 0.15,
            }),
            color_dynamics: BrushColorDynamics {
                stamp_lightness_jitter: 0.08,
                ..BrushColorDynamics::default()
            },
            mappings: Arc::from([BrushMapping::pressure_size(), pressure_flow(0.12, 0.72)]),
            ..BrushSnapshot::default()
        },
        DefaultBrushPreset::Marker => BrushSnapshot {
            color_rgba_linear: [0.72, 0.06, 0.16, 0.82],
            diameter: 180.0,
            hardness: 0.78,
            flow: 0.26,
            spacing: 0.055,
            aspect: 0.42,
            angle_radians: -0.45,
            mappings: Arc::from([BrushMapping::pressure_size(), direction_rotation()]),
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
            tip: BrushTip::Mask(AssetId::from(PAINTBRUSH_TEXTURE_ASSET)),
            color_rgba_linear: [0.08, 0.025, 0.42, 1.0],
            diameter: 260.0,
            flow: 0.36,
            spacing: 0.16,
            aspect: 0.72,
            grain: Some(BrushGrain {
                asset: AssetId::from(PAPER_GRAIN_TEXTURE_ASSET),
                behavior: BrushGrainBehavior::Canvas,
                scale: 2.4,
                depth: 0.58,
                rotation_radians: 0.0,
                offset_jitter: 0.0,
            }),
            dual: Some(Arc::new(DualBrush {
                tip: BrushTip::Mask(AssetId::from(PENCIL_TEXTURE_ASSET)),
                grain: None,
                combine: DualCombineMode::Multiply,
                scale: 0.74,
                aspect: 1.3,
                angle_radians: 0.55,
                offset: [0.08, -0.04],
            })),
            mappings: Arc::from([BrushMapping::pressure_size(), direction_rotation()]),
            ..BrushSnapshot::default()
        },
        DefaultBrushPreset::Smudge => BrushSnapshot {
            color_rgba_linear: [0.0, 0.0, 0.0, 1.0],
            diameter: 220.0,
            hardness: 0.42,
            flow: 0.72,
            spacing: 0.04,
            execution: BrushExecution::Smudge,
            wet_mix: BrushWetMix {
                amount_of_paint: 0.0,
                density: 0.92,
                charge: 1.0,
                charge_depletion: 0.12,
                dilution: 0.0,
                attack: 0.82,
                pull: 0.88,
                blur: 0.12,
                wetness_jitter: 0.0,
                wetness: 0.08,
                mix_space: ColorMixSpace::Oklab,
            },
            mappings: Arc::from([BrushMapping::pressure_size()]),
            ..BrushSnapshot::default()
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
            },
            mappings: Arc::from([BrushMapping::pressure_size(), pressure_flow(0.18, 0.72)]),
            ..BrushSnapshot::default()
        },
        DefaultBrushPreset::LiquifyPush => BrushSnapshot {
            diameter: 420.0,
            hardness: 0.34,
            flow: 1.0,
            spacing: 0.08,
            execution: BrushExecution::Liquify,
            deform: BrushDeform {
                mode: LiquifyMode::Push,
                strength: 0.82,
                pressure: 1.0,
                momentum: 0.28,
                distortion: 0.0,
            },
            mappings: Arc::from([BrushMapping::pressure_size()]),
            ..BrushSnapshot::default()
        },
        DefaultBrushPreset::LiquifyTwirl => BrushSnapshot {
            diameter: 620.0,
            hardness: 0.22,
            flow: 1.0,
            spacing: 0.22,
            execution: BrushExecution::Liquify,
            deform: BrushDeform {
                mode: LiquifyMode::TwirlClockwise,
                strength: 0.72,
                pressure: 1.0,
                momentum: 0.0,
                distortion: 0.0,
            },
            mappings: Arc::from([BrushMapping::pressure_size()]),
            ..BrushSnapshot::default()
        },
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
        DefaultBrushPreset::TexturedFlat => painter_brush(PainterBrushSpec {
            diameter: 360.0,
            aspect: 0.62,
            flow: 0.42,
            opacity: 0.72,
            spacing: 0.16,
            hardness: 0.38,
            execution: BrushExecution::Dry,
            grain: Some(canvas_grain(PAPER_GRAIN_TEXTURE_ASSET, 1.7, 0.62, 0.08)),
            rendering: BrushRendering::default(),
            wet_mix: BrushWetMix::default(),
        }),
        DefaultBrushPreset::DryScumble => painter_brush(PainterBrushSpec {
            diameter: 430.0,
            aspect: 0.72,
            flow: 0.27,
            opacity: 0.86,
            spacing: 0.22,
            hardness: 0.52,
            execution: BrushExecution::Dry,
            grain: Some(canvas_grain(PAPER_GRAIN_TEXTURE_ASSET, 3.4, 0.82, 0.21)),
            rendering: BrushRendering {
                accumulation: BrushAccumulation::Uniform,
                alpha_threshold: 0.08,
                ..BrushRendering::default()
            },
            wet_mix: BrushWetMix::default(),
        }),
        DefaultBrushPreset::PastelBlock => painter_brush(PainterBrushSpec {
            diameter: 280.0,
            aspect: 0.58,
            flow: 0.34,
            opacity: 0.78,
            spacing: 0.14,
            hardness: 0.46,
            execution: BrushExecution::Dry,
            grain: Some(canvas_grain(PAPER_GRAIN_TEXTURE_ASSET, 5.2, 0.76, -0.13)),
            rendering: BrushRendering::default(),
            wet_mix: BrushWetMix::default(),
        }),
        DefaultBrushPreset::TransparentGlaze => painter_brush(PainterBrushSpec {
            diameter: 520.0,
            aspect: 0.52,
            flow: 0.19,
            opacity: 0.62,
            spacing: 0.08,
            hardness: 0.68,
            execution: BrushExecution::Dry,
            grain: Some(canvas_grain(PAPER_GRAIN_TEXTURE_ASSET, 1.9, 0.34, 0.0)),
            rendering: BrushRendering::default(),
            wet_mix: BrushWetMix {
                wetness: 0.28,
                wetness_jitter: 0.08,
                ..BrushWetMix::default()
            },
        }),
        DefaultBrushPreset::OpaqueGouache => painter_brush(PainterBrushSpec {
            diameter: 410.0,
            aspect: 0.68,
            flow: 0.76,
            opacity: 0.84,
            spacing: 0.05,
            hardness: 0.54,
            execution: BrushExecution::Wet,
            grain: Some(canvas_grain(PAPER_GRAIN_TEXTURE_ASSET, 2.2, 0.48, 0.04)),
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
            grain: Some(canvas_grain(PAPER_GRAIN_TEXTURE_ASSET, 1.15, 0.72, 0.03)),
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
            grain: Some(canvas_grain(PAPER_GRAIN_TEXTURE_ASSET, 0.82, 0.86, -0.05)),
            rendering: BrushRendering::default(),
            wet_mix: wet_mix(0.82, 0.88, 0.025, 0.16, 0.18, 0.08),
        }),
        DefaultBrushPreset::NaturalBlender => painter_brush(PainterBrushSpec {
            diameter: 440.0,
            aspect: 0.64,
            flow: 0.78,
            opacity: 0.56,
            spacing: 0.04,
            hardness: 0.66,
            execution: BrushExecution::Smudge,
            grain: Some(canvas_grain(PAPER_GRAIN_TEXTURE_ASSET, 1.6, 0.36, 0.08)),
            rendering: BrushRendering::default(),
            wet_mix: wet_mix(0.0, 0.94, 0.040, 0.86, 0.0, 0.20),
        }),
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

fn canvas_grain(asset: &'static str, scale: f32, depth: f32, rotation: f32) -> BrushGrain {
    BrushGrain {
        asset: AssetId::from(asset),
        behavior: BrushGrainBehavior::Canvas,
        scale,
        depth,
        rotation_radians: rotation,
        // A canvas-locked texture must stay fixed between adjacent contacts;
        // per-dab offsets expose the stamp footprint instead of paper grain.
        offset_jitter: 0.0,
    }
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
    fn every_builtin_is_valid() {
        for preset in [
            DefaultBrushPreset::GPen,
            DefaultBrushPreset::Pencil,
            DefaultBrushPreset::Eraser,
            DefaultBrushPreset::Paintbrush,
            DefaultBrushPreset::Airbrush,
            DefaultBrushPreset::Chalk,
            DefaultBrushPreset::Marker,
            DefaultBrushPreset::Spray,
            DefaultBrushPreset::DualTexture,
            DefaultBrushPreset::Smudge,
            DefaultBrushPreset::WetRound,
            DefaultBrushPreset::LiquifyPush,
            DefaultBrushPreset::LiquifyTwirl,
            DefaultBrushPreset::MultiplyGlaze,
            DefaultBrushPreset::TexturedFlat,
            DefaultBrushPreset::DryScumble,
            DefaultBrushPreset::PastelBlock,
            DefaultBrushPreset::TransparentGlaze,
            DefaultBrushPreset::OpaqueGouache,
            DefaultBrushPreset::WatercolorWash,
            DefaultBrushPreset::WetWatercolor,
            DefaultBrushPreset::LoadedOil,
            DefaultBrushPreset::PaletteKnife,
            DefaultBrushPreset::NaturalBlender,
        ] {
            default_brush(preset).validate().unwrap();
        }
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
            assert_eq!(brush.tip, BrushTip::AnalyticEllipse, "{preset:?}");
            let grain = brush.grain.expect("painter preset must provide grain");
            assert_eq!(grain.behavior, BrushGrainBehavior::Canvas, "{preset:?}");
            assert_eq!(
                grain.asset.0.as_ref(),
                PAPER_GRAIN_TEXTURE_ASSET,
                "{preset:?}"
            );
            assert_eq!(grain.offset_jitter, 0.0, "{preset:?}");
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
