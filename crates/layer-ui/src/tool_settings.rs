//! Brush controls are a shared schema, not a GTK form. Field access is kept
//! beside its label, constraints and visibility so native hosts never guess.
use crate::NumericControl;
use crate::localization::{Localizer, MessageId, UiLanguage};
use std::sync::Arc;
use layer_core::{BrushExecution, BrushSnapshot, BrushTip, ColorMixSpace, LiquifyMode};
use serde::Serialize;

/// Reusable tool settings actions. Labels, enabled/checked state and shortcuts
/// come from the same command model as menus and toolbar tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ToolSettingAction {
    pub command: crate::CommandId,
    pub checkable: bool,
}

macro_rules! tool_action_groups {
    ($($group:ident: $id:literal, $label:ident, $segmented:literal, [$($command:ident),+];)+) => {
        /// Mutually exclusive actions retain their bar or list presentation when compact.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum ToolActionGroup { $($group),+ }
        impl ToolActionGroup {
            pub fn segmented(self) -> bool {
                match self { $(Self::$group => $segmented),+ }
            }
            pub fn id(self) -> &'static str {
                match self { $(Self::$group => $id),+ }
            }
            pub fn message_id(self) -> MessageId { match self { $(Self::$group => MessageId::$label),+ } }
            pub fn localized_label(self, localizer: &Localizer) -> Arc<str> { localizer.text(self.message_id()) }
            pub fn label(self) -> Arc<str> { self.localized_label(&Localizer::shared(UiLanguage::English)) }
        }
        impl ToolSettingAction {
            pub fn group(self) -> Option<ToolActionGroup> {
                match self.command {
                    $($(crate::CommandId::$command)|+ => Some(ToolActionGroup::$group),)+
                    _ => None,
                }
            }
        }
    };
}

tool_action_groups! {
    SelectionMode: "selection-mode", TOOL_ACTION_GROUP_SELECTION_MODE, true,
        [SelectionNew, SelectionAdd, SelectionSubtract, SelectionIntersect];
    SelectionSource: "selection-source", TOOL_ACTION_GROUP_SELECTION_SOURCE, false,
        [SelectionVisible, SelectionEditing, SelectionReference];
    TransformMode: "transform-mode", TOOL_ACTION_GROUP_TRANSFORM_MODE, true,
        [TransformFree, TransformUniform, TransformDistort, TransformWarp];
    TransformInterpolation: "transform-interpolation", TOOL_ACTION_GROUP_TRANSFORM_INTERPOLATION, false,
        [TransformNearest, TransformBilinear, TransformBicubic, TransformLanczos];
    TransformWarpGrid: "transform-warp-grid", TOOL_ACTION_GROUP_TRANSFORM_WARP_GRID, false,
        [WarpGridThree, WarpGridFour, WarpGridFive];
    CropRatio: "crop-ratio", TOOL_ACTION_GROUP_CROP_RATIO, false,
        [CropRatioFree, CropRatioOriginal, CropRatioSquare, CropRatioFourFive, CropRatioTwoThree, CropRatioFiveSeven, CropRatioSixteenNine];
    CropOverlay: "crop-overlay", TOOL_ACTION_GROUP_CROP_OVERLAY, false,
        [CropOverlayThirds, CropOverlayGrid, CropOverlayDiagonal, CropOverlayGolden];
    ColorMixing: "color-mixing", TOOL_ACTION_GROUP_COLOR_MIXING, false,
        [ColorMixOklab, ColorMixLinear, ColorMixClassic];
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolSetting {
    pub id: &'static str,
    pub label: Arc<str>,
    pub group: Arc<str>,
    pub numeric: NumericControl,
    pub value: f32,
}

impl ToolSetting {
    pub fn tooltip_localized(&self, localizer: &Localizer) -> Arc<str> {
        match self.id {
            "tonal_lower" => localizer.text(MessageId::TOOL_SETTING_TONAL_LOWER_TOOLTIP),
            "tonal_upper" => localizer.text(MessageId::TOOL_SETTING_TONAL_UPPER_TOOLTIP),
            "bristle_scale" => localizer.text(MessageId::TOOL_SETTING_BRISTLE_SCALE_TOOLTIP),
            _ => self.label.clone(),
        }
    }
    pub fn tooltip(&self) -> Arc<str> { self.tooltip_localized(&Localizer::shared(UiLanguage::English)) }
}

struct Definition {
    id: &'static str,
    label: MessageId,
    group: Option<MessageId>,
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
        label: MessageId::TOOL_SETTING_SIZE,
        group: None,
        numeric: NumericControl::brush_size,
        field: |b| Some(&mut b.diameter),
    },
    Definition {
        id: "opacity",
        label: MessageId::TOOL_SETTING_OPACITY,
        group: None,
        numeric: NumericControl::percent,
        field: |b| Some(&mut b.opacity),
    },
    Definition {
        id: "flow",
        label: MessageId::TOOL_SETTING_FLOW,
        group: None,
        numeric: NumericControl::percent,
        field: |b| (b.execution != BrushExecution::Liquify).then_some(&mut b.flow),
    },
    Definition {
        id: "hardness",
        label: MessageId::TOOL_SETTING_HARDNESS,
        group: Some(MessageId::TOOL_SETTING_GROUP_TIP),
        numeric: NumericControl::percent,
        field: |b| (b.tip == BrushTip::AnalyticEllipse).then_some(&mut b.hardness),
    },
    Definition {
        id: "spacing",
        label: MessageId::TOOL_SETTING_SPACING,
        group: Some(MessageId::TOOL_SETTING_GROUP_TIP),
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
        label: MessageId::TOOL_SETTING_ANGLE,
        group: Some(MessageId::TOOL_SETTING_GROUP_TIP),
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
        label: MessageId::TOOL_SETTING_SIZE_JITTER,
        group: Some(MessageId::TOOL_SETTING_GROUP_TIP),
        numeric: NumericControl::percent,
        field: |b| Some(&mut b.shape.size_jitter),
    },
    Definition {
        id: "rotation_jitter",
        label: MessageId::TOOL_SETTING_ROTATION_JITTER,
        group: Some(MessageId::TOOL_SETTING_GROUP_TIP),
        numeric: NumericControl::percent,
        field: |b| Some(&mut b.shape.rotation_jitter),
    },
    Definition {
        id: "grain_depth",
        label: MessageId::TOOL_SETTING_GRAIN_DEPTH,
        group: Some(MessageId::TOOL_SETTING_GROUP_TEXTURE),
        numeric: NumericControl::percent,
        field: |b| b.grain.as_mut().map(|g| &mut g.depth),
    },
    Definition {
        id: "bristle_scale",
        label: MessageId::TOOL_SETTING_BRISTLE_SCALE,
        group: Some(MessageId::TOOL_SETTING_GROUP_BRISTLES),
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
        label: MessageId::TOOL_SETTING_BRISTLE_LOAD,
        group: Some(MessageId::TOOL_SETTING_GROUP_BRISTLES),
        numeric: NumericControl::percent,
        field: |b| b.contact.as_mut()?.bristles.as_mut().map(|b| &mut b.load),
    },
    Definition {
        id: "paint",
        label: MessageId::TOOL_SETTING_PAINT,
        group: Some(MessageId::TOOL_SETTING_GROUP_MIXING),
        numeric: NumericControl::percent,
        field: |b| mixes_color(b).then_some(&mut b.wet_mix.amount_of_paint),
    },
    Definition {
        id: "pull",
        label: MessageId::TOOL_SETTING_PULL,
        group: Some(MessageId::TOOL_SETTING_GROUP_MIXING),
        numeric: NumericControl::percent,
        field: |b| mixes_color(b).then_some(&mut b.wet_mix.pull),
    },
    Definition {
        id: "dilution",
        label: MessageId::TOOL_SETTING_DILUTION,
        group: Some(MessageId::TOOL_SETTING_GROUP_MIXING),
        numeric: NumericControl::percent,
        field: |b| mixes_color(b).then_some(&mut b.wet_mix.dilution),
    },
    Definition {
        id: "wet_edge",
        label: MessageId::TOOL_SETTING_WET_EDGE,
        group: Some(MessageId::TOOL_SETTING_GROUP_WATERCOLOR),
        numeric: NumericControl::percent,
        field: |b| (b.execution == BrushExecution::Watercolor).then_some(&mut b.rendering.wet_edge),
    },
    Definition {
        id: "edge_width",
        label: MessageId::TOOL_SETTING_EDGE_WIDTH,
        group: Some(MessageId::TOOL_SETTING_GROUP_WATERCOLOR),
        numeric: || NumericControl::number(0.0, 32.0, 1.0, 1).unit("px"),
        field: |b| {
            (b.execution == BrushExecution::Watercolor).then_some(&mut b.rendering.edge_width)
        },
    },
    Definition {
        id: "wet_flow",
        label: MessageId::TOOL_SETTING_WET_FLOW,
        group: Some(MessageId::TOOL_SETTING_GROUP_BLEED),
        numeric: NumericControl::percent,
        field: |b| b.transport.as_mut().map(|t| &mut t.wet_flow),
    },
    Definition {
        id: "dry_flow",
        label: MessageId::TOOL_SETTING_DRY_FLOW,
        group: Some(MessageId::TOOL_SETTING_GROUP_BLEED),
        numeric: NumericControl::percent,
        field: |b| b.transport.as_mut().map(|t| &mut t.dry_flow),
    },
    Definition {
        id: "distance",
        label: MessageId::TOOL_SETTING_DISTANCE,
        group: Some(MessageId::TOOL_SETTING_GROUP_BLEED),
        numeric: || NumericControl::number(0.0, 96.0, 1.0, 1).unit("px"),
        field: |b| b.transport.as_mut().map(|t| &mut t.distance),
    },
    Definition {
        id: "water_load",
        label: MessageId::TOOL_SETTING_WATER_LOAD,
        group: Some(MessageId::TOOL_SETTING_GROUP_BLEED),
        numeric: NumericControl::percent,
        field: |b| b.transport.as_mut().map(|t| &mut t.water_load),
    },
    Definition {
        id: "strength",
        label: MessageId::TOOL_SETTING_STRENGTH,
        group: Some(MessageId::TOOL_SETTING_GROUP_LIQUIFY),
        numeric: NumericControl::percent,
        field: |b| (b.execution == BrushExecution::Liquify).then_some(&mut b.deform.strength),
    },
    Definition {
        id: "distortion",
        label: MessageId::TOOL_SETTING_DISTORTION,
        group: Some(MessageId::TOOL_SETTING_GROUP_LIQUIFY),
        numeric: NumericControl::percent,
        field: |b| liquify(b, LiquifyMode::Crystals).then_some(&mut b.deform.distortion),
    },
    Definition {
        id: "momentum",
        label: MessageId::TOOL_SETTING_MOMENTUM,
        group: Some(MessageId::TOOL_SETTING_GROUP_LIQUIFY),
        numeric: NumericControl::percent,
        field: |b| liquify(b, LiquifyMode::Push).then_some(&mut b.deform.momentum),
    },
];

fn liquify(b: &BrushSnapshot, mode: LiquifyMode) -> bool {
    b.execution == BrushExecution::Liquify && b.deform.mode == mode
}

pub(crate) fn controls(brush: &BrushSnapshot, localizer: &Localizer) -> Vec<ToolSetting> {
    let mut brush = brush.clone();
    DEFINITIONS
        .iter()
        .filter_map(|d| {
            Some(ToolSetting {
                id: d.id,
                label: localizer.text(d.label),
                group: d.group.map(|id| localizer.text(id)).unwrap_or_else(|| Arc::from("")),
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
    (d.numeric)().validate(value, &Localizer::shared(UiLanguage::English).text(d.label))?;
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
    fn option_control_localization_preserves_ids_values_and_numeric_rules() {
        let project = |localizer: &Localizer| {
            let mut selection = crate::SelectionOptions::default();
            selection.constraint = crate::SelectionConstraint::Ratio;
            let mut controls = selection.controls(localizer);
            controls.extend(selection.edge_controls(localizer));
            selection.constraint = crate::SelectionConstraint::Size;
            controls.extend(selection.controls(localizer));
            controls.extend(crate::SelectionBrushOptions::default().controls(localizer));
            controls.extend(crate::TonalOptions { tone: 7, ..Default::default() }.controls(localizer));
            controls
        };
        let canonical = project(&Localizer::shared(UiLanguage::English));
        for language in UiLanguage::ALL {
            let controls = project(&Localizer::shared(language));
            assert_eq!(controls.len(), canonical.len());
            for (control, expected) in controls.iter().zip(&canonical) {
                assert_eq!((control.id, control.value, &control.numeric), (expected.id, expected.value, &expected.numeric));
                assert!(!control.label.is_empty());
                control.numeric.validate(control.value, &control.label).unwrap();
            }
        }
    }

    #[test]
    fn every_visible_control_accepts_its_default_and_endpoints() {
        for choice in crate::brush_catalog() {
            let brush = default_brush(crate::preset(choice.id).unwrap());
            for control in controls(&brush, &Localizer::shared(UiLanguage::English)) {
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
        let ids = |preset| -> Vec<_> { controls(&default_brush(preset), &Localizer::shared(UiLanguage::English)).iter().map(|c| c.id).collect() };
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
            controls(&default_brush(DefaultBrushPreset::Paintbrush), &Localizer::shared(UiLanguage::English))
                .iter()
                .all(|c| c.id != "bristle_scale")
        );
    }
}
