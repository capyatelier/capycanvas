//! Indexed project transport. Metadata and each compressed tile have independent
//! integrity checks; stream offsets are relative to the end of the manifest.
use super::project::{AssetRecord, io_error, metadata, read_block, validate_document};
use crate::{raster::*, *};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

const MAGIC: &[u8; 12] = b"CAPYRASTER\x01\0";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TileCodec {
    Zstd,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TileRecord {
    key: TileKey,
    blob: usize,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RasterRecord {
    target: LayerId,
    tiles: Vec<TileRecord>,
    watercolor: Option<RasterWatercolor>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlobRecord {
    offset: u64,
    size: u64,
    digest: [u8; 32],
    descriptor: color::PixelDescriptor,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRecord {
    asset: AssetRecord,
    offset: u64,
    size: u64,
    digest: [u8; 32],
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest<D = Document> {
    document: D,
    tile_size: u32,
    tile_codec: TileCodec,
    rasters: Vec<RasterRecord>,
    blobs: Vec<BlobRecord>,
    sources: Vec<SourceRecord>,
}

pub(super) fn write(project: &Project, mut output: impl Write) -> Result<(), String> {
    let limits = ProjectLimits::default();
    project.validate(limits)?;
    let mut rasters = Vec::new();
    let mut blobs = Vec::<Arc<TileBlob>>::new();
    let mut ids = BTreeMap::new();
    let mut tile_count = 0;
    let mut raw_bytes = 0u64;
    for layer in &project.document.layers {
        for (target, raster, mask) in std::iter::once((layer.id, &layer.raster, false))
            .chain(layer.mask.iter().map(|m| (m.id, &m.raster, true)))
        {
            let data = raster.wait_data()?;
            data.validate([project.document.width, project.document.height], mask)?;
            let mut tiles = Vec::new();
            for (key, tile) in &data.tiles {
                tile_count += 1;
                let blob = tile.wait_backing()?;
                raw_bytes = raw_bytes
                    .checked_add(blob.descriptor.byte_len([TILE_SIZE; 2]).unwrap() as u64)
                    .filter(|v| *v <= limits.raster_bytes)
                    .ok_or("Raster data exceeds the memory budget")?;
                if tile_count > limits.tiles {
                    return Err("Project has too many raster tiles".into());
                }
                let id = *ids.entry(blob.digest).or_insert_with(|| {
                    let id = blobs.len();
                    blobs.push(blob);
                    id
                });
                tiles.push(TileRecord {
                    key: *key,
                    blob: id,
                });
            }
            rasters.push(RasterRecord {
                target,
                tiles,
                watercolor: data.watercolor,
            });
        }
    }
    let mut offset = 0u64;
    let records = blobs
        .iter()
        .map(|blob| {
            let size = blob.compressed().len() as u64;
            let record = BlobRecord {
                offset,
                size,
                digest: blob.digest,
                descriptor: blob.descriptor,
            };
            offset += size;
            record
        })
        .collect();
    let sources = project
        .assets
        .iter()
        .map(|(id, asset)| {
            let size = asset.bytes.len() as u64;
            let record = SourceRecord {
                asset: AssetRecord {
                    id: id.clone(),
                    extent: asset.extent,
                    format: asset.format,
                },
                offset,
                size,
                digest: Sha256::digest(&asset.bytes).into(),
            };
            offset += size;
            record
        })
        .collect();
    let manifest = Manifest {
        document: &project.document,
        tile_size: TILE_SIZE,
        tile_codec: TileCodec::Zstd,
        rasters,
        blobs: records,
        sources,
    };
    let json = metadata(&manifest, limits.metadata_bytes)?;
    output.write_all(MAGIC).map_err(io_error)?;
    output
        .write_all(&(json.len() as u64).to_le_bytes())
        .map_err(io_error)?;
    output.write_all(&Sha256::digest(&json)).map_err(io_error)?;
    output.write_all(&json).map_err(io_error)?;
    for blob in blobs {
        output.write_all(blob.compressed()).map_err(io_error)?;
    }
    for source in project.assets.values() {
        output.write_all(&source.bytes).map_err(io_error)?;
    }
    Ok(())
}

pub(super) fn read(mut input: impl Read, limits: ProjectLimits) -> Result<Project, String> {
    let mut magic = [0; 12];
    input.read_exact(&mut magic).map_err(io_error)?;
    if &magic != MAGIC {
        return Err(
            "Unsupported Capy Canvas project version; this app opens raster projects only".into(),
        );
    }
    let mut length = [0; 8];
    input.read_exact(&mut length).map_err(io_error)?;
    let mut digest = [0; 32];
    input.read_exact(&mut digest).map_err(io_error)?;
    let json = read_block(
        &mut input,
        u64::from_le_bytes(length),
        limits.metadata_bytes,
    )?;
    if <[u8; 32]>::from(Sha256::digest(&json)) != digest {
        return Err("Project metadata integrity check failed".into());
    }
    let mut manifest: Manifest =
        serde_json::from_slice(&json).map_err(|e| format!("Invalid project metadata: {e}"))?;
    validate_document(&manifest.document, limits)?;
    if manifest.tile_size != TILE_SIZE
        || manifest.blobs.len() > limits.tiles
        || manifest.rasters.len() > limits.layers * 2
    {
        return Err("Unsupported or oversized raster index".into());
    }
    // Validate all offsets, roles and allocation budgets before reading payload.
    let mut offset = 0u64;
    let mut decoded = 0u64;
    for blob in &manifest.blobs {
        let raw = blob
            .descriptor
            .byte_len([TILE_SIZE; 2])
            .ok_or("Unsupported raster pixel descriptor")? as u64;
        decoded = decoded
            .checked_add(raw)
            .filter(|v| *v <= limits.raster_bytes)
            .ok_or("Raster data exceeds the memory budget")?;
        if blob.offset != offset || blob.size == 0 || blob.size > MAX_TILE_BYTES as u64 + 1024 {
            return Err("Invalid raster chunk index".into());
        }
        offset = offset
            .checked_add(blob.size)
            .ok_or("Raster index overflow")?;
    }
    let mut source_bytes = 0u64;
    let mut asset_ids = BTreeSet::new();
    for source in &manifest.sources {
        if source.offset != offset
            || source.size != source.asset.size(limits)?
            || !asset_ids.insert(&source.asset.id)
        {
            return Err("Invalid source image index".into());
        }
        source_bytes = source_bytes
            .checked_add(source.size)
            .filter(|v| *v <= limits.asset_bytes)
            .ok_or("Source images exceed the memory budget")?;
        offset = offset
            .checked_add(source.size)
            .ok_or("Source index overflow")?;
    }
    let mut referenced = BTreeSet::new();
    let mut target_ids = BTreeSet::new();
    let mut tile_count = 0;
    let mut instance_bytes = 0u64;
    for raster in &manifest.rasters {
        if !target_ids.insert(raster.target) {
            return Err("Duplicate raster target".into());
        }
        let mut keys = BTreeSet::new();
        for tile in &raster.tiles {
            let blob = manifest.blobs.get(tile.blob).ok_or("Missing raster blob")?;
            if !keys.insert(tile.key) || blob.descriptor != tile.key.plane.descriptor() {
                return Err("Invalid raster tile reference".into());
            }
            referenced.insert(tile.blob);
            tile_count += 1;
            instance_bytes = instance_bytes
                .checked_add(blob.descriptor.byte_len([TILE_SIZE; 2]).unwrap() as u64)
                .filter(|v| *v <= limits.raster_bytes)
                .ok_or("Raster instances exceed the memory budget")?;
        }
    }
    if tile_count > limits.tiles || referenced.len() != manifest.blobs.len() {
        return Err("Oversized raster index or unused blobs".into());
    }
    let mut tiles = Vec::new();
    for blob in manifest.blobs {
        let bytes = read_block(&mut input, blob.size, MAX_TILE_BYTES as u64 + 1024)?;
        tiles.push(RasterTile::backed(TileBlob::from_compressed(
            blob.descriptor,
            blob.digest,
            bytes.into(),
        )?));
    }
    for raster in manifest.rasters {
        let mask = manifest
            .document
            .layers
            .iter()
            .any(|l| l.mask.as_ref().is_some_and(|m| m.id == raster.target));
        let data = RasterData {
            tiles: raster
                .tiles
                .into_iter()
                .map(|t| (t.key, tiles[t.blob].clone()))
                .collect(),
            watercolor: raster.watercolor,
        };
        data.validate([manifest.document.width, manifest.document.height], mask)?;
        let revision = if let Some(layer) = manifest
            .document
            .layers
            .iter_mut()
            .find(|l| l.id == raster.target)
        {
            &mut layer.raster
        } else {
            &mut manifest
                .document
                .layers
                .iter_mut()
                .filter_map(|l| l.mask.as_mut())
                .find(|m| m.id == raster.target)
                .ok_or("Raster target is missing")?
                .raster
        };
        *revision = RasterRevision::backed(data);
    }
    if manifest.document.layers.iter().any(|l| {
        !target_ids.contains(&l.id) || l.mask.as_ref().is_some_and(|m| !target_ids.contains(&m.id))
    }) {
        return Err("Project is missing a raster target".into());
    }
    let mut assets = BTreeMap::new();
    for source in manifest.sources {
        let bytes = read_block(&mut input, source.size, limits.asset_bytes)?;
        if <[u8; 32]>::from(Sha256::digest(&bytes)) != source.digest {
            return Err("Source image integrity check failed".into());
        }
        assets.insert(
            source.asset.id,
            ProjectAsset {
                extent: source.asset.extent,
                format: source.asset.format,
                bytes: bytes.into(),
            },
        );
    }
    let mut extra = [0];
    if input.read(&mut extra).map_err(io_error)? != 0 {
        return Err("Unexpected project data".into());
    }
    let project = Project {
        document: manifest.document,
        assets,
    };
    project.validate(limits)?;
    Ok(project)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Project {
        let mut document = Document::new("raster fixture", 512, 256);
        let tile = RasterTile::backed(
            TileBlob::encode(
                color::PixelDescriptor::SRGB8_PAINT,
                &(0..crate::color::PixelDescriptor::SRGB8_PAINT.byte_len([TILE_SIZE; 2]).unwrap())
                    .map(|i| (i % 251) as u8)
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        );
        document.layers[0].raster = RasterRevision::backed(RasterData {
            tiles: BTreeMap::from([
                (
                    TileKey {
                        plane: RasterPlane::Color,
                        coordinate: [0, 0],
                    },
                    tile.clone(),
                ),
                (
                    TileKey {
                        plane: RasterPlane::Color,
                        coordinate: [1, 0],
                    },
                    tile,
                ),
            ]),
            watercolor: None,
        });
        Project {
            document,
            assets: BTreeMap::new(),
        }
    }
    #[test]
    fn indexed_raster_roundtrip_deduplicates_and_reuses_backing() {
        let project = fixture();
        let mut bytes = Vec::new();
        project.write(&mut bytes).unwrap();
        let loaded = Project::read(bytes.as_slice(), Default::default()).unwrap();
        assert_eq!(loaded.document.color, project.document.color);
        let a = project.document.layers[0].raster.wait_data().unwrap();
        let b = loaded.document.layers[0].raster.wait_data().unwrap();
        assert_eq!(a.tiles.len(), b.tiles.len());
        for (key, tile) in &a.tiles {
            assert_eq!(
                tile.wait_backing().unwrap().decode().unwrap(),
                b.tiles[key].wait_backing().unwrap().decode().unwrap()
            );
        }
        let tiles: Vec<_> = b.tiles.values().collect();
        assert!(tiles[0].same_capture(tiles[1]));
        let mut second = Vec::new();
        loaded.write(&mut second).unwrap();
        assert_eq!(bytes, second, "repeat save reuses exact compressed chunks");
    }
    #[test]
    fn indexed_raster_rejects_corruption_and_instance_budget_bypass() {
        let mut bytes = Vec::new();
        fixture().write(&mut bytes).unwrap();
        let end = bytes.len();
        for index in [0, 12, 20, 52, end - 1] {
            let mut corrupt = bytes.clone();
            corrupt[index] ^= 1;
            assert!(Project::read(corrupt.as_slice(), Default::default()).is_err());
        }
        assert!(
            Project::read(
                bytes.as_slice(),
                ProjectLimits {
                    raster_bytes: crate::color::PixelDescriptor::SRGB8_PAINT.byte_len([TILE_SIZE; 2]).unwrap() as u64,
                    ..Default::default()
                }
            )
            .is_err(),
            "two references to one blob still occupy two physical tiles"
        );
        assert!(
            Project::read(
                bytes.as_slice(),
                ProjectLimits {
                    tiles: 1,
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert!(
            Project::read(&b"CAPYPROJECT\x01"[..], Default::default())
                .unwrap_err()
                .contains("Unsupported")
        );
    }
}
