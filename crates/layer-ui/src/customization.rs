//! Panel configuration and contextual editing. Durable data lives in the dock
//! layout; open menus, inspectors and picker drafts belong to the UI session.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabStyle {
    #[default]
    Automatic,
    ActiveName,
    IconName,
    Name,
    Icon,
}
impl TabStyle {
    pub const ALL: [Self; 5] = [
        Self::Automatic,
        Self::ActiveName,
        Self::IconName,
        Self::Name,
        Self::Icon,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::ActiveName => "Icons and active tab name",
            Self::IconName => "Icons and names",
            Self::Name => "Names only",
            Self::Icon => "Icons only",
        }
    }
    fn presentation(self, active: bool, tab_count: usize) -> TabPresentation {
        TabPresentation {
            show_icon: self != Self::Name,
            show_name: matches!(self, Self::Name | Self::IconName)
                || (self == Self::Automatic && (tab_count <= 2 || active))
                || (self == Self::ActiveName && active),
        }
    }
}

/// Resolved in Rust from the group's style and selection; hosts only render it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct TabPresentation {
    pub show_icon: bool,
    pub show_name: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TileStyle {
    #[default]
    Small,
    Large,
    Labeled,
}
impl TileStyle {
    pub fn size(self) -> [f32; 2] {
        let [w, h] = match self {
            Self::Small => [1.0, 1.0],
            Self::Large => [2.0, 2.0],
            Self::Labeled => [3.0, 2.0],
        };
        [w * TILE_SIZE, h * TILE_SIZE]
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Small Tiles",
            Self::Large => "Large Tiles",
            Self::Labeled => "Labeled Tiles",
        }
    }
    pub fn icon_size(self) -> u32 {
        if self == Self::Large { 32 } else { 16 }
    }
    pub(crate) fn floating_width(self) -> f32 {
        let columns = if self == Self::Labeled { 2.0 } else { 3.0 };
        columns * (self.size()[0] + 2.0) - 2.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelControl {
    Brushes,
    ToolSettings,
    ColorWheel,
    BrushSize,
    SizePresets,
    BrushOpacity,
    BrushColor,
    Layers,
    LayerActions,
    LayerOpacity,
    Adjustments,
    Properties,
    Stats,
    Navigator,
}
impl PanelControl {
    pub fn label(self) -> &'static str {
        match self {
            Self::Brushes => "Tool Set",
            Self::ToolSettings => "Tool Settings",
            Self::ColorWheel => "Color wheel",
            Self::BrushSize => "Brush size",
            Self::SizePresets => "Size presets",
            Self::BrushOpacity => "Brush opacity",
            Self::BrushColor => "Brush color",
            Self::Layers => "Layers",
            Self::LayerActions => "Layer actions",
            Self::LayerOpacity => "Layer opacity",
            Self::Adjustments => "Filters",
            Self::Properties => "Properties",
            Self::Stats => "Diagnostics",
            Self::Navigator => "Navigator",
        }
    }
    pub fn available(panel: Panel) -> &'static [Self] {
        match panel {
            Panel::ToolSettings => &[Self::ToolSettings],
            Panel::Color => &[Self::ColorWheel],
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
            Panel::Adjustments => &[Self::Adjustments],
            Panel::Properties => &[Self::Properties],
            Panel::Stats => &[Self::Stats],
            Panel::Navigator => &[Self::Navigator],
            _ => &[],
        }
    }
    fn defaults(panel: Panel) -> &'static [Self] {
        match panel {
            Panel::Brushes => &[Self::Brushes],
            Panel::Sizes => &[Self::BrushSize, Self::SizePresets],
            Panel::Layers
            | Panel::Adjustments
            | Panel::Properties
            | Panel::Stats
            | Panel::Navigator
            | Panel::ToolSettings
            | Panel::Color => Self::available(panel),
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
    pub hide_tab: bool,
    #[serde(default)]
    pub tile_style: TileStyle,
    pub content: PanelContent,
}
impl PanelConfig {
    pub fn icon(&self) -> &'static str {
        self.tiles()
            .first()
            .map(|t| tool_choice(t.control).icon)
            .unwrap_or(self.id.icon())
    }
    pub fn title(&self) -> &str {
        match &self.content {
            PanelContent::Toolbar { name, .. } => name,
            PanelContent::Controls { .. } => self.id.label(),
        }
    }
    fn menu_name(&self) -> String {
        format!(
            "{} {}",
            self.title(),
            if self.id.kind() == PanelKind::Content {
                "panel"
            } else {
                "toolbar"
            }
        )
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
                hide_tab: false,
                tile_style: TileStyle::Small,
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
    /// Immediate action, if any. Dedicated panel tiles are opened through
    /// ActivateTile, whose identity also anchors their drawer.
    pub fn action(self) -> Option<UiAction> {
        Some(match self {
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
            Self::Panel { .. } | Self::Divider => return None,
        })
    }
    pub fn validate(self) -> Result<(), String> {
        match self {
            Self::Brush { id } => {
                preset(id)?;
            }
            Self::Size { pixels } => {
                NumericControl::brush_size().validate(pixels as f32, "Brush size")?;
            }
            Self::Panel { panel } if panel.kind() != PanelKind::Content => {
                return Err("Choose a built-in panel".into());
            }
            _ => (),
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContextTarget {
    ZenMode,
    Panel { panel: Panel },
    Group { group: u32 },
    Tile { panel: Panel, tile: u32 },
    Ribbon { panel: Panel },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CustomizationAction {
    SetPanelVisible {
        panel: Panel,
        visible: bool,
    },
    AddPanel {
        panel: Panel,
        group: u32,
    },
    SetTileStyle {
        panel: Panel,
        style: TileStyle,
    },
    RenameToolbar {
        panel: Panel,
    },
    DuplicateToolbar {
        panel: Panel,
    },
    DeleteToolbar {
        panel: Panel,
    },
    ManageToolbars,
    SelectManagedToolbar {
        panel: Option<Panel>,
    },
    CloseToolbarManager,
    ToolbarName {
        name: String,
    },
    ConfirmToolbar,
    CancelToolbar,
    SetTabStyle {
        group: u32,
        style: TabStyle,
    },
    SetTabHidden {
        panel: Panel,
        hidden: bool,
    },
    ShowAllControls {
        panel: Panel,
    },
    ToggleToolDrawer {
        anchor: TileAnchor,
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
        group: Option<u32>,
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
    pub label: String,
    /// None is an ordinary command; a value is a radio-style choice.
    pub selected: Option<bool>,
    pub action: Option<UiAction>,
    pub enabled: bool,
    pub hint: String,
    pub sections: Vec<Vec<ContextMenuItem>>,
}
impl ContextMenuItem {
    pub fn command(label: impl Into<String>, action: UiAction) -> Self {
        Self {
            label: label.into(),
            selected: None,
            action: Some(action),
            enabled: true,
            hint: String::new(),
            sections: Vec::new(),
        }
    }
    fn edit(label: impl Into<String>, action: CustomizationAction) -> Self {
        Self::command(label, UiAction::Customize { action })
    }
    pub(crate) fn submenu(label: &str, sections: Vec<Vec<Self>>) -> Self {
        Self {
            label: label.into(),
            selected: None,
            action: None,
            enabled: sections.iter().any(|s| !s.is_empty()),
            hint: String::new(),
            sections,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct ContextMenu {
    pub title: String,
    pub sections: Vec<Vec<ContextMenuItem>>,
}
impl ContextMenu {
    pub(crate) fn with_shortcuts(mut self, settings: &Settings, platform: Platform) -> Self {
        fn visit(sections: &mut [Vec<ContextMenuItem>], settings: &Settings, platform: Platform) {
            for item in sections.iter_mut().flatten() {
                if let Some(action) = &item.action {
                    let shortcut = settings.action_shortcut(action, platform);
                    if !shortcut.is_empty() && item.hint != shortcut {
                        item.hint = if item.hint.is_empty() {
                            shortcut
                        } else {
                            format!("{} · {shortcut}", item.hint)
                        };
                    }
                }
                visit(&mut item.sections, settings, platform);
            }
        }
        visit(&mut self.sections, settings, platform);
        self
    }
}
impl DockLayout {
    pub fn context_menu(&self, target: ContextTarget) -> Result<ContextMenu, String> {
        self.context_menu_on(target, Platform::Generic)
    }
    pub(crate) fn context_menu_on(
        &self,
        target: ContextTarget,
        platform: Platform,
    ) -> Result<ContextMenu, String> {
        let entry = ContextMenuItem::edit;
        let (title, sections) = match target {
            ContextTarget::ZenMode => return Err("Not a panel context".into()),
            ContextTarget::Panel { panel } => {
                let p = self.panel(panel)?;
                (
                    p.menu_name(),
                    vec![self.hide_tab_items(target)?, self.panel_actions(p)],
                )
            }
            ContextTarget::Group { group } => (
                "Panel Group".into(),
                vec![
                    self.tab_style_items(group)?,
                    self.hide_tab_items(target)?,
                    if let [panel] = self.group_panels(group)?
                        && let p = self.panel(*panel)?
                        && panel.kind() == PanelKind::Content
                        && p.hide_tab
                    {
                        self.panel_actions(p)
                    } else {
                        Vec::new()
                    },
                    vec![
                        ContextMenuItem::submenu(
                            "Add built-in panel",
                            vec![self.panel_items(PanelKind::Content, Some(group), platform)],
                        ),
                        ContextMenuItem::submenu(
                            "Add Toolbar",
                            vec![self.panel_items(PanelKind::Tiles, Some(group), platform)],
                        ),
                    ],
                    vec![entry(
                        "New Toolbar…",
                        CustomizationAction::NewToolbar { group: Some(group) },
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
                (p.menu_name(), self.toolbar_options(panel)?)
            }
        };
        Ok(ContextMenu { title, sections })
    }
    fn panel_actions(&self, panel: &PanelConfig) -> Vec<ContextMenuItem> {
        vec![
            ContextMenuItem::edit(
                format!("Configure {}…", panel.menu_name()),
                CustomizationAction::ShowAllControls { panel: panel.id },
            ),
            self.hide_item(panel.id),
        ]
    }
    fn hide_item(&self, panel: Panel) -> ContextMenuItem {
        ContextMenuItem::edit(
            format!(
                "Hide {}",
                self.panel(panel).expect("validated panel").menu_name()
            ),
            CustomizationAction::SetPanelVisible {
                panel,
                visible: false,
            },
        )
    }
    pub fn panel_items(
        &self,
        kind: PanelKind,
        group: Option<u32>,
        platform: Platform,
    ) -> Vec<ContextMenuItem> {
        self.panels
            .iter()
            .filter(|p| p.id.kind() == kind && p.id.available_on(platform))
            .map(|p| {
                let selected = if let Some(group) = group {
                    self.panel_group(p.id) == Some(group)
                } else {
                    self.panel_group(p.id).is_some()
                };
                let action = if let Some(group) = group {
                    CustomizationAction::AddPanel { panel: p.id, group }
                } else {
                    CustomizationAction::SetPanelVisible {
                        panel: p.id,
                        visible: !selected,
                    }
                };
                let mut item = ContextMenuItem::edit(p.menu_name(), action);
                item.selected = Some(selected);
                item.enabled = selected
                    || group.is_none_or(|g| {
                        !matches!(self.group_edge(g), Some(Edge::Top | Edge::Bottom))
                    });
                item
            })
            .collect()
    }
    pub fn toolbar_options(&self, panel: Panel) -> Result<Vec<Vec<ContextMenuItem>>, String> {
        let p = self.panel(panel)?;
        if panel.kind() != PanelKind::Tiles {
            return Err("Choose a toolbar".into());
        }
        let name = p.menu_name();
        Ok(vec![
            vec![
                ContextMenuItem::edit(
                    format!("Configure {name}…"),
                    CustomizationAction::ShowAllControls { panel },
                ),
                ContextMenuItem::edit(
                    "Add Tools…",
                    CustomizationAction::InsertTools {
                        panel,
                        before: None,
                    },
                ),
            ],
            [TileStyle::Small, TileStyle::Large, TileStyle::Labeled]
                .into_iter()
                .map(|style| {
                    let mut item = ContextMenuItem::edit(
                        style.label(),
                        CustomizationAction::SetTileStyle { panel, style },
                    );
                    item.selected = Some(p.tile_style == style);
                    item
                })
                .collect(),
            vec![
                ContextMenuItem::edit(
                    format!("Rename {name}…"),
                    CustomizationAction::RenameToolbar { panel },
                ),
                ContextMenuItem::edit(
                    format!("Duplicate {name}…"),
                    CustomizationAction::DuplicateToolbar { panel },
                ),
            ],
            vec![self.hide_item(panel)],
        ])
    }
    fn tab_style_items(&self, group: u32) -> Result<Vec<ContextMenuItem>, String> {
        let selected = self.group_tab_style(group)?;
        Ok(TabStyle::ALL
            .into_iter()
            .map(|style| {
                let mut item = ContextMenuItem::edit(
                    style.label(),
                    CustomizationAction::SetTabStyle { group, style },
                );
                item.selected = Some(selected == style);
                item
            })
            .collect())
    }
    pub fn tab_presentation(&self, panel: Panel) -> TabPresentation {
        let Some(group) = self.panel_group(panel) else {
            return TabStyle::default().presentation(true, 1);
        };
        let DockNode::Tabs {
            active,
            tab_style,
            panels,
            ..
        } = self.node(group).unwrap()
        else {
            unreachable!()
        };
        tab_style.presentation(*active == panel, panels.len())
    }
    fn hide_tab_items(&self, target: ContextTarget) -> Result<Vec<ContextMenuItem>, String> {
        let group = match target {
            ContextTarget::Panel { panel } => self.panel_group(panel),
            ContextTarget::Group { group } => Some(group),
            _ => None,
        };
        let Some(group) = group else {
            return Ok(Vec::new());
        };
        let [panel] = self.group_panels(group)? else {
            return Ok(Vec::new());
        };
        if panel.kind() != PanelKind::Content {
            return Ok(Vec::new());
        }
        let hidden = self.panel(*panel)?.hide_tab;
        let mut item = ContextMenuItem::edit(
            "Show tab bar",
            CustomizationAction::SetTabHidden {
                panel: *panel,
                hidden: !hidden,
            },
        );
        item.selected = Some(!hidden);
        Ok(vec![item])
    }
    pub fn validate_toolbar_name(&self, name: &str) -> Result<(), String> {
        self.check_toolbar_name(name, None)
    }
    pub(crate) fn check_toolbar_name(
        &self,
        name: &str,
        except: Option<Panel>,
    ) -> Result<(), String> {
        validate_toolbar_name(name.trim())?;
        if self
            .panels
            .iter()
            .any(|p| Some(p.id) != except && p.title().to_lowercase() == name.trim().to_lowercase())
        {
            return Err("A panel already uses this name".into());
        }
        Ok(())
    }
    fn unused_toolbar_name(&self, stem: &str) -> String {
        if self.validate_toolbar_name(stem).is_ok() {
            return stem.into();
        }
        for suffix in 2u32.. {
            let name = format!("{} {suffix}", stem.chars().take(52).collect::<String>());
            if self.validate_toolbar_name(&name).is_ok() {
                return name;
            }
        }
        unreachable!("finite registry cannot exhaust names")
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
                CommandId::Pen => "Draw ink lines with a pen",
                CommandId::Pencil => "Sketch with pencils and pastels",
                CommandId::Brush => "Paint with the current brush",
                CommandId::Eraser => "Erase paint from the active layer",
                CommandId::Airbrush => "Build up soft color or spray",
                CommandId::Decoration => "Paint with textured stamps",
                CommandId::Blend => "Mix and smear existing paint",
                CommandId::Liquify => "Push and twist existing paint",
                CommandId::Lasso => "Draw a freehand selection",
                CommandId::Move => "Move the editing layer or its mask",
                CommandId::Hand => "Drag to move the canvas view",
                CommandId::Eyedropper => "Pick a color from the canvas",
                CommandId::Undo => "Undo the last change",
                CommandId::Redo => "Restore the last undone change",
                CommandId::UndoWorkspace => "Undo the last workspace change",
                CommandId::RedoWorkspace => "Restore the last undone workspace change",
                CommandId::NewToolbar => "Create a named toolbar",
                CommandId::ManageToolbars => "Select and delete toolbars",
                CommandId::FitCanvas => "Fit the whole drawing in the available space",
                CommandId::Settings => "Open application preferences",
                CommandId::ToggleTheme => "Switch between light and dark appearance",
                CommandId::AddLayer => "Create a new paint layer",
                CommandId::DeleteLayer => "Delete the active paint layer",
                CommandId::RaiseLayer => "Move the active layer up",
                CommandId::LowerLayer => "Move the active layer down",
                CommandId::ResetLayout => "Restore panel docking positions",
                CommandId::ZenMode => "Hide or show the editor controls",
                CommandId::NewWindow => "Open another drawing window",
                CommandId::KeyboardShortcuts => "Customize application shortcuts",
                CommandId::About => "Application information and links",
                CommandId::ZoomIn | CommandId::ZoomOut => "Change the canvas viewing scale",
                CommandId::RotateLeft | CommandId::RotateRight => {
                    "Rotate the view without changing the image"
                }
                CommandId::FlipHorizontal | CommandId::FlipVertical => {
                    "Mirror the view without changing the image"
                }
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
        ToolbarControl::Panel { panel } => (
            format!("{} panel", panel.label()),
            "Open this panel in a drawer".into(),
            panel.icon(),
        ),
        ToolbarControl::Divider => (
            "Divider".into(),
            "Separate groups of toolbar items".into(),
            "minus",
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
        .filter(|id| id.available_on(platform))
        .map(|command| ToolbarControl::Command { command })
        .chain([ToolbarControl::Color, ToolbarControl::Opacity])
        .chain(
            matches!(platform, Platform::Gtk | Platform::Generic)
                .then_some(ToolbarControl::Divider),
        )
        .chain(
            Panel::ALL
                .into_iter()
                .filter(move |p| {
                    p.kind() == PanelKind::Content
                        && p.available_on(platform)
                        && matches!(platform, Platform::Gtk | Platform::Generic)
                })
                .map(|panel| ToolbarControl::Panel { panel }),
        )
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
    NewToolbar { group: Option<u32>, name: String },
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
    pub tooltip: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct PanelView {
    pub id: Panel,
    pub title: String,
    pub icon: &'static str,
    pub tab: TabPresentation,
    pub tile_style: TileStyle,
    pub toolbar_options: Vec<Vec<ContextMenuItem>>,
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
        .filter(|_| panel.available_on(state.platform))
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
                ToolbarControl::Brush { id } => {
                    state.brush.preset == id && state.layer_tools.tool == LayerCanvasTool::Paint
                }
                ToolbarControl::Size { pixels } => {
                    (state.brush.diameter - pixels as f32).abs() < 0.01
                }
                ToolbarControl::Panel { panel } => {
                    enabled = panel.available_on(state.platform)
                        && matches!(state.platform, Platform::Gtk | Platform::Generic);
                    false
                }
                _ => false,
            };
            TileView {
                id: tile.id,
                tooltip: tile.control.action().map_or_else(
                    || choice.label.clone(),
                    |action| {
                        state
                            .settings
                            .action_tooltip(&choice.label, &action, state.platform)
                    },
                ),
                choice,
                enabled,
            }
        })
        .collect();
    Ok(PanelView {
        id: panel,
        title: config.title().into(),
        icon: config.icon(),
        tab: state.workspace.layout.tab_presentation(panel),
        tile_style: config.tile_style,
        toolbar_options: if panel.kind() == PanelKind::Tiles {
            let mut options = state.workspace.layout.toolbar_options(panel)?;
            options[0].remove(0); // The configuration column is already open.
            ContextMenu {
                title: String::new(),
                sections: options,
            }
            .with_shortcuts(&state.settings, state.platform)
            .sections
        } else {
            Vec::new()
        },
        expanded,
        configuration_title: format!("Configure {}", config.menu_name()),
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
                if let Some(group) = group {
                    layout.group_panels(*group)?;
                }
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

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum ToolbarOperation {
    Rename,
    Duplicate,
    Delete,
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolbarPrompt {
    panel: Panel,
    operation: ToolbarOperation,
    name: String,
    error: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ToolbarPromptView {
    pub title: &'static str,
    pub message: String,
    pub name: Option<String>,
    pub name_label: &'static str,
    pub confirm_label: &'static str,
    pub cancel_label: &'static str,
    pub destructive: bool,
    pub can_confirm: bool,
    pub error: Option<String>,
}
impl ToolbarPrompt {
    fn validate(&self, layout: &DockLayout) -> Result<(), String> {
        layout.panel(self.panel)?;
        if self.panel.kind() != PanelKind::Tiles {
            return Err("Choose a toolbar".into());
        }
        match self.operation {
            ToolbarOperation::Rename => layout.check_toolbar_name(&self.name, Some(self.panel)),
            ToolbarOperation::Duplicate => layout.validate_toolbar_name(&self.name),
            ToolbarOperation::Delete => Ok(()),
        }
    }
    pub fn view(&self, layout: &DockLayout, undo_shortcut: &str) -> ToolbarPromptView {
        let (title, confirm_label, destructive) = match self.operation {
            ToolbarOperation::Rename => ("Rename Toolbar", "Rename", false),
            ToolbarOperation::Duplicate => ("Duplicate Toolbar", "Duplicate", false),
            ToolbarOperation::Delete => ("Delete Toolbar?", "Delete Toolbar", true),
        };
        let shortcut = if undo_shortcut.is_empty() {
            String::new()
        } else {
            format!(" ({undo_shortcut})")
        };
        ToolbarPromptView {
            title,
            confirm_label,
            destructive,
            cancel_label: "Cancel",
            name_label: "Toolbar name",
            message: if destructive {
                format!(
                    "Delete “{}” and its tools, not just hide it? You can restore it with Workspace → Undo Workspace Change{shortcut}.",
                    self.name
                )
            } else {
                String::new()
            },
            name: (!destructive).then(|| self.name.clone()),
            can_confirm: self.validate(layout).is_ok(),
            error: self.error.clone().or_else(|| self.validate(layout).err()),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ToolbarManager {
    selected: Option<Panel>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ManagedToolbar {
    pub panel: Panel,
    pub title: String,
    pub subtitle: String,
    pub icon: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolbarManagerView {
    pub title: &'static str,
    pub close_label: &'static str,
    pub description: &'static str,
    pub empty_label: &'static str,
    pub toolbars: Vec<ManagedToolbar>,
    pub selected: Option<Panel>,
    pub delete_label: &'static str,
    pub delete_action: Option<CustomizationAction>,
}
impl ToolbarManager {
    pub fn view(&self, layout: &DockLayout) -> ToolbarManagerView {
        let toolbars: Vec<_> = layout
            .panels
            .iter()
            .filter(|p| p.id.kind() == PanelKind::Tiles)
            .map(|p| {
                let count = p.tiles().len();
                let unit = if count == 1 { "tool" } else { "tools" };
                let visibility = if layout.panel_group(p.id).is_some() {
                    "Visible"
                } else {
                    "Hidden"
                };
                ManagedToolbar {
                    panel: p.id,
                    title: p.title().into(),
                    subtitle: format!("{count} {unit} · {visibility}"),
                    icon: p.icon(),
                }
            })
            .collect();
        let selected = self
            .selected
            .filter(|id| toolbars.iter().any(|p| p.panel == *id));
        ToolbarManagerView {
            title: "Manage Toolbars",
            close_label: "Close",
            description: "Select a toolbar to delete.",
            empty_label: "No toolbars",
            toolbars,
            selected,
            delete_label: "Delete Toolbar…",
            delete_action: selected.map(|panel| CustomizationAction::DeleteToolbar { panel }),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CustomizationState {
    pub expanded: Option<Panel>,
    pub drawer: Option<ContentDrawer>,
    pub picker: Option<ToolPicker>,
    pub control: Option<PanelControl>,
    pub toolbar_prompt: Option<ToolbarPrompt>,
    pub toolbar_manager: Option<ToolbarManager>,
}
impl CustomizationState {
    pub fn has_drawer(&self) -> bool {
        self.expanded.is_some() || self.drawer.is_some()
    }
    pub fn is_open(&self) -> bool {
        self.drawer.is_some() || self.blocks_shortcuts()
    }
    pub(crate) fn blocks_shortcuts(&self) -> bool {
        self.expanded.is_some()
            || self.picker.is_some()
            || self.control.is_some()
            || self.toolbar_prompt.is_some()
            || self.toolbar_manager.is_some()
    }
    pub(crate) fn edit(
        &mut self,
        layout: &mut DockLayout,
        action: CustomizationAction,
        platform: Platform,
        viewport: [f32; 2],
        partial_zen: bool,
    ) -> Result<u32, String> {
        use CustomizationAction::*;
        if let SetPanelVisible {
            panel,
            visible: true,
        }
        | AddPanel { panel, .. }
        | ShowAllControls { panel } = &action
            && !panel.available_on(platform)
        {
            return Err("This panel is not available on this platform yet".into());
        }
        let mut changed = regions::CUSTOMIZATION;
        if matches!(
            action,
            NewToolbar { .. }
                | InsertTools { .. }
                | OpenControl { .. }
                | RenameToolbar { .. }
                | DuplicateToolbar { .. }
                | DeleteToolbar { .. }
        ) {
            self.drawer = None;
        }
        match action {
            ManageToolbars => {
                *self = Self {
                    toolbar_manager: Some(ToolbarManager::default()),
                    ..Self::default()
                };
            }
            SelectManagedToolbar { panel } => {
                if let Some(panel) = panel {
                    layout.panel(panel)?;
                    if panel.kind() != PanelKind::Tiles {
                        return Err("Choose a toolbar".into());
                    }
                }
                self.toolbar_manager
                    .as_mut()
                    .ok_or("The toolbar manager is closed")?
                    .selected = panel;
            }
            CloseToolbarManager => {
                if self.toolbar_manager.is_some() {
                    *self = Self::default();
                }
            }
            SetPanelVisible { panel, visible } => {
                layout.set_panel_visible(panel, visible)?;
                changed |= regions::LAYOUT;
            }
            AddPanel { panel, group } => {
                layout.add_panel_to_group(panel, group)?;
                changed |= regions::LAYOUT;
            }
            SetTileStyle { panel, style } => {
                layout.set_tile_style(panel, style, viewport)?;
                changed |= regions::LAYOUT;
            }
            RenameToolbar { panel } | DuplicateToolbar { panel } | DeleteToolbar { panel } => {
                if panel.kind() != PanelKind::Tiles {
                    return Err("Choose a toolbar".into());
                }
                let operation = match action {
                    RenameToolbar { .. } => ToolbarOperation::Rename,
                    DuplicateToolbar { .. } => ToolbarOperation::Duplicate,
                    _ => ToolbarOperation::Delete,
                };
                let title = layout.panel(panel)?.title();
                let name = if matches!(operation, ToolbarOperation::Duplicate) {
                    layout.unused_toolbar_name(&format!(
                        "{} Copy",
                        title.chars().take(59).collect::<String>()
                    ))
                } else {
                    title.into()
                };
                self.picker = None;
                self.control = None;
                if !matches!(operation, ToolbarOperation::Delete) {
                    self.toolbar_manager = None;
                }
                self.toolbar_prompt = Some(ToolbarPrompt {
                    panel,
                    operation,
                    name,
                    error: None,
                });
            }
            ToolbarName { name } => {
                let draft = self
                    .toolbar_prompt
                    .as_mut()
                    .ok_or("The toolbar dialog is closed")?;
                if matches!(draft.operation, ToolbarOperation::Delete) {
                    return Err("This dialog does not edit a name".into());
                }
                draft.name = name;
                draft.error = None;
            }
            ConfirmToolbar => {
                let draft = self
                    .toolbar_prompt
                    .as_mut()
                    .ok_or("The toolbar dialog is closed")?;
                let result = draft.validate(layout).and_then(|_| match draft.operation {
                    ToolbarOperation::Rename => layout.rename_toolbar(draft.panel, &draft.name),
                    ToolbarOperation::Duplicate => layout
                        .duplicate_toolbar(draft.panel, &draft.name)
                        .map(|_| ()),
                    ToolbarOperation::Delete => layout.delete_toolbar(draft.panel),
                });
                match result {
                    Ok(()) => {
                        if matches!(draft.operation, ToolbarOperation::Delete)
                            && let Some(manager) = &mut self.toolbar_manager
                        {
                            manager.selected = None;
                        }
                        self.toolbar_prompt = None;
                        changed |= regions::LAYOUT;
                    }
                    Err(error) => draft.error = Some(error),
                }
            }
            CancelToolbar => self.toolbar_prompt = None,
            SetTabStyle { group, style } => {
                layout.set_tab_style(group, style)?;
                changed |= regions::LAYOUT;
            }
            SetTabHidden { panel, hidden } => {
                let group = layout.panel_group(panel).ok_or("Panel is not docked")?;
                if panel.kind() != PanelKind::Content || layout.group_panels(group)?.len() != 1 {
                    return Err("Only a lone built-in panel can hide its tab".into());
                }
                layout.panel_mut(panel)?.hide_tab = hidden;
                changed |= regions::LAYOUT;
            }
            ShowAllControls { panel } => {
                layout.panel(panel)?;
                let group = layout.panel_group(panel).ok_or("Panel is not docked")?;
                layout.select_tab(group, panel)?;
                self.picker = None;
                self.control = None;
                self.toolbar_manager = None;
                self.expanded = Some(panel);
                self.drawer = None;
                changed |= regions::LAYOUT;
            }
            ToggleToolDrawer { anchor } => {
                if !matches!(platform, Platform::Gtk | Platform::Generic) {
                    return Err("Tool drawers are not available on this platform yet".into());
                }
                let drawer = ContentDrawer::for_tile(layout, anchor)?;
                if drawer
                    .placement(
                        layout,
                        viewport,
                        &vec![0.0; drawer.columns.len()],
                        partial_zen,
                    )
                    .is_none()
                {
                    return Err("The originating tile is not visible".into());
                }
                let close = self.drawer.as_ref().is_some_and(|d| d.anchor == anchor);
                *self = Self {
                    drawer: (!close).then_some(drawer),
                    ..Self::default()
                };
            }
            CloseExpanded => {
                self.expanded = None;
                self.drawer = None;
            }
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
                    return Err("Choose a built-in panel".into());
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
                if let Some(group) = group {
                    layout.group_panels(group)?;
                }
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
                self.toolbar_manager = None;
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
                self.toolbar_manager = None;
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
            .add_toolbar(Some(8), "Painting", &[PEN, ERASE])
            .unwrap();
        state.layout.set_tab_style(8, TabStyle::Icon).unwrap();
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
        state.layout.reset_docking().unwrap();
        assert_eq!(state.layout.panels, contents);
        assert!(
            state
                .layout
                .bands
                .iter()
                .filter(|b| matches!(b.edge, Edge::Top | Edge::Bottom))
                .all(
                    |b| matches!(&b.root, crate::DockNode::Tabs { panels, .. } if panels.len() == 1)
                )
        );
        state.validate().unwrap();
        state.layout.add_toolbar(Some(8), "Second", &[PEN]).unwrap();
        state.validate().unwrap();
    }

    #[test]
    fn toolbar_names_and_invalid_targets_fail_atomically() {
        let mut layout = DockLayout::default();
        layout
            .add_toolbar(Some(8), "  Favorites  ", &[PEN])
            .unwrap();
        for name in ["favorites", "FAVORITES", " ", "Layers", "a\nb"] {
            let before = layout.clone();
            assert!(
                layout.add_toolbar(Some(8), name, &[PEN]).is_err(),
                "{name:?}"
            );
            assert_eq!(layout, before);
        }
        let before = layout.clone();
        assert!(layout.add_toolbar(Some(999), "Other", &[PEN]).is_err());
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
        let custom = layout
            .add_toolbar(Some(8), "Paint", &[PEN, ERASE, PEN])
            .unwrap();
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
            &layout.panel(Panel::Toolbar).unwrap().tiles()[TOOLBAR_CONTROLS.len()..]
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
                    bad["layout"]["panels"].as_array_mut().unwrap().remove(1);
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
            state
                .edit(layout, action, Platform::Gtk, [1200.0, 900.0], false)
                .unwrap()
        };
        let original = layout.clone();
        edit(
            &mut state,
            &mut layout,
            CustomizationAction::NewToolbar { group: Some(8) },
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
        assert_eq!(layout.panels.len(), Panel::ALL.len() + 1);
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
    fn automatic_tabs_follow_group_membership_without_changing_the_saved_style() {
        let mut layout = DockLayout::default();
        let check = |layout: &mut DockLayout| {
            assert_eq!(layout.group_tab_style(8).unwrap(), TabStyle::Automatic);
            let panels = layout.group_panels(8).unwrap().to_vec();
            for &active in &panels {
                layout.select_tab(8, active).unwrap();
                for &panel in &panels {
                    assert_eq!(
                        layout.tab_presentation(panel),
                        TabPresentation {
                            show_icon: true,
                            show_name: panels.len() <= 2 || panel == active,
                        }
                    );
                }
            }
        };
        check(&mut layout);
        for panel in [Panel::Brushes, Panel::Sizes] {
            layout
                .move_panel(
                    [1200.0, 900.0],
                    panel,
                    DockTarget::Tab {
                        group: 8,
                        index: None,
                    },
                )
                .unwrap();
            check(&mut layout);
        }
        for panel in [
            Panel::Sizes,
            Panel::Brushes,
            Panel::Properties,
            Panel::Adjustments,
        ] {
            layout.set_panel_visible(panel, false).unwrap();
            check(&mut layout);
            layout = serde_json::from_str(&serde_json::to_string(&layout).unwrap()).unwrap();
            check(&mut layout);
        }
    }

    #[test]
    fn all_tab_styles_apply_to_the_whole_group_and_persist() {
        let styles = [
            (TabStyle::Automatic, [true, true], [true, false]),
            (TabStyle::ActiveName, [true, true], [true, false]),
            (TabStyle::IconName, [true, true], [true, true]),
            (TabStyle::Name, [false, true], [false, true]),
            (TabStyle::Icon, [true, false], [true, false]),
        ];
        assert_eq!(TabStyle::default(), TabStyle::Automatic);
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut layout = DockLayout::default();
            let mut state = CustomizationState::default();
            for (style, active, inactive) in styles {
                let menu = layout
                    .context_menu(ContextTarget::Group { group: 8 })
                    .unwrap();
                assert_eq!(menu.sections[0].len(), TabStyle::ALL.len());
                let item = menu.sections[0]
                    .iter()
                    .find(|item| item.label == style.label())
                    .unwrap();
                let Some(UiAction::Customize { action }) = item.action.clone() else {
                    panic!("Missing group style action")
                };
                state
                    .edit(&mut layout, action, platform, [1200.0, 900.0], false)
                    .unwrap();
                let encoded = serde_json::to_string(&layout).unwrap();
                layout = serde_json::from_str(&encoded).unwrap();
                layout.validate().unwrap();
                assert_eq!(layout.group_tab_style(8).unwrap(), style);
                for selected in [Panel::Layers, Panel::Adjustments, Panel::Properties] {
                    layout.select_tab(8, selected).unwrap();
                    for panel in [Panel::Layers, Panel::Adjustments, Panel::Properties] {
                        let expected = if selected == panel { active } else { inactive };
                        let tab = layout.tab_presentation(panel);
                        assert_eq!([tab.show_icon, tab.show_name], expected);
                    }
                }
                let menu = layout
                    .context_menu(ContextTarget::Group { group: 8 })
                    .unwrap();
                let checked: Vec<_> = menu.sections[0]
                    .iter()
                    .filter(|i| i.selected == Some(true))
                    .collect();
                assert_eq!(checked.len(), 1);
                assert_eq!(checked[0].label, style.label());
            }
        }
    }

    #[test]
    fn tab_style_is_owned_by_the_group_and_not_individual_panels() {
        let mut layout = DockLayout::default();
        let custom = layout.add_toolbar(Some(8), "Paint", &[PEN]).unwrap();
        let mut state = CustomizationState::default();
        let group = ContextTarget::Group { group: 8 };
        state
            .edit(
                &mut layout,
                CustomizationAction::SetTabStyle {
                    group: 8,
                    style: TabStyle::Icon,
                },
                Platform::Gtk,
                [1200.0, 900.0],
                false,
            )
            .unwrap();
        assert_eq!(layout.group_tab_style(8).unwrap(), TabStyle::Icon);
        for panel in [Panel::Layers, custom] {
            assert_eq!(
                layout.tab_presentation(panel),
                TabPresentation {
                    show_icon: true,
                    show_name: false
                }
            );
        }
        state
            .edit(
                &mut layout,
                CustomizationAction::SetTabStyle {
                    group: 8,
                    style: TabStyle::Name,
                },
                Platform::Gtk,
                [1200.0, 900.0],
                false,
            )
            .unwrap();
        assert!(
            layout.context_menu(group).unwrap().sections[0]
                .iter()
                .any(|i| i.label == "Names only" && i.selected == Some(true))
        );
        let panel = layout
            .context_menu(ContextTarget::Panel {
                panel: Panel::Brushes,
            })
            .unwrap();
        assert_eq!(panel.sections[1][0].label, "Configure Tool Set panel…");
        assert!(!panel.sections.iter().flatten().any(|i| matches!(
            i.action,
            Some(UiAction::Customize {
                action: CustomizationAction::SetTabStyle { .. }
            })
        )));
        assert!(
            !layout
                .toolbar_options(custom)
                .unwrap()
                .iter()
                .flatten()
                .any(|i| matches!(
                    i.action,
                    Some(UiAction::Customize {
                        action: CustomizationAction::SetTabStyle { .. }
                    })
                ))
        );
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
                .map(|i| i.label.as_str())
                .collect::<Vec<_>>(),
            ["Remove Tool", "Insert Tools…"]
        );
        assert_eq!(
            layout
                .context_menu(ContextTarget::Ribbon { panel: custom })
                .unwrap()
                .sections[0][0]
                .label,
            "Configure Paint toolbar…"
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
        assert_eq!(
            native.len(),
            web.len()
                + 2
                + Panel::ALL
                    .iter()
                    .filter(|p| p.kind() == PanelKind::Content)
                    .count()
        );
        assert!(native.iter().all(|c| !c.label.is_empty()
            && !c.description.is_empty()
            && ui_catalog().icons.contains(&c.icon)));
        for choice in native {
            choice.control.validate().unwrap();
            let Some(action) = choice.control.action() else {
                assert!(
                    choice.control.drawer_columns().is_some()
                        || choice.control == ToolbarControl::Divider
                );
                continue;
            };
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
                let custom = layout.add_toolbar(Some(8), "Many", &[PEN]).unwrap();
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
