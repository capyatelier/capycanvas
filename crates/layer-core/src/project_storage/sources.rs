//! Immutable source indices share tile payloads with the archive. Profiles are
//! binary payloads, never large JSON byte arrays or reconstructed display names.
use super::*;
use crate::color::{ColorProfile, IntegerDepth, RgbSpace, source::*};

#[derive(Serialize, Deserialize)]
enum ProfileReference {
    Builtin(RgbSpace),
    Embedded(usize),
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageRecord {
    extent: [u32; 2],
    channels: SourceChannels,
    depth: IntegerDepth,
    profile: ProfileReference,
    profile_assumed: bool,
    tiles: Vec<SourceTileRecord>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceTileRecord {
    coordinate: [u32; 2],
    blob: usize,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayerSourceRecord {
    target: LayerId,
    image: usize,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileRecord {
    offset: u64,
    size: u64,
    digest: [u8; 32],
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceIndex {
    images: Vec<ImageRecord>,
    layers: Vec<LayerSourceRecord>,
    profiles: Vec<ProfileRecord>,
}
impl SourceIndex {
    pub(super) fn collect(
        document: &Document,
        blobs: &mut Vec<Arc<TileBlob>>,
        blob_ids: &mut BTreeMap<[u8; 32], usize>,
        tile_count: &mut usize,
    ) -> (Self, Vec<Arc<[u8]>>) {
        let mut result = Self::default();
        let mut images = BTreeMap::new();
        let mut profiles = BTreeMap::new();
        let mut payloads = Vec::new();
        for layer in &document.layers {
            let Some(source) = &layer.source else {
                continue;
            };
            let image = *images
                .entry(Arc::as_ptr(source) as usize)
                .or_insert_with(|| {
                    let profile = match &source.interpretation.profile {
                        ColorProfile::Builtin(space) => ProfileReference::Builtin(*space),
                        ColorProfile::Icc(bytes) => {
                            let digest: [u8; 32] = Sha256::digest(bytes).into();
                            ProfileReference::Embedded(*profiles.entry(digest).or_insert_with(
                                || {
                                    let id = payloads.len();
                                    payloads.push(bytes.clone());
                                    id
                                },
                            ))
                        }
                    };
                    let tiles = source
                        .tiles
                        .iter()
                        .map(|(coordinate, blob)| {
                            *tile_count += 1;
                            let index = *blob_ids.entry(blob.digest).or_insert_with(|| {
                                let index = blobs.len();
                                blobs.push(blob.clone());
                                index
                            });
                            SourceTileRecord {
                                coordinate: *coordinate,
                                blob: index,
                            }
                        })
                        .collect();
                    let id = result.images.len();
                    result.images.push(ImageRecord {
                        extent: source.extent,
                        channels: source.interpretation.channels,
                        depth: source.interpretation.depth,
                        profile,
                        profile_assumed: source.interpretation.profile_assumed,
                        tiles,
                    });
                    id
                });
            result.layers.push(LayerSourceRecord {
                target: layer.id,
                image,
            });
        }
        (result, payloads)
    }

    pub(super) fn index_profiles(&mut self, payloads: &[Arc<[u8]>], offset: &mut u64) {
        self.profiles = payloads
            .iter()
            .map(|bytes| {
                let record = ProfileRecord {
                    offset: *offset,
                    size: bytes.len() as u64,
                    digest: Sha256::digest(bytes).into(),
                };
                *offset += record.size;
                record
            })
            .collect();
    }

    /// Validate every role, coordinate, reference and retained allocation before
    /// decoding any payload. Source raw samples are never allocated all at once.
    pub(super) fn validate(
        &self,
        document: &Document,
        blobs: &[BlobRecord],
        limits: ProjectLimits,
        offset: &mut u64,
        referenced: &mut BTreeSet<usize>,
        tile_count: &mut usize,
    ) -> Result<u64, String> {
        if self.images.len() > limits.layers
            || self.layers.len() > limits.layers
            || self.profiles.len() > self.images.len()
        {
            return Err("Oversized source index".into());
        }
        let mut targets = BTreeSet::new();
        let mut images = BTreeSet::new();
        for binding in &self.layers {
            let layer = document
                .layer(binding.target)
                .ok_or("Missing source target")?;
            if layer.asset.is_some()
                || !matches!(
                    layer.kind,
                    LayerKind::Paint | LayerKind::ImportedImage | LayerKind::AiSuggestion
                )
                || binding.image >= self.images.len()
                || !targets.insert(binding.target)
            {
                return Err("Invalid source target".into());
            }
            images.insert(binding.image);
        }
        if images.len() != self.images.len() {
            return Err("Unused source image".into());
        }
        let mut profiles = BTreeSet::new();
        let mut source_blobs = BTreeSet::new();
        let mut bytes = 0u64;
        for image in &self.images {
            if image
                .extent
                .iter()
                .any(|v| *v == 0 || *v > limits.dimension.min(32768))
            {
                return Err("Invalid source dimensions".into());
            }
            let interpretation = SourceInterpretation {
                channels: image.channels,
                depth: image.depth,
                profile: ColorProfile::default(),
                profile_assumed: image.profile_assumed,
            };
            let [columns, rows] = image.extent.map(|v| v.div_ceil(TILE_SIZE));
            if image.tiles.len() != columns as usize * rows as usize {
                return Err("Incomplete source tile index".into());
            }
            if let ProfileReference::Embedded(id) = image.profile {
                if id >= self.profiles.len() {
                    return Err("Missing embedded source profile".into());
                }
                profiles.insert(id);
            }
            bytes = bytes
                .saturating_add(std::mem::size_of::<SourceImage>() as u64)
                .saturating_add(image.tiles.len() as u64 * 96);
            let mut coordinates = BTreeSet::new();
            for tile in &image.tiles {
                let blob = blobs.get(tile.blob).ok_or("Missing source tile blob")?;
                if tile.coordinate[0] >= columns
                    || tile.coordinate[1] >= rows
                    || !coordinates.insert(tile.coordinate)
                    || blob.descriptor != interpretation.descriptor()
                {
                    return Err("Invalid source tile reference".into());
                }
                if source_blobs.insert(tile.blob) {
                    bytes = bytes.saturating_add(blob.size);
                }
                referenced.insert(tile.blob);
                *tile_count = tile_count.saturating_add(1);
            }
        }
        if profiles.len() != self.profiles.len() {
            return Err("Unused embedded source profile".into());
        }
        for profile in &self.profiles {
            if profile.offset != *offset
                || profile.size == 0
                || profile.size > MAX_PROFILE_BYTES as u64
            {
                return Err("Invalid embedded source profile index".into());
            }
            *offset = offset
                .checked_add(profile.size)
                .ok_or("Source profile index overflow")?;
            bytes = bytes.saturating_add(profile.size);
        }
        if bytes > limits.asset_bytes || *tile_count > limits.tiles {
            return Err("Source images exceed the memory budget".into());
        }
        Ok(bytes)
    }

    pub(super) fn read(
        self,
        input: &mut impl Read,
        tiles: &[RasterTile],
        document: &mut Document,
    ) -> Result<(), String> {
        let mut profiles: Vec<Arc<[u8]>> = Vec::new();
        for record in self.profiles {
            let bytes = read_block(input, record.size, MAX_PROFILE_BYTES as u64)?;
            if <[u8; 32]>::from(Sha256::digest(&bytes)) != record.digest {
                return Err("Source profile integrity check failed".into());
            }
            profiles.push(bytes.into());
        }
        let mut images = Vec::new();
        for image in self.images {
            images.push(Arc::new(SourceImage {
                extent: image.extent,
                interpretation: SourceInterpretation {
                    channels: image.channels,
                    depth: image.depth,
                    profile: match image.profile {
                        ProfileReference::Builtin(space) => ColorProfile::Builtin(space),
                        ProfileReference::Embedded(id) => ColorProfile::Icc(profiles[id].clone()),
                    },
                    profile_assumed: image.profile_assumed,
                },
                tiles: image
                    .tiles
                    .into_iter()
                    .map(|tile| Ok((tile.coordinate, tiles[tile.blob].wait_backing()?)))
                    .collect::<Result<_, String>>()?,
            }));
        }
        for binding in self.layers {
            document
                .layers
                .iter_mut()
                .find(|l| l.id == binding.target)
                .ok_or("Missing source target")?
                .source = Some(images[binding.image].clone());
        }
        Ok(())
    }
}
