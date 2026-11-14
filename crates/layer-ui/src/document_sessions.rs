//! Window-owned drawing membership and inactive resource policy. The active
//! editor stays in the host's existing canvas slot; every parked owner lives
//! here. Hosts supply completed capture inventories and schedule I/O/GPU work.
use crate::{DocumentFileState, DocumentTabs, PixelClip, Localizer, MessageId, FluentArgs};
use layer_core::{Document, MemoryBudget, MemoryOwner, MemoryOwnerKind, MemoryClass, raster_storage::RetainedTiles};
use serde::{Serialize, Deserialize};
#[cfg(test)]
use layer_core::MemoryReport;
use std::{collections::BTreeMap, ops::Deref};

#[derive(Clone, Copy, Default)]
pub struct RetainedClipboard;

#[derive(Default)]
struct ClipboardState {
    sequence: u64,
    publication: u64,
    clip: Option<PixelClip>,
}
#[cfg(not(target_arch = "wasm32"))]
static CLIPBOARD: std::sync::Mutex<ClipboardState> = std::sync::Mutex::new(ClipboardState { sequence: 0, publication: 0, clip: None });
#[cfg(target_arch = "wasm32")]
thread_local! {
    static CLIPBOARD: std::cell::RefCell<ClipboardState> = const { std::cell::RefCell::new(ClipboardState { sequence: 0, publication: 0, clip: None }) };
}

impl RetainedClipboard {
    fn with<T>(apply: impl FnOnce(&mut ClipboardState) -> T) -> T {
        #[cfg(not(target_arch = "wasm32"))]
        { apply(&mut CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner())) }
        #[cfg(target_arch = "wasm32")]
        { CLIPBOARD.with_borrow_mut(apply) }
    }
    pub fn get(&self) -> Option<PixelClip> {
        Self::with(|state| state.clip.clone())
    }
    pub fn capture(&self, nonce: &str, localization: &Localizer) -> Result<PixelClip, String> {
        self.get().filter(|clip| clip.nonce == nonce)
            .ok_or_else(|| crate::DocumentDeliveryMessage::ClipboardChanged.message(localization))
    }
    pub fn publication() -> u64 {
        Self::with(|state| { state.sequence += 1; state.sequence })
    }
    pub fn set_published(&self, clip: PixelClip, publication: u64) {
        Self::with(|state| {
            if publication > state.publication && publication <= state.sequence {
                state.publication = publication;
                state.clip = Some(clip);
            }
        });
    }
    pub fn set(&self, clip: PixelClip) {
        self.set_published(clip, Self::publication());
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DocumentBudget {
    pub inactive_ram: usize,
    pub metadata: usize,
}
impl Default for DocumentBudget {
    fn default() -> Self {
        Self {
            inactive_ram: usize::MAX,
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

#[path = "document_sessions/trim.rs"]
mod trim;
pub use trim::TrimWait;
mod pressure;
pub use pressure::StoragePressure;

pub struct DocumentSessions<T> {
    tabs: DocumentTabs,
    parked: BTreeMap<u64, ParkedDocument<T>>,
    clock: u64,
    language: crate::UiLanguage,
    untitled: String,
    pub budget: DocumentBudget,
    pub clip: RetainedClipboard,
    memory: MemoryBudget,
    memory_owner: MemoryOwner,
    reported: std::cell::Cell<usize>,
    pressure:StoragePressure,
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
    pub fn reserve_identities(&mut self, ids: &[u64]) -> Result<(), String> {
        self.tabs.reserve_identities(ids)
    }
    pub fn restore_identity(&mut self,id:u64)->Result<(),String> {
        self.tabs.restore_identity(id)?;
        self.untitled=DocumentTabLabel::untitled(id,&Localizer::shared(self.language));
        Ok(())
    }
    pub fn restore_order(&mut self,order:&[u64],selected:u64)->Result<(),String> {
        self.tabs.restore_order(order,selected)
    }
    pub fn localized(localization: &Localizer) -> Self {
        Self::with_memory_budget(localization, MemoryBudget::process())
    }
    fn with_memory_budget(localization: &Localizer, memory: MemoryBudget) -> Self {
        let memory_owner = memory.register_owner(MemoryOwnerKind::ParkedTabs, MemoryClass::Warm, None);
        Self {
            tabs: Default::default(), parked: Default::default(), clock: 0, language: localization.language(),
            untitled: DocumentTabLabel::untitled(1, localization),
            budget: Default::default(), clip: RetainedClipboard,
            pressure:StoragePressure::new(memory_owner.clone(),memory.clone()), memory, memory_owner, reported: std::cell::Cell::new(0),
        }
    }
    pub fn trim_wait(&self)->Option<TrimWait> {self.pressure.wait()}
    pub fn trim_in_flight(&self)->bool {self.pressure.in_flight()}
    pub fn set_localization(&mut self, localization: &Localizer) -> bool {
        if self.language == localization.language() { return false; }
        self.language = localization.language();
        self.untitled = DocumentTabLabel::untitled(self.selected(), localization);
        for (&id, parked) in &mut self.parked {
            parked.untitled = DocumentTabLabel::untitled(id, localization);
        }
        true
    }

    pub fn upcoming_id(&self) -> u64 { self.tabs.upcoming() }

    pub fn upcoming_untitled(&self, localization: &Localizer) -> String {
        DocumentTabLabel::untitled(self.tabs.upcoming(), localization)
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
        tiles.set_parked(true);
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
        self.refresh_memory(true);
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
    pub fn append_parked(&mut self, owner: T, tiles: RetainedTiles, localization: &Localizer) -> u64 {
        assert_ne!(self.selected(), 0, "an inactive drawing needs an active owner");
        self.set_localization(localization);
        let selected = self.selected();
        let id = self.tabs.add();
        let untitled = std::mem::replace(&mut self.untitled, DocumentTabLabel::untitled(id, localization));
        self.park(id, owner, tiles);
        self.untitled = untitled;
        self.tabs.select(selected);
        id
    }
    pub fn append_parked_with_id(&mut self, id: u64, owner: T, tiles: RetainedTiles, localization: &Localizer) -> Result<(), (String, T)> {
        if let Err(error) = self.tabs.add_parked_identity(id) { return Err((error, owner)); }
        self.set_localization(localization);
        let untitled = std::mem::replace(&mut self.untitled, DocumentTabLabel::untitled(id, localization));
        self.park(id, owner, tiles);
        self.untitled = untitled;
        Ok(())
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
    pub fn prepare_empty(&self, localization: &Localizer) -> Result<Self, DocumentSessionError> {
        if self.order().len() != 1 || !self.parked.is_empty() {
            return Err(DocumentSessionError::DrawingsStillOpen);
        }
        let mut next = Self::with_memory_budget(localization, self.memory.clone());
        next.tabs = self.tabs.clone();
        next.tabs.close(self.selected());
        let id = next.tabs.add();
        next.untitled = DocumentTabLabel::untitled(id, localization);
        next.budget = self.budget;
        next.storage_completed(self.storage_error().map_or(Ok(()),Err));
        Ok(next)
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
        next.tiles.set_parked(false);
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
        incoming.tiles.set_parked(false);
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
        let next = self.parked.remove(&self.selected()).map(|next| {
            self.untitled = next.untitled;
            next.tiles.set_parked(false);
            next.owner
        });
        self.refresh_memory(true);
        next
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
    fn refresh_memory(&self, force: bool) {
        let resident = self.resident_bytes();
        let previous = self.reported.get();
        if force || previous.abs_diff(resident) >= 1024 * 1024 {
            self.reported.set(resident);
            self.pressure.update(self.parked.iter().map(|(&id,parked)|(id,parked.used,parked.tiles.clone())).collect(),resident,self.budget.inactive_ram);
        }
    }
    pub fn inactive_ram_budget(&self) -> usize {
        self.refresh_memory(false);
        self.budget.inactive_ram.min(self.memory_owner.grant().ram_bytes.min(usize::MAX as u64) as usize)
    }
    pub fn pressure_source(&self)->StoragePressure {self.pressure.clone()}
    pub fn set_inactive_ram_budget(&mut self,bytes:usize) {
        if self.budget.inactive_ram!=bytes {self.budget.inactive_ram=bytes;self.refresh_memory(true);}
    }
    pub fn pressure_needed(&self)->bool {self.resident_bytes()>self.inactive_ram_budget()}
    /// Oldest inactive resident payload first. Recount shared handles after each
    /// completion because spilling updates current/undo/redo owners together.
    pub fn spill_candidate(&self) -> Option<(u64, RetainedTiles)> {
        self.pressure_needed()
            .then(|| {
                self.parked
                    .iter()
                    .filter(|(_,p)| p.tiles.resident_bytes() > 0)
                    .min_by_key(|(_,p)| p.used)
                    .map(|(&id,p)| (id,p.tiles.clone()))
            })
            .flatten()
    }
    pub fn storage_error(&self)->Option<String> {self.pressure.error()}
    pub fn storage_completed(&mut self, result: Result<(), String>) {
        self.pressure.storage_completed(result);
    }
    pub fn admit(&self, active: &RetainedTiles, candidate: &Document) -> Result<(), DocumentSessionError> {
        self.admission(active).admit(candidate)
    }
    /// Freeze admission inputs before asynchronous decoding. Recheck against the
    /// live collection before publication if the host permits concurrent opens.
    pub fn admission(&self, active: &RetainedTiles) -> DocumentAdmission {
        let mut owners=std::collections::HashMap::new();let mut existing=0usize;
        for tiles in std::iter::once(active).chain(self.parked.values().map(|parked|&parked.tiles)) {
            existing=existing.saturating_add(tiles.metadata_bytes);tiles.metadata_owners(&mut owners);
        }
        DocumentAdmission {
            existing, owners,
            limit: self.budget.metadata,
            storage_error:self.storage_error(),
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
    owners: std::collections::HashMap<u64,usize>,
    limit: usize,
    storage_error: Option<String>,
}
impl DocumentAdmission {
    pub fn admit_sessions<'a>(&self, editors: impl IntoIterator<Item = &'a layer_core::Editor>) -> Result<(), DocumentSessionError> {
        if let Some(error) = &self.storage_error {
            return Err(DocumentSessionError::StorageUnavailable { detail: error.clone() });
        }
        let mut metadata = self.existing;let mut owners=self.owners.clone();
        for editor in editors {
            let tiles=editor.retained_tiles();tiles.metadata_owners(&mut owners);
            metadata = metadata.saturating_add(tiles.metadata_bytes).saturating_add(2 * 1024 * 1024);
            if metadata > self.limit { return Err(DocumentSessionError::MetadataBudgetExceeded); }
        }
        metadata=owners.values().fold(metadata,|bytes,size|bytes.saturating_add(*size));
        if metadata > self.limit { return Err(DocumentSessionError::MetadataBudgetExceeded); }
        Ok(())
    }
    pub fn admit(&self, candidate: &Document) -> Result<(), DocumentSessionError> {
        self.admit_sessions(std::iter::once(&layer_core::Editor::new(candidate.clone())))
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
    pub fn menu_label(&self, count: usize) -> String {
        format!("{}{}{}", if self.modified { "• " } else { "" }, self.title, if count > 1 { format!(" ({count})") } else { String::new() })
    }
    pub fn menu_item(&self, count: usize) -> crate::ContextMenuItem {
        let mut item = crate::ContextMenuItem::command(self.menu_label(count), crate::UiAction::Invoke { command: crate::CommandId::Drawings });
        item.enabled = count > 1;
        item
    }
    pub fn new(id: u64, file: &DocumentFileState, localization: &Localizer) -> Self {
        let title = if file.location.is_none() && file.unsaved_name.is_none() {
            Self::untitled(id, localization)
        } else { file.title().into() };
        Self::with_title(id, file, title, localization)
    }
    pub(crate) fn untitled(id: u64, localization: &Localizer) -> String {
        let mut args = FluentArgs::new();
        args.set("number", id);
        localization.format(MessageId::DOCUMENTS_UNTITLED_NUMBERED, &args)
    }
    fn with_title(id: u64, file: &DocumentFileState, title: String, localization: &Localizer) -> Self {
        let title=if file.recovered {
            let mut args=FluentArgs::new();args.set("name",title);
            localization.format(MessageId::DOCUMENTS_RECOVERED_NAME,&args)
        } else {title};
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

#[derive(Clone, Debug, PartialEq)]
pub struct DrawingSelector {
    pub label: String,
    pub count: String,
    pub page: crate::ContextMenuItem,
}
impl DrawingSelector {
    pub const DIALOG: crate::DialogIntent=crate::DialogIntent::editor(Some(crate::DialogPrimary::Close));
    pub fn new(labels: &[DocumentTabLabel], current: u64, new: &crate::CommandState, localization: &Localizer) -> Self {
        let caption = |label: &DocumentTabLabel| format!("{}{}", if label.modified { "• " } else { "" }, label.title);
        let rows = labels.iter().map(|label| {
            let mut row = crate::ContextMenuItem::command(caption(label), crate::UiAction::Phone { action: crate::PhoneAction::Drawing { id: label.id, close: false } });
            row.selected = Some(label.id == current);
            row.hint = crate::NativeCaption::CloseDrawing { title: label.title.clone() }.message(localization);
            row
        }).collect();
        let mut create = crate::ContextMenuItem::command(format!("{}…", localization.text(MessageId::DOCUMENTS_NEW)), crate::UiAction::Invoke { command: crate::CommandId::NewDocument });
        create.enabled = new.enabled;
        create.hint = new.shortcut.clone();
        create.bindings = new.bindings.clone();
        let mut page = crate::ContextMenuItem::submenu(&localization.text(MessageId::NATIVE_HEADER_DRAWINGS), vec![rows, vec![create]]);
        page.page = Some(crate::MenuPageId::Drawings);
        Self { label: labels.iter().find(|label| label.id == current).map(caption).unwrap_or_default(),
            count: if labels.len() > 1 { labels.len().to_string() } else { String::new() },
            page }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn english() -> std::sync::Arc<Localizer> { Localizer::shared(crate::UiLanguage::English) }
    fn test_budget(limit: u64) -> MemoryBudget {
        let budget = MemoryBudget::new();
        budget.update_ram(layer_core::MemoryFacts { total_bytes: Some(16 << 30), available_bytes: Some(12 << 30), override_bytes: Some(limit), ..Default::default() }, 0);
        budget
    }
    fn inventory() -> RetainedTiles {
        RetainedTiles::default()
    }
    #[test]
    fn parked_tiles_share_one_warm_owner_and_spill_only_above_its_grant() {
        use layer_core::raster::{RasterData, RasterRevision, RasterTile, TileBlob, TileKey, RasterPlane, TILE_SIZE};
        let budget = test_budget(16 * 1024 * 1024);
        let mut tabs = DocumentSessions::with_memory_budget(&english(), budget.clone());
        let mut document = Document::new(layer_core::PortableId::random(), 64, 64, layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
        let layer_core::SourceTarget::Paint(target) = document.working.target.unwrap() else { panic!() };
        let descriptor = document.composition().color.paint_descriptor();
        let pixels = [23, 47, 83, 255].repeat((TILE_SIZE * TILE_SIZE) as usize);
        document.artwork.paint.get_mut(target).unwrap().raster = RasterRevision::backed(RasterData {
            tiles: [(TileKey { plane: RasterPlane::Color, coordinate: [0; 2] }, RasterTile::backed(TileBlob::encode(descriptor, &pixels).unwrap()))].into(),
            ..Default::default()
        });
        let tiles = layer_core::Editor::new(document).retained_tiles();
        let bytes = tiles.resident_bytes();
        assert!(bytes > 0);
        tabs.append("first", tiles.clone(), &english());
        tabs.append("second", tiles, &english());
        assert_eq!(tabs.resident_bytes(), bytes);
        assert!(tabs.spill_candidate().is_none());
        let row = budget.snapshot().rows[MemoryOwnerKind::ParkedTabs as usize][MemoryClass::Warm as usize];
        assert_eq!(row.owners, 1);
        assert_eq!(row.ram_bytes, bytes as u64);
        let visible = budget.register_owner(MemoryOwnerKind::DecodedSamples, MemoryClass::Visible, None);
        visible.report(MemoryReport { demand_ram_bytes: budget.ram_limit_bytes(), ..Default::default() });
        assert_eq!(tabs.inactive_ram_budget(), 0);
        assert!(tabs.spill_candidate().is_some());
        assert_eq!(tabs.order(), [1, 2, 3]);
        drop(visible);
        assert!(tabs.spill_candidate().is_none());
        drop(tabs);
        assert_eq!(budget.snapshot().ram_physical_bytes, 0);
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn independent_document_collections_keep_the_same_clip_after_the_source_collection_closes() {
        use crate::session::test_support::{Recorder, invoke, assert_live_artwork_eq};
        use layer_core::color::{DocumentColor, source::rgba8_source};
        use std::sync::Arc;
        struct RestoreClipboard(ClipboardState);
        impl Drop for RestoreClipboard {
            fn drop(&mut self) {
                *CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner()) = std::mem::take(&mut self.0);
            }
        }
        let _restore = RestoreClipboard(std::mem::take(&mut *CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner())));
        let first = DocumentSessions::<()>::default();
        let second = DocumentSessions::<()>::default();
        let original = rgba8_source([3, 1], |x, _| [x as u8, 29, 47, 255]);
        let copied = PixelClip {
            nonce: "retained-clipboard-test".into(), name: "Retained pixels".into(),
            source: original.clone(), policy: layer_core::PaintBasePolicy::WorkingPixels,
            origin: [11, 17], color: DocumentColor::default(), blend: Default::default(),
            png: Arc::from(&b"clipboard rendition"[..]), layers: None,
        };
        let mut older = copied.clone();
        older.nonce = "superseded-clipboard-test".into();
        older.source = rgba8_source([3, 1], |x, _| [x as u8, 211, 7, 255]);
        older.origin = [2, 5];
        first.clip.set(older.clone());
        let first_publication = RetainedClipboard::publication();
        let second_publication = RetainedClipboard::publication();
        first.clip.set_published(copied.clone(), second_publication);
        second.clip.set_published(older.clone(), first_publication);
        let unpublished = RetainedClipboard::publication();
        first.clip.set_published(older.clone(), 0);
        first.clip.set_published(older.clone(), unpublished + 1);
        first.clip.set_published(older, second_publication);
        drop(first);
        let localization = Localizer::shared(crate::UiLanguage::English);
        assert_eq!(second.clip.capture("superseded-clipboard-test", &localization).err().unwrap(),
            crate::DocumentDeliveryMessage::ClipboardChanged.message(&localization));
        let retained = second.clip.capture(&copied.nonce, &localization).unwrap();
        assert_eq!(retained.nonce, copied.nonce);
        let document = Document::new(layer_core::PortableId::random(), 64, 64, layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
        let mut pasted = crate::UiSession::new(Recorder { tiled_sources: true, ..Default::default() }, document, [128, 128], crate::Platform::Gtk).unwrap();
        let before = pasted.engine().document().clone();
        pasted.paste_clip(&retained, crate::PasteMode::InPlace).unwrap();
        let document = pasted.engine().document();
        let owner = document.working.occurrence.unwrap();
        let base = document.scene().paint_source(owner).unwrap().base.as_ref().unwrap();
        assert!(Arc::ptr_eq(base.image.storage(), &original));
        assert_eq!(document.layer_offset(owner), copied.origin);
        invoke(&mut pasted, crate::CommandId::Undo);
        assert_live_artwork_eq(pasted.engine().document(), &before);
        assert_eq!(pasted.engine().document().working.occurrence, before.working.occurrence);
        assert_eq!(pasted.engine().document().working.target, before.working.target);
    }
    #[test]
    fn session_admission_counts_the_whole_batch_before_gpu_preparation() {
        let document = Document::new(layer_core::PortableId::random(), 64, 64, layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        let editor = layer_core::Editor::new(document);
        let bytes = editor.retained_tiles().metadata_bytes + 2 * 1024 * 1024;
        let admission = DocumentAdmission {existing:0,owners:Default::default(),limit:bytes * 2 - 1,storage_error:None};
        assert!(admission.admit_sessions([&editor]).is_ok());
        assert_eq!(admission.admit_sessions([&editor, &editor]), Err(DocumentSessionError::MetadataBudgetExceeded));
    }
    #[test]
    fn admission_deduplicates_content_trees_across_parked_active_and_candidate_drawings() {
        use layer_core::raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileBlob,TileKey,TILE_SIZE};
        let memory=test_budget(16<<20);
        let mut document=Document::new(layer_core::PortableId::random(),256,256,layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()})
            .with_memory(layer_core::DocumentMemory::new(memory.clone(),None));
        let target=document.working.target.unwrap();
        let descriptor=RasterPlane::Color.descriptor(Default::default());
        *document.target_raster_mut(target).unwrap()=RasterRevision::backed(RasterData {
            tiles:[(TileKey {plane:RasterPlane::Color,coordinate:[0,0]},RasterTile::backed(TileBlob::encode(descriptor,&[31,63,127,255].repeat((TILE_SIZE*TILE_SIZE) as usize)).unwrap()))].into(),
            ..Default::default()
        });
        let editor=layer_core::Editor::new(document);let retained=editor.retained_tiles();
        let mut owners=std::collections::HashMap::new();retained.metadata_owners(&mut owners);
        let trees=owners.values().sum::<usize>();
        assert!(trees>0);
        let mut documents=DocumentSessions::with_memory_budget(&english(),memory);
        documents.append((),retained.clone(),&english());documents.append((),retained.clone(),&english());
        documents.budget.metadata=5*retained.metadata_bytes+4*1024*1024+trees;
        assert!(documents.admission(&retained).admit_sessions([&editor,&editor]).is_ok(),"shared tree allocations count once across the entire admission");
        documents.budget.metadata-=1;
        assert_eq!(documents.admission(&retained).admit_sessions([&editor,&editor]),Err(DocumentSessionError::MetadataBudgetExceeded),"tree metadata remains inside the admission bound");
    }
    #[test]
    fn prepared_empty_collection_retains_allocator_and_original_until_commit() {
        let mut sessions = DocumentSessions::<String>::default();
        sessions.reserve_identities(&[2]).unwrap();
        let prepared = sessions.prepare_empty(&english()).unwrap();
        assert_eq!(sessions.order(), [1]);
        assert_eq!(sessions.selected(), 1);
        assert_eq!(prepared.order(), [3]);
        assert_eq!(prepared.selected(), 3);
        assert_eq!(prepared.untitled, "Untitled 3");
        sessions.append_parked("inactive".into(), inventory(), &english());
        assert!(sessions.prepare_empty(&english()).is_err());
        assert_eq!(sessions.order(), [1, 3]);
    }
    #[test]
    fn restored_inactive_membership_preserves_active_owner_and_caption() {
        let mut sessions = DocumentSessions::default();
        sessions.restore_identity(crate::session_recovery::MAX_SESSION_DRAWING_ID).unwrap();
        let before = sessions.untitled.clone();
        assert_eq!(sessions.append_parked("restored", inventory(), &english()), 1);
        assert_eq!(sessions.selected(), crate::session_recovery::MAX_SESSION_DRAWING_ID);
        assert_eq!(sessions.untitled, before);
        assert_eq!(sessions.parked_owner_mut(1).map(|owner| *owner), Some("restored"));
        let file = DocumentFileState::localized(&english());
        assert_eq!(sessions.labels(&file, |_| &file, &english()).first().unwrap().title, before);
    }
    #[test]
    fn explicit_inactive_identity_preserves_selection_and_returns_refused_owner() {
        let mut sessions = DocumentSessions::default();
        let caption = sessions.untitled.clone();
        sessions.append_parked_with_id(9, Box::new("Restored drawing".to_string()), inventory(), &english()).unwrap();
        assert_eq!(sessions.selected(), 1);
        assert_eq!(sessions.untitled, caption);
        assert_eq!(sessions.order(), [1, 9]);
        let order = sessions.order().to_vec();
        for id in [0, 1, 9, crate::session_recovery::MAX_SESSION_DRAWING_ID + 1] {
            let owner = Box::new("Refused drawing".to_string());
            let identity = std::ptr::from_ref(owner.as_ref());
            let (_, owner) = sessions.append_parked_with_id(id, owner, inventory(), &english()).unwrap_err();
            assert_eq!(std::ptr::from_ref(owner.as_ref()), identity);
            assert_eq!(sessions.order(), order);
            assert_eq!(sessions.selected(), 1);
            assert_eq!(sessions.untitled, caption);
        }
        let file = DocumentFileState::localized(&english());
        assert_eq!(sessions.labels(&file, |_| &file, &english()).iter().map(|label| label.title.as_str()).collect::<Vec<_>>(), ["Untitled 1", "Untitled 9"]);
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
    fn main_menu_drawing_row_tracks_count_and_unsaved_state() {
        let mut label = DocumentTabLabel::new(1, &DocumentFileState::localized(&english()), &english());
        for modified in [false, true] {
            label.modified = modified;
            for count in [1, 2] {
                let item = label.menu_item(count);
                assert_eq!(item.label, label.menu_label(count));
                assert_eq!(item.enabled, count > 1);
                assert_eq!(item.action, Some(crate::UiAction::Invoke { command: crate::CommandId::Drawings }));
                assert_eq!(item.label.starts_with("• "), modified);
                assert_eq!(item.label.ends_with(" (2)"), count == 2);
            }
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
