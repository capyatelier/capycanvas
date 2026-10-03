use super::*;
use layer_ui::{Localizer, MessageId, UiLanguage};
use std::cell::{Cell, RefCell};

#[derive(Clone, Default)]
struct Store {
    database: Rc<RefCell<BrowserDatabase>>,
    requests: Rc<Cell<usize>>,
}
impl WorkspaceStore for Store {
    async fn execute(&self, request: StoreRequest) -> Result<StoreResponse> {
        self.requests.set(self.requests.get() + 1);
        self.database.borrow_mut().execute(request, 1000)
    }
}
fn controller() -> WorkspaceController<Store> {
    let mut controller = WorkspaceController::new_localized(
        Store::default(), Platform::Web, 1000, Localizer::shared(UiLanguage::English),
    );
    let result = controller.task.as_mut().unwrap().poll().unwrap().unwrap();
    let Outcome::Adopt(incoming) = result else { panic!() };
    controller.task = None;
    controller.manager.activate(*incoming);
    controller.manager.clock.set(1000);
    controller.view.ready = true;
    controller.present(1000);
    controller
}

#[test]
fn localization_reprojects_workspace_forms_errors_and_details_without_storage() {
    let mut controller = controller();
    let manager = controller.manager.clone();
    let stored = manager.current_record().unwrap();
    let source = stored.entity.clone();
    controller.view.page = Some(ManagerPage::Workspaces);
    controller.view.selected = Some(source.id.clone());
    controller.details_source = Some(stored.clone());
    controller.refresh_details(1000);
    controller.view.prompt_action = Some(ManagerAction::Delete(source.id.clone()));
    controller.prompt_source = Some(source.metadata.clone());
    controller.view.prompt = Some(manager.form_prompt(
        controller.view.prompt_action.as_ref().unwrap(), controller.prompt_source.as_ref(),
    ).unwrap());
    controller.set_error(StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ChooseAToolbar));
    controller.set_switcher_error(Some(StoreError::known(ErrorKind::InvalidData,
        WorkspaceRefusal::OpenAWorkspaceBeforeEditingItsSwitcher)));
    let requests = manager.store.requests.get();
    let name = controller.view.prompt.as_ref().unwrap().name.clone();
    for language in [UiLanguage::Japanese, UiLanguage::SimplifiedChinese, UiLanguage::TraditionalChinese, UiLanguage::Korean, UiLanguage::English] {
        let localization = Localizer::shared(language);
        assert!(controller.set_localization(localization.clone()));
        assert!(Rc::ptr_eq(&manager, &controller.manager));
        assert!(Arc::ptr_eq(&localization, &manager.localization()));
        assert_eq!(controller.view.title, localization.text(MessageId::WORKSPACE_WORKSPACES).as_ref());
        assert_eq!(controller.view.name, manager.display_name(&source.id, &source.metadata));
        assert_eq!(controller.view.details.as_ref().unwrap().title, controller.view.name);
        let prompt = controller.view.prompt.as_ref().unwrap();
        assert_eq!(prompt.title, localization.text(MessageId::WORKSPACE_DELETE).as_ref());
        assert_eq!(prompt.name, name);
        assert_eq!(controller.view.error.as_deref(), Some(WorkspaceRefusal::ChooseAToolbar.message(&localization).as_ref()));
        assert_eq!(controller.view.switcher_error.as_deref(), Some(WorkspaceRefusal::OpenAWorkspaceBeforeEditingItsSwitcher.message(&localization).as_ref()));
        assert_eq!(manager.current_record().unwrap().entity, stored.entity);
        assert_eq!(manager.current_record().unwrap().claim, stored.claim);
        assert_eq!(manager.store.requests.get(), requests);
        let title = controller.view.title.as_ptr();
        assert!(!controller.set_localization(localization));
        assert_eq!(controller.view.title.as_ptr(), title);
        assert_eq!(manager.store.requests.get(), requests);
    }
}

#[test]
fn localization_preserves_queries_literal_names_drafts_and_pending_tasks() {
    let mut controller = controller();
    let manager = controller.manager.clone();
    let source = manager.current().unwrap();
    let incoming = pollster::block_on(manager.save_as_new(source.capture().unwrap(), "Untitled 日本語 {name} 🎨", 1000)).unwrap();
    manager.activate(incoming);
    controller.view.page = Some(ManagerPage::Workspaces);
    controller.view.selected = manager.active_id();
    controller.view.query = "日本語".into();
    controller.view.prompt_action = Some(ManagerAction::NewToolbar(None));
    let mut prompt = manager.new_toolbar_prompt();
    prompt.name = Some("未確定 draft {name} 🎨".into());
    prompt.description = Some("literal description".into());
    prompt.selected = Some(String::new());
    controller.view.prompt = Some(prompt.clone());
    controller.view.busy = true;
    controller.view.dirty = true;
    controller.run(std::future::pending());
    controller.preferences = controller.spawn(std::future::pending());
    controller.preference_edits.push_back(SwitcherEdit::Show { id: source.id, visible: false });
    let task = controller.task.as_ref().unwrap().signal.clone();
    let preferences = controller.preferences.as_ref().unwrap().signal.clone();
    let requests = manager.store.requests.get();
    assert!(controller.set_localization(Localizer::shared(UiLanguage::Japanese)));
    assert_eq!(controller.view.name, "Untitled 日本語 {name} 🎨");
    assert_eq!(controller.view.query, "日本語");
    assert_eq!(controller.view.selected, manager.active_id());
    let retained = controller.view.prompt.as_ref().unwrap();
    assert_eq!(retained.name, prompt.name);
    assert_eq!(retained.description, prompt.description);
    assert_eq!(retained.selected, prompt.selected);
    assert_eq!(retained.choices.iter().map(|choice| &choice.id).collect::<Vec<_>>(),
        prompt.choices.iter().map(|choice| &choice.id).collect::<Vec<_>>());
    assert_eq!(retained.choices[0].label, controller.localization.text(MessageId::WORKSPACE_EMPTY_TOOLBAR).as_ref());
    assert!(Arc::ptr_eq(&task, &controller.task.as_ref().unwrap().signal));
    assert!(Arc::ptr_eq(&preferences, &controller.preferences.as_ref().unwrap().signal));
    assert!(controller.view.busy && controller.view.dirty);
    assert_eq!(controller.preference_edits.len(), 1);
    assert_eq!(manager.store.requests.get(), requests);
}

#[test]
fn a_pending_preview_adopts_current_language_details_without_another_load() {
    let mut controller = controller();
    let manager = controller.manager.clone();
    let id = DEFAULT_WORKSPACES[0].0.to_owned();
    controller.view.page = Some(ManagerPage::Workspaces);
    controller.view.selected = Some(id.clone());
    controller.preview = controller.spawn(async move {
        let stored = manager.load(&id).await?;
        Ok((Some(stored), None))
    }).map(|task| (controller.selection_generation, task));
    let signal = controller.preview.as_ref().unwrap().1.signal.clone();
    let requests = controller.manager.store.requests.get();
    controller.set_localization(Localizer::shared(UiLanguage::Korean));
    assert_eq!(controller.manager.store.requests.get(), requests);
    assert!(Arc::ptr_eq(&signal, &controller.preview.as_ref().unwrap().1.signal));
    let (stored, _) = controller.preview.as_mut().unwrap().1.poll().unwrap().unwrap();
    controller.details_source = stored;
    controller.refresh_details(1000);
    assert_eq!(controller.view.details.as_ref().unwrap().title,
        controller.manager.display_name(&controller.view.selected.clone().unwrap(),
            &controller.details_source.as_ref().unwrap().entity.metadata));
}
