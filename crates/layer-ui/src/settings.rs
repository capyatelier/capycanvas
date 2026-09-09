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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub version: u32,
    pub theme: Option<Theme>,
    pub dark_base: HexColor,
    pub light_base: HexColor,
    pub pressure_gamma: f32,
    pub cursor: CursorMode,
    pub zen_reveal: f32,
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
            pressure_gamma: 1.0,
            cursor: CursorMode::default(),
            zen_reveal: 80.0,
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
    DarkBase,
    LightBase,
    ZenReveal,
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
            Self::DarkBase => "dark-base",
            Self::LightBase => "light-base",
            Self::ZenReveal => "zen-reveal",
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
    pub query: String,
    pub searching: bool,
    pub search_focus: u64,
    pub shortcut_query: String,
    pub editing_shortcut: Option<String>,
    pub editing_preference: Option<PreferenceId>,
    pub capture: Option<ShortcutCapture>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct PreferencesView {
    pub pages: Vec<PreferencePage>,
    pub page: SettingsPage,
    pub query: String,
    pub searching: bool,
    /// Changes when typing outside an editor should reveal and focus search.
    pub search_focus: u64,
    pub search_results: Vec<PreferenceSearchResult>,
    pub shortcut_query: String,
    pub shortcut_editor: Option<ShortcutEditor>,
    pub detail: Option<PreferenceRow>,
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
    EditPreference {
        id: PreferenceId,
    },
    ClosePreference,
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
                PreferenceId::ZenReveal => NumericControl::number(min, max, step, 0).unit("px"),
                _ => {
                    NumericControl::number(min, max, step, if step < 1.0 { 2 } else { 0 }).unit("×")
                }
            },
        },
    )
}
impl Settings {
    pub(crate) fn pages(&self, platform: Platform) -> Vec<PreferencePage> {
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
        let groups = [
            vec![
                PreferenceGroup {
                    title: "Interface".into(),
                    rows: vec![
                        row(
                            Theme,
                            "Color theme",
                            "Match your system theme, or choose light or dark.",
                            PreferenceKind::Choice {
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
                            "Dark base color",
                            "Set the dark theme's base color with #RRGGBB.",
                            PreferenceKind::Text {
                                value: self.dark_base.to_string(),
                                constraint: TextConstraint::HexColor,
                                max_length: 7,
                                placeholder: crate::Theme::Dark.default_base().to_string(),
                            },
                        ),
                        row(
                            LightBase,
                            "Light base color",
                            "Set the light theme's base color with #RRGGBB.",
                            PreferenceKind::Text {
                                value: self.light_base.to_string(),
                                constraint: TextConstraint::HexColor,
                                max_length: 7,
                                placeholder: crate::Theme::Light.default_base().to_string(),
                            },
                        ),
                    ],
                },
                PreferenceGroup {
                    title: "Zen mode".into(),
                    rows: vec![number(
                        ZenReveal,
                        "Edge reveal distance",
                        "Show controls when the pointer nears a window edge.",
                        self.zen_reveal,
                        20.0,
                        200.0,
                        1.0,
                    )],
                },
            ],
            vec![
                PreferenceGroup {
                    title: "Pointer".into(),
                    rows: vec![row(
                        Cursor,
                        "Canvas cursor",
                        "Choose how the pointer looks over the canvas.",
                        PreferenceKind::Choice {
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
                            "Set how far scrolling moves the canvas.",
                            self.pan_speed,
                            0.25,
                            4.0,
                            0.05,
                        ),
                        number(
                            ZoomSpeed,
                            "Scroll zoom speed",
                            "Set how much each scroll changes the zoom.",
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
                        "Your GPU draws and displays the canvas.",
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
            ZenReveal => self.zen_reveal = n,
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
}
impl PreferencesState {
    pub(crate) fn view(&self, settings: &Settings, platform: Platform) -> PreferencesView {
        let query = self.query.trim().to_lowercase();
        let pages = settings.pages(platform);
        let detail = self.editing_preference.and_then(|id| {
            pages
                .iter()
                .flat_map(|p| &p.groups)
                .flat_map(|g| &g.rows)
                .find(|r| r.id == id)
                .cloned()
        });
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
            .map(|(definition, group)| ShortcutRow {
                shortcut: settings.shortcut_label(&definition.id, platform),
                modified: settings.shortcuts.contains_key(&definition.id),
                visible: format!("{group} {}", definition.label)
                    .to_lowercase()
                    .contains(&shortcut_query),
                id: definition.id,
                label: definition.label,
                group: group.into(),
            })
            .collect();
        for row in &shortcuts {
            if !query.is_empty()
                && format!("keyboard shortcuts {} {}", row.group, row.label)
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
            query: self.query.clone(),
            // Android keeps its search field visible; an empty query shows
            // categories. Desktop/web retain their explicit search toggle.
            searching: self.searching && (platform != Platform::Android || !query.is_empty()),
            search_focus: self.search_focus,
            search_results,
            shortcut_query: self.shortcut_query.clone(),
            shortcut_editor,
            detail,
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
            PreferenceAction::EditPreference { id } => {
                let row = settings.field(id, platform)?;
                if !row.enabled
                    || !matches!(
                        row.kind,
                        PreferenceKind::Number { .. } | PreferenceKind::Choice { .. }
                    )
                {
                    return Err("This setting cannot be edited here.".into());
                }
                self.editing_preference = Some(id);
                self.editing_shortcut = None;
                self.capture = None;
            }
            PreferenceAction::ClosePreference => self.editing_preference = None,
            PreferenceAction::Page { page } => {
                self.page = page;
                self.query.clear();
                self.searching = false;
                self.editing_shortcut = None;
                self.editing_preference = None;
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
                self.editing_preference = None;
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

    fn check(text: &str) {
        assert!(
            text.chars().count() <= 54,
            "Preferences copy exceeds 54 characters: {text}"
        );
        assert_eq!(text, text.trim());
        assert!(!text.contains(['\n', '\r', '\t']));
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
