use crate::{FluentArgs, Localizer, MessageId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

macro_rules! copy_struct {
    ($name:ident { $($field:ident: $message:ident $(=> $argument:ident)?,)* }) => {
        #[derive(Clone, Debug, Serialize)]
        pub struct $name { $(pub $field: Arc<str>,)* }
        impl $name {
            pub(crate) fn new(l: &Localizer) -> Self { Self { $($field: copy_struct!(@value l, $message $(, $argument)?),)* } }
        }
    };
    (@value $l:ident, $message:ident) => { $l.text(MessageId::$message) };
    (@value $l:ident, $message:ident, $argument:ident) => {
        crate::color_feature_copy::named($l, MessageId::$message, &$l.text(MessageId::$argument)).into()
    };
}

copy_struct! { SearchCopy {
    search_commands: NATIVE_SEARCH_SEARCH_COMMANDS,
    enter_value: NATIVE_SEARCH_ENTER_VALUE,
    close_search: NATIVE_SEARCH_CLOSE_SEARCH,
    no_matches: NATIVE_SEARCH_NO_MATCHES,
    commands: COMMANDS_COMMANDS,
    title: NATIVE_SEARCH_TITLE,
    selected: NATIVE_SEARCH_SELECTED,
} }

copy_struct! { LayerCopy {
    layer: MENU_LAYER,
    actions: WORKSPACE_CONTROL_LAYER_ACTIONS,
    blend_opacity: NATIVE_LAYERS_BLEND_OPACITY,
    blend: NATIVE_LAYERS_BLEND,
    controls: NATIVE_LAYERS_CONTROLS,
    flags: NATIVE_LAYERS_FLAGS,
    mask_preview: NATIVE_LAYERS_MASK_PREVIEW,
    name: NATIVE_LAYERS_NAME,
    preview: NATIVE_LAYERS_PREVIEW,
    row: NATIVE_LAYERS_ROW,
    title: WORKSPACE_PANEL_LAYERS,
    new_layer: COMMAND_ADD_LAYER,
    new_selection_layer: COMMAND_NEW_SELECTION_LAYER,
    import_image: COMMAND_IMPORT_IMAGE,
    alpha_lock: RESOURCES_LAYER_MENU_ALPHA_LOCK,
    lock_editing: RESOURCES_LAYER_MENU_LOCK_EDITING,
    clip: RESOURCES_LAYER_MENU_CLIP_TO_LAYER_BELOW,
    new_group: RESOURCES_LAYER_MENU_NEW_GROUP,
    add_mask: RESOURCES_LAYER_MENU_ADD_MASK,
    delete_selected: RESOURCES_LAYER_MENU_DELETE_SELECTED_LAYERS,
    opacity: WORKSPACE_CONTROL_LAYER_OPACITY,
    visibility: RESOURCES_LAYER_MENU_VISIBILITY,
    editing_mask: NATIVE_LAYERS_EDITING_MASK,
    editing_selection: NATIVE_LAYERS_EDITING_SELECTION,
    drawing_target: NATIVE_LAYERS_DRAWING_TARGET,
    selected: NATIVE_SEARCH_SELECTED,
    select_row_help: NATIVE_LAYERS_SELECT_ROW_HELP,
    expanded: NATIVE_LAYERS_EXPANDED,
    collapsed: NATIVE_LAYERS_COLLAPSED,
    move_layer: NATIVE_LAYERS_MOVE_LAYER,
    hide_selection: NATIVE_LAYERS_HIDE_SELECTION,
    show_selection: NATIVE_LAYERS_SHOW_SELECTION,
    hide: NATIVE_LAYERS_HIDE,
    show: NATIVE_LAYERS_SHOW,
    edit_selection: NATIVE_LAYERS_EDIT_SELECTION,
    expand: NATIVE_LAYERS_EXPAND,
    collapse: NATIVE_LAYERS_COLLAPSE,
    edit_content: NATIVE_LAYERS_EDIT_CONTENT,
    edit_mask: NATIVE_LAYERS_EDIT_MASK,
    link_mask: NATIVE_LAYERS_LINK_MASK,
    unlink_mask: NATIVE_LAYERS_UNLINK_MASK,
    link_mask_to_layer: NATIVE_LAYERS_LINK_MASK_TO_LAYER,
    locked: NATIVE_LAYERS_LOCKED,
    alpha_locked: NATIVE_LAYERS_ALPHA_LOCKED,
    unselected: NATIVE_LAYERS_UNSELECTED,
    pending: NATIVE_LAYERS_PENDING,
    drop_into: NATIVE_LAYERS_DROP_INTO,
    drop_above: NATIVE_LAYERS_DROP_ABOVE,
    drop_below: NATIVE_LAYERS_DROP_BELOW,
} }

copy_struct! { PaletteCopy {
    hex_help: NATIVE_PALETTES_HEX_HELP,
    title: WORKSPACE_PANEL_PALETTES,
    rename_title: NATIVE_PALETTES_RENAME_TITLE,
    new_title: NATIVE_PALETTES_NEW_TITLE,
    palette_name: NATIVE_PALETTES_PALETTE_NAME,
    import: NATIVE_PALETTES_IMPORT,
    export: NATIVE_PALETTES_EXPORT,
    history_empty: NATIVE_PALETTES_HISTORY_EMPTY,
    expand_history: NATIVE_PALETTES_EXPAND_HISTORY,
    collapse_history: NATIVE_PALETTES_COLLAPSE_HISTORY,
    recent: NATIVE_PALETTES_RECENT,
    history_help: NATIVE_PALETTES_HISTORY_HELP,
    saved: NATIVE_PALETTES_SAVED,
    new_import: NATIVE_PALETTES_NEW_IMPORT,
    add_current: NATIVE_PALETTES_ADD_CURRENT,
    rename_help: NATIVE_PALETTES_RENAME_HELP,
    read_failed: NATIVE_PALETTES_READ_FAILED,
    import_failed: NATIVE_PALETTES_IMPORT_FAILED,
    export_failed: NATIVE_PALETTES_EXPORT_FAILED,

    find: NATIVE_PALETTES_FIND,
    no_matches: NATIVE_PALETTES_NO_MATCHES,
    remove_title: NATIVE_PALETTES_REMOVE_TITLE,
    name: NATIVE_PALETTES_NAME,
    choose: NATIVE_PALETTES_CHOOSE,
    options: NATIVE_PALETTES_OPTIONS,
} }

copy_struct! { HeaderCopy {
    save_as_new_workspace: WORKSPACE_ACTION_SAVE_AS_NEW_WORKSPACE,
    owned_elsewhere: WORKSPACE_REFUSAL_THIS_WORKSPACE_IS_OPEN_IN_ANOTHER_WINDOW,
    on: SETTINGS_ON,
    off: SETTINGS_OFF,
    zoom: MENU_ZOOM,
    this_workspace: WORKSPACE_THIS_WORKSPACE,
    saved_toolbars: WORKSPACE_SAVED_TOOLBARS,
    toolbar_actions: NATIVE_SHORTCUTS_ACTIONS,
    new_workspace: WORKSPACE_NEW_WORKSPACE,
    new_toolbar: WORKSPACE_NEW_TOOLBAR,
    search: COLOR_FEATURES_PROFILE_SEARCH,
    retry: WORKSPACE_ACTION_RETRY_STORAGE,
    title_bar: WORKSPACE_HEADER_TITLE,
    title_bar_size: WORKSPACE_HEADER_SIZE,
    clock: WORKSPACE_HEADER_CLOCK,
    battery: WORKSPACE_HEADER_BATTERY,
    current_workspace: WORKSPACE_CURRENT_WORKSPACE,
    drag_to_reorder: WORKSPACE_HEADER_DRAG_ITEM,
    show_top: NATIVE_HEADER_SHOW_TOP,
    shown_top: NATIVE_HEADER_SHOWN_TOP,
    move_up: NATIVE_HEADER_MOVE_UP,
    move_down: NATIVE_HEADER_MOVE_DOWN,
    no_items: NATIVE_HEADER_NO_ITEMS,

    application_header: NATIVE_HEADER_APPLICATION_HEADER,
    more_items: NATIVE_HEADER_MORE_ITEMS,
    task_workspaces: NATIVE_HEADER_TASK_WORKSPACES,
    main_menu: MENU_MAIN_MENU,
    menus: NATIVE_HEADER_MENUS,
    workspaces: WORKSPACE_WORKSPACES,
    customize: NATIVE_HEADER_CUSTOMIZE,
    drawings: NATIVE_HEADER_DRAWINGS,
    choose_drawing: NATIVE_HEADER_CHOOSE_DRAWING,
    close_drawing: NATIVE_HEADER_CLOSE_DRAWING,
    reorder_drawing: NATIVE_HEADER_REORDER_DRAWING,
    slide: NATIVE_HEADER_SLIDE,
    move_drawing: NATIVE_HEADER_MOVE_DRAWING,
    move_earlier: WORKSPACE_HEADER_MOVE_EARLIER,
    move_later: WORKSPACE_HEADER_MOVE_LATER,
    retry_storage: NATIVE_HEADER_RETRY_STORAGE,
    move_left_up: NATIVE_HEADER_MOVE_LEFT_UP,
    move_right_down: NATIVE_HEADER_MOVE_RIGHT_DOWN,
    undo_order: NATIVE_HEADER_UNDO_ORDER,
    redo_order: NATIVE_HEADER_REDO_ORDER,
    add_tools: WORKSPACE_ADD_TOOLS_MENU,
    show_footer: WORKSPACE_HEADER_SHOW_FOOTER,
    open: NATIVE_HEADER_OPEN,
    closed: NATIVE_HEADER_CLOSED,
    ready: NATIVE_HEADER_READY,
    pressed: NATIVE_HEADER_PRESSED,
    dragging: NATIVE_HEADER_DRAGGING,
} }

copy_struct! { ColorCopy {
    reset_balance_contrast: COLOR_FEATURES_EXPORT_RESET_NAMED => COLOR_FEATURES_PROOF_BALANCE_CONTRAST,
    reset_brightness: COLOR_FEATURES_EXPORT_RESET_NAMED => COLOR_FEATURES_PROOF_BRIGHTNESS,
    reset_highlight_color: COLOR_FEATURES_EXPORT_RESET_NAMED => COLOR_FEATURES_PROOF_HIGHLIGHT_COLOR,
    highlight_clipped_colors: NATIVE_HIGHLIGHT_CLIPPED_COLORS,
    screen_details: NATIVE_SCREEN_DETAILS,
    manage_profiles: COLOR_FEATURES_PROFILE_MANAGE,
    effects: NATIVE_COLOR_EFFECTS,
    category: NATIVE_COLOR_CATEGORY,
    color: WORKSPACE_PANEL_COLOR,
    opacity: RESOURCES_OPACITY,
    position: TOOL_CONTROL_GROUP_POSITION,
    gradient: COMMAND_GRADIENT,
    add_stop: NATIVE_COLOR_ADD_STOP,
    reset_gradient: NATIVE_COLOR_RESET_GRADIENT,
    use_selected: NATIVE_COLOR_USE_SELECTED,
    invalid: NATIVE_COLOR_INVALID,
    read_failed: NATIVE_COLOR_READ_FAILED,
    wheel_unavailable: NATIVE_COLOR_WHEEL_UNAVAILABLE,
    wheel_failed: NATIVE_COLOR_WHEEL_FAILED,
    inspection_preparing: NATIVE_COLOR_INSPECTION_PREPARING,
    inspection_updating: NATIVE_COLOR_INSPECTION_UPDATING,
    inspection_current: NATIVE_COLOR_INSPECTION_CURRENT,
    inspection_refresh: NATIVE_COLOR_INSPECTION_REFRESH,
    canvas_unavailable: NATIVE_COLOR_CANVAS_UNAVAILABLE,
    inspection_failed: NATIVE_COLOR_INSPECTION_FAILED,
    inspection_help: NATIVE_COLOR_INSPECTION_HELP,
    inspection_hdr_help: NATIVE_COLOR_INSPECTION_HDR_HELP,

    brush_size: WORKSPACE_CONTROL_BRUSH_SIZE,
    luminance: NATIVE_COLOR_LUMINANCE,
    red: SETTINGS_RED,
    green: SETTINGS_GREEN,
    blue: SETTINGS_BLUE,
    canvas_actions: NATIVE_COLOR_CANVAS_ACTIONS,
    controls: NATIVE_COLOR_CONTROLS,
    picker: NATIVE_COLOR_PICKER,
    wheel: WORKSPACE_CONTROL_COLOR_WHEEL,
    intensity: NATIVE_COLOR_INTENSITY,
    intensity_ev: NATIVE_COLOR_INTENSITY_EV,
    sdr_white: NATIVE_COLOR_SDR_WHITE,
    outside_srgb: NATIVE_COLOR_OUTSIDE_SRGB,
    outside_p3: NATIVE_COLOR_OUTSIDE_P3,
    edit: NATIVE_COLOR_EDIT,
    edit_menu: NATIVE_COLOR_EDIT_MENU,
    base: NATIVE_COLOR_BASE,
    adjusted: NATIVE_COLOR_ADJUSTED,
    model: NATIVE_COLOR_MODEL,
    use_color: NATIVE_COLOR_USE_COLOR,
    palettes: NATIVE_COLOR_PALETTES,
    swap: COMMANDS_SWAP_FOREGROUND_AND_BACKGROUND,
    paint_white: NATIVE_COLOR_PAINT_WHITE,
    paint_black: NATIVE_COLOR_PAINT_BLACK,
    shape: NATIVE_COLOR_SHAPE,
    switch_readout: NATIVE_COLOR_SWITCH_READOUT,
    circle: NATIVE_COLOR_CIRCLE,
    triangle: NATIVE_COLOR_TRIANGLE,
    square: NATIVE_COLOR_SQUARE,
    remove_stop: NATIVE_COLOR_REMOVE_STOP,
    reset_curve: NATIVE_COLOR_RESET_CURVE,
    curve_help: NATIVE_COLOR_CURVE_HELP,
    input: RESOURCES_SECTION_LEVELS_INPUT,
    output: RESOURCES_SECTION_LEVELS_OUTPUT,
    navigator: WORKSPACE_PANEL_NAVIGATOR,
    navigator_panel: COMMANDS_SHOW_NAVIGATOR,
    navigator_overview: NATIVE_COLOR_NAVIGATOR_OVERVIEW,
    record_tablet: NATIVE_COLOR_RECORD_TABLET,
    histogram: NATIVE_COLOR_HISTOGRAM,
    auto_update: NATIVE_COLOR_AUTO_UPDATE,
    channel: NATIVE_COLOR_CHANNEL,
    details: NATIVE_COLOR_DETAILS,
    distribution: NATIVE_COLOR_DISTRIBUTION,
    log_counts: NATIVE_COLOR_LOG_COUNTS,
    log_scale: NATIVE_COLOR_LOG_SCALE,
    refresh: NATIVE_COLOR_REFRESH,
    picking: NATIVE_COLOR_PICKING,
} }

copy_struct! { ShortcutCopy {
    clear_search: SETTINGS_CLEAR_SEARCH,
    keymap: NATIVE_SHORTCUTS_KEYMAP,
    no_differences: NATIVE_SHORTCUTS_NO_DIFFERENCES,
    defaults_help: NATIVE_SHORTCUTS_DEFAULTS_HELP,
    added: NATIVE_SHORTCUTS_ADDED,
    changed: NATIVE_SHORTCUTS_CHANGED,
    removed: NATIVE_SHORTCUTS_REMOVED,
    not_available: NATIVE_SHORTCUTS_NOT_AVAILABLE,
    no_changes: NATIVE_SHORTCUTS_NO_CHANGES,
    unassigned: NATIVE_SHORTCUTS_UNASSIGNED,
    modified: NATIVE_SHORTCUTS_MODIFIED,
    replace: NATIVE_SHORTCUTS_REPLACE,

    title: SETTINGS_PAGE_SHORTCUTS,
    search_settings: NATIVE_SHORTCUTS_SEARCH_SETTINGS,
    search_preferences: SETTINGS_SEARCH,
    search_shortcuts: NATIVE_SHORTCUTS_SEARCH_SHORTCUTS,
    search_or_press: NATIVE_SHORTCUTS_SEARCH_OR_PRESS,
    search_actions: NATIVE_SHORTCUTS_SEARCH_ACTIONS,
    no_matching_settings: NATIVE_SHORTCUTS_NO_MATCHING_SETTINGS,
    no_matching_preferences: NATIVE_SHORTCUTS_NO_MATCHING_PREFERENCES,
    settings: WORKSPACE_HEADER_SETTINGS,
    preferences: SETTINGS_TITLE,
    keymap_options: NATIVE_SHORTCUTS_KEYMAP_OPTIONS,
    keymap_preset: NATIVE_SHORTCUTS_KEYMAP_PRESET,
    preset: DOCUMENTS_PRESET_LABEL,
    differences: NATIVE_SHORTCUTS_DIFFERENCES,
    import_menu: NATIVE_SHORTCUTS_IMPORT_MENU,
    export_menu: NATIVE_SHORTCUTS_EXPORT_MENU,
    actions: NATIVE_SHORTCUTS_ACTIONS,
    choose_actions: NATIVE_SHORTCUTS_CHOOSE_ACTIONS,
    tool_kind: NATIVE_SHORTCUTS_TOOL_KIND,
    tool_shortcuts: NATIVE_SHORTCUTS_TOOL_SHORTCUTS,
    add_modifier: NATIVE_SHORTCUTS_ADD_MODIFIER,
    new_modifier: NATIVE_SHORTCUTS_NEW_MODIFIER,
    remove_modifier: NATIVE_SHORTCUTS_REMOVE_MODIFIER,
    hold_key_help: NATIVE_SHORTCUTS_HOLD_KEY_HELP,
    press_hold_key: NATIVE_SHORTCUTS_PRESS_HOLD_KEY,
    press_shortcut: NATIVE_SHORTCUTS_PRESS_SHORTCUT,
    same_all_tools: NATIVE_SHORTCUTS_SAME_ALL_TOOLS,
    reset_default: SETTINGS_RESET_TO_DEFAULT,
    reset_all: NATIVE_SHORTCUTS_RESET_ALL,
    set_shortcut: NATIVE_SHORTCUTS_SET_SHORTCUT,
    remove_shortcut: NATIVE_SHORTCUTS_REMOVE_SHORTCUT,
    add_shortcut: NATIVE_SHORTCUTS_ADD_SHORTCUT,
    open: NATIVE_SHORTCUTS_OPEN,
    add: NATIVE_SHORTCUTS_ADD,
    reassign: NATIVE_SHORTCUTS_REASSIGN,
    pen_touch: NATIVE_SHORTCUTS_PEN_TOUCH,
    pen_page_help: NATIVE_SHORTCUTS_PEN_PAGE_HELP,
    no_results: SHORTCUT_SEARCH_EMPTY_TITLE,
    search_help: SHORTCUT_SEARCH_EMPTY_HELP,
    pen_action_help: SHORTCUT_PEN_ACTION_HELP,
    updated: NATIVE_SHORTCUTS_UPDATED,
} }

copy_struct! { ToolControlCopy {
    selection_mode: TOOL_ACTION_GROUP_SELECTION_MODE,
    range_hint: TOOLBAR_RANGE_IN_STOPS_RELATIVE_TO_REFERENCE_WHITE_0,
    selection_menu: MENU_SELECT,
    more_options: WORKSPACE_TOOLBAR_MORE_OPTIONS,
    bookmark_value: WORKSPACE_TOOLBAR_BOOKMARK_VALUE,
    remove_bookmark: WORKSPACE_TOOLBAR_REMOVE_BOOKMARK,
} }

#[derive(Clone, Debug, Serialize)]
pub struct SamplerCopy {
    pub source: Arc<str>,
    pub visible_color: Arc<str>,
    pub selected_layer: Arc<str>,
    pub sample_size: Arc<str>,
    pub sizes: [(u32, Arc<str>); 5],
}
impl SamplerCopy {
    fn new(l: &Localizer) -> Self {
        let labels = [MessageId::TOOLBAR_SINGLE_PIXEL, MessageId::TOOLBAR_5_PX_CIRCLE, MessageId::TOOLBAR_15_PX_CIRCLE, MessageId::TOOLBAR_51_PX_CIRCLE, MessageId::TOOLBAR_101_PX_CIRCLE];
        Self {
            source: l.text(MessageId::TOOLBAR_SOURCE),
            visible_color: l.text(MessageId::TOOLBAR_VISIBLE_COLOR),
            selected_layer: l.text(MessageId::TOOLBAR_SELECTED_LAYER),
            sample_size: l.text(MessageId::TOOLBAR_SAMPLE_SIZE),
            sizes: std::array::from_fn(|i| (crate::COLOR_SAMPLE_WIDTHS[i], l.text(labels[i]))),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct NativeCopy {
    pub tool_controls: ToolControlCopy,
    pub sampler: SamplerCopy,
    pub search: SearchCopy,
    pub layers: LayerCopy,
    pub palettes: PaletteCopy,
    pub header: HeaderCopy,
    pub color: ColorCopy,
    pub shortcuts: ShortcutCopy,
}
impl NativeCopy {
    pub fn new(l: &Localizer) -> Self {
        Self {
            tool_controls: ToolControlCopy::new(l),
            sampler: SamplerCopy::new(l),
            search: SearchCopy::new(l),
            layers: LayerCopy::new(l),
            palettes: PaletteCopy::new(l),
            header: HeaderCopy::new(l),
            color: ColorCopy::new(l),
            shortcuts: ShortcutCopy::new(l),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NativeCaption {
    NumericError { reason: crate::NumericError },
    HeaderDrag { item: String },
    DrawingStorage { detail: String },
    InspectionRange { start: f64, end: f64 },
    InspectionPixels { sampled: u64, transparent: u64 },
    InspectionChannel { below: u64, above: u64, black: u64, white: u64 },
    InspectionGraph { channel: String },
    InspectionClipped { count: u64 },
    ShortcutDefaults { keys: Vec<String> },
    ModifierHold { label: String },
    SwitchWorkspace { title: String },
    Reorder { title: String },
    ColorShape { shape: crate::ColorShape },
    InspectionSample { seconds: f64 },
    InspectionChanged { status: String },
    LayerRow { title: String },
    CloseDrawing { title: String },
    SwitchDrawing { title: String },
    DrawingTitle { title: String, width: u32, height: u32 },
    OptionsFor { title: String },
    MovePanel { title: String },
    ChoosePalette { name: String },
    RemovePalette { name: String },
    ColorIntensity { value: f64 },
}
impl NativeCaption {
    pub fn message(&self, l: &Localizer) -> String {
        let mut args = FluentArgs::new();
        let id = match self {
            Self::NumericError { reason } => return reason.message(l),
            Self::HeaderDrag { item } => return crate::header_drag_label(item, l),
            Self::DrawingStorage { detail } => { args.set("detail", detail.as_str()); MessageId::NATIVE_DRAWING_STORAGE_HELP },
            Self::InspectionRange { start, end } => { args.set("start", start.to_string()); args.set("end", end.to_string()); MessageId::NATIVE_INSPECTION_RANGE },
            Self::InspectionPixels { sampled, transparent } => { args.set("sampled", *sampled); args.set("transparent", *transparent); MessageId::NATIVE_INSPECTION_PIXELS },
            Self::InspectionChannel { below, above, black, white } => { args.set("below", *below); args.set("above", *above); args.set("black", *black); args.set("white", *white); MessageId::NATIVE_INSPECTION_CHANNEL },
            Self::InspectionGraph { channel } => { args.set("channel", channel.as_str()); MessageId::NATIVE_INSPECTION_GRAPH },
            Self::InspectionClipped { count } => { args.set("count", *count); MessageId::NATIVE_INSPECTION_CLIPPED },
            Self::ShortcutDefaults { keys } => return crate::shortcut_default_caption(l, keys),
            Self::ModifierHold { label } => return crate::modifier_hold_help(l, label),
            Self::SwitchWorkspace { title } => { args.set("title", title.as_str()); MessageId::NATIVE_SWITCH_WORKSPACE },
            Self::Reorder { title } => { args.set("title", title.as_str()); MessageId::NATIVE_REORDER },
            Self::ColorShape { shape } => return l.text(match shape {
                crate::ColorShape::Circle => MessageId::NATIVE_COLOR_USE_CIRCLE,
                crate::ColorShape::Triangle => MessageId::NATIVE_COLOR_USE_TRIANGLE,
                crate::ColorShape::Square => MessageId::NATIVE_COLOR_USE_SQUARE,
            }).to_string(),
            Self::InspectionSample { seconds } => { args.set("seconds", format!("{seconds:.2}")); MessageId::NATIVE_INSPECTION_SAMPLE },
            Self::InspectionChanged { status } => { args.set("status", status.as_str()); MessageId::NATIVE_INSPECTION_CHANGED },
            Self::LayerRow { title } => { args.set("title", title.as_str()); MessageId::NATIVE_LAYER_ROW },
            Self::CloseDrawing { title } => { args.set("title", title.as_str()); MessageId::NATIVE_CLOSE_DRAWING },
            Self::SwitchDrawing { title } => { args.set("title", title.as_str()); MessageId::NATIVE_SWITCH_DRAWING },
            Self::DrawingTitle { title, width, height } => {
                args.set("title", title.as_str()); args.set("width", width.to_string()); args.set("height", height.to_string());
                MessageId::NATIVE_DRAWING_TITLE
            },
            Self::OptionsFor { title } => { args.set("title", title.as_str()); MessageId::NATIVE_OPTIONS_FOR },
            Self::MovePanel { title } => { args.set("title", title.as_str()); MessageId::NATIVE_MOVE_PANEL },
            Self::ChoosePalette { name } => { args.set("name", name.as_str()); MessageId::NATIVE_CHOOSE_PALETTE },
            Self::RemovePalette { name } => { args.set("name", name.as_str()); MessageId::NATIVE_REMOVE_PALETTE },
            Self::ColorIntensity { value } => { args.set("value", format!("{value:+.2}")); MessageId::NATIVE_COLOR_INTENSITY_VALUE },
        };
        l.format(id, &args)
    }
}

copy_struct! { RecoveryCopy {
    preferences_failed: NATIVE_DIALOG_PREFERENCES_FAILED,
    open: DOCUMENTS_OPEN_ACCEPT,
    import: DOCUMENTS_IMPORT_ACCEPT,
    rename: WORKSPACE_ACTION_RENAME,
    retry: WORKSPACE_ACTION_RETRY_STORAGE,
    attention: NATIVE_DIALOG_ATTENTION,
    title: NATIVE_DIALOG_TITLE,
    restore: NATIVE_DIALOG_RESTORE,
    discard: NATIVE_DIALOG_DISCARD,
    later: NATIVE_DIALOG_LATER,
    restoring: NATIVE_DIALOG_RESTORING,
    explanation: NATIVE_DIALOG_EXPLANATION,
    loading: NATIVE_DIALOG_LOADING,
    saving: NATIVE_DIALOG_SAVING,
    workspace_missing: NATIVE_DIALOG_WORKSPACE_MISSING,
    workspace_activation_failed: NATIVE_DIALOG_WORKSPACE_ACTIVATION_FAILED,
    workspace_manager_failed: NATIVE_DIALOG_WORKSPACE_MANAGER_FAILED,
    document_dialog_failed: NATIVE_DIALOG_DOCUMENT_DIALOG_FAILED,
    unsaved_dialog_failed: NATIVE_DIALOG_UNSAVED_DIALOG_FAILED,
    unsupported_operation: NATIVE_DIALOG_UNSUPPORTED_OPERATION,
    link_failed: NATIVE_DIALOG_LINK_FAILED,
    window_failed: NATIVE_DIALOG_WINDOW_FAILED,
    fullscreen_failed: NATIVE_DIALOG_FULLSCREEN_FAILED,
    link_unavailable: NATIVE_DIALOG_LINK_UNAVAILABLE,
    drop_drawings: NATIVE_DIALOG_DROP_DRAWINGS,
} }

#[cfg(test)]
mod sampler_tests {
    use super::*;
    #[test]
    fn cjk_tool_controls_retain_cached_semantic_captions() {
        let english = ToolControlCopy::new(&Localizer::shared(crate::UiLanguage::English));
        for language in [crate::UiLanguage::Japanese, crate::UiLanguage::Korean] {
            let localizer = Localizer::shared(language);
            let copy = NativeCopy::new(&localizer).tool_controls;
            for (label, canonical, id) in [
                (&copy.selection_mode, &english.selection_mode, MessageId::TOOL_ACTION_GROUP_SELECTION_MODE),
                (&copy.range_hint, &english.range_hint, MessageId::TOOLBAR_RANGE_IN_STOPS_RELATIVE_TO_REFERENCE_WHITE_0),
                (&copy.selection_menu, &english.selection_menu, MessageId::MENU_SELECT),
            ] {
                assert_ne!(label, canonical);
                assert!(!label.is_ascii());
                assert!(Arc::ptr_eq(label, &localizer.text(id)));
            }
            let json = serde_json::to_value(&copy).unwrap();
            assert_eq!(json["selection_menu"], copy.selection_menu.as_ref());
            assert_eq!(json["range_hint"], copy.range_hint.as_ref());
        }
    }
    #[test]
    fn cjk_sampler_copy_preserves_sample_values_and_cached_active_captions() {
        let en = Localizer::shared(crate::UiLanguage::English);
        let english = SamplerCopy::new(&en);
        for language in [crate::UiLanguage::Japanese, crate::UiLanguage::Korean] {
            let localizer = Localizer::shared(language);
            let copy = NativeCopy::new(&localizer).sampler;
            assert_ne!(copy.source, english.source);
            assert!(!copy.source.is_ascii());
            assert!(Arc::ptr_eq(&copy.source, &localizer.text(MessageId::TOOLBAR_SOURCE)));
            assert_eq!(copy.visible_color, localizer.text(MessageId::TOOLBAR_VISIBLE_COLOR));
            assert_eq!(copy.selected_layer, localizer.text(MessageId::TOOLBAR_SELECTED_LAYER));
            assert_eq!(copy.sample_size, localizer.text(MessageId::TOOLBAR_SAMPLE_SIZE));
            assert_eq!(copy.sizes.each_ref().map(|(width, _)| *width), crate::COLOR_SAMPLE_WIDTHS);
            assert_eq!(copy.sizes[0].1, localizer.text(MessageId::TOOLBAR_SINGLE_PIXEL));
            assert_eq!(copy.sizes[4].1, localizer.text(MessageId::TOOLBAR_101_PX_CIRCLE));
            let serialized = serde_json::to_value(&copy).unwrap();
            for (index, (width, label)) in copy.sizes.iter().enumerate() {
                assert_eq!(serialized["sizes"][index][0], *width);
                assert_eq!(serialized["sizes"][index][1], label.as_ref());
                assert!(Arc::ptr_eq(label, &localizer.text([MessageId::TOOLBAR_SINGLE_PIXEL, MessageId::TOOLBAR_5_PX_CIRCLE, MessageId::TOOLBAR_15_PX_CIRCLE, MessageId::TOOLBAR_51_PX_CIRCLE, MessageId::TOOLBAR_101_PX_CIRCLE][index])));
            }
        }
    }
}
#[cfg(test)]
mod numeric_caption_tests {
    use super::*;
    use crate::{NumericError, UiLanguage};
    use layer_core::ResourceLabel;

    #[test]
    fn inspection_counts_select_numeric_plural_branches_and_preserve_exact_integers() {
        let english = Localizer::shared(UiLanguage::English);
        for count in [0, 1, 2, 5, 11, 21, 22, 1_000_003, (1_u64 << 53) - 1] {
            let text = NativeCaption::InspectionPixels { sampled:count, transparent:count }.message(&english);
            let sampled = if count == 1 { "sampled pixel" } else { "sampled pixels" };
            let transparent = if count == 1 { "transparent pixel excluded" } else { "transparent pixels excluded" };
            assert_eq!(text, format!("{count} {sampled} · {count} {transparent}"));
            for language in UiLanguage::ALL {
                let localizer = Localizer::shared(language);
                for text in [
                    NativeCaption::InspectionPixels {sampled:count, transparent:count}.message(&localizer),
                    NativeCaption::InspectionChannel {below:count, above:count, black:count, white:count}.message(&localizer),
                    NativeCaption::InspectionClipped {count}.message(&localizer),
                ] {
                    assert!(text.split(|c:char| !c.is_ascii_digit()).any(|number| number == count.to_string()), "{}: {text}", language.tag());
                }
            }
        }
    }

    #[test]
    fn retained_native_numeric_errors_format_current_copy_and_preserve_literal_arguments() {
        let literal = "İı ไทย Tie\u{302}\u{301}ng Vie\u{323}\u{302}t { $label }\n{\"type\":\"numeric_error\"} 🎨";
        let label: ResourceLabel = ResourceLabel::Literal(literal.into());
        let leaves = [
            (NumericError::InvalidDefinition, MessageId::NUMERIC_INVALID_DEFINITION),
            (NumericError::PositiveLogarithmicBounds, MessageId::NUMERIC_POSITIVE_LOGARITHMIC_BOUNDS),
            (NumericError::InvalidRangeExponent, MessageId::NUMERIC_INVALID_RANGE_EXPONENT),
            (NumericError::InvalidMappedRange, MessageId::NUMERIC_INVALID_MAPPED_RANGE),
            (NumericError::FiniteNumber, MessageId::NUMERIC_FINITE_NUMBER),
            (NumericError::InvalidStep, MessageId::NUMERIC_INVALID_STEP),
            (NumericError::InvalidPosition, MessageId::NUMERIC_INVALID_POSITION),
            (NumericError::ExpressionRequired, MessageId::NUMERIC_EXPRESSION_REQUIRED),
            (NumericError::ExpressionTooLong, MessageId::NUMERIC_EXPRESSION_TOO_LONG),
            (NumericError::InvalidExpression, MessageId::NUMERIC_INVALID_EXPRESSION),
            (NumericError::InvalidNumber, MessageId::SETTINGS_EXPECTED_A_NUMBER),
        ];
        let reasons = leaves.iter().map(|(reason, _)| reason.clone()).chain([
            NumericError::Range { label:label.clone(), min:-1.25, max:32768. },
            NumericError::WholePixels { label:label.clone() },
            NumericError::Range { label:MessageId::COLOR_FEATURES_EXPORT_MAXIMUM_WIDTH.into(), min:1., max:32768. },
        ]).collect::<Vec<_>>();
        for reason in reasons {
            assert!(reason.valid());
            let serialized = serde_json::to_value(NativeCaption::NumericError { reason:reason.clone() }).unwrap();
            assert_eq!(serialized["type"], "numeric_error");
            assert_eq!(serialized["reason"], serde_json::to_value(&reason).unwrap());
            let caption: NativeCaption = serde_json::from_value(serialized.clone()).unwrap();
            for language in UiLanguage::ALL {
                let localizer = Localizer::shared(language);
                let message = caption.message(&localizer);
                assert_eq!(message, reason.message(&localizer));
                if let Some((_, id)) = leaves.iter().find(|(leaf, _)| leaf == &reason) { assert_eq!(message, localizer.text(*id).as_ref()); }
                match &reason {
                    NumericError::Range { label:ResourceLabel::Literal(_), .. } | NumericError::WholePixels { label:ResourceLabel::Literal(_) } => assert!(message.contains(literal)),
                    NumericError::Range { label:ResourceLabel::Message { .. }, .. } => assert!(message.contains(localizer.text(MessageId::COLOR_FEATURES_EXPORT_MAXIMUM_WIDTH).as_ref())),
                    _ => {},
                }
                assert_eq!(serde_json::to_value(&caption).unwrap(), serialized);
            }
        }
        let invalid = [
            NumericError::Range { label:label.clone(), min:2., max:1. },
            NumericError::Range { label:label.clone(), min:f64::NAN, max:1. },
            NumericError::Range { label:label.clone(), min:0., max:f64::INFINITY },
            NumericError::WholePixels { label:ResourceLabel::Message { message:"unknown-host-message".into() } },
        ];
        for reason in invalid {
            assert!(!reason.valid());
            let caption = NativeCaption::NumericError { reason };
            for language in UiLanguage::ALL {
                let localizer = Localizer::shared(language);
                assert_eq!(caption.message(&localizer), localizer.text(MessageId::NUMERIC_INVALID_DEFINITION).as_ref());
            }
        }
        for malformed in [
            serde_json::json!({"type":"numeric_error","reason":{"reason":"unknown"}}),
            serde_json::json!({"type":"numeric_error","reason":{"reason":"range","label":"literal","min":0,"max":1,"error":"literal"}}),
            serde_json::json!({"type":"numeric_error","reason":{"reason":"range","label":"literal","min":null,"max":1}}),
        ] { assert!(serde_json::from_value::<NativeCaption>(malformed).is_err()); }
    }
}
