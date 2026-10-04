//! Atomic package publication and live instance migration. Hosts supply bytes
//! and drive frames; all acceptance, invalidation and UI policy stays here.
use super::*;
use layer_core::{Edit, EffectCatalog, EffectInstallMode, EffectPackage, EffectInstance, authored::RecordChange};
use layer_render::EffectValidationRequest;
use std::sync::Arc;

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct FilterLoadState {
    pub request_id: u64,
    pub pending: bool,
    pub error: Option<String>,
    #[serde(skip)]
    error_source: Option<MessageId>,
}
impl FilterLoadState {
    pub fn localized_error(&self, localization: &Localizer) -> Option<String> {
        self.error_source.map(|message| localization.text(message).to_string()).or_else(|| self.error.clone())
    }
    pub(super) fn set_localization(&mut self, localization: &Localizer) {
        self.error = self.localized_error(localization);
    }
}
pub(super) struct Pending {
    catalog: EffectCatalog,
    pub(super) validation: EffectValidationRequest,
    pub(super) validated: bool,
    migrate_instances: bool,
}
impl Pending {
    pub(super) fn library_only(&self) -> bool {
        !self.migrate_instances
    }
}
impl<R: CanvasRenderer> UiSession<R> {
    /// Hosts acquiring package bytes asynchronously can retain them until this
    /// boundary. Loading still validates the package and renderer availability.
    pub fn can_stage_effect_package(&self) -> bool {
        self.pending_filters.is_none()
            && !self.state.document_file.busy
            && self.require_idle().is_ok()
    }

    /// The returned change wakes frame polling, including on an idle canvas.
    /// Existing layers/catalog remain usable until validation and an idle
    /// document boundary. Only one cold package compilation runs at a time.
    pub fn load_effect_package(
        &mut self,
        json: &str,
        read: impl FnMut(&str) -> Result<Arc<str>, String>,
        mode: EffectInstallMode,
    ) -> Result<UiChange, String> {
        self.stage_effect_package(json, read, mode, true)
    }

    /// Startup/library refresh must not silently rewrite a reopened project's
    /// embedded programs. Explicit package replacement can still migrate them.
    pub fn load_effect_library(
        &mut self,
        json: &str,
        read: impl FnMut(&str) -> Result<Arc<str>, String>,
        mode: EffectInstallMode,
    ) -> Result<UiChange, String> {
        self.stage_effect_package(json, read, mode, false)
    }

    fn stage_effect_package(
        &mut self,
        json: &str,
        read: impl FnMut(&str) -> Result<Arc<str>, String>,
        mode: EffectInstallMode,
        migrate_instances: bool,
    ) -> Result<UiChange, String> {
        self.require_idle()?;
        if self.pending_filters.is_some() {
            return Err(self.state.localization.text(MessageId::RESOURCES_ERROR_PACKAGE_PENDING).to_string());
        }
        let package = EffectPackage::parse(json).and_then(|p| p.resolve(read)).map_err(|error| {
            eprintln!("Filter package: {error}");
            self.state.localization.text(MessageId::RESOURCES_PACKAGE_FAILED).to_string()
        })?;
        effects::validate_catalog_labels(&package, &self.state.localization)?;
        let candidate = self.effect_catalog.stage(package, mode).map_err(|error| {
            eprintln!("Filter catalog: {error}");
            self.state.localization.text(MessageId::RESOURCES_PACKAGE_CONFLICT).to_string()
        })?;
        let changed: Vec<_> = candidate
            .filters()
            .iter()
            .filter(|f| {
                self.effect_catalog
                    .get(f.id())
                    .is_none_or(|old| old.program != f.program)
            })
            .map(|f| f.program())
            .collect();
        // The bundled catalog is already accepted. A refresh with no new code
        // only updates library metadata; it must not lock the document or ask
        // the renderer to compile every unused program again.
        if !migrate_instances && changed.is_empty() {
            if candidate.filters() == self.effect_catalog.filters()
                && candidate.categories() == self.effect_catalog.categories()
            {
                return Ok(UiChange::default());
            }
            self.publish_filters(candidate, false)?;
            return Ok(self.changed(regions::DOCUMENT | regions::COMMANDS, false));
        }
        let mut namespace: Vec<_> = candidate.filters().iter().map(|f| f.program()).collect();
        // Self-contained documents may contain programs absent from the catalog.
        // Programs being replaced are deliberately excluded from this namespace.
        for (_, _, definition) in self.engine.document().artwork.definitions.iter() {
            let program = &definition.program;
            if matches!(mode, EffectInstallMode::Add)
                && changed.iter().any(|p| p.id == program.id && p != program)
            {
                return Err(self.state.localization.text(MessageId::RESOURCES_DOCUMENT_FILTER_CONFLICT).to_string());
            }
            if (!migrate_instances || !changed.iter().any(|p| p.id == program.id))
                && !namespace.contains(program)
            {
                namespace.push(program.clone());
            }
        }
        let request_id = self.state.filter_load.request_id.wrapping_add(1);
        let validation = EffectValidationRequest {
            request_id,
            programs: changed,
            namespace,
        };
        let accepted = self
            .engine
            .backend_mut()
            .request_effect_validation(validation.clone())
            .map_err(|error| {
                eprintln!("Filter validation request: {error}");
                self.state.localization.text(MessageId::RESOURCES_PACKAGE_VALIDATION_FAILED).to_string()
            })?;
        if !accepted {
            return Err(self.state.localization.text(MessageId::RESOURCES_ERROR_DEVICE_BUSY).to_string());
        }
        self.pending_filters = Some(Pending {
            catalog: candidate,
            validation,
            validated: false,
            migrate_instances,
        });
        self.state.filter_load = FilterLoadState {
            request_id,
            pending: true,
            ..Default::default()
        };
        Ok(self.changed(regions::DOCUMENT, true))
    }

    pub(super) fn poll_filter_installation(&mut self) -> u32 {
        let Some(pending) = &mut self.pending_filters else {
            return 0;
        };
        if let Some(result) = self.engine.backend_mut().take_effect_validation()
            && result.request_id == self.state.filter_load.request_id
        {
            if let Err(error) = result.result {
                self.pending_filters = None;
                self.state.filter_load.pending = false;
                eprintln!("Filter validation: {error}");
                self.state.filter_load.error_source = Some(MessageId::RESOURCES_PACKAGE_VALIDATION_FAILED);
                self.state.filter_load.set_localization(&self.state.localization);
                return regions::DOCUMENT;
            }
            pending.validated = true;
        }
        if !pending.validated
            || self.require_idle().is_err()
            || self.engine.has_pending_document_edits()
        {
            return 0;
        }
        let pending = self.pending_filters.take().unwrap();
        let result = self.publish_filters(pending.catalog, pending.migrate_instances);
        self.state.filter_load.pending = false;
        self.state.filter_load.error_source = None;
        self.state.filter_load.error = result.err();
        regions::DOCUMENT | regions::COMMANDS
    }

    fn publish_filters(
        &mut self,
        catalog: EffectCatalog,
        migrate_instances: bool,
    ) -> Result<(), String> {
        let mut edits = Vec::new();
        let artwork = &self.engine.document().artwork;
        for (handle, _, authored) in artwork.definitions.iter().filter(|_| migrate_instances) {
            let Some(definition) = catalog.get(&authored.program.id) else { continue; };
            if self.effect_catalog.get(definition.id()).is_some_and(|f| f.program == definition.program)
                || authored.program == definition.program
            { continue; }
            for (effect, _, application) in artwork.effects.iter().filter(|(_, _, application)| application.definition == handle) {
                let draft = EffectInstance {program: authored.program.clone(), values: application.values.clone()};
                let replacement = draft.rebind(definition.program()).map_err(|error| {
                    eprintln!("Filter update {}: {error}", definition.id());
                    let mut args = fluent_bundle::FluentArgs::new();
                    args.set("name", effects::resource_label(definition.label(), &self.state.localization).to_string());
                    self.state.localization.format(MessageId::RESOURCES_UPDATE_FAILED, &args)
                })?;
                let application = layer_core::authored::EffectApplication {definition: application.definition, domain: application.domain, values: replacement.values};
                edits.push(Edit::Effect(RecordChange::replace(&artwork.effects, effect, Some(application)).map_err(str::to_string)?));
            }
            let mut authored = authored.clone();
            authored.program = definition.program();
            authored.dimensions.retain(|key, _| authored.program.parameters.iter().any(|parameter| parameter.key == *key));
            edits.push(Edit::Definition(RecordChange::replace(&artwork.definitions, handle, Some(authored)).map_err(str::to_string)?));
        }
        if !edits.is_empty() {
            self.layer_edit(Edit::Batch(edits))?;
        }
        self.effect_catalog = catalog;
        self.state.filter_catalog_revision = self.state.filter_catalog_revision.wrapping_add(1);
        self.state.adjustments = effects::catalog(&self.effect_catalog, &self.state.filter_picker, &self.state.localization);
        self.state.filter_categories = effects::categories(&self.effect_catalog, &self.state.localization);
        self.refresh_document();
        self.refresh_commands();
        Ok(())
    }
}

#[cfg(test)]
mod localization_tests {
    use super::*;
    use crate::session::test_support::{package_json, session};

    #[test]
    fn language_refresh_retains_filter_validation_progress_and_failure() {
        let mut session = session(Platform::Gtk);
        let mut definition = session.effect_catalog.get("unsharp_mask").unwrap().clone();
        Arc::make_mut(&mut definition.program).label = "Literal imported filter 日本語".into();
        let package = package_json(session.effect_catalog.categories().to_vec(), vec![definition]);
        session.load_effect_package(&package, |_| panic!("inline package"), EffectInstallMode::Replace).unwrap();
        let validation = session.engine.backend().validation.clone().unwrap();
        let request_id = session.state.filter_load.request_id;
        let catalog_revision = session.state.filter_catalog_revision;
        let document = session.engine.document().clone();
        let checkpoint = session.engine.checkpoint();
        let rendering = (session.engine.backend().composites, session.engine.backend().dabs);
        for language in [UiLanguage::Japanese, UiLanguage::Korean] {
            assert!(session.set_localization(Localizer::shared(language)));
            assert!(session.state.filter_load.pending);
            assert_eq!(session.state.filter_load.request_id, request_id);
            assert!(session.state.filter_load.error.is_none());
            let queued = session.engine.backend().validation.as_ref().unwrap();
            assert_eq!(queued.request_id, validation.request_id);
            assert_eq!(queued.programs.len(), validation.programs.len());
            assert!(queued.programs.iter().zip(&validation.programs).all(|(a, b)| Arc::ptr_eq(a, b)));
            assert_eq!(queued.namespace.len(), validation.namespace.len());
            assert!(queued.namespace.iter().zip(&validation.namespace).all(|(a, b)| Arc::ptr_eq(a, b)));
            assert_eq!(session.pending_filters.as_ref().unwrap().validation.request_id, request_id);
        }
        session.engine.backend_mut().validation_result = Some(layer_render::EffectValidationResult {
            request_id, result: Err("literal shader diagnostic".into()),
        });
        assert_eq!(session.poll_filter_installation(), regions::DOCUMENT);
        assert!(!session.state.filter_load.pending);
        assert!(session.pending_filters.is_none());
        let retained = session.state.filter_load.clone();
        session.engine.backend_mut().validation = None;
        for language in UiLanguage::ALL {
            let localization = Localizer::shared(language);
            session.set_localization(localization.clone());
            assert_eq!(session.state.filter_load.error.as_deref(), Some(localization.text(MessageId::RESOURCES_PACKAGE_VALIDATION_FAILED).as_ref()));
            assert_eq!(session.state.filter_load.error, retained.localized_error(&localization));
            assert_eq!(session.state.filter_load.request_id, request_id);
            assert!(!session.state.filter_load.pending);
            assert!(session.pending_filters.is_none());
            assert!(session.engine.backend().validation.is_none());
            assert_eq!(session.state.filter_catalog_revision, catalog_revision);
            assert_eq!(session.engine.document(), &document);
            assert_eq!(session.engine.checkpoint(), checkpoint);
            assert_eq!((session.engine.backend().composites, session.engine.backend().dabs), rendering);
        }
        let serialized = serde_json::to_value(&session.state.filter_load).unwrap();
        assert_eq!(serialized.as_object().unwrap().len(), 3);
    }
}
