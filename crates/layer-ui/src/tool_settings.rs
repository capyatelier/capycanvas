//! Brush controls are a shared schema, not a GTK form. Field access is kept
//! beside its label, constraints and visibility so native hosts never guess.
use crate::NumericControl;
use crate::localization::{Localizer, MessageId, UiLanguage};
use std::sync::Arc;
use layer_core::{BlenderKind, BrushExecution, BrushSnapshot, BrushTip, ColorMixSpace, LiquifyMode};
use serde::Serialize;

pub(crate) mod profiles;

/// Reusable tool settings actions. Labels, enabled/checked state and shortcuts
/// come from the same command model as menus and toolbar tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(into = "ToolSettingActionView")]
pub struct ToolSettingAction {
    pub command: crate::CommandId,
    pub checkable: bool,
}

#[derive(Serialize)]
struct ToolSettingActionView {
    command: crate::CommandId,
    checkable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    group: Option<&'static str>,
    segmented: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    row: Option<&'static str>,
    icon_only: bool,
}
impl From<ToolSettingAction> for ToolSettingActionView {
    fn from(action: ToolSettingAction) -> Self {
        Self {
            command: action.command, checkable: action.checkable,
            group: action.group().map(ToolActionGroup::id),
            segmented: action.group().is_some_and(ToolActionGroup::segmented),
            row: action.row().map(ToolActionRow::id),
            icon_only: action.row().is_some_and(ToolActionRow::icon_only),
        }
    }
}

macro_rules! tool_action_groups {
    ($($group:ident: $id:literal, $label:ident, $segmented:literal, [$($command:ident),+];)+) => {
        /// Mutually exclusive actions retain their bar or list presentation when compact.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum ToolActionGroup { $($group),+ }
        impl ToolActionGroup {
            pub const ALL: &'static [Self] = &[$(Self::$group),+];
            pub fn segmented(self) -> bool {
                match self { $(Self::$group => $segmented),+ }
            }
            pub fn id(self) -> &'static str {
                match self { $(Self::$group => $id),+ }
            }
            pub fn commands(self) -> &'static [crate::CommandId] {
                match self { $(Self::$group => &[$(crate::CommandId::$command),+]),+ }
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

#[derive(Clone, Debug, Serialize)]
pub struct ToolActionGroupView {
    pub id: &'static str,
    pub label: Arc<str>,
    pub segmented: bool,
    pub commands: &'static [crate::CommandId],
}
impl ToolActionGroupView {
    pub fn catalog(localizer: &Localizer) -> Vec<Self> {
        ToolActionGroup::ALL.iter().map(|group| Self {
            id: group.id(), label: group.localized_label(localizer),
            segmented: group.segmented(), commands: group.commands(),
        }).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolActionRow { Flip, Rotate, Completion }
impl ToolActionRow {
    pub const ALL: &'static [Self] = &[Self::Flip, Self::Rotate, Self::Completion];
    pub fn id(self) -> &'static str {
        match self { Self::Flip => "transform-flip", Self::Rotate => "transform-rotate", Self::Completion => "completion" }
    }
    pub fn commands(self) -> &'static [crate::CommandId] {
        use crate::CommandId::*;
        match self {
            Self::Flip => &[TransformFlipHorizontal, TransformFlipVertical],
            Self::Rotate => &[TransformRotateLeft, TransformRotateRight],
            Self::Completion => &[ApplyTransform, CancelTransform, CompleteSelection, CancelSelection],
        }
    }
    pub fn icon_only(self) -> bool { self != Self::Completion }
}
impl ToolSettingAction {
    pub fn row(self) -> Option<ToolActionRow> {
        ToolActionRow::ALL.iter().copied().find(|row| row.commands().contains(&self.command))
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct ToolActionRowView {
    pub id: &'static str,
    pub commands: &'static [crate::CommandId],
    pub icon_only: bool,
}
impl ToolActionRowView {
    pub fn catalog() -> Vec<Self> {
        ToolActionRow::ALL.iter().map(|row| Self { id: row.id(), commands: row.commands(), icon_only: row.icon_only() }).collect()
    }
}

tool_action_groups! {
    SelectionMode: "selection-mode", TOOL_ACTION_GROUP_SELECTION_MODE, true,
        [SelectionNew, SelectionAdd, SelectionSubtract, SelectionIntersect];
    FillScope: "fill-scope", COMMAND_FILL, false,
        [FillConnected, FillEverywhere];
    SelectionSource: "selection-source", TOOL_ACTION_GROUP_SELECTION_SOURCE, false,
        [SelectionReference, SelectionVisible, SelectionEditing];
    TransformMode: "transform-mode", TOOL_ACTION_GROUP_TRANSFORM_MODE, true,
        [TransformFree, TransformDistort, TransformWarp];
    TransformInterpolation: "transform-interpolation", TOOL_ACTION_GROUP_TRANSFORM_INTERPOLATION, false,
        [TransformNearest, TransformBilinear, TransformBicubic, TransformLanczos];
    TransformWarpGrid: "transform-warp-grid", TOOL_ACTION_GROUP_TRANSFORM_WARP_GRID, false,
        [WarpGridThree, WarpGridFour, WarpGridFive];
    TransformWarpSplit: "transform-warp-split", TOOL_ACTION_GROUP_TRANSFORM_WARP_SPLIT, false,
        [WarpSplitVertical, WarpSplitHorizontal, WarpSplitCross];
    CropRatio: "crop-ratio", TOOL_ACTION_GROUP_CROP_RATIO, false,
        [CropRatioFree, CropRatioOriginal, CropRatioSquare, CropRatioFourFive, CropRatioTwoThree, CropRatioFiveSeven, CropRatioSixteenNine];
    CropOverlay: "crop-overlay", TOOL_ACTION_GROUP_CROP_OVERLAY, false,
        [CropOverlayThirds, CropOverlayGrid, CropOverlayDiagonal, CropOverlayGolden];
    ColorMixing: "color-mixing", TOOL_ACTION_GROUP_COLOR_MIXING, false,
        [ColorMixOklab, ColorMixLinear, ColorMixClassic];
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolSetting {
    pub preview: bool,
    pub id: &'static str,
    pub label: Arc<str>,
    #[serde(skip)]
    pub(crate) label_id: MessageId,
    pub group_id: &'static str,
    pub group: Arc<str>,
    pub numeric: NumericControl,
    pub value: f32,
}

impl ToolSetting {
    pub(crate) fn ungrouped(mut self) -> Self {
        self.group_id = "";
        self.group = "".into();
        self
    }
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

impl Definition {
    fn applicable(&self, brush: &BrushSnapshot) -> bool {
        match self.id {
            "paint" | "dilution" => brush.execution_class() != BrushExecution::Smudge,
            "pull" => brush.execution_class() != BrushExecution::Smudge || !matches!(brush.blender.kind,
                BlenderKind::Blur | BlenderKind::Sharpen | BlenderKind::Crystal | BlenderKind::Comb | BlenderKind::Dither),
            "persistence" => brush.wet_mix.average > 0.,
            _ => true,
        }
    }

    fn numeric(&self, brush: &BrushSnapshot) -> NumericControl {
        let mut numeric = (self.numeric)();
        if self.id == "grain_depth" && brush.contact.is_some_and(|c| c.dry.is_some()) {
            numeric = texture_strength_control();
        }
        if self.id == "spread" && brush.execution == BrushExecution::Smudge && matches!(brush.blender.kind, BlenderKind::Dither | BlenderKind::Scatter) {
            numeric.mapping = crate::NumericMapping::Log;
            numeric.log_offset = SMOOTHING_LOG_OFFSET;
        }
        numeric
    }
}

pub(super) fn texture_strength_control() -> NumericControl {
    NumericControl { max: 2., soft_max: 2., ..NumericControl::percent() }
}

fn default_primary(brush: &BrushSnapshot) -> &'static [&'static str] {
    if brush.contact.is_some_and(|c| c.dry.is_some()) {
        &["grain_depth", "material_hardness"]
    } else if brush.contact.is_some_and(|c| c.bristles.is_some()) {
        &["bristle_load", "bristle_scale"]
    } else {
        match brush.execution {
            BrushExecution::Wet => &["paint", "pull"],
            BrushExecution::Watercolor => &["water_load", "wet_edge"],
            BrushExecution::Smudge => &["pull", "spread"],
            BrushExecution::Liquify => &[],
            _ => &["hardness"],
        }
    }
}

const SMOOTHING_LOG_OFFSET: f64 = 0.00886269125382811;

fn blender_cell_size() -> NumericControl {
    let max = f64::from(layer_core::BLENDER_CELL_SIZE_LIMIT);
    NumericControl {
        kind: crate::NumericKind::Slider,
        mapping: crate::NumericMapping::Log,
        log_offset: (max - 1.) * SMOOTHING_LOG_OFFSET - 1.,
        ..NumericControl::whole_pixels(1., max)
    }
}

pub(crate) fn defined(id: &str) -> bool {
    DEFINITIONS.iter().any(|d| d.id == id)
}

pub(crate) fn ring_defaults(preset: u32, brush: &BrushSnapshot, controls: &[ToolSetting]) -> Vec<&'static str> {
    let shown = |id: &&str| controls.iter().any(|c| c.id == *id);
    if let Some(profile) = crate::tools::control_profile(preset) {
        profile.primary().filter(shown).collect()
    } else if brush.execution == BrushExecution::Liquify {
        ["momentum", "distortion"].into_iter().find(shown).into_iter().collect()
    } else {
        default_primary(brush).iter().copied().filter(shown).collect()
    }
}

const GROUP_ORDER: &[MessageId] = &[
    MessageId::TOOL_SETTING_GROUP_BLENDER,
    MessageId::TOOL_SETTING_GROUP_TIP,
    MessageId::TOOL_SETTING_GROUP_MATERIAL,
    MessageId::TOOL_SETTING_GROUP_TEXTURE,
    MessageId::TOOL_SETTING_GROUP_BRISTLES,
    MessageId::TOOL_SETTING_GROUP_MIXING,
    MessageId::TOOL_SETTING_GROUP_WATERCOLOR,
    MessageId::TOOL_SETTING_GROUP_BLEED,
    MessageId::TOOL_SETTING_GROUP_LIQUIFY,
    MessageId::TOOL_SETTING_GROUP_BLENDING,
];

pub(crate) fn group_order(id: &str) -> usize {
    if id.is_empty() { 0 } else {
        1 + GROUP_ORDER.iter().position(|g| g.key() == id).unwrap_or(GROUP_ORDER.len())
    }
}

pub(crate) fn mixes_color(b: &BrushSnapshot) -> bool {
    matches!(
        b.execution_class(),
        BrushExecution::Wet | BrushExecution::Smudge | BrushExecution::Watercolor
    )
}

pub(crate) fn color_mixing_visible(brush: &BrushSnapshot) -> bool {
    mixes_color(brush) && !(brush.execution_class() == BrushExecution::Smudge && brush.blender.kind == BlenderKind::Dither)
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
        id: "flow",
        label: MessageId::TOOL_SETTING_STRENGTH,
        group: None,
        numeric: NumericControl::percent,
        field: |b| (b.execution != BrushExecution::Liquify).then_some(&mut b.flow),
    },
    Definition {
        id: "strength",
        label: MessageId::TOOL_SETTING_STRENGTH,
        group: None,
        numeric: NumericControl::percent,
        field: |b| (b.execution == BrushExecution::Liquify).then_some(&mut b.deform.strength),
    },
    Definition {
        id: "opacity",
        label: MessageId::TOOL_SETTING_OPACITY,
        group: Some(MessageId::TOOL_SETTING_GROUP_BLENDING),
        numeric: NumericControl::percent,
        field: |b| (b.execution != BrushExecution::Liquify).then_some(&mut b.opacity),
    },
    Definition {
        id: "hardness",
        label: MessageId::TOOL_SETTING_HARDNESS,
        group: Some(MessageId::TOOL_SETTING_GROUP_TIP),
        numeric: NumericControl::percent,
        field: |b| (b.tip == BrushTip::AnalyticEllipse && !b.contact.is_some_and(|c|c.dry.is_some())).then_some(&mut b.hardness),
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
        field: |b| (!b.contact.is_some_and(|c| c.dry.is_some())).then_some(&mut b.spacing),
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
        field: |b| (!b.contact.is_some_and(|c| c.dry.is_some())).then_some(&mut b.shape.size_jitter),
    },
    Definition {
        id: "rotation_jitter",
        label: MessageId::TOOL_SETTING_ROTATION_JITTER,
        group: Some(MessageId::TOOL_SETTING_GROUP_TIP),
        numeric: NumericControl::percent,
        field: |b| (!b.contact.is_some_and(|c| c.dry.is_some())).then_some(&mut b.shape.rotation_jitter),
    },
    Definition {
        id: "material_hardness",
        label: MessageId::TOOL_SETTING_MATERIAL_HARDNESS,
        group: Some(MessageId::TOOL_SETTING_GROUP_MATERIAL),
        numeric: NumericControl::percent,
        field: |b| b.contact.as_mut()?.dry.as_mut().map(|d| &mut d.loading),
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
        id: "average_pickup",
        label: MessageId::TOOL_SETTING_AVERAGE_PICKUP,
        group: Some(MessageId::TOOL_SETTING_GROUP_MIXING),
        numeric: NumericControl::percent,
        field: |b| (b.execution_class() == BrushExecution::Wet).then_some(&mut b.wet_mix.average),
    },
    Definition {
        id: "persistence",
        label: MessageId::TOOL_SETTING_PERSISTENCE,
        group: Some(MessageId::TOOL_SETTING_GROUP_MIXING),
        numeric: four_times,
        field: |b| (b.execution_class() == BrushExecution::Wet).then_some(&mut b.wet_mix.stretch),
    },
    Definition {
        id: "pressure_refill",
        label: MessageId::TOOL_SETTING_PRESSURE_REFILL,
        group: Some(MessageId::TOOL_SETTING_GROUP_MIXING),
        numeric: NumericControl::percent,
        field: |b| (b.execution_class() == BrushExecution::Wet).then_some(&mut b.wet_mix.refill),
    },
    Definition {
        id: "spread",
        label: MessageId::TOOL_SETTING_SPREAD,
        group: Some(MessageId::TOOL_SETTING_GROUP_BLENDER),
        numeric: NumericControl::percent,
        field: |b| blender(b, |kind| !matches!(kind, BlenderKind::Knife | BlenderKind::Marbling)).then_some(&mut b.blender.reach),
    },
    Definition {
        id: "streaks",
        label: MessageId::TOOL_SETTING_STREAKS,
        group: Some(MessageId::TOOL_SETTING_GROUP_BLENDER),
        numeric: NumericControl::percent,
        field: |b| blender(b, |kind| matches!(kind, BlenderKind::Soft | BlenderKind::Bristle | BlenderKind::Knife)).then_some(&mut b.blender.character),
    },
    Definition {
        id: "sharpness",
        label: MessageId::TOOL_SETTING_SHARPNESS,
        group: Some(MessageId::TOOL_SETTING_GROUP_BLENDER),
        numeric: four_times,
        field: |b| blender(b, |kind| kind == BlenderKind::Sharpen).then_some(&mut b.blender.character),
    },
    Definition {
        id: "texture_kept",
        label: MessageId::TOOL_SETTING_TEXTURE_KEPT,
        group: Some(MessageId::TOOL_SETTING_GROUP_BLENDER),
        numeric: NumericControl::percent,
        field: |b| blender(b, |kind| kind == BlenderKind::TextureKeeping).then_some(&mut b.blender.character),
    },
    Definition {
        id: "blend_lightness",
        label: MessageId::TOOL_SETTING_BLEND_LIGHTNESS,
        group: Some(MessageId::TOOL_SETTING_GROUP_BLENDER),
        numeric: NumericControl::percent,
        field: |b| blender(b, |kind| kind == BlenderKind::Hue).then_some(&mut b.blender.character),
    },
    Definition {
        id: "grit",
        label: MessageId::TOOL_SETTING_GRIT,
        group: Some(MessageId::TOOL_SETTING_GROUP_BLENDER),
        numeric: NumericControl::percent,
        field: |b| blender(b, |kind| kind == BlenderKind::Scatter).then_some(&mut b.blender.character),
    },
    Definition {
        id: "stretch",
        label: MessageId::TOOL_SETTING_STRETCH,
        group: Some(MessageId::TOOL_SETTING_GROUP_BLENDER),
        numeric: four_times,
        field: |b| blender(b, |kind| matches!(kind, BlenderKind::Painterly | BlenderKind::Crystal)).then_some(&mut b.blender.character),
    },
    Definition {
        id: "swirl",
        label: MessageId::TOOL_SETTING_SWIRL,
        group: Some(MessageId::TOOL_SETTING_GROUP_BLENDER),
        numeric: four_times,
        field: |b| blender(b, |kind| kind == BlenderKind::Marbling).then_some(&mut b.blender.character),
    },
    Definition {
        id: "edge_keeping",
        label: MessageId::TOOL_SETTING_EDGE_KEEPING,
        group: Some(MessageId::TOOL_SETTING_GROUP_BLENDER),
        numeric: NumericControl::percent,
        field: |b| blender(b, |kind| kind == BlenderKind::Comb).then_some(&mut b.blender.character),
    },
    Definition {
        id: "detail_size",
        label: MessageId::TOOL_SETTING_DETAIL_SIZE,
        group: Some(MessageId::TOOL_SETTING_GROUP_BLENDER),
        numeric: NumericControl::percent,
        field: |b| blender(b, |kind| kind == BlenderKind::TextureKeeping).then_some(&mut b.blender.scale),
    },
    Definition {
        id: "cell_size",
        label: MessageId::TOOL_SETTING_CELL_SIZE,
        group: Some(MessageId::TOOL_SETTING_GROUP_BLENDER),
        numeric: NumericControl::percent,
        field: |b| blender(b, |kind| kind == BlenderKind::Crystal).then_some(&mut b.blender.scale),
    },
    Definition {
        id: "dither_cell_size",
        label: MessageId::TOOL_SETTING_CELL_SIZE,
        group: Some(MessageId::TOOL_SETTING_GROUP_BLENDER),
        numeric: blender_cell_size,
        field: |b| blender(b, |kind| kind == BlenderKind::Dither).then_some(&mut b.blender.cell_size),
    },
    Definition {
        id: "scatter_cell_size",
        label: MessageId::TOOL_SETTING_CELL_SIZE,
        group: Some(MessageId::TOOL_SETTING_GROUP_BLENDER),
        numeric: blender_cell_size,
        field: |b| blender(b, |kind| kind == BlenderKind::Scatter).then_some(&mut b.blender.cell_size),
    },
    Definition {
        id: "swirl_size",
        label: MessageId::TOOL_SETTING_SWIRL_SIZE,
        group: Some(MessageId::TOOL_SETTING_GROUP_BLENDER),
        numeric: NumericControl::percent,
        field: |b| blender(b, |kind| kind == BlenderKind::Marbling).then_some(&mut b.blender.scale),
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
        id: "distortion",
        label: MessageId::TOOL_SETTING_DISTORTION,
        group: Some(MessageId::TOOL_SETTING_GROUP_LIQUIFY),
        numeric: NumericControl::percent,
        field: |b| (b.execution == BrushExecution::Liquify && b.deform.mode != LiquifyMode::Reconstruct).then_some(&mut b.deform.distortion),
    },
    Definition {
        id: "momentum",
        label: MessageId::TOOL_SETTING_MOMENTUM,
        group: Some(MessageId::TOOL_SETTING_GROUP_LIQUIFY),
        numeric: NumericControl::percent,
        field: |b| (b.execution == BrushExecution::Liquify && b.deform.mode != LiquifyMode::Reconstruct).then_some(&mut b.deform.momentum),
    },
    Definition {
        id: "smoothing_strength",
        label: MessageId::TOOL_SETTING_SMOOTHING,
        group: None,
        numeric: || NumericControl {
            mapping: crate::NumericMapping::Log,
            log_offset: SMOOTHING_LOG_OFFSET,
            step: 0.01, resolution: 0.01, digits: 0,
            slider_snap: crate::NumericSliderSnap::Resolution,
            ..NumericControl::percent().unit("")
        },
        field: |b| Some(&mut b.stabilization.strength),
    },
];

fn blender(b: &BrushSnapshot, kind: fn(BlenderKind) -> bool) -> bool {
    b.execution == BrushExecution::Smudge && kind(b.blender.kind)
}

fn four_times() -> NumericControl {
    NumericControl { max: 4., soft_max: 4., ..NumericControl::percent() }
}

pub fn shapes_dab(id: &str) -> bool {
    matches!(id, "size" | "flow" | "strength" | "opacity" | "hardness" | "angle" | "grain_depth" | "material_hardness" | "bristle_scale" | "bristle_load")
}

pub(crate) fn controls(preset: u32, brush: &BrushSnapshot, localizer: &Localizer) -> Vec<ToolSetting> {
    let profile = crate::tools::control_profile(preset);
    let primary = default_primary(brush);
    let mut brush = brush.clone();
    let mut controls = DEFINITIONS
        .iter()
        .enumerate()
        .filter_map(|(index, d)| {
            if !d.applicable(&brush) { return None; }
            let placement = profile.and_then(|profile| profile.placement(d.id));
            let rank = match d.id {
                "size" => Some(0),
                "flow" | "strength" => Some(1),
                id => if profile.is_some() {
                    placement.and_then(|(i, place)| matches!(place, profiles::Placement::Primary).then_some(i + 2))
                } else { primary.iter().position(|&p| p == id).map(|i| i + 2) },
            };
            let group = if rank.is_some() { None } else { match placement {
                Some((_, profiles::Placement::Section(group))) => Some(group),
                _ => d.group,
            } };
            let order = rank.map_or((1, group_order(group.map_or("", MessageId::key)), placement.map_or(index, |(i, _)| i)),
                |rank| (0, rank, 0));
            Some((order, ToolSetting { preview: false,
                id: d.id,
                label: localizer.text(d.label),
                label_id: d.label,
                group_id: group.map_or("", MessageId::key),
                group: group.map(|id| localizer.text(id)).unwrap_or_else(|| Arc::from("")),
                numeric: d.numeric(&brush),
                value: *(d.field)(&mut brush)?,
            }))
        })
        .collect::<Vec<_>>();
    controls.sort_unstable_by_key(|(order, _)| *order);
    controls.into_iter().map(|(_, field)| field).collect()
}

pub(crate) fn edit(brush: &BrushSnapshot, id: &str, value: f32, localizer: &Localizer) -> Result<BrushSnapshot, String> {
    edit_value(brush, id, value).map_err(|reason| reason.message(localizer))
}

pub(crate) fn edit_value(brush: &BrushSnapshot, id: &str, value: f32) -> Result<BrushSnapshot, crate::WorkspaceValidationError> {
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
    d.numeric(brush).validate(value, d.label)?;
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
    fn compact_action_transport_preserves_exclusive_groups_and_command_rows() {
        let catalog=crate::ui_catalog();
        for group in &catalog.tool_action_groups {
            assert!(!group.label.is_empty());
            for &command in group.commands {
                let action=ToolSettingAction { command, checkable:true };
                let value=serde_json::to_value(action).unwrap();
                assert_eq!(value["group"],group.id);
                assert_eq!(value["segmented"],group.segmented);
            }
        }
        for row in &catalog.tool_action_rows {
            for &command in row.commands {
                let value=serde_json::to_value(ToolSettingAction { command, checkable:false }).unwrap();
                assert_eq!(value["row"],row.id);
                assert_eq!(value["icon_only"],row.icon_only);
            }
        }
        let value=serde_json::to_value(ToolSettingAction { command:crate::CommandId::SelectionAntialias,checkable:true }).unwrap();
        assert!(value.get("group").is_none());
        assert!(value.get("row").is_none());
        let metrics=catalog.panel_metrics;
        assert_eq!(metrics.control_radius,metrics.control_height/2.);
        assert!(metrics.compound_spacing<metrics.control_spacing && metrics.control_spacing<metrics.group_spacing);
        assert!(2.*metrics.field_padding<metrics.control_height);
    }

    #[test]
    fn option_control_localization_preserves_ids_values_and_numeric_rules() {
        let project = |localizer: &Localizer| {
            let mut selection = crate::SelectionOptions { constraint: crate::SelectionConstraint::Ratio, ..Default::default() };
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
                control.numeric.validate(control.value, control.label.as_ref()).unwrap();
            }
        }
    }

    #[test]
    fn every_visible_control_accepts_its_default_and_endpoints() {
        for choice in crate::brush_catalog() {
            let brush = default_brush(crate::preset(choice.id).unwrap());
            for control in controls(choice.id, &brush, &Localizer::shared(UiLanguage::English)) {
                assert_eq!(
                    edit(&brush, control.id, control.value, &Localizer::shared(UiLanguage::English)).unwrap(),
                    brush,
                    "{}: {}",
                    choice.label,
                    control.id
                );
                for value in [control.numeric.min as f32, control.numeric.max as f32] {
                    edit(&brush, control.id, value, &Localizer::shared(UiLanguage::English)).unwrap();
                }
            }
        }
    }

    #[test]
    fn liquify_shows_distortion_and_momentum_for_deformation() {
        let ids = |preset| -> Vec<_> { controls(preset as u32, &default_brush(preset), &Localizer::shared(UiLanguage::English)).iter().map(|c| c.id).collect() };
        for (preset, distortion, momentum) in [
            (DefaultBrushPreset::LiquifyPush, true, true),
            (DefaultBrushPreset::LiquifyTwirl, true, true),
            (DefaultBrushPreset::LiquifyPinch, true, true),
            (DefaultBrushPreset::LiquifyCrystals, true, true),
            (DefaultBrushPreset::LiquifyReconstruct, false, false),
        ] {
            let ids = ids(preset);
            assert!(ids.contains(&"strength"), "{preset:?}");
            assert_eq!(ids.contains(&"distortion"), distortion, "{preset:?}");
            assert_eq!(ids.contains(&"momentum"), momentum, "{preset:?}");
        }
        let crystals = default_brush(DefaultBrushPreset::LiquifyCrystals);
        assert_eq!(edit(&crystals, "distortion", 0.9, &Localizer::shared(UiLanguage::English)).unwrap().deform.distortion, 0.9);
        assert_eq!(edit(&crystals, "momentum", 0.5, &Localizer::shared(UiLanguage::English)).unwrap().deform.momentum, 0.5);
    }

    #[test]
    fn blenders_show_the_controls_their_kind_uses() {
        let english = Localizer::shared(UiLanguage::English);
        let ids = |preset| -> Vec<_> { controls(preset as u32, &default_brush(preset), &english).iter().map(|c| c.id).collect() };
        for (preset, expected, absent) in [
            (DefaultBrushPreset::Smudge, &["pull", "spread", "streaks"][..], &["swirl", "average_pickup"][..]),
            (DefaultBrushPreset::CrystalBlender, &["spread", "stretch", "cell_size"], &["streaks", "swirl_size"]),
            (DefaultBrushPreset::Marbling, &["swirl", "swirl_size"], &["spread", "cell_size"]),
            (DefaultBrushPreset::CombBlender, &["spread", "edge_keeping"], &["streaks"]),
            (DefaultBrushPreset::TextureBlender, &["texture_kept", "detail_size"], &["stretch"]),
            (DefaultBrushPreset::ColorMixer, &["pull", "average_pickup", "persistence", "pressure_refill"], &["spread"]),
            (DefaultBrushPreset::WetBlender, &["average_pickup", "pressure_refill"], &["persistence", "spread"]),
            (DefaultBrushPreset::GPen, &[], &["spread", "average_pickup", "streaks"]),
        ] {
            let ids = ids(preset);
            assert!(expected.iter().all(|id| ids.contains(id)), "{preset:?} shows {expected:?}: {ids:?}");
            assert!(!absent.iter().any(|id| ids.contains(id)), "{preset:?} hides {absent:?}: {ids:?}");
        }
        let crystal = edit(&default_brush(DefaultBrushPreset::CrystalBlender), "cell_size", 0.3, &english).unwrap();
        assert_eq!(crystal.blender.scale, 0.3);
        assert!(edit(&crystal, "swirl", 1., &english).is_err());
        let wet = edit(&default_brush(DefaultBrushPreset::WetBlender), "average_pickup", 0.5, &english).unwrap();
        assert_eq!(edit(&wet, "persistence", 2., &english).unwrap().wet_mix.stretch, 2.);
    }

    #[test]
    fn blender_profiles_reference_unique_bound_settings() {
        for choice in crate::brush_catalog() {
            let profile = crate::tools::control_profile(choice.id);
            assert_eq!(profile.is_some(), crate::tools::tool(choice.id) == crate::Tool::Blend, "{}", choice.label);
            if let Some(profile) = profile {
                let brush = default_brush(crate::preset(choice.id).unwrap());
                let mut ids = std::collections::HashSet::new();
                for control in profile.0 {
                    assert!(ids.insert(control.setting), "{} repeats {}", choice.label, control.setting);
                    assert!(value(&brush, control.setting).is_some(), "{} has no {} binding", choice.label, control.setting);
                }
                assert!(profile.primary().count() <= crate::MAX_RING_SETTINGS);
            }
        }
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
            ("smoothing_strength", f32::NAN),
            ("smoothing_strength", -0.01),
            ("smoothing_strength", 1.01),
            ("flow", f32::NAN),
            ("size", 3000.0),
        ] {
            assert!(edit(&brush, id, value, &Localizer::shared(UiLanguage::English)).is_err());
        }
    }

    #[test]
    fn bristle_controls_preserve_relative_scale_when_resizing_and_round_trip() {
        let brush = default_brush(DefaultBrushPreset::BristlePaintbrush);
        let scaled = edit(&brush, "bristle_scale", 2., &Localizer::shared(UiLanguage::English)).unwrap();
        let resized = edit(&scaled, "size", 1000., &Localizer::shared(UiLanguage::English)).unwrap();
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
            controls(DefaultBrushPreset::Paintbrush as u32, &default_brush(DefaultBrushPreset::Paintbrush), &Localizer::shared(UiLanguage::English))
                .iter()
                .all(|c| c.id != "bristle_scale")
        );
    }
}
