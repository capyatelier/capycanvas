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
    fn default_preset(self) -> u32 {
        PRESETS.iter().find(|p| p.2 == self).unwrap().0 as u32
    }
}

// Each preset has exactly one owning tool/group. Stable numeric preset IDs are
// also the keys of the GPU preview cache and saved brush assets.
const PRESETS: &[(DefaultBrushPreset, &str, ToolGroup)] = &[
    (DefaultBrushPreset::GPen, "G-Pen", ToolGroup::Pen),
    (DefaultBrushPreset::Marker, "Marker", ToolGroup::Marker),
    (DefaultBrushPreset::Pencil, "Pencil", ToolGroup::Pencil),
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
    })
}
pub fn brush_categories() -> impl Iterator<Item = BrushCategory> {
    ToolGroup::ALL.into_iter().map(|group| BrushCategory {
        label: group.label(),
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
                ("Visible artwork", RegionSource::Visible),
                ("Editing layer", RegionSource::Editing),
                ("Reference layers", RegionSource::Reference),
            ]
            .into_iter()
            .map(|(label, item_source)| ToolSetItem {
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
                    icon: "gradient",
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
                ("Visible color", LayerCanvasTool::PickVisible),
                ("Layer color", LayerCanvasTool::PickLayer),
            ]
            .into_iter()
            .map(|(label, tool)| ToolSetItem {
                label,
                icon: "eyedropper",
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
            LayerCanvasTool::LassoFill => ("Lasso fill", "lasso"),
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
                icon: g.tool().command().icon().unwrap(),
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
                icon: group.tool().command().icon().unwrap(),
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
    pub overrides: BTreeMap<u32, BTreeMap<String, f32>>,
}
pub(crate) type ToolMemory = WorkspaceToolMemory;
impl WorkspaceToolMemory {
    pub fn remember(&mut self, id: u32, _brush: &BrushSnapshot) {
        let group = group(id);
        self.tools.insert(group.tool(), id);
        self.groups.insert(group, id);
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
    pub fn group(&self, group: ToolGroup) -> u32 {
        self.groups
            .get(&group)
            .copied()
            .unwrap_or_else(|| group.default_preset())
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
    fn every_brush_has_one_tool_and_group_and_every_group_is_populated() {
        assert_eq!(PRESETS.len(), 24);
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
