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
fn held_overrides_compose_and_the_latest_hold_wins() {
    for release_alt_first in [true, false] {
        let mut s = session(Platform::Gtk);
        s.state.settings.shortcuts.insert("hold.eraser".into(), vec![KeyChord::new("e", Modifiers::default())]);
        let preset = s.state.brush.preset;
        held_key(&mut s, "Alt_L", true, Modifiers::default());
        held_key(&mut s, "e", true, alt());
        assert_eq!(s.state.brush.tool, Tool::Eraser);
        assert!(s.eyedropper.picking.previous.is_none());
        if release_alt_first {
            held_key(&mut s, "Alt_L", false, alt());
            assert_eq!(s.state.brush.tool, Tool::Eraser);
            held_key(&mut s, "e", false, Modifiers::default());
        } else {
            held_key(&mut s, "e", false, alt());
            assert!(sampling(&s) && s.eyedropper.picking.previous.is_some());
            held_key(&mut s, "Alt_L", false, alt());
        }
        assert!(painting_with(&s, preset), "{release_alt_first}");
    }
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
fn scoped_bindings_resolve_by_specificity_and_conflict_only_when_overlapping() {
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
    assert!(settings.conflict(&CommandId::Undo.shortcut_id(), &bracket, platform).is_none());
    settings.validate_shortcuts().unwrap();
    let alt = KeyChord::new("alt_l", Modifiers::default());
    assert_eq!(alt.key, "alt");
    assert_eq!(matched(&settings, &alt, Some(ToolCategory::Drawing)).as_deref(), Some("hold.eyedropper"));
    assert_eq!(matched(&settings, &alt, Some(ToolCategory::Selection)), None);
    assert_eq!(matched(&settings, &alt, None), None);
    assert_eq!(settings.conflict("hold.eraser", &alt, platform).map(|d| d.id).as_deref(), Some("hold.eyedropper"));
    assert!(settings.conflict("hold.move", &alt, platform).is_none());
    settings.shortcuts.insert("hold.move".into(), vec![alt.clone()]);
    assert_eq!(matched(&settings, &alt, Some(ToolCategory::Drawing)).as_deref(), Some("hold.move"));
    settings.shortcuts.insert("hold.eyedropper".into(), vec![alt.clone()]);
    settings.validate_shortcuts().unwrap();
    assert_eq!(matched(&settings, &alt, Some(ToolCategory::Drawing)).as_deref(), Some("hold.eyedropper"));
    assert_eq!(matched(&settings, &alt, Some(ToolCategory::Selection)).as_deref(), Some("hold.move"));
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
fn held_rows_record_modifier_only_triggers() {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    preference(&mut s, PreferenceAction::BeginShortcut { id: "hold.eraser".into() });
    held_key(&mut s, "Shift_L", true, Modifiers::default());
    let capture = s.preferences().unwrap().capture.unwrap();
    assert_eq!(capture.chord, Some(KeyChord::new("shift", Modifiers::default())));
    assert_eq!(capture.shortcut, "Shift");
    held_key(&mut s, "Shift_L", false, Modifiers { shift: true, ..Modifiers::default() });
    preference(&mut s, PreferenceAction::ConfirmShortcut { replace: false });
    assert_eq!(s.state.settings.keys("hold.eraser"), vec![KeyChord::new("shift", Modifiers::default())]);
    preference(&mut s, PreferenceAction::BeginShortcut { id: CommandId::Undo.shortcut_id() });
    held_key(&mut s, "Shift_L", true, Modifiers::default());
    assert!(s.preferences().unwrap().capture.unwrap().chord.is_none());
}

#[test]
fn catalog_lists_held_and_step_bindings() {
    let mut s = session(Platform::Gtk);
    let catalog = s.command_catalog();
    let held = catalog.iter().find(|d| d.id == "hold.eyedropper").unwrap();
    assert_eq!(held.kind, CommandKind::Held);
    assert_eq!(held.shortcut, "Alt");
    let step = catalog.iter().find(|d| d.id == "tool_setting.size.increase").unwrap();
    assert_eq!(step.shortcut, "]");
    assert!(step.enabled);
    invoke(&mut s, CommandId::Hand);
    let step = s.command_catalog().into_iter().find(|d| d.id == "tool_setting.size.increase").unwrap();
    assert!(!step.enabled);
    assert_eq!(step.disabled_reason.as_deref(), Some("The selected tool has no size setting"));
}
