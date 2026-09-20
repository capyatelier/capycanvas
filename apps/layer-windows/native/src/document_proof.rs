//! Windows transport around the shared print-proof transaction.
use layer_host::{NativeHost, Renderer};
use layer_render_wgpu::snapshot::CaptureControl;
use layer_ui::{
    UiSession,
    proof_panel::{PROOF_INTENTS, PrintProofSettings, ProofSimulation},
    proof_workflow::{ProofPreparation, ProofView, proof_form},
};
use serde_json::{Value, json};
use std::sync::{Arc, atomic::AtomicBool};

pub(super) struct Task {
    job: ProofPreparation,
    form: Value,
    settings: PrintProofSettings,
    lut: Option<Arc<layer_color::ProofLut>>,
    validated: bool,
    preserved: bool,
    applied: bool,
}
impl Task {
    pub fn capture(session: &UiSession<Renderer>, id: u32) -> Result<Self, String> {
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
            job: ProofPreparation::begin(session, Some(id), Some(recipe))?,
            form,
            settings,
            lut: None,
            validated: false,
            preserved: false,
            applied: false,
        })
    }
    pub fn details(&self, cancel: &AtomicBool) -> Result<Value, String> {
        Ok(json!({
            "form": self.form, "settings": self.settings,
            "profiles": crate::color_storage::list(cancel)?,
            "intents": PROOF_INTENTS.map(|choice| json!({
                "value": choice.value, "label": choice.label,
                "bpc_available": PrintProofSettings { intent: choice.value, ..Default::default() }.bpc_available()
            })),
            "simulations": ProofSimulation::CHOICES,
        }))
    }
    pub fn work(
        &mut self,
        mut settings: PrintProofSettings,
        profile_id: Option<String>,
        control: &CaptureControl,
    ) -> Result<(), String> {
        self.validated = false;
        self.preserved = false;
        self.lut = None;
        if let Some(id) = profile_id {
            let profile = crate::color_storage::profile(&id, control.cancellation_flag())?;
            settings.profile = Some(layer_ui::ExportProfile {
                channels: layer_color::profile_channels(&profile)?,
                name: layer_color::profile_description(&profile)?,
                profile,
            });
        }
        self.job.recipe = settings.recipe()?;
        self.settings = settings;
        self.lut = Some(self.job.build(|| control.is_cancelled())?);
        Ok(())
    }
    pub fn validate(&mut self, host: &NativeHost, control: &CaptureControl) -> Result<(), String> {
        crate::document_io::check_cancelled(control.cancellation_flag())?;
        self.job.validate(&host.session)?;
        self.validated = true;
        Ok(())
    }
    pub fn preserve(&mut self, control: &CaptureControl) -> Result<(), String> {
        if !self.validated || self.lut.is_none() {
            return Err("Validate the prepared proof first".into());
        }
        crate::document_io::check_cancelled(control.cancellation_flag())?;
        if let Some(bytes) = self.job.preservation() {
            crate::color_storage::preserve(bytes, control.cancellation_flag())?;
        }
        self.preserved = true;
        Ok(())
    }
    pub fn adopt(&mut self, host: &mut NativeHost, control: &CaptureControl) -> Result<(), String> {
        self.validate(host, control)?;
        if self.lut.is_none() || !self.preserved {
            return Err("Prepare the proof before applying it".into());
        }
        let previous = host.session.state().revision;
        let change = self.job.apply(&mut host.session, self.preserved)?;
        host.apply_change(previous, change);
        host.dirty = true;
        self.applied = true;
        Ok(())
    }
    pub fn retain(&self, view: &mut ProofView) -> Result<(), String> {
        if self.applied {
            view.retain(&self.job, self.lut.clone().ok_or("Proof is not prepared")?)?;
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
