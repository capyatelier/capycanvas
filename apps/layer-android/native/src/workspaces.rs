use crate::app::App;
use layer_ui::{HostRequestKind, UiAction, WorkspaceCommand};
use layer_workspace::{StoreWorker, WorkspaceController, WorkspaceInput};
use serde_json::{Value, json};

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
impl App {
    pub fn workspace(&mut self, request: Value) -> Result<Value, String> {
        let time = now();
        if request["type"] == "start" {
            if self.workspaces.is_none() {
                let path = request["directory"]
                    .as_str()
                    .ok_or("Workspace storage directory is missing")?;
                let store =
                    StoreWorker::shared(std::path::Path::new(path)).map_err(|e| e.to_string())?;
                let capture = self.host.session.capture_workspace()?;
                self.host.session.set_workspace_read_only(true);
                self.workspaces = Some(WorkspaceController::new(
                    store,
                    layer_ui::Platform::Android,
                    "android:capy-canvas:workspace:v1".into(),
                    request["legacy"]
                        .as_bool()
                        .unwrap_or(false)
                        .then_some(capture),
                    time,
                ));
                if let Some(error) = request["legacy_error"].as_str() {
                    self.workspaces
                        .as_mut()
                        .unwrap()
                        .legacy_error(error.into(), time);
                }
            }
        }
        let Some(c) = &mut self.workspaces else {
            return Ok(Value::Null);
        };
        let changes = self.host.take_service_changes();
        if changes
            & (layer_ui::regions::LAYOUT
                | layer_ui::regions::BRUSH
                | layer_ui::regions::DOCUMENT
                | layer_ui::regions::CUSTOMIZATION)
            != 0
        {
            c.observe(&mut self.host.session, time);
        }
        if !matches!(request["type"].as_str(), Some("start" | "tick" | "capture")) {
            let input: WorkspaceInput =
                serde_json::from_value(request.clone()).map_err(|e| e.to_string())?;
            let before = self.host.session.state().revision;
            match c.input(&mut self.host.session, input, time) {
                Ok(change) => self.host.apply_change(before, change),
                Err(error) => c.view.error = Some(error.to_string()),
            }
        }
        if request["type"] == "capture" {
            return serde_json::to_value(self.host.session.capture_workspace()?)
                .map_err(|e| e.to_string());
        }
        let requests: Vec<_> = self
            .host
            .session
            .state()
            .requests
            .iter()
            .filter_map(|r| match &r.kind {
                HostRequestKind::Workspace { command } => Some((r.id, command.clone())),
                _ => None,
            })
            .collect();
        for (id, command) in requests {
            let input = match command {
                WorkspaceCommand::Manage => Some(WorkspaceInput::Open {
                    page: "workspaces".into(),
                }),
                WorkspaceCommand::LayoutHistory => Some(WorkspaceInput::Open {
                    page: "history".into(),
                }),
                WorkspaceCommand::New => Some(WorkspaceInput::Form {
                    kind: "new".into(),
                    id: None,
                }),
                WorkspaceCommand::ResetBrushes => Some(WorkspaceInput::Form {
                    kind: "reset_brushes".into(),
                    id: None,
                }),
                WorkspaceCommand::ResetLayout => Some(WorkspaceInput::Form {
                    kind: "reset".into(),
                    id: None,
                }),
                WorkspaceCommand::Switch { id } => Some(WorkspaceInput::Switch { id }),
                WorkspaceCommand::ManageToolbars => {
                    self.host.dispatch(UiAction::Customize {
                        action: layer_ui::CustomizationAction::ManageToolbars,
                    })?;
                    None
                }
                WorkspaceCommand::NewToolbar { group } => {
                    self.host.dispatch(UiAction::Customize {
                        action: layer_ui::CustomizationAction::NewToolbar { group },
                    })?;
                    None
                }
                _ => None,
            };
            self.host
                .dispatch(UiAction::CompleteRequest { id, error: None })?;
            if let Some(input) = input {
                let before = self.host.session.state().revision;
                match c.input(&mut self.host.session, input, time) {
                    Ok(change) => self.host.apply_change(before, change),
                    Err(error) => c.view.error = Some(error.to_string()),
                }
            }
        }
        let before = self.host.session.state().revision;
        let change = c.tick(&mut self.host.session, time);
        self.host.apply_change(before, change);
        Ok(json!({"view": c.view, "wake": change.canvas_wake, "refresh": change.regions != 0}))
    }
}
