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
    /// Native hosts supply complete and icon-only widths at the current font
    /// size. Reserve every icon, then spend the remaining width left to right.
    /// Selection never changes Automatic's label priority.
    pub fn automatic_names(available: f32, widths: &[[f32; 2]]) -> Vec<bool> {
        let mut spare = (available - widths.iter().map(|w| w[1]).sum::<f32>()).max(0.);
        widths
            .iter()
            .map(|[full, icon]| {
                let extra = (full - icon).max(0.);
                if extra <= spare {
                    spare -= extra;
                    true
                } else {
                    false
                }
            })
            .collect()
    }
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
    pub fn localized_label(self, localization: &Localizer) -> std::sync::Arc<str> {
        match self {
            Self::Automatic => localization.text(MessageId::WORKSPACE_TAB_STYLE_AUTOMATIC),
            Self::ActiveName => localization.text(MessageId::WORKSPACE_TAB_STYLE_ACTIVE_NAME),
            Self::IconName => localization.text(MessageId::WORKSPACE_TAB_STYLE_ICON_NAME),
            Self::Name => localization.text(MessageId::WORKSPACE_TAB_STYLE_NAME),
            Self::Icon => localization.text(MessageId::WORKSPACE_TAB_STYLE_ICON),
        }
    }

    // Unmeasured fallback for hosts awaiting the allocation-aware projection.
    // GTK refines Automatic with automatic_names and native measurements.
    fn presentation(self, active: bool, tab_count: usize) -> TabPresentation {
        TabPresentation {
            show_icon: self != Self::Name,
            show_name: matches!(self, Self::Name | Self::IconName)
                || (self == Self::Automatic && (tab_count <= 2 || active))
                || (self == Self::ActiveName && active),
        }
    }
}

#[cfg(test)]
#[test]
fn automatic_tab_names_follow_measured_space_and_left_priority() {
    let widths = [[80., 32.], [100., 32.], [60., 32.], [70., 32.]];
    assert_eq!(TabStyle::automatic_names(310., &widths), [true; 4]);
    assert_eq!(
        TabStyle::automatic_names(220., &widths),
        [true, false, true, false]
    );
    assert_eq!(TabStyle::automatic_names(128., &widths), [false; 4]);
    assert_eq!(TabStyle::automatic_names(40., &widths[..1]), [false]);
    assert_eq!(TabStyle::automatic_names(180., &widths[..2]), [true; 2]);
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
    Medium,
    Large,
    MediumLabeled,
    Labeled,
}
impl TileStyle {
    pub fn size(self) -> [f32; 2] {
        let [w, h] = match self {
            Self::Small => [1.0, 1.0],
            Self::Medium => [1.5, 1.5],
            Self::Large => [2.0, 2.0],
            Self::MediumLabeled => [3.0, 1.5],
            Self::Labeled => [3.0, 2.0],
        };
        [w * TILE_SIZE, h * TILE_SIZE]
    }
    /// Squircle radius shared by tiles and toolbars built from them.
    pub fn corner_radius(self) -> f32 {
        let [width, height] = self.size();
        width.min(height) / 2.0
    }
    /// Space between neighboring tiles and lanes. Like the radius, it follows
    /// the shorter side so labeled tiles match their square counterparts.
    pub fn gap(self) -> f32 {
        match self {
            Self::Small | Self::Medium | Self::MediumLabeled => TILE_GAP,
            Self::Large | Self::Labeled => 2.0 * TILE_GAP,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Small Tiles",
            Self::Medium => "Medium Tiles",
            Self::Large => "Large Tiles",
            Self::MediumLabeled => "Medium Labeled Tiles",
            Self::Labeled => "Large Labeled Tiles",
        }
    }
    pub fn localized_label(self, localization: &Localizer) -> std::sync::Arc<str> {
        match self {
            Self::Small => localization.text(MessageId::WORKSPACE_TILE_STYLE_SMALL),
            Self::Medium => localization.text(MessageId::WORKSPACE_TILE_STYLE_MEDIUM),
            Self::Large => localization.text(MessageId::WORKSPACE_TILE_STYLE_LARGE),
            Self::MediumLabeled => localization.text(MessageId::WORKSPACE_TILE_STYLE_MEDIUM_LABELED),
            Self::Labeled => localization.text(MessageId::WORKSPACE_TILE_STYLE_LABELED),
        }
    }

    pub fn icon_size(self) -> u32 {
        match self {
            Self::Small | Self::MediumLabeled | Self::Labeled => 16,
            Self::Medium => 24,
            Self::Large => 32,
        }
    }
    pub fn label_lines(self) -> u32 {
        match self {
            Self::MediumLabeled => 2,
            Self::Labeled => 3,
            _ => 0,
        }
    }
    pub(crate) fn floating_width(self) -> f32 {
        let columns = if self.label_lines() > 0 { 2.0 } else { 3.0 };
        columns * (self.size()[0] + self.gap()) - self.gap()
    }
    pub(crate) fn smaller(self) -> Option<Self> {
        match self {
            Self::Large => Some(Self::Medium),
            Self::Medium => Some(Self::Small),
            Self::Small | Self::MediumLabeled | Self::Labeled => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TilePresentation {
    pub tile_style: TileStyle,
    pub tile_icon_size: u32,
    pub tile_corner_radius: f32,
    pub tile_label_lines: u32,
    pub tile_label_bold: bool,
}
impl From<TileStyle> for TilePresentation {
    fn from(style: TileStyle) -> Self {
        Self {
            tile_style: style,
            tile_icon_size: style.icon_size(),
            tile_corner_radius: style.corner_radius(),
            tile_label_lines: style.label_lines(),
            tile_label_bold: style == TileStyle::Labeled,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelControl {
    Brushes,
    BrushSets,
    FilterTypes,
    SculptSets,
    Tools,
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
            Self::BrushSets => Panel::BrushSets.label(),
            Self::FilterTypes => Panel::FilterTypes.label(),
            Self::SculptSets => Panel::SculptSets.label(),
            Self::Tools => Panel::Tools.label(),
            Self::ToolSettings => Panel::ToolSettings.label(),
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
    pub fn localized_label(self, localization: &Localizer) -> std::sync::Arc<str> {
        match self {
            Self::Brushes => localization.text(MessageId::WORKSPACE_CONTROL_BRUSHES),
            Self::BrushSets => Panel::BrushSets.localized_label(localization),
            Self::FilterTypes => Panel::FilterTypes.localized_label(localization),
            Self::SculptSets => Panel::SculptSets.localized_label(localization),
            Self::Tools => Panel::Tools.localized_label(localization),
            Self::ToolSettings => Panel::ToolSettings.localized_label(localization),
            Self::ColorWheel => localization.text(MessageId::WORKSPACE_CONTROL_COLOR_WHEEL),
            Self::BrushSize => localization.text(MessageId::WORKSPACE_CONTROL_BRUSH_SIZE),
            Self::SizePresets => localization.text(MessageId::WORKSPACE_CONTROL_SIZE_PRESETS),
            Self::BrushOpacity => localization.text(MessageId::WORKSPACE_CONTROL_BRUSH_OPACITY),
            Self::BrushColor => localization.text(MessageId::WORKSPACE_CONTROL_BRUSH_COLOR),
            Self::Layers => localization.text(MessageId::WORKSPACE_CONTROL_LAYERS),
            Self::LayerActions => localization.text(MessageId::WORKSPACE_CONTROL_LAYER_ACTIONS),
            Self::LayerOpacity => localization.text(MessageId::WORKSPACE_CONTROL_LAYER_OPACITY),
            Self::Adjustments => localization.text(MessageId::WORKSPACE_CONTROL_ADJUSTMENTS),
            Self::Properties => localization.text(MessageId::WORKSPACE_CONTROL_PROPERTIES),
            Self::Stats => localization.text(MessageId::WORKSPACE_CONTROL_STATS),
            Self::Navigator => localization.text(MessageId::WORKSPACE_CONTROL_NAVIGATOR),
        }
    }
    pub fn available(panel: Panel) -> &'static [Self] {
        match panel {
            Panel::BrushSets => &[Self::BrushSets],
            Panel::FilterTypes => &[Self::FilterTypes],
            Panel::SculptSets => &[Self::SculptSets],
            Panel::Tools => &[Self::Tools],
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
            | Panel::BrushSets
            | Panel::FilterTypes
            | Panel::SculptSets
            | Panel::Tools
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
        name: Option<String>,
        tiles: Vec<ToolbarTile>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelConfig {
    pub id: Panel,
    pub hide_tab: bool,
    pub tile_style: TileStyle,
    pub content: PanelContent,
}
impl Panel {
    pub fn localized_label(self, localization: &Localizer) -> std::sync::Arc<str> {
        match self {
            Self::Toolbar => localization.text(MessageId::WORKSPACE_PANEL_TOOLBAR),
            Self::Commands => localization.text(MessageId::WORKSPACE_PANEL_COMMANDS),
            Self::Brushes => localization.text(MessageId::WORKSPACE_PANEL_BRUSHES),
            Self::BrushSets => localization.text(MessageId::WORKSPACE_PANEL_BRUSH_SETS),
            Self::FilterTypes => localization.text(MessageId::WORKSPACE_PANEL_FILTER_TYPES),
            Self::SculptSets => localization.text(MessageId::WORKSPACE_PANEL_SCULPT_SETS),
            Self::Tools => localization.text(MessageId::WORKSPACE_PANEL_TOOLS),
            Self::ToolSettings => localization.text(MessageId::WORKSPACE_PANEL_TOOL_SETTINGS),
            Self::Color => localization.text(MessageId::WORKSPACE_PANEL_COLOR),
            Self::Palettes => localization.text(MessageId::WORKSPACE_PANEL_PALETTES),
            Self::Sizes => localization.text(MessageId::WORKSPACE_PANEL_SIZES),
            Self::Layers => localization.text(MessageId::WORKSPACE_PANEL_LAYERS),
            Self::Adjustments => localization.text(MessageId::WORKSPACE_PANEL_ADJUSTMENTS),
            Self::Properties => localization.text(MessageId::WORKSPACE_PANEL_PROPERTIES),
            Self::Stats => localization.text(MessageId::WORKSPACE_PANEL_STATS),
            Self::Navigator => localization.text(MessageId::WORKSPACE_PANEL_NAVIGATOR),
            Self::Proof => localization.text(MessageId::WORKSPACE_PANEL_PROOF),
            Self::CustomToolbar(_) => localization.text(MessageId::WORKSPACE_PANEL_CUSTOM_TOOLBAR),
        }
    }
}
impl PanelConfig {
    pub fn icon(&self) -> &'static str {
        self.tiles()
            .first()
            .map(|t| t.control.icon())
            .unwrap_or(self.id.icon())
    }
    pub fn custom_name(&self) -> Option<&str> {
        match &self.content { PanelContent::Toolbar { name, .. } => name.as_deref(), _ => None }
    }
    pub fn title(&self) -> &str { self.custom_name().unwrap_or_else(|| self.id.label()) }
    pub fn title_localized(&self, localization: &Localizer) -> String {
        self.custom_name().map(str::to_owned).unwrap_or_else(|| self.id.localized_label(localization).to_string())
    }
    pub(crate) fn menu_name(&self, localization: &Localizer) -> String {
        customization_text(localization, if self.id.kind() == PanelKind::Content { MessageId::WORKSPACE_HISTORY_PANEL } else { MessageId::WORKSPACE_HISTORY_TOOLBAR }, &[("name", self.title_localized(localization))])
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
            // Commands is installed by the full editor preset.
            .filter(|id| *id != Panel::Commands)
            .map(|id| Self {
                id,
                hide_tab: false,
                tile_style: TileStyle::Small,
                content: if id == Panel::Toolbar {
                    PanelContent::Toolbar {
                        name: None,
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
    pub fn validate(&self) -> Result<(), String> {
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
                if let Some(name) = name { validate_toolbar_name(name)?; }
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
    pub(crate) fn icon(self) -> &'static str {
        match self {
            Self::Command { command } => command.icon().unwrap_or(match command {
                CommandId::ToggleTheme => "appearance", CommandId::KeyboardShortcuts => "keyboard", CommandId::About => "info", _ => "menu",
            }),
            Self::Brush { id } => preset(id).map(|_| crate::tools::group(id).icon()).unwrap_or("brush"),
            Self::Size { .. } | Self::BrushSizeSlider => "size",
            Self::Color => "colors", Self::ColorPicker => "color-picker",
            Self::Opacity | Self::BrushOpacitySlider => "opacity",
            Self::ToolOptions { .. } => "settings", Self::Panel { panel } => panel.icon(), Self::Divider => "minus",
        }
    }
    pub fn action(self) -> Option<UiAction> {
        Some(match self {
            Self::ColorPicker => UiAction::ColorPicker { action: crate::ColorPickerAction::Toggle },
            Self::Command { command } => UiAction::Invoke { command },
            Self::Brush { id } => UiAction::SelectBrush { id },
            Self::Size { pixels } => UiAction::SetBrushSize {
                value: pixels as f32,
            },
            Self::Color | Self::Opacity | Self::Panel { .. } | Self::Divider
            | Self::BrushSizeSlider | Self::BrushOpacitySlider | Self::ToolOptions { .. } => return None,
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
    Header { id: Option<u32> },
    Column { column: u32 },
    ZenMode,
    Panel { panel: Panel },
    Group { group: u32 },
    Tile { panel: Panel, tile: u32 },
    Ribbon { panel: Panel },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CustomizationAction {
    ToggleHeaderDrawer {
        id: u32,
    },
    Header {
        action: HeaderAction,
    },
    SetPanelVisible {
        panel: Panel,
        visible: bool,
    },
    AddPanel {
        panel: Panel,
        group: u32,
    },
    RestoreBuiltinToolbar {
        panel: Panel,
        group: Option<u32>,
    },
    SetToolOptionsStyle { panel: Panel, tile: u32, style: crate::ToolOptionsStyle },
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
    SetColumnCollapsed {
        group: u32,
        collapsed: bool,
    },
    ToggleColumnDrawer {
        group: u32,
        panel: Panel,
    },
    SetColumnDrawers {
        column: u32,
        drawers: bool,
    },
    SetColumnAutoHide {
        column: u32,
        auto_hide: bool,
    },
    ApplyColumnStack {
        column: u32,
    },
    CloseColumn {
        column: u32,
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
    pub bindings: Vec<KeyChord>,
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
            bindings: Vec::new(),
            sections: Vec::new(),
        }
    }
    pub(crate) fn edit(label: impl Into<String>, action: CustomizationAction) -> Self {
        Self::command(label, UiAction::Customize { action })
    }
    pub(crate) fn submenu(label: &str, sections: Vec<Vec<Self>>) -> Self {
        let sections: Vec<_> = sections.into_iter().filter(|s| !s.is_empty()).collect();
        Self {
            label: label.into(),
            selected: None,
            action: None,
            enabled: !sections.is_empty(),
            hint: String::new(),
            bindings: Vec::new(),
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
    pub(crate) fn with_shortcuts(self, settings: &Settings, platform: Platform) -> Self {
        self.with_shortcuts_localized(settings, platform, &Localizer::shared(UiLanguage::English))
    }
    pub(crate) fn with_shortcuts_localized(mut self, settings: &Settings, platform: Platform, localization: &Localizer) -> Self {
        fn visit(sections: &mut [Vec<ContextMenuItem>], settings: &Settings, platform: Platform, localization: &Localizer) {
            for item in sections.iter_mut().flatten() {
                if let Some(action) = &item.action {
                    item.bindings = settings.action_keys(action, platform);
                    let shortcut = item
                        .bindings
                        .iter()
                        .map(|key| key.localized_label(platform, localization))
                        .collect::<Vec<_>>()
                        .join(" / ");
                    if !shortcut.is_empty() && item.hint != shortcut {
                        item.hint = if item.hint.is_empty() {
                            shortcut
                        } else {
                            format!("{} · {shortcut}", item.hint)
                        };
                    }
                }
                visit(&mut item.sections, settings, platform, localization);
            }
        }
        visit(&mut self.sections, settings, platform, localization);
        self
    }
}
impl DockLayout {
    pub(crate) fn context_menu_on(
        &self,
        target: ContextTarget,
        platform: Platform,
    ) -> Result<ContextMenu, String> {
        self.context_menu_localized_on(target, platform, &Localizer::shared(UiLanguage::English))
    }
    pub(crate) fn context_menu_localized_on(&self, target: ContextTarget, platform: Platform, localization: &Localizer) -> Result<ContextMenu, String> {
        let entry = ContextMenuItem::edit;
        let (title, sections) = match target {
            ContextTarget::Header { id } => {
                return self.header.projected_for(platform).context_menu(id, false);
            }
            ContextTarget::Column { column } => {
                if !self.is_collapsed(column) {
                    return Err("The column is not collapsed".into());
                }
                (
                    localization.text(MessageId::WORKSPACE_COLUMN_TITLE).to_string(),
                    self.column_sections(Some(column), localization),
                )
            }
            ContextTarget::ZenMode => return Err("Not a panel context".into()),
            ContextTarget::Panel { panel } => {
                let p = self.panel(panel)?;
                let mut sections = vec![self.hide_tab_items(target, localization)?];
                sections.extend(self.column_sections(self.panel_group(panel), localization));
                sections.push(self.panel_actions(p, localization));
                (p.menu_name(localization), sections)
            }
            ContextTarget::Group { group } => {
                let mut sections = vec![self.tab_style_items(group, localization)?, self.hide_tab_items(target, localization)?];
                sections.extend(self.column_sections(Some(group), localization));
                sections.extend([
                    if let [panel] = self.group_panels(group)?
                        && let p = self.panel(*panel)?
                        && panel.kind() == PanelKind::Content
                        && p.hide_tab
                    {
                        self.panel_actions(p, localization)
                    } else {
                        Vec::new()
                    },
                    vec![
                        ContextMenuItem::submenu(
                            &localization.text(MessageId::WORKSPACE_ADD_PANEL_MENU).to_string(),
                            vec![self.panel_items_localized(PanelKind::Content, Some(group), localization)],
                        ),
                        ContextMenuItem::submenu(
                            &localization.text(MessageId::WORKSPACE_ADD_TOOLBAR_MENU).to_string(),
                            vec![self.panel_items_localized(PanelKind::Tiles, Some(group), localization)],
                        ),
                    ],
                    vec![entry(
                        localization.text(MessageId::WORKSPACE_NEW_TOOLBAR_MENU).to_string(),
                        CustomizationAction::NewToolbar { group: Some(group) },
                    )],
                ]);
                (localization.text(MessageId::WORKSPACE_GROUP_TITLE).to_string(), sections)
            }
            ContextTarget::Tile { panel, tile } => {
                let p = self.panel(panel)?;
                let t = p
                    .tiles()
                    .iter()
                    .find(|t| t.id == tile)
                    .ok_or("The tool no longer exists")?;
                let mut sections = vec![vec![
                    entry(localization.text(MessageId::WORKSPACE_REMOVE_TOOL_MENU).to_string(), CustomizationAction::RemoveTool { panel, tile }),
                    entry(localization.text(MessageId::WORKSPACE_INSERT_TOOLS_MENU).to_string(), CustomizationAction::InsertTools { panel, before: Some(tile) }),
                ]];
                if let Some(style) = t.control.options_style() {
                    let mut choices = Vec::new();
                    for (label, text) in [(localization.text(MessageId::WORKSPACE_TOOL_OPTIONS_TEXT).to_string(), true), (localization.text(MessageId::WORKSPACE_TOOL_OPTIONS_ICONS).to_string(), false)] {
                        let mut item = entry(label, CustomizationAction::SetToolOptionsStyle {
                            panel, tile, style: crate::ToolOptionsStyle { text, ..style },
                        });
                        item.selected = Some(style.text == text);
                        choices.push(item);
                    }
                    let mut sliders = entry(localization.text(MessageId::WORKSPACE_SHOW_SLIDERS_MENU).to_string(), CustomizationAction::SetToolOptionsStyle {
                        panel, tile, style: crate::ToolOptionsStyle { sliders: !style.sliders, ..style },
                    });
                    sliders.selected = Some(style.sliders);
                    choices.push(sliders);
                    sections.insert(0, choices);
                }
                (tool_choice_localized(t.control, localization).label, sections)
            }
            ContextTarget::Ribbon { panel } => {
                let p = self.panel(panel)?;
                if panel.kind() != PanelKind::Tiles {
                    return Err("Choose a toolbar".into());
                }
                (p.menu_name(localization), self.toolbar_options_localized(panel, localization)?)
            }
        };
        Ok(ContextMenu { title, sections })
    }
    fn panel_actions(&self, panel: &PanelConfig, localization: &Localizer) -> Vec<ContextMenuItem> {
        vec![
            ContextMenuItem::edit(
                customization_text(localization, MessageId::WORKSPACE_CONFIGURE_MENU, &[("name", panel.menu_name(localization))]),
                CustomizationAction::ShowAllControls { panel: panel.id },
            ),
            self.hide_item(panel.id, localization),
        ]
    }
    fn column_sections(&self, group: Option<u32>, localization: &Localizer) -> Vec<Vec<ContextMenuItem>> {
        let Some(group) = group else { return Vec::new(); };
        if self.collapsible_column_for_group(group).is_none()
            && self.collapsed_column_for_group(group).is_none()
        {
            return Vec::new();
        }
        let column = self.collapsed_column_for_group(group);
        let mut sections = vec![vec![ContextMenuItem::edit(
            if column.is_some() {
                localization.text(MessageId::WORKSPACE_EXPAND_COLUMN_MENU).to_string()
            } else {
                localization.text(MessageId::WORKSPACE_COLLAPSE_COLUMN_MENU).to_string()
            },
            CustomizationAction::SetColumnCollapsed {
                group: column.unwrap_or(group),
                collapsed: column.is_none(),
            },
        )]];
        if let Some(column) = column {
            let settings = self.column_stack(column);
            let mut item = ContextMenuItem::edit(localization.text(MessageId::WORKSPACE_COLUMN_DRAWERS_MENU).to_string(),
                CustomizationAction::SetColumnDrawers { column, drawers: !settings.drawers });
            item.selected = Some(settings.drawers);
            sections.push(vec![item]);
            let mut item = ContextMenuItem::edit(
                localization.text(MessageId::WORKSPACE_AUTO_HIDE_MENU).to_string(),
                CustomizationAction::SetColumnAutoHide {
                    column,
                    auto_hide: !settings.auto_hide,
                },
            );
            item.selected = Some(settings.auto_hide);
            sections.push(vec![item]);
            sections.push(vec![ContextMenuItem::edit(
                localization.text(MessageId::WORKSPACE_APPLY_COLUMNS_MENU).to_string(),
                CustomizationAction::ApplyColumnStack { column },
            )]);
        }
        sections
    }
    fn hide_item(&self, panel: Panel, localization: &Localizer) -> ContextMenuItem {
        ContextMenuItem::edit(
            customization_text(localization, MessageId::WORKSPACE_HIDE_MENU, &[("name", self.panel(panel).expect("validated panel").menu_name(localization))]),
            CustomizationAction::SetPanelVisible {
                panel,
                visible: false,
            },
        )
    }
    pub fn panel_items(&self, kind: PanelKind, group: Option<u32>) -> Vec<ContextMenuItem> {
        self.panel_items_localized(kind, group, &Localizer::shared(UiLanguage::English))
    }
    pub fn panel_items_localized(&self, kind: PanelKind, group: Option<u32>, localization: &Localizer) -> Vec<ContextMenuItem> {
        let mut items: Vec<_> = self
            .panels
            .iter()
            .filter(|p| p.id.kind() == kind)
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
                let mut item = ContextMenuItem::edit(p.menu_name(localization), action);
                item.selected = Some(selected);
                item.enabled = selected
                    || group.is_none_or(|g| {
                        !matches!(self.group_edge(g), Some(Edge::Top | Edge::Bottom))
                    });
                item
            })
            .collect();
        if kind == PanelKind::Tiles {
            items.extend(
                [Panel::Toolbar, Panel::Commands]
                    .into_iter()
                    .filter(|p| self.panel(*p).is_err())
                    .map(|panel| {
                        ContextMenuItem::edit(
                            customization_text(localization, MessageId::WORKSPACE_RESTORE_TOOLBAR_MENU, &[("name", panel.localized_label(localization).to_string())]),
                            CustomizationAction::RestoreBuiltinToolbar { panel, group },
                        )
                    }),
            );
        }
        items
    }
    pub fn toolbar_options(&self, panel: Panel) -> Result<Vec<Vec<ContextMenuItem>>, String> {
        self.toolbar_options_localized(panel, &Localizer::shared(UiLanguage::English))
    }
    pub fn toolbar_options_localized(&self, panel: Panel, localization: &Localizer) -> Result<Vec<Vec<ContextMenuItem>>, String> {
        let p = self.panel(panel)?;
        if panel.kind() != PanelKind::Tiles {
            return Err("Choose a toolbar".into());
        }
        let name = p.menu_name(localization);
        Ok(vec![
            vec![
                ContextMenuItem::edit(
                    customization_text(localization, MessageId::WORKSPACE_CONFIGURE_MENU, &[("name", name.clone())]),
                    CustomizationAction::ShowAllControls { panel },
                ),
                ContextMenuItem::edit(
                    localization.text(MessageId::WORKSPACE_ADD_TOOLS_MENU).to_string(),
                    CustomizationAction::InsertTools {
                        panel,
                        before: None,
                    },
                ),
            ],
            [
                TileStyle::Small,
                TileStyle::Medium,
                TileStyle::Large,
                TileStyle::MediumLabeled,
                TileStyle::Labeled,
            ]
            .into_iter()
            .map(|style| {
                let mut item = ContextMenuItem::edit(
                    style.localized_label(localization).to_string(),
                    CustomizationAction::SetTileStyle { panel, style },
                );
                item.selected = Some(p.tile_style == style);
                item
            })
            .collect(),
            vec![
                ContextMenuItem::edit(
                    customization_text(localization, MessageId::WORKSPACE_RENAME_MENU, &[("name", name.clone())]),
                    CustomizationAction::RenameToolbar { panel },
                ),
                ContextMenuItem::edit(
                    customization_text(localization, MessageId::WORKSPACE_DUPLICATE_MENU, &[("name", name.clone())]),
                    CustomizationAction::DuplicateToolbar { panel },
                ),
            ],
            vec![self.hide_item(panel, localization)],
        ])
    }
    fn tab_style_items(&self, group: u32, localization: &Localizer) -> Result<Vec<ContextMenuItem>, String> {
        let selected = self.group_tab_style(group)?;
        Ok(TabStyle::ALL
            .into_iter()
            .map(|style| {
                let mut item = ContextMenuItem::edit(
                    style.localized_label(localization).to_string(),
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
    fn hide_tab_items(&self, target: ContextTarget, localization: &Localizer) -> Result<Vec<ContextMenuItem>, String> {
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
            localization.text(MessageId::WORKSPACE_SHOW_TAB_BAR_MENU).to_string(),
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
            .any(|p| Some(p.id) != except && p.custom_name().is_some_and(|title| title.to_lowercase() == name.trim().to_lowercase()))
        {
            return Err("A panel already uses this name".into());
        }
        Ok(())
    }
    pub(crate) fn unused_toolbar_name(&self, stem: &str) -> String {
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
    tool_choice_localized(control, &Localizer::shared(UiLanguage::English))
}
pub fn tool_choice_localized(control: ToolbarControl, localization: &Localizer) -> ToolChoice {
    let (label, description, icon) = match control {
        ToolbarControl::Command { command } => {
            let label = command.localized_label(localization);
            (
            label.to_string(),
            match command {
                CommandId::SearchCommands => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SEARCH_COMMANDS).to_string(),
                CommandId::DrawingBrush => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_DRAWING_BRUSH).to_string(),
                CommandId::Sculpt => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SCULPT).to_string(),
                CommandId::DocumentProperties => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_DOCUMENT_PROPERTIES).to_string(),
                CommandId::SdrRendition => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SDR_RENDITION).to_string(),
                CommandId::PreviewSdr => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_PREVIEW_SDR).to_string(),
                CommandId::SoftProofSetup => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SOFT_PROOF_SETUP).to_string(),
                CommandId::SoftProof => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SOFT_PROOF).to_string(),
                CommandId::GamutWarning => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_GAMUT_WARNING).to_string(),
                CommandId::Histogram => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_HISTOGRAM).to_string(),
                CommandId::AssignProfile => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_ASSIGN_PROFILE).to_string(),
                CommandId::ConvertColorSpace => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CONVERT_COLOR_SPACE).to_string(),
                CommandId::ChangeBitDepth => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CHANGE_BIT_DEPTH).to_string(),
                CommandId::ImportImage => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_IMPORT_IMAGE).to_string(),
                CommandId::RasterizeSource => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_RASTERIZE_SOURCE).to_string(),
                CommandId::RepairSourceProfile => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_REPAIR_SOURCE_PROFILE).to_string(),
                CommandId::Copy => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_COPY).to_string(),
                CommandId::Cut => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CUT).to_string(),
                CommandId::CopyMerged => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_COPY_MERGED).to_string(),
                CommandId::PasteImage => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_PASTE_IMAGE).to_string(),
                CommandId::PasteInPlace => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_PASTE_IN_PLACE).to_string(),
                CommandId::PasteInto => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_PASTE_INTO).to_string(),
                CommandId::Pen => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_PEN).to_string(),
                CommandId::Pencil => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_PENCIL).to_string(),
                CommandId::Brush => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_BRUSH).to_string(),
                CommandId::Eraser => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_ERASER).to_string(),
                CommandId::Airbrush => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_AIRBRUSH).to_string(),
                CommandId::Decoration => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_DECORATION).to_string(),
                CommandId::Blend => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_BLEND).to_string(),
                CommandId::Liquify => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_LIQUIFY).to_string(),
                CommandId::Lasso => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_LASSO).to_string(),
                CommandId::LassoFill => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_LASSO_FILL).to_string(),
                CommandId::Select => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SELECT).to_string(),
                CommandId::QuickMask | CommandId::ReturnToArtwork | CommandId::NewSelectionLayer | CommandId::SaveSelectionLayer | CommandId::Reselect | CommandId::SelectionOutline | CommandId::MaskOverlay | CommandId::MaskOverlayProtected | CommandId::ResetMaskColors | CommandId::SwapMaskColors | CommandId::FillSelectionMask | CommandId::ClearSelectionMask => label.to_string(),
                CommandId::TonalSelect => label.to_string(),
                CommandId::SelectionBrush => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SELECTION_BRUSH).to_string(),
                CommandId::SelectionBrushPressure => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SELECTION_BRUSH_PRESSURE).to_string(),
                CommandId::RectangleSelect => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_RECTANGLE_SELECT).to_string(),
                CommandId::EllipseSelect => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_ELLIPSE_SELECT).to_string(),
                CommandId::PolygonSelect => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_POLYGON_SELECT).to_string(),
                CommandId::ColorSelect => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_COLOR_SELECT).to_string(),
                CommandId::SelectionNew => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SELECTION_NEW).to_string(),
                CommandId::SelectionAdd => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SELECTION_ADD).to_string(),
                CommandId::SelectionSubtract => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SELECTION_SUBTRACT).to_string(),
                CommandId::SelectionIntersect => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SELECTION_INTERSECT).to_string(),
                CommandId::SelectionAntialias => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SELECTION_ANTIALIAS).to_string(),
                CommandId::SelectionConstrainAngles => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SELECTION_CONSTRAIN_ANGLES).to_string(),
                CommandId::SelectionFixedRatio => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SELECTION_FIXED_RATIO).to_string(),
                CommandId::SelectionFixedSize => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SELECTION_FIXED_SIZE).to_string(),
                CommandId::SelectionFromCenter => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SELECTION_FROM_CENTER).to_string(),
                CommandId::CompleteSelection => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_COMPLETE_SELECTION).to_string(),
                CommandId::CancelSelection => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CANCEL_SELECTION).to_string(),
                CommandId::SelectionVisible => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SELECTION_VISIBLE).to_string(),
                CommandId::SelectionEditing => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SELECTION_EDITING).to_string(),
                CommandId::SelectionReference => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SELECTION_REFERENCE).to_string(),
                CommandId::CloneSourceArm => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CLONE_SOURCE_ARM).to_string(),
                CommandId::Clone => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CLONE).to_string(),
                CommandId::Heal => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_HEAL).to_string(),
                CommandId::SpotHeal => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SPOT_HEAL).to_string(),
                CommandId::CloneAligned => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CLONE_ALIGNED).to_string(),
                CommandId::CloneFlipHorizontal => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CLONE_FLIP_HORIZONTAL).to_string(),
                CommandId::CloneFlipVertical => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CLONE_FLIP_VERTICAL).to_string(),
                CommandId::CloneResetOffset => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CLONE_RESET_OFFSET).to_string(),

                CommandId::Move => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_MOVE).to_string(),
                CommandId::MoveLeaveCopy => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_MOVE_LEAVE_COPY).to_string(),
                CommandId::ScaleRotate => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SCALE_ROTATE).to_string(),
                CommandId::ApplyTransform => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_APPLY_TRANSFORM).to_string(),
                CommandId::CancelTransform => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CANCEL_TRANSFORM).to_string(),
                CommandId::PlacementOriginalSize => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_PLACEMENT_ORIGINAL_SIZE).to_string(),
                CommandId::Hand => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_HAND).to_string(),
                CommandId::Eyedropper => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_EYEDROPPER).to_string(),
                CommandId::Gradient => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_GRADIENT).to_string(),
                CommandId::Figure => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_FIGURE).to_string(),
                CommandId::Ruler => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_RULER).to_string(),
                CommandId::ShowRulers => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SHOW_RULERS).to_string(),
                CommandId::SnapRulers => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SNAP_RULERS).to_string(),
                CommandId::DeleteRuler => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_DELETE_RULER).to_string(),
                CommandId::AutoSelect => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_AUTO_SELECT).to_string(),
                CommandId::Fill => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_FILL).to_string(),
                CommandId::Undo => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_UNDO).to_string(),
                CommandId::Redo => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_REDO).to_string(),
                CommandId::ClearLayer => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CLEAR_LAYER).to_string(),
                CommandId::ClearSelected => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CLEAR_SELECTED).to_string(),
                CommandId::ClearOutside => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CLEAR_OUTSIDE).to_string(),
                CommandId::CopySelectionToLayer => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_COPY_SELECTION_TO_LAYER).to_string(),
                CommandId::CutSelectionToLayer => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CUT_SELECTION_TO_LAYER).to_string(),
                CommandId::RevertToOriginal => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_REVERT_TO_ORIGINAL).to_string(),
                CommandId::ApplyTransformPixels => localization.text(MessageId::COMMANDS_HELP_APPLY_TRANSFORM_PIXELS).to_string(),
                CommandId::MergeDown => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_MERGE_DOWN).to_string(),
                CommandId::MergeGroup => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_MERGE_GROUP).to_string(),
                CommandId::MergeVisible => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_MERGE_VISIBLE).to_string(),
                CommandId::FlattenImage => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_FLATTEN_IMAGE).to_string(),
                CommandId::StampVisible => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_STAMP_VISIBLE).to_string(),
                CommandId::BlendPerceptual => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_BLEND_PERCEPTUAL).to_string(),
                CommandId::BlendLinear => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_BLEND_LINEAR).to_string(),
                CommandId::NewDodgeBurnLayer => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_NEW_DODGE_BURN_LAYER).to_string(),
                CommandId::FrequencySeparation => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_FREQUENCY_SEPARATION).to_string(),
                CommandId::CanvasSize => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CANVAS_SIZE).to_string(),
                CommandId::CropCanvasToSelection => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CROP_CANVAS_TO_SELECTION).to_string(),
                CommandId::GrowSelection => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_GROW_SELECTION).to_string(),
                CommandId::ShrinkSelection => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SHRINK_SELECTION).to_string(),
                CommandId::FeatherSelection => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_FEATHER_SELECTION).to_string(),
                CommandId::BorderSelection => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_BORDER_SELECTION).to_string(),
                CommandId::SmoothSelection => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SMOOTH_SELECTION).to_string(),
                CommandId::TransformSelectionOutline => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_TRANSFORM_SELECTION_OUTLINE).to_string(),
                CommandId::Crop => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CROP).to_string(),
                CommandId::CropRatioFree => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CROP_RATIO_FREE).to_string(),
                CommandId::CropRatioOriginal => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CROP_RATIO_ORIGINAL).to_string(),
                CommandId::CropRatioSquare => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CROP_RATIO_SQUARE).to_string(),
                CommandId::CropRatioFourFive => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CROP_RATIO_FOUR_FIVE).to_string(),
                CommandId::CropRatioTwoThree => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CROP_RATIO_TWO_THREE).to_string(),
                CommandId::CropRatioFiveSeven => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CROP_RATIO_FIVE_SEVEN).to_string(),
                CommandId::CropRatioSixteenNine => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CROP_RATIO_SIXTEEN_NINE).to_string(),
                CommandId::CropSwapOrientation => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CROP_SWAP_ORIENTATION).to_string(),
                CommandId::CropOverlayThirds => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CROP_OVERLAY_THIRDS).to_string(),
                CommandId::CropOverlayGrid => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CROP_OVERLAY_GRID).to_string(),
                CommandId::CropOverlayDiagonal => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CROP_OVERLAY_DIAGONAL).to_string(),
                CommandId::CropOverlayGolden => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CROP_OVERLAY_GOLDEN).to_string(),
                CommandId::CropCycleOverlay => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CROP_CYCLE_OVERLAY).to_string(),
                CommandId::CropStraighten => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CROP_STRAIGHTEN).to_string(),
                CommandId::CropDeleteCroppedPixels => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CROP_DELETE_CROPPED_PIXELS).to_string(),
                CommandId::StraightenToGuide => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_STRAIGHTEN_TO_GUIDE).to_string(),
                CommandId::CropFitContent => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CROP_FIT_CONTENT).to_string(),
                CommandId::ImageSize => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_IMAGE_SIZE).to_string(),
                CommandId::RotateImageLeft => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_ROTATE_IMAGE_LEFT).to_string(),
                CommandId::RotateImageRight => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_ROTATE_IMAGE_RIGHT).to_string(),
                CommandId::RotateImage180 => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_ROTATE_IMAGE_180).to_string(),
                CommandId::FlipImageHorizontal => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_FLIP_IMAGE_HORIZONTAL).to_string(),
                CommandId::FlipImageVertical => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_FLIP_IMAGE_VERTICAL).to_string(),
                CommandId::Trim => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_TRIM).to_string(),
                CommandId::RevealAll => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_REVEAL_ALL).to_string(),
                CommandId::FillSelection => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_FILL_SELECTION).to_string(),
                CommandId::SelectAll => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SELECT_ALL).to_string(),
                CommandId::Deselect => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_DESELECT).to_string(),
                CommandId::InvertSelection => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_INVERT_SELECTION).to_string(),
                CommandId::UndoWorkspace => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_UNDO_WORKSPACE).to_string(),
                CommandId::RedoWorkspace => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_REDO_WORKSPACE).to_string(),
                CommandId::NewToolbar => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_NEW_TOOLBAR).to_string(),
                CommandId::ManageToolbars => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_MANAGE_TOOLBARS).to_string(),
                CommandId::FitCanvas => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_FIT_CANVAS).to_string(),
                CommandId::ActualPixels => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_ACTUAL_PIXELS).to_string(),
                CommandId::Settings => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SETTINGS).to_string(),
                CommandId::ToggleTheme => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_TOGGLE_THEME).to_string(),
                CommandId::AddLayer => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_ADD_LAYER).to_string(),
                CommandId::DeleteLayer => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_DELETE_LAYER).to_string(),
                CommandId::RaiseLayer => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_RAISE_LAYER).to_string(),
                CommandId::LowerLayer => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_LOWER_LAYER).to_string(),
                CommandId::ResetLayout => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_RESET_LAYOUT).to_string(),
                CommandId::ZenMode => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_ZEN_MODE).to_string(),
                CommandId::CustomizeWorkspaceUi => {
                    localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CUSTOMIZE_WORKSPACE_UI).to_string()
                }
                CommandId::Fullscreen => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_FULLSCREEN).to_string(),
                CommandId::NewWindow => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_NEW_WINDOW).to_string(),
                CommandId::Drawings => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_DRAWINGS).to_string(),
                CommandId::ShowCanvasActionBar => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SHOW_CANVAS_ACTION_BAR).to_string(),
                CommandId::TransformFlipHorizontal => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_TRANSFORM_FLIP_HORIZONTAL).to_string(),
                CommandId::TransformFlipVertical => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_TRANSFORM_FLIP_VERTICAL).to_string(),
                CommandId::TransformRotateLeft => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_TRANSFORM_ROTATE_LEFT).to_string(),
                CommandId::TransformRotateRight => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_TRANSFORM_ROTATE_RIGHT).to_string(),
                CommandId::ResetTransform => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_RESET_TRANSFORM).to_string(),
                CommandId::RemoveSelectionPoint => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_REMOVE_SELECTION_POINT).to_string(),
                CommandId::MaskSelection => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_MASK_SELECTION).to_string(),
                CommandId::UseReferenceBelow => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_USE_REFERENCE_BELOW).to_string(),
                CommandId::LoadSelectionLayer => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_LOAD_SELECTION_LAYER).to_string(),
                CommandId::InvertSelectionLayer => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_INVERT_SELECTION_LAYER).to_string(),
                CommandId::InvertLayerMask => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_INVERT_LAYER_MASK).to_string(),
                CommandId::LayerMaskEnabled => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_LAYER_MASK_ENABLED).to_string(),
                CommandId::ApplyLayerMask => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_APPLY_LAYER_MASK).to_string(),
                CommandId::EditLayerMask => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_EDIT_LAYER_MASK).to_string(),
                CommandId::EditLayerContent => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_EDIT_LAYER_CONTENT).to_string(),
                CommandId::TransformFree => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_TRANSFORM_FREE).to_string(),
                CommandId::TransformUniform => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_TRANSFORM_UNIFORM).to_string(),
                CommandId::TransformDistort => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_TRANSFORM_DISTORT).to_string(),
                CommandId::TransformPerspective => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_TRANSFORM_PERSPECTIVE).to_string(),
                CommandId::TransformNearest => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_TRANSFORM_NEAREST).to_string(),
                CommandId::TransformBilinear => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_TRANSFORM_BILINEAR).to_string(),
                CommandId::TransformBicubic => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_TRANSFORM_BICUBIC).to_string(),
                CommandId::TransformLanczos => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_TRANSFORM_LANCZOS).to_string(),
                CommandId::ColorMixOklab => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_COLOR_MIX_OKLAB).to_string(),
                CommandId::ColorMixLinear => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_COLOR_MIX_LINEAR).to_string(),
                CommandId::ColorMixClassic => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_COLOR_MIX_CLASSIC).to_string(),
                CommandId::TransformWarp => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_TRANSFORM_WARP).to_string(),
                CommandId::WarpGridThree => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_WARP_GRID_THREE).to_string(),
                CommandId::WarpGridFour => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_WARP_GRID_FOUR).to_string(),
                CommandId::WarpGridFive => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_WARP_GRID_FIVE).to_string(),
                CommandId::NewDocument => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_NEW_DOCUMENT).to_string(),
                CommandId::OpenDocument => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_OPEN_DOCUMENT).to_string(),
                CommandId::SaveDocument => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SAVE_DOCUMENT).to_string(),
                CommandId::SaveDocumentAs => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SAVE_DOCUMENT_AS).to_string(),
                CommandId::ExportDocument => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_EXPORT_DOCUMENT).to_string(),
                CommandId::CloseDocument => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_CLOSE_DOCUMENT).to_string(),
                CommandId::KeyboardShortcuts => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_KEYBOARD_SHORTCUTS).to_string(),
                CommandId::About => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_ABOUT).to_string(),
                CommandId::Website => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_WEBSITE).to_string(),
                CommandId::SourceCode => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_SOURCE_CODE).to_string(),
                CommandId::ZoomIn | CommandId::ZoomOut => localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_ZOOM_IN).to_string(),
                CommandId::RotateLeft | CommandId::RotateRight => {
                    localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_ROTATE_LEFT).to_string()
                }
                CommandId::FlipHorizontal | CommandId::FlipVertical => {
                    localization.text(MessageId::WORKSPACE_TOOL_DESCRIPTION_FLIP_HORIZONTAL).to_string()
                }
            }
            .into(),
            control.icon(),
            )
        },
        ToolbarControl::Brush { id } => {
            let choice = crate::tools::brush_catalog_localized(localization).find(|b| b.id == id);
            (
                choice.as_ref().map(|b| b.label.to_string()).unwrap_or_else(|| localization.text(MessageId::WORKSPACE_TOOL_UNKNOWN_BRUSH).to_string()),
                customization_text(localization, MessageId::WORKSPACE_TOOL_BRUSH_PRESET, &[("category", choice.as_ref().map(|b| b.category.to_string()).unwrap_or_else(|| localization.text(MessageId::WORKSPACE_TOOL_BRUSH_CATEGORY).to_string()))]),
                control.icon(),
            )
        }
        ToolbarControl::Size { pixels } => (
            { let mut args = FluentArgs::new(); args.set("pixels", pixels); localization.format(MessageId::WORKSPACE_TOOL_BRUSH_SIZE, &args) },
            localization.text(MessageId::WORKSPACE_TOOL_SIZE_DESCRIPTION).to_string(),
            control.icon(),
        ),
        ToolbarControl::Color => (
            localization.text(MessageId::WORKSPACE_TOOL_COLOR).to_string(),
            localization.text(MessageId::WORKSPACE_TOOL_COLOR_DESCRIPTION).to_string(),
            control.icon(),
        ),
        ToolbarControl::ColorPicker => (
            localization.text(MessageId::WORKSPACE_TOOL_COLOR_PICKER).to_string(),
            localization.text(MessageId::WORKSPACE_TOOL_COLOR_PICKER_DESCRIPTION).to_string(),
            control.icon(),
        ),
        ToolbarControl::Opacity => (
            localization.text(MessageId::WORKSPACE_TOOL_OPACITY).to_string(),
            localization.text(MessageId::WORKSPACE_TOOL_OPACITY_DESCRIPTION).to_string(),
            control.icon(),
        ),
        ToolbarControl::BrushSizeSlider => (
            localization.text(MessageId::WORKSPACE_TOOL_SIZE_SLIDER).to_string(), localization.text(MessageId::WORKSPACE_TOOL_SIZE_SLIDER_DESCRIPTION).to_string(), control.icon(),
        ),
        ToolbarControl::BrushOpacitySlider => (
            localization.text(MessageId::WORKSPACE_TOOL_OPACITY_SLIDER).to_string(), localization.text(MessageId::WORKSPACE_TOOL_OPACITY_SLIDER_DESCRIPTION).to_string(), control.icon(),
        ),
        ToolbarControl::ToolOptions { .. } => (
            localization.text(MessageId::WORKSPACE_TOOL_OPTIONS).to_string(), localization.text(MessageId::WORKSPACE_TOOL_OPTIONS_DESCRIPTION).to_string(), control.icon(),
        ),
        ToolbarControl::Panel { panel } => (
            customization_text(localization, MessageId::WORKSPACE_HISTORY_PANEL, &[("name", panel.localized_label(localization).to_string())]),
            localization.text(MessageId::WORKSPACE_TOOL_PANEL_DESCRIPTION).to_string(),
            control.icon(),
        ),
        ToolbarControl::Divider => (
            localization.text(MessageId::WORKSPACE_TOOL_DIVIDER).to_string(),
            localization.text(MessageId::WORKSPACE_TOOL_DIVIDER_DESCRIPTION).to_string(),
            control.icon(),
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
fn tool_available(control: ToolbarControl, platform: Platform) -> bool {
    match control {
        ToolbarControl::Command { command } => command.available_on(platform),
        ToolbarControl::Brush { id } => preset(id).is_ok(),
        ToolbarControl::Size { pixels } => BRUSH_SIZES.iter().any(|size| *size as u16 == pixels),
        ToolbarControl::Panel { panel } => Panel::ALL.contains(&panel) && panel.kind() == PanelKind::Content,
        _ => true,
    }
}
pub(crate) fn tool_catalog(platform: Platform) -> Vec<ToolChoice> {
    tool_catalog_localized(platform, &Localizer::shared(UiLanguage::English))
}
pub(crate) fn tool_catalog_localized(platform: Platform, localization: &Localizer) -> Vec<ToolChoice> {
    CommandId::ALL
        .into_iter()
        .filter(|id| id.available_on(platform))
        .map(|command| ToolbarControl::Command { command })
        .chain([ToolbarControl::Color, ToolbarControl::Opacity])
        .chain([ToolbarControl::ColorPicker, ToolbarControl::BrushSizeSlider, ToolbarControl::BrushOpacitySlider, ToolbarControl::TOOL_OPTIONS])
        .chain([ToolbarControl::Divider])
        .chain(
            Panel::ALL
                .into_iter()
                .filter(move |p| p.kind() == PanelKind::Content)
                .map(|panel| ToolbarControl::Panel { panel }),
        )
        .chain(crate::tools::brush_catalog_localized(localization).map(|b| ToolbarControl::Brush { id: b.id }))
        .chain(
            BRUSH_SIZES
                .iter()
                .map(|s| ToolbarControl::Size { pixels: *s as u16 }),
        )
        .map(|control| tool_choice_localized(control, localization))
        .collect()
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolDestination {
    NewToolbar {
        group: Option<u32>,
        name: String,
    },
    Insert {
        panel: Panel,
        before: Option<u32>,
    },
    Header {
        zone: HeaderZone,
        before: Option<u32>,
    },
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ToolPicker {
    pub destination: ToolDestination,
    pub query: String,
    pub selected: Vec<ToolbarControl>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ToolPickerView {
    pub title: std::sync::Arc<str>,
    pub confirm_label: std::sync::Arc<str>,
    pub name: Option<String>,
    pub name_label: std::sync::Arc<str>,
    pub search_hint: std::sync::Arc<str>,
    pub query: String,
    pub choices: Vec<ToolChoice>,
    pub selected_count: usize,
    pub can_confirm: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PanelControlView {
    pub control: PanelControl,
    pub label: std::sync::Arc<str>,
    pub visible_in_panel: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct TileView {
    pub id: u32,
    #[serde(flatten)]
    pub choice: ToolChoice,
    pub enabled: bool,
    pub tooltip: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<crate::ToolbarComponentView>,
}
#[derive(Clone, Debug, Serialize)]
pub struct PanelView {
    pub id: Panel,
    pub title: String,
    pub icon: &'static str,
    pub tab: TabPresentation,
    #[serde(flatten)]
    pub tile: TilePresentation,
    pub toolbar_options: Vec<Vec<ContextMenuItem>>,
    pub expanded: bool,
    pub configuration_title: String,
    pub configuration_hint: std::sync::Arc<str>,
    pub controls: Vec<PanelControlView>,
    pub tiles: Vec<TileView>,
}

pub(crate) struct PanelCopy {
    pub content: PanelContent,
    pub tile_style: TileStyle,
    pub title: std::sync::Arc<str>,
    pub configuration_title: std::sync::Arc<str>,
    toolbar_options: Vec<Vec<ContextMenuItem>>,
    tiles: Vec<(u32, ToolChoice, std::sync::Arc<str>)>,
}
impl PanelCopy {
    pub fn new(state: &UiState, config: &PanelConfig) -> Self {
        let localization = &state.localization;
        let toolbar_options = if config.id.kind() == PanelKind::Tiles {
            let mut options = state.workspace.layout.toolbar_options_localized(config.id, localization).expect("validated toolbar");
            options[0].remove(0);
            ContextMenu { title: String::new(), sections: options }
                .with_shortcuts_localized(&state.settings, state.platform, localization).sections
        } else { Vec::new() };
        let tiles = config.tiles().iter().map(|tile| {
            let choice = tool_choice_localized(tile.control, localization);
            let tooltip = if matches!(tile.control, ToolbarControl::Command { .. }) { choice.label.clone() }
                else { tile.control.action().map_or_else(|| choice.label.clone(), |action|
                    state.settings.action_tooltip_localized(&choice.label, &action, state.platform, localization)) };
            (tile.id, choice, std::sync::Arc::from(tooltip))
        }).collect();
        Self {
            content: config.content.clone(), tile_style: config.tile_style,
            title: config.title_localized(localization).into(),
            configuration_title: customization_text(localization, MessageId::WORKSPACE_CONFIGURE_TITLE, &[("name", config.menu_name(localization))]).into(),
            toolbar_options, tiles,
        }
    }
}

pub(crate) fn panel_view(state: &UiState, panel: Panel, copy: &PanelCopy) -> Result<PanelView, String> {
    let localization = &state.localization;
    let config = state.workspace.layout.panel(panel)?;
    let expanded = state.customization.expanded == Some(panel);
    let controls = PanelControl::available(panel)
        .iter()
        .map(|&control| PanelControlView {
            control,
            label: control.localized_label(localization),
            visible_in_panel: config.shows(control),
        })
        .collect();
    let tiles = config
        .tiles()
        .iter()
        .map(|tile| {
            let (_, choice, tooltip) = copy.tiles.iter().find(|(id, choice, _)| *id == tile.id && choice.control == tile.control).ok_or("The tool no longer exists")?;
            let mut choice = choice.clone();
            let mut enabled = true;
            choice.selected = match tile.control {
                ToolbarControl::ColorPicker => {
                    let state = crate::tool_state(state, tile.control);
                    enabled = state.0;
                    state.1
                }
                ToolbarControl::Command { command } => {
                    if let Some(command) = state.commands.iter().find(|c| c.id == command) {
                        enabled = command.enabled;
                        choice.label = command.label.to_string();
                        choice.icon = command.icon.unwrap_or(choice.icon);
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
                _ => false,
            };
            Ok(TileView {
                id: tile.id,
                component: state.toolbar_component(tile.control),
                tooltip: match tile.control {
                    ToolbarControl::Command { command } => state.commands.iter().find(|entry| entry.id == command)
                        .map_or_else(|| tooltip.to_string(), |entry| entry.tooltip.clone()),
                    _ => tooltip.to_string(),
                },
                choice,
                enabled,
            })
        })
        .collect::<Result<Vec<_>, &str>>()?;
    Ok(PanelView {
        id: panel,
        title: copy.title.to_string(),
        icon: config.icon(),
        tab: state.workspace.layout.tab_presentation(panel),
        tile: config.tile_style.into(),
        toolbar_options: copy.toolbar_options.clone(),
        expanded,
        configuration_title: copy.configuration_title.to_string(),
        configuration_hint: if panel.kind() == crate::PanelKind::Tiles {
            localization.text(MessageId::WORKSPACE_TOOLBAR_CONFIGURATION_HINT)
        } else {
            localization.text(MessageId::WORKSPACE_PANEL_CONFIGURATION_HINT)
        },
        controls,
        tiles,
    })
}
impl ToolPicker {
    pub(crate) fn validate(&self, layout: &DockLayout) -> Result<(), String> {
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
            ToolDestination::Header { zone, before } => {
                if self.selected.iter().any(|c| c.is_component()) {
                    return Err("Place this component in a toolbar".into());
                }
                layout.header.insertion(*zone, *before)?;
                if layout.header.entries().count() + self.selected.len() > 128 {
                    return Err("Too many title-bar items".into());
                }
            }
        }
        if self.selected.is_empty() {
            return Err("Select at least one tool".into());
        }
        Ok(())
    }
    pub fn view(&self, layout: &DockLayout, platform: Platform) -> ToolPickerView {
        self.view_localized(layout, platform, &Localizer::shared(UiLanguage::English))
    }
    pub fn view_localized(&self, layout: &DockLayout, platform: Platform, localization: &Localizer) -> ToolPickerView {
        let name = match &self.destination {
            ToolDestination::NewToolbar { name, .. } => Some(name.clone()),
            _ => None,
        };
        let words = crate::search::normalize(&self.query);
        let choices = tool_catalog_localized(platform, localization)
            .into_iter()
            .filter(|c| !matches!(self.destination, ToolDestination::Header { .. }) || !c.control.is_component())
            .filter_map(|mut choice| {
                choice.selected = self.selected.contains(&choice.control);
                let english = tool_choice(choice.control);
                let text = crate::search::normalize(&format!("{} {} {} {}", choice.label, choice.description, english.label, english.description));
                words
                    .split_whitespace()
                    .all(|w| text.contains(w))
                    .then_some(choice)
            })
            .collect();
        ToolPickerView {
            title: if matches!(self.destination, ToolDestination::Header { .. }) {
                localization.text(MessageId::WORKSPACE_PICKER_HEADER_TITLE)
            } else if name.is_some() {
                localization.text(MessageId::WORKSPACE_PICKER_NEW_TOOLBAR_TITLE)
            } else {
                localization.text(MessageId::WORKSPACE_PICKER_ADD_TOOLS)
            },
            confirm_label: if name.is_some() {
                localization.text(MessageId::WORKSPACE_PICKER_CREATE_TOOLBAR)
            } else {
                localization.text(MessageId::WORKSPACE_PICKER_ADD_TOOLS)
            },
            name,
            name_label: localization.text(MessageId::WORKSPACE_TOOLBAR_NAME),
            search_hint: localization.text(MessageId::WORKSPACE_PICKER_SEARCH_HINT),
            query: self.query.clone(),
            choices,
            selected_count: self.selected.len(),
            can_confirm: self.validate(layout).is_ok(),
            error: self.error.clone().or_else(|| match &self.destination {
                ToolDestination::NewToolbar { name, .. } => {
                    layout.validate_toolbar_name(name).err()
                }
                _ if !self.selected.is_empty() => self.validate(layout).err(),
                _ => None,
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ToolbarOperation {
    Rename,
    Duplicate,
    Delete,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ToolbarPrompt {
    panel: Panel,
    operation: ToolbarOperation,
    name: String,
    error: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ToolbarPromptView {
    pub title: std::sync::Arc<str>,
    pub message: String,
    pub name: Option<String>,
    pub name_label: std::sync::Arc<str>,
    pub confirm_label: std::sync::Arc<str>,
    pub cancel_label: std::sync::Arc<str>,
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
        self.view_localized(layout, undo_shortcut, &Localizer::shared(UiLanguage::English))
    }
    pub fn view_localized(&self, layout: &DockLayout, undo_shortcut: &str, localization: &Localizer) -> ToolbarPromptView {
        let (title, confirm_label, destructive) = match self.operation {
            ToolbarOperation::Rename => (localization.text(MessageId::WORKSPACE_TOOLBAR_RENAME_TITLE), localization.text(MessageId::WORKSPACE_TOOLBAR_RENAME_CONFIRM), false),
            ToolbarOperation::Duplicate => (localization.text(MessageId::WORKSPACE_TOOLBAR_DUPLICATE_TITLE), localization.text(MessageId::WORKSPACE_TOOLBAR_DUPLICATE_CONFIRM), false),
            ToolbarOperation::Delete => (localization.text(MessageId::WORKSPACE_TOOLBAR_DELETE_TITLE), localization.text(MessageId::WORKSPACE_TOOLBAR_DELETE_CONFIRM), true),
        };
        ToolbarPromptView {
            title,
            confirm_label,
            destructive,
            cancel_label: localization.text(MessageId::COMMON_CANCEL),
            name_label: localization.text(MessageId::WORKSPACE_TOOLBAR_NAME),
            message: if destructive {
                customization_text(localization,
                    if undo_shortcut.is_empty() { MessageId::WORKSPACE_TOOLBAR_DELETE_MESSAGE } else { MessageId::WORKSPACE_TOOLBAR_DELETE_SHORTCUT_MESSAGE },
                    &[("name", self.name.clone()), ("shortcut", undo_shortcut.to_owned())])
            } else {
                String::new()
            },
            name: (!destructive).then(|| self.name.clone()),
            can_confirm: self.validate(layout).is_ok(),
            error: self.error.clone().or_else(|| self.validate(layout).err()),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
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
    pub title: std::sync::Arc<str>,
    pub close_label: std::sync::Arc<str>,
    pub description: std::sync::Arc<str>,
    pub empty_label: std::sync::Arc<str>,
    pub toolbars: Vec<ManagedToolbar>,
    pub selected: Option<Panel>,
    pub delete_label: std::sync::Arc<str>,
    pub delete_action: Option<CustomizationAction>,
}
impl ToolbarManager {
    pub fn view(&self, layout: &DockLayout) -> ToolbarManagerView {
        self.view_localized(layout, &Localizer::shared(UiLanguage::English))
    }
    pub fn view_localized(&self, layout: &DockLayout, localization: &Localizer) -> ToolbarManagerView {
        let toolbars: Vec<_> = layout
            .panels
            .iter()
            .filter(|p| p.id.kind() == PanelKind::Tiles)
            .map(|p| {
                let count = p.tiles().len();
                let mut args = FluentArgs::new(); args.set("count", count);
                let summary = localization.format(if layout.panel_group(p.id).is_some() { MessageId::WORKSPACE_TOOLBAR_SUMMARY_VISIBLE } else { MessageId::WORKSPACE_TOOLBAR_SUMMARY_HIDDEN }, &args);
                ManagedToolbar {
                    panel: p.id,
                    title: p.title_localized(localization),
                    subtitle: summary,
                    icon: p.icon(),
                }
            })
            .collect();
        let selected = self
            .selected
            .filter(|id| toolbars.iter().any(|p| p.panel == *id));
        ToolbarManagerView {
            title: localization.text(MessageId::WORKSPACE_TOOLBAR_MANAGER_TITLE),
            close_label: localization.text(MessageId::WORKSPACE_TOOLBAR_MANAGER_CLOSE),
            description: localization.text(MessageId::WORKSPACE_TOOLBAR_MANAGER_DESCRIPTION),
            empty_label: localization.text(MessageId::WORKSPACE_TOOLBAR_MANAGER_EMPTY),
            toolbars,
            selected,
            delete_label: localization.text(MessageId::WORKSPACE_TOOLBAR_MANAGER_DELETE),
            delete_action: selected.map(|panel| CustomizationAction::DeleteToolbar { panel }),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CustomizationState {
    pub header_editing: bool,
    /// A single preview baseline, not an editor undo stack. Never persisted.
    #[serde(skip)]
    pub(crate) header_original: Option<(HeaderLayout, CanvasInfoLayout)>,
    pub expanded: Option<Panel>,
    pub drawer: Option<ContentDrawer>,
    pub column_drawers: Vec<ContentDrawer>,
    #[serde(skip)]
    pub(crate) drawer_tiles: Vec<DrawerTileMeasurement>,
    #[serde(skip)]
    pub(crate) column_drawer_bounds: Vec<ColumnDrawerMeasurement>,
    pub picker: Option<ToolPicker>,
    pub control: Option<PanelControl>,
    pub toolbar_prompt: Option<ToolbarPrompt>,
    pub toolbar_manager: Option<ToolbarManager>,
}
impl CustomizationState {
    pub(crate) fn begin_header(&mut self, layout: &DockLayout) {
        if !self.header_editing {
            self.header_original = Some((layout.header.clone(), layout.canvas_info.clone()));
        }
        self.header_editing = true;
        self.drawer = None;
        self.picker = None;
    }
    pub(crate) fn committed_header(&self, layout: &mut DockLayout) {
        if let Some((header, info)) = &self.header_original {
            layout.header = header.clone();
            layout.canvas_info = info.clone();
        }
    }
    pub(crate) fn cancel_header(&mut self, layout: &mut DockLayout) -> bool {
        let editing = self.header_editing;
        self.committed_header(layout);
        self.header_original = None;
        self.header_editing = false;
        self.close_header_picker();
        editing
    }
    fn close_header_picker(&mut self) {
        if self
            .picker
            .as_ref()
            .is_some_and(|p| matches!(p.destination, ToolDestination::Header { .. }))
        {
            self.picker = None;
        }
    }
    pub fn has_drawer(&self) -> bool {
        self.expanded.is_some() || self.drawer.is_some() || !self.column_drawers.is_empty()
    }
    pub fn is_open(&self) -> bool {
        self.has_drawer() || self.blocks_shortcuts()
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
    ) -> Result<u32, String> {
        self.edit_localized(layout, action, platform, viewport, &Localizer::shared(UiLanguage::English))
    }
    pub(crate) fn edit_localized(&mut self, layout: &mut DockLayout, action: CustomizationAction, platform: Platform, viewport: [f32; 2], localization: &Localizer) -> Result<u32, String> {
        use CustomizationAction::*;
        let mut changed = regions::CUSTOMIZATION;
        let header_picker_action = self
            .picker
            .as_ref()
            .is_some_and(|p| matches!(p.destination, ToolDestination::Header { .. }))
            && matches!(
                action,
                PickerSearch { .. } | PickerSelect { .. } | ConfirmTools | CancelTools
            );
        if !matches!(action, Header { .. }) && !header_picker_action && self.cancel_header(layout) {
            changed |= regions::LAYOUT;
        }
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
            Header { action } => {
                use HeaderAction as H;
                match action {
                    H::Edit { editing } => {
                        if editing {
                            self.begin_header(layout);
                        } else {
                            self.header_editing = false;
                            self.header_original = None;
                            self.close_header_picker();
                            changed |= regions::LAYOUT;
                        }
                    }
                    H::Cancel => {
                        self.cancel_header(layout);
                        self.drawer = None;
                        changed |= regions::LAYOUT;
                    }
                    H::InsertTools { zone, before } => {
                        if !self.header_editing {
                            return Err("Open title-bar customization first".into());
                        }
                        layout.header.insertion(zone, before)?;
                        self.picker = Some(ToolPicker {
                            destination: ToolDestination::Header { zone, before },
                            query: String::new(),
                            selected: Vec::new(),
                            error: None,
                        });
                    }
                    action => {
                        match action {
                            H::SetSize { size } => layout.header.size = size,
                            H::Add { zone, before, item } => {
                                if let HeaderItem::Tool { control } = item
                                    && !tool_available(control, platform)
                                {
                                    return Err("This tool is not available".into());
                                }
                                layout.header.add(zone, before, &[item])?;
                            }
                            H::Move { id, zone, before } => {
                                layout.header.move_item(id, zone, before)?
                            }
                            H::Remove { id } => layout.header.remove(id)?,
                            H::CanvasInfo { visible } => {
                                layout.canvas_info = CanvasInfoLayout { visible }
                            }
                            _ => unreachable!(),
                        }
                        changed |= regions::LAYOUT;
                    }
                }
            }
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
            RestoreBuiltinToolbar { panel, group } => {
                layout.restore_builtin_toolbar(panel, group)?;
                changed |= regions::LAYOUT;
            }
            SetToolOptionsStyle { panel, tile, style } => {
                let config = layout.panels.iter_mut().find(|p| p.id == panel).ok_or("Choose a toolbar")?;
                let tile = config.tiles_mut()?.iter_mut()
                    .find(|t| t.id == tile && t.control.options_style().is_some()).ok_or("Choose tool options")?;
                tile.control = ToolbarControl::ToolOptions { style };
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
                let title = layout.panel(panel)?.title_localized(localization);
                let name = if matches!(operation, ToolbarOperation::Duplicate) {
                    layout.unused_toolbar_name(&customization_text(localization, MessageId::WORKSPACE_COPY_NAME,
                        &[("name", title.chars().take(59).collect())]).chars().take(64).collect::<String>())
                } else {
                    title
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
                // Configuration extends an ordinary panel, not the compact
                // column's content drawer. Reveal its normal source first.
                if let Some(column) = layout.collapsed_column_for_group(group) {
                    layout.set_column_collapsed(column, false, viewport)?;
                }
                layout.select_tab(group, panel)?;
                self.picker = None;
                self.control = None;
                self.toolbar_manager = None;
                self.expanded = Some(panel);
                self.drawer = None;
                changed |= regions::LAYOUT;
            }
            ToggleToolDrawer { .. } | ToggleHeaderDrawer { .. } => {
                let mut drawer = match action {
                    ToggleToolDrawer { anchor } => ContentDrawer::for_tile(layout, anchor)?,
                    ToggleHeaderDrawer { id } => ContentDrawer::for_header(layout, id)?,
                    _ => unreachable!(),
                };
                drawer.configure_picker(layout);
                if drawer.columns == [vec![Panel::Color]] {
                    drawer.columns[0].push(Panel::Palettes);
                }
                if self
                    .drawer_placement(&drawer, layout, viewport, &vec![0.0; drawer.columns.len()])
                    .is_none()
                {
                    return Err("The originating tile is not visible".into());
                }
                let close = self
                    .drawer
                    .as_ref()
                    .is_some_and(|d| d.anchor == drawer.anchor);
                self.expanded = None;
                self.picker = None;
                self.control = None;
                self.toolbar_manager = None;
                self.drawer = (!close).then_some(drawer);
            }
            SetColumnCollapsed { group, collapsed } => {
                layout.set_column_collapsed(group, collapsed, viewport)?;
                self.expanded = None;
                changed |= regions::LAYOUT;
            }
            ToggleColumnDrawer { group, panel } => {
                let column = layout
                    .collapsed_column_for_group(group)
                    .ok_or("The column is not collapsed")?;
                let settings = layout.column_stack(column);
                if !settings.drawers {
                    let close = settings.open_column == Some(column) && layout.active_panel(panel) == Some(panel);
                    layout.select_tab(group, panel)?;
                    layout.column_stack_mut(column).open_column = (!close).then_some(column);
                    self.column_drawers.retain(|d| !matches!(d.anchor, DrawerAnchor::Column { column: id, .. }
                        if layout.column_stack(id).column == settings.column));
                    self.expanded = None;
                    self.drawer = None;
                    return Ok(changed | regions::LAYOUT);
                }
                self.column_drawers.retain(|d| !matches!(d.anchor, DrawerAnchor::Column { column: id, .. }
                    if id != column && layout.column_stack(id).column == settings.column));
                let mut next = ContentDrawer::for_column(layout, group, panel)?;
                let DrawerAnchor::Column { column, .. } = next.anchor else {
                    unreachable!()
                };
                self.drawer_tiles.retain(|m| m.column != column);
                self.column_drawer_bounds.retain(|m| m.group != group);
                let existing = self.column_drawers.iter().position(
                    |d| matches!(d.anchor, DrawerAnchor::Column { column: id, .. } if id == column),
                );
                let previous = existing.map(|i| self.column_drawers.remove(i));
                let close = previous.as_ref().is_some_and(|d| matches!(d.anchor, DrawerAnchor::Column { group: id, origin, .. } if id == group && origin == panel));
                if !close {
                    layout.select_tab(group, panel)?;
                    if let Some(old) =
                        previous.filter(|d| d.tabs.as_ref().is_some_and(|t| t.group == group))
                    {
                        next.anchor = old.anchor;
                    }
                    self.column_drawers.push(next);
                }
                self.expanded = None;
                self.drawer = None;
                changed |= regions::LAYOUT;
            }
            SetColumnDrawers { column, drawers } => {
                if !layout.is_top_level_column(layout.column_stack(column).column) {
                    return Err("Column settings require a top-level column".into());
                }
                if layout.node(column).is_none() { return Err("Unknown column".into()); }
                let root = layout.column_stack(column).column;
                let s = layout.column_stack_mut(column);
                s.drawers = drawers;
                s.open_column = None;
                self.column_drawers.retain(|d| !matches!(d.anchor, DrawerAnchor::Column { column: id, .. }
                    if layout.column_stack(id).column == root));
                changed |= regions::LAYOUT;
            }
            SetColumnAutoHide { column, auto_hide } => {
                if !layout.is_top_level_column(layout.column_stack(column).column) {
                    return Err("Column settings require a top-level column".into());
                }
                if layout.node(column).is_none() {
                    return Err("Unknown column".into());
                }
                layout.column_stack_mut(column).auto_hide = auto_hide;
                changed |= regions::LAYOUT;
            }
            ApplyColumnStack { column } => {
                if !layout.is_top_level_column(layout.column_stack(column).column) {
                    return Err("Column settings require a top-level column".into());
                }
                if layout.node(column).is_none() { return Err("Unknown column".into()); }
                let source = layout.column_stack(column);
                for root in layout.column_roots() {
                    let s = layout.column_stack_mut(root);
                    s.drawers = source.drawers;
                    s.auto_hide = source.auto_hide;
                    if s.drawers { s.open_column = None; }
                }
                changed |= regions::LAYOUT;
            }
            CloseColumn { column } => {
                let root = layout.column_stack(column).column;
                self.column_drawers.retain(|d| !matches!(d.anchor, DrawerAnchor::Column { column: id, .. }
                    if layout.column_stack(id).column == root));
                if let Some(s) = layout.column_stacks.iter_mut().find(|s| s.column == root) {
                    s.open_column = None;
                }
                changed |= regions::LAYOUT;
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
                if control != PanelControl::BrushColor {
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
                    let mut args = FluentArgs::new(); args.set("number", suffix);
                    let name = localization.format(MessageId::WORKSPACE_TOOLBAR_NUMBERED, &args);
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
                if !tool_available(control, platform) {
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
                        ToolDestination::Header { zone, before } => layout.header.add(
                            *zone,
                            *before,
                            &picker
                                .selected
                                .iter()
                                .map(|control| HeaderItem::Tool { control: *control })
                                .collect::<Vec<_>>(),
                        ),
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
    use crate::session::test_support::tile_ids;
    use super::*;
    const VIEWPORT: [f32; 2] = [1200.0, 900.0];
    const PEN: ToolbarControl = ToolbarControl::Command {
        command: CommandId::Brush,
    };
    const ERASE: ToolbarControl = ToolbarControl::Command {
        command: CommandId::Eraser,
    };

    #[test]
    fn localized_customization_views_preserve_literal_titles_and_layout_data() {
        let mut layout = DockLayout::default();
        layout.rename_toolbar(Panel::Toolbar, "Tools").unwrap();
        let custom = layout.add_toolbar(None, "日本語 한글 {draft} 🎨", &[PEN]).unwrap();
        let before = serde_json::to_string(&layout).unwrap();
        for language in UiLanguage::ALL {
            let localization = Localizer::shared(language);
            let config = layout.panel(Panel::Toolbar).unwrap();
            assert_eq!(config.custom_name(), Some("Tools"));
            assert_eq!(config.title_localized(&localization), "Tools");
            let manager = ToolbarManager::default().view_localized(&layout, &localization);
            assert!(manager.toolbars.iter().any(|toolbar| toolbar.title == "日本語 한글 {draft} 🎨"));
            let options = layout.toolbar_options_localized(custom, &localization).unwrap();
            assert!(options.iter().flatten().any(|item| item.label.contains("日本語 한글 {draft} 🎨")));
            assert!(std::sync::Arc::ptr_eq(&manager.title, &localization.text(MessageId::WORKSPACE_TOOLBAR_MANAGER_TITLE)));
            assert_eq!(serde_json::to_string(&layout).unwrap(), before);
        }
        let default = PanelConfig::defaults().into_iter().find(|panel| panel.id == Panel::Toolbar).unwrap();
        assert!(default.custom_name().is_none());
        let localization = Localizer::shared(UiLanguage::English);
        assert_eq!(default.title_localized(&localization), localization.text(MessageId::WORKSPACE_PANEL_TOOLBAR).to_string());
    }

    #[test]
    fn localized_picker_search_normalizes_display_and_english_aliases() {
        let layout = DockLayout::default();
        let picker = ToolPicker {
            destination: ToolDestination::Insert { panel: Panel::Toolbar, before: None },
            query: "ＢＲＵＳＨ ＳＩＺＥ".into(), selected: vec![PEN], error: None,
        };
        for language in UiLanguage::ALL {
            let localization = Localizer::shared(language);
            let view = picker.view_localized(&layout, Platform::Gtk, &localization);
            assert!(view.choices.iter().any(|choice| choice.control == ToolbarControl::BrushSizeSlider));
            assert_eq!(view.selected_count, 1);
            assert!(std::sync::Arc::ptr_eq(&view.search_hint, &localization.text(MessageId::WORKSPACE_PICKER_SEARCH_HINT)));
        }
    }

    #[test]
    fn toolbar_creation_localizes_proposal_then_keeps_it_as_literal_data() {
        for language in UiLanguage::ALL {
            let localization = Localizer::shared(language);
            let mut layout = DockLayout::default();
            let mut state = CustomizationState::default();
            state.edit_localized(&mut layout, CustomizationAction::NewToolbar { group: None }, Platform::Gtk, VIEWPORT, &localization).unwrap();
            let ToolDestination::NewToolbar { name, .. } = &state.picker.as_ref().unwrap().destination else { panic!() };
            let saved_name = name.clone();
            assert!(!saved_name.contains(['\u{2068}', '\u{2069}']));
            state.edit_localized(&mut layout, CustomizationAction::PickerSelect { control: PEN, selected: true }, Platform::Gtk, VIEWPORT, &localization).unwrap();
            state.edit_localized(&mut layout, CustomizationAction::ConfirmTools, Platform::Gtk, VIEWPORT, &localization).unwrap();
            let config = layout.panels.last().unwrap();
            assert_eq!(config.custom_name(), Some(saved_name.as_str()));
            for display_language in UiLanguage::ALL {
                assert_eq!(config.title_localized(&Localizer::shared(display_language)), saved_name);
            }
            layout.validate().unwrap();
        }
    }

    #[test]
    fn medium_tiles_follow_shared_docking_and_roundtrip() {
        for style in [TileStyle::Medium, TileStyle::MediumLabeled] {
            for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
                let mut workspace = WorkspaceState::default();
                workspace
                    .layout
                    .set_tile_style(Panel::Toolbar, style, VIEWPORT)
                    .unwrap();
                workspace
                    .layout
                    .move_panel(
                        VIEWPORT,
                        Panel::Toolbar,
                        DockTarget::Edge { edge, outer: true },
                    )
                    .unwrap();
                workspace.validate().unwrap();
                let resolved = workspace.layout.workspace(
                    VIEWPORT[0],
                    VIEWPORT[1],
                    crate::HEADER_HEIGHT,
                    crate::STATUS_HEIGHT,
                );
                let group = resolved
                    .groups
                    .iter()
                    .find(|g| g.panels.contains(&Panel::Toolbar))
                    .unwrap();
                let tiles = &group.tiles.as_ref().unwrap().tiles;
                assert!(!tiles.is_empty());
                for tile in tiles {
                    assert_eq!([tile.width, tile.height], style.size());
                    assert!(tile.x >= 0.0 && tile.y >= 0.0);
                    assert!(tile.x + tile.width <= group.bounds.width + 0.01);
                    assert!(tile.y + tile.height <= group.bounds.height + 0.01);
                }
                let json = serde_json::to_string(&workspace).unwrap();
                assert!(json.contains(&format!(
                    "\"tile_style\":{}",
                    serde_json::to_string(&style).unwrap()
                )));
                assert_eq!(
                    serde_json::from_str::<WorkspaceState>(&json).unwrap(),
                    workspace
                );
            }
        }
    }

    #[test]
    fn toolbar_context_menu_offers_every_tile_style() {
        let layout = DockLayout::default();
        let target = ContextTarget::Ribbon { panel: Panel::Toolbar };
        let context = layout.context_menu_on(target, Platform::Gtk).unwrap();
        let options = layout.toolbar_options(Panel::Toolbar).unwrap();
        assert_eq!(serde_json::to_value(context.sections).unwrap(), serde_json::to_value(&options).unwrap());
        for style in [TileStyle::Medium, TileStyle::MediumLabeled] {
            assert!(options.iter().flatten().any(|item| item.label == style.label()));
            let mut changed = layout.clone();
            CustomizationState::default()
                .edit(&mut changed, CustomizationAction::SetTileStyle { panel: Panel::Toolbar, style }, Platform::Gtk, VIEWPORT)
                .unwrap();
            assert_eq!(changed.panel(Panel::Toolbar).unwrap().tile_style, style);
        }
        assert_eq!(TileStyle::Medium.icon_size(), 24);
        assert_eq!(TileStyle::MediumLabeled.label_lines(), 2);
    }

    #[test]
    fn customized_workspace_state_roundtrips_and_reset_docking_keeps_panels() {
        let mut state = WorkspaceState::default();
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
        state
            .layout
            .reset_docking(crate::Platform::Gtk)
            .unwrap();
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
        let group = state.layout.panel_group(Panel::Layers).unwrap();
        state
            .layout
            .add_toolbar(Some(group), "Second", &[PEN])
            .unwrap();
        state.validate().unwrap();
    }

    #[test]
    fn toolbar_names_and_invalid_targets_fail_atomically() {
        let mut layout = DockLayout::default();
        layout
            .add_toolbar(Some(8), "  Favorites  ", &[PEN])
            .unwrap();
        for name in ["favorites", "FAVORITES", " ", "a\nb"] {
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
        let ids = tile_ids(&layout, custom);
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
            tile_ids(&layout, custom),
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
        layout.add_toolbar(None, "Tools", &[]).unwrap();
        let mut state = CustomizationState::default();
        let edit = |state: &mut CustomizationState, layout: &mut DockLayout, action| {
            state
                .edit(layout, action, Platform::Gtk, [1200.0, 900.0])
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
        assert_eq!(layout.panels.len(), original.panels.len() + 1);
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
        let mut layout = DockLayout::default();
        let mut state = CustomizationState::default();
        for (style, active, inactive) in styles {
            let menu = layout
                .context_menu_on(ContextTarget::Group { group: 8 }, Platform::Gtk)
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
                .edit(&mut layout, action, Platform::Gtk, [1200.0, 900.0])
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
                .context_menu_on(ContextTarget::Group { group: 8 }, Platform::Gtk)
                .unwrap();
            let checked: Vec<_> = menu.sections[0]
                .iter()
                .filter(|i| i.selected == Some(true))
                .collect();
            assert_eq!(checked.len(), 1);
            assert_eq!(checked[0].label, style.label());
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
            )
            .unwrap();
        assert!(
            layout.context_menu_on(group, Platform::Gtk).unwrap().sections[0]
                .iter()
                .any(|i| i.label == "Names only" && i.selected == Some(true))
        );
        let panel = layout
            .context_menu_on(ContextTarget::Panel {
                panel: Panel::Brushes,
            }, Platform::Gtk)
            .unwrap();
        assert!(
            panel
                .sections
                .iter()
                .flatten()
                .any(|i| i.label == "Configure Tool Set panel…")
        );
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
                .context_menu_on(ContextTarget::Tile {
                    panel: custom,
                    tile
                }, Platform::Gtk)
                .unwrap()
                .sections[0]
                .iter()
                .map(|i| i.label.as_str())
                .collect::<Vec<_>>(),
            ["Remove Tool", "Insert Tools…"]
        );
        assert_eq!(
            layout
                .context_menu_on(ContextTarget::Ribbon { panel: custom }, Platform::Gtk)
                .unwrap()
                .sections[0][0]
                .label,
            "Configure Paint toolbar…"
        );
        assert!(
            layout
                .context_menu_on(ContextTarget::Ribbon {
                    panel: Panel::Layers
                }, Platform::Gtk)
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
                + CommandId::ALL
                    .iter()
                    .filter(|id| id.available_on(Platform::Gtk) && !id.available_on(Platform::Web))
                    .count()
        );
        assert!(native.iter().any(|c| c.control == ToolbarControl::ColorPicker));
        assert!(web.iter().any(|c| c.control == ToolbarControl::ColorPicker));
        assert!(native.iter().all(|c| !c.label.is_empty()
            && !c.description.is_empty()
            && crate::icon_ships(c.icon)));
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

fn customization_text(localization: &Localizer, id: MessageId, values: &[(&str, String)]) -> String {
    let mut args = FluentArgs::new();
    for (key, value) in values { args.set(*key, value.as_str()); }
    localization.format(id, &args)
}
