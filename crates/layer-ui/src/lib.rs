//! Shared application behavior, with native widgets supplied by each frontend.
//!
//! One synchronous `UiSession` owns the engine and presentation state. Hosts
//! dispatch typed actions, refresh changed regions, and feed pen records through
//! the separate input path. No toolkit, executor, callbacks, or pixel copies.

mod camera;
mod eyedropper;
pub use layer_core::{FigurePaint, FigureShape, RulerKind};
mod navigator;
pub use navigator::NavigatorGeometry;
mod color;
mod tool_settings;
mod tools;
pub use color::{
    ColorAction, ColorComponentView, ColorPanelView, ColorSlot, ColorSpace, ColorState,
    ColorSwatchView, ColorWheelGeometry, ColorWheelPart, hue_color,
};
pub use tool_settings::{ToolSetting, ToolSettingAction};
use tools::preset;
pub use tools::{
    Tool, ToolFamily, ToolGroup, ToolSetItem, ToolSetView, WorkspaceToolMemory, brush_catalog,
    brush_categories,
};
mod cursor;
mod customization;
mod drawers;
mod zen;
pub use drawers::{
    ColumnDrawerMeasurement, ContentDrawer, DrawerAnchor, DrawerConnection, DrawerDismissal,
    DrawerPlacement, DrawerTabs, DrawerTileMeasurement, TileAnchor,
};
pub use zen::{ZenSection, ZenToolbars};
mod interaction;
mod layout;
mod numeric;
mod session;
mod settings;
mod shortcuts;
mod theme;
mod workspace;
pub use session::{LayerAction, LayerCanvasTool, LayersView, RegionSource};
mod stats;
pub use session::{
    AdjustmentChoice, ApplicationLink, ApplicationMenu, CANCEL_DOCUMENT_LABEL, CloseDecision,
    DEFAULT_DOCUMENT_EXTENT, DISCARD_DOCUMENT_LABEL, DOCUMENT_HEIGHT_LABEL, DOCUMENT_WIDTH_LABEL,
    DocumentFileState, DocumentLocation, DocumentRequest, EffectAction, FilterCategoryChoice,
    FilterLoadState, FilterPickerAction, FilterPickerState, LayerPropertiesView,
    MAX_NEW_DOCUMENT_DIMENSION, PropertyControl, PropertyKind, UNSAVED_DESCRIPTION, new_drawing,
};
pub use stats::{StatRow, StatsView};

pub use camera::{Camera, TouchGesture};
pub use cursor::{CanvasCursor, CursorMode};
pub use customization::{
    ContextMenu, ContextMenuItem, ContextTarget, CustomizationAction, CustomizationState,
    PanelConfig, PanelContent, PanelControl, PanelControlView, PanelView, TabPresentation,
    TabStyle, TileStyle, TileView, ToolChoice, ToolPickerView, ToolbarManagerView, ToolbarTile,
    tool_choice,
};
pub use interaction::{
    ChromeEvent, ChromeFacts, InputReply, Modifiers, PointerButton, PointerKind, UiInput,
};
pub use layout::{
    Axis, Bounds, CollapsedColumn, CollapsedColumnPlacement, CollapsedGroup, ColumnIcon, Divider,
    DockBand, DockItem, DockLayout, DockNode, DockTarget, Edge, FloatingGroup,
    FloatingResizeHandle, FloatingToolbarLayout, GroupPlacement, PANEL_CONFIGURATION_WIDTH,
    PANEL_EXPANSION_MS, Panel, PanelExpansion, PanelMeasurement, ResizeEdge, ResolvedLayout,
};
pub use layout::{
    DropHint, LAYERS_MIN_WIDTH, PANEL_CONTENT_INSET, PanelKind, TAB_BAR_HEIGHT, TILE_SIZE,
    TOOL_PANEL_MIN_WIDTH, TabHit, TileLayout, tile_layout, toolbar_content_height,
    toolbar_tile_layout,
};
pub use numeric::{
    NumericControl, NumericKind, NumericMapping, NumericOperation, NumericRequest, NumericValue,
};
pub use session::{LayerControls, PreparedWorkspace, UiSession};
pub use settings::{
    ChoicePresentation, ClockVisibility, HostRequest, HostRequestKind, Platform, PreferenceAction,
    PreferenceGroup, PreferenceId, PreferenceKind, PreferencePage, PreferenceReset, PreferenceRow,
    PreferenceSearchResult, PreferenceValue, PreferencesState, PreferencesView, Settings,
    SettingsPage, ShortcutEditor, TextConstraint, ZenIcon,
};
pub use shortcuts::{
    KeyChord, ShortcutAction, ShortcutCapture, ShortcutDefinition, ShortcutRow, TextEditAction,
    TextEditMenuItem, text_edit_menu,
};
pub use theme::{HexColor, Theme, ThemePalette};
pub use workspace::{
    LayoutHistory, LayoutRevision, WorkspaceCapture, WorkspaceState, WorkspaceWorkingState,
    durable_layout,
};

/// Logical units; rendering still uses the entire physical window viewport.
pub const HEADER_HEIGHT: f32 = 48.0;
pub const WORKSPACE_SPACING: f32 = 6.0;
pub const STATUS_HEIGHT: f32 = 28.0;
pub const APP_NAME: &str = "Capy Canvas";
/// Shared UI typography in points, including panels, menus and status text.
pub const UI_TEXT_PT: u8 = 11;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolbarControl {
    Command { command: CommandId },
    Brush { id: u32 },
    Size { pixels: u16 },
    Color,
    Opacity,
    Panel { panel: Panel },
    Divider,
}
pub const TOOLBAR_CONTROLS: &[ToolbarControl] = &[
    ToolbarControl::Command {
        command: CommandId::Brush,
    },
    ToolbarControl::Command {
        command: CommandId::Eraser,
    },
    ToolbarControl::Command {
        command: CommandId::Lasso,
    },
    ToolbarControl::Command {
        command: CommandId::Move,
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
pub const EDIT_MENU: MenuSpec = MenuSpec {
    label: "Edit",
    sections: &[
        &[CommandId::Undo, CommandId::Redo],
        &[CommandId::ClearLayer, CommandId::FillSelection],
        &[CommandId::ScaleRotate],
        &[CommandId::Settings],
    ],
};
pub const VIEW_MENU: MenuSpec = MenuSpec {
    label: "View",
    sections: &[
        &[CommandId::ZoomIn, CommandId::ZoomOut, CommandId::FitCanvas],
        &[CommandId::RotateLeft, CommandId::RotateRight],
        &[CommandId::FlipHorizontal, CommandId::FlipVertical],
        &[CommandId::ShowRulers, CommandId::SnapRulers],
        &[CommandId::ZenMode, CommandId::Fullscreen],
        &[CommandId::ResetLayout],
    ],
};
/// Catalog used by hosts awaiting the expanded application-menu presentation.
pub const MENUS: &[MenuSpec] = &[
    EDIT_MENU,
    VIEW_MENU,
    MenuSpec {
        label: WORKSPACE_MENU_LABEL,
        sections: &[],
    },
];
/// GTK-first until the document transport is available on the other hosts.
pub const FILE_MENU: MenuSpec = MenuSpec {
    label: "File",
    sections: &[
        &[
            CommandId::NewDocument,
            CommandId::OpenDocument,
            CommandId::NewWindow,
        ],
        &[
            CommandId::SaveDocument,
            CommandId::SaveDocumentAs,
            CommandId::ExportDocument,
        ],
        &[CommandId::CloseDocument],
    ],
};
pub const WORKSPACE_MENU_LABEL: &str = "Window";
pub const ZEN_ICON_SIZE: u32 = 28;

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
#[derive(Clone, Debug, Serialize)]
pub struct UiCatalog {
    pub app_name: &'static str,
    pub text_size_pt: u8,
    pub zen_icon_size: u32,
    pub panel_expansion_ms: u32,
    pub cursors: &'static [(CursorMode, &'static str)],
    pub icons: Vec<&'static str>,
    pub panels: Vec<PanelChoice>,
    pub toolbar: &'static [ToolbarControl],
    pub menus: &'static [MenuSpec],
    /// Primary drawing tools for hosts that also expose a compact tool chooser.
    pub tool_commands: &'static [CommandId],
    pub file_menu: MenuSpec,
    pub new_document: session::NewDocumentSpec,
    pub layer_commands: &'static [CommandId],
    pub brush_categories: Vec<BrushCategory>,
    pub brush_sizes: &'static [f32],
    pub brush_size: NumericControl,
    pub opacity: NumericControl,
    pub layer_opacity: NumericControl,
    pub layer_blends: Vec<&'static str>,
    pub pressure: NumericControl,
}
pub fn ui_catalog() -> UiCatalog {
    UiCatalog {
        zen_icon_size: ZEN_ICON_SIZE,
        app_name: APP_NAME,
        text_size_pt: UI_TEXT_PT,
        panel_expansion_ms: PANEL_EXPANSION_MS,
        cursors: CursorMode::CHOICES,
        icons: [
            "new-document",
            "open-document",
            "save-document",
            "export-document",
            "adjustments",
            "properties",
            "stats",
            "brush",
            "pen",
            "pencil",
            "airbrush",
            "decoration",
            "blend",
            "liquify",
            "eraser",
            "lasso",
            "move",
            "alpha-lock",
            "clip",
            "reference",
            "selection-checked",
            "selection-empty",
            "eye",
            "eye-hidden",
            "lock",
            "folder",
            "folder-open",
            "link",
            "mask",
            "image",
            "more",
            "delete",
            "animation",
            "undo",
            "redo",
            "plus",
            "minus",
            "up",
            "down",
            "color",
            "swap",
            "opacity",
            "grip",
            "check",
            "fit",
            "navigator",
            "hand",
            "eyedropper",
            "gradient",
            "figure",
            "ruler",
            "ruler-parallel",
            "ruler-radial",
            "ruler-snap",
            "line",
            "rectangle",
            "ellipse",
            "auto-select",
            "fill",
            "rotate-left",
            "rotate-right",
            "flip-horizontal",
            "fullscreen-enter",
            "fullscreen-exit",
            "flip-vertical",
            const { ZenIcon::LookingUp.icon() },
            const { ZenIcon::FacingForward.icon() },
            const { ZenIcon::Bathing.icon() },
            const { ZenIcon::Sleeping.icon() },
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
        ]
        .into_iter()
        .chain(
            layer_core::bundled_effect_catalog()
                .filters()
                .iter()
                .map(|e| e.icon.as_ref()),
        )
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect(),
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
        file_menu: FILE_MENU,
        new_document: session::new_document_spec(),
        layer_commands: &CommandId::LAYERS,
        tool_commands: &CommandId::TOOLS,
        brush_categories: brush_categories().collect(),
        brush_sizes: BRUSH_SIZES,
        brush_size: NumericControl::brush_size(),
        opacity: NumericControl::percent(),
        layer_opacity: NumericControl::layer_opacity(),
        layer_blends: layer_core::LayerBlend::ALL
            .iter()
            .map(|b| b.label())
            .collect(),
        pressure: NumericControl::pressure(),
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandId {
    NewDocument,
    OpenDocument,
    SaveDocument,
    SaveDocumentAs,
    ExportDocument,
    CloseDocument,
    Pen,
    Pencil,
    Brush,
    Eraser,
    Airbrush,
    Decoration,
    Blend,
    Liquify,
    Lasso,
    Move,
    ScaleRotate,
    ApplyTransform,
    CancelTransform,
    TransformAspect,
    Hand,
    Eyedropper,
    Gradient,
    Figure,
    Ruler,
    ShowRulers,
    SnapRulers,
    DeleteRuler,
    AutoSelect,
    Fill,
    Undo,
    Redo,
    ClearLayer,
    FillSelection,
    SelectAll,
    Deselect,
    InvertSelection,
    UndoWorkspace,
    RedoWorkspace,
    NewToolbar,
    ManageToolbars,
    FitCanvas,
    ZoomIn,
    ZoomOut,
    RotateLeft,
    RotateRight,
    FlipHorizontal,
    FlipVertical,
    Settings,
    ToggleTheme,
    AddLayer,
    DeleteLayer,
    RaiseLayer,
    LowerLayer,
    ResetLayout,
    // Old saved toolbar/custom-action entries now use the sole visibility toggle.
    #[serde(alias = "toggle_panels")]
    ZenMode,
    Fullscreen,
    NewWindow,
    KeyboardShortcuts,
    About,
    Website,
    SourceCode,
}
impl CommandId {
    pub fn available_on(self, platform: Platform) -> bool {
        match self {
            Self::Fullscreen => matches!(platform, Platform::Gtk | Platform::Web | Platform::Mac),
            Self::NewDocument
            | Self::OpenDocument
            | Self::SaveDocument
            | Self::SaveDocumentAs
            | Self::CloseDocument => {
                matches!(
                    platform,
                    Platform::Gtk
                        | Platform::Mac
                        | Platform::Ios
                        | Platform::Android
                        | Platform::Windows
                        | Platform::Web
                )
            }
            Self::ExportDocument => {
                matches!(
                    platform,
                    Platform::Gtk
                        | Platform::Mac
                        | Platform::Ios
                        | Platform::Android
                        | Platform::Windows
                        | Platform::Web
                )
            }
            Self::Website | Self::SourceCode => {
                matches!(
                    platform,
                    Platform::Gtk
                        | Platform::Ios
                        | Platform::Mac
                        | Platform::Android
                        | Platform::Web
                        | Platform::Windows
                )
            }
            // Windows currently replaces documents in its single native window.
            Self::NewWindow => platform.native_windows() && platform != Platform::Windows,
            _ => true,
        }
    }
    /// Retained on/off commands can be presented as checkable menu items.
    pub fn is_toggle(self) -> bool {
        matches!(
            self,
            Self::ZenMode
                | Self::Fullscreen
                | Self::ToggleTheme
                | Self::FlipHorizontal
                | Self::FlipVertical
                | Self::ShowRulers
                | Self::SnapRulers
                | Self::TransformAspect
        )
    }
    pub fn icon(self) -> Option<&'static str> {
        Some(match self {
            Self::NewDocument => "new-document",
            Self::OpenDocument => "open-document",
            Self::SaveDocument | Self::SaveDocumentAs => "save-document",
            Self::ExportDocument => "export-document",
            Self::Pen => "pen",
            Self::Pencil => "pencil",
            Self::Brush => "brush",
            Self::Eraser => "eraser",
            Self::Airbrush => "airbrush",
            Self::Decoration => "decoration",
            Self::Blend => "blend",
            Self::Liquify => "liquify",
            Self::Lasso => "lasso",
            Self::Move => "move",
            Self::ScaleRotate => "fit",
            Self::ApplyTransform => "check",
            Self::CancelTransform => "undo",
            Self::Hand => "hand",
            Self::Eyedropper => "eyedropper",
            Self::Gradient => "gradient",
            Self::Figure => "figure",
            Self::Ruler | Self::ShowRulers => "ruler",
            Self::SnapRulers => "ruler-snap",
            Self::DeleteRuler => "delete",
            Self::AutoSelect => "auto-select",
            Self::Fill => "fill",
            Self::Undo | Self::UndoWorkspace => "undo",
            Self::Redo | Self::RedoWorkspace => "redo",
            Self::ClearLayer => "eraser",
            Self::FillSelection => "fill",
            Self::SelectAll | Self::Deselect | Self::InvertSelection => "lasso",
            Self::FitCanvas => "fit",
            Self::ZoomIn => "plus",
            Self::ZoomOut => "minus",
            Self::RotateLeft => "rotate-left",
            Self::RotateRight => "rotate-right",
            Self::FlipHorizontal => "flip-horizontal",
            Self::FlipVertical => "flip-vertical",
            Self::ZenMode => ZenIcon::LookingUp.icon(),
            Self::Fullscreen => "fullscreen-enter",
            Self::Settings => "settings",
            Self::AddLayer => "plus",
            Self::DeleteLayer => "minus",
            Self::RaiseLayer => "up",
            Self::LowerLayer => "down",
            _ => return None,
        })
    }
    pub const ALL: [Self; 62] = [
        Self::NewDocument,
        Self::OpenDocument,
        Self::SaveDocument,
        Self::SaveDocumentAs,
        Self::ExportDocument,
        Self::CloseDocument,
        Self::Pen,
        Self::Pencil,
        Self::Brush,
        Self::Eraser,
        Self::Airbrush,
        Self::Decoration,
        Self::Blend,
        Self::Liquify,
        Self::Lasso,
        Self::Move,
        Self::ScaleRotate,
        Self::ApplyTransform,
        Self::CancelTransform,
        Self::TransformAspect,
        Self::Hand,
        Self::Eyedropper,
        Self::Gradient,
        Self::Figure,
        Self::Ruler,
        Self::ShowRulers,
        Self::SnapRulers,
        Self::DeleteRuler,
        Self::AutoSelect,
        Self::Fill,
        Self::Undo,
        Self::Redo,
        Self::ClearLayer,
        Self::FillSelection,
        Self::SelectAll,
        Self::Deselect,
        Self::InvertSelection,
        Self::UndoWorkspace,
        Self::RedoWorkspace,
        Self::NewToolbar,
        Self::ManageToolbars,
        Self::FitCanvas,
        Self::ZoomIn,
        Self::ZoomOut,
        Self::RotateLeft,
        Self::RotateRight,
        Self::FlipHorizontal,
        Self::FlipVertical,
        Self::ToggleTheme,
        Self::Settings,
        Self::AddLayer,
        Self::DeleteLayer,
        Self::RaiseLayer,
        Self::LowerLayer,
        Self::ResetLayout,
        Self::ZenMode,
        Self::Fullscreen,
        Self::NewWindow,
        Self::KeyboardShortcuts,
        Self::About,
        Self::Website,
        Self::SourceCode,
    ];
    pub const TOOLS: [Self; 18] = [
        Self::Pen,
        Self::Pencil,
        Self::Brush,
        Self::Eraser,
        Self::Airbrush,
        Self::Decoration,
        Self::Blend,
        Self::Liquify,
        Self::Lasso,
        Self::Move,
        Self::ScaleRotate,
        Self::Hand,
        Self::Eyedropper,
        Self::Gradient,
        Self::Figure,
        Self::Ruler,
        Self::AutoSelect,
        Self::Fill,
    ];
    pub const LAYERS: [Self; 4] = [
        Self::AddLayer,
        Self::DeleteLayer,
        Self::RaiseLayer,
        Self::LowerLayer,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::NewDocument => "New…",
            Self::OpenDocument => "Open…",
            Self::SaveDocument => "Save",
            Self::SaveDocumentAs => "Save As…",
            Self::ExportDocument => "Export PNG…",
            Self::CloseDocument => "Close",
            Self::Pen => "Pen",
            Self::Pencil => "Pencil",
            Self::Brush => "Brush",
            Self::Eraser => "Eraser",
            Self::Airbrush => "Airbrush",
            Self::Decoration => "Decoration",
            Self::Blend => "Blend",
            Self::Liquify => "Liquify",
            Self::Lasso => "Lasso selection",
            Self::Move => "Operation",
            Self::ScaleRotate => "Scale / rotate",
            Self::ApplyTransform => "Apply transform",
            Self::CancelTransform => "Cancel transform",
            Self::TransformAspect => "Keep proportions",
            Self::Hand => "Hand",
            Self::Eyedropper => "Eyedropper",
            Self::Gradient => "Gradient",
            Self::Figure => "Figure",
            Self::Ruler => "Ruler",
            Self::ShowRulers => "Show rulers",
            Self::SnapRulers => "Snap to rulers",
            Self::DeleteRuler => "Delete ruler",
            Self::AutoSelect => "Auto select",
            Self::Fill => "Fill",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::ClearLayer => "Clear layer",
            Self::FillSelection => "Fill selection",
            Self::SelectAll => "Select all pixels",
            Self::Deselect => "Deselect pixels",
            Self::InvertSelection => "Invert selection",
            Self::UndoWorkspace => "Undo Workspace Change",
            Self::RedoWorkspace => "Redo Workspace Change",
            Self::NewToolbar => "New Toolbar…",
            Self::ManageToolbars => "Manage Toolbars…",
            Self::FitCanvas => "Fit canvas",
            Self::ZoomIn => "Zoom in",
            Self::ZoomOut => "Zoom out",
            Self::RotateLeft => "Rotate view 90° left",
            Self::RotateRight => "Rotate view 90° right",
            Self::FlipHorizontal => "Flip view horizontally",
            Self::FlipVertical => "Flip view vertically",
            Self::Settings => "Preferences",
            Self::ToggleTheme => "Dark Mode",
            Self::AddLayer => "New layer",
            Self::DeleteLayer => "Delete layer",
            Self::RaiseLayer => "Raise layer",
            Self::LowerLayer => "Lower layer",
            Self::ResetLayout => "Reset layout",
            Self::ZenMode => "Zen mode",
            Self::Fullscreen => "Full screen",
            Self::NewWindow => "New Window",
            Self::KeyboardShortcuts => "Keyboard Shortcuts",
            Self::About => "About Capy Canvas",
            Self::Website => ApplicationLink::Website.label(),
            Self::SourceCode => ApplicationLink::SourceCode.label(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CommandState {
    pub id: CommandId,
    /// Use a native checkable menu item only for retained on/off commands.
    pub checkable: bool,
    pub icon: Option<&'static str>,
    pub label: &'static str,
    pub enabled: bool,
    pub selected: bool,
    pub shortcut: String,
    pub tooltip: String,
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
    pub content_icon: Option<String>,
    pub label: String,
    pub editable: bool,
    pub visible: bool,
    pub opacity: f32,
    pub selected: bool,
    pub mask_selected: bool,
    /// Checked-selection precedence is shared across native hosts.
    pub selection_icon: &'static str,
    /// Content/mask target, independent of the selected row set.
    pub editing: bool,
    pub has_mask: bool,
    pub mask_enabled: bool,
    pub mask_linked: bool,
    pub show_mask_area: bool,
    pub alpha_locked: bool,
    pub locked: bool,
    pub clipped: bool,
    pub reference: bool,
    pub group: bool,
    /// Paper is the bottom anchor; dropping there always inserts above it.
    pub can_drop_below: bool,
    pub depth: u32,
    pub collapsed: bool,
    pub blend: u32,
    pub blend_label: String,
    pub paint_revision: u64,
    pub mask_revision: u64,
    pub mask_id: Option<u64>,
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
    /// Observed native/browser window state; never stored in workspace preferences.
    pub fullscreen: bool,
    pub workspace: WorkspaceState,
    pub brush: BrushState,
    pub colors: ColorState,
    pub tool_settings: Vec<ToolSetting>,
    pub tool_actions: Vec<ToolSettingAction>,
    pub tool_set: ToolSetView,
    pub layers: Vec<LayerState>,
    pub layer_tools: LayersView,
    pub adjustments: Vec<AdjustmentChoice>,
    pub filter_picker: FilterPickerState,
    pub filter_categories: Vec<FilterCategoryChoice>,
    pub filter_catalog_revision: u64,
    pub filter_load: FilterLoadState,
    pub layer_properties: LayerPropertiesView,
    pub tabs: Vec<DocumentTab>,
    pub document_file: DocumentFileState,
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
    WindowFullscreen {
        fullscreen: bool,
    },
    Navigator {
        phase: ContactPhase,
        position: [f32; 2],
        viewport: [f32; 2],
    },
    FilterPicker {
        action: FilterPickerAction,
    },
    Effect {
        action: EffectAction,
    },
    Layer {
        action: LayerAction,
    },
    MeasurePanels {
        measurements: Vec<PanelMeasurement>,
    },
    MeasureDrawerTiles {
        measurements: Vec<DrawerTileMeasurement>,
    },
    MeasureColumnDrawers {
        measurements: Vec<ColumnDrawerMeasurement>,
    },
    MeasureColumnScroll {
        column: u32,
        offset: f32,
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
    /// Double-click a drag handle: toggle a docked panel's tab / refit its
    /// toolbar, or restore a float before cycling its layout / panel header.
    DoubleClickPanelHandle {
        group: u32,
        viewport: [f32; 2],
    },
    /// Double-click the canvas-facing divider to restore component defaults.
    ResetColumnWidth {
        id: u32,
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
    SelectToolGroup {
        group: ToolGroup,
    },
    CycleTool {
        family: ToolFamily,
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
    Color {
        action: ColorAction,
    },
    SetToolSetting {
        id: String,
        value: f32,
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
    MoveColumn {
        column: u32,
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
