fn chord(key: &str, command: bool, shift: bool, alt: bool) -> KeyChord {
    KeyChord { key: key.into(), command, shift, alt }
}

fn bound(settings: &Settings, chord: &KeyChord) -> Option<String> {
    settings.shortcut_match(chord, Platform::Gtk, Some(ToolCategory::Drawing)).map(|d| d.id)
}

#[test]
fn every_keymap_preset_is_valid_and_conflict_free() {
    for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Windows, Platform::Mac] {
        let ids: Vec<_> = crate::shortcuts::definitions(platform).into_iter().map(|(d, _)| d.id).collect();
        for preset in crate::keymaps::KEYMAP_PRESETS {
            let mut settings = Settings::default();
            crate::keymaps::select(&mut settings, preset.id).unwrap();
            settings.validate().unwrap_or_else(|e| panic!("{} on {platform:?}: {e}", preset.id));
            let parsed = crate::keymaps::preset(preset.id).unwrap();
            for (id, keys) in &parsed.keys {
                if platform == Platform::Gtk {
                    assert!(ids.iter().any(|i| i == id), "{} binds unknown {id}", preset.id);
                }
                for key in keys {
                    key.validate_for(settings.held_shortcut(id, Platform::Gtk)).unwrap();
                    assert!(settings.conflict(id, key, Platform::Gtk).is_none(), "{} {id} {key:?}", preset.id);
                }
            }
            for (trigger, id) in preset.gestures {
                assert!(GESTURE_TRIGGERS.iter().any(|t| t.id == *trigger));
                assert!(ids.iter().any(|i| i == id));
            }
            assert!(preset.id == "capy" || (!preset.source.is_empty() && !preset.links.is_empty()));
            for (trigger, note) in preset.differences {
                assert!(!trigger.is_empty() && note.ends_with('.'), "{} {trigger}", preset.id);
            }
        }
    }
}

#[test]
fn presets_layer_between_defaults_and_user_overrides() {
    let mut settings = Settings::default();
    let redo_y = chord("y", true, false, false);
    assert_eq!(bound(&settings, &chord("o", false, false, false)).as_deref(), Some("command.Move"));
    assert_eq!(bound(&settings, &redo_y).as_deref(), Some("command.Redo"));
    settings.shortcuts.insert(CommandId::Undo.shortcut_id(), vec![chord("f13", false, false, false)]);
    crate::keymaps::select(&mut settings, "photoshop").unwrap();
    for (key, id) in [
        (chord("v", false, false, false), "command.Move"),
        (chord("m", false, false, false), "command.RectangleSelect"),
        (chord("l", false, false, false), "command.Lasso"),
        (chord("x", false, false, false), "color.swap"),
        (chord("y", true, false, false), "command.SoftProof"),
        (chord("n", true, true, false), "command.AddLayer"),
        (chord("k", true, false, false), "command.Settings"),
        (chord("f13", false, false, false), "command.Undo"),
    ] {
        assert_eq!(bound(&settings, &key).as_deref(), Some(id), "{key:?}");
    }
    assert_eq!(bound(&settings, &chord("o", false, false, false)), None, "the preset replaces Move's key");
    assert!(settings.keys("command.NewWindow").is_empty());
    assert!(!settings.shortcut_modified("command.Move"), "preset bindings are the new baseline");
    assert!(settings.shortcut_modified(&CommandId::Undo.shortcut_id()));
    crate::keymaps::select(&mut settings, "capy").unwrap();
    assert_eq!(settings.keymap, None);
    assert_eq!(bound(&settings, &chord("f13", false, false, false)).as_deref(), Some("command.Undo"), "overrides survive preset changes");
    assert_eq!(bound(&settings, &redo_y).as_deref(), Some("command.Redo"));
    assert!(crate::keymaps::select(&mut settings, "nope").is_err());
    settings.keymap = Some(crate::keymaps::KeymapRef { id: "nope".into(), revision: 1 });
    assert!(settings.validate().is_err());
}

#[test]
fn krita_keymap_samples_with_ctrl_without_breaking_ctrl_chords() {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    preference(&mut s, PreferenceAction::SelectKeymap { id: "krita".into() });
    s.dispatch(UiAction::CloseSettings).unwrap();
    s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
    s.pen(event(&s, 2, PenPhase::Up, 1.)).unwrap();
    s.frame(3, 3).unwrap();
    let preset = s.state.brush.preset;
    held_key(&mut s, "Control_L", true, Modifiers::default());
    assert!(sampling(&s), "held Ctrl samples color");
    held_key(&mut s, "z", true, Modifiers { command: true, ..Modifiers::default() });
    assert!(s.command(CommandId::Redo).enabled, "Ctrl+Z still undoes");
    held_key(&mut s, "z", false, Modifiers { command: true, ..Modifiers::default() });
    held_key(&mut s, "Control_L", false, Modifiers { command: true, ..Modifiers::default() });
    assert!(painting_with(&s, preset));
    held_key(&mut s, "Alt_L", true, Modifiers::default());
    assert!(!sampling(&s), "Krita's keymap moves sampling off Alt");
    held_key(&mut s, "Alt_L", false, alt());
    held_key(&mut s, "m", true, Modifiers::default());
    assert!(s.state.camera.flipped[0], "M mirrors the view");
}

#[test]
fn procreate_keymap_changes_gesture_defaults_and_resets_to_them() {
    let mut s = session(Platform::Android);
    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    preference(&mut s, PreferenceAction::SelectKeymap { id: "procreate".into() });
    assert_eq!(s.state.settings.gesture_binding("touch.tap.4"), "command.ZenMode");
    edit_preference(&mut s, PreferenceId::FourFingerTap, PreferenceValue::Choice(0));
    assert_eq!(s.state.settings.gesture_binding("touch.tap.4"), "");
    let row = s.state.settings.field(PreferenceId::FourFingerTap, Platform::Android).unwrap();
    assert!(row.reset.as_ref().unwrap().enabled);
    preference(&mut s, PreferenceAction::Reset { id: PreferenceId::FourFingerTap });
    assert_eq!(s.state.settings.gesture_binding("touch.tap.4"), "command.ZenMode");
    let row = s.state.settings.field(PreferenceId::FourFingerTap, Platform::Android).unwrap();
    assert!(!row.reset.as_ref().unwrap().enabled);
    let view = s.preferences().unwrap().keymap;
    assert_eq!(view.selected, "procreate");
    assert!(view.differences.iter().any(|d| d.trigger == "Space"));
    assert!(!view.outdated);
}

#[test]
fn keymap_files_round_trip_with_a_preview() {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    preference(&mut s, PreferenceAction::SelectKeymap { id: "clip-studio".into() });
    s.state.settings.shortcuts.insert(CommandId::Undo.shortcut_id(), vec![chord("volumedown", false, false, false)]);
    s.state.settings.gestures.insert("pen.button.primary".into(), "hold.eyedropper".into());
    let exported = crate::keymaps::export(&s.state.settings);
    let value: serde_json::Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(value["format"], "capycanvas-keymap");
    assert_eq!(value["version"], 1);
    assert_eq!(value["keymap"]["id"], "clip-studio");
    assert_eq!(crate::keymaps::export(&s.state.settings), exported, "exports are deterministic");
    let before = s.state.settings.clone();
    s.state.settings = Settings::default();
    s.state.settings.shortcuts.insert(CommandId::Redo.shortcut_id(), vec![chord("volumedown", false, false, false)]);
    let mut file: serde_json::Value = value.clone();
    file["shortcuts"]["command.FutureThing"] = serde_json::json!([{"key": "q", "command": false, "shift": false, "alt": false}]);
    preference(&mut s, PreferenceAction::ImportKeymap { text: file.to_string() });
    let preview = s.preferences().unwrap().keymap.import.unwrap();
    assert_eq!(preview.title, "Clip Studio Paint-inspired");
    assert_eq!(preview.unavailable, ["command.FutureThing"]);
    assert!(preview.changed.iter().any(|c| c.starts_with("Undo:")), "{preview:?}");
    assert!(preview.changed.iter().any(|c| c.starts_with("Pen side button: Nothing →")), "{preview:?}");
    assert!(preview.changed.iter().any(|c| c.starts_with(&format!("{}: O → K", CommandId::Move.label()))), "{preview:?}");
    assert!(preview.added.iter().any(|c| c == "Swap colors: X"), "{preview:?}");
    assert!(preview.removed.iter().any(|c| c == "Redo: Volume Down"), "{preview:?}");
    assert_eq!(s.state.settings.keymap, None, "nothing changes before confirmation");
    preference(&mut s, PreferenceAction::ConfirmKeymapImport);
    assert!(s.preferences().unwrap().keymap.import.is_none());
    assert_eq!(s.state.settings.keymap, before.keymap);
    assert_eq!(s.state.settings.keys(&CommandId::Undo.shortcut_id()), before.keys(&CommandId::Undo.shortcut_id()));
    assert!(!s.state.settings.keys(&CommandId::Redo.shortcut_id()).contains(&chord("volumedown", false, false, false)), "imported keys take their chords");
    assert_eq!(s.state.settings.gesture_binding("pen.button.primary"), "hold.eyedropper");
    for (text, error) in [
        ("{", "This isn't a CapyCanvas keymap"),
        (r#"{"format":"other","version":1}"#, "This isn't a CapyCanvas keymap"),
        (r#"{"format":"capycanvas-keymap","version":9}"#, "newer version"),
    ] {
        preference(&mut s, PreferenceAction::ImportKeymap { text: text.into() });
        assert!(s.preferences().unwrap().error.unwrap().contains(error), "{text}");
    }
    preference(&mut s, PreferenceAction::ImportKeymap { text: exported.clone() });
    preference(&mut s, PreferenceAction::CancelKeymapImport);
    assert!(s.preferences().unwrap().keymap.import.is_none());
    let change = s.dispatch(UiAction::Preferences { action: PreferenceAction::ExportKeymap }).unwrap();
    assert!(change.regions & regions::HOST != 0);
    assert!(s.state.requests.iter().any(|r| matches!(&r.kind, HostRequestKind::ExportKeymap { name, text } if name.ends_with(".json") && text.contains("clip-studio"))));
    s.dispatch(UiAction::Preferences { action: PreferenceAction::ChooseKeymapFile }).unwrap();
    assert!(s.state.requests.iter().any(|r| matches!(r.kind, HostRequestKind::ImportKeymap)));
}

#[test]
fn shortcut_editor_explains_scope_source_and_overlaps() {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    s.state.settings.shortcuts.insert(CommandId::Undo.shortcut_id(), vec![chord("[", false, false, false)]);
    s.state.settings.shortcuts.insert("tool_setting.size.decrease".into(), vec![chord("[", false, false, false)]);
    preference(&mut s, PreferenceAction::EditShortcut { id: CommandId::Undo.shortcut_id() });
    let editor = s.preferences().unwrap().shortcut_editor.unwrap();
    assert_eq!(editor.scope, "Everywhere");
    assert_eq!(editor.source, "Custom");
    assert_eq!(editor.overlaps, ["[ does Decrease brush size on the canvas instead"]);
    preference(&mut s, PreferenceAction::EditShortcut { id: "tool_setting.size.decrease".into() });
    let editor = s.preferences().unwrap().shortcut_editor.unwrap();
    assert_eq!(editor.scope, "On the canvas");
    assert_eq!(editor.overlaps, ["[ does Undo elsewhere"]);
    preference(&mut s, PreferenceAction::EditShortcut { id: "hold.eyedropper".into() });
    let editor = s.preferences().unwrap().shortcut_editor.unwrap();
    assert_eq!(editor.scope, "With drawing, blending and fill and gradient tools");
    assert_eq!(editor.source, "CapyCanvas default");
    preference(&mut s, PreferenceAction::SelectKeymap { id: "photoshop".into() });
    preference(&mut s, PreferenceAction::EditShortcut { id: "command.Move".into() });
    assert_eq!(s.preferences().unwrap().shortcut_editor.unwrap().source, "Photoshop-inspired");
    assert_eq!(s.preferences().unwrap().shortcut_editor.unwrap().defaults, ["V"]);
}
