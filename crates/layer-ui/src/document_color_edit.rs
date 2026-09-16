//! Shared preparation/publication policy for host document color workflows.
use super::*;
use layer_core::{ColorTransition, PreparedColorTransition, Project};

impl<R: CanvasRenderer> UiSession<R> {
    pub fn prepare_document_color_transition(
        &self,
        transition: ColorTransition,
    ) -> Result<(PreparedColorTransition, Project), String> {
        self.require_document_idle()?;
        if !matches!(
            self.state.platform,
            Platform::Gtk | Platform::Web | Platform::Android
        ) {
            return Err("Document color changes are unavailable on this host".into());
        }
        let prepared = self
            .engine
            .prepare_color_transition(transition)
            .map_err(error)?;
        let project = Project::snapshot(prepared.document(), &self.files.assets)?;
        Ok((prepared, project))
    }

    pub fn commit_document_color_transition(
        &mut self,
        prepared: PreparedColorTransition,
    ) -> Result<UiChange, String> {
        self.require_document_idle()?;
        if !matches!(
            self.state.platform,
            Platform::Gtk | Platform::Web | Platform::Android
        ) {
            return Err("Document color changes are unavailable on this host".into());
        }
        let mut colors = self.state.colors.clone();
        colors.set_rgb_space(prepared.document().color.space)?;
        self.engine
            .commit_color_transition(prepared)
            .map_err(error)?;
        self.state.colors = colors;
        self.eyedropper.renderer_replaced();
        self.region_tools.renderer_replaced();
        self.navigator_preview = Default::default();
        self.cursor.hover.reset();
        self.refresh_document();
        self.refresh_commands();
        Ok(self.changed(regions::DOCUMENT | regions::BRUSH | regions::COMMANDS, true))
    }
}
