//! Windows transport around the shared print-proof transaction.
use layer_host::{NativeHost, Renderer};
use layer_render_wgpu::snapshot::CaptureControl;
use layer_ui::{
    UiSession,
    proof_panel::PrintProofSettings,
    proof_workflow::{ProofPreparation, ProofView, proof_form},
};
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) struct Task {
    job: ProofPreparation,
    localization: Arc<layer_ui::Localizer>,
    form: Value,
    settings: PrintProofSettings,
    lut: Option<Arc<layer_color::ProofLut>>,
    validated: bool,
    preserved: bool,
    applied: bool,
}
impl Task {
    pub fn capture(session: &UiSession<Renderer>, id: u32) -> Result<Self, layer_ui::ColorFeatureError> {
        let mut form = proof_form(session);
        form["document_profile"] = session
            .engine()
            .document()
            .proof
            .as_ref()
            .map(PrintProofSettings::from_recipe)
            .transpose()?
            .and_then(|s| s.profile)
            .map(|p| serde_json::to_value(p).unwrap())
            .unwrap_or(Value::Null);
        let recipe = serde_json::from_value(form["recipe"].clone()).map_err(|e| e.to_string())?;
        let settings = PrintProofSettings::from_recipe(&recipe)?;
        Ok(Self {
            localization: session.localization().clone(),
            job: ProofPreparation::begin(session, Some(id), Some(recipe))?,
            form,
            settings,
            lut: None,
            validated: false,
            preserved: false,
            applied: false,
        })
    }
    pub(crate) fn relocalize(&mut self, details: &mut Value, localization: Arc<layer_ui::Localizer>) {
        self.localization = localization;
        self.form["copy"] = json!(layer_ui::color_feature_copy::ProofCopy::new(&self.localization));
        self.form["document_profile_label"] = json!(self.form["document_profile"]["name"].as_str().map(|name|
            layer_ui::ExportProfileCaption::for_name(name.to_owned()).message(&self.localization)));
        details["form"]["copy"] = self.form["copy"].clone();
        details["form"]["document_profile_label"] = self.form["document_profile_label"].clone();
        details["settings_profile_label"] = json!(self.settings.profile.as_ref().map(|profile| profile.display_name(&self.localization)));
        details["intents"] = json!(layer_ui::proof_panel::proof_intents(&self.localization).map(|choice| json!({
            "value":choice.value,"label":choice.label,
            "bpc_available": PrintProofSettings { intent:choice.value,..Default::default() }.bpc_available()
        })));
        details["simulations"] = json!(layer_ui::proof_panel::proof_simulations(&self.localization));
    }
    pub fn details(&self, profiles: Value) -> Result<Value, String> {
        Ok(json!({
            "form": self.form, "settings": self.settings,
            "settings_profile_label": self.settings.profile.as_ref().map(|profile| profile.display_name(&self.localization)),
            "profiles": profiles,
            "intents": layer_ui::proof_panel::proof_intents(&self.localization).map(|choice| json!({
                "value": choice.value, "label": choice.label,
                "bpc_available": PrintProofSettings { intent: choice.value, ..Default::default() }.bpc_available()
            })),
            "simulations": layer_ui::proof_panel::proof_simulations(&self.localization),
        }))
    }
    pub fn work(
        &mut self,
        mut settings: PrintProofSettings,
        profile_id: Option<String>,
        control: &CaptureControl,
    ) -> Result<(), layer_ui::ColorFeatureError> {
        self.validated = false;
        self.preserved = false;
        self.lut = None;
        if let Some(id) = profile_id {
            settings.profile = Some(crate::color_storage::export_profile(&id, control.cancellation_flag(), &self.localization)?);
        }
        self.job.recipe = settings.recipe()?;
        self.settings = settings;
        self.lut = Some(self.job.build(|| control.is_cancelled())?);
        Ok(())
    }
    pub fn validate(&mut self, host: &NativeHost, control: &CaptureControl) -> Result<(), layer_ui::ColorFeatureError> {
        if control.is_cancelled() {return Err(layer_ui::ColorFeatureError::ProofCancelled)}
        self.job.validate(&host.session)?;
        self.validated = true;
        Ok(())
    }
    pub fn preserve(&mut self, control: &CaptureControl) -> Result<(), layer_ui::ColorFeatureError> {
        if !self.validated || self.lut.is_none() {
            return Err(layer_ui::ColorFeatureError::ProofValidateFirst);
        }
        if control.is_cancelled() {return Err(layer_ui::ColorFeatureError::ProofCancelled)}
        if let Some(bytes) = self.job.preservation() {
            crate::color_storage::preserve(bytes, control.cancellation_flag(), &self.localization)?;
        }
        self.preserved = true;
        Ok(())
    }
    pub fn adopt(&mut self, host: &mut NativeHost, control: &CaptureControl) -> Result<(), layer_ui::ColorFeatureError> {
        self.validate(host, control)?;
        if self.lut.is_none() || !self.preserved {
            return Err(layer_ui::ColorFeatureError::ProofPrepareBeforeApply);
        }
        let previous = host.session.state().revision;
        let change = self.job.apply(&mut host.session, self.preserved)?;
        host.apply_change(previous, change);
        host.dirty = true;
        self.applied = true;
        Ok(())
    }
    pub fn retain(&self, view: &mut ProofView) -> Result<(), layer_ui::ColorFeatureError> {
        if self.applied {
            view.retain(&self.job, self.lut.clone().ok_or(layer_ui::ColorFeatureError::ProofPreviewNotPrepared)?)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::color::{ColorProfile, ProofRecipe, RgbSpace};
    use layer_ui::{CommandId, Platform, UiAction};
    fn fixture() -> (NativeHost, Task) {
        let mut host = NativeHost::new(Platform::Windows).unwrap();
        host.dispatch(UiAction::Invoke {
            command: CommandId::SoftProofSetup,
        })
        .unwrap();
        let id = host.session.state().requests.last().unwrap().id;
        let task = Task::capture(&host.session, id).unwrap();
        (host, task)
    }
    #[test]
    fn retained_proof_profile_captions_follow_language_without_changing_candidates() {
        let (_host, mut task) = fixture();
        let bytes: Arc<[u8]> = layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap().into();
        let mut names = vec![String::new(), "Literal e\u{301} ไทย { name }".into()];
        names.extend(layer_ui::UiLanguage::ALL.map(|language| layer_ui::Localizer::shared(language)
            .text(layer_ui::MessageId::COLOR_FEATURES_PROFILE_EMBEDDED).to_string()));
        for name in names {
            let profile = layer_ui::ExportProfile { name:name.clone(), profile:ColorProfile::Icc(bytes.clone().into()), channels:layer_color::ProfileChannels::Rgb };
            task.form["document_profile"] = json!(profile);
            task.settings.profile = Some(profile);
            let recipe = task.settings.recipe().unwrap();
            let prepared = task.job.recipe.clone();
            let raw_document_profile = task.form["document_profile"].clone();
            let mut details = task.details(Value::Null).unwrap();
            for language in layer_ui::UiLanguage::ALL {
                let localization = layer_ui::Localizer::shared(language);
                task.relocalize(&mut details, localization.clone());
                let expected = if name.is_empty() { localization.text(layer_ui::MessageId::COLOR_FEATURES_PROFILE_EMBEDDED).to_string() } else { name.clone() };
                assert_eq!(details["form"]["document_profile_label"], expected);
                assert_eq!(details["settings_profile_label"], expected);
                assert_eq!(task.form["document_profile"], raw_document_profile);
                assert_eq!(task.settings.recipe().unwrap(), recipe);
                assert_eq!(task.job.recipe, prepared);
                let ColorProfile::Icc(current) = &task.settings.profile.as_ref().unwrap().profile else { panic!("ICC candidate replaced") };
                assert!(Arc::ptr_eq(current.storage(), &bytes));
                assert!(!task.validated && !task.preserved && !task.applied);
            }
        }
        task.form["document_profile"] = Value::Null;
        task.settings.profile = None;
        let mut details = task.details(Value::Null).unwrap();
        for language in layer_ui::UiLanguage::ALL {
            task.relocalize(&mut details, layer_ui::Localizer::shared(language));
            assert!(details["form"]["document_profile_label"].is_null());
            assert!(details["settings_profile_label"].is_null());
        }
    }
    #[test]
    fn cancellation_before_and_after_preparation_preserves_artwork_and_history() {
        for after in [false, true] {
            let (mut host, mut task) = fixture();
            let checkpoint = host.session.engine().checkpoint();
            let document = host.session.engine().document().clone();
            let control = CaptureControl::default();
            if after {
                task.work(task.settings.clone(), None, &control).unwrap();
                task.validate(&host, &control).unwrap();
                task.preserve(&control).unwrap();
            }
            control.cancel();
            assert!(task.work(task.settings.clone(), None, &control).is_err());
            assert!(task.adopt(&mut host, &control).is_err());
            assert_eq!(host.session.engine().checkpoint(), checkpoint);
            assert_eq!(host.session.engine().document(), &document);
            assert!(!host.session.state().soft_proof);
        }
    }
    #[test]
    fn prepared_proof_rejects_cancelled_request_and_renderer_suspension() {
        for suspended in [false, true] {
            let (mut host, mut task) = fixture();
            let control = CaptureControl::default();
            task.work(task.settings.clone(), None, &control).unwrap();
            let checkpoint = host.session.engine().checkpoint();
            if suspended {
                host.suspend_renderer().unwrap();
            } else {
                let id = host.session.state().requests.last().unwrap().id;
                host.dispatch(UiAction::CompleteRequest { id, error: None })
                    .unwrap();
            }
            assert!(task.validate(&host, &control).is_err());
            assert!(task.adopt(&mut host, &control).is_err());
            assert_eq!(host.session.engine().checkpoint(), checkpoint);
        }
    }
    #[test]
    fn proof_settings_round_trip_and_invalid_profile_never_commit() {
        let recipe = ProofRecipe::new(
            "Display P3".into(),
            ColorProfile::Builtin(RgbSpace::DisplayP3),
        );
        let settings = PrintProofSettings::from_recipe(&recipe).unwrap();
        assert_eq!(settings.recipe().unwrap(), recipe);
        let (mut host, mut task) = fixture();
        let control = CaptureControl::default();
        let mut invalid = settings.clone();
        invalid.profile.as_mut().unwrap().profile = ColorProfile::Icc(vec![0; 128].into());
        assert!(task.work(invalid, None, &control).is_err());
        assert!(task.adopt(&mut host, &control).is_err());
        assert!(host.session.engine().document().proof.is_none());
        // A failed preparation leaves the same request available for correction.
        task.work(settings, None, &control).unwrap();
        task.validate(&host, &control).unwrap();
        task.preserve(&control).unwrap();
        task.adopt(&mut host, &control).unwrap();
        assert_eq!(host.session.engine().document().proof, Some(recipe.clone()));
        host.dispatch(UiAction::Invoke {
            command: CommandId::Undo,
        })
        .unwrap();
        assert!(host.session.engine().document().proof.is_none());
        host.dispatch(UiAction::Invoke {
            command: CommandId::Redo,
        })
        .unwrap();
        assert_eq!(host.session.engine().document().proof, Some(recipe));
    }
}
