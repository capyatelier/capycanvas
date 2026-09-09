//! One configurable keymap routes to the same typed actions as native controls.
//! Raw pen samples and gesture records are deliberately not keyboard commands.
use crate::*;
use serde::{Deserialize, Serialize};
pub(crate) const MAX_SHORTCUTS: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyChord {
    pub key: String,
    #[serde(default)]
    pub command: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
}
impl KeyChord {
    pub fn new(key: &str, modifiers: Modifiers) -> Self {
        Self {
            key: key.to_lowercase(),
            command: modifiers.command,
            shift: modifiers.shift,
            alt: modifiers.alt,
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        let printable = self.key.chars().count() == 1 && !self.key.chars().any(char::is_control);
        let named = matches!(
            self.key.as_str(),
            "enter"
                | "backspace"
                | "delete"
                | "insert"
                | "home"
                | "end"
                | "pageup"
                | "pagedown"
                | "arrowleft"
                | "arrowright"
                | "arrowup"
                | "arrowdown"
        ) || self
            .key
            .strip_prefix('f')
            .and_then(|n| n.parse::<u8>().ok())
            .is_some_and(|n| (1..=24).contains(&n));
        if !(printable || named)
            || self.key.len() > 32
            || self.key != self.key.to_lowercase()
            || Self::modifier(&self.key)
            || matches!(self.key.as_str(), "escape" | "tab")
        {
            return Err("Choose another key for this shortcut.".into());
        }
        Ok(())
    }
    pub fn modifier(key: &str) -> bool {
        matches!(
            key,
            "shift"
                | "control"
                | "alt"
                | "meta"
                | "super"
                | "shift_l"
                | "shift_r"
                | "control_l"
                | "control_r"
                | "alt_l"
                | "alt_r"
                | "meta_l"
                | "meta_r"
                | "super_l"
                | "super_r"
                | "capslock"
                | "caps_lock"
                | "altgraph"
                | "iso_level3_shift"
        )
    }
    pub fn available(&self, platform: Platform) -> bool {
        !(platform == Platform::Web
            && (matches!(self.key.as_str(), "f5" | "f11" | "f12")
                || self.command
                    && !self.alt
                    && matches!(self.key.as_str(), "w" | "t" | "n" | "r" | "l" | "q" | "p")))
    }
    pub fn label(&self, platform: Platform) -> String {
        let mut parts = Vec::new();
        if self.command {
            parts.push(if platform.apple() { "⌘" } else { "Ctrl" }.to_string());
        }
        if self.alt {
            parts.push("Alt".into());
        }
        if self.shift {
            parts.push("Shift".into());
        }
        parts.push(match self.key.as_str() {
            " " => "Space".into(),
            "arrowleft" => "←".into(),
            "arrowright" => "→".into(),
            "arrowup" => "↑".into(),
            "arrowdown" => "↓".into(),
            key if key.len() == 1 => key.to_uppercase(),
            key => {
                let mut chars = key.chars();
                chars.next().map_or_else(String::new, |c| {
                    c.to_uppercase().collect::<String>() + chars.as_str()
                })
            }
        });
        parts.join("+")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShortcutAction {
    Action {
        action: Box<UiAction>,
    },
    /// A momentary input mode: release its recorded key to leave it.
    Pan,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShortcutDefinition {
    pub id: String,
    pub label: String,
    pub action: ShortcutAction,
    #[serde(default)]
    pub repeat: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct ShortcutRow {
    pub id: String,
    pub label: String,
    pub group: String,
    pub shortcut: String,
    pub modified: bool,
    pub visible: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShortcutCapture {
    pub id: String,
    pub label: String,
    pub chord: Option<KeyChord>,
    pub shortcut: String,
    pub conflict: Option<String>,
    pub error: Option<String>,
    /// Authored in the core; hosts display this without composing messages.
    #[serde(default)]
    pub notice: String,
}

impl CommandId {
    pub fn shortcut_id(self) -> String {
        format!("command.{self:?}")
    }
}
fn key(key: &str, command: bool, shift: bool) -> KeyChord {
    KeyChord {
        key: key.into(),
        command,
        shift,
        alt: false,
    }
}
pub(crate) fn defaults(id: &str) -> Vec<KeyChord> {
    let chord = match id {
        "command.Brush" => key("b", false, false),
        "command.Eraser" => key("e", false, false),
        "command.FitCanvas" => key("f", false, false),
        "command.ZenMode" => key("z", false, false),
        "command.Undo" => key("z", true, false),
        "command.Redo" => return vec![key("z", true, true), key("y", true, false)],
        "command.Settings" => key(",", true, false),
        "command.KeyboardShortcuts" => key("?", true, true),
        "command.NewWindow" => key("n", true, false),
        "canvas.pan" => key(" ", false, false),
        _ => return Vec::new(),
    };
    vec![chord]
}

pub(crate) fn definitions(
    settings: &Settings,
    platform: Platform,
) -> Vec<(ShortcutDefinition, &'static str)> {
    let mut rows: Vec<_> = CommandId::ALL
        .into_iter()
        .filter(|c| *c != CommandId::NewWindow || platform.native_windows())
        .map(|command| {
            (
                ShortcutDefinition {
                    id: command.shortcut_id(),
                    label: command.label().into(),
                    action: ShortcutAction::Action {
                        action: Box::new(UiAction::Invoke { command }),
                    },
                    repeat: matches!(command, CommandId::Undo | CommandId::Redo),
                },
                "Commands",
            )
        })
        .collect();
    rows.push((
        ShortcutDefinition {
            id: "canvas.pan".into(),
            label: "Pan while held".into(),
            action: ShortcutAction::Pan,
            repeat: false,
        },
        "Canvas",
    ));
    rows.extend(brush_catalog().map(|brush| {
        (
            ShortcutDefinition {
                id: format!("brush.{}", brush.id),
                label: brush.label.into(),
                action: ShortcutAction::Action {
                    action: Box::new(UiAction::SelectBrush { id: brush.id }),
                },
                repeat: false,
            },
            "Brushes",
        )
    }));
    rows.extend(BRUSH_SIZES.iter().map(|&value| {
        (
            ShortcutDefinition {
                id: format!("size.{value}"),
                label: format!("Brush size {value}"),
                action: ShortcutAction::Action {
                    action: Box::new(UiAction::SetBrushSize { value }),
                },
                repeat: false,
            },
            "Brush sizes",
        )
    }));
    rows.extend(
        settings
            .custom_actions
            .iter()
            .cloned()
            .map(|a| (a, "Custom actions")),
    );
    rows
}
impl Settings {
    pub(crate) fn keys(&self, id: &str) -> Vec<KeyChord> {
        self.shortcuts
            .get(id)
            .cloned()
            .unwrap_or_else(|| defaults(id))
    }
    pub(crate) fn shortcut_label(&self, id: &str, platform: Platform) -> String {
        self.keys(id)
            .iter()
            .filter(|c| c.available(platform))
            .map(|c| c.label(platform))
            .collect::<Vec<_>>()
            .join(" / ")
    }
    pub(crate) fn shortcut_match(
        &self,
        chord: &KeyChord,
        platform: Platform,
    ) -> Option<ShortcutDefinition> {
        if !chord.available(platform) {
            return None;
        }
        definitions(self, platform)
            .into_iter()
            .find_map(|(definition, _)| {
                self.keys(&definition.id)
                    .contains(chord)
                    .then_some(definition)
            })
    }
    pub(crate) fn conflict(
        &self,
        id: &str,
        chord: &KeyChord,
        platform: Platform,
    ) -> Option<ShortcutDefinition> {
        definitions(self, platform)
            .into_iter()
            .find_map(|(definition, _)| {
                (definition.id != id && self.keys(&definition.id).contains(chord))
                    .then_some(definition)
            })
    }
    pub(crate) fn validate_shortcuts(&self) -> Result<(), String> {
        let mut ids = std::collections::BTreeSet::new();
        if self.custom_actions.len() > 128 {
            return Err("Too many custom shortcut actions".into());
        }
        for action in &self.custom_actions {
            if !action.id.starts_with("custom.")
                || action.id.len() > 100
                || action.label.is_empty()
                || action.label.len() > 120
                || !ids.insert(&action.id)
            {
                return Err("Use a unique custom.* ID and a short action name.".into());
            }
        }
        let all = definitions(self, Platform::Gtk);
        for (id, keys) in &self.shortcuts {
            if !all.iter().any(|(a, _)| a.id == *id) || keys.len() > MAX_SHORTCUTS {
                return Err("Unknown action or too many shortcut alternatives".into());
            }
            for (index, chord) in keys.iter().enumerate() {
                chord.validate()?;
                if keys[..index].contains(chord) {
                    return Err("Duplicate shortcut alternative".into());
                }
                if self.conflict(id, chord, Platform::Gtk).is_some() {
                    return Err("Two actions cannot use the same shortcut".into());
                }
            }
        }
        Ok(())
    }
}
