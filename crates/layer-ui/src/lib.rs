//! Shared application behavior, with native widgets supplied by each frontend.
//!
//! One synchronous `UiSession` owns the engine and presentation state. Hosts
//! dispatch typed actions, refresh changed regions, and feed pen records through
//! the separate input path. No toolkit, executor, callbacks, or pixel copies.

mod camera;
mod cursor;
mod customization;
mod interaction;
mod layout;
mod numeric;
mod session;
mod settings;
mod shortcuts;
mod theme;
mod workspace;

pub use camera::{Camera, TouchGesture};
pub use cursor::{CanvasCursor, CursorMode};
pub use customization::{
    ContextMenu, ContextMenuItem, ContextTarget, CustomizationAction, CustomizationState,
    PanelConfig, PanelContent, PanelControl, PanelControlView, PanelView, TabStyle, TileStyle,
    TileView, ToolChoice, ToolPickerView, ToolbarTile, tool_choice,
};
pub use interaction::{
    ChromeEvent, ChromeFacts, InputReply, Modifiers, PointerButton, PointerKind, UiInput,
};
pub use layout::{
    Axis, Bounds, Divider, DockBand, DockItem, DockLayout, DockNode, DockTarget, Edge,
    FloatingGroup, FloatingResizeHandle, FloatingToolbarLayout, GroupPlacement,
    PANEL_CONFIGURATION_WIDTH, PANEL_EXPANSION_MS, Panel, PanelExpansion, PanelMeasurement,
    ResizeEdge, ResolvedLayout,
};
pub use layout::{DropHint, PanelKind, TAB_BAR_HEIGHT, TILE_SIZE, TabHit, TileLayout, tile_layout};
pub use numeric::{
    NumericControl, NumericKind, NumericMapping, NumericOperation, NumericRequest, NumericValue,
};
pub use session::UiSession;
pub use settings::{
    HostRequest, HostRequestKind, Platform, PreferenceAction, PreferenceGroup, PreferenceId,
    PreferenceKind, PreferencePage, PreferenceReset, PreferenceRow, PreferenceSearchResult,
    PreferenceValue, PreferencesState, PreferencesView, Settings, SettingsPage, ShortcutEditor,
    TextConstraint,
};
pub use shortcuts::{KeyChord, ShortcutAction, ShortcutCapture, ShortcutDefinition, ShortcutRow};
pub use theme::{HexColor, Theme, ThemePalette};
pub use workspace::WorkspaceState;

/// Logical units; rendering still uses the entire physical window viewport.
pub const HEADER_HEIGHT: f32 = 48.0;
pub const WORKSPACE_SPACING: f32 = 6.0;
pub const STATUS_HEIGHT: f32 = 28.0;
pub const APP_NAME: &str = "Capy Canvas";
/// Shared UI typography in points, including panels, menus and status text.
pub const UI_TEXT_PT: u8 = 11;

use layer_core::DefaultBrushPreset;
use serde::{Deserialize, Serialize};

pub const BRUSH_SIZES: &[f32] = &[
    2.0, 4.0, 6.0, 8.0, 12.0, 16.0, 24.0, 32.0, 48.0, 64.0, 96.0, 128.0, 192.0, 256.0, 384.0, 512.0,
];

#[derive(Clone, Copy, Debug, Serialize)]
pub struct BrushChoice {
    pub id: u32,
    pub label: &'static str,
    pub category: &'static str,
}

// Preset identity and labels have one source for GTK, DOM, and future hosts.
const BRUSHES: &[(DefaultBrushPreset, &str, &str)] = &[
    (DefaultBrushPreset::GPen, "G-Pen", "Draw"),
    (DefaultBrushPreset::Pencil, "Pencil", "Draw"),
    (DefaultBrushPreset::Paintbrush, "Paintbrush", "Paint"),
    (DefaultBrushPreset::Airbrush, "Airbrush", "Paint"),
    (DefaultBrushPreset::Chalk, "Chalk", "Draw"),
    (DefaultBrushPreset::Marker, "Marker", "Draw"),
    (DefaultBrushPreset::TexturedFlat, "Textured Flat", "Paint"),
    (DefaultBrushPreset::DryScumble, "Dry Scumble", "Paint"),
    (DefaultBrushPreset::PastelBlock, "Pastel Block", "Draw"),
    (
        DefaultBrushPreset::TransparentGlaze,
        "Transparent Glaze",
        "Paint",
    ),
    (DefaultBrushPreset::OpaqueGouache, "Opaque Gouache", "Paint"),
    (
        DefaultBrushPreset::WatercolorWash,
        "Watercolor Wash",
        "Water",
    ),
    (DefaultBrushPreset::WetWatercolor, "Wet Watercolor", "Water"),
    (DefaultBrushPreset::LoadedOil, "Loaded Oil", "Paint"),
    (DefaultBrushPreset::PaletteKnife, "Palette Knife", "Paint"),
    (
        DefaultBrushPreset::NaturalBlender,
        "Natural Blender",
        "Blend",
    ),
    (DefaultBrushPreset::Smudge, "Smudge", "Blend"),
    (DefaultBrushPreset::WetRound, "Wet Round", "Paint"),
    (DefaultBrushPreset::LiquifyPush, "Liquify Push", "Shape"),
    (DefaultBrushPreset::LiquifyTwirl, "Liquify Twirl", "Shape"),
    (DefaultBrushPreset::MultiplyGlaze, "Multiply Glaze", "Paint"),
    (DefaultBrushPreset::Spray, "Spray", "Draw"),
    (DefaultBrushPreset::DualTexture, "Dual Texture", "Draw"),
    (DefaultBrushPreset::Eraser, "Eraser", "Draw"),
];
pub fn brush_catalog() -> impl Iterator<Item = BrushChoice> {
    BRUSHES
        .iter()
        .map(|&(preset, label, category)| BrushChoice {
            id: preset as u32,
            label,
            category,
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolbarControl {
    Command { command: CommandId },
    Brush { id: u32 },
    Size { pixels: u16 },
    Color,
    Opacity,
}
pub const TOOLBAR_CONTROLS: &[ToolbarControl] = &[
    ToolbarControl::Command {
        command: CommandId::Brush,
    },
    ToolbarControl::Command {
        command: CommandId::Eraser,
    },
    ToolbarControl::Command {
        command: CommandId::Undo,
    },
    ToolbarControl::Command {
        command: CommandId::Redo,
    },
    ToolbarControl::Color,
    ToolbarControl::Opacity,
];

#[derive(Clone, Copy, Debug, Serialize)]
pub struct MenuSpec {
    pub label: &'static str,
    /// Related commands; empty means the live `UiSession::workspace_menu` model.
    pub sections: &'static [&'static [CommandId]],
}
pub const PRIMARY_MENU: &[&[CommandId]] = &[
    &[CommandId::NewWindow],
    &[
        CommandId::Settings,
        CommandId::KeyboardShortcuts,
        CommandId::About,
    ],
];
pub const MENUS: &[MenuSpec] = &[
    MenuSpec {
        label: "Edit",
        sections: &[&[CommandId::Undo, CommandId::Redo]],
    },
    MenuSpec {
        label: "View",
        sections: &[
            &[CommandId::FitCanvas],
            &[
                CommandId::ZenMode,
                CommandId::ToggleTheme,
                CommandId::TogglePanels,
            ],
            &[CommandId::ResetLayout],
        ],
    },
    MenuSpec {
        label: WORKSPACE_MENU_LABEL,
        sections: &[],
    },
];
pub const WORKSPACE_MENU_LABEL: &str = "Workspace";
pub const ZEN_ICON_SIZE: u32 = 24;

#[derive(Clone, Debug, Serialize)]
pub struct PanelChoice {
    pub id: Panel,
    pub label: &'static str,
    pub kind: PanelKind,
}
#[derive(Clone, Debug, Serialize)]
pub struct BrushCategory {
    pub label: &'static str,
    pub brushes: Vec<BrushChoice>,
}
pub fn brush_categories() -> impl Iterator<Item = BrushCategory> {
    ["Draw", "Paint", "Water", "Blend", "Shape"]
        .into_iter()
        .map(|label| BrushCategory {
            label,
            brushes: brush_catalog().filter(|b| b.category == label).collect(),
        })
}
#[derive(Clone, Debug, Serialize)]
pub struct UiCatalog {
    pub app_name: &'static str,
    pub text_size_pt: u8,
    pub zen_icon_size: u32,
    pub panel_expansion_ms: u32,
    pub cursors: &'static [(CursorMode, &'static str)],
    pub icons: &'static [&'static str],
    pub panels: Vec<PanelChoice>,
    pub toolbar: &'static [ToolbarControl],
    pub menus: &'static [MenuSpec],
    pub layer_commands: &'static [CommandId],
    pub brush_categories: Vec<BrushCategory>,
    pub brush_sizes: &'static [f32],
    pub brush_size: NumericControl,
    pub opacity: NumericControl,
    pub pressure: NumericControl,
}
pub fn ui_catalog() -> UiCatalog {
    UiCatalog {
        zen_icon_size: ZEN_ICON_SIZE,
        app_name: APP_NAME,
        text_size_pt: UI_TEXT_PT,
        panel_expansion_ms: PANEL_EXPANSION_MS,
        cursors: CursorMode::CHOICES,
        icons: &[
            "brush",
            "eraser",
            "undo",
            "redo",
            "plus",
            "minus",
            "up",
            "down",
            "color",
            "opacity",
            "grip",
            "check",
            "fit",
            "zen",
            "settings",
            "menu",
            "size",
            "layers",
            "appearance",
            "keyboard",
            "info",
            "search",
            "cursor-brush",
            "cursor-brush-cross",
            "cursor-cross",
            "cursor-dot",
            "cursor-none",
        ],
        panels: Panel::ALL
            .into_iter()
            .map(|id| PanelChoice {
                id,
                label: id.label(),
                kind: id.kind(),
            })
            .collect(),
        toolbar: TOOLBAR_CONTROLS,
        menus: MENUS,
        layer_commands: &CommandId::LAYERS,
        brush_categories: brush_categories().collect(),
        brush_sizes: BRUSH_SIZES,
        brush_size: NumericControl::brush_size(),
        opacity: NumericControl::percent(),
        pressure: NumericControl::pressure(),
    }
}
fn preset(id: u32) -> Result<DefaultBrushPreset, String> {
    BRUSHES
        .iter()
        .find(|entry| entry.0 as u32 == id)
        .map(|entry| entry.0)
        .ok_or_else(|| "Unknown brush".into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    Brush,
    Eraser,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandId {
    Brush,
    Eraser,
    Undo,
    Redo,
    UndoWorkspace,
    RedoWorkspace,
    NewToolbar,
    FitCanvas,
    Settings,
    ToggleTheme,
    AddLayer,
    DeleteLayer,
    RaiseLayer,
    LowerLayer,
    ResetLayout,
    TogglePanels,
    ZenMode,
    NewWindow,
    KeyboardShortcuts,
    About,
}
impl CommandId {
    /// Retained on/off commands can be presented as checkable menu items.
    pub fn is_toggle(self) -> bool {
        matches!(self, Self::ZenMode | Self::TogglePanels | Self::ToggleTheme)
    }
    pub fn icon(self) -> Option<&'static str> {
        Some(match self {
            Self::Brush => "brush",
            Self::Eraser => "eraser",
            Self::Undo | Self::UndoWorkspace => "undo",
            Self::Redo | Self::RedoWorkspace => "redo",
            Self::FitCanvas => "fit",
            Self::ZenMode => "zen",
            Self::Settings => "settings",
            Self::AddLayer => "plus",
            Self::DeleteLayer => "minus",
            Self::RaiseLayer => "up",
            Self::LowerLayer => "down",
            _ => return None,
        })
    }
    pub const ALL: [Self; 20] = [
        Self::Brush,
        Self::Eraser,
        Self::Undo,
        Self::Redo,
        Self::UndoWorkspace,
        Self::RedoWorkspace,
        Self::NewToolbar,
        Self::FitCanvas,
        Self::ToggleTheme,
        Self::Settings,
        Self::AddLayer,
        Self::DeleteLayer,
        Self::RaiseLayer,
        Self::LowerLayer,
        Self::ResetLayout,
        Self::TogglePanels,
        Self::ZenMode,
        Self::NewWindow,
        Self::KeyboardShortcuts,
        Self::About,
    ];
    pub const LAYERS: [Self; 4] = [
        Self::AddLayer,
        Self::DeleteLayer,
        Self::RaiseLayer,
        Self::LowerLayer,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Brush => "Brush",
            Self::Eraser => "Eraser",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::UndoWorkspace => "Undo Workspace Change",
            Self::RedoWorkspace => "Redo Workspace Change",
            Self::NewToolbar => "New Toolbar…",
            Self::FitCanvas => "Fit canvas",
            Self::Settings => "Preferences",
            Self::ToggleTheme => "Dark Mode",
            Self::AddLayer => "New layer",
            Self::DeleteLayer => "Delete layer",
            Self::RaiseLayer => "Raise layer",
            Self::LowerLayer => "Lower layer",
            Self::ResetLayout => "Reset layout",
            Self::TogglePanels => "Show panels",
            Self::ZenMode => "Zen mode",
            Self::NewWindow => "New Window",
            Self::KeyboardShortcuts => "Keyboard Shortcuts",
            Self::About => "About Capy Canvas",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CommandState {
    pub id: CommandId,
    pub icon: Option<&'static str>,
    pub label: &'static str,
    pub enabled: bool,
    pub selected: bool,
    pub shortcut: String,
    pub bindings: Vec<KeyChord>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BrushState {
    pub preset: u32,
    pub tool: Tool,
    pub diameter: f32,
    pub opacity: f32,
    /// Display-encoded sRGB; conversion to linear paint happens in Rust.
    pub color: [f32; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LayerState {
    pub id: u64,
    pub label: String,
    pub editable: bool,
    pub visible: bool,
    pub opacity: f32,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DocumentTab {
    pub id: String,
    pub title: String,
    pub active: bool,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct UiState {
    pub revision: u64,
    pub workspace: WorkspaceState,
    pub brush: BrushState,
    pub layers: Vec<LayerState>,
    pub tabs: Vec<DocumentTab>,
    pub commands: Vec<CommandState>,
    pub settings: Settings,
    /// Resolved appearance for widgets, previews and GPU canvas surround.
    pub theme: Theme,
    pub palette: ThemePalette,
    /// Settings are applied individually; dismissal only closes the view.
    pub settings_open: bool,
    pub preferences: PreferencesState,
    pub customization: CustomizationState,
    pub platform: Platform,
    pub requests: Vec<HostRequest>,
    pub host_error: Option<String>,
    pub camera: Camera,
}

/// The same payload works for buttons, menus, shortcuts, and accessibility.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiAction {
    MeasurePanels {
        measurements: Vec<PanelMeasurement>,
    },
    DragWorkspace {
        item: DockItem,
        phase: ContactPhase,
        position: [f32; 2],
        viewport: [f32; 2],
        #[serde(default)]
        tabs: Vec<TabHit>,
    },
    ResizeFloating {
        group: u32,
        edge: ResizeEdge,
        phase: ContactPhase,
        position: [f32; 2],
        viewport: [f32; 2],
    },
    /// Double-click a float's drag area: reset size, then cycle toolbar layouts
    /// or toggle the header of a lone built-in panel already at default size.
    CycleFloatingSize {
        group: u32,
        viewport: [f32; 2],
    },
    Customize {
        action: CustomizationAction,
    },
    ActivateTile {
        panel: Panel,
        tile: u32,
    },
    RestoreWorkspace {
        workspace: WorkspaceState,
    },
    Invoke {
        command: CommandId,
    },
    SelectBrush {
        id: u32,
    },
    SetBrushSize {
        value: f32,
    },
    SetBrushOpacity {
        value: f32,
    },
    SetColor {
        rgba: [f32; 4],
    },
    SelectLayer {
        id: u64,
    },
    SetLayerVisibility {
        id: u64,
        visible: bool,
    },
    SetLayerOpacity {
        /// Omitted by the selected-layer control; resolved against live Rust
        /// document state, never a potentially stale frontend snapshot.
        #[serde(default)]
        id: Option<u64>,
        opacity: f32,
    },
    MoveLayer {
        id: u64,
        index: u32,
    },
    MovePanel {
        panel: Panel,
        target: DockTarget,
        viewport: [f32; 2],
    },
    MoveGroup {
        group: u32,
        target: DockTarget,
        viewport: [f32; 2],
    },
    MoveTile {
        panel: Panel,
        tile: u32,
        target: DockTarget,
        viewport: [f32; 2],
    },
    /// Activate a tab. The selected tab toggles its drawer; another tab switches
    /// content while preserving an open drawer in the same group.
    SelectPanelTab {
        group: u32,
        panel: Panel,
    },
    /// Divider center in logical workspace units, for mouse/touch or keyboard.
    ResizeDock {
        id: u32,
        position: [f32; 2],
        viewport: [f32; 2],
    },
    DragDivider {
        id: u32,
        phase: ContactPhase,
        position: [f32; 2],
        viewport: [f32; 2],
    },
    NudgeDivider {
        id: u32,
        forward: bool,
        viewport: [f32; 2],
    },
    PrioritizeBand {
        id: u32,
    },
    SetTheme {
        theme: Option<Theme>,
    },
    SystemThemeChanged {
        theme: Theme,
    },
    EditSettings {
        settings: Settings,
    },
    OpenSettings {
        page: SettingsPage,
    },
    Preferences {
        action: PreferenceAction,
    },
    RestoreSettings {
        #[serde(deserialize_with = "Settings::deserialize_saved")]
        settings: Settings,
    },
    CompleteRequest {
        id: u32,
        error: Option<String>,
    },
    CloseSettings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContactPhase {
    Down,
    Move,
    Up,
    Cancel,
}

/// Hosts request only the affected state fields. Pen movement normally returns
/// no UI changes; canvas_wake schedules a display callback, never a GPU wait.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct UiChange {
    pub revision: u64,
    pub regions: u32,
    pub canvas_wake: bool,
}
pub mod regions {
    pub const LAYOUT: u32 = 1;
    pub const BRUSH: u32 = 2;
    pub const DOCUMENT: u32 = 4;
    pub const COMMANDS: u32 = 8;
    pub const SETTINGS: u32 = 16;
    pub const CAMERA: u32 = 32;
    pub const HOST: u32 = 64;
    pub const CUSTOMIZATION: u32 = 128;
    pub const ALL: u32 = 255;
}

pub fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}
