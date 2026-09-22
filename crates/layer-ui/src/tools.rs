//! Tool/group identity and subtool memory. Hosts only render the resolved view.
use crate::*;
use layer_core::{BrushSnapshot, DefaultBrushPreset};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    #[default]
    Pen,
    Pencil,
    Brush,
    Eraser,
    Airbrush,
    Decoration,
    Blend,
    Liquify,
}
impl Tool {
    pub const ALL: [Self; 8] = [
        Self::Pen,
        Self::Pencil,
        Self::Brush,
        Self::Eraser,
        Self::Airbrush,
        Self::Decoration,
        Self::Blend,
        Self::Liquify,
    ];
    pub fn command(self) -> CommandId {
        match self {
            Self::Pen => CommandId::Pen,
            Self::Pencil => CommandId::Pencil,
            Self::Brush => CommandId::Brush,
            Self::Eraser => CommandId::Eraser,
            Self::Airbrush => CommandId::Airbrush,
            Self::Decoration => CommandId::Decoration,
            Self::Blend => CommandId::Blend,
            Self::Liquify => CommandId::Liquify,
        }
    }
    pub fn default_preset(self) -> u32 {
        PRESETS.iter().find(|p| p.2.tool() == self).unwrap().0 as u32
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
}
impl ToolFamily {
    pub const ALL: [Self; 3] = [Self::Ink, Self::Paint, Self::Blend];
    pub fn commands(self) -> &'static [CommandId] {
        match self {
            Self::Ink => &[CommandId::Pen, CommandId::Pencil],
            Self::Paint => &[CommandId::Brush, CommandId::Airbrush, CommandId::Decoration],
            Self::Blend => &[CommandId::Blend, CommandId::Liquify],
        }
    }
    pub fn shortcut_id(self) -> &'static str {
        match self {
            Self::Ink => "tools.ink",
            Self::Paint => "tools.paint",
            Self::Blend => "tools.blend",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Ink => "Pen / Pencil",
            Self::Paint => "Paint tools",
            Self::Blend => "Blend / Liquify",
        }
    }
    pub fn for_command(command: CommandId) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|f| f.commands().contains(&command))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolGroup {
    Pen,
    Marker,
    Pencil,
    Pastel,
    Paint,
    Watercolor,
    Oil,
    Eraser,
    Airbrush,
    Spray,
    Decoration,
    Blend,
    Liquify,
}
impl ToolGroup {
    pub const ALL: [Self; 13] = [
        Self::Pen,
        Self::Marker,
        Self::Pencil,
        Self::Pastel,
        Self::Paint,
        Self::Watercolor,
        Self::Oil,
        Self::Eraser,
        Self::Airbrush,
        Self::Spray,
        Self::Decoration,
        Self::Blend,
        Self::Liquify,
    ];
    pub fn tool(self) -> Tool {
        match self {
            Self::Pen | Self::Marker => Tool::Pen,
            Self::Pencil | Self::Pastel => Tool::Pencil,
            Self::Paint | Self::Watercolor | Self::Oil => Tool::Brush,
            Self::Eraser => Tool::Eraser,
            Self::Airbrush | Self::Spray => Tool::Airbrush,
            Self::Decoration => Tool::Decoration,
            Self::Blend => Tool::Blend,
            Self::Liquify => Tool::Liquify,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Pen => "Pen",
            Self::Marker => "Marker",
            Self::Pencil => "Pencil",
            Self::Pastel => "Pastel",
            Self::Paint => "Paint",
            Self::Watercolor => "Watercolor",
            Self::Oil => "Oil paint",
            Self::Eraser => "Eraser",
            Self::Airbrush => "Airbrush",
            Self::Spray => "Spray",
            Self::Decoration => "Texture",
            Self::Blend => "Blend",
            Self::Liquify => "Liquify",
        }
    }
    /// Identity of the medium, independent of its parent drawing engine.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Pen => "pen",
            Self::Marker => "marker",
            Self::Pencil => "pencil",
            Self::Pastel => "pastel",
            Self::Paint => "paint",
            Self::Watercolor => "watercolor",
            Self::Oil => "oil-paint",
            Self::Eraser => "eraser",
            Self::Airbrush => "airbrush",
            Self::Spray => "spray",
            Self::Decoration => "decoration",
            Self::Blend => "blend",
            Self::Liquify => "liquify",
        }
    }
    fn default_preset(self) -> u32 {
        PRESETS.iter().find(|p| p.2 == self).unwrap().0 as u32
    }
}

// Each preset has exactly one owning tool/group. Stable numeric preset IDs are
// also the keys of the GPU preview cache and saved brush assets.
const PRESETS: &[(DefaultBrushPreset, &str, ToolGroup)] = &[
    (DefaultBrushPreset::GPen, "G-Pen", ToolGroup::Pen),
    (DefaultBrushPreset::RoughGPen, "Rough G-Pen", ToolGroup::Pen),
    (
        DefaultBrushPreset::CalligraphyPen,
        "Calligraphy Pen",
        ToolGroup::Pen,
    ),
    (
        DefaultBrushPreset::AntiquePen,
        "Antique Pen",
        ToolGroup::Pen,
    ),
    (
        DefaultBrushPreset::RealisticPen,
        "Realistic Pen",
        ToolGroup::Pen,
    ),
    (DefaultBrushPreset::WetInk, "Wet Ink", ToolGroup::Pen),
    (DefaultBrushPreset::BlottyInk, "Blotty Ink", ToolGroup::Pen),
    (
        DefaultBrushPreset::BrushedInk,
        "Realistic Brushed Ink",
        ToolGroup::Pen,
    ),
    (DefaultBrushPreset::Marker, "Marker", ToolGroup::Marker),
    (DefaultBrushPreset::Pencil, "Pencil", ToolGroup::Pencil),
    (
        DefaultBrushPreset::PointyPencil,
        "Pointy Pencil",
        ToolGroup::Pencil,
    ),
    (
        DefaultBrushPreset::ShadingPencil,
        "Shading Pencil",
        ToolGroup::Pencil,
    ),
    (DefaultBrushPreset::Charcoal, "Charcoal", ToolGroup::Pastel),
    (DefaultBrushPreset::Chalk, "Chalk", ToolGroup::Pastel),
    (
        DefaultBrushPreset::PastelBlock,
        "Pastel Block",
        ToolGroup::Pastel,
    ),
    (
        DefaultBrushPreset::Paintbrush,
        "Paintbrush",
        ToolGroup::Paint,
    ),
    (
        DefaultBrushPreset::TexturedFlat,
        "Textured Flat",
        ToolGroup::Paint,
    ),
    (
        DefaultBrushPreset::DryScumble,
        "Dry Scumble",
        ToolGroup::Paint,
    ),
    (
        DefaultBrushPreset::TransparentGlaze,
        "Transparent Glaze",
        ToolGroup::Paint,
    ),
    (
        DefaultBrushPreset::OpaqueGouache,
        "Opaque Gouache",
        ToolGroup::Paint,
    ),
    (
        DefaultBrushPreset::MultiplyGlaze,
        "Multiply Glaze",
        ToolGroup::Paint,
    ),
    (
        DefaultBrushPreset::WatercolorWash,
        "Watercolor Wash",
        ToolGroup::Watercolor,
    ),
    (
        DefaultBrushPreset::WetWatercolor,
        "Wet Watercolor",
        ToolGroup::Watercolor,
    ),
    (DefaultBrushPreset::LoadedOil, "Loaded Oil", ToolGroup::Oil),
    (
        DefaultBrushPreset::PaletteKnife,
        "Palette Knife",
        ToolGroup::Oil,
    ),
    (DefaultBrushPreset::WetRound, "Wet Round", ToolGroup::Oil),
    (DefaultBrushPreset::Eraser, "Eraser", ToolGroup::Eraser),
    (
        DefaultBrushPreset::Airbrush,
        "Airbrush",
        ToolGroup::Airbrush,
    ),
    (DefaultBrushPreset::Spray, "Spray", ToolGroup::Spray),
    (
        DefaultBrushPreset::DualTexture,
        "Dual Texture",
        ToolGroup::Decoration,
    ),
    (
        DefaultBrushPreset::NaturalBlender,
        "Natural Blender",
        ToolGroup::Blend,
    ),
    (DefaultBrushPreset::Smudge, "Smudge", ToolGroup::Blend),
    (
        DefaultBrushPreset::LiquifyPush,
        "Liquify Push",
        ToolGroup::Liquify,
    ),
    (
        DefaultBrushPreset::LiquifyTwirl,
        "Liquify Twirl",
        ToolGroup::Liquify,
    ),
];

pub fn brush_catalog() -> impl Iterator<Item = BrushChoice> {
    PRESETS.iter().map(|&(preset, label, group)| BrushChoice {
        id: preset as u32,
        label,
        category: group.label(),
        icon: group.icon(),
    })
}
pub fn brush_categories() -> impl Iterator<Item = BrushCategory> {
    ToolGroup::ALL.into_iter().map(|group| BrushCategory {
        label: group.label(),
        icon: group.icon(),
        brushes: brush_catalog()
            .filter(|b| b.category == group.label())
            .collect(),
    })
}
pub(crate) fn preset(id: u32) -> Result<DefaultBrushPreset, String> {
    PRESETS
        .iter()
        .find(|p| p.0 as u32 == id)
        .map(|p| p.0)
        .ok_or_else(|| "Unknown brush".into())
}
pub(crate) fn group(id: u32) -> ToolGroup {
    PRESETS
        .iter()
        .find(|p| p.0 as u32 == id)
        .expect("validated brush")
        .2
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolSetItem {
    pub label: &'static str,
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
impl ToolPanels {
    pub(crate) fn new(brush: &BrushState, canvas_tool: LayerCanvasTool, current: &ToolSetView) -> Self {
        Self {
            brush_sets: ToolSetView {
                groups: sets(brush, canvas_tool, is_drawing),
                subtools: Vec::new(),
            },
            sculpt_sets: ToolSetView {
                groups: sets(brush, canvas_tool, is_sculpt),
                subtools: Vec::new(),
            },
            tools: ToolSetView {
                groups: Vec::new(),
                subtools: current.subtools.clone(),
            },
        }
    }
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
    !matches!(tool, Tool::Eraser | Tool::Blend | Tool::Liquify)
}

pub(crate) fn is_sculpt(tool: Tool) -> bool {
    matches!(tool, Tool::Blend | Tool::Liquify)
}

fn sets(brush: &BrushState, canvas_tool: LayerCanvasTool, includes: fn(Tool) -> bool) -> Vec<ToolSetItem> {
    ToolGroup::ALL
        .into_iter()
        .filter(|set| includes(set.tool()))
        .map(|set| ToolSetItem {
            label: set.label(),
            icon: set.icon(),
            action: UiAction::SelectBrushSet { group: set },
            selected: canvas_tool == LayerCanvasTool::Paint && set == group(brush.preset),
            preview: None,
        })
        .collect()
}

pub(crate) fn view(brush: &BrushState, canvas_tool: LayerCanvasTool) -> ToolSetView {
    if matches!(
        canvas_tool,
        LayerCanvasTool::Move | LayerCanvasTool::Transform
    ) {
        return crate::session::operation::tool_set(canvas_tool == LayerCanvasTool::Transform);
    }
    if let LayerCanvasTool::Ruler { kind } = canvas_tool {
        return crate::session::rulers::tool_set(kind);
    }
    if let LayerCanvasTool::Figure { shape, paint } = canvas_tool {
        return crate::session::figures::tool_set(shape, paint);
    }
    if let LayerCanvasTool::Region { fill, source } = canvas_tool {
        let command = if fill {
            CommandId::Fill
        } else {
            CommandId::AutoSelect
        };
        let icon = command.icon().unwrap();
        return ToolSetView {
            groups: vec![ToolSetItem {
                label: command.label(),
                icon,
                action: UiAction::Invoke { command },
                selected: true,
                preview: None,
            }],
            subtools: [
                ("Visible artwork", "eye", RegionSource::Visible),
                ("Editing layer", "layers", RegionSource::Editing),
                ("Reference layers", "reference", RegionSource::Reference),
            ]
            .into_iter()
            .map(|(label, icon, item_source)| ToolSetItem {
                label,
                icon,
                action: UiAction::Layer {
                    action: LayerAction::Tool {
                        tool: LayerCanvasTool::Region {
                            fill,
                            source: item_source,
                        },
                    },
                },
                selected: source == item_source,
                preview: None,
            })
            .collect(),
        };
    }
    if let LayerCanvasTool::Gradient { .. } = canvas_tool {
        return ToolSetView {
            groups: vec![ToolSetItem {
                label: "Gradient",
                icon: "gradient",
                action: UiAction::Invoke {
                    command: CommandId::Gradient,
                },
                selected: true,
                preview: None,
            }],
            subtools: [
                ("Linear: color to color", false, false),
                ("Linear: color to clear", false, true),
                ("Radial: color to color", true, false),
                ("Radial: color to clear", true, true),
            ]
            .into_iter()
            .map(|(label, radial, transparent)| {
                let tool = LayerCanvasTool::Gradient {
                    radial,
                    transparent,
                };
                ToolSetItem {
                    label,
                    icon: match (radial, transparent) {
                        (false, false) => "gradient",
                        (false, true) => "gradient-transparent",
                        (true, false) => "gradient-radial",
                        (true, true) => "gradient-radial-transparent",
                    },
                    action: UiAction::Layer {
                        action: LayerAction::Tool { tool },
                    },
                    selected: canvas_tool == tool,
                    preview: None,
                }
            })
            .collect(),
        };
    }
    if canvas_tool.picks_color() {
        return ToolSetView {
            groups: vec![ToolSetItem {
                label: "Eyedropper",
                icon: "eyedropper",
                action: UiAction::Layer {
                    action: LayerAction::Tool { tool: canvas_tool },
                },
                selected: true,
                preview: None,
            }],
            subtools: [
                ("Visible color", "eye", LayerCanvasTool::PickVisible),
                ("Layer color", "layers", LayerCanvasTool::PickLayer),
            ]
            .into_iter()
            .map(|(label, icon, tool)| ToolSetItem {
                label,
                icon,
                action: UiAction::Layer {
                    action: LayerAction::Tool { tool },
                },
                selected: tool == canvas_tool,
                preview: None,
            })
            .collect(),
        };
    }
    if canvas_tool != LayerCanvasTool::Paint {
        let (label, icon) = match canvas_tool {
            LayerCanvasTool::Select => ("Lasso", "lasso"),
            LayerCanvasTool::LassoFill => ("Lasso fill", "lasso-fill"),
            LayerCanvasTool::Move | LayerCanvasTool::Transform => unreachable!(),
            LayerCanvasTool::Hand => ("Hand", "hand"),
            LayerCanvasTool::PickVisible | LayerCanvasTool::PickLayer => unreachable!(),
            LayerCanvasTool::Gradient { .. } => unreachable!(),
            LayerCanvasTool::Figure { .. } | LayerCanvasTool::Ruler { .. } => unreachable!(),
            LayerCanvasTool::Region { .. } => unreachable!(),
            LayerCanvasTool::Paint => unreachable!(),
        };
        let item = ToolSetItem {
            label,
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
            .filter(|g| g.tool() == brush.tool)
            .map(|g| ToolSetItem {
                label: g.label(),
                icon: g.icon(),
                action: UiAction::SelectToolGroup { group: g },
                selected: g == active,
                preview: None,
            })
            .collect(),
        subtools: PRESETS
            .iter()
            .filter(|p| p.2 == active)
            .map(|&(preset, label, group)| ToolSetItem {
                label,
                icon: group.icon(),
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
pub struct WorkspaceToolMemory {
    tools: BTreeMap<Tool, u32>,
    groups: BTreeMap<ToolGroup, u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    drawing: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sculpt: Option<u32>,
    pub overrides: BTreeMap<u32, BTreeMap<String, f32>>,
}
pub(crate) type ToolMemory = WorkspaceToolMemory;
impl WorkspaceToolMemory {
    pub fn remember(&mut self, id: u32, _brush: &BrushSnapshot) {
        let group = group(id);
        self.tools.insert(group.tool(), id);
        self.groups.insert(group, id);
        if is_drawing(group.tool()) { self.drawing = Some(id); }
        if is_sculpt(group.tool()) { self.sculpt = Some(id); }
    }
    pub fn set_override(&mut self, id: u32, setting: &str, value: f32) -> Result<(), String> {
        let defaults = layer_core::default_brush(preset(id)?);
        crate::tool_settings::edit(&defaults, setting, value)?;
        let default = crate::tool_settings::controls(&defaults)
            .into_iter()
            .find(|c| c.id == setting)
            .ok_or("Unknown tool setting")?
            .value;
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
    pub fn validate(&self) -> Result<(), String> {
        for (id, includes) in [(self.drawing, is_drawing as fn(Tool) -> bool), (self.sculpt, is_sculpt)] {
            if let Some(id) = id {
                preset(id)?;
                if !includes(group(id).tool()) { return Err("Invalid remembered tool class".into()); }
            }
        }
        for (&tool, &id) in &self.tools {
            preset(id)?;
            if group(id).tool() != tool {
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
                brush = crate::tool_settings::edit(&brush, setting, value)?;
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
        self.drawing.unwrap_or_else(|| self.tool(Tool::Brush))
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
    pub fn brush(&self, preset: DefaultBrushPreset) -> BrushSnapshot {
        let mut brush = layer_core::default_brush(preset);
        if let Some(values) = self.overrides.get(&(preset as u32)) {
            for (setting, &value) in values {
                brush = crate::tool_settings::edit(&brush, setting, value)
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
    fn medium_icons_survive_categories_subtools_and_toolbar_customization() {
        let catalog = ui_catalog();
        let icons: std::collections::BTreeSet<_> = ToolGroup::ALL.map(ToolGroup::icon).into();
        assert_eq!(icons.len(), ToolGroup::ALL.len(), "Different media must remain distinguishable");
        for category in &catalog.brush_categories {
            assert!(catalog.icons.contains(&category.icon));
            for choice in &category.brushes {
                let group = group(choice.id);
                let brush = BrushState { preset: choice.id, tool: group.tool(), diameter: 10., opacity: 1., color: [0., 0., 0., 1.] };
                let view = view(&brush, LayerCanvasTool::Paint);
                let selected = view.groups.iter().find(|g| g.selected).unwrap();
                assert_eq!((selected.label, selected.icon), (category.label, category.icon));
                assert_eq!(view.subtools.iter().find(|b| b.selected).unwrap().icon, choice.icon);
                assert_eq!(crate::customization::tool_choice(ToolbarControl::Brush { id: choice.id }).icon, choice.icon);
            }
        }
    }

    #[test]
    fn non_painting_modes_have_distinct_packaged_icons() {
        let brush = BrushState { preset: Tool::Pen.default_preset(), tool: Tool::Pen, diameter: 10., opacity: 1., color: [0., 0., 0., 1.] };
        let bank = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/layer-web/icons");
        let catalog = ui_catalog();
        for tool in [
            LayerCanvasTool::Gradient { radial: false, transparent: false },
            LayerCanvasTool::Region { fill: true, source: RegionSource::Visible },
            LayerCanvasTool::Region { fill: false, source: RegionSource::Visible },
            LayerCanvasTool::PickVisible,
            LayerCanvasTool::Figure { shape: FigureShape::Rectangle, paint: FigurePaint::Outline },
            LayerCanvasTool::Figure { shape: FigureShape::Ellipse, paint: FigurePaint::Outline },
            LayerCanvasTool::Ruler { kind: RulerKind::Straight },
            LayerCanvasTool::Move,
        ] {
            let view = view(&brush, tool);
            for items in [&view.groups, &view.subtools] {
                let unique: std::collections::BTreeSet<_> = items.iter().map(|i| i.icon).collect();
                assert_eq!(unique.len(), items.len(), "{tool:?}: each mode needs its own meaning");
                for item in items {
                    assert!(catalog.icons.contains(&item.icon));
                    assert!(bank.join(format!("layer-{}-symbolic.svg", item.icon)).is_file());
                }
            }
        }
    }

    #[test]
    fn every_brush_has_one_tool_and_group_and_every_group_is_populated() {
        assert_eq!(PRESETS.len(), 34);
        let mut ids = std::collections::BTreeSet::new();
        for &(preset, _, group) in PRESETS {
            assert!(ids.insert(preset as u32));
            assert!(Tool::ALL.contains(&group.tool()));
        }
        for group in ToolGroup::ALL {
            assert!(ids.contains(&group.default_preset()));
        }
        for tool in Tool::ALL {
            assert_eq!(group(tool.default_preset()).tool(), tool);
        }
    }
}
