use crate::{ManagerAction, ManagerPage, SwitcherEdit, WorkspacePin, WorkspaceRow, WorkspaceView};
use layer_ui::{ContextMenu, ContextMenuItem, Localizer, MessageId, UiAction, WorkspaceCommand};

fn command(label: String, command: WorkspaceCommand, selected: Option<bool>) -> ContextMenuItem {
    let mut item = ContextMenuItem::command(label, UiAction::WorkspaceManager { command });
    item.selected = selected;
    item
}

impl WorkspaceView {
    pub(crate) fn present_manager_rows(&mut self, localization: &Localizer, pins: &[String]) {
        if self.page != Some(ManagerPage::Workspaces) { return; }
        self.rows.sort_by_key(|row| self.order.iter().position(|id| id == &row.id).unwrap_or(usize::MAX));
        for row in &mut self.rows {
            row.options = row.actions.iter().filter(|button| button.enabled && matches!(button.action, ManagerAction::Rename(_) | ManagerAction::Delete(_))).cloned().collect();
            let selected = pins.contains(&row.id);
            row.pin = Some(WorkspacePin {
                label: localization.text(if selected { MessageId::WORKSPACE_HIDE_FROM_TOP_BAR } else { MessageId::WORKSPACE_SHOW_IN_TOP_BAR }).to_string(),
                icon: if selected { "pin" } else { "pin-outline" }.into(),
                selected,
                enabled: false,
                edit: SwitcherEdit::Toggle { id: row.id.clone() },
            });
        }
        self.update_switcher_menu_availability();
    }

    pub(crate) fn present_switcher_menus(&mut self, localization: &Localizer, rows: Vec<WorkspaceRow>) {
        self.switcher_options_label = localization.text(MessageId::WORKSPACE_SWITCHER_OPTIONS).to_string();
        let visibility_label = localization.text(MessageId::WORKSPACE_SHOW_IN_TOP_BAR).to_string();
        let choices: Vec<_> = rows.into_iter().map(|row| {
            let visible = self.switcher.iter().any(|pinned| pinned.id == row.id);
            command(row.title, WorkspaceCommand::ShowInSwitcher { id: row.id, visible: !visible }, Some(visible))
        }).collect();
        let manage = command(localization.text(MessageId::WORKSPACE_ACTION_MANAGE_WORKSPACES).to_string(),
            WorkspaceCommand::Manage, None);
        self.switcher_options = ContextMenu {
            title: visibility_label.clone(),
            sections: vec![choices.clone(), vec![manage.clone()]],
        };
        let submenu = ContextMenuItem {
            label: visibility_label, icon: None, selected: None, action: None, enabled: true,
            hint: String::new(), bindings: Vec::new(), sections: vec![choices],
        };
        self.switcher_menu = ContextMenu {
            title: localization.text(MessageId::WORKSPACE_WORKSPACES).to_string(),
            sections: vec![self.switcher_display.iter().map(|row| {
                command(row.title.clone(), WorkspaceCommand::Switch { id: row.id.clone() }, Some(row.current))
            }).collect(), vec![submenu], vec![manage]],
        };
        self.update_switcher_menu_availability();
    }

    pub(crate) fn update_switcher_menu_availability(&mut self) {
        fn update(menu: &mut [Vec<ContextMenuItem>], available: bool, editing: bool) {
            for item in menu.iter_mut().flatten() {
                item.enabled = match &item.action {
                    Some(UiAction::WorkspaceManager { command: WorkspaceCommand::ShowInSwitcher { .. } })
                    | None => editing,
                    _ => available,
                };
                update(&mut item.sections, available, editing);
            }
        }
        let available = self.ready && !self.busy && !self.closing && self.page.is_none() && self.prompt.is_none();
        let editing = available && !self.switcher_busy;
        update(&mut self.switcher_options.sections, available, editing);
        update(&mut self.switcher_menu.sections, available, editing);
        let renaming = self.page == Some(ManagerPage::Workspaces)
            && matches!(self.prompt_action, Some(ManagerAction::Rename(_)));
        let manager_editing = self.ready && !self.closing && (renaming || (!self.busy && self.prompt.is_none()));
        for pin in self.rows.iter_mut().filter_map(|row| row.pin.as_mut()) {
            pin.enabled = manager_editing;
        }
    }
}
