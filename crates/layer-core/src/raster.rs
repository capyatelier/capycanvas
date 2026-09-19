//! Immutable sparse raster revisions shared by editing, history and file workers.
//! Pending GPU capture is explicit; only workers may wait for host backing.
use crate::color::PixelDescriptor;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
mod decoded_cache;
pub use decoded_cache::{DecodedTileCache, DecodedTileCacheStats};
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
pub const MAX_TILE_BYTES: usize = (TILE_SIZE * TILE_SIZE * 16) as usize;
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
    pub fn descriptor(self, color: crate::color::DocumentColor) -> PixelDescriptor {
        if self == Self::Color {
            color.paint_descriptor()
        } else {
            color.coverage_descriptor()
        }
    }
    /// Native paint is straight profile RGB. The original sRGB attachment
    /// representation remains explicit for hosts awaiting native integration;
    /// it must never be inferred from the document color/depth alone.
    pub fn accepts_descriptor(self, color: crate::color::DocumentColor, descriptor: PixelDescriptor) -> bool {
        descriptor == self.descriptor(color)
            || (self == Self::Color
                && color == crate::color::DocumentColor::default()
                && descriptor == PixelDescriptor::SRGB8_PAINT)
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
    pub(crate) compressed: crate::raster_storage::Bytes,
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
fn shuffle_samples(descriptor: PixelDescriptor, bytes: &[u8]) -> Vec<u8> {
    let bpp = descriptor
        .bytes_per_pixel()
        .expect("validated multibyte descriptor");
    let pixels = bytes.len() / bpp;
    let mut result = vec![0; bytes.len()];
    for (pixel, source) in bytes.chunks_exact(bpp).enumerate() {
        for (channel, byte) in source.iter().enumerate() {
            result[channel * pixels + pixel] = *byte;
        }
    }
    result
}
fn unshuffle_samples(descriptor: PixelDescriptor, bytes: &[u8]) -> Vec<u8> {
    let bpp = descriptor
        .bytes_per_pixel()
        .expect("validated multibyte descriptor");
    let pixels = bytes.len() / bpp;
    let mut result = vec![0; bytes.len()];
    for (pixel, destination) in result.chunks_exact_mut(bpp).enumerate() {
        for (channel, byte) in destination.iter_mut().enumerate() {
            *byte = bytes[channel * pixels + pixel];
        }
    }
    result
}

impl TileBlob {
    pub fn encode(descriptor: PixelDescriptor, bytes: &[u8]) -> Result<Self, String> {
        Self::encode_at_level(descriptor, bytes, -20)
    }
    /// Immutable imported samples are compressed on the file worker. Unlike
    /// interactive capture, favor source residency over minimum commit latency.
    pub fn encode_source(descriptor: PixelDescriptor, bytes: &[u8]) -> Result<Self, String> {
        Self::encode_at_level(descriptor, bytes, 1)
    }
    fn encode_at_level(
        descriptor: PixelDescriptor,
        bytes: &[u8],
        level: i32,
    ) -> Result<Self, String> {
        let expected = descriptor
            .byte_len([TILE_SIZE; 2])
            .ok_or("Unsupported raster pixels")?;
        if bytes.len() != expected {
            return Err("Invalid raster tile byte count".into());
        }
        descriptor.validate_samples(bytes)?;
        // Multibyte source channels benefit from byte planes: smooth high
        // bytes no longer alternate with noisy low bytes. This is a reversible
        // permutation, not a precision change; the digest covers original bytes.
        let shuffled = (descriptor.bits_per_channel > 8).then(|| shuffle_samples(descriptor, bytes));
        Ok(Self {
            digest: Self::digest(descriptor, bytes),
            descriptor,
            compressed: Arc::<[u8]>::from(zstd::bulk::compress(shuffled.as_deref().unwrap_or(bytes), level)
                .map_err(|e| e.to_string())?).into(),
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
    pub fn compressed_len(&self) -> usize { self.compressed.len() }
    pub fn compressed(&self) -> Result<Arc<[u8]>, String> { self.compressed.read() }
    #[cfg(target_arch = "wasm32")]
    pub fn compressed_owned(&self) -> Arc<[u8]> {
        self.compressed().expect("browser tiles have immutable memory backing")
    }
    pub fn resident_bytes(&self) -> usize { self.compressed.resident_bytes() }
    pub fn decode(&self) -> Result<Vec<u8>, String> {
        let size = self
            .descriptor
            .byte_len([TILE_SIZE; 2])
            .ok_or("Unsupported raster pixels")?;
        let compressed = self.compressed()?;
        let frame_size = zstd::zstd_safe::find_frame_compressed_size(&compressed)
            .map_err(|_| "Invalid compressed raster frame")?;
        if frame_size != self.compressed.len() {
            return Err("Trailing compressed raster data".into());
        }
        let mut bytes =
            zstd::bulk::decompress(&compressed, size).map_err(|e| e.to_string())?;
        if bytes.len() == size && self.descriptor.bits_per_channel > 8 {
            bytes = unshuffle_samples(self.descriptor, &bytes);
        }
        if bytes.len() != size || Self::digest(self.descriptor, &bytes) != self.digest {
            return Err("Raster tile integrity check failed".into());
        }
        self.descriptor.validate_samples(&bytes)?;
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
            compressed: bytes.into(),
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
            compressed: bytes.into(),
        })
    }
}

/// A dirty tile's queue-ordered capture. Its CPU data is published once by the
/// readback/compression worker. Cloning a revision only clones these handles.
#[derive(Debug)]
struct TilePublication {
    data: Publication<TileBlob>,
    descriptor: PixelDescriptor,
}
#[derive(Clone, Debug)]
pub struct RasterTile(Arc<TilePublication>);
impl RasterTile {
    /// The native layout is known before readback completes. History can charge
    /// the correct pending payload even while document precision is changing.
    pub fn pending(descriptor: PixelDescriptor) -> Self {
        Self(Arc::new(TilePublication {
            data: Publication::default(),
            descriptor,
        }))
    }
    pub fn descriptor(&self) -> PixelDescriptor {
        self.0.descriptor
    }
    pub fn identity(&self) -> u64 {
        self.0.data.id
    }
    pub fn backed(blob: TileBlob) -> Self {
        Self::backed_shared(Arc::new(blob))
    }
    pub fn backed_shared(blob: Arc<TileBlob>) -> Self {
        let tile = Self::pending(blob.descriptor);
        tile.0.data.publish(Ok(blob)).expect("new tile");
        tile
    }
    pub fn publish(&self, value: Result<TileBlob, String>) -> Result<(), String> {
        if value
            .as_ref()
            .is_ok_and(|blob| blob.descriptor != self.0.descriptor)
        {
            let error = "Raster capture changed its declared pixel representation".to_string();
            self.0.data.publish(Err(error.clone()))?;
            return Err(error);
        }
        self.0.data.publish(value.map(Arc::new))
    }
    pub fn try_backing(&self) -> Option<Result<Arc<TileBlob>, String>> {
        self.0.data.get()
    }
    pub fn wait_backing(&self) -> Result<Arc<TileBlob>, String> {
        self.0.data.wait()
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
    pub fn validate(
        &self,
        extent: [u32; 2],
        mask: bool,
        color: crate::color::DocumentColor,
    ) -> Result<(), String> {
        self.validate_index(extent, mask, color)?;
        for (key, tile) in &self.tiles {
            if !key.plane.accepts_descriptor(color, tile.wait_backing()?.descriptor) {
                return Err("Raster plane has the wrong pixel representation".into());
            }
        }
        Ok(())
    }
    /// Validate topology without awaiting unrelated tile captures. Restoration
    /// checks each replacement's representation when it decodes that tile.
    pub fn validate_index(
        &self,
        extent: [u32; 2],
        mask: bool,
        color: crate::color::DocumentColor,
    ) -> Result<(), String> {
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
        for (key, tile) in &self.tiles {
            if key.coordinate[0] >= extent[0].div_ceil(TILE_SIZE)
                || key.coordinate[1] >= extent[1].div_ceil(TILE_SIZE)
                || mask != (key.plane == RasterPlane::Mask)
            {
                return Err("Invalid raster tile coordinates or plane".into());
            }
            if !key.plane.accepts_descriptor(color, tile.descriptor()) {
                return Err("Raster plane has the wrong pixel representation".into());
            }
        }
        Ok(())
    }
}

/// A revision is a stable identity even before its GPU work is submitted. The
/// renderer publishes its sparse index, then the worker publishes tile backing.
/// Durability is deliberately absent: only atomic file publication grants it.
#[derive(Clone, Debug)]
pub struct RasterRevision(Arc<RasterPublication>);
#[derive(Debug)]
struct RasterPublication {
    data: Publication<RasterData>,
    pending_bytes: AtomicU64,
}
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
    /// Pending work may still succeed. Only an explicit producer failure makes
    /// a revision unusable for restoration or history navigation.
    pub fn failed(&self) -> bool {
        match self.try_data() {
            Some(Err(_)) => true,
            Some(Ok(data)) => data
                .tiles
                .values()
                .any(|t| matches!(t.try_backing(), Some(Err(_)))),
            None => false,
        }
    }
    pub fn identity(&self) -> u64 {
        self.0.data.id
    }
    pub fn is_empty(&self) -> bool {
        matches!(self.try_data(), Some(Ok(data)) if data.tiles.is_empty() && data.watercolor.is_none())
    }
    pub fn pending() -> Self {
        Self(Arc::new(RasterPublication {
            data: Publication::default(),
            pending_bytes: AtomicU64::new(MAX_CAPTURE_BYTES),
        }))
    }
    /// A producer admitting a larger native publication must account its
    /// retained output before allocating it. This is not an allocation target;
    /// published tile identities replace the reservation in history accounting.
    pub fn reserve_pending_bytes(&self, bytes: u64) {
        self.0.pending_bytes.fetch_max(bytes, Ordering::Relaxed);
    }
    pub(crate) fn pending_bytes(&self) -> usize {
        usize::try_from(self.0.pending_bytes.load(Ordering::Relaxed)).unwrap_or(usize::MAX)
    }
    pub fn backed(data: RasterData) -> Self {
        let revision = Self::pending();
        revision.publish(Ok(data)).expect("new revision");
        revision
    }
    pub fn publish(&self, value: Result<RasterData, String>) -> Result<(), String> {
        self.0.data.publish(value.map(Arc::new))
    }
    pub fn try_data(&self) -> Option<Result<Arc<RasterData>, String>> {
        self.0.data.get()
    }
    pub fn wait_data(&self) -> Result<Arc<RasterData>, String> {
        self.0.data.wait()
    }
    pub fn host_backed(&self) -> bool {
        matches!(self.try_data(), Some(Ok(data)) if data.host_backed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_history_charges_each_retained_tiles_own_precision() {
        use crate::color::{DocumentColor, SampleDepth, RgbSpace};
        use crate::{Document, Edit, Editor, LayerId};
        for (bits, expected_undo) in [(8, 4), (16, 2), (32, 1)] {
            // Even while the current document is still sRGB8, old revision
            // tickets own their layout. No pixel allocation/readback is needed
            // to enforce the 512 MiB history ceiling.
            let mut editor = Editor::new(Document::new("pending history", 6400, 5120));
            let descriptor = DocumentColor {
                space: RgbSpace::ProPhoto,
                depth: match bits { 8 => SampleDepth::U8, 16 => SampleDepth::U16, _ => SampleDepth::F32 },
            }.paint_descriptor();
            for _ in 0..3 {
                let data = RasterData {
                    tiles: (0..500)
                        .map(|i| {
                            (
                                TileKey {
                                    plane: RasterPlane::Color,
                                    coordinate: [i % 25, i / 25],
                                },
                                RasterTile::pending(descriptor),
                            )
                        })
                        .collect(),
                    watercolor: None,
                };
                editor
                    .perform(Edit::SetRaster {
                        target: LayerId(1),
                        revision: RasterRevision::backed(data),
                    })
                    .unwrap();
            }
            editor
                .perform(Edit::SetRaster {
                    target: LayerId(1),
                    revision: RasterRevision::backed(RasterData::default()),
                })
                .unwrap();
            let mut restored = 0;
            while editor.undo().unwrap() {
                restored += 1;
            }
            assert_eq!(restored, expected_undo, "{bits}-bit ticket accounting");
        }
    }
    #[test]
    fn pending_native_layout_is_stable_and_rejects_mismatched_publication() {
        use crate::color::{DocumentColor, SampleDepth, RgbSpace};
        let color = DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: SampleDepth::U16,
        };
        let descriptor = RasterPlane::Color.descriptor(color);
        let tile = RasterTile::pending(descriptor);
        assert_eq!(tile.descriptor().byte_len([TILE_SIZE; 2]), Some(524288));
        let data = RasterData {
            tiles: BTreeMap::from([(
                TileKey {
                    plane: RasterPlane::Color,
                    coordinate: [0, 0],
                },
                tile.clone(),
            )]),
            watercolor: None,
        };
        data.validate_index([256; 2], false, color).unwrap();
        assert!(
            data.validate_index(
                [256; 2],
                false,
                DocumentColor {
                    depth: SampleDepth::U8,
                    ..color
                }
            )
            .is_err()
        );
        let wrong = TileBlob::encode(PixelDescriptor::SRGB8_PAINT, &vec![0; 262144]).unwrap();
        assert!(tile.publish(Ok(wrong)).is_err());
        assert!(matches!(tile.try_backing(), Some(Err(_))));
        assert_eq!(tile.descriptor(), descriptor);
    }
    #[test]
    fn failed_raster_suffix_recovers_atomically_without_redoing_lost_pixels() {
        use crate::{Document, Edit, Editor, LayerId};
        for tile_failure in [false, true] {
            let mut editor = Editor::new(Document::new("recovery", 256, 256));
            let first = RasterRevision::backed(RasterData::default());
            editor
                .perform(Edit::SetRaster {
                    target: LayerId(1),
                    revision: first.clone(),
                })
                .unwrap();
            let checkpoint = editor.checkpoint();
            let pending = RasterRevision::pending();
            editor
                .perform(Edit::SetRaster {
                    target: LayerId(1),
                    revision: pending.clone(),
                })
                .unwrap();
            assert_eq!(
                editor.recover_failed_rasters().unwrap(),
                0,
                "pending is not failed"
            );
            if tile_failure {
                let tile = RasterTile::pending(PixelDescriptor::SRGB8_PAINT);
                tile.publish(Err("readback failed".into())).unwrap();
                pending
                    .publish(Ok(RasterData {
                        tiles: BTreeMap::from([(
                            TileKey {
                                plane: RasterPlane::Color,
                                coordinate: [0, 0],
                            },
                            tile,
                        )]),
                        ..Default::default()
                    }))
                    .unwrap();
            } else {
                pending.publish(Err("encoding failed".into())).unwrap();
            }
            assert_eq!(editor.recover_failed_rasters().unwrap(), 1);
            assert_eq!(editor.checkpoint(), checkpoint);
            assert_eq!(editor.document().layers[0].raster, first);
            assert!(!editor.can_redo());
            assert!(editor.undo().unwrap());
            assert!(editor.redo().unwrap());
            assert_eq!(editor.document().layers[0].raster, first);
            assert_eq!(editor.recover_failed_rasters().unwrap(), 0);
        }
        let mut document = Document::new("no retained boundary", 256, 256);
        document.layers[0].raster = RasterRevision::pending();
        document.layers[0]
            .raster
            .publish(Err("lost source".into()))
            .unwrap();
        let mut editor = Editor::new(document.clone());
        assert!(editor.recover_failed_rasters().is_err());
        assert_eq!(
            editor.document(),
            &document,
            "failure cannot partially roll back"
        );

        let mut editor = Editor::new(Document::new("failure after undo", 256, 256));
        let pending = RasterRevision::pending();
        editor
            .perform(Edit::SetRaster {
                target: LayerId(1),
                revision: pending.clone(),
            })
            .unwrap();
        editor.undo().unwrap();
        pending
            .publish(Err("abandoned queued frame".into()))
            .unwrap();
        assert!(editor.can_redo());
        assert_eq!(editor.recover_failed_rasters().unwrap(), 0);
        assert!(
            !editor.can_redo(),
            "late failure cannot be restored through redo"
        );
    }
    #[test]
    fn exact_backing_reuses_unchanged_tiles_and_preserves_snapshot() {
        let size = PixelDescriptor::SRGB8_PAINT
            .byte_len([TILE_SIZE; 2])
            .unwrap();
        let bytes: Vec<_> = (0..size).map(|i| (i % 251) as u8).collect();
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
        next.tiles
            .insert(changed, RasterTile::pending(PixelDescriptor::SRGB8_PAINT));
        assert!(next.tiles[&key].same_capture(&revision.wait_data().unwrap().tiles[&key]));
        assert!(!next.host_backed());
        assert!(revision.host_backed());
        assert_eq!(tile.wait_backing().unwrap().decode().unwrap(), bytes);
        assert_eq!(revision.wait_data().unwrap().tiles.len(), 1);
    }
    #[test]
    fn capture_failure_is_shared_and_publication_is_single_assignment() {
        let tile = RasterTile::pending(PixelDescriptor::SRGB8_PAINT);
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
        let mut compressed = blob.compressed().unwrap().to_vec();
        compressed[4] ^= 1;
        assert!(
            TileBlob::from_compressed(blob.descriptor, blob.digest, compressed.into()).is_err()
        );
        assert!(
            TileBlob::from_compressed(
                PixelDescriptor::SRGB8_PAINT,
                blob.digest,
                blob.compressed().unwrap()
            )
            .is_err()
        );
        let mut tail = blob.compressed().unwrap().to_vec();
        tail.push(0);
        assert!(TileBlob::from_compressed(blob.descriptor, blob.digest, tail.into()).is_err());
    }
}
