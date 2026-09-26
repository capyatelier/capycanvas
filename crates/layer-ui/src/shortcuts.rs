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
        let key = key.to_lowercase();
        let key = Self::device_key(&key).map_or(key, str::to_owned);
        match Self::modifier_name(&key) {
            Some(name) => Self {
                key: name.into(),
                command: modifiers.command && name != "control" && name != "meta",
                shift: modifiers.shift && name != "shift",
                alt: modifiers.alt && name != "alt",
            },
            None => Self {
                key,
                command: modifiers.command,
                shift: modifiers.shift,
                alt: modifiers.alt,
            },
        }
    }
    pub fn modifier_name(key: &str) -> Option<&'static str> {
        Some(match key {
            "shift" | "shift_l" | "shift_r" => "shift",
            "control" | "control_l" | "control_r" | "ctrl" => "control",
            "alt" | "alt_l" | "alt_r" => "alt",
            "meta" | "meta_l" | "meta_r" | "super" | "super_l" | "super_r" => "meta",
            _ => return None,
        })
    }
    pub fn device_key(key: &str) -> Option<&'static str> {
        Some(match key.strip_prefix("xf86").unwrap_or(key) {
            "volumeup" | "audiovolumeup" | "audioraisevolume" => "volumeup",
            "volumedown" | "audiovolumedown" | "audiolowervolume" => "volumedown",
            "volumemute" | "audiovolumemute" | "audiomute" => "volumemute",
            "mediaplaypause" | "audioplay" | "audiopause" => "mediaplaypause",
            "mediatracknext" | "audionext" => "mediatracknext",
            "mediatrackprevious" | "audioprev" => "mediatrackprevious",
            _ => return None,
        })
    }
    fn device_label(key: &str) -> Option<String> {
        if let Some(button) = key.strip_prefix("gamepad_") {
            return GAMEPAD_BUTTONS
                .contains(&key)
                .then(|| format!("Gamepad {}", match button {
                    "up" => "↑".into(),
                    "down" => "↓".into(),
                    "left" => "←".into(),
                    "right" => "→".into(),
                    "select" | "start" | "home" => button[..1].to_uppercase() + &button[1..],
                    _ => button.to_uppercase(),
                }));
        }
        Some(match key {
            "volumeup" => "Volume Up",
            "volumedown" => "Volume Down",
            "volumemute" => "Mute",
            "mediaplaypause" => "Play/Pause",
            "mediatracknext" => "Next Track",
            "mediatrackprevious" => "Previous Track",
            _ => return None,
        }
        .into())
    }
    pub fn validate_for(&self, held: bool) -> Result<(), String> {
        if held && matches!(self.key.as_str(), "shift" | "control" | "alt") && !self.command && !self.shift && !self.alt {
            return Ok(());
        }
        self.validate()
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
        ) || Self::device_label(&self.key).is_some()
            || self
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
            "control" => if platform.apple() { "⌃" } else { "Ctrl" }.into(),
            "alt" => if platform.apple() { "⌥" } else { "Alt" }.into(),
            "arrowleft" => "←".into(),
            "arrowright" => "→".into(),
            "arrowup" => "↑".into(),
            "arrowdown" => "↓".into(),
            key if key.len() == 1 => key.to_uppercase(),
            key if let Some(label) = Self::device_label(key) => label,
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
    Hold {
        command: CommandId,
    },
}
impl ShortcutAction {
    pub fn held(&self) -> bool {
        matches!(self, Self::Pan | Self::Hold { .. })
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BindingScope {
    #[default]
    Application,
    Canvas,
    Tools {
        categories: Vec<ToolCategory>,
    },
}
impl BindingScope {
    pub fn is_application(&self) -> bool {
        *self == Self::Application
    }
    pub fn specificity(&self) -> u8 {
        match self {
            Self::Application => 0,
            Self::Canvas => 1,
            Self::Tools { .. } => 2,
        }
    }
    pub fn applies(&self, canvas: Option<ToolCategory>) -> bool {
        match self {
            Self::Application => true,
            Self::Canvas => canvas.is_some(),
            Self::Tools { categories } => canvas.is_some_and(|c| categories.contains(&c)),
        }
    }
    pub fn overlaps(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Tools { categories: a }, Self::Tools { categories: b }) => a.iter().any(|c| b.contains(c)),
            _ => true,
        }
    }
}
pub const GAMEPAD_BUTTONS: [&str; 17] = [
    "gamepad_a",
    "gamepad_b",
    "gamepad_x",
    "gamepad_y",
    "gamepad_l1",
    "gamepad_r1",
    "gamepad_l2",
    "gamepad_r2",
    "gamepad_select",
    "gamepad_start",
    "gamepad_l3",
    "gamepad_r3",
    "gamepad_up",
    "gamepad_down",
    "gamepad_left",
    "gamepad_right",
    "gamepad_home",
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GestureTrigger {
    pub id: &'static str,
    pub label: &'static str,
    pub default: &'static str,
    pub held: bool,
}
pub const GESTURE_TRIGGERS: [GestureTrigger; 5] = [
    GestureTrigger { id: "touch.tap.2", label: "Two-finger tap", default: "command.Undo", held: false },
    GestureTrigger { id: "touch.tap.3", label: "Three-finger tap", default: "command.Redo", held: false },
    GestureTrigger { id: "touch.tap.4", label: "Four-finger tap", default: "", held: false },
    GestureTrigger { id: "pen.button.primary", label: "Pen side button", default: "", held: true },
    GestureTrigger { id: "pen.button.secondary", label: "Pen second side button", default: "", held: true },
];
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShortcutDefinition {
    pub id: String,
    pub label: String,
    pub action: ShortcutAction,
    #[serde(default)]
    pub repeat: bool,
    #[serde(default, skip_serializing_if = "BindingScope::is_application")]
    pub scope: BindingScope,
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

/// Exact semantic aliases, shared by binding hints and command discovery.
/// Tool variants with parameters only share hints; they are not aliases here.
pub(crate) fn action_command(action: &UiAction) -> Option<CommandId> {
    match action {
        UiAction::Invoke { command } => Some(*command),
        UiAction::Layer { action: LayerAction::FillSelection } => Some(CommandId::FillSelection),
        UiAction::Layer { action: LayerAction::Deselect } => Some(CommandId::Deselect),
        UiAction::Layer { action: LayerAction::InvertSelection } => Some(CommandId::InvertSelection),
        UiAction::Layer { action: LayerAction::New { group: false, clipped: false } } => Some(CommandId::AddLayer),
        _ => None,
    }
}

pub(crate) fn tool_command(action: &UiAction) -> Option<CommandId> {
    let UiAction::Layer { action: LayerAction::Tool { tool } } = action else {
        return None;
    };
    Some(match tool {
        LayerCanvasTool::Region { fill: true, .. } => CommandId::Fill,
        LayerCanvasTool::Region { fill: false, .. } => CommandId::AutoSelect,
        LayerCanvasTool::Gradient { .. } => CommandId::Gradient,
        LayerCanvasTool::Figure { .. } => CommandId::Figure,
        LayerCanvasTool::Ruler { .. } => CommandId::Ruler,
        LayerCanvasTool::Hand => CommandId::Hand,
        tool if tool.picks_color() => CommandId::Eyedropper,
        LayerCanvasTool::Select => CommandId::Lasso,
        LayerCanvasTool::Move => CommandId::Move,
        _ => return None,
    })
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
    // Primary+K is also usable in browsers, which reserve Primary+Shift+P.
    if id == "command.SearchCommands" { return vec![key("p", true, true), key("k", true, false)]; }
    let chord = match id {
        "command.SoftProof" => KeyChord { key: "p".into(), command: true, shift: false, alt: true },
        "command.GamutWarning" => KeyChord { key: "y".into(), command: true, shift: true, alt: false },
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
        // Browser-owned F11 is already excluded by KeyChord::available(Web).
        "command.Fullscreen" => key("f11", false, false),
        "command.Undo" => key("z", true, false),
        "command.Redo" => return vec![key("z", true, true), key("y", true, false)],
        "command.FillSelection" => key("backspace", false, true),
        "command.QuickMask" => key("q", false, false),
        "command.Reselect" => key("d", true, true),
        "command.ResetMaskColors" => key("d", false, false),
        "command.SelectAll" => key("a", true, false),
        "command.Deselect" => key("d", true, false),
        "command.InvertSelection" => key("i", true, true),
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
        "command.NewDocument" => key("n", true, false),
        "command.NewWindow" => key("n", true, true),
        "command.OpenDocument" => key("o", true, false),
        "command.ImportImage" => key("o", true, true),
        "command.PasteImage" => key("v", true, false),
        "command.SaveDocument" => key("s", true, false),
        "command.SaveDocumentAs" => key("s", true, true),
        "command.ExportDocument" => key("e", true, true),
        "command.CloseDocument" => key("w", true, false),
        "canvas.pan" => key(" ", false, false),
        "hold.eyedropper" => key("alt", false, false),
        "tool_setting.size.decrease" => key("[", false, false),
        "tool_setting.size.increase" => key("]", false, false),
        _ => return Vec::new(),
    };
    vec![chord]
}

pub(crate) fn definitions(platform: Platform) -> Vec<(ShortcutDefinition, &'static str)> {
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
                    scope: BindingScope::Application,
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
                scope: BindingScope::Application,
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
            scope: BindingScope::Canvas,
        },
        "Canvas",
    ));
    use ToolCategory as C;
    for (id, label, command, scope) in [
        ("hold.eyedropper", "Sample color while held", CommandId::Eyedropper,
            BindingScope::Tools { categories: vec![C::Drawing, C::Blending, C::FillGradient] }),
        ("hold.eraser", "Erase while held", CommandId::Eraser,
            BindingScope::Tools { categories: vec![C::Drawing, C::Blending] }),
        ("hold.move", "Move while held", CommandId::Move, BindingScope::Canvas),
    ] {
        if command.available_on(platform) {
            rows.push((
                ShortcutDefinition { id: id.into(), label: label.into(), action: ShortcutAction::Hold { command }, repeat: false, scope },
                "Canvas",
            ));
        }
    }
    for (setting, noun) in [("size", "brush size"), ("opacity", "brush opacity")] {
        for (direction, steps) in [("decrease", -1.), ("increase", 1.)] {
            rows.push((
                ShortcutDefinition {
                    id: format!("tool_setting.{setting}.{direction}"),
                    label: format!("{}{} {noun}", direction[..1].to_uppercase(), &direction[1..]),
                    action: ShortcutAction::Action {
                        action: Box::new(UiAction::StepToolSetting { id: setting.into(), steps }),
                    },
                    repeat: true,
                    scope: BindingScope::Canvas,
                },
                "Tool settings",
            ));
        }
    }
    for (id, label, action, group) in [
        ("color.swap", "Swap colors", UiAction::Color { action: ColorAction::Swap }, "Colors"),
        ("layer.duplicate", "Duplicate layer", UiAction::Layer { action: LayerAction::DuplicateSelected }, "Layers"),
        ("layer.group", "Group layers", UiAction::Layer { action: LayerAction::GroupSelected }, "Layers"),
    ] {
        rows.push((
            ShortcutDefinition {
                id: id.into(),
                label: label.into(),
                action: ShortcutAction::Action { action: Box::new(action) },
                repeat: false,
                scope: BindingScope::Application,
            },
            group,
        ));
    }
    rows.extend(brush_catalog().map(|brush| {
        (
            ShortcutDefinition {
                id: format!("brush.{}", brush.id),
                label: brush.label.into(),
                action: ShortcutAction::Action {
                    action: Box::new(UiAction::SelectBrush { id: brush.id }),
                },
                repeat: false,
                scope: BindingScope::Application,
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
                scope: BindingScope::Application,
            },
            "Brush sizes",
        )
    }));
    rows
}
impl Settings {
    /// Resolve action identity, not translated labels or widget names.
    pub fn action_shortcut(&self, action: &UiAction, platform: Platform) -> String {
        self.action_keys(action, platform)
            .iter()
            .map(|key| key.label(platform))
            .collect::<Vec<_>>()
            .join(" / ")
    }
    /// Native menu equivalents use typed chords, never parsed display labels.
    pub fn action_keys(&self, action: &UiAction, platform: Platform) -> Vec<KeyChord> {
        fn canonical(action: &UiAction) -> UiAction {
            action_command(action)
                .or_else(|| tool_command(action))
                .map_or_else(|| action.clone(), |command| UiAction::Invoke { command })
        }
        let action = canonical(action);
        let builtin = match &action {
            UiAction::Invoke { command } => Some(command.shortcut_id()),
            UiAction::SelectBrush { id } => Some(format!("brush.{id}")),
            UiAction::SetBrushSize { value } => Some(format!("size.{value}")),
            UiAction::Color { action: ColorAction::Swap } => Some("color.swap".into()),
            UiAction::Layer { action: LayerAction::DuplicateSelected } => Some("layer.duplicate".into()),
            UiAction::Layer { action: LayerAction::GroupSelected } => Some("layer.group".into()),
            UiAction::StepToolSetting { id, steps } if steps.abs() == 1. => {
                Some(format!("tool_setting.{id}.{}", if *steps < 0. { "decrease" } else { "increase" }))
            }
            _ => None,
        };
        let mut keys = if let Some(id) = builtin {
            if let UiAction::Invoke { command } = action {
                self.command_keys(command)
            } else {
                self.keys(&id)
            }
        } else {
            Vec::new()
        };
        keys.retain(|key| key.available(platform));
        keys
    }

    pub fn action_tooltip(&self, label: &str, action: &UiAction, platform: Platform) -> String {
        let shortcut = self.action_shortcut(action, platform);
        if shortcut.is_empty() {
            label.into()
        } else {
            format!("{label} ({shortcut})")
        }
    }
    pub(crate) fn keymap_preset(&self) -> Option<&'static crate::keymaps::ParsedPreset> {
        self.keymap.as_ref().and_then(|keymap| crate::keymaps::preset(&keymap.id))
    }
    pub(crate) fn base_keys(&self, id: &str) -> Vec<KeyChord> {
        let preset = self.keymap_preset();
        if let Some(keys) = preset.and_then(|p| p.keys_for(id)) {
            return keys.to_vec();
        }
        defaults(id)
            .into_iter()
            .filter(|key| !preset.is_some_and(|p| p.binds(key)))
            .collect()
    }
    pub(crate) fn keys(&self, id: &str) -> Vec<KeyChord> {
        if let Some(keys) = self.shortcuts.get(id) {
            return keys.clone();
        }
        // An upgrade may add defaults on keys the artist already assigned.
        // Explicit saved bindings win; two explicit bindings still conflict.
        self.base_keys(id)
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
        let defaults = self.base_keys(id);
        keys.len() != defaults.len() || keys.iter().any(|key| !defaults.contains(key))
    }
    pub(crate) fn shortcut_match(
        &self,
        chord: &KeyChord,
        platform: Platform,
        canvas: Option<ToolCategory>,
    ) -> Option<ShortcutDefinition> {
        if !chord.available(platform) {
            return None;
        }
        definitions(platform)
            .into_iter()
            .map(|(definition, _)| definition)
            .filter(|definition| definition.scope.applies(canvas) && self.keys(&definition.id).contains(chord))
            .max_by_key(|definition| definition.scope.specificity())
    }
    pub(crate) fn shortcut_scope(&self, id: &str, platform: Platform) -> (String, Vec<String>) {
        let all = definitions(platform);
        let Some((definition, _)) = all.iter().find(|(d, _)| d.id == id) else {
            return (String::new(), Vec::new());
        };
        let describe = |scope: &BindingScope| match scope {
            BindingScope::Application => "everywhere".to_string(),
            BindingScope::Canvas => "on the canvas".to_string(),
            BindingScope::Tools { categories } => {
                let names: Vec<_> = categories.iter().map(|c| c.label().to_lowercase()).collect();
                format!("with {} tools", match names.as_slice() {
                    [one] => one.clone(),
                    [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
                    [] => String::new(),
                })
            }
        };
        let mut overlaps = Vec::new();
        for chord in self.keys(id) {
            for (other, _) in &all {
                if other.id != definition.id
                    && other.scope.overlaps(&definition.scope)
                    && other.scope.specificity() != definition.scope.specificity()
                    && self.keys(&other.id).contains(&chord)
                {
                    overlaps.push(if other.scope.specificity() > definition.scope.specificity() {
                        format!("{} does {} {} instead", chord.label(platform), other.label, describe(&other.scope))
                    } else {
                        format!("{} does {} elsewhere", chord.label(platform), other.label)
                    });
                }
            }
        }
        let scope = describe(&definition.scope);
        (scope[..1].to_uppercase() + &scope[1..], overlaps)
    }
    pub(crate) fn held_shortcut(&self, id: &str, platform: Platform) -> bool {
        definitions(platform).iter().any(|(definition, _)| definition.id == id && definition.action.held())
    }
    pub(crate) fn conflict(
        &self,
        id: &str,
        chord: &KeyChord,
        platform: Platform,
    ) -> Option<ShortcutDefinition> {
        let all = definitions(platform);
        let scope = all.iter().find(|(d, _)| d.id == id).map(|(d, _)| d.scope.clone()).unwrap_or_default();
        all.into_iter().find_map(|(definition, _)| {
            (definition.id != id
                && definition.scope.specificity() == scope.specificity()
                && definition.scope.overlaps(&scope)
                && self.keys(&definition.id).contains(chord))
            .then_some(definition)
        })
    }
    pub(crate) fn gesture_default(&self, trigger: &str) -> &'static str {
        self.keymap_preset()
            .and_then(|p| p.preset.gestures.iter().find(|(t, _)| *t == trigger))
            .map(|(_, id)| *id)
            .or_else(|| GESTURE_TRIGGERS.iter().find(|t| t.id == trigger).map(|t| t.default))
            .unwrap_or("")
    }
    pub(crate) fn gesture_binding(&self, trigger: &str) -> &str {
        self.gestures.get(trigger).map_or_else(|| self.gesture_default(trigger), String::as_str)
    }
    pub(crate) fn gesture_definition(&self, trigger: &str, platform: Platform) -> Option<ShortcutDefinition> {
        let id = self.gesture_binding(trigger);
        if id.is_empty() {
            return None;
        }
        definitions(platform).into_iter().map(|(d, _)| d).find(|d| d.id == id)
    }
    fn validate_gestures(&self) -> Result<(), String> {
        if self.keymap.as_ref().is_some_and(|k| crate::keymaps::preset(&k.id).is_none()) {
            return Err("Unknown keymap".into());
        }
        let all = definitions(Platform::Gtk);
        for (trigger, id) in &self.gestures {
            let trigger = GESTURE_TRIGGERS
                .iter()
                .find(|t| t.id == trigger)
                .ok_or("Unknown gesture or pen button")?;
            if id.is_empty() {
                continue;
            }
            let definition = all
                .iter()
                .find(|(d, _)| d.id == *id)
                .ok_or("Unknown gesture action")?;
            if definition.0.action.held() && !trigger.held {
                return Err(format!("{} cannot hold an action", trigger.label));
            }
        }
        Ok(())
    }
    pub(crate) fn validate_shortcuts(&self) -> Result<(), String> {
        self.validate_gestures()?;
        let all = definitions(Platform::Gtk);
        for (id, keys) in &self.shortcuts {
            if !all.iter().any(|(a, _)| a.id == *id) || keys.len() > MAX_SHORTCUTS {
                return Err("Unknown action or too many shortcut alternatives".into());
            }
            let held = all.iter().any(|(a, _)| a.id == *id && a.action.held());
            for (index, chord) in keys.iter().enumerate() {
                chord.validate_for(held)?;
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
                .shortcut_match(&key("j", false, false), Platform::Gtk, None)
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
    fn nested_context_menus_preserve_hints_and_keys() {
        let mut settings = Settings::default();
        let action = UiAction::SetBrushSize { value: 48. };
        settings
            .shortcuts
            .insert("size.48".into(), vec![key("i", true, true)]);
        let item = ContextMenuItem {
            label: "Translated size".into(),
            hint: "Default icon".into(),
            bindings: Vec::new(),
            selected: Some(false),
            enabled: true,
            action: Some(action),
            sections: Vec::new(),
        };
        let menu = ContextMenu {
            title: "Context".into(),
            sections: vec![vec![ContextMenuItem {
                label: "Submenu".into(),
                bindings: Vec::new(),
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
        assert_eq!(
            menu.sections[0][0].sections[0][0].bindings,
            [key("i", true, true)]
        );
    }
}
