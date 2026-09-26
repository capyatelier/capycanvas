//! Portable preferences, editor state and host requests. Hosts render this model
//! and perform storage/window services; they do not decide settings policy.
use crate::*;
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
    pub fn canvas_bar(self) -> bool {
        matches!(self, Self::Gtk | Self::Web | Self::Android)
    }
    pub const ALL: [Self; 6] = [Self::Gtk, Self::Web, Self::Windows, Self::Mac, Self::Ios, Self::Android];
    pub fn apple(self) -> bool {
        matches!(self, Self::Mac | Self::Ios)
    }
    pub fn native_windows(self) -> bool {
        // The iOS host is an iPad app with independent native editor scenes.
        matches!(self, Self::Gtk | Self::Windows | Self::Mac | Self::Ios)
    }
    pub fn touch_gestures(self) -> bool {
        matches!(self, Self::Gtk | Self::Web | Self::Android)
    }
    pub fn pen_buttons(self) -> bool {
        matches!(self, Self::Gtk | Self::Web | Self::Android)
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
    pub feedback: bool,
    pub platform_prediction: bool,
    pub prediction_ms: f32,
    /// Only overrides are stored. Empty keys disable an action's shortcut.
    pub shortcuts: BTreeMap<String, Vec<KeyChord>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub gestures: BTreeMap<String, String>,
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
            feedback: true,
            platform_prediction: true,
            prediction_ms: 16.0,
            shortcuts: BTreeMap::new(),
            gestures: BTreeMap::new(),
            slider_bookmarks: BTreeMap::new(),
        }
    }
}
impl Settings {
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
    Pressure,
    Feedback,
    PlatformPrediction,
    PredictionHorizon,
    Version,
    License,
    Renderer,
    Website,
    SourceCode,
    TwoFingerTap,
    ThreeFingerTap,
    FourFingerTap,
    PenButton,
    PenSecondaryButton,
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
            Self::Pressure => "pressure",
            Self::Feedback => "feedback",
            Self::PlatformPrediction => "platform-prediction",
            Self::PredictionHorizon => "prediction-horizon",
            Self::Version => "version",
            Self::License => "license",
            Self::Renderer => "renderer",
            Self::Website => "website",
            Self::SourceCode => "source-code",
            Self::TwoFingerTap => "two-finger-tap",
            Self::ThreeFingerTap => "three-finger-tap",
            Self::FourFingerTap => "four-finger-tap",
            Self::PenButton => "pen-button",
            Self::PenSecondaryButton => "pen-secondary-button",
        }
    }
    fn gesture_trigger(self) -> Option<&'static GestureTrigger> {
        let id = match self {
            Self::TwoFingerTap => "touch.tap.2",
            Self::ThreeFingerTap => "touch.tap.3",
            Self::FourFingerTap => "touch.tap.4",
            Self::PenButton => "pen.button.primary",
            Self::PenSecondaryButton => "pen.button.secondary",
            _ => return None,
        };
        GESTURE_TRIGGERS.iter().find(|t| t.id == id)
    }
}
const TAP_CHOICES: [&str; 6] = [
    "",
    "command.Undo",
    "command.Redo",
    "command.Eyedropper",
    "command.SearchCommands",
    "command.ZenMode",
];
const PEN_BUTTON_CHOICES: [&str; 9] = [
    "",
    "hold.eyedropper",
    "hold.eraser",
    "canvas.pan",
    "hold.move",
    "command.Undo",
    "command.Redo",
    "command.Eyedropper",
    "command.SearchCommands",
];
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
        ];
        for r in &mut input {
            if matches!(r.id, PredictionHorizon | PlatformPrediction) {
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
                        Transparency,
                        "Panel transparency",
                        "",
                        PreferenceKind::Choice {
                            presentation: ChoicePresentation::Circles {
                                alphas: crate::Transparency::CHOICES
                                    .map(|c| [false, true].map(|dark| c.0.surface_alpha(dark))),
                            },
                            options: crate::Transparency::CHOICES
                                .iter()
                                .map(|c| c.1.into())
                                .collect(),
                            icons: Vec::new(),
                            selected: crate::Transparency::CHOICES
                                .iter()
                                .position(|c| c.0 == self.transparency)
                                .unwrap() as u32,
                        },
                    ),
                    self.base_row(crate::Theme::Dark),
                    self.base_row(crate::Theme::Light),
                    self.accent_row(platform),
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
                PreferenceGroup {
                    title: "Touch and pen buttons".into(),
                    rows: [TwoFingerTap, ThreeFingerTap, FourFingerTap, PenButton, PenSecondaryButton]
                        .into_iter()
                        .filter_map(|id| self.gesture_row(id, platform))
                        .collect(),
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
        groups.insert(2, self.color_groups());
        SettingsPage::ALL
            .into_iter()
            .zip(groups)
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
    fn base_row(&self, theme: crate::Theme) -> PreferenceRow {
        let (id, title, value) = match theme {
            crate::Theme::Dark => (PreferenceId::DarkBase, "Dark theme base color", self.dark_base),
            crate::Theme::Light => (PreferenceId::LightBase, "Light theme base color", self.light_base),
        };
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

    fn gesture_choices(&self, trigger: &GestureTrigger, platform: Platform) -> Vec<(String, String)> {
        let definitions = crate::shortcuts::definitions(platform);
        let label = |id: &str| {
            if id.is_empty() {
                Some("Nothing".to_string())
            } else {
                definitions.iter().find(|(d, _)| d.id == id).map(|(d, _)| d.label.clone())
            }
        };
        let current = self.gesture_binding(trigger.id);
        let curated = if trigger.held { &PEN_BUTTON_CHOICES[..] } else { &TAP_CHOICES[..] };
        curated
            .iter()
            .copied()
            .chain((!curated.contains(&current)).then_some(current))
            .filter_map(|id| label(id).map(|label| (id.to_string(), label)))
            .collect()
    }
    fn gesture_row(&self, id: PreferenceId, platform: Platform) -> Option<PreferenceRow> {
        let trigger = id.gesture_trigger()?;
        if !(if trigger.held { platform.pen_buttons() } else { platform.touch_gestures() }) {
            return None;
        }
        let choices = self.gesture_choices(trigger, platform);
        let current = self.gesture_binding(trigger.id);
        Some(row(
            id,
            trigger.label,
            if trigger.held { "Nothing leaves the button to the tablet driver." } else { "" },
            PreferenceKind::Choice {
                presentation: ChoicePresentation::Dropdown,
                icons: Vec::new(),
                selected: choices.iter().position(|(c, _)| c == current).unwrap_or(0) as u32,
                options: choices.into_iter().map(|(_, label)| label).collect(),
            },
        ))
    }
    pub(crate) fn field(&self, id: PreferenceId, platform: Platform) -> Result<PreferenceRow, String> {
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
            Feedback => self.feedback = matches!(value, PreferenceValue::Bool(true)),
            PlatformPrediction => {
                self.platform_prediction = matches!(value, PreferenceValue::Bool(true))
            }
            Version | License | Renderer | Website | SourceCode => {
                return Err("This information is read-only".into());
            }
            TwoFingerTap | ThreeFingerTap | FourFingerTap | PenButton | PenSecondaryButton => {
                let trigger = id.gesture_trigger().unwrap();
                let (choice, _) = self
                    .gesture_choices(trigger, platform)
                    .swap_remove(value.choice().unwrap() as usize);
                if choice == trigger.default {
                    self.gestures.remove(trigger.id);
                } else {
                    self.gestures.insert(trigger.id.into(), choice);
                }
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
        let shortcuts: Vec<_> = crate::shortcuts::definitions(platform)
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
                if !crate::shortcuts::definitions(platform)
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
                let (definition, _) = crate::shortcuts::definitions(platform)
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
                chord.validate_for(settings.held_shortcut(&capture.id, platform))?;
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
                if !crate::shortcuts::definitions(platform)
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
        }
        Ok(())
    }
    pub(crate) fn record(&mut self, settings: &Settings, chord: KeyChord, platform: Platform) {
        self.error = None;
        let held = self.capture.as_ref().is_some_and(|c| settings.held_shortcut(&c.id, platform));
        if KeyChord::modifier(&chord.key) && !(held && chord.validate_for(true).is_ok()) {
            return;
        }
        if chord.key == "escape" {
            self.capture = None;
            return;
        }
        if let Some(capture) = &mut self.capture {
            capture.error = chord.validate_for(held).err().or_else(|| {
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
        state.edit(&mut web, PreferenceAction::Edit { id: PreferenceId::Accent, value: blue }, Platform::Web);
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
                preferences.edit(&mut settings, PreferenceAction::Reset { id }, platform);
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
            );
            assert!(state.error.is_none());
            assert_eq!(settings, Settings::default());
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
