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
    assert!(f.controller.view.prompt.as_ref().unwrap().message.contains(command.label.as_ref()));
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
