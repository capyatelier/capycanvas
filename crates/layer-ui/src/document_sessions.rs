//! Window-owned drawing membership and inactive resource policy. The active
//! editor stays in the host's existing canvas slot; every parked owner lives
//! here. Hosts supply completed capture inventories and schedule I/O/GPU work.
use crate::{DocumentFileState, DocumentTabs};
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
}

pub struct DocumentSessions<T> {
    tabs: DocumentTabs,
    parked: BTreeMap<u64, ParkedDocument<T>>,
    clock: u64,
    pub budget: DocumentBudget,
    storage_error: Option<String>,
}
impl<T> Default for DocumentSessions<T> {
    fn default() -> Self {
        Self {
            tabs: Default::default(),
            parked: Default::default(),
            clock: 0,
            budget: Default::default(),
            storage_error: None,
        }
    }
}
impl<T> Deref for DocumentSessions<T> {
    type Target = DocumentTabs;
    fn deref(&self) -> &Self::Target {
        &self.tabs
    }
}
impl<T> DocumentSessions<T> {
    pub fn parked(&self) -> impl Iterator<Item = (&u64, &ParkedDocument<T>)> {
        self.parked.iter()
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
            },
        );
        debug_assert!(old.is_none(), "an active drawing cannot already be parked");
    }
    /// Publish a prepared drawing only after its initiating request completes.
    /// The host installs the new active owner; this retains the outgoing one.
    pub fn append(&mut self, outgoing: T, tiles: RetainedTiles) -> u64 {
        assert_ne!(
            self.selected(),
            0,
            "use start_empty after the final drawing closes"
        );
        self.park(self.selected(), outgoing, tiles);
        self.tabs.add()
    }
    /// A host such as Web can leave a fresh drawing after closing the final
    /// tab. Its identity must be new, without retaining the discarded owner.
    pub fn start_empty(&mut self) -> Result<u64, String> {
        if !self.tabs.order().is_empty() || !self.parked.is_empty() {
            return Err("Drawings are still open".into());
        }
        Ok(self.tabs.add())
    }
    pub fn labels<'a>(
        &'a self,
        active: &'a DocumentFileState,
        file: impl Fn(&'a T) -> &'a DocumentFileState,
    ) -> Vec<DocumentTabLabel> {
        self.order()
            .iter()
            .filter_map(|&id| {
                let state = if id == self.selected() {
                    active
                } else {
                    file(&self.parked.get(&id)?.owner)
                };
                Some(DocumentTabLabel::new(id, state))
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
        Ok(())
    }
    /// Only call after the selected drawing's Save/Discard/Cancel succeeds.
    /// Final-tab behavior belongs to the host; no session can be resurrected by
    /// order history after this membership change.
    pub fn close_selected(&mut self) -> Option<T> {
        self.tabs.close(self.selected());
        self.parked.remove(&self.selected()).map(|next| next.owner)
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
        let assets = candidate
            .assets
            .values()
            .fold(0usize, |n, a| n.saturating_add(a.bytes.len()));
        let metadata = layer_core::Editor::new(candidate.document.clone())
            .retained_tiles()
            .metadata_bytes;
        if self
            .existing
            .saturating_add(assets)
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
    pub fn new(id: u64, file: &DocumentFileState) -> Self {
        Self {
            id,
            title: if file.location.is_none() && file.unsaved_name.is_none() {
                format!("Untitled {id}")
            } else {
                file.title().into()
            },
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
                .unwrap_or_else(|| "Unsaved drawing".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inventory() -> RetainedTiles {
        RetainedTiles::default()
    }
    #[test]
    fn ownership_follows_identity_across_reorder_switch_and_close() {
        let mut tabs = DocumentSessions::default();
        assert_eq!(tabs.append("first", inventory()), 2);
        assert_eq!(tabs.append("second", inventory()), 3);
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
        tabs.append("first", inventory());
        tabs.budget.metadata = 1;
        let project = Project {
            document: layer_core::Document::new("candidate", 16, 16),
            assets: Default::default(),
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
