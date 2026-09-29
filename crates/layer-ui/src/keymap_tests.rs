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
                let specificity = settings.scope_of(id).specificity();
                for key in keys {
                    key.validate_for(settings.held_shortcut(id, Platform::Gtk)).unwrap();
                    assert!(
                        settings
                            .conflicts(id, key, Platform::Gtk)
                            .iter()
                            .all(|other| other.scope.specificity() != specificity),
                        "{} {id} {key:?}",
                        preset.id
                    );
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
    let trigger = |s: &UiSession<Recorder>| {
        s.preferences().unwrap().shortcut_page.triggers.into_iter().find(|t| t.id == "touch.tap.4").unwrap()
    };
    assert_eq!(trigger(&s).action, CommandId::ZenMode.label());
    assert!(!trigger(&s).modified);
    preference(&mut s, PreferenceAction::OpenActionPicker { trigger: "touch.tap.4".into() });
    preference(&mut s, PreferenceAction::ChooseAction { id: String::new() });
    assert_eq!(s.state.settings.gesture_binding("touch.tap.4"), "");
    assert!(trigger(&s).modified);
    assert_eq!(trigger(&s).action, "Nothing");
    preference(&mut s, PreferenceAction::ResetTrigger { trigger: "touch.tap.4".into() });
    assert_eq!(s.state.settings.gesture_binding("touch.tap.4"), "command.ZenMode");
    assert!(!trigger(&s).modified);
    preference(&mut s, PreferenceAction::OpenActionPicker { trigger: "touch.tap.4".into() });
    preference(&mut s, PreferenceAction::ChooseAction { id: "command.ZenMode".into() });
    assert!(!s.state.settings.gestures.contains_key("touch.tap.4"), "choosing the default stores nothing");
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
    assert_eq!(preview.title, "Clip Studio Paint Style");
    assert_eq!(preview.unavailable, ["command.FutureThing"]);
    assert!(preview.changed.iter().any(|c| c.starts_with("Undo:")), "{preview:?}");
    assert!(preview.changed.iter().any(|c| c.starts_with("Lower side button: Nothing →")), "{preview:?}");
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
    assert!(s.state.requests.iter().any(|r| matches!(&r.kind, HostRequestKind::ExportKeymap { name, text } if name.ends_with(".capykeys") && text.contains("clip-studio"))));
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
    assert_eq!(editor.id, "command.Eyedropper", "a held action opens the action it holds");
    assert_eq!(editor.description, "Pick a color from the canvas");
    assert_eq!(editor.source, "CapyCanvas default");
    preference(&mut s, PreferenceAction::SelectKeymap { id: "photoshop".into() });
    preference(&mut s, PreferenceAction::EditShortcut { id: "command.Move".into() });
    assert_eq!(s.preferences().unwrap().shortcut_editor.unwrap().source, "Photoshop Style");
    assert_eq!(s.preferences().unwrap().shortcut_editor.unwrap().defaults, ["V"]);
}

#[test]
fn shortcut_page_categories_filters_and_key_search() {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    let view = s.preferences().unwrap();
    let page = &view.shortcut_page;
    assert!(!page.filtering && page.category.is_none() && page.empty.is_none());
    let ids: Vec<_> = page.categories.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids[0], "Modifier keys", "modifier keys come first");
    assert_eq!(ids[1..], crate::shortcuts::SHORTCUT_SECTIONS);
    assert_eq!(page.categories.iter().find(|c| c.id == "Brush presets").unwrap().count, tools::brush_catalog().count());
    assert!(view.shortcuts.iter().all(|r| !r.visible), "the category list shows no rows until one is opened");
    let row = |view: &PreferencesView, id: &str| view.shortcuts.iter().find(|r| r.id == id).unwrap().clone();

    preference(&mut s, PreferenceAction::ShortcutCategory { id: Some("Brush presets".into()) });
    let view = s.preferences().unwrap();
    assert_eq!(view.shortcut_page.category.as_deref(), Some("Brush presets"));
    let visible: Vec<_> = view.shortcuts.iter().filter(|r| r.visible).collect();
    assert_eq!(visible.len(), tools::brush_catalog().count());
    assert!(visible.iter().all(|r| r.group == "Brush presets" && !r.subgroup.is_empty() && r.detail.is_empty()));
    assert_eq!(row(&view, "brush.2").subgroup, "Pencil", "presets are grouped under their tool");
    preference(&mut s, PreferenceAction::ShortcutCategory { id: Some("Nope".into()) });
    assert!(s.preferences().unwrap().error.is_some());
    preference(&mut s, PreferenceAction::ShortcutCategory { id: Some("Edit".into()) });
    let view = s.preferences().unwrap();
    let undo = row(&view, "command.Undo");
    assert!(undo.visible);
    assert_eq!(undo.bindings, [vec!["Ctrl".to_string(), "Z".into()]]);
    assert_eq!(undo.gestures, ["Two-finger tap"]);
    assert_eq!(row(&view, "command.Redo").bindings.len(), 2, "every alternative is listed");
    assert_eq!(row(&view, "tools.ink").detail, "Press again to switch between them");

    preference(&mut s, PreferenceAction::SearchShortcuts { query: "pencil".into() });
    let view = s.preferences().unwrap();
    assert!(view.shortcut_page.filtering && view.shortcut_page.category.is_none());
    assert!(row(&view, "command.Pencil").visible && row(&view, "brush.2").visible);
    assert_eq!(row(&view, "brush.2").detail, "Brush for the Pencil tool", "search results explain which tool a preset belongs to");
    assert_eq!(row(&view, "command.Pencil").detail, "Switch to the tool with its current brush", "a tool sharing a brush's name says so");
    assert!(row(&view, "command.Pen").detail.is_empty());
    preference(&mut s, PreferenceAction::SearchShortcuts { query: "z".into() });
    let view = s.preferences().unwrap();
    let visible: Vec<_> = view.shortcuts.iter().filter(|r| r.visible).map(|r| r.id.as_str()).collect();
    assert!(visible.contains(&"command.Undo") && visible.contains(&"command.Redo"));
    assert!(!visible.contains(&"command.ZoomIn") && !visible.contains(&"command.Sculpt"), "one letter finds keys, not names: {visible:?}");
    preference(&mut s, PreferenceAction::SearchShortcuts { query: "nothing like this".into() });
    let empty = s.preferences().unwrap().shortcut_page.empty.unwrap();
    assert_eq!(empty.title, "No Results Found");

    preference(&mut s, PreferenceAction::SearchShortcutKey { chord: chord("z", true, false, false) });
    let view = s.preferences().unwrap();
    assert_eq!(view.shortcut_query, "Ctrl+Z");
    assert_eq!(view.shortcut_page.key.as_deref(), Some("Ctrl+Z"));
    let visible: Vec<_> = view.shortcuts.iter().filter(|r| r.visible).map(|r| r.id.as_str()).collect();
    assert_eq!(visible, ["command.Undo"], "a pressed key finds exactly what it runs");
    preference(&mut s, PreferenceAction::SearchShortcutKey { chord: chord("f7", false, false, false) });
    let empty = s.preferences().unwrap().shortcut_page.empty.unwrap();
    assert_eq!(empty.description, "F7 isn't assigned to anything.");
    preference(&mut s, PreferenceAction::SearchShortcuts { query: String::new() });
    assert!(s.preferences().unwrap().shortcut_page.key.is_none());

    preference(&mut s, PreferenceAction::ShortcutContext { category: Some(ToolCategory::Selection) });
    let view = s.preferences().unwrap();
    assert!(!view.shortcut_page.modifiers.iter().any(|m| m.label == "Alt" && m.visible), "Alt does not sample with selection tools");
    assert!(row(&view, "command.Undo").visible);
    s.state.settings.shortcuts.insert("command.Figure".into(), vec![chord("u", false, false, false)]);
    s.state.settings.shortcuts.insert("tool_setting.size.decrease".into(), vec![chord("u", false, false, false)]);
    preference(&mut s, PreferenceAction::ShortcutContext { category: Some(ToolCategory::Drawing) });
    let view = s.preferences().unwrap();
    assert!(view.shortcut_page.modifiers.iter().any(|m| m.label == "Alt" && m.visible && m.action == "Sample color"));
    assert!(!row(&view, "command.Figure").visible, "U resolves to the canvas step while drawing");
    preference(&mut s, PreferenceAction::ShortcutContext { category: None });

    preference(&mut s, PreferenceAction::ShortcutShow { show: ShortcutShow::Customized });
    let view = s.preferences().unwrap();
    let visible: Vec<_> = view.shortcuts.iter().filter(|r| r.visible).map(|r| r.id.as_str()).collect();
    assert_eq!(visible, ["tool_setting.size.decrease"]);
    preference(&mut s, PreferenceAction::ShortcutShow { show: ShortcutShow::Assigned });
    let view = s.preferences().unwrap();
    assert!(row(&view, "command.Undo").visible && row(&view, "tool_setting.size.decrease").visible);
    assert!(!row(&view, "command.ClearLayer").visible);
    assert_eq!(view.shortcut_page.shows.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["All actions", "With shortcuts", "Customized"]);
    assert_eq!(view.shortcut_page.contexts[1].label, "Drawing tools");
    s.state.settings.shortcuts.clear();
    preference(&mut s, PreferenceAction::ShortcutShow { show: ShortcutShow::Customized });
    assert_eq!(s.preferences().unwrap().shortcut_page.empty.unwrap().title, "No Customized Shortcuts");
    preference(&mut s, PreferenceAction::ShortcutShow { show: ShortcutShow::All });

    let triggers: Vec<_> = view.shortcut_page.triggers.iter().map(|t| (t.label.as_str(), t.action.as_str())).collect();
    assert_eq!(triggers[0], ("Lower side button", "Nothing"), "pen buttons come first, beside the eraser end");
    assert_eq!(triggers[3], ("Two-finger tap", "Undo"));
    preference(&mut s, PreferenceAction::Search { query: "lower side".into() });
    let results = s.preferences().unwrap().search_results;
    assert!(results.iter().any(|r| r.title == "Lower side button" && r.action == PreferenceAction::Page { page: SettingsPage::Input }));
    preference(&mut s, PreferenceAction::Page { page: SettingsPage::Shortcuts });
    assert!(s.preferences().unwrap().shortcut_page.category.is_none());

    preference(&mut s, PreferenceAction::OpenActionPicker { trigger: "touch.tap.2".into() });
    let picker = s.preferences().unwrap().shortcut_page.picker.unwrap();
    assert!(!picker.nothing);
    let pencil = picker.sections.iter().flat_map(|s| &s.actions).find(|a| a.id == "brush.2").unwrap();
    assert_eq!(pencil.detail, "Brush for the Pencil tool");
    let undo = picker.sections.iter().flat_map(|s| &s.actions).find(|a| a.id == "command.Undo").unwrap();
    assert!(undo.selected && undo.detail == "Ctrl+Z");
    preference(&mut s, PreferenceAction::CloseActionPicker);

    preference(&mut s, PreferenceAction::KeymapDetails { open: true });
    assert!(s.preferences().unwrap().keymap.details);
    preference(&mut s, PreferenceAction::KeymapDetails { open: false });
    assert!(!s.preferences().unwrap().keymap.details);
}

#[test]
fn gimp_and_affinity_keymaps_follow_their_apps() {
    let mut gimp = Settings::default();
    crate::keymaps::select(&mut gimp, "gimp").unwrap();
    for (key, command, shift, expected) in [
        ("e", false, true, Some("command.Eraser")),
        ("e", false, false, Some("command.EllipseSelect")),
        ("m", false, false, Some("command.Move")),
        ("x", false, false, Some("color.swap")),
        ("/", false, false, Some("command.SearchCommands")),
        ("y", true, false, Some("command.Redo")),
        ("a", true, true, Some("command.Deselect")),
        ("d", true, false, None),
        ("z", true, true, None),
        ("delete", false, false, Some("command.ClearSelected")),
        ("backspace", false, false, None),
        ("j", true, false, None),
        ("j", true, true, Some("command.FitCanvas")),
    ] {
        assert_eq!(bound(&gimp, &chord(key, command, shift, false)).as_deref(), expected, "GIMP {key}");
    }
    let ruler = |settings: &Settings, key| {
        settings
            .shortcut_matches(&chord(key, false, false, false), Platform::Gtk, Some(ToolCategory::ShapesRulers))
            .into_iter()
            .map(|d| d.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ruler(&gimp, "delete"), ["command.DeleteRuler", "command.ClearSelected"]);
    assert_eq!(ruler(&gimp, "backspace"), ["command.DeleteRuler"]);
    let mut affinity = Settings::default();
    crate::keymaps::select(&mut affinity, "affinity").unwrap();
    for (key, command, shift, alt, expected) in [
        ("x", false, true, false, Some("color.swap")),
        ("x", false, false, false, None),
        ("v", false, false, false, Some("command.Move")),
        ("z", true, true, false, Some("command.Redo")),
        ("y", true, false, false, None),
        ("f", true, true, true, Some("command.SearchCommands")),
        ("j", true, false, false, Some("command.CopySelectionToLayer")),
        ("j", true, true, false, Some("command.CutSelectionToLayer")),
        ("d", true, false, false, Some("command.Deselect")),
        ("delete", false, false, false, Some("command.ClearSelected")),
        ("backspace", false, false, false, Some("command.ClearSelected")),
        ("backspace", false, false, true, Some("command.FillSelection")),
    ] {
        assert_eq!(bound(&affinity, &chord(key, command, shift, alt)).as_deref(), expected, "Affinity {key}");
    }
}

#[test]
fn selection_to_layer_and_clear_keys_follow_each_preset() {
    let preset = |id: &str| {
        let mut settings = Settings::default();
        crate::keymaps::select(&mut settings, id).unwrap();
        settings
    };
    let key = |key: &str, command, shift| chord(key, command, shift, false);
    for id in ["capy", "photoshop", "krita", "affinity"] {
        let settings = preset(id);
        assert_eq!(bound(&settings, &key("j", true, false)).as_deref(), Some("command.CopySelectionToLayer"), "{id}");
        assert_eq!(bound(&settings, &key("j", true, true)).as_deref(), Some("command.CutSelectionToLayer"), "{id}");
        assert!(settings.keys("layer.duplicate").is_empty(), "{id} moves Ctrl+J from Duplicate layer");
        assert_eq!(bound(&settings, &key("delete", false, false)).as_deref(), Some("command.ClearSelected"), "{id}");
        assert_eq!(bound(&settings, &key("backspace", false, false)).as_deref(), Some("command.ClearSelected"), "{id}");
        assert!(settings.keys("command.ClearOutside").is_empty(), "{id}");
    }
    for preset in crate::keymaps::KEYMAP_PRESETS.iter().filter(|p| ["photoshop", "krita", "gimp", "affinity"].contains(&p.id)) {
        assert!(preset.revision >= 2, "{} changed its rows", preset.id);
    }
    assert!(!crate::keymaps::KEYMAP_PRESETS.iter().any(|p| p.differences.iter().any(|(_, note)| note.contains("clears only the selection"))));
    for chord in [key("j", true, false), key("j", true, true), key("delete", false, false), key("backspace", false, false)] {
        assert!(chord.available(Platform::Web), "{chord:?}");
    }
}

#[test]
fn feather_selection_is_bound_only_in_the_photoshop_preset() {
    let feather = chord("f6", false, true, false);
    for preset in crate::keymaps::KEYMAP_PRESETS {
        let mut settings = Settings::default();
        crate::keymaps::select(&mut settings, preset.id).unwrap();
        let expected = (preset.id == "photoshop").then_some("command.FeatherSelection");
        assert_eq!(bound(&settings, &feather).as_deref(), expected, "{}", preset.id);
    }
    let photoshop = crate::keymaps::KEYMAP_PRESETS.iter().find(|p| p.id == "photoshop").unwrap();
    assert!(photoshop.revision >= 5);
    assert!(feather.available(Platform::Web));
    for command in [
        CommandId::GrowSelection,
        CommandId::ShrinkSelection,
        CommandId::BorderSelection,
        CommandId::SmoothSelection,
        CommandId::TransformSelectionOutline,
    ] {
        assert!(Settings::default().keys(&command.shortcut_id()).is_empty(), "{command:?}");
    }
}
