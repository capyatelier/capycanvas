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
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

pub const TILE_SIZE: u32 = crate::package::RASTER_TILE_SIZE;
pub const MAX_TILE_BYTES: usize = (TILE_SIZE * TILE_SIZE * 16) as usize;
pub(crate) const MAX_COMPRESSED_TILE_BYTES: usize =
    lz4_flex::block::get_maximum_output_size(MAX_TILE_BYTES);
mod compression;
pub const MAX_CAPTURE_BYTES: u64 = 256 * 1024 * 1024;
/// Immutable native output one frame may publish: a full 60 MP 16-bit edit
/// with its linked mask.
pub const MAX_PUBLICATION_BYTES: u64 = 1024 * 1024 * 1024;

/// Raster pages of a target `extent` pixels large that `bounds` touches.
pub fn page_count(bounds: crate::Rect, extent: [u32; 2]) -> u64 {
    if bounds.is_empty() {
        return 0;
    }
    let size = TILE_SIZE as f32;
    let span = |min: f32, max: f32, limit: u32| {
        let first = (min.max(0.) / size).floor() as u64;
        let last = (max.min(limit as f32) / size).ceil() as u64;
        last.saturating_sub(first)
    };
    span(bounds.min.x, bounds.max.x, extent[0]) * span(bounds.min.y, bounds.max.y, extent[1])
}
#[cfg(not(target_arch = "wasm32"))]
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(30);
static NEXT_PUBLICATION: AtomicU64 = AtomicU64::new(1);
static NEXT_TILE_OWNER: AtomicU64 = AtomicU64::new(1);
fn next_tile_owner() -> u64 {
    NEXT_TILE_OWNER.try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1)).expect("Raster tile identity exhausted")
}

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
    fn wait(&self) -> Result<Arc<T>, String> {
        self.wait_cancellable(None)
    }
    #[cfg(target_arch = "wasm32")]
    fn wait_cancellable(&self, cancelled: Option<&AtomicBool>) -> Result<Arc<T>, String> {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Relaxed)) { return Err("Raster capture cancelled".into()); }
        self.get().ok_or("Raster capture is still pending")?
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn wait_cancellable(&self, cancelled: Option<&AtomicBool>) -> Result<Arc<T>, String> {
        let deadline = std::time::Instant::now() + CAPTURE_TIMEOUT;
        let mut state = self.value.lock().map_err(|_| "Raster publication failed")?;
        loop {
            if cancelled.is_some_and(|flag| flag.load(Ordering::Relaxed)) { return Err("Raster capture cancelled".into()); }
            if let Some(value) = &*state { return value.clone(); }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() { return Err("Raster capture did not complete".into()); }
            let interval = if cancelled.is_some() { remaining.min(Duration::from_millis(10)) } else { remaining };
            (state, _) = self.ready.wait_timeout(state, interval).map_err(|_| "Raster publication failed")?;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RasterPlane {
    Color,
    Mask,
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
    pub fn descriptor_for(self, color: crate::color::DocumentColor, mode: crate::color::LayerColorMode) -> crate::color::PixelDescriptor {
        if self == Self::Color { mode.descriptor(color) } else { self.descriptor(color) }
    }

}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TileKey {
    pub plane: RasterPlane,
    pub coordinate: [u32; 2],
}

/// Independently compressed exact samples. Immutable backing
/// can be written repeatedly without readback, conversion or recompression.
pub struct TileBlob {
    resource_id: crate::authored::PortableId,
    owner_identity: u64,
    encoded_fingerprint: Option<[u8; 32]>,
    digest: OnceLock<[u8; 32]>,
    expected_digest: Option<[u8; 32]>,
    pub descriptor: PixelDescriptor,
    pub(crate) compressed: Arc<crate::raster_storage::Bytes>,
}
impl std::fmt::Debug for TileBlob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TileBlob")
            .field("owner_identity", &self.owner_identity)
            .field("resource_id", &self.resource_id)
            .field("digest", &self.digest.get())
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
    fn interleave<const N: usize>(bytes: &[u8]) -> Vec<u8> {
        let pixels = bytes.len() / N;
        let planes: [&[u8]; N] = std::array::from_fn(|i| &bytes[i * pixels..(i + 1) * pixels]);
        let mut result = vec![0; bytes.len()];
        for (pixel, destination) in result.as_chunks_mut::<N>().0.iter_mut().enumerate() {
            for (channel, byte) in destination.iter_mut().enumerate() {
                *byte = planes[channel][pixel];
            }
        }
        result
    }
    let bpp = descriptor
        .bytes_per_pixel()
        .expect("validated multibyte descriptor");
    match bpp {
        2 => interleave::<2>(bytes), 4 => interleave::<4>(bytes),
        6 => interleave::<6>(bytes), 8 => interleave::<8>(bytes),
        12 => interleave::<12>(bytes), 16 => interleave::<16>(bytes),
        _ => unreachable!("validated multibyte descriptor"),
    }
}

impl TileBlob {
    pub fn owner_identity(&self) -> u64 { self.owner_identity }
    pub fn encoded_fingerprint(&self) -> Option<[u8; 32]> { self.encoded_fingerprint }
    pub fn resource_id(&self) -> crate::authored::PortableId { self.resource_id }
    pub(crate) fn alias(&self, resource_id: crate::authored::PortableId) -> Self {
        Self { resource_id, owner_identity: self.owner_identity,
            encoded_fingerprint: self.encoded_fingerprint, digest: self.digest.clone(), expected_digest: self.expected_digest, descriptor: self.descriptor, compressed: self.compressed.clone() }
    }
    /// Worst-case encoded ownership reserved before a tile is published.
    pub fn max_compressed_len(descriptor: PixelDescriptor) -> Option<usize> {
        descriptor.byte_len([TILE_SIZE; 2]).map(lz4_flex::block::get_maximum_output_size)
    }
    pub fn encode(descriptor: PixelDescriptor, bytes: &[u8]) -> Result<Self, String> {
        let expected = descriptor
            .byte_len([TILE_SIZE; 2])
            .ok_or("Unsupported raster pixels")?;
        if bytes.len() != expected {
            return Err("Invalid raster tile byte count".into());
        }
        descriptor.validate_samples(bytes)?;
        // Multibyte source channels benefit from byte planes: smooth high
        // bytes no longer alternate with noisy low bytes. This is a reversible
        // permutation, not a precision change.
        let shuffled = (descriptor.bits_per_channel > 8).then(|| shuffle_samples(descriptor, bytes));
        let compressed = compression::compress(shuffled.as_deref().unwrap_or(bytes))?;
        let encoded_fingerprint = Some(Self::descriptor_digest(descriptor, &compressed));
        Ok(Self {
            resource_id: crate::authored::PortableId::random(),
            owner_identity: next_tile_owner(),
            encoded_fingerprint,
            digest: OnceLock::new(),
            expected_digest: None,
            descriptor,
            compressed: Arc::new(Arc::<[u8]>::from(compressed).into()),
        })
    }
    fn descriptor_digest(descriptor: PixelDescriptor, bytes: &[u8]) -> [u8; 32] {
        let mut hash = Sha256::new();
        // Representation participates in identity; equal bytes need not mean
        // equal samples. The descriptor vocabulary is intentionally restricted.
        hash.update(serde_json::to_vec(&descriptor).expect("fixed descriptor"));
        hash.update(bytes);
        hash.finalize().into()
    }
    pub fn compressed_len(&self) -> usize { self.compressed.len() }
    pub fn compressed(&self) -> Result<Arc<[u8]>, String> { self.compressed.read() }
    /// Poll asynchronous backing without synchronously reading native files.
    pub fn compressed_ready(&self) -> Result<bool, String> { self.compressed.ready() }
    pub fn resident_bytes(&self) -> usize { self.compressed.resident_bytes() }
    fn decode_samples(descriptor: PixelDescriptor, compressed: &[u8]) -> Result<Vec<u8>, String> {
        let size = descriptor.byte_len([TILE_SIZE; 2]).ok_or("Unsupported raster pixels")?;
        let mut bytes = compression::decompress(compressed, size)?;
        if descriptor.bits_per_channel > 8 { bytes = unshuffle_samples(descriptor, &bytes); }
        descriptor.validate_samples(&bytes)?;
        Ok(bytes)
    }
    pub fn decode(&self) -> Result<Vec<u8>, String> {
        let bytes = Self::decode_samples(self.descriptor, &self.compressed()?)?;
        if let Some(expected) = self.expected_digest {
            let actual = Self::descriptor_digest(self.descriptor, &bytes);
            if actual != expected { return Err("Raster tile integrity check failed".into()); }
            let _ = self.digest.set(actual);
        }
        Ok(bytes)
    }
    /// Decodes and hashes exact samples on a worker; ordinary identity needs neither.
    pub fn content_digest(&self) -> Result<[u8; 32], String> {
        if let Some(digest) = self.digest.get() { return Ok(*digest); }
        let bytes = self.decode()?;
        if let Some(digest) = self.digest.get() { return Ok(*digest); }
        let digest = Self::descriptor_digest(self.descriptor, &bytes);
        let _ = self.digest.set(digest);
        Ok(digest)
    }
    pub fn from_package(
        resource_id: crate::authored::PortableId,
        descriptor: PixelDescriptor,
        compressed: Arc<[u8]>,
    ) -> Result<Self, String> {
        if compressed.len() > MAX_COMPRESSED_TILE_BYTES { return Err("Oversized compressed raster tile".into()); }
        Self::decode_samples(descriptor, &compressed)?;
        let encoded_fingerprint=Some(Self::descriptor_digest(descriptor,&compressed));
        Ok(Self { resource_id, owner_identity: next_tile_owner(), descriptor,
            encoded_fingerprint, digest: OnceLock::new(), expected_digest: None, compressed: Arc::new(compressed.into()) })
    }
    pub fn from_compressed(
        descriptor: PixelDescriptor,
        digest: [u8; 32],
        bytes: Arc<[u8]>,
    ) -> Result<Self, String> {
        Self::from_compressed_with_id(crate::authored::PortableId::random(), descriptor, digest, bytes)
    }
    pub fn from_compressed_with_id(
        resource_id: crate::authored::PortableId,
        descriptor: PixelDescriptor,
        digest: [u8; 32],
        bytes: Arc<[u8]>,
    ) -> Result<Self, String> {
        if bytes.len() > MAX_COMPRESSED_TILE_BYTES {
            return Err("Oversized compressed raster tile".into());
        }
        let result = Self {
            resource_id,
            descriptor,
            owner_identity: next_tile_owner(),
            encoded_fingerprint: None,
            digest: OnceLock::new(),
            expected_digest: Some(digest),
            compressed: Arc::new(bytes.into()),
        };
        result.decode()?;
        Ok(result)
    }

    /// Adopts a validated worker resource without decoding on the receiving owner.
    pub fn from_verified_resource(
        resource_id: crate::authored::PortableId,
        descriptor: PixelDescriptor,
        bytes: Arc<[u8]>,
    ) -> Result<Self, String> {
        Self::from_verified_resource_with_encoded_fingerprint(resource_id,descriptor,bytes,None)
    }
    pub fn from_verified_resource_with_encoded_fingerprint(
        resource_id: crate::authored::PortableId,
        descriptor: PixelDescriptor,
        bytes: Arc<[u8]>,
        encoded_fingerprint: Option<[u8;32]>,
    ) -> Result<Self, String> {
        if bytes.is_empty() || bytes.len() > MAX_COMPRESSED_TILE_BYTES || descriptor.byte_len([TILE_SIZE; 2]).is_none() {
            return Err("Invalid raster worker blob".into());
        }
        Ok(Self { resource_id, owner_identity: next_tile_owner(), descriptor,
            encoded_fingerprint, digest: OnceLock::new(), expected_digest: None, compressed: Arc::new(bytes.into()) })
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
    pub fn wait_backing_cancellable(&self, cancelled: &AtomicBool) -> Result<Arc<TileBlob>, String> {
        self.0.data.wait_cancellable(Some(cancelled))
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
    pub fn has_plane(&self, plane: RasterPlane) -> bool {
        self.tiles.range(TileKey { plane, coordinate: [0; 2] }..=TileKey { plane, coordinate: [u32::MAX; 2] }).next().is_some()
    }
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
        self.validate_mode(extent, mask, color, Default::default())
    }
    pub fn validate_mode(&self, extent: [u32; 2], mask: bool, color: crate::color::DocumentColor, mode: crate::color::LayerColorMode) -> Result<(), String> {
        self.validate_index_mode(extent, mask, color, mode)?;
        for (key, tile) in &self.tiles {
            if tile.wait_backing()?.descriptor != key.plane.descriptor_for(color, mode) {
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
        self.validate_index_mode(extent, mask, color, Default::default())
    }
    pub fn validate_storage_index(&self, extent: [u32; 2], mask: bool, color: crate::color::DocumentColor) -> Result<(), String> {
        let mode = if self.tiles.iter().find(|(key, _)| key.plane == RasterPlane::Color).is_some_and(|(_, tile)| tile.descriptor().channels == 2) {
            crate::color::LayerColorMode::Grayscale
        } else { Default::default() };
        self.validate_index_mode(extent, mask, color, mode)
    }
    pub fn validate_index_mode(&self, extent: [u32; 2], mask: bool, color: crate::color::DocumentColor, mode: crate::color::LayerColorMode) -> Result<(), String> {
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
                || (key.plane == RasterPlane::WatercolorWetness && self.watercolor.is_none())
            {
                return Err("Invalid raster tile coordinates or plane".into());
            }
            if tile.descriptor() != key.plane.descriptor_for(color, mode) {
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
        Self::pending_within(MAX_CAPTURE_BYTES)
    }
    /// A pending revision whose capture holds at most `bytes`, such as an
    /// empty new layer that operations write known pages into.
    pub fn pending_within(bytes: u64) -> Self {
        Self(Arc::new(RasterPublication {
            data: Publication::default(),
            pending_bytes: AtomicU64::new(bytes),
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
    pub fn wait_data_cancellable(&self, cancelled: &AtomicBool) -> Result<Arc<RasterData>, String> {
        self.0.data.wait_cancellable(Some(cancelled))
    }
    pub fn host_backed(&self) -> bool {
        matches!(self.try_data(), Some(Ok(data)) if data.host_backed())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn tile_resource_identity_preserves_saved_compression_and_samples() {
        let descriptor = crate::color::PixelDescriptor::COVERAGE8;
        let samples = vec![81; (super::TILE_SIZE * super::TILE_SIZE) as usize];
        let original = super::TileBlob::encode(descriptor, &samples).unwrap();
        assert!(original.digest.get().is_none());
        assert!(original.encoded_fingerprint().is_some());
        let alias = original.alias(crate::authored::PortableId::random());
        assert_eq!(alias.owner_identity(), original.owner_identity());
        assert_eq!(alias.encoded_fingerprint(), original.encoded_fingerprint());
        assert_eq!(original.decode().unwrap(), samples);
        assert!(original.digest.get().is_none(), "ordinary decode does not compute a content hash");
        let encoded = original.compressed().unwrap();
        let restored = super::TileBlob::from_compressed_with_id(original.resource_id(), descriptor, original.content_digest().unwrap(), encoded.clone()).unwrap();
        assert_eq!(restored.resource_id(), original.resource_id());
        assert!(restored.encoded_fingerprint().is_none());
        assert!(std::sync::Arc::ptr_eq(&restored.compressed().unwrap(), &encoded));
        assert_eq!(restored.decode().unwrap(), samples);
        let package = super::TileBlob::from_package(original.resource_id(), descriptor, encoded.clone()).unwrap();
        assert!(package.digest.get().is_none(), "package validation does not compute a content hash");
        assert_eq!(package.encoded_fingerprint(),original.encoded_fingerprint());
        let adopted = super::TileBlob::from_verified_resource(original.resource_id(), descriptor, encoded.clone()).unwrap();
        assert!(adopted.encoded_fingerprint().is_none());
        assert!(adopted.digest.get().is_none());
        assert_ne!(package.owner_identity(), original.owner_identity());
        assert_ne!(restored.owner_identity(), original.owner_identity());
        assert_eq!(package.content_digest().unwrap(), original.content_digest().unwrap());
        assert!(package.digest.get().is_some());
        assert_eq!(package.decode().unwrap(), samples);
        assert_eq!(package.resource_id(), original.resource_id());
        assert!(std::sync::Arc::ptr_eq(&package.compressed().unwrap(), &encoded));
        let independent = super::TileBlob::encode(descriptor, &samples).unwrap();
        assert_ne!(independent.resource_id(), original.resource_id());
        assert_ne!(independent.owner_identity(), original.owner_identity());
        assert_eq!(independent.encoded_fingerprint(), original.encoded_fingerprint());
        assert_eq!(independent.content_digest().unwrap(), original.content_digest().unwrap());
    }

    use super::*;
    #[test]
    fn encoded_fingerprints_distinguish_descriptors_with_equal_compressed_bytes() {
        use crate::color::{AlphaAssociation, SampleType, TransferEncoding};
        let rgba = PixelDescriptor { sample: SampleType::Unsigned, channels: 4, bits_per_channel: 8,
            encoding: TransferEncoding::Srgb, alpha: AlphaAssociation::Straight };
        let rgba16 = PixelDescriptor { bits_per_channel: 16, encoding: TransferEncoding::Linear, ..rgba };
        for (first, second) in [
            (rgba, PixelDescriptor { alpha: AlphaAssociation::PremultipliedLinear, ..rgba }),
            (rgba, PixelDescriptor { encoding: TransferEncoding::Linear, ..rgba }),
            (rgba16, PixelDescriptor { sample: SampleType::Float, ..rgba16 }),
            (PixelDescriptor { bits_per_channel: 16, ..PixelDescriptor::COVERAGE8 },
                PixelDescriptor { channels: 2, alpha: AlphaAssociation::Straight, ..PixelDescriptor::COVERAGE8 }),
        ] {
            let first = TileBlob::encode(first, &vec![0; first.byte_len([TILE_SIZE; 2]).unwrap()]).unwrap();
            let second = TileBlob::encode(second, &vec![0; second.byte_len([TILE_SIZE; 2]).unwrap()]).unwrap();
            assert_eq!(first.compressed().unwrap(), second.compressed().unwrap());
            assert_ne!(first.encoded_fingerprint(), second.encoded_fingerprint());
            assert!(first.digest.get().is_none());
            assert!(second.digest.get().is_none());
        }
    }
    #[test]
    fn multibyte_tiles_preserve_every_channel_and_validate_integrity() {
        use crate::color::{AlphaAssociation, SampleType, TransferEncoding};
        for (sample, bits, channels) in [
            (SampleType::Unsigned, 16, 1), (SampleType::Unsigned, 16, 2),
            (SampleType::Unsigned, 16, 3), (SampleType::Unsigned, 16, 4),
            (SampleType::Float, 16, 3), (SampleType::Float, 16, 4),
            (SampleType::Float, 32, 3), (SampleType::Float, 32, 4),
        ] {
            let descriptor = PixelDescriptor {
                sample, bits_per_channel: bits, channels, encoding: TransferEncoding::Linear,
                alpha: if channels == 2 || channels == 4 { AlphaAssociation::Straight } else { AlphaAssociation::None },
            };
            let bytes: Vec<_> = (0..65536 * u32::from(channels)).flat_map(|i| {
                let code = (i.wrapping_mul(103) ^ (i / 37)) as u16;
                if bits == 32 { (f32::from(code) / 65535.).to_le_bytes().to_vec() }
                else { (if sample == SampleType::Float { code % 0x3c00 } else { code }).to_le_bytes().to_vec() }
            }).collect();
            let blob = TileBlob::encode(descriptor, &bytes).unwrap();
            assert_eq!(blob.decode().unwrap(), bytes, "{descriptor:?}");
            let mut digest = blob.content_digest().unwrap();
            digest[7] ^= 1;
            assert!(TileBlob::from_compressed(descriptor, digest, blob.compressed().unwrap()).is_err());
        }
    }
    #[test]
    fn pending_history_charges_each_retained_tiles_own_precision() {
        use crate::color::{DocumentColor, SampleDepth, RgbSpace};
        use crate::{Document, Edit, Editor};
        for (bits, expected_undo) in [(8, 4), (16, 2), (32, 1)] {
            // Even while the current document is still sRGB8, old revision
            // tickets own their layout. No pixel allocation/readback is needed
            // to enforce the 512 MiB history ceiling. 450 tiles leave room
            // for the codec's worst-case expansion within that ceiling.
            let mut document = Document::new(crate::authored::PortableId::random(), 6400, 5120, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
            let color = DocumentColor {
                space: RgbSpace::ProPhoto,
                depth: match bits { 8 => SampleDepth::U8, 16 => SampleDepth::U16, _ => SampleDepth::F32 },
            };
            document.artwork.compositions.get_mut(document.artwork.root).unwrap().color = color;
            let mut editor = Editor::new(document);
            let descriptor = color.paint_descriptor();
            for _ in 0..3 {
                let data = RasterData {
                    tiles: (0..450)
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
                        target: editor.document().working.target.unwrap(),
                        revision: RasterRevision::backed(data),
                    })
                    .unwrap();
            }
            let mut composition = editor.document().composition().clone(); composition.color = DocumentColor::default();
            editor.perform(Edit::Batch(vec![
                Edit::SetRaster { target: editor.document().working.target.unwrap(), revision: RasterRevision::backed(RasterData::default()) },
                Edit::Composition(crate::authored::RecordChange::replace(&editor.document().artwork.compositions, editor.document().artwork.root, Some(composition)).unwrap()),
            ])).unwrap();
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
        let wetness = TileKey { plane: RasterPlane::WatercolorWetness, coordinate: [0, 0] };
        let mut watercolor = RasterData { tiles: BTreeMap::from([(wetness, RasterTile::pending(RasterPlane::WatercolorWetness.descriptor(color)))]), watercolor: None };
        assert!(watercolor.validate_index([256; 2], false, color).is_err());
        watercolor.watercolor = Some(RasterWatercolor { wet_edge: 0.5, burnt_edge: 0.5, edge_width: 2. });
        watercolor.validate_index([256; 2], false, color).unwrap();
        let wrong = TileBlob::encode(crate::color::SRGB8_PAINT, &vec![0; 262144]).unwrap();
        assert!(tile.publish(Ok(wrong)).is_err());
        assert!(matches!(tile.try_backing(), Some(Err(_))));
        assert_eq!(tile.descriptor(), descriptor);
    }
    #[test]
    fn failed_raster_suffix_recovers_atomically_without_redoing_lost_pixels() {
        use crate::{Document, Edit, Editor};
        for tile_failure in [false, true] {
            let mut editor = Editor::new(Document::new(crate::authored::PortableId::random(), 256, 256, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }));
            let first = RasterRevision::backed(RasterData::default());
            editor
                .perform(Edit::SetRaster {
                    target: editor.document().working.target.unwrap(),
                    revision: first.clone(),
                })
                .unwrap();
            let checkpoint = editor.checkpoint();
            let pending = RasterRevision::pending();
            editor
                .perform(Edit::SetRaster {
                    target: editor.document().working.target.unwrap(),
                    revision: pending.clone(),
                })
                .unwrap();
            assert_eq!(
                editor.recover_failed_rasters().unwrap(),
                0,
                "pending is not failed"
            );
            if tile_failure {
                let tile = RasterTile::pending(RasterPlane::Color.descriptor(Default::default()));
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
            assert_eq!(editor.document().target_raster(editor.document().working.target.unwrap()).unwrap(), &first);
            assert!(!editor.can_redo());
            assert!(editor.undo().unwrap());
            assert!(editor.redo().unwrap());
            assert_eq!(editor.document().target_raster(editor.document().working.target.unwrap()).unwrap(), &first);
            assert_eq!(editor.recover_failed_rasters().unwrap(), 0);
        }
        let mut document = Document::new(crate::authored::PortableId::random(), 256, 256, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        let missing = RasterRevision::pending();
        document.apply(Edit::SetRaster { target: document.working.target.unwrap(), revision: missing.clone() }).unwrap();
        missing.publish(Err("lost source".into())).unwrap();
        let mut editor = Editor::new(document.clone());
        assert!(editor.recover_failed_rasters().is_err());
        assert_eq!(
            editor.document(),
            &document,
            "failure cannot partially roll back"
        );

        let mut editor = Editor::new(Document::new(crate::authored::PortableId::random(), 256, 256, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }));
        let pending = RasterRevision::pending();
        editor
            .perform(Edit::SetRaster {
                target: editor.document().working.target.unwrap(),
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
        let descriptor = RasterPlane::Color.descriptor(Default::default());
        let size = descriptor.byte_len([TILE_SIZE; 2]).unwrap();
        let bytes: Vec<_> = (0..size).map(|i| (i % 251) as u8).collect();
        let tile = RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap());
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
        next.tiles.insert(changed, RasterTile::pending(descriptor));
        assert!(next.tiles[&key].same_capture(&revision.wait_data().unwrap().tiles[&key]));
        assert!(!next.host_backed());
        assert!(revision.host_backed());
        assert_eq!(tile.wait_backing().unwrap().decode().unwrap(), bytes);
        assert_eq!(revision.wait_data().unwrap().tiles.len(), 1);
    }
    #[test]
    fn capture_failure_is_shared_and_publication_is_single_assignment() {
        let tile = RasterTile::pending(RasterPlane::Color.descriptor(Default::default()));
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
            TileBlob::from_compressed(blob.descriptor, blob.content_digest().unwrap(), compressed.into()).is_err()
        );
        assert!(
            TileBlob::from_compressed(
                crate::color::SRGB8_PAINT,
                blob.content_digest().unwrap(),
                blob.compressed().unwrap()
            )
            .is_err()
        );
        let mut tail = blob.compressed().unwrap().to_vec();
        tail.push(0);
        assert!(TileBlob::from_compressed(blob.descriptor, blob.content_digest().unwrap(), tail.into()).is_err());
    }
}
