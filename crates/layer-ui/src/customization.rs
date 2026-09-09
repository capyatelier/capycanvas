//! Panel configuration and contextual editing. Durable data lives in the dock
//! layout; open menus, inspectors and picker drafts belong to the UI session.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabStyle {
    #[default]
    Name,
    Icon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelControl {
    Brushes,
    BrushSize,
    SizePresets,
    BrushOpacity,
    BrushColor,
    Layers,
    LayerActions,
    LayerOpacity,
}
impl PanelControl {
    pub fn label(self) -> &'static str {
        match self {
            Self::Brushes => "Brushes",
            Self::BrushSize => "Brush size",
            Self::SizePresets => "Size presets",
            Self::BrushOpacity => "Brush opacity",
            Self::BrushColor => "Brush color",
            Self::Layers => "Layers",
            Self::LayerActions => "Layer actions",
            Self::LayerOpacity => "Layer opacity",
        }
    }
    pub fn available(panel: Panel) -> &'static [Self] {
        match panel {
            Panel::Brushes => &[
                Self::Brushes,
                Self::BrushSize,
                Self::BrushOpacity,
                Self::BrushColor,
            ],
            Panel::Sizes => &[
                Self::BrushSize,
                Self::SizePresets,
                Self::BrushOpacity,
                Self::BrushColor,
            ],
            Panel::Layers => &[Self::LayerActions, Self::Layers, Self::LayerOpacity],
            _ => &[],
        }
    }
    fn defaults(panel: Panel) -> &'static [Self] {
        match panel {
            Panel::Brushes => &[Self::Brushes],
            Panel::Sizes => &[Self::BrushSize, Self::SizePresets],
            Panel::Layers => Self::available(panel),
            _ => &[],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolbarTile {
    pub id: u32,
    pub control: ToolbarControl,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PanelContent {
    Controls {
        visible: Vec<PanelControl>,
    },
    Toolbar {
        name: String,
        tiles: Vec<ToolbarTile>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelConfig {
    pub id: Panel,
    #[serde(default)]
    pub tab_style: TabStyle,
    pub content: PanelContent,
}
impl PanelConfig {
    pub fn title(&self) -> &str {
        match &self.content {
            PanelContent::Toolbar { name, .. } => name,
            PanelContent::Controls { .. } => self.id.label(),
        }
    }
    pub fn tiles(&self) -> &[ToolbarTile] {
        match &self.content {
            PanelContent::Toolbar { tiles, .. } => tiles,
            _ => &[],
        }
    }
    pub(crate) fn tiles_mut(&mut self) -> Result<&mut Vec<ToolbarTile>, String> {
        match &mut self.content {
            PanelContent::Toolbar { tiles, .. } => Ok(tiles),
            _ => Err("Choose a toolbar".into()),
        }
    }
    pub fn shows(&self, control: PanelControl) -> bool {
        matches!(&self.content, PanelContent::Controls { visible } if visible.contains(&control))
    }
    pub(crate) fn defaults() -> Vec<Self> {
        Panel::ALL
            .into_iter()
            .map(|id| Self {
                id,
                tab_style: TabStyle::Name,
                content: if id == Panel::Toolbar {
                    PanelContent::Toolbar {
                        name: id.label().into(),
                        tiles: TOOLBAR_CONTROLS
                            .iter()
                            .enumerate()
                            .map(|(index, &control)| ToolbarTile {
                                id: index as u32 + 1,
                                control,
                            })
                            .collect(),
                    }
                } else {
                    PanelContent::Controls {
                        visible: PanelControl::defaults(id).to_vec(),
                    }
                },
            })
            .collect()
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        match &self.content {
            PanelContent::Controls { visible } if self.id.kind() == PanelKind::Content => {
                for (index, control) in visible.iter().enumerate() {
                    if !PanelControl::available(self.id).contains(control)
                        || visible[..index].contains(control)
                    {
                        return Err("Invalid panel control visibility".into());
                    }
                }
            }
            PanelContent::Toolbar { name, tiles } if self.id.kind() == PanelKind::Tiles => {
                validate_toolbar_name(name)?;
                for tile in tiles {
                    tile.control.validate()?;
                }
            }
            _ => return Err("Panel configuration does not match its type".into()),
        }
        Ok(())
    }
}

pub(crate) fn validate_toolbar_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Enter a toolbar name".into());
    }
    if name != name.trim() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err("Use a name of 1–64 characters without extra spaces".into());
    }
    Ok(())
}

impl ToolbarControl {
    pub fn action(self) -> UiAction {
        match self {
            Self::Command { command } => UiAction::Invoke { command },
            Self::Brush { id } => UiAction::SelectBrush { id },
            Self::Size { pixels } => UiAction::SetBrushSize {
                value: pixels as f32,
            },
            Self::Color | Self::Opacity => UiAction::Customize {
                action: CustomizationAction::OpenControl {
                    control: if self == Self::Color {
                        PanelControl::BrushColor
                    } else {
                        PanelControl::BrushOpacity
                    },
                },
            },
        }
    }
    pub fn validate(self) -> Result<(), String> {
        match self {
            Self::Brush { id } => {
                preset(id)?;
            }
            Self::Size { pixels } => {
                NumericControl::brush_size().validate(pixels as f32, "Brush size")?;
            }
            _ => (),
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContextTarget {
    Panel { panel: Panel },
    Group { group: u32 },
    Tile { panel: Panel, tile: u32 },
    Ribbon { panel: Panel },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CustomizationAction {
    SetTabStyle {
        target: ContextTarget,
        style: TabStyle,
    },
    ShowAllControls {
        panel: Panel,
    },
    CloseExpanded,
    SetControlVisible {
        panel: Panel,
        control: PanelControl,
        visible: bool,
    },
    OpenControl {
        control: PanelControl,
    },
    CloseControl,
    NewToolbar {
        group: u32,
    },
    InsertTools {
        panel: Panel,
        before: Option<u32>,
    },
    RemoveTool {
        panel: Panel,
        tile: u32,
    },
    PickerName {
        name: String,
    },
    PickerSearch {
        query: String,
    },
    PickerSelect {
        control: ToolbarControl,
        selected: bool,
    },
    ConfirmTools,
    CancelTools,
}

#[derive(Clone, Debug, Serialize)]
pub struct ContextMenuItem {
    pub label: &'static str,
    /// None is an ordinary command; a value is a radio-style choice.
    pub selected: Option<bool>,
    pub action: CustomizationAction,
}
#[derive(Clone, Debug, Serialize)]
pub struct ContextMenu {
    pub title: String,
    pub sections: Vec<Vec<ContextMenuItem>>,
}
impl DockLayout {
    pub fn context_menu(&self, target: ContextTarget) -> Result<ContextMenu, String> {
        let entry = |label, action| ContextMenuItem {
            label,
            selected: None,
            action,
        };
        let (title, sections) = match target {
            ContextTarget::Panel { panel } => {
                let p = self.panel(panel)?;
                (
                    p.title().into(),
                    vec![
                        self.tab_style_items(target, &[panel])?,
                        vec![entry(
                            "Configure Panel…",
                            CustomizationAction::ShowAllControls { panel },
                        )],
                    ],
                )
            }
            ContextTarget::Group { group } => (
                "Panel Group".into(),
                vec![
                    self.tab_style_items(target, self.group_panels(group)?)?,
                    vec![entry(
                        "New Toolbar…",
                        CustomizationAction::NewToolbar { group },
                    )],
                ],
            ),
            ContextTarget::Tile { panel, tile } => {
                let p = self.panel(panel)?;
                let t = p
                    .tiles()
                    .iter()
                    .find(|t| t.id == tile)
                    .ok_or("The tool no longer exists")?;
                (
                    tool_choice(t.control).label,
                    vec![vec![
                        entry(
                            "Remove Tool",
                            CustomizationAction::RemoveTool { panel, tile },
                        ),
                        entry(
                            "Insert Tools…",
                            CustomizationAction::InsertTools {
                                panel,
                                before: Some(tile),
                            },
                        ),
                    ]],
                )
            }
            ContextTarget::Ribbon { panel } => {
                let p = self.panel(panel)?;
                if panel.kind() != PanelKind::Tiles {
                    return Err("Choose a toolbar".into());
                }
                (
                    p.title().into(),
                    vec![vec![entry(
                        "Add Tools…",
                        CustomizationAction::InsertTools {
                            panel,
                            before: None,
                        },
                    )]],
                )
            }
        };
        Ok(ContextMenu { title, sections })
    }
    fn tab_style_items(
        &self,
        target: ContextTarget,
        panels: &[Panel],
    ) -> Result<Vec<ContextMenuItem>, String> {
        let group = matches!(target, ContextTarget::Group { .. });
        [TabStyle::Name, TabStyle::Icon]
            .into_iter()
            .map(|style| {
                let mut selected = true;
                for &panel in panels {
                    selected &= self.panel(panel)?.tab_style == style;
                }
                Ok(ContextMenuItem {
                    label: match (group, style) {
                        (true, TabStyle::Name) => "Tab Names",
                        (true, TabStyle::Icon) => "Tab Icons",
                        (false, TabStyle::Name) => "Tab Name",
                        (false, TabStyle::Icon) => "Tab Icon",
                    },
                    selected: Some(selected),
                    action: CustomizationAction::SetTabStyle { target, style },
                })
            })
            .collect()
    }
    pub fn validate_toolbar_name(&self, name: &str) -> Result<(), String> {
        validate_toolbar_name(name.trim())?;
        if self
            .panels
            .iter()
            .any(|p| p.title().to_lowercase() == name.trim().to_lowercase())
        {
            return Err("A panel already uses this name".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolChoice {
    pub control: ToolbarControl,
    pub label: String,
    pub description: String,
    pub icon: &'static str,
    pub selected: bool,
}
pub fn tool_choice(control: ToolbarControl) -> ToolChoice {
    let (label, description, icon) = match control {
        ToolbarControl::Command { command } => (
            command.label().into(),
            match command {
                CommandId::Brush => "Paint with the current brush",
                CommandId::Eraser => "Erase paint from the active layer",
                CommandId::Undo => "Undo the last change",
                CommandId::Redo => "Restore the last undone change",
                CommandId::FitCanvas => "Fit the whole drawing in the available space",
                CommandId::Settings => "Open application preferences",
                CommandId::ToggleTheme => "Switch between light and dark appearance",
                CommandId::AddLayer => "Create a new paint layer",
                CommandId::DeleteLayer => "Delete the active paint layer",
                CommandId::RaiseLayer => "Move the active layer up",
                CommandId::LowerLayer => "Move the active layer down",
                CommandId::ResetLayout => "Restore panel docking positions",
                CommandId::TogglePanels => "Show or hide the docked panels",
                CommandId::ZenMode => "Hide controls while drawing away from the edges",
                CommandId::NewWindow => "Open another drawing window",
                CommandId::KeyboardShortcuts => "Customize application shortcuts",
                CommandId::About => "Application information and links",
            }
            .into(),
            command.icon().unwrap_or(match command {
                CommandId::ToggleTheme => "appearance",
                CommandId::KeyboardShortcuts => "keyboard",
                CommandId::About => "info",
                _ => "menu",
            }),
        ),
        ToolbarControl::Brush { id } => {
            let choice = brush_catalog().find(|b| b.id == id);
            (
                choice.map(|b| b.label).unwrap_or("Unknown brush").into(),
                format!(
                    "{} brush preset",
                    choice.map(|b| b.category).unwrap_or("Paint")
                ),
                "brush",
            )
        }
        ToolbarControl::Size { pixels } => (
            format!("{pixels} px"),
            "Set the brush diameter".into(),
            "size",
        ),
        ToolbarControl::Color => (
            "Brush color".into(),
            "Choose the current paint color".into(),
            "color",
        ),
        ToolbarControl::Opacity => (
            "Brush opacity".into(),
            "Adjust the strength of the current brush".into(),
            "opacity",
        ),
    };
    ToolChoice {
        control,
        label,
        description,
        icon,
        selected: false,
    }
}
fn tool_catalog(platform: Platform) -> Vec<ToolChoice> {
    CommandId::ALL
        .into_iter()
        .filter(|id| *id != CommandId::NewWindow || platform.native_windows())
        .map(|command| ToolbarControl::Command { command })
        .chain([ToolbarControl::Color, ToolbarControl::Opacity])
        .chain(brush_catalog().map(|b| ToolbarControl::Brush { id: b.id }))
        .chain(
            BRUSH_SIZES
                .iter()
                .map(|s| ToolbarControl::Size { pixels: *s as u16 }),
        )
        .map(tool_choice)
        .collect()
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolDestination {
    NewToolbar { group: u32, name: String },
    Insert { panel: Panel, before: Option<u32> },
}
#[derive(Clone, Debug, Serialize)]
pub struct ToolPicker {
    pub destination: ToolDestination,
    pub query: String,
    pub selected: Vec<ToolbarControl>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ToolPickerView {
    pub title: &'static str,
    pub confirm_label: &'static str,
    pub name: Option<String>,
    pub name_label: &'static str,
    pub search_hint: &'static str,
    pub query: String,
    pub choices: Vec<ToolChoice>,
    pub selected_count: usize,
    pub can_confirm: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PanelControlView {
    pub control: PanelControl,
    pub label: &'static str,
    pub visible_in_panel: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct TileView {
    pub id: u32,
    #[serde(flatten)]
    pub choice: ToolChoice,
    pub enabled: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct PanelView {
    pub id: Panel,
    pub title: String,
    pub icon: &'static str,
    pub tab_style: TabStyle,
    pub expanded: bool,
    pub configuration_title: String,
    pub configuration_hint: &'static str,
    pub controls: Vec<PanelControlView>,
    pub tiles: Vec<TileView>,
}

pub(crate) fn panel_view(state: &UiState, panel: Panel) -> Result<PanelView, String> {
    let config = state.workspace.layout.panel(panel)?;
    let expanded = state.customization.expanded == Some(panel);
    let controls = PanelControl::available(panel)
        .iter()
        .map(|&control| PanelControlView {
            control,
            label: control.label(),
            visible_in_panel: config.shows(control),
        })
        .collect();
    let tiles = config
        .tiles()
        .iter()
        .map(|tile| {
            let mut choice = tool_choice(tile.control);
            let mut enabled = true;
            choice.selected = match tile.control {
                ToolbarControl::Command { command } => {
                    if let Some(command) = state.commands.iter().find(|c| c.id == command) {
                        enabled = command.enabled;
                        command.selected
                    } else {
                        false
                    }
                }
                ToolbarControl::Brush { id } => state.brush.preset == id,
                ToolbarControl::Size { pixels } => {
                    (state.brush.diameter - pixels as f32).abs() < 0.01
                }
                _ => false,
            };
            TileView {
                id: tile.id,
                choice,
                enabled,
            }
        })
        .collect();
    Ok(PanelView {
        id: panel,
        title: config.title().into(),
        icon: panel.icon(),
        tab_style: config.tab_style,
        expanded,
        configuration_title: format!("Configure {}", config.title()),
        configuration_hint: if panel.kind() == crate::PanelKind::Tiles {
            "Add buttons here; drag buttons in the preview to reorder them."
        } else {
            "Choose the controls shown in the panel."
        },
        controls,
        tiles,
    })
}
impl ToolPicker {
    fn validate(&self, layout: &DockLayout) -> Result<(), String> {
        match &self.destination {
            ToolDestination::NewToolbar { group, name } => {
                layout.group_panels(*group)?;
                layout.validate_toolbar_name(name)?;
            }
            ToolDestination::Insert { panel, before } => {
                let p = layout.panel(*panel)?;
                if panel.kind() != PanelKind::Tiles {
                    return Err("Choose a toolbar".into());
                }
                if before.is_some_and(|id| !p.tiles().iter().any(|t| t.id == id)) {
                    return Err("The target tool no longer exists".into());
                }
            }
        }
        if self.selected.is_empty() {
            return Err("Select at least one tool".into());
        }
        Ok(())
    }
    pub fn view(&self, layout: &DockLayout, platform: Platform) -> ToolPickerView {
        let name = match &self.destination {
            ToolDestination::NewToolbar { name, .. } => Some(name.clone()),
            _ => None,
        };
        let words = self.query.to_lowercase();
        let choices = tool_catalog(platform)
            .into_iter()
            .filter_map(|mut choice| {
                choice.selected = self.selected.contains(&choice.control);
                let text = format!("{} {}", choice.label, choice.description).to_lowercase();
                words
                    .split_whitespace()
                    .all(|w| text.contains(w))
                    .then_some(choice)
            })
            .collect();
        ToolPickerView {
            title: if name.is_some() {
                "New Toolbar"
            } else {
                "Add Tools"
            },
            confirm_label: if name.is_some() {
                "Create Toolbar"
            } else {
                "Add Tools"
            },
            name,
            name_label: "Toolbar name",
            search_hint: "Search tools",
            query: self.query.clone(),
            choices,
            selected_count: self.selected.len(),
            can_confirm: self.validate(layout).is_ok(),
            error: self.error.clone().or_else(|| match &self.destination {
                ToolDestination::NewToolbar { name, .. } => {
                    layout.validate_toolbar_name(name).err()
                }
                _ => None,
            }),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CustomizationState {
    pub expanded: Option<Panel>,
    pub picker: Option<ToolPicker>,
    pub control: Option<PanelControl>,
}
impl CustomizationState {
    pub fn is_open(&self) -> bool {
        self.expanded.is_some() || self.picker.is_some() || self.control.is_some()
    }
    pub(crate) fn edit(
        &mut self,
        layout: &mut DockLayout,
        action: CustomizationAction,
        platform: Platform,
    ) -> Result<u32, String> {
        use CustomizationAction::*;
        let mut changed = regions::CUSTOMIZATION;
        match action {
            SetTabStyle { target, style } => {
                let panels = match target {
                    ContextTarget::Panel { panel } => vec![panel],
                    ContextTarget::Group { group } => layout.group_panels(group)?.to_vec(),
                    _ => return Err("Choose a tab or tab group".into()),
                };
                for &panel in &panels {
                    layout.panel(panel)?;
                }
                for panel in panels {
                    layout.panel_mut(panel)?.tab_style = style;
                }
                changed |= regions::LAYOUT;
            }
            ShowAllControls { panel } => {
                layout.panel(panel)?;
                let group = layout.panel_group(panel).ok_or("Panel is not docked")?;
                layout.select_tab(group, panel)?;
                self.picker = None;
                self.control = None;
                self.expanded = Some(panel);
                changed |= regions::LAYOUT;
            }
            CloseExpanded => self.expanded = None,
            SetControlVisible {
                panel,
                control,
                visible: show,
            } => {
                if !PanelControl::available(panel).contains(&control) {
                    return Err("Control does not belong to this panel".into());
                }
                let PanelContent::Controls { visible } = &mut layout.panel_mut(panel)?.content
                else {
                    return Err("Choose a system panel".into());
                };
                visible.retain(|c| *c != control);
                if show {
                    visible.push(control);
                }
                // Catalog order is the native-control order, independent of click order.
                visible.sort_by_key(|c| PanelControl::available(panel).iter().position(|p| p == c));
                changed |= regions::LAYOUT;
            }
            OpenControl { control } => {
                if !matches!(
                    control,
                    PanelControl::BrushColor | PanelControl::BrushOpacity
                ) {
                    return Err("Unsupported tool popup".into());
                }
                self.control = Some(control);
            }
            CloseControl => self.control = None,
            NewToolbar { group } => {
                layout.group_panels(group)?;
                let mut suffix = 1u32;
                let name = loop {
                    let name = format!("Toolbar {suffix}");
                    if layout.validate_toolbar_name(&name).is_ok() {
                        break name;
                    }
                    suffix = suffix.checked_add(1).ok_or("Toolbar names exhausted")?;
                };
                self.expanded = None;
                self.control = None;
                self.picker = Some(ToolPicker {
                    destination: ToolDestination::NewToolbar { group, name },
                    query: String::new(),
                    selected: Vec::new(),
                    error: None,
                });
            }
            InsertTools { panel, before } => {
                let p = layout.panel(panel)?;
                if panel.kind() != PanelKind::Tiles {
                    return Err("Choose a toolbar".into());
                }
                if before.is_some_and(|id| !p.tiles().iter().any(|t| t.id == id)) {
                    return Err("The target tool no longer exists".into());
                }
                self.expanded = None;
                self.control = None;
                self.picker = Some(ToolPicker {
                    destination: ToolDestination::Insert { panel, before },
                    query: String::new(),
                    selected: Vec::new(),
                    error: None,
                });
            }
            RemoveTool { panel, tile } => {
                layout.remove_tool(panel, tile)?;
                changed |= regions::LAYOUT;
            }
            PickerName { name } => {
                let p = self.picker.as_mut().ok_or("The tool picker is closed")?;
                let ToolDestination::NewToolbar { name: value, .. } = &mut p.destination else {
                    return Err("This picker does not create a toolbar".into());
                };
                *value = name;
                p.error = None;
            }
            PickerSearch { query } => {
                self.picker
                    .as_mut()
                    .ok_or("The tool picker is closed")?
                    .query = query;
            }
            PickerSelect { control, selected } => {
                if !tool_catalog(platform).iter().any(|c| c.control == control) {
                    return Err("This tool is not available".into());
                }
                let p = self.picker.as_mut().ok_or("The tool picker is closed")?;
                if selected && !p.selected.contains(&control) {
                    p.selected.push(control);
                }
                if !selected {
                    p.selected.retain(|c| *c != control);
                }
                p.error = None;
            }
            ConfirmTools => {
                let picker = self.picker.as_mut().ok_or("The tool picker is closed")?;
                let result = picker
                    .validate(layout)
                    .and_then(|_| match &picker.destination {
                        ToolDestination::NewToolbar { group, name } => layout
                            .add_toolbar(*group, name, &picker.selected)
                            .map(|_| ()),
                        ToolDestination::Insert { panel, before } => {
                            layout.insert_tools(*panel, *before, &picker.selected)
                        }
                    });
                if let Err(error) = result {
                    picker.error = Some(error);
                } else {
                    self.picker = None;
                    changed |= regions::LAYOUT;
                }
            }
            CancelTools => self.picker = None,
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const VIEWPORT: [f32; 2] = [1200.0, 900.0];
    const PEN: ToolbarControl = ToolbarControl::Command {
        command: CommandId::Brush,
    };
    const ERASE: ToolbarControl = ToolbarControl::Command {
        command: CommandId::Eraser,
    };

    #[test]
    fn legacy_workspace_defaults_and_customized_state_roundtrip() {
        let original = WorkspaceState::default();
        let mut legacy = serde_json::to_value(&original).unwrap();
        legacy["layout"].as_object_mut().unwrap().remove("panels");
        legacy["layout"]
            .as_object_mut()
            .unwrap()
            .remove("next_tile_id");
        let restored: WorkspaceState = serde_json::from_value(legacy).unwrap();
        assert_eq!(restored, original);
        let mut state = original;
        let toolbar = state
            .layout
            .add_toolbar(8, "Painting", &[PEN, ERASE])
            .unwrap();
        state.layout.panel_mut(toolbar).unwrap().tab_style = TabStyle::Icon;
        state
            .layout
            .move_panel(
                VIEWPORT,
                toolbar,
                DockTarget::Edge {
                    edge: Edge::Bottom,
                    outer: false,
                },
            )
            .unwrap();
        state.validate().unwrap();
        let saved = serde_json::to_value(&state).unwrap();
        assert!(saved.to_string().contains("toolbar:"));
        assert!(!saved.to_string().contains("expanded"));
        assert_eq!(
            serde_json::from_value::<WorkspaceState>(saved).unwrap(),
            state
        );
        let contents = state.layout.panels.clone();
        state.layout.reset_docking();
        assert_eq!(state.layout.panels, contents);
        state.validate().unwrap();
        state.layout.add_toolbar(2, "Second", &[PEN]).unwrap();
        state.validate().unwrap();
    }

    #[test]
    fn toolbar_names_and_invalid_targets_fail_atomically() {
        let mut layout = DockLayout::default();
        layout.add_toolbar(8, "  Favorites  ", &[PEN]).unwrap();
        for name in ["favorites", "FAVORITES", " ", "Layers", "a\nb"] {
            let before = layout.clone();
            assert!(layout.add_toolbar(8, name, &[PEN]).is_err(), "{name:?}");
            assert_eq!(layout, before);
        }
        let before = layout.clone();
        assert!(layout.add_toolbar(999, "Other", &[PEN]).is_err());
        assert!(layout.add_toolbar(8, "Other", &[]).is_err());
        assert!(
            layout
                .insert_tools(Panel::Toolbar, Some(999), &[PEN])
                .is_err()
        );
        assert!(layout.insert_tools(Panel::Layers, None, &[PEN]).is_err());
        assert!(
            layout
                .insert_tools(
                    Panel::Toolbar,
                    None,
                    &[ToolbarControl::Brush { id: u32::MAX }]
                )
                .is_err()
        );
        assert_eq!(layout, before);
    }

    #[test]
    fn tile_moves_preserve_identity_and_order_and_allow_empty_ribbons() {
        let mut layout = DockLayout::default();
        let custom = layout.add_toolbar(8, "Paint", &[PEN, ERASE, PEN]).unwrap();
        let ids = layout
            .panel(custom)
            .unwrap()
            .tiles()
            .iter()
            .map(|t| t.id)
            .collect::<Vec<_>>();
        let move_to = |layout: &mut DockLayout, panel, tile, destination, before| {
            layout
                .move_item(
                    VIEWPORT,
                    DockItem::Tile { panel, tile },
                    DockTarget::Tile {
                        panel: destination,
                        before,
                    },
                )
                .unwrap()
        };
        move_to(&mut layout, custom, ids[0], custom, None);
        assert_eq!(
            layout
                .panel(custom)
                .unwrap()
                .tiles()
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>(),
            [ids[1], ids[2], ids[0]]
        );
        move_to(&mut layout, custom, ids[0], custom, Some(ids[1]));
        move_to(&mut layout, custom, ids[0], custom, Some(ids[0]));
        for id in &ids {
            move_to(&mut layout, custom, *id, Panel::Toolbar, None);
        }
        assert!(layout.panel(custom).unwrap().tiles().is_empty());
        assert_eq!(
            &layout.panel(Panel::Toolbar).unwrap().tiles()[6..]
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>(),
            &ids
        );
        layout.validate().unwrap();
        let before = layout.clone();
        assert!(
            layout
                .move_item(
                    VIEWPORT,
                    DockItem::Tile {
                        panel: custom,
                        tile: ids[0]
                    },
                    DockTarget::Tile {
                        panel: Panel::Toolbar,
                        before: None
                    }
                )
                .is_err()
        );
        assert_eq!(layout, before);
    }

    #[test]
    fn invalid_saved_registry_references_and_ids_are_rejected() {
        let original = serde_json::to_value(WorkspaceState::default()).unwrap();
        for path in [
            "duplicate_tile",
            "missing_panel",
            "wrong_control",
            "next_tile_id",
        ] {
            let mut bad = original.clone();
            match path {
                "duplicate_tile" => {
                    bad["layout"]["panels"][0]["content"]["tiles"][1]["id"] = serde_json::json!(1)
                }
                "missing_panel" => {
                    bad["layout"]["panels"].as_array_mut().unwrap().pop();
                }
                "wrong_control" => {
                    bad["layout"]["panels"][1]["content"]["visible"] =
                        serde_json::json!(["layer_opacity"])
                }
                _ => bad["layout"]["next_tile_id"] = serde_json::json!(1),
            }
            let state: WorkspaceState = serde_json::from_value(bad).unwrap();
            assert!(state.validate().is_err(), "{path}");
        }
        for id in [
            "toolbar:0",
            "toolbar:01",
            "toolbar:-1",
            "toolbar:4294967296",
        ] {
            assert!(Panel::try_from(id.to_string()).is_err());
        }
    }

    #[test]
    fn picker_is_transactional_search_preserves_selection_and_creation_is_named() {
        let mut layout = DockLayout::default();
        let mut state = CustomizationState::default();
        let edit = |state: &mut CustomizationState, layout: &mut DockLayout, action| {
            state.edit(layout, action, Platform::Gtk).unwrap()
        };
        let original = layout.clone();
        edit(
            &mut state,
            &mut layout,
            CustomizationAction::NewToolbar { group: 8 },
        );
        edit(
            &mut state,
            &mut layout,
            CustomizationAction::PickerSelect {
                control: PEN,
                selected: true,
            },
        );
        edit(
            &mut state,
            &mut layout,
            CustomizationAction::PickerName {
                name: "Tools".into(),
            },
        );
        assert!(
            !state
                .picker
                .as_ref()
                .unwrap()
                .view(&layout, Platform::Gtk)
                .can_confirm
        );
        edit(&mut state, &mut layout, CustomizationAction::ConfirmTools);
        assert!(state.picker.as_ref().unwrap().error.is_some());
        assert_eq!(layout, original);
        edit(
            &mut state,
            &mut layout,
            CustomizationAction::PickerSearch {
                query: "watercolor".into(),
            },
        );
        let view = state.picker.as_ref().unwrap().view(&layout, Platform::Gtk);
        assert_eq!(view.selected_count, 1);
        assert_eq!(view.choices.len(), 2);
        edit(
            &mut state,
            &mut layout,
            CustomizationAction::PickerName {
                name: "Painting".into(),
            },
        );
        edit(&mut state, &mut layout, CustomizationAction::ConfirmTools);
        assert!(state.picker.is_none());
        assert_eq!(layout.panels.len(), 5);
        assert_eq!(layout.panels.last().unwrap().tiles()[0].control, PEN);
        let before = layout.clone();
        edit(
            &mut state,
            &mut layout,
            CustomizationAction::InsertTools {
                panel: Panel::Toolbar,
                before: None,
            },
        );
        edit(
            &mut state,
            &mut layout,
            CustomizationAction::PickerSelect {
                control: ERASE,
                selected: true,
            },
        );
        edit(&mut state, &mut layout, CustomizationAction::CancelTools);
        assert_eq!(layout, before);
    }

    #[test]
    fn context_targets_offer_only_their_actions_and_group_styles_can_be_overridden() {
        let mut layout = DockLayout::default();
        let custom = layout.add_toolbar(8, "Paint", &[PEN]).unwrap();
        let mut state = CustomizationState::default();
        let group = ContextTarget::Group { group: 8 };
        state
            .edit(
                &mut layout,
                CustomizationAction::SetTabStyle {
                    target: group,
                    style: TabStyle::Icon,
                },
                Platform::Gtk,
            )
            .unwrap();
        assert_eq!(
            layout.panel(Panel::Layers).unwrap().tab_style,
            TabStyle::Icon
        );
        assert_eq!(layout.panel(custom).unwrap().tab_style, TabStyle::Icon);
        state
            .edit(
                &mut layout,
                CustomizationAction::SetTabStyle {
                    target: ContextTarget::Panel { panel: custom },
                    style: TabStyle::Name,
                },
                Platform::Gtk,
            )
            .unwrap();
        assert!(
            layout.context_menu(group).unwrap().sections[0]
                .iter()
                .all(|i| i.selected == Some(false))
        );
        let panel = layout
            .context_menu(ContextTarget::Panel {
                panel: Panel::Brushes,
            })
            .unwrap();
        assert_eq!(panel.sections[1][0].label, "Configure Panel…");
        let tile = layout.panel(custom).unwrap().tiles()[0].id;
        assert_eq!(
            layout
                .context_menu(ContextTarget::Tile {
                    panel: custom,
                    tile
                })
                .unwrap()
                .sections[0]
                .iter()
                .map(|i| i.label)
                .collect::<Vec<_>>(),
            ["Remove Tool", "Insert Tools…"]
        );
        assert_eq!(
            layout
                .context_menu(ContextTarget::Ribbon { panel: custom })
                .unwrap()
                .sections[0][0]
                .label,
            "Add Tools…"
        );
        assert!(
            layout
                .context_menu(ContextTarget::Ribbon {
                    panel: Panel::Layers
                })
                .is_err()
        );
    }

    #[test]
    fn catalog_is_platform_filtered_and_all_controls_execute_typed_actions() {
        let native = tool_catalog(Platform::Gtk);
        let web = tool_catalog(Platform::Web);
        assert_eq!(native.len(), web.len() + 1);
        assert!(native.iter().all(|c| !c.label.is_empty()
            && !c.description.is_empty()
            && ui_catalog().icons.contains(&c.icon)));
        for choice in native {
            choice.control.validate().unwrap();
            let action = choice.control.action();
            assert_eq!(
                serde_json::from_value::<UiAction>(serde_json::to_value(&action).unwrap()).unwrap(),
                action
            );
        }
    }

    #[test]
    fn dynamic_ribbons_wrap_and_every_insertion_marker_maps_to_its_slot() {
        for edge in [Edge::Top, Edge::Left] {
            for count in [0, 1, 7, 30] {
                let mut layout = DockLayout::default();
                let custom = layout.add_toolbar(8, "Many", &[PEN]).unwrap();
                let tile = layout.panel(custom).unwrap().tiles()[0].id;
                layout.remove_tool(custom, tile).unwrap();
                if count > 0 {
                    layout
                        .insert_tools(custom, None, &vec![PEN; count])
                        .unwrap();
                }
                layout
                    .move_panel(VIEWPORT, custom, DockTarget::Edge { edge, outer: false })
                    .unwrap();
                let resolved = layout.workspace(900.0, 900.0, HEADER_HEIGHT, STATUS_HEIGHT);
                let g = resolved.groups.iter().find(|g| g.active == custom).unwrap();
                let tiles = g.tiles.as_ref().unwrap();
                assert_eq!(tiles.tiles.len(), count);
                assert_eq!(tiles.insertion.len(), count + 1);
                for (index, line) in tiles.insertion.iter().enumerate() {
                    let point = [
                        g.bounds.x + line.x + line.width * 0.5,
                        g.bounds.y + line.y + line.height * 0.5,
                    ];
                    let hint = resolved.tile_drop_hint(point, &layout).unwrap_or_else(|| {
                        panic!(
                            "{edge:?}, {count} tiles, slot {index}: {point:?}, {:?}",
                            g.bounds
                        )
                    });
                    assert_eq!(
                        hint.target,
                        DockTarget::Tile {
                            panel: custom,
                            before: layout
                                .panel(custom)
                                .unwrap()
                                .tiles()
                                .get(index)
                                .map(|t| t.id)
                        }
                    );
                }
            }
        }
    }
}
