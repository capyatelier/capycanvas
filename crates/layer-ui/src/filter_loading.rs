//! Atomic package publication and live instance migration. Hosts supply bytes
//! and drive frames; all acceptance, invalidation and UI policy stays here.
use super::*;
use layer_core::{Edit, EffectCatalog, EffectInstallMode, EffectPackage};
use layer_render::EffectValidationRequest;
use std::sync::Arc;

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct FilterLoadState {
    pub request_id: u64,
    pub pending: bool,
    pub error: Option<String>,
}
pub(super) struct Pending {
    catalog: EffectCatalog,
    validated: bool,
    migrate_instances: bool,
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
            return Err("A filter package is already being validated".into());
        }
        let package = EffectPackage::parse(json)?.resolve(read)?;
        let candidate = self.effect_catalog.stage(package, mode)?;
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
        let mut namespace: Vec<_> = candidate.filters().iter().map(|f| f.program()).collect();
        // Self-contained documents may contain programs absent from the catalog.
        // Programs being replaced are deliberately excluded from this namespace.
        for layer in &self.engine.document().layers {
            if matches!(mode, EffectInstallMode::Add)
                && let Some(effect) = &layer.effect
                && changed
                    .iter()
                    .any(|p| p.id == effect.program.id && *p != effect.program)
            {
                return Err(format!(
                    "Filter ID already belongs to a document program: {}",
                    effect.program.id
                ));
            }
            if let Some(effect) = &layer.effect
                && (!migrate_instances || !changed.iter().any(|p| p.id == effect.program.id))
                && !namespace.contains(&effect.program)
            {
                namespace.push(effect.program.clone());
            }
        }
        let request_id = self.state.filter_load.request_id.wrapping_add(1);
        let accepted = self
            .engine
            .backend_mut()
            .request_effect_validation(EffectValidationRequest {
                request_id,
                programs: changed,
                namespace,
            })
            .map_err(|e| e.to_string())?;
        if !accepted {
            return Err("The GPU is unavailable or busy validating filters".into());
        }
        self.pending_filters = Some(Pending {
            catalog: candidate,
            validated: false,
            migrate_instances,
        });
        self.state.filter_load = FilterLoadState {
            request_id,
            pending: true,
            error: None,
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
                self.state.filter_load.error = Some(error);
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
        self.state.filter_load.error = result.err();
        regions::DOCUMENT | regions::COMMANDS
    }

    fn publish_filters(
        &mut self,
        catalog: EffectCatalog,
        migrate_instances: bool,
    ) -> Result<(), String> {
        let mut edits = Vec::new();
        for layer in self
            .engine
            .document()
            .layers
            .iter()
            .filter(|_| migrate_instances)
        {
            let Some(effect) = &layer.effect else {
                continue;
            };
            let Some(definition) = catalog.get(&effect.program.id) else {
                continue;
            };
            // Do not replace document-specific programs on an unrelated import.
            if self
                .effect_catalog
                .get(definition.id())
                .is_some_and(|f| f.program == definition.program)
                || effect.program == definition.program
            {
                continue;
            }
            let replacement = effect
                .rebind(definition.program())
                .map_err(|e| format!("Cannot update {}: {e}", definition.label()))?;
            let mut layer = layer.clone();
            layer.effect = Some(Arc::new(replacement));
            edits.push(Edit::ReplaceLayer(Box::new(layer)));
        }
        if !edits.is_empty() {
            self.layer_edit(Edit::Batch(edits))?;
        }
        self.effect_catalog = catalog;
        self.state.filter_catalog_revision = self.state.filter_catalog_revision.wrapping_add(1);
        self.state.adjustments = effects::catalog(&self.effect_catalog, &self.state.filter_picker);
        self.state.filter_categories = effects::categories(&self.effect_catalog);
        self.refresh_document();
        self.refresh_commands();
        Ok(())
    }
}
