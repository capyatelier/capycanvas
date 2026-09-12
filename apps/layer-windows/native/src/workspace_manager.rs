//! Native dialog state and transport for the approved shared workspace manager.
//! The live UiSession stays on the canvas owner; async reads never borrow it.
use super::*;
use layer_ui::{
    CustomizationAction, DockLayout, HostRequestKind, Panel, UiAction, WorkspaceCommand,
};
use layer_workspace::{ManagerAction, ManagerPage};
use serde::Deserialize;

#[path = "toolbar_library.rs"]
mod toolbar_library;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Page {
    Workspaces,
    History,
    ThisWorkspace,
    ToolbarLibrary,
    Prompt,
}
impl Page {
    fn toolbars(self) -> bool {
        matches!(self, Self::ThisWorkspace | Self::ToolbarLibrary)
    }
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct Row {
    id: String,
    title: String,
    subtitle: String,
    current: bool,
    rename: bool,
    delete: bool,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Choice {
    id: String,
    title: String,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Prompt {
    title: String,
    message: String,
    name: Option<String>,
    choice_label: String,
    choices: Vec<Choice>,
    choice: Option<String>,
    confirm: String,
    destructive: bool,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct ManagerView {
    id: u64,
    page: Page,
    title: String,
    intro: String,
    query: String,
    rows: Vec<Row>,
    selected: Option<String>,
    apply_label: String,
    can_apply: bool,
    loading: bool,
    busy: bool,
    error: Option<String>,
    can_retry: bool,
    focus_owner: Option<String>,
    prompt: Option<Prompt>,
    toolbar_actions: Vec<layer_workspace::ManagerButton>,
}
#[derive(Clone)]
enum Mutation {
    Switch(String),
    Create,
    SaveToolbar(Panel),
    NewToolbar { group: Option<u32> },
    AddToolbar(String),
    ReplaceToolbar(Panel),
    Rename(String),
    Delete(String),
    History(String),
    Reset,
    ResetBrushes,
}
enum ReadReply {
    Refresh,
    Selected(Box<StoredEntity>),
    Prompt(Box<layer_workspace::ManagerPrompt>),
}

pub(super) struct ManagerUi {
    pub view: Option<ManagerView>,
    read: AsyncTask<Result<ReadReply>>,
    epoch: u64,
    preview: bool,
    selected: Option<StoredEntity>,
    layouts: std::collections::BTreeMap<String, DockLayout>,
    prompt: Option<Mutation>,
    pending: Option<(Mutation, String, Option<String>)>,
    submitted: bool,
    toolbar_installed: bool,
    retry: bool,
}
impl ManagerUi {
    pub fn new(wake: impl Fn() + Send + 'static) -> Self {
        Self {
            view: None,
            read: AsyncTask::new(wake),
            epoch: 0,
            preview: false,
            selected: None,
            layouts: Default::default(),
            prompt: None,
            pending: None,
            submitted: false,
            toolbar_installed: false,
            retry: false,
        }
    }
    pub fn has_accepted_write(&self) -> bool {
        self.submitted || (self.pending.is_some() && self.view.as_ref().is_some_and(|v| v.busy))
    }
    pub fn active(&self) -> bool {
        self.view.is_some()
    }
    pub fn stop(&mut self) {
        self.read.close();
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ManagerInput {
    Select {
        id: Option<String>,
    },
    Search {
        query: String,
    },
    Create,
    ToolbarPage {
        page: Page,
    },
    Toolbar {
        action: ManagerAction,
    },
    Rename {
        id: String,
    },
    Delete {
        id: String,
    },
    Submit {
        name: Option<String>,
        choice: Option<String>,
    },
    Apply,
    Back,
    Cancel,
    Retry,
    FocusResult {
        error: Option<String>,
    },
}
impl<S: WorkspaceStore + 'static> WorkspaceService<S> {
    pub(crate) fn manager_view(&self) -> Option<&ManagerView> {
        self.ui.view.as_ref()
    }

    pub(super) fn sync_binding(&mut self, native: &mut NativeHost) -> Result<()> {
        if let Some(binding) = self.manager.binding() {
            let before = native.session.state().revision;
            let change = native
                .session
                .configure_workspace_manager(binding)
                .map_err(StoreError::invalid)?;
            native.apply_change(before, change);
        }
        self.status.name = self.manager.active_name();
        self.status.id = self.manager.active_id();
        let items = self.manager.items();
        self.status.defaults = layer_workspace::DEFAULT_WORKSPACES
            .iter()
            .map(|(id, preset)| WorkspaceShortcut {
                id: (*id).into(),
                key: preset.name().to_lowercase(),
                name: items
                    .iter()
                    .find(|i| i.id == *id)
                    .map(|i| i.metadata.name.clone())
                    .unwrap_or_else(|| preset.name().into()),
            })
            .collect();
        Ok(())
    }
    fn preview_restore(&mut self, native: &mut NativeHost) {
        self.ui.read.cancel_read();
        self.ui.selected = None;
        if self.ui.preview {
            let before = native.session.state().revision;
            let change = native.session.cancel_workspace_layout_preview();
            native.apply_change(before, change);
            self.ui.preview = false;
        }
        if let Some(view) = &mut self.ui.view {
            view.loading = false;
            view.can_apply = false;
            view.toolbar_actions.clear();
        }
    }
    fn preview_begin(&mut self, native: &mut NativeHost) -> Result<()> {
        if self.ui.view.as_ref().is_some_and(|v| v.page.toolbars()) {
            return Ok(());
        }
        if !self.ui.preview {
            native
                .session
                .begin_workspace_layout_preview()
                .map_err(StoreError::invalid)?;
            self.ui.preview = true;
        }
        Ok(())
    }
    pub(super) fn close_manager(&mut self, native: &mut NativeHost) {
        self.preview_restore(native);
        self.ui.view = None;
        self.ui.prompt = None;
        self.ui.pending = None;
        self.ui.toolbar_installed = false;
        self.ui.layouts.clear();
        self.ui.epoch = self.ui.epoch.wrapping_add(1);
        native.session.end_workspace_transition();
        native.invalidate_snapshot();
    }
    pub(super) fn ui_error(&mut self, native: &mut NativeHost, error: StoreError) {
        if let Some(view) = &mut self.ui.view {
            view.error = Some(error.to_string());
            view.loading = false;
            view.busy = false;
            view.can_retry = self.ui.pending.is_some();
        } else {
            self.status.error = Some(error.to_string());
        }
        native.invalidate_snapshot();
    }
    fn open_manager(
        &mut self,
        native: &mut NativeHost,
        page: Page,
        now: Instant,
        wall_ms: u64,
    ) -> Result<()> {
        if self.ui.active() || self.status.close_requested || !self.status.ready {
            return Err(StoreError::invalid(
                "Finish the current workspace operation first.",
            ));
        }
        native
            .session
            .require_workspace_idle()
            .map_err(StoreError::invalid)?;
        self.observe(native, now, wall_ms, true)?;
        native
            .session
            .begin_workspace_transition()
            .map_err(StoreError::invalid)?;
        self.ui.epoch = self.ui.epoch.wrapping_add(1);
        let (title, intro, apply) = match page {
            Page::Workspaces => (
                "Workspaces".into(),
                "Workspaces save your tool settings and layout for different tasks.",
                "Switch to Workspace",
            ),
            Page::History => (
                format!(
                    "Layout History — {}",
                    self.manager.active_name().unwrap_or_default()
                ),
                "",
                "Restore This Version",
            ),
            Page::ThisWorkspace => (
                "Manage Toolbars".into(),
                "Arrange the toolbars in this workspace.",
                "",
            ),
            Page::ToolbarLibrary => (
                "Manage Toolbars".into(),
                "Save toolbars to reuse in any workspace.",
                "Add to Workspace",
            ),
            Page::Prompt => ("Workspaces".into(), "", ""),
        };
        self.ui.view = Some(ManagerView {
            id: self.ui.epoch,
            page,
            title,
            intro: intro.into(),
            query: String::new(),
            rows: Vec::new(),
            selected: None,
            apply_label: apply.into(),
            can_apply: false,
            loading: false,
            busy: false,
            error: None,
            can_retry: false,
            focus_owner: None,
            prompt: None,
            toolbar_actions: Vec::new(),
        });
        if page == Page::History {
            self.history_rows()?;
            self.preview_begin(native)?;
        } else if page != Page::Prompt {
            self.preview_begin(native)?;
            let manager = self.manager.clone();
            self.ui.view.as_mut().unwrap().loading = true;
            let _ = self.ui.read.start(async move {
                manager.refresh().await?;
                Ok(ReadReply::Refresh)
            });
        }
        native.invalidate_snapshot();
        Ok(())
    }
    fn history_rows(&mut self) -> Result<()> {
        let capture = self
            .manager
            .current()
            .ok_or_else(|| StoreError::invalid("Open a workspace first."))?
            .capture()?;
        let mut versions: Vec<_> = capture.history.revisions.values().cloned().collect();
        let chain: Vec<_> = capture
            .history
            .undo
            .iter()
            .chain(std::iter::once(&capture.history.current))
            .chain(capture.history.redo.iter().rev())
            .collect();
        for version in &mut versions {
            if let Some(name) = version
                .description
                .strip_prefix("Applied ")
                .and_then(|n| n.strip_suffix(" Workspace Template"))
            {
                version.description = format!("Loaded “{name}” layout");
            } else if version.description == "Reset to starting layout" {
                version.description = "Restored starting layout".into();
            } else if version.description == "Arrange panels and toolbars" {
                version.description = chain
                    .windows(2)
                    .find(|p| p[1] == &version.id)
                    .map(|p| {
                        layer_ui::layout_change_description(
                            &capture.history.revisions[p[0]].layout,
                            &version.layout,
                        )
                    })
                    .unwrap_or_else(|| "Earlier layout".into());
            }
        }
        versions.sort_by(|a, b| {
            b.timestamp_ms.cmp(&a.timestamp_ms).then_with(|| {
                let ordinal = |id: &str| {
                    id.strip_prefix('r')
                        .and_then(|n| n.parse::<u64>().ok())
                        .unwrap_or(0)
                };
                ordinal(&b.id).cmp(&ordinal(&a.id))
            })
        });
        self.ui.layouts.clear();
        let view = self.ui.view.as_mut().unwrap();
        view.rows = versions
            .into_iter()
            .map(|v| {
                let current = v.id == capture.history.current;
                self.ui.layouts.insert(v.id.clone(), v.layout);
                let title = if v.description == "Starting configuration" {
                    "Starting layout".into()
                } else {
                    v.description
                };
                let date = layer_workspace::date(v.timestamp_ms);
                Row {
                    id: v.id,
                    title,
                    subtitle: if current {
                        format!("Current layout · {date}")
                    } else {
                        date
                    },
                    current,
                    rename: false,
                    delete: false,
                }
            })
            .collect();
        view.selected = Some(capture.history.current);
        Ok(())
    }
    fn rows(&mut self, native: &mut NativeHost, initial: bool, wall_ms: u64) -> Result<()> {
        let view = self.ui.view.as_mut().unwrap();
        if matches!(view.page, Page::History | Page::Prompt) {
            return Ok(());
        }
        let page = match view.page {
            Page::ThisWorkspace => ManagerPage::ThisWorkspace,
            Page::ToolbarLibrary => ManagerPage::ToolbarLibrary,
            _ => ManagerPage::Workspaces,
        };
        let active = self.manager.active_id();
        let items = self.manager.items();
        view.rows = self
            .manager
            .rows(page, &view.query, wall_ms)
            .into_iter()
            .map(|r| {
                let elsewhere = items
                    .iter()
                    .find(|i| i.id == r.id)
                    .and_then(|i| i.claim.as_ref())
                    .is_some_and(|c| c.owner != self.manager.owner && c.expires_at_ms > wall_ms);
                Row {
                    current: active.as_ref() == Some(&r.id),
                    id: r.id,
                    title: r.title,
                    subtitle: r.subtitle,
                    rename: view.page != Page::ThisWorkspace && !r.builtin && !elsewhere
                        || view.page == Page::Workspaces && !elsewhere,
                    delete: view.page != Page::ThisWorkspace && !r.builtin && !elsewhere,
                }
            })
            .collect();
        let selected = if initial && view.page == Page::Workspaces {
            active
        } else {
            view.selected.clone()
        }
        .filter(|id| view.rows.iter().any(|r| &r.id == id));
        self.select(native, selected, wall_ms)
    }
    fn select(&mut self, native: &mut NativeHost, id: Option<String>, wall_ms: u64) -> Result<()> {
        self.preview_restore(native);
        self.preview_begin(native)?;
        let view = self.ui.view.as_mut().unwrap();
        view.selected = id.filter(|id| view.rows.iter().any(|r| &r.id == id));
        view.error = None;
        view.can_retry = false;
        let Some(id) = view.selected.clone() else {
            native.invalidate_snapshot();
            return Ok(());
        };
        if view.page == Page::ThisWorkspace {
            let panel = serde_json::from_str(&id)
                .map_err(|e| StoreError::invalid(format!("Invalid toolbar identity: {e}")))?;
            let details = self.manager.toolbar_details(panel, true)?;
            view.loading = false;
            view.can_apply = details.actions.iter().any(|a| a.primary && a.enabled);
            view.apply_label = details
                .actions
                .iter()
                .find(|a| a.primary)
                .map(|a| a.label.clone())
                .unwrap_or_default();
            view.toolbar_actions = details.actions;
        } else if view.page == Page::History {
            let layout = self
                .ui
                .layouts
                .get(&id)
                .ok_or_else(|| StoreError::invalid("This layout is no longer retained."))?
                .clone();
            let current = view.rows.iter().any(|r| r.id == id && r.current);
            if !current {
                let before = native.session.state().revision;
                let change = native
                    .session
                    .preview_workspace_layout(&layout)
                    .map_err(StoreError::invalid)?;
                native.apply_change(before, change);
            }
            view.can_apply = !current;
        } else if self.manager.active_id().as_deref() == Some(&id) {
            self.selected(native, self.manager.current_record().unwrap(), wall_ms)?;
        } else {
            view.loading = true;
            let manager = self.manager.clone();
            let _ = self.ui.read.start(async move {
                manager
                    .load(&id)
                    .await
                    .map(Box::new)
                    .map(ReadReply::Selected)
            });
        }
        native.invalidate_snapshot();
        Ok(())
    }
    fn selected(
        &mut self,
        native: &mut NativeHost,
        stored: StoredEntity,
        wall_ms: u64,
    ) -> Result<()> {
        let details = self.manager.details(&stored, true, wall_ms);
        let primary = details.actions.iter().find(|a| a.primary);
        let view = self.ui.view.as_mut().unwrap();
        view.loading = false;
        view.can_apply = primary.is_some_and(|a| a.enabled);
        if view.page == Page::ToolbarLibrary {
            view.apply_label = "Add to Workspace".into();
            view.toolbar_actions = details
                .actions
                .into_iter()
                .filter(|a| {
                    matches!(
                        a.action,
                        ManagerAction::AddToolbar(_)
                            | ManagerAction::Rename(_)
                            | ManagerAction::Delete(_)
                    )
                })
                .collect();
            self.ui.selected = Some(stored);
            native.invalidate_snapshot();
            return Ok(());
        }
        view.apply_label =
            if primary.is_some_and(|a| matches!(a.action, ManagerAction::SwitchToWindow(_))) {
                "Switch to Window"
            } else {
                "Switch to Workspace"
            }
            .into();
        if self.manager.active_id().as_deref() != Some(&stored.entity.id)
            && let Some(layout) = details.preview
        {
            let before = native.session.state().revision;
            let change = native
                .session
                .preview_workspace_layout(&layout)
                .map_err(StoreError::invalid)?;
            native.apply_change(before, change);
        }
        self.ui.selected = Some(stored);
        native.invalidate_snapshot();
        Ok(())
    }
    fn prompt(&mut self, native: &mut NativeHost, operation: Mutation) -> Result<()> {
        self.preview_restore(native);
        if matches!(operation, Mutation::NewToolbar { .. }) {
            self.ui.prompt = Some(operation);
            let view = self.ui.view.as_mut().unwrap();
            view.prompt = None;
            view.loading = true;
            view.error = None;
            view.can_retry = false;
            let manager = self.manager.clone();
            let _ = self.ui.read.start(async move {
                manager.refresh().await?;
                Ok(ReadReply::Prompt(Box::new(manager.new_toolbar_prompt())))
            });
            native.invalidate_snapshot();
            return Ok(());
        }
        let action = match &operation {
            Mutation::Create => ManagerAction::New,
            Mutation::SaveToolbar(panel) => ManagerAction::SaveToolbar(*panel),
            Mutation::ReplaceToolbar(panel) => ManagerAction::ReplaceToolbar(*panel),
            Mutation::Reset => ManagerAction::Reset(self.manager.active_id().unwrap()),
            Mutation::ResetBrushes => ManagerAction::ResetBrushes,
            Mutation::Rename(id) | Mutation::Delete(id) => {
                let item = self
                    .manager
                    .items()
                    .into_iter()
                    .find(|i| &i.id == id)
                    .ok_or_else(|| StoreError::invalid("This workspace is no longer available."))?;
                let elsewhere = item
                    .claim
                    .as_ref()
                    .is_some_and(|c| c.owner != self.manager.owner && c.expires_at_ms > now_ms());
                if elsewhere || (item.metadata.builtin && matches!(operation, Mutation::Delete(_)))
                {
                    return Err(StoreError::invalid(
                        "This workspace cannot be changed here.",
                    ));
                }
                if matches!(operation, Mutation::Rename(_)) {
                    ManagerAction::Rename(id.clone())
                } else {
                    ManagerAction::Delete(id.clone())
                }
            }
            _ => return Err(StoreError::invalid("This operation does not use a prompt.")),
        };
        self.ui.prompt = Some(operation);
        let view = self.ui.view.as_mut().unwrap();
        view.prompt = None;
        view.loading = true;
        view.error = None;
        view.can_retry = false;
        let manager = self.manager.clone();
        let _ = self.ui.read.start(async move {
            if matches!(action, ManagerAction::ReplaceToolbar(_)) {
                manager.refresh().await?;
            }
            manager
                .prompt(&action, now_ms())
                .await
                .map(Box::new)
                .map(ReadReply::Prompt)
        });
        native.invalidate_snapshot();
        Ok(())
    }
    fn present_prompt(&mut self, native: &mut NativeHost, prompt: layer_workspace::ManagerPrompt) {
        let view = self.ui.view.as_mut().unwrap();
        view.loading = false;
        view.prompt = Some(Prompt {
            title: prompt.title,
            // The compact approved manager edits the name only.
            message: if matches!(self.ui.prompt, Some(Mutation::Rename(_))) {
                String::new()
            } else {
                prompt.message
            },
            name: prompt.name,
            choice_label: prompt.choice_label.unwrap_or_default(),
            choices: prompt
                .choices
                .into_iter()
                .map(|c| Choice {
                    id: c.id,
                    title: c.label,
                })
                .collect(),
            choice: prompt.selected,
            confirm: prompt.confirm,
            destructive: prompt.destructive,
        });
        native.invalidate_snapshot();
    }
    fn command(
        &mut self,
        native: &mut NativeHost,
        command: WorkspaceCommand,
        now: Instant,
        wall_ms: u64,
    ) -> Result<()> {
        let (page, operation) = match command {
            WorkspaceCommand::Manage => (Page::Workspaces, None),
            WorkspaceCommand::ManageTemplates | WorkspaceCommand::SaveAsTemplate => {
                return Err(StoreError::invalid(
                    "Use workspaces to save and load your setup.",
                ));
            }
            WorkspaceCommand::LayoutHistory => (Page::History, None),
            WorkspaceCommand::New => (Page::Prompt, Some(Mutation::Create)),
            WorkspaceCommand::ResetBrushes => (Page::Prompt, Some(Mutation::ResetBrushes)),
            WorkspaceCommand::ResetLayout => (Page::Prompt, Some(Mutation::Reset)),
            WorkspaceCommand::Switch { id } => {
                if self.manager.active_id().as_deref() == Some(&id) {
                    return Ok(());
                }
                self.open_manager(native, Page::Prompt, now, wall_ms)?;
                self.queue_mutation(native, Mutation::Switch(id), String::new(), None)?;
                return Ok(());
            }
            WorkspaceCommand::ManageToolbars => (Page::ThisWorkspace, None),
            WorkspaceCommand::NewToolbar { group } => {
                (Page::Prompt, Some(Mutation::NewToolbar { group }))
            }
            WorkspaceCommand::SaveToolbar { panel } => {
                (Page::Prompt, Some(Mutation::SaveToolbar(panel)))
            }
        };
        self.open_manager(native, page, now, wall_ms)?;
        if let Some(operation) = operation {
            self.prompt(native, operation)?;
        }
        Ok(())
    }
    pub(crate) fn manager_input(
        &mut self,
        native: &mut NativeHost,
        dialog: u64,
        input: ManagerInput,
    ) -> Result<()> {
        let Some(view) = self.ui.view.as_ref().filter(|v| v.id == dialog) else {
            return Ok(());
        };
        if self.ui.submitted || (view.busy && !matches!(input, ManagerInput::FocusResult { .. })) {
            return Ok(());
        }
        match input {
            ManagerInput::FocusResult { error } => {
                if view.focus_owner.is_none() {
                    return Ok(());
                }
                let view = self.ui.view.as_mut().unwrap();
                view.focus_owner = None;
                view.busy = false;
                if let Some(error) = error {
                    self.ui_error(native, StoreError::new(ErrorKind::OwnedElsewhere, error));
                } else {
                    self.close_manager(native);
                }
            }
            ManagerInput::Cancel => {
                if self.status.close_requested {
                    self.keep_open(native);
                }
                self.close_manager(native);
            }
            ManagerInput::Back => {
                if self.status.close_requested {
                    self.keep_open(native);
                    self.close_manager(native);
                } else if view.page == Page::Prompt {
                    self.close_manager(native);
                } else {
                    self.ui.prompt = None;
                    self.ui.view.as_mut().unwrap().prompt = None;
                    self.ui.pending = None;
                    self.rows(native, false, now_ms())?;
                    self.preview_begin(native)?;
                }
            }
            ManagerInput::Select { id } => {
                if view.prompt.is_none() {
                    self.select(native, id, now_ms())?;
                }
            }
            ManagerInput::Search { query } => {
                if view.prompt.is_none() {
                    self.ui.view.as_mut().unwrap().query = query;
                    self.rows(native, false, now_ms())?;
                }
            }
            ManagerInput::ToolbarPage { page } => self.toolbar_page(native, page)?,
            ManagerInput::Toolbar { action } => self.toolbar_action(native, action)?,
            ManagerInput::Create => {
                if view.prompt.is_none() {
                    if view.page == Page::Workspaces {
                        self.prompt(native, Mutation::Create)?;
                    } else if view.page == Page::ThisWorkspace {
                        self.prompt(native, Mutation::NewToolbar { group: None })?;
                    }
                }
            }
            ManagerInput::Rename { id } => {
                if view.loading || view.prompt.is_some() || !view.rows.iter().any(|r| r.id == id) {
                    return Ok(());
                }
                self.prompt(native, Mutation::Rename(id))?;
            }
            ManagerInput::Delete { id } => {
                if view.loading || view.prompt.is_some() || !view.rows.iter().any(|r| r.id == id) {
                    return Ok(());
                }
                self.prompt(native, Mutation::Delete(id))?;
            }
            ManagerInput::Submit { name, choice } => {
                if view.loading || view.prompt.is_none() {
                    return Ok(());
                }
                if let Some(operation) = self.ui.prompt.clone() {
                    self.queue_mutation(native, operation, name.unwrap_or_default(), choice)?;
                }
            }
            ManagerInput::Apply => {
                if !view.can_apply || view.loading || view.prompt.is_some() {
                    return Ok(());
                }
                let Some(id) = view.selected.clone() else {
                    return Ok(());
                };
                if view.page.toolbars() {
                    if let Some(action) = view
                        .toolbar_actions
                        .iter()
                        .find(|a| a.primary && a.enabled)
                        .map(|a| a.action.clone())
                    {
                        self.toolbar_action(native, action)?;
                    }
                    return Ok(());
                }
                let operation = match view.page {
                    Page::Workspaces => Mutation::Switch(id),
                    Page::History => Mutation::History(id),
                    Page::Prompt | Page::ThisWorkspace | Page::ToolbarLibrary => return Ok(()),
                };
                self.queue_mutation(native, operation, String::new(), None)?;
            }
            ManagerInput::Retry => {
                if self.ui.pending.is_some() {
                    self.ui.retry = true;
                    let view = self.ui.view.as_mut().unwrap();
                    view.busy = true;
                    view.error = None;
                    view.can_retry = false;
                }
            }
        }
        native.invalidate_snapshot();
        Ok(())
    }
    fn queue_mutation(
        &mut self,
        native: &mut NativeHost,
        operation: Mutation,
        name: String,
        choice: Option<String>,
    ) -> Result<()> {
        if let Some(prompt) = self.ui.view.as_mut().and_then(|v| v.prompt.as_mut()) {
            if prompt.name.is_some() {
                prompt.name = Some(name.clone());
                layer_workspace::validate_name(name.trim())?;
            }
            if !prompt.choices.is_empty()
                && !prompt
                    .choices
                    .iter()
                    .any(|c| Some(&c.id) == choice.as_ref())
            {
                return Err(StoreError::invalid("Choose a replacement workspace."));
            }
            prompt.choice = choice.clone();
        }
        let focus = if let Mutation::Switch(id) = &operation {
            let claim = self
                .ui
                .selected
                .as_ref()
                .filter(|s| &s.entity.id == id)
                .and_then(|s| s.claim.clone())
                .or_else(|| {
                    self.manager
                        .items()
                        .into_iter()
                        .find(|i| &i.id == id)
                        .and_then(|i| i.claim)
                });
            claim
                .filter(|c| c.owner != self.manager.owner && c.expires_at_ms > now_ms())
                .map(|c| c.owner.id)
        } else {
            None
        };
        self.preview_restore(native);
        if let Some(owner) = focus {
            self.ui.pending = None;
            let view = self.ui.view.as_mut().unwrap();
            view.focus_owner = Some(owner);
            view.busy = true;
            view.error = None;
            native.invalidate_snapshot();
            return Ok(());
        }
        self.ui.pending = Some((operation, name, choice));
        self.ui.toolbar_installed = false;
        self.ui.retry = false;
        let view = self.ui.view.as_mut().unwrap();
        view.busy = true;
        view.error = None;
        view.can_retry = false;
        native.invalidate_snapshot();
        Ok(())
    }
    pub(super) fn poll_manager(&mut self, native: &mut NativeHost, now: Instant, wall_ms: u64) {
        if let Some(reply) = self.ui.read.poll() {
            let result = match reply {
                Ok(ReadReply::Refresh) => self
                    .sync_binding(native)
                    .and_then(|()| self.rows(native, true, wall_ms)),
                Ok(ReadReply::Selected(stored)) => self.selected(native, *stored, wall_ms),
                Ok(ReadReply::Prompt(prompt)) => {
                    self.present_prompt(native, *prompt);
                    Ok(())
                }
                Err(error) => Err(error),
            };
            if let Err(error) = result {
                self.ui_error(native, error);
            }
        }
        if self.ui.view.as_ref().is_some_and(|v| v.busy)
            && !self.ui.submitted
            && !self.operation.busy()
            && !self.ownership.busy()
            && let Some((operation, name, choice)) = self.ui.pending.clone()
        {
            // Reset on the canvas owner once; storage retries preserve this accepted capture.
            if matches!(operation, Mutation::ResetBrushes) && !self.ui.retry {
                let before = native.session.state().revision;
                let result = native
                    .session
                    .reset_workspace_brushes()
                    .map_err(StoreError::invalid)
                    .and_then(|change| {
                        native.apply_change(before, change);
                        self.observe(native, now, wall_ms, true)
                    });
                if let Err(error) = result {
                    self.ui_error(native, error);
                    return;
                }
            }
            self.ui.submitted = true;
            if matches!(
                operation,
                Mutation::NewToolbar { .. } | Mutation::AddToolbar(_) | Mutation::ReplaceToolbar(_)
            ) {
                self.submit_toolbar(native, operation, name, choice, now, wall_ms);
                return;
            }
            let manager = self.manager.clone();
            let retry = self.ui.retry;
            let _ = self.operation.start(async move {
                let result = async {
                    let incoming = if retry && manager.has_failed_operation() {
                        manager.retry_failed_operation().await?
                    } else {
                        match operation {
                            Mutation::Switch(id) => {
                                Some(manager.prepare_switch(&id, wall_ms).await?)
                            }
                            Mutation::Create => Some(
                                manager
                                    .create_workspace(&name, None, false, wall_ms)
                                    .await?,
                            ),
                            Mutation::History(revision) => Some(
                                manager
                                    .change_layout(
                                        &manager.active_id().unwrap(),
                                        Some(&revision),
                                        wall_ms,
                                    )
                                    .await?,
                            ),
                            Mutation::Reset => Some(
                                manager
                                    .change_layout(&manager.active_id().unwrap(), None, wall_ms)
                                    .await?,
                            ),
                            Mutation::ResetBrushes => {
                                manager.flush().await?;
                                None
                            }
                            Mutation::NewToolbar { .. }
                            | Mutation::AddToolbar(_)
                            | Mutation::ReplaceToolbar(_) => {
                                unreachable!("Toolbar installation has a canvas-owner completion")
                            }
                            Mutation::SaveToolbar(panel) => {
                                manager.save_toolbar(panel, &name, wall_ms).await?;
                                None
                            }
                            Mutation::Rename(id) => {
                                let stored = manager.load(&id).await?;
                                manager
                                    .rename(
                                        &id,
                                        &name,
                                        &stored.entity.metadata.description,
                                        wall_ms,
                                    )
                                    .await?;
                                None
                            }
                            Mutation::Delete(id) => {
                                manager
                                    .delete_item(
                                        &id,
                                        choice.as_deref().filter(|s| !s.is_empty()),
                                        wall_ms,
                                    )
                                    .await?
                            }
                        }
                    };
                    if let Some(incoming) = &incoming {
                        PreparedWorkspace::new(incoming.entity.capture()?)
                            .map_err(StoreError::invalid)?;
                    }
                    manager.refresh().await?;
                    Ok(incoming.map(Box::new))
                }
                .await;
                Completion::Manager(result)
            });
        }
        if self.status.ready && !self.ui.active() && !self.status.close_requested {
            let request = native.session.state().requests.iter().find_map(|r| {
                if let HostRequestKind::Workspace { command } = &r.kind {
                    Some((r.id, command.clone()))
                } else {
                    None
                }
            });
            if let Some((id, command)) = request {
                // Acknowledge delivery before beginning the modal transition.
                let _ = native.dispatch(UiAction::CompleteRequest { id, error: None });
                if let Err(error) = self.command(native, command, now, wall_ms) {
                    self.ui_error(native, error);
                }
            }
        }
    }
    pub(super) fn manager_completed(
        &mut self,
        native: &mut NativeHost,
        result: Result<Option<Box<StoredEntity>>>,
        now: Instant,
    ) {
        self.ui.submitted = false;
        self.ui.retry = false;
        self.status.dirty = self.manager.dirty();
        match result {
            Err(error) => self.ui_error(native, error),
            Ok(incoming) => {
                let switched = incoming.is_some();
                if let Some(incoming) = incoming {
                    let outgoing = self
                        .manager
                        .current_record()
                        .filter(|old| old.entity.id != incoming.entity.id);
                    if let Err(error) = self.adopt(native, *incoming, now) {
                        self.ui_error(native, error);
                        return;
                    }
                    // Retain the outgoing claim until the live session accepts adoption.
                    // The ordered service queue also drains this release before close.
                    if let Some(outgoing) = outgoing {
                        let manager = self.manager.clone();
                        let started = self.operation.start(async move {
                            manager.release(&outgoing).await;
                            Completion::Released
                        });
                        debug_assert!(started.is_ok());
                    }
                }
                if let Err(error) = self.sync_binding(native) {
                    self.ui_error(native, error);
                    return;
                }
                let page = self.ui.view.as_ref().map(|v| v.page);
                if switched
                    || self.ui.toolbar_installed
                    || page == Some(Page::Prompt)
                    || self.status.close_requested
                {
                    self.close_manager(native);
                } else {
                    self.ui.prompt = None;
                    self.ui.pending = None;
                    if let Some(view) = &mut self.ui.view {
                        view.prompt = None;
                        view.busy = false;
                        view.can_retry = false;
                    }
                    if let Err(error) = self
                        .preview_begin(native)
                        .and_then(|()| self.rows(native, false, now_ms()))
                    {
                        self.ui_error(native, error);
                    }
                }
            }
        }
        native.invalidate_snapshot();
    }
}
