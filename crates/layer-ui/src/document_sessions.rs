//! Window-owned drawing membership and inactive resource policy. The active
//! editor stays in the host's existing canvas slot; every parked owner lives
//! here. Hosts supply completed capture inventories and schedule I/O/GPU work.
use crate::{DocumentFileState, DocumentTabs, PixelClip, Localizer, MessageId, FluentArgs};
use layer_core::{Project, raster_storage::RetainedTiles};
use serde::Serialize;
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
            tabs: Default::default(), parked: Default::default(), clock: 0,
            untitled: DocumentTabLabel::untitled(1, localization),
            budget: Default::default(), storage_error: None, clip: None,
        }
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
        self.park(self.selected(), outgoing, tiles);
        let id = self.tabs.add();
        self.untitled = DocumentTabLabel::untitled(id, localization);
        id
    }
    /// A host such as Web can leave a fresh drawing after closing the final
    /// tab. Its identity must be new, without retaining the discarded owner.
    pub fn start_empty(&mut self, localization: &Localizer) -> Result<u64, String> {
        if !self.tabs.order().is_empty() || !self.parked.is_empty() {
            return Err("Drawings are still open".into());
        }
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
    ) -> Result<T, (String, T)> {
        let Some(next) = self.parked.remove(&id) else {
            return Err(("Drawing tab is no longer open".into(), outgoing));
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
    ) -> Result<(), String> {
        self.exchange_with(id, tiles, |incoming| std::mem::swap(active, incoming))
    }
    /// Swap a host slot through its owner's representation (for example a
    /// boxed editor), without moving large sessions through collection frames.
    pub fn exchange_with(&mut self, id:u64, tiles:RetainedTiles, swap:impl FnOnce(&mut T)) -> Result<(),String> {
        let Some(mut incoming) = self.parked.remove(&id) else {
            return Err("Drawing tab is no longer open".into());
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
    pub fn admit(&self, active: &RetainedTiles, candidate: &Project) -> Result<(), String> {
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

pub struct DocumentAdmission {
    existing: usize,
    limit: usize,
    storage_error: Option<String>,
}
impl DocumentAdmission {
    pub fn admit(&self, candidate: &Project) -> Result<(), String> {
        if let Some(error) = &self.storage_error {
            return Err(format!(
                "{error}\nFree disk space or close some tabs before opening another drawing."
            ));
        }
        let metadata = layer_core::Editor::new(candidate.document.clone())
            .retained_tiles()
            .metadata_bytes;
        if self
            .existing
            .saturating_add(metadata)
            .saturating_add(2 * 1024 * 1024)
            > self.limit
        {
            return Err("Too much drawing data is open. Save and close some tabs before opening another drawing.".into());
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
        args.set("number", id.to_string());
        localization.format(MessageId::DOCUMENTS_UNTITLED_NUMBERED, &args)
    }
    fn with_title(id: u64, file: &DocumentFileState, title: String, localization: &Localizer) -> Self {
        Self {
            id,
            title,
            modified: file.modified,
            uri: file.location.as_ref().map(|l| l.uri.clone()),
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
        let project = Project {
            document: layer_core::Document::new("candidate", 16, 16, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }),
        };
        assert!(tabs.admit(&inventory(), &project).is_err());
        tabs.storage_completed(Err("Disk full".into()));
        assert!(
            tabs.admit(&inventory(), &project)
                .unwrap_err()
                .contains("Disk full")
        );
        assert_eq!(tabs.exchange(1, "second", inventory()).unwrap(), "first");
        assert_eq!(tabs.order(), &[1, 2]);
    }
}
