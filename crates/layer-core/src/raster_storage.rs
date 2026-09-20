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
    #[cfg(not(target_arch = "wasm32"))]
    Disk {
        file: Arc<Mutex<std::fs::File>>,
        offset: u64,
        digest: [u8; 32],
    },
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
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn resident_bytes(&self) -> usize {
        match &*self.value.lock().unwrap() {
            Value::Memory(_) => self.len,
            #[cfg(not(target_arch = "wasm32"))]
            Value::Disk { .. } => 0,
        }
    }
    pub fn read(&self) -> Result<Arc<[u8]>, String> {
        match &*self.value.lock().map_err(|_| "Tile storage lock failed")? {
            Value::Memory(bytes) => Ok(bytes.clone()),
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
            let Some(data) = raster.try_data() else { return Ok(None) };
            for tile in data?.tiles.values() {
                let Some(blob) = tile.try_backing() else { return Ok(None) };
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
        let mut seen = HashSet::new();
        let mut bytes = 0usize;
        let mut charge = |blob: &Arc<TileBlob>| {
            if seen.insert(Arc::as_ptr(blob) as usize) {
                bytes = bytes.saturating_add(blob.resident_bytes());
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
                            pending = pending.saturating_add(crate::raster::MAX_TILE_BYTES + 1024);
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

/// Shared chunk bound for native and browser backing. Chunk ownership follows
/// the immutable tiles, including references held by undo, redo and file jobs.
pub const SPILL_CHUNK_BYTES: usize = 8 * 1024 * 1024;

/// Hosts choose an appropriate private cache directory and run this on their
/// file worker. Unlinked open files survive pathname eviction and disappear on
/// final-owner release or process exit, without a compactor or orphan scan.
#[cfg(unix)]
pub fn spill_to_directory(tiles: &RetainedTiles, directory: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::fs::create_dir_all(directory).map_err(|e| format!("Cannot create drawing cache: {e}"))?;
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| e.to_string())?;
    let blobs: Vec<_> = tiles.blobs()?.into_iter().filter(|b| b.resident_bytes() > 0).collect();
    let mut start = 0;
    while start < blobs.len() {
        let mut end = start;
        let mut bytes = 0;
        while end < blobs.len() && (bytes == 0 || bytes + blobs[end].compressed_len() <= SPILL_CHUNK_BYTES) {
            bytes += blobs[end].compressed_len();
            end += 1;
        }
        let path = directory.join(format!("{}-{}.tiles", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let file = std::fs::OpenOptions::new().create_new(true).read(true).write(true).mode(0o600)
            .open(&path).map_err(|e| format!("Cannot create drawing cache: {e}"))?;
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

#[cfg(all(test, unix))]
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
        editor.perform(Edit::SetRaster { target, revision: root.clone() }).unwrap();
        editor.undo().unwrap();
        let retained = editor.retained_tiles();
        assert!(retained.try_blobs().unwrap().is_none());
        let tile = RasterTile::pending(crate::color::PixelDescriptor::SRGB8_PAINT);
        root.publish(Ok(RasterData {
            tiles: [(TileKey { plane: RasterPlane::Color, coordinate: [0, 0] }, tile.clone())].into(),
            ..Default::default()
        })).unwrap();
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
                std::fs::OpenOptions::new()
                    .write(true)
                    .open("/dev/full")
                    .unwrap()
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
