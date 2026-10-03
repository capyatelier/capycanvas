//! Portable preferences, editor state and host requests. Hosts render this model
//! and perform storage/window services; they do not decide settings policy.
use crate::*;
use crate::localization::{LanguagePreference, Localizer, MessageId, UiLanguage, SHIPPED_LANGUAGES};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[path = "settings_color.rs"]
mod color;
pub use color::{MissingProfilePolicy, PhotoOpenPolicy};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Gtk,
    Web,
    Windows,
    Mac,
    Ios,
    Android,
}
impl Platform {
    pub const ALL: [Self; 6] = [Self::Gtk, Self::Web, Self::Windows, Self::Mac, Self::Ios, Self::Android];
    pub fn apple(self) -> bool {
        matches!(self, Self::Mac | Self::Ios)
    }
    pub fn native_windows(self) -> bool {
        // The iOS host is an iPad app with independent native editor scenes.
        matches!(self, Self::Gtk | Self::Windows | Self::Mac | Self::Ios)
    }
    /// Hosts that write pixels to the system clipboard and read their own
    /// copies back at full depth.
    pub fn pixel_clipboard(self) -> bool {
        matches!(self, Self::Gtk | Self::Web | Self::Android)
    }
    pub fn touch_gestures(self) -> bool {
        matches!(self, Self::Gtk | Self::Web | Self::Android | Self::Ios)
    }
    pub fn pen_buttons(self) -> bool {
        matches!(self, Self::Gtk | Self::Web | Self::Android | Self::Mac)
    }
    pub fn system_accent(self) -> bool {
        matches!(self, Self::Gtk | Self::Android | Self::Windows | Self::Mac)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZenIcon {
    #[default]
    LookingUp,
    FacingForward,
    Bathing,
    Sleeping,
}
impl ZenIcon {
    pub const CHOICES: [(Self, &'static str); 4] = [
        (Self::LookingUp, "Looking up"),
        (Self::FacingForward, "Facing forward"),
        (Self::Bathing, "Bathing"),
        (Self::Sleeping, "Sleeping"),
    ];
    pub const fn icon(self) -> &'static str {
        match self {
            Self::LookingUp => "zen-looking-up",
            Self::FacingForward => "zen-facing-forward",
            Self::Bathing => "zen-bathing",
            Self::Sleeping => "zen-sleeping",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub version: u32,
    pub new_document: NewDocumentSettings,
    pub photo_open: PhotoOpenPolicy,
    pub language: LanguagePreference,
    pub theme: Option<Theme>,
    pub transparency: crate::Transparency,
    pub dark_base: HexColor,
    pub light_base: HexColor,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent: Option<HexColor>,
    pub zen_icon: ZenIcon,
    pub zen_show_capy: bool,
    pub zen_reveal_at_edges: bool,
    /// Shared by Quick Mask and every saved selection, across documents.
    pub selection_painting: layer_core::SelectionPaintBehavior,
    pub pressure_gamma: f32,
    pub cursor: CursorMode,
    pub hide_cursor_while_drawing: bool,
    pub pan_speed: f32,
    pub zoom_speed: f32,
    /// New groups blend Pass Through instead of Normal.
    pub pass_through_groups: bool,
    pub feedback: bool,
    pub platform_prediction: bool,
    pub prediction_ms: f32,
    /// Only overrides are stored. Empty keys disable an action's shortcut.
    pub shortcuts: BTreeMap<String, Vec<KeyChord>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub gestures: BTreeMap<String, String>,
    /// What each pen button does per kind of tool, when set that way.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub pen_buttons: BTreeMap<String, BTreeMap<ToolCategory, String>>,
    /// The artist's modifier keys; `None` follows the keymap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hold_keys: Option<Vec<crate::shortcuts::HoldKey>>,
    #[serde(skip_serializing_if = "EraserEnd::is_default")]
    pub eraser_end: EraserEnd,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keymap: Option<crate::keymaps::KeymapRef>,
    /// Per-preset slider values, shared by every placement of that slider.
    pub slider_bookmarks: BTreeMap<String, SliderBookmarks>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            new_document: NewDocumentSettings::default(),
            photo_open: PhotoOpenPolicy::default(),
            language: LanguagePreference::System,
            theme: None,
            transparency: crate::Transparency::default(),
            dark_base: Theme::Dark.default_base(),
            light_base: Theme::Light.default_base(),
            accent: None,
            zen_icon: ZenIcon::default(),
            zen_show_capy: true,
            zen_reveal_at_edges: false,
            selection_painting: Default::default(),
            pressure_gamma: 1.0,
            cursor: CursorMode::default(),
            hide_cursor_while_drawing: true,
            pan_speed: 1.0,
            zoom_speed: 1.0,
            pass_through_groups: false,
            feedback: true,
            platform_prediction: true,
            prediction_ms: 16.0,
            shortcuts: BTreeMap::new(),
            gestures: BTreeMap::new(),
            hold_keys: None,
            pen_buttons: BTreeMap::new(),
            eraser_end: EraserEnd::default(),
            keymap: None,
            slider_bookmarks: BTreeMap::new(),
        }
    }
}
/// What the pen's eraser end does, beside the keyboard and button bindings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EraserEnd {
    /// `None` keeps the current tool.
    pub tool: Option<CommandId>,
    /// Paint with transparency, erasing with the tool's brush.
    pub erase: bool,
}
impl Default for EraserEnd {
    fn default() -> Self {
        Self { tool: None, erase: true }
    }
}
impl EraserEnd {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    pub fn erases(&self) -> bool {
        self.erase || self.tool == Some(CommandId::Eraser)
    }
}
impl Settings {
    pub fn language_preference(saved: &str) -> crate::LanguagePreference {
        let value = serde_json::from_str::<serde_json::Value>(saved).ok();
        let preference = value.as_ref().and_then(|value| value.as_object())
            .and_then(|object| object.get("language"))
            .and_then(|value| serde_json::from_value::<crate::LanguagePreference>(value.clone()).ok())
            .unwrap_or_default();
        match preference {
            crate::LanguagePreference::Explicit(language) if !crate::localization::SHIPPED_LANGUAGES.contains(&language) => crate::LanguagePreference::System,
            _ => preference,
        }
    }
    /// Read settings saved by any build. Each field this build cannot read or
    /// validate keeps its default; the next save replaces the saved copy.
    pub fn restore(saved: &str) -> Self {
        Self::restore_localized(saved, &Localizer::shared(UiLanguage::English))
    }
    pub fn restore_localized(saved: &str, localization: &Localizer) -> Self {
        let valid = |value: &serde_json::Value| {
            serde_json::from_value::<Self>(value.clone())
                .ok()
                .filter(|settings| settings.validate_localized(localization).is_ok())
        };
        let mut kept = serde_json::Value::Object(Default::default());
        if let Ok(serde_json::Value::Object(saved)) = serde_json::from_str(saved) {
            if let Some(settings) = valid(&serde_json::Value::Object(saved.clone())) {
                return settings;
            }
            for (key, value) in saved {
                let mut candidate = kept.clone();
                candidate[key] = value;
                if valid(&candidate).is_some() {
                    kept = candidate;
                }
            }
        }
        valid(&kept).unwrap_or_default()
    }
    #[cfg(test)]
    pub(crate) fn validate(&self) -> Result<(), String> {
        self.validate_localized(&Localizer::shared(UiLanguage::English))
    }
    pub fn validate_localized(&self, localization: &Localizer) -> Result<(), String> {
        if let LanguagePreference::Explicit(language) = self.language {
            if !SHIPPED_LANGUAGES.contains(&language) {
                return Err("This language isn't available in this build.".into());
            }
        }
        if self.eraser_end.tool.is_some_and(|t| !crate::shortcuts::ERASER_END_TOOLS.contains(&t)) {
            return Err("The eraser end can't use this tool".into());
        }
        for marks in self.slider_bookmarks.values() {
            marks.validate().map_err(|reason| reason.message(localization))?;
        }
        self.new_document.validate().map_err(|error| error.message(localization))?;
        if self.version != 1 {
            return Err("Unsupported settings version".into());
        }
        for id in [PreferenceId::Pressure, PreferenceId::PanSpeed, PreferenceId::ZoomSpeed, PreferenceId::PredictionHorizon] {
            let (control, value, label) = self.numeric_field(id).unwrap();
            control.validate(value, label).map_err(|reason| reason.message(localization))?;
        }
        self.feedback_config().validate().map_err(|e| e.to_string())?;
        self.validate_shortcuts()
    }
    pub(crate) fn feedback_config(&self) -> layer_engine::InstantFeedbackConfig {
        layer_engine::InstantFeedbackConfig {
            enabled: self.feedback,
            use_platform_prediction: self.platform_prediction,
            prediction_horizon_micros: (self.prediction_ms * 1000.0).round() as u32,
            ..Default::default()
        }
    }
    pub(crate) fn feedback_config_for(
        &self,
        platform: Platform,
        native_available: bool,
    ) -> layer_engine::InstantFeedbackConfig {
        let mut config = self.feedback_config();
        if platform == Platform::Gtk {
            // GDK current-event and history timestamps are integer milliseconds.
            config.timestamp_resolution_micros = 1_000;
        }
        config.use_platform_prediction &= native_available;
        if config.use_platform_prediction {
            // Native samples carry their own lookahead. The fallback uses
            // automatic timing, never the disabled, saved prediction time.
            let defaults = layer_engine::InstantFeedbackConfig::default();
            config.prediction_horizon_micros = defaults.prediction_horizon_micros;
        }
        config
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsPage {
    #[default]
    Appearance,
    Canvas,
    Color,
    Input,
    Shortcuts,
    About,
}
impl SettingsPage {
    pub const ALL: [Self; 6] = [
        Self::Appearance,
        Self::Canvas,
        Self::Color,
        Self::Input,
        Self::Shortcuts,
        Self::About,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::Appearance => "appearance",
            Self::Canvas => "canvas",
            Self::Color => "color",
            Self::Input => "input",
            Self::Shortcuts => "shortcuts",
            Self::About => "about",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Canvas => "Canvas",
            Self::Color => "Color",
            Self::Input => "Pen & Input",
            Self::Shortcuts => "Keyboard Shortcuts",
            Self::About => "About",
        }
    }
    pub fn localized_title(self, localizer: &Localizer) -> String {
        localizer.text(match self {
            Self::Appearance => MessageId::SETTINGS_PAGE_APPEARANCE,
            Self::Canvas => MessageId::SETTINGS_PAGE_CANVAS,
            Self::Color => MessageId::SETTINGS_PAGE_COLOR,
            Self::Input => MessageId::SETTINGS_PAGE_INPUT,
            Self::Shortcuts => MessageId::SETTINGS_PAGE_SHORTCUTS,
            Self::About => MessageId::SETTINGS_PAGE_ABOUT,
        }).to_string()
    }
    pub fn icon(self) -> &'static str {
        match self {
            Self::Appearance => "appearance",
            Self::Canvas => "fit",
            Self::Color => "color",
            Self::Input => "brush",
            Self::Shortcuts => "keyboard",
            Self::About => "info",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreferenceId {
    Language,
    NewColorSpace,
    NewBitDepth,
    NewBackground,
    PhotoDepth,
    MissingProfile,
    Theme,
    Transparency,
    ZenIcon,
    ZenShowCapy,
    ZenRevealAtEdges,
    DarkBase,
    LightBase,
    Accent,
    Cursor,
    HideCursorWhileDrawing,
    PanSpeed,
    ZoomSpeed,
    PassThroughGroups,
    Pressure,
    Feedback,
    PlatformPrediction,
    PredictionHorizon,
    EraserTool,
    EraserErase,
    Version,
    License,
    Renderer,
    Website,
    SourceCode,
}
impl PreferenceId {
    pub fn key(self) -> &'static str {
        match self {
            Self::NewColorSpace => "new-color-space",
            Self::NewBitDepth => "new-bit-depth",
            Self::NewBackground => "new-background",
            Self::PhotoDepth => "photo-depth",
            Self::MissingProfile => "missing-profile",
            Self::Language => "language",
            Self::Theme => "theme",
            Self::Transparency => "transparency",
            Self::ZenIcon => "zen-icon",
            Self::ZenShowCapy => "zen-show-capy",
            Self::ZenRevealAtEdges => "zen-reveal-at-edges",
            Self::DarkBase => "dark-base",
            Self::LightBase => "light-base",
            Self::Accent => "accent",
            Self::Cursor => "cursor",
            Self::HideCursorWhileDrawing => "hide-cursor-while-drawing",
            Self::PanSpeed => "pan-speed",
            Self::ZoomSpeed => "zoom-speed",
            Self::PassThroughGroups => "pass-through-groups",
            Self::Pressure => "pressure",
            Self::Feedback => "feedback",
            Self::PlatformPrediction => "platform-prediction",
            Self::PredictionHorizon => "prediction-horizon",
            Self::EraserTool => "eraser-tool",
            Self::EraserErase => "eraser-erase",
            Self::Version => "version",
            Self::License => "license",
            Self::Renderer => "renderer",
            Self::Website => "website",
            Self::SourceCode => "source-code",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PreferenceValue {
    Bool(bool),
    Number(f32),
    Choice(u32),
    /// Native numeric editors submit text when editing completes. Parsing and
    /// range validation stay in the core; incomplete IME text is not persisted.
    Text(String),
}
// Choice indices and numbers have distinct Rust types; JSON numbers are decoded
// according to the destination field below, so clients need no tagged wrappers.
impl PreferenceValue {
    fn number(&self) -> Option<f32> {
        match self {
            Self::Number(v) => Some(*v),
            Self::Choice(v) => Some(*v as f32),
            Self::Text(v) => v.trim().parse().ok(),
            _ => None,
        }
    }
    fn choice(&self) -> Option<u32> {
        self.number()
            .filter(|v| v.is_finite() && *v >= 0.0 && v.fract() == 0.0)
            .map(|v| v as u32)
    }
}
/// Presentation only; both styles share choice validation, persistence and reset.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChoicePresentation {
    Dropdown,
    ImageTiles { columns: u32 },
    Circles { alphas: [[f32; 2]; 4] },
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PreferenceKind {
    Choice {
        options: Vec<String>,
        selected: u32,
        icons: Vec<String>,
        presentation: ChoicePresentation,
    },
    Swatches {
        swatches: Vec<Swatch>,
        selected: u32,
        value: String,
        custom: String,
        placeholder: String,
        inline: bool,
    },
    Number {
        control: NumericControl,
        value: f32,
    },
    Switch {
        active: bool,
    },
    Info {
        value: String,
    },
    Link {
        label: String,
        url: String,
    },
}

impl PreferenceKind {
    fn value(&self) -> Option<PreferenceValue> {
        Some(match self {
            Self::Number { value, .. } => PreferenceValue::Number(*value),
            Self::Choice { selected, .. } => PreferenceValue::Choice(*selected),
            Self::Swatches { value, .. } => PreferenceValue::Text(value.clone()),
            Self::Switch { active } => PreferenceValue::Bool(*active),
            Self::Info { .. } | Self::Link { .. } => return None,
        })
    }

    fn display_value(&self, localizer: &Localizer) -> String {
        match self {
            Self::Number { value, control } => {
                control
                    .resolve(*value as f64, NumericOperation::Format)
                    .expect("valid setting default")
                    .text
            }
            Self::Choice {
                options, selected, ..
            } => options[*selected as usize].clone(),
            Self::Swatches {
                swatches,
                selected,
                custom,
                ..
            } => match &swatches[*selected as usize] {
                swatch if swatch.custom => custom.clone(),
                swatch => swatch.label.clone(),
            },
            Self::Switch { active } => localizer.text(if *active { MessageId::SETTINGS_ON } else { MessageId::SETTINGS_OFF }).to_string(),
            Self::Info { .. } | Self::Link { .. } => String::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Swatch {
    pub label: String,
    pub value: String,
    pub color: Option<HexColor>,
    pub foreground: Option<HexColor>,
    pub icon: Option<String>,
    pub custom: bool,
}
impl Swatch {
    fn new(label: &str, value: String, color: Option<HexColor>, icon: Option<&str>) -> Self {
        Self {
            label: label.into(),
            value,
            color,
            foreground: color.map(HexColor::contrasting),
            icon: icon.map(Into::into),
            custom: false,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct PreferenceReset {
    pub label: String,
    pub value: String,
    pub hint: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct PreferenceRow {
    pub id: PreferenceId,
    pub title: String,
    pub description: String,
    pub kind: PreferenceKind,
    pub enabled: bool,
    pub visible: bool,
    pub reset: Option<PreferenceReset>,
}
#[derive(Clone, Debug, Serialize)]
pub struct PreferenceGroup {
    pub title: String,
    pub rows: Vec<PreferenceRow>,
}
#[derive(Clone, Debug, Serialize)]
pub struct PreferencePage {
    pub id: SettingsPage,
    pub title: String,
    pub icon: String,
    pub groups: Vec<PreferenceGroup>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PreferencesState {
    pub page: SettingsPage,
    pub reveal: Option<PreferenceId>,
    pub query: String,
    pub searching: bool,
    pub search_focus: u64,
    pub shortcut_query: String,
    pub editing_shortcut: Option<String>,
    pub capture: Option<ShortcutCapture>,
    pub error: Option<String>,
    #[serde(skip)]
    error_source: Option<PreferenceErrorSource>,
    #[serde(skip)]
    capture_key: Option<KeyChord>,
    #[serde(skip)]
    pub(crate) keymap_import: Option<crate::keymaps::KeymapImport>,
    #[serde(skip)]
    pub(crate) keymap_details: bool,
    #[serde(skip)]
    pub(crate) shortcut_page: crate::shortcut_page::ShortcutPageState,
    /// The open modifier key and whether it lists every kind of tool.
    #[serde(skip)]
    pub(crate) modifier_editor: Option<(KeyChord, bool)>,
    /// The open pen button and whether it lists every kind of tool.
    #[serde(skip)]
    pub(crate) pen_editor: Option<(String, bool)>,
}
#[derive(Clone, Debug)]
enum PreferenceErrorSource {
    Message(MessageId),
    KeymapImport(crate::keymaps::KeymapImportError),
    Action(Box<PreferenceErrorAction>),
}
enum PreferenceEditError {
    Localized(String),
    KeymapImport(crate::keymaps::KeymapImportError),
}
impl From<String> for PreferenceEditError {
    fn from(message: String) -> Self { Self::Localized(message) }
}
impl PreferenceEditError {
    fn message(&self, localizer: &Localizer) -> String {
        match self {
            Self::Localized(message) => message.clone(),
            Self::KeymapImport(reason) => reason.message(localizer),
        }
    }
}
#[derive(Clone, Debug)]
struct PreferenceErrorAction {
    action: PreferenceAction,
    settings: Settings,
    capture: Option<ShortcutCapture>,
    editing_shortcut: Option<String>,
    shortcut_page: crate::shortcut_page::ShortcutPageState,
}
impl PreferenceErrorSource {
    fn message(&self, platform: Platform, localizer: &Localizer) -> Option<String> {
        let source = match self {
            Self::Message(id) => return Some(localizer.text(*id).to_string()),
            Self::KeymapImport(reason) => return Some(reason.message(localizer)),
            Self::Action(source) => source,
        };
        PreferencesState {
            capture: source.capture.clone(),
            editing_shortcut: source.editing_shortcut.clone(),
            shortcut_page: source.shortcut_page.clone(),
            ..Default::default()
        }.try_edit(&mut source.settings.clone(), source.action.clone(), platform, localizer).err().map(|error| error.message(localizer))
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct PreferencesView {
    pub title: String,
    pub close_label: String,
    pub search_label: String,
    pub search_placeholder: String,
    pub clear_search_label: String,
    pub no_results: String,
    pub text_edit_menu: Vec<crate::shortcuts::TextEditMenuItem>,
    pub pages: Vec<PreferencePage>,
    pub page: SettingsPage,
    /// Bring this row into view after opening or navigating preferences.
    pub reveal: Option<PreferenceId>,
    pub query: String,
    pub searching: bool,
    /// Changes when typing outside an editor should reveal and focus search.
    pub search_focus: u64,
    pub search_results: Vec<PreferenceSearchResult>,
    pub shortcut_query: String,
    pub shortcut_editor: Option<ShortcutEditor>,
    pub modifier_editor: Option<ModifierKeyEditor>,
    pub pen_button_editor: Option<crate::shortcut_page::PenButtonEditor>,
    pub shortcut_page: crate::shortcut_page::ShortcutPageView,
    pub shortcuts: Vec<ShortcutRow>,
    pub keymap: crate::keymaps::KeymapView,
    pub capture: Option<ShortcutCapture>,
    pub error: Option<String>,
    pub empty: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct PreferenceSearchResult {
    pub title: String,
    pub description: String,
    pub action: PreferenceAction,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ModifierKeyAction {
    /// `None` sets every kind of tool at once.
    pub category: Option<ToolCategory>,
    pub label: String,
    pub action: String,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ModifierKeyEditor {
    pub key: KeyChord,
    pub label: String,
    pub per_tool: bool,
    pub actions: Vec<ModifierKeyAction>,
    pub modified: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct ShortcutEditor {
    pub id: String,
    pub label: String,
    pub description: String,
    pub group: String,
    pub scope: String,
    pub source: String,
    pub overlaps: Vec<String>,
    pub bindings: Vec<String>,
    pub keys: Vec<Vec<String>>,
    pub gestures: Vec<String>,
    pub defaults: Vec<String>,
    pub modified: bool,
    pub can_add: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PreferenceAction {
    Reveal {
        id: PreferenceId,
    },
    Page {
        page: SettingsPage,
    },
    Search {
        query: String,
    },
    ToggleSearch {
        open: bool,
    },
    SearchShortcuts {
        query: String,
    },
    Edit {
        id: PreferenceId,
        value: PreferenceValue,
    },
    Reset {
        id: PreferenceId,
    },
    BeginShortcut {
        id: String,
    },
    EditShortcut {
        id: String,
    },
    CloseShortcutEditor,
    RemoveShortcut {
        id: String,
        index: usize,
    },
    ConfirmShortcut {
        replace: bool,
    },
    CancelShortcut,
    ResetShortcut {
        id: String,
    },
    ResetAllShortcuts,
    EditModifierKey {
        key: KeyChord,
    },
    CloseModifierKey,
    AddModifierKey,
    ModifierKeyPerTool {
        key: KeyChord,
        per_tool: bool,
    },
    OpenModifierPicker {
        key: KeyChord,
        category: Option<ToolCategory>,
    },
    RemoveModifierKey {
        key: KeyChord,
    },
    ResetModifierKey {
        key: KeyChord,
    },
    EditPenButton {
        trigger: String,
    },
    ClosePenButton,
    PenButtonPerTool {
        trigger: String,
        per_tool: bool,
    },
    OpenPenButtonPicker {
        trigger: String,
        category: Option<ToolCategory>,
    },
    SelectKeymap {
        id: String,
    },
    ExportKeymap,
    ChooseKeymapFile,
    ImportKeymap {
        text: String,
    },
    ConfirmKeymapImport,
    CancelKeymapImport,
    KeymapDetails {
        open: bool,
    },
    ShortcutCategory {
        id: Option<String>,
    },
    SearchShortcutKey {
        chord: KeyChord,
    },
    ShortcutContext {
        category: Option<crate::ToolCategory>,
    },
    ShortcutShow {
        show: crate::ShortcutShow,
    },
    OpenActionPicker {
        trigger: String,
    },
    SearchActionPicker {
        query: String,
    },
    ChooseAction {
        id: String,
    },
    ResetTrigger {
        trigger: String,
    },
    CloseActionPicker,
}

#[derive(Clone, Debug, Serialize)]
pub struct HostRequest {
    pub id: u32,
    pub kind: HostRequestKind,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostRequestKind {
    Drawings,
    SdrRendition,
    SoftProofSetup,
    Histogram,
    Workspace { command: crate::WorkspaceCommand },
    SetFullscreen { fullscreen: bool },
    NewWindow,
    OpenLink { link: crate::ApplicationLink },
    Document { request: crate::DocumentRequest },
    SaveSettings { settings: Box<Settings> },
    ExportKeymap { name: String, text: String },
    ImportKeymap,
}

fn cursor_label(value: CursorMode, localizer: &Localizer) -> String {
    localizer.text(match value {
        CursorMode::None => MessageId::SETTINGS_NONE,
        CursorMode::Cross => MessageId::SETTINGS_CROSS,
        CursorMode::Triangle => MessageId::SETTINGS_TRIANGLE,
        CursorMode::Dot => MessageId::SETTINGS_DOT,
        CursorMode::SinglePixelDot => MessageId::SETTINGS_SINGLE_PIXEL_DOT,
        CursorMode::Sight => MessageId::SETTINGS_SIGHT,
        CursorMode::BrushSize => MessageId::SETTINGS_BRUSH_SIZE,
        CursorMode::BrushSizeCross => MessageId::SETTINGS_BRUSH_SIZE_AND_CROSS,
        CursorMode::BrushSizeDot => MessageId::SETTINGS_BRUSH_SIZE_AND_DOT,
        CursorMode::BrushSizeSinglePixelDot => MessageId::SETTINGS_BRUSH_SIZE_AND_SINGLE_PIXEL_DOT,
    }).to_string()
}
fn zen_label(value: ZenIcon, localizer: &Localizer) -> String {
    localizer.text(match value {
        ZenIcon::LookingUp => MessageId::SETTINGS_LOOKING_UP,
        ZenIcon::FacingForward => MessageId::SETTINGS_FACING_FORWARD,
        ZenIcon::Bathing => MessageId::SETTINGS_BATHING,
        ZenIcon::Sleeping => MessageId::SETTINGS_SLEEPING,
    }).to_string()
}
fn accent_label(index: usize, localizer: &Localizer) -> String {
    localizer.text([MessageId::SETTINGS_BLUE, MessageId::SETTINGS_TEAL, MessageId::SETTINGS_GREEN, MessageId::SETTINGS_YELLOW, MessageId::SETTINGS_ORANGE, MessageId::SETTINGS_RED, MessageId::SETTINGS_PINK, MessageId::SETTINGS_PURPLE, MessageId::SETTINGS_SLATE][index]).to_string()
}
fn transparency_label(value: crate::Transparency, localizer: &Localizer) -> String {
    let index = crate::Transparency::CHOICES.iter().position(|choice| choice.0 == value).unwrap();
    localizer.text([MessageId::SETTINGS_OFF, MessageId::SETTINGS_LOW, MessageId::SETTINGS_MEDIUM, MessageId::SETTINGS_HIGH][index]).to_string()
}

fn preference_choice_search(kind: &PreferenceKind) -> String {
    match kind {
        PreferenceKind::Choice { options, .. } => options.join(" "),
        PreferenceKind::Swatches { swatches, .. } => swatches.iter().map(|swatch| swatch.label.as_str()).collect::<Vec<_>>().join(" "),
        _ => String::new(),
    }
}

fn row(id: PreferenceId, title: &str, description: &str, kind: PreferenceKind) -> PreferenceRow {
    PreferenceRow {
        id,
        title: title.into(),
        description: description.into(),
        kind,
        enabled: true,
        visible: true,
        reset: None,
    }
}
fn swatch_row(
    id: PreferenceId,
    title: &str,
    mut swatches: Vec<Swatch>,
    value: Option<HexColor>,
    custom: HexColor,
    placeholder: HexColor,
    inline: bool,
    localizer: &Localizer,
) -> PreferenceRow {
    let preset = value.and_then(|value| {
        swatches
            .iter()
            .position(|s| !s.value.is_empty() && s.color == Some(value))
    });
    swatches.push(Swatch {
        custom: true,
        ..Swatch::new(&localizer.text(MessageId::SETTINGS_CUSTOM), String::new(), value.filter(|_| preset.is_none()), Some("pencil"))
    });
    let selected = match (value, preset) {
        (None, _) => 0,
        (Some(_), Some(index)) => index,
        (Some(_), None) => swatches.len() - 1,
    };
    row(
        id,
        title,
        "",
        PreferenceKind::Swatches {
            selected: selected as u32,
            value: value.map(|c| c.to_string()).unwrap_or_default(),
            custom: custom.to_string(),
            placeholder: placeholder.to_string(),
            inline,
            swatches,
        },
    )
}
fn number(id: PreferenceId, title: &str, description: &str, settings: &Settings) -> PreferenceRow {
    let (control, value, _) = settings.numeric_field(id).unwrap();
    row(id, title, description, PreferenceKind::Number { value, control })
}
impl Settings {
    fn numeric_field(&self, id: PreferenceId) -> Option<(NumericControl, f32, MessageId)> {
        use PreferenceId::*;
        Some(match id {
            Pressure => (NumericControl::pressure(), self.pressure_gamma, MessageId::SETTINGS_PRESSURE_RESPONSE),
            PanSpeed => (NumericControl::number(0.25, 4., 0.05, 2).unit("×"), self.pan_speed, MessageId::SETTINGS_SCROLL_PAN_SPEED),
            ZoomSpeed => (NumericControl::number(0.25, 4., 0.05, 2).unit("×"), self.zoom_speed, MessageId::SETTINGS_SCROLL_ZOOM_SPEED),
            PredictionHorizon => (NumericControl { kind: NumericKind::Slider, ..NumericControl::number(0., 64., 1., 0).unit("ms") }, self.prediction_ms, MessageId::SETTINGS_PREDICTION_AMOUNT),
            _ => return None,
        })
    }
    pub(crate) fn pages(&self, platform: Platform) -> Vec<PreferencePage> {
        self.localized_pages(platform, &Localizer::shared(UiLanguage::English))
    }
    pub(crate) fn localized_pages(&self, platform: Platform, localizer: &Localizer) -> Vec<PreferencePage> {
        let mut pages = self.raw_pages(platform, localizer);
        let defaults = Self::default().raw_pages(platform, localizer);
        let rows = |pages: Vec<PreferencePage>| {
            pages
                .into_iter()
                .flat_map(|p| p.groups)
                .flat_map(|g| g.rows)
        };
        for (row, default) in pages
            .iter_mut()
            .flat_map(|p| &mut p.groups)
            .flat_map(|g| &mut g.rows)
            .zip(rows(defaults))
        {
            let Some(default_value) = default.kind.value() else {
                continue;
            };
            let value = default.kind.display_value(localizer);
            let shortcut = self.action_shortcut_localized(
                &UiAction::Preferences {
                    action: PreferenceAction::Reset { id: row.id },
                },
                platform,
                localizer,
            );
            row.reset = Some(PreferenceReset {
                label: localizer.text(MessageId::SETTINGS_RESET_TO_DEFAULT).to_string(),
                hint: if shortcut.is_empty() {
                    value.clone()
                } else {
                    let mut args = crate::localization::FluentArgs::new();
                    args.set("value", value.as_str());
                    args.set("shortcut", shortcut.as_str());
                    localizer.format(MessageId::SETTINGS_RESET_HINT, &args)
                },
                value,
                enabled: row.enabled && row.kind.value().as_ref() != Some(&default_value),
            });
            if let (PreferenceKind::Number { control, .. }, PreferenceValue::Number(value)) =
                (&mut row.kind, default_value)
            {
                control.default_value = Some(value as f64);
            }
        }
        pages
    }

    fn raw_pages(&self, platform: Platform, localizer: &Localizer) -> Vec<PreferencePage> {
        use PreferenceId::*;
        let mut input = vec![
            row(
                Pressure,
                &localizer.text(MessageId::SETTINGS_PRESSURE_RESPONSE),
                &localizer.text(MessageId::SETTINGS_LOWER_VALUES_MAKE_LIGHT_PEN_PRESSURE_STRONGER),
                PreferenceKind::Number {
                    control: self.numeric_field(Pressure).unwrap().0,
                    value: self.pressure_gamma,
                },
            ),
            row(
                Feedback,
                &localizer.text(MessageId::SETTINGS_ENABLE_STROKE_PREDICTION),
                &localizer.text(MessageId::SETTINGS_REDUCE_THE_GAP_BETWEEN_YOUR_PEN_AND_THE_STROKE),
                PreferenceKind::Switch {
                    active: self.feedback,
                },
            ),
            row(
                PlatformPrediction,
                &localizer.text(match platform {
                    Platform::Android => MessageId::SETTINGS_USE_ANDROID_STROKE_PREDICTION,
                    Platform::Ios => MessageId::SETTINGS_USE_IPADOS_STROKE_PREDICTION,
                    Platform::Web => MessageId::SETTINGS_USE_BROWSER_STROKE_PREDICTION,
                    Platform::Windows => MessageId::SETTINGS_USE_WINDOWS_STROKE_PREDICTION,
                    Platform::Mac => MessageId::SETTINGS_USE_MACOS_STROKE_PREDICTION,
                    Platform::Gtk => MessageId::SETTINGS_USE_LINUX_STROKE_PREDICTION,
                }),
                "",
                PreferenceKind::Switch {
                    active: self.platform_prediction,
                },
            ),
            number(
                PredictionHorizon,
                &localizer.text(MessageId::SETTINGS_PREDICTION_AMOUNT),
                "",
                self,
            ),
        ];
        for r in &mut input {
            if matches!(r.id, PredictionHorizon | PlatformPrediction) {
                r.enabled = self.feedback;
            }
        }
        let mut groups = vec![
            vec![PreferenceGroup {
                title: localizer.text(MessageId::SETTINGS_INTERFACE).to_string(),
                rows: vec![
                    row(
                        Language,
                        &localizer.text(MessageId::SETTINGS_LANGUAGE),
                        "",
                        PreferenceKind::Choice {
                            presentation: ChoicePresentation::Dropdown,
                            icons: Vec::new(),
                            options: std::iter::once(localizer.text(MessageId::SETTINGS_LANGUAGE_SYSTEM).to_string())
                                .chain(SHIPPED_LANGUAGES.iter().map(|language| language.native_name().to_string())).collect(),
                            selected: match self.language {
                                LanguagePreference::System => 0,
                                LanguagePreference::Explicit(language) => SHIPPED_LANGUAGES.iter().position(|&candidate| candidate == language).map_or(0, |index| index as u32 + 1),
                            },
                        },
                    ),
                    row(
                        Theme,
                        &localizer.text(MessageId::SETTINGS_COLOR_THEME),
                        "",
                        PreferenceKind::Choice {
                            presentation: ChoicePresentation::Dropdown,
                            icons: Vec::new(),
                            options: vec![localizer.text(MessageId::SETTINGS_SYSTEM).to_string(), localizer.text(MessageId::SETTINGS_LIGHT).to_string(), localizer.text(MessageId::SETTINGS_DARK).to_string()],
                            selected: match self.theme {
                                None => 0,
                                Some(crate::Theme::Light) => 1,
                                Some(crate::Theme::Dark) => 2,
                            },
                        },
                    ),
                    row(
                        Transparency,
                        &localizer.text(MessageId::SETTINGS_PANEL_TRANSPARENCY),
                        "",
                        PreferenceKind::Choice {
                            presentation: ChoicePresentation::Circles {
                                alphas: crate::Transparency::CHOICES
                                    .map(|c| [false, true].map(|dark| c.0.surface_alpha(dark))),
                            },
                            options: crate::Transparency::CHOICES
                                .iter()
                                .map(|c| transparency_label(c.0, localizer))
                                .collect(),
                            icons: Vec::new(),
                            selected: crate::Transparency::CHOICES
                                .iter()
                                .position(|c| c.0 == self.transparency)
                                .unwrap() as u32,
                        },
                    ),
                    self.base_row(crate::Theme::Dark, localizer),
                    self.base_row(crate::Theme::Light, localizer),
                    self.accent_row(platform, localizer),
                ],
            }],
            vec![
                PreferenceGroup {
                    title: localizer.text(MessageId::SETTINGS_NAVIGATION).to_string(),
                    rows: vec![
                        number(
                            PanSpeed,
                            &localizer.text(MessageId::SETTINGS_SCROLL_PAN_SPEED),
                            "",
                            self,
                        ),
                        number(
                            ZoomSpeed,
                            &localizer.text(MessageId::SETTINGS_SCROLL_ZOOM_SPEED),
                            "",
                            self,
                        ),
                    ],
                },
                PreferenceGroup {
                    title: localizer.text(MessageId::SETTINGS_LAYERS).to_string(),
                    rows: vec![row(
                        PassThroughGroups,
                        &localizer.text(MessageId::SETTINGS_USE_PASS_THROUGH_FOR_NEW_GROUPS),
                        &localizer.text(MessageId::SETTINGS_GROUPED_LAYERS_BLEND_WITH_THE_LAYERS_BELOW_THE_GROUP),
                        PreferenceKind::Switch {
                            active: self.pass_through_groups,
                        },
                    )],
                },
            ],
            vec![
                PreferenceGroup {
                    title: localizer.text(MessageId::SETTINGS_POINTER).to_string(),
                    rows: vec![
                        row(
                            Cursor,
                            &localizer.text(MessageId::SETTINGS_CURSOR_SHAPE),
                            "",
                            PreferenceKind::Choice {
                                presentation: ChoicePresentation::Dropdown,
                                options: CursorMode::CHOICES.iter().map(|c| cursor_label(c.0, localizer)).collect(),
                                icons: CursorMode::CHOICES
                                    .iter()
                                    .map(|(mode, _)| mode.icon().into())
                                    .collect(),
                                selected: CursorMode::CHOICES
                                    .iter()
                                    .position(|c| c.0 == self.cursor)
                                    .unwrap() as u32,
                            },
                        ),
                        row(
                            HideCursorWhileDrawing,
                            &localizer.text(MessageId::SETTINGS_HIDE_CURSOR_WHEN_PAINTING),
                            "",
                            PreferenceKind::Switch {
                                active: self.hide_cursor_while_drawing,
                            },
                        ),
                    ],
                },
                PreferenceGroup {
                    title: localizer.text(MessageId::SETTINGS_PEN_RESPONSE).to_string(),
                    rows: input,
                },
                PreferenceGroup {
                    title: localizer.text(MessageId::SETTINGS_ERASER_END).to_string(),
                    rows: vec![
                        PreferenceRow {
                            visible: platform.pen_buttons(),
                            ..row(
                                EraserTool,
                                &localizer.text(MessageId::SETTINGS_TOOL),
                                &localizer.text(MessageId::SETTINGS_FLIP_THE_PEN_TO_USE_THIS_TOOL),
                                PreferenceKind::Choice {
                                    presentation: ChoicePresentation::Dropdown,
                                    options: std::iter::once(localizer.text(MessageId::SETTINGS_CURRENT_TOOL).to_string())
                                        .chain(crate::shortcuts::ERASER_END_TOOLS.iter().map(|c| c.localized_label(localizer).to_string()))
                                        .collect(),
                                    icons: Vec::new(),
                                    selected: self.eraser_end.tool.map_or(0, |tool| {
                                        1 + crate::shortcuts::ERASER_END_TOOLS.iter().position(|c| *c == tool).unwrap_or(0) as u32
                                    }),
                                },
                            )
                        },
                        PreferenceRow {
                            visible: platform.pen_buttons() && self.eraser_end.tool != Some(CommandId::Eraser),
                            ..row(
                                EraserErase,
                                &localizer.text(MessageId::SETTINGS_PAINT_WITH_TRANSPARENCY),
                                &localizer.text(MessageId::SETTINGS_ERASE_WITH_THE_TOOL_S_BRUSH),
                                PreferenceKind::Switch { active: self.eraser_end.erase },
                            )
                        },
                    ],
                },
            ],
            Vec::new(),
            vec![PreferenceGroup {
                title: APP_NAME.into(),
                rows: vec![
                    row(
                        Version,
                        &localizer.text(MessageId::SETTINGS_VERSION),
                        "",
                        PreferenceKind::Info {
                            value: env!("CARGO_PKG_VERSION").into(),
                        },
                    ),
                    row(
                        License,
                        &localizer.text(MessageId::SETTINGS_APPLICATION_LICENSE),
                        &localizer.text(MessageId::SETTINGS_BRANDING_AND_DEPENDENCIES_HAVE_SEPARATE_LICENSES),
                        PreferenceKind::Info {
                            value: env!("CARGO_PKG_LICENSE").into(),
                        },
                    ),
                    row(
                        Renderer,
                        &localizer.text(MessageId::SETTINGS_CANVAS_RENDERING),
                        "",
                        PreferenceKind::Info {
                            value: localizer.text(match platform {
                                Platform::Gtk => MessageId::SETTINGS_VULKAN_WAYLAND,
                                Platform::Web => MessageId::SETTINGS_WEBGPU,
                                _ => MessageId::SETTINGS_NATIVE_GPU,
                })
                            .to_string(),
                        },
                    ),
                    row(
                        Website,
                        &localizer.text(MessageId::SETTINGS_WEBSITE),
                        "",
                        PreferenceKind::Link {
                            label: crate::ApplicationLink::Website.display().into(),
                            url: crate::ApplicationLink::Website.url().into(),
                        },
                    ),
                    row(
                        SourceCode,
                        &localizer.text(MessageId::SETTINGS_SOURCE_CODE),
                        "",
                        PreferenceKind::Link {
                            label: crate::ApplicationLink::SourceCode.display().into(),
                            url: crate::ApplicationLink::SourceCode.url().into(),
                        },
                    ),
                ],
            }],
        ];
        groups[0].push(PreferenceGroup {
            title: CommandId::ZenMode.localized_label(localizer).to_string(),
            rows: vec![
                row(
                    PreferenceId::ZenShowCapy,
                    &localizer.text(MessageId::SETTINGS_SHOW_CAPY_IN_ZEN_MODE),
                    "",
                    PreferenceKind::Switch {
                        active: self.zen_show_capy,
                    },
                ),
                row(
                    PreferenceId::ZenRevealAtEdges,
                    &localizer.text(MessageId::SETTINGS_REVEAL_PANELS_NEAR_SCREEN_EDGES),
                    "",
                    PreferenceKind::Switch {
                        active: self.zen_reveal_at_edges,
                    },
                ),
                row(
                    PreferenceId::ZenIcon,
                    &localizer.text(MessageId::SETTINGS_BUTTON_ICON),
                    "",
                    PreferenceKind::Choice {
                        presentation: ChoicePresentation::ImageTiles { columns: 4 },
                        options: crate::ZenIcon::CHOICES.iter().map(|c| zen_label(c.0, localizer)).collect(),
                        icons: crate::ZenIcon::CHOICES
                            .iter()
                            .map(|c| c.0.icon().into())
                            .collect(),
                        selected: crate::ZenIcon::CHOICES
                            .iter()
                            .position(|c| c.0 == self.zen_icon)
                            .unwrap() as u32,
                    },
                ),
            ],
        });
        groups.insert(2, self.color_groups(localizer));
        SettingsPage::ALL
            .into_iter()
            .zip(groups)
            .map(|(id, groups)| PreferencePage {
                id,
                title: id.localized_title(localizer),
                icon: id.icon().into(),
                groups,
            })
            .collect()
    }
    fn accent_row(&self, platform: Platform, localizer: &Localizer) -> PreferenceRow {
        let presets = platform
            .system_accent()
            .then(|| Swatch::new(&localizer.text(MessageId::SETTINGS_SYSTEM), String::new(), Some(DEFAULT_ACCENT), Some("appearance")))
            .into_iter()
            .chain(ACCENTS.iter().enumerate().map(|(index, &(_, color))| {
                Swatch::new(&accent_label(index, localizer), color.to_string(), Some(color), None)
            }))
            .collect();
        swatch_row(
            PreferenceId::Accent,
            &localizer.text(MessageId::SETTINGS_ACCENT_COLOR),
            presets,
            self.accent,
            self.accent.unwrap_or(DEFAULT_ACCENT),
            DEFAULT_ACCENT,
            false,
            localizer,
        )
    }
    fn base_row(&self, theme: crate::Theme, localizer: &Localizer) -> PreferenceRow {
        let (id, title, value) = match theme {
            crate::Theme::Dark => (PreferenceId::DarkBase, localizer.text(MessageId::SETTINGS_DARK_THEME_BASE_COLOR), self.dark_base),
            crate::Theme::Light => (PreferenceId::LightBase, localizer.text(MessageId::SETTINGS_LIGHT_THEME_BASE_COLOR), self.light_base),
        };
        let presets = theme
            .base_choices()
            .iter()
            .map(|&c| Swatch::new(&c.to_string(), c.to_string(), Some(c), None))
            .collect();
        swatch_row(id, &title, presets, Some(value), value, theme.default_base(), true, localizer)
    }
    pub(crate) fn zen_menu(&self, _platform: Platform, localizer: &Localizer) -> Result<ContextMenu, String> {
        Ok(ContextMenu {
            title: CommandId::ZenMode.localized_label(localizer).to_string(),
            sections: vec![vec![ContextMenuItem::command(
                localizer.text(MessageId::SETTINGS_CHANGE_ICON).to_string(),
                UiAction::Preferences {
                    action: PreferenceAction::Reveal {
                        id: PreferenceId::ZenIcon,
                    },
                },
            )]],
        })
    }

    #[cfg(test)]
    pub(crate) fn field(&self, id: PreferenceId, platform: Platform) -> Result<PreferenceRow, String> {
        self.localized_field(id, platform, &Localizer::shared(UiLanguage::English))
    }
    pub(crate) fn localized_field(&self, id: PreferenceId, platform: Platform, localizer: &Localizer) -> Result<PreferenceRow, String> {
        self.localized_pages(platform, localizer)
            .into_iter()
            .flat_map(|p| p.groups)
            .flat_map(|g| g.rows)
            .find(|r| r.id == id)
            .ok_or_else(|| localizer.text(MessageId::SETTINGS_THIS_SETTING_ISN_T_AVAILABLE_ON_THIS_DEVICE).to_string())
    }
    #[cfg(test)]
    fn edit(
        &mut self,
        id: PreferenceId,
        value: PreferenceValue,
        platform: Platform,
    ) -> Result<(), String> {
        self.localized_edit(id, value, platform, &Localizer::shared(UiLanguage::English))
    }
    fn localized_edit(&mut self, id: PreferenceId, value: PreferenceValue, platform: Platform, localizer: &Localizer) -> Result<(), String> {
        let field = self.localized_field(id, platform, localizer)?;
        if !field.enabled {
            return Err(localizer.text(MessageId::SETTINGS_ENABLE_STROKE_PREDICTION_TO_CHANGE_THIS_SETTING).to_string());
        }
        let value = match (&field.kind, value) {
            (PreferenceKind::Number { .. }, PreferenceValue::Text(text))
                if text.trim().is_empty() =>
            {
                self.default_value(id, platform)?
            }
            (PreferenceKind::Swatches { .. }, PreferenceValue::Text(text))
                if text.trim().is_empty() =>
            {
                self.default_value(id, platform)?
            }
            (PreferenceKind::Number { .. }, PreferenceValue::Text(text)) => PreferenceValue::Number(crate::numeric::parse_numeric_text(&text).map_err(|reason| reason.message(localizer))?),
            (_, value) => value,
        };
        use PreferenceId::*;
        match (&field.kind, &value) {
            (PreferenceKind::Number { control, .. }, _) => {
                control.validate(value.number().ok_or_else(|| localizer.text(MessageId::SETTINGS_EXPECTED_A_NUMBER).to_string())?, self.numeric_field(id).unwrap().2)
                    .map_err(|reason| reason.message(localizer))?
            }
            (PreferenceKind::Choice { options, .. }, _)
                if value.choice().is_some_and(|v| (v as usize) < options.len()) => {}
            (PreferenceKind::Switch { .. }, PreferenceValue::Bool(_)) => {}
            (PreferenceKind::Swatches { .. }, PreferenceValue::Text(text)) => {
                if !text.trim().is_empty() {
                    HexColor::try_from(text.trim().to_owned()).map_err(|_| localizer.text(MessageId::SETTINGS_INVALID_COLOR).to_string())?;
                }
            }
            _ => return Err(localizer.text(MessageId::SETTINGS_INVALID_SETTING_VALUE).to_string()),
        }
        let n = value.number().unwrap_or(0.0);
        match id {
            NewColorSpace | NewBitDepth | NewBackground | PhotoDepth | MissingProfile => self.edit_color(id, value.choice().unwrap()),
            Language => {
                self.language = match value.choice().unwrap() {
                    0 => LanguagePreference::System,
                    index => LanguagePreference::Explicit(SHIPPED_LANGUAGES[index as usize - 1]),
                }
            }
            Theme => {
                self.theme = match value.choice().unwrap() {
                    1 => Some(crate::Theme::Light),
                    2 => Some(crate::Theme::Dark),
                    _ => None,
                }
            }
            Transparency => {
                self.transparency = crate::Transparency::CHOICES[value.choice().unwrap() as usize].0
            }
            Cursor => self.cursor = CursorMode::CHOICES[value.choice().unwrap() as usize].0,
            HideCursorWhileDrawing => {
                self.hide_cursor_while_drawing = matches!(value, PreferenceValue::Bool(true))
            }
            ZenIcon => self.zen_icon = crate::ZenIcon::CHOICES[value.choice().unwrap() as usize].0,
            ZenShowCapy => self.zen_show_capy = matches!(value, PreferenceValue::Bool(true)),
            ZenRevealAtEdges => {
                self.zen_reveal_at_edges = matches!(value, PreferenceValue::Bool(true))
            }
            DarkBase | LightBase => {
                let PreferenceValue::Text(text) = value else {
                    unreachable!()
                };
                let color = HexColor::try_from(text.trim().to_owned()).map_err(|_| localizer.text(MessageId::SETTINGS_INVALID_COLOR).to_string())?;
                if id == DarkBase {
                    self.dark_base = color;
                } else {
                    self.light_base = color;
                }
            }
            Accent => {
                let PreferenceValue::Text(text) = value else {
                    unreachable!()
                };
                self.accent = match text.trim() {
                    "" => None,
                    text => Some(HexColor::try_from(text.to_owned())?),
                }
                .filter(|color| platform.system_accent() || *color != DEFAULT_ACCENT);
            }
            Pressure => self.pressure_gamma = n,
            PanSpeed => self.pan_speed = n,
            ZoomSpeed => self.zoom_speed = n,
            PassThroughGroups => {
                self.pass_through_groups = matches!(value, PreferenceValue::Bool(true))
            }
            PredictionHorizon => self.prediction_ms = n,
            Feedback => self.feedback = matches!(value, PreferenceValue::Bool(true)),
            PlatformPrediction => {
                self.platform_prediction = matches!(value, PreferenceValue::Bool(true))
            }
            EraserTool => {
                self.eraser_end.tool = match value.choice().unwrap() {
                    0 => None,
                    index => Some(crate::shortcuts::ERASER_END_TOOLS[index as usize - 1]),
                }
            }
            EraserErase => self.eraser_end.erase = matches!(value, PreferenceValue::Bool(true)),
            Version | License | Renderer | Website | SourceCode => {
                return Err(localizer.text(MessageId::SETTINGS_THIS_INFORMATION_IS_READ_ONLY).to_string());
            }
        }
        Ok(())
    }

    fn default_value(
        &self,
        id: PreferenceId,
        platform: Platform,
    ) -> Result<PreferenceValue, String> {
        Self::default()
            .raw_pages(platform, &Localizer::shared(UiLanguage::English))
            .into_iter()
            .flat_map(|p| p.groups)
            .flat_map(|g| g.rows)
            .find(|r| r.id == id)
            .and_then(|r| r.kind.value())
            .ok_or_else(|| "This setting cannot be reset.".into())
    }
}
fn reset_binding(settings: &mut Settings, id: &str, platform: Platform, localizer: &Localizer) -> Result<(), String> {
    if !crate::shortcuts::definitions(platform).iter().any(|(d, _)| d.id == id) {
        return Err(localizer.text(MessageId::SETTINGS_UNKNOWN_SHORTCUT_ACTION).to_string());
    }
    let mut candidate = settings.clone();
    candidate.shortcuts.remove(id);
    for chord in candidate.base_keys(id) {
        if let Some(conflict) = candidate.conflict(id, &chord, platform).filter(|c| settings.shortcuts.contains_key(&c.id)) {
            return Err({ let mut args = crate::localization::FluentArgs::new(); args.set("action", conflict.label.resolve(localizer)); localizer.format(MessageId::SETTINGS_REMOVE_SHORTCUT_FIRST, &args) });
        }
    }
    *settings = candidate;
    Ok(())
}
impl PreferencesState {
    pub(crate) fn set_error(&mut self, id: MessageId, localizer: &Localizer) {
        self.error_source = Some(PreferenceErrorSource::Message(id));
        self.error = Some(localizer.text(id).to_string());
    }
    pub(crate) fn set_localization(&mut self, settings: &Settings, platform: Platform, localizer: &Localizer) {
        if let Some(source) = &self.error_source {
            self.error = source.message(platform, localizer);
        }
        self.refresh_capture(settings, platform, localizer);
        if let Some(key) = &self.shortcut_page.key {
            self.shortcut_query = key.localized_label(platform, localizer);
        }
        if let Some(import) = &mut self.keymap_import {
            import.set_localization(localizer);
        }
    }
    pub(crate) fn view(
        &self,
        settings: &Settings,
        platform: Platform,
        platform_prediction_available: bool,
        system_accent: Option<HexColor>,
        localizer: &Localizer,
    ) -> PreferencesView {
        let query = crate::search::normalize(self.query.trim());
        let mut pages = settings.localized_pages(platform, localizer);
        let canonical_pages = settings.pages(platform);
        for row in pages
            .iter_mut()
            .flat_map(|p| &mut p.groups)
            .flat_map(|g| &mut g.rows)
        {
            if row.id == PreferenceId::PlatformPrediction && !platform_prediction_available {
                row.enabled = false;
                row.kind = PreferenceKind::Switch { active: false };
            }
            if row.id == PreferenceId::PredictionHorizon
                && platform_prediction_available
                && settings.platform_prediction
            {
                row.enabled = false;
                row.visible = platform != Platform::Ios;
            }
            if !row.enabled
                && let Some(reset) = &mut row.reset
            {
                reset.enabled = false;
            }
            if let PreferenceKind::Swatches {
                swatches,
                value,
                custom,
                ..
            } = &mut row.kind
            {
                let system = system_accent.unwrap_or(DEFAULT_ACCENT);
                for swatch in swatches.iter_mut().filter(|s| !s.custom && s.value.is_empty()) {
                    swatch.color = Some(system);
                    swatch.foreground = Some(system.contrasting());
                }
                if value.is_empty() {
                    *custom = system.to_string();
                }
            }
        }
        let mut search_results = Vec::new();
        for page in &pages {
            for group in &page.groups {
                for row in &group.rows {
                    let value = match &row.kind {
                        PreferenceKind::Info { value } => value.as_str(),
                        PreferenceKind::Link { url, .. } => url.as_str(),
                        _ => "",
                    };
                    let (canonical_group, canonical) = canonical_pages.iter().flat_map(|page| &page.groups)
                        .find_map(|group| group.rows.iter().find(|candidate| candidate.id == row.id).map(|row| (group, row))).unwrap();
                    if row.visible
                        && !query.is_empty()
                        && crate::search::normalize(&format!(
                            "{} {} {} {} {} {} {} {} {} {} {}",
                            page.title, group.title, row.title, row.description, value,
                            page.id.title(), canonical_group.title, canonical.title, canonical.description,
                            preference_choice_search(&row.kind), preference_choice_search(&canonical.kind)
                        ))
                        .contains(&query)
                    {
                        search_results.push(PreferenceSearchResult {
                            title: row.title.clone(),
                            description: page.title.clone(),
                            action: PreferenceAction::Page { page: page.id },
                        });
                    }
                }
            }
        }
        let shortcuts = crate::shortcut_page::rows_localized(settings, platform, &self.shortcut_page, &self.shortcut_query, localizer);
        let shortcut_page = crate::shortcut_page::view_localized(settings, platform, &self.shortcut_page, &self.shortcut_query, &shortcuts, localizer);
        let english = Localizer::shared(UiLanguage::English);
        let canonical_shortcuts = if query.is_empty() || localizer.language() == UiLanguage::English { Vec::new() } else {
            crate::shortcut_page::rows_localized(settings, platform, &self.shortcut_page, &self.shortcut_query, &english)
        };
        let canonical_by_id: std::collections::BTreeMap<_, _> = canonical_shortcuts.iter().map(|row| (row.id.as_str(), row)).collect();
        let canonical_triggers = if query.is_empty() || localizer.language() == UiLanguage::English { Vec::new() } else {
            crate::shortcut_page::trigger_rows(settings, platform, &english)
        };
        for trigger in &shortcut_page.triggers {
            let canonical = canonical_triggers.iter().find(|row| row.id == trigger.id).unwrap_or(trigger);
            if !query.is_empty() && crate::search::normalize(&format!("{} {} {} {} {} {} {} {}", SettingsPage::Shortcuts.localized_title(localizer), trigger.label, trigger.action, trigger.detail,
                SettingsPage::Shortcuts.title(), canonical.label, canonical.action, canonical.detail)).contains(&query) {
                search_results.push(PreferenceSearchResult {
                    title: trigger.label.clone(),
                    description: SettingsPage::Input.localized_title(localizer),
                    action: PreferenceAction::Page { page: SettingsPage::Input },
                });
            }
        }
        for row in &shortcuts {
            let canonical = canonical_by_id.get(row.id.as_str()).copied().unwrap_or(row);
            if !query.is_empty()
                && crate::search::normalize(&format!(
                    "keyboard shortcuts {} {} {} {} {} {} {} {} {} {}",
                    SettingsPage::Shortcuts.localized_title(localizer), row.group, row.label, row.shortcut, row.detail,
                    canonical.group, canonical.label, canonical.shortcut, canonical.subgroup, canonical.detail
                ))
                .contains(&query)
            {
                search_results.push(PreferenceSearchResult {
                    title: row.label.clone(),
                    description: SettingsPage::Shortcuts.localized_title(localizer),
                    action: PreferenceAction::EditShortcut { id: row.id.clone() },
                });
            }
        }
        let shortcut_editor = self.editing_shortcut.as_ref().and_then(|id| {
            let row = shortcuts.iter().find(|r| &r.id == id)?;
            let bindings: Vec<_> = settings
                .keys(id)
                .iter()
                .map(|k| k.localized_label(platform, localizer))
                .collect();
            let (scope, overlaps) = settings.shortcut_scope_localized(id, platform, localizer);
            Some(ShortcutEditor {
                id: id.clone(),
                label: row.label.clone(),
                description: CommandId::ALL
                    .into_iter()
                    .find(|c| c.shortcut_id() == *id)
                    .map(|command| crate::customization::tool_choice_localized(crate::ToolbarControl::Command { command }, localizer).description)
                    .or_else(|| (!row.subgroup.is_empty()).then(|| { let mut args = crate::localization::FluentArgs::new(); args.set("tool", row.subgroup.as_str()); localizer.format(MessageId::SETTINGS_BRUSH_FOR_TOOL, &args) }))
                    .or_else(|| (!row.detail.is_empty()).then(|| row.detail.clone()))
                    .unwrap_or_else(|| row.group.clone()),
                group: row.group.clone(),
                scope,
                source: if settings.shortcuts.contains_key(id) {
                    localizer.text(MessageId::SETTINGS_CUSTOM).to_string()
                } else if settings.keymap_preset().is_some_and(|p| p.keys_for(id).is_some()) {
                    settings.keymap_preset().unwrap().preset.title.into()
                } else {
                    localizer.text(MessageId::SETTINGS_CAPYCANVAS_DEFAULT).to_string()
                },
                overlaps,
                can_add: bindings.len() < crate::shortcuts::MAX_SHORTCUTS,
                bindings,
                keys: settings.keys(id).iter().map(|k| k.localized_label_parts(platform, localizer)).collect(),
                gestures: row.gestures.clone(),
                defaults: settings
                    .base_keys(id)
                    .iter()
                    .map(|k| k.localized_label(platform, localizer))
                    .collect(),
                modified: row.modified,
            })
        });
        PreferencesView {
            title: localizer.text(MessageId::SETTINGS_TITLE).to_string(),
            close_label: localizer.text(MessageId::SETTINGS_CLOSE).to_string(),
            search_label: localizer.text(MessageId::SETTINGS_SEARCH).to_string(),
            search_placeholder: localizer.text(MessageId::SETTINGS_SEARCH_PLACEHOLDER).to_string(),
            clear_search_label: localizer.text(MessageId::SETTINGS_CLEAR_SEARCH).to_string(),
            no_results: localizer.text(MessageId::SETTINGS_NO_RESULTS).to_string(),
            text_edit_menu: crate::shortcuts::text_edit_menu_localized(platform, localizer),
            empty: !query.is_empty() && search_results.is_empty(),
            pages,
            page: self.page,
            reveal: self.reveal,
            query: self.query.clone(),
            // Android keeps its search field visible; an empty query shows
            // categories. Desktop/web retain their explicit search toggle.
            searching: self.searching && (platform != Platform::Android || !query.is_empty()),
            search_focus: self.search_focus,
            search_results,
            shortcut_query: self.shortcut_query.clone(),
            shortcut_editor,
            modifier_editor: self.modifier_editor.as_ref().map(|(key, per_tool)| {
                crate::shortcut_page::modifier_editor_localized(settings, platform, key, *per_tool, localizer)
            }),
            pen_button_editor: self
                .pen_editor
                .as_ref()
                .and_then(|(trigger, per_tool)| crate::shortcut_page::pen_editor_localized(settings, platform, trigger, *per_tool, localizer)),
            shortcut_page,
            keymap: crate::keymaps::view(settings, self.keymap_import.as_ref(), self.keymap_details),
            shortcuts,
            capture: self.capture.clone().map(|mut c| {
                if self.error.is_some() {
                    c.error = self.error.clone();
                }
                c.notice = c.error.clone().unwrap_or_else(|| {
                    if c.existing {
                        return localizer.text(MessageId::SETTINGS_ALREADY_A_MODIFIER_KEY).to_string();
                    }
                    c.conflict.as_ref().map_or_else(String::new, |label| {
                        { let mut args = crate::localization::FluentArgs::new(); args.set("action", label.as_str()); localizer.format(MessageId::SETTINGS_USED_BY, &args) }
                    })
                });
                c
            }),
            error: self.error.clone(),
        }
    }
    pub(crate) fn edit(
        &mut self,
        settings: &mut Settings,
        action: PreferenceAction,
        platform: Platform,
        localizer: &Localizer,
    ) -> bool {
        let language = matches!(action, PreferenceAction::Edit { id: PreferenceId::Language, .. } | PreferenceAction::Reset { id: PreferenceId::Language });
        let error = self.try_edit(settings, action.clone(), platform, localizer).err();
        let accepted = error.is_none();
        if error.is_some() || !language {
            self.error_source = error.as_ref().map(|reason| match reason {
                PreferenceEditError::KeymapImport(reason) => PreferenceErrorSource::KeymapImport(reason.clone()),
                PreferenceEditError::Localized(_) => match action {
                    PreferenceAction::Search { .. } | PreferenceAction::SearchShortcuts { .. } | PreferenceAction::SearchActionPicker { .. } => PreferenceErrorSource::Message(MessageId::SETTINGS_SEARCH_IS_TOO_LONG),
                    PreferenceAction::Reveal { .. } => PreferenceErrorSource::Message(MessageId::SETTINGS_THIS_SETTING_ISN_T_AVAILABLE_ON_THIS_DEVICE),
                    PreferenceAction::ShortcutCategory { .. } => PreferenceErrorSource::Message(MessageId::SETTINGS_UNKNOWN_SHORTCUT_CATEGORY),
                    PreferenceAction::OpenActionPicker { .. } | PreferenceAction::ResetTrigger { .. } => PreferenceErrorSource::Message(MessageId::SETTINGS_UNKNOWN_GESTURE_OR_PEN_BUTTON),
                    PreferenceAction::EditPenButton { .. } | PreferenceAction::OpenPenButtonPicker { .. } => PreferenceErrorSource::Message(MessageId::SETTINGS_UNKNOWN_PEN_BUTTON),
                    PreferenceAction::EditModifierKey { .. } | PreferenceAction::OpenModifierPicker { .. } | PreferenceAction::ResetModifierKey { .. } => PreferenceErrorSource::Message(MessageId::SETTINGS_UNKNOWN_MODIFIER_KEY),
                    PreferenceAction::ConfirmKeymapImport => PreferenceErrorSource::Message(MessageId::SETTINGS_CHOOSE_A_KEYMAP_FILE_FIRST),
                    PreferenceAction::ExportKeymap | PreferenceAction::ChooseKeymapFile => PreferenceErrorSource::Message(MessageId::SETTINGS_KEYMAP_FILES_NEED_THE_APP_S_FILE_CHOOSER),
                    action => PreferenceErrorSource::Action(Box::new(PreferenceErrorAction {
                        action,
                        settings: settings.clone(),
                        capture: self.capture.clone(),
                        editing_shortcut: self.editing_shortcut.clone(),
                        shortcut_page: self.shortcut_page.clone(),
                    })),
                },
            });
            self.error = error.map(|reason| reason.message(localizer));
        }
        if self.capture.is_none() {
            self.capture_key = None;
        }
        accepted
    }
    fn try_edit(
        &mut self,
        settings: &mut Settings,
        action: PreferenceAction,
        platform: Platform,
        localizer: &Localizer,
    ) -> Result<(), PreferenceEditError> {
        match action {
            PreferenceAction::Reveal { id } => {
                let page = settings
                    .pages(platform)
                    .into_iter()
                    .find(|p| p.groups.iter().flat_map(|g| &g.rows).any(|r| r.id == id))
                    .ok_or(localizer.text(MessageId::SETTINGS_THIS_SETTING_ISN_T_AVAILABLE_ON_THIS_DEVICE).to_string())?
                    .id;
                self.try_edit(settings, PreferenceAction::Page { page }, platform, localizer)?;
                self.reveal = Some(id);
            }
            PreferenceAction::Page { page } => {
                self.page = page;
                self.shortcut_page.category = None;
                self.modifier_editor = None;
                self.pen_editor = None;
                self.reveal = None;
                self.query.clear();
                self.searching = false;
                self.editing_shortcut = None;
                self.capture = None;
            }
            PreferenceAction::Search { query } => {
                if query.len() > 256 {
                    return Err(PreferenceEditError::Localized(localizer.text(MessageId::SETTINGS_SEARCH_IS_TOO_LONG).to_string()));
                }
                self.query = query;
                self.searching = true;
            }
            PreferenceAction::ToggleSearch { open } => {
                self.searching = open;
                if !open {
                    self.query.clear();
                }
            }
            PreferenceAction::SearchShortcuts { query } => {
                if query.len() > 256 {
                    return Err(PreferenceEditError::Localized(localizer.text(MessageId::SETTINGS_SEARCH_IS_TOO_LONG).to_string()));
                }
                if query != self.shortcut_query {
                    self.shortcut_page.key = None;
                }
                self.shortcut_query = query;
            }
            PreferenceAction::Edit { id, value } => settings.localized_edit(id, value, platform, localizer)?,
            PreferenceAction::Reset { id } => {
                settings.localized_edit(id, settings.default_value(id, platform)?, platform, localizer)?;
            }
            PreferenceAction::EditShortcut { id } => {
                let id = crate::shortcuts::hold_target(&id).unwrap_or(id);
                if !crate::shortcuts::definitions(platform)
                    .iter()
                    .any(|(d, _)| d.id == id)
                {
                    return Err(PreferenceEditError::Localized(localizer.text(MessageId::SETTINGS_UNKNOWN_SHORTCUT_ACTION).to_string()));
                }
                self.page = SettingsPage::Shortcuts;
                self.query.clear();
                self.searching = false;
                self.editing_shortcut = Some(id);
                self.capture = None;
            }
            PreferenceAction::CloseShortcutEditor => {
                self.editing_shortcut = None;
                self.capture = None;
            }
            PreferenceAction::RemoveShortcut { id, index } => {
                let target = crate::shortcuts::hold_target(&id).unwrap_or_else(|| id.clone());
                if self.editing_shortcut.as_ref() != Some(&target) {
                    return Err(PreferenceEditError::Localized(localizer.text(MessageId::SETTINGS_SHORTCUT_EDITOR_IS_NOT_OPEN).to_string()));
                }
                let mut keys = settings.keys(&id);
                if index >= keys.len() {
                    return Err(PreferenceEditError::Localized(localizer.text(MessageId::SETTINGS_UNKNOWN_SHORTCUT_BINDING).to_string()));
                }
                keys.remove(index);
                settings.shortcuts.insert(id, keys);
            }
            PreferenceAction::BeginShortcut { id } => {
                let (definition, _) = crate::shortcuts::definitions(platform)
                    .into_iter()
                    .find(|(d, _)| d.id == id)
                    .ok_or(localizer.text(MessageId::SETTINGS_UNKNOWN_SHORTCUT_ACTION).to_string())?;
                if settings.keys(&id).len() >= crate::shortcuts::MAX_SHORTCUTS {
                    return Err(PreferenceEditError::Localized(localizer.text(MessageId::SETTINGS_REMOVE_A_SHORTCUT_BEFORE_ADDING_ANOTHER).to_string()));
                }
                self.editing_shortcut = Some(crate::shortcuts::hold_target(&id).unwrap_or_else(|| id.clone()));
                self.capture_key = None;
                self.capture = Some(ShortcutCapture {
                    id,
                    label: definition.label.resolve(localizer),
                    chord: None,
                    shortcut: localizer.text(MessageId::SETTINGS_PRESS_A_NEW_KEY_COMBINATION).to_string(),
                    keys: Vec::new(),
                    conflict: None,
                    error: None,
                    existing: false,
                    notice: String::new(),
                });
            }
            PreferenceAction::CancelShortcut => self.capture = None,
            PreferenceAction::ConfirmShortcut { replace } => {
                let capture = self
                    .capture
                    .as_ref()
                    .ok_or(localizer.text(MessageId::SETTINGS_NO_SHORTCUT_IS_BEING_RECORDED).to_string())?;
                let chord = capture
                    .chord
                    .clone()
                    .ok_or(localizer.text(MessageId::SETTINGS_PRESS_A_KEY_COMBINATION_FIRST).to_string())?;
                let modifier = capture.id == crate::shortcuts::MODIFIER_CAPTURE;
                if modifier && !chord.holdable() {
                    return Err(PreferenceEditError::Localized(localizer.text(MessageId::SETTINGS_CHOOSE_ANOTHER_KEY_FOR_THIS_MODIFIER_KEY).to_string()));
                } else if !modifier {
                    chord.validate_for_localized(settings.held_shortcut(&capture.id, platform), localizer)?;
                }
                if !chord.available(platform) {
                    return Err(PreferenceEditError::Localized(localizer.text(MessageId::SETTINGS_THIS_SHORTCUT_IS_RESERVED_BY_THE_BROWSER).to_string()));
                }
                let mut keys = settings.keys(&capture.id);
                if !modifier && keys.contains(&chord) {
                    return Err(PreferenceEditError::Localized(localizer.text(MessageId::SETTINGS_THIS_SHORTCUT_IS_ALREADY_ASSIGNED_TO_THIS_ACTION).to_string()));
                }
                if !modifier && keys.len() >= crate::shortcuts::MAX_SHORTCUTS {
                    return Err(PreferenceEditError::Localized(localizer.text(MessageId::SETTINGS_REMOVE_A_SHORTCUT_BEFORE_ADDING_ANOTHER).to_string()));
                }
                let conflicts = settings.conflicts(&capture.id, &chord, platform);
                if let Some(conflict) = conflicts.first().filter(|_| !replace) {
                    return Err(PreferenceEditError::Localized({ let mut args = crate::localization::FluentArgs::new(); args.set("action", conflict.label.resolve(localizer)); localizer.format(MessageId::SETTINGS_ALREADY_ASSIGNED, &args) }));
                }
                for conflict in conflicts {
                    if conflict.id.starts_with(crate::shortcuts::MODIFIER_PREFIX) {
                        let mut table = settings.hold_keys(platform);
                        table.retain(|h| h.key != chord);
                        crate::shortcut_page::store_modifiers(settings, platform, table);
                        continue;
                    }
                    let keys = settings
                        .keys(&conflict.id)
                        .into_iter()
                        .filter(|c| *c != chord)
                        .collect();
                    settings.shortcuts.insert(conflict.id, keys);
                }
                if modifier {
                    let mut table = settings.hold_keys(platform);
                    if !table.iter().any(|h| h.key == chord) {
                        table.push(crate::shortcuts::HoldKey { key: chord.clone(), actions: Default::default() });
                    }
                    crate::shortcut_page::store_modifiers(settings, platform, table);
                    self.modifier_editor = Some((chord, false));
                } else {
                    keys.push(chord);
                    settings.shortcuts.insert(capture.id.clone(), keys);
                }
                self.capture = None;
            }
            PreferenceAction::ResetShortcut { id } => reset_binding(settings, &id, platform, localizer)?,
            PreferenceAction::ResetAllShortcuts => {
                settings.shortcuts.clear();
                settings.hold_keys = None;
                settings.gestures.clear();
                settings.pen_buttons.clear();
                self.capture = None;
            }
            PreferenceAction::EditModifierKey { key } => {
                if !settings.hold_keys(platform).iter().any(|h| h.key == key) {
                    return Err(PreferenceEditError::Localized(localizer.text(MessageId::SETTINGS_UNKNOWN_MODIFIER_KEY).to_string()));
                }
                self.page = SettingsPage::Shortcuts;
                self.capture = None;
                self.modifier_editor = Some((key, false));
            }
            PreferenceAction::CloseModifierKey => {
                self.modifier_editor = None;
                self.shortcut_page.modifier_picker = None;
                if self.capture.as_ref().is_some_and(|c| c.id == crate::shortcuts::MODIFIER_CAPTURE) {
                    self.capture = None;
                }
            }
            PreferenceAction::AddModifierKey => {
                self.editing_shortcut = None;
                self.modifier_editor = None;
                self.capture_key = None;
                self.capture = Some(ShortcutCapture {
                    id: crate::shortcuts::MODIFIER_CAPTURE.into(),
                    label: localizer.text(MessageId::SETTINGS_NEW_MODIFIER_KEY).to_string(),
                    chord: None,
                    shortcut: localizer.text(MessageId::SETTINGS_PRESS_A_KEY_OR_BUTTON).to_string(),
                    keys: Vec::new(),
                    conflict: None,
                    error: None,
                    existing: false,
                    notice: String::new(),
                });
            }
            PreferenceAction::ModifierKeyPerTool { key, per_tool } => {
                if !per_tool {
                    crate::shortcut_page::unify_modifier_localized(settings, platform, &key, localizer)?;
                }
                self.modifier_editor = Some((key, per_tool));
            }
            PreferenceAction::OpenModifierPicker { key, category } => {
                if !settings.hold_keys(platform).iter().any(|h| h.key == key) {
                    return Err(PreferenceEditError::Localized(localizer.text(MessageId::SETTINGS_UNKNOWN_MODIFIER_KEY).to_string()));
                }
                self.shortcut_page.modifier_picker = Some((key, category));
                self.shortcut_page.picker = Some((crate::shortcuts::MODIFIER_CAPTURE.into(), String::new()));
            }
            PreferenceAction::RemoveModifierKey { key } => {
                let mut table = settings.hold_keys(platform);
                table.retain(|h| h.key != key);
                crate::shortcut_page::store_modifiers(settings, platform, table);
                self.modifier_editor = None;
            }
            PreferenceAction::ResetModifierKey { key } => {
                let default = settings.default_hold_keys(platform).into_iter().find(|h| h.key == key);
                let mut table = settings.hold_keys(platform);
                match (table.iter().position(|h| h.key == key), default) {
                    (Some(index), Some(default)) => table[index] = default,
                    (Some(index), None) => {
                        table.remove(index);
                        self.modifier_editor = None;
                    }
                    (None, _) => return Err(PreferenceEditError::Localized(localizer.text(MessageId::SETTINGS_UNKNOWN_MODIFIER_KEY).to_string())),
                }
                crate::shortcut_page::store_modifiers(settings, platform, table);
            }
            PreferenceAction::SelectKeymap { id } => crate::keymaps::select(settings, &id)?,
            PreferenceAction::ImportKeymap { text } => {
                self.keymap_import = Some(crate::keymaps::import_typed(settings, &text, platform, localizer).map_err(PreferenceEditError::KeymapImport)?);
            }
            PreferenceAction::ConfirmKeymapImport => {
                *settings = self.keymap_import.take().ok_or(localizer.text(MessageId::SETTINGS_CHOOSE_A_KEYMAP_FILE_FIRST).to_string())?.settings;
                self.capture = None;
            }
            PreferenceAction::CancelKeymapImport => self.keymap_import = None,
            PreferenceAction::KeymapDetails { open } => self.keymap_details = open,
            PreferenceAction::ShortcutCategory { id } => {
                if id.as_deref().is_some_and(|id| {
                    id != crate::shortcut_page::MODIFIER_SECTION && !crate::shortcuts::SHORTCUT_SECTIONS.iter().any(|section| section.id() == id)
                }) {
                    return Err(PreferenceEditError::Localized(localizer.text(MessageId::SETTINGS_UNKNOWN_SHORTCUT_CATEGORY).to_string()));
                }
                self.shortcut_page.category = id;
            }
            PreferenceAction::SearchShortcutKey { chord } => {
                let modifiers = crate::Modifiers { command: chord.command, shift: chord.shift, alt: chord.alt };
                let chord = KeyChord::new(&chord.key, modifiers);
                self.shortcut_query = chord.localized_label(platform, localizer);
                self.shortcut_page.key = Some(chord);
            }
            PreferenceAction::ShortcutContext { category } => self.shortcut_page.context = category,
            PreferenceAction::ShortcutShow { show } => self.shortcut_page.show = show,
            PreferenceAction::OpenActionPicker { trigger } => {
                if !crate::GESTURE_TRIGGERS.iter().any(|t| t.id == trigger) {
                    return Err(PreferenceEditError::Localized(localizer.text(MessageId::SETTINGS_UNKNOWN_GESTURE_OR_PEN_BUTTON).to_string()));
                }
                self.shortcut_page.picker = Some((trigger, String::new()));
            }
            PreferenceAction::SearchActionPicker { query } => {
                if query.len() > 256 {
                    return Err(PreferenceEditError::Localized(localizer.text(MessageId::SETTINGS_SEARCH_IS_TOO_LONG).to_string()));
                }
                if let Some((_, current)) = &mut self.shortcut_page.picker {
                    *current = query;
                }
            }
            PreferenceAction::ChooseAction { id } => {
                if let Some((trigger, category)) = self.shortcut_page.pen_picker.clone() {
                    crate::shortcut_page::set_pen_button_localized(settings, platform, &trigger, category, &id, localizer)?;
                    self.shortcut_page.pen_picker = None;
                    self.shortcut_page.picker = None;
                    return Ok(());
                }
                if let Some((key, category)) = self.shortcut_page.modifier_picker.clone() {
                    crate::shortcut_page::set_modifier_localized(settings, platform, &key, category, &id, localizer)?;
                    self.shortcut_page.modifier_picker = None;
                    self.shortcut_page.picker = None;
                    return Ok(());
                }
                let (trigger, _) = self.shortcut_page.picker.clone().ok_or(localizer.text(MessageId::SETTINGS_CHOOSE_A_GESTURE_OR_PEN_BUTTON_FIRST).to_string())?;
                crate::shortcut_page::choose_localized(settings, platform, &trigger, &id, localizer)?;
                self.shortcut_page.picker = None;
            }
            PreferenceAction::ResetTrigger { trigger } => {
                if !crate::GESTURE_TRIGGERS.iter().any(|t| t.id == trigger) {
                    return Err(PreferenceEditError::Localized(localizer.text(MessageId::SETTINGS_UNKNOWN_GESTURE_OR_PEN_BUTTON).to_string()));
                }
                settings.gestures.remove(&trigger);
                settings.pen_buttons.remove(&trigger);
                self.shortcut_page.picker = None;
            }
            PreferenceAction::CloseActionPicker => {
                self.shortcut_page.picker = None;
                self.shortcut_page.modifier_picker = None;
                self.shortcut_page.pen_picker = None;
            }
            PreferenceAction::EditPenButton { trigger } => {
                if !GESTURE_TRIGGERS.iter().any(|t| t.id == trigger && t.held) {
                    return Err(PreferenceEditError::Localized(localizer.text(MessageId::SETTINGS_UNKNOWN_PEN_BUTTON).to_string()));
                }
                self.page = SettingsPage::Input;
                self.pen_editor = Some((trigger, false));
            }
            PreferenceAction::ClosePenButton => {
                self.pen_editor = None;
                self.shortcut_page.pen_picker = None;
            }
            PreferenceAction::PenButtonPerTool { trigger, per_tool } => {
                if !per_tool {
                    crate::shortcut_page::unify_pen_button_localized(settings, platform, &trigger, localizer)?;
                }
                self.pen_editor = Some((trigger, per_tool));
            }
            PreferenceAction::OpenPenButtonPicker { trigger, category } => {
                if !GESTURE_TRIGGERS.iter().any(|t| t.id == trigger && t.held) {
                    return Err(PreferenceEditError::Localized(localizer.text(MessageId::SETTINGS_UNKNOWN_PEN_BUTTON).to_string()));
                }
                self.shortcut_page.pen_picker = Some((trigger.clone(), category));
                self.shortcut_page.modifier_picker = None;
                self.shortcut_page.picker = Some((trigger, String::new()));
            }
            PreferenceAction::ExportKeymap | PreferenceAction::ChooseKeymapFile => {
                return Err(PreferenceEditError::Localized(localizer.text(MessageId::SETTINGS_KEYMAP_FILES_NEED_THE_APP_S_FILE_CHOOSER).to_string()));
            }
        }
        Ok(())
    }
    pub(crate) fn record(&mut self, settings: &Settings, chord: KeyChord, platform: Platform, localizer: &Localizer) {
        self.error = None;
        self.error_source = None;
        let modifier = self.capture.as_ref().is_some_and(|c| c.id == crate::shortcuts::MODIFIER_CAPTURE);
        let held = self.capture.as_ref().is_some_and(|c| settings.held_shortcut(&c.id, platform));
        if KeyChord::modifier(&chord.key)
            && !(modifier && chord.holdable())
            && !(held && chord.validate_for(true).is_ok())
        {
            return;
        }
        if chord.key == "escape" {
            self.capture = None;
            self.capture_key = None;
            return;
        }
        self.capture_key = Some(chord);
        self.refresh_capture(settings, platform, localizer);
    }
    fn refresh_capture(&mut self, settings: &Settings, platform: Platform, localizer: &Localizer) {
        if let Some(capture) = &mut self.capture {
            let modifier = capture.id == crate::shortcuts::MODIFIER_CAPTURE;
            capture.label = if modifier {
                localizer.text(MessageId::SETTINGS_NEW_MODIFIER_KEY).to_string()
            } else {
                crate::shortcuts::definitions(platform).into_iter().find(|(d, _)| d.id == capture.id)
                    .map_or_else(|| capture.label.clone(), |(d, _)| d.label.resolve(localizer))
            };
            let Some(chord) = self.capture_key.as_ref().or(capture.chord.as_ref()).cloned() else {
                capture.shortcut = localizer.text(if modifier { MessageId::SETTINGS_PRESS_A_KEY_OR_BUTTON } else { MessageId::SETTINGS_PRESS_A_NEW_KEY_COMBINATION }).to_string();
                return;
            };
            let held = settings.held_shortcut(&capture.id, platform);
            let valid = if modifier {
                (!chord.holdable()).then(|| localizer.text(MessageId::SETTINGS_CHOOSE_ANOTHER_KEY_FOR_THIS_MODIFIER_KEY).to_string())
            } else {
                chord.validate_for_localized(held, localizer).err()
            };
            capture.error = valid.or_else(|| {
                (!chord.available(platform))
                    .then(|| localizer.text(MessageId::SETTINGS_THIS_SHORTCUT_IS_RESERVED_BY_THE_BROWSER).to_string())
            });
            capture.existing = modifier && settings.hold_keys(platform).iter().any(|h| h.key == chord);
            capture.conflict = settings
                .conflict(&capture.id, &chord, platform)
                .map(|d| d.label.resolve(localizer))
                .filter(|_| !capture.existing);
            capture.shortcut = chord.localized_label(platform, localizer);
            capture.keys = chord.localized_label_parts(platform, localizer);
            capture.chord = capture.error.is_none().then_some(chord);
        }
    }
}

#[cfg(test)]
mod copy_tests {
    use super::*;

    fn accent_row(settings: &Settings, system: Option<HexColor>) -> (Vec<Swatch>, u32, String) {
        let row = PreferencesState::default()
            .view(settings, Platform::Gtk, false, system, &Localizer::shared(UiLanguage::English))
            .pages
            .into_iter()
            .flat_map(|p| p.groups)
            .flat_map(|g| g.rows)
            .find(|r| r.id == PreferenceId::Accent)
            .unwrap();
        let PreferenceKind::Swatches { swatches, selected, custom, .. } = row.kind else {
            panic!("accent row is swatches");
        };
        (swatches, selected, custom)
    }

    #[test]
    fn accent_swatches_follow_system_presets_and_custom_hex() {
        let teal = ACCENTS[1].1;
        let mut state = PreferencesState::default();
        let mut settings = Settings::default();
        let (swatches, selected, custom) = accent_row(&settings, Some(teal));
        assert_eq!(swatches.len(), ACCENTS.len() + 2);
        assert_eq!((swatches[0].label.as_str(), swatches[0].color, selected), ("System", Some(teal), 0));
        assert_eq!(custom, teal.to_string());
        let custom_swatch = swatches.last().unwrap();
        assert!(custom_swatch.custom && custom_swatch.color.is_none());
        let mut apply = |settings: &mut Settings, action| {
            state.edit(settings, action, Platform::Gtk, &Localizer::shared(UiLanguage::English));
            state.error.clone()
        };
        let edit = |text: &str| PreferenceAction::Edit {
            id: PreferenceId::Accent,
            value: PreferenceValue::Text(text.into()),
        };
        assert_eq!(apply(&mut settings, edit(&ACCENTS[5].1.to_string())), None);
        assert_eq!(settings.accent, Some(ACCENTS[5].1));
        assert_eq!(accent_row(&settings, Some(teal)).1, 6);
        assert_eq!(apply(&mut settings, edit(&ACCENTS[0].1.to_string())), None);
        assert_eq!(settings.accent, Some(DEFAULT_ACCENT), "Blue differs from System here");
        assert_eq!(apply(&mut settings, edit(" #12AB56 ")), None);
        let (swatches, selected, custom) = accent_row(&settings, Some(teal));
        assert_eq!(selected as usize, swatches.len() - 1);
        assert_eq!(swatches[selected as usize].color, Some(HexColor([0x12, 0xab, 0x56])));
        assert_eq!(custom, "#12ab56");
        assert!(apply(&mut settings, edit("#12")).is_some());
        assert_eq!(settings.accent, Some(HexColor([0x12, 0xab, 0x56])));
        let field = settings.field(PreferenceId::Accent, Platform::Gtk).unwrap();
        assert!(field.reset.as_ref().unwrap().enabled);
        assert_eq!(field.reset.unwrap().value, "System");
        let reset = PreferenceAction::Reset { id: PreferenceId::Accent };
        assert_eq!(apply(&mut settings, reset), None);
        assert_eq!(settings.accent, None);
        assert_eq!(apply(&mut settings, edit(&ACCENTS[2].1.to_string())), None);
        assert_eq!(apply(&mut settings, edit("")), None);
        assert_eq!(settings.accent, None);
        for (platform, system) in [(Platform::Mac, true), (Platform::Ios, false)] {
            let PreferenceKind::Swatches { swatches, .. } = settings.field(PreferenceId::Accent, platform).unwrap().kind else {
                unreachable!()
            };
            assert_eq!(swatches[0].label == "System", system, "{platform:?}");
        }
        let PreferenceKind::Swatches { swatches, selected, .. } =
            settings.field(PreferenceId::Accent, Platform::Web).unwrap().kind
        else {
            unreachable!()
        };
        assert_eq!((swatches[0].label.as_str(), selected), ("Blue", 0), "the web has no system accent");
        let mut web = Settings::default();
        let blue = PreferenceValue::Text(DEFAULT_ACCENT.to_string());
        let mut state = PreferencesState::default();
        state.edit(&mut web, PreferenceAction::Edit { id: PreferenceId::Accent, value: blue }, Platform::Web, &Localizer::shared(UiLanguage::English));
        assert_eq!(web.accent, None, "Blue is the web default");
    }

    #[test]
    fn base_colors_are_inline_grey_swatches_above_the_accent() {
        let rows = |settings: &Settings, platform| -> Vec<PreferenceRow> {
            settings.pages(platform)[0].groups[0].rows.clone()
        };
        for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Mac, Platform::Ios] {
            let ids: Vec<_> = rows(&Settings::default(), platform).iter().map(|r| r.id).collect();
            assert_eq!(&ids[ids.len() - 3..], [PreferenceId::DarkBase, PreferenceId::LightBase, PreferenceId::Accent]);
        }
        let mut state = PreferencesState::default();
        let mut settings = Settings::default();
        for theme in [crate::Theme::Dark, crate::Theme::Light] {
            let id = if theme == crate::Theme::Dark { PreferenceId::DarkBase } else { PreferenceId::LightBase };
            let base = |settings: &Settings| {
                if theme == crate::Theme::Dark { settings.dark_base } else { settings.light_base }
            };
            let kind = |settings: &Settings| rows(settings, Platform::Gtk).into_iter().find(|r| r.id == id).unwrap().kind;
            let PreferenceKind::Swatches { swatches, selected, inline, .. } = kind(&settings) else {
                panic!("GTK base colors are swatches");
            };
            assert!(inline);
            assert_eq!(swatches.len(), 5);
            assert_eq!(swatches[selected as usize].color, Some(theme.default_base()));
            let mut edit = |settings: &mut Settings, text: &str| {
                let value = PreferenceValue::Text(text.into());
                state.edit(settings, PreferenceAction::Edit { id, value }, Platform::Gtk, &Localizer::shared(UiLanguage::English));
                state.error.clone()
            };
            assert_eq!(edit(&mut settings, &swatches[0].value), None);
            assert_eq!(base(&settings), theme.base_choices()[0]);
            assert_eq!(edit(&mut settings, " #445566 "), None);
            let PreferenceKind::Swatches { swatches, selected, custom, .. } = kind(&settings) else { unreachable!() };
            assert!(swatches[selected as usize].custom);
            assert_eq!(custom, "#445566");
            assert!(edit(&mut settings, "#4455").is_some());
            assert_eq!(edit(&mut settings, ""), None);
            assert_eq!(base(&settings), theme.default_base());
        }
    }

    #[test]
    fn panel_transparency_rows_follow_the_presenting_hosts() {
        for platform in Platform::ALL {
            let row = Settings::default().pages(platform)[0].groups.iter()
                .flat_map(|g| g.rows.clone()).find(|r| r.id == PreferenceId::Transparency).unwrap();
            let json = serde_json::to_value(&row.kind).unwrap();
            assert_eq!(json["presentation"]["type"], "circles");
            assert_eq!(json["presentation"]["alphas"].as_array().unwrap().len(), 4);
            assert_eq!(json["selected"], 1, "Low is the default");
        }
    }

    #[test]
    fn accent_setting_is_omitted_until_chosen() {
        let json = serde_json::to_value(Settings::default()).unwrap();
        assert!(json.get("accent").is_none());
        let saved = Settings { accent: Some(ACCENTS[3].1), ..Settings::default() };
        let json = serde_json::to_string(&saved).unwrap();
        assert!(json.contains("\"accent\":\"#c88800\""));
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), saved);
    }

    #[test]
    fn cursor_choices_preserve_saved_modes_and_reset_after_reordering() {
        assert_eq!(CursorMode::CHOICES[0], (CursorMode::None, "None"));
        for platform in Platform::ALL {
            for saved in [
                "brush_size",
                "brush_size_cross",
                "cross",
                "dot",
                "none",
                "triangle",
                "single_pixel_dot",
                "sight",
                "brush_size_dot",
                "brush_size_single_pixel_dot",
            ] {
                let mut settings: Settings =
                    serde_json::from_value(serde_json::json!({ "cursor": saved })).unwrap();
                let row = settings.field(PreferenceId::Cursor, platform).unwrap();
                let PreferenceKind::Choice {
                    options,
                    icons,
                    selected,
                    ..
                } = row.kind
                else {
                    panic!()
                };
                assert_eq!(options.len(), 10);
                assert_eq!(icons.len(), options.len());
                assert_eq!(CursorMode::CHOICES[selected as usize].0, settings.cursor);
                settings
                    .edit(
                        PreferenceId::Cursor,
                        PreferenceValue::Choice(selected),
                        platform,
                    )
                    .unwrap();
                assert_eq!(serde_json::to_value(&settings).unwrap()["cursor"], saved);
                settings
                    .edit(PreferenceId::Cursor, PreferenceValue::Choice(0), platform)
                    .unwrap();
                assert_eq!(settings.cursor, CursorMode::None);
                PreferencesState::default().edit(
                    &mut settings,
                    PreferenceAction::Reset {
                        id: PreferenceId::Cursor,
                    },
                    platform,
                 &Localizer::shared(UiLanguage::English));
                assert_eq!(settings.cursor, CursorMode::BrushSize);
            }
        }
    }

    #[test]
    fn pointer_preferences_belong_to_input_on_every_platform() {
        for platform in Platform::ALL {
            let mut settings = Settings::default();
            assert!(settings.hide_cursor_while_drawing);
            for page in settings.pages(platform) {
                for group in page.groups {
                    for row in group.rows {
                        if matches!(
                            row.id,
                            PreferenceId::Cursor | PreferenceId::HideCursorWhileDrawing
                        ) {
                            assert_eq!(page.id, SettingsPage::Input);
                            assert_eq!(group.title, "Pointer");
                        }
                    }
                }
            }
            let id = PreferenceId::HideCursorWhileDrawing;
            assert!(
                settings
                    .edit(id, PreferenceValue::Number(0.0), platform)
                    .is_err()
            );
            settings
                .edit(id, PreferenceValue::Bool(false), platform)
                .unwrap();
            assert!(!settings.hide_cursor_while_drawing);
            let restored: Settings =
                serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
            assert_eq!(settings, restored);
            assert!(settings.field(id, platform).unwrap().reset.unwrap().enabled);
            settings
                .edit(id, settings.default_value(id, platform).unwrap(), platform)
                .unwrap();
            assert!(settings.hide_cursor_while_drawing);
        }
    }

    #[test]
    fn zen_preferences_roundtrip_and_reset() {
        for platform in Platform::ALL {
            let mut settings = Settings::default();
            let group = settings
                .pages(platform)
                .into_iter()
                .flat_map(|p| p.groups)
                .find(|g| g.title == "Zen mode")
                .unwrap();
            assert_eq!(
                group.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
                [
                    PreferenceId::ZenShowCapy,
                    PreferenceId::ZenRevealAtEdges,
                    PreferenceId::ZenIcon
                ]
            );
            assert!(settings.zen_show_capy);
            assert!(!settings.zen_reveal_at_edges);
            for (id, value) in [
                (PreferenceId::ZenShowCapy, false),
                (PreferenceId::ZenRevealAtEdges, true),
            ] {
                settings
                    .edit(id, PreferenceValue::Bool(value), platform)
                    .unwrap();
                let restored =
                    serde_json::from_str::<Settings>(&serde_json::to_string(&settings).unwrap())
                        .unwrap();
                assert_eq!(restored, settings);
                let mut preferences = PreferencesState::default();
                preferences.edit(&mut settings, PreferenceAction::Reset { id }, platform, &Localizer::shared(UiLanguage::English));
                assert!(preferences.error.is_none());
                assert_eq!(settings, Settings::default());
            }
            settings
                .edit(PreferenceId::ZenIcon, PreferenceValue::Choice(3), platform)
                .unwrap();
            let saved = serde_json::to_string(&settings).unwrap();
            assert_eq!(serde_json::from_str::<Settings>(&saved).unwrap(), settings);
            let mut state = PreferencesState::default();
            state.edit(
                &mut settings,
                PreferenceAction::Reset {
                    id: PreferenceId::ZenIcon,
                },
                platform,
             &Localizer::shared(UiLanguage::English));
            assert!(state.error.is_none());
            assert_eq!(settings, Settings::default());
        }
    }

    #[test]
    fn the_new_group_preference_sits_with_layers_on_the_canvas_page_and_resets() {
        for platform in Platform::ALL {
            let mut settings = Settings::default();
            let page = settings.pages(platform).into_iter().find(|p| p.id == SettingsPage::Canvas).unwrap();
            let group = page.groups.iter().find(|g| g.title == "Layers").unwrap();
            assert_eq!(group.rows.iter().map(|r| r.id).collect::<Vec<_>>(), [PreferenceId::PassThroughGroups]);
            assert_eq!(group.rows[0].title, "Use Pass Through for new groups");
            assert!(!settings.pass_through_groups);
            settings.edit(PreferenceId::PassThroughGroups, PreferenceValue::Bool(true), platform).unwrap();
            assert!(settings.pass_through_groups);
            let restored = serde_json::from_str::<Settings>(&serde_json::to_string(&settings).unwrap()).unwrap();
            assert_eq!(restored, settings);
            let mut preferences = PreferencesState::default();
            preferences.edit(&mut settings, PreferenceAction::Reset { id: PreferenceId::PassThroughGroups }, platform, &Localizer::shared(UiLanguage::English));
            assert!(preferences.error.is_none());
            assert_eq!(settings, Settings::default());
        }
    }

    #[test]
    fn reset_metadata_and_empty_commits_share_schema_defaults() {
        for platform in Platform::ALL {
            let mut settings = Settings::default();
            for row in settings
                .pages(platform)
                .into_iter()
                .flat_map(|p| p.groups)
                .flat_map(|g| g.rows)
            {
                assert_eq!(row.reset.is_some(), row.kind.value().is_some());
                let Some(reset) = row.reset else { continue };
                assert!(!reset.enabled);
                assert!(!reset.value.is_empty());
                if !row.visible || !row.enabled {
                    continue;
                }
                let value = match &row.kind {
                    PreferenceKind::Switch { active } => PreferenceValue::Bool(!active),
                    PreferenceKind::Choice { options, selected, .. } => {
                        PreferenceValue::Choice((selected + 1) % options.len() as u32)
                    }
                    PreferenceKind::Number { control, .. } => PreferenceValue::Number(control.max as f32),
                    PreferenceKind::Swatches { .. } => PreferenceValue::Text("#123456".into()),
                    PreferenceKind::Info { .. } | PreferenceKind::Link { .. } => unreachable!(),
                };
                settings.edit(row.id, value, platform).unwrap();
                let edited = settings.field(row.id, platform).unwrap().reset.unwrap();
                assert!(edited.enabled, "{platform:?} {:?}", row.id);
                let mut state = PreferencesState::default();
                state.edit(&mut settings, PreferenceAction::Reset { id: row.id }, platform, &Localizer::shared(UiLanguage::English));
                assert!(state.error.is_none());
                assert_eq!(settings, Settings::default(), "{platform:?} {:?}", row.id);
            }
            for (id, value) in [
                (
                    PreferenceId::DarkBase,
                    PreferenceValue::Text("#ABCDEF".into()),
                ),
                (PreferenceId::Pressure, PreferenceValue::Number(2.0)),
                (
                    PreferenceId::PredictionHorizon,
                    PreferenceValue::Number(32.0),
                ),
            ] {
                settings.edit(id, value.clone(), platform).unwrap();
                assert!(settings.field(id, platform).unwrap().reset.unwrap().enabled);
                settings
                    .edit(id, PreferenceValue::Text(String::new()), platform)
                    .unwrap();
                assert!(!settings.field(id, platform).unwrap().reset.unwrap().enabled);
                settings.edit(id, value, platform).unwrap();
                let mut state = PreferencesState::default();
                state.edit(&mut settings, PreferenceAction::Reset { id }, platform, &Localizer::shared(UiLanguage::English));
                assert!(state.error.is_none());
                assert!(!settings.field(id, platform).unwrap().reset.unwrap().enabled);
            }
            let mut state = PreferencesState::default();
            for (id, value) in [
                (PreferenceId::Theme, PreferenceValue::Choice(2)),
                (PreferenceId::Feedback, PreferenceValue::Bool(false)),
            ] {
                settings.edit(id, value, platform).unwrap();
                state.edit(&mut settings, PreferenceAction::Reset { id }, platform, &Localizer::shared(UiLanguage::English));
                assert!(state.error.is_none());
            }
            assert_eq!(settings, Settings::default());
            state.edit(
                &mut settings,
                PreferenceAction::Reset {
                    id: PreferenceId::Version,
                },
                platform,
             &Localizer::shared(UiLanguage::English));
            assert!(state.error.is_some());
            settings
                .edit(
                    PreferenceId::Feedback,
                    PreferenceValue::Bool(false),
                    platform,
                )
                .unwrap();
            assert!(
                settings
                    .edit(
                        PreferenceId::PredictionHorizon,
                        PreferenceValue::Text(String::new()),
                        platform
                    )
                    .is_err()
            );
            assert!(
                settings
                    .edit(
                        PreferenceId::DarkBase,
                        PreferenceValue::Text("#invalid".into()),
                        platform
                    )
                    .is_err()
            );
            settings.validate().unwrap();
        }
    }

    fn check(text: &str) {
        assert!(
            text.chars().count() <= 54,
            "Preferences copy exceeds 54 characters: {text}"
        );
        assert_eq!(text, text.trim());
        assert!(!text.contains(['\n', '\r', '\t']));
    }

    #[test]
    fn obvious_settings_do_not_repeat_the_title_or_visible_choices() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let settings = Settings::default();
            for id in [
                PreferenceId::Language,
                PreferenceId::Theme,
                PreferenceId::DarkBase,
                PreferenceId::LightBase,
                PreferenceId::Cursor,
                PreferenceId::PanSpeed,
                PreferenceId::ZoomSpeed,
                PreferenceId::PredictionHorizon,
                PreferenceId::Renderer,
                PreferenceId::ZenIcon,
                PreferenceId::ZenShowCapy,
                PreferenceId::ZenRevealAtEdges,
            ] {
                assert!(settings.field(id, platform).unwrap().description.is_empty());
            }
            assert_eq!(
                settings
                    .field(PreferenceId::DarkBase, platform)
                    .unwrap()
                    .title,
                "Dark theme base color"
            );
            assert_eq!(
                settings
                    .field(PreferenceId::LightBase, platform)
                    .unwrap()
                    .title,
                "Light theme base color"
            );
            for id in [
                PreferenceId::Pressure,
                PreferenceId::Feedback,
                PreferenceId::License,
            ] {
                assert!(!settings.field(id, platform).unwrap().description.is_empty());
            }
        }
    }

    #[test]
    fn settings_copy_is_short_on_every_platform() {
        let settings = Settings::default();
        for platform in Platform::ALL {
            for page in settings.pages(platform) {
                check(&page.title);
                for group in page.groups {
                    check(&group.title);
                    for row in group.rows {
                        check(&row.title);
                        check(&row.description);
                        if !row.description.is_empty() {
                            assert!(
                                row.description.ends_with('.'),
                                "Use a complete sentence: {}",
                                row.description
                            );
                        }
                        if let PreferenceKind::Choice { options, .. } = row.kind {
                            for option in options {
                                check(&option);
                            }
                        }
                    }
                }
            }
            for (definition, group) in crate::shortcuts::definitions(platform) {
                check(&definition.label.resolve(&Localizer::shared(UiLanguage::English)));
                check(&group.localized_label(&Localizer::shared(UiLanguage::English)));
                let mut state = PreferencesState::default();
                state.edit(
                    &mut settings.clone(),
                    PreferenceAction::BeginShortcut { id: definition.id },
                    platform,
                 &Localizer::shared(UiLanguage::English));
                check(&state.capture.as_ref().unwrap().shortcut);
                state.capture.as_mut().unwrap().conflict = Some(definition.label.resolve(&Localizer::shared(UiLanguage::English)));
                check(
                    &state
                        .view(&settings, platform, true, None, &Localizer::shared(UiLanguage::English))
                        .capture
                        .unwrap()
                        .notice,
                );
                state.error = Some("Choose another key for this shortcut.".into());
                assert_eq!(
                    state
                        .view(&settings, platform, true, None, &Localizer::shared(UiLanguage::English))
                        .capture
                        .unwrap()
                        .notice,
                    state.error.unwrap()
                );
            }
        }
    }
}

#[cfg(test)]
mod restore_tests {
    use super::*;

    #[test]
    fn localized_restore_preserves_fieldwise_salvage_and_launch_context() {
        let localization = Localizer::shared(UiLanguage::Japanese);
        let saved = r#"{"language":{"Explicit":"en"},"theme":"dark","pan_speed":-1,"new_document":null,"version":37}"#;
        let settings = Settings::restore_localized(saved, &localization);
        assert_eq!(settings.language, LanguagePreference::Explicit(UiLanguage::English));
        assert_eq!(settings.theme, Some(crate::Theme::Dark));
        assert_eq!(settings.pan_speed, Settings::default().pan_speed);
        assert_eq!(settings.new_document, Settings::default().new_document);
        assert_eq!(settings.version, Settings::default().version);
        assert_eq!(localization.language(), UiLanguage::Japanese);
        assert_eq!(settings, Settings::restore(saved));
    }

    #[test]
    fn saved_settings_keep_every_field_this_build_reads() {
        let chosen = Settings {
            theme: Some(Theme::Dark),
            pan_speed: 2.0,
            zen_show_capy: false,
            zen_reveal_at_edges: true,
            pressure_gamma: 1.5,
            ..Default::default()
        };
        let mut saved = serde_json::to_value(&chosen).unwrap();
        assert_eq!(Settings::restore(&saved.to_string()), chosen);
        saved["retired_setting"] = serde_json::json!(true);
        saved["zen_reveal_at_edges"] = serde_json::json!({});
        saved["pressure_gamma"] = serde_json::json!(-1.0);
        assert_eq!(
            Settings::restore(&saved.to_string()),
            Settings {
                zen_reveal_at_edges: false,
                pressure_gamma: 1.0,
                ..chosen
            }
        );
        for saved in ["", "not json", "null", "[]", r#"{"version":2}"#, r#"{"shortcuts":7}"#] {
            assert_eq!(Settings::restore(saved), Settings::default(), "{saved}");
        }
    }
}

#[cfg(test)]
mod localization_tests {
    use super::*;

    #[test]
    fn refused_keymap_language_refresh_retains_reason_and_existing_candidate() {
        let english = Localizer::shared(UiLanguage::English);
        let mut settings = Settings::default();
        let mut state = PreferencesState::default();
        let text = crate::keymaps::export(&settings);
        assert!(state.edit(&mut settings, PreferenceAction::ImportKeymap { text }, Platform::Gtk, &english));
        let candidate = state.keymap_import.as_ref().unwrap().settings.clone();
        let before = settings.clone();
        for text in [format!("{}{{broken 日本語", " ".repeat(1024 * 1024)), r#"{"format":"unsupported","version":1}"#.into(), r#"{"format":"capycanvas-keymap","version":999}"#.into()] {
            assert!(!state.edit(&mut settings, PreferenceAction::ImportKeymap { text }, Platform::Gtk, &english));
            let Some(PreferenceErrorSource::KeymapImport(reason)) = state.error_source.clone() else { panic!("keymap refusal must retain its reason"); };
            for &language in SHIPPED_LANGUAGES {
                let localizer = Localizer::shared(language);
                state.set_localization(&settings, Platform::Gtk, &localizer);
                assert_eq!(state.error.as_ref(), Some(&reason.message(&localizer)));
                assert_eq!(state.keymap_import.as_ref().unwrap().settings, candidate);
                assert_eq!(settings, before);
            }
        }
    }

    #[test]
    fn language_refresh_preserves_validation_and_search_drafts() {
        let english = Localizer::shared(UiLanguage::English);
        let mut state = PreferencesState {
            page: SettingsPage::Input,
            reveal: Some(PreferenceId::Pressure),
            query: "Literal 日本語 🖌".into(),
            searching: true,
            search_focus: 12,
            shortcut_query: "User text 日本語".into(),
            ..Default::default()
        };
        let mut settings = Settings::default();
        let action = PreferenceAction::Edit { id: PreferenceId::Pressure, value: PreferenceValue::Text("１．５".into()) };
        state.edit(&mut settings, action.clone(), Platform::Gtk, &english);
        let error = state.error.clone().unwrap();
        let before = settings.clone();
        for language in [UiLanguage::Japanese, UiLanguage::Korean, UiLanguage::English] {
            let l = Localizer::shared(language);
            state.set_localization(&settings, Platform::Gtk, &l);
            let mut expected = PreferencesState::default();
            expected.edit(&mut settings.clone(), action.clone(), Platform::Gtk, &l);
            assert_eq!(state.error, expected.error);
            assert_eq!(settings, before);
            assert_eq!(state.page, SettingsPage::Input);
            assert_eq!(state.reveal, Some(PreferenceId::Pressure));
            assert_eq!(state.query, "Literal 日本語 🖌");
            assert!(state.searching);
            assert_eq!(state.search_focus, 12);
            assert_eq!(state.shortcut_query, "User text 日本語");
        }
        assert!(state.edit(&mut settings, PreferenceAction::Edit { id: PreferenceId::Language, value: PreferenceValue::Choice(2) }, Platform::Gtk, &english));
        assert_eq!(state.error.as_ref(), Some(&error));
        state.set_error(MessageId::SETTINGS_NATIVE_PREDICTION_UNAVAILABLE, &english);
        let japanese = Localizer::shared(UiLanguage::Japanese);
        state.set_localization(&settings, Platform::Gtk, &japanese);
        assert_eq!(state.error.as_ref().unwrap(), &japanese.text(MessageId::SETTINGS_NATIVE_PREDICTION_UNAVAILABLE).to_string());
        let key = KeyChord::new("enter", Default::default());
        state.edit(&mut settings, PreferenceAction::SearchShortcutKey { chord: key.clone() }, Platform::Gtk, &english);
        state.shortcut_page.context = Some(ToolCategory::Drawing);
        state.shortcut_page.show = crate::ShortcutShow::Customized;
        state.set_localization(&settings, Platform::Gtk, &japanese);
        assert_eq!(state.shortcut_query, key.localized_label(Platform::Gtk, &japanese));
        assert_eq!(state.shortcut_page.key, Some(key));
        assert_eq!(state.shortcut_page.context, Some(ToolCategory::Drawing));
        assert_eq!(state.shortcut_page.show, crate::ShortcutShow::Customized);
    }

    #[test]
    fn language_refresh_keeps_shortcut_conflicts_and_confirmation() {
        let english = Localizer::shared(UiLanguage::English);
        for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Windows] {
            let mut settings = Settings::default();
            let mut state = PreferencesState::default();
            state.edit(&mut settings, PreferenceAction::BeginShortcut { id: CommandId::Redo.shortcut_id() }, platform, &english);
            let chord = settings.keys(&CommandId::Undo.shortcut_id()).first().unwrap().clone();
            state.record(&settings, chord.clone(), platform, &english);
            state.edit(&mut settings, PreferenceAction::ConfirmShortcut { replace: false }, platform, &english);
            let before = settings.clone();
            for &language in SHIPPED_LANGUAGES {
                let l = Localizer::shared(language);
                state.set_localization(&settings, platform, &l);
                let capture = state.capture.as_ref().unwrap();
                assert_eq!(capture.chord.as_ref(), Some(&chord));
                assert_eq!(capture.label, CommandId::Redo.localized_label(&l).to_string());
                assert_eq!(capture.conflict.as_ref().unwrap(), &CommandId::Undo.localized_label(&l).to_string());
                let view = state.view(&settings, platform, true, None, &l);
                assert_eq!(view.capture.unwrap().notice, state.error.clone().unwrap());
                assert_eq!(settings, before);
            }
            state.edit(&mut settings, PreferenceAction::ConfirmShortcut { replace: true }, platform, &english);
            assert!(state.error.is_none());
            assert!(state.capture.is_none());
            assert!(settings.keys(&CommandId::Redo.shortcut_id()).contains(&chord));
            assert!(!settings.keys(&CommandId::Undo.shortcut_id()).contains(&chord));
        }
    }

    #[test]
    fn language_refresh_keeps_rejected_keys_and_enter_escape_behavior() {
        let english = Localizer::shared(UiLanguage::English);
        let japanese = Localizer::shared(UiLanguage::Japanese);
        let mut settings = Settings::default();
        let mut state = PreferencesState::default();
        let id = CommandId::Undo.shortcut_id();
        state.edit(&mut settings, PreferenceAction::BeginShortcut { id: id.clone() }, Platform::Web, &english);
        state.set_localization(&settings, Platform::Web, &japanese);
        assert_eq!(state.capture.as_ref().unwrap().shortcut, japanese.text(MessageId::SETTINGS_PRESS_A_NEW_KEY_COMBINATION).to_string());
        let reserved = KeyChord::new("w", crate::Modifiers { command: true, ..Default::default() });
        state.record(&settings, reserved.clone(), Platform::Web, &english);
        state.set_localization(&settings, Platform::Web, &japanese);
        let capture = state.capture.as_ref().unwrap();
        assert!(capture.chord.is_none());
        assert_eq!(capture.error.as_ref().unwrap(), &japanese.text(MessageId::SETTINGS_THIS_SHORTCUT_IS_RESERVED_BY_THE_BROWSER).to_string());
        assert_eq!(capture.shortcut, reserved.localized_label(Platform::Web, &japanese));
        let enter = KeyChord::new("enter", Default::default());
        state.record(&settings, enter.clone(), Platform::Web, &japanese);
        state.set_localization(&settings, Platform::Web, &english);
        assert_eq!(state.capture.as_ref().unwrap().chord.as_ref(), Some(&enter));
        state.edit(&mut settings, PreferenceAction::ConfirmShortcut { replace: true }, Platform::Web, &english);
        assert!(settings.keys(&id).contains(&enter));
        state.edit(&mut settings, PreferenceAction::BeginShortcut { id }, Platform::Web, &english);
        state.set_localization(&settings, Platform::Web, &japanese);
        state.record(&settings, KeyChord::new("escape", Default::default()), Platform::Web, &japanese);
        assert!(state.capture.is_none());
    }

    #[test]
    fn language_refresh_preserves_refused_picker_and_modifier_drafts() {
        let english = Localizer::shared(UiLanguage::English);
        let japanese = Localizer::shared(UiLanguage::Japanese);
        let mut settings = Settings::default();
        let mut state = PreferencesState::default();
        let action = PreferenceAction::ChooseAction { id: "literal unknown action 日本語".into() };
        for pen in [true, false] {
            if pen {
                state.edit(&mut settings, PreferenceAction::OpenPenButtonPicker { trigger: "pen.button.primary".into(), category: Some(ToolCategory::Drawing) }, Platform::Gtk, &english);
            } else {
                let key = settings.hold_keys(Platform::Gtk).first().unwrap().key.clone();
                state.edit(&mut settings, PreferenceAction::OpenModifierPicker { key, category: Some(ToolCategory::Drawing) }, Platform::Gtk, &english);
            }
            state.edit(&mut settings, PreferenceAction::SearchActionPicker { query: "literal 日本語".into() }, Platform::Gtk, &english);
            state.edit(&mut settings, action.clone(), Platform::Gtk, &english);
            assert!(state.error.is_some());
            let picker = state.shortcut_page.picker.clone();
            let pen_picker = state.shortcut_page.pen_picker.clone();
            let modifier_picker = state.shortcut_page.modifier_picker.clone();
            state.set_localization(&settings, Platform::Gtk, &japanese);
            assert_eq!(state.shortcut_page.picker, picker);
            assert_eq!(state.shortcut_page.pen_picker, pen_picker);
            assert_eq!(state.shortcut_page.modifier_picker, modifier_picker);
            state.edit(&mut settings, PreferenceAction::CloseActionPicker, Platform::Gtk, &japanese);
        }
        state.edit(&mut settings, PreferenceAction::AddModifierKey, Platform::Gtk, &english);
        let key = settings.hold_keys(Platform::Gtk).first().unwrap().key.clone();
        state.record(&settings, key.clone(), Platform::Gtk, &english);
        state.set_localization(&settings, Platform::Gtk, &japanese);
        let capture = state.capture.as_ref().unwrap();
        assert!(capture.existing);
        assert_eq!(capture.chord.as_ref(), Some(&key));
        assert_eq!(capture.label, japanese.text(MessageId::SETTINGS_NEW_MODIFIER_KEY).to_string());
        assert_eq!(state.view(&settings, Platform::Gtk, true, None, &japanese).capture.unwrap().notice, japanese.text(MessageId::SETTINGS_ALREADY_A_MODIFIER_KEY).to_string());
    }

    #[test]
    fn language_preferences_restore_only_shipped_choices() {
        assert_eq!(Settings::default().language, LanguagePreference::System);
        let mut settings = Settings::default();
        for &language in SHIPPED_LANGUAGES {
            settings.language = LanguagePreference::Explicit(language);
            let saved = serde_json::to_string(&settings).unwrap();
            assert_eq!(Settings::restore(&saved), settings);
        }
        assert_eq!(Settings::restore(r#"{"language":{"Explicit":"unknown"}}"#).language, LanguagePreference::System);
        for language in UiLanguage::ALL.into_iter().filter(|language| !SHIPPED_LANGUAGES.contains(language)) {
            let saved = serde_json::json!({"language": LanguagePreference::Explicit(language)}).to_string();
            assert_eq!(Settings::restore(&saved).language, LanguagePreference::System);
        }
    }

    #[test]
    fn language_edit_and_reset_preserve_active_context() {
        let localizer = Localizer::shared(UiLanguage::English);
        let mut state = PreferencesState::default();
        let mut settings = Settings::default();
        for (index, &language) in SHIPPED_LANGUAGES.iter().enumerate() {
            state.edit(&mut settings, PreferenceAction::Edit { id: PreferenceId::Language, value: PreferenceValue::Choice(index as u32 + 1) }, Platform::Gtk, &localizer);
            assert!(state.error.is_none());
            assert_eq!(settings.language, LanguagePreference::Explicit(language));
            assert_eq!(localizer.language(), UiLanguage::English);
            let row = settings.localized_field(PreferenceId::Language, Platform::Gtk, &localizer).unwrap();
            assert!(row.reset.unwrap().enabled);
        }
        state.edit(&mut settings, PreferenceAction::Reset { id: PreferenceId::Language }, Platform::Gtk, &localizer);
        assert_eq!(settings.language, LanguagePreference::System);
        assert!(!settings.localized_field(PreferenceId::Language, Platform::Gtk, &localizer).unwrap().reset.unwrap().enabled);
    }

    #[test]
    fn preference_search_matches_translated_and_canonical_labels() {
        let localizer = Localizer::shared(UiLanguage::Japanese);
        for query in ["言語", "ＬＡＮＧＵＡＧＥ"] {
            let state = PreferencesState { query: query.into(), ..Default::default() };
            let view = state.view(&Settings::default(), Platform::Gtk, true, None, &localizer);
            assert!(view.search_results.iter().any(|result| result.title == "言語"));
        }
    }

    #[test]
    fn preference_search_keeps_canonical_shortcut_and_trigger_actions() {
        let localizer = Localizer::shared(UiLanguage::Japanese);
        let mut settings = Settings::default();
        crate::shortcut_page::set_pen_button_localized(&mut settings, Platform::Gtk, "pen.button.primary", None, "command.Eyedropper", &localizer).unwrap();
        for query in ["Undo", "元に戻す"] {
            let state = PreferencesState { query: query.into(), ..Default::default() };
            let view = state.view(&settings, Platform::Gtk, true, None, &localizer);
            assert!(view.search_results.iter().any(|result| result.action == PreferenceAction::EditShortcut { id: crate::CommandId::Undo.shortcut_id() }));
            let trigger = view.shortcut_page.triggers.iter().find(|trigger| trigger.id == "touch.tap.2").unwrap();
            assert!(view.search_results.iter().any(|result| result.title == trigger.label && result.action == PreferenceAction::Page { page: SettingsPage::Input }));
        }
        for query in ["Sample color", "色を取得"] {
            let state = PreferencesState { query: query.into(), ..Default::default() };
            let view = state.view(&settings, Platform::Gtk, true, None, &localizer);
            let trigger = view.shortcut_page.triggers.iter().find(|trigger| trigger.id == "pen.button.primary").unwrap();
            assert!(view.search_results.iter().any(|result| result.title == trigger.label && result.action == PreferenceAction::Page { page: SettingsPage::Input }));
        }
    }

    #[test]
    fn preference_search_normalizes_compatibility_characters() {
        let localizer = Localizer::shared(UiLanguage::English);
        let state = PreferencesState { query: "ＣＯＬＯＲ ＴＨＥＭＥ".into(), ..Default::default() };
        let view = state.view(&Settings::default(), Platform::Gtk, true, None, &localizer);
        assert!(view.search_results.iter().any(|result| result.title == "Color theme"));
    }
}

#[cfg(test)]
mod launch_language_tests {
    use super::Settings;
    use crate::{LanguagePreference, UiLanguage};
    #[test]
    fn saved_launch_preference_is_data_only_and_matches_fieldwise_salvage() {
        for saved in ["", "broken", "[]", "{}", r#"{"language":"broken"}"#, r#"{"language":{"Explicit":"unknown"}}"#] {
            assert_eq!(Settings::language_preference(saved), LanguagePreference::System);
        }
        for saved in [r#"{"language":{"Explicit":"en"},"pressure_gamma":"broken"}"#, r#"{"language":{"Explicit":"en"},"pressure_gamma":-5,"version":999}"#] {
            assert_eq!(Settings::language_preference(saved), LanguagePreference::Explicit(UiLanguage::English));
        }
        for language in UiLanguage::ALL {
            let saved = serde_json::json!({"language": LanguagePreference::Explicit(language)}).to_string();
            let expected = if crate::localization::SHIPPED_LANGUAGES.contains(&language) { LanguagePreference::Explicit(language) } else { LanguagePreference::System };
            assert_eq!(Settings::language_preference(&saved), expected);
        }
    }
}

#[cfg(test)]
mod numeric_input_tests {
    use super::*;

    #[test]
    fn settings_compatibility_numeric_text_is_refused_without_mutation() {
        let localization = Localizer::shared(UiLanguage::English);
        let mut settings = Settings::default();
        let before = settings.clone();
        for text in ["１．５", "２３", "１２３", "５", "１＋１", "１，５", "２ ×", "ＮａＮ", "ｉｎｆ", "ﷺﷺﷺﷺﷺﷺﷺﷺﷺﷺﷺﷺ"] {
            assert!(settings.localized_edit(PreferenceId::Pressure, PreferenceValue::Text(text.into()), Platform::Gtk, &localization).is_err(), "{text}");
            assert_eq!(settings, before);
        }
        settings.localized_edit(PreferenceId::Pressure, PreferenceValue::Text("1.5".into()), Platform::Gtk, &localization).unwrap();
        assert_eq!(settings.pressure_gamma, 1.5);
        settings.validate_localized(&localization).unwrap();
    }
}
