use super::*;

impl NativeWorkspaces {
    pub(super) fn recover_close(&self, w: &Rc<Workspace>) {
        if self.close_prompt.replace(true) {
            return;
        }
        glib::spawn_future_local(glib::clone!(
            #[weak]
            w,
            async move {
                let prompt = adw::AlertDialog::builder().heading("Workspace Changes Aren’t Saved")
                    .body("Your latest layout changes couldn’t be saved. Keep this window open and try saving again.").build();
                prompt.set_widget_name("workspace-close-recovery");
                prompt.add_responses(&[
                    ("cancel", "Keep Open"),
                    ("discard", "Discard Unsaved Changes"),
                ]);
                prompt.set_close_response("cancel");
                prompt.set_default_response(Some("cancel"));
                prompt.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
                let response = crate::alert::choose(prompt, &w.window).await;
                w.workspaces.close_prompt.set(false);
                if response == "discard" {
                    w.workspaces.send(&w, WorkspaceInput::DiscardClose);
                } else {
                    w.workspaces.close_requested.set(false);
                    w.workspaces.send(&w, WorkspaceInput::Resume);
                }
            }
        ));
    }
}
