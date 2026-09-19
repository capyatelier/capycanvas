//! Shared preparation/publication policy for host document color workflows.
use super::*;
use layer_core::{ColorTransition, PreparedColorTransition, Project};

/// Viewing mode is transient. Saved SDR and print recipes are document data,
/// independently of whether either simulation is currently visible.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofMode { #[default] Off, Sdr, Print }

impl<R: CanvasRenderer> UiSession<R> {
    pub fn proof_mode(&self) -> ProofMode {
        if self.state.soft_proof { ProofMode::Print }
        else if self.state.preview_sdr { ProofMode::Sdr }
        else { ProofMode::Off }
    }
    /// The Print page can be selected before a profile is ready. It is not a
    /// rendered proof until preparation succeeds; Off cancels that selection.
    pub fn proof_panel_mode(&self) -> ProofMode {
        if self.proof_setup_pending { ProofMode::Print } else { self.proof_mode() }
    }
    pub fn select_proof_mode(&mut self, mode: ProofMode) -> Result<UiChange, String> {
        if mode == ProofMode::Print && self.engine.document().proof.is_none() {
            let change = self.set_proof_mode(ProofMode::Off)?;
            self.last_proof_mode = Some(ProofMode::Print);
            self.proof_setup_pending = true;
            return Ok(change);
        }
        self.set_proof_mode(mode)
    }
    /// Shared menu/shortcut policy; hosts reveal the panel only when enabling.
    pub fn toggle_proof(&mut self) -> Result<UiChange, String> {
        let mode = if self.proof_panel_mode() != ProofMode::Off {
            ProofMode::Off
        } else {
            match self.last_proof_mode {
                Some(ProofMode::Print) => ProofMode::Print,
                _ if self.engine.document().color.depth.is_float() => ProofMode::Sdr,
                _ => ProofMode::Print,
            }
        };
        self.select_proof_mode(mode)
    }
    pub fn set_proof_mode(&mut self, mode: ProofMode) -> Result<UiChange, String> {
        self.require_document_idle()?;
        if mode == ProofMode::Sdr && !self.engine.document().color.depth.is_float() {
            return Err("This drawing is already SDR".into());
        }
        if mode == ProofMode::Print && self.engine.document().proof.is_none() {
            return Err("Choose a print profile".into());
        }
        self.proof_setup_pending = false;
        if mode != ProofMode::Off { self.last_proof_mode = Some(mode); }
        self.state.preview_sdr = mode == ProofMode::Sdr;
        self.state.soft_proof = mode == ProofMode::Print;
        if mode != ProofMode::Print { self.state.gamut_warning = false; }
        self.state.sdr_appearance_preview = None;
        self.refresh_commands();
        Ok(self.changed(regions::COMMANDS | regions::BRUSH, true))
    }
    pub(super) fn cancel_sdr_gesture(&mut self) -> Result<bool, String> {
        let Some(original) = self.sdr_gesture.take() else { return Ok(false) };
        self.engine.preview_edit(layer_core::Edit::SetSdrRendition(original)).map_err(error)?;
        self.refresh_document();
        self.refresh_commands();
        Ok(true)
    }
    /// Live panel editing follows the same one-contact/one-undo contract as
    /// effect controls. Autosave/export waits until the contact finishes.
    pub fn edit_sdr_rendition(&mut self, phase: ContactPhase, recipe: layer_core::color::hdr::SdrRendition) -> Result<UiChange, String> {
        if phase == ContactPhase::Down {
            self.require_document_idle()?;
            if !self.engine.document().color.depth.is_float() {
                return Err("SDR rendition settings require HDR artwork".into());
            }
            self.sdr_gesture = Some(self.engine.document().sdr_rendition);
        } else if self.sdr_gesture.is_none() {
            return Ok(self.changed(0, false));
        }
        if phase == ContactPhase::Cancel || self.workspace_read_only || self.workspace_transition || self.rendering_suspended {
            self.cancel_sdr_gesture()?;
        } else {
            if let Err(e) = recipe.validate() {
                self.cancel_sdr_gesture()?;
                return Err(e.into());
            }
            if phase == ContactPhase::Up {
                let original = self.sdr_gesture.take().unwrap();
                self.engine.preview_edit(layer_core::Edit::SetSdrRendition(original)).map_err(error)?;
                if recipe != original { self.engine.apply_edit(layer_core::Edit::SetSdrRendition(recipe)).map_err(error)?; }
            } else {
                self.engine.preview_edit(layer_core::Edit::SetSdrRendition(recipe)).map_err(error)?;
            }
        }
        self.refresh_document();
        self.refresh_commands();
        Ok(self.changed(regions::DOCUMENT | regions::COMMANDS | regions::BRUSH, true))
    }
    pub fn effective_sdr_rendition(&self) -> layer_core::color::hdr::SdrRendition {
        self.state.sdr_appearance_preview.unwrap_or(self.engine.document().sdr_rendition)
    }
    pub fn preview_sdr_appearance(&mut self, recipe: Option<layer_core::color::hdr::SdrRendition>) -> Result<UiChange, String> {
        if let Some(recipe) = recipe {
            self.require_document_idle()?;
            if !self.engine.document().color.depth.is_float() { return Err("SDR appearance requires HDR artwork".into()); }
            recipe.validate().map_err(str::to_string)?;
        }
        self.state.sdr_appearance_preview = recipe;
        self.refresh_document();
        self.refresh_commands();
        Ok(self.changed(regions::DOCUMENT | regions::COMMANDS | regions::BRUSH, true))
    }
    pub fn set_hdr_display_available(&mut self, available: bool) -> bool {
        if self.state.hdr_display_available == available { return false; }
        self.state.hdr_display_available = available;
        self.refresh_commands();
        self.changed(regions::COMMANDS, true);
        true
    }

    /// A nonmodal proof panel can compare the master without losing its local
    /// draft. The host owns that draft; only an enabled preview reaches viewing.
    pub fn set_sdr_view(&mut self, recipe: Option<layer_core::color::hdr::SdrRendition>, enabled: bool) -> Result<UiChange, String> {
        self.require_document_idle()?;
        if !self.engine.document().color.depth.is_float() { return Err("SDR appearance requires HDR artwork".into()); }
        if let Some(recipe) = recipe { recipe.validate().map_err(str::to_string)?; }
        self.state.sdr_appearance_preview = if enabled { recipe } else { None };
        self.state.preview_sdr = enabled;
        if enabled {
            self.state.soft_proof = false;
            self.state.gamut_warning = false;
            self.proof_setup_pending = false;
            self.last_proof_mode = Some(ProofMode::Sdr);
        }
        self.refresh_document();
        self.refresh_commands();
        Ok(self.changed(regions::DOCUMENT | regions::COMMANDS | regions::BRUSH, true))
    }

    pub fn set_sdr_rendition(&mut self, recipe: layer_core::color::hdr::SdrRendition) -> Result<UiChange,String> {
        self.require_document_idle()?;
        if !self.engine.document().color.depth.is_float() { return Err("SDR rendition settings require HDR artwork".into()); }
        recipe.validate().map_err(str::to_string)?;
        if recipe != self.engine.document().sdr_rendition { self.engine.apply_edit(layer_core::Edit::SetSdrRendition(recipe)).map_err(error)?; }
        self.refresh_document(); self.refresh_commands();
        Ok(self.changed(regions::DOCUMENT | regions::COMMANDS | regions::BRUSH, true))
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
        self.proof_setup_pending = false;
        if self.state.soft_proof {
            self.last_proof_mode = Some(ProofMode::Print);
            self.state.preview_sdr = false;
            self.state.sdr_appearance_preview = None;
        }
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
            Platform::Gtk | Platform::Web | Platform::Android | Platform::Mac | Platform::Ios | Platform::Windows
        ) {
            return Err("Document color changes are unavailable on this host".into());
        }
        let prepared = self
            .engine
            .prepare_color_transition(transition)
            .map_err(error)?;
        if !matches!(self.state.platform, Platform::Gtk | Platform::Windows) { crate::require_sdr_host(prepared.document(), "this host")?; }
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
            Platform::Gtk | Platform::Web | Platform::Android | Platform::Mac | Platform::Ios | Platform::Windows
        ) {
            return Err("Document color changes are unavailable on this host".into());
        }
        let mut colors = self.state.colors.clone();
        colors.set_rgb_space(prepared.document().color.space)?;
        colors.set_document_depth(prepared.document().color.depth)?;
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
