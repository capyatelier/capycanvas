use crate::shortcuts::KeyChord;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeymapRef {
    pub id: String,
    pub revision: u32,
}

pub struct KeymapPreset {
    pub id: &'static str,
    pub revision: u32,
    pub title: &'static str,
    pub app: &'static str,
    pub source: &'static str,
    pub links: &'static [&'static str],
    keys: &'static [(&'static str, &'static [&'static str])],
    pub gestures: &'static [(&'static str, &'static str)],
    pub differences: &'static [(&'static str, &'static str)],
}

pub(crate) struct ParsedPreset {
    pub preset: &'static KeymapPreset,
    pub keys: Vec<(&'static str, Vec<KeyChord>)>,
}
impl ParsedPreset {
    pub fn keys_for(&self, id: &str) -> Option<&[KeyChord]> {
        self.keys.iter().find(|(i, _)| *i == id).map(|(_, k)| k.as_slice())
    }
    pub fn binds(&self, chord: &KeyChord) -> bool {
        self.keys.iter().any(|(_, keys)| keys.contains(chord))
    }
}

fn chord(spec: &str) -> KeyChord {
    let mut parts: Vec<_> = spec.split('+').collect();
    let key = match parts.pop().unwrap() {
        "" => "+",
        "space" => " ",
        key => key,
    };
    KeyChord {
        key: key.into(),
        command: parts.contains(&"primary"),
        shift: parts.contains(&"shift"),
        alt: parts.contains(&"alt"),
    }
}

pub(crate) static PARSED: LazyLock<Vec<ParsedPreset>> = LazyLock::new(|| {
    KEYMAP_PRESETS
        .iter()
        .map(|preset| ParsedPreset {
            preset,
            keys: preset.keys.iter().map(|(id, specs)| (*id, specs.iter().map(|s| chord(s)).collect())).collect(),
        })
        .collect()
});

pub(crate) fn preset(id: &str) -> Option<&'static ParsedPreset> {
    PARSED.iter().find(|p| p.preset.id == id)
}

pub const KEYMAP_PRESETS: &[KeymapPreset] = &[
    KeymapPreset {
        id: "capy",
        revision: 1,
        title: "CapyCanvas",
        app: "CapyCanvas",
        source: "CapyCanvas defaults",
        links: &[],
        keys: &[],
        gestures: &[],
        differences: &[],
    },
    KeymapPreset {
        id: "photoshop",
        revision: 10,
        title: "Photoshop Style",
        app: "Photoshop",
        source: "Adobe Photoshop default keyboard shortcuts, US layout, modern undo; checked 2026-09-25",
        links: &["https://helpx.adobe.com/content/dam/help/en/photoshop/using/default-keyboard-shortcuts/photoshop-keyboard-shortcuts.pdf"],
        keys: &[
            ("command.Redo", &["primary+shift+z"]),
            ("command.SoftProof", &["primary+y"]),
            ("command.Move", &["v"]),
            ("command.Lasso", &["l"]),
            ("command.RectangleSelect", &["m"]),
            ("command.Settings", &["primary+k"]),
            ("command.KeyboardShortcuts", &["primary+alt+shift+k"]),
            ("command.SearchCommands", &["primary+f"]),
            ("command.AddLayer", &["primary+shift+n"]),
            ("command.NewWindow", &[]),
            ("command.RaiseLayer", &["primary+]"]),
            ("command.LowerLayer", &["primary+["]),
            ("command.ExportDocument", &["primary+alt+shift+w"]),
            ("command.CopySelectionToLayer", &["primary+j"]),
            ("command.CutSelectionToLayer", &["primary+shift+j"]),
            ("command.ActualPixels", &["primary+1"]),
            ("command.CanvasSize", &["primary+alt+c"]),
            ("command.FeatherSelection", &["shift+f6"]),
            ("command.ImageSize", &["primary+alt+i"]),
            ("command.PasteInto", &["primary+alt+shift+v"]),
            ("command.MergeVisible", &["primary+shift+e"]),
            ("command.StampVisible", &["primary+alt+shift+e"]),
            ("command.Clone", &["s"]),
            ("command.SpotHeal", &["j"]),
            ("command.Heal", &["shift+j"]),
            ("tools.retouch", &[]),
            ("layer.group", &["primary+g"]),
            ("color.swap", &["x"]),
        ],
        gestures: &[],
        differences: &[
            ("R", "Photoshop's Rotate View tool has no Capy tool. Rotate with two fingers or the view rotation commands."),
            ("F", "Photoshop cycles three screen modes. Capy has one full-screen mode on F11."),
            ("D", "Photoshop resets black and white. Capy's D resets mask colors only."),
            ("Hold ~", "Photoshop erases with the current brush. Bind Erase while held to use Capy's eraser instead."),
            ("Shift+[ and Shift+]", "Capy has no brush hardness steps."),
            ("Alt+right-drag", "Capy has no on-canvas brush resize drag."),
        ],
    },
    KeymapPreset {
        id: "krita",
        revision: 2,
        title: "Krita Style",
        app: "Krita",
        source: "Krita 5.3 manual default shortcuts, US layout; checked 2026-09-25",
        links: &[
            "https://docs.krita.org/en/user_manual/getting_started/navigation.html",
            "https://docs.krita.org/en/reference_manual/main_menu/view_menu.html",
            "https://docs.krita.org/en/user_manual/introduction_from_other_software/introduction_from_photoshop.html",
        ],
        keys: &[
            ("command.Redo", &["primary+shift+z"]),
            ("command.SoftProof", &["primary+y"]),
            ("hold.eyedropper", &["control"]),
            ("command.Move", &["t"]),
            ("command.FlipHorizontal", &["m"]),
            ("command.Lasso", &[]),
            ("command.RotateLeft", &["4"]),
            ("command.RotateRight", &["6"]),
            ("command.Fullscreen", &["primary+shift+f"]),
            ("command.Deselect", &["primary+shift+a"]),
            ("command.AddLayer", &["insert"]),
            ("command.SearchCommands", &["primary+enter", "primary+k"]),
            ("command.CopySelectionToLayer", &["primary+j"]),
            ("command.CutSelectionToLayer", &["primary+shift+j"]),
            ("layer.group", &["primary+g"]),
            ("color.swap", &["x"]),
        ],
        gestures: &[],
        differences: &[
            ("E", "Krita toggles the brush's erase mode. Capy's E selects the eraser; press B to return."),
            ("Hold Shift+Space", "Krita rotates the view while held. Capy rotates with two fingers or 4 and 6."),
            ("5", "Capy has no view rotation reset command."),
            ("Hold Shift and drag", "Capy has no on-canvas brush resize drag."),
            ("/", "Capy has no command to swap to the previous brush preset."),
            ("+ and -", "Capy zooms with Ctrl+= and Ctrl+-."),
            ("Ctrl+J", "With a selection, Capy copies only the selected pixels to the new layer, as Ctrl+Alt+J does in Krita. Without one it duplicates the layer."),
        ],
    },
    KeymapPreset {
        id: "clip-studio",
        revision: 2,
        title: "Clip Studio Paint Style",
        app: "Clip Studio Paint",
        source: "Clip Studio Paint manual shortcut lists, Studio Mode defaults; checked 2026-09-25",
        links: &[
            "https://help.clip-studio.com/en-us/manual_en/780_shortcuts/Tool_Shortcuts.htm",
            "https://help.clip-studio.com/en-us/manual_en/780_shortcuts/Menu_Shortcuts.htm",
            "https://help.clip-studio.com/en-us/manual_en/780_shortcuts/Optional_Shortcuts.htm",
        ],
        keys: &[
            ("command.Move", &["k"]),
            ("command.Fill", &["g"]),
            ("command.Gradient", &[]),
            ("command.Settings", &["primary+k"]),
            ("command.KeyboardShortcuts", &["primary+alt+shift+k"]),
            ("command.SearchCommands", &["primary+shift+p"]),
            ("command.AddLayer", &["primary+shift+n"]),
            ("command.NewWindow", &[]),
            ("tool_setting.opacity.decrease", &["primary+["]),
            ("tool_setting.opacity.increase", &["primary+]"]),
            ("layer.group", &["primary+g"]),
            ("color.swap", &["x"]),
        ],
        gestures: &[],
        differences: &[
            ("G", "Clip Studio cycles fill and gradient tools. This keymap selects Fill."),
            ("R", "Clip Studio's Rotate tool has no Capy tool. Rotate with two fingers or the view rotation commands."),
            ("C", "Capy has no transparent-color toggle."),
            ("Hold Ctrl+Alt and drag", "Capy has no on-canvas brush resize drag."),
            ("[ and ]", "Clip Studio steps through preset sizes. Capy steps by the size setting's increment."),
            ("Ctrl+Shift+P", "Browsers reserve this command search chord. On the web, open search from the toolbar."),
        ],
    },
    KeymapPreset {
        id: "procreate",
        revision: 2,
        title: "Procreate Style",
        app: "Procreate",
        source: "Procreate Handbook keyboard and gesture pages, current iPadOS; checked 2026-09-25",
        links: &[
            "https://help.procreate.com/procreate/handbook/interface-gestures/keyboard",
            "https://help.procreate.com/procreate/handbook/interface-gestures/gestures",
        ],
        keys: &[
            ("command.Lasso", &["s"]),
            ("tools.retouch", &[]),
            ("command.ScaleRotate", &["v", "primary+t"]),
        ],
        gestures: &[("touch.tap.2", "command.Undo"), ("touch.tap.3", "command.Redo"), ("touch.tap.4", "command.ZenMode")],
        differences: &[
            ("Space", "Capy has no QuickMenu. Space pans the canvas while held."),
            ("L and C", "Capy has no commands that open the Layers or Colors panel."),
            ("X", "Procreate swaps the current and previous color. Capy's swap uses foreground and background."),
            ("Ctrl+A", "Procreate copies everything visible. Capy selects all."),
            ("Hold two or three fingers", "Capy does not repeat undo or redo while fingers are held."),
            ("Quick pinch", "Fit the canvas with Ctrl+0."),
        ],
    },
    KeymapPreset {
        id: "gimp",
        revision: 8,
        title: "GIMP Style",
        app: "GIMP",
        source: "GIMP 3.2 default shortcuts, US layout; checked 2026-09-26",
        links: &[
            "https://docs.gimp.org/3.2/en/gimp-tools-paint.html",
            "https://gitlab.gnome.org/GNOME/gimp/-/tree/GIMP_3_2_6/app/actions",
            "https://gitlab.gnome.org/GNOME/gimp/-/tree/GIMP_3_2_6/app/tools",
        ],
        keys: &[
            ("command.Redo", &["primary+y"]),
            ("command.SearchCommands", &["/"]),
            ("command.ImportImage", &["primary+alt+o"]),
            ("command.DocumentProperties", &["alt+enter"]),
            ("hold.eyedropper", &["control"]),
            ("command.Eraser", &["shift+e"]),
            ("command.EllipseSelect", &["e"]),
            ("command.RectangleSelect", &["r"]),
            ("command.Lasso", &["f"]),
            ("command.AutoSelect", &["u"]),
            ("command.Figure", &[]),
            ("command.ColorSelect", &["shift+o"]),
            ("command.Move", &["m"]),
            ("command.ScaleRotate", &["shift+t", "shift+s", "shift+r"]),
            ("command.Crop", &["shift+c"]),
            ("command.SelectionOutline", &["primary+t"]),
            ("command.Eyedropper", &["o"]),
            ("command.Fill", &["shift+b"]),
            ("command.Brush", &["p"]),
            ("tools.ink", &[]),
            ("command.Pencil", &["n"]),
            ("command.Airbrush", &["a"]),
            ("command.Pen", &["k"]),
            ("command.Blend", &["s"]),
            ("command.Liquify", &["w"]),
            ("command.Clone", &["c"]),
            ("command.Heal", &["h"]),
            ("tools.retouch", &[]),
            ("command.QuickMask", &["shift+q"]),
            ("command.Deselect", &["primary+shift+a"]),
            ("command.InvertSelection", &["primary+i"]),
            ("command.Reselect", &[]),
            ("command.FitCanvas", &["primary+shift+j"]),
            ("command.ActualPixels", &["1"]),
            ("command.AddLayer", &["primary+shift+n"]),
            ("command.NewWindow", &[]),
            ("command.FillSelection", &["primary+,"]),
            ("command.ClearSelected", &["delete"]),
            ("command.DeleteRuler", &["delete", "backspace"]),
            ("command.CopySelectionToLayer", &[]),
            ("command.CutSelectionToLayer", &[]),
            ("command.PasteInPlace", &["primary+alt+v"]),
            ("command.MergeDown", &[]),
            ("command.MergeVisible", &["primary+m"]),
            ("command.Settings", &[]),
            ("layer.duplicate", &["primary+shift+d"]),
            ("color.swap", &["x"]),
        ],
        gestures: &[],
        differences: &[
            ("D", "GIMP resets black and white. Capy's D resets mask colors only."),
            ("Ctrl+Shift+Z and Ctrl+Shift+Y", "Capy has no strong undo or redo. Ctrl+Shift+Y toggles Capy's gamut warning."),
            ("+ and -", "Capy zooms with Ctrl+= and Ctrl+-."),
            ("Ctrl+E", "Capy has no quick re-export. Export with Ctrl+Shift+E; Merge Down has no key in this keymap."),
            ("{ and }", "Capy has no large brush size steps."),
            ("B", "Capy has no Paths tool. B keeps Capy's paint tools."),
            ("Ctrl+Shift+V", "Capy has no Paste as New Image. Create a drawing, then paste into it."),
        ],
    },
    KeymapPreset {
        id: "affinity",
        revision: 8,
        title: "Affinity Style",
        app: "Affinity",
        source: "Affinity Help Center shortcut lists, Pixel Studio, September 2026 release, Windows keys; checked 2026-09-26",
        links: &[
            "https://www.affinity.studio/help/workspace-shortcuts-pixel-studio/",
            "https://www.affinity.studio/help/workspace-shortcuts-editing/",
            "https://www.affinity.studio/help/workspace-shortcuts-workspace/",
            "https://www.affinity.studio/help/workspace-feature-finder/",
        ],
        keys: &[
            ("command.Redo", &["primary+shift+z"]),
            ("command.SearchCommands", &["primary+alt+shift+f"]),
            ("command.ImportImage", &["primary+shift+m"]),
            ("command.ExportDocument", &["primary+alt+shift+w"]),
            ("hold.move", &["control"]),
            ("command.Move", &["v"]),
            ("command.Lasso", &["l"]),
            ("command.RectangleSelect", &["m"]),
            ("command.Brush", &["b"]),
            ("tools.paint", &[]),
            ("command.AddLayer", &["primary+shift+n"]),
            ("command.NewWindow", &[]),
            ("command.RaiseLayer", &["primary+]"]),
            ("command.LowerLayer", &["primary+["]),
            ("command.FillSelection", &["alt+backspace"]),
            ("command.CopySelectionToLayer", &["primary+j"]),
            ("command.CutSelectionToLayer", &["primary+shift+j"]),
            ("command.ActualPixels", &["primary+1"]),
            ("command.CanvasSize", &["primary+alt+c"]),
            ("command.ImageSize", &["primary+alt+i"]),
            ("command.MergeVisible", &["primary+shift+e"]),
            ("command.StampVisible", &["primary+alt+shift+e"]),
            ("command.Clone", &["s"]),
            ("command.SpotHeal", &["j"]),
            ("command.Heal", &["shift+j"]),
            ("tools.retouch", &[]),
            ("layer.group", &["primary+g"]),
            ("color.swap", &["shift+x"]),
        ],
        gestures: &[],
        differences: &[
            ("X", "Affinity switches the active color selector. Capy swaps colors with Shift+X."),
            ("D", "Affinity resets black and white. Capy's D resets mask colors only."),
            ("G, M and W", "Affinity cycles tool groups. This keymap selects Gradient, Rectangle select and Auto select; Fill stays on F."),
            ("Number keys", "Capy has no number-key opacity shortcuts."),
            ("Shift+[ and Shift+]", "Capy has no brush hardness steps."),
            ("Ctrl+Alt+drag", "Capy has no on-canvas brush resize drag."),
        ],
    },
];

#[derive(Clone, Debug, Serialize)]
pub struct KeymapChoice {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct KeymapDifference {
    pub trigger: String,
    pub note: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct KeymapImportPreview {
    pub title: String,
    pub heading: String,
    pub body: String,
    pub cancel_label: std::sync::Arc<str>,
    pub import_label: std::sync::Arc<str>,
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
    pub unavailable: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct KeymapView {
    pub presets: Vec<KeymapChoice>,
    pub selected: String,
    pub title: String,
    pub app: String,
    pub source: String,
    pub links: Vec<String>,
    pub outdated: bool,
    pub differences: Vec<KeymapDifference>,
    pub import: Option<KeymapImportPreview>,
    pub details: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct KeymapImport {
    pub settings: Settings,
    pub preview: KeymapImportPreview,
    copy: KeymapPreviewCopy,
    platform: crate::Platform,
}

#[derive(Clone, Debug)]
pub(crate) enum KeymapImportError {
    InvalidDetail(String),
    Message(crate::MessageId),
    Literal(String),
}
impl KeymapImportError {
    pub(crate) fn message(&self, localization: &crate::Localizer) -> String {
        match self {
            Self::InvalidDetail(detail) => keymap_copy(localization, crate::MessageId::SETTINGS_KEYMAP_INVALID_DETAIL, &[("detail", detail.clone())]),
            Self::Message(message) => localization.text(*message).to_string(),
            Self::Literal(message) => message.clone(),
        }
    }
}

#[derive(Clone, Debug)]
enum KeymapPreviewValue {
    Literal(String),
    Label(crate::shortcuts::ShortcutLabel),
    Keys(Vec<KeyChord>),
}
impl KeymapPreviewValue {
    fn text(&self, platform: crate::Platform, localization: &crate::Localizer) -> String {
        match self {
            Self::Literal(text) => text.clone(),
            Self::Label(label) => label.resolve(localization),
            Self::Keys(keys) => keys.iter().map(|key| key.localized_label(platform, localization)).collect::<Vec<_>>().join(" / "),
        }
    }
}

#[derive(Clone, Debug)]
enum KeymapPreviewEntry {
    Literal(String),
    UnavailableName(String),
    Binding { action: KeymapPreviewValue, keys: KeymapPreviewValue },
    Change { action: KeymapPreviewValue, before: KeymapPreviewValue, after: KeymapPreviewValue },
}
impl KeymapPreviewEntry {
    fn text(&self, platform: crate::Platform, localization: &crate::Localizer) -> String {
        match self {
            Self::Literal(text) => text.clone(),
            Self::UnavailableName(name) => keymap_copy(localization, crate::MessageId::SETTINGS_KEYMAP_UNAVAILABLE_NAME, &[("name", name.clone())]),
            Self::Binding { action, keys } => keymap_copy(localization, crate::MessageId::SETTINGS_KEYMAP_PREVIEW_BINDING, &[("action", action.text(platform, localization)), ("keys", keys.text(platform, localization))]),
            Self::Change { action, before, after } => keymap_copy(localization, crate::MessageId::SETTINGS_KEYMAP_PREVIEW_CHANGE, &[("action", action.text(platform, localization)), ("before", before.text(platform, localization)), ("after", after.text(platform, localization))]),
        }
    }
}

#[derive(Clone, Debug)]
struct KeymapPreviewCopy {
    preset: &'static str,
    added: Vec<KeymapPreviewEntry>,
    changed: Vec<KeymapPreviewEntry>,
    removed: Vec<KeymapPreviewEntry>,
    unavailable: Vec<KeymapPreviewEntry>,
}

impl KeymapImport {
    pub(crate) fn set_localization(&mut self, localization: &crate::Localizer) {
        let project = |entries: &[KeymapPreviewEntry]| entries.iter().map(|entry| entry.text(self.platform, localization)).collect();
        self.preview = KeymapImportPreview {
            title: preset(self.copy.preset).unwrap().preset.localized_title(localization).to_string(),
            added: project(&self.copy.added), changed: project(&self.copy.changed),
            removed: project(&self.copy.removed), unavailable: project(&self.copy.unavailable),
            ..Default::default()
        };
        let preview = &mut self.preview;
        preview.heading = keymap_copy(localization, crate::MessageId::SETTINGS_KEYMAP_PREVIEW_HEADING, &[("name", preview.title.clone())]);
        preview.cancel_label = localization.text(crate::MessageId::COMMON_CANCEL);
        preview.import_label = localization.text(crate::MessageId::COMMON_IMPORT);
        let mut lines = Vec::new();
        for (message, items) in [(crate::MessageId::SETTINGS_KEYMAP_PREVIEW_ADDED, &preview.added), (crate::MessageId::SETTINGS_KEYMAP_PREVIEW_CHANGED, &preview.changed), (crate::MessageId::SETTINGS_KEYMAP_PREVIEW_REMOVED, &preview.removed), (crate::MessageId::SETTINGS_KEYMAP_PREVIEW_UNAVAILABLE, &preview.unavailable)] {
            if !items.is_empty() {
                let mut args = crate::FluentArgs::new(); args.set("count", items.len());
                lines.push(localization.format(message, &args));
                lines.extend(items.iter().take(6).map(|item| keymap_copy(localization, crate::MessageId::SETTINGS_KEYMAP_PREVIEW_ENTRY, &[("item", item.clone())])));
            }
        }
        preview.body = if lines.is_empty() { localization.text(crate::MessageId::SETTINGS_KEYMAP_PREVIEW_NONE).to_string() } else { lines.join("\n") };
    }
}

pub const KEYMAP_FORMAT: &str = "capycanvas-keymap";
pub const KEYMAP_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeymapFile {
    format: String,
    version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    keymap: Option<KeymapRef>,
    #[serde(default)]
    shortcuts: std::collections::BTreeMap<String, Vec<KeyChord>>,
    #[serde(default)]
    gestures: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    modifiers: Option<Vec<crate::shortcuts::HoldKey>>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pen_buttons: std::collections::BTreeMap<String, std::collections::BTreeMap<crate::ToolCategory, String>>,
}

use crate::Settings;

pub(crate) fn view(settings: &Settings, import: Option<&KeymapImport>, details: bool) -> KeymapView {
    let current = settings.keymap_preset().map_or(&KEYMAP_PRESETS[0], |p| p.preset);
    KeymapView {
        presets: KEYMAP_PRESETS.iter().map(|p| KeymapChoice { id: p.id.into(), title: p.title.into() }).collect(),
        selected: current.id.into(),
        title: current.title.into(),
        app: current.app.into(),
        source: current.source.into(),
        links: current.links.iter().map(|l| l.to_string()).collect(),
        outdated: settings.keymap.as_ref().is_some_and(|k| k.revision < current.revision),
        differences: current
            .differences
            .iter()
            .map(|(trigger, note)| KeymapDifference { trigger: trigger.to_string(), note: note.to_string() })
            .collect(),
        import: import.map(|i| i.preview.clone()),
        details,
    }
}

pub(crate) fn select(settings: &mut Settings, id: &str) -> Result<(), String> {
    let preset = preset(id).ok_or("Unknown keymap")?.preset;
    settings.keymap = (preset.id != KEYMAP_PRESETS[0].id).then(|| KeymapRef { id: preset.id.into(), revision: preset.revision });
    Ok(())
}

pub(crate) fn export(settings: &Settings) -> String {
    serde_json::to_string_pretty(&KeymapFile {
        format: KEYMAP_FORMAT.into(),
        version: KEYMAP_VERSION,
        keymap: settings.keymap.clone(),
        shortcuts: settings.shortcuts.clone(),
        gestures: settings.gestures.clone(),
        modifiers: settings.hold_keys.clone(),
        pen_buttons: settings.pen_buttons.clone(),
    })
    .expect("keymap serialization")
}

pub(crate) fn import_typed(settings: &Settings, text: &str, platform: crate::Platform, localization: &crate::Localizer) -> Result<KeymapImport, KeymapImportError> {
    use KeymapPreviewEntry as Entry;
    use KeymapPreviewValue as Value;
    let file: KeymapFile = serde_json::from_str(text).map_err(|error| KeymapImportError::InvalidDetail(error.to_string()))?;
    if file.format != KEYMAP_FORMAT { return Err(KeymapImportError::Message(crate::MessageId::SETTINGS_KEYMAP_INVALID)); }
    if file.version > KEYMAP_VERSION { return Err(KeymapImportError::Message(crate::MessageId::SETTINGS_KEYMAP_NEWER_VERSION)); }
    let mut candidate = settings.clone();
    let mut unavailable = Vec::new();
    match file.keymap {
        Some(keymap) if preset(&keymap.id).is_some() => candidate.keymap = Some(keymap),
        Some(keymap) => unavailable.push(Entry::UnavailableName(keymap.id)),
        None => {}
    }
    let definitions = crate::shortcuts::definitions(platform);
    let known = |id: &str| definitions.iter().any(|(definition, _)| definition.id == id);
    for (id, keys) in file.shortcuts {
        if !known(&id) { unavailable.push(Entry::Literal(id)); continue; }
        for key in &keys {
            for other in candidate.shortcuts.values_mut() { other.retain(|other| other != key); }
        }
        candidate.shortcuts.insert(id, keys);
    }
    if let Some(mut modifiers) = file.modifiers {
        for hold in &mut modifiers {
            hold.actions.retain(|_, target| {
                let available = crate::shortcuts::hold_id(target).is_some_and(|id| known(&id));
                if !available {
                    unavailable.push(Entry::Binding { action: Value::Keys(vec![hold.key.clone()]), keys: Value::Literal(target.clone()) });
                }
                available
            });
        }
        candidate.hold_keys = Some(modifiers);
    }
    for (trigger, mut actions) in file.pen_buttons {
        actions.retain(|_, id| known(id));
        candidate.gestures.remove(&trigger);
        candidate.pen_buttons.insert(trigger, actions);
    }
    for (trigger, id) in file.gestures {
        if crate::GESTURE_TRIGGERS.iter().any(|candidate| candidate.id == trigger) && (id.is_empty() || known(&id)) {
            candidate.gestures.insert(trigger, id);
        } else {
            unavailable.push(Entry::Binding { action: Value::Literal(trigger), keys: Value::Literal(id) });
        }
    }
    candidate.validate_localized(localization).map_err(KeymapImportError::Literal)?;
    let mut copy = KeymapPreviewCopy {
        preset: candidate.keymap_preset().map_or(KEYMAP_PRESETS[0].id, |preset| preset.preset.id),
        added: Vec::new(), changed: Vec::new(), removed: Vec::new(), unavailable,
    };
    for (definition, _) in &definitions {
        let (before, after) = (settings.keys(&definition.id), candidate.keys(&definition.id));
        let action = Value::Label(definition.label.clone());
        match (before.is_empty(), after.is_empty()) {
            _ if before == after => {}
            (true, false) => copy.added.push(Entry::Binding { action, keys: Value::Keys(after) }),
            (false, true) => copy.removed.push(Entry::Binding { action, keys: Value::Keys(before) }),
            _ => copy.changed.push(Entry::Change { action, before: Value::Keys(before), after: Value::Keys(after) }),
        }
    }
    for trigger in crate::GESTURE_TRIGGERS {
        let binding = |settings: &Settings| definitions.iter().find(|(definition, _)| definition.id == settings.gesture_binding(trigger.id))
            .map_or(crate::shortcuts::ShortcutLabel::Message(crate::MessageId::SHORTCUT_NOTHING), |(definition, _)| definition.label.clone());
        let (before, after) = (binding(settings), binding(&candidate));
        if before != after {
            copy.changed.push(Entry::Change { action: Value::Label(crate::shortcuts::ShortcutLabel::Message(trigger.label)), before: Value::Label(before), after: Value::Label(after) });
        }
    }
    let mut imported = KeymapImport { settings: candidate, preview: Default::default(), copy, platform };
    imported.set_localization(localization);
    Ok(imported)
}

fn keymap_copy(localization: &crate::Localizer, message: crate::MessageId, values: &[(&str, String)]) -> String {
    let mut args = crate::FluentArgs::new(); for (key, value) in values { args.set(*key, value.as_str()); }
    localization.format(message, &args)
}
impl KeymapPreset {
    pub fn localized_title(&self, localization: &crate::Localizer) -> std::sync::Arc<str> {
        localization.text(match self.id {
            "photoshop" => crate::MessageId::SETTINGS_KEYMAP_TITLE_PHOTOSHOP,
            "krita" => crate::MessageId::SETTINGS_KEYMAP_TITLE_KRITA,
            "clip-studio" => crate::MessageId::SETTINGS_KEYMAP_TITLE_CLIPSTUDIO,
            "procreate" => crate::MessageId::SETTINGS_KEYMAP_TITLE_PROCREATE,
            "gimp" => crate::MessageId::SETTINGS_KEYMAP_TITLE_GIMP,
            "affinity" => crate::MessageId::SETTINGS_KEYMAP_TITLE_AFFINITY,
            _ => crate::MessageId::SETTINGS_KEYMAP_TITLE_CAPY,
        })
    }
}
pub fn shortcut_default_caption(localization: &crate::Localizer, keys: &[String]) -> String {
    if keys.is_empty() { return localization.text(crate::MessageId::SETTINGS_SHORTCUT_DEFAULT_EMPTY).to_string(); }
    keymap_copy(localization, crate::MessageId::SETTINGS_SHORTCUT_DEFAULT, &[("keys", keys.join(" / "))])
}
pub fn modifier_hold_help(localization: &crate::Localizer, label: &str) -> String {
    keymap_copy(localization, crate::MessageId::SETTINGS_MODIFIER_HOLD_HELP, &[("label", label.to_owned())])
}

#[cfg(test)]
mod copy_tests {
    use super::*;
    #[test]
    fn keymap_language_refresh_retains_candidate_and_unavailable_source() {
        let english = crate::Localizer::shared(crate::UiLanguage::English);
        let japanese = crate::Localizer::shared(crate::UiLanguage::Japanese);
        let settings = Settings::default();
        let source = serde_json::json!({"format": KEYMAP_FORMAT, "version": KEYMAP_VERSION,
            "keymap": {"id": "未知 { $name } 🖌", "revision": 1},
            "shortcuts": {"unknown.literal": []}, "gestures": {"unknown.trigger": "unknown.action"}}).to_string();
        let mut imported = import_typed(&settings, &source, crate::Platform::Gtk, &english).unwrap();
        let candidate = serde_json::to_value(&imported.settings).unwrap();
        let before = imported.preview.clone();
        imported.set_localization(&japanese);
        assert_eq!(serde_json::to_value(&imported.settings).unwrap(), candidate);
        assert_eq!(imported.preview.unavailable.len(), before.unavailable.len());
        assert!(imported.preview.unavailable.iter().any(|text| text.contains("未知 { $name } 🖌")));
        assert_ne!(imported.preview.heading, before.heading);
        assert_ne!(imported.preview.body, before.body);
        imported.set_localization(&english);
        assert_eq!(imported.preview, before);
    }

    #[test]
    fn keymap_import_retains_complete_preview_and_stable_preset_identity() {
        let localization = crate::Localizer::shared(crate::UiLanguage::English);
        for preset in KEYMAP_PRESETS { assert_eq!(preset.localized_title(&localization).as_ref(), preset.title); }
        let settings = Settings::default();
        let file = serde_json::json!({"format": KEYMAP_FORMAT, "version": KEYMAP_VERSION, "keymap": {"id": "clip-studio", "revision": 1}, "shortcuts": {}}).to_string();
        let imported = import_typed(&settings, &file, crate::Platform::Gtk, &localization).unwrap();
        assert_eq!(imported.settings.keymap.as_ref().unwrap().id, "clip-studio");
        assert_eq!(imported.preview.heading, "Import Clip Studio Paint Style?");
        assert_eq!(imported.preview.cancel_label.as_ref(), "Cancel");
        assert_eq!(imported.preview.import_label.as_ref(), "Import");
        assert!(!imported.preview.body.is_empty());
        assert_eq!(shortcut_default_caption(&localization, &[]), "Default: none");
        assert_eq!(shortcut_default_caption(&localization, &["⌘K".into(), "Shift+Q".into()]), "Default: ⌘K / Shift+Q");
        let literal = "键 {label} 🖌";
        assert_eq!(modifier_hold_help(&localization, literal), format!("Hold {literal} to use an action until you let go."));
        assert_eq!(view(&imported.settings, Some(&imported), false).import.unwrap(), imported.preview);
    }
}
