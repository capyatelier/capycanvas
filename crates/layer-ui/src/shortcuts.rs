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
        if let Some(button) = key.strip_prefix("pad_button_") {
            return Some(format!("Pad button {button}"));
        }
        if let Some(name) = key.strip_prefix("xf86").filter(|name| !name.is_empty()) {
            return Some(name[..1].to_uppercase() + &name[1..]);
        }
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
    /// Any key or button, alone or combined; Escape stays free to cancel.
    pub fn holdable(&self) -> bool {
        match self.key.as_str() {
            "shift" => !self.shift,
            "alt" => !self.alt,
            "control" => !self.command,
            "meta" => true,
            _ => self.validate().is_ok(),
        }
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
            || (self.key.len() > 1
                && !self.key.starts_with("gamepad_")
                && self.key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
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
        self.label_parts(platform).join("+")
    }
    pub fn label_parts(&self, platform: Platform) -> Vec<String> {
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
        parts
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
    /// Use a tool or brush until release, then return to the previous one.
    Hold {
        action: Box<UiAction>,
    },
    /// Turn a mode on until release, then restore what it replaced.
    Momentary {
        action: Box<UiAction>,
    },
}
impl ShortcutAction {
    pub fn held(&self) -> bool {
        matches!(self, Self::Pan | Self::Hold { .. } | Self::Momentary { .. })
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
pub const GESTURE_TRIGGERS: [GestureTrigger; 6] = [
    GestureTrigger { id: "touch.tap.2", label: "Two-finger tap", default: "command.Undo", held: false },
    GestureTrigger { id: "touch.tap.3", label: "Three-finger tap", default: "command.Redo", held: false },
    GestureTrigger { id: "touch.tap.4", label: "Four-finger tap", default: "", held: false },
    GestureTrigger { id: "pen.button.primary", label: "Lower side button", default: "", held: true },
    GestureTrigger { id: "pen.button.secondary", label: "Upper side button", default: "", held: true },
    GestureTrigger { id: "pen.button.tertiary", label: "Third side button", default: "", held: true },
];
pub const MODIFIER_CAPTURE: &str = "modifier";
pub(crate) const MODIFIER_PREFIX: &str = "modifier:";
/// Holding the key uses an action until it's released, chosen per kind of tool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldKey {
    pub key: KeyChord,
    #[serde(default)]
    pub actions: std::collections::BTreeMap<ToolCategory, String>,
}
/// Commands that can be switched on only while a key or button is held.
pub const MOMENTARY_COMMANDS: [CommandId; 7] = [
    CommandId::SnapRulers,
    CommandId::ShowRulers,
    CommandId::FlipHorizontal,
    CommandId::FlipVertical,
    CommandId::ZenMode,
    CommandId::SoftProof,
    CommandId::PreviewSdr,
];
/// Tools the pen's eraser end can use in place of the current one.
pub const ERASER_END_TOOLS: [CommandId; 6] =
    [CommandId::Eraser, CommandId::Pen, CommandId::Pencil, CommandId::Brush, CommandId::Airbrush, CommandId::Blend];
/// The held variant of a press action, if it has one.
pub fn hold_id(target: &str) -> Option<String> {
    match target {
        "command.Eyedropper" => Some("hold.eyedropper".into()),
        "command.Eraser" => Some("hold.eraser".into()),
        "command.Move" => Some("hold.move".into()),
        "command.Hand" => Some("canvas.pan".into()),
        _ => holdable(target).then(|| format!("hold.{target}")),
    }
}
/// The press action a held binding belongs to.
pub fn hold_target(id: &str) -> Option<String> {
    match id {
        "hold.eyedropper" => Some("command.Eyedropper".into()),
        "hold.eraser" => Some("command.Eraser".into()),
        "hold.move" => Some("command.Move".into()),
        "canvas.pan" => Some("command.Hand".into()),
        _ => id.strip_prefix("hold.").filter(|target| holdable(target)).map(String::from),
    }
}
fn holdable(target: &str) -> bool {
    let command = |id: &str| CommandId::ALL.into_iter().find(|c| c.shortcut_id() == id);
    target == "color.transparent"
        || target.strip_prefix("brush.").is_some_and(|id| brush_catalog().any(|b| b.id.to_string() == id))
        || command(target).is_some_and(|c| CommandId::TOOLS.contains(&c) || MOMENTARY_COMMANDS.contains(&c))
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShortcutDefinition {
    pub id: String,
    pub label: String,
    pub action: ShortcutAction,
    #[serde(default)]
    pub repeat: bool,
    #[serde(default, skip_serializing_if = "BindingScope::is_application")]
    pub scope: BindingScope,
    /// Held variants name the press action they belong to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ShortcutRow {
    pub id: String,
    pub label: String,
    pub group: String,
    pub subgroup: String,
    pub detail: String,
    pub scope: String,
    pub bindings: Vec<Vec<String>>,
    pub gestures: Vec<String>,
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
    #[serde(default)]
    pub keys: Vec<String>,
    pub conflict: Option<String>,
    pub error: Option<String>,
    /// A new modifier key that already exists opens it instead.
    #[serde(default)]
    pub existing: bool,
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
    if matches!(id, "command.ClearSelected" | "command.DeleteRuler") {
        return vec![key("delete", false, false), key("backspace", false, false)];
    }
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
        "command.ActualPixels" => {
            return vec![key("1", true, false), KeyChord { key: "0".into(), command: true, shift: false, alt: true }];
        }
        "command.ZoomIn" => key("=", true, false),
        "command.ZoomOut" => key("-", true, false),
        "command.ZenMode" => key("tab", false, false),
        // Browser-owned F11 is already excluded by KeyChord::available(Web).
        "command.Fullscreen" => key("f11", false, false),
        "command.Undo" => key("z", true, false),
        "command.Redo" => return vec![key("z", true, true), key("y", true, false)],
        "command.FillSelection" => key("backspace", false, true),
        "command.CopySelectionToLayer" => key("j", true, false),
        "command.CutSelectionToLayer" => key("j", true, true),
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

pub const SHORTCUT_SECTIONS: [&str; 13] = [
    "Tools", "Painting", "Edit", "Select", "Transform", "Layer", "View", "Color", "File", "Window", "Help",
    "Brush presets", "Brush sizes",
];

fn command_section(command: CommandId) -> &'static str {
    use CommandId as C;
    match command {
        C::Undo | C::Redo | C::UndoWorkspace | C::RedoWorkspace | C::PasteImage | C::ClearLayer | C::FillSelection
        | C::ClearSelected | C::ClearOutside => "Edit",
        C::ApplyTransform | C::CancelTransform | C::PlacementOriginalSize | C::ResetTransform
        | C::TransformFlipHorizontal | C::TransformFlipVertical | C::TransformRotateLeft | C::TransformRotateRight
        | C::TransformFree | C::TransformUniform | C::TransformDistort | C::TransformPerspective | C::TransformNearest
        | C::TransformBilinear | C::TransformBicubic => "Transform",
        command if CommandId::TOOLS.contains(&command) => "Tools",
        C::TonalSelect | C::QuickMask | C::ReturnToArtwork | C::NewSelectionLayer | C::SaveSelectionLayer | C::Reselect
        | C::SelectionOutline | C::MaskOverlay | C::MaskOverlayProtected | C::ResetMaskColors | C::SwapMaskColors
        | C::FillSelectionMask | C::ClearSelectionMask | C::SelectionBrushPressure | C::SelectionNew | C::SelectionAdd
        | C::SelectionSubtract | C::SelectionIntersect | C::SelectionAntialias | C::SelectionConstrainAngles
        | C::SelectionFixedRatio | C::SelectionFixedSize | C::SelectionFromCenter | C::CompleteSelection
        | C::CancelSelection | C::SelectionVisible | C::SelectionEditing | C::SelectionReference | C::SelectAll
        | C::Deselect | C::InvertSelection | C::RemoveSelectionPoint | C::MaskSelection => "Select",
        C::AddLayer | C::DeleteLayer | C::RaiseLayer | C::LowerLayer | C::RasterizeSource | C::RepairSourceProfile
        | C::UseReferenceBelow | C::CopySelectionToLayer | C::CutSelectionToLayer | C::RevertToOriginal => "Layer",
        C::FitCanvas | C::ActualPixels | C::ZoomIn | C::ZoomOut | C::RotateLeft | C::RotateRight | C::FlipHorizontal | C::FlipVertical
        | C::ZenMode | C::Fullscreen | C::ShowRulers | C::SnapRulers | C::DeleteRuler | C::ShowCanvasActionBar
        | C::ToggleTheme => "View",
        C::SdrRendition | C::PreviewSdr | C::SoftProofSetup | C::SoftProof | C::GamutWarning | C::Histogram
        | C::AssignProfile | C::ConvertColorSpace | C::ChangeBitDepth => "Color",
        C::NewDocument | C::OpenDocument | C::SaveDocument | C::SaveDocumentAs | C::ExportDocument | C::CloseDocument
        | C::ImportImage | C::DocumentProperties | C::NewWindow | C::Drawings => "File",
        C::About | C::Website | C::SourceCode => "Help",
        _ => "Window",
    }
}

/// Where a command's keys apply; a more specific scope wins where it is enabled.
fn command_scope(command: CommandId) -> BindingScope {
    match command {
        CommandId::DeleteRuler => BindingScope::Tools {
            categories: vec![ToolCategory::ShapesRulers, ToolCategory::MoveTransform],
        },
        _ => BindingScope::Application,
    }
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
                    scope: command_scope(command),
                    target: None,
                },
                command_section(command),
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
                target: None,
            },
            "Tools",
        )
    }));
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
                    target: None,
                },
                "Painting",
            ));
        }
    }
    for (id, label, action, group) in [
        ("color.swap", "Swap colors", UiAction::Color { action: ColorAction::Swap }, "Painting"),
        ("color.transparent", "Paint with transparency", UiAction::Color { action: ColorAction::ToggleTransparent }, "Painting"),
        ("layer.duplicate", "Duplicate layer", UiAction::Layer { action: LayerAction::DuplicateSelected }, "Layer"),
        ("layer.group", "Group layers", UiAction::Layer { action: LayerAction::GroupSelected }, "Layer"),
    ] {
        rows.push((
            ShortcutDefinition {
                id: id.into(),
                label: label.into(),
                action: ShortcutAction::Action { action: Box::new(action) },
                repeat: false,
                scope: BindingScope::Application,
                target: None,
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
                target: None,
            },
            "Brush presets",
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
                target: None,
            },
            "Brush sizes",
        )
    }));
    let held: Vec<_> = rows.iter().filter_map(|(definition, section)| held(definition, section)).collect();
    rows.extend(held);
    rows
}
fn held(target: &ShortcutDefinition, section: &'static str) -> Option<(ShortcutDefinition, &'static str)> {
    use ToolCategory as C;
    let id = hold_id(&target.id)?;
    let ShortcutAction::Action { action } = &target.action else {
        return None;
    };
    let (label, action, scope) = match id.as_str() {
        "canvas.pan" => ("Pan while held".to_string(), ShortcutAction::Pan, BindingScope::Canvas),
        "hold.eyedropper" => (
            "Sample color while held".into(),
            ShortcutAction::Hold { action: action.clone() },
            BindingScope::Tools { categories: vec![C::Drawing, C::Blending, C::FillGradient] },
        ),
        "hold.eraser" => (
            "Erase while held".into(),
            ShortcutAction::Hold { action: action.clone() },
            BindingScope::Tools { categories: vec![C::Drawing, C::Blending] },
        ),
        "hold.move" => ("Move while held".into(), ShortcutAction::Hold { action: action.clone() }, BindingScope::Canvas),
        _ => {
            let momentary = match **action {
                UiAction::Color { action: ColorAction::ToggleTransparent } => {
                    Some(UiAction::Color { action: ColorAction::Select { slot: crate::ColorSlot::Transparent } })
                }
                UiAction::Invoke { command } if MOMENTARY_COMMANDS.contains(&command) => Some(UiAction::Invoke { command }),
                _ => None,
            };
            (
                format!("{} while held", target.label),
                match momentary {
                    Some(action) => ShortcutAction::Momentary { action: Box::new(action) },
                    None => ShortcutAction::Hold { action: action.clone() },
                },
                if target.id == "command.ZenMode" { BindingScope::Application } else { BindingScope::Canvas },
            )
        }
    };
    Some((ShortcutDefinition { id, label, action, repeat: false, scope, target: Some(target.id.clone()) }, section))
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
        let scope = self.scope_of(id);
        self.base_keys(id)
            .into_iter()
            .filter(|key| {
                !self.shortcuts.iter().any(|(other, keys)| keys.contains(key) && self.scope_of(other).overlaps(&scope))
            })
            .collect()
    }
    pub(crate) fn scope_of(&self, id: &str) -> BindingScope {
        static DEFAULTS: std::sync::LazyLock<std::collections::HashMap<String, BindingScope>> = std::sync::LazyLock::new(|| {
            [Platform::Gtk, Platform::Web, Platform::Android, Platform::Windows, Platform::Mac, Platform::Ios]
                .into_iter()
                .flat_map(definitions)
                .map(|(definition, _)| (definition.id, definition.scope))
                .collect()
        });
        DEFAULTS.get(id).cloned().unwrap_or_default()
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
        self.shortcut_matches(chord, platform, canvas).into_iter().next()
    }
    /// Every binding of `chord` that applies here, most specific scope first.
    /// Dispatch runs the first one that is enabled.
    pub(crate) fn shortcut_matches(
        &self,
        chord: &KeyChord,
        platform: Platform,
        canvas: Option<ToolCategory>,
    ) -> Vec<ShortcutDefinition> {
        if !chord.available(platform) {
            return Vec::new();
        }
        let mut matches: Vec<_> = definitions(platform)
            .into_iter()
            .map(|(definition, _)| definition)
            .filter(|definition| definition.target.is_none())
            .filter(|definition| definition.scope.applies(canvas) && self.keys(&definition.id).contains(chord))
            .collect();
        matches.reverse();
        matches.sort_by_key(|definition| std::cmp::Reverse(definition.scope.specificity()));
        matches
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
    /// Every other action this chord would also trigger in a shared context.
    pub(crate) fn conflicts(&self, id: &str, chord: &KeyChord, platform: Platform) -> Vec<ShortcutDefinition> {
        let all = definitions(platform);
        let scope = all.iter().find(|(d, _)| d.id == id).map(|(d, _)| d.scope.clone()).unwrap_or_default();
        let mut conflicts: Vec<_> = all
            .into_iter()
            .map(|(definition, _)| definition)
            .filter(|definition| {
                definition.target.is_none()
                    && definition.id != id
                    && definition.scope.overlaps(&scope)
                    && self.keys(&definition.id).contains(chord)
            })
            .collect();
        let press = definitions(platform).iter().any(|(d, _)| d.id == id && d.target.is_none());
        if press && self.hold_keys(platform).iter().any(|h| h.key == *chord && !h.actions.is_empty()) {
            conflicts.push(ShortcutDefinition {
                id: format!("{MODIFIER_PREFIX}{}", chord.label(platform)),
                label: format!("the modifier key {}", chord.label(platform)),
                action: ShortcutAction::Pan,
                repeat: false,
                scope: BindingScope::Canvas,
                target: None,
            });
        }
        conflicts
    }
    /// Modifier keys, from the artist's table or derived from the keymap.
    pub(crate) fn hold_keys(&self, platform: Platform) -> Vec<HoldKey> {
        self.hold_keys.clone().unwrap_or_else(|| self.default_hold_keys(platform))
    }
    pub(crate) fn default_hold_keys(&self, platform: Platform) -> Vec<HoldKey> {
        let mut table: Vec<HoldKey> = Vec::new();
        for (definition, _) in definitions(platform) {
            let Some(target) = &definition.target else {
                continue;
            };
            let categories = match &definition.scope {
                BindingScope::Tools { categories } => categories.clone(),
                _ => crate::shortcut_page::CONTEXTS.to_vec(),
            };
            for key in self.keys(&definition.id).into_iter().filter(KeyChord::holdable) {
                let index = table.iter().position(|h| h.key == key).unwrap_or_else(|| {
                    table.push(HoldKey { key, actions: Default::default() });
                    table.len() - 1
                });
                for category in &categories {
                    table[index].actions.entry(*category).or_insert_with(|| target.clone());
                }
            }
        }
        table
    }
    pub(crate) fn conflict(&self, id: &str, chord: &KeyChord, platform: Platform) -> Option<ShortcutDefinition> {
        self.conflicts(id, chord, platform).into_iter().next()
    }
    fn ambiguous(&self, id: &str, chord: &KeyChord, platform: Platform) -> bool {
        let scope = definitions(platform).into_iter().find(|(d, _)| d.id == id).map(|(d, _)| d.scope);
        self.conflicts(id, chord, platform).iter().any(|other| {
            !other.id.starts_with(MODIFIER_PREFIX) && scope.as_ref().is_some_and(|s| s.specificity() == other.scope.specificity())
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
    /// What a pen button does with each kind of tool, by press action id.
    pub(crate) fn pen_actions(&self, trigger: &str) -> std::collections::BTreeMap<ToolCategory, String> {
        if let Some(actions) = self.pen_buttons.get(trigger) {
            return actions.clone();
        }
        let bound = self.gesture_binding(trigger);
        let target = hold_target(bound).unwrap_or_else(|| bound.to_string());
        if target.is_empty() {
            return Default::default();
        }
        crate::shortcut_page::CONTEXTS.into_iter().map(|c| (c, target.clone())).collect()
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
        for (trigger, actions) in &self.pen_buttons {
            if !GESTURE_TRIGGERS.iter().any(|t| t.id == trigger && t.held) {
                return Err("Unknown pen button".into());
            }
            if actions.values().any(|id| !all.iter().any(|(d, _)| d.id == *id && d.target.is_none())) {
                return Err("Unknown pen button action".into());
            }
        }
        Ok(())
    }
    pub(crate) fn validate_shortcuts(&self) -> Result<(), String> {
        self.validate_gestures()?;
        let all = definitions(Platform::Gtk);
        for (index, hold) in self.hold_keys.iter().flatten().enumerate() {
            if !hold.key.holdable() {
                return Err("Modifier keys use Space, Shift, Ctrl or Alt".into());
            }
            if self.hold_keys.iter().flatten().take(index).any(|other| other.key == hold.key) {
                return Err("Each modifier key is listed once".into());
            }
            if hold.actions.values().any(|target| hold_id(target).is_none()) {
                return Err("This action can't be used while a key is held".into());
            }
        }
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
                if self.ambiguous(id, chord, Platform::Gtk) {
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
