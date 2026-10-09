#[test]
fn every_tool_brush_and_mode_can_be_held_by_a_button_or_modifier_key() {
    let all = crate::shortcuts::definitions(Platform::Gtk);
    for command in CommandId::TOOLS.into_iter().filter(|c| c.available_on(Platform::Gtk)) {
        let target = command.shortcut_id();
        let hold = crate::shortcuts::hold_id(&target).unwrap();
        let definition = &all.iter().find(|(d, _)| d.id == hold).unwrap().0;
        assert!(definition.action.held(), "{hold}");
        assert_eq!(crate::shortcuts::hold_target(&hold), Some(target));
    }
    for id in ["hold.brush.2", "hold.color.transparent", "hold.command.SnapRulers"] {
        assert!(all.iter().any(|(d, _)| d.id == id), "{id}");
    }
    assert!(crate::shortcuts::hold_id("command.Undo").is_none(), "one-shot commands have no held form");
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    let view = s.preferences().unwrap();
    assert!(!view.shortcuts.iter().any(|r| r.id.starts_with("hold.") || r.id == "canvas.pan"));
    assert_eq!(view.shortcuts.iter().find(|r| r.id == "command.Hand").unwrap().shortcut, "H");
    assert_eq!(view.shortcut_page.categories[0].id, "Modifier keys");
    assert_eq!(view.shortcut_page.categories[0].count, 6);
    let modifiers: Vec<_> = view.shortcut_page.modifiers.iter().map(|m| (m.label.as_str(), m.action.as_str(), m.detail.as_str())).collect();
    assert_eq!(modifiers, [
        ("Space", "Pan", ""),
        ("Ctrl+Space", "Zoom", ""),
        ("Shift+Space", "Rotate view", ""),
        ("Alt", "Depends on the tool", "Set source · Sample color"),
        ("Alt+Space", "Zoom out", ""),
        ("Ctrl+Alt+Space", "Zoom out", ""),
    ]);
}

#[test]
fn a_pen_button_holds_any_tool_or_brush_and_returns() {
    let mut s = session(Platform::Gtk);
    let preset = s.state.brush.preset;
    let press = |s: &mut UiSession<Recorder>, button, pressed| s.input(UiInput::PenButton { button, pressed }).unwrap();
    bind(&mut s, "pen.button.tertiary", "command.Pencil");
    assert_eq!(s.state.settings.gesture_binding("pen.button.tertiary"), "hold.command.Pencil");
    assert!(press(&mut s, PenButton::Tertiary, true).handled);
    assert_eq!(Some(s.state.brush.tool), CommandId::Pencil.paint_tool());
    press(&mut s, PenButton::Tertiary, false);
    assert!(painting_with(&s, preset));
    bind(&mut s, "pen.button.primary", "brush.3");
    assert_eq!(s.state.settings.gesture_binding("pen.button.primary"), "hold.brush.3");
    press(&mut s, PenButton::Primary, true);
    assert_eq!(s.state.brush.preset, 3);
    press(&mut s, PenButton::Primary, false);
    assert!(painting_with(&s, preset));
    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    let triggers = s.preferences().unwrap().shortcut_page.triggers;
    let third = triggers.iter().find(|t| t.id == "pen.button.tertiary").unwrap();
    assert_eq!((third.section.as_str(), third.action.as_str()), ("Pen buttons", "Pencil"));
    let pencil = s.preferences().unwrap().shortcuts.into_iter().find(|r| r.id == "command.Pencil").unwrap();
    assert_eq!(pencil.gestures, ["Third side button"]);
}

#[test]
fn mode_keys_toggle_on_tap_and_last_only_while_held_in_use() {
    let mut s = session(Platform::Gtk);
    s.state.settings.shortcuts.insert("color.transparent".into(), vec![KeyChord::new("t", Modifiers::default())]);
    s.state.settings.validate().unwrap();
    held_key(&mut s, "t", true, Modifiers::default());
    held_key(&mut s, "t", false, Modifiers::default());
    assert!(s.state.colors.transparent(), "a tap turns transparency on");
    held_key(&mut s, "t", true, Modifiers::default());
    held_key(&mut s, "t", false, Modifiers::default());
    assert!(!s.state.colors.transparent(), "and another turns it off");
    held_key(&mut s, "t", true, Modifiers::default());
    s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
    s.pen(event(&s, 2, PenPhase::Up, 1.)).unwrap();
    held_key(&mut s, "t", false, Modifiers::default());
    s.frame(3, 3).unwrap();
    assert!(!s.state.colors.transparent(), "held while drawing, it returns on release");
    let snapping = s.command(CommandId::SnapRulers).selected;
    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    preference(&mut s, PreferenceAction::AddModifierKey);
    held_key(&mut s, "Shift_L", true, Modifiers::default());
    preference(&mut s, PreferenceAction::ConfirmShortcut { replace: false });
    held_key(&mut s, "Shift_L", false, Modifiers { shift: true, ..Modifiers::default() });
    let shift = KeyChord::new("shift", Modifiers::default());
    preference(&mut s, PreferenceAction::OpenModifierPicker { key: shift, category: None });
    preference(&mut s, PreferenceAction::ChooseAction { id: "command.SnapRulers".into() });
    s.dispatch(UiAction::CloseSettings).unwrap();
    held_key(&mut s, "Shift_L", true, Modifiers::default());
    assert_ne!(s.command(CommandId::SnapRulers).selected, snapping, "a modifier key can hold a mode");
    held_key(&mut s, "Shift_L", false, Modifiers { shift: true, ..Modifiers::default() });
    assert_eq!(s.command(CommandId::SnapRulers).selected, snapping);
}

#[test]
fn modifier_keys_choose_an_action_per_kind_of_tool() {
    let mut s = session(Platform::Gtk);
    let control = KeyChord::new("control", Modifiers::default());
    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    preference(&mut s, PreferenceAction::AddModifierKey);
    held_key(&mut s, "Control_L", true, Modifiers::default());
    preference(&mut s, PreferenceAction::ConfirmShortcut { replace: false });
    held_key(&mut s, "Control_L", false, Modifiers { command: true, ..Modifiers::default() });
    preference(&mut s, PreferenceAction::ModifierKeyPerTool { key: control.clone(), per_tool: true });
    let editor = s.preferences().unwrap().modifier_editor.unwrap();
    assert_eq!(editor.actions.len(), 11, "one row per kind of tool");
    preference(&mut s, PreferenceAction::OpenModifierPicker { key: control.clone(), category: Some(ToolCategory::Selection) });
    let picker = s.preferences().unwrap().shortcut_page.picker.unwrap();
    assert_eq!(picker.title, "Ctrl · Selection tools");
    let actions: Vec<_> = picker.sections.iter().flat_map(|s| &s.actions).collect();
    assert!(actions.iter().any(|a| a.id == "command.Hand" && a.label == "Pan"));
    assert!(!actions.iter().any(|a| a.id == "command.Undo"), "only holdable actions are offered");
    preference(&mut s, PreferenceAction::ChooseAction { id: "command.Move".into() });
    preference(&mut s, PreferenceAction::OpenModifierPicker { key: control.clone(), category: Some(ToolCategory::Drawing) });
    preference(&mut s, PreferenceAction::ChooseAction { id: "command.Eyedropper".into() });
    let view = s.preferences().unwrap();
    let row = view.shortcut_page.modifiers.iter().find(|m| m.key == control).unwrap();
    assert_eq!((row.action.as_str(), row.detail.as_str()), ("Depends on the tool", "Sample color · Move"));
    let exported = crate::keymaps::export(&s.state.settings);
    assert!(serde_json::from_str::<serde_json::Value>(&exported).unwrap()["modifiers"].is_array());
    s.dispatch(UiAction::CloseSettings).unwrap();

    let preset = s.state.brush.preset;
    held_key(&mut s, "Control_L", true, Modifiers::default());
    assert!(sampling(&s), "Ctrl samples with a brush");
    held_key(&mut s, "Control_L", false, Modifiers { command: true, ..Modifiers::default() });
    assert!(painting_with(&s, preset));
    invoke(&mut s, CommandId::Lasso);
    let lasso = s.layer_interaction.tool;
    held_key(&mut s, "Control_L", true, Modifiers::default());
    assert!(s.layer_interaction.tool != lasso, "and moves with a selection tool");
    held_key(&mut s, "Control_L", false, Modifiers { command: true, ..Modifiers::default() });
    assert_eq!(s.layer_interaction.tool, lasso);

    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    preference(&mut s, PreferenceAction::EditModifierKey { key: control.clone() });
    preference(&mut s, PreferenceAction::ModifierKeyPerTool { key: control.clone(), per_tool: false });
    let editor = s.preferences().unwrap().modifier_editor.unwrap();
    assert_eq!(editor.actions.len(), 1);
    preference(&mut s, PreferenceAction::RemoveModifierKey { key: control.clone() });
    assert!(s.state.settings.hold_keys.is_none(), "removing the added key restores the keymap's table");
    preference(&mut s, PreferenceAction::ResetAllShortcuts);
    assert!(s.state.settings.hold_keys.is_none() && s.state.settings.gestures.is_empty());
}

#[test]
fn combinations_win_over_the_keys_they_contain() {
    let mut s = session(Platform::Gtk);
    let ctrl_space = KeyChord { key: " ".into(), command: true, shift: false, alt: false };
    let mut table = s.state.settings.hold_keys(Platform::Gtk);
    table.retain(|hold| hold.key != ctrl_space);
    table.push(crate::shortcuts::HoldKey {
        key: ctrl_space,
        actions: crate::shortcut_page::CONTEXTS.into_iter().map(|c| (c, "command.Pencil".to_string())).collect(),
    });
    s.state.settings.hold_keys = Some(table);
    s.state.settings.validate().unwrap();
    let preset = s.state.brush.preset;
    let ctrl = Modifiers { command: true, ..Modifiers::default() };
    held_key(&mut s, " ", true, Modifiers::default());
    assert_eq!(s.interaction.navigation.as_ref().map(|(token, _)| token.as_str()), Some(" "));
    held_key(&mut s, "Control_L", true, Modifiers::default());
    assert!(s.interaction.navigation.is_none(), "Ctrl+Space replaces Space");
    assert_eq!(Some(s.state.brush.tool), CommandId::Pencil.paint_tool());
    held_key(&mut s, "Control_L", false, ctrl);
    assert_eq!(s.interaction.navigation.as_ref().map(|(token, _)| token.as_str()), Some(" "), "letting go of Ctrl goes back to panning");
    assert!(painting_with(&s, preset));
    held_key(&mut s, " ", false, Modifiers::default());
    assert!(s.interaction.navigation.is_none());
    held_key(&mut s, "Control_L", true, Modifiers::default());
    held_key(&mut s, "z", true, ctrl);
    held_key(&mut s, "z", false, ctrl);
    held_key(&mut s, "Control_L", false, ctrl);
    assert!(s.interaction.modifier_holds.is_empty());
}

#[test]
fn modifier_keys_and_shortcuts_never_share_a_key() {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    preference(&mut s, PreferenceAction::EditShortcut { id: CommandId::Undo.shortcut_id() });
    preference(&mut s, PreferenceAction::BeginShortcut { id: CommandId::Undo.shortcut_id() });
    held_key(&mut s, " ", true, Modifiers::default());
    assert_eq!(s.preferences().unwrap().capture.unwrap().conflict.as_deref(), Some("the modifier key Space"));
    preference(&mut s, PreferenceAction::ConfirmShortcut { replace: true });
    held_key(&mut s, " ", false, Modifiers::default());
    assert!(!s.state.settings.hold_keys(Platform::Gtk).iter().any(|h| h.key == KeyChord::new(" ", Modifiers::default())), "reassigning takes the key");
    assert!(s.state.settings.keys(&CommandId::Undo.shortcut_id()).contains(&KeyChord::new(" ", Modifiers::default())));
}

#[test]
fn defaults_and_presets_never_share_a_key_between_overlapping_contexts() {
    // A tool-scoped binding may share its key with an application binding;
    // dispatch runs the more specific one while it is enabled.
    let layered = |a: &str, b: &str| {
        [
            ("command.DeleteRuler", "command.ClearSelected"),
            ("command.CropCycleOverlay", "command.Move"),
            ("command.CropCycleOverlay", "command.Eyedropper"),
        ]
        .iter()
        .any(|&(x, y)| (a, b) == (x, y) || (a, b) == (y, x))
    };
    for preset in crate::keymaps::KEYMAP_PRESETS {
        let mut settings = Settings::default();
        crate::keymaps::select(&mut settings, preset.id).unwrap();
        for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Windows, Platform::Mac] {
            for (definition, _) in crate::shortcuts::definitions(platform).into_iter().filter(|(d, _)| d.target.is_none()) {
                for key in settings.keys(&definition.id) {
                    let others: Vec<_> = settings
                        .conflicts(&definition.id, &key, platform)
                        .into_iter()
                        .filter(|other| {
                            other.scope.specificity() == definition.scope.specificity() || !layered(&definition.id, &other.id)
                        })
                        .map(|d| d.id)
                        .collect();
                    assert!(others.is_empty(), "{} on {platform:?}: {} and {others:?} share {key:?}", preset.id, definition.id);
                }
            }
        }
    }
}

#[test]
fn the_eraser_end_uses_its_own_tool_while_near_the_tablet() {
    let mut s = session(Platform::Gtk);
    let preset = s.state.brush.preset;
    let input = |s: &UiSession<Recorder>| {
        s.state.settings.pages(Platform::Gtk).into_iter().find(|p| p.id == SettingsPage::Input).unwrap()
    };
    let row = |s: &UiSession<Recorder>, id| input(s).groups.into_iter().flat_map(|g| g.rows).find(|r| r.id == id).unwrap();
    assert!(row(&s, PreferenceId::EraserTool).visible && row(&s, PreferenceId::EraserErase).visible);
    let airbrush = 1 + crate::shortcuts::ERASER_END_TOOLS.iter().position(|c| *c == CommandId::Airbrush).unwrap() as u32;
    edit_preference(&mut s, PreferenceId::EraserTool, PreferenceValue::Choice(airbrush));
    assert_eq!(s.state.settings.eraser_end.tool, Some(CommandId::Airbrush));
    let eraser = |s: &UiSession<Recorder>, sequence, phase| PenEvent { tool: ToolKind::Eraser, ..event(s, sequence, phase, 1.) };
    s.cursor_input(Some(eraser(&s, 1, PenPhase::Hover)));
    s.frame(1, 1).unwrap();
    assert_eq!(Some(s.state.brush.tool), CommandId::Airbrush.paint_tool(), "the eraser end switches tools on approach");
    s.cursor_input(None);
    s.frame(2, 2).unwrap();
    assert!(painting_with(&s, preset), "and returns when it leaves");
    s.pen(eraser(&s, 3, PenPhase::Down)).unwrap();
    assert_eq!(Some(s.state.brush.tool), CommandId::Airbrush.paint_tool(), "a direct touch-down switches before painting");
    s.pen(eraser(&s, 4, PenPhase::Up)).unwrap();
    s.cursor_input(Some(event(&s, 5, PenPhase::Hover, 1.)));
    s.frame(5, 5).unwrap();
    assert!(painting_with(&s, preset), "the pen tip brings back the previous tool");
    let eraser_tool = 1 + crate::shortcuts::ERASER_END_TOOLS.iter().position(|c| *c == CommandId::Eraser).unwrap() as u32;
    edit_preference(&mut s, PreferenceId::EraserTool, PreferenceValue::Choice(eraser_tool));
    assert!(!row(&s, PreferenceId::EraserErase).visible, "the Eraser always erases");
    s.state.settings.eraser_end = crate::EraserEnd { tool: Some(CommandId::Liquify), erase: true };
    assert!(s.state.settings.validate().is_err());
}

#[test]
fn pen_buttons_choose_hold_or_one_shot_actions_per_kind_of_tool() {
    let mut s = session(Platform::Gtk);
    let preset = s.state.brush.preset;
    let press = |s: &mut UiSession<Recorder>, pressed| s.input(UiInput::PenButton { button: PenButton::Primary, pressed }).unwrap();
    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    preference(&mut s, PreferenceAction::EditPenButton { trigger: "pen.button.primary".into() });
    let editor = s.preferences().unwrap().pen_button_editor.unwrap();
    assert_eq!((editor.label.as_str(), editor.actions.len()), ("Lower side button", 1));
    preference(&mut s, PreferenceAction::PenButtonPerTool { trigger: "pen.button.primary".into(), per_tool: true });
    assert_eq!(s.preferences().unwrap().pen_button_editor.unwrap().actions.len(), 11);
    for (category, action) in [(ToolCategory::Drawing, "command.Eyedropper"), (ToolCategory::Selection, "command.Undo")] {
        preference(&mut s, PreferenceAction::OpenPenButtonPicker { trigger: "pen.button.primary".into(), category: Some(category) });
        let picker = s.preferences().unwrap().shortcut_page.picker.unwrap();
        assert!(picker.title.starts_with("Lower side button · "));
        let actions: Vec<_> = picker.sections.iter().flat_map(|s| &s.actions).collect();
        assert!(actions.iter().any(|a| a.id == "command.Undo") && actions.iter().any(|a| a.id == "command.Hand" && a.label == "Pan"));
        preference(&mut s, PreferenceAction::ChooseAction { id: action.into() });
    }
    let row = s.preferences().unwrap().shortcut_page.triggers.into_iter().find(|t| t.id == "pen.button.primary").unwrap();
    assert_eq!((row.action.as_str(), row.detail.as_str()), ("Depends on the tool", "Sample color · Undo"));
    assert!(serde_json::from_str::<serde_json::Value>(&crate::keymaps::export(&s.state.settings)).unwrap()["pen_buttons"].is_object());
    s.dispatch(UiAction::CloseSettings).unwrap();
    assert!(press(&mut s, true).handled);
    assert!(sampling(&s), "a tool lasts while the button is held");
    press(&mut s, false);
    assert!(painting_with(&s, preset));
    s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
    s.pen(event(&s, 2, PenPhase::Move, 1.)).unwrap();
    s.pen(event(&s, 3, PenPhase::Up, 1.)).unwrap();
    s.frame(4, 4).unwrap();
    let painted = s.engine.document().clone();
    invoke(&mut s, CommandId::Lasso);
    assert!(press(&mut s, true).handled);
    press(&mut s, false);
    assert_ne!(s.engine.document(), &painted, "a one-shot action runs once on press");
    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    preference(&mut s, PreferenceAction::ResetTrigger { trigger: "pen.button.primary".into() });
    assert!(s.state.settings.pen_buttons.is_empty());
}

#[test]
fn set_source_holds_with_retouching_tools_and_is_listed_with_them() {
    let definitions = crate::shortcuts::definitions(Platform::Gtk);
    let hold = crate::shortcuts::hold_id("command.CloneSourceArm").unwrap();
    let (held, _) = definitions.iter().find(|(d, _)| d.id == hold).unwrap();
    assert!(matches!(held.action, crate::shortcuts::ShortcutAction::Momentary { .. }));
    assert_eq!(held.scope, crate::shortcuts::BindingScope::Tools { categories: vec![ToolCategory::Retouching] });
    let mut s = session(Platform::Gtk);
    let alt = s.state.settings.hold_keys(Platform::Gtk).into_iter().find(|h| h.key.key == "alt").unwrap();
    assert_eq!(alt.actions.get(&ToolCategory::Retouching).map(String::as_str), Some("command.CloneSourceArm"));
    assert_eq!(alt.actions.get(&ToolCategory::Drawing).map(String::as_str), Some("command.Eyedropper"));
    crate::shortcut_page::set_pen_button(
        &mut s.state.settings,
        Platform::Gtk,
        "pen.button.primary",
        Some(ToolCategory::Retouching),
        "command.CloneSourceArm",
    )
    .unwrap();
    s.state.settings.validate().unwrap();
    let barrel = s.state.settings.pen_actions("pen.button.primary");
    assert_eq!(barrel.get(&ToolCategory::Retouching).map(String::as_str), Some("command.CloneSourceArm"));
    assert!(!barrel.contains_key(&ToolCategory::Drawing), "other tools keep the button unbound");

    let set_source = s.command(CommandId::CloneSourceArm);
    assert!(!set_source.enabled && set_source.checkable);
    assert_eq!(s.command_disabled_reason(CommandId::CloneSourceArm).as_deref(), Some("Choose a retouching tool first"));
    assert!(s.dispatch(UiAction::Invoke { command: CommandId::CloneSourceArm }).is_err());
    invoke(&mut s, CommandId::Clone);
    assert!(s.command(CommandId::CloneSourceArm).enabled);
    assert!(s.command_catalog().iter().any(|d| d.id == "command.clone_source_arm"), "search lists Set Source");
    assert!(crate::customization::tool_catalog(Platform::Gtk)
        .iter()
        .any(|c| c.control == crate::ToolbarControl::Command { command: CommandId::CloneSourceArm }));
    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    let view = s.preferences().unwrap();
    assert!(view.shortcuts.iter().any(|r| r.id == "command.CloneSourceArm"));
    assert!(view.shortcut_page.contexts.iter().any(|c| c.category == Some(ToolCategory::Retouching)));
    let pen = view.shortcut_page.triggers.iter().find(|t| t.id == "pen.button.primary").unwrap();
    assert!(pen.action.contains("source"), "{}", pen.action);
}
