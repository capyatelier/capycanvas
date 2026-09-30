fn held_key(s: &mut UiSession<Recorder>, name: &str, pressed: bool, modifiers: Modifiers) -> InputReply {
    s.input(UiInput::Key {
        key: name.into(),
        pressed,
        repeat: false,
        modifiers,
        editing: false,
        divider: None,
    })
    .unwrap()
}

fn alt() -> Modifiers {
    Modifiers { alt: true, ..Modifiers::default() }
}

fn sampling(s: &UiSession<Recorder>) -> bool {
    s.layer_interaction.tool.picks_color()
}

fn painting_with(s: &UiSession<Recorder>, preset: u32) -> bool {
    s.layer_interaction.tool == LayerCanvasTool::Paint && s.state.brush.preset == preset
}

#[test]
fn alt_samples_color_while_held_and_restores_the_brush() {
    let mut s = session(Platform::Gtk);
    let preset = s.state.brush.preset;
    let reply = held_key(&mut s, "Alt_L", true, Modifiers::default());
    assert!(reply.handled && reply.change.regions & regions::BRUSH != 0);
    assert!(sampling(&s));
    assert!(s.eyedropper.picking.previous.is_some());
    let reply = held_key(&mut s, "Alt_L", false, alt());
    assert!(reply.handled);
    assert!(painting_with(&s, preset));
    assert!(s.eyedropper.picking.previous.is_none());
    assert!(s.interaction.holds.is_empty() && s.interaction.hold_base.is_none());
}

#[test]
fn space_and_alt_compose_in_every_press_and_release_order() {
    for press_space_first in [true, false] {
        for release_space_first in [true, false] {
            let mut s = session(Platform::Gtk);
            let preset = s.state.brush.preset;
            if press_space_first {
                held_key(&mut s, " ", true, Modifiers::default());
                held_key(&mut s, "Alt_L", true, Modifiers::default());
            } else {
                held_key(&mut s, "Alt_L", true, Modifiers::default());
                assert!(held_key(&mut s, " ", true, alt()).handled);
            }
            assert_eq!(s.interaction.pan_key.as_deref(), Some(" "));
            assert!(sampling(&s));
            if release_space_first {
                assert!(!held_key(&mut s, " ", false, alt()).pan_cursor);
                assert!(sampling(&s));
                held_key(&mut s, "Alt_L", false, alt());
            } else {
                assert!(held_key(&mut s, "Alt_L", false, alt()).pan_cursor);
                assert!(painting_with(&s, preset));
                held_key(&mut s, " ", false, Modifiers::default());
            }
            assert!(s.interaction.pan_key.is_none());
            assert!(painting_with(&s, preset), "{press_space_first} {release_space_first}");
        }
    }
}

#[test]
fn tool_keys_switch_on_tap_and_return_after_a_held_use() {
    let mut s = session(Platform::Gtk);
    let preset = s.state.brush.preset;
    held_key(&mut s, "e", true, Modifiers::default());
    held_key(&mut s, "e", false, Modifiers::default());
    assert_eq!(s.state.brush.tool, Tool::Eraser, "a tap switches tools");
    invoke(&mut s, CommandId::Brush);
    s.dispatch(UiAction::SelectBrush { id: preset }).unwrap();
    held_key(&mut s, "e", true, Modifiers::default());
    assert_eq!(s.state.brush.tool, Tool::Eraser);
    s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
    s.pen(event(&s, 2, PenPhase::Up, 1.)).unwrap();
    held_key(&mut s, "e", false, Modifiers::default());
    s.frame(3, 3).unwrap();
    assert!(painting_with(&s, preset), "holding the key and drawing returns on release");
    held_key(&mut s, "Alt_L", true, Modifiers::default());
    held_key(&mut s, "e", true, alt());
    assert_eq!(s.state.brush.tool, Tool::Eraser, "a tool key chosen during a hold replaces it");
    held_key(&mut s, "e", false, alt());
    held_key(&mut s, "Alt_L", false, alt());
    assert_eq!(s.state.brush.tool, Tool::Eraser);
}

#[test]
fn held_modifiers_do_not_block_other_chords() {
    let mut s = session(Platform::Gtk);
    let size = s.state.brush.diameter;
    held_key(&mut s, "Alt_L", true, Modifiers::default());
    held_key(&mut s, "Shift_L", true, alt());
    assert!(sampling(&s));
    held_key(&mut s, "Shift_L", false, Modifiers { shift: true, alt: true, ..Modifiers::default() });
    held_key(&mut s, "Alt_L", false, alt());
    held_key(&mut s, "]", true, Modifiers::default());
    assert!(s.state.brush.diameter > size);
    let increased = s.state.brush.diameter;
    held_key(&mut s, "]", false, Modifiers::default());
    held_key(&mut s, "Control_L", true, Modifiers::default());
    held_key(&mut s, "z", true, Modifiers { command: true, ..Modifiers::default() });
    assert_eq!(s.state.brush.diameter, increased, "brush size is not document history");
}

#[test]
fn selection_tools_keep_alt_for_their_own_modes() {
    let mut s = session(Platform::Gtk);
    invoke(&mut s, CommandId::Lasso);
    let tool = s.layer_interaction.tool;
    assert!(tool.selection_tool().is_some());
    let reply = held_key(&mut s, "Alt_L", true, Modifiers::default());
    assert!(!reply.handled);
    assert_eq!(s.layer_interaction.tool, tool);
    assert!(s.interaction.modifiers.alt && s.interaction.holds.is_empty());
    held_key(&mut s, "Alt_L", false, alt());
    assert!(!s.interaction.modifiers.alt);
}

#[test]
fn blur_ends_holds_idempotently() {
    let mut s = session(Platform::Gtk);
    let preset = s.state.brush.preset;
    held_key(&mut s, "Alt_L", true, Modifiers::default());
    assert!(sampling(&s));
    s.input(UiInput::Blur).unwrap();
    assert!(painting_with(&s, preset));
    assert!(s.interaction.holds.is_empty() && s.interaction.held_tool.is_none());
    s.input(UiInput::Blur).unwrap();
    assert!(painting_with(&s, preset));
    assert!(!held_key(&mut s, "Alt_L", false, Modifiers::default()).handled);
    assert!(painting_with(&s, preset));
}

#[test]
fn holds_wait_for_an_active_stroke_to_finish() {
    let mut s = session(Platform::Gtk);
    let preset = s.state.brush.preset;
    s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
    s.pen(event(&s, 2, PenPhase::Move, 1.)).unwrap();
    held_key(&mut s, "Alt_L", true, Modifiers::default());
    assert!(painting_with(&s, preset), "a stroke keeps its tool");
    s.pen(event(&s, 3, PenPhase::Up, 1.)).unwrap();
    s.frame(4, 4).unwrap();
    assert!(sampling(&s), "the pending hold applies once idle");
    s.pen(event(&s, 5, PenPhase::Down, 1.)).unwrap();
    held_key(&mut s, "Alt_L", false, alt());
    s.pen(event(&s, 6, PenPhase::Up, 1.)).unwrap();
    s.frame(7, 7).unwrap();
    assert!(painting_with(&s, preset));
}

#[test]
fn explicit_tool_choice_ends_the_hold() {
    let mut s = session(Platform::Gtk);
    held_key(&mut s, "Alt_L", true, Modifiers::default());
    assert!(sampling(&s));
    invoke(&mut s, CommandId::Move);
    let tool = s.layer_interaction.tool;
    assert!(s.interaction.holds.is_empty() && s.interaction.hold_base.is_none());
    assert!(!held_key(&mut s, "Alt_L", false, alt()).handled);
    assert_eq!(s.layer_interaction.tool, tool);
}

#[test]
fn sampling_rearms_while_the_hold_continues() {
    let mut s = session(Platform::Gtk);
    let preset = s.state.brush.preset;
    held_key(&mut s, "Alt_L", true, Modifiers::default());
    assert!(s.cancel_picker());
    held_key(&mut s, "Shift_L", true, alt());
    assert!(s.eyedropper.picking.previous.is_some());
    held_key(&mut s, "Shift_L", false, alt());
    held_key(&mut s, "Alt_L", false, alt());
    assert!(painting_with(&s, preset));
}

#[test]
fn relative_steps_follow_the_tool_setting_bounds() {
    let mut s = session(Platform::Gtk);
    let size = s.state.brush.diameter;
    let step = |s: &mut UiSession<Recorder>, key: &str, repeat: bool| {
        s.input(UiInput::Key {
            key: key.into(),
            pressed: true,
            repeat,
            modifiers: Modifiers::default(),
            editing: false,
            divider: None,
        })
        .unwrap()
    };
    step(&mut s, "]", false);
    step(&mut s, "]", true);
    assert!(s.state.brush.diameter > size);
    held_key(&mut s, "]", false, Modifiers::default());
    step(&mut s, "[", false);
    step(&mut s, "[", true);
    held_key(&mut s, "[", false, Modifiers::default());
    assert!((s.state.brush.diameter - size).abs() < 1e-3);
    for _ in 0..10_000 {
        step(&mut s, "[", true);
    }
    let setting = s.state.tool_settings.iter().find(|c| c.id == "size").unwrap();
    assert_eq!(s.state.brush.diameter, setting.numeric.min as f32);
    held_key(&mut s, "[", false, Modifiers::default());
    invoke(&mut s, CommandId::Hand);
    let before = s.state.clone();
    assert!(step(&mut s, "[", false).handled);
    assert_eq!(s.state.brush, before.brush);
}

#[test]
fn scoped_bindings_conflict_whenever_their_tools_overlap() {
    let platform = Platform::Gtk;
    let mut settings = Settings::default();
    let bracket = KeyChord::new("[", Modifiers::default());
    let matched = |settings: &Settings, chord: &KeyChord, canvas| {
        settings.shortcut_match(chord, platform, canvas).map(|d| d.id)
    };
    settings.shortcuts.insert(CommandId::Undo.shortcut_id(), vec![bracket.clone()]);
    assert_eq!(
        matched(&settings, &bracket, Some(ToolCategory::Drawing)),
        Some(CommandId::Undo.shortcut_id()),
        "an explicit saved binding keeps a newer default from shadowing it"
    );
    settings.shortcuts.insert("tool_setting.size.decrease".into(), vec![bracket.clone()]);
    assert_eq!(matched(&settings, &bracket, Some(ToolCategory::Drawing)).as_deref(), Some("tool_setting.size.decrease"));
    assert_eq!(matched(&settings, &bracket, None), Some(CommandId::Undo.shortcut_id()));
    assert_eq!(
        settings.conflict(&CommandId::Undo.shortcut_id(), &bracket, platform).map(|d| d.id).as_deref(),
        Some("tool_setting.size.decrease"),
        "a key that does two things with the same tool is a conflict"
    );
    settings.validate_shortcuts().expect("overlaps saved by earlier versions still load");
    let alt = KeyChord::new("alt_l", Modifiers::default());
    assert_eq!(alt.key, "alt");
    assert_eq!(matched(&settings, &alt, Some(ToolCategory::Drawing)), None, "modifier keys aren't press shortcuts");
    let table = settings.hold_keys(platform);
    let alt_key = table.iter().find(|h| h.key == alt).unwrap();
    assert_eq!(alt_key.actions.get(&ToolCategory::Drawing).map(String::as_str), Some("command.Eyedropper"));
    assert!(!alt_key.actions.contains_key(&ToolCategory::Selection));
    assert_eq!(
        settings.conflict(&CommandId::Undo.shortcut_id(), &alt, platform).map(|d| d.id),
        Some("modifier:Alt".to_string()),
        "a modifier key conflicts with shortcuts on the same key"
    );
    settings.shortcuts.insert(CommandId::Undo.shortcut_id(), vec![alt]);
    assert!(settings.validate_shortcuts().is_err(), "instant actions need a non-modifier key");
}

#[test]
fn stored_bindings_without_scopes_keep_their_meaning() {
    let definition: ShortcutDefinition = serde_json::from_value(serde_json::json!({
        "id": "command.Undo",
        "label": "Undo",
        "action": { "kind": "action", "action": { "type": "invoke", "command": "undo" } },
    }))
    .unwrap();
    assert_eq!(definition.scope, BindingScope::Application);
    assert!(serde_json::to_value(&definition).unwrap().get("scope").is_none());
    let settings: Settings = serde_json::from_value(serde_json::json!({})).unwrap();
    assert_eq!(settings.keys("hold.eyedropper"), vec![KeyChord::new("alt", Modifiers::default())]);
    assert_eq!(settings.keys("canvas.pan"), vec![KeyChord::new(" ", Modifiers::default())]);
}

#[test]
fn modifier_keys_record_bare_modifiers_and_combinations() {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    preference(&mut s, PreferenceAction::AddModifierKey);
    held_key(&mut s, "Shift_L", true, Modifiers::default());
    let capture = s.preferences().unwrap().capture.unwrap();
    assert_eq!((capture.chord.clone(), capture.shortcut.as_str()), (Some(KeyChord::new("shift", Modifiers::default())), "Shift"));
    held_key(&mut s, " ", true, Modifiers { shift: true, ..Modifiers::default() });
    let capture = s.preferences().unwrap().capture.unwrap();
    assert_eq!(capture.shortcut, "Shift+Space", "the fullest combination held is kept");
    preference(&mut s, PreferenceAction::ConfirmShortcut { replace: false });
    let editor = s.preferences().unwrap().modifier_editor.unwrap();
    assert_eq!(editor.label, "Shift+Space");
    assert_eq!(editor.actions.len(), 1, "one action for every tool until asked otherwise");
    for (key, allowed) in [("q", true), ("F13", true), ("pad_button_3", true), ("gamepad_a", true), ("XF86Tools", true)] {
        preference(&mut s, PreferenceAction::AddModifierKey);
        held_key(&mut s, key, true, Modifiers::default());
        let capture = s.preferences().unwrap().capture.unwrap();
        assert_eq!(capture.error.is_none(), allowed, "{key}: {capture:?}");
        held_key(&mut s, key, false, Modifiers::default());
        preference(&mut s, PreferenceAction::CancelShortcut);
    }
    preference(&mut s, PreferenceAction::AddModifierKey);
    held_key(&mut s, "Escape", true, Modifiers::default());
    assert!(s.preferences().unwrap().capture.is_none(), "Escape cancels instead");
    held_key(&mut s, "Escape", false, Modifiers::default());
    preference(&mut s, PreferenceAction::AddModifierKey);
    held_key(&mut s, "Alt_L", true, Modifiers::default());
    let capture = s.preferences().unwrap().capture.unwrap();
    assert!(capture.existing && capture.notice == "Already a modifier key" && capture.conflict.is_none());
    held_key(&mut s, "Alt_L", false, alt());
    preference(&mut s, PreferenceAction::ConfirmShortcut { replace: false });
    assert_eq!(s.preferences().unwrap().modifier_editor.unwrap().label, "Alt", "confirming opens the existing key");
    preference(&mut s, PreferenceAction::AddModifierKey);
    held_key(&mut s, "e", true, Modifiers::default());
    assert_eq!(s.preferences().unwrap().capture.unwrap().conflict.as_deref(), Some("Eraser"), "keys used by shortcuts conflict");
    preference(&mut s, PreferenceAction::ConfirmShortcut { replace: true });
    held_key(&mut s, "e", false, Modifiers::default());
    assert!(s.state.settings.keys(&CommandId::Eraser.shortcut_id()).is_empty(), "reassigning takes the key from the shortcut");
    assert_eq!(s.preferences().unwrap().modifier_editor.unwrap().label, "E");
}

#[test]
fn catalog_lists_held_and_step_bindings() {
    let mut s = session(Platform::Gtk);
    let catalog = s.command_catalog();
    let held = catalog.iter().find(|d| d.id == "hold.eyedropper").unwrap();
    assert_eq!(held.shortcut, "Alt");
    let step = catalog.iter().find(|d| d.id == "tool_setting.size.increase").unwrap();
    assert_eq!(step.shortcut, "]");
    assert!(step.enabled);
    invoke(&mut s, CommandId::Hand);
    let step = s.command_catalog().into_iter().find(|d| d.id == "tool_setting.size.increase").unwrap();
    assert!(!step.enabled);
    assert_eq!(step.disabled_reason.as_deref(), Some("The selected tool has no size setting"));
}
