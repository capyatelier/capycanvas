//! Shared application behavior, with native widgets supplied by each frontend.
//!
//! One synchronous `UiSession` owns the engine and presentation state. Hosts
//! dispatch typed actions, refresh changed regions, and feed pen records through
//! the separate input path. No toolkit, executor, callbacks, or pixel copies.

mod numeric_labels;
pub use numeric_labels::NumericLabels;
mod document_delivery_copy;
pub use document_delivery_copy::{DocumentDeliveryCopy, DocumentDeliveryMessage};
macro_rules! variants {
    ($(#[$attr:meta])* $vis:vis enum $name:ident { $($(#[$variant_attr:meta])* $variant:ident),+ $(,)? }) => {
        $(#[$attr])* $vis enum $name { $($(#[$variant_attr])* $variant),+ }
        impl $name {
            pub const ALL: [Self; [$(stringify!($variant)),+].len()] = [$(Self::$variant),+];
        }
    };
}
pub(crate) use variants;

pub mod localization;
pub use fluent_bundle::FluentArgs;
pub use localization::{LanguagePreference, LanguageRequest, LanguageTransition, Localizer, LocalizerPreparation, MessageId, UiLanguage, resolve_language, resolve_launch_language, launch_localization, BootstrapView, bootstrap_view, file_open_failure};
#[cfg(test)]
mod localization_catalog_tests;
#[cfg(test)]
mod localization_inventory;

mod search;
mod camera;
mod document_tabs;
pub use document_tabs::{DocumentTabDrag, DocumentTabHit, DocumentTabSlide, DocumentTabs};
mod document_sessions;
pub use document_sessions::{DocumentSessionError, DocumentTransportRefusal, document_storage_retained, document_recovery_unavailable, DocumentAdmission, DocumentBudget, DocumentSessions, DocumentTabLabel, ParkedDocument};
mod document_creation;
pub use document_creation::{BlendingChoice, DocumentBackground, NewDocumentAction, NewDocumentBlending, NewDocumentOptions, NewDocumentPreset, NewDocumentPresetId, NewDocumentPresetView, NewDocumentSettings, NewDocumentError, NewDocumentForm, NewDocumentText, NewDocumentAppearance};
mod document_workflow;
pub use document_workflow::{CandidateIdentity, ColorWorkflow, ColorPreparation, SourceWorkflow};

pub mod profile_library;
pub mod proof_workflow;
pub mod proof_panel;
pub mod color_management;
pub mod parameter_pad;

mod import_policy;
pub use import_policy::{ImageImportBatch, ImportIntent, ImportSource, ImportedDocument, photo_document_names, read_import};

pub mod recovery;
mod workspace_update;
pub use workspace_update::*;
mod eyedropper;
pub use eyedropper::{COLOR_SAMPLE_WIDTHS, ColorPickerAction, ColorPickerState, ColorPickerStyle, PickerPreview};
mod export;
pub use export::{ExportChoice, ExportChoices, ExportNumericControls, ExportDraft, ExportDraftAction, ExportForm, ExportBackground, ExportFormat, ExportMetadataView, ExportProfile, ExportProfileCaption, ExportRecipe, ExportResolution, ExportSize, MetadataChoice};
pub use layer_color::photo::{ExportMetadata, MetadataKeep};
mod export_presets;
pub use export_presets::{ExportPresets, ExportPresetAction, ExportPresetView};
pub use layer_core::{FigurePaint, FigureShape, RulerKind};
mod navigator;
pub use navigator::NavigatorGeometry;
pub use session::tonal_selection::{TonalAction, TonalOptions};
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
    color_intensity_input, color_intensity_input_typed, ColorEditor, ColorEditorError, ColorInputModel, ColorFormCopy, ColorFormCopyView, ColorValidationCopy, ColorFormRequest, ColorFormView, ColorPreview, ColorUiRequest, color_form_localized, color_preview, color_validation_localized, color_ui_localized,
    ColorLibrary, ColorLibraryAction, ColorPalette, ColorReorderPreview, SavedColor,
    PaletteChoiceView, PaletteCommand, PaletteExport, PaletteFileRequest, PaletteFormat, palette_file, PaletteMenuItem, PaletteMenuTarget, PalettePanelView,
    PaletteTileView, selected_swatch,
    HdrIntensityArc, ColorAction, ColorComponentView, ColorHueStop, ColorPanelLayout, ColorPanelView, ColorReadout, ColorShape, ColorSlot, ColorSpace, ColorState,
    ColorSwatchView, PaintPairView, PaintSwatchView, ColorWheelGeometry, ColorWheelPart, render_color_field, render_hue_guide_in,
};
pub use tool_settings::{ToolActionGroup, ToolSetting, ToolSettingAction};
pub use tools::{
    Tool, ToolFamily, ToolGroup, ToolPanels, ToolSetItem, ToolSetView, WorkspaceToolMemory, brush_catalog,
    brush_categories, brush_categories_localized, brush_catalog_localized, brush_ids, preset,
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
pub use session::{ToolSlotId, ToolVariant, ToolSlotMemory, ToolSlotSelection};
pub use session::{CANVAS_BAR_REAPPEAR_MS, CanvasBarContext, CanvasBarItem, CanvasBarKind, CanvasBarLayout, CanvasBarMenu, CanvasBarMeasure, CanvasBarPlacement, CanvasBarSide, CanvasBarView, place_canvas_bar, COMMAND_SEARCH_STYLE, CommandSearchStyle, CommandDescriptor, CommandFocus, CommandParameter, CommandSearchAction, CommandSearchView, ToolCategory};
pub mod keymaps;
mod settings;
mod shortcut_page;
mod shortcuts;
mod theme;
mod glass;
pub use glass::{BlurStyle, GlassColor, GlassPalette, Transparency};
mod workspace;
mod workspace_manager_ui;
pub use session::{ImageLayerDestination, ImagePlacementContext, LayerAction, LayerCanvasTool, LayerDropPosition, LayersView, RegionSource};
pub use workspace_manager_ui::{ManagedWorkspace, WorkspaceChoice, WorkspaceCommand};
mod stats;
pub use session::{HistogramAction, HistogramView};
pub use session::{
    AdjustmentChoice, ApplicationLink, ApplicationMenu, ClipboardCapture, CloseDecision,
    LARGE_CLIP_PIXELS, PasteMode, PixelClip,
    DEFAULT_DOCUMENT_EXTENT,
    DocumentColorOperation, DocumentIdleReason, DocumentHostError, DocumentHostErrorCopy, HostRequestFailure, DocumentExport, DocumentFileState, DocumentLocation, DocumentRequest, EffectAction, FilterCategoryChoice,
    FilterLoadState, FilterPickerAction, FilterPickerState, LayerPropertiesView,
    MAX_NEW_DOCUMENT_DIMENSION, PropertyControl, PropertyKind, PropertyPageView, CurveAxis, CurveAxisView, CurveControls, CurveCoordinateControl, CurveDomain, new_drawing, new_document_spec,
};
pub use stats::{StatRow, StatsView};

pub use camera::{Camera, MAX_ZOOM, MIN_ZOOM, TouchGesture};
pub use cursor::{CanvasCursor, CursorMode};
pub use customization::{ToolbarNameRefusal,
    ContextMenu, ContextMenuItem, ContextTarget, CustomizationAction, CustomizationState,
    PanelConfig, PanelContent, PanelControl, PanelControlView, PanelView, TabPresentation,
    TabStyle, TilePresentation, TileStyle, TileView, ToolChoice, ToolPickerView, ToolbarManagerView, ToolbarTile,
    canonical_tool_choice, tool_choice_localized,
};
pub use interaction::{
    ChromeEvent, ChromeFacts, InputReply, Modifiers, PenButton, PointerButton, PointerKind, StylusAction, TouchPolicy,
    UiInput,
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
    DropHint, BRUSH_SETS_MIN_WIDTH, LAYERS_MIN_WIDTH, PANEL_CONTENT_INSET, PanelKind, SURFACE_RADIUS, TAB_BAR_HEIGHT,
    TILE_GAP, TILE_SIZE,
    TOOL_PANEL_MIN_WIDTH, TOOL_SETTINGS_MIN_WIDTH, TabHit, TileLayout, tile_layout, toolbar_content_height,
    toolbar_tile_layout,
};
pub use numeric::{
    NumericControl, NumericError, WorkspaceValidationError, NumericKind, NumericMapping, NumericOperation, NumericRequest, NumericValue,
};
pub use session::{Notice, NoticeAction};
pub use session::{ScreenChip, ScreenDetails, ScreenState};
pub use session::{CanvasAnchor, CanvasAnchorChoice, CanvasSizeAction, CanvasSizeUnit, CanvasSizeView, CanvasUnitChoice};
pub use session::{ImageResample, ImageResampleChoice, ImageSizeAction, ImageSizeView};
pub use session::{FrequencySeparationAction, FrequencySeparationView};
pub use session::{LayerControls, PreparedWorkspace, ProofMode, UiSession, SelectionBrushOptions, SelectionMenu, SelectionAction, RefineKind, SelectionRefineView, SelectionDisplayOptions, MaskEditingView, SelectionTool, SelectionConstraint, SelectionOptions, SelectionMode};
pub use settings::{
    ChoicePresentation, HostRequest, HostRequestKind, Platform,
    PreferenceAction,
    PreferenceGroup, PreferenceId, PreferenceKind, PreferencePage, PreferenceReset, PreferenceRow,
    PreferenceSearchResult, PreferenceValue, PreferencesState, PreferencesView, Settings,
    SettingsPage, ShortcutEditor, Swatch, ZenIcon, MissingProfilePolicy, PhotoOpenPolicy,
    EraserEnd, ModifierKeyAction, ModifierKeyEditor,
};
pub use shortcut_page::{
    ActionPickerView, MODIFIER_SECTION, ModifierKeyRow, PenButtonEditor, PickerAction, PickerSection, ShortcutCategoryView,
    ShortcutContextChoice, ShortcutEmpty, ShortcutPageView, ShortcutShow, ShortcutShowChoice, TriggerRow,
};
pub use shortcuts::{
    BindingScope, GAMEPAD_BUTTONS, GESTURE_TRIGGERS, GestureTrigger, HoldKey, KeyChord, MODIFIER_CAPTURE, ShortcutAction, ShortcutCapture, ShortcutDefinition, ShortcutRow, TextEditAction,
    TextEditMenuItem, text_edit_menu, text_edit_menu_localized,
};
pub use theme::{
    ACCENTS, DEFAULT_ACCENT, HexColor, TRANSPARENCY_CHECKER, TRANSPARENCY_CHECKER_CELL, Theme,
    ThemePalette,
};
pub use workspace::{
    LayoutHistory, LayoutRevision, LayoutChange, LayoutPanelAction, LayoutPanelName, WorkspaceCapture, WorkspaceState, WorkspaceWorkingState,
    counter, durable_layout, layout_change_description,
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

#[derive(Clone, Debug, Serialize)]
pub struct BrushChoice {
    pub id: u32,
    pub group: ToolGroup,
    pub label: std::sync::Arc<str>,
    pub category: std::sync::Arc<str>,
    pub icon: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolbarControl {
    ToolSlot { slot: ToolSlotId },
    Command { command: CommandId },
    Brush { id: u32 },
    Size { pixels: u16 },
    Color,
    ColorPicker,
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

#[derive(Clone, Copy, Debug)]
pub struct MenuSpec {
    pub id: ApplicationMenu,
    pub sections: &'static [&'static [CommandId]],
}
pub const VIEW_MENU: MenuSpec = MenuSpec {
    id: ApplicationMenu::View,
    sections: &[
        &[CommandId::SoftProofSetup, CommandId::SoftProof, CommandId::GamutWarning, CommandId::SdrRendition, CommandId::PreviewSdr],
        &[CommandId::ZoomIn, CommandId::ZoomOut, CommandId::FitCanvas, CommandId::ActualPixels],
        &[CommandId::RotateLeft, CommandId::RotateRight],
        &[CommandId::FlipHorizontal, CommandId::FlipVertical],
        &[CommandId::ShowRulers, CommandId::SnapRulers],
        &[CommandId::ShowCanvasActionBar, CommandId::ZenMode, CommandId::Fullscreen],
        &[CommandId::ResetLayout],
    ],
};
/// GTK-first until the document transport is available on the other hosts.
pub const FILE_MENU: MenuSpec = MenuSpec {
    id: ApplicationMenu::File,
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
    pub label: std::sync::Arc<str>,
    pub kind: PanelKind,
}
#[derive(Clone, Debug, Serialize)]
pub struct BrushCategory {
    pub id: ToolGroup,
    pub label: std::sync::Arc<str>,
    pub icon: &'static str,
    pub brushes: Vec<BrushChoice>,
}
#[derive(Clone, Debug, Serialize)]
pub struct UiCatalog {
    pub native_copy: NativeCopy,
    pub profile_copy: color_feature_copy::ProfileCopy,
    pub export_copy: color_feature_copy::ExportCopy,
    pub document_color_copy: color_feature_copy::DocumentColorCopy,
    pub proof_copy: color_feature_copy::ProofCopy,
    pub document_delivery_copy: DocumentDeliveryCopy,
    pub command_search_style: CommandSearchStyle,
    pub canvas_bar_reappear_ms: u32,
    pub app_name: &'static str,
    pub text_size_pt: u8,
    pub zen_icon_size: u32,
    pub panel_expansion_ms: u32,
    pub panels: Vec<PanelChoice>,
    /// Primary drawing tools for hosts that also expose a compact tool chooser.
    pub tool_commands: &'static [CommandId],
    pub new_document: session::NewDocumentSpec,
    pub layer_commands: &'static [CommandId],
    pub brush_categories: Vec<BrushCategory>,
    pub brush_sizes: &'static [f32],
    pub brush_size: NumericControl,
    pub opacity: NumericControl,
    pub layer_opacity: NumericControl,
    pub zoom: NumericControl,
    /// Blend mode labels in code order, for hosts that show a flat list.
    pub layer_blends: Vec<std::sync::Arc<str>>,
}
pub fn ui_catalog() -> UiCatalog {
    ui_catalog_localized(&Localizer::shared(UiLanguage::English))
}
pub fn ui_catalog_localized(localization: &Localizer) -> UiCatalog {
    UiCatalog {
        native_copy: NativeCopy::new(localization),
        profile_copy: color_feature_copy::ProfileCopy::new(localization),
        export_copy: color_feature_copy::ExportCopy::new(localization),
        document_color_copy: color_feature_copy::DocumentColorCopy::new(localization),
        proof_copy: color_feature_copy::ProofCopy::new(localization),
        document_delivery_copy: DocumentDeliveryCopy::new(localization),
        command_search_style: COMMAND_SEARCH_STYLE,
        canvas_bar_reappear_ms: CANVAS_BAR_REAPPEAR_MS,
        zen_icon_size: ZEN_ICON_SIZE,
        app_name: APP_NAME,
        text_size_pt: UI_TEXT_PT,
        panel_expansion_ms: PANEL_EXPANSION_MS,
        panels: Panel::ALL
            .into_iter()
            .map(|id| PanelChoice {
                id,
                label: id.localized_label(localization),
                kind: id.kind(),
            })
            .collect(),
        new_document: session::new_document_spec(localization),
        layer_commands: &CommandId::LAYERS,
        tool_commands: &CommandId::TOOLS,
        brush_categories: tools::brush_categories_localized(localization).collect(),
        brush_sizes: BRUSH_SIZES,
        brush_size: NumericControl::brush_size(),
        opacity: NumericControl::percent(),
        layer_opacity: NumericControl::layer_opacity(),
        zoom: NumericControl::zoom(),
        layer_blends: layer_core::LayerBlend::ALL
            .iter()
            .map(|b| session::effects::blend_label(*b, localization))
            .collect(),
    }
}
macro_rules! command_ids {
    ($($name:ident),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum CommandId {
            $($name),*
        }
        impl CommandId {
            pub const ALL: [Self; [$(stringify!($name)),*].len()] = [$(Self::$name),*];
        }
    };
}
command_ids! {
    SearchCommands,
    DrawingBrush,
    Sculpt,
    SdrRendition,
    PreviewSdr,
    SoftProofSetup,
    SoftProof,
    GamutWarning,
    Histogram,
    Waveform,
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
    SelectionBrush,
    TonalSelect,
    QuickMask,
    ReturnToArtwork,
    NewSelectionLayer,
    SaveSelectionLayer,
    Reselect,
    SelectionOutline,
    MaskOverlay,
    MaskOverlayProtected,
    ResetMaskColors,
    SwapMaskColors,
    FillSelectionMask,
    ClearSelectionMask,
    SelectionBrushPressure,
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
    TransformAgain,
    TransformSnapping,
    ApplyTransform,
    CancelTransform,
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
    ToggleTheme,
    Settings,
    AddLayer,
    DeleteLayer,
    RaiseLayer,
    LowerLayer,
    ResetLayout,
    ZenMode,
    Fullscreen,
    NewWindow,
    KeyboardShortcuts,
    About,
    Website,
    SourceCode,
    Drawings,
    ShowCanvasActionBar,
    TransformFlipHorizontal,
    TransformFlipVertical,
    TransformRotateLeft,
    TransformRotateRight,
    ResetTransform,
    RemoveSelectionPoint,
    MaskSelection,
    TransformFree,
    TransformUniform,
    TransformDistort,
    TransformPerspective,
    TransformNearest,
    TransformBilinear,
    TransformBicubic,
    TransformWarp,
    WarpGridThree,
    WarpGridFour,
    WarpGridFive,
    WarpSplitVertical,
    WarpSplitHorizontal,
    WarpSplitCross,
    WarpSelectPoints,
    WarpResetGrid,

    UseReferenceBelow,
    ClearSelected,
    ClearOutside,
    CopySelectionToLayer,
    CutSelectionToLayer,
    ActualPixels,
    RevertToOriginal,
    ApplyTransformPixels,
    LoadSelectionLayer,
    InvertSelectionLayer,
    InvertLayerMask,
    LayerMaskEnabled,
    ApplyLayerMask,
    EditLayerMask,
    EditLayerContent,
    LassoFill,
    CanvasSize,
    CropCanvasToSelection,
    GrowSelection,
    ShrinkSelection,
    FeatherSelection,
    BorderSelection,
    SmoothSelection,
    TransformSelectionOutline,
    Crop,
    CropRatioFree,
    CropRatioOriginal,
    CropRatioSquare,
    CropRatioFourFive,
    CropRatioTwoThree,
    CropRatioFiveSeven,
    CropRatioSixteenNine,
    CropSwapOrientation,
    CropOverlayThirds,
    CropOverlayGrid,
    CropOverlayDiagonal,
    CropOverlayGolden,
    CropCycleOverlay,
    CropStraighten,
    CropDeleteCroppedPixels,
    StraightenToGuide,
    ImageSize,
    RotateImageLeft,
    RotateImageRight,
    RotateImage180,
    FlipImageHorizontal,
    FlipImageVertical,
    Trim,
    RevealAll,
    CropFitContent,
    TransformLanczos,
    MoveLeaveCopy,
    Copy,
    Cut,
    CopyMerged,
    PasteInPlace,
    PasteInto,
    MergeDown,
    MergeVisible,
    FlattenImage,
    StampVisible,
    MergeGroup,
    CloneSourceArm,
    Clone,
    CloneAligned,
    CloneFlipHorizontal,
    CloneFlipVertical,
    CloneResetOffset,
    ColorMixOklab,
    ColorMixLinear,
    ColorMixClassic,
    Heal,
    SpotHeal,
    BlendPerceptual,
    BlendLinear,
    NewDodgeBurnLayer,
    FrequencySeparation,
}
impl CommandId {
    pub fn available_on(self, platform: Platform) -> bool {
        match self {
            Self::Waveform => Panel::Waveform.available_on(platform),
            Self::Fullscreen => matches!(platform, Platform::Gtk | Platform::Web | Platform::Mac | Platform::Windows),
            Self::NewWindow => platform.native_windows(),
            _ => true,
        }
    }
    /// Polygon construction spans several contacts; these follow it live.
    pub fn follows_construction(self) -> bool {
        matches!(self, Self::CompleteSelection | Self::CancelSelection | Self::RemoveSelectionPoint)
    }
    /// Retained on/off commands can be presented as checkable menu items.
    pub fn is_toggle(self) -> bool {
        matches!(
            self,
            Self::QuickMask | Self::SelectionOutline | Self::MaskOverlay | Self::MaskOverlayProtected | Self::SelectionBrushPressure | Self::SelectionNew | Self::SelectionAdd | Self::SelectionSubtract | Self::SelectionIntersect | Self::SelectionAntialias | Self::SelectionConstrainAngles
                | Self::SelectionFixedRatio | Self::SelectionFixedSize | Self::SelectionFromCenter
                | Self::SelectionVisible | Self::SelectionEditing | Self::SelectionReference
                | Self::CropRatioFree | Self::CropRatioOriginal | Self::CropRatioSquare | Self::CropRatioFourFive
                | Self::CropRatioTwoThree | Self::CropRatioFiveSeven | Self::CropRatioSixteenNine
                | Self::CropOverlayThirds | Self::CropOverlayGrid | Self::CropOverlayDiagonal | Self::CropOverlayGolden
                | Self::CropStraighten | Self::CropDeleteCroppedPixels
                | Self::TransformSnapping | Self::TransformFree | Self::TransformUniform | Self::TransformDistort | Self::TransformPerspective
                | Self::TransformNearest | Self::TransformBilinear | Self::TransformBicubic | Self::TransformLanczos
                | Self::ColorMixOklab | Self::ColorMixLinear | Self::ColorMixClassic
                | Self::TransformWarp | Self::WarpGridThree | Self::WarpGridFour | Self::WarpGridFive
                | Self::WarpSplitVertical | Self::WarpSplitHorizontal | Self::WarpSplitCross | Self::WarpSelectPoints
                | Self::ZenMode
                | Self::Fullscreen
                | Self::ToggleTheme
                | Self::FlipHorizontal
                | Self::FlipVertical
                | Self::ShowRulers
                | Self::SnapRulers
                | Self::LayerMaskEnabled
                | Self::MoveLeaveCopy
                | Self::BlendPerceptual
                | Self::BlendLinear
                | Self::PreviewSdr
                | Self::SoftProof
                | Self::GamutWarning
                | Self::CloneSourceArm
                | Self::CloneAligned
                | Self::CloneFlipHorizontal
                | Self::CloneFlipVertical
        )
    }
    pub fn icon(self) -> Option<&'static str> {
        Some(match self {
            Self::SearchCommands => "search",
            Self::DrawingBrush => "drawing-tools",
            Self::Sculpt => "sculpt",
            Self::SdrRendition | Self::PreviewSdr | Self::SoftProofSetup | Self::SoftProof | Self::GamutWarning => "image",
            Self::Histogram => "stats",
            Self::Waveform => "waveform",
            Self::ImportImage | Self::RasterizeSource => "image",
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
            Self::SelectionBrush => "selection-brush",
            Self::TonalSelect => "tonal-select",
            Self::QuickMask => "mask",
            Self::ReturnToArtwork => "brush",
            Self::NewSelectionLayer => "add-layer",
            Self::SaveSelectionLayer => "save-document",
            Self::Reselect => "select-all",
            Self::SelectionOutline => "select",
            Self::MaskOverlay => "eye",
            Self::MaskOverlayProtected => "mask",
            Self::ResetMaskColors => "color",
            Self::SwapMaskColors => "swap",
            Self::FillSelectionMask => "fill",
            Self::ClearSelectionMask => "clear",

            Self::SelectionBrushPressure => "pen",
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
            Self::CloneSourceArm => "cursor-sight",
            Self::Clone => "clone",
            Self::Heal => "heal",
            Self::SpotHeal => "spot-heal",
            Self::CloneAligned => "link",
            Self::CloneFlipHorizontal => "flip-horizontal",
            Self::CloneFlipVertical => "flip-vertical",
            Self::CloneResetOffset => "reset",
            Self::SelectionVisible => "eye",
            Self::SelectionEditing => "layers",
            Self::SelectionReference => "reference",

            Self::Move => "move",
            Self::ScaleRotate | Self::TransformAgain => "transform",
            Self::TransformSnapping => "ruler-snap",
            Self::ApplyTransform => "check",
            Self::CancelTransform => "close",
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
            Self::ShowCanvasActionBar => "toolbar",
            Self::TransformFlipHorizontal => "flip-horizontal",
            Self::TransformFlipVertical => "flip-vertical",
            Self::TransformRotateLeft => "rotate-left",
            Self::TransformRotateRight => "rotate-right",
            Self::ResetTransform => "reset",
            Self::RemoveSelectionPoint => "back",
            Self::MaskSelection => "mask",
            Self::TransformFree => "transform",
            Self::TransformUniform => "link",
            Self::TransformDistort => "distort",
            Self::TransformPerspective => "perspective",
            Self::TransformNearest => "mosaic",
            Self::TransformBilinear => "blur",
            Self::TransformBicubic => "sharpen",
            Self::TransformWarp | Self::WarpGridThree | Self::WarpGridFour | Self::WarpGridFive
            | Self::WarpSplitVertical | Self::WarpSplitHorizontal | Self::WarpSplitCross | Self::WarpSelectPoints | Self::WarpResetGrid => "warp",
            Self::KeyboardShortcuts => "keyboard",
            Self::About => "info",
            Self::Website => "website",
            Self::SourceCode => "source-code",
            Self::UseReferenceBelow => "reference",
            Self::ClearSelected => "clear-selection",
            Self::ClearOutside => "clear-outside",
            Self::CopySelectionToLayer => "copy-to-layer",
            Self::CutSelectionToLayer => "cut-to-layer",
            Self::ActualPixels => "actual-pixels",
            Self::RevertToOriginal => "reset",
            Self::ApplyTransformPixels => "image",
            Self::LoadSelectionLayer => "selection-load",
            Self::InvertSelectionLayer | Self::InvertLayerMask => "invert-selection",
            Self::LayerMaskEnabled => "eye",
            Self::ApplyLayerMask | Self::EditLayerMask => "mask",
            Self::EditLayerContent => "brush",
            Self::LassoFill => "lasso-fill",
            Self::CanvasSize => "canvas-size",
            Self::CropCanvasToSelection | Self::Crop => "crop",
            Self::GrowSelection => "selection-grow",
            Self::ShrinkSelection => "selection-shrink",
            Self::FeatherSelection => "feather",
            Self::BorderSelection => "selection-border",
            Self::SmoothSelection => "edge-smooth",
            Self::TransformSelectionOutline => "transform-outline",
            Self::CropRatioFree
            | Self::CropRatioOriginal
            | Self::CropRatioSquare
            | Self::CropRatioFourFive
            | Self::CropRatioTwoThree
            | Self::CropRatioFiveSeven
            | Self::CropRatioSixteenNine => "aspect-ratio",
            Self::CropSwapOrientation => "swap-orientation",
            Self::CropOverlayThirds
            | Self::CropOverlayGrid
            | Self::CropOverlayDiagonal
            | Self::CropOverlayGolden
            | Self::CropCycleOverlay => "crop-guides",
            Self::CropStraighten | Self::StraightenToGuide => "straighten",
            Self::CropDeleteCroppedPixels => "crop-delete",
            Self::ImageSize => "image-size",
            Self::RotateImageLeft => "image-rotate-left",
            Self::RotateImageRight => "image-rotate-right",
            Self::RotateImage180 => "image-rotate-180",
            Self::FlipImageHorizontal => "image-flip-horizontal",
            Self::FlipImageVertical => "image-flip-vertical",
            Self::Trim => "trim",
            Self::RevealAll => "reveal-all",
            Self::CropFitContent => "fit-content",
            Self::TransformLanczos => "lanczos",
            Self::MoveLeaveCopy => "leave-copy",
            Self::Copy => "copy",
            Self::Cut => "cut",
            Self::CopyMerged => "copy-merged",
            Self::PasteImage => "paste",
            Self::PasteInPlace => "paste-in-place",
            Self::PasteInto => "paste-into",
            Self::MergeDown => "merge-down",
            Self::MergeVisible => "merge-visible",
            Self::FlattenImage => "flatten",
            Self::StampVisible => "stamp-visible",
            Self::MergeGroup => "merge-group",
            Self::ColorMixOklab => "color-mix-oklab",
            Self::ColorMixLinear => "color-mix-linear",
            Self::ColorMixClassic => "color-mix-classic",
            Self::BlendPerceptual | Self::BlendLinear => "blend",
            Self::NewDodgeBurnLayer => "dodge-burn",
            Self::FrequencySeparation => "frequency-separation",
        })
    }
    pub const TOOLS: [Self; 32] = [
        Self::SelectionBrush,
        Self::TonalSelect,
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
        Self::Clone,
        Self::Heal,
        Self::SpotHeal,
        Self::Lasso,
        Self::LassoFill,
        Self::Select,
        Self::RectangleSelect,
        Self::EllipseSelect,
        Self::PolygonSelect,
        Self::ColorSelect,
        Self::Move,
        Self::ScaleRotate,
        Self::Crop,
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
    pub fn message_id(self) -> MessageId {
        match self {
            Self::SearchCommands => MessageId::COMMAND_SEARCH_COMMANDS,
            Self::DrawingBrush => MessageId::COMMAND_DRAWING_BRUSH,
            Self::Sculpt => MessageId::COMMAND_SCULPT,
            Self::SdrRendition => MessageId::COMMAND_SDR_RENDITION,
            Self::PreviewSdr => MessageId::COMMAND_PREVIEW_SDR,
            Self::SoftProofSetup => MessageId::COMMAND_SOFT_PROOF_SETUP,
            Self::SoftProof => MessageId::COMMAND_SOFT_PROOF,
            Self::GamutWarning => MessageId::COMMAND_GAMUT_WARNING,
            Self::Histogram => MessageId::COMMAND_HISTOGRAM,
            Self::Waveform => MessageId::COMMAND_WAVEFORM,
            Self::ImportImage => MessageId::COMMAND_IMPORT_IMAGE,
            Self::PasteImage => MessageId::COMMAND_PASTE_IMAGE,
            Self::DocumentProperties => MessageId::COMMAND_DOCUMENT_PROPERTIES,
            Self::AssignProfile => MessageId::COMMAND_ASSIGN_PROFILE,
            Self::ConvertColorSpace => MessageId::COMMAND_CONVERT_COLOR_SPACE,
            Self::ChangeBitDepth => MessageId::COMMAND_CHANGE_BIT_DEPTH,
            Self::RepairSourceProfile => MessageId::COMMAND_REPAIR_SOURCE_PROFILE,
            Self::RasterizeSource => MessageId::COMMAND_RASTERIZE_SOURCE,
            Self::NewDocument => MessageId::COMMAND_NEW_DOCUMENT,
            Self::OpenDocument => MessageId::COMMAND_OPEN_DOCUMENT,
            Self::SaveDocument => MessageId::COMMAND_SAVE_DOCUMENT,
            Self::SaveDocumentAs => MessageId::COMMAND_SAVE_DOCUMENT_AS,
            Self::ExportDocument => MessageId::COMMAND_EXPORT_DOCUMENT,
            Self::CloseDocument => MessageId::COMMAND_CLOSE_DOCUMENT,
            Self::Pen => MessageId::COMMAND_PEN,
            Self::Pencil => MessageId::COMMAND_PENCIL,
            Self::Brush => MessageId::COMMAND_BRUSH,
            Self::Eraser => MessageId::COMMAND_ERASER,
            Self::Airbrush => MessageId::COMMAND_AIRBRUSH,
            Self::Decoration => MessageId::COMMAND_DECORATION,
            Self::Blend => MessageId::COMMAND_BLEND,
            Self::Liquify => MessageId::COMMAND_LIQUIFY,
            Self::Lasso => MessageId::COMMAND_LASSO,
            Self::Select => MessageId::COMMAND_SELECT,
            Self::RectangleSelect => MessageId::COMMAND_RECTANGLE_SELECT,
            Self::EllipseSelect => MessageId::COMMAND_ELLIPSE_SELECT,
            Self::PolygonSelect => MessageId::COMMAND_POLYGON_SELECT,
            Self::ColorSelect => MessageId::COMMAND_COLOR_SELECT,
            Self::SelectionBrush => MessageId::COMMAND_SELECTION_BRUSH,
            Self::TonalSelect => MessageId::COMMAND_TONAL_SELECT,
            Self::QuickMask => MessageId::COMMAND_QUICK_MASK,
            Self::ReturnToArtwork => MessageId::COMMAND_RETURN_TO_ARTWORK,
            Self::NewSelectionLayer => MessageId::COMMAND_NEW_SELECTION_LAYER,
            Self::SaveSelectionLayer => MessageId::COMMAND_SAVE_SELECTION_LAYER,
            Self::Reselect => MessageId::COMMAND_RESELECT,
            Self::SelectionOutline => MessageId::COMMAND_SELECTION_OUTLINE,
            Self::MaskOverlay => MessageId::COMMAND_MASK_OVERLAY,
            Self::MaskOverlayProtected => MessageId::COMMAND_MASK_OVERLAY_PROTECTED,
            Self::ResetMaskColors => MessageId::COMMAND_RESET_MASK_COLORS,
            Self::SwapMaskColors => MessageId::COMMAND_SWAP_MASK_COLORS,
            Self::FillSelectionMask => MessageId::COMMAND_FILL_SELECTION_MASK,
            Self::ClearSelectionMask => MessageId::COMMAND_CLEAR_SELECTION_MASK,
            Self::SelectionBrushPressure => MessageId::COMMAND_SELECTION_BRUSH_PRESSURE,
            Self::SelectionNew => MessageId::COMMAND_SELECTION_NEW,
            Self::SelectionAdd => MessageId::COMMAND_SELECTION_ADD,
            Self::SelectionSubtract => MessageId::COMMAND_SELECTION_SUBTRACT,
            Self::SelectionIntersect => MessageId::COMMAND_SELECTION_INTERSECT,
            Self::SelectionAntialias => MessageId::COMMAND_SELECTION_ANTIALIAS,
            Self::SelectionConstrainAngles => MessageId::COMMAND_SELECTION_CONSTRAIN_ANGLES,
            Self::SelectionFixedRatio => MessageId::COMMAND_SELECTION_FIXED_RATIO,
            Self::SelectionFixedSize => MessageId::COMMAND_SELECTION_FIXED_SIZE,
            Self::SelectionFromCenter => MessageId::COMMAND_SELECTION_FROM_CENTER,
            Self::CompleteSelection => MessageId::COMMAND_COMPLETE_SELECTION,
            Self::CancelSelection => MessageId::COMMAND_CANCEL_SELECTION,
            Self::CloneSourceArm => MessageId::COMMAND_CLONE_SOURCE_ARM,
            Self::Clone => MessageId::COMMAND_CLONE,
            Self::Heal => MessageId::COMMAND_HEAL,
            Self::SpotHeal => MessageId::COMMAND_SPOT_HEAL,
            Self::CloneAligned => MessageId::COMMAND_CLONE_ALIGNED,
            Self::CloneFlipHorizontal => MessageId::COMMAND_CLONE_FLIP_HORIZONTAL,
            Self::CloneFlipVertical => MessageId::COMMAND_CLONE_FLIP_VERTICAL,
            Self::CloneResetOffset => MessageId::COMMAND_CLONE_RESET_OFFSET,
            Self::SelectionVisible => MessageId::COMMAND_SELECTION_VISIBLE,
            Self::SelectionEditing => MessageId::COMMAND_SELECTION_EDITING,
            Self::SelectionReference => MessageId::COMMAND_SELECTION_REFERENCE,
            Self::Move => MessageId::COMMAND_MOVE,
            Self::ScaleRotate => MessageId::COMMAND_SCALE_ROTATE,
            Self::TransformAgain => MessageId::COMMAND_TRANSFORM_AGAIN,
            Self::TransformSnapping => MessageId::COMMAND_TRANSFORM_SNAPPING,
            Self::ApplyTransform => MessageId::COMMAND_APPLY_TRANSFORM,
            Self::CancelTransform => MessageId::COMMAND_CANCEL_TRANSFORM,
            Self::PlacementOriginalSize => MessageId::COMMAND_PLACEMENT_ORIGINAL_SIZE,
            Self::Hand => MessageId::COMMAND_HAND,
            Self::Eyedropper => MessageId::COMMAND_EYEDROPPER,
            Self::Gradient => MessageId::COMMAND_GRADIENT,
            Self::Figure => MessageId::COMMAND_FIGURE,
            Self::Ruler => MessageId::COMMAND_RULER,
            Self::ShowRulers => MessageId::COMMAND_SHOW_RULERS,
            Self::SnapRulers => MessageId::COMMAND_SNAP_RULERS,
            Self::DeleteRuler => MessageId::COMMAND_DELETE_RULER,
            Self::AutoSelect => MessageId::COMMAND_AUTO_SELECT,
            Self::Fill => MessageId::COMMAND_FILL,
            Self::Undo => MessageId::COMMAND_UNDO,
            Self::Redo => MessageId::COMMAND_REDO,
            Self::ClearLayer => MessageId::COMMAND_CLEAR_LAYER,
            Self::FillSelection => MessageId::COMMAND_FILL_SELECTION,
            Self::SelectAll => MessageId::COMMAND_SELECT_ALL,
            Self::Deselect => MessageId::COMMAND_DESELECT,
            Self::InvertSelection => MessageId::COMMAND_INVERT_SELECTION,
            Self::UndoWorkspace => MessageId::COMMAND_UNDO_WORKSPACE,
            Self::RedoWorkspace => MessageId::COMMAND_REDO_WORKSPACE,
            Self::NewToolbar => MessageId::COMMAND_NEW_TOOLBAR,
            Self::ManageToolbars => MessageId::COMMAND_MANAGE_TOOLBARS,
            Self::CustomizeWorkspaceUi => MessageId::COMMAND_CUSTOMIZE_WORKSPACE_UI,
            Self::FitCanvas => MessageId::COMMAND_FIT_CANVAS,
            Self::ZoomIn => MessageId::COMMAND_ZOOM_IN,
            Self::ZoomOut => MessageId::COMMAND_ZOOM_OUT,
            Self::RotateLeft => MessageId::COMMAND_ROTATE_LEFT,
            Self::RotateRight => MessageId::COMMAND_ROTATE_RIGHT,
            Self::FlipHorizontal => MessageId::COMMAND_FLIP_HORIZONTAL,
            Self::FlipVertical => MessageId::COMMAND_FLIP_VERTICAL,
            Self::Settings => MessageId::COMMAND_SETTINGS,
            Self::ToggleTheme => MessageId::COMMAND_TOGGLE_THEME,
            Self::AddLayer => MessageId::COMMAND_ADD_LAYER,
            Self::DeleteLayer => MessageId::COMMAND_DELETE_LAYER,
            Self::RaiseLayer => MessageId::COMMAND_RAISE_LAYER,
            Self::LowerLayer => MessageId::COMMAND_LOWER_LAYER,
            Self::ResetLayout => MessageId::COMMAND_RESET_LAYOUT,
            Self::ZenMode => MessageId::COMMAND_ZEN_MODE,
            Self::Fullscreen => MessageId::COMMAND_FULLSCREEN,
            Self::NewWindow => MessageId::COMMAND_NEW_WINDOW,
            Self::Drawings => MessageId::COMMAND_DRAWINGS,
            Self::ShowCanvasActionBar => MessageId::COMMAND_SHOW_CANVAS_ACTION_BAR,
            Self::TransformFlipHorizontal => MessageId::COMMAND_TRANSFORM_FLIP_HORIZONTAL,
            Self::TransformFlipVertical => MessageId::COMMAND_TRANSFORM_FLIP_VERTICAL,
            Self::TransformRotateLeft => MessageId::COMMAND_TRANSFORM_ROTATE_LEFT,
            Self::TransformRotateRight => MessageId::COMMAND_TRANSFORM_ROTATE_RIGHT,
            Self::ResetTransform => MessageId::COMMAND_RESET_TRANSFORM,
            Self::RemoveSelectionPoint => MessageId::COMMAND_REMOVE_SELECTION_POINT,
            Self::MaskSelection => MessageId::COMMAND_MASK_SELECTION,
            Self::TransformFree => MessageId::COMMAND_TRANSFORM_FREE,
            Self::TransformUniform => MessageId::COMMAND_TRANSFORM_UNIFORM,
            Self::TransformDistort => MessageId::COMMAND_TRANSFORM_DISTORT,
            Self::TransformPerspective => MessageId::COMMAND_TRANSFORM_PERSPECTIVE,
            Self::TransformNearest => MessageId::COMMAND_TRANSFORM_NEAREST,
            Self::TransformBilinear => MessageId::COMMAND_TRANSFORM_BILINEAR,
            Self::TransformBicubic => MessageId::COMMAND_TRANSFORM_BICUBIC,
            Self::TransformWarp => MessageId::COMMAND_TRANSFORM_WARP,
            Self::WarpGridThree => MessageId::COMMAND_WARP_GRID_THREE,
            Self::WarpGridFour => MessageId::COMMAND_WARP_GRID_FOUR,
            Self::WarpGridFive => MessageId::COMMAND_WARP_GRID_FIVE,
            Self::WarpSplitVertical => MessageId::COMMAND_WARP_SPLIT_VERTICAL,
            Self::WarpSplitHorizontal => MessageId::COMMAND_WARP_SPLIT_HORIZONTAL,
            Self::WarpSplitCross => MessageId::COMMAND_WARP_SPLIT_CROSS,
            Self::WarpSelectPoints => MessageId::COMMAND_WARP_SELECT_POINTS,
            Self::WarpResetGrid => MessageId::COMMAND_WARP_RESET_GRID,

            Self::KeyboardShortcuts => MessageId::COMMAND_KEYBOARD_SHORTCUTS,
            Self::About => MessageId::COMMAND_ABOUT,
            Self::Website => MessageId::COMMAND_WEBSITE,
            Self::SourceCode => MessageId::COMMAND_SOURCE_CODE,
            Self::UseReferenceBelow => MessageId::COMMAND_USE_REFERENCE_BELOW,
            Self::ClearSelected => MessageId::COMMAND_CLEAR_SELECTED,
            Self::ClearOutside => MessageId::COMMAND_CLEAR_OUTSIDE,
            Self::CopySelectionToLayer => MessageId::COMMAND_COPY_SELECTION_TO_LAYER,
            Self::CutSelectionToLayer => MessageId::COMMAND_CUT_SELECTION_TO_LAYER,
            Self::ActualPixels => MessageId::COMMAND_ACTUAL_PIXELS,
            Self::RevertToOriginal => MessageId::COMMAND_REVERT_TO_ORIGINAL,
            Self::ApplyTransformPixels => MessageId::COMMAND_APPLY_TRANSFORM_PIXELS,
            Self::LoadSelectionLayer => MessageId::COMMAND_LOAD_SELECTION_LAYER,
            Self::InvertSelectionLayer => MessageId::COMMAND_INVERT_SELECTION_LAYER,
            Self::InvertLayerMask => MessageId::COMMAND_INVERT_LAYER_MASK,
            Self::LayerMaskEnabled => MessageId::COMMAND_LAYER_MASK_ENABLED,
            Self::ApplyLayerMask => MessageId::COMMAND_APPLY_LAYER_MASK,
            Self::EditLayerMask => MessageId::COMMAND_EDIT_LAYER_MASK,
            Self::EditLayerContent => MessageId::COMMAND_EDIT_LAYER_CONTENT,
            Self::LassoFill => MessageId::COMMAND_LASSO_FILL,
            Self::CanvasSize => MessageId::COMMAND_CANVAS_SIZE,
            Self::CropCanvasToSelection => MessageId::COMMAND_CROP_CANVAS_TO_SELECTION,
            Self::GrowSelection => MessageId::COMMAND_GROW_SELECTION,
            Self::ShrinkSelection => MessageId::COMMAND_SHRINK_SELECTION,
            Self::FeatherSelection => MessageId::COMMAND_FEATHER_SELECTION,
            Self::BorderSelection => MessageId::COMMAND_BORDER_SELECTION,
            Self::SmoothSelection => MessageId::COMMAND_SMOOTH_SELECTION,
            Self::TransformSelectionOutline => MessageId::COMMAND_TRANSFORM_SELECTION_OUTLINE,
            Self::Crop => MessageId::COMMAND_CROP,
            Self::CropRatioFree => MessageId::COMMAND_CROP_RATIO_FREE,
            Self::CropRatioOriginal => MessageId::COMMAND_CROP_RATIO_ORIGINAL,
            Self::CropRatioSquare => MessageId::COMMAND_CROP_RATIO_SQUARE,
            Self::CropRatioFourFive => MessageId::COMMAND_CROP_RATIO_FOUR_FIVE,
            Self::CropRatioTwoThree => MessageId::COMMAND_CROP_RATIO_TWO_THREE,
            Self::CropRatioFiveSeven => MessageId::COMMAND_CROP_RATIO_FIVE_SEVEN,
            Self::CropRatioSixteenNine => MessageId::COMMAND_CROP_RATIO_SIXTEEN_NINE,
            Self::CropSwapOrientation => MessageId::COMMAND_CROP_SWAP_ORIENTATION,
            Self::CropOverlayThirds => MessageId::COMMAND_CROP_OVERLAY_THIRDS,
            Self::CropOverlayGrid => MessageId::COMMAND_CROP_OVERLAY_GRID,
            Self::CropOverlayDiagonal => MessageId::COMMAND_CROP_OVERLAY_DIAGONAL,
            Self::CropOverlayGolden => MessageId::COMMAND_CROP_OVERLAY_GOLDEN,
            Self::CropCycleOverlay => MessageId::COMMAND_CROP_CYCLE_OVERLAY,
            Self::CropStraighten => MessageId::COMMAND_CROP_STRAIGHTEN,
            Self::CropDeleteCroppedPixels => MessageId::COMMAND_CROP_DELETE_CROPPED_PIXELS,
            Self::StraightenToGuide => MessageId::COMMAND_STRAIGHTEN_TO_GUIDE,
            Self::ImageSize => MessageId::COMMAND_IMAGE_SIZE,
            Self::RotateImageLeft => MessageId::COMMAND_ROTATE_IMAGE_LEFT,
            Self::RotateImageRight => MessageId::COMMAND_ROTATE_IMAGE_RIGHT,
            Self::RotateImage180 => MessageId::COMMAND_ROTATE_IMAGE180,
            Self::FlipImageHorizontal => MessageId::COMMAND_FLIP_IMAGE_HORIZONTAL,
            Self::FlipImageVertical => MessageId::COMMAND_FLIP_IMAGE_VERTICAL,
            Self::Trim => MessageId::COMMAND_TRIM,
            Self::RevealAll => MessageId::COMMAND_REVEAL_ALL,
            Self::CropFitContent => MessageId::COMMAND_CROP_FIT_CONTENT,
            Self::TransformLanczos => MessageId::COMMAND_TRANSFORM_LANCZOS,
            Self::MoveLeaveCopy => MessageId::COMMAND_MOVE_LEAVE_COPY,
            Self::Copy => MessageId::COMMAND_COPY,
            Self::Cut => MessageId::COMMAND_CUT,
            Self::CopyMerged => MessageId::COMMAND_COPY_MERGED,
            Self::PasteInPlace => MessageId::COMMAND_PASTE_IN_PLACE,
            Self::PasteInto => MessageId::COMMAND_PASTE_INTO,
            Self::MergeDown => MessageId::COMMAND_MERGE_DOWN,
            Self::MergeVisible => MessageId::COMMAND_MERGE_VISIBLE,
            Self::FlattenImage => MessageId::COMMAND_FLATTEN_IMAGE,
            Self::StampVisible => MessageId::COMMAND_STAMP_VISIBLE,
            Self::MergeGroup => MessageId::COMMAND_MERGE_GROUP,
            Self::ColorMixOklab => MessageId::COMMAND_COLOR_MIX_OKLAB,
            Self::ColorMixLinear => MessageId::COMMAND_COLOR_MIX_LINEAR,
            Self::ColorMixClassic => MessageId::COMMAND_COLOR_MIX_CLASSIC,
            Self::BlendPerceptual => MessageId::COMMAND_BLEND_PERCEPTUAL,
            Self::BlendLinear => MessageId::COMMAND_BLEND_LINEAR,
            Self::NewDodgeBurnLayer => MessageId::COMMAND_NEW_DODGE_BURN_LAYER,
            Self::FrequencySeparation => MessageId::COMMAND_FREQUENCY_SEPARATION,
        }
    }

    pub fn localized_label(self, localization: &Localizer) -> std::sync::Arc<str> {
        localization.text(self.message_id())
    }

    pub fn label(self) -> std::sync::Arc<str> {
        self.localized_label(&Localizer::shared(UiLanguage::English))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CommandState {
    pub id: CommandId,
    /// Use a native checkable menu item only for retained on/off commands.
    pub checkable: bool,
    pub icon: Option<&'static str>,
    pub label: std::sync::Arc<str>,
    pub enabled: bool,
    /// Why a disabled command is unavailable; always serialized, as null when
    /// enabled. Retained controls keep it steady during canvas input, as they
    /// keep `enabled`.
    pub disabled_reason: Option<std::sync::Arc<str>>,
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
    pub selection_layer: bool,
    pub quick_mask: bool,
    pub can_rename: bool,
    pub content_icon: Option<String>,
    pub content_icon_color: Option<HexColor>,
    pub label: String,
    pub description: String,
    pub can_delete: bool,
    pub can_alpha_lock: bool,
    pub visible: bool,
    pub opacity: f32,
    pub selected: bool,
    pub mask_selected: bool,
    /// Checked-selection precedence is shared across native hosts.
    pub selection_icon: &'static str,
    pub load_selection_tooltip: &'static str,
    /// Content/mask target, independent of the selected row set.
    pub editing: bool,
    pub drawing: bool,
    pub has_mask: bool,
    pub mask_enabled: bool,
    pub mask_linked: bool,
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
    #[serde(skip)]
    pub tool_slots: ToolSlotMemory,
    pub histogram: HistogramView,
    pub waveform: HistogramView,
    pub tonal_histogram: HistogramView,
    #[serde(skip)]
    pub(crate) localization: std::sync::Arc<Localizer>,
    pub command_search: Option<CommandSearchView>,
    /// Temporary viewing choices, excluded from document and workspace saving.
    pub soft_proof: bool,
    pub preview_sdr: bool,
    pub hdr_display_available: bool,
    pub screen: ScreenState,
    pub gamut_warning: bool,
    pub revision: u64,
    /// Observed native/browser window state; never stored in workspace preferences.
    pub fullscreen: bool,
    pub workspace: WorkspaceState,
    pub brush: BrushState,
    pub colors: ColorState,
    pub color_picker: ColorPickerState,
    pub tool_settings: Vec<ToolSetting>,
    pub tool_extra: Vec<ToolOption>,
    pub toolbar_context_generation: u64,
    pub tool_actions: Vec<ToolSettingAction>,
    pub tool_set: ToolSetView,
    pub tool_panels: ToolPanels,
    pub canvas_bar: Option<CanvasBarView>,
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
    /// File and renderer errors. Canvas gesture refusals use `notice`.
    pub host_error: Option<String>,
    /// A transient refusal or hint, published under `regions::HOST`.
    pub notice: Option<Notice>,
    pub camera: Camera,
}

/// The same payload works for buttons, menus, shortcuts, and accessibility.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiAction {
    ChooseToolVariant { anchor: DrawerAnchor, variant: ToolVariant },
    Histogram { action: HistogramAction },
    CommandSearch { action: CommandSearchAction },
    Selection { action: SelectionAction },
    CanvasSize { action: CanvasSizeAction },
    ImageSize { action: ImageSizeAction },
    FrequencySeparation { action: FrequencySeparationAction },
    Tonal { action: TonalAction },
    TransformReference { reference: CanvasAnchor },
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
    ShowClippedColors {
        visible: bool,
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
    /// Zoom about the work-area centre, clamped to the camera limits.
    SetZoom {
        zoom: f32,
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
    StepToolSetting {
        id: String,
        steps: f32,
    },
    ResetToolSetting {
        id: String,
    },
    /// A retained toolbar editor must never apply to a different tool/target.
    ToolbarEdit {
        context: ToolbarContext,
        action: Box<UiAction>,
    },
    CanvasBarEdit {
        context: CanvasBarContext,
        action: Box<UiAction>,
    },
    /// Accept runs the notice's action; decline dismisses it. A notice that
    /// was replaced or cleared is rejected.
    Notice {
        id: u64,
        accept: bool,
    },
    ColorPicker {
        action: ColorPickerAction,
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
    SetTheme {
        theme: Option<Theme>,
    },
    SystemThemeChanged {
        theme: Theme,
        #[serde(default)]
        accent: Option<HexColor>,
    },
    NewDocumentPreferences {
        action: NewDocumentAction,
    },
    OpenSettings {
        page: SettingsPage,
    },
    Preferences {
        action: PreferenceAction,
    },
    RestoreSettings {
        settings: Settings,
    },
    /// Settings as a host saved them, possibly by another build.
    RestoreSavedSettings {
        saved: String,
    },
    CompleteRequest {
        id: u32,
        error: Option<String>,
    },
    CompleteRequestFailure {
        id: u32,
        reason: HostRequestFailure,
    },
    CloseSettings,
}
impl UiAction {
    pub(crate) fn is_host_report(&self) -> bool {
        matches!(
            self,
            Self::CompleteRequest { .. }
                | Self::CompleteRequestFailure { .. }
                | Self::CloseSettings
                | Self::RestoreSettings { .. }
                | Self::RestoreSavedSettings { .. }
                | Self::MeasureColumnDrawers { .. }
                | Self::MeasureDrawerTiles { .. }
                | Self::MeasureColumnScroll { .. }
                | Self::MeasurePanels { .. }
                | Self::MeasureTitlebar { .. }
                | Self::MeasureHeader { .. }
                | Self::MeasureWorkspaceBottom { .. }
                | Self::SystemThemeChanged { .. }
                | Self::WindowFullscreen { .. }
        )
    }
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
    pub const COLOR_PREVIEW: u32 = 256;
    pub const COMMAND_SEARCH: u32 = 512;
    pub const CANVAS_BAR: u32 = 1024;
    pub const HISTOGRAM: u32 = 2048;
    pub const ALL: u32 = 4095;
}

#[cfg(test)]
pub(crate) fn icon_ships(icon: &str) -> bool {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../apps/layer-web/icons/layer-{icon}-symbolic.svg"))
        .is_file()
}

#[cfg(test)]
mod icon_tests {
    use super::*;

    fn required_icons() -> std::collections::BTreeSet<&'static str> {
        CommandId::ALL
            .into_iter()
            .filter_map(CommandId::icon)
            .chain(CursorMode::CHOICES.iter().map(|(mode, _)| mode.icon()))
            .chain(ZenIcon::CHOICES.iter().map(|(icon, _)| icon.icon()))
            .chain(Panel::ALL.into_iter().map(Panel::icon))
            .chain(brush_categories().map(|c| c.icon))
            .chain(layer_core::bundled_effect_catalog().filters().iter().map(|e| e.icon.as_ref()))
            .collect()
    }

    #[test]
    fn bundled_filters_have_specific_icons_and_catalog_assets_ship() {
        for icon in required_icons() {
            assert!(icon_ships(icon), "{icon}: missing asset");
        }
        let mut meanings = std::collections::BTreeSet::new();
        for filter in layer_core::bundled_effect_catalog().filters() {
            assert_ne!(filter.icon.as_ref(), "adjustments", "{} must not use the picker icon", filter.program.id);
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
        for command in CommandId::ALL {
            let icon = command
                .icon()
                .expect("toolbar commands need meaningful icons");
            assert!(icon_ships(icon), "{command:?}: missing SVG");
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
            &[CommandId::FitCanvas, CommandId::ScaleRotate, CommandId::TransformSelectionOutline],
            &[CommandId::GrowSelection, CommandId::ShrinkSelection, CommandId::FeatherSelection, CommandId::BorderSelection, CommandId::SmoothSelection],
            &[CommandId::Fill, CommandId::FillSelection],
            &[CommandId::Undo, CommandId::CancelTransform],
            &[CommandId::RotateLeft, CommandId::RotateImageLeft],
            &[CommandId::RotateRight, CommandId::RotateImageRight],
            &[CommandId::FlipHorizontal, CommandId::FlipImageHorizontal],
            &[CommandId::FlipVertical, CommandId::FlipImageVertical],
            &[CommandId::CanvasSize, CommandId::ImageSize, CommandId::CropFitContent, CommandId::FitCanvas],
            &[CommandId::TransformNearest, CommandId::TransformBilinear, CommandId::TransformBicubic, CommandId::TransformLanczos],
            &[CommandId::ColorMixOklab, CommandId::ColorMixLinear, CommandId::ColorMixClassic],
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

pub fn normalize_search(text: &str) -> String { search::normalize(text) }

pub mod color_feature_copy;
pub mod common_copy;
pub use common_copy::CommonCopy;
pub use keymaps::{shortcut_default_caption, modifier_hold_help};

pub mod native_copy;
pub use native_copy::{NativeCopy, NativeCaption, RecoveryCopy};
pub mod document_properties;
pub use document_properties::{DocumentPropertiesView, document_properties};
pub mod color_feature_error;
pub use color_feature_error::ColorFeatureError;
