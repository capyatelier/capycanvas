//! Brush controls are a shared schema, not a GTK form. Field access is kept
//! beside its label, constraints and visibility so native hosts never guess.
use crate::NumericControl;
use layer_core::{BrushExecution, BrushSnapshot, BrushTip, ColorMixSpace, LiquifyMode};
use serde::Serialize;

/// Reusable tool settings actions. Labels, enabled/checked state and shortcuts
/// come from the same command model as menus and toolbar tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ToolSettingAction {
    pub command: crate::CommandId,
    pub checkable: bool,
}

/// Mutually exclusive actions retain their bar or list presentation when compact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolActionGroup {
    SelectionMode,
    SelectionSource,
    TransformMode,
    TransformInterpolation,
    TransformWarpGrid,
    CropRatio,
    CropOverlay,
    ColorMixing,
}
impl ToolActionGroup {
    pub fn segmented(self) -> bool {
        matches!(self, Self::SelectionMode | Self::TransformMode)
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::SelectionMode => "selection-mode",
            Self::SelectionSource => "selection-source",
            Self::TransformMode => "transform-mode",
            Self::TransformInterpolation => "transform-interpolation",
            Self::TransformWarpGrid => "transform-warp-grid",
            Self::CropRatio => "crop-ratio",
            Self::CropOverlay => "crop-overlay",
            Self::ColorMixing => "color-mixing",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::SelectionMode | Self::TransformMode => "Mode",
            Self::SelectionSource => "Source",
            Self::TransformInterpolation => "Interpolation",
            Self::TransformWarpGrid => "Grid",
            Self::CropRatio => "Ratio",
            Self::CropOverlay => "Overlay",
            Self::ColorMixing => "Color mixing",
        }
    }
}
impl ToolSettingAction {
    pub fn group(self) -> Option<ToolActionGroup> {
        use crate::CommandId::*;
        match self.command {
            SelectionNew | SelectionAdd | SelectionSubtract | SelectionIntersect => {
                Some(ToolActionGroup::SelectionMode)
            }
            SelectionVisible | SelectionEditing | SelectionReference => {
                Some(ToolActionGroup::SelectionSource)
            }
            TransformFree | TransformUniform | TransformDistort | TransformWarp => Some(ToolActionGroup::TransformMode),
            WarpGridThree | WarpGridFour | WarpGridFive => Some(ToolActionGroup::TransformWarpGrid),
            TransformNearest | TransformBilinear | TransformBicubic | TransformLanczos => Some(ToolActionGroup::TransformInterpolation),
            CropRatioFree | CropRatioOriginal | CropRatioSquare | CropRatioFourFive | CropRatioTwoThree
            | CropRatioFiveSeven | CropRatioSixteenNine => Some(ToolActionGroup::CropRatio),
            CropOverlayThirds | CropOverlayGrid | CropOverlayDiagonal | CropOverlayGolden => {
                Some(ToolActionGroup::CropOverlay)
            }
            ColorMixOklab | ColorMixLinear | ColorMixClassic => Some(ToolActionGroup::ColorMixing),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolSetting {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub numeric: NumericControl,
    pub value: f32,
}

impl ToolSetting {
    pub fn tooltip(&self) -> &'static str {
        match self.id {
            "tonal_lower" => "From — lower bound in stops relative to reference white (0)",
            "tonal_upper" => "To — upper bound in stops relative to reference white (0)",
            "bristle_scale" => "Bristle texture size relative to the configured brush size",
            _ => self.label,
        }
    }
}

struct Definition {
    id: &'static str,
    label: &'static str,
    group: &'static str,
    numeric: fn() -> NumericControl,
    field: fn(&mut BrushSnapshot) -> Option<&mut f32>,
}

pub(crate) fn mixes_color(b: &BrushSnapshot) -> bool {
    matches!(
        b.execution_class(),
        BrushExecution::Wet | BrushExecution::Smudge | BrushExecution::Watercolor
    )
}

/// The brush's Color mixing choice, kept with the numeric overrides as its
/// `ColorMixSpace` discriminant.
pub(crate) const COLOR_MIXING: &str = "color_mixing";
pub(crate) const COLOR_MIXING_COMMANDS: [crate::CommandId; 3] =
    [crate::CommandId::ColorMixOklab, crate::CommandId::ColorMixLinear, crate::CommandId::ColorMixClassic];
pub(crate) const NOT_MIXING: &str = "Choose a brush that mixes paint first";

pub(crate) fn color_mixing(command: crate::CommandId) -> Option<ColorMixSpace> {
    use crate::CommandId::*;
    Some(match command {
        ColorMixOklab => ColorMixSpace::Oklab,
        ColorMixLinear => ColorMixSpace::LinearRgb,
        ColorMixClassic => ColorMixSpace::Classic,
        _ => return None,
    })
}

/// The stored value of a setting, including the Color mixing choice.
pub(crate) fn value(brush: &BrushSnapshot, id: &str) -> Option<f32> {
    if id == COLOR_MIXING {
        return mixes_color(brush).then_some(f32::from(brush.wet_mix.mix_space as u8));
    }
    let mut brush = brush.clone();
    let definition = DEFINITIONS.iter().find(|d| d.id == id)?;
    (definition.field)(&mut brush).copied()
}

const DEFINITIONS: &[Definition] = &[
    Definition {
        id: "size",
        label: "Brush size",
        group: "",
        numeric: NumericControl::brush_size,
        field: |b| Some(&mut b.diameter),
    },
    Definition {
        id: "opacity",
        label: "Opacity",
        group: "",
        numeric: NumericControl::percent,
        field: |b| Some(&mut b.opacity),
    },
    Definition {
        id: "flow",
        label: "Flow",
        group: "",
        numeric: NumericControl::percent,
        field: |b| (b.execution != BrushExecution::Liquify).then_some(&mut b.flow),
    },
    Definition {
        id: "hardness",
        label: "Hardness",
        group: "Tip",
        numeric: NumericControl::percent,
        field: |b| (b.tip == BrushTip::AnalyticEllipse).then_some(&mut b.hardness),
    },
    Definition {
        id: "spacing",
        label: "Spacing",
        group: "Tip",
        numeric: || NumericControl {
            min: 0.005,
            soft_min: 0.005,
            max: 10.0,
            soft_max: 1.0,
            ..NumericControl::percent()
        },
        field: |b| Some(&mut b.spacing),
    },
    Definition {
        id: "angle",
        label: "Angle",
        group: "Tip",
        numeric: || NumericControl {
            scale: 180.0 / std::f64::consts::PI,
            step: std::f64::consts::PI / 180.0,
            resolution: 0.0001,
            digits: 0,
            ..NumericControl::number(-std::f64::consts::PI, std::f64::consts::PI, 0.01, 2).unit("°")
        },
        field: |b| Some(&mut b.angle_radians),
    },
    Definition {
        id: "size_jitter",
        label: "Size variation",
        group: "Tip",
        numeric: NumericControl::percent,
        field: |b| Some(&mut b.shape.size_jitter),
    },
    Definition {
        id: "rotation_jitter",
        label: "Rotation variation",
        group: "Tip",
        numeric: NumericControl::percent,
        field: |b| Some(&mut b.shape.rotation_jitter),
    },
    Definition {
        id: "grain_depth",
        label: "Texture strength",
        group: "Texture",
        numeric: NumericControl::percent,
        field: |b| b.grain.as_mut().map(|g| &mut g.depth),
    },
    Definition {
        id: "bristle_scale",
        label: "Bristle scale",
        group: "Bristles",
        numeric: || NumericControl {
            min: 0.25,
            soft_min: 0.25,
            max: 4.,
            soft_max: 2.,
            ..NumericControl::percent()
        },
        field: |b| {
            b.contact
                .as_mut()?
                .bristles
                .as_mut()
                .map(|b| &mut b.texture_scale)
        },
    },
    Definition {
        id: "bristle_load",
        label: "Paint load",
        group: "Bristles",
        numeric: NumericControl::percent,
        field: |b| b.contact.as_mut()?.bristles.as_mut().map(|b| &mut b.load),
    },
    Definition {
        id: "paint",
        label: "Paint load",
        group: "Mixing",
        numeric: NumericControl::percent,
        field: |b| mixes_color(b).then_some(&mut b.wet_mix.amount_of_paint),
    },
    Definition {
        id: "pull",
        label: "Color pickup",
        group: "Mixing",
        numeric: NumericControl::percent,
        field: |b| mixes_color(b).then_some(&mut b.wet_mix.pull),
    },
    Definition {
        id: "dilution",
        label: "Dilution",
        group: "Mixing",
        numeric: NumericControl::percent,
        field: |b| mixes_color(b).then_some(&mut b.wet_mix.dilution),
    },
    Definition {
        id: "wet_edge",
        label: "Edge strength",
        group: "Watercolor",
        numeric: NumericControl::percent,
        field: |b| (b.execution == BrushExecution::Watercolor).then_some(&mut b.rendering.wet_edge),
    },
    Definition {
        id: "edge_width",
        label: "Edge width",
        group: "Watercolor",
        numeric: || NumericControl::number(0.0, 32.0, 1.0, 1).unit("px"),
        field: |b| {
            (b.execution == BrushExecution::Watercolor).then_some(&mut b.rendering.edge_width)
        },
    },
    Definition {
        id: "wet_flow",
        label: "Wet bleed",
        group: "Bleed",
        numeric: NumericControl::percent,
        field: |b| b.transport.as_mut().map(|t| &mut t.wet_flow),
    },
    Definition {
        id: "dry_flow",
        label: "Dry bleed",
        group: "Bleed",
        numeric: NumericControl::percent,
        field: |b| b.transport.as_mut().map(|t| &mut t.dry_flow),
    },
    Definition {
        id: "distance",
        label: "Bleed distance",
        group: "Bleed",
        numeric: || NumericControl::number(0.0, 96.0, 1.0, 1).unit("px"),
        field: |b| b.transport.as_mut().map(|t| &mut t.distance),
    },
    Definition {
        id: "water_load",
        label: "Water load",
        group: "Bleed",
        numeric: NumericControl::percent,
        field: |b| b.transport.as_mut().map(|t| &mut t.water_load),
    },
    Definition {
        id: "strength",
        label: "Strength",
        group: "Liquify",
        numeric: NumericControl::percent,
        field: |b| (b.execution == BrushExecution::Liquify).then_some(&mut b.deform.strength),
    },
    Definition {
        id: "distortion",
        label: "Distortion",
        group: "Liquify",
        numeric: NumericControl::percent,
        field: |b| liquify(b, LiquifyMode::Crystals).then_some(&mut b.deform.distortion),
    },
    Definition {
        id: "momentum",
        label: "Momentum",
        group: "Liquify",
        numeric: NumericControl::percent,
        field: |b| liquify(b, LiquifyMode::Push).then_some(&mut b.deform.momentum),
    },
];

fn liquify(b: &BrushSnapshot, mode: LiquifyMode) -> bool {
    b.execution == BrushExecution::Liquify && b.deform.mode == mode
}

pub(crate) fn controls(brush: &BrushSnapshot) -> Vec<ToolSetting> {
    let mut brush = brush.clone();
    DEFINITIONS
        .iter()
        .filter_map(|d| {
            Some(ToolSetting {
                id: d.id,
                label: d.label,
                group: d.group,
                numeric: (d.numeric)(),
                value: *(d.field)(&mut brush)?,
            })
        })
        .collect()
}

pub(crate) fn edit(brush: &BrushSnapshot, id: &str, value: f32) -> Result<BrushSnapshot, String> {
    if id == COLOR_MIXING {
        let space = ColorMixSpace::ALL
            .into_iter()
            .find(|space| f32::from(*space as u8) == value)
            .ok_or("Unknown color mixing")?;
        if !mixes_color(brush) {
            return Err(NOT_MIXING.into());
        }
        let mut next = brush.clone();
        next.wet_mix.mix_space = space;
        return Ok(next);
    }
    let d = DEFINITIONS
        .iter()
        .find(|d| d.id == id)
        .ok_or("Unknown tool setting")?;
    (d.numeric)().validate(value, d.label)?;
    let mut next = brush.clone();
    *(d.field)(&mut next).ok_or("This setting is not used by the selected tool")? = value;
    next.validate().map_err(|e| e.to_string())?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::{DefaultBrushPreset, default_brush};

    #[test]
    fn every_visible_control_accepts_its_default_and_endpoints() {
        for choice in crate::brush_catalog() {
            let brush = default_brush(crate::preset(choice.id).unwrap());
            for control in controls(&brush) {
                assert_eq!(
                    edit(&brush, control.id, control.value).unwrap(),
                    brush,
                    "{}: {}",
                    choice.label,
                    control.id
                );
                for value in [control.numeric.min as f32, control.numeric.max as f32] {
                    edit(&brush, control.id, value).unwrap();
                }
            }
        }
    }

    #[test]
    fn liquify_shows_distortion_for_crystals_and_momentum_for_push() {
        let ids = |preset| -> Vec<_> { controls(&default_brush(preset)).iter().map(|c| c.id).collect() };
        for (preset, distortion, momentum) in [
            (DefaultBrushPreset::LiquifyPush, false, true),
            (DefaultBrushPreset::LiquifyTwirl, false, false),
            (DefaultBrushPreset::LiquifyPinch, false, false),
            (DefaultBrushPreset::LiquifyCrystals, true, false),
        ] {
            let ids = ids(preset);
            assert!(ids.contains(&"strength"), "{preset:?}");
            assert_eq!(ids.contains(&"distortion"), distortion, "{preset:?}");
            assert_eq!(ids.contains(&"momentum"), momentum, "{preset:?}");
        }
        let crystals = default_brush(DefaultBrushPreset::LiquifyCrystals);
        assert_eq!(edit(&crystals, "distortion", 0.9).unwrap().deform.distortion, 0.9);
        assert!(edit(&crystals, "momentum", 0.5).is_err());
    }

    #[test]
    fn irrelevant_or_invalid_edits_are_rejected() {
        let brush = default_brush(DefaultBrushPreset::GPen);
        for (id, value) in [
            ("water_load", 0.5),
            ("strength", 0.5),
            ("distortion", 0.5),
            ("momentum", 0.5),
            ("nope", 1.0),
            ("flow", f32::NAN),
            ("size", 3000.0),
        ] {
            assert!(edit(&brush, id, value).is_err());
        }
    }

    #[test]
    fn bristle_controls_preserve_relative_scale_when_resizing_and_round_trip() {
        let brush = default_brush(DefaultBrushPreset::BristlePaintbrush);
        let scaled = edit(&brush, "bristle_scale", 2.).unwrap();
        let resized = edit(&scaled, "size", 1000.).unwrap();
        assert_eq!(scaled.contact, resized.contact);
        let restored: BrushSnapshot =
            serde_json::from_str(&serde_json::to_string(&resized).unwrap()).unwrap();
        assert_eq!(resized, restored);
        let catalog = crate::brush_catalog().collect::<Vec<_>>();
        let index = catalog
            .iter()
            .position(|p| p.id == DefaultBrushPreset::Paintbrush as u32)
            .unwrap();
        assert_eq!(
            catalog[index + 1].id,
            DefaultBrushPreset::BristlePaintbrush as u32
        );
        assert!(
            controls(&default_brush(DefaultBrushPreset::Paintbrush))
                .iter()
                .all(|c| c.id != "bristle_scale")
        );
    }
}
