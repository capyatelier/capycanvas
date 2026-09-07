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
    pub pressure_gamma: f32,
    pub cursor: CursorMode,
    pub zen_reveal: f32,
    pub zen_hide: f32,
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
            pressure_gamma: 1.0,
            cursor: CursorMode::default(),
            zen_reveal: 80.0,
            zen_hide: 40.0,
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
    ZenReveal,
    ZenHide,
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
}
impl PreferenceId {
    pub fn key(self) -> &'static str {
        match self {
            Self::Theme => "theme",
            Self::ZenReveal => "zen-reveal",
            Self::ZenHide => "zen-hide",
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
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PreferenceValue {
    Bool(bool),
    Number(f32),
    Choice(u32),
}
// Choice indices and numbers have distinct Rust types; JSON numbers are decoded
// according to the destination field below, so clients need no tagged wrappers.
impl PreferenceValue {
    fn number(&self) -> Option<f32> {
        match self {
            Self::Number(v) => Some(*v),
            Self::Choice(v) => Some(*v as f32),
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
    Choice { options: Vec<String>, selected: u32 },
    Number { control: NumericControl, value: f32 },
    Switch { active: bool },
    Info { value: String },
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
    pub capture: Option<ShortcutCapture>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct PreferencesView {
    pub pages: Vec<PreferencePage>,
    pub page: SettingsPage,
    pub query: String,
    pub shortcuts: Vec<ShortcutRow>,
    pub capture: Option<ShortcutCapture>,
    pub error: Option<String>,
    pub dirty: bool,
    pub empty: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PreferenceAction {
    Page {
        page: SettingsPage,
    },
    Search {
        query: String,
    },
    Edit {
        id: PreferenceId,
        value: PreferenceValue,
    },
    BeginShortcut {
        id: String,
    },
    ConfirmShortcut {
        replace: bool,
    },
    CancelShortcut,
    ClearShortcut,
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
            control: NumericControl {
                min,
                max,
                step,
                digits: if step < 1.0 { 2 } else { 0 },
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
                "1 is linear. Lower values reach full pressure sooner.",
                PreferenceKind::Number {
                    control: PRESSURE_CONTROL,
                    value: self.pressure_gamma,
                },
            ),
            row(
                Feedback,
                "Instant stroke feedback",
                "Use a replaceable stroke tip to reduce apparent input lag.",
                PreferenceKind::Switch {
                    active: self.feedback,
                },
            ),
            number(
                PredictionHorizon,
                "Prediction horizon (ms)",
                "Maximum lookahead; lower values reduce overshoot.",
                self.prediction_ms,
                0.0,
                32.0,
                1.0,
            ),
            number(
                TipLock,
                "Tip lock",
                "0 preserves modeled geometry; 1 brings coverage to the pen tip.",
                self.tip_lock,
                0.0,
                1.0,
                0.05,
            ),
        ];
        if matches!(platform, Platform::Web | Platform::Ios | Platform::Android) {
            input.push(row(
                PlatformPrediction,
                "Use platform predictions",
                "Use native predicted samples when the platform supplies them.",
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
                    rows: vec![row(
                        Theme,
                        "Appearance",
                        "Follow the system theme, or choose an override.",
                        PreferenceKind::Choice {
                            options: vec!["System".into(), "Light".into(), "Dark".into()],
                            selected: match self.theme {
                                None => 0,
                                Some(crate::Theme::Light) => 1,
                                Some(crate::Theme::Dark) => 2,
                            },
                        },
                    )],
                },
                PreferenceGroup {
                    title: "Zen mode".into(),
                    rows: vec![
                        number(
                            ZenReveal,
                            "Reveal distance (px)",
                            "Reveal controls near an occupied window edge.",
                            self.zen_reveal,
                            20.0,
                            200.0,
                            1.0,
                        ),
                        number(
                            ZenHide,
                            "Keep-visible distance (px)",
                            "Keep controls visible while the pointer is near them.",
                            self.zen_hide,
                            10.0,
                            200.0,
                            1.0,
                        ),
                    ],
                },
            ],
            vec![
                PreferenceGroup {
                    title: "Pointer".into(),
                    rows: vec![row(
                        Cursor,
                        "Canvas cursor",
                        "The live outline follows the brush shape, pressure and rotation.",
                        PreferenceKind::Choice {
                            options: CursorMode::CHOICES.iter().map(|c| c.1.into()).collect(),
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
                            "Scroll pans; Shift-scroll moves horizontally.",
                            self.pan_speed,
                            0.25,
                            4.0,
                            0.05,
                        ),
                        number(
                            ZoomSpeed,
                            "Scroll zoom speed",
                            "Ctrl-scroll zooms around the pointer.",
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
                        "A lightweight, GPU-powered drawing workspace.",
                        PreferenceKind::Info {
                            value: env!("CARGO_PKG_VERSION").into(),
                        },
                    ),
                    row(
                        License,
                        "Application license",
                        "Software and non-brand assets. The capybara mark has separate branding terms; dependencies retain their own licenses.",
                        PreferenceKind::Info {
                            value: env!("CARGO_PKG_LICENSE").into(),
                        },
                    ),
                    row(
                        Renderer,
                        "Canvas rendering",
                        "Hardware GPU raster; no CPU canvas fallback.",
                        PreferenceKind::Info {
                            value: match platform {
                                Platform::Gtk => "Vulkan · Wayland",
                                Platform::Web => "WebGPU",
                                _ => "Native GPU",
                            }
                            .into(),
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
    fn edit(
        &mut self,
        id: PreferenceId,
        value: PreferenceValue,
        platform: Platform,
    ) -> Result<(), String> {
        let field = self
            .pages(platform)
            .into_iter()
            .flat_map(|p| p.groups)
            .flat_map(|g| g.rows)
            .find(|r| r.id == id)
            .ok_or("This setting is unavailable on this platform")?;
        if !field.enabled {
            return Err("Enable instant stroke feedback before editing this setting".into());
        }
        use PreferenceId::*;
        match (&field.kind, &value) {
            (PreferenceKind::Number { control, .. }, _) => {
                control.validate(value.number().ok_or("Expected a number")?, &field.title)?
            }
            (PreferenceKind::Choice { options, .. }, _)
                if value.choice().is_some_and(|v| (v as usize) < options.len()) => {}
            (PreferenceKind::Switch { .. }, PreferenceValue::Bool(_)) => {}
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
            Pressure => self.pressure_gamma = n,
            ZenReveal => self.zen_reveal = n,
            ZenHide => self.zen_hide = n,
            PanSpeed => self.pan_speed = n,
            ZoomSpeed => self.zoom_speed = n,
            PredictionHorizon => self.prediction_ms = n,
            TipLock => self.tip_lock = n,
            Feedback => self.feedback = matches!(value, PreferenceValue::Bool(true)),
            PlatformPrediction => {
                self.platform_prediction = matches!(value, PreferenceValue::Bool(true))
            }
            Version | License | Renderer => return Err("This information is read-only".into()),
        }
        Ok(())
    }
}
impl PreferencesState {
    pub(crate) fn view(
        &self,
        draft: &Settings,
        active: &Settings,
        platform: Platform,
    ) -> PreferencesView {
        let query = self.query.trim().to_lowercase();
        let mut pages = draft.pages(platform);
        for page in &mut pages {
            for group in &mut page.groups {
                for row in &mut group.rows {
                    row.visible = format!(
                        "{} {} {} {}",
                        page.title, group.title, row.title, row.description
                    )
                    .to_lowercase()
                    .contains(&query);
                }
            }
        }
        let shortcuts: Vec<_> = crate::shortcuts::definitions(draft, platform)
            .into_iter()
            .map(|(definition, group)| ShortcutRow {
                shortcut: draft.shortcut_label(&definition.id, platform),
                modified: draft.shortcuts.contains_key(&definition.id),
                visible: format!("{group} {}", definition.label)
                    .to_lowercase()
                    .contains(&query),
                id: definition.id,
                label: definition.label,
                group: group.into(),
            })
            .collect();
        let matches = |page: &PreferencePage| {
            page.groups.iter().flat_map(|g| &g.rows).any(|r| r.visible)
                || (page.id == SettingsPage::Shortcuts && shortcuts.iter().any(|r| r.visible))
        };
        let page = if !query.is_empty() && !pages.iter().any(|p| p.id == self.page && matches(p)) {
            pages
                .iter()
                .find(|p| matches(p))
                .map_or(self.page, |p| p.id)
        } else {
            self.page
        };
        PreferencesView {
            empty: !pages.iter().any(matches),
            pages,
            page,
            query: self.query.clone(),
            shortcuts,
            capture: self.capture.clone(),
            error: self.error.clone(),
            dirty: draft != active,
        }
    }
    pub(crate) fn edit(
        &mut self,
        draft: &mut Settings,
        action: PreferenceAction,
        platform: Platform,
    ) {
        self.error = self.try_edit(draft, action, platform).err();
    }
    fn try_edit(
        &mut self,
        draft: &mut Settings,
        action: PreferenceAction,
        platform: Platform,
    ) -> Result<(), String> {
        match action {
            PreferenceAction::Page { page } => {
                self.page = page;
                self.query.clear();
                self.capture = None;
            }
            PreferenceAction::Search { query } => {
                if query.len() > 256 {
                    return Err("Search is too long".into());
                }
                self.query = query;
            }
            PreferenceAction::Edit { id, value } => draft.edit(id, value, platform)?,
            PreferenceAction::BeginShortcut { id } => {
                let (definition, _) = crate::shortcuts::definitions(draft, platform)
                    .into_iter()
                    .find(|(d, _)| d.id == id)
                    .ok_or("Unknown shortcut action")?;
                self.capture = Some(ShortcutCapture {
                    id,
                    label: definition.label,
                    chord: None,
                    shortcut: "Press a new key combination".into(),
                    conflict: None,
                    error: None,
                });
            }
            PreferenceAction::CancelShortcut => self.capture = None,
            PreferenceAction::ClearShortcut => {
                let capture = self.capture.take().ok_or("No shortcut is being recorded")?;
                draft.shortcuts.insert(capture.id, Vec::new());
            }
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
                if let Some(conflict) = draft.conflict(&capture.id, &chord, platform) {
                    if !replace {
                        return Err(format!("Already assigned to {}", conflict.label));
                    }
                    let keys = draft
                        .keys(&conflict.id)
                        .into_iter()
                        .filter(|c| *c != chord)
                        .collect();
                    draft.shortcuts.insert(conflict.id, keys);
                }
                draft.shortcuts.insert(capture.id.clone(), vec![chord]);
                self.capture = None;
            }
            PreferenceAction::ResetShortcut { id } => {
                if !crate::shortcuts::definitions(draft, platform)
                    .iter()
                    .any(|(d, _)| d.id == id)
                {
                    return Err("Unknown shortcut action".into());
                }
                for chord in crate::shortcuts::defaults(&id) {
                    if let Some(conflict) = draft.conflict(&id, &chord, platform) {
                        return Err(format!(
                            "Reset conflicts with {}. Reset all shortcuts or change that binding first.",
                            conflict.label
                        ));
                    }
                }
                draft.shortcuts.remove(&id);
            }
            PreferenceAction::ResetAllShortcuts => {
                draft.shortcuts.clear();
                self.capture = None;
            }
            PreferenceAction::RegisterAction { definition } => {
                let mut candidate = draft.clone();
                candidate.custom_actions.retain(|a| a.id != definition.id);
                candidate.custom_actions.push(definition);
                candidate.validate()?;
                *draft = candidate;
            }
        }
        Ok(())
    }
    pub(crate) fn record(&mut self, draft: &Settings, chord: KeyChord, platform: Platform) {
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
            capture.conflict = draft
                .conflict(&capture.id, &chord, platform)
                .map(|d| d.label);
            capture.shortcut = chord.label(platform);
            capture.chord = capture.error.is_none().then_some(chord);
        }
    }
}
