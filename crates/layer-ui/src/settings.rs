//! Portable preferences, editor state and host requests. Hosts render this model
//! and perform storage/window services; they do not decide settings policy.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    pub fn apple(self) -> bool {
        matches!(self, Self::Mac | Self::Ios)
    }
    pub fn native_windows(self) -> bool {
        matches!(self, Self::Gtk | Self::Windows | Self::Mac)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZenRevealMode {
    #[default]
    #[serde(alias = "reveal_at_edges")]
    Edges,
    #[serde(alias = "button_only")]
    Button,
}
impl ZenRevealMode {
    // Compact settings value and standalone context-menu label.
    const CHOICES: [(Self, &'static str, &'static str); 2] = [
        (Self::Edges, "Screen edges", "Reveal at screen edges"),
        (Self::Button, "Zen button", "Reveal with Zen button"),
    ];
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
    pub theme: Option<Theme>,
    pub dark_base: HexColor,
    pub light_base: HexColor,
    #[serde(alias = "zen_behavior")]
    pub zen_reveal_mode: ZenRevealMode,
    pub zen_show_button: bool,
    pub zen_icon: ZenIcon,
    pub pressure_gamma: f32,
    pub cursor: CursorMode,
    pub pan_speed: f32,
    pub zoom_speed: f32,
    pub feedback: bool,
    pub platform_prediction: bool,
    pub prediction_ms: f32,
    pub tip_lock: f32,
    /// Only overrides are stored. Empty keys disable an action's shortcut.
    pub shortcuts: BTreeMap<String, Vec<KeyChord>>,
    /// Any typed UiAction can be registered, including parameterized controls.
    pub custom_actions: Vec<ShortcutDefinition>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            theme: None,
            dark_base: Theme::Dark.default_base(),
            light_base: Theme::Light.default_base(),
            zen_reveal_mode: ZenRevealMode::default(),
            zen_show_button: true,
            zen_icon: ZenIcon::default(),
            pressure_gamma: 1.0,
            cursor: CursorMode::default(),
            pan_speed: 1.0,
            zoom_speed: 1.0,
            feedback: true,
            platform_prediction: true,
            prediction_ms: 8.0,
            tip_lock: 1.0,
            shortcuts: BTreeMap::new(),
            custom_actions: Vec::new(),
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
            fields.remove("panel_text_pt");
            fields.remove("zen_hide");
            fields.remove("zen_reveal");
            if let Some(shortcuts) = fields.get_mut("shortcuts").and_then(|v| v.as_object_mut()) {
                shortcuts.remove("command.TogglePanels");
            }
        }
        serde_json::from_value(value).map_err(serde::de::Error::custom)
    }
    pub fn validate(&self) -> Result<(), String> {
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
        self.feedback_config()
            .validate()
            .map_err(|e| e.to_string())?;
        self.validate_shortcuts()
    }
    pub(crate) fn feedback_config(&self) -> layer_engine::InstantFeedbackConfig {
        layer_engine::InstantFeedbackConfig {
            enabled: self.feedback,
            use_platform_prediction: self.platform_prediction,
            prediction_horizon_micros: (self.prediction_ms * 1000.0).round() as u32,
            tip_lock: self.tip_lock,
            ..Default::default()
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsPage {
    #[default]
    Appearance,
    Canvas,
    Input,
    Shortcuts,
    About,
}
impl SettingsPage {
    pub const ALL: [Self; 5] = [
        Self::Appearance,
        Self::Canvas,
        Self::Input,
        Self::Shortcuts,
        Self::About,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::Appearance => "appearance",
            Self::Canvas => "canvas",
            Self::Input => "input",
            Self::Shortcuts => "shortcuts",
            Self::About => "about",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Canvas => "Canvas",
            Self::Input => "Pen & Input",
            Self::Shortcuts => "Keyboard Shortcuts",
            Self::About => "About",
        }
    }
    pub fn icon(self) -> &'static str {
        match self {
            Self::Appearance => "appearance",
            Self::Canvas => "fit",
            Self::Input => "brush",
            Self::Shortcuts => "keyboard",
            Self::About => "info",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreferenceId {
    Theme,
    #[serde(alias = "zen_behavior")]
    ZenRevealMode,
    ZenShowButton,
    ZenIcon,
    DarkBase,
    LightBase,
    Cursor,
    PanSpeed,
    ZoomSpeed,
    Pressure,
    Feedback,
    PlatformPrediction,
    PredictionHorizon,
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
            Self::Theme => "theme",
            Self::ZenRevealMode => "zen-reveal-mode",
            Self::ZenShowButton => "zen-show-button",
            Self::ZenIcon => "zen-icon",
            Self::DarkBase => "dark-base",
            Self::LightBase => "light-base",
            Self::Cursor => "cursor",
            Self::PanSpeed => "pan-speed",
            Self::ZoomSpeed => "zoom-speed",
            Self::Pressure => "pressure",
            Self::Feedback => "feedback",
            Self::PlatformPrediction => "platform-prediction",
            Self::PredictionHorizon => "prediction-horizon",
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
            Self::Switch { active } => if *active { "On" } else { "Off" }.into(),
            Self::Info { .. } | Self::Link { .. } => String::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct PreferenceReset {
    pub label: String,
    pub value: String,
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
    NewWindow,
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
                PreferenceId::TipLock => NumericControl::percent(),
                PreferenceId::PredictionHorizon => {
                    NumericControl::number(min, max, step, 0).unit("ms")
                }
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
            row.reset = Some(PreferenceReset {
                label: "Reset to Default".into(),
                value: default.kind.display_value(),
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
                "Live stroke preview",
                "Reduce the gap between your pen and the stroke.",
                PreferenceKind::Switch {
                    active: self.feedback,
                },
            ),
            number(
                PredictionHorizon,
                "Prediction time",
                "Lower values keep strokes from running ahead.",
                self.prediction_ms,
                0.0,
                64.0,
                1.0,
            ),
            number(
                TipLock,
                "Pen tip tracking",
                "Higher values bring the stroke closer to your pen.",
                self.tip_lock,
                0.0,
                1.0,
                0.05,
            ),
        ];
        if matches!(platform, Platform::Web | Platform::Ios | Platform::Android) {
            input.push(row(
                PlatformPrediction,
                "Device pen prediction",
                "Use your device's estimate of the next pen position.",
                PreferenceKind::Switch {
                    active: self.platform_prediction,
                },
            ));
        }
        for r in &mut input {
            if matches!(r.id, PredictionHorizon | TipLock | PlatformPrediction) {
                r.enabled = self.feedback;
            }
        }
        let mut groups = [
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
                        DarkBase,
                        "Dark theme base color",
                        "",
                        PreferenceKind::Text {
                            value: self.dark_base.to_string(),
                            constraint: TextConstraint::HexColor,
                            max_length: 7,
                            placeholder: crate::Theme::Dark.default_base().to_string(),
                        },
                    ),
                    row(
                        LightBase,
                        "Light theme base color",
                        "",
                        PreferenceKind::Text {
                            value: self.light_base.to_string(),
                            constraint: TextConstraint::HexColor,
                            max_length: 7,
                            placeholder: crate::Theme::Light.default_base().to_string(),
                        },
                    ),
                ],
            }],
            vec![
                PreferenceGroup {
                    title: "Pointer".into(),
                    rows: vec![row(
                        Cursor,
                        "Canvas cursor",
                        "",
                        PreferenceKind::Choice {
                            presentation: ChoicePresentation::Dropdown,
                            options: CursorMode::CHOICES.iter().map(|c| c.1.into()).collect(),
                            icons: [
                                "cursor-brush",
                                "cursor-brush-cross",
                                "cursor-cross",
                                "cursor-dot",
                                "cursor-none",
                            ]
                            .map(String::from)
                            .to_vec(),
                            selected: CursorMode::CHOICES
                                .iter()
                                .position(|c| c.0 == self.cursor)
                                .unwrap() as u32,
                        },
                    )],
                },
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
            vec![PreferenceGroup {
                title: "Pen response".into(),
                rows: input,
            }],
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
                        "Website",
                        "",
                        PreferenceKind::Link {
                            label: "capycanvas.art".into(),
                            url: "https://capycanvas.art/".into(),
                        },
                    ),
                    row(
                        SourceCode,
                        "Source code",
                        "",
                        PreferenceKind::Link {
                            label: "github.com/capyatelier/capycanvas".into(),
                            url: "https://github.com/capyatelier/capycanvas".into(),
                        },
                    ),
                ],
            }],
        ];
        groups[0].push(PreferenceGroup {
            title: CommandId::ZenMode.label().into(),
            rows: vec![
                row(
                    PreferenceId::ZenRevealMode,
                    "Show controls",
                    "",
                    PreferenceKind::Choice {
                        presentation: ChoicePresentation::Dropdown,
                        icons: Vec::new(),
                        options: crate::ZenRevealMode::CHOICES
                            .iter()
                            .map(|c| c.1.into())
                            .collect(),
                        selected: crate::ZenRevealMode::CHOICES
                            .iter()
                            .position(|c| c.0 == self.zen_reveal_mode)
                            .unwrap() as u32,
                    },
                ),
                row(
                    ZenShowButton,
                    "Keep Zen button visible",
                    "",
                    PreferenceKind::Switch {
                        active: self.zen_show_button,
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
    pub(crate) fn zen_menu(&self, platform: Platform) -> Result<ContextMenu, String> {
        let mut sections = Vec::new();
        for id in [PreferenceId::ZenRevealMode, PreferenceId::ZenShowButton] {
            let row = self.field(id, platform)?;
            let item = |label, value, selected| {
                let mut item = ContextMenuItem::command(
                    label,
                    UiAction::Preferences {
                        action: PreferenceAction::Edit { id, value },
                    },
                );
                item.enabled = row.enabled;
                item.selected = Some(selected);
                item
            };
            sections.push(match row.kind {
                PreferenceKind::Choice {
                    options, selected, ..
                } => options
                    .into_iter()
                    .enumerate()
                    .map(|(i, label)| {
                        let label = if id == PreferenceId::ZenRevealMode {
                            crate::ZenRevealMode::CHOICES[i].2.into()
                        } else {
                            label
                        };
                        item(
                            label,
                            PreferenceValue::Choice(i as u32),
                            i as u32 == selected,
                        )
                    })
                    .collect(),
                PreferenceKind::Switch { active } => {
                    vec![item(row.title, PreferenceValue::Bool(!active), active)]
                }
                _ => unreachable!("Zen menu fields must be choices or switches"),
            });
        }
        sections.push(vec![ContextMenuItem::command(
            "Change icon…",
            UiAction::Preferences {
                action: PreferenceAction::Reveal {
                    id: PreferenceId::ZenIcon,
                },
            },
        )]);
        Ok(ContextMenu {
            title: CommandId::ZenMode.label().into(),
            sections,
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
            return Err("Enable live stroke preview to change this setting.".into());
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
                },
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
            _ => return Err("Invalid setting value".into()),
        }
        let n = value.number().unwrap_or(0.0);
        match id {
            Theme => {
                self.theme = match value.choice().unwrap() {
                    1 => Some(crate::Theme::Light),
                    2 => Some(crate::Theme::Dark),
                    _ => None,
                }
            }
            Cursor => self.cursor = CursorMode::CHOICES[value.choice().unwrap() as usize].0,
            ZenRevealMode => {
                self.zen_reveal_mode =
                    crate::ZenRevealMode::CHOICES[value.choice().unwrap() as usize].0
            }
            ZenShowButton => self.zen_show_button = matches!(value, PreferenceValue::Bool(true)),
            ZenIcon => self.zen_icon = crate::ZenIcon::CHOICES[value.choice().unwrap() as usize].0,
            DarkBase | LightBase => {
                let PreferenceValue::Text(text) = value else {
                    unreachable!()
                };
                let color = HexColor::try_from(text)?;
                if id == DarkBase {
                    self.dark_base = color;
                } else {
                    self.light_base = color;
                }
            }
            Pressure => self.pressure_gamma = n,
            PanSpeed => self.pan_speed = n,
            ZoomSpeed => self.zoom_speed = n,
            PredictionHorizon => self.prediction_ms = n,
            TipLock => self.tip_lock = n,
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
    pub(crate) fn view(&self, settings: &Settings, platform: Platform) -> PreferencesView {
        let query = self.query.trim().to_lowercase();
        let pages = settings.pages(platform);
        let mut search_results = Vec::new();
        for page in &pages {
            for group in &page.groups {
                for row in &group.rows {
                    let value = match &row.kind {
                        PreferenceKind::Info { value } => value.as_str(),
                        PreferenceKind::Link { url, .. } => url.as_str(),
                        _ => "",
                    };
                    if !query.is_empty()
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

    #[test]
    fn zen_preferences_are_persistent_resettable_on_all_platforms() {
        let mut settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.zen_reveal_mode, ZenRevealMode::Edges);
        assert!(settings.zen_show_button);
        let group = settings
            .pages(Platform::Gtk)
            .into_iter()
            .find(|p| p.id == SettingsPage::Appearance)
            .unwrap()
            .groups
            .into_iter()
            .find(|g| g.title == "Zen mode")
            .unwrap();
        assert_eq!(
            group.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            [
                PreferenceId::ZenRevealMode,
                PreferenceId::ZenShowButton,
                PreferenceId::ZenIcon
            ]
        );
        let legacy: Settings = serde_json::from_str(r#"{"zen_behavior":"button_only"}"#).unwrap();
        assert_eq!(legacy.zen_reveal_mode, ZenRevealMode::Button);
        assert!(legacy.zen_show_button);
        let id = PreferenceId::ZenRevealMode;
        let row = settings.field(id, Platform::Gtk).unwrap();
        assert!(
            matches!(row.kind, PreferenceKind::Choice { selected: 0, options, .. }
            if options == ["Screen edges", "Zen button"])
        );
        assert!(!row.reset.unwrap().enabled);
        settings
            .edit(id, PreferenceValue::Choice(1), Platform::Gtk)
            .unwrap();
        assert_eq!(settings.zen_reveal_mode, ZenRevealMode::Button);
        let reset = settings.field(id, Platform::Gtk).unwrap().reset.unwrap();
        assert!(reset.enabled);
        assert_eq!(reset.value, "Screen edges");
        assert!(
            settings
                .edit(id, PreferenceValue::Choice(2), Platform::Gtk)
                .is_err()
        );
        assert_eq!(settings.zen_reveal_mode, ZenRevealMode::Button);
        let saved = serde_json::to_string(&settings).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&saved).unwrap(), settings);
        assert!(serde_json::from_str::<Settings>(r#"{"zen_reveal_mode":"unknown"}"#).is_err());
        for platform in [
            Platform::Generic,
            Platform::Web,
            Platform::Android,
            Platform::Mac,
            Platform::Ios,
            Platform::Windows,
        ] {
            assert!(settings.field(id, platform).is_ok());
            assert!(
                settings
                    .edit(id, PreferenceValue::Choice(1), platform)
                    .is_ok()
            );
        }
        let mut state = PreferencesState::default();
        state.edit(&mut settings, PreferenceAction::Reset { id }, Platform::Gtk);
        assert!(state.error.is_none());
        assert_eq!(settings, Settings::default());
        settings
            .edit(
                PreferenceId::ZenShowButton,
                PreferenceValue::Bool(false),
                Platform::Gtk,
            )
            .unwrap();
        let saved = serde_json::to_string(&settings).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&saved).unwrap(), settings);
        state.edit(
            &mut settings,
            PreferenceAction::Reset {
                id: PreferenceId::ZenShowButton,
            },
            Platform::Gtk,
        );
        assert_eq!(settings, Settings::default());
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
                (PreferenceId::TipLock, PreferenceValue::Number(0.5)),
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
                PreferenceId::Renderer,
                PreferenceId::ZenRevealMode,
                PreferenceId::ZenShowButton,
                PreferenceId::ZenIcon,
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
                PreferenceId::PredictionHorizon,
                PreferenceId::TipLock,
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
                check(&state.view(&settings, platform).capture.unwrap().notice);
                state.error = Some("Choose another key for this shortcut.".into());
                assert_eq!(
                    state.view(&settings, platform).capture.unwrap().notice,
                    state.error.unwrap()
                );
            }
        }
    }
}
