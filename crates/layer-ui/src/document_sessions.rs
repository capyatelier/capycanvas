//! Window-owned drawing membership and inactive resource policy. The active
//! editor stays in the host's existing canvas slot; every parked owner lives
//! here. Hosts supply completed capture inventories and schedule I/O/GPU work.
use crate::{DocumentFileState, DocumentTabs, PixelClip, Localizer, MessageId, FluentArgs};
use layer_core::{Document, raster_storage::RetainedTiles};
use serde::{Serialize, Deserialize};
use std::{collections::BTreeMap, ops::Deref};

#[derive(Clone, Copy, Debug)]
pub struct DocumentBudget {
    pub inactive_ram: usize,
    pub metadata: usize,
}
impl Default for DocumentBudget {
    fn default() -> Self {
        Self {
            inactive_ram: 64 * 1024 * 1024,
            metadata: 256 * 1024 * 1024,
        }
    }
}

pub struct ParkedDocument<T> {
    pub owner: T,
    pub tiles: RetainedTiles,
    used: u64,
    untitled: String,
}

pub struct DocumentSessions<T> {
    tabs: DocumentTabs,
    parked: BTreeMap<u64, ParkedDocument<T>>,
    clock: u64,
    language: crate::UiLanguage,
    untitled: String,
    pub budget: DocumentBudget,
    storage_error: Option<String>,
    /// The window's last copy, pasted at full depth into any of its drawings.
    pub clip: Option<PixelClip>,
}
impl<T> Default for DocumentSessions<T> {
    fn default() -> Self { Self::localized(&Localizer::shared(crate::UiLanguage::English)) }
}
impl<T> Deref for DocumentSessions<T> {
    type Target = DocumentTabs;
    fn deref(&self) -> &Self::Target {
        &self.tabs
    }
}
impl<T> DocumentSessions<T> {
    pub fn localized(localization: &Localizer) -> Self {
        Self {
            tabs: Default::default(), parked: Default::default(), clock: 0, language: localization.language(),
            untitled: DocumentTabLabel::untitled(1, localization),
            budget: Default::default(), storage_error: None, clip: None,
        }
    }
    pub fn set_localization(&mut self, localization: &Localizer) -> bool {
        if self.language == localization.language() { return false; }
        self.language = localization.language();
        self.untitled = DocumentTabLabel::untitled(self.selected(), localization);
        for (&id, parked) in &mut self.parked {
            parked.untitled = DocumentTabLabel::untitled(id, localization);
        }
        true
    }

    pub fn parked(&self) -> impl Iterator<Item = (&u64, &ParkedDocument<T>)> {
        self.parked.iter()
    }
    pub fn parked_mut(&mut self) -> impl Iterator<Item = (&u64, &mut ParkedDocument<T>)> {
        self.parked.iter_mut()
    }
    pub fn contains_parked(&self, id: u64) -> bool {
        self.parked.contains_key(&id)
    }
    pub fn parked_owner_mut(&mut self, id: u64) -> Option<&mut T> {
        self.parked.get_mut(&id).map(|p| &mut p.owner)
    }
    fn park(&mut self, id: u64, owner: T, tiles: RetainedTiles) {
        self.clock = self.clock.saturating_add(1);
        let old = self.parked.insert(
            id,
            ParkedDocument {
                owner,
                tiles,
                used: self.clock,
                untitled: self.untitled.clone(),
            },
        );
        debug_assert!(old.is_none(), "an active drawing cannot already be parked");
    }
    /// Publish a prepared drawing only after its initiating request completes.
    /// The host installs the new active owner; this retains the outgoing one.
    pub fn append(&mut self, outgoing: T, tiles: RetainedTiles, localization: &Localizer) -> u64 {
        assert_ne!(
            self.selected(),
            0,
            "use start_empty after the final drawing closes"
        );
        self.set_localization(localization);
        self.park(self.selected(), outgoing, tiles);
        let id = self.tabs.add();
        self.untitled = DocumentTabLabel::untitled(id, localization);
        id
    }
    /// A host such as Web can leave a fresh drawing after closing the final
    /// tab. Its identity must be new, without retaining the discarded owner.
    pub fn start_empty(&mut self, localization: &Localizer) -> Result<u64, DocumentSessionError> {
        if !self.tabs.order().is_empty() || !self.parked.is_empty() {
            return Err(DocumentSessionError::DrawingsStillOpen);
        }
        self.set_localization(localization);
        let id = self.tabs.add();
        self.untitled = DocumentTabLabel::untitled(id, localization);
        Ok(id)
    }
    pub fn labels<'a>(
        &'a self,
        active: &'a DocumentFileState,
        file: impl Fn(&'a T) -> &'a DocumentFileState,
        localization: &Localizer,
    ) -> Vec<DocumentTabLabel> {
        self.order()
            .iter()
            .filter_map(|&id| {
                let (state, untitled) = if id == self.selected() {
                    (active, &self.untitled)
                } else {
                    let parked = self.parked.get(&id)?;
                    (file(&parked.owner), &parked.untitled)
                };
                let title = if state.location.is_none() && state.unsaved_name.is_none() {
                    untitled.clone()
                } else { state.title().into() };
                Some(DocumentTabLabel::with_title(id, state, title, localization))
            })
            .collect()
    }
    /// Atomically exchange membership/selection and ownership. An invalid target
    /// returns the outgoing owner unchanged instead of losing a live document.
    pub fn exchange(
        &mut self,
        id: u64,
        outgoing: T,
        tiles: RetainedTiles,
    ) -> Result<T, (DocumentSessionError, T)> {
        let Some(next) = self.parked.remove(&id) else {
            return Err((DocumentSessionError::TabClosed, outgoing));
        };
        self.park(self.selected(), outgoing, tiles);
        self.tabs.select(id);
        self.untitled = next.untitled;
        Ok(next.owner)
    }
    /// Exchange a host's retained active slot without a placeholder editor.
    pub fn exchange_in_place(
        &mut self,
        id: u64,
        active: &mut T,
        tiles: RetainedTiles,
    ) -> Result<(), DocumentSessionError> {
        self.exchange_with(id, tiles, |incoming| std::mem::swap(active, incoming))
    }
    /// Swap a host slot through its owner's representation (for example a
    /// boxed editor), without moving large sessions through collection frames.
    pub fn exchange_with(&mut self, id:u64, tiles:RetainedTiles, swap:impl FnOnce(&mut T)) -> Result<(),DocumentSessionError> {
        let Some(mut incoming) = self.parked.remove(&id) else {
            return Err(DocumentSessionError::TabClosed);
        };
        swap(&mut incoming.owner);
        self.park(self.selected(), incoming.owner, tiles);
        self.tabs.select(id);
        self.untitled = incoming.untitled;
        Ok(())
    }
    /// Only call after the selected drawing's Save/Discard/Cancel succeeds.
    /// Final-tab behavior belongs to the host; no session can be resurrected by
    /// order history after this membership change.
    pub fn close_selected(&mut self) -> Option<T> {
        self.tabs.close(self.selected());
        self.untitled.clear();
        self.parked.remove(&self.selected()).map(|next| {
            self.untitled = next.untitled;
            next.owner
        })
    }
    pub fn reorder(&mut self, id: u64, before: Option<u64>) -> bool {
        self.tabs.reorder(id, before)
    }
    pub fn undo(&mut self) {
        self.tabs.undo();
    }
    pub fn redo(&mut self) {
        self.tabs.redo();
    }
    pub fn resident_bytes(&self) -> usize {
        layer_core::raster_storage::resident_tile_bytes(self.parked.values().map(|p| &p.tiles))
    }
    /// Oldest inactive resident payload first. Recount shared handles after each
    /// completion because spilling updates current/undo/redo owners together.
    pub fn spill_candidate(&self) -> Option<RetainedTiles> {
        (self.resident_bytes() > self.budget.inactive_ram)
            .then(|| {
                self.parked
                    .values()
                    .filter(|p| p.tiles.resident_bytes() > 0)
                    .min_by_key(|p| p.used)
                    .map(|p| p.tiles.clone())
            })
            .flatten()
    }
    pub fn storage_error(&self) -> Option<&str> {
        self.storage_error.as_deref()
    }
    pub fn storage_completed(&mut self, result: Result<(), String>) {
        self.storage_error = result.err();
    }
    pub fn admit(&self, active: &RetainedTiles, candidate: &Document) -> Result<(), DocumentSessionError> {
        self.admission(active).admit(candidate)
    }
    /// Freeze admission inputs before asynchronous decoding. Recheck against the
    /// live collection before publication if the host permits concurrent opens.
    pub fn admission(&self, active: &RetainedTiles) -> DocumentAdmission {
        DocumentAdmission {
            existing: self.parked.values().fold(active.metadata_bytes, |n, p| {
                n.saturating_add(p.tiles.metadata_bytes)
            }),
            limit: self.budget.metadata,
            storage_error: self.storage_error.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentSessionError {
    DrawingsStillOpen,
    TabClosed,
    StorageUnavailable { detail: String },
    MetadataBudgetExceeded,
}
impl DocumentSessionError {
    pub fn message(&self, localization: &Localizer) -> String {
        match self {
            Self::DrawingsStillOpen => localization.text(MessageId::DOCUMENTS_REFUSAL_DRAWINGS_STILL_OPEN).to_string(),
            Self::TabClosed => localization.text(MessageId::DOCUMENTS_REFUSAL_TAB_CLOSED).to_string(),
            Self::MetadataBudgetExceeded => localization.text(MessageId::DOCUMENTS_REFUSAL_ADMISSION_BUDGET).to_string(),
            Self::StorageUnavailable { detail } => {
                let mut args = FluentArgs::new(); args.set("detail", detail.as_str());
                localization.format(MessageId::DOCUMENTS_REFUSAL_ADMISSION_STORAGE, &args)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentTransportRefusal {
    SnapshotChanged,
    OpenSnapshotChanged,
    RestoreOperation,
    RecoveryGpuChanged,
    SwitchGpuChanged,
    PaintingUnavailable,
    RecoveryServiceUnavailable,
    RecoveryInProgress,
    SwitchOperation,
    ChangeInProgress,
    SwitchDialog,
    CloseOperation,
    SelectedChanged,
    OpenOperation,
    BatchOpening,
    OpenDrawingsOperation,
}
impl DocumentTransportRefusal {
    pub fn message(self, localization: &Localizer) -> std::sync::Arc<str> {
        localization.text(match self {
            Self::SnapshotChanged => MessageId::DOCUMENTS_REFUSAL_SNAPSHOT_CHANGED,
            Self::OpenSnapshotChanged => MessageId::DOCUMENTS_REFUSAL_OPEN_SNAPSHOT_CHANGED,
            Self::RestoreOperation => MessageId::DOCUMENTS_REFUSAL_RESTORE_OPERATION,
            Self::RecoveryGpuChanged => MessageId::DOCUMENTS_REFUSAL_RECOVERY_GPU_CHANGED,
            Self::SwitchGpuChanged => MessageId::DOCUMENTS_REFUSAL_SWITCH_GPU_CHANGED,
            Self::PaintingUnavailable => MessageId::DOCUMENTS_REFUSAL_PAINTING_UNAVAILABLE,
            Self::RecoveryServiceUnavailable => MessageId::DOCUMENTS_REFUSAL_RECOVERY_SERVICE_UNAVAILABLE,
            Self::RecoveryInProgress => MessageId::DOCUMENTS_REFUSAL_RECOVERY_IN_PROGRESS,

            Self::SwitchOperation => MessageId::DOCUMENTS_REFUSAL_SWITCH_OPERATION,
            Self::ChangeInProgress => MessageId::DOCUMENTS_REFUSAL_CHANGE_IN_PROGRESS,
            Self::SwitchDialog => MessageId::DOCUMENTS_REFUSAL_SWITCH_DIALOG,
            Self::CloseOperation => MessageId::DOCUMENTS_REFUSAL_CLOSE_OPERATION,
            Self::SelectedChanged => MessageId::DOCUMENTS_REFUSAL_SELECTED_CHANGED,
            Self::OpenOperation => MessageId::DOCUMENTS_REFUSAL_OPEN_OPERATION,
            Self::BatchOpening => MessageId::DOCUMENTS_REFUSAL_BATCH_OPENING,
            Self::OpenDrawingsOperation => MessageId::DOCUMENTS_REFUSAL_OPEN_DRAWINGS_OPERATION,
        })
    }
}
pub fn document_storage_retained(localization: &Localizer, detail: &str) -> String {
    let mut args = FluentArgs::new(); args.set("detail", detail);
    localization.format(MessageId::DOCUMENTS_STORAGE_RETAINED, &args)
}
pub fn document_recovery_unavailable(localization: &Localizer, detail: &str) -> String {
    let mut args = FluentArgs::new(); args.set("detail", detail);
    localization.format(MessageId::DOCUMENTS_RECOVERY_UNAVAILABLE, &args)
}

pub struct DocumentAdmission {
    existing: usize,
    limit: usize,
    storage_error: Option<String>,
}
impl DocumentAdmission {
    pub fn admit(&self, candidate: &Document) -> Result<(), DocumentSessionError> {
        if let Some(error) = &self.storage_error {
            return Err(DocumentSessionError::StorageUnavailable { detail: error.clone() });
        }
        let metadata = layer_core::Editor::new(candidate.clone())
            .retained_tiles()
            .metadata_bytes;
        if self
            .existing
            .saturating_add(metadata)
            .saturating_add(2 * 1024 * 1024)
            > self.limit
        {
            return Err(DocumentSessionError::MetadataBudgetExceeded);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DocumentTabLabel {
    pub id: u64,
    pub title: String,
    pub location: String,
    pub uri: Option<String>,
    pub export_uri: Option<String>,
    pub modified: bool,
}
impl DocumentTabLabel {
    pub fn new(id: u64, file: &DocumentFileState, localization: &Localizer) -> Self {
        let title = if file.location.is_none() && file.unsaved_name.is_none() {
            Self::untitled(id, localization)
        } else { file.title().into() };
        Self::with_title(id, file, title, localization)
    }
    fn untitled(id: u64, localization: &Localizer) -> String {
        let mut args = FluentArgs::new();
        args.set("number", id);
        localization.format(MessageId::DOCUMENTS_UNTITLED_NUMBERED, &args)
    }
    fn with_title(id: u64, file: &DocumentFileState, title: String, localization: &Localizer) -> Self {
        Self {
            id,
            title,
            modified: file.modified,
            uri: file.location.as_ref().map(|l| l.uri.clone()),
            export_uri: file.export_uri.clone(),
            location: file
                .location
                .as_ref()
                .map(|l| {
                    if l.uri.starts_with("browser:") {
                        l.name.clone()
                    } else {
                        l.uri.clone()
                    }
                })
                .unwrap_or_else(|| localization.text(MessageId::DOCUMENTS_UNSAVED_DRAWING).to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn english() -> std::sync::Arc<Localizer> { Localizer::shared(crate::UiLanguage::English) }
    fn inventory() -> RetainedTiles {
        RetainedTiles::default()
    }
    #[test]
    fn tab_language_refresh_retains_membership_and_literal_names() {
        let japanese = Localizer::shared(crate::UiLanguage::Japanese);
        let mut active = DocumentFileState::localized(&english());
        let mut tabs = DocumentSessions::localized(&english());
        tabs.append(active.clone(), inventory(), &english());
        let order = tabs.order().to_vec();
        assert!(tabs.set_localization(&japanese));
        assert!(!tabs.set_localization(&japanese));
        assert_eq!(tabs.order(), order);
        assert_eq!(tabs.labels(&active, |file| file, &japanese).iter().map(|label| label.title.as_str()).collect::<Vec<_>>(), ["無題 1", "無題 2"]);
        active.unsaved_name = Some("Untitled 2".into());
        let labels = tabs.labels(&active, |file| file, &japanese);
        assert_eq!(labels[0].title, "無題 1");
        assert_eq!(labels[1].title, "Untitled 2");
        let before = tabs.untitled.as_ptr();
        assert!(!tabs.set_localization(&japanese));
        assert_eq!(tabs.untitled.as_ptr(), before);
    }

    #[test]
    fn tab_projection_localizes_only_absent_titles_without_mutating_files() {
        let japanese = crate::Localizer::shared(crate::UiLanguage::Japanese);
        let mut file = DocumentFileState::localized(&japanese);
        let before = serde_json::to_value(&file).unwrap();
        assert_eq!(DocumentTabLabel::new(7, &file, &japanese).title, "無題 7");
        assert_eq!(serde_json::to_value(&file).unwrap(), before);
        let mut tabs = DocumentSessions::localized(&japanese);
        tabs.append(file.clone(), inventory(), &japanese);
        assert_eq!(tabs.labels(&file, |file| file, &japanese).iter().map(|label| label.title.as_str()).collect::<Vec<_>>(), ["無題 1", "無題 2"]);
        tabs.exchange(1, file.clone(), inventory()).unwrap();
        assert_eq!(tabs.labels(&file, |file| file, &japanese).iter().map(|label| label.title.as_str()).collect::<Vec<_>>(), ["無題 1", "無題 2"]);
        tabs.close_selected();
        assert_eq!(tabs.labels(&file, |file| file, &japanese)[0].title, "無題 2");
        assert_eq!(serde_json::to_value(&file).unwrap(), before);
        for name in ["Untitled", "Untitled 7", "無題", "{ $number }「絵」🖌️\u{2068}literal\u{2069}"] {
            file.unsaved_name = Some(name.into());
            assert_eq!(DocumentTabLabel::new(7, &file, &japanese).title, name);
            file.location = Some(crate::DocumentLocation { uri: "private:literal".into(), name: name.into() });
            assert_eq!(DocumentTabLabel::new(7, &file, &japanese).title, name);
            assert_eq!(file.title(), name);
            file.location = None;
        }
    }

    #[test]
    fn ownership_follows_identity_across_reorder_switch_and_close() {
        let mut tabs = DocumentSessions::default();
        assert_eq!(tabs.append("first", inventory(), &english()), 2);
        assert_eq!(tabs.append("second", inventory(), &english()), 3);
        assert!(tabs.reorder(3, Some(1)));
        assert_eq!(tabs.exchange(1, "third", inventory()).unwrap(), "first");
        assert_eq!(tabs.selected(), 1);
        tabs.undo();
        assert_eq!(tabs.order(), &[1, 2, 3]);
        assert_eq!(tabs.close_selected(), Some("second"));
        assert_eq!(tabs.selected(), 2);
        assert!(!tabs.can_redo());
        assert_eq!(
            tabs.exchange(99, "second", inventory()).unwrap_err().1,
            "second"
        );
        assert_eq!(tabs.close_selected(), Some("third"));
        assert_eq!(tabs.close_selected(), None);
        assert_eq!(tabs.selected(), 0);
        assert_eq!(tabs.parked().count(), 0);
    }
    #[test]
    fn admission_failure_never_removes_existing_drawings_or_blocks_selection() {
        let mut tabs = DocumentSessions::default();
        tabs.append("first", inventory(), &english());
        tabs.budget.metadata = 1;
        let project = layer_core::Document::new(layer_core::PortableId::random(), 16, 16, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        assert!(tabs.admit(&inventory(), &project).is_err());
        tabs.storage_completed(Err("Disk full".into()));
        assert!(
            tabs.admit(&inventory(), &project)
                .unwrap_err()
                .message(&english())
                .contains("Disk full")
        );
        assert_eq!(tabs.exchange(1, "second", inventory()).unwrap(), "first");
        assert_eq!(tabs.order(), &[1, 2]);
    }
}

#[cfg(test)]
mod refusal_copy_tests {
    use super::*;
    #[test]
    fn active_document_refusals_retain_cached_copy_and_literal_detail() {
        let localization = Localizer::shared(crate::UiLanguage::Japanese);
        let a = DocumentTransportRefusal::SwitchOperation.message(&localization);
        let b = DocumentTransportRefusal::SwitchOperation.message(&localization);
        assert!(std::sync::Arc::ptr_eq(&a, &b));
        assert_eq!(a.as_ref(), "描画を切り替える前に、現在の操作を終えてください。");
        assert_eq!(DocumentSessionError::TabClosed.message(&localization), "描画タブはすでに閉じています");
        let detail = "literal {detail} 🖌";
        let message = document_storage_retained(&localization, detail);
        assert!(message.contains(detail));
        assert!(message.contains("描画はメモリ内に保持されています"));
        assert!(document_recovery_unavailable(&localization, detail).contains(detail));
    }
}
