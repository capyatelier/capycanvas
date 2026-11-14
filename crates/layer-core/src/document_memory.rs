use crate::{MemoryBudget, MemoryClass, MemoryOwner, MemoryOwnerKind, MemoryReport, MemoryReservation};
use std::sync::{Arc, Weak, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}};

#[derive(Clone)]
pub struct DocumentMemory {
    pub(crate) owner: Arc<Owner>,
    pub(crate) reservation: Option<MemoryReservation>,
    preview_inventory: Option<Arc<crate::raster_storage::RetainedTiles>>,
    _capture: Option<Arc<CaptureMemory>>,
}
struct CaptureMemory { inventory: Arc<crate::raster_storage::RetainedTiles>, owner: Weak<Owner>, reservation: Option<MemoryReservation> }
impl Drop for CaptureMemory {
    fn drop(&mut self) {
        if let Some(owner)=self.owner.upgrade() {
            let current=owner.inventories.lock().unwrap().current.clone();
            if owner.parked.load(Ordering::Relaxed) || !Arc::ptr_eq(&self.inventory,&current) { owner.refresh(); }
        }
    }
}
#[derive(Default)]
struct Inventories { current: Arc<crate::raster_storage::RetainedTiles>, captures: Vec<Weak<CaptureMemory>> }
pub(crate) struct Owner {
    storage_identity:u64,
    inventories: Mutex<Inventories>,
    captured_resident: AtomicU64,
    memory: MemoryOwner,
    history: MemoryOwner,
    budget: MemoryBudget,
    resident: AtomicU64,
    history_resident: AtomicU64,
    metadata: AtomicU64,
    history_metadata: AtomicU64,
    parked: AtomicBool,
    initialized: AtomicBool,
    package:AtomicBool,package_pending:AtomicBool,presented:AtomicBool,
    storage:Mutex<Option<crate::package::session_store::DrawingStore>>,
}
impl std::fmt::Debug for DocumentMemory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_struct("DocumentMemory").finish_non_exhaustive() }
}
impl Default for DocumentMemory {
    fn default() -> Self { Self::new(MemoryBudget::process(), None) }
}
impl DocumentMemory {
    pub fn new(budget: MemoryBudget, reservation: Option<MemoryReservation>) -> Self {
        static NEXT:AtomicU64=AtomicU64::new(1);
        Self { _capture: None, owner: Arc::new(Owner {storage_identity:NEXT.fetch_add(1,Ordering::Relaxed), inventories:Mutex::new(Inventories::default()), captured_resident:AtomicU64::new(0), memory: budget.register_owner(MemoryOwnerKind::Authored, MemoryClass::Pinned, None),
            history: budget.register_owner(MemoryOwnerKind::History, MemoryClass::Pinned, None), budget,
            resident: AtomicU64::new(0), history_resident: AtomicU64::new(0), metadata: AtomicU64::new(0), history_metadata: AtomicU64::new(0), parked: AtomicBool::new(false), initialized: AtomicBool::new(false),package:AtomicBool::new(false),package_pending:AtomicBool::new(false),presented:AtomicBool::new(false),storage:Default::default() }), reservation, preview_inventory: None }
    }
    pub fn package_failure(&self)->Option<String> {
        if !self.has_package() {return None;}
        self.owner.inventories.lock().unwrap().current.check_integrity().err()
    }
    pub fn release_package_wait(&self) {self.owner.package_pending.store(false,Ordering::Release);}
    pub fn package_pending(&self)->bool {self.owner.package_pending.load(Ordering::Acquire)}
    pub fn has_package(&self)->bool {self.owner.package.load(Ordering::Acquire)}
    pub fn presented(&self) {self.owner.presented.store(true,Ordering::Release);}
    pub fn package_inventory(&self)->Option<Arc<crate::raster_storage::RetainedTiles>> {
        (self.package_pending() && self.owner.presented.load(Ordering::Acquire)).then(||self.owner.inventories.lock().unwrap().current.clone())
    }
    pub fn storage_identity(&self)->u64 {self.owner.storage_identity}
    pub fn storage(&self)->Option<crate::package::session_store::DrawingStore> {self.owner.storage.lock().unwrap().clone()}
    pub fn bind_storage(&self,storage:crate::package::session_store::DrawingStore)->Result<(),String> {
        let mut current=self.owner.storage.lock().unwrap();
        if current.as_ref().is_some_and(|current|current.path()!=storage.path()) {return Err("Drawing storage identity changed".into());}
        *current=Some(storage);Ok(())
    }
    pub(crate) fn requires_inventory_commit(&self) -> bool {
        self.preview_inventory.is_some() || self.reservation.as_ref().is_some_and(MemoryReservation::active)
    }
    pub fn take_reservation(&mut self) -> Option<MemoryReservation> { self.reservation.take() }
    pub(crate) fn reserve(&mut self, bytes: u64) -> bool {
        if bytes == 0 { return true; }
        if let Some(reservation) = &self.reservation && reservation.active() { return reservation.resize(bytes.max(reservation.reserved_bytes())); }
        let Some(reservation) = self.owner.budget.admit_import(bytes) else { return false; };
        self.reservation = Some(reservation);
        true
    }
    pub(crate) fn capture_current(&self) -> Self {
        if self._capture.is_some() { return self.clone(); }
        let mut inventories=self.owner.inventories.lock().unwrap();
        inventories.captures.retain(|capture| capture.strong_count() != 0);
        let inventory=self.preview_inventory.clone().unwrap_or_else(||inventories.current.clone());
        let capture=Arc::new(CaptureMemory {inventory,owner:self.weak(),reservation:self.reservation.clone()});
        inventories.captures.push(Arc::downgrade(&capture));
        drop(inventories);
        if self.owner.parked.load(Ordering::Relaxed) {self.owner.refresh();}
        Self {owner:self.owner.clone(),reservation:self.reservation.clone(),preview_inventory:self.preview_inventory.clone(),_capture:Some(capture)}
    }
    pub(crate) fn capture(&self, document: &crate::Document) -> Self {
        let editor=crate::Editor {document:document.clone(),undo:Vec::new(),redo:Vec::new(),checkpoint:0,next_checkpoint:1,ledger:Default::default(),memory_dirty:false};
        let capture=Arc::new(CaptureMemory {inventory:Arc::new(editor.retained_tiles()),owner:self.weak(),reservation:self.reservation.clone()});
        self.owner.inventories.lock().unwrap().captures.push(Arc::downgrade(&capture));
        self.owner.refresh();
        Self {owner:self.owner.clone(),reservation:self.reservation.clone(),preview_inventory:self.preview_inventory.clone(),_capture:Some(capture)}
    }
    pub(crate) fn cache_preview(&mut self, inventory: crate::raster_storage::RetainedTiles) {
        self.preview_inventory=Some(Arc::new(inventory));
    }
    pub(crate) fn report(&mut self, inventory: crate::raster_storage::RetainedTiles, force: bool) {
        if !self.owner.initialized.load(Ordering::Relaxed) && inventory.try_blobs().ok().flatten().is_some_and(|blobs|blobs.iter().any(|blob|blob.has_package_origin())) {self.owner.package.store(true,Ordering::Release);self.owner.package_pending.store(true,Ordering::Release);}
        self.owner.inventories.lock().unwrap().current=Arc::new(inventory);
        self.preview_inventory=None;
        let (parts,_captures)=self.owner.parts(true);
        let fields=[&self.owner.resident,&self.owner.history_resident,&self.owner.metadata,&self.owner.history_metadata];
        let changed=fields.iter().zip(parts).fold(0u64,|bytes,(field,value)|bytes.saturating_add(field.load(Ordering::Relaxed).abs_diff(value)));
        if !force && self.reservation.is_none() && changed<1024*1024 {return;}
        for (field,value) in fields.into_iter().zip(parts) {field.store(value,Ordering::Relaxed);}
        self.owner.initialized.store(true,Ordering::Relaxed);
        let [authored,history]=self.owner.reports();
        if let Some(reservation)=self.reservation.take().filter(|reservation|reservation.active()) {
            self.owner.history.publish(history);
            reservation.commit(&self.owner.memory,authored).expect("reservation and document share a budget");
        } else if force {
            self.owner.memory.report(authored);self.owner.history.report(history);
        } else {
            self.owner.memory.publish(authored);self.owner.history.publish(history);
        }
    }
    pub(crate) fn initialized(&self) -> bool { self.owner.initialized.load(Ordering::Relaxed) }
    pub(crate) fn weak(&self) -> Weak<Owner> { Arc::downgrade(&self.owner) }
}
impl Owner {
    fn parts(&self, committing: bool)->([u64;4],Vec<Arc<CaptureMemory>>) {
        let (current,captures)={
            let mut inventories=self.inventories.lock().unwrap();
            inventories.captures.retain(|capture|capture.strong_count()!=0);
            (inventories.current.clone(),inventories.captures.iter().filter_map(Weak::upgrade).collect::<Vec<_>>())
        };
        let mut resident=std::collections::HashMap::new();
        let mut metadata=[0u64;2];
        for capture in captures.iter().filter(|capture|committing || !capture.reservation.as_ref().is_some_and(MemoryReservation::active)) {
            capture.inventory.resident_owners(&mut resident);metadata[0]=metadata[0].saturating_add(capture.inventory.metadata_bytes as u64);
        }
        self.captured_resident.store(resident.iter().filter(|((kind,_),_)|*kind!=crate::raster_storage::CONTENT_TREE_METADATA).fold(0u64,|total,(_,(bytes,_))|total.saturating_add(*bytes as u64)),Ordering::Relaxed);
        current.resident_owners(&mut resident);
        metadata[0]=metadata[0].saturating_add(current.authored_metadata as u64);
        metadata[1]=current.metadata_bytes.saturating_sub(current.authored_metadata) as u64;
        let mut parts=[0,0,metadata[0],metadata[1]];
        for ((kind,_),(bytes,authored)) in &resident {
            let index=usize::from(!*authored)+if *kind==crate::raster_storage::CONTENT_TREE_METADATA {2}else{0};
            parts[index]=parts[index].saturating_add(*bytes as u64);
        }
        (parts,captures)
    }
    pub(crate) fn refresh(&self) {
        let (parts,_captures)=self.parts(false);
        for (field,value) in [&self.resident,&self.history_resident,&self.metadata,&self.history_metadata].into_iter().zip(parts) {field.store(value,Ordering::Relaxed);}
        let [authored,history]=self.reports();self.memory.publish(authored);self.history.publish(history);
    }
    fn reports(&self)->[MemoryReport;2] {
        let parked=self.parked.load(Ordering::Relaxed);
        std::array::from_fn(|index|{
            let (resident,metadata)=if index==0 {(&self.resident,&self.metadata)}else{(&self.history_resident,&self.history_metadata)};
            let bytes=metadata.load(Ordering::Relaxed).saturating_add(if parked {if index==0 {self.captured_resident.load(Ordering::Relaxed)}else{0}}else{resident.load(Ordering::Relaxed)});
            MemoryReport {ram_bytes:bytes,pinned_ram_bytes:bytes,..Default::default()}
        })
    }
    pub(crate) fn parked(&self, parked: bool) {
        if self.parked.swap(parked,Ordering::Relaxed)!=parked {
            self.refresh();
            let [authored,history]=self.reports();self.memory.report(authored);self.history.report(history);
        }
    }
}

impl crate::Document {
    pub fn capture_artwork(&self, mut context: crate::authored::EvaluationContext) -> crate::authored::ArtworkCapture {
        context.retain_effects(&self.artwork);
        let mut artwork = self.artwork.clone();
        artwork.outputs.get_mut(artwork.default_output).expect("Admitted output").context = context;
        crate::authored::ArtworkCapture { artwork: Arc::new(artwork), checkpoint: crate::authored::CaptureCheckpoint {
            owner: self.owner, document: self.artwork.id, session_generation: 0,
            artwork_generation: self.revision, working_generation: self.working.generation, edit_checkpoint: 0,
        }, memory: Some(self.memory.capture(self)) }
    }
    pub fn initialize_memory(&mut self) {
        if self.memory.initialized() {return;}
        let editor=crate::Editor {document:self.clone(),undo:Vec::new(),redo:Vec::new(),checkpoint:0,next_checkpoint:1,ledger:Default::default(),memory_dirty:false};
        self.memory.report(editor.retained_tiles(),true);
    }
    pub fn reserve_import(&mut self) -> Result<(), String> {
        let editor = crate::Editor { document: self.clone(), undo: Vec::new(), redo: Vec::new(), checkpoint: 0, next_checkpoint: 1, ledger: Default::default(), memory_dirty: false };
        let inventory = editor.retained_tiles();
        let bytes = inventory.total_bytes() as u64;
        if self.memory.reserve(bytes) { Ok(()) } else { Err(crate::MEMORY_BUDGET_EXCEEDED.into()) }
    }
}

#[cfg(test)]
#[path = "document_memory_tests.rs"]
mod tests;
