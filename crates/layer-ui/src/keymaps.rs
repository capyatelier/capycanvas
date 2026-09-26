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
        source: "CapyCanvas defaults",
        links: &[],
        keys: &[],
        gestures: &[],
        differences: &[],
    },
    KeymapPreset {
        id: "photoshop",
        revision: 1,
        title: "Photoshop-inspired",
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
            ("layer.duplicate", &["primary+j"]),
            ("layer.group", &["primary+g"]),
            ("color.swap", &["x"]),
        ],
        gestures: &[],
        differences: &[
            ("R", "Photoshop's Rotate View tool has no Capy tool. Rotate with two fingers or the view rotation commands."),
            ("F", "Photoshop cycles three screen modes. Capy has one full-screen mode on F11."),
            ("D", "Photoshop resets black and white. Capy's D resets mask colors only."),
            ("Hold ~", "Photoshop erases with the current brush. Bind Erase while held to use Capy's eraser instead."),
            ("Primary+1", "Capy has no 100% zoom command."),
            ("Primary+E", "Capy has no merge-down command."),
            ("Shift+[ and Shift+]", "Capy has no brush hardness steps."),
            ("Alt+right-drag", "Capy has no on-canvas brush resize drag."),
        ],
    },
    KeymapPreset {
        id: "krita",
        revision: 1,
        title: "Krita-inspired",
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
            ("layer.duplicate", &["primary+j"]),
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
            ("+ and -", "Capy zooms with Primary+= and Primary+-."),
        ],
    },
    KeymapPreset {
        id: "clip-studio",
        revision: 1,
        title: "Clip Studio Paint-inspired",
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
            ("Primary+E", "Capy has no merge-down command."),
            ("Primary+Shift+P", "Browsers reserve this command search chord. On the web, open search from the toolbar."),
        ],
    },
    KeymapPreset {
        id: "procreate",
        revision: 1,
        title: "Procreate-inspired",
        source: "Procreate Handbook keyboard and gesture pages, current iPadOS; checked 2026-09-25",
        links: &[
            "https://help.procreate.com/procreate/handbook/interface-gestures/keyboard",
            "https://help.procreate.com/procreate/handbook/interface-gestures/gestures",
        ],
        keys: &[
            ("command.Lasso", &["s"]),
            ("command.ScaleRotate", &["v", "primary+t"]),
        ],
        gestures: &[("touch.tap.2", "command.Undo"), ("touch.tap.3", "command.Redo"), ("touch.tap.4", "command.ZenMode")],
        differences: &[
            ("Space", "Capy has no QuickMenu. Space pans the canvas while held."),
            ("L and C", "Capy has no commands that open the Layers or Colors panel."),
            ("X", "Procreate swaps the current and previous color. Capy's swap uses foreground and background."),
            ("Primary+A", "Procreate copies everything visible. Capy selects all."),
            ("Hold two or three fingers", "Capy does not repeat undo or redo while fingers are held."),
            ("Quick pinch", "Fit the canvas with Primary+0."),
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
    pub source: String,
    pub links: Vec<String>,
    pub outdated: bool,
    pub differences: Vec<KeymapDifference>,
    pub import: Option<KeymapImportPreview>,
}

#[derive(Clone, Debug)]
pub(crate) struct KeymapImport {
    pub settings: Settings,
    pub preview: KeymapImportPreview,
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
}

use crate::Settings;

pub(crate) fn view(settings: &Settings, import: Option<&KeymapImport>) -> KeymapView {
    let current = settings.keymap_preset().map_or(&KEYMAP_PRESETS[0], |p| p.preset);
    KeymapView {
        presets: KEYMAP_PRESETS.iter().map(|p| KeymapChoice { id: p.id.into(), title: p.title.into() }).collect(),
        selected: current.id.into(),
        title: current.title.into(),
        source: current.source.into(),
        links: current.links.iter().map(|l| l.to_string()).collect(),
        outdated: settings.keymap.as_ref().is_some_and(|k| k.revision < current.revision),
        differences: current
            .differences
            .iter()
            .map(|(trigger, note)| KeymapDifference { trigger: trigger.to_string(), note: note.to_string() })
            .collect(),
        import: import.map(|i| i.preview.clone()),
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
    })
    .expect("keymap serialization")
}

pub(crate) fn import(settings: &Settings, text: &str, platform: crate::Platform) -> Result<KeymapImport, String> {
    let file: KeymapFile = serde_json::from_str(text).map_err(|e| format!("This isn't a CapyCanvas keymap: {e}"))?;
    if file.format != KEYMAP_FORMAT {
        return Err("This isn't a CapyCanvas keymap".into());
    }
    if file.version > KEYMAP_VERSION {
        return Err("This keymap was made by a newer version of CapyCanvas".into());
    }
    let mut candidate = settings.clone();
    let mut unavailable = Vec::new();
    match file.keymap {
        Some(keymap) if preset(&keymap.id).is_some() => candidate.keymap = Some(keymap),
        Some(keymap) => unavailable.push(format!("Keymap “{}”", keymap.id)),
        None => {}
    }
    let definitions = crate::shortcuts::definitions(platform);
    let known = |id: &str| definitions.iter().any(|(d, _)| d.id == id);
    for (id, keys) in file.shortcuts {
        if !known(&id) {
            unavailable.push(id);
            continue;
        }
        for key in &keys {
            for other in candidate.shortcuts.values_mut() {
                other.retain(|k| k != key);
            }
        }
        candidate.shortcuts.insert(id, keys);
    }
    for (trigger, id) in file.gestures {
        if crate::GESTURE_TRIGGERS.iter().any(|t| t.id == trigger) && (id.is_empty() || known(&id)) {
            candidate.gestures.insert(trigger, id);
        } else {
            unavailable.push(format!("{trigger}: {id}"));
        }
    }
    candidate.validate()?;
    let label = |keys: Vec<KeyChord>| keys.iter().map(|k| k.label(platform)).collect::<Vec<_>>().join(" / ");
    let mut preview = KeymapImportPreview {
        title: candidate.keymap_preset().map_or(KEYMAP_PRESETS[0].title, |p| p.preset.title).into(),
        unavailable,
        ..Default::default()
    };
    for (definition, _) in crate::shortcuts::definitions(platform) {
        let (before, after) = (label(settings.keys(&definition.id)), label(candidate.keys(&definition.id)));
        match (before.is_empty(), after.is_empty()) {
            _ if before == after => {}
            (true, false) => preview.added.push(format!("{}: {after}", definition.label)),
            (false, true) => preview.removed.push(format!("{}: {before}", definition.label)),
            _ => preview.changed.push(format!("{}: {before} → {after}", definition.label)),
        }
    }
    for trigger in crate::GESTURE_TRIGGERS {
        let name = |s: &Settings| {
            let id = s.gesture_binding(trigger.id);
            definitions.iter().find(|(d, _)| d.id == id).map_or_else(|| "Nothing".to_string(), |(d, _)| d.label.clone())
        };
        let (before, after) = (name(settings), name(&candidate));
        if before != after {
            preview.changed.push(format!("{}: {before} → {after}", trigger.label));
        }
    }
    Ok(KeymapImport { settings: candidate, preview })
}
