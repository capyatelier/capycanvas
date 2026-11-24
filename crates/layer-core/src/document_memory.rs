use crate::{MemoryBudget, MemoryClass, MemoryOwner, MemoryOwnerKind, MemoryReport, MemoryReservation, raster_storage::{CONTENT_TREE_METADATA, RetainedTiles}};
use std::{collections::{HashMap, hash_map::Entry}, sync::{Arc, Weak, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}}};

#[derive(Clone)]
pub struct DocumentMemory {
    pub(crate) owner: Arc<Owner>,
    pub(crate) reservation: Option<MemoryReservation>,
    preview_inventory: Option<Arc<RetainedTiles>>,
    _capture: Option<Arc<CaptureMemory>>,
}
struct CaptureMemory { inventory: Arc<RetainedTiles>, owner: Weak<Owner>, reservation: Option<MemoryReservation> }
impl Drop for CaptureMemory {
    fn drop(&mut self) {
        if let Some(owner)=self.owner.upgrade() {
            let current=owner.inventories.lock().unwrap().current.clone();
            if !Arc::ptr_eq(&self.inventory,&current) { owner.refresh(); }
        }
    }
}
#[derive(Default)]
struct Inventories { current: Arc<RetainedTiles>, captures: Vec<Weak<CaptureMemory>>, reservation: Option<MemoryReservation>, ticket: u64 }
struct Snapshot { ticket: u64, current: Arc<RetainedTiles>, captures: Vec<Arc<CaptureMemory>>, reservation: Option<MemoryReservation> }
#[derive(Clone, Copy, Default)]
struct Classification { authored: u64, history: u64, authored_metadata: u64, history_metadata: u64, captured: u64 }
#[derive(Default)]
struct Published { ticket: u64, classification: Classification, parked: bool }
impl Published {
    fn rows(&self) -> [MemoryReport; 2] {
        let Classification {authored,history,authored_metadata,history_metadata,captured}=self.classification;
        let rows=if self.parked {[authored_metadata.saturating_add(captured),history_metadata]} else {[authored_metadata.saturating_add(authored),history_metadata.saturating_add(history)]};
        rows.map(|bytes|MemoryReport {ram_bytes:bytes,pinned_ram_bytes:bytes,..Default::default()})
    }
}
pub(crate) struct Owner {
    storage_identity:u64,
    inventories: Mutex<Inventories>,
    published: Mutex<Published>,
    memory: MemoryOwner,
    history: MemoryOwner,
    budget: MemoryBudget,
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
        Self { _capture: None, owner: Arc::new(Owner {storage_identity:NEXT.fetch_add(1,Ordering::Relaxed), inventories:Default::default(), published:Default::default(), memory: budget.register_owner(MemoryOwnerKind::Authored, MemoryClass::Pinned, None),
            history: budget.register_owner(MemoryOwnerKind::History, MemoryClass::Pinned, None), budget,
            initialized: AtomicBool::new(false),package:AtomicBool::new(false),package_pending:AtomicBool::new(false),presented:AtomicBool::new(false),storage:Default::default() }), reservation, preview_inventory: None }
    }
    pub fn package_failure(&self)->Option<String> {
        if !self.has_package() {return None;}
        self.owner.inventories.lock().unwrap().current.check_stored_bytes().err()
    }
    pub fn release_package_wait(&self) {self.owner.package_pending.store(false,Ordering::Release);}
    pub fn package_pending(&self)->bool {self.owner.package_pending.load(Ordering::Acquire)}
    pub fn has_package(&self)->bool {self.owner.package.load(Ordering::Acquire)}
    pub fn presented(&self) {self.owner.presented.store(true,Ordering::Release);}
    pub fn package_inventory(&self,awaited:bool)->Option<Arc<RetainedTiles>> {
        (self.package_pending() && (awaited || self.owner.presented.load(Ordering::Acquire))).then(||self.owner.inventories.lock().unwrap().current.clone())
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
        Self {owner:self.owner.clone(),reservation:self.reservation.clone(),preview_inventory:self.preview_inventory.clone(),_capture:Some(capture)}
    }
    pub(crate) fn capture(&self, document: &crate::Document) -> Self {
        let editor=crate::Editor {document:document.clone(),undo:Vec::new(),redo:Vec::new(),checkpoint:0,next_checkpoint:1,ledger:Default::default(),memory_dirty:false,amends:Vec::new()};
        let capture=Arc::new(CaptureMemory {inventory:Arc::new(editor.retained_tiles()),owner:self.weak(),reservation:self.reservation.clone()});
        self.owner.inventories.lock().unwrap().captures.push(Arc::downgrade(&capture));
        self.owner.refresh();
        Self {owner:self.owner.clone(),reservation:self.reservation.clone(),preview_inventory:self.preview_inventory.clone(),_capture:Some(capture)}
    }
    pub(crate) fn cache_preview(&mut self, inventory: RetainedTiles) {
        self.preview_inventory=Some(Arc::new(inventory));
    }
    pub(crate) fn report(&mut self, inventory: RetainedTiles) {
        if !self.owner.initialized.load(Ordering::Relaxed) && inventory.try_blobs().ok().flatten().is_some_and(|blobs|blobs.iter().any(|blob|blob.has_package_origin())) {self.owner.package.store(true,Ordering::Release);self.owner.package_pending.store(true,Ordering::Release);}
        let reservation=self.reservation.take().filter(MemoryReservation::active);
        let (snapshot,_replaced)=self.owner.snapshot(|inventories|(std::mem::replace(&mut inventories.current,Arc::new(inventory)),
            reservation.and_then(|reservation|inventories.reservation.replace(reservation))));
        self.preview_inventory=None;
        self.owner.initialized.store(true,Ordering::Relaxed);
        self.owner.publish(snapshot,None);
    }
    pub(crate) fn initialized(&self) -> bool { self.owner.initialized.load(Ordering::Relaxed) }
    pub(crate) fn weak(&self) -> Weak<Owner> { Arc::downgrade(&self.owner) }
}
impl Owner {
    fn snapshot<T>(&self, update: impl FnOnce(&mut Inventories)->T)->(Snapshot,T) {
        let mut inventories=self.inventories.lock().unwrap();
        let updated=update(&mut inventories);
        inventories.captures.retain(|capture|capture.strong_count()!=0);
        inventories.ticket+=1;
        let snapshot=Snapshot {ticket:inventories.ticket,current:inventories.current.clone(),captures:inventories.captures.iter().filter_map(Weak::upgrade).collect(),
            reservation:inventories.reservation.as_ref().filter(|reservation|reservation.active()).cloned()};
        (snapshot,updated)
    }
    fn classify(snapshot:&Snapshot)->Classification {
        let current=&snapshot.current;
        let mut owners=HashMap::new();current.resident_owners(&mut owners);
        let mut classification=Classification {authored_metadata:current.authored_metadata as u64,history_metadata:current.metadata_bytes.saturating_sub(current.authored_metadata) as u64,..Default::default()};
        let mut inventories=vec![Arc::as_ptr(current)];let mut captured=HashMap::new();
        let committed=|reservation:&MemoryReservation|!reservation.active() || snapshot.reservation.as_ref()==Some(reservation);
        for capture in snapshot.captures.iter().filter(|capture|capture.reservation.as_ref().is_none_or(committed)) {
            if inventories.contains(&Arc::as_ptr(&capture.inventory)) {continue;}
            inventories.push(Arc::as_ptr(&capture.inventory));
            capture.inventory.resident_owners(&mut captured);
            classification.authored_metadata=classification.authored_metadata.saturating_add(capture.inventory.metadata_bytes as u64);
        }
        for (key,(bytes,authored)) in captured {
            match owners.entry(key) {
                Entry::Occupied(mut entry)=>{let (size,held)=entry.get_mut();*size=(*size).max(bytes);*held|=authored;},
                Entry::Vacant(entry)=>{if key.0!=CONTENT_TREE_METADATA {classification.captured=classification.captured.saturating_add(bytes as u64);}entry.insert((bytes,authored));},
            }
        }
        for ((kind,_),(bytes,authored)) in owners {
            let field=match (kind==CONTENT_TREE_METADATA,authored) {
                (false,true)=>&mut classification.authored,(false,false)=>&mut classification.history,
                (true,true)=>&mut classification.authored_metadata,(true,false)=>&mut classification.history_metadata,
            };
            *field=field.saturating_add(bytes as u64);
        }
        classification
    }
    fn publish(&self, snapshot: Snapshot, transition: Option<(bool,&MemoryOwner,MemoryReport)>) {
        let classification=Self::classify(&snapshot);
        let elsewhere=self.budget.transact(|ledger| {
            if let Some(reservation)=&snapshot.reservation {ledger.commit(reservation,&self.memory).expect("reservation and document share a budget");}
            let mut published=self.published.lock().unwrap();
            if snapshot.ticket>published.ticket {published.ticket=snapshot.ticket;published.classification=classification;}
            let elsewhere=transition.and_then(|(parked,tabs,report)|{published.parked=parked;ledger.report(tabs,report).err().map(|_|(tabs,report))});
            for (owner,report) in [&self.memory,&self.history].into_iter().zip(published.rows()) {ledger.report(owner,report).expect("document rows share the document budget");}
            elsewhere
        });
        if let Some((tabs,report))=elsewhere {tabs.report(report);}
    }
    pub(crate) fn refresh(&self) {
        self.publish(self.snapshot(|_|()).0,None);
    }
    pub(crate) fn park(&self, parked: bool, inventory: &RetainedTiles, tabs: &MemoryOwner, report: MemoryReport) {
        let (snapshot,_replaced)=self.snapshot(|inventories|parked.then(||std::mem::replace(&mut inventories.current,Arc::new(inventory.clone()))));
        self.publish(snapshot,Some((parked,tabs,report)));
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
        let editor=crate::Editor {document:self.clone(),undo:Vec::new(),redo:Vec::new(),checkpoint:0,next_checkpoint:1,ledger:Default::default(),memory_dirty:false,amends:Vec::new()};
        self.memory.report(editor.retained_tiles());
    }
    pub fn reserve_import(&mut self) -> Result<(), String> {
        let editor = crate::Editor { document: self.clone(), undo: Vec::new(), redo: Vec::new(), checkpoint: 0, next_checkpoint: 1, ledger: Default::default(), memory_dirty: false, amends: Vec::new() };
        let inventory = editor.retained_tiles();
        let bytes = inventory.total_bytes() as u64;
        if self.memory.reserve(bytes) { Ok(()) } else { Err(crate::MEMORY_BUDGET_EXCEEDED.into()) }
    }
}

#[cfg(test)]
#[path = "document_memory_tests.rs"]
mod tests;
