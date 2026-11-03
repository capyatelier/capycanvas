//! Immutable tile memory and leased session-store locations.
use crate::{
    Editor,
    raster::{RasterRevision, TileBlob},
};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

pub(crate) struct Bytes {
    len: usize,
    value: Mutex<Value>,
    integrity: crate::authored::IntegrityCache,
}
#[derive(Clone)]
enum Value {
    Memory(Arc<[u8]>),
    #[cfg(target_arch = "wasm32")]
    External {
        chunk: Arc<dyn TileChunk>,
        offset: usize,
        digest: [u8; 32],
    },
    #[cfg(not(target_arch = "wasm32"))]
    Store(Arc<crate::package::session_store::StoreLocation>),
}
/// Immutable host storage with an asynchronous read/cache boundary. Polling
/// requests a read and returns None until it completes. The final Arc owns the
/// chunk's file lifetime; no document or GPU identity is stored in the transport.
#[cfg(target_arch = "wasm32")]
pub trait TileChunk: Send + Sync {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool { self.len() == 0 }
    fn poll(&self) -> Result<Option<Arc<[u8]>>, String>;
    fn resident_bytes(&self) -> usize;
    fn evict(&self);
}
impl From<Arc<[u8]>> for Bytes {
    fn from(bytes: Arc<[u8]>) -> Self {
        Self {
            len: bytes.len(),
            value: Mutex::new(Value::Memory(bytes)),
            integrity: crate::authored::IntegrityCache::default(),
        }
    }
}
impl Bytes {
    #[cfg(not(target_arch="wasm32"))]
    pub(crate) fn stored(location:Arc<crate::package::session_store::StoreLocation>)->Self {
        let integrity=crate::authored::IntegrityCache::default();
        integrity.seed(crate::authored::EncodedIntegrity {bytes:location.length,crc32:location.crc}).unwrap();
        Self {len:location.length as usize,value:Mutex::new(Value::Store(location)),integrity}
    }
    pub(crate) fn integrity(&self, cancelled:&std::sync::atomic::AtomicBool) -> Result<crate::authored::EncodedIntegrity,String> {
        self.integrity.get_or_compute(cancelled,||self.read_on_worker().map(crate::package::ByteRange::into_arc))
    }
    pub(crate) fn digest(&self,cancelled:&std::sync::atomic::AtomicBool)->Result<[u8;32],String> {
        self.integrity.digest(cancelled,||self.read_on_worker().map(crate::package::ByteRange::into_arc))
    }
    pub(crate) fn set_integrity(&self,value:crate::authored::EncodedIntegrity)->Result<(),String> {
        if value.bytes!=self.len as u64 {return Err("Immutable tile integrity length mismatch".into());}
        self.integrity.seed(value)
    }
    fn snapshot(&self) -> Result<Value, String> {
        Ok(self.value.lock().map_err(|_| "Tile storage lock failed")?.clone())
    }
    fn resident_owner(&self) -> (usize, usize) {
        match self.snapshot().unwrap() {
            Value::Memory(bytes) => (Arc::as_ptr(&bytes) as *const () as usize, bytes.len()),
            #[cfg(target_arch = "wasm32")]
            Value::External { chunk, .. } => (Arc::as_ptr(&chunk) as *const () as usize, chunk.resident_bytes()),
            #[cfg(not(target_arch = "wasm32"))]
            Value::Store(_) => (0, 0),
        }
    }
    pub fn len(&self) -> usize { self.len }
    pub fn resident_bytes(&self) -> usize {
        match self.snapshot().unwrap() {
            Value::Memory(_) => self.len,
            #[cfg(target_arch = "wasm32")]
            Value::External { chunk, .. } => if chunk.resident_bytes()>0 {self.len} else {0},
            #[cfg(not(target_arch = "wasm32"))]
            Value::Store(_) => 0,
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn memory(&self) -> Result<Option<Arc<[u8]>>,String> {
        Ok(match self.snapshot()? {Value::Memory(bytes)=>Some(bytes),_=>None})
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn location(&self)->Result<Option<Arc<crate::package::session_store::StoreLocation>>,String> {
        Ok(match self.snapshot()? {Value::Store(location)=>Some(location),_=>None})
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn bind(&self, bytes:&Arc<[u8]>, location:Arc<crate::package::session_store::StoreLocation>) -> Result<bool,String> {
        let mut value=self.value.lock().map_err(|_|"Tile storage lock failed")?;
        if !matches!(&*value,Value::Memory(current) if Arc::ptr_eq(bytes,current)) {return Ok(false);}
        *value=Value::Store(location);Ok(true)
    }
    pub(crate) fn poll(&self)->Result<crate::package::RangeState,String> {
        use crate::package::{ByteRange,RangeState};
        match self.snapshot()? {
            Value::Memory(bytes)=>Ok(RangeState::Ready(ByteRange::new(bytes,0..self.len)?)),
            #[cfg(not(target_arch = "wasm32"))]
            Value::Store(location)=>location.poll(),
            #[cfg(target_arch = "wasm32")]
            Value::External {chunk,offset,..}=>match chunk.poll()? {
                None=>Ok(RangeState::Pending),
                Some(bytes)=>Ok(RangeState::Ready(ByteRange::new(bytes,offset..offset.checked_add(self.len).ok_or("Drawing tile range overflow")?)?)),
            },
        }
    }
    pub(crate) fn poll_on_worker(&self)->Result<crate::package::RangeState,String> {
        let state=self.poll()?;
        #[cfg(target_arch = "wasm32")]
        if let (crate::package::RangeState::Ready(bytes),Value::External {digest,..})=(&state,self.snapshot()?) {
            use sha2::{Digest,Sha256};
            if <[u8;32]>::from(Sha256::digest(&**bytes))!=digest {return Err("Parked drawing tile integrity check failed".into());}
        }
        Ok(state)
    }
    pub(crate) fn read_on_worker(&self)->Result<crate::package::ByteRange,String> {
        #[cfg(not(target_arch = "wasm32"))]
        return match self.snapshot()? {
            Value::Memory(bytes)=>Ok(crate::package::ByteRange::new(bytes,0..self.len)?),
            Value::Store(location)=>location.read_on_worker(),
        };
        #[cfg(target_arch = "wasm32")]
        match self.poll_on_worker()? {
            crate::package::RangeState::Ready(bytes)=>Ok(bytes),
            crate::package::RangeState::Pending=>Err("Drawing tile is still loading".into()),
        }
    }

}

/// One bounded, immutable write transaction. The original tile payloads remain
/// resident until the host has committed every byte. Failed writes just drop
/// this ticket; they cannot change document/history handles.
#[cfg(target_arch = "wasm32")]
#[derive(Default)]
pub struct PreparedSpill {
    pub bytes: Vec<u8>,
    records: Vec<SpillRecord>,
}
#[cfg(target_arch = "wasm32")]
struct SpillRecord {
    blob: Arc<TileBlob>,
    bytes: Arc<[u8]>,
    offset: usize,
    digest: [u8; 32],
}
#[cfg(target_arch = "wasm32")]
impl PreparedSpill {
    fn retain(&mut self, blob: Arc<TileBlob>, bytes: Arc<[u8]>) {
        use sha2::{Digest, Sha256};
        let offset = self.records.last().map_or(0, |r| r.offset + r.bytes.len());
        let digest = Sha256::digest(&bytes).into();
        self.records.push(SpillRecord { blob, bytes, offset, digest });
    }
    fn publish(self, backing: impl Fn(usize, [u8; 32]) -> Value) -> Result<(), String> {
        for record in self.records {
            let mut value = record.blob.compressed.value.lock().map_err(|_| "Tile storage lock failed")?;
            if matches!(&*value, Value::Memory(bytes) if Arc::ptr_eq(bytes, &record.bytes)) {
                *value = backing(record.offset, record.digest);
            }
        }
        Ok(())
    }
    pub fn commit(self, chunk: Arc<dyn TileChunk>) -> Result<(), String> {
        if chunk.len() != self.bytes.len() {
            return Err("Incomplete parked drawing write".into());
        }
        self.publish(|offset, digest| Value::External { chunk: chunk.clone(), offset, digest })
    }

}

/// Evict previous read caches, then prepare at most one chunk of remaining RAM
/// payloads. The host repeats this while the shared inactive budget is exceeded.
#[cfg(target_arch = "wasm32")]
pub fn prepare_external_spill(tiles: &RetainedTiles) -> Result<Option<PreparedSpill>, String> {
    let blobs = tiles.try_blobs()?.ok_or("Wait for drawing capture before parking")?;
    let mut spill = PreparedSpill::default();
    for blob in blobs {
        match blob.compressed.snapshot()? {
            Value::External { chunk, .. } => chunk.evict(),
            Value::Memory(bytes) => {
                if !spill.bytes.is_empty() && spill.bytes.len() + bytes.len() > SPILL_CHUNK_BYTES {
                    continue;
                }
                spill.bytes.extend_from_slice(&bytes);
                spill.retain(blob, bytes);
            }
        }
    }
    Ok((!spill.records.is_empty()).then_some(spill))
}

/// Tile roots include both undo directions. Cloning this inventory never copies
/// pixels and is safe before queued raster capture has finished.
#[derive(Default, Clone)]
pub struct RetainedTiles {
    pub(crate) rasters: Vec<RasterRevision>,
    pub(crate) sources: Vec<Arc<TileBlob>>,
    pub(crate) backings: Vec<crate::package::ImmutableBacking>,
    pub metadata_bytes: usize,
    pub(crate) owner: Option<std::sync::Weak<crate::document_memory::Owner>>,
    pub(crate) authored_rasters: HashSet<u64>,
    pub(crate) authored_sources: HashSet<u64>,
    pub(crate) authored_backings: HashSet<u64>,
    pub(crate) authored_metadata: usize,
}
impl Editor {
    pub fn retained_images(&self) -> Vec<crate::Image> {
        let mut roots=crate::RootInventory::default();roots.document(&self.document);
        for entry in self.undo.iter().chain(&self.redo) {entry.edit.roots(&mut roots);}
        let states: Vec<_> = roots.rasters.iter().filter_map(|r| r.liquify()).collect();
        for state in &states { roots.paint(&state.original); }
        let mut images=std::collections::BTreeMap::new();
        for image in roots.images {images.entry(image.id()).or_insert_with(||image.clone());}
        images.into_values().collect()
    }
    pub fn discard_liquify_maps(&self) {
        let mut roots=crate::RootInventory::default();roots.document(&self.document);
        for entry in self.undo.iter().chain(&self.redo) {entry.edit.roots(&mut roots);}
        for raster in roots.rasters { raster.drop_liquify(); }
    }
    pub fn retained_tiles(&self) -> RetainedTiles { self.retained_tiles_for(&self.document, false) }
    pub(crate) fn retained_tiles_for(&self, document: &crate::Document, include_current: bool) -> RetainedTiles {
        let mut roots=crate::RootInventory::default();
        roots.document(document);
        if include_current { roots.document(&self.document); }
        let mut metadata_bytes=crate::artwork_metadata(&document.artwork).saturating_add(document.working.selection_overlays.metadata_bytes());
        if include_current { metadata_bytes=metadata_bytes.saturating_add(crate::artwork_metadata(&self.document.artwork)); }
        let current_states: Vec<_> = roots.rasters.iter().filter_map(|r| r.liquify()).collect();
        for state in &current_states { roots.paint(&state.original); }
        let authored_rasters: HashSet<_> = roots.rasters.iter().map(|r| r.identity()).collect();
        let authored_sources: HashSet<_> = roots.sources.iter().flat_map(|source|source.tiles.values().map(|tile|tile.owner_identity())).collect();
        let authored_backings: HashSet<_> = roots.extensions.iter().flat_map(|extensions|extensions.resources.values().map(|resource|resource.backing.identity())).collect();
        let source_count=roots.sources.len();let image_count=roots.images.len();let resource_count=roots.resources.len();
        let selection_count=roots.selections.len();let mesh_count=roots.meshes.len();let profile_count=roots.profiles.len();let program_count=roots.programs.len();
        let mut authored_metadata=metadata_bytes;
        for entry in self.undo.iter().chain(&self.redo) {entry.edit.roots(&mut roots);metadata_bytes=metadata_bytes.saturating_add(entry.metadata_bytes);}
        let states: Vec<_> = roots.rasters.iter().filter_map(|r| r.liquify()).collect();
        for state in &states { roots.paint(&state.original); }
        let mut seen=HashSet::new();
        let rasters:Vec<_>=roots.rasters.into_iter().filter(|r|seen.insert(r.identity())).cloned().collect();
        for raster in &rasters {
            let tiles=raster.try_data().and_then(Result::ok).map_or_else(||raster.pending_bytes()/crate::raster::MAX_TILE_BYTES,|data|data.tiles.len());
            let bytes=tiles.saturating_mul(192);metadata_bytes=metadata_bytes.saturating_add(bytes);
            if authored_rasters.contains(&raster.identity()) {authored_metadata=authored_metadata.saturating_add(bytes);}
        }
        seen.clear();
        for (index,image) in roots.images.into_iter().enumerate() {
            if seen.insert(image.owner_identity() as u64) {let bytes=image.owner_metadata_bytes();metadata_bytes=metadata_bytes.saturating_add(bytes);if index<image_count {authored_metadata=authored_metadata.saturating_add(bytes);}}
        }
        seen.clear();
        let mut profiles=HashSet::new();
        for (index,source) in roots.sources.iter().enumerate() {
            if seen.insert(Arc::as_ptr(source) as usize as u64) {
                let bytes=source.tiles.len().saturating_mul(192);metadata_bytes=metadata_bytes.saturating_add(bytes);
                if index<source_count {authored_metadata=authored_metadata.saturating_add(bytes);}
                if let crate::color::ColorProfile::Icc(bytes)=&source.interpretation.profile && profiles.insert(bytes.as_ptr() as usize) {
                    metadata_bytes=metadata_bytes.saturating_add(bytes.len());
                    if index<source_count {authored_metadata=authored_metadata.saturating_add(bytes.len());}
                }
            }
        }
        seen.clear();
        let sources=roots.sources.into_iter().flat_map(|s|s.tiles.values()).filter(|t|seen.insert(t.owner_identity())).cloned().collect();
        let mut accounting=crate::history_budget::Accounting::default();
        accounting.charge_extension_metadata(&self.document.artwork.extensions);
        let mut backing_ids=HashSet::new();let mut backings=Vec::new();
        for extensions in roots.extensions {
            metadata_bytes=metadata_bytes.saturating_add(accounting.charge_extension_metadata(extensions));
            for resource in extensions.resources.values() {
                if backing_ids.insert(resource.backing.identity()){backings.push(resource.backing.clone());}
            }
        }
        for (index,resource) in roots.resources.into_iter().enumerate(){let bytes=accounting.charge_resource(resource);metadata_bytes=metadata_bytes.saturating_add(bytes);if index<resource_count {authored_metadata=authored_metadata.saturating_add(bytes);}}
        for (index,selection) in roots.selections.into_iter().enumerate(){let bytes=accounting.charge_selection(selection);metadata_bytes=metadata_bytes.saturating_add(bytes);if index<selection_count {authored_metadata=authored_metadata.saturating_add(bytes);}}
        for (index,mesh) in roots.meshes.into_iter().enumerate(){let bytes=accounting.charge_mesh(mesh);metadata_bytes=metadata_bytes.saturating_add(bytes);if index<mesh_count {authored_metadata=authored_metadata.saturating_add(bytes);}}
        for (index,profile) in roots.profiles.into_iter().enumerate(){if let crate::color::ColorProfile::Icc(bytes)=profile && profiles.insert(bytes.as_ptr() as usize){metadata_bytes=metadata_bytes.saturating_add(bytes.len());if index<profile_count {authored_metadata=authored_metadata.saturating_add(bytes.len());}}}
        for (index,program) in roots.programs.into_iter().enumerate(){let bytes=accounting.charge_program(program);metadata_bytes=metadata_bytes.saturating_add(bytes);if index<program_count {authored_metadata=authored_metadata.saturating_add(bytes);}}
        for block in document.artwork.metadata.blocks().into_iter().flatten(){metadata_bytes=metadata_bytes.saturating_add(block.len());authored_metadata=authored_metadata.saturating_add(block.len());}
        RetainedTiles{rasters,sources,backings,metadata_bytes,owner:Some(self.document.memory.weak()),authored_rasters,authored_sources,authored_backings,authored_metadata}
    }
}
impl RetainedTiles {
    pub fn set_parked(&self, parked: bool) {
        if let Some(owner) = self.owner.as_ref().and_then(std::sync::Weak::upgrade) { owner.parked(parked); }
    }
    /// Poll every current/undo/redo root without blocking the host event loop.
    /// Hosts await completion before normal parking; pending readbacks may need
    /// that same event loop to publish their immutable backing.
    pub fn try_blobs(&self) -> Result<Option<Vec<Arc<TileBlob>>>, String> {
        let mut blobs = self.sources.clone();
        for raster in &self.rasters {
            let Some(data) = raster.try_data() else {
                return Ok(None);
            };
            for tile in data?.tiles.values() {
                let Some(blob) = tile.try_backing() else {
                    return Ok(None);
                };
                blobs.push(blob?);
            }
        }
        let mut seen = HashSet::new();
        blobs.retain(|b| seen.insert(b.owner_identity()));
        Ok(Some(blobs))
    }

    /// Run on a worker: pending immutable captures can require a GPU fence.
    pub fn blobs(&self) -> Result<Vec<Arc<TileBlob>>, String> {
        let mut blobs = self.sources.clone();
        for raster in &self.rasters {
            for tile in raster.wait_data()?.tiles.values() {
                blobs.push(tile.wait_backing()?);
            }
        }
        let mut seen = HashSet::new();
        blobs.retain(|b| seen.insert(b.owner_identity()));
        Ok(blobs)
    }
    /// Conservative nonblocking accounting; pending captures reserve their limit.
    pub fn resident_bytes(&self) -> usize {
        let mut owners=std::collections::HashMap::new();self.resident_owners(&mut owners);
        owners.values().fold(0usize,|bytes,(size,_)|bytes.saturating_add(*size))
    }
    pub(crate) fn resident_owners(&self, owners:&mut std::collections::HashMap<(u8,u64),(usize,bool)>) {
        fn charge(owners:&mut std::collections::HashMap<(u8,u64),(usize,bool)>,key:(u8,u64),size:usize,current:bool) {
            owners.entry(key).and_modify(|(bytes,authored)|{*bytes=(*bytes).max(size);*authored|=current;}).or_insert((size,current));
        }
        let blob=|owners:&mut std::collections::HashMap<(u8,u64),(usize,bool)>,blob:&Arc<TileBlob>,current|{
            let (identity,size)=blob.compressed.resident_owner();charge(owners,(0,identity as u64),size,current);
        };
        for source in &self.sources {blob(owners,source,self.authored_sources.contains(&source.owner_identity()));}
        for raster in &self.rasters {
            let current=self.authored_rasters.contains(&raster.identity());
            match raster.try_data() {
                Some(Ok(data))=>{let mut pending=0usize;for tile in data.tiles.values() {
                    if let Some(Ok(backing))=tile.try_backing() {blob(owners,&backing,current);}
                    else {pending=pending.saturating_add(crate::raster::MAX_COMPRESSED_TILE_BYTES);}
                }charge(owners,(3,raster.identity()),pending,current);},
                None=>charge(owners,(4,raster.identity()),raster.pending_bytes(),current),Some(Err(_))=>(),
            }
            if let Some(state)=raster.liquify() {state.field.resources(&mut |id,size|charge(owners,(2,id),size,current));}
        }
        for backing in &self.backings {charge(owners,(1,backing.identity()),backing.resident_bytes(),self.authored_backings.contains(&backing.identity()));}
    }
}

/// Count each resident immutable allocation once across all inactive editors,
/// including a shared external read chunk containing several tile descriptors.
pub fn resident_tile_bytes<'a>(inventories: impl IntoIterator<Item = &'a RetainedTiles>) -> usize {
    let mut owners=std::collections::HashMap::new();
    for tiles in inventories {tiles.resident_owners(&mut owners);}
    owners.values().fold(0usize,|bytes,(size,_)|bytes.saturating_add(*size))
}

/// Shared chunk bound for native and browser backing. Chunk ownership follows
/// the immutable tiles, including references held by undo, redo and file jobs.
pub const SPILL_CHUNK_BYTES: usize = 8 * 1024 * 1024;

#[cfg(all(test, any(unix, windows)))]
pub(crate) mod tests {
    use super::*;
    use crate::{Document, Edit, raster::*};
    fn blob(value: u8) -> Arc<TileBlob> {
        Arc::new(TileBlob::encode(RasterPlane::Color.descriptor(Default::default()), &vec![value;256*256*4]).unwrap())
    }
    pub(crate) struct Store {
        pub(crate) path: std::path::PathBuf,
        pub(crate) store: crate::package::session_store::SessionStore,
    }
    impl Store {
        pub(crate) fn new(tiles: &RetainedTiles) -> Self {
            let path = std::env::temp_dir().join(format!("capy-tile-store-{}", crate::PortableId::random()));
            let mut store = crate::package::session_store::SessionStore::open(&path).unwrap();
            store.make_durable(tiles, &std::sync::atomic::AtomicBool::new(false)).unwrap();
            Self { path, store }
        }
        pub(crate) fn range(blob: &TileBlob) -> crate::package::ByteRange {
            match blob.compressed.snapshot().unwrap() {
                Value::Store(location) => location.read_on_worker().unwrap(),
                _ => panic!("tile must be durable"),
            }
        }
        pub(crate) fn location(blob: &TileBlob) -> std::sync::Weak<crate::package::session_store::StoreLocation> {
            match blob.compressed.snapshot().unwrap() {
                Value::Store(location) => Arc::downgrade(&location),
                _ => panic!("tile must be durable"),
            }
        }
        pub(crate) fn pack(&self) -> std::path::PathBuf {
            std::fs::read_dir(self.path.join("packs")).unwrap().next().unwrap().unwrap().path()
        }
    }
    impl Drop for Store {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.path); }
    }
    #[test]
    fn ancillary_resources_retain_and_charge_the_shared_package_owner_once() {
        use crate::{authored::{Extensions, OpaqueResource, PortableId, RecordChange}, package::{ByteSource, ImmutableBacking, RangeState}};
        struct Memory {bytes:Arc<[u8]>}
        impl ByteSource for Memory {
            fn byte_len(&self)->u64 {self.bytes.len() as u64}
            fn poll(&self,_:u64,_:usize)->Result<RangeState,String> {panic!("Accounting must not read package bytes")}
        }
        let bytes:Arc<[u8]>=vec![31;32*1024].into();let weak=Arc::downgrade(&bytes);
        let backing=ImmutableBacking::new(Arc::new(Memory{bytes})).unwrap();let owner=backing.identity();
        let mut document=crate::operation_test_support::document([32,32], &["Ink"]);
        let baseline=Editor::new(document.clone()).retained_tiles().metadata_bytes;
        let mut extensions=Extensions::default();
        for offset in [0,8] {
            let id=PortableId::random();
            extensions.records.insert(PortableId::random(),serde_json::json!({"type":"future.note/1","ancillary":true,"copy_safe":true,"data":{"payload":{"ref":id}}}));
            extensions.resources.insert(id,Arc::new(OpaqueResource{id,kind:Arc::from("future.attachment/1"),data:serde_json::json!({}),encoding:Arc::from("raw"),extra_fields:Default::default(),backing:backing.clone(),offset,length:8,crc32:0}));
        }
        let ids:Vec<_>=extensions.resources.keys().copied().collect();
        document.artwork.extensions=Arc::new(extensions);drop(backing);
        let mut editor=Editor::new(document);
        let first=editor.capture(7,Default::default()).unwrap();
        let handle=editor.document().working.occurrence.unwrap();
        let mut occurrence=editor.document().artwork.occurrences.get(handle).unwrap().clone();occurrence.name=Arc::from("Renamed");
        let change=RecordChange::replace(&editor.document().artwork.occurrences,handle,Some(occurrence)).unwrap();editor.perform(Edit::Occurrence(change)).unwrap();
        let second=editor.capture(7,Default::default()).unwrap();
        assert!(Arc::ptr_eq(&first.artwork.extensions,&second.artwork.extensions));
        for capture in [&first,&second] {
            assert_eq!(capture.artwork.extensions.resources.keys().copied().collect::<Vec<_>>(),ids);
            assert!(capture.artwork.extensions.resources.values().all(|resource|resource.backing.identity()==owner));
        }
        let retained=editor.retained_tiles();
        let other=Editor::new(Document::from_artwork((*first.artwork).clone()).unwrap());let shared=other.retained_tiles();
        assert_eq!(retained.backings.len(),1);assert_eq!(shared.backings.len(),1);
        assert_eq!(retained.resident_bytes(),32*1024);assert_eq!(resident_tile_bytes([&retained,&shared]),32*1024);
        assert!(retained.metadata_bytes>baseline+crate::extension_metadata(&first.artwork.extensions));
        let mut accounting=crate::history_budget::Accounting::default();
        assert_eq!(accounting.charge_extensions(&first.artwork.extensions),32*1024+crate::extension_metadata(&first.artwork.extensions));
        assert_eq!(accounting.charge_extensions(&second.artwork.extensions),0);
        let copied=Arc::new((*first.artwork.extensions).clone());
        assert_eq!(accounting.charge_extensions(&copied),crate::extension_record_metadata(&copied));
        assert_eq!(crate::history_budget::Accounting::new(editor.document()).charge_extensions(&copied),crate::extension_record_metadata(&copied));
        drop(copied);drop(first);drop(second);drop(editor);drop(other);drop(retained);
        assert!(weak.upgrade().is_some(),"The final retained inventory owns the package independently");
        drop(shared);assert!(weak.upgrade().is_none());
    }
    #[test]
    fn durable_ranges_keep_exact_identity_and_input_leases() {
        let first = blob(42);
        let original = first.compressed_on_worker().map(|range| range.into_arc()).unwrap();
        let identity = first.owner_identity();
        let retained = RetainedTiles { sources: vec![first.clone()], ..Default::default() };
        assert_eq!(resident_tile_bytes([&retained, &retained]), original.len());
        let mut store = Store::new(&retained);
        assert_eq!(first.owner_identity(), identity);
        assert_eq!(retained.resident_bytes(), 0);
        let location = Store::location(&first);
        let range = Store::range(&first);
        assert_eq!(&range[..], original.as_ref());
        assert_eq!(first.decode_on_worker().unwrap(), vec![42; 256 * 256 * 4]);
        assert_eq!(retained.resident_bytes(), 0);
        drop(retained); drop(first);
        store.store.collect_garbage().unwrap();
        assert!(location.upgrade().is_none());
        assert_eq!(&range[..], original.as_ref());
    }
    fn revision(blob: Arc<TileBlob>) -> RasterRevision {
        RasterRevision::backed(RasterData {
            tiles: [(
                TileKey {
                    plane: RasterPlane::Color,
                    coordinate: [0, 0],
                },
                RasterTile::backed_shared(blob),
            )]
            .into(),
            ..Default::default()
        })
    }
    #[test]
    fn nonblocking_parking_waits_for_redo_only_captures_and_reports_failure() {
        let mut editor = Editor::new(Document::new(crate::PortableId::random(), 256, 256, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }));
        let target = editor.document().working.target.unwrap();
        let root = RasterRevision::pending();
        editor
            .perform(Edit::SetRaster {
                target,
                revision: root.clone(),
            })
            .unwrap();
        editor.undo().unwrap();
        let retained = editor.retained_tiles();
        assert!(retained.try_blobs().unwrap().is_none());
        let tile = RasterTile::pending(RasterPlane::Color.descriptor(Default::default()));
        root.publish(Ok(RasterData {
            tiles: [(
                TileKey {
                    plane: RasterPlane::Color,
                    coordinate: [0, 0],
                },
                tile.clone(),
            )]
            .into(),
            ..Default::default()
        }))
        .unwrap();
        assert!(retained.try_blobs().unwrap().is_none());
        tile.publish(Err("Capture worker stopped".into())).unwrap();
        assert_eq!(retained.try_blobs().unwrap_err(), "Capture worker stopped");
    }
    #[test]
    fn store_backed_history_save_and_restore_are_exact() {
        let mut editor = Editor::new(Document::new(crate::PortableId::random(), 256, 256, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }));
        let target = editor.document().working.target.unwrap();
        let first = blob(30);
        let second = blob(80);
        editor
            .perform(Edit::SetRaster {
                target,
                revision: revision(first.clone()),
            })
            .unwrap();
        editor
            .perform(Edit::SetRaster {
                target,
                revision: revision(second.clone()),
            })
            .unwrap();
        editor.undo().unwrap();
        let retained = editor.retained_tiles();
        let blobs = retained.blobs().unwrap();
        assert_eq!(blobs.len(), 2, "include redo and deduplicate shared roots");
        let expected = retained.resident_bytes();
        let mut store = Store::new(&retained);
        assert_eq!(store.store.make_durable(&retained, &std::sync::atomic::AtomicBool::new(false)).unwrap(), 0);
        assert!(expected > 0);
        assert_eq!(retained.resident_bytes(), 0);
        assert_eq!(first.decode_on_worker().unwrap(), vec![30; 256 * 256 * 4]);
        editor.redo().unwrap();
        let cancelled=std::sync::atomic::AtomicBool::new(false);
        let capture=editor.capture(0,crate::EvaluationContext::default()).unwrap();
        let prepared=crate::package::codec::PreparedPackage::prepare(&capture,None,&cancelled).unwrap();
        let mut saved=Vec::new();prepared.write(&mut saved,&cancelled).unwrap();
        let loaded=crate::package::codec::open(crate::package::ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(saved))).unwrap(),crate::ProjectLimits::default(),&cancelled).unwrap();
        let crate::package::codec::OpenOutcome::Candidate{artwork,..}=loaded else{panic!("editable package")};
        assert_eq!(
            artwork.paint.iter().next().unwrap().2
                .raster
                .wait_data()
                .unwrap()
                .tiles
                .values()
                .next()
                .unwrap()
                .wait_backing()
                .unwrap()
                .decode_on_worker()
                .unwrap(),
            second.decode_on_worker().unwrap()
        );
        editor.undo().unwrap();
        editor.redo().unwrap();
        assert_eq!(
            retained.resident_bytes(),
            0,
            "reads must not permanently rehydrate RAM"
        );
    }
    #[test]
    fn corrupt_store_range_is_reported() {
        use std::io::{Seek, SeekFrom, Write};
        let blob = blob(55);
        let retained = RetainedTiles { sources: vec![blob.clone()], ..Default::default() };
        let store = Store::new(&retained);
        let mut corrupt = std::fs::OpenOptions::new().write(true).open(store.pack()).unwrap();
        corrupt.seek(SeekFrom::Start(0)).unwrap();
        corrupt.write_all(b"bad").unwrap();
        assert!(blob.decode_on_worker().unwrap_err().contains("checksum"));
    }
    #[test]
    fn float_samples_survive_store_without_precision_changes() {
        let descriptor = crate::color::DocumentColor { depth: crate::color::SampleDepth::F32, ..Default::default() }.paint_descriptor();
        let samples: Vec<_> = [-0.125f32, 20., 0.12345679, 1.].into_iter().flat_map(f32::to_le_bytes).cycle().take(256 * 256 * 16).collect();
        let blob = Arc::new(TileBlob::encode(descriptor, &samples).unwrap());
        let _store = Store::new(&RetainedTiles { sources: vec![blob.clone()], ..Default::default() });
        assert_eq!(blob.decode_on_worker().unwrap(), samples);
    }
    #[test]
    fn final_tile_owner_releases_its_store_location() {
        let first = blob(12);
        let second = Arc::new(first.alias(crate::PortableId::random()));
        let mut store = Store::new(&RetainedTiles { sources: vec![first.clone(), second.clone()], ..Default::default() });
        let weak = Store::location(&first);
        store.store.retire().unwrap();
        drop(first);
        assert!(weak.upgrade().is_some());
        assert_eq!(second.decode_on_worker().unwrap(), vec![12; 256 * 256 * 4]);
        drop(second);
        store.store.collect_garbage().unwrap();
        assert!(weak.upgrade().is_none());
        drop(store);
    }
}
