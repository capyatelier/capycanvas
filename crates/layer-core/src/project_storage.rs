//! Indexed project transport. Metadata and each compressed tile have independent
//! integrity checks; stream offsets are relative to the end of the manifest.
use super::project::{AssetRecord, io_error, metadata, read_block, validate_document};
use crate::{raster::*, *};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
mod sources;
use sources::SourceIndex;
#[cfg(test)]
mod native_color;

const MAGIC: &[u8; 12] = b"CAPYRASTER\x04\0";
// Older readers must reject placed artwork instead of silently ignoring its
// geometry. Continue writing v4 for documents needing no placement semantics.
const PLACEMENT_MAGIC: &[u8; 12] = b"CAPYRASTER\x05\0";

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
    tiled_sources: SourceIndex,
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
            data.validate(
                layer.local_extent([project.document.width, project.document.height]),
                mask,
                project.document.color,
            )?;
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
    let (mut tiled_sources, profiles) =
        SourceIndex::collect(&project.document, &mut blobs, &mut ids, &mut tile_count);
    if tile_count > limits.tiles {
        return Err("Project has too many raster/source tiles".into());
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
    tiled_sources.index_profiles(&profiles, &mut offset);
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
        tiled_sources,
    };
    let json = metadata(&manifest, limits.metadata_bytes)?;
    let canvas = [project.document.width, project.document.height];
    let placed = project.document.layers.iter().any(|l| l.properties.placement != Affine::IDENTITY || l.masks().any(|m| m.placement != Affine::IDENTITY))
        || manifest.rasters.iter().any(|r| r.tiles.iter().any(|t| {
            (0..2).any(|i| t.key.coordinate[i] >= canvas[i].div_ceil(TILE_SIZE))
        }));
    output.write_all(if placed { PLACEMENT_MAGIC } else { MAGIC }).map_err(io_error)?;
    output
        .write_all(&(json.len() as u64).to_le_bytes())
        .map_err(io_error)?;
    output.write_all(&Sha256::digest(&json)).map_err(io_error)?;
    output.write_all(&json).map_err(io_error)?;
    for blob in blobs {
        output.write_all(blob.compressed()).map_err(io_error)?;
    }
    for profile in profiles {
        output.write_all(&profile).map_err(io_error)?;
    }
    for source in project.assets.values() {
        output.write_all(&source.bytes).map_err(io_error)?;
    }
    Ok(())
}

pub(super) fn read(mut input: impl Read, limits: ProjectLimits) -> Result<Project, String> {
    let mut magic = [0; 12];
    input.read_exact(&mut magic).map_err(io_error)?;
    if &magic != MAGIC && &magic != PLACEMENT_MAGIC {
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
    for blob in &manifest.blobs {
        blob.descriptor
            .byte_len([TILE_SIZE; 2])
            .ok_or("Unsupported raster pixel descriptor")?;
        if blob.offset != offset || blob.size == 0 || blob.size > MAX_TILE_BYTES as u64 + 1024 {
            return Err("Invalid raster chunk index".into());
        }
        offset = offset
            .checked_add(blob.size)
            .ok_or("Raster index overflow")?;
    }
    let mut referenced = BTreeSet::new();
    let mut tile_count = 0;
    let mut source_bytes = manifest.tiled_sources.validate(
        &manifest.document,
        &manifest.blobs,
        limits,
        &mut offset,
        &mut referenced,
        &mut tile_count,
    )?;
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
    let mut target_ids = BTreeSet::new();
    let expected_targets: BTreeMap<_, _> = manifest
        .document
        .layers
        .iter()
        .flat_map(|l| std::iter::once((l.id, false)).chain(l.mask.iter().map(|m| (m.id, true))))
        .collect();
    let mut instance_bytes = 0u64;
    for raster in &manifest.rasters {
        let mask = *expected_targets
            .get(&raster.target)
            .ok_or("Missing raster target")?;
        if !target_ids.insert(raster.target) {
            return Err("Duplicate raster target".into());
        }
        let owner = manifest.document.target_owner(raster.target).ok_or("Missing raster owner")?;
        let canvas = [manifest.document.width, manifest.document.height];
        let extent = manifest.tiled_sources.extent(owner.id).map_or(canvas, |source| {
            std::array::from_fn(|i| canvas[i].max(source[i]))
        });
        let mut keys = BTreeSet::new();
        for tile in &raster.tiles {
            let blob = manifest.blobs.get(tile.blob).ok_or("Missing raster blob")?;
            if !keys.insert(tile.key)
                || !tile.key.plane.accepts_descriptor(manifest.document.color, blob.descriptor)
                || mask != (tile.key.plane == RasterPlane::Mask)
                || tile.key.coordinate[0] >= extent[0].div_ceil(TILE_SIZE)
                || tile.key.coordinate[1] >= extent[1].div_ceil(TILE_SIZE)
            {
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
    if target_ids.len() != expected_targets.len() {
        return Err("Project is missing a raster target".into());
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
    manifest
        .tiled_sources
        .read(&mut input, &tiles, &mut manifest.document)?;
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
        data.validate(
            manifest.document.target_extent(raster.target),
            mask,
            manifest.document.color,
        )?;
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
    use crate::color::{ColorProfile, IntegerDepth, source::*};

    fn source_fixture() -> Project {
        let mut project = fixture();
        let interpretation = SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: IntegerDepth::U16,
            // Transport treats ICC as exact opaque bytes; color services validate
            // its tags and supported CMM role before any renderer adopts it.
            profile: ColorProfile::Icc((0..256).map(|n| n as u8).collect::<Vec<_>>().into()),
            profile_assumed: false,
        };
        let mut builder = SourceBuilder::new([257, 259], interpretation, 8 * 1024 * 1024).unwrap();
        for y in 0..259u32 {
            let mut row = Vec::new();
            for x in 0..257u32 {
                for value in [
                    x.wrapping_mul(255) as u16,
                    y.wrapping_mul(253) as u16,
                    (x ^ y) as u16,
                    if x % 7 == 0 { 0 } else { 65535 },
                ] {
                    row.extend_from_slice(&value.to_le_bytes());
                }
            }
            builder.push_row(&row).unwrap();
        }
        let source = Arc::new(builder.finish().unwrap());
        project.document.layers[0].source = Some(source);
        let mut duplicate = project.document.layers[0].clone();
        duplicate.id = project.document.allocate_layer_id();
        project.document.layers.insert(0, duplicate);
        project
    }

    #[test]
    fn persistent_placement_preserves_sources_overrides_and_independent_history() {
        let mut project = source_fixture();
        for layer in &mut project.document.layers {
            layer.raster = Default::default();
            if let Some(mask) = &mut layer.mask { mask.raster = Default::default(); }
        }
        project.document.width = 32;
        project.document.height = 32;
        let id = project.document.layers[0].id;
        let original = project.document.layers[0].clone();
        let mut editor = Editor::new(project.document);
        let mut placed = original.clone();
        placed.properties.placement = Affine::around(
            Point::default(), [1. / 3.; 2], 0.3, Point { x: -45., y: 8. },
        );
        editor.perform(Edit::ReplaceLayer(Box::new(placed.clone()))).unwrap();
        assert!(editor.document().layer(id).unwrap().raster.is_empty());
        assert!(Arc::ptr_eq(editor.document().layer(id).unwrap().source.as_ref().unwrap(), original.source.as_ref().unwrap()));
        assert!(editor.undo().unwrap());
        assert_eq!(editor.document().layer(id).unwrap().properties.placement, Affine::IDENTITY);
        assert!(editor.redo().unwrap());
        project.document = editor.document().clone();
        let mut bytes = Vec::new();
        project.write(&mut bytes).unwrap();
        assert_eq!(&bytes[..12], PLACEMENT_MAGIC);
        let mut loaded = Project::read(bytes.as_slice(), Default::default()).unwrap();
        assert_eq!(loaded.document.layer(id).unwrap().properties, placed.properties);
        assert_eq!(loaded.document.layers[1].properties.placement, Affine::IDENTITY);
        assert_eq!(loaded.document.layer(id).unwrap().source, original.source);
        // A subsequent 100% placement still reads the original samples.
        loaded.document.layers[0].properties.placement = Affine::IDENTITY;
        assert_eq!(loaded.document.layers[0].source, original.source);
        // Editable source-local backing beyond the 32px canvas survives saving.
        let key = TileKey { plane: RasterPlane::Color, coordinate: [1, 0] };
        let descriptor = key.plane.descriptor(loaded.document.color);
        let blob = TileBlob::encode(descriptor, &vec![0; descriptor.byte_len([TILE_SIZE; 2]).unwrap()]).unwrap();
        let digest = blob.digest;
        loaded.document.layers[0].raster = RasterRevision::backed(RasterData {
            tiles: BTreeMap::from([(key, RasterTile::backed(blob))]), watercolor: None,
        });
        bytes.clear();
        loaded.write(&mut bytes).unwrap();
        let reopened = Project::read(bytes.as_slice(), Default::default()).unwrap();
        assert_eq!(reopened.document.layers[0].raster.wait_data().unwrap().tiles[&key].wait_backing().unwrap().digest, digest);
    }

    #[test]
    fn native_sources_preserve_u16_profiles_hidden_rgb_and_shared_ownership() {
        let project = source_fixture();
        let snapshot = Project::snapshot(&project.document, &project.assets).unwrap();
        assert!(Arc::ptr_eq(
            snapshot.document.layers[0].source.as_ref().unwrap(),
            project.document.layers[0].source.as_ref().unwrap()
        ));
        let mut bytes = Vec::new();
        snapshot.write(&mut bytes).unwrap();
        let loaded = Project::read(bytes.as_slice(), Default::default()).unwrap();
        let a = loaded.document.layers[0].source.as_ref().unwrap();
        let b = loaded.document.layers[1].source.as_ref().unwrap();
        assert!(
            Arc::ptr_eq(a, b),
            "duplicated layers share a source after reopening"
        );
        let original = project.document.layers[0].source.as_ref().unwrap();
        assert_eq!(a, original);
        for (key, tile) in &a.tiles {
            assert_eq!(
                tile.decode().unwrap(),
                original.tiles[key].decode().unwrap()
            );
        }
        let mut second = Vec::new();
        loaded.write(&mut second).unwrap();
        assert_eq!(
            bytes, second,
            "repeat saves reuse exact source/profile payloads"
        );

        let weak = Arc::downgrade(a);
        let mut editor = Editor::new(loaded.document);
        for id in [LayerId(1), LayerId(3)] {
            let mut layer = editor.document().layer(id).unwrap().clone();
            layer.source = None;
            editor.perform(Edit::ReplaceLayer(Box::new(layer))).unwrap();
        }
        assert!(editor.undo().unwrap());
        assert!(Arc::ptr_eq(
            editor
                .document()
                .layer(LayerId(3))
                .unwrap()
                .source
                .as_ref()
                .unwrap(),
            &weak.upgrade().unwrap()
        ));
        assert!(editor.redo().unwrap());
        assert!(weak.upgrade().is_some(), "history retains original source");
        editor.clear_history();
        assert!(
            weak.upgrade().is_none(),
            "cleared history releases an unused source"
        );
    }

    fn rewrite_manifest(bytes: &[u8], edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
        let length = u64::from_le_bytes(bytes[12..20].try_into().unwrap()) as usize;
        let mut value = serde_json::from_slice(&bytes[52..52 + length]).unwrap();
        edit(&mut value);
        let json = serde_json::to_vec(&value).unwrap();
        let mut changed = MAGIC.to_vec();
        changed.extend_from_slice(&(json.len() as u64).to_le_bytes());
        changed.extend_from_slice(&Sha256::digest(&json));
        changed.extend_from_slice(&json);
        changed.extend_from_slice(&bytes[52 + length..]);
        changed
    }

    #[test]
    fn proof_metadata_rejects_invalid_policy_and_profile_references_before_payloads() {
        use crate::color::{ColorProfile, ProofRecipe};
        let mut project = source_fixture();
        project.document.proof = Some(ProofRecipe::new("Lab paper".into(),
            ColorProfile::Icc(vec![19; 1024].into())));
        let mut bytes = Vec::new();
        project.write(&mut bytes).unwrap();
        for mutate in [
            |v: &mut serde_json::Value| v["tiled_sources"]["proof"]["profile"] = serde_json::json!({"Embedded": 999}),
            |v: &mut serde_json::Value| v["tiled_sources"]["proof"]["name"] = "".into(),
            |v: &mut serde_json::Value| v["tiled_sources"]["proof"]["name"] = "x".repeat(1025).into(),
            |v: &mut serde_json::Value| {
                v["tiled_sources"]["proof"]["simulate_paper"] = true.into();
                v["tiled_sources"]["proof"]["simulate_black_ink"] = false.into();
            },
            |v: &mut serde_json::Value| {
                v["tiled_sources"]["proof"]["conversion"]["intent"] = "AbsoluteColorimetric".into();
                v["tiled_sources"]["proof"]["conversion"]["black_point_compensation"] = true.into();
            },
        ] {
            let invalid = rewrite_manifest(&bytes, mutate);
            let length = u64::from_le_bytes(invalid[12..20].try_into().unwrap()) as usize;
            let error = Project::read(&invalid[..52 + length], Default::default()).unwrap_err();
            assert!(!error.contains("incomplete") && !error.contains("I/O"), "{error}");
        }
        let mut invalid = project.clone();
        invalid.document.proof.as_mut().unwrap().profile = ColorProfile::Icc(Arc::from([]));
        assert!(invalid.write(&mut Vec::new()).unwrap_err().contains("proof"));
    }

    #[test]
    fn rasterized_image_role_roundtrips_and_rejects_wrong_document_interpretation() {
        use crate::color::{ColorProfile, source::SourceKind};
        let mut project = source_fixture();
        let mut image = (**project.document.layers[0].source.as_ref().unwrap()).clone();
        image.kind = SourceKind::Rasterized;
        image.interpretation.profile = ColorProfile::Builtin(project.document.color.space);
        image.interpretation.profile_assumed = false;
        project.document.color.depth = image.interpretation.depth;
        for layer in &mut project.document.layers {
            layer.raster = Default::default();
            if let Some(mask) = &mut layer.mask { mask.raster = Default::default(); }
        }
        project.document.layers[0].source = Some(Arc::new(image.clone()));
        let mut bytes = Vec::new();
        project.write(&mut bytes).unwrap();
        assert_eq!(&bytes[..12], b"CAPYRASTER\x04\0");
        let loaded = Project::read(bytes.as_slice(), Default::default()).unwrap();
        assert_eq!(loaded.document.layers[0].source.as_deref(), Some(&image));
        assert!(loaded.document.layers[1].source.as_ref().unwrap().is_original());
        let mut repeated = Vec::new(); loaded.write(&mut repeated).unwrap();
        assert_eq!(bytes, repeated);
        let invalid = rewrite_manifest(&bytes, |v| v["tiled_sources"]["images"][0]["profile_assumed"] = true.into());
        let length = u64::from_le_bytes(invalid[12..20].try_into().unwrap()) as usize;
        assert!(Project::read(&invalid[..52 + length], Default::default()).unwrap_err().contains("interpretation differs"));
        let invalid = rewrite_manifest(&bytes, |v| v["tiled_sources"]["images"][0]["depth"] = "U8".into());
        assert!(Project::read(invalid.as_slice(), Default::default()).unwrap_err().contains("interpretation differs"));
        let mut obsolete = bytes.clone(); obsolete[10] = 3;
        assert!(Project::read(obsolete.as_slice(), Default::default()).unwrap_err().contains("Unsupported"));
    }

    #[test]
    fn source_indices_reject_aliases_corruption_and_budget_bypasses() {
        let mut bytes = Vec::new();
        source_fixture().write(&mut bytes).unwrap();
        for mutate in [
            |v: &mut serde_json::Value| {
                v["tiled_sources"]["layers"][1]["target"] =
                    v["tiled_sources"]["layers"][0]["target"].clone()
            },
            |v: &mut serde_json::Value| v["tiled_sources"]["layers"][0]["image"] = 999.into(),
            |v: &mut serde_json::Value| {
                v["tiled_sources"]["images"][0]["tiles"][1]["coordinate"] =
                    serde_json::json!([0, 0])
            },
            |v: &mut serde_json::Value| {
                v["tiled_sources"]["images"][0]["profile"] = serde_json::json!({"Embedded": 1})
            },
            |v: &mut serde_json::Value| v["tiled_sources"]["profiles"][0]["offset"] = 0.into(),
        ] {
            let changed = rewrite_manifest(&bytes, mutate);
            // Supply only metadata. A bad index must fail before requesting any
            // payload, rather than relying on a later decompression failure.
            let length = u64::from_le_bytes(changed[12..20].try_into().unwrap()) as usize;
            let error = Project::read(&changed[..52 + length], Default::default()).unwrap_err();
            assert!(
                !error.contains("incomplete") && !error.contains("I/O"),
                "{error}"
            );
        }
        let end = bytes.len();
        bytes[end - 1] ^= 1;
        assert!(
            Project::read(bytes.as_slice(), Default::default())
                .unwrap_err()
                .contains("profile integrity")
        );
        bytes[end - 1] ^= 1;
        for limits in [
            ProjectLimits {
                asset_bytes: 128,
                ..Default::default()
            },
            ProjectLimits {
                tiles: 5,
                ..Default::default()
            },
            ProjectLimits {
                dimension: 256,
                ..Default::default()
            },
        ] {
            assert!(Project::read(bytes.as_slice(), limits).is_err());
        }
    }
    fn fixture() -> Project {
        let mut document = Document::new("raster fixture", 512, 256);
        let tile = RasterTile::backed(
            TileBlob::encode(
                color::PixelDescriptor::SRGB8_PAINT,
                &(0..crate::color::PixelDescriptor::SRGB8_PAINT
                    .byte_len([TILE_SIZE; 2])
                    .unwrap())
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
                    raster_bytes: crate::color::PixelDescriptor::SRGB8_PAINT
                        .byte_len([TILE_SIZE; 2])
                        .unwrap() as u64,
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
