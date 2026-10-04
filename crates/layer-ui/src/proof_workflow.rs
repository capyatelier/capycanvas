//! Print-view policy shared by hosts. Scheduling, cancellation and durable
//! profile writes belong to the host; no viewing state enters document history.
use crate::{ColorFeatureError, HostRequestKind, UiAction, UiChange, UiSession};
use layer_core::color::{ColorProfile, ProofRecipe, RgbSpace};
use layer_render::CanvasRenderer;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Image-analysis identity deliberately excludes camera, proof and rendition
/// settings. Hosts additionally check their GPU generation before publishing.
#[derive(Clone)]
pub struct ToneKey {
    epoch: u64,
    color: layer_core::color::DocumentColor,
    extent: [u32; 2],
    scene: Arc<layer_core::authored::SceneSnapshot>,
}
impl PartialEq for ToneKey {
    fn eq(&self, other: &Self) -> bool {
        self.epoch == other.epoch && self.color == other.color && self.extent == other.extent
            && self.scene.view().same_artwork(other.scene.view())
    }
}
impl ToneKey {
    /// Stale illumination is a useful drawing preview only within the same
    /// document geometry and color interpretation. Layer edits can
    /// retain it; replacing/resizing/converting a document must clear it.
    /// Hosts must additionally match their device/owner generation.
    pub fn can_preview(&self, next: &Self) -> bool {
        self.epoch == next.epoch && self.color == next.color && self.extent == next.extent
    }
    /// Cheap per-frame guard: never display an old document's guide while the
    /// host's debounced content analysis catches up to a replacement or resize.
    pub fn can_preview_current<R: CanvasRenderer>(&self, s: &UiSession<R>) -> bool {
        let d = s.engine().document();
        !s.rendering_suspended() && self.epoch == s.state().document_file.epoch
            && self.color == d.composition().color && self.extent == d.composition().size
    }
    pub fn current<R: CanvasRenderer>(s: &UiSession<R>) -> Option<Self> {
        let d = s.engine().document();
        (d.composition().color.depth.is_float() && !s.rendering_suspended()).then(|| Self {
            epoch: s.state().document_file.epoch, color: d.composition().color,
            extent: d.composition().size,
            scene: d.snapshot(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofKey {
    epoch: u64,
    space: RgbSpace,
    recipe: Option<ProofRecipe>,
}
impl ProofKey {
    fn current<R: CanvasRenderer>(s: &UiSession<R>) -> Self {
        Self {
            epoch: s.state().document_file.epoch,
            space: s.engine().document().composition().color.space,
            recipe: s.engine().document().output().proof.clone(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ProofPreparation {
    original: ProofKey,
    request: Option<u32>,
    #[serde(default)]
    edit: bool,
    pub recipe: ProofRecipe,
}
impl ProofPreparation {
    pub fn begin<R: CanvasRenderer>(
        s: &UiSession<R>,
        request: Option<u32>,
        recipe: Option<ProofRecipe>,
    ) -> Result<Self, ColorFeatureError> {
        let original = ProofKey::current(s);
        let recipe = recipe
            .or_else(|| original.recipe.clone())
            .ok_or(ColorFeatureError::ProofChooseProfile)?;
        recipe.validate()?;
        let job = Self {
            original,
            request,
            edit: request.is_some(),
            recipe,
        };
        job.validate(s)?;
        Ok(job)
    }
    pub fn panel<R: CanvasRenderer>(s: &UiSession<R>, recipe: ProofRecipe) -> Result<Self,ColorFeatureError> {
        let mut job=Self::begin(s,None,Some(recipe))?;
        s.require_proof_idle()?;
        job.edit=true;
        Ok(job)
    }
    pub fn space(&self) -> RgbSpace {
        self.original.space
    }
    pub fn build(
        &self,
        cancelled: impl Fn() -> bool,
    ) -> Result<Arc<layer_color::ProofLut>, ColorFeatureError> {
        self.recipe.validate()?;
        if cancelled() {
            return Err(ColorFeatureError::ProofCancelled);
        }
        layer_color::ProofLut::build(self.space(), &self.recipe, cancelled).map(Arc::new).map_err(Into::into)
    }
    pub fn validate<R: CanvasRenderer>(&self, s: &UiSession<R>) -> Result<(), ColorFeatureError> {
        self.recipe.validate()?;
        if ProofKey::current(s) != self.original || s.rendering_suspended() {
            return Err(ColorFeatureError::ProofDrawingChanged);
        }
        if self.edit {
            if self.request.is_none() && s.proof_panel_mode()!=crate::ProofMode::Print {
                return Err(ColorFeatureError::ProofInactive);
            }
            s.require_proof_idle()?;
            if s.state().document_file.busy { return Err(ColorFeatureError::ProofDocumentBusy); }
        }
        if let Some(id) = self.request
            && (s.state().document_file.busy
                || !s
                    .state()
                    .requests
                    .iter()
                    .any(|r| r.id == id && matches!(r.kind, HostRequestKind::SoftProofSetup)))
            {
                return Err(ColorFeatureError::ProofSetupInactive);
            }
        Ok(())
    }
    /// Only the replaced embedded ICC needs a durable local copy. Builtins are
    /// always selectable. The document still embeds only its active recipe.
    pub fn preservation(&self) -> Option<&[u8]> {
        if !self.edit {
            return None;
        }
        let old = self.original.recipe.as_ref()?;
        if old.profile == self.recipe.profile {
            return None;
        }
        match &old.profile {
            ColorProfile::Icc(bytes) => Some(bytes),
            _ => None,
        }
    }
    pub fn apply<R: CanvasRenderer>(
        &self,
        s: &mut UiSession<R>,
        preserved: bool,
    ) -> Result<UiChange, ColorFeatureError> {
        self.validate(s)?;
        if self.preservation().is_some() && !preserved {
            return Err(ColorFeatureError::ProofPreserveOriginal);
        }
        if self.edit {
            let mut change = s.set_proof_recipe(Some(self.recipe.clone()))?;
            if let Some(id) = self.request {
                let done = s.dispatch(UiAction::CompleteRequest { id, error: None })?;
                change.regions |= done.regions;
                change.revision = done.revision;
            }
            Ok(change)
        } else {
            Ok(UiChange {
                canvas_wake: true,
                ..Default::default()
            })
        }
    }
}

#[derive(Default)]
pub struct ProofView {
    key: Option<ProofKey>,
    generation: u32,
    cache: Option<(RgbSpace, ProofRecipe, Arc<layer_color::ProofLut>)>,
    error: Option<crate::ColorFeatureError>,
    error_text: Option<String>,
    text_key: Option<(u32, bool, bool, u8, crate::UiLanguage)>,
    text: String,
}
#[derive(Serialize)]
pub struct ProofStatus {
    pub generation: u32,
    pub needed: bool,
    pub text: String,
    pub error: Option<String>,
    pub error_reason: Option<crate::ColorFeatureError>,
    pub bytes: usize,
}
impl ProofView {
    pub fn observe<R: CanvasRenderer>(&mut self, s: &UiSession<R>) -> ProofStatus {
        let key = ProofKey::current(s);
        if self.key.as_ref() != Some(&key) {
            self.generation = self.generation.wrapping_add(1);
            self.error = None;
            if self.cache.as_ref().is_some_and(|(space, recipe, _)| {
                *space != key.space || Some(recipe) != key.recipe.as_ref()
            }) {
                self.cache = None;
            }
            self.key = Some(key);
        }
        let visible = (s.state().soft_proof || s.state().gamut_warning)
            && s.engine().document().output().proof.is_some();
        let needed =
            visible && self.cache.is_none() && self.error.is_none() && !s.rendering_suspended();
        let stage = if !visible {0} else if self.error.is_some() {1} else if self.cache.is_none() {2} else {3};
        let text_key = (self.generation, s.state().soft_proof, s.state().gamut_warning, stage, s.localization().language());
        if self.text_key != Some(text_key) {
            let localizer = s.localization();
            self.error_text = self.error.as_ref().map(|reason| reason.proof_message(localizer));
            self.text = match stage {
                0 => String::new(),
                1 => localizer.text(crate::MessageId::COLOR_FEATURES_PROOF_UNAVAILABLE).to_string(),
                2 => localizer.text(crate::MessageId::COLOR_FEATURES_PROOF_PREPARING).to_string(),
                _ => {
                    let name = &s.engine().document().output().proof.as_ref().unwrap().name;
                    let name = crate::profile_library::profile_description_name((!name.is_empty()).then(|| name.clone()), localizer);
                    crate::color_feature_copy::proof_status(localizer, &name, s.state().soft_proof, s.state().gamut_warning)
                },
            };
            self.text_key = Some(text_key);
        }
        ProofStatus {
            generation: self.generation,
            needed,
            text: self.text.clone(),
            error: self.error_text.clone(),
            error_reason: self.error.clone(),
            bytes: self.cache.as_ref().map_or(0, |c| c.2.byte_len()),
        }
    }
    pub fn lut<R: CanvasRenderer>(
        &mut self,
        s: &UiSession<R>,
    ) -> Option<Arc<layer_color::ProofLut>> {
        self.observe(s);
        self.cache.as_ref().map(|c| c.2.clone())
    }
    pub fn retain(
        &mut self,
        job: &ProofPreparation,
        lut: Arc<layer_color::ProofLut>,
    ) -> Result<(), ColorFeatureError> {
        if lut.space() != job.space() {
            return Err(ColorFeatureError::ProofWorkingSpaceChanged);
        }
        self.cache = Some((job.space(), job.recipe.clone(), lut));
        self.error = None;
        Ok(())
    }
    pub fn fail<R: CanvasRenderer>(
        &mut self,
        s: &UiSession<R>,
        job: &ProofPreparation,
        error: String,
    ) {
        self.fail_reason(s, job, crate::ColorFeatureError::Diagnostic(error));
    }

    pub fn fail_reason<R: CanvasRenderer>(
        &mut self,
        s: &UiSession<R>,
        job: &ProofPreparation,
        reason: crate::ColorFeatureError,
    ) {
        self.observe(s);
        if job.request.is_none() && job.validate(s).is_ok() {
            self.cache = None;
            self.error = Some(reason);
            self.text_key = None;
        }
    }
}

#[derive(Serialize)]
pub struct ProofCopyView {
    pub identity: (u64, layer_core::color::DocumentColor),
    pub mode: crate::ProofMode,
    pub hdr: bool,
    pub rendition: layer_core::color::hdr::SdrRendition,
    pub copy: crate::color_feature_copy::ProofCopy,
    pub numbers: serde_json::Value,
    pub pad: serde_json::Value,
    pub pad_values: [f64; 2],
    pub recipe_valid: bool,
    pub document_profile_label: Option<String>,
    pub recipe_profile_label: String,
    pub print_controls: [serde_json::Value; 5],
    pub intents: [crate::proof_panel::LocalizedProofChoice<layer_core::color::RenderingIntent>; 4],
    pub simulations: [crate::proof_panel::LocalizedProofChoice<crate::proof_panel::ProofSimulation>; 3],
}

pub fn proof_copy<R: CanvasRenderer>(s: &UiSession<R>) -> ProofCopyView {
    let document = s.engine().document();
    let localizer = s.localization();
    let name = document.output().proof.as_ref().map(|recipe| &recipe.name);
    let document_profile_label = name.map(|name| crate::profile_library::profile_description_name((!name.is_empty()).then(|| name.clone()), localizer));
    let rendition = s.effective_sdr_rendition();
    ProofCopyView {
        identity: (s.state().document_file.epoch, document.composition().color),
        mode: s.proof_panel_mode(), hdr: document.composition().color.depth.is_float(), rendition,
        copy: crate::color_feature_copy::ProofCopy::new(localizer),
        numbers: crate::proof_panel::localized_numbers(localizer),
        pad: crate::proof_panel::localized_pad(localizer),
        pad_values: crate::proof_panel::sdr_pad_values(rendition),
        recipe_valid: document.output().proof.as_ref().is_some_and(|recipe| recipe.validate().is_ok()
            && layer_color::profile_declared_channels(&recipe.profile).is_ok()),
        recipe_profile_label: document_profile_label.clone().unwrap_or_else(|| document.composition().color.space.name().into()),
        document_profile_label,
        print_controls: crate::proof_panel::PrintProofControl::ALL.map(|control| serde_json::json!({"id":control,"label":control.localized_label(localizer)})),
        intents: crate::proof_panel::proof_intents(localizer),
        simulations: crate::proof_panel::proof_simulations(localizer),
    }
}

pub fn proof_form<R: CanvasRenderer>(s: &UiSession<R>) -> serde_json::Value {
    let document = s.engine().document();
    let print=document.output().proof.as_ref().and_then(|p|crate::proof_panel::PrintProofSettings::from_recipe(p).ok()).unwrap_or_default();
    let mut form = serde_json::json!(proof_copy(s));
    let fields = form.as_object_mut().unwrap();
    fields.insert("recipe".into(), serde_json::json!(document.output().proof.clone().unwrap_or_else(|| ProofRecipe::new(document.composition().color.space.name().into(), ColorProfile::Builtin(document.composition().color.space)))));
    fields.insert("document_profile".into(), serde_json::json!(document.output().proof));
    fields.insert("print_settings".into(), serde_json::json!(print));
    fields.insert("profiles".into(), serde_json::json!(RgbSpace::ALL.map(crate::ExportProfile::builtin)));
    form
}

#[cfg(test)]
mod copy_tests {
    use super::*;
    use crate::{Localizer, MessageId, Platform, UiLanguage, session::test_support::Recorder};

    #[test]
    fn proof_captions_reproject_without_serializing_or_decoding_profiles() {
        let mut bytes = vec![0_u8; 1_048_576];
        bytes[..4].copy_from_slice(&(132_u32).to_be_bytes());
        bytes[12..16].copy_from_slice(b"prtr");
        bytes[16..20].copy_from_slice(b"CMYK");
        bytes[36..40].copy_from_slice(b"acsp");
        let profile = ColorProfile::Icc(bytes.into());
        assert!(layer_color::profile_channels(&profile).is_err());
        let mut session = UiSession::blank(Recorder::default(), [64,64], Platform::Gtk).unwrap();
        let literal = "Embedded ICC profile İı ไทย { $name } 🎨";
        session.set_proof_recipe(Some(ProofRecipe::new(literal.into(), profile.clone()))).unwrap();
        let checkpoint = session.engine().checkpoint();
        for language in UiLanguage::ALL {
            session.set_localization(Localizer::shared(language));
            let copy = serde_json::to_value(proof_copy(&session)).unwrap();
            assert_eq!(copy["recipe_valid"], true);
            assert_eq!(copy["document_profile_label"], literal);
            assert_eq!(copy["recipe_profile_label"], literal);
            assert_eq!(copy["copy"]["title"], session.localization().text(MessageId::COLOR_FEATURES_PROOF_TITLE).as_ref());
            assert!(copy.to_string().len() < 32768);
            for field in ["recipe", "document_profile", "print_settings", "profiles"] { assert!(copy.get(field).is_none()); }
            assert_eq!(session.engine().checkpoint(), checkpoint);
            let ColorProfile::Icc(retained) = &session.engine().document().output().proof.as_ref().unwrap().profile else { unreachable!() };
            let ColorProfile::Icc(original) = &profile else { unreachable!() };
            assert!(Arc::ptr_eq(retained.storage(), original.storage()));
        }
    }

    #[test]
    fn proof_failure_captions_reproject_without_restart_and_keep_literal_diagnostics() {
        let mut session = UiSession::blank(Recorder::default(), [64,64], Platform::Gtk).unwrap();
        session.set_proof_recipe(Some(ProofRecipe::new(String::new(), ColorProfile::Builtin(RgbSpace::Srgb)))).unwrap();
        let mut view = ProofView::default();
        let job = ProofPreparation::begin(&session, None, None).unwrap();
        view.fail_reason(&session, &job, crate::ColorFeatureError::ProfileMissing);
        let generation = view.observe(&session).generation;
        let checkpoint = session.engine().checkpoint();
        for language in UiLanguage::ALL {
            session.set_localization(Localizer::shared(language));
            let status = view.observe(&session);
            assert_eq!(status.generation, generation);
            assert!(!status.needed);
            assert_eq!(status.error, Some(crate::ColorFeatureError::ProfileMissing.message(session.localization())));
            assert_eq!(status.text, session.localization().text(MessageId::COLOR_FEATURES_PROOF_UNAVAILABLE).as_ref());
            assert_eq!(session.engine().checkpoint(), checkpoint);
        }
        let literal = "{\"type\":\"numeric_error\"} ไทย { $name } 🎨";
        view.fail(&session, &job, literal.into());
        for language in UiLanguage::ALL {
            session.set_localization(Localizer::shared(language));
            assert_eq!(view.observe(&session).error.as_deref(), Some(literal));
        }
    }
}

#[cfg(test)]
#[path = "proof_refusal_tests.rs"]
mod refusal_tests;
