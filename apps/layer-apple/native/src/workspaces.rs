//! Runs the shared workspace controller on the drawing owner. SQLite stays on
//! the Rust storage worker; this owner only polls its replies.
use super::*;
use layer_workspace::{StoreWorker, WorkspaceController, WorkspaceInput};
use serde_json::{Value, json};

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
impl CapyApple {
    pub(crate) fn workspace(&mut self, request: Value) -> Result<Value, String> {
        let time = now();
        if request["type"] == "start" && self.workspaces.is_none() {
            let directory = request["directory"]
                .as_str()
                .ok_or("Workspace storage directory is missing")?;
            let scene = request["scene"].as_str().unwrap_or("default");
            let store =
                StoreWorker::shared(std::path::Path::new(directory)).map_err(|e| e.to_string())?;
            self.host.session.set_workspace_read_only(true);
            let platform = self.host.session.state().platform;
            self.workspaces = Some(
                WorkspaceController::new_localized(store, platform, time, self.host.session.localization().clone())
                    .with_resume_key(format!("apple:scene:{scene}"), time),
            );
        }
        let Some(c) = &mut self.workspaces else {
            return Ok(Value::Null);
        };
        let changes = self.host.take_service_changes();
        c.observe_regions(&mut self.host.session, changes, time);
        if request["type"] == "capture" {
            return serde_json::to_value(self.host.session.capture_workspace()?)
                .map_err(|e| e.to_string());
        }
        if !matches!(request["type"].as_str(), Some("start" | "tick")) {
            let input: WorkspaceInput =
                serde_json::from_value(request).map_err(|e| e.to_string())?;
            let before = self.host.session.state().revision;
            match c.input(&mut self.host.session, input, time) {
                Ok(change) => self.host.apply_change(before, change),
                Err(error) => c.view.error = Some(error.localized_message(self.host.session.localization())),
            }
        }
        let before = self.host.session.state().revision;
        let change = c.tick(&mut self.host.session, time);
        self.host.apply_change(before, change);
        Ok(json!({"view": c.view, "wake": change.canvas_wake, "refresh": change.regions != 0}))
    }
}
