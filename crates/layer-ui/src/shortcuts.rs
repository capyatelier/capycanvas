//! One configurable keymap routes to the same typed actions as native controls.
//! Raw pen samples and gesture records are deliberately not keyboard commands.
use crate::*;
use serde::{Deserialize, Serialize};
pub(crate) const MAX_SHORTCUTS: usize = 4;

/// Native text editing is not the canvas keymap. Hosts execute these through
/// their text editor; shared copy and standard shortcut hints live here.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextEditAction {
    Cut,
    Copy,
    Paste,
    SelectAll,
}
#[derive(Clone, Debug, Serialize)]
pub struct TextEditMenuItem {
    pub action: TextEditAction,
    pub label: &'static str,
    pub key: KeyChord,
    pub shortcut: String,
}
pub fn text_edit_menu(platform: Platform) -> Vec<TextEditMenuItem> {
    [
        (TextEditAction::Cut, "Cut", "x"),
        (TextEditAction::Copy, "Copy", "c"),
        (TextEditAction::Paste, "Paste", "v"),
        (TextEditAction::SelectAll, "Select All", "a"),
    ]
    .into_iter()
    .map(|(action, label, letter)| {
        let key = key(letter, true, false);
        TextEditMenuItem {
            action,
            label,
            shortcut: key.label(platform),
            key,
        }
    })
    .collect()
}

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
                | "tab"
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
            || self.key == "escape"
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
    /// Whether the current binding set differs from the defaults, not merely
    /// whether a saved override exists. The order of alternatives is immaterial.
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
        "tools.ink" => key("p", false, false),
        "tools.paint" => key("b", false, false),
        "tools.blend" => key("j", false, false),
        "command.Eraser" => key("e", false, false),
        "command.Lasso" => key("m", false, false),
        "command.Move" => key("o", false, false),
        "command.ScaleRotate" => key("t", true, false),
        "command.ApplyTransform" => key("enter", false, false),
        "command.CancelTransform" => key("escape", false, false),
        "command.Hand" => key("h", false, false),
        "command.Eyedropper" => key("i", false, false),
        "command.Gradient" => key("g", false, false),
        "command.Figure" => key("u", false, false),
        "command.Ruler" => key("u", false, true),
        "command.AutoSelect" => key("w", false, false),
        "command.Fill" => key("f", false, false),
        "command.FitCanvas" => key("0", true, false),
        "command.ZoomIn" => key("=", true, false),
        "command.ZoomOut" => key("-", true, false),
        "command.ZenMode" => key("tab", false, false),
        "command.Undo" => key("z", true, false),
        "command.Redo" => return vec![key("z", true, true), key("y", true, false)],
        "command.UndoWorkspace" | "command.RedoWorkspace" => {
            return vec![KeyChord {
                key: "z".into(),
                command: true,
                alt: true,
                shift: id == "command.RedoWorkspace",
            }];
        }
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
        .filter(|c| c.available_on(platform))
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
    rows.extend(ToolFamily::ALL.into_iter().map(|family| {
        (
            ShortcutDefinition {
                id: family.shortcut_id().into(),
                label: family.label().into(),
                action: ShortcutAction::Action {
                    action: Box::new(UiAction::CycleTool { family }),
                },
                repeat: false,
            },
            "Tools",
        )
    }));
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
    /// Resolve action identity, not translated labels or widget names. Custom
    /// actions work in contextual menus too, including parameterized actions.
    pub fn action_shortcut(&self, action: &UiAction, platform: Platform) -> String {
        fn canonical(action: &UiAction) -> UiAction {
            use LayerAction as L;
            match action {
                UiAction::Layer {
                    action:
                        L::Tool {
                            tool: LayerCanvasTool::Region { fill, .. },
                        },
                } => UiAction::Invoke {
                    command: if *fill {
                        CommandId::Fill
                    } else {
                        CommandId::AutoSelect
                    },
                },
                UiAction::Layer {
                    action:
                        L::Tool {
                            tool: LayerCanvasTool::Gradient { .. },
                        },
                } => UiAction::Invoke {
                    command: CommandId::Gradient,
                },
                UiAction::Layer {
                    action:
                        L::Tool {
                            tool: LayerCanvasTool::Figure { .. },
                        },
                } => UiAction::Invoke {
                    command: CommandId::Figure,
                },
                UiAction::Layer {
                    action:
                        L::Tool {
                            tool: LayerCanvasTool::Ruler { .. },
                        },
                } => UiAction::Invoke {
                    command: CommandId::Ruler,
                },
                UiAction::Layer {
                    action:
                        L::Tool {
                            tool: LayerCanvasTool::Hand,
                        },
                } => UiAction::Invoke {
                    command: CommandId::Hand,
                },
                UiAction::Layer {
                    action: L::Tool { tool },
                } if tool.picks_color() => UiAction::Invoke {
                    command: CommandId::Eyedropper,
                },
                UiAction::Layer {
                    action:
                        L::Tool {
                            tool: LayerCanvasTool::Select,
                        },
                } => UiAction::Invoke {
                    command: CommandId::Lasso,
                },
                UiAction::Layer {
                    action:
                        L::Tool {
                            tool: LayerCanvasTool::Move,
                        },
                } => UiAction::Invoke {
                    command: CommandId::Move,
                },
                UiAction::Layer {
                    action:
                        L::New {
                            group: false,
                            clipped: false,
                        },
                } => UiAction::Invoke {
                    command: CommandId::AddLayer,
                },
                _ => action.clone(),
            }
        }
        let action = canonical(action);
        let builtin = match &action {
            UiAction::Invoke { command } => Some(command.shortcut_id()),
            UiAction::SelectBrush { id } => Some(format!("brush.{id}")),
            UiAction::SetBrushSize { value } => Some(format!("size.{value}")),
            _ => None,
        };
        let mut labels = Vec::new();
        if let Some(id) = builtin {
            let label = if let UiAction::Invoke { command } = action {
                self.command_keys(command)
                    .into_iter()
                    .filter(|k| k.available(platform))
                    .map(|k| k.label(platform))
                    .collect::<Vec<_>>()
                    .join(" / ")
            } else {
                self.shortcut_label(&id, platform)
            };
            if !label.is_empty() {
                labels.push(label);
            }
        }
        for definition in &self.custom_actions {
            if matches!(&definition.action, ShortcutAction::Action { action: a } if canonical(a) == action)
            {
                let label = self.shortcut_label(&definition.id, platform);
                if !label.is_empty() && !labels.contains(&label) {
                    labels.push(label);
                }
            }
        }
        labels.join(" / ")
    }
    pub fn action_tooltip(&self, label: &str, action: &UiAction, platform: Platform) -> String {
        let shortcut = self.action_shortcut(action, platform);
        if shortcut.is_empty() {
            label.into()
        } else {
            format!("{label} ({shortcut})")
        }
    }
    pub(crate) fn keys(&self, id: &str) -> Vec<KeyChord> {
        if let Some(keys) = self.shortcuts.get(id) {
            return keys.clone();
        }
        // An upgrade may add defaults on keys the artist already assigned.
        // Explicit saved bindings win; two explicit bindings still conflict.
        defaults(id)
            .into_iter()
            .filter(|key| !self.shortcuts.values().any(|keys| keys.contains(key)))
            .collect()
    }
    pub(crate) fn command_keys(&self, command: CommandId) -> Vec<KeyChord> {
        let mut keys = self.keys(&command.shortcut_id());
        if let Some(family) = ToolFamily::for_command(command) {
            for key in self.keys(family.shortcut_id()) {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        keys
    }
    pub(crate) fn shortcut_label(&self, id: &str, platform: Platform) -> String {
        self.keys(id)
            .iter()
            .filter(|c| c.available(platform))
            .map(|c| c.label(platform))
            .collect::<Vec<_>>()
            .join(" / ")
    }
    pub(crate) fn shortcut_modified(&self, id: &str) -> bool {
        let keys = self.keys(id);
        let defaults = defaults(id);
        keys.len() != defaults.len() || keys.iter().any(|key| !defaults.contains(key))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_default_keys_do_not_break_saved_custom_bindings() {
        let mut settings = Settings::default();
        let brush = CommandId::Brush.shortcut_id();
        settings.shortcuts.insert(
            brush.clone(),
            vec![key("b", false, false), key("j", false, false)],
        );
        settings.validate().unwrap();
        for family in [ToolFamily::Paint, ToolFamily::Blend] {
            assert!(settings.keys(family.shortcut_id()).is_empty());
            assert!(settings.shortcut_modified(family.shortcut_id()));
        }
        assert_eq!(
            settings
                .shortcut_match(&key("j", false, false), Platform::Gtk)
                .unwrap()
                .id,
            brush
        );
        settings
            .shortcuts
            .insert(CommandId::Pen.shortcut_id(), vec![key("j", false, false)]);
        assert!(
            settings.validate().is_err(),
            "two explicit overrides remain a conflict"
        );
        settings.shortcuts.clear();
        assert_eq!(
            settings.keys(ToolFamily::Paint.shortcut_id()),
            [key("b", false, false)]
        );
        assert!(!settings.shortcut_modified(ToolFamily::Paint.shortcut_id()));
    }

    #[test]
    fn tooltips_follow_rebindings_and_platform_without_matching_copy() {
        let mut settings = Settings::default();
        let zen = UiAction::Invoke {
            command: CommandId::ZenMode,
        };
        assert_eq!(
            settings.action_tooltip("Zen mode", &zen, Platform::Gtk),
            "Zen mode (Tab)"
        );
        settings.shortcuts.insert(
            CommandId::ZenMode.shortcut_id(),
            vec![key("j", true, false), key("k", true, true)],
        );
        assert_eq!(
            settings.action_tooltip("Translated label", &zen, Platform::Web),
            "Translated label (Ctrl+J / Ctrl+Shift+K)"
        );
        assert_eq!(
            settings.action_shortcut(&zen, Platform::Mac),
            "⌘+J / ⌘+Shift+K"
        );
        settings
            .shortcuts
            .insert(CommandId::ZenMode.shortcut_id(), Vec::new());
        assert_eq!(
            settings.action_tooltip("Zen mode", &zen, Platform::Gtk),
            "Zen mode"
        );
        let new = UiAction::Layer {
            action: LayerAction::New {
                group: false,
                clipped: false,
            },
        };
        settings.shortcuts.insert(
            CommandId::AddLayer.shortcut_id(),
            vec![key("n", true, true)],
        );
        assert_eq!(
            settings.action_shortcut(&new, Platform::Gtk),
            "Ctrl+Shift+N"
        );
    }

    #[test]
    fn nested_context_menus_and_reset_preserve_hints_and_custom_action_keys() {
        let mut settings = Settings::default();
        let action = UiAction::Preferences {
            action: PreferenceAction::Reset {
                id: PreferenceId::ZenIcon,
            },
        };
        settings.custom_actions.push(ShortcutDefinition {
            id: "custom.reset-icon".into(),
            label: "Restore icon".into(),
            action: ShortcutAction::Action {
                action: Box::new(action.clone()),
            },
            repeat: false,
        });
        settings
            .shortcuts
            .insert("custom.reset-icon".into(), vec![key("i", true, true)]);
        let item = ContextMenuItem {
            label: "Translated reset".into(),
            hint: "Default icon".into(),
            selected: Some(false),
            enabled: true,
            action: Some(action),
            sections: Vec::new(),
        };
        let menu = ContextMenu {
            title: "Context".into(),
            sections: vec![vec![ContextMenuItem {
                label: "Submenu".into(),
                hint: String::new(),
                selected: None,
                enabled: true,
                action: None,
                sections: vec![vec![item]],
            }]],
        }
        .with_shortcuts(&settings, Platform::Android);
        assert_eq!(
            menu.sections[0][0].sections[0][0].hint,
            "Default icon · Ctrl+Shift+I"
        );
        let reset = settings
            .pages(Platform::Gtk)
            .into_iter()
            .flat_map(|p| p.groups)
            .flat_map(|g| g.rows)
            .find(|r| r.id == PreferenceId::ZenIcon)
            .unwrap()
            .reset
            .unwrap();
        assert_eq!(reset.hint, format!("{} · Ctrl+Shift+I", reset.value));
        assert!(!reset.enabled);
    }
}
