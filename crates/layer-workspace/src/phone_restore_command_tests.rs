use super::*;
use layer_ui::{CommandId, PhoneAction};

#[test]
fn phone_restore_is_undone_by_the_command_named_in_its_confirmation() {
    let mut f = Fixture::for_platform(Platform::Gtk);
    f.host.session.set_viewport([412., 891.], [412, 891]).unwrap();
    let first = f.host.session.state().workspace.layout.phone.as_ref().unwrap()[0].id;
    f.action(serde_json::to_value(UiAction::Phone { action: PhoneAction::RemoveTool { tile: first } }).unwrap());
    f.save();
    let before = f.host.session.capture_workspace().unwrap();
    let current = f.controller.manager.current().unwrap();
    let starting = current.starting_layout(Platform::Gtk, false).unwrap();
    let restored = before.history.layout().starting_layout_candidate(&starting, true).unwrap();
    f.input(serde_json::json!({ "type": "form", "action": { "type": "reset", "value": current.id } }));
    assert_eq!(&layer_ui::durable_layout(&f.host.session.state().workspace.layout), before.history.layout(), "the phone confirmation does not preview a different layout");
    let command = f.host.session.command(CommandId::UndoWorkspace);
    let prompt = f.controller.view.prompt.as_ref().unwrap();
    assert!(prompt.message.contains(command.label.as_ref()));
    assert_eq!(prompt.dialog_intent(), layer_ui::DialogIntent::alert());
    let wire = serde_json::to_value(&f.controller.view).unwrap()["prompt"].clone();
    assert_eq!(wire["dialog"]["role"], "alert");
    assert_eq!(wire["dialog"]["modal"], true);
    f.input(serde_json::json!({ "type": "submit", "name": "" }));
    assert_ne!(&restored, before.history.layout());
    assert_eq!(layer_ui::durable_layout(&f.host.session.state().workspace.layout), restored);
    let undo = f.host.session.workspace_menu().sections.into_iter().flatten().find(|item|
        matches!(item.action.as_ref(), Some(UiAction::Invoke { command: CommandId::UndoWorkspace }))).unwrap();
    assert!(undo.enabled);
    assert_eq!(undo.label, command.label.as_ref());
    f.action(serde_json::to_value(undo.action.unwrap()).unwrap());
    assert_eq!(&layer_ui::durable_layout(&f.host.session.state().workspace.layout), before.history.layout());
}

#[test]
fn restore_confirmations_name_the_runtime_undo_command_in_every_catalog() {
    let mut f = Fixture::for_platform(Platform::Gtk);
    let sources = f.controller.manager.items().into_iter().filter(|item| item.metadata.kind == ItemKind::Workspace).collect::<Vec<_>>();
    assert!(sources.iter().any(|item| item.metadata.builtin) && sources.iter().any(|item| !item.metadata.builtin));
    for language in UiLanguage::ALL {
        let localization = Localizer::shared(language);
        f.host.set_localization(localization.clone());
        f.controller.set_localization(localization);
        let command = f.host.session.command(CommandId::UndoWorkspace);
        assert!(!command.label.is_empty());
        for phone in [false, true] {
            for source in &sources {
                let prompt = f.controller.manager.form_prompt_for_scope(&ManagerAction::Reset(source.id.clone()), Some(&source.metadata), phone).unwrap();
                assert!(prompt.message.contains(command.label.as_ref()), "{language:?}, phone={phone}, builtin={}: prompt {:?} must name the actual command {:?}",
                    source.metadata.builtin, prompt.message, command.label);
            }
        }
    }
}

#[test]
fn restore_prompt_keeps_its_opening_scope_across_resize_and_language_change() {
    for phone in [false, true] {
        let mut f = Fixture::for_platform(Platform::Gtk);
        f.host.session.set_viewport([1200., 900.], [1200, 900]).unwrap();
        f.action(serde_json::json!({ "type": "move_panel", "panel": "toolbar", "viewport": [1200, 900],
            "target": { "kind": "float", "position": [480, 220] } }));
        f.host.session.set_viewport([412., 891.], [412, 891]).unwrap();
        let first = f.host.session.state().workspace.layout.phone.as_ref().unwrap()[0].id;
        f.action(serde_json::to_value(UiAction::Phone { action: PhoneAction::RemoveTool { tile: first } }).unwrap());
        f.save();
        let opening = if phone { [412, 891] } else { [1200, 900] };
        f.host.session.set_viewport(opening.map(|value| value as f32), opening).unwrap();
        let before = f.host.session.capture_workspace().unwrap();
        let current = f.controller.manager.current().unwrap();
        let starting = current.starting_layout(Platform::Gtk, false).unwrap();
        let expected = before.history.layout().starting_layout_candidate(&starting, phone).unwrap();
        assert_ne!(expected, before.history.layout().starting_layout_candidate(&starting, !phone).unwrap());
        let action = ManagerAction::Reset(current.id);
        f.input(serde_json::json!({ "type": "form", "action": action }));
        assert_eq!(&layer_ui::durable_layout(&f.host.session.state().workspace.layout),
            if phone { before.history.layout() } else { &expected }, "the opening scope determines the preview");
        let resized = if phone { [1200, 900] } else { [412, 891] };
        f.host.session.set_viewport(resized.map(|value| value as f32), resized).unwrap();
        let localization = Localizer::shared(UiLanguage::Japanese);
        f.host.set_localization(localization.clone());
        f.controller.set_localization(localization);
        let prompt = f.controller.view.prompt.as_ref().unwrap();
        let original_scope = f.controller.manager.form_prompt_for_scope(&action, Some(&current.metadata), phone).unwrap();
        let opposite_scope = f.controller.manager.form_prompt_for_scope(&action, Some(&current.metadata), !phone).unwrap();
        assert_eq!(prompt.message, original_scope.message);
        assert_ne!(prompt.message, opposite_scope.message);
        f.input(serde_json::json!({ "type": "submit", "name": "" }));
        assert_eq!(layer_ui::durable_layout(&f.host.session.state().workspace.layout), expected, "resizing does not retarget the confirmed restore");
        f.action(serde_json::to_value(UiAction::Invoke { command: CommandId::UndoWorkspace }).unwrap());
        assert_eq!(&layer_ui::durable_layout(&f.host.session.state().workspace.layout), before.history.layout());
    }
}
