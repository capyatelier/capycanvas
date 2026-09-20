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
}
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
    fn poll(&self) -> Result<Option<Arc<[u8]>>, String>;
    fn resident_bytes(&self) -> usize;
    fn evict(&self);
}
impl From<Arc<[u8]>> for Bytes {
    fn from(bytes: Arc<[u8]>) -> Self {
        Self {
            len: bytes.len(),
            value: Mutex::new(Value::Memory(bytes)),
        }
    }
}
impl Bytes {
    fn resident_owner(&self) -> (usize, usize) {
        match &*self.value.lock().unwrap() {
            Value::Memory(bytes) => (Arc::as_ptr(bytes) as *const () as usize, bytes.len()),
            Value::External { chunk, .. } => (
                Arc::as_ptr(chunk) as *const () as usize,
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
        match &*self.value.lock().unwrap() {
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
        match &*self.value.lock().map_err(|_| "Tile storage lock failed")? {
            Value::Memory(bytes) => Ok(bytes.clone()),
            Value::External {
                chunk,
                offset,
                digest,
            } => {
                use sha2::{Digest, Sha256};
                let bytes = chunk.poll()?.ok_or("Drawing tile is still loading")?;
                let bytes = bytes
                    .get(*offset..offset + self.len)
                    .ok_or("Incomplete parked drawing chunk")?;
                if <[u8; 32]>::from(Sha256::digest(bytes)) != *digest {
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
                file.seek(SeekFrom::Start(*offset))
                    .map_err(|e| e.to_string())?;
                file.read_exact(&mut bytes)
                    .map_err(|e| format!("Cannot read parked drawing: {e}"))?;
                if <[u8; 32]>::from(Sha256::digest(&bytes)) != *digest {
                    return Err("Parked drawing tile integrity check failed".into());
                }
                Ok(bytes.into())
            }
        }
    }
    pub fn ready(&self) -> Result<bool, String> {
        match &*self.value.lock().map_err(|_| "Tile storage lock failed")? {
            Value::External { chunk, .. } => Ok(chunk.poll()?.is_some()),
            _ => Ok(true),
        }
    }
}

/// One bounded, immutable write transaction. The original tile payloads remain
/// resident until the host has committed every byte. Failed writes just drop
/// this ticket; they cannot change document/history handles.
pub struct PreparedSpill {
    pub bytes: Vec<u8>,
    records: Vec<(Arc<TileBlob>, usize, [u8; 32])>,
}
impl PreparedSpill {
    pub fn commit(self, chunk: Arc<dyn TileChunk>) -> Result<(), String> {
        if chunk.len() != self.bytes.len() {
            return Err("Incomplete parked drawing write".into());
        }
        for (blob, offset, digest) in self.records {
            let mut value = blob
                .compressed
                .value
                .lock()
                .map_err(|_| "Tile storage lock failed")?;
            if matches!(*value, Value::Memory(_)) {
                *value = Value::External {
                    chunk: chunk.clone(),
                    offset,
                    digest,
                };
            }
        }
        Ok(())
    }
}

/// Evict previous read caches, then prepare at most one chunk of remaining RAM
/// payloads. The host repeats this while the shared inactive budget is exceeded.
pub fn prepare_external_spill(tiles: &RetainedTiles) -> Result<Option<PreparedSpill>, String> {
    use sha2::{Digest, Sha256};
    let blobs = tiles
        .try_blobs()?
        .ok_or("Wait for drawing capture before parking")?;
    let mut spill = PreparedSpill {
        bytes: Vec::new(),
        records: Vec::new(),
    };
    for blob in blobs {
        let value = blob
            .compressed
            .value
            .lock()
            .map_err(|_| "Tile storage lock failed")?;
        match &*value {
            Value::External { chunk, .. } => chunk.evict(),
            Value::Memory(bytes) => {
                if !spill.bytes.is_empty() && spill.bytes.len() + bytes.len() > SPILL_CHUNK_BYTES {
                    continue;
                }
                let offset = spill.bytes.len();
                spill.bytes.extend_from_slice(bytes);
                spill.records.push((
                    blob.clone(),
                    offset,
                    <[u8; 32]>::from(Sha256::digest(bytes)),
                ));
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
    pub metadata_bytes: usize,
}
impl Editor {
    pub fn retained_tiles(&self) -> RetainedTiles {
        let mut rasters = Vec::new();
        let mut sources = Vec::new();
        for layer in &self.document.layers {
            rasters.push(&layer.raster);
            rasters.extend(layer.masks().map(|m| &m.raster));
            sources.extend(layer.source.iter());
        }
        let mut metadata_bytes = 0usize;
        if let Some(proof) = &self.document.proof {
            metadata_bytes = proof.name.len();
            if let crate::color::ColorProfile::Icc(bytes) = &proof.profile {
                metadata_bytes = metadata_bytes.saturating_add(bytes.len());
            }
        }
        for entry in self.undo.iter().chain(&self.redo) {
            entry.edit.raster_roots(&mut rasters);
            entry.edit.source_roots(&mut sources);
            metadata_bytes = metadata_bytes.saturating_add(entry.metadata_bytes);
        }
        let mut seen = HashSet::new();
        let rasters: Vec<_> = rasters
            .into_iter()
            .filter(|r| seen.insert(r.identity()))
            .cloned()
            .collect();
        for raster in &rasters {
            // Sparse maps and publication handles stay resident after payloads
            // spill. Count them even though project JSON omits raster backing.
            let tiles = raster.try_data().and_then(Result::ok).map_or_else(
                || raster.pending_bytes() / crate::raster::MAX_TILE_BYTES,
                |data| data.tiles.len(),
            );
            metadata_bytes = metadata_bytes.saturating_add(tiles.saturating_mul(192));
        }
        seen.clear();
        for source in &sources {
            if seen.insert(Arc::as_ptr(source) as usize as u64) {
                metadata_bytes =
                    metadata_bytes.saturating_add(source.tiles.len().saturating_mul(192));
                if let crate::color::ColorProfile::Icc(bytes) = &source.interpretation.profile {
                    metadata_bytes = metadata_bytes.saturating_add(bytes.len());
                }
            }
        }
        seen.clear();
        let sources = sources
            .into_iter()
            .flat_map(|s| s.tiles.values())
            .filter(|t| seen.insert(Arc::as_ptr(t) as usize as u64))
            .cloned()
            .collect();
        // Metadata excludes payloads (sources/rasters are independently stored).
        struct Counter(usize);
        impl std::io::Write for Counter {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0 = self.0.saturating_add(b.len());
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut count = Counter(0);
        let _ = serde_json::to_writer(&mut count, &self.document);
        metadata_bytes = metadata_bytes.saturating_add(count.0.saturating_mul(4));
        RetainedTiles {
            rasters,
            sources,
            metadata_bytes,
        }
    }
}
impl RetainedTiles {
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
        blobs.retain(|b| seen.insert(Arc::as_ptr(b) as usize));
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
        blobs.retain(|b| seen.insert(Arc::as_ptr(b) as usize));
        Ok(blobs)
    }
    /// Conservative nonblocking accounting; pending captures reserve their limit.
    pub fn resident_bytes(&self) -> usize {
        self.resident_bytes_with(&mut HashSet::new())
    }
    fn resident_bytes_with(&self, seen: &mut HashSet<usize>) -> usize {
        let mut bytes = 0usize;
        let mut charge = |blob: &Arc<TileBlob>| {
            let (identity, size) = blob.compressed.resident_owner();
            if seen.insert(identity) {
                bytes = bytes.saturating_add(size);
            }
        };
        for blob in &self.sources {
            charge(blob);
        }
        let mut pending = 0usize;
        for raster in &self.rasters {
            match raster.try_data() {
                Some(Ok(data)) => {
                    for tile in data.tiles.values() {
                        if let Some(Ok(blob)) = tile.try_backing() {
                            charge(&blob);
                        } else {
                            pending =
                                pending.saturating_add(crate::raster::MAX_COMPRESSED_TILE_BYTES);
                        }
                    }
                }
                None => pending = pending.saturating_add(raster.pending_bytes()),
                Some(Err(_)) => (),
            }
        }
        bytes.saturating_add(pending)
    }
}

/// Count each resident immutable allocation once across all inactive editors,
/// including a shared external read chunk containing several tile descriptors.
pub fn resident_tile_bytes<'a>(inventories: impl IntoIterator<Item = &'a RetainedTiles>) -> usize {
    let mut seen = HashSet::new();
    inventories.into_iter().fold(0usize, |n, tiles| {
        n.saturating_add(tiles.resident_bytes_with(&mut seen))
    })
}

/// Shared chunk bound for native and browser backing. Chunk ownership follows
/// the immutable tiles, including references held by undo, redo and file jobs.
pub const SPILL_CHUNK_BYTES: usize = 8 * 1024 * 1024;

/// Hosts choose an appropriate private cache directory and run this on their
/// file worker. Unlinked open files survive pathname eviction and disappear on
/// final-owner release or process exit, without a compactor or orphan scan.
#[cfg(any(unix, windows))]
pub fn spill_to_directory(
    tiles: &RetainedTiles,
    directory: &std::path::Path,
) -> Result<(), String> {
    #[cfg(unix)]
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    #[cfg(windows)]
    use std::os::windows::fs::OpenOptionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::fs::create_dir_all(directory).map_err(|e| format!("Cannot create drawing cache: {e}"))?;
    #[cfg(unix)]
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| e.to_string())?;
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
        let path = directory.join(format!(
            "{}-{}.tiles",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).read(true).write(true);
        #[cfg(unix)]
        options.mode(0o600);
        // Temporary private backing follows the final open handle on Windows.
        // It is never a user file or a durable recovery destination.
        #[cfg(windows)]
        options.custom_flags(0x0400_0100).share_mode(0x7); // DELETE_ON_CLOSE | TEMPORARY; share read/write/delete
        let file = options.open(&path)
            .map_err(|e| format!("Cannot create drawing cache: {e}"))?;
        #[cfg(unix)]
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
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
    use sha2::{Digest, Sha256};
    use std::io::Write;
    let mut records = Vec::new();
    let mut offset = 0u64;
    let mut seen = HashSet::new();
    for blob in blobs {
        if !seen.insert(Arc::as_ptr(blob) as usize) {
            continue;
        }
        let value = blob
            .compressed
            .value
            .lock()
            .map_err(|_| "Tile storage lock failed")?;
        if let Value::Memory(bytes) = &*value {
            file.write_all(bytes)
                .map_err(|e| format!("Cannot park drawing on disk: {e}"))?;
            records.push((blob, offset, <[u8; 32]>::from(Sha256::digest(bytes))));
            offset += bytes.len() as u64;
        }
    }
    file.sync_data()
        .map_err(|e| format!("Cannot flush parked drawing: {e}"))?;
    let file = Arc::new(Mutex::new(file));
    for (blob, offset, digest) in records {
        *blob
            .compressed
            .value
            .lock()
            .map_err(|_| "Tile storage lock failed")? = Value::Disk {
            file: file.clone(),
            offset,
            digest,
        };
    }
    Ok(offset as usize)
}

#[cfg(all(test, any(unix, windows)))]
mod tests {
    use super::*;
    use crate::{Document, Edit, Project, ProjectLimits, raster::*};
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
                crate::color::PixelDescriptor::SRGB8_PAINT,
                &vec![value; 256 * 256 * 4],
            )
            .unwrap(),
        )
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
        let mut editor = Editor::new(Document::new("pending redo", 256, 256));
        let target = editor.document().layers[0].id;
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
        let tile = RasterTile::pending(crate::color::PixelDescriptor::SRGB8_PAINT);
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
        let mut editor = Editor::new(Document::new("parked", 256, 256));
        let target = editor.document().layers[0].id;
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
        let project = Project {
            document: editor.document().clone(),
            assets: Default::default(),
        };
        let mut saved = Vec::new();
        project.write(&mut saved).unwrap();
        let loaded = Project::read(saved.as_slice(), ProjectLimits::default()).unwrap();
        assert_eq!(
            loaded.document.layers[0]
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
                &[blob.clone()],
                // A read-only descriptor rejects the write on every native host.
                std::fs::File::open(std::env::current_exe().unwrap()).unwrap()
            )
            .is_err()
        );
        assert_eq!(blob.resident_bytes(), before);
        blob.decode().unwrap();
        let file = file();
        let mut corrupt = file.try_clone().unwrap();
        spill_tiles(&[blob.clone()], file).unwrap();
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
        spill_tiles(&[blob.clone()], file()).unwrap();
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
