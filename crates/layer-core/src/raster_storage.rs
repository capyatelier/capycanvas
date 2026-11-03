//! Reclaimable immutable tile payloads. Hosts supply private disk files and run
//! spilling on their file worker; document/history handles keep their identity.
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
    External {
        chunk: Arc<dyn TileChunk>,
        offset: usize,
        digest: [u8; 32],
    },
    #[cfg(not(target_arch = "wasm32"))]
    Disk {
        file: Arc<Mutex<std::fs::File>>,
        offset: u64,
        digest: [u8; 32],
    },
}
/// Immutable host storage with an asynchronous read/cache boundary. Polling
/// requests a read and returns None until it completes. The final Arc owns the
/// chunk's file lifetime; no document or GPU identity is stored in the transport.
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
    pub(crate) fn integrity(&self, cancelled:&std::sync::atomic::AtomicBool) -> Result<crate::authored::EncodedIntegrity,String> {
        self.integrity.get_or_compute(cancelled,||self.read())
    }
    pub(crate) fn digest(&self,cancelled:&std::sync::atomic::AtomicBool)->Result<[u8;32],String> {
        self.integrity.digest(cancelled,||self.read())
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
            Value::External { chunk, .. } => (
                Arc::as_ptr(&chunk) as *const () as usize,
                chunk.resident_bytes(),
            ),
            #[cfg(not(target_arch = "wasm32"))]
            Value::Disk { .. } => (0, 0),
        }
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn resident_bytes(&self) -> usize {
        match self.snapshot().unwrap() {
            Value::Memory(_) => self.len,
            // Per-tile callers count this payload. Window inventory accounting
            // separately charges the whole shared read chunk exactly once.
            Value::External { chunk, .. } => {
                if chunk.resident_bytes() > 0 {
                    self.len
                } else {
                    0
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            Value::Disk { .. } => 0,
        }
    }
    pub fn read(&self) -> Result<Arc<[u8]>, String> {
        match self.snapshot()? {
            Value::Memory(bytes) => Ok(bytes),
            Value::External {
                chunk,
                offset,
                digest,
            } => {
                use sha2::{Digest, Sha256};
                let bytes = chunk.poll()?.ok_or("Drawing tile is still loading")?;
                let bytes = bytes
                    .get(offset..offset + self.len)
                    .ok_or("Incomplete parked drawing chunk")?;
                if <[u8; 32]>::from(Sha256::digest(bytes)) != digest {
                    return Err("Parked drawing tile integrity check failed".into());
                }
                Ok(bytes.into())
            }
            #[cfg(not(target_arch = "wasm32"))]
            Value::Disk {
                file,
                offset,
                digest,
            } => {
                use sha2::{Digest, Sha256};
                use std::io::{Read, Seek, SeekFrom};
                let mut bytes = vec![0; self.len];
                let mut file = file.lock().map_err(|_| "Tile file lock failed")?;
                file.seek(SeekFrom::Start(offset))
                    .map_err(|e| e.to_string())?;
                file.read_exact(&mut bytes)
                    .map_err(|e| format!("Cannot read parked drawing: {e}"))?;
                drop(file);
                if <[u8; 32]>::from(Sha256::digest(&bytes)) != digest {
                    return Err("Parked drawing tile integrity check failed".into());
                }
                Ok(bytes.into())
            }
        }
    }
    pub fn ready(&self) -> Result<bool, String> {
        match self.snapshot()? {
            Value::External { chunk, .. } => Ok(chunk.poll()?.is_some()),
            _ => Ok(true),
        }
    }
}

/// One bounded, immutable write transaction. The original tile payloads remain
/// resident until the host has committed every byte. Failed writes just drop
/// this ticket; they cannot change document/history handles.
#[derive(Default)]
pub struct PreparedSpill {
    pub bytes: Vec<u8>,
    records: Vec<SpillRecord>,
}
struct SpillRecord {
    blob: Arc<TileBlob>,
    bytes: Arc<[u8]>,
    offset: usize,
    digest: [u8; 32],
}
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
    #[cfg(not(target_arch = "wasm32"))]
    fn native(blobs: &[Arc<TileBlob>]) -> Result<Self, String> {
        let mut spill = Self::default();
        let mut seen = HashSet::new();
        for blob in blobs {
            if seen.insert(blob.owner_identity()) && let Value::Memory(bytes) = blob.compressed.snapshot()? {
                spill.retain(blob.clone(), bytes);
            }
        }
        Ok(spill)
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn write<W: std::io::Write>(&self, writer: &mut W, sync: impl FnOnce(&mut W) -> std::io::Result<()>) -> Result<usize, String> {
        let mut bytes = 0;
        for record in &self.records {
            writer.write_all(&record.bytes).map_err(|e| format!("Cannot park drawing on disk: {e}"))?;
            bytes += record.bytes.len();
        }
        sync(writer).map_err(|e| format!("Cannot flush parked drawing: {e}"))?;
        Ok(bytes)
    }
}

/// Evict previous read caches, then prepare at most one chunk of remaining RAM
/// payloads. The host repeats this while the shared inactive budget is exceeded.
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
            #[cfg(not(target_arch = "wasm32"))]
            Value::Disk { .. } => (),
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

/// Runs on a host file worker. Anonymous files survive pathname eviction and
/// disappear on final-owner release or process exit, without an orphan scan.
#[cfg(any(unix, windows))]
pub fn spill_to_directory(
    tiles: &RetainedTiles,
    directory: &std::path::Path,
) -> Result<(), String> {
    let blobs: Vec<_> = tiles
        .blobs()?
        .into_iter()
        .filter(|b| b.resident_bytes() > 0)
        .collect();
    let mut start = 0;
    while start < blobs.len() {
        let mut end = start;
        let mut bytes = 0;
        while end < blobs.len()
            && (bytes == 0 || bytes + blobs[end].compressed_len() <= SPILL_CHUNK_BYTES)
        {
            bytes += blobs[end].compressed_len();
            end += 1;
        }
        let file = crate::temp_files::anonymous(directory, "tiles")
            .map_err(|e| format!("Cannot create drawing cache: {e}"))?;
        spill_tiles(&blobs[start..end], file)?;
        start = end;
    }
    Ok(())
}

/// Atomically migrate one immutable chunk after successful write + flush.
/// A failed write never releases any original RAM payload. Existing readers
/// may finish using their Arc; all later readers use the shared disk backing.
#[cfg(not(target_arch = "wasm32"))]
pub fn spill_tiles(blobs: &[Arc<TileBlob>], mut file: std::fs::File) -> Result<usize, String> {
    let spill = PreparedSpill::native(blobs)?;
    let bytes = spill.write(&mut file, |file| file.sync_data())?;
    let file = Arc::new(Mutex::new(file));
    spill.publish(|offset, digest| Value::Disk { file: file.clone(), offset: offset as u64, digest })?;
    Ok(bytes)
}

#[cfg(all(test, any(unix, windows)))]
mod tests {
    use super::*;
    use crate::{Document, Edit, raster::*};
    use std::io::{Seek, SeekFrom, Write};
    fn file() -> std::fs::File {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "capy-tile-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let f = std::fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&p)
            .unwrap();
        std::fs::remove_file(p).unwrap();
        f
    }
    fn blob(value: u8) -> Arc<TileBlob> {
        Arc::new(
            TileBlob::encode(
                RasterPlane::Color.descriptor(Default::default()),
                &vec![value; 256 * 256 * 4],
            )
            .unwrap(),
        )
    }
    struct MemoryChunk(Arc<[u8]>);
    impl TileChunk for MemoryChunk {
        fn len(&self) -> usize { self.0.len() }
        fn poll(&self) -> Result<Option<Arc<[u8]>>, String> { Ok(Some(self.0.clone())) }
        fn resident_bytes(&self) -> usize { self.0.len() }
        fn evict(&self) {}
    }
    struct BlockingWriter {
        file: std::fs::File,
        entered: std::sync::mpsc::SyncSender<()>,
        release: std::sync::mpsc::Receiver<()>,
        block_write: bool,
    }
    impl Write for BlockingWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.block_write {
                self.block_write = false;
                self.entered.send(()).unwrap();
                self.release.recv().unwrap();
            }
            self.file.write(bytes)
        }
        fn flush(&mut self) -> std::io::Result<()> { self.file.flush() }
    }
    #[test]
    fn blocked_spill_write_and_sync_leave_tile_queries_available() {
        use std::sync::mpsc::{channel, sync_channel};
        use std::time::Duration;
        for block_write in [true, false] {
            let blob = blob(43);
            let original = blob.compressed().unwrap();
            let spill = PreparedSpill::native(std::slice::from_ref(&blob)).unwrap();
            assert!(spill.bytes.is_empty());
            assert!(Arc::ptr_eq(&spill.records[0].bytes, &original));
            let (entered, waiting) = sync_channel(0);
            let (release, released) = channel();
            let worker = std::thread::spawn(move || {
                let mut writer = BlockingWriter { file: file(), entered, release: released, block_write };
                spill.write(&mut writer, |writer| {
                    if !block_write {
                        writer.entered.send(()).unwrap();
                        writer.release.recv().unwrap();
                    }
                    writer.file.sync_data()
                }).unwrap();
                let file = Arc::new(Mutex::new(writer.file));
                spill.publish(|offset, digest| Value::Disk { file: file.clone(), offset: offset as u64, digest }).unwrap();
            });
            waiting.recv_timeout(Duration::from_secs(5)).unwrap();
            let (completed, completion) = channel();
            let reader_blob = blob.clone();
            let reader = std::thread::spawn(move || {
                let tiles = RetainedTiles { sources: vec![reader_blob.clone()], ..Default::default() };
                let result = (reader_blob.compressed_ready(), tiles.resident_bytes(), reader_blob.compressed());
                completed.send(result).unwrap();
            });
            let result = completion.recv_timeout(Duration::from_secs(5));
            release.send(()).unwrap();
            worker.join().unwrap();
            reader.join().unwrap();
            let (ready, resident, bytes) = result.expect("Tile queries waited for spill I/O");
            assert!(ready.unwrap());
            assert_eq!(resident, original.len());
            assert!(Arc::ptr_eq(&bytes.unwrap(), &original));
            assert_eq!(blob.decode().unwrap(), vec![43; 256 * 256 * 4]);
        }
    }
    #[test]
    fn competing_native_and_external_spills_preserve_first_publication() {
        for external_first in [true, false] {
            let blob = blob(47);
            let original = blob.compressed().unwrap();
            let retained = RetainedTiles { sources: vec![blob.clone()], ..Default::default() };
            let native = PreparedSpill::native(std::slice::from_ref(&blob)).unwrap();
            let external = prepare_external_spill(&retained).unwrap().unwrap();
            let chunk: Arc<dyn TileChunk> = Arc::new(MemoryChunk(external.bytes.clone().into()));
            let mut backing = file();
            native.write(&mut backing, |file| file.sync_data()).unwrap();
            let backing = Arc::new(Mutex::new(backing));
            let mut external = Some(external);
            if external_first { external.take().unwrap().commit(chunk.clone()).unwrap(); }
            native.publish(|offset, digest| Value::Disk { file: backing.clone(), offset: offset as u64, digest }).unwrap();
            if !external_first { external.take().unwrap().commit(chunk.clone()).unwrap(); }
            match blob.compressed.snapshot().unwrap() {
                Value::External { chunk: winner, .. } if external_first => assert!(Arc::ptr_eq(&winner, &chunk)),
                Value::Disk { file: winner, .. } if !external_first => assert!(Arc::ptr_eq(&winner, &backing)),
                _ => panic!("A later spill replaced the published backing"),
            }
            assert_eq!(blob.compressed().unwrap(), original);
            assert_eq!(blob.decode().unwrap(), vec![47; 256 * 256 * 4]);
        }
    }
    #[test]
    fn failed_spill_write_and_sync_keep_exact_memory_owner() {
        struct FailedWrite;
        impl Write for FailedWrite {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> { Err(std::io::Error::other("write failed")) }
            fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
        }
        let blob = blob(51);
        let original = blob.compressed().unwrap();
        let spill = PreparedSpill::native(std::slice::from_ref(&blob)).unwrap();
        assert!(spill.write(&mut FailedWrite, |_| panic!("Failed writes must not sync")).unwrap_err().contains("write failed"));
        drop(spill);
        assert!(Arc::ptr_eq(&blob.compressed().unwrap(), &original));
        let spill = PreparedSpill::native(std::slice::from_ref(&blob)).unwrap();
        assert!(spill.write(&mut Vec::new(), |_| Err(std::io::Error::other("sync failed"))).unwrap_err().contains("sync failed"));
        drop(spill);
        assert!(Arc::ptr_eq(&blob.compressed().unwrap(), &original));
        assert_eq!(blob.decode().unwrap(), vec![51; 256 * 256 * 4]);
    }
    #[test]
    fn external_chunk_callbacks_run_outside_tile_lock() {
        struct Chunk { blob: std::sync::Weak<TileBlob>, bytes: Arc<[u8]> }
        impl Chunk {
            fn unlocked(&self) { assert!(self.blob.upgrade().unwrap().compressed.value.try_lock().is_ok()); }
        }
        impl TileChunk for Chunk {
            fn len(&self) -> usize { self.unlocked(); self.bytes.len() }
            fn poll(&self) -> Result<Option<Arc<[u8]>>, String> { self.unlocked(); Ok(Some(self.bytes.clone())) }
            fn resident_bytes(&self) -> usize { self.unlocked(); self.bytes.len() }
            fn evict(&self) { self.unlocked(); }
        }
        let blob = blob(59);
        let retained = RetainedTiles { sources: vec![blob.clone()], ..Default::default() };
        let spill = prepare_external_spill(&retained).unwrap().unwrap();
        let chunk = Arc::new(Chunk { blob: Arc::downgrade(&blob), bytes: spill.bytes.clone().into() });
        spill.commit(chunk).unwrap();
        assert!(blob.compressed_ready().unwrap());
        assert_eq!(blob.resident_bytes(), blob.compressed_len());
        assert_eq!(retained.resident_bytes(), blob.compressed_len());
        assert_eq!(blob.decode().unwrap(), vec![59; 256 * 256 * 4]);
        assert!(prepare_external_spill(&retained).unwrap().is_none());
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
    fn external_chunks_publish_only_after_commit_and_keep_exact_identity() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        struct Chunk {
            bytes: Arc<[u8]>,
            ready: AtomicBool,
            drops: Arc<AtomicUsize>,
        }
        impl TileChunk for Chunk {
            fn len(&self) -> usize {
                self.bytes.len()
            }
            fn poll(&self) -> Result<Option<Arc<[u8]>>, String> {
                Ok(self
                    .ready
                    .load(Ordering::Relaxed)
                    .then(|| self.bytes.clone()))
            }
            fn resident_bytes(&self) -> usize {
                if self.ready.load(Ordering::Relaxed) {
                    self.bytes.len()
                } else {
                    0
                }
            }
            fn evict(&self) {
                self.ready.store(false, Ordering::Relaxed);
            }
        }
        impl Drop for Chunk {
            fn drop(&mut self) {
                self.drops.fetch_add(1, Ordering::Relaxed);
            }
        }
        let first = blob(42);
        let retained = RetainedTiles {
            sources: vec![first.clone()],
            ..Default::default()
        };
        let original = first.compressed().unwrap();
        assert_eq!(resident_tile_bytes([&retained, &retained]), original.len());
        let failed = prepare_external_spill(&retained).unwrap().unwrap();
        drop(failed); // failed/cancelled host write
        assert!(first.resident_bytes() > 0);
        assert_eq!(first.compressed().unwrap(), original);
        let spill = prepare_external_spill(&retained).unwrap().unwrap();
        let drops = Arc::new(AtomicUsize::new(0));
        let chunk = Arc::new(Chunk {
            bytes: spill.bytes.clone().into(),
            ready: AtomicBool::new(false),
            drops: drops.clone(),
        });
        spill.commit(chunk.clone()).unwrap();
        assert_eq!(first.resident_bytes(), 0);
        assert!(!first.compressed_ready().unwrap());
        assert!(
            first.compressed().is_err(),
            "pending read is recoverable, never a panic or empty tile"
        );
        chunk.ready.store(true, Ordering::Relaxed);
        assert_eq!(resident_tile_bytes([&retained, &retained]), original.len());
        assert!(first.compressed_ready().unwrap());
        assert_eq!(first.compressed().unwrap(), original);
        assert_eq!(first.decode().unwrap(), vec![42; 256 * 256 * 4]);
        assert!(
            prepare_external_spill(&retained).unwrap().is_none(),
            "reread caches evict without rewriting immutable bytes"
        );
        drop(chunk);
        drop(retained);
        assert_eq!(
            drops.load(Ordering::Relaxed),
            0,
            "the exact tile still owns its file"
        );
        drop(first);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
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
    fn spill_shared_history_save_and_restore_are_exact() {
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
        assert_eq!(spill_tiles(&blobs, file()).unwrap(), expected);
        assert_eq!(retained.resident_bytes(), 0);
        assert_eq!(first.decode().unwrap(), vec![30; 256 * 256 * 4]);
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
                .decode()
                .unwrap(),
            second.decode().unwrap()
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
    fn failed_spill_keeps_original_and_corrupt_disk_is_reported() {
        let blob = blob(55);
        let before = blob.resident_bytes();
        assert!(
            spill_tiles(
                std::slice::from_ref(&blob),
                // A read-only descriptor rejects the write on every native host.
                std::fs::File::open(std::env::current_exe().unwrap()).unwrap()
            )
            .is_err()
        );
        assert_eq!(blob.resident_bytes(), before);
        blob.decode().unwrap();
        let file = file();
        let mut corrupt = file.try_clone().unwrap();
        spill_tiles(std::slice::from_ref(&blob), file).unwrap();
        corrupt.seek(SeekFrom::Start(0)).unwrap();
        corrupt.write_all(b"bad").unwrap();
        assert!(blob.compressed().unwrap_err().contains("integrity"));
    }
    #[test]
    fn float_samples_survive_disk_without_precision_changes() {
        let descriptor = crate::color::DocumentColor {
            depth: crate::color::SampleDepth::F32,
            ..Default::default()
        }
        .paint_descriptor();
        let samples: Vec<_> = [-0.125f32, 20., 0.12345679, 1.]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .cycle()
            .take(256 * 256 * 16)
            .collect();
        let blob = Arc::new(TileBlob::encode(descriptor, &samples).unwrap());
        spill_tiles(std::slice::from_ref(&blob), file()).unwrap();
        assert_eq!(blob.decode().unwrap(), samples);
    }
    #[test]
    fn temporary_directory_backing_dies_with_the_final_owner() {
        let path=std::env::temp_dir().join(format!("capy-private-backing-{}",std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        let blob=blob(77);
        let tiles=RetainedTiles { sources:vec![blob.clone()], ..Default::default() };
        spill_to_directory(&tiles,&path).unwrap();assert_eq!(tiles.resident_bytes(),0);
        assert_eq!(blob.decode().unwrap(),vec![77;256*256*4]);
        drop(tiles);assert_eq!(blob.decode().unwrap(),vec![77;256*256*4]);drop(blob);
        assert_eq!(std::fs::read_dir(&path).unwrap().count(),0);
        std::fs::remove_dir(&path).unwrap();
    }
    #[test]
    fn final_tile_owner_releases_the_disk_chunk() {
        let first = blob(12);
        let second = blob(24);
        spill_tiles(&[first.clone(), second.clone()], file()).unwrap();
        let weak = match &*first.compressed.value.lock().unwrap() {
            Value::Disk { file, .. } => Arc::downgrade(file),
            _ => panic!("tile must be on disk"),
        };
        drop(first);
        assert!(weak.upgrade().is_some(), "other tiles still own this chunk");
        assert_eq!(second.decode().unwrap(), vec![24; 256 * 256 * 4]);
        drop(second);
        assert!(
            weak.upgrade().is_none(),
            "no stale spill file survives its final tile"
        );
    }
}
