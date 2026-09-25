//! Portable preferences, editor state and host requests. Hosts render this model
//! and perform storage/window services; they do not decide settings policy.
use crate::*;
use layer_engine::PredictionAlgorithm as StrokePrediction;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[path = "settings_color.rs"]
mod color;
pub use color::{MissingProfilePolicy, PhotoOpenPolicy};

const PREDICTION_ALGORITHMS: [(StrokePrediction, &str); 1] =
    [(StrokePrediction::Optimized, "Smooth Motion (Optimized)")];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    #[default]
    Generic,
    Gtk,
    Web,
    Windows,
    Mac,
    Ios,
    Android,
}
impl Platform {
    pub fn color_picker(self) -> bool {
        matches!(self, Self::Gtk | Self::Web | Self::Android)
    }
    pub fn apple(self) -> bool {
        matches!(self, Self::Mac | Self::Ios)
    }
    pub fn native_windows(self) -> bool {
        // The iOS host is an iPad app with independent native editor scenes.
        matches!(self, Self::Gtk | Self::Windows | Self::Mac | Self::Ios)
    }
    pub fn system_accent(self) -> bool {
        matches!(self, Self::Gtk)
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockVisibility {
    #[default]
    Fullscreen,
    Always,
    Never,
}
impl ClockVisibility {
    pub const CHOICES: [(Self, &'static str); 3] = [
        (Self::Fullscreen, "In fullscreen mode"),
        (Self::Always, "Always"),
        (Self::Never, "Never"),
    ];
    pub const fn visible(self, fullscreen: bool) -> bool {
        match self {
            Self::Fullscreen => fullscreen,
            Self::Always => true,
            Self::Never => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub version: u32,
    pub new_document: NewDocumentSettings,
    pub photo_open: PhotoOpenPolicy,
    pub theme: Option<Theme>,
    // Keep the persisted key compatible with the original clock-only setting.
    pub show_clock: ClockVisibility,
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
    pub feedback: bool,
    pub platform_prediction: bool,
    pub prediction_algorithm: StrokePrediction,
    pub prediction_ms: f32,
    /// Retained for saved-settings compatibility; preview tracking is automatic.
    pub tip_lock: f32,
    /// Only overrides are stored. Empty keys disable an action's shortcut.
    pub shortcuts: BTreeMap<String, Vec<KeyChord>>,
    /// Any typed UiAction can be registered, including parameterized controls.
    pub custom_actions: Vec<ShortcutDefinition>,
    /// Per-preset slider values, shared by every placement of that slider.
    pub slider_bookmarks: BTreeMap<String, SliderBookmarks>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            new_document: NewDocumentSettings::default(),
            photo_open: PhotoOpenPolicy::default(),
            theme: None,
            show_clock: ClockVisibility::default(),
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
            feedback: true,
            platform_prediction: true,
            prediction_algorithm: StrokePrediction::default(),
            prediction_ms: 16.0,
            tip_lock: 1.0,
            shortcuts: BTreeMap::new(),
            custom_actions: Vec::new(),
            slider_bookmarks: BTreeMap::new(),
        }
    }
}
impl Settings {
    /// Discard retired preferences, preserving strict validation of every
    /// other field and all of the user's other settings.
    pub fn deserialize_saved<'de, D: serde::Deserializer<'de>>(
        reader: D,
    ) -> Result<Self, D::Error> {
        let mut value = serde_json::Value::deserialize(reader)?;
        if let Some(fields) = value.as_object_mut() {
            if fields.get("prediction_algorithm").is_some_and(|v| {
                !matches!(v.as_str(), Some("optimized"))
            }) {
                fields.remove("prediction_algorithm");
            }
            fields.remove("panel_text_pt");
            fields.remove("zen_hide");
            fields.remove("zen_reveal");
            fields.remove("zen_behavior");
            fields.remove("zen_reveal_mode");
            fields.remove("zen_show_button");
            fields.remove("total_zen");
            if let Some(shortcuts) = fields.get_mut("shortcuts").and_then(|v| v.as_object_mut()) {
                shortcuts.remove("command.TogglePanels");
            }
        }
        serde_json::from_value(value).map_err(serde::de::Error::custom)
    }
    pub fn validate(&self) -> Result<(), String> {
        for marks in self.slider_bookmarks.values() {
            marks.validate()?;
        }
        self.new_document.validate()?;
        if self.version != 1 {
            return Err("Unsupported settings version".into());
        }
        for row in self
            .pages(Platform::Gtk)
            .into_iter()
            .flat_map(|p| p.groups)
            .flat_map(|g| g.rows)
        {
            if let PreferenceKind::Number { control, value } = row.kind {
                control.validate(value, &row.title)?;
            }
        }
        // Continue validating the retained legacy value without applying it.
        layer_engine::InstantFeedbackConfig {
            tip_lock: self.tip_lock,
            ..self.feedback_config()
        }
        .validate()
        .map_err(|e| e.to_string())?;
        self.validate_shortcuts()
    }
    pub(crate) fn feedback_config(&self) -> layer_engine::InstantFeedbackConfig {
        layer_engine::InstantFeedbackConfig {
            enabled: self.feedback,
            use_platform_prediction: self.platform_prediction,
            prediction_algorithm: self.prediction_algorithm,
            prediction_horizon_micros: (self.prediction_ms * 1000.0).round() as u32,
            // Always track the predicted endpoint at full strength. Prediction
            // time alone controls how far ahead the preview should reach.
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
    NewColorSpace,
    NewBitDepth,
    NewBackground,
    PhotoDepth,
    MissingProfile,
    Theme,
    ShowClock,
    /// Retired preference ID, retained to decode saved custom actions.
    TotalZen,
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
    Pressure,
    Feedback,
    PlatformPrediction,
    PredictionHorizon,
    PredictionAlgorithm,
    /// Retired preference ID, retained to decode old serialized actions.
    TipLock,
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
            Self::Theme => "theme",
            Self::ShowClock => "show-clock",
            Self::TotalZen => "total-zen",
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
            Self::Pressure => "pressure",
            Self::Feedback => "feedback",
            Self::PlatformPrediction => "platform-prediction",
            Self::PredictionHorizon => "prediction-horizon",
            Self::PredictionAlgorithm => "prediction-algorithm",
            Self::TipLock => "tip-lock",
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChoicePresentation {
    Dropdown,
    ImageTiles { columns: u32 },
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PreferenceKind {
    Text {
        value: String,
        constraint: TextConstraint,
        max_length: u32,
        placeholder: String,
    },
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
            Self::Text { value, .. } => PreferenceValue::Text(value.clone()),
            Self::Choice { selected, .. } => PreferenceValue::Choice(*selected),
            Self::Swatches { value, .. } => PreferenceValue::Text(value.clone()),
            Self::Switch { active } => PreferenceValue::Bool(*active),
            Self::Info { .. } | Self::Link { .. } => return None,
        })
    }

    fn display_value(&self) -> String {
        match self {
            Self::Number { value, control } => {
                control
                    .resolve(*value as f64, NumericOperation::Format)
                    .expect("valid setting default")
                    .text
            }
            Self::Text { value, .. } => value.clone(),
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
            Self::Switch { active } => if *active { "On" } else { "Off" }.into(),
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

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextConstraint {
    HexColor,
}
impl TextConstraint {
    fn validate(self, text: &str) -> Result<(), String> {
        match self {
            Self::HexColor => HexColor::try_from(text.to_owned()).map(|_| ()),
        }
    }
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
}
#[derive(Clone, Debug, Serialize)]
pub struct PreferencesView {
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
    pub shortcuts: Vec<ShortcutRow>,
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
#[derive(Clone, Debug, Serialize)]
pub struct ShortcutEditor {
    pub id: String,
    pub label: String,
    pub group: String,
    pub bindings: Vec<String>,
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
    RegisterAction {
        definition: ShortcutDefinition,
    },
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
) -> PreferenceRow {
    let preset = value.and_then(|value| {
        swatches
            .iter()
            .position(|s| !s.value.is_empty() && s.color == Some(value))
    });
    swatches.push(Swatch {
        custom: true,
        ..Swatch::new("Custom", String::new(), value.filter(|_| preset.is_none()), Some("pencil"))
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
fn number(
    id: PreferenceId,
    title: &str,
    description: &str,
    value: f32,
    min: f64,
    max: f64,
    step: f64,
) -> PreferenceRow {
    row(
        id,
        title,
        description,
        PreferenceKind::Number {
            value,
            control: match id {
                PreferenceId::PredictionHorizon => NumericControl {
                    kind: NumericKind::Slider,
                    ..NumericControl::number(min, max, step, 0).unit("ms")
                },
                _ => {
                    NumericControl::number(min, max, step, if step < 1.0 { 2 } else { 0 }).unit("×")
                }
            },
        },
    )
}
impl Settings {
    pub(crate) fn pages(&self, platform: Platform) -> Vec<PreferencePage> {
        let mut pages = self.raw_pages(platform);
        let defaults = Self::default().raw_pages(platform);
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
            let value = default.kind.display_value();
            let shortcut = self.action_shortcut(
                &UiAction::Preferences {
                    action: PreferenceAction::Reset { id: row.id },
                },
                platform,
            );
            row.reset = Some(PreferenceReset {
                label: "Reset to Default".into(),
                hint: if shortcut.is_empty() {
                    value.clone()
                } else {
                    format!("{value} · {shortcut}")
                },
                value,
                enabled: row.enabled && row.kind.value().as_ref() != Some(&default_value),
            });
            match (&mut row.kind, default_value) {
                (PreferenceKind::Number { control, .. }, PreferenceValue::Number(value)) => {
                    control.default_value = Some(value as f64);
                }
                (PreferenceKind::Text { placeholder, .. }, PreferenceValue::Text(value)) => {
                    *placeholder = value;
                }
                _ => {}
            }
        }
        pages
    }

    fn raw_pages(&self, platform: Platform) -> Vec<PreferencePage> {
        use PreferenceId::*;
        let mut input = vec![
            row(
                Pressure,
                "Pressure response",
                "Lower values make light pen pressure stronger.",
                PreferenceKind::Number {
                    control: NumericControl::pressure(),
                    value: self.pressure_gamma,
                },
            ),
            row(
                Feedback,
                "Enable stroke prediction",
                "Reduce the gap between your pen and the stroke.",
                PreferenceKind::Switch {
                    active: self.feedback,
                },
            ),
            row(
                PlatformPrediction,
                match platform {
                    Platform::Android => "Use Android stroke prediction",
                    Platform::Ios => "Use iPadOS stroke prediction",
                    Platform::Web => "Use browser stroke prediction",
                    Platform::Windows => "Use Windows stroke prediction",
                    Platform::Mac => "Use macOS stroke prediction",
                    Platform::Gtk => "Use Linux stroke prediction",
                    Platform::Generic => "Use native stroke prediction",
                },
                "",
                PreferenceKind::Switch {
                    active: self.platform_prediction,
                },
            ),
            number(
                PredictionHorizon,
                "Prediction amount",
                "",
                self.prediction_ms,
                0.0,
                64.0,
                1.0,
            ),
            row(
                PredictionAlgorithm,
                "Prediction algorithm",
                "",
                PreferenceKind::Choice {
                    presentation: ChoicePresentation::Dropdown,
                    icons: Vec::new(),
                    options: PREDICTION_ALGORITHMS.iter().map(|c| c.1.into()).collect(),
                    selected: PREDICTION_ALGORITHMS.iter().position(|c| c.0 == self.prediction_algorithm).unwrap() as u32,
                },
            ),
        ];
        for r in &mut input {
            if matches!(
                r.id,
                PredictionHorizon | PredictionAlgorithm | PlatformPrediction
            ) {
                r.enabled = self.feedback;
            }
        }
        let mut groups = vec![
            vec![PreferenceGroup {
                title: "Interface".into(),
                rows: vec![
                    row(
                        Theme,
                        "Color theme",
                        "",
                        PreferenceKind::Choice {
                            presentation: ChoicePresentation::Dropdown,
                            icons: Vec::new(),
                            options: vec!["System".into(), "Light".into(), "Dark".into()],
                            selected: match self.theme {
                                None => 0,
                                Some(crate::Theme::Light) => 1,
                                Some(crate::Theme::Dark) => 2,
                            },
                        },
                    ),
                    row(
                        ShowClock,
                        "Show battery and clock",
                        "",
                        PreferenceKind::Choice {
                            presentation: ChoicePresentation::Dropdown,
                            icons: Vec::new(),
                            options: ClockVisibility::CHOICES
                                .iter()
                                .map(|c| c.1.into())
                                .collect(),
                            selected: ClockVisibility::CHOICES
                                .iter()
                                .position(|c| c.0 == self.show_clock)
                                .unwrap() as u32,
                        },
                    ),
                    self.base_row(crate::Theme::Dark, platform),
                    self.base_row(crate::Theme::Light, platform),
                ],
            }],
            vec![
                PreferenceGroup {
                    title: "Navigation".into(),
                    rows: vec![
                        number(
                            PanSpeed,
                            "Scroll pan speed",
                            "",
                            self.pan_speed,
                            0.25,
                            4.0,
                            0.05,
                        ),
                        number(
                            ZoomSpeed,
                            "Scroll zoom speed",
                            "",
                            self.zoom_speed,
                            0.25,
                            4.0,
                            0.05,
                        ),
                    ],
                },
            ],
            vec![
                PreferenceGroup {
                    title: "Pointer".into(),
                    rows: vec![
                        row(
                            Cursor,
                            "Cursor shape",
                            "",
                            PreferenceKind::Choice {
                                presentation: ChoicePresentation::Dropdown,
                                options: CursorMode::CHOICES.iter().map(|c| c.1.into()).collect(),
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
                            "Hide cursor when painting",
                            "",
                            PreferenceKind::Switch {
                                active: self.hide_cursor_while_drawing,
                            },
                        ),
                    ],
                },
                PreferenceGroup {
                    title: "Pen response".into(),
                    rows: input,
                },
            ],
            Vec::new(),
            vec![PreferenceGroup {
                title: APP_NAME.into(),
                rows: vec![
                    row(
                        Version,
                        "Version",
                        "",
                        PreferenceKind::Info {
                            value: env!("CARGO_PKG_VERSION").into(),
                        },
                    ),
                    row(
                        License,
                        "Application license",
                        "Branding and dependencies have separate licenses.",
                        PreferenceKind::Info {
                            value: env!("CARGO_PKG_LICENSE").into(),
                        },
                    ),
                    row(
                        Renderer,
                        "Canvas rendering",
                        "",
                        PreferenceKind::Info {
                            value: match platform {
                                Platform::Gtk => "Vulkan · Wayland",
                                Platform::Web => "WebGPU",
                                _ => "Native GPU",
                            }
                            .into(),
                        },
                    ),
                    row(
                        Website,
                        crate::ApplicationLink::Website.label(),
                        "",
                        PreferenceKind::Link {
                            label: crate::ApplicationLink::Website.display().into(),
                            url: crate::ApplicationLink::Website.url().into(),
                        },
                    ),
                    row(
                        SourceCode,
                        crate::ApplicationLink::SourceCode.label(),
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
            title: CommandId::ZenMode.label().into(),
            rows: vec![
                row(
                    PreferenceId::ZenShowCapy,
                    "Show Capy in Zen mode",
                    "",
                    PreferenceKind::Switch {
                        active: self.zen_show_capy,
                    },
                ),
                row(
                    PreferenceId::ZenRevealAtEdges,
                    "Reveal panels near screen edges",
                    "",
                    PreferenceKind::Switch {
                        active: self.zen_reveal_at_edges,
                    },
                ),
                row(
                    PreferenceId::ZenIcon,
                    "Button icon",
                    "",
                    PreferenceKind::Choice {
                        presentation: ChoicePresentation::ImageTiles { columns: 4 },
                        options: crate::ZenIcon::CHOICES.iter().map(|c| c.1.into()).collect(),
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
        if crate::CommandId::CustomizeWorkspaceUi.available_on(platform) {
            // Clock/battery visibility belongs to each workspace's window
            // bar. Keep the legacy preference for hosts with the older chrome.
            for group in &mut groups[0] {
                group.rows.retain(|row| row.id != ShowClock);
            }
        }
        if matches!(platform, Platform::Gtk) {
            let rows = &mut groups[0][0].rows;
            let light = rows.iter().position(|r| r.id == LightBase).unwrap();
            rows.insert(light + 1, self.accent_row(platform));
        }
        groups.insert(2, self.color_groups(platform));
        SettingsPage::ALL
            .into_iter()
            .zip(groups)
            .filter(|(id, _)| *id != SettingsPage::Color || matches!(platform, Platform::Gtk | Platform::Web | Platform::Android | Platform::Ios | Platform::Mac | Platform::Windows))
            .map(|(id, groups)| PreferencePage {
                id,
                title: id.title().into(),
                icon: id.icon().into(),
                groups,
            })
            .collect()
    }
    fn accent_row(&self, platform: Platform) -> PreferenceRow {
        let presets = platform
            .system_accent()
            .then(|| Swatch::new("System", String::new(), Some(DEFAULT_ACCENT), Some("appearance")))
            .into_iter()
            .chain(ACCENTS.iter().map(|&(label, color)| {
                Swatch::new(label, color.to_string(), Some(color), None)
            }))
            .collect();
        swatch_row(
            PreferenceId::Accent,
            "Accent color",
            presets,
            self.accent,
            self.accent.unwrap_or(DEFAULT_ACCENT),
            DEFAULT_ACCENT,
            false,
        )
    }
    fn base_row(&self, theme: crate::Theme, platform: Platform) -> PreferenceRow {
        let (id, title, value) = match theme {
            crate::Theme::Dark => (PreferenceId::DarkBase, "Dark theme base color", self.dark_base),
            crate::Theme::Light => (PreferenceId::LightBase, "Light theme base color", self.light_base),
        };
        if platform != Platform::Gtk {
            return row(
                id,
                title,
                "",
                PreferenceKind::Text {
                    value: value.to_string(),
                    constraint: TextConstraint::HexColor,
                    max_length: 7,
                    placeholder: theme.default_base().to_string(),
                },
            );
        }
        let presets = theme
            .base_choices()
            .iter()
            .map(|&c| Swatch::new(&c.to_string(), c.to_string(), Some(c), None))
            .collect();
        swatch_row(id, title, presets, Some(value), value, theme.default_base(), true)
    }
    pub(crate) fn zen_menu(&self, _platform: Platform) -> Result<ContextMenu, String> {
        Ok(ContextMenu {
            title: CommandId::ZenMode.label().into(),
            sections: vec![vec![ContextMenuItem::command(
                "Change icon…",
                UiAction::Preferences {
                    action: PreferenceAction::Reveal {
                        id: PreferenceId::ZenIcon,
                    },
                },
            )]],
        })
    }

    fn field(&self, id: PreferenceId, platform: Platform) -> Result<PreferenceRow, String> {
        self.pages(platform)
            .into_iter()
            .flat_map(|p| p.groups)
            .flat_map(|g| g.rows)
            .find(|r| r.id == id)
            .ok_or_else(|| "This setting isn't available on this device.".into())
    }
    fn edit(
        &mut self,
        id: PreferenceId,
        value: PreferenceValue,
        platform: Platform,
    ) -> Result<(), String> {
        let field = self.field(id, platform)?;
        if !field.enabled {
            return Err("Enable stroke prediction to change this setting.".into());
        }
        let value = match (&field.kind, value) {
            (PreferenceKind::Number { .. }, PreferenceValue::Text(text))
                if text.trim().is_empty() =>
            {
                self.default_value(id, platform)?
            }
            (
                PreferenceKind::Text {
                    constraint: TextConstraint::HexColor,
                    ..
                }
                | PreferenceKind::Swatches { .. },
                PreferenceValue::Text(text),
            ) if text.trim().is_empty() => self.default_value(id, platform)?,
            (_, value) => value,
        };
        use PreferenceId::*;
        match (&field.kind, &value) {
            (PreferenceKind::Number { control, .. }, _) => {
                control.validate(value.number().ok_or("Expected a number")?, &field.title)?
            }
            (PreferenceKind::Choice { options, .. }, _)
                if value.choice().is_some_and(|v| (v as usize) < options.len()) => {}
            (PreferenceKind::Switch { .. }, PreferenceValue::Bool(_)) => {}
            (PreferenceKind::Text { constraint, .. }, PreferenceValue::Text(text)) => {
                constraint.validate(text)?;
            }
            (PreferenceKind::Swatches { .. }, PreferenceValue::Text(text)) => {
                if !text.trim().is_empty() {
                    HexColor::try_from(text.trim().to_owned())?;
                }
            }
            _ => return Err("Invalid setting value".into()),
        }
        let n = value.number().unwrap_or(0.0);
        match id {
            NewColorSpace | NewBitDepth | NewBackground | PhotoDepth | MissingProfile => self.edit_color(id, value.choice().unwrap()),
            Theme => {
                self.theme = match value.choice().unwrap() {
                    1 => Some(crate::Theme::Light),
                    2 => Some(crate::Theme::Dark),
                    _ => None,
                }
            }
            ShowClock => {
                self.show_clock = ClockVisibility::CHOICES[value.choice().unwrap() as usize].0
            }
            Cursor => self.cursor = CursorMode::CHOICES[value.choice().unwrap() as usize].0,
            HideCursorWhileDrawing => {
                self.hide_cursor_while_drawing = matches!(value, PreferenceValue::Bool(true))
            }
            TotalZen => return Err("Zen mode no longer has a partial mode.".into()),
            ZenIcon => self.zen_icon = crate::ZenIcon::CHOICES[value.choice().unwrap() as usize].0,
            ZenShowCapy => self.zen_show_capy = matches!(value, PreferenceValue::Bool(true)),
            ZenRevealAtEdges => {
                self.zen_reveal_at_edges = matches!(value, PreferenceValue::Bool(true))
            }
            DarkBase | LightBase => {
                let PreferenceValue::Text(text) = value else {
                    unreachable!()
                };
                let color = HexColor::try_from(text.trim().to_owned())?;
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
            PredictionHorizon => self.prediction_ms = n,
            PredictionAlgorithm => self.prediction_algorithm = PREDICTION_ALGORITHMS[n as usize].0,

            TipLock => return Err("Pen tip tracking is automatic.".into()),
            Feedback => self.feedback = matches!(value, PreferenceValue::Bool(true)),
            PlatformPrediction => {
                self.platform_prediction = matches!(value, PreferenceValue::Bool(true))
            }
            Version | License | Renderer | Website | SourceCode => {
                return Err("This information is read-only".into());
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
            .raw_pages(platform)
            .into_iter()
            .flat_map(|p| p.groups)
            .flat_map(|g| g.rows)
            .find(|r| r.id == id)
            .and_then(|r| r.kind.value())
            .ok_or_else(|| "This setting cannot be reset.".into())
    }
}
impl PreferencesState {
    pub(crate) fn view(
        &self,
        settings: &Settings,
        platform: Platform,
        platform_prediction_available: bool,
        system_accent: Option<HexColor>,
    ) -> PreferencesView {
        let query = self.query.trim().to_lowercase();
        let mut pages = settings.pages(platform);
        for row in pages
            .iter_mut()
            .flat_map(|p| &mut p.groups)
            .flat_map(|g| &mut g.rows)
        {
            if row.id == PreferenceId::PlatformPrediction && !platform_prediction_available {
                row.enabled = false;
                row.kind = PreferenceKind::Switch { active: false };
            }
            if matches!(row.id, PreferenceId::PredictionHorizon | PreferenceId::PredictionAlgorithm)
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
                    if row.visible
                        && !query.is_empty()
                        && format!(
                            "{} {} {} {} {}",
                            page.title, group.title, row.title, row.description, value
                        )
                        .to_lowercase()
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
        let shortcut_query = self.shortcut_query.trim().to_lowercase();
        let shortcuts: Vec<_> = crate::shortcuts::definitions(settings, platform)
            .into_iter()
            .map(|(definition, group)| {
                let mut shortcut = settings.shortcut_label(&definition.id, platform);
                if shortcut.is_empty() {
                    shortcut = "Disabled".into();
                }
                ShortcutRow {
                    visible: format!("{group} {} {shortcut}", definition.label)
                        .to_lowercase()
                        .contains(&shortcut_query),
                    modified: settings.shortcut_modified(&definition.id),
                    shortcut,
                    id: definition.id,
                    label: definition.label,
                    group: group.into(),
                }
            })
            .collect();
        for row in &shortcuts {
            if !query.is_empty()
                && format!(
                    "keyboard shortcuts {} {} {}",
                    row.group, row.label, row.shortcut
                )
                .to_lowercase()
                .contains(&query)
            {
                search_results.push(PreferenceSearchResult {
                    title: row.label.clone(),
                    description: SettingsPage::Shortcuts.title().into(),
                    action: PreferenceAction::EditShortcut { id: row.id.clone() },
                });
            }
        }
        let shortcut_editor = self.editing_shortcut.as_ref().and_then(|id| {
            let row = shortcuts.iter().find(|r| &r.id == id)?;
            let bindings: Vec<_> = settings
                .keys(id)
                .iter()
                .map(|k| k.label(platform))
                .collect();
            Some(ShortcutEditor {
                id: id.clone(),
                label: row.label.clone(),
                group: row.group.clone(),
                can_add: bindings.len() < crate::shortcuts::MAX_SHORTCUTS,
                bindings,
                defaults: crate::shortcuts::defaults(id)
                    .iter()
                    .map(|k| k.label(platform))
                    .collect(),
                modified: row.modified,
            })
        });
        PreferencesView {
            text_edit_menu: crate::shortcuts::text_edit_menu(platform),
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
            shortcuts,
            capture: self.capture.clone().map(|mut c| {
                if self.error.is_some() {
                    c.error = self.error.clone();
                }
                c.notice = c.error.clone().unwrap_or_else(|| {
                    c.conflict.as_ref().map_or_else(String::new, |label| {
                        format!("Replace the shortcut used by {label}?")
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
    ) {
        self.error = self.try_edit(settings, action, platform).err();
    }
    fn try_edit(
        &mut self,
        settings: &mut Settings,
        action: PreferenceAction,
        platform: Platform,
    ) -> Result<(), String> {
        match action {
            PreferenceAction::Reveal { id } => {
                let page = settings
                    .pages(platform)
                    .into_iter()
                    .find(|p| p.groups.iter().flat_map(|g| &g.rows).any(|r| r.id == id))
                    .ok_or("This setting isn't available on this device.")?
                    .id;
                self.try_edit(settings, PreferenceAction::Page { page }, platform)?;
                self.reveal = Some(id);
            }
            PreferenceAction::Page { page } => {
                self.page = page;
                self.reveal = None;
                self.query.clear();
                self.searching = false;
                self.editing_shortcut = None;
                self.capture = None;
            }
            PreferenceAction::Search { query } => {
                if query.len() > 256 {
                    return Err("Search is too long".into());
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
                    return Err("Search is too long".into());
                }
                self.shortcut_query = query;
            }
            PreferenceAction::Edit { id, value } => settings.edit(id, value, platform)?,
            PreferenceAction::Reset { id } => {
                settings.edit(id, settings.default_value(id, platform)?, platform)?;
            }
            PreferenceAction::EditShortcut { id } => {
                if !crate::shortcuts::definitions(settings, platform)
                    .iter()
                    .any(|(d, _)| d.id == id)
                {
                    return Err("Unknown shortcut action".into());
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
                if self.editing_shortcut.as_ref() != Some(&id) {
                    return Err("Shortcut editor is not open".into());
                }
                let mut keys = settings.keys(&id);
                if index >= keys.len() {
                    return Err("Unknown shortcut binding".into());
                }
                keys.remove(index);
                settings.shortcuts.insert(id, keys);
            }
            PreferenceAction::BeginShortcut { id } => {
                let (definition, _) = crate::shortcuts::definitions(settings, platform)
                    .into_iter()
                    .find(|(d, _)| d.id == id)
                    .ok_or("Unknown shortcut action")?;
                if settings.keys(&id).len() >= crate::shortcuts::MAX_SHORTCUTS {
                    return Err("Remove a shortcut before adding another".into());
                }
                self.capture = Some(ShortcutCapture {
                    id,
                    label: definition.label,
                    chord: None,
                    shortcut: "Press a new key combination".into(),
                    conflict: None,
                    error: None,
                    notice: String::new(),
                });
            }
            PreferenceAction::CancelShortcut => self.capture = None,
            PreferenceAction::ConfirmShortcut { replace } => {
                let capture = self
                    .capture
                    .as_ref()
                    .ok_or("No shortcut is being recorded")?;
                let chord = capture
                    .chord
                    .clone()
                    .ok_or("Press a key combination first")?;
                chord.validate()?;
                if !chord.available(platform) {
                    return Err("This shortcut is reserved by the browser".into());
                }
                let mut keys = settings.keys(&capture.id);
                if keys.contains(&chord) {
                    return Err("This shortcut is already assigned to this action".into());
                }
                if keys.len() >= crate::shortcuts::MAX_SHORTCUTS {
                    return Err("Remove a shortcut before adding another".into());
                }
                if let Some(conflict) = settings.conflict(&capture.id, &chord, platform) {
                    if !replace {
                        return Err(format!("Already assigned to {}", conflict.label));
                    }
                    let keys = settings
                        .keys(&conflict.id)
                        .into_iter()
                        .filter(|c| *c != chord)
                        .collect();
                    settings.shortcuts.insert(conflict.id, keys);
                }
                keys.push(chord);
                settings.shortcuts.insert(capture.id.clone(), keys);
                self.capture = None;
            }
            PreferenceAction::ResetShortcut { id } => {
                if !crate::shortcuts::definitions(settings, platform)
                    .iter()
                    .any(|(d, _)| d.id == id)
                {
                    return Err("Unknown shortcut action".into());
                }
                for chord in crate::shortcuts::defaults(&id) {
                    if let Some(conflict) = settings.conflict(&id, &chord, platform) {
                        return Err(format!("Remove {}'s shortcut first.", conflict.label));
                    }
                }
                settings.shortcuts.remove(&id);
            }
            PreferenceAction::ResetAllShortcuts => {
                settings.shortcuts.clear();
                self.capture = None;
            }
            PreferenceAction::RegisterAction { definition } => {
                let mut candidate = settings.clone();
                candidate.custom_actions.retain(|a| a.id != definition.id);
                candidate.custom_actions.push(definition);
                candidate.validate()?;
                *settings = candidate;
            }
        }
        Ok(())
    }
    pub(crate) fn record(&mut self, settings: &Settings, chord: KeyChord, platform: Platform) {
        self.error = None;
        if KeyChord::modifier(&chord.key) {
            return;
        }
        if chord.key == "escape" {
            self.capture = None;
            return;
        }
        if let Some(capture) = &mut self.capture {
            capture.error = chord.validate().err().or_else(|| {
                (!chord.available(platform))
                    .then(|| "This shortcut is reserved by the browser".into())
            });
            capture.conflict = settings
                .conflict(&capture.id, &chord, platform)
                .map(|d| d.label);
            capture.shortcut = chord.label(platform);
            capture.chord = capture.error.is_none().then_some(chord);
        }
    }
}

#[cfg(test)]
mod copy_tests {
    use super::*;

    fn accent_row(settings: &Settings, system: Option<HexColor>) -> (Vec<Swatch>, u32, String) {
        let row = PreferencesState::default()
            .view(settings, Platform::Gtk, false, system)
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
            state.edit(settings, action, Platform::Gtk);
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
        assert!(settings.field(PreferenceId::Accent, Platform::Web).is_err());
    }

    #[test]
    fn base_colors_are_inline_grey_swatches_above_the_accent_on_gtk() {
        let rows = |settings: &Settings, platform| -> Vec<PreferenceRow> {
            settings.pages(platform)[0].groups[0].rows.clone()
        };
        let ids: Vec<_> = rows(&Settings::default(), Platform::Gtk).iter().map(|r| r.id).collect();
        assert_eq!(
            ids,
            [PreferenceId::Theme, PreferenceId::DarkBase, PreferenceId::LightBase, PreferenceId::Accent]
        );
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
                state.edit(settings, PreferenceAction::Edit { id, value }, Platform::Gtk);
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
            assert!(matches!(
                rows(&settings, Platform::Web).into_iter().find(|r| r.id == id).unwrap().kind,
                PreferenceKind::Text { .. }
            ));
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
        for platform in [
            Platform::Generic,
            Platform::Gtk,
            Platform::Web,
            Platform::Windows,
            Platform::Mac,
            Platform::Ios,
            Platform::Android,
        ] {
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
                );
                assert_eq!(settings.cursor, CursorMode::BrushSize);
            }
        }
    }

    #[test]
    fn pointer_preferences_belong_to_input_on_every_platform() {
        for platform in [
            Platform::Gtk,
            Platform::Web,
            Platform::Android,
            Platform::Ios,
            Platform::Mac,
            Platform::Windows,
        ] {
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
    fn prediction_choice_round_trips_resets_and_reaches_every_platform() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Ios, Platform::Mac, Platform::Windows, Platform::Generic] {
            let mut settings = Settings::default();
            let id = PreferenceId::PredictionAlgorithm;
            assert_eq!(settings.prediction_algorithm, StrokePrediction::Optimized);
            let row = settings.field(id, platform).unwrap();
            assert!(matches!(row.kind, PreferenceKind::Choice { presentation: ChoicePresentation::Dropdown, ref options, selected: 0, .. }
                if options == &["Smooth Motion (Optimized)"]));
            for (index, &(algorithm, _)) in PREDICTION_ALGORITHMS.iter().enumerate() {
                settings.edit(id, PreferenceValue::Choice(index as u32), platform).unwrap();
                let restored = Settings::deserialize_saved(serde_json::to_value(&settings).unwrap()).unwrap();
                assert_eq!(restored, settings);
                for native in [false, true] {
                    assert_eq!(restored.feedback_config_for(platform, native).prediction_algorithm, algorithm);
                }
            }
            assert!(settings.edit(id, PreferenceValue::Choice(1), platform).is_err());
            settings.edit(id, settings.default_value(id, platform).unwrap(), platform).unwrap();
            assert_eq!(settings.prediction_algorithm, StrokePrediction::Optimized);
            settings.feedback = false;
            assert!(settings.edit(id, PreferenceValue::Choice(0), platform).is_err());
        }
    }

    #[test]
    fn retired_prediction_choices_do_not_change_any_platform_fallback() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Ios, Platform::Mac, Platform::Windows, Platform::Generic] {
            for old in ["previous", "linear", "kalman", "trajectory", "trajectory_tapered", "trajectory_tapered_filtered", "local_acceleration", "local_acceleration_smooth"] {
                let settings = Settings::deserialize_saved(serde_json::json!({
                    "prediction_algorithm": old, "prediction_ms": 23.0, "platform_prediction": true,
                })).unwrap();
                assert_eq!(settings.prediction_ms, 23.0);
                assert_eq!(settings.prediction_algorithm, StrokePrediction::Optimized);
                let expected = Settings { prediction_ms: 23.0, platform_prediction: true, ..Default::default() };
                assert_eq!(settings.feedback_config_for(platform, false), expected.feedback_config_for(platform, false));
                let fallback = settings.feedback_config_for(platform, false);
                assert!(fallback.use_engine_prediction && !fallback.use_platform_prediction);
                assert_eq!(fallback.prediction_horizon_micros, 23_000);
                let native = settings.feedback_config_for(platform, true);
                assert!(native.use_engine_prediction && native.use_platform_prediction);
                assert!(settings.field(PreferenceId::PredictionAlgorithm, platform).is_ok());
            }
        }
    }

    #[test]
    fn clock_visibility_defaults_round_trips_and_resets() {
        let original: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(original.show_clock, ClockVisibility::Fullscreen);
        for platform in [
            Platform::Gtk,
            Platform::Web,
            Platform::Android,
            Platform::Ios,
            Platform::Mac,
            Platform::Windows,
        ] {
            assert!(original.field(PreferenceId::ShowClock, platform).is_err());
        }
        {
            let platform = Platform::Generic;
            let mut settings = original.clone();
            let row = settings.field(PreferenceId::ShowClock, platform).unwrap();
            assert_eq!(row.title, "Show battery and clock");
            assert!(matches!(
                row.kind,
                PreferenceKind::Choice { selected: 0, .. }
            ));
            for (index, policy) in [
                ClockVisibility::Fullscreen,
                ClockVisibility::Always,
                ClockVisibility::Never,
            ]
            .into_iter()
            .enumerate()
            {
                settings
                    .edit(
                        PreferenceId::ShowClock,
                        PreferenceValue::Choice(index as u32),
                        platform,
                    )
                    .unwrap();
                let saved = serde_json::to_string(&settings).unwrap();
                let restored =
                    Settings::deserialize_saved(&mut serde_json::Deserializer::from_str(&saved))
                        .unwrap();
                assert_eq!(restored.show_clock, policy);
                assert_eq!(policy.visible(false), policy == ClockVisibility::Always);
                assert_eq!(policy.visible(true), policy != ClockVisibility::Never);
            }
            assert!(
                settings
                    .edit(
                        PreferenceId::ShowClock,
                        PreferenceValue::Choice(3),
                        platform
                    )
                    .is_err()
            );
            let mut preferences = PreferencesState::default();
            preferences.edit(
                &mut settings,
                PreferenceAction::Reset {
                    id: PreferenceId::ShowClock,
                },
                platform,
            );
            assert_eq!(settings, original);
        }
    }

    #[test]
    fn zen_preferences_roundtrip_reset_and_retire_legacy_mode() {
        for platform in [
            Platform::Generic,
            Platform::Gtk,
            Platform::Web,
            Platform::Android,
            Platform::Mac,
            Platform::Ios,
            Platform::Windows,
        ] {
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
            assert!(settings.field(PreferenceId::TotalZen, platform).is_err());
            assert!(
                settings
                    .edit(
                        PreferenceId::TotalZen,
                        PreferenceValue::Bool(false),
                        platform
                    )
                    .is_err()
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
                preferences.edit(&mut settings, PreferenceAction::Reset { id }, platform);
                assert!(preferences.error.is_none());
                assert_eq!(settings, Settings::default());
            }
            settings
                .edit(PreferenceId::ZenIcon, PreferenceValue::Choice(3), platform)
                .unwrap();
            let saved = serde_json::to_string(&settings).unwrap();
            assert!(!saved.contains("total_zen"));
            assert_eq!(serde_json::from_str::<Settings>(&saved).unwrap(), settings);
            let mut state = PreferencesState::default();
            state.edit(
                &mut settings,
                PreferenceAction::Reset {
                    id: PreferenceId::ZenIcon,
                },
                platform,
            );
            assert!(state.error.is_none());
            assert_eq!(settings, Settings::default());
        }
        for total in [false, true] {
            let json = format!(
                r#"{{"total_zen":{total},"zen_reveal_mode":"button","zen_show_button":false,"pan_speed":2.0}}"#
            );
            let mut reader = serde_json::Deserializer::from_str(&json);
            let settings = Settings::deserialize_saved(&mut reader).unwrap();
            assert_eq!(settings.pan_speed, 2.0);
            assert!(
                !serde_json::to_string(&settings)
                    .unwrap()
                    .contains("total_zen")
            );
        }
    }

    #[test]
    fn reset_metadata_and_empty_commits_share_schema_defaults() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut settings = Settings::default();
            for row in settings
                .pages(platform)
                .into_iter()
                .flat_map(|p| p.groups)
                .flat_map(|g| g.rows)
            {
                assert_eq!(row.reset.is_some(), row.kind.value().is_some());
                if let Some(reset) = row.reset {
                    assert!(!reset.enabled);
                    assert!(!reset.value.is_empty());
                }
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
                state.edit(&mut settings, PreferenceAction::Reset { id }, platform);
                assert!(state.error.is_none());
                assert!(!settings.field(id, platform).unwrap().reset.unwrap().enabled);
            }
            let mut state = PreferencesState::default();
            for (id, value) in [
                (PreferenceId::Theme, PreferenceValue::Choice(2)),
                (PreferenceId::Feedback, PreferenceValue::Bool(false)),
            ] {
                settings.edit(id, value, platform).unwrap();
                state.edit(&mut settings, PreferenceAction::Reset { id }, platform);
                assert!(state.error.is_none());
            }
            assert_eq!(settings, Settings::default());
            state.edit(
                &mut settings,
                PreferenceAction::Reset {
                    id: PreferenceId::Version,
                },
                platform,
            );
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
        for platform in [
            Platform::Generic,
            Platform::Gtk,
            Platform::Web,
            Platform::Windows,
            Platform::Mac,
            Platform::Ios,
            Platform::Android,
        ] {
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
            for (definition, group) in crate::shortcuts::definitions(&settings, platform) {
                check(&definition.label);
                check(group);
                let mut state = PreferencesState::default();
                state.edit(
                    &mut settings.clone(),
                    PreferenceAction::BeginShortcut { id: definition.id },
                    platform,
                );
                check(&state.capture.as_ref().unwrap().shortcut);
                state.capture.as_mut().unwrap().conflict = Some(definition.label);
                check(
                    &state
                        .view(&settings, platform, true, None)
                        .capture
                        .unwrap()
                        .notice,
                );
                state.error = Some("Choose another key for this shortcut.".into());
                assert_eq!(
                    state
                        .view(&settings, platform, true, None)
                        .capture
                        .unwrap()
                        .notice,
                    state.error.unwrap()
                );
            }
        }
    }
}
