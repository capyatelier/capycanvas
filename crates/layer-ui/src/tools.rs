//! Tool/group identity and subtool memory. Hosts only render the resolved view.
use crate::*;
use layer_core::{BrushSnapshot, DefaultBrushPreset};
use std::{collections::BTreeMap, sync::Arc};
use crate::localization::{Localizer, MessageId, UiLanguage};

crate::variants! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum Tool {
        #[default]
        Pen,
        Pencil,
        Brush,
        Eraser,
        Airbrush,
        Blend,
        Liquify,
        Clone,
        Heal,
        SpotHeal,
    }
}
impl Tool {
    pub fn command(self) -> CommandId {
        match self {
            Self::Pen => CommandId::Pen,
            Self::Pencil => CommandId::Pencil,
            Self::Brush => CommandId::Brush,
            Self::Eraser => CommandId::Eraser,
            Self::Airbrush => CommandId::Airbrush,
            Self::Blend => CommandId::Blend,
            Self::Liquify => CommandId::Liquify,
            Self::Clone => CommandId::Clone,
            Self::Heal => CommandId::Heal,
            Self::SpotHeal => CommandId::SpotHeal,
        }
    }
    pub fn default_preset(self) -> u32 {
        PRESETS.iter().find(|p| tool(p.0 as u32) == self).unwrap().0 as u32
    }
}
impl CommandId {
    pub fn paint_tool(self) -> Option<Tool> {
        Tool::ALL.into_iter().find(|tool| tool.command() == self)
    }
}

/// Repeated family keys cycle tools; direct command bindings still select one
/// tool. This keeps intentional cycling separate from shortcut conflicts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolFamily {
    Ink,
    Paint,
    Blend,
    Retouch,
}
impl ToolFamily {
    pub const ALL: [Self; 4] = [Self::Ink, Self::Paint, Self::Blend, Self::Retouch];
    pub fn commands(self) -> &'static [CommandId] {
        match self {
            Self::Ink => &[CommandId::Pen, CommandId::Pencil],
            Self::Paint => &[CommandId::Brush, CommandId::Airbrush],
            Self::Blend => &[CommandId::Blend, CommandId::Liquify],
            Self::Retouch => &[CommandId::Clone, CommandId::Heal, CommandId::SpotHeal],
        }
    }
    pub fn shortcut_id(self) -> &'static str {
        match self {
            Self::Ink => "tools.ink",
            Self::Paint => "tools.paint",
            Self::Blend => "tools.blend",
            Self::Retouch => "tools.retouch",
        }
    }
    pub fn message_id(self) -> MessageId {
        match self {
            Self::Ink => MessageId::TOOL_FAMILY_INK,
            Self::Paint => MessageId::TOOL_FAMILY_PAINT,
            Self::Blend => MessageId::TOOL_FAMILY_BLEND,
            Self::Retouch => MessageId::TOOL_FAMILY_RETOUCH,
        }
    }
    pub fn localized_label(self, localizer: &Localizer) -> Arc<str> { localizer.text(self.message_id()) }
    pub fn label(self) -> Arc<str> { self.localized_label(&Localizer::shared(UiLanguage::English)) }
    pub fn for_command(command: CommandId) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|f| f.commands().contains(&command))
    }
}

macro_rules! tool_groups {
    ($($group:ident: $tool:ident, $label:ident, $icon:literal;)+) => {
        crate::variants! {
            #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
            #[serde(rename_all = "snake_case")]
            pub enum ToolGroup { $($group),+ }
        }
        impl ToolGroup {
            pub fn tool(self) -> Tool {
                match self { $(Self::$group => Tool::$tool),+ }
            }
            pub fn message_id(self) -> MessageId { match self { $(Self::$group => MessageId::$label),+ } }
            pub fn localized_label(self, localizer: &Localizer) -> Arc<str> { localizer.text(self.message_id()) }
            pub fn label(self) -> Arc<str> { self.localized_label(&Localizer::shared(UiLanguage::English)) }
            /// Identity of the medium, independent of its parent drawing engine.
            pub fn icon(self) -> &'static str {
                match self { $(Self::$group => $icon),+ }
            }
        }
    };
}

tool_groups! {
    Pen: Pen, TOOL_GROUP_PEN, "pen";
    Pencil: Pencil, TOOL_GROUP_PENCIL, "pencil";
    Pastel: Pencil, TOOL_GROUP_PASTEL, "pastel";
    Paint: Brush, TOOL_GROUP_PAINT, "paint";
    Watercolor: Brush, TOOL_GROUP_WATERCOLOR, "watercolor";
    Oil: Brush, TOOL_GROUP_OIL, "oil-paint";
    Eraser: Eraser, TOOL_GROUP_ERASER, "eraser";
    Airbrush: Airbrush, TOOL_GROUP_AIRBRUSH, "airbrush";
    Blend: Blend, TOOL_GROUP_BLEND, "blend";
    WetPaint: Blend, TOOL_GROUP_WET_PAINT, "water";
    Effects: Blend, TOOL_GROUP_EFFECTS, "star";
    Liquify: Liquify, TOOL_GROUP_LIQUIFY, "liquify";
    Retouch: Clone, TOOL_GROUP_RETOUCH, "heal";
}
impl ToolGroup {
    fn default_preset(self) -> u32 {
        PRESETS.iter().find(|p| p.2 == self).unwrap().0 as u32
    }
    /// Whether this group shows presets of `tool`.
    pub(crate) fn holds(self, tool: Tool) -> bool {
        group_presets(self).any(|id| self::tool(id) == tool)
    }
}

// Each preset has exactly one owning tool/group. Stable numeric preset IDs are
// also the keys of the GPU preview cache and saved brush assets.
const PRESETS: &[(DefaultBrushPreset, MessageId, ToolGroup)] = &[
    (DefaultBrushPreset::GPen, MessageId::BRUSH_PRESET_GPEN, ToolGroup::Pen),
    (DefaultBrushPreset::Monoline, MessageId::BRUSH_PRESET_MONOLINE, ToolGroup::Pen),
    (DefaultBrushPreset::RoughGPen, MessageId::BRUSH_PRESET_ROUGH_GPEN, ToolGroup::Pen),
    (DefaultBrushPreset::BlottyInk, MessageId::BRUSH_PRESET_BLOTTY_INK, ToolGroup::Pen),
    (
        DefaultBrushPreset::CalligraphyPen,
        MessageId::BRUSH_PRESET_CALLIGRAPHY_PEN,
        ToolGroup::Pen,
    ),
    (
        DefaultBrushPreset::AntiquePen,
        MessageId::BRUSH_PRESET_ANTIQUE_PEN,
        ToolGroup::Pen,
    ),
    (
        DefaultBrushPreset::RealisticPen,
        MessageId::BRUSH_PRESET_REALISTIC_PEN,
        ToolGroup::Pen,
    ),
    (DefaultBrushPreset::WetInk, MessageId::BRUSH_PRESET_WET_INK, ToolGroup::Pen),
    (
        DefaultBrushPreset::BrushedInk,
        MessageId::BRUSH_PRESET_BRUSHED_INK,
        ToolGroup::Pen,
    ),
    (DefaultBrushPreset::Marker, MessageId::BRUSH_PRESET_MARKER, ToolGroup::Pen),
    (DefaultBrushPreset::Pencil, MessageId::BRUSH_PRESET_PENCIL, ToolGroup::Pencil),
    (
        DefaultBrushPreset::PointyPencil,
        MessageId::BRUSH_PRESET_POINTY_PENCIL,
        ToolGroup::Pencil,
    ),
    (
        DefaultBrushPreset::ShadingPencil,
        MessageId::BRUSH_PRESET_SHADING_PENCIL,
        ToolGroup::Pencil,
    ),
    (DefaultBrushPreset::Charcoal, MessageId::BRUSH_PRESET_CHARCOAL, ToolGroup::Pastel),
    (DefaultBrushPreset::Crayon, MessageId::BRUSH_PRESET_CRAYON, ToolGroup::Pastel),
    (DefaultBrushPreset::Chalk, MessageId::BRUSH_PRESET_CHALK, ToolGroup::Pastel),
    (
        DefaultBrushPreset::PastelBlock,
        MessageId::BRUSH_PRESET_PASTEL_BLOCK,
        ToolGroup::Pastel,
    ),
    (
        DefaultBrushPreset::Paintbrush,
        MessageId::BRUSH_PRESET_PAINTBRUSH,
        ToolGroup::Paint,
    ),
    (DefaultBrushPreset::BristlePaintbrush, MessageId::BRUSH_PRESET_BRISTLE_PAINTBRUSH, ToolGroup::Paint),
    (
        DefaultBrushPreset::TexturedFlat,
        MessageId::BRUSH_PRESET_TEXTURED_FLAT,
        ToolGroup::Paint,
    ),
    (
        DefaultBrushPreset::DryScumble,
        MessageId::BRUSH_PRESET_DRY_SCUMBLE,
        ToolGroup::Paint,
    ),
    (
        DefaultBrushPreset::TransparentGlaze,
        MessageId::BRUSH_PRESET_TRANSPARENT_GLAZE,
        ToolGroup::Paint,
    ),
    (
        DefaultBrushPreset::OpaqueGouache,
        MessageId::BRUSH_PRESET_OPAQUE_GOUACHE,
        ToolGroup::Paint,
    ),
    (
        DefaultBrushPreset::MultiplyGlaze,
        MessageId::BRUSH_PRESET_MULTIPLY_GLAZE,
        ToolGroup::Paint,
    ),
    (
        DefaultBrushPreset::WatercolorWash,
        MessageId::BRUSH_PRESET_WATERCOLOR_WASH,
        ToolGroup::Watercolor,
    ),
    (
        DefaultBrushPreset::WetWatercolor,
        MessageId::BRUSH_PRESET_WET_WATERCOLOR,
        ToolGroup::Watercolor,
    ),
    (DefaultBrushPreset::LoadedOil, MessageId::BRUSH_PRESET_LOADED_OIL, ToolGroup::Oil),
    (
        DefaultBrushPreset::PaletteKnife,
        MessageId::BRUSH_PRESET_PALETTE_KNIFE,
        ToolGroup::Oil,
    ),
    (DefaultBrushPreset::WetRound, MessageId::BRUSH_PRESET_WET_ROUND, ToolGroup::Oil),
    (DefaultBrushPreset::MixingBrush, MessageId::BRUSH_PRESET_MIXING_BRUSH, ToolGroup::Oil),
    (DefaultBrushPreset::Eraser, MessageId::BRUSH_PRESET_ERASER, ToolGroup::Eraser),
    (
        DefaultBrushPreset::Airbrush,
        MessageId::BRUSH_PRESET_AIRBRUSH,
        ToolGroup::Airbrush,
    ),
    (DefaultBrushPreset::Spray, MessageId::BRUSH_PRESET_SPRAY, ToolGroup::Airbrush),
    (
        DefaultBrushPreset::DualTexture,
        MessageId::BRUSH_PRESET_DUAL_TEXTURE,
        ToolGroup::Paint,
    ),
    (DefaultBrushPreset::Smudge, MessageId::BRUSH_PRESET_SMUDGE, ToolGroup::Blend),
    (DefaultBrushPreset::SoftBlender, MessageId::BRUSH_PRESET_SOFT_BLENDER, ToolGroup::Blend),
    (DefaultBrushPreset::Blur, MessageId::BRUSH_PRESET_BLUR, ToolGroup::Blend),
    (DefaultBrushPreset::BlendingStump, MessageId::BRUSH_PRESET_BLENDING_STUMP, ToolGroup::Blend),
    (DefaultBrushPreset::Tissue, MessageId::BRUSH_PRESET_TISSUE, ToolGroup::Blend),
    (DefaultBrushPreset::GrainyBlender, MessageId::BRUSH_PRESET_GRAINY_BLENDER, ToolGroup::Blend),
    (DefaultBrushPreset::TextureBlender, MessageId::BRUSH_PRESET_TEXTURE_BLENDER, ToolGroup::Blend),
    (DefaultBrushPreset::HueBlender, MessageId::BRUSH_PRESET_HUE_BLENDER, ToolGroup::Blend),
    (DefaultBrushPreset::NaturalBlender, MessageId::BRUSH_PRESET_NATURAL_BLENDER, ToolGroup::WetPaint),
    (DefaultBrushPreset::WetBlender, MessageId::BRUSH_PRESET_WET_BLENDER, ToolGroup::WetPaint),
    (DefaultBrushPreset::ColorMixer, MessageId::BRUSH_PRESET_COLOR_MIXER, ToolGroup::WetPaint),
    (DefaultBrushPreset::BlendingKnife, MessageId::BRUSH_PRESET_BLENDING_KNIFE, ToolGroup::WetPaint),
    (DefaultBrushPreset::BristleBlender, MessageId::BRUSH_PRESET_BRISTLE_BLENDER, ToolGroup::WetPaint),
    (DefaultBrushPreset::PainterlyBlender, MessageId::BRUSH_PRESET_PAINTERLY_BLENDER, ToolGroup::Effects),
    (DefaultBrushPreset::CombBlender, MessageId::BRUSH_PRESET_COMB_BLENDER, ToolGroup::Effects),
    (DefaultBrushPreset::ScatterBlender, MessageId::BRUSH_PRESET_SCATTER_BLENDER, ToolGroup::Effects),
    (DefaultBrushPreset::Sharpen, MessageId::BRUSH_PRESET_SHARPEN, ToolGroup::Effects),
    (DefaultBrushPreset::Marbling, MessageId::BRUSH_PRESET_MARBLING, ToolGroup::Effects),
    (DefaultBrushPreset::CrystalBlender, MessageId::BRUSH_PRESET_CRYSTAL_BLENDER, ToolGroup::Effects),
    (DefaultBrushPreset::DitherBlender, MessageId::BRUSH_PRESET_DITHER_BLENDER, ToolGroup::Effects),

    (
        DefaultBrushPreset::LiquifyPush,
        MessageId::BRUSH_PRESET_LIQUIFY_PUSH,
        ToolGroup::Liquify,
    ),
    (
        DefaultBrushPreset::LiquifyTwirl,
        MessageId::BRUSH_PRESET_LIQUIFY_TWIRL,
        ToolGroup::Liquify,
    ),
    (
        DefaultBrushPreset::LiquifyTwirlClockwise,
        MessageId::BRUSH_PRESET_LIQUIFY_TWIRL_CLOCKWISE,
        ToolGroup::Liquify,
    ),
    (
        DefaultBrushPreset::LiquifyPinch,
        MessageId::BRUSH_PRESET_LIQUIFY_PINCH,
        ToolGroup::Liquify,
    ),
    (
        DefaultBrushPreset::LiquifyExpand,
        MessageId::BRUSH_PRESET_LIQUIFY_EXPAND,
        ToolGroup::Liquify,
    ),
    (
        DefaultBrushPreset::LiquifyCrystals,
        MessageId::BRUSH_PRESET_LIQUIFY_CRYSTALS,
        ToolGroup::Liquify,
    ),
    (DefaultBrushPreset::LiquifyEdge, MessageId::BRUSH_PRESET_LIQUIFY_EDGE, ToolGroup::Liquify),
    (DefaultBrushPreset::LiquifyReconstruct, MessageId::BRUSH_PRESET_LIQUIFY_RECONSTRUCT, ToolGroup::Liquify),
    (DefaultBrushPreset::CloneStamp, MessageId::BRUSH_PRESET_CLONE_STAMP, ToolGroup::Retouch),
    (DefaultBrushPreset::HealingBrush, MessageId::BRUSH_PRESET_HEALING_BRUSH, ToolGroup::Retouch),
    (DefaultBrushPreset::SpotHealingBrush, MessageId::BRUSH_PRESET_SPOT_HEALING_BRUSH, ToolGroup::Retouch),
];

pub(crate) fn control_profile(id: u32) -> Option<&'static crate::tool_settings::profiles::ControlProfile> {
    use crate::tool_settings::profiles::*;
    use DefaultBrushPreset::*;
    Some(match preset(id).ok()? {
        Smudge | SoftBlender | NaturalBlender | BlendingStump | Tissue => &SOFT,
        Blur => &BLUR,
        GrainyBlender => &GRAINY,
        BristleBlender => &BRISTLE,
        BlendingKnife => &KNIFE,
        TextureBlender => &TEXTURE,
        HueBlender => &HUE,
        WetBlender => &WET,
        ColorMixer => &COLOR_MIXER,
        PainterlyBlender => &PAINTERLY,
        CombBlender => &COMB,
        ScatterBlender => &SCATTER,
        Sharpen => &SHARPEN,
        Marbling => &MARBLING,
        CrystalBlender => &CRYSTAL,
        DitherBlender => &DITHER,
        _ => return None,
    })
}

/// The retouching tools share the Retouch set; each of its presets keeps its
/// own tool.
const RETOUCHING: [(DefaultBrushPreset, Tool); 3] = [
    (DefaultBrushPreset::CloneStamp, Tool::Clone),
    (DefaultBrushPreset::HealingBrush, Tool::Heal),
    (DefaultBrushPreset::SpotHealingBrush, Tool::SpotHeal),
];

fn brush_choice(preset: DefaultBrushPreset, label: MessageId, group: ToolGroup, localizer: &Localizer) -> BrushChoice {
    BrushChoice { id: preset as u32, label: localizer.text(label), category: group.localized_label(localizer), group, icon: icon(preset as u32) }
}
pub fn brush_ids() -> impl Iterator<Item = u32> {
    PRESETS.iter().map(|preset| preset.0 as u32)
}
pub fn brush_catalog() -> impl Iterator<Item = BrushChoice> {
    let english = Localizer::shared(UiLanguage::English);
    PRESETS.iter().map(move |&(preset, label, group)| brush_choice(preset, label, group, &english))
}
pub fn brush_catalog_localized(localizer: &Localizer) -> impl Iterator<Item = BrushChoice> + '_ {
    PRESETS.iter().map(move |&(preset, label, group)| brush_choice(preset, label, group, localizer))
}
pub fn brush_label_localized(id: u32, localizer: &Localizer) -> Option<Arc<str>> {
    PRESETS.iter().find(|preset| preset.0 as u32 == id).map(|preset| localizer.text(preset.1))
}
fn brush_category(group: ToolGroup, localizer: &Localizer) -> BrushCategory {
    BrushCategory {
        id: group, label: group.localized_label(localizer), icon: group.icon(),
        brushes: PRESETS.iter().filter(|preset| preset.2 == group)
            .map(|&(preset, label, group)| brush_choice(preset, label, group, localizer)).collect(),
    }
}
pub fn brush_categories() -> impl Iterator<Item = BrushCategory> {
    let english = Localizer::shared(UiLanguage::English);
    ToolGroup::ALL.into_iter().map(move |group| brush_category(group, &english))
}
pub fn brush_categories_localized(localizer: &Localizer) -> impl Iterator<Item = BrushCategory> + '_ {
    ToolGroup::ALL.into_iter().map(move |group| brush_category(group, localizer))
}
pub fn preset(id: u32) -> Result<DefaultBrushPreset, String> {
    PRESETS
        .iter()
        .find(|p| p.0 as u32 == id)
        .map(|p| p.0)
        .ok_or_else(|| "Unknown brush".into())
}
pub(crate) fn group_presets(group: ToolGroup) -> impl Iterator<Item = u32> {
    PRESETS.iter().filter(move |preset| preset.2 == group).map(|preset| preset.0 as u32)
}
pub(crate) fn group(id: u32) -> ToolGroup {
    PRESETS
        .iter()
        .find(|p| p.0 as u32 == id)
        .expect("validated brush")
        .2
}
/// The tool a preset selects.
pub(crate) fn tool(id: u32) -> Tool {
    RETOUCHING.iter().find(|r| r.0 as u32 == id).map_or_else(|| group(id).tool(), |r| r.1)
}
/// A preset's icon: its medium for drawing tools and its tool for sculpting,
/// whose sets mix kinds of the same tool.
pub(crate) fn icon(id: u32) -> &'static str {
    let tool = tool(id);
    if is_sculpt(tool) { tool.command().icon().expect("sculpting tools have icons") } else { group(id).icon() }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolSetItem {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<Arc<str>>,
    pub label: Arc<str>,
    pub icon: &'static str,
    pub action: UiAction,
    pub selected: bool,
    /// Shared cached stroke preview, absent for non-painting tools.
    pub preview: Option<u32>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ToolSetView {
    pub groups: Vec<ToolSetItem>,
    pub subtools: Vec<ToolSetItem>,
}

/// Hosts render these projections without deciding which tools belong together.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ToolPanels {
    pub brush_sets: ToolSetView,
    pub sculpt_sets: ToolSetView,
    pub tools: ToolSetView,
}
impl crate::UiState {
    pub fn tool_panel(&self, panel: crate::Panel) -> &ToolSetView {
        match panel {
            crate::Panel::BrushSets => &self.tool_panels.brush_sets,
            crate::Panel::SculptSets => &self.tool_panels.sculpt_sets,
            crate::Panel::Tools => &self.tool_panels.tools,
            _ => &self.tool_set,
        }
    }
}
pub(crate) fn is_drawing(tool: Tool) -> bool {
    !matches!(tool, Tool::Eraser | Tool::Blend | Tool::Liquify) && !is_retouching(tool)
}

pub(crate) fn is_sculpt(tool: Tool) -> bool {
    matches!(tool, Tool::Blend | Tool::Liquify) || is_retouching(tool)
}

/// Retouching tools paint with pixels from the image rather than a color.
pub(crate) fn is_retouching(tool: Tool) -> bool {
    matches!(tool, Tool::Clone | Tool::Heal | Tool::SpotHeal)
}

pub(crate) fn view(brush: &BrushState, canvas_tool: LayerCanvasTool, localizer: &Localizer) -> ToolSetView {
    if let LayerCanvasTool::Figure { shape, paint } = canvas_tool {
        return ToolSetView { groups: Vec::new(), subtools: crate::session::figures::modes(shape, paint, localizer) };
    }
    if let LayerCanvasTool::Region { fill, source } = canvas_tool {
        return ToolSetView {
            groups: Vec::new(),
            subtools: [
                (CommandId::SelectionVisible, RegionSource::Visible),
                (CommandId::SelectionEditing, RegionSource::Editing),
                (CommandId::SelectionReference, RegionSource::Reference),
            ]
            .into_iter()
            .map(|(command, item_source)| ToolSetItem { enabled: true, disabled_reason: None,
                label: command.localized_label(localizer),
                icon: command.icon().unwrap(),
                action: UiAction::Layer {
                    action: LayerAction::Tool {
                        tool: LayerCanvasTool::Region { fill, source: item_source },
                    },
                },
                selected: source == item_source,
                preview: None,
            })
            .collect(),
        };
    }
    if canvas_tool.picks_color() {
        return ToolSetView::default();
    }
    if canvas_tool != LayerCanvasTool::Paint {
        let (label, icon) = match canvas_tool {
            LayerCanvasTool::Hand => (MessageId::TOOL_MODE_HAND, "hand"),
            LayerCanvasTool::Zoom => (MessageId::COMMAND_ZOOM, "search"),
            LayerCanvasTool::RotateView => (MessageId::COMMAND_ROTATE_VIEW, "rotate-right"),
            LayerCanvasTool::Crop => (MessageId::TOOL_MODE_CROP, "crop"),
            _ => return ToolSetView::default(),
        };
        let item = ToolSetItem { enabled: true, disabled_reason: None,
            label: localizer.text(label),
            icon,
            selected: true,
            preview: None,
            action: UiAction::Layer {
                action: LayerAction::Tool { tool: canvas_tool },
            },
        };
        return ToolSetView {
            groups: vec![item.clone()],
            subtools: vec![item],
        };
    }
    let active = group(brush.preset);
    ToolSetView {
        groups: ToolGroup::ALL
            .into_iter()
            .filter(|g| g.holds(brush.tool))
            .map(|g| ToolSetItem { enabled: true, disabled_reason: None,
                label: g.localized_label(localizer),
                icon: g.icon(),
                action: UiAction::SelectToolGroup { group: g },
                selected: g == active,
                preview: None,
            })
            .collect(),
        subtools: PRESETS
            .iter()
            .filter(|p| p.2 == active)
            .map(|&(preset, label, _)| ToolSetItem { enabled: true, disabled_reason: None,
                label: localizer.text(label),
                icon: icon(preset as u32),
                action: UiAction::SelectBrush { id: preset as u32 },
                selected: preset as u32 == brush.preset,
                preview: Some(preset as u32),
            })
            .collect(),
    }
}

/// Latest semantic overrides. Built-in defaults are resolved by this installation;
/// merely loading or remembering a brush never rewrites an explicit override.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolMemory {
    tools: BTreeMap<Tool, u32>,
    groups: BTreeMap<ToolGroup, u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    drawing: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sculpt: Option<u32>,
    pub overrides: BTreeMap<u32, BTreeMap<String, f32>>,
}
impl ToolMemory {
    pub(crate) fn command_preset(&self, command: CommandId) -> Option<u32> {
        Some(match command {
            CommandId::DrawingBrush => self.drawing(),
            CommandId::Sculpt => self.sculpt(),
            _ => self.tool(command.paint_tool()?),
        })
    }
    pub(crate) fn command_preset_in(&self, command: CommandId, brush: &BrushState, canvas_tool: LayerCanvasTool) -> Option<u32> {
        let remembered = self.command_preset(command)?;
        let active = canvas_tool == LayerCanvasTool::Paint && match command {
            CommandId::DrawingBrush => is_drawing(brush.tool),
            CommandId::Sculpt => is_sculpt(brush.tool),
            _ => command.paint_tool() == Some(brush.tool),
        };
        Some(if active { brush.preset } else { remembered })
    }
    pub(crate) fn tooltip_label(&self, command: CommandId, label: &str, brush: &BrushState, canvas_tool: LayerCanvasTool, localizer: &Localizer) -> String {
        if command == CommandId::CenterZoomClicks { return localizer.text(MessageId::ZOOM_TOOL_CENTER_ON_CLICK_TOOLTIP).to_string(); }
        let Some(preset) = self.command_preset_in(command, brush, canvas_tool) else { return label.to_string(); };
        let medium = group(preset).localized_label(localizer);
        if medium.as_ref() == label { label.to_string() } else { format!("{medium} · {label}") }
    }
    pub fn remember(&mut self, id: u32) -> bool {
        let tool = tool(id);
        let mut changed = self.tools.insert(tool, id) != Some(id);
        changed |= self.groups.insert(group(id), id) != Some(id);
        if is_drawing(tool) { changed |= self.drawing != Some(id); self.drawing = Some(id); }
        if is_sculpt(tool) { changed |= self.sculpt != Some(id); self.sculpt = Some(id); }
        changed
    }
    pub fn set_override(&mut self, id: u32, setting: &str, value: f32, localizer: &Localizer) -> Result<(), String> {
        let defaults = layer_core::default_brush(preset(id)?);
        crate::tool_settings::edit(&defaults, setting, value, localizer)?;
        let default = crate::tool_settings::value(&defaults, setting).ok_or("Unknown tool setting")?;
        if value == default {
            if let Some(values) = self.overrides.get_mut(&id) {
                values.remove(setting);
                if values.is_empty() {
                    self.overrides.remove(&id);
                }
            }
        } else {
            self.overrides
                .entry(id)
                .or_default()
                .insert(setting.into(), value);
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), crate::WorkspaceValidationError> {
        for (id, includes) in [(self.drawing, is_drawing as fn(Tool) -> bool), (self.sculpt, is_sculpt)] {
            if let Some(id) = id {
                preset(id)?;
                if !includes(tool(id)) { return Err("Invalid remembered tool class".into()); }
            }
        }
        for (&tool, &id) in &self.tools {
            preset(id)?;
            if self::tool(id) != tool {
                return Err("Invalid remembered tool".into());
            }
        }
        for (&saved_group, &id) in &self.groups {
            preset(id)?;
            if group(id) != saved_group {
                return Err("Invalid remembered tool group".into());
            }
        }
        for (&id, values) in &self.overrides {
            let mut brush = layer_core::default_brush(preset(id)?);
            for (setting, &value) in values {
                brush = crate::tool_settings::edit_value(&brush, setting, value)?;
            }
        }
        Ok(())
    }
    pub fn tool(&self, tool: Tool) -> u32 {
        self.tools
            .get(&tool)
            .copied()
            .unwrap_or_else(|| tool.default_preset())
    }
    pub(crate) fn drawing(&self) -> u32 {
        self.drawing.unwrap_or_else(|| self.tool(Tool::Pencil))
    }
    pub(crate) fn sculpt(&self) -> u32 {
        self.sculpt.unwrap_or_else(|| self.tool(Tool::Blend))
    }
    pub fn group(&self, group: ToolGroup) -> u32 {
        self.groups
            .get(&group)
            .copied()
            .unwrap_or_else(|| group.default_preset())
    }
    /// Built-in pigment definitions use linear sRGB. Resolve them once into
    /// the destination document before exposing the configured brush to edits.
    pub(crate) fn brush_in(&self, preset: DefaultBrushPreset, space: layer_core::color::RgbSpace) -> BrushSnapshot {
        let mut brush = self.brush(preset);
        let transform = layer_core::color::RgbSpace::Srgb.linear_transform(space);
        for color in [&mut brush.color_rgba_linear, &mut brush.color_dynamics.secondary_color_rgba_linear] {
            let rgb = layer_core::color::rgb::apply(transform, [color[0] as f64, color[1] as f64, color[2] as f64]);
            color[..3].copy_from_slice(&rgb.map(|v| v as f32));
        }
        brush
    }
    pub(crate) fn path_blocker(&self, preset: DefaultBrushPreset) -> Option<&str> {
        let defaults = layer_core::default_brush(preset);
        if !defaults.draws_vector_lines() { return None; }
        self.overrides.get(&(preset as u32))?.iter()
            .find(|(setting, value)| crate::tool_settings::edit_value(&defaults, setting, **value).is_ok_and(|brush| !brush.draws_vector_lines()))
            .map(|(setting, _)| setting.as_str())
    }
    pub fn brush(&self, preset: DefaultBrushPreset) -> BrushSnapshot {
        let mut brush = layer_core::default_brush(preset);
        if let Some(values) = self.overrides.get(&(preset as u32)) {
            for (setting, &value) in values {
                brush = crate::tool_settings::edit_value(&brush, setting, value)
                    .expect("validated workspace brush override");
            }
        }
        brush
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn localized_catalog_membership_uses_medium_identity() {
        let canonical: Vec<_> = brush_catalog().map(|choice| (choice.id, choice.group, choice.icon)).collect();
        for language in UiLanguage::ALL {
            let localizer = Localizer::shared(language);
            let localized: Vec<_> = brush_catalog_localized(&localizer).map(|choice| (choice.id, choice.group, choice.icon)).collect();
            assert_eq!(localized, canonical);
            let categories: Vec<_> = brush_categories_localized(&localizer).collect();
            assert_eq!(categories.len(), ToolGroup::ALL.len());
            for category in categories {
                assert!(!category.brushes.is_empty());
                assert_eq!(category.label, category.id.localized_label(&localizer));
                for choice in category.brushes {
                    assert_eq!(choice.group, category.id);
                    assert_eq!(group(choice.id), category.id);
                    assert_eq!(brush_label_localized(choice.id, &localizer).as_ref(), Some(&choice.label));
                }
            }
        }
    }

    #[test]
    fn medium_icons_survive_categories_subtools_and_toolbar_customization() {
        let catalog = ui_catalog();
        let icons: std::collections::BTreeSet<_> = ToolGroup::ALL.map(ToolGroup::icon).into();
        assert_eq!(icons.len(), ToolGroup::ALL.len(), "Different media must remain distinguishable");
        for category in &catalog.brush_categories {
            assert!(crate::icon_ships(category.icon));
            for choice in &category.brushes {
                let brush = BrushState { preset: choice.id, tool: tool(choice.id), diameter: 10., opacity: 1., color: [0., 0., 0., 1.] };
                let view = view(&brush, LayerCanvasTool::Paint, &Localizer::shared(UiLanguage::English));
                let selected = view.groups.iter().find(|g| g.selected).unwrap();
                assert_eq!((&selected.label, selected.icon), (&category.label, category.icon));
                assert_eq!(view.subtools.iter().find(|b| b.selected).unwrap().icon, choice.icon);
                assert_eq!(crate::customization::canonical_tool_choice(ToolbarControl::Brush { id: choice.id }).icon, choice.icon);
            }
        }
    }

    #[test]
    fn non_painting_modes_have_distinct_packaged_icons() {
        let brush = BrushState { preset: Tool::Pen.default_preset(), tool: Tool::Pen, diameter: 10., opacity: 1., color: [0., 0., 0., 1.] };
        for tool in [
            LayerCanvasTool::Gradient {shape:layer_core::GradientShape::Linear},
            LayerCanvasTool::Region { fill: true, source: RegionSource::Visible },
            LayerCanvasTool::Region { fill: false, source: RegionSource::Visible },
            LayerCanvasTool::PickVisible,
            LayerCanvasTool::Figure { shape: FigureShape::Rectangle, paint: FigurePaint::Outline },
            LayerCanvasTool::Figure { shape: FigureShape::Ellipse, paint: FigurePaint::Outline },
            LayerCanvasTool::Ruler { kind: RulerKind::Straight },
            LayerCanvasTool::Move,
        ] {
            let view = view(&brush, tool, &Localizer::shared(UiLanguage::English));
            for items in [&view.groups, &view.subtools] {
                let unique: std::collections::BTreeSet<_> = items.iter().map(|i| i.icon).collect();
                assert_eq!(unique.len(), items.len(), "{tool:?}: each mode needs its own meaning");
                for item in items {
                    assert!(crate::icon_ships(item.icon));
                }
            }
        }
    }

    #[test]
    fn every_brush_has_one_tool_and_group_and_every_group_is_populated() {
        assert_eq!(PRESETS.len(), 65);
        assert_eq!(ToolGroup::ALL.len(), 13);
        let mut ids = std::collections::BTreeSet::new();
        for &(preset, _, group) in PRESETS {
            assert!(ids.insert(preset as u32));
            assert!(group.holds(tool(preset as u32)));
        }
        for group in ToolGroup::ALL {
            assert!(ids.contains(&group.default_preset()));
        }
        for tool in Tool::ALL {
            assert_eq!(self::tool(tool.default_preset()), tool);
        }
    }
}
