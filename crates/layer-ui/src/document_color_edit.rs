//! Shared preparation/publication policy for host document color workflows.
use super::*;
use layer_core::{ColorTransition, PreparedColorTransition, Project};

impl<R: CanvasRenderer> UiSession<R> {
    pub fn set_sdr_rendition(&mut self, recipe: layer_core::color::hdr::SdrRendition) -> Result<UiChange,String> {
        self.require_document_idle()?;
        if !self.engine.document().color.depth.is_float() { return Err("SDR rendition settings require HDR artwork".into()); }
        recipe.validate().map_err(str::to_string)?;
        if recipe != self.engine.document().sdr_rendition { self.engine.apply_edit(layer_core::Edit::SetSdrRendition(recipe)).map_err(error)?; }
        self.refresh_document(); self.refresh_commands();
        Ok(self.changed(regions::DOCUMENT | regions::COMMANDS, true))
    }

    /// A host validates the actual bidirectional ICC transform before publishing
    /// this saved recipe. Temporary comparison toggles never enter history.
    pub fn set_proof_recipe(&mut self, recipe: Option<layer_core::color::ProofRecipe>) -> Result<UiChange, String> {
        self.require_document_idle()?;
        if !CommandId::SoftProofSetup.available_on(self.state.platform) {
            return Err("Soft proofing is unavailable on this host".into());
        }
        if recipe != self.engine.document().proof {
            self.engine.apply_edit(layer_core::Edit::SetProof(recipe)).map_err(error)?;
        }
        self.state.soft_proof = self.engine.document().proof.is_some();
        if !self.state.soft_proof { self.state.gamut_warning = false; }
        self.refresh_document();
        self.refresh_commands();
        Ok(self.changed(regions::DOCUMENT | regions::COMMANDS, true))
    }

    pub fn prepare_document_color_transition(
        &self,
        transition: ColorTransition,
    ) -> Result<(PreparedColorTransition, Project), String> {
        self.require_document_idle()?;
        if !matches!(
            self.state.platform,
            Platform::Gtk | Platform::Web | Platform::Android | Platform::Mac | Platform::Ios
        ) {
            return Err("Document color changes are unavailable on this host".into());
        }
        let prepared = self
            .engine
            .prepare_color_transition(transition)
            .map_err(error)?;
        if self.state.platform != Platform::Gtk { crate::require_sdr_host(prepared.document(), "this host")?; }
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
            Platform::Gtk | Platform::Web | Platform::Android | Platform::Mac | Platform::Ios
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
