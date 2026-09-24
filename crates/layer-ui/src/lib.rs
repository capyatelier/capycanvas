//! Shared application behavior, with native widgets supplied by each frontend.
//!
//! One synchronous `UiSession` owns the engine and presentation state. Hosts
//! dispatch typed actions, refresh changed regions, and feed pen records through
//! the separate input path. No toolkit, executor, callbacks, or pixel copies.

mod camera;
mod document_tabs;
pub use document_tabs::{DocumentTabs, DocumentTabHit};
mod document_sessions;
pub use document_sessions::{DocumentAdmission, DocumentBudget, DocumentSessions, DocumentTabLabel, ParkedDocument};
mod document_creation;
pub use document_creation::{DocumentBackground, NewDocumentAction, NewDocumentOptions, NewDocumentPreset, NewDocumentSettings};
mod document_workflow;
pub use document_workflow::{CandidateIdentity, ColorWorkflow, ColorPreparation, SourceWorkflow};

pub mod profile_library;
pub mod proof_workflow;
pub mod proof_panel;
pub mod color_management;
pub mod parameter_pad;

mod import_policy;
pub use import_policy::{ImageImportBatch, ImportIntent, ImportSource, ImportedDocument, read_import, require_sdr_host};

pub mod recovery;
mod workspace_update;
pub use workspace_update::*;
mod eyedropper;
mod export;
pub use export::{ExportDraft, ExportDraftAction, ExportForm, ExportBackground, ExportFormat, ExportProfile, ExportRecipe, ExportResolution, ExportSize};
mod export_presets;
pub use export_presets::{ExportPresets, ExportPresetAction, ExportPresetView};
pub use layer_core::{FigurePaint, FigureShape, RulerKind};
mod navigator;
pub use navigator::NavigatorGeometry;
pub use session::{FilterPreviewCache, FilterPreviewStatus, FilterPreviewUpdate};
mod color;
mod tool_settings;
mod toolbar_components;
pub use toolbar_components::*;
mod toolbar_transport;
pub use toolbar_transport::*;
mod toolbar_preview;
pub use toolbar_preview::*;
mod tools;
pub use color::{
    ColorEditor, ColorInputModel, ColorFormRequest, ColorFormView, ColorPreview, ColorUiRequest, color_form, color_preview, color_validation, color_ui,
    ColorLibrary, ColorLibraryAction, ColorPalette, SavedColor,
    HdrIntensityArc, ColorAction, ColorComponentView, ColorHueStop, ColorPanelLayout, ColorPanelView, ColorReadout, ColorShape, ColorSlot, ColorSpace, ColorState,
    ColorSwatchView, ColorWheelGeometry, ColorWheelPart, hue_color, render_color_field, render_hls_field, render_okhsv_disc, render_hsv_field, render_hue_guide, render_hue_guide_in,
};
pub use tool_settings::{ToolActionGroup, ToolSetting, ToolSettingAction};
use tools::preset;
pub use tools::{
    Tool, ToolFamily, ToolGroup, ToolPanels, ToolSetItem, ToolSetView, WorkspaceToolMemory, brush_catalog,
    brush_categories,
};
mod cursor;
mod customization;
mod header;
pub use header::*;
mod header_drag;
pub use header_drag::*;
mod drawers;
pub use drawers::{
    ColumnDrawerMeasurement, ContentDrawer, DrawerAnchor, DrawerConnection, DrawerDismissal,
    DrawerPlacement, DrawerTabs, DrawerTileMeasurement, TileAnchor,
};
mod interaction;
mod layout;
mod tab_drag;
pub use tab_drag::{TabDragOffset, TabDragPreview};
mod numeric;
mod session;
mod settings;
mod shortcuts;
mod theme;
mod workspace;
mod workspace_manager_ui;
pub use session::{ImageLayerDestination, ImagePlacementContext, LayerAction, LayerCanvasTool, LayerDropPosition, LayersView, RegionSource};
pub use workspace_manager_ui::{ManagedWorkspace, WorkspaceChoice, WorkspaceCommand};
mod stats;
pub use session::{
    AdjustmentChoice, ApplicationLink, ApplicationMenu, CANCEL_DOCUMENT_LABEL, CloseDecision,
    DEFAULT_DOCUMENT_EXTENT, DISCARD_DOCUMENT_LABEL, DOCUMENT_HEIGHT_LABEL, DOCUMENT_WIDTH_LABEL,
    DocumentColorOperation, DocumentExport, DocumentFileState, DocumentLocation, DocumentRequest, EffectAction, FilterCategoryChoice,
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
    Axis, Bounds, CollapsedColumn, CollapsedColumnPlacement, CollapsedGroup, OpenColumn,
    ColumnIcon, ColumnStack, Divider, DockBand, DockItem,
    DockLayout, DockNode, DockTarget, Edge, EdgeAlignment, FloatingGroup, FloatingResizeHandle,
    FloatingToolbarLayout, GroupPlacement, PANEL_CONFIGURATION_WIDTH, PANEL_EXPANSION_MS, Panel,
    PanelExpansion, PanelMeasurement, PanelScrollMeasurement, ResizeEdge, ResolvedLayout,
    WorkspacePreset,
};
pub use layout::{
    DropHint, BRUSH_SETS_MIN_WIDTH, LAYERS_MIN_WIDTH, PANEL_CONTENT_INSET, PanelKind, TAB_BAR_HEIGHT, TILE_SIZE,
    TOOL_PANEL_MIN_WIDTH, TabHit, TileLayout, tile_layout, toolbar_content_height,
    toolbar_tile_layout,
};
pub use numeric::{
    NumericControl, NumericKind, NumericMapping, NumericOperation, NumericRequest, NumericValue,
};
pub use session::{LayerControls, PreparedWorkspace, ProofMode, UiSession, SelectionTool, SelectionConstraint, SelectionOptions, SelectionMode};
pub use settings::{
    ChoicePresentation, ClockVisibility, HostRequest, HostRequestKind, Platform,
    PreferenceAction,
    PreferenceGroup, PreferenceId, PreferenceKind, PreferencePage, PreferenceReset, PreferenceRow,
    PreferenceSearchResult, PreferenceValue, PreferencesState, PreferencesView, Settings,
    SettingsPage, ShortcutEditor, TextConstraint, ZenIcon, MissingProfilePolicy, PhotoOpenPolicy,
};
pub use shortcuts::{
    KeyChord, ShortcutAction, ShortcutCapture, ShortcutDefinition, ShortcutRow, TextEditAction,
    TextEditMenuItem, text_edit_menu,
};
pub use theme::{HexColor, Theme, ThemePalette};
pub use workspace::{
    LayoutHistory, LayoutRevision, WorkspaceCapture, WorkspaceState, WorkspaceWorkingState,
    durable_layout, layout_change_description,
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
    pub icon: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolbarControl {
    Command { command: CommandId },
    Brush { id: u32 },
    Size { pixels: u16 },
    Color,
    Opacity,
    BrushSizeSlider,
    BrushOpacitySlider,
    ToolOptions { #[serde(default)] style: ToolOptionsStyle },
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
    &[CommandId::Drawings],
];
pub const EDIT_MENU: MenuSpec = MenuSpec {
    label: "Edit",
    sections: &[
        &[CommandId::Undo, CommandId::Redo],
        &[CommandId::PasteImage],
        &[CommandId::RasterizeSource, CommandId::ClearLayer, CommandId::FillSelection],
        &[CommandId::ScaleRotate],
        &[CommandId::AssignProfile, CommandId::ConvertColorSpace, CommandId::ChangeBitDepth],
        &[CommandId::Settings],
    ],
};
pub const VIEW_MENU: MenuSpec = MenuSpec {
    label: "View",
    sections: &[
        &[CommandId::Histogram],
        &[CommandId::SoftProofSetup, CommandId::SoftProof, CommandId::GamutWarning, CommandId::SdrRendition, CommandId::PreviewSdr],
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
            CommandId::ImportImage,
            CommandId::NewWindow,
        ],
        &[
            CommandId::SaveDocument,
            CommandId::SaveDocumentAs,
            CommandId::ExportDocument,
        ],
        &[CommandId::DocumentProperties, CommandId::RepairSourceProfile, CommandId::Drawings, CommandId::CloseDocument],
    ],
};
pub const WORKSPACE_MENU_LABEL: &str = "Window";
pub const ZEN_ICON_SIZE: u32 = 31;

#[derive(Clone, Debug, Serialize)]
pub struct PanelChoice {
    pub id: Panel,
    pub label: &'static str,
    pub kind: PanelKind,
}
#[derive(Clone, Debug, Serialize)]
pub struct BrushCategory {
    pub label: &'static str,
    pub icon: &'static str,
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
            "brush-size",
            "paint-flow",
            "hardness",
            "brush-spacing",
            "angle",
            "rotation-variation",
            "paint-load",
            "water",
            "dilution",
            "edge-strength",
            "edge-width",
            "wet-bleed",
            "dry-bleed",
            "strength",
            "close-gap",
            "expand",
            "edge-smooth",
            "feather",
            "width",
            "height",
            "position-x",
            "position-y",
            "new-document",
            "open-document",
            "save-document",
            "export-document",
            "save-as",
            "close-document",
            "clear",
            "transform",
            "close",
            "select-all",
            "deselect",
            "invert-selection",
            "fill-selection",
            "lasso-fill",
            "add-layer",
            "toolbar",
            "new-toolbar",
            "reset-layout",
            "new-window",
            "website",
            "source-code",
            "adjustments",
            "properties",
            "stats",
            "brush",
            "drawing-tools",
            "paper",
            "sculpt",
            "paint",
            "watercolor",
            "oil-paint",
            "marker",
            "pastel",
            "spray",
            "gradient-transparent",
            "gradient-radial",
            "gradient-radial-transparent",
            "rectangle-fill",
            "rectangle-both",
            "ellipse-fill",
            "ellipse-both",
            "reset",
            "chevron-double-left",
            "chevron-double-right",
            "pen",
            "pencil",
            "airbrush",
            "decoration",
            "blend",
            "liquify",
            "eraser",
            "lasso",
            "select",
            "rectangle-select",
            "ellipse-select",
            "polygon-select",
            "color-select",
            "selection-new",
            "selection-add",
            "selection-subtract",
            "selection-intersect",
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
            "color-swap",
            "color-circle",
            "color-square",
            "color-triangle",
            "opacity",
            "grip",
            "pin",
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
        ]
        .into_iter()
        .chain(CursorMode::CHOICES.iter().map(|(mode, _)| mode.icon()))
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
    DrawingBrush,
    Sculpt,
    SdrRendition,
    PreviewSdr,
    SoftProofSetup,
    SoftProof,
    GamutWarning,
    Histogram,
    ImportImage,
    PasteImage,
    DocumentProperties,
    AssignProfile,
    ConvertColorSpace,
    ChangeBitDepth,
    RepairSourceProfile,
    RasterizeSource,
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
    Select,
    RectangleSelect,
    EllipseSelect,
    PolygonSelect,
    ColorSelect,
    SelectionNew,
    SelectionAdd,
    SelectionSubtract,
    SelectionIntersect,
    SelectionAntialias,
    SelectionConstrainAngles,
    SelectionFixedRatio,
    SelectionFixedSize,
    SelectionFromCenter,
    CompleteSelection,
    CancelSelection,
    SelectionVisible,
    SelectionEditing,
    SelectionReference,
    Move,
    ScaleRotate,
    ApplyTransform,
    CancelTransform,
    TransformAspect,
    PlacementOriginalSize,
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
    CustomizeWorkspaceUi,
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
    Drawings,
}
impl CommandId {
    pub fn available_on(self, platform: Platform) -> bool {
        match self {
            Self::Select | Self::RectangleSelect | Self::EllipseSelect | Self::PolygonSelect | Self::ColorSelect | Self::SelectionNew | Self::SelectionAdd | Self::SelectionSubtract | Self::SelectionIntersect | Self::SelectionAntialias | Self::SelectionConstrainAngles | Self::SelectionFixedRatio | Self::SelectionFixedSize | Self::SelectionFromCenter | Self::CompleteSelection | Self::CancelSelection | Self::SelectionVisible | Self::SelectionEditing | Self::SelectionReference => matches!(platform, Platform::Gtk | Platform::Web | Platform::Android),
            Self::DrawingBrush | Self::Sculpt => true,
            Self::Drawings => matches!(platform, Platform::Gtk | Platform::Web | Platform::Android | Platform::Mac | Platform::Ios | Platform::Windows),
            Self::SdrRendition | Self::PreviewSdr => color_management::enabled(platform),
            Self::SoftProofSetup | Self::SoftProof | Self::GamutWarning => matches!(platform, Platform::Gtk | Platform::Web | Platform::Android | Platform::Mac | Platform::Ios | Platform::Windows),
            Self::Histogram => matches!(platform, Platform::Gtk | Platform::Web | Platform::Android | Platform::Mac | Platform::Ios | Platform::Windows),
            Self::ImportImage | Self::PasteImage => matches!(platform, Platform::Gtk | Platform::Web | Platform::Android | Platform::Ios | Platform::Mac | Platform::Windows),
            Self::AssignProfile | Self::ConvertColorSpace | Self::ChangeBitDepth | Self::DocumentProperties => matches!(platform, Platform::Gtk | Platform::Web | Platform::Android | Platform::Mac | Platform::Ios | Platform::Windows),
            Self::RasterizeSource | Self::RepairSourceProfile => matches!(platform, Platform::Gtk | Platform::Web | Platform::Android | Platform::Mac | Platform::Ios | Platform::Windows),
            Self::CustomizeWorkspaceUi => matches!(
                platform,
                Platform::Gtk
                    | Platform::Web
                    | Platform::Android
                    | Platform::Ios
                    | Platform::Mac
                    | Platform::Windows
            ),
            Self::Fullscreen => matches!(
                platform,
                Platform::Gtk | Platform::Web | Platform::Mac | Platform::Windows
            ),
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
            Self::NewWindow => platform.native_windows(),
            _ => true,
        }
    }
    /// Retained on/off commands can be presented as checkable menu items.
    pub fn is_toggle(self) -> bool {
        matches!(
            self,
            Self::SelectionNew | Self::SelectionAdd | Self::SelectionSubtract | Self::SelectionIntersect | Self::SelectionAntialias | Self::SelectionConstrainAngles
                | Self::SelectionFixedRatio | Self::SelectionFixedSize | Self::SelectionFromCenter
                | Self::SelectionVisible | Self::SelectionEditing | Self::SelectionReference
                | Self::ZenMode
                | Self::Fullscreen
                | Self::ToggleTheme
                | Self::FlipHorizontal
                | Self::FlipVertical
                | Self::ShowRulers
                | Self::SnapRulers
                | Self::TransformAspect
                | Self::PreviewSdr
                | Self::SoftProof
                | Self::GamutWarning
        )
    }
    pub fn icon(self) -> Option<&'static str> {
        Some(match self {
            Self::DrawingBrush => "drawing-tools",
            Self::Sculpt => "sculpt",
            Self::SdrRendition | Self::PreviewSdr | Self::SoftProofSetup | Self::SoftProof | Self::GamutWarning => "image",
            Self::Histogram => "stats",
            Self::ImportImage | Self::PasteImage | Self::RasterizeSource => "image",
            Self::AssignProfile | Self::ConvertColorSpace | Self::ChangeBitDepth | Self::DocumentProperties | Self::RepairSourceProfile => "info",
            Self::NewDocument => "new-document",
            Self::OpenDocument => "open-document",
            Self::SaveDocument => "save-document",
            Self::SaveDocumentAs => "save-as",
            Self::ExportDocument => "export-document",
            Self::CloseDocument => "close-document",
            Self::Pen => "pen",
            Self::Pencil => "pencil",
            Self::Brush => "brush",
            Self::Eraser => "eraser",
            Self::Airbrush => "airbrush",
            Self::Decoration => "decoration",
            Self::Blend => "blend",
            Self::Liquify => "liquify",
            Self::Lasso => "lasso",
            Self::Select => "select",
            Self::RectangleSelect => "rectangle-select",
            Self::EllipseSelect => "ellipse-select",
            Self::PolygonSelect => "polygon-select",
            Self::ColorSelect => "color-select",
            Self::SelectionNew => "selection-new",
            Self::SelectionAdd => "selection-add",
            Self::SelectionSubtract => "selection-subtract",
            Self::SelectionIntersect => "selection-intersect",
            Self::SelectionAntialias => "select",
            Self::SelectionConstrainAngles => "ruler",
            Self::SelectionFixedRatio => "rectangle-select",
            Self::SelectionFixedSize => "rectangle-select",
            Self::SelectionFromCenter => "select",
            Self::CompleteSelection => "selection-checked",
            Self::CancelSelection => "deselect",
            Self::SelectionVisible => "eye",
            Self::SelectionEditing => "layers",
            Self::SelectionReference => "reference",

            Self::Move => "move",
            Self::ScaleRotate => "transform",
            Self::ApplyTransform => "check",
            Self::CancelTransform => "close",
            Self::TransformAspect => "link",
            Self::PlacementOriginalSize => "transform",
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
            Self::ClearLayer => "clear",
            Self::FillSelection => "fill-selection",
            Self::SelectAll => "select-all",
            Self::Deselect => "deselect",
            Self::InvertSelection => "invert-selection",
            Self::NewToolbar => "new-toolbar",
            Self::ManageToolbars | Self::CustomizeWorkspaceUi => "toolbar",
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
            Self::ToggleTheme => "appearance",
            Self::AddLayer => "add-layer",
            Self::DeleteLayer => "delete",
            Self::RaiseLayer => "up",
            Self::LowerLayer => "down",
            Self::ResetLayout => "reset-layout",
            Self::NewWindow | Self::Drawings => "new-window",
            Self::KeyboardShortcuts => "keyboard",
            Self::About => "info",
            Self::Website => "website",
            Self::SourceCode => "source-code",
        })
    }
    pub const ALL: [Self; 100] = [
        Self::DrawingBrush,
        Self::Sculpt,
        Self::SdrRendition,
        Self::PreviewSdr,
        Self::SoftProofSetup,
        Self::SoftProof,
        Self::GamutWarning,
        Self::Histogram,
        Self::ImportImage,
        Self::PasteImage,
        Self::DocumentProperties,
        Self::AssignProfile,
        Self::ConvertColorSpace,
        Self::ChangeBitDepth,
        Self::RepairSourceProfile,
        Self::RasterizeSource,
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
        Self::Select,
        Self::RectangleSelect,
        Self::EllipseSelect,
        Self::PolygonSelect,
        Self::ColorSelect,
        Self::SelectionNew,
        Self::SelectionAdd,
        Self::SelectionSubtract,
        Self::SelectionIntersect,
        Self::SelectionAntialias,
        Self::SelectionConstrainAngles,
        Self::SelectionFixedRatio,
        Self::SelectionFixedSize,
        Self::SelectionFromCenter,
        Self::CompleteSelection,
        Self::CancelSelection,
        Self::SelectionVisible,
        Self::SelectionEditing,
        Self::SelectionReference,
        Self::Move,
        Self::ScaleRotate,
        Self::ApplyTransform,
        Self::CancelTransform,
        Self::TransformAspect,
        Self::PlacementOriginalSize,
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
        Self::CustomizeWorkspaceUi,
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
        Self::Drawings,
    ];
    pub const TOOLS: [Self; 25] = [
        Self::DrawingBrush,
        Self::Sculpt,
        Self::Pen,
        Self::Pencil,
        Self::Brush,
        Self::Eraser,
        Self::Airbrush,
        Self::Decoration,
        Self::Blend,
        Self::Liquify,
        Self::Lasso,
        Self::Select,
        Self::RectangleSelect,
        Self::EllipseSelect,
        Self::PolygonSelect,
        Self::ColorSelect,
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
            Self::DrawingBrush => "Brush",
            Self::Sculpt => "Sculpt",
            Self::SdrRendition => "Proof SDR",
            Self::PreviewSdr => "Preview SDR",
            Self::SoftProofSetup => "Proof…",
            Self::SoftProof => "Proof Colors",
            Self::GamutWarning => "Gamut Warning",
            Self::Histogram => "Histogram…",
            Self::ImportImage => "Import Image as Layer…",
            Self::PasteImage => "Paste Image as Layer",
            Self::DocumentProperties => "Document Properties…",
            Self::AssignProfile => "Assign Profile…",
            Self::ConvertColorSpace => "Convert Color Space…",
            Self::ChangeBitDepth => "Change Bit Depth…",
            Self::RepairSourceProfile => "Repair Source Profile…",
            Self::RasterizeSource => "Rasterize Source…",
            Self::NewDocument => "New…",
            Self::OpenDocument => "Open…",
            Self::SaveDocument => "Save",
            Self::SaveDocumentAs => "Save As…",
            Self::ExportDocument => "Export…",
            Self::CloseDocument => "Close",
            Self::Pen => "Pen",
            Self::Pencil => "Pencil",
            Self::Brush => "Paint Brush",
            Self::Eraser => "Eraser",
            Self::Airbrush => "Airbrush",
            Self::Decoration => "Decoration",
            Self::Blend => "Blend",
            Self::Liquify => "Liquify",
            Self::Lasso => "Lasso selection",
            Self::Select => "Select",
            Self::RectangleSelect => "Rectangle select",
            Self::EllipseSelect => "Ellipse select",
            Self::PolygonSelect => "Polygonal lasso",
            Self::ColorSelect => "Select by color",
            Self::SelectionNew => "New selection",
            Self::SelectionAdd => "Add to selection",
            Self::SelectionSubtract => "Subtract from selection",
            Self::SelectionIntersect => "Intersect with selection",
            Self::SelectionAntialias => "Anti-aliasing",
            Self::SelectionConstrainAngles => "Constrain edges to 45°",
            Self::SelectionFixedRatio => "Fixed aspect ratio",
            Self::SelectionFixedSize => "Fixed size",
            Self::SelectionFromCenter => "Draw from center",
            Self::CompleteSelection => "Finish selection",
            Self::CancelSelection => "Cancel selection",
            Self::SelectionVisible => "Sample visible artwork",
            Self::SelectionEditing => "Sample editing layer",
            Self::SelectionReference => "Sample reference layers",

            Self::Move => "Operation",
            Self::ScaleRotate => "Scale / rotate",
            Self::ApplyTransform => "Apply transform",
            Self::CancelTransform => "Cancel transform",
            Self::TransformAspect => "Keep proportions",
            Self::PlacementOriginalSize => "Original Size (100%)",
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
            Self::UndoWorkspace => "Undo Layout Change",
            Self::RedoWorkspace => "Redo Layout Change",
            Self::NewToolbar => "New Toolbar…",
            Self::ManageToolbars => "Manage Toolbars…",
            Self::CustomizeWorkspaceUi => "Customize Title Bar…",
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
            Self::Drawings => "Drawings…",
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
    pub content_icon_color: Option<HexColor>,
    pub label: String,
    pub description: String,
    pub can_delete: bool,
    pub editable: bool,
    pub visible: bool,
    pub opacity: f32,
    pub selected: bool,
    pub mask_selected: bool,
    /// Checked-selection precedence is shared across native hosts.
    pub selection_icon: &'static str,
    /// Content/mask target, independent of the selected row set.
    pub editing: bool,
    pub drawing: bool,
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
    /// Temporary viewing choices, excluded from document and workspace saving.
    pub soft_proof: bool,
    pub preview_sdr: bool,
    pub hdr_display_available: bool,
    pub sdr_appearance_preview: Option<layer_core::color::hdr::SdrRendition>,
    pub gamut_warning: bool,
    pub revision: u64,
    /// Observed native/browser window state; never stored in workspace preferences.
    pub fullscreen: bool,
    pub workspace: WorkspaceState,
    pub brush: BrushState,
    pub colors: ColorState,
    pub tool_settings: Vec<ToolSetting>,
    pub toolbar_context_generation: u64,
    pub tool_actions: Vec<ToolSettingAction>,
    pub tool_set: ToolSetView,
    pub tool_panels: ToolPanels,
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
    /// Retained control presentation. Enabled states stay steady during canvas
    /// input; use `UiSession::command` for live execution availability.
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
    ActivateHeaderItem {
        id: u32,
    },
    MeasureHeader {
        height: f32,
        items: Vec<HeaderItemBounds>,
    },
    WorkspaceManager {
        command: WorkspaceCommand,
    },
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
    /// Native caption controls reserve left/right widths and a height in DIPs.
    MeasureTitlebar {
        insets: [f32; 3],
    },
    /// Runtime clearance for native bottom-edge window controls, in DIPs.
    MeasureWorkspaceBottom {
        inset: f32,
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
    /// Freeze the displayed tab slots and clip after recognizing a panel drag.
    BeginTabDrag {
        tabs: Vec<TabHit>,
        clip: Bounds,
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
        workspace: Box<WorkspaceState>,
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
    SelectBrushSet {
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
    ResetToolSetting {
        id: String,
    },
    /// A retained toolbar editor must never apply to a different tool/target.
    ToolbarEdit {
        context: ToolbarContext,
        action: Box<UiAction>,
    },
    ToggleSliderBookmark {
        control: ToolbarControl,
    },
    SetColorSampleSize {
        width: u32,
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
    NewDocumentSettings {
        settings: NewDocumentSettings,
    },
    NewDocumentPreferences {
        action: NewDocumentAction,
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

#[cfg(test)]
mod icon_tests {
    use super::*;

    #[test]
    fn bundled_filters_have_specific_icons_and_catalog_assets_ship() {
        let bank = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/layer-web/icons");
        let catalog = ui_catalog();
        for icon in &catalog.icons {
            assert!(bank.join(format!("layer-{icon}-symbolic.svg")).is_file(), "{icon}: missing asset");
        }
        let mut meanings = std::collections::BTreeSet::new();
        for filter in layer_core::bundled_effect_catalog().filters() {
            assert_ne!(filter.icon.as_ref(), "adjustments", "{} must not use the picker icon", filter.program.label);
            assert!(meanings.insert(filter.icon.as_ref()), "Different filters need recognizable identities");
        }
    }

    #[test]
    fn workspace_restore_action_preserves_the_host_json_contract() {
        let workspace = WorkspaceState::default();
        let expected = serde_json::json!({
            "type": "restore_workspace",
            "workspace": workspace,
        });
        let action = UiAction::RestoreWorkspace {
            workspace: Box::new(workspace),
        };
        assert_eq!(serde_json::to_value(&action).unwrap(), expected);
        assert_eq!(
            serde_json::from_value::<UiAction>(expected).unwrap(),
            action
        );
    }

    #[test]
    fn every_command_has_a_packaged_icon_and_distinct_editing_semantics() {
        let catalog = ui_catalog();
        let bank =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/layer-web/icons");
        for command in CommandId::ALL {
            let icon = command
                .icon()
                .expect("toolbar commands need meaningful icons");
            assert!(
                catalog.icons.contains(&icon),
                "{command:?}: {icon} missing from catalog"
            );
            assert!(
                bank.join(format!("layer-{icon}-symbolic.svg")).is_file(),
                "{command:?}: missing SVG"
            );
        }
        // These actions previously shared misleading glyphs in both hosts.
        for commands in [
            &[
                CommandId::ClearLayer,
                CommandId::Eraser,
                CommandId::DeleteLayer,
            ][..],
            &[
                CommandId::Lasso,
                CommandId::SelectAll,
                CommandId::Deselect,
                CommandId::InvertSelection,
            ],
            &[CommandId::FitCanvas, CommandId::ScaleRotate],
            &[CommandId::Fill, CommandId::FillSelection],
            &[CommandId::Undo, CommandId::CancelTransform],
        ] {
            let icons: std::collections::BTreeSet<_> = commands.iter().map(|c| c.icon()).collect();
            assert_eq!(
                icons.len(),
                commands.len(),
                "different operations need distinct symbols"
            );
        }
    }
}
