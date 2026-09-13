//! Immutable sparse raster revisions shared by editing, history and file workers.
//! Pending GPU capture is explicit; only workers may wait for host backing.
use crate::color::PixelDescriptor;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

pub const TILE_SIZE: u32 = 256;
pub const MAX_TILE_BYTES: usize = (TILE_SIZE * TILE_SIZE * 4) as usize;
pub const MAX_CAPTURE_BYTES: u64 = 256 * 1024 * 1024;
#[cfg(not(target_arch = "wasm32"))]
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(30);
static NEXT_PUBLICATION: AtomicU64 = AtomicU64::new(1);

/// Awaitable single publication, with errors preserved for every consumer.
/// Dropping a save never cancels a capture also owned by document history.
#[derive(Debug)]
struct Publication<T> {
    id: u64,
    value: Mutex<Option<Result<Arc<T>, String>>>,
    ready: Condvar,
}
impl<T> Default for Publication<T> {
    fn default() -> Self {
        Self {
            id: NEXT_PUBLICATION.fetch_add(1, Ordering::Relaxed),
            value: Mutex::new(None),
            ready: Condvar::new(),
        }
    }
}
impl<T> Publication<T> {
    fn publish(&self, value: Result<Arc<T>, String>) -> Result<(), String> {
        let mut state = self.value.lock().map_err(|_| "Raster publication failed")?;
        if state.is_some() {
            return Err("Raster revision was already published".into());
        }
        *state = Some(value);
        self.ready.notify_all();
        Ok(())
    }
    fn get(&self) -> Option<Result<Arc<T>, String>> {
        self.value.lock().ok()?.clone()
    }
    #[cfg(target_arch = "wasm32")]
    fn wait(&self) -> Result<Arc<T>, String> {
        // Blocking would prevent WebGPU's map callbacks from publishing.
        self.get().ok_or("Raster capture is still pending")?
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn wait(&self) -> Result<Arc<T>, String> {
        if let Some(value) = self.get() {
            return value;
        }
        let state = self.value.lock().map_err(|_| "Raster publication failed")?;
        let (state, _) = self
            .ready
            .wait_timeout_while(state, CAPTURE_TIMEOUT, |v| v.is_none())
            .map_err(|_| "Raster publication failed")?;
        state.clone().ok_or("Raster capture did not complete")?
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RasterPlane {
    Color,
    Mask,
    Wetness,
    WatercolorWetness,
}
impl RasterPlane {
    pub fn descriptor(self) -> PixelDescriptor {
        if self == Self::Color {
            PixelDescriptor::SRGB8_PAINT
        } else {
            PixelDescriptor::COVERAGE8
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TileKey {
    pub plane: RasterPlane,
    pub coordinate: [u32; 2],
}

/// Independently compressed, content-addressed exact samples. Immutable backing
/// can be written repeatedly without readback, conversion or recompression.
pub struct TileBlob {
    pub digest: [u8; 32],
    pub descriptor: PixelDescriptor,
    compressed: Arc<[u8]>,
}
impl std::fmt::Debug for TileBlob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TileBlob")
            .field("digest", &self.digest)
            .field("descriptor", &self.descriptor)
            .field("compressed_bytes", &self.compressed.len())
            .finish()
    }
}
impl TileBlob {
    pub fn encode(descriptor: PixelDescriptor, bytes: &[u8]) -> Result<Self, String> {
        let expected = descriptor
            .byte_len([TILE_SIZE; 2])
            .ok_or("Unsupported raster pixels")?;
        if bytes.len() != expected {
            return Err("Invalid raster tile byte count".into());
        }
        Ok(Self {
            digest: Self::digest(descriptor, bytes),
            descriptor,
            compressed: zstd::bulk::compress(bytes, -20)
                .map_err(|e| e.to_string())?
                .into(),
        })
    }
    fn digest(descriptor: PixelDescriptor, bytes: &[u8]) -> [u8; 32] {
        let mut hash = Sha256::new();
        // Representation participates in identity; equal bytes need not mean
        // equal samples. The descriptor vocabulary is intentionally restricted.
        hash.update(serde_json::to_vec(&descriptor).expect("fixed descriptor"));
        hash.update(bytes);
        hash.finalize().into()
    }
    pub fn compressed(&self) -> &[u8] {
        &self.compressed
    }
    pub fn compressed_owned(&self) -> Arc<[u8]> {
        self.compressed.clone()
    }
    pub fn resident_bytes(&self) -> usize {
        self.compressed.len()
    }
    pub fn decode(&self) -> Result<Vec<u8>, String> {
        let size = self
            .descriptor
            .byte_len([TILE_SIZE; 2])
            .ok_or("Unsupported raster pixels")?;
        let frame_size = zstd::zstd_safe::find_frame_compressed_size(&self.compressed)
            .map_err(|_| "Invalid compressed raster frame")?;
        if frame_size != self.compressed.len() {
            return Err("Trailing compressed raster data".into());
        }
        let bytes = zstd::bulk::decompress(&self.compressed, size).map_err(|e| e.to_string())?;
        if bytes.len() != size || Self::digest(self.descriptor, &bytes) != self.digest {
            return Err("Raster tile integrity check failed".into());
        }
        Ok(bytes)
    }
    pub fn from_compressed(
        descriptor: PixelDescriptor,
        digest: [u8; 32],
        bytes: Arc<[u8]>,
    ) -> Result<Self, String> {
        if bytes.len() > MAX_TILE_BYTES + 1024 {
            return Err("Oversized compressed raster tile".into());
        }
        let result = Self {
            descriptor,
            digest,
            compressed: bytes,
        };
        result.decode()?;
        Ok(result)
    }

    /// Transfer from this application's browser codec worker, which already
    /// encoded or validated the blob. This avoids repeating decompression on the
    /// input owner. Untrusted project files must use `from_compressed` instead.
    #[cfg(target_arch = "wasm32")]
    pub fn from_verified_worker(
        descriptor: PixelDescriptor,
        digest: [u8; 32],
        bytes: Arc<[u8]>,
    ) -> Result<Self, String> {
        if bytes.is_empty()
            || bytes.len() > MAX_TILE_BYTES + 1024
            || descriptor.byte_len([TILE_SIZE; 2]).is_none()
        {
            return Err("Invalid raster worker blob".into());
        }
        Ok(Self {
            descriptor,
            digest,
            compressed: bytes,
        })
    }
}

/// A dirty tile's queue-ordered capture. Its CPU data is published once by the
/// readback/compression worker. Cloning a revision only clones these handles.
#[derive(Clone, Debug, Default)]
pub struct RasterTile(Arc<Publication<TileBlob>>);
impl RasterTile {
    pub fn identity(&self) -> u64 {
        self.0.id
    }
    pub fn backed(blob: TileBlob) -> Self {
        let tile = Self::default();
        tile.publish(Ok(blob)).expect("new tile");
        tile
    }
    pub fn publish(&self, value: Result<TileBlob, String>) -> Result<(), String> {
        self.0.publish(value.map(Arc::new))
    }
    pub fn try_backing(&self) -> Option<Result<Arc<TileBlob>, String>> {
        self.0.get()
    }
    pub fn wait_backing(&self) -> Result<Arc<TileBlob>, String> {
        self.0.wait()
    }
    pub fn same_capture(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Watercolor's live edge is part of composition, and its wetness affects the
/// next brush. Both belong to committed raster state. Per-contact reservoirs
/// and accumulation coverage are transient and end at the stroke boundary.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RasterWatercolor {
    pub wet_edge: f32,
    pub burnt_edge: f32,
    pub edge_width: f32,
}

#[derive(Clone, Debug, Default)]
pub struct RasterData {
    pub tiles: BTreeMap<TileKey, RasterTile>,
    pub watercolor: Option<RasterWatercolor>,
}
impl RasterData {
    pub fn host_backed(&self) -> bool {
        self.tiles
            .values()
            .all(|tile| matches!(tile.try_backing(), Some(Ok(_))))
    }
    pub fn resident_bytes(&self) -> usize {
        self.tiles
            .values()
            .filter_map(|tile| tile.try_backing()?.ok())
            .map(|blob| blob.resident_bytes())
            .sum()
    }
    pub fn validate(&self, extent: [u32; 2], mask: bool) -> Result<(), String> {
        self.validate_index(extent, mask)?;
        for (key, tile) in &self.tiles {
            if tile.wait_backing()?.descriptor != key.plane.descriptor() {
                return Err("Raster plane has the wrong pixel representation".into());
            }
        }
        Ok(())
    }
    /// Validate topology without awaiting unrelated tile captures. Restoration
    /// checks each replacement's representation when it decodes that tile.
    pub fn validate_index(&self, extent: [u32; 2], mask: bool) -> Result<(), String> {
        if let Some(w) = self.watercolor
            && (mask
                || ![w.wet_edge, w.burnt_edge, w.edge_width]
                    .into_iter()
                    .all(f32::is_finite)
                || !(0.0..=1.0).contains(&w.wet_edge)
                || !(0.0..=1.0).contains(&w.burnt_edge)
                || !(1.0..=16.0).contains(&w.edge_width))
        {
            return Err("Invalid raster watercolor state".into());
        }
        for key in self.tiles.keys() {
            if key.coordinate[0] >= extent[0].div_ceil(TILE_SIZE)
                || key.coordinate[1] >= extent[1].div_ceil(TILE_SIZE)
                || mask != (key.plane == RasterPlane::Mask)
            {
                return Err("Invalid raster tile coordinates or plane".into());
            }
        }
        Ok(())
    }
}

/// A revision is a stable identity even before its GPU work is submitted. The
/// renderer publishes its sparse index, then the worker publishes tile backing.
/// Durability is deliberately absent: only atomic file publication grants it.
#[derive(Clone, Debug)]
pub struct RasterRevision(Arc<Publication<RasterData>>);
impl Default for RasterRevision {
    fn default() -> Self {
        Self::backed(RasterData::default())
    }
}
impl PartialEq for RasterRevision {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
            || match (self.try_data(), other.try_data()) {
                (Some(Ok(a)), Some(Ok(b))) => {
                    a.tiles.is_empty() && b.tiles.is_empty() && a.watercolor == b.watercolor
                }
                _ => false,
            }
    }
}
impl RasterRevision {
    pub fn identity(&self) -> u64 {
        self.0.id
    }
    pub fn is_empty(&self) -> bool {
        matches!(self.try_data(), Some(Ok(data)) if data.tiles.is_empty() && data.watercolor.is_none())
    }
    pub fn pending() -> Self {
        Self(Arc::default())
    }
    pub fn backed(data: RasterData) -> Self {
        let revision = Self::pending();
        revision.publish(Ok(data)).expect("new revision");
        revision
    }
    pub fn publish(&self, value: Result<RasterData, String>) -> Result<(), String> {
        self.0.publish(value.map(Arc::new))
    }
    pub fn try_data(&self) -> Option<Result<Arc<RasterData>, String>> {
        self.0.get()
    }
    pub fn wait_data(&self) -> Result<Arc<RasterData>, String> {
        self.0.wait()
    }
    pub fn host_backed(&self) -> bool {
        matches!(self.try_data(), Some(Ok(data)) if data.host_backed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_backing_reuses_unchanged_tiles_and_preserves_snapshot() {
        let bytes: Vec<_> = (0..MAX_TILE_BYTES).map(|i| (i % 251) as u8).collect();
        let tile =
            RasterTile::backed(TileBlob::encode(PixelDescriptor::SRGB8_PAINT, &bytes).unwrap());
        let key = TileKey {
            plane: RasterPlane::Color,
            coordinate: [0, 0],
        };
        let original = RasterData {
            tiles: BTreeMap::from([(key, tile.clone())]),
            watercolor: None,
        };
        let revision = RasterRevision::backed(original.clone());
        let mut next = original;
        let changed = TileKey {
            coordinate: [1, 0],
            ..key
        };
        next.tiles.insert(changed, RasterTile::default());
        assert!(next.tiles[&key].same_capture(&revision.wait_data().unwrap().tiles[&key]));
        assert!(!next.host_backed());
        assert!(revision.host_backed());
        assert_eq!(tile.wait_backing().unwrap().decode().unwrap(), bytes);
        assert_eq!(revision.wait_data().unwrap().tiles.len(), 1);
    }
    #[test]
    fn capture_failure_is_shared_and_publication_is_single_assignment() {
        let tile = RasterTile::default();
        let snapshot = tile.clone();
        tile.publish(Err("Device lost before capture".into()))
            .unwrap();
        assert_eq!(
            snapshot.wait_backing().unwrap_err(),
            "Device lost before capture"
        );
        assert!(tile.publish(Err("overwrite".into())).is_err());
        let revision = RasterRevision::pending();
        assert!(!revision.host_backed());
        revision
            .publish(Err("cancelled before submission".into()))
            .unwrap();
        assert!(revision.wait_data().is_err());
    }
    #[test]
    fn independently_compressed_tiles_reject_corruption_and_expansion() {
        let blob = TileBlob::encode(PixelDescriptor::COVERAGE8, &vec![17; 65536]).unwrap();
        assert_eq!(blob.decode().unwrap(), vec![17; 65536]);
        let mut compressed = blob.compressed().to_vec();
        compressed[4] ^= 1;
        assert!(
            TileBlob::from_compressed(blob.descriptor, blob.digest, compressed.into()).is_err()
        );
        assert!(
            TileBlob::from_compressed(
                PixelDescriptor::SRGB8_PAINT,
                blob.digest,
                blob.compressed.clone()
            )
            .is_err()
        );
        let mut tail = blob.compressed().to_vec();
        tail.push(0);
        assert!(TileBlob::from_compressed(blob.descriptor, blob.digest, tail.into()).is_err());
    }
}
